//! File 04, Running engine core: pure, deterministic run-training math.
//!
//! No IO, no clock, no randomness. Every function is either a pure calculation
//! (HRmax, zone classification, band table lookup) returning a plain value, or a
//! prescriptive recommendation (spike gate, volume caps, taper) returning a
//! [`Recommended<T>`] carrying the backing [`Evidence`](crate::schema::Evidence)
//! and [`ConfidenceTag`](crate::schema::ConfidenceTag) from the claim registry.
//!
//! Rule ids and table values are transcribed verbatim from
//! `knowledge-base/extracted/04-running.md`.
//!
//! DELIBERATELY NOT IMPLEMENTED: ACWR (acute:chronic workload ratio,
//! `LOAD-ACWR-001`) is a hard-blocked `MarketingMyth`, formally challenged as
//! statistically invalid (mathematical coupling → spurious correlation, per
//! Impellizzeri 2020 / Lolli 2019 / Nielsen 2025) with a retraction request
//! filed for the Gabbett "sweet spot". No progression gate in this module
//! consults it; single-session distance spike (`RUN-SPIKE-001`) is the
//! strongest injury signal we act on instead.

use crate::schema::{Recommended, ThreeZone, VdotBand};

/// Wrap a value with the evidence + confidence of a registry claim (File 09).
///
/// Panics if `claim_id` is not in the registry: callers pass only the
/// canonical ids documented per function, so a miss is a programming error.
fn recommend<T>(value: T, claim_id: &str) -> Recommended<T> {
    let e = crate::evidence::claim(claim_id).expect("known claim");
    Recommended {
        value,
        evidence: e.to_evidence(),
        confidence: e.to_confidence_tag(),
    }
}

// ---------------------------------------------------------------------------
// 1. HRmax (pure calculation)
// ---------------------------------------------------------------------------

/// Estimate maximal heart rate via Tanaka (208 − 0.7·age). Rule RUN-HRMAX-001.
///
/// Pure population estimate; SEE ≈ ±10 bpm (individual variation is large, so
/// this is a fallback for a measured field-test max, not a criterion value).
pub fn hr_max_tanaka(age_years: f64) -> f64 {
    208.0 - 0.7 * age_years
}

// ---------------------------------------------------------------------------
// 2. Three-zone classification (pure calculation)
// ---------------------------------------------------------------------------

/// Classify an intensity by %HRmax into the LT1/LT2 three-zone model. Rule running-003.
///
/// Boundaries per File 04 table: Z1 < ~82 %HRmax, Z2 ~82–88 %, Z3 > ~88 %.
/// These are engine defaults; File 04 stresses LT1/LT2 should be field-measured
/// (LT1 45–70 %HRmax, LT2 55–93 %HRmax) rather than hardcoded when data exist.
pub fn classify_three_zone(pct_hr_max: f64) -> ThreeZone {
    if pct_hr_max < 82.0 {
        ThreeZone::Z1
    } else if pct_hr_max <= 88.0 {
        ThreeZone::Z2
    } else {
        ThreeZone::Z3
    }
}

// ---------------------------------------------------------------------------
// 3. VDOT band → physiological ranges (pure table lookup)
// ---------------------------------------------------------------------------

/// %HRmax range (low, high) for a VDOT band. Rule running-007 / table verbatim.
///
/// R (Repetition) uses pace not HR; its HR row is a nominal placeholder and
/// should not anchor prescription (see [`vdot_band_uses_hr`]).
pub fn vdot_band_hr_pct(band: VdotBand) -> (f64, f64) {
    match band {
        VdotBand::Easy => (65.0, 79.0),
        VdotBand::Marathon => (80.0, 85.0),
        VdotBand::Threshold => (88.0, 92.0),
        VdotBand::Interval => (97.0, 100.0),
        // "use pace, not HR", nominal, do not prescribe from this.
        VdotBand::Repetition => (100.0, 100.0),
    }
}

/// %VO2max range (low, high) for a VDOT band. Rule running-007 / table verbatim.
///
/// R (Repetition) is ">100 %VO2max"; represented here as an open upper bound
/// with `f64::INFINITY`.
pub fn vdot_band_vo2max_pct(band: VdotBand) -> (f64, f64) {
    match band {
        VdotBand::Easy => (59.0, 74.0),
        VdotBand::Marathon => (80.0, 84.0),
        VdotBand::Threshold => (83.0, 88.0),
        VdotBand::Interval => (95.0, 100.0),
        VdotBand::Repetition => (100.0, f64::INFINITY),
    }
}

/// Whether HR is a valid anchor for this band. Rules running-002 / running-007.
///
/// HR is a secondary check for E/M/T; pace/effort governs I and R because HR
/// lags on short reps (running-002 declares hr_valid_for = {E, M, T}).
pub fn vdot_band_uses_hr(band: VdotBand) -> bool {
    matches!(band, VdotBand::Easy | VdotBand::Marathon | VdotBand::Threshold)
}

