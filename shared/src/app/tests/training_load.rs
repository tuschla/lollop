//! Training load (CTL/ATL/TSB) and the weekly report's volume system, ramp
//! guard and heavy-leg / hard-run spacing.

use super::*;

#[test]
fn training_load_chains_lucia_trimp_into_ctl_atl_tsb() {
    let app = Engine;
    let mut model = Model::default();
    assert!(
        app.view(&model).training_load.is_none(),
        "no runs → no load view"
    );

    let t0 = 100 * WEEK;
    for (i, hr) in [(0i64, 70.0), (2, 75.0), (4, 90.0)] {
        app.update(log_run_at(10.0, 60.0, hr, t0 + i * DAY_SEC), &mut model)
            .expect_only_render();
    }
    // An undated run and an HR-less run are skipped, never fabricated.
    app.update(log_run_at(8.0, 45.0, 75.0, 0), &mut model)
        .expect_only_render();
    app.update(log_run_at(8.0, 45.0, 0.0, t0 + 5 * DAY_SEC), &mut model)
        .expect_only_render();

    let tl = app.view(&model).training_load.expect("load view present");
    assert_eq!(tl.sessions_counted, 3);
    assert_eq!(tl.sessions_skipped, 2);
    assert_eq!(tl.days, 5, "day span 0..=4");
    assert!(tl.ctl > 0.0 && tl.atl > 0.0);
    // Fresh loading: acute fatigue outruns chronic fitness → negative form.
    assert!(tl.atl > tl.ctl, "ATL {} vs CTL {}", tl.atl, tl.ctl);
    assert!((tl.tsb - (tl.ctl - tl.atl)).abs() < 0.11, "TSB = CTL − ATL");
    // Plain-language method, TRIMP + CTL/ATL kept, but the raw τ/EWMA
    // formula-speak is gone (the shell glossary defines the abbreviations).
    assert!(
        tl.method.contains("TRIMP") && tl.method.contains("CTL"),
        "{}",
        tl.method
    );
    assert!(
        !tl.method.contains('τ') && !tl.method.contains("EWMA"),
        "{}",
        tl.method
    );
    assert!(tl.summary.contains("not a prediction of performance"));
    assert_eq!(tl.evidence.grade, Some(EvidenceGrade::Moderate));
    assert!(!tl.evidence.citation.is_empty());
}

#[test]
fn training_load_day_loop_is_bounded_against_a_mis_unit_timestamp() {
    // A run logged with a MILLISECOND observed_at (1.7e12) next to a normal
    // seconds run would, without a cap, make the CTL/ATL day loop iterate
    // over ~19.7M epoch-days inside view(): a data-reachable hang (HIGH
    // bug). The accumulation window must stay bounded to MAX_LOAD_DAYS
    // regardless of the raw span between the two stamps.
    let app = Engine;
    let mut model = Model::default();
    app.update(log_run_at(10.0, 60.0, 75.0, 1_700_000_000), &mut model)
        .expect_only_render(); // ~2023, seconds
    app.update(log_run_at(10.0, 60.0, 75.0, 1_700_000_000_000), &mut model)
        .expect_only_render(); // milliseconds - off by 1000×

    // view() returns (no unbounded loop) and the window is capped, never the
    // ~19.7M-day raw span between the two stamps.
    let tl = app.view(&model).training_load.expect("load view present");
    assert!(
        tl.days <= MAX_LOAD_DAYS as u32,
        "CTL/ATL window {} days must be capped at {}",
        tl.days,
        MAX_LOAD_DAYS
    );
    // Spans past the cap collapse to exactly the cap, and both runs are still
    // counted (counting is independent of the accumulation window).
    assert_eq!(tl.days, MAX_LOAD_DAYS as u32);
    assert_eq!(tl.sessions_counted, 2);

    // A stamp near i64::MAX must also return promptly, not freeze view().
    app.update(log_run_at(10.0, 60.0, 75.0, i64::MAX - 1), &mut model)
        .expect_only_render();
    let tl = app.view(&model).training_load.expect("load view present");
    assert!(tl.days <= MAX_LOAD_DAYS as u32);
}

