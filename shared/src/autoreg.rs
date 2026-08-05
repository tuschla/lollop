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
    Adjustment, Goal, IllnessSeverity, PainDetail, PainKind, PainTrend, ReadinessInput,
    ReadinessSignal, Recommended, SafetyTier,
};

/// Build a `Recommended<Adjustment>` from a registry claim id (must exist).
fn recommend(value: Adjustment, claim_id: &str) -> Recommended<Adjustment> {
    let e = evidence::claim(claim_id).expect("known claim");
    Recommended::new(value, e.to_evidence(), e.to_confidence_tag())
}

/// Latest observation for a signal, if present (deterministic: max `observed_at`).
fn latest_input(inputs: &[ReadinessInput], signal: ReadinessSignal) -> Option<&ReadinessInput> {
    inputs
        .iter()
        .filter(|i| i.signal == signal)
        .max_by_key(|i| i.observed_at)
}

/// Latest observed value for a signal, if present.
fn latest(inputs: &[ReadinessInput], signal: ReadinessSignal) -> Option<f64> {
    latest_input(inputs, signal).map(|i| i.value)
}

// ---------------------------------------------------------------------------
// Safety-condition predicates (File 06 §5 ladder; §6C/§6E rules)
// ---------------------------------------------------------------------------

/// The graded pain verdict (File 08 Table 4.1; safety-038/039), replacing the
/// old binary any-pain hard stop.
enum PainGate {
    /// Stop-or-defer level: dominates every other output (structural pattern,
    /// uncharacterized pain, or a persistence escalation to DEFER).
    Block(Recommended<Adjustment>),
    /// Non-blocking response: tolerable tendon pain → modify & monitor
    /// ("avoid complete rest"); reactive tendon pain → reduce/downgrade.
    Adjust(Recommended<Adjustment>),
    /// DOMS / normal training discomfort → continue (Table 4.1 row 1).
    Continue,
    /// No pain reported.
    None,
}

/// Tendon reactive band (safety-039): >5/10, worsening after, or rising
/// week-to-week. The tolerable band is ≤3–5/10 AND stable; the operational
/// threshold is the stated ≤5/10.
fn tendon_reactive(d: &PainDetail) -> bool {
    d.severity > 5 || d.trend == PainTrend::Rising
}

