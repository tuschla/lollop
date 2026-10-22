//! File 04, Running engine core: pure, deterministic run-training math.
//!
//! No IO, no clock, no randomness. Every function is either a pure calculation
//! (HRmax, zone classification, band table lookup) returning a plain value, or a
//! prescriptive recommendation (spike gate, volume caps, taper) returning a
//! [`Recommended<T>`] whose [`ClaimId`](crate::evidence::ClaimId) names the
//! backing evidence row in the claim registry.
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

use crate::evidence::claims;
use crate::schema::{
    MesoPhase, Recommended, RunSegment, RunSessionType, RunVolume, ThreeZone, VdotBand, WorkoutType,
};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

// ---------------------------------------------------------------------------
// 1. HRmax (pure calculation)
// ---------------------------------------------------------------------------

/// Estimate maximal heart rate via Tanaka (208 − 0.7·age). Rule RUN-HRMAX-001.
///
/// Pure population estimate; SEE ≈ ±10 bpm (individual variation is large, so
/// this is a fallback for a measured field-test max, not a criterion value).
pub fn hr_max_tanaka(age_years: f64) -> f64 {
    TANAKA_INTERCEPT_BPM - TANAKA_SLOPE_BPM_PER_YEAR * age_years
}

/// Tanaka intercept, bpm (RUN-HRMAX-001: 208 − 0.7·age).
pub const TANAKA_INTERCEPT_BPM: f64 = 208.0;
/// Tanaka slope, bpm per year of age (RUN-HRMAX-001: 208 − 0.7·age).
pub const TANAKA_SLOPE_BPM_PER_YEAR: f64 = 0.7;

/// Whether an age (years) is one the Tanaka estimate may be emitted for: a
/// realistic adult/junior age, 5–100. Outside it no HRmax is fabricated.
pub fn tanaka_age_in_range(age_years: f64) -> bool {
    (5.0..=100.0).contains(&age_years)
}

/// A measured HRmax worth trusting over the Tanaka estimate: finite and
/// physiologically plausible (100–240 bpm). The one gate the HR-zone
/// calculator and run targets share.
pub fn plausible_measured_hr_max(measured_bpm: Option<f64>) -> Option<f64> {
    measured_bpm.filter(|m| m.is_finite() && (100.0..=240.0).contains(m))
}

// ---------------------------------------------------------------------------
// 2. Three-zone classification (pure calculation)
// ---------------------------------------------------------------------------

/// Upper bound of zone 1 (below LT1), %HRmax (File 04 running-003 table).
pub const ZONE1_CEILING_PCT_HRMAX: f64 = 82.0;
/// Floor of zone 3 and of threshold (T) work, %HRmax (running-003 / running-018).
pub const THRESHOLD_FLOOR_PCT_HRMAX: f64 = 88.0;
/// Floor of interval/VO2max (I) work, %HRmax (running-019).
pub const INTERVAL_FLOOR_PCT_HRMAX: f64 = 95.0;

/// Classify an intensity by %HRmax into the LT1/LT2 three-zone model. Rule running-003.
///
/// Boundaries per File 04 table: Z1 < ~82 %HRmax, Z2 ~82–<88 %, Z3 ≥ ~88 %.
/// The 88 % boundary is inclusive to the hard zone so a run held exactly at the
/// prescribed Tempo/Threshold floor (0.88, `tempo_rx`) counts as threshold work
/// rather than bucketing as moderate. These are engine defaults; File 04
/// stresses LT1/LT2 should be field-measured (LT1 45–70 %HRmax, LT2 55–93
/// %HRmax) rather than hardcoded when data exist.
pub fn classify_three_zone(pct_hr_max: f64) -> ThreeZone {
    if pct_hr_max < ZONE1_CEILING_PCT_HRMAX {
        ThreeZone::Z1
    } else if pct_hr_max < THRESHOLD_FLOOR_PCT_HRMAX {
        ThreeZone::Z2
    } else {
        ThreeZone::Z3
    }
}

/// How a hard run counts against the weekly quality caps: threshold work (T,
/// ≤10% weekly) vs interval/VO2max work (I, ≤8% weekly), or neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardRunKind {
    None,
    Threshold,
    Interval,
}

/// Classify a run for the weekly hard-run cap check. A tagged Tempo run is
/// threshold and a tagged Interval run is interval, regardless of average HR (a
/// correctly executed tempo must not be charged as interval volume). An untagged
/// run falls back to its average %HRmax: below the T floor (88 %HRmax,
/// running-018) it is neither; 88–95 % is threshold; ≥95 % is interval
/// (running-019). `pct_hr_max` is a percent (e.g. 90.0), not a fraction.
pub fn classify_hard_run(pct_hr_max: f64, workout_type: Option<&WorkoutType>) -> HardRunKind {
    match workout_type {
        Some(WorkoutType::Tempo) => return HardRunKind::Threshold,
        Some(WorkoutType::Interval) => return HardRunKind::Interval,
        _ => {}
    }
    if pct_hr_max < THRESHOLD_FLOOR_PCT_HRMAX {
        HardRunKind::None
    } else if pct_hr_max < INTERVAL_FLOOR_PCT_HRMAX {
        HardRunKind::Threshold
    } else {
        HardRunKind::Interval
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
/// R (Repetition) is ">100 %VO2max": its upper bound is open, so `high` is
/// `None` rather than a non-finite number that would serialize as JSON `null`.
pub fn vdot_band_vo2max_pct(band: VdotBand) -> (f64, Option<f64>) {
    match band {
        VdotBand::Easy => (59.0, Some(74.0)),
        VdotBand::Marathon => (80.0, Some(84.0)),
        VdotBand::Threshold => (83.0, Some(88.0)),
        VdotBand::Interval => (95.0, Some(100.0)),
        VdotBand::Repetition => (100.0, None),
    }
}

/// Whether HR is a valid anchor for this band. Rules running-002 / running-007.
///
/// HR is a secondary check for E/M/T; pace/effort governs I and R because HR
/// lags on short reps (running-002 declares hr_valid_for = {E, M, T}).
pub fn vdot_band_uses_hr(band: VdotBand) -> bool {
    matches!(
        band,
        VdotBand::Easy | VdotBand::Marathon | VdotBand::Threshold
    )
}

/// The marathon distance in metres (42.195 km).
pub const MARATHON_M: f64 = 42_195.0;

/// Pace band (fast, slow) in seconds per km for a workout row that states its
/// pace relative to marathon pace (running-015 Easy 15–25 %, running-016 Long
/// run 10–20 % slower than MP). MP is the athlete's Daniels marathon prediction
/// ÷ the marathon distance in km. `None` for a non-positive VDOT or a row
/// without both bounds.
pub fn mp_relative_pace_sec_per_km(vdot: f64, workout: RunWorkout) -> Option<(f64, f64)> {
    if !vdot.is_finite() || vdot <= 0.0 {
        return None;
    }
    let (Some(lo), Some(hi)) = workout_rx(workout).value.pct_slower_than_mp else {
        return None;
    };
    let mp = crate::load::daniels_predict(vdot, MARATHON_M) / (MARATHON_M / KM_M);
    (mp.is_finite() && mp > 0.0).then(|| (mp * (1.0 + lo / 100.0), mp * (1.0 + hi / 100.0)))
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

/// Long run's share of weekly volume, low end (running-016 / running-046,
/// RUN-LONGRUN-001 / RUN-LONGRUN-TARGET-001): Daniels' single-run cap, and the
/// share at or above [`LONG_RUN_SHARE_SPLIT_KM`].
pub const LONG_RUN_SHARE_MIN: f64 = 0.25;
/// Long run's share of weekly volume, high end (running-016 / running-046,
/// RUN-LONGRUN-001 / RUN-LONGRUN-TARGET-001): the share under
/// [`LONG_RUN_SHARE_SPLIT_KM`].
pub const LONG_RUN_SHARE_MAX: f64 = 0.30;
/// Threshold (T) work's weekly cap, fraction of volume (running-018, RUN-TEMPO-001).
pub const THRESHOLD_WEEKLY_CAP_FRAC: f64 = 0.10;
/// Interval (I) work's weekly cap, fraction of volume (running-019, RUN-INTERVAL-001).
pub const INTERVAL_WEEKLY_CAP_FRAC: f64 = 0.08;
/// Repetition (R) work's weekly cap, fraction of volume (running-051,
/// RUN-REPETITION-002).
pub const REPETITION_WEEKLY_CAP_FRAC: f64 = 0.05;

/// Canonical File 04 weekly volume caps (Daniels). Rule running-016/018/019 + table.
pub fn default_volume_caps() -> VolumeCaps {
    VolumeCaps {
        long_run_max_frac: LONG_RUN_SHARE_MIN,
        threshold_max_frac: THRESHOLD_WEEKLY_CAP_FRAC,
        interval_max_frac: INTERVAL_WEEKLY_CAP_FRAC,
        repetition_max_frac: REPETITION_WEEKLY_CAP_FRAC,
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

/// True if repetition (R) volume is within the ≤5 % weekly cap. VDOT R row +
/// volume-caps section (the KB has no dedicated R-session rule).
pub fn repetition_within_cap(repetition_km: f64, weekly_km: f64) -> bool {
    if weekly_km <= 0.0 {
        return false;
    }
    repetition_km / weekly_km <= default_volume_caps().repetition_max_frac
}

/// Check all four caps, returning the first violation (if any). Prescriptive →
/// RUN-VOLCAP-001 (the Daniels weekly-share caps themselves, ExpertOpinion
/// floor), not the polarized-vs-threshold distribution claim.
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
    crate::evidence::graded(violation, claims::RUN_VOLCAP_001)
}

// ---------------------------------------------------------------------------
// 5. Single-session distance spike (prescriptive gate)
// ---------------------------------------------------------------------------

/// RUN-SPIKE-001 / running-029: a single session more than this fraction over
/// the 30-day longest run is a spike.
pub const SPIKE_FRAC: f64 = 0.10;

/// Raw predicate: does this session exceed the 30-day longest run by more than
/// [`SPIKE_FRAC`]? Rule RUN-SPIKE-001 / running-029.
///
/// Strongest running injury signal (Frandsen 2025). A non-positive 30-day
/// longest (no history) means any real session is unbounded relative to it →
/// treated as a spike so the gate errs safe.
pub fn single_session_spike(session_km: f64, longest_30d_km: f64) -> bool {
    if longest_30d_km <= 0.0 {
        return session_km > 0.0;
    }
    session_km > longest_30d_km * (1.0 + SPIKE_FRAC)
}

/// Prescriptive block/flag on a single-session distance spike (running-029
/// HARD RULE → RUN-SPIKE-BLOCK-001, safety-critical).
///
/// `true` = block/flag. Wrapped in `Recommended` carrying RUN-SPIKE-BLOCK-001
/// evidence because it drives an action (block the session).
pub fn single_session_spike_flag(session_km: f64, longest_30d_km: f64) -> Recommended<bool> {
    crate::evidence::graded(
        single_session_spike(session_km, longest_30d_km),
        claims::RUN_SPIKE_BLOCK_001,
    )
}

// ---------------------------------------------------------------------------
// 6. Intensity distribution (prescriptive): File 04 §"distribution" table
// ---------------------------------------------------------------------------

/// Which easy/moderate/hard split model to run (File 04 running-011/013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionModel {
    /// ~80/15/5, beginners and base phase.
    Pyramidal,
    /// ~80/5/15, trained athletes, peak/specific phase.
    Polarized,
}

/// A three-bucket time-in-zone target (percent of weekly training time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntensityDistribution {
    /// Z1 easy share, % (both models anchor ~80).
    pub easy_pct: u8,
    /// Z2 moderate/threshold share, %.
    pub moderate_pct: u8,
    /// Z3 hard share, %.
    pub hard_pct: u8,
}

/// Target easy/moderate/hard split for a distribution model. Rule running-011.
///
/// Both models keep ~80% easy; they trade the moderate↔hard balance. 80/20 is a
/// POPULATION optimum (Seiler), individualize ±5–10% via TT re-tests.
pub fn intensity_distribution(model: DistributionModel) -> Recommended<IntensityDistribution> {
    let d = match model {
        DistributionModel::Pyramidal => IntensityDistribution {
            easy_pct: 80,
            moderate_pct: 15,
            hard_pct: 5,
        },
        DistributionModel::Polarized => IntensityDistribution {
            easy_pct: 80,
            moderate_pct: 5,
            hard_pct: 15,
        },
    };
    crate::evidence::graded(d, claims::RUN_DIST_001)
}

/// Distribution model for a mesocycle phase: pyramidal early, polarized near
/// race (File 04 running-013/035; engine default for contested CQ-01).
pub fn distribution_for_phase(phase: MesoPhase) -> Recommended<IntensityDistribution> {
    let model = match phase {
        MesoPhase::Base | MesoPhase::Build | MesoPhase::Deload => DistributionModel::Pyramidal,
        MesoPhase::Peak | MesoPhase::Taper => DistributionModel::Polarized,
    };
    intensity_distribution(model)
}

// ---------------------------------------------------------------------------
// 7. Quality-session spacing (prescriptive validator): File 04 running-023
// ---------------------------------------------------------------------------

/// Quality-session governance limits (File 04 running-023 / §"Volume caps").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityLimits {
    /// Max hard/quality sessions per week.
    pub max_per_week: u8,
    /// Minimum spacing between quality sessions, hours.
    pub min_spacing_hours: u8,
    /// Whether two Z3 sessions on consecutive days are allowed (never, non-elite).
    pub allow_consecutive_z3: bool,
}

/// Canonical quality limits: ≤3/week, ≥48 h apart, no back-to-back Z3. Rule
/// running-023 → RUN-QUALITY-001 (ExpertOpinion, safety-critical).
pub fn quality_limits() -> Recommended<QualityLimits> {
    crate::evidence::graded(
        QualityLimits {
            max_per_week: 3,
            min_spacing_hours: 48,
            allow_consecutive_z3: false,
        },
        claims::RUN_QUALITY_001,
    )
}

/// True when a week's quality plan respects the caps: ≤3 sessions, ≥48 h gaps,
/// and no consecutive-Z3 stacking (File 04 running-023).
pub fn quality_plan_ok(
    sessions_per_week: u8,
    min_gap_hours: u8,
    has_consecutive_z3: bool,
) -> Recommended<bool> {
    let limits = quality_limits().value;
    let ok = sessions_per_week <= limits.max_per_week
        && min_gap_hours >= limits.min_spacing_hours
        && (!has_consecutive_z3 || limits.allow_consecutive_z3);
    crate::evidence::graded(ok, claims::RUN_QUALITY_001)
}

// ---------------------------------------------------------------------------
// 8. Weekly mileage progression (prescriptive): File 04 running-031/028
// ---------------------------------------------------------------------------

/// Safe single-week volume-increase cap as a fraction, by training age
/// (File 04 running-031). Novice (<1 yr) tolerates up to +10 %; experienced
/// runners are held to ~+5 %. NOT the discredited hard "10 % rule", a ceiling.
pub fn weekly_increase_cap_frac(training_age_years: f64) -> Recommended<f64> {
    let cap = if training_age_years < 1.0 { 0.10 } else { 0.05 };
    crate::evidence::graded(cap, claims::RUN_PROGRESS_001)
}

/// True when next week's volume stays within the training-age increase cap
/// (File 04 running-031). A non-positive current volume cannot be ratioed → false.
pub fn weekly_increase_ok(
    current_km: f64,
    next_km: f64,
    training_age_years: f64,
) -> Recommended<bool> {
    let cap = weekly_increase_cap_frac(training_age_years).value;
    let ok = ratio_increase_within(current_km, next_km, cap).unwrap_or(false);
    crate::evidence::graded(ok, claims::RUN_PROGRESS_001)
}

/// Whether growing from `current` to `next` stays within a fractional increase
/// `cap` (0.10 = +10 %). `None` when `current` is not a positive finite baseline:
/// the ratio is undefined, and each caller decides what "no baseline" means.
/// The one ratio behind both the training-age cap ([`weekly_increase_ok`]) and
/// the hybrid combined-load ramp (`hybrid::combined_load_progression_ok`), so
/// the two can only differ by cap, never by arithmetic.
pub fn ratio_increase_within(current: f64, next: f64, cap: f64) -> Option<bool> {
    (current.is_finite() && current > 0.0).then(|| (next - current) / current <= cap + 1e-9)
}

/// Two-week distance growth ratio above which injury risk is flagged: a rise of
/// more than 30 % (File 04 running-028; Nielsen 2014 ~1.6× risk; RUN-PROGRESS-001).
pub const TWO_WEEK_INCREASE_FLAG_RATIO: f64 = 1.30;

/// Flag elevated injury risk when weekly distance rises more than
/// [`TWO_WEEK_INCREASE_FLAG_RATIO`] across two weeks (File 04 running-028).
/// `true` = flag. A non-positive baseline (no history) flags any real increase,
/// erring safe.
pub fn two_week_increase_flag(baseline_km: f64, current_km: f64) -> Recommended<bool> {
    let flag = if baseline_km <= 0.0 {
        current_km > 0.0
    } else {
        current_km > baseline_km * TWO_WEEK_INCREASE_FLAG_RATIO
    };
    crate::evidence::graded(flag, claims::RUN_PROGRESS_001)
}

// ---------------------------------------------------------------------------
// 9. Deload cadence (prescriptive): File 04 running-033
// ---------------------------------------------------------------------------

/// Load:recovery cycle prescription (File 04 running-033; RUN-DELOAD-001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeloadCadence {
    /// Consecutive loading weeks before a recovery week.
    pub load_weeks: u8,
    /// Recovery weeks (always 1 here).
    pub recovery_weeks: u8,
    /// Recovery-week volume+intensity reduction range (low, high) as fractions.
    pub reduction_frac: (f64, f64),
}

