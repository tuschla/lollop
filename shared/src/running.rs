//! File 04, Running engine core: run-training math.
//!
//! Every function is either a pure calculation
//! (HRmax, zone classification, band table lookup) returning a plain value, or a
//! prescriptive recommendation (spike gate, volume caps, taper) returning a
//! [`Recommended<T>`] whose [`ClaimId`](crate::evidence::ClaimId) names the
//! backing evidence row in the claim registry.
//!
//! Rule ids and table values are transcribed verbatim from
//! `planning/knowledge-base/extracted/04-running.md`.
//!
//! ACWR is not implemented: `LOAD-ACWR-001` is a blocked `MarketingMyth`;
//! the single-session spike gate (`RUN-SPIKE-001`) is the gate instead.

use crate::evidence::claims;
use crate::schema::{
    MesoPhase, Recommended, RunSegment, RunSessionType, RunVolume, ThreeZone, VdotBand, WorkoutType,
};

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
/// realistic adult/junior age, [`TANAKA_AGE_MIN_YEARS`]–[`TANAKA_AGE_MAX_YEARS`].
/// Outside it no HRmax is fabricated.
pub fn tanaka_age_in_range(age_years: f64) -> bool {
    (TANAKA_AGE_MIN_YEARS..=TANAKA_AGE_MAX_YEARS).contains(&age_years)
}

/// Youngest age the Tanaka HRmax estimate is emitted for, years.
pub const TANAKA_AGE_MIN_YEARS: f64 = 5.0;
/// Oldest age the Tanaka HRmax estimate is emitted for, years.
pub const TANAKA_AGE_MAX_YEARS: f64 = 100.0;

/// A measured HRmax worth trusting over the Tanaka estimate: finite and
/// physiologically plausible ([`MEASURED_HR_MAX_MIN_BPM`]–
/// [`MEASURED_HR_MAX_MAX_BPM`]). The one gate the HR-zone calculator and run
/// targets share.
pub fn plausible_measured_hr_max(measured_bpm: Option<f64>) -> Option<f64> {
    measured_bpm.filter(|m| {
        m.is_finite() && (MEASURED_HR_MAX_MIN_BPM..=MEASURED_HR_MAX_MAX_BPM).contains(m)
    })
}

/// Lowest measured HRmax the core trusts, bpm.
pub const MEASURED_HR_MAX_MIN_BPM: f64 = 100.0;
/// Highest measured HRmax the core trusts, bpm.
pub const MEASURED_HR_MAX_MAX_BPM: f64 = 240.0;

/// A resting HR the Karvonen %HRR ceiling (RUN-KARVONEN-001) may anchor on:
/// finite and [`RESTING_HR_MIN_BPM`]–[`RESTING_HR_MAX_BPM`].
pub fn plausible_resting_hr(bpm: f64) -> bool {
    bpm.is_finite() && (RESTING_HR_MIN_BPM..=RESTING_HR_MAX_BPM).contains(&bpm)
}

/// Lowest plausible resting HR, bpm.
pub const RESTING_HR_MIN_BPM: f64 = 25.0;
/// Highest plausible resting HR, bpm.
pub const RESTING_HR_MAX_BPM: f64 = 120.0;

/// The highest average heart rate a logged run states, % of HRmax.
pub const HR_PCT_MAX_CEILING: f64 = 100.0;

/// Upper bound of zone 1 (below LT1), %HRmax (File 04 running-003 table).
const ZONE1_CEILING_PCT_HRMAX: f64 = 82.0;
/// Floor of zone 3 and of threshold (T) work, %HRmax (running-003 / running-018).
pub const THRESHOLD_FLOOR_PCT_HRMAX: f64 = 88.0;
/// Floor of interval/VO2max (I) work, %HRmax (running-019).
const INTERVAL_FLOOR_PCT_HRMAX: f64 = 95.0;

/// Classify an intensity by %HRmax into the LT1/LT2 three-zone model. Rule running-003.
///
/// Boundaries per File 04 table: Z1 < ~82 %HRmax, Z2 ~82–<88 %, Z3 ≥ ~88 %.
/// The 88 % boundary is inclusive to the hard zone so a run held exactly at the
/// prescribed Tempo/Threshold floor (0.88, `workout_rx`) counts as threshold work
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

