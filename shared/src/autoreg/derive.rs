//! Check-in → derived readiness signals.
//!
//! The user reports raw human observations (sleep/soreness/mood 1–5, optional
//! resting HR bpm, optional HRV rMSSD ms). The core, not the user, computes the
//! rolling baselines, z-scores, deltas and streaks the autoregulation rules
//! consume. Derivation changes no threshold: it only feeds the existing rules
//! synthetic [`ReadinessInput`]s, so every emitted judgement still travels with
//! the rule's own registry evidence via the normal path.
//!
//! Pure + deterministic: time enters only through each check-in's `observed_at`
//! (day-bucketed); a bad or non-positive stamp is skipped, never a panic.

use super::prescriptions::suspend_hrv_gating;
use super::rules::{
    CHECKIN_RECENCY_DAYS, HRV_MIN_WEEKLY_READINGS, HRV_ROLLING_DAYS, HRV_SWC_LOWER_Z, RhrNoise,
    WELLNESS_MULTIDAY_Z, WELLNESS_SINGLE_DAY_Z, rhr_bands,
};
use crate::schema::{CheckinInput, ReadinessInput, ReadinessSignal};

// Day-bucketing note. Check-ins bucket by LOCAL calendar day (`crate::local_day`),
// with the offset threaded from the call site
// (`derive_readiness(&checkins, offset, today)` in `app::readiness`) exactly like
// `session_logged` / `build_run_anchors`. This stops a late-evening +
// next-morning LOCAL pair (e.g. 23:30 then 07:00 Berlin) from collapsing into
// one UTC bucket and silently breaking a multi-day streak. Offset 0 is UTC
// bucketing.
//
// Rule-4 compaction safety (log.rs): `compact_event_log` drops a `SubmitCheckin`
// only when its raw `observed_at < anchor − RETAIN_CHECKIN_DAYS` (45 days),
// where `anchor` is the MIN over present channels of that channel's newest raw
// `observed_at`. That cutoff is computed on RAW UTC seconds, independent of
// `utc_offset_sec`. Derivation reads a day only when `newest_day − day < 30`
// (`LONG_BASELINE_WINDOW_DAYS`, the widest channel window); applying the SAME
// offset to both `newest_day` and `day` cancels in their difference except for a
// floor-rounding boundary, so the offset can widen the window's RAW-second reach
// by at most one day (≈30→≈31 days back from the newest reading). A line old
// enough to have been compacted away can therefore never re-enter the
// derivation window; log.rs asserts `RETAIN_CHECKIN_DAYS > 30 + 1` at compile
// time.

/// Minimum number of distinct check-in days before a rolling baseline is
/// trustworthy enough to emit a z-score / delta for the wellness composite and
/// resting-HR channels. Below this the core reports an honest "collecting
/// baseline" state instead of a fabricated number (HARD RULE 1). It is about the
/// width a wellness SWC band (baseline ± 0.5 SD) needs — roughly a week of
/// readings. HRV is held to a stricter, longer maturity gate: see
/// `HRV_BASELINE_CALENDAR_DAYS` and `HRV_MIN_BASELINE_READING_DAYS`.
pub(super) const MIN_BASELINE_CHECKINS: usize = 7;

/// Calendar span (LOCAL days) the `HrvLnRmssd` baseline must cover before it
/// emits a z: the newest reading must sit at least this many days after the
/// first reading in the window. The KB puts the lnRMSSD baseline at "≥3–4 weeks
/// to trust" (06-autoregulation.md, autoreg-047); 21 days is the low end.
const HRV_BASELINE_CALENDAR_DAYS: i64 = 21;

/// Distinct reading-days the `HrvLnRmssd` baseline needs inside its 30-day
/// window before it emits: 4 readings/week × 3 weeks, the KB's minimum cadence
/// over the minimum span (autoreg-047). Below this the channel reports an honest
/// "collecting (have/need)" status (HARD RULE 1) rather than a not-yet-trustworthy
/// number. The 7-day rolling *window* ([`HRV_ROLLING_DAYS`]) is unchanged; this
/// is the baseline-maturity gate, not the averaging window.
const HRV_MIN_BASELINE_READING_DAYS: usize = 12;

/// Trailing window (days) the wellness / soreness rolling baselines span, back
/// from the newest reading (deterministic; no clock). The KB puts the subjective
/// wellness baseline at a rolling 7–14-day window; 14 is its upper bound. The
/// boundary is EXCLUSIVE (`newest − day < N`), i.e. 14 inclusive days. HRV and
/// resting-HR need a longer individual baseline — see
/// [`LONG_BASELINE_WINDOW_DAYS`].
const BASELINE_WINDOW_DAYS: i64 = 14;

/// Trailing window (days) for the HRV and resting-HR baselines. HRV needs a
/// 3–4-week baseline to trust ([`HRV_MIN_BASELINE_READING_DAYS`] reading-days must fit
/// inside it), and resting HR wants a wide, stable individual baseline so a
/// chronic elevation cannot creep into its own reference. Same exclusive
/// boundary as [`BASELINE_WINDOW_DAYS`]; this is the widest derivation reach and
/// governs the compaction-retention margin above.
pub(crate) const LONG_BASELINE_WINDOW_DAYS: i64 = 30;

