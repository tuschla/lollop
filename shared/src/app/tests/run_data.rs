//! Run data: numeric run targets, session-RPE load and run meta edits, run
//! progress / best efforts / plan labels, GPS QC and moving time, tag- and
//! pace-aware hard-run spacing, and CAP-3 heavy-leg spacing in both
//! directions.

use super::*;

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

/// A run prescription through the wire shape (new optional structure fields
/// default), so the test does not depend on the struct's full field list.
fn run_rx(
    volume: serde_json::Value,
    intensity: serde_json::Value,
    claim: ClaimId,
) -> Recommended<Prescription> {
    let rx: crate::schema::RunPrescription = serde_json::from_value(serde_json::json!({
        "volume": volume,
        "intensity": intensity,
        "repeats": null,
    }))
    .expect("run prescription wire shape");
    graded(Prescription::Run(rx), claim)
}

fn flatten_with_targets(
    kind: RunSessionType,
    rx: Recommended<Prescription>,
    anchors: &crate::plan::Anchors,
    model: &Model,
    day: i64,
) -> PrescriptionView {
    let session = Session {
        session_type: SessionType::Run(kind),
        day: 0,
        prescriptions: vec![rx],
        dup_day: None,
    };
    let mut items: Vec<PrescriptionView> = session
        .prescriptions
        .iter()
        .map(|rx| flatten_prescription(rx, anchors, None, false))
        .collect();
    super::super::run_targets::apply_run_targets(&mut items, &session, anchors, model, day);
    items.remove(0)
}

#[test]
fn threshold_run_renders_pace_range_from_a_recent_10k() {
    let today = 704;
    let model = Model {
        profile: Some(racer_profile(today)),
        ..Default::default()
    };
    let vdot = super::super::run_targets::pace_anchor_vdot(model.profile.as_ref().unwrap(), today)
        .expect("a 1-week-old 10K is a fresh pace anchor");
    assert!(
        (vdot - 40.0).abs() < 0.2,
        "10K in 50:00 ≈ VDOT 40, got {vdot}"
    );
    let anchors = crate::plan::Anchors {
        run_vdot: Some(vdot),
        ..Default::default()
    };
    let rx = run_rx(
        serde_json::json!({"DistanceKm": 5.0}),
        serde_json::json!({"HrPercentMax": {"lo": 88.0, "hi": 92.0, "hrr_hi": null}}),
        claims::RUN_TEMPO_001,
    );
    let it = flatten_with_targets(RunSessionType::Tempo, rx, &anchors, &model, today);
    let t = it
        .run_target
        .clone()
        .expect("a tempo run carries a numeric target");
    assert_eq!(t.band, "threshold");
    // Daniels VDOT 40: T ≈ 5:06 /km (88 %VO2max) to ≈ 5:20 /km (83 %).
    assert!((303..=309).contains(&t.pace_fast_sec_per_km), "{t:?}");
    assert!((317..=323).contains(&t.pace_slow_sec_per_km), "{t:?}");
    let expect = format!(
        "{}:{:02}–{}:{:02} /km",
        t.pace_fast_sec_per_km / 60,
        t.pace_fast_sec_per_km % 60,
        t.pace_slow_sec_per_km / 60,
        t.pace_slow_sec_per_km % 60
    );
    assert_eq!(t.pace_label_km, expect);
    assert!(t.pace_label_mi.ends_with("/mi"), "{t:?}");
    assert!(
        it.summary.starts_with("5 km · ") && it.summary.contains(&expect),
        "the card reads the pace: {}",
        it.summary
    );
    assert!(
        it.intensity_label.contains("effort 6–7 of 10"),
        "{}",
        it.intensity_label
    );
    assert_eq!(t.citation, graded((), claims::RUN_VDOT_001).reference());
    // A stale race (>8 weeks) is not an anchor.
    let mut stale = racer_profile(today);
    stale.recent_race.as_mut().unwrap().epoch_day = Some(today - 70);
    assert_eq!(
        super::super::run_targets::pace_anchor_vdot(&stale, today),
        None
    );
}

