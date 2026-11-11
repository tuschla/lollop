//! The `Event` vocabulary the shells send, and the rules both `update` and
//! log compaction (`crate::log`) read off an event: how a raw line is
//! normalized on entry ([`Event::normalized`]), which logged row an edit
//! names ([`entry`]), which family a clear resets and how plan spans,
//! readiness removals and end markers are keyed ([`rules`]), and how a review
//! payload splits ([`review`]).

use serde::{Deserialize, Serialize};

use crate::gps::GpsPoint;
use crate::profile::Profile;
use crate::sanitize::Sanitize;
use crate::schema::{CheckinInput, ReadinessInput, ReadinessSignal, WorkoutType};

pub(crate) mod entry;
pub mod review;
pub(crate) mod rules;

pub use review::{Decouple, IntervalExec, LiftExec, SessionReview};

/// Wire tags of events the core no longer has (the deleted Cooper, critical
/// speed and APRE calculators). Replay rejects them, so compaction drops them.
pub const RETIRED_TAGS: [&str; 6] = [
    "ComputeCooper",
    "ClearCooper",
    "ComputeCriticalSpeed",
    "ClearCriticalSpeed",
    "ComputeApre",
    "ClearApre",
];

/// Which logged history a [`Event::DeleteEntry`] targets: the model's
/// `sets`, `runs` or `activities`.
#[derive(
    Serialize, Deserialize, crate::wire::Wire, Debug, Clone, Copy, PartialEq, Eq, Sanitize,
)]
pub enum EntryKind {
    Set,
    Run,
    Activity,
}

/// What kind of other activity a [`Event::LogActivity`] records (wire:
/// the variant name).
#[derive(
    Serialize, Deserialize, crate::wire::Wire, Debug, Clone, Copy, PartialEq, Eq, Default, Sanitize,
)]
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

