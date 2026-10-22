//! Readiness assembly helpers: performance-signal expiry, run-spike and
//! track-split signals, provisional/autoreg source rows, the Today headline,
//! check-in/baseline echoes, and the readiness signal groups.

use super::*;

/// Echo the most recent check-in for shell rehydration.
pub(super) fn to_checkin_echo(c: &CheckinInput) -> CheckinEchoView {
    CheckinEchoView {
        observed_at: c.observed_at,
        sleep_quality: c.sleep_quality,
        soreness: c.soreness,
        mood: c.mood,
        fatigue: c.fatigue,
        stress: c.stress,
        resting_hr_bpm: c.resting_hr_bpm,
        hrv_rmssd_ms: c.hrv_rmssd_ms,
    }
}

/// Human label for one still-collecting check-in channel. The shell phrases
/// the progress row itself from `kind`/`have`/`need`; no training claim
/// (HARD RULE 1/2).
pub(super) fn to_baseline_status_view(s: &autoreg::BaselineStatus) -> BaselineStatusView {
    let label = match s.signal {
        ReadinessSignal::WellnessZ => "Sleep, soreness, fatigue, stress & mood",
        ReadinessSignal::HrvLnRmssd => "HRV (rMSSD)",
        ReadinessSignal::RestingHr => "Resting HR",
        _ => "Readiness",
    };
    BaselineStatusView {
        signal: s.signal,
        label: label.to_string(),
        have: s.have as u32,
        need: s.need as u32,
        kind: s.kind,
    }
}

/// Second-half slowdown of the most recent GPS-tracked run, if any (the
/// feedback path reads it with the run's day and tag via
/// `feedback_track_split`).
#[cfg(test)]
pub(super) fn latest_track_split(model: &Model) -> Option<f64> {
    let facts = RunFacts::new(&model.runs);
    latest_tracked_run(model).and_then(|i| track_split_pct(model, &facts, i))
}

/// Run `i`'s second-half slowdown, read straight off its memoized run card:
/// the SAME QC'd track the row renders and the SAME
/// `track_positive_split_pct_seg` computation, so the coaching cue and the run
/// chip can never disagree. Only a run long enough for the card's own pacing
/// verdict (two full km splits) has one: a fragment's halves say nothing about
/// pacing. The measurement is descriptive (like pace or zone), so feeding it
/// to the resolver introduces no unevidenced claim.
pub(super) fn track_split_pct(model: &Model, facts: &RunFacts, i: usize) -> Option<f64> {
    let r = model.runs.get(i)?;
    let v = to_run_view(r, facts.keys[i], facts.qc[i].as_deref());
    v.split.and(v.split_pct)
}

/// Index of the chronologically latest tracked run (ties → most recently
/// logged), so a backdated GPS run never masquerades as "the latest session"
/// here. A FUTURE-dated import (a mis-timestamped 2030 GPX) is excluded so its
/// positive-split can't hijack "the most recent session" — the same guard the
/// weekly-report / CTL-ATL builders apply (`end_of_local_today_sec`); `None`
/// (no clock supplied) keeps the no-guard behavior.
pub(super) fn latest_tracked_run(model: &Model) -> Option<usize> {
    let cutoff = end_of_local_today_sec(model);
    model
        .runs
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.track.is_empty())
        .filter(|(_, r)| cutoff.is_none_or(|c| r.observed_at <= c))
        .max_by_key(|(i, r)| (r.observed_at, *i))
        .map(|(i, _)| i)
}

/// Readiness window (days) after which a performance signal that can drive a
/// load INCREASE is treated as stale and dropped. An engine heuristic (the KB
/// gives no expiry parameter), chosen conservatively (a fortnight). Only the
/// increase-driving signals expire (see [`is_expirable_perf_signal`]); protective
/// signals never do (HARD RULE 3).
pub(super) const PERF_SIGNAL_EXPIRY_DAYS: i64 = 14;

/// End-of-local-today boundary, unix seconds: a row with `observed_at`
/// beyond it is FUTURE-dated (a mis-imported 2030 GPX) and must not anchor
/// "current" windows (weekly-report cur-week, CTL/ATL last day). Same local-day
/// boundary the run anchors / `session_logged` use. `None` when the shell has not
/// supplied "today" (no clock in the core) → no guard.
pub(super) fn end_of_local_today_sec(model: &Model) -> Option<i64> {
    model.today_epoch_day.map(|d| {
        d.saturating_add(1)
            .saturating_mul(DAY_SEC)
            .saturating_sub(model.today_utc_offset_sec)
            .saturating_sub(1)
    })
}

