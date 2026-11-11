//! Today's surface facts (`today_surface::mark_today`): the day's state, the
//! session's run item and the repeat marks the shell's card stack reads
//! instead of comparing card text.

use super::*;
use crate::app::today_surface::mark_today;
use crate::app::view_types::{
    HeadlineKind, RunTargetView, SessionStatus, TodayHeadlineView, TodayState,
};
use crate::schema::{LiftSessionType, RunSessionType};

const DAY: i64 = 20_400;

fn day(
    epoch_day: i64,
    session_type: SessionType,
    status: SessionStatus,
    items: Vec<PrescriptionView>,
) -> SessionPlanView {
    SessionPlanView {
        epoch_day,
        title: format!("S{epoch_day}"),
        session_type,
        status,
        items,
        ..SessionPlanView::default()
    }
}

fn lift_day(epoch_day: i64, status: SessionStatus) -> SessionPlanView {
    let item = PrescriptionView {
        exercise: "Back squat".into(),
        done: status.is_done(),
        ..PrescriptionView::default()
    };
    day(
        epoch_day,
        SessionType::Lift(LiftSessionType::MaxEffort),
        status,
        vec![item],
    )
}

fn vm(next: Option<SessionPlanView>, week: Vec<SessionPlanView>) -> ViewModel {
    ViewModel {
        today_epoch_day: Some(DAY),
        profile: Some(base_profile()),
        next_session: next,
        week_plan: week,
        ..ViewModel::default()
    }
}

fn marked(mut v: ViewModel, model: &Model) -> ViewModel {
    mark_today(&mut v, model);
    v
}

#[test]
fn todays_state_is_the_widgets_and_todays_gates() {
    let m = Model::default();
    let state = |v: ViewModel| marked(v, &m).today_surface.state;
    let today = lift_day(DAY, SessionStatus::Next);
    assert_eq!(
        state(vm(Some(today.clone()), vec![today.clone()])),
        TodayState::Session
    );
    // A hold wins over any plan.
    let held = ViewModel {
        hold_reason: Some(HoldReason::Pain),
        ..vm(Some(today.clone()), vec![today])
    };
    assert_eq!(state(held), TodayState::Paused);
    // No plan: setup without a profile, else the plan prompt.
    assert_eq!(state(vm(None, vec![])), TodayState::NoPlan);
    let fresh = ViewModel {
        profile: None,
        ..vm(None, vec![])
    };
    assert_eq!(state(fresh), TodayState::Setup);
    // Today's session done: next_session moved on, today's row says what was done.
    let next = lift_day(DAY + 2, SessionStatus::Next);
    let done = lift_day(DAY, SessionStatus::Done);
    assert_eq!(
        state(vm(Some(next.clone()), vec![done, next.clone()])),
        TodayState::Done
    );
    // A plan rest day, or no plan day today at all.
    let rest = day(DAY, SessionType::Rest, SessionStatus::Rest, vec![]);
    let v = marked(vm(Some(next.clone()), vec![rest, next.clone()]), &m);
    assert_eq!(v.today_surface.state, TodayState::Rest);
    assert!(v.today_surface.rest_today);
    let v = marked(vm(Some(next.clone()), vec![next]), &m);
    assert_eq!(v.today_surface.state, TodayState::Rest);
    assert!(!v.today_surface.rest_today);
}

#[test]
fn the_guide_shows_until_three_days_with_a_logged_lift() {
    let at = |d: i64| d * DAY_SEC + 10 * 3600;
    let mut events = vec![
        log_set("Back squat", 100.0, 5, 8.0, at(DAY - 5)),
        log_set("Bench", 60.0, 5, 8.0, at(DAY - 5)),
        log_set("Back squat", 100.0, 5, 8.0, at(DAY - 3)),
        // An undated set belongs to no day.
        log_set("Row", 50.0, 8, 8.0, 0),
    ];
    let (_, model) = replay(&events);
    assert!(marked(vm(None, vec![]), &model).today_surface.show_guide);
    events.push(log_set("Deadlift", 140.0, 3, 8.0, at(DAY - 1)));
    let (_, model) = replay(&events);
    assert!(!marked(vm(None, vec![]), &model).today_surface.show_guide);
}

