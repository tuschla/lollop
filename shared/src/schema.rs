//! Core data schema: types only, no coaching logic.
//!
//! Sources: `knowledge-base/` (evidence-graded research files). Field ranges and
//! enums mirror File 09 (evidence/confidence), File 06 (autoregulation/readiness),
//! File 02 (strength prescription/periodization), File 04 (running prescription).
//!
//! Invariant: every recommendation-bearing value is wrapped in [`Recommended`],
//! which forces an attached [`Evidence`] and [`ConfidenceTag`]. Nothing that
//! tells a user what to do may exist outside that wrapper.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Evidence & confidence layer (knowledge-base File 09)
// ---------------------------------------------------------------------------

/// Canonical evidence grading scale (File 09).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvidenceGrade {
    /// Contradicted or retracted claims; must be hard-blocked, never programmed.
    MarketingMyth,
    /// Practice heuristic with no direct trial evidence.
    ExpertOpinion,
    /// Mechanistic or observational evidence only.
    Weak,
    /// Mixed or limited RCTs; promising but unsettled.
    Moderate,
    /// Well-replicated meta-analyses / RCTs.
    Strong,
}

impl EvidenceGrade {
    /// Default confidence score per grade (File 09 mapping).
    pub const fn default_confidence(self) -> f32 {
        match self {
            EvidenceGrade::Strong => 0.90,
            EvidenceGrade::Moderate => 0.65,
            EvidenceGrade::Weak => 0.40,
            EvidenceGrade::ExpertOpinion => 0.30,
            EvidenceGrade::MarketingMyth => 0.05,
        }
    }
}

/// A literature reference backing (or contradicting) a claim.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    /// Stable knowledge-base claim key, e.g. `"strength-intensity-001"`.
    pub claim_id: Option<String>,
    /// Human-readable reference: author/year + journal/DOI.
    pub reference: String,
}

/// Evidence attached to a recommendation. Non-negotiable on every
/// recommendation-bearing type (see [`Recommended`]).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Evidence {
    pub grade: EvidenceGrade,
    pub citation: Citation,
    /// Named counter-evidence, if the claim is contested.
    pub contradicting: Vec<Citation>,
}

/// Confidence tag (File 09 `ConfidenceTag`), attached alongside [`Evidence`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ConfidenceTag {
    /// 0.05–0.90; defaults derived from grade via [`EvidenceGrade::default_confidence`].
    pub score: f32,
    /// True when linked to a contested-question row (e.g. `"CQ-01"`).
    pub contested: bool,
    pub contested_question_ref: Option<String>,
    /// Safety-critical claims override optimization concerns.
    pub safety_critical: bool,
}

/// Wrapper forcing evidence + confidence onto any recommendation-bearing value.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Recommended<T> {
    pub value: T,
    pub evidence: Evidence,
    pub confidence: ConfidenceTag,
}

// ---------------------------------------------------------------------------
// Program hierarchy: Program → Mesocycle → Session → Prescription
// ---------------------------------------------------------------------------

/// Top-level training goal.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Goal {
    Strength,
    Hypertrophy,
    Power,
    RunningRace {
        distance_km: f32,
    },
    GeneralEndurance,
    /// Concurrent resistance + running.
    Hybrid,
}

/// A full training program.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Program {
    pub id: String,
    pub name: String,
    pub goal: Goal,
    pub mesocycles: Vec<Mesocycle>,
}

/// Mesocycle phase (File 02 block model; File 04 base/build/peak/taper).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MesoPhase {
    /// Volume-dominant accumulation / aerobic base.
    Base,
    /// Intensification / transmutation (T-, I-work; 80–90 %1RM).
    Build,
    /// Realization / race- or peak-specific work.
    Peak,
    /// Volume −41–60 %, intensity + frequency maintained (Bosquet 2007).
    Taper,
    /// Planned recovery week (default 3:1 load:recovery).
    Deload,
}

/// A block of weeks with one dominant training emphasis.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Mesocycle {
    pub phase: MesoPhase,
    pub weeks: u8,
    pub sessions: Vec<Session>,
}

/// Lifting session types (File 02 §6).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiftSessionType {
    MaxEffort,
    DynamicEffort,
    Repetition,
    Accessory,
}

/// Running session types (File 04 §3).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunSessionType {
    Recovery,
    LongRun,
    Tempo,
    Interval,
    Repetition,
    Strides,
    Hills,
    RacePace,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    Lift(LiftSessionType),
    Run(RunSessionType),
    Rest,
}

/// One planned training day/unit inside a mesocycle.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Session {
    pub session_type: SessionType,
    /// Day offset within the mesocycle, 0-based.
    pub day: u16,
    /// Every prescription carries evidence + confidence by construction.
    pub prescriptions: Vec<Recommended<Prescription>>,
}

// ---------------------------------------------------------------------------
// Prescriptions
// ---------------------------------------------------------------------------

