//! Run planning: C25K progression monotonicity, Karvonen %HRR bands, the
//! co-primary endurance-frequency cap, the heavy-leg / long-run spacing
//! definition, and run anchors from logged history (trailing windows, local
//! days, the detraining taper) feeding the long run and interval structure.

use super::*;

fn planned_long_run_summary(app: &Engine, model: &Model) -> String {
    planned_long_run_item(app, model).summary
}

/// Build a run history via the real ingest path with explicit timestamps.
fn runs_via_log(stamps: &[(f64, i64)]) -> Vec<LoggedRun> {
    let app = Engine;
    let mut m = Model::default();
    for (i, (km, at)) in stamps.iter().enumerate() {
        app.update(log_run(*km, km * 6.0, 70.0, 0.0, *at, i as u64 + 1), &mut m)
            .expect_only_render();
    }
    m.runs
}

// ── C25K per-session running minutes are monotone ─────────────────────────

/// Total running minutes in the week's last (hardest) C25K session.
fn c25k_run_min(week: u8) -> f64 {
    crate::running::c25k_session_rx(week, 3).value.run_sec() as f64 / 60.0
}

/// The longest continuous running bout (seconds) of the week's last session.
fn c25k_rep_sec(week: u8) -> f64 {
    crate::running::c25k_session_rx(week, 3)
        .value
        .longest_bout_sec() as f64
}

#[test]
fn c25k_running_minutes_never_regress_week_to_week() {
    // Per-session running minutes follow the NHS table (running-049): week 1
    // runs 8 min, week 9 runs 30 min, and no week runs less than the one
    // before it (weeks advance only once completed).
    let mins: Vec<f64> = (1..=9).map(c25k_run_min).collect();
    assert!(
        (mins[0] - 8.0).abs() < 1e-9,
        "week-1 anchor is 8 running min: {mins:?}"
    );
    assert!(
        (mins[8] - 30.0).abs() < 1e-9,
        "week-9 anchor is 30 running min: {mins:?}"
    );
    for w in 1..mins.len() {
        assert!(
            mins[w] >= mins[w - 1] - 1e-9,
            "week {} running minutes {} regressed below week {} ({})",
            w + 1,
            mins[w],
            w,
            mins[w - 1]
        );
    }
    // The longest continuous bout is monotone non-decreasing too, ending at
    // the 30-min continuous run.
    let reps: Vec<f64> = (1..=9).map(c25k_rep_sec).collect();
    for w in 1..reps.len() {
        assert!(
            reps[w] >= reps[w - 1] - 1e-9,
            "week {} bout {}s is shorter than week {} ({}s)",
            w + 1,
            reps[w],
            w,
            reps[w - 1]
        );
    }
    assert_eq!(reps[8], 1800.0);
}

// ── Karvonen bands use the %HRR (VO2max) reserve, not %HRmax ───────────────

#[test]
fn karvonen_band_uses_hr_reserve_not_hrmax_fractions() {
    // running-005 / RUN-KARVONEN-001: %HRR ≈ %VO2R ≈ %VO2max, never %HRmax. At
    // HRmax 190 / RHR 50 the Easy band's %HRR fractions (59–74% VO2max) give
    // 133–154 bpm; reusing the %HRmax band fractions (65–79%) as %HRR would
    // render ~141–161 bpm — ~8 bpm too hot for the low-RHR cohort this targets.
    let q = HrZoneQuery {
        age_years: 30.0,
        resting_hr_bpm: Some(50.0),
        weeks_since_recalc: None,
        weeks_since_pace_test: None,
    };
    let (rows, _, _) = build_hr_zones(&q, Some(190.0));
    let easy = rows
        .iter()
        .find(|r| r.summary.starts_with("Easy"))
        .expect("easy row present");
    assert!(
        easy.summary.contains("Karvonen 133–154 bpm"),
        "Karvonen must use the %HRR (VO2max) reserve fractions: {}",
        easy.summary
    );
    assert!(
        !easy.summary.contains("141–161"),
        "Karvonen must not reuse the %HRmax band fractions as %HRR: {}",
        easy.summary
    );
}

