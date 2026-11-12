//! Session moves inside the plan week: the arrangement, the spacing check and
//! the move options.

use crate::app::format::weekday_name;
use crate::app::logged_days::LoggedDays;
use crate::app::model::Model;
use crate::app::plan_views::plan_item::PlanRow;
use crate::app::view_types::SessionStatus;
use crate::schema::Session;

/// The one wording for "skipping a session is fine" (ADH-MISSED-001), shared
/// by the missed-session note and the move notes.
pub(super) const MISSED_SESSION_COSTS_LITTLE: &str = "One missed session costs little.";

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
            Some(conflict) => {
                notes[from] = Some(format!(
                    "Couldn't move this session to {}: {}. Pick another day, or skip it. {MISSED_SESSION_COSTS_LITTLE}",
                    weekday_name(mv.to_epoch_day),
                    move_conflict_copy(conflict)
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
        day < start || (day < today && !days.active.contains(&day))
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
    row: &mut PlanRow,
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
        crate::plan::MoveBlock::Spacing { day, conflict } => format!(
            "No other day this week works. On {}, {}. {skip}",
            weekday_name(week_monday.saturating_add(*day as i64)),
            move_conflict_copy(*conflict)
        ),
    }
}

/// Why a session can't move to a day, as a clause.
fn move_conflict_copy(conflict: crate::plan::MoveConflict) -> String {
    use crate::plan::{MoveConflict, SpacingRule};
    let rule = match conflict {
        MoveConflict::NotAnotherDay => return "pick a different day in the same week".into(),
        MoveConflict::NothingToMove => return "there is no session on that day to move".into(),
        MoveConflict::DayTaken => return "that day already has a session".into(),
        MoveConflict::Spacing(rule) => rule,
    };
    let h = rule.min_gap_hours();
    match rule {
        SpacingRule::SameMuscles => format!(
            "the same muscles would be trained on back-to-back days (keep about {h:.0} hours between them)"
        ),
        SpacingRule::HardRunAfterHeavyLegs => format!(
            "a hard or long run would come the day after heavy leg work (keep at least {h:.0} hours between them)"
        ),
        SpacingRule::HardRunBeforeHeavyLegs => format!(
            "a hard or long run would come the day before heavy leg work (keep at least {h:.0} hours between them)"
        ),
        SpacingRule::HardBackToBack => format!(
            "two hard sessions would land on back-to-back days (keep about {h:.0} hours between them)"
        ),
    }
}
