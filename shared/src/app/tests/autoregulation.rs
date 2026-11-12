//! Readiness autoregulation end to end: the performance backbone reads logged
//! sets, autonomic and subjective signals only hold lift progression, RHR needs
//! persistence, pain is scoped to the movements that load it, and a hold
//! resolves into a graded return without deleting history.

use super::*;
use crate::schema::{PainDetail, PainKind, PainTrend};

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

/// `signal` at `value`, suppressed for `streak` days, reported on `day`.
fn reported(signal: ReadinessSignal, value: f64, streak: u8, day: i64) -> Event {
    Event::SubmitReadiness(ReadinessInput {
        observed_at: day * DAY_SEC + 60,
        streak,
        ..input(signal, value)
    })
}

/// The item of today's session for `exercise`.
fn hero_item(app: &Engine, model: &Model, exercise: &str) -> PrescriptionView {
    app.view(model)
        .next_session
        .expect("today's session")
        .items
        .into_iter()
        .find(|i| i.exercise == exercise)
        .unwrap_or_else(|| panic!("{exercise} today"))
}

#[test]
fn single_low_hrv_week_holds_a_lift_at_its_load_and_eases_runs() {
    // HRV below the SWC band (no 3-day streak) only holds lift progression:
    // the lift keeps its planned load, one rep further from failure. Runs go
    // easy with their distance kept.
    let (app, mut model) = planned_strength_model(true);
    app.update(
        reported(ReadinessSignal::HrvLnRmssd, -0.8, 0, MON),
        &mut model,
    )
    .expect_only_render();
    let squat = hero_item(&app, &model, "Back Squat");
    assert_eq!(squat.load_kg, Some(117.5), "the planned load stays");
    assert_eq!(squat.target_rpe, Some(6.0), "RPE 7 one rep further off");

    let runner = Profile {
        goal_distance: GoalDistance::FiveK,
        weekly_sets: 0,
        running_days_per_week: 4,
        running_km_per_week: 40.0,
        ..strength_profile()
    };
    let (app, mut model) = planned_on(runner, [], MON);
    let tempo = app
        .view(&model)
        .week_plan
        .into_iter()
        .find(|s| s.session_type == SessionType::Run(RunSessionType::Tempo))
        .expect("a tempo day");
    set_today(&app, &mut model, tempo.epoch_day);
    app.update(
        reported(ReadinessSignal::HrvLnRmssd, -0.8, 0, tempo.epoch_day),
        &mut model,
    )
    .expect_only_render();
    let run = app.view(&model).next_session.expect("the tempo day").items[0].clone();
    assert_eq!(
        run.face_lead, tempo.items[0].face_lead,
        "the distance stays"
    );
    assert!(
        run.intensity_label.starts_with("Easy")
            && !tempo.items[0].intensity_label.starts_with("Easy"),
        "{} → {}",
        tempo.items[0].intensity_label,
        run.intensity_label
    );
}

#[test]
fn readiness_downgrade_keeps_numbers_one_level_down() {
    // A multi-day wellness downgrade = −5 % load + one more rep in reserve,
    // never a load-less card: 117.5 kg less 5 %, down to the 2.5 kg grid.
    let (app, mut model) = planned_strength_model(true);
    app.update(
        reported(
            ReadinessSignal::WellnessZ,
            -1.5,
            crate::autoreg::WELLNESS_MULTIDAY_STREAK,
            MON,
        ),
        &mut model,
    )
    .expect_only_render();
    let squat = hero_item(&app, &model, "Back Squat");
    assert_eq!(squat.load_kg, Some(110.0));
    assert_eq!(squat.target_rpe, Some(6.0), "RPE 7 one rep further off");
}

#[test]
fn knee_pain_stops_leg_work_and_keeps_upper_body_work() {
    // A localized sharp pain stops only the movements that load it.
    let knee = pain_at(PainKind::SharpJoint, Some("Left knee"), MON * DAY_SEC + 60);
    assert_eq!(
        crate::autoreg::tests::resolve_safety_for_goal(std::slice::from_ref(&knee), None),
        Some(SafetyTier::Pain)
    );
    let (app, mut model) = planned_on(
        strength_profile(),
        [
            log_set("Back Squat", 120.0, 3, 8.0, (MON - 1) * DAY_SEC),
            log_set("Bench press", 80.0, 5, 8.0, (MON - 1) * DAY_SEC),
        ],
        MON,
    );
    app.update(Event::SubmitReadiness(knee.clone()), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.hold_reason.is_none(), "{:?}", vm.hold_reason);
    assert!(
        vm.hold_resolutions
            .iter()
            .any(|h| h.signal == ReadinessSignal::Pain)
    );
    for leg in ["Back Squat", "Deadlift"] {
        let it = hero_item(&app, &model, leg);
        assert_eq!(it.status, ItemStatus::SkippedPain, "{it:?}");
        assert!(
            it.summary.contains("skip") && it.load_kg.is_none(),
            "{it:?}"
        );
    }
    let bench = hero_item(&app, &model, "Bench press");
    assert!(
        bench.load_kg.is_some() && bench.status == ItemStatus::Open,
        "upper-body work goes ahead: {bench:?}"
    );
    // Runs load the knee.
    let (app, mut model) = planned_on(running_profile(), [], MON);
    app.update(Event::SubmitReadiness(knee), &mut model)
        .expect_only_render();
    let run = app.view(&model).next_session.expect("today's run").items[0].clone();
    assert_eq!(run.status, ItemStatus::SkippedPain, "{run:?}");
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
    assert!(vm.hold_reason.is_some());
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
    use crate::autoreg::item_spared_by_pain;
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
    // Matched on the folded name, like the e1RM key: a Turkish keyboard's
    // caps-lock "LAT PULLDOWN" or "LEG EXTENSİON" is the same movement.
    assert!(item_spared_by_pain(PainRegion::Back, "LAT PULLDOWN"));
    assert!(item_spared_by_pain(PainRegion::UpperLimb, "LEG EXTENSİON"));
    assert!(item_spared_by_pain(PainRegion::LowerLimb, "Bench press"));
    assert!(!item_spared_by_pain(PainRegion::LowerLimb, ""));
}
