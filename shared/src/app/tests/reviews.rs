//! Session review and weekly check-in rows: autoreg and hypertrophy triggers,
//! week-level deloads, trend tone, VBT and decoupling rows, and clears.

use super::*;

#[test]
fn review_bone_pain_short_circuits_to_injury_concern() {
    let app = Engine;
    let mut model = Model::default();
    let review = SessionReview {
        bone_pain_red_flag: true,
        // Execution says "mastery", but safety must suppress it.
        lift: Some(LiftExec {
            reps_met: true,
            rir_actual: 2,
            rir_target: 2,
        }),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();

    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!(fb.category, FeedbackCategory::ConcernInjury);
    assert!(fb.suppresses_praise);
    assert!(fb.evidence.safety_critical);
    assert!(!fb.message.is_empty());
}

#[test]
fn review_clean_lift_yields_mastery() {
    let app = Engine;
    let mut model = Model::default();
    let review = SessionReview {
        lift: Some(LiftExec {
            reps_met: true,
            rir_actual: 2,
            rir_target: 2,
        }),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();

    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!(fb.category, FeedbackCategory::PositiveMastery);
    assert!(!fb.suppresses_praise);
}

#[test]
fn review_even_split_run_carries_human_category_label() {
    // An even-effort run (no positive split) resolves to PositiveExecution;
    // the core ships the human overline verbatim so the shell renders it
    // without a hand-maintained parallel label map.
    let app = Engine;
    let mut model = Model::default();
    let review = SessionReview {
        positive_split_pct: Some(0.0),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();

    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!(fb.category, FeedbackCategory::PositiveExecution);
    assert_eq!(fb.category_label, "Pacing");
}

#[test]
fn review_week_level_deloads_surface_as_adjustments() {
    // Two failed key sessions is a week-level fatigue deload trigger
    // (autoreg-036) carried on the review, not a single-session readiness
    // marker. It must reach the adjustments surface, and, being a volume
    // deload, must not block training.
    let app = Engine;
    let mut model = Model::default();
    let review = SessionReview {
        failed_key_sessions: Some(2),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert!(
        vm.review_adjustments
            .iter()
            .any(|a| a.summary.starts_with("Lighter week")),
        "a two-failed-session review should surface a deload adjustment"
    );
    assert!(
        vm.adjustments.is_empty(),
        "a review deload must not leak into the readiness-driven list"
    );
    assert!(!vm.train_blocked, "a volume deload must not block training");
}

#[test]
fn review_sub_threshold_deload_counts_add_nothing() {
    // One failed session / an unset velocity drop are below every trigger,
    // so the adjustments surface stays as bare as a default review's.
    let app = Engine;
    let mut model = Model::default();
    let review = SessionReview {
        failed_key_sessions: Some(1),
        rpe_load_gap_sessions: Some(1),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert!(
        vm.review_adjustments.is_empty(),
        "sub-threshold counts must not fabricate a deload"
    );
}

#[test]
fn clearing_review_empties_feedback() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SubmitReview(SessionReview::default()), &mut model)
        .expect_only_render();
    assert!(app.view(&model).feedback.is_some());
    app.update(Event::ClearReview, &mut model)
        .expect_only_render();
    assert!(app.view(&model).feedback.is_none());
}

#[test]
fn clearing_the_weekly_checkin_drops_its_week_deloads() {
    // Week deloads share the weekly check-in's lifecycle: ClearReadiness (the
    // separate readiness list) and ClearReview (the per-session review) leave
    // them in force; ClearWeeklyCheckin takes them.
    let app = Engine;
    let mut model = Model::default();
    let review = SessionReview {
        failed_key_sessions: Some(2),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(!vm.review_adjustments.is_empty());
    assert!(vm.weekly_checkin_present);
    assert!(
        !vm.review_present,
        "a weekly-only payload sets no session review"
    );

    app.update(Event::ClearReadiness, &mut model)
        .expect_only_render();
    assert!(
        !app.view(&model).review_adjustments.is_empty(),
        "clearing readiness must not touch review-owned deloads"
    );
    app.update(Event::ClearReview, &mut model)
        .expect_only_render();
    assert!(
        !app.view(&model).review_adjustments.is_empty(),
        "clearing the session review must not touch the weekly deloads"
    );

    app.update(Event::ClearWeeklyCheckin, &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(
        vm.review_adjustments.is_empty(),
        "clearing the weekly check-in drops its deloads"
    );
    assert!(!vm.weekly_checkin_present);
}

#[test]
fn review_carries_the_new_autoreg_and_hypertrophy_triggers() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    app.update(
        Event::SubmitReview(SessionReview {
            mcv_delta_m_s: Some(-0.10),
            first_set_reps_met: Some(false),
            first_set_rpe_delta: Some(1.5),
            cut_last_two_sessions: true,
            interval_reps_over_target: Some(2),
            can_hold_easy_pace_under_hr_cap: Some(false),
            hrv_unreliable_last_three: Some(2),
            hrv_suppressed_days: Some(3),
            wellness_suppressed_days: Some(2),
            rhr_rising: true,
            hypertrophy_deload_triggers: Some(2),
            rep_drop_frac: Some(0.2),
            low_recovery: true,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    let has = |needle: &str| {
        vm.review_adjustments
            .iter()
            .any(|a| a.summary.contains(needle))
    };
    assert!(
        has("reduce working weights"),
        "VBT: {:#?}",
        vm.review_adjustments
    );
    assert!(has("drop the last planned set"), "set-volume action");
    assert!(has("hold weekly volume"), "two-cut-session hold");
    assert!(has("slow the remaining reps ~3%"), "interval autoreg");
    assert!(has("heart rate decides the pace"), "easy-pace cap");
    assert!(has("HRV readings look unreliable"), "unreliable HRV");
    assert!(
        has("a recovery day or a few easy days"),
        "HRV suppression streak"
    );
    assert!(has("1–3 easy days"), "wellness+RHR");
    assert!(has("take the lighter week now"), "hypertrophy triggers");
    assert!(has("Rest longer between sets"), "rep-drop rest");
    assert!(has("Cut this week to"), "recovery-adjusted volume");
    // Every review row is graded (HARD RULE 2).
    for a in &vm.review_adjustments {
        assert!(
            a.evidence.grade.is_some() && !a.evidence.citation.is_empty(),
            "{}",
            a.summary
        );
    }
}

#[test]
fn review_week_deloads_fire_from_rpe_creep_and_single_day_hrv() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReview(SessionReview {
            rpe_creep_plus_one: true,
            wellness_z_low_days: Some(3),
            hrv_single_day_z: Some(-1.5),
            hrv_downtrend_days: Some(2),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(
        vm.review_adjustments
            .iter()
            .any(|a| a.summary.starts_with("Lighter week:")),
        "autoreg-024 deload: {:#?}",
        vm.review_adjustments
    );
    assert!(
        vm.review_adjustments.iter().any(|a| a
            .summary
            .starts_with("HRV has dropped below your normal range")),
        "autoreg-028 single-day downgrade"
    );
}

#[test]
fn review_decoupling_band_verdict_rides_with_the_measurement() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReview(SessionReview {
            decoupling: Some(Decouple {
                drift_pct: 12.0,
                cool_steady_context: true,
            }),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let band = vm
        .review_adjustments
        .iter()
        .find(|a| a.summary.contains("Heart-rate drift 12.0%"))
        .expect("band verdict row");
    assert!(band.summary.contains("≥10%"), "{}", band.summary);
    assert!(!band.evidence.citation.is_empty());
}

#[test]
fn novice_stall_action_reaches_the_review_rows() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReview(SessionReview {
            stall_failed_sessions: Some(3),
            stall_adequate_recovery: true,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(
        vm.review_adjustments
            .iter()
            .any(|a| a.summary.contains("lower this lift's weight 10%")),
        "{:#?}",
        vm.review_adjustments
    );
}

#[test]
fn trend_tone_provisional_and_signal_source_surface() {
    let app = Engine;
    let mut model = Model::default();

    // Fresh model: everything is a provisional population default.
    let vm = app.view(&model);
    assert!(vm.provisional.is_some(), "0 days of data → provisional");
    assert!(vm.autoreg_source.is_none(), "no inputs → no source row");
    assert!(vm.trend.is_none());

    // A wellness input without HRV → subjective + performance source.
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::WellnessZ,
            value: -0.2,
            observed_at: 100 * WEEK,
            streak: 0,
            pain: None,
            effort_min: None,
        }),
        &mut model,
    )
    .expect_only_render();
    let src = app.view(&model).autoreg_source.expect("source row");
    assert!(
        src.summary
            .contains("Weights follow your check-ins and logged sets"),
        "{}",
        src.summary
    );

    // An HRV reading upgrades the source to the rolling HRV gate.
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::HrvLnRmssd,
            value: 0.1,
            observed_at: 100 * WEEK,
            streak: 0,
            pain: None,
            effort_min: None,
        }),
        &mut model,
    )
    .expect_only_render();
    let src = app.view(&model).autoreg_source.expect("source row");
    assert!(src.summary.contains("7-day HRV trend"), "{}", src.summary);

    // A load-explained decline routes to the recovery-first trend message.
    app.update(
        Event::SubmitReview(SessionReview {
            trend_direction: Some(TrendDirection::Down),
            performance_down: true,
            low_recovery: true,
            planned_hard: Some(true),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let trend = vm.trend.expect("trend row");
    assert!(
        trend
            .summary
            .contains("A lighter week is worth trying first"),
        "{}",
        trend.summary
    );

    // 14 distinct dated days of logging ends the provisional window.
    for i in 1..=14 {
        app.update(
            log_set("Bench press", 60.0, 5, 7.0, i * DAY_SEC + 10),
            &mut model,
        )
        .expect_only_render();
    }
    assert!(app.view(&model).provisional.is_none(), "baseline reached");
}

#[test]
fn weekly_red_flag_answers_defer_training_like_the_readiness_signals() {
    // safety-040/049, ready-upd-005: a weekly bone-pain, RED-S or
    // compulsive-exercise answer stops training and defers to a professional,
    // exactly like the same signs entered as readiness signals.
    for review in [
        SessionReview {
            bone_pain_red_flag: true,
            ..SessionReview::default()
        },
        SessionReview {
            reds_signal_reported: true,
            ..SessionReview::default()
        },
        SessionReview {
            compulsive_flag: true,
            ..SessionReview::default()
        },
    ] {
        let (app, mut model) = planned_strength_model(true);
        app.update(Event::SubmitWeeklyCheckin(review.clone()), &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.train_blocked, "{review:?}");
        assert_eq!(vm.safety_tier, Some(SafetyTier::MedicalReferral));
        assert_eq!(vm.today_headline.kind, HeadlineKind::SafetyHold);
        let ns = vm.next_session.as_ref().expect("hero present");
        assert_eq!(ns.status, SessionStatus::Blocked);
        assert!(ns.items.is_empty());
        let today = model.today_epoch_day.expect("clock");
        assert!(
            vm.week_plan
                .iter()
                .filter(|s| s.epoch_day > today)
                .all(|s| s.items.is_empty()),
            "no future session is programmed through a referral"
        );
        let expected = if review.bone_pain_red_flag {
            HoldReason::MedicalSignal
        } else {
            HoldReason::Reds
        };
        assert_eq!(vm.hold_reason, Some(expected));
    }
}

#[test]
fn vbt_increase_row_is_neutral_under_hrv_downgrade() {
    // A readiness downgrade in scope neutralizes a review's "raise working
    // loads" verdict so it can never headline over the active suppression.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Soreness, 7.0)),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitReview(SessionReview {
            mcv_delta_m_s: Some(0.10),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let inc = vm
        .review_adjustments
        .iter()
        .find(|a| a.summary.contains("raise working weights"))
        .expect("the VBT increase row renders");
    assert!(
        inc.neutral,
        "an increase must be neutral while a downgrade is active"
    );
    assert_ne!(
        vm.today_headline.summary, inc.summary,
        "a neutral increase must not headline over the downgrade"
    );
}

#[test]
fn vbt_increase_row_expires_after_seven_days() {
    // The IncreaseLoad verdict expires with the session-execution note after a
    // week, so an 8-day-old review no longer surfaces it.
    let app = Engine;
    let mut model = Model::default();
    let today = 500i64;
    app.update(
        Event::SubmitReview(SessionReview {
            mcv_delta_m_s: Some(0.10),
            observed_at: (today - 8) * DAY_SEC,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(
        !vm.review_adjustments
            .iter()
            .any(|a| a.summary.contains("raise working weights")),
        "an 8-day-old VBT increase verdict has expired: {:#?}",
        vm.review_adjustments
    );
}

#[test]
fn set_volume_rows_are_graded_moderate() {
    // autoreg-011..014 set-volume verdicts carry AUTOREG-SETVOL-001 (Moderate),
    // not the Strong performance-autoregulation backbone.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReview(SessionReview {
            first_set_reps_met: Some(false),
            first_set_rpe_delta: Some(1.5),
            cut_last_two_sessions: true,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    for needle in ["drop the last planned set", "hold weekly volume"] {
        let row = vm
            .review_adjustments
            .iter()
            .find(|a| a.summary.contains(needle))
            .unwrap_or_else(|| panic!("missing set-volume row: {needle}"));
        assert_eq!(
            row.evidence.grade,
            Some(EvidenceGrade::Moderate),
            "{needle} is graded Moderate (AUTOREG-SETVOL-001)"
        );
    }
}

#[test]
fn decoupling_feedback_does_not_claim_a_missed_target() {
    // Corrective copy is trigger-specific: a high-decoupling run talks about
    // drift, never a missed lifting target.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReview(SessionReview {
            decoupling: Some(Decouple {
                drift_pct: 12.0,
                cool_steady_context: true,
            }),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!(fb.category, FeedbackCategory::CorrectiveProcess);
    assert!(
        fb.message.to_lowercase().contains("drift"),
        "decoupling copy mentions drift: {}",
        fb.message
    );
    assert!(
        !fb.message.contains("Missed target") && !fb.message.contains("Came up short"),
        "decoupling must not use the missed-lift copy: {}",
        fb.message
    );
}
