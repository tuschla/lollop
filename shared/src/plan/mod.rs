//! Program synthesis: compose the graded band functions (`strength::dup_day_rx`,
//! `strength::loading_rx`, `running::workout_rx`, `running::goal_week_plan`)
//! into a dated [`Program`] of `Recommended<Prescription>` for ONE Mon..Sun
//! week at 0-based `day` offsets. Pure and deterministic; `app/` maps the
//! offsets to calendar days and applies readiness/safety downstream.
//!
//! 1. Shape: lift and run day counts on the athlete's weekdays (LIFT-DAYS-001,
//!    ADH-DAYS-001, HYB-RUNCAP-001).
//! 2. Content: run kinds and volume, lift-day template, hybrid dosing
//!    (HYB-MAINT-001/002, HYB-TENDON-001).
//! 3. Placement: `schedule::place_week` (HYB-CAP-001, RUN-QUALITY-001).
//! 4. Block position: lift deload every 5th–6th week (HYP-DELOAD-CADENCE-001,
//!    STR-DELOAD-VOL-001) or the meet taper.

mod calis;
mod hypertrophy;
mod lift;
mod modifiers;
mod run;
mod schedule;

use crate::app::{PlanWithheld, Profile};
use crate::evidence::claims;
use crate::hybrid::{self, ConcurrentGoal};
use crate::individualization::{TrainingAge, training_age_from_cadence};
use crate::running::{self, GoalDistance};
use crate::schema::{
    Goal, LadderId, MesoPhase, Mesocycle, Program, Recommended, RunSessionType, Session,
    SessionType,
};
use crate::strength::{DupDay, LiftGoal};

use calis::calisthenics_session;
use lift::{BlockCtx, dup_emphasis, lift_session, main_lifts};
use run::{RunWeekPlan, RunWeekRequest, run_week_plan, run_week_sessions};
use schedule::{LiftSpec, place_week};

pub(crate) use hypertrophy::{
    hypertrophy_block_targets, hypertrophy_week_targets, uses_double_progression,
};
#[cfg(test)]
pub(crate) use lift::NAME_CLASSIFICATIONS;
pub(crate) use lift::count_name_classification;
pub(crate) use lift::{
    MovementPattern, is_back_squat, is_barbell_lift, is_isolation_lift, movement_key,
    movement_pattern, same_movement,
};
pub(crate) use run::is_quality as is_quality_run;
pub use run::{daily_avg_run_km, run_slot_count};
pub(crate) use schedule::{DayLoad, MoveBlock, move_conflict, move_options, session_load};

/// Anchors derived from the user's logged history by
/// `app/anchors.rs::build_plan_anchors` and completed per week in
/// `app/plan_views/plan_week.rs::build_plan_views`. Turns the %1RM bands into
/// real kg and lets the plan lead with the user's OWN exercises — a lift the
/// user has logged carries an e1RM anchor and is prescribed by load; anything
/// unlogged falls back to RIR.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Anchors {
    /// (exercise name, e1RM kg), most-recent-first, one entry per MOVEMENT
    /// (`movement_key`: "Back squat" and "Barbell back squat" share one anchor).
    /// The e1RM is the best estimate within the movement's most recent window
    /// (last 3 sessions or 21 days), never the all-time best, and is
    /// the logged value itself: a working-load derate (detraining, layoff ramp,
    /// e1RM drop) lives in `lift_derates`. A lift untrained for >8 weeks has no
    /// entry (falls back to RIR).
    pub lift_e1rm: Vec<(String, f64)>,
    /// Longest completed run (km) in the trailing 30-day window, from
    /// `app/anchors.rs::build_run_anchors`; `None` with no run in the window.
    /// Same window and predicate as the RUN-SPIKE-001 baseline
    /// (`spike_baseline_km`), so a long run at this value is no spike.
    pub longest_recent_run_km: Option<f64>,
    /// Measured weekly running volume (km): trailing-28-day total ÷ the 1–4
    /// weeks the window spans since its first run, from
    /// `app/anchors.rs::build_run_anchors`; `None` with no run in the window.
    /// Read by `run::effective_weekly_km`, which sizes the whole run week.
    pub recent_weekly_km: Option<f64>,
    /// `true` when `longest_recent_run_km` comes from a run older than the
    /// 30-day window, tapered per DETRAIN-001 in
    /// `app/anchors.rs::build_run_anchors`. A long run raised by it cites
    /// DETRAIN-001 instead of RUN-SPIKE-001.
    pub longest_run_detrained: bool,
    /// `true` while the profile's `stated_longest_recent_km` is inside its own
    /// 30-day window (dated by the profile edit that first stated it), set in
    /// `app/plan_views/plan_week.rs::build_plan_views`. The stated run then still IS a
    /// run of the last 30 days, so the long-run baseline is the longer of it
    /// and `longest_recent_run_km` (RUN-START-001 / RUN-SPIKE-001); past the
    /// window a logged run supersedes it.
    pub stated_longest_in_window: bool,
    /// Per-lift working-load derates (at most one per movement), applied to the
    /// %1RM LOAD in `flatten_prescription` and to a carried last-working load:
    /// detraining credit after 31–56 days without the movement (DETRAIN-STR-001
    /// via the REENTRY-001 bracket), the re-entry ramp for pre-break numbers
    /// (REENTRY-RAMP-001) after a declared layoff, a week or more without any
    /// lifting, or an undeclared 31–56-day gap in the movement's own history,
    /// or a 5 % cut after the latest session's e1RM fell ≥5 % below baseline
    /// (AUTOREG-E1RM-GATE-001). Never stacked: one derate per movement.
    pub lift_derates: Vec<LiftDerate>,
    /// `true` while a declared >8 wk layoff's re-entry window is running (KB
    /// Table 3.4b: no load fraction, treat as a fresh novice). Pre-layoff anchors
    /// are dropped from `lift_e1rm` during the window; a movement logged since the
    /// return anchors normally. Cleared once the ramp window ends.
    pub reentry_novice: bool,
    /// Per-ladder calisthenics tier state, derived in
    /// `app/plan_views/calis_anchors.rs::build_calisthenics_anchors`
    /// from logged sets (empty for a `Weights` profile). A dynamic ladder with no
    /// logged rung is simply absent (the plan starts it at rung 0); a skill track is
    /// absent until a rung is logged (opt-in, never force-fed).
    pub calisthenics: Vec<CalisAnchor>,
    /// Pace anchor: the athlete's current race fitness (Daniels VDOT) from a
    /// recent race no older than ~8 weeks (running-007 / running-041,
    /// RUN-RETEST-001), set in `app/plan_views/plan_week.rs::build_plan_views` via
    /// `app/run_targets.rs::pace_anchor_vdot`. Training paces per band come from
    /// `load::vdot_band_pace_sec_per_km`. `None` = no fresh race: runs are
    /// prescribed by heart rate / effort only, never an invented pace.
    pub run_vdot: Option<f64>,
    /// The athlete's LAST working sets per movement (one entry per movement,
    /// most-recent-first), derived in `app/anchors.rs::build_plan_anchors` from
    /// the newest logged session of that movement within the detraining
    /// horizon. Unlike `lift_e1rm` it covers isolation and bodyweight lifts
    /// too, so a logged curl stays in the plan. Read by:
    /// - hypertrophy double progression (HYP-DOUBLEPROG-001);
    /// - `lift::earned_week` (a missed rep withholds the STR-PCTPROG-001 step);
    /// - `lift::novice_jump_cap_pct` (the STR-1RMTEST-001 novice step cap);
    /// - [`Anchors::carried_derate_for`] and the app's carried load.
    pub last_working: Vec<LastWorking>,
    /// Local epoch day of the Monday of the week being synthesized, set in
    /// `app/plan_views/plan_week.rs::build_plan_views`. Dates the week against
    /// `Profile::goal_race_epoch_day` (race-date periodization, RUN-PERIOD-001)
    /// and the strength meet date. `None` = undated (no race-date phases).
    pub week_start_epoch_day: Option<i64>,
    /// Logged running km per COMPLETED calendar week before
    /// `week_start_epoch_day`, most-recent-first (index 0 = last week), up to
    /// 12 weeks; built by `app/anchors.rs::build_run_week_km`. A measured fact
    /// that carries volume progression across run cycles (RUN-VOLUME-001):
    /// the next cycle starts from the load the athlete actually reached.
    pub run_week_km: Vec<f64>,
    /// Local epoch day the plan started (`GeneratePlan`), set in
    /// `app/plan_views/plan_week.rs::build_plan_views`. In the start week (the Monday is
    /// `week_start_epoch_day`) that weekday holds a session when the athlete
    /// can train on it, so a new plan never opens on a rest day.
    pub plan_start_epoch_day: Option<i64>,
    /// The athlete's latest undeclared break from running (a week or more
    /// with no logged run), set in
    /// `app/plan_views/plan_week.rs::build_plan_views` from
    /// `app/anchors.rs::build_run_break` while it still shapes the plan (its
    /// re-entry ramp runs, or a run from before it still carries long-run
    /// credit). `None` without one, or when the stated weekly distance was
    /// entered after the break began (the athlete's current answer then
    /// governs). Read by `run::effective_weekly_km` (the File 08 Table 3.4b
    /// running re-entry, RUN-REENTRY-001), `run::base_weekly_km` (pre-break
    /// weeks never carry the cycle's load level) and `run::day_ctx` (the
    /// long-run and spike baselines).
    pub run_break: Option<RunBreak>,
}

