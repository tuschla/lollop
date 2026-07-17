//! Strength & power calculators (knowledge-base File 02, Strength & Power
//! Resistance-Training Programming Rules).
//!
//! Pure, deterministic math: e1RM estimators, RPE/RIR mapping, reps→%1RM
//! estimation, and Prilepin volume governance. No IO. Values that make a
//! prescriptive recommendation are wrapped in [`Recommended`] via
//! [`recommend`], which forces attached evidence + confidence.
//!
//! All numbers transcribed verbatim from File 02 (rules strength-005,
//! strength-007, strength-011 and the Prilepin's-chart verbatim table). Claim
//! ids come from the canonical registry in `crate::evidence`.

use crate::evidence::claim;
use crate::schema::{
    Citation, ConfidenceTag, Evidence, EvidenceGrade, Recommended,
};

// ---------------------------------------------------------------------------
// Recommendation helper
// ---------------------------------------------------------------------------

/// Wrap a prescriptive value with evidence + confidence pulled from the
/// registry (File 02 claim ids). Falls back to an `ExpertOpinion` synthesis
/// citation if the id is absent, so the helper never silently drops evidence.
pub fn recommend<T>(value: T, claim_id: &str) -> Recommended<T> {
    match claim(claim_id) {
        Some(entry) => Recommended {
            value,
            evidence: entry.to_evidence(),
            confidence: entry.to_confidence_tag(),
        },
        None => Recommended {
            value,
            evidence: Evidence {
                grade: EvidenceGrade::ExpertOpinion,
                citation: Citation {
                    claim_id: None,
                    reference: "File 02 synthesis (unregistered id)".to_string(),
                },
                contradicting: vec![],
            },
            confidence: ConfidenceTag {
                score: EvidenceGrade::ExpertOpinion.default_confidence(),
                contested: false,
                contested_question_ref: None,
                safety_critical: false,
            },
        },
    }
}

// ---------------------------------------------------------------------------
// 1. Estimated 1RM (File 02 strength-005)
// ---------------------------------------------------------------------------

/// Epley estimated 1RM: `weight * (1 + reps/30)` (File 02 strength-005).
pub fn e1rm_epley(weight: f64, reps: u32) -> f64 {
    weight * (1.0 + reps as f64 / 30.0)
}

/// Brzycki estimated 1RM: `weight * 36 / (37 - reps)` (File 02 strength-005).
/// Undefined at 37 reps (division by zero); callers should keep reps in the
/// 1–10 accuracy window noted in strength-005/strength-006.
pub fn e1rm_brzycki(weight: f64, reps: u32) -> f64 {
    weight * 36.0 / (37.0 - reps as f64)
}

// ---------------------------------------------------------------------------
// 2. RPE ↔ RIR mapping (File 02 strength-007; registry AUTOREG-RIR-001)
// ---------------------------------------------------------------------------

/// Reps in reserve from RPE via the Zourdos anchor: RPE 10 = 0 RIR, each RIR
/// = −1 RPE (File 02 strength-007; registry AUTOREG-RIR-001). Clamped at 0.
pub fn rpe_to_rir(rpe: f64) -> f64 {
    let rir = 10.0 - rpe;
    if rir < 0.0 {
        0.0
    } else {
        rir
    }
}

/// RPE from reps in reserve, inverse of [`rpe_to_rir`] (File 02 strength-007;
/// registry AUTOREG-RIR-001). Clamped at 10.
pub fn rir_to_rpe(rir: f64) -> f64 {
    let rpe = 10.0 - rir;
    if rpe > 10.0 {
        10.0
    } else {
        rpe
    }
}

// ---------------------------------------------------------------------------
// 3. Reps → %1RM estimate (File 02 strength-005, Epley inverse)
// ---------------------------------------------------------------------------

