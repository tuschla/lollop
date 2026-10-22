//! Stage-0 onboarding gates flow into the view (File 08 onboard-050), including
//! the youth gate derived from age alone.

use super::*;

#[test]
fn pregnancy_screen_gates_the_view_at_medical_referral() {
    // safety-045: SAFE-PREG-001 actually emitted, visible as a Safety
    // guidance row, MedicalReferral tier, train blocked: plus the
    // safety-047 avoid-list row.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(profile_with_health(HealthScreen {
            pregnant: true,
            ..HealthScreen::default()
        })),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.safety_tier, Some(SafetyTier::MedicalReferral));
    assert!(vm.train_blocked, "a deferral gate blocks training");
    let safety_rows: Vec<_> = vm
        .guidance
        .iter()
        .filter(|g| g.section == "Safety")
        .collect();
    assert!(
        safety_rows
            .iter()
            .any(|g| g.summary.contains("pregnant") && g.summary.contains("150 minutes")),
        "the SAFE-PREG-001 deferral row must surface with the reference target"
    );
    assert!(
        safety_rows
            .iter()
            .any(|g| g.summary.contains("holding your breath") && g.summary.contains("2500 m")),
        "the safety-047 avoid-list must accompany the deferral"
    );
    assert!(safety_rows.iter().all(|g| g.evidence.safety_critical));
    // safety-046: the full warning-sign list reaches the user.
    assert!(
        safety_rows
            .iter()
            .any(|g| g.summary.contains("calf pain") && g.summary.contains("baby moving less")),
        "every pregnancy warning sign must surface"
    );
    // Gates lead the guidance list (screen BEFORE any prescription).
    assert_eq!(vm.guidance[0].section, "Safety");
}

#[test]
fn gated_profiles_get_no_prescriptive_guidance_or_calculator_numbers() {
    // onboard-050 / safety-011 / safety-045 / safety-049: gates run BEFORE any
    // prescription, so no %1RM, 1RM-testing, volume-ramp or running-cap row,
    // and the plan calculators show the hold instead of numbers.
    for health in [
        HealthScreen {
            youth: true,
            ..HealthScreen::default()
        },
        HealthScreen {
            pregnant: true,
            ..HealthScreen::default()
        },
        HealthScreen {
            reds_signal: true,
            ..HealthScreen::default()
        },
        HealthScreen {
            parq_positive: true,
            ..HealthScreen::default()
        },
    ] {
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::SetProfile(profile_with_health(health.clone())),
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::PlanHypertrophyMeso {
                muscle: "chest".into(),
                weeks: 4,
                not_growing: false,
                recovering_easily: false,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::ComputeApre {
                scheme: crate::autoreg::ApreScheme::Apre6,
                reps: 10,
                current_load_kg: 100.0,
            },
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(
            vm.guidance
                .iter()
                .all(|g| matches!(g.section.as_str(), "Safety" | "Profile" | "Environment")),
            "{health:?}: only safety/profile/environment rows, got {:?}",
            vm.guidance.iter().map(|g| &g.section).collect::<Vec<_>>()
        );
        assert_eq!(vm.hypertrophy_plan.len(), 1, "{health:?}");
        assert!(!vm.hypertrophy_plan[0].summary.contains("sets"));
        assert_eq!(vm.apre.len(), 1, "{health:?}");
        assert!(
            vm.apre[0].summary.contains("on hold"),
            "{}",
            vm.apre[0].summary
        );
        assert!(vm.apre[0].evidence.safety_critical);
    }
}

#[test]
fn parq_gate_clears_with_medical_clearance() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(profile_with_health(HealthScreen {
            parq_positive: true,
            ..HealthScreen::default()
        })),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.train_blocked);
    assert!(
        vm.guidance
            .iter()
            .any(|g| g.summary.contains("health answers") && g.summary.contains("doctor's OK"))
    );

    // Clearance lifts the gate (safety-044): programming resumes.
    app.update(
        Event::SetProfile(profile_with_health(HealthScreen {
            parq_positive: true,
            medically_cleared: true,
            ..HealthScreen::default()
        })),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(!vm.train_blocked);
    assert_eq!(vm.safety_tier, None);
}

#[test]
fn youth_and_injury_screens_defer_visibly() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(profile_with_health(HealthScreen {
            youth: true,
            injury_or_rehab: true,
            ..HealthScreen::default()
        })),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert_eq!(vm.safety_tier, Some(SafetyTier::MedicalReferral));
    assert!(vm.train_blocked);
    // Both gates surface: pediatric says no plan is built (safety-011),
    // injury defers to doctor or physio (safety-048).
    assert!(
        vm.guidance
            .iter()
            .any(|g| g.summary.contains("under 18") && g.summary.contains("coach"))
    );
    assert!(vm.guidance.iter().any(|g| g.summary.contains("physio")));
    // A clean profile clears everything again.
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    assert!(!app.view(&model).train_blocked);
}