/// Whether this performance-signal READING is stale-expirable: only when its
/// VALUE drives (or is adjacent to) a load INCREASE, so dropping it as stale is
/// CONSERVATIVE. The gate is direction-aware, not signal-type-only — the SAME
/// channel carries both directions, so a PROTECTIVE reading (a below-baseline
/// e1RM decline, ratio < 1.0, or an above-target RPE) is NEVER expired: dropping
/// it is anti-conservative and would silently expire a stale decline into
/// optimism (HARD RULE 3). `Rpe`/`EstimatedOneRm` are the two
/// `IncreaseLoadPct` producers; `BarVelocity` is the recorded VBT-increase input
/// with no protective gate of its own (the protective velocity signal is the
/// separate `VelocityLoss` channel, which is not expirable here). Every other
/// signal — the velocity-loss stop, aerobic decoupling, wellness/HRV/RHR
/// suppression, and all pain/illness/RED-S/soreness flags — is protective and
/// never reaches the expiry window.
pub(super) fn is_expirable_perf_signal(input: &ReadinessInput) -> bool {
    match input.signal {
        // RPE below target proposes an increase; on/above target holds or reduces.
        ReadinessSignal::Rpe => input.value < 0.0,
        // e1RM ratio above baseline proposes an increase; at/below baseline is
        // stable or a protective decline (reduce/deload) → keep.
        ReadinessSignal::EstimatedOneRm => input.value > 1.0,
        // Recorded-only VBT-increase input; no protective BarVelocity gate exists.
        ReadinessSignal::BarVelocity => true,
        _ => false,
    }
}

/// feedback-040 provisional framing: present until ~14 distinct dated days of
/// logged data (readiness, sets, runs) exist.
pub(super) fn build_provisional(model: &Model) -> Option<AdjustmentView> {
    use std::collections::BTreeSet;
    let offset = model.today_utc_offset_sec;
    let mut days: BTreeSet<i64> = model
        .inputs
        .iter()
        .map(|i| i.observed_at)
        .chain(model.sets.iter().map(|s| s.observed_at))
        .chain(model.runs.iter().map(|r| r.observed_at))
        .filter(|t| *t > 0)
        .map(|t| local_day(t, offset))
        .collect();
    // Count check-in days only within the baseline window of the NEWEST check-in.
    // A check-in older than that is compaction-dropped (log Rule 4, 45-day
    // retention) and never affects the derived baselines, so counting it would
    // make a full and a compacted log render a different provisional "N so far".
    const PROVISIONAL_CHECKIN_WINDOW_DAYS: i64 = 30;
    let newest_checkin_day = model
        .checkins
        .iter()
        .filter(|c| c.observed_at > 0)
        .map(|c| local_day(c.observed_at, offset))
        .max();
    for c in &model.checkins {
        if c.observed_at > 0 {
            let d = local_day(c.observed_at, offset);
            if newest_checkin_day.is_none_or(|n| n - d < PROVISIONAL_CHECKIN_WINDOW_DAYS) {
                days.insert(d);
            }
        }
    }
    let n = days.len().min(usize::from(u16::MAX)) as u16;
    let prov = feedback::provisional_until_baseline(n);
    prov.value.then(|| {
        // One plain line: the loads are typical starting values that adapt.
        let text = if n == 0 {
            "Starting weights are typical values; they adapt as you log.".to_string()
        } else {
            format!(
                "Starting weights are typical values; they adapt as you log (day {n} of about {}).",
                feedback::PROVISIONAL_BASELINE_DAYS
            )
        };
        to_view_with(text, &prov)
    })
}

