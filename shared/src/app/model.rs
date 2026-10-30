//! Core state and wire types: the persisted `Model`, its private logged
//! records, the athlete `Profile`, post-session reviews, calculator queries,
//! and the `Event` vocabulary the shells send.

use super::*;

#[derive(Clone)]
pub(super) struct LoggedSet {
    pub(super) exercise: String,
    pub(super) weight_kg: f64,
    pub(super) reps: u32,
    pub(super) rpe: f64,
    /// Log time, unix seconds; 0 when undated.
    pub(super) observed_at: i64,
    /// Stable per-entry identity: a shell-assigned monotonic id
    /// (epoch-millis at first log) that survives edits so `AmendSet`/`DeleteEntry`
    /// target THIS row even when its `observed_at` was backdated to collide with
    /// another. 0 = no id; such a row is matched on `observed_at`.
    pub(super) entry_id: u64,
    /// Timed-hold duration, seconds — `Some` for a static/hold set (plank,
    /// dead hang, lever), `None` for a reps×load set. History renders a hold row
    /// as "<name> — <s> s hold".
    pub(super) hold_sec: Option<u16>,
}

impl LoggedSet {
    /// The one log order for sets, never their storage slot (an Undo or amend
    /// re-pushes a row at the tail): log time, then the stable `entry_id`,
    /// then the set's content, so two same-second sets (an import, a double
    /// tap, a legacy `entry_id == 0` pair) still order the same way every time.
    pub(super) fn chrono_cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.observed_at
            .cmp(&other.observed_at)
            .then(self.entry_id.cmp(&other.entry_id))
            .then_with(|| self.exercise.cmp(&other.exercise))
            .then(self.weight_kg.total_cmp(&other.weight_kg))
            .then(self.reps.cmp(&other.reps))
            .then(self.rpe.total_cmp(&other.rpe))
            .then(self.hold_sec.cmp(&other.hold_sec))
    }
}

#[derive(Clone)]
pub(super) struct LoggedRun {
    /// Manual distance; ignored when `track` is non-empty (derived instead).
    pub(super) distance_km: f64,
    /// Manual duration; ignored when `track` is non-empty (derived instead).
    pub(super) duration_min: f64,
    /// Average %HRmax: the wire figure, or for a typed bpm its conversion
    /// against the HRmax of the profile in force at the run's instant
    /// (`ingest::derived_hr_pct_max`), re-derived whenever the profile history
    /// changes so it never depends on log order (unless `hr_pinned`).
    pub(super) hr_pct_max: f64,
    /// The wire `hr_pct_max` (sanitized), which stands when no bpm conversion
    /// applies.
    pub(super) wire_hr_pct_max: f64,
    /// Card spike baseline: the longest OTHER run in this run's trailing
    /// 30-day window, floored by the caller-supplied paired-history value,
    /// re-derived whenever a run in that window changes
    /// (`ingest::refresh_spike_baselines`). Drives the run CARD's descriptive
    /// spike chip only; the spike SAFETY GATE derives its baseline at view()
    /// time from `model.runs` (see `latest_run_spike`).
    pub(super) longest_recent_km: f64,
    /// The caller-supplied paired-history spike baseline (sanitized, >= 0).
    pub(super) wire_longest_recent_km: f64,
    /// GPS fixes for a tracked run; empty for a hand-entered run.
    pub(super) track: Vec<GpsPoint>,
    /// Indices into `track` that BEGIN a new recording segment: the first
    /// fix captured after a pause + possible relocation. Every track metric skips
    /// the pause-bridge leg entering such an index (no distance, no time), and the
    /// GPX export breaks a `<trkseg>` there. Empty = one continuous segment, the
    /// legacy (re-anchored) and hand-logged path. Model-internal, not wire-decoded.
    pub(super) track_segment_starts: Vec<u32>,
    /// Log time, unix seconds; 0 when undated.
    pub(super) observed_at: i64,
    /// Stable per-entry identity; see [`LoggedSet::entry_id`]. 0 =
    /// legacy/absent → matched on `observed_at` instead.
    pub(super) entry_id: u64,
    /// The device's UTC offset when the row was logged (the wire
    /// `utc_offset_sec` the shell stamps; an amend keeps the original row's).
    /// The %HRmax conversion reads the run's local day with it, so a later
    /// time-zone change never re-converts the run. `None` (a legacy row) =
    /// the clock's current offset.
    pub(super) logged_utc_offset_sec: Option<i64>,
    /// A legacy undated `SetProfile` arrived after this row was logged: its
    /// `hr_pct_max` stays the figure it had then (the edit applies from its
    /// position in the log, not to runs already logged). Cleared by
    /// `ClearProfile`.
    pub(super) hr_pinned: bool,
    /// User-declared run-intent label. USER DATA, carries no evidence of its
    /// own: history display, and the weekly hard-session checks honor it as
    /// the session's declared goal (running-012/023). `None` = untagged; never
    /// fabricated. (Model-internal struct, not wire-decoded.)
    pub(super) workout_type: Option<WorkoutType>,
    /// Whole-session effort on the CR-10 scale (0–10), rated ~30 min after the
    /// run (LOAD-SRPE-001). `None` = unrated; never prefilled or invented.
    /// Drives the separate session-RPE load series (minutes × rating).
    pub(super) session_rpe: Option<u8>,
    /// The average heart rate the athlete entered, bpm, when plausible
    /// (`run_targets::plausible_avg_hr_bpm`). Kept beside the derived
    /// `hr_pct_max` so the run card shows the figure the user typed rather than
    /// a re-derived percentage. `None` = not entered.
    pub(super) avg_hr_bpm: Option<f64>,
}

/// One logged "other activity" (sport, cycling, a PE lesson…): the minutes the
/// under-18 WHO activity card counts (YOUTH-PA-001). Model-internal, built
/// from `Event::LogActivity`.
#[derive(Clone)]
pub(super) struct LoggedActivity {
    pub(super) kind: ActivityKind,
    /// Finite, in (0, `ACTIVITY_MAX_MINUTES`].
    pub(super) minutes: f64,
    pub(super) vigorous: bool,
    pub(super) strengthening: bool,
    /// Log time, unix seconds; 0 when undated.
    pub(super) observed_at: i64,
    /// Stable per-entry identity; see [`LoggedSet::entry_id`]. 0 =
    /// legacy/absent → matched on `observed_at` instead.
    pub(super) entry_id: u64,
}