/// Recency window (LOCAL days) for the soreness endpoint bridge: the maxed-sore
/// downgrade fires only when the newest soreness reading falls within this many
/// days of the GLOBAL newest check-in day (any channel), using the same
/// local-day math as [`per_day_series`].
///
/// DOMS is transient: muscle soreness peaks 24–72 h post-exercise and then
/// resolves, so an old maxed-sore reading must NOT downgrade sessions
/// indefinitely. Unlike the wellness/HRV/RHR channels, the bridge is a
/// single-reading gate with no baseline minimum, so without a recency bound one
/// sore morning would persist forever.
///
/// It also restores compaction replay-equivalence. Compaction Rule 4
/// (`log.rs`; its cutoff mechanics are spelled out in the day-bucketing note) can
/// push the folded "wellness" anchor forward off late sleep-only check-ins and
/// evict an old soreness line the uncompacted bridge would still read. Anchoring
/// THIS gate on the GLOBAL newest check-in
/// closes that gap: `anchor ≤ global_newest` always, and passing the gate means
/// `global_newest − soreness_day ≤ 2` local days, i.e. `< 3` days of raw
/// separation (the offset cancels in the difference), so
/// `anchor − soreness_day < 3 < 45` days → the reading is provably inside Rule
/// 4's window and survives. Equivalently, any soreness line old enough for Rule
/// 4 to drop is already too old to pass this gate, so compacted and uncompacted
/// replays agree.
pub(super) const SORENESS_RECENCY_DAYS: i64 = 2;

/// Recency window (LOCAL days) for the HRV rolling verdict: the newest HRV
/// reading must fall within this many days of the GLOBAL newest check-in (any
/// channel), or the channel emits nothing. Without it a folded HRV baseline
/// would keep driving the autoreg-028 gate off readings the user stopped taking
/// weeks ago; a verdict must not outlive its data.
pub(super) const HRV_RECENCY_DAYS: i64 = 7;

/// Absolute floor for the wellness composite (1–5 goodness scale, 5 = best): a
/// morning at/below this, e.g. sleep 1 / soreness 5 / mood 1 → composite 1.0,
/// is catastrophic on its face and downgrades intensity regardless of the
/// z-score. It closes the flat-baseline blind spot: 7 identical days give
/// SD ≈ 0 → z 0 "normal", so a z-only rule can never see the dip. It covers the
/// all-items-bad morning; the soreness-only morning (maxed soreness but decent
/// sleep/mood, composite still above the floor) is handled by the soreness
/// endpoint bridge in [`derive_readiness`], not here.
///
/// Heuristic parameter (no KB entry sets an absolute composite floor): the
/// emitted signal is an ordinary `WellnessZ` input, so the downgrade still
/// travels with the wellness rule's own `WELLNESS-001` evidence, no invented
/// citation. The constant itself is expert-opinion, following the
/// AUTOREG-EASY-CAP precedent for a KB-silent number. Conservative direction
/// (HARD RULE 3): it can only ADD a downgrade, never remove one or add load.
const WELLNESS_ABS_FLOOR: f64 = 2.0;

/// Normalized value forced onto a catastrophic-floor reading: the single-day
/// wellness downgrade trigger (`WellnessZ ≤ −1.5`, autoreg-030), so a floored
/// morning drives the SAME downgrade a genuine z ≤ −1.5 would, no new rule.
const WELLNESS_FLOOR_Z: f64 = WELLNESS_SINGLE_DAY_Z;

/// How the core normalizes a channel's raw daily value against its baseline.
#[derive(Clone, Copy)]
enum Normalize {
    /// z-score = (today − baseline mean) / baseline population SD (HRV).
    Z,
    /// z-score against the SAMPLE SD (n − 1) floored at `sd_floor` (the
    /// wellness composite, ready-upd-007 / WELLNESS-Z-001): a consistent
    /// responder's near-zero spread must never turn one one-step answer change
    /// into a ≥1.5 SD drop.
    ZSample { sd_floor: f64 },
    /// signed delta = today − baseline mean (resting-HR bpm over baseline).
    DeltaFromMean,
}

/// Wellness-composite SD floor, composite points on the 1–5 mean scale
/// (ready-upd-007, WELLNESS-Z-001; ExpertOpinion good-practice parameter): one
/// item moving one step moves a 5-item composite by 0.2 → z ≥ −0.8, never a
/// single-day downgrade on its own.
const WELLNESS_SD_FLOOR: f64 = 0.25;

/// Baseline SD from a sum of squared deviations over `count` readings, per
/// normalization mode.
fn mode_sd(mode: Normalize, sq_sum: f64, count: f64) -> f64 {
    match mode {
        Normalize::Z => (sq_sum / count).sqrt(),
        Normalize::ZSample { sd_floor } => {
            let sd = if count > 1.0 {
                (sq_sum / (count - 1.0)).sqrt()
            } else {
                0.0
            };
            sd.max(sd_floor)
        }
        Normalize::DeltaFromMean => 0.0,
    }
}

/// How [`derive_channel`] collapses its per-day normalized series into the one
/// value it emits.
#[derive(Clone, Copy)]
enum EmitValue {
    /// Emit the most recent day's (streak-aware) normalized value.
    LastDay,
    /// Emit the mean of the fresh per-day z-scores over the trailing N local
    /// days (`newest_day − day < N`): the HRV rolling window (autoreg-028).
    TrailingMeanDays(i64),
}

/// Whether a [`BaselineStatus`] row is still gathering its first baseline or is
/// a temporary sparse-week gap in an otherwise-established channel.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineKind {
    Collecting,
    SparseWeek,
    /// HRV gating suspended: ≥2 of the last 3 readings were unreliable
    /// (autoreg-049/050); the coach falls back to check-ins + logged sets until
    /// clean readings return.
    Suspended,
}

/// One channel that has some check-in data but not yet a trustworthy baseline -
/// surfaced honestly ("collecting your baseline") rather than as a fabricated z.
pub struct BaselineStatus {
    pub signal: ReadinessSignal,
    pub have: usize,
    pub need: usize,
    pub kind: BaselineKind,
}

