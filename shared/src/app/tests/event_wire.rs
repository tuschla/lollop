//! Event and view wire compatibility: the shell's JSON forms and older
//! persisted logs deserialize and replay; additive fields default.

use super::*;

/// The Android shell hand-serializes each [`Event`] to serde's external
/// tagging (`Core.kt::toJson`) and omits every absent optional field. Replay
/// feeds those exact bytes back through `update`, and the FFI *swallows*
/// deserialize errors (`ffi::process_event` clears the output on failure), so
/// a drift between Kotlin's wire form and this enum's `Deserialize` would
/// silently drop the event on every cold start. Pin the omit-optionals form.
#[test]
fn android_wire_form_deserializes_with_optionals_omitted() {
    // Exactly what Core.kt emits for a review with no run/lift context: only
    // the four non-optional booleans/count, every Option field absent.
    let bare = r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"bad_day":false}}"#;
    match serde_json::from_str::<Event>(bare).expect("bare review must parse") {
        Event::SubmitReview(r) => assert_eq!(
            r,
            SessionReview::default(),
            "omitted optionals must deserialize to None/default"
        ),
        other => panic!("expected SubmitReview, got {other:?}"),
    }

    // SetProfile carries no optionals, but pin its snake_case field contract
    // too, a renamed field here silently drops the profile on replay. The
    // retired `endurance_intensity_pct_vo2max` key is deliberate: older logs
    // still carry it, and an unknown key must not break replay.
    let profile = r#"{"SetProfile":{"progression_cadence":"WeekToWeek","lift_goal":"MaxStrength","goal_distance":"TenK","concurrent_goal":"Strength","weekly_sets":14,"running_days_per_week":4,"running_km_per_week":45.0,"advanced":false,"endurance_intensity_pct_vo2max":75.0}}"#;
    match serde_json::from_str::<Event>(profile).expect("profile wire form must parse") {
        Event::SetProfile(p) => {
            // The person fields are additive: an older profile line
            // (nine fields, no person data) must replay with them absent.
            assert_eq!(p.age_years, None, "old profile must default age to None");
            assert_eq!(p.birth_epoch_day, None);
            assert_eq!(p.resting_hr_bpm, None);
            assert_eq!(p.measured_hr_max, None);
            assert_eq!(p.bodyweight_kg, None);
            assert_eq!(p.female, None);
            // weight_lb is additive too: an old profile line (no field)
            // replays metric.
            assert!(!p.weight_lb, "old profile must default weight_lb to kg");
            // The scheduling / race / layoff / equipment fields are additive
            // too: an old profile line replays with every one absent.
            assert_eq!(p.lift_days_per_week, None);
            assert_eq!(p.training_weekdays, None);
            assert_eq!(p.goal_race_epoch_day, None);
            assert_eq!(p.recent_race, None);
            assert_eq!(p.stated_longest_recent_km, None);
            assert_eq!(p.layoff_returned_epoch_day, None);
            assert_eq!(p.meet_epoch_day, None);
            assert_eq!(p.calis_equipment, None);
            assert_eq!(p.specialization_muscle, None);
            assert!(!p.runner_strength);
            assert!(!p.health.symptomatic && !p.health.acute_illness);
        }
        other => panic!("expected SetProfile, got {other:?}"),
    }

    // TrimRunTrack: the EXACT shape Core.kt emits (all four snake_case keys).
    // FFI deserialize failure on replay is a SILENT drop, so shape drift here
    // is silent loss of a trim (an untrimmed track resurrects on cold start).
    let trim = r#"{"TrimRunTrack":{"entry_id":42,"observed_at_fallback":1700,"keep_from_sec":1720,"keep_until_sec":1900}}"#;
    match serde_json::from_str::<Event>(trim).expect("trim wire form must parse") {
        Event::TrimRunTrack {
            entry_id,
            observed_at_fallback,
            keep_from_sec,
            keep_until_sec,
        } => {
            assert_eq!(entry_id, 42);
            assert_eq!(observed_at_fallback, 1700);
            assert_eq!(keep_from_sec, 1720);
            assert_eq!(keep_until_sec, 1900);
        }
        other => panic!("expected TrimRunTrack, got {other:?}"),
    }
    // observed_at_fallback is #[serde(default)]: an omitted key decodes to 0
    // (a nonzero-id run is matched by id anyway) so an old/minimal line replays.
    let trim_min = r#"{"TrimRunTrack":{"entry_id":7,"keep_from_sec":10,"keep_until_sec":20}}"#;
    match serde_json::from_str::<Event>(trim_min).expect("minimal trim must parse") {
        Event::TrimRunTrack {
            observed_at_fallback,
            ..
        } => {
            assert_eq!(observed_at_fallback, 0, "omitted fallback defaults to 0");
        }
        other => panic!("expected TrimRunTrack, got {other:?}"),
    }
}

