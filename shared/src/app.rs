//! The side-effect-free core (crux `App`). First coaching slice: readiness
//! inputs accumulate in the model; `view()` runs the pure autoregulation layer
//! (`crate::autoreg`) to surface the highest safety tier plus every
//! evidence-cited adjustment. No IO, no clock, no randomness.

use crux_core::{
    App, Command,
    macros::effect,
    render::{RenderOperation, render},
};
use serde::{Deserialize, Serialize};

use crate::feedback::FeedbackCategory;
use crate::hybrid::ConcurrentGoal;
use crate::individualization::{Environment, ProgressionCadence};
use crate::running::{GoalDistance, GpsPoint};
use crate::schema::{
    Adjustment, EvidenceGrade, Goal, HealthScreen, MesoPhase, ReadinessInput, ReadinessSignal,
    Recommended, SafetyTier, VdotBand,
};
use crate::strength::LiftGoal;
use crate::{autoreg, feedback, hybrid, hypertrophy, individualization, load, running, strength};

#[derive(Clone)]
struct LoggedSet {
    exercise: String,
    weight_kg: f64,
    reps: u32,
    rpe: f64,
    /// Log time, unix seconds; 0 when undated (pre-timestamp persisted event).
    observed_at: i64,
}

#[derive(Clone)]
struct LoggedRun {
    /// Manual distance; ignored when `track` is non-empty (derived instead).
    distance_km: f64,
    /// Manual duration; ignored when `track` is non-empty (derived instead).
    duration_min: f64,
    hr_pct_max: f64,
    longest_recent_km: f64,
    /// GPS fixes for a tracked run; empty for a hand-entered run.
    track: Vec<GpsPoint>,
    /// Log time, unix seconds; 0 when undated (pre-timestamp persisted event).
    observed_at: i64,
}

/// The athlete's programming profile: drives the evidence-cited guidance
/// section. Shell-supplied; no logged data required.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Profile {
    /// Cadence at which load can still be added → training age.
    pub progression_cadence: ProgressionCadence,
    /// Target lifting quality for loading bands.
    pub lift_goal: LiftGoal,
    /// Target running goal distance.
    pub goal_distance: GoalDistance,
    /// Concurrent-training priority for same-session ordering.
    pub concurrent_goal: ConcurrentGoal,
    /// Planned weekly sets per muscle (frequency / cap logic).
    pub weekly_sets: u8,
    /// Running days per week (hybrid lower-lift cap trigger).
    pub running_days_per_week: u8,
    /// Running km per week (hybrid lower-lift cap trigger).
    pub running_km_per_week: f64,
    /// Advanced marathoner → extended session range.
    pub advanced: bool,
    /// Endurance session intensity as % VO2max (interference trigger).
    pub endurance_intensity_pct_vo2max: f64,
    /// Female user: routes the feedback-035 menstrual/nutrition clinician
    /// prompt on a bone-stress referral and the hybrid-024 energy-availability
    /// risk cohort. Wire default `false` keeps old persisted profiles parsing.
    #[serde(default)]
    pub female: bool,
    /// Currently inside a high-load/overload mesocycle: arms the autoreg-029
    /// parasympathetic-saturation guard (HRV above the SWC upper band then
    /// blocks auto load-adds). Wire default `false`.
    #[serde(default)]
    pub high_load_block: bool,
    /// Stage-0 onboarding health screen (File 08 onboard-050). Every flag
    /// defaults to `false`, so profiles persisted before the screen existed
    /// parse unchanged and gate nothing.
    #[serde(default)]
    pub health: HealthScreen,
    /// Training-environment context (File 08 indiv-025 / safety-024): heat,
    /// altitude, or cold arms the environment-modifier guidance. `None` (wire
    /// default) states nothing and gates nothing.
    #[serde(default)]
    pub environment: Option<Environment>,
    /// Typical session temperature, °C, running-041 pace-correction trigger
    /// (heat correction above ~15 °C). `None` = unstated, no row.
    #[serde(default)]
    pub env_temp_c: Option<f64>,
    /// Typical session altitude, m, running-041 pace-correction trigger
    /// (altitude correction above ~900 m). `None` = unstated, no row.
    #[serde(default)]
    pub env_altitude_m: Option<f64>,
    /// Weeks fully off training when returning from a layoff: arms the
    /// REENTRY-001 resistance re-entry ramp and the hypertrophy-045 reduced
    /// re-entry MEV note. `None` = not returning.
    #[serde(default)]
    pub weeks_off: Option<f64>,
    /// Bodyweight, kg (optional): arms the strength-plyo depth-jump
    /// readiness gate (needs squat 1RM ≥ 1.5× bodyweight). `None` = no gate.
    #[serde(default)]
    pub bodyweight_kg: Option<f64>,
}

/// Lifting-set execution context for post-session feedback.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LiftExec {
    pub reps_met: bool,
    pub rir_actual: u8,
    pub rir_target: u8,
}

/// Aerobic-decoupling context for a run's feedback.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Decouple {
    pub drift_pct: f64,
    /// Only cool, steady, sub-threshold efforts get a decoupling verdict.
    pub cool_steady_context: bool,
}

/// Interval/threshold-session execution context (feedback-015): did every rep
/// hit its target pace, and did the session cost at or below the target RPE.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct IntervalExec {
    pub target_paces_met: bool,
    pub rpe_at_or_below_target: bool,
}

/// One session's post-hoc review, safety signals plus optional execution
/// context. Feeds the safety-first feedback resolver.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct SessionReview {
    pub bone_pain_red_flag: bool,
    pub compulsive_flag: bool,
    pub overtraining_signal_count: u8,
    /// How long the overtraining/NFOR signals have co-occurred, in weeks
    /// (feedback-036 duration condition: fire only over ≥1–2 wks, never on a
    /// single noisy night). `None` = untracked → count alone gates
    /// (pre-duration behavior, protective).
    #[serde(default)]
    pub overtraining_signal_weeks: Option<f64>,
    /// Single-session distance over the prior-30-day longest, as a fraction.
    pub single_session_spike_frac: Option<f64>,
    pub lift: Option<LiftExec>,
    /// Interval/threshold execution for the feedback-015 mastery arm.
    #[serde(default)]
    pub interval: Option<IntervalExec>,
    pub decoupling: Option<Decouple>,
    /// Fraction of an easy run spent above VT1/Zone-2 ceiling.
    pub easy_frac_time_above_vt1: Option<f64>,
    /// Second-half slowdown percent on an even-effort run.
    pub positive_split_pct: Option<f64>,
    /// Count of sessions this week where the planned RPE was only met at loads
    /// ≥7% below plan, a fatigue-accumulation deload trigger (autoreg-023).
    pub rpe_load_gap_sessions: Option<u8>,
    /// Reference-load bar-velocity drop across the week, m/s (autoreg-026).
    pub weekly_velocity_drop_m_s: Option<f64>,
    /// Failed key (top-set / quality) sessions this week (autoreg-036).
    pub failed_key_sessions: Option<u8>,
    /// The at/above-MRV sign cluster is present, joint aches, performance
    /// stall, sleep disruption, motivation drop (autoreg-025; the KB defines
    /// the cluster qualitatively, no numeric count).
    #[serde(default)]
    pub mrv_sign_cluster: bool,
    /// Weeks of unexplained performance decline (safety-042 / autoreg-042
    /// NFOR triggers; both need ≥2).
    #[serde(default)]
    pub decline_weeks: Option<u8>,
    /// Wellness domains currently suppressed alongside the decline -
    /// fatigue/mood/sleep/etc. (autoreg-042 needs ≥2).
    #[serde(default)]
    pub suppressed_wellness_domains: Option<u8>,
    /// The decline has persisted DESPITE a deload (safety-042 escalation).
    #[serde(default)]
    pub despite_deload: bool,
    /// Pace at target HR improved ≥ the smallest-worthwhile change, and for how
    /// many weeks it has held (autoreg-032; ≥2 → re-test threshold pace).
    #[serde(default)]
    pub pace_at_hr_improved_weeks: Option<u8>,
    /// Count of hypertrophy overreaching triggers present this week -
    /// performance decrement, RIR drift to 0, persistent joint/tendon aches,
    /// disrupted sleep, elevated RHR, mood drop (hypertrophy-035; ≥2 → deload).
    #[serde(default)]
    pub hypertrophy_deload_triggers: Option<u8>,
    /// Persistent joint aches this week (hypertrophy-009 over-MRV sign).
    #[serde(default)]
    pub joint_ache: bool,
    /// Performance regressing this week (hypertrophy-009 over-MRV sign;
    /// feedback-029 trend input).
    #[serde(default)]
    pub performance_down: bool,
    /// Recovery currently compromised, deficit, poor sleep, or high stress
    /// (hypertrophy-010/045 → scale weekly sets to 70–80%; also the
    /// feedback-029 low-recovery trend input and the autoreg-011 wellness gate).
    #[serde(default)]
    pub low_recovery: bool,
    /// Set-to-set rep drop as a fraction (e.g. 0.15 = reps fell 15% between
    /// sets), hypertrophy-039: >10% → lengthen rest.
    #[serde(default)]
    pub rep_drop_frac: Option<f64>,
    /// Today's reference-load mean concentric velocity minus baseline, m/s
    /// (autoreg-008/009 VBT daily readiness; ±0.06 m/s reliability band).
    #[serde(default)]
    pub mcv_delta_m_s: Option<f64>,
    /// First work set hit its target reps (autoreg-011/012 set-volume gate).
    #[serde(default)]
    pub first_set_reps_met: Option<bool>,
    /// First work set RPE minus target RPE (autoreg-011/012).
    #[serde(default)]
    pub first_set_rpe_delta: Option<f64>,
    /// The same lift needed set cuts in both of the last two sessions
    /// (autoreg-014 → hold weekly volume, no add).
    #[serde(default)]
    pub cut_last_two_sessions: bool,
    /// Session RPE crept +1 across the week at unchanged loads (autoreg-024
    /// first condition).
    #[serde(default)]
    pub rpe_creep_plus_one: bool,
    /// Days the wellness composite z-score has been ≤ −1 (autoreg-024 second
    /// condition; ≥3 with the creep → deload).
    #[serde(default)]
    pub wellness_z_low_days: Option<u8>,
    /// Interval reps that landed at RPE ≥ target+1 or above the HR cap
    /// (autoreg-031; ≥2 → cut remaining-rep pace ~2–4%).
    #[serde(default)]
    pub interval_reps_over_target: Option<u8>,
    /// Whether the prescribed easy pace was holdable under the HR cap
    /// (autoreg-033; `Some(false)` → slow the easy pace).
    #[serde(default)]
    pub can_hold_easy_pace_under_hr_cap: Option<bool>,
    /// HRV readings flagged unreliable among the last three (autoreg-050;
    /// ≥2 → suspend HRV gating, fall back to subjective + performance).
    #[serde(default)]
    pub hrv_unreliable_last_three: Option<u8>,
    /// Consecutive days lnRMSSD has sat below the SWC band (autoreg-034;
    /// ≥3 → insert a recovery day / easy block).
    #[serde(default)]
    pub hrv_suppressed_days: Option<u8>,
    /// Days subjective wellness has been suppressed (autoreg-035).
    #[serde(default)]
    pub wellness_suppressed_days: Option<u8>,
    /// Morning resting HR is trending up (autoreg-035; with ≥2 suppressed
    /// wellness days → 1–3 easy days or cross-training).
    #[serde(default)]
    pub rhr_rising: bool,
    /// Single-day lnRMSSD z-score vs baseline (autoreg-028 second trigger;
    /// < −1 with a ≥2-day downtrend → downgrade hard→easy).
    #[serde(default)]
    pub hrv_single_day_z: Option<f64>,
    /// Days the HRV has been trending down (autoreg-028 second trigger).
    #[serde(default)]
    pub hrv_downtrend_days: Option<u8>,
    /// Concurrent-training interference symptoms present (hybrid-018/CAP-6;
    /// with running not mandatory → substitute cycling/rowing for part of
    /// the aerobic volume).
    #[serde(default)]
    pub interference_symptoms: bool,
    /// Consecutive failed sessions on one novice linear-progression lift
    /// (Starting Strength stall governance; ≥3 with adequate recovery →
    /// deload that lift ~10% and re-ramp).
    #[serde(default)]
    pub stall_failed_sessions: Option<u8>,
    /// Recovery (sleep/food/stress) was adequate across the stall, required
    /// before the stall triggers a deload rather than a recovery fix.
    #[serde(default)]
    pub stall_adequate_recovery: bool,
    /// The same lift stalled again after a deload + re-ramp → transition that
    /// lift to intermediate (weekly) progression.
    #[serde(default)]
    pub stalled_again_after_reramp: bool,
    /// Whether the reviewed session was planned-hard (feedback-026 tone:
    /// planned-hard → praise effort; planned-easy → celebrate restraint).
    /// `None` = unstated, no tone modifier.
    #[serde(default)]
    pub planned_hard: Option<bool>,
    /// Rolling multi-week trend direction of the athlete's key metric:
    /// `"up"` / `"flat"` / `"down"` (feedback-027/028/029). `None` = no trend
    /// submitted.
    #[serde(default)]
    pub trend_direction: Option<String>,
    /// Weeks the rolling trend has been flat (feedback-028; ≥4 → plateau
    /// reframing).
    #[serde(default)]
    pub weeks_flat: Option<u8>,
    /// Target missed on a genuine high-RPE off day.
    pub bad_day: bool,
}

#[derive(Default)]
pub struct Model {
    /// Observed readiness signals, in submission order.
    inputs: Vec<ReadinessInput>,
    /// Logged lift sets, in submission order.
    sets: Vec<LoggedSet>,
    /// Logged runs, in submission order.
    runs: Vec<LoggedRun>,
    /// The athlete's programming profile, if set.
    profile: Option<Profile>,
    /// The current session review, if submitted.
    review: Option<SessionReview>,
    /// The last race-time prediction query, if requested. Stored as the raw
    /// inputs so the two-method estimate is re-derived in `view` (like a logged
    /// run), keeping the core deterministic and the prediction reproducible.
    race_query: Option<RaceQuery>,
    /// The last hypertrophy-mesocycle volume-plan query, if requested. Stored as
    /// the raw inputs so the graded per-week plan is re-derived in `view`.
    hypertrophy_plan_query: Option<HypertrophyPlanQuery>,
    /// The last absolute-protein-target query, if requested. Stored as the raw
    /// inputs so the graded g/day range is re-derived in `view` (bodyweight ×
    /// each graded g/kg bound). Self-contained so old logs never carry it.
    protein_query: Option<ProteinQuery>,
    /// The last heart-rate-zone query, if requested. Stored as the raw age so the
    /// graded HRmax + %HRmax band table is re-derived in `view`.
    hr_zone_query: Option<HrZoneQuery>,
    /// The last Cooper 12-min-test query (distance run, metres), if requested.
    cooper_query: Option<f64>,
    /// The last Critical-Speed protocol query (maximal efforts), if requested.
    cs_query: Option<Vec<CsEffortIn>>,
    /// The last APRE next-load query, if requested.
    apre_query: Option<ApreQuery>,
}

/// One maximal effort submitted to the Critical-Speed calculator (running-009):
/// distance in metres, time in seconds. Wire shape for `ComputeCriticalSpeed`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct CsEffortIn {
    pub distance_m: f64,
    pub time_sec: f64,
}

/// Inputs for an APRE next-load adjustment (File 06 autoreg-015…021): the
/// scheme, the AMRAP-set rep count, and the current load in lb (arms the
/// autoreg-019 small-lifter percentage cap).
#[derive(Debug, Clone, PartialEq)]
struct ApreQuery {
    scheme: autoreg::ApreScheme,
    reps: u8,
    current_load_lb: f64,
}

/// Inputs for a hypertrophy accumulation-block volume plan: a target muscle and
/// the number of accumulation weeks. Retained so the view recomputes the graded
/// per-week plan (landmarks, set ramp, RIR schedule, frequency) deterministically.
#[derive(Debug, Clone, PartialEq)]
struct HypertrophyPlanQuery {
    muscle: String,
    weeks: u8,
    /// The muscle is not growing on the current volume (hypertrophy-008).
    not_growing: bool,
    /// Recovery from the current volume is easy (hypertrophy-008; with
    /// `not_growing` → +2 sets next mesocycle).
    recovering_easily: bool,
}

/// Inputs for an absolute daily protein target: the athlete's bodyweight and
/// which graded goal contexts to surface. Retained so the view recomputes the
/// g/day range (bodyweight × each graded g/kg bound) deterministically.
#[derive(Debug, Clone, PartialEq)]
struct ProteinQuery {
    bodyweight_kg: f64,
    masters: bool,
    deficit: bool,
}

/// Inputs for a Daniels+Riegel race-time prediction (running-039), retained so
/// the view can recompute the graded estimate. Distances in metres, time in
/// seconds, `weekly_km` selects the Riegel fatigue exponent.
#[derive(Debug, Clone, PartialEq)]
struct RaceQuery {
    recent_distance_m: f64,
    recent_time_sec: f64,
    goal_distance_m: f64,
    weekly_km: f64,
    /// How many weeks old the input race is (running-041 freshness window:
    /// ≤6 wk fresh, 7–8 marginal, >8 stale → re-test). `None` = untracked.
    weeks_since_race: Option<u8>,
}

/// Inputs for a heart-rate-zone table: the athlete's age in years. Retained so
/// the view recomputes the graded HRmax estimate (Tanaka) and the five Daniels
/// %HRmax band ranges deterministically.
#[derive(Debug, Clone, PartialEq)]
struct HrZoneQuery {
    age_years: f64,
    /// Resting HR, bpm: arms the running-005 %HRmax-vs-Karvonen method
    /// preference and the Karvonen band targets. `None` = %HRmax only.
    resting_hr_bpm: Option<f64>,
    /// Weeks since HR zones were last recalculated off a measured max
    /// (running-006: recompute every 4–6 weeks). `None` = untracked.
    weeks_since_recalc: Option<u8>,
    /// Weeks since training paces were last re-tested (running-041: re-test
    /// every 4–6 weeks). `None` = untracked.
    weeks_since_pace_test: Option<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Event {
    /// Record one readiness observation, then recompute adjustments.
    SubmitReadiness(ReadinessInput),
    /// Drop all accumulated inputs (new day / new session).
    ClearReadiness,
    /// Log one completed lift set (weight in kg, reps, session RPE).
    LogSet {
        exercise: String,
        weight_kg: f64,
        reps: u32,
        rpe: f64,
        /// When the set was logged, unix seconds; shell-supplied (the core holds
        /// no clock). `#[serde(default)]` keeps event logs persisted before this
        /// field existed replayable, they decode as 0 ("undated").
        #[serde(default)]
        observed_at: i64,
    },
    /// Drop all logged sets.
    ClearSets,
    /// Log one run (distance km, duration min, average % HRmax, and the
    /// longest run in the last 30 days for spike detection).
    LogRun {
        distance_km: f64,
        duration_min: f64,
        hr_pct_max: f64,
        longest_recent_km: f64,
        /// Log time, unix seconds; shell-supplied, `#[serde(default)]` for
        /// back-compat with pre-timestamp persisted events (decode as 0).
        #[serde(default)]
        observed_at: i64,
    },
    /// Log one GPS-tracked run. Distance and duration are derived in-core from
    /// the fix track (haversine + time span); `hr_pct_max` comes from a paired
    /// HR sensor (0.0 when none), `longest_recent_km` drives the spike gate.
    LogRunTrack {
        points: Vec<GpsPoint>,
        hr_pct_max: f64,
        longest_recent_km: f64,
        /// Log time, unix seconds; shell-supplied, `#[serde(default)]` for
        /// back-compat with pre-timestamp persisted events (decode as 0). The
        /// GPS fixes carry their own per-point `observed_at`; this is the
        /// session's logged-at stamp for history display.
        #[serde(default)]
        observed_at: i64,
    },
    /// Drop all logged runs.
    ClearRuns,
    /// Set the athlete's programming profile, then recompute guidance.
    SetProfile(Profile),
    /// Drop the profile (clears the guidance section).
    ClearProfile,
    /// Submit a post-session review, then resolve one feedback message.
    SubmitReview(SessionReview),
    /// Drop the session review (clears the feedback section).
    ClearReview,
    /// Predict a goal-race finish time from a recent race (Daniels VDOT +
    /// Riegel, combined per running-039). Distances in metres, time in seconds;
    /// `weekly_km` selects the Riegel fatigue exponent.
    PredictRace {
        recent_distance_m: f64,
        recent_time_sec: f64,
        goal_distance_m: f64,
        weekly_km: f64,
        /// Weeks since the input race (running-041 freshness; serde default
        /// `None` keeps old persisted events replayable).
        #[serde(default)]
        weeks_since_race: Option<u8>,
    },
    /// Drop the race prediction (clears the prediction section).
    ClearRacePrediction,
    /// Plan a hypertrophy accumulation block for one muscle over `weeks`
    /// accumulation weeks, producing a graded per-week volume plan. The two
    /// optional flags feed the hypertrophy-008 next-mesocycle volume decision
    /// (not growing + recovering easily → +2 sets next block).
    PlanHypertrophyMeso {
        muscle: String,
        weeks: u8,
        #[serde(default)]
        not_growing: bool,
        #[serde(default)]
        recovering_easily: bool,
    },
    /// Drop the hypertrophy volume plan (clears the planner section).
    ClearHypertrophyPlan,
    /// Compute an absolute daily protein target (g/day) by scaling the graded
    /// g/kg ranges by `bodyweight_kg`. `masters`/`deficit` select which graded
    /// goal contexts to surface; neither set yields no rows (no general number
    /// is invented, HARD RULE 1).
    ComputeProtein {
        bodyweight_kg: f64,
        masters: bool,
        deficit: bool,
    },
    /// Drop the protein target (clears the protein section).
    ClearProtein,
    /// Compute a heart-rate-zone table from age: an estimated HRmax (Tanaka) and
    /// the five Daniels %HRmax training bands mapped to absolute bpm ranges.
    /// Optional extras (all serde-default for replay compatibility): a resting
    /// HR arms the running-005 Karvonen preference + band targets; the two
    /// weeks-since fields arm the running-006/041 recalibration-due checks.
    ComputeHrZones {
        age_years: f64,
        #[serde(default)]
        resting_hr_bpm: Option<f64>,
        #[serde(default)]
        weeks_since_recalc: Option<u8>,
        #[serde(default)]
        weeks_since_pace_test: Option<u8>,
    },
    /// Drop the heart-rate-zone table (clears the zone section).
    ClearHrZones,
    /// Compute a Cooper 12-min-test VO2max estimate from the distance covered
    /// (metres) in a 12-minute maximal run (File 07 formulas).
    ComputeCooper { distance_m_12min: f64 },
    /// Drop the Cooper estimate (clears the section).
    ClearCooper,
    /// Fit the Critical-Speed 2-parameter model over 2–5 maximal efforts
    /// (running-009 protocol-gated: CS + D′, or the specific protocol
    /// violation explained).
    ComputeCriticalSpeed { efforts: Vec<CsEffortIn> },
    /// Drop the Critical-Speed fit (clears the section).
    ClearCriticalSpeed,
    /// Compute the APRE next-load adjustment from an AMRAP set (File 06
    /// autoreg-015…021, with the autoreg-019 small-lifter cap).
    ComputeApre {
        scheme: autoreg::ApreScheme,
        reps: u8,
        current_load_lb: f64,
    },
    /// Drop the APRE adjustment (clears the section).
    ClearApre,
}

/// One adjustment flattened for shells: human summary + its evidence tag.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct AdjustmentView {
    pub summary: String,
    /// Evidence grade, e.g. `"Strong"`.
    pub grade: String,
    /// Backing reference (author/year or DOI).
    pub citation: String,
    /// 0.05–0.90 confidence score.
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
}

/// One logged set with its derived strength metrics, flattened for shells.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct LiftResultView {
    pub exercise: String,
    /// The set as logged, the raw input a shell shows alongside the derived
    /// metrics, so a lifter sees what they did (100 kg × 5 @ RPE 8), not only the
    /// estimated 1RM.
    pub weight_kg: f64,
    pub reps: u32,
    pub rpe: f64,
    /// Estimated 1RM (Epley), kg, rounded to 0.1.
    pub e1rm_kg: f64,
    /// The set's load as an estimated %1RM (Epley inverse of the rep count),
    /// rounded to a whole percent. Descriptive intensity, not a prescription.
    pub pct_1rm: f64,
    /// Reps in reserve implied by the session RPE.
    pub rir: f64,
    /// Cross-checked e1RM range across Epley/Brzycki/Lombardi (strength-005/
    /// 006). `None` when the estimate is unreliable per strength-006 (0 reps,
    /// >10 reps, or an isolation lift), the shell should then suggest a 3–6
    /// rep test set instead of showing a false range.
    #[serde(default)]
    pub cross_check: Option<E1rmRangeView>,
    /// e1RM change vs the previous logged set of the same exercise, kg
    /// (rounded to 0.1). `None` for the first logged set of an exercise -
    /// there is nothing to compare against. Factual measurement (like pace or
    /// zone), NOT a `Recommended`: it states what changed, not whether the
    /// athlete is "improving"/"declining": that judgment is the trend arm
    /// (feedback-027/028/029, `feedback::trend_summary`, FB-TREND-001), which
    /// a shell must not phrase from this delta alone.
    #[serde(default)]
    pub e1rm_delta_kg: Option<f64>,
    /// `"up"` / `"down"` / `"flat"` direction of `e1rm_delta_kg` (flat when the
    /// 0.1 kg-rounded delta is zero). `None` when `e1rm_delta_kg` is `None`.
    /// Same factual-measurement caveat as the delta.
    #[serde(default)]
    pub e1rm_direction: Option<String>,
    pub summary: String,
    /// When the set was logged, unix seconds; 0 when undated. A shell dates the
    /// history card from this; the core does no formatting (it holds no clock).
    pub observed_at: i64,
}

/// The strength-006 multi-formula e1RM cross-check, flattened for shells.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct E1rmRangeView {
    /// Lowest estimate across the formulas, kg (0.1-rounded).
    pub low_kg: f64,
    /// Highest estimate across the formulas, kg (0.1-rounded).
    pub high_kg: f64,
    /// Number of formulas cross-checked (Epley, Brzycki, Lombardi).
    pub formulas: u8,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
}

/// The CTL/ATL/TSB fitness–fatigue bookkeeping over the logged history,
/// flattened for shells (File 07 impulse-response; LOAD-PMC-001). Daily load
/// is a Lucia 3-zone TRIMP from each dated, HR-carrying run, the method
/// string states exactly what was counted so the shell never has to guess.
/// Bookkeeping only: TSB is NOT a validated performance predictor (the claim
/// statement carries that caveat).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct TrainingLoadView {
    /// Chronic Training Load (fitness): EWMA τ=42 d of daily load.
    pub ctl: f64,
    /// Acute Training Load (fatigue): EWMA τ=7 d of daily load.
    pub atl: f64,
    /// Training Stress Balance (form) entering the next day: CTL − ATL.
    pub tsb: f64,
    /// Days spanned from the first to the last counted session.
    pub days: u32,
    /// Sessions that contributed load (dated runs with HR + duration).
    pub sessions_counted: u32,
    /// Logged sessions that could NOT contribute (undated, no HR, or lifts -
    /// the KB defines no HR-free load formula; nothing is invented).
    pub sessions_skipped: u32,
    /// How daily load was quantified, e.g. `"Lucia TRIMP (3-zone HR)"`.
    pub method: String,
    pub summary: String,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ViewModel {
    /// Highest safety tier triggered, e.g. `"Pain"`; `None` when all clear.
    pub safety_tier: Option<String>,
    /// True when a Stop-level or rest-day condition fires, do not train.
    pub train_blocked: bool,
    pub adjustments: Vec<AdjustmentView>,
    /// Week-level deload triggers carried by the current review. Kept separate
    /// from `adjustments` (which are readiness-driven and cleared by
    /// `ClearReadiness`) because these share the review's lifecycle, they clear
    /// with `ClearReview`, so a shell must render them under the review, not the
    /// readiness section.
    pub review_adjustments: Vec<AdjustmentView>,
    pub input_count: usize,
    pub lifts: Vec<LiftResultView>,
    pub runs: Vec<RunResultView>,
    /// Evidence-cited programming guidance derived from the profile.
    pub guidance: Vec<GuidanceView>,
    /// The single resolved post-session feedback message, if a review is set.
    pub feedback: Option<FeedbackView>,
    /// Always-available evidence-cited reference defaults (profile-independent).
    pub reference: Vec<GuidanceView>,
    /// The currently-set profile, echoed back so a shell can hydrate its editor
    /// after a log replay instead of falling back to a hardcoded default.
    pub profile: Option<Profile>,
    /// The last requested race-time prediction, if any.
    pub race_prediction: Option<RacePredictionView>,
    /// The last requested hypertrophy volume plan, as graded rows. Empty when no
    /// plan has been requested.
    pub hypertrophy_plan: Vec<GuidanceView>,
    /// The last requested absolute protein target(s), as graded rows. Empty when
    /// no query has been made (or neither goal context was selected).
    pub protein_targets: Vec<GuidanceView>,
    /// The last requested heart-rate-zone table, as graded rows (HRmax + five
    /// %HRmax band bpm ranges). Empty when no query has been made.
    pub hr_zones: Vec<GuidanceView>,
    /// CTL/ATL/TSB training-load bookkeeping over the logged run history.
    /// `None` until at least one dated, HR-carrying run is logged.
    #[serde(default)]
    pub training_load: Option<TrainingLoadView>,
    /// Weekly running-volume system rows for the most recent logged week:
    /// week-over-week increase check, two-week ramp flag, Daniels volume caps,
    /// easy-share floor, quality-plan check, hybrid combined-load guard.
    /// Empty until a dated run exists.
    #[serde(default)]
    pub weekly_report: Vec<GuidanceView>,
    /// Lift-session audit rows for the most recent dated lifting day:
    /// Prilepin volume check per exercise, plus the depth-jump readiness gate
    /// when bodyweight + a squat e1RM are known. Empty otherwise.
    #[serde(default)]
    pub lift_audit: Vec<GuidanceView>,
    /// The last requested Cooper 12-min-test estimate, as graded rows.
    #[serde(default)]
    pub cooper: Vec<GuidanceView>,
    /// The last requested Critical-Speed protocol fit, as graded rows.
    #[serde(default)]
    pub critical_speed: Vec<GuidanceView>,
    /// The last requested APRE next-load adjustment, as graded rows.
    #[serde(default)]
    pub apre: Vec<GuidanceView>,
    /// The feedback-027/028/029 longitudinal trend message, when the review
    /// carries a trend direction. Rendered under the review.
    #[serde(default)]
    pub trend: Option<AdjustmentView>,
    /// feedback-040 provisional framing: present while fewer than ~14 distinct
    /// days of logged data exist, recommendations are population defaults
    /// still converging on the user.
    #[serde(default)]
    pub provisional: Option<AdjustmentView>,
    /// Which autoregulation signal source is active given the submitted
    /// readiness data (autoreg-047/048 graceful fallback): HRV rolling,
    /// subjective + performance, or performance-only-hold. `None` until any
    /// readiness input exists.
    #[serde(default)]
    pub autoreg_source: Option<AdjustmentView>,
}

/// A goal-race finish prediction, flattened for shells. Combines a Daniels VDOT
/// projection with a Riegel one (running-039): when the two agree within ~2% a
/// single `predicted` time is shown; otherwise a low–high range, so a single
/// method's false precision is never presented alone.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RacePredictionView {
    /// Human label for the goal distance, e.g. `"10K"` or `"21.1 km"`.
    pub goal_label: String,
    /// Formatted finish time (`"41:30"`) or range (`"41:30–43:00"`).
    pub predicted: String,
    /// True when the two methods agreed (single time); false = range.
    pub agreed: bool,
    /// Range bounds in seconds (equal when `agreed`); 0.0 on degenerate input.
    pub low_sec: f64,
    pub high_sec: f64,
    pub summary: String,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
    /// Graded caveat rows riding on the prediction: input-race staleness
    /// (running-041) and the under-long-run marathon-optimism warning
    /// (running-030). Empty when nothing applies.
    #[serde(default)]
    pub notes: Vec<GuidanceView>,
}

/// The one resolved coaching message for a session, flattened for shells.
/// Safety concerns short-circuit and suppress competing praise (HARD RULE 3).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct FeedbackView {
    /// Category name, e.g. `"ConcernInjury"`.
    pub category: String,
    /// Human-readable coaching copy for the category.
    pub message: String,
    /// True when this message suppresses all competing praise this cycle.
    pub suppresses_praise: bool,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
    /// Verbosity personalization (feedback-023/024): cap on takeaways the
    /// shell should render for this user (beginner: 1).
    #[serde(default)]
    pub max_takeaways: u8,
    /// Cap on metrics surfaced (advanced: up to 3; beginner: 1).
    #[serde(default)]
    pub max_metrics: u8,
    /// Always include the "why" (mandatory for beginners).
    #[serde(default)]
    pub rationale_mandatory: bool,
    /// Keep jargon minimal (beginners).
    #[serde(default)]
    pub minimize_jargon: bool,
    /// Praise copy must name a concrete mastery experience, a specific PR,
    /// completed session, or barrier overcome (feedback-005).
    #[serde(default)]
    pub anchor_mastery: bool,
    /// feedback-026 tone modifier from the session's planned intent:
    /// `"PraiseEffort"` (planned-hard) or `"CelebrateRestraint"`
    /// (planned-easy). `None` when the review states no plan.
    #[serde(default)]
    pub tone: Option<String>,
}

/// One evidence-cited programming recommendation, flattened for shells.
/// `MarketingMyth`-graded claims are never emitted here.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct GuidanceView {
    /// Grouping label, e.g. `"Strength"`, `"Running"`, `"Hybrid"`.
    pub section: String,
    pub summary: String,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
}

/// The core-owned pacing verdict for a run's measured split (feedback-016/017;
/// FB-PACING-001). Carries everything a shell needs to render the chip -
/// verdict, label, coaching copy, and the full evidence tag, so the ~3%
/// threshold (`feedback::POSITIVE_SPLIT_FLAG_PCT`) never leaks into shell code.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct SplitVerdictView {
    /// `"fade"` (back half >3% slower), `"even"` (within ±3%), or
    /// `"negative"` (back half >3% faster).
    pub verdict: String,
    /// Short chip label, e.g. `"FADE +5%"`, `"EVEN SPLIT"`, `"NEG SPLIT 4%"`.
    pub label: String,
    /// Evidence-backed coaching copy: the feedback-016 easier-start cue on a
    /// fade, the feedback-017 pacing-discipline praise otherwise.
    pub message: String,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
}