/// Estimated %1RM for a maximal set of `reps`, as the Epley inverse
/// `100 / (1 + reps/30)` (File 02 strength-005). ESTIMATE ONLY: accurate at
/// 2–10 reps (±5%), diverges ±15–20% above 10 reps (strength-005/006).
pub fn est_pct_1rm_from_reps(reps: u32) -> f64 {
    100.0 / (1.0 + reps as f64 / 30.0)
}

// ---------------------------------------------------------------------------
// 4. Prilepin's chart (File 02 strength-011, verbatim table)
// ---------------------------------------------------------------------------

/// One row of Prilepin's chart: an intensity band with its rep-per-set range,
/// optimal total reps, and total-rep range (File 02 verbatim table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrilepinRow {
    /// Inclusive lower %1RM bound for this band.
    pub pct_min: u8,
    /// Inclusive upper %1RM bound for this band.
    pub pct_max: u8,
    /// Reps per set (min, max).
    pub reps_per_set: (u8, u8),
    /// Optimal total reps for the session at this intensity.
    pub optimal_total: u16,
    /// Acceptable total-rep range (min, max).
    pub total_range: (u16, u16),
}

/// Prilepin's chart, transcribed verbatim from File 02 (strength-011).
///
/// | %1RM   | Reps/set | Optimal total | Total range |
/// |--------|----------|---------------|-------------|
/// | <70%   | 3–6      | 24            | 18–30       |
/// | 70–80% | 3–6      | 18            | 12–24       |
/// | 80–90% | 2–4      | 15            | 10–20       |
/// | >90%   | 1–2      | 7             | 4–10        |
///
/// Band bounds are encoded as half-open on the verbatim breakpoints (69, 79,
/// 89, 100) so every intensity 0–100% resolves to exactly one row.
pub static PRILEPIN: &[PrilepinRow] = &[
    PrilepinRow {
        pct_min: 0,
        pct_max: 69,
        reps_per_set: (3, 6),
        optimal_total: 24,
        total_range: (18, 30),
    },
    PrilepinRow {
        pct_min: 70,
        pct_max: 79,
        reps_per_set: (3, 6),
        optimal_total: 18,
        total_range: (12, 24),
    },
    PrilepinRow {
        pct_min: 80,
        pct_max: 89,
        reps_per_set: (2, 4),
        optimal_total: 15,
        total_range: (10, 20),
    },
    PrilepinRow {
        pct_min: 90,
        pct_max: 100,
        reps_per_set: (1, 2),
        optimal_total: 7,
        total_range: (4, 10),
    },
];

/// Look up the Prilepin row governing a given %1RM (File 02 strength-011).
/// Returns `None` for out-of-range (<0 or >100) intensities. The <70% band is
/// treated as a floor, matching the verbatim "<70%" row.
pub fn prilepin_for(pct_1rm: f64) -> Option<&'static PrilepinRow> {
    if !(0.0..=100.0).contains(&pct_1rm) {
        return None;
    }
    // Round to nearest whole percent for band membership.
    let pct = pct_1rm.round() as i64;
    PRILEPIN
        .iter()
        .find(|row| pct >= row.pct_min as i64 && pct <= row.pct_max as i64)
}

/// True when `total_reps` falls within the Prilepin total-rep range for the
/// band at `pct_1rm` (File 02 strength-011 volume governor). False if the
/// intensity is out of range or the volume is outside the band's window.
pub fn prilepin_volume_ok(pct_1rm: f64, total_reps: u16) -> bool {
    match prilepin_for(pct_1rm) {
        Some(row) => total_reps >= row.total_range.0 && total_reps <= row.total_range.1,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 5. Loading prescription bands (File 02 strength-001/002/003; STR-INTENT-001)
// ---------------------------------------------------------------------------

/// The training quality a loading prescription targets (File 02 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiftGoal {
    MaxStrength,
    Power,
    Hypertrophy,
}

/// A loading prescription: intensity, reps, sets, rest, and proximity to failure
/// (File 02 strength-001/002/003, verbatim bands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadingRx {
    /// Working intensity as %1RM (min, max).
    pub pct_1rm: (u8, u8),
    /// Reps per set (min, max).
    pub reps: (u8, u8),
    /// Sets (min, max).
    pub sets: (u8, u8),
    /// Inter-set rest in seconds (min, max).
    pub rest_sec: (u16, u16),
    /// Reps in reserve target (min, max); power stops before velocity decay.
    pub rir: (u8, u8),
}