/// Proximity-to-failure / load anchor for a lift set (File 02).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum LiftIntensity {
    /// 65–100.
    PercentOneRm(f32),
    /// Borg CR-10, 5.0–10.0 (Zourdos mapping: RPE 10 = 0 RIR).
    Rpe(f32),
    /// Reps in reserve, 0–10.
    Rir(u8),
    /// Mean concentric velocity target, m/s (0.15–1.30, exercise-specific).
    VelocityMs(f32),
}

/// A prescribed lift: the set/rep/load/rest contract for one exercise.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LiftPrescription {
    pub exercise: String,
    pub sets: u8,
    /// Target reps per set (1–12+).
    pub reps: u8,
    pub intensity: LiftIntensity,
    /// Between-set rest, 30–300 s.
    pub rest_sec: u16,
    /// Eccentric-pause-concentric, e.g. `"2-1-3"`.
    pub tempo: Option<String>,
    /// End set when velocity drops this % from fastest rep (10–40).
    pub velocity_loss_pct: Option<u8>,
}

/// Running intensity, in whichever zone model anchors the session (File 04 §1).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum RunIntensity {
    /// 3-zone lactate model: Z1 < LT1, Z2 = LT1–LT2, Z3 > LT2.
    ThreeZone(ThreeZone),
    /// VDOT 5-band prescription model.
    Vdot(VdotBand),
    /// % of HRmax (50–100).
    HrPercentMax(f32),
    /// Pace in seconds per km.
    PaceSecPerKm(u16),
    /// % of critical power / FTP.
    PowerPercentCp(f32),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreeZone {
    Z1,
    Z2,
    Z3,
}

/// Daniels VDOT bands: E 59–74 %VO2max, M 80–84, T 83–88, I 95–100, R >100.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum VdotBand {
    Easy,
    Marathon,
    Threshold,
    Interval,
    Repetition,
}

/// Session volume target: exactly one of duration or distance anchors the run.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum RunVolume {
    DurationMin(u16),
    DistanceKm(f32),
}

/// A prescribed run (continuous or as repeats).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RunPrescription {
    pub volume: RunVolume,
    pub intensity: RunIntensity,
    /// For interval/repetition work: (rep count, rep volume).
    pub repeats: Option<(u8, RunVolume)>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Prescription {
    Lift(LiftPrescription),
    Run(RunPrescription),
}

// ---------------------------------------------------------------------------
// Autoregulation hooks: readiness inputs → adjustments (File 06)
// ---------------------------------------------------------------------------

/// Normalized readiness signals the engine consumes (File 06 §2).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessSignal {
    /// Session RPE vs. target.
    Rpe,
    /// Estimated 1RM vs. 14-day rolling best.
    EstimatedOneRm,
    /// Mean concentric velocity at reference load, m/s.
    BarVelocity,
    /// Within-set velocity loss, %.
    VelocityLoss,
    /// 5-item wellness composite (sleep/fatigue/soreness/stress/mood), z-score.
    WellnessZ,
    /// lnRMSSD 7-day rolling vs. SWC band (baseline ± 0.5 SD).
    HrvLnRmssd,
    /// HRV coefficient of variation, % (high = unreliable signal).
    HrvCv,
    /// Aerobic decoupling on Z1/Z2 runs > 20 min, %.
    AerobicDecoupling,
    /// Morning resting HR vs. baseline, bpm.
    RestingHr,
    /// Joint/sharp pain report. Highest safety priority.
    Pain,
    /// Fever/illness report. Absolute rest.
    Illness,
    /// RED-S / low-energy-availability red flag (amenorrhea, rapid weight loss,
    /// compulsive exercise, recurrent BSI). Absolute deferral, never a
    /// programming variable (File 08 safety-035/049).
    RedS,
    /// Cardiovascular red-flag symptom (chest pain, syncope, unexplained
    /// dyspnea, palpitations). Stop + defer for medical clearance (File 08
    /// safety-043).
    CardiacRedFlag,
    /// Bone-stress-injury signs (pinpoint tenderness, night pain, pain with
    /// impact). Stop impact loading + urgent referral (File 08 safety-040).
    BoneStress,
}

/// One observed readiness data point.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ReadinessInput {
    pub signal: ReadinessSignal,
    pub value: f64,
    /// Unix seconds.
    pub observed_at: i64,
}

/// Neck-check illness classification (File 06). Encoded in a
/// [`ReadinessInput`] whose `signal == ReadinessSignal::Illness`, this decodes
/// the numeric `value` convention (0 = none, 1 = above-neck, ≥2 = below-neck /
/// any fever) into a self-documenting category, the autoregulation layer
/// branches on the enum, never on raw float literals.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum IllnessSeverity {
    /// No illness signal → no adjustment.
    None,
    /// Above-neck only (congestion, sore throat), no fever → downgrade session.
    AboveNeck,
    /// Below-neck symptoms or any fever → absolute no-train.
    BelowNeckOrFever,
}

