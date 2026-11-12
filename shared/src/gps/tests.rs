use super::*;
use crate::running::{INTERVAL_VI_THRESHOLD, KM_M, MILE_M};
use crate::test_support::M_PER_DEG;

fn pt(lat: f64, lon: f64, t: i64) -> GpsPoint {
    GpsPoint {
        lat,
        lon,
        observed_at: t,
        accuracy_m: 5.0,
    }
}

#[test]
fn variability_index_is_steady_normally_and_finite_when_dt_is_corrupt() {
    // Normal: a 90 s leg at 3 m/s smooths into three equal 30 s windows, a
    // steady VI.
    let steady = [pt(0.0, 0.0, 0), pt(0.0, 270.0 / M_PER_DEG, 90)];
    assert_eq!(track_variability_index(&steady, &[]), Some(1.0));
    // Corrupt: one ~20 000 km leg over ~2e7 s (a millisecond timestamp read as
    // seconds) still moves at ~1 m/s, so its ~700 000 windows hit the
    // smoothing cap; the index stays a finite steady figure.
    let corrupt = [pt(0.0, 0.0, 0), pt(0.0, 179.9, 20_000_000)];
    assert_eq!(track_variability_index(&corrupt, &[]), Some(1.0));
}

#[test]
fn track_splits_reject_sub_metre_unit_and_bound_a_corrupt_track() {
    let track = vec![pt(0.0, 0.0, 0), pt(0.0, 0.02, 300)]; // ~2.2 km, 5 min
    assert!(
        !track_splits(&track, KM_M, &[]).is_empty(),
        "km splits work"
    );
    // A sub-metre unit is rejected up front: the boundary loop below emits
    // one split per unit, so a tiny unit would iterate millions of times.
    assert!(
        track_splits(&track, 0.5, &[]).is_empty(),
        "sub-metre unit rejected"
    );
    assert!(
        track_splits(&track, 0.0, &[]).is_empty(),
        "zero unit rejected"
    );
    // Even at a legal unit, a long track cannot emit unbounded splits: twice
    // the cap in 1 m units, at half the plausible-runner ceiling so no leg
    // is capped.
    let metres = 2.0 * MAX_SPLITS as f64;
    let secs = (metres / (MAX_PLAUSIBLE_SPEED_MPS / 2.0)) as i64;
    let long = vec![pt(0.0, 0.0, 0), pt(0.0, metres / M_PER_DEG, secs)];
    assert_eq!(
        track_splits(&long, 1.0, &[]).len(),
        MAX_SPLITS,
        "splits capped"
    );
}

#[test]
fn haversine_one_degree_latitude_is_about_111km() {
    // One degree of latitude ≈ R * (π/180) ≈ 111.19 km on a sphere.
    let d = haversine_m(pt(0.0, 0.0, 0), pt(1.0, 0.0, 0));
    assert!((d - 111_194.0).abs() < 50.0, "got {d}");
}

#[test]
fn haversine_stays_finite_for_near_antipodal_coords() {
    // A demonstrated valid coordinate pair whose rounded haversine `h`
    // exceeds 1.0, so an unclamped `sqrt(h).asin()` returns NaN. The
    // `h.min(1.0)` clamp must keep the distance finite (and ≈ half the
    // Earth's circumference for an antipodal pair).
    let d = haversine_m(pt(-59.13, 0.0, 0), pt(59.1300000000043, 180.0, 0));
    assert!(
        d.is_finite(),
        "near-antipodal haversine must be finite, got {d}"
    );
    assert!(d > 0.0, "distance should be positive, got {d}");
}

#[test]
fn positive_split_detects_back_half_slowdown() {
    // Five equatorial fixes 0.001° apart → four equal ~111.3 m segments.
    // First half covered in 20 s, second half in 40 s → back half is exactly
    // twice as slow, i.e. a +100 % positive split.
    let track = vec![
        pt(0.0, 0.000, 0),
        pt(0.0, 0.001, 10),
        pt(0.0, 0.002, 20),
        pt(0.0, 0.003, 40),
        pt(0.0, 0.004, 60),
    ];
    let split = track_positive_split_pct(&track, &[]).expect("split");
    assert!((split - 100.0).abs() < 1.0, "got {split}");
}

#[test]
fn even_pace_run_reports_no_meaningful_split() {
    let track = vec![
        pt(0.0, 0.000, 0),
        pt(0.0, 0.001, 10),
        pt(0.0, 0.002, 20),
        pt(0.0, 0.003, 30),
        pt(0.0, 0.004, 40),
    ];
    let split = track_positive_split_pct(&track, &[]).expect("split");
    assert!(split.abs() < 1.0, "got {split}");
}