/// One logged run with derived zone / pace / spike flag, flattened for shells.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RunResultView {
    /// 3-zone lactate classification from % HRmax, e.g. `"Z2"`.
    pub zone: String,
    /// Pace as `m:ss/km`.
    pub pace: String,
    /// Measured distance in km (0.0 for a GPS run with no usable fixes). Lets a
    /// shell render the run's structured fields without re-parsing `summary`.
    pub distance_km: f64,
    /// True when this run's distance spikes >10% over the recent longest.
    pub spike_flag: bool,
    /// Why the spike gate fired, phrased honestly (empty when it did not). A
    /// first run with no baseline says so rather than claiming a ">10%" jump.
    pub spike_note: String,
    /// Second-half pace slowdown percent for a GPS-tracked run (a positive split;
    /// positive = slowed in the back half). `None` for a hand-entered run or a
    /// track too short/degenerate to split. Descriptive, not a prescription.
    pub split_pct: Option<f64>,
    /// Core-owned pacing verdict + chip data for `split_pct` (feedback-016/017).
    /// `None` exactly when `split_pct` is `None`.
    #[serde(default)]
    pub split: Option<SplitVerdictView>,
    pub summary: String,
    /// Evidence backing the spike gate.
    pub citation: String,
    /// Evidence grade backing the spike gate, e.g. `"Strong"`; empty for an
    /// unmeasurable run (no gate ran).
    #[serde(default)]
    pub grade: String,
    /// Confidence score of the spike-gate claim (0.0 when no gate ran).
    #[serde(default)]
    pub confidence: f32,
    /// Whether the spike-gate claim is safety-critical.
    #[serde(default)]
    pub safety_critical: bool,
    /// Whether the spike-gate claim is contested.
    #[serde(default)]
    pub contested: bool,
    /// GPS fixes dropped by the quality gates before distance/pace were
    /// derived (File 07 QC: accuracy >30 m, implied speed >12 m/s, <2.5 m
    /// move, or a non-advancing timestamp). 0 for a hand-entered run.
    #[serde(default)]
    pub qc_dropped: u32,
    /// GPX 1.1 document for a GPS-tracked run, ready for the shell to write and
    /// share; empty string for a hand-entered run with no fix track.
    pub gpx: String,
    /// When the run was logged, unix seconds; 0 when undated. A shell dates the
    /// history card from this; the core does no formatting (it holds no clock).
    pub observed_at: i64,
}

#[effect(typegen)]
#[derive(Debug)]
pub enum Effect {
    Render(RenderOperation),
}

#[derive(Default)]
pub struct Engine;

impl App for Engine {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Self::Event, model: &mut Self::Model) -> Command<Effect, Event> {
        match event {
            Event::SubmitReadiness(input) => model.inputs.push(input),
            Event::ClearReadiness => model.inputs.clear(),
            Event::LogSet {
                exercise,
                weight_kg,
                reps,
                rpe,
                observed_at,
            } => model.sets.push(LoggedSet {
                exercise,
                weight_kg,
                reps,
                rpe,
                observed_at,
            }),
            Event::ClearSets => model.sets.clear(),
            Event::LogRun {
                distance_km,
                duration_min,
                hr_pct_max,
                longest_recent_km,
                observed_at,
            } => {
                // Floor the spike baseline to the longest run we already hold, so a
                // manual entry gets the same gate a GPS-tracked one does: an
                // explicit caller value (paired 30-day history) still wins when
                // larger.
                let prior_longest = model
                    .runs
                    .iter()
                    .map(run_distance_km)
                    .fold(0.0_f64, f64::max);
                model.runs.push(LoggedRun {
                    distance_km,
                    duration_min,
                    hr_pct_max,
                    longest_recent_km: longest_recent_km.max(prior_longest),
                    track: Vec::new(),
                    observed_at,
                });
            }
            Event::LogRunTrack {
                points,
                hr_pct_max,
                longest_recent_km,
                observed_at,
            } => {
                // Spike baseline = longest prior run we already hold, so the gate
                // works without the shell fabricating a recent-longest figure. An
                // explicit caller value (paired history from a tracker) still wins
                // when larger.
                let prior_longest = model
                    .runs
                    .iter()
                    .map(run_distance_km)
                    .fold(0.0_f64, f64::max);
                model.runs.push(LoggedRun {
                    distance_km: 0.0,
                    duration_min: 0.0,
                    hr_pct_max,
                    longest_recent_km: longest_recent_km.max(prior_longest),
                    track: points,
                    observed_at,
                });
            }
            Event::ClearRuns => model.runs.clear(),
            Event::SetProfile(profile) => model.profile = Some(profile),
            Event::ClearProfile => model.profile = None,
            Event::SubmitReview(review) => model.review = Some(review),
            Event::ClearReview => model.review = None,
            Event::PredictRace {
                recent_distance_m,
                recent_time_sec,
                goal_distance_m,
                weekly_km,
                weeks_since_race,
            } => {
                model.race_query = Some(RaceQuery {
                    recent_distance_m,
                    recent_time_sec,
                    goal_distance_m,
                    weekly_km,
                    weeks_since_race,
                });
            }
            Event::ClearRacePrediction => model.race_query = None,
            Event::PlanHypertrophyMeso {
                muscle,
                weeks,
                not_growing,
                recovering_easily,
            } => {
                model.hypertrophy_plan_query = Some(HypertrophyPlanQuery {
                    muscle,
                    weeks,
                    not_growing,
                    recovering_easily,
                });
            }
            Event::ClearHypertrophyPlan => model.hypertrophy_plan_query = None,
            Event::ComputeProtein {
                bodyweight_kg,
                masters,
                deficit,
            } => {
                model.protein_query = Some(ProteinQuery {
                    bodyweight_kg,
                    masters,
                    deficit,
                });
            }
            Event::ClearProtein => model.protein_query = None,
            Event::ComputeHrZones {
                age_years,
                resting_hr_bpm,
                weeks_since_recalc,
                weeks_since_pace_test,
            } => {
                model.hr_zone_query = Some(HrZoneQuery {
                    age_years,
                    resting_hr_bpm,
                    weeks_since_recalc,
                    weeks_since_pace_test,
                });
            }
            Event::ClearHrZones => model.hr_zone_query = None,
            Event::ComputeCooper { distance_m_12min } => {
                model.cooper_query = Some(distance_m_12min);
            }
            Event::ClearCooper => model.cooper_query = None,
            Event::ComputeCriticalSpeed { efforts } => model.cs_query = Some(efforts),
            Event::ClearCriticalSpeed => model.cs_query = None,
            Event::ComputeApre {
                scheme,
                reps,
                current_load_lb,
            } => {
                model.apre_query = Some(ApreQuery {
                    scheme,
                    reps,
                    current_load_lb,
                });
            }
            Event::ClearApre => model.apre_query = None,
        }
        render()
    }

    fn view(&self, model: &Self::Model) -> Self::ViewModel {
        // The profile's lifting goal selects goal-dependent autoregulation
        // thresholds (velocity-loss termination, autoreg-010). No profile →
        // conservative defaults.
        let goal = model.profile.as_ref().map(|p| match p.lift_goal {
            LiftGoal::MaxStrength => Goal::Strength,
            LiftGoal::Power => Goal::Power,
            LiftGoal::Hypertrophy => Goal::Hypertrophy,
        });
        let high_load_block = model.profile.as_ref().is_some_and(|p| p.high_load_block);
        let recommended =
            autoreg::adjustments_with_context(&model.inputs, goal.as_ref(), high_load_block);

        // Stage-0 onboarding gates (File 08 onboard-050): profile health-screen
        // deferrals. Rendered as Safety guidance rows (build_guidance) AND
        // reflected in safety_tier/train_blocked so no shell can miss them.
        let gates = model
            .profile
            .as_ref()
            .map(|p| individualization::onboarding_gates(&p.health))
            .unwrap_or_default();

        let review_recs = model
            .review
            .as_ref()
            .map(review_deloads)
            .unwrap_or_default();

        let blocks = |r: &Recommended<Adjustment>| {
            matches!(
                r.value,
                Adjustment::Stop | Adjustment::RestDay | Adjustment::Defer { .. }
            )
        };
        let train_blocked = recommended.iter().any(blocks)
            || gates.iter().any(blocks)
            || review_recs.iter().any(blocks);

        // Highest safety tier: readiness ladder, raised to MedicalReferral when
        // an onboarding gate or a review-carried NFOR/OTS deferral fires: the
        // KB defers those to a professional (File 08 §5; HARD RULE 3).
        let readiness_tier = autoreg::resolve_safety_for_goal(&model.inputs, goal.as_ref());
        let deferred = !gates.is_empty()
            || review_recs
                .iter()
                .any(|r| matches!(r.value, Adjustment::Defer { .. }));
        let safety_tier = if deferred {
            Some(SafetyTier::MedicalReferral)
        } else {
            readiness_tier
        };

        // RED-S flag from either channel, readiness signal or the onboarding
        // screen, feeds the safety-022 deficit refusal (build_protein_targets).
        let reds_present = model
            .inputs
            .iter()
            .filter(|i| i.signal == ReadinessSignal::RedS)
            .max_by_key(|i| i.observed_at)
            .is_some_and(|i| i.value > 0.0)
            || model
                .profile
                .as_ref()
                .is_some_and(|p| p.health.reds_signal);

        // Feedback personalization inputs (feedback-023/024/035).
        let advanced_user = model.profile.as_ref().is_some_and(|p| {
            individualization::training_age_from_cadence(p.progression_cadence).value
                == individualization::TrainingAge::Advanced
        });
        let female_user = model.profile.as_ref().is_some_and(|p| p.female);

        let review_adjustments = model
            .review
            .as_ref()
            .map(|r| review_views(r, &review_recs, model.profile.as_ref()))
            .unwrap_or_default();

        ViewModel {
            safety_tier: safety_tier.map(|t| format!("{t:?}")),
            train_blocked,
            adjustments: recommended.iter().map(to_view).collect(),
            review_adjustments,
            input_count: model.inputs.len(),
            lifts: lift_views(&model.sets),
            runs: model.runs.iter().map(to_run_view).collect(),
            guidance: model
                .profile
                .as_ref()
                .map(build_guidance)
                .unwrap_or_default(),
            feedback: model.review.as_ref().map(|r| {
                build_feedback(
                    r,
                    latest_track_split(model),
                    latest_run_spike_frac(model),
                    advanced_user,
                    female_user,
                )
            }),
            reference: build_reference(),
            profile: model.profile.clone(),
            race_prediction: model
                .race_query
                .as_ref()
                .map(|q| to_race_view(q, longest_logged_run_km(model))),
            hypertrophy_plan: model
                .hypertrophy_plan_query
                .as_ref()
                .map(|q| build_hypertrophy_plan(q, model.profile.as_ref()))
                .unwrap_or_default(),
            protein_targets: model
                .protein_query
                .as_ref()
                .map(|q| build_protein_targets(q, reds_present))
                .unwrap_or_default(),
            hr_zones: model
                .hr_zone_query
                .as_ref()
                .map(build_hr_zones)
                .unwrap_or_default(),
            training_load: build_training_load(model),
            weekly_report: build_weekly_report(model),
            lift_audit: build_lift_audit(model),
            cooper: model
                .cooper_query
                .map(build_cooper)
                .unwrap_or_default(),
            critical_speed: model
                .cs_query
                .as_deref()
                .map(build_critical_speed)
                .unwrap_or_default(),
            apre: model.apre_query.as_ref().map(build_apre).unwrap_or_default(),
            trend: model.review.as_ref().and_then(build_trend),
            provisional: build_provisional(model),
            autoreg_source: build_autoreg_source(model),
        }
    }
}

/// Profile-independent evidence-cited reference defaults, surfaced always so a
/// shell can show coaching rationale without a full profile set.
fn build_reference() -> Vec<GuidanceView> {
    let mut rows = Vec::new();

    let sp = hybrid::session_spacing();
    push_guidance(
        &mut rows,
        "Hybrid",
        format!(
            "Same-day spacing: {}-{} h ideal, ≥{} h fallback",
            sp.value.ideal_min_hours, sp.value.ideal_max_hours, sp.value.fallback_min_hours
        ),
        &sp,
    );

    let ml = hybrid::maintenance_lift_sessions();
    push_guidance(
        &mut rows,
        "Hybrid",
        format!(
            "Keep {}-{} lift sessions/wk in a running block",
            ml.value.0, ml.value.1
        ),
        &ml,
    );

    let efc = hybrid::endurance_frequency_cap();
    push_guidance(
        &mut rows,
        "Hybrid",
        format!(
            "Endurance frequency cap: ≤{} d/wk when strength co-primary",
            efc.value
        ),
        &efc,
    );

    let md = hybrid::maintenance_dose_fraction();
    push_guidance(
        &mut rows,
        "Hybrid",
        format!(
            "Maintaining a quality (not improving): ~{:.0}% of the improvement volume (~2 low-volume sessions/wk)",
            md.value * 100.0
        ),
        &md,
    );

    // hybrid-011: place the priority quality when freshest.
    let sched = hybrid::priority_quality_when_freshest(true, true);
    push_guidance(
        &mut rows,
        "Hybrid",
        "Schedule the highest-priority quality when freshest - start of the week or right after a rest day".to_string(),
        &sched,
    );

    // hybrid-019 / CAP-8: double-day fueling.
    let cho = hybrid::double_day_cho_refuel(true);
    push_guidance(
        &mut rows,
        "Hybrid",
        "Double (AM/PM) days: fully refuel carbohydrate between the endurance session and the lift - low glycogen amplifies interference".to_string(),
        &cho,
    );

    // hybrid-020: phase interference policy (general vs specific/event).
    let phase = hybrid::phase_interference_policy(hybrid::HybridPhase::SpecificEvent);
    push_guidance(
        &mut rows,
        "Hybrid",
        "Phase policy: separate strength and endurance qualities in general phases; combine them only in a specific/event phase, accepting some interference for transfer (hybrid-race split: 2–3 strength + 3–4 endurance/wk)".to_string(),
        &phase,
    );

    // hybrid-025: tendon-stiffness evidence gap → conservative dual progression.
    let dual = hybrid::conservative_dual_progression(true, true);
    push_guidance(
        &mut rows,
        "Safety",
        "Never progress high running volume and heavy lifting aggressively in the same week - the concurrent effect on tendon stiffness is unstudied; progress one, hold the other".to_string(),
        &dual,
    );

    // hybrid-024: energy-availability guard (higher-risk cohorts named).
    let ea = hybrid::energy_availability_guard(true, true, true);
    push_guidance(
        &mut rows,
        "Safety",
        "Keep energy availability adequate (RED-S/LEA guard) - especially for high-volume endurance, leaner, and female athletes".to_string(),
        &ea,
    );

    let tr = strength::taper_rx();
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Peak taper: cut volume {:.0}-{:.0}% over {}-{} days, hold intensity",
            tr.value.volume_reduction_frac.0 * 100.0,
            tr.value.volume_reduction_frac.1 * 100.0,
            tr.value.duration_days.0,
            tr.value.duration_days.1
        ),
        &tr,
    );

    let dl = strength::deadlift_peak_days_out();
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Last near-max deadlift {}-{} days out from a meet",
            dl.value.0, dl.value.1
        ),
        &dl,
    );

    let pap = strength::pap_rest_window_min();
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "PAP/PAPE contrast rest window: {}-{} min",
            pap.value.0, pap.value.1
        ),
        &pap,
    );

    // Grouped with the Strength rows above (not the Hypertrophy block below) so
    // the section runs stay contiguous, reference cards are labelled per-section,
    // and a stray out-of-order Strength row reads as an orphan.
    let mf = individualization::maintenance_frequency_per_week();
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Maintenance: train each lift {}×/wk to hold strength",
            mf.value
        ),
        &mf,
    );

    let od = strength::olympic_derivative_rx();
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Olympic pulling derivatives: {}-{} sets × {}-{} reps at {}-{}% 1RM, {}-{} min rest",
            od.value.sets.0,
            od.value.sets.1,
            od.value.reps.0,
            od.value.reps.1,
            od.value.pct_1rm.0,
            od.value.pct_1rm.1,
            od.value.rest_sec.0 / 60,
            od.value.rest_sec.1 / 60
        ),
        &od,
    );

    let bo = strength::rpe_anchored_back_off();
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "RPE-anchored: top set at RPE {}, back-offs {:.0}-{:.0}% lighter",
            bo.value.top_set_rpe,
            bo.value.drop_frac.0 * 100.0,
            bo.value.drop_frac.1 * 100.0
        ),
        &bo,
    );

    // Call with the rule's own trigger thresholds purely to inherit the graded
    // citation; the prose states the rule itself (educational, not a live verdict).
    let two = strength::two_for_two_met(2, 2);
    push_guidance(
        &mut rows,
        "Strength",
        "Two-for-two rule: add load once you beat the top of the rep target by ≥2 reps on the last set for 2 straight sessions".to_string(),
        &two,
    );

    let stall = strength::stall_triggers_deload(2, true);
    push_guidance(
        &mut rows,
        "Strength",
        "Stall: if e1RM stays flat ≥2 weeks despite adequate recovery, deload or switch periodization model".to_string(),
        &stall,
    );

    let dbl = individualization::double_progression_add_load(true);
    push_guidance(
        &mut rows,
        "Strength",
        "Double progression: once you reach the top of the rep range on every set, add load and drop back to the range bottom".to_string(),
        &dbl,
    );

    let hd = hypertrophy::deload_rx();
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!(
            "Deload week: ~{:.0}% of sets, {}-{} RIR, load {:.0}-{:.0}% of working",
            hd.value.sets_fraction * 100.0,
            hd.value.rir.0,
            hd.value.rir.1,
            hd.value.load_frac_of_working.0 * 100.0,
            hd.value.load_frac_of_working.1 * 100.0
        ),
        &hd,
    );

    let mex = individualization::min_muscle_exposures_per_week();
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!("Minimum {} muscle exposures/wk", mex.value),
        &mex,
    );

    let jp = hypertrophy::joint_pain_rep_shift();
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!(
            "Joint pain at heavy load: shift that muscle to {}-{} reps at {}-{}% 1RM (growth preserved via load interchangeability)",
            jp.value.reps.0, jp.value.reps.1, jp.value.pct_1rm.0, jp.value.pct_1rm.1
        ),
        &jp,
    );

    let mp = individualization::masters_protein_target();
    push_guidance(
        &mut rows,
        "Nutrition",
        format!(
            "Masters (65+) protein: {:.1}-{:.1} g/kg/day for anabolic resistance",
            mp.value.g_per_kg.0, mp.value.g_per_kg.1
        ),
        &mp,
    );

    // Educational reference row, no RED-S context here; the live safety-022
    // refusal happens in `build_protein_targets`, where the flag is known.
    let dp = individualization::deficit_protein_target(false);
    if let Some(t) = dp.value {
        push_guidance(
            &mut rows,
            "Nutrition",
            format!(
                "Deficit protein (lean-mass preserving): {:.1}-{:.1} g/kg/day",
                t.g_per_kg.0, t.g_per_kg.1
            ),
            &dp,
        );
    }

    let mpm = individualization::masters_protein_per_meal();
    push_guidance(
        &mut rows,
        "Nutrition",
        format!(
            "Masters (65+) per-meal protein: ~{:.1} g/kg to overcome anabolic resistance",
            mpm.value
        ),
        &mpm,
    );

    let c25k = running::c25k_plan();
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "Couch-to-5K: {} run/walk sessions/wk over {}-{} weeks, rest day between; repeat a hard week without penalty",
            c25k.value.runs_per_week, c25k.value.weeks.0, c25k.value.weeks.1
        ),
        &c25k,
    );

    let pyr = running::intensity_distribution(running::DistributionModel::Pyramidal);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "Pyramidal split (base / newer runners): {}% easy / {}% moderate / {}% hard",
            pyr.value.easy_pct, pyr.value.moderate_pct, pyr.value.hard_pct
        ),
        &pyr,
    );

    let pol = running::intensity_distribution(running::DistributionModel::Polarized);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "Polarized split (trained / peak phase): {}% easy / {}% moderate / {}% hard",
            pol.value.easy_pct, pol.value.moderate_pct, pol.value.hard_pct
        ),
        &pol,
    );

    let join_levers = |levers: &[individualization::ScaleLever]| -> String {
        levers
            .iter()
            .map(|l| match l {
                individualization::ScaleLever::AccessoryVolume => "accessory volume",
                individualization::ScaleLever::SetsTowardMev => "sets → MEV",
                individualization::ScaleLever::Frequency => "frequency",
                individualization::ScaleLever::SecondaryQuality => "secondary quality",
                individualization::ScaleLever::IntensityAndMainCompounds => "intensity/main lifts",
            })
            .collect::<Vec<_>>()
            .join(" → ")
    };

    let sd = individualization::scale_down_order();
    push_guidance(
        &mut rows,
        "Scaling",
        format!(
            "Scale DOWN (cut first → protect last): {}",
            join_levers(&sd.value)
        ),
        &sd,
    );

    let su = individualization::scale_up_order();
    push_guidance(
        &mut rows,
        "Scaling",
        format!(
            "Scale UP (add first → add last): {}",
            join_levers(&su.value)
        ),
        &su,
    );

    let sub = individualization::substitution_rule();
    let mut sub_parts = Vec::new();
    if sub.value.match_movement_pattern {
        sub_parts.push("match the movement pattern");
    }
    if sub.value.compensate_with_reps_near_failure {
        sub_parts.push("add reps near failure to offset the lighter load");
    }
    push_guidance(
        &mut rows,
        "Substitutions",
        format!("Home / minimal equipment: {}", sub_parts.join(", then ")),
        &sub,
    );

    let framing = feedback::default_goal_framing();
    let framing_text = match framing.value {
        feedback::GoalFraming::Process => {
            "Goals are framed as controllable process targets (cadence, pacing discipline, RIR) - you steer the process, the outcome follows"
        }
        feedback::GoalFraming::Outcome => {
            "Outcome/result framing (only where an individual goal-efficacy signal supports it)"
        }
    };
    push_guidance(&mut rows, "Coaching", framing_text.to_string(), &framing);

    rows
}

/// Pick the single execution category for a review, in a fixed priority order
/// (lifting → decoupling → easy-intensity → pacing → off-day). `None` when no
/// execution arm fires: the resolver then emits a neutral competence note.
fn session_execution(r: &SessionReview) -> Option<Recommended<FeedbackCategory>> {
    if let Some(l) = &r.lift {
        return Some(feedback::lifting_feedback(
            l.reps_met,
            l.rir_actual,
            l.rir_target,
        ));
    }
    // feedback-015: interval/threshold mastery, reps at target pace at/below
    // target RPE confirm the intended adaptation.
    if let Some(i) = &r.interval
        && let Some(f) =
            feedback::interval_mastery_feedback(i.target_paces_met, i.rpe_at_or_below_target)
    {
        return Some(f);
    }
    if let Some(d) = &r.decoupling
        && let Some(f) = feedback::decoupling_feedback(d.drift_pct, d.cool_steady_context)
    {
        return Some(f);
    }
    if let Some(frac) = r.easy_frac_time_above_vt1
        && let Some(f) = feedback::easy_run_intensity_discipline(frac)
    {
        return Some(f);
    }
    if let Some(ps) = r.positive_split_pct {
        let f = feedback::positive_split_discipline(ps);
        // feedback-016: a positive-split discipline cue always fires. The
        // even/negative-split praise (feedback-017) yields to an explicitly
        // flagged off day: praising pacing over a flagged bad session would
        // bury the off-day reassurance.
        if f.value == FeedbackCategory::IntensityDiscipline || !r.bad_day {
            return Some(f);
        }
    }
    if r.bad_day {
        return Some(feedback::bad_day_feedback());
    }
    None
}

/// Week-level deload triggers carried by a review. Distinct from the readiness
/// adjustments (which react to a single session's markers), these fire off
/// accumulated weekly fatigue counts the shell tallies. Each dormant autoreg
/// fn returns `None` until its threshold is crossed, so an unset field or a
/// sub-threshold count contributes nothing.
fn review_deloads(r: &SessionReview) -> Vec<Recommended<Adjustment>> {
    let mut out = Vec::new();
    if let Some(n) = r.rpe_load_gap_sessions
        && let Some(d) = autoreg::deload_from_rpe_load_gap(n)
    {
        out.push(d);
    }
    if let Some(v) = r.weekly_velocity_drop_m_s
        && let Some(d) = autoreg::deload_from_velocity_drop(v)
    {
        out.push(d);
    }
    if let Some(n) = r.failed_key_sessions
        && let Some(d) = autoreg::deload_from_failed_sessions(n)
    {
        out.push(d);
    }
    // autoreg-025: the at/above-MRV sign cluster (qualitative) → deload.
    if let Some(d) = autoreg::mrv_signs_deload(r.mrv_sign_cluster) {
        out.push(d);
    }
    // autoreg-024: RPE creep +1 at the same loads AND wellness z ≤ −1 for ≥3
    // days → the standard 1-week deload.
    if let Some(days) = r.wellness_z_low_days
        && let Some(d) = autoreg::deload_from_rpe_creep_and_wellness(r.rpe_creep_plus_one, days)
    {
        out.push(d);
    }
    // autoreg-028 second trigger: a single-day lnRMSSD z < −1 with a ≥2-day
    // downtrend → downgrade the session even before the 7-day average clears
    // the SWC band.
    if let (Some(z), Some(days)) = (r.hrv_single_day_z, r.hrv_downtrend_days)
        && let Some(d) = autoreg::hrv_single_day_downgrade(z, days)
    {
        out.push(d);
    }
    // Overtraining escalations (File 08 safety-042 > File 06 autoreg-042): a
    // ≥2-week unexplained decline that survived a deload is the Strong
    // rest-and-defer rule; otherwise the ≥2-domain NFOR cluster mandates a
    // recovery block + professional referral. At most one fires: the
    // stronger-graded rule wins when both conditions hold.
    let decline_weeks = r.decline_weeks.unwrap_or(0);
    let domains = r.suppressed_wellness_domains.unwrap_or(0);
    if let Some(d) =
        autoreg::unexplained_decline_rest_defer(decline_weeks, domains >= 1, r.despite_deload)
    {
        out.push(d);
    } else if let Some(d) = autoreg::nfor_cluster_defer(decline_weeks, domains) {
        out.push(d);
    }
    out
}

/// Flatten the review's week-level triggers for the shell: the deload/deferral
/// adjustments plus the autoreg-032 threshold re-test note (a progression cue,
/// not an `Adjustment`, so it is rendered with its own summary).
fn review_views(
    r: &SessionReview,
    recs: &[Recommended<Adjustment>],
    profile: Option<&Profile>,
) -> Vec<AdjustmentView> {
    let mut out: Vec<AdjustmentView> = recs.iter().map(to_view).collect();
    if let Some(weeks) = r.pace_at_hr_improved_weeks {
        let retest = autoreg::threshold_retest_due(true, weeks);
        if retest.value {
            out.push(to_view_with(
                "Pace at target HR has improved for 2+ weeks - re-test and raise the threshold pace".to_string(),
                &retest,
            ));
        }
    }

    // running-034: ≥2 overtraining signals → insert an unscheduled down week.
    let down = running::unscheduled_deload(r.overtraining_signal_count);
    if down.value {
        out.push(to_view_with(
            "2+ overtraining signals - insert an unscheduled down week now".to_string(),
            &down,
        ));
    }

    // autoreg-008/009 (VBT): reference-load velocity delta → daily-1RM verdict.
    if let Some(delta) = r.mcv_delta_m_s {
        let v = autoreg::vbt_daily_readiness(delta);
        let text = match v.value {
            autoreg::VbtReadiness::IncreaseLoad => {
                "Bar speed up >0.06 m/s at the reference load - daily 1RM is up, raise working loads"
            }
            autoreg::VbtReadiness::Hold => {
                "Bar speed within the ±0.06 m/s reliability band - hold planned loads"
            }
            autoreg::VbtReadiness::ReduceLoad => {
                "Bar speed down >0.06 m/s at the reference load - daily 1RM is down, reduce working loads"
            }
        };
        out.push(to_view_with(text.to_string(), &v));
    }

    // autoreg-011/012: first-work-set outcome → add/drop/hold the set count.
    // Wellness gate reuses the review's low-recovery flag (normal = not low).
    if let (Some(met), Some(rpe_delta)) = (r.first_set_reps_met, r.first_set_rpe_delta) {
        let a = autoreg::set_volume_action(met, rpe_delta, !r.low_recovery);
        let text = match a.value {
            autoreg::SetVolumeAction::AddSet => {
                "Strong first set at low cost with normal wellness - add a set today"
            }
            autoreg::SetVolumeAction::DropLastSet => {
                "First set short or over target RPE - drop the last planned set"
            }
            autoreg::SetVolumeAction::HoldPlanned => "Run the planned sets - no set-count change",
        };
        out.push(to_view_with(text.to_string(), &a));
    }

    // autoreg-013 (RPE-stop) via the lift execution the review already carries:
    // RIR at/below target means the target RPE is reached, cut remaining sets.
    // strength::rir_to_rpe supplies the RIR→RPE mapping.
    if let Some(l) = &r.lift {
        let rpe_actual = strength::rir_to_rpe(f64::from(l.rir_actual));
        let rpe_target = strength::rir_to_rpe(f64::from(l.rir_target));
        if autoreg::rpe_stop_reached(rpe_actual, rpe_target) && !l.reps_met {
            out.push(to_view_with(
                "Target RPE reached before the planned rep count - stop the exercise here (RPE-stop)"
                    .to_string(),
                &graded((), "AUTOREG-RIR-001"),
            ));
        }
    }

    // autoreg-014: two consecutive sessions needing set cuts → hold volume.
    if autoreg::hold_volume_after_two_cut_sessions(r.cut_last_two_sessions) {
        out.push(to_view_with(
            "Set cuts in two straight sessions on this lift - hold weekly volume, no adds"
                .to_string(),
            &graded((), "AUTOREG-RIR-001"),
        ));
    }

    // autoreg-031: ≥2 interval reps over target RPE / HR cap → slow the rest.
    if let Some(n) = r.interval_reps_over_target
        && let Some(cut) = autoreg::interval_pace_autoreg(n)
    {
        out.push(to_view_with(
            format!(
                "2+ interval reps over target - slow the remaining reps ~{:.0}%",
                cut.value * 100.0
            ),
            &cut,
        ));
    }

    // autoreg-033: the HR cap governs easy pace, not the watch pace.
    if let Some(can_hold) = r.can_hold_easy_pace_under_hr_cap {
        let slow = autoreg::slow_easy_pace_if_over_cap(can_hold);
        if slow.value {
            out.push(to_view_with(
                "Easy pace pushes HR over the cap - slow down; the HR cap governs easy days"
                    .to_string(),
                &slow,
            ));
        }
    }

    // autoreg-050: ≥2 unreliable HRV readings in the last 3 → suspend gating.
    if let Some(n) = r.hrv_unreliable_last_three
        && autoreg::suspend_hrv_gating(n)
    {
        out.push(to_view_with(
            "2 of the last 3 HRV readings unreliable - suspend HRV gating; use subjective + performance until a clean baseline returns".to_string(),
            &graded((), "AUTOREG-FALLBACK-001"),
        ));
    }

    // autoreg-034: multi-day lnRMSSD suppression → recovery day / easy block.
    if let Some(days) = r.hrv_suppressed_days {
        let rec = autoreg::hrv_suppressed_recovery_day(days);
        if rec.value {
            out.push(to_view_with(
                "HRV suppressed 3+ consecutive days - insert a recovery day / easy block"
                    .to_string(),
                &rec,
            ));
        }
    }

    // autoreg-035: suppressed wellness + rising RHR → 1–3 easy days.
    if let Some(days) = r.wellness_suppressed_days {
        let easy = autoreg::wellness_rhr_multiday_easy(days, r.rhr_rising);
        if easy.value {
            out.push(to_view_with(
                "Wellness suppressed 2+ days with resting HR trending up - take 1–3 easy days or cross-train".to_string(),
                &easy,
            ));
        }
    }

    // RUN-DECOUPLE-001: Friel band verdict for a measured aerobic-decoupling
    // percentage, only in the valid context (cool, steady, sub-threshold,
    // long enough), same gate the feedback resolver applies.
    if let Some(d) = &r.decoupling
        && d.cool_steady_context
    {
        let (band, evidence, tag) = load::decoupling_band(d.drift_pct);
        if let (Some(ev), Some(tag)) = (evidence, tag) {
            let text = match band {
                load::DecouplingBand::SoundBase => format!(
                    "Decoupling {:.1}% (<5%) - sound aerobic base",
                    d.drift_pct
                ),
                load::DecouplingBand::BuildBase => format!(
                    "Decoupling {:.1}% (5–10%) - build the aerobic base another 3–6 weeks",
                    d.drift_pct
                ),
                load::DecouplingBand::Insufficient => format!(
                    "Decoupling {:.1}% (≥10%) - effort sat above aerobic threshold; endurance base not yet sufficient",
                    d.drift_pct
                ),
            };
            out.push(AdjustmentView {
                summary: text,
                grade: format!("{:?}", ev.grade),
                citation: ev.citation.reference.clone(),
                confidence: tag.score,
                safety_critical: tag.safety_critical,
                contested: tag.contested,
            });
        }
    }

    // hypertrophy-035: ≥2 accumulated overreaching triggers → deload now.
    if let Some(n) = r.hypertrophy_deload_triggers {
        let d = hypertrophy::deload_indicated(n);
        if d.value {
            out.push(to_view_with(
                "2+ overreaching triggers accumulated - take the deload week now rather than waiting for the scheduled one".to_string(),
                &d,
            ));
        }
    }

    // hypertrophy-039: >10% set-to-set rep drop → lengthen rest.
    if let Some(frac) = r.rep_drop_frac {
        let rest = hypertrophy::increase_rest_on_rep_drop(frac);
        if rest.value {
            out.push(to_view_with(
                "Reps fell >10% set-to-set - lengthen the rest interval to protect per-set volume"
                    .to_string(),
                &rest,
            ));
        }
    }

    // Profile-dependent review rows: over-MRV, recovery-scaled volume, hybrid
    // fatigue deload, modality substitution, novice stall.
    if let Some(p) = profile {
        // hypertrophy-009: >20 sets/muscle with regression or aching joints.
        let over = hypertrophy::over_mrv_deload(p.weekly_sets, r.performance_down, r.joint_ache);
        if over.value {
            out.push(to_view_with(
                format!(
                    "{} weekly sets with {} - treat as over MRV and deload",
                    p.weekly_sets,
                    if r.joint_ache {
                        "aching joints"
                    } else {
                        "regressing performance"
                    }
                ),
                &over,
            ));
        }

        // hypertrophy-010/045: low recovery scales weekly sets to 70–80%.
        if r.low_recovery {
            let adj = hypertrophy::recovery_adjusted_volume(p.weekly_sets, true);
            out.push(to_view_with(
                format!(
                    "Recovery is compromised - scale this week to {:.0}–{:.0} sets (70–80% of {}) and cut failure frequency",
                    adj.value.0, adj.value.1, p.weekly_sets
                ),
                &adj,
            ));
        }

        // hybrid-026: ≥2 combined overreaching red flags persisting ≥1 week -
        // only meaningful for a concurrent (lifting + running) athlete.
        if p.running_days_per_week > 0 && p.weekly_sets > 0 {
            let weeks = r.overtraining_signal_weeks.unwrap_or(0.0);
            let hyb = hybrid::combined_fatigue_deload(
                r.overtraining_signal_count,
                weeks.floor().max(0.0) as u8,
            );
            if hyb.value {
                out.push(to_view_with(
                    "Combined-training red flags persisting a week or more - insert a deload / recovery block".to_string(),
                    &hyb,
                ));
            }
        }

        // hybrid-018 / CAP-6: interference symptoms + running not mandatory →
        // substitute a low-impact modality. Running is treated as optional only
        // for a General (no-race) goal: a race goal makes running mandatory.
        let running_optional = p.goal_distance == GoalDistance::General;
        let sub = hybrid::substitute_modality(r.interference_symptoms, running_optional);
        if sub.value {
            out.push(to_view_with(
                "Interference symptoms with no race commitment - swap part of the run volume for cycling/rowing".to_string(),
                &sub,
            ));
        }
    }

    // Starting-Strength stall governance (individualization): 3 straight failed
    // sessions with adequate recovery → 10% single-lift deload; a repeat stall
    // after the re-ramp → move that lift to intermediate progression.
    if let Some(n) = r.stall_failed_sessions {
        let stall = individualization::novice_stall_action(
            n,
            r.stall_adequate_recovery,
            r.stalled_again_after_reramp,
        );
        if let Some(o) = stall.value {
            let text = if o.transition_to_intermediate {
                "Stalled again after the re-ramp - transition this lift to intermediate (weekly) progression".to_string()
            } else {
                format!(
                    "3 straight failed sessions with recovery in order - deload this lift {:.0}% and re-ramp",
                    o.deload_frac * 100.0
                )
            };
            out.push(to_view_with(text, &stall));
        }
    }

    out
}