#[derive(Serialize, Deserialize, crate::wire::Wire, Debug, Clone, PartialEq, Sanitize)]
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
        #[sanitize_with = "crate::sanitize::reps"]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        hold_sec: Option<u16>,
    },
    /// Drop all logged sets.
    ClearSets,
    /// Mark a plan exercise ended early for a session day, or reopen it.
    /// Last-write-wins per `rules::end_marker_key` (movement, `epoch_day`);
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
        #[sanitize_with = "crate::sanitize::non_negative"]
        distance_km: f64,
        #[sanitize_with = "crate::sanitize::non_negative"]
        duration_min: f64,
        hr_pct_max: f64,
        #[sanitize_with = "crate::sanitize::non_negative"]
        longest_recent_km: f64,
        /// Log time, unix seconds; shell-supplied, 0 = undated.
        #[serde(default)]
        observed_at: i64,
        /// User-declared run-intent label. USER DATA with no evidence of its
        /// own: the weekly report's hard-session checks honor it as the
        /// session's declared goal (running-012/023). `None` = untagged.
        #[serde(skip_serializing_if = "Option::is_none")]
        workout_type: Option<WorkoutType>,
        /// Whole-session effort on the CR-10 scale (0–10), rated ~30 min after
        /// the run (LOAD-SRPE-001). `None` = unrated; values above 10 clamp to
        /// 10 at ingest.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[sanitize_with = "crate::sanitize::cr10"]
        session_rpe: Option<u8>,
        /// Average heart rate in bpm as the user's watch shows it (UXL-12). When
        /// set and plausible, `update` converts it to `hr_pct_max` against the
        /// measured HRmax, else the Tanaka age estimate (RUN-HRMAX-001), of the
        /// profile in force at the run's instant (re-derived when the profile
        /// history changes); with neither known the sent `hr_pct_max` stands.
        #[serde(skip_serializing_if = "Option::is_none")]
        avg_hr_bpm: Option<f64>,
        /// Stable per-entry id; see [`Event::LogSet`].
        #[serde(default)]
        entry_id: u64,
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
        #[sanitize_with = "crate::sanitize::non_negative"]
        longest_recent_km: f64,
        /// Log time, unix seconds; shell-supplied, 0 = undated. The GPS fixes
        /// carry their own per-point `observed_at`; this is the session's
        /// logged-at stamp for history display.
        #[serde(default)]
        observed_at: i64,
        /// Stable per-entry id; see [`Event::LogSet`].
        #[serde(default)]
        entry_id: u64,
        /// User-declared run-intent label; see [`Event::LogRun`].
        #[serde(skip_serializing_if = "Option::is_none")]
        workout_type: Option<WorkoutType>,
        /// Indices into `points` that begin a new recording segment: the shell's
        /// pause/relocation boundaries. Empty = one continuous segment.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        segment_starts: Vec<u32>,
        /// Whole-session effort, CR-10 (0–10), see [`Event::LogRun`].
        #[serde(skip_serializing_if = "Option::is_none")]
        #[sanitize_with = "crate::sanitize::cr10"]
        session_rpe: Option<u8>,
        /// Average heart rate the athlete entered, bpm; see [`Event::LogRun`].
        #[serde(skip_serializing_if = "Option::is_none")]
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
    /// whose id matches (`event::entry::row_id`). A no-op when nothing
    /// matches. Compaction cancels it against its matched log line (Rule 3).
    DeleteEntry {
        kind: EntryKind,
        entry_id: u64,
        /// The row's `observed_at`: names the row in a line an old build
        /// wrote with `entry_id` 0.
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
        #[sanitize_with = "crate::sanitize::reps"]
        reps: u32,
        rpe: f64,
        /// The NEW timestamp for the amended entry (may differ from the original
        /// when the user re-dates a set).
        #[serde(default)]
        observed_at: i64,
        /// The OLD `observed_at` of the row being amended: names the row in
        /// a line an old build wrote with `entry_id` 0. 0 = `observed_at`.
        #[serde(default)]
        observed_at_fallback: i64,
        /// Timed-hold duration, seconds (see [`Event::LogSet`]).
        #[serde(skip_serializing_if = "Option::is_none")]
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
        #[sanitize_with = "crate::sanitize::non_negative"]
        distance_km: f64,
        #[sanitize_with = "crate::sanitize::non_negative"]
        duration_min: f64,
        hr_pct_max: f64,
        #[serde(default)]
        #[sanitize_with = "crate::sanitize::non_negative"]
        longest_recent_km: f64,
        /// The NEW timestamp for the amended entry.
        #[serde(default)]
        observed_at: i64,
        /// User-declared run-intent label; see [`Event::LogRun`].
        #[serde(skip_serializing_if = "Option::is_none")]
        workout_type: Option<WorkoutType>,
        /// Whole-session effort, CR-10 (0–10), see [`Event::LogRun`].
        #[serde(skip_serializing_if = "Option::is_none")]
        #[sanitize_with = "crate::sanitize::cr10"]
        session_rpe: Option<u8>,
        /// Average heart rate in bpm, converted like [`Event::LogRun`]'s.
        #[serde(skip_serializing_if = "Option::is_none")]
        avg_hr_bpm: Option<f64>,
        /// The OLD `observed_at` of the row being amended; see [`Event::AmendSet`].
        #[serde(default)]
        observed_at_fallback: i64,
    },
    /// Trim a GPS run's recorded head/tail: keep only the fixes whose per-point
    /// `observed_at` lies in `[keep_from_sec, keep_until_sec]` (inclusive,
    /// absolute unix seconds — decimation-agnostic). The run's identity
    /// (`entry_id`, `observed_at`) is UNCHANGED; every derived figure re-derives
    /// from the sliced track at `view()`. Matches like `DeleteEntry`/`AmendRun`.
    TrimRunTrack {
        entry_id: u64,
        /// The run's `observed_at`; see [`Event::DeleteEntry`].
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
        /// The run's `observed_at`; see [`Event::DeleteEntry`].
        #[serde(default)]
        observed_at_fallback: i64,
        /// Whole-session effort, CR-10 (0–10); values above 10 clamp to 10.
        #[sanitize_with = "crate::sanitize::cr10"]
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
        #[serde(skip_serializing_if = "Option::is_none")]
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
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        not_growing: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
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
        #[serde(skip_serializing_if = "Option::is_none")]
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
    /// buckets every UTC `observed_at` into a local day (`crate::day::local_day`).
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

impl Event {
    /// The event as the core applies it: legacy wire shapes upgraded to the
    /// current one, then every value sanitized. `update` and compaction both
    /// read events only through this.
    #[must_use]
    pub(crate) fn normalized(mut self) -> Self {
        self.upgrade_legacy();
        self.sanitized()
    }

    /// An amend written before `observed_at_fallback` existed (wire 0) names
    /// its row by its `observed_at`.
    fn upgrade_legacy(&mut self) {
        if let Event::AmendSet {
            observed_at,
            observed_at_fallback,
            ..
        }
        | Event::AmendRun {
            observed_at,
            observed_at_fallback,
            ..
        } = self
            && *observed_at_fallback == 0
        {
            *observed_at_fallback = *observed_at;
        }
    }
}