/// An undeclared break from running (see [`Anchors::run_break`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunBreak {
    /// Weeks fully off running: from the last run before the break to the
    /// first run after it, or to the day being prescribed while it runs.
    pub weeks_off: f64,
    /// Local day of the last run before the break.
    pub last_run_epoch_day: i64,
    /// The measured weekly volume (28-day average) as of that last run: the
    /// prior volume the re-entry fraction scales, with the stated one.
    pub prior_weekly_km: Option<f64>,
    /// The RUN-REENTRY-001 bracket as of the day being prescribed (restart
    /// fraction on its ramp, rebuild-long-run and near-novice spike flags).
    /// `rebuild_long_run` / `near_novice_spike` hold only while a run from
    /// before the break would still carry long-run credit (DETRAIN-001).
    pub reentry: crate::individualization::RunReEntry,
    /// Longest run since the break, detraining-adjusted like
    /// `Anchors::longest_recent_run_km` but over runs since the return only;
    /// `None` when none was logged yet. After 2+ weeks off it replaces the
    /// pre-break longest as the demonstrated long-run capacity ("rebuild long
    /// run"); after 4+ weeks also as every run's spike baseline.
    pub longest_since_km: Option<f64>,
    /// `true` when `longest_since_km` is a detraining-tapered value (its run
    /// is older than the 30-day full-credit window, DETRAIN-001).
    pub longest_since_detrained: bool,
    /// The stated longest run was entered after the break began (a current
    /// answer, RUN-START-001), set in `build_plan_views`.
    pub stated_longest_after: bool,
}

/// One movement's last logged working sets (see [`Anchors::last_working`]).
#[derive(Debug, Clone, PartialEq)]
pub struct LastWorking {
    /// Display name as last logged.
    pub exercise: String,
    /// The session's top working load in kg (0.0 for a bodyweight set).
    pub weight_kg: f64,
    /// `(reps, rpe)` of every set logged at `weight_kg` that session, in log
    /// order (rpe 0.0 = unrated).
    pub sets: Vec<(u32, f64)>,
    /// Whole days from that session to the anchor's reference day.
    pub age_days: i64,
}

impl Anchors {
    /// The last working sets of the same movement as `exercise`
    /// (`same_movement`-normalized), if any were logged.
    pub fn last_working_for(&self, exercise: &str) -> Option<&LastWorking> {
        let key = movement_key(exercise);
        self.last_working
            .iter()
            .find(|w| movement_key(&w.exercise) == key)
    }
}

/// Why an anchored lift's working load is scaled below its logged e1RM.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DerateCause {
    /// The movement has not been trained for `days_since` days (31–56).
    Detraining { days_since: i64 },
    /// A pre-break anchor on the re-entry ramp (a declared layoff, a week or
    /// more without any lifting, or an undeclared 31–56-day gap in the
    /// movement): day `day` of a `ramp_days`-long ramp back to full load.
    Reentry { day: i64, ramp_days: i64 },
    /// The latest session's e1RM came in ≥5 % under the anchor baseline.
    E1rmDrop,
}

/// One lift's working-load derate (see [`Anchors::lift_derates`]).
#[derive(Debug, Clone, PartialEq)]
pub struct LiftDerate {
    pub exercise: String,
    /// Fraction of the logged e1RM the working load is computed from (< 1).
    pub frac: f64,
    pub cause: DerateCause,
}