#[test]
fn mid_run_pause_does_not_produce_a_false_positive_split() {
    // Evenly paced ~3.7 m/s throughout, but with a 10-min standstill in the
    // SECOND half (the app's Pause button leaves a ~0-speed bridge leg).
    // Moving-time halves see it as the even run it is.
    let track = vec![
        pt(0.0, 0.000, 0),
        pt(0.0, 0.001, 30),
        pt(0.0, 0.002, 60),  // ~halfway
        pt(0.0, 0.002, 660), // 10-min stop, 0 m → excluded
        pt(0.0, 0.003, 690),
        pt(0.0, 0.004, 720),
    ];
    let split = track_positive_split_pct(&track, &[]).expect("split");
    assert!(
        split.abs() < 1.0,
        "paused steady run falsely split: {split}"
    );
}

#[test]
fn positive_split_needs_three_usable_fixes() {
    let track = vec![pt(0.0, 0.0, 0), pt(0.0, 0.001, 10)];
    assert!(track_positive_split_pct(&track, &[]).is_none());
}

/// Even-pace straight track along the equator: constant longitude step +
/// constant time step ⇒ constant ground speed, so every FULL split must
/// carry the same per-unit pace regardless of where fixes land relative to
/// the km/mile boundaries. `legs` × ~111 m each.
fn even_equator_track(legs: usize, dlon_deg: f64, dt_s: i64) -> Vec<GpsPoint> {
    (0..=legs)
        .map(|i| pt(0.0, i as f64 * dlon_deg, i as i64 * dt_s))
        .collect()
}

#[test]
fn splits_even_pace_all_full_splits_equal_pace() {
    // 50 legs × ~111.19 m ≈ 5.56 km at a constant ~3.71 m/s.
    let track = even_equator_track(50, 0.001, 30);
    let km = track_splits(&track, KM_M, &[]);
    let full: Vec<_> = km.iter().filter(|s| !s.partial).collect();
    assert_eq!(full.len(), 5, "≈5.56 km → 5 full km splits");
    let p0 = full[0].pace_sec_per_unit;
    for s in &full {
        assert!(
            (s.pace_sec_per_unit - p0).abs() < 0.5,
            "full split {} pace {} vs {p0}",
            s.index,
            s.pace_sec_per_unit
        );
        assert!((s.distance_m - KM_M).abs() < 1e-6);
    }
    assert_eq!(
        km.iter().map(|s| s.index).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6]
    );
}

#[test]
fn splits_mid_run_stop_does_not_inflate_split_pace() {
    // Even-pace equator track (~111.19 m / 30 s legs, ~3.71 m/s) with a
    // 300 s standstill spliced in mid-first-km (after ~556 m): the stopped
    // leg (0 m over 300 s, below the 0.5 m/s auto-pause floor) must be
    // excluded from split 1's TIME, so its pace matches the other full
    // splits (the run's moving pace) instead of reading ~5 min slower -
    // consistent with `moving_duration_min` on the same detail sheet.
    let mut track: Vec<GpsPoint> = (0..=5).map(|i| pt(0.0, i as f64 * 0.001, i * 30)).collect();
    // Standstill: same position as fix 5, 300 s later.
    track.push(pt(0.0, 0.005, 5 * 30 + 300));
    track.extend((6..=50).map(|i| pt(0.0, i as f64 * 0.001, i * 30 + 300)));

    let km = track_splits(&track, KM_M, &[]);
    let full: Vec<_> = km.iter().filter(|s| !s.partial).collect();
    assert_eq!(full.len(), 5, "≈5.56 km → 5 full km splits");
    // Moving average: 1000 m at ~3.7065 m/s ≈ 269.8 s/km.
    let moving_avg = KM_M / (0.001f64.to_radians() * super::EARTH_RADIUS_M / 30.0);
    for s in &full {
        assert!(
            (s.pace_sec_per_unit - moving_avg).abs() < 2.0,
            "split {} pace {} should sit at the moving average {moving_avg}, \
             not be inflated by the stop",
            s.index,
            s.pace_sec_per_unit
        );
    }
    assert!(
        (full[0].pace_sec_per_unit - full[1].pace_sec_per_unit).abs() < 0.5,
        "stop split {} vs clean split {}",
        full[0].pace_sec_per_unit,
        full[1].pace_sec_per_unit
    );
}