/// The consolidated person fields (age, bodyweight, sex, resting
/// HR, measured HRmax) round-trip through the `SetProfile` wire exactly as
/// Core.kt's guided setup / profile editor emit them. All are `#[serde(default)]`
/// so the shell omits absent ones (previous test) and sends present ones here.
#[test]
fn set_profile_person_fields_round_trip() {
    let wire = r#"{"SetProfile":{"progression_cadence":"EverySession","lift_goal":"Hypertrophy","goal_distance":"General","concurrent_goal":"Hypertrophy","weekly_sets":10,"running_days_per_week":0,"running_km_per_week":0.0,"advanced":false,"female":true,"bodyweight_kg":62.5,"age_years":34.0,"resting_hr_bpm":54.0,"measured_hr_max":188.0}}"#;
    match serde_json::from_str::<Event>(wire).expect("person-field profile must parse") {
        Event::SetProfile(p) => {
            assert_eq!(p.female, Some(true));
            assert_eq!(p.bodyweight_kg, Some(62.5));
            assert_eq!(p.age_years, Some(34.0));
            assert_eq!(p.resting_hr_bpm, Some(54.0));
            assert_eq!(p.measured_hr_max, Some(188.0));
        }
        other => panic!("expected SetProfile, got {other:?}"),
    }

    // And the reverse: a constructed Profile serializes those fields under the
    // snake_case names the shell parses back (view exposes `profile` verbatim).
    let p = Profile {
        age_years: Some(34.0),
        resting_hr_bpm: Some(54.0),
        measured_hr_max: Some(188.0),
        bodyweight_kg: Some(62.5),
        female: Some(true),
        ..sample_profile()
    };
    let json = serde_json::to_string(&p).expect("profile serializes");
    for key in [
        "\"age_years\":34.0",
        "\"resting_hr_bpm\":54.0",
        "\"measured_hr_max\":188.0",
        "\"bodyweight_kg\":62.5",
        "\"female\":true",
    ] {
        assert!(
            json.contains(key),
            "serialized profile missing {key}: {json}"
        );
    }
}

/// The scheduling, race, layoff, equipment and specialization profile fields
/// and the two added health-screen flags decode from the exact snake_case keys
/// a shell sends. A key drift would make the core silently ignore the answer.
#[test]
fn set_profile_planning_fields_round_trip() {
    let wire = r#"{"SetProfile":{"progression_cadence":"EverySession","lift_goal":"Hypertrophy","goal_distance":"HalfMarathon","concurrent_goal":"Hypertrophy","weekly_sets":10,"running_days_per_week":3,"running_km_per_week":20.0,"advanced":false,"health":{"symptomatic":true,"acute_illness":true},"lift_days_per_week":3,"training_weekdays":21,"goal_race_epoch_day":20600,"recent_race":{"distance_km":10.0,"time_sec":3000},"stated_longest_recent_km":12.5,"layoff_returned_epoch_day":20400,"meet_epoch_day":20650,"calis_equipment":{"pull_up_bar":true,"dip_bars":false,"rings":false},"specialization_muscle":"glutes","runner_strength":true}}"#;
    let p = match serde_json::from_str::<Event>(wire).expect("planning-field profile must parse") {
        Event::SetProfile(p) => p,
        other => panic!("expected SetProfile, got {other:?}"),
    };
    assert!(p.health.symptomatic);
    assert!(p.health.acute_illness);
    assert_eq!(p.lift_days_per_week, Some(3));
    assert_eq!(p.training_weekdays, Some(0b0010101));
    assert_eq!(p.goal_race_epoch_day, Some(20600));
    assert_eq!(
        p.recent_race,
        Some(RecentRace {
            distance_km: 10.0,
            time_sec: 3000,
            epoch_day: None,
        }),
        "an undated race result decodes with no date"
    );
    assert_eq!(p.stated_longest_recent_km, Some(12.5));
    assert_eq!(p.layoff_returned_epoch_day, Some(20400));
    assert_eq!(p.meet_epoch_day, Some(20650));
    assert_eq!(
        p.calis_equipment,
        Some(CalisEquipment {
            pull_up_bar: true,
            ..CalisEquipment::default()
        })
    );
    assert_eq!(p.specialization_muscle.as_deref(), Some("glutes"));
    assert!(p.runner_strength);

    // The echoed profile serializes back to the same values.
    let back: Profile =
        serde_json::from_str(&serde_json::to_string(&p).expect("profile serializes"))
            .expect("serialized profile parses");
    assert_eq!(back, p);
}

