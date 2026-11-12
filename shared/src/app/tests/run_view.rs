//! Run cards: zone and pace classification, GPS-derived figures, amends and
//! GPX export.

use super::*;

#[test]
fn logging_a_run_classifies_zone_and_pace() {
    let app = Engine;
    let mut model = Model::default();

    app.update(log_run(10.0, 50.0, 70.0, 12.0, 0, 0), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.runs.len(), 1);
    assert!(
        vm.runs[0].summary.contains("@ 5:00/km"),
        "{}",
        vm.runs[0].summary
    );
    assert!(!vm.runs[0].spike_flag);
    assert_eq!(vm.runs[0].zone, Some(ThreeZone::Z1));
}

#[test]
fn run_zone_strings_match_the_shell_colour_contract() {
    // The view emits the zone as a bare `"Z1"/"Z2"/"Z3"` string (or `"-"`
    // with no HR), and the Kotlin shell's `ThreeZoneOrNoneSerializer` accepts
    // exactly those literals. Renaming the `ThreeZone` variants would compile
    // clean but make the shell reject every view with a zone: this locks the
    // wire strings so such a rename fails here instead.
    let app = Engine;
    let cases = [(70.0, "Z1"), (85.0, "Z2"), (92.0, "Z3"), (0.0, "-")];
    for (hr, expected) in cases {
        let mut model = Model::default();
        app.update(log_run(10.0, 50.0, hr, 12.0, 0, 0), &mut model)
            .expect_only_render();
        let card = serde_json::to_value(&app.view(&model).runs[0]).unwrap();
        assert_eq!(card["zone"], expected, "hr {hr}%");
    }
}

#[test]
fn implausible_hr_pct_max_is_clamped_to_100_in_the_view() {
    // A stale/low measured_hr_max paired with a hard race file can put
    // the imported average %HRmax above 100 (e.g. 104.7). The shell clamps at
    // import, but the core is the single trust boundary and re-clamps: a run
    // whose stored %HRmax exceeds 100 is echoed and classified as exactly 100,
    // so no impossible percentage ever reaches a shell or the load bookkeeping.
    let app = Engine;
    let mut model = Model::default();
    app.update(log_run(10.0, 50.0, 104.7, 12.0, 0, 0), &mut model)
        .expect_only_render();
    let run = &app.view(&model).runs[0];
    assert_eq!(
        run.hr_pct_max, 100.0,
        "104.7% is clamped to the 100% ceiling"
    );
    // 100% is still Z3 (>=88%), the same bucket 104.7% would have landed in,
    // so the clamp corrects the echo without moving the classification.
    assert_eq!(run.zone, Some(ThreeZone::Z3));
}

#[test]
fn zero_duration_run_reports_unknown_pace_not_zero() {
    // A run with distance but zero elapsed time (e.g. GPS fixes sharing a
    // timestamp) must not derive a nonsense "0:00/km" pace.
    let app = Engine;
    let mut model = Model::default();

    app.update(log_run(5.0, 0.0, 70.0, 6.0, 0, 0), &mut model)
        .expect_only_render();

    let summary = &app.view(&model).runs[0].summary;
    assert!(summary.contains("@ - ("), "{summary}");
}

#[test]
fn pace_rounds_to_the_nearest_second_and_zero_pads() {
    // Every displayed pace/clock rounds to the nearest whole second
    // (`format::whole_seconds`). 40.95 min / 10 km = 245.7 s/km renders as
    // "4:06/km". Rounding happens before the minutes/seconds split, so a value
    // that rounds up across a minute reads "1:00", never "0:60".
    let app = Engine;
    let mut model = Model::default();

    app.update(log_run(10.0, 40.95, 70.0, 12.0, 0, 0), &mut model)
        .expect_only_render();

    let summary = &app.view(&model).runs[0].summary;
    assert!(summary.contains("@ 4:06/km"), "{summary}");
    assert_eq!(fmt_mss(59.6), "1:00");
    assert_eq!(fmt_mss(245.4), "4:05");
    assert_eq!(fmt_mss(f64::NAN), "0:00");
    assert_eq!(fmt_mss(-5.0), "0:00");
    assert_eq!(fmt_clock(3599.6), "1:00:00");
    assert_eq!(fmt_clock(3599.4), "59:59");
    // A tiny speed (huge s/km) is not clipped by a 32-bit cast.
    assert_eq!(fmt_mss(1.0e10), "166666666:40");
}

