//! Autoregulation core: readiness inputs → evidence-cited adjustments.
//!
//! Pure, deterministic logic derived from knowledge-base File 06
//! (`knowledge-base/extracted/06-autoregulation.md`): readiness-signal
//! definitions, the ~50 IF→THEN adjustment rules with verbatim thresholds, and
//! the §5 six-tier safety ladder. No IO, no `SystemTime`, no randomness.
//!
//! Every emitted recommendation is a [`Recommended<Adjustment>`] whose
//! [`Evidence`] + [`ConfidenceTag`] come from the compile-time claim registry
//! (`crate::evidence`). Safety-critical stops (pain, fever/illness, RHR +10 bpm)
//! dominate all optimization triggers.

use crate::evidence;
use crate::schema::{
    Adjustment, IllnessSeverity, ReadinessInput, ReadinessSignal, Recommended, SafetyTier,
};

/// Build a `Recommended<Adjustment>` from a registry claim id (must exist).
fn recommend(value: Adjustment, claim_id: &str) -> Recommended<Adjustment> {
    let e = evidence::claim(claim_id).expect("known claim");
    Recommended {
        value,
        evidence: e.to_evidence(),
        confidence: e.to_confidence_tag(),
    }
}

/// Latest observed value for a signal, if present (deterministic: max `observed_at`).
fn latest(inputs: &[ReadinessInput], signal: ReadinessSignal) -> Option<f64> {
    inputs
        .iter()
        .filter(|i| i.signal == signal)
        .max_by_key(|i| i.observed_at)
        .map(|i| i.value)
}

// ---------------------------------------------------------------------------
// Safety-condition predicates (File 06 §5 ladder; §6C/§6E rules)
// ---------------------------------------------------------------------------

/// True when any sharp/localized/joint/tendon pain is reported (`value > 0`).
/// autoreg-043: ANY pain → hard stop; highest tier (`SafetyTier::Pain`).
fn pain_stop(inputs: &[ReadinessInput]) -> bool {
    latest(inputs, ReadinessSignal::Pain).is_some_and(|v| v > 0.0)
}

/// True for below-neck symptoms / any fever: absolute no-train.
/// autoreg-046: below-neck OR fever → do NOT train (`SafetyTier::Illness`).
fn illness_stop(inputs: &[ReadinessInput]) -> bool {
    latest(inputs, ReadinessSignal::Illness)
        .is_some_and(|v| IllnessSeverity::from_value(v) == IllnessSeverity::BelowNeckOrFever)
}

/// True for above-neck-only illness: cut intensity ~50%.
/// autoreg-045: above-neck only, no fever → downgrade session.
fn illness_downgrade(inputs: &[ReadinessInput]) -> bool {
    latest(inputs, ReadinessSignal::Illness)
        .is_some_and(|v| IllnessSeverity::from_value(v) == IllnessSeverity::AboveNeck)
}

/// True when morning RHR is ≥ +10 bpm over baseline: rest / neck-check.
/// autoreg-041: RHR > baseline + 10 bpm (safety_critical) → `Adjustment::RestDay`.
fn rhr_stop(inputs: &[ReadinessInput]) -> bool {
    latest(inputs, ReadinessSignal::RestingHr).is_some_and(|v| v >= 10.0)
}

// ---------------------------------------------------------------------------
// Medical-referral deferrals (File 08 §5). Absolute: override every training
// adjustment, including training-pain. `value > 0` encodes "flag present".
// ---------------------------------------------------------------------------

/// RED-S / low-energy-availability red flag (safety-035/049): never a
/// programming variable, reduce stress and defer to a professional.
fn reds_defer(inputs: &[ReadinessInput]) -> bool {
    latest(inputs, ReadinessSignal::RedS).is_some_and(|v| v > 0.0)
}

/// Cardiovascular red-flag symptom (safety-043): stop + defer for clearance.
fn cardiac_defer(inputs: &[ReadinessInput]) -> bool {
    latest(inputs, ReadinessSignal::CardiacRedFlag).is_some_and(|v| v > 0.0)
}

/// Bone-stress-injury signs (safety-040): stop impact + urgent referral.
fn bone_stress_defer(inputs: &[ReadinessInput]) -> bool {
    latest(inputs, ReadinessSignal::BoneStress).is_some_and(|v| v > 0.0)
}

