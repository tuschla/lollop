//! Run data: numeric run targets, session-RPE load and run meta edits, run
//! progress / best efforts / plan labels, GPS QC and moving time, tag- and
//! pace-aware hard-run spacing, and CAP-3 heavy-leg spacing in both
//! directions.

use super::*;
use crate::app::plan_views::EASY_RUN_LABEL;
use crate::app::run_views::PROGRESS_WEEKS;

/// A recent-race runner: 10 km in 50:00 (VDOT ≈ 40) one week before `today`.
fn racer_profile(today: i64) -> Profile {
    Profile {
        recent_race: Some(RecentRace {
            distance_km: 10.0,
            time_sec: 3000,
            epoch_day: Some(today - 7),
        }),
        ..running_profile()
    }
}

/// A fresh-10K racer who runs five days a week (10K goal, 40 km/wk): the
/// plan week carries a tempo run.
fn tempo_racer_profile() -> Profile {
    Profile {
        advanced: true,
        running_days_per_week: 5,
        running_km_per_week: 40.0,
        goal_distance: GoalDistance::TenK,
        measured_hr_max: Some(190.0),
        ..racer_profile(MON)
    }
}

/// A 5K racer (fresh 10K, measured HRmax 190) four weeks from race day: the
/// peak-phase plan week carries intervals.
fn interval_racer_profile() -> Profile {
    Profile {
        running_days_per_week: 5,
        running_km_per_week: 50.0,
        goal_distance: GoalDistance::FiveK,
        goal_race_epoch_day: Some(MON + 33),
        measured_hr_max: Some(190.0),
        ..racer_profile(MON)
    }
}

/// `profile` planned from `MON`: the first item of the week's `title` session
/// as planned, then the same item on its own day after a low-HRV morning
/// eased it, and the week's easy run as planned.
fn eased_on_its_day(
    profile: Profile,
    title: &str,
) -> (PrescriptionView, PrescriptionView, PrescriptionView) {
    let (app, mut model) = planned_on(profile, [], MON);
    let week = app.view(&model).week_plan;
    let row = |t: &str| {
        week.iter()
            .find(|r| r.title == t)
            .unwrap_or_else(|| panic!("a {t} in the plan week: {week:#?}"))
    };
    let (day, planned) = (row(title).epoch_day, row(title).items[0].clone());
    let easy = row("Easy run").items[0].clone();
    set_today(&app, &mut model, day);
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            observed_at: day * DAY_SEC + 60,
            ..input(ReadinessSignal::HrvLnRmssd, -0.8)
        }),
        &mut model,
    )
    .expect_only_render();
    let today = app.view(&model).next_session.expect("today's session");
    assert_eq!(today.epoch_day, day);
    (planned, today.items[0].clone(), easy)
}

#[test]
fn threshold_run_renders_pace_range_from_a_recent_10k() {
    let (app, model) = planned_on(tempo_racer_profile(), [], MON);
    let it = app
        .view(&model)
        .week_plan
        .into_iter()
        .find(|r| r.title == "Tempo run")
        .expect("a tempo run in the plan week")
        .items
        .swap_remove(0);
    let t = it
        .run_target
        .clone()
        .expect("a tempo run carries a numeric target");
    assert_eq!(it.workout_type, Some(WorkoutType::Tempo));
    // Daniels VDOT 40: T ≈ 5:06 /km (88 %VO2max) to ≈ 5:20 /km (83 %).
    assert!((303..=309).contains(&t.pace_fast_sec_per_km), "{t:?}");
    assert!((317..=323).contains(&t.pace_slow_sec_per_km), "{t:?}");
    assert!(
        t.pace_label.starts_with("5:0") && t.pace_label.ends_with(" /km"),
        "{t:?}"
    );
    assert!(
        it.summary.contains(&t.pace_label),
        "the card reads the pace: {}",
        it.summary
    );
    assert!(
        it.intensity_label.contains("effort 6–7 of 10"),
        "{}",
        it.intensity_label
    );
    assert_eq!(
        t.evidence.citation,
        Recommended::new((), claims::RUN_VDOT_001).reference()
    );
}

/// Each run item carries the run type of the session it belongs to, so a
/// tracker following one item preselects that item's type whatever the next
/// session is. The long run reads LongRun though it runs in the easy band;
/// lift items carry none.
#[test]
fn run_items_carry_their_own_sessions_workout_type() {
    let (app, model) = planned_on(tempo_racer_profile(), [], MON);
    let vm = app.view(&model);
    let mut types = Vec::new();
    for s in vm.next_session.iter().chain(&vm.week_plan) {
        for it in &s.items {
            let at = format!("{}: {}", s.title, it.summary);
            match s.session_type {
                SessionType::Run(k) if it.is_run() => {
                    let wt = k
                        .workout_type()
                        .expect("each kind in this week names a run type");
                    assert_eq!(it.workout_type, Some(wt), "{at}");
                }
                _ => assert_eq!(it.workout_type, None, "{at}"),
            }
            types.extend(it.workout_type);
        }
    }
    for wt in [WorkoutType::LongRun, WorkoutType::Tempo, WorkoutType::Easy] {
        assert!(types.contains(&wt), "{wt:?} in {types:?}");
    }
}

/// A race-pace day names no run type of its own, so it preselects by the band
/// it runs in: Steady at marathon pace, Easy once a low-HRV morning eases it
/// to an easy run.
#[test]
fn race_pace_day_preselects_by_its_band() {
    let profile = Profile {
        goal_distance: GoalDistance::Marathon,
        running_days_per_week: 5,
        running_km_per_week: 60.0,
        measured_hr_max: Some(190.0),
        goal_race_epoch_day: Some(MON + 7 * 10 + 6),
        ..racer_profile(MON)
    };
    let (planned, eased, _) = eased_on_its_day(profile, "Race-pace run");
    assert_eq!(planned.workout_type, Some(WorkoutType::Steady));
    assert!(
        eased.intensity_label.starts_with(EASY_RUN_LABEL),
        "{}",
        eased.intensity_label
    );
    assert_eq!(eased.workout_type, Some(WorkoutType::Easy));
}

