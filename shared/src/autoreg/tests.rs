use super::derive::*;
use super::pain::*;
use super::prescriptions::*;
use super::rules::*;
use super::*;
use crate::evidence::claims;
use crate::schema::{
    Adjustment, CheckinInput, Goal, PainDetail, PainKind, PainTrend, ReadinessInput,
    ReadinessSignal, Recommended, SafetyTier,
};

/// The rules with no context beyond the inputs. `pub(crate)` for the app tests.
pub(crate) fn adjustments(inputs: &[ReadinessInput]) -> Vec<Recommended<Adjustment>> {
    adjustments_for_goal(inputs, None)
}

fn adjustments_for_goal(
    inputs: &[ReadinessInput],
    goal: Option<&Goal>,
) -> Vec<Recommended<Adjustment>> {
    adjustments_with_context(inputs, goal, false)
}

/// The rules with goal and block context only; `high_load_block` arms the
/// autoreg-029 parasympathetic-saturation guard.
fn adjustments_with_context(
    inputs: &[ReadinessInput],
    goal: Option<&Goal>,
    high_load_block: bool,
) -> Vec<Recommended<Adjustment>> {
    adjustments_in(
        inputs,
        &RuleContext {
            goal,
            high_load_block,
            ..RuleContext::default()
        },
    )
}

/// The safety tier with goal context only. `pub(crate)` for the app tests.
pub(crate) fn resolve_safety_for_goal(
    inputs: &[ReadinessInput],
    goal: Option<&Goal>,
) -> Option<SafetyTier> {
    resolve_safety_in(
        inputs,
        &RuleContext {
            goal,
            ..RuleContext::default()
        },
    )
}

/// [`signal_states_in`] with goal and block context only.
fn signal_states(
    inputs: &[ReadinessInput],
    goal: Option<&Goal>,
    high_load_block: bool,
    stale: &[ReadinessInput],
) -> Vec<SignalState> {
    signal_states_in(
        inputs,
        &RuleContext {
            goal,
            high_load_block,
            ..RuleContext::default()
        },
        stale,
    )
}

/// Whether the newest morning RHR sits at/above the fixed +10 bpm stop band
/// (the reading level only; whether it rests is [`rhr_verdict`]'s call).
fn rhr_stop(inputs: &[ReadinessInput]) -> bool {
    latest(inputs, ReadinessSignal::RestingHr).is_some_and(|v| v >= RHR_STOP_BPM)
}

/// [`wellness_downgrade_in`] without a clock.
fn wellness_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    wellness_downgrade_in(inputs, &RuleContext::default())
}

/// [`rhr_downgrade_in`] with the fixed KB bands (no individual SD).
fn rhr_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    rhr_downgrade_in(inputs, &RuleContext::default())
}

/// A bare readiness observation. `pub(crate)` because `app::tests` builds the
/// same fixture and one definition keeps them from drifting apart.
pub(crate) fn input(signal: ReadinessSignal, value: f64) -> ReadinessInput {
    ReadinessInput {
        signal,
        value,
        observed_at: 0,
        streak: 0,
        pain: None,
        effort_min: None,
    }
}

fn input_streak(signal: ReadinessSignal, value: f64, streak: u8) -> ReadinessInput {
    ReadinessInput {
        streak,
        ..input(signal, value)
    }
}

fn pain_input(kind: PainKind, severity: u8, trend: PainTrend, persists: bool) -> ReadinessInput {
    pain_input_loc(kind, severity, trend, persists, None)
}

fn pain_input_loc(
    kind: PainKind,
    severity: u8,
    trend: PainTrend,
    persists: bool,
    location: Option<&str>,
) -> ReadinessInput {
    ReadinessInput {
        pain: Some(PainDetail {
            kind,
            severity: Some(severity),
            trend,
            persists,
            location: location.map(str::to_string),
        }),
        ..input(ReadinessSignal::Pain, 1.0)
    }
}

fn state_str(states: &[SignalState], signal: ReadinessSignal) -> String {
    states
        .iter()
        .find(|s| s.signal == signal)
        .unwrap_or_else(|| panic!("signal {signal:?} present"))
        .state
        .clone()
}

/// Locks the displayed "why" (`signal_states`) to the SAME named thresholds
/// the adjustment rules fire on. Both sides read one set of constants,
/// so they cannot desync; this test additionally pins the state text to the
/// rule outcome at each band boundary, so re-inlining a literal on either
/// side (reintroducing the drift hazard) breaks a build.
#[test]
fn signal_states_track_rule_thresholds() {
    // --- RPE delta bands vs rpe_load_adjust ---
    for (v, want_state) in [
        (RPE_DELTA_WELL_ABOVE, "well above target"),
        (RPE_DELTA_ABOVE, "above target"),
        (0.0, "on target"),
        (RPE_DELTA_BELOW, "below target"),
        (RPE_DELTA_WELL_BELOW, "well below target"),
    ] {
        let ins = vec![input(ReadinessSignal::Rpe, v)];
        assert_eq!(
            state_str(&signal_states(&ins, None, false, &[]), ReadinessSignal::Rpe),
            want_state
        );
    }
    // On-target is the only band that yields no load change.
    assert!(rpe_load_adjust(&[input(ReadinessSignal::Rpe, 0.0)]).is_none());
    assert!(rpe_load_adjust(&[input(ReadinessSignal::Rpe, RPE_DELTA_ABOVE)]).is_some());
    assert!(rpe_load_adjust(&[input(ReadinessSignal::Rpe, RPE_DELTA_BELOW)]).is_some());

    // --- e1RM ratio gates vs e1rm_gate ---
    let deload = vec![input_streak(
        ReadinessSignal::EstimatedOneRm,
        E1RM_DELOAD_RATIO - 0.01,
        E1RM_DELOAD_STREAK,
    )];
    assert_eq!(
        state_str(
            &signal_states(&deload, None, false, &[]),
            ReadinessSignal::EstimatedOneRm
        ),
        "down more than 10% for 2+ sessions"
    );
    assert!(
        e1rm_gate(&deload)
            .iter()
            .any(|r| matches!(r.value, Adjustment::Deload { .. }))
    );

    let reduce = vec![input(
        ReadinessSignal::EstimatedOneRm,
        E1RM_REDUCE_RATIO - 0.01,
    )];
    assert_eq!(
        state_str(
            &signal_states(&reduce, None, false, &[]),
            ReadinessSignal::EstimatedOneRm
        ),
        "down"
    );
    assert!(
        e1rm_gate(&reduce)
            .iter()
            .any(|r| matches!(r.value, Adjustment::ReduceLoadPct(_)))
    );

    let up = vec![input(
        ReadinessSignal::EstimatedOneRm,
        E1RM_INCREASE_RATIO + 0.01,
    )];
    assert_eq!(
        state_str(
            &signal_states(&up, None, false, &[]),
            ReadinessSignal::EstimatedOneRm
        ),
        "up"
    );
    assert!(
        e1rm_gate(&up)
            .iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_)))
    );

    let stable = vec![input(ReadinessSignal::EstimatedOneRm, 1.0)];
    assert_eq!(
        state_str(
            &signal_states(&stable, None, false, &[]),
            ReadinessSignal::EstimatedOneRm
        ),
        "stable"
    );
    assert!(e1rm_gate(&stable).is_empty());

    // --- Wellness z bands vs wellness_downgrade (≥2 consecutive suppressed days) ---
    let two_day = vec![input_streak(
        ReadinessSignal::WellnessZ,
        WELLNESS_MULTIDAY_Z,
        WELLNESS_DOWNGRADE_STREAK,
    )];
    assert_eq!(
        state_str(
            &signal_states(&two_day, None, false, &[]),
            ReadinessSignal::WellnessZ
        ),
        "below normal"
    );
    assert!(wellness_downgrade(&two_day).is_some());

    let multiday = vec![input_streak(
        ReadinessSignal::WellnessZ,
        WELLNESS_MULTIDAY_Z,
        WELLNESS_MULTIDAY_STREAK,
    )];
    assert_eq!(
        state_str(
            &signal_states(&multiday, None, false, &[]),
            ReadinessSignal::WellnessZ
        ),
        "below normal"
    );
    assert!(wellness_downgrade(&multiday).is_some());

    // A shallow single day (z between −1 and −1.5) never vetoes training.
    let low_single = vec![input(ReadinessSignal::WellnessZ, WELLNESS_MULTIDAY_Z)];
    assert_eq!(
        state_str(
            &signal_states(&low_single, None, false, &[]),
            ReadinessSignal::WellnessZ
        ),
        "low today"
    );
    assert!(wellness_downgrade(&low_single).is_none());

    // A deep single day (z ≤ −1.5) downgrades on its own, and the strip says so.
    let deep_single = vec![input(ReadinessSignal::WellnessZ, -2.0)];
    assert_eq!(
        state_str(
            &signal_states(&deep_single, None, false, &[]),
            ReadinessSignal::WellnessZ
        ),
        "below normal"
    );
    assert!(wellness_downgrade(&deep_single).is_some());

    // --- HRV SWC band vs hrv_downgrade / hrv_saturation_hold ---
    let hrv_low = vec![input(ReadinessSignal::HrvLnRmssd, HRV_SWC_LOWER_Z - 0.1)];
    assert_eq!(
        state_str(
            &signal_states(&hrv_low, None, false, &[]),
            ReadinessSignal::HrvLnRmssd
        ),
        "below normal"
    );
    assert!(hrv_downgrade(&hrv_low).is_some());

    let hrv_high = HRV_SWC_UPPER_Z + 0.1;
    assert_eq!(
        state_str(
            &signal_states(
                &[input(ReadinessSignal::HrvLnRmssd, hrv_high)],
                None,
                true,
                &[]
            ),
            ReadinessSignal::HrvLnRmssd
        ),
        "above normal: no extra weight today"
    );
    assert!(hrv_saturation_hold(hrv_high, true).value);
    assert!(!hrv_saturation_hold(hrv_high, false).value);

    // --- RHR bands vs rhr_verdict (ready-upd-002) ---
    // One stop-level morning is likely noise: an easier day + illness check.
    let rhr_stop_in = vec![input(ReadinessSignal::RestingHr, RHR_STOP_BPM)];
    assert_eq!(
        state_str(
            &signal_states(&rhr_stop_in, None, false, &[]),
            ReadinessSignal::RestingHr
        ),
        "high this morning: easier day, check for illness"
    );
    assert!(rhr_stop(&rhr_stop_in));
    // Stop level on 2 mornings running → rest.
    let rhr_rest = vec![input_streak(
        ReadinessSignal::RestingHr,
        RHR_STOP_BPM,
        RHR_DOWNGRADE_STREAK,
    )];
    assert_eq!(
        state_str(
            &signal_states(&rhr_rest, None, false, &[]),
            ReadinessSignal::RestingHr
        ),
        "well above normal: rest"
    );

    let rhr_down = vec![input_streak(
        ReadinessSignal::RestingHr,
        RHR_DOWNGRADE_FLOOR_BPM,
        RHR_DOWNGRADE_STREAK,
    )];
    assert_eq!(
        state_str(
            &signal_states(&rhr_down, None, false, &[]),
            ReadinessSignal::RestingHr
        ),
        "high 2+ days"
    );
    assert!(rhr_downgrade(&rhr_down).is_some());

    let rhr_single = vec![input(ReadinessSignal::RestingHr, RHR_DOWNGRADE_FLOOR_BPM)];
    assert_eq!(
        state_str(
            &signal_states(&rhr_single, None, false, &[]),
            ReadinessSignal::RestingHr
        ),
        "high today, probably a one-off"
    );
    assert!(rhr_downgrade(&rhr_single).is_none());
    assert!(!rhr_stop(&rhr_single));

    // --- Soreness floor vs soreness_downgrade ---
    let sore = vec![input(ReadinessSignal::Soreness, SORENESS_HIGH)];
    assert_eq!(
        state_str(
            &signal_states(&sore, None, false, &[]),
            ReadinessSignal::Soreness
        ),
        "high"
    );
    assert!(soreness_downgrade(&sore).is_some());

    // --- Decoupling boundary vs decoupling_downgrade: a drift of EXACTLY
    // 10% is the flag band, and the strip and the rule must agree on it.
    let decouple = vec![ReadinessInput {
        effort_min: Some(45.0),
        ..input(
            ReadinessSignal::AerobicDecoupling,
            crate::load::DECOUPLING_HIGH_PCT,
        )
    }];
    assert_eq!(
        state_str(
            &signal_states(&decouple, None, false, &[]),
            ReadinessSignal::AerobicDecoupling
        ),
        "high"
    );
    assert!(matches!(
        decoupling_downgrade(&decouple).map(|r| r.value),
        Some(Adjustment::DowngradeSession)
    ));
    // Just under the boundary: neither side flags.
    let ok = vec![ReadinessInput {
        effort_min: Some(45.0),
        ..input(
            ReadinessSignal::AerobicDecoupling,
            crate::load::DECOUPLING_HIGH_PCT - 0.1,
        )
    }];
    assert_eq!(
        state_str(
            &signal_states(&ok, None, false, &[]),
            ReadinessSignal::AerobicDecoupling
        ),
        "normal"
    );
    assert!(decoupling_downgrade(&ok).is_none());

    // --- Velocity-loss band vs vl_stop (already shared via vl_threshold_pct) ---
    let vl = vec![input(ReadinessSignal::VelocityLoss, vl_threshold_pct(None))];
    assert_eq!(
        state_str(
            &signal_states(&vl, None, false, &[]),
            ReadinessSignal::VelocityLoss
        ),
        "over threshold"
    );
    assert!(vl_stop(&vl, None).is_some());
}

