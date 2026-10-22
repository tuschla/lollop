//! A mis-imported future-dated run (e.g. a phantom ~2030 import) never poses as
//! "the most recent session": the weekly report, training load and the two
//! feedback fallbacks (`latest_run_spike`, `latest_track_split`) all stop at
//! `end_of_local_today_sec`, so it neither injects a bogus figure nor, with no
//! real neighbours in its window, silently disarms a genuine spike/split cue.

use super::*;

#[test]
fn a_future_dated_run_is_excluded_from_the_spike_fallback() {
    let app = Engine;
    let mut model = Model::default();
    let today = 1000i64;
    app.update(
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
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
    let frac = latest_run_spike(&model, &RunFacts::new(&model.runs))
        .expect("the real today run, not the phantom future run, drives the spike")
        .frac;
    assert!(
        (frac - 0.2).abs() < 1e-9,
        "12 km vs the 10 km baseline is a +20% spike, got {frac}"
    );
}

#[test]
fn a_future_dated_tracked_run_is_excluded_from_the_split_fallback() {
    // A fast front half then a back half ~2x slower → a measurable positive
    // split (the run-view split fixture, stretched to ~4.4 km so the run is
    // long enough for a pacing verdict).
    let split_track = || {
        [
            GpsPoint {
                lat: 0.0,
                lon: 0.000,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.001,
                observed_at: 20,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.002,
                observed_at: 40,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.003,
                observed_at: 90,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.004,
                observed_at: 140,
                accuracy_m: 5.0,
            },
        ]
        .map(|p| GpsPoint {
            lon: p.lon * 10.0,
            observed_at: p.observed_at * 15,
            ..p
        })
        .to_vec()
    };
    let today = 1000i64;

    // Dated today: the real tracked run feeds the split fallback.
    let app = Engine;
    let mut today_model = Model::default();
    app.update(
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
        &mut today_model,
    )
    .expect_only_render();
    app.update(
        log_run_track(split_track(), 78.0, 12.0, today * DAY_SEC, 1),
        &mut today_model,
    )
    .expect_only_render();
    assert!(
        latest_track_split(&today_model).is_some(),
        "a real tracked run feeds the positive-split fallback"
    );

    // The SAME run mis-dated to the future must not: no real run backs it, so
    // the fallback goes quiet instead of surfacing the phantom's split.
    let mut future_model = Model::default();
    app.update(
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
        &mut future_model,
    )
    .expect_only_render();
    app.update(
        log_run_track(split_track(), 78.0, 12.0, (today + 100) * DAY_SEC, 1),
        &mut future_model,
    )
    .expect_only_render();
    assert_eq!(
        latest_track_split(&future_model),
        None,
        "a future-dated tracked run is excluded from the split fallback"
    );
}

// ── A future-dated run must not hijack weekly report or CTL/ATL ──
#[test]
fn a_future_dated_run_does_not_hijack_weekly_report_or_training_load() {
    let app = Engine;
    let mut model = Model::default();
    // Real recent history: a 10 km HR run.
    app.update(log_run(10.0, 60.0, 75.0, 0.0, 100 * DAY_SEC, 1), &mut model)
        .expect_only_render();
    // A mis-imported 2030-dated GPX (far future).
    app.update(
        log_run(5.0, 30.0, 60.0, 0.0, 22_000 * DAY_SEC, 2),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: 105,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();

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
    assert_eq!((tl.ctl, tl.atl, tl.tsb), (tl2.ctl, tl2.atl, tl2.tsb));
    assert_eq!(tl.sessions_counted, tl2.sessions_counted);
}
