use super::*;
use crate::schema::WorkoutType;

const DAY: i64 = 86_400;
/// Epoch-day 704 is a Monday, so a week anchored here is one clean Mon–Sun
/// block for the local-week bucketing.
const MON: i64 = 704 * DAY;

fn run(km: f64, minutes: f64, hr_pct: f64, wt: Option<WorkoutType>, at: i64) -> LoggedRun {
    LoggedRun {
        distance_km: km,
        duration_min: minutes,
        hr_pct_max: hr_pct,
        longest_recent_km: 0.0,
        track: Vec::new(),
        track_segment_starts: Vec::new(),
        observed_at: at,
        entry_id: 0,
        workout_type: wt,
        session_rpe: None,
        avg_hr_bpm: None,
    }
}

fn lift(exercise: &str, reps: u32, rpe: f64, at: i64) -> LoggedSet {
    LoggedSet {
        exercise: exercise.to_string(),
        weight_kg: 100.0,
        reps,
        rpe,
        observed_at: at,
        entry_id: 0,
        hold_sec: None,
    }
}

fn profile_with_run_days(days: u8, km: f64) -> Profile {
    serde_json::from_str(&format!(
        r#"{{"progression_cadence":"EverySession","lift_goal":"Hypertrophy",
            "goal_distance":"General","concurrent_goal":"Hypertrophy","weekly_sets":10,
            "running_days_per_week":{days},"running_km_per_week":{km},"advanced":false,
            "endurance_intensity_pct_vo2max":75.0}}"#
    ))
    .expect("profile fixture parses")
}

