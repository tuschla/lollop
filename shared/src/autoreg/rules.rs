//! The readiness-stream rules: File 06 IF→THEN rules as amended by File 13
//! ready-upd-001..007, their threshold constants, the §5 safety ladder, and the
//! illness / resting-HR / medical-referral predicates.

use super::inputs::{latest, latest_input};
use super::pain::{PainGate, active_pain_responses, pain_blocks_progression, pain_gate};
use super::perf::{MovementPerf, e1rm_gate, perf_adjustments};
use crate::evidence::claims;
use crate::schema::{
    Adjustment, AdjustmentCall, AdjustmentKind, IllnessSeverity, ReadinessInput, ReadinessSignal,
    Recommended, SafetyTier,
};
use crate::strength::{self, LiftGoal};

/// True for below-neck symptoms / any fever: absolute no-train.
/// autoreg-046: below-neck OR fever → do NOT train (`SafetyTier::Illness`).
pub fn illness_stop(inputs: &[ReadinessInput]) -> bool {
    latest(inputs, ReadinessSignal::Illness)
        .is_some_and(|v| IllnessSeverity::from_value(v) == IllnessSeverity::BelowNeckOrFever)
}

/// True for above-neck-only illness: cut intensity ~50%.
/// autoreg-045: above-neck only, no fever → downgrade session.
pub(super) fn illness_downgrade(inputs: &[ReadinessInput]) -> bool {
    latest(inputs, ReadinessSignal::Illness)
        .is_some_and(|v| IllnessSeverity::from_value(v) == IllnessSeverity::AboveNeck)
}

/// Any active illness report (head cold or worse).
pub fn illness_active(inputs: &[ReadinessInput]) -> bool {
    illness_stop(inputs) || illness_downgrade(inputs)
}

/// Lifting-volume cut while a head cold is active (autoreg-045 "cut
/// intensity/duration ~50%", ILLNESS-NECK-001).
const HEAD_COLD_VOLUME_CUT_PCT: f32 = 50.0;

/// The neck-check response in force while an illness report is active
/// (autoreg-045/046), for today AND every upcoming plan day until the user
/// reports recovery: fever / below-neck → `Stop` (no training); above-neck
/// only → a safety easy session (`DowngradeSession`: no heavy load, every run
/// becomes one short easy run, ILLNESS-COLD-RUN-001 applied plan-side) with
/// lifting volume halved (autoreg-045 "cut intensity/duration ~50%"). Empty
/// when no illness report is active.
pub fn illness_plan_recs(inputs: &[ReadinessInput]) -> Vec<AdjustmentCall> {
    if illness_stop(inputs) {
        vec![AdjustmentKind::IllnessStop.call(Adjustment::Stop, claims::ILLNESS_NECK_001)]
    } else if illness_downgrade(inputs) {
        // The volume cut carries the full head-cold call ("short and easy only,
        // or rest"), so it leads: the headline takes the first of equals.
        vec![
            AdjustmentKind::HeadCold.call(
                Adjustment::ReduceVolumePct(HEAD_COLD_VOLUME_CUT_PCT),
                claims::ILLNESS_NECK_001,
            ),
            AdjustmentKind::HeadCold.call(Adjustment::DowngradeSession, claims::ILLNESS_NECK_001),
        ]
    } else {
        Vec::new()
    }
}

/// The individual resting-HR noise picture the check-in derivation computed
/// for its newest `RestingHr` input (ready-upd-002,
/// AUTOREG-RHR-NOISE-001): resting HR varies ~10 % day to day, so the bands
/// are widened to the person's own spread and a rest day needs persistence.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RhrNoise {
    /// Day-to-day SD of the morning resting HR over the baseline window, bpm.
    pub sd_bpm: f64,
    /// Consecutive recent mornings (gaps of one missed morning tolerated) at or
    /// above the individual stop threshold, including the newest.
    pub stop_level_days: u8,
    /// `observed_at` of the derived input this describes; a newer manual
    /// reading falls back to its own shell-tallied streak.
    pub observed_at: i64,
}

/// `(downgrade, stop)` bpm-over-baseline thresholds for a baseline SD:
/// `max(5, 1 SD)` and `max(10, 2 SD)` (ready-upd-002). A zero (or unknown) SD
/// leaves the fixed KB bands.
pub(super) fn rhr_bands(sd_bpm: f64) -> (f64, f64) {
    (
        RHR_DOWNGRADE_FLOOR_BPM.max(sd_bpm),
        RHR_STOP_BPM.max(2.0 * sd_bpm),
    )
}