/// Which autoregulation signal source is active (autoreg-047/048): HRV rolling
/// when HRV data is usable, else subjective + performance, else
/// performance-only with loads held. `None` until any readiness input exists.
/// `hrv_suspended` = the HRV channel is suspended by the artifact guard
/// (autoreg-049/050), which falls back to check-ins + logged sets.
pub(super) fn build_autoreg_source(
    inputs: &[ReadinessInput],
    hrv_baseline_active: bool,
    hrv_suspended: bool,
    offset: i64,
) -> Option<AdjustmentView> {
    if inputs.is_empty() && !hrv_suspended {
        return None;
    }
    let latest_day = inputs
        .iter()
        .map(|i| local_day(i.observed_at, offset))
        .max()
        .unwrap_or(0);
    let hrv_today = inputs.iter().any(|i| {
        i.signal == ReadinessSignal::HrvLnRmssd && local_day(i.observed_at, offset) == latest_day
    });
    let recent_hrv = inputs
        .iter()
        .filter(|i| {
            i.signal == ReadinessSignal::HrvLnRmssd
                && local_day(i.observed_at, offset) >= latest_day - 7
        })
        .count()
        .min(255) as u8;
    let subjective = inputs.iter().any(|i| {
        matches!(
            i.signal,
            ReadinessSignal::WellnessZ | ReadinessSignal::Soreness
        )
    });
    // The derived HRV channel collapses the whole rolling baseline into ONE
    // `HrvLnRmssd` input dated at the newest HRV day (see
    // `autoreg::derive_readiness`), so counting inputs in the trailing week
    // undercounts an active baseline — it sees 1, not the ≥7 days behind it —
    // whenever the newest HRV day isn't the global latest day. An active baseline
    // is the state that actually arms the autoreg-028 downgrade, so treat it as
    // usable HRV regardless of whether a reading landed on the latest day.
    let src = autoreg::autoreg_source(hrv_today || hrv_baseline_active, recent_hrv, subjective);
    // One plain line: what the weights follow right now.
    let text = match src.value {
        _ if hrv_suspended => {
            "HRV readings look unreliable, so weights follow your check-ins and logged sets."
        }
        // ready-upd-001: on lift days HRV only pauses progression; runs ease.
        autoreg::AutoregSource::HrvRolling => {
            "Your 7-day HRV trend can pause progress and ease runs; lift weights follow your logged sets."
        }
        autoreg::AutoregSource::SubjectivePlusPerformance => {
            "Weights follow your check-ins and logged sets."
        }
        autoreg::AutoregSource::PerformanceOnlyHold => {
            "Weights stay as planned until you add a Morning check-in."
        }
    };
    Some(to_view_with(text.to_string(), &src))
}

/// Whether any active readiness adjustment suppresses a load/volume increase: a
/// protective (load-reducing / rest / defer) recommendation is in scope, or
/// training is blocked outright. An increase row must not displace the
/// prescription while this holds (the readiness↔review reconciliation and the
/// review's own increase rows share this predicate).
pub(super) fn suppression_active(recs: &[Recommended<Adjustment>], train_blocked: bool) -> bool {
    train_blocked
        || recs.iter().any(|r| {
            matches!(
                r.value.base(),
                Adjustment::ReduceLoadPct(_)
                    | Adjustment::Deload { .. }
                    | Adjustment::CapRpe(_)
                    | Adjustment::CapTopEnd { .. }
                    | Adjustment::ReduceVolumePct(_)
                    | Adjustment::DowngradeSession
                    | Adjustment::ModifyAndMonitor { .. }
                    | Adjustment::StopItems { .. }
                    | Adjustment::ReturnCheck
                    | Adjustment::RestDay
                    | Adjustment::Stop
                    | Adjustment::Defer { .. }
            )
        })
}

/// How long a positive weekly RED-S screen keeps the deficit refusal armed
/// (REDS-SCREEN-001): the 4 weekly check-ins after it, an engine window for a
/// recurring weekly screen (conservative: it only withholds a deficit).
pub(super) const REDS_SCREEN_WINDOW_DAYS: i64 = 28;

/// Claims behind a pain response (the File 08 pain ladder).
fn is_pain_claim(c: ClaimId) -> bool {
    matches!(
        c,
        claims::SAFE_PAIN_001 | claims::SAFE_PAIN_STRUCT_001 | claims::SAFE_TENDON_001
    )
}

/// Claims behind an illness response (neck check, return-to-training, head cold).
fn is_illness_claim(c: ClaimId) -> bool {
    matches!(
        c,
        claims::ILLNESS_NECK_001 | claims::ILLNESS_RTT_001 | claims::ILLNESS_COLD_RUN_001
    )
}

