//! Set-level and next-load prescriptions from scalar inputs (File 06): a single
//! lift's within-session observations (AMRAP reps, first-set RPE,
//! reference-load velocity) rather than the daily readiness vector, plus the
//! overtraining-continuum deferrals (File 08 safety-041/042; autoreg-042). All
//! pure and deterministic; thresholds verbatim from File 06.

use super::rules::{
    HRV_ESCALATE_STREAK, HRV_MIN_WEEKLY_READINGS, HRV_SWC_UPPER_Z, WELLNESS_DOWNGRADE_STREAK,
    WELLNESS_MULTIDAY_STREAK,
};
use crate::evidence::{self, claims};
use crate::schema::{Adjustment, Recommended};

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
    evidence::graded(v, claims::AUTOREG_VBT_001)
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
    evidence::graded(a, claims::AUTOREG_SETVOL_001)
}

/// RPE-stop: cut remaining sets once the target RPE is reached before the
/// planned set count (File 06 autoreg-013). `true` = stop now.
pub fn rpe_stop_reached(rpe_actual: f64, rpe_target: f64) -> bool {
    rpe_actual >= rpe_target
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

/// APRE next-load adjustment (kg) from the AMRAP-set rep count (File 06
/// autoreg-015…021; Mann 2010). Returns the `(low, high)` kg delta to apply to
/// the next set/session; negatives reduce load. `[Moderate]` (AUTOREG-APRE-001).
///
/// UNITS: the whole engine domain is kg; Mann 2010's source tables are stated in
/// lb increments (±5 / ±10 / ±15 lb). Those are converted to plate-rounded kg
/// (5 lb ≈ 2.5 kg, 10 lb ≈ 5 kg, 15 lb ≈ 7.5 kg) so the adjustment lands on
/// standard kg plate steps. The original lb values are noted here for traceability.
///
/// The Mann 2010 tables are stated for a rep window centred on
/// each scheme's target RM. The `0..=N` low arms and the open-ended `_` high arm
/// EXTEND the table's own lowest/highest published rows to cover reps below/above
/// that window (e.g. an APRE-6 AMRAP of 0–2 reps or 13+ reps). This is a
/// conservative saturation of the KB band's endpoints, not an invented new
/// band, so far-out rep counts clamp to the nearest published adjustment rather
/// than extrapolating a fabricated larger jump/cut.
pub fn apre_load_adjustment_kg(scheme: ApreScheme, reps: u8) -> Recommended<(f64, f64)> {
    // kg values are the Mann 2010 lb increments plate-rounded:
    // −10/−5/0/+5/+10/+15 lb → −5.0/−2.5/0/+2.5/+5.0/+7.5 kg.
    let range = match scheme {
        ApreScheme::Apre6 => match reps {
            0..=2 => (-5.0, -2.5),
            3..=4 => (-2.5, 0.0),
            5..=7 => (0.0, 0.0),
            8..=12 => (2.5, 5.0),
            _ => (5.0, 7.5),
        },
        ApreScheme::Apre10 => match reps {
            0..=6 => (-5.0, -2.5),
            7..=8 => (-2.5, 0.0),
            9..=11 => (0.0, 0.0),
            12..=16 => (2.5, 5.0),
            _ => (5.0, 7.5),
        },
        ApreScheme::Apre3 => match reps {
            0..=2 => (-5.0, -2.5),
            3..=4 => (0.0, 0.0),
            5..=6 => (2.5, 5.0),
            _ => (5.0, 7.5),
        },
    };
    evidence::graded(range, claims::AUTOREG_APRE_001)
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
pub fn deload_from_rpe_load_gap(sessions_ge_7pct_below: u8) -> Option<Recommended<Adjustment>> {
    (sessions_ge_7pct_below >= 2)
        .then(|| evidence::graded(standard_deload(), claims::AUTOREG_DELOAD_001))
}

/// Deload when session RPE creeps +1 across the week at the same loads AND the
/// wellness composite z ≤ −1 for ≥3 days (File 06 autoreg-024).
pub fn deload_from_rpe_creep_and_wellness(
    rpe_creep_plus_one: bool,
    wellness_z_le_neg1_days: u8,
) -> Option<Recommended<Adjustment>> {
    (rpe_creep_plus_one && wellness_z_le_neg1_days >= WELLNESS_MULTIDAY_STREAK)
        .then(|| evidence::graded(standard_deload(), claims::AUTOREG_DELOAD_001))
}

/// Deload when reference-load velocity is down more than
/// `VBT_WEEKLY_DROP_DELOAD_M_S` across the week (File 06 autoreg-026).
pub fn deload_from_velocity_drop(weekly_mcv_drop_m_s: f64) -> Option<Recommended<Adjustment>> {
    (weekly_mcv_drop_m_s > VBT_WEEKLY_DROP_DELOAD_M_S)
        .then(|| evidence::graded(standard_deload(), claims::AUTOREG_DELOAD_VBT_001))
}

/// Reduce weekly volume 20–30% (defer hard work) after two failed key sessions
/// in a week (File 06 autoreg-036). Encoded as a load-neutral volume deload
/// (25% midpoint).
pub fn deload_from_failed_sessions(failed_key_sessions: u8) -> Option<Recommended<Adjustment>> {
    (failed_key_sessions >= 2).then(|| {
        evidence::graded(
            Adjustment::Deload {
                volume_reduction_pct: 25.0,
                load_reduction_pct: 0.0,
                weeks: 1,
            },
            claims::AUTOREG_DELOAD_001,
        )
    })
}

/// Interval-pace autoregulation (File 06 autoreg-031): when ≥2 reps land at
/// RPE ≥ target+1 or above the HR cap, cut the remaining-rep pace target ~2–4%
/// (returns the fractional pace reduction to apply, else `None`).
pub fn interval_pace_autoreg(reps_over_target: u8) -> Option<Recommended<f64>> {
    // Cite the autoreg-031 interval-pace rule itself, not RUN-VDOT-001 (the
    // VDOT *estimator* claim, unrelated to this within-session pace autoreg).
    (reps_over_target >= 2).then(|| evidence::graded(0.03, claims::AUTOREG_INTERVAL_PACE_001))
}

/// Easy-day pace is governed by the HR cap, not pace (File 06 autoreg-033):
/// if the runner cannot hold the prescribed easy pace under the HR cap, slow
/// the pace. `true` = slow the easy pace.
pub fn slow_easy_pace_if_over_cap(can_hold_pace_under_cap: bool) -> Recommended<bool> {
    // Cite the autoreg-033 easy-day HR-cap rule, not RUN-VDOT-001.
    evidence::graded(!can_hold_pace_under_cap, claims::AUTOREG_EASY_CAP_001)
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
    let s = if hrv_today || recent_hrv_count as usize >= HRV_MIN_WEEKLY_READINGS {
        AutoregSource::HrvRolling
    } else if has_subjective {
        AutoregSource::SubjectivePlusPerformance
    } else {
        AutoregSource::PerformanceOnlyHold
    };
    evidence::graded(s, claims::AUTOREG_FALLBACK_001)
}

/// Suspend HRV gating once ≥2 of the last 3 readings were flagged unreliable
/// (File 06 autoreg-050); use subjective + performance until a clean baseline
/// returns. `true` = suspend HRV gating.
pub fn suspend_hrv_gating(unreliable_in_last_three: u8) -> bool {
    unreliable_in_last_three >= 2
}

/// autoreg-034: a multi-day lnRMSSD suppression streak (≥3–4 consecutive days
/// below the SWC band) warrants inserting a recovery day / easy block, beyond the
/// 7-day rolling verdict in `hrv_downgrade`. Fires at ≥3 days. HRV-001.
pub fn hrv_suppressed_recovery_day(consecutive_suppressed_days: u8) -> Recommended<bool> {
    evidence::graded(
        consecutive_suppressed_days >= HRV_ESCALATE_STREAK,
        claims::HRV_001,
    )
}

/// autoreg-035: multi-day suppressed wellness combined with a rising resting HR
/// trend calls for 1–3 easy days or cross-training. Fires when wellness has been
/// suppressed ≥2 days AND RHR is trending up. AUTOREG-WELLNESS-RHR-001
/// (Moderate, File 06 §3C, not the Strong OTS deferral).
pub fn wellness_rhr_multiday_easy(
    wellness_suppressed_days: u8,
    rhr_rising: bool,
) -> Recommended<bool> {
    evidence::graded(
        wellness_suppressed_days >= WELLNESS_DOWNGRADE_STREAK && rhr_rising,
        claims::AUTOREG_WELLNESS_RHR_001,
    )
}

/// autoreg-029: parasympathetic-saturation guard. `true` = do NOT auto-add
/// load; hold and weigh with wellness. Fires when the 7-day rolling lnRMSSD
/// z-score sits above the SWC upper limit (baseline + 0.5 SD) *during a
/// high-load block*, unusually high HRV under heavy loading can be
/// saturation, not readiness. AUTOREG-HRV-SAT-001 (Moderate, Plews).
pub(super) fn hrv_saturation_hold(hrv_z: f64, high_load_block: bool) -> Recommended<bool> {
    evidence::graded(
        high_load_block && hrv_z > HRV_SWC_UPPER_Z,
        claims::AUTOREG_HRV_SAT_001,
    )
}

/// autoreg-028 second trigger: a SINGLE-DAY lnRMSSD reading more than 1 SD
/// below baseline combined with a ≥2-day downtrend → downgrade hard→easy/Z2
/// (or rest), even before the 7-day rolling average clears the SWC lower band
/// (the first trigger, `hrv_downgrade` via the readiness vector). HRV-001
/// (Moderate, Kiviniemi 2007; Javaloyes 2019).
pub fn hrv_single_day_downgrade(
    single_day_z: f64,
    downtrend_days: u8,
) -> Option<Recommended<Adjustment>> {
    (single_day_z < -1.0 && downtrend_days >= 2)
        .then(|| evidence::graded(Adjustment::DowngradeSession, claims::HRV_001))
}

/// autoreg-025: the at/above-MRV sign cluster (joint aches, performance stall,
/// sleep disruption, motivation drop) → deload. The KB defines the cluster
/// qualitatively, NO numeric sign count exists (HARD RULE 1: none invented),
/// so the caller supplies the judgment that the cluster is present. Deload
/// magnitude per the rule's RP-framework mapping: volume −50%, load −10%,
/// 1 week. AUTOREG-MRV-001 (ExpertOpinion, Israetel et al. 2021).
pub fn mrv_signs_deload(sign_cluster_present: bool) -> Option<Recommended<Adjustment>> {
    sign_cluster_present.then(|| evidence::graded(standard_deload(), claims::AUTOREG_MRV_001))
}

/// APRE next-load adjustment (kg) with the autoreg-019 small-lifter cap applied.
///
/// autoreg-019 (verbatim, in the source's lb): reps 13+ on APRE-6 → "+10 to 15
/// lb (cap as % of load for smaller/weaker lifters, 15 lb on 100 lb = +15%)".
/// The KB states no separate %-cap constant; its worked example fixes the top
/// band at +15% (15 lb on 100 lb). Converted to kg the top band is +7.5 kg, so
/// the same +15% is reproduced at a 50 kg reference load (7.5/50 = 15%): each
/// positive bound is capped at `min(band_kg, current_load_kg × band_kg / 50)`.
/// Because both the band and the reference load are the kg analogues of the lb
/// figures, every bound's PERCENT cap is identical to the source's (5 kg/50 =
/// 10%, matching 10 lb/100). At the reference load this equals the flat
/// band; below it the jump shrinks proportionally; above it the flat band already
/// IS the smaller value and governs unchanged. Reductions (negative bounds) are
/// never capped, capping a load *cut* would weaken a fatigue response.
/// AUTOREG-APRE-001 (Moderate - Mann 2010).
pub fn apre_load_adjustment_capped_kg(
    scheme: ApreScheme,
    reps: u8,
    current_load_kg: f64,
) -> Recommended<(f64, f64)> {
    let (lo, hi) = apre_load_adjustment_kg(scheme, reps).value;
    // A non-positive current load has no meaningful proportional cap:
    // `current_load_kg × band/50` would flip a positive jump negative (a
    // fabricated load *cut*). Reject it: fall back to the uncapped KB flat band.
    if current_load_kg <= 0.0 || current_load_kg.is_nan() {
        return evidence::graded((lo, hi), claims::AUTOREG_APRE_001);
    }
    let cap = |bound_kg: f64| -> f64 {
        if bound_kg > 0.0 {
            bound_kg.min(current_load_kg * bound_kg / 50.0)
        } else {
            bound_kg
        }
    };
    evidence::graded((cap(lo), cap(hi)), claims::AUTOREG_APRE_001)
}

/// autoreg-032: pace at target HR improved by at least a smallest-worthwhile
/// amount, sustained over 2–3 weeks → re-test / raise threshold pace. `true` =
/// schedule the re-test. The SWC magnitude itself is unstated in the KB, so
/// the caller judges "improved ≥ SWC"; the ≥2-week duration bound is the
/// stated number. AUTOREG-PACE-RETEST-001 (Moderate, File 06 §3B).
pub fn threshold_retest_due(improved_ge_swc: bool, weeks_sustained: u8) -> Recommended<bool> {
    evidence::graded(
        improved_ge_swc && weeks_sustained >= 2,
        claims::AUTOREG_PACE_RETEST_001,
    )
}

// ---------------------------------------------------------------------------
// Overtraining continuum (File 08 safety-041/042 Table 4.4; File 06 autoreg-042)
// ---------------------------------------------------------------------------

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
        evidence::graded(Adjustment::Defer {
            reason: "Your performance has dropped for no clear reason for 2+ weeks, with tiredness, low mood or poor sleep, even after a lighter week. Rest, and see a professional to rule out overreaching, overtraining, under-eating (RED-S) or another medical cause."
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
) -> Option<Recommended<Adjustment>> {
    (decrement_weeks >= 2 && suppressed_wellness_domains >= 2).then(|| {
        evidence::graded(Adjustment::Defer {
            reason: "Possible overreaching: your performance has dropped for no clear reason for 2+ weeks, and two or more of your check-in scores are low. Take a recovery break, and see a professional if it doesn't improve."
                .into(),
        },
        claims::AUTOREG_NFOR_001,)
    })
}