#[test]
fn pain_location_surfaces_in_signal_detail_and_none_omits_it() {
    // A reported body-part location must reach the shell as display context
    // on the Pain row's `detail` sub-line; absent one, `detail` names no
    // body part (never fabricated, HARD RULE 1).
    let with_loc = vec![pain_input_loc(
        PainKind::SharpJoint,
        6,
        PainTrend::Stable,
        false,
        Some("Left knee"),
    )];
    let row = signal_states(&with_loc, None, false, &[])
        .into_iter()
        .find(|s| s.signal == ReadinessSignal::Pain)
        .expect("pain row present");
    assert!(
        row.detail.contains("Left knee"),
        "location must surface in the sub-line detail, got {:?}",
        row.detail
    );
    assert!(row.detail.contains("sharp/joint") && row.detail.contains("6/10"));

    let no_loc = vec![pain_input_loc(
        PainKind::SharpJoint,
        6,
        PainTrend::Stable,
        false,
        None,
    )];
    let row = signal_states(&no_loc, None, false, &[])
        .into_iter()
        .find(|s| s.signal == ReadinessSignal::Pain)
        .expect("pain row present");
    assert!(
        !row.detail.to_lowercase().contains("knee"),
        "no location must be fabricated when None, got {:?}",
        row.detail
    );
    assert_eq!(row.detail, "sharp/joint · 6/10");
}

#[test]
fn pain_location_names_the_body_part_in_a_defer_reason() {
    // The persistent (Defer) branch carries a message field, so the location
    // rides along into the headline; the base reason is preserved verbatim.
    let inputs = vec![pain_input_loc(
        PainKind::SharpJoint,
        6,
        PainTrend::Stable,
        true,
        Some("Left knee"),
    )];
    match &adjustments(&inputs)[0].value {
        Adjustment::Defer { reason } => {
            assert!(reason.contains("Sharp or joint pain that isn't going away"));
            assert!(reason.contains("(Left knee)"), "reason: {reason}");
        }
        other => panic!("expected a Defer, got {other:?}"),
    }

    // No location → the Defer reason is exactly the base message.
    let inputs = vec![pain_input_loc(
        PainKind::SharpJoint,
        6,
        PainTrend::Stable,
        true,
        None,
    )];
    match &adjustments(&inputs)[0].value {
        Adjustment::Defer { reason } => assert!(
            !reason.contains('('),
            "no parenthetical location when None, got {reason}"
        ),
        other => panic!("expected a Defer, got {other:?}"),
    }
}

#[test]
fn pain_resolves_to_pain_tier_and_stop() {
    let inputs = vec![input(ReadinessSignal::Pain, 1.0)];
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::Pain)
    );
    let adj = adjustments(&inputs);
    assert!(adj.iter().any(|r| r.value == Adjustment::Stop));
    assert_eq!(adj.len(), 1, "stop must dominate");
}

#[test]
fn above_neck_illness_downgrades_without_blocking_training() {
    // autoreg-045: an above-neck illness cuts intensity but must NOT stop the
    // session. Guards the invariant that the Illness tier is raised (so the
    // shell shows the safety marker) while training stays permitted, i.e. the
    // adjustment is a DowngradeSession, never a Stop/RestDay/Defer.
    let inputs = vec![input(ReadinessSignal::Illness, 1.0)];
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::Illness)
    );
    let adj = adjustments(&inputs);
    assert!(
        adj.iter().any(|r| r.value == Adjustment::DowngradeSession),
        "above-neck illness must downgrade the session"
    );
    assert!(
        !adj.iter().any(|r| matches!(
            r.value,
            Adjustment::Stop | Adjustment::RestDay | Adjustment::Defer { .. }
        )),
        "above-neck illness must not block training"
    );
}

#[test]
fn rhr_plus_five_band_needs_two_days_then_downgrades() {
    // autoreg-040 (as amended by ready-upd-001): RHR +5–7 bpm must hold ≥2
    // days before it acts, and then it HOLDS the top end (lifts stop 1 rep
    // earlier at the planned load, runs easy); it never stops (lowest tier).
    let inputs = vec![input_streak(ReadinessSignal::RestingHr, 7.0, 2)];
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::SingleDayMarker)
    );
    let adj = adjustments(&inputs);
    assert!(adj.iter().any(|r| r.value
        == Adjustment::CapTopEnd {
            trigger: ReadinessSignal::RestingHr
        }));
    assert!(!adj.iter().any(|r| r.value == Adjustment::RestDay));
}

#[test]
fn rhr_single_day_elevation_is_a_no_op() {
    // File 06 signal spec: a single elevated morning RHR is likely noise
    // (caffeine/heat): act on ≥2 days. One day at +7 bpm yields nothing.
    let inputs = vec![input(ReadinessSignal::RestingHr, 7.0)];
    assert_eq!(resolve_safety_for_goal(&inputs, None), None);
    assert!(adjustments(&inputs).is_empty());
}

#[test]
fn rhr_plus_ten_forces_rest_day_at_red_flag_tier() {
    // ready-upd-002 (supersedes the single-reading autoreg-041 rest): ONE
    // +10 morning is likely noise → an easier day + illness check, never a
    // rest day; stop level on ≥2 consecutive mornings (or with symptoms)
    // is the safety-critical RestDay that dominates, at the Illness tier.
    let single = vec![input(ReadinessSignal::RestingHr, 10.0)];
    let adj = adjustments(&single);
    assert!(!adj.iter().any(|r| r.value == Adjustment::RestDay));
    assert!(adj.iter().any(|r| r.value == Adjustment::DowngradeSession
        && r.claim_id() == claims::AUTOREG_RHR_NOISE_001));
    assert_eq!(
        resolve_safety_for_goal(&single, None),
        Some(SafetyTier::SingleDayMarker)
    );

    let inputs = vec![input_streak(ReadinessSignal::RestingHr, 10.0, 2)];
    let adj = adjustments(&inputs);
    assert_eq!(adj.len(), 1, "rest-day stop must dominate");
    assert_eq!(adj[0].value, Adjustment::RestDay);
    assert_eq!(adj[0].claim_id(), claims::AUTOREG_RHR_NOISE_001);
    assert!(adj[0].safety_critical());
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::Illness)
    );
}

#[test]
fn high_rpe_over_target_reduces_load() {
    let inputs = vec![input(ReadinessSignal::Rpe, 2.0)];
    let adj = adjustments(&inputs);
    assert!(
        adj.iter()
            .any(|r| matches!(r.value, Adjustment::ReduceLoadPct(p) if p == 10.0))
    );
}

#[test]
fn e1rm_drop_needs_two_consecutive_sessions_to_deload() {
    // autoreg-022: the >10% e1RM drop must hold ≥2 consecutive sessions.
    let inputs = vec![input_streak(ReadinessSignal::EstimatedOneRm, 0.85, 2)];
    let adj = adjustments(&inputs);
    assert!(
        adj.iter()
            .any(|r| matches!(r.value, Adjustment::Deload { .. }))
    );
}

#[test]
fn e1rm_single_session_drop_reduces_but_never_deloads() {
    // A single-session >10% drop is not a deload trigger (autoreg-022 needs
    // ≥2 sessions); it falls through to the autoreg-006 session cap
    // (reduce top-set load ~5%) per the §5 conflict table.
    let inputs = vec![input(ReadinessSignal::EstimatedOneRm, 0.85)];
    let adj = adjustments(&inputs);
    assert!(
        !adj.iter()
            .any(|r| matches!(r.value, Adjustment::Deload { .. })),
        "single reading must not deload"
    );
    assert!(
        adj.iter()
            .any(|r| matches!(r.value, Adjustment::ReduceLoadPct(p) if p == 5.0))
    );
}

#[test]
fn clean_inputs_yield_no_stop() {
    let inputs = vec![
        input(ReadinessSignal::Rpe, 0.0),
        input(ReadinessSignal::EstimatedOneRm, 1.02),
        input(ReadinessSignal::HrvLnRmssd, 0.1),
        input(ReadinessSignal::WellnessZ, 0.0),
    ];
    assert_eq!(resolve_safety_for_goal(&inputs, None), None);
    let adj = adjustments(&inputs);
    assert!(adj.iter().all(|r| r.value != Adjustment::Stop));
    assert!(adj.is_empty());
}

#[test]
fn reds_flag_defers_above_pain() {
    // A RED-S flag alongside training pain: deferral is the sole output and
    // the safety tier is MedicalReferral (above Pain).
    let inputs = vec![
        input(ReadinessSignal::RedS, 1.0),
        input(ReadinessSignal::Pain, 1.0),
    ];
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::MedicalReferral)
    );
    let adj = adjustments(&inputs);
    assert_eq!(adj.len(), 1, "defer must dominate even over pain");
    assert!(matches!(adj[0].value, Adjustment::Defer { .. }));
    assert!(adj[0].safety_critical());
}

#[test]
fn cardiac_flag_outranks_bone_stress_and_reds() {
    let inputs = vec![
        input(ReadinessSignal::RedS, 1.0),
        input(ReadinessSignal::BoneStress, 1.0),
        input(ReadinessSignal::CardiacRedFlag, 1.0),
    ];
    let adj = adjustments(&inputs);
    assert_eq!(adj.len(), 1);
    match &adj[0].value {
        Adjustment::Defer { reason } => assert!(reason.contains("heart warning sign")),
        other => panic!("expected cardiac defer, got {other:?}"),
    }
    assert_eq!(adj[0].claim_id(), claims::SAFE_CVD_001);
}

#[test]
fn bone_stress_defers_with_urgent_referral() {
    let inputs = vec![input(ReadinessSignal::BoneStress, 1.0)];
    let adj = adjustments(&inputs);
    assert_eq!(adj.len(), 1);
    match &adj[0].value {
        Adjustment::Defer { reason } => assert!(reason.contains("as soon as you can")),
        other => panic!("expected bone-stress defer, got {other:?}"),
    }
}

#[test]
fn rpe_under_target_increases_load() {
    // RPE two below target → +7.5% load, and no safety tier.
    let inputs = vec![input(ReadinessSignal::Rpe, -2.0)];
    let adj = adjustments(&inputs);
    assert!(
        adj.iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(p) if p == 7.5))
    );
    assert_eq!(resolve_safety_for_goal(&inputs, None), None);
}

#[test]
fn e1rm_over_baseline_increases_load_without_safety_tier() {
    // e1RM ratio > 1.05 → add load; a load increase must NOT raise a tier.
    let inputs = vec![input(ReadinessSignal::EstimatedOneRm, 1.08)];
    let adj = adjustments(&inputs);
    assert!(
        adj.iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(p) if p == 3.5))
    );
    assert_eq!(resolve_safety_for_goal(&inputs, None), None);
}

#[test]
fn e1rm_drop_still_raises_objective_tier() {
    let inputs = vec![input(ReadinessSignal::EstimatedOneRm, 0.85)];
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::ObjectivePerformance)
    );
}

#[test]
fn vbt_daily_readiness_bands() {
    assert_eq!(vbt_daily_readiness(0.08).value, VbtReadiness::IncreaseLoad);
    assert_eq!(vbt_daily_readiness(-0.08).value, VbtReadiness::ReduceLoad);
    assert_eq!(vbt_daily_readiness(0.03).value, VbtReadiness::Hold);
    assert_eq!(vbt_daily_readiness(0.06).value, VbtReadiness::Hold); // band edge
}