#[test]
fn easy_run_without_anchors_shows_the_talk_test_cue() {
    // No race, no age, no measured HRmax: the long run has no pace or bpm, but
    // it still tells the runner how it should feel.
    let model = {
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
        set_today(&app, &mut model, MON);
        model
    };
    let it = planned_long_run_item(&model);
    let t = it.run_target.clone().expect("run items carry a target");
    assert!(t.pace_label.is_empty() && t.hr_label.is_empty(), "{t:?}");
    assert!(
        t.effort_cue.contains("talk in full sentences"),
        "talk-test cue without anchors: {t:?}"
    );
    assert!(
        it.intensity_label.contains("talk in full sentences"),
        "{}",
        it.intensity_label
    );
    assert_eq!(
        t.evidence.citation,
        Recommended::new((), claims::RUN_TALKTEST_001).reference()
    );
}

#[test]
fn easy_run_shows_bpm_from_measured_hrmax_and_pace_from_race() {
    let app = Engine;
    let mut model = Model::default();
    let profile = Profile {
        measured_hr_max: Some(190.0),
        ..racer_profile(MON)
    };
    app.update(Event::SetProfile(profile), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, MON);
    let it = planned_long_run_item(&model);
    let t = it.run_target.clone().expect("run items carry a target");
    assert!(!t.pace_label.is_empty(), "race anchor → easy pace: {t:?}");
    let ceiling = |t: &RunTargetView| -> u32 {
        t.hr_label
            .strip_suffix(" bpm")
            .and_then(|s| s.rsplit(['–', ' ']).next())
            .and_then(|h| h.parse().ok())
            .unwrap_or_else(|| panic!("a bpm ceiling: {t:?}"))
    };
    let high = ceiling(&t);
    assert!(high > 0 && high <= 190, "{t:?}");
    // The band follows the measured HRmax, not the age estimate.
    let (_, lower) = planned_on(
        Profile {
            measured_hr_max: Some(171.0),
            ..racer_profile(MON)
        },
        [],
        MON,
    );
    let lower = planned_long_run_item(&lower).run_target.expect("a target");
    assert!(
        (f64::from(ceiling(&lower)) - f64::from(high) * 0.9).abs() <= 1.0,
        "{high} bpm at 190, {lower:?} at 171"
    );
    assert!(
        !t.effort_cue.contains("talk"),
        "measured anchors replace the talk test: {t:?}"
    );
    assert!(
        it.intensity_label.starts_with(&t.pace_label),
        "{}",
        it.intensity_label
    );
}

#[test]
fn downgrade_re_resolves_the_easy_band() {
    let (planned, eased, easy) = eased_on_its_day(tempo_racer_profile(), "Tempo run");
    let t = planned.run_target.expect("a planned threshold target");
    let e = eased.run_target.clone().expect("still targeted");
    let easy = easy.run_target.expect("easy target");
    assert_eq!(
        e.effort_cue, easy.effort_cue,
        "the downgrade reads the easy run's effort"
    );
    assert_ne!(e.effort_cue, t.effort_cue);
    // A tempo session names its run type, so it stays the preselected one.
    assert_eq!(eased.workout_type, Some(WorkoutType::Tempo));
    assert!(
        e.pace_fast_sec_per_km > t.pace_slow_sec_per_km,
        "easy is slower: {e:?}"
    );
    assert!(
        eased
            .intensity_label
            .starts_with(&format!("{EASY_RUN_LABEL} · ")),
        "{}",
        eased.intensity_label
    );

    // A miles target stays in miles through the downgrade; the item's own
    // line keeps km.
    let (_, eased, _) = eased_on_its_day(
        Profile {
            distance_unit: DistanceUnit::Mi,
            ..tempo_racer_profile()
        },
        "Tempo run",
    );
    let e = eased.run_target.expect("still targeted");
    assert!(e.pace_label.ends_with(" /mi"), "{e:?}");
    assert!(
        eased.intensity_label.contains(" /km"),
        "{}",
        eased.intensity_label
    );
}

#[test]
fn downgrade_keeps_anchors_from_interval() {
    // An interval shows no bpm; it still carries the anchors, so its
    // downgrade reads like the week's planned easy run.
    let (planned, eased, easy) = eased_on_its_day(interval_racer_profile(), "Intervals");
    assert_eq!(planned.workout_type, Some(WorkoutType::Interval));
    let p = planned.run_target.expect("a planned interval target");
    assert!(p.hr_label.is_empty(), "{p:?}");
    let (e, easy) = (
        eased.run_target.expect("target"),
        easy.run_target.expect("easy target"),
    );
    assert!(!e.hr_label.is_empty() && !e.pace_label.is_empty(), "{e:?}");
    assert_eq!(
        (e.hr_label.as_str(), e.pace_label.as_str()),
        (easy.hr_label.as_str(), easy.pace_label.as_str()),
        "the interval downgraded to easy"
    );
}

fn track_event(points: Vec<GpsPoint>, at: i64, id: u64, rpe: Option<u8>) -> Event {
    with_run_meta(log_run_track(points, 0.0, 50.0, at, id), None, rpe, None)
}

