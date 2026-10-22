//! Core data schema: types only, no coaching logic.
//!
//! Sources: `knowledge-base/` (evidence-graded research files). Field ranges and
//! enums mirror File 09 (evidence/confidence), File 06 (autoregulation/readiness),
//! File 02 (strength prescription/periodization), File 04 (running prescription).
//!
//! Invariant: every recommendation-bearing value is wrapped in [`Recommended`],
//! which forces an attached registry claim (grade, citation, confidence).
//! Nothing that tells a user what to do may exist outside that wrapper.

use crate::evidence::{ClaimId, EvidenceEntry};
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

/// Wrapper forcing evidence + confidence onto any recommendation-bearing value.
///
/// The backing claim is private and set only by [`Recommended::new`], which
/// rejects `MarketingMyth` claims unconditionally, in release builds too
/// (HARD RULE 2), so no code path can point a recommendation at a myth.
/// `value` is the payload plan rewrites edit in place; grade, citation,
/// confidence and contested question are read off the claim's registry row,
/// so a recommendation costs no allocation beyond its value.
#[must_use]
#[derive(Debug, Clone, PartialEq)]
pub struct Recommended<T> {
    pub value: T,
    claim: ClaimId,
}

impl<T> Recommended<T> {
    /// Sole constructor for a recommendation-bearing value.
    ///
    /// # Panics
    /// Unconditionally (release builds included) if `claim` is graded
    /// [`EvidenceGrade::MarketingMyth`]: myth claims are hard-blocked and must
    /// never be surfaced as advice (HARD RULE 2).
    pub fn new(value: T, claim: ClaimId) -> Self {
        assert!(
            !claim.entry().is_blocked(),
            "MarketingMyth claim {} must never back a recommendation (HARD RULE 2)",
            claim.entry().claim_id,
        );
        Recommended { value, claim }
    }

    /// The claim backing this recommendation.
    pub fn claim_id(&self) -> ClaimId {
        self.claim
    }

    /// The registry row backing this recommendation.
    pub fn evidence(&self) -> &'static EvidenceEntry {
        self.claim.entry()
    }

    pub fn grade(&self) -> EvidenceGrade {
        self.evidence().grade
    }

    /// The citation line shown for the backing claim.
    pub fn reference(&self) -> &'static str {
        self.evidence().reference()
    }

    /// 0.05–0.90 confidence, the grade's File 09 default.
    pub fn confidence(&self) -> f32 {
        self.evidence().confidence_score()
    }

    /// Safety-critical claims override optimization concerns.
    pub fn safety_critical(&self) -> bool {
        self.evidence().safety_critical
    }

    /// The contested-question id (e.g. `"CQ-01"`) when the claim is debated.
    pub fn contested(&self) -> Option<&'static str> {
        self.evidence().contested
    }

    /// The same claim backing a value derived from this one.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Recommended<U> {
        Recommended {
            value: f(self.value),
            claim: self.claim,
        }
    }

    /// The same claim backing another value.
    pub fn with_value<U>(&self, value: U) -> Recommended<U> {
        Recommended {
            value,
            claim: self.claim,
        }
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

/// A full training program: the plan week in force, as one mesocycle.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub name: String,
    pub goal: Goal,
    pub mesocycle: Mesocycle,
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
#[derive(Debug, Clone, PartialEq)]
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
    /// A bodyweight (calisthenics) day: leverage-tier ladders, not %1RM
    /// barbell loading. Kept under `SessionType::Lift(_)` so the shell's
    /// discipline parsing (keys on the `Lift` prefix) is unchanged, but titled
    /// honestly rather than borrowing a barbell heavy/power/volume label.
    Calisthenics,
    /// A uniform full-body strength day (novice linear progression, or a day
    /// prescribed by effort rather than undulating %1RM): titled by what it
    /// actually prescribes ("Strength day"), never a borrowed DUP label.
    FullBody,
}

