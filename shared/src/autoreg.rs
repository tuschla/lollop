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

/// RPE-based load adjustment from first work-set RPE vs. target.
/// autoreg-001: RPE ≥ target + 2 → −7 to 10% (uses 10% ceiling of the band).
/// autoreg-002: RPE = target + 1 → −3 to 5% (uses 5%).
/// autoreg-004: RPE = target − 1 → +3 to 5% (uses 4% midpoint).
/// autoreg-005: RPE ≤ target − 2 → +5 to 10% (uses 7.5% midpoint).
/// `value` is the signed RPE delta (actual − target).
fn rpe_load_adjust(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let delta = latest(inputs, ReadinessSignal::Rpe)?;
    if delta >= 2.0 {
        Some(recommend(
            Adjustment::ReduceLoadPct(10.0),
            "AUTOREG-RIR-001",
        ))
    } else if delta >= 1.0 {
        Some(recommend(Adjustment::ReduceLoadPct(5.0), "AUTOREG-RIR-001"))
    } else if delta <= -2.0 {
        Some(recommend(
            Adjustment::IncreaseLoadPct(7.5),
            "AUTOREG-RIR-001",
        ))
    } else if delta <= -1.0 {
        Some(recommend(
            Adjustment::IncreaseLoadPct(4.0),
            "AUTOREG-RIR-001",
        ))
    } else {
        None
    }
}

/// e1RM-driven load gate (both directions).
/// autoreg-022: e1RM at fixed RPE down >10% for ≥2 sessions → deload
/// (volume −40–50%, load −5–10%, 1 wk). `EstimatedOneRm` carries the ratio
/// today ÷ baseline; < 0.90 means a >10% drop.
/// autoreg-006: e1RM < baseline − 5% (ratio < 0.95) → cap/reduce top-set ~5%.
/// autoreg-007: e1RM > baseline + 5% (ratio > 1.05) → add load ~2.5–5% (uses 3.5%).
fn e1rm_gate(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
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
    } else if ratio > 1.05 {
        Some(recommend(
            Adjustment::IncreaseLoadPct(3.5),
            "AUTOREG-PCT-001",
        ))
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
    // Tier 3, objective within-session performance (e1RM drop / velocity loss).
    // A load *increase* signals high readiness, never a safety concern, only
    // reductions/deloads raise the tier.
    let e1rm_concern = matches!(
        e1rm_gate(inputs).map(|r| r.value),
        Some(Adjustment::Deload { .. } | Adjustment::ReduceLoadPct(_))
    );
    if e1rm_concern || vl_stop(inputs).is_some() {
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
        return vec![recommend(Adjustment::Stop, "SAFE-PAIN-001")];
    }
    if illness_stop(inputs) {
        return vec![recommend(Adjustment::Stop, "ILLNESS-NECK-001")];
    }
    if rhr_stop(inputs) {
        return vec![recommend(Adjustment::RestDay, "SAFE-OTS-001")];
    }

    // Non-stop rules accumulate (deterministic order).
    let mut out = Vec::new();
    if let Some(r) = rpe_load_adjust(inputs) {
        out.push(r);
    }
    if let Some(r) = e1rm_gate(inputs) {
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

// ---------------------------------------------------------------------------
// Set-level & next-load prescriptions (scalar inputs, not readiness streams)
//
// These operate on a single lift's within-session observations (AMRAP reps,
// first-set RPE, reference-load velocity) rather than the daily readiness
// vector, so they take plain scalars and return small decision enums. All are
// pure and deterministic. Thresholds verbatim from File 06.
// ---------------------------------------------------------------------------

/// Daily-1RM readiness verdict from mean concentric velocity at a reference
/// load vs. baseline (File 06 autoreg-008/009; VBT). ±0.06 m/s is the
/// reliability band (Weakley/Pearson 2020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VbtReadiness {
    /// MCV > baseline + 0.06 m/s → daily 1RM up → raise working loads.
    IncreaseLoad,
    /// Within the ±0.06 m/s reliability band → hold planned loads.
    Hold,
    /// MCV < baseline − 0.06 m/s → daily 1RM down → reduce working loads.
    ReduceLoad,
}

/// Map a reference-load velocity delta (m/s, today − baseline) to a daily-1RM
/// readiness verdict (File 06 autoreg-008/009).
pub fn vbt_daily_readiness(mcv_delta_m_s: f64) -> Recommended<VbtReadiness> {
    let v = if mcv_delta_m_s > 0.06 {
        VbtReadiness::IncreaseLoad
    } else if mcv_delta_m_s < -0.06 {
        VbtReadiness::ReduceLoad
    } else {
        VbtReadiness::Hold
    };
    recommend_t(v, "AUTOREG-RIR-001")
}

/// Within-session set-volume decision (File 06 autoreg-011/012).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetVolumeAction {
    /// First set strong (≥ target reps) AND RPE ≤ target−1 AND wellness normal
    /// → add a set (progress toward MAV/MRV). autoreg-011.
    AddSet,
    /// First set short OR RPE ≥ target+1 → drop the last planned set. autoreg-012.
    DropLastSet,
    /// Neither trigger, run the planned sets.
    HoldPlanned,
}

