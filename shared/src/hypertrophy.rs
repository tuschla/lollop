//! Hypertrophy programming calculators (knowledge-base File 03 -
//! Evidence-Graded Hypertrophy Programming Logic).
//!
//! Pure, deterministic look-ups + arithmetic: per-muscle weekly volume
//! landmarks (Table 1), rep/load prescription by exercise class (Table 2),
//! volume→frequency mapping (Table 3), rest defaults (Table 5), the RP
//! accumulation set-ramp and RIR schedule. No IO, no clocks, no randomness.
//!
//! Numbers transcribed verbatim from File 03. Every prescriptive value is
//! wrapped in [`Recommended`] via [`recommend`], which forces attached evidence
//! + confidence from the compile-time registry (`crate::evidence`). Claim ids:
//! HYP-VOL-001, HYP-LANDMARKS-001, HYP-REPLOAD-001, HYP-FREQ-001, HYP-REST-001,
//! HYP-RIR-RAMP-001.

use crate::evidence;
use crate::individualization::TrainingAge;
use crate::schema::Recommended;

/// Build a `Recommended<T>` from a registry claim id (must exist).
fn recommend<T>(value: T, claim_id: &str) -> Recommended<T> {
    let e = evidence::claim(claim_id).expect("known hypertrophy claim");
    Recommended {
        value,
        evidence: e.to_evidence(),
        confidence: e.to_confidence_tag(),
    }
}

// ---------------------------------------------------------------------------
// 1. Per-muscle weekly volume landmarks (File 03 Table 1; HYP-LANDMARKS-001)
// ---------------------------------------------------------------------------

/// Weekly direct-set landmarks for one muscle (intermediate lifter).
/// Population starting points, not validated constants (HYP-LANDMARKS-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeLandmarks {
    pub muscle: &'static str,
    /// Maintenance Volume, weekly sets to retain muscle.
    pub mv: u8,
    /// Minimum Effective Volume, mesocycle start point.
    pub mev: u8,
    /// Maximum Adaptive Volume, the productive climb range (lo, hi).
    pub mav: (u8, u8),
    /// Maximum Recoverable Volume, recovery ceiling.
    pub mrv: u8,
}

/// Table 1, transcribed verbatim from File 03 (RP framework, ExpertOpinion).
/// `mrv` uses the "N+" lower bound (e.g. "22+" → 22).
pub static LANDMARKS: &[VolumeLandmarks] = &[
    VolumeLandmarks { muscle: "chest", mv: 8, mev: 10, mav: (12, 20), mrv: 22 },
    VolumeLandmarks { muscle: "back", mv: 8, mev: 10, mav: (14, 22), mrv: 25 },
    VolumeLandmarks { muscle: "quads", mv: 6, mev: 8, mav: (12, 18), mrv: 20 },
    VolumeLandmarks { muscle: "hamstrings", mv: 4, mev: 6, mav: (10, 16), mrv: 20 },
    VolumeLandmarks { muscle: "glutes", mv: 0, mev: 0, mav: (4, 12), mrv: 16 },
    VolumeLandmarks { muscle: "side delts", mv: 6, mev: 8, mav: (16, 22), mrv: 26 },
    VolumeLandmarks { muscle: "rear delts", mv: 0, mev: 6, mav: (12, 18), mrv: 22 },
    VolumeLandmarks { muscle: "biceps", mv: 4, mev: 6, mav: (14, 20), mrv: 26 },
    VolumeLandmarks { muscle: "triceps", mv: 4, mev: 6, mav: (10, 14), mrv: 18 },
    VolumeLandmarks { muscle: "calves", mv: 4, mev: 6, mav: (8, 16), mrv: 20 },
    VolumeLandmarks { muscle: "abs", mv: 0, mev: 0, mav: (10, 16), mrv: 20 },
];

/// Look up landmarks by muscle name (case-insensitive). `None` if unknown.
pub fn landmarks_for(muscle: &str) -> Option<&'static VolumeLandmarks> {
    LANDMARKS.iter().find(|l| l.muscle.eq_ignore_ascii_case(muscle))
}

// ---------------------------------------------------------------------------
// 2. Rep/load by exercise class (File 03 Table 2; HYP-REPLOAD-001)
// ---------------------------------------------------------------------------