/// Result of [`derive_readiness`]: synthetic `ReadinessInput`s to feed the
/// existing rules, the honest still-collecting status of any channel that has
/// readings but hasn't reached its baseline minimum, and `stale` — gated channels
/// whose newest reading is older than `CHECKIN_RECENCY_DAYS`. Stale inputs are
/// NOT fed to the rules (no verdict from an old check-in); the shell passes them
/// to [`signal_states_in`](super::signal_states_in) to render an honest "No
/// check-in today" row.
#[derive(Default)]
pub struct DerivedReadiness {
    pub inputs: Vec<ReadinessInput>,
    pub collecting: Vec<BaselineStatus>,
    pub stale: Vec<ReadinessInput>,
    /// `observed_at` of every HRV reading flagged unreliable (autoreg-049) and
    /// left out of the baseline and the rolling mean.
    pub hrv_unreliable: Vec<i64>,
    /// The individual resting-HR noise picture for the derived `RestingHr`
    /// input (ready-upd-002), fed to [`RuleContext::rhr`](super::RuleContext::rhr).
    pub rhr: Option<RhrNoise>,
}

/// Top of the 1–5 check-in item scale (5 = maxed).
const CHECKIN_ITEM_MAX: f64 = 5.0;

/// Clamp a 1–5 human scale item to its range as f64.
fn scale5(v: u8) -> f64 {
    (v as f64).clamp(1.0, 5.0)
}

/// Rescale a 1–5 check-in soreness item onto the KB's 7-point manual soreness
/// scale (autoreg-030): 1→1, 3→4, 4→5.5, 5→7, so a maxed 5/5 morning clears the
/// 7-point [`SORENESS_HIGH`](super::rules::SORENESS_HIGH) gate the manual
/// soreness input feeds. The raw value
/// is clamped to 1–5 first (a Hooper 1–7 value that leaks into the field can't
/// over-drive the gate).
fn checkin_soreness_to_seven_point(raw: f64) -> f64 {
    1.0 + (raw.clamp(1.0, 5.0) - 1.0) * 1.5
}

/// Wellness "goodness" for one check-in: the mean of the answered items of the
/// KB's 5-item short-form wellness instrument (`ReadinessSignal::WellnessZ`,
/// autoreg-030) on a 1–5 → 1–5 scale where 5 = best. Sleep and mood are scored
/// in the good direction; soreness, fatigue and stress are reverse-scored
/// (`6 − x`, so higher raw = worse → lower goodness), making the composite
/// uniformly "higher = better". `None` when no wellness item was answered.
///
/// # KB mapping / deliberate 1–5 scale
/// The KB `WellnessZ` entry (06-autoregulation.md §WellnessZ) accepts EITHER the
/// Hooper 4-item ×1–7 index OR a 5-item short-form (sleep, fatigue, soreness,
/// stress, mood/motivation) on 1–5 or 1–10. We implement the 1–5 short-form so
/// the five check-in rows share one identical scale widget. The absolute
/// magnitudes differ from a 1–7 Hooper sum, but the emitted signal is a z-score
/// against the user's OWN rolling baseline, so the compressed range only lowers
/// resolution — it does not change the z's meaning; direction-normalization is
/// preserved.
///
/// # Variable answered-item count (documented limitation)
/// The composite is a MEAN over answered items, so it always lands in [1, 5]
/// regardless of how many items (n = 1..5) the user filled — the baseline MEAN
/// stays scale-comparable across days. What varies is the per-day estimate
/// noise: a one-item morning is a noisier draw than a five-item one, which can
/// inflate the rolling SD. We deliberately do NOT drop low-n days behind a hard
/// "≥2 items" gate: a genuinely bad single item (sleep 1, fatigue 5, …) must
/// still be able to trip the absolute floor / z downgrade, and silencing it
/// would be the LESS-conservative call (HARD RULE 3). The residual baseline
/// noise is accepted in exchange for never dropping a safety-relevant reading.
pub(super) fn wellness_goodness(c: &CheckinInput) -> Option<f64> {
    let mut sum = 0.0;
    let mut n = 0.0;
    if let Some(s) = c.sleep_quality {
        sum += scale5(s); // higher = better
        n += 1.0;
    }
    if let Some(s) = c.soreness {
        sum += 6.0 - scale5(s); // higher soreness = worse → reverse-score
        n += 1.0;
    }
    if let Some(f) = c.fatigue {
        sum += 6.0 - scale5(f); // higher fatigue = worse → reverse-score
        n += 1.0;
    }
    if let Some(s) = c.stress {
        sum += 6.0 - scale5(s); // higher stress = worse → reverse-score
        n += 1.0;
    }
    if let Some(m) = c.mood {
        sum += scale5(m); // higher = better
        n += 1.0;
    }
    if n == 0.0 { None } else { Some(sum / n) }
}

/// The HRV channel's reading: a positive RMSSD (ms). Derivation and compaction
/// Rule 4 (`log.rs`, via [`has_hrv`]) must agree on which check-ins carry it.
fn hrv_reading(c: &CheckinInput) -> Option<f64> {
    c.hrv_rmssd_ms.filter(|v| *v > 0.0)
}

/// The resting-HR channel's reading: a positive bpm. Shared with compaction
/// Rule 4 via [`has_rhr`].
fn rhr_reading(c: &CheckinInput) -> Option<f64> {
    c.resting_hr_bpm.filter(|v| *v > 0.0)
}

/// Whether a check-in carries an HRV reading the derivation reads.
pub(crate) fn has_hrv(c: &CheckinInput) -> bool {
    hrv_reading(c).is_some()
}

/// Whether a check-in carries a resting-HR reading the derivation reads.
pub(crate) fn has_rhr(c: &CheckinInput) -> bool {
    rhr_reading(c).is_some()
}