/// Loading bands by goal (File 02 strength-001/002/003; STR-INTENT-001).
/// Strength 80-100% / 1-5 / 1-3 RIR; power a velocity-biased spectrum stopped
/// well short of failure; hypertrophy 65-85% / 6-12 / 0-3 RIR.
pub fn loading_rx(goal: LiftGoal) -> Recommended<LoadingRx> {
    let rx = match goal {
        LiftGoal::MaxStrength => LoadingRx { pct_1rm: (80, 100), reps: (1, 5), sets: (3, 6), rest_sec: (180, 300), rir: (1, 3) },
        LiftGoal::Power => LoadingRx { pct_1rm: (30, 70), reps: (1, 5), sets: (3, 6), rest_sec: (180, 300), rir: (3, 5) },
        LiftGoal::Hypertrophy => LoadingRx { pct_1rm: (65, 85), reps: (6, 12), sets: (3, 6), rest_sec: (30, 90), rir: (0, 3) },
    };
    recommend(rx, "STR-INTENT-001")
}

// ---------------------------------------------------------------------------
// 6. Load progression (File 02 strength-012/014; DBLPROG-001)
// ---------------------------------------------------------------------------

/// 2-for-2 rule (File 02 strength-012): increase load once the athlete beats the
/// goal by >=2 reps on the last set in 2 consecutive sessions. DBLPROG-001.
pub fn two_for_two_met(reps_over_goal_last_set: u8, consecutive_sessions: u8) -> Recommended<bool> {
    recommend(reps_over_goal_last_set >= 2 && consecutive_sessions >= 2, "DBLPROG-001")
}

/// Percentage auto-progression per successful week (File 02 strength-014):
/// lower-body +2.5-5%, upper-body +1-2.5% of load. Returns (min, max) fraction.
/// DBLPROG-001.
pub fn weekly_pct_increment(upper_body: bool) -> Recommended<(f64, f64)> {
    let inc = if upper_body { (0.01, 0.025) } else { (0.025, 0.05) };
    recommend(inc, "DBLPROG-001")
}

/// Whether a stall triggers a deload / model switch (File 02 strength-039):
/// (estimated) 1RM flat for >=2 weeks despite adequate recovery. PERIOD-001.
pub fn stall_triggers_deload(weeks_stalled: u8, recovery_adequate: bool) -> Recommended<bool> {
    recommend(weeks_stalled >= 2 && recovery_adequate, "PERIOD-001")
}

// ---------------------------------------------------------------------------
// 6b. Periodization model selection (File 02 strength-009/010; PERIOD-001)
// ---------------------------------------------------------------------------

/// A periodization model (File 02 strength-010/021-024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodizationModel {
    /// High-volume/low-intensity → low-volume/high-intensity across mesocycles.
    Linear,
    /// Daily-undulating: vary intensity/rep focus session-to-session.
    Dup,
    /// Accumulation → transmutation → realization concentrated blocks.
    Block,
    /// Max-effort / dynamic-effort / repetition rotation (advanced only).
    Conjugate,
}

/// Select a periodization model by training age (File 02 strength-010): novice →
/// linear; intermediate → DUP or block; advanced → block or conjugate. No single
/// model is hard-coded as superior (strength-009). Returns the primary default
/// per level. PERIOD-001.
pub fn periodization_model(level: crate::individualization::TrainingAge) -> Recommended<PeriodizationModel> {
    use crate::individualization::TrainingAge;
    let model = match level {
        TrainingAge::Novice => PeriodizationModel::Linear,
        TrainingAge::Intermediate => PeriodizationModel::Dup,
        TrainingAge::Advanced => PeriodizationModel::Block,
    };
    recommend(model, "PERIOD-001")
}