#[test]
fn weekly_report_runs_the_volume_system_over_the_latest_week() {
    let app = Engine;
    let mut model = Model::default();
    assert!(app.view(&model).weekly_report.is_empty());

    // The weekly report buckets Mon–Sun LOCAL weeks, not Thu-00:00-UTC
    // blocks, so the running-031 ramp cap / running-028 flag are judged over
    // the user's real training week. Anchor both weeks on a Monday
    // (mon0_weekday(704) == 0) so the prior/current split is one clean
    // calendar week.
    let w1 = 704 * DAY_SEC; // a Monday
    let w2 = w1 + WEEK; // the following Monday
    // Prior week: 3 × 10 km easy (Z1, 75 %HRmax) = 30 km.
    for i in [0i64, 2, 4] {
        app.update(log_run_at(10.0, 62.0, 75.0, w1 + i * DAY_SEC), &mut model)
            .expect_only_render();
    }
    // Current week: 20 + 16 = 36 km (+20%, over any cap), one Z3 run.
    app.update(log_run_at(20.0, 120.0, 75.0, w2), &mut model)
        .expect_only_render();
    app.update(log_run_at(16.0, 90.0, 90.0, w2 + 2 * DAY_SEC), &mut model)
        .expect_only_render();

    let report = app.view(&model).weekly_report;
    assert!(!report.is_empty());
    for row in &report {
        assert!(
            row.evidence.grade.is_some() && !row.evidence.citation.is_empty(),
            "{}",
            row.summary
        );
        assert_ne!(row.evidence.grade, Some(EvidenceGrade::MarketingMyth));
    }
    let has = |needle: &str| report.iter().any(|r| r.summary.contains(needle));
    assert!(has("over the increase cap"), "rows: {report:#?}");
    // Long run 20/36 km = 56%: over the ≤25% Daniels single-run cap.
    assert!(has("over the ≤25% single-run cap"), "rows: {report:#?}");
    assert!(
        has("time in each heart-rate zone"),
        "counting-method note present"
    );
    assert!(has("Long run 20.0 km"), "share row present");
    assert!(
        report
            .iter()
            .any(|r| r.summary_mi.contains("Long run 12.4 mi")),
        "the share row carries a miles twin: {report:#?}"
    );
    // 20 km long run vs a 5.1 km daily average: over the 2× bound.
    assert!(
        has("exceeds 2× your average daily distance"),
        "daily-avg bound row"
    );
    // Z1 share: 120 of 210 min = 57%, below the 80% easy floor.
    assert!(has("below the ~80% easy floor"), "rows: {report:#?}");
    // One hard (Z3-by-heart-rate) session, within quality caps.
    assert!(has("1 hard session"), "rows: {report:#?}");
}