/// Load:recovery cadence: default 3:1, dropping to 2:1 when `conservative`
/// (older, injury-prone, or low training age). Recovery week cuts both volume
/// and intensity 20–40 % (File 04 running-033; mileage-dependent depth and the
/// drop-a-quality-session directive live in [`recovery_week_rx`]).
pub fn deload_cadence(conservative: bool) -> Recommended<DeloadCadence> {
    let load_weeks = if conservative { 2 } else { 3 };
    crate::evidence::graded(
        DeloadCadence {
            load_weeks,
            recovery_weeks: 1,
            reduction_frac: (0.20, 0.40),
        },
        claims::RUN_DELOAD_001,
    )
}

// ---------------------------------------------------------------------------
// Workout-type prescription table (running-014 … running-022)
// ---------------------------------------------------------------------------

/// Prescription band for a single run workout. Rules running-014 … running-022.
///
/// Every band side is `Option`: `None` means the KB states NO bound there -
/// open sides are never filled with invented numbers. `pct_hr_max` is the
/// %HRmax band as fractions; `pct_hrr_max` is a Karvonen %HRR ceiling where the
/// KB states one (Recovery only); `pct_slower_than_mp` anchors pace relative to
/// marathon pace in percent SLOWER than MP (positive = slower; the KB's primary
/// pace parameter for Recovery/Easy/Long). When `hr_governed` is false the
/// effort is too short/intense for HR to settle, so pace/effort governs and any
/// HR band is only a coarse ceiling (running-002: hr_valid_for = {E, M, T}).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunWorkoutRx {
    /// Target %HRmax band (low, high), fractions of HRmax.
    pub pct_hr_max: (Option<f64>, Option<f64>),
    /// %HRR (Karvonen) ceiling, fraction. Recovery carries <70 %HRR alongside
    /// <76 %HRmax (running-014's two ceilings); when both are computable the
    /// stricter governs, the conservative reading of the KB's %HRmax/%HRR
    /// inconsistency (statement `<76%HRmax / <70%HRR` vs parameters
    /// `<70–76%HRmax`).
    pub pct_hrr_max: Option<f64>,
    /// Pace band relative to marathon pace, percent slower than MP (min, max).
    pub pct_slower_than_mp: (Option<f64>, Option<f64>),
    /// RPE band; `None` when the KB states none (Repetition).
    pub rpe: Option<(u8, u8)>,
    /// Session (continuous) or rep duration band, minutes.
    pub duration_min: (Option<u16>, Option<u16>),
    /// Session/segment distance band, km (RacePace: 8–26 km blocks).
    pub distance_km: (Option<f64>, Option<f64>),
    pub hr_governed: bool,
}

/// A run workout to prescribe. Wraps [`RunSessionType`] and adds the
/// Easy/General-Aerobic session (running-015), which is distinct from Recovery
/// (running-014) in pace, HR, RPE, and duration but has no schema variant -
/// the KB itself maps Easy runs to `RunSessionType::Recovery`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunWorkout {
    Session(RunSessionType),
    /// Easy / General-Aerobic run (running-015): E pace 15–25 % slower than
    /// MP, 65–79 %HRmax, RPE 3–4, 30–90 min.
    EasyGeneralAerobic,
}

/// Look up the prescription band for a run workout. Rules running-014…022 +
/// the VDOT R row. Per-rule claims: Recovery running-014 → RUN-RECOVERY-001,
/// Easy running-015 → RUN-EASY-001, LongRun running-016 → RUN-LONGRUN-001,
/// RacePace running-017 → RUN-RACEPACE-001, Tempo running-018 → RUN-TEMPO-001,
/// Interval running-019 → RUN-INTERVAL-001 (contested CQ-10), Repetition VDOT
/// R row → RUN-REPETITION-001, Strides running-020 → RUN-STRIDES-001; Hills
/// blends running-021/022 and stays on the workout-table synthesis claim
/// (RUN-WORKOUT-001), the hill-sprint-specific numbers live in
/// [`hill_sprint_rx`].
pub fn workout_rx(workout: RunWorkout) -> Recommended<RunWorkoutRx> {
    use RunSessionType::*;
    let open = RunWorkoutRx {
        pct_hr_max: (None, None),
        pct_hrr_max: None,
        pct_slower_than_mp: (None, None),
        rpe: None,
        duration_min: (None, None),
        distance_km: (None, None),
        hr_governed: false,
    };
    let (rx, claim_id) = match workout {
        // running-014 Recovery: ≥20 % slower than MP (no upper slowness
        // bound), ceilings <76 %HRmax AND <70 %HRR (the KB states no lower HR
        // bound), RPE 2–3, 20–40 min.
        RunWorkout::Session(Recovery) => (
            RunWorkoutRx {
                pct_hr_max: (None, Some(0.76)),
                pct_hrr_max: Some(0.70),
                pct_slower_than_mp: (Some(20.0), None),
                rpe: Some((2, 3)),
                duration_min: (Some(20), Some(40)),
                hr_governed: true,
                ..open
            },
            claims::RUN_RECOVERY_001,
        ),
        // running-015 Easy / General-Aerobic: E pace 15–25 % slower than MP,
        // 65–79 %HRmax, RPE 3–4, 30–90 min.
        RunWorkout::EasyGeneralAerobic | RunWorkout::Session(Easy) => (
            RunWorkoutRx {
                pct_hr_max: (Some(0.65), Some(0.79)),
                pct_slower_than_mp: (Some(15.0), Some(25.0)),
                rpe: Some((3, 4)),
                duration_min: (Some(30), Some(90)),
                hr_governed: true,
                ..open
            },
            claims::RUN_EASY_001,
        ),
        // running-016 Long Run: E→E+ (MP −10–20 %), 65–80 %HRmax, RPE 3–5.
        // The KB states NO duration floor;
        // the KB caps are shares (25–30 % weekly, single run ≤25 %) plus a
        // soft time-cap "~2:00–2:30": 150 min is the outer edge of that
        // window, kept as the only defensible hard ceiling. Share/guardrail
        // checks live in `long_run_within_cap` / `long_run_within_daily_avg`.
        RunWorkout::Session(LongRun) => (
            RunWorkoutRx {
                pct_hr_max: (Some(0.65), Some(0.80)),
                pct_slower_than_mp: (Some(10.0), Some(20.0)),
                rpe: Some((3, 5)),
                duration_min: (None, Some(150)),
                hr_governed: true,
                ..open
            },
            claims::RUN_LONGRUN_001,
        ),
        // running-018 Tempo / Threshold: 88–92 %HRmax, RPE 6–7, continuous
        // 20–40 min. The KB's "~90 % MP" does not say whether it means pace or
        // speed, so no MP-relative band is encoded (cruise-interval structure:
        // `cruise_interval_rx`).
        RunWorkout::Session(Tempo) => (
            RunWorkoutRx {
                pct_hr_max: (Some(0.88), Some(0.92)),
                rpe: Some((6, 7)),
                duration_min: (Some(20), Some(40)),
                hr_governed: true,
                ..open
            },
            claims::RUN_TEMPO_001,
        ),
        // running-017 Marathon-pace segments: M pace (0 % offset from MP by
        // definition), 80–85 %HRmax, RPE 5–6, 8–26 km blocks: the KB states
        // the blocks in km, not minutes, so no duration band is set. No weekly
        // M-pace cap exists (VDOT table M-row cap: "-").
        RunWorkout::Session(RacePace) => (
            RunWorkoutRx {
                pct_hr_max: (Some(0.80), Some(0.85)),
                pct_slower_than_mp: (Some(0.0), Some(0.0)),
                rpe: Some((5, 6)),
                distance_km: (Some(8.0), Some(26.0)),
                hr_governed: true,
                ..open
            },
            claims::RUN_RACEPACE_001,
        ),
        // running-019 VO2max intervals (pace-governed, HR lags): the canonical
        // VDOT table gives 97–100 %HRmax (running-019 prose says 95–100; the
        // table is authoritative and matches `vdot_band_hr_pct`), RPE 8–9, 3–5
        // min reps (structure: `vo2max_interval_rx`).
        RunWorkout::Session(Interval) => (
            RunWorkoutRx {
                pct_hr_max: (Some(0.97), Some(1.00)),
                rpe: Some((8, 9)),
                duration_min: (Some(3), Some(5)),
                ..open
            },
            claims::RUN_INTERVAL_001,
        ),
        // VDOT R row: >100 %VO2max, "use pace, not HR" (no HR band), reps
        // ≤2 min, ≤5 % weekly. The KB has NO dedicated R-session rule, rep
        // distance/count, recovery, and RPE are unstated, so none is set.
        RunWorkout::Session(Repetition) => (
            RunWorkoutRx {
                duration_min: (None, Some(2)),
                ..open
            },
            claims::RUN_REPETITION_001,
        ),
        // running-020 Strides (neuromuscular; 15–30 s reps are sub-minute, so
        // only a coarse ≤1-min row here, exact seconds in `strides_rx`).
        RunWorkout::Session(Strides) => (
            RunWorkoutRx {
                rpe: Some((6, 7)),
                duration_min: (None, Some(1)),
                ..open
            },
            claims::RUN_STRIDES_001,
        ),
        // running-021 / running-022 Hill sprints & long hill reps (blended
        // row; hill-sprint specifics in `hill_sprint_rx`).
        RunWorkout::Session(Hills) => (
            RunWorkoutRx {
                pct_hr_max: (Some(0.90), Some(1.00)),
                rpe: Some((7, 9)),
                duration_min: (None, Some(4)),
                ..open
            },
            claims::RUN_WORKOUT_001,
        ),
    };
    crate::evidence::graded(rx, claim_id)
}

