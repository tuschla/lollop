//! Run cards: zone and pace classification, GPS-derived figures, GPX export
//! and the run-view / GPX memo caches.

use super::*;

#[test]
fn logging_a_run_classifies_zone_and_pace() {
    let app = Engine;
    let mut model = Model::default();

    app.update(log_run(10.0, 50.0, 70.0, 12.0, 0, 0), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.runs.len(), 1);
    assert_eq!(vm.runs[0].pace, "5:00/km");
    assert!(!vm.runs[0].spike_flag);
    assert_eq!(vm.runs[0].zone, Some(ThreeZone::Z1));
}

#[test]
fn run_zone_strings_match_the_shell_colour_contract() {
    // The view emits the zone as a bare `"Z1"/"Z2"/"Z3"` string (or `"-"`
    // with no HR), and the Android shell's `hrZoneColor` matches on exactly
    // those literals. Renaming the `ThreeZone` variants would compile clean
    // but silently drop every zone colour on-device: this locks the wire
    // strings so such a rename fails here instead.
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

    assert_eq!(app.view(&model).runs[0].pace, "-");
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

    assert_eq!(app.view(&model).runs[0].pace, "4:06/km");
    assert_eq!(fmt_mss(59.6), "1:00");
    assert_eq!(fmt_mss(245.4), "4:05");
    assert_eq!(fmt_mss(f64::NAN), "0:00");
    assert_eq!(fmt_mss(-5.0), "0:00");
    assert_eq!(fmt_clock(3599.6), "1:00:00");
    assert_eq!(fmt_clock(3599.4), "59:59");
    // A tiny critical speed (huge s/km) is not clipped by a 32-bit cast.
    assert_eq!(fmt_mss(1.0e10), "166666666:40");
}

#[test]
fn gps_tracked_run_derives_distance_and_pace_from_fixes() {
    let app = Engine;
    let mut model = Model::default();

    // ~1.11 km along the equator (0 → 0.01° lon) over 400 s.
    let points = vec![
        GpsPoint {
            lat: 0.0,
            lon: 0.0,
            observed_at: 0,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.005,
            observed_at: 200,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.01,
            observed_at: 400,
            accuracy_m: 5.0,
        },
    ];
    // Distinctive (entry_id, observed_at) so the process-global GPX registry
    // this test reads back can't be clobbered by a parallel test's run.
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
    assert_ne!(vm.runs[0].pace, "-");
    assert!(!vm.runs[0].spike_flag);
    // The ~0.5 MB GPX string never rides the list payload. The run
    // advertises an exportable route via `has_gpx`, and the shell
    // fetches the document ON DEMAND via `export_run_gpx` (the `exportGpx`
    // FFI query, populated by the `view()` above). Prove both.
    assert!(
        vm.runs[0].has_gpx,
        "a GPS run advertises an exportable route"
    );
    let gpx = export_run_gpx(9_090_001, 9_090_001);
    assert!(
        gpx.contains("<trkpt "),
        "on-demand GPX carries the fixes: {gpx}"
    );
}