#[test]
fn splits_all_stopped_track_match_zero_moving_headline() {
    // A track covered entirely below the 0.5 m/s stop floor (~111.19 m legs
    // over 300 s each, ~0.37 m/s) has zero MOVING distance, so its headline
    // `track_distance_km` is 0. The split walk counts only moving legs, so
    // it agrees: no phantom split for distance the headline excludes.
    let track: Vec<GpsPoint> = (0..=4)
        .map(|i| pt(0.0, i as f64 * 0.001, i * 300))
        .collect();
    assert_eq!(
        track_distance_km(&track, &[]),
        0.0,
        "headline is zero moving km"
    );
    assert!(
        track_splits(&track, KM_M, &[]).is_empty(),
        "an all-stopped track yields no splits, matching its zero headline"
    );
}

#[test]
fn splits_final_partial_is_flagged() {
    // ≈5.56 km → the 6th km split is partial (~0.56 km).
    let track = even_equator_track(50, 0.001, 30);
    let km = track_splits(&track, KM_M, &[]);
    let last = km.last().expect("a split");
    assert!(last.partial, "final split flagged partial");
    assert!(last.distance_m < KM_M, "partial covers < 1 km");
    assert!(
        km[..km.len() - 1].iter().all(|s| !s.partial),
        "only the last split is partial"
    );
}

#[test]
fn splits_km_and_mi_counts_differ_for_same_track() {
    // Same ≈5.56 km track: 5 full + 1 partial km (6) vs 3 full + 1 partial mi
    // (4), since 5.56 km ≈ 3.45 mi.
    let track = even_equator_track(50, 0.001, 30);
    let km = track_splits(&track, KM_M, &[]);
    let mi = track_splits(&track, MILE_M, &[]);
    assert_eq!(km.len(), 6, "km splits");
    assert_eq!(mi.len(), 4, "mile splits");
    assert_eq!(mi.iter().filter(|s| !s.partial).count(), 3, "3 full miles");
    assert!(mi.last().unwrap().partial);
}

#[test]
fn splits_under_one_unit_is_single_partial() {
    // ~445 m: under a km → one partial split, index 1.
    let track = even_equator_track(4, 0.001, 30);
    let km = track_splits(&track, KM_M, &[]);
    assert_eq!(km.len(), 1);
    assert_eq!(km[0].index, 1);
    assert!(km[0].partial);
    assert!(km[0].distance_m < KM_M);
}

#[test]
fn splits_degenerate_tracks_are_empty() {
    assert!(track_splits(&[], KM_M, &[]).is_empty(), "empty track");
    assert!(
        track_splits(&[pt(0.0, 0.0, 0)], KM_M, &[]).is_empty(),
        "single fix"
    );
    // Zero-distance track (all fixes stacked) → nothing to split.
    let stacked = vec![pt(0.0, 0.0, 0), pt(0.0, 0.0, 10), pt(0.0, 0.0, 20)];
    assert!(
        track_splits(&stacked, KM_M, &[]).is_empty(),
        "zero distance"
    );
    assert!(track_splits(&even_equator_track(50, 0.001, 30), 0.0, &[]).is_empty());
}

#[test]
fn variability_index_separates_interval_from_steady_same_average() {
    // Two runs with the SAME total distance (0.004° lon ≈ 445 m) and the SAME
    // duration (120 s) → identical average pace. The current engine rates them
    // the same; the variability index must not.
    // Steady: four equal ~111 m / 30 s segments (~3.71 m/s throughout).
    let steady = vec![
        pt(0.0, 0.000, 0),
        pt(0.0, 0.001, 30),
        pt(0.0, 0.002, 60),
        pt(0.0, 0.003, 90),
        pt(0.0, 0.004, 120),
    ];
    // Interval with JOG recovery (recovery legs stay ABOVE the 0.5 m/s
    // auto-pause floor, so they count as moving time, matching the app's
    // moving-pace base; and the same 445 m / 120 s average holds): two
    // ~200 m / 30 s hard reps (~6.7 m/s) each followed by a ~22 m / 30 s jog
    // (~0.74 m/s). Standing (0 m/s) recovery would be excluded as a pause.
    let interval = vec![
        pt(0.0, 0.0000, 0),
        pt(0.0, 0.0018, 30),
        pt(0.0, 0.0020, 60),
        pt(0.0, 0.0038, 90),
        pt(0.0, 0.0040, 120),
    ];

    // Same average by construction (both span 120 s): sanity-check that first.
    let ds = track_distance_km(&steady, &[]);
    let di = track_distance_km(&interval, &[]);
    assert!((ds - di).abs() < 0.001, "distances differ: {ds} vs {di}");

    let vi_steady = track_variability_index(&steady, &[]).expect("steady VI");
    let vi_interval = track_variability_index(&interval, &[]).expect("interval VI");

    // Steady ≈ 1.0; interval well above it (jog-recovery 4th-power mean ~1.5).
    assert!((vi_steady - 1.0).abs() < 0.03, "steady VI {vi_steady}");
    assert!(vi_interval > 1.4, "interval VI {vi_interval}");
    assert!(
        vi_interval > vi_steady + 0.3,
        "interval {vi_interval} not clearly above steady {vi_steady}",
    );
}