/// The run card summary rounds its distance like every other distance line
/// (`round_tenths`, half away from zero): `{:.1}` on the raw 5.25 km would
/// round the tie half to even and print 5.2 beside a 5.3 elsewhere.
#[test]
fn run_card_summary_rounds_distance_like_every_distance_line() {
    let app = Engine;
    let mut model = Model::default();
    set_today(&app, &mut model, POS_MON);
    app.update(
        log_run(5.25, 30.0, 0.0, 0.0, POS_MON * DAY_SEC + 7 * HOUR, 1),
        &mut model,
    )
    .expect_only_render();
    let summary = &app.view(&model).runs[0].summary;
    assert!(summary.starts_with("5.3km @ "), "{summary}");
}

#[test]
fn gps_tracked_run_derives_distance_and_pace_from_fixes() {
    let app = Engine;
    let mut model = Model::default();

    // ~1.11 km along the equator (0 → 0.01° lon) over 400 s.
    let points = vec![gps(0.0, 0), gps(0.005, 200), gps(0.01, 400)];
    app.update(
        log_run_track(points, 70.0, 12.0, 9_090_001, 9_090_001),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.runs.len(), 1);
    assert!(
        vm.runs[0].summary.starts_with("GPS "),
        "got {}",
        vm.runs[0].summary
    );
    assert!(
        vm.runs[0].summary.contains("1.1km"),
        "got {}",
        vm.runs[0].summary
    );
    assert!(
        !vm.runs[0].summary.contains("@ -"),
        "{}",
        vm.runs[0].summary
    );
    assert!(!vm.runs[0].spike_flag);
    // The ~0.5 MB GPX string never rides the list payload. The run
    // advertises an exportable route via `has_gpx`, and the shell
    // fetches the document ON DEMAND via `export_run_gpx` (the bridge's
    // `ExportGpx` op). Prove both.
    assert!(
        vm.runs[0].has_gpx,
        "a GPS run advertises an exportable route"
    );
    let gpx = export_run_gpx(&model, 9_090_001, 9_090_001);
    assert!(
        gpx.contains("<trkpt "),
        "on-demand GPX carries the fixes: {gpx}"
    );
}

#[test]
fn gps_run_of_only_noise_fixes_offers_no_route() {
    let app = Engine;
    let mut model = Model::default();

    // Every fix is worse than the accuracy gate, so none survive filtering:
    // fewer than two usable points is not a route, and offering an "Export
    // GPX" affordance (has_gpx / a non-empty export) for a 0 km run would be
    // misleading.
    let points = vec![
        GpsPoint {
            accuracy_m: 80.0,
            ..gps(0.0, 0)
        },
        GpsPoint {
            accuracy_m: 120.0,
            ..gps(0.01, 400)
        },
    ];
    app.update(
        log_run_track(points, 70.0, 12.0, 9_115_001, 9_115_001),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.runs.len(), 1);
    // No exportable route for a noise-only track: the flag the shell gates the
    // map and share on is false, and there is no document to fetch.
    assert!(
        !vm.runs[0].has_gpx,
        "has_gpx should be false for a noise-only track"
    );
    let gpx = export_run_gpx(&model, 9_115_001, 9_115_001);
    assert!(gpx.is_empty(), "no route, no document: {gpx}");
    // An unmeasurable track must say so, not render a "0.0km" entry, and must
    // not trip the distance-spike gate against a phantom zero baseline.
    assert!(
        vm.runs[0].summary.contains("too poor"),
        "summary should flag the poor GPS signal, got {}",
        vm.runs[0].summary
    );
    assert!(!vm.runs[0].spike_flag);
}

#[test]
fn gps_run_without_hr_reports_unknown_zone_not_fabricated_z1() {
    let app = Engine;
    let mut model = Model::default();
    let points = vec![gps(0.0, 0), gps(0.01, 400)];
    // hr_pct_max = 0.0 → no HR sample → zone must not be fabricated.
    app.update(log_run_track(points, 0.0, 0.0, 0, 0), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.runs[0].zone, None);
    assert!(
        vm.runs[0].summary.contains("(-)"),
        "got {}",
        vm.runs[0].summary
    );
}