#[test]
fn set_volume_action_add_drop_hold() {
    // Strong first set, RPE 1 under target, wellness ok → add a set.
    assert_eq!(
        set_volume_action(true, -1.0, true).value,
        SetVolumeAction::AddSet
    );
    // Short first set → drop last set regardless of RPE.
    assert_eq!(
        set_volume_action(false, 0.0, true).value,
        SetVolumeAction::DropLastSet
    );
    // RPE over target → drop last set.
    assert_eq!(
        set_volume_action(true, 1.0, true).value,
        SetVolumeAction::DropLastSet
    );
    // On-target → hold planned.
    assert_eq!(
        set_volume_action(true, 0.0, true).value,
        SetVolumeAction::HoldPlanned
    );
    // Add blocked when wellness abnormal.
    assert_eq!(
        set_volume_action(true, -1.0, false).value,
        SetVolumeAction::HoldPlanned
    );
}

#[test]
fn rpe_stop_reached_at_target() {
    assert!(rpe_stop_reached(9.0, 8.0));
    assert!(!rpe_stop_reached(7.0, 8.0));
}

#[test]
fn apre_tables_verbatim() {
    // kg = Mann 2010 lb increments plate-rounded (±5/10/15 lb → ±2.5/5/7.5 kg).
    // APRE-6 bands.
    assert_eq!(
        apre_load_adjustment_kg(ApreScheme::Apre6, 1).value,
        (-5.0, -2.5)
    );
    assert_eq!(
        apre_load_adjustment_kg(ApreScheme::Apre6, 6).value,
        (0.0, 0.0)
    );
    assert_eq!(
        apre_load_adjustment_kg(ApreScheme::Apre6, 20).value,
        (5.0, 7.5)
    );
    // APRE-10 bands.
    assert_eq!(
        apre_load_adjustment_kg(ApreScheme::Apre10, 5).value,
        (-5.0, -2.5)
    );
    assert_eq!(
        apre_load_adjustment_kg(ApreScheme::Apre10, 14).value,
        (2.5, 5.0)
    );
    // APRE-3 bands.
    assert_eq!(
        apre_load_adjustment_kg(ApreScheme::Apre3, 3).value,
        (0.0, 0.0)
    );
    assert_eq!(
        apre_load_adjustment_kg(ApreScheme::Apre3, 8).value,
        (5.0, 7.5)
    );
}

#[test]
fn multi_session_deload_triggers() {
    assert!(deload_from_rpe_load_gap(2).is_some());
    assert!(deload_from_rpe_load_gap(1).is_none());
    assert!(deload_from_rpe_creep_and_wellness(true, 3).is_some());
    assert!(deload_from_rpe_creep_and_wellness(true, 2).is_none());
    assert!(deload_from_rpe_creep_and_wellness(false, 5).is_none());
    assert!(deload_from_velocity_drop(0.08).is_some());
    assert!(deload_from_velocity_drop(0.05).is_none());
    // Failed-sessions deload is volume-only (load-neutral).
    let d = deload_from_failed_sessions(2).expect("fires");
    assert!(matches!(
        d.value,
        Adjustment::Deload { load_reduction_pct, .. } if load_reduction_pct == 0.0
    ));
    assert!(deload_from_failed_sessions(1).is_none());
}

#[test]
fn running_pace_autoreg() {
    assert!(interval_pace_autoreg(2).is_some());
    assert!(interval_pace_autoreg(1).is_none());
    assert!(slow_easy_pace_if_over_cap(false).value); // cannot hold → slow
    assert!(!slow_easy_pace_if_over_cap(true).value);
}

#[test]
fn multiday_hrv_and_wellness_streaks() {
    assert!(!hrv_suppressed_recovery_day(2).value);
    assert!(hrv_suppressed_recovery_day(3).value);
    assert!(!wellness_rhr_multiday_easy(2, false).value);
    assert!(!wellness_rhr_multiday_easy(1, true).value);
    assert!(wellness_rhr_multiday_easy(2, true).value);
}

#[test]
fn safety_dominates_concurrent_performance_trigger() {
    // Pain + a genuine performance deload trigger fire together.
    let inputs = vec![
        input(ReadinessSignal::Pain, 1.0),
        input(ReadinessSignal::EstimatedOneRm, 0.80),
    ];
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::Pain)
    );
    let adj = adjustments(&inputs);
    assert_eq!(adj.len(), 1);
    assert_eq!(adj[0].value, Adjustment::Stop);
}

// --- autoreg-044: never auto-increase load into suppressed recovery ---

#[test]
fn performance_up_with_suppressed_wellness_never_increases_load() {
    // autoreg-044 + §5 conflict table: good performance but poor subjective
    // recovery → proceed, cap top-end, do NOT add load. e1RM +8% would
    // normally emit IncreaseLoadPct; a wellness z of −2.0 held two consecutive
    // days strips it and downgrades intensity.
    let inputs = vec![
        input(ReadinessSignal::EstimatedOneRm, 1.08),
        input_streak(ReadinessSignal::WellnessZ, -2.0, 2),
    ];
    let adj = adjustments(&inputs);
    assert!(
        !adj.iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_))),
        "no load increase may survive a wellness suppression"
    );
    assert!(
        adj.iter().any(|r| r.value
            == Adjustment::CapTopEnd {
                trigger: ReadinessSignal::WellnessZ
            }),
        "the 2-day wellness arm still holds the top end (ready-upd-001)"
    );
}

#[test]
fn hrv_and_rhr_suppression_also_block_increases() {
    // The reconciliation pass covers every suppression channel: HRV below
    // the SWC band and a corroborated elevated RHR each strip an
    // RPE-under-target load increase.
    let hrv = vec![
        input(ReadinessSignal::Rpe, -2.0),
        input(ReadinessSignal::HrvLnRmssd, -1.0),
    ];
    assert!(
        !adjustments(&hrv)
            .iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_)))
    );

    let rhr = vec![
        input(ReadinessSignal::Rpe, -2.0),
        input_streak(ReadinessSignal::RestingHr, 6.0, 2),
    ];
    assert!(
        !adjustments(&rhr)
            .iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_)))
    );

    // Sanity: without suppression the increase does flow.
    let clean = vec![input(ReadinessSignal::Rpe, -2.0)];
    assert!(
        adjustments(&clean)
            .iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_)))
    );
}

// --- autoreg-010: goal-dependent velocity-loss stop ---

#[test]
fn velocity_loss_stop_is_goal_dependent() {
    // Verbatim bands: 10% power / 15–20% strength+power / 25–40%
    // hypertrophy. Each goal terminates at its band ceiling.
    let vl12 = vec![input(ReadinessSignal::VelocityLoss, 12.0)];
    let vl22 = vec![input(ReadinessSignal::VelocityLoss, 22.0)];
    let vl42 = vec![input(ReadinessSignal::VelocityLoss, 42.0)];

    let fires = |inputs: &[ReadinessInput], goal: Option<&Goal>| {
        adjustments_for_goal(inputs, goal)
            .iter()
            .any(|r| r.value == Adjustment::DowngradeSession)
    };

    // Power: threshold 10%, 12% already terminates.
    assert!(fires(&vl12, Some(&Goal::Power)));
    // Strength: 12% is within plan; 22% exceeds the 20% ceiling.
    assert!(!fires(&vl12, Some(&Goal::Strength)));
    assert!(fires(&vl22, Some(&Goal::Strength)));
    // Hypertrophy: 22% within the 25–40 band's ceiling; 42% exceeds it.
    assert!(!fires(&vl22, Some(&Goal::Hypertrophy)));
    assert!(fires(&vl42, Some(&Goal::Hypertrophy)));
    // No goal → the conservative 20% default.
    assert!(fires(&vl22, None));
    assert!(!fires(&vl12, None));
}

// --- §5 tier 4: SubjectiveMultiDay needs ≥3 suppressed days ---

#[test]
fn deep_single_wellness_day_downgrades_but_does_not_raise_the_multiday_tier() {
    // autoreg-030 single-day arm (as scoped by ready-upd-001): a deep one-day
    // z ≤ −1.5 holds the top end (cap, no load added), but the §5 tier-4
    // SubjectiveMultiDay escalation still needs a ≥3-day run.
    let inputs = vec![input(ReadinessSignal::WellnessZ, -2.0)];
    let adj = adjustments(&inputs);
    assert!(adj.iter().any(|r| r.value
        == Adjustment::CapTopEnd {
            trigger: ReadinessSignal::WellnessZ
        }));
    assert!(!adj.iter().any(|r| r.value == Adjustment::DowngradeSession));
    assert_ne!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::SubjectiveMultiDay),
        "one bad day is not multi-day suppression"
    );
}

#[test]
fn three_day_wellness_suppression_raises_multiday_tier() {
    // z ≤ −1 held ≥3 days (streak) → tier 4 + an intensity cut.
    let inputs = vec![input_streak(ReadinessSignal::WellnessZ, -1.1, 3)];
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::SubjectiveMultiDay)
    );
    let adj = adjustments(&inputs);
    assert!(adj.iter().any(|r| r.value == Adjustment::DowngradeSession));
}

// --- autoreg-030 soreness item ---

#[test]
fn soreness_item_at_six_downgrades_intensity() {
    // Soreness ≥6/7 → hold the lifts' top end (§5: "train; modify the sore
    // muscle's work; keep systemic intensity"); below → nothing.
    let sore = vec![input(ReadinessSignal::Soreness, 6.0)];
    let adj = adjustments(&sore);
    assert!(adj.iter().any(|r| r.value
        == Adjustment::CapTopEnd {
            trigger: ReadinessSignal::Soreness
        }));
    // Localized soreness modifies, never stops or raises a stop tier.
    assert!(
        !adj.iter()
            .any(|r| matches!(r.value, Adjustment::Stop | Adjustment::RestDay))
    );

    let mild = vec![input(ReadinessSignal::Soreness, 5.0)];
    assert!(adjustments(&mild).is_empty());
}

#[test]
fn soreness_suppression_blocks_load_increase() {
    // autoreg-044 reconciliation includes the soreness channel.
    let inputs = vec![
        input(ReadinessSignal::EstimatedOneRm, 1.08),
        input(ReadinessSignal::Soreness, 7.0),
    ];
    assert!(
        !adjustments(&inputs)
            .iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_)))
    );
}

// --- File 08 Table 4.1: graded pain model ---

#[test]
fn bare_pain_report_still_hard_stops() {
    // Backward compatibility: a shell that predates the graded model sends
    // Pain with no detail: the conservative hard stop must survive.
    let inputs = vec![input(ReadinessSignal::Pain, 1.0)];
    let adj = adjustments(&inputs);
    assert_eq!(adj.len(), 1);
    assert_eq!(adj[0].value, Adjustment::Stop);
    assert_eq!(adj[0].claim_id(), claims::SAFE_PAIN_001);
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::Pain)
    );
}

#[test]
fn sharp_joint_pain_stops_and_defers_when_persistent() {
    // Table 4.1 structural row (safety-038): STOP; DEFER if it persists.
    let acute = vec![pain_input(
        PainKind::SharpJoint,
        4,
        PainTrend::Stable,
        false,
    )];
    let adj = adjustments(&acute);
    assert_eq!(adj.len(), 1);
    assert_eq!(adj[0].value, Adjustment::Stop);
    assert_eq!(adj[0].claim_id(), claims::SAFE_PAIN_STRUCT_001);
    assert!(adj[0].safety_critical());
    assert_eq!(
        resolve_safety_for_goal(&acute, None),
        Some(SafetyTier::Pain)
    );

    let persistent = vec![pain_input(PainKind::SharpJoint, 4, PainTrend::Stable, true)];
    let adj = adjustments(&persistent);
    assert_eq!(adj.len(), 1);
    assert!(matches!(adj[0].value, Adjustment::Defer { .. }));
}

#[test]
fn tolerable_tendon_pain_modifies_and_monitors_not_stops() {
    // safety-039 tolerable band: ≤5/10, stable → MODIFY/continue with
    // monitoring; explicitly avoid complete rest, never Stop/RestDay.
    let inputs = vec![pain_input(
        PainKind::TendonLoadRelated,
        3,
        PainTrend::Stable,
        false,
    )];
    let adj = adjustments(&inputs);
    assert!(
        adj.iter()
            .any(|r| matches!(r.value, Adjustment::ModifyAndMonitor { .. }))
    );
    assert!(
        !adj.iter().any(|r| matches!(
            r.value,
            Adjustment::Stop | Adjustment::RestDay | Adjustment::Defer { .. }
        )),
        "tolerable tendon pain must not block training (avoid complete rest)"
    );
    assert_eq!(adj[0].claim_id(), claims::SAFE_TENDON_001);
    assert!(adj[0].safety_critical());
    // Still surfaces at the Pain tier so the shell shows the safety marker.
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::Pain)
    );
    // The band edge: 5/10 stable is still tolerable.
    let edge = vec![pain_input(
        PainKind::TendonLoadRelated,
        5,
        PainTrend::Stable,
        false,
    )];
    assert!(
        adjustments(&edge)
            .iter()
            .any(|r| matches!(r.value, Adjustment::ModifyAndMonitor { .. }))
    );
}