/// A "the user ended this exercise early" marker. Keyed by
/// `same_movement`-normalized `exercise` + local `epoch_day`; last-write-wins.
/// `ended` true makes the plan item done regardless of set count, `false`
/// reopens it. Model-internal (built from `Event::SetExerciseEnded`).
#[derive(Clone)]
pub(super) struct EndedMarker {
    pub(super) exercise: String,
    pub(super) epoch_day: i64,
    pub(super) ended: bool,
    pub(super) observed_at: i64,
}

/// A calisthenics rung placement (`Event::SetCalisthenicsRung`): the athlete
/// chose this rung for a ladder — at setup (their starting rung) or via the
/// plan item's Easier / Harder choice. Replayed with the logged sets, in time
/// order, by `build_calisthenics_anchors`. Model-internal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CalisPlacement {
    pub(super) ladder: crate::schema::LadderId,
    pub(super) rung: u8,
    pub(super) observed_at: i64,
}

/// A readiness input removed by a dated `RemoveReadiness`. Today's state and
/// every future day ignore it exactly as if deleted; a past day whose local
/// date precedes the removal's local date was shown with it in force, so the
/// per-day recompute keeps it for that day. Model-internal.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RetractedInput {
    pub(super) input: ReadinessInput,
    /// Unix seconds of the removal.
    pub(super) retracted_at: i64,
}

/// How the athlete trains resistance: external loads (barbell/dumbbell) or
/// bodyweight leverage (calisthenics). A calisthenics user's plan is built from
/// leverage ladders instead of %1RM; they may still log arbitrary barbell sets
/// (those stay plain logged lifts). Default `Weights`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResistanceModality {
    #[default]
    Weights,
    Calisthenics,
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
    /// Running km per week (hybrid lower-lift cap trigger). Always 0 when
    /// `running_km_per_week_set` is false.
    pub running_km_per_week: f64,
    /// Whether the athlete actually answered the weekly-distance question.
    /// `false` = "not set": the core zeroes `running_km_per_week` and the plan
    /// sizes from logged runs, else the KB session-duration floors, never from
    /// an invented figure. Missing on the wire = `true`.
    #[serde(default = "serde_true")]
    pub running_km_per_week_set: bool,
    /// Advanced marathoner → extended session range.
    pub advanced: bool,
    /// Female user: routes the feedback-035 menstrual/nutrition clinician
    /// prompt on a bone-stress referral and the hybrid-024 energy-availability
    /// risk cohort. `None` reads as not-female.
    pub female: Option<bool>,
    /// Currently inside a high-load/overload mesocycle: arms the autoreg-029
    /// parasympathetic-saturation guard (HRV above the SWC upper band then
    /// blocks auto load-adds).
    #[serde(default)]
    pub high_load_block: bool,
    /// Stage-0 onboarding health screen (File 08 onboard-050). Every flag
    /// defaults to `false` and gates nothing.
    #[serde(default)]
    pub health: HealthScreen,
    /// Training-environment context (File 08 indiv-025 / safety-024): heat,
    /// altitude, or cold arms the environment-modifier guidance. `None` states
    /// nothing and gates nothing.
    pub environment: Option<Environment>,
    /// Typical session temperature, °C, running-041 pace-correction trigger
    /// (heat correction above ~15 °C). `None` = unstated, no row.
    pub env_temp_c: Option<f64>,
    /// Typical session altitude, m, running-041 pace-correction trigger
    /// (altitude correction above ~900 m). `None` = unstated, no row.
    pub env_altitude_m: Option<f64>,
    /// Weeks fully off training when returning from a layoff: arms the
    /// REENTRY-001 resistance re-entry ramp and the hypertrophy-045 reduced
    /// re-entry MEV note. `None` = not returning.
    pub weeks_off: Option<f64>,
    /// Bodyweight, kg (optional): arms the strength-plyo depth-jump
    /// readiness gate (needs squat 1RM ≥ 1.5× bodyweight). `None` = no gate.
    /// Also the consolidated person-data source the protein calculator prefills
    /// from: person data is entered once, on the profile.
    pub bodyweight_kg: Option<f64>,
    /// Age in years (optional). Read by the under-18 gate (`ingest`), the
    /// Tanaka HRmax behind run targets and logged-run %HRmax
    /// (`run_targets::hr_max_anchor`) when no measured max is set, the HR-zone
    /// calculator's age input, and the conservative deload cadence
    /// (`plan::conservative_deload`). `None` = unstated: no Tanaka estimate,
    /// adult rules. With a `birth_epoch_day` the core owns this value: it is
    /// re-derived from the birthday and the model's today on every
    /// `SetProfile` and `SetToday`; a profile with only an age keeps the
    /// stated value.
    pub age_years: Option<f64>,
    /// Birthday, local epoch day (optional). Source of the derived
    /// `age_years` and the under-18 gate. `None` = only a stated age, or no
    /// age at all.
    pub birth_epoch_day: Option<i64>,
    /// Resting HR, bpm (optional). Sets the Karvonen ceiling of run heart-rate
    /// targets (`run_targets::hr_rest_anchor`) and prefills the HR-zone
    /// calculator's Karvonen input. `None` = unstated.
    pub resting_hr_bpm: Option<f64>,
    /// Measured maximum HR, bpm (optional). When plausible it replaces the
    /// Tanaka age estimate in the HR-zone calculator, run targets and
    /// logged-run %HRmax, and dates the HR-zone re-test reminder. `None` =
    /// unstated.
    pub measured_hr_max: Option<f64>,
    /// Display weights in pounds instead of kilograms. DISPLAY-ONLY: every
    /// NUMERIC wire/view field (`weight_kg`, `e1rm_kg`, `load_kg`, …) stays in
    /// kg regardless; only the human-facing weight strings the shell renders
    /// verbatim convert (kg × `LB_PER_KG`, "lb" suffix, via the `weight_num` /
    /// `weight_suffix` / `fmt_weight` helpers).
    #[serde(default)]
    pub weight_lb: bool,
    /// Resistance-training modality: `Weights` (default) or `Calisthenics`.
    /// A calisthenics profile routes lift days through the leverage-ladder
    /// synthesizer (`plan::calisthenics_session`).
    #[serde(default)]
    pub resistance_modality: ResistanceModality,
    /// The local epoch-day this profile edit takes effect.
    /// The core keeps a `profile_history`; a past/today plan day is prescribed
    /// with the latest profile whose `effective_from_epoch_day <= that day`, so
    /// editing a goal or weekly-set target never re-prescribes an already-shown
    /// past day. `None` = effective from the beginning of time (−∞): a
    /// None-dated edit replaces the whole history, but applies only from its
    /// position in the log to logged runs (it pins the %HRmax of the runs
    /// already logged, `LoggedRun::hr_pinned`). Production shells send the
    /// core's today.
    pub effective_from_epoch_day: Option<i64>,
    /// When the edit was made, unix seconds (the shell's clock). On its
    /// effective day the edit is in force only from this instant, so an
    /// evening edit leaves a run logged that morning on the profile it was
    /// logged under ([`profile_at`]); it also orders same-day edits in the
    /// history. `None` = from the start of that day.
    /// Ignored on an undated edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<i64>,
    /// Lifting sessions per week the athlete commits to, 2..=5. `None` = the
    /// fixed default lift layout.
    pub lift_days_per_week: Option<u8>,
    /// Days the athlete can train, as a bitmask: bit0 = Monday … bit6 =
    /// Sunday. `None` = the engine's default weekday layout.
    pub training_weekdays: Option<u8>,
    /// Target race date, local epoch day. `None` = no race scheduled.
    pub goal_race_epoch_day: Option<i64>,
    /// A recent race result the athlete reports (distance, finish time,
    /// optional date). `None` = none stated.
    pub recent_race: Option<RecentRace>,
    /// Athlete-stated longest run in the last 30 days, km. Logged runs
    /// supersede it once present. `None` = unstated.
    pub stated_longest_recent_km: Option<f64>,
    /// Local epoch day training resumed after the `weeks_off` layoff. `None`
    /// = unstated (re-entry is then dated from the profile edit).
    pub layoff_returned_epoch_day: Option<i64>,
    /// Strength test / meet date, local epoch day. `None` = no meet scheduled.
    pub meet_epoch_day: Option<i64>,
    /// Calisthenics equipment on hand. `None` = assume every piece is available.
    pub calis_equipment: Option<CalisEquipment>,
    /// Hypertrophy priority muscle key the athlete wants to specialize.
    /// `None` = no specialization.
    pub specialization_muscle: Option<String>,
    /// A pure runner opted into supplemental strength sessions.
    #[serde(default)]
    pub runner_strength: bool,
}

