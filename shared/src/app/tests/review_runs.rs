//! Adversarial review proofs for the 2026-10-23 run fixes (standing-fix
//! reinsertion in `qc_track`, splits vs moving time, Finish preview vs saved
//! card, `started_at`, GPX export). `#[ignore = "REVIEW: …"]` tests FAIL on
//! the current tree and prove a finding; the others verify a fix claim.

use super::*;

/// Metres per degree on the equator for the core's haversine (R = 6 371 km).
const M_PER_DEG: f64 = 6_371_000.0 * std::f64::consts::PI / 180.0;

/// A fix `x_m` east and `y_m` north of (0, 0) at second `t`, 5 m accuracy.
fn fix_xy(x_m: f64, y_m: f64, t: i64) -> GpsPoint {
    GpsPoint {
        lat: y_m / M_PER_DEG,
        lon: x_m / M_PER_DEG,
        observed_at: t,
        accuracy_m: 5.0,
    }
}

/// Deterministic xorshift64* in [0, 1).
struct Rng(u64);
impl Rng {
    fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: u64) -> u64 {
        (self.next_f64() * n as f64) as u64
    }
}

fn saved_card(points: Vec<GpsPoint>, starts: Vec<u32>) -> RunResultView {
    let app = Engine;
    let mut model = Model::default();
    let end = points.last().map_or(1, |p| p.observed_at.max(1));
    let _ = app.update(
        log_run_track_seg(points, 0.0, 0.0, end, 1, starts),
        &mut model,
    );
    let vm = app.view(&model);
    assert_eq!(vm.runs.len(), 1);
    vm.runs[0].clone()
}

/// 200 s east at 3 m/s, a `stand_s` standing spell whose fixes are drawn by
/// `jitter` around the stop point, then 200 s more east at 3 m/s. The true
/// moving distance is 1 200 m and the true moving time 400 s.
fn run_stand_run(stand_s: i64, mut jitter: impl FnMut(i64) -> (f64, f64)) -> Vec<GpsPoint> {
    let mut pts = Vec::new();
    let mut t = 0i64;
    let mut x = 0.0;
    for _ in 0..=200 {
        pts.push(fix_xy(x, 0.0, t));
        x += 3.0;
        t += 1;
    }
    x -= 3.0;
    for k in 0..stand_s {
        let (dx, dy) = jitter(k);
        pts.push(fix_xy(x + dx, dy, t));
        t += 1;
    }
    for _ in 0..200 {
        x += 3.0;
        pts.push(fix_xy(x, 0.0, t));
        t += 1;
    }
    pts
}

/// Uncorrelated (white) horizontal jitter, uniform in a disc of `r` metres:
/// what a phone reports at 1 Hz standing under poor sky view.
fn white_disc(seed: u64, r: f64) -> impl FnMut(i64) -> (f64, f64) {
    let mut rng = Rng(seed);
    move |_| {
        let a = rng.next_f64() * std::f64::consts::TAU;
        let d = r * rng.next_f64().sqrt();
        (d * a.cos(), d * a.sin())
    }
}

#[test]
fn standing_white_jitter_is_not_distance_or_moving_time() {
    // Five minutes at a light with ±3 m white jitter (well inside the 30 m
    // accuracy gate). KB File 07 / BUGS 2026-09-11 #4: GPS wander while
    // standing still is not distance of record, and standing time never
    // dilutes pace. Allow 15 m / 10 s of slack for the resume stride.
    let mut worst = (0.0f64, 0.0f64);
    for seed in 1..=20u64 {
        let card = saved_card(run_stand_run(300, white_disc(seed * 7919, 3.0)), Vec::new());
        let extra_m = card.distance_km * 1000.0 - 1200.0;
        let extra_s = card.duration_min * 60.0 - 400.0;
        worst = (worst.0.max(extra_m), worst.1.max(extra_s));
    }
    assert!(
        worst.0 <= 15.0 && worst.1 <= 10.0,
        "a 300 s standing spell with 3 m jitter added up to {:.1} m and {:.0} s moving",
        worst.0,
        worst.1
    );
}