#[test]
fn reactive_tendon_pain_reduces_then_defers_when_persistent() {
    // safety-039 reactive band: >5/10 OR rising → REDUCE load; if it
    // persists → DEFER.
    let severe = vec![pain_input(
        PainKind::TendonLoadRelated,
        6,
        PainTrend::Stable,
        false,
    )];
    let adj = adjustments(&severe);
    assert!(adj.iter().any(|r| r.value == Adjustment::DowngradeSession));
    assert!(!adj.iter().any(|r| r.value == Adjustment::Stop));

    let rising = vec![pain_input(
        PainKind::TendonLoadRelated,
        3,
        PainTrend::Rising,
        false,
    )];
    assert!(
        adjustments(&rising)
            .iter()
            .any(|r| r.value == Adjustment::DowngradeSession),
        "rising trend is reactive even at low severity"
    );

    let persistent = vec![pain_input(
        PainKind::TendonLoadRelated,
        6,
        PainTrend::Rising,
        true,
    )];
    let adj = adjustments(&persistent);
    assert_eq!(adj.len(), 1, "defer must dominate");
    assert!(matches!(adj[0].value, Adjustment::Defer { .. }));
    assert_eq!(adj[0].claim_id(), claims::SAFE_TENDON_001);
}

#[test]
fn doms_continues_without_stop_or_tier() {
    // Table 4.1 row 1: normal training discomfort → continue. No
    // adjustment, no Pain tier.
    let inputs = vec![pain_input(PainKind::Doms, 4, PainTrend::Stable, false)];
    assert!(adjustments(&inputs).is_empty());
    assert_eq!(resolve_safety_for_goal(&inputs, None), None);
}

#[test]
fn uncharacterized_pain_kind_stays_conservative() {
    // PainKind::Other → same hard stop as a bare report.
    let inputs = vec![pain_input(PainKind::Other, 2, PainTrend::Stable, false)];
    let adj = adjustments(&inputs);
    assert_eq!(adj.len(), 1);
    assert_eq!(adj[0].value, Adjustment::Stop);
}

#[test]
fn medical_referral_still_outranks_graded_pain() {
    // File 08 §5: a RED-S flag outranks even a tolerable-tendon adjustment.
    let inputs = vec![
        input(ReadinessSignal::RedS, 1.0),
        pain_input(PainKind::TendonLoadRelated, 3, PainTrend::Stable, false),
    ];
    let adj = adjustments(&inputs);
    assert_eq!(adj.len(), 1);
    assert!(matches!(adj[0].value, Adjustment::Defer { .. }));
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::MedicalReferral)
    );
}

// --- autoreg-006 second clause: RPE cap alongside the −5% cut ---

#[test]
fn e1rm_dip_caps_session_rpe_alongside_load_cut() {
    // e1RM < baseline − 5% → BOTH the ~5% top-set cut and the planned
    // RPE − 1 session cap, each cited to the per-rule Strong claim
    // (Helms 2018), never the generic autoregulation claim.
    let inputs = vec![input(ReadinessSignal::EstimatedOneRm, 0.93)];
    let adj = adjustments(&inputs);
    let cut = adj
        .iter()
        .find(|r| matches!(r.value, Adjustment::ReduceLoadPct(p) if p == 5.0))
        .expect("load cut fires");
    let cap = adj
        .iter()
        .find(|r| matches!(r.value, Adjustment::CapRpe(1)))
        .expect("RPE cap fires");
    for r in [cut, cap] {
        assert_eq!(r.claim_id(), claims::AUTOREG_E1RM_GATE_001);
        assert!((r.confidence() - 0.90).abs() < f32::EPSILON);
    }
    // The two-session deload path is untouched: no CapRpe there.
    let deload = vec![input_streak(ReadinessSignal::EstimatedOneRm, 0.85, 2)];
    assert!(
        !adjustments(&deload)
            .iter()
            .any(|r| matches!(r.value, Adjustment::CapRpe(_)))
    );
}

// --- autoreg-029: parasympathetic saturation blocks auto load-adds ---

#[test]
fn hrv_saturation_in_high_load_block_strips_load_increases() {
    // RPE two under target would normally add load; lnRMSSD above the SWC
    // upper band during a high-load block must strip it (autoreg-029).
    let inputs = vec![
        input(ReadinessSignal::Rpe, -2.0),
        input(ReadinessSignal::HrvLnRmssd, 1.0),
    ];
    let in_block = adjustments_with_context(&inputs, None, true);
    assert!(
        !in_block
            .iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_))),
        "no auto load-add under saturation in a high-load block"
    );
    // Outside a high-load block the same readings still add load: high
    // HRV alone is not saturation.
    let out_of_block = adjustments_with_context(&inputs, None, false);
    assert!(
        out_of_block
            .iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_)))
    );
    // Scalar helper carries the per-rule Moderate claim.
    let hold = hrv_saturation_hold(1.0, true);
    assert!(hold.value);
    assert_eq!(hold.claim_id(), claims::AUTOREG_HRV_SAT_001);
    assert!(!hrv_saturation_hold(1.0, false).value);
    assert!(!hrv_saturation_hold(0.4, true).value); // within SWC band
}

// --- autoreg-028 second trigger: single-day >1 SD + 2-day downtrend ---

#[test]
fn hrv_single_day_trigger_needs_both_depth_and_downtrend() {
    assert!(hrv_single_day_downgrade(-1.2, 2).is_some());
    assert!(hrv_single_day_downgrade(-1.2, 1).is_none(), "no downtrend");
    assert!(hrv_single_day_downgrade(-0.8, 3).is_none(), "within 1 SD");
    let d = hrv_single_day_downgrade(-1.5, 2).unwrap();
    assert_eq!(d.value, Adjustment::DowngradeSession);
}

// --- autoreg-025: MRV sign cluster deload ---

#[test]
fn mrv_sign_cluster_deloads_with_rp_magnitude() {
    assert!(mrv_signs_deload(false).is_none());
    let d = mrv_signs_deload(true).expect("cluster fires");
    assert!(matches!(
        d.value,
        Adjustment::Deload {
            volume_reduction_pct,
            load_reduction_pct,
            weeks: 1,
        } if volume_reduction_pct == 50.0 && load_reduction_pct == 10.0
    ));
    assert_eq!(d.claim_id(), claims::AUTOREG_MRV_001);
    // ExpertOpinion: the RP cluster is a heuristic, not trial evidence.
    assert!((d.confidence() - 0.30).abs() < f32::EPSILON);
}

// --- autoreg-019: small-lifter cap on the APRE top band ---

#[test]
fn apre_top_band_caps_relative_to_load_for_small_lifters() {
    // reps 14 on APRE-6 is the open top arm → flat band +5..7.5 kg. At the
    // 50 kg reference load (kg analogue of the KB's 100 lb worked example) the
    // +15% cap equals the flat band and changes nothing.
    assert_eq!(
        apre_load_adjustment_capped_kg(ApreScheme::Apre6, 14, 50.0).value,
        (5.0, 7.5)
    );
    // A 30 kg working load shrinks the jump proportionally (7.5 kg would be
    // a +25% leap; capped to the band's relative size: 3.0–4.5 kg).
    assert_eq!(
        apre_load_adjustment_capped_kg(ApreScheme::Apre6, 14, 30.0).value,
        (3.0, 4.5)
    );
    // Heavier lifters keep the flat band: the cap only ever shrinks.
    assert_eq!(
        apre_load_adjustment_capped_kg(ApreScheme::Apre6, 14, 150.0).value,
        (5.0, 7.5)
    );
    // Load *reductions* are never capped (weakening a fatigue response).
    assert_eq!(
        apre_load_adjustment_capped_kg(ApreScheme::Apre6, 1, 30.0).value,
        (-5.0, -2.5)
    );
}

// --- autoreg-032: threshold re-test ---

#[test]
fn threshold_retest_needs_swc_improvement_sustained_two_weeks() {
    assert!(threshold_retest_due(true, 2).value);
    assert!(threshold_retest_due(true, 3).value);
    assert!(!threshold_retest_due(true, 1).value);
    assert!(!threshold_retest_due(false, 4).value);
    assert_eq!(
        threshold_retest_due(true, 2).claim_id(),
        claims::AUTOREG_PACE_RETEST_001
    );
}

// --- decoupling validity gate: efforts must exceed 20 min ---

#[test]
fn decoupling_ignored_for_short_efforts() {
    let short = vec![ReadinessInput {
        effort_min: Some(15.0),
        ..input(ReadinessSignal::AerobicDecoupling, 14.0)
    }];
    assert!(
        adjustments(&short).is_empty(),
        "decoupling from a ≤20 min effort is invalid - never acted on"
    );
    let long = vec![ReadinessInput {
        effort_min: Some(45.0),
        ..input(ReadinessSignal::AerobicDecoupling, 14.0)
    }];
    assert!(
        adjustments(&long)
            .iter()
            .any(|r| r.value == Adjustment::DowngradeSession)
    );
    // Untracked duration (wire default, effort_min None) is not KNOWN-valid,
    // so the validity gate discards it, no downgrade.
    let untracked = vec![input(ReadinessSignal::AerobicDecoupling, 14.0)];
    assert!(
        !adjustments(&untracked)
            .iter()
            .any(|r| r.value == Adjustment::DowngradeSession),
        "untracked-duration decoupling is not known-valid - never acted on"
    );
}

// --- File 08 safety-041/042 + File 06 autoreg-042 continuum ---

#[test]
fn decline_despite_deload_rests_and_defers() {
    // safety-042: all three conditions required.
    assert!(unexplained_decline_rest_defer(2, true, true).is_some());
    assert!(unexplained_decline_rest_defer(1, true, true).is_none());
    assert!(unexplained_decline_rest_defer(2, false, true).is_none());
    assert!(unexplained_decline_rest_defer(2, true, false).is_none());
    let d = unexplained_decline_rest_defer(3, true, true).unwrap();
    assert!(matches!(d.value, Adjustment::Defer { .. }));
    assert!((d.confidence() - 0.90).abs() < f32::EPSILON);
}

#[test]
fn nfor_cluster_mandates_recovery_and_referral() {
    // autoreg-042: ≥2 weeks decrement AND ≥2 suppressed wellness domains.
    assert!(nfor_cluster_defer(2, 2).is_some());
    assert!(nfor_cluster_defer(1, 3).is_none());
    assert!(nfor_cluster_defer(4, 1).is_none());
    let d = nfor_cluster_defer(2, 2).unwrap();
    match &d.value {
        Adjustment::Defer { reason } => {
            assert!(reason.contains("recovery"));
            assert!(reason.contains("professional"));
        }
        other => panic!("expected Defer, got {other:?}"),
    }
    assert_eq!(d.claim_id(), claims::AUTOREG_NFOR_001);
    // File 06 grades the cluster ExpertOpinion: safety-critical anyway.
    assert!((d.confidence() - 0.30).abs() < f32::EPSILON);
    assert!(d.safety_critical());
}

// --- Check-in → derived readiness signals ---

const D: i64 = 86_400;

fn checkin(day: i64, sleep: Option<u8>, sore: Option<u8>, mood: Option<u8>) -> CheckinInput {
    CheckinInput {
        observed_at: day * D + 100,
        sleep_quality: sleep,
        soreness: sore,
        mood,
        fatigue: None,
        stress: None,
        resting_hr_bpm: None,
        hrv_rmssd_ms: None,
    }
}

fn derived(inputs: &[ReadinessInput], signal: ReadinessSignal) -> Option<ReadinessInput> {
    inputs.iter().find(|i| i.signal == signal).cloned()
}

#[test]
fn below_minimum_checkins_emits_no_signal_only_a_collecting_status() {
    // A fresh user: 4 check-in days < MIN (7). No fabricated z: an honest
    // "collecting baseline - 4 of 7" status instead (HR1).
    let checkins: Vec<CheckinInput> = (0..4)
        .map(|d| checkin(d, Some(3), Some(3), Some(3)))
        .collect();
    let out = derive_readiness(&checkins, 0, None);
    assert!(out.inputs.is_empty(), "no derived signal below the minimum");
    let wellness = out
        .collecting
        .iter()
        .find(|b| b.signal == ReadinessSignal::WellnessZ)
        .expect("wellness collecting status");
    assert_eq!(wellness.have, 4);
    assert_eq!(wellness.need, MIN_BASELINE_CHECKINS);
}