pub const MARATHON_M: f64 = 42_195.0;

pub const FIVE_K_M: f64 = 5_000.0;

pub const TEN_K_M: f64 = 10_000.0;

/// Pace band (fast, slow) in seconds per km for a workout row that states its
/// pace relative to marathon pace (running-015 Easy 15–25 %, running-016 Long
/// run 10–20 % slower than MP). MP is the athlete's Daniels marathon prediction
/// ÷ the marathon distance in km. `None` for a non-positive VDOT or a row
/// without both bounds.
pub fn mp_relative_pace_sec_per_km(vdot: f64, kind: RunSessionType) -> Option<(f64, f64)> {
    if !vdot.is_finite() || vdot <= 0.0 {
        return None;
    }
    let (Some(lo), Some(hi)) = workout_rx(kind).value.pct_slower_than_mp else {
        return None;
    };
    let mp = crate::load::daniels_predict(vdot, MARATHON_M) / (MARATHON_M / KM_M);
    (mp.is_finite() && mp > 0.0).then(|| (mp * (1.0 + lo / 100.0), mp * (1.0 + hi / 100.0)))
}

/// Pace band (fast, slow) in seconds per km for a Daniels VDOT band, from the
/// band's %VO2max limits ([`vdot_band_vo2max_pct`], running-007 table).
/// Easy is the exception: its pace comes from marathon pace, 15–25 % slower
/// (running-015, the claim every easy prescription cites), because the 59–74
/// %VO2max row inverted through the oxygen-cost equation lands only ~7–9 %
/// slower than MP, well faster than Daniels' published E paces.
/// Repetition is ">100 %VO2max" with no stated upper limit, so it returns the
/// 100 % pace for both ends, read as "at least this fast". `[Moderate]`
pub fn vdot_band_pace_sec_per_km(vdot: f64, band: VdotBand) -> Option<(f64, f64)> {
    if matches!(band, VdotBand::Easy) {
        return mp_relative_pace_sec_per_km(vdot, RunSessionType::Easy);
    }
    let (lo_pct, hi_pct) = vdot_band_vo2max_pct(band);
    let slow = crate::load::pace_sec_per_km_at(vdot, lo_pct)?;
    let fast = match hi_pct {
        Some(hi) => crate::load::pace_sec_per_km_at(vdot, hi)?,
        None => slow,
    };
    Some((fast, slow))
}

/// Which weekly-share caps a week exceeds (File 04 "Volume caps"; running-016/018/019).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapViolations {
    /// The long run is over [`LONG_RUN_SHARE_MIN`] of the week.
    pub long_run: bool,
    /// Threshold (T) work is over [`THRESHOLD_WEEKLY_CAP_FRAC`] of the week.
    pub threshold: bool,
    /// Interval (I) work is over [`INTERVAL_WEEKLY_CAP_FRAC`] of the week.
    pub interval: bool,
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

fn within_share(km: f64, weekly_km: f64, cap: f64) -> bool {
    weekly_km > 0.0 && km / weekly_km <= cap
}

/// Check the long-run (≤25 %, running-016), threshold (≤10 %, running-018) and
/// interval (≤8 %, running-019) weekly-share caps; every exceeded cap is set.
/// Prescriptive → RUN-VOLCAP-001 (the Daniels weekly-share caps themselves,
/// ExpertOpinion floor), not the polarized-vs-threshold distribution claim.
pub fn check_volume_caps(
    long_run_km: f64,
    threshold_km: f64,
    interval_km: f64,
    weekly_km: f64,
) -> Recommended<CapViolations> {
    let violations = CapViolations {
        long_run: !within_share(long_run_km, weekly_km, LONG_RUN_SHARE_MIN),
        threshold: !within_share(threshold_km, weekly_km, THRESHOLD_WEEKLY_CAP_FRAC),
        interval: !within_share(interval_km, weekly_km, INTERVAL_WEEKLY_CAP_FRAC),
    };
    Recommended::new(violations, claims::RUN_VOLCAP_001)
}

