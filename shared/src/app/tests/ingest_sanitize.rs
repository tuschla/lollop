//! Hostile wire values (NaN, ±∞, 1e300, out-of-range integers) are clamped or
//! rejected on ingest, so the view never panics or serializes `null`.

use super::*;

// ── SetProfile sanitizes poison floats on ingest ──
#[test]
fn set_profile_sanitizes_poison_floats() {
    let app = Engine;
    let mut model = Model::default();

    let mut p = running_profile();
    p.running_km_per_week = 1e300;
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    let km = model.profile.as_ref().unwrap().running_km_per_week;
    assert!(
        km.is_finite() && km <= 1.0e12,
        "running_km_per_week must clamp, got {km}"
    );

    let mut p2 = running_profile();
    p2.running_km_per_week = -5.0;
    app.update(Event::SetProfile(p2), &mut model)
        .expect_only_render();
    assert_eq!(
        model.profile.as_ref().unwrap().running_km_per_week,
        0.0,
        "a negative weekly volume floors at 0"
    );

    // A NaN age sanitizes to 0 → the youth gate fires (conservative).
    let mut p3 = strength_profile();
    p3.age_years = Some(f64::NAN);
    app.update(Event::SetProfile(p3), &mut model)
        .expect_only_render();
    assert!(
        model.profile.as_ref().unwrap().health.youth,
        "a NaN age must sanitize to 0 and fire the youth gate"
    );
}

// ── A negative LogRun distance is clamped at ingest ──
#[test]
fn negative_log_run_distance_is_clamped() {
    let app = Engine;
    let mut model = Model::default();
    app.update(log_run(-10.0, 60.0, 70.0, 0.0, DAY_SEC, 1), &mut model)
        .expect_only_render();
    assert!(
        model.runs[0].distance_km >= 0.0,
        "a negative distance must clamp to ≥0, got {}",
        model.runs[0].distance_km
    );
    // The measured weekly anchor is never driven negative by it.
    let (_, weekly, _) = run_anchors(&model.runs, 2, 0);
    assert!(
        weekly.is_none_or(|w| w >= 0.0),
        "weekly anchor must not go negative"
    );
}

// ── Overflow-safe epoch math + non-finite/huge float clamp on ingest ──
#[test]
fn extreme_wire_values_do_not_panic_or_poison_the_view() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    // A huge finite weight must be clamped so e1RM math can't reach inf.
    app.update(
        log_set("Back Squat", 1.0e300, 3, 8.0, MON * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    assert!(
        model.sets[0].weight_kg.is_finite() && model.sets[0].weight_kg <= 1.0e12,
        "a huge wire weight must be clamped finite"
    );
    // A corrupt near-i64::MAX SetToday must not panic view() (weekday math).
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: i64::MAX,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model); // must not panic
    // A finite serializable view is produced.
    assert!(serde_json::to_string(&vm).is_ok());
    // mon0_weekday itself is overflow-safe at the extremes.
    let _ = mon0_weekday(i64::MAX);
    let _ = mon0_weekday(i64::MIN);
}