/// Prescription band for a schema run session type. Delegates to
/// [`workout_rx`]; use [`RunWorkout::EasyGeneralAerobic`] for Easy/GA runs
/// (running-015), which the schema folds into `Recovery`.
pub fn run_workout_rx(kind: RunSessionType) -> Recommended<RunWorkoutRx> {
    workout_rx(RunWorkout::Session(kind))
}

// ---------------------------------------------------------------------------
// Cruise intervals & VO2max interval structure (running-018/019)
// ---------------------------------------------------------------------------

/// Cruise-interval structure for threshold work (running-018 → RUN-TEMPO-001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CruiseIntervalRx {
    /// Rep duration band, minutes: 3–15.
    pub rep_duration_min: (u16, u16),
    /// Rest between reps, ~1 min (KB: "rest ~1 min/20–25%").
    pub rest_approx_min: f64,
    /// Alternative rest as a fraction, 20–25 %. The KB does not explicitly
    /// state the base of the percentage (read as fraction of rep time).
    pub rest_frac: (f64, f64),
    /// Cruise-interval total ≤10 % of weekly volume (volume-caps section).
    pub weekly_cap_frac: f64,
}

/// Cruise intervals (running-018): 3–15 min reps at T pace with ~1 min or
/// 20–25 % rest, total ≤10 % of weekly volume. Moderate (Daniels; Pfitzinger).
pub fn cruise_interval_rx() -> Recommended<CruiseIntervalRx> {
    crate::evidence::graded(
        CruiseIntervalRx {
            rep_duration_min: (3, 15),
            rest_approx_min: 1.0,
            rest_frac: (0.20, 0.25),
            weekly_cap_frac: THRESHOLD_WEEKLY_CAP_FRAC,
        },
        claims::RUN_TEMPO_001,
    )
}

/// VO2max-interval structure (running-019 → RUN-INTERVAL-001, contested CQ-10
/// vs Rønnestad short intervals).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vo2maxIntervalRx {
    /// Rep duration band, minutes: 3–5 (hard ceiling ≤5 min).
    pub rep_duration_min: (u16, u16),
    /// Rep distance band, metres: 800–1600.
    pub rep_distance_m: (u16, u16),
    /// Recovery ≈ rep time ("slightly less"): the ratio ceiling is 1.0×.
    pub recovery_max_ratio_of_rep: f64,
    /// Interval total ≤8 % of weekly volume.
    pub weekly_cap_frac: f64,
}

/// VO2max intervals (running-019): I pace (~3K–5K), reps 3–5 min (≤5 min,
/// 800–1600 m), recovery ≈ rep time (slightly less), total ≤8 % weekly.
/// Rep count per session is NOT stated in the KB.
pub fn vo2max_interval_rx() -> Recommended<Vo2maxIntervalRx> {
    crate::evidence::graded(
        Vo2maxIntervalRx {
            rep_duration_min: (3, 5),
            rep_distance_m: (800, 1600),
            recovery_max_ratio_of_rep: 1.0,
            weekly_cap_frac: INTERVAL_WEEKLY_CAP_FRAC,
        },
        claims::RUN_INTERVAL_001,
    )
}

// ---------------------------------------------------------------------------
// Strides & hill sprints (running-020/021)
// ---------------------------------------------------------------------------

/// Strides prescription (running-020 → RUN-STRIDES-001, ExpertOpinion).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StridesRx {
    /// Rep duration, seconds: 15–30.
    pub rep_sec: (u16, u16),
    /// Reps per session: 4–8.
    pub reps: (u8, u8),
    /// Near-full recovery, seconds: 45 s–2 min.
    pub recovery_sec: (u16, u16),
    /// Sessions per week: 1–3.
    pub per_week: (u8, u8),
    /// RPE 6–7, controlled-fast, NOT a sprint.
    pub rpe: (u8, u8),
}

/// Strides (running-020): 15–30 s ×4–8 controlled-fast efforts, recovery
/// 45 s–2 min, 1–3×/week, introduced a few weeks into base (the KB gives no
/// numeric week count for the introduction, and no pace band beyond
/// "controlled-fast (not sprint)" / RPE 6–7).
pub fn strides_rx() -> Recommended<StridesRx> {
    crate::evidence::graded(
        StridesRx {
            rep_sec: (15, 30),
            reps: (4, 8),
            recovery_sec: (45, 120),
            per_week: (1, 3),
            rpe: (6, 7),
        },
        claims::RUN_STRIDES_001,
    )
}

/// Hill-sprint prescription (running-021 → RUN-HILLSPRINT-001, ExpertOpinion;
/// contradiction on placement: Lydiard discrete 4–6-wk hill phase vs Hudson
/// year-round).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HillSprintRx {
    /// Rep duration, seconds: 8–20.
    pub rep_sec: (u16, u16),
    /// Reps per session: 4–10.
    pub reps: (u8, u8),
    /// Near-max effort, percent: 90–95.
    pub effort_pct: (f64, f64),
    /// Grade, percent: 6–10.
    pub grade_pct: (f64, f64),
    /// Full recovery: walk down, ~2 min.
    pub recovery_approx_sec: u16,
    /// RPE 9.
    pub rpe: u8,
    /// Sessions per week, NOT stated in the KB (`None`, never invented).
    pub per_week: Option<(u8, u8)>,
    /// Treated as strength work, placed on easy days.
    pub on_easy_days: bool,
}

/// Hill sprints (running-021): 8–20 s ×4–10 near-max (90–95 %) efforts on
/// 6–10 % grade, full recovery (walk down / ~2 min), RPE 9, treated as
/// strength work on easy days. Weekly frequency is unstated in the KB.
pub fn hill_sprint_rx() -> Recommended<HillSprintRx> {
    crate::evidence::graded(
        HillSprintRx {
            rep_sec: (8, 20),
            reps: (4, 10),
            effort_pct: (90.0, 95.0),
            grade_pct: (6.0, 10.0),
            recovery_approx_sec: 120,
            rpe: 9,
            per_week: None,
            on_easy_days: true,
        },
        claims::RUN_HILLSPRINT_001,
    )
}

// ---------------------------------------------------------------------------
// Long-run guardrails (running-016)
// ---------------------------------------------------------------------------

/// Low-mileage guardrail (running-016): the long run must not exceed 2× the
/// average daily run. Errs safe on a non-positive average (no history → any
/// real long run fails the guardrail).
pub fn long_run_within_daily_avg(long_run_km: f64, avg_daily_run_km: f64) -> Recommended<bool> {
    let ok = avg_daily_run_km > 0.0 && long_run_km <= 2.0 * avg_daily_run_km;
    crate::evidence::graded(ok, claims::RUN_LONGRUN_001)
}

/// Default weekly long-run share band (running-016 + volume-caps section):
/// 25–30 % of weekly volume, as fractions. `goal_week_plan` carries the same
/// band for its distance-race rows.
pub fn long_run_share_default() -> Recommended<(f64, f64)> {
    crate::evidence::graded(
        (LONG_RUN_SHARE_MIN, LONG_RUN_SHARE_MAX),
        claims::RUN_LONGRUN_001,
    )
}

// ---------------------------------------------------------------------------
// Run/walk extension, single-variable progression, recovery-week depth
// (running-026/032/033)
// ---------------------------------------------------------------------------

/// running-032: progress only ONE variable at a time, volume OR intensity,
/// never both in the same week. `true` = the week's plan is OK.
pub fn single_variable_progression_ok(
    volume_increased: bool,
    intensity_increased: bool,
) -> Recommended<bool> {
    crate::evidence::graded(
        !(volume_increased && intensity_increased),
        claims::RUN_ONEVAR_001,
    )
}

/// Mileage band for recovery-week depth (running-033). The KB gives NO numeric
/// threshold separating higher- from lower-mileage runners, the caller
/// supplies the judgement; `Unspecified` uses the general 20–40 % band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MileageBand {
    Higher,
    Lower,
    Unspecified,
}

/// Recovery-week prescription depth (running-033 → RUN-DELOAD-DEPTH-001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecoveryWeekRx {
    /// Volume-reduction band (floor, ceiling) as fractions. The floor is
    /// `None` for lower-mileage runners: the KB says only "up to 50 %".
    pub volume_reduction_frac: (Option<f64>, f64),
    /// Reduce intensity too (running-033: "reducing both volume and
    /// intensity"); the KB states no intensity magnitude.
    pub reduce_intensity: bool,
    /// Drop a quality session on the recovery week (which one is unstated).
    pub drop_quality_session: bool,
}

/// Recovery-week depth by mileage band (running-033): general 20–40 %,
/// higher-mileage 10–30 %, lower-mileage up to 50 %; reduce both volume and
/// intensity and drop a quality session. Cycle cadence (3:1 / 2:1) is
/// [`deload_cadence`].
pub fn recovery_week_rx(band: MileageBand) -> Recommended<RecoveryWeekRx> {
    let volume_reduction_frac = match band {
        MileageBand::Higher => (Some(0.10), 0.30),
        MileageBand::Lower => (None, 0.50),
        MileageBand::Unspecified => (Some(0.20), 0.40),
    };
    crate::evidence::graded(
        RecoveryWeekRx {
            volume_reduction_frac,
            reduce_intensity: true,
            drop_quality_session: true,
        },
        claims::RUN_DELOAD_DEPTH_001,
    )
}

// ---------------------------------------------------------------------------
// Distance-specific taper defaults (running-038)
// ---------------------------------------------------------------------------

/// Distance-specific taper prescription (running-038 → RUN-TAPER-001, Strong,
/// SAFETY-CRITICAL: intensity and frequency are always held; only volume
/// drops; never add new stimulus).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistanceTaperRx {
    /// Taper length band, days.
    pub days: (u8, u8),
    /// Volume-cut band as fractions.
    pub volume_cut_frac: (f64, f64),
    /// Progressive volume cut, the KB marks only the marathon taper so.
    pub progressive: bool,
    /// 5K/10K: keep 1–2 short sharp I/T sessions.
    pub keep_sharp_sessions: Option<(u8, u8)>,
    /// Marathon: keep MP touches + short tempo.
    pub keep_mp_touches_and_short_tempo: bool,
    /// Always true, never de-intensify during taper.
    pub hold_intensity: bool,
    /// Always true, never cut session frequency.
    pub hold_frequency: bool,
}

