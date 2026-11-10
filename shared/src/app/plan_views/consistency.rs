//! The planned-vs-done consistency card, and the session disciplines the
//! plan credits swapped sessions by.

use crate::app::format::{monday_of, weekday_name};
use crate::app::guidance::{push_guidance, push_guidance_basis};
use crate::app::logged_days::LoggedDays;
use crate::app::model::Model;
use crate::app::plan_views::week_moves::MISSED_SESSION_COSTS_LITTLE;
use crate::app::view_types::{
    ConsistencyView, ConsistencyWeekView, GuidanceView, SessionPlanView, SessionStatus,
};
use crate::evidence::claims;
use crate::schema::{Recommended, SessionType};

/// The finished plan weeks the consistency card shows and averages over.
const RECENT_WEEKS: usize = 8;

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
    let today = model.today_or_plan_start()?;
    let records = days.plan_weeks(model)?.before(monday_of(today));
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
    let selfmon = Recommended::new((), claims::ADH_SELFMON_001);
    let summary = if this_week_planned > 0 && this_week_done >= this_week_planned {
        format!("All {this_week_planned} planned sessions done this week.")
    } else {
        format!("{this_week_done} of {this_week_planned} planned sessions done this week.")
    };
    push_guidance(&mut notes, "Consistency", summary, &selfmon);

    if let Some(r) = last.filter(|_| repeating_week) {
        let prog = Recommended::new((), claims::ADH_PROGRESS_001);
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
            let re = Recommended::new((), claims::REENTRY_001);
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
            let rs = Recommended::new((), claims::ADH_RESCHED_001);
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
            let ms = Recommended::new((), claims::ADH_MISSED_001);
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
