//! Autoregulation core: readiness inputs → evidence-cited adjustments.
//!
//! Pure, deterministic logic derived from knowledge-base File 06
//! (`knowledge-base/extracted/06-autoregulation.md`): readiness-signal
//! definitions, the ~50 IF→THEN adjustment rules with verbatim thresholds, and
//! the §5 six-tier safety ladder. No IO, no `SystemTime`, no randomness.
//!
//! Every emitted recommendation is a [`Recommended<Adjustment>`] whose
//! [`ClaimId`](crate::evidence::ClaimId) resolves to its evidence row in the compile-time claim registry
//! (`crate::evidence`). Safety-critical stops (pain, fever/illness, RHR +10 bpm)
//! dominate all optimization triggers.
//!
//! [`Recommended<Adjustment>`]: crate::schema::Recommended

mod derive;
mod hold;
mod pain;
mod perf;
mod prescriptions;
mod rules;
mod signals;
#[cfg(test)]
pub(crate) mod tests;

pub use derive::{BaselineKind, BaselineStatus, DerivedReadiness, derive_readiness};
pub(crate) use derive::{LONG_BASELINE_WINDOW_DAYS, has_hrv, has_rhr, has_wellness};
pub use hold::{ReturnKind, TimedInput, last_hold_resolution, return_adjustment};
pub use pain::{
    PainRegion, item_spared_by_pain, pain_blocks_progression, pain_hold_active, pain_region,
    worst_active_pain,
};
pub use perf::{MovementPerf, PerfSet, derive_performance};
pub use prescriptions::{
    ApreScheme, AutoregSource, SetVolumeAction, VbtReadiness, apre_load_adjustment_capped_kg,
    apre_load_adjustment_kg, autoreg_source, deload_from_failed_sessions,
    deload_from_rpe_creep_and_wellness, deload_from_rpe_load_gap, deload_from_velocity_drop,
    hrv_single_day_downgrade, hrv_suppressed_recovery_day, interval_pace_autoreg, mrv_signs_deload,
    nfor_cluster_defer, rpe_stop_reached, set_volume_action, slow_easy_pace_if_over_cap,
    suspend_hrv_gating, threshold_retest_due, unexplained_decline_rest_defer, vbt_daily_readiness,
    wellness_rhr_multiday_easy,
};
pub(crate) use prescriptions::{
    RPE_LOAD_GAP_DELOAD_PCT, VBT_RELIABILITY_M_S, VBT_WEEKLY_DROP_DELOAD_M_S,
};
pub use rules::{
    RhrNoise, RuleContext, adjustments_in, bone_stress_referral, failed_set_stop,
    first_set_rpe_adjust, hrv_below_swc, illness_active, illness_plan_recs, illness_stop,
    increase_hold, reds_referral, resolve_safety_in, rhr_neck_check_due,
};
pub use signals::{ALL_SIGNALS, SignalGroup, SignalState, signal_group, signal_states_in};

use crate::schema::{ReadinessInput, ReadinessSignal};

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