/// Exercise class driving the rep/load window (File 03 Table 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExerciseClass {
    /// Squat, deadlift, press, row, manages joint/systemic fatigue per rep.
    HeavyCompound,
    /// Machine / moderate compound, balances tension and volume.
    ModerateCompound,
    /// Curls, raises, extensions, lighter loads, higher reps.
    Isolation,
}

/// Rep range + %1RM window for a hypertrophy set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepLoad {
    /// Target reps per set (min, max).
    pub reps: (u8, u8),
    /// %1RM band (min, max).
    pub pct_1rm: (u8, u8),
}

/// Rep/load prescription for an exercise class (File 03 Table 2; HYP-REPLOAD-001).
pub fn rep_load(class: ExerciseClass) -> Recommended<RepLoad> {
    let rl = match class {
        ExerciseClass::HeavyCompound => RepLoad { reps: (5, 10), pct_1rm: (75, 85) },
        ExerciseClass::ModerateCompound => RepLoad { reps: (8, 15), pct_1rm: (65, 75) },
        ExerciseClass::Isolation => RepLoad { reps: (12, 25), pct_1rm: (50, 70) },
    };
    recommend(rl, "HYP-REPLOAD-001")
}

/// Between-set rest window in seconds for an exercise class (File 03 Table 5;
/// HYP-REST-001). Compounds rest longer to preserve per-set volume; the goal is
/// keeping >=90% of first-set reps, not the clock itself.
pub fn rest_sec_for(class: ExerciseClass) -> Recommended<(u16, u16)> {
    let window = match class {
        ExerciseClass::HeavyCompound | ExerciseClass::ModerateCompound => (120, 180),
        ExerciseClass::Isolation => (60, 120),
    };
    recommend(window, "HYP-REST-001")
}

// ---------------------------------------------------------------------------
// 3. Volume→frequency mapping (File 03 Table 3; HYP-FREQ-001)
// ---------------------------------------------------------------------------

/// Suggested weekly frequency and per-session set spread for a weekly volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrequencyRx {
    /// Sessions per week for this muscle (min, max).
    pub freq: (u8, u8),
    /// Sets per session (min, max).
    pub per_session: (u8, u8),
}

/// Map weekly sets/muscle to a frequency + per-session spread (File 03 Table 3;
/// HYP-FREQ-001). Bands: ≤10 → 1–2×; 11–18 → 2–3×; >18 → 3×.
pub fn frequency_for_weekly_sets(weekly_sets: u8) -> Recommended<FrequencyRx> {
    let rx = if weekly_sets <= 10 {
        FrequencyRx { freq: (1, 2), per_session: (1, 8) }
    } else if weekly_sets <= 18 {
        FrequencyRx { freq: (2, 3), per_session: (5, 8) }
    } else {
        FrequencyRx { freq: (3, 3), per_session: (6, 9) }
    };
    recommend(rx, "HYP-FREQ-001")
}

// ---------------------------------------------------------------------------
// 4. RP accumulation drivers (File 03 hyp-032/019)
// ---------------------------------------------------------------------------

/// Weekly set counts ramping from `mev` to `mrv` over `weeks` accumulation
/// weeks (File 03 hyp-001/032; HYP-VOL-001). Linear interpolation, floored, so
/// the RP worked example (MEV 10 → MRV 20 over 4 wk) yields `[10, 13, 16, 20]`.
/// `weeks == 0` → empty; `weeks == 1` → `[mev]`; `mrv < mev` clamps to `mev`.
pub fn weekly_set_ramp(mev: u8, mrv: u8, weeks: u8) -> Recommended<Vec<u8>> {
    let ramp = if weeks == 0 {
        Vec::new()
    } else if weeks == 1 {
        vec![mev]
    } else {
        let top = mrv.max(mev);
        let span = (top - mev) as f64;
        let last = (weeks - 1) as f64;
        (0..weeks)
            .map(|i| mev + (span * i as f64 / last) as u8)
            .collect()
    };
    recommend(ramp, "HYP-VOL-001")
}