/// The File 06 §5 ladder tier a recommendation belongs to, for ranking the
/// Today headline: the same ladder `autoreg::resolve_safety_in` raises, keyed
/// by the rule that emitted the call. `None` for calls off the ladder (e.g. a
/// load increase).
pub(super) fn adjustment_tier(r: &Recommended<Adjustment>) -> Option<SafetyTier> {
    let tier = match (r.value.base(), r.claim_id()) {
        (Adjustment::Defer { .. }, _) => SafetyTier::MedicalReferral,
        (_, c) if is_pain_claim(c) => SafetyTier::Pain,
        (_, c) if is_illness_claim(c) || c == claims::RETURN_HOLD_001 => SafetyTier::Illness,
        (Adjustment::RestDay, claims::AUTOREG_RHR_NOISE_001) => SafetyTier::Illness,
        (Adjustment::IncreaseLoadPct(_), _) => return None,
        (Adjustment::DowngradeSession, claims::WELLNESS_001) => SafetyTier::SubjectiveMultiDay,
        (Adjustment::DowngradeSession, claims::HRV_001) => SafetyTier::HrvTrend,
        (Adjustment::CapTopEnd { trigger }, _) if *trigger == ReadinessSignal::HrvLnRmssd => {
            SafetyTier::HrvTrend
        }
        (Adjustment::CapTopEnd { .. }, _) => SafetyTier::SingleDayMarker,
        (_, claims::AUTOREG_RHR_NOISE_001 | claims::AUTOREG_RHR_DOWN_001) => {
            SafetyTier::SingleDayMarker
        }
        (
            Adjustment::ReduceLoadPct(_)
            | Adjustment::Deload { .. }
            | Adjustment::CapRpe(_)
            | Adjustment::DowngradeSession,
            _,
        ) => SafetyTier::ObjectivePerformance,
        _ => return None,
    };
    Some(tier)
}

/// Why training is on hold (contract C2), the most specific reason first: a
/// profile health gate (it stops the plan itself), then the readiness block in
/// the ladder order the rule layer applies (medical red flag > pain > illness),
/// then a weekly-check-in referral. `None` unless `train_blocked`.
pub(super) fn hold_reason(
    train_blocked: bool,
    health: Option<&HealthScreen>,
    recommended: &[Recommended<Adjustment>],
    review_recs: &[Recommended<Adjustment>],
) -> Option<HoldReason> {
    if !train_blocked {
        return None;
    }
    if let Some(h) = health.filter(|h| h.any_gate()) {
        return Some(if h.youth {
            HoldReason::Youth
        } else if h.pregnant || h.pregnancy_warning_sign {
            HoldReason::Pregnancy
        } else if h.symptomatic_any() {
            HoldReason::Symptomatic
        } else if h.parq_followup_uncleared() {
            HoldReason::Parq
        } else if h.injury_any() {
            HoldReason::InjuryRehab
        } else if h.reds_signal {
            HoldReason::Reds
        } else {
            HoldReason::AcuteIllnessProfile
        });
    }
    let blocking = |r: &&Recommended<Adjustment>| {
        matches!(
            r.value,
            Adjustment::Stop | Adjustment::RestDay | Adjustment::Defer { .. }
        )
    };
    let reason = |r: &Recommended<Adjustment>| {
        let claim = r.claim_id();
        if claim == claims::SAFE_REDS_001 {
            HoldReason::Reds
        } else if is_pain_claim(claim) {
            HoldReason::Pain
        } else if is_illness_claim(claim) || matches!(r.value, Adjustment::RestDay) {
            HoldReason::Illness
        } else {
            HoldReason::MedicalSignal
        }
    };
    recommended
        .iter()
        .chain(review_recs)
        .find(blocking)
        .map(reason)
        .or(Some(HoldReason::MedicalSignal))
}

/// Content fingerprint of the check-in history (every field the derivation
/// reads), so a memo over derived readiness misses when any check-in changes.
pub(super) fn checkins_fingerprint(checkins: &[CheckinInput]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    checkins.len().hash(&mut h);
    for c in checkins {
        c.observed_at.hash(&mut h);
        c.sleep_quality.hash(&mut h);
        c.soreness.hash(&mut h);
        c.mood.hash(&mut h);
        c.fatigue.hash(&mut h);
        c.stress.hash(&mut h);
        c.resting_hr_bpm.map(f64::to_bits).hash(&mut h);
        c.hrv_rmssd_ms.map(f64::to_bits).hash(&mut h);
    }
    h.finish()
}

/// The check-in history the derivation reads: HRV readings logged before a
/// `RestartHrvBaseline` are blanked (a new device / timing, HRV-SOURCE-001).
pub(super) fn checkins_for_derivation(model: &Model) -> std::borrow::Cow<'_, [CheckinInput]> {
    match model.hrv_baseline_from {
        None => std::borrow::Cow::Borrowed(&model.checkins),
        Some(from) => std::borrow::Cow::Owned(
            model
                .checkins
                .iter()
                .map(|c| {
                    let mut c = c.clone();
                    if c.observed_at < from {
                        c.hrv_rmssd_ms = None;
                    }
                    c
                })
                .collect(),
        ),
    }
}

