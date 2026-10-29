//! Saved GPS run figures must describe ONE set of legs: the headline distance,
//! the headline moving time and the per-unit splits. Found by the on-device
//! soak (`scripts/soak-emulator.sh`): every real tracked run begins with a
//! standing spell (GPS lock, tapping Start) and city runs stop at lights,
//! and in both places the three figures disagree.

use super::*;

/// Track of a runner who stands still for `stand_s` seconds after the first
/// fix (identical fixes, 1 Hz), then runs east at 3 m/s for `run_s` seconds,
/// with the first stride covering `first_stride_m` (≥ 2.5 m, so the core
/// keeps it). Returns the points and the end time.
fn standing_start(stand_s: i64, first_stride_m: f64, run_s: i64) -> (Vec<GpsPoint>, i64) {
    let mut pts: Vec<GpsPoint> = (0..stand_s).map(|t| fix(0.0, t)).collect();
    let mut x = first_stride_m;
    let mut t = stand_s;
    pts.push(fix(x, t));
    for _ in 1..run_s {
        t += 1;
        x += 3.0;
        pts.push(fix(x, t));
    }
    (pts, t)
}

fn saved_card(points: Vec<GpsPoint>) -> RunResultView {
    let app = Engine;
    let mut model = Model::default();
    let _ = app.update(log_run_track(points, 0.0, 0.0, 1, 1), &mut model);
    let vm = app.view(&model);
    assert_eq!(vm.runs.len(), 1);
    vm.runs[0].clone()
}

/// Seconds the split list accounts for: each split's moving seconds (full
/// splits carry the time itself, the partial one is normalized to a unit).
fn split_clock_s(splits: &[RunSplitView], unit_km: f64) -> f64 {
    let mut prev_km = 0.0;
    let mut sum = 0.0;
    for s in splits {
        let covered = s.distance_km - prev_km;
        sum += f64::from(s.seconds_per_unit) * covered / unit_km;
        prev_km = s.distance_km;
    }
    sum
}

#[test]
fn splits_account_for_exactly_the_headline_moving_time_after_a_standing_start() {
    // 9 s standing after the first fix, a 6 m first stride, then 3 m/s.
    // Headline moving time counts the stride from where the runner stood
    // (1 s); the splits must too, not time the whole 10 s leg from the first
    // fix (the first split would read slow and the splits add up to more
    // time than the run's moving time).
    let (pts, _) = standing_start(10, 6.0, 400);
    let card = saved_card(pts);
    let moving_s = card.duration_min * 60.0;
    for (splits, unit_km) in [(&card.splits_km, 1.0), (&card.splits_mi, 1.609_344)] {
        assert!(!splits.is_empty());
        let clock = split_clock_s(splits, unit_km);
        assert!(
            (clock - moving_s).abs() <= 2.0,
            "splits account for {clock:.1} s, headline moving time {moving_s:.1} s ({unit_km} km unit)"
        );
    }
}

#[test]
fn constant_pace_run_without_stops_has_matching_splits_and_pace() {
    // Control for the test above: with no standing spell the three figures
    // agree, so a failure above is the standing spell and not the fixture.
    let (pts, end) = standing_start(1, 3.0, 400);
    let card = saved_card(pts);
    let moving_s = card.duration_min * 60.0;
    assert!(
        (moving_s - end as f64).abs() <= 1.0,
        "moving {moving_s} s vs {end} s"
    );
    let clock = split_clock_s(&card.splits_km, 1.0);
    assert!(
        (clock - moving_s).abs() <= 2.0,
        "splits {clock:.1} s vs moving {moving_s:.1} s"
    );
    let speed = card.distance_km * 1000.0 / moving_s;
    assert!(
        (speed - 3.0).abs() < 0.01,
        "of-record speed {speed:.4} m/s for a 3 m/s runner"
    );
}

#[test]
fn distance_and_moving_time_cover_the_same_legs_across_traffic_light_stops() {
    // A runner at a constant 3 m/s who stops five times for 30 s (identical
    // fixes, as a phone at a light reports). Whatever the stop rule, distance
    // and moving time must describe the same legs, so the of-record pace of
    // a constant-speed runner is that speed.
    let mut pts = Vec::new();
    let (mut x, mut t) = (0.0, 0i64);
    for _ in 0..6 {
        for _ in 0..200 {
            pts.push(fix(x, t));
            x += 3.0;
            t += 1;
        }
        // Standing: the last running fix repeated for 30 s.
        x -= 3.0;
        for _ in 0..30 {
            pts.push(fix(x, t));
            t += 1;
        }
        x += 3.0;
    }
    let preview = preview_track_figures(pts.clone(), &[]);
    let card = saved_card(pts);
    // The Finish review's figures (the core's preview of the unsaved track)
    // are the saved run's, bit for bit.
    assert_eq!(preview, (card.distance_km, card.duration_min));
    let moving_s = card.duration_min * 60.0;
    let distance_m = card.distance_km * 1000.0;
    assert!(
        (distance_m - 3.0 * moving_s).abs() <= 1.0,
        "{distance_m:.1} m in {moving_s:.0} s moving = {:.4} m/s for a 3 m/s runner",
        distance_m / moving_s
    );
}

#[test]
fn a_runs_start_is_its_first_fix_or_its_logged_stamp_minus_its_duration() {
    let app = Engine;
    let mut model = Model::default();
    // A tracked run is logged at its LAST fix; it started at its first.
    let t0 = 1_790_000_000;
    let pts: Vec<GpsPoint> = (0..600).map(|s| fix(3.0 * s as f64, t0 + s)).collect();
    let end = t0 + 599;
    let _ = app.update(log_run_track(pts, 0.0, 0.0, end, 1), &mut model);
    // A hand-entered 30-minute run logged an hour later.
    let _ = app.update(log_run(5.0, 30.0, 0.0, 0.0, end + 3600, 2), &mut model);
    // An undated legacy run stays undated.
    let _ = app.update(log_run(5.0, 30.0, 0.0, 0.0, 0, 3), &mut model);
    let vm = app.view(&model);
    let by_id = |id: u64| {
        vm.runs
            .iter()
            .find(|r| r.entry_id == id)
            .expect("run listed")
    };
    assert_eq!((by_id(1).observed_at, by_id(1).started_at), (end, t0));
    assert_eq!(by_id(2).started_at, end + 3600 - 30 * 60);
    assert_eq!(by_id(3).started_at, 0);
}