/// A 4-fix GPS run (~330 m over 150 s) ending at `at`, logged as entry `id`.
fn tracked_run(id: u64, at: i64) -> Event {
    let track = (0..4)
        .map(|i| gps(0.001 * i as f64, at - 150 + 50 * i))
        .collect();
    log_run_track(track, 71.0, 3.3, at, id)
}

#[test]
fn amending_a_tracked_run_rebuilds_its_card() {
    // The card is derived once per distinct run; an amend of the same
    // (entry id, stamp) is a new run, so the card shows the amended figures.
    let app = Engine;
    let mut model = Model::default();
    app.update(tracked_run(918_273, 1_000), &mut model)
        .expect_only_render();
    let first = app.view(&model).runs[0].clone();
    assert!(first.has_gpx, "an exportable route is advertised");
    assert!(first.distance_km < 0.5, "{}", first.distance_km);
    assert_eq!(app.view(&model).runs[0], first, "an unchanged run");

    app.update(
        amend_run(918_273, 5.0, 30.0, 70.0, 1_000, 1_000),
        &mut model,
    )
    .expect_only_render();
    let amended = &app.view(&model).runs[0];
    assert_eq!(amended.distance_km, 5.0);
    assert!(amended.summary.contains("@ 6:00/km"), "{}", amended.summary);
}

