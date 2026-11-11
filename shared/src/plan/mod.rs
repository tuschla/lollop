//! Program synthesis: compose the graded band functions (`strength::dup_day_rx`,
//! `strength::loading_rx`, `running::workout_rx`, `running::goal_week_plan`)
//! into a dated [`Program`] of `Recommended<Prescription>` for ONE Mon..Sun
//! week at 0-based `day` offsets. `app/` maps the
//! offsets to calendar days and applies readiness/safety downstream.
//!
//! 1. Shape: lift and run day counts on the athlete's weekdays (LIFT-DAYS-001,
//!    ADH-DAYS-001, HYB-RUNCAP-001).
//! 2. Content: run kinds and volume, lift-day template, hybrid dosing
//!    (HYB-MAINT-001/002, HYB-TENDON-001).
//! 3. Placement: `schedule::place_week` (HYB-CAP-001, RUN-QUALITY-001).
//! 4. Block position: lift deload every 5th–6th week (HYP-DELOAD-CADENCE-001,
//!    STR-DELOAD-VOL-001) or the meet taper.

mod anchors;
mod calis;
mod day;
mod hypertrophy;
mod lift;
mod modifiers;
mod regions;
mod run;
mod schedule;
mod volume;

use crate::evidence::claims;
use crate::hybrid;
use crate::individualization::TrainingAge;
use crate::profile::{HybridLead, PlanProfile};
use crate::running::{self, GoalDistance};
use crate::schema::{Goal, MesoPhase, Mesocycle, Program, Recommended, Session, SessionType};
use crate::strength::{self, LiftGoal};

use calis::calisthenics_session;
use day::LiftDay;
use hypertrophy::{HypertrophyWeek, double_progression};
use lift::{dup_emphasis, full_body_weekly_sets, lift_session, main_lifts};
use regions::{WeekRegions, cap_lower_days, exposures, names_for_region, region_exposure_index};
use run::{RunWeekPlan, RunWeekRequest, run_week_plan, run_week_sessions};
use schedule::{LiftSpec, place_week};

pub use anchors::{
    Anchors, DerateCause, LastWorking, LiftAnchors, LiftDerate, RunAnchors, RunBreak,
};
pub(crate) use hypertrophy::{
    hypertrophy_block_targets, hypertrophy_week_targets, uses_double_progression,
};
pub(crate) use regions::{DayRegion, lift_template};
pub use run::prescribed_run_km;
pub(crate) use schedule::{
    DayLoad, MoveBlock, MoveConflict, SAME_MUSCLE_GAP_H, SpacingRule, move_conflict, move_options,
    session_load,
};

/// Lifting accumulation block length: 5 load weeks before a deload week
/// (HYP-DELOAD-CADENCE-001: deload about every 4–6 weeks, prefer 5).
const LIFT_LOAD_WEEKS: u8 = 5;