#[test]
fn variability_index_needs_two_usable_fixes() {
    assert!(track_variability_index(&[pt(0.0, 0.0, 0)], &[]).is_none());
    assert!(track_variability_index(&[], &[]).is_none());
}

/// Build a DENSE (1 Hz) GPS track from a speed profile `(speed_mps,
/// duration_s)`, with deterministic lateral position jitter to mimic a real
/// consumer GPS at ~a few metres accuracy. Forward motion runs along lon at
/// the equator; the lateral wobble is `jitter_deg·sin(i·1.3)`
/// on lat, which makes consecutive per-second leg lengths swing (exactly the
/// noise that, un-smoothed, blows up the 4th-power mean).
fn noisy_track(profile: &[(f64, i64)], jitter_deg: f64) -> Vec<GpsPoint> {
    let mut pts = Vec::new();
    let mut lon = 0.0f64;
    let mut t = 0i64;
    let deg_per_m = 1.0 / M_PER_DEG;
    pts.push(GpsPoint {
        lat: 0.0,
        lon: 0.0,
        observed_at: 0,
        accuracy_m: 5.0,
    });
    for &(speed, dur) in profile {
        for _ in 0..dur {
            lon += speed * deg_per_m;
            t += 1;
            let lat = jitter_deg * (t as f64 * 1.3).sin();
            pts.push(GpsPoint {
                lat,
                lon,
                observed_at: t,
                accuracy_m: 5.0,
            });
        }
    }
    pts
}

/// Save-time GPS decimation (shell `RunSession.decimatedTrackForCore`): keep
/// every `stride`-th fix plus the endpoints. Stride 1 is the full 1 Hz track;
/// at strides 2 and 3 the shell hands the core fewer points, and the
/// of-record verdicts must not move.
fn decimate(points: &[GpsPoint], stride: usize) -> Vec<GpsPoint> {
    assert!(stride >= 1);
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut out: Vec<GpsPoint> = points.iter().step_by(stride).copied().collect();
    let last = *points.last().unwrap();
    if out.last() != Some(&last) {
        out.push(last);
    }
    out
}

#[test]
fn decimation_preserves_the_steady_vi_verdict() {
    // A steady 5-min run at ~3 m/s with ~3 m lateral GPS jitter. Per-second
    // leg speeds swing wildly; the 30 s rolling window averages the jitter
    // out, so VI reads steady at 1 Hz and after ×2 and ×3 thinning.
    let full = noisy_track(&[(3.0, 300)], 0.000_03);
    for stride in [1usize, 2, 3] {
        let thinned = decimate(&full, stride);
        let vi = track_variability_index(&thinned, &[]).expect("vi");
        assert!(
            vi < INTERVAL_VI_THRESHOLD,
            "steady run flipped to interval after ×{stride} decimation: VI {vi}"
        );
    }
}

#[test]
fn decimation_preserves_the_interval_vi_verdict() {
    // A 1 Hz interval session at realistic (~1 m) GPS jitter: 4× (90 s hard
    // @ ~5 m/s, then 90 s jog @ ~2 m/s). The reps span many 30 s windows, so
    // the verdict survives smoothing at 1 Hz and ×2/×3 thinning (~10–15
    // samples per window at a 2–3 s cadence). Heavy high-frequency noise can
    // still pull VI under the cutoff: the metric is Weak-graded.
    let mut profile = Vec::new();
    for _ in 0..4 {
        profile.push((5.0, 90));
        profile.push((2.0, 90));
    }
    let full = noisy_track(&profile, 0.000_01);
    for stride in [1usize, 2, 3] {
        let thinned = decimate(&full, stride);
        let vi = track_variability_index(&thinned, &[]).expect("vi");
        assert!(
            vi >= INTERVAL_VI_THRESHOLD,
            "interval run lost its verdict after ×{stride} decimation: VI {vi}"
        );
    }
}

