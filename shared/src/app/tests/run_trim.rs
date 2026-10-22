//! Trimming a GPS run's head and tail: figures re-derive, segment starts remap,
//! and repeated trims compose.

use super::*;

/// A five-fix track; helper so each trim test states only its bounds.
fn trim_probe_points() -> Vec<GpsPoint> {
    vec![
        GpsPoint {
            lat: 0.0,
            lon: 0.000,
            observed_at: 0,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.005,
            observed_at: 100,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.010,
            observed_at: 200,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.015,
            observed_at: 300,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.020,
            observed_at: 400,
            accuracy_m: 5.0,
        },
    ]
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
    assert_eq!(model.runs[0].track.len(), 5);

    // Keep the middle three fixes (t=100,200,300); cut the first and last.
    app.update(trim(6_100_001, 6_100_001, 100, 300), &mut model)
        .expect_only_render();

    assert_eq!(
        model.runs.len(),
        1,
        "trim edits in place, never adds/removes a run"
    );
    assert_eq!(
        model.runs[0].track.len(),
        3,
        "head+tail fixes cut from the stored track"
    );
    let after = app.view(&model).runs[0].clone();
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
    app.update(
        log_run_track_seg(pts, 0.0, 0.0, 6_100_002, 6_100_002, vec![5]),
        &mut model,
    )
    .expect_only_render();
    assert_eq!(model.runs[0].track_segment_starts, vec![5]);

    // Keep t in [20,120]: drops raw idx 0,1 (head) and 8,9 (tail); the six
    // survivors are old idx 2,3,4,5,6,7, so the boundary at old idx 5 lands at
    // new idx 3.
    app.update(trim(6_100_002, 6_100_002, 20, 120), &mut model)
        .expect_only_render();
    assert_eq!(model.runs[0].track.len(), 6, "four fixes cut");
    assert_eq!(
        model.runs[0].track_segment_starts,
        vec![3],
        "boundary remapped to the slice"
    );
    // The two-segment route still exports as two <trkseg>s.
    let _ = app.view(&model);
    let gpx = export_run_gpx(6_100_002, 6_100_002);
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
    let full = model.runs[0].track.clone();

    // No matching run: untouched.
    app.update(trim(999, 0, 100, 300), &mut model)
        .expect_only_render();
    assert_eq!(model.runs[0].track, full, "no-match trim changes nothing");
    // Inverted range: untouched.
    app.update(trim(6_100_003, 6_100_003, 300, 100), &mut model)
        .expect_only_render();
    assert_eq!(model.runs[0].track, full, "inverted range changes nothing");
    // Fewer than two fixes retained: untouched.
    app.update(trim(6_100_003, 6_100_003, 350, 500), &mut model)
        .expect_only_render();
    assert_eq!(model.runs[0].track, full, "a <2-fix slice changes nothing");
    // No-op keeps-everything (not a strict subset): untouched.
    app.update(trim(6_100_003, 6_100_003, -100, 1000), &mut model)
        .expect_only_render();
    assert_eq!(
        model.runs[0].track, full,
        "keeping every fix changes nothing"
    );
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
    assert_eq!(model.runs[0].track.len(), 3);
    app.update(trim(6_100_004, 6_100_004, 200, 300), &mut model)
        .expect_only_render();
    assert_eq!(
        model.runs[0].track.len(),
        2,
        "second trim narrows the surviving slice"
    );
    assert_eq!(model.runs[0].track[0].observed_at, 200);
    assert_eq!(model.runs[0].track[1].observed_at, 300);
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
    assert_eq!(model.runs.len(), 1);
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
        model.runs.is_empty(),
        "deleting a trimmed run removes it entirely"
    );
}

#[test]
fn trim_dropping_a_whole_segment_drops_its_boundary() {
    // Two segments (boundary at raw index 5); keep only the FIRST segment's
    // fixes → the second segment is entirely cut, so its boundary vanishes
    // (no dangling start pointing past the sliced track).
    let app = Engine;
    let mut model = Model::default();
    let pts = vec![
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
        model.runs[0].track.len(),
        5,
        "only the first segment survives"
    );
    assert!(
        model.runs[0].track_segment_starts.is_empty(),
        "the cut segment's boundary is dropped, not left dangling"
    );
}