impl Profile {
    /// The declared layoff's return day: `None` without a layoff (`weeks_off`
    /// unset or 0), `Some(None)` when it cannot be dated, else the local epoch
    /// day training resumed (`layoff_returned_epoch_day`, else the day this
    /// profile took effect).
    pub fn declared_layoff_return(&self) -> Option<Option<i64>> {
        self.weeks_off.is_some_and(|w| w > 0.0).then_some(
            self.layoff_returned_epoch_day
                .or(self.effective_from_epoch_day),
        )
    }
}

/// Serde default for flags that were implicitly on before they existed.
fn serde_true() -> bool {
    true
}

/// A race result the athlete reports on their profile.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RecentRace {
    /// Race distance, km.
    pub distance_km: f64,
    /// Finish time, seconds.
    pub time_sec: u32,
    /// Race date, local epoch day. `None` = undated.
    pub epoch_day: Option<i64>,
}

/// Calisthenics equipment the athlete has access to.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CalisEquipment {
    pub pull_up_bar: bool,
    pub dip_bars: bool,
    pub rings: bool,
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

/// The four overtraining signs the weekly check-in asks about (feedback-036:
/// performance decrement, mood disturbance, elevated resting HR / suppressed
/// HRV, sleep deterioration). When a check-in carries this, the signal count
/// IS the number of signs ticked (it overrides the bare
/// `overtraining_signal_count`), so "a sign" always means one of these four.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(default)]
pub struct OvertrainingSigns {
    pub worse_performance: bool,
    pub low_mood: bool,
    pub higher_resting_hr: bool,
    pub worse_sleep: bool,
}

impl OvertrainingSigns {
    /// How many of the four signs are ticked.
    pub fn count(&self) -> u8 {
        [
            self.worse_performance,
            self.low_mood,
            self.higher_resting_hr,
            self.worse_sleep,
        ]
        .iter()
        .filter(|b| **b)
        .count() as u8
    }
}