impl Anchors {
    /// The logged e1RM anchor for an exercise, if any: an exact
    /// (case-insensitive) name first, else the same MOVEMENT (`same_movement`),
    /// so naming variants share one anchor. Only a POSITIVE e1RM counts; a
    /// 0/negative value never anchors a load (it would render "@ 0 kg").
    pub fn e1rm_for(&self, exercise: &str) -> Option<f64> {
        self.lift_e1rm
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(exercise))
            .or_else(|| {
                let key = movement_key(exercise);
                self.lift_e1rm.iter().find(|(n, _)| movement_key(n) == key)
            })
            .map(|(_, v)| *v)
            .filter(|v| *v > 0.0)
    }

    /// The working-load derate for an exercise's movement, if one applies.
    pub fn load_frac_for(&self, exercise: &str) -> Option<&LiftDerate> {
        let key = movement_key(exercise);
        self.lift_derates
            .iter()
            .find(|d| d.exercise.eq_ignore_ascii_case(exercise) || movement_key(&d.exercise) == key)
    }

    /// The derate on a load carried from the movement's last working session
    /// (hypertrophy double progression, the start-weight suggestion). A
    /// re-entry ramp scales only numbers set before the break
    /// (REENTRY-RAMP-001): a session logged since the return already reflects
    /// it, so its load is carried unscaled.
    pub fn carried_derate_for(&self, exercise: &str) -> Option<&LiftDerate> {
        let d = self.load_frac_for(exercise)?;
        match d.cause {
            DerateCause::Reentry { day, .. }
                if self
                    .last_working_for(exercise)
                    .is_some_and(|w| w.age_days <= day) =>
            {
                None
            }
            _ => Some(d),
        }
    }

    /// The e1RM the working load is computed from: the logged anchor times its
    /// derate (1.0 when none applies).
    fn working_e1rm_for(&self, exercise: &str) -> Option<f64> {
        self.e1rm_for(exercise)
            .map(|e| e * self.load_frac_for(exercise).map_or(1.0, |d| d.frac))
    }
}

/// Derived tier state for one calisthenics ladder, replayed chronologically from
/// logged sets and rung placements (`app/plan_views/calis_anchors.rs::build_calisthenics_anchors`):
/// a state machine, not a monotone max (KB calis-progression §2.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalisAnchor {
    pub ladder: LadderId,
    /// Current rung: where the athlete's newest accepted session (or rung
    /// placement) put this ladder. A harder rung logged once without reaching
    /// its band floor is an exploratory attempt and does not move it.
    pub tier: u8,
    /// Trigger fired → the plan prescribes `tier + advance_steps` at the band
    /// floor, or the next added-load step on a loaded weighted rung (subject to
    /// the tendon rate-limit on straight-arm rungs and the G8 pain suppression).
    pub advance_ready: bool,
    /// Rungs an advance moves: 1 normally, `calisthenics::SKIP_STEPS` when the
    /// newest session beat the CAL-PLACE-001 skip threshold.
    pub advance_steps: u8,
    /// Floor missed on the 2 newest sessions (or on the first session after a
    /// step up / placement) → the plan prescribes the rung below.
    pub regress: bool,
    /// The current rung was entered by a regress: it carries the tempo bridge
    /// (CAL-PROG-TEMPO-001) until the athlete steps back up.
    pub bridged: bool,
    /// Newest logged `weight_kg` at the current rung (0.0 default; the user's
    /// own last load, never invented).
    pub last_added_kg: f64,
    /// Skill tracks: the user has logged a rung AND (for muscle-up) the G1 gate
    /// is met. Dynamic ladders are always active. A dormant skill track is not
    /// scheduled; an inactive muscle-up emits a guidance row instead.
    pub active: bool,
    /// The newest session met the advance target except that its last set was
    /// not rated: advance waits for an effort rating.
    pub needs_effort_rating: bool,
    /// Rungs to step down for a break from this ladder (CAL-REENTRY-001).
    pub reentry_steps: u8,
    /// The break exceeds 8 weeks: re-test the starting rung.
    pub retest: bool,
    /// The rung comes from a placement with no sets logged since.
    pub placed: bool,
    /// Newest loaded set at a weighted rung, `(rung, added_kg, reps, rpe)`, for
    /// the system-load e1RM line (CAL-E1RM-001).
    pub last_loaded_set: Option<(u8, f64, u32, f64)>,
}

impl Anchors {
    /// The derived anchor for a ladder, if one exists.
    pub fn calis_for(&self, ladder: LadderId) -> Option<&CalisAnchor> {
        self.calisthenics.iter().find(|a| a.ladder == ladder)
    }
}

/// Body region a lift day trains (KB Table 1.6 time-constrained templates,
/// LIFT-DAYS-001): full body, or one half of an upper/lower split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DayRegion {
    #[default]
    Full,
    Upper,
    Lower,
}

impl DayRegion {
    /// The day loads the legs (squat / hinge work).
    pub(crate) fn has_lower(self) -> bool {
        !matches!(self, DayRegion::Upper)
    }

    /// The day loads the upper body (push / pull work).
    pub(crate) fn has_upper(self) -> bool {
        !matches!(self, DayRegion::Lower)
    }

    /// Two days train at least one muscle group in common.
    pub(crate) fn overlaps(self, other: DayRegion) -> bool {
        (self.has_lower() && other.has_lower()) || (self.has_upper() && other.has_upper())
    }

    /// A lift of movement pattern `p` belongs on this day: push/pull on upper
    /// days, squat/hinge on lower days, an unclassified lift anywhere.
    pub(crate) fn fits(self, p: Option<MovementPattern>) -> bool {
        match (self, p) {
            (DayRegion::Full, _) | (_, None) => true,
            (DayRegion::Upper, Some(p)) => {
                matches!(p, MovementPattern::Push | MovementPattern::Pull)
            }
            (DayRegion::Lower, Some(p)) => {
                matches!(p, MovementPattern::Squat | MovementPattern::Hinge)
            }
        }
    }
}

/// The region of every lift day of one week, in weekday order (index = the
/// day's `BlockCtx::slot`). `Copy`, so it rides along in `BlockCtx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WeekRegions {
    regions: [DayRegion; 7],
    len: usize,
}

impl WeekRegions {
    pub(crate) fn new(regions: &[DayRegion]) -> Self {
        let len = regions.len().min(7);
        let mut r = [DayRegion::Full; 7];
        r[..len].copy_from_slice(&regions[..len]);
        Self { regions: r, len }
    }

    /// `n` full-body days.
    pub(crate) fn full(n: usize) -> Self {
        Self {
            regions: [DayRegion::Full; 7],
            len: n.min(7),
        }
    }

    pub(crate) fn as_slice(&self) -> &[DayRegion] {
        &self.regions[..self.len]
    }
}

/// Which side leads a hybrid week: the athlete's "which matters more" answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridLead {
    /// Strength, power or muscle leads; running is kept at a supportive dose.
    Lifting,
    /// Running leads; lifting is kept at a maintenance dose.
    Running,
}

/// The leading side of a hybrid (lift + run) profile; `None` when the profile
/// does not both lift and run.
pub fn hybrid_lead(profile: &Profile) -> Option<HybridLead> {
    (profile.weekly_sets > 0 && profile.running_days_per_week > 0).then_some(
        match profile.concurrent_goal {
            ConcurrentGoal::EndurancePriority => HybridLead::Running,
            ConcurrentGoal::Strength | ConcurrentGoal::Power | ConcurrentGoal::Hypertrophy => {
                HybridLead::Lifting
            }
        },
    )
}