#[test]
fn gps_run_of_only_noise_fixes_offers_no_gpx_export() {
    let app = Engine;
    let mut model = Model::default();

    // Every fix is worse than the accuracy gate, so none survive filtering:
    // fewer than two usable points is not a route, and offering an "Export
    // GPX" affordance (has_gpx / a non-empty export) for a 0 km run would be
    // misleading.
    let points = vec![
        GpsPoint {
            lat: 0.0,
            lon: 0.0,
            observed_at: 0,
            accuracy_m: 80.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.01,
            observed_at: 400,
            accuracy_m: 120.0,
        },
    ];
    // Distinctive (entry_id, observed_at) so the global GPX registry read
    // below is this test's own, not a parallel test's.
    app.update(
        log_run_track(points, 70.0, 12.0, 9_115_001, 9_115_001),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.runs.len(), 1);
    assert_eq!(vm.runs[0].pace, "-");
    // No exportable route for a noise-only track: the flag is false AND the
    // on-demand export is empty.
    assert!(
        !vm.runs[0].has_gpx,
        "has_gpx should be false for a noise-only track"
    );
    let gpx = export_run_gpx(9_115_001, 9_115_001);
    assert!(
        gpx.is_empty(),
        "on-demand GPX should be empty for a noise-only track, got {gpx}"
    );
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
    let points = vec![
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
    ];
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

#[test]
fn run_view_derivation_is_memoized_and_gpx_built_once() {
    // The heavy per-run derivation (≈6 haversine passes + km/mile splits
    // + VI) must run ONCE per distinct run, not on every view(). A second
    // identical derivation serves a cached clone, zero rebuilds. The run
    // view carries no GPX string itself; it advertises an exportable route
    // via `has_gpx` and the document is built ON DEMAND.
    let run = probe_run(918_273);

    let before = RUN_VIEW_BUILDS.with(|c| c.get());
    let first = run_card(&run);
    let after_first = RUN_VIEW_BUILDS.with(|c| c.get());
    let second = run_card(&run);
    let after_second = RUN_VIEW_BUILDS.with(|c| c.get());

    assert_eq!(after_first - before, 1, "the first derivation builds once");
    assert_eq!(
        after_second - after_first,
        0,
        "the second is served from cache"
    );
    assert_eq!(first, second, "the cached view is identical");
    // The run advertises an exportable route (memoization did not drop it),
    // and the on-demand export still builds a valid two-fix-plus GPX.
    assert!(first.has_gpx, "an exportable route is still advertised");
    refresh_gpx_registry(
        std::slice::from_ref(&run),
        &RunFacts::new(std::slice::from_ref(&run)),
    );
    let gpx = export_run_gpx(918_273, 1_000);
    assert!(gpx.contains("<trkseg>"), "on-demand GPX builds: {gpx}");

    // A content change (an amend) must MISS the cache and rebuild.
    let mut amended = run.clone();
    amended.longest_recent_km = 99.0;
    let before_amend = RUN_VIEW_BUILDS.with(|c| c.get());
    let _ = run_card(&amended);
    assert_eq!(
        RUN_VIEW_BUILDS.with(|c| c.get()) - before_amend,
        1,
        "a changed fingerprint recomputes"
    );
}

#[test]
fn prune_run_memos_drops_deleted_run_keeps_live() {
    // A deleted run's per-run memo entries (RUN_VIEW_CACHE + QC_TRACK_CACHE)
    // must be evicted by the per-view() prune while the live run's stay warm.
    let live_run = probe_run(700_001);
    let deleted_run = probe_run(700_002);
    let _ = run_card(&live_run);
    let _ = run_card(&deleted_run);
    let live_key = run_content_key(&live_run);
    let deleted_key = run_content_key(&deleted_run);
    assert!(
        RUN_VIEW_CACHE.with(|c| c.borrow().contains(&deleted_key)),
        "both runs are warm before the prune"
    );

    prune_run_memos(&[live_key]);

    assert!(
        RUN_VIEW_CACHE.with(|c| c.borrow().contains(&live_key)),
        "the live run's view memo survives"
    );
    assert!(
        QC_TRACK_CACHE.with(|c| c.borrow().contains(&live_key)),
        "the live run's QC memo survives"
    );
    assert!(
        !RUN_VIEW_CACHE.with(|c| c.borrow().contains(&deleted_key)),
        "the deleted run's view memo is pruned"
    );
    assert!(
        !QC_TRACK_CACHE.with(|c| c.borrow().contains(&deleted_key)),
        "the deleted run's QC memo is pruned"
    );
}

#[test]
fn refresh_gpx_registry_drops_a_run_no_longer_live() {
    // The registry is pruned against the live history on every refresh, the
    // same path shipping builds run: a deleted run stops being exportable.
    let kept = probe_run(700_020);
    let deleted = probe_run(700_021);
    let both = [kept.clone(), deleted];
    refresh_gpx_registry(&both, &RunFacts::new(&both));
    assert!(export_run_gpx(700_021, 1_000).contains("<trkseg>"));

    let live = [kept];
    refresh_gpx_registry(&live, &RunFacts::new(&live));
    assert!(
        export_run_gpx(700_021, 1_000).is_empty(),
        "the deleted run's route is gone"
    );
    assert!(
        export_run_gpx(700_020, 1_000).contains("<trkseg>"),
        "the live run's route survives"
    );
}

#[test]
fn refresh_gpx_registry_evicts_key_when_run_loses_its_track() {
    // A same-identity run (a GPS→manual amend keeps (entry_id, observed_at))
    // that later carries an empty track must EVICT any stale positive entry,
    // so a subsequent export never serves the pre-amend route.
    let run = probe_run(700_010);
    refresh_gpx_registry(
        std::slice::from_ref(&run),
        &RunFacts::new(std::slice::from_ref(&run)),
    );
    assert!(
        export_run_gpx(700_010, 1_000).contains("<trkseg>"),
        "the >=2-fix run is registered and exportable"
    );

    let mut emptied = run.clone();
    emptied.track = Vec::new();
    let emptied = [emptied];
    refresh_gpx_registry(&emptied, &RunFacts::new(&emptied));
    assert!(
        export_run_gpx(700_010, 1_000).is_empty(),
        "the same-identity empty-track run evicts the stale registry key"
    );
}

#[test]
fn view_list_cache_shrinks_but_still_serves_a_hit() {
    // The bounded VIEW_LIST_CACHE_CAP must not cost correctness: a
    // repeated view() on an unchanged model must
    // return identical lift/run lists (the second serves cache hits).
    let app = Engine;
    let mut model = Model::default();
    app.update(log_set_id("Back Squat", 100.0, 1, 1_000), &mut model)
        .expect_only_render();
    let probe = probe_run(700_003);
    app.update(
        log_run_track(probe.track, 71.0, 0.0, 1_000, 700_003),
        &mut model,
    )
    .expect_only_render();

    let first = app.view(&model);
    let second = app.view(&model);
    assert_eq!(
        first.lifts, second.lifts,
        "lift list identical across views"
    );
    assert_eq!(first.runs, second.runs, "run list identical across views");
}

#[test]
fn run_view_differentiates_interval_from_steady_same_average() {
    // Two GPS runs with identical total distance (~445 m) and duration (120 s)
    // → identical average pace. The interval run mixes hard reps with standing
    // recovery; the steady run holds an even pace. The engine must rate them
    // differently now (RUN-INTERVAL-VI-001), not identically.
    let g = |lon: f64, t: i64| GpsPoint {
        lat: 0.0,
        lon,
        observed_at: t,
        accuracy_m: 5.0,
    };
    let steady = LoggedRun {
        distance_km: 0.0,
        duration_min: 0.0,
        hr_pct_max: 0.0,
        longest_recent_km: 5.0,
        // Four even ~111 m / 30 s segments (~3.71 m/s throughout).
        track: vec![
            g(0.000, 0),
            g(0.001, 30),
            g(0.002, 60),
            g(0.003, 90),
            g(0.004, 120),
        ],
        track_segment_starts: Vec::new(),
        observed_at: 0,
        entry_id: 0,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: None,
    };
    // Interval with JOG recovery (recovery legs stay above the 0.5 m/s
    // auto-pause floor, so moving time, and therefore average moving pace,
    // matches the steady run exactly; only the variability differs). Two
    // ~200 m hard reps (~6.7 m/s) each followed by a ~22 m jog (~0.74 m/s).
    let interval = LoggedRun {
        track: vec![
            g(0.0000, 0),
            g(0.0018, 30),
            g(0.0020, 60),
            g(0.0038, 90),
            g(0.0040, 120),
        ],
        ..steady.clone()
    };

    let sv = run_card(&steady);
    let iv = run_card(&interval);

    // Same average pace by construction.
    assert_eq!(sv.pace, iv.pace, "same-average precondition broke");

    let s = sv.interval.expect("steady interval verdict");
    let i = iv.interval.expect("interval verdict");
    assert_eq!(
        s.kind,
        PaceVariability::Steady,
        "VI {}",
        s.variability_index
    );
    assert_eq!(
        i.kind,
        PaceVariability::Interval,
        "VI {}",
        i.variability_index
    );
    assert!(i.variability_index > s.variability_index);
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
    // `m:ss` string can't be a bar length. `seconds_per_unit` carries it, and
    // MUST be derived from the same value `pace` is formatted from so the two
    // can never disagree (the shell never scrapes the prose).
    let app = Engine;
    let mut model = Model::default();

    // ~2.2 km along the equator (0 → 0.02° lon) over 600 s → ≥2 full-km
    // splits plus a trailing partial, so every branch is exercised.
    let points = vec![
        GpsPoint {
            lat: 0.0,
            lon: 0.000,
            observed_at: 0,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.005,
            observed_at: 150,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.010,
            observed_at: 300,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.015,
            observed_at: 450,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.020,
            observed_at: 600,
            accuracy_m: 5.0,
        },
    ];
    app.update(
        log_run_track(points, 70.0, 12.0, 9_091_001, 9_091_001),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    let splits = &vm.runs[0].splits_km;
    assert!(
        splits.len() >= 2,
        "expected multiple km splits, got {}",
        splits.len()
    );
    for s in splits {
        assert!(s.seconds_per_unit > 0, "split {} pace populated", s.index);
        // The guarantee: `pace` is exactly the format of `seconds_per_unit`.
        assert_eq!(
            s.pace,
            fmt_mss(s.seconds_per_unit as f64),
            "split {} pace string must equal format of seconds_per_unit",
            s.index,
        );
        assert_eq!(
            s.pace,
            format!("{}:{:02}", s.seconds_per_unit / 60, s.seconds_per_unit % 60),
            "split {} m:ss shape",
            s.index,
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
        GpsPoint {
            lat: 0.0,
            lon: 0.000,
            observed_at: 0,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.001,
            observed_at: 10,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.002,
            observed_at: 20,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.003,
            observed_at: 30,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.004,
            observed_at: 40,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 1.000,
            observed_at: 100,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 1.001,
            observed_at: 110,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 1.002,
            observed_at: 120,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 1.003,
            observed_at: 130,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 1.004,
            observed_at: 140,
            accuracy_m: 5.0,
        },
    ];

    // With the boundary: distance is the two ~445 m segments (~0.89 km), NOT
    // the ~111 km bridge, and the on-demand GPX opens two <trkseg>s at the
    // TRUE coords. Distinctive (entry_id, observed_at) so the process-global
    // GPX registry read back is this test's own, not a parallel test's.
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
    // The two-segment GPX is pinned on the on-demand export (populated by
    // the `view()` above).
    let gpx = export_run_gpx(11_203_001, 11_203_001);
    assert_eq!(gpx.matches("<trkseg>").count(), 2, "two segments in GPX");
    assert!(gpx.contains("lon=\"1.0000000\""), "true seg-2 coord kept");

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
    assert!(naive_run.qc_dropped >= 5, "the 5 seg-2 fixes were dropped");
}