/// Distance-specific taper defaults (running-038, Bosquet 2007 / Mujika &
/// Padilla): 5K/10K 7–10 d cut ~40–50 % keeping 1–2 short sharp I/T; HM
/// 10–14 d cut ~50 %; marathon 2–3 wk cut 40–60 % progressive keeping MP
/// touches + short tempo. Returns `None` for goals the KB states no
/// distance-specific default for (general fitness, C25K).
pub fn distance_taper(goal: GoalDistance) -> Option<Recommended<DistanceTaperRx>> {
    let base = DistanceTaperRx {
        days: (0, 0),
        volume_cut_frac: (0.0, 0.0),
        progressive: false,
        keep_sharp_sessions: None,
        keep_mp_touches_and_short_tempo: false,
        hold_intensity: true,
        hold_frequency: true,
    };
    let rx = match goal {
        GoalDistance::FiveK | GoalDistance::TenK => DistanceTaperRx {
            days: (7, 10),
            volume_cut_frac: (0.40, 0.50),
            keep_sharp_sessions: Some((1, 2)),
            ..base
        },
        GoalDistance::HalfMarathon => DistanceTaperRx {
            days: (10, 14),
            volume_cut_frac: (0.50, 0.50),
            ..base
        },
        GoalDistance::Marathon => DistanceTaperRx {
            days: (14, 21),
            volume_cut_frac: (0.40, 0.60),
            progressive: true,
            keep_mp_touches_and_short_tempo: true,
            ..base
        },
        GoalDistance::General | GoalDistance::C25k => return None,
    };
    Some(crate::evidence::graded(rx, claims::RUN_TAPER_001))
}

// ---------------------------------------------------------------------------
// Race-date periodization (running-035/042/043/052) and the per-goal,
// per-phase quality mix (running-024)
// ---------------------------------------------------------------------------

/// Race-specific Peak weeks placed right before the taper (running-052,
/// ExpertOpinion parameter: Daniels' final phase / Casado 2022's competitive
/// block, shortened to 3 weeks).
pub const PEAK_WEEKS: u32 = 3;

/// A dated race plan more than this many whole weeks out is still in Base
/// (running-052: ~16 weeks of race-specific preparation).
pub const BUILD_HORIZON_WEEKS: u32 = 16;

/// Whole calendar weeks the taper covers for a race goal: the distance
/// default's longest taper rounded up to whole weeks (running-038: 5K/10K
/// 7–10 d and HM 10–14 d → 2 weeks, marathon 14–21 d → 3 weeks), which also
/// keeps running-037's 2-week Bosquet taper. `0` for goals with no race.
pub fn taper_weeks(goal: GoalDistance) -> u32 {
    distance_taper(goal).map_or(0, |t| (t.value.days.1 as u32).div_ceil(7))
}

/// The training phase of a dated race-plan week that sits `weeks_to_race`
/// whole weeks before the race's own week (0 = the race week): Taper in the
/// final `taper_weeks`, Peak in the `PEAK_WEEKS` before that, Build back to
/// `BUILD_HORIZON_WEEKS`, Base earlier (running-035 sequence, running-042
/// race-date anchoring). `None` for goals with no race distance.
pub fn race_phase(goal: GoalDistance, weeks_to_race: u32) -> Option<Recommended<MesoPhase>> {
    let taper = taper_weeks(goal);
    if taper == 0 {
        return None;
    }
    let (phase, claim) = if weeks_to_race < taper {
        (MesoPhase::Taper, claims::RUN_PERIOD_001)
    } else if weeks_to_race < taper + PEAK_WEEKS {
        (MesoPhase::Peak, claims::RUN_PHASE_TID_001)
    } else if weeks_to_race < BUILD_HORIZON_WEEKS {
        (MesoPhase::Build, claims::RUN_PHASE_TID_001)
    } else {
        (MesoPhase::Base, claims::RUN_PHASE_TID_001)
    };
    Some(crate::evidence::graded(phase, claim))
}

/// Volume cut of taper week `k` of `n` (1-based) against the last full load
/// week: progressive from the Bosquet floor (41 %) up to the distance
/// default's deepest cut, capped at Bosquet's 60 % (running-037/038/042).
/// 10K: 41 % then 50 %; half marathon: a 50 % step taper; marathon: 41 / 50.5
/// / 60 %. Intensity and run frequency are held (the caller keeps every run day
/// and the quality sessions). `None` for goals with no race distance.
pub fn taper_volume_cut(goal: GoalDistance, k: u32, n: u32) -> Option<Recommended<f64>> {
    let t = distance_taper(goal)?;
    let (lo, hi) = t.value.volume_cut_frac;
    let (floor, cap) = crate::strength::TAPER_VOLUME_CUT_FRAC;
    let lo = lo.clamp(floor, cap);
    let hi = hi.clamp(lo, cap);
    let cut = if n <= 1 {
        hi
    } else {
        let k = k.clamp(1, n);
        lo + (hi - lo) * (k - 1) as f64 / (n - 1) as f64
    };
    Some(t.with_value(cut))
}

/// The quality sessions a week of `phase` schedules for `goal`, in priority
/// order (the plan takes as many as the week's quality budget allows).
/// running-024 goal table (5K: I, R, T; 10K: T, I, R; HM: T, MP, I; marathon:
/// T, MP) laid onto running-035's phases: Base = light tempo only; Build adds
/// the goal's main work; Peak is race-specific; Taper keeps 1–2 short sharp
/// sessions (running-038: I/T for 5K/10K, MP touches + short tempo for the
/// marathon). A recovery week (Deload) schedules none. RUN-PHASE-TID-001.
pub fn quality_mix(goal: GoalDistance, phase: MesoPhase) -> Recommended<&'static [RunSessionType]> {
    use GoalDistance::*;
    use MesoPhase::*;
    use RunSessionType::{Interval, RacePace, Repetition, Tempo};
    let mix: &'static [RunSessionType] = match (goal, phase) {
        (_, Deload) | (C25k, _) => &[],
        (General, _) | (_, Base) => &[Tempo],
        (FiveK, Build) => &[Interval, Tempo],
        (FiveK, Peak) => &[Interval, Repetition],
        (FiveK, Taper) | (TenK, Peak) | (TenK, Taper) => &[Interval, Tempo],
        (TenK, Build) | (HalfMarathon, Build) => &[Tempo, Interval],
        (HalfMarathon, Peak) | (HalfMarathon, Taper) => &[Tempo, RacePace],
        (Marathon, Build) => &[Tempo, RacePace],
        (Marathon, Peak) | (Marathon, Taper) => &[RacePace, Tempo],
    };
    crate::evidence::graded(mix, claims::RUN_PHASE_TID_001)
}

/// Goal long-run progression ceiling, km (running-046): half marathon
/// 16–21 km, marathon 29–32 km. Reached only through the spike rule, the
/// weekly ramp and the share cap. `None` for goals with no stated target.
pub fn goal_long_run_target_km(goal: GoalDistance) -> Option<Recommended<(f64, f64)>> {
    let t = match goal {
        GoalDistance::HalfMarathon => (16.0, 21.0),
        GoalDistance::Marathon => (29.0, 32.0),
        _ => return None,
    };
    Some(crate::evidence::graded(t, claims::RUN_LONGRUN_TARGET_001))
}

/// Weekly volume below which Daniels allows the long run a 30 % share
/// (running-046; ~40 mi/wk).
pub const LONG_RUN_SHARE_SPLIT_KM: f64 = 64.0;

/// The long run's share of weekly volume (running-046 / Daniels): at most 30 %
/// under 64 km/wk, 25 % at or above it.
pub fn long_run_share(weekly_km: f64) -> Recommended<f64> {
    let share = if weekly_km < LONG_RUN_SHARE_SPLIT_KM {
        LONG_RUN_SHARE_MAX
    } else {
        LONG_RUN_SHARE_MIN
    };
    crate::evidence::graded(share, claims::RUN_LONGRUN_TARGET_001)
}

/// Daniels' longest-steady-run time cap (running-016/046), minutes.
pub const LONG_RUN_MAX_MIN: f64 = 150.0;

// ---------------------------------------------------------------------------
// Warm-up / cool-down and repetition-session structure (running-050/051)
// ---------------------------------------------------------------------------

/// Warm-up / cool-down around a hard run (running-050 → RUN-WARMUP-001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WarmupRx {
    /// Easy-running warm-up before the main set, minutes (10–15).
    pub warmup_min: (u16, u16),
    /// Easy-running cool-down after it, minutes.
    pub cooldown_min: u16,
    /// Easy pace (s/km) the plan counts warm-up/cool-down minutes at when no
    /// pace anchor exists, so the week's km still adds up (an accounting
    /// default, never shown as a target): [`ACCOUNTING_EASY_PACE_SEC_PER_KM`].
    pub accounting_easy_pace_sec_per_km: f64,
}

/// Easy pace (6:30 /km) warm-up/cool-down minutes are counted at when no pace
/// anchor exists: the running-050 engineering default for weekly-volume
/// accounting only, never shown to the athlete as a target (RUN-WARMUP-001).
pub const ACCOUNTING_EASY_PACE_SEC_PER_KM: f64 = 390.0;

/// Precede threshold, interval, repetition and race-pace runs with 10–15 min
/// of easy running and follow them with ~10 min easy; both count toward the
/// week's volume (running-050). RUN-WARMUP-001.
pub fn warmup_rx() -> Recommended<WarmupRx> {
    crate::evidence::graded(
        WarmupRx {
            warmup_min: (10, 15),
            cooldown_min: 10,
            accounting_easy_pace_sec_per_km: ACCOUNTING_EASY_PACE_SEC_PER_KM,
        },
        claims::RUN_WARMUP_001,
    )
}

/// Repetition-session structure (running-051 → RUN-REPETITION-002).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepetitionRx {
    /// Rep distances, metres, longest first (400 m, then 200 m).
    pub rep_m: [u16; 2],
    /// Fewest reps that make a session.
    pub min_reps: u8,
    /// Most reps in one session.
    pub max_reps: u8,
    /// Recovery: easy jog as far as the rep (distance ratio 1.0).
    pub recovery_jog_ratio_of_rep_distance: f64,
    /// R work ≤5 % of weekly volume.
    pub weekly_cap_frac: f64,
}

/// Repetitions (running-051): 200 m or 400 m at R pace, an easy jog as far as
/// the rep between them, at least 4 and at most 12 reps, R total ≤5 % of the
/// week. RUN-REPETITION-002 (ExpertOpinion).
pub fn repetition_rx() -> Recommended<RepetitionRx> {
    crate::evidence::graded(
        RepetitionRx {
            rep_m: [400, 200],
            min_reps: 4,
            max_reps: 12,
            recovery_jog_ratio_of_rep_distance: 1.0,
            weekly_cap_frac: REPETITION_WEEKLY_CAP_FRAC,
        },
        claims::RUN_REPETITION_002,
    )
}

/// Marathon-pace volume limits (running-053 → RUN-RACEPACE-002).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RacePaceRx {
    /// M-pace running share of the week (min, max): 15–20 %.
    pub weekly_share_min: f64,
    pub weekly_share_max: f64,
    /// One M-pace run at most the lesser of 110 min or 29 km.
    pub max_min: u16,
    pub max_km: f64,
}

/// Marathon-pace work (running-053): about 15–20 % of weekly volume, and a
/// single M run no longer than the lesser of 110 min or 29 km (Daniels).
/// RUN-RACEPACE-002 (ExpertOpinion).
pub fn race_pace_rx() -> Recommended<RacePaceRx> {
    crate::evidence::graded(
        RacePaceRx {
            weekly_share_min: 0.15,
            weekly_share_max: 0.20,
            max_min: 110,
            max_km: 29.0,
        },
        claims::RUN_RACEPACE_002,
    )
}

// ---------------------------------------------------------------------------
// Pace re-testing & environment correction triggers (running-041)
// ---------------------------------------------------------------------------

/// Weeks after which training paces / HR zones are due for re-testing: the low
/// end of the KB's 4–6-week window (running-041 RUN-RETEST-001, running-006
/// RUN-HRRECALC-001).
pub const RETEST_DUE_WEEKS: u8 = 4;

/// Whether training paces are due for a re-test (running-041 →
/// RUN-RETEST-001): set paces from CURRENT fitness and re-test every 4–6
/// weeks. Due once [`RETEST_DUE_WEEKS`] have elapsed (same convention as
/// [`hr_zone_recalc_due`]).
pub fn pace_retest_due(weeks_since_test: u8) -> Recommended<bool> {
    crate::evidence::graded(weeks_since_test >= RETEST_DUE_WEEKS, claims::RUN_RETEST_001)
}

/// Freshness of the race result feeding a VDOT/CS pace computation
/// (running-041: "input recent race ≤6–8 wk, honest, flat/cool").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaceInputFreshness {
    /// ≤6 weeks old: inside the strict window.
    Fresh,
    /// 7–8 weeks old: inside the KB's soft 6–8-week margin.
    Marginal,
    /// >8 weeks old: outside even the soft window, re-test before trusting.
    Stale,
}

/// Classify race-input age against running-041's ≤6–8-week window.
pub fn race_input_freshness(weeks_since_race: u8) -> Recommended<RaceInputFreshness> {
    let f = if weeks_since_race <= 6 {
        RaceInputFreshness::Fresh
    } else if weeks_since_race <= 8 {
        RaceInputFreshness::Marginal
    } else {
        RaceInputFreshness::Stale
    };
    crate::evidence::graded(f, claims::RUN_RETEST_001)
}