/// A run-only athlete who opted into supplemental strength work
/// (RUN-STRENGTH-001: two short heavy sessions a week help runners).
fn runner_strength_active(profile: &Profile) -> bool {
    profile.weekly_sets == 0 && profile.running_days_per_week > 0 && profile.runner_strength
}

/// Weekdays the athlete can train (ADH-DAYS-001: the athlete picks the days).
/// `None` or an empty mask means any day. Bit 0 = Monday … bit 6 = Sunday.
fn available_weekdays(profile: &Profile) -> [bool; 7] {
    let mask = profile
        .training_weekdays
        .map(|m| m & 0x7f)
        .filter(|m| *m != 0)
        .unwrap_or(0x7f);
    std::array::from_fn(|d| mask & (1 << d) != 0)
}

/// How many lift and run days a week holds once fitted into the athlete's
/// available weekdays (one session per day: never doubling up).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WeekShape {
    pub(crate) lift_days: usize,
    pub(crate) run_days: usize,
}

/// The lift-day count when the athlete chose none: 3 pure, 2 hybrid.
fn default_lift_days(profile: &Profile) -> usize {
    if profile.running_days_per_week > 0 {
        2
    } else {
        3
    }
}

/// The week's lift/run day counts. Lift days come from the athlete's chosen
/// count (2–5, LIFT-DAYS-001), else the default (3 pure, 2 hybrid); a
/// running-led hybrid keeps 2–3 maintenance lift days (CONC-RE-001); a runner's
/// supplemental strength is 2 days (RUN-STRENGTH-001). Run days are
/// [`run_slot_count`] (goal table + the ≤3-day hybrid default, HYB-RUNCAP-001).
/// When the chosen weekdays can't hold both, the SUPPORTING side gives way
/// first (down to 2 days, HYB-MAINT-002's maintenance floor), then the leading
/// side.
pub(crate) fn week_shape(profile: &Profile) -> WeekShape {
    let lifting = profile.weekly_sets > 0;
    let running = profile.running_days_per_week > 0;
    let lead = hybrid_lead(profile);
    let mut n_lift = if lifting {
        let chosen = profile
            .lift_days_per_week
            .map(|d| (d as usize).clamp(2, 5))
            .unwrap_or(default_lift_days(profile));
        if lead == Some(HybridLead::Running) {
            chosen.min(hybrid::maintenance_lift_sessions().value.1 as usize)
        } else {
            chosen
        }
    } else if runner_strength_active(profile) {
        2
    } else {
        0
    };
    let mut n_run = if running { run_slot_count(profile) } else { 0 };
    let avail = available_weekdays(profile).iter().filter(|a| **a).count();
    if !running {
        n_lift = n_lift.min(avail);
    } else if !lifting {
        n_run = n_run.min(avail);
        n_lift = n_lift.min(avail - n_run);
    } else {
        let lift_leads = lead == Some(HybridLead::Lifting);
        while n_lift + n_run > avail {
            let (leading, supporting) = if lift_leads {
                (&mut n_lift, &mut n_run)
            } else {
                (&mut n_run, &mut n_lift)
            };
            if *supporting > 2 {
                *supporting -= 1;
            } else if *leading > 2 {
                *leading -= 1;
            } else if *supporting > 1 {
                *supporting -= 1;
            } else if *leading > 1 {
                *leading -= 1;
            } else {
                *supporting = 0;
            }
        }
    }
    WeekShape {
        lift_days: n_lift,
        run_days: n_run,
    }
}

/// Training sessions the plan schedules in a normal week for this profile (the
/// "planned" side of planned-vs-done, ADH-SELFMON-001 / ADH-PROGRESS-001).
pub fn planned_training_days(profile: &Profile) -> usize {
    if profile.health.any_gate() {
        return 0;
    }
    let s = week_shape(profile);
    s.lift_days + s.run_days
}

/// The lift-day template for `n` days a week (KB Table 1.6 via LIFT-DAYS-001):
/// 2 → full body ×2 (compounds), 3 → full body ×3, 4 → upper/lower ×2, 5 →
/// upper/lower ×2 plus a full-body day (every muscle 3×/week). Canonical order;
/// the scheduler tries its rotations.
fn lift_template(n: usize) -> Vec<DayRegion> {
    use DayRegion::{Full, Lower, Upper};
    match n {
        0 => Vec::new(),
        1..=3 => vec![Full; n],
        4 => vec![Upper, Lower, Upper, Lower],
        _ => vec![Upper, Lower, Full, Upper, Lower],
    }
}

/// CAP-1 (HYB-CAP-001): at most `max_lower` lift days carry lower-body work;
/// extra leg days become upper-body days, latest first.
fn cap_lower_days(regions: &mut [DayRegion], max_lower: usize) {
    let mut lower = regions.iter().filter(|r| r.has_lower()).count();
    for r in regions.iter_mut().rev() {
        if lower <= max_lower {
            break;
        }
        if r.has_lower() {
            *r = DayRegion::Upper;
            lower -= 1;
        }
    }
}

/// How many days a week the muscles of a `region` day are trained, the
/// volume-split denominator handed to the lift builders.
fn exposures(week: &[DayRegion], region: DayRegion) -> usize {
    let up = week.iter().filter(|r| r.has_upper()).count();
    let low = week.iter().filter(|r| r.has_lower()).count();
    match region {
        DayRegion::Upper => up,
        DayRegion::Lower => low,
        DayRegion::Full => up.max(low),
    }
    .max(1)
}

/// The day's exercises for its region: an upper day keeps push/pull lifts, a
/// lower day squat/hinge lifts, unclassified lifts go to both (and to full-body
/// days). `compounds_only` drops isolation work (Table 1.6: a 2-day week
/// preserves compounds and cuts accessories). Classified compounds lead, so
/// the first name stays a valid DUP primary.
fn names_for_region(names: &[String], region: DayRegion, compounds_only: bool) -> Vec<String> {
    let fits =
        |n: &String| region.fits(movement_pattern(n)) && !(compounds_only && is_isolation_lift(n));
    let (mut out, rest): (Vec<String>, Vec<String>) = names
        .iter()
        .filter(|n| fits(n))
        .cloned()
        .partition(|n| movement_pattern(n).is_some());
    out.extend(rest);
    if out.is_empty() { names.to_vec() } else { out }
}

/// Lifting accumulation block length: 5 load weeks before a deload week
/// (HYP-DELOAD-CADENCE-001: deload about every 4–6 weeks, prefer 5).
const LIFT_LOAD_WEEKS: u8 = 5;

