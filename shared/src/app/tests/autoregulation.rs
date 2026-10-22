//! Readiness autoregulation end to end: the performance backbone reads logged
//! sets, autonomic and subjective signals only hold lift progression, RHR needs
//! persistence, pain is scoped to the movements that load it, and a hold
//! resolves into a graded return without deleting history.

use super::*;
use crate::schema::{PainDetail, PainKind, PainTrend};

fn rec(a: Adjustment, claim: ClaimId) -> Recommended<Adjustment> {
    graded(a, claim)
}

fn interval_item() -> PrescriptionView {
    PrescriptionView {
        summary: "6 × 4 min · Interval pace".into(),
        intensity_label: "Interval pace".into(),
        rep_count: 6,
        rep_volume: "4 min".into(),
        rep_amount: Some(RunAmount::Minutes(4.0)),
        ..PrescriptionView::default()
    }
}

fn pain_at(kind: PainKind, location: Option<&str>, observed_at: i64) -> ReadinessInput {
    ReadinessInput {
        observed_at,
        pain: Some(PainDetail {
            kind,
            severity: Some(6),
            trend: PainTrend::Stable,
            persists: false,
            location: location.map(str::to_string),
        }),
        ..input(ReadinessSignal::Pain, 1.0)
    }
}

#[test]
fn logged_first_set_two_rpe_over_target_lowers_that_lifts_remaining_load() {
    // The first work set of the planned lift at RPE +2 over its target cuts
    // THAT item's remaining sets (autoreg-001); other items keep their numbers.
    let (app, mut model) = planned_strength_model(true);
    let before = app.view(&model).next_session.expect("today's session");
    let idx = before
        .items
        .iter()
        .position(|i| i.load_kg.is_some())
        .expect("an anchored lift");
    let item = before.items[idx].clone();
    let load = item.load_kg.unwrap();
    app.update(
        with_entry_id(
            log_set(
                &item.exercise,
                load,
                u32::from(item.reps_low.max(1)),
                10.0,
                MON * DAY_SEC + 3_600,
            ),
            7,
        ),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let after = vm.next_session.expect("still today's session");
    let cut = after.items[idx].load_kg.expect("the cut keeps a number");
    assert!(cut < load, "remaining sets drop below {load}, got {cut}");
    for (j, it) in after.items.iter().enumerate() {
        if j != idx && !it.exercise.is_empty() {
            assert_eq!(
                it.load_kg, before.items[j].load_kg,
                "{} untouched",
                it.exercise
            );
        }
    }
    assert!(
        vm.adjustments
            .iter()
            .any(|a| a.summary.contains("first set was harder than planned")),
        "the call names the first set: {:?}",
        vm.adjustments
            .iter()
            .map(|a| &a.summary)
            .collect::<Vec<_>>()
    );
}

#[test]
fn single_low_hrv_week_holds_a_lift_at_its_load_and_eases_runs() {
    // HRV below the SWC band (no 3-day streak) only holds lift progression;
    // runs go easy with their duration kept.
    let recs = crate::autoreg::tests::adjustments(&[input(ReadinessSignal::HrvLnRmssd, -0.8)]);
    assert!(
        recs.iter()
            .any(|r| matches!(r.value, Adjustment::CapTopEnd { .. })),
        "{recs:?}"
    );
    assert!(!recs.iter().any(|r| r.value == Adjustment::DowngradeSession));
    let mut ns = session(vec![lift_item("Back squat", 100.0), interval_item()]);
    apply_adjustments_to_session(&mut ns, &recs, false);
    assert_eq!(ns.items[0].load_kg, Some(100.0), "the planned load stays");
    assert_eq!(ns.items[0].intensity, PlanIntensity::Rir { rir: 3 });
    assert!(
        ns.items[1].summary.contains("24 min"),
        "{}",
        ns.items[1].summary
    );
    assert!(
        ns.items[1].summary.contains("Easy"),
        "{}",
        ns.items[1].summary
    );
}

#[test]
fn readiness_downgrade_keeps_numbers_one_level_down() {
    // A multi-day wellness downgrade = −5 % load + one more rep in reserve,
    // never a load-less card.
    let mut ns = session(vec![lift_item("Back squat", 100.0)]);
    apply_adjustments_to_session(
        &mut ns,
        &[rec(Adjustment::DowngradeSession, claims::WELLNESS_001)],
        false,
    );
    let it = &ns.items[0];
    let kg = it.load_kg.expect("a numeric load");
    assert!((kg - 95.0).abs() < 2.6, "≈0.95 × planned, got {kg}");
    assert_eq!(it.intensity, PlanIntensity::Rir { rir: 3 });
}

#[test]
fn one_high_rhr_morning_eases_the_day_but_rest_needs_persistence_or_symptoms() {
    // One high-RHR morning is noise-level: ease the day, never rest it
    // (ready-upd-002). A 2-day streak or illness symptoms earn the rest day.
    let one = crate::autoreg::tests::adjustments(&[input(ReadinessSignal::RestingHr, 11.0)]);
    assert!(
        !one.iter().any(|r| r.value == Adjustment::RestDay),
        "{one:?}"
    );
    assert!(one.iter().any(|r| r.value == Adjustment::DowngradeSession
        && r.claim_id() == claims::AUTOREG_RHR_NOISE_001));
    let two = crate::autoreg::tests::adjustments(&[ReadinessInput {
        streak: 2,
        ..input(ReadinessSignal::RestingHr, 11.0)
    }]);
    assert!(two.iter().any(|r| r.value == Adjustment::RestDay));
    let ill = crate::autoreg::tests::adjustments(&[
        input(ReadinessSignal::RestingHr, 11.0),
        input(ReadinessSignal::Illness, 1.0),
    ]);
    assert!(ill.iter().any(|r| r.value == Adjustment::RestDay));
}

#[test]
fn im_better_clears_an_illness_hold_into_a_return_check_keeping_history() {
    // A fever holds training; "I've recovered" retracts every illness report
    // (history kept, other inputs untouched) and today's session becomes the
    // IOC return check.
    let (app, mut model) = planned_strength_model(true);
    let wellness = ReadinessInput {
        observed_at: MON * DAY_SEC + 50,
        ..input(ReadinessSignal::WellnessZ, 0.2)
    };
    for i in [
        wellness.clone(),
        ReadinessInput {
            observed_at: MON * DAY_SEC + 100,
            ..input(ReadinessSignal::Illness, 2.0)
        },
        ReadinessInput {
            observed_at: MON * DAY_SEC + 150,
            ..input(ReadinessSignal::Illness, 2.0)
        },
    ] {
        app.update(Event::SubmitReadiness(i), &mut model)
            .expect_only_render();
    }
    let vm = app.view(&model);
    assert!(vm.train_blocked);
    assert!(
        vm.hold_resolutions
            .iter()
            .any(|h| h.signal == ReadinessSignal::Illness)
    );
    assert!(vm.illness_check_prompt);

    app.update(
        Event::ResolveHold {
            signal: ReadinessSignal::Illness,
            observed_at: MON * DAY_SEC + 200,
        },
        &mut model,
    )
    .expect_only_render();
    assert_eq!(model.retracted_inputs.len(), 2, "history kept as retracted");
    assert!(model.inputs.contains(&wellness), "other inputs untouched");
    let vm = app.view(&model);
    assert!(!vm.train_blocked);
    assert!(vm.hold_resolutions.is_empty());
    let ns = vm.next_session.expect("today's session");
    assert_eq!(ns.items.len(), 1, "one return-check item");
    assert!(
        ns.items[0].summary.contains("Return check"),
        "{}",
        ns.items[0].summary
    );
}

#[test]
fn knee_pain_stops_leg_work_and_keeps_upper_body_work() {
    // A localized sharp pain stops only the movements that load it.
    let inputs = [pain_at(PainKind::SharpJoint, Some("Left knee"), 100)];
    let recs = crate::autoreg::tests::adjustments(&inputs);
    assert!(
        !recs.iter().any(|r| r.value == Adjustment::Stop),
        "{recs:?}"
    );
    assert_eq!(
        crate::autoreg::tests::resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::Pain)
    );
    let mut ns = session(vec![
        lift_item("Back squat", 100.0),
        lift_item("Bench press", 80.0),
        interval_item(),
    ]);
    apply_adjustments_to_session(&mut ns, &recs, false);
    assert!(
        ns.items[0].summary.contains("skip"),
        "{}",
        ns.items[0].summary
    );
    assert!(ns.items[0].done && ns.items[0].load_kg.is_none());
    assert_eq!(
        ns.items[1].load_kg,
        Some(80.0),
        "upper-body work goes ahead"
    );
    assert!(!ns.items[1].done);
    assert!(ns.items[2].summary.contains("skip"), "runs load the knee");

    // Through the view: not train-blocked, banner action present.
    let (app, mut model) = planned_strength_model(true);
    app.update(Event::SubmitReadiness(inputs[0].clone()), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(!vm.train_blocked);
    assert!(
        vm.hold_resolutions
            .iter()
            .any(|h| h.signal == ReadinessSignal::Pain)
    );
    // An unmapped / bare report keeps the whole-day stop.
    let bare = crate::autoreg::tests::adjustments(&[input(ReadinessSignal::Pain, 1.0)]);
    assert_eq!(bare[0].value, Adjustment::Stop);
}

#[test]
fn symptomatic_screen_blocks_training_even_when_medically_cleared() {
    // Symptoms override a medical clearance (SCREEN-ACSM-001).
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(profile_with_health(HealthScreen {
            symptomatic: true,
            medically_cleared: true,
            ..HealthScreen::default()
        })),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.train_blocked);
    assert_eq!(vm.safety_tier, Some(SafetyTier::MedicalReferral));
}

