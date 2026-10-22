//! Stale performance signals expire; protective ones (an e1RM decline, an
//! active review deload) keep holding.

use super::*;

fn rpe_input_at(value: f64, observed_at: i64) -> ReadinessInput {
    ReadinessInput {
        signal: ReadinessSignal::Rpe,
        value,
        observed_at,
        streak: 0,
        pain: None,
        effort_min: None,
    }
}

#[test]
fn a_stale_felt_easy_rpe_expires_but_a_fresh_one_still_raises_load() {
    let app = Engine;
    let today = 100i64;
    let has_increase = |vm: &ViewModel| {
        vm.adjustments
            .iter()
            .any(|a| a.summary.contains("ahead of plan"))
    };

    // Fresh RPE −2 (yesterday) → a load increase is proposed.
    let mut fresh = Model::default();
    app.update(
        Event::SubmitReadiness(rpe_input_at(-2.0, (today - 1) * DAY_SEC)),
        &mut fresh,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
        &mut fresh,
    )
    .expect_only_render();
    assert!(
        has_increase(&app.view(&fresh)),
        "a fresh felt-easy RPE still raises load"
    );

    // Stale RPE −2 (21 days ago) → expired, no increase.
    let mut stale = Model::default();
    app.update(
        Event::SubmitReadiness(rpe_input_at(-2.0, (today - 21) * DAY_SEC)),
        &mut stale,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
        &mut stale,
    )
    .expect_only_render();
    assert!(
        !has_increase(&app.view(&stale)),
        "a 21-day-old RPE must expire"
    );

    // With no `today` reference (no clock) the signal is NOT expired.
    let mut noclock = Model::default();
    app.update(
        Event::SubmitReadiness(rpe_input_at(-2.0, (today - 21) * DAY_SEC)),
        &mut noclock,
    )
    .expect_only_render();
    assert!(
        has_increase(&app.view(&noclock)),
        "without a today reference the core keeps the signal"
    );

    // A 21-day-old PAIN report must NOT expire: safety persists (HARD RULE 3).
    let mut stale_pain = Model::default();
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::Pain,
            value: 1.0,
            observed_at: (today - 21) * DAY_SEC,
            streak: 0,
            pain: None,
            effort_min: None,
        }),
        &mut stale_pain,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
        &mut stale_pain,
    )
    .expect_only_render();
    assert!(
        app.view(&stale_pain).train_blocked,
        "a stale pain report must still hold - safety never expires"
    );
}

#[test]
fn a_stale_e1rm_decline_stays_protective_while_a_stale_increase_expires() {
    // The perf-signal expiry is direction-aware, not signal-type-only. A
    // stale INCREASE-driving e1RM reading (ratio > 1.0) still expires (dropping
    // optimism is conservative), but a stale PROTECTIVE e1RM decline (ratio <
    // 1.0) must NEVER expire into optimism (HARD RULE 3): the reduce still fires.
    let app = Engine;
    let today = 100i64;
    let e1rm_at = |ratio: f64, observed_at: i64| ReadinessInput {
        signal: ReadinessSignal::EstimatedOneRm,
        value: ratio,
        observed_at,
        streak: 0,
        pain: None,
        effort_min: None,
    };
    let has_reduce = |vm: &ViewModel| {
        vm.adjustments
            .iter()
            .any(|a| a.summary.contains("Drop the weight"))
    };
    let has_increase = |vm: &ViewModel| {
        vm.adjustments
            .iter()
            .any(|a| a.summary.contains("ahead of plan"))
    };

    // A 21-day-old e1RM DECLINE (ratio 0.85) stays protective: the reduce fires
    // (a type-only expiry would drop it → no reduce).
    let mut decline = Model::default();
    app.update(
        Event::SubmitReadiness(e1rm_at(0.85, (today - 21) * DAY_SEC)),
        &mut decline,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
        &mut decline,
    )
    .expect_only_render();
    assert!(
        has_reduce(&app.view(&decline)),
        "a stale protective e1RM decline must NOT expire into optimism"
    );

    // A 21-day-old e1RM INCREASE (ratio 1.08) still expires: no load increase.
    let mut increase = Model::default();
    app.update(
        Event::SubmitReadiness(e1rm_at(1.08, (today - 21) * DAY_SEC)),
        &mut increase,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
        &mut increase,
    )
    .expect_only_render();
    assert!(
        !has_increase(&app.view(&increase)),
        "a stale increase-driving e1RM must still expire (conservative)"
    );
}