#[test]
fn wellness_composite_z_is_derived_from_history_and_feeds_the_rule() {
    // Six good baseline days that VARY a little (so the baseline SD is real,
    // like actual data), then TWO consecutive rough mornings (sleep 2 /
    // soreness 4 / mood 3): goodness = (2 + (6−4) + 3)/3 = 2.33, well below the
    // ~4 baseline → a strong negative z computed by the CORE, no user z entry.
    // Two consecutive suppressed days arm the autoreg-030 downgrade (a single
    // day would not — WELLNESS-001 never vetoes one day).
    // Baseline composites alternate 4.33 (5/2/4) and 3.67 (4/3/4): mean 4.0.
    let mut checkins: Vec<CheckinInput> = (0..6)
        .map(|d| {
            if d % 2 == 0 {
                checkin(d, Some(5), Some(2), Some(4)) // (5 + 4 + 4)/3 = 4.33
            } else {
                checkin(d, Some(4), Some(3), Some(4)) // (4 + 3 + 4)/3 = 3.67
            }
        })
        .collect();
    checkins.push(checkin(6, Some(2), Some(4), Some(3)));
    checkins.push(checkin(7, Some(2), Some(4), Some(3)));
    let out = derive_readiness(&checkins, 0, None);
    let w = derived(&out.inputs, ReadinessSignal::WellnessZ).expect("derived wellness z");
    assert_eq!(w.observed_at, 7 * D + 100, "stamped at the latest check-in");
    assert!(
        w.value <= -1.5,
        "rough morning vs baseline z ≤ −1.5, got {}",
        w.value
    );
    assert!(
        w.streak >= 2,
        "two consecutive rough mornings → streak ≥ 2, got {}",
        w.streak
    );
    assert!(
        wellness_downgrade(&out.inputs).is_some(),
        "the derived z over two consecutive days drives the autoreg-030 downgrade"
    );
}

#[test]
fn one_off_hrv_morning_does_not_trip_autoreg_028_via_the_rolling_mean() {
    // autoreg-028 reads a 7-day ROLLING lnRMSSD, not a single morning. The
    // baseline is matured (22 reading-days spanning 21 days) with one low
    // morning (44) in the trailing 7: the rolling mean must average it back
    // into the SWC band, so a lone bad reading cannot trip the gate.
    let ms = [
        55.0, 52.0, 60.0, 48.0, 66.0, 55.0, 62.0, 50.0, 58.0, 64.0, 53.0, 61.0, 49.0, 57.0, 63.0,
        56.0, 62.0, 51.0, 60.0, 54.0, 63.0, 44.0, // trailing 7: one low morning
    ];
    let checkins: Vec<CheckinInput> = ms
        .iter()
        .enumerate()
        .map(|(d, &v)| CheckinInput {
            observed_at: d as i64 * D + 100,
            hrv_rmssd_ms: Some(v),
            ..Default::default()
        })
        .collect();
    let out = derive_readiness(&checkins, 0, None);
    let h = derived(&out.inputs, ReadinessSignal::HrvLnRmssd).expect("derived hrv z");
    assert!(
        h.value >= HRV_SWC_LOWER_Z,
        "one off morning must not drag the 7-day rolling mean below the SWC gate, got {}",
        h.value
    );
    assert!(
        hrv_downgrade(&out.inputs).is_none(),
        "a single suppressed morning does not downgrade under the rolling window"
    );
}

#[test]
fn four_of_seven_suppressed_hrv_days_trip_autoreg_028() {
    // Four of the trailing seven mornings suppressed: the 7-day rolling mean
    // crosses the SWC lower bound and the downgrade fires - the sustained
    // suppression the gate is meant to catch (vs the single off morning). The
    // baseline is matured (22 reading-days spanning 21 days) so the channel emits.
    let ms = [
        55.0, 52.0, 60.0, 48.0, 66.0, 55.0, 62.0, 50.0, 58.0, 64.0, 53.0, 61.0, 49.0, 57.0, 63.0,
        42.0, 45.0, 60.0, 43.0, 61.0, 44.0, 59.0, // trailing 7: four low
    ];
    let checkins: Vec<CheckinInput> = ms
        .iter()
        .enumerate()
        .map(|(d, &v)| CheckinInput {
            observed_at: d as i64 * D + 100,
            hrv_rmssd_ms: Some(v),
            ..Default::default()
        })
        .collect();
    let out = derive_readiness(&checkins, 0, None);
    let h = derived(&out.inputs, ReadinessSignal::HrvLnRmssd).expect("derived hrv z");
    assert!(
        h.value < HRV_SWC_LOWER_Z,
        "four suppressed of seven cross the rolling SWC gate, got {}",
        h.value
    );
    assert!(
        hrv_downgrade(&out.inputs).is_some(),
        "rolling suppression downgrades"
    );
}

#[test]
fn hrv_baseline_matures_at_four_readings_per_week_over_three_weeks() {
    // The HRV baseline needs both enough distinct reading-days (12) AND a
    // long-enough calendar span (21 days). Readings every other day for 22
    // days (12 reading-days over a 22-day span) satisfy both → active.
    let checkins: Vec<CheckinInput> = (0..12)
        .map(|i| CheckinInput {
            observed_at: (i * 2) * D + 100,
            hrv_rmssd_ms: Some(55.0 + (i % 4) as f64),
            ..Default::default()
        })
        .collect();
    let out = derive_readiness(&checkins, 0, None);
    assert!(
        derived(&out.inputs, ReadinessSignal::HrvLnRmssd).is_some(),
        "12 readings over a 22-day span is a mature HRV baseline"
    );
}

#[test]
fn hrv_baseline_not_mature_after_twelve_readings_in_one_week() {
    // Twelve reading-days but packed into 12 consecutive days (span 11 < 21):
    // enough readings, too short a span → still collecting, no z emitted.
    let checkins: Vec<CheckinInput> = (0..12)
        .map(|d| CheckinInput {
            observed_at: d * D + 100,
            hrv_rmssd_ms: Some(55.0 + (d % 4) as f64),
            ..Default::default()
        })
        .collect();
    let out = derive_readiness(&checkins, 0, None);
    assert!(
        derived(&out.inputs, ReadinessSignal::HrvLnRmssd).is_none(),
        "12 readings inside 12 days is too short a span to trust"
    );
    assert!(
        out.collecting
            .iter()
            .any(|b| b.signal == ReadinessSignal::HrvLnRmssd),
        "the channel reports an honest collecting status"
    );
}

#[test]
fn hrv_verdict_expires_after_seven_days_without_readings() {
    // 22 days of low HRV mature an active baseline; then 10 days of
    // sleep-only check-ins leave the newest HRV reading >7 days behind the
    // newest check-in, so the folded verdict expires and no HRV input emits.
    let mut checkins: Vec<CheckinInput> = (0..22)
        .map(|d| CheckinInput {
            observed_at: d * D + 100,
            hrv_rmssd_ms: Some(40.0),
            ..Default::default()
        })
        .collect();
    for d in 22..32 {
        checkins.push(CheckinInput {
            observed_at: d * D + 100,
            sleep_quality: Some(3),
            ..Default::default()
        });
    }
    let out = derive_readiness(&checkins, 0, None);
    assert!(
        derived(&out.inputs, ReadinessSignal::HrvLnRmssd).is_none(),
        "an HRV verdict must not outlive 7+ days without readings"
    );
}

#[test]
fn wellness_goodness_is_the_mean_of_the_answered_five_items() {
    // 5-item short-form: sleep + mood good-direction; soreness, fatigue
    // and stress reverse-scored (6 − x). All five answered.
    let all = CheckinInput {
        sleep_quality: Some(4), // 4
        soreness: Some(2),      // 6-2 = 4
        fatigue: Some(1),       // 6-1 = 5
        stress: Some(5),        // 6-5 = 1
        mood: Some(4),          // 4
        ..Default::default()
    };
    // (4 + 4 + 5 + 1 + 4) / 5 = 3.6
    assert!((wellness_goodness(&all).unwrap() - 3.6).abs() < 1e-9);

    // Reverse-scoring bites: maxed soreness/fatigue/stress drag the composite
    // down even with perfect sleep and mood.
    let bad = CheckinInput {
        sleep_quality: Some(5), // 5
        soreness: Some(5),      // 1
        fatigue: Some(5),       // 1
        stress: Some(5),        // 1
        mood: Some(5),          // 5
        ..Default::default()
    };
    // (5 + 1 + 1 + 1 + 5) / 5 = 2.6
    assert!((wellness_goodness(&bad).unwrap() - 2.6).abs() < 1e-9);

    // Mean is over ANSWERED items only: a legacy 3-item check-in (no fatigue
    // / stress) scores exactly as it always has — replay-equivalent.
    let three = CheckinInput {
        sleep_quality: Some(2), // 2
        soreness: Some(4),      // 6-4 = 2
        mood: Some(3),          // 3
        ..Default::default()
    };
    // (2 + 2 + 3) / 3 = 2.333…
    assert!((wellness_goodness(&three).unwrap() - 7.0 / 3.0).abs() < 1e-9);

    // A single new item still scores (reverse of fatigue 5 = 1.0): never
    // dropped, so a genuinely bad lone reading can still trip the floor.
    let fatigue_only = CheckinInput {
        fatigue: Some(5),
        ..Default::default()
    };
    assert!((wellness_goodness(&fatigue_only).unwrap() - 1.0).abs() < 1e-9);

    // Nothing answered → no composite.
    assert!(wellness_goodness(&CheckinInput::default()).is_none());
}

#[test]
fn resting_hr_delta_and_two_day_streak_arm_autoreg_040() {
    // Six baseline mornings ~50 bpm, then two consecutive elevated days
    // (+6 bpm). The baseline is FROZEN at streak entry, so day 2 is scored
    // against the pre-elevation 50 bpm (delta exactly +6), not a baseline the
    // elevated days have crept into. That arms the autoreg-040 downgrade
    // (5–9 bpm for ≥2 days) instead of self-diluting below the 5 bpm floor.
    let mut checkins: Vec<CheckinInput> = (0..6)
        .map(|d| CheckinInput {
            observed_at: d * D + 100,
            resting_hr_bpm: Some(50.0),
            ..Default::default()
        })
        .collect();
    checkins.push(CheckinInput {
        observed_at: 6 * D + 100,
        resting_hr_bpm: Some(56.0),
        ..Default::default()
    });
    checkins.push(CheckinInput {
        observed_at: 7 * D + 100,
        resting_hr_bpm: Some(56.0),
        ..Default::default()
    });
    let out = derive_readiness(&checkins, 0, None);
    let r = derived(&out.inputs, ReadinessSignal::RestingHr).expect("derived rhr delta");
    assert_eq!(
        r.value, 6.0,
        "day-2 delta vs the frozen pre-elevation baseline"
    );
    assert!(
        r.streak >= 2,
        "two elevated days → streak ≥ 2, got {}",
        r.streak
    );
    // The full ladder now resolves the corroborated-RHR downgrade (autoreg-040).
    let states = signal_states(&out.inputs, None, false, &[]);
    let rhr = states
        .iter()
        .find(|s| s.signal == ReadinessSignal::RestingHr)
        .unwrap();
    assert_eq!(rhr.state, "high 2+ days");
}

#[test]
fn sustained_rhr_elevation_keeps_firing() {
    // A sustained +6 elevation must keep arming autoreg-040 on day 3 and day
    // 4: the frozen pre-elevation baseline holds delta +6.
    let build = |elevated_days: i64| -> Vec<CheckinInput> {
        let mut c: Vec<CheckinInput> = (0..20)
            .map(|d| CheckinInput {
                observed_at: d * D + 100,
                resting_hr_bpm: Some(50.0),
                ..Default::default()
            })
            .collect();
        for i in 0..elevated_days {
            c.push(CheckinInput {
                observed_at: (20 + i) * D + 100,
                resting_hr_bpm: Some(56.0),
                ..Default::default()
            });
        }
        c
    };
    for (days, want_streak) in [(3i64, 3u8), (4, 4)] {
        let out = derive_readiness(&build(days), 0, None);
        let r = derived(&out.inputs, ReadinessSignal::RestingHr).expect("rhr");
        assert_eq!(r.value, 6.0, "delta stays +6 vs the frozen baseline");
        assert_eq!(r.streak, want_streak, "streak grows with the sustained run");
        assert!(
            rhr_downgrade(&out.inputs).is_some(),
            "sustained +6 keeps arming autoreg-040 on day {days}"
        );
    }
}