/// Length of the LIFTING block, independent of any running cadence (a hybrid
/// lifter does not inherit the 3:1 running deload): the load weeks plus a
/// deload week. Novices on linear progression get no calendar deload (the
/// cycle is the load weeks alone): their deloads come from the review
/// triggers (stalls / effort gaps) instead of the calendar (STR-DELOAD-VOL-001,
/// HYP-DELOAD-CADENCE-001 "deload earlier only on fatigue signs").
fn lift_cycle_len(pp: &PlanProfile) -> u8 {
    if pp.training_age() == TrainingAge::Novice {
        LIFT_LOAD_WEEKS
    } else {
        LIFT_LOAD_WEEKS + 1
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
/// "progress one side, hold the other"). A linear novice (no calendar deload)
/// keeps counting weeks; `strength::pct_for_week` holds the band cap
/// (STR-PCTPROG-001).
pub fn lift_block_position(pp: &PlanProfile, week_index: u32) -> BlockPosition {
    let block_weeks = LIFT_LOAD_WEEKS;
    let cycle_len = lift_cycle_len(pp);
    let pos = (week_index % u32::from(cycle_len)) as u8;
    let recovery = pos == block_weeks;
    let linear = cycle_len == block_weeks && !uses_double_progression(pp);
    let week_in_block = if pp.hybrid_lead() == Some(HybridLead::Running) {
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

/// Whether plan week `week_index` is the last load week before the planned
/// recovery week of the block the plan periodizes on: the lifting block for
/// anyone who lifts ([`lift_block_position`]), else the running deload
/// cadence (3:1, or 2:1 for masters / novices, RUN-DELOAD-001). A block of
/// fewer than two load weeks has no such peak week.
pub fn last_load_week(pp: &PlanProfile, week_index: u32) -> bool {
    let (load_weeks, cycle_len) = if pp.lifts() {
        (u32::from(LIFT_LOAD_WEEKS), u32::from(lift_cycle_len(pp)))
    } else {
        let load_weeks = pp.run_load_weeks();
        (load_weeks, load_weeks + 1)
    };
    load_weeks >= 2 && cycle_len > 0 && week_index % cycle_len == load_weeks - 1
}

/// Days from the week's Monday `week_start` to the meet when a barbell
/// MaxStrength/Power meet taper reaches one of the week's lift sessions
/// (`lift_days`, 0 = Monday): [`modifiers::apply_meet_taper`] shapes sessions
/// 0–14 days out. Such a week takes the taper in place of the calendar deload; a
/// week the taper misses keeps its deload. Other goals and calisthenics keep
/// the deload: the taper rules are barbell-meet rules. Reads the STATED lift
/// goal.
fn meet_taper_offset(
    pp: &PlanProfile,
    week_start: Option<i64>,
    lift_days: impl FnOnce() -> Vec<u16>,
) -> Option<i64> {
    let profile = pp.profile();
    let tapers = pp.lifts()
        && matches!(profile.lift_goal, LiftGoal::MaxStrength | LiftGoal::Power)
        && !pp.plans_calisthenics();
    let offset = profile
        .meet_epoch_day
        .zip(week_start)
        .filter(|_| tapers)
        .map(|(meet, ws)| meet.saturating_sub(ws))?;
    modifiers::meet_taper_reaches(offset, &lift_days()).then_some(offset)
}

/// Whether a week at lifting block position `recovery` is the planned lifting
/// deload: a lifting profile's recovery week that no meet taper replaces
/// (`taper_offset`, [`meet_taper_offset`]).
fn is_lift_deload(pp: &PlanProfile, recovery: bool, taper_offset: Option<i64>) -> bool {
    pp.lifts() && recovery && taper_offset.is_none()
}

/// Whether plan week `week_index` (Monday `week_monday`) is the planned lifting
/// deload week `synthesize` lays out ([`is_lift_deload`]; the lift days a meet
/// taper is judged on are the week placed from the profile alone,
/// [`Anchors::profile_only`]). Its light sets are prescribed, not a
/// performance decrement, so the e1RM anchors, the last working sets and the
/// e1RM-decline gate leave them out (autoreg-022 reads e1RM at fixed RPE;
/// HYP-DELOAD-RX-001 deloads from the working weight, 03-hypertrophy.md:395;
/// HYP-DOUBLEPROG-001, :361-362).
pub fn lift_deload_week(pp: &PlanProfile, week_index: u32, week_monday: i64) -> bool {
    if pp.withheld().is_some() || !lift_block_position(pp, week_index).recovery {
        return false;
    }
    let taper_offset = meet_taper_offset(pp, Some(week_monday), || {
        let anchors = Anchors::profile_only(Some(week_monday), None);
        let run = (pp.shape().run_days > 0).then(|| run_plan(pp, &anchors, week_index));
        let (_, placement) = place(pp, &anchors, run.as_ref());
        placement.lifts.iter().map(|(d, _)| *d).collect()
    });
    is_lift_deload(pp, true, taper_offset)
}

/// The run-week request for this profile's week. Hybrid dosing:
/// - `volume_scale`: when the ≤3-day hybrid default or the chosen weekdays cut
///   run days, weekly km shrinks with them so each run keeps its distance
///   (hybrid-004: shorten, never lengthen, endurance sessions);
/// - lifting-led hybrids hold weekly km steady instead of ramping it
///   (HYB-MAINT-002 maintenance at held intensity; HYB-TENDON-001 progress one
///   side at a time) and, with no race goal, keep a single quality session.
fn run_request(pp: &PlanProfile, week_index: u32) -> RunWeekRequest {
    let run_days = pp.shape().run_days;
    let volume_scale = (run_days as f64 / pp.goal_table_run_days() as f64).min(1.0);
    let lift_leads = pp.hybrid_lead() == Some(HybridLead::Lifting);
    RunWeekRequest {
        week_index,
        n_run_days: run_days,
        volume_scale,
        hold_volume: lift_leads,
        max_quality: (lift_leads && pp.profile().goal_distance == GoalDistance::General)
            .then_some(1),
    }
}

/// The run side of plan week `week_index` ([`run_week_plan`]). A taper week
/// cuts from the last full load week as it was prescribed: that week, `k`
/// weeks back, is replayed through the same plan, lift days and placement
/// ([`load_week_km`]).
fn run_plan(pp: &PlanProfile, anchors: &Anchors, week_index: u32) -> RunWeekPlan {
    run_week_plan(pp, anchors, &run_request(pp, week_index), &|k| {
        load_week_km(pp, anchors, week_index, k)
    })
}

/// The run km prescribed in plan week `week_index − back` (Monday `back`
/// weeks earlier, the weekly-km history seen from there). Only taper weeks
/// ask, for their last load week, which is no taper, so this recurses once.
fn load_week_km(pp: &PlanProfile, anchors: &Anchors, week_index: u32, back: u32) -> f64 {
    let anchors = anchors.weeks_back(back);
    let plan = run_plan(pp, &anchors, week_index.saturating_sub(back));
    let (_, placement) = place(pp, &anchors, Some(&plan));
    prescribed_run_km(
        &run_week_sessions(&plan, &placement.runs, pp.profile(), &anchors),
        &anchors,
    )
}

/// The week's lift days and run kinds on the athlete's weekdays (CAP-3 / 48 h
/// spacing), with the CAP-1 leg cap (HYB-CAP-001: ≥4 run days or ≥40 km a
/// week leave at most 2 lift days with leg work and trim weekly leg volume)
/// the run week triggers.
fn place(
    pp: &PlanProfile,
    anchors: &Anchors,
    run_plan: Option<&RunWeekPlan>,
) -> (
    Option<Recommended<hybrid::LowerLiftCap>>,
    schedule::Placement,
) {
    let shape = pp.shape();
    let lower_cap = run_plan
        .filter(|_| pp.lifts())
        .and_then(|p| hybrid::lower_lift_cap(shape.run_days, p.weekly_km));
    let specs = build_lift_specs(pp, anchors, shape.lift_days, lower_cap.as_ref());
    let run_kinds = run_plan.map(|p| p.kinds.as_slice()).unwrap_or_default();
    let placement = place_week(
        pp.available_weekdays(),
        &specs,
        run_kinds,
        run_plan.and_then(|p| p.race_day),
        plan_start_day(anchors),
    );
    (lower_cap, placement)
}

/// The evidence-cited weekly program for this profile, anchored to logged data.
///
/// Returns `None` whenever [`PlanProfile::withheld`] names a reason.
/// `week_index` is the plan week the caller has advanced to
/// (completion-gated, ADH-PROGRESS-001).
pub fn synthesize(pp: &PlanProfile, anchors: &Anchors, week_index: u32) -> Option<Program> {
    if pp.withheld().is_some() {
        return None;
    }

    let run_plan = (pp.shape().run_days > 0).then(|| run_plan(pp, anchors, week_index));
    let run_recovery = run_plan.as_ref().is_some_and(|p| p.recovery);
    let (lower_cap, placement) = place(pp, anchors, run_plan.as_ref());

    let week = LiftWeek {
        pp,
        anchors,
        block: lift_block_position(pp, week_index),
        regions: WeekRegions::new(
            &placement
                .lifts
                .iter()
                .map(|(_, s)| s.region)
                .collect::<Vec<_>>(),
        ),
        names: main_lifts(&anchors.lift, pp.effective_lift_goal()),
    };
    let mut sessions = build_lift_sessions(&week, &placement.lifts);
    let lift_deload =
        apply_lift_week_modifiers(&week, &mut sessions, lower_cap.as_ref(), run_recovery);

    if let Some(plan) = &run_plan {
        sessions.extend(run_week_sessions(
            plan,
            &placement.runs,
            pp.profile(),
            anchors,
        ));
    }
    fill_rest_days(&mut sessions);

    let (phase, weeks) = program_phase_weeks(pp, lift_deload, run_plan.as_ref());
    Some(Program {
        goal: ProgramFocus::of(pp).goal(),
        mesocycle: Mesocycle {
            phase,
            weeks,
            sessions,
        },
    })
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

/// How a week's lift days are laid out: their regions and their DUP
/// rotation, chosen once per profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiftLayout {
    /// A runner's strength block: every day leg work (squat + hinge) for
    /// running economy, at the first DUP emphasis (HYB-RE-DOSE-001).
    RunnerStrength,
    /// Running leads a strength or power goal: the moderate-load hypertrophy
    /// day is dropped, heavy and power/heavy-volume days alternate (running
    /// economy needs ≥80% 1RM or explosive work, HYB-RE-DOSE-001).
    RunLedAlternating,
    /// The LIFT-DAYS-001 template under the CAP-1 leg-day cap, each region
    /// rotating through the DUP emphases on its own exposures.
    Template,
}

impl LiftLayout {
    /// The layout for this profile. Reads the STATED lift goal.
    fn of(pp: &PlanProfile) -> Self {
        if pp.runner_strength() {
            LiftLayout::RunnerStrength
        } else if pp.hybrid_lead() == Some(HybridLead::Running)
            && matches!(
                pp.profile().lift_goal,
                LiftGoal::MaxStrength | LiftGoal::Power
            )
        {
            LiftLayout::RunLedAlternating
        } else {
            LiftLayout::Template
        }
    }

    /// The regions of `n_days` lift days, in canonical order.
    fn regions(
        self,
        n_days: usize,
        lower_cap: Option<&Recommended<hybrid::LowerLiftCap>>,
    ) -> Vec<DayRegion> {
        if self == LiftLayout::RunnerStrength {
            return vec![DayRegion::Lower; n_days];
        }
        let mut regions = lift_template(n_days);
        if let Some(cap) = lower_cap {
            cap_lower_days(&mut regions, cap.value.max_lower_sessions as usize);
        }
        regions
    }

    /// Day `k`'s index into the `dup_emphasis` rotation.
    fn emphasis_index(self, regions: &[DayRegion], k: usize) -> usize {
        match self {
            LiftLayout::RunnerStrength => 0,
            LiftLayout::RunLedAlternating => k % 2,
            LiftLayout::Template => region_exposure_index(regions, k),
        }
    }
}

/// The week's lift days before placement: each day's region and DUP
/// emphasis ([`LiftLayout`]), and whether it is heavy leg work (the CAP-3
/// spacing anchor). The emphasis reads the STATED lift goal.
fn build_lift_specs(
    pp: &PlanProfile,
    anchors: &Anchors,
    n_days: usize,
    lower_cap: Option<&Recommended<hybrid::LowerLiftCap>>,
) -> Vec<LiftSpec> {
    let stated_goal = pp.profile().lift_goal;
    let layout = LiftLayout::of(pp);
    let regions = layout.regions(n_days, lower_cap);
    let calisthenics = pp.plans_calisthenics();
    let power_base = lift::power_needs_strength_base(
        stated_goal,
        pp.profile(),
        &anchors.lift,
        pp.training_age(),
    );
    regions
        .iter()
        .enumerate()
        .map(|(k, &region)| {
            let emphasis_index = layout.emphasis_index(&regions, k);
            let heavy = layout == LiftLayout::RunnerStrength
                || (!calisthenics
                    && stated_goal != LiftGoal::Hypertrophy
                    && strength::is_heavy_dup_day(dup_emphasis(
                        stated_goal,
                        power_base,
                        emphasis_index,
                    )));
            LiftSpec {
                region,
                emphasis_index,
                heavy_legs: heavy && region.has_lower(),
            }
        })
        .collect()
}

/// The lift side of one plan week: what the lift-day builders and the week
/// modifiers share.
struct LiftWeek<'a> {
    pp: &'a PlanProfile<'a>,
    anchors: &'a Anchors,
    block: BlockPosition,
    /// The placed lift days' regions, in weekday order.
    regions: WeekRegions,
    /// The week's lifts: `main_lifts` for the effective lift goal.
    names: Vec<String>,
}

impl LiftWeek<'_> {
    /// The request for placed lift day `slot` on weekday `day`.
    fn day(&self, day: u16, slot: usize, region: DayRegion) -> LiftDay {
        LiftDay {
            day,
            slot,
            n_exposures: exposures(self.regions.as_slice(), region),
            week_in_block: self.block.week_in_block,
            block_weeks: self.block.block_weeks,
            regions: self.regions,
        }
    }
}

/// One session per placed lift day, from the builder the profile trains
/// with: calisthenics ladder days, the hypertrophy week (allocated once), or
/// DUP lift days of the region's exercises for the EFFECTIVE lift goal (a
/// 2-day week and a runner's strength block keep compounds only, Table 1.6).
fn build_lift_sessions(week: &LiftWeek, lifts: &[(u16, LiftSpec)]) -> Vec<Session> {
    let pp = week.pp;
    if lifts.is_empty() {
        return Vec::new();
    }
    let days = lifts
        .iter()
        .enumerate()
        .map(|(slot, &(day, spec))| (week.day(day, slot, spec.region), spec));
    let effective_goal = pp.effective_lift_goal();
    if pp.plans_calisthenics() {
        days.map(|(day, _)| calisthenics_session(pp, week.anchors, &day))
            .collect()
    } else if double_progression(effective_goal, pp.profile()) {
        let hypertrophy_week = HypertrophyWeek::new(
            pp,
            &week.anchors.lift,
            week.block.week_in_block,
            week.block.block_weeks,
            week.regions.as_slice(),
        );
        days.map(|(day, _)| hypertrophy_week.session(&day))
            .collect()
    } else {
        let compounds_only = lifts.len() == 2 || pp.runner_strength();
        days.map(|(day, spec)| {
            let names = names_for_region(&week.names, spec.region, compounds_only);
            lift_session(
                pp,
                effective_goal,
                &week.anchors.lift,
                &names,
                spec.emphasis_index,
                &day,
            )
        })
        .collect()
    }
}

/// Apply the week-level rules to the built lift sessions, in order: weekly-set
/// conservation across day counts, the maintenance dose, the CAP-1 leg cap,
/// the meet taper, then the lift deload or, in a running recovery week, the
/// effort floor. Returns whether this is a lift deload week. The
/// conservation targets and the deload read the STATED lift goal.
fn apply_lift_week_modifiers(
    week: &LiftWeek,
    sessions: &mut [Session],
    lower_cap: Option<&Recommended<hybrid::LowerLiftCap>>,
    run_recovery: bool,
) -> bool {
    let pp = week.pp;
    let stated_goal = pp.profile().lift_goal;
    let runner_strength = pp.runner_strength();
    let run_led = pp.hybrid_lead() == Some(HybridLead::Running);
    let n_lift = sessions.len();

    // Same weekly sets per exercise whatever the day count (LIFT-DAYS-001).
    // The hypertrophy builder sizes per-muscle weekly volume itself; a
    // maintenance dose replaces the volume target outright.
    let default_n = pp.default_lift_days();
    let default_layout = n_lift == default_n
        && week
            .regions
            .as_slice()
            .iter()
            .all(|r| *r == DayRegion::Full);
    let maintenance = run_led || runner_strength;
    if pp.lifts()
        && !pp.plans_calisthenics()
        && stated_goal != LiftGoal::Hypertrophy
        && !maintenance
        && !default_layout
        && n_lift > 0
    {
        let targets = full_body_weekly_sets(
            pp,
            stated_goal,
            &week.anchors.lift,
            &week.names,
            default_n,
            week.block.week_in_block,
            week.block.block_weeks,
        );
        modifiers::conserve_weekly_sets(sessions, &targets);
    }
    if runner_strength {
        modifiers::apply_maintenance_dose(sessions, claims::RUN_STRENGTH_001);
    } else if run_led {
        modifiers::apply_maintenance_dose(sessions, claims::HYB_MAINT_001);
    }
    if let Some(cap) = lower_cap {
        modifiers::apply_lower_cap(sessions, cap);
    }

    let taper_offset = meet_taper_offset(pp, week.anchors.week_start_epoch_day, || {
        sessions
            .iter()
            .filter(|s| matches!(s.session_type, SessionType::Lift(_)))
            .map(|s| s.day)
            .collect()
    });
    if let Some(days) = taper_offset {
        modifiers::apply_meet_taper(sessions, days);
    }
    let lift_deload = is_lift_deload(pp, week.block.recovery, taper_offset);
    if lift_deload {
        modifiers::apply_deload_week(sessions, stated_goal);
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
    pp: &PlanProfile,
    lift_deload: bool,
    run_plan: Option<&RunWeekPlan>,
) -> (MesoPhase, u8) {
    if pp.lifts() {
        let phase = if lift_deload {
            MesoPhase::Deload
        } else {
            run_plan.map_or(MesoPhase::Build, |p| p.phase)
        };
        return (phase, lift_cycle_len(pp));
    }
    run_plan.map_or((MesoPhase::Build, 1), |p| {
        (p.phase, p.cycle_len.clamp(1, u8::MAX as u32) as u8)
    })
}

/// Whether plan week `week_index` (Monday = `week_monday_epoch_day`) is a
/// planned cut for the weekly running report: the running side's recovery week
/// when the profile runs (the run week placed from the profile alone,
/// [`Anchors::profile_only`]), else the lifting deload week
/// ([`lift_deload_week`]).
pub fn planned_recovery_week(
    pp: &PlanProfile,
    week_index: u32,
    week_monday_epoch_day: i64,
) -> bool {
    if pp.runs() {
        let anchors = Anchors::profile_only(Some(week_monday_epoch_day), None);
        run_plan(pp, &anchors, week_index).recovery
    } else {
        lift_deload_week(pp, week_index, week_monday_epoch_day)
    }
}

/// The run days the plan schedules and the hardest scheduled run's intensity
/// (%VO2max, the floor of its Daniels band) over the coming 8 weeks (a base
/// and a build running cycle): the inputs to the interference check
/// (HYB-THRESH-001), taken from the athlete's OWN plan instead of a
/// self-reported %VO2max. Reads the run side's week content (the same
/// [`run_plan`] `synthesize` lays out), sized from the measured
/// `weekly_km` alone ([`Anchors::profile_only`], undated). `None` when no plan
/// is built or it schedules no run.
pub fn plan_run_intensity(pp: &PlanProfile, weekly_km: f64) -> Option<(u8, f64)> {
    if pp.withheld().is_some() || !pp.runs() {
        return None;
    }
    let shape = pp.shape();
    if shape.run_days == 0 {
        return None;
    }
    let anchors = Anchors::profile_only(None, Some(weekly_km));
    let mut peak = 0.0_f64;
    for w in 0..8u32 {
        let plan = run_plan(pp, &anchors, w);
        for &k in &plan.kinds {
            let band = run::vdot_band_for(k);
            peak = peak.max(running::vdot_band_vo2max_pct(band).0);
        }
    }
    Some((shape.run_days.min(u8::MAX as usize) as u8, peak))
}

/// What a program trains: the typed focus behind its title and its
/// top-level goal (the app renders the words).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgramFocus {
    /// Lifting and running.
    Hybrid(LiftGoal),
    /// Bodyweight leverage ladders. A Power goal leads every day with
    /// ballistic jump / explosive push-up ladders (CAL-POWER-001).
    Calisthenics(LiftGoal),
    /// Lifting only.
    Lifting(LiftGoal),
    /// Running only, toward the goal distance.
    Running(GoalDistance),
}

impl ProgramFocus {
    /// The focus of this profile's program. Reads the STATED lift goal.
    pub(crate) fn of(pp: &PlanProfile) -> Self {
        let profile = pp.profile();
        match (pp.lifts(), pp.runs()) {
            (true, true) => ProgramFocus::Hybrid(profile.lift_goal),
            (true, false) if pp.plans_calisthenics() => {
                ProgramFocus::Calisthenics(profile.lift_goal)
            }
            (true, false) => ProgramFocus::Lifting(profile.lift_goal),
            (false, _) => ProgramFocus::Running(profile.goal_distance),
        }
    }

    fn goal(self) -> Goal {
        match self {
            ProgramFocus::Hybrid(_) => Goal::Hybrid,
            ProgramFocus::Calisthenics(g) | ProgramFocus::Lifting(g) => match g {
                LiftGoal::MaxStrength => Goal::Strength,
                LiftGoal::Power => Goal::Power,
                LiftGoal::Hypertrophy => Goal::Hypertrophy,
            },
            ProgramFocus::Running(d) => {
                d.metres()
                    .map_or(Goal::GeneralEndurance, |m| Goal::RunningRace {
                        distance_km: (m / 1000.0) as f32,
                    })
            }
        }
    }
}

/// A representative registry claim for the whole program (drives the summary
/// card's evidence chip). Always the dominant branch's own claim.
pub fn summary_claim(pp: &PlanProfile) -> crate::evidence::ClaimId {
    if !pp.lifts() {
        return claims::RUN_WORKOUT_001;
    }
    if pp.plans_calisthenics() {
        return claims::CAL_PROG_MECHANIC_001;
    }
    let lift_goal = pp.profile().lift_goal;
    if pp.training_age() == TrainingAge::Novice {
        match lift_goal {
            LiftGoal::Power => claims::STR_PWR_001,
            _ => claims::STR_INTENT_001,
        }
    } else if lift_goal == LiftGoal::Hypertrophy {
        claims::HYP_DEFAULT_PROG_001
    } else {
        claims::STR_DUP_001
    }
}

#[cfg(test)]
mod tests;