/// Environment-correction triggers for prescribed paces (running-041).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaceCorrectionTriggers {
    /// Heat correction needed: air temperature > ~15 °C (a conservative
    /// flags-only trigger; see [`pace_correction_triggers`]).
    pub heat: bool,
    /// Altitude correction needed: elevation > ~900 m.
    pub altitude: bool,
}

/// Air temperature (°C) above which a heat pace correction is flagged
/// (running-041; RUN-RETEST-001). See [`pace_correction_triggers`].
pub const PACE_CORRECTION_HEAT_C: f64 = 15.0;

/// Altitude (m) above which an altitude pace correction is flagged
/// (running-041; RUN-RETEST-001).
pub const PACE_CORRECTION_ALTITUDE_M: f64 = 900.0;

/// Whether pace corrections are triggered (running-041): heat above
/// [`PACE_CORRECTION_HEAT_C`], altitude above [`PACE_CORRECTION_ALTITUDE_M`].
/// Returns FLAGS only — the evidence pins trigger
/// thresholds, never correction magnitudes (sec/km or %), so no adjusted pace is
/// ever emitted. The ~15 °C heat threshold is a CONSERVATIVE practical trigger,
/// NOT the physiological degradation onset: marathon pace optimises at ~4–10 °C
/// air temp and degrades continuously above it (El Helou 2012, PMID 22649525),
/// with the first statistically-significant slowing at WBGT >~15 °C (Ely 2007,
/// PMID 17473775) — so 15 °C avoids flagging imperceptible sub-15 °C loss. NB the
/// engine reads dry-bulb air temp only; true heat stress is WBGT (adds humidity),
/// so a fixed air-temp trigger under-fires in humid air (documented limitation).
pub fn pace_correction_triggers(
    temp_c: f64,
    altitude_m: f64,
) -> Recommended<PaceCorrectionTriggers> {
    crate::evidence::graded(
        PaceCorrectionTriggers {
            heat: temp_c > PACE_CORRECTION_HEAT_C,
            altitude: altitude_m > PACE_CORRECTION_ALTITUDE_M,
        },
        claims::RUN_RETEST_001,
    )
}

// ---------------------------------------------------------------------------
// Marathon realism gate (running-040)
// ---------------------------------------------------------------------------

/// Longest training run (km) under which a marathon goal-time prediction is
/// optimistic (running-040; RUN-VDOT-001).
pub const MARATHON_REALISM_LONG_RUN_KM: f64 = 30.0;

/// Flag a marathon goal-time prediction as optimistic when the longest training
/// run is under [`MARATHON_REALISM_LONG_RUN_KM`]. Rule running-040: first-timers
/// under this threshold often finish 10–15% slower than a Riegel-from-5K
/// projection, so the estimate should be derated (~2–3 VDOT points). Returns
/// `true` when the gate trips.
pub fn marathon_prediction_optimistic(longest_run_km: f64) -> Recommended<bool> {
    crate::evidence::graded(
        longest_run_km < MARATHON_REALISM_LONG_RUN_KM,
        claims::RUN_VDOT_001,
    )
}

// ---------------------------------------------------------------------------
// Race-time equivalency combine (running-039)
// ---------------------------------------------------------------------------

/// Result of combining two race-time predictions. Rule running-039.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Equivalency {
    /// The two methods agree within ~2%; use the midpoint (seconds).
    Agreed(f64),
    /// The methods diverge; present the span as a range (low, high) seconds.
    Range(f64, f64),
}

/// Combine a Riegel and a Daniels/VDOT goal-time prediction. Rule running-039:
/// if the two agree within ~2% return their midpoint, otherwise return the
/// span as a range rather than a single false-precision number.
pub fn race_equivalency(riegel_sec: f64, daniels_sec: f64) -> Recommended<Equivalency> {
    let lo = riegel_sec.min(daniels_sec);
    let hi = riegel_sec.max(daniels_sec);
    // A 0.0 sentinel (an undefined prediction) makes the agreement ratio
    // meaningless (division by zero → Inf/NaN); present the raw span rather than
    // a false-precision midpoint.
    if riegel_sec <= 0.0 || daniels_sec <= 0.0 {
        return crate::evidence::graded(Equivalency::Range(lo, hi), claims::RUN_EQUIV_001);
    }
    let rel_diff = (hi - lo) / lo;
    let out = if rel_diff <= 0.02 {
        Equivalency::Agreed((riegel_sec + daniels_sec) / 2.0)
    } else {
        Equivalency::Range(lo, hi)
    };
    // running-039: the Riegel/Daniels equivalency combiner (RUN-EQUIV-001), not
    // the VDOT-fitness estimate (RUN-VDOT-001).
    crate::evidence::graded(out, claims::RUN_EQUIV_001)
}

// ---------------------------------------------------------------------------
// HR-method preference & zone recalculation (running-005/006)
// ---------------------------------------------------------------------------

/// Which HR-anchoring method running-005 prefers at a given resting HR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HrMethodPreference {
    /// RHR < 55: prefer Karvonen (%HRR); the methods diverge substantially.
    PreferKarvonen,
    /// RHR ≥ 70: the methods converge; either %HRmax or Karvonen is fine.
    EitherConverged,
    /// RHR in [55, 70): the KB states no rule for this range.
    Unstated,
}

/// HR-method selection (running-005 → RUN-KARVONEN-001, Moderate): prefer
/// Karvonen (%HRR) below RHR 55 where the methods diverge; at RHR ≥70 they
/// converge and either is acceptable. The KB gives NO rule for RHR 55–69 -
/// that range returns [`HrMethodPreference::Unstated`] rather than an invented
/// default. Formulas (verbatim): %HRmax target = HRmax×%; Karvonen target =
/// ((HRmax−RHR)×%)+RHR (the latter is `load::karvonen_target_hr`).
pub fn hr_method_preference(resting_hr_bpm: f64) -> Recommended<HrMethodPreference> {
    let p = if resting_hr_bpm < 55.0 {
        HrMethodPreference::PreferKarvonen
    } else if resting_hr_bpm >= 70.0 {
        HrMethodPreference::EitherConverged
    } else {
        HrMethodPreference::Unstated
    };
    crate::evidence::graded(p, claims::RUN_KARVONEN_001)
}

/// Prefer Karvonen (%HRR) over %HRmax when resting HR is low, where the two
/// methods diverge substantially (running-005): true below RHR 55; at RHR ≥70
/// they converge and either is acceptable. Boolean view of
/// [`hr_method_preference`]. RUN-KARVONEN-001.
pub fn prefer_karvonen(resting_hr_bpm: f64) -> Recommended<bool> {
    let p = hr_method_preference(resting_hr_bpm);
    p.with_value(p.value == HrMethodPreference::PreferKarvonen)
}

/// Whether HR training zones are due for recalculation (running-006): recompute
/// every 4–6 weeks off a measured max HR. Due once [`RETEST_DUE_WEEKS`] have
/// elapsed. RUN-HRRECALC-001 (Strong, safety-critical, stale zones misplace
/// every prescription).
pub fn hr_zone_recalc_due(weeks_since_recalc: u8) -> Recommended<bool> {
    crate::evidence::graded(
        weeks_since_recalc >= RETEST_DUE_WEEKS,
        claims::RUN_HRRECALC_001,
    )
}

// ---------------------------------------------------------------------------
// VDOT derate & goal-distance session plan (running-008/024/025)
// ---------------------------------------------------------------------------

/// A training goal by target race distance (running-024 goal table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GoalDistance {
    General,
    C25k,
    FiveK,
    TenK,
    HalfMarathon,
    Marathon,
}

/// VDOT points to subtract for an under-mileaged runner at longer race
/// distances (running-008): ~1–1.5 pts at the half, ~2–3 pts at the marathon;
/// no derate at shorter distances or for adequately-mileaged runners. Returns
/// (min, max) points. RUN-VDOT-001.
pub fn vdot_derate_points(goal: GoalDistance, under_mileaged: bool) -> Recommended<(f64, f64)> {
    let d = if under_mileaged {
        match goal {
            GoalDistance::HalfMarathon => (1.0, 1.5),
            GoalDistance::Marathon => (2.0, 3.0),
            _ => (0.0, 0.0),
        }
    } else {
        (0.0, 0.0)
    };
    crate::evidence::graded(d, claims::RUN_VDOT_001)
}

/// Derate an optimistic marathon finish-time band for an under-mileaged runner
/// (running-008/040, option B, "flag AND derate"). The prediction band
/// `(low_sec, high_sec)` is shifted SLOWER by the same VDOT-point derate that
/// [`vdot_derate_points`] cites for the marathon: the fast bound is slowed by
/// the *minimum* derate and the slow bound by the *maximum*, so the displayed
/// range moves later (pace easier) and widens by the derate's own uncertainty.
/// `base_vdot` (from the runner's recent race) anchors the point→time
/// conversion through [`daniels_predict`](crate::load::daniels_predict) at the marathon distance, no
/// new magnitude is invented, it is the same 2–3 VDOT points the caveat states.
/// Carries RUN-VDOT-001 so the adjusted number still travels with its grade
/// (HARD RULE 2). A degenerate band, a VDOT the derate would take to zero or
/// below, or a prediction that fails is returned unchanged, so no bound ever
/// renders as a 0:00 finish.
pub fn marathon_derated_band(
    low_sec: f64,
    high_sec: f64,
    base_vdot: f64,
) -> Recommended<(f64, f64)> {
    let derate = vdot_derate_points(GoalDistance::Marathon, true);
    let (min_pts, max_pts) = derate.value;
    let base_time = crate::load::daniels_predict(base_vdot, MARATHON_M);
    // Translate the VDOT-point derate into a time slowdown factor via the same
    // Daniels marathon prediction the band is built from.
    let derated = (base_vdot - max_pts > 0.0 && base_time > 0.0 && low_sec > 0.0 && high_sec > 0.0)
        .then(|| {
            (
                crate::load::daniels_predict(base_vdot - min_pts, MARATHON_M),
                crate::load::daniels_predict(base_vdot - max_pts, MARATHON_M),
            )
        })
        .filter(|(t_min, t_max)| *t_min > 0.0 && *t_max > 0.0);
    let (lo, hi) = match derated {
        Some((t_min, t_max)) => (low_sec * t_min / base_time, high_sec * t_max / base_time),
        None => (low_sec, high_sec),
    };
    derate.with_value((lo, hi))
}

/// Weekly session/quality budget for a goal distance (running-024 goal table).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoalWeekPlan {
    /// Total run sessions per week (min, max).
    pub sessions_per_week: (u8, u8),
    /// Quality (hard) sessions per week (min, max).
    pub quality_per_week: (u8, u8),
    /// Long-run share of weekly volume as a fraction (min, max).
    pub long_run_share: (f64, f64),
}

/// Sessions + quality per week by goal distance (running-024). Marathon
/// `advanced` runners extend to 5–7 sessions. Long-run share is 25–30% of
/// weekly volume for the distance-race rows; C25K carries no long run
/// (running-025 is run/walk to 30 min), so its share is 0. RUN-WORKOUT-001.
///
/// The long-run share follows running-016 (25–30 %) over running-024's 20–30 %
/// (see planning/knowledge-base/long-run-weekly-volume-share.md): 25 % is
/// Daniels' prescribed long-run share (and cap) for runners at/above ~64 km/wk,
/// firsthand-verified, while running-024's 20 % lower bound is uncited. The band
/// matches [`long_run_share_default`].
pub fn goal_week_plan(goal: GoalDistance, advanced: bool) -> Recommended<GoalWeekPlan> {
    let lr = (LONG_RUN_SHARE_MIN, LONG_RUN_SHARE_MAX);
    let plan = match goal {
        GoalDistance::General => GoalWeekPlan {
            sessions_per_week: (3, 5),
            quality_per_week: (0, 1),
            long_run_share: lr,
        },
        GoalDistance::C25k => GoalWeekPlan {
            sessions_per_week: (3, 3),
            quality_per_week: (0, 0),
            // running-025 defines no long run for C25K (run/walk to 30 min); the
            // 25-30% long-run share belongs to the distance-race rows only.
            long_run_share: (0.0, 0.0),
        },
        GoalDistance::FiveK | GoalDistance::TenK | GoalDistance::HalfMarathon => GoalWeekPlan {
            sessions_per_week: (4, 6),
            quality_per_week: (2, 2),
            long_run_share: lr,
        },
        GoalDistance::Marathon => GoalWeekPlan {
            sessions_per_week: if advanced { (5, 7) } else { (4, 6) },
            quality_per_week: (2, 2),
            long_run_share: lr,
        },
    };
    crate::evidence::graded(plan, claims::RUN_WORKOUT_001)
}

/// Couch-to-5K beginner protocol (running-025): 3 run/walk sessions per week at
/// conversational effort with a rest day between, over 9 weeks (extendable to
/// 10–12); repeat any too-hard week without penalty. RUN-C25K-001.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct C25kPlan {
    pub runs_per_week: u8,
    /// Program length in weeks (nominal, extended).
    pub weeks: (u8, u8),
    pub rest_day_between: bool,
    /// A too-hard week may be repeated without penalty.
    pub repeat_hard_week_allowed: bool,
}

