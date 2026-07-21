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
///
/// Constructible **only** via [`Recommended::new`]: the private zero-sized
/// `_sanctioned` field blocks struct literals (and functional-record updates)
/// outside this module, so every recommendation funnels through the one
/// constructor, which rejects `MarketingMyth` evidence unconditionally, in
/// release builds too (HARD RULE 2). Fields stay publicly *readable*; the
/// marker is `#[serde(skip)]`, so the serialized shape is unchanged
/// (`value`/`evidence`/`confidence`). `Deserialize` is hand-written below -
/// a derive would let wire JSON construct one without passing the myth check.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Recommended<T> {
    pub value: T,
    pub evidence: Evidence,
    pub confidence: ConfidenceTag,
    #[serde(skip)]
    _sanctioned: Sanctioned,
}

/// Private construction token for [`Recommended`]. Being private, it makes
/// `Recommended { .. }` literals impossible outside `schema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Sanctioned;

impl<T> Recommended<T> {
    /// Sole sanctioned constructor for a recommendation-bearing value.
    ///
    /// # Panics
    /// Unconditionally (release builds included) if `evidence` carries a
    /// [`EvidenceGrade::MarketingMyth`] grade: myth-graded claims are
    /// hard-blocked and must never be surfaced as advice (HARD RULE 2).
    pub fn new(value: T, evidence: Evidence, confidence: ConfidenceTag) -> Self {
        assert!(
            evidence.grade != EvidenceGrade::MarketingMyth,
            "MarketingMyth evidence must never back a recommendation (HARD RULE 2)"
        );
        Recommended {
            value,
            evidence,
            confidence,
            _sanctioned: Sanctioned,
        }
    }
}

/// Hand-written so the wire cannot smuggle a `MarketingMyth`-graded value past
/// the [`Recommended::new`] choke point: deserialization re-runs the same
/// check and fails with an error (not a panic) on myth-graded input.
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Recommended<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw<T> {
            value: T,
            evidence: Evidence,
            confidence: ConfidenceTag,
        }
        let raw = Raw::<T>::deserialize(deserializer)?;
        if raw.evidence.grade == EvidenceGrade::MarketingMyth {
            return Err(serde::de::Error::custom(
                "MarketingMyth evidence must never back a recommendation (HARD RULE 2)",
            ));
        }
        Ok(Recommended {
            value: raw.value,
            evidence: raw.evidence,
            confidence: raw.confidence,
            _sanctioned: Sanctioned,
        })
    }
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
    /// Pain report. Highest training-safety priority. A bare report (no
    /// [`ReadinessInput::pain`] detail) is treated as sharp/joint pain →
    /// conservative hard stop. With detail attached, the graded File 08
    /// Table 4.1 model applies (DOMS/tendon/structural).
    Pain,
    /// Single wellness soreness item on the 1–7 Hooper scale (File 06
    /// autoreg-030 second clause: ≥6/7 → downgrade intensity one level).
    Soreness,
    /// Fever/illness report. Absolute rest.
    Illness,
    /// RED-S / low-energy-availability red flag (amenorrhea, rapid weight loss,
    /// compulsive exercise, recurrent BSI). Absolute deferral, never a
    /// programming variable (File 08 safety-049; the KB's "safety-035"
    /// cross-refs are a numbering bug, no such block exists).
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
    /// Consecutive days/sessions (including this one) the signal's condition
    /// has held, shell-tallied. `0` (the wire default for shells that predate
    /// the field) means "not tracked" and is treated like a single observation.
    /// Multi-day rules (File 06): e1RM deload needs ≥2 sessions (autoreg-022),
    /// RHR downgrade needs ≥2 days (autoreg-040), the SubjectiveMultiDay tier
    /// needs ≥3 days of wellness suppression (§5 tier 4).
    #[serde(default)]
    pub streak: u8,
    /// Pain characterization for `signal == Pain` (File 08 Table 4.1,
    /// safety-038/039). `None`, the wire default, keeps the conservative
    /// pre-existing behavior: any pain report is a hard stop.
    #[serde(default)]
    pub pain: Option<PainDetail>,
    /// Duration in minutes of the continuous effort backing this observation,
    /// for signals derived from one effort. File 06 signal spec: aerobic
    /// decoupling is *valid only for efforts >20 min*. `None`, the wire
    /// default, means "duration not tracked" and keeps the pre-existing
    /// behavior; an explicitly short effort invalidates the decoupling signal.
    #[serde(default)]
    pub effort_min: Option<f64>,
}

