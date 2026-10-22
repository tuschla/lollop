//! Per-signal readiness states: the KB-honest readiness summary. No composite
//! score exists in the KB, so none is emitted (HARD RULE 1); each row restates
//! the verdict of the rule that reads the signal.

use std::borrow::Cow;

use super::latest_input;
use super::pain::{PainVerdict, classify_pain, pain_context, worst_active_pain};
use super::rules::{
    DECOUPLING_MIN_EFFORT_MIN, E1RM_DELOAD_RATIO, E1RM_DELOAD_STREAK, E1rmOutcome, HRV_SWC_LOWER_Z,
    HRV_SWC_UPPER_Z, RPE_DELTA_ABOVE, RPE_DELTA_BELOW, RPE_DELTA_WELL_ABOVE, RPE_DELTA_WELL_BELOW,
    RhrVerdict, RuleContext, SORENESS_HIGH, WellnessVerdict, dated_today, e1rm_ratio_outcome,
    rhr_corroborated, rhr_thresholds, rhr_verdict, vl_threshold_pct, wellness_verdict,
};
use crate::evidence::{ClaimId, claims};
use crate::schema::{Goal, IllnessSeverity, ReadinessInput, ReadinessSignal};

/// Which picker/summary group a readiness signal belongs to. `RedFlag` is the
/// medical-referral / hard-stop block (Pain, Illness, RED-S, cardiac, bone
/// stress) that shells must visually fence off from the routine `Metric`
/// signals, the fence is data-driven from here, not a shell-side predicate.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignalGroup {
    Metric,
    RedFlag,
}

