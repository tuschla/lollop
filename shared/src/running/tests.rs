use super::*;

fn pt(lat: f64, lon: f64, t: i64) -> GpsPoint {
    GpsPoint {
        lat,
        lon,
        observed_at: t,
        accuracy_m: 5.0,
    }
}

#[test]
fn windowed_bins_correct_normally_and_bounded_when_dt_is_corrupt() {
    // Fix (normal): a 90 s leg at 3 m/s resamples into three full 30 s bins,
    // each averaging 3 m/s: the smoothing behaviour is unchanged.
    let normal = windowed_bin_speeds(&[(90.0, 270.0)], INTERVAL_WINDOW_SEC);
    assert_eq!(normal.len(), 3, "90 s / 30 s window = 3 bins");
    for (speed, dur) in &normal {
        assert!((speed - 3.0).abs() < 1e-9 && (*dur - 30.0).abs() < 1e-9);
    }
    // Corrupt: a single leg with a mis-unit'd dt (~4e7 s ≈ a millisecond
    // timestamp mistaken for seconds) would drive the resampling loop for
    // ~1.3M iterations; the cap bounds it to MAX_WINDOW_BINS and returns.
    let corrupt = windowed_bin_speeds(&[(4.0e7, 4.0e7)], INTERVAL_WINDOW_SEC);
    assert_eq!(corrupt.len(), MAX_WINDOW_BINS, "bin count is clamped");
    // A non-positive window can never spin forever.
    assert!(windowed_bin_speeds(&[(90.0, 270.0)], 0.0).is_empty());
}