/// Whether a check-in carries any wellness item (the composite and the soreness
/// bridge both read only these five answers).
pub(crate) fn has_wellness(c: &CheckinInput) -> bool {
    wellness_goodness(c).is_some()
}

/// One channel's reading for one local day: `(day, raw_value, observed_at)`.
type DaySample = (i64, f64, i64);

/// Latest raw value per LOCAL epoch-day for one channel, sorted ascending by day
/// and bounded to the trailing `window_days` of the newest reading (EXCLUSIVE:
/// `newest − day < window_days`, so `window_days` inclusive days). Each tuple is
/// `(day, raw_value, observed_at)`. `utc_offset_sec` shifts each stamp to the
/// device's local day before bucketing (see the day-bucketing note). Readings with
/// `observed_at <= 0` are skipped (they can't be day-bucketed; the skip is on the
/// RAW stamp, matching log.rs Rule 4); the most recent check-in per local day
/// wins so re-doing a check-in the same day overwrites, never double-counts.
fn per_day_series(
    checkins: &[CheckinInput],
    utc_offset_sec: i64,
    window_days: i64,
    extract: impl Fn(&CheckinInput) -> Option<f64>,
) -> Vec<DaySample> {
    use std::collections::BTreeMap;
    let mut by_day: BTreeMap<i64, (i64, f64)> = BTreeMap::new();
    for c in checkins {
        if c.observed_at <= 0 {
            continue;
        }
        let Some(v) = extract(c) else {
            continue;
        };
        let day = crate::local_day(c.observed_at, utc_offset_sec);
        let slot = by_day.entry(day).or_insert((i64::MIN, v));
        if c.observed_at >= slot.0 {
            *slot = (c.observed_at, v);
        }
    }
    let Some(&newest) = by_day.keys().max() else {
        return Vec::new();
    };
    by_day
        .into_iter()
        .filter(|(day, _)| newest - day < window_days)
        .map(|(day, (at, v))| (day, v, at))
        .collect()
}

/// When a channel emits: `need` distinct reading-days (wellness / RHR use
/// [`MIN_BASELINE_CHECKINS`]; HRV is held to the stricter
/// [`HRV_MIN_BASELINE_READING_DAYS`]); `min_span_days` additionally requires
/// the readings to span at least that many LOCAL days (newest − oldest), since
/// enough reading-days packed into too short a window is still maturing;
/// `recency = Some(today_local_day)` routes a newest reading older than
/// [`CHECKIN_RECENCY_DAYS`] to `stale` instead of `inputs` (`None` leaves the
/// channel ungated, e.g. HRV's trailing window).
#[derive(Clone, Copy)]
struct ChannelGate {
    need: usize,
    min_span_days: Option<i64>,
    recency: Option<i64>,
}

/// One derived channel: the signal it emits, how each day is normalized
/// against its baseline, the absolute floor that forces a raw reading at/below
/// it to [`WELLNESS_FLOOR_Z`] (the wellness composite only), how the per-day
/// series collapses into the emitted value, and when the channel may emit.
#[derive(Clone, Copy)]
struct ChannelSpec {
    signal: ReadinessSignal,
    mode: Normalize,
    abs_floor: Option<f64>,
    emit: EmitValue,
    gate: ChannelGate,
}

/// Largest day gap that still continues a streak: one missed morning does not
/// reset a multi-day run (the consecutive-day counts of autoreg-030/034/040).
const STREAK_MAX_DAY_GAP: i64 = 2;

/// A baseline SD below this is flat: a z-score against it is 0, not a blow-up.
const SD_EPSILON: f64 = 1e-6;

/// Derive one channel: below the minimum → a `collecting` status; at/above →
/// a synthetic `ReadinessInput` carrying the normalized value ([`EmitValue`])
/// plus a core-computed multi-day `streak` (consecutive calendar days the
/// rule's own predicate held, each measured against a baseline FROZEN at streak
/// entry). `streak_predicate` runs over the *normalized* value.
fn derive_channel(
    series: &[DaySample],
    spec: ChannelSpec,
    streak_predicate: impl Fn(f64) -> bool,
    out: &mut DerivedReadiness,
) {
    let Some(&(newest_day, newest_raw, newest_at)) = series.last() else {
        return;
    };
    let need = spec.gate.need;
    if series.len() < need {
        out.collecting.push(BaselineStatus {
            signal: spec.signal,
            have: series.len(),
            need,
            kind: BaselineKind::Collecting,
        });
        // A catastrophic morning at/below the absolute floor is plain on its
        // face and needs no baseline: it drives the single-day rule from the
        // very first check-in (conservative direction only).
        if spec.abs_floor.is_some_and(|floor| newest_raw <= floor) {
            let input = derived_input(spec.signal, WELLNESS_FLOOR_Z, newest_at, 0);
            route_by_recency(spec.gate.recency, newest_day, input, out);
        }
        return;
    }
    if let Some(span) = spec.gate.min_span_days {
        let newest = series.iter().map(|(d, _, _)| *d).max().unwrap_or(0);
        let oldest = series.iter().map(|(d, _, _)| *d).min().unwrap_or(0);
        if newest - oldest < span {
            out.collecting.push(BaselineStatus {
                signal: spec.signal,
                have: series.len().min(need),
                need,
                kind: BaselineKind::Collecting,
            });
            return;
        }
    }
    let scorer = Scorer {
        series,
        mode: spec.mode,
        abs_floor: spec.abs_floor,
    };
    let (day_values, streak) = streak_pass(&scorer, streak_predicate);
    let value = match spec.emit {
        EmitValue::LastDay => day_values[day_values.len() - 1],
        EmitValue::TrailingMeanDays(window) => {
            let (sum, count) = trailing_sum(series, &day_values, window);
            // autoreg-047: a weekly rolling baseline needs at least
            // HRV_MIN_WEEKLY_READINGS readings inside the trailing window; a
            // sparser week (e.g. one reading after a long gap) is untrustworthy,
            // so the channel emits nothing. The row must not vanish silently: an
            // honest "collecting (have/need)" status shows it is still gathering.
            if count < HRV_MIN_WEEKLY_READINGS {
                out.collecting.push(BaselineStatus {
                    signal: spec.signal,
                    have: count,
                    need: HRV_MIN_WEEKLY_READINGS,
                    kind: BaselineKind::SparseWeek,
                });
                return;
            }
            sum / count as f64
        }
    };
    let input = derived_input(spec.signal, value, newest_at, streak);
    route_by_recency(spec.gate.recency, newest_day, input, out);
}