// ── The ≤3 d/wk endurance cap is co-primary-conditional ────────────────────

fn hybrid_run_profile(concurrent_goal: &str, days: u8, km: f64) -> Profile {
    serde_json::from_str(&format!(
        r#"{{"progression_cadence":"WeekToWeek","lift_goal":"MaxStrength",
            "goal_distance":"Marathon","concurrent_goal":"{concurrent_goal}","weekly_sets":4,
            "running_days_per_week":{days},"running_km_per_week":{km},"advanced":true}}"#
    ))
    .expect("profile fixture parses")
}

/// A strength profile with running not configured (0 run days, 0 km).
fn pure_lifter_profile() -> Profile {
    serde_json::from_str(
        r#"{"progression_cadence":"WeekToWeek","lift_goal":"MaxStrength",
            "goal_distance":"General","concurrent_goal":"Strength","weekly_sets":12,
            "running_days_per_week":0,"running_km_per_week":0.0,"advanced":false}"#,
    )
    .expect("profile fixture parses")
}

#[test]
fn endurance_priority_hybrid_keeps_full_run_frequency() {
    // CAP-5 / hybrid-013 caps endurance at ≤3 d/wk ONLY when strength/hypertrophy/
    // power is co-primary (the cap protects LIFTING from endurance interference).
    // An endurance-priority marathoner who also lifts keeps every prescribed run
    // day (File 10 hybrid-010/CONC-RE-001), so 6 running days stay 6, not 3.
    let endurance = hybrid_run_profile("EndurancePriority", 6, 50.0);
    assert_eq!(
        crate::plan::run_slot_count(&endurance),
        6,
        "endurance-priority hybrid must not be capped to the ≤3 d/wk concurrency limit"
    );
    // The cap still applies when a strength quality is co-primary.
    for goal in ["Strength", "Power", "Hypertrophy"] {
        let strength = hybrid_run_profile(goal, 6, 50.0);
        assert_eq!(
            crate::plan::run_slot_count(&strength),
            3,
            "{goal} co-primary must cap endurance at 3 d/wk"
        );
    }
}

// ── A long run is the week's LONGEST run over an external full-week basis ───

fn manual_run(km: f64, at: i64) -> LoggedRun {
    LoggedRun {
        distance_km: km,
        duration_min: km * 6.0,
        hr_pct_max: 70.0,
        longest_recent_km: 0.0,
        track: Vec::new(),
        track_segment_starts: Vec::new(),
        observed_at: at,
        entry_id: 0,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: None,
    }
}

fn heavy_squat(at: i64) -> LoggedSet {
    LoggedSet {
        exercise: "Back Squat".to_string(),
        weight_kg: 150.0,
        reps: 3,
        rpe: 9.0,
        observed_at: at,
        entry_id: 0,
        hold_sec: None,
    }
}

/// A 3-run week (easy 5 / tempo 4 / long 8 km): only the 8 km run is the long
/// run. Mondays start local weeks (day ≡ 4 mod 7), so days 4–10 are one week.
fn three_run_week() -> Vec<LoggedRun> {
    vec![
        manual_run(5.0, 5 * DAY_SEC + 8 * 3600), // Tue easy
        manual_run(4.0, 6 * DAY_SEC + 8 * 3600), // Wed tempo
        manual_run(8.0, 9 * DAY_SEC + 8 * 3600), // Sat long
    ]
}

const GAP_FLAG: &str = "keep ≥24 h between";