/// Every source the Today headline ranks, consulted in priority order by
/// [`build_headline`].
pub(super) struct HeadlineInputs<'a> {
    pub(super) train_blocked: bool,
    /// Onboarding safety gates (medical referral).
    pub(super) gates: &'a [Recommended<Adjustment>],
    /// Readiness recommendations.
    pub(super) recommended: &'a [Recommended<Adjustment>],
    /// Post-session review recommendations.
    pub(super) review_recs: &'a [Recommended<Adjustment>],
    pub(super) adjustments: &'a [AdjustmentView],
    pub(super) review_adjustments: &'a [AdjustmentView],
    pub(super) feedback: Option<&'a FeedbackView>,
    /// The next planned session (the prescription rung).
    pub(super) prescription: Option<&'a SessionPlanView>,
}

pub(super) fn build_headline(inputs: &HeadlineInputs<'_>) -> TodayHeadlineView {
    let &HeadlineInputs {
        train_blocked,
        gates,
        recommended,
        review_recs,
        adjustments,
        review_adjustments,
        feedback,
        prescription,
    } = inputs;
    let from_adjustment = |kind: HeadlineKind, v: &AdjustmentView| TodayHeadlineView {
        kind,
        summary: v.summary.clone(),
        evidence: v.evidence.clone(),
        why: v.why.clone(),
    };
    let from_feedback = |fb: &FeedbackView| TodayHeadlineView {
        kind: HeadlineKind::Feedback,
        summary: fb.message.clone(),
        evidence: fb.evidence.clone(),
        why: fb.why.clone(),
    };
    if train_blocked {
        let blocks = |r: &&Recommended<Adjustment>| {
            matches!(
                r.value,
                Adjustment::Stop | Adjustment::RestDay | Adjustment::Defer { .. }
            )
        };
        // Same dominance order view() uses for the tier: onboarding gates
        // (medical referral), then readiness stops, then review deferrals.
        if let Some(stop) = gates
            .iter()
            .find(blocks)
            .or_else(|| recommended.iter().find(blocks))
            .or_else(|| review_recs.iter().find(blocks))
        {
            return from_adjustment(HeadlineKind::SafetyHold, &to_view(stop));
        }
    }
    // A medical-referral feedback card (bone-stress signs, RED-S, compulsive
    // exercise) is a referral, not a programming tweak: it outranks every
    // training rung below. Kind stays "feedback" so shells render it unchanged.
    let referral = |fb: &FeedbackView| {
        fb.evidence.safety_critical
            && matches!(
                fb.category,
                FeedbackCategory::ConcernInjury
                    | FeedbackCategory::ConcernEnergy
                    | FeedbackCategory::ConcernBehavior
            )
    };
    if let Some(fb) = feedback
        && referral(fb)
    {
        return from_feedback(fb);
    }
    // The safety/adjustment call OUTRANKS the prescription rung. When any
    // readiness/pain adjustment is active (e.g. a tolerable-pain day), the
    // headline IS that call, "modify & monitor", "easier session", never the
    // full-load next session. Ranked on the §5 ladder first (Pain > Illness >
    // objective performance > multi-day subjective > HRV trend > single-day
    // markers; HARD RULE 3), then safety-critical, grade and confidence; the
    // FIRST of equals wins, so the rule layer's own ordering (pain responses
    // lead) decides ties.
    let rank = |v: &AdjustmentView| {
        (
            v.tier,
            v.evidence.safety_critical,
            grade_rank(v.evidence.grade),
        )
    };
    let best = adjustments
        .iter()
        .chain(review_adjustments.iter())
        // Skip neutral verdict rows (graded "no change" calls carrying no real
        // Adjustment); a stale review must not headline "hold planned loads"
        // over the prescription.
        .filter(|v| !v.neutral)
        .fold(None::<&AdjustmentView>, |best, v| match best {
            Some(b)
                if rank(v)
                    .cmp(&rank(b))
                    .then(v.evidence.confidence.total_cmp(&b.evidence.confidence))
                    .is_le() =>
            {
                Some(b)
            }
            _ => Some(v),
        });
    // Other safety-critical feedback (a recovery concern, a run-load spike)
    // sits below Pain and Illness calls but above every lower rung.
    let critical_feedback = feedback.filter(|fb| fb.evidence.safety_critical);
    if let Some(v) = best {
        if let Some(fb) = critical_feedback
            && v.tier < Some(SafetyTier::Illness)
        {
            return from_feedback(fb);
        }
        return from_adjustment(HeadlineKind::Adjustment, v);
    }
    if let Some(fb) = critical_feedback {
        return from_feedback(fb);
    }
    // Prescription rung: with no active adjustment, the highest
    // non-safety call is the concrete next session. Only when a plan is set and
    // not blocked; otherwise fall through to feedback/all-clear. The session
    // card states the session, so this rung carries no summary of its own.
    if let Some(ns) = prescription
        && ns.status != SessionStatus::Blocked
        && let Some(first) = ns.items.first()
    {
        return TodayHeadlineView {
            kind: HeadlineKind::Prescription,
            summary: String::new(),
            evidence: first.evidence.clone(),
            why: first.why.clone(),
        };
    }
    // Non-critical post-session feedback: safety-critical feedback already
    // returned above (it outranks the prescription), so anything reaching here
    // is informational and correctly ranks below the prescription rung.
    if let Some(fb) = feedback {
        return from_feedback(fb);
    }
    TodayHeadlineView {
        kind: HeadlineKind::AllClear,
        // States the absence of any triggered rule, not a graded claim, so
        // no evidence tag is attached (empty grade; shells render no chip).
        summary: "Train as planned today. Nothing you've logged calls for a change.".to_string(),
        ..TodayHeadlineView::default()
    }
}

