//! The e1RM rule (autoreg-006/007/022): its thresholds, the manual
//! `EstimatedOneRm` gate, and logged sets → per-lift performance.
//!
//! The Strong-graded performance backbone (AUTOREG-RIR-001, Helms 2018) reads
//! the user's logged sets, not a hand-typed ratio: each lift's top-set e1RM is
//! compared against that lift's best over the previous 14 local days.

use super::inputs::latest_input;
use crate::evidence::claims;
use crate::schema::{Adjustment, AdjustmentCall, AdjustmentKind, ReadinessInput, ReadinessSignal};
use crate::strength;

/// AUTOREG-E1RM-GATE-001 noise band (autoreg-006/007, Helms 2018): an e1RM
/// within ±5 % of its baseline is day-to-day noise. The one band every reader
/// shares; each keeps its own baseline (this gate: the 14-day best; the plan's
/// load anchors: the last 3 sessions / 21 days; the review card: the prior
/// 90-day best and the previous 4-week block).
pub(crate) const E1RM_NOISE_FRAC: f64 = 0.05;
/// e1RM ratio (today ÷ baseline) gates (autoreg-022/006/007).
pub(super) const E1RM_DELOAD_RATIO: f64 = 0.90;
pub(super) const E1RM_DELOAD_STREAK: u8 = 2;
pub(crate) const E1RM_REDUCE_RATIO: f64 = 1.0 - E1RM_NOISE_FRAC;
pub(super) const E1RM_INCREASE_RATIO: f64 = 1.0 + E1RM_NOISE_FRAC;
/// Top-set load cut below baseline − 5 % (autoreg-006 "reduce top-set load ~5%").
pub(super) const E1RM_REDUCE_LOAD_PCT: f32 = 5.0;
/// Load add above baseline + 5 % (autoreg-007 "+2.5–5%"; 3.5 % used).
pub(super) const E1RM_INCREASE_LOAD_PCT: f32 = 3.5;

/// One logged set as the performance derivation reads it.
#[derive(Debug, Clone, Copy)]
pub struct PerfSet<'a> {
    pub exercise: &'a str,
    pub weight_kg: f64,
    pub reps: u32,
    /// Logged RPE; `0` = not recorded (the set is not scored).
    pub rpe: f64,
    pub observed_at: i64,
}

/// One lift's newest session, scored against its own recent best.
#[derive(Debug, Clone, PartialEq)]
pub struct MovementPerf {
    /// Exercise name of that session's top set, as logged.
    pub exercise: String,
    /// Top-set e1RM ÷ the best e1RM of the previous 14 local days.
    pub ratio: f64,
    /// Consecutive sessions, ending with this one, whose ratio was < 0.90.
    pub decline_streak: u8,
    /// Local day of the session.
    pub day: i64,
    /// Whether the session is today: per-item cuts only touch today's sets.
    pub today: bool,
}

/// Baseline window for the e1RM gate: the "14-day rolling best"
/// (`ReadinessSignal::EstimatedOneRm`), excluding the session itself.
const E1RM_BASELINE_DAYS: i64 = 14;
/// How long a confirmed e1RM-decline deload stays in force after the session
/// that confirmed it: the rule's own one week (autoreg-022, "1 wk").
const E1RM_DELOAD_HOLD_DAYS: i64 = 7;

/// e1RM of one set when it is a valid "e1RM at RPE" estimate: a loaded,
/// compound (non-isolation) set inside the reliable rep range with an exact
/// rating (`strength::rpe_is_exact`; "4+ reps left" is only a lower bound, so
/// it never scores as a drop). `None` otherwise (never a fabricated anchor).
fn scored_e1rm(s: &PerfSet) -> Option<f64> {
    (s.weight_kg > 0.0
        && strength::rpe_is_exact(s.rpe)
        && strength::e1rm_reliable(strength::effective_reps(s.reps, s.rpe), false).value
        && !crate::exercise::is_isolation_lift(s.exercise))
    .then(|| strength::e1rm_epley_rpe(s.weight_kg, s.reps, s.rpe))
}

/// One lift's scored sets on one local day.
struct DaySlot<'a> {
    /// Heaviest scored load.
    top_kg: f64,
    /// e1RM of the first set at that load.
    top_e1: f64,
    /// That set's exercise name, as logged.
    exercise: &'a str,
    /// Best e1RM of the day.
    best: f64,
}