/// Decide whether to add/drop a set from the first work set (File 06
/// autoreg-011/012). `rpe_delta` = first-set RPE − target RPE.
pub fn set_volume_action(
    first_set_reps_met: bool,
    rpe_delta: f64,
    wellness_normal: bool,
) -> Recommended<SetVolumeAction> {
    let a = if first_set_reps_met && rpe_delta <= -1.0 && wellness_normal {
        SetVolumeAction::AddSet
    } else if !first_set_reps_met || rpe_delta >= 1.0 {
        SetVolumeAction::DropLastSet
    } else {
        SetVolumeAction::HoldPlanned
    };
    recommend_t(a, "AUTOREG-RIR-001")
}

/// RPE-stop: cut remaining sets once the target RPE is reached before the
/// planned set count (File 06 autoreg-013). `true` = stop now.
pub fn rpe_stop_reached(rpe_actual: f64, rpe_target: f64) -> bool {
    rpe_actual >= rpe_target
}

/// Hold weekly volume (no add) after two consecutive sessions that both needed
/// set cuts on the same lift (File 06 autoreg-014).
pub fn hold_volume_after_two_cut_sessions(cut_last_two_sessions: bool) -> bool {
    cut_last_two_sessions
}

/// Which APRE scheme's adjustment table applies (File 06 autoreg-015…021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApreScheme {
    /// 3RM strength emphasis.
    Apre3,
    /// 6RM strength/hypertrophy.
    Apre6,
    /// 10RM hypertrophy.
    Apre10,
}

/// APRE next-load adjustment (lb) from the AMRAP-set rep count (File 06
/// autoreg-015…021; Mann 2010). Returns the `(low, high)` lb delta to apply to
/// the next set/session; negatives reduce load. `[Moderate]` (AUTOREG-APRE-001).
pub fn apre_load_adjustment_lb(scheme: ApreScheme, reps: u8) -> Recommended<(f64, f64)> {
    let range = match scheme {
        ApreScheme::Apre6 => match reps {
            0..=2 => (-10.0, -5.0),
            3..=4 => (-5.0, 0.0),
            5..=7 => (0.0, 0.0),
            8..=12 => (5.0, 10.0),
            _ => (10.0, 15.0),
        },
        ApreScheme::Apre10 => match reps {
            0..=6 => (-10.0, -5.0),
            7..=8 => (-5.0, 0.0),
            9..=11 => (0.0, 0.0),
            12..=16 => (5.0, 10.0),
            _ => (10.0, 15.0),
        },
        ApreScheme::Apre3 => match reps {
            0..=2 => (-10.0, -5.0),
            3..=4 => (0.0, 0.0),
            5..=6 => (5.0, 10.0),
            _ => (10.0, 15.0),
        },
    };
    recommend_t(range, "AUTOREG-APRE-001")
}

