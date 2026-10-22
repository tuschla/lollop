//! The two ways a user says they are ill (owner report 2026-10-17): an illness
//! report from the morning check-in and the profile's "I'm ill right now"
//! answer. Either one holds training; the banner offers ONE illness action
//! that ends both (the shell sends `ResolveHold` plus the profile switch-off
//! when `ends_profile_illness`), the resolved report still leads into the
//! return check (ready-upd-003), and while a plan cannot be built the view says
//! why (`plan_withheld`) so "Plan my training" is never a dead button.

use super::*;

const TODAY: i64 = 20_740;
const OFFSET: i64 = 7_200;

fn ev(app: &Engine, model: &mut Model, e: Event) {
    app.update(e, model).expect_only_render();
}

fn profile(acute_illness: bool) -> Profile {
    Profile {
        health: HealthScreen {
            acute_illness,
            ..HealthScreen::default()
        },
        effective_from_epoch_day: Some(TODAY),
        ..strength_profile()
    }
}

/// A set-up user on `TODAY` with the plan requested, as guided setup leaves it.
fn set_up(ill_in_profile: bool) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    ev(
        &app,
        &mut model,
        Event::SetToday {
            epoch_day: TODAY,
            utc_offset_sec: OFFSET,
        },
    );
    ev(&app, &mut model, Event::SetProfile(profile(ill_in_profile)));
    ev(
        &app,
        &mut model,
        Event::GeneratePlan {
            start_epoch_day: TODAY,
        },
    );
    (app, model)
}

fn report_flu(app: &Engine, model: &mut Model) {
    ev(
        app,
        model,
        Event::SubmitReadiness(ReadinessInput {
            observed_at: TODAY * DAY_SEC + 3_600,
            ..input(ReadinessSignal::Illness, 2.0)
        }),
    );
}

/// What the shell sends when the user confirms the core's illness action.
fn confirm_illness_action(app: &Engine, model: &mut Model) {
    let vm = app.view(model);
    let r = vm
        .hold_resolutions
        .iter()
        .find(|h| h.signal == ReadinessSignal::Illness)
        .expect("one illness action is offered")
        .clone();
    ev(
        app,
        model,
        Event::ResolveHold {
            signal: r.signal,
            observed_at: TODAY * DAY_SEC + 7_200,
        },
    );
    if r.ends_profile_illness {
        ev(app, model, Event::SetProfile(profile(false)));
    }
}

fn illness_actions(vm: &ViewModel) -> Vec<&HoldResolutionView> {
    vm.hold_resolutions
        .iter()
        .filter(|h| h.signal == ReadinessSignal::Illness)
        .collect()
}

#[test]
fn the_profile_illness_answer_offers_the_one_im_better_action() {
    let (app, mut model) = set_up(true);
    let vm = app.view(&model);
    assert!(vm.train_blocked);
    assert_eq!(vm.hold_reason, Some(HoldReason::AcuteIllnessProfile));
    let actions = illness_actions(&vm);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].action_label, "I'm better");
    assert!(actions[0].ends_profile_illness);
    assert_eq!(vm.plan_withheld, Some(PlanWithheld::HealthHold));
    assert!(vm.next_session.is_none());

    confirm_illness_action(&app, &mut model);
    let vm = app.view(&model);
    assert!(!vm.train_blocked);
    assert!(vm.hold_resolutions.is_empty());
    assert_eq!(vm.plan_withheld, None);
    let ns = vm.next_session.expect("the plan resumes today");
    assert_eq!(ns.epoch_day, TODAY);
}

#[test]
fn a_reported_illness_alone_keeps_ive_recovered_and_the_return_check() {
    let (app, mut model) = set_up(false);
    report_flu(&app, &mut model);
    let vm = app.view(&model);
    assert!(vm.train_blocked);
    let actions = illness_actions(&vm);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].action_label, "I've recovered");
    assert!(!actions[0].ends_profile_illness);
    assert_eq!(
        vm.plan_withheld, None,
        "a report blanks sessions, not the plan"
    );

    confirm_illness_action(&app, &mut model);
    let vm = app.view(&model);
    assert!(!vm.train_blocked);
    let ns = vm.next_session.expect("today's session");
    assert_eq!(ns.items.len(), 1);
    assert!(
        ns.items[0].summary.starts_with("Return check"),
        "{}",
        ns.items[0].summary
    );
    assert_eq!(
        model.retracted_inputs.len(),
        1,
        "the report stays in history"
    );
}

