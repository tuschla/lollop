//! Today as every surface reads it (the Today screen, the home-screen widget
//! and tile, the Log sheet, the run tracker): the day's state, the session's
//! run item and which cards already state the session's adjustment, derived
//! once from the finished view.

use std::collections::HashSet;

use crate::app::model::Model;
use crate::app::view_types::{
    AdjustmentView, HeadlineKind, PrescriptionView, SessionPlanView, SessionStatus, TodayState,
    TodaySurfaceView, ViewModel,
};
use crate::day::{is_undated, local_day};
use crate::schema::SessionType;

/// Lifting days with a logged set for which the session card still spells out
/// each lift's prescription and how to pick a first weight.
pub(super) const GUIDED_LIFT_DAYS: usize = 3;

/// Fill [`ViewModel::today_surface`], each run session's `run_item`, and the
/// repeat marks the Today card stack reads instead of comparing card text.
pub(super) fn mark_today(vm: &mut ViewModel, model: &Model) {
    vm.today_surface = surface(vm, model);
    for s in vm
        .next_session
        .iter_mut()
        .chain(vm.week_plan.iter_mut())
        .chain(vm.next_week_plan.iter_mut())
    {
        s.run_item = run_item(s);
        s.adjustment_on_item = s.adjustment.is_some() && s.items.iter().any(|it| it.return_check);
    }
    mark_repeats(vm);
}

fn surface(vm: &ViewModel, model: &Model) -> TodaySurfaceView {
    let today = vm.today_epoch_day;
    let today_row = |done: bool| {
        vm.week_plan
            .iter()
            .find(|s| Some(s.epoch_day) == today && (!done || s.status.is_done()))
    };
    let state = match &vm.next_session {
        _ if vm.hold_reason.is_some() => TodayState::Paused,
        None if vm.profile.is_none() => TodayState::Setup,
        None => TodayState::NoPlan,
        Some(ns) if Some(ns.epoch_day) == today => TodayState::Session,
        Some(_) if today_row(true).is_some_and(|s| !s.items.is_empty()) => TodayState::Done,
        Some(_) => TodayState::Rest,
    };
    TodaySurfaceView {
        state,
        rest_today: today_row(false).is_some_and(|s| s.status == SessionStatus::Rest),
        show_guide: logged_lift_days(model) < GUIDED_LIFT_DAYS,
    }
}

/// Distinct local days with a dated logged set, counted up to
/// [`GUIDED_LIFT_DAYS`].
fn logged_lift_days(model: &Model) -> usize {
    let offset = model.today_utc_offset_sec;
    let mut days = HashSet::new();
    for s in model.sets.iter().filter(|s| !is_undated(s.observed_at)) {
        days.insert(local_day(s.observed_at, offset));
        if days.len() >= GUIDED_LIFT_DAYS {
            break;
        }
    }
    days.len()
}

/// A run session's run item: its first open item that carries a target or a
/// structure, else its first open item.
fn run_item(s: &SessionPlanView) -> Option<usize> {
    if !matches!(s.session_type, SessionType::Run(_)) {
        return None;
    }
    let open = |it: &PrescriptionView| !it.done;
    s.items
        .iter()
        .position(|it| open(it) && (it.run_target.is_some() || it.run_structure.is_some()))
        .or_else(|| s.items.iter().position(open))
}

/// Mark the rows whose (summary, citation) the today headline or the next
/// session's adjustment already states: several rules emit one summary, and
/// a card never repeats one above it.
fn mark_repeats(vm: &mut ViewModel) {
    let key = |summary: &str, citation: &str| (summary.to_owned(), citation.to_owned());
    let headline = (vm.today_headline.kind == HeadlineKind::Adjustment).then(|| {
        key(
            &vm.today_headline.summary,
            &vm.today_headline.evidence.citation,
        )
    });
    let session = vm
        .next_session
        .as_ref()
        .and_then(|s| s.adjustment.as_ref())
        .map(|a| key(&a.summary, &a.evidence.citation));
    let mark = |a: &mut AdjustmentView| {
        let k = key(&a.summary, &a.evidence.citation);
        a.repeats_headline = headline.as_ref() == Some(&k);
        a.repeats_session = session.as_ref() == Some(&k);
    };
    vm.adjustments.iter_mut().for_each(&mark);
    vm.review_adjustments.iter_mut().for_each(&mark);
    let repeats_session =
        |summary: &str, citation: &str| session.as_ref() == Some(&key(summary, citation));
    vm.today_headline.repeats_session = repeats_session(
        &vm.today_headline.summary,
        &vm.today_headline.evidence.citation,
    );
    if let Some(fb) = vm.feedback.as_mut() {
        fb.repeats_session = repeats_session(&fb.message, &fb.evidence.citation);
    }
}