/// The reported body-part location, trimmed, if present and non-empty. Never
/// fabricated, `None`/blank stays `None` (display-only context, HARD RULE 1).
fn pain_location(d: &PainDetail) -> Option<&str> {
    d.location
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Human display string for a characterized pain report, the safety-banner
/// sub-line context, e.g. `"Left knee · sharp/joint · 6/10"` or, with no
/// location reported, `"tendon · 3/10"`. Location is included only when the
/// user supplied one. Empty for a bare (detail-less) report; the caller renders
/// that generic case itself.
pub fn pain_context(d: &PainDetail) -> String {
    let kind = match d.kind {
        PainKind::SharpJoint => "sharp/joint",
        PainKind::TendonLoadRelated => "tendon",
        PainKind::Doms => "DOMS",
        PainKind::Other => "unspecified",
    };
    let mut parts: Vec<String> = Vec::new();
    if let Some(loc) = pain_location(d) {
        parts.push(loc.to_string());
    }
    parts.push(kind.to_string());
    parts.push(format!("{}/10", d.severity));
    if d.trend == PainTrend::Rising {
        parts.push("worsening".to_string());
    }
    parts.join(" · ")
}

/// Append the reported body-part location to a Defer reason so the headline can
/// name it, e.g. `"… defer to a physician/physiotherapist. (Left knee)"`. The
/// base message is preserved verbatim when no location was reported.
fn with_location(reason: String, d: &PainDetail) -> String {
    match pain_location(d) {
        Some(loc) => format!("{reason} ({loc})"),
        None => reason,
    }
}

/// Resolve a reported pain input against File 08 Table 4.1.
///
/// Backward compatibility: a bare `Pain` report (`value > 0`, no detail) keeps
/// the conservative pre-existing behavior, hard stop (autoreg-043 / File 06
/// §6C: never ramp load into pain), as does `PainKind::Other`.
///
/// Deliberate deferral: Table 4.1's "MODIFY if DOMS severe" clause carries no
/// numeric severity threshold in the KB (the ≤3–5/10 band is tendon-specific),
/// so severe DOMS is NOT auto-modified here (HARD RULE 1, no invented
/// thresholds); the wellness soreness item (autoreg-030, ≥6/7 → downgrade)
/// covers high soreness via `ReadinessSignal::Soreness` instead.
fn pain_gate(inputs: &[ReadinessInput]) -> PainGate {
    let Some(input) = latest_input(inputs, ReadinessSignal::Pain) else {
        return PainGate::None;
    };
    if input.value <= 0.0 {
        return PainGate::None;
    }
    let Some(detail) = &input.pain else {
        // Generic pain report, conservative hard stop (autoreg-043).
        return PainGate::Block(recommend(Adjustment::Stop, "SAFE-PAIN-001"));
    };
    match detail.kind {
        // Possible structural injury (safety-038): STOP; DEFER if it persists.
        PainKind::SharpJoint => {
            if detail.persists {
                PainGate::Block(recommend(
                    Adjustment::Defer {
                        reason: with_location("Persistent sharp/joint-line pain - stop that exercise and defer to a physician/physiotherapist.".into(), detail),
                    },
                    "SAFE-PAIN-STRUCT-001",
                ))
            } else {
                PainGate::Block(recommend(Adjustment::Stop, "SAFE-PAIN-STRUCT-001"))
            }
        }
        // Tendon pain graded per Silbernagel (safety-039).
        PainKind::TendonLoadRelated => {
            if tendon_reactive(detail) {
                if detail.persists {
                    PainGate::Block(recommend(
                        Adjustment::Defer {
                            reason: with_location("Reactive tendon pain persisting despite reduced load - defer to a physician/physiotherapist.".into(), detail),
                        },
                        "SAFE-TENDON-001",
                    ))
                } else {
                    // REDUCE load & compressive positions: an easier session,
                    // not a stop (the KB states no reduction percentage).
                    PainGate::Adjust(recommend(Adjustment::DowngradeSession, "SAFE-TENDON-001"))
                }
            } else {
                // Tolerable band: modify/continue with monitoring; avoid
                // complete rest.
                PainGate::Adjust(recommend(Adjustment::ModifyAndMonitor, "SAFE-TENDON-001"))
            }
        }
        // Normal training discomfort → continue (see deferral note above).
        PainKind::Doms => PainGate::Continue,
        // Uncharacterized → conservative hard stop; DEFER once persistent.
        PainKind::Other => {
            if detail.persists {
                PainGate::Block(recommend(
                    Adjustment::Defer {
                        reason: with_location("Persistent uncharacterized pain - stop and defer to a professional for assessment.".into(), detail),
                    },
                    "SAFE-PAIN-001",
                ))
            } else {
                PainGate::Block(recommend(Adjustment::Stop, "SAFE-PAIN-001"))
            }
        }
    }
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

/// RED-S / low-energy-availability red flag (safety-049; the KB's own
/// "safety-035" cross-refs are a numbering bug: no such block exists): never a
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
/// autoreg-022: e1RM at fixed RPE down >10% for **≥2 consecutive sessions** →
/// deload (volume −40–50%, load −5–10%, 1 wk). `EstimatedOneRm` carries the
/// ratio today ÷ baseline (< 0.90 = >10% drop) and `streak` the consecutive
/// sessions the drop has held; a single-session drop never deloads: it falls
/// through to the autoreg-006 session cap instead.
/// autoreg-006 (both clauses, AUTOREG-E1RM-GATE-001, Strong, Helms 2018):
/// e1RM < baseline − 5% (ratio < 0.95) → reduce top-set load ~5% AND cap the
/// session at planned RPE − 1.
/// autoreg-007: e1RM > baseline + 5% (ratio > 1.05) → add load ~2.5–5% (uses 3.5%).
fn e1rm_gate(inputs: &[ReadinessInput]) -> Vec<Recommended<Adjustment>> {
    let Some(input) = latest_input(inputs, ReadinessSignal::EstimatedOneRm) else {
        return Vec::new();
    };
    let ratio = input.value;
    if ratio < 0.90 && input.streak >= 2 {
        vec![recommend(
            Adjustment::Deload {
                volume_reduction_pct: 45.0,
                load_reduction_pct: 7.5,
                weeks: 1,
            },
            "AUTOREG-PCT-001",
        )]
    } else if ratio < 0.95 {
        // Includes a single-session >10% drop: File 06 §5 conflict table -
        // "performance down >10% → trust performance → reduce load" (one
        // session reduces; only the ≥2-session streak deloads). autoreg-006's
        // second clause caps today's session at planned RPE − 1 alongside the
        // ~5% top-set cut.
        vec![
            recommend(Adjustment::ReduceLoadPct(5.0), "AUTOREG-E1RM-GATE-001"),
            recommend(Adjustment::CapRpe(1.0), "AUTOREG-E1RM-GATE-001"),
        ]
    } else if ratio > 1.05 {
        vec![recommend(
            Adjustment::IncreaseLoadPct(3.5),
            "AUTOREG-PCT-001",
        )]
    } else {
        Vec::new()
    }
}

/// Goal-dependent velocity-loss termination threshold (autoreg-010, verbatim
/// bands: 10% power / 15–20% strength+power / 25–40% hypertrophy). Each goal
/// uses its band's ceiling, the maximum VL the plan would ever prescribe, so
/// exceeding it is beyond-plan fatigue (matches
/// `strength::vl_termination_threshold`). No goal → the 20% strength+power
/// sweet-spot ceiling (the pre-existing conservative default).
fn vl_threshold_pct(goal: Option<&Goal>) -> f64 {
    match goal {
        Some(Goal::Power) => 10.0,
        Some(Goal::Hypertrophy) => 40.0,
        _ => 20.0,
    }
}

/// Velocity-loss set-termination stop.
/// autoreg-010: within-set VL ≥ goal threshold → terminate set (volume cut).
/// `VelocityLoss` carries the % drop; the threshold is goal-dependent.
fn vl_stop(inputs: &[ReadinessInput], goal: Option<&Goal>) -> Option<Recommended<Adjustment>> {
    let vl = latest(inputs, ReadinessSignal::VelocityLoss)?;
    if vl >= vl_threshold_pct(goal) {
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
/// autoreg-030: wellness composite z ≤ −1.5 (single-session flag) → downgrade
/// intensity one level, keep easy volume. Additionally the §5 tier-4 condition
/// (z ≤ −1 for ≥3 days, carried in `streak`) also cuts: "if poor ≥3 days → cut"
/// (conflict table). `WellnessZ` carries the individual composite z-score.
fn wellness_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let input = latest_input(inputs, ReadinessSignal::WellnessZ)?;
    if input.value <= -1.5 || (input.value <= -1.0 && input.streak >= 3) {
        Some(recommend(Adjustment::DowngradeSession, "WELLNESS-001"))
    } else {
        None
    }
}

/// §5 tier-4 condition: wellness composite z ≤ −1 for ≥3 consecutive days.
/// Only this raises `SafetyTier::SubjectiveMultiDay`; a single-day z ≤ −1.5
/// stays an intensity downgrade without raising that tier.
fn wellness_multiday(inputs: &[ReadinessInput]) -> bool {
    latest_input(inputs, ReadinessSignal::WellnessZ)
        .is_some_and(|i| i.value <= -1.0 && i.streak >= 3)
}

/// Wellness soreness-item downgrade (autoreg-030 second clause): a single
/// soreness item ≥6 on the 7-point scale → downgrade intensity one level,
/// keep easy volume. Localized soreness modifies, it does not stop (§5
/// conflict table: "Good HRV BUT high localized soreness → train; swap/modify
/// the sore muscle's high-eccentric work").
fn soreness_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let v = latest(inputs, ReadinessSignal::Soreness)?;
    if v >= 6.0 {
        Some(recommend(Adjustment::DowngradeSession, "WELLNESS-001"))
    } else {
        None
    }
}

/// RHR intensity downgrade (below the stop threshold).
/// autoreg-040: RHR > baseline + 5–7 bpm for **≥2 days** (`streak`) →
/// downgrade intensity; check illness/sleep. Fires on the +5 bpm floor; the
/// +10 stop is handled by [`rhr_stop`]. A single elevated day is a no-op -
/// "single-day = likely noise (caffeine/heat); act on ≥2 days" (File 06
/// signal spec; §5 conflict table). AUTOREG-RHR-DOWN-001 (Weak practitioner
/// convention, not the Strong OTS deferral).
fn rhr_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let input = latest_input(inputs, ReadinessSignal::RestingHr)?;
    if (5.0..10.0).contains(&input.value) && input.streak >= 2 {
        Some(recommend(Adjustment::DowngradeSession, "AUTOREG-RHR-DOWN-001"))
    } else {
        None
    }
}

/// Minimum continuous-effort duration for aerobic decoupling to be a valid
/// signal (File 06 signal spec: "Valid only for efforts >20 min").
pub const DECOUPLING_MIN_EFFORT_MIN: f64 = 20.0;

/// Aerobic-decoupling downgrade on easy/steady runs.
/// autoreg-037: decoupling > 10% → base insufficient/fatigued → keep easy.
/// `AerobicDecoupling` carries the % efficiency drift.
///
/// Validity gate (File 06 signal spec): decoupling is valid only for efforts
/// >20 min, an observation whose `effort_min` is at or under the floor is
/// discarded, never acted on. `effort_min == None` (duration untracked, the
/// wire default) keeps the pre-existing behavior.
fn decoupling_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let input = latest_input(inputs, ReadinessSignal::AerobicDecoupling)?;
    if input
        .effort_min
        .is_some_and(|d| d <= DECOUPLING_MIN_EFFORT_MIN)
    {
        return None;
    }
    if input.value > 10.0 {
        Some(recommend(Adjustment::DowngradeSession, "RUN-DECOUPLE-001"))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Highest safety tier triggered by the inputs (File 06 §5 ladder), without
/// goal context. See [`resolve_safety_for_goal`].
pub fn resolve_safety(inputs: &[ReadinessInput]) -> Option<SafetyTier> {
    resolve_safety_for_goal(inputs, None)
}

/// Highest safety tier triggered by the inputs (File 06 §5 ladder).
///
/// Pain outranks Illness, which outranks all objective/subjective/HRV tiers.
/// Returns `None` when no safety condition fires. autoreg-041/043/046.
/// `goal` selects the goal-dependent velocity-loss threshold (autoreg-010).
pub fn resolve_safety_for_goal(
    inputs: &[ReadinessInput],
    goal: Option<&Goal>,
) -> Option<SafetyTier> {
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
    // Tier 1, pain overrides everything else (autoreg-043; File 08 Table 4.1).
    // Any characterized non-DOMS pain (structural, tendon, other) sits at the
    // Pain tier even when its response is modify/reduce rather than stop; DOMS
    // is normal training discomfort and raises no tier.
    match pain_gate(inputs) {
        PainGate::Block(_) | PainGate::Adjust(_) => raise(SafetyTier::Pain),
        PainGate::Continue | PainGate::None => {}
    }
    // Tier 2, illness / fever gates regardless of readiness (autoreg-045/046).
    // The RHR +10 bpm stop lives here too: it is a red-flag rest/neck-check
    // (autoreg-041, safety-critical; the stop groups RHR +10 with
    // fever), NOT a tier-6 single-day marker, its tier must match its
    // train-blocking behavior.
    if illness_stop(inputs) || illness_downgrade(inputs) || rhr_stop(inputs) {
        raise(SafetyTier::Illness);
    }
    // Tier 3, objective within-session performance (e1RM drop / velocity loss).
    // A load *increase* signals high readiness, never a safety concern, only
    // reductions/deloads raise the tier.
    let e1rm_concern = e1rm_gate(inputs).iter().any(|r| {
        matches!(
            r.value,
            Adjustment::Deload { .. } | Adjustment::ReduceLoadPct(_)
        )
    });
    if e1rm_concern || vl_stop(inputs, goal).is_some() {
        raise(SafetyTier::ObjectivePerformance);
    }
    // Tier 4, persistent multi-day subjective suppression: ≥3 days of
    // wellness z ≤ −1 (§5 tier-4 definition). A single-day z ≤ −1.5 downgrade
    // (autoreg-030) does NOT raise this tier.
    if wellness_multiday(inputs) {
        raise(SafetyTier::SubjectiveMultiDay);
    }
    // Tier 5, HRV rolling baseline (autoreg-028).
    if hrv_downgrade(inputs).is_some() {
        raise(SafetyTier::HrvTrend);
    }
    // Tier 6, corroborated RHR marker (autoreg-040, ≥2 days); lowest. A
    // single elevated day is treated as noise → no tier at all.
    if rhr_downgrade(inputs).is_some() {
        raise(SafetyTier::SingleDayMarker);
    }

    tier
}

/// Apply the File 06 IF/THEN rules without goal context. See
/// [`adjustments_for_goal`].
pub fn adjustments(inputs: &[ReadinessInput]) -> Vec<Recommended<Adjustment>> {
    adjustments_for_goal(inputs, None)
}

/// Apply the File 06 IF/THEN rules without block context. See
/// [`adjustments_with_context`].
pub fn adjustments_for_goal(
    inputs: &[ReadinessInput],
    goal: Option<&Goal>,
) -> Vec<Recommended<Adjustment>> {
    adjustments_with_context(inputs, goal, false)
}

/// Apply the File 06 IF/THEN rules, emitting evidence-cited adjustments.
///
/// When a Stop-level safety condition fires (structural/uncharacterized pain,
/// fever/below-neck illness, or RHR +10 bpm) it dominates: the returned vec
/// contains only the safety stop. Otherwise every matching optimization rule
/// contributes its adjustment, and a final reconciliation pass (autoreg-044)
/// strips every load increase whenever any wellness/HRV/RHR suppression signal
/// fired: the engine never auto-increases load into suppressed recovery.
/// `goal` selects the goal-dependent velocity-loss threshold (autoreg-010).
///
/// `high_load_block` marks the current mesocycle as a high-load/overload block:
/// it arms the autoreg-029 parasympathetic-saturation guard, which also strips
/// auto load-adds when lnRMSSD sits ABOVE the SWC upper band (unusually high
/// HRV under heavy loading is not readiness, hold and weigh with wellness).
pub fn adjustments_with_context(
    inputs: &[ReadinessInput],
    goal: Option<&Goal>,
    high_load_block: bool,
) -> Vec<Recommended<Adjustment>> {
    // Safety override: a Stop/Defer-level condition suppresses all other output.
    // medical referral (File 08) > pain (autoreg-043 / Table 4.1) >
    // illness/fever (autoreg-046) > RHR +10 (autoreg-041).
    if let Some(defer) = medical_referral(inputs) {
        return vec![defer];
    }
    let mut pain_adjust = None;
    match pain_gate(inputs) {
        PainGate::Block(stop) => return vec![stop],
        PainGate::Adjust(r) => pain_adjust = Some(r),
        PainGate::Continue | PainGate::None => {}
    }
    // LOW (label consistency): a non-blocking pain response (PainGate::Adjust)
    // raises `SafetyTier::Pain` (:486-489, Pain outranks Illness), so it must
    // also appear in the output, otherwise the headline would cite illness/RHR
    // while `safety_tier` reads Pain. It leads the list (pain outranks), then
    // the illness/RHR stop; training stays blocked either way.
    if illness_stop(inputs) {
        let mut out = Vec::new();
        out.extend(pain_adjust.take());
        out.push(recommend(Adjustment::Stop, "ILLNESS-NECK-001"));
        return out;
    }
    if rhr_stop(inputs) {
        let mut out = Vec::new();
        out.extend(pain_adjust.take());
        out.push(recommend(Adjustment::RestDay, "AUTOREG-RHR-STOP-001"));
        return out;
    }

    // Non-stop rules accumulate (deterministic order). A non-blocking pain
    // response (tendon modify/reduce, Table 4.1) leads the list.
    let mut out = Vec::new();
    if let Some(r) = pain_adjust {
        out.push(r);
    }
    if let Some(r) = rpe_load_adjust(inputs) {
        out.push(r);
    }
    out.extend(e1rm_gate(inputs));
    if let Some(r) = vl_stop(inputs, goal) {
        out.push(r);
    }
    if let Some(r) = hrv_downgrade(inputs) {
        out.push(r);
    }
    if let Some(r) = wellness_downgrade(inputs) {
        out.push(r);
    }
    if let Some(r) = soreness_downgrade(inputs) {
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

    // Reconciliation pass (autoreg-044, safety-critical guardrail): when
    // performance is down and/or wellness is suppressed, never auto-increase
    // load. Any recovery-suppression signal, wellness composite (even a
    // single-day flag: §5 conflict table "proceed, cap top-end, do NOT add
    // load"), soreness item, HRV below SWC, or corroborated elevated RHR -
    // strips every IncreaseLoadPct from the output.
    // A2 (wrong-safety, HARD RULE 3): a load increase must never survive an
    // active pain or above-neck illness report. A non-blocking pain response
    // (PainGate::Adjust, tolerable tendon pain, Table 4.1) and an above-neck
    // illness downgrade (autoreg-045) are recovery-suppression signals too, so
    // they join the wellness/HRV/soreness/RHR set that strips IncreaseLoadPct.
    let suppressed = hrv_downgrade(inputs).is_some()
        || wellness_downgrade(inputs).is_some()
        || latest(inputs, ReadinessSignal::WellnessZ).is_some_and(|z| z <= -1.0)
        || soreness_downgrade(inputs).is_some()
        || rhr_downgrade(inputs).is_some()
        || matches!(pain_gate(inputs), PainGate::Adjust(_))
        || illness_downgrade(inputs);
    // H2 (safety-adjacent, autoreg-044): the guardrail's own doc says "when
    // performance is DOWN and/or wellness is suppressed, never auto-increase".
    // The `suppressed` set above only covered the wellness/recovery half: the
    // objective-performance half was missing, so a low first-set RPE could add
    // load in the same breath the session was being cut for a velocity/e1RM
    // drop. Any objective within-session decline, a velocity-loss set-stop
    // (autoreg-010), an e1RM gate resolving to a Deload/ReduceLoadPct
    // (autoreg-006/022), or an aerobic-decoupling downgrade (autoreg-037) -
    // now strips IncreaseLoadPct too. (An e1RM *increase*, ratio > 1.05, is not
    // a decline and does not suppress, mirrors the tier-3 test at :501-507.)
    let objective_perf_down = vl_stop(inputs, goal).is_some()
        || decoupling_downgrade(inputs).is_some()
        || e1rm_gate(inputs).iter().any(|r| {
            matches!(
                r.value,
                Adjustment::Deload { .. } | Adjustment::ReduceLoadPct(_)
            )
        });
    // autoreg-029 (parasympathetic saturation): in a high-load block, lnRMSSD
    // ABOVE the SWC upper limit (z > +0.5) also blocks auto load-adds, hold
    // and weigh with wellness (AUTOREG-HRV-SAT-001, Moderate, Plews).
    let saturation_hold = latest(inputs, ReadinessSignal::HrvLnRmssd)
        .is_some_and(|z| hrv_saturation_hold(z, high_load_block).value);
    if suppressed || saturation_hold || objective_perf_down {
        out.retain(|r| !matches!(r.value, Adjustment::IncreaseLoadPct(_)));
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
/// readiness verdict (File 06 autoreg-008/009; AUTOREG-VBT-001, Strong -
/// Banyard 2017; Weakley/Pearson 2020).
pub fn vbt_daily_readiness(mcv_delta_m_s: f64) -> Recommended<VbtReadiness> {
    let v = if mcv_delta_m_s > 0.06 {
        VbtReadiness::IncreaseLoad
    } else if mcv_delta_m_s < -0.06 {
        VbtReadiness::ReduceLoad
    } else {
        VbtReadiness::Hold
    };
    recommend_t(v, "AUTOREG-VBT-001")
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
/// Serde derives: the scheme crosses the JSON FFI as a bare variant name in
/// the `ComputeApre` calculator event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
///
/// D3 (documented): the Mann 2010 tables are stated for a rep window centred on
/// each scheme's target RM. The `0..=N` low arms and the open-ended `_` high arm
/// EXTEND the table's own lowest/highest published rows to cover reps below/above
/// that window (e.g. an APRE-6 AMRAP of 0–2 reps or 13+ reps). This is a
/// conservative saturation of the KB band's endpoints, not an invented new
/// band, so far-out rep counts clamp to the nearest published adjustment rather
/// than extrapolating a fabricated larger jump/cut.
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
    // D3: cite the autoreg-031 interval-pace rule itself, not RUN-VDOT-001 (the
    // VDOT *estimator* claim, unrelated to this within-session pace autoreg).
    (reps_over_target >= 2).then(|| recommend_t(0.03, "AUTOREG-INTERVAL-PACE-001"))
}

/// Easy-day pace is governed by the HR cap, not pace (File 06 autoreg-033):
/// if the runner cannot hold the prescribed easy pace under the HR cap, slow
/// the pace. `true` = slow the easy pace.
pub fn slow_easy_pace_if_over_cap(can_hold_pace_under_cap: bool) -> Recommended<bool> {
    // D3: cite the autoreg-033 easy-day HR-cap rule, not RUN-VDOT-001.
    recommend_t(!can_hold_pace_under_cap, "AUTOREG-EASY-CAP-001")
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
/// autoreg-047/048; AUTOREG-FALLBACK-001, ExpertOpinion, safety-critical -
/// missing data must degrade conservatively, never progress blind). HRV is
/// usable when a reading exists today or ≥4 recent readings back a 7-day
/// rolling baseline.
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
    recommend_t(s, "AUTOREG-FALLBACK-001")
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
/// suppressed ≥2 days AND RHR is trending up. AUTOREG-WELLNESS-RHR-001
/// (Moderate, File 06 §3C, not the Strong OTS deferral).
pub fn wellness_rhr_multiday_easy(
    wellness_suppressed_days: u8,
    rhr_rising: bool,
) -> Recommended<bool> {
    recommend_t(wellness_suppressed_days >= 2 && rhr_rising, "AUTOREG-WELLNESS-RHR-001")
}

/// autoreg-029: parasympathetic-saturation guard. `true` = do NOT auto-add
/// load; hold and weigh with wellness. Fires when the 7-day rolling lnRMSSD
/// z-score sits above the SWC upper limit (baseline + 0.5 SD) *during a
/// high-load block*, unusually high HRV under heavy loading can be
/// saturation, not readiness. AUTOREG-HRV-SAT-001 (Moderate, Plews).
pub fn hrv_saturation_hold(hrv_z: f64, high_load_block: bool) -> Recommended<bool> {
    recommend_t(high_load_block && hrv_z > 0.5, "AUTOREG-HRV-SAT-001")
}

/// autoreg-028 second trigger: a SINGLE-DAY lnRMSSD reading more than 1 SD
/// below baseline combined with a ≥2-day downtrend → downgrade hard→easy/Z2
/// (or rest), even before the 7-day rolling average clears the SWC lower band
/// (the first trigger, [`hrv_downgrade`] via the readiness vector). HRV-001
/// (Moderate, Kiviniemi 2007; Javaloyes 2019).
pub fn hrv_single_day_downgrade(
    single_day_z: f64,
    downtrend_days: u8,
) -> Option<Recommended<Adjustment>> {
    (single_day_z < -1.0 && downtrend_days >= 2)
        .then(|| recommend(Adjustment::DowngradeSession, "HRV-001"))
}

/// autoreg-025: the at/above-MRV sign cluster (joint aches, performance stall,
/// sleep disruption, motivation drop) → deload. The KB defines the cluster
/// qualitatively, NO numeric sign count exists (HARD RULE 1: none invented),
/// so the caller supplies the judgment that the cluster is present. Deload
/// magnitude per the rule's RP-framework mapping: volume −50%, load −10%,
/// 1 week. AUTOREG-MRV-001 (ExpertOpinion, Israetel et al. 2021).
pub fn mrv_signs_deload(sign_cluster_present: bool) -> Option<Recommended<Adjustment>> {
    sign_cluster_present.then(|| recommend(standard_deload(), "AUTOREG-MRV-001"))
}

/// APRE next-load adjustment with the autoreg-019 small-lifter cap applied.
///
/// autoreg-019 (verbatim): reps 13+ on APRE-6 → "+10 to 15 lb (cap as % of
/// load for smaller/weaker lifters, 15 lb on 100 lb = +15%)". The KB states
/// no separate %-cap constant; its own worked example anchors the flat band to
/// a 100 lb load (10–15 lb ≡ 10–15% there), so each positive bound is capped
/// at that same relative size: `min(band_lb, current_load_lb × band_lb/100)`.
/// At 100 lb this is identical to the flat band; below 100 lb the jump shrinks
/// proportionally; above 100 lb the flat band already IS the smaller value and
/// governs unchanged. Reductions (negative bounds) are never capped, capping
/// a load *cut* would weaken a fatigue response. AUTOREG-APRE-001 (Moderate -
/// Mann 2010).
pub fn apre_load_adjustment_capped_lb(
    scheme: ApreScheme,
    reps: u8,
    current_load_lb: f64,
) -> Recommended<(f64, f64)> {
    let (lo, hi) = apre_load_adjustment_lb(scheme, reps).value;
    // B7: a non-positive current load has no meaningful proportional cap -
    // `current_load_lb × band/100` would flip a positive jump negative (a
    // fabricated load *cut*). Reject it: fall back to the uncapped KB flat band.
    if !(current_load_lb > 0.0) {
        return recommend_t((lo, hi), "AUTOREG-APRE-001");
    }
    let cap = |bound_lb: f64| -> f64 {
        if bound_lb > 0.0 {
            bound_lb.min(current_load_lb * bound_lb / 100.0)
        } else {
            bound_lb
        }
    };
    recommend_t((cap(lo), cap(hi)), "AUTOREG-APRE-001")
}

/// autoreg-032: pace at target HR improved by at least a smallest-worthwhile
/// amount, sustained over 2–3 weeks → re-test / raise threshold pace. `true` =
/// schedule the re-test. The SWC magnitude itself is unstated in the KB, so
/// the caller judges "improved ≥ SWC"; the ≥2-week duration bound is the
/// stated number. AUTOREG-PACE-RETEST-001 (Moderate, File 06 §3B).
pub fn threshold_retest_due(improved_ge_swc: bool, weeks_sustained: u8) -> Recommended<bool> {
    recommend_t(
        improved_ge_swc && weeks_sustained >= 2,
        "AUTOREG-PACE-RETEST-001",
    )
}

// ---------------------------------------------------------------------------
// Overtraining continuum (File 08 safety-041/042 Table 4.4; File 06 autoreg-042)
// ---------------------------------------------------------------------------

/// The Meeusen 2013 overtraining continuum (File 08 Table 4.4; SAFE-OTS-001).
/// Diagnosis is by exclusion: there is no reliable biomarker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OvertrainingState {
    /// Transient dip then supercompensation; days–~2 wk; planned/expected.
    FunctionalOverreach,
    /// Stagnation/decline with no improvement above baseline; weeks–months;
    /// persistent fatigue, mood disturbance, ↑ resting HR.
    NonFunctionalOverreach,
    /// Prolonged decrement; months–years; systemic; diagnosis by exclusion.
    OvertrainingSyndrome,
}

/// Engine response per continuum state (File 08 safety-041, verbatim mapping:
/// FOR → REST/deload as planned; NFOR → REDUCE→REST and reassess; OTS → STOP
/// structured training and DEFER). Sequenced responses come back as ordered
/// vecs; the KB attaches no reduction percentages here, so the NFOR "REDUCE"
/// is the number-free session downgrade. SAFE-OTS-001 (Strong, Meeusen 2013
/// ECSS/ACSM consensus, safety-critical).
pub fn overtraining_response(state: OvertrainingState) -> Vec<Recommended<Adjustment>> {
    match state {
        OvertrainingState::FunctionalOverreach => {
            vec![recommend(Adjustment::RestDay, "SAFE-OTS-001")]
        }
        OvertrainingState::NonFunctionalOverreach => vec![
            recommend(Adjustment::DowngradeSession, "SAFE-OTS-001"),
            recommend(Adjustment::RestDay, "SAFE-OTS-001"),
        ],
        OvertrainingState::OvertrainingSyndrome => vec![recommend(
            Adjustment::Defer {
                reason: "Suspected overtraining syndrome - stop structured training and defer to a physician (diagnosis by exclusion; no reliable biomarker)."
                    .into(),
            },
            "SAFE-OTS-001",
        )],
    }
}

/// File 08 safety-042: ≥2 weeks of unexplained performance decline plus
/// elevated fatigue/mood/sleep disturbance DESPITE a deload → REST and DEFER
/// (rule out NFOR/OTS/RED-S/medical cause). Monitoring inputs are trend flags,
/// not diagnostics. SAFE-OTS-001 (Strong, Meeusen 2013, safety-critical).
pub fn unexplained_decline_rest_defer(
    decline_weeks: u8,
    wellness_disturbed: bool,
    despite_deload: bool,
) -> Option<Recommended<Adjustment>> {
    (decline_weeks >= 2 && wellness_disturbed && despite_deload).then(|| {
        recommend(
            Adjustment::Defer {
                reason: "≥2 weeks of unexplained performance decline with fatigue/mood/sleep disturbance despite a deload - rest and defer to a professional to rule out NFOR/OTS/RED-S or a medical cause."
                    .into(),
            },
            "SAFE-OTS-001",
        )
    })
}

/// File 06 autoreg-042 NFOR cluster: unexplained performance decrement ≥2
/// weeks with ≥2 wellness domains suppressed → mandatory recovery block plus a
/// "consult a professional" escalation. Graded ExpertOpinion by File 06 -
/// never inflated to the Strong File 08 rule above, yet safety-critical.
/// AUTOREG-NFOR-001.
pub fn nfor_cluster_defer(
    decrement_weeks: u8,
    suppressed_wellness_domains: u8,
) -> Option<Recommended<Adjustment>> {
    (decrement_weeks >= 2 && suppressed_wellness_domains >= 2).then(|| {
        recommend(
            Adjustment::Defer {
                reason: "Possible non-functional overreaching: ≥2 weeks of unexplained performance decrement with ≥2 wellness domains suppressed - take a mandatory recovery block and consult a professional if it persists."
                    .into(),
            },
            "AUTOREG-NFOR-001",
        )
    })
}

/// Generic `Recommended<T>` constructor for the scalar-input helpers above.
fn recommend_t<T>(value: T, claim_id: &str) -> Recommended<T> {
    let e = evidence::claim(claim_id).expect("known claim");
    Recommended::new(value, e.to_evidence(), e.to_confidence_tag())
}

// ---------------------------------------------------------------------------
// Per-signal readiness states (KB-honest readiness summary; NO composite score)
// ---------------------------------------------------------------------------

/// Which picker/summary group a readiness signal belongs to. `"red_flag"` is
/// the medical-referral / hard-stop block (Pain, Illness, RED-S, cardiac, bone
/// stress) that shells must visually fence off from the routine `"metric"`
/// signals, the fence is data-driven from here, not a shell-side predicate.
pub fn signal_group(signal: ReadinessSignal) -> &'static str {
    match signal {
        ReadinessSignal::Pain
        | ReadinessSignal::Illness
        | ReadinessSignal::RedS
        | ReadinessSignal::CardiacRedFlag
        | ReadinessSignal::BoneStress => "red_flag",
        _ => "metric",
    }
}

/// Every readiness signal in summary order: routine metrics first, then the
/// red-flag block, so a shell can divide exactly where the group changes.
pub const ALL_SIGNALS: [ReadinessSignal; 15] = [
    ReadinessSignal::Rpe,
    ReadinessSignal::EstimatedOneRm,
    ReadinessSignal::BarVelocity,
    ReadinessSignal::VelocityLoss,
    ReadinessSignal::WellnessZ,
    ReadinessSignal::HrvLnRmssd,
    ReadinessSignal::HrvCv,
    ReadinessSignal::AerobicDecoupling,
    ReadinessSignal::RestingHr,
    ReadinessSignal::Soreness,
    ReadinessSignal::Pain,
    ReadinessSignal::Illness,
    ReadinessSignal::RedS,
    ReadinessSignal::CardiacRedFlag,
    ReadinessSignal::BoneStress,
];

/// One readiness signal's latest observation with a qualitative state judged
/// by the SAME File 06/08 thresholds the adjustment rules above use, never a
/// new number, never a composite score (no 0–100 readiness index exists in the
/// KB, so none is emitted; HARD RULE 1).
pub struct SignalState {
    pub signal: ReadinessSignal,
    pub value: f64,
    pub streak: u8,
    /// Qualitative state, e.g. `"suppressed"` / `"elevated +10 bpm - rest"`.
    pub state: String,
    /// Extra display context for the shell, e.g. the pain sub-line
    /// `"Left knee · sharp/joint · 6/10"`. Empty for signals that carry no
    /// characterizing detail. Display-only, never a graded claim.
    pub detail: String,
    /// Registry claim id of the rule whose threshold judged the state. `None`
    /// when the row is a plain factual echo (a recorded-only signal with no
    /// gating rule, or an explicit all-clear), those judge nothing, so they
    /// carry no evidence tag.
    pub claim: Option<&'static str>,
}

/// Latest state of every signal that has at least one observation, in
/// [`ALL_SIGNALS`] order. Pure re-statement of the rule layer: each state
/// string is decided by the same predicate/threshold that drives the matching
/// adjustment, and cites the same claim.
pub fn signal_states(
    inputs: &[ReadinessInput],
    goal: Option<&Goal>,
    high_load_block: bool,
) -> Vec<SignalState> {
    let mut out = Vec::new();
    for &signal in ALL_SIGNALS.iter() {
        let Some(input) = latest_input(inputs, signal) else {
            continue;
        };
        let v = input.value;
        let streak = input.streak;
        let (state, claim): (String, Option<&'static str>) = match signal {
            // autoreg-001/002/004/005: signed RPE delta vs target.
            ReadinessSignal::Rpe => {
                let s = if v >= 2.0 {
                    "well above target"
                } else if v >= 1.0 {
                    "above target"
                } else if v <= -2.0 {
                    "well below target"
                } else if v <= -1.0 {
                    "below target"
                } else {
                    "on target"
                };
                (s.into(), Some("AUTOREG-RIR-001"))
            }
            // autoreg-022/006/007: e1RM ratio vs baseline.
            ReadinessSignal::EstimatedOneRm => {
                if v < 0.90 && streak >= 2 {
                    ("down >10% for 2+ sessions".into(), Some("AUTOREG-PCT-001"))
                } else if v < 0.95 {
                    ("down".into(), Some("AUTOREG-E1RM-GATE-001"))
                } else if v > 1.05 {
                    ("up".into(), Some("AUTOREG-PCT-001"))
                } else {
                    ("stable".into(), Some("AUTOREG-E1RM-GATE-001"))
                }
            }
            // Recorded only: no autoregulation gate exists in the KB yet.
            ReadinessSignal::BarVelocity | ReadinessSignal::HrvCv => ("recorded".into(), None),
            // autoreg-010: goal-dependent velocity-loss termination band.
            ReadinessSignal::VelocityLoss => {
                if v >= vl_threshold_pct(goal) {
                    ("over threshold".into(), Some("AUTOREG-VL-001"))
                } else {
                    ("within threshold".into(), Some("AUTOREG-VL-001"))
                }
            }
            // autoreg-030 + §5 tier 4.
            ReadinessSignal::WellnessZ => {
                if v <= -1.5 || (v <= -1.0 && streak >= 3) {
                    ("suppressed".into(), Some("WELLNESS-001"))
                } else if v <= -1.0 {
                    ("low (single day)".into(), Some("WELLNESS-001"))
                } else {
                    ("normal".into(), Some("WELLNESS-001"))
                }
            }
            // autoreg-028 SWC band; autoreg-029 saturation in a high-load block.
            ReadinessSignal::HrvLnRmssd => {
                if v < -0.5 {
                    ("suppressed".into(), Some("HRV-001"))
                } else if v > 0.5 && high_load_block {
                    (
                        "above band - hold load adds".into(),
                        Some("AUTOREG-HRV-SAT-001"),
                    )
                } else if v > 0.5 {
                    ("above band".into(), Some("HRV-001"))
                } else {
                    ("in band".into(), Some("HRV-001"))
                }
            }
            // autoreg-037, valid only for efforts >20 min (File 06 signal spec).
            ReadinessSignal::AerobicDecoupling => {
                if input
                    .effort_min
                    .is_some_and(|d| d <= DECOUPLING_MIN_EFFORT_MIN)
                {
                    ("not valid (effort ≤20 min)".into(), Some("RUN-DECOUPLE-001"))
                } else if v > 10.0 {
                    ("high".into(), Some("RUN-DECOUPLE-001"))
                } else {
                    ("normal".into(), Some("RUN-DECOUPLE-001"))
                }
            }
            // autoreg-041 stop / autoreg-040 two-day downgrade.
            ReadinessSignal::RestingHr => {
                if v >= 10.0 {
                    ("elevated +10 bpm - rest".into(), Some("AUTOREG-RHR-STOP-001"))
                } else if (5.0..10.0).contains(&v) && streak >= 2 {
                    ("elevated 2+ days".into(), Some("AUTOREG-RHR-DOWN-001"))
                } else if (5.0..10.0).contains(&v) {
                    (
                        "elevated (single day - likely noise)".into(),
                        Some("AUTOREG-RHR-DOWN-001"),
                    )
                } else {
                    ("normal".into(), Some("AUTOREG-RHR-DOWN-001"))
                }
            }
            // autoreg-030 second clause: soreness item ≥6/7.
            ReadinessSignal::Soreness => {
                if v >= 6.0 {
                    ("high".into(), Some("WELLNESS-001"))
                } else {
                    ("normal".into(), Some("WELLNESS-001"))
                }
            }
            // File 08 Table 4.1 graded pain model (mirrors pain_gate).
            ReadinessSignal::Pain => {
                if v <= 0.0 {
                    ("clear".into(), None)
                } else {
                    match &input.pain {
                        None => ("red flag - stop".into(), Some("SAFE-PAIN-001")),
                        Some(d) => match d.kind {
                            PainKind::SharpJoint => {
                                if d.persists {
                                    (
                                        "red flag - defer to a professional".into(),
                                        Some("SAFE-PAIN-STRUCT-001"),
                                    )
                                } else {
                                    ("red flag - stop".into(), Some("SAFE-PAIN-STRUCT-001"))
                                }
                            }
                            PainKind::TendonLoadRelated => {
                                if tendon_reactive(d) {
                                    if d.persists {
                                        (
                                            "reactive, persisting - defer".into(),
                                            Some("SAFE-TENDON-001"),
                                        )
                                    } else {
                                        ("reactive - reduce load".into(), Some("SAFE-TENDON-001"))
                                    }
                                } else {
                                    (
                                        "tolerable - modify & monitor".into(),
                                        Some("SAFE-TENDON-001"),
                                    )
                                }
                            }
                            PainKind::Doms => {
                                ("DOMS - normal training discomfort".into(), None)
                            }
                            PainKind::Other => {
                                if d.persists {
                                    (
                                        "red flag - defer to a professional".into(),
                                        Some("SAFE-PAIN-001"),
                                    )
                                } else {
                                    ("red flag - stop".into(), Some("SAFE-PAIN-001"))
                                }
                            }
                        },
                    }
                }
            }
            // autoreg-045/046 neck check.
            ReadinessSignal::Illness => match IllnessSeverity::from_value(v) {
                IllnessSeverity::BelowNeckOrFever => (
                    "below-neck / fever - do not train".into(),
                    Some("ILLNESS-NECK-001"),
                ),
                IllnessSeverity::AboveNeck => {
                    ("above-neck - downgrade".into(), Some("ILLNESS-NECK-001"))
                }
                IllnessSeverity::None => ("clear".into(), None),
            },
            // File 08 medical-referral red flags (safety-049/043/040).
            ReadinessSignal::RedS => {
                if v > 0.0 {
                    ("red flag - defer to a professional".into(), Some("SAFE-REDS-001"))
                } else {
                    ("clear".into(), None)
                }
            }
            ReadinessSignal::CardiacRedFlag => {
                if v > 0.0 {
                    ("red flag - seek medical clearance".into(), Some("SAFE-CVD-001"))
                } else {
                    ("clear".into(), None)
                }
            }
            ReadinessSignal::BoneStress => {
                if v > 0.0 {
                    (
                        "red flag - stop impact, urgent referral".into(),
                        Some("SAFE-BSI-001"),
                    )
                } else {
                    ("clear".into(), None)
                }
            }
        };
        // Display-only sub-line context. Only a characterized pain report
        // carries one today (e.g. "Left knee · sharp/joint · 6/10"); every
        // other signal leaves it empty.
        let detail = match (signal, &input.pain) {
            (ReadinessSignal::Pain, Some(d)) if v > 0.0 => pain_context(d),
            _ => String::new(),
        };
        out.push(SignalState {
            signal,
            value: v,
            streak,
            state,
            detail,
            claim,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Check-in → derived readiness signals (Phase 2: humanized readiness, B1)
// ---------------------------------------------------------------------------
//
// The user reports raw human observations (sleep/soreness/mood 1–5, optional
// resting HR bpm, optional HRV rMSSD ms). The CORE, not the user, computes the
// rolling baselines, z-scores, deltas and streaks the autoregulation rules
// above already consume. No threshold changes: derivation only *feeds* the
// existing rules synthetic [`ReadinessInput`]s, so every emitted judgement still
// travels with the rule's own registry evidence via the normal path.
//
// Pure + deterministic: time enters only through each check-in's `observed_at`
// (day-bucketed); a bad stamp is skipped, never a panic (BUGS.md HIGH #2 gate).

use crate::schema::CheckinInput;

// Day-bucketing is by LOCAL calendar day: `(observed_at + utc_offset_sec)`
// before `.div_euclid(DERIVE_DAY_SEC)`, threaded from the app.rs call site
// (`derive_readiness(&model.checkins, model.today_utc_offset_sec)`) exactly like
// `session_logged` / `build_run_anchors` (the H1 fix pattern). This stops a
// late-evening + next-morning LOCAL pair (e.g. 23:30 then 07:00 Berlin) from
// collapsing into one UTC bucket and silently breaking a multi-day streak.
// Offset 0 is byte-identical to the former UTC bucketing.
//
// Rule-4 compaction safety (log.rs, do NOT change there): `compact_event_log`
// drops a `SubmitCheckin` only when its raw `observed_at < anchor − 45 days`
// (`RETAIN_CHECKIN_DAYS`), where `anchor` is the MIN over present channels of
// that channel's newest raw `observed_at`. That cutoff is computed on RAW UTC
// seconds and is entirely independent of `utc_offset_sec`, so this change does
// not move it. Derivation reads a day only when `newest_day − day ≤ 30`
// (`BASELINE_WINDOW_DAYS`); applying the SAME offset to both `newest_day` and
// `day` cancels in their difference except for a floor-rounding boundary, so the
// offset can widen the window's RAW-second reach by at most one day (≈31→≈32
// days back from the newest reading). 45 − 32 = 13 days of slack remain over the
// derivation reach, so a line old enough to have been compacted away (>45 days)
// can never re-enter the 30-day derivation window: the 15-day retention margin
// absorbs the ±1-day bucket shift with room to spare.
const DERIVE_DAY_SEC: i64 = 86_400;

/// Minimum number of distinct check-in days before a rolling baseline is
/// trustworthy enough to emit a z-score / delta. Below this the core reports an
/// honest "collecting baseline" state instead of a fabricated number (HARD RULE
/// 1). Grounded in the KB's autoreg-028 7-day rolling HRV window and the
/// wellness SWC band (baseline ± 0.5 SD), both need ~a week of readings.
pub const MIN_BASELINE_CHECKINS: usize = 7;

/// Trailing window (days) the rolling baseline spans, measured back from the
/// newest check-in (deterministic; no clock). Bounds how far a baseline reaches
/// without dropping any log line, so replay stays exact.
const BASELINE_WINDOW_DAYS: i64 = 30;

/// Absolute floor for the wellness composite (1–5 goodness scale, 5 = best): a
/// morning at/below this, e.g. sleep 1 / soreness 5 / mood 1 → composite 1.0;
/// is catastrophic on its face and downgrades intensity regardless of the
/// z-score. It closes the flat-baseline blind spot (M10): 7 identical days give
/// SD ≈ 0 → z 0 "normal", so a z-only rule can never see the dip. It is ALSO the
/// compensating path for a structural gap, a check-in's soreness is a 1–5 item
/// that feeds only this composite, so it can never reach the `Soreness ≥ 6`
/// (7-point) gate in [`soreness_downgrade`]; the floor is how a maxed-out sore /
/// poor morning still cuts intensity from the check-in path.
///
/// Heuristic parameter (no KB entry sets an absolute composite floor): the
/// emitted signal is an ordinary `WellnessZ` input, so the downgrade still
/// travels with the wellness rule's own `WELLNESS-001` evidence, no invented
/// citation. The constant itself is expert-opinion, following the
/// AUTOREG-EASY-CAP precedent for a KB-silent number. Conservative direction
/// (HARD RULE 3): it can only ADD a downgrade, never remove one or add load.
const WELLNESS_ABS_FLOOR: f64 = 2.0;

/// Normalized value forced onto a catastrophic-floor reading: the single-day
/// wellness downgrade trigger (`WellnessZ ≤ −1.5`, autoreg-030), so a floored
/// morning drives the SAME downgrade a genuine z ≤ −1.5 would, no new rule.
const WELLNESS_FLOOR_Z: f64 = -1.5;

/// How the core normalizes a channel's raw daily value against its baseline.
#[derive(Clone, Copy)]
enum Normalize {
    /// z-score = (today − baseline mean) / baseline SD (wellness composite, HRV).
    Z,
    /// signed delta = today − baseline mean (resting-HR bpm over baseline).
    DeltaFromMean,
}

/// One channel that has some check-in data but not yet a trustworthy baseline -
/// surfaced honestly ("collecting your baseline") rather than as a fabricated z.
pub struct BaselineStatus {
    pub signal: ReadinessSignal,
    pub have: usize,
    pub need: usize,
}

/// Result of [`derive_readiness`]: synthetic `ReadinessInput`s to feed the
/// existing rules, plus the honest still-collecting status of any channel that
/// has readings but hasn't reached [`MIN_BASELINE_CHECKINS`].
pub struct DerivedReadiness {
    pub inputs: Vec<ReadinessInput>,
    pub collecting: Vec<BaselineStatus>,
}

/// Clamp a 1–5 human scale item to its range as f64.
fn scale5(v: u8) -> f64 {
    (v as f64).clamp(1.0, 5.0)
}

/// Wellness "goodness" for one check-in on a 1–5 → 1–5 scale where 5 = best,
/// direction-normalized (soreness is reverse-scored so higher soreness lowers
/// the composite). `None` when no wellness item was answered.
fn wellness_goodness(c: &CheckinInput) -> Option<f64> {
    let mut sum = 0.0;
    let mut n = 0.0;
    if let Some(s) = c.sleep_quality {
        sum += scale5(s); // higher = better
        n += 1.0;
    }
    if let Some(s) = c.soreness {
        sum += 6.0 - scale5(s); // higher soreness = worse → reverse-score
        n += 1.0;
    }
    if let Some(m) = c.mood {
        sum += scale5(m); // higher = better
        n += 1.0;
    }
    if n == 0.0 { None } else { Some(sum / n) }
}

/// Latest raw value per LOCAL epoch-day for one channel, sorted ascending by day
/// and bounded to the trailing [`BASELINE_WINDOW_DAYS`] of the newest reading.
/// Each tuple is `(day, raw_value, observed_at)`. `utc_offset_sec` shifts each
/// stamp to the device's local day before bucketing (see [`DERIVE_DAY_SEC`]).
/// Readings with `observed_at <= 0` are skipped (they can't be day-bucketed; the
/// skip is on the RAW stamp, matching log.rs Rule 4); the most recent check-in
/// per local day wins so re-doing a check-in the same day overwrites, never
/// double-counts.
fn per_day_series(
    checkins: &[CheckinInput],
    utc_offset_sec: i64,
    extract: impl Fn(&CheckinInput) -> Option<f64>,
) -> Vec<(i64, f64, i64)> {
    use std::collections::BTreeMap;
    let mut by_day: BTreeMap<i64, (i64, f64)> = BTreeMap::new();
    for c in checkins {
        if c.observed_at <= 0 {
            continue;
        }
        let Some(v) = extract(c) else {
            continue;
        };
        let day = c
            .observed_at
            .saturating_add(utc_offset_sec)
            .div_euclid(DERIVE_DAY_SEC);
        let slot = by_day.entry(day).or_insert((i64::MIN, v));
        if c.observed_at >= slot.0 {
            *slot = (c.observed_at, v);
        }
    }
    let Some(&newest) = by_day.keys().max() else {
        return Vec::new();
    };
    by_day
        .into_iter()
        .filter(|(day, _)| newest - day <= BASELINE_WINDOW_DAYS)
        .map(|(day, (at, v))| (day, v, at))
        .collect()
}

/// Derive one channel: below the minimum → a `collecting` status; at/above →
/// a synthetic `ReadinessInput` carrying the normalized today-value + a
/// core-computed multi-day `streak` (consecutive calendar days the rule's own
/// predicate held). `streak_predicate` runs over the *normalized* value.
fn derive_channel(
    series: &[(i64, f64, i64)],
    signal: ReadinessSignal,
    mode: Normalize,
    abs_floor: Option<f64>,
    streak_predicate: impl Fn(f64) -> bool,
    inputs: &mut Vec<ReadinessInput>,
    collecting: &mut Vec<BaselineStatus>,
) {
    if series.is_empty() {
        return;
    }
    if series.len() < MIN_BASELINE_CHECKINS {
        collecting.push(BaselineStatus {
            signal,
            have: series.len(),
            need: MIN_BASELINE_CHECKINS,
        });
        return;
    }
    let n = series.len();

    // Leave-one-out baseline stats (LOW self-inclusion fix): mean (and, for Z,
    // SD) over every reading EXCEPT the day being scored. Historical dip days
    // were previously normalized against a baseline that included themselves,
    // diluting the dip and making the multi-day tier-4 streak marginally harder
    // to reach; scoring each day against the rest of the window removes that
    // self-reference. Today (the last reading) still excludes exactly itself, so
    // its emitted value equals the old `series[..len−1]` baseline computation.
    let baseline_stats = |skip: usize| -> (f64, f64) {
        let count = (n - 1) as f64;
        let mean = series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != skip)
            .map(|(_, (_, v, _))| *v)
            .sum::<f64>()
            / count;
        let sd = match mode {
            Normalize::Z => {
                let var = series
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != skip)
                    .map(|(_, (_, v, _))| (v - mean).powi(2))
                    .sum::<f64>()
                    / count;
                var.sqrt()
            }
            Normalize::DeltaFromMean => 0.0,
        };
        (mean, sd)
    };

    // Normalize the reading at `idx` against its leave-one-out baseline, then
    // apply the wellness absolute-floor rung (M10).
    let normalize_at = |idx: usize| -> f64 {
        let raw = series[idx].1;
        let (mean, sd) = baseline_stats(idx);
        let base = match mode {
            Normalize::DeltaFromMean => raw - mean,
            // A ~flat baseline can't detect a dip via z; report in-band (z 0)
            // rather than a divide-by-zero or a false trigger (HR3-safe).
            Normalize::Z if sd < 1e-6 => 0.0,
            Normalize::Z => (raw - mean) / sd,
        };
        // M10: a catastrophic raw reading (wellness composite ≤ WELLNESS_ABS_FLOOR)
        // downgrades regardless of z, closing the flat-baseline blind spot and
        // the unreachable 7-point soreness gate. Never relaxes a worse z (min).
        match abs_floor {
            Some(floor) if raw <= floor => base.min(WELLNESS_FLOOR_Z),
            _ => base,
        }
    };

    // Streak: consecutive most-recent *calendar* days whose normalized value
    // satisfies the predicate. A day-gap breaks it (autoreg-040 "≥2 days",
    // autoreg-030 §5 tier-4 "≥3 days" both mean consecutive days).
    let mut streak: u8 = 0;
    let mut expected_day = series[n - 1].0;
    for idx in (0..n).rev() {
        let day = series[idx].0;
        if day != expected_day || !streak_predicate(normalize_at(idx)) {
            break;
        }
        streak = streak.saturating_add(1);
        expected_day -= 1;
    }

    inputs.push(ReadinessInput {
        signal,
        value: normalize_at(n - 1),
        observed_at: series[n - 1].2,
        streak,
        pain: None,
        effort_min: None,
    });
}

/// Normalize a check-in history into the synthetic readiness signals the
/// autoregulation rules consume (Phase 2 / B1). Three channels:
/// - `WellnessZ`: composite z of the answered 1–5 items (autoreg-030).
/// - `HrvLnRmssd`: z of `ln(rMSSD)` vs baseline (autoreg-028).
/// - `RestingHr`: bpm delta vs baseline mean (autoreg-040/041).
///
/// Streaks are core-computed (finally powering the multi-day rules the shell
/// never sent). Below [`MIN_BASELINE_CHECKINS`] a channel emits nothing and
/// reports a `collecting` status instead of a fabricated number.
///
/// `utc_offset_sec` is the device's local UTC offset (the shell's
/// `today_utc_offset_sec`): check-ins bucket by LOCAL calendar day so a
/// late-evening + next-morning pair spans two days and keeps a streak intact
/// (see [`DERIVE_DAY_SEC`]). Offset 0 is byte-identical to the former behaviour.
pub fn derive_readiness(checkins: &[CheckinInput], utc_offset_sec: i64) -> DerivedReadiness {
    let mut inputs = Vec::new();
    let mut collecting = Vec::new();

    // Wellness composite: multi-day streak counts consecutive days z ≤ −1
    // (autoreg-030 second clause / §5 tier 4).
    derive_channel(
        &per_day_series(checkins, utc_offset_sec, wellness_goodness),
        ReadinessSignal::WellnessZ,
        Normalize::Z,
        Some(WELLNESS_ABS_FLOOR),
        |z| z <= -1.0,
        &mut inputs,
        &mut collecting,
    );

    // HRV rolling baseline: single-day gate (autoreg-028), so no streak rule
    // keys off the emitted input, predicate never holds (streak stays 0).
    derive_channel(
        &per_day_series(checkins, utc_offset_sec, |c| {
            c.hrv_rmssd_ms.filter(|v| *v > 0.0).map(|v| v.ln())
        }),
        ReadinessSignal::HrvLnRmssd,
        Normalize::Z,
        None,
        |_| false,
        &mut inputs,
        &mut collecting,
    );

    // Resting-HR delta: streak counts consecutive elevated days (delta ≥ 5),
    // which arms autoreg-040's ≥2-day downgrade (autoreg-041 ≥10 stops on its
    // own regardless of streak).
    derive_channel(
        &per_day_series(checkins, utc_offset_sec, |c| {
            c.resting_hr_bpm.filter(|v| *v > 0.0)
        }),
        ReadinessSignal::RestingHr,
        Normalize::DeltaFromMean,
        None,
        |d| d >= 5.0,
        &mut inputs,
        &mut collecting,
    );

    DerivedReadiness { inputs, collecting }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(signal: ReadinessSignal, value: f64) -> ReadinessInput {
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
                severity,
                trend,
                persists,
                location: location.map(str::to_string),
            }),
            ..input(ReadinessSignal::Pain, 1.0)
        }
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
        let row = signal_states(&with_loc, None, false)
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
        let row = signal_states(&no_loc, None, false)
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
                assert!(reason.contains("Persistent sharp/joint-line pain"));
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
    fn rhr_plus_five_band_needs_two_days_then_downgrades() {
        // autoreg-040: RHR +5–7 bpm must hold ≥2 days before it downgrades -
        // and even then it does not stop (lowest tier). Guards the band boundary
        // so a future edit can't silently promote it to a RestDay stop.
        let inputs = vec![input_streak(ReadinessSignal::RestingHr, 7.0, 2)];
        assert_eq!(resolve_safety(&inputs), Some(SafetyTier::SingleDayMarker));
        let adj = adjustments(&inputs);
        assert!(adj.iter().any(|r| r.value == Adjustment::DowngradeSession));
        assert!(!adj.iter().any(|r| r.value == Adjustment::RestDay));
    }

    #[test]
    fn rhr_single_day_elevation_is_a_no_op() {
        // File 06 signal spec: a single elevated morning RHR is likely noise
        // (caffeine/heat): act on ≥2 days. One day at +7 bpm yields nothing.
        let inputs = vec![input(ReadinessSignal::RestingHr, 7.0)];
        assert_eq!(resolve_safety(&inputs), None);
        assert!(adjustments(&inputs).is_empty());
    }

    #[test]
    fn rhr_plus_ten_forces_rest_day_at_red_flag_tier() {
        // autoreg-041: at +10 bpm the downgrade escalates to a full RestDay stop
        // that dominates all other output, even on a single day (it is a
        // red flag). Its tier is Illness (rest + neck-check), never the
        // tier-6 single-day marker, so tier and stop behavior agree; and it is
        // cited to the KB's own Weak/safety-critical claim, never overstated.
        let inputs = vec![input(ReadinessSignal::RestingHr, 10.0)];
        let adj = adjustments(&inputs);
        assert_eq!(adj.len(), 1, "rest-day stop must dominate");
        assert_eq!(adj[0].value, Adjustment::RestDay);
        assert_eq!(
            adj[0].evidence.citation.claim_id.as_deref(),
            Some("AUTOREG-RHR-STOP-001")
        );
        assert!(adj[0].confidence.safety_critical);
        assert_eq!(resolve_safety(&inputs), Some(SafetyTier::Illness));
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

    // --- autoreg-044: never auto-increase load into suppressed recovery ---

    #[test]
    fn performance_up_with_suppressed_wellness_never_increases_load() {
        // autoreg-044 + §5 conflict table: good performance but poor subjective
        // recovery → proceed, cap top-end, do NOT add load. e1RM +8% would
        // normally emit IncreaseLoadPct; a wellness z of −2.0 strips it.
        let inputs = vec![
            input(ReadinessSignal::EstimatedOneRm, 1.08),
            input(ReadinessSignal::WellnessZ, -2.0),
        ];
        let adj = adjustments(&inputs);
        assert!(
            !adj.iter()
                .any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_))),
            "no load increase may survive a wellness suppression"
        );
        assert!(
            adj.iter().any(|r| r.value == Adjustment::DowngradeSession),
            "the wellness downgrade itself still fires"
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
        // No goal → conservative 20% default (pre-existing behavior).
        assert!(fires(&vl22, None));
        assert!(!fires(&vl12, None));
    }

    // --- §5 tier 4: SubjectiveMultiDay needs ≥3 suppressed days ---

    #[test]
    fn single_day_wellness_flag_downgrades_without_multiday_tier() {
        // autoreg-030: a single-day z ≤ −1.5 downgrades intensity, but the
        // SubjectiveMultiDay tier is defined as ≥3 days of z ≤ −1: a one-day
        // flag must not raise it.
        let inputs = vec![input(ReadinessSignal::WellnessZ, -2.0)];
        let adj = adjustments(&inputs);
        assert!(adj.iter().any(|r| r.value == Adjustment::DowngradeSession));
        assert_ne!(
            resolve_safety(&inputs),
            Some(SafetyTier::SubjectiveMultiDay),
            "one bad day is not multi-day suppression"
        );
    }

    #[test]
    fn three_day_wellness_suppression_raises_multiday_tier() {
        // z ≤ −1 held ≥3 days (streak) → tier 4 + an intensity cut.
        let inputs = vec![input_streak(ReadinessSignal::WellnessZ, -1.1, 3)];
        assert_eq!(
            resolve_safety(&inputs),
            Some(SafetyTier::SubjectiveMultiDay)
        );
        let adj = adjustments(&inputs);
        assert!(adj.iter().any(|r| r.value == Adjustment::DowngradeSession));
    }

    // --- autoreg-030 soreness item ---

    #[test]
    fn soreness_item_at_six_downgrades_intensity() {
        // Soreness ≥6/7 → downgrade intensity one level; below → nothing.
        let sore = vec![input(ReadinessSignal::Soreness, 6.0)];
        let adj = adjustments(&sore);
        assert!(adj.iter().any(|r| r.value == Adjustment::DowngradeSession));
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
        assert_eq!(
            adj[0].evidence.citation.claim_id.as_deref(),
            Some("SAFE-PAIN-001")
        );
        assert_eq!(resolve_safety(&inputs), Some(SafetyTier::Pain));
    }

    #[test]
    fn sharp_joint_pain_stops_and_defers_when_persistent() {
        // Table 4.1 structural row (safety-038): STOP; DEFER if it persists.
        let acute = vec![pain_input(PainKind::SharpJoint, 4, PainTrend::Stable, false)];
        let adj = adjustments(&acute);
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0].value, Adjustment::Stop);
        assert_eq!(
            adj[0].evidence.citation.claim_id.as_deref(),
            Some("SAFE-PAIN-STRUCT-001")
        );
        assert!(adj[0].confidence.safety_critical);
        assert_eq!(resolve_safety(&acute), Some(SafetyTier::Pain));

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
        assert!(adj.iter().any(|r| r.value == Adjustment::ModifyAndMonitor));
        assert!(
            !adj.iter().any(|r| matches!(
                r.value,
                Adjustment::Stop | Adjustment::RestDay | Adjustment::Defer { .. }
            )),
            "tolerable tendon pain must not block training (avoid complete rest)"
        );
        assert_eq!(
            adj[0].evidence.citation.claim_id.as_deref(),
            Some("SAFE-TENDON-001")
        );
        assert!(adj[0].confidence.safety_critical);
        // Still surfaces at the Pain tier so the shell shows the safety marker.
        assert_eq!(resolve_safety(&inputs), Some(SafetyTier::Pain));
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
                .any(|r| r.value == Adjustment::ModifyAndMonitor)
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
        assert_eq!(
            adj[0].evidence.citation.claim_id.as_deref(),
            Some("SAFE-TENDON-001")
        );
    }

    #[test]
    fn doms_continues_without_stop_or_tier() {
        // Table 4.1 row 1: normal training discomfort → continue. No
        // adjustment, no Pain tier.
        let inputs = vec![pain_input(PainKind::Doms, 4, PainTrend::Stable, false)];
        assert!(adjustments(&inputs).is_empty());
        assert_eq!(resolve_safety(&inputs), None);
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
        assert_eq!(resolve_safety(&inputs), Some(SafetyTier::MedicalReferral));
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
            .find(|r| matches!(r.value, Adjustment::CapRpe(d) if d == 1.0))
            .expect("RPE cap fires");
        for r in [cut, cap] {
            assert_eq!(
                r.evidence.citation.claim_id.as_deref(),
                Some("AUTOREG-E1RM-GATE-001")
            );
            assert!((r.confidence.score - 0.90).abs() < f32::EPSILON);
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
        assert_eq!(
            hold.evidence.citation.claim_id.as_deref(),
            Some("AUTOREG-HRV-SAT-001")
        );
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
        assert_eq!(
            d.evidence.citation.claim_id.as_deref(),
            Some("AUTOREG-MRV-001")
        );
        // ExpertOpinion: the RP cluster is a heuristic, not trial evidence.
        assert!((d.confidence.score - 0.30).abs() < f32::EPSILON);
    }

    // --- autoreg-019: small-lifter cap on the APRE top band ---

    #[test]
    fn apre_top_band_caps_relative_to_load_for_small_lifters() {
        // At the KB's own 100 lb anchor the cap changes nothing.
        assert_eq!(
            apre_load_adjustment_capped_lb(ApreScheme::Apre6, 14, 100.0).value,
            (10.0, 15.0)
        );
        // A 60 lb working load shrinks the jump proportionally (15 lb would be
        // a +25% leap; capped to the band's relative size: 6–9 lb).
        assert_eq!(
            apre_load_adjustment_capped_lb(ApreScheme::Apre6, 14, 60.0).value,
            (6.0, 9.0)
        );
        // Heavier lifters keep the flat band: the cap only ever shrinks.
        assert_eq!(
            apre_load_adjustment_capped_lb(ApreScheme::Apre6, 14, 300.0).value,
            (10.0, 15.0)
        );
        // Load *reductions* are never capped (weakening a fatigue response).
        assert_eq!(
            apre_load_adjustment_capped_lb(ApreScheme::Apre6, 1, 60.0).value,
            (-10.0, -5.0)
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
            threshold_retest_due(true, 2)
                .evidence
                .citation
                .claim_id
                .as_deref(),
            Some("AUTOREG-PACE-RETEST-001")
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
        // Untracked duration (wire default) keeps the pre-existing behavior.
        let untracked = vec![input(ReadinessSignal::AerobicDecoupling, 14.0)];
        assert!(
            adjustments(&untracked)
                .iter()
                .any(|r| r.value == Adjustment::DowngradeSession)
        );
    }

    // --- File 08 safety-041/042 + File 06 autoreg-042 continuum ---

    #[test]
    fn overtraining_continuum_maps_verbatim_responses() {
        // FOR → rest/deload as planned.
        let f = overtraining_response(OvertrainingState::FunctionalOverreach);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].value, Adjustment::RestDay);
        // NFOR → REDUCE → REST (ordered).
        let n = overtraining_response(OvertrainingState::NonFunctionalOverreach);
        assert_eq!(n[0].value, Adjustment::DowngradeSession);
        assert_eq!(n[1].value, Adjustment::RestDay);
        // OTS → stop structured training + defer (diagnosis by exclusion).
        let o = overtraining_response(OvertrainingState::OvertrainingSyndrome);
        assert_eq!(o.len(), 1);
        match &o[0].value {
            Adjustment::Defer { reason } => assert!(reason.contains("exclusion")),
            other => panic!("expected Defer, got {other:?}"),
        }
        // All three carry the Strong, safety-critical Meeusen claim.
        for r in f.iter().chain(&n).chain(&o) {
            assert_eq!(
                r.evidence.citation.claim_id.as_deref(),
                Some("SAFE-OTS-001")
            );
            assert!(r.confidence.safety_critical);
        }
    }

    #[test]
    fn decline_despite_deload_rests_and_defers() {
        // safety-042: all three conditions required.
        assert!(unexplained_decline_rest_defer(2, true, true).is_some());
        assert!(unexplained_decline_rest_defer(1, true, true).is_none());
        assert!(unexplained_decline_rest_defer(2, false, true).is_none());
        assert!(unexplained_decline_rest_defer(2, true, false).is_none());
        let d = unexplained_decline_rest_defer(3, true, true).unwrap();
        assert!(matches!(d.value, Adjustment::Defer { .. }));
        assert!((d.confidence.score - 0.90).abs() < f32::EPSILON);
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
        assert_eq!(
            d.evidence.citation.claim_id.as_deref(),
            Some("AUTOREG-NFOR-001")
        );
        // File 06 grades the cluster ExpertOpinion: safety-critical anyway.
        assert!((d.confidence.score - 0.30).abs() < f32::EPSILON);
        assert!(d.confidence.safety_critical);
    }

    // --- Phase 2 / B1: check-in → derived readiness signals ---

    const D: i64 = 86_400;

    fn checkin(day: i64, sleep: Option<u8>, sore: Option<u8>, mood: Option<u8>) -> CheckinInput {
        CheckinInput {
            observed_at: day * D + 100,
            sleep_quality: sleep,
            soreness: sore,
            mood,
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
        let out = derive_readiness(&checkins, 0);
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
        // like actual data), then a rough morning (sleep 2 / soreness 4 / mood
        // 3): goodness = (2 + (6−4) + 3)/3 = 2.33, well below the ~4 baseline →
        // a strong negative z computed by the CORE, no user z entry.
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
        let out = derive_readiness(&checkins, 0);
        let w = derived(&out.inputs, ReadinessSignal::WellnessZ).expect("derived wellness z");
        assert_eq!(w.observed_at, 6 * D + 100, "stamped at the latest check-in");
        // The derived z must feed the EXISTING rule unchanged: ≤ −1.5 downgrades.
        assert!(w.value <= -1.5, "rough morning vs baseline z ≤ −1.5, got {}", w.value);
        assert!(
            wellness_downgrade(&out.inputs).is_some(),
            "derived z drives the autoreg-030 downgrade"
        );
    }

    #[test]
    fn hrv_rmssd_becomes_a_baseline_z_that_trips_autoreg_028() {
        // Seven mornings of rMSSD: six around 60 ms, today suppressed to 35 ms.
        // The core takes ln + baseline z; a suppression must clear autoreg-028's
        // −0.5 gate, computed in the core, never entered by the user.
        let vals = [60.0, 62.0, 58.0, 61.0, 59.0, 60.0, 35.0];
        let checkins: Vec<CheckinInput> = vals
            .iter()
            .enumerate()
            .map(|(d, &ms)| CheckinInput {
                observed_at: d as i64 * D + 100,
                hrv_rmssd_ms: Some(ms),
                ..Default::default()
            })
            .collect();
        let out = derive_readiness(&checkins, 0);
        let h = derived(&out.inputs, ReadinessSignal::HrvLnRmssd).expect("derived hrv z");
        assert!(h.value < -0.5, "suppressed HRV z < −0.5, got {}", h.value);
        assert!(hrv_downgrade(&out.inputs).is_some(), "derived HRV z drives autoreg-028");
    }

    #[test]
    fn resting_hr_delta_and_two_day_streak_arm_autoreg_040() {
        // Six baseline mornings ~50 bpm, then two consecutive elevated days
        // (+6 bpm). The core computes the delta AND the ≥2-day streak the shell
        // never sent, arming the autoreg-040 downgrade (5–9 bpm for ≥2 days).
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
        let out = derive_readiness(&checkins, 0);
        let r = derived(&out.inputs, ReadinessSignal::RestingHr).expect("derived rhr delta");
        // The elevated days sit in the baseline too, so the delta is ~+5 bpm -
        // the point is it lands in autoreg-040's 5–9 downgrade band, not ≥10.
        assert!((5.0..10.0).contains(&r.value), "delta in the 5–9 band, got {}", r.value);
        assert!(r.streak >= 2, "two elevated days → streak ≥ 2, got {}", r.streak);
        // The full ladder now resolves the corroborated-RHR downgrade (autoreg-040).
        let states = signal_states(&out.inputs, None, false);
        let rhr = states
            .iter()
            .find(|s| s.signal == ReadinessSignal::RestingHr)
            .unwrap();
        assert_eq!(rhr.state, "elevated 2+ days");
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
        let local = derive_readiness(&checkins, BERLIN);
        let r = derived(&local.inputs, ReadinessSignal::RestingHr).expect("derived rhr delta");
        assert!(r.streak >= 2, "local pair spans two days → streak ≥ 2, got {}", r.streak);
        assert!((5.0..10.0).contains(&r.value), "delta in the 5–9 band, got {}", r.value);
        assert!(
            rhr_downgrade(&local.inputs).is_some(),
            "the preserved 2-day streak arms the autoreg-040 downgrade"
        );

        // Same stamps under UTC bucketing: the pair collapses to one day → the
        // streak breaks (this is exactly the bug the offset threading fixes).
        let utc = derive_readiness(&checkins, 0);
        let r0 = derived(&utc.inputs, ReadinessSignal::RestingHr).expect("derived rhr delta");
        assert!(r0.streak < 2, "UTC collapse loses a day → streak < 2, got {}", r0.streak);
        assert!(
            rhr_downgrade(&utc.inputs).is_none(),
            "collapsed to a single elevated day, autoreg-040 needs ≥2"
        );
    }

    #[test]
    fn derived_signal_never_fires_what_the_same_value_entered_manually_would_not() {
        // Property: a derived WellnessZ and the same z entered manually must
        // produce the identical adjustment set: derivation only supplies inputs.
        let mut checkins: Vec<CheckinInput> = (0..6)
            .map(|d| {
                if d % 2 == 0 {
                    checkin(d, Some(5), Some(2), Some(4))
                } else {
                    checkin(d, Some(4), Some(2), Some(5))
                }
            })
            .collect();
        checkins.push(checkin(6, Some(1), Some(5), Some(1)));
        let out = derive_readiness(&checkins, 0);
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
        let out = derive_readiness(&checkins, 0);
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
        let out = derive_readiness(&checkins, 0);
        assert!(out.inputs.is_empty());
        let w = out
            .collecting
            .iter()
            .find(|b| b.signal == ReadinessSignal::WellnessZ)
            .unwrap();
        assert_eq!(w.have, 6, "same-day recheck did not inflate the day count");
    }

    // ── A2: a load increase must never survive an active pain/illness report ──
    #[test]
    fn a2_no_load_increase_while_tolerable_pain_active() {
        // Repro from BUGS.md A2: tolerable tendon pain (sev 3, stable) → a
        // ModifyAndMonitor (PainGate::Adjust), plus RPE −2.0 → IncreaseLoadPct(7.5).
        // The reconciliation pass MUST strip the increase while pain is active.
        let inputs = vec![
            pain_input(PainKind::TendonLoadRelated, 3, PainTrend::Stable, false),
            input(ReadinessSignal::Rpe, -2.0),
        ];
        let out = adjustments_with_context(&inputs, None, false);
        assert!(
            out.iter().any(|r| matches!(r.value, Adjustment::ModifyAndMonitor)),
            "tolerable tendon pain should still surface modify-and-monitor"
        );
        assert!(
            !out.iter().any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_))),
            "no IncreaseLoadPct may surface while a pain signal is active"
        );
    }

    #[test]
    fn a2_no_load_increase_while_above_neck_illness() {
        // Above-neck illness (downgrade) + RPE −2.0 (would raise load) → no increase.
        let inputs = vec![
            input(ReadinessSignal::Illness, 1.0),
            input(ReadinessSignal::Rpe, -2.0),
        ];
        let out = adjustments_with_context(&inputs, None, false);
        assert!(
            !out.iter().any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_))),
            "no IncreaseLoadPct may surface while an illness signal is active"
        );
    }

    // ── B7: APRE must reject a non-positive current load (no negative "increase") ──
    #[test]
    fn b7_apre_rejects_nonpositive_current_load() {
        // reps 14 on APRE-6 → the flat +10..15 lb band. A 0 (or negative) current
        // load must NOT flip the positive jump negative via the proportional cap.
        let (lo, hi) = apre_load_adjustment_capped_lb(ApreScheme::Apre6, 14, 0.0).value;
        assert!(lo >= 0.0 && hi >= 0.0, "0 kg load must not yield a negative increase");
        assert_eq!((lo, hi), apre_load_adjustment_lb(ApreScheme::Apre6, 14).value);
        let (lo, hi) = apre_load_adjustment_capped_lb(ApreScheme::Apre6, 14, -50.0).value;
        assert!(lo >= 0.0 && hi >= 0.0, "negative load must not yield a negative increase");
    }

    // ── D3: autoreg-031/033 cite their OWN claims, not RUN-VDOT-001 ──
    #[test]
    fn d3_interval_and_easy_pace_cite_their_own_claims() {
        let iv = interval_pace_autoreg(2).expect("fires at >=2 reps over target");
        assert_eq!(
            iv.evidence.citation.claim_id.as_deref(),
            Some("AUTOREG-INTERVAL-PACE-001")
        );
        let easy = slow_easy_pace_if_over_cap(false);
        assert_eq!(
            easy.evidence.citation.claim_id.as_deref(),
            Some("AUTOREG-EASY-CAP-001")
        );
    }

    // ── H2: objective-performance decline strips a coexisting load increase ──
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
            !out.iter().any(|r| matches!(r.value, Adjustment::IncreaseLoadPct(_))),
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
        assert_eq!(resolve_safety(&inputs), Some(SafetyTier::Pain));
        let out = adjustments(&inputs);
        assert!(
            out.iter().any(|r| matches!(r.value, Adjustment::ModifyAndMonitor)),
            "the pain response that set the Pain tier must be in the output"
        );
        assert!(
            out.iter().any(|r| r.value == Adjustment::Stop),
            "the illness Stop still blocks training"
        );
    }

    // ── M10: catastrophic morning on a FLAT baseline downgrades via the floor ──
    #[test]
    fn flat_baseline_catastrophic_morning_downgrades_via_absolute_floor() {
        // Seven identical days (SD ≈ 0 → z 0), then sleep 1 / soreness 5 / mood 1
        // → composite 1.0. The z-only rule reads "normal"; the absolute floor
        // (composite ≤ 2) forces the single-day downgrade.
        let mut checkins: Vec<CheckinInput> = (0..7)
            .map(|d| checkin(d, Some(4), Some(2), Some(4))) // composite 4.0
            .collect();
        checkins.push(checkin(7, Some(1), Some(5), Some(1))); // composite 1.0
        let out = derive_readiness(&checkins, 0);
        let w = derived(&out.inputs, ReadinessSignal::WellnessZ).expect("derived wellness z");
        assert!(
            w.value <= -1.5,
            "floored catastrophic morning must present as a downgrade z, got {}",
            w.value
        );
        assert!(
            wellness_downgrade(&out.inputs).is_some(),
            "the absolute floor drives the autoreg-030 downgrade despite z ≈ 0"
        );
    }

    #[test]
    fn flat_baseline_normal_morning_stays_normal_despite_floor() {
        // The floor must not fire on an ordinary morning above it.
        let mut checkins: Vec<CheckinInput> = (0..7)
            .map(|d| checkin(d, Some(4), Some(2), Some(4))) // composite 4.0
            .collect();
        checkins.push(checkin(7, Some(4), Some(2), Some(4))); // composite 4.0 > floor
        let out = derive_readiness(&checkins, 0);
        let w = derived(&out.inputs, ReadinessSignal::WellnessZ).unwrap();
        assert_eq!(w.value, 0.0, "normal morning on a flat baseline stays z 0");
        assert!(wellness_downgrade(&out.inputs).is_none());
    }

    // ── LOW (#4): three consecutive dip days reach the multi-day tier. Each dip
    // day is scored against the rest of the window (leave-one-out), not a
    // baseline that includes itself. ──
    #[test]
    fn three_consecutive_dip_days_reach_the_multiday_tier() {
        // mood-only check-ins so composite = mood exactly: four strong days
        // (mood 5) then three consecutive dips (mood 3), today being a dip.
        let mood = |d: i64, m: u8| checkin(d, None, None, Some(m));
        let checkins = vec![
            mood(0, 5),
            mood(1, 5),
            mood(2, 5),
            mood(3, 5),
            mood(4, 3),
            mood(5, 3),
            mood(6, 3),
        ];
        let out = derive_readiness(&checkins, 0);
        let w = derived(&out.inputs, ReadinessSignal::WellnessZ).expect("derived wellness z");
        assert!(
            w.streak >= 3,
            "three consecutive dip days → streak ≥ 3, got {}",
            w.streak
        );
        assert_eq!(
            resolve_safety(&out.inputs),
            Some(SafetyTier::SubjectiveMultiDay),
            "≥3 days of z ≤ −1 raises the multi-day tier"
        );
    }
}