#[test]
fn reds_readiness_signal_blocks_deficit_protein_target() {
    // safety-022 end-to-end: deficit requested while a RED-S readiness flag
    // is up → the protein section carries the refusal, not a number.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::RedS, 1.0)),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::ComputeProtein {
            bodyweight_kg: 70.0,
            masters: false,
            deficit: true,
        },
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    let row = &vm.protein_targets[0];
    assert!(row.summary.contains("not prescribed"), "{}", row.summary);
    assert!(row.evidence.safety_critical);
    assert!(!row.summary.contains("g/day"), "no deficit number may leak");

    // Clearing the flag restores the graded target.
    app.update(Event::ClearReadiness, &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.protein_targets[0].summary.contains("g/day"));
}

#[test]
fn reds_screen_flag_also_blocks_deficit() {
    // The onboarding-screen RED-S flag is the second channel into the
    // safety-022 refusal (no readiness signal needed).
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(profile_with_health(HealthScreen {
            reds_signal: true,
            ..HealthScreen::default()
        })),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::ComputeProtein {
            bodyweight_kg: 70.0,
            masters: false,
            deficit: true,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.protein_targets[0].summary.contains("not prescribed"));
    // And the screen flag itself defers training.
    assert!(vm.train_blocked);
}

#[test]
fn reds_profile_reference_carries_no_deficit_framing() {
    // The static reference (`build_reference`) deficit-protein education
    // row is gated on RED-S exactly like the live calculator refusal
    // (File 08 safety-022/safety-049). A RED-S profile must see the cited
    // refusal copy, never a deficit g/kg number.
    let app = Engine;

    // No RED-S: the reference carries the deficit-framed number, no refusal.
    let vm = app.view(&Model::default());
    assert!(
        vm.reference.iter().any(|g| g
            .summary
            .contains("Protein while eating less to lose weight (keeps muscle)")),
        "non-RED-S reference should carry the deficit-framed education row"
    );
    assert!(
        !vm.reference
            .iter()
            .any(|g| g.summary.contains("not prescribed")),
        "non-RED-S reference must not show the refusal copy"
    );

    // RED-S onboarding-screen flag: deficit framing is gone, replaced by the
    // cited, safety-critical refusal.
    let mut model = Model::default();
    app.update(
        Event::SetProfile(profile_with_health(HealthScreen {
            reds_signal: true,
            ..HealthScreen::default()
        })),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let deficit_rows: Vec<_> = vm
        .reference
        .iter()
        .filter(|g| g.summary.contains("eating less"))
        .collect();
    assert!(
        !deficit_rows.is_empty(),
        "the deficit row must still be present as the refusal"
    );
    for g in &deficit_rows {
        assert!(
            !g.summary.contains("g/kg/day"),
            "RED-S reference must carry no deficit framing/number: {}",
            g.summary
        );
        assert!(g.summary.contains("not prescribed"), "{}", g.summary);
        assert!(
            g.evidence.safety_critical,
            "the RED-S refusal row stays safety-critical (SAFE-REDS-001)"
        );
        assert!(
            !g.evidence.citation.is_empty(),
            "the refusal row stays evidence-cited"
        );
    }
}

// ── Youth derived from age alone gates the view + blocks the plan ──
#[test]
fn age_15_gates_even_with_empty_health_screen() {
    let app = Engine;
    let mut model = Model::default();
    // A 15 yo with a SKIPPED health screen (all flags false) + a plan request.
    let youth = Profile {
        age_years: Some(15.0),
        health: HealthScreen::default(),
        ..strength_profile()
    };
    app.update(Event::SetProfile(youth), &mut model)
        .expect_only_render();
    app.update(
        log_set("Back Squat", 120.0, 3, 8.0, MON * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();

    // The pediatric gate fires from age alone: MedicalReferral tier, blocked.
    let vm = app.view(&model);
    assert!(vm.train_blocked, "age 15 must block training");
    assert_eq!(vm.safety_tier, Some(SafetyTier::MedicalReferral));
    // No adult plan renders through the gate.
    assert!(
        vm.next_session.is_none(),
        "no plan may render through a gate"
    );
    assert!(vm.program.is_none());
    // And the model's derived flag is set.
    assert!(model.profile.as_ref().unwrap().health.youth);
    // A gates-only hold produces NO adjustment rows: the headline's
    // safety_hold rung is therefore the ONLY cited surface for this state,
    // and the shells (blocked-branch card + SafetyBanner why-panel) render
    // from it. Pin that contract: kind, evidence, and a real citation.
    assert!(vm.adjustments.is_empty(), "gates emit no adjustment rows");
    assert_eq!(vm.today_headline.kind, HeadlineKind::SafetyHold);
    assert!(vm.today_headline.evidence.safety_critical);
    assert!(
        !vm.today_headline.evidence.citation.is_empty(),
        "a gates-only hold must still carry its citation to the shell"
    );

    // An 18 yo with the same setup is NOT gated (threshold check).
    let mut m2 = Model::default();
    app.update(
        Event::SetProfile(Profile {
            age_years: Some(18.0),
            ..strength_profile()
        }),
        &mut m2,
    )
    .expect_only_render();
    assert!(
        !m2.profile.as_ref().unwrap().health.youth,
        "18 is not youth"
    );
}

// ── Birthday: age derived from the model's today, never stale ──
const BORN_1991_03_12: i64 = 7740;
const D_2026_03_11: i64 = 20523;
const D_2026_03_12: i64 = 20524;
const BORN_2008_10_18: i64 = 14170;
const D_2026_10_17: i64 = 20743;
const D_2026_10_18: i64 = 20744;

fn set_today(app: &Engine, model: &mut Model, epoch_day: i64) {
    app.update(
        Event::SetToday {
            epoch_day,
            utc_offset_sec: 0,
        },
        model,
    )
    .expect_only_render();
}

#[test]
fn birthday_age_is_derived_and_ticks_over_on_the_birthday() {
    let app = Engine;
    let mut model = Model::default();
    set_today(&app, &mut model, D_2026_03_11);
    app.update(
        Event::SetProfile(Profile {
            birth_epoch_day: Some(BORN_1991_03_12),
            // A stale stated age is overridden by the birthday.
            age_years: Some(20.0),
            ..strength_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    assert_eq!(model.profile.as_ref().unwrap().age_years, Some(34.0));
    set_today(&app, &mut model, D_2026_03_12);
    assert_eq!(model.profile.as_ref().unwrap().age_years, Some(35.0));
    assert!(
        model
            .profile_history
            .iter()
            .all(|p| p.age_years == Some(35.0)),
        "the plan's dated profiles carry the same derived age"
    );
    assert_eq!(
        app.view(&model).profile.and_then(|p| p.age_years),
        Some(35.0),
        "the shell reads the derived age off the view"
    );
}

#[test]
fn birthday_profile_set_before_the_clock_derives_on_set_today() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(Profile {
            birth_epoch_day: Some(BORN_1991_03_12),
            ..strength_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    assert_eq!(model.profile.as_ref().unwrap().age_years, None);
    set_today(&app, &mut model, D_2026_03_12);
    assert_eq!(model.profile.as_ref().unwrap().age_years, Some(35.0));
}

#[test]
fn legacy_stated_age_without_birthday_is_kept_across_days() {
    let app = Engine;
    let mut model = Model::default();
    set_today(&app, &mut model, D_2026_03_11);
    app.update(
        Event::SetProfile(Profile {
            age_years: Some(34.0),
            ..strength_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, D_2026_03_11 + 400);
    assert_eq!(model.profile.as_ref().unwrap().age_years, Some(34.0));
}

#[test]
fn youth_gate_lifts_on_the_eighteenth_birthday() {
    let app = Engine;
    let mut model = Model::default();
    set_today(&app, &mut model, D_2026_10_17);
    app.update(
        Event::SetProfile(Profile {
            birth_epoch_day: Some(BORN_2008_10_18),
            effective_from_epoch_day: Some(D_2026_10_17),
            ..strength_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set("Back Squat", 120.0, 3, 8.0, D_2026_10_17 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: D_2026_10_17,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.train_blocked, "17 the day before the birthday: held");
    assert_eq!(vm.safety_tier, Some(SafetyTier::MedicalReferral));
    assert_eq!(model.profile.as_ref().unwrap().age_years, Some(17.0));

    set_today(&app, &mut model, D_2026_10_18);
    let vm = app.view(&model);
    assert_eq!(model.profile.as_ref().unwrap().age_years, Some(18.0));
    assert!(!model.profile.as_ref().unwrap().health.youth);
    assert!(!vm.train_blocked, "18 on the birthday: the hold lifts");
    assert!(vm.program.is_some(), "the plan builds once the gate lifts");
}

#[test]
fn whole_years_handle_leap_day_birthdays_and_future_dates() {
    const BORN_2008_02_29: i64 = 13938;
    const D_2026_02_28: i64 = 20512;
    const D_2026_03_01: i64 = 20513;
    assert_eq!(whole_years_between(BORN_2008_02_29, D_2026_02_28), 17);
    assert_eq!(whole_years_between(BORN_2008_02_29, D_2026_03_01), 18);
    assert_eq!(whole_years_between(D_2026_03_01, D_2026_02_28), 0);
    assert_eq!(whole_years_between(BORN_1991_03_12, i64::MAX), 128);
}