/// RUN-SPIKE-001 / running-029: a single session more than this fraction over
/// the longest run of the trailing [`SPIKE_WINDOW_DAYS`] is a spike.
pub const SPIKE_FRAC: f64 = 0.10;

/// RUN-SPIKE-001: the trailing window, days, whose longest run is the spike
/// baseline ("longest run in the last 30 days"). The baseline is NOT all-time,
/// so an old long run before a layoff never suppresses a spike after it.
pub const SPIKE_WINDOW_DAYS: i64 = 30;

/// Raw predicate: does this session exceed the 30-day longest run by more than
/// [`SPIKE_FRAC`]? Rule RUN-SPIKE-001 / running-029.
///
/// Strongest running injury signal (Frandsen 2025). A non-positive 30-day
/// longest (no history) means any real session is unbounded relative to it →
/// treated as a spike so the gate errs safe.
pub(crate) fn single_session_spike(session_km: f64, longest_30d_km: f64) -> bool {
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
    Recommended::new(
        single_session_spike(session_km, longest_30d_km),
        claims::RUN_SPIKE_BLOCK_001,
    )
}

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
    Recommended::new(d, claims::RUN_DIST_001)
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
    Recommended::new(
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
    Recommended::new(ok, claims::RUN_QUALITY_001)
}

/// A quality (hard) run kind: threshold, VO2max intervals, repetitions,
/// race-pace work or hill repeats. The long run is easy-paced (E→E+) and is
/// not one.
pub fn is_quality_session(kind: RunSessionType) -> bool {
    matches!(
        kind,
        RunSessionType::Tempo
            | RunSessionType::Interval
            | RunSessionType::Repetition
            | RunSessionType::RacePace
            | RunSessionType::Hills
    )
}

/// A run kind the spacing rules keep apart from other hard days and from heavy
/// leg work: a quality session or the long run (RUN-QUALITY-001, HYB-CAP-001
/// CAP-3).
pub fn is_hard_or_long_session(kind: RunSessionType) -> bool {
    kind == RunSessionType::LongRun || is_quality_session(kind)
}

/// Safe single-week volume-increase cap as a fraction, by training age
/// (File 04 running-031). Novice (<1 yr) tolerates up to +10 %; experienced
/// runners are held to ~+5 %. NOT the discredited hard "10 % rule", a ceiling.
pub fn weekly_increase_cap_frac(training_age_years: f64) -> Recommended<f64> {
    let cap = if training_age_years < 1.0 { 0.10 } else { 0.05 };
    Recommended::new(cap, claims::RUN_PROGRESS_001)
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
    Recommended::new(ok, claims::RUN_PROGRESS_001)
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
    Recommended::new(flag, claims::RUN_PROGRESS_001)
}

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
    Recommended::new(
        DeloadCadence {
            load_weeks,
            recovery_weeks: 1,
            reduction_frac: (0.20, 0.40),
        },
        claims::RUN_DELOAD_001,
    )
}

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