/// Running session types (File 04 §3).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunSessionType {
    /// A true recovery run (running-014: <76 %HRmax, 20–40 min), placed the
    /// day after a hard session or long run.
    Recovery,
    LongRun,
    Tempo,
    Interval,
    Repetition,
    Strides,
    Hills,
    RacePace,
    /// An easy / general-aerobic run (running-015: 65–79 %HRmax, 30–90 min),
    /// the default non-quality run day.
    Easy,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    Lift(LiftSessionType),
    Run(RunSessionType),
    #[default]
    Rest,
}

impl SessionType {
    /// Every session type, for decoding a [`wire_name`](Self::wire_name).
    pub const ALL: [SessionType; 16] = {
        use LiftSessionType as L;
        use RunSessionType as R;
        [
            SessionType::Lift(L::MaxEffort),
            SessionType::Lift(L::DynamicEffort),
            SessionType::Lift(L::Repetition),
            SessionType::Lift(L::Accessory),
            SessionType::Lift(L::Calisthenics),
            SessionType::Lift(L::FullBody),
            SessionType::Run(R::Recovery),
            SessionType::Run(R::LongRun),
            SessionType::Run(R::Tempo),
            SessionType::Run(R::Interval),
            SessionType::Run(R::Repetition),
            SessionType::Run(R::Strides),
            SessionType::Run(R::Hills),
            SessionType::Run(R::RacePace),
            SessionType::Run(R::Easy),
            SessionType::Rest,
        ]
    };

    /// Stable wire name of a planned day; the shell keys its discipline on the
    /// `"Lift"` / `"Run"` prefix.
    pub fn wire_name(self) -> &'static str {
        use LiftSessionType as L;
        use RunSessionType as R;
        match self {
            SessionType::Lift(L::MaxEffort) => "Lift(MaxEffort)",
            SessionType::Lift(L::DynamicEffort) => "Lift(DynamicEffort)",
            SessionType::Lift(L::Repetition) => "Lift(Repetition)",
            SessionType::Lift(L::Accessory) => "Lift(Accessory)",
            SessionType::Lift(L::Calisthenics) => "Lift(Calisthenics)",
            SessionType::Lift(L::FullBody) => "Lift(FullBody)",
            SessionType::Run(R::Recovery) => "Run(Recovery)",
            SessionType::Run(R::LongRun) => "Run(LongRun)",
            SessionType::Run(R::Tempo) => "Run(Tempo)",
            SessionType::Run(R::Interval) => "Run(Interval)",
            SessionType::Run(R::Repetition) => "Run(Repetition)",
            SessionType::Run(R::Strides) => "Run(Strides)",
            SessionType::Run(R::Hills) => "Run(Hills)",
            SessionType::Run(R::RacePace) => "Run(RacePace)",
            SessionType::Run(R::Easy) => "Run(Easy)",
            SessionType::Rest => "Rest",
        }
    }

    /// The session type whose [`wire_name`](Self::wire_name) is `name`.
    pub fn from_wire_name(name: &str) -> Option<SessionType> {
        Self::ALL.into_iter().find(|t| t.wire_name() == name)
    }
}

/// One planned training day/unit inside a mesocycle.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub session_type: SessionType,
    /// Day offset within the mesocycle, 0-based.
    pub day: u16,
    /// Every prescription carries evidence + confidence by construction.
    pub prescriptions: Vec<Recommended<Prescription>>,
    /// The DUP day a lift session undulates to (File 02 strength-023), so two
    /// heavy days of one week read apart. `None` for any other session.
    pub dup_day: Option<crate::strength::DupDay>,
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
    /// A ballistic bodyweight movement (jumps): every rep at maximal intent
    /// (jump as high as possible), no load and no effort target — the stimulus
    /// is the intent itself (STR-POWER-SELECT-001).
    MaxIntent,
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
    /// End set when velocity drops this % from fastest rep (10–40).
    pub velocity_loss_pct: Option<u8>,
    /// Top of the target rep RANGE when the lift is prescribed as a range
    /// (hypertrophy double progression: work from `reps` up to `reps_max`,
    /// then add load). `None` = a fixed rep target of `reps`.
    #[serde(default)]
    pub reps_max: Option<u8>,
    /// Explicit working load in kg carried forward from the athlete's own last
    /// working sets (hypertrophy double progression: same load, or the
    /// smallest increment once the top of the range was reached). `None` =
    /// no explicit load; a `PercentOneRm` intensity still resolves against the
    /// e1RM anchor at render time.
    #[serde(default)]
    pub load_kg: Option<f32>,
}