/// [`rhr_bands`] for the derived noise in `ctx`; without an SD the fixed KB
/// bands apply.
pub(super) fn rhr_thresholds(ctx: &RuleContext) -> (f64, f64) {
    rhr_bands(ctx.rhr.map_or(0.0, |n| n.sd_bpm))
}

/// The resting-HR verdict (autoreg-040/041 as amended by ready-upd-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RhrVerdict {
    Normal,
    /// ≥ downgrade band for ≥2 days: hold progression.
    Hold,
    /// One stop-level morning with no illness report: likely noise (alcohol,
    /// heat, a bad night) — an easier day plus an illness check, not rest.
    Spike,
    /// Stop level on ≥2 consecutive mornings, or with an illness report: rest.
    Rest,
}

pub(super) fn rhr_verdict(inputs: &[ReadinessInput], ctx: &RuleContext) -> RhrVerdict {
    let Some(i) = latest_input(inputs, ReadinessSignal::RestingHr) else {
        return RhrVerdict::Normal;
    };
    let (down, stop) = rhr_thresholds(ctx);
    if i.value >= stop {
        let days = ctx
            .rhr
            .filter(|n| n.observed_at == i.observed_at)
            .map_or(i.streak, |n| n.stop_level_days);
        if days >= RHR_DOWNGRADE_STREAK || illness_active(inputs) {
            RhrVerdict::Rest
        } else {
            RhrVerdict::Spike
        }
    } else if i.value >= down && i.streak >= RHR_DOWNGRADE_STREAK {
        RhrVerdict::Hold
    } else {
        RhrVerdict::Normal
    }
}

// ---------------------------------------------------------------------------
// Medical-referral deferrals (File 08 §5). Absolute: override every training
// adjustment, including training-pain. `value > 0` encodes "flag present".
// ---------------------------------------------------------------------------