/// Static signal→group metadata for every readiness signal, in picker order
/// (metrics before the red-flag block).
pub(super) fn build_signal_groups() -> std::borrow::Cow<'static, [SignalGroupView]> {
    // Static signal→group metadata, identical every call, memoize (see
    // `build_reference`).
    static CACHE: std::sync::LazyLock<Vec<SignalGroupView>> =
        std::sync::LazyLock::new(build_signal_groups_impl);
    std::borrow::Cow::Borrowed(&CACHE)
}

fn build_signal_groups_impl() -> Vec<SignalGroupView> {
    autoreg::ALL_SIGNALS
        .iter()
        .map(|&s| SignalGroupView {
            signal: s,
            group: autoreg::signal_group(s),
        })
        .collect()
}

/// Logged sets dated on or before local day `through` as performance-derivation
/// inputs. Holds and undated legacy rows are left out (a hold has
/// no e1RM, an undated set has no session day).
pub(super) fn perf_sets(
    sets: &[LoggedSet],
    through: i64,
    offset: i64,
) -> Vec<autoreg::PerfSet<'_>> {
    sets.iter()
        .filter(|s| s.hold_sec.is_none() && s.observed_at > 0)
        .filter(|s| local_day(s.observed_at, offset) <= through)
        .map(|s| autoreg::PerfSet {
            exercise: &s.exercise,
            weight_kg: s.weight_kg,
            reps: s.reps,
            rpe: s.rpe,
            observed_at: s.observed_at,
        })
        .collect()
}

/// The autoreg-029 saturation guard is armed by the profile's
/// manual "hard training block" flag OR automatically in the block's peak
/// accumulation week (the last load week before the planned recovery week),
/// where unusually high HRV is more likely saturation than freshness
/// (AUTOREG-HRV-SAT-001, Plews; Nuuttila 2024). Never in a recovery week.
pub(super) fn effective_high_load_block(
    model: &Model,
    days: &LoggedDays,
    profile: &Profile,
    day: i64,
) -> bool {
    if profile.high_load_block {
        return true;
    }
    let Some(idx) = plan_week_index(model, days, day.saturating_sub(mon0_weekday(day))) else {
        return false;
    };
    let (load_weeks, cycle_len) = crate::plan::cycle_shape(profile);
    load_weeks >= 2 && cycle_len > 0 && idx % cycle_len == load_weeks - 1
}

/// Whether readiness input `input` is dated on or before local day `day` (an
/// undated legacy input always counts).
fn input_dated_by(input: &ReadinessInput, day: i64, offset: i64) -> bool {
    is_undated(input.observed_at) || local_day(input.observed_at, offset) <= day
}

/// The manual readiness inputs dated on or before local day `dated_by` as they
/// stood on local day `shown_on`: every active one, plus every retracted one
/// whose removal fell AFTER `shown_on` (it was still in force that day).
/// Retracted inputs come first: they were submitted before their removal, and
/// the active ones keep their submission order behind them.
pub(super) fn manual_inputs_on(
    model: &Model,
    dated_by: i64,
    shown_on: i64,
    offset: i64,
) -> Vec<ReadinessInput> {
    model
        .retracted_inputs
        .iter()
        .filter(|r| local_day(r.retracted_at, offset) > shown_on)
        .map(|r| &r.input)
        .chain(model.inputs.iter())
        .filter(|i| input_dated_by(i, dated_by, offset))
        .cloned()
        .collect()
}