/// The standard RP-framework 1-week deload used by the multi-session triggers
/// (File 06 autoreg-023/024/025/026): volume −50%, load −10%.
fn standard_deload() -> Adjustment {
    Adjustment::Deload {
        volume_reduction_pct: 50.0,
        load_reduction_pct: 10.0,
        weeks: 1,
    }
}

/// Deload when planned RPE is only hit at loads ≥7% below plan for ≥2 sessions
/// (File 06 autoreg-023). `None` when the trigger has not fired.
pub fn deload_from_rpe_load_gap(sessions_ge_7pct_below: u8) -> Option<Recommended<Adjustment>> {
    (sessions_ge_7pct_below >= 2).then(|| recommend(standard_deload(), "AUTOREG-PCT-001"))
}

/// Deload when session RPE creeps +1 across the week at the same loads AND the
/// wellness composite z ≤ −1 for ≥3 days (File 06 autoreg-024).
pub fn deload_from_rpe_creep_and_wellness(
    rpe_creep_plus_one: bool,
    wellness_z_le_neg1_days: u8,
) -> Option<Recommended<Adjustment>> {
    (rpe_creep_plus_one && wellness_z_le_neg1_days >= 3)
        .then(|| recommend(standard_deload(), "AUTOREG-PCT-001"))
}

/// Deload when reference-load velocity is down >0.06 m/s across the week
/// (File 06 autoreg-026). Uses the lower bound of the 0.06–0.10 m/s band.
pub fn deload_from_velocity_drop(weekly_mcv_drop_m_s: f64) -> Option<Recommended<Adjustment>> {
    (weekly_mcv_drop_m_s > 0.06).then(|| recommend(standard_deload(), "AUTOREG-PCT-001"))
}

/// Reduce weekly volume 20–30% (defer hard work) after two failed key sessions
/// in a week (File 06 autoreg-036). Encoded as a load-neutral volume deload
/// (25% midpoint).
pub fn deload_from_failed_sessions(failed_key_sessions: u8) -> Option<Recommended<Adjustment>> {
    (failed_key_sessions >= 2).then(|| {
        recommend(
            Adjustment::Deload {
                volume_reduction_pct: 25.0,
                load_reduction_pct: 0.0,
                weeks: 1,
            },
            "AUTOREG-PCT-001",
        )
    })
}

/// Interval-pace autoregulation (File 06 autoreg-031): when ≥2 reps land at
/// RPE ≥ target+1 or above the HR cap, cut the remaining-rep pace target ~2–4%
/// (returns the fractional pace reduction to apply, else `None`).
pub fn interval_pace_autoreg(reps_over_target: u8) -> Option<Recommended<f64>> {
    (reps_over_target >= 2).then(|| recommend_t(0.03, "RUN-VDOT-001"))
}

/// Easy-day pace is governed by the HR cap, not pace (File 06 autoreg-033):
/// if the runner cannot hold the prescribed easy pace under the HR cap, slow
/// the pace. `true` = slow the easy pace.
pub fn slow_easy_pace_if_over_cap(can_hold_pace_under_cap: bool) -> Recommended<bool> {
    recommend_t(!can_hold_pace_under_cap, "RUN-VDOT-001")
}

/// Which signal source drives autoregulation given data availability
/// (File 06 autoreg-047/048; graceful fallback).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoregSource {
    /// HRV available (today, or ≥4 recent readings) → 7-day rolling HRV gate.
    HrvRolling,
    /// No usable HRV but subjective wellness present → subjective + performance.
    SubjectivePlusPerformance,
    /// Neither HRV nor subjective → performance-only, and HOLD load (no
    /// progression beyond plan). autoreg-048.
    PerformanceOnlyHold,
}

