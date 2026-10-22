use super::*;

#[test]
fn running_intensity_discipline_thresholds() {
    assert!(easy_run_intensity_discipline(0.25).is_none());
    assert_eq!(
        easy_run_intensity_discipline(0.26).unwrap().value,
        FeedbackCategory::IntensityDiscipline
    );
    assert_eq!(
        positive_split_discipline(3.1).value,
        FeedbackCategory::IntensityDiscipline
    );
}

#[test]
fn even_or_negative_split_earns_positive_execution() {
    // feedback-017: an even/negative split is praised, not silently
    // dropped, "textbook pacing discipline", cited per-rule.
    for pct in [3.0, 0.0, -2.0] {
        let f = positive_split_discipline(pct);
        assert_eq!(f.value, FeedbackCategory::PositiveExecution, "{pct}%");
        assert_eq!(f.claim_id(), claims::FB_PACING_001);
    }
}

#[test]
fn trend_summary_precedence_and_bands() {
    // Load-explained decline outranks a nominally-up direction.
    assert_eq!(
        trend_summary(TrendDirection::Up, 0, true, true, false).value,
        TrendSummary::LoadExplainedDecline
    );
    assert_eq!(
        trend_summary(TrendDirection::Down, 0, true, false, true).value,
        TrendSummary::LoadExplainedDecline
    );
    // Improving.
    assert_eq!(
        trend_summary(TrendDirection::Up, 0, false, false, false).value,
        TrendSummary::Improving
    );
    // Plateau needs >=4 flat weeks.
    assert_eq!(
        trend_summary(TrendDirection::Flat, 4, false, false, false).value,
        TrendSummary::Plateau
    );
    assert_eq!(
        trend_summary(TrendDirection::Flat, 3, false, false, false).value,
        TrendSummary::Stable
    );
    // Performance down but no load explanation -> not a recovery-first message.
    assert_eq!(
        trend_summary(TrendDirection::Down, 0, true, false, false).value,
        TrendSummary::Stable
    );
}

#[test]
fn safety_gate_priority_order() {
    // Pain outranks everything.
    let all = SafetySignals {
        bone_pain_red_flag: true,
        compulsive_flag: true,
        overtraining_signal_count: 5,
        single_session_spike_frac: Some(0.9),
        ..Default::default()
    };
    assert_eq!(
        safety_gate(all).unwrap().value,
        FeedbackCategory::ConcernInjury
    );

    // Behavior outranks recovery + progression.
    let behav = SafetySignals {
        bone_pain_red_flag: false,
        compulsive_flag: true,
        overtraining_signal_count: 5,
        single_session_spike_frac: Some(0.9),
        ..Default::default()
    };
    assert_eq!(
        safety_gate(behav).unwrap().value,
        FeedbackCategory::ConcernBehavior
    );

    // Recovery outranks progression.
    let rec = SafetySignals {
        overtraining_signal_count: 2,
        single_session_spike_frac: Some(0.9),
        ..Default::default()
    };
    assert_eq!(
        safety_gate(rec).unwrap().value,
        FeedbackCategory::ConcernRecovery
    );

    // Spike alone.
    let spike = SafetySignals {
        single_session_spike_frac: Some(0.11),
        ..Default::default()
    };
    assert_eq!(
        safety_gate(spike).unwrap().value,
        FeedbackCategory::DangerousProgression
    );

    // Clear session.
    assert!(safety_gate(SafetySignals::default()).is_none());
    // A single NFOR signal does not fire (needs >=2).
    assert!(
        safety_gate(SafetySignals {
            overtraining_signal_count: 1,
            ..Default::default()
        })
        .is_none()
    );
    // Spike at/below 10% does not fire.
    assert!(
        safety_gate(SafetySignals {
            single_session_spike_frac: Some(0.10),
            ..Default::default()
        })
        .is_none()
    );
}

#[test]
fn concern_injury_is_safety_critical() {
    let c = evidence::graded(FeedbackCategory::ConcernInjury, claims::SAFE_BSI_001);
    assert!(c.safety_critical());
}

#[test]
fn suppression_set_matches_file05() {
    for c in [
        FeedbackCategory::ConcernInjury,
        FeedbackCategory::ConcernRecovery,
        FeedbackCategory::ConcernBehavior,
        FeedbackCategory::ConcernEnergy,
        FeedbackCategory::DangerousProgression,
    ] {
        assert!(
            c.suppresses_competing_praise(),
            "{c:?} must suppress praise"
        );
    }
    for c in [
        FeedbackCategory::PositiveMastery,
        FeedbackCategory::PositiveExecution,
        FeedbackCategory::ProgressionNudge,
        FeedbackCategory::CorrectiveProcess,
        FeedbackCategory::InformationalNeutral,
        FeedbackCategory::IntensityDiscipline,
        FeedbackCategory::ContextualBadDay,
    ] {
        assert!(
            !c.suppresses_competing_praise(),
            "{c:?} must not suppress praise"
        );
    }
}