#[test]
fn easy_run_near_leg_day_is_not_treated_as_a_long_run() {
    // A heavy squat Mon 20:00 and the easy 5 km Tue 08:00 are ~12 h apart, but
    // the 5 km run is 29% of a partial-week total only — it is NOT the week's
    // longest run, so it must not trip the heavy-leg/long-run CAP-3 spacing flag.
    // The actual long run (8 km, Sat) is 5 days from the squat, well clear.
    let model = Model {
        profile: Some(hybrid_run_profile("EndurancePriority", 3, 17.0)),
        runs: three_run_week(),
        sets: vec![heavy_squat(4 * DAY_SEC + 20 * 3600)], // Mon evening
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    assert!(
        !report.iter().any(|r| r.summary.contains(GAP_FLAG)),
        "an easy 5 km run near leg day must not flag as a long run: {report:#?}"
    );
}

#[test]
fn heavy_squat_close_to_the_actual_long_run_still_flags() {
    // Positive control: move the heavy squat to Fri 20:00, ~12 h before the 8 km
    // long run on Sat 08:00. The 8 km run IS the week's longest and clears 25% of
    // the 17 km full-week basis, so CAP-3 must fire.
    let model = Model {
        profile: Some(hybrid_run_profile("EndurancePriority", 3, 17.0)),
        runs: three_run_week(),
        sets: vec![heavy_squat(8 * DAY_SEC + 20 * 3600)], // Fri evening
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    assert!(
        report.iter().any(|r| r.summary.contains(GAP_FLAG)),
        "a heavy squat ~12 h from the actual long run must flag CAP-3: {report:#?}"
    );
}

#[test]
fn a_pure_lifters_lone_jog_after_squats_is_not_a_long_run() {
    // A lifter with no planned mileage logs a single 3 km recovery jog the morning
    // after heavy squats. With no external basis (no plan, no other runs in the
    // trailing 7 days) the jog cannot certify itself as a long run, so CAP-3 stays
    // silent instead of flagging the recovery jog.
    let model = Model {
        profile: Some(pure_lifter_profile()),
        runs: vec![manual_run(3.0, 5 * DAY_SEC + 8 * 3600)], // Tue morning
        sets: vec![heavy_squat(4 * DAY_SEC + 20 * 3600)],    // Mon evening, ~12 h prior
        ..Default::default()
    };
    let report = build_weekly_report(&ViewCtx::new(&model), &[]);
    assert!(
        !report.iter().any(|r| r.summary.contains(GAP_FLAG)),
        "a lone recovery jog with no external basis must not flag CAP-3: {report:#?}"
    );
}

// ── Run-plan anchors: completed-week km and race-date taper wiring ─────────

/// A Monday epoch day (epoch day 0 was a Thursday).
const PLAN_MONDAY: i64 = 20_000 - (20_000_i64.rem_euclid(7) + 3).rem_euclid(7);

#[test]
fn run_week_km_buckets_completed_local_weeks_most_recent_first() {
    let at = |day: i64| day * DAY_SEC + 12 * 3600;
    let runs = vec![
        manual_run(10.0, at(PLAN_MONDAY - 1)), // last Sunday
        manual_run(5.0, at(PLAN_MONDAY - 7)),  // last Monday
        manual_run(12.0, at(PLAN_MONDAY - 8)), // the Sunday before
        manual_run(99.0, at(PLAN_MONDAY)),     // this week: not completed
        manual_run(7.0, 0),                    // undated: never counts
    ];
    let facts = RunFacts::new(&runs);
    assert_eq!(
        build_run_week_km(&runs, &facts, PLAN_MONDAY, 0),
        vec![15.0, 12.0]
    );
    assert!(build_run_week_km(&[], &RunFacts::new(&[]), PLAN_MONDAY, 0).is_empty());
}

#[test]
fn a_race_ten_days_out_synthesizes_a_taper_week() {
    let mut p = running_profile();
    p.goal_distance = GoalDistance::TenK;
    p.running_days_per_week = 5;
    p.running_km_per_week = 50.0;
    p.goal_race_epoch_day = Some(PLAN_MONDAY + 10);
    let anchors = crate::plan::Anchors {
        week_start_epoch_day: Some(PLAN_MONDAY),
        ..Default::default()
    };
    let prog = crate::plan::synthesize(&p, &anchors, 3).expect("a plan");
    assert_eq!(prog.mesocycle.phase, MesoPhase::Taper);
    // Without the date the same week is an ordinary rolling-cycle week.
    p.goal_race_epoch_day = None;
    let prog = crate::plan::synthesize(&p, &anchors, 3).expect("a plan");
    assert_ne!(prog.mesocycle.phase, MesoPhase::Taper);
}

/// A device-reported seed: a 10K runner (32.19 km/wk, 3 days) who stated an
/// 8 mi (12.87 km) longest run, set up on Fri 16 Oct 2026.
const SEEDED_10K_DAY: i64 = 20_742;

fn seeded_10k_model(stated_on: i64, today: i64) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    let profile: Profile = serde_json::from_value(serde_json::json!({
        "progression_cadence": "WeekToWeek", "lift_goal": "MaxStrength",
        "goal_distance": "TenK", "concurrent_goal": "EndurancePriority",
        "weekly_sets": 12, "running_days_per_week": 3, "running_km_per_week": 32.19,
        "advanced": false, "female": false, "age_years": 34.0, "weight_lb": true,
        "lift_days_per_week": 2, "recent_race": {"distance_km": 5.0, "time_sec": 1350},
        "stated_longest_recent_km": 12.87, "effective_from_epoch_day": stated_on
    }))
    .expect("seed profile");
    for e in [
        Event::SetToday {
            epoch_day: stated_on,
            utc_offset_sec: 0,
        },
        Event::SetProfile(profile),
        Event::GeneratePlan {
            start_epoch_day: stated_on,
        },
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
    ] {
        let _ = app.update(e, &mut model);
    }
    let _ = app.update(logrun(3.22, today), &mut model);
    (app, model)
}

fn long_run_km_of(item: &PrescriptionView) -> f64 {
    match item.run_volume {
        Some(RunAmount::PlanKm(k)) => k as f64,
        Some(RunAmount::Km(k)) => k,
        other => panic!("a distance long run, got {other:?}"),
    }
}

#[test]
fn a_logged_shakeout_keeps_the_stated_long_run() {
    // One logged 2 mi run collapsed the Sunday long run from
    // 7.8 mi to 2.2 mi (10% over the logged run).
    let (app, model) = seeded_10k_model(SEEDED_10K_DAY, SEEDED_10K_DAY);
    let item = planned_long_run_item(&app, &model);
    let km = long_run_km_of(&item);
    let mi = (km / 1.609_344 * 10.0).round() / 10.0;
    assert!(mi >= 7.8, "long run {km} km shows ≥ 7.8 mi");
    assert_eq!(
        item.evidence.citation,
        graded((), claims::RUN_START_001).reference()
    );
    // The safety badge says what it caps, in plain words.
    assert_eq!(
        item.safety_reason,
        format!(
            "Capped at {}: at most 10% over the longest recent run you told us about",
            item.summary.split(" · ").next().unwrap()
        )
    );
    // The miles twin states the same cap in miles; the head carries its km.
    let shown_km = (km * 10.0).round() / 10.0;
    assert_eq!(item.run_head_km, Some(shown_km));
    let mi_txt = format!("{:.1}", shown_km / 1.609_344);
    assert_eq!(
        item.safety_reason_mi,
        format!(
            "Capped at {} mi: at most 10% over the longest recent run you told us about",
            mi_txt.strip_suffix(".0").unwrap_or(&mi_txt)
        )
    );
}

#[test]
fn a_stale_stated_long_run_yields_to_the_logged_run() {
    // 31+ days after it was stated, the answer is no longer "the last 30 days".
    let today = SEEDED_10K_DAY + 35;
    let (app, model) = seeded_10k_model(SEEDED_10K_DAY, today);
    let km = long_run_km_of(&planned_long_run_item(&app, &model));
    assert!(km <= 3.22 * 1.1 + 1e-9, "logged baseline governs: {km}");
}

#[test]
fn re_saving_an_unchanged_stated_long_run_does_not_refresh_it() {
    let today = SEEDED_10K_DAY + 35;
    let (app, mut model) = seeded_10k_model(SEEDED_10K_DAY, today);
    let mut p = model.profile.clone().expect("profile");
    p.effective_from_epoch_day = Some(today);
    p.weekly_sets = 10;
    let _ = app.update(Event::SetProfile(p), &mut model);
    let km = long_run_km_of(&planned_long_run_item(&app, &model));
    assert!(km <= 3.22 * 1.1 + 1e-9, "re-save keeps the old date: {km}");
}

#[test]
fn a_dated_race_plan_counts_weeks_to_the_race() {
    // A 10K on Sat 28 Nov 2026 set up on Wed 14 Oct read
    // "build phase · week 1 of 4" (the 3:1 block position), though the race
    // is 7 calendar weeks away and the block is aligned back from it.
    let today = 20_740;
    let race = 20_785;
    let app = Engine;
    let mut model = Model::default();
    let profile = Profile {
        goal_distance: GoalDistance::TenK,
        goal_race_epoch_day: Some(race),
        ..running_profile()
    };
    for e in [
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
        Event::SetProfile(profile),
        Event::GeneratePlan {
            start_epoch_day: today,
        },
    ] {
        let _ = app.update(e, &mut model);
    }
    let program = |m: &Model| app.view(m).program.expect("a program card");
    let p = program(&model);
    assert_eq!((p.week, p.weeks_total), (1, 7), "{p:?}");
    assert_eq!(p.phase, MesoPhase::Build);
    let _ = app.update(
        Event::SetToday {
            epoch_day: race - 1,
            utc_offset_sec: 0,
        },
        &mut model,
    );
    let p = program(&model);
    assert_eq!((p.week, p.weeks_total), (7, 7), "race week: {p:?}");
    assert_eq!(p.phase, MesoPhase::Taper);
}

#[test]
fn the_long_run_anchors_to_logged_recent_distance_not_a_stale_profile() {
    // Profile claims 16 km/wk, but the athlete has logged two recent 21 km
    // runs. The measured weekly volume reflects the real recent training
    // (a partial window is not diluted by a fixed /4), so the plan trusts
    // the demonstrated capacity and anchors the long run to the 21 km the
    // athlete has actually run (never a stale-profile 4 km).
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(running_profile()), &mut model)
        .expect_only_render();
    app.update(logrun(21.0, MON - 2), &mut model)
        .expect_only_render();
    app.update(logrun(21.0, MON - 1), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: MON,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();

    let item = planned_long_run_item(&app, &model);
    assert!(
        item.summary.starts_with("21 km"),
        "the long run anchors to the demonstrated recent distance, got {:?}",
        item.summary
    );
    assert!(
        !item.evidence.citation.is_empty(),
        "the long run carries an evidence citation, got {:?}",
        item.evidence.citation
    );
}

#[test]
fn a_volume_dominated_long_run_card_still_cites_the_share_cap() {
    // Counterpart: a long run set by the weekly-volume share (30 % under
    // 64 km/wk) keeps citing RUN-LONGRUN-001 (Daniels).
    let app = Engine;
    let mut model = Model::default();
    let mut p = running_profile();
    p.running_km_per_week = 60.0; // 0.30×60 = 18 km, volume-dominated
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: MON,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();

    let item = planned_long_run_item(&app, &model);
    assert!(item.summary.starts_with("18 km"), "{:?}", item.summary);
    assert!(
        item.evidence.citation.contains("Daniels"),
        "a volume-dominated long run keeps the ≤25% share citation, got {:?}",
        item.evidence.citation
    );
}

#[test]
fn a_stale_run_outside_the_window_does_not_raise_the_long_run() {
    // A FULLY-detrained 60-day-old 21 km run is past the 30 + 28 = 58-day decay
    // horizon → zero capacity credit → the plan falls back to the stated-volume
    // rule (the KB "> 8 wk off → rebuild base" regime). Within the horizon the
    // anchor TAPERS instead of vanishing (covered by the taper tests); this guards
    // the far end where it is finally gone.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(running_profile()), &mut model)
        .expect_only_render();
    app.update(logrun(21.0, MON - 60), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: MON,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();

    let summary = planned_long_run_summary(&app, &model);
    // Fully-detrained run ignored → 0.30 × 16 = 4.8 km (as with no history).
    assert!(
        summary.starts_with("4.8 km"),
        "a fully-detrained run must not raise the long run, got {summary:?}"
    );
}

#[test]
fn a_log_less_running_plan_is_unchanged_by_the_run_anchors() {
    // Regression: with no logged runs the long run is the pre-anchor value.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(running_profile()), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: MON,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(planned_long_run_summary(&app, &model).starts_with("4.8 km"));
}

#[test]
fn the_run_reactive_plan_is_deterministic_across_repeated_views() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(running_profile()), &mut model)
        .expect_only_render();
    app.update(logrun(21.0, MON - 2), &mut model)
        .expect_only_render();
    app.update(logrun(18.0, MON - 1), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: MON,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let a = app.view(&model);
    let b = app.view(&model);
    assert_eq!(
        a.week_plan, b.week_plan,
        "same model must yield the same week"
    );
    assert_eq!(a.next_session, b.next_session);
}

#[test]
fn build_run_anchors_uses_the_trailing_windows() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(running_profile()), &mut model)
        .expect_only_render();
    let today = 400i64;
    // In-window: 21 km (2 d ago) and 10 km (5 d ago).
    app.update(logrun(21.0, today - 2), &mut model)
        .expect_only_render();
    app.update(logrun(10.0, today - 5), &mut model)
        .expect_only_render();
    // Stale: 30 km 40 d ago, outside both the 30-day and 28-day windows.
    app.update(logrun(30.0, today - 40), &mut model)
        .expect_only_render();

    let (longest, weekly, _) = run_anchors(&model.runs, today, 0);
    assert_eq!(longest, Some(21.0), "the stale 30 km run must not anchor");
    // recent_weekly divides by the ~1 covered week, not a fixed 4:
    // (21 + 10) / 1 = 31.
    assert_eq!(weekly, Some(31.0));

    // A future-dated row (after `today`) is excluded, like `spike_baseline_km`.
    app.update(logrun(99.0, today + 5), &mut model)
        .expect_only_render();
    let (longest2, _, _) = run_anchors(&model.runs, today, 0);
    assert_eq!(longest2, Some(21.0), "a future-dated run must not anchor");
    // Weekly-window boundary (strict `<`). A run 27 days old counts
    // toward weekly km; a run exactly 28 days old is excluded.
    app.update(logrun(8.0, today - 27), &mut model)
        .expect_only_render();
    app.update(logrun(12.0, today - 28), &mut model)
        .expect_only_render();
    let (longest3, weekly3, _) = run_anchors(&model.runs, today, 0);
    assert_eq!(
        weekly3,
        Some((21.0 + 10.0 + 8.0) / 4.0),
        "27 d old is inside the 28-day window; exactly 28 d old is excluded"
    );
    assert_eq!(
        longest3,
        Some(21.0),
        "neither boundary run exceeds the anchor"
    );
}