/// Running intensity, in whichever zone model anchors the session (File 04 §1).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum RunIntensity {
    /// 3-zone lactate model: Z1 < LT1, Z2 = LT1–LT2, Z3 > LT2.
    ThreeZone(ThreeZone),
    /// VDOT 5-band prescription model.
    Vdot(VdotBand),
    /// %HRmax band (percent units, 50–100). At least one of `lo`/`hi` is set;
    /// `hi`-only renders as a ceiling, `lo`-only as a floor, both as a range.
    /// `hrr_hi` is the Karvonen %HRR ceiling appended when the KB states one.
    HrPercentMax {
        lo: Option<f32>,
        hi: Option<f32>,
        hrr_hi: Option<f32>,
    },
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

/// Session volume target: a duration, a distance, or a beginner run/walk
/// interval set (running-025 C25K: `rounds` repeats of `run_sec` running then
/// `walk_sec` walking).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum RunVolume {
    DurationMin(u16),
    DistanceKm(f32),
    RunWalk {
        run_sec: u16,
        walk_sec: u16,
        rounds: u8,
    },
}

/// One run/walk bout of a beginner session (NHS Couch to 5K): run for
/// `run_sec`, then walk for `walk_sec` (0 on the final bout).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSegment {
    pub run_sec: u16,
    pub walk_sec: u16,
}

/// Strides added to the end of an easy run (running-020): `reps` × `rep_sec`
/// controlled-fast efforts with `recovery_sec` of easy jogging/walking between.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StridesAddOn {
    pub reps: u8,
    pub rep_sec: u16,
    pub recovery_sec: u16,
}

/// A prescribed run (continuous or as repeats). Every field after `repeats` is
/// serde-default so older persisted/wire rows decode unchanged.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RunPrescription {
    pub volume: RunVolume,
    pub intensity: RunIntensity,
    /// For interval/repetition work: (rep count, rep volume).
    pub repeats: Option<(u8, RunVolume)>,
    /// Recovery between reps; `None` for a continuous run.
    #[serde(default)]
    pub recovery: Option<RepRecovery>,
    /// Easy warm-up before the main set, minutes (C25K: the brisk walk).
    #[serde(default)]
    pub warmup_min: Option<u16>,
    /// Easy cool-down after the main set, minutes.
    #[serde(default)]
    pub cooldown_min: Option<u16>,
    /// Ordered run/walk bouts of a beginner session (C25K), the last bout's
    /// walk 0; empty for every other run.
    #[serde(default)]
    pub segments: Vec<RunSegment>,
    /// Strides to finish an easy run with; `None` = no strides.
    #[serde(default)]
    pub strides: Option<StridesAddOn>,
}

impl RunPrescription {
    /// A run with only the core volume/intensity/repeats set; every structure
    /// field (recovery, warm-up, cool-down, bouts, strides) empty.
    pub fn new(
        volume: RunVolume,
        intensity: RunIntensity,
        repeats: Option<(u8, RunVolume)>,
    ) -> Self {
        RunPrescription {
            volume,
            intensity,
            repeats,
            recovery: None,
            warmup_min: None,
            cooldown_min: None,
            segments: Vec::new(),
            strides: None,
        }
    }
}