/// The Couch-to-5K default plan (running-025). RUN-C25K-001 (ExpertOpinion,
/// safety-critical beginner guardrail).
pub fn c25k_plan() -> Recommended<C25kPlan> {
    crate::evidence::graded(
        C25kPlan {
            runs_per_week: 3,
            weeks: (9, 12),
            rest_day_between: true,
            repeat_hard_week_allowed: true,
        },
        claims::RUN_C25K_001,
    )
}

/// One Couch to 5K session: a brisk warm-up walk, then the run/walk bouts in
/// order (each bout runs `run_sec`, then walks `walk_sec`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct C25kSession {
    pub warmup_walk_min: u16,
    pub bouts: Vec<RunSegment>,
}

impl C25kSession {
    /// Total running seconds in the session (warm-up walk excluded).
    pub fn run_sec(&self) -> u32 {
        self.bouts.iter().map(|b| b.run_sec as u32).sum()
    }

    /// Longest single running bout, seconds.
    pub fn longest_bout_sec(&self) -> u16 {
        self.bouts.iter().map(|b| b.run_sec).max().unwrap_or(0)
    }

    /// The session as a `RunVolume`: a uniform run/walk set stays a `RunWalk`
    /// (weeks 1–2); anything else is its whole duration (bouts incl. walks,
    /// warm-up walk excluded), rounded up to whole minutes.
    pub fn volume(&self) -> RunVolume {
        let first = self.bouts.first().copied().unwrap_or(RunSegment {
            run_sec: 0,
            walk_sec: 0,
        });
        let uniform = self.bouts.len() > 1 && self.bouts.iter().all(|b| *b == first);
        if uniform && first.walk_sec > 0 {
            return RunVolume::RunWalk {
                run_sec: first.run_sec,
                walk_sec: first.walk_sec,
                rounds: self.bouts.len().min(u8::MAX as usize) as u8,
            };
        }
        let total: u32 = self
            .bouts
            .iter()
            .map(|b| b.run_sec as u32 + b.walk_sec as u32)
            .sum();
        RunVolume::DurationMin(total.div_ceil(60).min(u16::MAX as u32) as u16)
    }
}

/// The NHS Couch to 5K session for programme week `week` (1-based) and the
/// `run_of_week`-th run of that week (1-based; weeks 5 and 6 differ by run),
/// transcribed from the NHS week table (running-049): a 5-minute brisk walk,
/// then
/// - wk1: 8 × (60 s run, 90 s walk); wk2: 6 × (90 s run, 2 min walk);
/// - wk3: 2 × (90 s run, 90 s walk, 3 min run, 3 min walk);
/// - wk4: 3 run, 90 s walk, 5 run, 2.5 walk, 3 run, 90 s walk, 5 run (min);
/// - wk5: run 1 = 3 × 5 min with 3 min walks; run 2 = 8, 5 walk, 8; run 3 = 20;
/// - wk6: run 1 = 5, 3 walk, 8, 3 walk, 5; run 2 = 10, 3 walk, 10; run 3 = 25;
/// - wk7: 25 min; wk8: 28 min; wk9 (and any repeat beyond): 30 min continuous.
///
/// Weeks advance only when completed (repeat a week without penalty).
/// RUN-C25K-002 (ExpertOpinion, safety-critical beginner guardrail).
pub fn c25k_session_rx(week: u8, run_of_week: u8) -> Recommended<C25kSession> {
    let b = |run_sec: u16, walk_sec: u16| RunSegment { run_sec, walk_sec };
    let run = run_of_week.clamp(1, 3);
    let bouts = match week.max(1) {
        1 => vec![b(60, 90); 8],
        2 => vec![b(90, 120); 6],
        3 => vec![b(90, 90), b(180, 180), b(90, 90), b(180, 180)],
        4 => vec![b(180, 90), b(300, 150), b(180, 90), b(300, 0)],
        5 => match run {
            1 => vec![b(300, 180), b(300, 180), b(300, 0)],
            2 => vec![b(480, 300), b(480, 0)],
            _ => vec![b(1200, 0)],
        },
        6 => match run {
            1 => vec![b(300, 180), b(480, 180), b(300, 0)],
            2 => vec![b(600, 180), b(600, 0)],
            _ => vec![b(1500, 0)],
        },
        7 => vec![b(1500, 0)],
        8 => vec![b(1680, 0)],
        _ => vec![b(1800, 0)],
    };
    crate::evidence::graded(
        C25kSession {
            warmup_walk_min: 5,
            bouts,
        },
        claims::RUN_C25K_002,
    )
}

// ---------------------------------------------------------------------------
// Maffetone MAF aerobic cap (running-036)
// ---------------------------------------------------------------------------

/// Maffetone-cap adjustment bracket (running-036).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MafAdjustment {
    /// Elite / 2+ years injury-free and improving: +5 bpm.
    EliteImproving,
    /// No adjustment (default).
    None,
    /// Returning from injury/illness or 2+ colds/yr: −5 bpm.
    Returning,
    /// Chronically overtrained/sedentary/on meds: −10 bpm.
    Overtrained,
}

/// Maffetone aerobic-base HR cap: `180 − age` with a category adjustment
/// (running-036). Offered as a base-phase OPTION, never the default; personalize
/// toward measured LT1 when data exist. Weak/contested (global CQ-12, MAF vs
/// measured LT1). RUN-MAF-001.
pub fn maf_cap_bpm(age_years: f64, adj: MafAdjustment) -> Recommended<f64> {
    let delta = match adj {
        MafAdjustment::EliteImproving => 5.0,
        MafAdjustment::None => 0.0,
        MafAdjustment::Returning => -5.0,
        MafAdjustment::Overtrained => -10.0,
    };
    crate::evidence::graded(180.0 - age_years + delta, claims::RUN_MAF_001)
}

// ---------------------------------------------------------------------------
// Distribution invariant & counting method (running-011/012)
// ---------------------------------------------------------------------------

/// Enforce the architectural invariant that ~80% of running time is easy
/// (running-011): true when the easy (Z1) time share is at least 80%. RUN-DIST-001.
pub fn easy_share_floor_ok(easy_frac_by_time: f64) -> Recommended<bool> {
    crate::evidence::graded(easy_frac_by_time >= 0.80, claims::RUN_DIST_001)
}

/// Intensity-counting method (running-012). Pick ONE per athlete and declare it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntensityCountingMethod {
    /// Time spent in each zone, default for distribution *reporting*.
    TimeInZone,
    /// Primary session goal, default for plan *design*.
    SessionGoal,
}

/// Default intensity-counting method by use (running-012): session-goal for plan
/// design, time-in-zone for distribution reporting. RUN-DIST-001.
pub fn default_counting_method(for_plan_design: bool) -> Recommended<IntensityCountingMethod> {
    let m = if for_plan_design {
        IntensityCountingMethod::SessionGoal
    } else {
        IntensityCountingMethod::TimeInZone
    };
    crate::evidence::graded(m, claims::RUN_DIST_001)
}

// ---------------------------------------------------------------------------
// Volume-bump hold (running-031)
// ---------------------------------------------------------------------------

/// Weeks to hold before the next volume bump for novices (running-031):
/// `Some((2, 3))` under one year of training age; `None` for experienced runners
/// (the source specifies no fixed hold beyond a ~5%/absolute cap). RUN-PROGRESS-001.
pub fn novice_volume_bump_hold_weeks(training_age_years: f64) -> Recommended<Option<(u8, u8)>> {
    let hold = if training_age_years < 1.0 {
        Some((2, 3))
    } else {
        None
    };
    crate::evidence::graded(hold, claims::RUN_PROGRESS_001)
}

// ---------------------------------------------------------------------------
// GPS track geometry (pure geodesy, not a coached claim, no evidence tag)
// ---------------------------------------------------------------------------

/// Mean Earth radius (metres) for the haversine great-circle distance.
const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Fixes with worse horizontal accuracy than this (metres) are noise; they are
/// dropped before deriving distance *and* duration so both are computed from the
/// same usable slice (otherwise a noisy first/last fix skews derived pace).
pub const MAX_GPS_ACCURACY_M: f32 = 30.0;

/// One GPS fix from a run track. Deterministic core input: wall-clock time
/// enters only as `observed_at` (unix seconds), never a live clock. Lat/lon in
/// decimal degrees; `accuracy_m` is the shell provider's reported horizontal
/// error, used to drop noisy fixes before summing distance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GpsPoint {
    pub lat: f64,
    pub lon: f64,
    pub observed_at: i64,
    pub accuracy_m: f32,
}

/// Great-circle distance between two fixes, in metres (haversine).
pub fn haversine_m(a: GpsPoint, b: GpsPoint) -> f64 {
    let lat1 = a.lat.to_radians();
    let lat2 = b.lat.to_radians();
    let dlat = (b.lat - a.lat).to_radians();
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    // Clamp `h` to `1.0` before `asin`: for near-antipodal (valid) coordinates
    // rounding can push `h` a hair above 1.0, making `sqrt(h) > 1` and
    // `asin` return NaN. The clamp is standard haversine insurance, cheap for
    // any future non-`qc_track` caller that doesn't already drop NaN-speed legs.
    2.0 * EARTH_RADIUS_M * h.min(1.0).sqrt().asin()
}

/// One MOVING leg between consecutive fixes as `(dt_seconds, distance_m)`.
/// `None` when the timestamps don't advance or the ground speed is below the
/// auto-pause floor (`load::is_stopped`, <0.5 m/s, File 07), so every moving
/// time/distance derivation pauses on the same legs. Saturating: a corrupt
/// stamp pair (i64::MIN → i64::MAX) cannot overflow, and the dt gate discards
/// the saturated value. Distance is raw haversine; callers that need jitter
/// rejection cap it at [`MAX_PLAUSIBLE_SPEED_MPS`].
pub(crate) fn leg(a: GpsPoint, b: GpsPoint) -> Option<(f64, f64)> {
    let dt = b.observed_at.saturating_sub(a.observed_at) as f64;
    if dt <= 0.0 {
        return None;
    }
    let d = haversine_m(a, b);
    (!crate::load::is_stopped(d / dt)).then_some((dt, d))
}

/// The subset of fixes good enough to trust, in original order. Shared by
/// distance, duration, and GPX export so all three describe the same track, an
/// exported file opened in Strava/Garmin shows the same distance the app does.
pub fn usable_track(points: &[GpsPoint], max_accuracy_m: f32) -> Vec<GpsPoint> {
    points
        .iter()
        .copied()
        .filter(|p| p.accuracy_m <= max_accuracy_m)
        .collect()
}

/// Split `points` into recording SEGMENTS at `segment_starts`, the indices that
/// BEGIN a new segment (the first fix captured after a pause/resume or a long
/// gap in fixes). The leg from `points[i-1]` to `points[i]` at such an index is a
/// PAUSE BRIDGE the runner may have relocated across, so no track metric may sum
/// distance or time across it. An EMPTY `segment_starts` (a re-anchored log or
/// a hand-logged run) yields the WHOLE track as a single segment, so every
/// metric routed through this reduces BIT-IDENTICALLY to its whole-track form.
/// Out-of-range, zero, and duplicate indices are ignored,
/// so a malformed boundary list can only under-split, never panic.
pub(crate) fn segments<'a>(points: &'a [GpsPoint], segment_starts: &[u32]) -> Vec<&'a [GpsPoint]> {
    let mut bounds: Vec<usize> = segment_starts
        .iter()
        .map(|&i| i as usize)
        .filter(|&i| i > 0 && i < points.len())
        .collect();
    bounds.sort_unstable();
    bounds.dedup();
    if bounds.is_empty() {
        return vec![points];
    }
    let mut out = Vec::with_capacity(bounds.len() + 1);
    let mut prev = 0usize;
    for b in bounds {
        out.push(&points[prev..b]);
        prev = b;
    }
    out.push(&points[prev..]);
    out
}

/// The usable fixes of every segment, flattened in order, each paired with a flag
/// that is `true` for the FIRST usable fix of its segment, i.e. the leg ENTERING
/// that fix is a pause bridge and must contribute neither distance nor time.
/// Accuracy filtering happens PER segment, which yields the same usable set as
/// filtering the whole track. For an empty `segment_starts` the flag is `true`
/// only at index 0 (which has no entering leg), so every downstream cumulative
/// walk is bit-identical to the single-track behaviour it replaces.
fn usable_segments(
    points: &[GpsPoint],
    segment_starts: &[u32],
    max_accuracy_m: f32,
) -> (Vec<GpsPoint>, Vec<bool>) {
    let mut pts = Vec::with_capacity(points.len());
    let mut is_start = Vec::with_capacity(points.len());
    for seg in segments(points, segment_starts) {
        let mut first = true;
        for p in usable_track(seg, max_accuracy_m) {
            pts.push(p);
            is_start.push(first);
            first = false;
        }
    }
    (pts, is_start)
}