// ── Run anchors bucket by LOCAL day, so a run logged today counts ──
#[test]
fn run_anchors_count_a_same_day_run_by_local_day() {
    let today = 500i64;

    // Berlin (+2 h): a run at 08:00 LOCAL today. Its UTC `observed_at` is later
    // than `today * DAY_SEC`, so a UTC-instant predicate would exclude it.
    let berlin = 2 * 3600;
    let berlin_0800 = today * DAY_SEC + 8 * 3600 - berlin;
    let runs_b = runs_via_log(&[(12.0, berlin_0800)]);
    let (longest_b, weekly_b, _) = run_anchors(&runs_b, today, berlin);
    assert_eq!(
        longest_b,
        Some(12.0),
        "an 08:00-today run (Berlin) must count"
    );
    assert_eq!(weekly_b, Some(12.0)); // one run this week / 1 covered week

    // UTC−5: an evening run today whose UTC stamp rolls into the next UTC day.
    let west = -5 * 3600;
    let west_2000 = today * DAY_SEC + 20 * 3600 - west;
    let runs_w = runs_via_log(&[(9.0, west_2000)]);
    let (longest_w, _, _) = run_anchors(&runs_w, today, west);
    assert_eq!(longest_w, Some(9.0), "a 20:00-today run (UTC−5) must count");

    // A run on local TOMORROW is future-dated and excluded.
    let tomorrow_0030 = (today + 1) * DAY_SEC + 30 * 60 - berlin;
    let runs_f = runs_via_log(&[(30.0, tomorrow_0030)]);
    let (longest_f, weekly_f, _) = run_anchors(&runs_f, today, berlin);
    assert_eq!(longest_f, None, "a local-tomorrow run must not anchor");
    assert_eq!(weekly_f, None);
}

