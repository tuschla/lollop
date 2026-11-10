//! Per-view dated-day facts: the session day every logged set and run counts
//! on, and the plan weeks those days complete (the plan's block position and
//! its planned lifting deload days).

use crate::app::format::monday_of;
use crate::app::model::{LoggedRun, LoggedSet, Model, profile_for_day};
use crate::app::run_metrics::RunFacts;
use crate::day::{DAY_SEC, is_undated, local_day};
use crate::profile::{PlanProfile, Profile};

/// A set logged before this local time of day (04:00) may still belong to the
/// previous evening's session.
const SESSION_CHAIN_CUTOFF_SEC: i64 = 4 * 3_600;
/// The longest gap (3 h) after the previous set at which such an early set
/// still chains to that set's session day.
const SESSION_CHAIN_GAP_SEC: i64 = 3 * 3_600;

/// Session-day attribution for every logged set, aligned to `sets` by
/// index: `None` for an undated legacy row ([`is_undated`], which never
/// counts toward a day), else the local day it belongs to. Sets are walked in
/// `observed_at` order; a set logged before [`SESSION_CHAIN_CUTOFF_SEC`]
/// inherits the previous set's session day when that set is at most
/// [`SESSION_CHAIN_GAP_SEC`] earlier (chaining transitively), so a late
/// session that crosses midnight scores as ONE day.
/// The single attribution pass every `view()` shares — the completion counts and
/// the per-day plan-snapshot cutoff both read it, so no rescan per rendered day.
pub(super) fn attribute_session_days(sets: &[LoggedSet], offset: i64) -> Vec<Option<i64>> {
    let mut order: Vec<usize> = (0..sets.len())
        .filter(|&i| !is_undated(sets[i].observed_at))
        .collect();
    order.sort_by_key(|&i| sets[i].observed_at);
    let mut out = vec![None; sets.len()];
    let mut prev: Option<(i64, i64)> = None; // (observed_at, session_day)
    for &i in &order {
        let oa = sets[i].observed_at;
        let shifted = oa.saturating_add(offset);
        let local = shifted.div_euclid(DAY_SEC);
        let tod = shifted.rem_euclid(DAY_SEC);
        let sd = match prev {
            Some((poa, psd))
                if tod < SESSION_CHAIN_CUTOFF_SEC
                    && oa.saturating_sub(poa) <= SESSION_CHAIN_GAP_SEC =>
            {
                psd
            }
            _ => local,
        };
        out[i] = Some(sd);
        prev = Some((oa, sd));
    }
    out
}

/// Per-run session day, aligned to `runs` by index: the local day of the run's
/// START, [`RunFacts::started`] (its first QC'd fix, else its stamp minus its
/// duration), the one start every view and coaching bucket uses. `None` for an
/// undated legacy run ([`is_undated`]), which never counts.
fn attribute_run_days(runs: &[LoggedRun], facts: &RunFacts, offset: i64) -> Vec<Option<i64>> {
    runs.iter()
        .zip(&facts.started)
        .map(|(r, &start)| (!is_undated(r.observed_at)).then(|| local_day(start, offset)))
        .collect()
}

/// Every logged set's and run's session day plus the distinct local days with
/// lifting and with running, computed once per `view()` and shared by the
/// completion counts, the per-day snapshot cutoff and the plan-week records.
pub(in crate::app) struct LoggedDays {
    /// Per-set session day, aligned to `model.sets`.
    pub(in crate::app) sets: Vec<Option<i64>>,
    /// Per-run session day (its start day), aligned to `model.runs`.
    pub(super) runs: Vec<Option<i64>>,
    /// Distinct days with lifting / running, indexed by `consistency::Discipline`.
    pub(super) by_discipline: [std::collections::BTreeSet<i64>; 2],
    /// Distinct days with any logged training.
    pub(super) active: std::collections::BTreeSet<i64>,
    /// The view's plan-week records through today's week, built on first use
    /// ([`LoggedDays::plan_weeks`]).
    plan_weeks: std::cell::OnceCell<Option<PlanWeeks>>,
}