/// A channel's per-day series with its normalization: the baselines each day
/// is scored against.
struct Scorer<'a> {
    series: &'a [DaySample],
    mode: Normalize,
    abs_floor: Option<f64>,
}

impl Scorer<'_> {
    /// Leave-one-out baseline `(mean, sd)`, the "fresh" baseline: every reading
    /// EXCEPT `skip`. An isolated day is scored against the rest of the window,
    /// removing the self-reference that would dilute its deviation.
    fn loo_stats(&self, skip: usize) -> (f64, f64) {
        let (mean, sq_sum, count) = mean_and_sq_dev(
            self.series
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != skip)
                .map(|(_, (_, v, _))| *v),
        );
        (mean, mode_sd(self.mode, sq_sum, count))
    }

    /// Pre-streak baseline `(mean, sd)` over the days STRICTLY BEFORE `idx`,
    /// frozen at streak entry so every in-streak day is measured against the
    /// baseline as it stood before the deviation began — a sustained shift can
    /// never absorb itself into its own baseline. A chronic RHR elevation
    /// therefore keeps arming the autoreg-040 downgrade and the autoreg-041
    /// stop band until the 30-day window contains only elevated days (~30
    /// days), after which the elevated level becomes the reference (the
    /// corroboration the ≥2-day rules exist for). `idx == 0` (first-day streak,
    /// no prior data) falls back to the leave-one-out baseline.
    fn prefix_stats(&self, idx: usize) -> (f64, f64) {
        if idx == 0 {
            return self.loo_stats(0);
        }
        let (mean, sq_sum, count) = mean_and_sq_dev(self.series[..idx].iter().map(|(_, v, _)| *v));
        (mean, mode_sd(self.mode, sq_sum, count))
    }

    /// The reading at `idx` normalized against `(mean, sd)`, then the wellness
    /// absolute-floor rung.
    fn normalize(&self, idx: usize, (mean, sd): (f64, f64)) -> f64 {
        let raw = self.series[idx].1;
        let base = match self.mode {
            Normalize::DeltaFromMean => raw - mean,
            Normalize::Z | Normalize::ZSample { .. } if sd < SD_EPSILON => 0.0,
            Normalize::Z | Normalize::ZSample { .. } => (raw - mean) / sd,
        };
        match self.abs_floor {
            Some(floor) if raw <= floor => base.min(WELLNESS_FLOOR_Z),
            _ => base,
        }
    }
}

/// Forward chronological pass: every day's normalized value and the final
/// consecutive-suppressed-day streak.
///
/// The streak test scores EVERY day against a PAST-ONLY baseline (frozen at
/// streak entry, [`Scorer::prefix_stats`] for the entry day itself) so a later
/// reading can never contaminate the entry decision — a sustained shift is
/// measured against the baseline as it stood before it began. A gap of up to
/// [`STREAK_MAX_DAY_GAP`] days does not reset the run. A day's value uses that
/// frozen past-only baseline while in a streak (so the shift can't dilute
/// itself) and the leave-one-out baseline otherwise (the strip's current-day
/// z). The trailing-mean emit reuses these per-day values, so a sustained
/// suppression is measured against the same frozen baseline there too, instead
/// of a full-window leave-one-out the other suppressed days have already
/// pulled down.
fn streak_pass(scorer: &Scorer, streak_predicate: impl Fn(f64) -> bool) -> (Vec<f64>, u8) {
    let mut streak_len: u8 = 0;
    let mut prev_day: i64 = 0;
    let mut frozen: Option<(f64, f64)> = None;
    // Pre-dip baseline retained across a short break so a streak that re-enters
    // right after a lone recovery day is still measured against the reference
    // frozen at the FIRST dip day, not one the dip has crept into.
    let mut carried: Option<((f64, f64), i64)> = None;
    let mut day_values = Vec::with_capacity(scorer.series.len());
    for (idx, &(day, _, _)) in scorer.series.iter().enumerate() {
        let consecutive = streak_len > 0 && day - prev_day <= STREAK_MAX_DAY_GAP;
        // The baseline this day is scored against while a streak is active (the
        // streak's `frozen` baseline after this day's update); `None` outside a
        // streak.
        let streak_base = if consecutive {
            let base = frozen.unwrap_or_else(|| scorer.prefix_stats(idx));
            if streak_predicate(scorer.normalize(idx, base)) {
                streak_len = streak_len.saturating_add(1);
                Some(base)
            } else if streak_predicate(scorer.normalize(idx, scorer.prefix_stats(idx))) {
                // The frozen baseline does not flag this day, but a fresh
                // past-only deviation opens a new streak.
                streak_len = 1;
                let fresh = scorer.prefix_stats(idx);
                frozen = Some(fresh);
                Some(fresh)
            } else {
                // Streak ends: retain its pre-dip baseline for a quick re-entry.
                carried = frozen.map(|b| (b, prev_day));
                streak_len = 0;
                frozen = None;
                None
            }
        } else {
            // Streak entry (or re-entry): a re-entry within the gap of the last
            // dip day reuses that dip's frozen pre-dip baseline instead of one
            // the intervening days have moved.
            let base = match carried {
                Some((b, last)) if day - last <= STREAK_MAX_DAY_GAP => b,
                _ => scorer.prefix_stats(idx),
            };
            if streak_predicate(scorer.normalize(idx, base)) {
                streak_len = 1;
                frozen = Some(base);
                carried = None;
                Some(base)
            } else {
                carried = None;
                streak_len = 0;
                frozen = None;
                None
            }
        };
        let base = streak_base.unwrap_or_else(|| scorer.loo_stats(idx));
        day_values.push(scorer.normalize(idx, base));
        prev_day = day;
    }
    (day_values, streak_len)
}

