//! Plan-week progression and the planned-vs-done consistency card.

use super::*;

/// ADH-PROGRESS-001: a plan week advances the plan once at least this share of
/// its planned sessions was done (`done / planned >= NUM / DEN`, two-thirds;
/// for Couch-to-5K that is 2 of 3 runs). Kept as a ratio so the test stays in
/// integers.
const WEEK_ADVANCE_SHARE: (usize, usize) = (2, 3);

/// The finished plan weeks the consistency card shows and averages over.
const RECENT_WEEKS: usize = 8;

/// The plan's block position (0-based) for the week whose Monday is
/// `week_monday`: the number of EARLIER plan weeks that were completed. The
/// plan moves forward by completed work, not the calendar: a week with fewer
/// than [`WEEK_ADVANCE_SHARE`] of its planned sessions done is repeated,
/// without penalty (ADH-PROGRESS-001). A partial first week (the plan started
/// mid-week) always counts. `None` without a plan request + profile. This is
/// the SAME index [`build_plan_views`] periodizes on, exposed so the weekly
/// report can align a logged week to the plan and tell whether it was the
/// planned recovery week.
pub(in crate::app) fn plan_week_index(
    model: &Model,
    days: &LoggedDays,
    week_monday: i64,
) -> Option<u32> {
    let start = model.plan_request?.start_epoch_day;
    let weeks = plan_weeks(model, days)?;
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
    monday: i64,
    /// The plan's block position that week: the earlier weeks that advanced it.
    index: u32,
    done: usize,
    planned: usize,
    /// The plan started mid-way through this week.
    partial: bool,
    advanced: bool,
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
    fn before(&self, week_monday: i64) -> &[PlanWeekRecord] {
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
    DeloadDays(plan_weeks(model, days).map_or(&[], |w| w.lift_deloads.as_slice()))
}

/// The view's [`PlanWeeks`], built once per view on `days`: the plan-week
/// index of this and next week, the consistency card, the readiness
/// high-load check, the weekly report and the deload days all read it.
fn plan_weeks<'d>(model: &Model, days: &'d LoggedDays) -> Option<&'d PlanWeeks> {
    days.plan_weeks
        .get_or_init(|| {
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
        })
        .as_ref()
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
            if crate::plan::lift_deload_week(p.as_deref().unwrap_or(profile), r.index, r.monday) {
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
            let planned = crate::plan::planned_training_days(p.as_deref().unwrap_or(profile));
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

/// English weekday name for a local epoch day.
pub(in crate::app) fn weekday_name(epoch_day: i64) -> &'static str {
    const NAMES: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    NAMES[mon0_weekday(epoch_day) as usize]
}

/// What a session trains, for crediting swapped sessions: logged sets
/// (lifting or calisthenics) or a run. Indexes the per-discipline tables.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Discipline {
    Sets = 0,
    Run = 1,
}

pub(super) fn discipline(t: SessionType) -> Option<Discipline> {
    match t {
        SessionType::Lift(_) => Some(Discipline::Sets),
        SessionType::Run(_) => Some(Discipline::Run),
        SessionType::Rest => None,
    }
}

/// How far (days) a same-discipline session logged on another day of the week
/// may sit from a planned one and still be credited to it, and how far a
/// "Missed …" nudge looks for one before nagging. Kept inside the plan week,
/// the reschedule window (ADH-RESCHED-001).
pub(super) const CREDIT_WINDOW_DAYS: usize = 2;

