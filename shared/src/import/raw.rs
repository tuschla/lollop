//! What a file reader hands the gates: each run's fixes as the file names
//! them, and the heart-rate samples a reader keeps.

use super::number::{kt_to_double, kt_trim};

/// Physiologically plausible heart-rate samples (bpm) an import keeps.
pub(super) const PLAUSIBLE_HR_BPM: std::ops::RangeInclusive<i32> = 20..=250;

/// One fix as a file names it, before the gates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RawFix {
    pub(super) lat: f64,
    pub(super) lon: f64,
    /// Unix seconds; 0 when the file names no (readable) time.
    pub(super) time_sec: i64,
    pub(super) hr_bpm: Option<i32>,
    /// The file's accuracy (FIT `gps_accuracy`, GPX `<hdop>`), else `None`.
    pub(super) accuracy_m: Option<f64>,
}

impl Default for RawFix {
    fn default() -> Self {
        Self {
            lat: f64::NAN,
            lon: f64::NAN,
            time_sec: 0,
            hr_bpm: None,
            accuracy_m: None,
        }
    }
}

/// One run as a file names it: its fixes per recording segment, the sport it
/// names (verbatim: TCX `<Activity Sport>`, GPX `<trk><type>`; FIT's sport is
/// gated in [`crate::fit`]) and lollop's own logged-at stamp.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RawRun {
    pub(super) segments: Vec<Vec<RawFix>>,
    pub(super) sport: Option<String>,
    pub(super) logged_at_sec: Option<i64>,
}

/// A heart-rate sample from a file, kept only when finite and physiologically
/// plausible ([`PLAUSIBLE_HR_BPM`]), truncated to whole bpm: a NaN, an
/// `Infinity` or 1e9 bpm never reaches the averages.
pub(super) fn hr_bpm(raw: &str) -> Option<i32> {
    let v = kt_to_double(kt_trim(raw))?;
    let (lo, hi) = (*PLAUSIBLE_HR_BPM.start(), *PLAUSIBLE_HR_BPM.end());
    (v.is_finite() && v >= f64::from(lo) && v <= f64::from(hi)).then_some(v as i32)
}