/// Reps-in-reserve target for `week` of a `block_weeks`-long accumulation block
/// (File 03 hyp-019; HYP-RIR-RAMP-001). RIR descends to 1 in the final week:
/// a 4-week block gives week 1→4, 2→3, 3→2, 4→1. `None` outside `1..=block_weeks`.
pub fn rir_for_week(week: u8, block_weeks: u8) -> Option<Recommended<u8>> {
    if week == 0 || week > block_weeks {
        return None;
    }
    let rir = block_weeks - week + 1;
    Some(recommend(rir, "HYP-RIR-RAMP-001"))
}

// ---------------------------------------------------------------------------
// 5. Volume ceilings, training-age MEV, and recovery scaling (File 03
//    hypertrophy-003/004/006/007/008/009/010/045/025)
// ---------------------------------------------------------------------------

/// Growth-target ceiling: outcome superiority is undetectable beyond ~31
/// fractional weekly sets/muscle (File 03 hypertrophy-003; Pelland 2025). Never
/// program a growth target above this.
pub const WEEKLY_FRACTIONAL_SET_CEILING: u8 = 31;

/// Per-session cap: ~11 fractional sets/muscle; beyond this, redistribute work
/// to another session rather than adding sets (File 03 hypertrophy-004).
pub const PER_SESSION_SET_CEILING: u8 = 11;

/// Minimum sets per exercise, multiple sets beat single sets ~+40% ES
/// (File 03 hypertrophy-006; Krieger 2010).
pub const MIN_SETS_PER_EXERCISE: u8 = 2;

/// The weekly-set target above which a muscle's work must be split across ≥2
/// sessions (File 03 hypertrophy-025).
pub const WEEKLY_SPLIT_THRESHOLD: u8 = 12;

/// Clamp a proposed weekly growth-target set count to the ~31-set ceiling
/// (File 03 hypertrophy-003).
pub fn cap_weekly_growth_target(weekly_sets: u8) -> Recommended<u8> {
    recommend(weekly_sets.min(WEEKLY_FRACTIONAL_SET_CEILING), "HYP-VOL-001")
}

/// MEV weekly-set band per muscle by training age (File 03 hypertrophy-007):
/// beginner 6–10, intermediate 10–18, advanced 12–20(+). Returns `(lo, hi)`.
pub fn mev_sets_by_training_age(age: TrainingAge) -> Recommended<(u8, u8)> {
    let band = match age {
        TrainingAge::Novice => (6, 10),
        TrainingAge::Intermediate => (10, 18),
        TrainingAge::Advanced => (12, 20),
    };
    recommend(band, "HYP-LANDMARKS-001")
}

/// Next-mesocycle weekly-set target given growth/recovery signals (File 03
/// hypertrophy-008): if a muscle is not growing while recovery is easy, add +2
/// sets (still below MEV); otherwise hold. Capped at the growth ceiling.
pub fn next_meso_weekly_sets(
    current_weekly_sets: u8,
    not_growing: bool,
    recovering_easily: bool,
) -> Recommended<u8> {
    let next = if not_growing && recovering_easily {
        (current_weekly_sets + 2).min(WEEKLY_FRACTIONAL_SET_CEILING)
    } else {
        current_weekly_sets
    };
    recommend(next, "HYP-VOL-001")
}

/// At/over-MRV deload gate (File 03 hypertrophy-009): weekly sets > ~20/muscle
/// with regressing performance OR aching joints → treat as over MRV and deload.
pub fn over_mrv_deload(
    weekly_sets: u8,
    performance_down: bool,
    joint_ache: bool,
) -> Recommended<bool> {
    recommend(weekly_sets > 20 && (performance_down || joint_ache), "HYP-VOL-001")
}

/// Recovery-adjusted weekly volume (File 03 hypertrophy-010/045): in a deficit
/// or with poor sleep/high stress, scale weekly sets to 70–80% of the base and
/// reduce failure frequency. Returns the `(lo, hi)` adjusted set counts; when
/// recovery is high the base passes through unchanged.
pub fn recovery_adjusted_volume(base_weekly_sets: u8, low_recovery: bool) -> Recommended<(f64, f64)> {
    let b = base_weekly_sets as f64;
    let range = if low_recovery { (b * 0.70, b * 0.80) } else { (b, b) };
    recommend(range, "HYP-VOL-001")
}

