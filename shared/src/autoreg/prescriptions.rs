//! Set-level and next-load prescriptions from scalar inputs (File 06): a single
//! lift's within-session observations (AMRAP reps, first-set RPE,
//! reference-load velocity) rather than the daily readiness vector, plus the
//! overtraining-continuum deferrals (File 08 safety-041/042; autoreg-042). All
//! pure and deterministic; thresholds verbatim from File 06.

use super::rules::HRV_ESCALATE_STREAK;
use crate::evidence::claims;
use crate::schema::{Adjustment, AdjustmentCall, AdjustmentKind, DeloadTrigger, Recommended};

/// Reliability band of a reference-load mean concentric velocity, m/s
/// (autoreg-008/009, AUTOREG-VBT-001; Weakley/Pearson 2020): a daily change
/// inside ±this is noise.
pub(crate) const VBT_RELIABILITY_M_S: f64 = 0.06;

/// Weekly reference-load velocity drop that deloads, m/s (autoreg-026,
/// AUTOREG-DELOAD-VBT-001): the lower bound of the 0.06–0.10 m/s band.
pub(crate) const VBT_WEEKLY_DROP_DELOAD_M_S: f64 = 0.06;

/// Load gap below plan, %, at which planned RPE counts as only hit at a lower
/// load (autoreg-023, AUTOREG-DELOAD-001).
pub(crate) const RPE_LOAD_GAP_DELOAD_PCT: f64 = 7.0;

/// Sessions in a week with that load gap that deload (autoreg-023 "≥2
/// sessions").
pub(crate) const RPE_LOAD_GAP_DELOAD_SESSIONS: u8 = 2;

/// Daily-1RM readiness verdict from mean concentric velocity at a reference
/// load vs. baseline (File 06 autoreg-008/009; VBT), banded by
/// `VBT_RELIABILITY_M_S`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VbtReadiness {
    /// MCV above the band → daily 1RM up → raise working loads.
    IncreaseLoad,
    /// Within the reliability band → hold planned loads.
    Hold,
    /// MCV below the band → daily 1RM down → reduce working loads.
    ReduceLoad,
}

/// Map a reference-load velocity delta (m/s, today − baseline) to a daily-1RM
/// readiness verdict (File 06 autoreg-008/009; AUTOREG-VBT-001, Strong -
/// Banyard 2017; Weakley/Pearson 2020).
pub fn vbt_daily_readiness(mcv_delta_m_s: f64) -> Recommended<VbtReadiness> {
    let v = if mcv_delta_m_s > VBT_RELIABILITY_M_S {
        VbtReadiness::IncreaseLoad
    } else if mcv_delta_m_s < -VBT_RELIABILITY_M_S {
        VbtReadiness::ReduceLoad
    } else {
        VbtReadiness::Hold
    };
    Recommended::new(v, claims::AUTOREG_VBT_001)
}

/// First-set RPE delta at/below which a strong first set adds a set
/// (autoreg-011: "RPE ≤ target−1").
const ADD_SET_MAX_RPE_DELTA: f64 = -1.0;

/// First-set RPE delta at/above which the last planned set drops
/// (autoreg-012: "RPE ≥ target+1").
const DROP_SET_MIN_RPE_DELTA: f64 = 1.0;

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
    let a = if first_set_reps_met && rpe_delta <= ADD_SET_MAX_RPE_DELTA && wellness_normal {
        SetVolumeAction::AddSet
    } else if !first_set_reps_met || rpe_delta >= DROP_SET_MIN_RPE_DELTA {
        SetVolumeAction::DropLastSet
    } else {
        SetVolumeAction::HoldPlanned
    };
    Recommended::new(a, claims::AUTOREG_SETVOL_001)
}