/// HARD RULE 3 at the wire level: the shell's red-flag readiness buttons must
/// never be silently dropped on replay. Each safety signal's Kotlin `.name`
/// string (`Core.kt::ReadinessSignal`) must deserialize to the matching
/// variant, a rename on either side would make the FFI swallow the event
/// (deserialize error → cleared output) and leave a red-flag session
/// un-blocked. Pin the safety signals' exact wire names.
#[test]
fn safety_readiness_signals_deserialize_from_shell_names() {
    for (name, expected) in [
        ("Pain", ReadinessSignal::Pain),
        ("Illness", ReadinessSignal::Illness),
        ("RedS", ReadinessSignal::RedS),
        ("CardiacRedFlag", ReadinessSignal::CardiacRedFlag),
        ("BoneStress", ReadinessSignal::BoneStress),
    ] {
        let wire =
            format!(r#"{{"SubmitReadiness":{{"signal":"{name}","value":1.0,"observed_at":0}}}}"#);
        match serde_json::from_str::<Event>(&wire).expect("safety signal must parse") {
            Event::SubmitReadiness(i) => {
                assert_eq!(i.signal, expected, "{name} deserialized to wrong signal")
            }
            other => panic!("expected SubmitReadiness for {name}, got {other:?}"),
        }
    }
}

/// Wire pin for the additive readiness fields (`streak`, `pain`): the
/// pre-existing three-field shape must keep parsing (defaults), and the new
/// graded-pain shape must parse field-for-field as Core.kt will emit it.
/// Kotlin-side `ignoreUnknownKeys` tolerates the view side; this guards the
/// event side, where a drift would make the FFI silently drop the report.
#[test]
fn readiness_wire_shape_old_and_graded_pain_forms_parse() {
    // Legacy shape: no streak, no pain detail → conservative defaults.
    let legacy = r#"{"SubmitReadiness":{"signal":"Pain","value":1.0,"observed_at":0}}"#;
    match serde_json::from_str::<Event>(legacy).expect("legacy shape must parse") {
        Event::SubmitReadiness(i) => {
            assert_eq!(i.streak, 0);
            assert_eq!(
                i.pain, None,
                "missing detail must default to None (hard stop)"
            );
        }
        other => panic!("expected SubmitReadiness, got {other:?}"),
    }

    // Graded shape, snake_case fields, unit-variant enum strings, and
    // `persists` omitted (serde default false).
    let graded = r#"{"SubmitReadiness":{"signal":"Pain","value":1.0,"observed_at":0,"streak":2,"pain":{"kind":"TendonLoadRelated","severity":3,"trend":"Stable"}}}"#;
    match serde_json::from_str::<Event>(graded).expect("graded shape must parse") {
        Event::SubmitReadiness(i) => {
            assert_eq!(i.streak, 2);
            let d = i.pain.expect("pain detail parsed");
            assert_eq!(d.kind, crate::schema::PainKind::TendonLoadRelated);
            assert_eq!(d.severity, Some(3));
            assert_eq!(d.trend, crate::schema::PainTrend::Stable);
            assert!(!d.persists, "omitted persists defaults to false");
        }
        other => panic!("expected SubmitReadiness, got {other:?}"),
    }
}

#[test]
fn checkin_wire_without_fatigue_stress_still_replays() {
    // Fatigue and stress are #[serde(default)] Option fields. A check-in
    // persisted by an older app version omits both keys; it MUST decode
    // (fields default to None) and replay unchanged through the engine.
    let old = r#"{"SubmitCheckin":{"observed_at":100,"sleep_quality":2,"soreness":4,"mood":3}}"#;
    let event: Event = serde_json::from_str(old).expect("an older check-in decodes");
    match &event {
        Event::SubmitCheckin(c) => {
            assert_eq!(c.fatigue, None, "absent fatigue defaults to None");
            assert_eq!(c.stress, None, "absent stress defaults to None");
            assert_eq!(c.sleep_quality, Some(2));
            assert_eq!(c.soreness, Some(4));
            assert_eq!(c.mood, Some(3));
        }
        _ => panic!("expected SubmitCheckin"),
    }

    // The new wire form carries both and decodes as an ordinary check-in.
    let new = r#"{"SubmitCheckin":{"observed_at":100,"sleep_quality":2,"soreness":4,"mood":3,"fatigue":5,"stress":1}}"#;
    match serde_json::from_str::<Event>(new).expect("a full check-in decodes") {
        Event::SubmitCheckin(c) => {
            assert_eq!(c.fatigue, Some(5));
            assert_eq!(c.stress, Some(1));
        }
        _ => panic!("expected SubmitCheckin"),
    }

    // Replay-equivalence: the old-format log and the same log with fatigue /
    // stress explicitly null produce byte-identical ViewModels — the fields
    // are inert when unanswered (wellness_goodness averages answered items).
    let app = Engine;
    let mut m_old = Model::default();
    let mut m_null = Model::default();
    app.update(event, &mut m_old).expect_only_render();
    let null_event: Event = serde_json::from_str(
            r#"{"SubmitCheckin":{"observed_at":100,"sleep_quality":2,"soreness":4,"mood":3,"fatigue":null,"stress":null}}"#,
        )
        .expect("explicit-null check-in decodes");
    app.update(null_event, &mut m_null).expect_only_render();
    assert_eq!(
        serde_json::to_string(&app.view(&m_old)).unwrap(),
        serde_json::to_string(&app.view(&m_null)).unwrap(),
        "absent vs explicit-null fatigue/stress must replay identically"
    );
}

#[test]
fn readiness_signal_wire_names_match_the_shell_contract() {
    // Each `ReadinessSignal` serialises to its bare variant name, and the
    // Android shell hand-builds that exact string as the event's `signal`
    // field (Core.kt `ReadinessSignal.name`). Renaming a variant here would
    // compile clean but silently drop every readiness submission for it -
    // including the medical-referral flags at the top of the safety ladder.
    // This locks the wire names so such a rename fails here instead.
    let cases = [
        (ReadinessSignal::Rpe, "Rpe"),
        (ReadinessSignal::EstimatedOneRm, "EstimatedOneRm"),
        (ReadinessSignal::BarVelocity, "BarVelocity"),
        (ReadinessSignal::VelocityLoss, "VelocityLoss"),
        (ReadinessSignal::WellnessZ, "WellnessZ"),
        (ReadinessSignal::HrvLnRmssd, "HrvLnRmssd"),
        (ReadinessSignal::HrvCv, "HrvCv"),
        (ReadinessSignal::AerobicDecoupling, "AerobicDecoupling"),
        (ReadinessSignal::RestingHr, "RestingHr"),
        (ReadinessSignal::Pain, "Pain"),
        (ReadinessSignal::Illness, "Illness"),
        (ReadinessSignal::RedS, "RedS"),
        (ReadinessSignal::CardiacRedFlag, "CardiacRedFlag"),
        (ReadinessSignal::BoneStress, "BoneStress"),
    ];
    for (signal, wire) in cases {
        assert_eq!(
            serde_json::to_string(&signal).unwrap(),
            format!("\"{wire}\""),
            "wire name for {signal:?}"
        );
    }
}

#[test]
fn entry_id_events_round_trip_the_wire_and_default_entry_id() {
    // LogSet with an entry_id encodes/decodes it; DeleteEntry / AmendSet /
    // AmendRun round-trip; observed_at_fallback / longest_recent_km / amend
    // observed_at are serde-default for forward-lean shells.
    let with_id: Event = serde_json::from_str(
            r#"{"LogSet":{"exercise":"Bench","weight_kg":60.0,"reps":8,"rpe":7.0,"observed_at":5,"entry_id":1717171717}}"#,
        )
        .expect("LogSet with entry_id decodes");
    match with_id {
        Event::LogSet { entry_id, .. } => assert_eq!(entry_id, 1_717_171_717),
        other => panic!("expected LogSet, got {other:?}"),
    }

    let del: Event = serde_json::from_str(r#"{"DeleteEntry":{"kind":"Run","entry_id":42}}"#)
        .expect("DeleteEntry decodes with default observed_at_fallback");
    match del {
        Event::DeleteEntry {
            kind,
            entry_id,
            observed_at_fallback,
        } => {
            assert_eq!(kind, EntryKind::Run);
            assert_eq!(entry_id, 42);
            assert_eq!(observed_at_fallback, 0);
        }
        other => panic!("expected DeleteEntry, got {other:?}"),
    }

    let amend: Event = serde_json::from_str(
        r#"{"AmendSet":{"entry_id":7,"exercise":"Squat","weight_kg":150.0,"reps":3,"rpe":9.0}}"#,
    )
    .expect("AmendSet decodes with default observed_at");
    assert!(matches!(amend, Event::AmendSet { entry_id: 7, .. }));

    let amend_run: Event = serde_json::from_str(
        r#"{"AmendRun":{"entry_id":8,"distance_km":6.0,"duration_min":30.0,"hr_pct_max":0.0}}"#,
    )
    .expect("AmendRun decodes with default longest_recent_km/observed_at");
    assert!(matches!(amend_run, Event::AmendRun { entry_id: 8, .. }));
}

#[test]
fn workout_type_tag_round_trips_and_is_back_compat() {
    // The user-declared workout-type label is an additive
    // `#[serde(default)] Option<WorkoutType>` on LogRun/LogRunTrack/AmendRun.
    // (1) A tagged run decodes the label AND echoes it on the run view. (2) An
    // OLD-shape event with NO `workout_type` key still decodes (→ None), so
    // older persisted logs and shells replay unchanged (HARD RULE 1:
    // storage + display only, nothing here branches coaching on the tag).
    let app = Engine;

    // (2) back-compat: the exact old wire shape (no workout_type key) parses.
    let old_shape: Event = serde_json::from_str(
            r#"{"LogRun":{"distance_km":10.0,"duration_min":50.0,"hr_pct_max":70.0,"longest_recent_km":12.0,"observed_at":0,"entry_id":0}}"#,
        )
        .expect("pre-I16 LogRun (no workout_type) must still decode");
    match old_shape {
        Event::LogRun { workout_type, .. } => {
            assert_eq!(workout_type, None, "absent field defaults to None")
        }
        other => panic!("expected LogRun, got {other:?}"),
    }

    // (1) a tagged run decodes the label and the run view echoes it verbatim.
    let tagged: Event = serde_json::from_str(
            r#"{"LogRun":{"distance_km":8.0,"duration_min":48.0,"hr_pct_max":70.0,"longest_recent_km":12.0,"observed_at":0,"entry_id":0,"workout_type":"Interval"}}"#,
        )
        .expect("tagged LogRun must decode");
    assert!(matches!(
        tagged,
        Event::LogRun {
            workout_type: Some(WorkoutType::Interval),
            ..
        }
    ));

    let mut model = Model::default();
    app.update(tagged, &mut model).expect_only_render();
    assert_eq!(
        app.view(&model).runs[0].workout_type,
        Some(WorkoutType::Interval),
        "the view echoes the user's tag for history display"
    );

    // The bare variant name is the wire form the Kotlin shell mirrors by
    // `WorkoutType.name`: pin it so a rename fails here, not silently on-device.
    assert_eq!(
        serde_json::to_string(&WorkoutType::LongRun).unwrap(),
        "\"LongRun\""
    );

    // An untagged run leaves the field None on the view (never fabricated).
    let mut m2 = Model::default();
    app.update(log_run(5.0, 25.0, 70.0, 10.0, 0, 0), &mut m2)
        .expect_only_render();
    assert_eq!(app.view(&m2).runs[0].workout_type, None);
}

#[test]
fn why_view_serde_defaults_to_empty_for_old_shells() {
    // An AdjustmentView from an old core (no `why` key) decodes to an empty
    // WhyView; the block is additive and never breaks replay.
    let json = r#"{"summary":"Take a full rest day","grade":"Strong","citation":"x","confidence":0.9,"safety_critical":true,"contested":false}"#;
    let v: AdjustmentView = serde_json::from_str(json).expect("old shape decodes");
    assert_eq!(v.why, WhyView::default());
    assert!(v.why.basis.is_empty() && v.why.grade_note.is_empty() && v.why.improves.is_empty());
}

#[test]
fn profile_is_echoed_back_for_shell_hydration() {
    let app = Engine;
    let mut model = Model::default();
    assert!(app.view(&model).profile.is_none());

    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    assert_eq!(app.view(&model).profile.as_ref(), Some(&sample_profile()));

    app.update(Event::ClearProfile, &mut model)
        .expect_only_render();
    assert!(app.view(&model).profile.is_none());
}

#[test]
fn protein_and_hr_wire_fields_are_additive_and_serde_default() {
    // The ViewModel fields hr_max and protein_figures and
    // RunResultView.spike_has_baseline are additive with serde defaults: an
    // old ViewModel JSON lacking them still decodes.
    let app = Engine;
    let mut model = Model {
        profile: Some(sample_profile()),
        ..Default::default()
    };
    // Request both calculators so the structured figures populate.
    app.update(
        Event::ComputeProtein {
            bodyweight_kg: 80.0,
            masters: true,
            deficit: false,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::ComputeHrZones {
            age_years: 40.0,
            resting_hr_bpm: None,
            weeks_since_recalc: None,
            weeks_since_pace_test: None,
        },
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    // Structured protein figure mirrors the prose row (80 kg × masters band).
    let pf = vm
        .protein_figures
        .iter()
        .find(|f| f.kind == ProteinTargetKind::Masters)
        .expect("masters figure");
    assert!(pf.low_g_per_day > 0.0 && pf.high_g_per_day >= pf.low_g_per_day && !pf.refused);
    // Structured HRmax: Tanaka(40) = 180, with the 208 − 0.7 split exposed.
    let hm = vm.hr_max.as_ref().expect("hr_max figure");
    assert!(!hm.measured && hm.bpm == 180.0 && hm.tanaka_intercept == 208.0);

    // Round-trip through JSON with the three new fields stripped: still decodes.
    let mut json: serde_json::Value = serde_json::to_value(&vm).unwrap();
    let obj = json.as_object_mut().unwrap();
    obj.remove("hr_max");
    obj.remove("protein_figures");
    for run in obj
        .get_mut("runs")
        .and_then(|r| r.as_array_mut())
        .into_iter()
        .flatten()
    {
        run.as_object_mut().unwrap().remove("spike_has_baseline");
    }
    let decoded: ViewModel = serde_json::from_value(json).expect("old-shape JSON decodes");
    assert!(decoded.hr_max.is_none() && decoded.protein_figures.is_empty());
}

// The Android shell hand-builds SubmitReview JSON (Core.kt), omitting the
// Option fields it does not set. These two tests pin the exact wire form the
// shell emits for the decoupling / easy-run-intensity review context, so a
// rename on either side of the FFI fails here rather than silently dropping
// the field to None.
#[test]
fn decoupling_review_wire_json_drives_feedback() {
    let wire = r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"decoupling":{"drift_pct":12.0,"cool_steady_context":true},"bad_day":false}}"#;
    let event: Event = serde_json::from_str(wire).expect("shell wire form parses");

    let app = Engine;
    let mut model = Model::default();
    app.update(event, &mut model).expect_only_render();

    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!(fb.category, FeedbackCategory::CorrectiveProcess);
}

#[test]
fn easy_run_intensity_wire_json_drives_feedback() {
    let wire = r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"easy_frac_time_above_vt1":0.3,"bad_day":false}}"#;
    let event: Event = serde_json::from_str(wire).expect("shell wire form parses");

    let app = Engine;
    let mut model = Model::default();
    app.update(event, &mut model).expect_only_render();

    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!(fb.category, FeedbackCategory::IntensityDiscipline);
}

#[test]
fn review_observed_at_decodes_and_defaults() {
    // New wire form carries the stamp…
    let with: Event = serde_json::from_str(
            r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"bad_day":false,"observed_at":424242}}"#,
        )
        .expect("review with observed_at parses");
    match with {
        Event::SubmitReview(r) => assert_eq!(r.observed_at, 424_242),
        other => panic!("expected SubmitReview, got {other:?}"),
    }
    // …and the old persisted form still replays (defaults to 0).
    let without: Event = serde_json::from_str(
            r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"bad_day":false}}"#,
        )
        .expect("pre-timestamp review parses");
    match without {
        Event::SubmitReview(r) => assert_eq!(r.observed_at, 0),
        other => panic!("expected SubmitReview, got {other:?}"),
    }
}
