use super::*;
use crate::DAY_SEC as DAY;
use crate::app::tests::{base_profile, logged_run};
use crate::hybrid::ConcurrentGoal;
use crate::individualization::ProgressionCadence;
use crate::schema::WorkoutType;

/// Noon of epoch-day 704, a Monday, so a week anchored here is one clean
/// Mon–Sun block for the local-week bucketing. Noon, not 00:00: a run stamped
/// at its end counts in the week it STARTED, so a run "logged" at Monday 00:00
/// would be the previous Sunday's.
const MON: i64 = 704 * DAY + 12 * 3600;

/// Weekly-report row text the tests match on.
const OVER_TWICE_DAILY: &str = "more than twice your average daily distance";
const SHARE_CAP_FROM_64KM: &str = "The 25% limit applies from 64 km a week";
const OVER_SHARE_CAP: &str = "over the 25% limit";

fn run(km: f64, minutes: f64, hr_pct: f64, wt: Option<WorkoutType>, at: i64) -> LoggedRun {
    LoggedRun {
        workout_type: wt,
        ..logged_run(km, minutes, hr_pct, at)
    }
}

fn profile_with_run_days(days: u8, km: f64) -> Profile {
    Profile {
        progression_cadence: ProgressionCadence::EverySession,
        lift_goal: LiftGoal::Hypertrophy,
        concurrent_goal: ConcurrentGoal::Hypertrophy,
        weekly_sets: 10,
        running_days_per_week: days,
        running_km_per_week: km,
        ..base_profile()
    }
}

// The ≤2×-daily bound averages over RUN days, not calendar days, so the
// report and the plan agree. A 3-day/30 km runner's 15 km long run is 2× the
// 10 km run-day average (within bound); a calendar-day denominator (30/7 ≈
// 4.3) would flag it.
#[test]
fn weekly_report_uses_run_day_average() {
    let model = Model {
        profile_history: vec![profile_with_run_days(3, 30.0)],
        runs: vec![
            run(15.0, 90.0, 75.0, None, MON),
            run(10.0, 60.0, 75.0, None, MON + 2 * DAY),
            run(5.0, 30.0, 75.0, None, MON + 4 * DAY),
        ],
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    assert!(
        !report.iter().any(|r| r.summary.contains(OVER_TWICE_DAILY)),
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
        report
            .iter()
            .any(|r| r.summary.contains("Threshold running")
                && r.summary.contains("is over 10% of the week's distance")),
        "the tempo run must be charged as threshold volume: {report:#?}"
    );
    assert!(
        !report
            .iter()
            .any(|r| r.summary.contains("Interval running") && r.summary.contains("is over")),
        "a correctly executed tempo must not trip the interval cap: {report:#?}"
    );
}

// A long run over 25 % of the week does not hide the threshold overrun: 8 km
// at 90 %HRmax is 24 % of a 33 km week against the 10 % cap.
#[test]
fn weekly_report_flags_a_threshold_overrun_next_to_a_long_run_overrun() {
    let model = Model {
        runs: vec![
            run(15.0, 90.0, 75.0, None, MON),
            run(8.0, 40.0, 90.0, None, MON + DAY),
            run(5.0, 30.0, 75.0, None, MON + 2 * DAY),
            run(5.0, 30.0, 75.0, None, MON + 3 * DAY),
        ],
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    assert!(
        report
            .iter()
            .any(|r| r.summary.starts_with("Long run 15.0 km is 45% of the week")),
        "long-run row: {report:#?}"
    );
    assert!(
        report.iter().any(|r| r
            .summary
            .starts_with("Threshold running 8.0 km is over 10%")),
        "threshold row: {report:#?}"
    );
}

// A muscle whose direct MEV is 0 (indirect compound work suffices) gets no
// direct sets in the plan, and the planner says so; its OPTIONAL direct-set
// ramp starts at MAV low, never 0.
#[test]
fn mev_zero_muscle_starts_above_zero() {
    let q = HypertrophyInputView {
        muscle: "glutes".into(),
        weeks: 4,
        not_growing: false,
        recovering_easily: false,
    };
    let (rows, _, _) = build_hypertrophy_plan(&q, None, None);
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
    let q = HrZoneInputView {
        age_years: 30.0,
        resting_hr_bpm: None,
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
        profile_history: vec![profile_with_run_days(3, 20.0)],
        runs: vec![
            run(5.0, 30.0, 75.0, None, MON),
            run(5.0, 30.0, 75.0, None, MON + DAY),
            run(10.0, 60.0, 75.0, None, MON + 2 * DAY),
        ],
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    assert!(
        report
            .iter()
            .any(|r| r.summary.contains(SHARE_CAP_FROM_64KM)),
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
        !report.iter().any(|r| r.summary.contains(OVER_SHARE_CAP)),
        "it must not be flagged as a cap violation: {report:#?}"
    );
}

#[test]
fn hybrid_report_uses_the_capped_run_day_count() {
    let model = Model {
        profile_history: vec![profile_with_run_days(5, 30.0)],
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
        report
            .iter()
            .any(|r| r.summary.contains(SHARE_CAP_FROM_64KM)),
        "the report must average over the capped run-day count: {report:#?}"
    );
    assert!(
        !report.iter().any(|r| r.summary.contains(OVER_TWICE_DAILY)),
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
        !report.iter().any(|r| r.summary.contains(OVER_SHARE_CAP)),
        "the future 15 km long run must not create a cap violation: {report:#?}"
    );
}

/// The long-run rows of a weekly report (not the "within weekly limits"
/// summary row).
fn long_run_rows(report: &[GuidanceView]) -> Vec<&str> {
    report
        .iter()
        .map(|r| r.summary.as_str())
        .filter(|s| s.starts_with("Long run ") && !s.starts_with("Long run and"))
        .collect()
}

// Under 64 km a week a long run past twice the daily average
// is one row, without the 25% share cap (which applies from 64 km a week)
// and without a second and third row restating the share.
#[test]
fn low_mileage_long_run_over_2x_daily_average_is_one_row() {
    let model = Model {
        profile_history: vec![profile_with_run_days(3, 20.0)],
        runs: vec![
            run(14.0, 84.0, 75.0, None, MON),
            run(3.0, 18.0, 75.0, None, MON + DAY),
            run(3.0, 18.0, 75.0, None, MON + 2 * DAY),
        ],
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    let rows = long_run_rows(&report);
    assert_eq!(
        rows,
        [
            "Long run 14.0 km is 70% of the week and more than twice your average daily distance (6.7 km)."
        ],
        "{report:#?}"
    );
    let mi = report
        .iter()
        .find(|r| r.summary == rows[0])
        .map(|r| r.summary_mi.as_str());
    assert_eq!(
        mi,
        Some(
            "Long run 8.7 mi is 70% of the week and more than twice your average daily distance (4.2 mi)."
        )
    );
}

// At 64 km a week and up the 25% share cap applies, stated once.
#[test]
fn high_mileage_long_run_over_the_share_cap_is_one_row() {
    let model = Model {
        profile_history: vec![profile_with_run_days(3, 70.0)],
        runs: vec![
            run(30.0, 180.0, 75.0, None, MON),
            run(20.0, 120.0, 75.0, None, MON + DAY),
            run(20.0, 120.0, 75.0, None, MON + 2 * DAY),
        ],
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    assert_eq!(
        long_run_rows(&report),
        ["Long run 30.0 km is 43% of the week: over the 25% limit for one run."],
        "{report:#?}"
    );
}