/// Look up the prescription band for a run session type. Rules running-014…022 +
/// the VDOT R row. Per-rule claims: Recovery running-014 → RUN-RECOVERY-001,
/// Easy running-015 → RUN-EASY-001, LongRun running-016 → RUN-LONGRUN-001,
/// RacePace running-017 → RUN-RACEPACE-001, Tempo running-018 → RUN-TEMPO-001,
/// Interval running-019 → RUN-INTERVAL-001 (contested CQ-10), Repetition VDOT
/// R row → RUN-REPETITION-001, Strides running-020 → RUN-STRIDES-001; Hills
/// blends running-021/022 and stays on the workout-table synthesis claim
/// (RUN-WORKOUT-001), the hill-sprint-specific numbers live in
/// [`hill_sprint_rx`].
pub fn workout_rx(kind: RunSessionType) -> Recommended<RunWorkoutRx> {
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
    let (rx, claim_id) = match kind {
        // running-014 Recovery: ≥20 % slower than MP (no upper slowness
        // bound), ceilings <76 %HRmax AND <70 %HRR (the KB states no lower HR
        // bound), RPE 2–3, 20–40 min.
        Recovery => (
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
        Easy => (
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
        // checks live in `check_volume_caps` / `long_run_within_daily_avg`.
        LongRun => (
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
        Tempo => (
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
        RacePace => (
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
        Interval => (
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
        Repetition => (
            RunWorkoutRx {
                duration_min: (None, Some(2)),
                ..open
            },
            claims::RUN_REPETITION_001,
        ),
        // running-020 Strides (neuromuscular; 15–30 s reps are sub-minute, so
        // only a coarse ≤1-min row here, exact seconds in `strides_rx`).
        Strides => (
            RunWorkoutRx {
                rpe: Some((6, 7)),
                duration_min: (None, Some(1)),
                ..open
            },
            claims::RUN_STRIDES_001,
        ),
        // running-021 / running-022 Hill sprints & long hill reps (blended
        // row; hill-sprint specifics in `hill_sprint_rx`).
        Hills => (
            RunWorkoutRx {
                pct_hr_max: (Some(0.90), Some(1.00)),
                rpe: Some((7, 9)),
                duration_min: (None, Some(4)),
                ..open
            },
            claims::RUN_WORKOUT_001,
        ),
    };
    Recommended::new(rx, claim_id)
}

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
    Recommended::new(
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
    Recommended::new(
        Vo2maxIntervalRx {
            rep_duration_min: (3, 5),
            rep_distance_m: (800, 1600),
            recovery_max_ratio_of_rep: 1.0,
            weekly_cap_frac: INTERVAL_WEEKLY_CAP_FRAC,
        },
        claims::RUN_INTERVAL_001,
    )
}

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
    Recommended::new(
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
    Recommended::new(
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

/// The low-mileage guardrail's multiple (running-016): no run longer than this
/// many times the average run-day distance.
pub const DAILY_AVG_RUN_MULTIPLE: f64 = 2.0;

/// Average distance per RUN day — the KB reading of "daily average" for the
/// long-run guardrail (running-016): weekly volume over the days actually run,
/// not calendar days. The single source shared by the plan's long-run
/// decision and the weekly report so the two never disagree.
pub fn daily_avg_run_km(weekly_km: f64, n_run_days: usize) -> f64 {
    weekly_km / n_run_days.max(1) as f64
}

/// The longest run the guardrail allows in a week of `weekly_km` over
/// `n_run_days` run days: [`DAILY_AVG_RUN_MULTIPLE`] × [`daily_avg_run_km`].
pub fn daily_avg_run_cap_km(weekly_km: f64, n_run_days: usize) -> f64 {
    DAILY_AVG_RUN_MULTIPLE * daily_avg_run_km(weekly_km, n_run_days)
}

/// Low-mileage guardrail (running-016): the long run must not exceed
/// [`DAILY_AVG_RUN_MULTIPLE`] × the average daily run. Errs safe on a
/// non-positive average (no history → any real long run fails the
/// guardrail).
pub fn long_run_within_daily_avg(long_run_km: f64, avg_daily_run_km: f64) -> Recommended<bool> {
    let ok = avg_daily_run_km > 0.0 && long_run_km <= DAILY_AVG_RUN_MULTIPLE * avg_daily_run_km;
    Recommended::new(ok, claims::RUN_LONGRUN_001)
}

/// The longest equal distance for `n_easy` easy runs in a week of
/// `n_run_days` runs whose other runs total `fixed_km` such that no easy run
/// breaks the guardrail against the week as executed: `e ≤ m·(fixed +
/// n_easy·e) / n`, i.e. `e ≤ m·fixed / (n − m·n_easy)`. `None` when it never
/// binds: no easy day, or easy days at least `n / m` (their own distance then
/// carries the average).
pub fn easy_run_cap_for_daily_avg(fixed_km: f64, n_easy: usize, n_run_days: usize) -> Option<f64> {
    let n = n_run_days as f64;
    let easy = n_easy as f64;
    (n_easy > 0 && n > DAILY_AVG_RUN_MULTIPLE * easy)
        .then(|| DAILY_AVG_RUN_MULTIPLE * fixed_km / (n - DAILY_AVG_RUN_MULTIPLE * easy))
}

/// Default weekly long-run share band (running-016 + volume-caps section):
/// 25–30 % of weekly volume, as fractions. `goal_week_plan` carries the same
/// band for its distance-race rows.
pub fn long_run_share_default() -> Recommended<(f64, f64)> {
    Recommended::new(
        (LONG_RUN_SHARE_MIN, LONG_RUN_SHARE_MAX),
        claims::RUN_LONGRUN_001,
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
    Recommended::new(
        RecoveryWeekRx {
            volume_reduction_frac,
            reduce_intensity: true,
            drop_quality_session: true,
        },
        claims::RUN_DELOAD_DEPTH_001,
    )
}

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
    Some(Recommended::new(rx, claims::RUN_TAPER_001))
}

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
    Some(Recommended::new(phase, claim))
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
        (FiveK, Build | Taper) | (TenK, Peak | Taper) => &[Interval, Tempo],
        (FiveK, Peak) => &[Interval, Repetition],
        (TenK | HalfMarathon, Build) => &[Tempo, Interval],
        (HalfMarathon, Peak | Taper) | (Marathon, Build) => &[Tempo, RacePace],
        (Marathon, Peak | Taper) => &[RacePace, Tempo],
    };
    Recommended::new(mix, claims::RUN_PHASE_TID_001)
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
    Some(Recommended::new(t, claims::RUN_LONGRUN_TARGET_001))
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
    Recommended::new(share, claims::RUN_LONGRUN_TARGET_001)
}

/// Daniels' longest-steady-run time cap (running-016/046), minutes.
pub const LONG_RUN_MAX_MIN: f64 = 150.0;

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
    Recommended::new(
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
    Recommended::new(
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
    Recommended::new(
        RacePaceRx {
            weekly_share_min: 0.15,
            weekly_share_max: 0.20,
            max_min: 110,
            max_km: 29.0,
        },
        claims::RUN_RACEPACE_002,
    )
}

/// Weeks after which training paces / HR zones are due for re-testing: the low
/// end of the KB's 4–6-week window (running-041 RUN-RETEST-001, running-006
/// RUN-HRRECALC-001).
pub const RETEST_DUE_WEEKS: u8 = 4;

/// Whether training paces are due for a re-test (running-041 →
/// RUN-RETEST-001): set paces from CURRENT fitness and re-test every 4–6
/// weeks. Due once [`RETEST_DUE_WEEKS`] have elapsed (same convention as
/// [`hr_zone_recalc_due`]).
pub fn pace_retest_due(weeks_since_test: u8) -> Recommended<bool> {
    Recommended::new(weeks_since_test >= RETEST_DUE_WEEKS, claims::RUN_RETEST_001)
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
    Recommended::new(f, claims::RUN_RETEST_001)
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
/// Flags only: the evidence pins trigger thresholds, not correction
/// magnitudes. 15 °C is a conservative trigger (first significant slowing at
/// WBGT >~15 °C, Ely 2007, PMID 17473775; optimum ~4–10 °C, El Helou 2012,
/// PMID 22649525). Dry-bulb air temp only, so humid heat under-fires.
pub fn pace_correction_triggers(
    temp_c: f64,
    altitude_m: f64,
) -> Recommended<PaceCorrectionTriggers> {
    Recommended::new(
        PaceCorrectionTriggers {
            heat: temp_c > PACE_CORRECTION_HEAT_C,
            altitude: altitude_m > PACE_CORRECTION_ALTITUDE_M,
        },
        claims::RUN_RETEST_001,
    )
}

/// Longest training run (km) under which a marathon goal-time prediction is
/// optimistic (running-040; RUN-VDOT-001).
pub const MARATHON_REALISM_LONG_RUN_KM: f64 = 30.0;

/// Flag a marathon goal-time prediction as optimistic when the longest training
/// run is under [`MARATHON_REALISM_LONG_RUN_KM`]. Rule running-040: first-timers
/// under this threshold often finish 10–15% slower than a Riegel-from-5K
/// projection, so the estimate should be derated (~2–3 VDOT points). Returns
/// `true` when the gate trips.
pub fn marathon_prediction_optimistic(longest_run_km: f64) -> Recommended<bool> {
    Recommended::new(
        longest_run_km < MARATHON_REALISM_LONG_RUN_KM,
        claims::RUN_VDOT_001,
    )
}

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
        return Recommended::new(Equivalency::Range(lo, hi), claims::RUN_EQUIV_001);
    }
    let rel_diff = (hi - lo) / lo;
    let out = if rel_diff <= 0.02 {
        Equivalency::Agreed((riegel_sec + daniels_sec) / 2.0)
    } else {
        Equivalency::Range(lo, hi)
    };
    // running-039: the Riegel/Daniels equivalency combiner (RUN-EQUIV-001), not
    // the VDOT-fitness estimate (RUN-VDOT-001).
    Recommended::new(out, claims::RUN_EQUIV_001)
}

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
    Recommended::new(p, claims::RUN_KARVONEN_001)
}

/// Whether HR training zones are due for recalculation (running-006): recompute
/// every 4–6 weeks off a measured max HR. Due once [`RETEST_DUE_WEEKS`] have
/// elapsed. RUN-HRRECALC-001 (Strong, safety-critical, stale zones misplace
/// every prescription).
pub fn hr_zone_recalc_due(weeks_since_recalc: u8) -> Recommended<bool> {
    Recommended::new(
        weeks_since_recalc >= RETEST_DUE_WEEKS,
        claims::RUN_HRRECALC_001,
    )
}

/// A training goal by target race distance (running-024 goal table).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    crate::sanitize::Sanitize,
    crate::wire::Wire,
)]
pub enum GoalDistance {
    General,
    C25k,
    FiveK,
    TenK,
    HalfMarathon,
    Marathon,
}

/// How close a distance (m) must be to a goal's race distance to count as that
/// race for a distance-specific coaching rule (the running-040/008 marathon
/// derate): the exact distance or its one-decimal km entry (42.2 km is 5 m
/// off), never a neighbouring race.
const RULE_MATCH_TOLERANCE_M: f64 = 10.0;

/// How close a distance (m) must be to a standard race distance to be NAMED
/// after it in copy: wide enough for the 42.2 / 21.1 km roundings and a
/// whole-metre 21 097 m best effort. Naming only, never a rule input.
pub const NAME_MATCH_TOLERANCE_M: f64 = 50.0;

impl GoalDistance {
    /// The goal's race distance, metres; `None` for the goals without a race
    /// (general fitness, Couch to 5K).
    pub const fn metres(self) -> Option<f64> {
        match self {
            GoalDistance::General | GoalDistance::C25k => None,
            GoalDistance::FiveK => Some(FIVE_K_M),
            GoalDistance::TenK => Some(TEN_K_M),
            GoalDistance::HalfMarathon => Some(MARATHON_M / 2.0),
            GoalDistance::Marathon => Some(MARATHON_M),
        }
    }

    /// Whether `distance_m` is this goal's race distance for a
    /// distance-specific rule (within [`RULE_MATCH_TOLERANCE_M`]). False for a
    /// goal without a race.
    pub fn is_race_distance(self, distance_m: f64) -> bool {
        self.metres()
            .is_some_and(|m| (distance_m - m).abs() < RULE_MATCH_TOLERANCE_M)
    }

    /// The race goal a free distance in km is named after (within
    /// [`NAME_MATCH_TOLERANCE_M`]), if any.
    pub fn named_at_km(km: f64) -> Option<Self> {
        let tolerance_km = NAME_MATCH_TOLERANCE_M / 1000.0;
        [
            GoalDistance::Marathon,
            GoalDistance::HalfMarathon,
            GoalDistance::TenK,
            GoalDistance::FiveK,
        ]
        .into_iter()
        .find(|g| {
            g.metres()
                .is_some_and(|m| (km - m / 1000.0).abs() < tolerance_km)
        })
    }

    /// The goal's title, as the shell's profile editor names it.
    pub fn label(self) -> &'static str {
        match self {
            GoalDistance::General => "General fitness",
            GoalDistance::C25k => "Couch to 5K",
            GoalDistance::FiveK => "5K",
            GoalDistance::TenK => "10K",
            GoalDistance::HalfMarathon => "Half marathon",
            GoalDistance::Marathon => "Marathon",
        }
    }

    /// The race's name mid-sentence ("half marathon", "10K"); `None` for a
    /// goal without a race.
    pub fn race_name(self) -> Option<&'static str> {
        match self {
            GoalDistance::General | GoalDistance::C25k => None,
            GoalDistance::FiveK => Some("5K"),
            GoalDistance::TenK => Some("10K"),
            GoalDistance::HalfMarathon => Some("half marathon"),
            GoalDistance::Marathon => Some("marathon"),
        }
    }
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
    Recommended::new(d, claims::RUN_VDOT_001)
}

/// Derate an optimistic marathon finish-time band for an under-mileaged runner
/// (running-008/040, "flag AND derate"): the fast bound is slowed by the
/// minimum and the slow bound by the maximum [`vdot_derate_points`], converted
/// to time through [`daniels_predict`](crate::load::daniels_predict) at
/// `base_vdot`. RUN-VDOT-001. A degenerate band, a VDOT the derate would take
/// to zero or below, or a failed prediction returns the band unchanged.
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
    Recommended::new(plan, claims::RUN_WORKOUT_001)
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
    Recommended::new(
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
    Recommended::new(
        C25kSession {
            warmup_walk_min: 5,
            bouts,
        },
        claims::RUN_C25K_002,
    )
}

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
    Recommended::new(180.0 - age_years + delta, claims::RUN_MAF_001)
}