/// Second-half slowdown of the most recent GPS-tracked run, if any. Lets the
/// positive-split coaching feedback fire from a logged run when the review does
/// not carry an explicit figure: the measurement is descriptive (like pace or
/// zone), so feeding it to the resolver introduces no unevidenced claim.
fn latest_track_split(model: &Model) -> Option<f64> {
    model
        .runs
        .iter()
        .rev()
        .find(|r| !r.track.is_empty())
        .and_then(|r| running::track_positive_split_pct(&r.track, running::MAX_GPS_ACCURACY_M))
}

/// Fraction by which the most recent logged run's distance exceeds the athlete's
/// recent-longest baseline (e.g. `0.15` = 15 % over). `None` when there is no
/// baseline yet, a first run has nothing to be a spike *over*, so the safety
/// gate must not defer on it (the run view already says why it looks unbounded).
/// Lets a logged over-distance run drive the safety gate even when the review
/// omits an explicit figure, mirroring the positive-split fallback.
fn latest_run_spike_frac(model: &Model) -> Option<f64> {
    let r = model.runs.last()?;
    if r.longest_recent_km <= 0.0 {
        return None;
    }
    Some(run_distance_km(r) / r.longest_recent_km - 1.0)
}

/// Seconds per (epoch) week, the deterministic week bucket for the weekly
/// running-volume system. Weeks are `observed_at / 604800` (epoch-aligned, so
/// boundaries fall on Thursday 00:00 UTC): bookkeeping, not a calendar claim.
const WEEK_SEC: i64 = 604_800;

/// Seconds per day, the deterministic day bucket for CTL/ATL chaining.
const DAY_SEC: i64 = 86_400;

/// Longest logged run, km, the running-030 marathon-optimism input. `None`
/// when no run with a measurable distance exists.
fn longest_logged_run_km(model: &Model) -> Option<f64> {
    let m = model
        .runs
        .iter()
        .map(run_distance_km)
        .fold(0.0_f64, f64::max);
    (m > 0.0).then_some(m)
}

/// One dated run flattened for the weekly-volume system and the load chain.
struct DatedRun {
    observed_at: i64,
    km: f64,
    minutes: f64,
    /// 3-zone classification of the average %HRmax; `None` without HR.
    zone: Option<crate::schema::ThreeZone>,
}

/// All dated (`observed_at > 0`) runs with their derived distance, moving
/// duration, and avg-HR zone, in log order. QC-filtered for GPS tracks.
fn dated_runs(model: &Model) -> Vec<DatedRun> {
    model
        .runs
        .iter()
        .filter(|r| r.observed_at > 0)
        .map(|r| {
            let minutes = if r.track.is_empty() {
                r.duration_min
            } else {
                moving_duration_min(&qc_track(&r.track).0)
            };
            DatedRun {
                observed_at: r.observed_at,
                km: run_distance_km(r),
                minutes,
                zone: (r.hr_pct_max > 0.0).then(|| running::classify_three_zone(r.hr_pct_max)),
            }
        })
        .collect()
}

/// CTL/ATL/TSB bookkeeping over the logged run history (File 07
/// impulse-response; LOAD-PMC-001). Daily load is a Lucia 3-zone TRIMP: each
/// dated, HR-carrying run's moving minutes weighted 1/2/3 by the 3-zone
/// classification of its average %HRmax (File 07 "Lucia TRIMP"). Lifts and
/// HR-less runs are counted as skipped, the KB's load formulas (TRIMP/TSS
/// family) are all HR/power/pace-anchored and no session-RPE load formula
/// exists in the KB, so nothing is invented for them (HARD RULE 1). Time
/// enters only via each event's `observed_at` (no clock in the core).
fn build_training_load(model: &Model) -> Option<TrainingLoadView> {
    use std::collections::BTreeMap;

    let mut daily: BTreeMap<i64, f64> = BTreeMap::new();
    let mut counted = 0u32;
    let mut skipped = 0u32;
    for r in &model.runs {
        let minutes = if r.track.is_empty() {
            r.duration_min
        } else {
            moving_duration_min(&qc_track(&r.track).0)
        };
        if r.observed_at <= 0 || r.hr_pct_max <= 0.0 || minutes <= 0.0 {
            skipped += 1;
            continue;
        }
        let zone_minutes = match running::classify_three_zone(r.hr_pct_max) {
            crate::schema::ThreeZone::Z1 => [minutes, 0.0, 0.0],
            crate::schema::ThreeZone::Z2 => [0.0, minutes, 0.0],
            crate::schema::ThreeZone::Z3 => [0.0, 0.0, minutes],
        };
        *daily.entry(r.observed_at.div_euclid(DAY_SEC)).or_insert(0.0) +=
            load::lucia_trimp(zone_minutes);
        counted += 1;
    }
    // Logged lift sets never contribute: no KB HR-free load formula (above).
    skipped += model.sets.len() as u32;

    let (&first, _) = daily.first_key_value()?;
    let (&last, _) = daily.last_key_value()?;
    let (mut ctl, mut atl) = (0.0_f64, 0.0_f64);
    for day in first..=last {
        let l = daily.get(&day).copied().unwrap_or(0.0);
        ctl = load::ctl(ctl, l);
        atl = load::atl(atl, l);
    }
    let tsb = load::tsb(ctl, atl);
    let round1 = |x: f64| (x * 10.0).round() / 10.0;
    let g = graded((), "LOAD-PMC-001");
    Some(TrainingLoadView {
        ctl: round1(ctl),
        atl: round1(atl),
        tsb: round1(tsb),
        days: (last - first + 1) as u32,
        sessions_counted: counted,
        sessions_skipped: skipped,
        method: "Lucia TRIMP (3-zone avg-HR) → EWMA CTL τ=42 d / ATL τ=7 d".to_string(),
        summary: format!(
            "Fitness (CTL) {:.1} · Fatigue (ATL) {:.1} · Form (TSB) {:+.1} over {} days - bookkeeping, not a performance predictor",
            round1(ctl),
            round1(atl),
            round1(tsb),
            last - first + 1
        ),
        grade: format!("{:?}", g.evidence.grade),
        citation: g.evidence.citation.reference.clone(),
        confidence: g.confidence.score,
        safety_critical: g.confidence.safety_critical,
        contested: g.confidence.contested,
    })
}

/// Representative training-age years for the running progression caps
/// (running-031 splits only novice `<1 yr` from everyone else). Same mapping
/// `build_guidance` uses; without a profile the engine keeps its beginner-safe
/// posture and treats the athlete as a novice.
fn training_age_years(profile: Option<&Profile>) -> f64 {
    match profile.map(|p| {
        individualization::training_age_from_cadence(p.progression_cadence).value
    }) {
        Some(individualization::TrainingAge::Novice) | None => 0.5,
        Some(_) => 2.0,
    }
}

/// The weekly running-volume system (File 04): aggregates the most recent
/// logged (epoch-)week of dated runs and runs the progression/cap/distribution
/// validators over it. Zone-dependent shares are counted by the avg-HR
/// time-in-zone convention, running-012's stated default for distribution
/// *reporting*, and say so. Empty until a dated run exists.
fn build_weekly_report(model: &Model) -> Vec<GuidanceView> {
    let mut rows = Vec::new();
    let runs = dated_runs(model);
    let Some(cur_week) = runs.iter().map(|r| r.observed_at.div_euclid(WEEK_SEC)).max() else {
        return rows;
    };
    let week_km = |w: i64| -> f64 {
        runs.iter()
            .filter(|r| r.observed_at.div_euclid(WEEK_SEC) == w)
            .map(|r| r.km)
            .sum()
    };
    let cur: Vec<&DatedRun> = runs
        .iter()
        .filter(|r| r.observed_at.div_euclid(WEEK_SEC) == cur_week)
        .collect();
    let weekly_km: f64 = cur.iter().map(|r| r.km).sum();
    let prev_km = week_km(cur_week - 1);
    let baseline2_km = week_km(cur_week - 2);
    let long_run_km = cur.iter().map(|r| r.km).fold(0.0_f64, f64::max);

    // Week-over-week increase vs the training-age cap (running-031). Only
    // meaningful with a prior week to ratio against.
    if prev_km > 0.0 {
        let ok = running::weekly_increase_ok(prev_km, weekly_km, training_age_years(model.profile.as_ref()));
        let pct = (weekly_km - prev_km) / prev_km * 100.0;
        push_guidance(
            &mut rows,
            "Weekly volume",
            format!(
                "Week-over-week {prev_km:.1} → {weekly_km:.1} km ({pct:+.0}%) - {}",
                if ok.value {
                    "within the increase cap"
                } else {
                    "over the increase cap; hold or trim next week"
                }
            ),
            &ok,
        );
    }

    // Two-week ramp flag (running-028: >30% over two weeks ≈ 1.6× injury odds).
    if baseline2_km > 0.0 {
        let flag = running::two_week_increase_flag(baseline2_km, weekly_km);
        if flag.value {
            push_guidance(
                &mut rows,
                "Weekly volume",
                format!(
                    "Volume up >30% over two weeks ({baseline2_km:.1} → {weekly_km:.1} km) - elevated injury risk, flatten the ramp"
                ),
                &flag,
            );
        }
    }

    if weekly_km > 0.0 {
        // Zone-counting convention note (running-012): time-in-zone is the
        // reporting default; the T/I shares below are avg-HR-classified.
        let method = running::default_counting_method(false);
        push_guidance(
            &mut rows,
            "Weekly volume",
            "Shares below are counted by time-in-zone (avg HR) - the reporting default"
                .to_string(),
            &method,
        );

        // Daniels weekly-share caps (running-016/018/019): long-run share plus
        // the T/I shares as classified by each run's average HR. No rep (R)
        // distance is derivable from logged data, so R is counted as 0.
        let threshold_km: f64 = cur
            .iter()
            .filter(|r| r.zone == Some(crate::schema::ThreeZone::Z2))
            .map(|r| r.km)
            .sum();
        let interval_km: f64 = cur
            .iter()
            .filter(|r| r.zone == Some(crate::schema::ThreeZone::Z3))
            .map(|r| r.km)
            .sum();
        let caps = running::check_volume_caps(long_run_km, threshold_km, interval_km, 0.0, weekly_km);
        let text = match caps.value {
            None => format!(
                "Weekly shares within caps ({weekly_km:.1} km: long {long_run_km:.1}, T {threshold_km:.1}, I {interval_km:.1})"
            ),
            Some(running::CapViolation::LongRun) => format!(
                "Long run {long_run_km:.1} km is {:.0}% of the week - over the ≤25% single-run cap",
                long_run_km / weekly_km * 100.0
            ),
            Some(running::CapViolation::Threshold) => format!(
                "Threshold (Z2-classified) volume {threshold_km:.1} km exceeds the ≤10% weekly cap"
            ),
            Some(running::CapViolation::Interval) => format!(
                "Interval (Z3-classified) volume {interval_km:.1} km exceeds the ≤8% weekly cap"
            ),
            Some(running::CapViolation::Repetition) => {
                "Repetition volume exceeds the ≤5% weekly cap".to_string()
            }
        };
        push_guidance(&mut rows, "Weekly volume", text, &caps);

        // running-016 alternative long-run bound: ≤2× the average daily
        // distance. Only shown when violated: the share row above already
        // states the primary band.
        let daily_avg = running::long_run_within_daily_avg(long_run_km, weekly_km / 7.0);
        if !daily_avg.value {
            push_guidance(
                &mut rows,
                "Weekly volume",
                format!(
                    "Long run {long_run_km:.1} km exceeds 2× your average daily distance ({:.1} km) - outsized relative to the week",
                    weekly_km / 7.0
                ),
                &daily_avg,
            );
        }

        // Long-run share vs the running-016 default band.
        let share = running::long_run_share_default();
        push_guidance(
            &mut rows,
            "Weekly volume",
            format!(
                "Long run {long_run_km:.1} km = {:.0}% of the week (default share {:.0}–{:.0}%)",
                long_run_km / weekly_km * 100.0,
                share.value.0 * 100.0,
                share.value.1 * 100.0
            ),
            &share,
        );
    }

    // Easy-share floor (running-011): ~80% of running time easy, counted over
    // the HR-carrying runs.
    let hr_minutes: f64 = cur
        .iter()
        .filter(|r| r.zone.is_some())
        .map(|r| r.minutes)
        .sum();
    if hr_minutes > 0.0 {
        let easy_minutes: f64 = cur
            .iter()
            .filter(|r| r.zone == Some(crate::schema::ThreeZone::Z1))
            .map(|r| r.minutes)
            .sum();
        let frac = easy_minutes / hr_minutes;
        let floor = running::easy_share_floor_ok(frac);
        push_guidance(
            &mut rows,
            "Intensity",
            format!(
                "Easy (Z1) share {:.0}% of run time - {}",
                frac * 100.0,
                if floor.value {
                    "≥80% floor met"
                } else {
                    "below the ~80% easy floor; make easy days easier"
                }
            ),
            &floor,
        );
    }

    // Quality-plan governance (running-023): count Z3-classified runs this
    // week, their minimum spacing, and consecutive-day Z3 stacking.
    let mut quality: Vec<&&DatedRun> = cur
        .iter()
        .filter(|r| r.zone == Some(crate::schema::ThreeZone::Z3))
        .collect();
    quality.sort_by_key(|r| r.observed_at);
    if !quality.is_empty() {
        let min_gap_hours = quality
            .windows(2)
            .map(|w| (w[1].observed_at - w[0].observed_at) / 3600)
            .min()
            .unwrap_or(i64::from(u8::MAX))
            .clamp(0, i64::from(u8::MAX)) as u8;
        let consecutive_z3 = quality
            .windows(2)
            .any(|w| (w[1].observed_at.div_euclid(DAY_SEC) - w[0].observed_at.div_euclid(DAY_SEC)) <= 1);
        let ok = running::quality_plan_ok(quality.len().min(255) as u8, min_gap_hours, consecutive_z3);
        push_guidance(
            &mut rows,
            "Intensity",
            format!(
                "{} hard (Z3) session{} this week - {}",
                quality.len(),
                if quality.len() == 1 { "" } else { "s" },
                if ok.value {
                    "within quality caps (≤3/wk, ≥48 h apart)"
                } else {
                    "outside quality caps (≤3/wk, ≥48 h apart, no back-to-back hard days)"
                }
            ),
            &ok,
        );
    }

    // Hybrid combined-load ramp guard (hybrid-021): a concurrent athlete keeps
    // weekly running growth ≤~10% to bound stacked mechanical+systemic load.
    if let Some(p) = model.profile.as_ref()
        && p.weekly_sets > 0
        && prev_km > 0.0
    {
        let ok = hybrid::combined_load_progression_ok(prev_km, weekly_km);
        if !ok.value {
            push_guidance(
                &mut rows,
                "Hybrid",
                format!(
                    "Running volume up >10% ({prev_km:.1} → {weekly_km:.1} km) while lifting - cap the combined ramp"
                ),
                &ok,
            );
        }
    }

    // hybrid-012 / CAP-3: keep a heavy leg day and a hard/long run ≥24 h
    // apart, both directions. Judged from logged timestamps: lower-body sets
    // (per the File 03 exercise catalog's primary muscles) vs this week's
    // hard (Z3) runs and its long run.
    {
        let leg_sets: Vec<i64> = model
            .sets
            .iter()
            .filter(|s| s.observed_at > 0)
            .filter(|s| {
                hypertrophy::exercise_entry(&s.exercise).is_some_and(|e| {
                    e.primary_muscles
                        .iter()
                        .any(|m| matches!(*m, "quads" | "hamstrings" | "glutes" | "calves"))
                })
            })
            .map(|s| s.observed_at)
            .collect();
        let long_at = cur
            .iter()
            .filter(|r| r.km > 0.0)
            .max_by(|a, b| a.km.total_cmp(&b.km))
            .map(|r| r.observed_at);
        let hard_runs: Vec<i64> = cur
            .iter()
            .filter(|r| r.zone == Some(crate::schema::ThreeZone::Z3))
            .map(|r| r.observed_at)
            .chain(long_at)
            .collect();
        let min_gap_h = leg_sets
            .iter()
            .flat_map(|s| hard_runs.iter().map(move |r| (s - r).abs()))
            .min()
            .map(|sec| sec as f64 / 3600.0);
        if let Some(h) = min_gap_h {
            let gap_ok = hybrid::heavy_leg_run_gap_ok(h);
            if !gap_ok.value {
                push_guidance(
                    &mut rows,
                    "Hybrid",
                    format!(
                        "Heavy leg work and a hard/long run only {h:.0} h apart - keep ≥24 h between them (residual fatigue lasts 24–48 h)"
                    ),
                    &gap_ok,
                );
            }
        }
    }

    // Novice volume-bump hold (running-031): hold 2–3 weeks between bumps.
    let hold = running::novice_volume_bump_hold_weeks(training_age_years(model.profile.as_ref()));
    if let Some((lo, hi)) = hold.value {
        push_guidance(
            &mut rows,
            "Weekly volume",
            format!("After a volume bump, hold the new volume {lo}–{hi} weeks before the next"),
            &hold,
        );
    }

    rows
}

/// Lift-session audit for the most recent dated lifting day: a Prilepin
/// total-rep check per exercise (strength-013, descriptive %1RM from the rep
/// count), plus the depth-jump readiness gate when the profile carries a
/// bodyweight and a squat e1RM exists in the log.
fn build_lift_audit(model: &Model) -> Vec<GuidanceView> {
    use std::collections::BTreeMap;
    let mut rows = Vec::new();

    if let Some(last_day) = model
        .sets
        .iter()
        .filter(|s| s.observed_at > 0)
        .map(|s| s.observed_at.div_euclid(DAY_SEC))
        .max()
    {
        // Per-exercise totals for that day: total reps + rep-weighted mean
        // estimated %1RM (Epley inverse, descriptive, not a prescription).
        let mut by_exercise: BTreeMap<&str, (u32, f64)> = BTreeMap::new();
        for s in model
            .sets
            .iter()
            .filter(|s| s.observed_at.div_euclid(DAY_SEC) == last_day)
        {
            let e = by_exercise.entry(s.exercise.as_str()).or_insert((0, 0.0));
            e.0 += s.reps;
            e.1 += f64::from(s.reps) * strength::est_pct_1rm_from_reps(s.reps);
        }
        for (exercise, (total_reps, pct_sum)) in by_exercise {
            if total_reps == 0 {
                continue;
            }
            let mean_pct = pct_sum / f64::from(total_reps);
            let ok = strength::prilepin_volume_ok(mean_pct, total_reps.min(u32::from(u16::MAX)) as u16);
            push_guidance(
                &mut rows,
                "Session audit",
                format!(
                    "{exercise}: {total_reps} total reps @ ~{mean_pct:.0}%1RM - {}",
                    if ok.value {
                        "within Prilepin's optimal range"
                    } else {
                        "outside Prilepin's optimal rep range for that load zone"
                    }
                ),
                &ok,
            );

            // hypertrophy-012: below ~30 %1RM the stimulus underperforms even
            // near failure: flag the load floor per exercise-day.
            let floor = hypertrophy::load_below_effective_floor(mean_pct.round().clamp(0.0, 100.0) as u8);
            if floor.value {
                push_guidance(
                    &mut rows,
                    "Session audit",
                    format!(
                        "{exercise}: ~{mean_pct:.0}%1RM sits below the ~30%1RM effective floor - add load"
                    ),
                    &floor,
                );
            }
        }

        // hypertrophy-020: RIR estimates are only ~±1-rep accurate at 0–5 RIR;
        // beyond that the report is a guess: say so once for the day.
        if let Some(worst_rir) = model
            .sets
            .iter()
            .filter(|s| s.observed_at.div_euclid(DAY_SEC) == last_day)
            .map(|s| strength::rpe_to_rir(s.rpe).round().clamp(0.0, 255.0) as u8)
            .max()
        {
            let rel = hypertrophy::rir_reliability(worst_rir);
            if rel.value != hypertrophy::RirReliability::WithinOneRep {
                push_guidance(
                    &mut rows,
                    "Session audit",
                    format!(
                        "A set was logged at ~{worst_rir} RIR - beyond 5 RIR the estimate is unreliable (error >2 reps); train closer to failure to calibrate"
                    ),
                    &rel,
                );
            }
        }

        // strength-006: when a set that day sits outside the reliable e1RM
        // window (>10 reps), point at the preferred 3–6-rep test set instead
        // of trusting formula output.
        if model
            .sets
            .iter()
            .filter(|s| s.observed_at.div_euclid(DAY_SEC) == last_day)
            .any(|s| s.reps > 10)
        {
            let test = strength::e1rm_test_set_reps();
            push_guidance(
                &mut rows,
                "Session audit",
                format!(
                    "High-rep sets make e1RM formulas unreliable - prefer a {}–{}-rep test set to gauge strength",
                    test.value.0, test.value.1
                ),
                &test,
            );
        }
    }

    // Depth-jump gate (strength plyometrics): squat ≥1.5× bodyweight.
    if let Some(bw) = model.profile.as_ref().and_then(|p| p.bodyweight_kg)
        && bw > 0.0
    {
        let squat_e1rm = model
            .sets
            .iter()
            .filter(|s| s.exercise.to_lowercase().contains("squat"))
            .map(|s| strength::e1rm_epley(s.weight_kg, s.reps))
            .fold(0.0_f64, f64::max);
        if squat_e1rm > 0.0 {
            let ready = strength::depth_jump_ready(squat_e1rm, bw);
            push_guidance(
                &mut rows,
                "Session audit",
                format!(
                    "Depth jumps: squat e1RM {squat_e1rm:.0} kg vs {bw:.0} kg BW - {}",
                    if ready.value {
                        "≥1.5× bodyweight, cleared for depth jumps"
                    } else {
                        "below 1.5× bodyweight, hold off on depth jumps"
                    }
                ),
                &ready,
            );
        }
    }

    rows
}

/// Cooper 12-min-test VO2max estimate (File 07 formulas; LOAD-COOPER-001).
/// A distance at/below the formula floor yields an explanatory row, never a
/// negative estimate.
fn build_cooper(distance_m_12min: f64) -> Vec<GuidanceView> {
    let mut rows = Vec::new();
    let g = graded((), "LOAD-COOPER-001");
    if !distance_m_12min.is_finite() || distance_m_12min <= 504.9 {
        push_guidance(
            &mut rows,
            "Cooper test",
            "12-minute distance too short to estimate VO2max - the formula floor is ~505 m"
                .to_string(),
            &g,
        );
        return rows;
    }
    let vo2 = load::cooper_vo2max(distance_m_12min);
    push_guidance(
        &mut rows,
        "Cooper test",
        format!(
            "Cooper 12-min test: {distance_m_12min:.0} m → estimated VO2max {vo2:.1} ml/kg/min"
        ),
        &g,
    );
    rows
}

/// Critical-Speed protocol fit (running-009; RUN-CS-PROTOCOL-001): CS + D′
/// when the 2–5-effort protocol validates, otherwise the specific violation
/// explained. The advisory ideal-pairing note rides along when both fit and
/// the effort set misses the 3–8 min + 12–30 min windows.
fn build_critical_speed(efforts: &[CsEffortIn]) -> Vec<GuidanceView> {
    let mut rows = Vec::new();
    let cs_efforts: Vec<load::CsEffort> = efforts
        .iter()
        .map(|e| load::CsEffort {
            distance_m: e.distance_m,
            time_sec: e.time_sec,
        })
        .collect();
    let fit = load::critical_speed_checked(&cs_efforts);
    match fit.value {
        Ok(f) => {
            let sec_per_km = if f.cs_m_per_s > 0.0 {
                1000.0 / f.cs_m_per_s
            } else {
                0.0
            };
            let pace = if sec_per_km > 0.0 {
                format!("{}:{:02}/km", (sec_per_km as u32) / 60, (sec_per_km as u32) % 60)
            } else {
                "-".to_string()
            };
            push_guidance(
                &mut rows,
                "Critical speed",
                format!(
                    "Critical Speed {:.2} m/s ({pace}) · D′ {:.0} m from {} maximal efforts",
                    f.cs_m_per_s,
                    f.d_prime_m,
                    cs_efforts.len()
                ),
                &fit,
            );
            if !load::cs_pairing_ideal(&cs_efforts) {
                push_guidance(
                    &mut rows,
                    "Critical speed",
                    "Protocol note: the ideal pairing is one 3–8 min and one 12–30 min effort"
                        .to_string(),
                    &fit,
                );
            }
        }
        Err(v) => {
            let why = match v {
                load::CsProtocolViolation::TooFewEfforts => {
                    "needs at least 2 maximal efforts"
                }
                load::CsProtocolViolation::TooManyEfforts => {
                    "uses at most 5 maximal efforts"
                }
                load::CsProtocolViolation::DurationOutOfRange => {
                    "each effort must last 2–30 minutes"
                }
                load::CsProtocolViolation::DegenerateDurations => {
                    "efforts must differ in duration to fit a slope"
                }
                load::CsProtocolViolation::NegativeDPrime => {
                    "fitted D′ is negative - a trial was likely not maximal; re-test"
                }
            };
            push_guidance(
                &mut rows,
                "Critical speed",
                format!("No Critical-Speed estimate: the protocol {why}"),
                &fit,
            );
        }
    }
    rows
}

/// APRE next-load adjustment (File 06 autoreg-015…021 with the autoreg-019
/// small-lifter cap), as one graded row.
fn build_apre(q: &ApreQuery) -> Vec<GuidanceView> {
    let mut rows = Vec::new();
    let adj = autoreg::apre_load_adjustment_capped_lb(q.scheme, q.reps, q.current_load_lb);
    let (lo, hi) = adj.value;
    let label = match q.scheme {
        autoreg::ApreScheme::Apre3 => "APRE-3",
        autoreg::ApreScheme::Apre6 => "APRE-6",
        autoreg::ApreScheme::Apre10 => "APRE-10",
    };
    let text = if lo == 0.0 && hi == 0.0 {
        format!(
            "{label}: {} reps on the AMRAP set at {:.0} lb - hold the load",
            q.reps, q.current_load_lb
        )
    } else {
        format!(
            "{label}: {} reps on the AMRAP set at {:.0} lb - adjust next load {lo:+.0} to {hi:+.0} lb",
            q.reps, q.current_load_lb
        )
    };
    push_guidance(&mut rows, "APRE", text, &adj);
    rows
}

/// The feedback-027/028/029 longitudinal trend message, when the review
/// carries a rolling trend direction.
fn build_trend(r: &SessionReview) -> Option<AdjustmentView> {
    let dir = match r.trend_direction.as_deref()? {
        "up" => feedback::TrendDirection::Up,
        "down" => feedback::TrendDirection::Down,
        _ => feedback::TrendDirection::Flat,
    };
    let load_spike = r.single_session_spike_frac.is_some_and(|f| f > 0.10);
    let t = feedback::trend_summary(
        dir,
        r.weeks_flat.unwrap_or(0),
        r.performance_down,
        load_spike,
        r.low_recovery,
    );
    let text = match t.value {
        feedback::TrendSummary::Improving => {
            "Rolling trend is up - consistency is paying off; set the next process goal"
        }
        feedback::TrendSummary::Plateau => {
            "Flat 4+ weeks - normal consolidation; change ONE variable and protect the routine"
        }
        feedback::TrendSummary::LoadExplainedDecline => {
            "The dip lines up with load/recovery, not lost fitness - recovery first; consider a deload week"
        }
        feedback::TrendSummary::Stable => "Trend steady - keep stacking consistent weeks",
    };
    Some(to_view_with(text.to_string(), &t))
}

/// feedback-040 provisional framing: present until ~14 distinct dated days of
/// logged data (readiness, sets, runs) exist.
fn build_provisional(model: &Model) -> Option<AdjustmentView> {
    use std::collections::BTreeSet;
    let days: BTreeSet<i64> = model
        .inputs
        .iter()
        .map(|i| i.observed_at)
        .chain(model.sets.iter().map(|s| s.observed_at))
        .chain(model.runs.iter().map(|r| r.observed_at))
        .filter(|t| *t > 0)
        .map(|t| t.div_euclid(DAY_SEC))
        .collect();
    let n = days.len().min(usize::from(u16::MAX)) as u16;
    let prov = feedback::provisional_until_baseline(n);
    prov.value.then(|| {
        to_view_with(
            format!(
                "Recommendations are population defaults until ~14 days of data exist ({n} so far)"
            ),
            &prov,
        )
    })
}

/// Which autoregulation signal source is active (autoreg-047/048): HRV rolling
/// when HRV data is usable, else subjective + performance, else
/// performance-only with loads held. `None` until any readiness input exists.
fn build_autoreg_source(model: &Model) -> Option<AdjustmentView> {
    if model.inputs.is_empty() {
        return None;
    }
    let latest_day = model
        .inputs
        .iter()
        .map(|i| i.observed_at)
        .max()
        .unwrap_or(0)
        .div_euclid(DAY_SEC);
    let hrv_today = model.inputs.iter().any(|i| {
        i.signal == ReadinessSignal::HrvLnRmssd && i.observed_at.div_euclid(DAY_SEC) == latest_day
    });
    let recent_hrv = model
        .inputs
        .iter()
        .filter(|i| {
            i.signal == ReadinessSignal::HrvLnRmssd
                && i.observed_at.div_euclid(DAY_SEC) >= latest_day - 7
        })
        .count()
        .min(255) as u8;
    let subjective = model.inputs.iter().any(|i| {
        matches!(
            i.signal,
            ReadinessSignal::WellnessZ | ReadinessSignal::Soreness
        )
    });
    let src = autoreg::autoreg_source(hrv_today, recent_hrv, subjective);
    let text = match src.value {
        autoreg::AutoregSource::HrvRolling => "Autoregulating on the 7-day rolling HRV gate",
        autoreg::AutoregSource::SubjectivePlusPerformance => {
            "No usable HRV - autoregulating on subjective wellness + performance"
        }
        autoreg::AutoregSource::PerformanceOnlyHold => {
            "No HRV or wellness data - performance-only mode: hold loads, no progression beyond plan"
        }
    };
    Some(to_view_with(text.to_string(), &src))
}

/// Resolve one session's feedback message (safety gate first), flattened.
///
/// `track_split` is a run-derived positive-split fallback and `spike_frac` a
/// run-derived distance-spike fallback, each used only when the review omits its
/// own figure, so a run-only day still gets pacing and safety feedback.
fn build_feedback(
    r: &SessionReview,
    track_split: Option<f64>,
    spike_frac: Option<f64>,
    advanced_user: bool,
    female_user: bool,
) -> FeedbackView {
    let safety = feedback::SafetySignals {
        bone_pain_red_flag: r.bone_pain_red_flag,
        compulsive_flag: r.compulsive_flag,
        overtraining_signal_count: r.overtraining_signal_count,
        overtraining_weeks: r.overtraining_signal_weeks,
        single_session_spike_frac: r.single_session_spike_frac.or(spike_frac),
    };
    let effective = SessionReview {
        positive_split_pct: r.positive_split_pct.or(track_split),
        ..r.clone()
    };
    let resolved = feedback::resolve_feedback(safety, session_execution(&effective));

    // feedback-035: a female user routed to the bone-stress referral gets the
    // gentle menstrual/nutrition clinician prompt appended (safety-critical,
    // defers to the professional, never a diagnosis).
    let bsi_referral = resolved.value == FeedbackCategory::ConcernInjury;
    let mut message = feedback_message(resolved.value).to_string();
    if feedback::bsi_menstrual_nutrition_prompt(female_user, bsi_referral).value {
        message.push(' ');
        message.push_str(feedback::BSI_FEMALE_PROMPT);
    }

    // feedback-023/024 verbosity + feedback-005 mastery anchoring.
    let verbosity = feedback::verbosity_for_experience(advanced_user).value;
    let anchor = feedback::mastery_anchor_required(resolved.value).value;

    // feedback-026 tone-by-planned-intent, when the review states the plan.
    let tone = r
        .planned_hard
        .map(|hard| format!("{:?}", feedback::planned_intensity_tone(hard).value));

    // Myth guards (HARD RULE 2): the retracted 2.9:1 positivity ratio and any
    // hard ACWR injury-prediction claim are blocked in the registry: the
    // feedback pipeline consults the guards so neither can ever shape copy.
    debug_assert!(
        !feedback::positivity_ratio_enforced(),
        "MYTH-POSITIVITY must stay hard-blocked"
    );
    debug_assert!(
        !feedback::acwr_injury_claim_allowed(),
        "LOAD-ACWR-001 must stay hard-blocked"
    );

    FeedbackView {
        category: format!("{:?}", resolved.value),
        message,
        suppresses_praise: resolved.value.suppresses_competing_praise(),
        grade: format!("{:?}", resolved.evidence.grade),
        citation: resolved.evidence.citation.reference.clone(),
        confidence: resolved.confidence.score,
        safety_critical: resolved.confidence.safety_critical,
        contested: resolved.confidence.contested,
        max_takeaways: verbosity.max_takeaways,
        max_metrics: verbosity.max_metrics,
        rationale_mandatory: verbosity.rationale_mandatory,
        minimize_jargon: verbosity.minimize_jargon,
        anchor_mastery: anchor,
        tone,
    }
}

/// Coaching copy for a feedback category (File 05 voice: autonomy-supportive,
/// process-framed, never guilt-inducing).
fn feedback_message(cat: FeedbackCategory) -> &'static str {
    match cat {
        FeedbackCategory::ConcernInjury => {
            "Stop training this area and see a professional - this looks like a bone-stress red flag."
        }
        FeedbackCategory::ConcernRecovery => {
            "Several overtraining signals are stacking up. Back off and prioritize recovery this week."
        }
        FeedbackCategory::ConcernBehavior => {
            "This pattern looks compulsive. A rest day is not lost progress - consider stepping back."
        }
        FeedbackCategory::DangerousProgression => {
            "That was a large single-session jump. Rein in the progression to protect connective tissue."
        }
        FeedbackCategory::IntensityDiscipline => {
            "Easy days should stay easy - dial the effort back to build the aerobic base."
        }
        FeedbackCategory::PositiveExecution => "Well-paced, durable effort. Nicely controlled.",
        FeedbackCategory::InformationalNeutral => {
            "Mild aerobic fatigue noted - nothing to correct."
        }
        FeedbackCategory::CorrectiveProcess => {
            "Missed target is data, not failure. Here's an adjustment for next time - your call."
        }
        FeedbackCategory::PositiveMastery => {
            "Target hit at planned cost. You've earned the next planned progression."
        }
        FeedbackCategory::ContextualBadDay => {
            "Off day - normal variation. The stimulus still counts; no guilt."
        }
        FeedbackCategory::ProgressionNudge => {
            "That was well under target effort. Room to add load next session."
        }
    }
}