/// Sum and count of the per-day values over the trailing `window` local days
/// (`newest_day − day < window`), summed in day order.
fn trailing_sum(series: &[DaySample], day_values: &[f64], window: i64) -> (f64, usize) {
    let newest = series[series.len() - 1].0;
    series
        .iter()
        .zip(day_values)
        .filter(|((day, _, _), _)| newest - day < window)
        .fold((0.0, 0), |(sum, count), (_, v)| (sum + v, count + 1))
}

/// A core-derived reading: no pain detail, no effort duration.
fn derived_input(
    signal: ReadinessSignal,
    value: f64,
    observed_at: i64,
    streak: u8,
) -> ReadinessInput {
    ReadinessInput {
        signal,
        value,
        observed_at,
        streak,
        pain: None,
        effort_min: None,
    }
}

/// Route a derived reading: a gated channel (`recency` = `Some(today)`) whose
/// newest reading (local `day`) predates the [`CHECKIN_RECENCY_DAYS`] window is
/// set aside as `stale` (surfaced as "out of date") rather than fed to the
/// rules; an ungated channel (`None`, e.g. HRV's trailing window) always emits.
fn route_by_recency(
    recency: Option<i64>,
    day: i64,
    input: ReadinessInput,
    out: &mut DerivedReadiness,
) {
    match recency {
        Some(today) if day < today - CHECKIN_RECENCY_DAYS => out.stale.push(input),
        _ => out.inputs.push(input),
    }
}

/// The check-in history one derivation reads, in its local-day frame.
struct History<'a> {
    checkins: &'a [CheckinInput],
    utc_offset_sec: i64,
    /// The shell's local today, when it has a clock.
    today_epoch_day: Option<i64>,
    /// Newest local check-in day over every channel.
    global_newest_day: Option<i64>,
}

impl History<'_> {
    fn series(
        &self,
        window_days: i64,
        extract: impl Fn(&CheckinInput) -> Option<f64>,
    ) -> Vec<DaySample> {
        per_day_series(self.checkins, self.utc_offset_sec, window_days, extract)
    }

    /// Whether a verdict from a reading on local `day` is still current: within
    /// `days` of the global newest check-in AND, when the shell gave a clock, of
    /// today. The global bound alone never expires a verdict whose reading IS
    /// the newest check-in (the user stopped checking in, so `g − day == 0`
    /// forever); the today bound lets it go quiet once the window elapses. It is
    /// strictly narrower than the global bound, so compaction Rule 4
    /// replay-equivalence is intact.
    fn recent(&self, day: i64, days: i64) -> bool {
        self.global_newest_day.is_some_and(|g| g - day <= days)
            && self.today_epoch_day.is_none_or(|t| t - day <= days)
    }
}

/// Normalize a check-in history into the synthetic readiness signals the
/// autoregulation rules consume:
/// - `WellnessZ`: composite z of the answered 1–5 items (autoreg-030).
/// - `Soreness`: a maxed check-in soreness item on the 7-point scale.
/// - `HrvLnRmssd`: z of `ln(rMSSD)` vs baseline (autoreg-028).
/// - `RestingHr`: bpm delta vs baseline mean (autoreg-040/041).
///
/// Streaks are core-computed. Below its baseline minimum a channel emits
/// nothing and reports a `collecting` status instead of a fabricated number.
///
/// `utc_offset_sec` is the device's local UTC offset (the shell's
/// `today_utc_offset_sec`): check-ins bucket by LOCAL calendar day so a
/// late-evening + next-morning pair spans two days and keeps a streak intact
/// (see the day-bucketing note). Offset 0 buckets by UTC day.
///
/// `today_epoch_day` (the shell's `model.today_epoch_day`, when known) recency-
/// gates the derived RHR-delta and wellness-z channels: a channel whose newest
/// reading is older than `CHECKIN_RECENCY_DAYS` is routed to `stale` (surfaced
/// as "out of date") instead of driving a verdict. HRV is not routed
/// through this per-reading gate — it reads a trailing rolling window and instead
/// bounds its own verdict by `HRV_RECENCY_DAYS` against today (and the newest
/// check-in), so a folded HRV verdict cannot outlive its data either. `None`
/// disables the gate (a shell without a clock).
pub fn derive_readiness(
    checkins: &[CheckinInput],
    utc_offset_sec: i64,
    today_epoch_day: Option<i64>,
) -> DerivedReadiness {
    let history = History {
        checkins,
        utc_offset_sec,
        today_epoch_day,
        global_newest_day: checkins
            .iter()
            .filter(|c| c.observed_at > 0)
            .map(|c| crate::local_day(c.observed_at, utc_offset_sec))
            .max(),
    };
    let mut out = DerivedReadiness::default();
    derive_wellness(&history, &mut out);
    soreness_bridge(&history, &mut out);
    derive_hrv(&history, &mut out);
    derive_rhr(&history, &mut out);
    out
}