/// `(load_weeks, cycle_len)` of the LIFTING block, independent of any running
/// cadence (a hybrid lifter does not inherit the 3:1 running deload).
/// Novices on linear progression get no calendar deload
/// (`cycle_len == load_weeks`): their deloads come from the review triggers
/// (stalls / effort gaps) instead of the calendar (STR-DELOAD-VOL-001,
/// HYP-DELOAD-CADENCE-001 "deload earlier only on fatigue signs").
fn lift_cycle(profile: &Profile) -> (u8, u8) {
    let ta = training_age_from_cadence(profile.progression_cadence).value;
    if ta == TrainingAge::Novice {
        (LIFT_LOAD_WEEKS, LIFT_LOAD_WEEKS)
    } else {
        (LIFT_LOAD_WEEKS, LIFT_LOAD_WEEKS + 1)
    }
}

/// Where a plan week sits in the lifting block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockPosition {
    /// 1-based week within the loading block (a linear novice's plan week:
    /// no calendar deload restarts it).
    pub week_in_block: u8,
    /// Loading weeks in the block.
    pub block_weeks: u8,
    /// The block's deload week.
    pub recovery: bool,
}

/// Where `week_index` sits in the lifting block. The single source for the
/// lift builders and the program summary. A running-led hybrid holds week 1
/// (maintenance dose, no set ramp: HYB-MAINT-001, and HYB-TENDON-001's
/// "progress one side, hold the other"). A novice on linear %1RM progression
/// has no calendar deload, so nothing restarts the ramp: the week keeps
/// counting and `strength::pct_for_week` holds its band/RIR cap (STR-PCTPROG-001
/// adds load per successful week; indiv-007 linear progression runs 3–6
/// months). Restarting at the band floor every block would be an unplanned
/// deload on completed sessions.
pub fn lift_block_position(profile: &Profile, week_index: u32) -> BlockPosition {
    let (block_weeks, cycle_len) = lift_cycle(profile);
    // `pos < cycle_len`, a `u8`.
    let pos = (week_index % u32::from(cycle_len)) as u8;
    let recovery = pos == block_weeks;
    let linear = cycle_len == block_weeks && !uses_double_progression(profile);
    let week_in_block = if hybrid_lead(profile) == Some(HybridLead::Running) {
        1
    } else if linear {
        u8::try_from(week_index.saturating_add(1)).unwrap_or(u8::MAX)
    } else if recovery {
        block_weeks
    } else {
        pos + 1
    };
    BlockPosition {
        week_in_block,
        block_weeks,
        recovery,
    }
}

/// Days from the week's Monday `week_start` to the meet when a barbell
/// MaxStrength/Power meet taper (applied per session 0–20 days out) shapes the
/// week; it replaces the calendar deload there. Other goals and calisthenics
/// keep the deload: the taper rules are barbell-meet rules.
fn meet_taper_offset(profile: &Profile, week_start: Option<i64>) -> Option<i64> {
    let tapers = profile.weekly_sets > 0
        && matches!(profile.lift_goal, LiftGoal::MaxStrength | LiftGoal::Power)
        && !calisthenics_mode(profile);
    match (profile.meet_epoch_day, week_start) {
        (Some(meet), Some(ws)) if tapers => {
            Some(meet.saturating_sub(ws)).filter(|d| (0..=20).contains(d))
        }
        _ => None,
    }
}

/// Whether plan week `week_index` (Monday `week_monday`) is the planned lifting
/// deload week `synthesize` lays out: the block's recovery week, unless a meet
/// taper replaces it. Its light sets are prescribed, not a performance
/// decrement, so the e1RM anchors, the last working sets and the e1RM-decline
/// gate leave them out (autoreg-022 reads e1RM at fixed RPE; HYP-DELOAD-RX-001
/// deloads from the working weight).
pub fn lift_deload_week(profile: &Profile, week_index: u32, week_monday: i64) -> bool {
    plan_withheld(profile).is_none()
        && profile.weekly_sets > 0
        && lift_block_position(profile, week_index).recovery
        && meet_taper_offset(profile, Some(week_monday)).is_none()
}

/// The run-week request for this profile's week. Hybrid dosing:
/// - `volume_scale`: when the ≤3-day hybrid default or the chosen weekdays cut
///   run days, weekly km shrinks with them so each run keeps its distance
///   (hybrid-004: shorten, never lengthen, endurance sessions);
/// - lifting-led hybrids hold weekly km steady instead of ramping it
///   (HYB-MAINT-002 maintenance at held intensity; HYB-TENDON-001 progress one
///   side at a time) and, with no race goal, keep a single quality session.
fn run_request(profile: &Profile, shape: &WeekShape, week_index: u32) -> RunWeekRequest {
    let table_days = run::goal_table_run_days(profile);
    let volume_scale = (shape.run_days as f64 / table_days as f64).min(1.0);
    let lift_leads = hybrid_lead(profile) == Some(HybridLead::Lifting);
    RunWeekRequest {
        week_index,
        n_run_days: shape.run_days,
        volume_scale,
        hold_volume: lift_leads,
        max_quality: (lift_leads && profile.goal_distance == GoalDistance::General).then_some(1),
    }
}

/// Why no plan can be built for this profile, `None` when one can: any
/// onboarding gate (HARD RULE 3: the engine never programs through a medical
/// deferral), else a profile that describes neither lifting nor running
/// (nothing to plan). [`synthesize`] returns `None` exactly when this is `Some`.
pub fn plan_withheld(profile: &Profile) -> Option<PlanWithheld> {
    if profile.health.any_gate() {
        Some(PlanWithheld::HealthHold)
    } else if profile.weekly_sets == 0 && profile.running_days_per_week == 0 {
        Some(PlanWithheld::NothingToPlan)
    } else {
        None
    }
}