// ---------------------------------------------------------------------------
// 4. Volume caps (prescriptive validators)
// ---------------------------------------------------------------------------

/// The weekly-share limits from File 04's "Volume caps" section (fractions of weekly volume).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeCaps {
    /// Long-run single-run cap (Daniels ≤25 %).
    pub long_run_max_frac: f64,
    /// Threshold (T) weekly cap ≤10 %.
    pub threshold_max_frac: f64,
    /// Interval (I) weekly cap ≤8 %.
    pub interval_max_frac: f64,
    /// Repetition (R) weekly cap ≤5 %.
    pub repetition_max_frac: f64,
}

/// Which cap a volume-cap check violated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapViolation {
    LongRun,
    Threshold,
    Interval,
    Repetition,
}

/// Canonical File 04 weekly volume caps (Daniels). Rule running-016/018/019 + table.
pub fn default_volume_caps() -> VolumeCaps {
    VolumeCaps {
        long_run_max_frac: 0.25,
        threshold_max_frac: 0.10,
        interval_max_frac: 0.08,
        repetition_max_frac: 0.05,
    }
}

/// True if the long run is within the single-run cap (≤25 % of weekly). Rule running-016.
///
/// A non-positive weekly total cannot satisfy a share cap, so returns false.
pub fn long_run_within_cap(long_run_km: f64, weekly_km: f64) -> bool {
    if weekly_km <= 0.0 {
        return false;
    }
    long_run_km / weekly_km <= default_volume_caps().long_run_max_frac
}

/// True if threshold (T) volume is within the ≤10 % weekly cap. Rule running-018.
pub fn threshold_within_cap(threshold_km: f64, weekly_km: f64) -> bool {
    if weekly_km <= 0.0 {
        return false;
    }
    threshold_km / weekly_km <= default_volume_caps().threshold_max_frac
}

/// True if interval (I) volume is within the ≤8 % weekly cap. Rule running-019.
pub fn interval_within_cap(interval_km: f64, weekly_km: f64) -> bool {
    if weekly_km <= 0.0 {
        return false;
    }
    interval_km / weekly_km <= default_volume_caps().interval_max_frac
}

/// True if repetition (R) volume is within the ≤5 % weekly cap. Rule running-018 table.
pub fn repetition_within_cap(repetition_km: f64, weekly_km: f64) -> bool {
    if weekly_km <= 0.0 {
        return false;
    }
    repetition_km / weekly_km <= default_volume_caps().repetition_max_frac
}

/// Check all four caps, returning the first violation (if any). Prescriptive → RUN-DIST-001.
///
/// Wrapped in `Recommended` because "these shares are/are not within safe
/// distribution" is coaching advice; cites the distribution claim RUN-DIST-001.
pub fn check_volume_caps(
    long_run_km: f64,
    threshold_km: f64,
    interval_km: f64,
    repetition_km: f64,
    weekly_km: f64,
) -> Recommended<Option<CapViolation>> {
    let violation = if !long_run_within_cap(long_run_km, weekly_km) {
        Some(CapViolation::LongRun)
    } else if !threshold_within_cap(threshold_km, weekly_km) {
        Some(CapViolation::Threshold)
    } else if !interval_within_cap(interval_km, weekly_km) {
        Some(CapViolation::Interval)
    } else if !repetition_within_cap(repetition_km, weekly_km) {
        Some(CapViolation::Repetition)
    } else {
        None
    };
    recommend(violation, "RUN-DIST-001")
}

// ---------------------------------------------------------------------------
// 5. Single-session distance spike (prescriptive gate)
// ---------------------------------------------------------------------------

/// Raw predicate: does this session exceed the 30-day longest run by >10 %? Rule RUN-SPIKE-001 / running-029.
///
/// Strongest running injury signal (Frandsen 2025). A non-positive 30-day
/// longest (no history) means any real session is unbounded relative to it →
/// treated as a spike so the gate errs safe.
pub fn single_session_spike(session_km: f64, longest_30d_km: f64) -> bool {
    if longest_30d_km <= 0.0 {
        return session_km > 0.0;
    }
    session_km > longest_30d_km * 1.10
}

/// Prescriptive block/flag on a single-session distance spike. Rule RUN-SPIKE-001.
///
/// `true` = block/flag. Wrapped in `Recommended` carrying RUN-SPIKE-001 evidence
/// because it drives an action (block the session).
pub fn single_session_spike_flag(session_km: f64, longest_30d_km: f64) -> Recommended<bool> {
    recommend(single_session_spike(session_km, longest_30d_km), "RUN-SPIKE-001")
}

// ---------------------------------------------------------------------------
// 6. Taper (prescriptive)
// ---------------------------------------------------------------------------

