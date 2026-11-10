//! The logged facts a plan week is anchored to: lift e1RMs, derates and last
//! working sets, the run history, calisthenics ladder state and the dates
//! that place the week.

use crate::calisthenics::progression::CalisAnchor;
use crate::exercise::movement_key;
use crate::schema::LadderId;

/// What the athlete's logged history says, for one plan week: turns the %1RM
/// bands into real kg and lets the plan lead with the user's OWN exercises (a
/// logged lift carries an e1RM anchor and is prescribed by load; anything
/// unlogged falls back to RIR), and sizes the run week from what was run.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Anchors {
    pub(crate) lift: LiftAnchors,
    pub(crate) run: RunAnchors,
    /// `true` while the profile's `stated_longest_recent_km` is inside its own
    /// 30-day window (dated by the profile edit that first stated it). The
    /// stated run then still IS a run of the last 30 days, so the long-run
    /// baseline is the longer of it and `run.longest_recent_run_km`
    /// (RUN-START-001 / RUN-SPIKE-001); past the window a logged run
    /// supersedes it.
    pub(crate) stated_longest_in_window: bool,
    /// Per-ladder calisthenics tier state replayed from logged sets (empty for
    /// a `Weights` profile). A dynamic ladder with no logged rung is simply
    /// absent (the plan starts it at rung 0); a skill track is absent until a
    /// rung is logged (opt-in, never force-fed).
    pub(crate) calisthenics: Vec<CalisAnchor>,
    /// Pace anchor: the athlete's current race fitness (Daniels VDOT) from a
    /// recent race no older than ~8 weeks (running-007 / running-041,
    /// RUN-RETEST-001). Training paces per band come from
    /// `running::vdot_band_pace_sec_per_km`. `None` = no fresh race: runs are
    /// prescribed by heart rate / effort only, never an invented pace.
    pub(crate) run_vdot: Option<f64>,
    /// Local epoch day of the Monday of the week being synthesized. Dates the
    /// week against `Profile::goal_race_epoch_day` (race-date periodization,
    /// RUN-PERIOD-001) and the strength meet date. `None` = undated (no
    /// race-date phases).
    pub(crate) week_start_epoch_day: Option<i64>,
    /// Logged running km per COMPLETED calendar week before
    /// `week_start_epoch_day`, most-recent-first (index 0 = last week), up to
    /// 12 weeks. A measured fact that carries volume progression across run
    /// cycles (RUN-VOLUME-001): the next cycle starts from the load the
    /// athlete actually reached.
    pub(crate) run_week_km: Vec<f64>,
    /// Local epoch day the plan started (`GeneratePlan`). In the start week
    /// that weekday holds a session when the athlete can train on it, so a new
    /// plan never opens on a rest day.
    pub(crate) plan_start_epoch_day: Option<i64>,
    /// The athlete's latest undeclared break from running (a week or more
    /// with no logged run) while it still shapes the plan (its re-entry ramp
    /// runs, or a run from before it still carries long-run credit). `None`
    /// without one, or when the stated weekly distance was entered after the
    /// break began (the athlete's current answer then governs). Read by
    /// `run::effective_weekly_km` (the File 08 Table 3.4b running re-entry,
    /// RUN-REENTRY-001), `run::base_weekly_km` (pre-break weeks never carry
    /// the cycle's load level) and `run::day_ctx` (the long-run and spike
    /// baselines).
    pub(crate) run_break: Option<RunBreak>,
}

impl Anchors {
    /// The anchors of a week judged from the profile alone: dated
    /// `week_start_epoch_day`, with at most the measured weekly running volume
    /// known. Every other logged fact is absent.
    pub(super) fn profile_only(
        week_start_epoch_day: Option<i64>,
        recent_weekly_km: Option<f64>,
    ) -> Self {
        Anchors {
            lift: LiftAnchors::default(),
            run: RunAnchors {
                longest_recent_run_km: None,
                recent_weekly_km,
                longest_run_detrained: false,
            },
            stated_longest_in_window: false,
            calisthenics: Vec::new(),
            run_vdot: None,
            week_start_epoch_day,
            run_week_km: Vec::new(),
            plan_start_epoch_day: None,
            run_break: None,
        }
    }