#[test]
fn track_splits_reject_sub_metre_unit_and_bound_a_corrupt_track() {
    // A normal km split still works (fix half of the pair).
    let track = vec![pt(0.0, 0.0, 0), pt(0.0, 0.02, 300)]; // ~2.2 km, 5 min
    assert!(
        !track_splits(&track, 30.0, KM_M).is_empty(),
        "km splits still work"
    );
    // A sub-metre unit is rejected up front: the boundary loop below emits
    // one split per unit, so a tiny unit would iterate millions of times.
    assert!(
        track_splits(&track, 30.0, 0.5).is_empty(),
        "sub-metre unit rejected"
    );
    assert!(
        track_splits(&track, 30.0, 0.0).is_empty(),
        "zero unit rejected"
    );
    // Even at a legit unit, a corrupt long track cannot emit unbounded splits.
    // The cumulative walk caps each leg at the plausible-runner ceiling, so
    // the corrupt leg must span enough time (~23 days) that even the capped
    // 12 m/s distance (~24 000 km) still overruns the split cap.
    let corrupt = vec![pt(0.0, 0.0, 0), pt(0.0, 120.0, 2_000_000)];
    assert_eq!(
        track_splits(&corrupt, 30.0, KM_M).len(),
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
    let split = track_positive_split_pct(&track, 30.0).expect("split");
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
    let split = track_positive_split_pct(&track, 30.0).expect("split");
    assert!(split.abs() < 1.0, "got {split}");
}

#[test]
fn mid_run_pause_does_not_produce_a_false_positive_split() {
    // Evenly paced ~3.7 m/s throughout, but with a 10-min standstill in the
    // SECOND half (the app's Pause button leaves a ~0-speed bridge leg). With
    // wall-clock halves this read a massive "FADE" (back half ~11× slower);
    // moving-time halves must see it as the even run it is.
    let track = vec![
        pt(0.0, 0.000, 0),
        pt(0.0, 0.001, 30),
        pt(0.0, 0.002, 60),  // ~halfway
        pt(0.0, 0.002, 660), // 10-min stop, 0 m → excluded
        pt(0.0, 0.003, 690),
        pt(0.0, 0.004, 720),
    ];
    let split = track_positive_split_pct(&track, 30.0).expect("split");
    assert!(
        split.abs() < 1.0,
        "paused steady run falsely split: {split}"
    );
}

#[test]
fn positive_split_needs_three_usable_fixes() {
    let track = vec![pt(0.0, 0.0, 0), pt(0.0, 0.001, 10)];
    assert!(track_positive_split_pct(&track, 30.0).is_none());
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
    let km = track_splits(&track, 30.0, KM_M);
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
    // Indices are 1-based and contiguous.
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

    let km = track_splits(&track, 30.0, KM_M);
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
    // Explicitly: split 1 (which contains the stop) equals split 2.
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
    // `track_distance_km` is 0. The split walk now counts only moving legs, so
    // it agrees: no phantom split for distance the headline excludes.
    let track: Vec<GpsPoint> = (0..=4)
        .map(|i| pt(0.0, i as f64 * 0.001, i * 300))
        .collect();
    assert_eq!(
        track_distance_km(&track, 30.0),
        0.0,
        "headline is zero moving km"
    );
    assert!(
        track_splits(&track, 30.0, KM_M).is_empty(),
        "an all-stopped track yields no splits, matching its zero headline"
    );
}

#[test]
fn splits_final_partial_is_flagged() {
    // ≈5.56 km → the 6th km split is partial (~0.56 km).
    let track = even_equator_track(50, 0.001, 30);
    let km = track_splits(&track, 30.0, KM_M);
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
    let km = track_splits(&track, 30.0, KM_M);
    let mi = track_splits(&track, 30.0, MILE_M);
    assert_eq!(km.len(), 6, "km splits");
    assert_eq!(mi.len(), 4, "mile splits");
    assert_eq!(mi.iter().filter(|s| !s.partial).count(), 3, "3 full miles");
    assert!(mi.last().unwrap().partial);
}

#[test]
fn splits_under_one_unit_is_single_partial() {
    // ~445 m: under a km → one partial split, index 1.
    let track = even_equator_track(4, 0.001, 30);
    let km = track_splits(&track, 30.0, KM_M);
    assert_eq!(km.len(), 1);
    assert_eq!(km[0].index, 1);
    assert!(km[0].partial);
    assert!(km[0].distance_m < KM_M);
}

#[test]
fn splits_degenerate_tracks_are_empty() {
    assert!(track_splits(&[], 30.0, KM_M).is_empty(), "empty track");
    assert!(
        track_splits(&[pt(0.0, 0.0, 0)], 30.0, KM_M).is_empty(),
        "single fix"
    );
    // Zero-distance track (all fixes stacked) → nothing to split.
    let stacked = vec![pt(0.0, 0.0, 0), pt(0.0, 0.0, 10), pt(0.0, 0.0, 20)];
    assert!(
        track_splits(&stacked, 30.0, KM_M).is_empty(),
        "zero distance"
    );
    // Non-positive unit.
    assert!(track_splits(&even_equator_track(50, 0.001, 30), 30.0, 0.0).is_empty());
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
    let ds = track_distance_km(&steady, 30.0);
    let di = track_distance_km(&interval, 30.0);
    assert!((ds - di).abs() < 0.001, "distances differ: {ds} vs {di}");

    let vi_steady = track_variability_index(&steady, 30.0).expect("steady VI");
    let vi_interval = track_variability_index(&interval, 30.0).expect("interval VI");

    // Steady ≈ 1.0; interval well above it (jog-recovery 4th-power mean ~1.5).
    assert!((vi_steady - 1.0).abs() < 0.03, "steady VI {vi_steady}");
    assert!(vi_interval > 1.4, "interval VI {vi_interval}");
    assert!(
        vi_interval > vi_steady + 0.3,
        "interval {vi_interval} not clearly above steady {vi_steady}",
    );

    // Normalized speed exceeds the plain average for the interval run.
    let ns = normalized_speed_mps(&interval, 30.0).expect("interval NS");
    let avg = di * 1000.0 / 120.0;
    assert!(ns > avg, "NS {ns} should exceed avg {avg}");
}

#[test]
fn variability_index_needs_two_usable_fixes() {
    assert!(normalized_speed_mps(&[pt(0.0, 0.0, 0)], 30.0).is_none());
    assert!(track_variability_index(&[pt(0.0, 0.0, 0)], 30.0).is_none());
}

/// Build a DENSE (1 Hz) GPS track from a speed profile `(speed_mps,
/// duration_s)`, with deterministic lateral position jitter to mimic a real
/// consumer GPS at ~a few metres accuracy. Forward motion runs along lon at
/// the equator (1° ≈ 111 320 m); the lateral wobble is `jitter_deg·sin(i·1.3)`
/// on lat, which makes consecutive per-second leg lengths swing (exactly the
/// noise that, un-smoothed, blows up the 4th-power mean).
fn noisy_track(profile: &[(f64, i64)], jitter_deg: f64) -> Vec<GpsPoint> {
    let mut pts = Vec::new();
    let mut lon = 0.0f64;
    let mut t = 0i64;
    let deg_per_m = 1.0 / 111_320.0;
    // Seed the first fix.
    pts.push(GpsPoint {
        lat: 0.0,
        lon: 0.0,
        observed_at: 0,
        accuracy_m: 5.0,
    });
    for &(speed, dur) in profile {
        for _ in 0..dur {
            lon += speed * deg_per_m; // advance ~speed metres this second
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

#[test]
fn steady_noisy_1hz_run_is_not_flagged_interval() {
    // A genuinely STEADY 5-min run at ~3 m/s, sampled 1 Hz with ~3 m lateral
    // GPS jitter. Per-second leg speeds swing wildly; without the rolling
    // window the 4th-power mean would read this as an "interval". With the
    // window, jitter averages out → VI must stay steady.
    let track = noisy_track(&[(3.0, 300)], 0.000_03);
    let vi = track_variability_index(&track, 30.0).expect("vi");
    assert!(
        vi < INTERVAL_VI_THRESHOLD,
        "steady noisy run wrongly flagged interval: VI {vi}"
    );
}

#[test]
fn short_noisy_1hz_run_is_not_flagged_interval() {
    // The exact failure the user reported: a short ~1 km steady run (≈5.5 min
    // at 3 m/s) sampled 1 Hz with jitter must NOT read as interval.
    let track = noisy_track(&[(3.0, 330)], 0.000_03);
    let vi = track_variability_index(&track, 30.0).expect("vi");
    assert!(
        vi < INTERVAL_VI_THRESHOLD,
        "short steady run wrongly flagged: VI {vi}"
    );
}

#[test]
fn interval_noisy_1hz_run_is_flagged() {
    // A real 1 Hz interval session at realistic (~1 m) GPS jitter: 4× (90 s
    // hard @ ~5 m/s, then 90 s jog @ ~2 m/s). The rep structure spans many
    // 30 s windows, so it survives smoothing and flags interval. (Under
    // pathologically heavy high-frequency noise VI can still fall under the
    // cutoff: the metric is Weak-graded; this guards the realistic case.)
    let mut profile = Vec::new();
    for _ in 0..4 {
        profile.push((5.0, 90));
        profile.push((2.0, 90));
    }
    let track = noisy_track(&profile, 0.000_01);
    let vi = track_variability_index(&track, 30.0).expect("vi");
    assert!(
        vi >= INTERVAL_VI_THRESHOLD,
        "interval run not flagged: VI {vi}"
    );
}

/// Save-time GPS decimation (shell `RunSession.decimatedTrackForCore`): keep
/// every `stride`-th fix plus the endpoints. These tests prove the core's
/// of-record figures survive that lossy thinning at strides 2 and 3: the
/// shell may hand the core fewer points, and the verdicts must not move.
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
    // A genuinely steady noisy run reads steady (VI < threshold); thinning it
    // ×2 and ×3 must keep that verdict, fewer points, same call.
    let full = noisy_track(&[(3.0, 300)], 0.000_03);
    for stride in [2usize, 3] {
        let thinned = decimate(&full, stride);
        let vi = track_variability_index(&thinned, 30.0).expect("vi");
        assert!(
            vi < INTERVAL_VI_THRESHOLD,
            "steady run flipped to interval after ×{stride} decimation: VI {vi}"
        );
    }
}

#[test]
fn decimation_preserves_the_interval_vi_verdict() {
    // A real interval session (4× 90 s hard / 90 s jog) flags interval; the
    // 30 s rolling window still gets ~10–15 samples/bin at a 2–3 s cadence,
    // so the verdict survives ×2 and ×3 thinning.
    let mut profile = Vec::new();
    for _ in 0..4 {
        profile.push((5.0, 90));
        profile.push((2.0, 90));
    }
    let full = noisy_track(&profile, 0.000_01);
    for stride in [2usize, 3] {
        let thinned = decimate(&full, stride);
        let vi = track_variability_index(&thinned, 30.0).expect("vi");
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
        let full_km = track_distance_km(&track, 30.0);
        for stride in [2usize, 3] {
            let thinned_km = track_distance_km(&decimate(&track, stride), 30.0);
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
        let ps_pos = track_positive_split_pct(&decimate(&positive, stride), 30.0).expect("pos");
        let ps_neg = track_positive_split_pct(&decimate(&negative, stride), 30.0).expect("neg");
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
    let vi = track_variability_index(&steady_paused, 30.0).expect("vi");
    assert!(
        vi < INTERVAL_VI_THRESHOLD,
        "paused steady wrongly flagged: VI {vi}"
    );
    assert!((vi - 1.0).abs() < 0.05, "VI {vi} should be ~1.0");
}

#[test]
fn normalized_speed_clamps_gps_jitter_spike() {
    // A single teleport fix (huge distance in 1 s) must be clamped, not allowed
    // to blow up the 4th-power mean. Two clean 30 s segments plus one spike.
    let spiky = vec![
        pt(0.0, 0.000, 0),
        pt(0.0, 0.001, 30),
        pt(5.0, 5.000, 31), // ~780 km in 1 s → clamp to MAX_PLAUSIBLE_SPEED_MPS
        pt(5.0, 5.001, 61),
    ];
    let ns = normalized_speed_mps(&spiky, 30.0).expect("ns");
    assert!(ns <= MAX_PLAUSIBLE_SPEED_MPS + 1e-9, "NS {ns} not clamped");
}

#[test]
fn track_distance_sums_segments_and_drops_noisy_fixes() {
    let clean = vec![pt(0.0, 0.0, 0), pt(0.0, 0.001, 10), pt(0.0, 0.002, 20)];
    let d = track_distance_km(&clean, 30.0);
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
    let d2 = track_distance_km(&noisy, 30.0);
    assert!((d2 - 0.2226).abs() < 0.001, "got {d2}");
}

#[test]
fn rfc3339_renders_known_epochs_utc() {
    assert_eq!(unix_to_rfc3339_utc(0), "1970-01-01T00:00:00Z");
    // 1_700_000_000 seconds converts to the fixed UTC instant asserted below
    // (verified against a UTC clock).
    assert_eq!(unix_to_rfc3339_utc(1_700_000_000), "2023-11-14T22:13:20Z");
}

#[test]
fn export_gpx_wraps_fixes_and_escapes_name() {
    let track = vec![
        pt(52.52, 13.405, 1_700_000_000),
        pt(52.521, 13.406, 1_700_000_030),
    ];
    let gpx = export_gpx(&track, "Tom & \"Jerry\" <run>'s");
    assert!(gpx.starts_with("<?xml version=\"1.0\""));
    assert!(gpx.contains("<gpx version=\"1.1\""));
    // All five XML predefined entities are escaped so the name cannot break
    // the document (& " < > ').
    assert!(gpx.contains("<name>Tom &amp; &quot;Jerry&quot; &lt;run&gt;&apos;s</name>"));
    // One trkpt per fix, with lat/lon and an RFC-3339 timestamp.
    assert_eq!(gpx.matches("<trkpt ").count(), 2);
    assert!(gpx.contains("lat=\"52.5200000\" lon=\"13.4050000\""));
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
    let gpx = export_gpx(&[a, b, unknown], "run");
    assert!(gpx.contains("</time><hdop>1</hdop></trkpt>"), "{gpx}");
    assert!(gpx.contains("<hdop>2.5</hdop>"), "{gpx}");
    assert_eq!(
        gpx.matches("<hdop>").count(),
        2,
        "no hdop for unknown accuracy"
    );
    let hdop: f32 = gpx
        .split("<hdop>")
        .nth(2)
        .unwrap()
        .split('<')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(hdop * GPX_HDOP_UERE_M, 12.5);
}

#[test]
fn usable_track_drops_noisy_fixes() {
    let track = vec![
        pt(52.52, 13.405, 0),
        GpsPoint {
            lat: 52.60,
            lon: 13.50,
            observed_at: 30,
            accuracy_m: 80.0, // beyond the gate → excluded
        },
        pt(52.521, 13.406, 60),
    ];
    // The GPX must carry the same fixes distance uses, the noisy one gone.
    let usable = usable_track(&track, MAX_GPS_ACCURACY_M);
    assert_eq!(usable.len(), 2);
    let gpx = export_gpx(&usable, "run");
    assert_eq!(gpx.matches("<trkpt ").count(), 2);
    assert!(!gpx.contains("lat=\"52.6000000\""));
}

#[test]
fn tanaka_at_age_30_is_about_187() {
    // 208 - 0.7*30 = 187.0
    let hr = hr_max_tanaka(30.0);
    assert!((hr - 187.0).abs() < 1e-9, "got {hr}");
}

#[test]
fn three_zone_boundaries() {
    assert_eq!(classify_three_zone(70.0), ThreeZone::Z1);
    assert_eq!(classify_three_zone(81.9), ThreeZone::Z1);
    // 82 is the LT1 boundary → Z2 (inclusive lower edge).
    assert_eq!(classify_three_zone(82.0), ThreeZone::Z2);
    assert_eq!(classify_three_zone(87.9), ThreeZone::Z2);
    // 88 is the LT2 boundary → Z3 (inclusive to the hard zone), so a run at
    // the prescribed Tempo floor (0.88) counts as threshold work.
    assert_eq!(classify_three_zone(88.0), ThreeZone::Z3);
    assert_eq!(classify_three_zone(88.1), ThreeZone::Z3);
    assert_eq!(classify_three_zone(95.0), ThreeZone::Z3);
}

#[test]
fn vdot_table_values_verbatim() {
    assert_eq!(vdot_band_hr_pct(VdotBand::Easy), (65.0, 79.0));
    assert_eq!(vdot_band_vo2max_pct(VdotBand::Marathon), (80.0, Some(84.0)));
    assert_eq!(vdot_band_hr_pct(VdotBand::Threshold), (88.0, 92.0));
    assert!(!vdot_band_uses_hr(VdotBand::Repetition));
    assert!(vdot_band_uses_hr(VdotBand::Threshold));
    // R is >100 %VO2max: open upper bound.
    assert_eq!(vdot_band_vo2max_pct(VdotBand::Repetition).1, None);
}

#[test]
fn long_run_cap_pass_and_fail() {
    // 12 km of a 50 km week = 24 % ≤ 25 %: pass.
    assert!(long_run_within_cap(12.0, 50.0));
    // Exactly 25 %: pass (cap is inclusive).
    assert!(long_run_within_cap(12.5, 50.0));
    // 15 km of 50 km = 30 %: fail.
    assert!(!long_run_within_cap(15.0, 50.0));
    // Zero weekly volume cannot satisfy a share cap.
    assert!(!long_run_within_cap(5.0, 0.0));
}

#[test]
fn spike_true_just_above_10pct_false_at_or_below() {
    // 10 % over exactly: not a spike (rule is strictly >10 %).
    assert!(!single_session_spike(22.0, 20.0));
    // Just above 10 %: spike.
    assert!(single_session_spike(22.01, 20.0));
    // Below the longest run: never a spike.
    assert!(!single_session_spike(18.0, 20.0));
    // No history → any real session errs to spike.
    assert!(single_session_spike(5.0, 0.0));
}

#[test]
fn hr_method_and_recalc() {
    assert!(prefer_karvonen(50.0).value);
    assert!(!prefer_karvonen(55.0).value);
    assert!(!prefer_karvonen(72.0).value);
    assert!(!hr_zone_recalc_due(3).value);
    assert!(hr_zone_recalc_due(4).value);
}

#[test]
fn vdot_derate_and_goal_plan() {
    assert_eq!(
        vdot_derate_points(GoalDistance::HalfMarathon, true).value,
        (1.0, 1.5)
    );
    assert_eq!(
        vdot_derate_points(GoalDistance::Marathon, true).value,
        (2.0, 3.0)
    );
    assert_eq!(
        vdot_derate_points(GoalDistance::FiveK, true).value,
        (0.0, 0.0)
    );
    assert_eq!(
        vdot_derate_points(GoalDistance::Marathon, false).value,
        (0.0, 0.0)
    );
    let c = goal_week_plan(GoalDistance::C25k, false).value;
    assert_eq!((c.sessions_per_week, c.quality_per_week), ((3, 3), (0, 0)));
    // running-025 defines no long run for C25K: the share is 0, not 25-30%.
    assert_eq!(c.long_run_share, (0.0, 0.0));
    assert_eq!(
        goal_week_plan(GoalDistance::FiveK, false)
            .value
            .quality_per_week,
        (2, 2)
    );
    assert_eq!(
        goal_week_plan(GoalDistance::Marathon, true)
            .value
            .sessions_per_week,
        (5, 7)
    );
    assert_eq!(
        goal_week_plan(GoalDistance::Marathon, false)
            .value
            .sessions_per_week,
        (4, 6)
    );
    // marathon_derated_band (running-040/008, option B): the optimistic
    // band is shifted SLOWER, the fast bound by the min derate and the slow
    // bound by the max, so the range moves later and widens, and the
    // result carries the same RUN-VDOT-001 evidence as the derate itself.
    let vdot = crate::load::vdot(10_000.0, 2_520.0); // a real 42:00 10K
    let base_low = 12_000.0;
    let base_high = 12_600.0;
    let band = marathon_derated_band(base_low, base_high, vdot);
    assert!(
        band.value.0 > base_low && band.value.1 > base_high,
        "both bounds slow down: {:?} vs ({base_low}, {base_high})",
        band.value
    );
    assert!(
        band.value.1 - band.value.0 > base_high - base_low,
        "the derate widens the span by its own uncertainty: {:?}",
        band.value
    );
    assert_eq!(
        band.claim_id(),
        claims::RUN_VDOT_001,
        "the derated number still cites running-040/008 (HARD RULE 2)"
    );
    // A degenerate prediction (no valid race → VDOT 0) is left untouched,
    // never fabricated slower.
    let degen = marathon_derated_band(0.0, 0.0, 0.0);
    assert_eq!(degen.value, (0.0, 0.0));
    // A VDOT the 2–3 point derate would take to zero keeps its band: no
    // bound collapses to a 0:00 finish.
    let tiny = marathon_derated_band(36_000.0, 40_000.0, 2.5);
    assert_eq!(tiny.value, (36_000.0, 40_000.0));

    let p = c25k_plan().value;
    assert_eq!(
        (
            p.runs_per_week,
            p.rest_day_between,
            p.repeat_hard_week_allowed
        ),
        (3, true, true)
    );
}

#[test]
fn distribution_and_progression_guards() {
    assert!(easy_share_floor_ok(0.80).value);
    assert!(!easy_share_floor_ok(0.79).value);
    assert_eq!(
        default_counting_method(true).value,
        IntensityCountingMethod::SessionGoal
    );
    assert_eq!(
        default_counting_method(false).value,
        IntensityCountingMethod::TimeInZone
    );
    assert_eq!(novice_volume_bump_hold_weeks(0.5).value, Some((2, 3)));
    assert_eq!(novice_volume_bump_hold_weeks(3.0).value, None);
}

#[test]
fn maf_cap_adjusts() {
    assert_eq!(maf_cap_bpm(40.0, MafAdjustment::None).value, 140.0);
    assert_eq!(
        maf_cap_bpm(40.0, MafAdjustment::EliteImproving).value,
        145.0
    );
    assert_eq!(maf_cap_bpm(40.0, MafAdjustment::Returning).value, 135.0);
    assert_eq!(maf_cap_bpm(40.0, MafAdjustment::Overtrained).value, 130.0);
    // Weak + contested per global CQ-12 (MAF vs measured LT1).
    assert!(maf_cap_bpm(40.0, MafAdjustment::None).contested().is_some());
}

#[test]
fn distribution_models_keep_easy_at_80() {
    let pyr = intensity_distribution(DistributionModel::Pyramidal).value;
    assert_eq!((pyr.easy_pct, pyr.moderate_pct, pyr.hard_pct), (80, 15, 5));
    let pol = intensity_distribution(DistributionModel::Polarized).value;
    assert_eq!((pol.easy_pct, pol.moderate_pct, pol.hard_pct), (80, 5, 15));
    // Phase mapping: base pyramidal, peak polarized.
    assert_eq!(distribution_for_phase(MesoPhase::Base).value.hard_pct, 5);
    assert_eq!(distribution_for_phase(MesoPhase::Peak).value.hard_pct, 15);
}

#[test]
fn quality_plan_respects_caps() {
    // 3 sessions, 48h apart, no back-to-back Z3: ok.
    assert!(quality_plan_ok(3, 48, false).value);
    // 4 sessions: too many.
    assert!(!quality_plan_ok(4, 48, false).value);
    // Too tight spacing.
    assert!(!quality_plan_ok(2, 24, false).value);
    // Consecutive Z3 never allowed.
    assert!(!quality_plan_ok(2, 48, true).value);
}

#[test]
fn weekly_increase_cap_scales_with_training_age() {
    assert!((weekly_increase_cap_frac(0.5).value - 0.10).abs() < 1e-9);
    assert!((weekly_increase_cap_frac(3.0).value - 0.05).abs() < 1e-9);
    // Novice +10% ok, experienced +10% not.
    assert!(weekly_increase_ok(50.0, 55.0, 0.5).value);
    assert!(!weekly_increase_ok(50.0, 55.0, 3.0).value);
    // Zero base cannot be ratioed.
    assert!(!weekly_increase_ok(0.0, 10.0, 0.5).value);
}

#[test]
fn two_week_spike_flags_above_30pct() {
    // +30% exactly: not flagged (rule is strictly >30%).
    assert!(!two_week_increase_flag(40.0, 52.0).value);
    // Just above 30%: flagged.
    assert!(two_week_increase_flag(40.0, 52.1).value);
    // No history errs to flag.
    assert!(two_week_increase_flag(0.0, 10.0).value);
}

#[test]
fn deload_cadence_defaults_and_conservative() {
    let default = deload_cadence(false).value;
    assert_eq!((default.load_weeks, default.recovery_weeks), (3, 1));
    assert_eq!(deload_cadence(true).value.load_weeks, 2);
    let (lo, hi) = default.reduction_frac;
    assert!((lo - 0.20).abs() < 1e-9 && (hi - 0.40).abs() < 1e-9);
}

#[test]
fn check_volume_caps_flags_first_violation() {
    // All within caps: None.
    let ok = check_volume_caps(12.0, 4.0, 3.0, 2.0, 50.0);
    assert_eq!(ok.value, None);
    // Long run over 25 %: LongRun wins (checked first).
    let bad = check_volume_caps(20.0, 4.0, 3.0, 2.0, 50.0);
    assert_eq!(bad.value, Some(CapViolation::LongRun));
}

#[test]
fn workout_rx_hr_governance_split() {
    // Aerobic sessions are HR-governed.
    assert!(run_workout_rx(RunSessionType::Recovery).value.hr_governed);
    assert!(run_workout_rx(RunSessionType::LongRun).value.hr_governed);
    assert!(run_workout_rx(RunSessionType::Tempo).value.hr_governed);
    assert!(run_workout_rx(RunSessionType::RacePace).value.hr_governed);
    assert!(workout_rx(RunWorkout::EasyGeneralAerobic).value.hr_governed);
    // Short/max efforts are effort-governed, HR lags.
    assert!(!run_workout_rx(RunSessionType::Interval).value.hr_governed);
    assert!(!run_workout_rx(RunSessionType::Repetition).value.hr_governed);
    assert!(!run_workout_rx(RunSessionType::Strides).value.hr_governed);
    assert!(!run_workout_rx(RunSessionType::Hills).value.hr_governed);
}

#[test]
fn workout_rx_bands_verbatim() {
    // running-018 Tempo 88–92 %HRmax, RPE 6–7, 20–40 min continuous.
    let t = run_workout_rx(RunSessionType::Tempo).value;
    assert_eq!(t.pct_hr_max, (Some(0.88), Some(0.92)));
    assert_eq!(t.rpe, Some((6, 7)));
    assert_eq!(t.duration_min, (Some(20), Some(40)));
    // VDOT-table Interval 97–100 %HRmax (matches `vdot_band_hr_pct`), 3–5 min.
    let i = run_workout_rx(RunSessionType::Interval).value;
    assert_eq!(i.pct_hr_max, (Some(0.97), Some(1.00)));
    assert_eq!(i.duration_min, (Some(3), Some(5)));
    assert_eq!(i.rpe, Some((8, 9)));
}

#[test]
fn recovery_rx_has_open_lower_hr_and_both_ceilings() {
    // running-014: KB gives NO lower HR bound (the 0.65 floor was
    // invented), <76 %HRmax AND <70 %HRR ceilings, pace ≥20 % slower than
    // MP with no upper slowness bound, RPE 2–3, 20–40 min.
    let r = run_workout_rx(RunSessionType::Recovery);
    assert_eq!(r.value.pct_hr_max, (None, Some(0.76)));
    assert_eq!(r.value.pct_hrr_max, Some(0.70));
    assert_eq!(r.value.pct_slower_than_mp, (Some(20.0), None));
    assert_eq!(r.value.rpe, Some((2, 3)));
    assert_eq!(r.value.duration_min, (Some(20), Some(40)));
    assert_eq!(r.claim_id(), claims::RUN_RECOVERY_001);
}

#[test]
fn easy_ga_is_distinct_from_recovery() {
    // running-015: E pace 15–25 % slower than MP, 65–79 %HRmax, RPE 3–4,
    // 30–90 min, a different band set from Recovery on every axis.
    let e = workout_rx(RunWorkout::EasyGeneralAerobic);
    assert_eq!(e.value.pct_hr_max, (Some(0.65), Some(0.79)));
    assert_eq!(e.value.pct_slower_than_mp, (Some(15.0), Some(25.0)));
    assert_eq!(e.value.rpe, Some((3, 4)));
    assert_eq!(e.value.duration_min, (Some(30), Some(90)));
    assert_eq!(e.claim_id(), claims::RUN_EASY_001);
    assert_ne!(e.value, run_workout_rx(RunSessionType::Recovery).value);
}

#[test]
fn long_run_rx_has_no_duration_floor() {
    // running-016: no duration floor in the KB (the 60-min floor was
    // invented); soft time-cap ~2:00–2:30 → 150-min outer ceiling; pace
    // MP −10–20 %; 65–80 %HRmax; RPE 3–5.
    let lr = run_workout_rx(RunSessionType::LongRun).value;
    assert_eq!(lr.duration_min, (None, Some(150)));
    assert_eq!(lr.pct_hr_max, (Some(0.65), Some(0.80)));
    assert_eq!(lr.pct_slower_than_mp, (Some(10.0), Some(20.0)));
    assert_eq!(lr.rpe, Some((3, 5)));
}

#[test]
fn race_pace_rx_is_km_blocks_not_minutes() {
    // running-017: 8–26 km blocks (KB states km, not minutes), M pace,
    // 80–85 %HRmax, RPE 5–6. No weekly M cap exists (VDOT M row: "-").
    let m = run_workout_rx(RunSessionType::RacePace).value;
    assert_eq!(m.distance_km, (Some(8.0), Some(26.0)));
    assert_eq!(m.duration_min, (None, None));
    assert_eq!(m.pct_hr_max, (Some(0.80), Some(0.85)));
    assert_eq!(m.pct_slower_than_mp, (Some(0.0), Some(0.0)));
    assert_eq!(m.rpe, Some((5, 6)));
}

#[test]
fn repetition_rx_carries_only_kb_backed_parameters() {
    // VDOT R row only: reps ≤2 min, pace-not-HR. RPE is NOT stated in the
    // KB and no HR band exists.
    let r = run_workout_rx(RunSessionType::Repetition);
    assert_eq!(r.value.rpe, None);
    assert_eq!(r.value.pct_hr_max, (None, None));
    assert_eq!(r.value.duration_min, (None, Some(2)));
    assert!(!r.value.hr_governed);
    assert_eq!(r.claim_id(), claims::RUN_REPETITION_001);
}

#[test]
fn cruise_intervals_verbatim() {
    // running-018: 3–15 min reps, rest ~1 min or 20–25 %, total ≤10 %.
    let c = cruise_interval_rx().value;
    assert_eq!(c.rep_duration_min, (3, 15));
    assert!((c.rest_approx_min - 1.0).abs() < 1e-9);
    assert_eq!(c.rest_frac, (0.20, 0.25));
    assert!((c.weekly_cap_frac - 0.10).abs() < 1e-9);
}

#[test]
fn strides_and_hill_sprints_verbatim() {
    let s = strides_rx().value;
    assert_eq!(
        (s.rep_sec, s.reps, s.recovery_sec, s.per_week, s.rpe),
        ((15, 30), (4, 8), (45, 120), (1, 3), (6, 7))
    );
    let h = hill_sprint_rx().value;
    assert_eq!((h.rep_sec, h.reps), ((8, 20), (4, 10)));
    assert_eq!(h.effort_pct, (90.0, 95.0));
    assert_eq!(h.grade_pct, (6.0, 10.0));
    assert_eq!(h.recovery_approx_sec, 120);
    assert_eq!(h.rpe, 9);
    // Weekly frequency is NOT stated in the KB → never invented.
    assert_eq!(h.per_week, None);
    assert!(h.on_easy_days);
    // Placement contradiction (Lydiard vs Hudson) must ride along.
    assert!(!hill_sprint_rx().evidence().contradicting.is_empty());
}

#[test]
fn long_run_guardrails() {
    // running-016: LR ≤ 2× average daily run.
    assert!(long_run_within_daily_avg(16.0, 8.0).value);
    assert!(!long_run_within_daily_avg(16.1, 8.0).value);
    // No history errs safe.
    assert!(!long_run_within_daily_avg(10.0, 0.0).value);
    // long_run_share_default() carries RUN-LONGRUN-001 / running-016's 25–30 %.
    assert_eq!(long_run_share_default().value, (0.25, 0.30));
    // goal_week_plan is reconciled onto the 25–30% band (running-016 wins;
    // running-024's uncited 20% lower bound was corrected up).
    assert_eq!(
        goal_week_plan(GoalDistance::General, false)
            .value
            .long_run_share,
        (0.25, 0.30)
    );
}

#[test]
fn hr_method_preference_branches() {
    // running-005: <55 prefer Karvonen; ≥70 converged; [55,70) unstated.
    assert_eq!(
        hr_method_preference(54.9).value,
        HrMethodPreference::PreferKarvonen
    );
    assert_eq!(
        hr_method_preference(55.0).value,
        HrMethodPreference::Unstated
    );
    assert_eq!(
        hr_method_preference(69.9).value,
        HrMethodPreference::Unstated
    );
    assert_eq!(
        hr_method_preference(70.0).value,
        HrMethodPreference::EitherConverged
    );
}

#[test]
fn single_variable_progression_gate() {
    // running-032: volume OR intensity, never both in one week.
    assert!(single_variable_progression_ok(true, false).value);
    assert!(single_variable_progression_ok(false, true).value);
    assert!(single_variable_progression_ok(false, false).value);
    assert!(!single_variable_progression_ok(true, true).value);
}

#[test]
fn recovery_week_depth_by_mileage() {
    // running-033: general 20–40 %, higher-mileage 10–30 %, lower-mileage
    // up to 50 % (floor unstated); both volume and intensity reduced, one
    // quality session dropped.
    let g = recovery_week_rx(MileageBand::Unspecified).value;
    assert_eq!(g.volume_reduction_frac, (Some(0.20), 0.40));
    let hi = recovery_week_rx(MileageBand::Higher).value;
    assert_eq!(hi.volume_reduction_frac, (Some(0.10), 0.30));
    let lo = recovery_week_rx(MileageBand::Lower).value;
    assert_eq!(lo.volume_reduction_frac, (None, 0.50));
    assert!(g.reduce_intensity && g.drop_quality_session);
}

#[test]
fn distance_taper_defaults_verbatim() {
    // running-038 (Strong, safety-critical): distance-specific defaults.
    let five = distance_taper(GoalDistance::FiveK).expect("5K default");
    assert_eq!(five.value.days, (7, 10));
    assert_eq!(five.value.volume_cut_frac, (0.40, 0.50));
    assert_eq!(five.value.keep_sharp_sessions, Some((1, 2)));
    assert!(five.safety_critical());
    let ten = distance_taper(GoalDistance::TenK).expect("10K default");
    assert_eq!(ten.value.days, (7, 10));
    let hm = distance_taper(GoalDistance::HalfMarathon).expect("HM default");
    assert_eq!(hm.value.days, (10, 14));
    assert_eq!(hm.value.volume_cut_frac, (0.50, 0.50));
    assert!(!hm.value.progressive);
    let m = distance_taper(GoalDistance::Marathon).expect("marathon default");
    assert_eq!(m.value.days, (14, 21));
    assert_eq!(m.value.volume_cut_frac, (0.40, 0.60));
    assert!(m.value.progressive && m.value.keep_mp_touches_and_short_tempo);
    // Intensity + frequency always held; no defaults stated for
    // general/C25K goals.
    assert!(m.value.hold_intensity && m.value.hold_frequency);
    assert!(distance_taper(GoalDistance::General).is_none());
    assert!(distance_taper(GoalDistance::C25k).is_none());
}

#[test]
fn pace_retest_and_environment_triggers() {
    // running-041: re-test every 4–6 wk.
    assert!(!pace_retest_due(3).value);
    assert!(pace_retest_due(4).value);
    // Race input freshness window ≤6–8 wk.
    assert_eq!(race_input_freshness(6).value, RaceInputFreshness::Fresh);
    assert_eq!(race_input_freshness(7).value, RaceInputFreshness::Marginal);
    assert_eq!(race_input_freshness(8).value, RaceInputFreshness::Marginal);
    assert_eq!(race_input_freshness(9).value, RaceInputFreshness::Stale);
    // Corrections trigger above ~15 °C / ~900 m, flags only, the KB
    // states no magnitudes.
    let t = pace_correction_triggers(15.1, 900.1).value;
    assert!(t.heat && t.altitude);
    let none = pace_correction_triggers(15.0, 900.0).value;
    assert!(!none.heat && !none.altitude);
}

#[test]
fn marathon_gate_trips_under_30km() {
    assert!(marathon_prediction_optimistic(24.0).value);
    assert!(!marathon_prediction_optimistic(32.0).value);
    // Boundary: exactly 30 km is not optimistic.
    assert!(!marathon_prediction_optimistic(30.0).value);
}

#[test]
fn equivalency_agrees_or_ranges() {
    // Within 2 %: midpoint.
    match race_equivalency(3600.0, 3636.0).value {
        Equivalency::Agreed(m) => assert!((m - 3618.0).abs() < 1e-9),
        other => panic!("expected Agreed, got {other:?}"),
    }
    // >2 % apart: range, low first.
    match race_equivalency(3800.0, 3600.0).value {
        Equivalency::Range(lo, hi) => {
            assert!((lo - 3600.0).abs() < 1e-9 && (hi - 3800.0).abs() < 1e-9)
        }
        other => panic!("expected Range, got {other:?}"),
    }
}

#[test]
fn equivalency_guards_the_zero_sentinel() {
    // A 0.0 prediction (undefined) must not divide-by-zero into Inf/NaN; the
    // span is returned as a plain range instead.
    for (r, d) in [(0.0, 3600.0), (3600.0, 0.0), (0.0, 0.0)] {
        match race_equivalency(r, d).value {
            Equivalency::Range(lo, hi) => {
                assert!(lo.is_finite() && hi.is_finite(), "no Inf/NaN: {lo}, {hi}");
            }
            Equivalency::Agreed(m) => panic!("a sentinel input must range, got Agreed({m})"),
        }
    }
}

#[test]
fn equivalency_cites_the_equivalency_claim_not_vdot() {
    // running-039 is the Riegel/Daniels equivalency combiner (RUN-EQUIV-001),
    // NOT the VDOT-fitness estimate (RUN-VDOT-001). Guards the citation fix.
    let r = race_equivalency(3600.0, 3636.0);
    assert_eq!(r.claim_id(), claims::RUN_EQUIV_001,);
    assert_eq!(r.grade(), crate::schema::EvidenceGrade::Moderate);
}

// ── Segment-aware track fns (pause + relocation) ──────────────────

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
fn empty_starts_is_bit_identical_to_the_single_track_metrics() {
    // Every track fn's segment-aware form must reduce EXACTLY to the
    // whole-track form when there is no interior boundary, the regression net
    // for re-anchored logs and hand-logged runs. A start of 0 (index 0
    // never has an entering leg) and an out-of-range index are both no-ops.
    let t = reanchored_equivalent(); // one continuous run with slight structure
    for starts in [&[][..], &[0][..], &[999][..]] {
        assert_eq!(
            track_distance_km(&t, 30.0),
            track_distance_km_seg(&t, 30.0, starts),
            "distance parity, starts={starts:?}",
        );
        assert_eq!(
            track_positive_split_pct(&t, 30.0),
            track_positive_split_pct_seg(&t, 30.0, starts),
            "split parity, starts={starts:?}",
        );
        assert_eq!(
            track_splits(&t, 30.0, KM_M),
            track_splits_seg(&t, 30.0, KM_M, starts),
            "km-splits parity, starts={starts:?}",
        );
        assert_eq!(
            normalized_speed_mps(&t, 30.0),
            normalized_speed_mps_seg(&t, 30.0, starts),
            "NS parity, starts={starts:?}",
        );
        assert_eq!(
            track_variability_index(&t, 30.0),
            track_variability_index_seg(&t, 30.0, starts),
            "VI parity, starts={starts:?}",
        );
        assert_eq!(
            export_gpx(&t, "run"),
            export_gpx_seg(&t, "run", starts),
            "gpx parity, starts={starts:?}",
        );
    }
}

#[test]
fn segmented_distance_excludes_the_pause_bridge() {
    let t = relocation_fixture();
    // Without boundaries the bridge leg is still counted, now capped at the
    // plausible-runner ceiling (12 m/s × 60 s ≈ 0.72 km) instead of its raw
    // ~111 km, so it still swamps the two real segments.
    let naive = track_distance_km(&t, 30.0);
    let capped_bridge_km = MAX_PLAUSIBLE_SPEED_MPS * 60.0 / 1000.0;
    assert!(
        (naive - (0.889 + capped_bridge_km)).abs() < 0.05,
        "naive sum carries the capped bridge: {naive}"
    );
    // With the boundary at index 5 the bridge contributes nothing → only the
    // two ~445 m segments remain (~0.89 km).
    let seg = track_distance_km_seg(&t, 30.0, &[5]);
    assert!((seg - 0.889).abs() < 0.05, "segmented distance {seg}");
    // And it equals the distance of the re-anchored equivalent (bit-identical
    // route length, no shift needed).
    assert!(
        (seg - track_distance_km(&reanchored_equivalent(), 30.0)).abs() < 1e-9,
        "segmented == re-anchored distance",
    );
}

#[test]
fn standing_still_wander_is_not_distance_of_record() {
    // 100 m run leg (5 m/s), then a 3 m GPS wander over 60 s while standing
    // still (0.05 m/s, under the auto-pause floor). The saved distance must
    // be the run leg alone: counting the wander would make the recorded
    // distance disagree with the moving pace derived from the same track.
    let run_leg_deg = 100.0 / 111_320.0; // ~100 m of longitude at the equator
    let wander_deg = 3.0 / 111_320.0;
    let t = vec![
        pt(0.0, 0.0, 0),
        pt(0.0, run_leg_deg, 20),
        pt(0.0, run_leg_deg + wander_deg, 80),
    ];
    let d = track_distance_km(&t, 30.0);
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
        track_variability_index_seg(&t, 30.0, &[5]),
        track_variability_index(&re, 30.0),
        "VI parity vs re-anchored",
    );
    assert_eq!(
        track_positive_split_pct_seg(&t, 30.0, &[5]),
        track_positive_split_pct(&re, 30.0),
        "split parity vs re-anchored",
    );
    // Raw splits agree to f64 rounding (the equatorial re-anchor translation is
    // exact only up to the last ULP, so compare per field with a tolerance).
    let seg_splits = track_splits_seg(&t, 30.0, KM_M, &[5]);
    let re_splits = track_splits(&re, 30.0, KM_M);
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
    let gpx = export_gpx_seg(&t, "paused run", &[5]);
    // Two recording segments → two <trkseg> blocks with a gap between.
    assert_eq!(gpx.matches("<trkseg>").count(), 2, "two segments\n{gpx}");
    assert_eq!(gpx.matches("</trkseg>").count(), 2);
    // TRUE coordinates: segment 2's real relocated longitude (1.000…) is
    // present, NOT re-anchored back onto segment 1 (0.004…).
    assert!(gpx.contains("lon=\"1.0000000\""), "true seg-2 coord\n{gpx}");
    // Every real fix is still emitted (10 points across the two segments).
    assert_eq!(gpx.matches("<trkpt").count(), 10);
    // Empty starts → a single <trkseg>, byte-identical to the bare export.
    let flat = export_gpx_seg(&t, "paused run", &[]);
    assert_eq!(flat.matches("<trkseg>").count(), 1);
    assert_eq!(flat, export_gpx(&t, "paused run"));
}

#[test]
fn c25k_matches_the_nhs_bout_table() {
    let bouts = |w: u8, r: u8| -> Vec<(u16, u16)> {
        c25k_session_rx(w, r)
            .value
            .bouts
            .iter()
            .map(|b| (b.run_sec, b.walk_sec))
            .collect()
    };
    assert_eq!(bouts(1, 1), vec![(60, 90); 8]);
    assert_eq!(bouts(2, 2), vec![(90, 120); 6]);
    assert_eq!(
        bouts(3, 1),
        vec![(90, 90), (180, 180), (90, 90), (180, 180)]
    );
    assert_eq!(
        bouts(4, 3),
        vec![(180, 90), (300, 150), (180, 90), (300, 0)]
    );
    assert_eq!(bouts(5, 1), vec![(300, 180), (300, 180), (300, 0)]);
    assert_eq!(bouts(5, 2), vec![(480, 300), (480, 0)]);
    assert_eq!(bouts(5, 3), vec![(1200, 0)], "first 20-min run ends week 5");
    assert_eq!(bouts(6, 1), vec![(300, 180), (480, 180), (300, 0)]);
    assert_eq!(bouts(6, 2), vec![(600, 180), (600, 0)]);
    assert_eq!(bouts(6, 3), vec![(1500, 0)], "25 min straight ends week 6");
    assert_eq!(bouts(7, 2), vec![(1500, 0)]);
    assert_eq!(bouts(8, 1), vec![(1680, 0)]);
    assert_eq!(bouts(9, 3), vec![(1800, 0)]);
    assert_eq!(
        bouts(12, 1),
        vec![(1800, 0)],
        "a repeat past week 9 stays 30 min"
    );
    for w in 1..=9 {
        assert_eq!(c25k_session_rx(w, 1).value.warmup_walk_min, 5);
    }
    // Weeks 1-2 keep the uniform run/walk volume; continuous weeks are minutes.
    assert_eq!(
        c25k_session_rx(1, 1).value.volume(),
        RunVolume::RunWalk {
            run_sec: 60,
            walk_sec: 90,
            rounds: 8
        }
    );
    assert_eq!(
        c25k_session_rx(9, 1).value.volume(),
        RunVolume::DurationMin(30)
    );
    assert_eq!(c25k_session_rx(1, 1).claim_id(), claims::RUN_C25K_002);
}

#[test]
fn c25k_longest_bout_monotone_and_reaches_20min_by_week5() {
    // The week's hardest (last) run: its longest continuous bout never
    // shrinks week to week, and the 20-minute run arrives in week 5.
    let longest: Vec<u16> = (1..=9)
        .map(|w| c25k_session_rx(w, 3).value.longest_bout_sec())
        .collect();
    for w in 1..longest.len() {
        assert!(
            longest[w] >= longest[w - 1],
            "week {} regressed: {longest:?}",
            w + 1
        );
    }
    assert_eq!(longest[4], 1200);
    assert_eq!(longest[8], 1800);
}

#[test]
fn race_phases_run_base_build_peak_taper_in_order() {
    use MesoPhase::*;
    let ph = |g, w| race_phase(g, w).unwrap().value;
    assert_eq!(taper_weeks(GoalDistance::TenK), 2);
    assert_eq!(taper_weeks(GoalDistance::Marathon), 3);
    assert_eq!(taper_weeks(GoalDistance::General), 0);
    assert_eq!(ph(GoalDistance::TenK, 20), Base);
    assert_eq!(ph(GoalDistance::TenK, 10), Build);
    assert_eq!(ph(GoalDistance::TenK, 4), Peak);
    assert_eq!(ph(GoalDistance::TenK, 1), Taper);
    assert_eq!(ph(GoalDistance::TenK, 0), Taper);
    assert!(race_phase(GoalDistance::General, 3).is_none());
    // Bosquet: every taper week cuts 41-60 %, deepening toward the race.
    let c1 = taper_volume_cut(GoalDistance::TenK, 1, 2).unwrap().value;
    let c2 = taper_volume_cut(GoalDistance::TenK, 2, 2).unwrap().value;
    assert!((0.41..=0.60).contains(&c1) && (0.41..=0.60).contains(&c2));
    assert!(c2 > c1, "progressive taper: {c1} then {c2}");
}

#[test]
fn quality_mix_follows_the_goal_table() {
    use MesoPhase::*;
    use RunSessionType::*;
    let mix = |g, p| quality_mix(g, p).value;
    assert!(mix(GoalDistance::Marathon, Build).contains(&RacePace));
    assert!(mix(GoalDistance::FiveK, Peak).contains(&Repetition));
    assert_eq!(mix(GoalDistance::HalfMarathon, Build)[0], Tempo);
    assert_eq!(mix(GoalDistance::TenK, Base), &[Tempo]);
    assert!(mix(GoalDistance::TenK, Deload).is_empty());
    assert!(mix(GoalDistance::C25k, Build).is_empty());
}

#[test]
fn long_run_share_is_30pct_under_64km() {
    assert_eq!(long_run_share(40.0).value, 0.30);
    assert_eq!(long_run_share(64.0).value, 0.25);
    assert_eq!(
        goal_long_run_target_km(GoalDistance::Marathon)
            .unwrap()
            .value,
        (29.0, 32.0)
    );
}