impl LoggedDays {
    pub(in crate::app) fn new(model: &Model, facts: &RunFacts) -> Self {
        let offset = model.today_utc_offset_sec;
        let sets = attribute_session_days(&model.sets, offset);
        let runs = attribute_run_days(&model.runs, facts, offset);
        let by_discipline: [std::collections::BTreeSet<i64>; 2] = [
            sets.iter().flatten().copied().collect(),
            runs.iter().flatten().copied().collect(),
        ];
        let active = by_discipline[0].union(&by_discipline[1]).copied().collect();
        Self {
            sets,
            runs,
            by_discipline,
            active,
            plan_weeks: std::cell::OnceCell::new(),
        }
    }

    /// Per-run session day (the day the run STARTED), aligned to `model.runs`;
    /// `None` for an undated run.
    pub(in crate::app) fn run_days(&self) -> &[Option<i64>] {
        &self.runs
    }

    /// The view's plan-week records through today's week
    /// ([`build_plan_weeks`]), built on first use and shared by every reader.
    pub(super) fn plan_weeks(&self, model: &Model) -> Option<&PlanWeeks> {
        self.plan_weeks
            .get_or_init(|| build_plan_weeks(model, self))
            .as_ref()
    }
}

/// ADH-PROGRESS-001: a plan week advances the plan once at least this share of
/// its planned sessions was done (`done / planned >= NUM / DEN`, two-thirds;
/// for Couch-to-5K that is 2 of 3 runs). Kept as a ratio so the test stays in
/// integers.
const WEEK_ADVANCE_SHARE: (usize, usize) = (2, 3);

/// The plan's block position (0-based) for the week whose Monday is
/// `week_monday`: the number of EARLIER plan weeks that were completed. The
/// plan moves forward by completed work, not the calendar: a week with fewer
/// than [`WEEK_ADVANCE_SHARE`] of its planned sessions done is repeated,
/// without penalty (ADH-PROGRESS-001). A partial first week (the plan started
/// mid-week) always counts. `None` without a plan request + profile. This is
/// the SAME index `build_plan_views` periodizes on, exposed so the weekly
/// report can align a logged week to the plan and tell whether it was the
/// planned recovery week.
pub(in crate::app) fn plan_week_index(
    model: &Model,
    days: &LoggedDays,
    week_monday: i64,
) -> Option<u32> {
    let start = model.plan_request?.start_epoch_day;
    let weeks = days.plan_weeks(model)?;
    Some(if week_monday <= weeks.through_monday {
        next_index(weeks.before(week_monday))
    } else {
        next_index(&plan_week_records(
            model,
            days,
            model.profile()?,
            start,
            week_monday,
        ))
    })
}

/// How far back plan weeks are scored (20 years): bounds the per-view work
/// for a corrupt far-future `today`.
const MAX_TRACKED_WEEKS: i64 = 1_040;

/// One finished plan week: planned vs done, and whether it moved the plan on.
pub(super) struct PlanWeekRecord {
    pub(super) monday: i64,
    /// The plan's block position that week: the earlier weeks that advanced it.
    index: u32,
    pub(super) done: usize,
    pub(super) planned: usize,
    /// The plan started mid-way through this week.
    pub(super) partial: bool,
    pub(super) advanced: bool,
}

/// The block position after `records` (0 for none).
fn next_index(records: &[PlanWeekRecord]) -> u32 {
    records
        .last()
        .map_or(0, |r| r.index.saturating_add(u32::from(r.advanced)))
}

/// A view's plan-week records: every week of the active plan before
/// `through_monday` (next week's Monday, so this week's own record is the
/// last; none without a plan), and the planned lifting deload days.
pub(super) struct PlanWeeks {
    through_monday: i64,
    records: Vec<PlanWeekRecord>,
    /// Local days (ascending) that were planned lifting deload days
    /// ([`lift_deload_days`]).
    lift_deloads: Vec<i64>,
}

impl PlanWeeks {
    /// The records of the weeks before `week_monday` (at most
    /// `through_monday`).
    pub(super) fn before(&self, week_monday: i64) -> &[PlanWeekRecord] {
        &self.records[..self.records.partition_point(|r| r.monday < week_monday)]
    }
}

/// The local days, through this week, that were planned lifting deload days.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::app) struct DeloadDays<'a>(pub(in crate::app) &'a [i64]);

impl DeloadDays<'_> {
    /// Whether local `day` was a planned lifting deload day.
    pub(in crate::app) fn contains(&self, day: i64) -> bool {
        self.0.binary_search(&day).is_ok()
    }
}