/// Enforce the architectural invariant that ~80% of running time is easy
/// (running-011): true when the easy (Z1) time share is at least 80%. RUN-DIST-001.
pub fn easy_share_floor_ok(easy_frac_by_time: f64) -> Recommended<bool> {
    Recommended::new(easy_frac_by_time >= 0.80, claims::RUN_DIST_001)
}

/// Weeks to hold before the next volume bump for novices (running-031):
/// `Some((2, 3))` under one year of training age; `None` for experienced runners
/// (the source specifies no fixed hold beyond a ~5%/absolute cap). RUN-PROGRESS-001.
pub fn novice_volume_bump_hold_weeks(training_age_years: f64) -> Recommended<Option<(u8, u8)>> {
    let hold = if training_age_years < 1.0 {
        Some((2, 3))
    } else {
        None
    };
    Recommended::new(hold, claims::RUN_PROGRESS_001)
}

/// One kilometre in metres, the per-km split unit.
pub const KM_M: f64 = 1000.0;

/// One international mile in metres (exact), the per-mile split unit.
pub const MILE_M: f64 = 1609.344;

/// Variability index at or above which a run reads as interval-like. With the
/// 30 s smoothing of `gps::track_variability_index`, steady runs sit at
/// ~1.00–1.01 and rep-plus-recovery sessions at ~1.11–1.21; 1.10 splits the
/// gap. An engine
/// heuristic (Skiba GOVSS gives no boundary), so RUN-INTERVAL-VI-001 is graded
/// Weak.
pub const INTERVAL_VI_THRESHOLD: f64 = 1.10;

/// Graded verdict for whether a run's variability index marks it interval-like
/// (`true`) or steady (`false`). Carries the RUN-INTERVAL-VI-001 evidence so a
/// shell renders it with the same grade chip / why? chrome as any other
/// recommendation, the differentiation is descriptive but still evidence-cited.
pub fn interval_verdict(variability_index: f64) -> Recommended<bool> {
    Recommended::new(
        variability_index >= INTERVAL_VI_THRESHOLD,
        claims::RUN_INTERVAL_VI_001,
    )
}

#[cfg(test)]
pub(crate) mod tests;