#[test]
fn chronic_rhr_plus_ten_keeps_rhr_stop_firing() {
    // Chronic +10 bpm for eight straight days: the frozen pre-elevation
    // baseline holds the emitted delta at +10 so `rhr_stop` (autoreg-041, the
    // RestDay / Illness-tier safety stop) still fires on day 8.
    let mut checkins: Vec<CheckinInput> = (0..15)
        .map(|d| CheckinInput {
            observed_at: d * D + 100,
            resting_hr_bpm: Some(50.0),
            ..Default::default()
        })
        .collect();
    for i in 0..8 {
        checkins.push(CheckinInput {
            observed_at: (15 + i) * D + 100,
            resting_hr_bpm: Some(60.0),
            ..Default::default()
        });
    }
    let out = derive_readiness(&checkins, 0, None);
    let r = derived(&out.inputs, ReadinessSignal::RestingHr).expect("rhr");
    assert_eq!(r.value, 10.0, "delta stays +10 vs the frozen baseline");
    assert_eq!(r.streak, 8, "eight consecutive elevated days");
    assert!(
        rhr_stop(&out.inputs),
        "chronic +10 keeps the RHR stop firing on day 8"
    );
    assert_eq!(
        resolve_safety_for_goal(&out.inputs, None),
        Some(SafetyTier::Illness)
    );
}

#[test]
fn local_offset_splits_an_evening_morning_pair_that_utc_would_collapse() {
    // Residual (BUGS fix-batch): a late-evening + next-morning LOCAL check-in
    // must bucket into TWO local days so a streak survives. Six baseline days
    // at 50 bpm (noon UTC), then an elevated pair straddling the Berlin (+2h)
    // local-midnight boundary: 21:30 UTC = 23:30 local (day 6) and 22:30 UTC =
    // 00:30 local (day 7). In UTC both sit on day 6 and collapse to one bucket
    // → streak 1; in local +2h they split → streak 2 (arms autoreg-040).
    const BERLIN: i64 = 2 * 3600;
    let mut checkins: Vec<CheckinInput> = (0..6)
        .map(|d| CheckinInput {
            observed_at: d * D + 12 * 3600, // noon UTC, unambiguous
            resting_hr_bpm: Some(50.0),
            ..Default::default()
        })
        .collect();
    checkins.push(CheckinInput {
        observed_at: 6 * D + 21 * 3600 + 30 * 60, // 21:30 UTC → 23:30 Berlin, local day 6
        resting_hr_bpm: Some(56.0),
        ..Default::default()
    });
    checkins.push(CheckinInput {
        observed_at: 6 * D + 22 * 3600 + 30 * 60, // 22:30 UTC → 00:30 Berlin, local day 7
        resting_hr_bpm: Some(56.0),
        ..Default::default()
    });

    // Local +2h: the pair is two distinct days → streak ≥ 2 → downgrade fires.
    let local = derive_readiness(&checkins, BERLIN, None);
    let r = derived(&local.inputs, ReadinessSignal::RestingHr).expect("derived rhr delta");
    assert!(
        r.streak >= 2,
        "local pair spans two days → streak ≥ 2, got {}",
        r.streak
    );
    assert!(
        (5.0..10.0).contains(&r.value),
        "delta in the 5–9 band, got {}",
        r.value
    );
    assert!(
        rhr_downgrade(&local.inputs).is_some(),
        "the preserved 2-day streak arms the autoreg-040 downgrade"
    );

    // Same stamps under UTC bucketing: the pair collapses to one day → the
    // streak breaks (this is exactly the bug the offset threading fixes).
    let utc = derive_readiness(&checkins, 0, None);
    let r0 = derived(&utc.inputs, ReadinessSignal::RestingHr).expect("derived rhr delta");
    assert!(
        r0.streak < 2,
        "UTC collapse loses a day → streak < 2, got {}",
        r0.streak
    );
    assert!(
        rhr_downgrade(&utc.inputs).is_none(),
        "collapsed to a single elevated day, autoreg-040 needs ≥2"
    );
}

#[test]
fn derived_signal_never_fires_what_the_same_value_entered_manually_would_not() {
    // Property: a derived WellnessZ and the same z entered manually must
    // produce the identical adjustment set: derivation only supplies inputs.
    // Soreness stays at 4 (not the scale max) so the WellnessZ channel is the
    // only derived signal — the soreness endpoint bridge is exercised
    // separately (`bridged_maxed_soreness_fires_the_seven_point_gate`).
    let mut checkins: Vec<CheckinInput> = (0..6)
        .map(|d| {
            if d % 2 == 0 {
                checkin(d, Some(5), Some(2), Some(4))
            } else {
                checkin(d, Some(4), Some(2), Some(5))
            }
        })
        .collect();
    checkins.push(checkin(6, Some(1), Some(4), Some(1)));
    let out = derive_readiness(&checkins, 0, None);
    let w = derived(&out.inputs, ReadinessSignal::WellnessZ).unwrap();
    let manual = vec![ReadinessInput {
        observed_at: w.observed_at,
        ..input_streak(ReadinessSignal::WellnessZ, w.value, w.streak)
    }];
    assert_eq!(
        adjustments(&out.inputs),
        adjustments(&manual),
        "derived and manually-entered identical z must autoregulate identically"
    );
}

#[test]
fn a_flat_baseline_does_not_fabricate_a_dip() {
    // Seven identical neutral days: SD ≈ 0. The core must report z 0 (in
    // band), never a divide-by-zero or a false downgrade (HR3-safe).
    let checkins: Vec<CheckinInput> = (0..7)
        .map(|d| checkin(d, Some(3), Some(3), Some(3)))
        .collect();
    let out = derive_readiness(&checkins, 0, None);
    let w = derived(&out.inputs, ReadinessSignal::WellnessZ).unwrap();
    assert_eq!(w.value, 0.0);
    assert!(wellness_downgrade(&out.inputs).is_none());
}

#[test]
fn same_day_recheck_overwrites_rather_than_double_counting() {
    // Two check-ins on the same day only count once (latest wins), so a
    // corrected morning entry never skews the baseline day count.
    let mut checkins: Vec<CheckinInput> = (0..6)
        .map(|d| checkin(d, Some(3), Some(3), Some(3)))
        .collect();
    // Re-do day 0 with a different value.
    checkins.push(CheckinInput {
        observed_at: 100 + 50, // still day 0, later instant
        sleep_quality: Some(5),
        soreness: Some(1),
        mood: Some(5),
        ..Default::default()
    });
    // 6 distinct days < MIN → still collecting (proves no double count).
    let out = derive_readiness(&checkins, 0, None);
    assert!(out.inputs.is_empty());
    let w = out
        .collecting
        .iter()
        .find(|b| b.signal == ReadinessSignal::WellnessZ)
        .unwrap();
    assert_eq!(w.have, 6, "same-day recheck did not inflate the day count");
}

// ── A load increase must never survive an active pain/illness report ──
#[test]
fn load_increase_stripped_while_tolerable_pain_active() {
    // Repro: tolerable tendon pain (sev 3, stable) → a
    // ModifyAndMonitor (PainGate::Adjust), plus RPE −2.0 → IncreaseLoadPct(7.5).
    // The reconciliation pass MUST strip the increase while pain is active.
    let inputs = vec![
        pain_input(PainKind::TendonLoadRelated, 3, PainTrend::Stable, false),
        input(ReadinessSignal::Rpe, -2.0),
    ];
    let out = adjustments_with_context(&inputs, None, false);
    assert!(
        out.iter()
            .any(|r| matches!(r.value, Adjustment::ModifyAndMonitor { .. })),
        "tolerable tendon pain should still surface modify-and-monitor"
    );
    assert!(
        !out.iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_))),
        "no IncreaseLoadPct may surface while a pain signal is active"
    );
}

#[test]
fn illness_plan_recs_follow_the_neck_check_and_clear_on_an_all_clear() {
    // Fever / below-neck → one Stop; head cold → the easy session at half
    // volume; a newer "I feel fine" (0) report clears both.
    let stop = illness_plan_recs(&[input(ReadinessSignal::Illness, 2.0)]);
    assert_eq!(stop.len(), 1);
    assert_eq!(stop[0].value, Adjustment::Stop);
    // The volume cut leads: it carries the head-cold headline copy.
    let cold = illness_plan_recs(&[input(ReadinessSignal::Illness, 1.0)]);
    assert_eq!(
        cold.iter().map(|r| r.value.clone()).collect::<Vec<_>>(),
        vec![
            Adjustment::ReduceVolumePct(50.0),
            Adjustment::DowngradeSession
        ]
    );
    assert!(
        cold.iter()
            .chain(stop.iter())
            .all(|r| { r.claim_id() == claims::ILLNESS_NECK_001 })
    );
    let fine = ReadinessInput {
        observed_at: 200,
        ..input(ReadinessSignal::Illness, 0.0)
    };
    let ill = ReadinessInput {
        observed_at: 100,
        ..input(ReadinessSignal::Illness, 2.0)
    };
    assert!(illness_plan_recs(&[ill, fine]).is_empty());
}

#[test]
fn load_increase_stripped_while_above_neck_illness() {
    // Above-neck illness (downgrade) + RPE −2.0 (would raise load) → no increase.
    let inputs = vec![
        input(ReadinessSignal::Illness, 1.0),
        input(ReadinessSignal::Rpe, -2.0),
    ];
    let out = adjustments_with_context(&inputs, None, false);
    assert!(
        !out.iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_))),
        "no IncreaseLoadPct may surface while an illness signal is active"
    );
}

// ── APRE must reject a non-positive current load (no negative "increase") ──
#[test]
fn apre_rejects_nonpositive_current_load() {
    // reps 14 on APRE-6 → the flat +2.5..5 kg band. A 0 (or negative) current
    // load must NOT flip the positive jump negative via the proportional cap.
    let (lo, hi) = apre_load_adjustment_capped_kg(ApreScheme::Apre6, 14, 0.0).value;
    assert!(
        lo >= 0.0 && hi >= 0.0,
        "0 kg load must not yield a negative increase"
    );
    assert_eq!(
        (lo, hi),
        apre_load_adjustment_kg(ApreScheme::Apre6, 14).value
    );
    let (lo, hi) = apre_load_adjustment_capped_kg(ApreScheme::Apre6, 14, -50.0).value;
    assert!(
        lo >= 0.0 && hi >= 0.0,
        "negative load must not yield a negative increase"
    );
}

// ── autoreg-031/033 cite their OWN claims, not RUN-VDOT-001 ──
#[test]
fn interval_and_easy_pace_cite_their_own_claims() {
    let iv = interval_pace_autoreg(2).expect("fires at >=2 reps over target");
    assert_eq!(iv.claim_id(), claims::AUTOREG_INTERVAL_PACE_001);
    let easy = slow_easy_pace_if_over_cap(false);
    assert_eq!(easy.claim_id(), claims::AUTOREG_EASY_CAP_001);
}

// ── Objective-performance decline strips a coexisting load increase ──
#[test]
fn objective_performance_decline_strips_load_increase() {
    // Control: RPE −2.0 alone still raises load (no over-suppression).
    let rpe_only = vec![input(ReadinessSignal::Rpe, -2.0)];
    assert!(
        adjustments(&rpe_only)
            .iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_))),
        "RPE −2 alone must still raise load"
    );

    // RPE −2.0 (IncreaseLoadPct 7.5) + within-set VL 25% (> the 20% no-goal
    // ceiling) → the objective set-stop strips the increase (autoreg-044).
    let rpe_vl = vec![
        input(ReadinessSignal::Rpe, -2.0),
        input(ReadinessSignal::VelocityLoss, 25.0),
    ];
    let out = adjustments(&rpe_vl);
    assert!(
        !out.iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_))),
        "VL set-stop is objective decline → no load increase survives"
    );
    assert!(out.iter().any(|r| r.value == Adjustment::DowngradeSession));

    // RPE −1 (IncreaseLoadPct 4.0) + e1RM ratio 0.93 (< 0.95 → ReduceLoadPct)
    // → the e1RM reduction strips the increase.
    let rpe_e1rm = vec![
        input(ReadinessSignal::Rpe, -1.0),
        input(ReadinessSignal::EstimatedOneRm, 0.93),
    ];
    assert!(
        !adjustments(&rpe_e1rm)
            .iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_))),
        "e1RM ratio 0.93 → ReduceLoadPct is objective decline → no increase"
    );
}

#[test]
fn rpe_above_target_strips_a_coexisting_e1rm_load_increase() {
    // Symmetrized autoreg-044: a first-set RPE +2 over target is its
    // own ReduceLoadPct, an objective within-session decline. It must strip a
    // coexisting e1RM-increase load-add (ratio 1.06 → IncreaseLoadPct) so the
    // session is never cut and loaded up in the same breath.
    let inputs = vec![
        input(ReadinessSignal::Rpe, 2.0),
        input(ReadinessSignal::EstimatedOneRm, 1.06),
    ];
    let out = adjustments(&inputs);
    assert!(
        out.iter()
            .any(|r| matches!(r.value, Adjustment::ReduceLoadPct(_))),
        "RPE +2 over target must reduce load"
    );
    assert!(
        !out.iter()
            .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_))),
        "the RPE-above-target decline strips the e1RM load increase"
    );
}