    /// These anchors as seen from the Monday `back` weeks earlier: the week
    /// start moves back and the weekly-km history starts `back` weeks later.
    pub(super) fn weeks_back(&self, back: u32) -> Self {
        Anchors {
            lift: self.lift.clone(),
            run: self.run,
            stated_longest_in_window: self.stated_longest_in_window,
            calisthenics: self.calisthenics.clone(),
            run_vdot: self.run_vdot,
            week_start_epoch_day: self.week_start_epoch_day.map(|d| d - 7 * i64::from(back)),
            run_week_km: self
                .run_week_km
                .get(back as usize..)
                .unwrap_or_default()
                .to_vec(),
            plan_start_epoch_day: self.plan_start_epoch_day,
            run_break: self.run_break,
        }
    }

    /// The derived anchor for a ladder, if one exists.
    pub fn calis_for(&self, ladder: LadderId) -> Option<&CalisAnchor> {
        self.calisthenics.iter().find(|a| a.ladder == ladder)
    }
}

/// The logged-lift anchors of a plan week.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiftAnchors {
    /// (exercise name, e1RM kg), most-recent-first, one entry per MOVEMENT
    /// (`movement_key`: "Back squat" and "Barbell back squat" share one anchor).
    /// The e1RM is the best estimate within the movement's most recent window
    /// (last 3 sessions or 21 days), never the all-time best, and is
    /// the logged value itself: a working-load derate (detraining, layoff ramp,
    /// e1RM drop) lives in `derates`. A lift untrained for >8 weeks has no
    /// entry (falls back to RIR).
    pub(crate) e1rm: Vec<(String, f64)>,
    /// Per-lift working-load derates (at most one per movement), applied to the
    /// %1RM LOAD and to a carried last-working load: detraining credit after
    /// 31–56 days without the movement (DETRAIN-STR-001 via the REENTRY-001
    /// bracket), the re-entry ramp for pre-break numbers (REENTRY-RAMP-001)
    /// after a declared layoff, a week or more without any lifting, or an
    /// undeclared 31–56-day gap in the movement's own history, or a 5 % cut
    /// after the latest session's e1RM fell ≥5 % below baseline
    /// (AUTOREG-E1RM-GATE-001). Never stacked: one derate per movement.
    pub(crate) derates: Vec<LiftDerate>,
    /// `true` while a declared >8 wk layoff's re-entry window is running (KB
    /// Table 3.4b: no load fraction, treat as a fresh novice). Pre-layoff anchors
    /// are dropped from `e1rm` during the window; a movement logged since the
    /// return anchors normally. Cleared once the ramp window ends.
    pub(crate) reentry_novice: bool,
    /// The athlete's LAST working sets per movement (one entry per movement,
    /// most-recent-first), from the newest logged session of that movement
    /// within the detraining horizon. Unlike `e1rm` it covers isolation and
    /// bodyweight lifts too, so a logged curl stays in the plan. Read by:
    /// - hypertrophy double progression (HYP-DOUBLEPROG-001);
    /// - `lift::earned_week` (a missed rep withholds the STR-PCTPROG-001 step);
    /// - `lift::novice_jump_cap_pct` (the STR-1RMTEST-001 novice step cap);
    /// - [`LiftAnchors::carried_derate_for`] and the carried load.
    pub(crate) last_working: Vec<LastWorking>,
}

/// The measured running history of a plan week.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RunAnchors {
    /// Longest completed run (km) in the trailing 30-day RUN-SPIKE-001 window
    /// (`spike_baseline_km`'s window and predicate, so a long run at this
    /// value is no spike); an older run counts at its detraining credit.
    /// `None` when no run carries credit.
    pub(crate) longest_recent_run_km: Option<f64>,
    /// Measured weekly running volume (km): trailing-28-day total ÷ the 1–4
    /// weeks the window spans since its first run; `None` with no run in the
    /// window. Read by `run::effective_weekly_km`, which sizes the whole run
    /// week.
    pub(crate) recent_weekly_km: Option<f64>,
    /// `true` when `longest_recent_run_km` comes from a run older than the
    /// 30-day window, tapered per DETRAIN-001. A long run raised by it cites
    /// DETRAIN-001 instead of RUN-SPIKE-001.
    pub(crate) longest_run_detrained: bool,
}