/// One review payload, used by two events with separate lifecycles:
/// `SubmitReview` (one session's execution: lift/run context, off day) and
/// `SubmitWeeklyCheckin` (the week-level counts and the clinical screens).
/// The core splits every payload into its session part and its weekly part
/// (`reviews::session_part` / `reviews::weekly_part`), so a routine session
/// review can never overwrite the weekly check-in's referral or deloads.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct SessionReview {
    pub bone_pain_red_flag: bool,
    pub compulsive_flag: bool,
    /// Weekly RED-S screen positive (File 13 ready-upd-005, REDS-SCREEN-001):
    /// missed or longer cycles over the last 3 months (not explained by
    /// pregnancy or contraception), or noticeably lower sex drive / fewer
    /// morning erections for several weeks. Routes to the RED-S referral
    /// feedback and the deficit refusal, never a programming variable.
    #[serde(default)]
    pub reds_signal_reported: bool,
    pub overtraining_signal_count: u8,
    /// How long the overtraining/NFOR signals have co-occurred, in weeks
    /// (feedback-036 duration condition: fire only over ≥1–2 wks, never on a
    /// single noisy night). `None` = untracked → the count alone gates.
    pub overtraining_signal_weeks: Option<f64>,
    /// Which of the four listed overtraining signs were ticked (weekly
    /// check-in). `Some` → the effective signal count is the ticked count.
    pub overtraining_signs: Option<OvertrainingSigns>,
    /// Single-session distance over the prior-30-day longest, as a fraction.
    pub single_session_spike_frac: Option<f64>,
    pub lift: Option<LiftExec>,
    /// Interval/threshold execution for the feedback-015 mastery arm.
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
    pub decline_weeks: Option<u8>,
    /// Wellness domains currently suppressed alongside the decline -
    /// fatigue/mood/sleep/etc. (autoreg-042 needs ≥2).
    pub suppressed_wellness_domains: Option<u8>,
    /// The decline has persisted DESPITE a deload (safety-042 escalation).
    #[serde(default)]
    pub despite_deload: bool,
    /// Pace at target HR improved ≥ the smallest-worthwhile change, and for how
    /// many weeks it has held (autoreg-032; ≥2 → re-test threshold pace).
    pub pace_at_hr_improved_weeks: Option<u8>,
    /// Count of hypertrophy overreaching triggers present this week -
    /// performance decrement, RIR drift to 0, persistent joint/tendon aches,
    /// disrupted sleep, elevated RHR, mood drop (hypertrophy-035; ≥2 → deload).
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
    pub rep_drop_frac: Option<f64>,
    /// Today's reference-load mean concentric velocity minus baseline, m/s
    /// (autoreg-008/009 VBT daily readiness; ±0.06 m/s reliability band).
    pub mcv_delta_m_s: Option<f64>,
    /// First work set hit its target reps (autoreg-011/012 set-volume gate).
    pub first_set_reps_met: Option<bool>,
    /// First work set RPE minus target RPE (autoreg-011/012).
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
    pub wellness_z_low_days: Option<u8>,
    /// Interval reps that landed at RPE ≥ target+1 or above the HR cap
    /// (autoreg-031; ≥2 → cut remaining-rep pace ~2–4%).
    pub interval_reps_over_target: Option<u8>,
    /// Whether the prescribed easy pace was holdable under the HR cap
    /// (autoreg-033; `Some(false)` → slow the easy pace).
    pub can_hold_easy_pace_under_hr_cap: Option<bool>,
    /// HRV readings flagged unreliable among the last three (autoreg-050;
    /// ≥2 → suspend HRV gating, fall back to subjective + performance).
    pub hrv_unreliable_last_three: Option<u8>,
    /// Consecutive days lnRMSSD has sat below the SWC band (autoreg-034;
    /// ≥3 → insert a recovery day / easy block).
    pub hrv_suppressed_days: Option<u8>,
    /// Days subjective wellness has been suppressed (autoreg-035).
    pub wellness_suppressed_days: Option<u8>,
    /// Morning resting HR is trending up (autoreg-035; with ≥2 suppressed
    /// wellness days → 1–3 easy days or cross-training).
    #[serde(default)]
    pub rhr_rising: bool,
    /// Single-day lnRMSSD z-score vs baseline (autoreg-028 second trigger;
    /// < −1 with a ≥2-day downtrend → downgrade hard→easy).
    pub hrv_single_day_z: Option<f64>,
    /// Days the HRV has been trending down (autoreg-028 second trigger).
    pub hrv_downtrend_days: Option<u8>,
    /// Concurrent-training interference symptoms present (hybrid-018/CAP-6;
    /// with running not mandatory → substitute cycling/rowing for part of
    /// the aerobic volume).
    #[serde(default)]
    pub interference_symptoms: bool,
    /// Consecutive failed sessions on one novice linear-progression lift
    /// (Starting Strength stall governance; ≥3 with adequate recovery →
    /// deload that lift ~10% and re-ramp).
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
    pub planned_hard: Option<bool>,
    /// Rolling multi-week trend direction of the athlete's key metric:
    /// `"up"` / `"flat"` / `"down"` (feedback-027/028/029; any other value
    /// reads as flat). `None` = no trend submitted.
    pub trend_direction: Option<crate::feedback::TrendDirection>,
    /// Weeks the rolling trend has been flat (feedback-028; ≥4 → plateau
    /// reframing).
    pub weeks_flat: Option<u8>,
    /// Target missed on a genuine high-RPE off day.
    pub bad_day: bool,
    /// When the review was submitted, unix seconds; shell-supplied (the core
    /// holds no clock), 0 = undated. Consumed by review expiry: week-scoped
    /// deloads age out after `7 × weeks` days and a session-execution note after
    /// a week (safety rows never expire); an undated review never expires.
    #[serde(default)]
    pub observed_at: i64,
}

