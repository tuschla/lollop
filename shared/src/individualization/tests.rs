use super::*;

#[test]
fn cadence_maps_to_training_age() {
    assert_eq!(
        training_age_from_cadence(ProgressionCadence::EverySession).value,
        TrainingAge::Novice
    );
    assert_eq!(
        training_age_from_cadence(ProgressionCadence::WeekToWeek).value,
        TrainingAge::Intermediate
    );
    assert_eq!(
        training_age_from_cadence(ProgressionCadence::MonthToMonth).value,
        TrainingAge::Advanced
    );
}

#[test]
fn strength_defaults_match_table_1_1() {
    let n = strength_defaults(TrainingAge::Novice).value;
    assert_eq!(
        (n.intensity_pct_1rm, n.freq_per_muscle, n.sets_per_muscle),
        (60, 3, 4)
    );
    let i = strength_defaults(TrainingAge::Intermediate).value;
    assert_eq!(
        (i.intensity_pct_1rm, i.freq_per_muscle, i.sets_per_muscle),
        (80, 2, 4)
    );
    let a = strength_defaults(TrainingAge::Advanced).value;
    assert_eq!(
        (a.intensity_pct_1rm, a.freq_per_muscle, a.sets_per_muscle),
        (85, 2, 8)
    );
    // Defaults carry Strong evidence (Rhea 2003).
    assert!((strength_defaults(TrainingAge::Novice).confidence() - 0.90).abs() < f32::EPSILON);
}

#[test]
fn volume_sensitivity_gradient_by_training_age() {
    assert_eq!(
        volume_sensitivity(TrainingAge::Novice).value,
        VolumeSensitivity::High
    );
    assert_eq!(
        volume_sensitivity(TrainingAge::Intermediate).value,
        VolumeSensitivity::Moderate
    );
    assert_eq!(
        volume_sensitivity(TrainingAge::Advanced).value,
        VolumeSensitivity::Low
    );
}

#[test]
fn scaling_protects_intensity_last_and_first() {
    let down = scale_down_order().value;
    assert_eq!(down[0], ScaleLever::AccessoryVolume);
    assert_eq!(down[4], ScaleLever::IntensityAndMainCompounds);
    // Scale-up adds intensity late, secondary quality last.
    let up = scale_up_order().value;
    assert_eq!(up[0], ScaleLever::SetsTowardMev);
    assert_eq!(up[4], ScaleLever::SecondaryQuality);
    assert_eq!(min_muscle_exposures_per_week().value, 2);
}

#[test]
fn reentry_ramp_climbs_to_full_and_ends() {
    // 6 wk off: 70 % on the return day, 85 % at week 2 of the 4-wk ramp,
    // full and inactive at its end (REENTRY-RAMP-001).
    let day0 = reentry_ramp(6.0, 0).value;
    assert_eq!(day0.frac, Some(0.70));
    assert!(day0.active);
    assert_eq!(day0.ramp_days, 28);
    let mid = reentry_ramp(6.0, 14).value;
    assert!((mid.frac.unwrap() - 0.85).abs() < 1e-9);
    let end = reentry_ramp(6.0, 28).value;
    assert_eq!(end.frac, Some(1.0));
    assert!(!end.active);
    // >8 wk: no fraction while the window runs, then the window ends.
    let long = reentry_ramp(12.0, 10).value;
    assert_eq!(long.frac, None);
    assert!(long.active && long.treat_as_novice);
    assert!(!reentry_ramp(12.0, 42).value.active);
    assert_eq!(reentry_ramp(6.0, 0).claim_id(), claims::REENTRY_RAMP_001);
}

#[test]
fn reentry_brackets_scale_with_time_off() {
    assert_eq!(resistance_reentry(0.5).value.load_frac, Some(1.00));
    assert_eq!(resistance_reentry(1.5).value.load_frac, Some(0.90));
    assert_eq!(resistance_reentry(3.0).value.load_frac, Some(0.825));
    assert_eq!(resistance_reentry(6.0).value.load_frac, Some(0.70));
    let long = resistance_reentry(12.0).value;
    // >8 wk: KB states no fraction, treat as novice, drop the anchor.
    assert_eq!(long.load_frac, None);
    assert!(long.treat_as_novice);
    // Monotonic non-increasing load fraction across the numeric brackets.
    assert!(resistance_reentry(6.0).value.load_frac < resistance_reentry(3.0).value.load_frac);
}