#[test]
fn run_anchors_offset_zero_midnight_history_is_unchanged() {
    // The FULL-CREDIT window edge for an offset-0, midnight-stamped history is
    // unchanged: a run exactly 30 days ago still anchors at full distance and
    // is NOT flagged detrained (unchanged from the pre-taper behaviour).
    let today = 500i64;
    let at_30 = (today - 30) * DAY_SEC;
    let (l30, _, det30) = run_anchors(&runs_via_log(&[(8.0, at_30)]), today, 0);
    assert_eq!(
        l30,
        Some(8.0),
        "a run exactly 30 days ago still anchors at full"
    );
    assert!(
        !det30,
        "a run at the window edge is full-credit, not detrained"
    );
    // A run 31 days ago does not VANISH at the window edge; its credit is
    // tapered by one day of the 28-day detraining slope: 8 × 27/28 = 7.714…
    let at_31 = (today - 31) * DAY_SEC;
    let (l31, _, det31) = run_anchors(&runs_via_log(&[(8.0, at_31)]), today, 0);
    let expected_31 = 8.0 * 27.0 / 28.0;
    assert!(
        (l31.unwrap() - expected_31).abs() < 1e-9,
        "day-31 taper: got {l31:?}, want {expected_31}"
    );
    assert!(
        det31,
        "a beyond-window anchor is flagged detraining-adjusted"
    );
}