/// Easy-jog recovery between interval / repetition reps.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepRecovery {
    /// A known number of seconds.
    Seconds(u16),
    /// An easy-jog DISTANCE, metres (repetition work: jog as far as the rep).
    Metres(u16),
    /// An easy jog lasting as long as the rep took (VO2max intervals with no
    /// pace anchor to turn the rep into seconds).
    EqualToRep,
}

/// Identifies one calisthenics leverage ladder (see `crate::calisthenics`
/// `LADDERS`). A plain data enum: `schema` owns the shared wire types so it need
/// not depend on `evidence`/`calisthenics`; `calisthenics.rs` re-exports this.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LadderId {
    // dynamic resistance ladders (calis-progression.md §3.1-3.8)
    HorizontalPush,
    Dip,
    VerticalPush,
    HorizontalPull,
    VerticalPull,
    Squat,
    Hinge,
    Core,
    // skill tracks (§3.3 skill row, §3.9, §3.10)
    Handstand,
    FrontLever,
    Planche,
    BackLever,
    MuscleUp,
    // power (ballistic) ladders for a bodyweight Power goal (CAL-POWER-001,
    // CAL-POWER-LADDER-001; 13-calisthenics-addendum cal-add-005/006)
    Jump,
    PlyoPush,
}

/// An intra-tier tempo bridge (CAL-PROG-TEMPO-001 / CAL-PROG-BRIDGE-001): a slow
/// lowering plus a pause at the hardest point, used on the easier rung after a
/// failed step up so the jump back is smaller. Total time per rep stays within
/// the 0.5–8 s window the evidence supports.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TempoBridge {
    pub eccentric_sec: u8,
    pub pause_sec: u8,
}

/// A per-rung working band: reps for dynamic rungs, hold seconds for timed
/// (skill/core) rungs. The band top doubles as the tier-advance trigger.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierTarget {
    Reps { low: u8, high: u8 },
    HoldSec { low: u16, high: u16 },
}

/// A prescribed calisthenics rung: the tier/target/RIR/rest contract for one
/// pattern. Load is the leverage tier (not a bar weight); `added_kg` is set only
/// on one of the ladder's `weighted_rungs`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CalisthenicsPrescription {
    pub ladder: LadderId,
    pub tier: u8,
    /// Canonical tier name (denormalized so shells need no ladder table).
    pub exercise: String,
    pub sets: u8,
    /// The WORKING band for this goal (hypertrophy widens rep rungs; skill holds
    /// prescribe the submaximal working hold, with the advance test separate in
    /// `test_hold_sec`).
    pub target: TierTarget,
    /// Rung-dependent RIR band (low, high) from `calisthenics::rir_band`.
    /// Not used for a `ballistic` item: power work states no numeric RIR (only
    /// "never to failure", STR-PWR-001), so the view renders that cue instead.
    pub rir: (u8, u8),
    pub rest_sec: u16,
    /// Added external load, kg — `Some` only on a ladder's weighted rung.
    pub added_kg: Option<f64>,
    /// The rung accepts external load (one of the ladder's `weighted_rungs`), even while
    /// the suggested load is 0 kg, so the shell shows the added-load field.
    #[serde(default)]
    pub load_capable: bool,
    /// Tempo bridge on this rung (after a failed step up), if any.
    #[serde(default)]
    pub tempo_bridge: Option<TempoBridge>,
    /// Straight-arm skill holds: the clean test-hold seconds that advance the
    /// rung (one test hold at the end, once a week); `target` is the working hold.
    #[serde(default)]
    pub test_hold_sec: Option<u16>,
    /// Ballistic power item (jumps / plyometric push-ups): maximal intent,
    /// never to failure.
    #[serde(default)]
    pub ballistic: bool,
    /// Nearest easier / harder rung the athlete's equipment allows.
    #[serde(default)]
    pub easier: Option<u8>,
    #[serde(default)]
    pub harder: Option<u8>,
    /// The newest session hit the advance target but its last set was not
    /// rated, so the rung cannot advance yet.
    #[serde(default)]
    pub needs_effort_rating: bool,
    /// Rungs stepped down after a break from this exercise (CAL-REENTRY-001).
    #[serde(default)]
    pub reentry_steps: u8,
    /// The break was long enough (> 8 weeks) to re-test the starting rung.
    #[serde(default)]
    pub retest: bool,
    /// The plain rung needs equipment the athlete lacks; this is the nearest
    /// rung that does not (CAL-EQUIP-001).
    #[serde(default)]
    pub substituted: bool,
    /// No set logged and no rung chosen for this ladder yet.
    #[serde(default)]
    pub unplaced: bool,
    /// A horizontal row with no bar or rings in the athlete's kit: done under a
    /// sturdy table, gripping its edge (CAL-EQUIP-001 "table/inverted rows for
    /// pulling").
    #[serde(default)]
    pub table_row: bool,
    /// Reps on every set that mark the rung as far too easy, so the plan skips
    /// a step after one session (CAL-PLACE-001); `None` where skipping never
    /// applies (loaded, straight-arm, leave-RIR, timed, ballistic, last rung).
    #[serde(default)]
    pub skip_reps: Option<u16>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Prescription {
    Lift(LiftPrescription),
    Run(RunPrescription),
    Calisthenics(CalisthenicsPrescription),
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
    /// decoupling is *valid only for efforts >20 min*, so the downgrade fires
    /// only when duration is KNOWN valid. `None`, the wire default, means
    /// "duration not tracked": it is not known-valid, so the decoupling signal
    /// is discarded (no downgrade), as is an explicitly short effort.
    #[serde(default)]
    pub effort_min: Option<f64>,
}