// ── A review-channel deload folds into the rendered next session ──
#[test]
fn a_review_deload_reduces_the_rendered_next_session_load() {
    let (app, mut model) = planned_strength_model(true);
    let squat_load = |vm: &ViewModel| {
        vm.next_session
            .as_ref()
            .unwrap()
            .items
            .iter()
            .find(|i| i.exercise.eq_ignore_ascii_case("Back Squat"))
            .unwrap()
            .load_kg
            .unwrap()
    };
    let baseline = squat_load(&app.view(&model));

    // An rpe-load-gap over ≥2 sessions → a standard deload (−10 % load), a
    // NON-blocking review adjustment.
    let review = SessionReview {
        rpe_load_gap_sessions: Some(2),
        ..Default::default()
    };
    app.update(Event::SubmitReview(review), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert!(
        squat_load(&vm) < baseline,
        "the review deload must fold into the rendered session load: {} !< {baseline}",
        squat_load(&vm)
    );
    assert_eq!(
        vm.next_session.as_ref().unwrap().status,
        SessionStatus::Adjusted
    );
}

#[test]
fn a_stale_review_deload_expires() {
    let (app, mut model) = planned_strength_model(true);
    let today = 500i64;
    // A 1-week deload trigger recorded 8 days ago has aged out (7-day expiry).
    app.update(
        Event::SubmitReview(SessionReview {
            rpe_load_gap_sessions: Some(2),
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
            .any(|a| a.summary.contains("Lighter week")),
        "an 8-day-old 1-week deload has expired: {:?}",
        vm.review_adjustments
            .iter()
            .map(|a| &a.summary)
            .collect::<Vec<_>>()
    );
    assert!(
        vm.weekly_checkin_present,
        "the weekly check-in itself is still present (clearable)"
    );

    // A fresh trigger of the same review (dated today) still surfaces.
    app.update(
        Event::SubmitReview(SessionReview {
            rpe_load_gap_sessions: Some(2),
            observed_at: today * DAY_SEC,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm2 = app.view(&model);
    assert!(
        vm2.review_adjustments
            .iter()
            .any(|a| a.summary.contains("Lighter week")),
        "a fresh deload trigger still surfaces"
    );
}

// ── An active review deload outranks a readiness load increase ──
#[test]
fn an_active_review_deload_outranks_a_readiness_load_increase() {
    // A readiness RPE undershoot recommends "Increase load"; the session review
    // recommends a protective deload. The protective action wins: no load-increase
    // row survives and the headline is the deload, not the increase.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Rpe, -2.0)),
        &mut model,
    )
    .expect_only_render();
    assert!(
        app.view(&model)
            .adjustments
            .iter()
            .any(|a| a.summary.contains("ahead of plan")),
        "readiness alone recommends a load increase"
    );
    app.update(
        Event::SubmitReview(SessionReview {
            rpe_load_gap_sessions: Some(2),
            ..Default::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(
        !vm.adjustments
            .iter()
            .any(|a| a.summary.contains("ahead of plan")),
        "an active review deload strips the readiness load increase: {:?}",
        vm.adjustments
            .iter()
            .map(|a| &a.summary)
            .collect::<Vec<_>>()
    );
    assert!(
        !vm.today_headline.summary.contains("ahead of plan"),
        "the headline must not be the load increase: {}",
        vm.today_headline.summary
    );
    assert!(
        vm.today_headline.summary.contains("Lighter week"),
        "the protective deload leads the headline: {}",
        vm.today_headline.summary
    );
}