/// Select the autoregulation signal source from availability (File 06
/// autoreg-047/048). HRV is usable when a reading exists today or ≥4 recent
/// readings back a 7-day rolling baseline.
pub fn autoreg_source(
    hrv_today: bool,
    recent_hrv_count: u8,
    has_subjective: bool,
) -> Recommended<AutoregSource> {
    let s = if hrv_today || recent_hrv_count >= 4 {
        AutoregSource::HrvRolling
    } else if has_subjective {
        AutoregSource::SubjectivePlusPerformance
    } else {
        AutoregSource::PerformanceOnlyHold
    };
    recommend_t(s, "HRV-001")
}

/// Whether an HRV reading is reliable (File 06 autoreg-049): reject on high
/// artifacts, a recording shorter than the standardized window, OR a >3 SD
/// deviation from the rolling mean while the CV is already elevated. `true` =
/// usable.
pub fn hrv_reading_reliable(
    high_artifacts: bool,
    window_too_short: bool,
    deviation_sd: f64,
    cv_elevated: bool,
) -> bool {
    !(high_artifacts || window_too_short || (deviation_sd > 3.0 && cv_elevated))
}

/// Suspend HRV gating once ≥2 of the last 3 readings were flagged unreliable
/// (File 06 autoreg-050); use subjective + performance until a clean baseline
/// returns. `true` = suspend HRV gating.
pub fn suspend_hrv_gating(unreliable_in_last_three: u8) -> bool {
    unreliable_in_last_three >= 2
}

/// autoreg-034: a multi-day lnRMSSD suppression streak (≥3–4 consecutive days
/// below the SWC band) warrants inserting a recovery day / easy block, beyond the
/// single-day downgrade in [`hrv_downgrade`]. Fires at ≥3 days. HRV-001.
pub fn hrv_suppressed_recovery_day(consecutive_suppressed_days: u8) -> Recommended<bool> {
    recommend_t(consecutive_suppressed_days >= 3, "HRV-001")
}

/// autoreg-035: multi-day suppressed wellness combined with a rising resting HR
/// trend calls for 1–3 easy days or cross-training. Fires when wellness has been
/// suppressed ≥2 days AND RHR is trending up. SAFE-OTS-001.
pub fn wellness_rhr_multiday_easy(
    wellness_suppressed_days: u8,
    rhr_rising: bool,
) -> Recommended<bool> {
    recommend_t(wellness_suppressed_days >= 2 && rhr_rising, "SAFE-OTS-001")
}