#[test]
fn reds_screen_routes_to_energy_referral_above_behavior() {
    let reds = SafetySignals {
        reds_signal_reported: true,
        overtraining_signal_count: 3,
        ..Default::default()
    };
    let c = safety_gate(reds).expect("energy concern");
    assert_eq!(c.value, FeedbackCategory::ConcernEnergy);
    assert!(c.safety_critical());
    assert_eq!(c.claim_id(), claims::REDS_SCREEN_001);
    // With the compulsive-exercise screen also positive, the referral
    // still shows (one card only).
    let both = SafetySignals {
        compulsive_flag: true,
        ..reds
    };
    assert_eq!(
        safety_gate(both).unwrap().value,
        FeedbackCategory::ConcernEnergy
    );
}

#[test]
fn overtraining_sustained_needs_two_signs_for_a_week() {
    assert!(!overtraining_sustained(1, Some(3.0)));
    assert!(!overtraining_sustained(2, Some(0.5)));
    assert!(overtraining_sustained(2, Some(1.0)));
    assert!(overtraining_sustained(2, None));
}

#[test]
fn resolve_suppresses_praise_under_concern() {
    let praise = Some(evidence::graded(
        FeedbackCategory::PositiveMastery,
        claims::AUTOREG_RIR_001,
    ));
    let injury = SafetySignals {
        bone_pain_red_flag: true,
        ..Default::default()
    };
    assert_eq!(
        resolve_feedback(injury, praise).value,
        FeedbackCategory::ConcernInjury
    );

    // Safety-clear: execution passes through.
    let praise = Some(evidence::graded(
        FeedbackCategory::PositiveMastery,
        claims::AUTOREG_RIR_001,
    ));
    assert_eq!(
        resolve_feedback(SafetySignals::default(), praise).value,
        FeedbackCategory::PositiveMastery
    );

    // Safety-clear, no execution: informational default.
    assert_eq!(
        resolve_feedback(SafetySignals::default(), None).value,
        FeedbackCategory::InformationalNeutral
    );
}

#[test]
fn lifting_branches() {
    // Missed reps.
    assert_eq!(
        lifting_feedback(false, 2, 2).0.value,
        FeedbackCategory::CorrectiveProcess
    );
    // Reps met, RIR 0 vs 2-3 target, caution.
    assert_eq!(
        lifting_feedback(true, 0, 3).0.value,
        FeedbackCategory::CorrectiveProcess
    );
    // Reps met, RIR 4 vs 1-2 target, nudge.
    assert_eq!(
        lifting_feedback(true, 4, 1).0.value,
        FeedbackCategory::ProgressionNudge
    );
    // Reps met at target cost, mastery.
    assert_eq!(
        lifting_feedback(true, 2, 2).0.value,
        FeedbackCategory::PositiveMastery
    );
    // Above-target reserve is also mastery (feedback-019: RIR ≥ target).
    assert_eq!(
        lifting_feedback(true, 3, 3).0.value,
        FeedbackCategory::PositiveMastery
    );
}

#[test]
fn below_target_rir_is_never_mastery() {
    // feedback-019 requires rir_actual >= rir_target for PositiveMastery;
    // a set that cost more than planned (RIR 1 vs target 3) routes to a
    // neutral process tone, not praise-with-progression.
    let f = lifting_feedback(true, 1, 3).0;
    assert_ne!(f.value, FeedbackCategory::PositiveMastery);
    assert_eq!(f.value, FeedbackCategory::InformationalNeutral);
    // Per-rule citation, graded Moderate per File 05, not the Strong
    // File 09 autoregulation-backbone claim.
    assert_eq!(f.claim_id(), claims::FB_RIR_001);

    // RIR 2 vs target 3, still short of plan, still not mastery.
    assert_ne!(
        lifting_feedback(true, 2, 3).0.value,
        FeedbackCategory::PositiveMastery
    );
    // At failure against a 2-3 target → corrective (feedback-020).
    assert_eq!(
        lifting_feedback(true, 0, 2).0.value,
        FeedbackCategory::CorrectiveProcess
    );
}

#[test]
fn decoupling_gated_and_banded() {
    // Confounded context: no message.
    assert!(decoupling_feedback(3.0, false).is_none());
    // <5% durability.
    assert_eq!(
        decoupling_feedback(3.0, true).unwrap().0.value,
        FeedbackCategory::PositiveExecution
    );
    // 5-10% neutral.
    assert_eq!(
        decoupling_feedback(7.0, true).unwrap().0.value,
        FeedbackCategory::InformationalNeutral
    );
    // >10% corrective.
    assert_eq!(
        decoupling_feedback(12.0, true).unwrap().0.value,
        FeedbackCategory::CorrectiveProcess
    );
    // Boundary must agree with `load::decoupling_band`: exactly 10% and a
    // float 9.9999…% both land in the flag band, so the feedback card and the
    // base-fitness band row never contradict each other at ~10%.
    assert_eq!(
        decoupling_feedback(10.0, true).unwrap().0.value,
        FeedbackCategory::CorrectiveProcess
    );
    let just_under = 10.0 - 1e-12;
    assert_eq!(
        decoupling_feedback(just_under, true).unwrap().0.value,
        FeedbackCategory::CorrectiveProcess,
        "9.9999…% must flag, matching load::decoupling_band"
    );
    assert!(matches!(
        crate::load::decoupling_band(just_under).value,
        crate::load::DecouplingBand::Insufficient
    ));
}