#[test]
fn weekly_report_does_not_flag_the_planned_rebound_off_a_recovery_week() {
    // With a plan, the week after the PLANNED recovery week must ratio against the
    // last LOAD week, not the deloaded one — the athlete's own logged rebound off a
    // recovery week is expected, not an over-cap violation. Without a plan the same
    // +43% rebound is still flagged (behavior unchanged).
    let app = Engine;
    let mut model = Model::default();
    // Running-only WeekToWeek (intermediate) profile: cycle is 3 load weeks + 1
    // recovery week (recovery = block-week index 3).
    app.update(Event::SetProfile(running_profile()), &mut model)
        .expect_only_render();
    // Epoch day 704 is a Monday; block weeks 0..=4 anchor on successive Mondays.
    // Each week's volume is split over two runs (Mon + Wed): the plan advances
    // by completed work (ADH-PROGRESS-001), and 2 of 3 planned runs completes a
    // week.
    let wk = |w: i64| (704 + w * 7) * DAY_SEC; // Monday of block-week w, in seconds
    for (w, km) in [(0, 40.0), (1, 42.0), (2, 44.0), (3, 28.0), (4, 40.0)] {
        for d in [0, 2] {
            app.update(
                log_run_at(km / 2.0, km * 3.0, 75.0, wk(w) + d * DAY_SEC),
                &mut model,
            )
            .expect_only_render();
        }
    }
    app.update(
        Event::GeneratePlan {
            start_epoch_day: 704,
        },
        &mut model,
    )
    .expect_only_render();
    // Today is the Wednesday of block-week 4, the day of that week's second
    // run, so the whole 40 km rebound week is in view (a Monday "today" would
    // hide the Wednesday run as future-dated and show only 20 km).
    app.update(
        Event::SetToday {
            epoch_day: 704 + 4 * 7 + 2,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let report = app.view(&model).weekly_report;
    let has = |needle: &str| report.iter().any(|r| r.summary.contains(needle));
    assert!(
        !has("over the increase cap"),
        "the planned rebound off a recovery week is not over-cap: {report:#?}"
    );
    assert!(
        has("last week was the planned lighter week"),
        "the row explains the recovery-week baseline: {report:#?}"
    );

    // Without a plan the engine cannot know the 28 km week was a planned deload, so
    // the +43% rebound is flagged over-cap as before.
    app.update(Event::ClearPlan, &mut model)
        .expect_only_render();
    let report2 = app.view(&model).weekly_report;
    assert!(
        report2
            .iter()
            .any(|r| r.summary.contains("over the increase cap")),
        "without a plan the rebound is flagged over-cap: {report2:#?}"
    );
}

#[test]
fn weekly_report_hybrid_ramp_guard_fires_for_a_lifting_runner() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    let w1 = 100 * WEEK;
    app.update(log_run_at(30.0, 180.0, 75.0, w1), &mut model)
        .expect_only_render();
    app.update(log_run_at(36.0, 216.0, 75.0, w1 + WEEK), &mut model)
        .expect_only_render();

    let report = app.view(&model).weekly_report;
    assert!(
        report.iter().any(|r| r
            .summary
            .contains("keep the weekly increase to about 10% or less")),
        "hybrid-021 guard row expected: {report:#?}"
    );
}

#[test]
fn weekly_report_flags_heavy_leg_work_too_close_to_a_hard_run() {
    let app = Engine;
    let mut model = Model::default();
    let t = 100 * WEEK;
    // 18:00; the run is the next morning (a cross-day pair). A same-day pair
    // ≥6 h apart gets the softer same-day note instead.
    app.update(
        log_set("Barbell back squat", 120.0, 5, 8.0, t + 18 * 3600),
        &mut model,
    )
    .expect_only_render();
    // The week's long run 10 h later (04:00 next day): inside the 24 h buffer.
    app.update(log_run_at(14.0, 80.0, 90.0, t + 28 * 3600), &mut model)
        .expect_only_render();

    let report = app.view(&model).weekly_report;
    let row = report
        .iter()
        .find(|r| r.summary.contains("keep ≥24 h between"))
        .expect("heavy-leg/run spacing row");
    assert!(row.summary.contains("10 h apart"), "{}", row.summary);
}

#[test]
fn air_squat_without_rpe_is_not_heavy_leg_work() {
    // A bodyweight air squat with no RPE has no knowable intensity, so it is not
    // heavy leg work and does not trip the heavy-leg/long-run spacing guard.
    let app = Engine;
    let mut model = Model::default();
    let t = 100 * WEEK;
    app.update(log_set("Air squat", 0.0, 5, 0.0, t), &mut model)
        .expect_only_render();
    app.update(log_run_at(14.0, 80.0, 90.0, t + 10 * 3600), &mut model)
        .expect_only_render();
    let report = app.view(&model).weekly_report;
    assert!(
        !report
            .iter()
            .any(|r| r.summary.contains("keep ≥24 h between")),
        "bodyweight air squat is not heavy leg work: {:?}",
        report.iter().map(|r| &r.summary).collect::<Vec<_>>()
    );
}

#[test]
fn backdated_run_weekly_report_matches_in_order_logging() {
    let app = Engine;
    // Reference: two runs logged oldest-first.
    let mut in_order = Model::default();
    // WEEK == 604 800 s; day 70 and day 77 are both Thursdays, so they fall
    // in consecutive Mon–Sun local weeks either way — this asserts weekly
    // aggregation is independent of LOG order, not of the week convention.
    let prev_week_at = 10 * WEEK + 1000;
    let cur_week_at = 11 * WEEK + 1000;
    for (at, km) in [(prev_week_at, 10.0), (cur_week_at, 11.0)] {
        app.update(log_run(km, km * 6.0, 70.0, 12.0, at, 0), &mut in_order)
            .expect_only_render();
    }
    // Same data, backdated: current week logged first.
    let mut backdated = Model::default();
    for (at, km) in [(cur_week_at, 11.0), (prev_week_at, 10.0)] {
        app.update(log_run(km, km * 6.0, 70.0, 12.0, at, 0), &mut backdated)
            .expect_only_render();
    }
    let a = app.view(&in_order);
    let b = app.view(&backdated);
    let wow = |vm: &ViewModel| {
        vm.weekly_report
            .iter()
            .find(|r| r.summary.contains("Week-over-week"))
            .map(|r| r.summary.clone())
            .expect("week-over-week row")
    };
    assert_eq!(
        wow(&a),
        wow(&b),
        "weekly aggregation must not depend on log order"
    );
    assert!(wow(&a).contains("10.0 → 11.0 km"), "{}", wow(&a));
    // And the run history renders chronologically in both.
    assert_eq!(b.runs[0].observed_at, prev_week_at);
    assert_eq!(b.runs[1].observed_at, cur_week_at);
}

// ── The weekly report groups a whole Mon–Sun local week ──
#[test]
fn weekly_report_groups_a_full_mon_sun_week() {
    // MON (epoch day 4) is a Monday and MON+6 its Sunday: one Mon–Sun week.
    // The legacy Thu-00:00-UTC bucket splits this week (Thursday falls
    // mid-week), isolating the Sunday run in its own "week". With local-week
    // buckets both runs share a week, so the 10 km Monday run is the week's
    // long run - not the 6 km Sunday run alone.
    let app = Engine;
    let mut model = Model::default();
    app.update(log_run_at(10.0, 62.0, 75.0, MON * DAY_SEC), &mut model)
        .expect_only_render();
    app.update(log_run_at(6.0, 37.0, 75.0, (MON + 6) * DAY_SEC), &mut model)
        .expect_only_render();
    let report = app.view(&model).weekly_report;
    assert!(
        report
            .iter()
            .any(|r| r.summary.contains("Long run 10.0 km")),
        "Mon+Sun share one local week; long run = 10 km: {report:#?}"
    );
}