// ---------------------------------------------------------------------------
// 7. Taper (File 02 strength-026/027; TAPER-001)
// ---------------------------------------------------------------------------

/// Peaking taper prescription (File 02 strength-026; TAPER-001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TaperRx {
    /// Exponential volume reduction (min, max) as fractions.
    pub volume_reduction_frac: (f64, f64),
    /// Taper duration in days (min, max).
    pub duration_days: (u8, u8),
    /// Intensity and frequency are held, not reduced.
    pub hold_intensity: bool,
}

/// Best-evidenced peaking taper: cut volume 41-60% exponentially over ~2 weeks
/// while holding intensity and frequency (File 02 strength-026; TAPER-001).
pub fn taper_rx() -> Recommended<TaperRx> {
    recommend(
        TaperRx { volume_reduction_frac: (0.41, 0.60), duration_days: (8, 14), hold_intensity: true },
        "TAPER-001",
    )
}

// ---------------------------------------------------------------------------
// 8. Plyometric volume caps (File 02 strength-032; PLYO-001)
// ---------------------------------------------------------------------------

/// Foot-contact ceiling per plyometric session by training level (File 02
/// strength-032; PLYO-001). Returns (min, max) foot contacts. Progress volume
/// OR intensity, never both.
pub fn plyo_foot_contact_cap(level: crate::individualization::TrainingAge) -> Recommended<(u16, u16)> {
    use crate::individualization::TrainingAge;
    let cap = match level {
        TrainingAge::Novice => (80, 100),
        TrainingAge::Intermediate => (100, 120),
        TrainingAge::Advanced => (120, 140),
    };
    recommend(cap, "PLYO-001")
}

// ---------------------------------------------------------------------------
// 9. Power/peaking specifics (File 02 strength-029/031/034)
// ---------------------------------------------------------------------------

/// Days before a test/meet to schedule the last true near-max deadlift, given
/// its high systemic fatigue cost (File 02 strength-029). Returns (min, max)
/// days out. TAPER-001.
pub fn deadlift_peak_days_out() -> Recommended<(u8, u8)> {
    recommend((10, 14), "TAPER-001")
}

/// PAP/PAPE contrast rest window in minutes (File 02 strength-034): pair a heavy
/// conditioning activity with explosive work after 3-7 min (default ~5).
/// Stronger athletes potentiate earlier. STR-INTENT-001.
pub fn pap_rest_window_min() -> Recommended<(u8, u8)> {
    recommend((3, 7), "STR-INTENT-001")
}

/// Olympic-lift pulling-derivative loading (File 02 strength-031): 3-5 sets ×
/// 1-3 reps at 85-100%+ of full-lift 1RM, placed early in the session.
/// STR-INTENT-001.
pub fn olympic_derivative_rx() -> Recommended<LoadingRx> {
    recommend(
        LoadingRx { pct_1rm: (85, 100), reps: (1, 3), sets: (3, 5), rest_sec: (180, 300), rir: (3, 5) },
        "STR-INTENT-001",
    )
}

// ---------------------------------------------------------------------------
// 10. Lombardi e1RM (File 02 strength-005, third estimator)
// ---------------------------------------------------------------------------

/// Lombardi estimated 1RM: `weight * reps^0.10` (File 02 strength-005). Same
/// 1–10 rep accuracy window (±5%) as Epley/Brzycki; treat as approximate.
pub fn e1rm_lombardi(weight: f64, reps: u32) -> f64 {
    weight * (reps as f64).powf(0.10)
}

// ---------------------------------------------------------------------------
// 11. RPE-anchored top set + back-offs (File 02 strength-015; AUTOREG-RIR-001)
// ---------------------------------------------------------------------------

