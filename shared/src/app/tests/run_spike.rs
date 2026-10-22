//! The single-session distance spike gate: the baseline is the longest other
//! run in the window (RUN-SPIKE-001), derived from logged and GPS runs.

use super::*;

#[test]
fn spike_summary_explains_a_missing_baseline_vs_a_real_jump() {
    // The spike gate errs safe with no history, so the first run trips it. Its
    // summary must not claim a ">10%" jump over a baseline that does not exist;
    // a later run that genuinely exceeds the prior longest gets the ">10%"
    // wording. Both still set spike_flag, only the wording differs.
    let app = Engine;
    let mut model = Model::default();

    app.update(log_run(5.0, 25.0, 70.0, 0.0, 0, 0), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.runs[0].spike_flag);
    assert!(
        vm.runs[0].summary.contains("No earlier run"),
        "first run should explain the missing baseline, got: {}",
        vm.runs[0].summary
    );
    // Structured fields a shell renders instead of re-parsing `summary`.
    assert_eq!(vm.runs[0].distance_km, 5.0);
    assert!(
        vm.runs[0].spike_note.contains("No earlier run"),
        "spike_note should carry the honest reason, got: {}",
        vm.runs[0].spike_note
    );

    app.update(log_run(20.0, 100.0, 70.0, 0.0, 0, 0), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.runs[1].spike_flag);
    assert!(
        vm.runs[1]
            .summary
            .contains("Longer than your recent longest run"),
        "a real jump over the prior longest should say so plainly, got: {}",
        vm.runs[1].summary
    );
}

#[test]
fn manual_run_spike_baseline_is_derived_from_prior_runs() {
    // Same guarantee as the GPS path: a manual run's spike gate floors its
    // baseline to the longest run already held, even when the caller sends 0.
    let app = Engine;
    let mut model = Model::default();

    app.update(log_run(20.0, 100.0, 70.0, 0.0, 0, 0), &mut model)
        .expect_only_render();

    // 10 km is well under the derived 20 km baseline → no spike.
    app.update(log_run(10.0, 50.0, 70.0, 0.0, 0, 0), &mut model)
        .expect_only_render();

    assert!(!app.view(&model).runs[1].spike_flag);
}

#[test]
fn manual_run_spike_baseline_reads_a_prior_gps_runs_derived_distance() {
    // Cross-type baseline: a GPS run stores distance_km = 0.0 (it derives from
    // the track), so a later manual run's spike gate only sees a real baseline
    // if `ingest_spike_baseline_km` reconstructs the GPS run's distance from its fixes.
    // If that derivation regressed to the stored 0.0, the baseline would be 0,
    // the safe-with-no-history gate would fire, and this 1 km run would flag -
    // so asserting NO spike pins the GPS branch of the fold.
    let app = Engine;
    let mut model = Model::default();

    // GPS run: ~1.11 km along the equator (0 → 0.01° longitude).
    app.update(
        log_run_track(
            vec![
                GpsPoint {
                    lat: 0.0,
                    lon: 0.0,
                    observed_at: 0,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.01,
                    observed_at: 400,
                    accuracy_m: 5.0,
                },
            ],
            0.0,
            0.0,
            0,
            0,
        ),
        &mut model,
    )
    .expect_only_render();

    // Manual 1 km run: under the ~1.11 km baseline derived from the GPS track,
    // so no spike, but only because the baseline came from the GPS run.
    app.update(log_run(1.0, 6.0, 70.0, 0.0, 0, 0), &mut model)
        .expect_only_render();

    assert!(!app.view(&model).runs[1].spike_flag);
}

#[test]
fn run_distance_spike_is_flagged() {
    let app = Engine;
    let mut model = Model::default();

    app.update(log_run(20.0, 100.0, 75.0, 12.0, 0, 0), &mut model)
        .expect_only_render();

    assert!(app.view(&model).runs[0].spike_flag);
}

#[test]
fn gps_spike_baseline_is_derived_from_prior_runs() {
    let app = Engine;
    let mut model = Model::default();
    let one_km = || {
        vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.0,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.01,
                observed_at: 400,
                accuracy_m: 5.0,
            },
        ]
    };

    // Run 1: ~1.11 km with no history. The core spike gate errs safe with no
    // baseline, so this first run is flagged, expected and intentional.
    app.update(log_run_track(one_km(), 0.0, 0.0, 0, 0), &mut model)
        .expect_only_render();
    assert!(app.view(&model).runs[0].spike_flag);

    // Run 2: same ~1.11 km. Shell again sends baseline 0.0, but the core now
    // derives the baseline from run 1 (~1.11 km): 1.11 is not >10 % over
    // itself, so NO spike. This only passes if the baseline came from history.
    app.update(log_run_track(one_km(), 0.0, 0.0, 0, 0), &mut model)
        .expect_only_render();
    assert!(!app.view(&model).runs[1].spike_flag);

    // Run 3: ~2.22 km, nearly double the derived baseline → spike.
    let long = vec![
        GpsPoint {
            lat: 0.0,
            lon: 0.0,
            observed_at: 0,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.02,
            observed_at: 800,
            accuracy_m: 5.0,
        },
    ];
    app.update(log_run_track(long, 0.0, 0.0, 0, 0), &mut model)
        .expect_only_render();
    assert!(app.view(&model).runs[2].spike_flag);
}