/// Pain character/context per File 08 Table 4.1 (Silbernagel monitoring model).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PainKind {
    /// Sharp, localized, or joint-line pain; alters movement/gait; or occurs
    /// with swelling → possible structural injury (safety-038). Hard stop.
    SharpJoint,
    /// Load-related tendon pain → graded by severity/trend per the Silbernagel
    /// pain-monitoring model (safety-039). "Avoid complete rest."
    TendonLoadRelated,
    /// Muscle burn during a set / DOMS 24–72 h easing with movement → normal
    /// training discomfort; continue.
    Doms,
    /// Uncharacterized pain → conservative hard stop (same as a bare report).
    Other,
}

/// Week-to-week / post-session pain trajectory (File 08 Table 4.1).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PainTrend {
    /// Stable during & 24 h after; returns to baseline next morning.
    Stable,
    /// Worsening after sessions or rising week-to-week → reactive.
    Rising,
}

/// A characterized pain report attached to a `Pain` readiness input.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PainDetail {
    pub kind: PainKind,
    /// 0–10 numeric rating scale. Tendon tolerable band ≤5/10 (safety-039).
    pub severity: u8,
    pub trend: PainTrend,
    /// True when the pain has persisted across sessions despite modification -
    /// escalates STOP/REDUCE responses to DEFER (safety-038/039 "if persists").
    #[serde(default)]
    pub persists: bool,
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

/// Stage-0 onboarding health screen (File 08 onboard-050): deferral-relevant
/// flags collected BEFORE any prescription. All fields are self/shell-reported
/// booleans with serde defaults, so profiles persisted before this screen
/// existed parse unchanged (every flag `false` = no gate).
///
/// The KB defines the *gates*, not questionnaires: it does not enumerate the
/// PAR-Q+ question list (only that positive answers trigger the gate), gives no
/// numeric age cutoff for "child/adolescent" (safety-011), and no week-window
/// for "recent surgery" (safety-044/048), hence plain flags, no thresholds.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct HealthScreen {
    /// Child/adolescent user (File 08 safety-011). No autonomous maximal
    /// loading or 1RM testing; qualified supervision, technique-first.
    #[serde(default)]
    pub youth: bool,
    /// PAR-Q+/ACSM preparticipation screen positive: known cardiovascular,
    /// metabolic (diabetes), or renal disease while inactive and/or seeking
    /// vigorous intensity; or uncontrolled hypertension, recent surgery, or
    /// acute illness (File 08 safety-044).
    #[serde(default)]
    pub parq_positive: bool,
    /// Medical clearance obtained after a positive PAR-Q+/ACSM screen -
    /// clears the safety-044 gate only ("require medical clearance"). Other
    /// gates are cleared by unsetting their own flag once resolved (e.g.
    /// safety-048 injury: "resume general programming only upon clearance" →
    /// the shell clears `injury_or_rehab`); pregnancy (safety-045) requires
    /// provider clearance AND individualization, so the engine keeps deferring
    /// autonomous prescription for the whole pregnancy.
    #[serde(default)]
    pub medically_cleared: bool,
    /// Currently pregnant (File 08 safety-045/046/047).
    #[serde(default)]
    pub pregnant: bool,
    /// A pregnancy warning sign is present (File 08 safety-046, e.g. vaginal
    /// bleeding, dyspnea before exertion, chest pain, decreased fetal
    /// movement) → STOP and DEFER.
    #[serde(default)]
    pub pregnancy_warning_sign: bool,
    /// Current injury under care, recent surgery, or active rehab
    /// (File 08 safety-048). The engine never prescribes rehabilitation.
    #[serde(default)]
    pub injury_or_rehab: bool,
    /// RED-S / disordered-eating signal reported at screening (File 08
    /// safety-049 signal enum: amenorrhea/menstrual disturbance, rapid or
    /// excessive weight loss, compulsive exercise, chronic low intake,
    /// recurrent BSI, unexplained fatigue/underperformance, disordered-eating
    /// statements).
    #[serde(default)]
    pub reds_signal: bool,
}