/// Bosquet-style taper prescription: only volume drops. Rule TAPER-001 / running-037.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TaperRx {
    /// Taper length, weeks (default 2; >3 wk risks detraining).
    pub weeks: u8,
    /// Volume reduction range (low, high) as fractions, e.g. (0.41, 0.60).
    pub volume_reduction_frac: (f64, f64),
    /// Hold training intensity unchanged (always true, never de-intensify).
    pub hold_intensity: bool,
    /// Hold session frequency unchanged (always true).
    pub hold_frequency: bool,
    /// Never introduce a new stimulus during taper (always true).
    pub add_new_stimulus: bool,
}

/// Recommend a taper `weeks_out` from the race, or `None` if too early. Rule TAPER-001 / running-037.
///
/// File 04 default: 2-week taper, exponential volume −41–60 %, intensity and
/// frequency held. Distance-specific variants (running-038) live in the planner;
/// this returns the population default keyed on how close the race is.
/// `weeks_out == 0` (race week) still returns the active taper prescription;
/// `weeks_out > 3` returns `None` (outside the taper window, and >21 days risks
/// detraining).
pub fn taper(weeks_out: u8) -> Option<Recommended<TaperRx>> {
    if weeks_out > 3 {
        return None;
    }
    let rx = TaperRx {
        weeks: 2,
        volume_reduction_frac: (0.41, 0.60),
        hold_intensity: true,
        hold_frequency: true,
        add_new_stimulus: false,
    };
    Some(recommend(rx, "TAPER-001"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tanaka_at_age_30_is_about_187() {
        // 208 - 0.7*30 = 187.0
        let hr = hr_max_tanaka(30.0);
        assert!((hr - 187.0).abs() < 1e-9, "got {hr}");
    }

    #[test]
    fn three_zone_boundaries() {
        assert_eq!(classify_three_zone(70.0), ThreeZone::Z1);
        assert_eq!(classify_three_zone(81.9), ThreeZone::Z1);
        // 82 is the LT1 boundary → Z2 (inclusive lower edge).
        assert_eq!(classify_three_zone(82.0), ThreeZone::Z2);
        assert_eq!(classify_three_zone(88.0), ThreeZone::Z2);
        // Just above LT2 → Z3.
        assert_eq!(classify_three_zone(88.1), ThreeZone::Z3);
        assert_eq!(classify_three_zone(95.0), ThreeZone::Z3);
    }

    #[test]
    fn vdot_table_values_verbatim() {
        assert_eq!(vdot_band_hr_pct(VdotBand::Easy), (65.0, 79.0));
        assert_eq!(vdot_band_vo2max_pct(VdotBand::Marathon), (80.0, 84.0));
        assert_eq!(vdot_band_hr_pct(VdotBand::Threshold), (88.0, 92.0));
        assert!(!vdot_band_uses_hr(VdotBand::Repetition));
        assert!(vdot_band_uses_hr(VdotBand::Threshold));
        // R is >100 %VO2max: open upper bound.
        assert!(vdot_band_vo2max_pct(VdotBand::Repetition).1.is_infinite());
    }

    #[test]
    fn long_run_cap_pass_and_fail() {
        // 12 km of a 50 km week = 24 % ≤ 25 %: pass.
        assert!(long_run_within_cap(12.0, 50.0));
        // Exactly 25 %: pass (cap is inclusive).
        assert!(long_run_within_cap(12.5, 50.0));
        // 15 km of 50 km = 30 %: fail.
        assert!(!long_run_within_cap(15.0, 50.0));
        // Zero weekly volume cannot satisfy a share cap.
        assert!(!long_run_within_cap(5.0, 0.0));
    }

    #[test]
    fn spike_true_just_above_10pct_false_at_or_below() {
        // 10 % over exactly: not a spike (rule is strictly >10 %).
        assert!(!single_session_spike(22.0, 20.0));
        // Just above 10 %: spike.
        assert!(single_session_spike(22.01, 20.0));
        // Below the longest run: never a spike.
        assert!(!single_session_spike(18.0, 20.0));
        // No history → any real session errs to spike.
        assert!(single_session_spike(5.0, 0.0));
    }

    #[test]
    fn taper_returns_reduction_in_documented_range() {
        let rx = taper(2).expect("2 weeks out is inside the taper window");
        let (lo, hi) = rx.value.volume_reduction_frac;
        assert!((lo - 0.41).abs() < 1e-9);
        assert!((hi - 0.60).abs() < 1e-9);
        assert!(rx.value.hold_intensity && rx.value.hold_frequency);
        assert!(!rx.value.add_new_stimulus);
        // Outside the window → no taper prescription.
        assert!(taper(5).is_none());
    }

    #[test]
    fn check_volume_caps_flags_first_violation() {
        // All within caps: None.
        let ok = check_volume_caps(12.0, 4.0, 3.0, 2.0, 50.0);
        assert_eq!(ok.value, None);
        // Long run over 25 %: LongRun wins (checked first).
        let bad = check_volume_caps(20.0, 4.0, 3.0, 2.0, 50.0);
        assert_eq!(bad.value, Some(CapViolation::LongRun));
    }
}