#[test]
fn reentry_includes_exactly_eight_weeks() {
    // KB re-entry window is "4–8 wk" inclusive: exactly 8 weeks off is still
    // a scaled re-entry (0.70 load), not the >8 wk treat-as-novice arm.
    let eight = resistance_reentry(8.0).value;
    assert_eq!(eight.load_frac, Some(0.70));
    assert!(!eight.treat_as_novice);
    // Just past 8 weeks drops the anchor and treats as novice.
    let past = resistance_reentry(8.5).value;
    assert_eq!(past.load_frac, None);
    assert!(past.treat_as_novice);
}

#[test]
fn progression_and_nutrition_targets() {
    let inc = novice_load_increment().value;
    assert_eq!((inc.upper_kg, inc.lower_kg), (2.5, 5.0));
    assert!(double_progression_add_load(true).value);
    assert!(!double_progression_add_load(false).value);
    assert_eq!(masters_protein_target().value.g_per_kg, (1.2, 1.6));
    assert_eq!(
        deficit_protein_target(false)
            .value
            .expect("no RED-S")
            .g_per_kg,
        (1.8, 2.7)
    );
    // Deficit protein guidance is Strong (Helms/Longland).
    assert!((deficit_protein_target(false).confidence() - 0.90).abs() < f32::EPSILON);
    assert_eq!(masters_protein_per_meal().value, 0.4);
}

#[test]
fn deficit_refused_inside_the_fn_when_reds_signal_present() {
    // File 08 safety-022: deficit request + RED-S signal → the target fn
    // itself refuses (no call path can obtain a deficit past the flag) and
    // the refusal is cited to the RED-S deferral, safety-critical.
    let blocked = deficit_protein_target(true);
    assert!(blocked.value.is_none(), "no deficit target under RED-S");
    assert_eq!(blocked.claim_id(), claims::SAFE_REDS_001);
    assert!(blocked.safety_critical());
}

#[test]
fn novice_stall_response() {
    assert!(novice_stall_action(2, true, false).value.is_none());
    assert!(novice_stall_action(3, false, false).value.is_none());
    let o = novice_stall_action(3, true, false).value.unwrap();
    assert_eq!(
        (
            o.deload_frac,
            o.scope_single_lift,
            o.transition_to_intermediate
        ),
        (0.10, true, false)
    );
    assert!(
        novice_stall_action(3, true, true)
            .value
            .unwrap()
            .transition_to_intermediate
    );
}

#[test]
fn time_substitution_and_environment() {
    assert_eq!(maintenance_frequency_per_week().value, 1);
    let sub = substitution_rule().value;
    assert!(sub.match_movement_pattern && sub.compensate_with_reps_near_failure);
    // Heat: reduce intensity (no KB factor), acclimatize 10–14 days, hard
    // heat-illness stop.
    let heat = environment_modifier(Environment::Heat).value;
    assert!(heat.reduce_intensity);
    assert_eq!(heat.acclimatization_days, Some((10, 14)));
    assert!(heat.stop_on_heat_illness_signs);
    assert!(!heat.extend_warm_up);
    // Altitude: reduce intensity until acclimatized; the KB states NO day
    // count for altitude (unlike heat), and no illness stop.
    let alt = environment_modifier(Environment::Altitude).value;
    assert!(alt.reduce_intensity);
    assert_eq!(
        alt.acclimatization_days, None,
        "no altitude day count in KB"
    );
    assert!(!alt.stop_on_heat_illness_signs);
    assert_eq!(ALTITUDE_THRESHOLD_M, 2_500.0);
    // Cold: extend warm-up only; the KB states nothing else for cold.
    let cold = environment_modifier(Environment::Cold).value;
    assert!(cold.extend_warm_up);
    assert!(!cold.reduce_intensity);
    assert_eq!(cold.acclimatization_days, None);
    // Neutral leaves the prescription unchanged.
    let neutral = environment_modifier(Environment::Neutral).value;
    assert!(!neutral.reduce_intensity && !neutral.extend_warm_up);
    // ENV-001 is safety-critical via the heat STOP branch.
    assert!(environment_modifier(Environment::Heat).safety_critical());
}

// --- Stage-0 onboarding gates (File 08 onboard-050) ---