/// One morning (or pre-session) check-in: raw *human* observations only. Every
/// scored field is optional; the user answers what they know and the core never
/// fabricates an unanswered item (HARD RULE 1). The core normalizes these into
/// the z-scores / deltas the autoregulation rules consume, so the user never
/// has to supply a z-score they cannot know.
///
/// The 1–5 wellness items are the KB's 5-item short-form wellness composite
/// (`ReadinessSignal::WellnessZ`: sleep/soreness/fatigue/stress/mood,
/// direction-normalized in the core); no new instrument is invented. The scale
/// is deliberately 1–5 (not the Hooper 1–7 index) to match the check-in UI's
/// identical scale rows. Additive on the wire: every field is `#[serde(default)]`
/// so a check-in written by any app version replays byte-for-byte.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct CheckinInput {
    /// Unix seconds, shell-supplied (the core holds no clock). Day-buckets the
    /// rolling baseline; `<= 0` readings are ignored by the derivation.
    #[serde(default)]
    pub observed_at: i64,
    /// Sleep quality, 1–5 (5 = slept great). Higher = better.
    #[serde(default)]
    pub sleep_quality: Option<u8>,
    /// Muscle soreness today, 1–5 (5 = very sore). Higher = worse, the core
    /// reverse-scores it into the "higher = better" wellness composite.
    #[serde(default)]
    pub soreness: Option<u8>,
    /// Mood / motivation, 1–5 (5 = great mood). Higher = better. Perceived stress
    /// is a SEPARATE item (`stress`), not folded in here.
    #[serde(default)]
    pub mood: Option<u8>,
    /// Fatigue today, 1–5 (5 = utterly wiped). Higher = worse (same direction as
    /// soreness); the core reverse-scores it into the "higher = better" wellness
    /// composite. KB 5-item short-form item (autoreg WellnessZ).
    #[serde(default)]
    pub fatigue: Option<u8>,
    /// Perceived stress today, 1–5 (5 = extremely stressed). Higher = worse; the
    /// core reverse-scores it. Distinct from `mood` (affect/motivation, scored in
    /// the good direction). KB 5-item short-form item (autoreg WellnessZ).
    #[serde(default)]
    pub stress: Option<u8>,
    /// Morning resting HR, bpm (from a watch/manual pulse). The core computes
    /// the vs-baseline delta the RHR rules read (autoreg-040/041).
    #[serde(default)]
    pub resting_hr_bpm: Option<f64>,
    /// Morning HRV as raw rMSSD in ms (from a watch/app). The core takes
    /// `ln(rmssd)` and the rolling-baseline z the HRV rule reads (autoreg-028).
    #[serde(default)]
    pub hrv_rmssd_ms: Option<f64>,
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
    /// Pain that worsens in the 24 h AFTER load even if stable during → reactive
    /// (Silbernagel monitoring, File 08 Table 4.1). Treated like `Rising`.
    WorseningAfter24h,
}