#[test]
fn one_tap_ends_a_reported_illness_and_the_profile_answer_together() {
    let (app, mut model) = set_up(true);
    report_flu(&app, &mut model);
    let vm = app.view(&model);
    assert_eq!(vm.hold_reason, Some(HoldReason::AcuteIllnessProfile));
    let actions = illness_actions(&vm);
    assert_eq!(actions.len(), 1, "both illnesses share one action");
    assert_eq!(actions[0].action_label, "I'm better");
    assert!(actions[0].ends_profile_illness);
    assert!(actions[0].confirm_text.contains("easier session"));

    confirm_illness_action(&app, &mut model);
    let vm = app.view(&model);
    assert!(!vm.train_blocked, "no second switch-off in Profile needed");
    assert_eq!(vm.hold_reason, None);
    assert!(vm.hold_resolutions.is_empty());
    assert!(!model.profile.as_ref().unwrap().health.acute_illness);
    let ns = vm.next_session.expect("today's session");
    assert!(
        ns.items[0].summary.starts_with("Return check"),
        "the resolved report still leads into the return check: {}",
        ns.items[0].summary
    );
    assert_eq!(
        model.retracted_inputs.len(),
        1,
        "the report stays in history"
    );
}

#[test]
fn plan_my_training_is_withheld_while_ill_and_works_after() {
    let (app, mut model) = set_up(true);
    ev(&app, &mut model, Event::ClearPlan);
    // A tap while the profile illness holds training could build nothing.
    let vm = app.view(&model);
    assert_eq!(vm.plan_withheld, Some(PlanWithheld::HealthHold));
    ev(
        &app,
        &mut model,
        Event::GeneratePlan {
            start_epoch_day: TODAY,
        },
    );
    assert!(app.view(&model).next_session.is_none());

    confirm_illness_action(&app, &mut model);
    ev(&app, &mut model, Event::ClearPlan);
    let vm = app.view(&model);
    assert_eq!(vm.plan_withheld, None);
    assert!(vm.next_session.is_none(), "the plan was removed");
    ev(
        &app,
        &mut model,
        Event::GeneratePlan {
            start_epoch_day: TODAY,
        },
    );
    assert!(
        app.view(&model).next_session.is_some(),
        "Plan my training builds the plan"
    );
}

#[test]
fn plan_my_training_after_a_reported_illness_builds_the_plan() {
    let (app, mut model) = set_up(false);
    report_flu(&app, &mut model);
    ev(&app, &mut model, Event::ClearPlan);
    assert_eq!(app.view(&model).plan_withheld, None);
    ev(
        &app,
        &mut model,
        Event::GeneratePlan {
            start_epoch_day: TODAY,
        },
    );
    let vm = app.view(&model);
    assert!(vm.program.is_some());
    assert_eq!(
        vm.next_session.expect("the held session renders").status,
        SessionStatus::Blocked
    );
}

#[test]
fn a_profile_with_nothing_to_plan_says_so() {
    let app = Engine;
    let mut model = Model::default();
    ev(
        &app,
        &mut model,
        Event::SetProfile(Profile {
            weekly_sets: 0,
            running_days_per_week: 0,
            ..base_profile()
        }),
    );
    let vm = app.view(&model);
    assert_eq!(vm.plan_withheld, Some(PlanWithheld::NothingToPlan));
    assert!(!vm.train_blocked);
}

#[test]
fn only_a_fever_class_illness_asks_about_fever() {
    // ready-upd-008: a head cold clears once its symptoms are gone; "no fever"
    // is the recovery check of a fever / below-neck illness (ready-upd-004)
    // and of the profile's ill answer.
    let illness_text = |severity: f64, profile_ill: bool| {
        let (app, mut model) = set_up(profile_ill);
        if severity > 0.0 {
            ev(
                &app,
                &mut model,
                Event::SubmitReadiness(ReadinessInput {
                    observed_at: TODAY * DAY_SEC + 3_600,
                    ..input(ReadinessSignal::Illness, severity)
                }),
            );
        }
        let vm = app.view(&model);
        let r = illness_actions(&vm)[0].clone();
        (r.paused_text, r.confirm_text)
    };
    let (paused, confirm) = illness_text(1.0, false);
    assert!(!paused.contains("fever"), "{paused}");
    assert!(!confirm.contains("fever"), "{confirm}");
    assert!(paused.contains("symptoms are gone"), "{paused}");
    for (severity, profile_ill) in [(2.0, false), (0.0, true), (1.0, true)] {
        let (paused, confirm) = illness_text(severity, profile_ill);
        assert!(paused.contains("no fever"), "{paused}");
        assert!(confirm.contains("no fever"), "{confirm}");
    }
}
