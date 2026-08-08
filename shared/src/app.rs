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
use std::sync::OnceLock;

use crate::feedback::FeedbackCategory;
use crate::hybrid::ConcurrentGoal;
use crate::individualization::{Environment, ProgressionCadence};
use crate::running::{GoalDistance, GpsPoint};
use crate::schema::{
    Adjustment, CheckinInput, EvidenceGrade, Goal, HealthScreen, LiftIntensity, LiftSessionType,
    MesoPhase, Mesocycle, Prescription, Program, ReadinessInput, ReadinessSignal, Recommended,
    RunIntensity, RunSessionType, RunVolume, SafetyTier, Session, SessionType, ThreeZone, VdotBand,
    WorkoutType,
};
use crate::evidence::graded;
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
    /// Stable per-entry identity: a shell-assigned monotonic id
    /// (epoch-millis at first log) that survives edits so `AmendSet`/`DeleteEntry`
    /// target THIS row even when its `observed_at` was backdated to collide with
    /// another. 0 = legacy/absent; those fall back to matching
    /// on `observed_at`.
    entry_id: u64,
}

#[derive(Clone)]
struct LoggedRun {
    /// Manual distance; ignored when `track` is non-empty (derived instead).
    distance_km: f64,
    /// Manual duration; ignored when `track` is non-empty (derived instead).
    duration_min: f64,
    hr_pct_max: f64,
    /// Spike baseline baked at ingest (max of the caller-supplied paired-history
    /// value and the 30-day-longest at log time). Still drives the run
    /// CARD's descriptive spike chip; the SAFETY GATE now derives its baseline
    /// fresh from `model.runs` at view() time so a deleted baseline run re-arms it
    /// (see `latest_run_spike_frac`).
    longest_recent_km: f64,
    /// GPS fixes for a tracked run; empty for a hand-entered run.
    track: Vec<GpsPoint>,
    /// Indices into `track` that BEGIN a new recording segment: the first
    /// fix captured after a pause + possible relocation. Every track metric skips
    /// the pause-bridge leg entering such an index (no distance, no time), and the
    /// GPX export breaks a `<trkseg>` there. Empty = one continuous segment, the
    /// legacy (re-anchored) and hand-logged path. Model-internal, not wire-decoded.
    track_segment_starts: Vec<u32>,
    /// Log time, unix seconds; 0 when undated (pre-timestamp persisted event).
    observed_at: i64,
    /// Stable per-entry identity; see [`LoggedSet::entry_id`]. 0 =
    /// legacy/absent → matched on `observed_at` instead.
    entry_id: u64,
    /// User-declared run-intent label. USER DATA, carries no evidence and
    /// drives no coaching: storage + history display only (HARD RULE 1). `None`
    /// = untagged; never fabricated. (Model-internal struct, not wire-decoded.)
    workout_type: Option<WorkoutType>,
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
    /// Also the consolidated person-data source the protein calculator prefills
    /// from: person data is entered once, on the profile.
    #[serde(default)]
    pub bodyweight_kg: Option<f64>,
    /// Age in years (optional): consolidated person data. The
    /// HR-zone calculator prefills its age input from here; no rule branches on
    /// it (display/prefill only, HARD RULE 1; invents no claim). `None` =
    /// unstated (old profiles parse unchanged).
    #[serde(default)]
    pub age_years: Option<f64>,
    /// Resting HR, bpm (optional), consolidated person data. Prefills the
    /// HR-zone Karvonen input and can seed the morning-checkin RHR baseline.
    /// Display/prefill only; `None` = unstated.
    #[serde(default)]
    pub resting_hr_bpm: Option<f64>,
    /// Measured maximum HR, bpm (optional), consolidated person data. Stored so
    /// it is entered once; the HR-zone estimate still shows the honest Tanaka
    /// age formula (no rule branches on this, HARD RULE 1). `None` = unstated.
    #[serde(default)]
    pub measured_hr_max: Option<f64>,
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
    /// When the review was submitted, unix seconds; shell-supplied (the core
    /// holds no clock), `#[serde(default)]` keeps pre-timestamp persisted
    /// events replayable (decode as 0 = undated). Provenance only, no rule
    /// consumes it yet.
    #[serde(default)]
    pub observed_at: i64,
}

#[derive(Default)]
pub struct Model {
    /// Observed readiness signals, in submission order. Day-scoped: cleared by
    /// `ClearReadiness` (the raw-signal / advanced path + red-flag reports).
    inputs: Vec<ReadinessInput>,
    /// Morning check-ins: a RETAINED multi-day history of raw
    /// human observations. NOT cleared by `ClearReadiness`; the whole point is
    /// a rolling baseline the core normalizes into z-scores/deltas/streaks.
    checkins: Vec<CheckinInput>,
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
    /// The accepted "Plan my training" request. Present once the
    /// user accepts a generated plan; the `Program` itself is re-derived in
    /// `view()` from profile + anchors + this request (deterministic, like every
    /// calculator). `None` → no plan surfaced.
    plan_request: Option<PlanRequest>,
    /// The shell's clock, entering as event data: the current
    /// epoch-day, sent on foreground so `view()` can date the week and pick the
    /// next session without reading a clock. `None`
    /// → the plan anchors to the request's `start_epoch_day`.
    today_epoch_day: Option<i64>,
    /// The shell's UTC offset (seconds east of UTC) that accompanied the last
    /// `SetToday`, used to bucket UTC `observed_at` timestamps into the shell's
    /// LOCAL calendar day for session-done matching. 0 = UTC (default).
    today_utc_offset_sec: i64,
}

/// Inputs for a synthesized training plan: the epoch-day the user
/// accepted the plan. Retained so the `Program` is re-derived in `view()`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PlanRequest {
    start_epoch_day: i64,
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

/// Which logged history family a [`Event::DeleteEntry`] targets. Selects the
/// model vec (`sets` vs `runs`) and the compaction family (1 vs 2).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Set,
    Run,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Event {
    /// Record one readiness observation, then recompute adjustments.
    SubmitReadiness(ReadinessInput),
    /// Drop all accumulated inputs (new day / new session).
    ClearReadiness,
    /// Undo for one accidental report: drop the most recent input carrying
    /// `signal` (e.g. a mis-tapped Pain red flag), leaving unrelated inputs
    /// intact. No-op when no such input exists.
    RemoveReadiness { signal: ReadinessSignal },
    /// Record one morning check-in (raw human observations). Appended to the
    /// RETAINED check-in history the core normalizes into z-scores/deltas;
    /// unlike `SubmitReadiness`, this is not day-cleared.
    SubmitCheckin(CheckinInput),
    /// Drop the entire check-in history (part of "Clear all data"). Not tied to
    /// `ClearReadiness`, the two stores have different lifecycles.
    ClearCheckins,
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
        /// Stable per-entry id: a shell-assigned monotonic value
        /// (epoch-millis at log) so `AmendSet`/`DeleteEntry` can target this exact
        /// set. `#[serde(default)]` → 0 for legacy logs (matched on
        /// `observed_at` instead).
        #[serde(default)]
        entry_id: u64,
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
        /// Stable per-entry id; see [`Event::LogSet`]. 0 = legacy.
        #[serde(default)]
        entry_id: u64,
        /// User-declared run-intent label. USER DATA: no evidence, no
        /// coaching consumes it (HARD RULE 1). `#[serde(default)]` → `None` so
        /// old logs / shells that never sent it replay unchanged (back-compat).
        #[serde(default)]
        workout_type: Option<WorkoutType>,
    },
    /// Log one GPS-tracked run. Distance and duration are derived in-core from
    /// the fix track (haversine + time span); `hr_pct_max` comes from a paired
    /// HR sensor (0.0 when none), `longest_recent_km` drives the spike gate.
    LogRunTrack {
        points: Vec<GpsPoint>,
        hr_pct_max: f64,
        longest_recent_km: f64,
        /// Indices into `points` that begin a new recording segment: the shell's
        /// pause/relocation boundaries. `#[serde(default)]` → empty for
        /// old logs and shells that never sent it (they pre-collapsed the geometry
        /// by re-anchoring), which replay as one continuous segment, unchanged.
        #[serde(default)]
        segment_starts: Vec<u32>,
        /// Log time, unix seconds; shell-supplied, `#[serde(default)]` for
        /// back-compat with pre-timestamp persisted events (decode as 0). The
        /// GPS fixes carry their own per-point `observed_at`; this is the
        /// session's logged-at stamp for history display.
        #[serde(default)]
        observed_at: i64,
        /// Stable per-entry id; see [`Event::LogSet`]. 0 = legacy.
        #[serde(default)]
        entry_id: u64,
        /// User-declared run-intent label. USER DATA: no evidence, no
        /// coaching consumes it (HARD RULE 1). `#[serde(default)]` → `None` so
        /// old logs / shells that never sent it replay unchanged (back-compat).
        #[serde(default)]
        workout_type: Option<WorkoutType>,
    },
    /// Drop all logged runs.
    ClearRuns,
    /// Delete one logged set or run. Targets the newest entry
    /// whose `entry_id` matches (or, for a legacy row with no id, whose
    /// `observed_at` matches `observed_at_fallback`). A no-op when nothing
    /// matches. Compaction cancels it against its matched log line (Rule 3).
    DeleteEntry {
        kind: EntryKind,
        entry_id: u64,
        /// Match target for a legacy (`entry_id == 0`) row that predates entry
        /// ids. `#[serde(default)]` → 0.
        #[serde(default)]
        observed_at_fallback: i64,
    },
    /// Edit one logged set's fields: delete the matched set and
    /// push the replacement carrying the SAME `entry_id`, so the derivation in
    /// `view()` (e1RM chain, ordering) is untouched. Matches like `DeleteEntry`.
    AmendSet {
        entry_id: u64,
        exercise: String,
        weight_kg: f64,
        reps: u32,
        rpe: f64,
        /// The NEW timestamp for the amended entry (may differ from the original
        /// when the user re-dates a set).
        #[serde(default)]
        observed_at: i64,
        /// The OLD `observed_at` of the row being amended. For a legacy row
        /// (`entry_id == 0`) whose date the user CHANGED, the original entry can
        /// only be located by its old timestamp; matching on the new one would
        /// find nothing and push a DUPLICATE. `#[serde(default)]` → 0 (old logs;
        /// the handler falls back to `observed_at` then).
        #[serde(default)]
        observed_at_fallback: i64,
    },
    /// Edit one hand-entered run's fields. GPS-tracked runs are
    /// delete-only in this phase (their measured track is not field-editable), so
    /// amending one replaces it with a manual run. Matches like `DeleteEntry`.
    AmendRun {
        entry_id: u64,
        distance_km: f64,
        duration_min: f64,
        hr_pct_max: f64,
        #[serde(default)]
        longest_recent_km: f64,
        /// The NEW timestamp for the amended entry.
        #[serde(default)]
        observed_at: i64,
        /// The OLD `observed_at` of the row being amended; see [`Event::AmendSet`].
        #[serde(default)]
        observed_at_fallback: i64,
        /// User-declared run-intent label. USER DATA: no evidence, no
        /// coaching consumes it (HARD RULE 1). `#[serde(default)]` → `None` so
        /// old logs / shells that never sent it replay unchanged (back-compat).
        #[serde(default)]
        workout_type: Option<WorkoutType>,
    },
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
    /// Accept a synthesized training plan: the user tapped "Plan
    /// my training". `start_epoch_day` anchors the block; the `Program` is
    /// re-derived in `view()` from profile + logged anchors (determinism).
    GeneratePlan { start_epoch_day: i64 },
    /// Drop the plan (Coach returns to no-plan state).
    ClearPlan,
    /// The shell's clock as event data: today's LOCAL epoch-day, sent
    /// on foreground so `view()` dates the week + picks the next session with no
    /// clock in-core. Last-write-wins singleton (compaction keeps one line).
    ///
    /// Local/UTC day convention: `epoch_day` is the shell's LOCAL calendar
    /// day (days since 1970-01-01 in the device's timezone). `utc_offset_sec` is
    /// the device's current offset east of UTC in seconds (e.g. Berlin summer =
    /// +7200); the core buckets a run/set's UTC `observed_at` into the SAME local
    /// day via `(observed_at + utc_offset_sec).div_euclid(86_400)`, so "was this
    /// planned session done today?" compares like-for-like. `#[serde(default)]`
    /// → 0 (old logs / a shell that only knows UTC fall back to UTC bucketing,
    /// unchanged behavior).
    SetToday {
        epoch_day: i64,
        #[serde(default)]
        utc_offset_sec: i64,
    },
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
    /// The three-part "why?" disclosure (basis → why this grade → what would
    /// improve it). Serde-default so old shells ignore it and old logs replay.
    #[serde(default)]
    pub why: WhyView,
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
    /// Stable per-entry id echoed for the shell's edit/delete affordance (Phase
    /// 4): the shell sends it back in `AmendSet`/`DeleteEntry`. 0 for a legacy
    /// row (the shell then falls back to `observed_at`). Serde-default so old
    /// shells ignore it and old logs replay.
    #[serde(default)]
    pub entry_id: u64,
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
    /// Raw calculator inputs echoed back so shells rehydrate their forms after a
    /// log replay / process death, keeping the core the single source of truth.
    /// Each is a sibling of its result field above; `None`/empty until queried.
    /// (`race_prediction` echoes its inputs inline; these cover the Vec-result
    /// calculators.) Additive with serde defaults so old logs replay.
    #[serde(default)]
    pub hr_zone_input: Option<HrZoneInputView>,
    #[serde(default)]
    pub protein_input: Option<ProteinInputView>,
    #[serde(default)]
    pub hypertrophy_input: Option<HypertrophyInputView>,
    #[serde(default)]
    pub cooper_input: Option<f64>,
    #[serde(default)]
    pub critical_speed_input: Vec<CsEffortIn>,
    #[serde(default)]
    pub apre_input: Option<ApreInputView>,
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
    /// KB-honest per-signal readiness summary: the latest state of every
    /// observed readiness signal, judged by the SAME File 06/08 thresholds the
    /// adjustment rules use, each citing the rule's evidence. Deliberately NO
    /// composite 0–100 readiness score, the KB defines none (HARD RULE 1).
    /// Metrics first, then the red-flag block (see `signal_groups`).
    #[serde(default)]
    pub readiness_summary: Vec<ReadinessSignalView>,
    /// The single highest-priority call for today (usability-ia-spec §7):
    /// safety hold > deload/adjustment > session feedback > all-clear default.
    /// Prioritization is coaching logic, so it lives here, not in shells.
    #[serde(default)]
    pub today_headline: TodayHeadlineView,
    /// Static signal→group metadata for every readiness signal (in picker
    /// order, metrics before red flags), so the shell's red-flag fence in the
    /// readiness picker is data-driven from the core, not a shell predicate.
    #[serde(default)]
    pub signal_groups: Vec<SignalGroupView>,
    /// The most recent morning check-in, echoed back so the shell can rehydrate
    /// the check-in sheet and show that today's check-in is recorded. `None`
    /// until any check-in exists. Additive/serde-default.
    #[serde(default)]
    pub checkin_today: Option<CheckinEchoView>,
    /// Honest "collecting your baseline" status for each check-in channel that
    /// has readings but not yet enough history to emit a z-score/delta, so the
    /// shell shows honesty, never a fabricated number (HARD RULE 1). Empty once
    /// every channel is either baseline-ready (its derived signal appears in
    /// `readiness_summary`) or has no data.
    #[serde(default)]
    pub baseline_status: Vec<BaselineStatusView>,
    /// The evidence-grade legend, core-provided so the "How evidence grading
    /// works" sheet renders the File 09 definitions from core data rather than
    /// hardcoded shell copy. Static; always populated.
    #[serde(default)]
    pub grade_definitions: Vec<GradeDefView>,
    /// Today's concrete next session: the hero of the inverted Coach.
    /// `None` until the user accepts a plan. Rendered strictly downstream of
    /// the safety gates: a `train_blocked` hold sets `status = "blocked"` and
    /// empties its items (HARD RULE 3).
    #[serde(default)]
    pub next_session: Option<SessionPlanView>,
    /// The seven days of the current week around today, for the week strip.
    #[serde(default)]
    pub week_plan: Vec<SessionPlanView>,
    /// The active program summary card (name/goal/phase/week), when a plan is set.
    #[serde(default)]
    pub program: Option<ProgramSummaryView>,
    /// #6: structured HRmax figure (bpm + measured/estimate + Tanaka split) for
    /// the last HR-zone query, so the shell stops regex-scraping `hr_zones`.
    /// `None` until an HR-zone query is made (or the age is out of range).
    #[serde(default)]
    pub hr_max: Option<HrMaxView>,
    /// #6: structured protein g/day figures paralleling `protein_targets`, so the
    /// shell stops regex-scraping those rows. Empty until a protein query is made.
    #[serde(default)]
    pub protein_figures: Vec<ProteinFigureView>,
}

/// The most recent check-in echoed for rehydration + a "checked in today" cue
/// (app.rs `checkin_today`). Every field serde-default so old shells/cores stay
/// compatible.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct CheckinEchoView {
    pub observed_at: i64,
    #[serde(default)]
    pub sleep_quality: Option<u8>,
    #[serde(default)]
    pub soreness: Option<u8>,
    #[serde(default)]
    pub mood: Option<u8>,
    #[serde(default)]
    pub resting_hr_bpm: Option<f64>,
    #[serde(default)]
    pub hrv_rmssd_ms: Option<f64>,
}

/// One check-in channel still collecting its baseline (app.rs `baseline_status`):
/// an honest progress row shown in place of a derived signal until enough
/// history exists. Carries NO evidence tag: it asserts no training claim, it
/// states the absence of one (HARD RULE 2 is about recommendations).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct BaselineStatusView {
    /// Wire name of the signal this baseline feeds, e.g. `"WellnessZ"`.
    pub signal: String,
    /// Friendly channel label, e.g. `"Sleep, soreness & mood"`.
    pub label: String,
    /// Check-in days collected so far.
    pub have: u32,
    /// Days needed before a baseline is emitted.
    pub need: u32,
    /// Honest one-line status, e.g. `"Collecting your baseline - 4 of 7 check-ins"`.
    pub note: String,
}

/// One readiness signal's latest observation, flattened for shells: the raw
/// value plus a qualitative state string decided by the same KB threshold that
/// drives the matching adjustment rule (autoreg.rs `signal_states`). The
/// evidence fields cite that rule; they are empty (grade `""`) for the plain
/// factual rows ("recorded"/"clear") that judge nothing.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ReadinessSignalView {
    /// Wire name of the signal, e.g. `"HrvLnRmssd"` (serde unit-variant name).
    pub signal: String,
    /// `"metric"` or `"red_flag"` (medical-referral / hard-stop block).
    pub group: String,
    pub value: f64,
    pub streak: u8,
    /// Qualitative state, e.g. `"suppressed"`, `"elevated 2+ days"`.
    pub state: String,
    /// Display-only sub-line context, e.g. a pain report's
    /// `"Left knee · sharp/joint · 6/10"`. Empty (serde default, so old logs
    /// and pre-field shells stay compatible) for signals without such detail.
    #[serde(default)]
    pub detail: String,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
}

/// The core-owned "today's call" headline (usability-ia-spec §7 PROPOSED item,
/// now shipped): one field carrying the highest-priority call for today so no
/// shell re-implements the ranking. `kind` is the priority bucket that won:
/// `"safety_hold"` > `"adjustment"` > `"feedback"` > `"all_clear"`. The
/// all-clear default states the ABSENCE of any triggered rule: it makes no
/// evidence-bearing claim, so its evidence fields are empty.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct TodayHeadlineView {
    pub kind: String,
    pub summary: String,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
    /// The three-part "why?" disclosure (basis → why this grade → what would
    /// improve it). Serde-default so old shells ignore it and old logs replay.
    #[serde(default)]
    pub why: WhyView,
}

/// The three-part "why?" disclosure carried by every action-bearing card:
/// `basis` (the rule/method and the user datum behind the call), `grade_note`
/// (a one-sentence gloss of this claim's evidence grade, contested question
/// appended when relevant), and `improves` (what data would sharpen the call).
/// `basis` and `improves` describe the engine's own reasoning and data needs,
/// never new training advice, so they invent no claim (HARD RULE 1). All fields
/// serde-default, so old shells ignore the block and old logs replay.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct WhyView {
    pub basis: String,
    pub grade_note: String,
    pub improves: String,
}

/// One prescribed exercise, flattened for shells.
/// The concrete "do X" contract: sets/reps + either a load (from the user's
/// logged e1RM) or a proximity-to-failure target when no anchor exists, plus the
/// same evidence block every action-bearing view carries (HARD RULE 2). Every
/// field serde-default so old shells ignore it and a new shell on an old core
/// renders nothing.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct PrescriptionView {
    /// The full human line, e.g. `"Back Squat - 5×3 @ 85 kg · 85% e1RM"`.
    pub summary: String,
    pub exercise: String,
    pub sets: u8,
    pub reps_low: u8,
    pub reps_high: u8,
    /// Working load in kg when anchored to a logged e1RM; `None` → the
    /// `intensity_label` carries the RIR/RPE/pace target instead (no invented
    /// load, HARD RULE 1).
    #[serde(default)]
    pub load_kg: Option<f64>,
    /// Intensity target, e.g. `"85% e1RM"` | `"RIR 3"` | `"90% HRmax"`.
    pub intensity_label: String,
    /// Rest between sets, seconds (0 for runs).
    #[serde(default)]
    pub rest_sec: u16,
    /// For an interval/repetition run, the rep count. `0` for a continuous
    /// run or a lift: the `summary` then carries the whole-session volume. When
    /// nonzero the `summary` already reads as "N × <rep_volume> · <pace>".
    #[serde(default)]
    pub rep_count: u8,
    /// For an interval/repetition run, the per-rep volume label, e.g. `"4 min"`
    /// or `"800 m"`. Empty for a continuous run or a lift.
    #[serde(default)]
    pub rep_volume: String,
    /// The honesty line for a load, e.g. `"e1RM 120.0 kg (your logged best)"`.
    /// Empty when RIR-prescribed or a run.
    #[serde(default)]
    pub anchored_on: String,
    /// Set when a readiness adjustment modified this item, e.g.
    /// `"load −10% - readiness (AUTOREG-RIR-001)"`. Empty otherwise.
    #[serde(default)]
    pub adjusted_note: String,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
    #[serde(default)]
    pub why: WhyView,
}

/// One planned day in the week, flattened for shells.
/// The plan renders strictly downstream of the safety gates: a `train_blocked`
/// hold empties `items` and sets `status = "blocked"` (HARD RULE 3).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct SessionPlanView {
    /// Calendar day of this session, unix epoch-day (days since 1970-01-01).
    pub epoch_day: i64,
    /// Human title, e.g. `"Heavy day"`, `"Long run"`, `"Rest"`.
    pub title: String,
    /// Wire name of the `SessionType`, e.g. `"Lift(MaxEffort)"`, `"Rest"`.
    pub session_type: String,
    /// `"next" | "planned" | "done" | "missed" | "adjusted" | "blocked" | "rest"`.
    pub status: String,
    pub items: Vec<PrescriptionView>,
    /// The readiness adjustment folded into this session, when `status ==
    /// "adjusted"`. Carries the adjustment's own evidence.
    #[serde(default)]
    pub adjustment: Option<AdjustmentView>,
}

/// The program summary card: what the plan IS, plus its
/// representative evidence chip. Serde-default throughout.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ProgramSummaryView {
    pub name: String,
    /// Human goal, e.g. `"Strength"`, `"Hybrid"`, `"10 km race"`.
    pub goal: String,
    /// Phase name, e.g. `"Build"`.
    pub phase: String,
    pub week: u8,
    pub weeks_total: u8,
    /// Once the athlete passes the last block week, the plan is a repeated
    /// maintenance cycle rather than fresh progression (the synthesizer emits a
    /// single static block; plan.rs owns real progression/deload/taper). `week`
    /// then cycles 1..weeks_total instead of pinning at the last week, and this
    /// flag is set so a shell can say "maintenance" honestly. `false` while the
    /// original block is still running.
    #[serde(default)]
    pub maintenance: bool,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
    #[serde(default)]
    pub why: WhyView,
}

/// One row of the evidence-grade legend ("How evidence grading works"),
/// exported from core data so the shell renders the KB definitions rather than
/// hardcoding them. Carries no per-claim evidence;
/// it defines the grading scale itself (File 09).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct GradeDefView {
    /// Wire name matching the grade strings on every card, e.g. `"Strong"`.
    pub grade: String,
    /// Friendly label, e.g. `"Expert opinion"`.
    pub label: String,
    /// One-sentence KB definition of the grade (File 09 scale).
    pub definition: String,
    /// Default confidence this grade maps to (File 09), e.g. `0.90`.
    pub confidence: f32,
}

/// Signal→group row for the static readiness-picker metadata (`signal_groups`).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct SignalGroupView {
    pub signal: String,
    /// `"metric"` or `"red_flag"`.
    pub group: String,
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
    /// (running-040). Empty when nothing applies.
    #[serde(default)]
    pub notes: Vec<GuidanceView>,
    /// The raw inputs this prediction was computed from, echoed back so a shell
    /// can rehydrate its form after a log replay instead of falling back to a
    /// hardcoded default (single source of truth stays the core). Additive with
    /// serde defaults so pre-echo logs still replay.
    #[serde(default)]
    pub recent_distance_m: f64,
    #[serde(default)]
    pub recent_time_sec: f64,
    #[serde(default)]
    pub goal_distance_m: f64,
    #[serde(default)]
    pub weekly_km: f64,
    #[serde(default)]
    pub weeks_since_race: Option<u8>,
}

/// The raw heart-rate-zone query echoed back so a shell rehydrates its form
/// (app.rs `hr_zone_input`). Sibling to the `hr_zones` result rows.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct HrZoneInputView {
    pub age_years: f64,
    #[serde(default)]
    pub resting_hr_bpm: Option<f64>,
    #[serde(default)]
    pub weeks_since_recalc: Option<u8>,
    #[serde(default)]
    pub weeks_since_pace_test: Option<u8>,
}

/// The raw protein query echoed back so a shell rehydrates its form (app.rs
/// `protein_input`). Sibling to the `protein_targets` result rows.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ProteinInputView {
    pub bodyweight_kg: f64,
    pub masters: bool,
    pub deficit: bool,
}

/// #6: structured form of the HRmax figure the shell otherwise regex-scraped out
/// of the `hr_zones` summary rows (incl. the "208 − 0.7 × age" Tanaka split). All
/// additive / serde-default so the wire stays backward compatible.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct HrMaxView {
    /// Resolved max heart rate, bpm, the measured value when available, else the
    /// age-based Tanaka estimate. Drives the %HRmax band targets in `hr_zones`.
    pub bpm: f64,
    /// `true` when `bpm` is the user's logged measured maximum; `false` when it
    /// is the Tanaka estimate (the `tanaka_*` fields then describe it).
    pub measured: bool,
    /// Age used for the estimate. `0.0` when a measured max bypassed age entirely.
    pub age_years: f64,
    /// Tanaka intercept (208) so the shell renders `208 − 0.7 × age` from data,
    /// not by paren-scraping the summary. `0.0` when `measured`.
    pub tanaka_intercept: f64,
    /// Tanaka slope (0.7). `0.0` when `measured`.
    pub tanaka_slope: f64,
}

/// #6: structured form of a protein target the shell otherwise regex-scraped out
/// of the `protein_targets` summary rows. One per emitted row (masters and/or
/// deficit). Additive / serde-default.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ProteinFigureView {
    /// Which target this is: `"masters"` or `"deficit"`.
    pub kind: String,
    /// Low end of the daily target, grams. `0.0` when `refused`.
    pub low_g_per_day: f64,
    /// High end of the daily target, grams. `0.0` when `refused`.
    pub high_g_per_day: f64,
    /// `true` when a deficit target was refused (RED-S / low-energy-availability
    /// signal present, safety-022), no g/day figure is offered.
    pub refused: bool,
}

/// The raw hypertrophy-plan query echoed back so a shell rehydrates its form
/// (app.rs `hypertrophy_input`). Sibling to the `hypertrophy_plan` result rows.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct HypertrophyInputView {
    pub muscle: String,
    pub weeks: u8,
    pub not_growing: bool,
    pub recovering_easily: bool,
}

/// The raw APRE query echoed back so a shell rehydrates its form (app.rs
/// `apre_input`). Sibling to the `apre` result rows.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ApreInputView {
    pub scheme: autoreg::ApreScheme,
    pub reps: u8,
    pub current_load_lb: f64,
}

/// The one resolved coaching message for a session, flattened for shells.
/// Safety concerns short-circuit and suppress competing praise (HARD RULE 3).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct FeedbackView {
    /// Category name, e.g. `"ConcernInjury"`.
    pub category: String,
    /// Human overline for the card face; render verbatim.
    pub category_label: String,
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
    /// The three-part "why?" disclosure (basis → why this grade → what would
    /// improve it). Serde-default so old shells ignore it and old logs replay.
    #[serde(default)]
    pub why: WhyView,
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
    /// The three-part "why?" disclosure (basis → why this grade → what would
    /// improve it). Serde-default so old shells ignore it and old logs replay.
    #[serde(default)]
    pub why: WhyView,
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

/// Core-owned interval-vs-steady verdict for a GPS run (RUN-INTERVAL-VI-001).
/// Built from the track's variability index (normalized speed ÷ average speed);
/// lets the shell render why two runs of the *same average pace* rate
/// differently, with the same evidence chrome as any other recommendation.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct IntervalVerdictView {
    /// `"interval"` (variability index ≥ threshold) or `"steady"`. Wire string
    /// the shell matches on, never re-derives the threshold.
    pub kind: String,
    /// Short chip label, e.g. `"INTERVAL · VI 1.7"` / `"STEADY · VI 1.0"`.
    pub label: String,
    /// Evidence-backed copy explaining the differentiation honestly.
    pub message: String,
    /// The variability index itself (normalized speed ÷ average speed), 2 dp.
    pub variability_index: f64,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
}

