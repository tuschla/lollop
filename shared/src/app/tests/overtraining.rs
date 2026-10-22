//! Overtraining escalations carried by the review, and the unscheduled deload
//! row they raise.

use super::*;

#[test]
fn nfor_cluster_review_defers_at_medical_referral_tier() {
    // autoreg-042: ≥2 wk decrement + ≥2 wellness domains → mandatory
    // recovery + professional referral, visible and blocking.
    let app = Engine;
    let mut model = Model::default();
    let review = SessionReview {
        decline_weeks: Some(2),
        suppressed_wellness_domains: Some(2),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.safety_tier, Some(SafetyTier::MedicalReferral));
    assert!(vm.train_blocked);
    assert!(
        vm.review_adjustments
            .iter()
            .any(|a| a.summary.contains("recovery") && a.evidence.safety_critical)
    );
    // A routine session review never lifts it…
    app.update(Event::SubmitReview(SessionReview::default()), &mut model)
        .expect_only_render();
    assert!(app.view(&model).train_blocked);
    // …ClearWeeklyCheckin does.
    app.update(Event::ClearWeeklyCheckin, &mut model)
        .expect_only_render();
    assert!(!app.view(&model).train_blocked);
}

#[test]
fn decline_despite_deload_uses_the_strong_rule() {
    // safety-042 outranks the ExpertOpinion cluster when the decline
    // survived a deload: one deferral, graded Strong (Meeusen).
    let app = Engine;
    let mut model = Model::default();
    let review = SessionReview {
        decline_weeks: Some(2),
        suppressed_wellness_domains: Some(2),
        despite_deload: true,
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    let defers: Vec<_> = vm
        .review_adjustments
        .iter()
        .filter(|a| a.summary.contains("defer") || a.summary.contains("professional"))
        .collect();
    assert_eq!(defers.len(), 1, "exactly one escalation fires");
    assert_eq!(defers[0].evidence.grade, Some(EvidenceGrade::Strong));
}

#[test]
fn sub_threshold_decline_reviews_add_nothing() {
    let app = Engine;
    let mut model = Model::default();
    let review = SessionReview {
        decline_weeks: Some(1),
        suppressed_wellness_domains: Some(1),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.review_adjustments.is_empty());
    assert!(!vm.train_blocked);
    assert_eq!(vm.safety_tier, None);
}

#[test]
fn mrv_cluster_and_threshold_retest_surface_in_review() {
    let app = Engine;
    let mut model = Model::default();
    let review = SessionReview {
        mrv_sign_cluster: true,
        pace_at_hr_improved_weeks: Some(2),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(
        vm.review_adjustments
            .iter()
            .any(|a| a.summary.starts_with("Lighter week")),
        "autoreg-025 MRV deload surfaces"
    );
    assert!(
        vm.review_adjustments
            .iter()
            .any(|a| a.summary.contains("Re-test")),
        "autoreg-032 threshold re-test cue surfaces"
    );
    // Neither blocks training.
    assert!(!vm.train_blocked);
    // One week of improvement is below the autoreg-032 bound → no cue.
    let review = SessionReview {
        pace_at_hr_improved_weeks: Some(1),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();
    assert!(app.view(&model).review_adjustments.is_empty());
}

#[test]
fn unscheduled_deload_row_rides_on_sustained_overtraining_signals() {
    // Two signs over half a week is a noisy patch, not under-recovery
    // (feedback-036 duration condition): no concern, no easier week.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitWeeklyCheckin(SessionReview {
            overtraining_signal_count: 2,
            overtraining_signal_weeks: Some(0.5),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(
        vm.review_adjustments.is_empty(),
        "{:#?}",
        vm.review_adjustments
    );
    assert!(vm.feedback.is_none(), "{:?}", vm.feedback);
    // Lasting a week or more: one easier-week row carries the action.
    app.update(
        Event::SubmitWeeklyCheckin(SessionReview {
            overtraining_signal_count: 2,
            overtraining_signal_weeks: Some(1.0),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let rows: Vec<_> = vm
        .review_adjustments
        .iter()
        .filter(|a| a.summary.contains("Lighter week"))
        .collect();
    assert_eq!(rows.len(), 1, "{:#?}", vm.review_adjustments);
}