/// A characterized pain report attached to a `Pain` readiness input.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PainDetail {
    pub kind: PainKind,
    /// 0–10 numeric rating scale. Tendon tolerable band ≤5/10 (safety-039).
    /// `None` = the user gave no rating; graded conservatively (outside the
    /// tolerable band). Old logs carrying a bare number decode to `Some`.
    #[serde(default)]
    pub severity: Option<u8>,
    pub trend: PainTrend,
    /// True when the pain has persisted across sessions despite modification -
    /// escalates STOP/REDUCE responses to DEFER (safety-038/039 "if persists").
    #[serde(default)]
    pub persists: bool,
    /// Free-text body-part location (e.g. `"Left knee"`) for display context on
    /// the safety banner sub-line and pain history. For a non-persisting
    /// sharp/joint report it also scopes the stop: a location that maps to one
    /// body region stops only the movements that load it (File 08 safety-038
    /// "STOP that exercise"; `autoreg::pain_region`); anything unmapped keeps the
    /// whole-day stop. `None`, the wire default, keeps every existing log
    /// replaying unchanged (whole-day stop, no body part on the sub-line).
    #[serde(default)]
    pub location: Option<String>,
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
/// File 08 defines the *gates*: it gives no numeric age cutoff for
/// "child/adolescent" (safety-011) and no week-window for "recent surgery"
/// (safety-044/048), hence plain flags, no thresholds. The seven PAR-Q+ items
/// themselves come from File 12 (onboard-051/052; [`HealthScreen::parq_yes`]).
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
    /// PAR-Q+ symptom answer (Q2 chest pain, or Q3 dizziness / fainting), at
    /// rest or with exertion. STOP + DEFER for medical clearance, and NOT
    /// cleared by `medically_cleared` (SCREEN-ACSM-001: symptomatic people stop
    /// and get cleared "whatever their activity level"); the shell clears it
    /// only by re-answering the questions. Wire default `false` keeps old
    /// profiles parsing.
    #[serde(default)]
    pub symptomatic: bool,
    /// Currently acutely ill (e.g. fever or infection) at screening: training
    /// is deferred until the user reports it resolved (ILLNESS-NECK-001). Wire
    /// default `false` keeps old profiles parsing.
    #[serde(default)]
    pub acute_illness: bool,
    /// The seven PAR-Q+ answers as a bitmask (bit0 = Q1 … bit6 = Q7). The core
    /// derives the gates from it as well as from the flags (see
    /// [`HealthScreen::parq_followup_uncleared`], [`HealthScreen::symptomatic_any`],
    /// [`HealthScreen::injury_any`]), so a shell that sends the answers but
    /// forgets a flag cannot bypass a gate.
    #[serde(default)]
    pub parq_yes: u8,
    /// The follow-up answers ([`PARQ_FOLLOWUP_BITS`]) that the medical clearance
    /// covers. A Yes added after clearance is not covered and gates again
    /// (safety-044: positive answers need clearance). `0` with
    /// `medically_cleared` is a clearance saved before this field existed and
    /// covers every current follow-up answer.
    #[serde(default)]
    pub cleared_parq_mask: u8,
}