#[test]
fn run_view_carries_structured_spike_baseline_flag() {
    // The spike-baseline provenance is a structured bool, not prose for a
    // shell to scrape out of `spike_note` ("no prior run"). A first run (no
    // baseline) flags has_baseline=false; a run with a prior baseline is true.
    let mut no_baseline = probe_run(1);
    no_baseline.longest_recent_km = 0.0;
    let v = run_card(&no_baseline);
    assert!(
        v.spike_flag && !v.spike_has_baseline,
        "first run: no baseline"
    );
    assert!(
        v.spike_note.contains("No earlier run"),
        "prose kept for compat"
    );

    let mut with_baseline = probe_run(2);
    with_baseline.longest_recent_km = 3.0; // a short run vs a 3 km baseline
    let v2 = run_card(&with_baseline);
    assert!(v2.spike_has_baseline, "a prior baseline exists");
}

#[test]
fn run_view_carries_full_spike_gate_evidence() {
    // The row carries grade/confidence/safety_critical/contested, not just the
    // citation: parity with the other evidence-bearing view structs.
    let app = Engine;
    let mut model = Model::default();
    app.update(log_run(12.0, 60.0, 70.0, 10.0, 0, 0), &mut model)
        .expect_only_render();
    let run = &app.view(&model).runs[0];
    assert!(run.spike_flag, "20% over baseline must flag");
    assert!(run.grade.is_some(), "grade populated");
    assert!(!run.citation.is_empty(), "citation populated");
    assert!(run.confidence > 0.0, "confidence populated");
    // RUN-SPIKE-001 is the strongest running injury signal: its
    // safety-critical marker must survive the flattening.
    assert!(run.safety_critical, "spike gate is safety-critical");
}

#[test]
fn logged_distance_spike_drives_the_safety_gate_without_a_manual_figure() {
    // A logged run 20 % over the recent-longest baseline is the strongest
    // running injury signal (RUN-SPIKE-001). A plain review that carries no
    // explicit spike figure must still trip the safety gate off the logged
    // run, so the deferral is reachable from ordinary logging. The gate
    // now derives its baseline from LOGGED history (`model.runs`), so the
    // spike must be demonstrable against a real prior run: a 10 km baseline
    // run, then a 12 km run (+20 %).
    let app = Engine;
    let mut model = Model::default();
    app.update(log_run(10.0, 50.0, 70.0, 0.0, DAY_SEC, 0), &mut model)
        .expect_only_render();
    app.update(log_run(12.0, 60.0, 70.0, 0.0, 20 * DAY_SEC, 0), &mut model)
        .expect_only_render();
    app.update(Event::SubmitReview(SessionReview::default()), &mut model)
        .expect_only_render();

    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!(fb.category, FeedbackCategory::DangerousProgression);
}

#[test]
fn a_first_run_with_no_baseline_does_not_trip_the_spike_safety_gate() {
    // With no recent-longest baseline the run view flags the distance
    // descriptively, but there is nothing for the run to be a spike *over*,
    // so the safety gate must not defer; otherwise every first-ever run
    // would raise a dangerous-progression deferral.
    let app = Engine;
    let mut model = Model::default();
    app.update(log_run(12.0, 60.0, 70.0, 0.0, 0, 0), &mut model)
        .expect_only_render();
    app.update(Event::SubmitReview(SessionReview::default()), &mut model)
        .expect_only_render();

    let fb = app.view(&model).feedback.expect("feedback present");
    assert_ne!(fb.category, FeedbackCategory::DangerousProgression);
}

// ── Deleting the baseline run re-arms the derived spike gate ──
#[test]
fn deleting_the_baseline_run_re_arms_the_spike_gate() {
    let app = Engine;
    let mut model = Model::default();
    // 20 km, then a 30 km typo, then a 23 km run.
    app.update(log_run(20.0, 100.0, 70.0, 0.0, DAY_SEC, 1), &mut model)
        .expect_only_render();
    app.update(log_run(30.0, 150.0, 70.0, 0.0, 5 * DAY_SEC, 2), &mut model)
        .expect_only_render();
    app.update(log_run(23.0, 115.0, 70.0, 0.0, 10 * DAY_SEC, 3), &mut model)
        .expect_only_render();
    app.update(Event::SubmitReview(SessionReview::default()), &mut model)
        .expect_only_render();

    // With the 30 km baseline standing, the latest 23 km run is not a spike.
    let before = app.view(&model).feedback.expect("feedback present");
    assert_ne!(
        before.category,
        FeedbackCategory::DangerousProgression,
        "23 km is not a spike over the standing 30 km baseline"
    );

    // Delete the 30 km typo: the true recent longest is now 20 km, so 23 km is
    // a +15 % spike: the DERIVED gate re-arms (the stale stored 30 would not).
    app.update(
        Event::DeleteEntry {
            kind: EntryKind::Run,
            entry_id: 2,
            observed_at_fallback: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let after = app.view(&model).feedback.expect("feedback present");
    assert_eq!(
        after.category,
        FeedbackCategory::DangerousProgression,
        "deleting the 30 km baseline must re-arm the spike gate"
    );
}

// ── The run-spike baseline is a trailing 30-day window, not all-time ──
#[test]
fn old_long_run_does_not_suppress_a_current_spike() {
    let app = Engine;
    let mut model = Model::default();
    // A 40 km run 60 days ago (outside the 30-day window).
    app.update(
        log_run(40.0, 240.0, 70.0, 0.0, 100 * DAY_SEC, 0),
        &mut model,
    )
    .expect_only_render();
    // A 10 km run today (day 160): its baseline must NOT include the old 40 km.
    app.update(log_run(10.0, 60.0, 75.0, 0.0, 160 * DAY_SEC, 0), &mut model)
        .expect_only_render();
    let today = model.runs.last().unwrap();
    assert!(
        today.longest_recent_km < 40.0,
        "an all-time baseline leaked in: {}",
        today.longest_recent_km
    );
}