/// The evidence-cited weekly program for this profile, anchored to logged data.
///
/// Returns `None` whenever [`plan_withheld`] names a reason. `week_index` is
/// the plan week the caller has advanced to (completion-gated, ADH-PROGRESS-001).
pub fn synthesize(profile: &Profile, anchors: &Anchors, week_index: u32) -> Option<Program> {
    if plan_withheld(profile).is_some() {
        return None;
    }
    let ta = training_age_from_cadence(profile.progression_cadence).value;
    let lifting = profile.weekly_sets > 0;
    let running = profile.running_days_per_week > 0;
    let shape = week_shape(profile);

    // ── Run content: kinds + volume for this week (phase-aware, run.rs). ──
    let run_plan = (shape.run_days > 0)
        .then(|| run_week_plan(profile, anchors, &run_request(profile, &shape, week_index)));
    let run_recovery = run_plan.as_ref().is_some_and(|p| p.recovery);

    // ── Lift content: template and per-day emphasis. ──
    // CAP-1 (HYB-CAP-001): with ≥4 run days or ≥40 km a week, at most 2 lift
    // days carry leg work and weekly leg volume is trimmed.
    let lower_cap = if lifting && running {
        run_plan
            .as_ref()
            .and_then(|p| hybrid::lower_lift_cap(shape.run_days, p.weekly_km))
    } else {
        None
    };
    let specs = build_lift_specs(profile, anchors, ta, shape.lift_days, lower_cap.as_ref());

    // ── Placement onto the athlete's weekdays (CAP-3 / 48 h spacing). ──
    let run_kinds: Vec<RunSessionType> = run_plan
        .as_ref()
        .map(|p| p.kinds.clone())
        .unwrap_or_default();
    let placement = place_week(
        available_weekdays(profile),
        &specs,
        &run_kinds,
        run_plan.as_ref().and_then(|p| p.race_day),
        plan_start_day(anchors),
    );

    // ── Lift sessions, then the week-level rules. ──
    let week = LiftWeek {
        profile,
        anchors,
        ta,
        block: lift_block_position(profile, week_index),
        regions: WeekRegions::new(
            &placement
                .lifts
                .iter()
                .map(|(_, s)| s.region)
                .collect::<Vec<_>>(),
        ),
        names: main_lifts(anchors, effective_lift_goal(profile)),
    };
    let mut sessions = build_lift_sessions(&week, &placement.lifts);
    let lift_deload =
        apply_lift_week_modifiers(&week, &mut sessions, lower_cap.as_ref(), run_recovery);

    // ── Run sessions on their placed days. ──
    if let Some(plan) = &run_plan {
        sessions.extend(run_week_sessions(plan, &placement.runs, profile, anchors));
    }
    fill_rest_days(&mut sessions);

    let (goal, name) = program_goal_name(profile, lifting, running);
    let (phase, weeks) = program_phase_weeks(profile, lift_deload, run_plan.as_ref());
    Some(Program {
        name,
        goal,
        mesocycle: Mesocycle {
            phase,
            weeks,
            sessions,
        },
    })
}

/// A lifting profile that trains bodyweight ladders instead of weights.
fn calisthenics_mode(profile: &Profile) -> bool {
    profile.weekly_sets > 0
        && profile.resistance_modality == crate::app::ResistanceModality::Calisthenics
}

/// The lifting goal the week is built for: a runner's supplemental strength
/// block is heavy strength work (RUN-STRENGTH-001), else the athlete's own
/// goal.
fn effective_lift_goal(profile: &Profile) -> LiftGoal {
    if runner_strength_active(profile) {
        LiftGoal::MaxStrength
    } else {
        profile.lift_goal
    }
}

/// The plan's first day as a weekday offset into the week being built (only
/// in the start week): placement keeps a session on it.
fn plan_start_day(anchors: &Anchors) -> Option<u16> {
    match (anchors.plan_start_epoch_day, anchors.week_start_epoch_day) {
        (Some(start), Some(monday)) => u16::try_from(start.saturating_sub(monday))
            .ok()
            .filter(|d| *d < 7),
        _ => None,
    }
}

/// The week's lift days before placement: each day's region (the
/// LIFT-DAYS-001 template, the CAP-1 leg-day cap, a runner's leg-only
/// strength block), its DUP emphasis, and whether it is heavy leg work (the
/// CAP-3 spacing anchor).
fn build_lift_specs(
    profile: &Profile,
    anchors: &Anchors,
    ta: TrainingAge,
    n_days: usize,
    lower_cap: Option<&Recommended<hybrid::LowerLiftCap>>,
) -> Vec<LiftSpec> {
    let runner_strength = runner_strength_active(profile);
    let mut regions = lift_template(n_days);
    if let Some(cap) = lower_cap {
        cap_lower_days(&mut regions, cap.value.max_lower_sessions as usize);
    }
    // A runner's strength block is leg work (squat + hinge) for running
    // economy (HYB-RE-DOSE-001).
    if runner_strength {
        regions = vec![DayRegion::Lower; n_days];
    }
    let calisthenics = calisthenics_mode(profile);
    let power_base = lift::power_needs_strength_base(profile, anchors, ta);
    // Running leads: drop the moderate-load hypertrophy day for a strength or
    // power goal, alternating heavy and power/heavy-volume days (running economy
    // needs ≥80% 1RM or explosive work, HYB-RE-DOSE-001).
    let maintenance_emphasis = hybrid_lead(profile) == Some(HybridLead::Running)
        && matches!(profile.lift_goal, LiftGoal::MaxStrength | LiftGoal::Power);
    regions
        .iter()
        .enumerate()
        .map(|(k, &region)| {
            let emphasis_index = if runner_strength {
                0
            } else if maintenance_emphasis {
                k % 2
            } else {
                region_exposure_index(&regions, k)
            };
            let heavy = runner_strength
                || (!calisthenics
                    && profile.lift_goal != LiftGoal::Hypertrophy
                    && matches!(
                        dup_emphasis(profile.lift_goal, power_base, emphasis_index),
                        DupDay::Heavy | DupDay::HeavyVolume
                    ));
            LiftSpec {
                region,
                emphasis_index,
                heavy_legs: heavy && region.has_lower(),
            }
        })
        .collect()
}

/// How many earlier lift days of the week train the muscles of day `k`: its
/// exposure index for the DUP rotation. DUP undulates each LIFT across its own
/// sessions (File 02 strength-023), so an upper/lower week rotates per region:
/// on U/L/U/L each region gets Heavy then HeavyVolume, and every main lift is
/// lifted at ≥80 % 1RM in ≥2 sessions (STR-HEAVYFREQ-001; File 13 strpow-add-001:
/// lighter days supplement, not replace, the heavy exposures). A full-body day
/// takes the further-along of its two regions; an all-full-body week reads `k`.
fn region_exposure_index(regions: &[DayRegion], k: usize) -> usize {
    let before = &regions[..k];
    let upper = before.iter().filter(|r| r.has_upper()).count();
    let lower = before.iter().filter(|r| r.has_lower()).count();
    match regions[k] {
        DayRegion::Upper => upper,
        DayRegion::Lower => lower,
        DayRegion::Full => upper.max(lower),
    }
}

/// The lift side of one plan week: what the lift-day builder and the week
/// modifiers share.
struct LiftWeek<'a> {
    profile: &'a Profile,
    anchors: &'a Anchors,
    ta: TrainingAge,
    block: BlockPosition,
    /// The placed lift days' regions, in weekday order.
    regions: WeekRegions,
    /// The week's lifts: `main_lifts` for [`effective_lift_goal`].
    names: Vec<String>,
}