#[test]
fn one_step_wellness_dip_on_a_flat_baseline_never_fires() {
    // The SD floor: one item one step down (4.0 → 3.8) is z ≥ −0.8.
    let day = |d: i64, sleep: u8| CheckinInput {
        observed_at: d * DAY_SEC + 100,
        sleep_quality: Some(sleep),
        soreness: Some(2),
        mood: Some(4),
        fatigue: Some(2),
        stress: Some(2),
        ..CheckinInput::default()
    };
    let mut c: Vec<CheckinInput> = (0..13).map(|d| day(d, 4)).collect();
    c.push(day(13, 3));
    let out = crate::autoreg::derive_readiness(&c, 0, Some(13));
    let recs = crate::autoreg::tests::adjustments(&out.inputs);
    assert!(
        !recs.iter().any(|r| matches!(
            r.value,
            Adjustment::CapTopEnd { .. } | Adjustment::DowngradeSession
        )),
        "{recs:?}"
    );
    // A catastrophic first-week morning fires from day 1.
    let bad = CheckinInput {
        observed_at: 100,
        sleep_quality: Some(1),
        fatigue: Some(5),
        mood: Some(1),
        ..CheckinInput::default()
    };
    let out = crate::autoreg::derive_readiness(&[bad], 0, Some(0));
    assert!(
        crate::autoreg::tests::adjustments(&out.inputs)
            .iter()
            .any(|r| matches!(r.value, Adjustment::CapTopEnd { .. }))
    );
}