// ── Past the window, credit tapers on a detraining slope, no cliff ──
#[test]
fn longest_run_anchor_tapers_past_the_window_instead_of_a_cliff() {
    // Flagship: a lone 21.1 km race is the only logged run. Across the 30-day
    // full-credit edge the anchor must SLOPE (retained then decaying) rather
    // than drop from 21.1 to None in one overnight step.
    let today = 1_000i64;
    let race_at = |age: i64| (today - age) * DAY_SEC;
    let anchor = |age: i64| run_anchors(&runs_via_log(&[(21.1, race_at(age))]), today, 0);

    // Day 30: full credit, not detrained.
    let (l30, _, d30) = anchor(30);
    assert!((l30.unwrap() - 21.1).abs() < 1e-9, "day 30 full: {l30:?}");
    assert!(!d30);

    // Day 31: retained, tapered by one slope-day (21.1 × 27/28 = 20.346…) -
    // a ~0.75 km step, NOT a 21.1 → None cliff. Detraining-adjusted.
    let (l31, _, d31) = anchor(31);
    let want31 = 21.1 * 27.0 / 28.0;
    assert!(
        (l31.unwrap() - want31).abs() < 1e-9,
        "day 31: {l31:?} want {want31}"
    );
    assert!(d31, "beyond-window anchor is detraining-adjusted");
    assert!(
        l31.unwrap() > 20.0,
        "the step off the window is small, not a cliff"
    );

    // Day 45: 15 slope-days in → 21.1 × 13/28 = 9.796… (still a real anchor).
    let (l45, _, d45) = anchor(45);
    let want45 = 21.1 * 13.0 / 28.0;
    assert!(
        (l45.unwrap() - want45).abs() < 1e-9,
        "day 45: {l45:?} want {want45}"
    );
    assert!(d45);

    // The trajectory is monotonically decreasing across the boundary, a slope.
    assert!(l30.unwrap() > l31.unwrap() && l31.unwrap() > l45.unwrap());

    // Day 58 (30 + 28): fully detrained → no capacity credit (aligns with the
    // KB "4–8 wk off → treat near-novice" bracket). Day 57 still carries a sliver.
    let (l58, _, _) = anchor(58);
    assert_eq!(
        l58, None,
        "at day 58 the demonstrated-capacity credit is gone"
    );
    let (l57, _, _) = anchor(57);
    assert!(
        l57.unwrap() > 0.0 && l57.unwrap() < 1.0,
        "day 57 is a sliver: {l57:?}"
    );
}