// ── Calculator/predict events sanitize floats → no `inf`→`null` brick ──
#[test]
fn calculator_events_1e300_inputs_never_serialize_to_null() {
    // The threat sanitize_f64 guards: a huge FINITE wire float (serde_json
    // accepts 1e300) overflows a downstream product to `inf`, which
    // serializes to JSON `null`, which a non-null Kotlin `Double` decode
    // rejects → a persisted-singleton view-brick. Feeding 1e300-scale
    // inputs to PredictRace + ComputeProtein must leave every numeric
    // output field a finite JSON number, not `null`, and never panic.
    let app = Engine;
    let mut model = Model::default();

    app.update(
        Event::PredictRace {
            recent_distance_m: 1.0e300,
            recent_time_sec: 1.0e300,
            goal_distance_m: 1.0e300,
            weekly_km: 1.0e300,
            weeks_since_race: None,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::ComputeProtein {
            bodyweight_kg: 1.0e300,
            masters: true,
            deficit: false,
        },
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model); // must not panic
    // Whole view round-trips through JSON.
    assert!(serde_json::to_string(&vm).is_ok());

    // Every numeric field of the race prediction is a finite JSON number,
    // never `null` (an `inf` would have serialized to `null`).
    let pred = vm
        .race_prediction
        .as_ref()
        .expect("a prediction is present after PredictRace");
    for f in [
        pred.low_sec,
        pred.high_sec,
        pred.recent_distance_m,
        pred.recent_time_sec,
        pred.goal_distance_m,
        pred.weekly_km,
    ] {
        assert!(f.is_finite(), "race-prediction float went non-finite: {f}");
    }
    assert!(pred.evidence.confidence.is_finite());
    let pred_json = serde_json::to_value(pred).unwrap();
    for key in [
        "low_sec",
        "high_sec",
        "confidence",
        "recent_distance_m",
        "recent_time_sec",
        "goal_distance_m",
        "weekly_km",
    ] {
        assert!(
            pred_json[key].is_number(),
            "race-prediction field {key} serialized to a non-number (null?): {}",
            pred_json[key]
        );
    }

    // Same for every emitted protein figure.
    assert!(
        !vm.protein_figures.is_empty(),
        "a masters protein figure must be present"
    );
    for fig in &vm.protein_figures {
        assert!(
            fig.low_g_per_day.is_finite() && fig.high_g_per_day.is_finite(),
            "protein figure went non-finite: {fig:?}"
        );
        let fig_json = serde_json::to_value(fig).unwrap();
        assert!(
            fig_json["low_g_per_day"].is_number() && fig_json["high_g_per_day"].is_number(),
            "protein figure serialized a numeric field to null: {fig_json}"
        );
    }
}

// ── Submit* events sanitize floats → no out-of-domain magnitude on the wire ──
#[test]
fn submit_events_1e300_inputs_never_serialize_to_null() {
    // Same threat as the calculator path, and reachable by replaying any
    // imported NDJSON log: a huge FINITE wire float either accumulates to
    // `inf` in the derivation math (serializing as JSON `null`, which a
    // non-null Kotlin decode rejects) or is echoed back verbatim as an absurd
    // magnitude. Every float the three Submit* events carry is clamped into
    // the ingest domain instead.
    let app = Engine;
    let mut model = Model::default();
    // Enough huge, VARYING check-in days to build a baseline: identical
    // readings would cancel in the SD term and never exercise the variance.
    for d in 0..8 {
        app.update(
            Event::SubmitCheckin(CheckinInput {
                observed_at: (MON + d) * DAY_SEC + 100,
                sleep_quality: Some(3),
                soreness: Some(3),
                mood: Some(3),
                fatigue: Some(3),
                stress: Some(3),
                resting_hr_bpm: Some(1.0e200 * (1.0 + d as f64)),
                hrv_rmssd_ms: Some(1.0e200 * (1.0 + d as f64)),
            }),
            &mut model,
        )
        .expect_only_render();
    }
    app.update(
        Event::SetToday {
            epoch_day: MON + 7,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            effort_min: Some(1.0e300),
            ..input(ReadinessSignal::AerobicDecoupling, 1.0e300)
        }),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitReview(SessionReview {
            overtraining_signal_weeks: Some(1.0e300),
            single_session_spike_frac: Some(1.0e300),
            easy_frac_time_above_vt1: Some(1.0e300),
            positive_split_pct: Some(1.0e300),
            weekly_velocity_drop_m_s: Some(1.0e300),
            rep_drop_frac: Some(1.0e300),
            mcv_delta_m_s: Some(1.0e300),
            first_set_rpe_delta: Some(1.0e300),
            hrv_single_day_z: Some(1.0e300),
            decoupling: Some(Decouple {
                drift_pct: 1.0e300,
                cool_steady_context: true,
            }),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model); // must not panic
    let json = serde_json::to_string(&vm).expect("the whole view round-trips through JSON");
    // Nothing 1e200/1e300-scale survives to the wire: the clamp keeps every
    // float inside ±1e12, so no figure serializes in exponent form and no
    // downstream product overflowed to `inf` → `null`.
    assert!(
        !json.contains("e200") && !json.contains("e300") && !json.contains("e+"),
        "an out-of-domain magnitude reached the wire: {json}"
    );
    for row in &vm.readiness_summary {
        assert!(
            row.value.is_finite(),
            "readiness row {:?} went non-finite",
            row.signal
        );
    }
    let echo = vm.checkin_today.expect("today's check-in is echoed");
    for v in [echo.resting_hr_bpm, echo.hrv_rmssd_ms]
        .into_iter()
        .flatten()
    {
        assert!(
            v.abs() <= 1.0e12,
            "echoed check-in float outside the ingest domain: {v}"
        );
    }
}

#[test]
fn integer_wire_fields_are_clamped_on_ingest() {
    // A wire set carrying u32::MAX reps / u64::MAX entry_id is clamped into a
    // sane, Kotlin-decodable range at ingest.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        with_entry_id(log_set("Back squat", 100.0, u32::MAX, 8.0, 0), u64::MAX),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let lift = vm.lifts.first().expect("the set was ingested");
    assert_eq!(lift.reps, 10_000, "reps clamped to the ingest maximum");
    assert_eq!(
        lift.entry_id,
        i64::MAX as u64,
        "entry_id clamped into the i64 range a Kotlin Long can decode"
    );
}

#[test]
fn delete_by_an_out_of_range_entry_id_matches_the_clamped_row() {
    let app = Engine;
    let mut model = Model::default();
    app.update(log_set_id("Back squat", 100.0, u64::MAX, 0), &mut model)
        .expect_only_render();
    app.update(
        Event::DeleteEntry {
            kind: EntryKind::Set,
            entry_id: u64::MAX,
            observed_at_fallback: 0,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(
        app.view(&model).lifts.is_empty(),
        "a delete carrying the same out-of-range id must remove the clamped row"
    );
}