/// Per-lift performance from logged sets: for every lift with a
/// session in the last `E1RM_DELOAD_HOLD_DAYS` local days, its newest
/// session's FIRST top set (the first set at that day's heaviest scored load)
/// is scored against the best scored e1RM of the previous
/// `E1RM_BASELINE_DAYS` local days, and the run of consecutive sessions below
/// 90 % is counted (autoreg-022 "≥2 consecutive sessions"). Lifts with no
/// baseline yet emit nothing. Deterministic: days are local, time is the
/// shell's `today_epoch_day`.
pub fn derive_performance(
    sets: &[PerfSet],
    today_epoch_day: i64,
    utc_offset_sec: i64,
) -> Vec<MovementPerf> {
    use std::collections::{BTreeMap, HashMap};
    let mut ordered: Vec<&PerfSet> = sets
        .iter()
        .filter(|s| !crate::day::is_undated(s.observed_at))
        .collect();
    ordered.sort_by_key(|s| s.observed_at);
    // One movement key per distinct exercise name, not per set.
    let mut keys: HashMap<&str, Vec<String>> = HashMap::new();
    for s in &ordered {
        keys.entry(s.exercise)
            .or_insert_with(|| crate::exercise::movement_key(s.exercise));
    }
    // movement key → local day → DaySlot.
    let mut by_lift: BTreeMap<&[String], BTreeMap<i64, DaySlot>> = BTreeMap::new();
    for s in ordered {
        let Some(e) = scored_e1rm(s) else { continue };
        let day = crate::day::local_day(s.observed_at, utc_offset_sec);
        if day > today_epoch_day {
            continue;
        }
        let top = || DaySlot {
            top_kg: s.weight_kg,
            top_e1: e,
            exercise: s.exercise,
            best: e,
        };
        let slot = by_lift
            .entry(keys[s.exercise].as_slice())
            .or_default()
            .entry(day)
            .or_insert_with(top);
        if s.weight_kg > slot.top_kg {
            *slot = DaySlot {
                best: slot.best.max(e),
                ..top()
            };
        } else {
            slot.best = slot.best.max(e);
        }
    }
    let mut out = Vec::new();
    for days in by_lift.values() {
        let sessions: Vec<(i64, &DaySlot)> = days.iter().map(|(d, s)| (*d, s)).collect();
        let ratio_at = |i: usize| -> Option<f64> {
            let (d, top) = sessions[i];
            let base = sessions[..i]
                .iter()
                .filter(|(pd, _)| d - pd <= E1RM_BASELINE_DAYS)
                .map(|(_, s)| s.best)
                .fold(f64::NAN, f64::max);
            (base.is_finite() && base > 0.0).then(|| top.top_e1 / base)
        };
        let Some(last) = sessions.len().checked_sub(1) else {
            continue;
        };
        // The newest session, plus (when that one is today's, still in
        // progress) the session before it: the week-level deload is judged on
        // finished sessions only, so logging today's sets never deloads today's
        // own session; today's sets drive only the per-item cuts.
        let mut candidates = vec![last];
        if sessions[last].0 == today_epoch_day && last >= 1 {
            candidates.push(last - 1);
        }
        for idx in candidates {
            let (day, top) = sessions[idx];
            if today_epoch_day.saturating_sub(day) > E1RM_DELOAD_HOLD_DAYS {
                continue;
            }
            let Some(ratio) = ratio_at(idx) else {
                continue;
            };
            let mut decline_streak: u8 = 0;
            for i in (0..=idx).rev() {
                match ratio_at(i) {
                    Some(r) if r < E1RM_DELOAD_RATIO => {
                        decline_streak = decline_streak.saturating_add(1)
                    }
                    _ => break,
                }
            }
            out.push(MovementPerf {
                exercise: top.exercise.to_string(),
                ratio,
                decline_streak,
                day,
                today: day == today_epoch_day,
            });
        }
    }
    out
}

/// The evidence-cited adjustments a per-lift performance picture calls for
/// (autoreg-006/007/022): any lift down >10 % for ≥2 consecutive sessions →
/// the 1-week deload (AUTOREG-DELOAD-001); a lift scored TODAY below 95 % →
/// that lift's remaining sets −5 % and one more rep in reserve; above 105 % →
/// an item-scoped +3.5 % suggestion (stripped under suppression, autoreg-044).
pub(super) fn perf_adjustments(perf: &[MovementPerf]) -> Vec<AdjustmentCall> {
    let mut out = Vec::new();
    if perf
        .iter()
        .any(|p| !p.today && e1rm_ratio_outcome(p.ratio, p.decline_streak) == E1rmOutcome::Deload)
    {
        out.push(
            AdjustmentKind::E1rmDeload.call(e1rm_decline_deload(), claims::AUTOREG_DELOAD_001),
        );
    }
    for p in perf.iter().filter(|p| p.today) {
        let item = |a: Adjustment| Adjustment::ForItem {
            exercise: p.exercise.clone(),
            adjustment: Box::new(a),
        };
        // Today's session is cut per item; the deload arm is the multi-session
        // verdict above, so today's ratio is classified without its streak.
        out.extend(e1rm_session_recs(e1rm_ratio_outcome(p.ratio, 0), item));
    }
    out
}