#[test]
fn goal_framing_defaults_to_process() {
    // Process framing is the default (never outcome-first), graded as the
    // KB rule is: Moderate and contested (feedback-003).
    let g = default_goal_framing();
    assert_eq!(g.value, GoalFraming::Process);
    assert_eq!(g.grade(), crate::schema::EvidenceGrade::Moderate);
    assert!(g.contested().is_some());
}

// --- feedback-036: the >=1-2 wk duration condition ---

#[test]
fn recovery_concern_respects_duration_condition() {
    // Two signals over half a week is a "single noisy night" territory -
    // the gate must NOT fire (feedback-036 duration condition).
    let short = SafetySignals {
        overtraining_signal_count: 2,
        overtraining_weeks: Some(0.5),
        ..Default::default()
    };
    assert!(safety_gate(short).is_none());
    // >=1 week satisfies the stated >=1-2 wk band.
    let sustained = SafetySignals {
        overtraining_signal_count: 2,
        overtraining_weeks: Some(1.0),
        ..Default::default()
    };
    assert_eq!(
        safety_gate(sustained).unwrap().value,
        FeedbackCategory::ConcernRecovery
    );
    // Untracked duration errs protective: count alone still fires.
    let untracked = SafetySignals {
        overtraining_signal_count: 2,
        overtraining_weeks: None,
        ..Default::default()
    };
    assert!(safety_gate(untracked).is_some());
}

// --- feedback-015: interval/threshold mastery ---

#[test]
fn interval_mastery_needs_paces_and_rpe() {
    let m = interval_mastery_feedback(true, true).expect("mastery fires");
    assert_eq!(m.value, FeedbackCategory::PositiveMastery);
    assert_eq!(m.claim_id(), claims::FB_INTERVAL_MASTERY_001);
    assert!((m.confidence() - 0.65).abs() < f32::EPSILON);
    assert!(interval_mastery_feedback(true, false).is_none());
    assert!(interval_mastery_feedback(false, true).is_none());
}

// --- feedback-035: female BSI clinician prompt ---

#[test]
fn female_bsi_prompt_gated_on_both_conditions() {
    let p = bsi_menstrual_nutrition_prompt(true, true);
    assert!(p.value);
    assert!(p.safety_critical());
    assert_eq!(p.claim_id(), claims::FB_BSI_FEMALE_001);
    // ExpertOpinion: the linkage prompt has no direct trial evidence.
    assert!((p.confidence() - 0.30).abs() < f32::EPSILON);
    assert!(!bsi_menstrual_nutrition_prompt(true, false).value);
    assert!(!bsi_menstrual_nutrition_prompt(false, true).value);
    // The copy defers to the clinician and diagnoses nothing.
    assert!(BSI_FEMALE_PROMPT.contains("tell them about your periods"));
}

// --- feedback-023/024: verbosity personalization ---

#[test]
fn verbosity_defaults_by_experience() {
    let b = verbosity_for_experience(false).value;
    assert_eq!((b.max_takeaways, b.max_metrics), (1, 1));
    assert!(b.rationale_mandatory && b.minimize_jargon && b.lead_with_single_lever);
    let a = verbosity_for_experience(true).value;
    assert_eq!((a.max_takeaways, a.max_metrics), (1, 3));
    assert!(!a.minimize_jargon && a.lead_with_single_lever);
    assert_eq!(
        verbosity_for_experience(true).claim_id(),
        claims::FB_VERBOSITY_001
    );
}

// --- feedback-005: mastery anchoring in praise copy ---

#[test]
fn mastery_anchor_only_for_praise_categories() {
    assert!(mastery_anchor_required(FeedbackCategory::PositiveMastery).value);
    // Durability/pacing affirmed is an observation, not a mastery
    // experience: the KB trigger names mastery and consistency only.
    for c in [
        FeedbackCategory::PositiveExecution,
        FeedbackCategory::ConcernInjury,
        FeedbackCategory::ConcernRecovery,
        FeedbackCategory::CorrectiveProcess,
        FeedbackCategory::InformationalNeutral,
        FeedbackCategory::ContextualBadDay,
    ] {
        assert!(!mastery_anchor_required(c).value, "{c:?} never anchored");
    }
    // Strong (Bandura self-efficacy), the best-evidenced feedback rule.
    assert!(
        (mastery_anchor_required(FeedbackCategory::PositiveMastery).confidence() - 0.90).abs()
            < f32::EPSILON
    );
}