pub fn signal_group(signal: ReadinessSignal) -> SignalGroup {
    match signal {
        ReadinessSignal::Pain
        | ReadinessSignal::Illness
        | ReadinessSignal::RedS
        | ReadinessSignal::CardiacRedFlag
        | ReadinessSignal::BoneStress => SignalGroup::RedFlag,
        _ => SignalGroup::Metric,
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
/// by the SAME File 06/08 thresholds the adjustment rules use, never a
/// new number, never a composite score (no 0–100 readiness index exists in the
/// KB, so none is emitted; HARD RULE 1).
pub struct SignalState {
    pub signal: ReadinessSignal,
    pub value: f64,
    pub streak: u8,
    /// Qualitative state, e.g. `"below normal"` / `"well above normal: rest"`.
    pub state: String,
    /// Extra display context for the shell, e.g. the pain sub-line
    /// `"Left knee · sharp/joint · 6/10"`. Empty for signals that carry no
    /// characterizing detail. Display-only, never a graded claim.
    pub detail: String,
    /// Registry claim of the rule whose threshold judged the state. `None`
    /// when the row is a plain factual echo (a recorded-only signal with no
    /// gating rule, or an explicit all-clear), those judge nothing, so they
    /// carry no evidence tag.
    pub claim: Option<ClaimId>,
    /// Whether the displayed reading is dated on the shell's local today (true
    /// without a clock or for an undated legacy reading). False = the value comes
    /// from an earlier check-in.
    pub from_today: bool,
    /// The displayed reading is recency-expired (routed to `stale`): `state`
    /// then reads "out of date" and already says the value is not current.
    pub stale: bool,
}

/// A row's qualitative state and the claim of the rule that judged it.
type Label = (Cow<'static, str>, Option<ClaimId>);

/// State shown for a row whose displayed reading is recency-expired.
const NO_CHECKIN_TODAY: &str = "out of date";

/// Latest state of every signal that has at least one observation, in
/// [`ALL_SIGNALS`] order, judged with the full rule context (individual
/// resting-HR bands, the shell's today). Pure re-statement of the rule layer:
/// each state string is decided by the same predicate/threshold that drives the
/// matching adjustment, and cites the same claim.
pub fn signal_states_in(
    inputs: &[ReadinessInput],
    ctx: &RuleContext,
    // Readings routed to `stale` by `derive_readiness` (recency-expired). A row
    // whose displayed reading is one of these is not today's verdict, so it
    // renders "out of date" instead of a stale judgement. Local-day recency
    // gating lives solely in `derive_channel`; this is a pure membership check.
    stale: &[ReadinessInput],
) -> Vec<SignalState> {
    ALL_SIGNALS
        .iter()
        .filter_map(|&signal| {
            let input = displayed_input(inputs, signal)?;
            let v = input.value;
            let (state, claim) = signal_label(signal, input, inputs, ctx);
            // A stale row keeps its grade and citation; only the state text
            // admits there is no current reading.
            let stale_row = stale
                .iter()
                .any(|i| i.signal == signal && i.observed_at == input.observed_at);
            let state = if stale_row {
                NO_CHECKIN_TODAY.to_string()
            } else {
                state.into_owned()
            };
            // Display-only sub-line context: only a characterized pain report
            // carries one (e.g. "Left knee · sharp/joint · 6/10").
            let detail = match (signal, &input.pain) {
                (ReadinessSignal::Pain, Some(d)) if v > 0.0 => pain_context(d),
                _ => String::new(),
            };
            Some(SignalState {
                signal,
                value: v,
                streak: input.streak,
                state,
                detail,
                claim,
                from_today: dated_today(input, ctx),
                stale: stale_row,
            })
        })
        .collect()
}

/// The reading a signal's row displays. The Pain row/banner names the SAME
/// report the gate holds on (the worst active report), not merely the newest:
/// a later milder ache would otherwise mislabel a red-flag stop and mis-target
/// the "remove" undo. It falls back to the latest reading (a clear) when
/// nothing is active.
fn displayed_input(inputs: &[ReadinessInput], signal: ReadinessSignal) -> Option<&ReadinessInput> {
    if signal == ReadinessSignal::Pain {
        worst_active_pain(inputs).or_else(|| latest_input(inputs, signal))
    } else {
        latest_input(inputs, signal)
    }
}

/// The state of one signal's displayed reading `input`, through the verdict
/// of the rule that reads that signal.
fn signal_label(
    signal: ReadinessSignal,
    input: &ReadinessInput,
    inputs: &[ReadinessInput],
    ctx: &RuleContext,
) -> Label {
    let v = input.value;
    match signal {
        ReadinessSignal::Rpe => rpe_label(v),
        ReadinessSignal::EstimatedOneRm => e1rm_label(v, input.streak),
        // Recorded only: no autoregulation gate exists in the KB yet.
        ReadinessSignal::BarVelocity | ReadinessSignal::HrvCv => ("recorded".into(), None),
        ReadinessSignal::VelocityLoss => velocity_loss_label(v, ctx.goal),
        ReadinessSignal::WellnessZ => wellness_label(input, ctx),
        ReadinessSignal::HrvLnRmssd => hrv_label(input, ctx),
        ReadinessSignal::AerobicDecoupling => decoupling_label(input),
        ReadinessSignal::RestingHr => resting_hr_label(input, inputs, ctx),
        ReadinessSignal::Soreness => soreness_label(v),
        ReadinessSignal::Pain if v <= 0.0 => ("clear".into(), None),
        ReadinessSignal::Pain => pain_label(classify_pain(input)),
        ReadinessSignal::Illness => illness_label(v),
        // File 08 medical-referral red flags (safety-049/043/040).
        ReadinessSignal::RedS => {
            red_flag_label(v, "see a professional first", claims::SAFE_REDS_001)
        }
        ReadinessSignal::CardiacRedFlag => {
            red_flag_label(v, "see a doctor before training", claims::SAFE_CVD_001)
        }
        ReadinessSignal::BoneStress => red_flag_label(
            v,
            "no running or jumping; see a doctor soon",
            claims::SAFE_BSI_001,
        ),
    }
}

/// autoreg-001/002/004/005: signed RPE delta vs target.
fn rpe_label(delta: f64) -> Label {
    let s = if delta >= RPE_DELTA_WELL_ABOVE {
        "well above target"
    } else if delta >= RPE_DELTA_ABOVE {
        "above target"
    } else if delta <= RPE_DELTA_WELL_BELOW {
        "well below target"
    } else if delta <= RPE_DELTA_BELOW {
        "below target"
    } else {
        "on target"
    };
    (s.into(), Some(claims::AUTOREG_RIR_001))
}

/// autoreg-022/006/007: e1RM ratio vs baseline.
fn e1rm_label(ratio: f64, streak: u8) -> Label {
    match e1rm_ratio_outcome(ratio, streak) {
        E1rmOutcome::Deload => (
            format!(
                "down more than {:.0}% for {E1RM_DELOAD_STREAK}+ sessions",
                (1.0 - E1RM_DELOAD_RATIO) * 100.0
            )
            .into(),
            Some(claims::AUTOREG_DELOAD_001),
        ),
        E1rmOutcome::Reduce => ("down".into(), Some(claims::AUTOREG_E1RM_GATE_001)),
        E1rmOutcome::Increase => ("up".into(), Some(claims::AUTOREG_E1RM_GATE_001)),
        E1rmOutcome::Stable => ("stable".into(), Some(claims::AUTOREG_E1RM_GATE_001)),
    }
}

/// autoreg-010: goal-dependent velocity-loss termination band.
fn velocity_loss_label(vl_pct: f64, goal: Option<&Goal>) -> Label {
    let s = if vl_pct >= vl_threshold_pct(goal) {
        "over threshold"
    } else {
        "within threshold"
    };
    (s.into(), Some(claims::AUTOREG_VL_001))
}

/// autoreg-030 via the rule's own verdict: "below normal" only when the
/// hold/lower call actually fires. Before the baseline exists the call can
/// only come from the absolute floor, so the state says the morning was very
/// low, never that it fell below a normal not yet known; "today" only for a
/// reading dated today.
fn wellness_label(input: &ReadinessInput, ctx: &RuleContext) -> Label {
    let s = match wellness_verdict(input, ctx) {
        WellnessVerdict::Lower | WellnessVerdict::Hold if ctx.wellness_collecting => {
            if dated_today(input, ctx) {
                "very low today"
            } else {
                "very low"
            }
        }
        WellnessVerdict::Lower | WellnessVerdict::Hold => "below normal",
        WellnessVerdict::Low if dated_today(input, ctx) => "low today",
        WellnessVerdict::Low => "low yesterday",
        WellnessVerdict::Normal => "normal",
    };
    (s.into(), Some(claims::WELLNESS_001))
}

/// autoreg-028 SWC band on the 7-day mean; autoreg-029 saturation in a
/// high-load block. `streak ≥ 1` means the newest day's own z is below the band
/// (the channel's streak predicate).
fn hrv_label(input: &ReadinessInput, ctx: &RuleContext) -> Label {
    let v = input.value;
    if v < HRV_SWC_LOWER_Z {
        ("below normal".into(), Some(claims::HRV_001))
    } else if v > HRV_SWC_UPPER_Z && ctx.high_load_block {
        (
            "above normal: no extra weight today".into(),
            Some(claims::AUTOREG_HRV_SAT_001),
        )
    } else if input.streak >= 1 && dated_today(input, ctx) {
        (
            "low today, 7-day average normal".into(),
            Some(claims::HRV_001),
        )
    } else if v > HRV_SWC_UPPER_Z {
        ("above normal".into(), Some(claims::HRV_001))
    } else {
        ("normal".into(), Some(claims::HRV_001))
    }
}

/// autoreg-037, valid only for efforts over [`DECOUPLING_MIN_EFFORT_MIN`]
/// (File 06 signal spec).
fn decoupling_label(input: &ReadinessInput) -> Label {
    let s: Cow<'static, str> = match input.effort_min {
        // Duration unknown → cannot validate (the downgrade path discards it
        // too); never render 'high' for an unvalidatable reading.
        None => "not valid (duration unknown)".into(),
        Some(d) if d <= DECOUPLING_MIN_EFFORT_MIN => {
            format!("not valid (effort ≤{DECOUPLING_MIN_EFFORT_MIN} min)").into()
        }
        _ if crate::load::decoupling_high(input.value) => "high".into(),
        _ => "normal".into(),
    };
    (s, Some(claims::RUN_DECOUPLE_001))
}

/// ready-upd-002 (autoreg-040/041 amended): individual bands; a rest day needs
/// ≥2 stop-level mornings or symptoms. "This morning" / "today" only for a
/// reading dated today.
fn resting_hr_label(input: &ReadinessInput, inputs: &[ReadinessInput], ctx: &RuleContext) -> Label {
    let delta_bpm = input.value;
    let today = dated_today(input, ctx);
    match rhr_verdict(inputs, ctx) {
        RhrVerdict::Rest => (
            "well above normal: rest".into(),
            Some(claims::AUTOREG_RHR_NOISE_001),
        ),
        RhrVerdict::Spike => (
            if today {
                "high this morning: easier day, check for illness"
            } else {
                "high: easier day, check for illness"
            }
            .into(),
            Some(claims::AUTOREG_RHR_NOISE_001),
        ),
        RhrVerdict::Hold => ("high 2+ days".into(), Some(claims::AUTOREG_RHR_DOWN_001)),
        // Below the rest bands with no 2-day streak: noise unless another
        // signal from the same morning points the same way (ready-upd-002).
        RhrVerdict::Normal
            if delta_bpm >= rhr_thresholds(ctx).0 && rhr_corroborated(inputs, ctx) =>
        {
            (
                if today {
                    "high this morning, and your other readings are low too"
                } else {
                    "high, and your other readings are low too"
                }
                .into(),
                Some(claims::AUTOREG_RHR_DOWN_001),
            )
        }
        RhrVerdict::Normal if delta_bpm >= rhr_thresholds(ctx).0 => (
            if today {
                "high today, probably a one-off"
            } else {
                "high, probably a one-off"
            }
            .into(),
            Some(claims::AUTOREG_RHR_DOWN_001),
        ),
        RhrVerdict::Normal => ("normal".into(), Some(claims::AUTOREG_RHR_DOWN_001)),
    }
}

/// autoreg-030 second clause: soreness item ≥ [`SORENESS_HIGH`] on 7 points.
fn soreness_label(v: f64) -> Label {
    let s = if v >= SORENESS_HIGH { "high" } else { "normal" };
    (s.into(), Some(claims::WELLNESS_001))
}

/// File 08 Table 4.1 graded pain model, through the gate's own classifier.
fn pain_label(verdict: PainVerdict) -> Label {
    let (s, claim) = match verdict {
        PainVerdict::BareStop | PainVerdict::OtherStop => ("stop training", claims::SAFE_PAIN_001),
        PainVerdict::StructuralDefer => {
            ("see a doctor or physio first", claims::SAFE_PAIN_STRUCT_001)
        }
        PainVerdict::StructuralScoped => (
            "skip the exercises that load it",
            claims::SAFE_PAIN_STRUCT_001,
        ),
        PainVerdict::StructuralStop => ("stop training", claims::SAFE_PAIN_STRUCT_001),
        PainVerdict::TendonDefer => (
            "still flaring: see a doctor or physio",
            claims::SAFE_TENDON_001,
        ),
        PainVerdict::TendonReduce => ("flaring: easier session", claims::SAFE_TENDON_001),
        PainVerdict::TendonModify => ("mild: keep it light", claims::SAFE_TENDON_001),
        // Normal training discomfort judges nothing: no claim.
        PainVerdict::Doms => return ("normal muscle soreness".into(), None),
        PainVerdict::OtherDefer => ("see a doctor or physio first", claims::SAFE_PAIN_001),
    };
    (s.into(), Some(claim))
}

/// autoreg-045/046 neck check.
fn illness_label(v: f64) -> Label {
    match IllnessSeverity::from_value(v) {
        IllnessSeverity::BelowNeckOrFever => {
            ("flu or fever: rest".into(), Some(claims::ILLNESS_NECK_001))
        }
        IllnessSeverity::AboveNeck => (
            "head cold: easy only".into(),
            Some(claims::ILLNESS_NECK_001),
        ),
        IllnessSeverity::None => ("clear".into(), None),
    }
}

/// A medical-referral red flag (`v > 0` = present) or an all-clear.
fn red_flag_label(v: f64, state: &'static str, claim: ClaimId) -> Label {
    if v > 0.0 {
        (state.into(), Some(claim))
    } else {
        ("clear".into(), None)
    }
}