/// One per-unit (per-km or per-mile) split of a GPS run, flattened for shells.
/// Purely descriptive, a measurement of the run, not a recommendation, so it
/// carries no evidence chrome. Built from [`running::track_splits`]; the core
/// owns the pace formatting so a shell renders `pace` verbatim.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RunSplitView {
    /// 1-based split index (1 = first km/mile).
    pub index: u32,
    /// Pace over this split as `m:ss` (seconds per km / per mile; the final
    /// partial split's pace is normalized to a full unit so it stays comparable).
    pub pace: String,
    /// Cumulative track distance at the END of this split, in km (both the km
    /// and mile lists express the running total in km).
    pub distance_km: f64,
    /// True only for a final split shorter than a full unit.
    pub partial: bool,
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
    /// Duration in minutes (moving time for a GPS run). Echoed so the shell can
    /// prefill the run editor for a manual-run amend. 0.0 when
    /// unmeasurable. Serde-default so old shells ignore it.
    #[serde(default)]
    pub duration_min: f64,
    /// The run's average % HRmax as logged (0.0 = no HR sample). Echoed for the
    /// manual-run edit prefill. Serde-default.
    #[serde(default)]
    pub hr_pct_max: f64,
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
    /// Core-owned interval-vs-steady verdict from the track's variability index
    /// (RUN-INTERVAL-VI-001). `None` for a hand-entered run or a track too short
    /// to derive a variability index. Serde-default so old shells ignore it.
    #[serde(default)]
    pub interval: Option<IntervalVerdictView>,
    /// User-declared run-intent label echoed for history display. USER
    /// DATA: no evidence, no coaching branches on it (HARD RULE 1). `None` =
    /// untagged. Serde-default so old shells ignore it and old views decode.
    #[serde(default)]
    pub workout_type: Option<WorkoutType>,
    /// One-line run recap. For a MEASURED run this is internal/citation-only -
    /// never rendered; the shell builds unit-aware labels (`runDistLabel` /
    /// `runPaceLabel`), so the hardcoded `km` here never reaches a mi-preferring
    /// user. Only the zero-distance fallback ("GPS signal too poor to measure
    /// this run", no unit) is surfaced, in `RunCard`.
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
    /// Stable per-entry id echoed for the shell's edit/delete affordance (Phase
    /// 4); see [`LiftResultView::entry_id`]. 0 for a legacy row.
    #[serde(default)]
    pub entry_id: u64,
    /// Per-kilometre splits for a GPS run (one entry per completed km + a final
    /// partial), derived core-side from the same QC-gated track as
    /// distance/pace. Empty for a hand-entered run or a track too short to split.
    /// Serde-default so old shells ignore it and old logs replay.
    #[serde(default)]
    pub splits_km: Vec<RunSplitView>,
    /// Per-mile splits for a GPS run (same shape and track as [`splits_km`],
    /// unit = one international mile). Empty for a hand-entered run.
    #[serde(default)]
    pub splits_mi: Vec<RunSplitView>,
    /// #6: structured form of the spike-baseline provenance the shell otherwise
    /// scraped from `spike_note` (`contains("no prior run")`). `true` when a
    /// prior 30-day baseline distance exists to gauge this run against; `false`
    /// for a first run with no baseline (the spike gate then errs safe). Only
    /// meaningful when `spike_flag`. Serde-default so old shells/logs are fine.
    #[serde(default)]
    pub spike_has_baseline: bool,
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
            Event::RemoveReadiness { signal } => {
                if let Some(pos) = model.inputs.iter().rposition(|i| i.signal == signal) {
                    model.inputs.remove(pos);
                }
            }
            Event::SubmitCheckin(input) => model.checkins.push(input),
            Event::ClearCheckins => model.checkins.clear(),
            Event::LogSet {
                exercise,
                weight_kg,
                reps,
                rpe,
                observed_at,
                entry_id,
            } => model.sets.push(LoggedSet {
                exercise,
                weight_kg: sanitize_f64(weight_kg),
                reps,
                rpe: sanitize_f64(rpe),
                observed_at,
                entry_id,
            }),
            Event::ClearSets => model.sets.clear(),
            Event::LogRun {
                distance_km,
                duration_min,
                hr_pct_max,
                longest_recent_km,
                observed_at,
                entry_id,
                workout_type,
            } => {
                // Floor the spike baseline to the longest run in the trailing
                // 30-day window (not all-time), so a manual entry gets the same
                // RUN-SPIKE-001 gate a GPS-tracked one does; an explicit caller
                // value (paired 30-day history) still wins when larger.
                let prior_longest = spike_baseline_km(&model.runs, observed_at);
                model.runs.push(LoggedRun {
                    // A negative distance would subtract from `recent_weekly_km`
                    // (nulling the measured plan anchor): clamp it out at ingest.
                    distance_km: sanitize_f64(distance_km).max(0.0),
                    duration_min: sanitize_f64(duration_min).max(0.0),
                    hr_pct_max: sanitize_f64(hr_pct_max),
                    longest_recent_km: sanitize_f64(longest_recent_km).max(prior_longest),
                    track: Vec::new(),
                    track_segment_starts: Vec::new(),
                    observed_at,
                    entry_id,
                    workout_type,
                });
            }
            Event::LogRunTrack {
                points,
                hr_pct_max,
                longest_recent_km,
                observed_at,
                entry_id,
                workout_type,
                segment_starts,
            } => {
                // Spike baseline = longest run in the trailing 30-day window,
                // so the gate works without the shell fabricating a recent-longest
                // figure. An explicit caller value (paired history from a tracker)
                // still wins when larger.
                let prior_longest = spike_baseline_km(&model.runs, observed_at);
                let track: Vec<GpsPoint> = points
                    .into_iter()
                    .map(|p| GpsPoint {
                        lat: sanitize_f64(p.lat),
                        lon: sanitize_f64(p.lon),
                        observed_at: p.observed_at,
                        accuracy_m: if p.accuracy_m.is_finite() {
                            p.accuracy_m.clamp(-1.0e12, 1.0e12)
                        } else {
                            0.0
                        },
                    })
                    .collect();
                // Boundary indices are stored raw; the core's `segments()` ignores
                // any that are 0, out-of-range, or duplicated, and the fix mapping
                // above preserves length + order so the indices still align.
                model.runs.push(LoggedRun {
                    distance_km: 0.0,
                    duration_min: 0.0,
                    hr_pct_max: sanitize_f64(hr_pct_max),
                    longest_recent_km: sanitize_f64(longest_recent_km).max(prior_longest),
                    track,
                    track_segment_starts: segment_starts,
                    observed_at,
                    entry_id,
                    workout_type,
                });
            }
            Event::ClearRuns => model.runs.clear(),
            // Delete/amend of a logged set or run. Both target the
            // NEWEST matching row (id first, else `observed_at` for a legacy row)
            // mirroring the compaction Rule 3 matcher exactly. Amend = remove +
            // push carrying the SAME id, so `view()`'s derivation stays untouched.
            Event::DeleteEntry {
                kind: EntryKind::Set,
                entry_id,
                observed_at_fallback,
            } => {
                if let Some(pos) = find_set(&model.sets, entry_id, observed_at_fallback) {
                    model.sets.remove(pos);
                }
            }
            Event::DeleteEntry {
                kind: EntryKind::Run,
                entry_id,
                observed_at_fallback,
            } => {
                if let Some(pos) = find_run(&model.runs, entry_id, observed_at_fallback) {
                    model.runs.remove(pos);
                }
            }
            Event::AmendSet {
                entry_id,
                exercise,
                weight_kg,
                reps,
                rpe,
                observed_at,
                observed_at_fallback,
            } => {
                // Locate the row to replace by its OLD identity: the id
                // (nonzero), or a legacy row's OLD timestamp (`observed_at_fallback`,
                // else the new `observed_at` for old logs that predate this field).
                // Matching the OLD row is what fixes the duplicate: a re-dated
                // legacy row is now FOUND (and removed) before the replacement is
                // pushed, instead of the removal silently missing on the new date.
                // Full prevention: this is a STRICT update; the push happens
                // ONLY when a matching row is found. An amend that matches nothing
                // (e.g. its target was already deleted) is a NO-OP, so a
                // Log→Delete→Amend sequence can no longer RESURRECT the deleted
                // row. Compaction Rule 3 mirrors this: a surviving amend always
                // keeps its base log line, so a compacted edit still replays.
                let match_at = if observed_at_fallback != 0 {
                    observed_at_fallback
                } else {
                    observed_at
                };
                if let Some(pos) = find_set(&model.sets, entry_id, match_at) {
                    model.sets.remove(pos);
                    model.sets.push(LoggedSet {
                        exercise,
                        weight_kg: sanitize_f64(weight_kg),
                        reps,
                        rpe: sanitize_f64(rpe),
                        observed_at,
                        entry_id,
                    });
                }
            }
            Event::AmendRun {
                entry_id,
                distance_km,
                duration_min,
                hr_pct_max,
                longest_recent_km,
                observed_at,
                observed_at_fallback,
                workout_type,
            } => {
                // Match the OLD identity (see `AmendSet`) so a re-dated legacy
                // run is replaced, not duplicated. Full prevention: STRICT update;
                // the push happens ONLY when a matching row is found, so an amend
                // whose target was already deleted is a no-op (no resurrection).
                let match_at = if observed_at_fallback != 0 {
                    observed_at_fallback
                } else {
                    observed_at
                };
                if let Some(pos) = find_run(&model.runs, entry_id, match_at) {
                    model.runs.remove(pos);
                    // Spike-baseline flooring over the trailing 30-day window
                    // (the amended run is a fresh manual entry); an explicit caller
                    // value still wins when larger.
                    let prior_longest = spike_baseline_km(&model.runs, observed_at);
                    model.runs.push(LoggedRun {
                        // Clamp a negative distance out (see `LogRun`).
                        distance_km: sanitize_f64(distance_km).max(0.0),
                        duration_min: sanitize_f64(duration_min).max(0.0),
                        hr_pct_max: sanitize_f64(hr_pct_max),
                        longest_recent_km: sanitize_f64(longest_recent_km).max(prior_longest),
                        track: Vec::new(),
                        track_segment_starts: Vec::new(),
                        observed_at,
                        entry_id,
                        workout_type,
                    });
                }
            }
            Event::SetProfile(mut profile) => {
                // Sanitize every wire float BEFORE any branch reads it,
                // so a poisoned `NaN`/`1e300` can't render "inf km" or slip past a
                // gate. `running_km_per_week` also floors at 0 (a negative weekly
                // volume is nonsense and would break the plan's volume math).
                profile.running_km_per_week = sanitize_f64(profile.running_km_per_week).max(0.0);
                profile.endurance_intensity_pct_vo2max =
                    sanitize_f64(profile.endurance_intensity_pct_vo2max);
                for f in [
                    &mut profile.env_temp_c,
                    &mut profile.env_altitude_m,
                    &mut profile.weeks_off,
                    &mut profile.bodyweight_kg,
                    &mut profile.age_years,
                    &mut profile.resting_hr_bpm,
                    &mut profile.measured_hr_max,
                ] {
                    if let Some(v) = f {
                        *v = sanitize_f64(*v);
                    }
                }
                // HARD RULE 3: auto-derive the pediatric gate from age alone
                // so it fires even when the health screen is skipped. The KB gives
                // no numeric cutoff for "child/adolescent" (safety-011); 18 y is
                // used. Never CLEARS the flag: a shell that set it explicitly (or
                // a user who declined to give an age) keeps the gate. Runs AFTER
                // the sanitize above, so a `NaN` age becomes 0 → gate fires
                // (conservative), not a silently-skipped `NaN < 18.0` (false).
                if profile.age_years.is_some_and(|a| a < 18.0) {
                    profile.health.youth = true;
                }
                model.profile = Some(profile);
            }
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
            Event::GeneratePlan { start_epoch_day } => {
                // Preserve an existing anchor: re-firing GeneratePlan (e.g. the
                // shell auto-generates on every launch so a set-up user always has
                // a plan) must NOT re-date the plan to today. Re-anchoring would
                // pin the week strip at "week 1" forever (weeks_elapsed = today −
                // start) and flip already-logged done/missed days (epoch_day <
                // start) back to a neutral "planned" on every relaunch. Keep the
                // ORIGINAL start; only ClearPlan → GeneratePlan makes a new anchor.
                let anchor = model
                    .plan_request
                    .as_ref()
                    .map(|p| p.start_epoch_day)
                    .unwrap_or(start_epoch_day);
                model.plan_request = Some(PlanRequest {
                    start_epoch_day: anchor,
                });
            }
            Event::ClearPlan => model.plan_request = None,
            Event::SetToday {
                epoch_day,
                utc_offset_sec,
            } => {
                model.today_epoch_day = Some(epoch_day);
                model.today_utc_offset_sec = utc_offset_sec;
            }
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

        // Normalize the retained check-in history into the synthetic
        // readiness signals (WellnessZ / HrvLnRmssd / RestingHr) the autoreg rules
        // already consume, then MERGE with the manual/advanced inputs. One merge
        // point, zero rule changes: derivation only supplies inputs. Manual inputs
        // go last so `latest_input`'s recency tie-break lets an explicit advanced
        // entry win over a same-instant derived one.
        let derived = autoreg::derive_readiness(&model.checkins, model.today_utc_offset_sec);
        let mut readiness_inputs: Vec<ReadinessInput> = derived.inputs.clone();
        readiness_inputs.extend(model.inputs.iter().cloned());

        // A "felt easy" performance signal goes stale: a three-week-old
        // RPE −2 must not keep proposing IncreaseLoadPct today. Expire ONLY the
        // performance signals that can drive a load INCREASE (RPE / e1RM /
        // bar-velocity), and only when the shell has supplied "today" (no clock in
        // the core). This is deliberately one-directional: dropping a stale
        // increase is CONSERVATIVE, but the protective signals, velocity-loss
        // stops, aerobic-decoupling downgrades, wellness/HRV/RHR suppression, and
        // every pain / illness / RED-S / soreness flag is NEVER expired, since
        // dropping a protective signal is anti-conservative (HARD RULE 3). The
        // 14-day window is an engine heuristic (no KB parameter), documented as
        // such.
        if let Some(today) = model.today_epoch_day {
            let offset = model.today_utc_offset_sec;
            readiness_inputs.retain(|i| {
                if !is_expirable_perf_signal(i.signal) {
                    return true;
                }
                let local_day = i.observed_at.saturating_add(offset).div_euclid(DAY_SEC);
                today.saturating_sub(local_day) <= PERF_SIGNAL_EXPIRY_DAYS
            });
        }

        let recommended =
            autoreg::adjustments_with_context(&readiness_inputs, goal.as_ref(), high_load_block);

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
        let readiness_tier = autoreg::resolve_safety_for_goal(&readiness_inputs, goal.as_ref());
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

        let adjustments: Vec<AdjustmentView> = recommended.iter().map(to_view).collect();
        let feedback = model.review.as_ref().map(|r| {
            build_feedback(
                r,
                latest_track_split(model),
                latest_run_spike_frac(model),
                advanced_user,
                female_user,
            )
        });
        // Coach-as-planner: synthesize + date the week, then apply
        // readiness/safety INSIDE the rendered session (strictly downstream of the
        // gates; HARD RULE 3). The prescription becomes the top non-safety
        // headline rung.
        let (next_session, week_plan, program) =
            build_plan_views(model, train_blocked, &recommended, &review_recs);

        // #6: protein + HRmax calculators now return their prose rows AND a
        // structured figure, so the shell consumes numbers instead of scraping
        // the summary strings. Computed here so both halves land in the literal.
        let (protein_targets, protein_figures) = match model.protein_query.as_ref() {
            Some(q) => build_protein_targets(q, reds_present),
            None => (Vec::new(), Vec::new()),
        };
        let (hr_zones, hr_max) = match model.hr_zone_query.as_ref() {
            Some(q) => build_hr_zones(q, model.profile.as_ref().and_then(|p| p.measured_hr_max)),
            None => (Vec::new(), None),
        };
        let today_headline = build_headline(
            train_blocked,
            &gates,
            &recommended,
            &review_recs,
            &adjustments,
            &review_adjustments,
            feedback.as_ref(),
            next_session.as_ref(),
        );

        // History views in chronological (observed_at) order, stable for ties
        // and undated (0) legacy entries, so a backdated log slots into its
        // true position, the e1RM delta chain (lift_views) and the shells'
        // oldest→newest rendering follow log TIME, not submission order.
        let runs = {
            let mut ordered: Vec<&LoggedRun> = model.runs.iter().collect();
            ordered.sort_by_key(|r| r.observed_at);
            ordered.into_iter().map(to_run_view).collect()
        };

        ViewModel {
            safety_tier: safety_tier.map(|t| format!("{t:?}")),
            train_blocked,
            adjustments,
            review_adjustments,
            input_count: model.inputs.len(),
            lifts: lift_views(&model.sets),
            runs,
            guidance: model
                .profile
                .as_ref()
                .map(build_guidance)
                .unwrap_or_default(),
            feedback,
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
            protein_targets,
            hr_zones,
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
            hr_zone_input: model.hr_zone_query.as_ref().map(|q| HrZoneInputView {
                age_years: q.age_years,
                resting_hr_bpm: q.resting_hr_bpm,
                weeks_since_recalc: q.weeks_since_recalc,
                weeks_since_pace_test: q.weeks_since_pace_test,
            }),
            protein_input: model.protein_query.as_ref().map(|q| ProteinInputView {
                bodyweight_kg: q.bodyweight_kg,
                masters: q.masters,
                deficit: q.deficit,
            }),
            hypertrophy_input: model.hypertrophy_plan_query.as_ref().map(|q| {
                HypertrophyInputView {
                    muscle: q.muscle.clone(),
                    weeks: q.weeks,
                    not_growing: q.not_growing,
                    recovering_easily: q.recovering_easily,
                }
            }),
            cooper_input: model.cooper_query,
            critical_speed_input: model.cs_query.clone().unwrap_or_default(),
            apre_input: model.apre_query.as_ref().map(|q| ApreInputView {
                scheme: q.scheme,
                reps: q.reps,
                current_load_lb: q.current_load_lb,
            }),
            trend: model.review.as_ref().and_then(build_trend),
            provisional: build_provisional(model),
            autoreg_source: build_autoreg_source(&readiness_inputs),
            readiness_summary: build_readiness_summary(
                &readiness_inputs,
                goal.as_ref(),
                high_load_block,
            ),
            today_headline,
            signal_groups: build_signal_groups(),
            checkin_today: model
                .checkins
                .iter()
                .max_by_key(|c| c.observed_at)
                .map(to_checkin_echo),
            baseline_status: derived.collecting.iter().map(to_baseline_status_view).collect(),
            grade_definitions: grade_definitions(),
            next_session,
            week_plan,
            program,
            hr_max,
            protein_figures,
        }
    }
}

/// Echo the most recent check-in for shell rehydration.
fn to_checkin_echo(c: &CheckinInput) -> CheckinEchoView {
    CheckinEchoView {
        observed_at: c.observed_at,
        sleep_quality: c.sleep_quality,
        soreness: c.soreness,
        mood: c.mood,
        resting_hr_bpm: c.resting_hr_bpm,
        hrv_rmssd_ms: c.hrv_rmssd_ms,
    }
}

/// Human label + honest "collecting baseline" copy for one still-collecting
/// check-in channel. States progress only, no training claim (HARD RULE 1/2).
fn to_baseline_status_view(s: &autoreg::BaselineStatus) -> BaselineStatusView {
    let label = match s.signal {
        ReadinessSignal::WellnessZ => "Sleep, soreness & mood",
        ReadinessSignal::HrvLnRmssd => "HRV (rMSSD)",
        ReadinessSignal::RestingHr => "Resting HR",
        _ => "Readiness",
    };
    BaselineStatusView {
        signal: format!("{:?}", s.signal),
        label: label.to_string(),
        have: s.have as u32,
        need: s.need as u32,
        note: format!(
            "Collecting your baseline: {} of {} check-ins",
            s.have, s.need
        ),
    }
}

/// Profile-independent evidence-cited reference defaults, surfaced always so a
/// shell can show coaching rationale without a full profile set.
fn build_reference() -> Vec<GuidanceView> {
    // P2: the reference rows are profile-independent and byte-identical every
    // call, yet each rebuild re-runs the evidence-registry lookups behind ~30
    // `push_guidance` rows on every `view()`. Compute once, then hand back a
    // cheap clone. Deterministic (no clock/rand), a lazy constant, not state.
    static CACHE: OnceLock<Vec<GuidanceView>> = OnceLock::new();
    CACHE.get_or_init(build_reference_impl).clone()
}