#[test]
fn one_mistyped_hrv_reading_is_flagged_and_ignored() {
    // 22 stable mornings, a 15 ms typo, then a normal morning that
    // does not reproduce it → the isolated spike is unreliable, no HRV call.
    let mut c: Vec<CheckinInput> = (0..22)
        .map(|d| CheckinInput {
            observed_at: d * DAY_SEC + 100,
            hrv_rmssd_ms: Some(55.0 + (d % 4) as f64),
            ..CheckinInput::default()
        })
        .collect();
    c.push(CheckinInput {
        observed_at: 22 * DAY_SEC + 100,
        hrv_rmssd_ms: Some(15.0),
        ..CheckinInput::default()
    });
    c.push(CheckinInput {
        observed_at: 23 * DAY_SEC + 100,
        hrv_rmssd_ms: Some(56.0),
        ..CheckinInput::default()
    });
    let out = crate::autoreg::derive_readiness(&c, 0, Some(23));
    assert_eq!(out.hrv_unreliable, vec![22 * DAY_SEC + 100]);
    assert!(
        !crate::autoreg::tests::adjustments(&out.inputs)
            .iter()
            .any(|r| {
                r.claim_id() == claims::HRV_001 || matches!(r.value, Adjustment::CapTopEnd { .. })
            })
    );
}

#[test]
fn a_sustained_hrv_drop_is_a_signal_not_an_artifact() {
    // Three consecutive −30% mornings after a tight baseline are a
    // reproduced shift (illness onset), never "unreliable readings".
    let mut c: Vec<CheckinInput> = (0..22)
        .map(|d| CheckinInput {
            observed_at: d * DAY_SEC + 100,
            hrv_rmssd_ms: Some([51.0, 55.0, 59.0, 55.0][(d % 4) as usize]),
            ..CheckinInput::default()
        })
        .collect();
    for d in 22..25 {
        c.push(CheckinInput {
            observed_at: d * DAY_SEC + 100,
            hrv_rmssd_ms: Some(38.5),
            ..CheckinInput::default()
        });
    }
    let out = crate::autoreg::derive_readiness(&c, 0, Some(24));
    assert!(out.hrv_unreliable.is_empty(), "{:?}", out.hrv_unreliable);
    assert!(
        !out.collecting
            .iter()
            .any(|b| b.kind == crate::autoreg::BaselineKind::Suspended)
    );
    let hrv = out
        .inputs
        .iter()
        .find(|i| i.signal == ReadinessSignal::HrvLnRmssd)
        .expect("HRV channel emits");
    assert!(hrv.value < -0.5, "7-day mean z {}", hrv.value);
}

#[test]
fn pain_scoping_keeps_only_movements_that_cannot_load_the_painful_part() {
    // Conservative allow-lists per region.
    use crate::autoreg::{PainRegion, item_spared_by_pain};
    for lift in [
        "Back Squat",
        "Front Squat",
        "Overhead Squat",
        "Goblet squat",
    ] {
        assert!(!item_spared_by_pain(PainRegion::UpperLimb, lift), "{lift}");
    }
    assert!(item_spared_by_pain(PainRegion::UpperLimb, "Leg press"));
    assert!(
        item_spared_by_pain(PainRegion::UpperLimb, ""),
        "runs stay for arm pain"
    );
    for lift in ["Leg Press", "Box Jump", "Hip thrust", "Bench press", ""] {
        assert!(!item_spared_by_pain(PainRegion::Back, lift), "{lift:?}");
    }
    assert!(item_spared_by_pain(PainRegion::Back, "Lat pulldown"));
    assert!(item_spared_by_pain(PainRegion::LowerLimb, "Bench press"));
    assert!(!item_spared_by_pain(PainRegion::LowerLimb, ""));
}