/// An RPE-anchored top-set / back-off scheme (File 02 strength-015).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackOffRx {
    /// Target RPE for the top set (~8).
    pub top_set_rpe: u8,
    /// Load drop for back-off sets as a fraction of the top set (min, max).
    pub drop_frac: (f64, f64),
}

/// Top set at RPE ~8, back-off sets dropped 10–15% of the top-set load (File 02
/// strength-015). AUTOREG-RIR-001 (RPE anchoring).
pub fn rpe_anchored_back_off() -> Recommended<BackOffRx> {
    recommend(BackOffRx { top_set_rpe: 8, drop_frac: (0.10, 0.15) }, "AUTOREG-RIR-001")
}

// ---------------------------------------------------------------------------
// 12. Velocity-loss set termination by goal (File 02 strength-018; AUTOREG-VL-001)
// ---------------------------------------------------------------------------

/// Velocity-loss fraction at which to terminate the set for a given goal (File
/// 02 strength-018): ≥20% VL for strength/power (preserves CMJ/1RM), ≥40% VL
/// permitted for hypertrophy. AUTOREG-VL-001.
pub fn vl_termination_threshold(goal: LiftGoal) -> Recommended<f64> {
    let vl = match goal {
        LiftGoal::MaxStrength | LiftGoal::Power => 0.20,
        LiftGoal::Hypertrophy => 0.40,
    };
    recommend(vl, "AUTOREG-VL-001")
}

// ---------------------------------------------------------------------------
// 13. Periodization phase tables (File 02 strength-021 linear / 022 block)
// ---------------------------------------------------------------------------

/// A per-phase loading prescription. Fields the knowledge base leaves
/// unspecified for a phase are `None` (e.g. block realization gives only reps;
/// linear taper defers to the taper template). No numbers are invented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseRx {
    /// Working intensity %1RM (min, max), when the table specifies it.
    pub pct_1rm: Option<(u8, u8)>,
    /// Sets (min, max), when specified.
    pub sets: Option<(u8, u8)>,
    /// Reps per set (min, max), when specified.
    pub reps: Option<(u8, u8)>,
    /// Mesocycle week span for this phase (min, max).
    pub weeks: (u8, u8),
}

/// Linear/step periodization phase (File 02 strength-021 verbatim table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinearPhase {
    /// Hypertrophy base: 67–75%, 3–5×8–12, weeks 1–4.
    Base,
    /// Basic strength build: 80–87%, 4–5×4–6, weeks 5–8.
    Build,
    /// Peak strength: 90–95%+, 3–5×1–3, weeks 9–11.
    Peak,
    /// Taper/test week 12: maintain intensity, per the taper template.
    Taper,
}

/// Linear periodization phase prescription (File 02 strength-021). Taper defers
/// to [`taper_rx`] (pct/sets/reps `None`, maintain intensity). PERIOD-001.
pub fn linear_phase_rx(phase: LinearPhase) -> Recommended<PhaseRx> {
    let rx = match phase {
        LinearPhase::Base => PhaseRx { pct_1rm: Some((67, 75)), sets: Some((3, 5)), reps: Some((8, 12)), weeks: (1, 4) },
        LinearPhase::Build => PhaseRx { pct_1rm: Some((80, 87)), sets: Some((4, 5)), reps: Some((4, 6)), weeks: (5, 8) },
        LinearPhase::Peak => PhaseRx { pct_1rm: Some((90, 95)), sets: Some((3, 5)), reps: Some((1, 3)), weeks: (9, 11) },
        LinearPhase::Taper => PhaseRx { pct_1rm: None, sets: None, reps: None, weeks: (12, 12) },
    };
    recommend(rx, "PERIOD-001")
}