#[test]
fn clear_health_screen_yields_no_gates() {
    assert!(onboarding_gates(&HealthScreen::default()).is_empty());
    assert!(!HealthScreen::default().any_gate());
}

#[test]
fn each_screen_flag_defers_with_its_own_safety_claim() {
    type ScreenCase = (fn(&mut HealthScreen), crate::evidence::ClaimId);
    let cases: &[ScreenCase] = &[
        (|s| s.youth = true, claims::SAFE_PEDS_001),
        (|s| s.parq_positive = true, claims::SAFE_CVD_001),
        (|s| s.pregnant = true, claims::SAFE_PREG_001),
        (
            |s| s.pregnancy_warning_sign = true,
            claims::SAFE_PREG_WARN_001,
        ),
        (|s| s.injury_or_rehab = true, claims::SAFE_INJURY_001),
        (|s| s.reds_signal = true, claims::SAFE_REDS_001),
        (|s| s.symptomatic = true, claims::SCREEN_ACSM_001),
        (|s| s.acute_illness = true, claims::ILLNESS_NECK_001),
    ];
    for (set, claim_id) in cases {
        let mut s = HealthScreen::default();
        set(&mut s);
        let gates = onboarding_gates(&s);
        assert_eq!(gates.len(), 1, "{claim_id:?}: exactly one gate fires");
        assert!(
            matches!(gates[0].value, Adjustment::Defer { .. }),
            "{claim_id:?}: gate must defer to a professional"
        );
        assert_eq!(gates[0].claim_id(), *claim_id);
        assert!(
            gates[0].safety_critical(),
            "{claim_id:?}: every onboarding gate is safety-critical"
        );
        assert!(s.any_gate());
    }
}

#[test]
fn medical_clearance_clears_only_the_parq_gate() {
    // safety-044: clearance re-opens programming after a positive screen…
    let cleared = HealthScreen {
        parq_positive: true,
        medically_cleared: true,
        ..HealthScreen::default()
    };
    assert!(onboarding_gates(&cleared).is_empty());
    // …but pregnancy keeps deferring autonomous prescription regardless
    // (safety-045: provider clearance AND individualization, the engine
    // still must not autonomously prescribe/progress load).
    let pregnant_cleared = HealthScreen {
        pregnant: true,
        medically_cleared: true,
        ..HealthScreen::default()
    };
    assert_eq!(onboarding_gates(&pregnant_cleared).len(), 1);
}

#[test]
fn symptomatic_screen_is_not_cleared_by_medical_clearance() {
    // SCREEN-ACSM-001: a symptomatic answer defers regardless of a prior
    // clearance answer; only re-answering the questions lifts it.
    let s = HealthScreen {
        symptomatic: true,
        medically_cleared: true,
        ..HealthScreen::default()
    };
    let gates = onboarding_gates(&s);
    assert_eq!(gates.len(), 1);
    assert_eq!(gates[0].claim_id(), claims::SCREEN_ACSM_001);
    assert!(s.any_gate());
    // A symptom answer gates by itself, even when the shell forgot the flag.
    let answers_only = HealthScreen {
        parq_yes: 0b10,
        ..HealthScreen::default()
    };
    let gates = onboarding_gates(&answers_only);
    assert_eq!(gates.len(), 1);
    assert_eq!(gates[0].claim_id(), claims::SCREEN_ACSM_001);
    assert!(answers_only.any_gate());
}

#[test]
fn clearance_covers_only_the_answers_it_was_given_for() {
    // safety-044: a follow-up Yes added after clearance gates again.
    let cleared_q1 = HealthScreen {
        parq_positive: true,
        parq_yes: 0b1,
        medically_cleared: true,
        cleared_parq_mask: 0b1,
        ..HealthScreen::default()
    };
    assert!(onboarding_gates(&cleared_q1).is_empty());
    let q5_added = HealthScreen {
        parq_yes: 0b1_0001,
        ..cleared_q1.clone()
    };
    let gates = onboarding_gates(&q5_added);
    assert_eq!(gates.len(), 1);
    assert_eq!(gates[0].claim_id(), claims::SAFE_CVD_001);
    // A follow-up answer gates even when the shell forgot `parq_positive`.
    let answer_without_flag = HealthScreen {
        parq_yes: 0b1000,
        ..HealthScreen::default()
    };
    assert!(answer_without_flag.any_gate());
    // A clearance saved before the mask existed covers the current answers.
    let legacy_cleared = HealthScreen {
        parq_positive: true,
        parq_yes: 0b1_0001,
        medically_cleared: true,
        ..HealthScreen::default()
    };
    assert!(!legacy_cleared.any_gate());
    // The bone/joint answer is the injury gate.
    let q6 = HealthScreen {
        parq_yes: crate::schema::PARQ_INJURY_BIT,
        ..HealthScreen::default()
    };
    assert_eq!(onboarding_gates(&q6)[0].claim_id(), claims::SAFE_INJURY_001);
}