/// Push one evidence-cited row, dropping `MarketingMyth`-graded claims (HARD
/// RULE 2, myths are never programmed or surfaced).
fn push_guidance<T>(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    summary: String,
    r: &Recommended<T>,
) {
    if r.evidence.grade == EvidenceGrade::MarketingMyth {
        return;
    }
    rows.push(GuidanceView {
        section: section.into(),
        summary,
        grade: format!("{:?}", r.evidence.grade),
        citation: r.evidence.citation.reference.clone(),
        confidence: r.confidence.score,
        safety_critical: r.confidence.safety_critical,
        contested: r.confidence.contested,
    });
}

/// Run the programming engine over the profile, producing evidence-cited rows.
fn build_guidance(p: &Profile) -> Vec<GuidanceView> {
    let mut rows = Vec::new();

    // Stage-0 onboarding gates lead every guidance list (File 08 onboard-050:
    // screen BEFORE any prescription; safety-000: never overridden by goals).
    // Each fired gate renders as a Safety row with its deferral reason.
    for gate in individualization::onboarding_gates(&p.health) {
        push_guidance(&mut rows, "Safety", describe(&gate.value), &gate);
    }
    // Pregnancy avoid-list (safety-047) travels with the safety-045 deferral.
    if p.health.pregnant {
        let pre = individualization::pregnancy_precautions();
        push_guidance(
            &mut rows,
            "Safety",
            format!(
                "Pregnancy: avoid prolonged supine positioning, overheating, contact/fall-risk activities, scuba diving, altitude above {:.0} m, and breath-holding (Valsalva) during strength work",
                pre.value.avoid_altitude_above_m
            ),
            &pre,
        );
    }

    // Safety-critical: high weekly mileage raises bone-stress-injury surveillance
    // (File 10 hybrid-023). Only surfaced once the profile's mileage crosses the
    // threshold, so a low-volume runner is not warned needlessly. Lives in the
    // leading Safety block so the section stays contiguous with the gates.
    let bsi = hybrid::bsi_surveillance_flag(p.running_km_per_week);
    if bsi.value {
        push_guidance(
            &mut rows,
            "Safety",
            format!(
                "Bone-stress-injury surveillance: {:.0} km/wk exceeds ~64 km - monitor for focal bone pain; keep energy availability adequate",
                p.running_km_per_week
            ),
            &bsi,
        );
        // hybrid-024 energy-availability guard: the higher-risk cohorts
        // (high-volume endurance, leaner, female) get named vigilance.
        let ea = hybrid::energy_availability_guard(true, false, p.female);
        if ea.value {
            push_guidance(
                &mut rows,
                "Safety",
                "Energy-availability guard (RED-S/LEA): high endurance volume raises under-fueling risk - keep intake matched to load".to_string(),
                &ea,
            );
        }
    }

    let age_r = individualization::training_age_from_cadence(p.progression_cadence);
    let age = age_r.value;
    push_guidance(
        &mut rows,
        "Profile",
        format!("Training age: {age:?}"),
        &age_r,
    );

    let sd = individualization::strength_defaults(age);
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Defaults: {}%1RM, {}×/muscle/wk, {} sets/muscle",
            sd.value.intensity_pct_1rm, sd.value.freq_per_muscle, sd.value.sets_per_muscle
        ),
        &sd,
    );

    let lr = strength::loading_rx(p.lift_goal);
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "{:?} loading: {}-{}%1RM, {}-{} reps, {}-{} sets, RIR {}-{}",
            p.lift_goal,
            lr.value.pct_1rm.0,
            lr.value.pct_1rm.1,
            lr.value.reps.0,
            lr.value.reps.1,
            lr.value.sets.0,
            lr.value.sets.1,
            lr.value.rir.0,
            lr.value.rir.1
        ),
        &lr,
    );

    let vlt = strength::vl_termination_threshold(p.lift_goal);
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Velocity-loss set cutoff for {:?}: end the set at ~{:.0}% bar-speed loss",
            p.lift_goal,
            vlt.value * 100.0
        ),
        &vlt,
    );

    // strength-040 1RM-test gate, stated educationally (the all-clear call
    // only inherits the safety-critical citation; the conditions are the row).
    let novice = matches!(age, individualization::TrainingAge::Novice);
    let test_gate = strength::one_rm_test_allowed(strength::OneRmTestContext {
        technically_proficient: true,
        adequately_recovered: true,
        warmed_up: true,
        is_novice: novice,
        supervised: true,
        spinal_loading: true,
        bracing_competent: true,
    });
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Test a true 1RM only when technically proficient, recovered, and warmed up{}; spinal lifts need bracing competence",
            if novice {
                " - as a novice, only supervised (prefer the estimated 1RM)"
            } else {
                ""
            }
        ),
        &test_gate,
    );

    // strength-040 novice progression-jump caps between attempts.
    if novice {
        let upper = strength::novice_load_jump_cap_frac(true);
        let lower = strength::novice_load_jump_cap_frac(false);
        push_guidance(
            &mut rows,
            "Strength",
            format!(
                "Novice load jumps: upper body +{:.1}–{:.0}%, lower body +{:.0}–{:.0}% per step",
                upper.value.0 * 100.0,
                upper.value.1 * 100.0,
                lower.value.0 * 100.0,
                lower.value.1 * 100.0
            ),
            &upper,
        );
    }

    let pm = strength::periodization_model(age);
    push_guidance(
        &mut rows,
        "Strength",
        format!("Periodization model: {:?}", pm.value),
        &pm,
    );

    // Surface the actual phase-by-phase prescription for the athlete's model, so
    // the periodization label above becomes an actionable mesocycle plan. Only
    // Linear and Block have phase tables in the source; DUP does not, so it is
    // left as the label only.
    let fmt_phase = |rx: &strength::PhaseRx| -> String {
        let mut parts = Vec::new();
        if let Some((lo, hi)) = rx.pct_1rm {
            parts.push(format!("{lo}-{hi}%1RM"));
        }
        if let (Some((slo, shi)), Some((rlo, rhi))) = (rx.sets, rx.reps) {
            parts.push(format!("{slo}-{shi}×{rlo}-{rhi}"));
        } else if let Some((rlo, rhi)) = rx.reps {
            parts.push(format!("{rlo}-{rhi} reps"));
        }
        if parts.is_empty() {
            parts.push("maintain intensity (taper template)".to_string());
        }
        format!("{} · wk {}-{}", parts.join(", "), rx.weeks.0, rx.weeks.1)
    };
    match pm.value {
        strength::PeriodizationModel::Linear => {
            use strength::LinearPhase::*;
            for (name, phase) in [
                ("Base", Base),
                ("Build", Build),
                ("Peak", Peak),
                ("Taper", Taper),
            ] {
                let ph = strength::linear_phase_rx(phase);
                push_guidance(
                    &mut rows,
                    "Strength",
                    format!("Linear {name}: {}", fmt_phase(&ph.value)),
                    &ph,
                );
            }
        }
        strength::PeriodizationModel::Block => {
            use strength::BlockPhase::*;
            for (name, phase) in [
                ("Accumulation", Accumulation),
                ("Transmutation", Transmutation),
                ("Realization", Realization),
            ] {
                let ph = strength::block_phase_rx(phase);
                push_guidance(
                    &mut rows,
                    "Strength",
                    format!("Block {name}: {}", fmt_phase(&ph.value)),
                    &ph,
                );
            }
        }
        strength::PeriodizationModel::Dup | strength::PeriodizationModel::Conjugate => {}
    }

    // Double-progression load jump, only actionable for a novice on linear
    // progression; intermediate/advanced lifters no longer add fixed load every
    // session, so showing it to them would misdescribe their programming.
    if matches!(age, individualization::TrainingAge::Novice) {
        let inc = individualization::novice_load_increment();
        push_guidance(
            &mut rows,
            "Strength",
            format!(
                "Novice load jump: +{:.1} kg upper / +{:.1} kg lower per session (double progression)",
                inc.value.upper_kg, inc.value.lower_kg
            ),
            &inc,
        );
    } else {
        // Past the novice phase the fixed per-session jump no longer applies;
        // progression moves to a per-week percentage of load. Surface both body
        // regions since the bands differ (upper is more conservative).
        let upper = strength::weekly_pct_increment(true);
        let lower = strength::weekly_pct_increment(false);
        // Render each bound faithfully: some bands carry a half-percent (e.g.
        // 2.5%), so a whole-number format would understate the cited value. Drop
        // the decimal only when the bound is whole.
        let pct = |frac: f64| -> String {
            let v = frac * 100.0;
            if v.fract().abs() < 1e-9 {
                format!("{v:.0}")
            } else {
                format!("{v:.1}")
            }
        };
        push_guidance(
            &mut rows,
            "Strength",
            format!(
                "Weekly load increment: +{}-{}% upper / +{}-{}% lower per successful week",
                pct(upper.value.0),
                pct(upper.value.1),
                pct(lower.value.0),
                pct(lower.value.1)
            ),
            &upper,
        );
    }

    // Prilepin volume ceiling at the midpoint of the goal's loading intensity -
    // the reps/set + optimal-total governor that keeps quality high per zone.
    // Kept adjacent to the other Strength rows (before the Power block below) so
    // the section stays contiguous: the shell suppresses a repeated section
    // header only for consecutive same-section rows.
    let pct_mid = f64::from(lr.value.pct_1rm.0 + lr.value.pct_1rm.1) / 2.0;
    if let Some(pr) = strength::prilepin_for(pct_mid) {
        let pr = strength::recommend(*pr, "STR-PRILEPIN-001");
        push_guidance(
            &mut rows,
            "Strength",
            format!(
                "Prilepin @~{:.0}%1RM: {}-{} reps/set, optimal {} total ({}-{} range)",
                pct_mid,
                pr.value.reps_per_set.0,
                pr.value.reps_per_set.1,
                pr.value.optimal_total,
                pr.value.total_range.0,
                pr.value.total_range.1
            ),
            &pr,
        );
    }

    // Plyometric foot-contact ceiling, only relevant when the goal is Power.
    // Placed after all Strength rows so the "Power" section forms its own
    // contiguous block rather than splitting Strength in two.
    if p.lift_goal == LiftGoal::Power {
        let plyo = strength::plyo_foot_contact_cap(age);
        push_guidance(
            &mut rows,
            "Power",
            format!(
                "Plyo foot-contact cap: {}-{}/session (progress volume OR intensity, not both)",
                plyo.value.0, plyo.value.1
            ),
            &plyo,
        );
    }

    let fr = hypertrophy::frequency_for_weekly_sets(p.weekly_sets);
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!(
            "{} weekly sets → {}×/wk, {} sets/session",
            p.weekly_sets,
            fmt_u8_range(fr.value.freq.0, fr.value.freq.1),
            fmt_u8_range(fr.value.per_session.0, fr.value.per_session.1)
        ),
        &fr,
    );

    let split = hypertrophy::needs_session_split(p.weekly_sets);
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!(
            "{} weekly sets/muscle: {}",
            p.weekly_sets,
            if split.value {
                "split across ≥2 sessions/wk to keep per-session quality"
            } else {
                "fit in one weekly session"
            }
        ),
        &split,
    );

    // Rep/load prescription by exercise class. The frequency/volume rows above
    // answer "how many sets"; this answers "how many reps and what load", the
    // per-set target that was otherwise absent from the hypertrophy guidance.
    // All three classes share one evidence source, so surface them together.
    let heavy = hypertrophy::rep_load(hypertrophy::ExerciseClass::HeavyCompound);
    let moderate = hypertrophy::rep_load(hypertrophy::ExerciseClass::ModerateCompound);
    let iso = hypertrophy::rep_load(hypertrophy::ExerciseClass::Isolation);
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!(
            "Rep/load: heavy compound {}-{} @{}-{}%1RM, moderate {}-{} @{}-{}%, isolation {}-{} @{}-{}%",
            heavy.value.reps.0,
            heavy.value.reps.1,
            heavy.value.pct_1rm.0,
            heavy.value.pct_1rm.1,
            moderate.value.reps.0,
            moderate.value.reps.1,
            moderate.value.pct_1rm.0,
            moderate.value.pct_1rm.1,
            iso.value.reps.0,
            iso.value.reps.1,
            iso.value.pct_1rm.0,
            iso.value.pct_1rm.1
        ),
        &heavy,
    );

    let cap = hypertrophy::cap_weekly_growth_target(p.weekly_sets);
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!("Growth-target weekly sets capped at {}", cap.value),
        &cap,
    );

    let mev = hypertrophy::mev_sets_by_training_age(age);
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!(
            "MEV for {age:?}: {}-{} sets/muscle/wk",
            mev.value.0, mev.value.1
        ),
        &mev,
    );

    let hvs = individualization::high_volume_sensitivity(age);
    push_guidance(
        &mut rows,
        "Individualization",
        format!(
            "High-volume sensitivity: {}",
            if hvs.value {
                "yes - cap added volume"
            } else {
                "no"
            }
        ),
        &hvs,
    );

    let gp = running::goal_week_plan(p.goal_distance, p.advanced);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "{:?}: {}-{} sessions/wk, {}-{} quality, long run {:.0}-{:.0}% of volume",
            p.goal_distance,
            gp.value.sessions_per_week.0,
            gp.value.sessions_per_week.1,
            gp.value.quality_per_week.0,
            gp.value.quality_per_week.1,
            gp.value.long_run_share.0 * 100.0,
            gp.value.long_run_share.1 * 100.0
        ),
        &gp,
    );

    // Quality-session governance caps, the guardrail behind the "quality/wk"
    // count above. Only relevant to someone who actually runs, so a pure lifter
    // is not shown running caps.
    if p.running_days_per_week > 0 {
        let ql = running::quality_limits();
        push_guidance(
            &mut rows,
            "Running",
            format!(
                "Quality-session caps: ≤{}/wk, ≥{} h apart, no back-to-back Z3",
                ql.value.max_per_week, ql.value.min_spacing_hours
            ),
            &ql,
        );
    }

    // Default to the base-phase distribution (pyramidal), the sound starting
    // point before a mesocycle phase is chosen.
    let id = running::distribution_for_phase(MesoPhase::Base);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "Base-phase intensity: {}/{}/{} easy/moderate/hard %time",
            id.value.easy_pct, id.value.moderate_pct, id.value.hard_pct
        ),
        &id,
    );

    let ef = hybrid::endurance_frequency_ok(p.running_days_per_week);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "{} running days/wk within endurance-frequency guidance: {}",
            p.running_days_per_week, ef.value
        ),
        &ef,
    );

    let conservative = matches!(age, individualization::TrainingAge::Novice);
    let dc = running::deload_cadence(conservative);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "Deload cadence: {}:{} load:recovery, cut {:.0}-{:.0}%",
            dc.value.load_weeks,
            dc.value.recovery_weeks,
            dc.value.reduction_frac.0 * 100.0,
            dc.value.reduction_frac.1 * 100.0
        ),
        &dc,
    );

    // Weekly volume-increase ceiling. The cap only distinguishes sub-1-year
    // runners from everyone else, so map the training age onto a representative
    // year count rather than inventing a precise figure the profile lacks.
    let age_years = match age {
        individualization::TrainingAge::Novice => 0.5,
        _ => 2.0,
    };
    let wc = running::weekly_increase_cap_frac(age_years);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "Weekly volume increase cap: +{:.0}% max week-to-week",
            wc.value * 100.0
        ),
        &wc,
    );

    // Session-type prescriptions (running-014…020) for an athlete who runs:
    // the easy-run band, cruise-interval and VO2max-interval structure, and
    // strides. Each row is built from the Rx struct so no number is retyped.
    if p.running_days_per_week > 0 {
        let easy = running::workout_rx(running::RunWorkout::EasyGeneralAerobic);
        let mut parts: Vec<String> = Vec::new();
        if let (Some(lo), Some(hi)) = easy.value.pct_hr_max {
            parts.push(format!("{:.0}–{:.0} %HRmax", lo * 100.0, hi * 100.0));
        }
        if let Some((lo, hi)) = easy.value.rpe {
            parts.push(format!("RPE {lo}–{hi}"));
        }
        if let (Some(lo), Some(hi)) = easy.value.duration_min {
            parts.push(format!("{lo}–{hi} min"));
        }
        push_guidance(
            &mut rows,
            "Running",
            format!("Easy / general-aerobic runs: {}", parts.join(", ")),
            &easy,
        );

        // running-016/018 session Rx from the schema session types.
        let long = running::run_workout_rx(crate::schema::RunSessionType::LongRun);
        let mut long_parts: Vec<String> = Vec::new();
        if let (Some(lo), Some(hi)) = long.value.pct_hr_max {
            long_parts.push(format!("{:.0}–{:.0} %HRmax", lo * 100.0, hi * 100.0));
        }
        if let (Some(lo), Some(hi)) = long.value.pct_slower_than_mp {
            long_parts.push(format!("{lo:.0}–{hi:.0}% slower than MP"));
        }
        if let Some((lo, hi)) = long.value.rpe {
            long_parts.push(format!("RPE {lo}–{hi}"));
        }
        if let (None, Some(hi)) = long.value.duration_min {
            long_parts.push(format!("≤{hi} min"));
        }
        push_guidance(
            &mut rows,
            "Running",
            format!("Long runs: {}", long_parts.join(", ")),
            &long,
        );

        let cruise = running::cruise_interval_rx();
        push_guidance(
            &mut rows,
            "Running",
            format!(
                "Cruise intervals (T pace): {}–{} min reps, ~{:.0} min rest, ≤{:.0}% of weekly volume",
                cruise.value.rep_duration_min.0,
                cruise.value.rep_duration_min.1,
                cruise.value.rest_approx_min,
                cruise.value.weekly_cap_frac * 100.0
            ),
            &cruise,
        );

        let vo2 = running::vo2max_interval_rx();
        push_guidance(
            &mut rows,
            "Running",
            format!(
                "VO2max intervals (I pace): {}–{} min reps ({}–{} m), recovery ≈ rep time, ≤{:.0}% of weekly volume",
                vo2.value.rep_duration_min.0,
                vo2.value.rep_duration_min.1,
                vo2.value.rep_distance_m.0,
                vo2.value.rep_distance_m.1,
                vo2.value.weekly_cap_frac * 100.0
            ),
            &vo2,
        );

        // running-021 hill sprints, strength-flavoured running work, most
        // relevant to a power-goal athlete who runs.
        if matches!(p.lift_goal, LiftGoal::Power) {
            let hills = running::hill_sprint_rx();
            push_guidance(
                &mut rows,
                "Running",
                format!(
                    "Hill sprints: {}–{} × {}–{} s at {:.0}–{:.0}% effort on a {:.0}–{:.0}% grade, full (~{} s) recovery, on easy days",
                    hills.value.reps.0,
                    hills.value.reps.1,
                    hills.value.rep_sec.0,
                    hills.value.rep_sec.1,
                    hills.value.effort_pct.0,
                    hills.value.effort_pct.1,
                    hills.value.grade_pct.0,
                    hills.value.grade_pct.1,
                    hills.value.recovery_approx_sec
                ),
                &hills,
            );
        }

        let strides = running::strides_rx();
        push_guidance(
            &mut rows,
            "Running",
            format!(
                "Strides: {}–{} × {}–{} s controlled-fast (RPE {}–{}), {}–{} s recovery, {}–{}×/wk",
                strides.value.reps.0,
                strides.value.reps.1,
                strides.value.rep_sec.0,
                strides.value.rep_sec.1,
                strides.value.rpe.0,
                strides.value.rpe.1,
                strides.value.recovery_sec.0,
                strides.value.recovery_sec.1,
                strides.value.per_week.0,
                strides.value.per_week.1
            ),
            &strides,
        );

        // running-033 depth: the KB names no mileage threshold separating the
        // bands, so the general 20–40 % band is stated (never a guess).
        let rw = running::recovery_week_rx(running::MileageBand::Unspecified);
        push_guidance(
            &mut rows,
            "Running",
            format!(
                "Recovery week: cut volume {:.0}–{:.0}%, reduce intensity, drop a quality session",
                rw.value.volume_reduction_frac.0.unwrap_or(0.0) * 100.0,
                rw.value.volume_reduction_frac.1 * 100.0
            ),
            &rw,
        );

        // running-038 distance-specific taper for race goals.
        if let Some(taper) = running::distance_taper(p.goal_distance) {
            let t = taper.value;
            push_guidance(
                &mut rows,
                "Running",
                format!(
                    "Race taper ({:?}): {}–{} days, cut volume {:.0}–{:.0}%{} - hold intensity and frequency",
                    p.goal_distance,
                    t.days.0,
                    t.days.1,
                    t.volume_cut_frac.0 * 100.0,
                    t.volume_cut_frac.1 * 100.0,
                    if t.keep_mp_touches_and_short_tempo {
                        ", keep MP touches + short tempo"
                    } else if t.keep_sharp_sessions.is_some() {
                        ", keep 1–2 short sharp sessions"
                    } else {
                        ""
                    }
                ),
                &taper,
            );
        }

        // running-032 single-variable rule, stated educationally (the call's
        // both-true arguments only inherit the citation, same pattern as the
        // two-for-two reference row).
        let onevar = running::single_variable_progression_ok(true, true);
        push_guidance(
            &mut rows,
            "Running",
            "Progress ONE variable at a time - never raise weekly volume and intensity in the same week"
                .to_string(),
            &onevar,
        );

        // running-041 environment pace-correction triggers, when the profile
        // states conditions. Only trigger flags are evidence-stated, no
        // correction magnitudes exist in the KB, so none are shown.
        if p.env_temp_c.is_some() || p.env_altitude_m.is_some() {
            let pc = running::pace_correction_triggers(
                p.env_temp_c.unwrap_or(0.0),
                p.env_altitude_m.unwrap_or(0.0),
            );
            if pc.value.heat || pc.value.altitude {
                let what = match (pc.value.heat, pc.value.altitude) {
                    (true, true) => "heat (>15 °C) and altitude (>900 m)",
                    (true, false) => "heat (>15 °C)",
                    _ => "altitude (>900 m)",
                };
                push_guidance(
                    &mut rows,
                    "Running",
                    format!(
                        "Conditions trigger pace correction for {what} - expect slower paces at the same effort; anchor to HR/RPE"
                    ),
                    &pc,
                );
            }
        }
    }

    // File 08 indiv-025 / safety-024 environment modifiers, when declared.
    if let Some(env) = p.environment {
        let m = individualization::environment_modifier(env);
        let text = match env {
            Environment::Heat => format!(
                "Heat: reduce intensity, acclimatize progressively (~{}–{} days), hydrate - STOP on heat-illness signs (confusion, cessation of sweating, dizziness)",
                m.value.acclimatization_days.map(|d| d.0).unwrap_or(10),
                m.value.acclimatization_days.map(|d| d.1).unwrap_or(14)
            ),
            Environment::Altitude =>
                "Altitude (>~2,500 m): reduce absolute intensity until acclimatized".to_string(),
            Environment::Cold => "Cold: extend the warm-up".to_string(),
            Environment::Neutral => "Neutral environment: no modifier".to_string(),
        };
        if env != Environment::Neutral {
            push_guidance(&mut rows, "Environment", text, &m);
        }
    }

    // REENTRY-001 layoff re-entry ramp + the post-layoff MEV reduction.
    if let Some(weeks_off) = p.weeks_off
        && weeks_off > 0.0
    {
        let re = individualization::resistance_reentry(weeks_off);
        push_guidance(
            &mut rows,
            "Return to training",
            format!(
                "After {weeks_off:.0} wk off: restart at ~{:.0}% of prior loads, ramp back over {}{}",
                re.value.load_frac * 100.0,
                fmt_u8_range(re.value.ramp_weeks.0, re.value.ramp_weeks.1),
                if re.value.treat_as_novice {
                    " wk - progress like a novice until loads return"
                } else {
                    " wk"
                }
            ),
            &re,
        );
        let mev = hypertrophy::layoff_reduces_mev(true);
        if mev.value {
            push_guidance(
                &mut rows,
                "Return to training",
                "Post-layoff MEV is reduced - less volume regrows muscle at re-entry; restart below the old set counts".to_string(),
                &mev,
            );
        }
    }

    let so = hybrid::same_session_order(p.concurrent_goal);
    push_guidance(
        &mut rows,
        "Hybrid",
        format!("{:?} same-session order: {:?}", p.concurrent_goal, so.value),
        &so,
    );

    // Peak strength/power block running override (File 10 CAP-2): only relevant
    // when the lifting goal is a maximal quality, so a hypertrophy or endurance
    // athlete is not shown a cap that does not apply to their block.
    if matches!(p.lift_goal, LiftGoal::MaxStrength | LiftGoal::Power) {
        let pk = hybrid::peak_phase_run_cap();
        push_guidance(
            &mut rows,
            "Hybrid",
            format!(
                "Peak block: cap running to {}-{} easy runs/wk, no hard intervals, long runs ≥{} h from heavy-lower days",
                pk.value.max_easy_runs_per_week.0,
                pk.value.max_easy_runs_per_week.1,
                pk.value.long_run_min_gap_hours
            ),
            &pk,
        );
    }

    if let Some(llc) = hybrid::lower_lift_cap(p.running_days_per_week, p.running_km_per_week) {
        push_guidance(
            &mut rows,
            "Hybrid",
            format!(
                "Lower-lift cap: ≤{}/wk, cut lower-hyp volume {:.0}-{:.0}%",
                llc.value.max_lower_sessions,
                llc.value.volume_reduction_frac.0 * 100.0,
                llc.value.volume_reduction_frac.1 * 100.0
            ),
            &llc,
        );
    }

    let ie =
        hybrid::interference_expected(p.running_days_per_week, p.endurance_intensity_pct_vo2max);
    push_guidance(
        &mut rows,
        "Hybrid",
        format!("Interference expected: {}", ie.value),
        &ie,
    );

    // hybrid-004: when interference is on the table, name the strongest lever -
    // continuous per-session duration outweighs frequency (Wilson 2012), so
    // shorten endurance sessions before cutting days.
    if ie.value {
        let im = hybrid::interference_moderators();
        push_guidance(
            &mut rows,
            "Hybrid",
            "Interference scales most with continuous session duration (strongest moderator), then frequency - shorten endurance sessions before cutting days".to_string(),
            &im,
        );
    }

    // Whether *this* athlete's training age makes them susceptible to the small
    // trained-lower-body 1RM decrement (File 10 hybrid-009): only trained lifters
    // (>1 yr) show it. Reuses the representative `age_years` above and is only
    // relevant when the athlete actually runs, so a pure lifter is not shown it.
    if p.running_days_per_week > 0 {
        let li = hybrid::expect_lower_strength_interference(age_years);
        push_guidance(
            &mut rows,
            "Hybrid",
            format!(
                "Lower-body strength interference susceptibility: {}",
                if li.value {
                    "yes - trained lifter, expect a small lower-body 1RM decrement"
                } else {
                    "no - novice/untrained lower body is spared"
                }
            ),
            &li,
        );
    }

    rows
}

/// One run's realised distance in km: derived from the GPS track when present,
/// otherwise the hand-entered scalar.
fn run_distance_km(r: &LoggedRun) -> f64 {
    if r.track.is_empty() {
        r.distance_km
    } else {
        running::track_distance_km(&qc_track(&r.track).0, running::MAX_GPS_ACCURACY_M)
    }
}

/// File 07 GPS quality gates over a fix track, applied BEFORE any distance /
/// duration / split is derived. Returns the surviving fixes plus the dropped
/// count (accuracy-gated fixes included). Gates, in order per surviving pair:
/// a non-advancing timestamp (speed undefined), an implied speed >12 m/s
/// (`load::gps_speed_plausible`, impossible for a runner), and a <2.5 m move
/// (`load::gps_point_accept`, the Apple jitter/auto-pause pattern; the
/// vertical-rate arm passes 0.0 because fixes carry no altitude).
fn qc_track(points: &[GpsPoint]) -> (Vec<GpsPoint>, u32) {
    let usable = running::usable_track(points, running::MAX_GPS_ACCURACY_M);
    let mut dropped = (points.len() - usable.len()) as u32;
    let mut out: Vec<GpsPoint> = Vec::with_capacity(usable.len());
    for p in usable {
        let Some(last) = out.last() else {
            out.push(p);
            continue;
        };
        let dt = p.observed_at - last.observed_at;
        if dt <= 0 {
            dropped += 1;
            continue;
        }
        let dist_m = running::haversine_m(*last, p);
        let speed = dist_m / dt as f64;
        if !load::gps_speed_plausible(speed) || !load::gps_point_accept(dist_m, 0.0) {
            dropped += 1;
            continue;
        }
        out.push(p);
    }
    (out, dropped)
}

/// Moving time over a QC'd track, minutes: interval seconds where the implied
/// speed clears the File 07 stop gate (`load::is_stopped`, <0.5 m/s counts as
/// stopped, the auto-pause rule), so standing time never dilutes pace.
fn moving_duration_min(track: &[GpsPoint]) -> f64 {
    let mut sec = 0.0;
    for w in track.windows(2) {
        let dt = (w[1].observed_at - w[0].observed_at) as f64;
        if dt <= 0.0 {
            continue;
        }
        let speed = running::haversine_m(w[0], w[1]) / dt;
        if !load::is_stopped(speed) {
            sec += dt;
        }
    }
    sec / 60.0
}

/// Derive zone + pace + distance-spike flag for one logged run.
fn to_run_view(r: &LoggedRun) -> RunResultView {
    // A GPS track derives its own distance/duration; a manual run uses scalars.
    let gps = !r.track.is_empty();
    // File 07 QC gates run BEFORE any derivation (accuracy, implausible speed,
    // jitter, non-advancing time); the dropped count is surfaced to the shell.
    let (track, qc_dropped) = if gps {
        qc_track(&r.track)
    } else {
        (Vec::new(), 0)
    };

    // A GPS run whose fixes all fail the QC gates has no measurable route:
    // distance/duration collapse to 0, which would otherwise render as a
    // "0.0km @ -" entry *and* trip the spike gate against a phantom baseline.
    // Report the signal problem honestly instead of fabricating a null run.
    if gps && track.len() < 2 {
        return RunResultView {
            zone: "-".to_string(),
            pace: "-".to_string(),
            distance_km: 0.0,
            spike_flag: false,
            spike_note: String::new(),
            split_pct: None,
            split: None,
            summary: "GPS signal too poor to measure this run".to_string(),
            // No measurable distance → the spike gate never ran; carry an
            // empty evidence tag rather than citing a claim that was not used.
            citation: String::new(),
            grade: String::new(),
            confidence: 0.0,
            safety_critical: false,
            contested: false,
            qc_dropped,
            gpx: String::new(),
            observed_at: r.observed_at,
        };
    }

    let distance_km = if gps {
        running::track_distance_km(&track, running::MAX_GPS_ACCURACY_M)
    } else {
        r.distance_km
    };
    // GPS duration is MOVING time (File 07 auto-pause: <0.5 m/s intervals are
    // excluded), so a paused run's pace reflects running, not standing.
    let duration_min = if gps {
        moving_duration_min(&track)
    } else {
        r.duration_min
    };

    // Zone is a heart-rate classification (File 04). With no HR sample we do not
    // fabricate one, report it as unknown rather than defaulting to Z1.
    let has_hr = r.hr_pct_max > 0.0;
    let zone = running::classify_three_zone(r.hr_pct_max);
    let zone_str = if has_hr {
        format!("{zone:?}")
    } else {
        "-".to_string()
    };
    let spike = running::single_session_spike_flag(distance_km, r.longest_recent_km);

    let pace = if distance_km > 0.0 && duration_min > 0.0 {
        let sec_per_km = (duration_min * 60.0) / distance_km;
        format!(
            "{}:{:02}/km",
            (sec_per_km as u32) / 60,
            (sec_per_km as u32) % 60
        )
    } else {
        "-".to_string()
    };

    let split_pct = if gps {
        running::track_positive_split_pct(&track, running::MAX_GPS_ACCURACY_M)
    } else {
        None
    };

    // A positive split is a back-half slowdown; a negative value is a negative
    // split (faster finish). Thresholds are strict (>3 %, <-3 %) to match
    // `feedback::positive_split_discipline`, so this note and the coaching cue it
    // pairs with never disagree at exactly 3 %.
    let split_note = match split_pct {
        Some(p) if p > feedback::POSITIVE_SPLIT_FLAG_PCT => {
            format!(" · +{p:.0}% back-half slowdown")
        }
        Some(p) if p < -feedback::POSITIVE_SPLIT_FLAG_PCT => {
            format!(" · {:.0}% negative split", p.abs())
        }
        _ => String::new(),
    };

    // The spike gate errs safe with no history (see single_session_spike), so a
    // user's first run trips it. Say *why* rather than claiming a ">10%" jump over
    // a baseline that does not exist yet, the SPIKE flag itself is unchanged, only
    // the wording is honest about the cause. Held as a field so a shell can render
    // it beside the SPIKE chip without re-parsing `summary`.
    let spike_note = if spike.value {
        if r.longest_recent_km > 0.0 {
            "distance spike >10% over recent longest"
        } else {
            "flagged: no prior run to gauge distance yet"
        }
    } else {
        ""
    };
    let spike_seg = if spike_note.is_empty() {
        String::new()
    } else {
        format!(" - {spike_note}")
    };

    RunResultView {
        zone: zone_str.clone(),
        pace: pace.clone(),
        distance_km,
        spike_flag: spike.value,
        spike_note: spike_note.to_string(),
        split_pct,
        split: split_pct.map(split_verdict_view),
        summary: format!(
            "{}{:.1}km @ {} ({}){}{}",
            if gps { "GPS " } else { "" },
            distance_km,
            pace,
            zone_str,
            spike_seg,
            split_note,
        ),
        citation: spike.evidence.citation.reference.clone(),
        grade: format!("{:?}", spike.evidence.grade),
        confidence: spike.confidence.score,
        safety_critical: spike.confidence.safety_critical,
        contested: spike.confidence.contested,
        qc_dropped,
        gpx: {
            // Export the same QC-gated fixes used for distance/duration so the
            // file's distance matches what the app shows. A track whose fixes
            // all fail the gates leaves fewer than two usable points: no real
            // route, so emit no GPX rather than a degenerate file the shell
            // would still offer an "Export" button for.
            if track.len() >= 2 {
                running::export_gpx(&track, &format!("Run {distance_km:.1}km"))
            } else {
                String::new()
            }
        },
        observed_at: r.observed_at,
    }
}