#[test]
fn easy_run_without_anchors_shows_the_talk_test_cue() {
    // No race, no age, no measured HRmax: the long run has no pace or bpm, but
    // it still tells the runner how it should feel.
    let (app, model) = {
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
        (app, model)
    };
    let it = planned_long_run_item(&app, &model);
    let t = it.run_target.clone().expect("run items carry a target");
    assert!(t.pace_label_km.is_empty() && t.hr_label.is_empty(), "{t:?}");
    assert!(
        t.effort_cue.contains("talk in full sentences"),
        "talk-test cue without anchors: {t:?}"
    );
    assert!(
        it.intensity_label.contains("talk in full sentences"),
        "{}",
        it.intensity_label
    );
    assert_eq!(t.citation, graded((), claims::RUN_TALKTEST_001).reference());
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
    app.update(
        Event::SetToday {
            epoch_day: MON,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let it = planned_long_run_item(&app, &model);
    let t = it.run_target.clone().expect("run items carry a target");
    assert!(
        !t.pace_label_km.is_empty(),
        "race anchor → easy pace: {t:?}"
    );
    assert!(t.hr_high_bpm > 0 && t.hr_high_bpm <= 190, "{t:?}");
    assert!(t.hr_max_measured);
    assert!(
        !t.effort_cue.contains("talk"),
        "measured anchors replace the talk test: {t:?}"
    );
    assert!(
        it.intensity_label.starts_with(&t.pace_label_km),
        "{}",
        it.intensity_label
    );
}

#[test]
fn downgrade_re_resolves_the_easy_band() {
    let hr = super::super::run_targets::HrAnchor {
        max_bpm: 190.0,
        measured: true,
        rest_bpm: 0.0,
    };
    let tempo = crate::schema::RunIntensity::Vdot(VdotBand::Threshold);
    let t = super::super::run_targets::resolve_target(
        super::super::run_targets::TargetBand::Threshold,
        &tempo,
        Some(50.0),
        hr,
    );
    let mut it = PrescriptionView {
        intensity_label: "Easy: zone 1–2".into(),
        run_target: Some(t.clone()),
        ..Default::default()
    };
    super::super::run_targets::downgrade_run_target_to_easy(&mut it);
    let e = it.run_target.expect("still targeted");
    assert_eq!(e.band, "easy");
    assert!(
        e.pace_fast_sec_per_km > t.pace_slow_sec_per_km,
        "easy is slower: {e:?}"
    );
    assert!(
        it.intensity_label.starts_with("Easy: zone 1–2 · "),
        "{}",
        it.intensity_label
    );
}

/// A straight eastward GPS track along the equator: `(metres, seconds)` legs
/// sampled every `step_s` seconds, starting at `t0`.
fn equator_track(t0: i64, legs: &[(f64, f64)], step_s: i64) -> Vec<GpsPoint> {
    let origin = GpsPoint {
        lat: 0.0,
        lon: 0.0,
        observed_at: 0,
        accuracy_m: 5.0,
    };
    let m_per_deg = crate::running::haversine_m(origin, GpsPoint { lon: 1.0, ..origin });
    let mut pts = vec![GpsPoint {
        observed_at: t0,
        ..origin
    }];
    let (mut lon, mut t) = (0.0_f64, t0);
    for &(metres, speed) in legs {
        let n = (metres / speed / step_s as f64).round() as i64;
        let per = metres / n as f64;
        for _ in 0..n {
            lon += per / m_per_deg;
            t += step_s;
            pts.push(GpsPoint {
                lon,
                observed_at: t,
                ..origin
            });
        }
    }
    pts
}

fn track_event(points: Vec<GpsPoint>, at: i64, id: u64, rpe: Option<u8>) -> Event {
    Event::LogRunTrack {
        points,
        hr_pct_max: 0.0,
        longest_recent_km: 50.0,
        segment_starts: Vec::new(),
        observed_at: at,
        entry_id: id,
        workout_type: None,
        session_rpe: rpe,
        avg_hr_bpm: None,
    }
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
    assert_eq!(s.week_au, 280.0);
    assert_eq!(s.rated_runs, 1);
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
    assert!((five.time_sec as i64 - 1200).abs() <= 3, "{five:?}");
    assert!(five.from_split, "taken from a stretch of the 10K");
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
    // Weekly series: 16 weeks, the run in the last one.
    assert_eq!(p.weekly.len(), 16);
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
    assert!(!five.from_split);
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

// ── Hard-run spacing and CAP-3 ────────────

fn heavy_squat_at(at: i64) -> Event {
    log_set("Barbell back squat", 120.0, 5, 8.0, at)
}

fn tagged_run(km: f64, minutes: f64, hr: f64, wt: Option<WorkoutType>, at: i64) -> Event {
    Event::LogRun {
        distance_km: km,
        duration_min: minutes,
        hr_pct_max: hr,
        longest_recent_km: 50.0,
        observed_at: at,
        entry_id: 0,
        workout_type: wt,
        session_rpe: None,
        avg_hr_bpm: None,
    }
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
    app.update(
        Event::SetToday {
            epoch_day: 705,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
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
        graded((), claims::CONC_SEP_001).reference()
    );
}

fn manual_run_bpm(id: u64, avg_hr_bpm: Option<f64>) -> Event {
    Event::LogRun {
        distance_km: 8.0,
        duration_min: 45.0,
        hr_pct_max: 70.0,
        longest_recent_km: 0.0,
        observed_at: 700 * DAY_SEC,
        entry_id: id,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm,
    }
}

fn hr_pct_after(profile: Profile, events: Vec<Event>) -> f64 {
    let app = Engine;
    let mut model = Model {
        profile: Some(profile),
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
    let amend = Event::AmendRun {
        entry_id: 1,
        distance_km: 8.0,
        duration_min: 45.0,
        hr_pct_max: 0.0,
        longest_recent_km: 0.0,
        observed_at: 700 * DAY_SEC,
        observed_at_fallback: 700 * DAY_SEC,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: Some(171.0),
    };
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
        .find(|&w| crate::plan::planned_recovery_week(&profile, w as u32, monday(w)))
        .expect("the plan has a recovery week");
    assert!(!crate::plan::planned_recovery_week(
        &profile,
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
    app.update(
        Event::SetToday {
            epoch_day: monday(rec + 1) + 6,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
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
    assert!(
        run.splits_km.iter().all(|s| s.partial),
        "{:?}",
        run.splits_km
    );
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
    Event::LogRun {
        distance_km: 8.0,
        duration_min: 45.0,
        hr_pct_max,
        longest_recent_km: 0.0,
        observed_at: 700 * DAY_SEC,
        entry_id: 1,
        workout_type: None,
        session_rpe,
        avg_hr_bpm,
    }
}

fn run_view_after(profile: Profile, event: Event) -> RunResultView {
    let app = Engine;
    let mut model = Model {
        profile: Some(profile),
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
    let set_today = |model: &mut Model, epoch_day: i64| {
        app.update(
            Event::SetToday {
                epoch_day,
                utc_offset_sec: 0,
            },
            model,
        )
        .expect_only_render();
    };
    let today = 704;
    set_today(&mut model, today);
    let mut p = running_profile();
    p.recent_race = Some(RecentRace {
        distance_km: 10.0,
        time_sec: 3000,
        epoch_day: None,
    });
    app.update(Event::SetProfile(p.clone()), &mut model)
        .expect_only_render();
    let race_day = |model: &Model| {
        model
            .profile
            .as_ref()
            .unwrap()
            .recent_race
            .as_ref()
            .unwrap()
            .epoch_day
    };
    assert_eq!(
        race_day(&model),
        Some(today),
        "an undated race is dated when first stated"
    );
    let anchor = |model: &Model, day: i64| {
        super::super::run_targets::pace_anchor_vdot(model.profile.as_ref().unwrap(), day)
    };
    assert!(anchor(&model, today + 7).is_some());

    // Re-saving the profile with the same race keeps its first date, so the
    // race still goes stale.
    let later = today + 9 * 7;
    set_today(&mut model, later);
    app.update(Event::SetProfile(p.clone()), &mut model)
        .expect_only_render();
    assert_eq!(race_day(&model), Some(today));
    assert_eq!(
        anchor(&model, later),
        None,
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
    assert!(anchor(&model, later).is_some());
}

#[test]
fn an_undated_or_pre_layoff_race_is_not_a_pace_anchor() {
    let today = 704;
    let mut undated = racer_profile(today);
    undated.recent_race.as_mut().unwrap().epoch_day = None;
    assert_eq!(
        super::super::run_targets::pace_anchor_vdot(&undated, today),
        None
    );

    // Returning from 4 weeks off: a race run before the return no longer
    // reflects current fitness, even inside the 8-week window.
    let mut returning = racer_profile(today);
    returning.weeks_off = Some(4.0);
    returning.layoff_returned_epoch_day = Some(today - 3);
    assert_eq!(
        super::super::run_targets::pace_anchor_vdot(&returning, today),
        None
    );
    returning.recent_race.as_mut().unwrap().epoch_day = Some(today - 1);
    assert!(super::super::run_targets::pace_anchor_vdot(&returning, today).is_some());
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
    let stored = model.profile.as_ref().unwrap();
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
    // Undo re-sent the derived %HRmax without the bpm, so the
    // card turned "152 bpm · Z2" into "HR 82% · Z2".
    let (app, mut model) = bpm_model();
    let _ = app.update(
        Event::LogRun {
            distance_km: 8.0,
            duration_min: 48.0,
            hr_pct_max: 0.0,
            longest_recent_km: 0.0,
            observed_at: 100 * DAY_SEC,
            entry_id: 7,
            workout_type: None,
            session_rpe: Some(5),
            avg_hr_bpm: Some(152.0),
        },
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
        Event::LogRun {
            distance_km: before.distance_km,
            duration_min: before.duration_min,
            hr_pct_max: before.hr_pct_max,
            longest_recent_km: 0.0,
            observed_at: before.observed_at,
            entry_id: before.entry_id,
            workout_type: before.workout_type,
            session_rpe: before.session_rpe,
            avg_hr_bpm: before.avg_hr_bpm,
        },
        &mut model,
    );
    assert_eq!(app.view(&model).runs[0], before);
}

#[test]
fn a_tracked_run_keeps_its_entered_bpm() {
    let (app, mut model) = bpm_model();
    let t0 = 100 * DAY_SEC;
    let mut ev = track_event(equator_track(t0, &[(4000.0, 3.0)], 5), t0, 9, None);
    if let Event::LogRunTrack { avg_hr_bpm, .. } = &mut ev {
        *avg_hr_bpm = Some(150.0);
    }
    let _ = app.update(ev, &mut model);
    let run = &app.view(&model).runs[0];
    assert_eq!(run.avg_hr_bpm, Some(150.0));
    assert!(run.hr_label.starts_with("150 bpm"), "{}", run.hr_label);
    assert!(run.hr_pct_max > 0.0, "the %HRmax is derived from the bpm");
}

#[test]
fn gps_qc_drops_teleport_and_stuck_time_fixes_and_reports_the_count() {
    let app = Engine;
    let mut model = Model::default();
    let p = |lon: f64, at: i64| GpsPoint {
        lat: 0.0,
        lon,
        observed_at: at,
        accuracy_m: 5.0,
    };
    app.update(
        log_run_track(
            vec![
                p(0.000, 0),
                p(0.001, 20),
                // Teleport: ~11 km implied in 1 s (>12 m/s), dropped.
                p(0.100, 21),
                p(0.002, 40),
                // Non-advancing timestamp, dropped.
                p(0.0025, 40),
                p(0.003, 60),
            ],
            78.0,
            12.0,
            0,
            0,
        ),
        &mut model,
    )
    .expect_only_render();

    let run = &app.view(&model).runs[0];
    assert_eq!(run.qc_dropped, 2, "teleport + stuck-time fixes dropped");
    // Distance is the clean 3-hop track (~0.33 km), not an 11 km teleport.
    assert!(
        run.distance_km > 0.2 && run.distance_km < 0.5,
        "QC'd distance, got {}",
        run.distance_km
    );
    // Manual runs report zero dropped fixes.
    app.update(log_run_at(5.0, 30.0, 75.0, 0), &mut model)
        .expect_only_render();
    assert_eq!(app.view(&model).runs[1].qc_dropped, 0);
}

#[test]
fn gps_timestamp_arithmetic_saturates_without_panicking() {
    // A track carrying i64::MAX / i64::MIN stamps must not overflow
    // the dt subtraction (a debug panic) nor corrupt distance. The saturated
    // dt is then discarded/handled by the existing dt gates.
    let pts = vec![
        GpsPoint {
            lat: 0.0,
            lon: 0.000,
            observed_at: i64::MIN,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.001,
            observed_at: i64::MAX,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 0.002,
            observed_at: 10,
            accuracy_m: 5.0,
        },
    ];
    let qc = qc_track(&pts, &[]);
    let dist =
        running::track_distance_km_seg(&qc.track, running::MAX_GPS_ACCURACY_M, &qc.seg_starts);
    assert!(
        dist.is_finite() && dist >= 0.0,
        "distance stays finite: {dist}"
    );
    let moving = moving_duration_min(&qc);
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
    let m_per_deg = running::haversine_m(
        GpsPoint {
            lat: 0.0,
            lon: 0.0,
            observed_at: 0,
            accuracy_m: 5.0,
        },
        GpsPoint {
            lat: 0.0,
            lon: 1.0,
            observed_at: 0,
            accuracy_m: 5.0,
        },
    );
    let at = |m: f64, t: i64| GpsPoint {
        lat: 0.0,
        lon: m / m_per_deg,
        observed_at: t,
        accuracy_m: 5.0,
    };
    let mut pts: Vec<GpsPoint> = (0..=20).map(|s| at(3.0 * s as f64, s)).collect();
    let stop = 60.0;
    for s in 1..=6 {
        pts.push(at(stop + if s % 2 == 0 { 0.4 } else { 0.1 }, 20 + s));
    }
    for s in 1..=20 {
        pts.push(at(stop + 3.5 + 3.0 * (s - 1) as f64, 26 + s));
    }
    let qc = qc_track(&pts, &[]);
    assert_eq!(qc.dropped, 6, "the standing fixes are jitter-dropped");
    let moving_sec = moving_duration_min(&qc) * 60.0;
    assert!(
        (moving_sec - 40.0).abs() < 1e-9,
        "20 s + 20 s of running, not the 6 s stand: {moving_sec}"
    );
}