/// RED-S / low-energy-availability red flag (safety-049; the KB's own
/// "safety-035" cross-refs are a numbering bug: no such block exists): never a
/// programming variable, reduce stress and defer to a professional. The one
/// readiness-channel RED-S test: the hold (`medical_referral`) and the app's
/// RED-S-dependent refusals (safety-022 deficit, calculators) both read it over
/// the same date-scoped inputs.
pub fn reds_defer(inputs: &[ReadinessInput]) -> bool {
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

/// Bone-stress-injury referral (safety-040): stop impact, urgent evaluation.
/// One constructor for the readiness signal and the weekly check-in answer, so
/// both channels defer with the same copy.
pub fn bone_stress_referral() -> AdjustmentCall {
    AdjustmentKind::BoneStressReferral.call(
        Adjustment::Defer {
            reason: "Signs of a bone stress injury: training is paused. See a doctor as soon as you can, and don't run or jump until they clear you.".into(),
        },
        claims::SAFE_BSI_001,
    )
}

/// RED-S / low-energy-availability referral (safety-049, whose signal list
/// includes compulsive exercise): reduce stress and defer. Shared by the
/// readiness signal and the weekly screen.
pub fn reds_referral() -> AdjustmentCall {
    AdjustmentKind::RedsReferral.call(
        Adjustment::Defer {
            reason: "Signs of under-eating (RED-S): training is paused. See a doctor, a registered dietitian or a mental-health professional.".into(),
        },
        claims::SAFE_REDS_001,
    )
}

/// The dominant medical-referral deferral, if any red flag is present.
/// Deterministic priority: cardiovascular (acute) > bone stress > RED-S.
/// Returns the deferral cited to its safety claim.
fn medical_referral(inputs: &[ReadinessInput]) -> Option<AdjustmentCall> {
    if cardiac_defer(inputs) {
        Some(
            AdjustmentKind::CardiacReferral.call(
                Adjustment::Defer {
                    reason:
                        "A heart warning sign. Stop and get a doctor's OK before you train again."
                            .into(),
                },
                claims::SAFE_CVD_001,
            ),
        )
    } else if bone_stress_defer(inputs) {
        Some(bone_stress_referral())
    } else if reds_defer(inputs) {
        Some(reds_referral())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Readiness-stream rules (thresholds verbatim from File 06)
// ---------------------------------------------------------------------------

/// RPE delta (actual − target) bands (autoreg-001/002/004/005).
pub(super) const RPE_DELTA_WELL_ABOVE: f64 = 2.0;
pub(super) const RPE_DELTA_ABOVE: f64 = 1.0;
pub(super) const RPE_DELTA_WELL_BELOW: f64 = -2.0;
pub(super) const RPE_DELTA_BELOW: f64 = -1.0;

/// Subjective-wellness composite z-score bands (autoreg-030). A single composite
/// z ≤ [`WELLNESS_SINGLE_DAY_Z`] on ONE observation downgrades intensity one
/// level. WELLNESS-001 (Saw 2016) also carries a multi-day early-warning arm:
/// z ≤ [`WELLNESS_MULTIDAY_Z`] for [`WELLNESS_DOWNGRADE_STREAK`] consecutive
/// days. The §5 tier-4 SubjectiveMultiDay escalation needs the longer
/// [`WELLNESS_MULTIDAY_STREAK`] run.
pub(super) const WELLNESS_SINGLE_DAY_Z: f64 = -1.5;
pub(super) const WELLNESS_MULTIDAY_Z: f64 = -1.0;
/// Consecutive suppressed days that arm the wellness multi-day intensity downgrade.
pub(super) const WELLNESS_DOWNGRADE_STREAK: u8 = 2;
/// Consecutive suppressed days that raise the §5 tier-4 SubjectiveMultiDay tier.
pub(super) const WELLNESS_MULTIDAY_STREAK: u8 = 3;

/// HRV lnRMSSD z-score SWC band, ± around rolling baseline (autoreg-028/029).
pub(super) const HRV_SWC_LOWER_Z: f64 = -0.5;
pub(super) const HRV_SWC_UPPER_Z: f64 = 0.5;

/// Resting-HR elevation bands, bpm above baseline (autoreg-040/041).
pub(super) const RHR_STOP_BPM: f64 = 10.0;
pub(super) const RHR_DOWNGRADE_FLOOR_BPM: f64 = 5.0;
pub(super) const RHR_DOWNGRADE_STREAK: u8 = 2;

/// Soreness-item downgrade floor on the 7-point scale (autoreg-030).
pub(super) const SORENESS_HIGH: f64 = 6.0;

/// The load change one RPE-vs-target delta calls for (autoreg-001/002/004/005):
/// RPE ≥ target + 2 → −10 % (ceiling of the −7–10 % band); target + 1 → −5 %;
/// target − 1 → +4 % (midpoint); ≤ target − 2 → +7.5 % (midpoint).
fn rpe_delta_adjustment(delta: f64) -> Option<Adjustment> {
    if delta >= RPE_DELTA_WELL_ABOVE {
        Some(Adjustment::ReduceLoadPct(10.0))
    } else if delta >= RPE_DELTA_ABOVE {
        Some(Adjustment::ReduceLoadPct(5.0))
    } else if delta <= RPE_DELTA_WELL_BELOW {
        Some(Adjustment::IncreaseLoadPct(7.5))
    } else if delta <= RPE_DELTA_BELOW {
        Some(Adjustment::IncreaseLoadPct(4.0))
    } else {
        None
    }
}

/// RPE-based load adjustment from a raw (advanced-entry) first work-set RPE vs.
/// target signal, session-wide. `value` is the signed RPE delta (actual − target).
pub(super) fn rpe_load_adjust(inputs: &[ReadinessInput]) -> Option<AdjustmentCall> {
    let delta = latest(inputs, ReadinessSignal::Rpe)?;
    rpe_delta_adjustment(delta).map(|a| AdjustmentKind::RpeLoad.call(a, claims::AUTOREG_RIR_001))
}

/// The same autoreg-001/002/004/005 bands applied to ONE plan item from its
/// logged first work set: the change is scoped to that item's
/// remaining sets ("reduce load for remaining sets"), never the whole session.
/// `delta` = logged RPE − the item's target RPE. AUTOREG-RIR-001 (Strong).
pub fn first_set_rpe_adjust(exercise: &str, delta: f64) -> Option<AdjustmentCall> {
    rpe_delta_adjustment(delta).map(|a| {
        AdjustmentKind::FirstSet.call(
            Adjustment::ForItem {
                exercise: exercise.to_string(),
                adjustment: Box::new(a),
            },
            claims::AUTOREG_RIR_001,
        )
    })
}

/// autoreg-013 (Helms 2018, the RPE stop): a later work set of ONE plan item
/// that reached failure (no reps left, or missed reps) before the planned set
/// count → stop or cut that item's remaining sets. AUTOREG-RIR-001.
pub fn failed_set_stop(exercise: &str) -> AdjustmentCall {
    AdjustmentKind::FailedSet.call(
        Adjustment::ForItem {
            exercise: exercise.to_string(),
            adjustment: Box::new(Adjustment::DowngradeSession),
        },
        claims::AUTOREG_RIR_001,
    )
}

/// Velocity-loss termination threshold, as whole-percent. The lifting-goal→VL
/// map lives in `strength::vl_termination_threshold` (a fraction; its doc
/// reconciles autoreg-010 with strength-018). Each goal uses its band ceiling,
/// the maximum VL the plan would ever prescribe, so exceeding it is beyond-plan
/// fatigue. No lifting goal → the 20% max-strength default (the conservative
/// fallback).
pub(super) fn vl_threshold_pct(lift_goal: Option<LiftGoal>) -> f64 {
    strength::vl_termination_threshold(lift_goal.unwrap_or(LiftGoal::MaxStrength)).value * 100.0
}

/// Velocity-loss set-termination stop.
/// autoreg-010: within-set VL ≥ goal threshold → terminate set (volume cut).
/// `VelocityLoss` carries the % drop; the threshold is goal-dependent.
pub(super) fn vl_stop(
    inputs: &[ReadinessInput],
    lift_goal: Option<LiftGoal>,
) -> Option<AdjustmentCall> {
    let vl = latest(inputs, ReadinessSignal::VelocityLoss)?;
    if vl >= vl_threshold_pct(lift_goal) {
        Some(
            AdjustmentKind::VelocityLoss.call(Adjustment::DowngradeSession, claims::AUTOREG_VL_001),
        )
    } else {
        None
    }
}

/// Consecutive suppressed HRV days at which a lift session is lowered, not
/// just held (autoreg-034: "lnRMSSD7d suppressed ≥3–4 days → recovery day /
/// easy block"; ready-upd-001 "several days of feeling run-down").
pub(crate) const HRV_ESCALATE_STREAK: u8 = 3;

/// HRV rolling-baseline verdict (autoreg-028 as scoped by ready-upd-001).
/// `HrvLnRmssd` carries the 7-day mean z vs the rolling baseline; z < −0.5
/// clears the SWC lower band. HRV-guided training is an endurance tool, so a
/// suppressed week HOLDS progression (`CapTopEnd`: runs easy, lifts at the
/// planned load one rep further from failure, AUTOREG-HRV-LIFT-001); only a
/// ≥3-day suppression streak lowers the session (`DowngradeSession`, HRV-001).
pub(super) fn hrv_downgrade(inputs: &[ReadinessInput]) -> Option<AdjustmentCall> {
    let i = latest_input(inputs, ReadinessSignal::HrvLnRmssd)?;
    if i.value >= HRV_SWC_LOWER_Z {
        return None;
    }
    Some(if i.streak >= HRV_ESCALATE_STREAK {
        AdjustmentKind::HrvLower.call(Adjustment::DowngradeSession, claims::HRV_001)
    } else {
        AdjustmentKind::HrvHold.call(
            Adjustment::CapTopEnd {
                trigger: ReadinessSignal::HrvLnRmssd,
            },
            claims::AUTOREG_HRV_LIFT_001,
        )
    })
}

/// autoreg-029: parasympathetic-saturation guard. `true` = do NOT auto-add
/// load; hold and weigh with wellness. Fires when the 7-day rolling lnRMSSD
/// z-score sits above the SWC upper limit (baseline + 0.5 SD) *during a
/// high-load block*, unusually high HRV under heavy loading can be
/// saturation, not readiness. AUTOREG-HRV-SAT-001 (Moderate, Plews).
pub(super) fn hrv_saturation_hold(hrv_z: f64, high_load_block: bool) -> Recommended<bool> {
    Recommended::new(
        high_load_block && hrv_z > HRV_SWC_UPPER_Z,
        claims::AUTOREG_HRV_SAT_001,
    )
}

/// Whether a derived/entered reading is dated on the shell's local today (an
/// undated legacy reading, or no clock, counts as today).
pub(super) fn dated_today(i: &ReadinessInput, ctx: &RuleContext) -> bool {
    ctx.today_epoch_day.is_none_or(|t| {
        crate::day::is_undated(i.observed_at)
            || crate::day::local_day(i.observed_at, ctx.utc_offset_sec) == t
    })
}

/// The subjective-wellness verdict (autoreg-030; §5 conflict table;
/// ready-upd-001 and ready-upd-007). The one predicate the rule
/// ([`wellness_downgrade_in`]) and the readiness label share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WellnessVerdict {
    Normal,
    /// z ≤ −1 without a rule firing (one day, or a deep day not dated today).
    Low,
    /// A single-day z ≤ [`WELLNESS_SINGLE_DAY_Z`] from a check-in dated TODAY
    /// (WELLNESS-Z-001), or z ≤ [`WELLNESS_MULTIDAY_Z`] for
    /// ≥[`WELLNESS_DOWNGRADE_STREAK`] days: hold the top end ("proceed, cap
    /// top-end, do not add load").
    Hold,
    /// The §5 tier-4 run (z ≤ −1 for ≥[`WELLNESS_MULTIDAY_STREAK`] days, "if
    /// poor ≥3 days → cut"): lower the session.
    Lower,
}

pub(super) fn wellness_verdict(input: &ReadinessInput, ctx: &RuleContext) -> WellnessVerdict {
    let v = input.value;
    if wellness_lower(input) {
        WellnessVerdict::Lower
    } else if (v <= WELLNESS_SINGLE_DAY_Z && dated_today(input, ctx))
        || (v <= WELLNESS_MULTIDAY_Z && input.streak >= WELLNESS_DOWNGRADE_STREAK)
    {
        WellnessVerdict::Hold
    } else if v <= WELLNESS_MULTIDAY_Z {
        WellnessVerdict::Low
    } else {
        WellnessVerdict::Normal
    }
}

/// Subjective-wellness adjustment from [`wellness_verdict`]: Hold caps the top
/// end, Lower lowers the session.
pub(super) fn wellness_downgrade_in(
    inputs: &[ReadinessInput],
    ctx: &RuleContext,
) -> Option<AdjustmentCall> {
    let input = latest_input(inputs, ReadinessSignal::WellnessZ)?;
    match wellness_verdict(input, ctx) {
        WellnessVerdict::Lower => Some(
            AdjustmentKind::WellnessLower.call(Adjustment::DowngradeSession, claims::WELLNESS_001),
        ),
        WellnessVerdict::Hold => Some(AdjustmentKind::WellnessHold.call(
            Adjustment::CapTopEnd {
                trigger: ReadinessSignal::WellnessZ,
            },
            claims::AUTOREG_HRV_LIFT_001,
        )),
        WellnessVerdict::Low | WellnessVerdict::Normal => None,
    }
}

/// The verdict's clock-independent Lower arm: z ≤ −1 for
/// ≥[`WELLNESS_MULTIDAY_STREAK`] consecutive days (§5 tier 4).
fn wellness_lower(input: &ReadinessInput) -> bool {
    input.value <= WELLNESS_MULTIDAY_Z && input.streak >= WELLNESS_MULTIDAY_STREAK
}

/// Wellness soreness-item verdict (autoreg-030 second clause): a soreness item
/// ≥6 on the 7-point scale. Localized soreness modifies, it does not stop or
/// lower systemic intensity (§5 conflict table: "high localized soreness →
/// train; modify the sore muscle's eccentric work; keep systemic intensity"),
/// so lifts stop one rep earlier at the planned load and runs are untouched.
pub(super) fn soreness_downgrade(inputs: &[ReadinessInput]) -> Option<AdjustmentCall> {
    let v = latest(inputs, ReadinessSignal::Soreness)?;
    if v >= SORENESS_HIGH {
        Some(AdjustmentKind::Soreness.call(
            Adjustment::CapTopEnd {
                trigger: ReadinessSignal::Soreness,
            },
            claims::WELLNESS_001,
        ))
    } else {
        None
    }
}

/// Resting-HR verdict → adjustment (autoreg-040/041 as amended by
/// ready-upd-002). A corroborated ≥2-day elevation holds the top end
/// (AUTOREG-HRV-LIFT-001); a single stop-level morning with no illness report
/// is an easier day plus an illness check (AUTOREG-RHR-NOISE-001); the rest
/// day itself is emitted by the stop branch of [`adjustments_in`].
pub(super) fn rhr_downgrade_in(
    inputs: &[ReadinessInput],
    ctx: &RuleContext,
) -> Option<AdjustmentCall> {
    match rhr_verdict(inputs, ctx) {
        RhrVerdict::Hold => Some(AdjustmentKind::RhrHold.call(
            Adjustment::CapTopEnd {
                trigger: ReadinessSignal::RestingHr,
            },
            claims::AUTOREG_HRV_LIFT_001,
        )),
        RhrVerdict::Spike => Some(
            AdjustmentKind::RhrSpike
                .call(Adjustment::DowngradeSession, claims::AUTOREG_RHR_NOISE_001),
        ),
        RhrVerdict::Normal | RhrVerdict::Rest => None,
    }
}

/// Whether the single-morning RHR spike asks the user to run the illness
/// neck-check (ready-upd-002): the shell surfaces the check-in illness item.
pub fn rhr_neck_check_due(inputs: &[ReadinessInput], ctx: &RuleContext) -> bool {
    rhr_verdict(inputs, ctx) == RhrVerdict::Spike
}

/// Whether another check-in signal from this morning points the same way as an
/// elevated resting HR (ready-upd-002 "unless corroborated"): a wellness
/// composite at/below the low band, HRV below its band today, or an illness
/// report. Display wording only; the rest-day rule keeps its own predicate.
pub(super) fn rhr_corroborated(inputs: &[ReadinessInput], ctx: &RuleContext) -> bool {
    latest_input(inputs, ReadinessSignal::WellnessZ)
        .is_some_and(|i| dated_today(i, ctx) && wellness_verdict(i, ctx) != WellnessVerdict::Normal)
        || latest_input(inputs, ReadinessSignal::HrvLnRmssd)
            .is_some_and(|i| dated_today(i, ctx) && (hrv_below_swc(i.value) || i.streak >= 1))
        || illness_active(inputs)
}

/// Whether an `HrvLnRmssd` 7-day mean z sits below the SWC lower band
/// (autoreg-028/034): the trend the HRV rules act on.
pub fn hrv_below_swc(z: f64) -> bool {
    z < HRV_SWC_LOWER_Z
}

/// Minimum continuous-effort duration for aerobic decoupling to be a valid
/// signal (File 06 signal spec: "Valid only for efforts >20 min").
pub(super) const DECOUPLING_MIN_EFFORT_MIN: f64 = 20.0;

/// Aerobic-decoupling downgrade on easy/steady runs.
/// autoreg-037: decoupling at/above 10% → base insufficient/fatigued → keep
/// easy. `AerobicDecoupling` carries the % efficiency drift; the band boundary
/// is `load::decoupling_high`, shared with the feedback card and the
/// base-fitness band so all three agree on a drift of exactly 10.0.
///
/// Validity gate (File 06 signal spec): decoupling is valid only for efforts
/// over 20 min. The downgrade fires only when duration is KNOWN valid; an
/// observation whose `effort_min` is at/under the floor OR `None` (duration
/// untracked, the wire default) is discarded, never acted on.
pub(super) fn decoupling_downgrade(inputs: &[ReadinessInput]) -> Option<AdjustmentCall> {
    let input = latest_input(inputs, ReadinessSignal::AerobicDecoupling)?;
    if !input
        .effort_min
        .is_some_and(|d| d > DECOUPLING_MIN_EFFORT_MIN)
    {
        return None;
    }
    if crate::load::decoupling_high(input.value) {
        Some(
            AdjustmentKind::Decoupling.call(Adjustment::DowngradeSession, claims::RUN_DECOUPLE_001),
        )
    } else {
        None
    }
}

/// The shared clock and derived inputs the rules read beyond the readiness
/// inputs themselves. `Default` = no clock, fixed RHR bands, no logged-set
/// performance.
#[derive(Debug, Clone, Copy, Default)]
pub struct RuleContext<'a> {
    /// The shell's local today: only a check-in dated today drives the
    /// single-day wellness arm (WELLNESS-Z-001). `None` = no clock.
    pub today_epoch_day: Option<i64>,
    pub utc_offset_sec: i64,
    /// Individual resting-HR noise from the check-in derivation.
    pub rhr: Option<RhrNoise>,
    /// Per-lift e1RM performance derived from logged sets
    /// ([`derive_performance`](super::derive_performance)).
    pub perf: &'a [MovementPerf],
}

/// Whether a set of recommendations signals an objective decline (a deload or
/// a load cut, item-scoped or not): §5 tier 3 and the autoreg-044 strip.
fn perf_decline(recs: &[AdjustmentCall]) -> bool {
    recs.iter().any(|r| {
        matches!(
            r.value.base(),
            Adjustment::Deload { .. } | Adjustment::ReduceLoadPct(_)
        )
    })
}

impl AdjustmentKind {
    /// The File 06 §5 ladder rung of the rule (File 08 §5 for referrals):
    /// medical referral > pain > illness > objective performance > subjective
    /// multi-day > HRV trend > single-day markers.
    pub fn tier(self) -> SafetyTier {
        use AdjustmentKind as K;
        match self {
            K::Screen(_)
            | K::CardiacReferral
            | K::BoneStressReferral
            | K::RedsReferral
            | K::OvertrainingReferral
            | K::NforReferral => SafetyTier::MedicalReferral,
            // Any characterized non-DOMS pain sits at the Pain tier, even when
            // its response is a scoped stop or a modify/reduce (autoreg-043).
            K::PainStop
            | K::PainDefer
            | K::PainScoped(_)
            | K::TendonReduce
            | K::TendonModify(_) => SafetyTier::Pain,
            // The corroborated resting-HR rest day blocks training like the
            // illness stop (ready-upd-002).
            K::IllnessStop | K::HeadCold | K::ReturnCheck | K::ReturnHold | K::RhrRest => {
                SafetyTier::Illness
            }
            K::RpeLoad
            | K::FirstSet
            | K::FailedSet
            | K::E1rm
            | K::E1rmDeload
            | K::VelocityLoss
            | K::Decoupling
            | K::WeeklyDeload(_) => SafetyTier::ObjectivePerformance,
            K::WellnessLower => SafetyTier::SubjectiveMultiDay,
            K::HrvLower | K::HrvHold => SafetyTier::HrvTrend,
            K::WellnessHold | K::Soreness | K::RhrHold | K::RhrSpike => SafetyTier::SingleDayMarker,
        }
    }

    /// Whether the rule raises the day's safety tier. A low check-in or
    /// soreness hold is a single-day subjective call (§5 tier 4 needs ≥3 days
    /// of z ≤ −1, autoreg-030), and the session RPE and decoupling calls are
    /// not the tier-3 e1RM / velocity-loss decline: they rank on the ladder
    /// but leave the day's tier alone.
    fn raises_day_tier(self) -> bool {
        !matches!(
            self,
            AdjustmentKind::WellnessHold
                | AdjustmentKind::Soreness
                | AdjustmentKind::RpeLoad
                | AdjustmentKind::Decoupling
        )
    }
}

impl AdjustmentCall {
    /// The call's rung for ranking: a deferral ranks as a medical referral
    /// whatever raised it, and a load increase is off the ladder.
    pub fn tier(&self) -> Option<SafetyTier> {
        match self.value.base() {
            Adjustment::Defer { .. } => Some(SafetyTier::MedicalReferral),
            Adjustment::IncreaseLoadPct(_) => None,
            _ => Some(self.kind().tier()),
        }
    }
}

/// Highest safety tier the day's calls raise (File 06 §5 ladder;
/// autoreg-041/043/046). `None` when no safety condition fires. A load
/// increase signals high readiness, never a safety concern.
pub fn safety_tier_of(calls: &[AdjustmentCall]) -> Option<SafetyTier> {
    calls
        .iter()
        .filter(|r| {
            r.kind().raises_day_tier() && !matches!(r.value.base(), Adjustment::IncreaseLoadPct(_))
        })
        .map(|r| r.kind().tier())
        .max()
}

/// Whether auto load-adds are withheld today (autoreg-044 / autoreg-029): any
/// recovery-suppression signal (wellness — even a single low day: §5 "proceed,
/// cap top-end, do NOT add load" —, soreness, HRV, resting HR, active pain or
/// illness), an objective performance decline (velocity-loss stop, e1RM drop,
/// aerobic decoupling, a hard first set), or the high-load-block HRV
/// saturation guard. The plan-side per-item first-set rule strips its
/// increases on the same predicate. `lift_goal` selects the velocity-loss
/// threshold; `high_load_block` arms the saturation guard.
pub fn increase_hold(
    inputs: &[ReadinessInput],
    ctx: &RuleContext,
    lift_goal: Option<LiftGoal>,
    high_load_block: bool,
) -> bool {
    let suppressed = hrv_downgrade(inputs).is_some()
        || latest_input(inputs, ReadinessSignal::WellnessZ)
            .is_some_and(|i| wellness_verdict(i, ctx) != WellnessVerdict::Normal)
        || soreness_downgrade(inputs).is_some()
        || rhr_verdict(inputs, ctx) != RhrVerdict::Normal
        || pain_blocks_progression(inputs)
        || illness_active(inputs);
    // An e1RM *increase* (ratio > 1.05) is not a decline and does not suppress.
    let objective_perf_down = vl_stop(inputs, lift_goal).is_some()
        || decoupling_downgrade(inputs).is_some()
        || perf_decline(&e1rm_gate(inputs))
        || rpe_load_adjust(inputs).is_some_and(|r| matches!(r.value, Adjustment::ReduceLoadPct(_)))
        || perf_decline(&perf_adjustments(ctx.perf));
    // autoreg-029 (parasympathetic saturation): in a high-load block, lnRMSSD
    // ABOVE the SWC upper limit (z > +0.5) also blocks auto load-adds, hold
    // and weigh with wellness (AUTOREG-HRV-SAT-001, Moderate, Plews).
    let saturation_hold = latest(inputs, ReadinessSignal::HrvLnRmssd)
        .is_some_and(|z| hrv_saturation_hold(z, high_load_block).value);
    suppressed || saturation_hold || objective_perf_down
}

/// Apply the File 06 IF/THEN rules (as amended by File 13 ready-upd-001..007),
/// emitting evidence-cited adjustments. A safety stop dominates and returns
/// alone (led by any non-blocking pain response); otherwise every matching
/// rule contributes, and the autoreg-044 pass strips every load increase
/// while [`increase_hold`] holds.
pub fn adjustments_in(
    inputs: &[ReadinessInput],
    ctx: &RuleContext,
    lift_goal: Option<LiftGoal>,
    high_load_block: bool,
) -> Vec<AdjustmentCall> {
    // Medical referral (File 08) > pain (autoreg-043 / Table 4.1) >
    // illness/fever (autoreg-046) > corroborated RHR (ready-upd-002).
    if let Some(defer) = medical_referral(inputs) {
        return vec![defer];
    }
    if let PainGate::Block(stop) = pain_gate(inputs) {
        return vec![stop];
    }
    // Non-blocking pain responses (scoped sharp/joint stops, tendon
    // modify/reduce) raise `SafetyTier::Pain`, so they lead the list, even ahead
    // of an illness/RHR stop, so the headline names the tier the strip shows.
    let pain_recs = active_pain_responses(inputs);
    if illness_stop(inputs) {
        let mut out = pain_recs;
        out.extend(illness_plan_recs(inputs));
        return out;
    }
    if rhr_verdict(inputs, ctx) == RhrVerdict::Rest {
        let mut out = pain_recs;
        out.push(AdjustmentKind::RhrRest.call(Adjustment::RestDay, claims::AUTOREG_RHR_NOISE_001));
        return out;
    }

    // Non-stop rules accumulate (deterministic order).
    let mut out = pain_recs;
    out.extend(rpe_load_adjust(inputs));
    out.extend(e1rm_gate(inputs));
    out.extend(perf_adjustments(ctx.perf));
    out.extend(vl_stop(inputs, lift_goal));
    out.extend(hrv_downgrade(inputs));
    out.extend(wellness_downgrade_in(inputs, ctx));
    out.extend(soreness_downgrade(inputs));
    // autoreg-045: above-neck only → an easy session at half the volume.
    out.extend(illness_plan_recs(inputs));
    out.extend(rhr_downgrade_in(inputs, ctx));
    out.extend(decoupling_downgrade(inputs));

    // Reconciliation pass (autoreg-044, safety-critical guardrail): never
    // auto-increase load into suppressed recovery or declining performance,
    // item-scoped increases included.
    if increase_hold(inputs, ctx, lift_goal, high_load_block) {
        out.retain(|r| !matches!(r.value.base(), Adjustment::IncreaseLoadPct(_)));
    }
    out
}
