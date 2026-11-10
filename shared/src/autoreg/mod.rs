//! Autoregulation core: readiness inputs → evidence-cited adjustments.
//!
//! Pure logic derived from knowledge-base File 06
//! (`knowledge-base/extracted/06-autoregulation.md`): readiness-signal
//! definitions, the ~50 IF→THEN adjustment rules with verbatim thresholds, and
//! the §5 six-tier safety ladder.
//!
//! Every emitted recommendation is a [`Recommended<Adjustment>`] whose
//! [`ClaimId`](crate::evidence::ClaimId) resolves to its evidence row in the compile-time claim registry
//! (`crate::evidence`). Safety-critical stops (pain, fever/illness, a
//! corroborated resting-HR elevation) dominate all optimization triggers.
//!
//! [`Recommended<Adjustment>`]: crate::schema::Recommended

mod derive;
mod hold;
mod inputs;
mod pain;
mod perf;
mod prescriptions;
mod rules;
mod signals;
#[cfg(test)]
pub(crate) mod tests;

pub use derive::{BaselineKind, BaselineStatus, DerivedReadiness, derive_readiness};
pub(crate) use derive::{HRV_ROLLING_DAYS, checkin_days_in_reach};
pub use hold::{ReturnKind, TimedInput, last_hold_resolution, return_adjustment};
pub use pain::{
    PainRegion, item_spared_by_pain, pain_blocks_progression, pain_blocks_training,
    pain_hold_active, pain_region, worst_active_pain,
};
pub(crate) use perf::{E1RM_NOISE_FRAC, E1RM_REDUCE_RATIO};
pub use perf::{MovementPerf, PerfSet, derive_performance};
pub use prescriptions::{
    AutoregSource, SetVolumeAction, VbtReadiness, autoreg_source, deload_from_failed_sessions,
    deload_from_rpe_creep_and_wellness, deload_from_rpe_load_gap, deload_from_velocity_drop,
    hrv_single_day_downgrade, hrv_suppressed_recovery_day, interval_pace_autoreg, mrv_signs_deload,
    nfor_cluster_defer, rpe_stop_reached, set_volume_action, slow_easy_pace_if_over_cap,
    suspend_hrv_gating, threshold_retest_due, unexplained_decline_rest_defer, vbt_daily_readiness,
    wellness_rhr_multiday_easy,
};
pub(crate) use prescriptions::{
    FAILED_KEY_SESSIONS_DELOAD, HRV_QC_LOOKBACK, HRV_UNRELIABLE_SUSPEND,
    INTERVAL_REPS_OVER_TARGET_CUT, PACE_RETEST_WEEKS, RPE_CREEP_WELLNESS_LOW_DAYS,
    RPE_LOAD_GAP_DELOAD_PCT, RPE_LOAD_GAP_DELOAD_SESSIONS, VBT_RELIABILITY_M_S,
    VBT_WEEKLY_DROP_DELOAD_M_S, WELLNESS_RHR_EASY_DAYS,
};
pub(crate) use rules::HRV_ESCALATE_STREAK;
pub use rules::{
    RhrNoise, RuleContext, adjustments_in, bone_stress_referral, failed_set_stop,
    first_set_rpe_adjust, hrv_below_swc, illness_active, illness_plan_recs, illness_stop,
    increase_hold, reds_defer, reds_referral, resolve_safety_in, rhr_neck_check_due,
};
pub use signals::{ALL_SIGNALS, SignalGroup, SignalState, signal_group, signal_states_in};