#[test]
fn a_run_sessions_run_item_is_its_first_open_item_with_a_target() {
    let plain = PrescriptionView::default();
    let targeted = PrescriptionView {
        run_target: Some(RunTargetView::default()),
        ..PrescriptionView::default()
    };
    let done = PrescriptionView {
        done: true,
        run_target: Some(RunTargetView::default()),
        ..PrescriptionView::default()
    };
    let run = |items| {
        day(
            DAY,
            SessionType::Run(RunSessionType::Easy),
            SessionStatus::Next,
            items,
        )
    };
    let item_of = |s: SessionPlanView| {
        let v = marked(vm(Some(s.clone()), vec![s]), &Model::default());
        (v.next_session.unwrap().run_item, v.week_plan[0].run_item)
    };
    assert_eq!(
        item_of(run(vec![done.clone(), plain.clone(), targeted])),
        (Some(2), Some(2))
    );
    assert_eq!(item_of(run(vec![done.clone(), plain])), (Some(1), Some(1)));
    assert_eq!(item_of(run(vec![done])), (None, None));
    assert_eq!(item_of(lift_day(DAY, SessionStatus::Next)), (None, None));
}

fn adjustment(summary: &str, citation: &str) -> AdjustmentView {
    AdjustmentView {
        summary: summary.into(),
        evidence: EvidenceTagView {
            citation: citation.into(),
            ..EvidenceTagView::default()
        },
        ..AdjustmentView::default()
    }
}

#[test]
fn a_return_check_item_owns_its_sessions_adjustment() {
    let adj = adjustment("Return check after illness", "Schwellnus 2022");
    let check = PrescriptionView {
        return_check: true,
        ..PrescriptionView::default()
    };
    let mut s = lift_day(DAY, SessionStatus::Adjusted);
    s.adjustment = Some(adj.clone());
    s.items = vec![check];
    let v = marked(vm(Some(s.clone()), vec![s.clone()]), &Model::default());
    assert!(v.next_session.unwrap().adjustment_on_item);
    s.items = vec![PrescriptionView::default()];
    let v = marked(vm(Some(s.clone()), vec![s]), &Model::default());
    assert!(!v.next_session.unwrap().adjustment_on_item);
}

#[test]
fn rows_the_headline_or_the_sessions_adjustment_state_are_marked() {
    let said = adjustment("Stop a rep earlier", "c1");
    let other = adjustment("Stop a rep earlier", "c2");
    let mut s = lift_day(DAY, SessionStatus::Adjusted);
    s.adjustment = Some(said.clone());
    let headline = |kind| TodayHeadlineView {
        kind,
        summary: said.summary.clone(),
        evidence: said.evidence.clone(),
        ..TodayHeadlineView::default()
    };
    let feedback = FeedbackView {
        category: crate::feedback::FeedbackCategory::PositiveMastery,
        category_label: String::new(),
        message: said.summary.clone(),
        glossary: Vec::new(),
        subject: String::new(),
        prose: false,
        evidence: said.evidence.clone(),
        why: WhyView::default(),
        repeats_session: false,
    };
    let v = marked(
        ViewModel {
            today_headline: headline(HeadlineKind::Adjustment),
            adjustments: vec![said.clone(), other.clone()],
            review_adjustments: vec![said.clone()],
            feedback: Some(feedback),
            ..vm(Some(s.clone()), vec![s.clone()])
        },
        &Model::default(),
    );
    // Same summary, another citation: another statement.
    assert_eq!(
        v.adjustments
            .iter()
            .map(|a| (a.repeats_headline, a.repeats_session))
            .collect::<Vec<_>>(),
        [(true, true), (false, false)]
    );
    assert!(v.review_adjustments[0].repeats_headline && v.review_adjustments[0].repeats_session);
    assert!(v.today_headline.repeats_session);
    assert!(v.feedback.unwrap().repeats_session);
    // Only an adjustment headline states a row.
    let v = marked(
        ViewModel {
            today_headline: headline(HeadlineKind::Feedback),
            adjustments: vec![said],
            ..vm(None, vec![])
        },
        &Model::default(),
    );
    assert!(!v.adjustments[0].repeats_headline && !v.adjustments[0].repeats_session);
}