/// Build the core-owned pacing-verdict chip for a measured split
/// (feedback-016/017; FB-PACING-001). The verdict/message routing reuses
/// `feedback::positive_split_discipline`, so the ~3% line
/// (`feedback::POSITIVE_SPLIT_FLAG_PCT`) is decided in exactly one place: a
/// shell renders `verdict`/`label`/`message` verbatim, never re-deriving the
/// threshold. "fade"/"even"/"negative" are wire strings the shell matches on.
fn split_verdict_view(split_pct: f64) -> SplitVerdictView {
    let f = feedback::positive_split_discipline(split_pct);
    let (verdict, label, message) = match f.value {
        // feedback-016: >3% back-half slowdown on an even-effort run → the
        // easier-start cue toward an even-to-negative split.
        FeedbackCategory::IntensityDiscipline => (
            "fade",
            format!("FADE +{split_pct:.0}%"),
            format!(
                "Back half {split_pct:.0}% slower - start easier and aim for an even-to-negative split."
            ),
        ),
        // feedback-017: even or negative split → pacing-discipline praise.
        _ if split_pct < -feedback::POSITIVE_SPLIT_FLAG_PCT => (
            "negative",
            format!("NEG SPLIT {:.0}%", split_pct.abs()),
            "Negative split - textbook pacing discipline.".to_string(),
        ),
        _ => (
            "even",
            "EVEN SPLIT".to_string(),
            "Even split - textbook pacing discipline.".to_string(),
        ),
    };
    SplitVerdictView {
        verdict: verdict.to_string(),
        label,
        message,
        grade: format!("{:?}", f.evidence.grade),
        citation: f.evidence.citation.reference.clone(),
        confidence: f.confidence.score,
        safety_critical: f.confidence.safety_critical,
        contested: f.confidence.contested,
    }
}

/// Format a duration in seconds as a race clock: `h:mm:ss` past an hour, else
/// `m:ss`. Non-positive input renders as `-` (no false prediction).
fn fmt_race_clock(sec: f64) -> String {
    if sec <= 0.0 {
        return "-".to_string();
    }
    let s = sec.round() as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Render an inclusive integer range, collapsing to a single value when the
/// bounds coincide (e.g. `(3, 3) -> "3"`, `(6, 9) -> "6–9"`). Keeps guidance
/// copy from printing degenerate `3–3` spans.
fn fmt_u8_range(lo: u8, hi: u8) -> String {
    if lo == hi {
        lo.to_string()
    } else {
        format!("{lo}–{hi}")
    }
}

/// Human label for a goal race distance: the classic names at their standard
/// metrages (within 10 m), otherwise a plain kilometre figure.
fn race_distance_label(m: f64) -> String {
    match m {
        d if (d - 5_000.0).abs() < 10.0 => "5K".to_string(),
        d if (d - 10_000.0).abs() < 10.0 => "10K".to_string(),
        d if (d - 21_097.5).abs() < 10.0 => "Half marathon".to_string(),
        d if (d - 42_195.0).abs() < 10.0 => "Marathon".to_string(),
        d => format!("{:.1} km", d / 1000.0),
    }
}

/// Combine a Daniels VDOT projection and a Riegel projection into a graded
/// finish-time prediction (running-039). Agreement within ~2% collapses to a
/// single time; otherwise a low–high range is shown so neither method's false
/// precision stands alone. Degenerate input yields an empty (`-`) prediction.
fn to_race_view(q: &RaceQuery, longest_logged_km: Option<f64>) -> RacePredictionView {
    let exponent = load::riegel_exponent(q.weekly_km);
    let riegel_sec = load::riegel_predict(
        q.recent_time_sec,
        q.recent_distance_m,
        q.goal_distance_m,
        exponent,
    );
    let vdot = load::vdot(q.recent_distance_m, q.recent_time_sec);
    let daniels_sec = load::daniels_predict(vdot, q.goal_distance_m);
    let eq = running::race_equivalency(riegel_sec, daniels_sec);
    let (agreed, low_sec, high_sec) = match eq.value {
        running::Equivalency::Agreed(mid) => (true, mid, mid),
        running::Equivalency::Range(lo, hi) => (false, lo, hi),
    };

    let goal_label = race_distance_label(q.goal_distance_m);
    let degenerate = low_sec <= 0.0;
    let predicted = if degenerate {
        "-".to_string()
    } else if agreed {
        fmt_race_clock(low_sec)
    } else {
        format!("{}–{}", fmt_race_clock(low_sec), fmt_race_clock(high_sec))
    };
    let summary = if degenerate {
        format!("{goal_label}: need a valid recent race to predict")
    } else if agreed {
        format!("{goal_label} ≈ {predicted} (Daniels & Riegel agree)")
    } else {
        format!("{goal_label} ≈ {predicted} (Daniels–Riegel range)")
    };

    // Graded caveats riding on the prediction. running-041: the input race
    // must be recent (≤6 wk strict, 7–8 marginal, >8 stale → re-test).
    let mut notes = Vec::new();
    if let Some(weeks) = q.weeks_since_race {
        let fresh = running::race_input_freshness(weeks);
        match fresh.value {
            running::RaceInputFreshness::Fresh => {}
            running::RaceInputFreshness::Marginal => push_guidance(
                &mut notes,
                "Prediction",
                format!("Input race is {weeks} weeks old - at the edge of the 6–8-week freshness window"),
                &fresh,
            ),
            running::RaceInputFreshness::Stale => push_guidance(
                &mut notes,
                "Prediction",
                format!("Input race is {weeks} weeks old (>8) - re-test before trusting these paces"),
                &fresh,
            ),
        }
    }
    // running-030: marathon predictions run optimistic without long-run
    // support. Judged from the logged run history; no history → no claim.
    if (q.goal_distance_m - 42_195.0).abs() < 10.0
        && let Some(longest) = longest_logged_km
    {
        let opt = running::marathon_prediction_optimistic(longest);
        if opt.value {
            // running-008: the matching correction, derate the projection by
            // ~2–3 VDOT points for an under-mileaged marathoner.
            let derate =
                running::vdot_derate_points(GoalDistance::Marathon, true);
            push_guidance(
                &mut notes,
                "Prediction",
                format!(
                    "Longest logged run {longest:.1} km - marathon predictions run optimistic without long-run support (derate ~{:.0}–{:.0} VDOT points)",
                    derate.value.0, derate.value.1
                ),
                &opt,
            );
        }
    }

    RacePredictionView {
        goal_label,
        predicted,
        agreed,
        low_sec,
        high_sec,
        summary,
        grade: format!("{:?}", eq.evidence.grade),
        citation: eq.evidence.citation.reference.clone(),
        confidence: eq.confidence.score,
        safety_critical: eq.confidence.safety_critical,
        contested: eq.confidence.contested,
        notes,
    }
}

/// Wrap a raw value in a `Recommended` carrying a registry claim's evidence +
/// confidence, the same mechanism `hypertrophy::recommend` uses, so a
/// non-`Recommended` engine value (the volume landmarks) still surfaces graded
/// (HARD RULE 2). Panics only on a missing claim id (a compile-time constant).
fn graded<T>(value: T, claim_id: &str) -> Recommended<T> {
    let c = crate::evidence::claim(claim_id).expect("known claim id");
    Recommended::new(value, c.to_evidence(), c.to_confidence_tag())
}

/// Build a graded per-week hypertrophy accumulation plan for one muscle. Every
/// row carries its own evidence + confidence via [`push_guidance`] (HARD RULE 2);
/// no training numbers are invented: all come from [`hypertrophy`]. An unknown
/// muscle yields a single explanatory row (no fabricated landmarks). `weeks == 0`
/// yields no plan rows beyond the landmarks context.
fn build_hypertrophy_plan(q: &HypertrophyPlanQuery, profile: Option<&Profile>) -> Vec<GuidanceView> {
    let mut rows = Vec::new();
    let section = q.muscle.clone();

    let Some(lm) = hypertrophy::landmarks_for(&q.muscle) else {
        // No landmarks for this muscle: say so plainly rather than guess. The
        // note itself is the RP landmark framing, so it is graded from that claim.
        let note = graded((), "HYP-LANDMARKS-001");
        push_guidance(
            &mut rows,
            &section,
            format!(
                "\"{}\" is not a known muscle - pick one of: chest, back, quads, hamstrings, glutes, side delts, rear delts, biceps, triceps, calves, abs",
                q.muscle
            ),
            &note,
        );
        return rows;
    };

    // Landmarks row, landmarks are a raw engine value, not a Recommended, so
    // wrap them in their HYP-LANDMARKS-001 evidence (ExpertOpinion) to grade it.
    let landmarks = graded((), "HYP-LANDMARKS-001");
    push_guidance(
        &mut rows,
        &section,
        format!(
            "Landmarks - MEV {} · MAV {}–{} · MRV {} sets/wk",
            lm.mev, lm.mav.0, lm.mav.1, lm.mrv
        ),
        &landmarks,
    );

    // `weeks == 0` is degenerate: the ramp is empty and there is no schedule to
    // show, so stop after the landmarks context rather than emit empty rows.
    if q.weeks == 0 {
        return rows;
    }

    // Weekly set ramp MEV → MRV across the accumulation block.
    let ramp = hypertrophy::weekly_set_ramp(lm.mev, lm.mrv, q.weeks);
    let ramp_str = ramp
        .value
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(" → ");
    push_guidance(
        &mut rows,
        &section,
        format!("Weekly set ramp: {ramp_str}"),
        &ramp,
    );

    // RIR schedule across the block (first week's Recommended carries the grade).
    let rir_vals: Vec<u8> = (1..=q.weeks)
        .filter_map(|w| hypertrophy::rir_for_week(w, q.weeks).map(|r| r.value))
        .collect();
    if let Some(first) = hypertrophy::rir_for_week(1, q.weeks) {
        let rir_str = rir_vals
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" → ");
        push_guidance(
            &mut rows,
            &section,
            format!("RIR schedule: {rir_str}"),
            &first,
        );
    }

    // Peak-week frequency + per-session spread (from the ramp's top set count).
    let peak_sets = ramp.value.iter().copied().max().unwrap_or(lm.mrv);
    let freq = hypertrophy::frequency_for_weekly_sets(peak_sets);
    push_guidance(
        &mut rows,
        &section,
        format!(
            "Peak frequency: {}×/wk · {} sets/session",
            fmt_u8_range(freq.value.freq.0, freq.value.freq.1),
            fmt_u8_range(freq.value.per_session.0, freq.value.per_session.1)
        ),
        &freq,
    );

    // hypertrophy-004: warn when the implied per-session dose exceeds the ~11
    // fractional-set ceiling, redistribute to another session, not more sets.
    let over_cap = hypertrophy::per_session_over_cap(freq.value.per_session.1);
    if over_cap.value {
        push_guidance(
            &mut rows,
            &section,
            format!(
                "{} sets in one session exceeds the ~11-set per-session cap - add a session instead of stacking sets",
                freq.value.per_session.1
            ),
            &over_cap,
        );
    }

    // hypertrophy-013/039 rest prescriptions per exercise class, so the plan
    // states how long to rest, not only how much to lift.
    for (label, class) in [
        ("heavy compounds", hypertrophy::ExerciseClass::HeavyCompound),
        ("isolation work", hypertrophy::ExerciseClass::Isolation),
    ] {
        let rest = hypertrophy::rest_sec_for(class);
        let (lo, hi) = rest.value;
        push_guidance(
            &mut rows,
            &section,
            format!(
                "Rest between sets ({label}): {:.0}–{:.0} min",
                f64::from(lo) / 60.0,
                f64::from(hi) / 60.0
            ),
            &rest,
        );
    }

    // hypertrophy-031: the block's macro-shape (accumulate : deload cadence).
    let ms = hypertrophy::meso_structure();
    push_guidance(
        &mut rows,
        &section,
        format!(
            "Mesocycle shape: {}–{} accumulation weeks + {} deload week (deload every {}–{} weeks)",
            ms.value.accumulation_weeks.0,
            ms.value.accumulation_weeks.1,
            ms.value.deload_weeks,
            ms.value.deload_cadence_weeks.0,
            ms.value.deload_cadence_weeks.1
        ),
        &ms,
    );

    // hypertrophy-018: default working proximity to failure.
    let rir_band = hypertrophy::default_rir_band();
    push_guidance(
        &mut rows,
        &section,
        format!(
            "Work most sets at {}–{} RIR - true failure is not required and costs recovery",
            rir_band.value.0, rir_band.value.1
        ),
        &rir_band,
    );

    // hypertrophy-021: where failure is permitted at all (machines/isolation;
    // never heavy free-weight compounds). Educational, the calls' arguments
    // only inherit the rule's citation.
    let fail = hypertrophy::failure_allowed(hypertrophy::ExerciseClass::Isolation, true);
    push_guidance(
        &mut rows,
        &section,
        "Take sets to failure only on machines/isolation - never on heavy free-weight compounds"
            .to_string(),
        &fail,
    );

    // hypertrophy-017: high-skill exercise guard.
    let guard = hypertrophy::high_skill_guard();
    push_guidance(
        &mut rows,
        &section,
        format!(
            "High-skill/high-stability lifts: keep reps ≥{} and stop at ≥{}–{} RIR to protect technique",
            guard.value.min_reps, guard.value.min_rir.0, guard.value.min_rir.1
        ),
        &guard,
    );

    // hypertrophy-012: load interchangeability window.
    let range = hypertrophy::interchangeable_load_range();
    push_guidance(
        &mut rows,
        &section,
        format!(
            "Loads are interchangeable for growth across {}–{}% 1RM ({}–{} reps) when sets are near failure",
            range.value.pct_1rm.0, range.value.pct_1rm.1, range.value.reps.0, range.value.reps.1
        ),
        &range,
    );

    // hypertrophy-040: controlled tempo window.
    let tempo = hypertrophy::tempo_rx();
    push_guidance(
        &mut rows,
        &section,
        format!(
            "Tempo: controlled {}–{} s/rep ({}–{} s up, {}–{} s down) - superslow (>10 s) is inferior",
            tempo.value.rep_duration_s.0,
            tempo.value.rep_duration_s.1,
            tempo.value.concentric_s.0,
            tempo.value.concentric_s.1,
            tempo.value.eccentric_s.0,
            tempo.value.eccentric_s.1
        ),
        &tempo,
    );

    // hypertrophy-008: next-mesocycle volume decision. The current weekly-set
    // base is the profile's planned weekly sets when one exists, else the
    // muscle's MEV (the block's own starting point).
    let current = profile.map(|p| p.weekly_sets).unwrap_or(lm.mev);
    let next = hypertrophy::next_meso_weekly_sets(current, q.not_growing, q.recovering_easily);
    push_guidance(
        &mut rows,
        &section,
        if next.value > current {
            format!(
                "Not growing while recovering easily - raise next mesocycle to {} sets/wk (from {current})",
                next.value
            )
        } else {
            format!("Next mesocycle: hold {current} sets/wk (no add indicated)")
        },
        &next,
    );

    // hyp-001 ramp discipline: warn when the plan's first week is an abrupt
    // jump from the athlete's current weekly sets.
    if let Some(p) = profile
        && let Some(&first_week) = ramp.value.first()
    {
        let jump = hypertrophy::abrupt_volume_jump(p.weekly_sets, first_week);
        if jump.value {
            push_guidance(
                &mut rows,
                &section,
                format!(
                    "Plan starts at {first_week} sets/wk vs your current {} - too abrupt; step volume up gradually",
                    p.weekly_sets
                ),
                &jump,
            );
        }
    }

    // Training-age extras: the novice RIR calibration start, the intermediate
    // default program synthesis.
    if let Some(p) = profile {
        match individualization::training_age_from_cadence(p.progression_cadence).value {
            individualization::TrainingAge::Novice => {
                let start = hypertrophy::novice_start_rir();
                push_guidance(
                    &mut rows,
                    &section,
                    format!(
                        "New to RIR: start at {}–{} RIR and calibrate against an occasional true-failure set",
                        start.value.0, start.value.1
                    ),
                    &start,
                );
            }
            individualization::TrainingAge::Intermediate => {
                let prog = hypertrophy::intermediate_default_program();
                push_guidance(
                    &mut rows,
                    &section,
                    format!(
                        "Default template: each muscle {}×/wk, {}–{} sets/wk, compounds {}–{} reps / isolation {}–{}, RIR {}→{}, deload week {}–{}",
                        prog.value.frequency_per_week,
                        prog.value.weekly_sets.0,
                        prog.value.weekly_sets.1,
                        prog.value.compound_reps.0,
                        prog.value.compound_reps.1,
                        prog.value.isolation_reps.0,
                        prog.value.isolation_reps.1,
                        prog.value.week1_rir,
                        prog.value.final_rir,
                        prog.value.deload_week.0,
                        prog.value.deload_week.1
                    ),
                    &prog,
                );
            }
            individualization::TrainingAge::Advanced => {}
        }
    }

    rows
}

/// Build absolute daily protein target rows by scaling each graded g/kg range by
/// the athlete's bodyweight. Multiplying a graded g/kg bound by bodyweight is
/// honest arithmetic, the grade travels with the underlying claim via
/// [`push_guidance`] (HARD RULE 2). No general/default protein number is
/// invented: if neither goal context is selected (or bodyweight is non-positive)
/// the section is empty (HARD RULE 1).
///
/// `reds_present` carries the RED-S flag from the readiness inputs or the
/// onboarding screen: with it set, a requested deficit target is REFUSED inside
/// `individualization::deficit_protein_target` (File 08 safety-022) and the row
/// becomes the safety-critical deferral instead of a number.
fn build_protein_targets(q: &ProteinQuery, reds_present: bool) -> Vec<GuidanceView> {
    let mut rows = Vec::new();

    // Cannot derive g/day from a non-positive bodyweight, say nothing rather
    // than emit a nonsensical or zero target.
    if q.bodyweight_kg <= 0.0 {
        return rows;
    }

    if q.masters {
        let r = individualization::masters_protein_target();
        let (lo, hi) = r.value.g_per_kg;
        push_guidance(
            &mut rows,
            "Protein",
            format!(
                "Masters (65+): {:.0}–{:.0} g/day ({lo:.1}–{hi:.1} g/kg × {:.0} kg)",
                q.bodyweight_kg * lo,
                q.bodyweight_kg * hi,
                q.bodyweight_kg
            ),
            &r,
        );
    }

    if q.deficit {
        let r = individualization::deficit_protein_target(reds_present);
        match r.value {
            Some(t) => {
                let (lo, hi) = t.g_per_kg;
                push_guidance(
                    &mut rows,
                    "Protein",
                    format!(
                        "Deficit (lean-mass preserving): {:.0}–{:.0} g/day ({lo:.1}–{hi:.1} g/kg × {:.0} kg)",
                        q.bodyweight_kg * lo,
                        q.bodyweight_kg * hi,
                        q.bodyweight_kg
                    ),
                    &r,
                );
            }
            None => {
                // safety-022: the deficit is refused, not silently omitted -
                // the user sees why, cited to the RED-S deferral.
                push_guidance(
                    &mut rows,
                    "Protein",
                    "Deficit not prescribed - a RED-S / low-energy-availability signal is present. Reduce training stress and consult a physician or registered dietitian before any caloric deficit.".to_string(),
                    &r,
                );
            }
        }
    }

    rows
}

/// Build a graded heart-rate-zone table from age: the Tanaka HRmax estimate plus
/// the five Daniels %HRmax training bands mapped to absolute bpm ranges
/// (`running::vdot_band_hr_pct`, running-007). The HRmax rows carry the Tanaka
/// formula's own claim RUN-HRMAX-001 (Weak, ±10 bpm SEE), citing the VDOT
/// claim here would overstate the formula's evidence; the %HRmax band rows are
/// Daniels tables (RUN-VDOT-001, Moderate). No training numbers are invented
/// (HARD RULE 1/2). A non-positive or implausible age yields a single
/// explanatory row rather than a bogus HRmax.
fn build_hr_zones(q: &HrZoneQuery) -> Vec<GuidanceView> {
    let mut rows = Vec::new();

    // HRmax from age is only meaningful for a realistic adult/junior age; refuse
    // to emit a fabricated maximum outside that range.
    if !(5.0..=100.0).contains(&q.age_years) {
        let note = graded((), "RUN-HRMAX-001");
        push_guidance(
            &mut rows,
            "Heart-rate zones",
            "Enter an age between 5 and 100 to estimate HRmax and training zones.".to_string(),
            &note,
        );
        return rows;
    }

    // running::hr_max_tanaka is the same Tanaka 208 − 0.7·age estimator the
    // load module exposes; the running-module wrapper is used so the zone
    // table and the running rules share one source.
    let hr_max = running::hr_max_tanaka(q.age_years);
    let hr_max_row = graded((), "RUN-HRMAX-001");
    push_guidance(
        &mut rows,
        "Heart-rate zones",
        format!(
            "Estimated HRmax: {hr_max:.0} bpm (Tanaka 208 − 0.7 × {:.0})",
            q.age_years
        ),
        &hr_max_row,
    );

    // running-005: HR-method preference by resting HR. Below RHR 55 the
    // %HRmax and Karvonen methods diverge → prefer Karvonen; ≥70 they
    // converge; 55–69 the KB states no rule (said honestly).
    let karvonen = q.resting_hr_bpm.map(|rhr| {
        let pref = running::hr_method_preference(rhr);
        let (text, use_karvonen) = match pref.value {
            running::HrMethodPreference::PreferKarvonen => (
                format!(
                    "Resting HR {rhr:.0} < 55 - %HRmax and %HRR diverge; Karvonen (%HRR) targets shown per band"
                ),
                true,
            ),
            running::HrMethodPreference::EitherConverged => (
                format!("Resting HR {rhr:.0} ≥ 70 - the two HR methods converge; either works"),
                false,
            ),
            running::HrMethodPreference::Unstated => (
                format!(
                    "Resting HR {rhr:.0} is in the 55–69 range, where the source states no method rule - %HRmax shown"
                ),
                false,
            ),
        };
        push_guidance(&mut rows, "Heart-rate zones", text, &pref);
        // The boolean view of the same rule drives the band targets below;
        // the two stay consistent by construction.
        debug_assert_eq!(use_karvonen, running::prefer_karvonen(rhr).value);
        running::prefer_karvonen(rhr).value.then_some(rhr)
    });

    // Daniels VDOT bands, easy → hard. Each carries its own RUN-VDOT-001
    // grade; running-002 marks which bands are HR-anchored at all: the fast
    // bands are pace-governed and HR is only a coarse ceiling there.
    for band in [
        VdotBand::Easy,
        VdotBand::Marathon,
        VdotBand::Threshold,
        VdotBand::Interval,
        VdotBand::Repetition,
    ] {
        let (lo_pct, hi_pct) = running::vdot_band_hr_pct(band);
        let (vo2_lo, vo2_hi) = running::vdot_band_vo2max_pct(band);
        let hr_anchored = running::vdot_band_uses_hr(band);
        let bpm_lo = hr_max * lo_pct / 100.0;
        let bpm_hi = hr_max * hi_pct / 100.0;
        let band_row = graded((), "RUN-VDOT-001");
        let range = if (bpm_hi - bpm_lo).abs() < 0.5 {
            format!("{bpm_lo:.0} bpm")
        } else {
            format!("{bpm_lo:.0}–{bpm_hi:.0} bpm")
        };
        // Karvonen targets per band when the preference selected them
        // (running-005 formula: target = RHR + frac·(HRmax − RHR)).
        let karvonen_seg = match karvonen {
            Some(Some(rhr)) if hr_anchored => {
                let k_lo = load::karvonen_target_hr(rhr, hr_max, lo_pct / 100.0);
                let k_hi = load::karvonen_target_hr(rhr, hr_max, hi_pct / 100.0);
                format!(" · Karvonen {k_lo:.0}–{k_hi:.0} bpm")
            }
            _ => String::new(),
        };
        let anchor_seg = if hr_anchored {
            ""
        } else {
            " · pace-governed (HR not valid here)"
        };
        push_guidance(
            &mut rows,
            "Heart-rate zones",
            format!(
                "{band:?}: {lo_pct:.0}–{hi_pct:.0} %HRmax ({vo2_lo:.0}–{vo2_hi:.0} %VO2max) → {range}{karvonen_seg}{anchor_seg}"
            ),
            &band_row,
        );
    }

    // running-036: Maffetone aerobic cap, a base-phase OPTION, never the
    // default (contested vs measured LT1; the claim carries that).
    let maf = running::maf_cap_bpm(q.age_years, running::MafAdjustment::None);
    push_guidance(
        &mut rows,
        "Heart-rate zones",
        format!(
            "MAF aerobic cap (base-phase option): 180 − age = {:.0} bpm - personalize toward measured LT1 when data exist",
            maf.value
        ),
        &maf,
    );

    // running-006: recompute zones off a measured max every 4–6 weeks.
    if let Some(weeks) = q.weeks_since_recalc {
        let due = running::hr_zone_recalc_due(weeks);
        if due.value {
            push_guidance(
                &mut rows,
                "Heart-rate zones",
                format!("Zones last recalculated {weeks} weeks ago - recompute from a measured HRmax (every 4–6 weeks)"),
                &due,
            );
        }
    }

    // running-041: training paces re-test on the same 4–6-week cadence.
    if let Some(weeks) = q.weeks_since_pace_test {
        let due = running::pace_retest_due(weeks);
        if due.value {
            push_guidance(
                &mut rows,
                "Heart-rate zones",
                format!("Paces last tested {weeks} weeks ago - re-test to set paces from CURRENT fitness"),
                &due,
            );
        }
    }

    rows
}

/// Flatten every logged set, threading each exercise's previous e1RM through so
/// the view carries the per-lift trend (delta + direction) and the shell renders
/// it without arithmetic. "Previous" = the most recent earlier logged set of the
/// same exercise (exact name match), the core holds no session boundary for
/// sets, so set-over-set is the deterministic proxy for session-over-session.
fn lift_views(sets: &[LoggedSet]) -> Vec<LiftResultView> {
    let mut last_e1rm: std::collections::HashMap<&str, f64> = std::collections::HashMap::new();
    sets.iter()
        .map(|s| {
            let prev = last_e1rm.get(s.exercise.as_str()).copied();
            let view = to_lift_view(s, prev);
            last_e1rm.insert(s.exercise.as_str(), view.e1rm_kg);
            view
        })
        .collect()
}

/// Derive strength metrics for one logged set (Epley e1RM, RIR from RPE).
/// `prev_e1rm_kg` is the same exercise's previous e1RM (already 0.1-rounded),
/// `None` for its first logged set.
fn to_lift_view(s: &LoggedSet, prev_e1rm_kg: Option<f64>) -> LiftResultView {
    let e1rm_kg = (strength::e1rm_epley(s.weight_kg, s.reps) * 10.0).round() / 10.0;
    let pct_1rm = strength::est_pct_1rm_from_reps(s.reps).round();
    let rir = strength::rpe_to_rir(s.rpe);

    // strength-005/006: cross-check the e1RM across Epley/Brzycki/Lombardi and
    // surface the spread. Isolation lifts (per the File 03 exercise catalog)
    // and out-of-range rep counts yield `None`: the formulas are unreliable
    // there (the shell should suggest a 3–6-rep test set instead). An exercise
    // the catalog does not know is treated as non-isolation: the exclusion is
    // isolation-specific, not unknown-specific.
    let isolation = hypertrophy::exercise_entry(&s.exercise)
        .is_some_and(|e| e.class == hypertrophy::ExerciseClass::Isolation);
    let check = strength::e1rm_cross_check(s.weight_kg, s.reps, isolation);
    let cross_check = check.value.map(|c| E1rmRangeView {
        low_kg: (c.low_kg * 10.0).round() / 10.0,
        high_kg: (c.high_kg * 10.0).round() / 10.0,
        formulas: c.formulas_used,
        grade: format!("{:?}", check.evidence.grade),
        citation: check.evidence.citation.reference.clone(),
        confidence: check.confidence.score,
        safety_critical: check.confidence.safety_critical,
        contested: check.confidence.contested,
    });
    // Delta of two 0.1-rounded values, re-rounded to kill float dust; direction
    // is factual (what changed), never an improving/declining judgment, that
    // phrasing belongs to the trend rules (see the field docs).
    let e1rm_delta_kg = prev_e1rm_kg.map(|p| ((e1rm_kg - p) * 10.0).round() / 10.0);
    let e1rm_direction = e1rm_delta_kg.map(|d| {
        if d > 0.0 {
            "up".to_string()
        } else if d < 0.0 {
            "down".to_string()
        } else {
            "flat".to_string()
        }
    });
    LiftResultView {
        exercise: s.exercise.clone(),
        weight_kg: s.weight_kg,
        reps: s.reps,
        rpe: s.rpe,
        e1rm_kg,
        pct_1rm,
        rir,
        cross_check,
        e1rm_delta_kg,
        e1rm_direction,
        summary: format!(
            // `{}` (not `{:.0}`) on the logged weight so a fractional plate load
            // (e.g. 92.5 kg on 2.5 kg jumps) shows as "92.5kg", not a truncated
            // "92kg": the summary is the human line the shell renders, and it must
            // echo what the lifter actually did. Integer loads still print clean
            // ("100kg", not "100.0kg").
            "{} {}kg × {} @RPE{:.1} → e1RM {:.1}kg (~{:.0}%1RM, {:.1} RIR)",
            s.exercise, s.weight_kg, s.reps, s.rpe, e1rm_kg, pct_1rm, rir
        ),
        observed_at: s.observed_at,
    }
}

/// Flatten one evidence-wrapped adjustment into a shell-facing row.
fn to_view(r: &Recommended<Adjustment>) -> AdjustmentView {
    to_view_with(describe(&r.value), r)
}

/// Flatten any evidence-wrapped value into an adjustment row with an explicit
/// summary, for graded verdicts that are not `Adjustment`s (e.g. the
/// autoreg-032 threshold re-test cue).
fn to_view_with<T>(summary: String, r: &Recommended<T>) -> AdjustmentView {
    AdjustmentView {
        summary,
        grade: format!("{:?}", r.evidence.grade),
        citation: r.evidence.citation.reference.clone(),
        confidence: r.confidence.score,
        safety_critical: r.confidence.safety_critical,
        contested: r.confidence.contested,
    }
}

