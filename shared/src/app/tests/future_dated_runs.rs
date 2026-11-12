//! A mis-imported future-dated run (e.g. a phantom ~2030 import) never poses as
//! "the most recent session": the weekly report, training load and the
//! spike/split feedback cues all stop at the end of local today
//! (`Model::up_to_today`), so it neither injects a bogus figure nor, with no
//! real neighbours in its window, silently disarms a genuine spike/split cue.

use super::*;

#[test]
fn a_future_dated_run_does_not_disarm_the_spike_cue() {
    let app = Engine;
    let mut model = Model::default();
    let today = 1000i64;
    set_today(&app, &mut model, today);
    // A 10 km baseline 20 days ago and a real 12 km run today → +20% spike.
    app.update(
        log_run_at(10.0, 50.0, 75.0, (today - 20) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(log_run_at(12.0, 60.0, 75.0, today * DAY_SEC), &mut model)
        .expect_only_render();
    // A phantom future import (mis-timestamped ~2030): 50 km, whose 30-day
    // trailing window holds no other run. Unguarded it is picked as the latest
    // run and returns None (no baseline), disarming the real spike above.
    app.update(
        log_run_at(50.0, 200.0, 75.0, (today + 100) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(Event::SubmitReview(SessionReview::default()), &mut model)
        .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!(
        fb.category,
        FeedbackCategory::DangerousProgression,
        "the real today run, not the phantom future run, drives the spike: {fb:?}"
    );
}

#[test]
fn a_future_dated_tracked_run_does_not_hijack_the_split_cue() {
    // A fast front half then a back half ~2x slower → a measurable positive
    // split over ~4.4 km, long enough for a pacing verdict, run from 08:00 of
    // `day` as entry `id`.
    let today = 1000i64;
    let split_run = |day: i64, id: u64| {
        let t0 = day * DAY_SEC + 8 * HOUR;
        let track = [
            (0.00, 0),
            (0.01, 300),
            (0.02, 600),
            (0.03, 1350),
            (0.04, 2100),
        ]
        .map(|(lon, t)| gps(lon, t0 + t))
        .to_vec();
        log_run_track(track, 78.0, 12.0, t0 + 2100, id)
    };
    // The feedback after `runs` and a review at 10:00 today.
    let reviewed = |runs: &[Event]| {
        let mut events = vec![set_today_ev(today)];
        events.extend_from_slice(runs);
        events.push(Event::SubmitReview(SessionReview {
            observed_at: today * DAY_SEC + 10 * HOUR,
            ..Default::default()
        }));
        let (app, model) = replay(&events);
        app.view(&model).feedback.expect("feedback present")
    };

    // Dated today: the real tracked run feeds the split cue, and a phantom
    // future import does not pose as the latest tracked run in its place.
    for runs in [
        vec![split_run(today, 1)],
        vec![split_run(today, 1), split_run(today + 100, 2)],
    ] {
        let fb = reviewed(&runs);
        assert_eq!(
            fb.category,
            FeedbackCategory::IntensityDiscipline,
            "the real tracked run feeds the positive-split cue: {fb:?}"
        );
    }

    // The SAME run mis-dated to the future alone gives no cue: no real run
    // backs it.
    let fb = reviewed(&[split_run(today + 100, 2)]);
    assert_ne!(
        fb.category,
        FeedbackCategory::IntensityDiscipline,
        "a future-dated tracked run is excluded from the split cue: {fb:?}"
    );
}

#[test]
fn a_future_dated_run_does_not_hijack_weekly_report_or_training_load() {
    let app = Engine;
    let mut model = Model::default();
    app.update(log_run(10.0, 60.0, 75.0, 0.0, 100 * DAY_SEC, 1), &mut model)
        .expect_only_render();
    // A mis-imported 2030-dated GPX (far future).
    app.update(
        log_run(5.0, 30.0, 60.0, 0.0, 22_000 * DAY_SEC, 2),
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, 105);

    let with_future = app.view(&model);
    // Training load counts only the real run: the 2030 row is skipped, so the
    // MAX_LOAD_DAYS window can't slide off the real history.
    let tl = with_future
        .training_load
        .clone()
        .expect("training load present");
    assert_eq!(
        tl.sessions_counted, 1,
        "the future run must be excluded from CTL/ATL"
    );

    // Deleting the future run leaves BOTH surfaces identical, proof it
    // was fully ignored while present.
    app.update(
        Event::DeleteEntry {
            kind: EntryKind::Run,
            entry_id: 2,
            observed_at_fallback: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let without = app.view(&model);
    assert_eq!(
        with_future.weekly_report, without.weekly_report,
        "the 2030 run must not affect the weekly report"
    );
    // CTL/ATL/TSB + counted are identical whether or not the future run is
    // present: it only ever contributes to the `skipped` tally.
    let tl2 = without.training_load.expect("training load present");
    assert_eq!(tl.summary, tl2.summary);
    assert_eq!(tl.sessions_counted, tl2.sessions_counted);
}