// The ≤2×-daily bound averages over RUN days, not calendar days, so the
// report and the plan agree. A 3-day/30 km runner's 15 km long run is 2× the
// 10 km run-day average (within bound); a calendar-day denominator (30/7 ≈
// 4.3) would flag it.
#[test]
fn weekly_report_uses_run_day_average() {
    let model = Model {
        profile: Some(profile_with_run_days(3, 30.0)),
        runs: vec![
            run(15.0, 90.0, 75.0, None, MON),
            run(10.0, 60.0, 75.0, None, MON + 2 * DAY),
            run(5.0, 30.0, 75.0, None, MON + 4 * DAY),
        ],
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    assert!(
        !report
            .iter()
            .any(|r| r.summary.contains("exceeds 2× your average daily distance")),
        "run-day average must not flag a 15 km long run for a 3-day/30 km runner: {report:#?}"
    );
}

// A tempo run is threshold volume, never interval volume, even though its
// average %HRmax lands in the Lucía Z3 bucket. Charging it as interval would
// raise a false ≤8%-interval violation; it must raise the ≤10%-threshold one.
#[test]
fn tempo_counts_as_threshold_not_interval() {
    // 40 km week: 6 km tempo (≈15% → over the 10% threshold cap) + four 8.5 km
    // easy runs (long run 8.5 km = 21% ≤ 25%, so the long-run cap is clear).
    let model = Model {
        runs: vec![
            run(6.0, 30.0, 90.0, Some(WorkoutType::Tempo), MON),
            run(8.5, 51.0, 75.0, None, MON + DAY),
            run(8.5, 51.0, 75.0, None, MON + 2 * DAY),
            run(8.5, 51.0, 75.0, None, MON + 3 * DAY),
            run(8.5, 51.0, 75.0, None, MON + 4 * DAY),
        ],
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    assert!(
        report.iter().any(|r| r.summary.contains("Threshold volume")
            && r.summary.contains("exceeds the ≤10% weekly cap")),
        "the tempo run must be charged as threshold volume: {report:#?}"
    );
    assert!(
        !report
            .iter()
            .any(|r| r.summary.contains("Interval volume") && r.summary.contains("exceeds")),
        "a correctly executed tempo must not trip the interval cap: {report:#?}"
    );
}

// The heavy-leg ↔ long-run guard fires only on squat/hinge work at ≥80%
// 1RM. Light calf work (not a squat/hinge pattern) next to a short run is not
// a heavy-leg/long-run collision and must raise no spacing flag.
#[test]
fn light_leg_work_and_a_short_run_raise_no_gap_flag() {
    let model = Model {
        sets: vec![
            lift("Calf raise", 15, 8.0, MON),
            lift("Calf raise", 15, 8.0, MON),
            lift("Calf raise", 15, 8.0, MON),
        ],
        runs: vec![run(3.0, 18.0, 70.0, None, MON + 10 * 3600)],
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    assert!(
        !report
            .iter()
            .any(|r| r.summary.contains("keep ≥24 h between")),
        "light calf work + a short run must not raise a heavy-leg/run spacing flag: {report:#?}"
    );
}

// A muscle whose direct MEV is 0 (indirect compound work suffices) gets no
// direct sets in the plan, and the planner says so; its OPTIONAL direct-set
// ramp starts at MAV low, never 0.
#[test]
fn mev_zero_muscle_starts_above_zero() {
    let q = HypertrophyPlanQuery {
        muscle: "glutes".into(),
        weeks: 4,
        not_growing: false,
        recovering_easily: false,
    };
    let (rows, _) = build_hypertrophy_plan(&q, None, None);
    let ramp = rows
        .iter()
        .find(|r| r.summary.starts_with("No direct sets:"))
        .expect("ramp row present");
    assert!(
        ramp.summary
            .contains("compound lifts already train this muscle"),
        "glutes row must say the plan gives no direct sets: {}",
        ramp.summary
    );
    // glutes MAV is 4–12 → [4, 6, 9, 12]: week 1 is 4, not 0, and the
    // ramp ends at MAV high, not MRV 16 (HYP-VOL-CAP-001).
    assert!(
        ramp.summary
            .contains("Optional direct work: 4 → 6 → 9 → 12"),
        "glutes optional week 1 must be a non-zero set count: {}",
        ramp.summary
    );
}

// The Repetition band is pace-governed with an open-ended VO2max ceiling.
// Its row must read as a floor (no literal "inf") and drop the bpm figure that
// HR cannot validly anchor on short reps.
#[test]
fn repetition_hr_row_reads_cleanly() {
    let q = HrZoneQuery {
        age_years: 30.0,
        resting_hr_bpm: None,
        weeks_since_recalc: None,
        weeks_since_pace_test: None,
    };
    let (rows, _, _) = build_hr_zones(&q, None);
    let rep = rows
        .iter()
        .find(|r| r.summary.starts_with("Repetition"))
        .expect("repetition row present");
    assert!(
        !rep.summary.contains("inf"),
        "no literal inf: {}",
        rep.summary
    );
    assert!(
        !rep.summary.contains("bpm"),
        "no bpm figure on a pace-governed band: {}",
        rep.summary
    );
    assert!(
        rep.why.basis.contains("at least 100% of VO2max"),
        "open VO2max ceiling renders as a floor in the basis: {}",
        rep.why.basis
    );
    assert!(
        !rep.summary.contains("100–100"),
        "equal %HRmax bounds collapse to one figure: {}",
        rep.summary
    );
    assert!(
        rep.summary.contains("go by pace, not heart rate"),
        "repetition is pace-governed: {}",
        rep.summary
    );
}

#[test]
fn low_mileage_long_run_within_2x_daily_average_is_informational() {
    let model = Model {
        profile: Some(profile_with_run_days(3, 20.0)),
        runs: vec![
            run(5.0, 30.0, 75.0, None, MON),
            run(5.0, 30.0, 75.0, None, MON + DAY),
            run(10.0, 60.0, 75.0, None, MON + 2 * DAY),
        ],
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    assert!(
        report.iter().any(|r| r
            .summary
            .contains("The 25% limit applies from 64 km a week")),
        "a sub-64 km long run within 2× daily average is informational: {report:#?}"
    );
    assert!(
        report
            .iter()
            .any(|r| r.summary_mi.contains("Long run 6.2 mi")
                && r.summary_mi
                    .contains("The 25% limit applies from 40 mi a week")),
        "the informational row carries a miles twin: {report:#?}"
    );
    assert!(
        !report
            .iter()
            .any(|r| r.summary.contains("over the ≤25% single-run cap")),
        "it must not be flagged as a cap violation: {report:#?}"
    );
}

#[test]
fn hybrid_report_uses_the_capped_run_day_count() {
    let model = Model {
        profile: Some(profile_with_run_days(5, 30.0)),
        runs: vec![
            run(13.0, 78.0, 75.0, None, MON),
            run(5.0, 30.0, 75.0, None, MON + DAY),
            run(4.0, 24.0, 75.0, None, MON + 2 * DAY),
            run(4.0, 24.0, 75.0, None, MON + 3 * DAY),
            run(4.0, 24.0, 75.0, None, MON + 4 * DAY),
        ],
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    // 30 km / 5 stated days = 6 km/day → 2× = 12 km would flag the 13 km long
    // run; capped to the 3-day hybrid concurrency count (10 km/day, 2× = 20
    // km) it does not, reading as the sub-64 km informational note instead.
    assert!(
        report.iter().any(|r| r
            .summary
            .contains("The 25% limit applies from 64 km a week")),
        "the report must average over the capped run-day count: {report:#?}"
    );
    assert!(
        !report
            .iter()
            .any(|r| r.summary.contains("exceeds 2× your average daily distance")),
        "the capped denominator must not flag the long run: {report:#?}"
    );
}

#[test]
fn run_dated_tomorrow_does_not_count_in_this_week() {
    let model = Model {
        today_epoch_day: Some(707),
        runs: vec![
            run(5.0, 30.0, 75.0, None, MON),
            run(5.0, 30.0, 75.0, None, MON + DAY),
            run(5.0, 30.0, 75.0, None, MON + 2 * DAY),
            run(5.0, 30.0, 75.0, None, MON + 3 * DAY),
            run(15.0, 90.0, 75.0, None, MON + 4 * DAY),
        ],
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    assert!(
        report.iter().any(|r| r.summary.contains("20.0 km")),
        "only the four runs through today count (20 km), not the future run: {report:#?}"
    );
    assert!(
        !report
            .iter()
            .any(|r| r.summary.contains("over the ≤25% single-run cap")),
        "the future 15 km long run must not create a cap violation: {report:#?}"
    );
}
