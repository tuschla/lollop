//! Session moves inside the plan week: the arrangement, the spacing check and
//! the move options.

use super::*;

/// This week's arrangement after the athlete's session moves
/// (ADH-RESCHED-001): `source[d]` is the plan-day offset whose session shows on
/// weekday `d`. Moves apply in order; one that would break the spacing rules
/// (`plan::move_conflict`, `idle` days loading nothing) is skipped, and its
/// plain-language reason is kept on the day the session stays on. Validated
/// against the current plan and logs, so a move's fate never depends on when
/// the log was replayed.
pub(super) fn week_arrangement(
    model: &Model,
    week_monday: i64,
    sessions: &[Session],
    idle: [bool; 7],
) -> ([u16; 7], [Option<String>; 7]) {
    let mut source: [u16; 7] = [0, 1, 2, 3, 4, 5, 6];
    let mut notes: [Option<String>; 7] = Default::default();
    for mv in &model.session_moves {
        let from = mv.from_epoch_day.saturating_sub(week_monday);
        let to = mv.to_epoch_day.saturating_sub(week_monday);
        if !(0..7).contains(&from) || !(0..7).contains(&to) {
            continue;
        }
        let (from, to) = (from as usize, to as usize);
        let loads = arranged_loads(&source, sessions);
        match crate::plan::move_conflict(loads, idle, from, to) {
            None => {
                source.swap(from, to);
                notes[from] = None;
                notes[to] = None;
            }
            Some(reason) => {
                notes[from] = Some(format!(
                    "Couldn't move this session to {}: {reason}. Pick another day, or skip it. {MISSED_SESSION_COSTS_LITTLE}",
                    weekday_name(mv.to_epoch_day)
                ));
            }
        }
    }
    (source, notes)
}

/// Each weekday's load under an arrangement, for the move spacing check.
pub(super) fn arranged_loads(source: &[u16; 7], sessions: &[Session]) -> [crate::plan::DayLoad; 7] {
    std::array::from_fn(|d| {
        sessions
            .iter()
            .find(|s| s.day == source[d])
            .map_or(crate::plan::DayLoad::Rest, crate::plan::session_load)
    })
}

/// The weekdays of the week starting `week_monday` that loaded nothing for
/// the move spacing check: before the plan started, or past with nothing
/// logged (a planned session that never happened leaves no fatigue, so a
/// session can come forward next to it, e.g. onto today after a quiet 48 h).
pub(super) fn idle_days(days: &LoggedDays, week_monday: i64, today: i64, start: i64) -> [bool; 7] {
    std::array::from_fn(|d| {
        let day = week_monday.saturating_add(d as i64);
        day < start
            || (day < today && !days.sets.contains(&Some(day)) && !days.runs.contains(&Some(day)))
    })
}

/// A row the athlete can still move: not rest, not finished or partly done,
/// not on hold, and inside the plan window.
pub(super) fn movable(status: SessionStatus) -> bool {
    !matches!(
        status,
        SessionStatus::Rest
            | SessionStatus::Blocked
            | SessionStatus::BeforePlan
            | SessionStatus::Partial
    ) && !status.is_done()
}

/// Fill `row`'s move targets: every weekday of the week starting
/// `week_monday` from `earliest` on (that day included) the session on weekday
/// `from` can move to (ADH-RESCHED-001); when none works, the plain-language
/// reason instead (never double up, ADH-MISSED-001). `idle`: see
/// [`idle_days`].
pub(super) fn fill_move_options(
    row: &mut SessionPlanView,
    loads: [crate::plan::DayLoad; 7],
    idle: [bool; 7],
    from: usize,
    week_monday: i64,
    earliest: i64,
) {
    let candidates = (0..7usize).filter(|&t| week_monday.saturating_add(t as i64) >= earliest);
    match crate::plan::move_options(loads, idle, from, candidates) {
        Ok(targets) => {
            row.move_targets = targets
                .into_iter()
                .map(|t| week_monday.saturating_add(t as i64))
                .collect();
            row.move_blocked_reason = None;
        }
        Err(block) => {
            row.move_targets.clear();
            row.move_blocked_reason = Some(move_blocked_copy(&block, week_monday));
        }
    }
}

/// Why a session can't be moved this week, in plain language.
fn move_blocked_copy(block: &crate::plan::MoveBlock, week_monday: i64) -> String {
    let skip =
        format!("If you can't train on the planned day, skip it. {MISSED_SESSION_COSTS_LITTLE}");
    match block {
        crate::plan::MoveBlock::NoDaysLeft => {
            format!("No day is left this week to move it to. {skip}")
        }
        crate::plan::MoveBlock::AllTaken => format!(
            "Every day left this week already has a session, and the plan never puts two on one day. {skip}"
        ),
        crate::plan::MoveBlock::Spacing { day, reason } => format!(
            "No free day left this week works. On {}, {reason}. {skip}",
            weekday_name(week_monday.saturating_add(*day as i64))
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_options_include_today_and_a_blocked_row_says_why() {
        use crate::plan::{DayLoad, DayRegion};
        let mut loads = [DayLoad::Rest; 7];
        loads[0] = DayLoad::Lift {
            region: DayRegion::Full,
            heavy_legs: true,
        };
        loads[3] = DayLoad::Run { hard: true };
        let monday = 700i64 - mon0_weekday(700);
        // Today is Wednesday (a rest day): the run can come forward to today.
        let mut row = SessionPlanView::default();
        fill_move_options(&mut row, loads, [false; 7], 3, monday, monday + 2);
        assert_eq!(row.move_targets, vec![monday + 2, monday + 4, monday + 5]);
        assert_eq!(row.move_blocked_reason, None);
        // From Sunday on nothing is left this week.
        let mut row = SessionPlanView::default();
        fill_move_options(&mut row, loads, [false; 7], 3, monday, monday + 7);
        assert!(row.move_targets.is_empty());
        let reason = row.move_blocked_reason.expect("explained");
        assert!(reason.starts_with("No day is left this week"), "{reason}");
        // Heavy legs planned on Tuesday blocks Wednesday for a hard run, but
        // not once Tuesday passed with nothing trained.
        let mut loads = [DayLoad::Rest; 7];
        loads[1] = DayLoad::Lift {
            region: DayRegion::Full,
            heavy_legs: true,
        };
        loads[3] = DayLoad::Run { hard: true };
        let mut row = SessionPlanView::default();
        fill_move_options(&mut row, loads, [false; 7], 3, monday, monday + 2);
        assert!(!row.move_targets.contains(&(monday + 2)));
        let mut idle = [false; 7];
        idle[0] = true;
        idle[1] = true;
        let mut row = SessionPlanView::default();
        fill_move_options(&mut row, loads, idle, 3, monday, monday + 2);
        assert_eq!(row.move_targets.first(), Some(&(monday + 2)));
    }
}
