//! The readiness-stream rules: File 06 IF→THEN rules as amended by File 13
//! ready-upd-001..007, their threshold constants, the §5 safety ladder, and the
//! illness / resting-HR / medical-referral predicates.

use super::pain::{PainGate, active_pain_responses, pain_gate};
use super::perf::{MovementPerf, perf_adjustments};
use super::prescriptions::hrv_saturation_hold;
use super::{latest, latest_input};
use crate::evidence::{self, claims};
use crate::schema::{
    Adjustment, Goal, IllnessSeverity, ReadinessInput, ReadinessSignal, Recommended, SafetyTier,
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
pub fn illness_plan_recs(inputs: &[ReadinessInput]) -> Vec<Recommended<Adjustment>> {
    if illness_stop(inputs) {
        vec![evidence::graded(Adjustment::Stop, claims::ILLNESS_NECK_001)]
    } else if illness_downgrade(inputs) {
        // The volume cut carries the full head-cold call ("short and easy only,
        // or rest"), so it leads: the headline takes the first of equals.
        vec![
            evidence::graded(
                Adjustment::ReduceVolumePct(HEAD_COLD_VOLUME_CUT_PCT),
                claims::ILLNESS_NECK_001,
            ),
            evidence::graded(Adjustment::DowngradeSession, claims::ILLNESS_NECK_001),
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

/// Bone-stress-injury referral (safety-040): stop impact, urgent evaluation.
/// One constructor for the readiness signal and the weekly check-in answer, so
/// both channels defer with the same copy.
pub fn bone_stress_referral() -> Recommended<Adjustment> {
    evidence::graded(
        Adjustment::Defer {
            reason: "Signs of a bone stress injury. Stop running and jumping now and see a doctor as soon as you can.".into(),
        },
        claims::SAFE_BSI_001,
    )
}

/// RED-S / low-energy-availability referral (safety-049, whose signal list
/// includes compulsive exercise): reduce stress and defer. Shared by the
/// readiness signal and the weekly screen.
pub fn reds_referral() -> Recommended<Adjustment> {
    evidence::graded(
        Adjustment::Defer {
            reason: "Signs of under-eating (RED-S). Train less and see a doctor, registered dietitian or mental-health professional.".into(),
        },
        claims::SAFE_REDS_001,
    )
}

/// The dominant medical-referral deferral, if any red flag is present.
/// Deterministic priority: cardiovascular (acute) > bone stress > RED-S.
/// Returns the `Recommended<Adjustment::Defer>` cited to its safety claim.
fn medical_referral(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    if cardiac_defer(inputs) {
        Some(evidence::graded(
            Adjustment::Defer {
                reason: "A heart warning sign. Stop and get a doctor's OK before you train again."
                    .into(),
            },
            claims::SAFE_CVD_001,
        ))
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

// Threshold constants: the SINGLE SOURCE OF TRUTH for every numeric band of
// the readiness rules. Both the adjustment rules AND the readiness rows
// (`signals.rs`, the displayed "why") read these named constants, so an applied
// adjustment and the text that explains it can never silently desync. Values
// are verbatim from File 06; changing a value here moves the rule and its
// explanation together. Do not re-inline these literals on either side.

/// RPE delta (actual − target) bands (autoreg-001/002/004/005).
pub(super) const RPE_DELTA_WELL_ABOVE: f64 = 2.0;
pub(super) const RPE_DELTA_ABOVE: f64 = 1.0;
pub(super) const RPE_DELTA_WELL_BELOW: f64 = -2.0;
pub(super) const RPE_DELTA_BELOW: f64 = -1.0;

/// e1RM ratio (today ÷ baseline) gates (autoreg-022/006/007).
pub(super) const E1RM_DELOAD_RATIO: f64 = 0.90;
pub(super) const E1RM_DELOAD_STREAK: u8 = 2;
pub(super) const E1RM_REDUCE_RATIO: f64 = 0.95;
pub(super) const E1RM_INCREASE_RATIO: f64 = 1.05;
/// Top-set load cut below baseline − 5 % (autoreg-006 "reduce top-set load ~5%").
pub(super) const E1RM_REDUCE_LOAD_PCT: f32 = 5.0;
/// Load add above baseline + 5 % (autoreg-007 "+2.5–5%"; 3.5 % used).
pub(super) const E1RM_INCREASE_LOAD_PCT: f32 = 3.5;

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

/// Width (local days) of the HRV rolling window: the `HrvLnRmssd` channel emits
/// the mean of the trailing 7 days' z-scores (autoreg-028's 7-day rolling
/// baseline) so one off morning cannot trip a gate a sustained shift should.
pub(super) const HRV_ROLLING_DAYS: i64 = 7;

/// Minimum lnRMSSD readings inside the trailing rolling window before the HRV
/// channel emits a weekly baseline (autoreg-047: "need ≥4–5 readings/week for a
/// valid weekly baseline"). Below this the window is too sparse to trust.
pub(super) const HRV_MIN_WEEKLY_READINGS: usize = 4;

/// Recency window (LOCAL days) for the derived RHR-delta and wellness-z channels:
/// a check-in signal older than this many days no longer drives a verdict (the
/// user has not checked in recently). HRV is not routed through this per-reading
/// gate — it reads a trailing rolling window and carries its own recency bound
/// (`HRV_RECENCY_DAYS`, against both today and the newest check-in). `1` = today
/// or yesterday counts as current.
pub(super) const CHECKIN_RECENCY_DAYS: i64 = 1;

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
pub(super) fn rpe_load_adjust(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let delta = latest(inputs, ReadinessSignal::Rpe)?;
    rpe_delta_adjustment(delta).map(|a| evidence::graded(a, claims::AUTOREG_RIR_001))
}

/// The same autoreg-001/002/004/005 bands applied to ONE plan item from its
/// logged first work set: the change is scoped to that item's
/// remaining sets ("reduce load for remaining sets"), never the whole session.
/// `delta` = logged RPE − the item's target RPE. AUTOREG-RIR-001 (Strong).
pub fn first_set_rpe_adjust(exercise: &str, delta: f64) -> Option<Recommended<Adjustment>> {
    rpe_delta_adjustment(delta).map(|a| {
        evidence::graded(
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
pub fn failed_set_stop(exercise: &str) -> Recommended<Adjustment> {
    evidence::graded(
        Adjustment::ForItem {
            exercise: exercise.to_string(),
            adjustment: Box::new(Adjustment::DowngradeSession),
        },
        claims::AUTOREG_RIR_001,
    )
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
/// autoreg-007: e1RM > baseline + 5% (ratio > 1.05) → add load ~2.5–5% (uses
/// 3.5%). Same Helms 2018 e1RM-gate source as its mirror autoreg-006, so it
/// cites AUTOREG-E1RM-GATE-001.
pub(super) fn e1rm_gate(inputs: &[ReadinessInput]) -> Vec<Recommended<Adjustment>> {
    let Some(input) = latest_input(inputs, ReadinessSignal::EstimatedOneRm) else {
        return Vec::new();
    };
    match e1rm_ratio_outcome(input.value, input.streak) {
        E1rmOutcome::Deload => vec![evidence::graded(
            e1rm_decline_deload(),
            claims::AUTOREG_DELOAD_001,
        )],
        // Includes a single-session >10% drop: File 06 §5 conflict table -
        // "performance down >10% → trust performance → reduce load" (one
        // session reduces; only the ≥2-session streak deloads). autoreg-006's
        // second clause caps today's session at planned RPE − 1 alongside the
        // ~5% top-set cut.
        E1rmOutcome::Reduce => vec![
            evidence::graded(
                Adjustment::ReduceLoadPct(E1RM_REDUCE_LOAD_PCT),
                claims::AUTOREG_E1RM_GATE_001,
            ),
            evidence::graded(Adjustment::CapRpe(1), claims::AUTOREG_E1RM_GATE_001),
        ],
        E1rmOutcome::Increase => vec![evidence::graded(
            Adjustment::IncreaseLoadPct(E1RM_INCREASE_LOAD_PCT),
            claims::AUTOREG_E1RM_GATE_001,
        )],
        E1rmOutcome::Stable => Vec::new(),
    }
}

/// Which arm of the e1RM rule (autoreg-022/006/007) a ratio today ÷ baseline
/// and its consecutive-decline streak select. The one classification the
/// manual gate, the logged-set performance gate and the readiness row share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum E1rmOutcome {
    /// Down >10 % for ≥2 consecutive sessions (autoreg-022).
    Deload,
    /// Below baseline − 5 % (autoreg-006), including a single >10 % drop.
    Reduce,
    /// Above baseline + 5 % (autoreg-007).
    Increase,
    Stable,
}

pub(super) fn e1rm_ratio_outcome(ratio: f64, streak: u8) -> E1rmOutcome {
    if ratio < E1RM_DELOAD_RATIO && streak >= E1RM_DELOAD_STREAK {
        E1rmOutcome::Deload
    } else if ratio < E1RM_REDUCE_RATIO {
        E1rmOutcome::Reduce
    } else if ratio > E1RM_INCREASE_RATIO {
        E1rmOutcome::Increase
    } else {
        E1rmOutcome::Stable
    }
}

/// The autoreg-022 e1RM-decline deload: 1 week at the midpoints of its
/// volume −40–50 % and load −5–10 % bands.
pub(super) fn e1rm_decline_deload() -> Adjustment {
    Adjustment::Deload {
        volume_reduction_pct: 45.0,
        load_reduction_pct: 7.5,
        weeks: 1,
    }
}

/// Goal-dependent velocity-loss termination threshold, as whole-percent. The
/// single source of the goal→VL map is `strength::vl_termination_threshold` (a
/// fraction); this maps the top-level `Goal` onto its `LiftGoal` and scales to
/// percent, so the two files can never drift (reconciling autoreg-010's
/// "10% power" against strength-018's grouping: power 10% / max-strength 20% /
/// hypertrophy 40%). Each goal uses its band ceiling, the maximum VL the plan
/// would ever prescribe, so exceeding it is beyond-plan fatigue. A non-lifting
/// or absent goal → the 20% max-strength default (the conservative fallback).
pub(super) fn vl_threshold_pct(goal: Option<&Goal>) -> f64 {
    let lift_goal = match goal {
        Some(Goal::Power) => LiftGoal::Power,
        Some(Goal::Hypertrophy) => LiftGoal::Hypertrophy,
        // Strength and every non-lifting/absent goal fall back to max-strength.
        _ => LiftGoal::MaxStrength,
    };
    strength::vl_termination_threshold(lift_goal).value * 100.0
}

/// Velocity-loss set-termination stop.
/// autoreg-010: within-set VL ≥ goal threshold → terminate set (volume cut).
/// `VelocityLoss` carries the % drop; the threshold is goal-dependent.
pub(super) fn vl_stop(
    inputs: &[ReadinessInput],
    goal: Option<&Goal>,
) -> Option<Recommended<Adjustment>> {
    let vl = latest(inputs, ReadinessSignal::VelocityLoss)?;
    if vl >= vl_threshold_pct(goal) {
        Some(evidence::graded(
            Adjustment::DowngradeSession,
            claims::AUTOREG_VL_001,
        ))
    } else {
        None
    }
}

/// Consecutive suppressed HRV days at which a lift session is lowered, not
/// just held (autoreg-034: "lnRMSSD7d suppressed ≥3–4 days → recovery day /
/// easy block"; ready-upd-001 "several days of feeling run-down").
pub(super) const HRV_ESCALATE_STREAK: u8 = 3;

/// HRV rolling-baseline verdict (autoreg-028 as scoped by ready-upd-001).
/// `HrvLnRmssd` carries the 7-day mean z vs the rolling baseline; z < −0.5
/// clears the SWC lower band. HRV-guided training is an endurance tool, so a
/// suppressed week HOLDS progression (`CapTopEnd`: runs easy, lifts at the
/// planned load one rep further from failure, AUTOREG-HRV-LIFT-001); only a
/// ≥3-day suppression streak lowers the session (`DowngradeSession`, HRV-001).
pub(super) fn hrv_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let i = latest_input(inputs, ReadinessSignal::HrvLnRmssd)?;
    if i.value >= HRV_SWC_LOWER_Z {
        return None;
    }
    Some(if i.streak >= HRV_ESCALATE_STREAK {
        evidence::graded(Adjustment::DowngradeSession, claims::HRV_001)
    } else {
        evidence::graded(
            Adjustment::CapTopEnd {
                trigger: ReadinessSignal::HrvLnRmssd,
            },
            claims::AUTOREG_HRV_LIFT_001,
        )
    })
}

/// Whether a derived/entered reading is dated on the shell's local today (an
/// undated legacy reading, or no clock, counts as today).
pub(super) fn dated_today(i: &ReadinessInput, ctx: &RuleContext) -> bool {
    ctx.today_epoch_day.is_none_or(|t| {
        i.observed_at <= 0 || crate::local_day(i.observed_at, ctx.utc_offset_sec) == t
    })
}

/// The subjective-wellness verdict (autoreg-030; §5 conflict table;
/// ready-upd-001 and ready-upd-007). The one predicate the rule
/// ([`wellness_downgrade_in`]), the tier ([`wellness_multiday`]) and the
/// readiness label share.
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
    if v <= WELLNESS_MULTIDAY_Z && input.streak >= WELLNESS_MULTIDAY_STREAK {
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
) -> Option<Recommended<Adjustment>> {
    let input = latest_input(inputs, ReadinessSignal::WellnessZ)?;
    match wellness_verdict(input, ctx) {
        WellnessVerdict::Lower => Some(evidence::graded(
            Adjustment::DowngradeSession,
            claims::WELLNESS_001,
        )),
        WellnessVerdict::Hold => Some(evidence::graded(
            Adjustment::CapTopEnd {
                trigger: ReadinessSignal::WellnessZ,
            },
            claims::AUTOREG_HRV_LIFT_001,
        )),
        WellnessVerdict::Low | WellnessVerdict::Normal => None,
    }
}

/// §5 tier-4 condition: wellness composite z ≤ −1 for ≥3 consecutive days.
/// Only this raises `SafetyTier::SubjectiveMultiDay`; the ≥2-day intensity
/// downgrade ([`wellness_downgrade_in`]) does not raise that tier. The verdict's
/// Lower arm is clock-independent, so the default context is exact here.
fn wellness_multiday(inputs: &[ReadinessInput]) -> bool {
    latest_input(inputs, ReadinessSignal::WellnessZ)
        .is_some_and(|i| wellness_verdict(i, &RuleContext::default()) == WellnessVerdict::Lower)
}

/// Wellness soreness-item verdict (autoreg-030 second clause): a soreness item
/// ≥6 on the 7-point scale. Localized soreness modifies, it does not stop or
/// lower systemic intensity (§5 conflict table: "high localized soreness →
/// train; modify the sore muscle's eccentric work; keep systemic intensity"),
/// so lifts stop one rep earlier at the planned load and runs are untouched.
pub(super) fn soreness_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let v = latest(inputs, ReadinessSignal::Soreness)?;
    if v >= SORENESS_HIGH {
        Some(evidence::graded(
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
) -> Option<Recommended<Adjustment>> {
    match rhr_verdict(inputs, ctx) {
        RhrVerdict::Hold => Some(evidence::graded(
            Adjustment::CapTopEnd {
                trigger: ReadinessSignal::RestingHr,
            },
            claims::AUTOREG_HRV_LIFT_001,
        )),
        RhrVerdict::Spike => Some(evidence::graded(
            Adjustment::DowngradeSession,
            claims::AUTOREG_RHR_NOISE_001,
        )),
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
pub(super) fn decoupling_downgrade(inputs: &[ReadinessInput]) -> Option<Recommended<Adjustment>> {
    let input = latest_input(inputs, ReadinessSignal::AerobicDecoupling)?;
    if !input
        .effort_min
        .is_some_and(|d| d > DECOUPLING_MIN_EFFORT_MIN)
    {
        return None;
    }
    if crate::load::decoupling_high(input.value) {
        Some(evidence::graded(
            Adjustment::DowngradeSession,
            claims::RUN_DECOUPLE_001,
        ))
    } else {
        None
    }
}

/// Everything the rule layer reads beyond the readiness inputs themselves.
/// `Default` = no goal, no block, no clock, fixed RHR bands, no logged-set
/// performance.
#[derive(Debug, Clone, Copy, Default)]
pub struct RuleContext<'a> {
    /// Selects the goal-dependent velocity-loss threshold (autoreg-010).
    pub goal: Option<&'a Goal>,
    /// Arms the autoreg-029 parasympathetic-saturation guard.
    pub high_load_block: bool,
    /// The shell's local today: only a check-in dated today drives the
    /// single-day wellness arm (WELLNESS-Z-001). `None` = no clock.
    pub today_epoch_day: Option<i64>,
    pub utc_offset_sec: i64,
    /// Individual resting-HR noise from the check-in derivation.
    pub rhr: Option<RhrNoise>,
    /// Per-lift e1RM performance derived from logged sets
    /// ([`derive_performance`](super::derive_performance)).
    pub perf: &'a [MovementPerf],
    /// The wellness composite is still collecting its baseline: a reading that
    /// fires the rule is the absolute floor of a very low morning, not a drop
    /// below a personal normal that doesn't exist yet.
    pub wellness_collecting: bool,
}

/// Whether a set of recommendations signals an objective decline (a deload or
/// a load cut, item-scoped or not): §5 tier 3 and the autoreg-044 strip.
fn perf_decline(recs: &[Recommended<Adjustment>]) -> bool {
    recs.iter().any(|r| {
        matches!(
            r.value.base(),
            Adjustment::Deload { .. } | Adjustment::ReduceLoadPct(_)
        )
    })
}

/// Highest safety tier triggered by the inputs (File 06 §5 ladder).
///
/// Pain outranks Illness, which outranks all objective/subjective/HRV tiers.
/// Returns `None` when no safety condition fires. autoreg-041/043/046.
pub fn resolve_safety_in(inputs: &[ReadinessInput], ctx: &RuleContext) -> Option<SafetyTier> {
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
    // Any characterized non-DOMS pain (structural, scoped structural, tendon,
    // other) sits at the Pain tier even when its response is a scoped stop or a
    // modify/reduce; DOMS is normal training discomfort and raises no tier.
    match pain_gate(inputs) {
        PainGate::Block(_) | PainGate::Scoped(_) | PainGate::Adjust(_) => raise(SafetyTier::Pain),
        PainGate::Continue | PainGate::None => {}
    }
    // Tier 2, illness / fever gates regardless of readiness (autoreg-045/046),
    // plus the corroborated resting-HR rest day (stop level on ≥2 mornings or
    // with symptoms, ready-upd-002): it blocks training like the illness stop.
    let rhr = rhr_verdict(inputs, ctx);
    if illness_active(inputs) || rhr == RhrVerdict::Rest {
        raise(SafetyTier::Illness);
    }
    // Tier 3, objective within-session performance (e1RM drop / velocity loss).
    // A load *increase* signals high readiness, never a safety concern, only
    // reductions/deloads raise the tier.
    if perf_decline(&e1rm_gate(inputs))
        || vl_stop(inputs, ctx.goal).is_some()
        || perf_decline(&perf_adjustments(ctx.perf))
    {
        raise(SafetyTier::ObjectivePerformance);
    }
    // Tier 4, persistent multi-day subjective suppression: ≥3 days of
    // wellness z ≤ −1 (§5 tier-4 definition). A single-day z ≤ −1.5
    // (autoreg-030) does NOT raise this tier.
    if wellness_multiday(inputs) {
        raise(SafetyTier::SubjectiveMultiDay);
    }
    // Tier 5, HRV rolling baseline (autoreg-028).
    if hrv_downgrade(inputs).is_some() {
        raise(SafetyTier::HrvTrend);
    }
    // Tier 6, resting-HR markers: a corroborated ≥2-day elevation or a single
    // stop-level morning without symptoms (likely noise, ready-upd-002).
    if matches!(rhr, RhrVerdict::Hold | RhrVerdict::Spike) {
        raise(SafetyTier::SingleDayMarker);
    }

    tier
}

/// Whether auto load-adds are withheld today (autoreg-044 / autoreg-029): any
/// recovery-suppression signal (wellness — even a single low day: §5 "proceed,
/// cap top-end, do NOT add load" —, soreness, HRV, resting HR, active pain or
/// illness), an objective performance decline (velocity-loss stop, e1RM drop,
/// aerobic decoupling, a hard first set), or the high-load-block HRV
/// saturation guard. The plan-side per-item first-set rule strips its
/// increases on the same predicate.
pub fn increase_hold(inputs: &[ReadinessInput], ctx: &RuleContext) -> bool {
    let pain_any = matches!(
        pain_gate(inputs),
        PainGate::Scoped(_) | PainGate::Adjust(_) | PainGate::Block(_)
    );
    let suppressed = hrv_downgrade(inputs).is_some()
        || wellness_downgrade_in(inputs, ctx).is_some()
        || latest(inputs, ReadinessSignal::WellnessZ).is_some_and(|z| z <= WELLNESS_MULTIDAY_Z)
        || soreness_downgrade(inputs).is_some()
        || rhr_verdict(inputs, ctx) != RhrVerdict::Normal
        || pain_any
        || illness_active(inputs);
    // An e1RM *increase* (ratio > 1.05) is not a decline and does not suppress.
    let objective_perf_down = vl_stop(inputs, ctx.goal).is_some()
        || decoupling_downgrade(inputs).is_some()
        || perf_decline(&e1rm_gate(inputs))
        || rpe_load_adjust(inputs).is_some_and(|r| matches!(r.value, Adjustment::ReduceLoadPct(_)))
        || perf_decline(&perf_adjustments(ctx.perf));
    // autoreg-029 (parasympathetic saturation): in a high-load block, lnRMSSD
    // ABOVE the SWC upper limit (z > +0.5) also blocks auto load-adds, hold
    // and weigh with wellness (AUTOREG-HRV-SAT-001, Moderate, Plews).
    let saturation_hold = latest(inputs, ReadinessSignal::HrvLnRmssd)
        .is_some_and(|z| hrv_saturation_hold(z, ctx.high_load_block).value);
    suppressed || saturation_hold || objective_perf_down
}

/// Apply the File 06 IF/THEN rules (as amended by File 13 ready-upd-001..007),
/// emitting evidence-cited adjustments.
///
/// When a Stop-level safety condition fires (structural/uncharacterized pain,
/// fever/below-neck illness, or a corroborated resting-HR elevation) it
/// dominates: the returned vec contains only the safety stop (led by any
/// non-blocking pain response, which outranks it on the ladder). Otherwise
/// every matching optimization rule contributes its adjustment, and a final
/// reconciliation pass (autoreg-044) strips every load increase whenever
/// [`increase_hold`] holds: the engine never auto-increases load into
/// suppressed recovery.
///
/// Magnitudes follow the evidence (ready-upd-001): HRV, resting HR, a single
/// low check-in and soreness only HOLD progression on lifts (`CapTopEnd`); a
/// lift session is lowered (`DowngradeSession`, one level) only on several days
/// of suppression or a hard logged first set (the per-item first-set rule,
/// applied plan-side).
pub fn adjustments_in(
    inputs: &[ReadinessInput],
    ctx: &RuleContext,
) -> Vec<Recommended<Adjustment>> {
    // Safety override: medical referral (File 08) > pain (autoreg-043 /
    // Table 4.1) > illness/fever (autoreg-046) > corroborated RHR (ready-upd-002).
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
        out.push(evidence::graded(
            Adjustment::RestDay,
            claims::AUTOREG_RHR_NOISE_001,
        ));
        return out;
    }

    // Non-stop rules accumulate (deterministic order).
    let mut out = pain_recs;
    out.extend(rpe_load_adjust(inputs));
    out.extend(e1rm_gate(inputs));
    out.extend(perf_adjustments(ctx.perf));
    out.extend(vl_stop(inputs, ctx.goal));
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
    if increase_hold(inputs, ctx) {
        out.retain(|r| !matches!(r.value.base(), Adjustment::IncreaseLoadPct(_)));
    }
    out
}