/// Planned-vs-done feedback for the active plan: this week's done/planned off
/// the week strip, the last [`RECENT_WEEKS`] finished plan weeks, and neutral
/// notes (ADH-SELFMON-001, ADH-PROGRESS-001, ADH-MISSED-001, ADH-RESCHED-001,
/// REENTRY-001). No streaks or loss framing (ADH-GAMIFY-001). `hold`: a safety
/// hold is active, so no make-up nudge (HARD RULE 3).
pub(in crate::app) fn build_consistency(
    model: &Model,
    days: &LoggedDays,
    week: &[SessionPlanView],
    hold: bool,
) -> Option<ConsistencyView> {
    let req = model.plan_request?;
    let profile = model.profile()?;
    let start = req.start_epoch_day;
    let today = model.today_epoch_day.unwrap_or(start);
    let records = plan_weeks(model, days)?.before(monday_of(today));
    let planned_rows: Vec<&SessionPlanView> = week
        .iter()
        .filter(|s| s.session_type != SessionType::Rest && s.epoch_day >= start)
        .collect();
    let this_week_planned = planned_rows.len();
    let this_week_done = planned_rows.iter().filter(|s| s.status.is_done()).count();
    let recent = &records[records.len().saturating_sub(RECENT_WEEKS)..];
    let (done_sum, planned_sum) = recent
        .iter()
        .filter(|r| !r.partial)
        .fold((0usize, 0usize), |(d, p), r| (d + r.done, p + r.planned));
    let share_pct = (planned_sum > 0).then(|| ((100 * done_sum) / planned_sum).min(100) as u8);
    let last = records.last().filter(|r| !r.partial);
    let repeating_week = last.is_some_and(|r| !r.advanced);

    let mut notes: Vec<GuidanceView> = Vec::new();
    let selfmon = graded((), claims::ADH_SELFMON_001);
    let summary = if this_week_planned > 0 && this_week_done >= this_week_planned {
        format!("All {this_week_planned} planned sessions done this week.")
    } else {
        format!("{this_week_done} of {this_week_planned} planned sessions done this week.")
    };
    push_guidance(&mut notes, "Consistency", summary, &selfmon);

    if let Some(r) = last.filter(|_| repeating_week) {
        let prog = graded((), claims::ADH_PROGRESS_001);
        push_guidance(
            &mut notes,
            "Consistency",
            format!(
                "Repeating this plan week: last week had {} of {} sessions.",
                r.done, r.planned
            ),
            &prog,
        );
        if r.done == 0 && r.planned > 0 && profile.weeks_off.is_none_or(|w| w <= 0.0) {
            let re = graded((), claims::REENTRY_001);
            push_guidance(
                &mut notes,
                "Consistency",
                "No sessions last week. If you were away a week or more, enter the break in your profile so the plan restarts lighter.".to_string(),
                &re,
            );
        }
    }

    if !hold {
        push_missed_session_note(&mut notes, days, week, start, today);
    }

    Some(ConsistencyView {
        this_week_done: this_week_done.min(u8::MAX as usize) as u8,
        this_week_planned: this_week_planned.min(u8::MAX as usize) as u8,
        weeks: recent
            .iter()
            .map(|r| ConsistencyWeekView {
                week_monday_epoch_day: r.monday,
                done: r.done.min(u8::MAX as usize) as u8,
                planned: r.planned.min(u8::MAX as usize) as u8,
            })
            .collect(),
        share_pct,
        repeating_week,
        notes,
    })
}

/// A missed session this week: make it up on a later free day only if the
/// spacing allows, never double up (ADH-MISSED-001 / ADH-RESCHED-001). No
/// note when the same kind of session was logged within the credit window
/// (the work was done, just not that day).
fn push_missed_session_note(
    notes: &mut Vec<GuidanceView>,
    days: &LoggedDays,
    week: &[SessionPlanView],
    start: i64,
    today: i64,
) {
    let logged = &days.by_discipline;
    let logged_nearby = |s: &SessionPlanView| {
        let w = CREDIT_WINDOW_DAYS as i64;
        let from = s.epoch_day.saturating_sub(w).max(start);
        let to = s.epoch_day.saturating_add(w);
        from <= to
            && discipline(s.session_type)
                .is_some_and(|k| logged[k as usize].range(from..=to).next().is_some())
    };
    let Some(missed) = week
        .iter()
        .find(|s| s.status == SessionStatus::Missed && !logged_nearby(s))
    else {
        return;
    };
    let day = weekday_name(missed.epoch_day);
    // Lower-case the session name's first letter mid-sentence ("Monday's
    // easy run"), keeping any acronym inside it.
    let mut chars = missed.title.chars();
    let title: String = chars
        .next()
        .map(|c| c.to_lowercase().chain(chars).collect())
        .unwrap_or_default();
    let never_double =
        format!("Don't do two sessions on one day to catch up. {MISSED_SESSION_COSTS_LITTLE}");
    match missed.move_targets.first() {
        Some(&t) => {
            let rs = graded((), claims::ADH_RESCHED_001);
            let (to, basis_day) = if t == today {
                ("today", "Today")
            } else {
                (weekday_name(t), weekday_name(t))
            };
            push_guidance_basis(
                notes,
                "Consistency",
                format!("Missed {day}'s {title}. Move it to {to}, or carry on with the plan."),
                Some(format!(
                    "{basis_day} still leaves a rest day before your next hard session. {never_double}"
                )),
                &rs,
            );
        }
        None => {
            let ms = graded((), claims::ADH_MISSED_001);
            push_guidance_basis(
                notes,
                "Consistency",
                format!("Missed {day}'s {title}. Carry on with the plan."),
                Some(never_double),
                &ms,
            );
        }
    }
}