#[test]
fn tracked_run_with_session_rpe_carries_srpe_load() {
    // 40 min at 3 m/s, no heart rate, rated 7/10 → 40 × 7 = 280 AU.
    let app = Engine;
    let mut model = Model::default();
    let t0 = 700 * DAY_SEC;
    let pts = equator_track(t0, &[(7200.0, 3.0)], 10);
    app.update(track_event(pts, t0 + 2400, 9_001, Some(7)), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    let run = &vm.runs[0];
    assert!(
        (run.duration_min - 40.0).abs() < 1e-6,
        "{}",
        run.duration_min
    );
    assert_eq!(run.session_rpe, Some(7));
    assert_eq!(run.srpe_load_au, Some(280.0));
    // The separate weekly series carries it; the HR load does not.
    let s = vm.srpe_load.expect("a rated run opens the sRPE series");
    assert!(
        s.summary
            .starts_with("Effort load 280 this week (1 rated run)"),
        "{}",
        s.summary
    );
    assert!(
        vm.training_load.is_none(),
        "an HR-less run adds no heart-rate load"
    );
    // An unrated run carries no sRPE load.
    let mut m2 = Model::default();
    app.update(
        track_event(
            equator_track(t0, &[(3000.0, 3.0)], 10),
            t0 + 1000,
            9_002,
            None,
        ),
        &mut m2,
    )
    .expect_only_render();
    assert_eq!(app.view(&m2).runs[0].srpe_load_au, None);
}

#[test]
fn amend_run_meta_rates_and_tags_a_gps_run_in_place() {
    let app = Engine;
    let mut model = Model::default();
    let t0 = 700 * DAY_SEC;
    app.update(
        track_event(
            equator_track(t0, &[(3000.0, 3.0)], 10),
            t0 + 1000,
            9_003,
            None,
        ),
        &mut model,
    )
    .expect_only_render();
    let before = app.view(&model).runs[0].clone();
    app.update(
        Event::AmendRunMeta {
            entry_id: 9_003,
            observed_at_fallback: t0 + 1000,
            session_rpe: Some(12),
            workout_type: Some(WorkoutType::Interval),
        },
        &mut model,
    )
    .expect_only_render();
    let after = app.view(&model).runs[0].clone();
    assert_eq!(after.session_rpe, Some(10), "CR-10 clamps at 10");
    assert_eq!(after.workout_type, Some(WorkoutType::Interval));
    assert!(after.has_gpx, "the GPS track is untouched");
    assert_eq!(after.distance_km, before.distance_km);
    assert_eq!(after.entry_id, before.entry_id);
    // A miss is a no-op.
    app.update(
        Event::AmendRunMeta {
            entry_id: 1,
            observed_at_fallback: 0,
            session_rpe: Some(3),
            workout_type: None,
        },
        &mut model,
    )
    .expect_only_render();
    assert_eq!(app.view(&model).runs[0].session_rpe, Some(10));
}

#[test]
fn best_effort_5k_is_found_inside_a_10k_gps_run() {
    // 5 km at 5:00 /km, then 5 km at 4:00 /km: the fastest 5K is the second
    // half (≈ 20:00), measured from the splits of the longer run.
    let app = Engine;
    let mut model = Model::default();
    let t0 = 700 * DAY_SEC;
    let pts = equator_track(
        t0,
        &[(5000.0, 1000.0 / 300.0), (5000.0, 1000.0 / 240.0)],
        10,
    );
    let end = pts.last().unwrap().observed_at;
    app.update(track_event(pts, end, 9_004, None), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    let p = vm
        .run_progress
        .expect("a dated run opens the progress view");
    let five = p
        .best_efforts
        .iter()
        .find(|b| b.distance_m == 5000)
        .expect("5K best effort");
    // 1200 s is the faster second half (240 s/km), not the whole run's pace.
    assert!((five.time_sec as i64 - 1200).abs() <= 3, "{five:?}");
    assert_eq!(five.entry_id, 9_004);
    let one = p
        .best_efforts
        .iter()
        .find(|b| b.distance_m == 1000)
        .unwrap();
    assert!((one.time_sec as i64 - 240).abs() <= 2, "{one:?}");
    assert!(
        vm.runs[0]
            .best_effort_labels
            .contains(&"Fastest 5K".to_string()),
        "{:?}",
        vm.runs[0].best_effort_labels
    );
    // Weekly series: the whole progress window, the run in the last week.
    assert_eq!(p.weekly.len(), PROGRESS_WEEKS as usize);
    let last = p.weekly.last().unwrap();
    assert_eq!(last.runs, 1);
    assert!((last.km - 10.0).abs() < 0.2, "{last:?}");
    assert!((p.longest_run_km - 10.0).abs() < 0.2);
}

#[test]
fn whole_hand_logged_5k_counts_and_a_faster_one_takes_the_record() {
    let app = Engine;
    let mut model = Model::default();
    app.update(log_run(5.0, 25.0, 0.0, 50.0, 700 * DAY_SEC, 1), &mut model)
        .expect_only_render();
    app.update(log_run(5.0, 24.0, 0.0, 50.0, 702 * DAY_SEC, 2), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    let five = vm
        .run_progress
        .unwrap()
        .best_efforts
        .into_iter()
        .find(|b| b.distance_m == 5000)
        .unwrap();
    assert_eq!(five.time_sec, 1440);
    assert_eq!(five.entry_id, 2);
    assert_eq!(five.time_label, "24:00");
}

#[test]
fn lift_e1rm_pr_marks_only_a_new_best() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        log_set_id("Back Squat", 100.0, 1, 700 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set_id("Back Squat", 110.0, 2, 701 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set_id("Back Squat", 105.0, 3, 702 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    let lifts = app.view(&model).lifts;
    let pr: Vec<bool> = lifts.iter().map(|l| l.e1rm_pr).collect();
    assert_eq!(pr, vec![false, true, false]);
}

/// A Turkish keyboard's "İNCLİNE BENCH PRESS" and "Inclıne bench press" continue
/// the e1RM chain of "Incline bench press" (`names::exercise_key`): the heavier
/// set is a PR over the ASCII spelling's, the lighter one is not.
#[test]
fn turkish_keyboard_spellings_continue_the_e1rm_chain() {
    let app = Engine;
    let mut model = Model::default();
    for (i, (name, kg)) in [
        ("Incline bench press", 100.0),
        ("İNCLİNE BENCH PRESS", 110.0),
        ("Inclıne bench press", 105.0),
    ]
    .into_iter()
    .enumerate()
    {
        app.update(
            log_set_id(name, kg, i as u64 + 1, (700 + i as i64) * DAY_SEC),
            &mut model,
        )
        .expect_only_render();
    }
    let pr: Vec<bool> = app.view(&model).lifts.iter().map(|l| l.e1rm_pr).collect();
    assert_eq!(pr, vec![false, true, false]);
}

fn heavy_squat_at(at: i64) -> Event {
    log_set("Barbell back squat", 120.0, 5, 8.0, at)
}

fn tagged_run(km: f64, minutes: f64, hr: f64, wt: Option<WorkoutType>, at: i64) -> Event {
    with_run_meta(log_run_at(km, minutes, hr, at), wt, None, None)
}

const CAP3_WARN: &str = "keep ≥24 h between";
const CAP3_SAME_DAY_NOTE: &str = "that gap works";

// 700 is a Thursday; 704 the next Monday. Weeks below start at 704.
const W: i64 = 704 * DAY_SEC;

#[test]
fn tagged_interval_without_hr_counts_toward_back_to_back_check() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        tagged_run(6.0, 30.0, 0.0, Some(WorkoutType::Interval), W + 8 * 3600),
        &mut model,
    )
    .expect_only_render();
    app.update(
        tagged_run(
            6.0,
            30.0,
            0.0,
            Some(WorkoutType::Interval),
            W + DAY_SEC + 8 * 3600,
        ),
        &mut model,
    )
    .expect_only_render();
    let report = app.view(&model).weekly_report;
    assert!(
        report
            .iter()
            .any(|r| r.summary.contains("2 hard sessions this week")
                && r.summary.contains("no back-to-back hard days")),
        "two HR-less tagged intervals on consecutive days are flagged: {report:#?}"
    );
}

#[test]
fn untagged_run_at_threshold_pace_counts_as_hard_with_a_race_anchor() {
    // No HR, no tag: a 10 km run at 4:50 /km is faster than a VDOT-40 runner's
    // threshold band, so with a fresh race anchor it counts as hard.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(racer_profile(705)), &mut model)
        .expect_only_render();
    set_today(&app, &mut model, 705);
    app.update(tagged_run(10.0, 48.3, 0.0, None, W + 8 * 3600), &mut model)
        .expect_only_render();
    app.update(
        tagged_run(10.0, 48.3, 0.0, None, W + DAY_SEC + 8 * 3600),
        &mut model,
    )
    .expect_only_render();
    let report = app.view(&model).weekly_report;
    assert!(
        report
            .iter()
            .any(|r| r.summary.contains("2 hard sessions this week")),
        "{report:#?}"
    );
}

#[test]
fn cap3_flags_tagged_tempo_day_after_heavy_squat() {
    let app = Engine;
    let mut model = Model::default();
    // Mon 18:00 heavy squats; Tue 08:00 a 5 km tagged tempo (no HR, NOT the
    // week's long run); Sat a 15 km long run, far from the squats.
    app.update(heavy_squat_at(W + 18 * 3600), &mut model)
        .expect_only_render();
    app.update(
        tagged_run(
            5.0,
            25.0,
            0.0,
            Some(WorkoutType::Tempo),
            W + DAY_SEC + 8 * 3600,
        ),
        &mut model,
    )
    .expect_only_render();
    app.update(
        tagged_run(15.0, 90.0, 70.0, None, W + 5 * DAY_SEC + 8 * 3600),
        &mut model,
    )
    .expect_only_render();
    let report = app.view(&model).weekly_report;
    let row = report
        .iter()
        .find(|r| r.summary.contains(CAP3_WARN))
        .unwrap_or_else(|| panic!("tempo 14 h after heavy squats is flagged: {report:#?}"));
    assert!(row.summary.contains("14 h apart"), "{}", row.summary);
}

#[test]
fn same_day_heavy_squat_then_long_run_2h_later_is_flagged() {
    // Lift-first is the right same-day ORDER, not an exemption from CAP-3.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    app.update(heavy_squat_at(W + 7 * 3600), &mut model)
        .expect_only_render();
    app.update(
        tagged_run(18.0, 108.0, 72.0, None, W + 9 * 3600),
        &mut model,
    )
    .expect_only_render();
    let report = app.view(&model).weekly_report;
    let row = report
        .iter()
        .find(|r| r.summary.contains(CAP3_WARN))
        .unwrap_or_else(|| panic!("same-day lift then long run 2 h later: {report:#?}"));
    assert!(row.summary.contains("2 h apart"), "{}", row.summary);
}

#[test]
fn same_day_7h_gap_gets_spacing_note_not_cap3_warning() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    app.update(heavy_squat_at(W + 7 * 3600), &mut model)
        .expect_only_render();
    app.update(
        tagged_run(18.0, 108.0, 72.0, None, W + 14 * 3600),
        &mut model,
    )
    .expect_only_render();
    let report = app.view(&model).weekly_report;
    assert!(
        !report.iter().any(|r| r.summary.contains(CAP3_WARN)),
        "a ≥6 h same-day gap is not the CAP-3 warning: {report:#?}"
    );
    let note = report
        .iter()
        .find(|r| r.summary.contains(CAP3_SAME_DAY_NOTE))
        .unwrap_or_else(|| panic!("same-day spacing note expected: {report:#?}"));
    assert_eq!(
        note.evidence.citation,
        Recommended::new((), claims::CONC_SEP_001).reference()
    );
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
        !report.iter().any(|r| r.summary.contains(CAP3_WARN)),
        "bodyweight air squat is not heavy leg work: {report:#?}"
    );
}

#[test]
fn light_leg_work_and_a_short_run_raise_no_gap_flag() {
    // The heavy-leg ↔ long-run guard fires only on squat/hinge work at ≥80%
    // 1RM. Light calf work (not a squat/hinge pattern) next to a short run is
    // not a heavy-leg/long-run collision.
    let app = Engine;
    let mut model = Model::default();
    let noon = W + 12 * 3600;
    for i in 0..3 {
        app.update(
            log_set("Calf raise", 100.0, 15, 8.0, noon + i * 60),
            &mut model,
        )
        .expect_only_render();
    }
    app.update(log_run_at(3.0, 18.0, 70.0, noon + 10 * 3600), &mut model)
        .expect_only_render();
    let report = app.view(&model).weekly_report;
    assert!(
        !report.iter().any(|r| r.summary.contains(CAP3_WARN)),
        "light calf work + a short run must not raise a spacing flag: {report:#?}"
    );
}

// A long run is the week's LONGEST run over an external full-week basis.

/// A 3-day, 17 km/wk endurance-priority hybrid.
fn hybrid_17km_profile() -> Profile {
    Profile {
        weekly_sets: 4,
        running_days_per_week: 3,
        running_km_per_week: 17.0,
        ..sample_profile()
    }
}

/// A 3 × 150 kg @ RPE 9 back squat at `at`.
fn heavy_squat(at: i64) -> Event {
    log_set("Back Squat", 150.0, 3, 9.0, at)
}

/// A 3-run week (easy 5 / tempo 4 / long 8 km): only the 8 km run is the long
/// run. Mondays start local weeks (day ≡ 4 mod 7), so days 4–10 are one week.
fn three_run_week() -> [Event; 3] {
    [(5.0, 5), (4.0, 6), (8.0, 9)] // Tue easy, Wed tempo, Sat long
        .map(|(km, day)| logrun_at(km, day * DAY_SEC + 8 * 3600))
}

/// Whether the weekly report raises the CAP-3 heavy-leg/long-run flag after
/// `profile` is set and `history` logged.
fn cap3_flagged(profile: Profile, history: impl IntoIterator<Item = Event>) -> bool {
    let events: Vec<Event> = std::iter::once(Event::SetProfile(profile))
        .chain(history)
        .collect();
    let (app, model) = replay(&events);
    app.view(&model)
        .weekly_report
        .iter()
        .any(|r| r.summary.contains(CAP3_WARN))
}

#[test]
fn easy_run_near_leg_day_is_not_treated_as_a_long_run() {
    // A heavy squat Mon 20:00 and the easy 5 km Tue 08:00 are ~12 h apart, but
    // the 5 km run is 29% of a partial-week total only — it is NOT the week's
    // longest run, so it must not trip the heavy-leg/long-run CAP-3 spacing flag.
    // The actual long run (8 km, Sat) is 5 days from the squat, well clear.
    let squat = heavy_squat(4 * DAY_SEC + 20 * 3600); // Mon evening
    assert!(
        !cap3_flagged(
            hybrid_17km_profile(),
            three_run_week().into_iter().chain([squat])
        ),
        "an easy 5 km run near leg day is not a long run"
    );
}

#[test]
fn heavy_squat_close_to_the_actual_long_run_still_flags() {
    // Positive control: move the heavy squat to Fri 20:00, ~12 h before the 8 km
    // long run on Sat 08:00. The 8 km run IS the week's longest and clears 25% of
    // the 17 km full-week basis, so CAP-3 must fire.
    let squat = heavy_squat(8 * DAY_SEC + 20 * 3600); // Fri evening
    assert!(
        cap3_flagged(
            hybrid_17km_profile(),
            three_run_week().into_iter().chain([squat])
        ),
        "a heavy squat ~12 h from the long run flags"
    );
}

#[test]
fn a_pure_lifters_lone_jog_after_squats_is_not_a_long_run() {
    // A lifter with no planned mileage logs a single 3 km recovery jog the morning
    // after heavy squats. With no external basis (no plan, no other runs in the
    // trailing 7 days) the jog cannot certify itself as a long run, so CAP-3 stays
    // silent instead of flagging the recovery jog.
    let history = [
        heavy_squat(4 * DAY_SEC + 20 * 3600), // Mon evening, ~12 h prior
        logrun_at(3.0, 5 * DAY_SEC + 8 * 3600), // Tue morning
    ];
    assert!(
        !cap3_flagged(base_profile(), history),
        "a lone recovery jog with no external basis"
    );
}

fn manual_run_bpm(id: u64, avg_hr_bpm: Option<f64>) -> Event {
    with_run_meta(
        log_run(8.0, 45.0, 70.0, 0.0, 700 * DAY_SEC, id),
        None,
        None,
        avg_hr_bpm,
    )
}

fn hr_pct_after(profile: Profile, events: Vec<Event>) -> f64 {
    let app = Engine;
    let mut model = Model {
        profile_history: vec![profile],
        ..Default::default()
    };
    for e in events {
        app.update(e, &mut model).expect_only_render();
    }
    app.view(&model).runs[0].hr_pct_max
}

#[test]
fn manual_run_avg_hr_bpm_converts_to_pct_of_hrmax() {
    // RUN-HRMAX-001: an average HR in bpm is stored as % HRmax against
    // the measured max (152 / 190 = 80.0 %), else the Tanaka estimate for the
    // age (40 y → 208 − 28 = 180; 152 / 180 = 84.4 %). With neither known the
    // sent % stands, and without a bpm figure nothing is converted.
    let measured = Profile {
        measured_hr_max: Some(190.0),
        age_years: Some(40.0),
        ..running_profile()
    };
    let aged = Profile {
        measured_hr_max: None,
        age_years: Some(40.0),
        ..running_profile()
    };
    let unknown = Profile {
        measured_hr_max: None,
        age_years: None,
        ..running_profile()
    };
    let close = |got: f64, want: f64| assert!((got - want).abs() < 1e-9, "got {got}, want {want}");

    close(
        hr_pct_after(measured.clone(), vec![manual_run_bpm(1, Some(152.0))]),
        80.0,
    );
    close(
        hr_pct_after(aged, vec![manual_run_bpm(1, Some(152.0))]),
        84.4,
    );
    close(
        hr_pct_after(unknown, vec![manual_run_bpm(1, Some(152.0))]),
        70.0,
    );
    close(
        hr_pct_after(measured.clone(), vec![manual_run_bpm(1, None)]),
        70.0,
    );
    // An implausible reading is ignored rather than stored as a 5 % run.
    close(
        hr_pct_after(measured.clone(), vec![manual_run_bpm(1, Some(9.0))]),
        70.0,
    );

    // The amend path converts the same way (171 / 190 = 90.0 %).
    let amend = with_run_meta(
        amend_run(1, 8.0, 45.0, 0.0, 700 * DAY_SEC, 700 * DAY_SEC),
        None,
        None,
        Some(171.0),
    );
    close(
        hr_pct_after(measured, vec![manual_run_bpm(1, None), amend]),
        90.0,
    );
}

#[test]
fn hybrid_and_running_ramp_rows_agree_on_the_rebound_after_a_recovery_week() {
    // A lifting runner: the week after the PLANNED recovery week rebounds to
    // the load-week volume. The running-031 row ratios against the last load
    // week (0 %, within cap); the hybrid-021 row must judge the same baseline
    // and stay silent rather than calling the rebound a >10 % ramp.
    let app = Engine;
    let mut model = Model::default();
    let profile = sample_profile();
    assert!(profile.weekly_sets > 0, "a lifting (hybrid) profile");
    let monday = |w: i64| 704 + w * 7;
    let rec = (2_i64..20)
        .find(|&w| {
            crate::plan::planned_recovery_week(&PlanProfile::new(&profile), w as u32, monday(w))
        })
        .expect("the plan has a recovery week");
    assert!(!crate::plan::planned_recovery_week(
        &PlanProfile::new(&profile),
        (rec + 1) as u32,
        monday(rec + 1)
    ));
    app.update(Event::SetProfile(profile), &mut model)
        .expect_only_render();
    // A run every day so each week counts as completed and the plan advances
    // one week per calendar week (ADH-PROGRESS-001).
    for w in 0..=rec + 1 {
        let week_km = if w == rec { 28.0 } else { 40.0 };
        for d in 0..7 {
            app.update(logrun(week_km / 7.0, monday(w) + d), &mut model)
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
    set_today(&app, &mut model, monday(rec + 1) + 6);
    let report = app.view(&model).weekly_report;
    let has = |needle: &str| report.iter().any(|r| r.summary.contains(needle));
    assert!(
        has("within the increase cap") && has("last week was the planned lighter week"),
        "running row judges the rebound against the last load week: {report:#?}"
    );
    assert!(
        !has("keep the weekly increase to about 10% or less"),
        "hybrid row must not contradict the running row: {report:#?}"
    );

    // Without a plan neither row knows about the recovery week: both flag the
    // +43 % rebound, still in agreement.
    app.update(Event::ClearPlan, &mut model)
        .expect_only_render();
    let report = app.view(&model).weekly_report;
    let has = |needle: &str| report.iter().any(|r| r.summary.contains(needle));
    assert!(has("over the increase cap"), "{report:#?}");
    assert!(
        has("keep the weekly increase to about 10% or less"),
        "{report:#?}"
    );
}

#[test]
fn trivial_gps_run_gets_no_pacing_verdict() {
    // A 0.12 mi (~193 m), one-minute run has no full km split, so there is
    // nothing to praise or fault about its pacing.
    let app = Engine;
    let mut model = Model::default();
    let t0 = 700 * DAY_SEC;
    let pts = equator_track(t0, &[(97.0, 3.4), (96.0, 3.0)], 5);
    let end = pts.last().unwrap().observed_at;
    assert!(end - t0 <= 65, "about a minute");
    app.update(track_event(pts, end, 9_101, None), &mut model)
        .expect_only_render();
    let run = &app.view(&model).runs[0];
    assert!(run.splits.iter().all(|s| s.partial), "{:?}", run.splits);
    assert!(run.split.is_none(), "no pacing verdict: {:?}", run.split);

    // A 3 km evenly paced run (3 full km) does get one.
    let mut model = Model::default();
    let pts = equator_track(t0, &[(3000.0, 3.0)], 10);
    let end = pts.last().unwrap().observed_at;
    app.update(track_event(pts, end, 9_102, None), &mut model)
        .expect_only_render();
    let run = &app.view(&model).runs[0];
    let verdict = run.split.as_ref().expect("a verdict on a 3 km run");
    assert_eq!(verdict.verdict, SplitVerdict::Even);
}

fn rated_bpm_run(avg_hr_bpm: Option<f64>, hr_pct_max: f64, session_rpe: Option<u8>) -> Event {
    with_run_meta(
        log_run(8.0, 45.0, hr_pct_max, 0.0, 700 * DAY_SEC, 1),
        None,
        session_rpe,
        avg_hr_bpm,
    )
}

fn run_view_after(profile: Profile, event: Event) -> RunResultView {
    let app = Engine;
    let mut model = Model {
        profile_history: vec![profile],
        ..Default::default()
    };
    app.update(event, &mut model).expect_only_render();
    app.view(&model).runs[0].clone()
}

#[test]
fn run_card_hr_line_shows_entered_bpm_and_never_labels_a_hard_effort_z1() {
    // 148 bpm against a measured 190 max = 77.9 %HRmax, Z1 (<82 %).
    let measured = Profile {
        measured_hr_max: Some(190.0),
        ..running_profile()
    };
    let easy = run_view_after(measured.clone(), rated_bpm_run(Some(148.0), 0.0, None));
    assert_eq!(easy.zone, Some(ThreeZone::Z1));
    assert_eq!(easy.hr_label, "148 bpm · Z1");

    // Rated 7/10, at or above the tempo floor (running-018, RPE 6–7): a Z1
    // label would contradict the athlete's own rating, so it is left off.
    let hard = run_view_after(measured.clone(), rated_bpm_run(Some(148.0), 0.0, Some(7)));
    assert_eq!(hard.zone, None);
    assert_eq!(hard.hr_label, "148 bpm");
    // Below the tempo floor the zone stands.
    let moderate = run_view_after(measured.clone(), rated_bpm_run(Some(148.0), 0.0, Some(5)));
    assert_eq!(moderate.zone, Some(ThreeZone::Z1));

    // No bpm entered: the %HRmax as a whole percent.
    let pct = run_view_after(measured.clone(), rated_bpm_run(None, 80.3, None));
    assert_eq!(pct.hr_label, "HR 80% · Z1");
    // No heart rate at all: no line.
    let none = run_view_after(measured, rated_bpm_run(None, 0.0, None));
    assert_eq!(none.hr_label, "");
}

#[test]
fn race_labels_name_the_classic_distances() {
    assert_eq!(race_distance_label(21.0975), "half marathon");
    assert_eq!(race_distance_label(21.1), "half marathon");
    assert_eq!(race_distance_label(42.195), "marathon");
    assert_eq!(race_distance_label(42.2), "marathon");
    assert_eq!(race_distance_label(10.0), "10K");
    assert_eq!(race_distance_label(5.0), "5K");
    assert_eq!(race_distance_label(15.0), "15 km");
    assert_eq!(race_distance_label(7.5), "7.5 km");
    assert_eq!(race_distance_title(21.0975), "Half marathon");
}

#[test]
fn an_undated_race_is_dated_when_stated_and_stops_setting_paces_after_eight_weeks() {
    let app = Engine;
    let mut model = Model::default();
    let today = 704;
    set_today(&app, &mut model, today);
    // The shell stamps every profile edit effective today; the race is dated
    // with that day (an undated legacy edit has no day to date it with).
    let mut p = running_profile();
    p.effective_from_epoch_day = Some(today);
    p.recent_race = Some(RecentRace {
        distance_km: 10.0,
        time_sec: 3000,
        epoch_day: None,
    });
    app.update(Event::SetProfile(p.clone()), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: today,
        },
        &mut model,
    )
    .expect_only_render();
    let race_day = |model: &Model| {
        app.view(model)
            .profile
            .and_then(|p| p.recent_race)
            .expect("the stated race")
            .epoch_day
    };
    assert_eq!(
        race_day(&model),
        Some(today),
        "an undated race is dated when first stated"
    );
    assert!(long_run_paced(&model));

    // Re-saving the profile with the same race keeps its first date, so the
    // race still goes stale.
    let later = today + 9 * 7;
    set_today(&app, &mut model, later);
    p.effective_from_epoch_day = Some(later);
    app.update(Event::SetProfile(p.clone()), &mut model)
        .expect_only_render();
    assert_eq!(race_day(&model), Some(today));
    assert!(
        !long_run_paced(&model),
        "a 9-week-old race no longer sets paces"
    );

    // A new result is dated afresh.
    p.recent_race = Some(RecentRace {
        distance_km: 10.0,
        time_sec: 2900,
        epoch_day: None,
    });
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    assert_eq!(race_day(&model), Some(later));
    assert!(long_run_paced(&model));
}

/// Whether the plan week's long run shows a pace (a race anchors paces).
fn long_run_paced(model: &Model) -> bool {
    !planned_long_run_item(model)
        .run_target
        .expect("run items carry a target")
        .pace_label
        .is_empty()
}

#[test]
fn an_undated_race_in_an_undated_profile_edit_stays_undated() {
    // A legacy edit (no effective day) gives the race no day to be dated
    // with: the clock is not read, so a compacted replay (which keeps only
    // the last SetToday) agrees, and the race of unknown age sets no paces.
    let mut p = racer_profile(MON);
    p.effective_from_epoch_day = None;
    p.recent_race.as_mut().unwrap().epoch_day = None;
    let (app, model) = planned_on(p, [], MON);
    let race = app.view(&model).profile.and_then(|p| p.recent_race);
    assert_eq!(race.expect("the stated race").epoch_day, None);
    assert!(!long_run_paced(&model));
}

#[test]
fn a_pre_layoff_race_is_not_a_pace_anchor() {
    // Returning from 4 weeks off: a race run before the return no longer
    // reflects current fitness, even inside the 8-week window.
    let returning = |race_day: i64| {
        let mut p = racer_profile(MON);
        p.weeks_off = Some(4.0);
        p.layoff_returned_epoch_day = Some(MON - 3);
        p.recent_race.as_mut().unwrap().epoch_day = Some(race_day);
        planned_on(p, [], MON).1
    };
    assert!(!long_run_paced(&returning(MON - 7)));
    assert!(long_run_paced(&returning(MON - 1)));
}

#[test]
fn an_unset_weekly_distance_is_stored_as_unset_never_as_a_figure() {
    let app = Engine;
    let mut model = Model::default();
    let mut p = running_profile();
    p.running_km_per_week = 24.0;
    p.running_km_per_week_set = false;
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    let stored = model.profile().unwrap();
    assert_eq!(stored.running_km_per_week, 0.0);
    assert!(!stored.running_km_per_week_set);
    let vm = app.view(&model);
    assert!(!vm.profile.as_ref().unwrap().running_km_per_week_set);
    // Old profiles (no field on the wire) read as answered.
    let wire = r#"{"progression_cadence":"WeekToWeek","lift_goal":"MaxStrength","goal_distance":"TenK","concurrent_goal":"Strength","weekly_sets":14,"running_days_per_week":4,"running_km_per_week":45.0,"advanced":false,"endurance_intensity_pct_vo2max":75.0}"#;
    let old: Profile = serde_json::from_str(wire).expect("old profile parses");
    assert!(old.running_km_per_week_set);
}

fn bpm_model() -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    let _ = app.update(
        Event::SetProfile(Profile {
            age_years: Some(34.0),
            ..running_profile()
        }),
        &mut model,
    );
    (app, model)
}

#[test]
fn deleting_then_re_logging_a_bpm_run_from_its_view_is_identical() {
    // Undo re-sends the run from its view: the entered bpm must survive, so
    // the card stays "152 bpm · Z2" and never turns into "HR 82% · Z2".
    let (app, mut model) = bpm_model();
    let _ = app.update(
        with_run_meta(
            log_run(8.0, 48.0, 0.0, 0.0, 100 * DAY_SEC, 7),
            None,
            Some(5),
            Some(152.0),
        ),
        &mut model,
    );
    let before = app.view(&model).runs[0].clone();
    assert_eq!(before.avg_hr_bpm, Some(152.0));
    assert!(
        before.hr_label.starts_with("152 bpm"),
        "{}",
        before.hr_label
    );
    let _ = app.update(
        Event::DeleteEntry {
            kind: EntryKind::Run,
            entry_id: 7,
            observed_at_fallback: 0,
        },
        &mut model,
    );
    assert!(app.view(&model).runs.is_empty());
    let _ = app.update(
        with_run_meta(
            log_run(
                before.distance_km,
                before.duration_min,
                before.hr_pct_max,
                0.0,
                before.observed_at,
                before.entry_id,
            ),
            before.workout_type,
            before.session_rpe,
            before.avg_hr_bpm,
        ),
        &mut model,
    );
    assert_eq!(app.view(&model).runs[0], before);
}

#[test]
fn a_tracked_run_keeps_its_entered_bpm() {
    let (app, mut model) = bpm_model();
    let t0 = 100 * DAY_SEC;
    let ev = with_run_meta(
        log_run_track(equator_track(t0, &[(4000.0, 3.0)], 5), 0.0, 50.0, t0, 9),
        None,
        None,
        Some(150.0),
    );
    let _ = app.update(ev, &mut model);
    let run = &app.view(&model).runs[0];
    assert_eq!(run.avg_hr_bpm, Some(150.0));
    assert!(run.hr_label.starts_with("150 bpm"), "{}", run.hr_label);
    assert!(run.hr_pct_max > 0.0, "the %HRmax is derived from the bpm");
}

#[test]
fn gps_qc_drops_teleport_and_stuck_time_fixes() {
    let app = Engine;
    let mut model = Model::default();
    let pts = vec![
        gps(0.000, 0),
        gps(0.001, 20),
        // Teleport: ~11 km implied in 1 s (>12 m/s), dropped.
        gps(0.100, 21),
        gps(0.002, 40),
        // Non-advancing timestamp, dropped.
        gps(0.0025, 40),
        gps(0.003, 60),
    ];
    assert_eq!(
        pts.len() - crate::gps::qc_track(&pts, &[]).track.len(),
        2,
        "teleport + stuck-time fixes dropped"
    );
    app.update(log_run_track(pts, 78.0, 12.0, 0, 0), &mut model)
        .expect_only_render();

    let run = &app.view(&model).runs[0];
    // Distance is the clean 3-hop track (~0.33 km), not an 11 km teleport.
    assert!(
        run.distance_km > 0.2 && run.distance_km < 0.5,
        "QC'd distance, got {}",
        run.distance_km
    );
}

#[test]
fn gps_timestamp_arithmetic_saturates_without_panicking() {
    // A track carrying i64::MAX / i64::MIN stamps must not overflow
    // the dt subtraction (a debug panic) nor corrupt distance. The saturated
    // dt is then discarded/handled by the existing dt gates.
    let pts = vec![gps(0.000, i64::MIN), gps(0.001, i64::MAX), gps(0.002, 10)];
    let qc = crate::gps::qc_track(&pts, &[]);
    let dist = crate::gps::track_distance_km(&qc.track, &qc.seg_starts);
    assert!(
        dist.is_finite() && dist >= 0.0,
        "distance stays finite: {dist}"
    );
    let moving = crate::gps::moving_duration_min(&qc);
    assert!(
        moving.is_finite() && moving >= 0.0,
        "moving time stays finite: {moving}"
    );
}

#[test]
fn a_short_stop_swallowed_by_the_jitter_gate_is_not_moving_time() {
    // 1 Hz fixes at 3 m/s, a 6 s stand (fixes within 0.5 m,
    // dropped by the <2.5 m gate), then running on. The merged leg spans 7 s
    // for ~3.5 m (0.5 m/s, just over the auto-pause floor), so the stand was
    // counted as running; only the 1 s after the last standing fix moves.
    let m_per_deg = crate::gps::haversine_m(gps(0.0, 0), gps(1.0, 0));
    let at = |m: f64, t: i64| gps(m / m_per_deg, t);
    let mut pts: Vec<GpsPoint> = (0..=20).map(|s| at(3.0 * s as f64, s)).collect();
    let stop = 60.0;
    for s in 1..=6 {
        pts.push(at(stop + if s % 2 == 0 { 0.4 } else { 0.1 }, 20 + s));
    }
    for s in 1..=20 {
        pts.push(at(stop + 3.5 + 3.0 * (s - 1) as f64, 26 + s));
    }
    let qc = crate::gps::qc_track(&pts, &[]);
    assert_eq!(
        pts.len() - qc.track.len(),
        5,
        "the standing fixes are jitter-dropped, but the last, where the runner stood until, is kept"
    );
    let moving_sec = crate::gps::moving_duration_min(&qc) * 60.0;
    assert!(
        (moving_sec - 40.0).abs() < 1e-9,
        "20 s + 20 s of running, not the 6 s stand: {moving_sec}"
    );
}