#[derive(Default)]
pub struct Model {
    /// Active readiness signals, in submission order. Day-scoped: cleared by
    /// `ClearReadiness` (the raw-signal / advanced path + red-flag reports).
    /// Every current-state consumer reads only these.
    pub(super) inputs: Vec<ReadinessInput>,
    /// Inputs removed by a dated `RemoveReadiness`, in removal order. Read only
    /// by the per-day recompute of a past day (see [`RetractedInput`]); wiped
    /// with `inputs` by `ClearReadiness`.
    pub(super) retracted_inputs: Vec<RetractedInput>,
    /// Morning check-ins: a RETAINED multi-day history of raw
    /// human observations. NOT cleared by `ClearReadiness`; the whole point is
    /// a rolling baseline the core normalizes into z-scores/deltas/streaks.
    pub(super) checkins: Vec<CheckinInput>,
    /// `RestartHrvBaseline`: HRV readings logged before this unix second are
    /// ignored by the derivation (a new device / timing). Reset by `ClearCheckins`.
    pub(super) hrv_baseline_from: Option<i64>,
    /// Logged lift sets, in submission order.
    pub(super) sets: Vec<LoggedSet>,
    /// Logged runs, in submission order.
    pub(super) runs: Vec<LoggedRun>,
    /// Logged other activities (sport, cycling…), in submission order.
    /// Cleared by `ClearActivities`.
    pub(super) activities: Vec<LoggedActivity>,
    /// Every applied profile in effect-order; the last is the current one
    /// ([`Model::profile`]). A `SetProfile` with effective-day E drops every
    /// entry with eff >= E (None = −∞) then appends, so a past plan day reads
    /// the profile effective THAT day and a later goal/weekly-set edit never
    /// re-prescribes it. `ClearProfile` clears it.
    pub(super) profile_history: Vec<Profile>,
    /// User-declared "I ended this exercise early" markers, keyed by
    /// `same_movement`-normalized exercise + local epoch-day, last-write-wins.
    /// An ended marker makes the plan item done regardless of set count;
    /// `ended: false` reopens it. Cleared by `ClearSets`.
    pub(super) ended_exercises: Vec<EndedMarker>,
    /// The current per-session review (execution context only), if submitted.
    /// `ClearReview` empties it. Never carries weekly/clinical fields.
    pub(super) review: Option<SessionReview>,
    /// The weekly check-in in force (week-level counts + clinical screens):
    /// the latest `SubmitWeeklyCheckin`, or the weekly part of a legacy
    /// `SubmitReview`. A red flag here stays until a LATER weekly check-in
    /// answers "no" (or `ClearWeeklyCheckin`); session reviews never touch it.
    pub(super) weekly_checkin: Option<SessionReview>,
    /// Every weekly check-in ever submitted, in submission order.
    /// `ClearWeeklyCheckin` empties only `weekly_checkin`: a past day keeps the
    /// deload / downgrade windows the check-ins gave it, so a later check-in or
    /// a clear never re-judges it.
    pub(super) weekly_history: Vec<SessionReview>,
    /// The last race-time prediction query, if requested. Stored as the raw
    /// inputs so the two-method estimate is re-derived in `view` (like a logged
    /// run), keeping the core deterministic and the prediction reproducible.
    pub(super) race_query: Option<RaceQuery>,
    /// The last hypertrophy-mesocycle volume-plan query, if requested. Stored as
    /// the raw inputs so the graded per-week plan is re-derived in `view`.
    pub(super) hypertrophy_plan_query: Option<HypertrophyInputView>,
    /// The last absolute-protein-target query, if requested. Stored as the raw
    /// inputs so the graded g/day range is re-derived in `view` (bodyweight ×
    /// each graded g/kg bound).
    pub(super) protein_query: Option<ProteinInputView>,
    /// The last heart-rate-zone query, if requested. Stored as the raw age so the
    /// graded HRmax + %HRmax band table is re-derived in `view`.
    pub(super) hr_zone_query: Option<HrZoneInputView>,
    /// The accepted "Plan my training" request. Present once the
    /// user accepts a generated plan; the `Program` itself is re-derived in
    /// `view()` from profile + anchors + this request (deterministic, like every
    /// calculator). `None` → no plan surfaced.
    pub(super) plan_request: Option<PlanRequest>,
    /// Earlier plans as (start day, local day `ClearPlan` removed them), in
    /// removal order, so their planned deload weeks stay known after the plan
    /// is removed and set up again (`consistency::lift_deload_days`).
    pub(super) past_plans: Vec<(i64, i64)>,
    /// The shell's clock, entering as event data: the current
    /// epoch-day, sent on foreground so `view()` can date the week and pick the
    /// next session without reading a clock. `None`
    /// → the plan anchors to the request's `start_epoch_day`.
    pub(super) today_epoch_day: Option<i64>,
    /// The shell's UTC offset (seconds east of UTC) that accompanied the last
    /// `SetToday`, used to bucket UTC `observed_at` timestamps into the shell's
    /// LOCAL calendar day for session-done matching. 0 = UTC (default).
    pub(super) today_utc_offset_sec: i64,
    /// Calisthenics rung placements, in submission order (SETS family:
    /// `ClearSets` clears them with the sets they steer).
    pub(super) calis_placements: Vec<CalisPlacement>,
    /// Session moves within a plan week ("do this on another day",
    /// ADH-RESCHED-001), in submission order. Validated at `view()` time against
    /// the week's spacing rules; a move that breaks them is ignored and
    /// reported. Plan family: `ClearPlan` clears them.
    pub(super) session_moves: Vec<SessionMove>,
}

impl Model {
    /// The athlete's current programming profile: the latest applied.
    pub(super) fn profile(&self) -> Option<&Profile> {
        self.profile_history.last()
    }
}

/// Inputs for a synthesized training plan: the epoch-day the user
/// accepted the plan. Retained so the `Program` is re-derived in `view()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PlanRequest {
    pub(super) start_epoch_day: i64,
}

/// One accepted `MoveSession` request: the planned session dated
/// `from_epoch_day` is done on `to_epoch_day` instead (same plan week).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SessionMove {
    pub(super) from_epoch_day: i64,
    pub(super) to_epoch_day: i64,
}

/// Inputs for a Daniels+Riegel race-time prediction (running-039), retained so
/// the view can recompute the graded estimate. Distances in metres, time in
/// seconds, `weekly_km` selects the Riegel fatigue exponent.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct RaceQuery {
    pub(super) recent_distance_m: f64,
    pub(super) recent_time_sec: f64,
    pub(super) goal_distance_m: f64,
    pub(super) weekly_km: f64,
    /// How many weeks old the input race is (running-041 freshness window:
    /// ≤6 wk fresh, 7–8 marginal, >8 stale → re-test). `None` = untracked.
    pub(super) weeks_since_race: Option<u8>,
}

/// Which logged history family a [`Event::DeleteEntry`] targets. Selects the
/// model vec (`sets`, `runs` or `activities`) and the compaction family
/// (1, 2 or 15).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Set,
    Run,
    Activity,
}

/// What kind of other activity a [`Event::LogActivity`] records (wire:
/// the variant name).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivityKind {
    /// Sport, games, PE.
    Sport,
    Cycling,
    Swimming,
    Walking,
    Dance,
    /// Muscle- or bone-strengthening: bodyweight work, climbing, jumping,
    /// gymnastics.
    Strength,
    #[default]
    Other,
}

/// Longest single activity `LogActivity` accepts, minutes (10 h).
pub(crate) const ACTIVITY_MAX_MINUTES: f64 = 600.0;