impl IllnessSeverity {
    /// Decode the `ReadinessInput.value` convention via range checks (no float
    /// equality): `value >= 2.0` → below-neck/fever, `>= 1.0` → above-neck,
    /// else none. Intermediate values (e.g. `1.5`) round down to the milder tier.
    pub fn from_value(value: f64) -> Self {
        if value >= 2.0 {
            IllnessSeverity::BelowNeckOrFever
        } else if value >= 1.0 {
            IllnessSeverity::AboveNeck
        } else {
            IllnessSeverity::None
        }
    }
}

/// Safety priority ladder (File 06 §5). Higher tier always overrides lower.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SafetyTier {
    SingleDayMarker,
    HrvTrend,
    SubjectiveMultiDay,
    ObjectivePerformance,
    Illness,
    Pain,
    /// Medical red flag (RED-S, cardiovascular symptom, bone stress injury):
    /// stop + defer to a professional. Overrides even training-pain adjustments
    /// (File 08 §5 safety-040/043/049).
    MedicalReferral,
}

/// Adjustments the autoregulation layer can emit (File 06 §4).
/// Always shipped as `Recommended<Adjustment>`, evidence attached.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Adjustment {
    /// Reduce load for remaining sets by this % (e.g. RPE ≥ target + 2).
    ReduceLoadPct(f32),
    /// Increase load by this % when readiness overshoots (RPE under target, or
    /// e1RM above baseline). File 06 autoreg-004/005/007.
    IncreaseLoadPct(f32),
    /// Deload: volume −40–50 %, load −5–10 %, typically 1 week.
    Deload {
        volume_reduction_pct: f32,
        load_reduction_pct: f32,
        weeks: u8,
    },
    /// Swap a hard session for an easy one.
    DowngradeSession,
    /// Insert a full rest day.
    RestDay,
    /// Non-negotiable stop (pain, fever, RHR +10 bpm). Safety override.
    Stop,
    /// Stop training and defer to a professional (physician / dietitian /
    /// mental-health). Emitted for medical red flags; `reason` names the trigger
    /// and referral target. Overrides all optimization output (File 08).
    Defer {
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strong_evidence() -> Evidence {
        Evidence {
            grade: EvidenceGrade::Strong,
            citation: Citation {
                claim_id: Some("taper-volume-001".into()),
                reference: "Bosquet et al. 2007, Med Sci Sports Exerc".into(),
            },
            contradicting: vec![],
        }
    }

    #[test]
    fn recommended_wrapper_carries_evidence_and_confidence() {
        let rx = Recommended {
            value: Prescription::Lift(LiftPrescription {
                exercise: "Back squat".into(),
                sets: 5,
                reps: 5,
                intensity: LiftIntensity::PercentOneRm(80.0),
                rest_sec: 180,
                tempo: None,
                velocity_loss_pct: Some(20),
            }),
            evidence: strong_evidence(),
            confidence: ConfidenceTag {
                score: EvidenceGrade::Strong.default_confidence(),
                contested: false,
                contested_question_ref: None,
                safety_critical: false,
            },
        };

        assert_eq!(rx.evidence.grade, EvidenceGrade::Strong);
        assert!((rx.confidence.score - 0.90).abs() < f32::EPSILON);
    }

    #[test]
    fn safety_ladder_orders_pain_above_everything() {
        assert!(SafetyTier::Pain > SafetyTier::Illness);
        assert!(SafetyTier::Illness > SafetyTier::ObjectivePerformance);
        assert!(SafetyTier::ObjectivePerformance > SafetyTier::SubjectiveMultiDay);
        assert!(SafetyTier::SubjectiveMultiDay > SafetyTier::HrvTrend);
        assert!(SafetyTier::HrvTrend > SafetyTier::SingleDayMarker);
    }

    #[test]
    fn illness_severity_decodes_neck_check_convention() {
        assert_eq!(IllnessSeverity::from_value(0.0), IllnessSeverity::None);
        assert_eq!(IllnessSeverity::from_value(1.0), IllnessSeverity::AboveNeck);
        // Intermediate rounds down to the milder tier.
        assert_eq!(IllnessSeverity::from_value(1.5), IllnessSeverity::AboveNeck);
        assert_eq!(
            IllnessSeverity::from_value(2.0),
            IllnessSeverity::BelowNeckOrFever
        );
        assert_eq!(
            IllnessSeverity::from_value(3.0),
            IllnessSeverity::BelowNeckOrFever
        );
    }

    #[test]
    fn grade_confidence_defaults_match_file_09() {
        assert_eq!(EvidenceGrade::MarketingMyth.default_confidence(), 0.05);
        assert_eq!(EvidenceGrade::Moderate.default_confidence(), 0.65);
    }
}
