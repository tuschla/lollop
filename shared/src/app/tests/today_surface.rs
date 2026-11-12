//! Today's surface facts as the view carries them: the day's state, each run
//! session's run item and the repeat marks the shell's card stack reads
//! instead of comparing card text.

use super::*;
use crate::app::today_surface::GUIDED_LIFT_DAYS;
use crate::app::view_types::{HeadlineKind, SessionStatus, TodayState};

fn state(app: &Engine, model: &Model) -> TodayState {
    app.view(model).today_surface.state
}

/// Log `day`'s prescribed lift sets until its week-strip row reads done.
fn finish_lift_day(app: &Engine, model: &mut Model, day: i64) {
    for _ in 0..40 {
        let row = day_row(app, model, day);
        if row.status == SessionStatus::Done {
            return;
        }
        for it in row
            .items
            .iter()
            .filter(|i| !i.done && !i.exercise.is_empty())
        {
            let reps = u32::from(it.reps_low.max(1));
            let load = it.load_kg.unwrap_or(100.0);
            app.update(
                log_set(&it.exercise, load, reps, 8.0, day * DAY_SEC + 10 * HOUR),
                model,
            )
            .expect_only_render();
        }
    }
    panic!("day {day} should complete");
}

#[test]
fn todays_state_is_the_widgets_and_todays_gates() {
    // No plan: setup without a profile, else the plan prompt.
    let app = Engine;
    let mut model = Model::default();
    assert_eq!(state(&app, &model), TodayState::Setup);
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    assert_eq!(state(&app, &model), TodayState::NoPlan);

    // Today (MON) is the plan's first session; once its sets are logged the
    // hero moves on and today's row says what was done.
    let (app, mut model) = planned_strength_model(true);
    assert_eq!(state(&app, &model), TodayState::Session);
    finish_lift_day(&app, &mut model, MON);
    assert_eq!(state(&app, &model), TodayState::Done);
    app.update(Event::ClearPlan, &mut model)
        .expect_only_render();
    assert_eq!(state(&app, &model), TodayState::NoPlan);

    // A hold wins over any plan.
    let (app, mut model) = planned_strength_model(true);
    app.update(
        Event::SubmitReadiness(pain_row(1.0, None, MON * DAY_SEC + HOUR)),
        &mut model,
    )
    .expect_only_render();
    assert_eq!(state(&app, &model), TodayState::Paused);

    // A plan rest day, or no plan day today at all (before the plan starts).
    let (app, mut model) = planned_strength_model(true);
    assert_eq!(day_row(&app, &model, MON + 1).status, SessionStatus::Rest);
    set_today(&app, &mut model, MON + 1);
    let surface = app.view(&model).today_surface;
    assert_eq!(surface.state, TodayState::Rest);
    assert!(surface.rest_today);
    set_today(&app, &mut model, MON - 1);
    let surface = app.view(&model).today_surface;
    assert_eq!(surface.state, TodayState::Rest);
    assert!(!surface.rest_today);
}

#[test]
fn the_guide_shows_until_three_days_with_a_logged_lift() {
    let at = |d: i64| d * DAY_SEC + 10 * HOUR;
    let day = 20_400;
    // One logged day short of the guided count: one of those days holds two
    // sets, and an undated set belongs to no day.
    let mut events: Vec<Event> = (1..GUIDED_LIFT_DAYS as i64)
        .map(|k| log_set("Back squat", 100.0, 5, 8.0, at(day - 2 * k)))
        .collect();
    events.push(log_set("Bench", 60.0, 5, 8.0, at(day - 2)));
    events.push(log_set("Row", 50.0, 8, 8.0, 0));
    let (app, model) = replay(&events);
    assert!(app.view(&model).today_surface.show_guide);
    events.push(log_set("Deadlift", 140.0, 3, 8.0, at(day - 1)));
    let (app, model) = replay(&events);
    assert!(!app.view(&model).today_surface.show_guide);
}