impl HealthScreen {
    /// True when any deferral-relevant flag is raised (a positive PAR-Q+ that
    /// has since been medically cleared no longer gates, safety-044).
    pub fn any_gate(&self) -> bool {
        self.youth
            || (self.parq_positive && !self.medically_cleared)
            || self.pregnant
            || self.pregnancy_warning_sign
            || self.injury_or_rehab
            || self.reds_signal
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
    /// Cap today's session at planned RPE minus this many points (File 06
    /// autoreg-006 second clause: e1RM < baseline − 5 % → "cap session at
    /// planned RPE − 1" alongside the ~5 % top-set load cut).
    CapRpe(f32),
    /// Swap a hard session for an easy one.
    DowngradeSession,
    /// Modify the provoking exercise (reduce tendon load / compressive
    /// positions) and continue with monitoring, tolerable tendon pain ≤3–5/10,
    /// stable (Silbernagel model, File 08 safety-039). Explicitly NOT a rest:
    /// "avoid complete rest".
    ModifyAndMonitor,
    /// Insert a full rest day.
    RestDay,
    /// Non-negotiable stop (pain, fever, RHR +10 bpm). Safety override.
    Stop,
    /// Stop training and defer to a professional (physician / dietitian /
    /// mental-health). Emitted for medical red flags; `reason` names the trigger
    /// and referral target. Overrides all optimization output (File 08).
    Defer { reason: String },
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
    fn deserializing_a_myth_graded_recommended_is_rejected() {
        let valid = Recommended::new(
            1.0_f64,
            strong_evidence(),
            ConfidenceTag {
                score: EvidenceGrade::Strong.default_confidence(),
                contested: false,
                contested_question_ref: None,
                safety_critical: false,
            },
        );
        let mut json = serde_json::to_value(&valid).unwrap();

        // Round-trips while the grade is legitimate…
        let ok: Result<Recommended<f64>, _> = serde_json::from_value(json.clone());
        assert!(ok.is_ok());

        // …but the wire cannot smuggle a myth past the choke point.
        json["evidence"]["grade"] = serde_json::json!("MarketingMyth");
        let err = serde_json::from_value::<Recommended<f64>>(json).unwrap_err();
        assert!(err.to_string().contains("MarketingMyth"), "{err}");
    }

    #[test]
    fn recommended_wrapper_carries_evidence_and_confidence() {
        let rx = Recommended::new(
            Prescription::Lift(LiftPrescription {
                exercise: "Back squat".into(),
                sets: 5,
                reps: 5,
                intensity: LiftIntensity::PercentOneRm(80.0),
                rest_sec: 180,
                tempo: None,
                velocity_loss_pct: Some(20),
            }),
            strong_evidence(),
            ConfidenceTag {
                score: EvidenceGrade::Strong.default_confidence(),
                contested: false,
                contested_question_ref: None,
                safety_critical: false,
            },
        );

        assert_eq!(rx.evidence.grade, EvidenceGrade::Strong);
        assert!((rx.confidence.score - 0.90).abs() < f32::EPSILON);
    }

    #[test]
    #[should_panic(expected = "MarketingMyth evidence must never back a recommendation")]
    fn recommended_new_rejects_marketing_myths_unconditionally() {
        // HARD RULE 2 at the constructor choke point: deliberately NOT gated
        // on `debug_assertions` must also pass under `cargo test --release`.
        let myth = Evidence {
            grade: EvidenceGrade::MarketingMyth,
            citation: Citation {
                claim_id: Some("LOAD-ACWR-001".into()),
                reference: "Gabbett 2016, BJSM".into(),
            },
            contradicting: vec![],
        };
        let _ = Recommended::new(
            0u8,
            myth,
            ConfidenceTag {
                score: EvidenceGrade::MarketingMyth.default_confidence(),
                contested: true,
                contested_question_ref: Some("CQ-05".into()),
                safety_critical: false,
            },
        );
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