/// Joint-pain rep shift (File 03 hypertrophy-016): on joint pain at heavy loads,
/// move that muscle's work to 12–25 reps at lighter load (50–70% 1RM);
/// hypertrophy is preserved via load interchangeability.
pub fn joint_pain_rep_shift() -> Recommended<RepLoad> {
    recommend(RepLoad { reps: (12, 25), pct_1rm: (50, 70) }, "HYP-REPLOAD-001")
}

/// Whether a weekly-set target must be split across ≥2 sessions (File 03
/// hypertrophy-025): true once weekly sets exceed ~12/muscle.
pub fn needs_session_split(weekly_sets: u8) -> Recommended<bool> {
    recommend(weekly_sets > WEEKLY_SPLIT_THRESHOLD, "HYP-FREQ-001")
}

/// Whether per-session volume exceeds the ~11-set cap and warrants adding a
/// session rather than more sets (File 03 hypertrophy-004/025).
pub fn per_session_over_cap(session_sets: u8) -> Recommended<bool> {
    recommend(session_sets > PER_SESSION_SET_CEILING, "HYP-FREQ-001")
}

/// Deload gate from accumulated overreaching triggers (File 03 hypertrophy-035):
/// deload now once ≥2 triggers appear (performance decrement, RIR drift to 0,
/// persistent joint/tendon aches, disrupted sleep, elevated RHR, mood drop) -
/// otherwise follow the preplanned 4–8 week schedule.
pub fn deload_indicated(trigger_count: u8) -> Recommended<bool> {
    recommend(trigger_count >= 2, "HYP-VOL-001")
}

/// A one-week hypertrophy deload prescription (File 03 hypertrophy-036).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeloadRx {
    /// Fraction of working sets to keep (~MV, roughly half).
    pub sets_fraction: f64,
    /// Reps-in-reserve to hold (min, max+).
    pub rir: (u8, u8),
    /// Load as a fraction of working weight (lo, hi).
    pub load_frac_of_working: (f64, f64),
}

/// The standard hypertrophy deload week: ~half the sets, 2–4+ RIR, loads
/// ~60–70% of working weight, movement patterns kept (File 03 hypertrophy-036).
pub fn deload_rx() -> Recommended<DeloadRx> {
    recommend(
        DeloadRx { sets_fraction: 0.50, rir: (2, 4), load_frac_of_working: (0.60, 0.70) },
        "HYP-VOL-001",
    )
}

