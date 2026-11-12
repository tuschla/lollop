//! The Rust constants the shells read by name: the live recorder's GPS limits,
//! the setup's input bounds and caps, the thresholds shell copy quotes, and
//! the muscles the hypertrophy landmarks cover. `shared/wiregen` generates the
//! Kotlin `CoreConstants` and `HypertrophyMuscle` from them.

use crate::wire::{ConstValue, Constant};

/// The muscle names `hypertrophy::landmarks_for` knows, in table order.
pub fn hypertrophy_muscles() -> impl Iterator<Item = &'static str> {
    crate::hypertrophy::LANDMARKS.iter().map(|l| l.muscle)
}

macro_rules! constants {
    ($($(#[doc = $doc:literal])* $path:path as $kind:ident;)*) => {
        pub fn constants() -> Vec<Constant> {
            vec![$(Constant {
                name: {
                    let path = stringify!($path);
                    match path.rfind(':') {
                        Some(i) => path.split_at(i + 1).1,
                        None => path,
                    }
                },
                docs: concat!($($doc, "\n"),*),
                value: ConstValue::$kind($path.into()),
            }),*]
        }
    };
}

constants! {
    /// Longest single activity `LogActivity` stores, minutes.
    crate::event::entry::ACTIVITY_MAX_MINUTES as Float;
    /// Lowest plausible resting HR, bpm (the Karvonen anchor's band).
    crate::running::RESTING_HR_MIN_BPM as Float;
    /// Highest plausible resting HR, bpm (the Karvonen anchor's band).
    crate::running::RESTING_HR_MAX_BPM as Float;
    /// Lowest measured HRmax the core trusts, bpm.
    crate::running::MEASURED_HR_MAX_MIN_BPM as Float;
    /// Highest measured HRmax the core trusts, bpm.
    crate::running::MEASURED_HR_MAX_MAX_BPM as Float;
    /// Youngest age the Tanaka HRmax estimate covers, years.
    crate::running::TANAKA_AGE_MIN_YEARS as Float;
    /// Oldest age the Tanaka HRmax estimate covers, years.
    crate::running::TANAKA_AGE_MAX_YEARS as Float;
    /// Lowest average heart rate a logged run keeps, bpm.
    crate::running::AVG_HR_MIN_BPM as Float;
    /// Highest average heart rate a logged run keeps, bpm.
    crate::running::AVG_HR_MAX_BPM as Float;
    /// The highest average heart rate a logged run states, % of HRmax.
    crate::running::HR_PCT_MAX_CEILING as Float;
    /// Fewest lift days a week a chosen count is held to.
    crate::profile::LIFT_DAYS_MIN as Int;
    /// Most lift days a week a chosen count is held to.
    crate::profile::LIFT_DAYS_MAX as Int;
    /// Lift days a lifting-only profile that chose none trains.
    crate::profile::LIFT_ONLY_DEFAULT_LIFT_DAYS as Int;
    /// Lift days a lifting-and-running profile that chose none trains.
    crate::profile::HYBRID_DEFAULT_LIFT_DAYS as Int;
    /// Most lift days of a run-led hybrid week (maintenance dose, CONC-RE-001).
    crate::hybrid::MAINTENANCE_LIFT_SESSIONS_MAX as Int;
    /// Most run days of a lift-led hybrid week (HYB-RUNCAP-001).
    crate::hybrid::ENDURANCE_FREQUENCY_CAP_DAYS as Int;
    /// One international mile, metres (exact).
    crate::running::MILE_M as Float;
    /// Race distances, metres (the half marathon is half the marathon).
    crate::running::FIVE_K_M as Float;
    crate::running::TEN_K_M as Float;
    crate::running::MARATHON_M as Float;
    crate::plates::LB_PER_KG as Float;
    /// Worst horizontal accuracy (m) a fix may carry to count (the QC gate).
    crate::gps::MAX_GPS_ACCURACY_M as Float;
    /// A ground speed above this (m/s) is GPS jitter, not running.
    crate::gps::MAX_PLAUSIBLE_SPEED_MPS as Float;
    /// A leg slower than this (m/s) is standing still: no moving time or distance.
    crate::gps::MIN_MOVING_SPEED_MPS as Float;
    /// Mean Earth radius (m) of the haversine distance.
    crate::gps::EARTH_RADIUS_M as Float;
    /// A gap between fixes longer than this (s) opens a new recording segment.
    crate::gps::MAX_FIX_GAP_SEC as Int;
    /// Most GPS fixes a saved run keeps.
    crate::gps::TRACK_DECIMATION_CAP as Int;
    /// A run under this distance (km) or [MIN_RUN_SEC] is too short to keep.
    crate::gps::MIN_RUN_KM as Float;
    crate::gps::MIN_RUN_SEC as Int;
    /// The largest run file an import reads, bytes.
    crate::import::MAX_RUN_IMPORT_BYTES as Int;
    /// RPE above target (actual − target) that trims the load (autoreg-001/002).
    crate::autoreg::RPE_DELTA_ABOVE as Float;
    /// RPE above target (actual − target) that trims the load further (autoreg-001/002).
    crate::autoreg::RPE_DELTA_WELL_ABOVE as Float;
    /// e1RM ratio (today ÷ baseline) below which the e1RM gate trims the top-set
    /// load (autoreg-006).
    crate::autoreg::E1RM_REDUCE_RATIO as Float;
    /// e1RM ratio (today ÷ baseline) above which the e1RM gate adds load (autoreg-007).
    crate::autoreg::E1RM_INCREASE_RATIO as Float;
    /// Velocity-loss fraction ending a power set, the lowest goal threshold (AUTOREG-VL-001).
    crate::strength::VL_STOP_POWER_FRAC as Float;
    /// Velocity-loss fraction ending a hypertrophy set, the highest goal threshold
    /// (AUTOREG-VL-001).
    crate::strength::VL_STOP_HYPERTROPHY_FRAC as Float;
    /// Wellness composite z-score at or below which one day eases the session
    /// (autoreg-030).
    crate::autoreg::WELLNESS_SINGLE_DAY_Z as Float;
    /// HRV lnRMSSD z-score below which the session eases (autoreg-028).
    crate::autoreg::HRV_SWC_LOWER_Z as Float;
    /// Aerobic-decoupling drift, %, at which the run stays easy (RUN-DECOUPLE-001).
    crate::load::DECOUPLING_HIGH_PCT as Float;
    /// Resting-HR elevation, bpm above baseline, that eases training once it
    /// lasts [RHR_DOWNGRADE_STREAK] days (autoreg-040/041).
    crate::autoreg::RHR_DOWNGRADE_FLOOR_BPM as Float;
    crate::autoreg::RHR_DOWNGRADE_STREAK as Int;
    /// Resting-HR elevation, bpm above baseline, that stops training (autoreg-040/041).
    crate::autoreg::RHR_STOP_BPM as Float;
    /// Soreness (1–7) at or above which intensity eases (autoreg-030).
    crate::autoreg::SORENESS_HIGH as Float;
    /// Shortest steady effort, minutes, whose decoupling counts (File 06 signal spec).
    crate::autoreg::DECOUPLING_MIN_EFFORT_MIN as Float;
    /// Age, years, where the masters bracket begins (MASTERS-001).
    crate::individualization::MASTERS_AGE_YEARS as Float;
    /// Co-occurring overtraining signs that raise the recovery concern (feedback-036).
    crate::feedback::OVERTRAINING_MIN_SIGNS as Int;
    /// Weeks the overtraining signs must last for the recovery concern (feedback-036).
    crate::feedback::OVERTRAINING_MIN_WEEKS as Float;
    /// Weeks of unexplained performance decline at which the overtraining
    /// referrals fire (safety-042, autoreg-042).
    crate::autoreg::NFOR_DECLINE_WEEKS as Int;
}