/// The readiness picture as of local day `as_of` (`None`: the shell has sent
/// no clock, so nothing is date-filtered or expired): the check-in signals
/// derived from the check-ins through that day, the manual inputs as they
/// stood on it (a later removal never re-judges it), logged-set performance
/// through it, and the rule context. Today's view and every past rendered day
/// read this one pipeline.
pub(super) struct ReadinessPicture {
    pub(super) derived: autoreg::DerivedReadiness,
    /// Derived signals first, then the manual inputs, so `latest_input`'s
    /// recency tie-break lets an explicit entry win over a same-instant
    /// derived one; stale increase-driving performance readings dropped.
    pub(super) inputs: Vec<ReadinessInput>,
    perf: Vec<autoreg::MovementPerf>,
    goal: Option<Goal>,
    high_load_block: bool,
    as_of: Option<i64>,
    offset: i64,
}

impl ReadinessPicture {
    pub(super) fn rule_ctx(&self) -> autoreg::RuleContext<'_> {
        autoreg::RuleContext {
            goal: self.goal.as_ref(),
            high_load_block: self.high_load_block,
            today_epoch_day: self.as_of,
            utc_offset_sec: self.offset,
            rhr: self.derived.rhr,
            perf: &self.perf,
            wellness_collecting: self
                .derived
                .collecting
                .iter()
                .any(|b| b.signal == ReadinessSignal::WellnessZ),
        }
    }

    /// The rules' adjustments plus, with a clock, the first session after a
    /// resolved pain or illness hold (the easy session or the return check).
    pub(super) fn adjustments(&self, model: &Model) -> Vec<Recommended<Adjustment>> {
        let mut recs = autoreg::adjustments_in(&self.inputs, &self.rule_ctx());
        if let Some(day) = self.as_of {
            recs.extend(return_step_on(model, day, self.offset));
        }
        recs
    }
}

/// Build the [`ReadinessPicture`] as of `as_of` with `profile` (the one in force
/// that day) selecting the goal-dependent thresholds; no profile → conservative
/// defaults.
pub(super) fn readiness_picture(
    model: &Model,
    days: &LoggedDays,
    profile: Option<&Profile>,
    as_of: Option<i64>,
) -> ReadinessPicture {
    let offset = model.today_utc_offset_sec;
    let goal = profile.map(|p| match p.lift_goal {
        LiftGoal::MaxStrength => Goal::Strength,
        LiftGoal::Power => Goal::Power,
        LiftGoal::Hypertrophy => Goal::Hypertrophy,
    });
    // autoreg-029 guard: the manual "hard block" flag, or auto-armed in the
    // block's peak accumulation week.
    let high_load_block = match (profile, as_of) {
        (Some(p), Some(day)) => effective_high_load_block(model, days, p, day),
        (Some(p), None) => p.high_load_block,
        (None, _) => false,
    };
    let all = checkins_for_derivation(model);
    let dated = |c: &CheckinInput| {
        c.observed_at <= 0 || as_of.is_none_or(|d| local_day(c.observed_at, offset) <= d)
    };
    let checkins: std::borrow::Cow<'_, [CheckinInput]> = if all.iter().all(dated) {
        all
    } else {
        std::borrow::Cow::Owned(all.iter().filter(|c| dated(c)).cloned().collect())
    };
    let derived = autoreg::derive_readiness(&checkins, offset, as_of);
    let mut inputs = derived.inputs.clone();
    match as_of {
        Some(day) => inputs.extend(manual_inputs_on(model, day, day, offset)),
        None => inputs.extend(model.inputs.iter().cloned()),
    }
    // A "felt easy" performance signal goes stale: a three-week-old RPE −2 must
    // not keep proposing IncreaseLoadPct. Only readings that can drive a load
    // INCREASE expire (dropping them is conservative); a protective reading in
    // the same channel, and every safety signal, never does (HARD RULE 3). The
    // 14-day window is an engine heuristic (no KB parameter). Needs a clock.
    if let Some(day) = as_of {
        inputs.retain(|i| {
            !is_expirable_perf_signal(i)
                || day.saturating_sub(local_day(i.observed_at, offset)) <= PERF_SIGNAL_EXPIRY_DAYS
        });
    }
    // The Strong-graded performance backbone reads the LOGGED sets
    // (top-set e1RM vs the 14-day best), not a hand-typed ratio.
    let perf = as_of
        .map(|day| autoreg::derive_performance(&perf_sets(&model.sets, day, offset), day, offset))
        .unwrap_or_default();
    ReadinessPicture {
        derived,
        inputs,
        perf,
        goal,
        high_load_block,
        as_of,
        offset,
    }
}