/// The dominant medical-referral deferral, if any red flag is present.
/// Deterministic priority: cardiovascular (acute) > bone stress > RED-S.
/// Returns the `Recommended<Adjustment::Defer>` cited to its safety claim.
fn medical_referral(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    if cardiac_defer(inputs) {
        Some(recommend(
            Adjustment::Defer {
                reason: "Cardiovascular red-flag symptom - stop and seek medical clearance before training.".into(),
            },
            "SAFE-CVD-001",
        ))
    } else if bone_stress_defer(inputs) {
        Some(recommend(
            Adjustment::Defer {
                reason: "Bone stress injury signs - stop impact loading immediately and seek urgent medical evaluation.".into(),
            },
            "SAFE-BSI-001",
        ))
    } else if reds_defer(inputs) {
        Some(recommend(
            Adjustment::Defer {
                reason: "Low-energy-availability / RED-S red flag - reduce training stress and defer to a physician, registered dietitian, or mental-health professional.".into(),
            },
            "SAFE-REDS-001",
        ))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Rule-family helpers (thresholds verbatim from File 06)
// ---------------------------------------------------------------------------

/// RPE-based load cut when first work-set RPE overshoots target.
/// autoreg-001: RPE ≥ target + 2 → −7 to 10% (uses 10% floor of the band).
/// autoreg-002: RPE = target + 1 → −3 to 5% (uses 5%).
/// `value` is the signed RPE delta (actual − target).
fn rpe_load_cut(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let delta = latest(inputs, ReadinessSignal::Rpe)?;
    if delta >= 2.0 {
        Some(recommend(Adjustment::ReduceLoadPct(10.0), "AUTOREG-RIR-001"))
    } else if delta >= 1.0 {
        Some(recommend(Adjustment::ReduceLoadPct(5.0), "AUTOREG-RIR-001"))
    } else {
        None
    }
}

/// e1RM / velocity deload gate.
/// autoreg-022: e1RM at fixed RPE down >10% for ≥2 sessions → deload
/// (volume −40–50%, load −5–10%, 1 wk). `EstimatedOneRm` carries the ratio
/// today ÷ baseline; < 0.90 means a >10% drop.
/// autoreg-006: e1RM < baseline − 5% (ratio < 0.95) → cap/reduce top-set ~5%.
fn e1rm_deload_gate(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let ratio = latest(inputs, ReadinessSignal::EstimatedOneRm)?;
    if ratio < 0.90 {
        Some(recommend(
            Adjustment::Deload {
                volume_reduction_pct: 45.0,
                load_reduction_pct: 7.5,
                weeks: 1,
            },
            "AUTOREG-PCT-001",
        ))
    } else if ratio < 0.95 {
        Some(recommend(Adjustment::ReduceLoadPct(5.0), "AUTOREG-PCT-001"))
    } else {
        None
    }
}

/// Velocity-loss set-termination stop.
/// autoreg-010: within-set VL ≥ goal threshold → terminate set (volume cut).
/// Uses the max-strength sweet-spot floor of 20% (`VelocityLoss` = % drop).
fn vl_stop(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let vl = latest(inputs, ReadinessSignal::VelocityLoss)?;
    if vl >= 20.0 {
        Some(recommend(Adjustment::DowngradeSession, "AUTOREG-VL-001"))
    } else {
        None
    }
}

/// HRV rolling-baseline downgrade.
/// autoreg-028: lnRMSSD 7-day below SWC lower limit → downgrade hard→easy/Z2.
/// `HrvLnRmssd` carries the z-score vs rolling baseline; z < −0.5 clears the SWC
/// lower band (baseline ± 0.5 SD).
fn hrv_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let z = latest(inputs, ReadinessSignal::HrvLnRmssd)?;
    if z < -0.5 {
        Some(recommend(Adjustment::DowngradeSession, "HRV-001"))
    } else {
        None
    }
}

/// Subjective-wellness downgrade.
/// autoreg-030: wellness composite z ≤ −1.5 → downgrade intensity one level.
/// `WellnessZ` carries the individual composite z-score.
fn wellness_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let z = latest(inputs, ReadinessSignal::WellnessZ)?;
    if z <= -1.5 {
        Some(recommend(Adjustment::DowngradeSession, "WELLNESS-001"))
    } else {
        None
    }
}