/// `update`'s gate for `LogActivity`: finite minutes in (0, 600]. The single
/// definition read by `update` and by compaction (an invalid line never
/// becomes a row, so it carries no entry identity).
pub(crate) fn activity_passes_gate(minutes: f64) -> bool {
    minutes.is_finite() && minutes > 0.0 && minutes <= ACTIVITY_MAX_MINUTES
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Event {
    /// Record one readiness observation, then recompute adjustments.
    SubmitReadiness(ReadinessInput),
    /// Drop all accumulated inputs (new day / new session). Undated, so it
    /// cannot tell which days the dropped inputs shaped: it wipes the active and
    /// the retracted inputs alike, and a past day is re-judged without them.
    ClearReadiness,
    /// Undo for one report: remove the most recent input carrying `signal`
    /// (for Pain, the worst active report), leaving unrelated inputs intact.
    /// No-op when no such input exists. A dated remove (`observed_at > 0`,
    /// unix seconds) retracts the input: gone for today and the future, still
    /// in force for any past day before the removal's local day. An undated
    /// remove deletes it outright.
    RemoveReadiness {
        signal: ReadinessSignal,
        #[serde(default)]
        observed_at: i64,
    },
    /// "I'm better" / "I'm pain-free now": every ACTIVE
    /// report of `signal` (Pain or Illness) is retracted as resolved at
    /// `observed_at`. History is kept (each moves to `retracted_inputs`, still
    /// in force for the past days it shaped) and the coach restarts with a
    /// graded return (RETURN-HOLD-001 / ILLNESS-RTT-001). Unlike
    /// `RemoveReadiness` it clears every report of that signal (several daily
    /// "still ill" answers belong to one episode) and never touches other
    /// signals. `observed_at <= 0` is a no-op: a resolution must be dated.
    ResolveHold {
        signal: ReadinessSignal,
        observed_at: i64,
    },
    /// Record one morning check-in (raw human observations). Appended to the
    /// RETAINED check-in history the core normalizes into z-scores/deltas;
    /// unlike `SubmitReadiness`, this is not day-cleared.
    SubmitCheckin(CheckinInput),
    /// Drop the entire check-in history (part of "Clear all data"). Not tied to
    /// `ClearReadiness`, the two stores have different lifecycles.
    ClearCheckins,
    /// "Changed device? Restart HRV baseline" (HRV-SOURCE-001: one
    /// HRV source and timing, never mixed): every HRV reading logged before
    /// `observed_at` is ignored from now on, so the baseline rebuilds from the
    /// new source. Only HRV is affected; the other check-in items stay. The
    /// latest restart wins; `ClearCheckins` also resets it.
    RestartHrvBaseline { observed_at: i64 },
    /// Log one completed lift set (weight in kg, reps, session RPE).
    LogSet {
        exercise: String,
        weight_kg: f64,
        reps: u32,
        rpe: f64,
        /// When the set was logged, unix seconds; shell-supplied (the core holds
        /// no clock). 0 = undated.
        #[serde(default)]
        observed_at: i64,
        /// Stable per-entry id: a shell-assigned monotonic value
        /// (epoch-millis at log) so `AmendSet`/`DeleteEntry` can target this exact
        /// set. 0 = no id (matched on `observed_at` instead).
        #[serde(default)]
        entry_id: u64,
        /// Timed-hold duration, seconds — `Some` for a static/hold set (a
        /// calisthenics lever/plank/dead-hang rung), `None` for a reps×load set.
        hold_sec: Option<u16>,
    },
    /// Drop all logged sets.
    ClearSets,
    /// Mark a plan exercise ended early for a session day, or reopen it.
    /// Last-write-wins per (`same_movement`-normalized `exercise`, `epoch_day`);
    /// `ended: false` reopens. An ended item is done regardless of logged sets,
    /// so the shell's "End exercise" / "Reopen" action drives it. Persisted;
    /// `ClearSets` clears every ended marker.
    SetExerciseEnded {
        exercise: String,
        epoch_day: i64,
        ended: bool,
        /// When the marker was set, unix seconds (shell clock); 0 = undated.
        #[serde(default)]
        observed_at: i64,
    },
    /// Log one run (distance km, duration min, average % HRmax, and the
    /// longest run in the last 30 days for spike detection).
    LogRun {
        distance_km: f64,
        duration_min: f64,
        hr_pct_max: f64,
        longest_recent_km: f64,
        /// Log time, unix seconds; shell-supplied, 0 = undated.
        #[serde(default)]
        observed_at: i64,
        /// Stable per-entry id; see [`Event::LogSet`].
        #[serde(default)]
        entry_id: u64,
        /// User-declared run-intent label. USER DATA with no evidence of its
        /// own: the weekly report's hard-session checks honor it as the
        /// session's declared goal (running-012/023). `None` = untagged.
        workout_type: Option<WorkoutType>,
        /// Whole-session effort on the CR-10 scale (0–10), rated ~30 min after
        /// the run (LOAD-SRPE-001). `None` = unrated; values above 10 clamp to
        /// 10 at ingest.
        session_rpe: Option<u8>,
        /// Average heart rate in bpm as the user's watch shows it (UXL-12). When
        /// set and plausible, `update` converts it to `hr_pct_max` against the
        /// measured HRmax, else the Tanaka age estimate (RUN-HRMAX-001), of the
        /// profile in force at the run's instant (re-derived when the profile
        /// history changes); with neither known the sent `hr_pct_max` stands.
        avg_hr_bpm: Option<f64>,
        /// The device's UTC offset, seconds east, when the run was logged. The
        /// bpm conversion reads the run's local day with it, so a later
        /// time-zone change never re-converts the run. `None` (omitted from the
        /// wire) = the clock's current offset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        utc_offset_sec: Option<i64>,
    },
    /// Log one GPS-tracked run. Distance and duration are derived in-core from
    /// the fix track (haversine + time span); `hr_pct_max` comes from a paired
    /// HR sensor (0.0 when none). `longest_recent_km` floors the run card's
    /// spike-chip baseline; the spike gate derives its own from the logged runs.
    LogRunTrack {
        points: Vec<GpsPoint>,
        hr_pct_max: f64,
        longest_recent_km: f64,
        /// Indices into `points` that begin a new recording segment: the shell's
        /// pause/relocation boundaries. Empty = one continuous segment.
        #[serde(default)]
        segment_starts: Vec<u32>,
        /// Log time, unix seconds; shell-supplied, 0 = undated. The GPS fixes
        /// carry their own per-point `observed_at`; this is the session's
        /// logged-at stamp for history display.
        #[serde(default)]
        observed_at: i64,
        /// Stable per-entry id; see [`Event::LogSet`].
        #[serde(default)]
        entry_id: u64,
        /// User-declared run-intent label; see [`Event::LogRun`].
        workout_type: Option<WorkoutType>,
        /// Whole-session effort, CR-10 (0–10), see [`Event::LogRun`].
        session_rpe: Option<u8>,
        /// Average heart rate the athlete entered, bpm; see [`Event::LogRun`].
        avg_hr_bpm: Option<f64>,
        /// The device's UTC offset when the run was logged; see [`Event::LogRun`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        utc_offset_sec: Option<i64>,
    },
    /// Drop all logged runs.
    ClearRuns,
    /// Log one other activity (sport, cycling, a PE lesson…) of moderate or
    /// harder effort, for the under-18 WHO activity card (YOUTH-PA-001).
    /// Ignored unless `minutes` is finite and in (0, 600]. Wire:
    /// `{"LogActivity":{"kind":"Sport","minutes":45.0,"vigorous":true,"strengthening":false,"observed_at":1760000000,"entry_id":123}}`.
    LogActivity {
        kind: ActivityKind,
        minutes: f64,
        /// Hard enough to be out of breath (about 7-8 of 10).
        #[serde(default)]
        vigorous: bool,
        /// Strengthened muscle or bone (jumping, climbing, bodyweight work).
        #[serde(default)]
        strengthening: bool,
        /// Log time, unix seconds; shell-supplied, 0 = undated.
        #[serde(default)]
        observed_at: i64,
        /// Stable per-entry id; see [`Event::LogSet`].
        #[serde(default)]
        entry_id: u64,
    },
    /// Drop all logged activities (part of "Clear all data").
    ClearActivities,
    /// Delete one logged set, run or activity. Targets the newest entry
    /// whose `entry_id` matches (or, for a row with no id, whose
    /// `observed_at` matches `observed_at_fallback`). A no-op when nothing
    /// matches. Compaction cancels it against its matched log line (Rule 3).
    DeleteEntry {
        kind: EntryKind,
        entry_id: u64,
        /// Match target for a row with no entry id (`entry_id == 0`).
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
        /// The OLD `observed_at` of the row being amended. For a row with no id
        /// (`entry_id == 0`) whose date the user CHANGED, the original entry can
        /// only be located by its old timestamp; matching on the new one would
        /// find nothing and push a DUPLICATE. 0 = match on `observed_at`.
        #[serde(default)]
        observed_at_fallback: i64,
        /// Timed-hold duration, seconds (see [`Event::LogSet`]).
        hold_sec: Option<u16>,
    },
    /// Edit one hand-entered run's fields. GPS-tracked runs are delete-only for
    /// field edits (their measured track is not field-editable), so amending one
    /// replaces it with a manual run. The exceptions: [`Event::TrimRunTrack`]
    /// cuts a GPS run's head/tail without leaving the track store, and
    /// [`Event::AmendRunMeta`] edits any run's rating and tag in place. Matches
    /// like `DeleteEntry`.
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
        /// User-declared run-intent label; see [`Event::LogRun`].
        workout_type: Option<WorkoutType>,
        /// Whole-session effort, CR-10 (0–10), see [`Event::LogRun`].
        session_rpe: Option<u8>,
        /// Average heart rate in bpm, converted like [`Event::LogRun`]'s.
        avg_hr_bpm: Option<f64>,
    },
    /// Trim a GPS run's recorded head/tail: keep only the fixes whose per-point
    /// `observed_at` lies in `[keep_from_sec, keep_until_sec]` (inclusive,
    /// absolute unix seconds — decimation-agnostic). The run's identity
    /// (`entry_id`, `observed_at`) is UNCHANGED; every derived figure re-derives
    /// from the sliced track at `view()`. Matches like `DeleteEntry`/`AmendRun`.
    TrimRunTrack {
        entry_id: u64,
        /// The run's `observed_at`, so a run with no id (`entry_id == 0`) is
        /// located by timestamp; see [`Event::AmendRun`].
        #[serde(default)]
        observed_at_fallback: i64,
        keep_from_sec: i64,
        keep_until_sec: i64,
    },
    /// Rate or tag one logged run IN PLACE (any run, GPS-tracked included):
    /// sets its whole-session CR-10 effort and its workout tag, leaving the
    /// distance/time/track untouched. Both fields are REPLACED (a `None` clears
    /// that field). Matches like [`Event::TrimRunTrack`]; a no-op when nothing
    /// matches. Compaction family 2 (runs), kept like a trim line.
    AmendRunMeta {
        entry_id: u64,
        /// The run's `observed_at`, so a run with no id (`entry_id == 0`) is
        /// located by timestamp.
        #[serde(default)]
        observed_at_fallback: i64,
        /// Whole-session effort, CR-10 (0–10); values above 10 clamp to 10.
        session_rpe: Option<u8>,
        workout_type: Option<WorkoutType>,
    },
    /// Set the athlete's programming profile, then recompute guidance.
    SetProfile(Profile),
    /// Drop the profile (clears the guidance section).
    ClearProfile,
    /// Submit a post-session review, then resolve one feedback message. A
    /// legacy payload that carries weekly/clinical fields is split: its weekly
    /// part becomes the weekly check-in, its session part the session review.
    SubmitReview(SessionReview),
    /// Drop the session review (its execution feedback). The weekly check-in,
    /// its referral and its deloads are untouched.
    ClearReview,
    /// Submit the weekly check-in (week-level fatigue counts, overtraining
    /// signs and duration, clinical screens). Only the weekly part of the
    /// payload is read; it replaces the weekly check-in in force.
    SubmitWeeklyCheckin(SessionReview),
    /// Drop the weekly check-in in force (its concern card and deload rows);
    /// past days keep the windows it gave them.
    ClearWeeklyCheckin,
    /// Predict a goal-race finish time from a recent race (Daniels VDOT +
    /// Riegel, combined per running-039). Distances in metres, time in seconds;
    /// `weekly_km` selects the Riegel fatigue exponent.
    PredictRace {
        recent_distance_m: f64,
        recent_time_sec: f64,
        goal_distance_m: f64,
        weekly_km: f64,
        /// Weeks since the input race (running-041 freshness).
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
    /// Compute a heart-rate-zone table: HRmax (the profile's measured max,
    /// else Tanaka from `age_years`) and the five Daniels %HRmax training bands
    /// mapped to absolute bpm ranges. A resting HR arms the running-005
    /// Karvonen preference + band targets.
    ComputeHrZones {
        age_years: f64,
        resting_hr_bpm: Option<f64>,
    },
    /// Drop the heart-rate-zone table (clears the zone section).
    ClearHrZones,
    /// Accept a synthesized training plan: the user tapped "Plan
    /// my training". `start_epoch_day` anchors the block; the `Program` is
    /// re-derived in `view()` from profile + logged anchors (determinism).
    GeneratePlan { start_epoch_day: i64 },
    /// Drop the plan (Coach returns to no-plan state).
    ClearPlan,
    /// The shell's clock as event data: today's LOCAL epoch-day, sent
    /// on foreground so `view()` dates the week + picks the next session with no
    /// clock in-core. Last-write-wins singleton (compaction keeps one line).
    /// `utc_offset_sec` is the device's offset east of UTC (0 = UTC), which
    /// buckets every UTC `observed_at` into a local day (`crate::local_day`).
    SetToday {
        epoch_day: i64,
        #[serde(default)]
        utc_offset_sec: i64,
    },
    /// Place a calisthenics ladder at a rung (CAL-PLACE-001): the starting rung
    /// chosen in setup, or an Easier / Harder choice on a plan item. Applies
    /// from its own local day on (that day's prescription included); sets
    /// logged afterwards drive the ladder as usual. A rung index past the
    /// ladder's end is ignored. Wire:
    /// `{"SetCalisthenicsRung":{"ladder":"HorizontalPush","rung":4,"observed_at":1759700000}}`.
    SetCalisthenicsRung {
        ladder: crate::schema::LadderId,
        rung: u8,
        /// Unix seconds (shell clock); 0 = undated (applies from the start of
        /// history).
        #[serde(default)]
        observed_at: i64,
    },
    /// Move one planned session to another day of the same plan week
    /// (ADH-RESCHED-001): a missed day's session made up later, or a later
    /// session pulled forward to today. `to_epoch_day` must be a rest day;
    /// the move is refused (left out of the plan, with a reason in
    /// `SessionPlanView::move_note`) when it would put two sessions on one day,
    /// leave under ~48 h between hard sessions, or bring a hard/long run within
    /// 24 h of heavy leg work. Wire:
    /// `{"MoveSession":{"from_epoch_day":20360,"to_epoch_day":20361}}`.
    MoveSession {
        from_epoch_day: i64,
        to_epoch_day: i64,
    },
}

/// A logged history row an amend or `DeleteEntry` can target.
pub(super) trait LoggedEntry {
    fn entry_id(&self) -> u64;
    fn observed_at(&self) -> i64;
}

macro_rules! impl_entry {
    ($($t:ty),*) => {$(
        impl LoggedEntry for $t {
            fn entry_id(&self) -> u64 {
                self.entry_id
            }
            fn observed_at(&self) -> i64 {
                self.observed_at
            }
        }
    )*};
}
impl_entry!(LoggedSet, LoggedRun, LoggedActivity);

/// Index of the NEWEST row an amend/`DeleteEntry` targets: the last
/// (`rposition`) whose sanitized `entry_id` matches, or, for a legacy target
/// (`id == 0`, no per-entry id), the last legacy row whose `observed_at`
/// matches. The compaction Rule 3 matcher (`log.rs`) mirrors this predicate
/// exactly, so a delete/amend compacts against the same row it removes on
/// replay.
pub(super) fn find_entry<T: LoggedEntry>(rows: &[T], id: u64, observed_at: i64) -> Option<usize> {
    let id = sanitize_entry_id(id);
    if id != 0 {
        rows.iter().rposition(|r| r.entry_id() == id)
    } else {
        rows.iter()
            .rposition(|r| r.entry_id() == 0 && r.observed_at() == observed_at)
    }
}

/// A profile-history entry's position: its effective day (`None` = −∞ as
/// `i64::MIN`), then the instant it was made (`edited_at`; `None`, or any
/// undated edit, = the start of that day). The history is kept ascending by
/// this key: an edit supersedes every entry at or after its own key (an
/// undated one the whole history), so a later same-day edit leaves the earlier
/// one in force for the part of the day before it.
pub(super) fn history_key(p: &Profile) -> (i64, i64) {
    match p.effective_from_epoch_day {
        Some(day) => (day, p.edited_at.unwrap_or(i64::MIN)),
        None => (i64::MIN, i64::MIN),
    }
}

/// The profile effective on local day `day`: the latest history entry whose
/// effective day ([`history_key`]) is `<= day`. When every entry is dated
/// strictly after `day` (a plan day before the first edit), the earliest entry
/// is used so the day still has a profile. A birthday-carrying profile is
/// aged for `day` itself
/// (`ingest::aged_on`), so a day the under-18 gate held keeps its gate after
/// the 18th birthday.
pub(super) fn profile_for_day(model: &Model, day: i64) -> Option<std::borrow::Cow<'_, Profile>> {
    in_force(model, day, |_| true)
}