impl LiftWeek<'_> {
    /// The builder context for lift day `slot` of a week laid out as
    /// `week_regions`, its muscles trained `n_lift_days` times a week.
    fn block_ctx(&self, n_lift_days: usize, slot: usize, week_regions: WeekRegions) -> BlockCtx {
        BlockCtx {
            n_lift_days,
            week_in_block: self.block.week_in_block,
            block_weeks: self.block.block_weeks,
            slot,
            week_regions,
        }
    }
}

/// One session per placed lift day: a calisthenics ladder day, or a lift day
/// of the region's exercises (a 2-day week and a runner's strength block keep
/// compounds only, Table 1.6).
fn build_lift_sessions(week: &LiftWeek, lifts: &[(u16, LiftSpec)]) -> Vec<Session> {
    let goal = effective_lift_goal(week.profile);
    let goal_profile = (goal != week.profile.lift_goal).then(|| {
        let mut p = week.profile.clone();
        p.lift_goal = goal;
        p
    });
    let lift_profile = goal_profile.as_ref().unwrap_or(week.profile);
    let calisthenics = calisthenics_mode(week.profile);
    let compounds_only = lifts.len() == 2 || runner_strength_active(week.profile);
    let regions = week.regions.as_slice();
    lifts
        .iter()
        .enumerate()
        .map(|(slot, &(day, spec))| {
            let n_exposures = exposures(regions, spec.region);
            if calisthenics {
                calisthenics_session(week.profile, week.anchors, slot, day, n_exposures, regions)
            } else {
                let names = names_for_region(&week.names, spec.region, compounds_only);
                lift_session(
                    lift_profile,
                    week.anchors,
                    week.ta,
                    &names,
                    spec.emphasis_index,
                    day,
                    week.block_ctx(n_exposures, slot, week.regions),
                )
            }
        })
        .collect()
}

/// Apply the week-level rules to the built lift sessions, in order: weekly-set
/// conservation across day counts, the maintenance dose, the CAP-1 leg cap,
/// the meet taper, then the lift deload or, in a running recovery week, the
/// effort floor. Returns whether this is a lift deload week.
fn apply_lift_week_modifiers(
    week: &LiftWeek,
    sessions: &mut [Session],
    lower_cap: Option<&Recommended<hybrid::LowerLiftCap>>,
    run_recovery: bool,
) -> bool {
    let profile = week.profile;
    let lifting = profile.weekly_sets > 0;
    let runner_strength = runner_strength_active(profile);
    let run_led = hybrid_lead(profile) == Some(HybridLead::Running);
    let n_lift = sessions.len();

    // Same weekly sets per exercise whatever the day count (LIFT-DAYS-001).
    // The hypertrophy builder sizes per-muscle weekly volume itself; a
    // maintenance dose replaces the volume target outright.
    let default_n = default_lift_days(profile);
    let default_layout = n_lift == default_n
        && week
            .regions
            .as_slice()
            .iter()
            .all(|r| *r == DayRegion::Full);
    let maintenance = run_led || runner_strength;
    if lifting
        && !calisthenics_mode(profile)
        && profile.lift_goal != LiftGoal::Hypertrophy
        && !maintenance
        && !default_layout
        && n_lift > 0
    {
        let reference: Vec<Session> = (0..default_n)
            .map(|k| {
                lift_session(
                    profile,
                    week.anchors,
                    week.ta,
                    &week.names,
                    k,
                    k as u16,
                    week.block_ctx(default_n, k, WeekRegions::full(default_n)),
                )
            })
            .collect();
        modifiers::conserve_weekly_sets(sessions, &reference);
    }
    if runner_strength {
        modifiers::apply_maintenance_dose(sessions, claims::RUN_STRENGTH_001);
    } else if run_led {
        modifiers::apply_maintenance_dose(sessions, claims::HYB_MAINT_001);
    }
    if let Some(cap) = lower_cap {
        modifiers::apply_lower_cap(sessions, cap);
    }

    // A meet taper (barbell MaxStrength/Power, applied per session 0–14 days
    // out) replaces the calendar deload in the weeks it touches.
    let taper_offset = meet_taper_offset(profile, week.anchors.week_start_epoch_day);
    if let Some(days) = taper_offset {
        lift::apply_meet_taper(sessions, days);
    }
    let lift_deload = lifting && week.block.recovery && taper_offset.is_none();
    if lift_deload {
        modifiers::apply_deload_week(sessions, profile.lift_goal);
    } else if run_recovery && n_lift > 0 {
        modifiers::apply_run_recovery_effort_floor(sessions);
    }
    lift_deload
}

/// Every weekday without a session is a planned rest day (a real session in
/// the week strip); the week is returned in weekday order.
fn fill_rest_days(sessions: &mut Vec<Session>) {
    for d in 0u16..7 {
        if !sessions.iter().any(|s| s.day == d) {
            sessions.push(Session {
                session_type: SessionType::Rest,
                day: d,
                prescriptions: Vec::new(),
                dup_day: None,
            });
        }
    }
    sessions.sort_by_key(|s| s.day);
}

/// The program's phase and block length (the summary's "week N of M"): the
/// lifting block for anyone who lifts (Deload in its deload week, else the run
/// phase, else Build), otherwise the run block.
fn program_phase_weeks(
    profile: &Profile,
    lift_deload: bool,
    run_plan: Option<&RunWeekPlan>,
) -> (MesoPhase, u8) {
    if profile.weekly_sets > 0 {
        let phase = if lift_deload {
            MesoPhase::Deload
        } else {
            run_plan.map_or(MesoPhase::Build, |p| p.phase)
        };
        return (phase, lift_cycle(profile).1);
    }
    run_plan.map_or((MesoPhase::Build, 1), |p| {
        (p.phase, p.cycle_len.clamp(1, u8::MAX as u32) as u8)
    })
}

/// Training-age in years for the running weekly-increase cap. The KB gives no
/// explicit years-per-band mapping, so a novice reads below the 1-year cutoff
/// (larger +10% steps) and everyone else above it (+5%): an expert-opinion split
/// flagged as RUN-PROGRESS-001's novice/other basis, mirroring the taper.
pub(crate) fn training_age_years(ta: TrainingAge) -> f64 {
    match ta {
        TrainingAge::Novice => 0.5,
        _ => 2.0,
    }
}

/// `(load_weeks, cycle_len)` of the block the plan periodizes on: the lifting
/// block for anyone who lifts ([`lift_block_position`]), else the running
/// deload cadence (3:1, or 2:1 for masters / novices, RUN-DELOAD-001).
pub fn cycle_shape(profile: &Profile) -> (u32, u32) {
    if profile.weekly_sets > 0 {
        let (load_weeks, cycle_len) = lift_cycle(profile);
        return (load_weeks.into(), cycle_len.into());
    }
    let load_weeks = run_load_weeks(profile);
    (load_weeks, load_weeks + 1)
}