/// Single-day RHR intensity downgrade (below the stop threshold).
/// autoreg-040: RHR > baseline + 5–7 bpm for ≥2 days → downgrade intensity.
/// Fires on the +5 bpm floor; the +10 stop is handled by [`rhr_stop`].
fn rhr_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let d = latest(inputs, ReadinessSignal::RestingHr)?;
    if (5.0..10.0).contains(&d) {
        Some(recommend(Adjustment::DowngradeSession, "SAFE-OTS-001"))
    } else {
        None
    }
}

/// Aerobic-decoupling downgrade on easy/steady runs.
/// autoreg-037: decoupling > 10% → base insufficient/fatigued → keep easy.
/// `AerobicDecoupling` carries the % efficiency drift.
fn decoupling_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let d = latest(inputs, ReadinessSignal::AerobicDecoupling)?;
    if d > 10.0 {
        Some(recommend(Adjustment::DowngradeSession, "RUN-DECOUPLE-001"))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Highest safety tier triggered by the inputs (File 06 §5 ladder).
///
/// Pain outranks Illness, which outranks all objective/subjective/HRV tiers.
/// Returns `None` when no safety condition fires. autoreg-041/043/046.
pub fn resolve_safety(inputs: &[ReadinessInput]) -> Option<SafetyTier> {
    let mut tier: Option<SafetyTier> = None;
    let mut raise = |t: SafetyTier| {
        tier = Some(match tier {
            Some(cur) if cur >= t => cur,
            _ => t,
        });
    };

    // Tier 0, medical red flags: defer to a professional, overrides all
    // training adjustments including pain (File 08 §5 safety-040/043/049).
    if medical_referral(inputs).is_some() {
        raise(SafetyTier::MedicalReferral);
    }
    // Tier 1, pain overrides everything else (autoreg-043).
    if pain_stop(inputs) {
        raise(SafetyTier::Pain);
    }
    // Tier 2, illness / fever gates regardless of readiness (autoreg-045/046).
    if illness_stop(inputs) || illness_downgrade(inputs) {
        raise(SafetyTier::Illness);
    }
    // Tier 3, objective within-session performance (e1RM / velocity).
    if e1rm_deload_gate(inputs).is_some() || vl_stop(inputs).is_some() {
        raise(SafetyTier::ObjectivePerformance);
    }
    // Tier 4, persistent multi-day subjective suppression (autoreg-030).
    if wellness_downgrade(inputs).is_some() {
        raise(SafetyTier::SubjectiveMultiDay);
    }
    // Tier 5, HRV rolling baseline (autoreg-028).
    if hrv_downgrade(inputs).is_some() {
        raise(SafetyTier::HrvTrend);
    }
    // Tier 6, single-day objective markers (RHR); lowest (autoreg-040/041).
    if rhr_stop(inputs) || rhr_downgrade(inputs).is_some() {
        raise(SafetyTier::SingleDayMarker);
    }

    tier
}

/// Apply the File 06 IF/THEN rules, emitting evidence-cited adjustments.
///
/// When a Stop-level safety condition fires (pain, fever/below-neck illness, or
/// RHR +10 bpm) it dominates: the returned vec contains only the safety stop.
/// Otherwise every matching optimization rule contributes its adjustment.
pub fn adjustments(inputs: &[ReadinessInput]) -> Vec<Recommended<Adjustment>> {
    // Safety override: a Stop/Defer-level condition suppresses all other output.
    // medical referral (File 08) > pain (autoreg-043) > illness/fever
    // (autoreg-046) > RHR +10 (autoreg-041).
    if let Some(defer) = medical_referral(inputs) {
        return vec![defer];
    }
    if pain_stop(inputs) {
        return vec![recommend(Adjustment::Stop, "MYTH-NO-PAIN-JOINT")];
    }
    if illness_stop(inputs) {
        return vec![recommend(Adjustment::Stop, "ILLNESS-NECK-001")];
    }
    if rhr_stop(inputs) {
        return vec![recommend(Adjustment::RestDay, "SAFE-OTS-001")];
    }

    // Non-stop rules accumulate (deterministic order).
    let mut out = Vec::new();
    if let Some(r) = rpe_load_cut(inputs) {
        out.push(r);
    }
    if let Some(r) = e1rm_deload_gate(inputs) {
        out.push(r);
    }
    if let Some(r) = vl_stop(inputs) {
        out.push(r);
    }
    if let Some(r) = hrv_downgrade(inputs) {
        out.push(r);
    }
    if let Some(r) = wellness_downgrade(inputs) {
        out.push(r);
    }
    if illness_downgrade(inputs) {
        // autoreg-045: above-neck only → cut intensity ~50% (downgrade).
        out.push(recommend(Adjustment::DowngradeSession, "ILLNESS-NECK-001"));
    }
    if let Some(r) = rhr_downgrade(inputs) {
        out.push(r);
    }
    if let Some(r) = decoupling_downgrade(inputs) {
        out.push(r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(signal: ReadinessSignal, value: f64) -> ReadinessInput {
        ReadinessInput {
            signal,
            value,
            observed_at: 0,
        }
    }

    #[test]
    fn pain_resolves_to_pain_tier_and_stop() {
        let inputs = vec![input(ReadinessSignal::Pain, 1.0)];
        assert_eq!(resolve_safety(&inputs), Some(SafetyTier::Pain));
        let adj = adjustments(&inputs);
        assert!(adj.iter().any(|r| r.value == Adjustment::Stop));
        assert_eq!(adj.len(), 1, "stop must dominate");
    }

    #[test]
    fn high_rpe_over_target_reduces_load() {
        let inputs = vec![input(ReadinessSignal::Rpe, 2.0)];
        let adj = adjustments(&inputs);
        assert!(adj
            .iter()
            .any(|r| matches!(r.value, Adjustment::ReduceLoadPct(p) if p == 10.0)));
    }

    #[test]
    fn e1rm_drop_triggers_deload() {
        let inputs = vec![input(ReadinessSignal::EstimatedOneRm, 0.85)];
        let adj = adjustments(&inputs);
        assert!(adj
            .iter()
            .any(|r| matches!(r.value, Adjustment::Deload { .. })));
    }

    #[test]
    fn clean_inputs_yield_no_stop() {
        let inputs = vec![
            input(ReadinessSignal::Rpe, 0.0),
            input(ReadinessSignal::EstimatedOneRm, 1.02),
            input(ReadinessSignal::HrvLnRmssd, 0.1),
            input(ReadinessSignal::WellnessZ, 0.0),
        ];
        assert_eq!(resolve_safety(&inputs), None);
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
        assert_eq!(resolve_safety(&inputs), Some(SafetyTier::MedicalReferral));
        let adj = adjustments(&inputs);
        assert_eq!(adj.len(), 1, "defer must dominate even over pain");
        assert!(matches!(adj[0].value, Adjustment::Defer { .. }));
        assert!(adj[0].confidence.safety_critical);
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
            Adjustment::Defer { reason } => assert!(reason.contains("Cardiovascular")),
            other => panic!("expected cardiac defer, got {other:?}"),
        }
        assert_eq!(adj[0].evidence.citation.claim_id.as_deref(), Some("SAFE-CVD-001"));
    }

    #[test]
    fn bone_stress_defers_with_urgent_referral() {
        let inputs = vec![input(ReadinessSignal::BoneStress, 1.0)];
        let adj = adjustments(&inputs);
        assert_eq!(adj.len(), 1);
        match &adj[0].value {
            Adjustment::Defer { reason } => assert!(reason.contains("urgent")),
            other => panic!("expected bone-stress defer, got {other:?}"),
        }
    }

    #[test]
    fn safety_dominates_concurrent_performance_trigger() {
        // Pain + a genuine performance deload trigger fire together.
        let inputs = vec![
            input(ReadinessSignal::Pain, 1.0),
            input(ReadinessSignal::EstimatedOneRm, 0.80),
        ];
        assert_eq!(resolve_safety(&inputs), Some(SafetyTier::Pain));
        let adj = adjustments(&inputs);
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0].value, Adjustment::Stop);
    }
}