/// Block periodization phase (File 02 strength-022 verbatim bands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockPhase {
    /// Accumulation: ~65–80%, 3–5×6–10, build work capacity.
    Accumulation,
    /// Transmutation: ~80–90%, 3–6×3–6, sport-specific.
    Transmutation,
    /// Realization: lowest volume, peak/taper, 1–3 reps (intensity/sets
    /// unspecified in the source, deferred to the taper template).
    Realization,
}

/// Block periodization phase prescription (File 02 strength-022); 2–4 wk
/// concentrated blocks. Realization gives only reps in the source. Contested
/// (CQ-02: no meta-analysis confirms block > traditional). PERIOD-001.
pub fn block_phase_rx(phase: BlockPhase) -> Recommended<PhaseRx> {
    let rx = match phase {
        BlockPhase::Accumulation => PhaseRx { pct_1rm: Some((65, 80)), sets: Some((3, 5)), reps: Some((6, 10)), weeks: (2, 4) },
        BlockPhase::Transmutation => PhaseRx { pct_1rm: Some((80, 90)), sets: Some((3, 6)), reps: Some((3, 6)), weeks: (2, 4) },
        BlockPhase::Realization => PhaseRx { pct_1rm: None, sets: None, reps: Some((1, 3)), weeks: (2, 4) },
    };
    recommend(rx, "PERIOD-001")
}

// ---------------------------------------------------------------------------
// 14. Depth-jump readiness gate (File 02 strength-033; SAFETY, ExpertOpinion)
// ---------------------------------------------------------------------------