/// Load weeks of the running deload cadence (3:1, or 2:1 when
/// [`conservative_deload`] holds, RUN-DELOAD-001).
fn run_load_weeks(profile: &Profile) -> u32 {
    let ta = training_age_from_cadence(profile.progression_cadence).value;
    running::deload_cadence(conservative_deload(profile.age_years, ta))
        .value
        .load_weeks as u32
}

/// Whether plan week `week_index` (Monday = `week_monday_epoch_day`) is a
/// planned cut for the weekly running report: the running side's recovery week
/// when the profile runs, else the lifting deload week.
pub fn planned_recovery_week(
    profile: &Profile,
    week_index: u32,
    week_monday_epoch_day: i64,
) -> bool {
    if profile.running_days_per_week > 0 {
        let anchors = Anchors {
            week_start_epoch_day: Some(week_monday_epoch_day),
            ..Default::default()
        };
        let shape = week_shape(profile);
        run_week_plan(profile, &anchors, &run_request(profile, &shape, week_index)).recovery
    } else {
        lift_block_position(profile, week_index).recovery
    }
}

/// The run days the plan schedules and the hardest scheduled run's intensity
/// (%VO2max, the floor of its Daniels band) over the coming 8 weeks (a base
/// and a build running cycle): the inputs to the interference check
/// (HYB-THRESH-001), taken from the athlete's OWN plan instead of a
/// self-reported %VO2max. Reads only the run side's week content (the same
/// `run_week_plan` `synthesize` lays out), never the lift days or placement,
/// so the guidance refresh stays cheap. `None` when the profile is gated or
/// schedules no run.
pub fn plan_run_intensity(profile: &Profile, anchors: &Anchors) -> Option<(u8, f64)> {
    if profile.health.any_gate() || profile.running_days_per_week == 0 {
        return None;
    }
    let shape = week_shape(profile);
    if shape.run_days == 0 {
        return None;
    }
    let mut peak = 0.0_f64;
    for w in 0..8u32 {
        let plan = run_week_plan(profile, anchors, &run_request(profile, &shape, w));
        for &k in &plan.kinds {
            let band = run::vdot_band_for(k);
            peak = peak.max(running::vdot_band_vo2max_pct(band).0);
        }
    }
    Some((shape.run_days.min(u8::MAX as usize) as u8, peak))
}

/// KB masters threshold, years: the NSCA masters bracket begins at 65
/// (`MASTERS-001`, Fragala et al. 2019; File 08 indiv-013/014 "masters (65+)").
const MASTERS_AGE_YEARS: f64 = 65.0;

/// Whether an athlete is a master ([`MASTERS_AGE_YEARS`]+). Operationalizes the
/// "older" half of `RUN-DELOAD-001`'s conservative-cadence basis. No further
/// age branch exists (HARD RULE 1, the KB states no other age-based
/// load/intensity modification).
fn is_masters(age_years: Option<f64>) -> bool {
    age_years.is_some_and(|a| a >= MASTERS_AGE_YEARS)
}

/// Whether the conservative 2:1 running deload cadence applies
/// (`RUN-DELOAD-001`: "2:1 for older / injury-prone / low training-age"):
/// masters age ([`is_masters`]) or a novice training age. Injury-prone is not
/// modeled. Shared by `run_load_weeks` (the plan) and
/// `app/guidance/running.rs` (the card), so the two cadences agree.
pub fn conservative_deload(age_years: Option<f64>, training_age: TrainingAge) -> bool {
    is_masters(age_years) || matches!(training_age, TrainingAge::Novice)
}

/// The top-level goal + program name shown in the plan footer: one short,
/// sentence-case focus name (the shell adds the phase and week).
fn program_goal_name(profile: &Profile, lifting: bool, running: bool) -> (Goal, String) {
    match (lifting, running) {
        (true, true) => {
            let lift = match profile.lift_goal {
                LiftGoal::MaxStrength => "Strength",
                LiftGoal::Power => "Power",
                LiftGoal::Hypertrophy => "Muscle building",
            };
            (Goal::Hybrid, format!("{lift} + running"))
        }
        (true, false) if calisthenics_mode(profile) => {
            match profile.lift_goal {
                LiftGoal::Hypertrophy => (Goal::Hypertrophy, "Calisthenics muscle building".into()),
                // A Power goal leads every day with ballistic jump / explosive
                // push-up ladders (CAL-POWER-001), so the title says so.
                LiftGoal::Power => (Goal::Power, "Calisthenics power".into()),
                LiftGoal::MaxStrength => (Goal::Strength, "Calisthenics strength".into()),
            }
        }
        (true, false) => match profile.lift_goal {
            LiftGoal::MaxStrength => (Goal::Strength, "Strength".into()),
            LiftGoal::Power => (Goal::Power, "Power".into()),
            LiftGoal::Hypertrophy => (Goal::Hypertrophy, "Muscle building".into()),
        },
        (false, true) => match profile.goal_distance {
            GoalDistance::General | GoalDistance::C25k => {
                (Goal::GeneralEndurance, "Running base".into())
            }
            GoalDistance::FiveK => (Goal::RunningRace { distance_km: 5.0 }, "5K plan".into()),
            GoalDistance::TenK => (Goal::RunningRace { distance_km: 10.0 }, "10K plan".into()),
            GoalDistance::HalfMarathon => (
                Goal::RunningRace { distance_km: 21.1 },
                "Half-marathon plan".into(),
            ),
            GoalDistance::Marathon => (
                Goal::RunningRace { distance_km: 42.2 },
                "Marathon plan".into(),
            ),
        },
        (false, false) => (Goal::GeneralEndurance, "Training plan".into()),
    }
}

/// A representative registry claim for the whole program (drives the summary
/// card's evidence chip). Always the dominant branch's own claim.
pub fn summary_claim(profile: &Profile) -> crate::evidence::ClaimId {
    let lifting = profile.weekly_sets > 0;
    let running = profile.running_days_per_week > 0;
    match (lifting, running) {
        (true, _) => {
            if calisthenics_mode(profile) {
                return claims::CAL_PROG_MECHANIC_001;
            }
            let ta = training_age_from_cadence(profile.progression_cadence).value;
            if ta == TrainingAge::Novice {
                match profile.lift_goal {
                    LiftGoal::Power => claims::STR_PWR_001,
                    _ => claims::STR_INTENT_001,
                }
            } else if profile.lift_goal == LiftGoal::Hypertrophy {
                claims::HYP_DEFAULT_PROG_001
            } else {
                claims::STR_DUP_001
            }
        }
        (false, true) => claims::RUN_WORKOUT_001,
        (false, false) => claims::HYP_MESO_STRUCT_001,
    }
}

#[cfg(test)]
mod tests;
