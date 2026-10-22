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
    let records = plan_week_records(model, days, week_monday)?;
    Some(
        records
            .iter()
            .filter(|r| r.advanced)
            .count()
            .min(u32::MAX as usize) as u32,
    )
}

/// How far back plan weeks are scored (20 years): bounds the per-view work
/// for a corrupt far-future `today`.
const MAX_TRACKED_WEEKS: i64 = 1_040;

/// One finished plan week: planned vs done, and whether it moved the plan on.
struct PlanWeekRecord {
    monday: i64,
    done: usize,
    planned: usize,
    /// The plan started mid-way through this week.
    partial: bool,
    advanced: bool,
}

/// Every finished plan week from the plan's start week up to, not including,
/// the week of `week_monday`. "Planned" is the week's training-day count for
/// the profile in force that Monday; "done" counts distinct days with logged
/// training, capped at planned.
fn plan_week_records(
    model: &Model,
    days: &LoggedDays,
    week_monday: i64,
) -> Option<Vec<PlanWeekRecord>> {
    let profile = model.profile.as_ref()?;
    let req = model.plan_request?;
    let start = req.start_epoch_day;
    let start_monday = start.saturating_sub(mon0_weekday(start));
    let weeks = week_monday
        .saturating_sub(start_monday)
        .div_euclid(7)
        .clamp(0, MAX_TRACKED_WEEKS);
    let active = &days.active;
    Some(
        (0..weeks)
            .map(|w| {
                let monday = start_monday.saturating_add(w * 7);
                let p = profile_for_day(model, monday).unwrap_or(profile);
                let planned = crate::plan::planned_training_days(p);
                let done = (0..7)
                    .filter(|d| active.contains(&monday.saturating_add(*d)))
                    .count()
                    .min(planned);
                let partial = monday < start;
                let (num, den) = WEEK_ADVANCE_SHARE;
                PlanWeekRecord {
                    monday,
                    done,
                    planned,
                    partial,
                    advanced: partial || planned == 0 || done * den >= planned * num,
                }
            })
            .collect(),
    )
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

/// [`discipline`] of a flattened row.
fn row_discipline(row: &SessionPlanView) -> Option<Discipline> {
    discipline(row.session_type)
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
    let profile = model.profile.as_ref()?;
    let start = req.start_epoch_day;
    let today = model.today_epoch_day.unwrap_or(start);
    let week_monday = today.saturating_sub(mon0_weekday(today));
    let records = plan_week_records(model, days, week_monday)?;
    let planned_rows: Vec<&SessionPlanView> = week
        .iter()
        .filter(|s| {
            s.session_type != SessionType::Rest
                && s.status != SessionStatus::BeforePlan
                && s.epoch_day >= start
        })
        .collect();
    let this_week_planned = planned_rows.len();
    let this_week_done = planned_rows.iter().filter(|s| s.status.is_done()).count();
    let recent: Vec<&PlanWeekRecord> = {
        let mut r: Vec<&PlanWeekRecord> = records.iter().rev().take(RECENT_WEEKS).collect();
        r.reverse();
        r
    };
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
                "Repeating this plan week: last week had {} of {} sessions, so the plan stays here before moving ahead.",
                r.done, r.planned
            ),
            &prog,
        );
        if r.done == 0 && r.planned > 0 && profile.weeks_off.is_none_or(|w| w <= 0.0) {
            let re = graded((), claims::REENTRY_001);
            push_guidance(
                &mut notes,
                "Consistency",
                "No sessions last week. If you've been away for a week or more, add your time off in your profile and the plan eases you back in.".to_string(),
                &re,
            );
        }
    }

    // A missed session this week: make it up on a later free day only if the
    // spacing allows, never double up (ADH-MISSED-001 / ADH-RESCHED-001). No
    // nudge while a hold is active, or when the same kind of session was
    // logged within the credit window (the work was done, just not that day).
    let logged = &days.by_discipline;
    let logged_nearby = |s: &SessionPlanView| {
        let w = CREDIT_WINDOW_DAYS as i64;
        let from = s.epoch_day.saturating_sub(w).max(start);
        let to = s.epoch_day.saturating_add(w);
        from <= to
            && row_discipline(s)
                .is_some_and(|k| logged[k as usize].range(from..=to).next().is_some())
    };
    if !hold
        && let Some(missed) = week
            .iter()
            .find(|s| s.status == SessionStatus::Missed && !logged_nearby(s))
    {
        let day = weekday_name(missed.epoch_day);
        // Lower-case the session name's first letter mid-sentence ("Monday's
        // easy run"), keeping any acronym inside it.
        let mut chars = missed.title.chars();
        let title: String = chars
            .next()
            .map(|c| c.to_lowercase().chain(chars).collect())
            .unwrap_or_default();
        let never_double = format!("Never double up. {MISSED_SESSION_COSTS_LITTLE}");
        match missed.move_targets.first() {
            Some(&t) => {
                let rs = graded((), claims::ADH_RESCHED_001);
                let (to, basis_day) = if t == today {
                    ("today", "Today")
                } else {
                    (weekday_name(t), weekday_name(t))
                };
                push_guidance_basis(
                    &mut notes,
                    "Consistency",
                    format!("Missed {day}'s {title}? Move it to {to}, or carry on with the plan."),
                    Some(format!(
                        "{basis_day} still leaves a rest day before your next hard session. {never_double}"
                    )),
                    &rs,
                );
            }
            None => {
                let ms = graded((), claims::ADH_MISSED_001);
                push_guidance_basis(
                    &mut notes,
                    "Consistency",
                    format!("Missed {day}'s {title}? Carry on with the plan."),
                    Some(never_double),
                    &ms,
                );
            }
        }
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