/// Human-readable one-liner for an adjustment.
fn describe(a: &Adjustment) -> String {
    match a {
        Adjustment::ReduceLoadPct(p) => format!("Reduce load {p:.0}% for remaining sets"),
        Adjustment::IncreaseLoadPct(p) => format!("Increase load {p:.0}% - readiness is high"),
        Adjustment::Deload {
            volume_reduction_pct,
            load_reduction_pct,
            weeks,
        } => format!(
            "Deload {weeks} wk: volume −{volume_reduction_pct:.0}%, load −{load_reduction_pct:.0}%"
        ),
        Adjustment::CapRpe(d) => {
            format!("Cap today's session at planned RPE −{d:.0}")
        }
        Adjustment::DowngradeSession => "Downgrade to an easier session".into(),
        Adjustment::ModifyAndMonitor => {
            "Modify the provoking exercise and continue with monitoring - avoid complete rest"
                .into()
        }
        Adjustment::RestDay => "Take a full rest day".into(),
        Adjustment::Stop => "Stop - do not train".into(),
        Adjustment::Defer { reason } => reason.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ReadinessSignal;

    fn input(signal: ReadinessSignal, value: f64) -> ReadinessInput {
        ReadinessInput {
            signal,
            value,
            observed_at: 0,
            streak: 0,
            pain: None,
            effort_min: None,
        }
    }

    /// The Android shell hand-serializes each [`Event`] to serde's external
    /// tagging (`Core.kt::toJson`) and omits every absent optional field. Replay
    /// feeds those exact bytes back through `update`, and the FFI *swallows*
    /// deserialize errors (`ffi::process_event` clears the output on failure), so
    /// a drift between Kotlin's wire form and this enum's `Deserialize` would
    /// silently drop the event on every cold start. Pin the omit-optionals form.
    #[test]
    fn android_wire_form_deserializes_with_optionals_omitted() {
        // Exactly what Core.kt emits for a review with no run/lift context: only
        // the four non-optional booleans/count, every Option field absent.
        let bare = r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"bad_day":false}}"#;
        match serde_json::from_str::<Event>(bare).expect("bare review must parse") {
            Event::SubmitReview(r) => assert_eq!(
                r,
                SessionReview::default(),
                "omitted optionals must deserialize to None/default"
            ),
            other => panic!("expected SubmitReview, got {other:?}"),
        }

        // SetProfile carries no optionals, but pin its snake_case field contract
        // too, a renamed field here silently drops the profile on replay.
        let profile = r#"{"SetProfile":{"progression_cadence":"WeekToWeek","lift_goal":"MaxStrength","goal_distance":"TenK","concurrent_goal":"Strength","weekly_sets":14,"running_days_per_week":4,"running_km_per_week":45.0,"advanced":false,"endurance_intensity_pct_vo2max":75.0}}"#;
        assert!(
            matches!(
                serde_json::from_str::<Event>(profile),
                Ok(Event::SetProfile(_))
            ),
            "profile wire form must parse"
        );
    }

    /// HARD RULE 3 at the wire level: the shell's red-flag readiness buttons must
    /// never be silently dropped on replay. Each safety signal's Kotlin `.name`
    /// string (`Core.kt::ReadinessSignal`) must deserialize to the matching
    /// variant, a rename on either side would make the FFI swallow the event
    /// (deserialize error → cleared output) and leave a red-flag session
    /// un-blocked. Pin the safety signals' exact wire names.
    #[test]
    fn safety_readiness_signals_deserialize_from_shell_names() {
        for (name, expected) in [
            ("Pain", ReadinessSignal::Pain),
            ("Illness", ReadinessSignal::Illness),
            ("RedS", ReadinessSignal::RedS),
            ("CardiacRedFlag", ReadinessSignal::CardiacRedFlag),
            ("BoneStress", ReadinessSignal::BoneStress),
        ] {
            let wire = format!(
                r#"{{"SubmitReadiness":{{"signal":"{name}","value":1.0,"observed_at":0}}}}"#
            );
            match serde_json::from_str::<Event>(&wire).expect("safety signal must parse") {
                Event::SubmitReadiness(i) => {
                    assert_eq!(i.signal, expected, "{name} deserialized to wrong signal")
                }
                other => panic!("expected SubmitReadiness for {name}, got {other:?}"),
            }
        }
    }

    /// Wire pin for the additive readiness fields (`streak`, `pain`): the
    /// pre-existing three-field shape must keep parsing (defaults), and the new
    /// graded-pain shape must parse field-for-field as Core.kt will emit it.
    /// Kotlin-side `ignoreUnknownKeys` tolerates the view side; this guards the
    /// event side, where a drift would make the FFI silently drop the report.
    #[test]
    fn readiness_wire_shape_old_and_graded_pain_forms_parse() {
        // Legacy shape: no streak, no pain detail → conservative defaults.
        let legacy = r#"{"SubmitReadiness":{"signal":"Pain","value":1.0,"observed_at":0}}"#;
        match serde_json::from_str::<Event>(legacy).expect("legacy shape must parse") {
            Event::SubmitReadiness(i) => {
                assert_eq!(i.streak, 0);
                assert_eq!(i.pain, None, "missing detail must default to None (hard stop)");
            }
            other => panic!("expected SubmitReadiness, got {other:?}"),
        }

        // Graded shape, snake_case fields, unit-variant enum strings, and
        // `persists` omitted (serde default false).
        let graded = r#"{"SubmitReadiness":{"signal":"Pain","value":1.0,"observed_at":0,"streak":2,"pain":{"kind":"TendonLoadRelated","severity":3,"trend":"Stable"}}}"#;
        match serde_json::from_str::<Event>(graded).expect("graded shape must parse") {
            Event::SubmitReadiness(i) => {
                assert_eq!(i.streak, 2);
                let d = i.pain.expect("pain detail parsed");
                assert_eq!(d.kind, crate::schema::PainKind::TendonLoadRelated);
                assert_eq!(d.severity, 3);
                assert_eq!(d.trend, crate::schema::PainTrend::Stable);
                assert!(!d.persists, "omitted persists defaults to false");
            }
            other => panic!("expected SubmitReadiness, got {other:?}"),
        }
    }

    #[test]
    fn graded_tendon_pain_modifies_without_blocking_through_the_view() {
        // File 08 Table 4.1 through the full event→view contract: tolerable
        // tendon pain (≤5/10, stable) surfaces the Pain tier and a
        // modify-and-monitor adjustment, but must NOT block training ("avoid
        // complete rest"), unlike the bare Pain report below, which stays a
        // conservative hard stop.
        let app = Engine;
        let mut model = Model::default();
        let event = r#"{"SubmitReadiness":{"signal":"Pain","value":1.0,"observed_at":0,"pain":{"kind":"TendonLoadRelated","severity":3,"trend":"Stable","persists":false}}}"#;
        let event: Event = serde_json::from_str(event).expect("graded pain event parses");
        app.update(event, &mut model).expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.safety_tier.as_deref(), Some("Pain"));
        assert!(!vm.train_blocked, "tolerable tendon pain must not block");
        let adj = vm
            .adjustments
            .iter()
            .find(|a| a.summary.contains("avoid complete rest"))
            .expect("modify-and-monitor adjustment surfaces");
        assert!(adj.safety_critical);
        assert_eq!(adj.grade, "Moderate");
    }

    #[test]
    fn pain_blocks_training_with_a_single_stop() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)),
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.safety_tier.as_deref(), Some("Pain"));
        assert!(vm.train_blocked);
        assert_eq!(vm.adjustments.len(), 1);
        assert_eq!(vm.adjustments[0].summary, "Stop - do not train");
    }

    #[test]
    fn medical_red_flags_surface_referral_tier_through_the_view() {
        // The Android shell can now submit the medical-referral signals
        // (Illness/RedS/CardiacRedFlag/BoneStress). Lock the full event→view
        // contract those buttons depend on: each must raise the top
        // `MedicalReferral` tier and block training, so the SafetyBanner shows a
        // DO-NOT-TRAIN state rather than silently swallowing a red flag.
        for signal in [
            ReadinessSignal::RedS,
            ReadinessSignal::CardiacRedFlag,
            ReadinessSignal::BoneStress,
        ] {
            let app = Engine;
            let mut model = Model::default();
            app.update(Event::SubmitReadiness(input(signal, 1.0)), &mut model)
                .expect_only_render();
            let vm = app.view(&model);
            assert_eq!(
                vm.safety_tier.as_deref(),
                Some("MedicalReferral"),
                "{signal:?} must raise the referral tier"
            );
            assert!(vm.train_blocked, "{signal:?} must block training");
        }
    }

    #[test]
    fn illness_severity_bands_map_to_tier_through_the_view() {
        // The Android ReadinessEditor sends Illness as a severity value (0 none,
        // 1 above-neck, 2 below-neck/fever) via its IllnessLevel picker. Lock the
        // full event→view contract that picker depends on: above-neck raises the
        // Illness tier without blocking (a downgraded session), while below-neck/
        // fever must block training outright. A drift in the value bands would let
        // the shell submit a fever as a mere downgrade.
        let app = Engine;
        let mut above = Model::default();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Illness, 1.0)),
            &mut above,
        )
        .expect_only_render();
        let vm = app.view(&above);
        assert_eq!(vm.safety_tier.as_deref(), Some("Illness"));
        assert!(
            !vm.train_blocked,
            "above-neck illness downgrades but must not block training"
        );

        let mut fever = Model::default();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Illness, 2.0)),
            &mut fever,
        )
        .expect_only_render();
        let vm = app.view(&fever);
        assert_eq!(vm.safety_tier.as_deref(), Some("Illness"));
        assert!(
            vm.train_blocked,
            "below-neck/fever illness must block training"
        );
    }

    #[test]
    fn resting_hr_plus_ten_blocks_training_through_the_view() {
        // autoreg-041: a morning RHR ≥ +10 bpm over baseline forces a rest day.
        // The autoreg unit tests pin rhr_stop → RestDay; this locks the rest of
        // the contract the shell renders, that the RestDay actually trips
        // train_blocked and surfaces the Illness tier (red-flag rest/neck-check,
        // never the tier-6 single-day marker while blocking).
        // A single +5 bpm reading stays a no-op (act on ≥2 days) and must NOT
        // block.
        let app = Engine;
        let mut blocked = Model::default();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::RestingHr, 10.0)),
            &mut blocked,
        )
        .expect_only_render();
        let vm = app.view(&blocked);
        assert_eq!(vm.safety_tier.as_deref(), Some("Illness"));
        assert!(vm.train_blocked, "RHR +10 bpm must block training");

        let mut downgrade = Model::default();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::RestingHr, 5.0)),
            &mut downgrade,
        )
        .expect_only_render();
        let vm = app.view(&downgrade);
        assert!(
            !vm.train_blocked,
            "a single RHR +5 bpm day must not block training"
        );
    }

    #[test]
    fn clean_inputs_leave_training_open() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Rpe, 0.0)),
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.safety_tier, None);
        assert!(!vm.train_blocked);
        assert!(vm.adjustments.is_empty());
        assert_eq!(vm.input_count, 1);
    }

    #[test]
    fn low_wellness_surfaces_cited_downgrade_without_blocking() {
        // Mirrors the shell's "Readiness" quick action (WellnessZ = −1.5): a
        // sub-threshold subjective signal must yield an optimization adjustment
        // that stays evidence-cited and does not trip the safety block.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::WellnessZ, -1.5)),
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        // A single-day subjective flag downgrades intensity but neither blocks
        // training nor raises the SubjectiveMultiDay tier: that tier is
        // defined as ≥3 days of wellness z ≤ −1 (File 06 §5 tier 4).
        assert!(!vm.train_blocked);
        assert_eq!(vm.safety_tier, None);
        let adj = vm
            .adjustments
            .iter()
            .find(|a| a.summary == "Downgrade to an easier session")
            .expect("WellnessZ −1.5 should surface a downgrade adjustment");
        assert!(!adj.grade.is_empty() && !adj.citation.is_empty());
    }

    #[test]
    fn logging_a_set_derives_e1rm_and_rir() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogSet {
                exercise: "Back squat".into(),
                weight_kg: 100.0,
                reps: 5,
                rpe: 8.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.lifts.len(), 1);
        // Epley: 100 * (1 + 5/30) = 116.7
        assert!((vm.lifts[0].e1rm_kg - 116.7).abs() < 0.05);
        // Epley inverse for 5 reps: 100 / (1 + 5/30) = 85.7 → 86%1RM.
        assert!((vm.lifts[0].pct_1rm - 86.0).abs() < f64::EPSILON);
        // RPE 8 → 2 RIR.
        assert!((vm.lifts[0].rir - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn logging_a_set_echoes_raw_input_alongside_derived_metrics() {
        // The view carries the set exactly as logged, not only the derived e1RM: a
        // lifter must see what they actually did (weight × reps @ RPE), which the
        // shell renders beside the coaching metrics. Locks that echo contract, a
        // fractional plate load must survive to the view unrounded.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogSet {
                exercise: "Front squat".into(),
                weight_kg: 92.5,
                reps: 3,
                rpe: 9.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        let lift = app.view(&model).lifts.remove(0);
        assert_eq!(lift.exercise, "Front squat");
        assert!((lift.weight_kg - 92.5).abs() < f64::EPSILON);
        assert_eq!(lift.reps, 3);
        assert!((lift.rpe - 9.0).abs() < f64::EPSILON);
        // The summary is the human line the shell shows; it names the raw set (reps
        // @ RPE), not only the derived e1RM. The fractional plate load must survive
        // here too: a 92.5 kg lift must read "92.5kg", not a truncated "92kg".
        assert!(lift.summary.contains("92.5kg"), "got {}", lift.summary);
        assert!(lift.summary.contains("× 3 @RPE9.0"), "got {}", lift.summary);
        assert!(lift.summary.contains("RIR"), "got {}", lift.summary);
    }

    #[test]
    fn logged_sets_accumulate_in_log_order() {
        // The view lists sets oldest-first, in the order they were logged: the
        // shell renders this sequence directly, so a regression that reversed or
        // de-duplicated the list would misreport a session. Two distinct sets must
        // both survive, in order.
        let app = Engine;
        let mut model = Model::default();
        for (exercise, reps) in [("Deadlift", 3), ("Bench", 8)] {
            app.update(
                Event::LogSet {
                    exercise: exercise.into(),
                    weight_kg: 80.0,
                    reps,
                    rpe: 7.0,
                    observed_at: 0,
                },
                &mut model,
            )
            .expect_only_render();
        }

        let lifts = app.view(&model).lifts;
        assert_eq!(lifts.len(), 2);
        assert_eq!(lifts[0].exercise, "Deadlift");
        assert_eq!(lifts[0].reps, 3);
        assert_eq!(lifts[1].exercise, "Bench");
        assert_eq!(lifts[1].reps, 8);
    }

    #[test]
    fn logging_a_run_classifies_zone_and_pace() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogRun {
                distance_km: 10.0,
                duration_min: 50.0,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.runs.len(), 1);
        assert_eq!(vm.runs[0].pace, "5:00/km");
        assert!(!vm.runs[0].spike_flag);
        assert_eq!(vm.runs[0].zone, "Z1");
    }

    #[test]
    fn run_zone_strings_match_the_shell_colour_contract() {
        // The view emits the zone as a bare `"Z1"/"Z2"/"Z3"` string (or `"-"`
        // with no HR), and the Android shell's `hrZoneColor` matches on exactly
        // those literals. Renaming the `ThreeZone` variants would compile clean
        // but silently drop every zone colour on-device: this locks the wire
        // strings so such a rename fails here instead.
        let app = Engine;
        let cases = [(70.0, "Z1"), (85.0, "Z2"), (92.0, "Z3"), (0.0, "-")];
        for (hr, expected) in cases {
            let mut model = Model::default();
            app.update(
                Event::LogRun {
                    distance_km: 10.0,
                    duration_min: 50.0,
                    hr_pct_max: hr,
                    longest_recent_km: 12.0,
                    observed_at: 0,
                },
                &mut model,
            )
            .expect_only_render();
            assert_eq!(app.view(&model).runs[0].zone, expected, "hr {hr}%");
        }
    }

    #[test]
    fn readiness_signal_wire_names_match_the_shell_contract() {
        // Each `ReadinessSignal` serialises to its bare variant name, and the
        // Android shell hand-builds that exact string as the event's `signal`
        // field (Core.kt `ReadinessSignal.name`). Renaming a variant here would
        // compile clean but silently drop every readiness submission for it -
        // including the medical-referral flags at the top of the safety ladder.
        // This locks the wire names so such a rename fails here instead.
        let cases = [
            (ReadinessSignal::Rpe, "Rpe"),
            (ReadinessSignal::EstimatedOneRm, "EstimatedOneRm"),
            (ReadinessSignal::BarVelocity, "BarVelocity"),
            (ReadinessSignal::VelocityLoss, "VelocityLoss"),
            (ReadinessSignal::WellnessZ, "WellnessZ"),
            (ReadinessSignal::HrvLnRmssd, "HrvLnRmssd"),
            (ReadinessSignal::HrvCv, "HrvCv"),
            (ReadinessSignal::AerobicDecoupling, "AerobicDecoupling"),
            (ReadinessSignal::RestingHr, "RestingHr"),
            (ReadinessSignal::Pain, "Pain"),
            (ReadinessSignal::Illness, "Illness"),
            (ReadinessSignal::RedS, "RedS"),
            (ReadinessSignal::CardiacRedFlag, "CardiacRedFlag"),
            (ReadinessSignal::BoneStress, "BoneStress"),
        ];
        for (signal, wire) in cases {
            assert_eq!(
                serde_json::to_string(&signal).unwrap(),
                format!("\"{wire}\""),
                "wire name for {signal:?}"
            );
        }
    }

    #[test]
    fn zero_duration_run_reports_unknown_pace_not_zero() {
        // A run with distance but zero elapsed time (e.g. GPS fixes sharing a
        // timestamp) must not derive a nonsense "0:00/km" pace.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogRun {
                distance_km: 5.0,
                duration_min: 0.0,
                hr_pct_max: 70.0,
                longest_recent_km: 6.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        assert_eq!(app.view(&model).runs[0].pace, "-");
    }

    #[test]
    fn spike_summary_explains_a_missing_baseline_vs_a_real_jump() {
        // The spike gate errs safe with no history, so the first run trips it. Its
        // summary must not claim a ">10%" jump over a baseline that does not exist;
        // a later run that genuinely exceeds the prior longest gets the ">10%"
        // wording. Both still set spike_flag, only the wording differs.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogRun {
                distance_km: 5.0,
                duration_min: 25.0,
                hr_pct_max: 70.0,
                longest_recent_km: 0.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.runs[0].spike_flag);
        assert!(
            vm.runs[0].summary.contains("no prior run"),
            "first run should explain the missing baseline, got: {}",
            vm.runs[0].summary
        );
        // Structured fields a shell renders instead of re-parsing `summary`.
        assert_eq!(vm.runs[0].distance_km, 5.0);
        assert!(
            vm.runs[0].spike_note.contains("no prior run"),
            "spike_note should carry the honest reason, got: {}",
            vm.runs[0].spike_note
        );

        app.update(
            Event::LogRun {
                distance_km: 20.0,
                duration_min: 100.0,
                hr_pct_max: 70.0,
                longest_recent_km: 0.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.runs[1].spike_flag);
        assert!(
            vm.runs[1].summary.contains(">10% over recent longest"),
            "a real jump over the prior longest should say so, got: {}",
            vm.runs[1].summary
        );
    }

    #[test]
    fn logged_at_timestamp_round_trips_to_the_history_views() {
        // A shell dates each history card from the view's `observed_at`; the core
        // must carry the shell-supplied log time verbatim (it holds no clock) from
        // the event through storage to both the lift and run views. A logged event
        // that omits the field (pre-timestamp persisted log) decodes as 0.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogSet {
                exercise: "Back squat".into(),
                weight_kg: 100.0,
                reps: 5,
                rpe: 8.0,
                observed_at: 1_700_000_000,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::LogRun {
                distance_km: 5.0,
                duration_min: 25.0,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
                observed_at: 1_700_000_500,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.lifts[0].observed_at, 1_700_000_000);
        assert_eq!(vm.runs[0].observed_at, 1_700_000_500);

        // Absent from the wire (old persisted event) → 0, not a decode failure.
        let undated: Event = serde_json::from_str(
            r#"{"LogSet":{"exercise":"Bench","weight_kg":60.0,"reps":8,"rpe":7.0}}"#,
        )
        .expect("pre-timestamp LogSet still decodes");
        app.update(undated, &mut model).expect_only_render();
        let vm = app.view(&model);
        assert_eq!(vm.lifts[1].observed_at, 0);
    }

    #[test]
    fn pace_truncates_fractional_seconds_and_zero_pads() {
        // Pace is formatted `m:ss/km` by flooring seconds (a `u32` cast) then
        // zero-padding. 40.95 min / 10 km = 245.7 s/km, which must render as
        // "4:05/km": 245.7 floored to 245 (not rounded up to 4:06) and the 5 s
        // padded to "05". Flooring also structurally prevents a "4:60" overflow,
        // since `245 % 60` can never reach 60. Locks both against a formatting
        // regression.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogRun {
                distance_km: 10.0,
                duration_min: 40.95,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        assert_eq!(app.view(&model).runs[0].pace, "4:05/km");
    }

    #[test]
    fn manual_run_spike_baseline_is_derived_from_prior_runs() {
        // Same guarantee as the GPS path: a manual run's spike gate floors its
        // baseline to the longest run already held, even when the caller sends 0.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogRun {
                distance_km: 20.0,
                duration_min: 100.0,
                hr_pct_max: 70.0,
                longest_recent_km: 0.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        // 10 km is well under the derived 20 km baseline → no spike.
        app.update(
            Event::LogRun {
                distance_km: 10.0,
                duration_min: 50.0,
                hr_pct_max: 70.0,
                longest_recent_km: 0.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        assert!(!app.view(&model).runs[1].spike_flag);
    }

    #[test]
    fn manual_run_spike_baseline_reads_a_prior_gps_runs_derived_distance() {
        // Cross-type baseline: a GPS run stores distance_km = 0.0 (it derives from
        // the track), so a later manual run's spike gate only sees a real baseline
        // if `run_distance_km` reconstructs the GPS run's distance from its fixes.
        // If that derivation regressed to the stored 0.0, the baseline would be 0,
        // the safe-with-no-history gate would fire, and this 1 km run would flag -
        // so asserting NO spike pins the GPS branch of the fold.
        let app = Engine;
        let mut model = Model::default();

        // GPS run: ~1.11 km along the equator (0 → 0.01° longitude).
        app.update(
            Event::LogRunTrack {
                points: vec![
                    GpsPoint {
                        lat: 0.0,
                        lon: 0.0,
                        observed_at: 0,
                        accuracy_m: 5.0,
                    },
                    GpsPoint {
                        lat: 0.0,
                        lon: 0.01,
                        observed_at: 400,
                        accuracy_m: 5.0,
                    },
                ],
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        // Manual 1 km run: under the ~1.11 km baseline derived from the GPS track,
        // so no spike, but only because the baseline came from the GPS run.
        app.update(
            Event::LogRun {
                distance_km: 1.0,
                duration_min: 6.0,
                hr_pct_max: 70.0,
                longest_recent_km: 0.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        assert!(!app.view(&model).runs[1].spike_flag);
    }

    #[test]
    fn run_distance_spike_is_flagged() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogRun {
                distance_km: 20.0,
                duration_min: 100.0,
                hr_pct_max: 75.0,
                longest_recent_km: 12.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        assert!(app.view(&model).runs[0].spike_flag);
    }

    #[test]
    fn gps_tracked_run_derives_distance_and_pace_from_fixes() {
        let app = Engine;
        let mut model = Model::default();

        // ~1.11 km along the equator (0 → 0.01° lon) over 400 s.
        let points = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.0,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.005,
                observed_at: 200,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.01,
                observed_at: 400,
                accuracy_m: 5.0,
            },
        ];
        app.update(
            Event::LogRunTrack {
                points,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.runs.len(), 1);
        assert!(
            vm.runs[0].summary.starts_with("GPS "),
            "got {}",
            vm.runs[0].summary
        );
        assert!(
            vm.runs[0].summary.contains("1.1km"),
            "got {}",
            vm.runs[0].summary
        );
        assert_ne!(vm.runs[0].pace, "-");
        assert!(!vm.runs[0].spike_flag);
        // GPS-tracked run carries a GPX document ready for export.
        assert!(
            vm.runs[0].gpx.contains("<trkpt "),
            "gpx: {}",
            vm.runs[0].gpx
        );
    }

    #[test]
    fn gps_run_of_only_noise_fixes_offers_no_gpx_export() {
        let app = Engine;
        let mut model = Model::default();

        // Every fix is worse than the accuracy gate, so none survive filtering:
        // fewer than two usable points is not a route, and offering an "Export
        // GPX" button (gpx non-empty) for a 0 km run would be misleading.
        let points = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.0,
                observed_at: 0,
                accuracy_m: 80.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.01,
                observed_at: 400,
                accuracy_m: 120.0,
            },
        ];
        app.update(
            Event::LogRunTrack {
                points,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.runs.len(), 1);
        assert_eq!(vm.runs[0].pace, "-");
        assert!(
            vm.runs[0].gpx.is_empty(),
            "gpx should be empty for a noise-only track, got {}",
            vm.runs[0].gpx
        );
        // An unmeasurable track must say so, not render a "0.0km" entry, and must
        // not trip the distance-spike gate against a phantom zero baseline.
        assert!(
            vm.runs[0].summary.contains("too poor"),
            "summary should flag the poor GPS signal, got {}",
            vm.runs[0].summary
        );
        assert!(!vm.runs[0].spike_flag);
    }

    #[test]
    fn gps_run_without_hr_reports_unknown_zone_not_fabricated_z1() {
        let app = Engine;
        let mut model = Model::default();
        let points = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.0,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.01,
                observed_at: 400,
                accuracy_m: 5.0,
            },
        ];
        // hr_pct_max = 0.0 → no HR sample → zone must not be fabricated.
        app.update(
            Event::LogRunTrack {
                points,
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.runs[0].zone, "-");
        assert!(
            vm.runs[0].summary.contains("(-)"),
            "got {}",
            vm.runs[0].summary
        );
    }

    #[test]
    fn gps_spike_baseline_is_derived_from_prior_runs() {
        let app = Engine;
        let mut model = Model::default();
        let one_km = || {
            vec![
                GpsPoint {
                    lat: 0.0,
                    lon: 0.0,
                    observed_at: 0,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.01,
                    observed_at: 400,
                    accuracy_m: 5.0,
                },
            ]
        };

        // Run 1: ~1.11 km with no history. The core spike gate errs safe with no
        // baseline, so this first run is flagged, expected and intentional.
        app.update(
            Event::LogRunTrack {
                points: one_km(),
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(app.view(&model).runs[0].spike_flag);

        // Run 2: same ~1.11 km. Shell again sends baseline 0.0, but the core now
        // derives the baseline from run 1 (~1.11 km): 1.11 is not >10 % over
        // itself, so NO spike. This only passes if the baseline came from history.
        app.update(
            Event::LogRunTrack {
                points: one_km(),
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(!app.view(&model).runs[1].spike_flag);

        // Run 3: ~2.22 km, nearly double the derived baseline → spike.
        let long = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.0,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.02,
                observed_at: 800,
                accuracy_m: 5.0,
            },
        ];
        app.update(
            Event::LogRunTrack {
                points: long,
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(app.view(&model).runs[2].spike_flag);
    }

    fn sample_profile() -> Profile {
        Profile {
            progression_cadence: ProgressionCadence::WeekToWeek,
            lift_goal: LiftGoal::MaxStrength,
            goal_distance: GoalDistance::Marathon,
            concurrent_goal: ConcurrentGoal::EndurancePriority,
            weekly_sets: 14,
            running_days_per_week: 5,
            running_km_per_week: 50.0,
            advanced: true,
            endurance_intensity_pct_vo2max: 75.0,
            female: false,
            high_load_block: false,
            health: HealthScreen::default(),
            environment: None,
            env_temp_c: None,
            env_altitude_m: None,
            weeks_off: None,
            bodyweight_kg: None,
        }
    }

    #[test]
    fn profile_drives_evidence_cited_guidance() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).guidance.is_empty());

        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();

        let vm = app.view(&model);
        assert!(!vm.guidance.is_empty());
        // Every surfaced row carries a grade + citation; none is MarketingMyth.
        assert!(
            vm.guidance
                .iter()
                .all(|g| !g.grade.is_empty() && !g.citation.is_empty())
        );
        assert!(vm.guidance.iter().all(|g| g.grade != "MarketingMyth"));
        // The running-days=5, km=50 profile trips the hybrid lower-lift cap.
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("Lower-lift cap"))
        );
        // 50 km/wk is under the ~64 km bone-stress threshold, so no BSI row yet.
        assert!(
            !vm.guidance
                .iter()
                .any(|g| g.summary.contains("Bone-stress-injury"))
        );
        // MaxStrength surfaces the 20% velocity-loss set cutoff (AUTOREG-VL-001).
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("Velocity-loss set cutoff"))
        );
        // 14 weekly sets/muscle (>12) trips the session-split guidance (HYP-FREQ-001).
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("split across ≥2 sessions"))
        );
    }

    #[test]
    fn predict_race_produces_graded_two_method_estimate() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).race_prediction.is_none());

        // 5 km in 20:00, predict a 10 km at ~40 km/wk.
        app.update(
            Event::PredictRace {
                recent_distance_m: 5000.0,
                recent_time_sec: 1200.0,
                goal_distance_m: 10_000.0,
                weekly_km: 40.0,
                weeks_since_race: None,
            },
            &mut model,
        )
        .expect_only_render();

        let pred = app
            .view(&model)
            .race_prediction
            .expect("a prediction must be present after PredictRace");
        assert_eq!(pred.goal_label, "10K");
        // Carries a real grade + citation (evidence discipline).
        assert!(!pred.grade.is_empty() && !pred.citation.is_empty());
        assert_ne!(pred.grade, "MarketingMyth");
        // A 20:00 5K projects a 10K well over 40:00 (both methods, endurance fade).
        assert!(
            pred.low_sec > 2400.0,
            "10K should exceed 40:00, got {}",
            pred.low_sec
        );
        assert!(pred.high_sec >= pred.low_sec);
        assert!(
            pred.predicted.contains(':'),
            "clock-formatted, got {}",
            pred.predicted
        );

        // Clearing drops the section.
        app.update(Event::ClearRacePrediction, &mut model)
            .expect_only_render();
        assert!(app.view(&model).race_prediction.is_none());
    }

    #[test]
    fn predict_race_with_degenerate_input_surfaces_no_false_time() {
        // A zero recent time is undefined for both Riegel and VDOT; the view must
        // say so rather than fabricate a finish clock (evidence discipline).
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::PredictRace {
                recent_distance_m: 5000.0,
                recent_time_sec: 0.0,
                goal_distance_m: 10_000.0,
                weekly_km: 40.0,
                weeks_since_race: None,
            },
            &mut model,
        )
        .expect_only_render();

        let pred = app
            .view(&model)
            .race_prediction
            .expect("the section is present, just with no numeric estimate");
        assert_eq!(pred.predicted, "-");
        assert_eq!(pred.low_sec, 0.0);
        assert!(!pred.agreed);
        assert!(
            pred.summary.contains("need a valid recent race"),
            "degenerate summary should explain the missing input, got {}",
            pred.summary
        );
        assert!(
            !pred.predicted.contains(':'),
            "must not render a clock time for undefined input"
        );
    }

    #[test]
    fn plan_hypertrophy_meso_produces_graded_rows() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).hypertrophy_plan.is_empty());

        // A known muscle + 4 accumulation weeks yields several graded rows.
        app.update(
            Event::PlanHypertrophyMeso {
                muscle: "chest".into(),
                weeks: 4,
                not_growing: false,
                recovering_easily: false,
            },
            &mut model,
        )
        .expect_only_render();

        let plan = app.view(&model).hypertrophy_plan;
        assert!(plan.len() >= 3, "expected several rows, got {}", plan.len());
        // Every row is graded and never a hard-blocked myth (evidence discipline).
        for row in &plan {
            assert!(!row.grade.is_empty() && !row.citation.is_empty());
            assert_ne!(row.grade, "MarketingMyth");
        }
        // The volume-ramp row is present (chest MEV 10 → MRV 22).
        assert!(
            plan.iter().any(|r| r.summary.contains("Weekly set ramp:")),
            "ramp row missing"
        );

        // Clearing empties the section.
        app.update(Event::ClearHypertrophyPlan, &mut model)
            .expect_only_render();
        assert!(app.view(&model).hypertrophy_plan.is_empty());

        // An unknown muscle yields a single explanatory row (no fake data).
        app.update(
            Event::PlanHypertrophyMeso {
                muscle: "tail".into(),
                weeks: 4,
                not_growing: false,
                recovering_easily: false,
            },
            &mut model,
        )
        .expect_only_render();
        let unknown = app.view(&model).hypertrophy_plan;
        assert_eq!(unknown.len(), 1, "unknown muscle should be one row");
        assert!(unknown[0].summary.contains("not a known muscle"));

        // weeks == 0 is degenerate: show the landmarks context only, not an
        // empty ramp/RIR schedule.
        app.update(
            Event::PlanHypertrophyMeso {
                muscle: "chest".into(),
                weeks: 0,
                not_growing: false,
                recovering_easily: false,
            },
            &mut model,
        )
        .expect_only_render();
        let zero = app.view(&model).hypertrophy_plan;
        assert_eq!(zero.len(), 1, "zero-week plan is landmarks-only");
        assert!(zero[0].summary.contains("Landmarks"));
        assert!(
            !zero.iter().any(|r| r.summary.contains("ramp")),
            "no ramp row without a week count"
        );
    }

    #[test]
    fn compute_protein_scales_graded_targets_by_bodyweight() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).protein_targets.is_empty());

        // Deficit context at 80 kg: 1.8–2.7 g/kg → 144–216 g/day, still graded.
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 80.0,
                masters: false,
                deficit: true,
            },
            &mut model,
        )
        .expect_only_render();

        let targets = app.view(&model).protein_targets;
        assert_eq!(targets.len(), 1, "one selected context → one row");
        let row = &targets[0];
        assert!(
            row.summary.contains("g/day"),
            "must state an absolute target"
        );
        assert!(
            !row.grade.is_empty() && row.grade != "MarketingMyth",
            "row must carry real evidence"
        );
        // The absolute bounds scale from bodyweight × each graded g/kg bound.
        assert!(
            row.summary.contains("144") && row.summary.contains("216"),
            "80 kg × (1.8, 2.7) should surface 144–216 g/day, got {}",
            row.summary
        );

        // Neither context selected → no fabricated general target (HARD RULE 1).
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 80.0,
                masters: false,
                deficit: false,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(app.view(&model).protein_targets.is_empty());

        // Clearing empties the section.
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 80.0,
                masters: true,
                deficit: false,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(!app.view(&model).protein_targets.is_empty());
        app.update(Event::ClearProtein, &mut model)
            .expect_only_render();
        assert!(app.view(&model).protein_targets.is_empty());
    }

    #[test]
    fn compute_protein_handles_both_flags_and_rejects_bad_weight() {
        let app = Engine;
        let mut model = Model::default();

        // Both contexts selected → one graded row each (masters + deficit).
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 70.0,
                masters: true,
                deficit: true,
            },
            &mut model,
        )
        .expect_only_render();
        let targets = app.view(&model).protein_targets;
        assert_eq!(targets.len(), 2, "both contexts → two rows");
        assert!(targets.iter().all(|r| r.summary.contains("g/day")));
        assert!(
            targets.iter().all(|r| !r.grade.is_empty()),
            "every context row stays evidence-graded"
        );

        // A non-positive bodyweight cannot yield a g/day figure: emit nothing
        // rather than a zero or nonsensical target.
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 0.0,
                masters: true,
                deficit: true,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(
            app.view(&model).protein_targets.is_empty(),
            "non-positive bodyweight must surface no target"
        );
    }

    #[test]
    fn compute_hr_zones_builds_graded_hrmax_and_five_bands() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).hr_zones.is_empty());

        app.update(Event::ComputeHrZones {
                age_years: 30.0,
                resting_hr_bpm: None,
                weeks_since_recalc: None,
                weeks_since_pace_test: None,
            }, &mut model)
            .expect_only_render();

        let zones = app.view(&model).hr_zones;
        // HRmax header + 5 Daniels bands + the MAF base-phase option row.
        assert_eq!(zones.len(), 7, "one HRmax row + five band rows + MAF cap");
        assert!(
            zones.last().unwrap().summary.contains("MAF aerobic cap"),
            "MAF row closes the table: {}",
            zones.last().unwrap().summary
        );

        // Tanaka at 30: 208 − 0.7·30 = 187 bpm.
        let hrmax_row = &zones[0];
        assert!(
            hrmax_row.summary.contains("187 bpm"),
            "{}",
            hrmax_row.summary
        );

        // Easy band is 65–79 %HRmax → 122–148 bpm at HRmax 187.
        let easy = zones
            .iter()
            .find(|z| z.summary.starts_with("Easy"))
            .expect("Easy band row present");
        assert!(easy.summary.contains("122–148 bpm"), "{}", easy.summary);

        // Every row is evidence-graded, never a myth (HARD RULE 2).
        for z in &zones {
            assert!(!z.grade.is_empty() && !z.citation.is_empty());
            assert_ne!(z.grade, "MarketingMyth");
        }

        // Clear empties the section.
        app.update(Event::ClearHrZones, &mut model)
            .expect_only_render();
        assert!(app.view(&model).hr_zones.is_empty());
    }

    #[test]
    fn compute_hr_zones_rejects_implausible_age() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::ComputeHrZones {
                age_years: 0.0,
                resting_hr_bpm: None,
                weeks_since_recalc: None,
                weeks_since_pace_test: None,
            }, &mut model)
            .expect_only_render();
        let zones = app.view(&model).hr_zones;
        assert_eq!(zones.len(), 1, "bad age yields a single explanatory row");
        assert!(zones[0].summary.contains("between 5 and 100"));
    }

    #[test]
    fn high_mileage_profile_surfaces_safety_critical_bsi_surveillance() {
        let app = Engine;
        let mut model = Model::default();
        let mut p = sample_profile();
        p.running_km_per_week = 70.0; // over the ~64 km bone-stress threshold
        app.update(Event::SetProfile(p), &mut model)
            .expect_only_render();

        let bsi = app
            .view(&model)
            .guidance
            .into_iter()
            .find(|g| g.summary.contains("Bone-stress-injury"))
            .expect("70 km/wk must raise BSI surveillance");
        // File 10 hybrid-023 is safety-critical and evidence-cited (HARD RULE 2/3).
        assert!(bsi.safety_critical);
        assert_eq!(bsi.section, "Safety");
        assert!(!bsi.grade.is_empty() && !bsi.citation.is_empty());
    }

    #[test]
    fn novice_load_jump_shown_only_for_novices() {
        let has_jump = |cadence: ProgressionCadence| -> bool {
            let mut p = sample_profile();
            p.progression_cadence = cadence;
            build_guidance(&p)
                .iter()
                .any(|g| g.summary.contains("Novice load jump"))
        };
        assert!(has_jump(ProgressionCadence::EverySession)); // Novice
        assert!(!has_jump(ProgressionCadence::WeekToWeek)); // Intermediate
        assert!(!has_jump(ProgressionCadence::MonthToMonth)); // Advanced
    }

    #[test]
    fn weekly_pct_increment_shown_only_past_novice() {
        // The complement of the novice fixed-kg jump: intermediate/advanced
        // lifters get the per-week percentage progression instead, so this gap
        // (previously no increment guidance at all for them) is now filled.
        let row = |cadence: ProgressionCadence| -> Option<GuidanceView> {
            let mut p = sample_profile();
            p.progression_cadence = cadence;
            build_guidance(&p)
                .into_iter()
                .find(|g| g.summary.contains("Weekly load increment"))
        };
        assert!(row(ProgressionCadence::EverySession).is_none()); // Novice
        for cadence in [
            ProgressionCadence::WeekToWeek,
            ProgressionCadence::MonthToMonth,
        ] {
            let r = row(cadence).expect("non-novice should get weekly % increment");
            assert!(
                !r.citation.is_empty(),
                "increment row must be evidence-cited"
            );
            assert_ne!(r.grade, "MarketingMyth");
            // Half-percent bounds must render faithfully, not round to a whole
            // number that understates the cited band (upper 1-2.5%, lower 2.5-5%).
            assert!(
                r.summary.contains("+1-2.5% upper"),
                "upper band must keep its half-percent: {}",
                r.summary
            );
            assert!(
                r.summary.contains("+2.5-5% lower"),
                "lower band must keep its half-percent: {}",
                r.summary
            );
        }
    }

    #[test]
    fn hypertrophy_rep_load_prescription_is_surfaced_and_cited() {
        // The rep/load row answers "how many reps and what load": absent before
        // this wire. Profile-independent (applies to all lifting), so any profile
        // shows it, evidence-cited and non-myth, with the heavy-compound band.
        let r = build_guidance(&sample_profile())
            .into_iter()
            .find(|g| g.summary.starts_with("Rep/load:"))
            .expect("rep/load prescription row must be present");
        assert!(
            !r.citation.is_empty(),
            "rep/load row must be evidence-cited"
        );
        assert_ne!(r.grade, "MarketingMyth");
        assert!(
            r.summary.contains("heavy compound 5-10 @75-85%1RM"),
            "heavy band must render: {}",
            r.summary
        );
    }

    #[test]
    fn build_guidance_never_panics_across_the_profile_domain() {
        // Exhaustive smoke test: build_guidance branches heavily on the profile
        // and does arithmetic on its integer fields (weekly_sets, running days,
        // km). Sweep every enum combination against the boundary integers the
        // Android profile editor can emit (weekly_sets 0..=30, days 0..=7, km
        // 0..=150) and assert (a) no panic (underflow / div-by-zero) and (b) the
        // HARD RULE 2 invariant holds everywhere: every surfaced row is
        // evidence-cited and none is graded MarketingMyth.
        let cadences = [
            ProgressionCadence::EverySession,
            ProgressionCadence::WeekToWeek,
            ProgressionCadence::MonthToMonth,
        ];
        let lift_goals = [
            LiftGoal::MaxStrength,
            LiftGoal::Power,
            LiftGoal::Hypertrophy,
        ];
        let distances = [
            GoalDistance::General,
            GoalDistance::C25k,
            GoalDistance::FiveK,
            GoalDistance::TenK,
            GoalDistance::HalfMarathon,
            GoalDistance::Marathon,
        ];
        let concurrent = [
            ConcurrentGoal::Strength,
            ConcurrentGoal::Power,
            ConcurrentGoal::Hypertrophy,
            ConcurrentGoal::EndurancePriority,
        ];
        for pc in cadences {
            for lg in lift_goals {
                for gd in distances {
                    for cg in concurrent {
                        for weekly_sets in [0u8, 1, 30] {
                            for running_days_per_week in [0u8, 1, 7] {
                                for running_km_per_week in [0.0, 0.1, 150.0] {
                                    for advanced in [false, true] {
                                        let p = Profile {
                                            progression_cadence: pc,
                                            lift_goal: lg,
                                            goal_distance: gd,
                                            concurrent_goal: cg,
                                            weekly_sets,
                                            running_days_per_week,
                                            running_km_per_week,
                                            advanced,
                                            endurance_intensity_pct_vo2max: 75.0,
                                            female: false,
                                            high_load_block: false,
                                            health: HealthScreen::default(),
                                            environment: None,
                                            env_temp_c: None,
                                            env_altitude_m: None,
                                            weeks_off: None,
                                            bodyweight_kg: None,
                                        };
                                        let rows = build_guidance(&p);
                                        for row in &rows {
                                            assert!(
                                                !row.citation.is_empty(),
                                                "uncited row for {p:?}: {}",
                                                row.summary
                                            );
                                            assert_ne!(
                                                row.grade, "MarketingMyth",
                                                "myth surfaced for {p:?}: {}",
                                                row.summary
                                            );
                                        }
                                        // Sections must be contiguous: the shell
                                        // suppresses a repeated section header
                                        // only when the previous row shares the
                                        // section, so a section that reappears
                                        // after a gap would render a duplicate
                                        // header. Once a section run closes it
                                        // must never return.
                                        let mut closed: Vec<&str> = Vec::new();
                                        let mut prev: Option<&str> = None;
                                        for row in &rows {
                                            let s = row.section.as_str();
                                            if prev != Some(s) {
                                                assert!(
                                                    !closed.contains(&s),
                                                    "section {s:?} is non-contiguous for {p:?}"
                                                );
                                                if let Some(p) = prev {
                                                    closed.push(p);
                                                }
                                                prev = Some(s);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn build_reference_sections_are_contiguous_and_cited() {
        // build_reference is profile-independent, so one call covers its domain.
        // Same contract as the guidance list: every row evidence-cited, no
        // MarketingMyth, and each section forms a single contiguous run (a
        // section that reappears after a gap would read as an orphan card, since
        // the reference list labels every card by section).
        let rows = build_reference();
        for row in &rows {
            assert!(
                !row.citation.is_empty(),
                "uncited reference row: {}",
                row.summary
            );
            assert_ne!(
                row.grade, "MarketingMyth",
                "myth in reference: {}",
                row.summary
            );
        }
        let mut closed: Vec<&str> = Vec::new();
        let mut prev: Option<&str> = None;
        for row in &rows {
            let s = row.section.as_str();
            if prev != Some(s) {
                assert!(
                    !closed.contains(&s),
                    "reference section {s:?} is non-contiguous"
                );
                if let Some(p) = prev {
                    closed.push(p);
                }
                prev = Some(s);
            }
        }

        // Each newly-surfaced dormant calc must appear as its own card with a
        // non-empty citation. Match on a load-bearing substring of each summary.
        let card = |needle: &str| rows.iter().find(|r| r.summary.contains(needle));
        for needle in [
            "Olympic pulling derivatives",
            "RPE-anchored: top set",
            "Two-for-two rule",
            "Stall: if e1RM",
            "Double progression",
            "Masters (65+) protein",
            "Deficit protein",
            "Masters (65+) per-meal protein",
            "Couch-to-5K",
            "Pyramidal split",
            "Polarized split",
            "Maintaining a quality",
            "Joint pain at heavy load",
            "Scale DOWN",
            "Scale UP",
            "Home / minimal equipment",
            "Goals are framed as controllable",
        ] {
            let row =
                card(needle).unwrap_or_else(|| panic!("missing reference card for {needle:?}"));
            assert!(
                !row.citation.is_empty(),
                "uncited reference card: {}",
                row.summary
            );
        }
    }

    #[test]
    fn coach_query_builders_never_panic_on_degenerate_inputs() {
        // The four Coach tool builders take free-form numeric/string queries from
        // the shell (a text field the user can leave empty, zero, or negative).
        // Each guards its own degenerate domain; sweep the boundaries and assert
        // (a) no panic (div-by-zero / NaN-format / underflow) and (b) HARD RULE 2:
        // every surfaced row is evidence-cited and none is graded MarketingMyth.
        let cited_non_myth = |rows: &[GuidanceView]| {
            for row in rows {
                assert!(
                    !row.citation.is_empty(),
                    "uncited coach row: {}",
                    row.summary
                );
                assert_ne!(row.grade, "MarketingMyth", "myth in coach: {}", row.summary);
            }
        };

        // Race predictor: zero/negative distances and times must degrade to the
        // "need a valid recent race" summary, never a NaN clock or panic.
        for &d in &[0.0, -1.0, 5000.0] {
            for &t in &[0.0, -1.0, 1200.0] {
                for &g in &[0.0, -1.0, 10_000.0] {
                    for &wk in &[0.0, -5.0, 60.0] {
                        let v = to_race_view(
                            &RaceQuery {
                                recent_distance_m: d,
                                recent_time_sec: t,
                                goal_distance_m: g,
                                weekly_km: wk,
                                weeks_since_race: None,
                            },
                            None,
                        );
                        assert!(
                            !v.predicted.contains("NaN"),
                            "NaN clock for d={d} t={t} g={g}"
                        );
                        assert!(!v.citation.is_empty(), "uncited race view");
                    }
                }
            }
        }

        // Hypertrophy plan: unknown muscle + zero/huge week counts.
        for muscle in ["", "chest", "not-a-muscle"] {
            for &weeks in &[0u8, 1, 12, 255] {
                cited_non_myth(&build_hypertrophy_plan(
                    &HypertrophyPlanQuery {
                        muscle: muscle.to_string(),
                        weeks,
                        not_growing: false,
                        recovering_easily: false,
                    },
                    None,
                ));
            }
        }

        // Protein: non-positive bodyweight yields no rows; positive scales
        // cleanly, with and without the RED-S refusal path (safety-022).
        for &bw in &[0.0, -70.0, 70.0, 1e6] {
            for &m in &[false, true] {
                for &d in &[false, true] {
                    for &reds in &[false, true] {
                        cited_non_myth(&build_protein_targets(
                            &ProteinQuery {
                                bodyweight_kg: bw,
                                masters: m,
                                deficit: d,
                            },
                            reds,
                        ));
                    }
                }
            }
        }

        // HR zones: ages outside 5..=100 yield the explanatory row, never a bogus
        // HRmax; in-range ages produce cited bands.
        for &age in &[-1.0, 0.0, 4.9, 5.0, 30.0, 100.0, 100.1, 200.0] {
            cited_non_myth(&build_hr_zones(&HrZoneQuery {
                age_years: age,
                resting_hr_bpm: None,
                weeks_since_recalc: None,
                weeks_since_pace_test: None,
            }));
        }
    }

    #[test]
    fn frequency_guidance_collapses_equal_bounds() {
        // A muscle whose peak set count lands in a fixed-frequency band (chest MRV
        // 22 → 3–3×/wk) must not print a degenerate "3–3×/wk" span; the per-session
        // spread (6–9) stays a real range.
        let plan = build_hypertrophy_plan(
            &HypertrophyPlanQuery {
                muscle: "chest".to_string(),
                weeks: 4,
                not_growing: false,
                recovering_easily: false,
            },
            None,
        );
        let peak = plan
            .iter()
            .find(|g| g.summary.starts_with("Peak frequency:"))
            .expect("peak frequency row");
        assert!(
            peak.summary.contains("3×/wk"),
            "expected collapsed frequency, got: {}",
            peak.summary
        );
        assert!(
            !peak.summary.contains("3–3"),
            "degenerate span not collapsed: {}",
            peak.summary
        );
        assert_eq!(fmt_u8_range(3, 3), "3");
        assert_eq!(fmt_u8_range(6, 9), "6–9");
    }

    #[test]
    fn quality_caps_shown_only_when_the_athlete_runs() {
        let has_caps = |run_days: u8| -> bool {
            let mut p = sample_profile();
            p.running_days_per_week = run_days;
            build_guidance(&p)
                .iter()
                .any(|g| g.summary.contains("Quality-session caps"))
        };
        assert!(has_caps(5));
        assert!(!has_caps(0));
    }

    #[test]
    fn periodization_phase_plan_tracks_the_model_by_training_age() {
        // Cadence maps to training age → periodization model: EverySession→Novice
        // →Linear; MonthToMonth→Advanced→Block; WeekToWeek→Intermediate→DUP
        // (no phase table in the source).
        let summaries = |cadence: ProgressionCadence| -> Vec<String> {
            let mut p = sample_profile();
            p.progression_cadence = cadence;
            build_guidance(&p).into_iter().map(|g| g.summary).collect()
        };

        let linear = summaries(ProgressionCadence::EverySession);
        assert!(linear.iter().any(|s| s.contains("Linear Base: 67-75%1RM")));
        assert!(
            linear
                .iter()
                .any(|s| s.contains("Linear Taper: maintain intensity"))
        );
        assert!(!linear.iter().any(|s| s.starts_with("Block ")));

        let block = summaries(ProgressionCadence::MonthToMonth);
        assert!(
            block
                .iter()
                .any(|s| s.contains("Block Accumulation: 65-80%1RM"))
        );
        assert!(
            block
                .iter()
                .any(|s| s.contains("Block Realization: 1-3 reps"))
        );
        assert!(!block.iter().any(|s| s.starts_with("Linear ")));

        let dup = summaries(ProgressionCadence::WeekToWeek);
        assert!(
            !dup.iter()
                .any(|s| s.starts_with("Linear ") || s.starts_with("Block "))
        );
    }

    #[test]
    fn peak_block_run_cap_shows_only_for_maximal_lift_goals() {
        let app = Engine;
        let has_peak_row = |goal: LiftGoal| {
            let mut model = Model::default();
            let mut p = sample_profile();
            p.lift_goal = goal;
            app.update(Event::SetProfile(p), &mut model)
                .expect_only_render();
            app.view(&model)
                .guidance
                .iter()
                .any(|g| g.summary.contains("Peak block"))
        };
        // Maximal-quality goals get the File 10 CAP-2 running override…
        assert!(has_peak_row(LiftGoal::MaxStrength));
        assert!(has_peak_row(LiftGoal::Power));
        // …a hypertrophy goal is not in a peak strength/power block, so no cap.
        assert!(!has_peak_row(LiftGoal::Hypertrophy));
    }

    #[test]
    fn lower_strength_interference_row_tracks_training_age_and_running() {
        let app = Engine;
        let row_text = |cadence: ProgressionCadence, run_days: u8| {
            let mut model = Model::default();
            let mut p = sample_profile();
            p.progression_cadence = cadence;
            p.running_days_per_week = run_days;
            app.update(Event::SetProfile(p), &mut model)
                .expect_only_render();
            app.view(&model)
                .guidance
                .into_iter()
                .find(|g| g.summary.contains("Lower-body strength interference"))
                .map(|g| g.summary)
        };
        // Trained lifter who runs → susceptible.
        assert!(
            row_text(ProgressionCadence::WeekToWeek, 5)
                .expect("trained runner shows the row")
                .contains("yes")
        );
        // Novice who runs → spared.
        assert!(
            row_text(ProgressionCadence::EverySession, 5)
                .expect("novice runner shows the row")
                .contains("no")
        );
        // Pure lifter (no running) → row absent entirely.
        assert!(row_text(ProgressionCadence::WeekToWeek, 0).is_none());
    }

    #[test]
    fn newly_wired_guidance_rows_are_present_and_cited() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        let vm = app.view(&model);

        // Each wired dormant calc must surface a row, and every row still carries
        // an evidence grade + citation (HARD RULE 2, no bypasses).
        for needle in [
            "MEV for",
            "High-volume sensitivity",
            "Deload cadence",
            "Base-phase intensity",
            "Prilepin @~",
            "Weekly volume increase cap",
        ] {
            let row = vm
                .guidance
                .iter()
                .find(|g| g.summary.contains(needle))
                .unwrap_or_else(|| panic!("missing guidance row: {needle}"));
            assert!(!row.grade.is_empty() && !row.citation.is_empty());
            assert_ne!(row.grade, "MarketingMyth");
        }

        // The Prilepin row must resolve to its registered claim (Moderate grade,
        // Prilepin citation), not the unregistered-id fallback.
        let prilepin = vm
            .guidance
            .iter()
            .find(|g| g.summary.contains("Prilepin @~"))
            .unwrap();
        assert_eq!(prilepin.grade, "Moderate");
        assert!(
            prilepin.citation.contains("Prilepin"),
            "{}",
            prilepin.citation
        );
        assert!(prilepin.contested, "STR-PRILEPIN-001 is contested (CQ-03)");

        // WeekToWeek cadence → Intermediate age → not conservative → 3:1 cadence.
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("Deload cadence: 3:1"))
        );
    }

    #[test]
    fn novice_profile_gets_conservative_running_deload() {
        let app = Engine;
        let mut model = Model::default();
        let mut p = sample_profile();
        p.progression_cadence = ProgressionCadence::EverySession; // → Novice
        app.update(Event::SetProfile(p), &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("Deload cadence: 2:1")),
            "novice (low training age) should get the conservative 2:1 cadence"
        );
    }

    #[test]
    fn power_goal_surfaces_cited_plyo_cap() {
        let app = Engine;
        let mut model = Model::default();
        let mut p = sample_profile();
        p.lift_goal = LiftGoal::Power;
        app.update(Event::SetProfile(p), &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        let plyo = vm
            .guidance
            .iter()
            .find(|g| g.summary.contains("Plyo foot-contact cap"))
            .expect("Power goal should surface the plyometric foot-contact cap row");
        assert!(!plyo.citation.is_empty());
    }

    #[test]
    fn non_power_goal_omits_plyo_cap() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        assert!(
            !app.view(&model)
                .guidance
                .iter()
                .any(|g| g.summary.contains("Plyo foot-contact cap")),
            "MaxStrength goal should not surface the plyo cap"
        );
    }

    #[test]
    fn clearing_profile_empties_guidance() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        app.update(Event::ClearProfile, &mut model)
            .expect_only_render();
        assert!(app.view(&model).guidance.is_empty());
    }

    #[test]
    fn profile_is_echoed_back_for_shell_hydration() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).profile.is_none());

        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        assert_eq!(app.view(&model).profile.as_ref(), Some(&sample_profile()));

        app.update(Event::ClearProfile, &mut model)
            .expect_only_render();
        assert!(app.view(&model).profile.is_none());
    }

    #[test]
    fn reference_cards_are_always_present_and_evidence_cited() {
        let app = Engine;
        let model = Model::default();
        let vm = app.view(&model);
        assert!(!vm.reference.is_empty());
        assert!(
            vm.reference
                .iter()
                .all(|g| !g.citation.is_empty() && g.grade != "MarketingMyth")
        );
    }

    #[test]
    fn review_bone_pain_short_circuits_to_injury_concern() {
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            bone_pain_red_flag: true,
            // Execution says "mastery", but safety must suppress it.
            lift: Some(LiftExec {
                reps_met: true,
                rir_actual: 2,
                rir_target: 2,
            }),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "ConcernInjury");
        assert!(fb.suppresses_praise);
        assert!(fb.safety_critical);
        assert!(!fb.message.is_empty());
    }

    #[test]
    fn review_clean_lift_yields_mastery() {
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            lift: Some(LiftExec {
                reps_met: true,
                rir_actual: 2,
                rir_target: 2,
            }),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "PositiveMastery");
        assert!(!fb.suppresses_praise);
    }

    #[test]
    fn review_week_level_deloads_surface_as_adjustments() {
        // Two failed key sessions is a week-level fatigue deload trigger
        // (autoreg-036) carried on the review, not a single-session readiness
        // marker. It must reach the adjustments surface, and, being a volume
        // deload, must not block training.
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            failed_key_sessions: Some(2),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();

        let vm = app.view(&model);
        assert!(
            vm.review_adjustments
                .iter()
                .any(|a| a.summary.starts_with("Deload")),
            "a two-failed-session review should surface a deload adjustment"
        );
        assert!(
            vm.adjustments.is_empty(),
            "a review deload must not leak into the readiness-driven list"
        );
        assert!(!vm.train_blocked, "a volume deload must not block training");
    }

    #[test]
    fn review_sub_threshold_deload_counts_add_nothing() {
        // One failed session / an unset velocity drop are below every trigger,
        // so the adjustments surface stays as bare as a default review's.
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            failed_key_sessions: Some(1),
            rpe_load_gap_sessions: Some(1),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();

        let vm = app.view(&model);
        assert!(
            vm.review_adjustments.is_empty(),
            "sub-threshold counts must not fabricate a deload"
        );
    }

    #[test]
    fn run_only_review_derives_positive_split_feedback_from_last_track() {
        // A GPS run whose back half is far slower than its front half, then a
        // run-only review (no lift, no explicit split figure). The core should
        // fall back to the track-derived split so pacing feedback still fires.
        let app = Engine;
        let mut model = Model::default();
        let points = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.000,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.001,
                observed_at: 20,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.002,
                observed_at: 40,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.003,
                observed_at: 90,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.004,
                observed_at: 140,
                accuracy_m: 5.0,
            },
        ];
        app.update(
            Event::LogRunTrack {
                points,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(Event::SubmitReview(SessionReview::default()), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "IntensityDiscipline");
    }

    #[test]
    fn negative_split_note_reads_as_a_magnitude_not_a_signed_number() {
        // A track whose back half is faster than its front half is a negative
        // split. The run summary must phrase it as a positive magnitude
        // ("N% negative split"), not a signed "-N%" that reads awkwardly next to
        // the word "negative".
        let run = LoggedRun {
            distance_km: 0.0,
            duration_min: 0.0,
            hr_pct_max: 70.0,
            longest_recent_km: 12.0,
            track: vec![
                GpsPoint {
                    lat: 0.0,
                    lon: 0.000,
                    observed_at: 0,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.001,
                    observed_at: 50,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.002,
                    observed_at: 100,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.003,
                    observed_at: 120,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.004,
                    observed_at: 140,
                    accuracy_m: 5.0,
                },
            ],
            observed_at: 0,
        };
        let view = to_run_view(&run);
        assert!(
            view.split_pct.expect("split derived") < 0.0,
            "back-half-faster track should read as a negative split"
        );
        assert!(view.summary.contains("negative split"));
        assert!(
            !view.summary.contains("-"),
            "note should show magnitude, not a signed number: {}",
            view.summary
        );
    }

    #[test]
    fn positive_split_note_reads_as_a_back_half_slowdown() {
        // A track whose back half is slower than its front half is a positive
        // split. The run summary must call it out as a "+N% back-half slowdown"
        // so the note and the coaching cue (positive_split_discipline) agree.
        let run = LoggedRun {
            distance_km: 0.0,
            duration_min: 0.0,
            hr_pct_max: 70.0,
            longest_recent_km: 12.0,
            // Front half (0 → 0.002° over 40 s) is fast; back half
            // (0.002 → 0.004° over 100 s) is slow, a clear back-half slowdown.
            track: vec![
                GpsPoint {
                    lat: 0.0,
                    lon: 0.000,
                    observed_at: 0,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.001,
                    observed_at: 20,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.002,
                    observed_at: 40,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.003,
                    observed_at: 90,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.004,
                    observed_at: 140,
                    accuracy_m: 5.0,
                },
            ],
            observed_at: 0,
        };
        let view = to_run_view(&run);
        assert!(
            view.split_pct.expect("split derived") > feedback::POSITIVE_SPLIT_FLAG_PCT,
            "back-half-slower track should read as a positive split above the flag"
        );
        assert!(
            view.summary.contains("back-half slowdown"),
            "got {}",
            view.summary
        );
        assert!(
            view.summary.contains('+'),
            "positive split note should carry a leading +: {}",
            view.summary
        );

        // Task 8: the same run also carries the core-owned verdict chip, in
        // agreement with the note (one threshold, one place).
        let split = view.split.expect("verdict for a measured split");
        assert_eq!(split.verdict, "fade");
        assert!(split.label.starts_with("FADE +"), "got {}", split.label);
        assert!(split.message.contains("even-to-negative split"));
        assert_eq!(split.grade, "Moderate");
        assert!(split.citation.contains("Hanley"), "got {}", split.citation);
        assert!(!split.safety_critical && !split.contested);
    }

    #[test]
    fn split_verdict_bands_match_the_single_core_threshold() {
        // The verdict must flip exactly where feedback::positive_split_discipline
        // does (±POSITIVE_SPLIT_FLAG_PCT, strict): the shell renders the string
        // without re-deriving any threshold.
        let fade = split_verdict_view(3.1);
        assert_eq!(fade.verdict, "fade");
        assert_eq!(fade.label, "FADE +3%");

        let at_line = split_verdict_view(feedback::POSITIVE_SPLIT_FLAG_PCT);
        assert_eq!(at_line.verdict, "even", "3% exactly is not yet a fade");

        let even = split_verdict_view(0.0);
        assert_eq!(even.verdict, "even");
        assert_eq!(even.label, "EVEN SPLIT");
        assert!(even.message.contains("pacing discipline"));

        let at_neg_line = split_verdict_view(-feedback::POSITIVE_SPLIT_FLAG_PCT);
        assert_eq!(at_neg_line.verdict, "even", "-3% exactly is still even");

        let neg = split_verdict_view(-4.0);
        assert_eq!(neg.verdict, "negative");
        assert_eq!(neg.label, "NEG SPLIT 4%");
        assert!(neg.message.contains("pacing discipline"));

        // Every band carries the same FB-PACING-001 evidence tag.
        for v in [&fade, &even, &neg] {
            assert_eq!(v.grade, "Moderate");
            assert!(v.citation.contains("Hanley"), "got {}", v.citation);
            assert!(v.confidence > 0.0);
        }
    }

    #[test]
    fn manual_run_has_no_split_verdict() {
        // No GPS track → no measurable split → no verdict chip (the core never
        // invents a pacing judgment from a hand-entered distance/duration).
        let view = to_run_view(&LoggedRun {
            distance_km: 8.0,
            duration_min: 45.0,
            hr_pct_max: 70.0,
            longest_recent_km: 10.0,
            track: vec![],
            observed_at: 0,
        });
        assert!(view.split_pct.is_none());
        assert!(view.split.is_none());
    }

    #[test]
    fn run_view_carries_full_spike_gate_evidence() {
        // Task 7: grade/confidence/safety_critical/contested, not just the
        // citation, parity with the other evidence-bearing view structs.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::LogRun {
                distance_km: 12.0,
                duration_min: 60.0,
                hr_pct_max: 70.0,
                longest_recent_km: 10.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        let run = &app.view(&model).runs[0];
        assert!(run.spike_flag, "20% over baseline must flag");
        assert!(!run.grade.is_empty(), "grade populated");
        assert!(!run.citation.is_empty(), "citation populated");
        assert!(run.confidence > 0.0, "confidence populated");
        // RUN-SPIKE-001 is the strongest running injury signal: its
        // safety-critical marker must survive the flattening.
        assert!(run.safety_critical, "spike gate is safety-critical");
    }

    #[test]
    fn e1rm_trend_tracks_previous_set_of_the_same_lift_only() {
        // Task 8: the core computes per-lift e1RM delta + direction so the
        // shell renders without arithmetic. Direction is a factual measurement
        // (what changed), not an improving/declining judgment: that phrasing
        // is the feedback-027/028/029 trend arm.
        let app = Engine;
        let mut model = Model::default();
        let mut log = |exercise: &str, weight_kg: f64, reps: u32| {
            app.update(
                Event::LogSet {
                    exercise: exercise.to_string(),
                    weight_kg,
                    reps,
                    rpe: 8.0,
                    observed_at: 0,
                },
                &mut model,
            )
            .expect_only_render();
        };
        log("Squat", 100.0, 5); // e1RM 116.7
        log("Bench", 80.0, 5); // other lift must not interfere
        log("Squat", 102.5, 5); // e1RM 119.6 → +2.9, up
        log("Squat", 102.5, 5); // unchanged → 0.0, flat
        log("Squat", 95.0, 5); // e1RM 110.8 → -8.8, down

        let lifts = app.view(&model).lifts;
        let squats: Vec<_> = lifts.iter().filter(|l| l.exercise == "Squat").collect();

        // First set of a lift: nothing to compare against.
        assert!(squats[0].e1rm_delta_kg.is_none());
        assert!(squats[0].e1rm_direction.is_none());
        // Bench in between must not break the Squat chain, and Bench's own
        // first set carries no trend either.
        let bench = lifts.iter().find(|l| l.exercise == "Bench").unwrap();
        assert!(bench.e1rm_delta_kg.is_none());

        assert!((squats[1].e1rm_delta_kg.unwrap() - 2.9).abs() < 0.05);
        assert_eq!(squats[1].e1rm_direction.as_deref(), Some("up"));

        assert_eq!(squats[2].e1rm_delta_kg, Some(0.0));
        assert_eq!(squats[2].e1rm_direction.as_deref(), Some("flat"));

        assert!((squats[3].e1rm_delta_kg.unwrap() + 8.8).abs() < 0.05);
        assert_eq!(squats[3].e1rm_direction.as_deref(), Some("down"));
    }

    #[test]
    fn logged_distance_spike_drives_the_safety_gate_without_a_manual_figure() {
        // A logged run 20 % over the recent-longest baseline is the strongest
        // running injury signal (RUN-SPIKE-001). A plain review that carries no
        // explicit spike figure must still trip the safety gate off the logged
        // run, so the deferral is reachable from ordinary logging.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::LogRun {
                distance_km: 12.0,
                duration_min: 60.0,
                hr_pct_max: 70.0,
                longest_recent_km: 10.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(Event::SubmitReview(SessionReview::default()), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "DangerousProgression");
    }

    #[test]
    fn a_first_run_with_no_baseline_does_not_trip_the_spike_safety_gate() {
        // With no recent-longest baseline the run view flags the distance
        // descriptively, but there is nothing for the run to be a spike *over*,
        // so the safety gate must not defer; otherwise every first-ever run
        // would raise a dangerous-progression deferral.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::LogRun {
                distance_km: 12.0,
                duration_min: 60.0,
                hr_pct_max: 70.0,
                longest_recent_km: 0.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(Event::SubmitReview(SessionReview::default()), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_ne!(fb.category, "DangerousProgression");
    }

    #[test]
    fn explicit_review_split_wins_over_track_fallback() {
        // GPS run finishes faster than it starts (a negative split), so the
        // track-derived fallback would not fire pacing feedback. An explicit
        // positive figure on the review must still win and fire it, proving the
        // review value takes precedence over the derived fallback.
        let app = Engine;
        let mut model = Model::default();
        let points = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.000,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.001,
                observed_at: 50,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.002,
                observed_at: 100,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.003,
                observed_at: 120,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.004,
                observed_at: 140,
                accuracy_m: 5.0,
            },
        ];
        app.update(
            Event::LogRunTrack {
                points,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        let review = SessionReview {
            positive_split_pct: Some(10.0),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "IntensityDiscipline");
    }

    // The Android shell hand-builds SubmitReview JSON (Core.kt), omitting the
    // Option fields it does not set. These two tests pin the exact wire form the
    // shell emits for the decoupling / easy-run-intensity review context, so a
    // rename on either side of the FFI fails here rather than silently dropping
    // the field to None.
    #[test]
    fn decoupling_review_wire_json_drives_feedback() {
        let wire = r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"decoupling":{"drift_pct":12.0,"cool_steady_context":true},"bad_day":false}}"#;
        let event: Event = serde_json::from_str(wire).expect("shell wire form parses");

        let app = Engine;
        let mut model = Model::default();
        app.update(event, &mut model).expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "CorrectiveProcess");
    }

    #[test]
    fn easy_run_intensity_wire_json_drives_feedback() {
        let wire = r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"easy_frac_time_above_vt1":0.3,"bad_day":false}}"#;
        let event: Event = serde_json::from_str(wire).expect("shell wire form parses");

        let app = Engine;
        let mut model = Model::default();
        app.update(event, &mut model).expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "IntensityDiscipline");
    }

    #[test]
    fn clearing_review_empties_feedback() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SubmitReview(SessionReview::default()), &mut model)
            .expect_only_render();
        assert!(app.view(&model).feedback.is_some());
        app.update(Event::ClearReview, &mut model)
            .expect_only_render();
        assert!(app.view(&model).feedback.is_none());
    }

    #[test]
    fn clearing_review_also_drops_its_week_deloads() {
        // review_adjustments share the review's lifecycle, so ClearReview must
        // take both the feedback and the deload cards, and ClearReadiness, which
        // owns the separate readiness list, must leave them untouched.
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            failed_key_sessions: Some(2),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();
        assert!(!app.view(&model).review_adjustments.is_empty());

        app.update(Event::ClearReadiness, &mut model)
            .expect_only_render();
        assert!(
            !app.view(&model).review_adjustments.is_empty(),
            "clearing readiness must not touch review-owned deloads"
        );

        app.update(Event::ClearReview, &mut model)
            .expect_only_render();
        assert!(
            app.view(&model).review_adjustments.is_empty(),
            "clearing the review must drop its deloads"
        );
    }

    #[test]
    fn clear_resets_inputs() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)),
            &mut model,
        )
        .expect_only_render();
        app.update(Event::ClearReadiness, &mut model)
            .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.input_count, 0);
        assert!(!vm.train_blocked);
    }

    #[test]
    fn clearing_sets_and_runs_empties_their_views() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogSet {
                exercise: "Back Squat".into(),
                weight_kg: 140.0,
                reps: 5,
                rpe: 8.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::LogRun {
                distance_km: 10.0,
                duration_min: 50.0,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        assert_eq!(app.view(&model).lifts.len(), 1);
        assert_eq!(app.view(&model).runs.len(), 1);

        app.update(Event::ClearSets, &mut model)
            .expect_only_render();
        assert!(app.view(&model).lifts.is_empty());
        assert_eq!(
            app.view(&model).runs.len(),
            1,
            "clearing sets must not touch runs"
        );

        app.update(Event::ClearRuns, &mut model)
            .expect_only_render();
        assert!(app.view(&model).runs.is_empty());
    }

    // -----------------------------------------------------------------------
    // Task 5: Stage-0 onboarding gates flow into the view (File 08 onboard-050)
    // -----------------------------------------------------------------------

    fn profile_with_health(health: HealthScreen) -> Profile {
        Profile {
            health,
            ..sample_profile()
        }
    }

    #[test]
    fn pregnancy_screen_gates_the_view_at_medical_referral() {
        // safety-045: SAFE-PREG-001 actually emitted, visible as a Safety
        // guidance row, MedicalReferral tier, train blocked: plus the
        // safety-047 avoid-list row.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SetProfile(profile_with_health(HealthScreen {
                pregnant: true,
                ..HealthScreen::default()
            })),
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.safety_tier.as_deref(), Some("MedicalReferral"));
        assert!(vm.train_blocked, "a deferral gate blocks training");
        let safety_rows: Vec<_> = vm
            .guidance
            .iter()
            .filter(|g| g.section == "Safety")
            .collect();
        assert!(
            safety_rows
                .iter()
                .any(|g| g.summary.contains("Pregnancy") && g.summary.contains("150 min/wk")),
            "the SAFE-PREG-001 deferral row must surface with the reference target"
        );
        assert!(
            safety_rows
                .iter()
                .any(|g| g.summary.contains("Valsalva") && g.summary.contains("2500 m")),
            "the safety-047 avoid-list must accompany the deferral"
        );
        assert!(safety_rows.iter().all(|g| g.safety_critical));
        // Gates lead the guidance list (screen BEFORE any prescription).
        assert_eq!(vm.guidance[0].section, "Safety");
    }

    #[test]
    fn parq_gate_clears_with_medical_clearance() {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SetProfile(profile_with_health(HealthScreen {
                parq_positive: true,
                ..HealthScreen::default()
            })),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.train_blocked);
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("PAR-Q+") && g.summary.contains("clearance"))
        );

        // Clearance lifts the gate (safety-044): programming resumes.
        app.update(
            Event::SetProfile(profile_with_health(HealthScreen {
                parq_positive: true,
                medically_cleared: true,
                ..HealthScreen::default()
            })),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(!vm.train_blocked);
        assert_eq!(vm.safety_tier, None);
    }

    #[test]
    fn youth_and_injury_screens_defer_visibly() {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SetProfile(profile_with_health(HealthScreen {
                youth: true,
                injury_or_rehab: true,
                ..HealthScreen::default()
            })),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert_eq!(vm.safety_tier.as_deref(), Some("MedicalReferral"));
        assert!(vm.train_blocked);
        // Both gates surface: pediatric names its prohibitions (safety-011),
        // injury defers to physio (safety-048).
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("1RM") && g.summary.contains("supervision"))
        );
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("physiotherapist"))
        );
        // A clean profile clears everything again.
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        assert!(!app.view(&model).train_blocked);
    }

    #[test]
    fn reds_readiness_signal_blocks_deficit_protein_target() {
        // safety-022 end-to-end: deficit requested while a RED-S readiness flag
        // is up → the protein section carries the refusal, not a number.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::RedS, 1.0)),
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 70.0,
                masters: false,
                deficit: true,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        let row = &vm.protein_targets[0];
        assert!(row.summary.contains("not prescribed"), "{}", row.summary);
        assert!(row.safety_critical);
        assert!(!row.summary.contains("g/day"), "no deficit number may leak");

        // Clearing the flag restores the graded target.
        app.update(Event::ClearReadiness, &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.protein_targets[0].summary.contains("g/day"));
    }

    #[test]
    fn reds_screen_flag_also_blocks_deficit() {
        // The onboarding-screen RED-S flag is the second channel into the
        // safety-022 refusal (no readiness signal needed).
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SetProfile(profile_with_health(HealthScreen {
                reds_signal: true,
                ..HealthScreen::default()
            })),
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 70.0,
                masters: false,
                deficit: true,
            },
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.protein_targets[0].summary.contains("not prescribed"));
        // And the screen flag itself defers training.
        assert!(vm.train_blocked);
    }

    // -----------------------------------------------------------------------
    // Task 5/19: overtraining escalations carried by the review
    // -----------------------------------------------------------------------

    #[test]
    fn nfor_cluster_review_defers_at_medical_referral_tier() {
        // autoreg-042: ≥2 wk decrement + ≥2 wellness domains → mandatory
        // recovery + professional referral, visible and blocking.
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            decline_weeks: Some(2),
            suppressed_wellness_domains: Some(2),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.safety_tier.as_deref(), Some("MedicalReferral"));
        assert!(vm.train_blocked);
        assert!(
            vm.review_adjustments
                .iter()
                .any(|a| a.summary.contains("recovery") && a.safety_critical)
        );
        // ClearReview lifts it.
        app.update(Event::ClearReview, &mut model)
            .expect_only_render();
        assert!(!app.view(&model).train_blocked);
    }

    #[test]
    fn decline_despite_deload_uses_the_strong_rule() {
        // safety-042 outranks the ExpertOpinion cluster when the decline
        // survived a deload: one deferral, graded Strong (Meeusen).
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            decline_weeks: Some(2),
            suppressed_wellness_domains: Some(2),
            despite_deload: true,
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        let defers: Vec<_> = vm
            .review_adjustments
            .iter()
            .filter(|a| a.summary.contains("defer") || a.summary.contains("professional"))
            .collect();
        assert_eq!(defers.len(), 1, "exactly one escalation fires");
        assert_eq!(defers[0].grade, "Strong");
    }

    #[test]
    fn sub_threshold_decline_reviews_add_nothing() {
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            decline_weeks: Some(1),
            suppressed_wellness_domains: Some(1),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.review_adjustments.is_empty());
        assert!(!vm.train_blocked);
        assert_eq!(vm.safety_tier, None);
    }

    #[test]
    fn mrv_cluster_and_threshold_retest_surface_in_review() {
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            mrv_sign_cluster: true,
            pace_at_hr_improved_weeks: Some(2),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        assert!(
            vm.review_adjustments
                .iter()
                .any(|a| a.summary.starts_with("Deload")),
            "autoreg-025 MRV deload surfaces"
        );
        assert!(
            vm.review_adjustments
                .iter()
                .any(|a| a.summary.contains("re-test")),
            "autoreg-032 threshold re-test cue surfaces"
        );
        // Neither blocks training.
        assert!(!vm.train_blocked);
        // One week of improvement is below the autoreg-032 bound → no cue.
        let review = SessionReview {
            pace_at_hr_improved_weeks: Some(1),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();
        assert!(app.view(&model).review_adjustments.is_empty());
    }

    // -----------------------------------------------------------------------
    // Task 19: readiness-context + feedback wiring
    // -----------------------------------------------------------------------

    #[test]
    fn e1rm_dip_view_shows_rpe_cap_row() {
        // autoreg-006 second clause reaches the shell as its own row.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::EstimatedOneRm, 0.93)),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(
            vm.adjustments
                .iter()
                .any(|a| a.summary.contains("planned RPE −1"))
        );
        assert!(!vm.train_blocked, "an RPE cap modifies, never blocks");
    }

    #[test]
    fn high_load_block_profile_arms_the_saturation_guard() {
        // autoreg-029 through the full view: high HRV + under-target RPE would
        // add load, but a high-load-block profile strips the increase.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SetProfile(Profile {
                high_load_block: true,
                ..sample_profile()
            }),
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Rpe, -2.0)),
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::HrvLnRmssd, 1.0)),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(
            !vm.adjustments
                .iter()
                .any(|a| a.summary.contains("Increase load")),
            "no auto load-add under parasympathetic saturation"
        );

        // Same readings without the block flag do add load.
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        assert!(
            app.view(&model)
                .adjustments
                .iter()
                .any(|a| a.summary.contains("Increase load"))
        );
    }

    #[test]
    fn interval_mastery_review_reaches_feedback() {
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            interval: Some(IntervalExec {
                target_paces_met: true,
                rpe_at_or_below_target: true,
            }),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();
        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "PositiveMastery");
        assert!(fb.anchor_mastery, "praise must anchor a concrete achievement");
        // Reps over target RPE fall through to other arms, not mastery.
        let review = SessionReview {
            interval: Some(IntervalExec {
                target_paces_met: true,
                rpe_at_or_below_target: false,
            }),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();
        assert_ne!(
            app.view(&model).feedback.expect("feedback").category,
            "PositiveMastery"
        );
    }

    #[test]
    fn female_bsi_referral_appends_clinician_prompt() {
        // feedback-035: female profile + bone-stress referral → the gentle
        // menstrual/nutrition prompt rides on the CONCERN_INJURY message.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SetProfile(Profile {
                female: true,
                ..sample_profile()
            }),
            &mut model,
        )
        .expect_only_render();
        let review = SessionReview {
            bone_pain_red_flag: true,
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();
        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "ConcernInjury");
        assert!(
            fb.message.contains("menstrual"),
            "prompt appended: {}",
            fb.message
        );

        // Male/unset profile: no prompt on the same referral.
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        let fb = app.view(&model).feedback.expect("feedback present");
        assert!(!fb.message.contains("menstrual"));
        // And a female non-injury review never gets it either.
        app.update(
            Event::SetProfile(Profile {
                female: true,
                ..sample_profile()
            }),
            &mut model,
        )
        .expect_only_render();
        app.update(Event::SubmitReview(SessionReview::default()), &mut model)
            .expect_only_render();
        let fb = app.view(&model).feedback.expect("feedback present");
        assert!(!fb.message.contains("menstrual"));
    }

    #[test]
    fn feedback_verbosity_tracks_training_age() {
        // feedback-023/024: WeekToWeek cadence (intermediate) → beginner-style
        // conservative defaults; MonthToMonth (advanced) → 2-3 metric density.
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        app.update(Event::SubmitReview(SessionReview::default()), &mut model)
            .expect_only_render();
        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!((fb.max_takeaways, fb.max_metrics), (1, 1));
        assert!(fb.rationale_mandatory && fb.minimize_jargon);

        app.update(
            Event::SetProfile(Profile {
                progression_cadence: ProgressionCadence::MonthToMonth,
                ..sample_profile()
            }),
            &mut model,
        )
        .expect_only_render();
        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!((fb.max_takeaways, fb.max_metrics), (1, 3));
        assert!(!fb.minimize_jargon);
    }

    #[test]
    fn overtraining_duration_condition_flows_through_review() {
        // feedback-036: two signals over half a week must NOT fire the
        // recovery concern; over a full week it must.
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            overtraining_signal_count: 2,
            overtraining_signal_weeks: Some(0.5),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();
        assert_ne!(
            app.view(&model).feedback.expect("feedback").category,
            "ConcernRecovery",
            "a single noisy night must not fire the recovery concern"
        );
        let review = SessionReview {
            overtraining_signal_count: 2,
            overtraining_signal_weeks: Some(1.5),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();
        assert_eq!(
            app.view(&model).feedback.expect("feedback").category,
            "ConcernRecovery"
        );
    }

    #[test]
    fn short_effort_decoupling_readiness_is_discarded() {
        // File 06 validity gate through the view: a 15-minute effort's
        // decoupling never downgrades; >20 min does.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SubmitReadiness(ReadinessInput {
                effort_min: Some(15.0),
                ..input(ReadinessSignal::AerobicDecoupling, 14.0)
            }),
            &mut model,
        )
        .expect_only_render();
        assert!(app.view(&model).adjustments.is_empty());

        app.update(Event::ClearReadiness, &mut model)
            .expect_only_render();
        app.update(
            Event::SubmitReadiness(ReadinessInput {
                effort_min: Some(40.0),
                ..input(ReadinessSignal::AerobicDecoupling, 14.0)
            }),
            &mut model,
        )
        .expect_only_render();
        assert!(!app.view(&model).adjustments.is_empty());
    }

    #[test]
    fn duration_moderator_row_rides_on_expected_interference() {
        // hybrid-004: the duration-strongest-moderator row appears only when
        // interference is expected for this profile.
        let app = Engine;
        let mut model = Model::default();
        // sample_profile: 5 running days → interference expected.
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        assert!(
            app.view(&model)
                .guidance
                .iter()
                .any(|g| g.summary.contains("strongest moderator"))
        );
        // 2 easy days at 60% VO2max → no interference, no moderator row.
        app.update(
            Event::SetProfile(Profile {
                running_days_per_week: 2,
                running_km_per_week: 20.0,
                endurance_intensity_pct_vo2max: 60.0,
                ..sample_profile()
            }),
            &mut model,
        )
        .expect_only_render();
        assert!(
            !app.view(&model)
                .guidance
                .iter()
                .any(|g| g.summary.contains("strongest moderator"))
        );
    }

    #[test]
    fn reference_carries_new_hybrid_and_safety_rows() {
        let vm = Engine.view(&Model::default());
        for needle in [
            "freshest",
            "refuel carbohydrate",
            "Phase policy",
            "tendon stiffness",
            "energy availability adequate (RED-S/LEA guard)",
        ] {
            assert!(
                vm.reference
                    .iter()
                    .any(|g| g.summary.to_lowercase().contains(&needle.to_lowercase())),
                "reference row missing: {needle}"
            );
        }
        // Sections stay contiguous in the reference list too.
        let mut seen: Vec<&str> = Vec::new();
        for g in &vm.reference {
            match seen.last() {
                Some(&last) if last == g.section => {}
                _ => {
                    assert!(
                        !seen.contains(&g.section.as_str()),
                        "section {} reappears non-contiguously",
                        g.section
                    );
                    seen.push(g.section.as_str());
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 20: dormant-engine wiring, training load, weekly volume system,
    // GPS QC, calculators, review triggers, trend/tone/provisional/source.
    // -----------------------------------------------------------------------

    const DAY: i64 = 86_400;
    const WEEK: i64 = 604_800;

    fn log_run_at(km: f64, minutes: f64, hr: f64, at: i64) -> Event {
        Event::LogRun {
            distance_km: km,
            duration_min: minutes,
            hr_pct_max: hr,
            longest_recent_km: 50.0, // quiet the spike gate; not under test here
            observed_at: at,
        }
    }

    #[test]
    fn training_load_chains_lucia_trimp_into_ctl_atl_tsb() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).training_load.is_none(), "no runs → no load view");

        let t0 = 100 * WEEK;
        for (i, hr) in [(0i64, 70.0), (2, 75.0), (4, 90.0)] {
            app.update(log_run_at(10.0, 60.0, hr, t0 + i * DAY), &mut model)
                .expect_only_render();
        }
        // An undated run and an HR-less run are skipped, never fabricated.
        app.update(log_run_at(8.0, 45.0, 75.0, 0), &mut model)
            .expect_only_render();
        app.update(log_run_at(8.0, 45.0, 0.0, t0 + 5 * DAY), &mut model)
            .expect_only_render();

        let tl = app.view(&model).training_load.expect("load view present");
        assert_eq!(tl.sessions_counted, 3);
        assert_eq!(tl.sessions_skipped, 2);
        assert_eq!(tl.days, 5, "day span 0..=4");
        assert!(tl.ctl > 0.0 && tl.atl > 0.0);
        // Fresh loading: acute fatigue outruns chronic fitness → negative form.
        assert!(tl.atl > tl.ctl, "ATL {} vs CTL {}", tl.atl, tl.ctl);
        assert!((tl.tsb - (tl.ctl - tl.atl)).abs() < 0.11, "TSB = CTL − ATL");
        assert!(tl.method.contains("Lucia TRIMP"), "{}", tl.method);
        assert!(tl.summary.contains("not a performance predictor"));
        assert_eq!(tl.grade, "Moderate");
        assert!(!tl.citation.is_empty());
    }

    #[test]
    fn weekly_report_runs_the_volume_system_over_the_latest_week() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).weekly_report.is_empty());

        let w1 = 100 * WEEK;
        let w2 = 101 * WEEK;
        // Prior week: 3 × 10 km easy (Z1, 75 %HRmax) = 30 km.
        for i in [0i64, 2, 4] {
            app.update(log_run_at(10.0, 62.0, 75.0, w1 + i * DAY), &mut model)
                .expect_only_render();
        }
        // Current week: 20 + 16 = 36 km (+20%, over any cap), one Z3 run.
        app.update(log_run_at(20.0, 120.0, 75.0, w2), &mut model)
            .expect_only_render();
        app.update(log_run_at(16.0, 90.0, 90.0, w2 + 2 * DAY), &mut model)
            .expect_only_render();

        let report = app.view(&model).weekly_report;
        assert!(!report.is_empty());
        for row in &report {
            assert!(!row.grade.is_empty() && !row.citation.is_empty(), "{}", row.summary);
            assert_ne!(row.grade, "MarketingMyth");
        }
        let has = |needle: &str| report.iter().any(|r| r.summary.contains(needle));
        assert!(has("over the increase cap"), "rows: {report:#?}");
        // Long run 20/36 km = 56%: over the ≤25% Daniels single-run cap.
        assert!(has("over the ≤25% single-run cap"), "rows: {report:#?}");
        assert!(has("time-in-zone"), "counting-method note present");
        assert!(has("Long run 20.0 km"), "share row present");
        // 20 km long run vs a 5.1 km daily average: over the 2× bound.
        assert!(has("exceeds 2× your average daily distance"), "daily-avg bound row");
        // Z1 share: 120 of 210 min = 57%, below the 80% easy floor.
        assert!(has("below the ~80% easy floor"), "rows: {report:#?}");
        // One Z3 session, within quality caps.
        assert!(has("1 hard (Z3) session"), "rows: {report:#?}");
    }

    #[test]
    fn weekly_report_hybrid_ramp_guard_fires_for_a_lifting_runner() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        let w1 = 100 * WEEK;
        app.update(log_run_at(30.0, 180.0, 75.0, w1), &mut model)
            .expect_only_render();
        app.update(log_run_at(36.0, 216.0, 75.0, w1 + WEEK), &mut model)
            .expect_only_render();

        let report = app.view(&model).weekly_report;
        assert!(
            report.iter().any(|r| r.summary.contains("cap the combined ramp")),
            "hybrid-021 guard row expected: {report:#?}"
        );
    }

    #[test]
    fn weekly_report_flags_heavy_leg_work_too_close_to_a_hard_run() {
        let app = Engine;
        let mut model = Model::default();
        let t = 100 * WEEK;
        app.update(
            Event::LogSet {
                exercise: "Barbell back squat".into(),
                weight_kg: 120.0,
                reps: 5,
                rpe: 8.0,
                observed_at: t,
            },
            &mut model,
        )
        .expect_only_render();
        // The week's long run 10 h later: inside the 24 h buffer.
        app.update(log_run_at(14.0, 80.0, 90.0, t + 10 * 3600), &mut model)
            .expect_only_render();

        let report = app.view(&model).weekly_report;
        let row = report
            .iter()
            .find(|r| r.summary.contains("keep ≥24 h between"))
            .expect("heavy-leg/run spacing row");
        assert!(row.summary.contains("10 h apart"), "{}", row.summary);
    }

    #[test]
    fn gps_qc_drops_teleport_and_stuck_time_fixes_and_reports_the_count() {
        let app = Engine;
        let mut model = Model::default();
        let p = |lon: f64, at: i64| GpsPoint {
            lat: 0.0,
            lon,
            observed_at: at,
            accuracy_m: 5.0,
        };
        app.update(
            Event::LogRunTrack {
                points: vec![
                    p(0.000, 0),
                    p(0.001, 20),
                    // Teleport: ~11 km implied in 1 s (>12 m/s), dropped.
                    p(0.100, 21),
                    p(0.002, 40),
                    // Non-advancing timestamp, dropped.
                    p(0.0025, 40),
                    p(0.003, 60),
                ],
                hr_pct_max: 78.0,
                longest_recent_km: 12.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        let run = &app.view(&model).runs[0];
        assert_eq!(run.qc_dropped, 2, "teleport + stuck-time fixes dropped");
        // Distance is the clean 3-hop track (~0.33 km), not an 11 km teleport.
        assert!(
            run.distance_km > 0.2 && run.distance_km < 0.5,
            "QC'd distance, got {}",
            run.distance_km
        );
        // Manual runs report zero dropped fixes.
        app.update(log_run_at(5.0, 30.0, 75.0, 0), &mut model)
            .expect_only_render();
        assert_eq!(app.view(&model).runs[1].qc_dropped, 0);
    }

    #[test]
    fn lift_view_cross_checks_e1rm_across_formulas_when_reliable() {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::LogSet {
                exercise: "Cross Check Squat".into(),
                weight_kg: 100.0,
                reps: 5,
                rpe: 8.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();
        // 12 reps is past the strength-006 reliability cap → no range.
        app.update(
            Event::LogSet {
                exercise: "Cross Check Squat".into(),
                weight_kg: 60.0,
                reps: 12,
                rpe: 8.0,
                observed_at: 0,
            },
            &mut model,
        )
        .expect_only_render();

        let lifts = app.view(&model).lifts;
        let check = lifts[0].cross_check.as_ref().expect("range for 5 reps");
        assert_eq!(check.formulas, 3);
        assert!(check.low_kg <= check.high_kg);
        // Brzycki at 100×5 = 112.5 (the low), Lombardi ≈ 117.5 (the high).
        assert!((check.low_kg - 112.5).abs() < 0.1, "low {}", check.low_kg);
        assert!((check.high_kg - 117.5).abs() < 0.1, "high {}", check.high_kg);
        assert!(!check.citation.is_empty());
        assert!(lifts[1].cross_check.is_none(), "12 reps is unreliable");
    }

    #[test]
    fn lift_audit_runs_prilepin_and_the_depth_jump_gate() {
        let app = Engine;
        let mut model = Model::default();
        let mut p = sample_profile();
        p.bodyweight_kg = Some(80.0);
        app.update(Event::SetProfile(p), &mut model)
            .expect_only_render();
        // 3 × 5 @ ~86 %1RM → 15 total reps, inside the Prilepin 80–90 window.
        for _ in 0..3 {
            app.update(
                Event::LogSet {
                    exercise: "Barbell back squat".into(),
                    weight_kg: 140.0,
                    reps: 5,
                    rpe: 8.5,
                    observed_at: 100 * WEEK,
                },
                &mut model,
            )
            .expect_only_render();
        }

        let audit = app.view(&model).lift_audit;
        let prilepin = audit
            .iter()
            .find(|r| r.summary.contains("Prilepin"))
            .expect("prilepin row");
        assert!(prilepin.summary.contains("15 total reps"), "{}", prilepin.summary);
        assert!(prilepin.summary.contains("within"), "{}", prilepin.summary);
        // Squat e1RM 140×5 (Epley ≈163 kg) vs 80 kg BW → >1.5×, cleared.
        let dj = audit
            .iter()
            .find(|r| r.summary.contains("Depth jumps"))
            .expect("depth-jump row");
        assert!(dj.summary.contains("cleared"), "{}", dj.summary);
    }

    #[test]
    fn cooper_calculator_estimates_vo2max_and_refuses_the_floor() {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::ComputeCooper {
                distance_m_12min: 2400.0,
            },
            &mut model,
        )
        .expect_only_render();
        let rows = app.view(&model).cooper;
        assert_eq!(rows.len(), 1);
        // (2400 − 504.9)/44.73 ≈ 42.4 ml/kg/min.
        assert!(rows[0].summary.contains("42.4"), "{}", rows[0].summary);
        assert_eq!(rows[0].grade, "Moderate");

        app.update(
            Event::ComputeCooper {
                distance_m_12min: 400.0,
            },
            &mut model,
        )
        .expect_only_render();
        let rows = app.view(&model).cooper;
        assert!(rows[0].summary.contains("too short"), "{}", rows[0].summary);

        app.update(Event::ClearCooper, &mut model).expect_only_render();
        assert!(app.view(&model).cooper.is_empty());
    }

    #[test]
    fn critical_speed_calculator_fits_the_protocol_and_names_violations() {
        let app = Engine;
        let mut model = Model::default();
        // Ideal pair: 1200 m / 3 min + 5000 m / 20 min.
        app.update(
            Event::ComputeCriticalSpeed {
                efforts: vec![
                    CsEffortIn {
                        distance_m: 1200.0,
                        time_sec: 180.0,
                    },
                    CsEffortIn {
                        distance_m: 5000.0,
                        time_sec: 1200.0,
                    },
                ],
            },
            &mut model,
        )
        .expect_only_render();
        let rows = app.view(&model).critical_speed;
        assert_eq!(rows.len(), 1, "ideal pairing → no protocol note: {rows:#?}");
        // CS = 3800/1020 ≈ 3.73 m/s, D′ = 1200 − 3.73·180 ≈ 529 m.
        assert!(rows[0].summary.contains("3.73"), "{}", rows[0].summary);
        assert!(rows[0].summary.contains("529"), "{}", rows[0].summary);

        // A single effort violates the 2-effort minimum, explained not faked.
        app.update(
            Event::ComputeCriticalSpeed {
                efforts: vec![CsEffortIn {
                    distance_m: 1200.0,
                    time_sec: 180.0,
                }],
            },
            &mut model,
        )
        .expect_only_render();
        let rows = app.view(&model).critical_speed;
        assert!(rows[0].summary.contains("at least 2"), "{}", rows[0].summary);

        app.update(Event::ClearCriticalSpeed, &mut model)
            .expect_only_render();
        assert!(app.view(&model).critical_speed.is_empty());
    }

    #[test]
    fn apre_calculator_applies_the_small_lifter_cap() {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::ComputeApre {
                scheme: autoreg::ApreScheme::Apre6,
                reps: 9,
                current_load_lb: 100.0,
            },
            &mut model,
        )
        .expect_only_render();
        let rows = app.view(&model).apre;
        assert!(rows[0].summary.contains("+5 to +10 lb"), "{}", rows[0].summary);

        // Small lifter: the +lb band shrinks proportionally under 100 lb.
        app.update(
            Event::ComputeApre {
                scheme: autoreg::ApreScheme::Apre6,
                reps: 9,
                current_load_lb: 60.0,
            },
            &mut model,
        )
        .expect_only_render();
        let rows = app.view(&model).apre;
        assert!(rows[0].summary.contains("+3 to +6 lb"), "{}", rows[0].summary);

        app.update(Event::ClearApre, &mut model).expect_only_render();
        assert!(app.view(&model).apre.is_empty());
    }

    #[test]
    fn review_carries_the_new_autoreg_and_hypertrophy_triggers() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        app.update(
            Event::SubmitReview(SessionReview {
                mcv_delta_m_s: Some(-0.10),
                first_set_reps_met: Some(false),
                first_set_rpe_delta: Some(1.5),
                cut_last_two_sessions: true,
                interval_reps_over_target: Some(2),
                can_hold_easy_pace_under_hr_cap: Some(false),
                hrv_unreliable_last_three: Some(2),
                hrv_suppressed_days: Some(3),
                wellness_suppressed_days: Some(2),
                rhr_rising: true,
                hypertrophy_deload_triggers: Some(2),
                rep_drop_frac: Some(0.2),
                low_recovery: true,
                ..SessionReview::default()
            }),
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        let has = |needle: &str| {
            vm.review_adjustments
                .iter()
                .any(|a| a.summary.contains(needle))
        };
        assert!(has("reduce working loads"), "VBT: {:#?}", vm.review_adjustments);
        assert!(has("drop the last planned set"), "set-volume action");
        assert!(has("hold weekly volume"), "two-cut-session hold");
        assert!(has("slow the remaining reps ~3%"), "interval autoreg");
        assert!(has("the HR cap governs easy days"), "easy-pace cap");
        assert!(has("suspend HRV gating"), "unreliable HRV");
        assert!(has("recovery day / easy block"), "HRV suppression streak");
        assert!(has("1–3 easy days"), "wellness+RHR");
        assert!(has("take the deload week now"), "hypertrophy triggers");
        assert!(has("lengthen the rest interval"), "rep-drop rest");
        assert!(has("scale this week to"), "recovery-adjusted volume");
        // Every review row is graded (HARD RULE 2).
        for a in &vm.review_adjustments {
            assert!(!a.grade.is_empty() && !a.citation.is_empty(), "{}", a.summary);
        }
    }

    #[test]
    fn review_week_deloads_fire_from_rpe_creep_and_single_day_hrv() {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SubmitReview(SessionReview {
                rpe_creep_plus_one: true,
                wellness_z_low_days: Some(3),
                hrv_single_day_z: Some(-1.5),
                hrv_downtrend_days: Some(2),
                ..SessionReview::default()
            }),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(
            vm.review_adjustments
                .iter()
                .any(|a| a.summary.starts_with("Deload 1 wk")),
            "autoreg-024 deload: {:#?}",
            vm.review_adjustments
        );
        assert!(
            vm.review_adjustments
                .iter()
                .any(|a| a.summary.contains("Downgrade to an easier session")),
            "autoreg-028 single-day downgrade"
        );
    }

    #[test]
    fn review_decoupling_band_verdict_rides_with_the_measurement() {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SubmitReview(SessionReview {
                decoupling: Some(Decouple {
                    drift_pct: 12.0,
                    cool_steady_context: true,
                }),
                ..SessionReview::default()
            }),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        let band = vm
            .review_adjustments
            .iter()
            .find(|a| a.summary.contains("Decoupling 12.0%"))
            .expect("band verdict row");
        assert!(band.summary.contains("≥10%"), "{}", band.summary);
        assert!(!band.citation.is_empty());
    }

    #[test]
    fn novice_stall_action_reaches_the_review_rows() {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SubmitReview(SessionReview {
                stall_failed_sessions: Some(3),
                stall_adequate_recovery: true,
                ..SessionReview::default()
            }),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(
            vm.review_adjustments
                .iter()
                .any(|a| a.summary.contains("deload this lift 10%")),
            "{:#?}",
            vm.review_adjustments
        );
    }

    #[test]
    fn trend_tone_provisional_and_signal_source_surface() {
        let app = Engine;
        let mut model = Model::default();

        // Fresh model: everything is a provisional population default.
        let vm = app.view(&model);
        assert!(vm.provisional.is_some(), "0 days of data → provisional");
        assert!(vm.autoreg_source.is_none(), "no inputs → no source row");
        assert!(vm.trend.is_none());

        // A wellness input without HRV → subjective + performance source.
        app.update(
            Event::SubmitReadiness(ReadinessInput {
                signal: ReadinessSignal::WellnessZ,
                value: -0.2,
                observed_at: 100 * WEEK,
                streak: 0,
                pain: None,
                effort_min: None,
            }),
            &mut model,
        )
        .expect_only_render();
        let src = app.view(&model).autoreg_source.expect("source row");
        assert!(src.summary.contains("subjective wellness + performance"), "{}", src.summary);

        // An HRV reading upgrades the source to the rolling HRV gate.
        app.update(
            Event::SubmitReadiness(ReadinessInput {
                signal: ReadinessSignal::HrvLnRmssd,
                value: 0.1,
                observed_at: 100 * WEEK,
                streak: 0,
                pain: None,
                effort_min: None,
            }),
            &mut model,
        )
        .expect_only_render();
        let src = app.view(&model).autoreg_source.expect("source row");
        assert!(src.summary.contains("rolling HRV gate"), "{}", src.summary);

        // A load-explained decline routes to the recovery-first trend message,
        // and a planned-hard session gets the praise-effort tone.
        app.update(
            Event::SubmitReview(SessionReview {
                trend_direction: Some("down".into()),
                performance_down: true,
                low_recovery: true,
                planned_hard: Some(true),
                ..SessionReview::default()
            }),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        let trend = vm.trend.expect("trend row");
        assert!(trend.summary.contains("recovery first"), "{}", trend.summary);
        assert_eq!(
            vm.feedback.expect("feedback present").tone.as_deref(),
            Some("PraiseEffort")
        );

        // 14 distinct dated days of logging ends the provisional window.
        for i in 1..=14 {
            app.update(
                Event::LogSet {
                    exercise: "Bench press".into(),
                    weight_kg: 60.0,
                    reps: 5,
                    rpe: 7.0,
                    observed_at: i * DAY + 10,
                },
                &mut model,
            )
            .expect_only_render();
        }
        assert!(app.view(&model).provisional.is_none(), "baseline reached");
    }

    #[test]
    fn hr_zone_table_extends_with_karvonen_maf_and_recalc_cues() {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::ComputeHrZones {
                age_years: 30.0,
                resting_hr_bpm: Some(50.0),
                weeks_since_recalc: Some(5),
                weeks_since_pace_test: Some(6),
            },
            &mut model,
        )
        .expect_only_render();
        let zones = app.view(&model).hr_zones;
        let has = |needle: &str| zones.iter().any(|z| z.summary.contains(needle));
        assert!(has("Karvonen (%HRR) targets shown"), "preference row: {zones:#?}");
        // Easy band 65–79%: Karvonen at HRmax 187 / RHR 50 → 139–158 bpm.
        let easy = zones
            .iter()
            .find(|z| z.summary.starts_with("Easy"))
            .expect("easy row");
        assert!(easy.summary.contains("Karvonen 139–158 bpm"), "{}", easy.summary);
        // Repetition is pace-governed: no HR validity claimed.
        let rep = zones
            .iter()
            .find(|z| z.summary.starts_with("Repetition"))
            .expect("repetition row");
        assert!(rep.summary.contains("pace-governed"), "{}", rep.summary);
        assert!(has("MAF aerobic cap"), "MAF row");
        assert!(has("recompute from a measured HRmax"), "recalc-due row");
        assert!(has("re-test to set paces"), "pace-retest row");
    }

    #[test]
    fn hypertrophy_plan_decides_the_next_meso_and_states_effort_rules() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        app.update(
            Event::PlanHypertrophyMeso {
                muscle: "chest".into(),
                weeks: 4,
                not_growing: true,
                recovering_easily: true,
            },
            &mut model,
        )
        .expect_only_render();
        let plan = app.view(&model).hypertrophy_plan;
        let has = |needle: &str| plan.iter().any(|r| r.summary.contains(needle));
        // sample_profile plans 14 weekly sets → +2 next block.
        assert!(has("raise next mesocycle to 16 sets/wk"), "{plan:#?}");
        assert!(has("Rest between sets (heavy compounds)"), "rest row");
        assert!(has("Mesocycle shape"), "meso structure row");
        assert!(has("1–3 RIR"), "default effort band");
        assert!(has("only on machines/isolation"), "failure gate");
        assert!(has("30–85% 1RM"), "load interchangeability");
        assert!(has("Tempo: controlled"), "tempo row");
    }

    #[test]
    fn race_prediction_notes_flag_stale_input_and_marathon_optimism() {
        let app = Engine;
        let mut model = Model::default();
        // Longest logged run: 20 km, under the 30 km marathon-support line.
        app.update(log_run_at(20.0, 110.0, 75.0, 100 * WEEK), &mut model)
            .expect_only_render();
        app.update(
            Event::PredictRace {
                recent_distance_m: 10_000.0,
                recent_time_sec: 2_520.0,
                goal_distance_m: 42_195.0,
                weekly_km: 50.0,
                weeks_since_race: Some(10),
            },
            &mut model,
        )
        .expect_only_render();
        let pred = app.view(&model).race_prediction.expect("prediction");
        assert_eq!(pred.notes.len(), 2, "stale + optimism: {:#?}", pred.notes);
        assert!(pred.notes.iter().any(|n| n.summary.contains("re-test")));
        assert!(
            pred.notes
                .iter()
                .any(|n| n.summary.contains("optimistic without long-run support")
                    && n.summary.contains("derate ~2–3 VDOT points")),
            "optimism note carries the running-008 derate: {:#?}",
            pred.notes
        );

        // A fresh input race against a 5K goal carries no notes.
        app.update(
            Event::PredictRace {
                recent_distance_m: 10_000.0,
                recent_time_sec: 2_520.0,
                goal_distance_m: 5_000.0,
                weekly_km: 50.0,
                weeks_since_race: Some(2),
            },
            &mut model,
        )
        .expect_only_render();
        assert!(app.view(&model).race_prediction.expect("prediction").notes.is_empty());
    }

    #[test]
    fn guidance_carries_environment_reentry_and_run_rx_rows() {
        let app = Engine;
        let mut model = Model::default();
        let mut p = sample_profile();
        p.environment = Some(Environment::Heat);
        p.env_temp_c = Some(30.0);
        p.weeks_off = Some(6.0);
        app.update(Event::SetProfile(p), &mut model)
            .expect_only_render();
        let g = app.view(&model).guidance;
        let has = |needle: &str| g.iter().any(|r| r.summary.contains(needle));
        assert!(has("STOP on heat-illness signs"), "heat modifier row");
        assert!(has("pace correction for heat"), "running-041 trigger row");
        assert!(has("restart at ~70% of prior loads"), "reentry row: 6 wk off");
        assert!(has("Post-layoff MEV is reduced"), "layoff MEV row");
        assert!(has("Easy / general-aerobic runs"), "easy Rx row");
        assert!(has("Cruise intervals"), "cruise Rx row");
        assert!(has("VO2max intervals"), "vo2 Rx row");
        assert!(has("Strides:"), "strides row");
        assert!(has("Recovery week:"), "recovery week row");
        assert!(has("Race taper (Marathon)"), "distance taper row");
        assert!(has("Progress ONE variable"), "single-variable rule row");
        assert!(has("Long runs:"), "long-run Rx row");
        assert!(has("Test a true 1RM only when"), "1RM-test gate row");
        // The heat-stop row is safety-critical, per ENV-001.
        let heat = g
            .iter()
            .find(|r| r.summary.contains("STOP on heat-illness"))
            .unwrap();
        assert!(heat.safety_critical);
    }

    #[test]
    fn unscheduled_deload_row_rides_on_overtraining_signals() {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SubmitReview(SessionReview {
                overtraining_signal_count: 2,
                overtraining_signal_weeks: Some(0.5),
                ..SessionReview::default()
            }),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(
            vm.review_adjustments
                .iter()
                .any(|a| a.summary.contains("unscheduled down week")),
            "{:#?}",
            vm.review_adjustments
        );
    }
}