/// Increase rest when per-set reps fall > ~10% set-to-set (File 03
/// hypertrophy-039): the mechanism is preserving per-set volume, not rest
/// duration itself. `true` = lengthen the rest interval.
pub fn increase_rest_on_rep_drop(rep_drop_frac: f64) -> Recommended<bool> {
    recommend(rep_drop_frac > 0.10, "HYP-REST-001")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::EvidenceGrade;

    #[test]
    fn landmarks_lookup_is_case_insensitive_and_verbatim() {
        let chest = landmarks_for("Chest").expect("chest present");
        assert_eq!(chest.mev, 10);
        assert_eq!(chest.mav, (12, 20));
        assert_eq!(chest.mrv, 22);
        assert_eq!(landmarks_for("QUADS").unwrap().mv, 6);
        assert!(landmarks_for("tail").is_none());
    }

    #[test]
    fn rep_load_windows_match_table_2() {
        let heavy = rep_load(ExerciseClass::HeavyCompound);
        assert_eq!(heavy.value.reps, (5, 10));
        assert_eq!(heavy.value.pct_1rm, (75, 85));
        assert_eq!(heavy.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(rep_load(ExerciseClass::Isolation).value.reps, (12, 25));
    }

    #[test]
    fn rest_windows_split_compound_vs_isolation() {
        assert_eq!(rest_sec_for(ExerciseClass::HeavyCompound).value, (120, 180));
        assert_eq!(rest_sec_for(ExerciseClass::Isolation).value, (60, 120));
    }

    #[test]
    fn frequency_bands_follow_table_3() {
        assert_eq!(frequency_for_weekly_sets(8).value.freq, (1, 2));
        assert_eq!(frequency_for_weekly_sets(14).value.freq, (2, 3));
        assert_eq!(frequency_for_weekly_sets(24).value.freq, (3, 3));
        // Frequency evidence is Strong (2x/week beats 1x).
        assert_eq!(
            frequency_for_weekly_sets(14).evidence.grade,
            EvidenceGrade::Strong
        );
    }

    #[test]
    fn set_ramp_matches_rp_worked_example() {
        let ramp = weekly_set_ramp(10, 20, 4);
        assert_eq!(ramp.value, vec![10, 13, 16, 20]);
        // Degenerate cases.
        assert_eq!(weekly_set_ramp(10, 20, 1).value, vec![10]);
        assert!(weekly_set_ramp(10, 20, 0).value.is_empty());
        // MRV below MEV clamps flat.
        assert_eq!(weekly_set_ramp(12, 8, 3).value, vec![12, 12, 12]);
    }

    #[test]
    fn growth_ceiling_and_min_sets() {
        assert_eq!(WEEKLY_FRACTIONAL_SET_CEILING, 31);
        assert_eq!(MIN_SETS_PER_EXERCISE, 2);
        assert_eq!(cap_weekly_growth_target(40).value, 31);
        assert_eq!(cap_weekly_growth_target(20).value, 20);
    }

    #[test]
    fn mev_scales_with_training_age() {
        assert_eq!(mev_sets_by_training_age(TrainingAge::Novice).value, (6, 10));
        assert_eq!(mev_sets_by_training_age(TrainingAge::Intermediate).value, (10, 18));
        assert_eq!(mev_sets_by_training_age(TrainingAge::Advanced).value, (12, 20));
    }

    #[test]
    fn next_meso_adds_two_when_stalled_and_fresh() {
        assert_eq!(next_meso_weekly_sets(12, true, true).value, 14);
        assert_eq!(next_meso_weekly_sets(12, true, false).value, 12);
        assert_eq!(next_meso_weekly_sets(30, true, true).value, 31); // capped
    }

    #[test]
    fn over_mrv_and_recovery_scaling() {
        assert!(over_mrv_deload(22, true, false).value);
        assert!(over_mrv_deload(22, false, true).value);
        assert!(!over_mrv_deload(18, true, true).value); // under 20 sets
        assert!(!over_mrv_deload(22, false, false).value); // no symptom
        // Low recovery scales to 70–80%.
        let (lo, hi) = recovery_adjusted_volume(20, true).value;
        assert!((lo - 14.0).abs() < 1e-9 && (hi - 16.0).abs() < 1e-9);
        assert_eq!(recovery_adjusted_volume(20, false).value, (20.0, 20.0));
    }

    #[test]
    fn joint_pain_shifts_to_light_high_reps() {
        let rx = joint_pain_rep_shift().value;
        assert_eq!(rx.reps, (12, 25));
        assert_eq!(rx.pct_1rm, (50, 70));
    }

    #[test]
    fn split_triggers_and_deload() {
        assert!(needs_session_split(13).value);
        assert!(!needs_session_split(12).value);
        assert!(per_session_over_cap(12).value);
        assert!(!per_session_over_cap(11).value);
        assert!(deload_indicated(2).value);
        assert!(!deload_indicated(1).value);
        let d = deload_rx().value;
        assert!((d.sets_fraction - 0.50).abs() < 1e-9);
        assert_eq!(d.rir, (2, 4));
        assert_eq!(d.load_frac_of_working, (0.60, 0.70));
        assert!(increase_rest_on_rep_drop(0.12).value);
        assert!(!increase_rest_on_rep_drop(0.08).value);
    }

    #[test]
    fn rir_ramp_descends_to_one() {
        assert_eq!(rir_for_week(1, 4).unwrap().value, 4);
        assert_eq!(rir_for_week(2, 4).unwrap().value, 3);
        assert_eq!(rir_for_week(3, 4).unwrap().value, 2);
        assert_eq!(rir_for_week(4, 4).unwrap().value, 1);
        assert!(rir_for_week(0, 4).is_none());
        assert!(rir_for_week(5, 4).is_none());
        // ExpertOpinion-graded schedule.
        assert_eq!(
            rir_for_week(1, 4).unwrap().evidence.grade,
            EvidenceGrade::ExpertOpinion
        );
    }
}