/// An undeclared break from running (see [`Anchors::run_break`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunBreak {
    /// Weeks fully off running: from the last run before the break to the
    /// first run after it, or to the day being prescribed while it runs.
    pub(crate) weeks_off: f64,
    /// Local day of the last run before the break.
    pub(crate) last_run_epoch_day: i64,
    /// The measured weekly volume (28-day average) as of that last run: the
    /// prior volume the re-entry fraction scales, with the stated one.
    pub(crate) prior_weekly_km: Option<f64>,
    /// The RUN-REENTRY-001 bracket as of the day being prescribed (restart
    /// fraction on its ramp, rebuild-long-run and near-novice spike flags).
    /// `rebuild_long_run` / `near_novice_spike` hold only while a run from
    /// before the break would still carry long-run credit (DETRAIN-001).
    pub(crate) reentry: crate::individualization::RunReEntry,
    /// Longest run since the break, detraining-adjusted like
    /// `RunAnchors::longest_recent_run_km` but over runs since the return only;
    /// `None` when none was logged yet. After 2+ weeks off it replaces the
    /// pre-break longest as the demonstrated long-run capacity ("rebuild long
    /// run"); after 4+ weeks also as every run's spike baseline.
    pub(crate) longest_since_km: Option<f64>,
    /// `true` when `longest_since_km` is a detraining-tapered value (its run
    /// is older than the 30-day full-credit window, DETRAIN-001).
    pub(crate) longest_since_detrained: bool,
    /// The stated longest run was entered after the break began (a current
    /// answer, RUN-START-001).
    pub(crate) stated_longest_after: bool,
}

/// One movement's last logged working sets (see [`LiftAnchors::last_working`]).
#[derive(Debug, Clone, PartialEq)]
pub struct LastWorking {
    /// Display name as last logged.
    pub(crate) exercise: String,
    /// The session's top working load in kg (0.0 for a bodyweight set).
    pub(crate) weight_kg: f64,
    /// `(reps, rpe)` of every set logged at `weight_kg` that session, in log
    /// order (rpe 0.0 = unrated).
    pub(crate) sets: Vec<(u32, f64)>,
    /// Whole days from that session to the anchor's reference day.
    pub(crate) age_days: i64,
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

/// One lift's working-load derate (see [`LiftAnchors::derates`]).
#[derive(Debug, Clone, PartialEq)]
pub struct LiftDerate {
    pub(crate) exercise: String,
    /// Fraction of the logged e1RM the working load is computed from (< 1).
    pub(crate) frac: f64,
    pub(crate) cause: DerateCause,
}

impl LiftAnchors {
    /// The last working sets of the same movement as `exercise`
    /// (`same_movement`-normalized), if any were logged.
    pub fn last_working_for(&self, exercise: &str) -> Option<&LastWorking> {
        let key = movement_key(exercise);
        self.last_working
            .iter()
            .find(|w| movement_key(&w.exercise) == key)
    }

    /// The logged e1RM anchor for an exercise, if any: an exact
    /// (case-insensitive) name first, else the same MOVEMENT (`same_movement`),
    /// so naming variants share one anchor. Only a POSITIVE e1RM counts; a
    /// 0/negative value never anchors a load (it would render "@ 0 kg").
    pub fn e1rm_for(&self, exercise: &str) -> Option<f64> {
        self.e1rm
            .iter()
            .find(|(n, _)| crate::names::fold_eq(n, exercise))
            .or_else(|| {
                let key = movement_key(exercise);
                self.e1rm.iter().find(|(n, _)| movement_key(n) == key)
            })
            .map(|(_, v)| *v)
            .filter(|v| *v > 0.0)
    }

    /// The working-load derate for an exercise's movement, if one applies.
    pub fn load_frac_for(&self, exercise: &str) -> Option<&LiftDerate> {
        let key = movement_key(exercise);
        self.derates.iter().find(|d| {
            crate::names::fold_eq(&d.exercise, exercise) || movement_key(&d.exercise) == key
        })
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
    pub(super) fn working_e1rm_for(&self, exercise: &str) -> Option<f64> {
        self.e1rm_for(exercise)
            .map(|e| e * self.load_frac_for(exercise).map_or(1.0, |d| d.frac))
    }
}