#[test]
fn decimation_preserves_distance_within_one_percent() {
    // Total distance is the storage-heaviest figure users see; thinning must
    // not move it more than ±1 % at strides 2 and 3. Uses realistic sub-metre
    // GPS jitter (0.000_004 ≈ 0.44 m), a real good-fix running track barely
    // wanders between 1 s samples, so thinning to a 2–3 s cadence keeps the
    // path length. (The heavier 0.000_03/0.000_01 wobble the VI fixtures use
    // is a deliberate per-second speed-swing stress case, whose fixed-freq
    // sine ALIASES under decimation, an artifact of the generator, not a
    // real track shape, so it is not the right fixture for distance parity.)
    let mut profile = Vec::new();
    for _ in 0..4 {
        profile.push((5.0, 90));
        profile.push((2.0, 90));
    }
    for track in [
        noisy_track(&[(3.0, 600)], 0.000_004),
        noisy_track(&profile, 0.000_004),
    ] {
        let full_km = track_distance_km(&track, &[]);
        for stride in [2usize, 3] {
            let thinned_km = track_distance_km(&decimate(&track, stride), &[]);
            let err = (thinned_km - full_km).abs() / full_km;
            assert!(
                err <= 0.01,
                "×{stride} decimation moved distance {err:.4} (>1%): {full_km} → {thinned_km}"
            );
        }
    }
}

#[test]
fn decimation_preserves_the_positive_split_sign() {
    // A run that clearly slows in the back half is a POSITIVE split; one that
    // speeds up is NEGATIVE. Decimation must not flip that sign: the split
    // verdict a runner reads stays the same.
    let positive = noisy_track(&[(4.0, 150), (2.5, 150)], 0.000_01);
    let negative = noisy_track(&[(2.5, 150), (4.0, 150)], 0.000_01);
    for stride in [2usize, 3] {
        let ps_pos = track_positive_split_pct(&decimate(&positive, stride), &[]).expect("pos");
        let ps_neg = track_positive_split_pct(&decimate(&negative, stride), &[]).expect("neg");
        assert!(
            ps_pos > 0.0,
            "positive-split run lost its sign after ×{stride}: {ps_pos}"
        );
        assert!(
            ps_neg < 0.0,
            "negative-split run lost its sign after ×{stride}: {ps_neg}"
        );
    }
}

#[test]
fn steady_run_with_a_mid_run_pause_stays_steady() {
    // A steady ~3.7 m/s run with one long standing pause in the middle: the
    // paused leg (a ~0 m/s bridge spanning the stop, exactly what the app's
    // Pause button leaves) must be EXCLUDED, so the run reads steady, not a
    // false "interval" from the depressed elapsed-time average.
    let steady_paused = vec![
        pt(0.0, 0.000, 0),
        pt(0.0, 0.001, 30),  // ~3.7 m/s
        pt(0.0, 0.002, 60),  // ~3.7 m/s
        pt(0.0, 0.002, 660), // 10-min stop, ~0 m/s → excluded
        pt(0.0, 0.003, 690), // ~3.7 m/s
        pt(0.0, 0.004, 720), // ~3.7 m/s
    ];
    let vi = track_variability_index(&steady_paused, &[]).expect("vi");
    assert!(
        vi < INTERVAL_VI_THRESHOLD,
        "paused steady wrongly flagged: VI {vi}"
    );
    assert!((vi - 1.0).abs() < 0.05, "VI {vi} should be ~1.0");
}

#[test]
fn a_gps_jitter_spike_is_clamped_in_distance_and_variability() {
    // A single teleport fix (huge distance in 1 s) is capped at the
    // plausible-runner ceiling, not allowed to blow up the distance or the
    // 4th-power mean. Two clean 30 s segments plus one spike.
    let spiky = vec![
        pt(0.0, 0.000, 0),
        pt(0.0, 0.001, 30),
        pt(5.0, 5.000, 31), // ~780 km in 1 s
        pt(5.0, 5.001, 61),
    ];
    let clean_m = haversine_m(spiky[0], spiky[1]) + haversine_m(spiky[2], spiky[3]);
    let km = track_distance_km(&spiky, &[]);
    assert!(
        km <= (clean_m + MAX_PLAUSIBLE_SPEED_MPS) / 1000.0 + 1e-9,
        "{km} km"
    );
    // Uncapped, the spike's window would read ~26 km/s and the VI thousands.
    let vi = track_variability_index(&spiky, &[]).expect("vi");
    assert!(vi < 1.2, "VI {vi} not clamped");
}

#[test]
fn track_distance_sums_segments_and_drops_noisy_fixes() {
    let clean = vec![pt(0.0, 0.0, 0), pt(0.0, 0.001, 10), pt(0.0, 0.002, 20)];
    let d = track_distance_km(&clean, &[]);
    // Two equal ~111.3 m equatorial-lon segments ≈ 0.2226 km.
    assert!((d - 0.2226).abs() < 0.001, "got {d}");

    // A noisy middle fix (accuracy 50 m) is dropped, leaving one long segment.
    let noisy = vec![
        pt(0.0, 0.0, 0),
        GpsPoint {
            lat: 5.0,
            lon: 5.0,
            observed_at: 10,
            accuracy_m: 50.0,
        },
        pt(0.0, 0.002, 20),
    ];
    let d2 = track_distance_km(&noisy, &[]);
    assert!((d2 - 0.2226).abs() < 0.001, "got {d2}");
}