/// RPE-stop: cut remaining sets once the target RPE is reached before the
/// planned set count (File 06 autoreg-013). `true` = stop now.
pub fn rpe_stop_reached(rpe_actual: f64, rpe_target: f64) -> bool {
    rpe_actual >= rpe_target
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

/// Deload when planned RPE is only hit at loads ≥`RPE_LOAD_GAP_DELOAD_PCT`
/// below plan for ≥2 sessions (File 06 autoreg-023). `None` when the trigger
/// has not fired.
pub fn deload_from_rpe_load_gap(sessions_ge_7pct_below: u8) -> Option<AdjustmentCall> {
    (sessions_ge_7pct_below >= RPE_LOAD_GAP_DELOAD_SESSIONS)
        .then(|| weekly_deload(DeloadTrigger::RpeLoadGap, claims::AUTOREG_DELOAD_001))
}

/// Days of wellness z ≤ −1 that, with an RPE creep, deload (autoreg-024 "≥3
/// days").
pub(crate) const RPE_CREEP_WELLNESS_LOW_DAYS: u8 = 3;

/// Deload when session RPE creeps +1 across the week at the same loads AND the
/// wellness composite z ≤ −1 for ≥3 days (File 06 autoreg-024).
pub fn deload_from_rpe_creep_and_wellness(
    rpe_creep_plus_one: bool,
    wellness_z_le_neg1_days: u8,
) -> Option<AdjustmentCall> {
    (rpe_creep_plus_one && wellness_z_le_neg1_days >= RPE_CREEP_WELLNESS_LOW_DAYS)
        .then(|| weekly_deload(DeloadTrigger::RpeCreep, claims::AUTOREG_DELOAD_001))
}

/// Deload when reference-load velocity is down more than
/// `VBT_WEEKLY_DROP_DELOAD_M_S` across the week (File 06 autoreg-026).
pub fn deload_from_velocity_drop(weekly_mcv_drop_m_s: f64) -> Option<AdjustmentCall> {
    (weekly_mcv_drop_m_s > VBT_WEEKLY_DROP_DELOAD_M_S)
        .then(|| weekly_deload(DeloadTrigger::VelocityDrop, claims::AUTOREG_DELOAD_VBT_001))
}

/// Failed key sessions in a week that cut weekly volume (autoreg-036).
pub(crate) const FAILED_KEY_SESSIONS_DELOAD: u8 = 2;

/// Reduce weekly volume 20–30% (defer hard work) after two failed key sessions
/// in a week (File 06 autoreg-036). Encoded as a load-neutral volume deload
/// (25% midpoint).
pub fn deload_from_failed_sessions(failed_key_sessions: u8) -> Option<AdjustmentCall> {
    (failed_key_sessions >= FAILED_KEY_SESSIONS_DELOAD).then(|| {
        AdjustmentKind::WeeklyDeload(DeloadTrigger::FailedSessions).call(
            Adjustment::Deload {
                volume_reduction_pct: 25.0,
                load_reduction_pct: 0.0,
                weeks: 1,
            },
            claims::AUTOREG_DELOAD_001,
        )
    })
}

/// Interval reps over target RPE / HR cap that slow the remaining reps
/// (autoreg-031 "≥2 reps").
pub(crate) const INTERVAL_REPS_OVER_TARGET_CUT: u8 = 2;

/// Interval-pace autoregulation (File 06 autoreg-031): when ≥2 reps land at
/// RPE ≥ target+1 or above the HR cap, cut the remaining-rep pace target ~2–4%
/// (returns the fractional pace reduction to apply, else `None`).
pub fn interval_pace_autoreg(reps_over_target: u8) -> Option<Recommended<f64>> {
    (reps_over_target >= INTERVAL_REPS_OVER_TARGET_CUT)
        .then(|| Recommended::new(0.03, claims::AUTOREG_INTERVAL_PACE_001))
}

/// Easy-day pace is governed by the HR cap, not pace (File 06 autoreg-033):
/// if the runner cannot hold the prescribed easy pace under the HR cap, slow
/// the pace. `true` = slow the easy pace.
pub fn slow_easy_pace_if_over_cap(can_hold_pace_under_cap: bool) -> Recommended<bool> {
    Recommended::new(!can_hold_pace_under_cap, claims::AUTOREG_EASY_CAP_001)
}

/// Minimum lnRMSSD readings inside the trailing rolling window before the HRV
/// channel emits a weekly baseline (autoreg-047: "need ≥4–5 readings/week for a
/// valid weekly baseline"). Below this the window is too sparse to trust.
pub(super) const HRV_MIN_WEEKLY_READINGS: usize = 4;

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
    let s = if hrv_today || recent_hrv_count as usize >= HRV_MIN_WEEKLY_READINGS {
        AutoregSource::HrvRolling
    } else if has_subjective {
        AutoregSource::SubjectivePlusPerformance
    } else {
        AutoregSource::PerformanceOnlyHold
    };
    Recommended::new(s, claims::AUTOREG_FALLBACK_001)
}

