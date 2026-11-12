//! Trimming a GPS run's head and tail: figures re-derive, segment starts remap,
//! and repeated trims compose.

use super::*;

/// A five-fix track; helper so each trim test states only its bounds.
fn trim_probe_points() -> Vec<GpsPoint> {
    vec![
        gps(0.000, 0),
        gps(0.005, 100),
        gps(0.010, 200),
        gps(0.015, 300),
        gps(0.020, 400),
    ]
}

/// The fix stamps of run `id` (logged at `id`) per recording segment, as the
/// bridge's `RunTrack` op hands them to the shell.
fn segment_stamps(model: &Model, id: u64) -> Vec<Vec<i64>> {
    run_track_segments(model, id, id as i64)
        .iter()
        .map(|seg| seg.iter().map(|p| p.observed_at).collect())
        .collect()
}

#[test]
fn trim_run_track_cuts_head_and_tail_and_reshrinks_figures() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        log_run_track(trim_probe_points(), 0.0, 0.0, 6_100_001, 6_100_001),
        &mut model,
    )
    .expect_only_render();
    let before = app.view(&model).runs[0].clone();
    assert_eq!(segment_stamps(&model, 6_100_001), [[0, 100, 200, 300, 400]]);

    // Keep the middle three fixes (t=100,200,300); cut the first and last.
    app.update(trim(6_100_001, 6_100_001, 100, 300), &mut model)
        .expect_only_render();

    assert_eq!(
        segment_stamps(&model, 6_100_001),
        [[100, 200, 300]],
        "head+tail fixes cut from the route"
    );
    let runs = app.view(&model).runs;
    assert_eq!(
        runs.len(),
        1,
        "trim edits in place, never adds/removes a run"
    );
    let after = &runs[0];
    // Every derived figure re-derived from the sliced track (content-key memo
    // self-invalidated): distance and splits shrank, HR/spike untouched.
    assert!(
        after.distance_km < before.distance_km,
        "distance shrank: {} < {}",
        after.distance_km,
        before.distance_km
    );
    assert!(after.distance_km > 0.0);
    // Identity is UNCHANGED so history/GPX/day-bucket keys stay stable.
    assert_eq!(after.observed_at, before.observed_at, "observed_at stable");
    assert_eq!(after.entry_id, before.entry_id, "entry_id stable");
    assert!(after.has_gpx, "a trimmed run still has an exportable route");
}

#[test]
fn trim_run_track_remaps_segment_starts_across_a_cut() {
    // Two segments (boundary at raw index 5); trim cuts two fixes off the head
    // and two off the tail. The surviving segment boundary must be REMAPPED to
    // its new position in the sliced track (qc_track's pending-boundary rule).
    let app = Engine;
    let mut model = Model::default();
    let pts = vec![
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
    app.update(
        log_run_track_seg(pts, 0.0, 0.0, 6_100_002, 6_100_002, vec![5]),
        &mut model,
    )
    .expect_only_render();
    assert_eq!(
        segment_stamps(&model, 6_100_002),
        [vec![0, 10, 20, 30, 40], vec![100, 110, 120, 130, 140]]
    );

    // Keep t in [20,120]: drops raw idx 0,1 (head) and 8,9 (tail); the six
    // survivors are old idx 2,3,4,5,6,7, so the boundary at old idx 5 lands at
    // new idx 3.
    app.update(trim(6_100_002, 6_100_002, 20, 120), &mut model)
        .expect_only_render();
    assert_eq!(
        segment_stamps(&model, 6_100_002),
        [[20, 30, 40], [100, 110, 120]],
        "four fixes cut, boundary remapped to the slice"
    );
    // The two-segment route still exports as two <trkseg>s.
    let gpx = export_run_gpx(&model, 6_100_002, 6_100_002);
    assert_eq!(
        gpx.matches("<trkseg>").count(),
        2,
        "boundary survives as two segments"
    );
}

#[test]
fn trim_run_track_rejects_invalid_and_no_op_bounds() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        log_run_track(trim_probe_points(), 0.0, 0.0, 6_100_003, 6_100_003),
        &mut model,
    )
    .expect_only_render();
    let full = export_run_gpx(&model, 6_100_003, 6_100_003);
    let unchanged = |model: &Model, why: &str| {
        assert_eq!(export_run_gpx(model, 6_100_003, 6_100_003), full, "{why}");
    };

    // No matching run: untouched.
    app.update(trim(999, 0, 100, 300), &mut model)
        .expect_only_render();
    unchanged(&model, "no-match trim changes nothing");
    // Inverted range: untouched.
    app.update(trim(6_100_003, 6_100_003, 300, 100), &mut model)
        .expect_only_render();
    unchanged(&model, "inverted range changes nothing");
    // Fewer than two fixes retained: untouched.
    app.update(trim(6_100_003, 6_100_003, 350, 500), &mut model)
        .expect_only_render();
    unchanged(&model, "a <2-fix slice changes nothing");
    // No-op keeps-everything (not a strict subset): untouched.
    app.update(trim(6_100_003, 6_100_003, -100, 1000), &mut model)
        .expect_only_render();
    unchanged(&model, "keeping every fix changes nothing");
}