/// The profile in force at instant `at` (unix seconds) on a clock at UTC
/// offset `utc_offset_sec`: [`profile_for_day`] of `at`'s local day, except
/// that an edit made later that same day (its `edited_at` after `at`, on its
/// history day) is not in force yet.
pub(super) fn profile_at(
    model: &Model,
    at: i64,
    utc_offset_sec: i64,
) -> Option<std::borrow::Cow<'_, Profile>> {
    let day = crate::local_day(at, utc_offset_sec);
    in_force(model, day, |p| {
        !p.edited_at.is_some_and(|t| {
            t > at && history_key(p).0 == day && crate::local_day(t, utc_offset_sec) == day
        })
    })
}

/// The latest history entry with history day `<= day` that `admit` lets in
/// (else the earliest entry), aged for `day`.
fn in_force(
    model: &Model,
    day: i64,
    admit: impl Fn(&Profile) -> bool,
) -> Option<std::borrow::Cow<'_, Profile>> {
    let p = model
        .profile_history
        .iter()
        .rev()
        .find(|p| history_key(p).0 <= day && admit(p))
        .or_else(|| model.profile_history.first())?;
    Some(super::ingest::aged_on(p, day))
}

/// The local day the profile `field` of `p` was first stated: the effective
/// day of the earliest unbroken run of profile-history entries ending at `p`
/// that all carry the same value, so re-saving the profile with the answer
/// unchanged never makes it look fresh. `None` when `field` is unset or the
/// entry undated. `p` is located by its [`history_key`] (unique in the
/// history), so a day-aged copy from `profile_for_day` finds its entry.
pub(super) fn stated_since(
    model: &Model,
    p: &Profile,
    field: impl Fn(&Profile) -> Option<f64>,
) -> Option<i64> {
    let km = field(p)?;
    let h = &model.profile_history;
    let Some(end) = h.iter().position(|q| history_key(q) == history_key(p)) else {
        return p.effective_from_epoch_day;
    };
    let start = h[..end]
        .iter()
        .rposition(|q| field(q) != Some(km))
        .map_or(0, |i| i + 1);
    h[start].effective_from_epoch_day
}