/// Wellness composite (autoreg-030 / §5 tier 4): the streak counts
/// consecutive days at z ≤ −1. Recency-gated on today.
fn derive_wellness(history: &History, out: &mut DerivedReadiness) {
    derive_channel(
        &history.series(BASELINE_WINDOW_DAYS, wellness_goodness),
        ChannelSpec {
            signal: ReadinessSignal::WellnessZ,
            mode: Normalize::ZSample {
                sd_floor: WELLNESS_SD_FLOOR,
            },
            abs_floor: Some(WELLNESS_ABS_FLOOR),
            emit: EmitValue::LastDay,
            gate: ChannelGate {
                need: MIN_BASELINE_CHECKINS,
                min_span_days: None,
                recency: history.today_epoch_day,
            },
        },
        |z| z <= WELLNESS_MULTIDAY_Z,
        out,
    );
}

/// Soreness endpoint bridge: the check-in soreness item is the 1–5 instrument
/// (5 = worst). A maxed-sore morning surfaces as a `Soreness` reading rescaled
/// onto the KB's 7-point manual soreness scale
/// ([`checkin_soreness_to_seven_point`]) so a 5/5 check-in reaches the
/// 7-point `soreness_downgrade` gate
/// ([`SORENESS_HIGH`](super::rules::SORENESS_HIGH)). High check-in soreness
/// still lowers the wellness composite via reverse-scoring. Stamped at the
/// newest soreness reading; streak 0 (single-item gate).
///
/// Recency-gated (see [`SORENESS_RECENCY_DAYS`]), so a transient sore morning
/// cannot record indefinitely and the lookback stays inside compaction Rule 4's
/// wellness-retention window (replay-equivalent).
fn soreness_bridge(history: &History, out: &mut DerivedReadiness) {
    let series = history.series(BASELINE_WINDOW_DAYS, |c| c.soreness.map(f64::from));
    if let Some(&(day, raw, at)) = series.last()
        && raw >= CHECKIN_ITEM_MAX
        && history.recent(day, SORENESS_RECENCY_DAYS)
    {
        out.inputs.push(derived_input(
            ReadinessSignal::Soreness,
            checkin_soreness_to_seven_point(raw),
            at,
            0,
        ));
    }
}

/// HRV rolling baseline (autoreg-028): the mean of the trailing
/// [`HRV_ROLLING_DAYS`] local days' z-scores, so one off morning cannot trip
/// the SWC gate a sustained suppression should. The streak counts consecutive
/// suppressed days, arming autoreg-034's recovery helper. A verdict must not
/// outlive its data: past [`HRV_RECENCY_DAYS`] the channel emits nothing.
///
/// autoreg-049/050 artifact guard: a reading >3 SD off the rolling mean of the
/// prior clean readings that also lifts the week's variability is unreliable (a
/// mistyped or standing reading, a different device): it never enters the
/// baseline or the rolling mean. ≥2 of the last [`HRV_QC_LOOKBACK`] readings
/// unreliable suspends HRV gating until clean readings return.
fn derive_hrv(history: &History, out: &mut DerivedReadiness) {
    let series = history.series(LONG_BASELINE_WINDOW_DAYS, |c| hrv_reading(c).map(f64::ln));
    let flags = hrv_unreliable_flags(&series);
    out.hrv_unreliable = series
        .iter()
        .zip(&flags)
        .filter(|(_, f)| **f)
        .map(|((_, _, at), _)| *at)
        .collect();
    let recent = series
        .last()
        .is_some_and(|&(day, _, _)| history.recent(day, HRV_RECENCY_DAYS));
    if !recent {
        return;
    }
    let unreliable_recent = flags
        .iter()
        .rev()
        .take(HRV_QC_LOOKBACK)
        .filter(|f| **f)
        .count();
    if suspend_hrv_gating(unreliable_recent as u8) {
        out.collecting.push(BaselineStatus {
            signal: ReadinessSignal::HrvLnRmssd,
            have: unreliable_recent,
            need: HRV_QC_LOOKBACK,
            kind: BaselineKind::Suspended,
        });
        return;
    }
    let clean: Vec<DaySample> = series
        .iter()
        .zip(&flags)
        .filter(|(_, f)| !**f)
        .map(|(e, _)| *e)
        .collect();
    derive_channel(
        &clean,
        ChannelSpec {
            signal: ReadinessSignal::HrvLnRmssd,
            mode: Normalize::Z,
            abs_floor: None,
            emit: EmitValue::TrailingMeanDays(HRV_ROLLING_DAYS),
            gate: ChannelGate {
                need: HRV_MIN_BASELINE_READING_DAYS,
                min_span_days: Some(HRV_BASELINE_CALENDAR_DAYS),
                recency: None,
            },
        },
        |z| z < HRV_SWC_LOWER_Z,
        out,
    );
}

