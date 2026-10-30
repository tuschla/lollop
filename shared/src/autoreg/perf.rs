//! Logged sets → per-lift performance (autoreg-006/007/022).
//!
//! The Strong-graded performance backbone (AUTOREG-RIR-001, Helms 2018) reads
//! the user's logged sets, not a hand-typed ratio: each lift's top-set e1RM is
//! compared against that lift's best over the previous 14 local days.

use super::rules::{
    E1RM_DELOAD_RATIO, E1rmOutcome, e1rm_decline_deload, e1rm_ratio_outcome, e1rm_session_recs,
};
use crate::evidence::{self, claims};
use crate::schema::{Adjustment, Recommended};
use crate::strength;

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
/// Lowest logged RPE whose reps-in-reserve the KB maps (Zourdos RPE 6–10 ↔ RIR
/// 4–0, File 02 strength-007): below it the e1RM estimate is not "at RPE".
const E1RM_MIN_RPE: f64 = 6.0;

/// e1RM of one set when it is a valid "e1RM at RPE" estimate: a loaded,
/// compound (non-isolation) set inside the reliable rep range with an RPE the
/// RIR mapping covers. `None` otherwise (never a fabricated anchor).
fn scored_e1rm(s: &PerfSet) -> Option<f64> {
    (s.weight_kg > 0.0
        && s.rpe >= E1RM_MIN_RPE
        && strength::e1rm_reliable(strength::effective_reps(s.reps, s.rpe), false).value
        && !crate::plan::is_isolation_lift(s.exercise))
    .then(|| strength::e1rm_epley_rpe(s.weight_kg, s.reps, s.rpe))
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
        .filter(|s| !crate::is_undated(s.observed_at))
        .collect();
    ordered.sort_by_key(|s| s.observed_at);
    // One movement key per distinct exercise name, not per set.
    let mut keys: HashMap<&str, Vec<String>> = HashMap::new();
    for s in &ordered {
        keys.entry(s.exercise)
            .or_insert_with(|| crate::plan::movement_key(s.exercise));
    }
    // Per local day: (top weight, first top-set e1RM, its exercise name, best e1RM).
    type DaySlot<'a> = (f64, f64, &'a str, f64);
    // movement key → local day → DaySlot.
    let mut by_lift: BTreeMap<&[String], BTreeMap<i64, DaySlot>> = BTreeMap::new();
    for s in ordered {
        let Some(e) = scored_e1rm(s) else { continue };
        let day = crate::local_day(s.observed_at, utc_offset_sec);
        if day > today_epoch_day {
            continue;
        }
        let slot = by_lift
            .entry(keys[s.exercise].as_slice())
            .or_default()
            .entry(day)
            .or_insert((s.weight_kg, e, s.exercise, e));
        if s.weight_kg > slot.0 {
            *slot = (s.weight_kg, e, s.exercise, slot.3.max(e));
        } else {
            slot.3 = slot.3.max(e);
        }
    }
    let mut out = Vec::new();
    for days in by_lift.values() {
        let sessions: Vec<(i64, f64, &str, f64)> = days
            .iter()
            .map(|(d, (_, top, n, best))| (*d, *top, *n, *best))
            .collect();
        let ratio_at = |i: usize| -> Option<f64> {
            let (d, top, _, _) = sessions[i];
            let base = sessions[..i]
                .iter()
                .filter(|(pd, ..)| d - pd <= E1RM_BASELINE_DAYS)
                .map(|(.., best)| *best)
                .fold(f64::NAN, f64::max);
            (base.is_finite() && base > 0.0).then(|| top / base)
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
            let (day, _, name, _) = sessions[idx];
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
                exercise: name.to_string(),
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
pub(super) fn perf_adjustments(perf: &[MovementPerf]) -> Vec<Recommended<Adjustment>> {
    let mut out = Vec::new();
    if perf
        .iter()
        .any(|p| !p.today && e1rm_ratio_outcome(p.ratio, p.decline_streak) == E1rmOutcome::Deload)
    {
        out.push(evidence::graded(
            e1rm_decline_deload(),
            claims::AUTOREG_DELOAD_001,
        ));
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