#[test]
fn one_outlier_fix_while_standing_adds_no_distance() {
    // A runner stands 120 s (identical fixes, as the existing tests model a
    // stop), with ONE 20 m outlier fix in the middle. The outlier passes the
    // 12 m/s gate measured from the stop's first fix (20 m over ~60 s), so it
    // survives; the reinserted standing fix right before it then forms a 1 s
    // leg of ~20 m, capped at 12 m/s and counted as moving.
    let card = saved_card(
        run_stand_run(120, |k| if k == 60 { (0.0, 20.0) } else { (0.0, 0.0) }),
        Vec::new(),
    );
    let extra_m = card.distance_km * 1000.0 - 1200.0;
    let extra_s = card.duration_min * 60.0 - 400.0;
    assert!(
        extra_m <= 25.0,
        "one 20 m outlier while standing added {extra_m:.1} m and {extra_s:.0} s moving"
    );
}

#[test]
fn standing_drift_is_not_distance() {
    // A runner stands 300 s while the fix drifts (correlated random walk,
    // 0.8 m steps, pulled back to the stop point, plus 0.5 m white noise).
    // HEAD (4169b19): mean +3.1 m distance, +18.6 s moving over 20 seeds; now
    // +18.9 m and +18.6 s. The moving-time leak predates the change; the fix
    // made distance follow it instead of dropping both.
    let mut extra_m = 0.0;
    for seed in 1..=20u64 {
        let mut rng = Rng(seed * 104_729);
        let (mut dx, mut dy) = (0.0f64, 0.0f64);
        let card = saved_card(
            run_stand_run(300, |_| {
                dx = (dx + (rng.next_f64() * 2.0 - 1.0) * 0.8) * 0.98;
                dy = (dy + (rng.next_f64() * 2.0 - 1.0) * 0.8) * 0.98;
                (dx + (rng.next_f64() - 0.5), dy + (rng.next_f64() - 0.5))
            }),
            Vec::new(),
        );
        extra_m += card.distance_km * 1000.0 - 1200.0;
    }
    let mean = extra_m / 20.0;
    assert!(
        mean <= 5.0,
        "300 s of standing drift added {mean:.1} m of distance on average"
    );
}

#[test]
fn a_stop_whose_fixes_settle_off_the_spot_still_counts_the_stride_out() {
    // 10 s on the spot, then the fix settles 3 m north (past the 2.5 m jitter
    // gate, inside the stop's GPS uncertainty) for 50 s, then the runner sets
    // off east. The stop ends where the runner set off: every metre and second
    // of the stride out is distance and moving time of record.
    let card = saved_card(
        run_stand_run(60, |k| if k < 10 { (0.0, 0.0) } else { (0.0, 3.0) }),
        Vec::new(),
    );
    let extra_m = card.distance_km * 1000.0 - 1200.0;
    let extra_s = card.duration_min * 60.0 - 400.0;
    assert!(
        extra_m.abs() <= 2.0 && extra_s.abs() < 0.5,
        "a settled stop changed the run by {extra_m:.1} m and {extra_s:.0} s moving"
    );
}

