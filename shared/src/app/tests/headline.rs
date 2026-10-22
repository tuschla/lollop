//! The Today headline: safety holds outrank adjustments, which outrank
//! feedback; neutral verdicts never headline.

use super::*;

#[test]
fn headline_propagates_the_adjustments_why() {
    // The today headline carries the winning adjustment's why? triad so the
    // Today card's "why?" is populated without the shell re-deriving it.
    let app = Engine;
    let mut model = Model::default();
    // A lone HRV suppression → DowngradeSession adjustment (HRV-001).
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::HrvLnRmssd, -1.2)),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert_eq!(vm.today_headline.kind, HeadlineKind::Adjustment);
    assert!(
        !vm.today_headline.why.grade_note.is_empty(),
        "headline carries a populated why?"
    );
}

#[test]
fn headline_defaults_to_train_as_planned_with_no_evidence_claim() {
    let app = Engine;
    let model = Model::default();
    let vm = app.view(&model);
    assert_eq!(vm.today_headline.kind, HeadlineKind::AllClear);
    assert!(vm.today_headline.summary.contains("Train as planned"));
    // The all-clear asserts the ABSENCE of a triggered rule: no evidence
    // tag may be fabricated for it (HARD RULE 1/2).
    assert!(vm.today_headline.evidence.grade.is_none());
    assert!(vm.today_headline.evidence.citation.is_empty());
}

#[test]
fn headline_prioritizes_safety_hold_over_adjustments_and_feedback() {
    let app = Engine;
    let mut model = Model::default();
    // A routine adjustment trigger AND a hard stop: the stop must win.
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::HrvLnRmssd, -1.0)),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitReview(SessionReview {
            lift: Some(LiftExec {
                reps_met: true,
                rir_actual: 2,
                rir_target: 2,
            }),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.train_blocked);
    assert_eq!(vm.today_headline.kind, HeadlineKind::SafetyHold);
    assert!(
        vm.today_headline.summary.contains("Stop"),
        "{}",
        vm.today_headline.summary
    );
    assert!(vm.today_headline.evidence.safety_critical);
    assert!(vm.today_headline.evidence.grade.is_some());
}

#[test]
fn one_low_hrv_morning_reads_this_morning_not_this_week() {
    // One very low morning drags the 7-day mean below the
    // band; the call must not claim a week of low HRV.
    let hrv = |streak: u8| {
        let app = Engine;
        let mut model = Model::default();
        let mut i = input(ReadinessSignal::HrvLnRmssd, -1.0);
        i.streak = streak;
        app.update(Event::SubmitReadiness(i), &mut model)
            .expect_only_render();
        app.view(&model)
    };
    let one = hrv(1);
    assert!(
        one.today_headline
            .summary
            .starts_with("HRV was below your normal range on your latest reading"),
        "{}",
        one.today_headline.summary
    );
    assert!(
        !one.adjustments
            .iter()
            .any(|a| a.summary.contains("this week"))
    );
    let week = hrv(2);
    assert!(
        week.today_headline
            .summary
            .starts_with("HRV has been below your normal range this week"),
        "{}",
        week.today_headline.summary
    );
}

#[test]
fn headline_falls_to_adjustment_then_feedback() {
    let app = Engine;
    let mut model = Model::default();
    // Non-blocking readiness trigger → adjustment headline.
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::HrvLnRmssd, -1.0)),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert_eq!(vm.today_headline.kind, HeadlineKind::Adjustment);
    // AUTOREG-HRV-LIFT-001: an HRV dip holds lift progression and eases runs;
    // the call still leads the headline as an adjustment.
    assert!(
        vm.today_headline
            .summary
            .contains("HRV has been below your normal range"),
        "{}",
        vm.today_headline.summary
    );
    assert!(vm.today_headline.evidence.grade.is_some());

    // Readiness cleared, review present → the session feedback leads.
    app.update(Event::ClearReadiness, &mut model)
        .expect_only_render();
    app.update(
        Event::SubmitReview(SessionReview {
            lift: Some(LiftExec {
                reps_met: true,
                rir_actual: 2,
                rir_target: 2,
            }),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert_eq!(vm.today_headline.kind, HeadlineKind::Feedback);
    assert_eq!(
        vm.today_headline.summary,
        vm.feedback.as_ref().unwrap().message
    );
}

#[test]
fn headline_ignores_a_neutral_review_verdict_and_keeps_the_prescription() {
    // A VBT "Hold" verdict (bar speed inside the ±0.06 m/s band) is a
    // graded "no change" call carrying no real Adjustment. `model.review` is
    // a persisted singleton, so it must NOT displace the prescription
    // headline — the next session leads, and the neutral row still renders.
    let (app, mut model) = planned_strength_model(true);
    app.update(
        Event::SubmitReview(SessionReview {
            mcv_delta_m_s: Some(0.0),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    let hold = vm
        .review_adjustments
        .iter()
        .find(|a| a.summary.contains("hold planned weights"))
        .expect("the neutral VBT hold row still renders in the review");
    assert!(hold.neutral, "the hold verdict is marked neutral");
    assert_eq!(
        vm.today_headline.kind,
        HeadlineKind::Prescription,
        "a neutral verdict must not displace the next-session prescription"
    );
    assert!(vm.today_headline.summary.is_empty());
}

#[test]
fn a_sound_aerobic_base_verdict_does_not_headline() {
    // A "sound aerobic base" decoupling verdict (<5% drift) is informational,
    // not a prescription change, so it must not headline over the session.
    let (app, mut model) = planned_strength_model(true);
    app.update(
        Event::SubmitReview(SessionReview {
            decoupling: Some(Decouple {
                drift_pct: 3.0,
                cool_steady_context: true,
            }),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let sound = vm
        .review_adjustments
        .iter()
        .find(|a| a.summary.contains("your endurance is solid"))
        .expect("the sound-base verdict renders in the review");
    assert!(sound.neutral, "the sound-base verdict is marked neutral");
    assert_ne!(
        vm.today_headline.kind,
        HeadlineKind::Adjustment,
        "an informational aerobic-base verdict must not headline as an adjustment"
    );
}

// ── A tolerable-pain day's headline is the pain call, and the session
//        prescription is actually capped (ModifyAndMonitor), not full-load ──
#[test]
fn tolerable_pain_headline_is_the_pain_call_and_caps_the_session() {
    let (app, mut model) = planned_strength_model(true);
    // Report tolerable, stable tendon pain (sev 3) → PainGate::Adjust
    // (ModifyAndMonitor), not a hard stop.
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::Pain,
            value: 1.0,
            observed_at: MON * DAY_SEC,
            streak: 0,
            pain: Some(crate::schema::PainDetail {
                kind: crate::schema::PainKind::TendonLoadRelated,
                severity: Some(3),
                trend: crate::schema::PainTrend::Stable,
                persists: false,
                location: Some("Left elbow".into()),
            }),
            effort_min: None,
        }),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    assert!(!vm.train_blocked, "tolerable pain must not block");
    // The headline is the adjustment (pain) call, NOT the full-load session.
    assert_eq!(
        vm.today_headline.kind,
        HeadlineKind::Adjustment,
        "an active adjustment outranks the prescription rung"
    );
    // The session is capped: no heavy load shown, and it is marked adjusted.
    let ns = vm.next_session.expect("still a next session");
    assert_eq!(ns.status, SessionStatus::Adjusted);
    for it in ns.items.iter().filter(|i| !i.exercise.is_empty()) {
        assert!(
            it.load_kg.is_none(),
            "a lift must not show a heavy load under active pain: {}",
            it.summary
        );
    }
}