/// Total track distance in km: the sum of the MOVING legs. Fixes whose reported
/// horizontal accuracy is worse than `max_accuracy_m` are dropped first, a leg
/// below the auto-pause floor (`load::is_stopped`, <0.5 m/s) contributes
/// nothing, and every leg is capped at [`MAX_PLAUSIBLE_SPEED_MPS`], so GPS
/// wander accumulated while standing still is not counted as distance. These
/// are the same gates the displayed pace, variation index and splits apply, so
/// distance-of-record and moving pace describe the same track. Pure and
/// order-dependent (order = fix order from the shell). The bare form treats the
/// whole track as one segment; see [`track_distance_km_seg`] for the
/// pause-bridge-aware form.
pub fn track_distance_km(points: &[GpsPoint], max_accuracy_m: f32) -> f64 {
    track_distance_km_seg(points, max_accuracy_m, &[])
}

/// [`track_distance_km`] that excludes each pause-bridge leg (a leg entering a
/// `segments` boundary) so a paused relocation contributes no distance, the
/// true coordinates stay put (no re-anchoring shift). Stop floor and jitter cap
/// apply per segment. Empty `segment_starts` is bit-identical to
/// [`track_distance_km`].
pub fn track_distance_km_seg(
    points: &[GpsPoint],
    max_accuracy_m: f32,
    segment_starts: &[u32],
) -> f64 {
    moving_legs(points, max_accuracy_m, segment_starts)
        .iter()
        .map(|(_, d)| d)
        .sum::<f64>()
        / 1000.0
}

/// Cumulative MOVING distance in metres to each usable fix (`out[0] == 0.0`). A
/// leg contributes distance ONLY when it is moving — its timestamps advance and
/// its ground speed clears the auto-pause floor (`load::is_stopped`, <0.5 m/s) —
/// and its distance is capped to the plausible-runner ceiling, the SAME gates as
/// [`moving_legs`]/[`track_distance_km`]. A pause-bridge leg entering a segment
/// start adds nothing. This keeps the split walk on the headline distance, so a
/// stopped run's below-floor drift does not inflate the cumulative total.
fn cumulative_moving_m(usable: &[GpsPoint], is_start: &[bool]) -> Vec<f64> {
    let mut cumulative = Vec::with_capacity(usable.len());
    let mut running_total = 0.0;
    cumulative.push(0.0);
    for i in 1..usable.len() {
        let leg_m = if is_start[i] {
            0.0
        } else {
            leg(usable[i - 1], usable[i]).map_or(0.0, |(dt, d)| d.min(MAX_PLAUSIBLE_SPEED_MPS * dt))
        };
        running_total += leg_m;
        cumulative.push(running_total);
    }
    cumulative
}

/// Second-half-vs-first-half pace slowdown across a track, as a percent (a
/// *positive split*: positive = the runner slowed in the back half). The track
/// is split at its halfway distance and each half's pace (time ÷ distance) is
/// compared; units cancel in the ratio. Purely descriptive, this is a
/// measurement of the run, not a recommendation. Returns `None` when the track
/// is too short, degenerate, or non-monotonic to split meaningfully (fewer than
/// three usable fixes, a zero-distance/zero-duration half). Same accuracy gate
/// as [`track_distance_km`] so both agree on the track.
pub fn track_positive_split_pct(points: &[GpsPoint], max_accuracy_m: f32) -> Option<f64> {
    track_positive_split_pct_seg(points, max_accuracy_m, &[])
}

/// [`track_positive_split_pct`] that treats each pause-bridge leg as zero distance
/// and zero time, so the
/// halfway split and each half's moving pace are measured over the true route
/// without the relocation displacement. Empty `segment_starts` is bit-identical
/// to [`track_positive_split_pct`].
pub fn track_positive_split_pct_seg(
    points: &[GpsPoint],
    max_accuracy_m: f32,
    segment_starts: &[u32],
) -> Option<f64> {
    let (usable, is_start) = usable_segments(points, segment_starts, max_accuracy_m);
    if usable.len() < 3 {
        return None;
    }
    // Cumulative MOVING distance to each fix (a pause-bridge or below-floor leg
    // adds nothing), so the halfway split and each half sit on the same distance
    // the headline uses (see `cumulative_moving_m`).
    let cumulative = cumulative_moving_m(&usable, &is_start);
    let total = *cumulative.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return None;
    }
    // First interior fix that reaches the halfway distance is the split point.
    let half = total / 2.0;
    let split = (1..usable.len() - 1).find(|&i| cumulative[i] >= half)?;

    let dist1 = cumulative[split];
    let dist2 = total - dist1;
    // Each half's time is MOVING seconds only: legs below the auto-pause floor
    // (`load::is_stopped`, <0.5 m/s) AND pause-bridge legs contribute distance-or-
    // relocation but no time, so a mid-run café stop or a paused relocation can't
    // produce a false "FADE" verdict. This matches the moving-time base of the
    // run's displayed pace, its km-splits, and its VI, which wall-clock halves
    // would contradict on any run with a pause.
    let moving_leg = |i: usize| -> f64 {
        if is_start[i] {
            return 0.0;
        }
        leg(usable[i - 1], usable[i]).map_or(0.0, |(dt, _)| dt)
    };
    let dur1: f64 = (1..=split).map(moving_leg).sum();
    let dur2: f64 = (split + 1..usable.len()).map(moving_leg).sum();
    if dist1 <= 0.0 || dist2 <= 0.0 || dur1 <= 0.0 || dur2 <= 0.0 {
        return None;
    }
    let pace1 = dur1 / dist1;
    let pace2 = dur2 / dist2;
    Some(((pace2 - pace1) / pace1 * 100.0 * 10.0).round() / 10.0)
}

/// One kilometre in metres, the per-km split unit.
pub const KM_M: f64 = 1000.0;

/// One international mile in metres (exact), the per-mile split unit.
pub const MILE_M: f64 = 1609.344;

/// One completed (or final partial) split of a run track. Purely descriptive -
/// a measurement of the run, not a recommendation, so it carries no evidence tag
/// (same standing as [`track_positive_split_pct`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunSplit {
    /// 1-based split index (1 = first km/mile).
    pub index: u32,
    /// Pace over this split, seconds per unit distance (per km, or per mile),
    /// timed in MOVING seconds (stopped legs excluded, see [`track_splits`])
    /// so it lines up with the run's moving-time headline pace.
    /// For the final partial split the pace is normalized to a full unit
    /// (`duration / covered_distance × unit`) so it stays comparable to the full
    /// splits rather than looking artificially fast over a short remainder.
    pub pace_sec_per_unit: f64,
    /// Cumulative track distance at the END of this split, metres.
    pub cumulative_m: f64,
    /// Distance actually covered by this split, metres. Equals the unit distance
    /// for a full split; smaller for the final partial split.
    pub distance_m: f64,
    /// True only for a final split shorter than a full unit (`distance_m < unit`).
    pub partial: bool,
}

/// Per-unit run splits from a GPS track: one [`RunSplit`] per completed
/// `unit_m`-metre interval (pass [`KM_M`] for kilometre splits, [`MILE_M`] for
/// miles), plus a final PARTIAL split when the track ends mid-unit (so a 5.4 km
/// run yields 5 full km splits + 1 partial). Split boundaries fall at exact unit
/// distances (apportioned within the crossing leg at its constant speed), so a
/// split's pace does not depend on where GPS fixes happened to land, but each
/// split is TIMED in MOVING seconds and MEASURED in moving distance only: a leg
/// whose ground speed is below the auto-pause floor (`load::is_stopped`,
/// <0.5 m/s) or whose timestamps don't advance contributes neither time nor
/// distance, exactly as `moving_duration_min` / `track_distance_km` exclude it.
/// So a mid-split café/traffic stop cannot make that split read minutes slower
/// than the run's own moving-pace average, and the splits sum to the headline
/// moving distance — an all-stopped track has zero moving distance and yields no
/// splits. Same accuracy gate as [`track_distance_km`], and the same
/// moving-distance walk as [`track_positive_split_pct`]. Pure and
/// order-dependent. Returns an empty vec for a sub-metre unit, an empty /
/// single-fix / zero-distance track (nothing to split).
pub fn track_splits(points: &[GpsPoint], max_accuracy_m: f32, unit_m: f64) -> Vec<RunSplit> {
    track_splits_seg(points, max_accuracy_m, unit_m, &[])
}

/// Hard cap on emitted splits (see [`track_splits_seg`]), bounding per-`view()`
/// work against a corrupt track. 10 k km/mile splits is far past any real run.
const MAX_SPLITS: usize = 10_000;

/// [`track_splits`] whose cumulative walk skips each pause-bridge leg (zero
/// distance, so no split boundary falls inside a paused relocation), letting km
/// splits continue seamlessly across a pause on the true route. Empty
/// `segment_starts` is bit-identical to [`track_splits`].
pub fn track_splits_seg(
    points: &[GpsPoint],
    max_accuracy_m: f32,
    unit_m: f64,
    segment_starts: &[u32],
) -> Vec<RunSplit> {
    // Reject a sub-metre unit. The boundary loop below emits one split per
    // `unit_m` of distance, so a tiny unit (a caller bug / corrupt input) would
    // spin for millions of iterations. Every real caller passes km/mile.
    if unit_m < 1.0 {
        return Vec::new();
    }
    let (usable, is_start) = usable_segments(points, segment_starts, max_accuracy_m);
    if usable.len() < 2 {
        return Vec::new();
    }
    // Cumulative MOVING distance to each fix (a pause-bridge or below-floor leg
    // adds nothing), so km-split boundaries and the total track the headline
    // distance and the splits sum to it (see `cumulative_moving_m`).
    let cumulative = cumulative_moving_m(&usable, &is_start);
    let total = *cumulative.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return Vec::new();
    }

    let mut splits = Vec::new();
    let mut index = 1u32;
    let mut boundary = unit_m;
    // Time accumulated into the CURRENT split since the last unit boundary.
    let mut moving_s = 0.0; // the split's pace clock (moving legs only)
    for i in 1..usable.len() {
        let leg_d = cumulative[i] - cumulative[i - 1];
        let dt = (usable[i]
            .observed_at
            .saturating_sub(usable[i - 1].observed_at) as f64)
            .max(0.0);
        // Same pause gate as `leg`, read off the leg's gated cumulative distance
        // (0 for a paused or pause-bridge leg).
        let moving = dt > 0.0 && !crate::load::is_stopped(leg_d / dt);
        // Distance already consumed within this leg (start of unconsumed part).
        let mut pos = cumulative[i - 1];
        // Emit every unit boundary that falls inside this leg; the leg's time
        // is apportioned to each side at the leg's constant speed.
        while boundary <= cumulative[i] + 1e-6 {
            let sub = if leg_d > 0.0 {
                dt * ((boundary - pos) / leg_d)
            } else {
                0.0
            };
            if moving {
                moving_s += sub;
            }
            let dur = moving_s;
            splits.push(RunSplit {
                index,
                // Full split: distance == unit, so pace IS the (moving) time.
                pace_sec_per_unit: dur,
                cumulative_m: boundary,
                distance_m: unit_m,
                partial: false,
            });
            // Even at a legitimate unit, a corrupt track (a mis-unit'd
            // coordinate inflating cumulative distance to thousands of km) could
            // emit millions of splits in `view()`. Bound the list: a genuine
            // run never approaches this many.
            if splits.len() >= MAX_SPLITS {
                return splits;
            }
            pos = boundary;
            moving_s = 0.0;
            index += 1;
            boundary = unit_m * index as f64;
        }
        // Remainder of the leg past the last boundary feeds the next split.
        // Only a moving leg advances the split clock; a paused / zero-distance
        // leg (leg_d == 0) contributes neither time nor distance.
        if moving {
            moving_s += dt * ((cumulative[i] - pos) / leg_d);
        }
    }
    // Final partial split: whatever remains after the last full unit.
    let remaining = total - unit_m * (index - 1) as f64;
    if remaining > 1e-6 {
        let dur = moving_s;
        splits.push(RunSplit {
            index,
            // Normalize the short remainder to a full unit so it is comparable.
            pace_sec_per_unit: dur / remaining * unit_m,
            cumulative_m: total,
            distance_m: remaining,
            partial: true,
        });
    }
    splits
}

/// A GPS ground speed above this is jitter, not a real running segment, and is
/// clamped before the convex weighting so a single noisy fix cannot inflate the
/// normalized speed. Re-exports the ONE plausible-runner-speed threshold from
/// `load` (File-07 QC value, 12.0 m/s) so the ingest gate, this clamp, and the
/// shell live-jitter guard all agree. (On the real path `qc_track` already drops
/// legs above it, so the clamp is a belt-and-braces guard for raw callers.)
pub use crate::load::MAX_PLAUSIBLE_SPEED_MPS;