#[test]
fn export_gpx_wraps_fixes_and_escapes_name() {
    let track = vec![
        pt(52.52, 13.405, 1_700_000_000),
        pt(52.521, 13.406, 1_700_000_030),
    ];
    let gpx = export_gpx(&track, "Tom & \"Jerry\" <run>'s", &[], None);
    assert!(gpx.starts_with("<?xml version=\"1.0\""));
    assert!(gpx.contains("<gpx version=\"1.1\""));
    // All five XML predefined entities are escaped so the name cannot break
    // the document (& " < > ').
    assert!(gpx.contains("<name>Tom &amp; &quot;Jerry&quot; &lt;run&gt;&apos;s</name>"));
    assert_eq!(gpx.matches("<trkpt ").count(), 2);
    assert!(gpx.contains("lat=\"52.52\" lon=\"13.405\""));
    assert!(gpx.contains("<time>2023-11-14T22:13:20Z</time>"));
    assert!(gpx.trim_end().ends_with("</gpx>"));
}

#[test]
fn export_gpx_writes_accuracy_as_hdop_the_importer_reads_back() {
    let mut a = pt(52.52, 13.405, 1_700_000_000);
    a.accuracy_m = 5.0;
    let mut b = pt(52.521, 13.406, 1_700_000_030);
    b.accuracy_m = 12.5;
    let mut unknown = pt(52.522, 13.407, 1_700_000_060);
    unknown.accuracy_m = 0.0;
    let gpx = export_gpx(&[a, b, unknown], "run", &[], None);
    assert!(gpx.contains("</time><hdop>1</hdop></trkpt>"), "{gpx}");
    assert!(gpx.contains("<hdop>2.5</hdop>"), "{gpx}");
    assert_eq!(
        gpx.matches("<hdop>").count(),
        2,
        "no hdop for unknown accuracy"
    );
    let hdop: f64 = gpx
        .split("<hdop>")
        .nth(2)
        .unwrap()
        .split('<')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!((hdop * GPX_HDOP_UERE_M) as f32, 12.5);
}

/// A pause + relocation: segment 1 is four ~111 m legs at the equator, then
/// the runner relocates ~111 km east and runs segment 2 (four more ~111 m
/// legs). Index 5 begins segment 2, so the bridge leg (seg1 end → seg2 start)
/// is a pause bridge. Segment 2 stays at the equator, so re-anchoring it back
/// onto segment 1 is a pure-longitude translation that preserves haversine
/// EXACTLY, the parity oracle for the VI/split verdicts.
fn relocation_fixture() -> Vec<GpsPoint> {
    vec![
        pt(0.0, 0.000, 0),
        pt(0.0, 0.001, 10),
        pt(0.0, 0.002, 20),
        pt(0.0, 0.003, 30),
        pt(0.0, 0.004, 40),
        // 60 s pause + ~111 km relocation east.
        pt(0.0, 1.000, 100),
        pt(0.0, 1.001, 110),
        pt(0.0, 1.002, 120),
        pt(0.0, 1.003, 130),
        pt(0.0, 1.004, 140),
    ]
}

/// The re-anchored equivalent track: segment 2 shifted onto
/// segment 1's end (pure-longitude shift at the equator), collapsing the
/// bridge to zero length. Empty `segment_starts` reproduces the whole-track metric.
fn reanchored_equivalent() -> Vec<GpsPoint> {
    let full = relocation_fixture();
    let off_lon = full[4].lon - full[5].lon; // 0.004 - 1.000
    let mut out = full[..5].to_vec();
    for p in &full[5..] {
        out.push(pt(p.lat, p.lon + off_lon, p.observed_at));
    }
    out
}