#[test]
fn a_fresh_run_overrides_a_decayed_older_run_no_change() {
    // A fresh in-window run that is longer than any decayed older run keeps the
    // anchor at full credit and NOT detrained, so the taper only ever RAISES
    // the past-window case, never perturbs live history.
    let today = 1_000i64;
    let runs = runs_via_log(&[
        (21.1, (today - 40) * DAY_SEC), // old race, decays to 21.1×18/28 = 13.56
        (16.0, (today - 3) * DAY_SEC),  // fresh 16 km run beats the decayed race
    ]);
    let (l, _, det) = run_anchors(&runs, today, 0);
    assert_eq!(l, Some(16.0), "the fresh run wins");
    assert!(!det, "a fresh winning anchor is not detraining-adjusted");
}

// ── Interval rep structure + HR band ceiling copy ──
#[test]
fn flatten_surfaces_interval_rep_structure_and_hr_band_ceiling() {
    use crate::schema::RunPrescription;
    let anchors = crate::plan::Anchors::default();

    let interval = graded(
        Prescription::Run(RunPrescription::new(
            RunVolume::DurationMin(16),
            RunIntensity::Vdot(VdotBand::Interval),
            Some((4, RunVolume::DurationMin(4))),
        )),
        claims::RUN_INTERVAL_001,
    );
    let it = flatten_prescription(&interval, &anchors, None, false);
    assert_eq!(it.rep_count, 4);
    assert_eq!(it.rep_volume, "4 min");
    assert!(
        it.summary.starts_with("4 × 4 min"),
        "the summary must lead with rep structure, got {:?}",
        it.summary
    );
    assert!(
        !it.summary.starts_with("16 min"),
        "must not read as one continuous 16 min run"
    );

    // A continuous ceiling-only %HRmax run: rep fields empty, band rendered
    // as a ceiling.
    let cont = graded(
        Prescription::Run(RunPrescription::new(
            RunVolume::DistanceKm(10.0),
            RunIntensity::HrPercentMax {
                lo: None,
                hi: Some(65.0),
                hrr_hi: None,
            },
            None,
        )),
        claims::RUN_LONGRUN_001,
    );
    let ci = flatten_prescription(&cont, &anchors, None, false);
    assert_eq!(ci.rep_count, 0);
    assert!(ci.rep_volume.is_empty());
    assert_eq!(
        ci.summary, "10 km · ≤ 65% HRmax",
        "a ceiling-only %HRmax target renders as a ≤ ceiling, not a point"
    );

    // A two-sided %HRmax band (Tempo 88–92) renders as a range, not a floor
    // masquerading as a ceiling.
    let tempo = graded(
        Prescription::Run(RunPrescription::new(
            RunVolume::DurationMin(30),
            RunIntensity::HrPercentMax {
                lo: Some(88.0),
                hi: Some(92.0),
                hrr_hi: None,
            },
            None,
        )),
        claims::RUN_TEMPO_001,
    );
    let ti = flatten_prescription(&tempo, &anchors, None, false);
    assert_eq!(ti.summary, "30 min · 88–92% HRmax");

    // A Recovery band carries both its %HRmax ceiling and the Karvonen %HRR
    // ceiling: the stricter reading is surfaced too.
    let recovery = graded(
        Prescription::Run(RunPrescription::new(
            RunVolume::DurationMin(30),
            RunIntensity::HrPercentMax {
                lo: None,
                hi: Some(76.0),
                hrr_hi: Some(70.0),
            },
            None,
        )),
        claims::RUN_RECOVERY_001,
    );
    let ri = flatten_prescription(&recovery, &anchors, None, false);
    assert_eq!(
        ri.summary,
        "30 min · ≤ 76% HRmax · ≤ 70% of heart-rate reserve"
    );
}