fn build_reference_impl() -> Vec<GuidanceView> {
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
        "Schedule the highest-priority quality when freshest: start of the week or right after a rest day".to_string(),
        &sched,
    );

    // hybrid-019 / CAP-8: double-day fueling.
    let cho = hybrid::double_day_cho_refuel(true);
    push_guidance(
        &mut rows,
        "Hybrid",
        "Double (AM/PM) days: fully refuel carbohydrate between the endurance session and the lift. Low glycogen amplifies interference".to_string(),
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
        "Never progress high running volume and heavy lifting aggressively in the same week. The concurrent effect on tendon stiffness is unstudied; progress one, hold the other".to_string(),
        &dual,
    );

    // hybrid-024: energy-availability guard (higher-risk cohorts named).
    let ea = hybrid::energy_availability_guard(true, true, true);
    push_guidance(
        &mut rows,
        "Safety",
        "Keep energy availability adequate (RED-S/LEA guard), especially for high-volume endurance, leaner, and female athletes".to_string(),
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
            "Goals are framed as controllable process targets (cadence, pacing discipline, RIR). You steer the process, the outcome follows"
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
                "Pace at target HR has improved for 2+ weeks. Re-test and raise the threshold pace".to_string(),
                &retest,
            ));
        }
    }

    // running-034: ≥2 overtraining signals → insert an unscheduled down week.
    let down = running::unscheduled_deload(r.overtraining_signal_count);
    if down.value {
        out.push(to_view_with(
            "2+ overtraining signals: insert an unscheduled down week now".to_string(),
            &down,
        ));
    }

    // autoreg-008/009 (VBT): reference-load velocity delta → daily-1RM verdict.
    if let Some(delta) = r.mcv_delta_m_s {
        let v = autoreg::vbt_daily_readiness(delta);
        let text = match v.value {
            autoreg::VbtReadiness::IncreaseLoad => {
                "Bar speed up >0.06 m/s at the reference load: daily 1RM is up, raise working loads"
            }
            autoreg::VbtReadiness::Hold => {
                "Bar speed within the ±0.06 m/s reliability band: hold planned loads"
            }
            autoreg::VbtReadiness::ReduceLoad => {
                "Bar speed down >0.06 m/s at the reference load: daily 1RM is down, reduce working loads"
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
                "Strong first set at low cost with normal wellness: add a set today"
            }
            autoreg::SetVolumeAction::DropLastSet => {
                "First set short or over target RPE: drop the last planned set"
            }
            autoreg::SetVolumeAction::HoldPlanned => "Run the planned sets: no set-count change",
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
                "Target RPE reached before the planned rep count: stop the exercise here (RPE-stop)"
                    .to_string(),
                &graded((), "AUTOREG-RIR-001"),
            ));
        }
    }

    // autoreg-014: two consecutive sessions needing set cuts → hold volume.
    if autoreg::hold_volume_after_two_cut_sessions(r.cut_last_two_sessions) {
        out.push(to_view_with(
            "Set cuts in two straight sessions on this lift: hold weekly volume, no adds"
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
                "2+ interval reps over target: slow the remaining reps ~{:.0}%",
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
                "Easy pace pushes HR over the cap. Slow down; the HR cap governs easy days"
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
            "2 of the last 3 HRV readings unreliable: suspend HRV gating; use subjective + performance until a clean baseline returns".to_string(),
            &graded((), "AUTOREG-FALLBACK-001"),
        ));
    }

    // autoreg-034: multi-day lnRMSSD suppression → recovery day / easy block.
    if let Some(days) = r.hrv_suppressed_days {
        let rec = autoreg::hrv_suppressed_recovery_day(days);
        if rec.value {
            out.push(to_view_with(
                "HRV suppressed 3+ consecutive days: insert a recovery day / easy block"
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
                "Wellness suppressed 2+ days with resting HR trending up: take 1–3 easy days or cross-train".to_string(),
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
                    "Decoupling {:.1}% (<5%): sound aerobic base",
                    d.drift_pct
                ),
                load::DecouplingBand::BuildBase => format!(
                    "Decoupling {:.1}% (5–10%): build the aerobic base another 3–6 weeks",
                    d.drift_pct
                ),
                load::DecouplingBand::Insufficient => format!(
                    "Decoupling {:.1}% (≥10%): effort sat above aerobic threshold; endurance base not yet sufficient",
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
                why: WhyView {
                    basis: claim_statement(ev.citation.claim_id.as_deref())
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("Based on {}", ev.citation.reference)),
                    grade_note: grade_note_str(
                        &format!("{:?}", ev.grade),
                        tag.contested,
                        tag.contested_question_ref.as_deref(),
                    ),
                    improves: improves_for(ev.citation.claim_id.as_deref()),
                },
            });
        }
    }

    // hypertrophy-035: ≥2 accumulated overreaching triggers → deload now.
    if let Some(n) = r.hypertrophy_deload_triggers {
        let d = hypertrophy::deload_indicated(n);
        if d.value {
            out.push(to_view_with(
                "2+ overreaching triggers accumulated: take the deload week now rather than waiting for the scheduled one".to_string(),
                &d,
            ));
        }
    }

    // hypertrophy-039: >10% set-to-set rep drop → lengthen rest.
    if let Some(frac) = r.rep_drop_frac {
        let rest = hypertrophy::increase_rest_on_rep_drop(frac);
        if rest.value {
            out.push(to_view_with(
                "Reps fell >10% set-to-set. Lengthen the rest interval to protect per-set volume"
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
                    "{} weekly sets with {}: treat as over MRV and deload",
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
                    "Recovery is compromised. Scale this week to {:.0}–{:.0} sets (70–80% of {}) and cut failure frequency",
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
                    "Combined-training red flags persisting a week or more: insert a deload / recovery block".to_string(),
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
                "Interference symptoms with no race commitment: swap part of the run volume for cycling/rowing".to_string(),
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
                "Stalled again after the re-ramp: transition this lift to intermediate (weekly) progression".to_string()
            } else {
                format!(
                    "3 straight failed sessions with recovery in order: deload this lift {:.0}% and re-ramp",
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
    // Chronologically latest tracked run (ties → most recently logged), so a
    // backdated GPS run never masquerades as "the latest session" here.
    model
        .runs
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.track.is_empty())
        .max_by_key(|(i, r)| (r.observed_at, *i))
        // Split feedback must be computed from the SAME QC'd track the run row
        // renders (`to_run_view` → `qc_track`), not the accuracy-only
        // `usable_track` inside `track_positive_split_pct`. A fix that clears the
        // accuracy gate but fails a QC gate (teleport / non-monotonic time /
        // auto-pause jitter) would otherwise shift this split vs the displayed
        // one, letting the run chip and the coaching cue disagree near ±3%.
        .and_then(|(_, r)| {
            let (track, _, starts) = qc_run_track(r);
            running::track_positive_split_pct_seg(&track, running::MAX_GPS_ACCURACY_M, &starts)
        })
}

/// Readiness window (days) after which a performance signal that can drive a
/// load INCREASE is treated as stale and dropped. An engine heuristic (the KB
/// gives no expiry parameter), chosen conservatively (a fortnight). Only the
/// increase-driving signals expire (see [`is_expirable_perf_signal`]); protective
/// signals never do (HARD RULE 3).
const PERF_SIGNAL_EXPIRY_DAYS: i64 = 14;

/// Whether a readiness signal is a performance signal that can only drive (or is
/// adjacent to) a load INCREASE, so expiring a stale one is CONSERVATIVE.
/// Deliberately narrow: `Rpe`/`EstimatedOneRm` are the two `IncreaseLoadPct`
/// producers, `BarVelocity` is the VBT-increase input. Everything else (the
/// velocity-loss stop, aerobic decoupling, wellness/HRV/RHR suppression, and all
/// pain/illness/RED-S/soreness safety flags) is protective, and dropping a
/// protective signal is anti-conservative, so it is NEVER expired.
/// End-of-local-today boundary, unix seconds: a row with `observed_at`
/// beyond it is FUTURE-dated (a mis-imported 2030 GPX) and must not anchor
/// "current" windows (weekly-report cur-week, CTL/ATL last day). Same local-day
/// boundary the run anchors / `session_logged` use. `None` when the shell has not
/// supplied "today" (no clock in the core) → no guard, pre-existing behavior.
fn end_of_local_today_sec(model: &Model) -> Option<i64> {
    model.today_epoch_day.map(|d| {
        d.saturating_add(1)
            .saturating_mul(DAY_SEC)
            .saturating_sub(model.today_utc_offset_sec)
            .saturating_sub(1)
    })
}

fn is_expirable_perf_signal(s: ReadinessSignal) -> bool {
    matches!(
        s,
        ReadinessSignal::Rpe | ReadinessSignal::EstimatedOneRm | ReadinessSignal::BarVelocity
    )
}

/// Fraction by which the most recent logged run's distance exceeds the athlete's
/// recent-longest baseline (e.g. `0.15` = 15 % over). `None` when there is no
/// baseline yet, a first run has nothing to be a spike *over*, so the safety
/// gate must not defer on it (the run view already says why it looks unbounded).
/// This is a FALLBACK figure for the feedback resolver, which only runs when a
/// `SessionReview` exists (see `view()`, `model.review.map(build_feedback)`): it
/// fills in the spike fraction the review omits, mirroring the positive-split
/// fallback, it does NOT arm the gate on its own without a review.
///
/// The baseline is DERIVED at view() time from the current run history (the
/// longest OTHER run in the trailing 30-day window ending at this run's
/// `observed_at`), NOT the per-row `longest_recent_km` baked at ingest. Deleting
/// the run that seeded the baseline therefore re-arms the gate on the next view
/// (the stored field stayed at, e.g., 30 km and silently disarmed it before).
fn latest_run_spike_frac(model: &Model) -> Option<f64> {
    // Chronologically latest run (ties → most recently logged): with
    // backdating, "the most recent logged run" means log TIME, not log order.
    let (idx, r) = model
        .runs
        .iter()
        .enumerate()
        .max_by_key(|(i, r)| (r.observed_at, *i))?;
    let baseline = model
        .runs
        .iter()
        .enumerate()
        .filter(|(i, o)| {
            *i != idx
                && o.observed_at <= r.observed_at
                && r.observed_at.saturating_sub(o.observed_at) <= SPIKE_WINDOW_SEC
        })
        .map(|(_, o)| run_distance_km(o))
        .fold(0.0_f64, f64::max);
    if baseline <= 0.0 {
        return None;
    }
    Some(run_distance_km(r) / baseline - 1.0)
}

/// Seconds per (epoch) week, the deterministic week bucket for the weekly
/// running-volume system. Weeks are `observed_at / 604800` (epoch-aligned, so
/// boundaries fall on Thursday 00:00 UTC): bookkeeping, not a calendar claim.
const WEEK_SEC: i64 = 604_800;

/// Seconds per day, the deterministic day bucket for CTL/ATL chaining.
const DAY_SEC: i64 = 86_400;

/// Widest CTL/ATL accumulation window, in days (~2 years). The CTL (τ=42 d) and
/// ATL (τ=7 d) EWMAs settle within a few time-constants, so 730 days is far past
/// convergence for both while bounding the daily chain. Without this cap the
/// `for day in first..=last` loop runs one iteration per epoch-day between the
/// earliest and latest counted run: a single corrupt or mis-unit'd `observed_at`
/// (a millisecond stamp ~1.7e12 beside a seconds one ⇒ ~19.7M days, or a line
/// near `i64::MAX`) would spin `view()` into a multi-million-iteration hang
/// (HIGH bug). `view()` runs on every event and every launch replay, so one bad
/// persisted run could wedge the app permanently.
const MAX_LOAD_DAYS: i64 = 730;

/// Longest logged run, km, the running-040 marathon-optimism input. `None`
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
                let (track, _, starts) = qc_run_track(r);
                moving_duration_min(&track, &starts)
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
    // A future-dated run (mis-imported 2030 GPX) would set the chain's `last`
    // day into the future, sliding the whole MAX_LOAD_DAYS window off the real
    // history so CTL/ATL/TSB reflect only that phantom run. Drop it here.
    let cutoff = end_of_local_today_sec(model);
    for r in &model.runs {
        let minutes = if r.track.is_empty() {
            r.duration_min
        } else {
            let (track, _, starts) = qc_run_track(r);
            moving_duration_min(&track, &starts)
        };
        if r.observed_at <= 0
            || r.hr_pct_max <= 0.0
            || minutes <= 0.0
            || cutoff.is_some_and(|c| r.observed_at > c)
        {
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
    // Bound the daily chain to the most recent MAX_LOAD_DAYS (see the constant):
    // the EWMAs have long converged by then, and this caps the loop so a corrupt
    // or mis-unit'd `observed_at` can never spin `view()` into an unbounded loop.
    let start = first.max(last - (MAX_LOAD_DAYS - 1));
    let (mut ctl, mut atl) = (0.0_f64, 0.0_f64);
    for day in start..=last {
        let l = daily.get(&day).copied().unwrap_or(0.0);
        ctl = load::ctl(ctl, l);
        atl = load::atl(atl, l);
    }
    let span_days = last - start + 1;
    let tsb = load::tsb(ctl, atl);
    let round1 = |x: f64| (x * 10.0).round() / 10.0;
    let g = graded((), "LOAD-PMC-001");
    Some(TrainingLoadView {
        ctl: round1(ctl),
        atl: round1(atl),
        tsb: round1(tsb),
        days: span_days as u32,
        sessions_counted: counted,
        sessions_skipped: skipped,
        method: "Heart-rate training load (Lucía 3-zone TRIMP), smoothed into a 42-day fitness average (CTL) and a 7-day fatigue average (ATL)".to_string(),
        summary: format!(
            "Fitness (CTL) {:.1} · Fatigue (ATL) {:.1} · Form (TSB) {:+.1} over {} days: bookkeeping, not a performance predictor",
            round1(ctl),
            round1(atl),
            round1(tsb),
            span_days
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
    // Anchor "this week" on the most recent NON-future logged week, so a
    // mis-imported 2030 run can't hijack the weekly guidance until it's deleted.
    let cutoff = end_of_local_today_sec(model);
    let Some(cur_week) = runs
        .iter()
        .filter(|r| cutoff.is_none_or(|c| r.observed_at <= c))
        .map(|r| r.observed_at.div_euclid(WEEK_SEC))
        .max()
    else {
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
                "Week-over-week {prev_km:.1} → {weekly_km:.1} km ({pct:+.0}%): {}",
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
                    "Volume up >30% over two weeks ({baseline2_km:.1} → {weekly_km:.1} km): elevated injury risk, flatten the ramp"
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
            "Shares below are counted by time-in-zone (avg HR), the reporting default"
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
                "Long run {long_run_km:.1} km is {:.0}% of the week: over the ≤25% single-run cap",
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
                    "Long run {long_run_km:.1} km exceeds 2× your average daily distance ({:.1} km): outsized relative to the week",
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
                "Easy (Z1) share {:.0}% of run time: {}",
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
                "{} hard (Z3) session{} this week: {}",
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
                    "Running volume up >10% ({prev_km:.1} → {weekly_km:.1} km) while lifting: cap the combined ramp"
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
                        "Heavy leg work and a hard/long run only {h:.0} h apart: keep ≥24 h between them (residual fatigue lasts 24–48 h)"
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
                    "{exercise}: {total_reps} total reps @ ~{mean_pct:.0}%1RM: {}",
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
                        "{exercise}: ~{mean_pct:.0}%1RM sits below the ~30%1RM effective floor. Add load"
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
                        "A set was logged at ~{worst_rir} RIR. Beyond 5 RIR the estimate is unreliable (error >2 reps); train closer to failure to calibrate"
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
                    "High-rep sets make e1RM formulas unreliable. Prefer a {}–{}-rep test set to gauge strength",
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
                    "Depth jumps: squat e1RM {squat_e1rm:.0} kg vs {bw:.0} kg BW: {}",
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
            "12-minute distance too short to estimate VO2max: the formula floor is ~505 m"
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
                    "fitted D′ is negative. A trial was likely not maximal; re-test"
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
            "{label}: {} reps on the AMRAP set at {:.0} lb: hold the load",
            q.reps, q.current_load_lb
        )
    } else {
        format!(
            "{label}: {} reps on the AMRAP set at {:.0} lb: adjust next load {lo:+.0} to {hi:+.0} lb",
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
            "Rolling trend is up. Consistency is paying off; set the next process goal"
        }
        feedback::TrendSummary::Plateau => {
            "Flat 4+ weeks: normal consolidation; change ONE variable and protect the routine"
        }
        feedback::TrendSummary::LoadExplainedDecline => {
            "The dip lines up with load/recovery, not lost fitness. Recovery first; consider a deload week"
        }
        feedback::TrendSummary::Stable => "Trend steady: keep stacking consistent weeks",
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
fn build_autoreg_source(inputs: &[ReadinessInput]) -> Option<AdjustmentView> {
    if inputs.is_empty() {
        return None;
    }
    let latest_day = inputs
        .iter()
        .map(|i| i.observed_at)
        .max()
        .unwrap_or(0)
        .div_euclid(DAY_SEC);
    let hrv_today = inputs.iter().any(|i| {
        i.signal == ReadinessSignal::HrvLnRmssd && i.observed_at.div_euclid(DAY_SEC) == latest_day
    });
    let recent_hrv = inputs
        .iter()
        .filter(|i| {
            i.signal == ReadinessSignal::HrvLnRmssd
                && i.observed_at.div_euclid(DAY_SEC) >= latest_day - 7
        })
        .count()
        .min(255) as u8;
    let subjective = inputs.iter().any(|i| {
        matches!(
            i.signal,
            ReadinessSignal::WellnessZ | ReadinessSignal::Soreness
        )
    });
    let src = autoreg::autoreg_source(hrv_today, recent_hrv, subjective);
    let text = match src.value {
        autoreg::AutoregSource::HrvRolling => "Autoregulating on the 7-day rolling HRV gate",
        autoreg::AutoregSource::SubjectivePlusPerformance => {
            "No usable HRV: autoregulating on subjective wellness + performance"
        }
        autoreg::AutoregSource::PerformanceOnlyHold => {
            "No HRV or wellness data: performance-only mode: hold loads, no progression beyond plan"
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
        category_label: feedback_category_label(resolved.value).to_string(),
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
        why: why_from(None, &resolved),
    }
}

/// Human overline for the feedback card face (File 05 category naming). Exported
/// from core so a new [`FeedbackCategory`] variant can never silently lose its
/// overline in the shell (the labels used to be hand-maintained in Kotlin).
fn feedback_category_label(cat: FeedbackCategory) -> &'static str {
    match cat {
        FeedbackCategory::ConcernInjury => "Injury concern",
        FeedbackCategory::ConcernRecovery => "Recovery concern",
        FeedbackCategory::ConcernBehavior => "Training pattern",
        FeedbackCategory::DangerousProgression => "Progression warning",
        FeedbackCategory::IntensityDiscipline => "Intensity",
        FeedbackCategory::PositiveExecution => "Pacing",
        FeedbackCategory::InformationalNeutral => "Note",
        FeedbackCategory::CorrectiveProcess => "Adjustment",
        FeedbackCategory::PositiveMastery => "Mastery",
        FeedbackCategory::ContextualBadDay => "Off day",
        FeedbackCategory::ProgressionNudge => "Progression",
    }
}

/// Coaching copy for a feedback category (File 05 voice: autonomy-supportive,
/// process-framed, never guilt-inducing).
fn feedback_message(cat: FeedbackCategory) -> &'static str {
    match cat {
        FeedbackCategory::ConcernInjury => {
            "Stop training this area and see a professional. This looks like a bone-stress red flag."
        }
        FeedbackCategory::ConcernRecovery => {
            "Several overtraining signals are stacking up. Back off and prioritize recovery this week."
        }
        FeedbackCategory::ConcernBehavior => {
            "This pattern looks compulsive. A rest day is not lost progress. Consider stepping back."
        }
        FeedbackCategory::DangerousProgression => {
            "That was a large single-session jump. Rein in the progression to protect connective tissue."
        }
        FeedbackCategory::IntensityDiscipline => {
            "Easy days should stay easy. Dial the effort back to build the aerobic base."
        }
        FeedbackCategory::PositiveExecution => "Well-paced, durable effort. Nicely controlled.",
        FeedbackCategory::InformationalNeutral => {
            "Mild aerobic fatigue noted: nothing to correct."
        }
        FeedbackCategory::CorrectiveProcess => {
            "Missed target is data, not failure. Here's an adjustment for next time, your call."
        }
        FeedbackCategory::PositiveMastery => {
            "Target hit at planned cost. You've earned the next planned progression."
        }
        FeedbackCategory::ContextualBadDay => {
            "Off day: normal variation. The stimulus still counts; no guilt."
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
    push_guidance_basis(rows, section, summary, None, r);
}

/// Like [`push_guidance`], but with a call-site-specific `basis` line for the
/// why? disclosure (a datum-rich restatement the generic claim statement can't
/// carry, e.g. the HR-zones card echoing the user's age through Tanaka). When
/// `basis` is `None`, the backing claim's registered statement is used.
fn push_guidance_basis<T>(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    summary: String,
    basis: Option<String>,
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
        why: why_from(basis, r),
    });
}

/// Human label for a running goal distance, mirroring the Kotlin
/// `GoalDistance.label` (ProfileEditor.kt) exactly. Used in guidance copy so the
/// row never leaks the raw Debug enum name (e.g. "FiveK", "C25k").
fn goal_distance_label(g: GoalDistance) -> &'static str {
    match g {
        GoalDistance::General => "General fitness",
        GoalDistance::C25k => "Couch to 5K",
        GoalDistance::FiveK => "5K",
        GoalDistance::TenK => "10K",
        GoalDistance::HalfMarathon => "Half marathon",
        GoalDistance::Marathon => "Marathon",
    }
}

/// Human label for a lift goal, mirroring the Kotlin `LiftGoal.label`
/// (ProfileEditor.kt) exactly. Used in guidance copy so the row never leaks the
/// raw Debug enum name (e.g. "MaxStrength").
fn lift_goal_label(g: LiftGoal) -> &'static str {
    match g {
        LiftGoal::MaxStrength => "Max strength",
        LiftGoal::Power => "Power",
        LiftGoal::Hypertrophy => "Hypertrophy",
    }
}

/// Human label for a training age, so guidance copy never leaks the raw Debug
/// enum name (e.g. a future `LateIntermediate` would render as PascalCase).
fn training_age_label(ta: individualization::TrainingAge) -> &'static str {
    match ta {
        individualization::TrainingAge::Novice => "Novice",
        individualization::TrainingAge::Intermediate => "Intermediate",
        individualization::TrainingAge::Advanced => "Advanced",
    }
}

/// Run the programming engine over the profile, producing evidence-cited rows.
fn build_guidance(p: &Profile) -> Vec<GuidanceView> {
    let mut rows = Vec::new();

    build_safety_guidance(p, &mut rows);

    let lifting = p.weekly_sets > 0;
    let running = p.running_days_per_week > 0;

    let age_r = individualization::training_age_from_cadence(p.progression_cadence);
    let age = age_r.value;
    push_guidance(
        &mut rows,
        "Profile",
        format!("Training age: {}", training_age_label(age)),
        &age_r,
    );

    if lifting {
        build_strength_guidance(p, age, &mut rows);
    }

    let hvs = individualization::high_volume_sensitivity(age);
    push_guidance(
        &mut rows,
        "Individualization",
        format!(
            "High-volume sensitivity: {}",
            if hvs.value {
                "yes: cap added volume"
            } else {
                "no"
            }
        ),
        &hvs,
    );

    // Representative training-age->years count, reused by the running weekly
    // volume-increase cap and the hybrid lower-body interference check. The KB
    // caps only distinguish sub-1-year runners, so no precise figure is needed.
    let age_years = match age {
        individualization::TrainingAge::Novice => 0.5,
        _ => 2.0,
    };

    if running {
        build_running_guidance(p, age, age_years, &mut rows);
    }

    build_environment_guidance(p, &mut rows);
    build_reentry_guidance(p, &mut rows);

    if lifting && running {
        build_hybrid_guidance(p, age_years, &mut rows);
    }

    rows
}

/// Leading Safety block: onboarding gates, pregnancy avoid-list, and the
/// high-mileage bone-stress / energy-availability guards (all `p`-gated).
fn build_safety_guidance(p: &Profile, rows: &mut Vec<GuidanceView>) {
    // Stage-0 onboarding gates lead every guidance list (File 08 onboard-050:
    // screen BEFORE any prescription; safety-000: never overridden by goals).
    // Each fired gate renders as a Safety row with its deferral reason.
    for gate in individualization::onboarding_gates(&p.health) {
        push_guidance(rows, "Safety", describe(&gate.value), &gate);
    }
    // Pregnancy avoid-list (safety-047) travels with the safety-045 deferral.
    if p.health.pregnant {
        let pre = individualization::pregnancy_precautions();
        push_guidance(
            rows,
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
            rows,
            "Safety",
            format!(
                "Bone-stress-injury surveillance: {:.0} km/wk exceeds ~64 km. Monitor for focal bone pain; keep energy availability adequate",
                p.running_km_per_week
            ),
            &bsi,
        );
        // hybrid-024 energy-availability guard: the higher-risk cohorts
        // (high-volume endurance, leaner, female) get named vigilance.
        let ea = hybrid::energy_availability_guard(true, false, p.female);
        if ea.value {
            push_guidance(
                rows,
                "Safety",
                "Energy-availability guard (RED-S/LEA): high endurance volume raises under-fueling risk. Keep intake matched to load".to_string(),
                &ea,
            );
        }
    }
}

/// Strength + Hypertrophy rows for a lifting profile.
fn build_strength_guidance(
    p: &Profile,
    age: individualization::TrainingAge,
    rows: &mut Vec<GuidanceView>,
) {
    let sd = individualization::strength_defaults(age);
    push_guidance(
        rows,
        "Strength",
        format!(
            "Defaults: {}%1RM, {}×/muscle/wk, {} sets/muscle",
            sd.value.intensity_pct_1rm, sd.value.freq_per_muscle, sd.value.sets_per_muscle
        ),
        &sd,
    );

    let lr = strength::loading_rx(p.lift_goal);
    // The Power band's numeric RIR (3-5) is an expert-opinion encoding
    // (STR-PWR-RIR-001), not a KB number; this row cites STR-PWR-001
    // (Moderate), so state only the KB's qualitative power instruction.
    let rir_clause = if p.lift_goal == LiftGoal::Power {
        "never to failure; stop before bar speed drops".to_string()
    } else {
        format!("RIR {}-{}", lr.value.rir.0, lr.value.rir.1)
    };
    push_guidance(
        rows,
        "Strength",
        format!(
            "{} loading: {}-{}%1RM, {}-{} reps, {}-{} sets, {}",
            lift_goal_label(p.lift_goal),
            lr.value.pct_1rm.0,
            lr.value.pct_1rm.1,
            lr.value.reps.0,
            lr.value.reps.1,
            lr.value.sets.0,
            lr.value.sets.1,
            rir_clause
        ),
        &lr,
    );

    let vlt = strength::vl_termination_threshold(p.lift_goal);
    push_guidance(
        rows,
        "Strength",
        format!(
            "Velocity-loss set cutoff for {}: end the set at ~{:.0}% bar-speed loss",
            lift_goal_label(p.lift_goal),
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
        rows,
        "Strength",
        format!(
            "Test a true 1RM only when technically proficient, recovered, and warmed up{}; spinal lifts need bracing competence",
            if novice {
                ", as a novice, only supervised (prefer the estimated 1RM)"
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
            rows,
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
    // Human label instead of the raw Debug enum (which leaked "Dup" etc.).
    let pm_label = match pm.value {
        strength::PeriodizationModel::Linear => "linear",
        strength::PeriodizationModel::Dup => "daily undulating (DUP)",
        strength::PeriodizationModel::Block => "block",
        strength::PeriodizationModel::Conjugate => "conjugate",
    };
    push_guidance(
        rows,
        "Strength",
        format!("Periodization model: {pm_label}"),
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
                    rows,
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
                    rows,
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
            rows,
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
            rows,
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
        let pr = graded(*pr, "STR-PRILEPIN-001");
        push_guidance(
            rows,
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
            rows,
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
        rows,
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
        rows,
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
        rows,
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
        rows,
        "Hypertrophy",
        format!("Growth-target weekly sets capped at {}", cap.value),
        &cap,
    );

    let mev = hypertrophy::mev_sets_by_training_age(age);
    push_guidance(
        rows,
        "Hypertrophy",
        format!(
            "MEV for {}: {}-{} sets/muscle/wk",
            training_age_label(age), mev.value.0, mev.value.1
        ),
        &mev,
    );
}

/// Running rows for a profile that runs.
fn build_running_guidance(
    p: &Profile,
    age: individualization::TrainingAge,
    age_years: f64,
    rows: &mut Vec<GuidanceView>,
) {
    let gp = running::goal_week_plan(p.goal_distance, p.advanced);
    // A4: C25K carries no long-run share (running-025 defines no long run for
    // C25K), so drop the "long run …%" fragment when the share is zero rather
    // than printing a misleading "long run 0-0% of volume". B2: the goal
    // distance renders as a human label, not the raw Debug enum name.
    let summary = if gp.value.long_run_share.1 > 0.0 {
        format!(
            "{}: {}-{} sessions/wk, {}-{} quality, long run {:.0}-{:.0}% of volume",
            goal_distance_label(p.goal_distance),
            gp.value.sessions_per_week.0,
            gp.value.sessions_per_week.1,
            gp.value.quality_per_week.0,
            gp.value.quality_per_week.1,
            gp.value.long_run_share.0 * 100.0,
            gp.value.long_run_share.1 * 100.0
        )
    } else {
        format!(
            "{}: {}-{} sessions/wk, {}-{} quality",
            goal_distance_label(p.goal_distance),
            gp.value.sessions_per_week.0,
            gp.value.sessions_per_week.1,
            gp.value.quality_per_week.0,
            gp.value.quality_per_week.1
        )
    };
    push_guidance(rows, "Running", summary, &gp);

    // Quality-session governance caps, the guardrail behind the "quality/wk"
    // count above. Only relevant to someone who actually runs, so a pure lifter
    // is not shown running caps.
    {
        let ql = running::quality_limits();
        push_guidance(
            rows,
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
        rows,
        "Running",
        format!(
            "Base-phase intensity: {}/{}/{} easy/moderate/hard %time",
            id.value.easy_pct, id.value.moderate_pct, id.value.hard_pct
        ),
        &id,
    );

    let ef = hybrid::endurance_frequency_ok(p.running_days_per_week);
    push_guidance(
        rows,
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
        rows,
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

    let wc = running::weekly_increase_cap_frac(age_years);
    push_guidance(
        rows,
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
    {
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
            rows,
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
            rows,
            "Running",
            format!("Long runs: {}", long_parts.join(", ")),
            &long,
        );

        let cruise = running::cruise_interval_rx();
        push_guidance(
            rows,
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
            rows,
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
                rows,
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
            rows,
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
            rows,
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
                rows,
                "Running",
                format!(
                    "Race taper ({}): {}–{} days, cut volume {:.0}–{:.0}%{}. Hold intensity and frequency",
                    goal_distance_label(p.goal_distance),
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
            rows,
            "Running",
            "Progress ONE variable at a time. Never raise weekly volume and intensity in the same week"
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
                    rows,
                    "Running",
                    format!(
                        "Conditions trigger pace correction for {what}. Expect slower paces at the same effort; anchor to HR/RPE"
                    ),
                    &pc,
                );
            }
        }
    }
}

/// Environment modifier row, when the profile declares one.
fn build_environment_guidance(p: &Profile, rows: &mut Vec<GuidanceView>) {
    // File 08 indiv-025 / safety-024 environment modifiers, when declared.
    if let Some(env) = p.environment {
        let m = individualization::environment_modifier(env);
        let text = match env {
            Environment::Heat => format!(
                "Heat: reduce intensity, acclimatize progressively (~{}–{} days), hydrate. STOP on heat-illness signs (confusion, cessation of sweating, dizziness)",
                m.value.acclimatization_days.map(|d| d.0).unwrap_or(10),
                m.value.acclimatization_days.map(|d| d.1).unwrap_or(14)
            ),
            Environment::Altitude =>
                "Altitude (>~2,500 m): reduce absolute intensity until acclimatized".to_string(),
            Environment::Cold => "Cold: extend the warm-up".to_string(),
            Environment::Neutral => "Neutral environment: no modifier".to_string(),
        };
        if env != Environment::Neutral {
            push_guidance(rows, "Environment", text, &m);
        }
    }
}

/// Layoff re-entry ramp + post-layoff MEV reduction, when a layoff is declared.
fn build_reentry_guidance(p: &Profile, rows: &mut Vec<GuidanceView>) {
    // REENTRY-001 layoff re-entry ramp + the post-layoff MEV reduction.
    if let Some(weeks_off) = p.weeks_off
        && weeks_off > 0.0
    {
        let re = individualization::resistance_reentry(weeks_off);
        // A1: the >8 wk bracket carries NO KB load fraction (`load_frac == None`),
        // so render a fresh-start message with no invented percentage (KB Table
        // 3.4b language). The 1-8 wk numeric brackets keep the derate sentence.
        let summary = match re.value.load_frac {
            Some(frac) => format!(
                "After {weeks_off:.0} wk off: restart at ~{:.0}% of prior loads, ramp back over {}{}",
                frac * 100.0,
                fmt_u8_range(re.value.ramp_weeks.0, re.value.ramp_weeks.1),
                if re.value.treat_as_novice {
                    " wk. Progress like a novice until loads return"
                } else {
                    " wk"
                }
            ),
            None => format!(
                "After {weeks_off:.0} wk off: treat it as a fresh start. Re-establish technique and rebuild over 4-6+ wk, progressing like a novice."
            ),
        };
        push_guidance(rows, "Return to training", summary, &re);
        let mev = hypertrophy::layoff_reduces_mev(true);
        if mev.value {
            push_guidance(
                rows,
                "Return to training",
                "Post-layoff MEV is reduced. Less volume regrows muscle at re-entry; restart below the old set counts".to_string(),
                &mev,
            );
        }
    }
}

/// Hybrid (concurrent lift + run) rows, gated on the athlete doing both.
fn build_hybrid_guidance(p: &Profile, age_years: f64, rows: &mut Vec<GuidanceView>) {
    let so = hybrid::same_session_order(p.concurrent_goal);
    // Full-sentence copy keyed on the ordering (the goal is implied by the
    // sentence), instead of leaking the raw Debug enum names.
    let so_text = match so.value {
        hybrid::SessionOrder::LiftFirst => {
            "Combined days: lift first, run after (strength and muscle goals)."
        }
        hybrid::SessionOrder::RunFirst => {
            "Combined days: running first is fine (endurance priority)."
        }
        hybrid::SessionOrder::ForbidSameSession => {
            "Power goal: keep lifting and running in separate sessions."
        }
    };
    push_guidance(rows, "Hybrid", so_text.to_string(), &so);

    // Peak strength/power block running override (File 10 CAP-2): only relevant
    // when the lifting goal is a maximal quality, so a hypertrophy or endurance
    // athlete is not shown a cap that does not apply to their block.
    if matches!(p.lift_goal, LiftGoal::MaxStrength | LiftGoal::Power) {
        let pk = hybrid::peak_phase_run_cap();
        push_guidance(
            rows,
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
            rows,
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
        rows,
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
            rows,
            "Hybrid",
            "Interference scales most with continuous session duration (strongest moderator), then frequency. Shorten endurance sessions before cutting days".to_string(),
            &im,
        );
    }

    // Whether *this* athlete's training age makes them susceptible to the small
    // trained-lower-body 1RM decrement (File 10 hybrid-009): only trained lifters
    // (>1 yr) show it. Reuses the representative `age_years` above and is only
    // relevant when the athlete actually runs, so a pure lifter is not shown it.
    {
        let li = hybrid::expect_lower_strength_interference(age_years);
        push_guidance(
            rows,
            "Hybrid",
            format!(
                "Lower-body strength interference susceptibility: {}",
                if li.value {
                    "yes: trained lifter, expect a small lower-body 1RM decrement"
                } else {
                    "no: novice/untrained lower body is spared"
                }
            ),
            &li,
        );
    }
}

/// One run's realised distance in km: derived from the GPS track when present,
/// otherwise the hand-entered scalar.
fn run_distance_km(r: &LoggedRun) -> f64 {
    if r.track.is_empty() {
        r.distance_km
    } else {
        let (track, _, starts) = qc_run_track(r);
        running::track_distance_km_seg(&track, running::MAX_GPS_ACCURACY_M, &starts)
    }
}

/// RUN-SPIKE-001: the spike baseline is the longest run in the TRAILING
/// 30-day window ending at `at`, NOT all-time. An old long run before a layoff
/// must not permanently suppress a genuine load spike after the layoff. Runs
/// with `observed_at` in `[at − 30 d, at]` count (a future-dated row is excluded
/// so a back-dated amend can't seed the baseline of an earlier run).
const SPIKE_WINDOW_SEC: i64 = 30 * 86_400;

fn spike_baseline_km(runs: &[LoggedRun], at: i64) -> f64 {
    runs.iter()
        .filter(|r| {
            r.observed_at <= at && at.saturating_sub(r.observed_at) <= SPIKE_WINDOW_SEC
        })
        .map(run_distance_km)
        .fold(0.0_f64, f64::max)
}

/// Index of the NEWEST logged set an `AmendSet`/`DeleteEntry` targets: the last
/// (`rposition`) whose `entry_id` matches, or, for a legacy target (`id == 0`,
/// no per-entry id), the last legacy row whose `observed_at` matches. The
/// compaction Rule 3 matcher (`log.rs`) mirrors this predicate exactly, so a
/// delete/amend compacts against the same row it removes on replay.
fn find_set(sets: &[LoggedSet], id: u64, observed_at: i64) -> Option<usize> {
    if id != 0 {
        sets.iter().rposition(|s| s.entry_id == id)
    } else {
        sets.iter()
            .rposition(|s| s.entry_id == 0 && s.observed_at == observed_at)
    }
}

/// Index of the newest logged run an `AmendRun`/`DeleteEntry` targets, the
/// run analog of [`find_set`].
fn find_run(runs: &[LoggedRun], id: u64, observed_at: i64) -> Option<usize> {
    if id != 0 {
        runs.iter().rposition(|r| r.entry_id == id)
    } else {
        runs.iter()
            .rposition(|r| r.entry_id == 0 && r.observed_at == observed_at)
    }
}

/// File 07 GPS quality gates over a fix track, applied BEFORE any distance /
/// duration / split is derived. Returns the surviving fixes plus the dropped
/// count (accuracy-gated fixes included). Gates, in order per surviving pair:
/// a non-advancing timestamp (speed undefined), an implied speed >12 m/s
/// (`load::gps_speed_plausible`, impossible for a runner), and a <2.5 m move
/// (`load::gps_point_accept`, the Apple jitter/auto-pause pattern; the
/// vertical-rate arm passes 0.0 because fixes carry no altitude).
fn qc_track(points: &[GpsPoint], segment_starts: &[u32]) -> (Vec<GpsPoint>, u32, Vec<u32>) {
    let mut dropped = 0u32;
    let mut out: Vec<GpsPoint> = Vec::with_capacity(points.len());
    // Segment-start indices REMAPPED to positions in the QC'd output, a boundary
    // fix may itself be dropped, so the first survivor at/after it opens the new
    // segment. Passed on to the segment-aware `running::*_seg` fns and the GPX
    // export so a pause + relocation is neither summed nor drawn across.
    let mut out_starts: Vec<u32> = Vec::new();
    // A boundary seen in the raw stream but not yet committed (its fix and any
    // until the next survivor may be dropped). While pending, the inter-fix QC
    // gates are SUPPRESSED: the leg across a pause bridge is a legitimate
    // relocation teleport, not a jitter fix, and gating it would wrongly drop the
    // first fix of the new segment.
    let mut pending_boundary = false;
    for (idx, p) in points.iter().enumerate() {
        if segment_starts.contains(&(idx as u32)) {
            pending_boundary = true;
        }
        // Accuracy gate, same as `usable_track`.
        if p.accuracy_m > running::MAX_GPS_ACCURACY_M {
            dropped += 1;
            continue;
        }
        if !pending_boundary {
            if let Some(last) = out.last() {
                let dt = p.observed_at - last.observed_at;
                if dt <= 0 {
                    dropped += 1;
                    continue;
                }
                let dist_m = running::haversine_m(*last, *p);
                let speed = dist_m / dt as f64;
                if !load::gps_speed_plausible(speed) || !load::gps_point_accept(dist_m, 0.0) {
                    dropped += 1;
                    continue;
                }
            }
        }
        // First survivor of a new segment (index 0 is never a boundary → the
        // `!out.is_empty()` guard mirrors `running::segments` ignoring index 0).
        if pending_boundary && !out.is_empty() {
            out_starts.push(out.len() as u32);
        }
        pending_boundary = false;
        out.push(*p);
    }
    (out, dropped, out_starts)
}

/// [`qc_track`] over one logged run, threading its stored segment boundaries.
fn qc_run_track(r: &LoggedRun) -> (Vec<GpsPoint>, u32, Vec<u32>) {
    qc_track(&r.track, &r.track_segment_starts)
}

/// Moving time over a QC'd track, minutes: interval seconds where the implied
/// speed clears the File 07 stop gate (`load::is_stopped`, <0.5 m/s counts as
/// stopped, the auto-pause rule), so standing time never dilutes pace.
fn moving_duration_min(track: &[GpsPoint], segment_starts: &[u32]) -> f64 {
    let mut sec = 0.0;
    // Sum moving legs WITHIN each segment: the pause-bridge leg between segments
    // is never formed, so a paused relocation adds no moving time. Empty
    // `segment_starts` → one segment → the whole-track sum, unchanged.
    for seg in running::segments(track, segment_starts) {
        for w in seg.windows(2) {
            let dt = (w[1].observed_at - w[0].observed_at) as f64;
            if dt <= 0.0 {
                continue;
            }
            let speed = running::haversine_m(w[0], w[1]) / dt;
            if !load::is_stopped(speed) {
                sec += dt;
            }
        }
    }
    sec / 60.0
}

/// Format a per-unit pace (seconds per km/mile) as `m:ss`. The core owns this
/// formatting so a shell never re-derives it (parity with the `m:ss/km` summary
/// pace); truncates to whole seconds like the summary pace does.
fn fmt_pace_ms(sec_per_unit: f64) -> String {
    let s = if sec_per_unit > 0.0 { sec_per_unit as u32 } else { 0 };
    format!("{}:{:02}", s / 60, s % 60)
}

/// Build the shell-facing per-unit split views for a QC-gated track (unit metres
/// = [`running::KM_M`] or [`running::MILE_M`]). `distance_km` carries the
/// cumulative distance in km for both unit systems.
fn run_split_views(track: &[GpsPoint], unit_m: f64, segment_starts: &[u32]) -> Vec<RunSplitView> {
    running::track_splits_seg(track, running::MAX_GPS_ACCURACY_M, unit_m, segment_starts)
        .into_iter()
        .map(|s| RunSplitView {
            index: s.index,
            pace: fmt_pace_ms(s.pace_sec_per_unit),
            distance_km: s.cumulative_m / 1000.0,
            partial: s.partial,
        })
        .collect()
}

/// Cap on the per-run view memo (see [`to_run_view`]). Deleting a run leaves a
/// stale entry behind; clearing the whole map at the cap bounds growth WITHOUT
/// tracking access order (LRU eviction would make the cache observable). No real
/// history approaches this many distinct runs.
const RUN_VIEW_CACHE_CAP: usize = 4096;

thread_local! {
    /// P1: [`to_run_view_uncached`] re-derives every logged run on EVERY `view()`
    /// after every event, even though a run is immutable once written. Each
    /// derivation is ≈6 haversine (trig) passes, per-km AND per-mile split
    /// tables, the variability index, plus a ~0.5 MB GPX string built with two
    /// `format!`s per fix. Memoize the finished `RunResultView` keyed by a
    /// content fingerprint so the heavy work runs once per distinct run and every
    /// later `view()` serves a clone. Referentially transparent, identical run
    /// content → identical view, no clock/rand, so the core stays deterministic
    /// and the ViewModel wire shape is unchanged (gpx stays populated for the
    /// shell's list label + detail map). Thread-local, so no lock and no
    /// cross-thread poisoning; a fingerprint change (an amend) misses and
    /// recomputes.
    static RUN_VIEW_CACHE: std::cell::RefCell<std::collections::HashMap<u64, RunResultView>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

thread_local! {
    /// Test-only counter of UNCACHED run-view builds, thread-local so a laziness
    /// test measures a race-free delta around two `view()` calls (see the P1
    /// test). Not compiled into shipping builds.
    #[cfg(test)]
    static RUN_VIEW_BUILDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A content fingerprint of a logged run, every field the derived view depends
/// on. Cheap integer hashing (bit patterns for the floats) versus the trig +
/// string allocation the cached derivation avoids. Collision probability is
/// ~n²/2⁶⁴ (negligible for a personal history); a collision would only swap one
/// display view, never a safety decision.
fn run_content_key(r: &LoggedRun) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    r.entry_id.hash(&mut h);
    r.observed_at.hash(&mut h);
    r.distance_km.to_bits().hash(&mut h);
    r.duration_min.to_bits().hash(&mut h);
    r.hr_pct_max.to_bits().hash(&mut h);
    r.longest_recent_km.to_bits().hash(&mut h);
    // Small enum-or-None; Debug form is a cheap, stable discriminator.
    format!("{:?}", r.workout_type).hash(&mut h);
    r.track_segment_starts.hash(&mut h);
    for p in &r.track {
        p.lat.to_bits().hash(&mut h);
        p.lon.to_bits().hash(&mut h);
        p.observed_at.hash(&mut h);
        p.accuracy_m.to_bits().hash(&mut h);
    }
    h.finish()
}

/// Derive zone + pace + distance-spike flag for one logged run, memoized (P1).
/// The heavy derivation lives in [`to_run_view_uncached`]; this wrapper serves a
/// cached clone whenever the run's content fingerprint is unchanged, so a run is
/// derived once rather than on every `view()`.
fn to_run_view(r: &LoggedRun) -> RunResultView {
    let key = run_content_key(r);
    if let Some(cached) = RUN_VIEW_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return cached;
    }
    let view = to_run_view_uncached(r);
    RUN_VIEW_CACHE.with(|c| {
        let mut m = c.borrow_mut();
        if m.len() >= RUN_VIEW_CACHE_CAP {
            m.clear();
        }
        m.insert(key, view.clone());
    });
    view
}

/// The uncached run-view derivation. The run CARD's spike chip judges against the
/// per-row `longest_recent_km` baked at ingest (which honors a tracker's
/// caller-supplied paired history). The SAFETY GATE that can defer training
/// derives its baseline fresh at view() time from `model.runs` instead (see
/// [`latest_run_spike_frac`]).
fn to_run_view_uncached(r: &LoggedRun) -> RunResultView {
    #[cfg(test)]
    RUN_VIEW_BUILDS.with(|c| c.set(c.get() + 1));
    let recent_longest_km = r.longest_recent_km;
    // A GPS track derives its own distance/duration; a manual run uses scalars.
    let gps = !r.track.is_empty();
    // File 07 QC gates run BEFORE any derivation (accuracy, implausible speed,
    // jitter, non-advancing time); the dropped count is surfaced to the shell.
    let (track, qc_dropped, seg_starts) = if gps {
        qc_run_track(r)
    } else {
        (Vec::new(), 0, Vec::new())
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
            duration_min: 0.0,
            hr_pct_max: r.hr_pct_max,
            spike_flag: false,
            spike_note: String::new(),
            split_pct: None,
            split: None,
            interval: None,
            workout_type: r.workout_type,
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
            entry_id: r.entry_id,
            splits_km: Vec::new(),
            splits_mi: Vec::new(),
            spike_has_baseline: recent_longest_km > 0.0,
        };
    }

    let distance_km = if gps {
        running::track_distance_km_seg(&track, running::MAX_GPS_ACCURACY_M, &seg_starts)
    } else {
        r.distance_km
    };
    // GPS duration is MOVING time (File 07 auto-pause: <0.5 m/s intervals are
    // excluded), so a paused run's pace reflects running, not standing.
    let duration_min = if gps {
        moving_duration_min(&track, &seg_starts)
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
    let spike = running::single_session_spike_flag(distance_km, recent_longest_km);

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
        running::track_positive_split_pct_seg(&track, running::MAX_GPS_ACCURACY_M, &seg_starts)
    } else {
        None
    };

    // Per-km and per-mile splits from the SAME QC-gated track as distance/pace
    // (GPS runs only; a hand-entered run has no track to walk). The core owns the
    // `m:ss` pace formatting so a shell renders each split verbatim.
    let (splits_km, splits_mi) = if gps {
        (
            run_split_views(&track, running::KM_M, &seg_starts),
            run_split_views(&track, running::MILE_M, &seg_starts),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    // Interval-vs-steady differentiation (RUN-INTERVAL-VI-001): the variability
    // index (normalized speed ÷ average speed) rates a reps-plus-recovery run
    // above a steady run of the *same average pace*. GPS-only, a hand-entered
    // run has no per-point speed series to compute it from.
    let variability_index = if gps {
        running::track_variability_index_seg(&track, running::MAX_GPS_ACCURACY_M, &seg_starts)
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
        if recent_longest_km > 0.0 {
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
        format!(": {spike_note}")
    };

    RunResultView {
        zone: zone_str.clone(),
        pace: pace.clone(),
        distance_km,
        duration_min,
        hr_pct_max: r.hr_pct_max,
        spike_flag: spike.value,
        spike_note: spike_note.to_string(),
        split_pct,
        split: split_pct.map(split_verdict_view),
        interval: variability_index.map(interval_verdict_view),
        workout_type: r.workout_type,
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
        splits_km,
        splits_mi,
        qc_dropped,
        gpx: {
            // Export the same QC-gated fixes used for distance/duration so the
            // file's distance matches what the app shows. A track whose fixes
            // all fail the gates leaves fewer than two usable points: no real
            // route, so emit no GPX rather than a degenerate file the shell
            // would still offer an "Export" button for.
            if track.len() >= 2 {
                running::export_gpx_seg(&track, &format!("Run {distance_km:.1}km"), &seg_starts)
            } else {
                String::new()
            }
        },
        observed_at: r.observed_at,
        entry_id: r.entry_id,
        spike_has_baseline: recent_longest_km > 0.0,
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
                "Back half {split_pct:.0}% slower: start easier and aim for an even-to-negative split."
            ),
        ),
        // feedback-017: even or negative split → pacing-discipline praise.
        _ if split_pct < -feedback::POSITIVE_SPLIT_FLAG_PCT => (
            "negative",
            format!("NEG SPLIT {:.0}%", split_pct.abs()),
            "Negative split: textbook pacing discipline.".to_string(),
        ),
        _ => (
            "even",
            "EVEN SPLIT".to_string(),
            "Even split: textbook pacing discipline.".to_string(),
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

/// Build the interval-vs-steady chip for a GPS run's variability index
/// (RUN-INTERVAL-VI-001). The `interval`/`steady` cutoff lives in exactly one
/// place, `running::interval_verdict` / `running::INTERVAL_VI_THRESHOLD`, so a
/// shell renders `kind`/`label`/`message` verbatim and never re-derives it. The
/// copy is honest about the grade: this is the flat-ground GOVSS/NGP
/// simplification, deliberately Weak-graded.
fn interval_verdict_view(vi: f64) -> IntervalVerdictView {
    let r = running::interval_verdict(vi);
    // Copy MEASURES, it does not "rate": the variability index is a descriptive
    // label only, no downstream load/adjustment consumes it yet, so claiming the
    // run is "rated above its average" would overstate. The interval/steady cutoff
    // is an engine heuristic (there is no cited threshold), stated as such.
    let (kind, label, message) = if r.value {
        (
            "interval",
            format!("INTERVAL · VI {vi:.2}"),
            "Hard efforts split by recovery: this run's pace varied well above its \
             average (variability index measures the spread; a steady run sits near 1.0)."
                .to_string(),
        )
    } else {
        (
            "steady",
            format!("STEADY · VI {vi:.2}"),
            "Evenly paced: its average pace reflects the whole run.".to_string(),
        )
    };
    IntervalVerdictView {
        kind: kind.to_string(),
        label,
        message,
        variability_index: vi,
        grade: format!("{:?}", r.evidence.grade),
        citation: r.evidence.citation.reference.clone(),
        confidence: r.confidence.score,
        safety_critical: r.confidence.safety_critical,
        contested: r.confidence.contested,
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
    let (agreed, mut low_sec, mut high_sec) = match eq.value {
        running::Equivalency::Agreed(mid) => (true, mid, mid),
        running::Equivalency::Range(lo, hi) => (false, lo, hi),
    };

    // running-040/008 (option B, "flag AND derate"): a marathon prediction
    // from a runner without long-run support runs optimistic, so shift the
    // DISPLAYED band SLOWER by the cited VDOT-point derate, not merely a
    // caveat. Gated on the logged run history (no history → no claim → band
    // untouched); computed once here so the evidence-bearing caveat note below
    // reuses it. ONLY the 42.2 km goal is affected; every other distance and
    // an adequately-mileaged marathon keep their band byte-identical.
    let marathon_derate = if (q.goal_distance_m - 42_195.0).abs() < 10.0 {
        longest_logged_km.and_then(|longest| {
            let opt = running::marathon_prediction_optimistic(longest);
            opt.value
                .then(|| (longest, opt, running::marathon_derated_band(low_sec, high_sec, vdot)))
        })
    } else {
        None
    };
    if let Some((_, _, band)) = &marathon_derate {
        low_sec = band.value.0;
        high_sec = band.value.1;
    }

    let goal_label = race_distance_label(q.goal_distance_m);
    let degenerate = low_sec <= 0.0;
    // Display shape follows the ACTUAL band: a derate can open a span even where
    // the two methods agreed. For every non-derated prediction
    // `low_sec == high_sec` iff the methods agreed, so this is byte-identical
    // there (`Equivalency::Agreed` sets the bounds equal, `Range` sets them
    // strictly apart).
    let single = low_sec == high_sec;
    let predicted = if degenerate {
        "-".to_string()
    } else if single {
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
                format!("Input race is {weeks} weeks old: at the edge of the 6–8-week freshness window"),
                &fresh,
            ),
            running::RaceInputFreshness::Stale => push_guidance(
                &mut notes,
                "Prediction",
                format!("Input race is {weeks} weeks old (>8). Re-test before trusting these paces"),
                &fresh,
            ),
        }
    }
    // running-040: marathon predictions run optimistic without long-run
    // support. Judged from the logged run history; no history → no claim. The
    // displayed band was already derated above (option B, "flag AND derate");
    // this keeps the flag + the ~2–3 VDOT-point magnitude, and both the note
    // (`opt`) and the derated band carry RUN-VDOT-001 evidence (HARD RULE 2).
    if let Some((longest, opt, _band)) = &marathon_derate {
        // running-008: the matching correction, derate the projection by
        // ~2–3 VDOT points for an under-mileaged marathoner.
        let derate = running::vdot_derate_points(GoalDistance::Marathon, true);
        push_guidance(
            &mut notes,
            "Prediction",
            format!(
                "Longest logged run {longest:.1} km: marathon predictions run optimistic without long-run support (derate ~{:.0}–{:.0} VDOT points)",
                derate.value.0, derate.value.1
            ),
            opt,
        );
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
        recent_distance_m: q.recent_distance_m,
        recent_time_sec: q.recent_time_sec,
        goal_distance_m: q.goal_distance_m,
        weekly_km: q.weekly_km,
        weeks_since_race: q.weeks_since_race,
    }
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
                "\"{}\" is not a known muscle. Pick one of: chest, back, quads, hamstrings, glutes, side delts, rear delts, biceps, triceps, calves, abs",
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
            "Landmarks: MEV {} · MAV {}–{} · MRV {} sets/wk",
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
                "{} sets in one session exceeds the ~11-set per-session cap. Add a session instead of stacking sets",
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
            "Work most sets at {}–{} RIR. True failure is not required and costs recovery",
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
        "Take sets to failure only on machines/isolation, never on heavy free-weight compounds"
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
            "Tempo: controlled {}–{} s/rep ({}–{} s up, {}–{} s down). Superslow (>10 s) is inferior",
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
                "Not growing while recovering easily: raise next mesocycle to {} sets/wk (from {current})",
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
                    "Plan starts at {first_week} sets/wk vs your current {}: too abrupt; step volume up gradually",
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
fn build_protein_targets(
    q: &ProteinQuery,
    reds_present: bool,
) -> (Vec<GuidanceView>, Vec<ProteinFigureView>) {
    let mut rows = Vec::new();
    // #6: structured g/day figures paralleling the prose rows (see `ProteinFigureView`).
    let mut figures = Vec::new();

    // Cannot derive g/day from a non-positive bodyweight, say nothing rather
    // than emit a nonsensical or zero target.
    if q.bodyweight_kg <= 0.0 {
        return (rows, figures);
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
        figures.push(ProteinFigureView {
            kind: "masters".to_string(),
            low_g_per_day: (q.bodyweight_kg * lo).round(),
            high_g_per_day: (q.bodyweight_kg * hi).round(),
            refused: false,
        });
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
                figures.push(ProteinFigureView {
                    kind: "deficit".to_string(),
                    low_g_per_day: (q.bodyweight_kg * lo).round(),
                    high_g_per_day: (q.bodyweight_kg * hi).round(),
                    refused: false,
                });
            }
            None => {
                // safety-022: the deficit is refused, not silently omitted -
                // the user sees why, cited to the RED-S deferral.
                push_guidance(
                    &mut rows,
                    "Protein",
                    "Deficit not prescribed: a RED-S / low-energy-availability signal is present. Reduce training stress and consult a physician or registered dietitian before any caloric deficit.".to_string(),
                    &r,
                );
                figures.push(ProteinFigureView {
                    kind: "deficit".to_string(),
                    low_g_per_day: 0.0,
                    high_g_per_day: 0.0,
                    refused: true,
                });
            }
        }
    }

    (rows, figures)
}

/// Build a graded heart-rate-zone table from age: the Tanaka HRmax estimate plus
/// the five Daniels %HRmax training bands mapped to absolute bpm ranges
/// (`running::vdot_band_hr_pct`, running-007). The HRmax rows carry the Tanaka
/// formula's own claim RUN-HRMAX-001 (Weak, ±10 bpm SEE), citing the VDOT
/// claim here would overstate the formula's evidence; the %HRmax band rows are
/// Daniels tables (RUN-VDOT-001, Moderate). No training numbers are invented
/// (HARD RULE 1/2). A non-positive or implausible age yields a single
/// explanatory row rather than a bogus HRmax.
fn build_hr_zones(
    q: &HrZoneQuery,
    measured_hr_max: Option<f64>,
) -> (Vec<GuidanceView>, Option<HrMaxView>) {
    let mut rows = Vec::new();

    // Wire contract: a MEASURED HRmax (from the profile, logged off an all-out
    // effort) supersedes the age-based Tanaka estimate: it is the person's own
    // datum, not a population average. Only a physiologically plausible value is
    // trusted (100–240 bpm, finite).
    let measured = measured_hr_max.filter(|m| m.is_finite() && (100.0..=240.0).contains(m));

    // HRmax from age is only meaningful for a realistic adult/junior age; refuse
    // to emit a fabricated maximum outside that range. A measured max bypasses
    // the age requirement (it needs no age).
    if measured.is_none() && !(5.0..=100.0).contains(&q.age_years) {
        let note = graded((), "RUN-HRMAX-001");
        push_guidance(
            &mut rows,
            "Heart-rate zones",
            "Enter an age between 5 and 100 to estimate HRmax and training zones.".to_string(),
            &note,
        );
        return (rows, None);
    }

    // running::hr_max_tanaka is the same Tanaka 208 − 0.7·age estimator the
    // load module exposes; the running-module wrapper is used so the zone
    // table and the running rules share one source. A measured max replaces it.
    let hr_max = measured.unwrap_or_else(|| running::hr_max_tanaka(q.age_years));
    let hr_max_row = graded((), "RUN-HRMAX-001");
    let (hr_max_text, hr_max_basis) = if let Some(m) = measured {
        (
            format!("Measured HRmax: {m:.0} bpm (your logged maximum)"),
            format!(
                "Using your measured HRmax ({m:.0} bpm) from an all-out effort: this replaces the \
                 age-based Tanaka estimate and drives the %HRmax band targets below."
            ),
        )
    } else {
        (
            format!(
                "Estimated HRmax: {hr_max:.0} bpm (Tanaka 208 − 0.7 × {:.0})",
                q.age_years
            ),
            format!(
                "Estimated from your age ({:.0}) with the Tanaka formula (208 − 0.7 × age = {hr_max:.0} bpm). \
                 It is a population average, not your measured maximum. Individuals vary by roughly ±10 bpm.",
                q.age_years
            ),
        )
    };
    push_guidance_basis(
        &mut rows,
        "Heart-rate zones",
        hr_max_text,
        Some(hr_max_basis),
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
                    "Resting HR {rhr:.0} < 55: %HRmax and %HRR diverge; Karvonen (%HRR) targets shown per band"
                ),
                true,
            ),
            running::HrMethodPreference::EitherConverged => (
                format!("Resting HR {rhr:.0} ≥ 70: the two HR methods converge; either works"),
                false,
            ),
            running::HrMethodPreference::Unstated => (
                format!(
                    "Resting HR {rhr:.0} is in the 55–69 range, where the source states no method rule: %HRmax shown"
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
            "MAF aerobic cap (base-phase option): 180 − age = {:.0} bpm. Personalize toward measured LT1 when data exist",
            maf.value
        ),
        &maf,
    );

    // running-006: recompute zones off a measured max every 4–6 weeks.
    if let Some(weeks) = q.weeks_since_recalc {
        let due = running::hr_zone_recalc_due(weeks);
        if due.value {
            let noun = if weeks == 1 { "week" } else { "weeks" };
            push_guidance(
                &mut rows,
                "Heart-rate zones",
                format!("Zones last recalculated {weeks} {noun} ago. Recompute from a measured HRmax (every 4–6 weeks)"),
                &due,
            );
        }
    }

    // running-041: training paces re-test on the same 4–6-week cadence.
    if let Some(weeks) = q.weeks_since_pace_test {
        let due = running::pace_retest_due(weeks);
        if due.value {
            let noun = if weeks == 1 { "week" } else { "weeks" };
            push_guidance(
                &mut rows,
                "Heart-rate zones",
                format!("Paces last tested {weeks} {noun} ago. Re-test to set paces from CURRENT fitness"),
                &due,
            );
        }
    }

    // #6: the same resolved figure as the prose HRmax row above, structured so
    // the shell reads bpm / measured-vs-estimate / the Tanaka split from data.
    // The 208 / 0.7 constants mirror `running::hr_max_tanaka` (208 − 0.7·age);
    // they apply only to the age-based estimate, so they are 0 when measured.
    let hr_max_view = if measured.is_some() {
        HrMaxView {
            bpm: hr_max.round(),
            measured: true,
            age_years: 0.0,
            tanaka_intercept: 0.0,
            tanaka_slope: 0.0,
        }
    } else {
        HrMaxView {
            bpm: hr_max.round(),
            measured: false,
            age_years: q.age_years,
            tanaka_intercept: 208.0,
            tanaka_slope: 0.7,
        }
    };

    (rows, Some(hr_max_view))
}

/// Flatten every logged set, threading each exercise's previous e1RM through so
/// the view carries the per-lift trend (delta + direction) and the shell renders
/// it without arithmetic. "Previous" = the most recent earlier logged set of the
/// same exercise (exact name match), the core holds no session boundary for
/// sets, so set-over-set is the deterministic proxy for session-over-session.
fn lift_views(sets: &[LoggedSet]) -> Vec<LiftResultView> {
    // Chronological (observed_at) order, stable for ties and undated (0)
    // legacy entries: a backdated set slots into its true position, so the
    // per-exercise e1RM delta chain compares against the chronologically
    // previous set, not whatever happened to be logged before it.
    let mut ordered: Vec<&LoggedSet> = sets.iter().collect();
    ordered.sort_by_key(|s| s.observed_at);
    let mut last_e1rm: std::collections::HashMap<&str, f64> = std::collections::HashMap::new();
    ordered
        .into_iter()
        .map(|s| {
            let prev = last_e1rm.get(s.exercise.as_str()).copied();
            let view = to_lift_view(s, prev);
            last_e1rm.insert(s.exercise.as_str(), view.e1rm_kg);
            view
        })
        .collect()
}

/// Evidence-grade rank for headline priority (mirrors `EvidenceGrade` order in
/// schema.rs: Strong > Moderate > Weak > ExpertOpinion > MarketingMyth). Works
/// on the flattened wire string; an unknown grade sorts last.
fn grade_rank(grade: &str) -> i32 {
    match grade {
        "Strong" => 4,
        "Moderate" => 3,
        "Weak" => 2,
        "ExpertOpinion" => 1,
        "MarketingMyth" => 0,
        _ => -1,
    }
}

/// The single highest-priority call for today (usability-ia-spec §7): a
/// train-blocking safety hold wins outright; else the strongest triggered
/// adjustment (safety-critical first, then grade, then confidence, the same
/// ranking shells previously re-implemented); else the session feedback; else
/// the all-clear default, which asserts no claim and carries no evidence tag.
// ── Coach-as-planner ────────────────────────────

/// Monday-indexed weekday of an epoch-day (epoch day 0 = 1970-01-01 = Thursday).
/// `rem_euclid` handles pre-epoch (negative) days too.
///
/// Reduce BEFORE adding so a wire `epoch_day` near `i64::MAX` (a corrupt
/// `SetToday`) can't debug-overflow on `+ 3`; the panic would cross the FFI
/// firewall as an error object and brick every later `view()`.
fn mon0_weekday(epoch_day: i64) -> i64 {
    (epoch_day.rem_euclid(7) + 3).rem_euclid(7)
}

/// Clamp a wire float to a finite, bounded domain on ingest. `serde_json`
/// already refuses NaN/Inf *tokens*, but a huge FINITE magnitude (e.g. 1e300)
/// can multiply/accumulate to `inf` in `view()`; which then serializes to
/// `null` and breaks a non-null Kotlin decode (a persistent view-brick). NaN
/// maps to 0.0; the ±1e12 bound keeps any downstream product well inside f64
/// range while comfortably covering every real kg / km / min / bpm value.
fn sanitize_f64(x: f64) -> f64 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(-1.0e12, 1.0e12)
    }
}

/// Round a load to the nearest 2.5 kg plate increment (honest arithmetic; the
/// grade still travels with the underlying %1RM claim).
fn round_2_5(x: f64) -> f64 {
    (x / 2.5).round() * 2.5
}

/// Format a kg load: integer when whole, else one decimal (e.g. `92.5`).
fn fmt_kg(kg: f64) -> String {
    if kg.fract().abs() < 1e-9 {
        format!("{kg:.0}")
    } else {
        format!("{kg:.1}")
    }
}

/// Best logged e1RM per exercise (0.1-rounded), most-recent-first; the plan
/// anchors loads to these. Deterministic despite the HashMap:
/// the output is sorted by (observed_at desc, name asc).
///
/// A set with a non-positive e1RM (a 0 kg / bodyweight or garbage entry)
/// NEVER becomes an anchor; otherwise the plan would prescribe "@ 0 kg". Only
/// exercises with a positive best e1RM are emitted; the exercise key is bucketed
/// case-INSENSITIVELY (the lookup in `plan::Anchors::e1rm_for` is), so "Squat"
/// and "squat" merge into one anchor (the first-seen display casing kept).
fn build_plan_anchors(sets: &[LoggedSet], weeks_off: Option<f64>) -> crate::plan::Anchors {
    // key = lowercased name; value = (best e1RM, latest observed_at, display name).
    let mut best: std::collections::HashMap<String, (f64, i64, String)> =
        std::collections::HashMap::new();
    // 2c REENTRY-001: after a declared layoff the anchored %1RM working LOAD is
    // derated by the KB-cited resistance re-entry fraction (`resistance_reentry`,
    // File 08 Table 3.4b) for the 1-8 wk brackets. Carry the fraction here and
    // apply it to the LOAD in `flatten_prescription`; the e1RM anchor itself stays
    // the true logged best, so the "your logged best" line never lies. Beyond
    // 8 wk the KB gives NO fraction and directs a fresh-novice re-entry, so
    // `reentry_novice` is set instead and `flatten_prescription` drops the e1RM
    // anchor entirely (no invented number). Keys ONLY on declared `weeks_off`; no
    // age-based decay is invented. `None`/`false` = full loads.
    let (reentry_load_frac, reentry_novice) = match weeks_off {
        Some(w) if w > 0.0 => {
            let re = individualization::resistance_reentry(w).value;
            (re.load_frac.filter(|f| *f < 1.0), re.treat_as_novice)
        }
        _ => (None, false),
    };
    for s in sets {
        let e1 = strength::e1rm_epley(s.weight_kg, s.reps);
        // Ignore a set that yields no positive e1RM; it can't anchor a load.
        if !(e1 > 0.0) {
            continue;
        }
        let key = s.exercise.to_ascii_lowercase();
        let e = best
            .entry(key)
            .or_insert((0.0, i64::MIN, s.exercise.clone()));
        if e1 > e.0 {
            e.0 = e1;
        }
        if s.observed_at > e.1 {
            e.1 = s.observed_at;
        }
    }
    let mut v: Vec<(String, f64, i64)> = best
        .into_values()
        .map(|(e1, t, name)| (name, e1, t))
        .collect();
    v.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    crate::plan::Anchors {
        lift_e1rm: v
            .into_iter()
            .map(|(k, e1, _)| (k, (e1 * 10.0).round() / 10.0))
            .collect(),
        reentry_load_frac,
        reentry_novice,
        ..Default::default()
    }
}

/// Run-history anchors for plan synthesis, derived from `model.runs` the same way
/// [`build_plan_anchors`] anchors lifts to logged e1RM. Returns
/// `(longest_recent_run_km, recent_weekly_km)`.
///
/// Deterministic: the "now" reference is the shell-supplied `today_epoch_day`
/// (× 86 400 → unix seconds), never a wall clock, the core stays pure.
///
/// - `longest_recent_run_km`: longest completed run in the TRAILING 30-day window
///   ending at `today`. Uses the SAME window (`SPIKE_WINDOW_SEC`) and predicate as
///   [`spike_baseline_km`] (RUN-SPIKE-001, Frandsen et al. 2025): a long run
///   prescribed AT this value is 0 % over the 30-day longest = no single-session
///   spike, so it is a SAFE demonstrated-capacity anchor (progression BEYOND it
///   stays governed by the spike/ramp rules).
/// - `recent_weekly_km`: MEASURED average weekly volume = total km in the trailing
///   28 days ÷ 4 (same window predicate). A fact about the athlete, fed to the
///   existing KB-cited long-run 25 % share rule (RUN-LONGRUN-001, running-016).
///
/// Both are `None` when the window holds no runs, so a log-less profile's plan is
/// unchanged (byte-identical to the pre-anchor behaviour).
///
/// Runs are bucketed by their LOCAL calendar day (`observed_at` shifted by
/// the shell's `today_utc_offset_sec`, exactly like [`session_logged`]) so the
/// window ends at the end of LOCAL today and a run logged earlier TODAY counts;
/// the earlier `observed_at <= today*86400` predicate treated the local epoch
/// day as a UTC instant at midnight and dropped any same-day (and, west of UTC,
/// yesterday-evening) run. For an offset-0 history stamped at local midnight this
/// is byte-identical to the old boundary; a future-dated row (local day past
/// today) is still excluded.
///
/// Detraining taper (no window-expiry cliff): past the 30-day full-credit
/// window the demonstrated longest-run anchor does NOT vanish in one step (the
/// old bug: a 21 km race holder dropped to the volume-only floor overnight at
/// day 31, e.g. 10 km → 4 km). Instead its credit decays LINEARLY to zero over a
/// further `DETRAIN_TAPER_DAYS`, reflecting the KB detraining timeline
/// (`DETRAIN-001` / File 08 load-037, Moderate: trained aerobic capacity is
/// retained ~2 wk and declines only ~6–20 % over ~4 wk, a gradual loss, not a
/// cliff; Mujika & Padilla 2000, Bosquet et al. 2013). Reaching zero credit at
/// day 30 + 28 = 58 (≈ 8 wk) aligns with the KB's re-entry bracket
/// (Table 3.4b: 4–8 wk off → "treat near-novice for spike caps"), beyond which
/// no demonstrated-capacity credit remains. The LINEAR shape + 28-day length are
/// an expert-opinion PARAMETER the KB does not state, flagged as such per the
/// GRADE good-practice-statement precedent (see
/// `knowledge-base/autoreg-citation-provenance-resolution.md`): the PRINCIPLE is
/// graded (Moderate), the exact taper is a heuristic. The winning run being
/// OLDER than the full-credit window sets the returned `detrained` flag so
/// `plan.rs` re-points the long-run citation to `DETRAIN-001`. All existing
/// guardrails (≤2×daily-average, spike ceiling, ≤25 % share) still bound the
/// resulting long run downstream. The separate LOG-TIME spike gate
/// (`spike_baseline_km`) keeps its strict hard-30-day window: the taper is a
/// planning-capacity concept only, never a loosening of the safety gate.
fn build_run_anchors(
    runs: &[LoggedRun],
    today_epoch_day: i64,
    utc_offset_sec: i64,
) -> (Option<f64>, Option<f64>, bool) {
    // Longest recent run over the trailing 30 days (then tapered); measured
    // volume over 28 (no taper, volume is a plain sum, not a capacity ceiling).
    const LONGEST_WINDOW_DAYS: i64 = 30;
    const WEEKLY_WINDOW_DAYS: i64 = 28;
    const DETRAIN_TAPER_DAYS: i64 = 28;
    const DECAY_END_DAYS: i64 = LONGEST_WINDOW_DAYS + DETRAIN_TAPER_DAYS; // 58
    let local_day = |t: i64| t.saturating_add(utc_offset_sec).div_euclid(86_400);
    // Detraining credit factor for a run of the given age in days: full credit
    // through the 30-day window, then a linear taper to 0 by day 58.
    let decay = |age_days: i64| -> f64 {
        if age_days <= LONGEST_WINDOW_DAYS {
            1.0
        } else if age_days >= DECAY_END_DAYS {
            0.0
        } else {
            1.0 - (age_days - LONGEST_WINDOW_DAYS) as f64 / DETRAIN_TAPER_DAYS as f64
        }
    };

    // Longest recent run, detraining-adjusted: max over every non-future run
    // within the decay horizon of `distance × decay(age)`. A run within the
    // full-credit window contributes at factor 1.0, so a history whose longest
    // run is ≤30 days old is byte-identical to the pre-taper behaviour (an older,
    // decayed run can only WIN when it was longer than every in-window run, the
    // exact cliff case the taper exists to smooth).
    let mut best: f64 = 0.0;
    let mut best_detrained = false;
    for r in runs.iter() {
        let d = local_day(r.observed_at);
        if d > today_epoch_day {
            continue; // future-dated row excluded, like spike_baseline_km
        }
        let age = today_epoch_day.saturating_sub(d);
        if age >= DECAY_END_DAYS {
            continue; // fully detrained → no capacity credit
        }
        let eff = run_distance_km(r) * decay(age);
        if eff > best {
            best = eff;
            best_detrained = age > LONGEST_WINDOW_DAYS;
        }
    }
    let longest_recent = (best > 0.0).then_some(best);
    let detrained = longest_recent.is_some() && best_detrained;

    let total_km: f64 = runs
        .iter()
        .filter(|r| {
            let d = local_day(r.observed_at);
            d <= today_epoch_day && today_epoch_day.saturating_sub(d) <= WEEKLY_WINDOW_DAYS
        })
        .map(run_distance_km)
        .sum();
    let recent_weekly = (total_km > 0.0).then_some(total_km / 4.0);

    (longest_recent, recent_weekly, detrained)
}

/// Human title for a session type ("Heavy day", "Long run", "Rest").
fn session_title(st: SessionType) -> String {
    match st {
        SessionType::Lift(LiftSessionType::MaxEffort) => "Heavy day",
        SessionType::Lift(LiftSessionType::DynamicEffort) => "Power day",
        SessionType::Lift(LiftSessionType::Repetition) => "Volume day",
        SessionType::Lift(LiftSessionType::Accessory) => "Accessory",
        SessionType::Run(RunSessionType::LongRun) => "Long run",
        SessionType::Run(RunSessionType::Tempo) => "Tempo run",
        SessionType::Run(RunSessionType::Interval) => "Intervals",
        SessionType::Run(RunSessionType::Recovery) => "Recovery run",
        SessionType::Run(RunSessionType::RacePace) => "Race-pace run",
        SessionType::Run(RunSessionType::Repetition) => "Repetitions",
        SessionType::Run(RunSessionType::Strides) => "Strides",
        SessionType::Run(RunSessionType::Hills) => "Hill repeats",
        SessionType::Rest => "Rest",
    }
    .to_string()
}

/// Human goal label for the program summary.
fn human_goal(goal: &Goal) -> String {
    match goal {
        Goal::Strength => "Strength".into(),
        Goal::Hypertrophy => "Hypertrophy".into(),
        Goal::Power => "Power".into(),
        Goal::RunningRace { distance_km } => format!("{distance_km:.0} km race"),
        Goal::GeneralEndurance => "Endurance".into(),
        Goal::Hybrid => "Hybrid".into(),
    }
}

/// Whether a logged entry of the session's discipline exists on `epoch_day`
/// (a LOCAL calendar day). A UTC `observed_at` is shifted by the shell's
/// `today_utc_offset_sec` before bucketing, so it lands on the SAME local day
/// the plan is dated against (a set logged 00:30 Berlin isn't attributed to
/// "yesterday UTC").
///
/// KNOWN LIMITATION (LOW, external review, documented, not fixable without a
/// wire change): every past entry is bucketed with TODAY's offset
/// (`model.today_utc_offset_sec`), because that is the only offset on the wire.
/// The offset that was actually in force when a past set was logged is not
/// retained per-entry, and is unknowable retroactively. So after a DST shift or
/// travel that changed the device offset since an entry was logged, a near-
/// midnight past entry can be attributed to the wrong LOCAL plan-strip day -
/// off by exactly one day, and only for entries whose local time sits within
/// `|Δoffset|` of midnight (Δoffset = today's offset − the offset at log time;
/// typically 1 h for DST, so only the ~23:00–01:00 band is at risk). The full
/// fix would stamp each entry with its own offset at log time, a wire/schema
/// change deliberately out of scope here. Effect is display-only (a plan-strip
/// day marked done/missed), never a safety gate; no fabricated data.
fn session_logged(st: SessionType, epoch_day: i64, model: &Model) -> bool {
    let offset = model.today_utc_offset_sec;
    let on_day = |t: i64| t > 0 && t.saturating_add(offset).div_euclid(86_400) == epoch_day;
    match st {
        SessionType::Lift(_) => model.sets.iter().any(|s| on_day(s.observed_at)),
        SessionType::Run(_) => model.runs.iter().any(|r| on_day(r.observed_at)),
        SessionType::Rest => false,
    }
}

/// Rebuild a lift item's one-line summary from its (possibly adjusted) fields.
fn refresh_lift_summary(it: &mut PrescriptionView) {
    if it.exercise.is_empty() {
        return; // a run item - its summary is not sets×reps
    }
    it.summary = match it.load_kg {
        Some(kg) => format!(
            "{}: {}×{} @ {} kg · {}",
            it.exercise,
            it.sets,
            it.reps_low,
            fmt_kg(kg),
            it.intensity_label
        ),
        None => format!(
            "{}: {}×{} · {}",
            it.exercise, it.sets, it.reps_low, it.intensity_label
        ),
    };
}

/// Cap a RUN item to an easy target: keep the bounded volume (the text
/// before the ` · ` in the summary) but replace the hard intensity with an easy
/// zone. Used by DowngradeSession / ModifyAndMonitor so a run prescription is
/// actually modified, not merely annotated.
fn cap_run_item_easy(it: &mut PrescriptionView, r: &Recommended<Adjustment>, note: &str) {
    // Downgraded to a continuous easy run: the original rep structure is
    // deferred, not prescribed, so drop it (and any "N × …" leading token that
    // would otherwise read as an interval on an easy card).
    it.rep_count = 0;
    it.rep_volume = String::new();
    let vol = it
        .summary
        .split(" · ")
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let vol = if vol.contains('×') { String::new() } else { vol };
    it.intensity_label = "Easy: Zone 1-2".into();
    it.summary = if vol.is_empty() {
        it.intensity_label.clone()
    } else {
        format!("{vol} · {}", it.intensity_label)
    };
    it.adjusted_note = note.to_string();
    // The item was downgraded to easy, so EVERY evidence field must surface the
    // decision that downgraded it, this readiness/safety adjustment `r`, not the
    // stale ORIGINAL band (e.g. Tempo Strong). Re-point the grade chip + citation
    // + confidence to `r`, and rebuild the whole why? triad from `r` via
    // `why_from` (basis overridden with the honest downgrade rationale, grade_note
    // now consistent with the re-pointed chip, improves keyed to `r`'s claim).
    it.grade = format!("{:?}", r.evidence.grade);
    it.citation = r.evidence.citation.reference.clone();
    it.confidence = r.confidence.score;
    it.safety_critical = r.confidence.safety_critical;
    it.contested = r.confidence.contested;
    it.why = why_from(
        Some(format!(
            "Downgraded to an easy Zone 1-2 run: {note}. The original session's \
             harder band is deferred, not prescribed today."
        )),
        r,
    );
}

/// Flatten one graded `Prescription` into a shell-facing `PrescriptionView`,
/// resolving the load from the anchor when the intensity is %1RM.
fn flatten_prescription(
    rx: &Recommended<Prescription>,
    anchors: &crate::plan::Anchors,
) -> PrescriptionView {
    let grade = format!("{:?}", rx.evidence.grade);
    let citation = rx.evidence.citation.reference.clone();
    let confidence = rx.confidence.score;
    let safety_critical = rx.confidence.safety_critical;
    let contested = rx.confidence.contested;

    match &rx.value {
        Prescription::Lift(l) => flatten_lift_prescription(
            l, anchors, rx, grade, citation, confidence, safety_critical, contested,
        ),
        Prescription::Run(r) => {
            flatten_run_prescription(r, rx, grade, citation, confidence, safety_critical, contested)
        }
    }
}

/// Flatten a `Prescription::Lift` arm: resolves the load from the anchor for a
/// %1RM target and re-points the evidence triad for a declared layoff. The
/// header vars are threaded in by value so the re-entry re-point can override
/// them before they land in the `PrescriptionView`.
fn flatten_lift_prescription(
    l: &crate::schema::LiftPrescription,
    anchors: &crate::plan::Anchors,
    rx: &Recommended<Prescription>,
    mut grade: String,
    mut citation: String,
    mut confidence: f32,
    mut safety_critical: bool,
    mut contested: bool,
) -> PrescriptionView {
    // The 5th tuple slot is the "why?" `improves` line: an honest ENGINE
    // data-need (which input would sharpen this load), never a training
    // claim (HARD RULE 1). It overrides the claim-keyed default so a
    // prescription card carries the full 3-part disclosure like
    // adjustment/guidance cards do.
    let mut reentry_repoint: Option<Recommended<()>> = None;
    let (load_kg, intensity_label, anchored_on, basis, improves) = match l.intensity {
        LiftIntensity::PercentOneRm(p) => {
            // 2c: a 1-8 wk layoff derates the working LOAD by the KB-cited
            // re-entry fraction while keeping the e1RM anchor honest. A >8 wk
            // layoff (`reentry_novice`) never reaches this arm: `plan.rs::
            // lift_prescription` prescribes it by RIR instead (KB Table 3.4b:
            // treat as novice), handled in the `Rir` arm below.
            let e1 = anchors.e1rm_for(&l.exercise);
            // 2c: derate the working LOAD for a declared layoff; the e1RM
            // anchor stays the true logged best (honest `anchored_on`).
            let frac = anchors.reentry_load_frac.unwrap_or(1.0);
            let load = e1.map(|e| round_2_5(e * p as f64 / 100.0 * frac));
            let label = format!("{p:.0}% e1RM");
            let anchored_on = e1
                .map(|e| format!("e1RM {e:.1} kg (your logged best)"))
                .unwrap_or_default();
            let reentry_pct = anchors.reentry_load_frac.map(|f| f * 100.0);
            let (basis, improves) = match (load, e1) {
                (Some(kg), Some(e)) => (
                    Some(match reentry_pct {
                        Some(pct) => format!(
                            "{}×{} at {:.0}% of your logged e1RM ({:.1} kg), scaled to {:.0}% for your layoff → {} kg.",
                            l.sets, l.reps, p, e, pct, fmt_kg(kg)
                        ),
                        None => format!(
                            "{}×{} at {:.0}% of your logged e1RM ({:.1} kg) → {} kg.",
                            l.sets, l.reps, p, e, fmt_kg(kg)
                        ),
                    }),
                    // Anchored: the load is only as fresh as the e1RM
                    // behind it. A new logged session keeps it current.
                    Some("Log this session to keep your e1RM anchor current.".to_string()),
                ),
                _ => (
                    Some(format!(
                        "{}×{} at {:.0}% of your estimated 1RM.",
                        l.sets, l.reps, p
                    )),
                    // No anchor yet: a logged set replaces the estimate
                    // with a measured e1RM the load can key off.
                    Some(
                        "Log a set of this lift so loads can anchor to your measured e1RM instead of an estimate."
                            .to_string(),
                    ),
                ),
            };
            // 2c REENTRY re-point: a load derated for a declared layoff is
            // driven by the re-entry ramp, not the loading band, so re-point the
            // card's whole evidence + why triad after the match (mirrors
            // cap_run_item_easy / the run DETRAIN-001 re-point).
            if load.is_some() && anchors.reentry_load_frac.is_some() {
                reentry_repoint = Some(graded((), "REENTRY-001"));
            }
            (load, label, anchored_on, basis, improves)
        }
        LiftIntensity::Rir(n) => {
            // A >8 wk novice re-entry (`reentry_novice`) is prescribed by RIR,
            // not a %1RM anchor (KB Table 3.4b: treat as novice, technique
            // first). It has no load to derate but still cites the re-entry
            // reason so the card explains why the anchor is set aside.
            if anchors.reentry_novice {
                reentry_repoint = Some(graded((), "REENTRY-001"));
            }
            (
                None,
                format!("RIR {n}"),
                String::new(),
                Some(format!(
                    "{}×{} keeping {} reps in reserve: log a set to anchor a working load.",
                    l.sets, l.reps, n
                )),
                // No e1RM anchor: loads are RIR-based until a set is logged.
                Some(
                    "Log a set of this lift so loads can anchor to your measured e1RM instead of RIR."
                        .to_string(),
                ),
            )
        }
        LiftIntensity::Rpe(r) => (None, format!("RPE {r:.1}"), String::new(), None, None),
        LiftIntensity::VelocityMs(v) => {
            (None, format!("{v:.2} m/s"), String::new(), None, None)
        }
    };
    let mut why = why_from(basis, rx);
    if let Some(imp) = improves {
        why.improves = imp;
    }
    if let Some(re) = &reentry_repoint {
        grade = format!("{:?}", re.evidence.grade);
        citation = re.evidence.citation.reference.clone();
        confidence = re.confidence.score;
        safety_critical = re.confidence.safety_critical;
        contested = re.confidence.contested;
        why.grade_note = grade_note_str(
            &format!("{:?}", re.evidence.grade),
            re.confidence.contested,
            re.confidence.contested_question_ref.as_deref(),
        );
    }
    let mut it = PrescriptionView {
        summary: String::new(),
        exercise: l.exercise.clone(),
        sets: l.sets,
        reps_low: l.reps,
        reps_high: l.reps,
        load_kg,
        intensity_label,
        rest_sec: l.rest_sec,
        rep_count: 0,
        rep_volume: String::new(),
        anchored_on,
        adjusted_note: String::new(),
        grade,
        citation,
        confidence,
        safety_critical,
        contested,
        why,
    };
    refresh_lift_summary(&mut it);
    it
}

/// Flatten a `Prescription::Run` arm into its `PrescriptionView`.
fn flatten_run_prescription(
    r: &crate::schema::RunPrescription,
    rx: &Recommended<Prescription>,
    grade: String,
    citation: String,
    confidence: f32,
    safety_critical: bool,
    contested: bool,
) -> PrescriptionView {
    let vol = match r.volume {
        RunVolume::DurationMin(m) => format!("{m} min"),
        RunVolume::DistanceKm(k) => format!("{k:.0} km"),
    };
    // An interval/repetition run carries per-rep structure; surface
    // it so the card reads "4 × 4 min · Interval pace", not a misleading
    // "16 min · Interval pace" that looks like one continuous VO2max run.
    let rep_volume = r.repeats.map(|(_, rv)| match rv {
        RunVolume::DurationMin(m) => format!("{m} min"),
        RunVolume::DistanceKm(k) => format!("{:.0} m", (k as f64) * 1000.0),
    });
    let rep_count = r.repeats.map(|(n, _)| n).unwrap_or(0);
    // A long run re-pointed to RUN-SPIKE-001 (plan.rs) is anchored to the
    // athlete's demonstrated recent distance, so its share can exceed the
    // RUN-LONGRUN-001 ≤25% guideline. State that honestly (HARD RULE 2)
    // instead of the generic claim statement, no invented numbers, only
    // what the two KB entries state.
    let run_basis = match rx.evidence.citation.claim_id.as_deref() {
        Some("RUN-SPIKE-001") => Some(
            "Distance is anchored to your longest recent run, so the long run stays at \
             a distance you have already trained (no single-session spike). This exceeds \
             the 25% weekly-share guideline for now. Adding easy midweek volume would \
             bring the long-run share back down."
                .to_string(),
        ),
        // The anchoring run is now older than the 30-day window, so its
        // credit is tapering (DETRAIN-001) rather than dropping off a cliff.
        Some("DETRAIN-001") => Some(
            "Distance is held near your recent demonstrated long run, then tapered \
             gradually as that run ages instead of dropping off overnight. Trained \
             endurance is retained for a couple of weeks and fades slowly, not all at \
             once. Log a new long run to refresh the anchor."
                .to_string(),
        ),
        _ => None,
    };
    let label = match r.intensity {
        // A %HRmax run target is a band CEILING (easy/recovery/long runs
        // are capped, not held at a point): render it as "≤ X% HRmax" so
        // it never reads as an exact-point prescription.
        RunIntensity::HrPercentMax(p) => format!("≤ {p:.0}% HRmax"),
        RunIntensity::Vdot(b) => format!("{b:?} pace"),
        RunIntensity::ThreeZone(ThreeZone::Z1) => "Zone 1 (easy)".into(),
        RunIntensity::ThreeZone(ThreeZone::Z2) => "Zone 2".into(),
        RunIntensity::ThreeZone(ThreeZone::Z3) => "Zone 3 (hard)".into(),
        RunIntensity::PaceSecPerKm(s) => format!("{}:{:02}/km", s / 60, s % 60),
        RunIntensity::PowerPercentCp(p) => format!("{p:.0}% CP"),
    };
    // Rep-structured runs lead with the honest "N × <rep> · <pace>";
    // continuous runs keep the whole-session "<vol> · <pace>".
    let summary = match &rep_volume {
        Some(rv) => format!("{rep_count} × {rv} · {label}"),
        None => format!("{vol} · {label}"),
    };
    PrescriptionView {
        summary,
        exercise: String::new(),
        sets: 0,
        reps_low: 0,
        reps_high: 0,
        load_kg: None,
        intensity_label: label,
        rest_sec: 0,
        rep_count,
        rep_volume: rep_volume.unwrap_or_default(),
        anchored_on: String::new(),
        adjusted_note: String::new(),
        grade,
        citation,
        confidence,
        safety_critical,
        contested,
        why: why_from(run_basis, rx),
    }
}

/// Flatten one plan session onto a calendar day, judging its past status.
/// `start` is the plan's start epoch-day: a day BEFORE the plan existed is never
/// scored "missed" (a mid-week plan must not back-date the week's earlier
/// days as missed adherence).
fn flatten_session(
    s: &Session,
    epoch_day: i64,
    today: i64,
    start: i64,
    anchors: &crate::plan::Anchors,
    model: &Model,
) -> SessionPlanView {
    let items: Vec<PrescriptionView> = s
        .prescriptions
        .iter()
        .map(|rx| flatten_prescription(rx, anchors))
        .collect();
    let status = if matches!(s.session_type, SessionType::Rest) {
        "rest".to_string()
    } else if epoch_day < start {
        // Before the plan started: the user never had this prescription, so it
        // is neither done nor missed; show it as a neutral (upcoming-style) day.
        "planned".to_string()
    } else if epoch_day < today {
        if session_logged(s.session_type, epoch_day, model) {
            "done".to_string()
        } else {
            "missed".to_string()
        }
    } else if epoch_day == today && session_logged(s.session_type, epoch_day, model) {
        // Today, already logged (any matching-discipline entry that local day):
        // the session is accomplished, same semantics as a past logged day.
        "done".to_string()
    } else {
        "planned".to_string()
    };
    SessionPlanView {
        epoch_day,
        title: session_title(s.session_type),
        session_type: format!("{:?}", s.session_type),
        status,
        items,
        adjustment: None,
    }
}

/// Fold the active (non-blocking) readiness adjustments INTO today's session
/// (the key safety composition): a load cut, an RPE cap, or a
/// downgrade modifies the shown items so the plan never renders a top-end above
/// what readiness allows. Blocking holds never reach here (handled upstream by
/// `train_blocked`). Each applied adjustment carries its own evidence.
fn apply_adjustments_to_session(ns: &mut SessionPlanView, recommended: &[Recommended<Adjustment>]) {
    let mut applied: Option<AdjustmentView> = None;
    for r in recommended {
        match &r.value {
            Adjustment::ReduceLoadPct(p) => {
                let frac = (*p as f64) / 100.0;
                for it in ns.items.iter_mut() {
                    if let Some(kg) = it.load_kg {
                        it.load_kg = Some(round_2_5(kg * (1.0 - frac)));
                        it.adjusted_note =
                            format!("load −{p:.0}%: readiness ({})", claim_id_of(r));
                        refresh_lift_summary(it);
                    }
                }
                applied.get_or_insert_with(|| to_view(r));
            }
            Adjustment::Deload {
                load_reduction_pct, ..
            } => {
                let frac = (*load_reduction_pct as f64) / 100.0;
                for it in ns.items.iter_mut() {
                    if let Some(kg) = it.load_kg {
                        it.load_kg = Some(round_2_5(kg * (1.0 - frac)));
                        it.adjusted_note = format!("deload −{load_reduction_pct:.0}% load");
                        refresh_lift_summary(it);
                    }
                }
                applied.get_or_insert_with(|| to_view(r));
            }
            Adjustment::CapRpe(d) => {
                for it in ns.items.iter_mut() {
                    if let Some(n) = it
                        .intensity_label
                        .strip_prefix("RIR ")
                        .and_then(|s| s.trim().parse::<i32>().ok())
                    {
                        let capped = (n + *d as i32).clamp(0, 10);
                        it.intensity_label = format!("RIR {capped}");
                        if it.adjusted_note.is_empty() {
                            it.adjusted_note = format!("cap RPE −{d:.0}");
                        }
                        refresh_lift_summary(it);
                    }
                }
                applied.get_or_insert_with(|| to_view(r));
            }
            Adjustment::DowngradeSession => {
                for it in ns.items.iter_mut() {
                    if it.exercise.is_empty() {
                        // Actually CAP the run target: swap the hard
                        // intensity for an easy zone (keeping the bounded volume),
                        // not just add a note. The plan never renders a hard run
                        // above readiness.
                        cap_run_item_easy(it, r, "downgraded to an easier run: readiness");
                    } else {
                        // Cap the top-end: drop the heavy load, prescribe an easy
                        // proximity-to-failure day (never program above readiness).
                        it.load_kg = None;
                        it.intensity_label = "Easy: keep 3+ reps in reserve".into();
                        it.anchored_on.clear();
                        it.adjusted_note = "downgraded to an easier session: readiness".into();
                        refresh_lift_summary(it);
                    }
                }
                applied.get_or_insert_with(|| to_view(r));
            }
            Adjustment::ModifyAndMonitor => {
                // Tolerable, stable pain (Table 4.1 / safety-039): "modify
                // the provoking movement & monitor; avoid complete rest". Never
                // program a top-end into active pain (HARD RULE 3): pull the heavy
                // load off every lift and keep it light/pain-free, and cap runs to
                // easy. No fabricated % (the KB states none for this response).
                let note = format!("modify & monitor: pain ({})", claim_id_of(r));
                for it in ns.items.iter_mut() {
                    if it.exercise.is_empty() {
                        cap_run_item_easy(it, r, "modify & monitor: keep it easy and pain-free");
                    } else {
                        it.load_kg = None;
                        it.intensity_label = "Light: modify the movement, keep it pain-free".into();
                        it.anchored_on.clear();
                        it.adjusted_note = note.clone();
                        refresh_lift_summary(it);
                    }
                }
                applied.get_or_insert_with(|| to_view(r));
            }
            _ => {}
        }
    }
    if let Some(adj) = applied {
        ns.status = "adjusted".into();
        ns.adjustment = Some(adj);
    }
}

/// The backing claim id of an adjustment (for the `adjusted_note`), or `"-"`.
fn claim_id_of(r: &Recommended<Adjustment>) -> String {
    r.evidence
        .citation
        .claim_id
        .clone()
        .unwrap_or_else(|| "-".into())
}

/// The program summary card, graded with the plan's representative claim. `week`
/// is the current program week (advances with `SetToday`), 1-based.
fn build_program_summary(
    profile: &Profile,
    meso: &Mesocycle,
    program: &Program,
    week: u8,
    maintenance: bool,
) -> ProgramSummaryView {
    let g = graded((), crate::plan::summary_claim(profile));
    let goal_h = human_goal(&program.goal);
    ProgramSummaryView {
        name: program.name.clone(),
        goal: goal_h.clone(),
        phase: format!("{:?}", meso.phase),
        week,
        weeks_total: meso.weeks,
        maintenance,
        grade: format!("{:?}", g.evidence.grade),
        citation: g.evidence.citation.reference.clone(),
        confidence: g.confidence.score,
        safety_critical: g.confidence.safety_critical,
        contested: g.confidence.contested,
        why: why_from(
            Some(format!(
                "A {goal_h} week built from your profile and logged training."
            )),
            &g,
        ),
    }
}

/// Build the whole plan surface (next session + week strip + summary), rendered
/// STRICTLY downstream of the safety gates (HARD RULE 3): a `train_blocked` hold
/// blanks the NEXT SESSION only (status "blocked", no load numbers) and mirrors
/// that blanked row back into the week strip, the other week days keep their
/// planned items (a shell must not render the next session's numbers under a
/// hold, but the week outline stays visible). Otherwise the active readiness +
/// non-blocking review adjustments are folded into today's items. `None`/empty
/// when no plan is set or the profile raises an onboarding gate.
fn build_plan_views(
    model: &Model,
    train_blocked: bool,
    recommended: &[Recommended<Adjustment>],
    review_recs: &[Recommended<Adjustment>],
) -> (Option<SessionPlanView>, Vec<SessionPlanView>, Option<ProgramSummaryView>) {
    let (Some(profile), Some(req)) = (model.profile.as_ref(), model.plan_request) else {
        return (None, Vec::new(), None);
    };
    let start = req.start_epoch_day;
    let today = model.today_epoch_day.unwrap_or(start);
    let mut anchors = build_plan_anchors(&model.sets, profile.weeks_off);
    // Make the plan reactive to logged run history: anchor the long run to
    // demonstrated recent capacity and size weekly volume to measured mileage.
    let (longest_recent, recent_weekly, detrained) =
        build_run_anchors(&model.runs, today, model.today_utc_offset_sec);
    anchors.longest_recent_run_km = longest_recent;
    anchors.recent_weekly_km = recent_weekly;
    anchors.longest_run_detrained = detrained;
    let Some(program) = crate::plan::synthesize(profile, &anchors, req.start_epoch_day) else {
        return (None, Vec::new(), None);
    };
    // Saturating so a corrupt wire `today`/`start` near i64::MIN/MAX can't
    // debug-overflow (a panic would brick every later view via the FFI firewall).
    let week_monday = today.saturating_sub(mon0_weekday(today));
    let meso = &program.mesocycles[0];

    let mut week: Vec<SessionPlanView> = meso
        .sessions
        .iter()
        .map(|s| {
            let epoch_day = week_monday.saturating_add(s.day as i64);
            flatten_session(s, epoch_day, today, start, &anchors, model)
        })
        .collect();
    week.sort_by_key(|s| s.epoch_day);

    // Next non-Rest session: earliest with epoch_day ≥ today; else wrap to next
    // week's first training day.
    let mut next_session = match week
        .iter()
        .find(|s| s.epoch_day >= today && !s.items.is_empty() && s.status != "done")
    {
        Some(s) => {
            let mut n = s.clone();
            n.status = "next".into();
            Some(n)
        }
        None => week.iter().find(|s| !s.items.is_empty()).map(|s| {
            let mut n = s.clone();
            n.epoch_day = n.epoch_day.saturating_add(7);
            n.status = "next".into();
            n
        }),
    };

    if let Some(ns) = next_session.as_mut() {
        if train_blocked {
            // HARD RULE 3: never program through a hold. Blank the session.
            ns.status = "blocked".into();
            ns.items.clear();
            ns.adjustment = None;
        } else if ns.epoch_day <= today {
            // Fold BOTH the readiness adjustments AND the non-blocking
            // review-channel deloads/downgrades (rpe-load-gap / velocity /
            // failed-session / MRV deloads, HRV downgrade) into the rendered
            // session, so the NextSessionCard loads match a headline that says
            // "deload". Readiness recs go first, so they win the single displayed
            // `adjustment` slot; blocking review recs (Stop/RestDay/Defer) fall
            // through `apply_adjustments_to_session`'s catch-all untouched (a
            // blocking one would have set `train_blocked` and blanked the session
            // above).
            let mut folded: Vec<Recommended<Adjustment>> = recommended.to_vec();
            folded.extend(review_recs.iter().cloned());
            apply_adjustments_to_session(ns, &folded);
        }
        // LOW (external review): when no session remains this week the next
        // session WRAPS to next week (epoch_day += 7); more generally the earliest
        // upcoming training day can fall on a later date than today. Today's
        // readiness/review adjustments are transient, a load cut earned by
        // today's low HRV must NOT be painted onto a session dated beyond today
        // (a wrong-week deload). So folding is gated to `ns.epoch_day <= today`;
        // a future-dated session keeps its clean, un-adjusted prescription. A
        // BLOCKING/safety hold is unaffected (it blanks the session above,
        // regardless of date, HARD RULE 3).
        // Mirror the resolved next-session row back into the week strip.
        if let Some(row) = week.iter_mut().find(|s| s.epoch_day == ns.epoch_day) {
            *row = ns.clone();
        }
    }

    // The program week ADVANCES with the shell's clock: week N = whole weeks
    // elapsed since the plan's start, 1-based (no week 0). Driven by SetToday, not
    // hardcoded to 1. Once past the last block week the plan is a repeated
    // maintenance cycle (plan.rs owns real progression/deload/taper), so the week
    // number CYCLES 1..weeks (via `rem_euclid`) instead of pinning at "week N of
    // N" forever, and `maintenance` is flagged so a shell can say so. Inside the
    // original block (`weeks_elapsed < weeks`) `rem_euclid` is the identity, so
    // the in-block week number is unchanged.
    let block_weeks = (meso.weeks as i64).max(1);
    let weeks_elapsed = today.saturating_sub(start).div_euclid(7).max(0);
    let maintenance = weeks_elapsed >= block_weeks;
    let week_num = (weeks_elapsed.rem_euclid(block_weeks) as u8) + 1;
    let program_view = Some(build_program_summary(
        profile, meso, &program, week_num, maintenance,
    ));
    (next_session, week, program_view)
}

fn build_headline(
    train_blocked: bool,
    gates: &[Recommended<Adjustment>],
    recommended: &[Recommended<Adjustment>],
    review_recs: &[Recommended<Adjustment>],
    adjustments: &[AdjustmentView],
    review_adjustments: &[AdjustmentView],
    feedback: Option<&FeedbackView>,
    prescription: Option<&SessionPlanView>,
) -> TodayHeadlineView {
    let from_adjustment = |kind: &str, v: &AdjustmentView| TodayHeadlineView {
        kind: kind.to_string(),
        summary: v.summary.clone(),
        grade: v.grade.clone(),
        citation: v.citation.clone(),
        confidence: v.confidence,
        safety_critical: v.safety_critical,
        contested: v.contested,
        why: v.why.clone(),
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
            return from_adjustment("safety_hold", &to_view(stop));
        }
    }
    // The safety/adjustment call OUTRANKS the prescription rung. When any
    // readiness/pain adjustment is active (e.g. a tolerable-pain day), the
    // headline IS that call, "modify & monitor", "easier session", never the
    // full-load next session. The adjustment is still folded INTO the rendered
    // session downstream, but the headline must not read as an unqualified
    // "Next: full session" while pain/readiness is capping it.
    let best = adjustments
        .iter()
        .chain(review_adjustments.iter())
        .max_by(|a, b| {
            (a.safety_critical, grade_rank(&a.grade))
                .cmp(&(b.safety_critical, grade_rank(&b.grade)))
                .then(
                    a.confidence
                        .partial_cmp(&b.confidence)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });
    if let Some(v) = best {
        return from_adjustment("adjustment", v);
    }
    // Prescription rung: with no active adjustment, the highest
    // non-safety call is the concrete next session. Only when a plan is set and
    // not blocked; otherwise fall through to feedback/all-clear. Old shells that
    // don't know the "prescription" kind still render `summary`, a correct
    // sentence.
    if let Some(ns) = prescription {
        if ns.status != "blocked" {
            if let Some(first) = ns.items.first() {
                return TodayHeadlineView {
                    kind: "prescription".to_string(),
                    summary: format!("Next: {}. {}", ns.title, first.summary),
                    grade: first.grade.clone(),
                    citation: first.citation.clone(),
                    confidence: first.confidence,
                    safety_critical: first.safety_critical,
                    contested: first.contested,
                    why: first.why.clone(),
                };
            }
        }
    }
    if let Some(fb) = feedback {
        return TodayHeadlineView {
            kind: "feedback".to_string(),
            summary: fb.message.clone(),
            grade: fb.grade.clone(),
            citation: fb.citation.clone(),
            confidence: fb.confidence,
            safety_critical: fb.safety_critical,
            contested: fb.contested,
            why: fb.why.clone(),
        };
    }
    TodayHeadlineView {
        kind: "all_clear".to_string(),
        // States the absence of any triggered rule, not a graded claim, so
        // no evidence tag is attached (empty grade; shells render no chip).
        summary: "Train as planned: no adjustment triggered.".to_string(),
        ..TodayHeadlineView::default()
    }
}

/// Flatten the autoreg per-signal states into wire rows, resolving each row's
/// judging claim to its registry evidence (rows with no judging rule keep an
/// empty tag, they state facts, not recommendations).
fn build_readiness_summary(
    inputs: &[ReadinessInput],
    goal: Option<&Goal>,
    high_load_block: bool,
) -> Vec<ReadinessSignalView> {
    autoreg::signal_states(inputs, goal, high_load_block)
        .into_iter()
        .map(|s| {
            let (grade, citation, confidence, safety_critical, contested) = match s.claim {
                Some(id) => {
                    let g = graded((), id);
                    (
                        format!("{:?}", g.evidence.grade),
                        g.evidence.citation.reference.clone(),
                        g.confidence.score,
                        g.confidence.safety_critical,
                        g.confidence.contested,
                    )
                }
                None => (String::new(), String::new(), 0.0, false, false),
            };
            ReadinessSignalView {
                signal: format!("{:?}", s.signal),
                group: autoreg::signal_group(s.signal).to_string(),
                value: s.value,
                streak: s.streak,
                state: s.state,
                detail: s.detail,
                grade,
                citation,
                confidence,
                safety_critical,
                contested,
            }
        })
        .collect()
}

/// Static signal→group metadata for every readiness signal, in picker order
/// (metrics before the red-flag block).
fn build_signal_groups() -> Vec<SignalGroupView> {
    // P2: static signal→group metadata, identical every call, memoize (see
    // `build_reference`).
    static CACHE: OnceLock<Vec<SignalGroupView>> = OnceLock::new();
    CACHE.get_or_init(build_signal_groups_impl).clone()
}

fn build_signal_groups_impl() -> Vec<SignalGroupView> {
    autoreg::ALL_SIGNALS
        .iter()
        .map(|&s| SignalGroupView {
            signal: format!("{s:?}"),
            group: autoreg::signal_group(s).to_string(),
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
        entry_id: s.entry_id,
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
        // Default basis = the summary itself (the rendered call). The claim
        // statement is appended for context; the improves/grade lines carry the
        // KB-grounded explanation.
        why: why_from(None, r),
    }
}

/// KB definition of what each evidence grade means (File 09), plus the default
/// confidence it maps to, the data behind the "How evidence grading works"
/// legend sheet. `MarketingMyth` is included so the legend can name the
/// hard-blocked bottom of the scale, but it is never emitted on a card.
fn grade_definitions() -> Vec<GradeDefView> {
    // P2: the File 09 grade legend is a fixed five-row table, memoize (see
    // `build_reference`).
    static CACHE: OnceLock<Vec<GradeDefView>> = OnceLock::new();
    CACHE.get_or_init(grade_definitions_impl).clone()
}

fn grade_definitions_impl() -> Vec<GradeDefView> {
    [
        EvidenceGrade::Strong,
        EvidenceGrade::Moderate,
        EvidenceGrade::Weak,
        EvidenceGrade::ExpertOpinion,
        EvidenceGrade::MarketingMyth,
    ]
    .into_iter()
    .map(|g| {
        let (label, definition) = match g {
            EvidenceGrade::Strong => (
                "Strong",
                "Well-replicated meta-analyses or randomized controlled trials.",
            ),
            EvidenceGrade::Moderate => (
                "Moderate",
                "Mixed or limited randomized trials: promising but not yet settled.",
            ),
            EvidenceGrade::Weak => (
                "Weak",
                "Mechanistic or observational evidence only, not direct trials on this outcome.",
            ),
            EvidenceGrade::ExpertOpinion => (
                "Expert opinion",
                "A practice heuristic with no direct trial evidence yet.",
            ),
            EvidenceGrade::MarketingMyth => (
                "Marketing myth",
                "Contradicted or retracted: hard-blocked, never programmed.",
            ),
        };
        GradeDefView {
            grade: format!("{g:?}"),
            label: label.to_string(),
            definition: definition.to_string(),
            confidence: g.default_confidence(),
        }
    })
    .collect()
}

/// The KB statement registered for a claim id, the authoritative plain-language
/// description of the rule/method, reused as a why? `basis` fallback so the
/// disclosure never invents a claim (HARD RULE 1).
fn claim_statement(claim_id: Option<&str>) -> Option<&'static str> {
    claim_id
        .and_then(crate::evidence::claim)
        .map(|c| c.statement)
}

/// Compose the "why?" triad from a `Recommended<T>`, optionally overriding the
/// `basis` line with a datum-rich, call-site-specific sentence (e.g. the
/// HR-zones card restating the user's age through the Tanaka formula). When no
/// override is given, the basis is the backing claim's own registered statement
/// (or its citation as a last resort), always KB-grounded.
fn why_from<T>(basis_override: Option<String>, r: &Recommended<T>) -> WhyView {
    let claim_id = r.evidence.citation.claim_id.as_deref();
    let basis = basis_override.unwrap_or_else(|| {
        claim_statement(claim_id)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Based on {}", r.evidence.citation.reference))
    });
    WhyView {
        basis,
        grade_note: grade_note_str(
            &format!("{:?}", r.evidence.grade),
            r.confidence.contested,
            r.confidence.contested_question_ref.as_deref(),
        ),
        improves: improves_for(claim_id),
    }
}

/// One-sentence gloss of what an evidence grade means for a claim, with the
/// contested question appended when the claim is under active debate. Factual
/// rows (empty grade string) get an empty note: they judge nothing.
fn grade_note_str(grade: &str, contested: bool, contested_ref: Option<&str>) -> String {
    let base = match grade {
        "Strong" => {
            "Strong evidence: backed by well-replicated meta-analyses or randomized trials."
        }
        "Moderate" => {
            "Moderate evidence: mixed or limited randomized trials; promising but not settled."
        }
        "Weak" => {
            "Weak evidence: from mechanism or observation, not direct trials on this outcome."
        }
        "ExpertOpinion" => "Expert opinion: a practice heuristic with no direct trial evidence yet.",
        _ => "",
    };
    if base.is_empty() {
        return String::new();
    }
    if contested {
        match contested_ref.and_then(crate::evidence::contested_question) {
            // #4: honour the deck's "here's both sides" promise, name the open
            // question in plain prose AND show the engine's current lean, rather
            // than dangling a bare topic label. `strip_file_ref` defensively
            // removes any trailing internal "(File 0x local CQ-NN)" shorthand so
            // doc-speak never reaches the user even if a future KB edit adds it.
            Some(cq) => {
                let mut s = format!(
                    "{base} This one is genuinely contested: experts differ on {}. Our current lean: {}.",
                    strip_file_ref(cq.question),
                    cq.engine_default
                );
                // #4: when the KB supplies an attributable opposing reference,
                // show one contradicting side so the disclosure honours the
                // deck's "here's both sides" promise. Left off entirely when the
                // CQ has no KB-sourced opposing cite (`other_side` is None) -
                // never a fabricated "other view".
                if let Some(other) = cq.other_side {
                    s.push_str(&format!(" One view on the other side: {other}"));
                }
                s
            }
            None => format!(
                "{base} This one is genuinely contested. We follow the best available evidence and will update as it settles."
            ),
        }
    } else {
        base.to_string()
    }
}

/// Defensively strip a trailing internal doc reference like ` (File 03 local
/// CQ-04)` from a contested-question label before it reaches a user. The KB
/// question strings are plain today; this keeps a future edit from leaking
/// file/CQ shorthand into the disclosure (#4).
fn strip_file_ref(s: &str) -> &str {
    let trimmed = s.trim_end();
    if trimmed.ends_with(')') {
        if let Some(open) = trimmed.rfind(" (File ") {
            return trimmed[..open].trim_end();
        }
    }
    trimmed
}

/// The engagement-loop line for the "why?" disclosure: which input the rule
/// lacks or would sharpen. Keyed by claim id, this describes the ENGINE's own
/// data needs, never training advice, so it carries no evidence tag and
/// invents no claim (HARD RULE 1-safe by construction; every string is reviewed
/// against that bar). `"-"` when nothing would genuinely improve the estimate.
fn improves_for(claim_id: Option<&str>) -> String {
    let line = match claim_id {
        Some("RUN-HRMAX-001") => {
            "Log a measured max HR from an all-out effort to replace the age-based estimate."
        }
        Some("RUN-VDOT-001") => {
            "Add a recent race time so these zones anchor to your own VDOT instead of a default."
        }
        Some("HRV-001") | Some("AUTOREG-HRV-SAT-001") => {
            "Keep logging morning check-ins. More readings tighten the HRV baseline this compares against."
        }
        Some("WELLNESS-001") | Some("AUTOREG-WELLNESS-RHR-001") => {
            "A couple more weeks of morning check-ins tighten your wellness baseline."
        }
        Some("AUTOREG-RHR-DOWN-001") | Some("AUTOREG-RHR-STOP-001") => {
            "Log your resting HR a few more mornings to sharpen the baseline this is measured against."
        }
        Some("RUN-DECOUPLE-001") => {
            "Log more long runs with heart rate to track how your aerobic base is trending."
        }
        Some("STR-PRILEPIN-001") => {
            "Log your working sets so session volume can be checked against the target band."
        }
        Some("AUTOREG-PCT-001") | Some("AUTOREG-E1RM-GATE-001") => {
            "Log a few more sessions at this intensity to confirm the trend."
        }
        _ => "-",
    };
    line.to_string()
}

/// Human-readable one-liner for an adjustment.
fn describe(a: &Adjustment) -> String {
    match a {
        Adjustment::ReduceLoadPct(p) => format!("Reduce load {p:.0}% for remaining sets"),
        // Never folded into the rendered session (the safe choice, an increase is
        // opt-in, unlike a safety cut), so the copy tells the user to apply it.
        Adjustment::IncreaseLoadPct(p) => {
            format!("Increase load {p:.0}%. Readiness is high; add it manually next session")
        }
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
            "Modify the provoking exercise and continue with monitoring. Avoid complete rest"
                .into()
        }
        Adjustment::RestDay => "Take a full rest day".into(),
        Adjustment::Stop => "Stop: do not train".into(),
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

    // ── Run-event constructor helpers ────────────────────────────────────────
    // The `LogRun`/`LogRunTrack`/`AmendRun` struct-enum variants have no
    // `Default`, so every literal construction site breaks when a field is
    // added. Routing all test construction through these helpers isolates that
    // churn to one place: adding an optional field edits only the helper
    // body, never the ~30 call sites. Positional args mirror the variant fields.
    fn log_run(
        distance_km: f64,
        duration_min: f64,
        hr_pct_max: f64,
        longest_recent_km: f64,
        observed_at: i64,
        entry_id: u64,
    ) -> Event {
        Event::LogRun {
            distance_km,
            duration_min,
            hr_pct_max,
            longest_recent_km,
            observed_at,
            entry_id,
            workout_type: None,
        }
    }

    fn log_run_track(
        points: Vec<GpsPoint>,
        hr_pct_max: f64,
        longest_recent_km: f64,
        observed_at: i64,
        entry_id: u64,
    ) -> Event {
        Event::LogRunTrack {
            points,
            hr_pct_max,
            longest_recent_km,
            observed_at,
            entry_id,
            workout_type: None,
            segment_starts: Vec::new(),
        }
    }

    /// [`log_run_track`] with explicit pause-bridge boundaries.
    #[allow(dead_code)]
    fn log_run_track_seg(
        points: Vec<GpsPoint>,
        hr_pct_max: f64,
        longest_recent_km: f64,
        observed_at: i64,
        entry_id: u64,
        segment_starts: Vec<u32>,
    ) -> Event {
        Event::LogRunTrack {
            points,
            hr_pct_max,
            longest_recent_km,
            observed_at,
            entry_id,
            workout_type: None,
            segment_starts,
        }
    }

    #[allow(dead_code)]
    fn amend_run(
        entry_id: u64,
        distance_km: f64,
        duration_min: f64,
        hr_pct_max: f64,
        longest_recent_km: f64,
        observed_at: i64,
        observed_at_fallback: i64,
    ) -> Event {
        Event::AmendRun {
            entry_id,
            distance_km,
            duration_min,
            hr_pct_max,
            longest_recent_km,
            observed_at,
            observed_at_fallback,
            workout_type: None,
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
        match serde_json::from_str::<Event>(profile).expect("profile wire form must parse") {
            Event::SetProfile(p) => {
                // The person fields are additive: an older profile line
                // (nine fields, no person data) must replay with them absent.
                assert_eq!(p.age_years, None, "old profile must default age to None");
                assert_eq!(p.resting_hr_bpm, None);
                assert_eq!(p.measured_hr_max, None);
                assert_eq!(p.bodyweight_kg, None);
                assert!(!p.female);
            }
            other => panic!("expected SetProfile, got {other:?}"),
        }
    }

    /// The consolidated person fields (age, bodyweight, sex, resting
    /// HR, measured HRmax) round-trip through the `SetProfile` wire exactly as
    /// Core.kt's guided setup / profile editor emit them. All are `#[serde(default)]`
    /// so the shell omits absent ones (previous test) and sends present ones here.
    #[test]
    fn set_profile_person_fields_round_trip() {
        let wire = r#"{"SetProfile":{"progression_cadence":"EverySession","lift_goal":"Hypertrophy","goal_distance":"General","concurrent_goal":"Hypertrophy","weekly_sets":10,"running_days_per_week":0,"running_km_per_week":0.0,"advanced":false,"endurance_intensity_pct_vo2max":75.0,"female":true,"bodyweight_kg":62.5,"age_years":34.0,"resting_hr_bpm":54.0,"measured_hr_max":188.0}}"#;
        match serde_json::from_str::<Event>(wire).expect("person-field profile must parse") {
            Event::SetProfile(p) => {
                assert!(p.female);
                assert_eq!(p.bodyweight_kg, Some(62.5));
                assert_eq!(p.age_years, Some(34.0));
                assert_eq!(p.resting_hr_bpm, Some(54.0));
                assert_eq!(p.measured_hr_max, Some(188.0));
            }
            other => panic!("expected SetProfile, got {other:?}"),
        }

        // And the reverse: a constructed Profile serializes those fields under the
        // snake_case names the shell parses back (view exposes `profile` verbatim).
        let p = Profile {
            age_years: Some(34.0),
            resting_hr_bpm: Some(54.0),
            measured_hr_max: Some(188.0),
            bodyweight_kg: Some(62.5),
            female: true,
            ..sample_profile()
        };
        let json = serde_json::to_string(&p).expect("profile serializes");
        for key in [
            "\"age_years\":34.0",
            "\"resting_hr_bpm\":54.0",
            "\"measured_hr_max\":188.0",
            "\"bodyweight_kg\":62.5",
            "\"female\":true",
        ] {
            assert!(json.contains(key), "serialized profile missing {key}: {json}");
        }
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
            .find(|a| a.summary.contains("Avoid complete rest"))
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
        assert_eq!(vm.adjustments[0].summary, "Stop: do not train");
    }

    // --- Humanized readiness via the morning check-in ---

    fn checkin_ev(day: i64, sleep: u8, sore: u8, mood: u8) -> Event {
        Event::SubmitCheckin(CheckinInput {
            observed_at: day * 86_400 + 100,
            sleep_quality: Some(sleep),
            soreness: Some(sore),
            mood: Some(mood),
            resting_hr_bpm: None,
            hrv_rmssd_ms: None,
        })
    }

    #[test]
    fn a_single_checkin_shows_building_baseline_not_a_fabricated_z() {
        // The acceptance path: a fresh user does one morning check-in (sleep 2
        // / soreness 4 / mood 3). Today must show an honest "collecting baseline"
        // state and NO derived readiness signal, never a made-up z-score, and
        // the user is never asked to enter one.
        let app = Engine;
        let mut model = Model::default();
        app.update(checkin_ev(0, 2, 4, 3), &mut model).expect_only_render();

        let vm = app.view(&model);
        assert!(!vm.train_blocked);
        // The check-in is echoed so the shell can rehydrate + show it's recorded.
        let echo = vm.checkin_today.expect("checkin echoed");
        assert_eq!(echo.sleep_quality, Some(2));
        assert_eq!(echo.soreness, Some(4));
        // Honest collecting status, no derived WellnessZ row yet.
        let status = vm
            .baseline_status
            .iter()
            .find(|b| b.signal == "WellnessZ")
            .expect("wellness baseline status");
        assert_eq!(status.have, 1);
        assert_eq!(status.need, 7);
        assert!(status.note.contains("Collecting your baseline"));
        assert!(
            !vm.readiness_summary.iter().any(|r| r.signal == "WellnessZ"),
            "no fabricated wellness z before the baseline exists"
        );
    }

    #[test]
    fn seven_checkins_build_a_baseline_and_surface_a_human_derived_state() {
        // Repeat the check-in across enough days (backdated) and the baseline
        // kicks in: the core derives a WellnessZ from the history and surfaces a
        // per-signal STATE in the summary, still no z-entry by the user.
        let app = Engine;
        let mut model = Model::default();
        // Six good, slightly varying baseline days, then a rough seventh.
        for d in 0..6 {
            let ev = if d % 2 == 0 {
                checkin_ev(d, 5, 2, 4)
            } else {
                checkin_ev(d, 4, 3, 4)
            };
            app.update(ev, &mut model).expect_only_render();
        }
        app.update(checkin_ev(6, 2, 4, 3), &mut model).expect_only_render();

        let vm = app.view(&model);
        // Baseline is built → the collecting status is gone for wellness…
        assert!(
            !vm.baseline_status.iter().any(|b| b.signal == "WellnessZ"),
            "wellness baseline is ready, no longer collecting"
        );
        // …and a human-derived wellness state now appears, judged by the SAME
        // KB rule (WELLNESS-001), carrying its evidence: no bare recommendation.
        let w = vm
            .readiness_summary
            .iter()
            .find(|r| r.signal == "WellnessZ")
            .expect("derived wellness row");
        assert_eq!(w.state, "suppressed", "rough morning vs baseline is suppressed");
        assert!(!w.grade.is_empty(), "derived state carries the rule's evidence");
        // The derived signal drives today's call (a downgrade-class adjustment).
        assert_eq!(vm.today_headline.kind, "adjustment");
    }

    #[test]
    fn checkins_and_old_readiness_logs_coexist() {
        // Backward compat: the retained check-in history and the legacy
        // day-scoped raw-signal path both feed the rules; ClearReadiness must
        // NOT wipe the multi-day check-in baseline.
        let app = Engine;
        let mut model = Model::default();
        for d in 0..7 {
            app.update(checkin_ev(d, 4, 3, 4), &mut model).expect_only_render();
        }
        // A legacy raw advanced-mode signal on top.
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Soreness, 6.0)),
            &mut model,
        )
        .expect_only_render();
        // Clearing readiness drops the raw signal but keeps the check-in history.
        app.update(Event::ClearReadiness, &mut model).expect_only_render();

        let vm = app.view(&model);
        assert!(
            vm.checkin_today.is_some(),
            "ClearReadiness must not wipe the check-in history"
        );
        assert!(
            !vm.readiness_summary.iter().any(|r| r.signal == "Soreness"),
            "the manually-logged raw Soreness was cleared"
        );
        // ClearCheckins is the only thing that drops the baseline.
        app.update(Event::ClearCheckins, &mut model).expect_only_render();
        let vm = app.view(&model);
        assert!(vm.checkin_today.is_none());
        assert!(vm.baseline_status.is_empty());
    }

    #[test]
    fn pain_location_surfaces_on_the_readiness_row_detail_through_the_view() {
        // A characterized pain report with a body-part location must reach the
        // shell's DO-NOT-TRAIN banner sub-line via ReadinessSignalView.detail;
        // an identical report with no location must leave detail body-part-free
        // (never fabricated, HARD RULE 1).
        let pain_row = |json: &str| -> ReadinessSignalView {
            let app = Engine;
            let mut model = Model::default();
            let event: Event = serde_json::from_str(json).expect("pain event parses");
            app.update(event, &mut model).expect_only_render();
            app.view(&model)
                .readiness_summary
                .into_iter()
                .find(|r| r.signal == "Pain")
                .expect("pain row present")
        };

        let with_loc = pain_row(
            r#"{"SubmitReadiness":{"signal":"Pain","value":1.0,"observed_at":0,"pain":{"kind":"SharpJoint","severity":6,"trend":"Stable","location":"Left knee"}}}"#,
        );
        assert!(
            with_loc.detail.contains("Left knee"),
            "banner sub-line must name the body part, got {:?}",
            with_loc.detail
        );

        let no_loc = pain_row(
            r#"{"SubmitReadiness":{"signal":"Pain","value":1.0,"observed_at":0,"pain":{"kind":"SharpJoint","severity":6,"trend":"Stable"}}}"#,
        );
        assert!(
            !no_loc.detail.to_lowercase().contains("knee"),
            "no location must be invented when None, got {:?}",
            no_loc.detail
        );
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
                entry_id: 0,
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

    // ── Entry ids + DeleteEntry / AmendSet / AmendRun ──────────

    fn log_set_id(exercise: &str, weight_kg: f64, id: u64, observed_at: i64) -> Event {
        Event::LogSet {
            exercise: exercise.into(),
            weight_kg,
            reps: 5,
            rpe: 8.0,
            observed_at,
            entry_id: id,
        }
    }

    #[test]
    fn view_echoes_the_entry_id_for_the_shell() {
        let app = Engine;
        let mut model = Model::default();
        app.update(log_set_id("Bench", 100.0, 4242, 0), &mut model)
            .expect_only_render();
        assert_eq!(app.view(&model).lifts[0].entry_id, 4242);
    }

    #[test]
    fn delete_entry_removes_the_targeted_set_by_id() {
        let app = Engine;
        let mut model = Model::default();
        app.update(log_set_id("Bench", 100.0, 1, 0), &mut model)
            .expect_only_render();
        app.update(log_set_id("Squat", 140.0, 2, 100), &mut model)
            .expect_only_render();
        app.update(
            Event::DeleteEntry {
                kind: EntryKind::Set,
                entry_id: 1,
                observed_at_fallback: 0,
            },
            &mut model,
        )
        .expect_only_render();
        let lifts = app.view(&model).lifts;
        assert_eq!(lifts.len(), 1);
        assert_eq!(lifts[0].exercise, "Squat");
        assert_eq!(lifts[0].entry_id, 2);
    }

    #[test]
    fn amend_set_edits_fields_in_place_keeping_the_id() {
        let app = Engine;
        let mut model = Model::default();
        app.update(log_set_id("Bench", 100.0, 7, 0), &mut model)
            .expect_only_render();
        app.update(
            Event::AmendSet {
                entry_id: 7,
                exercise: "Bench".into(),
                weight_kg: 120.0,
                reps: 3,
                rpe: 9.0,
                observed_at: 0,
                observed_at_fallback: 0,
            },
            &mut model,
        )
        .expect_only_render();
        let lifts = app.view(&model).lifts;
        assert_eq!(lifts.len(), 1, "amend edits, never appends a second row");
        assert!((lifts[0].weight_kg - 120.0).abs() < f64::EPSILON);
        assert_eq!(lifts[0].reps, 3);
        assert_eq!(lifts[0].entry_id, 7, "the amended set keeps its id");
    }

    #[test]
    fn amend_after_delete_is_a_no_op_never_resurrects() {
        // Full prevention: amend is a STRICT update. Log→Delete→Amend must leave
        // the set deleted; an amend whose target no longer exists adds nothing
        // (previously it pushed a fresh row, resurrecting the deleted entry).
        let app = Engine;
        let mut model = Model::default();
        app.update(log_set_id("Bench", 100.0, 5, 0), &mut model)
            .expect_only_render();
        app.update(
            Event::DeleteEntry {
                kind: EntryKind::Set,
                entry_id: 5,
                observed_at_fallback: 0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::AmendSet {
                entry_id: 5,
                exercise: "Bench".into(),
                weight_kg: 120.0,
                reps: 5,
                rpe: 8.0,
                observed_at: 0,
                observed_at_fallback: 0,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(
            app.view(&model).lifts.is_empty(),
            "a strict amend on a deleted row is a no-op, not a resurrection"
        );

        // Same for a run.
        let mut model = Model::default();
        app.update(log_run_track(vec![
                    GpsPoint { lat: 0.0, lon: 0.0, observed_at: 0, accuracy_m: 5.0 },
                    GpsPoint { lat: 0.01, lon: 0.0, observed_at: 300, accuracy_m: 5.0 },
                ], 0.0, 0.0, 10, 42), &mut model)
            .expect_only_render();
        app.update(
            Event::DeleteEntry {
                kind: EntryKind::Run,
                entry_id: 42,
                observed_at_fallback: 0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::AmendRun {
                entry_id: 42,
                distance_km: 9.0,
                duration_min: 45.0,
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
                observed_at: 0,
                observed_at_fallback: 0,
                workout_type: None,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(
            app.view(&model).runs.is_empty(),
            "a strict run amend on a deleted row is a no-op"
        );
    }

    #[test]
    fn delete_entry_removes_a_gps_tracked_run() {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            log_run_track(vec![
                    GpsPoint { lat: 0.0, lon: 0.0, observed_at: 0, accuracy_m: 5.0 },
                    GpsPoint { lat: 0.01, lon: 0.0, observed_at: 300, accuracy_m: 5.0 },
                ], 0.0, 0.0, 10, 99),
            &mut model,
        )
        .expect_only_render();
        assert_eq!(app.view(&model).runs.len(), 1);
        app.update(
            Event::DeleteEntry {
                kind: EntryKind::Run,
                entry_id: 99,
                observed_at_fallback: 0,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(app.view(&model).runs.is_empty(), "the junk GPS run is gone");
    }

    #[test]
    fn delete_entry_falls_back_to_observed_at_for_a_legacy_row() {
        // A legacy set has entry_id 0; the shell targets it by observed_at.
        let app = Engine;
        let mut model = Model::default();
        app.update(log_set_id("Old", 100.0, 0, 5000), &mut model)
            .expect_only_render();
        app.update(
            Event::DeleteEntry {
                kind: EntryKind::Set,
                entry_id: 0,
                observed_at_fallback: 5000,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(app.view(&model).lifts.is_empty());
    }

    #[test]
    fn delete_entry_with_no_match_is_a_no_op() {
        let app = Engine;
        let mut model = Model::default();
        app.update(log_set_id("Bench", 100.0, 1, 0), &mut model)
            .expect_only_render();
        app.update(
            Event::DeleteEntry {
                kind: EntryKind::Set,
                entry_id: 555,
                observed_at_fallback: 0,
            },
            &mut model,
        )
        .expect_only_render();
        assert_eq!(app.view(&model).lifts.len(), 1, "unmatched delete changes nothing");
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
                entry_id: 0,
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
                    entry_id: 0,
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
            log_run(10.0, 50.0, 70.0, 12.0, 0, 0),
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
                log_run(10.0, 50.0, hr, 12.0, 0, 0),
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
            log_run(5.0, 0.0, 70.0, 6.0, 0, 0),
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
            log_run(5.0, 25.0, 70.0, 0.0, 0, 0),
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
            log_run(20.0, 100.0, 70.0, 0.0, 0, 0),
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
                entry_id: 0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            log_run(5.0, 25.0, 70.0, 12.0, 1_700_000_500, 0),
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.lifts[0].observed_at, 1_700_000_000);
        assert_eq!(vm.runs[0].observed_at, 1_700_000_500);

        // Absent from the wire (old persisted event) → 0, not a decode failure.
        // The view lists are chronological by observed_at, so the undated (0)
        // entry sorts BEFORE the dated set, not after it in append order.
        let undated: Event = serde_json::from_str(
            r#"{"LogSet":{"exercise":"Bench","weight_kg":60.0,"reps":8,"rpe":7.0}}"#,
        )
        .expect("pre-timestamp LogSet still decodes");
        app.update(undated, &mut model).expect_only_render();
        let vm = app.view(&model);
        assert_eq!(vm.lifts[0].observed_at, 0);
        assert_eq!(vm.lifts[0].exercise, "Bench");
        assert_eq!(vm.lifts[1].observed_at, 1_700_000_000);
        // entry_id is likewise serde-default: a legacy LogSet decodes with
        // id 0 (the shell then targets it by observed_at).
        assert_eq!(vm.lifts[0].entry_id, 0);
    }

    #[test]
    fn phase4_events_round_trip_the_wire_and_default_entry_id() {
        // LogSet with an entry_id encodes/decodes it; DeleteEntry / AmendSet /
        // AmendRun round-trip; observed_at_fallback / longest_recent_km / amend
        // observed_at are serde-default for forward-lean shells.
        let with_id: Event = serde_json::from_str(
            r#"{"LogSet":{"exercise":"Bench","weight_kg":60.0,"reps":8,"rpe":7.0,"observed_at":5,"entry_id":1717171717}}"#,
        )
        .expect("LogSet with entry_id decodes");
        match with_id {
            Event::LogSet { entry_id, .. } => assert_eq!(entry_id, 1_717_171_717),
            other => panic!("expected LogSet, got {other:?}"),
        }

        let del: Event =
            serde_json::from_str(r#"{"DeleteEntry":{"kind":"Run","entry_id":42}}"#)
                .expect("DeleteEntry decodes with default observed_at_fallback");
        match del {
            Event::DeleteEntry {
                kind,
                entry_id,
                observed_at_fallback,
            } => {
                assert_eq!(kind, EntryKind::Run);
                assert_eq!(entry_id, 42);
                assert_eq!(observed_at_fallback, 0);
            }
            other => panic!("expected DeleteEntry, got {other:?}"),
        }

        let amend: Event = serde_json::from_str(
            r#"{"AmendSet":{"entry_id":7,"exercise":"Squat","weight_kg":150.0,"reps":3,"rpe":9.0}}"#,
        )
        .expect("AmendSet decodes with default observed_at");
        assert!(matches!(amend, Event::AmendSet { entry_id: 7, .. }));

        let amend_run: Event = serde_json::from_str(
            r#"{"AmendRun":{"entry_id":8,"distance_km":6.0,"duration_min":30.0,"hr_pct_max":0.0}}"#,
        )
        .expect("AmendRun decodes with default longest_recent_km/observed_at");
        assert!(matches!(amend_run, Event::AmendRun { entry_id: 8, .. }));
    }

    #[test]
    fn workout_type_tag_round_trips_and_is_back_compat() {
        // The user-declared workout-type label is an additive
        // `#[serde(default)] Option<WorkoutType>` on LogRun/LogRunTrack/AmendRun.
        // (1) A tagged run decodes the label AND echoes it on the run view. (2) An
        // OLD-shape event with NO `workout_type` key still decodes (→ None), so
        // older persisted logs and shells replay unchanged (HARD RULE 1:
        // storage + display only, nothing here branches coaching on the tag).
        let app = Engine;

        // (2) back-compat: the exact old wire shape (no workout_type key) parses.
        let old_shape: Event = serde_json::from_str(
            r#"{"LogRun":{"distance_km":10.0,"duration_min":50.0,"hr_pct_max":70.0,"longest_recent_km":12.0,"observed_at":0,"entry_id":0}}"#,
        )
        .expect("pre-I16 LogRun (no workout_type) must still decode");
        match old_shape {
            Event::LogRun { workout_type, .. } => {
                assert_eq!(workout_type, None, "absent field defaults to None")
            }
            other => panic!("expected LogRun, got {other:?}"),
        }

        // (1) a tagged run decodes the label and the run view echoes it verbatim.
        let tagged: Event = serde_json::from_str(
            r#"{"LogRun":{"distance_km":8.0,"duration_min":48.0,"hr_pct_max":70.0,"longest_recent_km":12.0,"observed_at":0,"entry_id":0,"workout_type":"Interval"}}"#,
        )
        .expect("tagged LogRun must decode");
        assert!(matches!(
            tagged,
            Event::LogRun { workout_type: Some(WorkoutType::Interval), .. }
        ));

        let mut model = Model::default();
        app.update(tagged, &mut model).expect_only_render();
        assert_eq!(
            app.view(&model).runs[0].workout_type,
            Some(WorkoutType::Interval),
            "the view echoes the user's tag for history display"
        );

        // The bare variant name is the wire form the Kotlin shell mirrors by
        // `WorkoutType.name`: pin it so a rename fails here, not silently on-device.
        assert_eq!(
            serde_json::to_string(&WorkoutType::LongRun).unwrap(),
            "\"LongRun\""
        );

        // An untagged run leaves the field None on the view (never fabricated).
        let mut m2 = Model::default();
        app.update(log_run(5.0, 25.0, 70.0, 10.0, 0, 0), &mut m2)
            .expect_only_render();
        assert_eq!(app.view(&m2).runs[0].workout_type, None);
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
            log_run(10.0, 40.95, 70.0, 12.0, 0, 0),
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
            log_run(20.0, 100.0, 70.0, 0.0, 0, 0),
            &mut model,
        )
        .expect_only_render();

        // 10 km is well under the derived 20 km baseline → no spike.
        app.update(
            log_run(10.0, 50.0, 70.0, 0.0, 0, 0),
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
            log_run_track(vec![
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
                ], 0.0, 0.0, 0, 0),
            &mut model,
        )
        .expect_only_render();

        // Manual 1 km run: under the ~1.11 km baseline derived from the GPS track,
        // so no spike, but only because the baseline came from the GPS run.
        app.update(
            log_run(1.0, 6.0, 70.0, 0.0, 0, 0),
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
            log_run(20.0, 100.0, 75.0, 12.0, 0, 0),
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
            log_run_track(points, 70.0, 12.0, 0, 0),
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
            log_run_track(points, 70.0, 12.0, 0, 0),
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
            log_run_track(points, 0.0, 0.0, 0, 0),
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
            log_run_track(one_km(), 0.0, 0.0, 0, 0),
            &mut model,
        )
        .expect_only_render();
        assert!(app.view(&model).runs[0].spike_flag);

        // Run 2: same ~1.11 km. Shell again sends baseline 0.0, but the core now
        // derives the baseline from run 1 (~1.11 km): 1.11 is not >10 % over
        // itself, so NO spike. This only passes if the baseline came from history.
        app.update(
            log_run_track(one_km(), 0.0, 0.0, 0, 0),
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
            log_run_track(long, 0.0, 0.0, 0, 0),
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
            age_years: None,
            resting_hr_bpm: None,
            measured_hr_max: None,
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
    fn power_guidance_row_states_kb_language_not_the_rir_number() {
        // The Power loading row is cited to STR-PWR-001 (Moderate). The numeric
        // RIR 3-5 band is an expert-opinion encoding (STR-PWR-RIR-001), so it
        // must NOT print under this row; the row states the KB's qualitative
        // "never to failure" instruction instead.
        let mut p = sample_profile();
        p.lift_goal = LiftGoal::Power;
        let rows = build_guidance(&p);
        let loading = rows
            .iter()
            .find(|r| r.summary.contains("Power loading:"))
            .expect("a Power loading guidance row exists");
        assert!(
            loading.summary.contains("never to failure"),
            "row states the KB power instruction: {}",
            loading.summary
        );
        assert!(
            !loading.summary.contains("RIR "),
            "no expert-opinion RIR digits under the STR-PWR-001 row: {}",
            loading.summary
        );
    }

    #[test]
    fn c25k_running_guidance_row_has_no_long_run_fragment() {
        // A4: C25K carries no long-run share (running-025 is run/walk to 30 min),
        // so the Running guidance row must not print a "long run …%" fragment.
        // B2: the goal distance renders as the human label "Couch to 5K", never
        // the raw Debug enum name "C25k".
        let mut p = sample_profile();
        p.goal_distance = GoalDistance::C25k;
        let rows = build_guidance(&p);
        let running = rows
            .iter()
            .find(|r| r.summary.starts_with("Couch to 5K:"))
            .expect("a C25K running guidance row exists");
        assert!(
            !running.summary.contains("long run"),
            "C25K has no long-run share, so no long-run fragment: {}",
            running.summary
        );
        assert!(
            running.summary.contains("3-3 sessions/wk"),
            "the C25K session budget still renders: {}",
            running.summary
        );
        // The raw Debug enum name never leaks into any guidance summary.
        assert!(
            !rows.iter().any(|r| r.summary.contains("C25k")),
            "no PascalCase enum name in the guidance copy"
        );
    }

    #[test]
    fn guidance_copy_uses_human_labels_not_debug_enums() {
        // B2: the periodization, goal-distance and same-session-order rows render
        // human labels, never the raw Debug enum names ("Dup"/"FiveK"/"RunFirst").
        let mut p = sample_profile(); // WeekToWeek cadence → DUP; EndurancePriority.
        p.goal_distance = GoalDistance::FiveK;
        let rows = build_guidance(&p);
        let any = |needle: &str| rows.iter().any(|r| r.summary.contains(needle));
        assert!(
            any("Periodization model: daily undulating (DUP)"),
            "periodization human label"
        );
        assert!(any("5K: "), "goal-distance human label in the running row");
        assert!(
            any("Combined days: running first is fine (endurance priority)."),
            "same-session-order full sentence"
        );
        // No PascalCase enum name leaks into any guidance summary.
        for bad in ["Dup", "FiveK", "RunFirst", "EndurancePriority"] {
            assert!(!any(bad), "raw Debug enum name '{bad}' leaked into guidance copy");
        }
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

    // --- The three-part "why?" disclosure + grade legend ---

    #[test]
    fn why_disclosure_on_hr_zones_carries_basis_grade_note_and_improves() {
        // The device-verification target: expanding "why?" on the HRmax card
        // shows the Tanaka basis, a grade rationale, and the engagement line
        // ("log a measured max HR to improve"). None of these three strings may
        // be empty on the HRmax action row.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::ComputeHrZones {
                age_years: 30.0,
                resting_hr_bpm: None,
                weeks_since_recalc: None,
                weeks_since_pace_test: None,
            },
            &mut model,
        )
        .expect_only_render();

        let hrmax = &app.view(&model).hr_zones[0];
        let why = &hrmax.why;
        // basis: datum-rich Tanaka restatement using the user's own age.
        assert!(
            why.basis.contains("Tanaka") && why.basis.contains("30"),
            "basis restates the rule + the user's datum: {}",
            why.basis
        );
        // grade_note: RUN-HRMAX-001 is Weak → the Weak gloss.
        assert_eq!(hrmax.grade, "Weak");
        assert!(
            why.grade_note.contains("Weak evidence"),
            "grade_note glosses the grade: {}",
            why.grade_note
        );
        // improves: the engagement loop, replace the age estimate.
        assert!(
            why.improves.to_lowercase().contains("measured max hr"),
            "improves points at the data that would sharpen it: {}",
            why.improves
        );
        // The improves line describes the engine's data need, never a training
        // prescription (HARD RULE 1): no imperative training terms (word-level
        // match so "replace"/"estimate" don't false-trip on "rep"/"set").
        let banned = ["squat", "deadlift", "%1rm", "rir", "rpe", "reps", "sets"];
        let words: Vec<String> = why
            .improves
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '%')
            .map(|w| w.to_string())
            .collect();
        for b in banned {
            assert!(
                !words.iter().any(|w| w == b),
                "improves must not prescribe: {}",
                why.improves
            );
        }
    }

    #[test]
    fn contested_guidance_grade_note_names_the_contested_question() {
        // A contested claim (HYP-VOL-001 → CQ-01) must append the contested
        // question to the grade note, not just restate the badge.
        let app = Engine;
        let mut model = Model::default();
        model.profile = Some(sample_profile());
        let vm = app.view(&model);
        let contested = vm
            .guidance
            .iter()
            .chain(vm.reference.iter())
            .find(|g| g.contested && !g.why.grade_note.is_empty());
        if let Some(g) = contested {
            assert!(
                g.why.grade_note.to_lowercase().contains("contested"),
                "contested claim's grade_note flags it: {}",
                g.why.grade_note
            );
        }
    }

    #[test]
    fn contested_render_shows_one_opposing_side_when_kb_supplies_it() {
        // #4: CQ-01 carries a KB-sourced opposing citation → the disclosure shows
        // it as "one view on the other side"; CQ-02 has none → the render stops
        // at the engine lean and fabricates no opposing view.
        let with = grade_note_str("Strong", true, Some("CQ-01"));
        assert!(
            with.contains("genuinely contested") && with.contains("Our current lean:"),
            "contested render carries the lean: {with}"
        );
        assert!(
            with.contains("One view on the other side:")
                && with.to_lowercase().contains("schoenfeld"),
            "CQ-01's KB opposing cite surfaces: {with}"
        );

        let without = grade_note_str("Moderate", true, Some("CQ-02"));
        assert!(
            without.contains("genuinely contested"),
            "CQ-02 still flags the debate: {without}"
        );
        assert!(
            !without.contains("One view on the other side:"),
            "CQ-02 has no KB opposing cite → no fabricated other side: {without}"
        );
    }

    #[test]
    fn grade_definitions_export_all_five_grades_with_confidence() {
        // The legend sheet renders from core data, not hardcoded Kotlin.
        let app = Engine;
        let model = Model::default();
        let defs = app.view(&model).grade_definitions;
        assert_eq!(defs.len(), 5, "five File-09 grades");
        let strong = defs
            .iter()
            .find(|d| d.grade == "Strong")
            .expect("Strong grade present");
        assert!(
            strong.definition.to_lowercase().contains("meta-analyses")
                || strong.definition.to_lowercase().contains("randomized"),
            "Strong definition is the KB text: {}",
            strong.definition
        );
        assert!((strong.confidence - 0.90).abs() < 1e-6);
        // MarketingMyth is named in the legend but never emitted on a card.
        assert!(defs.iter().any(|d| d.grade == "MarketingMyth"));
    }

    #[test]
    fn why_view_serde_defaults_to_empty_for_old_shells() {
        // An AdjustmentView from an old core (no `why` key) decodes to an empty
        // WhyView; the block is additive and never breaks replay.
        let json = r#"{"summary":"Take a full rest day","grade":"Strong","citation":"x","confidence":0.9,"safety_critical":true,"contested":false}"#;
        let v: AdjustmentView = serde_json::from_str(json).expect("old shape decodes");
        assert_eq!(v.why, WhyView::default());
        assert!(v.why.basis.is_empty() && v.why.grade_note.is_empty() && v.why.improves.is_empty());
    }

    #[test]
    fn headline_propagates_the_adjustments_why() {
        // The today headline carries the winning adjustment's why? triad so the
        // Today card's "why?" is populated without the shell re-deriving it.
        let app = Engine;
        let mut model = Model::default();
        // A lone HRV suppression → DowngradeSession adjustment (HRV-001).
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::HrvLnRmssd, -1.2)),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert_eq!(vm.today_headline.kind, "adjustment");
        assert!(
            !vm.today_headline.why.grade_note.is_empty(),
            "headline carries a populated why?"
        );
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
    fn lift_only_profile_omits_running_and_hybrid_guidance() {
        // Modality is implicit: running = running_days_per_week > 0. A pure
        // lifter (no running volume) must not be shown Running-section rows nor
        // the Hybrid concurrent-training block (the same-session-order card).
        let mut p = sample_profile();
        p.running_days_per_week = 0;
        p.running_km_per_week = 0.0;
        let rows = build_guidance(&p);
        assert!(
            !rows.iter().any(|g| g.section == "Hybrid"),
            "pure lifter must not see any Hybrid-section row"
        );
        assert!(
            !rows.iter().any(|g| g.summary.contains("same-session order")),
            "pure lifter must not see the same-session-order card"
        );
        assert!(
            !rows.iter().any(|g| g.section == "Running"),
            "pure lifter must not see any Running-section row"
        );
        // Sanity: they still get their Strength guidance.
        assert!(
            rows.iter().any(|g| g.section == "Strength"),
            "pure lifter must still see Strength rows"
        );
    }

    #[test]
    fn run_only_profile_omits_strength_and_hybrid_guidance() {
        // Modality is implicit: lifting = weekly_sets > 0. A pure runner (no
        // lifting volume) must not be shown Strength-section rows nor the Hybrid
        // concurrent-training block.
        let mut p = sample_profile();
        p.weekly_sets = 0;
        let rows = build_guidance(&p);
        assert!(
            !rows.iter().any(|g| g.section == "Strength"),
            "pure runner must not see any Strength-section row"
        );
        assert!(
            !rows.iter().any(|g| g.section == "Hybrid"),
            "pure runner must not see any Hybrid-section row"
        );
        // Sanity: they still get their Running guidance.
        assert!(
            rows.iter().any(|g| g.section == "Running"),
            "pure runner must still see Running rows"
        );
    }

    #[test]
    fn hybrid_profile_keeps_all_modality_sections() {
        // Guard against over-gating: the hybrid sample_profile (lifts and runs)
        // must retain Strength, Running and Hybrid rows all at once.
        let rows = build_guidance(&sample_profile());
        assert!(
            rows.iter().any(|g| g.section == "Strength"),
            "hybrid profile must keep Strength rows"
        );
        assert!(
            rows.iter().any(|g| g.section == "Running"),
            "hybrid profile must keep Running rows"
        );
        assert!(
            rows.iter().any(|g| g.section == "Hybrid"),
            "hybrid profile must keep Hybrid rows"
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
                                            age_years: None,
                                            resting_hr_bpm: None,
                                            measured_hr_max: None,
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
                        ).0);
                    }
                }
            }
        }

        // HR zones: ages outside 5..=100 yield the explanatory row, never a bogus
        // HRmax; in-range ages produce cited bands.
        for &age in &[-1.0, 0.0, 4.9, 5.0, 30.0, 100.0, 100.1, 200.0] {
            cited_non_myth(&build_hr_zones(
                &HrZoneQuery {
                    age_years: age,
                    resting_hr_bpm: None,
                    weeks_since_recalc: None,
                    weeks_since_pace_test: None,
                },
                None,
            ).0);
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
    fn review_even_split_run_carries_human_category_label() {
        // An even-effort run (no positive split) resolves to PositiveExecution;
        // the core ships the human overline verbatim so the shell renders it
        // without a hand-maintained parallel label map.
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            positive_split_pct: Some(0.0),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "PositiveExecution");
        assert_eq!(fb.category_label, "Pacing");
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
            log_run_track(points, 70.0, 12.0, 0, 0),
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
            track_segment_starts: Vec::new(),
            observed_at: 0,
            entry_id: 0,
            workout_type: None,
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

    /// A distinctive GPS run whose content is unlikely to be pre-cached by any
    /// other test on this thread, so the P1 memo counter delta is meaningful.
    fn p1_probe_run(entry_id: u64) -> LoggedRun {
        LoggedRun {
            distance_km: 0.0,
            duration_min: 0.0,
            hr_pct_max: 71.0,
            longest_recent_km: 3.3,
            track: vec![
                GpsPoint { lat: 12.5, lon: 34.5000, observed_at: 1_000, accuracy_m: 4.0 },
                GpsPoint { lat: 12.5, lon: 34.5010, observed_at: 1_050, accuracy_m: 4.0 },
                GpsPoint { lat: 12.5, lon: 34.5020, observed_at: 1_100, accuracy_m: 4.0 },
                GpsPoint { lat: 12.5, lon: 34.5030, observed_at: 1_150, accuracy_m: 4.0 },
            ],
            track_segment_starts: Vec::new(),
            observed_at: 1_000,
            entry_id,
            workout_type: None,
        }
    }

    #[test]
    fn p1_run_view_derivation_is_memoized_and_gpx_built_once() {
        // P1: the heavy per-run derivation (≈6 haversine passes + km/mile splits
        // + VI + a GPX string) must run ONCE per distinct run, not on every
        // view(). A second identical derivation serves a cached clone, zero
        // rebuilds, while the wire shape (incl. the populated gpx) is unchanged.
        let run = p1_probe_run(918_273);

        let before = RUN_VIEW_BUILDS.with(|c| c.get());
        let first = to_run_view(&run);
        let after_first = RUN_VIEW_BUILDS.with(|c| c.get());
        let second = to_run_view(&run);
        let after_second = RUN_VIEW_BUILDS.with(|c| c.get());

        assert_eq!(after_first - before, 1, "the first derivation builds once");
        assert_eq!(after_second - after_first, 0, "the second is served from cache");
        assert_eq!(first, second, "the cached view is identical");
        // The heavy GPX string is still populated (the shell's list label +
        // detail map depend on it): memoization did not drop it.
        assert!(first.gpx.contains("<trkseg>"), "gpx stays populated: {}", first.gpx);

        // A content change (an amend) must MISS the cache and rebuild.
        let mut amended = run.clone();
        amended.longest_recent_km = 99.0;
        let before_amend = RUN_VIEW_BUILDS.with(|c| c.get());
        let _ = to_run_view(&amended);
        assert_eq!(
            RUN_VIEW_BUILDS.with(|c| c.get()) - before_amend,
            1,
            "a changed fingerprint recomputes"
        );
    }

    #[test]
    fn p6_run_view_carries_structured_spike_baseline_flag() {
        // #6: the spike-baseline provenance the shell scraped from `spike_note`
        // ("no prior run") is now a structured bool. A first run (no baseline)
        // flags with has_baseline=false; a run with a prior baseline is true.
        let mut no_baseline = p1_probe_run(1);
        no_baseline.longest_recent_km = 0.0;
        let v = to_run_view(&no_baseline);
        assert!(v.spike_flag && !v.spike_has_baseline, "first run: no baseline");
        assert!(v.spike_note.contains("no prior run"), "prose kept for compat");

        let mut with_baseline = p1_probe_run(2);
        with_baseline.longest_recent_km = 3.0; // a short run vs a 3 km baseline
        let v2 = to_run_view(&with_baseline);
        assert!(v2.spike_has_baseline, "a prior baseline exists");
    }

    #[test]
    fn p6_protein_and_hr_wire_fields_are_additive_and_serde_default() {
        // #6 + wire compat: the new ViewModel fields (hr_max, protein_figures)
        // and RunResultView.spike_has_baseline are additive with serde defaults -
        // an old ViewModel JSON lacking them still decodes.
        let app = Engine;
        let mut model = Model::default();
        model.profile = Some(sample_profile());
        // Request both calculators so the structured figures populate.
        app.update(
            Event::ComputeProtein { bodyweight_kg: 80.0, masters: true, deficit: false },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::ComputeHrZones {
                age_years: 40.0,
                resting_hr_bpm: None,
                weeks_since_recalc: None,
                weeks_since_pace_test: None,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        // Structured protein figure mirrors the prose row (80 kg × masters band).
        let pf = vm.protein_figures.iter().find(|f| f.kind == "masters").expect("masters figure");
        assert!(pf.low_g_per_day > 0.0 && pf.high_g_per_day >= pf.low_g_per_day && !pf.refused);
        // Structured HRmax: Tanaka(40) = 180, with the 208 − 0.7 split exposed.
        let hm = vm.hr_max.as_ref().expect("hr_max figure");
        assert!(!hm.measured && hm.bpm == 180.0 && hm.tanaka_intercept == 208.0);

        // Round-trip through JSON with the three new fields stripped: still decodes.
        let mut json: serde_json::Value = serde_json::to_value(&vm).unwrap();
        let obj = json.as_object_mut().unwrap();
        obj.remove("hr_max");
        obj.remove("protein_figures");
        for run in obj.get_mut("runs").and_then(|r| r.as_array_mut()).into_iter().flatten() {
            run.as_object_mut().unwrap().remove("spike_has_baseline");
        }
        let decoded: ViewModel = serde_json::from_value(json).expect("old-shape JSON decodes");
        assert!(decoded.hr_max.is_none() && decoded.protein_figures.is_empty());
    }

    #[test]
    fn split_feedback_and_run_row_use_the_same_qc_filtered_track() {
        // A single teleport fix (implausible >12 m/s jump) that CLEARS the
        // accuracy gate but FAILS a qc_track speed gate. The displayed run-row
        // split (to_run_view → qc_track) drops it; the pacing-feedback split
        // (latest_track_split) must derive from the SAME qc_track output, not the
        // accuracy-only usable_track, so the run chip and the coaching cue can't
        // disagree near ±3% (MEDIUM bug).
        // Clean track is evenly paced (split ≈ 0 %, "even"); the teleport in the
        // back half, if kept, flips it to a large negative split, opposite
        // verdicts across the ±3 % line.
        let pts = vec![
            GpsPoint { lat: 0.0, lon: 0.000, observed_at: 0, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 0.001, observed_at: 25, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 0.002, observed_at: 50, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 0.003, observed_at: 75, accuracy_m: 5.0 },
            // Teleport: ~5.2 km in 1 s (>12 m/s), accuracy fine, qc drops it.
            GpsPoint { lat: 0.0, lon: 0.050, observed_at: 76, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 0.004, observed_at: 100, accuracy_m: 5.0 },
        ];
        let app = Engine;
        let mut model = Model::default();
        app.update(
            log_run_track(pts.clone(), 70.0, 12.0, 0, 0),
            &mut model,
        )
        .expect_only_render();

        let row_split = to_run_view(&model.runs[0]).split_pct.expect("run-row split");
        let fb_split = latest_track_split(&model).expect("feedback split");
        assert_eq!(
            row_split, fb_split,
            "run chip and pacing feedback must derive the split from the same qc_track"
        );

        // The accuracy-only path (the old bug) keeps the teleport fix and yields a
        // materially different split, proving the two filters actually diverge
        // on this track, so the assertion above is not vacuous.
        let raw_split = running::track_positive_split_pct(&pts, running::MAX_GPS_ACCURACY_M)
            .expect("raw split");
        assert!(
            (raw_split - fb_split).abs() > 3.0,
            "teleport must shift the accuracy-only split vs the qc split (raw {raw_split}, qc {fb_split})"
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
            track_segment_starts: Vec::new(),
            observed_at: 0,
            entry_id: 0,
            workout_type: None,
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
    fn run_view_differentiates_interval_from_steady_same_average() {
        // Two GPS runs with identical total distance (~445 m) and duration (120 s)
        // → identical average pace. The interval run mixes hard reps with standing
        // recovery; the steady run holds an even pace. The engine must rate them
        // differently now (RUN-INTERVAL-VI-001), not identically.
        let g = |lon: f64, t: i64| GpsPoint {
            lat: 0.0,
            lon,
            observed_at: t,
            accuracy_m: 5.0,
        };
        let steady = LoggedRun {
            distance_km: 0.0,
            duration_min: 0.0,
            hr_pct_max: 0.0,
            longest_recent_km: 5.0,
            // Four even ~111 m / 30 s segments (~3.71 m/s throughout).
            track: vec![g(0.000, 0), g(0.001, 30), g(0.002, 60), g(0.003, 90), g(0.004, 120)],
            track_segment_starts: Vec::new(),
            observed_at: 0,
            entry_id: 0,
            workout_type: None,
        };
        // Interval with JOG recovery (recovery legs stay above the 0.5 m/s
        // auto-pause floor, so moving time, and therefore average moving pace,
        // matches the steady run exactly; only the variability differs). Two
        // ~200 m hard reps (~6.7 m/s) each followed by a ~22 m jog (~0.74 m/s).
        let interval = LoggedRun {
            track: vec![g(0.0000, 0), g(0.0018, 30), g(0.0020, 60), g(0.0038, 90), g(0.0040, 120)],
            ..steady.clone()
        };

        let sv = to_run_view(&steady);
        let iv = to_run_view(&interval);

        // Same average pace by construction.
        assert_eq!(sv.pace, iv.pace, "same-average precondition broke");

        let s = sv.interval.expect("steady interval verdict");
        let i = iv.interval.expect("interval verdict");
        assert_eq!(s.kind, "steady", "VI {}", s.variability_index);
        assert_eq!(i.kind, "interval", "VI {}", i.variability_index);
        assert!(i.variability_index > s.variability_index);
        // Honestly graded Weak (flat-ground GOVSS/NGP simplification).
        assert_eq!(i.grade, "Weak");
        assert!(i.citation.contains("Skiba"), "got {}", i.citation);
        assert!(!i.safety_critical);
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
            track_segment_starts: Vec::new(),
            observed_at: 0,
            entry_id: 0,
            workout_type: None,
        });
        assert!(view.split_pct.is_none());
        assert!(view.split.is_none());
        // A hand-entered run has no track to walk → no per-unit splits either.
        assert!(view.splits_km.is_empty());
        assert!(view.splits_mi.is_empty());
    }

    #[test]
    fn run_view_carries_full_spike_gate_evidence() {
        // Task 7: grade/confidence/safety_critical/contested, not just the
        // citation, parity with the other evidence-bearing view structs.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            log_run(12.0, 60.0, 70.0, 10.0, 0, 0),
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
    fn logged_run_track_excludes_the_pause_bridge_and_breaks_the_gpx(){
        // End-to-end over the real event path: a run with a pause +
        // ~111 km relocation. Segment 1 (indices 0–4) then segment 2 (indices
        // 5–9), boundary at index 5. The bridge fix (index 5) is a 1853 m/s
        // "teleport" the QC gate would normally drop: the segment boundary must
        // suppress that gate so segment 2 survives.
        let relocation = vec![
            GpsPoint { lat: 0.0, lon: 0.000, observed_at: 0, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 0.001, observed_at: 10, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 0.002, observed_at: 20, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 0.003, observed_at: 30, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 0.004, observed_at: 40, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 1.000, observed_at: 100, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 1.001, observed_at: 110, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 1.002, observed_at: 120, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 1.003, observed_at: 130, accuracy_m: 5.0 },
            GpsPoint { lat: 0.0, lon: 1.004, observed_at: 140, accuracy_m: 5.0 },
        ];

        // With the boundary: distance is the two ~445 m segments (~0.89 km), NOT
        // the ~111 km bridge, and the GPX opens two <trkseg>s at the TRUE coords.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            log_run_track_seg(relocation.clone(), 70.0, 0.0, 1000, 1, vec![5]),
            &mut model,
        )
        .expect_only_render();
        let run = &app.view(&model).runs[0];
        assert!(run.distance_km < 5.0, "bridge excluded: {}", run.distance_km);
        assert!((run.distance_km - 0.889).abs() < 0.05, "distance {}", run.distance_km);
        assert_eq!(run.gpx.matches("<trkseg>").count(), 2, "two segments in GPX");
        assert!(run.gpx.contains("lon=\"1.0000000\""), "true seg-2 coord kept");

        // WITHOUT the boundary over these SAME true coords, the QC gate rejects
        // the ~1853 m/s bridge as a teleport and drops ALL of segment 2 with it -
        // so distance collapses to segment 1 alone (~0.445 km) and the relocated
        // half of the run is LOST. The boundary is exactly what lets the core keep
        // segment 2 (real distance) while still excluding the bridge.
        let mut naive = Model::default();
        app.update(
            log_run_track(relocation, 70.0, 0.0, 1000, 2),
            &mut naive,
        )
        .expect_only_render();
        let naive_run = &app.view(&naive).runs[0];
        assert!(
            naive_run.distance_km < 0.6,
            "no boundary → segment 2 dropped as a teleport: {}",
            naive_run.distance_km,
        );
        assert!(naive_run.qc_dropped >= 5, "the 5 seg-2 fixes were dropped");
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
                    entry_id: 0,
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
        // run, so the deferral is reachable from ordinary logging. The gate
        // now derives its baseline from LOGGED history (`model.runs`), so the
        // spike must be demonstrable against a real prior run: a 10 km baseline
        // run, then a 12 km run (+20 %).
        let app = Engine;
        let mut model = Model::default();
        app.update(log_run(10.0, 50.0, 70.0, 0.0, DAY_SEC, 0), &mut model)
            .expect_only_render();
        app.update(log_run(12.0, 60.0, 70.0, 0.0, 20 * DAY_SEC, 0), &mut model)
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
            log_run(12.0, 60.0, 70.0, 0.0, 0, 0),
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
            log_run_track(points, 70.0, 12.0, 0, 0),
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
                entry_id: 0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            log_run(10.0, 50.0, 70.0, 12.0, 0, 0),
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
                .any(|a| a.summary.contains("Re-test")),
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
        // 50.0 longest_recent_km quiets the spike gate; not under test here.
        log_run(km, minutes, hr, 50.0, at, 0)
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
        // #7: plain-language method, TRIMP + CTL/ATL kept, but the raw τ/EWMA
        // formula-speak is gone (the shell glossary defines the abbreviations).
        assert!(tl.method.contains("TRIMP") && tl.method.contains("CTL"), "{}", tl.method);
        assert!(!tl.method.contains('τ') && !tl.method.contains("EWMA"), "{}", tl.method);
        assert!(tl.summary.contains("not a performance predictor"));
        assert_eq!(tl.grade, "Moderate");
        assert!(!tl.citation.is_empty());
    }

    #[test]
    fn training_load_day_loop_is_bounded_against_a_mis_unit_timestamp() {
        // A run logged with a MILLISECOND observed_at (1.7e12) next to a normal
        // seconds run would, without a cap, make the CTL/ATL day loop iterate
        // over ~19.7M epoch-days inside view(): a data-reachable hang (HIGH
        // bug). The accumulation window must stay bounded to MAX_LOAD_DAYS
        // regardless of the raw span between the two stamps.
        let app = Engine;
        let mut model = Model::default();
        app.update(log_run_at(10.0, 60.0, 75.0, 1_700_000_000), &mut model)
            .expect_only_render(); // ~2023, seconds
        app.update(log_run_at(10.0, 60.0, 75.0, 1_700_000_000_000), &mut model)
            .expect_only_render(); // milliseconds - off by 1000×

        // view() returns (no unbounded loop) and the window is capped, never the
        // ~19.7M-day raw span between the two stamps.
        let tl = app.view(&model).training_load.expect("load view present");
        assert!(
            tl.days <= MAX_LOAD_DAYS as u32,
            "CTL/ATL window {} days must be capped at {}",
            tl.days,
            MAX_LOAD_DAYS
        );
        // Spans past the cap collapse to exactly the cap, and both runs are still
        // counted (counting is independent of the accumulation window).
        assert_eq!(tl.days, MAX_LOAD_DAYS as u32);
        assert_eq!(tl.sessions_counted, 2);

        // A stamp near i64::MAX must also return promptly, not freeze view().
        app.update(log_run_at(10.0, 60.0, 75.0, i64::MAX - 1), &mut model)
            .expect_only_render();
        let tl = app.view(&model).training_load.expect("load view present");
        assert!(tl.days <= MAX_LOAD_DAYS as u32);
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
                entry_id: 0,
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
            log_run_track(vec![
                    p(0.000, 0),
                    p(0.001, 20),
                    // Teleport: ~11 km implied in 1 s (>12 m/s), dropped.
                    p(0.100, 21),
                    p(0.002, 40),
                    // Non-advancing timestamp, dropped.
                    p(0.0025, 40),
                    p(0.003, 60),
                ], 78.0, 12.0, 0, 0),
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
                entry_id: 0,
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
                entry_id: 0,
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
                    entry_id: 0,
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
        assert!(has("Lengthen the rest interval"), "rep-drop rest");
        assert!(has("Scale this week to"), "recovery-adjusted volume");
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

        // A load-explained decline routes to the recovery-first trend message.
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
        assert!(trend.summary.contains("Recovery first"), "{}", trend.summary);

        // 14 distinct dated days of logging ends the provisional window.
        for i in 1..=14 {
            app.update(
                Event::LogSet {
                    exercise: "Bench press".into(),
                    weight_kg: 60.0,
                    reps: 5,
                    rpe: 7.0,
                    observed_at: i * DAY + 10,
                    entry_id: 0,
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
        assert!(has("Recompute from a measured HRmax"), "recalc-due row");
        assert!(has("Re-test to set paces"), "pace-retest row");
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
        assert!(pred.notes.iter().any(|n| n.summary.contains("Re-test")));
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
    fn marathon_under_mileage_derates_the_displayed_band_option_b() {
        // running-040/008 option B ("flag AND derate"): an under-mileaged
        // marathon prediction's SHOWN band must move slower, not just carry a
        // caveat. The base band depends only on the recent race/goal, not on
        // the longest logged run, so comparing an under-mileaged runner (20 km)
        // to a well-supported one (32 km) isolates exactly the derate.
        let marathon = RaceQuery {
            recent_distance_m: 10_000.0,
            recent_time_sec: 2_520.0, // 42:00 10K
            goal_distance_m: 42_195.0,
            weekly_km: 50.0,
            weeks_since_race: Some(2), // fresh → no freshness note in the way
        };
        let supported = to_race_view(&marathon, Some(32.0)); // ≥30 km → no derate
        let under = to_race_view(&marathon, Some(20.0)); // <30 km → derate

        // The displayed band shifts strictly SLOWER (later finish) at both ends.
        assert!(
            under.low_sec > supported.low_sec && under.high_sec > supported.high_sec,
            "under-mileaged band is slower: under=({}, {}) supported=({}, {})",
            under.low_sec,
            under.high_sec,
            supported.low_sec,
            supported.high_sec
        );
        // The caveat note is retained on the derated prediction, absent on the
        // well-supported one.
        assert!(
            under.notes.iter().any(|n| n
                .summary
                .contains("optimistic without long-run support")),
            "caveat retained: {:#?}",
            under.notes
        );
        assert!(
            !supported
                .notes
                .iter()
                .any(|n| n.summary.contains("optimistic without long-run support")),
            "well-supported marathon carries no optimism caveat: {:#?}",
            supported.notes
        );
        // The derated number still travels with running-040/008 evidence
        // (RUN-VDOT-001) via the retained note (HARD RULE 2).
        assert!(
            under.notes.iter().any(|n| n.summary.contains("VDOT points")),
            "derate magnitude + evidence present: {:#?}",
            under.notes
        );

        // Other distances are UNTOUCHED by the under-mileage gate: 5K/10K/half
        // predictions are byte-identical whether the runner is under-mileaged
        // or not, and carry no marathon caveat.
        for goal in [5_000.0, 10_000.0, 21_097.5] {
            let q = RaceQuery { goal_distance_m: goal, ..marathon.clone() };
            let a = to_race_view(&q, Some(20.0));
            let b = to_race_view(&q, Some(32.0));
            assert_eq!(
                (a.low_sec, a.high_sec, a.predicted.clone()),
                (b.low_sec, b.high_sec, b.predicted.clone()),
                "distance {goal} m is untouched by the marathon derate"
            );
            assert!(
                a.notes
                    .iter()
                    .all(|n| !n.summary.contains("optimistic without long-run support")),
                "no marathon caveat leaks onto {goal} m: {:#?}",
                a.notes
            );
        }

        // The well-supported marathon is itself byte-identical to the raw
        // (no-history) prediction: the derate is the ONLY thing 20 km changes.
        let no_history = to_race_view(&marathon, None);
        assert_eq!(
            (supported.low_sec, supported.high_sec, supported.predicted.clone()),
            (no_history.low_sec, no_history.high_sec, no_history.predicted.clone()),
            "an adequately-mileaged marathon matches the un-derated prediction"
        );
    }

    #[test]
    fn race_prediction_absurd_goal_is_degenerate_not_infinite() {
        // An absurd goal distance overflows Riegel's power to f64::INFINITY. The
        // prediction must collapse to the degenerate "-"/"need a valid recent
        // race" path, never leak Inf (→ serde null) into low_sec/high_sec (LOW
        // bug: the old `low_sec <= 0.0` guard didn't catch Inf).
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::PredictRace {
                recent_distance_m: 10_000.0,
                recent_time_sec: 2_520.0,
                goal_distance_m: 1e300,
                weekly_km: 50.0,
                weeks_since_race: Some(2),
            },
            &mut model,
        )
        .expect_only_render();
        let pred = app.view(&model).race_prediction.expect("prediction");
        assert_eq!(pred.predicted, "-", "absurd goal → degenerate dash");
        assert!(pred.summary.contains("need a valid recent race"));
        assert!(
            pred.low_sec.is_finite() && pred.high_sec.is_finite(),
            "no Inf reaches the view: low {} high {}",
            pred.low_sec,
            pred.high_sec
        );
    }

    #[test]
    fn calculators_echo_their_inputs_for_form_rehydration() {
        // Each calculator's result view must carry back the raw query it was
        // computed from, so a shell rehydrates its form after a log replay
        // instead of resetting to hardcoded defaults (the "it reset" bug).
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::PredictRace {
                recent_distance_m: 10_000.0,
                recent_time_sec: 2_520.0,
                goal_distance_m: 21_097.5,
                weekly_km: 55.0,
                weeks_since_race: Some(3),
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::ComputeHrZones {
                age_years: 47.0,
                resting_hr_bpm: Some(52.0),
                weeks_since_recalc: Some(5),
                weeks_since_pace_test: None,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 82.5,
                masters: true,
                deficit: false,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::PlanHypertrophyMeso {
                muscle: "back".into(),
                weeks: 6,
                not_growing: true,
                recovering_easily: false,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(Event::ComputeCooper { distance_m_12min: 2_750.0 }, &mut model)
            .expect_only_render();
        app.update(
            Event::ComputeCriticalSpeed {
                efforts: vec![
                    CsEffortIn { distance_m: 1_200.0, time_sec: 200.0 },
                    CsEffortIn { distance_m: 3_000.0, time_sec: 600.0 },
                ],
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::ComputeApre {
                scheme: autoreg::ApreScheme::Apre6,
                reps: 9,
                current_load_lb: 185.0,
            },
            &mut model,
        )
        .expect_only_render();

        let v = app.view(&model);

        let rp = v.race_prediction.expect("prediction");
        assert_eq!(rp.recent_distance_m, 10_000.0);
        assert_eq!(rp.recent_time_sec, 2_520.0);
        assert_eq!(rp.goal_distance_m, 21_097.5);
        assert_eq!(rp.weekly_km, 55.0);
        assert_eq!(rp.weeks_since_race, Some(3));

        let hz = v.hr_zone_input.expect("hr zone input");
        assert_eq!(hz.age_years, 47.0);
        assert_eq!(hz.resting_hr_bpm, Some(52.0));
        assert_eq!(hz.weeks_since_recalc, Some(5));
        assert_eq!(hz.weeks_since_pace_test, None);

        let pr = v.protein_input.expect("protein input");
        assert_eq!(pr.bodyweight_kg, 82.5);
        assert!(pr.masters && !pr.deficit);

        let hy = v.hypertrophy_input.expect("hypertrophy input");
        assert_eq!(hy.muscle, "back");
        assert_eq!(hy.weeks, 6);
        assert!(hy.not_growing && !hy.recovering_easily);

        assert_eq!(v.cooper_input, Some(2_750.0));
        assert_eq!(v.critical_speed_input.len(), 2);
        assert_eq!(v.critical_speed_input[0].distance_m, 1_200.0);

        let ap = v.apre_input.expect("apre input");
        assert_eq!(ap.scheme, autoreg::ApreScheme::Apre6);
        assert_eq!(ap.reps, 9);
        assert_eq!(ap.current_load_lb, 185.0);
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

    // ── Today headline (usability-ia-spec §7: core-owned "today's call") ──

    #[test]
    fn headline_defaults_to_train_as_planned_with_no_evidence_claim() {
        let app = Engine;
        let model = Model::default();
        let vm = app.view(&model);
        assert_eq!(vm.today_headline.kind, "all_clear");
        assert!(vm.today_headline.summary.contains("Train as planned"));
        // The all-clear asserts the ABSENCE of a triggered rule: no evidence
        // tag may be fabricated for it (HARD RULE 1/2).
        assert!(vm.today_headline.grade.is_empty());
        assert!(vm.today_headline.citation.is_empty());
    }

    #[test]
    fn headline_prioritizes_safety_hold_over_adjustments_and_feedback() {
        let app = Engine;
        let mut model = Model::default();
        // A routine adjustment trigger AND a hard stop: the stop must win.
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::HrvLnRmssd, -1.0)),
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)),
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::SubmitReview(SessionReview {
                lift: Some(LiftExec {
                    reps_met: true,
                    rir_actual: 2,
                    rir_target: 2,
                }),
                ..SessionReview::default()
            }),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.train_blocked);
        assert_eq!(vm.today_headline.kind, "safety_hold");
        assert!(
            vm.today_headline.summary.contains("Stop"),
            "{}",
            vm.today_headline.summary
        );
        assert!(vm.today_headline.safety_critical);
        assert!(!vm.today_headline.grade.is_empty());
    }

    #[test]
    fn headline_falls_to_adjustment_then_feedback() {
        let app = Engine;
        let mut model = Model::default();
        // Non-blocking readiness trigger → adjustment headline.
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::HrvLnRmssd, -1.0)),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert_eq!(vm.today_headline.kind, "adjustment");
        assert!(
            vm.today_headline.summary.contains("easier session"),
            "{}",
            vm.today_headline.summary
        );
        assert!(!vm.today_headline.grade.is_empty());

        // Readiness cleared, review present → the session feedback leads.
        app.update(Event::ClearReadiness, &mut model)
            .expect_only_render();
        app.update(
            Event::SubmitReview(SessionReview {
                lift: Some(LiftExec {
                    reps_met: true,
                    rir_actual: 2,
                    rir_target: 2,
                }),
                ..SessionReview::default()
            }),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert_eq!(vm.today_headline.kind, "feedback");
        assert_eq!(
            vm.today_headline.summary,
            vm.feedback.as_ref().unwrap().message
        );
    }

    // ── Per-signal readiness summary (KB-honest; no composite score) ──

    #[test]
    fn readiness_summary_states_cite_the_judging_rule() {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::HrvLnRmssd, -1.0)),
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::RestingHr, 12.0)),
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert_eq!(vm.readiness_summary.len(), 3);

        let hrv = vm
            .readiness_summary
            .iter()
            .find(|s| s.signal == "HrvLnRmssd")
            .unwrap();
        assert_eq!(hrv.state, "suppressed");
        assert_eq!(hrv.group, "metric");
        assert!(!hrv.grade.is_empty(), "state judged by a rule carries its evidence");
        assert!(!hrv.citation.is_empty());

        let rhr = vm
            .readiness_summary
            .iter()
            .find(|s| s.signal == "RestingHr")
            .unwrap();
        assert!(rhr.state.contains("+10 bpm"), "{}", rhr.state);
        assert!(rhr.safety_critical);

        let pain = vm
            .readiness_summary
            .iter()
            .find(|s| s.signal == "Pain")
            .unwrap();
        assert_eq!(pain.group, "red_flag");
        assert!(pain.state.contains("red flag"), "{}", pain.state);
    }

    #[test]
    fn readiness_summary_all_clear_rows_carry_no_fabricated_evidence() {
        let app = Engine;
        let mut model = Model::default();
        // A withdrawn/absent pain report ("clear") and a recorded-only signal
        // judge nothing: their evidence fields must stay empty rather than
        // borrowing a rule's citation they did not run through.
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Pain, 0.0)),
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::BarVelocity, 0.5)),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        let pain = vm
            .readiness_summary
            .iter()
            .find(|s| s.signal == "Pain")
            .unwrap();
        assert_eq!(pain.state, "clear");
        assert!(pain.grade.is_empty());
        let bv = vm
            .readiness_summary
            .iter()
            .find(|s| s.signal == "BarVelocity")
            .unwrap();
        assert_eq!(bv.state, "recorded");
        assert!(bv.grade.is_empty());
    }

    #[test]
    fn signal_groups_metadata_fences_the_red_flag_block() {
        let app = Engine;
        let vm = app.view(&Model::default());
        assert_eq!(vm.signal_groups.len(), 15);
        for red in ["Pain", "Illness", "RedS", "CardiacRedFlag", "BoneStress"] {
            assert_eq!(
                vm.signal_groups
                    .iter()
                    .find(|g| g.signal == red)
                    .unwrap()
                    .group,
                "red_flag"
            );
        }
        // Order contract: every metric precedes every red flag, so a shell can
        // divide exactly where the group changes.
        let first_red = vm
            .signal_groups
            .iter()
            .position(|g| g.group == "red_flag")
            .unwrap();
        assert!(vm.signal_groups[..first_red].iter().all(|g| g.group == "metric"));
        assert!(vm.signal_groups[first_red..].iter().all(|g| g.group == "red_flag"));
    }

    // ── Backdating: out-of-order logging is ordered by observed_at ──

    #[test]
    fn backdated_set_slots_into_the_e1rm_chain_chronologically() {
        let app = Engine;
        let mut model = Model::default();
        // Today's set first, then a BACKDATED earlier set of the same lift.
        app.update(
            Event::LogSet {
                exercise: "Back Squat".into(),
                weight_kg: 100.0,
                reps: 5,
                rpe: 8.0,
                observed_at: 2_000_000,
                entry_id: 0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::LogSet {
                exercise: "Back Squat".into(),
                weight_kg: 95.0,
                reps: 5,
                rpe: 8.0,
                observed_at: 1_000_000,
                entry_id: 0,
            },
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        // Chronological view order: the backdated set renders first…
        assert_eq!(vm.lifts[0].observed_at, 1_000_000);
        assert_eq!(vm.lifts[0].weight_kg, 95.0);
        assert!(vm.lifts[0].e1rm_delta_kg.is_none(), "first in time has no baseline");
        // …and the later set's delta compares against it (95→100 kg ⇒ up).
        assert_eq!(vm.lifts[1].observed_at, 2_000_000);
        assert_eq!(vm.lifts[1].e1rm_direction.as_deref(), Some("up"));
        let expected = ((strength::e1rm_epley(100.0, 5) * 10.0).round()
            - (strength::e1rm_epley(95.0, 5) * 10.0).round())
            / 10.0;
        assert!(
            (vm.lifts[1].e1rm_delta_kg.unwrap() - expected).abs() < 1e-9,
            "delta {:?} vs expected {expected}",
            vm.lifts[1].e1rm_delta_kg
        );
    }

    #[test]
    fn backdated_run_weekly_report_matches_in_order_logging() {
        let app = Engine;
        // Reference: two runs logged oldest-first.
        let mut in_order = Model::default();
        let prev_week_at = 10 * WEEK_SEC + 1000;
        let cur_week_at = 11 * WEEK_SEC + 1000;
        for (at, km) in [(prev_week_at, 10.0), (cur_week_at, 11.0)] {
            app.update(
                log_run(km, km * 6.0, 70.0, 12.0, at, 0),
                &mut in_order,
            )
            .expect_only_render();
        }
        // Same data, backdated: current week logged first.
        let mut backdated = Model::default();
        for (at, km) in [(cur_week_at, 11.0), (prev_week_at, 10.0)] {
            app.update(
                log_run(km, km * 6.0, 70.0, 12.0, at, 0),
                &mut backdated,
            )
            .expect_only_render();
        }
        let a = app.view(&in_order);
        let b = app.view(&backdated);
        let wow = |vm: &ViewModel| {
            vm.weekly_report
                .iter()
                .find(|r| r.summary.contains("Week-over-week"))
                .map(|r| r.summary.clone())
                .expect("week-over-week row")
        };
        assert_eq!(wow(&a), wow(&b), "weekly aggregation must not depend on log order");
        assert!(wow(&a).contains("10.0 → 11.0 km"), "{}", wow(&a));
        // And the run history renders chronologically in both.
        assert_eq!(b.runs[0].observed_at, prev_week_at);
        assert_eq!(b.runs[1].observed_at, cur_week_at);
    }

    #[test]
    fn review_observed_at_decodes_and_defaults() {
        // New wire form carries the stamp…
        let with: Event = serde_json::from_str(
            r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"bad_day":false,"observed_at":424242}}"#,
        )
        .expect("review with observed_at parses");
        match with {
            Event::SubmitReview(r) => assert_eq!(r.observed_at, 424_242),
            other => panic!("expected SubmitReview, got {other:?}"),
        }
        // …and the old persisted form still replays (defaults to 0).
        let without: Event = serde_json::from_str(
            r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"bad_day":false}}"#,
        )
        .expect("pre-timestamp review parses");
        match without {
            Event::SubmitReview(r) => assert_eq!(r.observed_at, 0),
            other => panic!("expected SubmitReview, got {other:?}"),
        }
    }

    // ── Coach-as-planner ────────────────────────

    /// A pure-strength profile with no running (guided-setup shape).
    fn strength_profile() -> Profile {
        Profile {
            lift_goal: LiftGoal::MaxStrength,
            goal_distance: GoalDistance::General,
            concurrent_goal: ConcurrentGoal::Strength,
            running_days_per_week: 0,
            running_km_per_week: 0.0,
            advanced: false,
            ..sample_profile()
        }
    }

    // Monday epoch-day: mon0_weekday(4) == 0, so day-0 (Heavy) lands on `today`.
    const MON: i64 = 4;

    fn logset(exercise: &str, weight_kg: f64, reps: u32) -> Event {
        Event::LogSet {
            exercise: exercise.into(),
            weight_kg,
            reps,
            rpe: 8.0,
            observed_at: MON * 86_400,
            entry_id: 0,
        }
    }

    fn planned_strength_model(with_set: bool) -> (Engine, Model) {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(strength_profile()), &mut model)
            .expect_only_render();
        if with_set {
            // A pre-plan baseline set (dated the day BEFORE the plan starts): it
            // seeds the e1RM anchor (day-independent) WITHOUT logging today's
            // session, so `today` (MON) stays the unlogged upcoming session the
            // next-session/readiness tests exercise. Tests that need TODAY logged
            // (done-day advancement) log it explicitly.
            app.update(
                Event::LogSet {
                    exercise: "Back Squat".into(),
                    weight_kg: 120.0,
                    reps: 3,
                    rpe: 8.0,
                    observed_at: (MON - 1) * 86_400,
                    entry_id: 0,
                },
                &mut model,
            )
            .expect_only_render();
        }
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();
        app.update(Event::SetToday { epoch_day: MON, utc_offset_sec: 0 }, &mut model)
            .expect_only_render();
        (app, model)
    }

    #[test]
    fn no_plan_means_no_next_session() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(strength_profile()), &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.next_session.is_none(), "no plan → no next session");
        assert!(vm.week_plan.is_empty());
        assert!(vm.program.is_none());
    }

    #[test]
    fn regenerating_a_plan_preserves_the_original_anchor() {
        // The shell auto-fires GeneratePlan on launch; re-firing with a LATER day
        // must NOT re-date the plan (else the week strip sits at "week 1" forever
        // and logged done/missed days flip back to "planned"). Only ClearPlan then
        // GeneratePlan makes a new anchor.
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(strength_profile()), &mut model)
            .expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();
        // A later launch re-fires with a different "today".
        app.update(Event::GeneratePlan { start_epoch_day: MON + 14 }, &mut model)
            .expect_only_render();
        assert_eq!(
            model.plan_request.as_ref().map(|p| p.start_epoch_day),
            Some(MON),
            "re-firing GeneratePlan must keep the ORIGINAL anchor",
        );
        // ClearPlan then GeneratePlan DOES re-anchor (a deliberate fresh plan).
        app.update(Event::ClearPlan, &mut model).expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON + 14 }, &mut model)
            .expect_only_render();
        assert_eq!(
            model.plan_request.as_ref().map(|p| p.start_epoch_day),
            Some(MON + 14),
            "after ClearPlan, GeneratePlan takes the new day",
        );
    }

    #[test]
    fn plan_leads_with_an_anchored_prescription() {
        let (app, model) = planned_strength_model(true);
        let vm = app.view(&model);

        let ns = vm.next_session.expect("a plan yields a next session");
        assert_eq!(ns.status, "next");
        assert_eq!(vm.week_plan.len(), 7, "a full week is dated");
        assert!(vm.program.is_some(), "the program summary is present");

        // The logged Back Squat is prescribed by %1RM → a concrete load.
        let sq = ns
            .items
            .iter()
            .find(|i| i.exercise.eq_ignore_ascii_case("Back Squat"))
            .expect("the user's own lift leads the session");
        let load = sq.load_kg.expect("an anchored lift shows a kg load");
        assert!(load > 0.0, "load must be positive: {load}");
        assert!(!sq.anchored_on.is_empty(), "the honesty line names the e1RM");
        assert!(!sq.grade.is_empty(), "HARD RULE 2: evidence travels with it");

        // The headline leads with the prescription.
        assert_eq!(vm.today_headline.kind, "prescription");
        assert!(vm.today_headline.summary.starts_with("Next:"));
    }

    #[test]
    fn plan_falls_back_to_rir_without_an_anchor() {
        let (app, model) = planned_strength_model(false);
        let vm = app.view(&model);
        let ns = vm.next_session.expect("plan without history still plans");
        assert!(!ns.items.is_empty());
        for it in &ns.items {
            assert!(
                it.load_kg.is_none(),
                "no e1RM anchor → no invented load (HARD RULE 1): {}",
                it.summary
            );
            assert!(
                it.intensity_label.starts_with("RIR"),
                "unanchored lifts are RIR-prescribed: {}",
                it.intensity_label
            );
        }
    }

    #[test]
    fn prescription_why_is_a_complete_three_part_disclosure() {
        // A prescription card must carry the same 3-part "why?" as adjustment /
        // guidance cards: basis → grade_note → improves, none empty. The
        // `improves` line is an ENGINE data-need (which input sharpens the load),
        // never a training claim (HARD RULE 1).

        // Word-level no-prescriptive-language guard, mirroring the HR-zones why
        // test. Bans specific exercise names + volume/intensity prescriptions.
        // ("set" singular / "rir" / "e1rm" are the engine's own honest vocabulary
        // on a lift card: a logged set and the RIR method are what the data-need
        // is *about*, not a prescription to train, so they are not banned; the
        // singular/plural split keeps "set" from tripping on "sets".)
        let assert_data_need = |improves: &str| {
            assert!(
                !improves.is_empty(),
                "improves must be populated (3-part complete): {improves}"
            );
            let banned = ["squat", "deadlift", "bench", "%1rm", "rpe", "reps", "sets"];
            let words: Vec<String> = improves
                .to_lowercase()
                .split(|c: char| !c.is_alphanumeric() && c != '%')
                .map(|w| w.to_string())
                .collect();
            for b in banned {
                assert!(
                    !words.iter().any(|w| w == b),
                    "improves must not prescribe training: {improves}"
                );
            }
        };

        let check = |it: &PrescriptionView| {
            assert!(
                !it.why.basis.is_empty(),
                "basis must state the method behind the call: {}",
                it.summary
            );
            assert!(
                !it.why.grade_note.is_empty(),
                "grade_note must gloss the evidence grade: {}",
                it.summary
            );
            assert_data_need(&it.why.improves);
        };

        // Anchored (%1RM × logged e1RM): the improves line is about keeping the
        // e1RM anchor fresh.
        let (app, model) = planned_strength_model(true);
        let ns = app.view(&model).next_session.expect("anchored plan");
        let sq = ns
            .items
            .iter()
            .find(|i| i.exercise.eq_ignore_ascii_case("Back Squat"))
            .expect("the user's anchored lift");
        assert!(sq.load_kg.is_some(), "this arm is the anchored case");
        check(sq);
        assert!(
            sq.why.improves.to_lowercase().contains("e1rm anchor"),
            "anchored improves points at the e1RM anchor: {}",
            sq.why.improves
        );

        // RIR fallback (no e1RM anchor yet): the improves line is about logging a
        // set so loads can anchor to a measured e1RM.
        let (app, model) = planned_strength_model(false);
        let ns = app.view(&model).next_session.expect("RIR plan");
        let it = ns
            .items
            .iter()
            .find(|i| i.intensity_label.starts_with("RIR"))
            .expect("an unanchored RIR lift");
        assert!(it.load_kg.is_none(), "this arm is the unanchored case");
        check(it);
        assert!(
            it.why.improves.to_lowercase().contains("measured e1rm"),
            "RIR improves points at anchoring to a measured e1RM: {}",
            it.why.improves
        );
    }

    #[test]
    fn readiness_downgrade_caps_the_shown_session() {
        let (app, mut model) = planned_strength_model(true);
        // Baseline load before any readiness input.
        let base = app
            .view(&model)
            .next_session
            .unwrap()
            .items
            .iter()
            .find(|i| i.exercise.eq_ignore_ascii_case("Back Squat"))
            .unwrap()
            .load_kg
            .unwrap();

        // RPE 2 over target → autoreg ReduceLoadPct(10) (AUTOREG-RIR-001).
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Rpe, 2.0)),
            &mut model,
        )
        .expect_only_render();

        let ns = app.view(&model).next_session.unwrap();
        assert_eq!(ns.status, "adjusted", "the session is marked adjusted");
        assert!(ns.adjustment.is_some(), "the adjustment's evidence rides along");
        let sq = ns
            .items
            .iter()
            .find(|i| i.exercise.eq_ignore_ascii_case("Back Squat"))
            .unwrap();
        assert!(
            sq.load_kg.unwrap() < base,
            "readiness must cap the top-end: {} !< {}",
            sq.load_kg.unwrap(),
            base
        );
        assert!(!sq.adjusted_note.is_empty(), "the cap is explained on the item");
    }

    #[test]
    fn cap_rpe_notes_only_the_items_it_relabels() {
        // CapRpe relabels only RIR-prescribed lifts. A run (or a %1RM lift) it
        // never touches must NOT get a spurious "cap RPE" note: that note is the
        // item-level explanation of a change that, for those items, didn't happen.
        let lift = PrescriptionView {
            exercise: "Back Squat".into(),
            intensity_label: "RIR 3".into(),
            ..Default::default()
        };
        let run = PrescriptionView {
            exercise: String::new(),
            intensity_label: "Easy pace".into(),
            ..Default::default()
        };
        let mut ns = SessionPlanView {
            items: vec![lift, run],
            ..Default::default()
        };
        let recs = vec![graded(Adjustment::CapRpe(1.0), "AUTOREG-E1RM-GATE-001")];
        apply_adjustments_to_session(&mut ns, &recs);

        assert_eq!(ns.items[0].intensity_label, "RIR 4", "the RIR lift is capped");
        assert!(
            !ns.items[0].adjusted_note.is_empty(),
            "the relabeled lift carries the cap note"
        );
        assert!(
            ns.items[1].adjusted_note.is_empty(),
            "the untouched run must not get a false cap note"
        );
        assert_eq!(ns.items[1].intensity_label, "Easy pace", "the run is unchanged");
    }

    #[test]
    fn a_do_not_train_hold_blocks_the_prescription() {
        let (app, mut model) = planned_strength_model(true);
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)),
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert!(vm.train_blocked, "a bare pain report holds training");
        let ns = vm.next_session.expect("the next session still renders, but blocked");
        assert_eq!(ns.status, "blocked");
        assert!(ns.items.is_empty(), "no load numbers are shown through a hold");
        // The safety hold, never the prescription, owns the headline.
        assert_eq!(vm.today_headline.kind, "safety_hold");
    }

    #[test]
    fn a_gate_profile_shows_no_plan() {
        let app = Engine;
        let mut model = Model::default();
        let mut p = strength_profile();
        p.health.reds_signal = true; // medical deferral (HARD RULE 3)
        app.update(Event::SetProfile(p), &mut model).expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.next_session.is_none(), "a gated profile never gets a plan");
        assert!(vm.train_blocked, "the deferral holds training");
    }

    // ── Run planning reactive to logged run history ──

    /// A pure-running half-marathon profile whose STATED weekly volume is the low
    /// guided-setup heuristic (16 km/wk) that never updates on its own.
    fn running_profile() -> Profile {
        Profile {
            lift_goal: LiftGoal::MaxStrength,
            goal_distance: GoalDistance::HalfMarathon,
            concurrent_goal: ConcurrentGoal::Strength,
            weekly_sets: 0,
            running_days_per_week: 3,
            running_km_per_week: 16.0,
            advanced: false,
            ..sample_profile()
        }
    }

    fn logrun(distance_km: f64, day: i64) -> Event {
        Event::LogRun {
            distance_km,
            duration_min: distance_km * 6.0,
            hr_pct_max: 70.0,
            longest_recent_km: 0.0,
            observed_at: day * 86_400,
            entry_id: 0,
            workout_type: None,
        }
    }

    fn planned_long_run_item(app: &Engine, model: &Model) -> PrescriptionView {
        app.view(model)
            .week_plan
            .iter()
            .find(|s| s.title == "Long run")
            .expect("a running week has a long run")
            .items[0]
            .clone()
    }

    fn planned_long_run_summary(app: &Engine, model: &Model) -> String {
        planned_long_run_item(app, model).summary
    }

    #[test]
    fn the_long_run_anchors_to_logged_recent_distance_not_a_stale_profile() {
        // Profile claims 16 km/wk over 3 run days, but the athlete has logged two
        // recent 21 km runs. The plan reacts to logged capacity, but the
        // rework treats the demonstrated run as a CAPACITY CEILING bounded by the
        // ≤2×-daily-average and ≤10%-spike guardrails (running.rs), NOT a weekly
        // target: floor(2 × 16 / 3) = 10 km binds, so the long run is 10 km (never
        // the old stale-profile 4 km, and never the raw 21 km capacity ceiling).
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(running_profile()), &mut model)
            .expect_only_render();
        app.update(logrun(21.0, MON - 2), &mut model).expect_only_render();
        app.update(logrun(21.0, MON - 1), &mut model).expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();
        app.update(Event::SetToday { epoch_day: MON, utc_offset_sec: 0 }, &mut model)
            .expect_only_render();

        let item = planned_long_run_item(&app, &model);
        // The daily-average cap binds: floor(2 × 16 / 3) = 10 km.
        assert!(
            item.summary.starts_with("10 km"),
            "long run must obey the daily-average ceiling (10 km), got {:?}",
            item.summary
        );
        // A within-guardrail long run stays on the share rule (RUN-LONGRUN-001,
        // Daniels): it no longer exceeds a cap, so no spike re-point.
        assert!(
            item.citation.contains("Daniels"),
            "a within-guardrail long run cites the share rule, got {:?}",
            item.citation
        );
        assert!(
            !item.citation.contains("Frandsen"),
            "no spike re-point when the run stays under the guardrails, got {:?}",
            item.citation
        );
        assert_eq!(item.grade, "ExpertOpinion", "RUN-LONGRUN-001 is ExpertOpinion-graded");
        assert!(
            item.why.basis.contains("Long runs"),
            "the basis states the long-run share rule, got {:?}",
            item.why.basis
        );
    }

    #[test]
    fn a_volume_dominated_long_run_card_still_cites_the_share_cap() {
        // Counterpart: a long run set by the weekly-volume 25% rule keeps citing
        // RUN-LONGRUN-001 (Daniels) exactly as before.
        let app = Engine;
        let mut model = Model::default();
        let mut p = running_profile();
        p.running_km_per_week = 60.0; // floor(0.25×60) = 15 km, volume-dominated
        app.update(Event::SetProfile(p), &mut model).expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();
        app.update(Event::SetToday { epoch_day: MON, utc_offset_sec: 0 }, &mut model)
            .expect_only_render();

        let item = planned_long_run_item(&app, &model);
        assert!(item.summary.starts_with("15 km"));
        assert!(
            item.citation.contains("Daniels"),
            "a volume-dominated long run keeps the ≤25% share citation, got {:?}",
            item.citation
        );
    }

    #[test]
    fn a_stale_run_outside_the_window_does_not_raise_the_long_run() {
        // A FULLY-detrained 60-day-old 21 km run is past the 30 + 28 = 58-day decay
        // horizon → zero capacity credit → the plan falls back to the stated-volume
        // rule (the KB "> 8 wk off → rebuild base" regime). Within the horizon the
        // anchor TAPERS instead of vanishing (covered by the taper tests); this guards
        // the far end where it is finally gone.
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(running_profile()), &mut model)
            .expect_only_render();
        app.update(logrun(21.0, MON - 60), &mut model).expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();
        app.update(Event::SetToday { epoch_day: MON, utc_offset_sec: 0 }, &mut model)
            .expect_only_render();

        let summary = planned_long_run_summary(&app, &model);
        // Fully-detrained run ignored → floor(0.25 × 16) = 4 km (as with no history).
        assert!(
            summary.starts_with("4 km"),
            "a fully-detrained run must not raise the long run, got {summary:?}"
        );
    }

    #[test]
    fn a_log_less_running_plan_is_unchanged_by_the_run_anchors() {
        // Regression: with no logged runs the long run is the pre-anchor value.
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(running_profile()), &mut model)
            .expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();
        app.update(Event::SetToday { epoch_day: MON, utc_offset_sec: 0 }, &mut model)
            .expect_only_render();
        assert!(planned_long_run_summary(&app, &model).starts_with("4 km"));
    }

    #[test]
    fn the_run_reactive_plan_is_deterministic_across_repeated_views() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(running_profile()), &mut model)
            .expect_only_render();
        app.update(logrun(21.0, MON - 2), &mut model).expect_only_render();
        app.update(logrun(18.0, MON - 1), &mut model).expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();
        app.update(Event::SetToday { epoch_day: MON, utc_offset_sec: 0 }, &mut model)
            .expect_only_render();
        let a = app.view(&model);
        let b = app.view(&model);
        assert_eq!(a.week_plan, b.week_plan, "same model must yield the same week");
        assert_eq!(a.next_session, b.next_session);
    }

    #[test]
    fn build_run_anchors_uses_the_trailing_windows() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(running_profile()), &mut model)
            .expect_only_render();
        let today = 400i64;
        // In-window: 21 km (2 d ago) and 10 km (5 d ago).
        app.update(logrun(21.0, today - 2), &mut model).expect_only_render();
        app.update(logrun(10.0, today - 5), &mut model).expect_only_render();
        // Stale: 30 km 40 d ago, outside both the 30-day and 28-day windows.
        app.update(logrun(30.0, today - 40), &mut model).expect_only_render();

        let (longest, weekly, _) = build_run_anchors(&model.runs, today, 0);
        assert_eq!(longest, Some(21.0), "the stale 30 km run must not anchor");
        // recent_weekly = (21 + 10) / 4 = 7.75.
        assert_eq!(weekly, Some(7.75));

        // A future-dated row (after `today`) is excluded, like `spike_baseline_km`.
        app.update(logrun(99.0, today + 5), &mut model).expect_only_render();
        let (longest2, _, _) = build_run_anchors(&model.runs, today, 0);
        assert_eq!(longest2, Some(21.0), "a future-dated run must not anchor");
    }

    #[test]
    fn declared_layoff_derates_the_lift_load_not_the_logged_best() {
        // 2c: after a declared layoff the WORKING LOAD is derated (REENTRY-001),
        // but the e1RM anchor stays the true logged best so the honesty line
        // never lies. build_plan_anchors keeps the true e1RM + carries the frac.
        let sets = vec![LoggedSet {
            exercise: "Back Squat".into(),
            weight_kg: 140.0,
            reps: 5,
            rpe: 8.0,
            observed_at: 1_000,
            entry_id: 1,
        }];
        let fresh = build_plan_anchors(&sets, None);
        let returning = build_plan_anchors(&sets, Some(6.0));
        assert_eq!(
            fresh.lift_e1rm[0].1, returning.lift_e1rm[0].1,
            "the e1RM anchor is the true logged best either way"
        );
        assert_eq!(fresh.reentry_load_frac, None);
        assert_eq!(returning.reentry_load_frac, Some(0.70), "6 wk off → 0.70 re-entry fraction");

        // Flatten an anchored %1RM prescription with vs without the derate.
        let pres = Prescription::Lift(crate::schema::LiftPrescription {
            exercise: "Back Squat".into(),
            sets: 3,
            reps: 5,
            intensity: LiftIntensity::PercentOneRm(80.0),
            rest_sec: 180,
            tempo: None,
            velocity_loss_pct: None,
        });
        let rx = graded(pres, "STR-INTENT-001");
        let anchors_full = crate::plan::Anchors {
            lift_e1rm: vec![("Back Squat".into(), 100.0)],
            ..Default::default()
        };
        let anchors_re = crate::plan::Anchors {
            lift_e1rm: vec![("Back Squat".into(), 100.0)],
            reentry_load_frac: Some(0.70),
            ..Default::default()
        };
        let full = flatten_prescription(&rx, &anchors_full);
        let re = flatten_prescription(&rx, &anchors_re);
        assert_eq!(full.load_kg, Some(round_2_5(100.0 * 0.80)));
        assert_eq!(re.load_kg, Some(round_2_5(100.0 * 0.80 * 0.70)));
        assert!(re.load_kg.unwrap() < full.load_kg.unwrap(), "the layoff load is lighter");
        assert!(
            re.anchored_on.contains("100.0 kg (your logged best)"),
            "the logged best stays honest: {}",
            re.anchored_on
        );
        assert_eq!(re.grade, "ExpertOpinion", "derated load re-points to REENTRY-001");
        assert_eq!(full.grade, "Strong", "the full load keeps its loading-band grade");
        // The why-panel note must match the re-pointed chip, not the stale band.
        assert!(
            re.why.grade_note.contains("Expert opinion"),
            "grade_note follows the re-pointed chip: {}",
            re.why.grade_note
        );
        assert!(
            !re.why.grade_note.contains("Strong evidence"),
            "no stale Strong note on a derated card"
        );
        assert!(
            full.why.grade_note.contains("Strong evidence"),
            "the full card keeps its band note: {}",
            full.why.grade_note
        );
    }

    #[test]
    fn long_layoff_drops_the_anchor_and_represcribes_as_novice() {
        // A1: a >8 wk layoff carries NO KB load fraction (Table 3.4b) and directs
        // a fresh-novice re-entry, so `build_plan_anchors` sets `reentry_novice`
        // (not a fraction) and `flatten_prescription` drops the e1RM anchor
        // entirely: `plan.rs::lift_prescription` prescribes it by RIR (treat as
        // novice, technique first), never a scaled %e1RM load (HARD RULE 1). The
        // card still cites REENTRY-001 so it explains the re-entry reason.
        let sets = vec![LoggedSet {
            exercise: "Back Squat".into(),
            weight_kg: 140.0,
            reps: 5,
            rpe: 8.0,
            observed_at: 1_000,
            entry_id: 1,
        }];
        let anchors = build_plan_anchors(&sets, Some(12.0));
        assert!(anchors.reentry_novice, "12 wk off → treat as a fresh novice");
        assert_eq!(
            anchors.reentry_load_frac, None,
            "no invented load fraction beyond 8 wk"
        );
        // The e1RM anchor itself is still the true logged best (never lost).
        assert!(anchors.lift_e1rm[0].1 > 0.0);

        // >8 wk re-entry: the pipeline prescribes by RIR, not a %1RM anchor.
        let pres = Prescription::Lift(crate::schema::LiftPrescription {
            exercise: "Back Squat".into(),
            sets: 3,
            reps: 5,
            intensity: LiftIntensity::Rir(3),
            rest_sec: 180,
            tempo: None,
            velocity_loss_pct: None,
        });
        let rx = graded(pres, "STR-INTENT-001");
        let item = flatten_prescription(&rx, &anchors);
        // No anchor-derived %e1RM load: a novice re-entry trains by RIR (HARD RULE 1).
        assert_eq!(item.load_kg, None, "no %e1RM load on a novice re-entry");
        assert!(
            item.intensity_label.starts_with("RIR"),
            "novice re-entry is prescribed by RIR, not % e1RM: {}",
            item.intensity_label
        );
        assert!(
            item.anchored_on.is_empty(),
            "no logged-best load line when the anchor is set aside: {}",
            item.anchored_on
        );
        // The card re-points to REENTRY-001 (ExpertOpinion), citing the reason.
        assert_eq!(item.grade, "ExpertOpinion", "re-points to REENTRY-001");
        assert!(
            item.why.grade_note.contains("Expert opinion"),
            "grade_note follows the re-pointed chip: {}",
            item.why.grade_note
        );
    }

    /// Build a run history via the real ingest path with explicit timestamps.
    fn runs_via_log(stamps: &[(f64, i64)]) -> Vec<LoggedRun> {
        let app = Engine;
        let mut m = Model::default();
        for (i, (km, at)) in stamps.iter().enumerate() {
            app.update(log_run(*km, km * 6.0, 70.0, 0.0, *at, i as u64 + 1), &mut m)
                .expect_only_render();
        }
        m.runs
    }

    // ── Run anchors bucket by LOCAL day, so a run logged today counts ──
    #[test]
    fn run_anchors_count_a_same_day_run_by_local_day_h1() {
        let today = 500i64;

        // Berlin (+2 h): a run at 08:00 LOCAL today. Its UTC `observed_at` is later
        // than `today*86400`, which the old UTC-instant predicate wrongly excluded.
        let berlin = 2 * 3600;
        let berlin_0800 = today * DAY_SEC + 8 * 3600 - berlin;
        let runs_b = runs_via_log(&[(12.0, berlin_0800)]);
        let (longest_b, weekly_b, _) = build_run_anchors(&runs_b, today, berlin);
        assert_eq!(longest_b, Some(12.0), "an 08:00-today run (Berlin) must count");
        assert_eq!(weekly_b, Some(3.0));

        // UTC−5: an evening run today whose UTC stamp rolls into the next UTC day.
        let west = -5 * 3600;
        let west_2000 = today * DAY_SEC + 20 * 3600 - west;
        let runs_w = runs_via_log(&[(9.0, west_2000)]);
        let (longest_w, _, _) = build_run_anchors(&runs_w, today, west);
        assert_eq!(longest_w, Some(9.0), "a 20:00-today run (UTC−5) must count");

        // A run on local TOMORROW is future-dated and excluded.
        let tomorrow_0030 = (today + 1) * DAY_SEC + 30 * 60 - berlin;
        let runs_f = runs_via_log(&[(30.0, tomorrow_0030)]);
        let (longest_f, weekly_f, _) = build_run_anchors(&runs_f, today, berlin);
        assert_eq!(longest_f, None, "a local-tomorrow run must not anchor");
        assert_eq!(weekly_f, None);
    }

    #[test]
    fn run_anchors_offset_zero_midnight_history_is_byte_identical_h1() {
        // The FULL-CREDIT window edge for an offset-0, midnight-stamped history is
        // unchanged: a run exactly 30 days ago still anchors at full distance and
        // is NOT flagged detrained (byte-identical to the pre-taper behaviour).
        let today = 500i64;
        let at_30 = (today - 30) * DAY_SEC;
        let (l30, _, det30) = build_run_anchors(&runs_via_log(&[(8.0, at_30)]), today, 0);
        assert_eq!(l30, Some(8.0), "a run exactly 30 days ago still anchors at full");
        assert!(!det30, "a run at the window edge is full-credit, not detrained");
        // A run 31 days ago no longer VANISHES (the old cliff); its credit is
        // tapered by one day of the 28-day detraining slope: 8 × 27/28 = 7.714…
        let at_31 = (today - 31) * DAY_SEC;
        let (l31, _, det31) = build_run_anchors(&runs_via_log(&[(8.0, at_31)]), today, 0);
        let expected_31 = 8.0 * 27.0 / 28.0;
        assert!(
            (l31.unwrap() - expected_31).abs() < 1e-9,
            "day-31 taper: got {l31:?}, want {expected_31}"
        );
        assert!(det31, "a beyond-window anchor is flagged detraining-adjusted");
    }

    // ── The window-expiry cliff is now a detraining slope ──
    #[test]
    fn h4_longest_run_anchor_tapers_past_the_window_instead_of_a_cliff() {
        // Flagship: a lone 21.1 km race is the only logged run. Across the 30-day
        // full-credit edge the anchor must SLOPE (retained then decaying) rather
        // than drop from 21.1 to None in one overnight step.
        let today = 1_000i64;
        let race_at = |age: i64| (today - age) * DAY_SEC;
        let anchor = |age: i64| build_run_anchors(&runs_via_log(&[(21.1, race_at(age))]), today, 0);

        // Day 30: full credit, not detrained (byte-identical to the old rule).
        let (l30, _, d30) = anchor(30);
        assert!((l30.unwrap() - 21.1).abs() < 1e-9, "day 30 full: {l30:?}");
        assert!(!d30);

        // Day 31: retained, tapered by one slope-day (21.1 × 27/28 = 20.346…) -
        // a ~0.75 km step, NOT the old 21.1 → None cliff. Detraining-adjusted.
        let (l31, _, d31) = anchor(31);
        let want31 = 21.1 * 27.0 / 28.0;
        assert!((l31.unwrap() - want31).abs() < 1e-9, "day 31: {l31:?} want {want31}");
        assert!(d31, "beyond-window anchor is detraining-adjusted");
        assert!(l31.unwrap() > 20.0, "the step off the window is small, not a cliff");

        // Day 45: 15 slope-days in → 21.1 × 13/28 = 9.796… (still a real anchor).
        let (l45, _, d45) = anchor(45);
        let want45 = 21.1 * 13.0 / 28.0;
        assert!((l45.unwrap() - want45).abs() < 1e-9, "day 45: {l45:?} want {want45}");
        assert!(d45);

        // The trajectory is monotonically decreasing across the boundary, a slope.
        assert!(l30.unwrap() > l31.unwrap() && l31.unwrap() > l45.unwrap());

        // Day 58 (30 + 28): fully detrained → no capacity credit (aligns with the
        // KB "4–8 wk off → treat near-novice" bracket). Day 57 still carries a sliver.
        let (l58, _, _) = anchor(58);
        assert_eq!(l58, None, "at day 58 the demonstrated-capacity credit is gone");
        let (l57, _, _) = anchor(57);
        assert!(l57.unwrap() > 0.0 && l57.unwrap() < 1.0, "day 57 is a sliver: {l57:?}");
    }

    #[test]
    fn h4_a_fresh_run_overrides_a_decayed_older_run_no_change() {
        // A fresh in-window run that is longer than any decayed older run keeps the
        // anchor at full credit and NOT detrained, byte-identical to the old rule,
        // so the taper only ever RAISES the cliff case, never perturbs live history.
        let today = 1_000i64;
        let runs = runs_via_log(&[
            (21.1, (today - 40) * DAY_SEC), // old race, decays to 21.1×18/28 = 13.56
            (16.0, (today - 3) * DAY_SEC),  // fresh 16 km run beats the decayed race
        ]);
        let (l, _, det) = build_run_anchors(&runs, today, 0);
        assert_eq!(l, Some(16.0), "the fresh run wins");
        assert!(!det, "a fresh winning anchor is not detraining-adjusted");
    }

    // ── A future-dated run must not hijack weekly report or CTL/ATL ──
    #[test]
    fn a_future_dated_run_does_not_hijack_weekly_report_or_training_load_m4() {
        let app = Engine;
        let mut model = Model::default();
        // Real recent history: a 10 km HR run.
        app.update(log_run(10.0, 60.0, 75.0, 0.0, 100 * DAY_SEC, 1), &mut model)
            .expect_only_render();
        // A mis-imported 2030-dated GPX (far future).
        app.update(log_run(5.0, 30.0, 60.0, 0.0, 22_000 * DAY_SEC, 2), &mut model)
            .expect_only_render();
        app.update(
            Event::SetToday { epoch_day: 105, utc_offset_sec: 0 },
            &mut model,
        )
        .expect_only_render();

        let with_future = app.view(&model);
        // Training load counts only the real run: the 2030 row is skipped, so the
        // MAX_LOAD_DAYS window can't slide off the real history.
        let tl = with_future.training_load.clone().expect("training load present");
        assert_eq!(tl.sessions_counted, 1, "the future run must be excluded from CTL/ATL");

        // Deleting the future run leaves BOTH surfaces byte-identical, proof it
        // was fully ignored while present.
        app.update(
            Event::DeleteEntry { kind: EntryKind::Run, entry_id: 2, observed_at_fallback: 0 },
            &mut model,
        )
        .expect_only_render();
        let without = app.view(&model);
        assert_eq!(
            with_future.weekly_report, without.weekly_report,
            "the 2030 run must not affect the weekly report"
        );
        // CTL/ATL/TSB + counted are identical whether or not the future run is
        // present: it only ever contributes to the `skipped` tally.
        let tl2 = without.training_load.expect("training load present");
        assert_eq!((tl.ctl, tl.atl, tl.tsb), (tl2.ctl, tl2.atl, tl2.tsb));
        assert_eq!(tl.sessions_counted, tl2.sessions_counted);
    }

    // ── Deleting the baseline run re-arms the derived spike gate ──
    #[test]
    fn deleting_the_baseline_run_re_arms_the_spike_gate_m6() {
        let app = Engine;
        let mut model = Model::default();
        // 20 km, then a 30 km typo, then a 23 km run.
        app.update(log_run(20.0, 100.0, 70.0, 0.0, DAY_SEC, 1), &mut model)
            .expect_only_render();
        app.update(log_run(30.0, 150.0, 70.0, 0.0, 5 * DAY_SEC, 2), &mut model)
            .expect_only_render();
        app.update(log_run(23.0, 115.0, 70.0, 0.0, 10 * DAY_SEC, 3), &mut model)
            .expect_only_render();
        app.update(Event::SubmitReview(SessionReview::default()), &mut model)
            .expect_only_render();

        // With the 30 km baseline standing, the latest 23 km run is not a spike.
        let before = app.view(&model).feedback.expect("feedback present");
        assert_ne!(
            before.category, "DangerousProgression",
            "23 km is not a spike over the standing 30 km baseline"
        );

        // Delete the 30 km typo: the true recent longest is now 20 km, so 23 km is
        // a +15 % spike: the DERIVED gate re-arms (the stale stored 30 would not).
        app.update(
            Event::DeleteEntry { kind: EntryKind::Run, entry_id: 2, observed_at_fallback: 0 },
            &mut model,
        )
        .expect_only_render();
        let after = app.view(&model).feedback.expect("feedback present");
        assert_eq!(
            after.category, "DangerousProgression",
            "deleting the 30 km baseline must re-arm the spike gate"
        );
    }

    // ── Stale performance signals expire; safety signals never do ──
    fn rpe_input_at(value: f64, observed_at: i64) -> ReadinessInput {
        ReadinessInput {
            signal: ReadinessSignal::Rpe,
            value,
            observed_at,
            streak: 0,
            pain: None,
            effort_min: None,
        }
    }

    #[test]
    fn a_stale_felt_easy_rpe_expires_but_a_fresh_one_still_raises_load_m7() {
        let app = Engine;
        let today = 100i64;
        let has_increase =
            |vm: &ViewModel| vm.adjustments.iter().any(|a| a.summary.contains("Increase load"));

        // Fresh RPE −2 (yesterday) → a load increase is proposed.
        let mut fresh = Model::default();
        app.update(Event::SubmitReadiness(rpe_input_at(-2.0, (today - 1) * DAY_SEC)), &mut fresh)
            .expect_only_render();
        app.update(Event::SetToday { epoch_day: today, utc_offset_sec: 0 }, &mut fresh)
            .expect_only_render();
        assert!(has_increase(&app.view(&fresh)), "a fresh felt-easy RPE still raises load");

        // Stale RPE −2 (21 days ago) → expired, no increase.
        let mut stale = Model::default();
        app.update(Event::SubmitReadiness(rpe_input_at(-2.0, (today - 21) * DAY_SEC)), &mut stale)
            .expect_only_render();
        app.update(Event::SetToday { epoch_day: today, utc_offset_sec: 0 }, &mut stale)
            .expect_only_render();
        assert!(!has_increase(&app.view(&stale)), "a 21-day-old RPE must expire");

        // With no `today` reference (no clock) the signal is NOT expired.
        let mut noclock = Model::default();
        app.update(Event::SubmitReadiness(rpe_input_at(-2.0, (today - 21) * DAY_SEC)), &mut noclock)
            .expect_only_render();
        assert!(
            has_increase(&app.view(&noclock)),
            "without a today reference the core keeps the signal"
        );

        // A 21-day-old PAIN report must NOT expire: safety persists (HARD RULE 3).
        let mut stale_pain = Model::default();
        app.update(
            Event::SubmitReadiness(ReadinessInput {
                signal: ReadinessSignal::Pain,
                value: 1.0,
                observed_at: (today - 21) * DAY_SEC,
                streak: 0,
                pain: None,
                effort_min: None,
            }),
            &mut stale_pain,
        )
        .expect_only_render();
        app.update(Event::SetToday { epoch_day: today, utc_offset_sec: 0 }, &mut stale_pain)
            .expect_only_render();
        assert!(
            app.view(&stale_pain).train_blocked,
            "a stale pain report must still hold - safety never expires"
        );
    }

    // ── A review-channel deload folds into the rendered next session ──
    #[test]
    fn a_review_deload_reduces_the_rendered_next_session_load_m8() {
        let (app, mut model) = planned_strength_model(true);
        let squat_load = |vm: &ViewModel| {
            vm.next_session
                .as_ref()
                .unwrap()
                .items
                .iter()
                .find(|i| i.exercise.eq_ignore_ascii_case("Back Squat"))
                .unwrap()
                .load_kg
                .unwrap()
        };
        let baseline = squat_load(&app.view(&model));

        // An rpe-load-gap over ≥2 sessions → a standard deload (−10 % load), a
        // NON-blocking review adjustment.
        let review = SessionReview { rpe_load_gap_sessions: Some(2), ..Default::default() };
        app.update(Event::SubmitReview(review), &mut model).expect_only_render();

        let vm = app.view(&model);
        assert!(
            squat_load(&vm) < baseline,
            "the review deload must fold into the rendered session load: {} !< {baseline}",
            squat_load(&vm)
        );
        assert_eq!(vm.next_session.as_ref().unwrap().status, "adjusted");
    }

    // ── Past the block, the week number cycles (maintenance), not pins ──
    #[test]
    fn program_week_cycles_into_maintenance_past_the_block_m13() {
        let (app, mut model) = planned_strength_model(true);
        let prog0 = app.view(&model).program.expect("program present");
        assert_eq!(prog0.week, 1, "in-block week 1 at the start");
        assert!(!prog0.maintenance, "not maintenance while the block runs");
        let weeks_total = prog0.weeks_total as i64;

        // Jump one whole block past the start.
        app.update(
            Event::SetToday { epoch_day: MON + 7 * weeks_total, utc_offset_sec: 0 },
            &mut model,
        )
        .expect_only_render();
        let prog = app.view(&model).program.expect("program present");
        assert!(prog.maintenance, "past the block, the plan is a maintenance cycle");
        assert_eq!(prog.week, 1, "the week cycles to 1, not pinned at weeks_total");
        assert!(prog.week >= 1 && prog.week <= prog.weeks_total);
    }

    // ── Interval rep structure + HR band ceiling copy ──
    #[test]
    fn flatten_surfaces_interval_rep_structure_and_hr_band_ceiling_h5_l3() {
        use crate::schema::RunPrescription;
        let anchors = crate::plan::Anchors::default();

        let interval = graded(
            Prescription::Run(RunPrescription {
                volume: RunVolume::DurationMin(16),
                intensity: RunIntensity::Vdot(VdotBand::Interval),
                repeats: Some((4, RunVolume::DurationMin(4))),
            }),
            "RUN-INTERVAL-001",
        );
        let it = flatten_prescription(&interval, &anchors);
        assert_eq!(it.rep_count, 4);
        assert_eq!(it.rep_volume, "4 min");
        assert!(
            it.summary.starts_with("4 × 4 min"),
            "the summary must lead with rep structure, got {:?}",
            it.summary
        );
        assert!(
            !it.summary.starts_with("16 min"),
            "must not read as one continuous 16 min run"
        );

        // A continuous %HRmax run: rep fields empty, band rendered as a ceiling.
        let cont = graded(
            Prescription::Run(RunPrescription {
                volume: RunVolume::DistanceKm(10.0),
                intensity: RunIntensity::HrPercentMax(65.0),
                repeats: None,
            }),
            "RUN-LONGRUN-001",
        );
        let ci = flatten_prescription(&cont, &anchors);
        assert_eq!(ci.rep_count, 0);
        assert!(ci.rep_volume.is_empty());
        assert_eq!(
            ci.summary, "10 km · ≤ 65% HRmax",
            "a %HRmax target renders as a ≤ ceiling, not a point"
        );
    }

    // ── SetProfile sanitizes poison floats on ingest ──
    #[test]
    fn set_profile_sanitizes_poison_floats_l1() {
        let app = Engine;
        let mut model = Model::default();

        let mut p = running_profile();
        p.running_km_per_week = 1e300;
        app.update(Event::SetProfile(p), &mut model).expect_only_render();
        let km = model.profile.as_ref().unwrap().running_km_per_week;
        assert!(km.is_finite() && km <= 1.0e12, "running_km_per_week must clamp, got {km}");

        let mut p2 = running_profile();
        p2.running_km_per_week = -5.0;
        app.update(Event::SetProfile(p2), &mut model).expect_only_render();
        assert_eq!(
            model.profile.as_ref().unwrap().running_km_per_week,
            0.0,
            "a negative weekly volume floors at 0"
        );

        // A NaN age sanitizes to 0 → the youth gate fires (conservative).
        let mut p3 = strength_profile();
        p3.age_years = Some(f64::NAN);
        app.update(Event::SetProfile(p3), &mut model).expect_only_render();
        assert!(
            model.profile.as_ref().unwrap().health.youth,
            "a NaN age must sanitize to 0 and fire the youth gate"
        );
    }

    // ── A negative LogRun distance is clamped at ingest ──
    #[test]
    fn negative_log_run_distance_is_clamped_l2() {
        let app = Engine;
        let mut model = Model::default();
        app.update(log_run(-10.0, 60.0, 70.0, 0.0, DAY_SEC, 1), &mut model)
            .expect_only_render();
        assert!(
            model.runs[0].distance_km >= 0.0,
            "a negative distance must clamp to ≥0, got {}",
            model.runs[0].distance_km
        );
        // The measured weekly anchor is never driven negative by it.
        let (_, weekly, _) = build_run_anchors(&model.runs, 2, 0);
        assert!(weekly.is_none_or(|w| w >= 0.0), "weekly anchor must not go negative");
    }

    #[test]
    fn week_plan_marks_a_logged_day_done() {
        // Today = MON; log today's (MON) Heavy session. A logged TODAY reads
        // "done" (accomplished), and the hero advances past it to the next
        // upcoming session.
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(strength_profile()), &mut model)
            .expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();
        app.update(Event::SetToday { epoch_day: MON, utc_offset_sec: 0 }, &mut model)
            .expect_only_render();
        // A Back Squat set observed on MON = today logs today's session.
        app.update(logset("Back Squat", 120.0, 3), &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        let mon_row = vm
            .week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .expect("Monday is in the week strip");
        assert_eq!(mon_row.status, "done", "a logged TODAY reads done");
        let ns = vm.next_session.as_ref().expect("a next session after today");
        assert!(
            ns.epoch_day > MON,
            "the hero advances past a done today, got epoch_day {}",
            ns.epoch_day
        );

        // A PAST logged day is also "done": move today forward two days (to Wed)
        // so Monday's logged Heavy day sits in the past.
        let app2 = Engine;
        let mut m2 = Model::default();
        app2.update(Event::SetProfile(strength_profile()), &mut m2)
            .expect_only_render();
        app2.update(logset("Back Squat", 120.0, 3), &mut m2)
            .expect_only_render();
        app2.update(Event::GeneratePlan { start_epoch_day: MON }, &mut m2)
            .expect_only_render();
        // Today = Wed (MON+2); Monday's Heavy day is in the past with a logged set.
        app2.update(Event::SetToday { epoch_day: MON + 2, utc_offset_sec: 0 }, &mut m2)
            .expect_only_render();
        let vm2 = app2.view(&m2);
        let mon_row = vm2
            .week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .expect("Monday is in the week strip");
        assert_eq!(mon_row.status, "done", "a past logged training day reads done");
    }

    #[test]
    fn week_plan_unlogged_today_stays_next() {
        // Today = MON, plan generated but NO set logged: today's training day is
        // still the "next" session, dated today.
        let (app, model) = planned_strength_model(false);
        let vm = app.view(&model);
        let mon_row = vm
            .week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .expect("Monday is in the week strip");
        assert_ne!(mon_row.status, "done", "an unlogged today is not done");
        let ns = vm.next_session.as_ref().expect("a next session on today");
        assert_eq!(ns.status, "next", "an unlogged today's session is next");
        assert_eq!(ns.epoch_day, MON, "the next session is dated today");
    }

    // ── Youth derived from age alone gates the view + blocks the plan ──
    #[test]
    fn a1_age_15_gates_even_with_empty_health_screen() {
        let app = Engine;
        let mut model = Model::default();
        // A 15 yo with a SKIPPED health screen (all flags false) + a plan request.
        let youth = Profile {
            age_years: Some(15.0),
            health: HealthScreen::default(),
            ..strength_profile()
        };
        app.update(Event::SetProfile(youth), &mut model)
            .expect_only_render();
        app.update(logset("Back Squat", 120.0, 3), &mut model)
            .expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();

        // The pediatric gate fires from age alone: MedicalReferral tier, blocked.
        let vm = app.view(&model);
        assert!(vm.train_blocked, "age 15 must block training");
        assert_eq!(vm.safety_tier.as_deref(), Some("MedicalReferral"));
        // No adult plan renders through the gate.
        assert!(vm.next_session.is_none(), "no plan may render through a gate");
        assert!(vm.program.is_none());
        // And the model's derived flag is set.
        assert!(model.profile.as_ref().unwrap().health.youth);
        // A gates-only hold produces NO adjustment rows: the headline's
        // safety_hold rung is therefore the ONLY cited surface for this state,
        // and the shells (blocked-branch card + SafetyBanner why-panel) render
        // from it. Pin that contract: kind, evidence, and a real citation.
        assert!(vm.adjustments.is_empty(), "gates emit no adjustment rows");
        assert_eq!(vm.today_headline.kind, "safety_hold");
        assert!(vm.today_headline.safety_critical);
        assert!(
            !vm.today_headline.citation.is_empty(),
            "a gates-only hold must still carry its citation to the shell"
        );

        // An 18 yo with the same setup is NOT gated (threshold check).
        let mut m2 = Model::default();
        app.update(
            Event::SetProfile(Profile {
                age_years: Some(18.0),
                ..strength_profile()
            }),
            &mut m2,
        )
        .expect_only_render();
        assert!(!m2.profile.as_ref().unwrap().health.youth, "18 is not youth");
    }

    // ── A tolerable-pain day's headline is the pain call, and the session
    //        prescription is actually capped (ModifyAndMonitor), not full-load ──
    #[test]
    fn a3_tolerable_pain_headline_is_the_pain_call_and_caps_the_session() {
        let (app, mut model) = planned_strength_model(true);
        // Report tolerable, stable tendon pain (sev 3) → PainGate::Adjust
        // (ModifyAndMonitor), not a hard stop.
        app.update(
            Event::SubmitReadiness(ReadinessInput {
                signal: ReadinessSignal::Pain,
                value: 1.0,
                observed_at: MON * 86_400,
                streak: 0,
                pain: Some(crate::schema::PainDetail {
                    kind: crate::schema::PainKind::TendonLoadRelated,
                    severity: 3,
                    trend: crate::schema::PainTrend::Stable,
                    persists: false,
                    location: Some("Left elbow".into()),
                }),
                effort_min: None,
            }),
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert!(!vm.train_blocked, "tolerable pain must not block");
        // The headline is the adjustment (pain) call, NOT the full-load session.
        assert_eq!(
            vm.today_headline.kind, "adjustment",
            "an active adjustment outranks the prescription rung"
        );
        // The session is capped: no heavy load shown, and it is marked adjusted.
        let ns = vm.next_session.expect("still a next session");
        assert_eq!(ns.status, "adjusted");
        for it in ns.items.iter().filter(|i| !i.exercise.is_empty()) {
            assert!(
                it.load_kg.is_none(),
                "a lift must not show a heavy load under active pain: {}",
                it.summary
            );
        }
    }

    // ── LOW (external review): a wrapped next session keeps a clean prescription ──
    #[test]
    fn low_wrapped_next_session_is_not_folded_with_todays_adjustment() {
        // Strength week lifts on days 0/2/4 (Mon/Wed/Fri). With today at day 5
        // (Sat) every training day is behind us, so next_session WRAPS to next
        // Monday (+7). Today's transient readiness downgrade must NOT paint that
        // future-dated session: it would be a wrong-week deload.
        let (app, mut model) = planned_strength_model(true);
        app.update(Event::SetToday { epoch_day: MON + 5, utc_offset_sec: 0 }, &mut model)
            .expect_only_render();
        // A single-day HRV suppression → non-blocking DowngradeSession for TODAY.
        app.update(
            Event::SubmitReadiness(ReadinessInput {
                signal: ReadinessSignal::HrvLnRmssd,
                value: -2.0,
                observed_at: (MON + 5) * 86_400,
                streak: 0,
                pain: None,
                effort_min: None,
            }),
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert!(!vm.train_blocked, "a downgrade must not block training");
        let ns = vm.next_session.expect("a wrapped next session renders");
        assert!(ns.epoch_day > MON + 5, "the next session wrapped past today");
        // Clean prescription: status stays "next" (never "adjusted"), no fold.
        assert_eq!(ns.status, "next", "a future-dated session is not folded/adjusted");
        assert!(ns.adjustment.is_none(), "no adjustment folded into a wrong-week session");
        for it in ns.items.iter() {
            assert!(
                it.adjusted_note.is_empty(),
                "a future-dated item must stay un-adjusted, got note: {}",
                it.adjusted_note
            );
        }
    }

    #[test]
    fn low_wrapped_next_session_still_blanks_under_a_safety_hold() {
        // The blocking/safety path is unchanged: a hold blanks the wrapped
        // session regardless of its date (HARD RULE 3).
        let (app, mut model) = planned_strength_model(true);
        app.update(Event::SetToday { epoch_day: MON + 5, utc_offset_sec: 0 }, &mut model)
            .expect_only_render();
        // Generic pain report → hard Stop → train_blocked.
        app.update(
            Event::SubmitReadiness(ReadinessInput {
                signal: ReadinessSignal::Pain,
                value: 1.0,
                observed_at: (MON + 5) * 86_400,
                streak: 0,
                pain: None,
                effort_min: None,
            }),
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.train_blocked, "generic pain blocks training");
        let ns = vm.next_session.expect("a wrapped next session renders");
        assert!(ns.epoch_day > MON + 5, "the next session wrapped past today");
        assert_eq!(ns.status, "blocked", "a hold blanks even a future-dated session");
        assert!(ns.items.is_empty(), "a blocked session shows no items");
    }

    #[test]
    fn a3_downgrade_caps_a_run_item_not_just_a_note() {
        // A running plan + an HRV downgrade must actually cap the run target.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SetProfile(Profile {
                weekly_sets: 0,
                running_days_per_week: 4,
                running_km_per_week: 40.0,
                goal_distance: GoalDistance::TenK,
                ..strength_profile()
            }),
            &mut model,
        )
        .expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();
        app.update(Event::SetToday { epoch_day: MON, utc_offset_sec: 0 }, &mut model)
            .expect_only_render();
        // A multi-day HRV downtrend → DowngradeSession.
        for d in 0..3 {
            app.update(
                Event::SubmitReadiness(ReadinessInput {
                    signal: ReadinessSignal::HrvLnRmssd,
                    value: -2.0,
                    observed_at: (MON - d) * 86_400,
                    streak: 3,
                    pain: None,
                    effort_min: None,
                }),
                &mut model,
            )
            .expect_only_render();
        }
        let vm = app.view(&model);
        if let Some(ns) = vm.next_session.filter(|n| n.status == "adjusted") {
            let run = ns.items.iter().find(|i| i.exercise.is_empty());
            if let Some(run) = run {
                assert!(
                    run.intensity_label.to_lowercase().contains("easy"),
                    "a downgraded run's intensity target must be capped to easy: {}",
                    run.intensity_label
                );
                assert!(!run.adjusted_note.is_empty(), "the run carries an adjusted note");
                // The chip must cite the DECISION (the HRV downgrade), not the
                // stale hard-run band. HRV multi-day downtrend routes through
                // AUTOREG-HRV-001 → its citation mentions HRV, and the why? basis
                // reflects the downgrade, not a Tempo/Interval band.
                assert!(
                    run.citation.to_lowercase().contains("hrv")
                        || run.why.basis.to_lowercase().contains("downgraded to an easy"),
                    "downgraded run must cite the adjustment, not the original band: \
                     grade={} citation={} basis={}",
                    run.grade,
                    run.citation,
                    run.why.basis,
                );
            }
        }
    }

    // ── The run-spike baseline is a trailing 30-day window, not all-time ──
    #[test]
    fn a4_old_long_run_does_not_suppress_a_current_spike() {
        let app = Engine;
        let mut model = Model::default();
        let day = 86_400i64;
        // A 40 km run 60 days ago (outside the 30-day window).
        app.update(
            log_run(40.0, 240.0, 70.0, 0.0, 100 * day, 0),
            &mut model,
        )
        .expect_only_render();
        // A 10 km run today (day 160): its baseline must NOT include the old 40 km.
        app.update(
            log_run(10.0, 60.0, 75.0, 0.0, 160 * day, 0),
            &mut model,
        )
        .expect_only_render();
        let today = model.runs.last().unwrap();
        assert!(
            today.longest_recent_km < 40.0,
            "an all-time baseline leaked in: {}",
            today.longest_recent_km
        );
    }

    // ── A 0 kg set never anchors the plan (no "@ 0 kg") ──
    #[test]
    fn b3_zero_weight_set_does_not_anchor_a_load() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(strength_profile()), &mut model)
            .expect_only_render();
        // Only a 0 kg "set" is logged for this exercise.
        app.update(logset("Back Squat", 0.0, 5), &mut model)
            .expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();
        app.update(Event::SetToday { epoch_day: MON, utc_offset_sec: 0 }, &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        let ns = vm.next_session.expect("a plan renders");
        for it in ns.items.iter().filter(|i| !i.exercise.is_empty()) {
            assert!(
                it.load_kg.map_or(true, |kg| kg > 0.0),
                "a 0 kg set must never anchor a load: {}",
                it.summary
            );
            assert!(!it.summary.contains("@ 0 kg"), "no @ 0 kg: {}", it.summary);
        }
    }

    // ── Session-done matching buckets by the shell's LOCAL day ──
    #[test]
    fn b5_session_done_matches_by_local_day() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(strength_profile()), &mut model)
            .expect_only_render();
        // A set logged at 23:30 UTC on (MON-1) is LOCAL Monday in a +2h zone.
        let utc_ts = (MON - 1) * 86_400 + 23 * 3600 + 30 * 60; // 23:30 UTC, MON-1
        app.update(
            Event::LogSet {
                exercise: "Back Squat".into(),
                weight_kg: 120.0,
                reps: 3,
                rpe: 8.0,
                observed_at: utc_ts,
                entry_id: 0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(Event::GeneratePlan { start_epoch_day: MON }, &mut model)
            .expect_only_render();
        // Today = Wed local; +2h offset → the 23:30 UTC set is LOCAL Monday.
        app.update(
            Event::SetToday {
                epoch_day: MON + 2,
                utc_offset_sec: 2 * 3600,
            },
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        let mon = vm
            .week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .expect("Monday present");
        assert_eq!(
            mon.status, "done",
            "a set at 23:30 UTC MON-1 counts as done on LOCAL Monday (+2h)"
        );
    }

    // ── The program week advances with SetToday; a mid-week plan does not
    //        back-date the week's earlier days as missed ──
    #[test]
    fn b6_week_advances_and_pre_start_days_are_not_missed() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(strength_profile()), &mut model)
            .expect_only_render();
        // Plan starts Wednesday (MON+2), mid-week.
        app.update(Event::GeneratePlan { start_epoch_day: MON + 2 }, &mut model)
            .expect_only_render();
        app.update(
            Event::SetToday { epoch_day: MON + 2, utc_offset_sec: 0 },
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        // Monday (before the plan started) must NOT read "missed".
        if let Some(mon) = vm.week_plan.iter().find(|s| s.epoch_day == MON) {
            assert_ne!(
                mon.status, "missed",
                "a day before the plan start is not a missed session"
            );
        }
        assert_eq!(vm.program.as_ref().unwrap().week, 1, "week 1 at the start");

        // Advance the clock 2 weeks → the summary reports a later program week.
        app.update(
            Event::SetToday { epoch_day: MON + 2 + 14, utc_offset_sec: 0 },
            &mut model,
        )
        .expect_only_render();
        let vm2 = app.view(&model);
        assert!(
            vm2.program.as_ref().unwrap().week >= 2,
            "the program week must advance with the clock: {}",
            vm2.program.as_ref().unwrap().week
        );
    }

    // ── Overflow-safe epoch math + non-finite/huge float clamp on ingest ──
    #[test]
    fn b7_extreme_wire_values_do_not_panic_or_poison_the_view() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(strength_profile()), &mut model)
            .expect_only_render();
        // A huge finite weight must be clamped so e1RM math can't reach inf.
        app.update(
            Event::LogSet {
                exercise: "Back Squat".into(),
                weight_kg: 1.0e300,
                reps: 3,
                rpe: 8.0,
                observed_at: MON * 86_400,
                entry_id: 0,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(
            model.sets[0].weight_kg.is_finite() && model.sets[0].weight_kg <= 1.0e12,
            "a huge wire weight must be clamped finite"
        );
        // A corrupt near-i64::MAX SetToday must not panic view() (weekday math).
        app.update(
            Event::GeneratePlan { start_epoch_day: MON },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::SetToday { epoch_day: i64::MAX, utc_offset_sec: 0 },
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model); // must not panic
        // A finite serializable view is produced.
        assert!(serde_json::to_string(&vm).is_ok());
        // mon0_weekday itself is overflow-safe at the extremes.
        let _ = mon0_weekday(i64::MAX);
        let _ = mon0_weekday(i64::MIN);
    }

    // ── Amending a legacy (id 0) row whose DATE changed does not duplicate ──
    #[test]
    fn b8_amend_legacy_date_change_replaces_not_duplicates() {
        let app = Engine;
        let mut model = Model::default();
        let old_ts = MON * 86_400;
        let new_ts = (MON + 1) * 86_400;
        // Legacy log (entry_id 0).
        app.update(
            Event::LogSet {
                exercise: "Bench".into(),
                weight_kg: 100.0,
                reps: 5,
                rpe: 8.0,
                observed_at: old_ts,
                entry_id: 0,
            },
            &mut model,
        )
        .expect_only_render();
        assert_eq!(model.sets.len(), 1);
        // Amend it, CHANGING the date: must match the OLD row via fallback.
        app.update(
            Event::AmendSet {
                entry_id: 0,
                exercise: "Bench".into(),
                weight_kg: 110.0,
                reps: 5,
                rpe: 8.0,
                observed_at: new_ts,
                observed_at_fallback: old_ts,
            },
            &mut model,
        )
        .expect_only_render();
        assert_eq!(
            model.sets.len(),
            1,
            "a re-dated legacy amend must replace, not duplicate"
        );
        assert_eq!(model.sets[0].weight_kg, 110.0);
        assert_eq!(model.sets[0].observed_at, new_ts);
    }

    // ── Wire contract: HR zones consume the profile's measured HRmax ──
    #[test]
    fn measured_hr_max_supersedes_the_tanaka_estimate() {
        let q = HrZoneQuery {
            age_years: 30.0,
            resting_hr_bpm: None,
            weeks_since_recalc: None,
            weeks_since_pace_test: None,
        };
        // Tanaka(30) = 187; a measured 200 must drive the table instead.
        let (rows, hr_max) = build_hr_zones(&q, Some(200.0));
        let hrmax_row = rows
            .iter()
            .find(|r| r.summary.contains("HRmax"))
            .expect("an HRmax row is present");
        assert!(
            hrmax_row.summary.contains("200") && hrmax_row.summary.to_lowercase().contains("measured"),
            "the measured max must be shown: {}",
            hrmax_row.summary
        );
        // #6: the structured figure mirrors the prose, measured, no Tanaka split.
        let m = hr_max.expect("structured hr_max present");
        assert!(m.measured && m.bpm == 200.0 && m.tanaka_intercept == 0.0);
        // Implausible measured values fall back to the age estimate.
        let (rows, hr_max) = build_hr_zones(&q, Some(9999.0));
        let hrmax_row = rows.iter().find(|r| r.summary.contains("HRmax")).unwrap();
        assert!(
            hrmax_row.summary.contains("187"),
            "an implausible measured max falls back to Tanaka: {}",
            hrmax_row.summary
        );
        // #6: fell back to Tanaka → estimate carries the 208 − 0.7 × age split.
        let e = hr_max.expect("structured hr_max present");
        assert!(!e.measured && e.bpm == 187.0 && e.tanaka_intercept == 208.0 && e.tanaka_slope == 0.7);
    }
}