/// The planned lifting deload days through this week, of the active plan and
/// of every earlier one up to the day it was removed (`Model::past_plans`): a
/// day is one when its plan week is a deload ([`crate::plan::lift_deload_week`])
/// under the profile in force that day. The e1RM anchors, the last working
/// sets and the e1RM-decline gate skip their sets.
pub(in crate::app) fn lift_deload_days<'d>(model: &Model, days: &'d LoggedDays) -> DeloadDays<'d> {
    DeloadDays(
        days.plan_weeks(model)
            .map_or(&[], |w| w.lift_deloads.as_slice()),
    )
}

/// The view's [`PlanWeeks`] on `days` (built once per view by
/// [`LoggedDays::plan_weeks`]): the plan-week index of this and next week, the
/// consistency card, the readiness high-load check, the weekly report and the
/// deload days all read it.
pub(super) fn build_plan_weeks(model: &Model, days: &LoggedDays) -> Option<PlanWeeks> {
    let profile = model.profile()?;
    let active = model.plan_request.map(|r| r.start_epoch_day);
    let today = model
        .today_epoch_day
        .or(active)
        .or_else(|| model.past_plans.last().map(|&(_, end)| end))?;
    let through_monday = monday_of(today).saturating_add(7);
    let records = active.map_or_else(Vec::new, |start| {
        plan_week_records(model, days, profile, start, through_monday)
    });
    let mut lift_deloads = Vec::new();
    for &(start, end) in &model.past_plans {
        let last_monday = monday_of(end).saturating_add(7).min(through_monday);
        let weeks = plan_week_records(model, days, profile, start, last_monday);
        push_deload_days(model, profile, &weeks, start..=end, &mut lift_deloads);
    }
    if let Some(start) = active {
        push_deload_days(model, profile, &records, start..=today, &mut lift_deloads);
    }
    lift_deloads.sort_unstable();
    lift_deloads.dedup();
    Some(PlanWeeks {
        through_monday,
        records,
        lift_deloads,
    })
}

/// The days in `span` of `weeks` (one plan's) that are a lifting deload under
/// the profile in force that day, so a mid-week profile edit counts from the
/// day it applies, as the plan prescribes each day.
fn push_deload_days(
    model: &Model,
    profile: &Profile,
    weeks: &[PlanWeekRecord],
    span: std::ops::RangeInclusive<i64>,
    out: &mut Vec<i64>,
) {
    for r in weeks {
        for day in (r.monday..r.monday.saturating_add(7)).filter(|d| span.contains(d)) {
            let p = profile_for_day(model, day);
            let pp = PlanProfile::new(p.as_deref().unwrap_or(profile));
            if crate::plan::lift_deload_week(&pp, r.index, r.monday) {
                out.push(day);
            }
        }
    }
}

/// Every finished week of the plan starting `start`, up to, not including,
/// the week of `week_monday`. "Planned" is the week's training-day count for
/// the profile in force that Monday (else `profile`); "done" counts distinct
/// days with logged training, capped at planned.
fn plan_week_records(
    model: &Model,
    days: &LoggedDays,
    profile: &Profile,
    start: i64,
    week_monday: i64,
) -> Vec<PlanWeekRecord> {
    let start_monday = monday_of(start);
    let weeks = week_monday
        .saturating_sub(start_monday)
        .div_euclid(7)
        .clamp(0, MAX_TRACKED_WEEKS);
    let active = &days.active;
    let mut index = 0u32;
    (0..weeks)
        .map(|w| {
            let monday = start_monday.saturating_add(w * 7);
            let p = profile_for_day(model, monday);
            let planned = PlanProfile::new(p.as_deref().unwrap_or(profile)).planned_training_days();
            let done = (0..7)
                .filter(|d| active.contains(&monday.saturating_add(*d)))
                .count()
                .min(planned);
            let partial = monday < start;
            let (num, den) = WEEK_ADVANCE_SHARE;
            let advanced = partial || planned == 0 || done * den >= planned * num;
            let r = PlanWeekRecord {
                monday,
                index,
                done,
                planned,
                partial,
                advanced,
            };
            index = index.saturating_add(u32::from(advanced));
            r
        })
        .collect()
}
