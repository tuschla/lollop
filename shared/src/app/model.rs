//! Core state: the persisted `Model`, its private logged records and the
//! calculator queries it echoes.

use serde::{Deserialize, Serialize};

use crate::app::profile_age::aged_on;
use crate::event::entry::{EntryRef, LoggedEntry};
use crate::event::rules::{Family, PlanStep};
use crate::event::{ActivityKind, RunMeta, SessionReview};
use crate::gps::GpsPoint;
use crate::profile::{DistanceUnit, Profile};
use crate::schema::{CheckinInput, ReadinessInput};

// `Serialize` derives the content fingerprint that keys the view caches
// (`memo::fingerprint`); the type never rides the wire.
#[derive(Clone, Serialize)]
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
    /// another. A row an old build logged without an id carries the one its
    /// `observed_at` gives (`event::entry::row_id`).
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
    /// tap, a pair an old build logged without ids) still order the same way
    /// every time.
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

// `Serialize`: the view-cache fingerprint only, as for `LoggedSet`.
#[derive(Clone, Serialize)]
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
    /// Stable per-entry identity; see [`LoggedSet::entry_id`].
    pub(super) entry_id: u64,
    /// A legacy undated `SetProfile` arrived after this row was logged: its
    /// `hr_pct_max` stays the figure it had then (the edit applies from its
    /// position in the log, not to runs already logged). Cleared by
    /// `ClearProfile`. Kept as row state: the edit carries no date to stamp it
    /// with that a compacted log would reproduce.
    pub(super) hr_pinned: bool,
    /// The run's tag, rating, typed bpm and logged UTC offset as the event
    /// stated them, with the bpm only when plausible
    /// (`running::plausible_avg_hr_bpm`) and an amend keeping the
    /// original row's offset. The run card shows the typed bpm rather than the
    /// derived percentage; the %HRmax conversion reads the run's local day on
    /// the logged offset (`None`, a legacy row: the clock's current offset).
    pub(super) meta: RunMeta,
}

/// One logged "other activity" (sport, cycling, a PE lesson…): the minutes the
/// under-18 WHO activity card counts (YOUTH-PA-001). Model-internal, built
/// from `Event::LogActivity`.
#[derive(Clone)]
pub(super) struct LoggedActivity {
    pub(super) kind: ActivityKind,
    /// In (0, `event::entry::ACTIVITY_MAX_MINUTES`].
    pub(super) minutes: f64,
    pub(super) vigorous: bool,
    pub(super) strengthening: bool,
    /// Log time, unix seconds; 0 when undated.
    pub(super) observed_at: i64,
    /// Stable per-entry identity; see [`LoggedSet::entry_id`].
    pub(super) entry_id: u64,
}

macro_rules! impl_entry {
    ($($t:ty),*) => {$(
        impl LoggedEntry for $t {
            fn entry(&self) -> EntryRef {
                EntryRef { id: self.entry_id }
            }
        }
    )*};
}
impl_entry!(LoggedSet, LoggedRun, LoggedActivity);

/// A "the user ended this exercise early" marker, last-write-wins per
/// `event::rules::end_marker_key` (movement + local `epoch_day`).
/// `ended` true makes the plan item done regardless of set count, `false`
/// reopens it. Model-internal (built from `Event::SetExerciseEnded`).
#[derive(Clone)]
pub(super) struct EndedMarker {
    pub(super) exercise: String,
    pub(super) epoch_day: i64,
    pub(super) ended: bool,
    pub(super) observed_at: i64,
}