/// Most recent readings autoreg-050 inspects before suspending HRV gating
/// ("≥2 of the last 3 readings flagged").
pub(crate) const HRV_QC_LOOKBACK: usize = 3;

/// Unreliable readings among the last [`HRV_QC_LOOKBACK`] at which HRV gating
/// is suspended (autoreg-050).
pub(crate) const HRV_UNRELIABLE_SUSPEND: u8 = 2;

/// Suspend HRV gating once ≥2 of the last 3 readings were flagged unreliable
/// (File 06 autoreg-050); use subjective + performance until a clean baseline
/// returns. `true` = suspend HRV gating.
pub fn suspend_hrv_gating(unreliable_in_last_three: u8) -> bool {
    unreliable_in_last_three >= HRV_UNRELIABLE_SUSPEND
}

/// autoreg-034: a multi-day lnRMSSD suppression streak (≥3–4 consecutive days
/// below the SWC band) warrants inserting a recovery day / easy block, beyond the
/// 7-day rolling verdict in `hrv_downgrade`. Fires at ≥3 days. HRV-001.
pub fn hrv_suppressed_recovery_day(consecutive_suppressed_days: u8) -> Recommended<bool> {
    Recommended::new(
        consecutive_suppressed_days >= HRV_ESCALATE_STREAK,
        claims::HRV_001,
    )
}

/// Days of suppressed wellness that, with a rising resting HR, call for easy
/// days (autoreg-035 "≥2 days").
pub(crate) const WELLNESS_RHR_EASY_DAYS: u8 = 2;

/// autoreg-035: multi-day suppressed wellness combined with a rising resting HR
/// trend calls for 1–3 easy days or cross-training. Fires when wellness has been
/// suppressed ≥2 days AND RHR is trending up. AUTOREG-WELLNESS-RHR-001
/// (Moderate, File 06 §3C, not the Strong OTS deferral).
pub fn wellness_rhr_multiday_easy(
    wellness_suppressed_days: u8,
    rhr_rising: bool,
) -> Recommended<bool> {
    Recommended::new(
        wellness_suppressed_days >= WELLNESS_RHR_EASY_DAYS && rhr_rising,
        claims::AUTOREG_WELLNESS_RHR_001,
    )
}

/// autoreg-028 second trigger: a SINGLE-DAY lnRMSSD reading more than 1 SD
/// below baseline combined with a ≥2-day downtrend → downgrade hard→easy/Z2
/// (or rest), even before the 7-day rolling average clears the SWC lower band
/// (the first trigger, `hrv_downgrade` via the readiness vector). HRV-001
/// (Moderate, Kiviniemi 2007; Javaloyes 2019).
pub fn hrv_single_day_downgrade(single_day_z: f64, downtrend_days: u8) -> Option<AdjustmentCall> {
    (single_day_z < -1.0 && downtrend_days >= 2)
        .then(|| AdjustmentKind::HrvLower.call(Adjustment::DowngradeSession, claims::HRV_001))
}