/// SAFETY gate: require a ~1.5× bodyweight back-squat before high-intensity
/// depth jumps (File 02 strength-033). ExpertOpinion prerequisite, contested
/// (CQ-05), but `safety_critical`, landing loads are injurious without the
/// strength/mechanics base. Also requires landing-mechanics competence, which
/// this numeric gate does not capture; callers must verify it separately.
pub fn depth_jump_ready(squat_1rm: f64, bodyweight: f64) -> Recommended<bool> {
    let ready = bodyweight > 0.0 && squat_1rm >= 1.5 * bodyweight;
    Recommended {
        value: ready,
        evidence: Evidence {
            grade: EvidenceGrade::ExpertOpinion,
            citation: Citation {
                claim_id: None,
                reference: "File 02 strength-033 (depth-jump readiness gate)".to_string(),
            },
            contradicting: vec![],
        },
        confidence: ConfidenceTag {
            score: EvidenceGrade::ExpertOpinion.default_confidence(),
            contested: true,
            contested_question_ref: Some("CQ-05".to_string()),
            safety_critical: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lombardi_and_backoff_and_vl() {
        assert!((e1rm_lombardi(100.0, 1) - 100.0).abs() < 1e-9);
        assert!(e1rm_lombardi(100.0, 5) > 100.0);
        let b = rpe_anchored_back_off().value;
        assert_eq!((b.top_set_rpe, b.drop_frac), (8, (0.10, 0.15)));
        assert_eq!(vl_termination_threshold(LiftGoal::MaxStrength).value, 0.20);
        assert_eq!(vl_termination_threshold(LiftGoal::Power).value, 0.20);
        assert_eq!(vl_termination_threshold(LiftGoal::Hypertrophy).value, 0.40);
    }

    #[test]
    fn periodization_phase_tables() {
        let base = linear_phase_rx(LinearPhase::Base).value;
        assert_eq!((base.pct_1rm, base.sets, base.reps, base.weeks), (Some((67, 75)), Some((3, 5)), Some((8, 12)), (1, 4)));
        assert_eq!(linear_phase_rx(LinearPhase::Taper).value.pct_1rm, None);
        let acc = block_phase_rx(BlockPhase::Accumulation).value;
        assert_eq!((acc.pct_1rm, acc.sets, acc.reps), (Some((65, 80)), Some((3, 5)), Some((6, 10))));
        let real = block_phase_rx(BlockPhase::Realization).value;
        assert_eq!((real.pct_1rm, real.reps), (None, Some((1, 3))));
    }

    #[test]
    fn depth_jump_gate_is_safety_critical() {
        let g = depth_jump_ready(150.0, 100.0);
        assert!(g.value);
        assert!(!depth_jump_ready(140.0, 100.0).value);
        assert!(!depth_jump_ready(200.0, 0.0).value);
        assert!(g.confidence.safety_critical);
        assert!(g.confidence.contested);
    }

    #[test]
    fn power_peaking_specifics() {
        assert_eq!(deadlift_peak_days_out().value, (10, 14));
        assert_eq!(pap_rest_window_min().value, (3, 7));
        let ol = olympic_derivative_rx().value;
        assert_eq!((ol.pct_1rm, ol.reps, ol.sets), ((85, 100), (1, 3), (3, 5)));
    }

    #[test]
    fn loading_bands_match_file02() {
        let s = loading_rx(LiftGoal::MaxStrength).value;
        assert_eq!((s.pct_1rm, s.reps, s.rir), ((80, 100), (1, 5), (1, 3)));
        let h = loading_rx(LiftGoal::Hypertrophy).value;
        assert_eq!((h.pct_1rm, h.reps, h.rest_sec, h.rir), ((65, 85), (6, 12), (30, 90), (0, 3)));
        // Strength intensity floor sits above hypertrophy's.
        assert!(loading_rx(LiftGoal::MaxStrength).value.pct_1rm.0 > loading_rx(LiftGoal::Hypertrophy).value.pct_1rm.0);
    }

    #[test]
    fn two_for_two_and_progression() {
        assert!(two_for_two_met(2, 2).value);
        assert!(!two_for_two_met(2, 1).value);
        assert!(!two_for_two_met(1, 3).value);
        assert_eq!(weekly_pct_increment(true).value, (0.01, 0.025));
        assert_eq!(weekly_pct_increment(false).value, (0.025, 0.05));
        assert!(stall_triggers_deload(3, true).value);
        assert!(!stall_triggers_deload(3, false).value);
        assert!(!stall_triggers_deload(1, true).value);
    }

    #[test]
    fn periodization_by_training_age() {
        use crate::individualization::TrainingAge;
        assert_eq!(periodization_model(TrainingAge::Novice).value, PeriodizationModel::Linear);
        assert_eq!(periodization_model(TrainingAge::Intermediate).value, PeriodizationModel::Dup);
        assert_eq!(periodization_model(TrainingAge::Advanced).value, PeriodizationModel::Block);
        // PERIOD-001 is contested (CQ-03): no model hard-coded as superior.
        assert!(periodization_model(TrainingAge::Novice).confidence.contested);
    }

    #[test]
    fn taper_holds_intensity() {
        let t = taper_rx().value;
        assert_eq!(t.volume_reduction_frac, (0.41, 0.60));
        assert_eq!(t.duration_days, (8, 14));
        assert!(t.hold_intensity);
    }

    #[test]
    fn plyo_caps_rise_with_level() {
        use crate::individualization::TrainingAge;
        assert_eq!(plyo_foot_contact_cap(TrainingAge::Novice).value, (80, 100));
        assert_eq!(plyo_foot_contact_cap(TrainingAge::Advanced).value, (120, 140));
        assert!(plyo_foot_contact_cap(TrainingAge::Advanced).value.1 > plyo_foot_contact_cap(TrainingAge::Novice).value.1);
    }

    #[test]
    fn epley_100kg_5reps_is_about_116_7() {
        let e = e1rm_epley(100.0, 5);
        assert!((e - 116.666_666).abs() < 1e-3, "got {e}");
    }

    #[test]
    fn brzycki_sanity() {
        // 100 kg x 5 -> 100 * 36 / 32 = 112.5
        let b = e1rm_brzycki(100.0, 5);
        assert!((b - 112.5).abs() < 1e-6, "got {b}");
        // A single rep must equal (near) the load itself: 100 * 36 / 36 = 100.
        assert!((e1rm_brzycki(100.0, 1) - 100.0).abs() < 1e-6);
        // Higher reps estimate a higher 1RM than fewer reps at equal load.
        assert!(e1rm_brzycki(100.0, 8) > e1rm_brzycki(100.0, 3));
    }

    #[test]
    fn rpe_to_rir_anchor() {
        assert_eq!(rpe_to_rir(8.0), 2.0);
        assert_eq!(rpe_to_rir(10.0), 0.0);
        // Clamp: overshoot stays at 0 RIR.
        assert_eq!(rpe_to_rir(11.0), 0.0);
        // Round trip.
        assert_eq!(rir_to_rpe(2.0), 8.0);
        assert_eq!(rir_to_rpe(0.0), 10.0);
    }

    #[test]
    fn est_pct_from_reps_is_estimate() {
        // 1 rep -> 100 / (1 + 1/30) ~= 96.77%
        let p = est_pct_1rm_from_reps(1);
        assert!((p - 96.774).abs() < 1e-2, "got {p}");
        // Monotonic decrease with more reps.
        assert!(est_pct_1rm_from_reps(10) < est_pct_1rm_from_reps(3));
    }

    #[test]
    fn prilepin_for_70pct_returns_correct_row() {
        let row = prilepin_for(70.0).expect("70% resolves");
        assert_eq!(row.pct_min, 70);
        assert_eq!(row.pct_max, 79);
        assert_eq!(row.reps_per_set, (3, 6));
        assert_eq!(row.optimal_total, 18);
        assert_eq!(row.total_range, (12, 24));
        // Boundaries and out-of-range.
        assert_eq!(prilepin_for(95.0).unwrap().optimal_total, 7);
        assert_eq!(prilepin_for(50.0).unwrap().optimal_total, 24);
        assert!(prilepin_for(-1.0).is_none());
        assert!(prilepin_for(101.0).is_none());
    }

    #[test]
    fn prilepin_volume_pass_and_fail() {
        // 85% band range is 10-20 total reps.
        assert!(prilepin_volume_ok(85.0, 15)); // optimal
        assert!(prilepin_volume_ok(85.0, 10)); // lower bound
        assert!(prilepin_volume_ok(85.0, 20)); // upper bound
        assert!(!prilepin_volume_ok(85.0, 9)); // under
        assert!(!prilepin_volume_ok(85.0, 25)); // over
        assert!(!prilepin_volume_ok(200.0, 15)); // out of intensity range
    }

    #[test]
    fn prilepin_table_nonempty_and_sorted() {
        assert!(!PRILEPIN.is_empty());
        assert_eq!(PRILEPIN.len(), 4);
        // Ascending, non-overlapping, gap-free bands.
        for w in PRILEPIN.windows(2) {
            assert!(w[0].pct_max < w[1].pct_min);
            assert_eq!(w[0].pct_max + 1, w[1].pct_min);
        }
        // Full 0-100 coverage.
        assert_eq!(PRILEPIN.first().unwrap().pct_min, 0);
        assert_eq!(PRILEPIN.last().unwrap().pct_max, 100);
    }

    #[test]
    fn recommend_attaches_registered_evidence() {
        let rx = recommend(LiftDummy, "AUTOREG-RIR-001");
        assert_eq!(rx.evidence.grade, EvidenceGrade::Strong);
        assert_eq!(
            rx.evidence.citation.claim_id.as_deref(),
            Some("AUTOREG-RIR-001")
        );
        // Unregistered id falls back to ExpertOpinion, never panics.
        let fallback = recommend(LiftDummy, "NOT-A-REAL-ID");
        assert_eq!(fallback.evidence.grade, EvidenceGrade::ExpertOpinion);
    }

    #[derive(Debug, PartialEq)]
    struct LiftDummy;
}