/// PAR-Q+ Q1 (heart / blood pressure), Q4 (other condition), Q5 (medication),
/// Q7 (supervised activity only): doctor follow-up items, clearable.
pub const PARQ_FOLLOWUP_BITS: u8 = 0b101_1001;
/// PAR-Q+ Q2 (chest pain) and Q3 (dizziness / fainting): symptoms, never
/// clearable (onboard-052).
pub const PARQ_SYMPTOM_BITS: u8 = 0b000_0110;
/// PAR-Q+ Q6 (bone or joint problem): the injury / rehab gate (safety-048).
pub const PARQ_INJURY_BIT: u8 = 0b010_0000;

impl HealthScreen {
    /// A PAR-Q+ follow-up answer is not covered by medical clearance
    /// (safety-044). With per-question answers, every follow-up Yes must sit in
    /// the cleared set; a legacy single toggle (no answers recorded) is cleared
    /// by `medically_cleared` alone.
    pub fn parq_followup_uncleared(&self) -> bool {
        let followups = self.parq_yes & PARQ_FOLLOWUP_BITS;
        if followups == 0 {
            return self.parq_positive && !self.medically_cleared;
        }
        if !self.medically_cleared {
            return true;
        }
        let covered = if self.cleared_parq_mask == 0 {
            followups
        } else {
            self.cleared_parq_mask
        };
        followups & !covered != 0
    }

    /// A chest-pain or dizziness / fainting answer, by flag or by PAR-Q+ answer.
    pub fn symptomatic_any(&self) -> bool {
        self.symptomatic || self.parq_yes & PARQ_SYMPTOM_BITS != 0
    }

    /// An injury / rehab answer, by flag or by the PAR-Q+ bone-or-joint answer.
    pub fn injury_any(&self) -> bool {
        self.injury_or_rehab || self.parq_yes & PARQ_INJURY_BIT != 0
    }