#[test]
fn gpx_export_reads_only_the_runs_the_model_holds() {
    let app = Engine;
    let mut model = Model::default();
    for id in [700_020, 700_021] {
        app.update(tracked_run(id, 1_000), &mut model)
            .expect_only_render();
    }
    assert!(export_run_gpx(&model, 700_021, 1_000).contains("<trkseg>"));

    app.update(
        Event::DeleteEntry {
            kind: EntryKind::Run,
            entry_id: 700_021,
            observed_at_fallback: 1_000,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(
        export_run_gpx(&model, 700_021, 1_000).is_empty(),
        "the deleted run's route is gone"
    );
    assert!(
        export_run_gpx(&model, 700_020, 1_000).contains("<trkseg>"),
        "the live run's route survives"
    );
}

#[test]
fn gpx_export_of_a_run_that_lost_its_track_is_empty() {
    // A GPS→manual amend keeps (entry_id, observed_at) and drops the track:
    // the export is empty, never the pre-amend route.
    let app = Engine;
    let mut model = Model::default();
    app.update(tracked_run(700_010, 1_000), &mut model)
        .expect_only_render();
    assert!(export_run_gpx(&model, 700_010, 1_000).contains("<trkseg>"));
    app.update(
        amend_run(700_010, 5.0, 30.0, 70.0, 1_000, 1_000),
        &mut model,
    )
    .expect_only_render();
    assert!(export_run_gpx(&model, 700_010, 1_000).is_empty());
}

#[test]
fn run_view_differentiates_interval_from_steady_same_average() {
    // Two GPS runs with identical total distance (~445 m) and duration (120 s)
    // → identical average pace. The interval run mixes hard reps with standing
    // recovery; the steady run holds an even pace. The engine rates them
    // differently (RUN-INTERVAL-VI-001).
    let card = |lons: [f64; 5]| {
        let track = lons
            .iter()
            .zip([0, 30, 60, 90, 120])
            .map(|(&lon, t)| gps(lon, t))
            .collect();
        let (app, model) = replay(&[log_run_track(track, 0.0, 5.0, 120, 0)]);
        app.view(&model).runs.swap_remove(0)
    };
    // Four even ~111 m / 30 s segments (~3.71 m/s throughout).
    let sv = card([0.000, 0.001, 0.002, 0.003, 0.004]);
    // Interval with JOG recovery (recovery legs stay above the 0.5 m/s
    // auto-pause floor, so moving time, and therefore average moving pace,
    // matches the steady run exactly; only the variability differs). Two
    // ~200 m hard reps (~6.7 m/s) each followed by a ~22 m jog (~0.74 m/s).
    let iv = card([0.0000, 0.0018, 0.0020, 0.0038, 0.0040]);

    let pace = |r: &RunResultView| fmt_mss(r.duration_min * 60.0 / r.distance_km);
    assert_eq!(pace(&sv), pace(&iv), "same-average precondition broke");

    let s = sv.interval.expect("steady interval verdict");
    let i = iv.interval.expect("interval verdict");
    let vi =
        |v: &IntervalVerdictView| -> f64 { v.label.rsplit(' ').next().unwrap().parse().unwrap() };
    assert_eq!(s.kind, PaceVariability::Steady, "{}", s.label);
    assert_eq!(i.kind, PaceVariability::Interval, "{}", i.label);
    assert!(vi(&i) > vi(&s), "{} vs {}", i.label, s.label);
    // Honestly graded Weak (flat-ground GOVSS/NGP simplification).
    assert_eq!(i.evidence.grade, Some(EvidenceGrade::Weak));
    assert!(
        i.evidence.citation.contains("Skiba"),
        "got {}",
        i.evidence.citation
    );
    assert!(!i.evidence.safety_critical);
}

#[test]
fn run_splits_carry_numeric_seconds_per_unit_consistent_with_pace() {
    // A splits bar chart needs the numeric per-unit pace; the pre-formatted
    // `m:ss` string can't be a bar length. `seconds_per_unit` carries it.
    let app = Engine;
    let mut model = Model::default();

    // ~2.224 km along the equator (0 → 0.02° lon) at an even ~270 s/km over
    // 600 s → two full-km splits plus a trailing partial.
    let points = vec![
        gps(0.000, 0),
        gps(0.005, 150),
        gps(0.010, 300),
        gps(0.015, 450),
        gps(0.020, 600),
    ];
    app.update(
        log_run_track(points, 70.0, 12.0, 9_091_001, 9_091_001),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    let splits = &vm.runs[0].splits;
    let full: Vec<_> = splits.iter().filter(|s| !s.partial).collect();
    assert_eq!(full.len(), 2, "{splits:?}");
    for s in full {
        assert_eq!(
            (s.seconds_per_unit, s.pace.as_str()),
            (270, "4:30"),
            "split {}",
            s.index
        );
    }
}

#[test]
fn logged_run_track_excludes_the_pause_bridge_and_breaks_the_gpx() {
    // End-to-end over the real event path: a run with a pause +
    // ~111 km relocation. Segment 1 (indices 0–4) then segment 2 (indices
    // 5–9), boundary at index 5. The bridge fix (index 5) is a 1853 m/s
    // "teleport" the QC gate would normally drop: the segment boundary must
    // suppress that gate so segment 2 survives.
    let relocation = vec![
        gps(0.000, 0),
        gps(0.001, 10),
        gps(0.002, 20),
        gps(0.003, 30),
        gps(0.004, 40),
        gps(1.000, 100),
        gps(1.001, 110),
        gps(1.002, 120),
        gps(1.003, 130),
        gps(1.004, 140),
    ];

    // With the boundary: distance is the two ~445 m segments (~0.89 km), NOT
    // the ~111 km bridge, and the on-demand GPX opens two <trkseg>s at the
    // TRUE coords.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        log_run_track_seg(
            relocation.clone(),
            70.0,
            0.0,
            11_203_001,
            11_203_001,
            vec![5],
        ),
        &mut model,
    )
    .expect_only_render();
    let run = &app.view(&model).runs[0];
    assert!(
        run.distance_km < 5.0,
        "bridge excluded: {}",
        run.distance_km
    );
    assert!(
        (run.distance_km - 0.889).abs() < 0.05,
        "distance {}",
        run.distance_km
    );
    assert!(
        run.has_gpx,
        "an exportable route survives the pause boundary"
    );
    // The two-segment GPX is pinned on the on-demand export.
    let gpx = export_run_gpx(&model, 11_203_001, 11_203_001);
    assert_eq!(gpx.matches("<trkseg>").count(), 2, "two segments in GPX");
    assert!(gpx.contains("lon=\"1\""), "true seg-2 coord kept");

    // WITHOUT the boundary over these SAME true coords, the QC gate rejects
    // the ~1853 m/s bridge as a teleport and drops ALL of segment 2 with it -
    // so distance collapses to segment 1 alone (~0.445 km) and the relocated
    // half of the run is LOST. The boundary is exactly what lets the core keep
    // segment 2 (real distance) while still excluding the bridge.
    let mut naive = Model::default();
    app.update(log_run_track(relocation, 70.0, 0.0, 1000, 2), &mut naive)
        .expect_only_render();
    let naive_run = &app.view(&naive).runs[0];
    assert!(
        naive_run.distance_km < 0.6,
        "no boundary → segment 2 dropped as a teleport: {}",
        naive_run.distance_km,
    );
}