#[test]
fn preview_of_malformed_tracks_is_finite_and_never_panics() {
    // The trackFigures JNI input path: wire JSON → Event → preview. Malformed
    // wire JSON is a serde error (the ffi returns its error envelope); a
    // well-formed but corrupt track must give finite figures (a NaN would
    // serialize as `null` and fail the shell's TrackFigures decode).
    for bad in [
        &br#"{"LogRunTrack":{"points":[{"lat":1e400,"lon":0,"observed_at":1,"accuracy_m":5}],"hr_pct_max":0,"longest_recent_km":0,"observed_at":1,"entry_id":1}}"#[..],
        br#"{"LogRunTrack":{"points":[{"lat":0,"lon":0,"observed_at":1.5,"accuracy_m":5}],"hr_pct_max":0,"longest_recent_km":0,"observed_at":1,"entry_id":1}}"#,
        br#"{"LogRunTrack":{"points":"x"}}"#,
        b"\xff\xfe",
        b"",
    ] {
        assert!(serde_json::from_slice::<Event>(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
    }
    let p = |lat: f64, lon: f64, t: i64, acc: f32| GpsPoint {
        lat,
        lon,
        observed_at: t,
        accuracy_m: acc,
    };
    let cases: Vec<(Vec<GpsPoint>, Vec<u32>)> = vec![
        (vec![], vec![]),
        (vec![p(0.0, 0.0, 1, 5.0)], vec![0, 1, u32::MAX]),
        (
            vec![
                p(f64::NAN, f64::INFINITY, i64::MIN, f32::NAN),
                p(1e300, -1e300, i64::MAX, f32::INFINITY),
            ],
            vec![1],
        ),
        (
            vec![
                p(0.0, 0.0, i64::MIN, 5.0),
                p(0.0, 1e-3, 0, 5.0),
                p(0.0, 2e-3, i64::MAX, 5.0),
            ],
            vec![],
        ),
        (
            vec![
                p(91.0, 181.0, 1, -1.0),
                p(-91.0, -181.0, 2, 0.0),
                p(91.0, 181.0, 3, 1e-30),
            ],
            vec![2, 2, 2],
        ),
        (
            (0..5000).map(|t| p(0.0, t as f64 * 1e-4, t, 5.0)).collect(),
            (1..5000).collect(),
        ),
    ];
    for (pts, starts) in cases {
        let (km, min) = preview_track_figures(pts.clone(), &starts);
        assert!(
            km.is_finite() && min.is_finite() && km >= 0.0 && min >= 0.0,
            "{km} {min} for {} fixes",
            pts.len()
        );
        assert!(
            km.is_sign_positive() && min.is_sign_positive(),
            "-0.0 figure: {km} {min}"
        );
    }
}

#[test]
fn negative_zero_manual_run_never_reads_minus_zero() {
    // The -0.0 fix covered GPS sums; a hand-entered run's -0.0 wire floats go
    // through `sanitize_f64(..).max(0.0)`. The whole ViewModel JSON must not
    // carry a "-0.0" for it.
    let app = Engine;
    let mut model = Model::default();
    let _ = app.update(
        log_run(-0.0, -0.0, -0.0, -0.0, 1_790_000_000, 1),
        &mut model,
    );
    let _ = app.update(log_run(5.0, -0.0, -0.0, -0.0, 1_790_000_100, 2), &mut model);
    let vm = app.view(&model);
    let json = serde_json::to_string(&vm.runs).unwrap();
    assert!(!json.contains("-0.0") && !json.contains(":-0,"), "{json}");
    for r in &vm.runs {
        assert!(!r.summary.contains("-0"), "{}", r.summary);
    }
}

/// A random track: legs at random speeds incl. stops (identical fixes and
/// sub-floor drift), white jitter, the occasional bad-accuracy fix, repeated
/// and backwards stamps, multi-second gaps and pause boundaries.
fn random_track(rng: &mut Rng) -> (Vec<GpsPoint>, Vec<u32>) {
    let n = 20 + rng.below(600) as usize;
    let (mut x, mut y, mut t) = (0.0f64, 0.0f64, 1_790_000_000i64);
    let mut pts = Vec::with_capacity(n);
    let mut starts = Vec::new();
    let mut speed = 3.0;
    for i in 0..n {
        if rng.below(40) == 0 {
            speed = match rng.below(5) {
                0 => 0.0,
                1 => 0.3,
                2 => 1.5,
                3 => 6.0,
                _ => 3.0 + rng.next_f64() * 2.0,
            };
        }
        let dt = match rng.below(30) {
            0 => 0,
            1 => -1,
            2 => 5 + rng.below(60) as i64,
            _ => 1,
        };
        t += dt;
        let heading = rng.next_f64() * 0.6;
        x += speed * dt.max(0) as f64 * heading.cos();
        y += speed * dt.max(0) as f64 * heading.sin();
        let (jx, jy) = if rng.below(3) == 0 {
            (rng.next_f64() * 4.0 - 2.0, rng.next_f64() * 4.0 - 2.0)
        } else {
            (0.0, 0.0)
        };
        let mut p = fix_xy(x + jx, y + jy, t);
        if rng.below(25) == 0 {
            p.accuracy_m = 45.0;
        }
        if rng.below(15) == 0 {
            // A jitter spike far off the route.
            p.lat += 80.0 / M_PER_DEG;
        }
        if i > 0 && rng.below(60) == 0 {
            starts.push(i as u32);
        }
        pts.push(p);
    }
    (pts, starts)
}

#[test]
fn splits_sum_to_moving_time_and_distance_on_random_tracks() {
    // Verifies the FixRunMetrics claim "distance, moving time, splits and the
    // other figures now all use the same legs" as a property over 2 000 random
    // tracks, on the exact f64 figures (not the whole-second view).
    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    let mut worst_s = 0.0f64;
    let mut worst_m = 0.0f64;
    for _ in 0..2000 {
        let (pts, starts) = random_track(&mut rng);
        let qc = qc_track(&crate::app::ingest::sanitize_track(pts), &starts);
        let moving_s = moving_duration_min(&qc) * 60.0;
        let dist_m =
            running::track_distance_km_seg(&qc.track, running::MAX_GPS_ACCURACY_M, &qc.seg_starts)
                * 1000.0;
        for unit in [running::KM_M, running::MILE_M] {
            let splits = running::track_splits_seg(
                &qc.track,
                running::MAX_GPS_ACCURACY_M,
                unit,
                &qc.seg_starts,
            );
            let clock: f64 = splits
                .iter()
                .map(|s| {
                    if s.partial {
                        s.pace_sec_per_unit * s.distance_m / unit
                    } else {
                        s.pace_sec_per_unit
                    }
                })
                .sum();
            let covered: f64 = splits.iter().map(|s| s.distance_m).sum();
            if dist_m > 0.0 {
                worst_s = worst_s.max((clock - moving_s).abs());
                worst_m = worst_m.max((covered - dist_m).abs());
            }
        }
    }
    assert!(
        worst_s < 1e-6 && worst_m < 1e-6,
        "splits vs moving time differ by up to {worst_s:e} s, splits vs distance by {worst_m:e} m"
    );
}

#[test]
fn finish_preview_equals_saved_card_on_random_tracks() {
    // Verifies "the preview equals the saved card" beyond the fixed fixtures:
    // bit-equal on 500 random tracks with pauses, jitter, bad fixes and
    // non-advancing stamps.
    let mut rng = Rng(0xF1F1_2222_3333_4444);
    for i in 0..500 {
        let (pts, starts) = random_track(&mut rng);
        let preview = preview_track_figures(pts.clone(), &starts);
        let card = saved_card(pts, starts);
        assert_eq!(
            (preview.0.to_bits(), preview.1.to_bits()),
            (card.distance_km.to_bits(), card.duration_min.to_bits()),
            "track {i}: preview {preview:?} vs card ({}, {})",
            card.distance_km,
            card.duration_min
        );
    }
}

#[test]
fn qc_is_idempotent_so_undo_of_a_route_restores_its_figures() {
    // Undo of a deleted GPS run re-logs the EXPORTED (QC'd) route, not the raw
    // fixes (RunDetailSheet → Core.exportGpx → restoreEventFor). With the
    // standing-fix reinsertion the QC must still be idempotent or Undo moves
    // distance / moving time.
    let mut rng = Rng(0x0DD0_1DE0_9999_0001);
    for i in 0..2000 {
        let (pts, starts) = random_track(&mut rng);
        let once = qc_track(&crate::app::ingest::sanitize_track(pts), &starts);
        let twice = qc_track(&once.track, &once.seg_starts);
        assert_eq!(
            once.track, twice.track,
            "track {i}: QC of the QC'd route changed it"
        );
        assert_eq!(
            once.seg_starts, twice.seg_starts,
            "track {i}: segment starts moved"
        );
    }
}

/// Every f32 accuracy in [1, 30] m as the export → shell import → core path
/// turns it: the exported `<hdop>` (`running::gpx_hdop`, written with `{}`),
/// read as a Double, × 5 (Gpx.kt `hdopToAccuracyM`), deserialized by the core
/// as f32.
fn hdop_round_trips() -> impl Iterator<Item = (f32, f32)> {
    let mut a = 1.0f32;
    std::iter::from_fn(move || {
        if a > running::MAX_GPS_ACCURACY_M {
            return None;
        }
        let out = (a, hdop_back(a));
        a = f32::from_bits(a.to_bits() + 1);
        Some(out)
    })
}

fn hdop_back(a: f32) -> f32 {
    let hdop: f64 = format!("{}", running::gpx_hdop(a)).parse().unwrap();
    (hdop * 5.0) as f32
}

#[test]
fn exported_hdop_never_moves_a_fix_across_the_qc_accuracy_gate() {
    // What the QC can see of accuracy: only the 30 m gate. No accuracy at or
    // under it comes back over it, and none just over it (an unmeasurable
    // run's raw fixes, exported for Undo) comes back under it.
    let crossing: Vec<(f32, f32)> = hdop_round_trips()
        .filter(|&(_, back)| back > running::MAX_GPS_ACCURACY_M)
        .take(3)
        .collect();
    assert!(crossing.is_empty(), "{crossing:?}");
    let mut a = f32::from_bits(running::MAX_GPS_ACCURACY_M.to_bits() + 1);
    while a <= 31.0 {
        let back = hdop_back(a);
        assert!(
            back > running::MAX_GPS_ACCURACY_M,
            "{a} came back as {back}"
        );
        a = f32::from_bits(a.to_bits() + 1);
    }
}

#[test]
fn exported_hdop_round_trips_accuracy_bit_exact() {
    // FixRunMetrics: "a route rebuilt from the GPX is bit-identical".
    let changed: Vec<(f32, f32)> = hdop_round_trips().filter(|&(a, back)| back != a).collect();
    assert!(
        changed.is_empty(),
        "{} accuracies in [1, 30] m do not round-trip, e.g. {:?}",
        changed.len(),
        &changed[..changed.len().min(3)]
    );
}

#[test]
fn exported_coordinates_round_trip_bit_exact() {
    // Verifies "export_gpx_seg writes lat/lon with `{}`, the shortest decimal
    // that reads back exactly", and "never exponent notation".
    let mut rng = Rng(42);
    for _ in 0..20_000 {
        let lat = (rng.next_f64() * 180.0 - 90.0) * if rng.below(10) == 0 { 1e-9 } else { 1.0 };
        let lon = rng.next_f64() * 360.0 - 180.0;
        let doc = running::export_gpx(
            &[
                GpsPoint {
                    lat,
                    lon,
                    observed_at: 1,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat,
                    lon,
                    observed_at: 2,
                    accuracy_m: 5.0,
                },
            ],
            "r",
        );
        let lat_s = doc
            .split("lat=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let lon_s = doc
            .split("lon=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        assert!(
            !lat_s.contains(['e', 'E']) && !lon_s.contains(['e', 'E']),
            "{lat_s} {lon_s}"
        );
        assert_eq!(lat_s.parse::<f64>().unwrap().to_bits(), lat.to_bits());
        assert_eq!(lon_s.parse::<f64>().unwrap().to_bits(), lon.to_bits());
    }
}

/// Writes one GPX per run kind (plain, paused, standing finish, trimmed,
/// undated legacy stamp, unmeasurable) to `$REVIEW_GPX_DIR` for xmllint
/// validation against the GPX 1.1 XSD. No-op without the variable.
#[test]
fn dump_exported_gpx_samples() {
    let Ok(dir) = std::env::var("REVIEW_GPX_DIR") else {
        return;
    };
    let line = |n: i64, t0: i64| -> Vec<GpsPoint> {
        (0..n)
            .map(|s| fix_xy(3.0 * s as f64, 0.0, t0 + s))
            .collect()
    };
    type Kind<'a> = (&'a str, Vec<GpsPoint>, Vec<u32>, i64, Option<(i64, i64)>);
    let mut kinds: Vec<Kind> = Vec::new();
    let t0 = 1_790_000_000;
    kinds.push(("plain", line(300, t0), vec![], t0 + 299, None));
    let mut paused = line(150, t0);
    paused.extend(line(150, t0 + 400).into_iter().map(|mut p| {
        p.lat += 50.0 / M_PER_DEG;
        p
    }));
    kinds.push(("paused", paused, vec![150], t0 + 549, None));
    let mut standing_end = line(300, t0);
    let last = *standing_end.last().unwrap();
    standing_end.extend((1..120).map(|k| GpsPoint {
        observed_at: last.observed_at + k,
        ..last
    }));
    kinds.push((
        "standing_end",
        standing_end,
        vec![],
        last.observed_at + 119,
        None,
    ));
    kinds.push((
        "trimmed",
        line(300, t0),
        vec![],
        t0 + 299,
        Some((t0 + 30, t0 + 250)),
    ));
    kinds.push(("legacy_zero_stamp", line(300, t0), vec![], 0, None));
    let bad: Vec<GpsPoint> = line(50, t0)
        .into_iter()
        .map(|p| GpsPoint {
            accuracy_m: 80.0,
            ..p
        })
        .collect();
    kinds.push(("unmeasurable", bad, vec![], t0 + 49, None));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, pts, starts, at, trim_to) in kinds {
        let app = Engine;
        let mut model = Model::default();
        let _ = app.update(log_run_track_seg(pts, 0.0, 0.0, at, 7, starts), &mut model);
        if let Some((from, until)) = trim_to {
            let _ = app.update(trim(7, at, from, until), &mut model);
        }
        let vm = app.view(&model);
        let r = &vm.runs[0];
        let doc = crate::app::export_run_gpx(r.entry_id, r.observed_at);
        std::fs::write(format!("{dir}/{name}.gpx"), &doc).unwrap();
        eprintln!(
            "{name}: has_gpx={} unmeasurable={} observed_at={} started_at={} bytes={}",
            r.has_gpx,
            r.gps_unmeasurable,
            r.observed_at,
            r.started_at,
            doc.len()
        );
    }
}