// ── LOW: illness/RHR stop keeps a coexisting pain adjustment (tier label) ──
#[test]
fn illness_stop_keeps_a_coexisting_pain_adjustment_in_output() {
    // Below-neck illness (Stop) coexists with tolerable tendon pain
    // (PainGate::Adjust → ModifyAndMonitor), which raises SafetyTier::Pain.
    // The output must include the pain response so the headline/tier agree;
    // the illness Stop still blocks training.
    let inputs = vec![
        input(ReadinessSignal::Illness, 2.0), // below-neck / fever
        pain_input(PainKind::TendonLoadRelated, 3, PainTrend::Stable, false),
    ];
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::Pain)
    );
    let out = adjustments(&inputs);
    assert!(
        out.iter()
            .any(|r| matches!(r.value, Adjustment::ModifyAndMonitor { .. })),
        "the pain response that set the Pain tier must be in the output"
    );
    assert!(
        out.iter().any(|r| r.value == Adjustment::Stop),
        "the illness Stop still blocks training"
    );
}

// ── Catastrophic mornings on a FLAT baseline register via the floor ──
#[test]
fn a_floored_catastrophic_morning_downgrades_on_its_own() {
    // Seven identical days (SD ≈ 0 → z 0), then a sleep 1 / soreness 5 / mood 1
    // morning → composite 1.0. The z-only rule reads "normal"; the absolute
    // floor (composite ≤ 2) forces the reading to present as z ≤ −1.5, the
    // autoreg-030 single-day downgrade trigger, so ONE such morning eases the
    // session (the floor-forced −1.5 fires day one).
    let base: Vec<CheckinInput> = (0..7)
        .map(|d| checkin(d, Some(4), Some(2), Some(4)))
        .collect();
    let mut one = base;
    one.push(checkin(7, Some(1), Some(5), Some(1))); // composite 1.0
    let out1 = derive_readiness(&one, 0, None);
    let w1 = derived(&out1.inputs, ReadinessSignal::WellnessZ).expect("derived wellness z");
    assert!(
        w1.value <= WELLNESS_SINGLE_DAY_Z,
        "floored catastrophic morning presents as z ≤ −1.5, got {}",
        w1.value
    );
    assert!(
        wellness_downgrade(&out1.inputs).is_some(),
        "a single floored morning at z ≤ −1.5 downgrades (autoreg-030 single-day arm)"
    );
}

#[test]
fn wellness_z_minus_1_5_downgrades_on_a_single_day() {
    let inputs = vec![input(ReadinessSignal::WellnessZ, -1.6)];
    // Single-day arm holds the top end (ready-upd-001), no clock → today.
    assert!(matches!(
        wellness_downgrade(&inputs).map(|r| r.value),
        Some(Adjustment::CapTopEnd {
            trigger: ReadinessSignal::WellnessZ
        })
    ));
    // WELLNESS-Z-001: yesterday's check-in never drives today's single-day call.
    let yesterday = vec![ReadinessInput {
        observed_at: D + 100,
        ..input(ReadinessSignal::WellnessZ, -1.6)
    }];
    let ctx = RuleContext {
        today_epoch_day: Some(2),
        ..RuleContext::default()
    };
    assert!(wellness_downgrade_in(&yesterday, &ctx).is_none());
}

#[test]
fn wellness_z_minus_1_2_single_day_does_not_downgrade() {
    let inputs = vec![input(ReadinessSignal::WellnessZ, -1.2)];
    assert!(
        wellness_downgrade(&inputs).is_none(),
        "a single z of −1.2 is above the −1.5 single-day trigger and below the 2-day streak"
    );
}

#[test]
fn flat_baseline_normal_morning_stays_normal_despite_floor() {
    // The floor must not fire on an ordinary morning above it.
    let mut checkins: Vec<CheckinInput> = (0..7)
        .map(|d| checkin(d, Some(4), Some(2), Some(4))) // composite 4.0
        .collect();
    checkins.push(checkin(7, Some(4), Some(2), Some(4))); // composite 4.0 > floor
    let out = derive_readiness(&checkins, 0, None);
    let w = derived(&out.inputs, ReadinessSignal::WellnessZ).unwrap();
    assert_eq!(w.value, 0.0, "normal morning on a flat baseline stays z 0");
    assert!(wellness_downgrade(&out.inputs).is_none());
}

// ── Consecutive wellness dip days reach the multi-day tier. The dip is
// scored against a PAST-ONLY baseline frozen at streak entry, so a sustained
// low can neither dilute itself nor be absorbed by future dips; the streak
// climbs past the ≥3-day §5 tier-4 bar. ──
#[test]
fn five_consecutive_dip_days_reach_the_multiday_tier() {
    // mood-only check-ins so composite = mood exactly: a VARYING strong
    // baseline (5,5,4,5 → real SD) then five consecutive dips (mood 3). The
    // varying baseline gives the past-only baseline a real SD, so the dip
    // crosses z ≤ −1 from the first dip day.
    let mood = |d: i64, m: u8| checkin(d, None, None, Some(m));
    let checkins = vec![
        mood(0, 5),
        mood(1, 5),
        mood(2, 4),
        mood(3, 5),
        mood(4, 3),
        mood(5, 3),
        mood(6, 3),
        mood(7, 3),
        mood(8, 3),
    ];
    let out = derive_readiness(&checkins, 0, None);
    let w = derived(&out.inputs, ReadinessSignal::WellnessZ).expect("derived wellness z");
    assert!(
        w.streak >= 3,
        "sustained dip days → streak ≥ 3, got {}",
        w.streak
    );
    assert_eq!(
        resolve_safety_for_goal(&out.inputs, None),
        Some(SafetyTier::SubjectiveMultiDay),
        "≥3 consecutive days of z ≤ −1 raises the multi-day tier"
    );
}

#[test]
fn streak_reentry_reuses_the_pre_dip_baseline() {
    // A strong varying baseline, a 2-day dip, one recovery day, then a re-dip
    // within 2 days. The re-dip is scored against the baseline frozen BEFORE
    // the first dip (a deep z), not one the dip has already crept into: with a
    // fresh baseline the re-dip's z would be shallow (~−1.5); reusing the
    // pre-dip baseline keeps it deep (≤ −3).
    let mood = |d: i64, m: u8| checkin(d, None, None, Some(m));
    let checkins = vec![
        mood(0, 5),
        mood(1, 5),
        mood(2, 4),
        mood(3, 5),
        mood(4, 3),
        mood(5, 3),
        mood(6, 5),
        mood(7, 3),
        mood(8, 3),
    ];
    let out = derive_readiness(&checkins, 0, None);
    let w = derived(&out.inputs, ReadinessSignal::WellnessZ).expect("derived wellness z");
    assert!(
        w.value <= -3.0,
        "re-dip scored against the pre-dip baseline gives a deep z, got {}",
        w.value
    );
}

#[test]
fn bridged_maxed_soreness_fires_the_seven_point_gate() {
    // The 1–5 check-in soreness item is rescaled onto the KB's 7-point manual
    // scale (5 → 7.0), so a maxed 5/5 morning clears SORENESS_HIGH and the
    // downgrade fires. A Hooper 1–7 value that leaks into the 1–5 field is
    // clamped to 5 before rescaling, so it can't over-drive the gate.
    let base: Vec<CheckinInput> = (0..7)
        .map(|d| checkin(d, Some(4), Some(2), Some(4)))
        .collect();
    let mut maxed = base.clone();
    maxed.push(checkin(7, Some(4), Some(5), Some(4))); // maxed 1–5 soreness
    let out = derive_readiness(&maxed, 0, None);
    let s = derived(&out.inputs, ReadinessSignal::Soreness).expect("bridged soreness");
    assert_eq!(s.value, 7.0, "a 5/5 check-in rescales to the 7-point max");
    assert!(
        soreness_downgrade(&out.inputs).is_some(),
        "a maxed check-in soreness now clears the 7-point downgrade gate"
    );
    // A Hooper 1–7 value leaking into the 1–5 field clamps to 5 before rescale.
    let mut hooper = base;
    hooper.push(checkin(7, Some(4), Some(7), Some(4)));
    let out2 = derive_readiness(&hooper, 0, None);
    let s2 = derived(&out2.inputs, ReadinessSignal::Soreness).expect("bridged soreness");
    assert_eq!(
        s2.value, 7.0,
        "a leaked Hooper value is clamped to 5, then rescaled to the 7-point max"
    );
}

#[test]
fn stale_soreness_reading_does_not_bridge_but_a_recent_one_does() {
    // DOMS is transient, so the endpoint bridge is recency-gated
    // (SORENESS_RECENCY_DAYS). A maxed-sore day-0 morning followed only by
    // sleep-only check-ins on days 46..=60 leaves the newest soreness reading
    // 60 days behind the global newest check-in → NO Soreness input, so an old
    // sore morning cannot downgrade sessions indefinitely. This also keeps the
    // bridge inside compaction Rule 4's retention window (replay-equivalent).
    let mut stale = vec![checkin(0, None, Some(5), None)];
    for d in 46..=60 {
        stale.push(checkin(d, Some(3), None, None));
    }
    let out = derive_readiness(&stale, 0, None);
    assert!(
        derived(&out.inputs, ReadinessSignal::Soreness).is_none(),
        "a 60-day-old maxed-sore reading is out of the recency window"
    );

    // Same maxed-sore reading, but now yesterday with a sleep-only check-in
    // today: within the recency window → the bridge fires.
    let recent = vec![
        checkin(0, Some(3), Some(5), None),
        checkin(1, Some(3), None, None),
    ];
    let out = derive_readiness(&recent, 0, None);
    let s = derived(&out.inputs, ReadinessSignal::Soreness)
        .expect("a soreness reading within the recency window bridges");
    assert_eq!(s.value, 7.0);
}

#[test]
fn wellness_needs_two_consecutive_days_for_the_multiday_arm() {
    // autoreg-030 multi-day arm downgrades only after ≥2 consecutive
    // suppressed days at z ≤ −1 (the single-day −1.5 arm is separate).
    let one = vec![input_streak(
        ReadinessSignal::WellnessZ,
        WELLNESS_MULTIDAY_Z - 0.1,
        1,
    )];
    assert!(
        wellness_downgrade(&one).is_none(),
        "a single day at z ≈ −1.1 reaches neither arm"
    );
    let two = vec![input_streak(
        ReadinessSignal::WellnessZ,
        WELLNESS_MULTIDAY_Z - 0.1,
        2,
    )];
    assert!(
        wellness_downgrade(&two).is_some(),
        "two consecutive suppressed days downgrade"
    );
}

#[test]
fn worsening_after_24h_is_reactive() {
    // PainTrend::WorseningAfter24h is treated like Rising: reactive tendon
    // pain (reduce load) and its context reads "worsening".
    let d = PainDetail {
        kind: PainKind::TendonLoadRelated,
        severity: Some(4),
        trend: PainTrend::WorseningAfter24h,
        persists: false,
        location: None,
    };
    assert!(
        tendon_reactive(&d),
        "worsening-after-24h tendon pain is reactive"
    );
    assert!(pain_context(&d).contains("worsening"));
    let inputs = vec![pain_input(
        PainKind::TendonLoadRelated,
        4,
        PainTrend::WorseningAfter24h,
        false,
    )];
    assert_eq!(
        resolve_safety_for_goal(&inputs, None),
        Some(SafetyTier::Pain)
    );
}

#[test]
fn streak_enters_on_a_pre_dip_baseline() {
    // A varying pre-dip baseline (real SD), then a MILD dip to mood 4 — only
    // ~0.75 below the pre-dip mean. Scored against the PAST-ONLY baseline the
    // dip crosses z ≤ −1 and the streak enters and holds; a leave-one-out
    // baseline would fold the dip days into the mean and never flag them. A
    // single missed morning (day 7) does not reset the run.
    let mood = |d: i64, m: u8| checkin(d, None, None, Some(m));
    let checkins = vec![
        mood(0, 5),
        mood(1, 5),
        mood(2, 4),
        mood(3, 5),
        mood(4, 4),
        mood(5, 4),
        mood(6, 4),
        // day 7 missing
        mood(8, 4),
    ];
    let out = derive_readiness(&checkins, 0, None);
    let w = derived(&out.inputs, ReadinessSignal::WellnessZ).expect("derived wellness z");
    assert!(
        w.value <= WELLNESS_MULTIDAY_Z,
        "the dip is scored against the pre-dip mean, got {}",
        w.value
    );
    assert!(
        w.streak >= 3,
        "the mild dip enters and holds a streak on the pre-dip baseline, got {}",
        w.streak
    );
}