/// Resting-HR delta (autoreg-040/041 as amended by ready-upd-002): resting HR
/// varies ~10 % day to day, so both bands widen to the person's own spread
/// (downgrade max(5, 1 SD), stop max(10, 2 SD)). The streak counts consecutive
/// days at the downgrade band; a second pass counts consecutive stop-level
/// mornings, since a rest day needs ≥2 of them (or symptoms). Recency-gated on
/// today.
fn derive_rhr(history: &History, out: &mut DerivedReadiness) {
    let series = history.series(LONG_BASELINE_WINDOW_DAYS, rhr_reading);
    let sd_bpm = baseline_sd_excluding_newest(&series);
    let (down_bpm, stop_bpm) = rhr_bands(sd_bpm);
    let spec = ChannelSpec {
        signal: ReadinessSignal::RestingHr,
        mode: Normalize::DeltaFromMean,
        abs_floor: None,
        emit: EmitValue::LastDay,
        gate: ChannelGate {
            need: MIN_BASELINE_CHECKINS,
            min_span_days: None,
            recency: history.today_epoch_day,
        },
    };
    derive_channel(&series, spec, |d| d >= down_bpm, out);
    let derived_at = out
        .inputs
        .iter()
        .chain(&out.stale)
        .find(|i| i.signal == ReadinessSignal::RestingHr)
        .map(|i| i.observed_at);
    if let Some(observed_at) = derived_at {
        let mut stop_pass = DerivedReadiness::default();
        derive_channel(&series, spec, |d| d >= stop_bpm, &mut stop_pass);
        let stop_level_days = stop_pass
            .inputs
            .iter()
            .chain(&stop_pass.stale)
            .next()
            .map_or(0, |i| i.streak);
        out.rhr = Some(RhrNoise {
            sd_bpm,
            stop_level_days,
            observed_at,
        });
    }
}

/// Population SD of a channel's raw daily values, leaving out the newest day
/// (the reading being judged). 0 below two baseline readings.
fn baseline_sd_excluding_newest(series: &[DaySample]) -> f64 {
    let n = series.len().saturating_sub(1);
    if n < 2 {
        return 0.0;
    }
    let (_, sq_sum, count) = mean_and_sq_dev(series[..n].iter().map(|(_, v, _)| *v));
    (sq_sum / count).sqrt()
}

/// `(mean, Σ (v − mean)², count)` over `vals`, summed in order. The single
/// mean/spread computation behind every baseline SD and CV in this module.
fn mean_and_sq_dev(vals: impl Iterator<Item = f64> + Clone) -> (f64, f64, f64) {
    let count = vals.clone().count() as f64;
    let mean = vals.clone().sum::<f64>() / count;
    let sq_sum = vals.map(|v| (v - mean).powi(2)).sum::<f64>();
    (mean, sq_sum, count)
}

/// |z| above which an HRV reading is an outlier vs the rolling mean
/// (autoreg-049: "deviation >3 SD from rolling mean").
const HRV_OUTLIER_Z: f64 = 3.0;

/// Prior clean readings needed before the outlier test runs: the
/// [`MIN_BASELINE_CHECKINS`] week of readings a spread estimate needs.
const HRV_QC_MIN_PRIOR: usize = MIN_BASELINE_CHECKINS;

/// Most recent readings autoreg-050 inspects before suspending HRV gating
/// ("≥2 of the last 3 readings flagged").
const HRV_QC_LOOKBACK: usize = 3;

/// Coefficient of variation of raw rMSSD (ms) for ln-valued readings.
fn rmssd_cv(ln_values: &[f64]) -> f64 {
    if ln_values.len() < 2 {
        return 0.0;
    }
    let raw: Vec<f64> = ln_values.iter().map(|v| v.exp()).collect();
    let (mean, sq_sum, n) = mean_and_sq_dev(raw.iter().copied());
    if mean <= 0.0 {
        return 0.0;
    }
    (sq_sum / n).sqrt() / mean
}

/// autoreg-049 flags, aligned with `series` (ln rMSSD per local day). A
/// reading more than [`HRV_OUTLIER_Z`] SD from the mean of the prior CLEAN
/// readings is unreliable when either
/// - the trailing-7-day CV was ALREADY above the baseline's own CV before it
///   ("while CV already elevated"; the candidate itself is left out, or every
///   large reading would qualify), or
/// - the next reading does not reproduce it: an isolated spike that lifts the
///   week's CV is the artifact the rule describes.
///
/// A shift the next reading reproduces (also beyond [`HRV_OUTLIER_Z`] SD, same
/// side) is a real change (illness onset, acute overreaching; autoreg-028/034)
/// and is never flagged. Flagged readings are excluded from later baselines, so
/// one bad value cannot inflate the spread that would hide a real suppression
/// later. The newest reading, with no successor yet, is flagged only on the
/// already-elevated CV: unconfirmed, it counts (the conservative direction).
fn hrv_unreliable_flags(series: &[DaySample]) -> Vec<bool> {
    let mut flags = vec![false; series.len()];
    for idx in 0..series.len() {
        let prior: Vec<(i64, f64)> = series[..idx]
            .iter()
            .zip(&flags)
            .filter(|(_, f)| !**f)
            .map(|((d, v, _), _)| (*d, *v))
            .collect();
        if prior.len() < HRV_QC_MIN_PRIOR {
            continue;
        }
        let vals: Vec<f64> = prior.iter().map(|(_, v)| *v).collect();
        let (mean, sq_sum, n) = mean_and_sq_dev(vals.iter().copied());
        let sd = (sq_sum / n).sqrt().max(SD_EPSILON);
        let (day, v, _) = series[idx];
        let z = (v - mean) / sd;
        if z.abs() <= HRV_OUTLIER_Z {
            continue;
        }
        let next_z = series.get(idx + 1).map(|&(_, nv, _)| (nv - mean) / sd);
        if next_z.is_some_and(|nz| nz.abs() > HRV_OUTLIER_Z && nz.signum() == z.signum()) {
            continue;
        }
        let mut week: Vec<f64> = prior
            .iter()
            .filter(|(d, _)| day - d < HRV_ROLLING_DAYS)
            .map(|(_, v)| *v)
            .collect();
        let baseline_cv = rmssd_cv(&vals);
        let already_elevated = rmssd_cv(&week) > baseline_cv;
        week.push(v);
        let isolated_spike = next_z.is_some() && rmssd_cv(&week) > baseline_cv;
        if already_elevated || isolated_spike {
            flags[idx] = true;
        }
    }
    flags
}