/// Generic `Recommended<T>` constructor for the scalar-input helpers above.
fn recommend_t<T>(value: T, claim_id: &str) -> Recommended<T> {
    let e = evidence::claim(claim_id).expect("known claim");
    Recommended {
        value,
        evidence: e.to_evidence(),
        confidence: e.to_confidence_tag(),
    }
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
    fn above_neck_illness_downgrades_without_blocking_training() {
        // autoreg-045: an above-neck illness cuts intensity but must NOT stop the
        // session. Guards the invariant that the Illness tier is raised (so the
        // shell shows the safety marker) while training stays permitted, i.e. the
        // adjustment is a DowngradeSession, never a Stop/RestDay/Defer.
        let inputs = vec![input(ReadinessSignal::Illness, 1.0)];
        assert_eq!(resolve_safety(&inputs), Some(SafetyTier::Illness));
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
    fn rhr_plus_five_band_downgrades_at_single_day_tier() {
        // autoreg-040: RHR +5..10 bpm downgrades intensity but does not stop -
        // the lowest safety tier (single-day marker). Guards the band boundary so
        // a future edit can't silently promote it to a RestDay stop.
        let inputs = vec![input(ReadinessSignal::RestingHr, 7.0)];
        assert_eq!(resolve_safety(&inputs), Some(SafetyTier::SingleDayMarker));
        let adj = adjustments(&inputs);
        assert!(adj.iter().any(|r| r.value == Adjustment::DowngradeSession));
        assert!(!adj.iter().any(|r| r.value == Adjustment::RestDay));
    }

    #[test]
    fn rhr_plus_ten_forces_rest_day() {
        // autoreg-041: at +10 bpm the downgrade escalates to a full RestDay stop
        // that dominates all other output.
        let inputs = vec![input(ReadinessSignal::RestingHr, 10.0)];
        let adj = adjustments(&inputs);
        assert_eq!(adj.len(), 1, "rest-day stop must dominate");
        assert_eq!(adj[0].value, Adjustment::RestDay);
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
    fn e1rm_drop_triggers_deload() {
        let inputs = vec![input(ReadinessSignal::EstimatedOneRm, 0.85)];
        let adj = adjustments(&inputs);
        assert!(
            adj.iter()
                .any(|r| matches!(r.value, Adjustment::Deload { .. }))
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
        assert_eq!(
            adj[0].evidence.citation.claim_id.as_deref(),
            Some("SAFE-CVD-001")
        );
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
    fn rpe_under_target_increases_load() {
        // RPE two below target → +7.5% load, and no safety tier.
        let inputs = vec![input(ReadinessSignal::Rpe, -2.0)];
        let adj = adjustments(&inputs);
        assert!(
            adj.iter()
                .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(p) if p == 7.5))
        );
        assert_eq!(resolve_safety(&inputs), None);
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
        assert_eq!(resolve_safety(&inputs), None);
    }

    #[test]
    fn e1rm_drop_still_raises_objective_tier() {
        let inputs = vec![input(ReadinessSignal::EstimatedOneRm, 0.85)];
        assert_eq!(
            resolve_safety(&inputs),
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
    fn rpe_stop_and_two_cut_hold() {
        assert!(rpe_stop_reached(9.0, 8.0));
        assert!(!rpe_stop_reached(7.0, 8.0));
        assert!(hold_volume_after_two_cut_sessions(true));
    }

    #[test]
    fn apre_tables_verbatim() {
        // APRE-6 bands.
        assert_eq!(
            apre_load_adjustment_lb(ApreScheme::Apre6, 1).value,
            (-10.0, -5.0)
        );
        assert_eq!(
            apre_load_adjustment_lb(ApreScheme::Apre6, 6).value,
            (0.0, 0.0)
        );
        assert_eq!(
            apre_load_adjustment_lb(ApreScheme::Apre6, 20).value,
            (10.0, 15.0)
        );
        // APRE-10 bands.
        assert_eq!(
            apre_load_adjustment_lb(ApreScheme::Apre10, 5).value,
            (-10.0, -5.0)
        );
        assert_eq!(
            apre_load_adjustment_lb(ApreScheme::Apre10, 14).value,
            (5.0, 10.0)
        );
        // APRE-3 bands.
        assert_eq!(
            apre_load_adjustment_lb(ApreScheme::Apre3, 3).value,
            (0.0, 0.0)
        );
        assert_eq!(
            apre_load_adjustment_lb(ApreScheme::Apre3, 8).value,
            (10.0, 15.0)
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
    fn hrv_availability_fallback() {
        assert_eq!(
            autoreg_source(true, 0, false).value,
            AutoregSource::HrvRolling
        );
        assert_eq!(
            autoreg_source(false, 4, false).value,
            AutoregSource::HrvRolling
        );
        assert_eq!(
            autoreg_source(false, 2, true).value,
            AutoregSource::SubjectivePlusPerformance
        );
        assert_eq!(
            autoreg_source(false, 0, false).value,
            AutoregSource::PerformanceOnlyHold
        );
        // Reliability gate.
        assert!(hrv_reading_reliable(false, false, 1.0, true));
        assert!(!hrv_reading_reliable(true, false, 0.0, false)); // artifacts
        assert!(!hrv_reading_reliable(false, false, 3.5, true)); // >3SD + high CV
        assert!(hrv_reading_reliable(false, false, 3.5, false)); // big dev but CV ok
        // Suspension.
        assert!(suspend_hrv_gating(2));
        assert!(!suspend_hrv_gating(1));
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
        assert_eq!(resolve_safety(&inputs), Some(SafetyTier::Pain));
        let adj = adjustments(&inputs);
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0].value, Adjustment::Stop);
    }
}