#[test]
fn zero_and_out_of_range_segment_starts_are_no_ops() {
    // Index 0 never has an entering leg and 999 is past the track, so both
    // leave every track metric equal to the empty-`segment_starts` form.
    let t = reanchored_equivalent(); // one continuous run with slight structure
    for starts in [&[0][..], &[999][..]] {
        assert_eq!(
            track_distance_km(&t, &[]),
            track_distance_km(&t, starts),
            "distance, starts={starts:?}",
        );
        assert_eq!(
            track_positive_split_pct(&t, &[]),
            track_positive_split_pct(&t, starts),
            "split, starts={starts:?}",
        );
        assert_eq!(
            track_splits(&t, KM_M, &[]),
            track_splits(&t, KM_M, starts),
            "km splits, starts={starts:?}",
        );
        assert_eq!(
            track_variability_index(&t, &[]),
            track_variability_index(&t, starts),
            "VI, starts={starts:?}",
        );
        assert_eq!(
            export_gpx(&t, "run", &[], None),
            export_gpx(&t, "run", starts, None),
            "gpx, starts={starts:?}",
        );
    }
}

#[test]
fn segmented_distance_excludes_the_pause_bridge() {
    let t = relocation_fixture();
    // Without boundaries the bridge leg is counted, capped at the
    // plausible-runner ceiling (12 m/s × 60 s ≈ 0.72 km) instead of its raw
    // ~111 km, so it swamps the two real segments.
    let naive = track_distance_km(&t, &[]);
    let capped_bridge_km = MAX_PLAUSIBLE_SPEED_MPS * 60.0 / 1000.0;
    assert!(
        (naive - (0.889 + capped_bridge_km)).abs() < 0.05,
        "naive sum carries the capped bridge: {naive}"
    );
    // With the boundary at index 5 the bridge contributes nothing → only the
    // two ~445 m segments remain (~0.89 km).
    let seg = track_distance_km(&t, &[5]);
    assert!((seg - 0.889).abs() < 0.05, "segmented distance {seg}");
    // And it equals the distance of the re-anchored equivalent (bit-identical
    // route length, no shift needed).
    assert!(
        (seg - track_distance_km(&reanchored_equivalent(), &[])).abs() < 1e-9,
        "segmented == re-anchored distance",
    );
}

#[test]
fn standing_still_wander_is_not_distance_of_record() {
    // 100 m run leg (5 m/s), then a 3 m GPS wander over 60 s while standing
    // still (0.05 m/s, under the auto-pause floor). The saved distance must
    // be the run leg alone: counting the wander would make the recorded
    // distance disagree with the moving pace derived from the same track.
    let run_leg_deg = 100.0 / M_PER_DEG; // ~100 m of longitude at the equator
    let wander_deg = 3.0 / M_PER_DEG;
    let t = vec![
        pt(0.0, 0.0, 0),
        pt(0.0, run_leg_deg, 20),
        pt(0.0, run_leg_deg + wander_deg, 80),
    ];
    let d = track_distance_km(&t, &[]);
    let run_leg_km = haversine_m(t[0], t[1]) / 1000.0;
    let wander_km = haversine_m(t[1], t[2]) / 1000.0;
    assert!(
        (d - run_leg_km).abs() < 1e-12,
        "distance is the moving leg only: {d} vs {run_leg_km}"
    );
    assert!(
        (d - (run_leg_km + wander_km)).abs() > 0.002,
        "the 3 m standstill wander must not be in the total: {d}"
    );
}

#[test]
fn segmented_vi_and_split_match_the_reanchored_equivalent() {
    let t = relocation_fixture();
    let re = reanchored_equivalent();
    // The re-anchored track is the oracle: the segment-aware metrics on
    // the TRUE coords must equal it (equatorial relocation ⇒ exact).
    assert_eq!(
        track_variability_index(&t, &[5]),
        track_variability_index(&re, &[]),
        "VI parity vs re-anchored",
    );
    assert_eq!(
        track_positive_split_pct(&t, &[5]),
        track_positive_split_pct(&re, &[]),
        "split parity vs re-anchored",
    );
    // Raw splits agree to f64 rounding (the equatorial re-anchor translation is
    // exact only up to the last ULP, so compare per field with a tolerance).
    let seg_splits = track_splits(&t, KM_M, &[5]);
    let re_splits = track_splits(&re, KM_M, &[]);
    assert_eq!(seg_splits.len(), re_splits.len(), "same split count");
    for (a, b) in seg_splits.iter().zip(&re_splits) {
        assert_eq!(a.index, b.index);
        assert_eq!(a.partial, b.partial);
        assert!((a.pace_sec_per_unit - b.pace_sec_per_unit).abs() < 1e-6);
        assert!((a.cumulative_m - b.cumulative_m).abs() < 1e-6);
        assert!((a.distance_m - b.distance_m).abs() < 1e-6);
    }
}