/// ready-upd-003/004: the first-session-back adjustment in force on
/// local `day`. A pain or illness hold must have ended on or before `day` (the
/// user's dated "I'm better" / "I'm pain-free", or a newer all-clear report),
/// no hold may be active now, and no session (a logged set or run) may have
/// been done on a day from the end of the hold up to, but not including,
/// `day`: one session back is easy (or the post-illness return check), then
/// the plan resumes. Inputs and retractions are taken as they stood on `day`,
/// so a later retraction never re-judges an earlier day.
pub(super) fn return_step_on(
    model: &Model,
    day: i64,
    offset: i64,
) -> Option<Recommended<Adjustment>> {
    let dated_by = |at: i64| at <= 0 || local_day(at, offset) <= day;
    let timeline: Vec<autoreg::TimedInput> = model
        .inputs
        .iter()
        .filter(|i| dated_by(i.observed_at))
        .map(|input| autoreg::TimedInput {
            input,
            retracted_at: None,
        })
        .chain(
            model
                .retracted_inputs
                .iter()
                .filter(|r| dated_by(r.input.observed_at))
                .map(|r| autoreg::TimedInput {
                    input: &r.input,
                    // A removal dated after `day` was not yet in force on it.
                    retracted_at: dated_by(r.retracted_at).then_some(r.retracted_at),
                }),
        )
        .collect();
    let (kind, ended_at) = autoreg::last_hold_resolution(&timeline)?;
    let ended_day = if ended_at > 0 {
        local_day(ended_at, offset)
    } else {
        i64::MIN
    };
    if ended_day > day {
        return None;
    }
    let trained_since = model
        .sets
        .iter()
        .map(|s| s.observed_at)
        .chain(model.runs.iter().map(|r| r.observed_at))
        .filter(|t| *t > 0)
        .map(|t| local_day(t, offset))
        .any(|d| d >= ended_day && d < day);
    (!trained_since).then(|| autoreg::return_adjustment(kind))
}

/// The banner's resolve action per active hold: pain first
/// (it outranks illness on the ladder), then illness. Illness is ONE action
/// covering both ways the user can say they are ill: an active illness report
/// and the profile's "I'm ill right now" answer (`profile_ill`), so a single
/// "I've recovered" / "I'm better" ends both. Its label follows the profile's
/// gate copy ("Then tap I'm better") when that answer is on; only a resolved
/// report leads into the easier return session (ready-upd-003). "No fever" is
/// part of the recovery check only for a fever-class illness (a below-neck or
/// fever report, or the profile's ill answer: ready-upd-004); a head cold
/// clears once its symptoms are gone (ready-upd-008).
pub(super) fn hold_resolutions(
    inputs: &[ReadinessInput],
    profile_ill: bool,
) -> Vec<HoldResolutionView> {
    let mut out = Vec::new();
    if autoreg::pain_hold_active(inputs) {
        out.push(HoldResolutionView {
            signal: ReadinessSignal::Pain,
            paused_text: "Paused until you're pain-free through the full movement.".into(),
            action_label: "I'm pain-free now".into(),
            confirm_text:
                "Pain-free through the full movement? The plan restarts you with an easier session."
                    .into(),
            ends_profile_illness: false,
        });
    }
    let reported = autoreg::illness_active(inputs);
    if reported || profile_ill {
        let action_label = if profile_ill {
            "I'm better"
        } else {
            "I've recovered"
        };
        let fever = profile_ill || autoreg::illness_stop(inputs);
        let (paused_text, check) = if fever {
            (
                "Paused until your symptoms are gone and you have no fever.",
                "Symptoms gone and no fever?",
            )
        } else {
            ("Paused until your symptoms are gone.", "Symptoms gone?")
        };
        let next = if reported {
            "The plan restarts you with an easier session."
        } else {
            "Your plan picks up again from today."
        };
        out.push(HoldResolutionView {
            signal: ReadinessSignal::Illness,
            paused_text: paused_text.into(),
            action_label: action_label.into(),
            confirm_text: format!("{check} {next}"),
            ends_profile_illness: profile_ill,
        });
    }
    out
}