#[test]
fn two_sequential_trims_compose_by_intersection() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        log_run_track(trim_probe_points(), 0.0, 0.0, 6_100_004, 6_100_004),
        &mut model,
    )
    .expect_only_render();
    // Keep [100,300] → 3 fixes, then [200,300] over the result → 2 fixes.
    app.update(trim(6_100_004, 6_100_004, 100, 300), &mut model)
        .expect_only_render();
    assert_eq!(segment_stamps(&model, 6_100_004), [[100, 200, 300]]);
    app.update(trim(6_100_004, 6_100_004, 200, 300), &mut model)
        .expect_only_render();
    assert_eq!(
        segment_stamps(&model, 6_100_004),
        [[200, 300]],
        "second trim narrows the surviving slice"
    );
}

#[test]
fn trim_then_delete_removes_the_run() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        log_run_track(trim_probe_points(), 0.0, 0.0, 6_100_005, 6_100_005),
        &mut model,
    )
    .expect_only_render();
    app.update(trim(6_100_005, 6_100_005, 100, 300), &mut model)
        .expect_only_render();
    assert_eq!(app.view(&model).runs.len(), 1);
    app.update(
        Event::DeleteEntry {
            kind: EntryKind::Run,
            entry_id: 6_100_005,
            observed_at_fallback: 6_100_005,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(
        app.view(&model).runs.is_empty(),
        "deleting a trimmed run removes it entirely"
    );
    assert!(export_run_gpx(&model, 6_100_005, 6_100_005).is_empty());
}

#[test]
fn trim_dropping_a_whole_segment_drops_its_boundary() {
    // Two segments (boundary at raw index 5); keep only the FIRST segment's
    // fixes → the second segment is entirely cut, so its boundary vanishes
    // (no dangling start pointing past the sliced track).
    let app = Engine;
    let mut model = Model::default();
    let pts = vec![
        gps(0.000, 0),
        gps(0.001, 10),
        gps(0.002, 20),
        gps(0.003, 30),
        gps(0.004, 40),
        gps(1.000, 100),
        gps(1.001, 110),
        gps(1.002, 120),
    ];
    app.update(
        log_run_track_seg(pts, 0.0, 0.0, 6_100_006, 6_100_006, vec![5]),
        &mut model,
    )
    .expect_only_render();
    // Keep t in [0,40]: the entire second segment (idx 5-7) is cut.
    app.update(trim(6_100_006, 6_100_006, 0, 40), &mut model)
        .expect_only_render();
    assert_eq!(
        segment_stamps(&model, 6_100_006),
        [[0, 10, 20, 30, 40]],
        "only the first segment survives, its boundary dropped, not left dangling"
    );
    let gpx = export_run_gpx(&model, 6_100_006, 6_100_006);
    assert_eq!(gpx.matches("<trkseg>").count(), 1);
}