#[test]
fn a_run_sessions_run_item_is_its_first_open_item_with_a_target() {
    let (app, mut model) = planned_on(running_profile(), [], MON);
    let vm = app.view(&model);
    let ns = vm.next_session.as_ref().expect("today's run");
    assert!(matches!(ns.session_type, SessionType::Run(_)));
    assert_eq!(ns.run_item, Some(0));
    assert!(ns.items[0].run_target.is_some());
    for s in &vm.week_plan {
        let expected = matches!(s.session_type, SessionType::Run(_)).then_some(0);
        assert_eq!(s.run_item, expected, "day {}", s.epoch_day);
    }
    // Today's run logged: no open item is left to track.
    app.update(logrun_at(5.0, MON * DAY_SEC + 8 * HOUR), &mut model)
        .expect_only_render();
    assert_eq!(day_row(&app, &model, MON).run_item, None);

    // A lifting session has no run item.
    let (app, model) = planned_strength_model(true);
    assert_eq!(app.view(&model).next_session.unwrap().run_item, None);
}

#[test]
fn a_return_check_item_owns_its_sessions_adjustment() {
    // Recovered from a flu: today's session is the return check, and its
    // card states the adjustment itself.
    let (app, mut model) = planned_strength_model_on(POS_MON, true);
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            observed_at: POS_MON * DAY_SEC + HOUR,
            ..input(ReadinessSignal::Illness, 2.0)
        }),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::ResolveHold {
            signal: ReadinessSignal::Illness,
            observed_at: POS_MON * DAY_SEC + 2 * HOUR,
        },
        &mut model,
    )
    .expect_only_render();
    let ns = app.view(&model).next_session.expect("today's session");
    assert!(ns.items.iter().any(|i| i.return_check));
    assert!(ns.adjustment.is_some());
    assert!(ns.adjustment_on_item);

    // An HRV downgrade adjusts the session without a return-check item.
    let (app, mut model) = planned_strength_model(true);
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::HrvLnRmssd, -1.0)),
        &mut model,
    )
    .expect_only_render();
    let ns = app.view(&model).next_session.expect("today's session");
    assert!(ns.adjustment.is_some());
    assert!(!ns.adjustment_on_item);
}

#[test]
fn rows_the_headline_or_the_sessions_adjustment_state_are_marked() {
    // An HRV dip is the headline, the session's adjustment and the one row.
    let (app, mut model) = planned_strength_model(true);
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::HrvLnRmssd, -1.0)),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert_eq!(vm.today_headline.kind, HeadlineKind::Adjustment);
    assert!(vm.today_headline.repeats_session);
    let marks = |rows: &[AdjustmentView]| {
        rows.iter()
            .map(|a| (a.repeats_headline, a.repeats_session))
            .collect::<Vec<_>>()
    };
    assert_eq!(marks(&vm.adjustments), [(true, true)]);

    // A weekly deload then leads the headline; the HRV row still states the
    // session's adjustment, the deload row only the headline.
    app.update(
        Event::SubmitReview(SessionReview {
            rpe_load_gap_sessions: Some(2),
            ..Default::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.today_headline.summary.starts_with("Lighter week"));
    assert!(!vm.today_headline.repeats_session);
    assert_eq!(marks(&vm.adjustments), [(false, true)]);
    assert_eq!(marks(&vm.review_adjustments), [(true, false)]);

    // Only an adjustment headline states a row: a pain hold's safety headline
    // has the pain row's words but is not that row.
    let (app, mut model) = planned_strength_model(true);
    app.update(
        Event::SubmitReadiness(pain_row(1.0, None, MON * DAY_SEC + HOUR)),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert_eq!(vm.today_headline.kind, HeadlineKind::SafetyHold);
    let pain = vm
        .adjustments
        .iter()
        .find(|a| a.summary == vm.today_headline.summary)
        .expect("the pain row the headline words");
    assert!(!pain.repeats_headline && !pain.repeats_session);
}