/// autoreg-025: the at/above-MRV sign cluster (joint aches, performance stall,
/// sleep disruption, motivation drop) → deload. The KB defines the cluster
/// qualitatively, NO numeric sign count exists (HARD RULE 1: none invented),
/// so the caller supplies the judgment that the cluster is present. Deload
/// magnitude per the rule's RP-framework mapping: volume −50%, load −10%,
/// 1 week. AUTOREG-MRV-001 (ExpertOpinion, Israetel et al. 2021).
pub fn mrv_signs_deload(sign_cluster_present: bool) -> Option<AdjustmentCall> {
    sign_cluster_present.then(|| weekly_deload(DeloadTrigger::MrvSigns, claims::AUTOREG_MRV_001))
}

/// The standard deload fired by a weekly `trigger`.
fn weekly_deload(trigger: DeloadTrigger, claim: crate::evidence::ClaimId) -> AdjustmentCall {
    AdjustmentKind::WeeklyDeload(trigger).call(standard_deload(), claim)
}

/// Weeks of improved pace at target HR that schedule a threshold re-test
/// (autoreg-032's stated ≥2-week bound).
pub(crate) const PACE_RETEST_WEEKS: u8 = 2;

/// autoreg-032: pace at target HR improved by at least a smallest-worthwhile
/// amount, sustained over 2–3 weeks → re-test / raise threshold pace. `true` =
/// schedule the re-test. The SWC magnitude itself is unstated in the KB, so
/// the caller judges "improved ≥ SWC"; the ≥2-week duration bound is the
/// stated number. AUTOREG-PACE-RETEST-001 (Moderate, File 06 §3B).
pub fn threshold_retest_due(improved_ge_swc: bool, weeks_sustained: u8) -> Recommended<bool> {
    Recommended::new(
        improved_ge_swc && weeks_sustained >= PACE_RETEST_WEEKS,
        claims::AUTOREG_PACE_RETEST_001,
    )
}

// ---------------------------------------------------------------------------
// Overtraining continuum (File 08 safety-041/042 Table 4.4; File 06 autoreg-042)
// ---------------------------------------------------------------------------

/// Weeks of unexplained performance decline at which the overtraining
/// referrals fire (File 08 safety-042, SAFE-OTS-001; File 06 autoreg-042,
/// AUTOREG-NFOR-001: both "≥2 weeks").
pub(crate) const NFOR_DECLINE_WEEKS: u8 = 2;

/// File 08 safety-042: ≥2 weeks of unexplained performance decline plus
/// elevated fatigue/mood/sleep disturbance DESPITE a deload → REST and DEFER
/// (rule out NFOR/OTS/RED-S/medical cause). Monitoring inputs are trend flags,
/// not diagnostics. SAFE-OTS-001 (Strong, Meeusen 2013, safety-critical).
pub fn unexplained_decline_rest_defer(
    decline_weeks: u8,
    wellness_disturbed: bool,
    despite_deload: bool,
) -> Option<AdjustmentCall> {
    (decline_weeks >= NFOR_DECLINE_WEEKS && wellness_disturbed && despite_deload).then(|| {
        AdjustmentKind::OvertrainingReferral.call(Adjustment::Defer {
            reason: "Performance down for 2+ weeks with no clear cause, plus tiredness, low mood or poor sleep, even after a lighter week. Rest, and see a doctor to rule out overtraining, under-eating (RED-S) or another medical cause."
                .into(),
        },
        claims::SAFE_OTS_001,)
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
) -> Option<AdjustmentCall> {
    (decrement_weeks >= NFOR_DECLINE_WEEKS && suppressed_wellness_domains >= 2).then(|| {
        AdjustmentKind::NforReferral.call(Adjustment::Defer {
            reason: "Possible overreaching: performance down for 2+ weeks with no clear cause, and 2+ check-in scores low. Take a recovery break; see a doctor if it doesn't improve."
                .into(),
        },
        claims::AUTOREG_NFOR_001,)
    })
}