/// Variability-index cutoff at or above which a run reads as interval-like rather
/// than steady (the verdict is `vi >= INTERVAL_VI_THRESHOLD`). With the rolling
/// [`INTERVAL_WINDOW_SEC`] smoothing, a steady run, even one with ordinary pace
/// drift (hills / a negative split, ±~15% swing), sits at ~1.00–1.01, while
/// genuine reps-plus-recovery sessions sit ~1.11–1.21 (degrading toward the low
/// end only under heavy GPS noise). 1.10 sits in that gap with margin on BOTH
/// sides: noisy real intervals (~1.11) still clear it and steady runs stay
/// comfortably below. An ENGINE HEURISTIC, not a cited threshold (the
/// KB source, Skiba GOVSS, gives no boundary), hence the RUN-INTERVAL-VI-001 chip
/// is graded Weak and its copy measures rather than prescribes.
pub const INTERVAL_VI_THRESHOLD: f64 = 1.10;

/// Graded verdict for whether a run's variability index marks it interval-like
/// (`true`) or steady (`false`). Carries the RUN-INTERVAL-VI-001 evidence so a
/// shell renders it with the same grade chip / why? chrome as any other
/// recommendation, the differentiation is descriptive but still evidence-cited.
pub fn interval_verdict(variability_index: f64) -> Recommended<bool> {
    crate::evidence::graded(
        variability_index >= INTERVAL_VI_THRESHOLD,
        claims::RUN_INTERVAL_VI_001,
    )
}

/// Rolling-average window (seconds) applied to the speed series BEFORE the
/// 4th-power weighting. This is the crux of the GOVSS/Normalized-Graded-Pace
/// algorithm and the KB's stated pre-implementation requirement (Skiba 120 s,
/// GoldenCheetah 30 s): at a 1 Hz GPS cadence the raw per-second leg speed is
/// dominated by position noise (±a few metres over a ~1–4 m move), and the convex
/// 4th power AMPLIFIES that noise, so without smoothing a steady run scores high
/// (false "interval") while genuine rep structure is drowned out. A 30 s window
/// averages the jitter away (≈30 samples/bin) yet preserves real reps: 800 m reps
/// (~2.5–3.5 min) and 400 m reps (~1.5–2 min) are many windows long. 30 s (the
/// GoldenCheetah value) is chosen over Skiba's 120 s so shorter reps still survive.
pub const INTERVAL_WINDOW_SEC: f64 = 30.0;

/// The MOVING legs of a track as `(dt_seconds, distance_m)`, in order. A leg is
/// dropped when its timestamps don't advance (`dt <= 0`) or its ground speed is
/// below the auto-pause floor (`load::is_stopped`, <0.5 m/s), so a café/traffic
/// stop (the Pause button leaves a ~0-speed bridge leg spanning the gap) is
/// excluded exactly as `moving_duration_min` excludes it from the displayed pace.
/// Distance is capped so the implied speed never exceeds [`MAX_PLAUSIBLE_SPEED_MPS`]
/// (GPS jitter rejection). Same accuracy gate as [`track_distance_km`].
fn moving_legs(
    points: &[GpsPoint],
    max_accuracy_m: f32,
    segment_starts: &[u32],
) -> Vec<(f64, f64)> {
    // Moving legs are computed WITHIN each segment, so the pause-bridge leg
    // between segments is never formed (a paused relocation is neither moving
    // time nor moving distance). Empty `segment_starts` → one segment → the
    // legs of the whole track.
    segments(points, segment_starts)
        .into_iter()
        .flat_map(|seg| {
            usable_track(seg, max_accuracy_m)
                .windows(2)
                .filter_map(|w| {
                    // Cap distance so speed ≤ the plausible-runner ceiling (jitter).
                    leg(w[0], w[1]).map(|(dt, d)| (dt, d.min(MAX_PLAUSIBLE_SPEED_MPS * dt)))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Resample moving legs into fixed [`INTERVAL_WINDOW_SEC`] bins along the moving
/// timeline (the pause gaps are already removed), returning each bin's
/// `(mean_speed, duration)`. A leg is split across bin boundaries at its own
/// constant speed, so the bins are a proper rolling average of speed over the
/// window, the smoothing that makes the 4th-power step respond to real pace
/// structure instead of per-second GPS noise. The final short bin keeps its true
/// (sub-window) duration so it is weighted correctly.
fn windowed_bin_speeds(legs: &[(f64, f64)], window_s: f64) -> Vec<(f64, f64)> {
    let mut bins = Vec::new();
    // A non-positive window would make `take` zero and spin forever; refuse it.
    if window_s <= 0.0 {
        return bins;
    }
    let mut bin_time = 0.0;
    let mut bin_dist = 0.0;
    for &(dt, dist) in legs {
        let speed = if dt > 0.0 { dist / dt } else { 0.0 };
        let mut rem = dt;
        while rem > 0.0 {
            let take = rem.min(window_s - bin_time);
            bin_time += take;
            bin_dist += speed * take;
            rem -= take;
            if bin_time >= window_s - 1e-9 {
                bins.push((bin_dist / bin_time, bin_time));
                bin_time = 0.0;
                bin_dist = 0.0;
                // A corrupt `dt` (e.g. a mis-unit'd timestamp yielding a
                // multi-week "leg") would otherwise drive this resampling loop
                // for millions of iterations inside `view()`. A real run never
                // approaches this many windows; past it the series is corrupt
                // and the truncated (still deterministic) VI is good enough.
                if bins.len() >= MAX_WINDOW_BINS {
                    return bins;
                }
            }
        }
    }
    if bin_time > 0.0 {
        bins.push((bin_dist / bin_time, bin_time));
    }
    bins
}

/// Hard cap on smoothing bins produced by [`windowed_bin_speeds`], bounding the
/// per-`view()` work against a corrupt GPS timeline (see the early return there).
/// ~50 k × 30 s ≈ 17 days of moving time, unreachable for any genuine run.
const MAX_WINDOW_BINS: usize = 50_000;

/// Normalized speed (m/s): the duration-weighted 4th-power-mean of the
/// WINDOW-AVERAGED speed series, GOVSS / Normalized-Graded-Pace, flat-ground
/// (no elevation → normalized *speed*, not grade-adjusted power). The convex
/// 4th power is what lets a 6×800 m session score above its own average while a
/// steady run does not (Jensen's inequality). Smoothing over
/// [`INTERVAL_WINDOW_SEC`] first is essential, see that constant. Returns `None`
/// for fewer than two usable fixes or no moving time.
pub fn normalized_speed_mps(points: &[GpsPoint], max_accuracy_m: f32) -> Option<f64> {
    normalized_speed_mps_seg(points, max_accuracy_m, &[])
}

/// [`normalized_speed_mps`] over the segment-aware moving legs (pause bridges
/// excluded). Empty `segment_starts` is bit-identical to [`normalized_speed_mps`].
pub fn normalized_speed_mps_seg(
    points: &[GpsPoint],
    max_accuracy_m: f32,
    segment_starts: &[u32],
) -> Option<f64> {
    if usable_track(points, max_accuracy_m).len() < 2 {
        return None;
    }
    let bins = windowed_bin_speeds(
        &moving_legs(points, max_accuracy_m, segment_starts),
        INTERVAL_WINDOW_SEC,
    );
    let (mut p4_sum, mut weight_sum) = (0.0, 0.0);
    for &(speed, dur) in &bins {
        p4_sum += dur * speed.powi(4);
        weight_sum += dur;
    }
    if weight_sum <= 0.0 {
        return None;
    }
    Some((p4_sum / weight_sum).powf(0.25))
}

/// Variability Index = normalized speed ÷ average MOVING speed for a track.
/// About 1.0 for an evenly-paced steady run (window smoothing removes GPS noise);
/// rises above 1.0 as a run becomes more interval-like (hard reps separated by
/// JOG recovery), because the convex normalized speed weights the fast reps far
/// above the moving-time average. Both numerator and denominator use the SAME
/// moving-leg base (paused legs excluded), so a stop can't inflate it. This is
/// the scalar that lets the engine tell a 6×800 m session from a steady run of
/// the *same average pace*.
///
/// Purely descriptive (a MEASUREMENT of the run, not a recommendation, and not a
/// load input). Returns `None` when the track has no moving legs or a
/// non-positive average speed.
pub fn track_variability_index(points: &[GpsPoint], max_accuracy_m: f32) -> Option<f64> {
    track_variability_index_seg(points, max_accuracy_m, &[])
}

/// [`track_variability_index`] over the segment-aware moving legs (both the
/// normalized speed and the average speed use the same pause-bridge-excluded
/// base). Empty `segment_starts` is bit-identical to [`track_variability_index`].
pub fn track_variability_index_seg(
    points: &[GpsPoint],
    max_accuracy_m: f32,
    segment_starts: &[u32],
) -> Option<f64> {
    let ns = normalized_speed_mps_seg(points, max_accuracy_m, segment_starts)?;
    let legs = moving_legs(points, max_accuracy_m, segment_starts);
    let dur: f64 = legs.iter().map(|&(dt, _)| dt).sum();
    let dist: f64 = legs.iter().map(|&(_, d)| d).sum();
    if dur <= 0.0 {
        return None;
    }
    let avg = dist / dur; // moving distance ÷ moving time
    if avg <= 0.0 {
        return None;
    }
    Some(((ns / avg) * 100.0).round() / 100.0)
}

/// Nominal GNSS range error (m) a GPX `<hdop>` multiplies into a horizontal
/// accuracy: the export writes `accuracy_m / 5` and the shell's importer reads
/// `hdop × 5` (Gpx.kt `GPS_UERE_NOMINAL_M`), so a fix's accuracy round-trips.
const GPX_HDOP_UERE_M: f32 = 5.0;

/// Serialise a fix track to a GPX 1.1 document (the format Strava, Garmin
/// Connect, Komoot, etc. import). Pure and deterministic: `observed_at` unix
/// seconds are rendered as RFC 3339 UTC timestamps in-core, never from a live
/// clock. Elevation is omitted: the shell does not yet capture altitude.
pub fn export_gpx(points: &[GpsPoint], track_name: &str) -> String {
    export_gpx_seg(points, track_name, &[])
}

/// [`export_gpx`] that emits one `<trkseg>` per recording segment, using the TRUE
/// coordinates (no re-anchoring shift), so a paused + relocated run opens in
/// Strava/Garmin as the real route with a visible gap at each pause instead of a
/// single continuous line drawn through wrong coordinates. Empty
/// `segment_starts` is a single `<trkseg>`, identical to [`export_gpx`].
pub fn export_gpx_seg(points: &[GpsPoint], track_name: &str, segment_starts: &[u32]) -> String {
    let mut s = String::with_capacity(256 + points.len() * 96);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(
        "<gpx version=\"1.1\" creator=\"lollop\" \
         xmlns=\"http://www.topografix.com/GPX/1/1\">\n",
    );
    s.push_str("  <trk>\n    <name>");
    xml_escape_into(&mut s, track_name);
    s.push_str("</name>\n");
    for seg in segments(points, segment_starts) {
        if seg.is_empty() {
            continue;
        }
        s.push_str("    <trkseg>\n");
        for p in seg {
            s.push_str("      <trkpt lat=\"");
            let _ = write!(s, "{:.7}", p.lat);
            s.push_str("\" lon=\"");
            let _ = write!(s, "{:.7}", p.lon);
            s.push_str("\"><time>");
            s.push_str(&unix_to_rfc3339_utc(p.observed_at));
            s.push_str("</time>");
            if p.accuracy_m.is_finite() && p.accuracy_m > 0.0 {
                let _ = write!(s, "<hdop>{}</hdop>", p.accuracy_m / GPX_HDOP_UERE_M);
            }
            s.push_str("</trkpt>\n");
        }
        s.push_str("    </trkseg>\n");
    }
    s.push_str("  </trk>\n</gpx>\n");
    s
}

/// Append `text` to `out`, escaping the five XML predefined entities so a track
/// name can never break the document.
fn xml_escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
}

/// Format unix seconds as `YYYY-MM-DDTHH:MM:SSZ` (UTC), pure integer math, no
/// `chrono`, no clock. Uses Howard Hinnant's civil-from-days algorithm so it is
/// correct across the whole proleptic Gregorian range.
fn unix_to_rfc3339_utc(secs: i64) -> String {
    let days = secs.div_euclid(crate::DAY_SEC);
    let tod = secs.rem_euclid(crate::DAY_SEC);
    let (h, m, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Convert a count of days since the unix epoch (1970-01-01) to a civil
/// `(year, month, day)`. Howard Hinnant, "chrono-Compatible Low-Level Date
/// Algorithms" (public domain).
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests;