/// A readiness input removed by a dated `RemoveReadiness`. Today's state and
/// every future day ignore it exactly as if deleted; a past day whose local
/// date precedes the removal's local date was shown with it in force, so the
/// per-day recompute keeps it for that day. Model-internal.
// `Serialize`: the view-cache fingerprint only, as for `LoggedSet`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(super) struct RetractedInput {
    pub(super) input: ReadinessInput,
    /// Unix seconds of the removal.
    pub(super) retracted_at: i64,
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
    /// User-declared "I ended this exercise early" markers, last-write-wins
    /// per `event::rules::end_marker_key`.
    /// An ended marker makes the plan item done regardless of set count;
    /// `ended: false` reopens it. Cleared by `ClearSets`.
    pub(super) ended_exercises: Vec<EndedMarker>,
    /// The current per-session review (execution context only), if submitted.
    /// `ClearReview` empties it. Never carries weekly/clinical fields.
    pub(super) review: Option<SessionReview>,
    /// The session part of every session-setting review, in submission order
    /// (`ClearReview` keeps it): a past day's plan reads the review in force
    /// on that day ([`crate::app::anchors::off_day_through`]).
    pub(super) review_history: Vec<SessionReview>,
    /// Every weekly check-in ever submitted, in submission order: the latest
    /// `SubmitWeeklyCheckin`, or the weekly part of a legacy `SubmitReview`,
    /// is the check-in in force ([`Model::weekly_checkin`]). A red flag there
    /// stays until a LATER weekly check-in answers "no" (or
    /// `ClearWeeklyCheckin`); session reviews never touch it. A clear keeps the
    /// history: a past day keeps the deload / downgrade windows the check-ins
    /// gave it, so a later check-in or a clear never re-judges it.
    pub(super) weekly_history: Vec<SessionReview>,
    /// `ClearWeeklyCheckin` since the last weekly check-in: none is in force.
    pub(super) weekly_checkin_cleared: bool,
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
    /// is removed and set up again (`logged_days::lift_deload_days`).
    pub(super) past_plans: Vec<(i64, i64)>,
    /// `ClearPlan` was the last plan event (`GeneratePlan` resets it).
    pub(super) plan_removed: bool,
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
    pub(super) calis_placements: Vec<crate::calisthenics::progression::CalisPlacement>,
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

    /// The unit core-rendered distances read in: the profile's, km before one
    /// exists or when it states none.
    pub(super) fn distance_unit(&self) -> DistanceUnit {
        self.profile()
            .map_or(DistanceUnit::Km, Profile::display_distance_unit)
    }

    /// The weekly check-in in force: the latest one, unless cleared since.
    pub(super) fn weekly_checkin(&self) -> Option<&SessionReview> {
        if self.weekly_checkin_cleared {
            None
        } else {
            self.weekly_history.last()
        }
    }

    /// `ClearWeeklyCheckin`: no check-in is in force until the next one.
    pub(super) fn clear_weekly_checkin(&mut self) {
        self.weekly_checkin_cleared = true;
    }

    /// A family's clear event: empty every store the family's events write
    /// ([`Event::family`](crate::event::Event::family)). The review family's
    /// two clears each reset one store (`model.review`,
    /// [`Model::clear_weekly_checkin`]) and keep the weekly history; the
    /// clock has no clear.
    pub(super) fn clear(&mut self, family: Family) {
        match family {
            Family::Readiness => {
                self.inputs.clear();
                self.retracted_inputs.clear();
            }
            Family::Sets => {
                self.sets.clear();
                self.ended_exercises.clear();
                self.calis_placements.clear();
            }
            Family::Runs => self.runs.clear(),
            Family::Activities => self.activities.clear(),
            Family::Profile => {
                self.profile_history.clear();
                // Every run falls back to the empty history, including the
                // ones a legacy undated edit pinned (as a compacted log, which
                // drops that edit with the history, replays them).
                for run in &mut self.runs {
                    run.hr_pinned = false;
                }
            }
            Family::RacePrediction => self.race_query = None,
            Family::HypertrophyPlan => self.hypertrophy_plan_query = None,
            Family::Protein => self.protein_query = None,
            Family::HrZones => self.hr_zone_query = None,
            Family::Checkins => {
                self.checkins.clear();
                self.hrv_baseline_from = None;
            }
            Family::Plan => {
                if PlanStep::of(false, self.plan_request.is_some()) == PlanStep::Close
                    && let (Some(end), Some(req)) =
                        (self.today_or_plan_start(), self.plan_request.take())
                {
                    self.past_plans.push((req.start_epoch_day, end));
                }
                self.session_moves.clear();
                self.plan_removed = true;
            }
            Family::Review | Family::Today => {}
        }
    }

    /// The shell's local today, else (before the first `SetToday`: a legacy
    /// log's replay, a test) the plan start, the one other day the shell's
    /// clock supplied. `None` with neither.
    pub(super) fn today_or_plan_start(&self) -> Option<i64> {
        self.today_epoch_day
            .or(self.plan_request.map(|p| p.start_epoch_day))
    }

    /// The "so far" row filter as of the shell's local today.
    pub(super) fn up_to_today(&self) -> crate::day::UpToToday {
        crate::day::UpToToday::new(self.today_epoch_day, self.today_utc_offset_sec)
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

/// The heart-rate-zone query, stored on the model and echoed back so a shell
/// rehydrates its form. Sibling to the `hr_zones` result rows.
#[derive(Serialize, Deserialize, crate::wire::Wire, Debug, Default, Clone, PartialEq)]
pub struct HrZoneInputView {
    pub age_years: f64,
    /// Resting HR, bpm: arms the running-005 %HRmax-vs-Karvonen method
    /// preference and the Karvonen band targets. `None` = %HRmax only.
    pub resting_hr_bpm: Option<f64>,
}

/// The absolute-protein-target query, stored on the model and echoed back so a
/// shell rehydrates its form. Sibling to the `protein_targets` result rows.
#[derive(Serialize, Deserialize, crate::wire::Wire, Debug, Default, Clone, PartialEq)]
pub struct ProteinInputView {
    pub bodyweight_kg: f64,
    pub masters: bool,
    pub deficit: bool,
}

/// The hypertrophy-plan query, stored on the model and echoed back so a shell
/// rehydrates its form. Sibling to the `hypertrophy_plan` result rows.
#[derive(Serialize, Deserialize, crate::wire::Wire, Debug, Default, Clone, PartialEq)]
pub struct HypertrophyInputView {
    pub muscle: String,
    pub weeks: u8,
    /// The muscle is not growing on the current volume (hypertrophy-008).
    pub not_growing: bool,
    /// Recovery from the current volume is easy (hypertrophy-008; with
    /// `not_growing` → +2 sets next mesocycle).
    pub recovering_easily: bool,
}

/// The profile effective on local day `day`: the latest history entry whose
/// effective day ([`Profile::history_key`]) is `<= day`. When every entry is dated
/// strictly after `day` (a plan day before the first edit), the earliest entry
/// is used so the day still has a profile. A birthday-carrying profile is
/// aged for `day` itself
/// ([`aged_on`]), so a day the under-18 gate held keeps its gate after
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
    let day = crate::day::local_day(at, utc_offset_sec);
    in_force(model, day, |p| {
        !p.edited_at.is_some_and(|t| {
            t > at && p.history_key().0 == day && crate::day::local_day(t, utc_offset_sec) == day
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
        .find(|p| p.history_key().0 <= day && admit(p))
        .or_else(|| model.profile_history.first())?;
    Some(aged_on(p, day))
}

/// The local day the profile `field` of `p` was first stated: the effective
/// day of the earliest unbroken run of profile-history entries ending at `p`
/// that all carry the same value, so re-saving the profile with the answer
/// unchanged never makes it look fresh. `None` when `field` is unset or the
/// entry undated. `p` is located by its [`Profile::history_key`] (unique in the
/// history), so a day-aged copy from `profile_for_day` finds its entry.
pub(super) fn stated_since(
    model: &Model,
    p: &Profile,
    field: impl Fn(&Profile) -> Option<f64>,
) -> Option<i64> {
    let km = field(p)?;
    let h = &model.profile_history;
    let Some(end) = h.iter().position(|q| q.history_key() == p.history_key()) else {
        return p.effective_from_epoch_day;
    };
    let start = h[..end]
        .iter()
        .rposition(|q| field(q) != Some(km))
        .map_or(0, |i| i + 1);
    h[start].effective_from_epoch_day
}