/// e1RM-driven load gate (both directions).
/// autoreg-022: e1RM at fixed RPE down >10% for **≥2 consecutive sessions** →
/// deload (volume −40–50%, load −5–10%, 1 wk). `EstimatedOneRm` carries the
/// ratio today ÷ baseline (< 0.90 = >10% drop) and `streak` the consecutive
/// sessions the drop has held; a single-session drop never deloads: it falls
/// through to the autoreg-006 session cap instead.
/// autoreg-006 (both clauses, AUTOREG-E1RM-GATE-001, Strong, Helms 2018):
/// e1RM < baseline − 5% (ratio < 0.95) → reduce top-set load ~5% AND cap the
/// session at planned RPE − 1.
/// autoreg-007: e1RM > baseline + 5% (ratio > 1.05) → add load ~2.5–5% (uses
/// 3.5%). Same Helms 2018 e1RM-gate source as its mirror autoreg-006, so it
/// cites AUTOREG-E1RM-GATE-001.
pub(super) fn e1rm_gate(inputs: &[ReadinessInput]) -> Vec<AdjustmentCall> {
    let Some(input) = latest_input(inputs, ReadinessSignal::EstimatedOneRm) else {
        return Vec::new();
    };
    match e1rm_ratio_outcome(input.value, input.streak) {
        E1rmOutcome::Deload => {
            vec![AdjustmentKind::E1rmDeload.call(e1rm_decline_deload(), claims::AUTOREG_DELOAD_001)]
        }
        outcome => e1rm_session_recs(outcome, |a| a),
    }
}

/// Today's session adjustments for an e1RM outcome, each passed through `wrap`
/// (AUTOREG-E1RM-GATE-001). Reduce (including a single-session >10% drop: File
/// 06 §5 "trust performance → reduce load") cuts the top-set load ~5% and caps
/// the session at planned RPE − 1 (autoreg-006); Increase adds load
/// (autoreg-007). The multi-session Deload arm is the caller's.
pub(super) fn e1rm_session_recs(
    outcome: E1rmOutcome,
    wrap: impl Fn(Adjustment) -> Adjustment,
) -> Vec<AdjustmentCall> {
    let gate = |a| AdjustmentKind::E1rm.call(wrap(a), claims::AUTOREG_E1RM_GATE_001);
    match outcome {
        E1rmOutcome::Reduce => vec![
            gate(Adjustment::ReduceLoadPct(E1RM_REDUCE_LOAD_PCT)),
            gate(Adjustment::CapRpe(1)),
        ],
        E1rmOutcome::Increase => vec![gate(Adjustment::IncreaseLoadPct(E1RM_INCREASE_LOAD_PCT))],
        E1rmOutcome::Deload | E1rmOutcome::Stable => Vec::new(),
    }
}

/// Which arm of the e1RM rule (autoreg-022/006/007) a ratio today ÷ baseline
/// and its consecutive-decline streak select. The one classification the
/// manual gate, the logged-set performance gate and the readiness row share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum E1rmOutcome {
    /// Down >10 % for ≥2 consecutive sessions (autoreg-022).
    Deload,
    /// Below baseline − 5 % (autoreg-006), including a single >10 % drop.
    Reduce,
    /// Above baseline + 5 % (autoreg-007).
    Increase,
    Stable,
}

pub(super) fn e1rm_ratio_outcome(ratio: f64, streak: u8) -> E1rmOutcome {
    if ratio < E1RM_DELOAD_RATIO && streak >= E1RM_DELOAD_STREAK {
        E1rmOutcome::Deload
    } else if ratio < E1RM_REDUCE_RATIO {
        E1rmOutcome::Reduce
    } else if ratio > E1RM_INCREASE_RATIO {
        E1rmOutcome::Increase
    } else {
        E1rmOutcome::Stable
    }
}

/// The autoreg-022 e1RM-decline deload: 1 week at the midpoints of its
/// volume −40–50 % and load −5–10 % bands.
pub(super) fn e1rm_decline_deload() -> Adjustment {
    Adjustment::Deload {
        volume_reduction_pct: 45.0,
        load_reduction_pct: 7.5,
        weeks: 1,
    }
}