#[test]
fn hrv_needs_four_readings() {
    // HRV emits only with ≥ HRV_MIN_WEEKLY_READINGS readings inside the trailing
    // rolling window (autoreg-047). 21 matured reading-days but a sparse recent
    // week (only three in the trailing seven days after a gap) → no HRV input; a
    // fourth recent reading tips it over and the channel emits.
    let make = |days: Vec<i64>| -> Vec<CheckinInput> {
        days.into_iter()
            .map(|d| CheckinInput {
                observed_at: d * D + 100,
                hrv_rmssd_ms: Some(58.0 + (d % 5) as f64),
                ..Default::default()
            })
            .collect()
    };
    let sparse: Vec<i64> = (0..18).chain(25..28).collect();
    let out = derive_readiness(&make(sparse), 0, None);
    assert!(
        derived(&out.inputs, ReadinessSignal::HrvLnRmssd).is_none(),
        "fewer than four readings in the trailing week emit no HRV z"
    );
    let dense: Vec<i64> = (0..18).chain(24..28).collect();
    let out2 = derive_readiness(&make(dense), 0, None);
    assert!(
        derived(&out2.inputs, ReadinessSignal::HrvLnRmssd).is_some(),
        "four readings in the trailing week emit the HRV z"
    );
}

#[test]
fn hrv_sparse_week_reports_collecting_not_silence() {
    // A matured baseline but only three readings in the trailing rolling window
    // must not make the HRV channel vanish: it reports an honest
    // collecting(have/need) status so the strip shows it is still gathering
    // (autoreg-047).
    let make = |days: Vec<i64>| -> Vec<CheckinInput> {
        days.into_iter()
            .map(|d| CheckinInput {
                observed_at: d * D + 100,
                hrv_rmssd_ms: Some(58.0 + (d % 5) as f64),
                ..Default::default()
            })
            .collect()
    };
    let sparse: Vec<i64> = (0..18).chain(25..28).collect();
    let out = derive_readiness(&make(sparse), 0, None);
    assert!(
        derived(&out.inputs, ReadinessSignal::HrvLnRmssd).is_none(),
        "a sparse trailing week still emits no HRV z"
    );
    let status = out
        .collecting
        .iter()
        .find(|b| b.signal == ReadinessSignal::HrvLnRmssd)
        .expect("HRV reports a collecting status on a sparse week");
    assert_eq!(status.have, 3, "three readings sit in the trailing window");
    assert_eq!(
        status.need, HRV_MIN_WEEKLY_READINGS,
        "the honest target is the weekly-reading minimum"
    );
}

#[test]
fn checkin_signal_expires_without_a_recent_reading() {
    // Six baseline mornings ~60 bpm then a +10 elevated morning on day 6, with
    // NO check-in since; today is day 9 (three days later). The stale RHR
    // reading must not drive a verdict — it is set aside as `stale`, no
    // train-block — and its state row reads "out of date".
    let build = || -> Vec<CheckinInput> {
        let mut c: Vec<CheckinInput> = (0..6)
            .map(|d| CheckinInput {
                observed_at: d * D + 100,
                resting_hr_bpm: Some(60.0),
                ..Default::default()
            })
            .collect();
        c.push(CheckinInput {
            observed_at: 6 * D + 100,
            resting_hr_bpm: Some(70.0),
            ..Default::default()
        });
        c
    };
    let today = 9;
    let out = derive_readiness(&build(), 0, Some(today));
    assert!(
        derived(&out.inputs, ReadinessSignal::RestingHr).is_none(),
        "a stale RHR reading is not fed to the rules"
    );
    assert!(
        derived(&out.stale, ReadinessSignal::RestingHr).is_some(),
        "the stale reading is set aside for display"
    );
    assert!(!rhr_stop(&out.inputs), "no RHR stop from a stale reading");
    assert_eq!(
        resolve_safety_for_goal(&out.inputs, None),
        None,
        "no train-block from an old check-in"
    );
    let display: Vec<ReadinessInput> = out
        .inputs
        .iter()
        .cloned()
        .chain(out.stale.iter().cloned())
        .collect();
    let stale_states = signal_states(&display, None, false, &out.stale);
    assert_eq!(
        state_str(&stale_states, ReadinessSignal::RestingHr),
        "out of date"
    );
    assert!(
        stale_states
            .iter()
            .any(|s| s.signal == ReadinessSignal::RestingHr && s.stale),
        "the out-of-date row carries the typed stale flag"
    );

    // Control: the SAME +10 morning logged today (day 6) is a live verdict:
    // one stop-level morning → easier day + illness check (ready-upd-002).
    let out_fresh = derive_readiness(&build(), 0, Some(6));
    assert!(
        rhr_stop(&out_fresh.inputs),
        "a same-day +10 is a stop-level reading"
    );
    let fresh_states = signal_states(&out_fresh.inputs, None, false, &out_fresh.stale);
    assert_eq!(
        state_str(&fresh_states, ReadinessSignal::RestingHr),
        "high this morning: easier day, check for illness"
    );
    assert!(
        fresh_states.iter().all(|s| !s.stale),
        "a live verdict is never stale"
    );
}

// ── Worst-active pain gate ───────────────────────────────────────────────

fn pain_at(kind: PainKind, severity: u8, at: i64) -> ReadinessInput {
    ReadinessInput {
        observed_at: at,
        ..pain_input(kind, severity, PainTrend::Stable, false)
    }
}

#[test]
fn a_sharp_joint_stop_survives_a_later_milder_pain_report() {
    // Sharp joint-line pain (hard stop) then, later the same day, a separate
    // tolerable tendon ache. Latest-wins would drop the red-flag stop to
    // "modify & monitor"; the gate must resolve the WORST active report.
    let inputs = vec![
        pain_at(PainKind::SharpJoint, 8, 100),
        pain_at(PainKind::TendonLoadRelated, 3, 200),
    ];
    match pain_gate(&inputs) {
        PainGate::Block(rec) => assert_eq!(rec.value, Adjustment::Stop),
        _ => panic!("a tolerable ache must not displace the sharp-joint stop"),
    }
    assert!(pain_blocks_progression(&inputs));
}

#[test]
fn a_later_doms_report_does_not_clear_an_active_sharp_joint_stop() {
    // DOMS ("continue") is the lowest severity; reporting it after a
    // sharp-joint stop must not reopen training.
    let inputs = vec![
        pain_at(PainKind::SharpJoint, 8, 100),
        pain_at(PainKind::Doms, 4, 200),
    ];
    assert!(
        matches!(pain_gate(&inputs), PainGate::Block(_)),
        "a DOMS report cannot clear a sharp-joint hard stop"
    );
}

#[test]
fn a_pain_clear_after_a_stop_reopens_training() {
    // A pain-clear (value 0) logged after the stop supersedes it: no active
    // report remains, so the gate re-opens (transient DOMS / resolved pain).
    let inputs = vec![
        pain_at(PainKind::SharpJoint, 8, 100),
        ReadinessInput {
            observed_at: 200,
            ..input(ReadinessSignal::Pain, 0.0)
        },
    ];
    assert!(
        matches!(pain_gate(&inputs), PainGate::None),
        "a later pain-clear reopens the gate"
    );
    assert!(!pain_blocks_progression(&inputs));
}

#[test]
fn the_pain_row_describes_the_worst_active_report_not_the_newest() {
    // Sharp joint-line knee pain (stop) then a later tolerable DOMS in the
    // calves. The readiness row (which the shell's hold banner reads) must
    // describe the STOP and name the knee, not call it normal DOMS in the calves.
    let inputs = vec![
        ReadinessInput {
            observed_at: 100,
            ..pain_input_loc(
                PainKind::SharpJoint,
                8,
                PainTrend::Stable,
                false,
                Some("Left knee"),
            )
        },
        ReadinessInput {
            observed_at: 200,
            ..pain_input_loc(PainKind::Doms, 3, PainTrend::Stable, false, Some("Calves"))
        },
    ];
    let states = signal_states(&inputs, None, false, &[]);
    let row = states
        .iter()
        .find(|s| s.signal == ReadinessSignal::Pain)
        .expect("pain row present");
    assert!(
        row.state.contains("skip the exercises that load it"),
        "the row must show the stop, got {}",
        row.state
    );
    assert!(
        row.detail.contains("knee"),
        "detail names the knee, got {}",
        row.detail
    );
    assert!(
        !row.detail.to_lowercase().contains("calves"),
        "the milder DOMS must not label the row, got {}",
        row.detail
    );
}

// ── Verdicts expire against today when check-ins stop ─────────────────────

fn hrv_checkin(day: i64, ln_rmssd: f64) -> CheckinInput {
    CheckinInput {
        observed_at: day * D + 100,
        hrv_rmssd_ms: Some(ln_rmssd.exp()),
        ..Default::default()
    }
}

/// A matured HRV baseline (no low-day streaks: a flat mean with two isolated
/// high days for a real SD, then three mean days) followed by seven suppressed
/// mornings ~0.55 SD below the frozen pre-streak baseline. The trailing mean
/// scored against that frozen baseline crosses the SWC gate; scored against a
/// full-window leave-one-out baseline the suppression dilutes its own z above
/// the gate.
fn sustained_hrv_suppression_checkins() -> Vec<CheckinInput> {
    let mut lns: Vec<f64> = Vec::new();
    for i in 0..20 {
        lns.push(if i % 2 == 0 { 4.10 } else { 3.90 });
    }
    lns.push(4.00);
    lns.push(4.00);
    lns.push(4.00);
    lns.extend([3.949; 7]);
    lns.iter()
        .enumerate()
        .map(|(d, &v)| hrv_checkin(d as i64, v))
        .collect()
}

#[test]
fn a_sustained_hrv_suppression_is_measured_against_the_frozen_baseline() {
    // The emitted trailing mean must score the in-streak days
    // against the frozen pre-streak baseline, so a sustained suppression
    // cannot dilute itself out of the SWC gate.
    let checkins = sustained_hrv_suppression_checkins();
    let out = derive_readiness(&checkins, 0, None);
    let h = derived(&out.inputs, ReadinessSignal::HrvLnRmssd).expect("derived hrv z");
    assert!(
        h.value < HRV_SWC_LOWER_Z,
        "a sustained suppression stays below the SWC gate, got {}",
        h.value
    );
    assert!(
        hrv_downgrade(&out.inputs).is_some(),
        "the sustained suppression drives the autoreg-028 downgrade"
    );
}

#[test]
fn an_hrv_verdict_expires_against_today_after_check_ins_stop() {
    // The newest HRV reading IS the suppressed morning, so the
    // global-newest recency bound (g - day == 0) never expires it. With a
    // clock the verdict is additionally bounded by today; once today is more
    // than HRV_RECENCY_DAYS past the newest reading, the channel goes silent.
    let checkins = sustained_hrv_suppression_checkins();
    let newest_day = 29;
    let fresh = derive_readiness(&checkins, 0, Some(newest_day));
    assert!(
        derived(&fresh.inputs, ReadinessSignal::HrvLnRmssd).is_some(),
        "a same-day suppression still drives the HRV verdict"
    );
    let stale = derive_readiness(&checkins, 0, Some(newest_day + HRV_RECENCY_DAYS + 1));
    assert!(
        derived(&stale.inputs, ReadinessSignal::HrvLnRmssd).is_none(),
        "an HRV verdict must not outlive its data once today passes the window"
    );
}

#[test]
fn a_maxed_soreness_expires_against_today_after_check_ins_stop() {
    // A maxed-sore morning that is also the newest check-in (user
    // then stops) would persist forever under the global-newest bound alone;
    // the today bound expires it once today passes SORENESS_RECENCY_DAYS.
    let mut checkins: Vec<CheckinInput> = (0..7)
        .map(|d| checkin(d, Some(4), Some(2), Some(4)))
        .collect();
    checkins.push(checkin(7, Some(4), Some(5), Some(4)));
    let no_clock = derive_readiness(&checkins, 0, None);
    assert!(
        derived(&no_clock.inputs, ReadinessSignal::Soreness).is_some(),
        "with no clock the bridge still fires from the global-newest bound"
    );
    let stale = derive_readiness(&checkins, 0, Some(7 + SORENESS_RECENCY_DAYS + 5));
    assert!(
        derived(&stale.inputs, ReadinessSignal::Soreness).is_none(),
        "a stale maxed-sore morning must not downgrade sessions after check-ins stop"
    );
}