#[test]
fn gpx_emits_one_trkseg_per_segment_with_true_coords() {
    let t = relocation_fixture();
    let gpx = export_gpx(&t, "paused run", &[5], None);
    assert_eq!(gpx.matches("<trkseg>").count(), 2, "two segments\n{gpx}");
    assert_eq!(gpx.matches("</trkseg>").count(), 2);
    // TRUE coordinates: segment 2's real relocated longitude (1.000…) is
    // present, NOT re-anchored back onto segment 1 (0.004…).
    assert!(gpx.contains("lon=\"1\""), "true seg-2 coord\n{gpx}");
    // Every real fix is still emitted (10 points across the two segments).
    assert_eq!(gpx.matches("<trkpt").count(), 10);
    let flat = export_gpx(&t, "paused run", &[], None);
    assert_eq!(flat.matches("<trkseg>").count(), 1);
}

#[test]
fn gpx_names_the_runs_logged_at_stamp_before_its_track() {
    // A run logged after its last kept fix (a standing-still finish, a trim):
    // the document names the stamp in lollop's namespaced track extension.
    let t = relocation_fixture();
    let gpx = export_gpx(&t, "run", &[], Some(1_700_000_000));
    assert!(gpx.contains(" xmlns:lollop=\"urn:lollop:gpx:1\">"), "{gpx}");
    let ext = "<extensions><lollop:logged_at>2023-11-14T22:13:20Z</lollop:logged_at></extensions>";
    let at = gpx.find(ext).expect("the stamp is written");
    assert!(
        at < gpx.find("<trkseg>").unwrap(),
        "extensions precede the segments\n{gpx}"
    );
}

#[test]
fn thinning_keeps_a_track_at_or_under_the_cap_whole() {
    assert_eq!(
        thin_track(3, TRACK_DECIMATION_CAP as usize, &[]).0,
        [0, 1, 2]
    );
    let (kept, starts) = thin_track(100, TRACK_DECIMATION_CAP as usize, &[]);
    assert_eq!(kept, (0..100).collect::<Vec<_>>());
    assert!(starts.is_empty());
    // No cap (the shell's whole-track read): every start where it was.
    let (kept, starts) = thin_track(60, i32::MAX as usize, &[51, 7]);
    assert_eq!(kept.len(), 60);
    assert_eq!(starts, [7, 51]);
    // A cap under two can't hold both ends: nothing is dropped.
    assert_eq!(thin_track(5, 1, &[2]), (vec![0, 1, 2, 3, 4], vec![2]));
    assert_eq!(thin_track(0, 4, &[]), (vec![], vec![]));
}

#[test]
fn thinning_keeps_both_ends_within_the_cap() {
    // A 20-minute 1 Hz save to 300, and a very long interrupted run recovered
    // to the save cap. Without segment starts only the last fix can push the
    // count one past the cap.
    for (n, cap) in [(1_200, 300), (20_000, TRACK_DECIMATION_CAP as usize)] {
        let (kept, _) = thin_track(n, cap, &[]);
        assert!(kept.len() < n, "thins below the source: {}", kept.len());
        assert!(kept.len() <= cap + 1, "respects the cap: {}", kept.len());
        assert_eq!(kept.first(), Some(&0), "first fix kept");
        assert_eq!(kept.last(), Some(&(n - 1)), "last fix kept");
        assert!(kept.windows(2).all(|w| w[0] < w[1]), "ascending");
    }
}

#[test]
fn thinning_keeps_every_segments_first_fix_off_the_stride() {
    // 401 fixes to 40: stride 11, so the pause boundary at 201 is off the
    // uniform grid and only the segment-start rule keeps it; its remapped
    // position points back at the same fix.
    let kept = decimation_keep(401, 40, &[201]);
    assert_eq!(201 % 11, 3, "off the stride");
    let starts = remap_starts(&kept, &[201]);
    assert_eq!(starts.len(), 1, "one remapped boundary");
    assert_eq!(kept[starts[0] as usize], 201);
    // A recovered run: 100 fixes to 10 (stride 10), a segment opened at 37.
    let (kept, starts) = thin_track(100, 10, &[37]);
    assert!(kept.len() < 100, "thinned: {}", kept.len());
    assert_eq!(starts.len(), 1);
    assert_eq!(
        kept[starts[0] as usize], 37,
        "the segment's first fix opens it"
    );
}

#[test]
fn thinning_reads_starts_in_any_order_and_ignores_those_off_the_track() {
    // Ten fixes to ~4: stride 3 (0, 3, 6, 9) plus the start 5, position 2.
    // Index 0 opens no segment and 12 is past the end; a repeat counts once.
    assert_eq!(
        thin_track(10, 4, &[12, 5, 0, 5]),
        (vec![0, 3, 5, 6, 9], vec![2])
    );
    let kept = decimation_keep(10, 4, &[5]);
    assert_eq!(remap_starts(&kept, &[5]), [2]);
}