#[test]
fn pediatric_gate_puts_technique_first() {
    // safety-011: no autonomous maximal loading or 1RM testing; the hold
    // banner already says no plan is built, so the card doesn't repeat it.
    let s = HealthScreen {
        youth: true,
        ..HealthScreen::default()
    };
    let gates = onboarding_gates(&s);
    match &gates[0].value {
        Adjustment::Defer { reason } => {
            assert!(reason.contains("technique ahead of weight"), "{reason}");
            assert!(reason.contains("1-rep-max"));
            assert!(!reason.contains("build plans"), "{reason}");
        }
        other => panic!("expected Defer, got {other:?}"),
    }
    // Strong evidence (Lloyd/Faigenbaum consensus), never overstated.
    assert!((gates[0].confidence() - 0.90).abs() < f32::EPSILON);
}

#[test]
fn illness_card_points_to_i_am_better_not_the_health_answers() {
    // The banner says "You marked yourself ill. Rest until
    // you're better." and carries an I'm better action.
    let s = HealthScreen {
        acute_illness: true,
        ..HealthScreen::default()
    };
    match &onboarding_gates(&s)[0].value {
        Adjustment::Defer { reason } => {
            assert!(reason.contains("tap I'm better"), "{reason}");
            assert!(reason.contains("no fever"), "{reason}");
            assert!(!reason.contains("marked yourself ill"), "{reason}");
            assert!(!reason.contains("health answers"), "{reason}");
        }
        other => panic!("expected Defer, got {other:?}"),
    }
}

#[test]
fn pregnancy_gate_emits_safe_preg_001_with_reference_target() {
    // safety-045: SAFE-PREG-001 actually emitted; the ~150 min a week figure is
    // a common guide only, phrased as such.
    let s = HealthScreen {
        pregnant: true,
        ..HealthScreen::default()
    };
    let gates = onboarding_gates(&s);
    assert_eq!(gates[0].claim_id(), claims::SAFE_PREG_001);
    match &gates[0].value {
        Adjustment::Defer { reason } => {
            assert!(reason.contains("150 minutes"));
            assert!(reason.contains("common guide"));
        }
        other => panic!("expected Defer, got {other:?}"),
    }
}

#[test]
fn injury_gate_is_expert_opinion_yet_safety_critical() {
    // safety-048 is the lowest-graded File 08 safety rule (ExpertOpinion,
    // 0.30) but still safety-critical: the grade is never inflated to make
    // the stop look better-evidenced.
    let s = HealthScreen {
        injury_or_rehab: true,
        ..HealthScreen::default()
    };
    let g = &onboarding_gates(&s)[0];
    assert!((g.confidence() - 0.30).abs() < f32::EPSILON);
    assert!(g.safety_critical());
}

#[test]
fn multiple_flags_all_surface_with_warning_sign_first() {
    let s = HealthScreen {
        pregnant: true,
        pregnancy_warning_sign: true,
        injury_or_rehab: true,
        ..HealthScreen::default()
    };
    let gates = onboarding_gates(&s);
    assert_eq!(gates.len(), 3, "every fired gate must surface");
    assert_eq!(
        gates[0].claim_id(),
        claims::SAFE_PREG_WARN_001,
        "acute warning sign leads"
    );
}

#[test]
fn pregnancy_precautions_verbatim_avoid_list() {
    let p = pregnancy_precautions();
    assert!(p.value.avoid_prolonged_supine);
    assert!(p.value.avoid_overheating);
    assert!(p.value.avoid_contact_fall_risk);
    assert!(p.value.avoid_scuba);
    assert_eq!(p.value.avoid_altitude_above_m, 2_500.0);
    assert!(p.value.avoid_valsalva);
    assert_eq!(p.claim_id(), claims::SAFE_PREG_AVOID_001);
    assert!(p.safety_critical());
}