    /// True when any deferral-relevant flag is raised (a positive PAR-Q+ whose
    /// every follow-up answer is medically cleared no longer gates, safety-044;
    /// a symptomatic answer always gates, SCREEN-ACSM-001).
    pub fn any_gate(&self) -> bool {
        self.youth
            || self.parq_followup_uncleared()
            || self.symptomatic_any()
            || self.acute_illness
            || self.pregnant
            || self.pregnancy_warning_sign
            || self.injury_any()
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
    /// Cap today's session at planned RPE minus this many points, i.e. this
    /// many more reps in reserve (File 06 autoreg-006 second clause: e1RM <
    /// baseline − 5 % → "cap session at planned RPE − 1" alongside the ~5 %
    /// top-set load cut).
    CapRpe(u8),
    /// Swap a hard session for an easy one.
    DowngradeSession,
    /// Modify the provoking exercise (reduce tendon load / compressive
    /// positions) and continue with monitoring, tolerable tendon pain ≤3–5/10,
    /// stable (Silbernagel model, File 08 safety-039). Explicitly NOT a rest:
    /// "avoid complete rest". `location` is the user's own words (empty when
    /// none was given); a location that maps to a body region scopes the
    /// modification to the movements that load it.
    ModifyAndMonitor {
        #[serde(default)]
        location: String,
    },
    /// Insert a full rest day.
    RestDay,
    /// Non-negotiable stop (pain, fever, RHR +10 bpm). Safety override.
    Stop,
    /// Stop training and defer to a professional (physician / dietitian /
    /// mental-health). Emitted for medical red flags; `reason` names the trigger
    /// and referral target. Overrides all optimization output (File 08).
    Defer { reason: String },
    /// Apply `adjustment` to ONE plan item only (matched by movement), e.g. a
    /// logged first work set above its target RPE cuts THAT lift's remaining
    /// sets (File 06 autoreg-001 "reduce load for remaining sets"). Never a
    /// stop: the inner adjustment is a load/RPE change.
    ForItem {
        exercise: String,
        adjustment: Box<Adjustment>,
    },
    /// Hold progression without lowering the session (File 13 ready-upd-001):
    /// lift items keep their planned load one rep further from failure and get
    /// no load added; run items run easy, except for a `Soreness` trigger
    /// ("keep systemic intensity"). `trigger` names the signal for the copy.
    CapTopEnd { trigger: ReadinessSignal },
    /// Cut today's volume by this %: lift set counts and run duration/distance
    /// (File 06 autoreg-045 head cold: "cut intensity/duration ~50%").
    ReduceVolumePct(f32),
    /// Stop only the plan items that load the painful body part (File 08
    /// safety-038 "STOP that exercise"); the rest of the day goes ahead with no
    /// load added. `location` is the user's own words, e.g. "Left knee".
    StopItems { location: String },
    /// Replace today's session with the post-illness return check: 10–20 min at
    /// 60–70 % of usual effort, stopping on any warning symptom (File 13
    /// ready-upd-003, ILLNESS-RTT-001).
    ReturnCheck,
}

impl Adjustment {
    /// The adjustment itself, looking through an item scope (`ForItem`).
    pub fn base(&self) -> &Adjustment {
        match self {
            Adjustment::ForItem { adjustment, .. } => adjustment.base(),
            other => other,
        }
    }
}

/// A user-declared factual label for a logged run's intent (steady, easy,
/// interval, tempo, long, recovery). This is **USER DATA**, like an exercise name: it is
/// NOT a knowledge-base claim and carries NO [`ClaimId`]. It is deliberately
/// *not* wrapped in [`Recommended`]: nothing here recommends anything, and per
/// HARD RULE 1 no coaching output may branch on it. It exists for storage and
/// display only (history rendering); anything that would consume it to shape
/// programming needs its own KB backing first. `None`/unset is always valid -
/// the label is never fabricated for a run the user did not tag.
///
/// Serializes to its bare variant name (external form), so the Kotlin shell can
/// mirror the strings by `WorkoutType.name`; renaming a variant is a wire break.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkoutType {
    /// Continuous even-effort run.
    Steady,
    /// Reps-plus-recovery structure (hand-entered/treadmill intervals have no
    /// GPS track, so the measured VI verdict cannot see them, this labels them).
    Interval,
    /// Sustained comfortably-hard effort.
    Tempo,
    /// The week's long endurance run.
    LongRun,
    /// Easy recovery / regeneration run.
    Recovery,
    /// Easy / general-aerobic run (a planned easy day).
    Easy,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::claims;

    #[test]
    fn recommended_wrapper_carries_evidence_and_confidence() {
        let rx = Recommended::new(
            Prescription::Lift(LiftPrescription {
                exercise: "Back squat".into(),
                sets: 5,
                reps: 5,
                intensity: LiftIntensity::PercentOneRm(80.0),
                rest_sec: 180,
                velocity_loss_pct: Some(20),
                reps_max: None,
                load_kg: None,
            }),
            claims::TAPER_001,
        );

        assert_eq!(rx.grade(), EvidenceGrade::Strong);
        assert!((rx.confidence() - 0.90).abs() < f32::EPSILON);
        assert_eq!(
            rx.reference(),
            claims::TAPER_001.entry().primary_citations[0]
        );
    }

    #[test]
    #[should_panic(expected = "MarketingMyth claim LOAD-ACWR-001 must never back a recommendation")]
    fn recommended_new_rejects_marketing_myths_unconditionally() {
        // HARD RULE 2 at the constructor choke point: deliberately NOT gated
        // on `debug_assertions` must also pass under `cargo test --release`.
        let _ = Recommended::new(0u8, claims::LOAD_ACWR_001);
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
