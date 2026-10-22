//! Readiness context wired into feedback and reference rows.

use super::*;

#[test]
fn e1rm_dip_view_shows_rpe_cap_row() {
    // autoreg-006 second clause reaches the shell as its own row.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::EstimatedOneRm, 0.93)),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(
        vm.adjustments
            .iter()
            .any(|a| a.summary.contains("Stop 1 rep earlier than planned"))
    );
    assert!(!vm.train_blocked, "an RPE cap modifies, never blocks");
}

#[test]
fn high_load_block_profile_arms_the_saturation_guard() {
    // autoreg-029 through the full view: high HRV + under-target RPE would
    // add load, but a high-load-block profile strips the increase.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(Profile {
            high_load_block: true,
            ..sample_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Rpe, -2.0)),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::HrvLnRmssd, 1.0)),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(
        !vm.adjustments
            .iter()
            .any(|a| a.summary.contains("ahead of plan")),
        "no auto load-add under parasympathetic saturation"
    );

    // Same readings without the block flag do add load.
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    assert!(
        app.view(&model)
            .adjustments
            .iter()
            .any(|a| a.summary.contains("ahead of plan"))
    );
}

#[test]
fn interval_mastery_review_reaches_feedback() {
    let app = Engine;
    let mut model = Model::default();
    let review = SessionReview {
        interval: Some(IntervalExec {
            target_paces_met: true,
            rpe_at_or_below_target: true,
        }),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!(fb.category, FeedbackCategory::PositiveMastery);
    assert!(
        fb.anchor_mastery,
        "praise must anchor a concrete achievement"
    );
    // Reps over target RPE fall through to other arms, not mastery.
    let review = SessionReview {
        interval: Some(IntervalExec {
            target_paces_met: true,
            rpe_at_or_below_target: false,
        }),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();
    assert_ne!(
        app.view(&model).feedback.expect("feedback").category,
        FeedbackCategory::PositiveMastery
    );
}

#[test]
fn female_bsi_referral_appends_clinician_prompt() {
    // feedback-035: female profile + bone-stress referral → the gentle
    // menstrual/nutrition prompt rides on the CONCERN_INJURY message.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(Profile {
            female: Some(true),
            ..sample_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    let review = SessionReview {
        bone_pain_red_flag: true,
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!(fb.category, FeedbackCategory::ConcernInjury);
    assert!(
        fb.message.contains("how much you eat"),
        "prompt appended: {}",
        fb.message
    );

    // Male/unset profile: no prompt on the same referral.
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback present");
    assert!(!fb.message.contains("how much you eat"));
    // And a female non-injury review never gets it either (the weekly
    // referral is cleared first: a session review alone never wipes it).
    app.update(
        Event::SetProfile(Profile {
            female: Some(true),
            ..sample_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    app.update(Event::ClearWeeklyCheckin, &mut model)
        .expect_only_render();
    app.update(Event::SubmitReview(SessionReview::default()), &mut model)
        .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback present");
    assert!(!fb.message.contains("how much you eat"));
}

#[test]
fn feedback_verbosity_tracks_training_age() {
    // feedback-023/024: WeekToWeek cadence (intermediate) → beginner-style
    // conservative defaults; MonthToMonth (advanced) → 2-3 metric density.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    app.update(Event::SubmitReview(SessionReview::default()), &mut model)
        .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!((fb.max_takeaways, fb.max_metrics), (1, 1));
    assert!(fb.rationale_mandatory && fb.minimize_jargon);

    app.update(
        Event::SetProfile(Profile {
            progression_cadence: ProgressionCadence::MonthToMonth,
            ..sample_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback present");
    assert_eq!((fb.max_takeaways, fb.max_metrics), (1, 3));
    assert!(!fb.minimize_jargon);
}

#[test]
fn overtraining_duration_condition_flows_through_review() {
    // feedback-036: two signals over half a week must NOT fire the
    // recovery concern; over a full week it must.
    let app = Engine;
    let mut model = Model::default();
    let review = SessionReview {
        overtraining_signal_count: 2,
        overtraining_signal_weeks: Some(0.5),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();
    assert!(
        app.view(&model)
            .feedback
            .is_none_or(|f| f.category != FeedbackCategory::ConcernRecovery),
        "a single noisy night must not fire the recovery concern"
    );
    let review = SessionReview {
        overtraining_signal_count: 2,
        overtraining_signal_weeks: Some(1.5),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();
    assert_eq!(
        app.view(&model).feedback.expect("feedback").category,
        FeedbackCategory::ConcernRecovery
    );
}

#[test]
fn short_effort_decoupling_readiness_is_discarded() {
    // File 06 validity gate through the view: a 15-minute effort's
    // decoupling never downgrades; >20 min does.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            effort_min: Some(15.0),
            ..input(ReadinessSignal::AerobicDecoupling, 14.0)
        }),
        &mut model,
    )
    .expect_only_render();
    assert!(app.view(&model).adjustments.is_empty());

    app.update(Event::ClearReadiness, &mut model)
        .expect_only_render();
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            effort_min: Some(40.0),
            ..input(ReadinessSignal::AerobicDecoupling, 14.0)
        }),
        &mut model,
    )
    .expect_only_render();
    assert!(!app.view(&model).adjustments.is_empty());
}

#[test]
fn duration_moderator_row_rides_on_expected_interference() {
    // hybrid-004: the duration-strongest-moderator row appears only when
    // interference is expected for this profile.
    let app = Engine;
    let mut model = Model::default();
    // sample_profile: 5 running days → interference expected.
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    assert!(
        app.view(&model)
            .guidance
            .iter()
            .any(|g| g.summary.contains("Run length matters most"))
    );
    // 2 easy days a week (no quality run fits a 2-day week) → no interference,
    // no moderator row.
    app.update(
        Event::SetProfile(Profile {
            running_days_per_week: 2,
            running_km_per_week: 20.0,
            ..sample_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    assert!(
        !app.view(&model)
            .guidance
            .iter()
            .any(|g| g.summary.contains("Run length matters most"))
    );
}

#[test]
fn reference_carries_new_hybrid_and_safety_rows() {
    let vm = Engine.view(&Model::default());
    for needle in [
        "freshest",
        "eat plenty of carbs",
        "keep strength and endurance work apart",
        "on tendons has not been studied",
        "under-eating (RED-S)",
    ] {
        assert!(
            vm.reference
                .iter()
                .any(|g| g.summary.to_lowercase().contains(&needle.to_lowercase())),
            "reference row missing: {needle}"
        );
    }
    // Sections stay contiguous in the reference list too.
    assert_sections_contiguous(&vm.reference, &"reference");
}
