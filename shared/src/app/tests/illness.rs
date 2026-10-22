//! An active illness report governs every upcoming plan day until the user
//! reports recovery, not only today's session (ILLNESS-NECK-001; IOC 2022
//! consensus, ILLNESS-COLD-RUN-001 / ILLNESS-RTT-001): a head cold turns every
//! run into one short easy run (10–20 min) or rest, flu or fever means no
//! training on any upcoming day, and "I've recovered" leads into the return
//! check before the plan resumes.

use super::*;

/// A half-marathon running plan (55 km/wk, so the long run is a real long
/// run) with today set to the rest day right before the long run. Returns the
/// long-run epoch day.
fn day_before_the_long_run() -> (Engine, Model, i64) {
    let app = Engine;
    let mut model = Model::default();
    let p = Profile {
        running_km_per_week: 55.0,
        ..running_profile()
    };
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    set_day(&app, &mut model, MON);
    let long_day = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| s.title == "Long run")
        .expect("a running week has a long run")
        .epoch_day;
    assert!(long_day > MON, "fixture: the long run is not on Monday");
    set_day(&app, &mut model, long_day - 1);
    let vm = app.view(&model);
    let eve = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == long_day - 1)
        .expect("the day before the long run is in the week");
    assert_eq!(
        eve.status,
        SessionStatus::Rest,
        "fixture: the long run follows a rest day"
    );
    let ns = vm.next_session.expect("the long run is the next session");
    assert_eq!(ns.epoch_day, long_day);
    assert!(
        !ns.items[0].summary.starts_with("10–20 min"),
        "fixture: the planned long run is a real long run: {}",
        ns.items[0].summary
    );
    (app, model, long_day)
}

fn set_day(app: &Engine, model: &mut Model, epoch_day: i64) {
    app.update(
        Event::SetToday {
            epoch_day,
            utc_offset_sec: 0,
        },
        model,
    )
    .expect_only_render();
}

fn report_illness(app: &Engine, model: &mut Model, value: f64, at: i64) {
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            observed_at: at,
            ..input(ReadinessSignal::Illness, value)
        }),
        model,
    )
    .expect_only_render();
}

#[test]
fn a_head_cold_today_turns_tomorrows_long_run_into_a_short_easy_run() {
    let (app, mut model, long_day) = day_before_the_long_run();
    report_illness(&app, &mut model, 1.0, (long_day - 1) * DAY_SEC + 3_600);
    let vm = app.view(&model);
    assert!(
        !vm.train_blocked,
        "a head cold eases training, never blocks it"
    );
    assert!(
        vm.hold_resolutions
            .iter()
            .any(|h| h.signal == ReadinessSignal::Illness)
    );
    assert!(
        vm.today_headline.summary.starts_with("Head cold"),
        "{}",
        vm.today_headline.summary
    );

    let ns = vm.next_session.clone().expect("tomorrow's session");
    assert_eq!(ns.epoch_day, long_day, "tomorrow is still the next session");
    assert_eq!(ns.status, SessionStatus::Adjusted);
    assert_eq!(ns.items.len(), 1);
    let run = &ns.items[0];
    assert!(
        run.summary.starts_with("10–20 min · Easy"),
        "no long run: a short easy run within the cap, got {}",
        run.summary
    );
    assert!(
        !run.summary.contains("km"),
        "no distance shown: {}",
        run.summary
    );
    assert_eq!(run.rep_count, 0);
    assert!(
        run.run_structure.is_none(),
        "the planned structure is dropped"
    );
    assert!(
        run.adjusted_note.contains("Head cold"),
        "{}",
        run.adjusted_note
    );
    assert!(
        run.evidence.citation.contains("Schwellnus"),
        "{}",
        run.evidence.citation
    );
    assert!(!run.done, "nothing logged yet");

    // Every upcoming run day in the week strip carries the same cap; past
    // days and today are untouched.
    for row in vm.week_plan.iter().filter(|s| s.epoch_day > long_day - 1) {
        for it in row.items.iter().filter(|i| i.exercise.is_empty()) {
            assert!(
                it.summary.starts_with("10–20 min"),
                "day {} still shows {}",
                row.epoch_day,
                it.summary
            );
        }
    }
}

#[test]
fn a_short_run_under_a_head_cold_completes_the_day() {
    let (app, mut model, long_day) = day_before_the_long_run();
    report_illness(&app, &mut model, 1.0, (long_day - 1) * DAY_SEC + 3_600);
    set_day(&app, &mut model, long_day);
    app.update(
        log_run_at(2.5, 15.0, 60.0, long_day * DAY_SEC + 9 * 3_600),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let row = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == long_day)
        .expect("the long-run day row");
    assert_eq!(
        row.status,
        SessionStatus::Done,
        "the short easy run as shown completes the day: {:?}",
        row.items
            .iter()
            .map(|i| (&i.summary, i.done))
            .collect::<Vec<_>>()
    );
}

#[test]
fn flu_rests_every_upcoming_day_then_the_return_check_then_the_plan() {
    let (app, mut model, long_day) = day_before_the_long_run();
    let today = long_day - 1;
    report_illness(&app, &mut model, 2.0, today * DAY_SEC + 3_600);
    let vm = app.view(&model);
    assert!(vm.train_blocked);
    let recovered = vm
        .hold_resolutions
        .iter()
        .find(|h| h.signal == ReadinessSignal::Illness)
        .expect("the 'I've recovered' path is offered");
    assert_eq!(recovered.action_label, "I've recovered");
    assert!(
        vm.today_headline.summary.starts_with("Flu or fever"),
        "{}",
        vm.today_headline.summary
    );
    let ns = vm.next_session.clone().expect("the next session renders");
    assert_eq!(
        ns.status,
        SessionStatus::Blocked,
        "the next session is rest"
    );
    assert!(ns.items.is_empty(), "no run distance is shown");
    for row in vm.week_plan.iter().filter(|s| s.epoch_day > today) {
        assert!(
            row.items.is_empty(),
            "day {} still shows {:?}",
            row.epoch_day,
            row.items.iter().map(|i| &i.summary).collect::<Vec<_>>()
        );
        assert!(row.move_targets.is_empty());
    }

    // "I've recovered" on the rest day: tomorrow's session is the return check.
    app.update(
        Event::ResolveHold {
            signal: ReadinessSignal::Illness,
            observed_at: today * DAY_SEC + 7_200,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(!vm.train_blocked);
    let ns = vm.next_session.expect("tomorrow's session");
    assert_eq!(ns.epoch_day, long_day);
    assert_eq!(ns.items.len(), 1, "one return-check item");
    assert!(
        ns.items[0].summary.starts_with("Return check"),
        "{}",
        ns.items[0].summary
    );
    assert!(
        ns.items[0].return_check,
        "the item says it is the return check"
    );

    // The return check is done on the long-run day: the plan resumes.
    set_day(&app, &mut model, long_day);
    app.update(
        log_run_at(2.5, 15.0, 60.0, long_day * DAY_SEC + 9 * 3_600),
        &mut model,
    )
    .expect_only_render();
    set_day(&app, &mut model, long_day + 1);
    let vm = app.view(&model);
    let ns = vm.next_session.expect("the next planned session");
    assert!(ns.epoch_day > long_day);
    assert_eq!(ns.status, SessionStatus::Next, "a normal session again");
    assert!(ns.adjustment.is_none());
    for it in &ns.items {
        assert!(!it.summary.starts_with("Return check"), "{}", it.summary);
        assert!(!it.return_check);
        assert!(it.adjusted_note.is_empty(), "{}", it.adjusted_note);
    }
}
