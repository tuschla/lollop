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

use crate::schema::{MesoPhase, Recommended, RunSessionType, ThreeZone, VdotBand};
use serde::{Deserialize, Serialize};

/// Wrap a value with the evidence + confidence of a registry claim (File 09).
///
/// Panics if `claim_id` is not in the registry: callers pass only the
/// canonical ids documented per function, so a miss is a programming error.
fn recommend<T>(value: T, claim_id: &str) -> Recommended<T> {
    let e = crate::evidence::claim(claim_id).expect("known claim");
    Recommended::new(value, e.to_evidence(), e.to_confidence_tag())
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
    matches!(
        band,
        VdotBand::Easy | VdotBand::Marathon | VdotBand::Threshold
    )
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
    recommend(violation, "RUN-VOLCAP-001")
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

/// Prescriptive block/flag on a single-session distance spike (running-029
/// HARD RULE → RUN-SPIKE-BLOCK-001, safety-critical).
///
/// `true` = block/flag. Wrapped in `Recommended` carrying RUN-SPIKE-BLOCK-001
/// evidence because it drives an action (block the session).
pub fn single_session_spike_flag(session_km: f64, longest_30d_km: f64) -> Recommended<bool> {
    recommend(
        single_session_spike(session_km, longest_30d_km),
        "RUN-SPIKE-BLOCK-001",
    )
}

// ---------------------------------------------------------------------------
// 6. Taper (prescriptive)
// ---------------------------------------------------------------------------

/// Bosquet-style taper prescription: only volume drops. RUN-TAPER-001 /
/// running-037 (safety-critical: never add stimulus, never de-intensify).
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

/// Recommend a taper `weeks_out` from the race, or `None` if too early. Rule
/// RUN-TAPER-001 / running-037/038.
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
    Some(recommend(rx, "RUN-TAPER-001"))
}

// ---------------------------------------------------------------------------
// 7. Intensity distribution (prescriptive): File 04 §"distribution" table
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
    recommend(d, "RUN-DIST-001")
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
// 8. Quality-session spacing (prescriptive validator): File 04 running-023
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
    recommend(
        QualityLimits {
            max_per_week: 3,
            min_spacing_hours: 48,
            allow_consecutive_z3: false,
        },
        "RUN-QUALITY-001",
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
    recommend(ok, "RUN-QUALITY-001")
}

// ---------------------------------------------------------------------------
// 9. Weekly mileage progression (prescriptive): File 04 running-031/028
// ---------------------------------------------------------------------------

/// Safe single-week volume-increase cap as a fraction, by training age
/// (File 04 running-031). Novice (<1 yr) tolerates up to +10 %; experienced
/// runners are held to ~+5 %. NOT the discredited hard "10 % rule", a ceiling.
pub fn weekly_increase_cap_frac(training_age_years: f64) -> Recommended<f64> {
    let cap = if training_age_years < 1.0 { 0.10 } else { 0.05 };
    recommend(cap, "RUN-PROGRESS-001")
}

/// True when next week's volume stays within the training-age increase cap
/// (File 04 running-031). A non-positive current volume cannot be ratioed → false.
pub fn weekly_increase_ok(
    current_km: f64,
    next_km: f64,
    training_age_years: f64,
) -> Recommended<bool> {
    let cap = weekly_increase_cap_frac(training_age_years).value;
    let ok = if current_km <= 0.0 {
        false
    } else {
        (next_km - current_km) / current_km <= cap + 1e-9
    };
    recommend(ok, "RUN-PROGRESS-001")
}

/// Flag elevated injury risk when weekly distance rises >30 % across two weeks
/// (File 04 running-028; Nielsen 2014 ~1.6× risk). `true` = flag. A non-positive
/// baseline (no history) flags any real increase, erring safe.
pub fn two_week_increase_flag(baseline_km: f64, current_km: f64) -> Recommended<bool> {
    let flag = if baseline_km <= 0.0 {
        current_km > 0.0
    } else {
        current_km > baseline_km * 1.30
    };
    recommend(flag, "RUN-PROGRESS-001")
}

// ---------------------------------------------------------------------------
// 10. Deload cadence (prescriptive): File 04 running-033
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
    recommend(
        DeloadCadence {
            load_weeks,
            recovery_weeks: 1,
            reduction_frac: (0.20, 0.40),
        },
        "RUN-DELOAD-001",
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
        // bound), ceilings <76 %HRmax AND <70 %HRR (no lower HR bound stated -
        // the old 0.65 floor was invented and is gone), RPE 2–3, 20–40 min.
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
            "RUN-RECOVERY-001",
        ),
        // running-015 Easy / General-Aerobic: E pace 15–25 % slower than MP,
        // 65–79 %HRmax, RPE 3–4, 30–90 min.
        RunWorkout::EasyGeneralAerobic => (
            RunWorkoutRx {
                pct_hr_max: (Some(0.65), Some(0.79)),
                pct_slower_than_mp: (Some(15.0), Some(25.0)),
                rpe: Some((3, 4)),
                duration_min: (Some(30), Some(90)),
                hr_governed: true,
                ..open
            },
            "RUN-EASY-001",
        ),
        // running-016 Long Run: E→E+ (MP −10–20 %), 65–80 %HRmax, RPE 3–5.
        // NO duration floor in the KB (the old 60-min floor was invented);
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
            "RUN-LONGRUN-001",
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
            "RUN-TEMPO-001",
        ),
        // running-017 Marathon-pace segments: M pace (0 % offset from MP by
        // definition), 80–85 %HRmax, RPE 5–6, 8–26 km blocks: the KB states
        // the blocks in km, not minutes (the old 30–120 min band was
        // invented). No weekly M-pace cap exists (VDOT table M-row cap: "-").
        RunWorkout::Session(RacePace) => (
            RunWorkoutRx {
                pct_hr_max: (Some(0.80), Some(0.85)),
                pct_slower_than_mp: (Some(0.0), Some(0.0)),
                rpe: Some((5, 6)),
                distance_km: (Some(8.0), Some(26.0)),
                hr_governed: true,
                ..open
            },
            "RUN-RACEPACE-001",
        ),
        // running-019 VO2max intervals (pace-governed, HR lags): 95–100
        // %HRmax, RPE 8–9, 3–5 min reps (structure: `vo2max_interval_rx`).
        RunWorkout::Session(Interval) => (
            RunWorkoutRx {
                pct_hr_max: (Some(0.95), Some(1.00)),
                rpe: Some((8, 9)),
                duration_min: (Some(3), Some(5)),
                ..open
            },
            "RUN-INTERVAL-001",
        ),
        // VDOT R row: >100 %VO2max, "use pace, not HR" (no HR band), reps
        // ≤2 min, ≤5 % weekly. The KB has NO dedicated R-session rule, rep
        // distance/count, recovery, and RPE are unstated (the old RPE 8–9 was
        // invented and is gone).
        RunWorkout::Session(Repetition) => (
            RunWorkoutRx {
                duration_min: (None, Some(2)),
                ..open
            },
            "RUN-REPETITION-001",
        ),
        // running-020 Strides (neuromuscular; 15–30 s reps are sub-minute, so
        // only a coarse ≤1-min row here, exact seconds in `strides_rx`).
        RunWorkout::Session(Strides) => (
            RunWorkoutRx {
                rpe: Some((6, 7)),
                duration_min: (None, Some(1)),
                ..open
            },
            "RUN-STRIDES-001",
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
            "RUN-WORKOUT-001",
        ),
    };
    recommend(rx, claim_id)
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
    recommend(
        CruiseIntervalRx {
            rep_duration_min: (3, 15),
            rest_approx_min: 1.0,
            rest_frac: (0.20, 0.25),
            weekly_cap_frac: 0.10,
        },
        "RUN-TEMPO-001",
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
    recommend(
        Vo2maxIntervalRx {
            rep_duration_min: (3, 5),
            rep_distance_m: (800, 1600),
            recovery_max_ratio_of_rep: 1.0,
            weekly_cap_frac: 0.08,
        },
        "RUN-INTERVAL-001",
    )
}

/// True when an interval rep's recovery respects running-019: recovery ≈ rep
/// time, "slightly less", i.e. recovery must not exceed the rep time. The KB
/// states no lower recovery bound, so none is enforced (a non-positive
/// recovery passes the stated rule; the KB is silent on it).
pub fn interval_recovery_ok(rep_sec: f64, recovery_sec: f64) -> Recommended<bool> {
    recommend(recovery_sec <= rep_sec, "RUN-INTERVAL-001")
}

/// True when an interval rep distance sits in the 800–1600 m band (running-019).
pub fn interval_rep_distance_ok(rep_m: f64) -> Recommended<bool> {
    recommend((800.0..=1600.0).contains(&rep_m), "RUN-INTERVAL-001")
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
    recommend(
        StridesRx {
            rep_sec: (15, 30),
            reps: (4, 8),
            recovery_sec: (45, 120),
            per_week: (1, 3),
            rpe: (6, 7),
        },
        "RUN-STRIDES-001",
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
    recommend(
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
        "RUN-HILLSPRINT-001",
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
    recommend(ok, "RUN-LONGRUN-001")
}

/// Default weekly long-run share band (running-016 + volume-caps section):
/// 25–30 % of weekly volume, as fractions. NOTE the KB discrepancy: running-024
/// gives 20–30 % for its goal table while running-016 and the caps table give
/// 25–30 %; the KB does not reconcile, so each rule keeps its own figure -
/// this is running-016's, `goal_week_plan` carries running-024's.
pub fn long_run_share_default() -> Recommended<(f64, f64)> {
    recommend((0.25, 0.30), "RUN-LONGRUN-001")
}

// ---------------------------------------------------------------------------
// Running power zones (running-010), proxy only, frameworks NOT interchangeable
// ---------------------------------------------------------------------------

/// Coggan 7-zone %FTP model (running-010). Distinct from [`StrydPowerZone`] on
/// purpose: the KB forbids treating the frameworks as interchangeable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CogganPowerZone {
    Z1,
    Z2,
    Z3,
    Z4,
    Z5,
    Z6,
    Z7,
}

/// Classify %FTP into the Coggan 7-zone table (running-010 → RUN-POWER-001,
/// Weak: consistent proxy, not a criterion metabolic measure). Verbatim bands:
/// Z1 <55, Z2 56–75, Z3 76–90, Z4 91–105, Z5 106–120, Z6 121–150, Z7 >150.
/// The KB leaves the between-band points (e.g. 55.5) unassigned, so those
/// return `None` rather than inventing a boundary rule.
pub fn coggan_power_zone(pct_ftp: f64) -> Recommended<Option<CogganPowerZone>> {
    use CogganPowerZone::*;
    let z = if pct_ftp < 55.0 {
        Some(Z1)
    } else if (56.0..=75.0).contains(&pct_ftp) {
        Some(Z2)
    } else if (76.0..=90.0).contains(&pct_ftp) {
        Some(Z3)
    } else if (91.0..=105.0).contains(&pct_ftp) {
        Some(Z4)
    } else if (106.0..=120.0).contains(&pct_ftp) {
        Some(Z5)
    } else if (121.0..=150.0).contains(&pct_ftp) {
        Some(Z6)
    } else if pct_ftp > 150.0 {
        Some(Z7)
    } else {
        None
    };
    recommend(z, "RUN-POWER-001")
}

/// Coggan sweet-spot band: 88–94 %FTP (running-010, verbatim).
pub fn coggan_sweet_spot(pct_ftp: f64) -> Recommended<bool> {
    recommend((88.0..=94.0).contains(&pct_ftp), "RUN-POWER-001")
}

/// Stryd 5-zone %CP model (running-010). CP ≈ 40-min power per Stryd docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrydPowerZone {
    Z1,
    Z2,
    Z3,
    Z4,
    Z5,
}

/// Classify %CP into the Stryd 5-zone table (running-010 → RUN-POWER-001).
/// Verbatim bands share their boundaries (Z1 65–80, Z2 80–90, Z3 90–100,
/// Z4 100–115, Z5 115–130); the KB does not say which zone owns a shared
/// boundary, so bands are taken half-open `[lo, hi)` with Z5's top inclusive.
/// Below 65 or above 130 %CP is unstated in the KB → `None`.
pub fn stryd_power_zone(pct_cp: f64) -> Recommended<Option<StrydPowerZone>> {
    use StrydPowerZone::*;
    let z = if (65.0..80.0).contains(&pct_cp) {
        Some(Z1)
    } else if (80.0..90.0).contains(&pct_cp) {
        Some(Z2)
    } else if (90.0..100.0).contains(&pct_cp) {
        Some(Z3)
    } else if (100.0..115.0).contains(&pct_cp) {
        Some(Z4)
    } else if (115.0..=130.0).contains(&pct_cp) {
        Some(Z5)
    } else {
        None
    };
    recommend(z, "RUN-POWER-001")
}

// ---------------------------------------------------------------------------
// Run/walk extension, single-variable progression, recovery-week depth
// (running-026/032/033)
// ---------------------------------------------------------------------------

/// running-026 (SAFETY-CRITICAL): for obese/very deconditioned runners, extend
/// the run/walk phase longer before continuous running to manage impact-injury
/// risk. `true` = extend. The KB states no numeric extension length and no
/// BMI/fitness threshold, the flag is qualitative by design (the adjacent
/// C25K rule already allows 9 → 10–12 weeks for the general case).
pub fn extend_run_walk_phase(obese_or_very_deconditioned: bool) -> Recommended<bool> {
    recommend(obese_or_very_deconditioned, "RUN-RUNWALK-EXT-001")
}

/// running-032: progress only ONE variable at a time, volume OR intensity,
/// never both in the same week. `true` = the week's plan is OK.
pub fn single_variable_progression_ok(
    volume_increased: bool,
    intensity_increased: bool,
) -> Recommended<bool> {
    recommend(!(volume_increased && intensity_increased), "RUN-ONEVAR-001")
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
    recommend(
        RecoveryWeekRx {
            volume_reduction_frac,
            reduce_intensity: true,
            drop_quality_session: true,
        },
        "RUN-DELOAD-DEPTH-001",
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
/// distance-specific default for (general fitness, C25K), use the generic
/// [`taper`] (running-037) there.
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
    Some(recommend(rx, "RUN-TAPER-001"))
}

// ---------------------------------------------------------------------------
// Pace re-testing & environment correction triggers (running-041)
// ---------------------------------------------------------------------------

/// Whether training paces are due for a re-test (running-041 →
/// RUN-RETEST-001): set paces from CURRENT fitness and re-test every 4–6
/// weeks. Due once ≥4 weeks have elapsed (same convention as
/// [`hr_zone_recalc_due`]'s 4–6-week window).
pub fn pace_retest_due(weeks_since_test: u8) -> Recommended<bool> {
    recommend(weeks_since_test >= 4, "RUN-RETEST-001")
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
    recommend(f, "RUN-RETEST-001")
}

/// Environment-correction triggers for prescribed paces (running-041).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaceCorrectionTriggers {
    /// Heat correction needed: temperature > ~15 °C.
    pub heat: bool,
    /// Altitude correction needed: elevation > ~900 m.
    pub altitude: bool,
}

/// Whether pace corrections are triggered (running-041): heat above ~15 °C,
/// altitude above ~900 m. The KB states ONLY these trigger thresholds, no
/// correction magnitudes (sec/km or %), so this returns flags, never adjusted
/// paces.
pub fn pace_correction_triggers(temp_c: f64, altitude_m: f64) -> Recommended<PaceCorrectionTriggers> {
    recommend(
        PaceCorrectionTriggers {
            heat: temp_c > 15.0,
            altitude: altitude_m > 900.0,
        },
        "RUN-RETEST-001",
    )
}

// ---------------------------------------------------------------------------
// Marathon realism gate (running-040)
// ---------------------------------------------------------------------------

/// Flag a marathon goal-time prediction as optimistic when the longest training
/// run is under 30 km. Rule running-040: first-timers under this threshold
/// often finish 10–15% slower than a Riegel-from-5K projection, so the estimate
/// should be derated (~2–3 VDOT points). Returns `true` when the gate trips.
pub fn marathon_prediction_optimistic(longest_run_km: f64) -> Recommended<bool> {
    recommend(longest_run_km < 30.0, "RUN-VDOT-001")
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
    let rel_diff = (hi - lo) / lo;
    let out = if rel_diff <= 0.02 {
        Equivalency::Agreed((riegel_sec + daniels_sec) / 2.0)
    } else {
        Equivalency::Range(lo, hi)
    };
    recommend(out, "RUN-VDOT-001")
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
    recommend(p, "RUN-KARVONEN-001")
}

/// Prefer Karvonen (%HRR) over %HRmax when resting HR is low, where the two
/// methods diverge substantially (running-005): true below RHR 55; at RHR ≥70
/// they converge and either is acceptable. Boolean view of
/// [`hr_method_preference`]. RUN-KARVONEN-001.
pub fn prefer_karvonen(resting_hr_bpm: f64) -> Recommended<bool> {
    let p = hr_method_preference(resting_hr_bpm);
    Recommended::new(
        p.value == HrMethodPreference::PreferKarvonen,
        p.evidence,
        p.confidence,
    )
}

/// Whether HR training zones are due for recalculation (running-006): recompute
/// every 4–6 weeks off a measured max HR. Due once ≥4 weeks have elapsed.
/// RUN-HRRECALC-001 (Strong, safety-critical, stale zones misplace every
/// prescription).
pub fn hr_zone_recalc_due(weeks_since_recalc: u8) -> Recommended<bool> {
    recommend(weeks_since_recalc >= 4, "RUN-HRRECALC-001")
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
    recommend(d, "RUN-VDOT-001")
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
/// `advanced` runners extend to 5–7 sessions. Long-run share is 20–30% of
/// weekly volume across goals. RUN-WORKOUT-001.
pub fn goal_week_plan(goal: GoalDistance, advanced: bool) -> Recommended<GoalWeekPlan> {
    let lr = (0.20, 0.30);
    let plan = match goal {
        GoalDistance::General => GoalWeekPlan {
            sessions_per_week: (3, 5),
            quality_per_week: (0, 1),
            long_run_share: lr,
        },
        GoalDistance::C25k => GoalWeekPlan {
            sessions_per_week: (3, 3),
            quality_per_week: (0, 0),
            long_run_share: lr,
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
    recommend(plan, "RUN-WORKOUT-001")
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
    recommend(
        C25kPlan {
            runs_per_week: 3,
            weeks: (9, 12),
            rest_day_between: true,
            repeat_hard_week_allowed: true,
        },
        "RUN-C25K-001",
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
    recommend(180.0 - age_years + delta, "RUN-MAF-001")
}

// ---------------------------------------------------------------------------
// Distribution invariant & counting method (running-011/012)
// ---------------------------------------------------------------------------

/// Enforce the architectural invariant that ~80% of running time is easy
/// (running-011): true when the easy (Z1) time share is at least 80%. RUN-DIST-001.
pub fn easy_share_floor_ok(easy_frac_by_time: f64) -> Recommended<bool> {
    recommend(easy_frac_by_time >= 0.80, "RUN-DIST-001")
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
    recommend(m, "RUN-DIST-001")
}

// ---------------------------------------------------------------------------
// Volume-bump hold & unscheduled deload (running-031/034)
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
    recommend(hold, "RUN-PROGRESS-001")
}

/// Insert an unscheduled down week when ≥2 overtraining signals fire
/// (running-034): elevated RHR >5–7 bpm ≥3 days, HRV trending down, RPE rising,
/// disrupted sleep/soreness/mood, or standard-workout performance down >3–5%.
/// RUN-DOWNWEEK-001 (ExpertOpinion, safety-critical recovery guard).
pub fn unscheduled_deload(overtraining_signal_count: u8) -> Recommended<bool> {
    recommend(overtraining_signal_count >= 2, "RUN-DOWNWEEK-001")
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
    2.0 * EARTH_RADIUS_M * h.sqrt().asin()
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

/// Total track distance in km. Fixes whose reported horizontal accuracy is
/// worse than `max_accuracy_m` are dropped first so GPS noise does not inflate
/// distance. Pure and order-dependent (order = fix order from the shell).
pub fn track_distance_km(points: &[GpsPoint], max_accuracy_m: f32) -> f64 {
    usable_track(points, max_accuracy_m)
        .windows(2)
        .map(|w| haversine_m(w[0], w[1]))
        .sum::<f64>()
        / 1000.0
}

/// Elapsed wall time across a track, in minutes, from the first to the last
/// *usable* fix (same accuracy gate as [track_distance_km], so derived pace is
/// consistent). Returns 0.0 for an empty/single-fix or non-monotonic track.
pub fn track_duration_min(points: &[GpsPoint], max_accuracy_m: f32) -> f64 {
    let usable = usable_track(points, max_accuracy_m);
    match (usable.first(), usable.last()) {
        (Some(f), Some(l)) if l.observed_at > f.observed_at => {
            (l.observed_at - f.observed_at) as f64 / 60.0
        }
        _ => 0.0,
    }
}

/// Second-half-vs-first-half pace slowdown across a track, as a percent (a
/// *positive split*: positive = the runner slowed in the back half). The track
/// is split at its halfway distance and each half's pace (time ÷ distance) is
/// compared; units cancel in the ratio. Purely descriptive, this is a
/// measurement of the run, not a recommendation. Returns `None` when the track
/// is too short, degenerate, or non-monotonic to split meaningfully (fewer than
/// three usable fixes, a zero-distance/zero-duration half). Same accuracy gate
/// as [track_distance_km]/[track_duration_min] so all three agree on the track.
pub fn track_positive_split_pct(points: &[GpsPoint], max_accuracy_m: f32) -> Option<f64> {
    let usable = usable_track(points, max_accuracy_m);
    if usable.len() < 3 {
        return None;
    }
    // Cumulative distance to each fix (cumulative[0] == 0.0).
    let mut cumulative = Vec::with_capacity(usable.len());
    let mut running_total = 0.0;
    cumulative.push(0.0);
    for w in usable.windows(2) {
        running_total += haversine_m(w[0], w[1]);
        cumulative.push(running_total);
    }
    let total = running_total;
    if total <= 0.0 {
        return None;
    }
    // First interior fix that reaches the halfway distance is the split point.
    let half = total / 2.0;
    let split = (1..usable.len() - 1).find(|&i| cumulative[i] >= half)?;

    let (first, mid, last) = (usable[0], usable[split], usable[usable.len() - 1]);
    let dist1 = cumulative[split];
    let dist2 = total - dist1;
    let dur1 = (mid.observed_at - first.observed_at) as f64;
    let dur2 = (last.observed_at - mid.observed_at) as f64;
    if dist1 <= 0.0 || dist2 <= 0.0 || dur1 <= 0.0 || dur2 <= 0.0 {
        return None;
    }
    let pace1 = dur1 / dist1;
    let pace2 = dur2 / dist2;
    Some(((pace2 - pace1) / pace1 * 100.0 * 10.0).round() / 10.0)
}

/// Serialise a fix track to a GPX 1.1 document (the format Strava, Garmin
/// Connect, Komoot, etc. import). Pure and deterministic: `observed_at` unix
/// seconds are rendered as RFC 3339 UTC timestamps in-core, never from a live
/// clock. Elevation is omitted: the shell does not yet capture altitude.
pub fn export_gpx(points: &[GpsPoint], track_name: &str) -> String {
    let mut s = String::with_capacity(256 + points.len() * 96);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(
        "<gpx version=\"1.1\" creator=\"fitness_anlage\" \
         xmlns=\"http://www.topografix.com/GPX/1/1\">\n",
    );
    s.push_str("  <trk>\n    <name>");
    xml_escape_into(&mut s, track_name);
    s.push_str("</name>\n    <trkseg>\n");
    for p in points {
        s.push_str("      <trkpt lat=\"");
        s.push_str(&format!("{:.7}", p.lat));
        s.push_str("\" lon=\"");
        s.push_str(&format!("{:.7}", p.lon));
        s.push_str("\"><time>");
        s.push_str(&unix_to_rfc3339_utc(p.observed_at));
        s.push_str("</time></trkpt>\n");
    }
    s.push_str("    </trkseg>\n  </trk>\n</gpx>\n");
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
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (h, m, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Convert a count of days since the unix epoch (1970-01-01) to a civil
/// `(year, month, day)`. Howard Hinnant, "chrono-Compatible Low-Level Date
/// Algorithms" (public domain).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(lat: f64, lon: f64, t: i64) -> GpsPoint {
        GpsPoint {
            lat,
            lon,
            observed_at: t,
            accuracy_m: 5.0,
        }
    }

    #[test]
    fn haversine_one_degree_latitude_is_about_111km() {
        // One degree of latitude ≈ R * (π/180) ≈ 111.19 km on a sphere.
        let d = haversine_m(pt(0.0, 0.0, 0), pt(1.0, 0.0, 0));
        assert!((d - 111_194.0).abs() < 50.0, "got {d}");
    }

    #[test]
    fn positive_split_detects_back_half_slowdown() {
        // Five equatorial fixes 0.001° apart → four equal ~111.3 m segments.
        // First half covered in 20 s, second half in 40 s → back half is exactly
        // twice as slow, i.e. a +100 % positive split.
        let track = vec![
            pt(0.0, 0.000, 0),
            pt(0.0, 0.001, 10),
            pt(0.0, 0.002, 20),
            pt(0.0, 0.003, 40),
            pt(0.0, 0.004, 60),
        ];
        let split = track_positive_split_pct(&track, 30.0).expect("split");
        assert!((split - 100.0).abs() < 1.0, "got {split}");
    }

    #[test]
    fn even_pace_run_reports_no_meaningful_split() {
        let track = vec![
            pt(0.0, 0.000, 0),
            pt(0.0, 0.001, 10),
            pt(0.0, 0.002, 20),
            pt(0.0, 0.003, 30),
            pt(0.0, 0.004, 40),
        ];
        let split = track_positive_split_pct(&track, 30.0).expect("split");
        assert!(split.abs() < 1.0, "got {split}");
    }

    #[test]
    fn positive_split_needs_three_usable_fixes() {
        let track = vec![pt(0.0, 0.0, 0), pt(0.0, 0.001, 10)];
        assert!(track_positive_split_pct(&track, 30.0).is_none());
    }

    #[test]
    fn track_distance_sums_segments_and_drops_noisy_fixes() {
        let clean = vec![pt(0.0, 0.0, 0), pt(0.0, 0.001, 10), pt(0.0, 0.002, 20)];
        let d = track_distance_km(&clean, 30.0);
        // Two equal ~111.3 m equatorial-lon segments ≈ 0.2226 km.
        assert!((d - 0.2226).abs() < 0.001, "got {d}");

        // A noisy middle fix (accuracy 50 m) is dropped, leaving one long segment.
        let noisy = vec![
            pt(0.0, 0.0, 0),
            GpsPoint {
                lat: 5.0,
                lon: 5.0,
                observed_at: 10,
                accuracy_m: 50.0,
            },
            pt(0.0, 0.002, 20),
        ];
        let d2 = track_distance_km(&noisy, 30.0);
        assert!((d2 - 0.2226).abs() < 0.001, "got {d2}");
    }

    #[test]
    fn track_duration_is_first_to_last_span() {
        let t = vec![pt(0.0, 0.0, 100), pt(0.0, 0.001, 400)];
        assert!((track_duration_min(&t, 30.0) - 5.0).abs() < 1e-9);
        assert_eq!(track_duration_min(&[], 30.0), 0.0);
        assert_eq!(track_duration_min(&[pt(0.0, 0.0, 100)], 30.0), 0.0);
    }

    #[test]
    fn track_duration_spans_only_usable_fixes() {
        // A noisy first fix must not inflate duration past the usable segment,
        // which would otherwise skew pace low against the accuracy-filtered
        // distance.
        let t = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.0,
                observed_at: 0,
                accuracy_m: 99.0,
            },
            pt(0.0, 0.001, 120),
            pt(0.0, 0.002, 300),
        ];
        // Usable span is 120..300 = 3 min, not 0..300 = 5 min.
        assert!((track_duration_min(&t, 30.0) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn rfc3339_renders_known_epochs_utc() {
        assert_eq!(unix_to_rfc3339_utc(0), "1970-01-01T00:00:00Z");
        // 1_700_000_000 → 2023-11-14T22:13:20Z (verified against a UTC clock).
        assert_eq!(unix_to_rfc3339_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn export_gpx_wraps_fixes_and_escapes_name() {
        let track = vec![
            pt(52.52, 13.405, 1_700_000_000),
            pt(52.521, 13.406, 1_700_000_030),
        ];
        let gpx = export_gpx(&track, "Tom & \"Jerry\" <run>'s");
        assert!(gpx.starts_with("<?xml version=\"1.0\""));
        assert!(gpx.contains("<gpx version=\"1.1\""));
        // All five XML predefined entities are escaped so the name cannot break
        // the document (& " < > ').
        assert!(gpx.contains("<name>Tom &amp; &quot;Jerry&quot; &lt;run&gt;&apos;s</name>"));
        // One trkpt per fix, with lat/lon and an RFC-3339 timestamp.
        assert_eq!(gpx.matches("<trkpt ").count(), 2);
        assert!(gpx.contains("lat=\"52.5200000\" lon=\"13.4050000\""));
        assert!(gpx.contains("<time>2023-11-14T22:13:20Z</time>"));
        assert!(gpx.trim_end().ends_with("</gpx>"));
    }

    #[test]
    fn usable_track_drops_noisy_fixes() {
        let track = vec![
            pt(52.52, 13.405, 0),
            GpsPoint {
                lat: 52.60,
                lon: 13.50,
                observed_at: 30,
                accuracy_m: 80.0, // beyond the gate → excluded
            },
            pt(52.521, 13.406, 60),
        ];
        // The GPX must carry the same fixes distance uses, the noisy one gone.
        let usable = usable_track(&track, MAX_GPS_ACCURACY_M);
        assert_eq!(usable.len(), 2);
        let gpx = export_gpx(&usable, "run");
        assert_eq!(gpx.matches("<trkpt ").count(), 2);
        assert!(!gpx.contains("lat=\"52.6000000\""));
    }

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
    fn hr_method_and_recalc() {
        assert!(prefer_karvonen(50.0).value);
        assert!(!prefer_karvonen(55.0).value);
        assert!(!prefer_karvonen(72.0).value);
        assert!(!hr_zone_recalc_due(3).value);
        assert!(hr_zone_recalc_due(4).value);
    }

    #[test]
    fn vdot_derate_and_goal_plan() {
        assert_eq!(
            vdot_derate_points(GoalDistance::HalfMarathon, true).value,
            (1.0, 1.5)
        );
        assert_eq!(
            vdot_derate_points(GoalDistance::Marathon, true).value,
            (2.0, 3.0)
        );
        assert_eq!(
            vdot_derate_points(GoalDistance::FiveK, true).value,
            (0.0, 0.0)
        );
        assert_eq!(
            vdot_derate_points(GoalDistance::Marathon, false).value,
            (0.0, 0.0)
        );
        let c = goal_week_plan(GoalDistance::C25k, false).value;
        assert_eq!((c.sessions_per_week, c.quality_per_week), ((3, 3), (0, 0)));
        assert_eq!(
            goal_week_plan(GoalDistance::FiveK, false)
                .value
                .quality_per_week,
            (2, 2)
        );
        assert_eq!(
            goal_week_plan(GoalDistance::Marathon, true)
                .value
                .sessions_per_week,
            (5, 7)
        );
        assert_eq!(
            goal_week_plan(GoalDistance::Marathon, false)
                .value
                .sessions_per_week,
            (4, 6)
        );
        let p = c25k_plan().value;
        assert_eq!(
            (
                p.runs_per_week,
                p.rest_day_between,
                p.repeat_hard_week_allowed
            ),
            (3, true, true)
        );
    }

    #[test]
    fn distribution_and_progression_guards() {
        assert!(easy_share_floor_ok(0.80).value);
        assert!(!easy_share_floor_ok(0.79).value);
        assert_eq!(
            default_counting_method(true).value,
            IntensityCountingMethod::SessionGoal
        );
        assert_eq!(
            default_counting_method(false).value,
            IntensityCountingMethod::TimeInZone
        );
        assert_eq!(novice_volume_bump_hold_weeks(0.5).value, Some((2, 3)));
        assert_eq!(novice_volume_bump_hold_weeks(3.0).value, None);
        assert!(unscheduled_deload(2).value);
        assert!(!unscheduled_deload(1).value);
    }

    #[test]
    fn maf_cap_adjusts() {
        assert_eq!(maf_cap_bpm(40.0, MafAdjustment::None).value, 140.0);
        assert_eq!(
            maf_cap_bpm(40.0, MafAdjustment::EliteImproving).value,
            145.0
        );
        assert_eq!(maf_cap_bpm(40.0, MafAdjustment::Returning).value, 135.0);
        assert_eq!(maf_cap_bpm(40.0, MafAdjustment::Overtrained).value, 130.0);
        // Weak + contested per global CQ-12 (MAF vs measured LT1).
        assert!(maf_cap_bpm(40.0, MafAdjustment::None).confidence.contested);
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
    fn distribution_models_keep_easy_at_80() {
        let pyr = intensity_distribution(DistributionModel::Pyramidal).value;
        assert_eq!((pyr.easy_pct, pyr.moderate_pct, pyr.hard_pct), (80, 15, 5));
        let pol = intensity_distribution(DistributionModel::Polarized).value;
        assert_eq!((pol.easy_pct, pol.moderate_pct, pol.hard_pct), (80, 5, 15));
        // Phase mapping: base pyramidal, peak polarized.
        assert_eq!(distribution_for_phase(MesoPhase::Base).value.hard_pct, 5);
        assert_eq!(distribution_for_phase(MesoPhase::Peak).value.hard_pct, 15);
    }

    #[test]
    fn quality_plan_respects_caps() {
        // 3 sessions, 48h apart, no back-to-back Z3: ok.
        assert!(quality_plan_ok(3, 48, false).value);
        // 4 sessions: too many.
        assert!(!quality_plan_ok(4, 48, false).value);
        // Too tight spacing.
        assert!(!quality_plan_ok(2, 24, false).value);
        // Consecutive Z3 never allowed.
        assert!(!quality_plan_ok(2, 48, true).value);
    }

    #[test]
    fn weekly_increase_cap_scales_with_training_age() {
        assert!((weekly_increase_cap_frac(0.5).value - 0.10).abs() < 1e-9);
        assert!((weekly_increase_cap_frac(3.0).value - 0.05).abs() < 1e-9);
        // Novice +10% ok, experienced +10% not.
        assert!(weekly_increase_ok(50.0, 55.0, 0.5).value);
        assert!(!weekly_increase_ok(50.0, 55.0, 3.0).value);
        // Zero base cannot be ratioed.
        assert!(!weekly_increase_ok(0.0, 10.0, 0.5).value);
    }

    #[test]
    fn two_week_spike_flags_above_30pct() {
        // +30% exactly: not flagged (rule is strictly >30%).
        assert!(!two_week_increase_flag(40.0, 52.0).value);
        // Just above 30%: flagged.
        assert!(two_week_increase_flag(40.0, 52.1).value);
        // No history errs to flag.
        assert!(two_week_increase_flag(0.0, 10.0).value);
    }

    #[test]
    fn deload_cadence_defaults_and_conservative() {
        let default = deload_cadence(false).value;
        assert_eq!((default.load_weeks, default.recovery_weeks), (3, 1));
        assert_eq!(deload_cadence(true).value.load_weeks, 2);
        let (lo, hi) = default.reduction_frac;
        assert!((lo - 0.20).abs() < 1e-9 && (hi - 0.40).abs() < 1e-9);
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

    #[test]
    fn workout_rx_hr_governance_split() {
        // Aerobic sessions are HR-governed.
        assert!(run_workout_rx(RunSessionType::Recovery).value.hr_governed);
        assert!(run_workout_rx(RunSessionType::LongRun).value.hr_governed);
        assert!(run_workout_rx(RunSessionType::Tempo).value.hr_governed);
        assert!(run_workout_rx(RunSessionType::RacePace).value.hr_governed);
        assert!(workout_rx(RunWorkout::EasyGeneralAerobic).value.hr_governed);
        // Short/max efforts are effort-governed, HR lags.
        assert!(!run_workout_rx(RunSessionType::Interval).value.hr_governed);
        assert!(!run_workout_rx(RunSessionType::Repetition).value.hr_governed);
        assert!(!run_workout_rx(RunSessionType::Strides).value.hr_governed);
        assert!(!run_workout_rx(RunSessionType::Hills).value.hr_governed);
    }

    #[test]
    fn workout_rx_bands_verbatim() {
        // running-018 Tempo 88–92 %HRmax, RPE 6–7, 20–40 min continuous.
        let t = run_workout_rx(RunSessionType::Tempo).value;
        assert_eq!(t.pct_hr_max, (Some(0.88), Some(0.92)));
        assert_eq!(t.rpe, Some((6, 7)));
        assert_eq!(t.duration_min, (Some(20), Some(40)));
        // running-019 Interval 95–100 %HRmax, 3–5 min reps.
        let i = run_workout_rx(RunSessionType::Interval).value;
        assert_eq!(i.pct_hr_max, (Some(0.95), Some(1.00)));
        assert_eq!(i.duration_min, (Some(3), Some(5)));
        assert_eq!(i.rpe, Some((8, 9)));
    }

    #[test]
    fn recovery_rx_has_open_lower_hr_and_both_ceilings() {
        // running-014: KB gives NO lower HR bound (the 0.65 floor was
        // invented), <76 %HRmax AND <70 %HRR ceilings, pace ≥20 % slower than
        // MP with no upper slowness bound, RPE 2–3, 20–40 min.
        let r = run_workout_rx(RunSessionType::Recovery);
        assert_eq!(r.value.pct_hr_max, (None, Some(0.76)));
        assert_eq!(r.value.pct_hrr_max, Some(0.70));
        assert_eq!(r.value.pct_slower_than_mp, (Some(20.0), None));
        assert_eq!(r.value.rpe, Some((2, 3)));
        assert_eq!(r.value.duration_min, (Some(20), Some(40)));
        assert_eq!(
            r.evidence.citation.claim_id.as_deref(),
            Some("RUN-RECOVERY-001")
        );
    }

    #[test]
    fn easy_ga_is_distinct_from_recovery() {
        // running-015: E pace 15–25 % slower than MP, 65–79 %HRmax, RPE 3–4,
        // 30–90 min, a different band set from Recovery on every axis.
        let e = workout_rx(RunWorkout::EasyGeneralAerobic);
        assert_eq!(e.value.pct_hr_max, (Some(0.65), Some(0.79)));
        assert_eq!(e.value.pct_slower_than_mp, (Some(15.0), Some(25.0)));
        assert_eq!(e.value.rpe, Some((3, 4)));
        assert_eq!(e.value.duration_min, (Some(30), Some(90)));
        assert_eq!(e.evidence.citation.claim_id.as_deref(), Some("RUN-EASY-001"));
        assert_ne!(e.value, run_workout_rx(RunSessionType::Recovery).value);
    }

    #[test]
    fn long_run_rx_has_no_duration_floor() {
        // running-016: no duration floor in the KB (the 60-min floor was
        // invented); soft time-cap ~2:00–2:30 → 150-min outer ceiling; pace
        // MP −10–20 %; 65–80 %HRmax; RPE 3–5.
        let lr = run_workout_rx(RunSessionType::LongRun).value;
        assert_eq!(lr.duration_min, (None, Some(150)));
        assert_eq!(lr.pct_hr_max, (Some(0.65), Some(0.80)));
        assert_eq!(lr.pct_slower_than_mp, (Some(10.0), Some(20.0)));
        assert_eq!(lr.rpe, Some((3, 5)));
    }

    #[test]
    fn race_pace_rx_is_km_blocks_not_minutes() {
        // running-017: 8–26 km blocks (KB states km, not minutes), M pace,
        // 80–85 %HRmax, RPE 5–6. No weekly M cap exists (VDOT M row: "-").
        let m = run_workout_rx(RunSessionType::RacePace).value;
        assert_eq!(m.distance_km, (Some(8.0), Some(26.0)));
        assert_eq!(m.duration_min, (None, None));
        assert_eq!(m.pct_hr_max, (Some(0.80), Some(0.85)));
        assert_eq!(m.pct_slower_than_mp, (Some(0.0), Some(0.0)));
        assert_eq!(m.rpe, Some((5, 6)));
    }

    #[test]
    fn repetition_rx_carries_only_kb_backed_parameters() {
        // VDOT R row only: reps ≤2 min, pace-not-HR. RPE was NOT stated in the
        // KB (the old (8,9) was invented) and no HR band exists.
        let r = run_workout_rx(RunSessionType::Repetition);
        assert_eq!(r.value.rpe, None);
        assert_eq!(r.value.pct_hr_max, (None, None));
        assert_eq!(r.value.duration_min, (None, Some(2)));
        assert!(!r.value.hr_governed);
        assert_eq!(
            r.evidence.citation.claim_id.as_deref(),
            Some("RUN-REPETITION-001")
        );
    }

    #[test]
    fn cruise_intervals_verbatim() {
        // running-018: 3–15 min reps, rest ~1 min or 20–25 %, total ≤10 %.
        let c = cruise_interval_rx().value;
        assert_eq!(c.rep_duration_min, (3, 15));
        assert!((c.rest_approx_min - 1.0).abs() < 1e-9);
        assert_eq!(c.rest_frac, (0.20, 0.25));
        assert!((c.weekly_cap_frac - 0.10).abs() < 1e-9);
    }

    #[test]
    fn vo2max_interval_structure_and_gates() {
        // running-019: 3–5 min reps, 800–1600 m, recovery ≈ rep time.
        let i = vo2max_interval_rx();
        assert_eq!(i.value.rep_duration_min, (3, 5));
        assert_eq!(i.value.rep_distance_m, (800, 1600));
        assert!((i.value.weekly_cap_frac - 0.08).abs() < 1e-9);
        // Contested CQ-10 (Rønnestad short intervals) must surface.
        assert!(i.confidence.contested);
        // Recovery ≤ rep time passes; longer recovery fails.
        assert!(interval_recovery_ok(240.0, 240.0).value);
        assert!(interval_recovery_ok(240.0, 200.0).value);
        assert!(!interval_recovery_ok(240.0, 241.0).value);
        // Rep distance band 800–1600 m inclusive.
        assert!(interval_rep_distance_ok(800.0).value);
        assert!(interval_rep_distance_ok(1600.0).value);
        assert!(!interval_rep_distance_ok(799.0).value);
        assert!(!interval_rep_distance_ok(1601.0).value);
    }

    #[test]
    fn strides_and_hill_sprints_verbatim() {
        let s = strides_rx().value;
        assert_eq!(
            (s.rep_sec, s.reps, s.recovery_sec, s.per_week, s.rpe),
            ((15, 30), (4, 8), (45, 120), (1, 3), (6, 7))
        );
        let h = hill_sprint_rx().value;
        assert_eq!((h.rep_sec, h.reps), ((8, 20), (4, 10)));
        assert_eq!(h.effort_pct, (90.0, 95.0));
        assert_eq!(h.grade_pct, (6.0, 10.0));
        assert_eq!(h.recovery_approx_sec, 120);
        assert_eq!(h.rpe, 9);
        // Weekly frequency is NOT stated in the KB → never invented.
        assert_eq!(h.per_week, None);
        assert!(h.on_easy_days);
        // Placement contradiction (Lydiard vs Hudson) must ride along.
        assert!(!hill_sprint_rx().evidence.contradicting.is_empty());
    }

    #[test]
    fn long_run_guardrails() {
        // running-016: LR ≤ 2× average daily run.
        assert!(long_run_within_daily_avg(16.0, 8.0).value);
        assert!(!long_run_within_daily_avg(16.1, 8.0).value);
        // No history errs safe.
        assert!(!long_run_within_daily_avg(10.0, 0.0).value);
        // Share default 25–30 % (running-016 figure; running-024's 20–30 % is
        // documented as a KB discrepancy and kept on goal_week_plan).
        assert_eq!(long_run_share_default().value, (0.25, 0.30));
        assert_eq!(
            goal_week_plan(GoalDistance::General, false).value.long_run_share,
            (0.20, 0.30)
        );
    }

    #[test]
    fn coggan_zones_verbatim_with_unstated_gaps() {
        use CogganPowerZone::*;
        assert_eq!(coggan_power_zone(54.9).value, Some(Z1));
        assert_eq!(coggan_power_zone(56.0).value, Some(Z2));
        assert_eq!(coggan_power_zone(75.0).value, Some(Z2));
        assert_eq!(coggan_power_zone(90.0).value, Some(Z3));
        assert_eq!(coggan_power_zone(105.0).value, Some(Z4));
        assert_eq!(coggan_power_zone(120.0).value, Some(Z5));
        assert_eq!(coggan_power_zone(150.0).value, Some(Z6));
        assert_eq!(coggan_power_zone(150.1).value, Some(Z7));
        // The KB leaves 55–56 (and the other between-band points) unassigned.
        assert_eq!(coggan_power_zone(55.5).value, None);
        // Sweet spot 88–94.
        assert!(coggan_sweet_spot(88.0).value && coggan_sweet_spot(94.0).value);
        assert!(!coggan_sweet_spot(87.9).value && !coggan_sweet_spot(94.1).value);
        // Weak grade must ride along (proxy, not criterion). Schema maps
        // Weak → 0.40 (File 09 header); File 04's per-rule 0.30 for Weak is a
        // KB-internal inconsistency, resolved codebase-wide by the grade enum.
        let c = coggan_power_zone(80.0);
        assert_eq!(c.evidence.grade, crate::schema::EvidenceGrade::Weak);
        assert!((c.confidence.score - 0.40).abs() < f32::EPSILON);
    }

    #[test]
    fn stryd_zones_half_open_bands() {
        use StrydPowerZone::*;
        assert_eq!(stryd_power_zone(65.0).value, Some(Z1));
        // Shared boundaries go to the upper zone (half-open bands).
        assert_eq!(stryd_power_zone(80.0).value, Some(Z2));
        assert_eq!(stryd_power_zone(90.0).value, Some(Z3));
        assert_eq!(stryd_power_zone(100.0).value, Some(Z4));
        assert_eq!(stryd_power_zone(115.0).value, Some(Z5));
        assert_eq!(stryd_power_zone(130.0).value, Some(Z5));
        // Outside the stated table → unstated, never invented.
        assert_eq!(stryd_power_zone(64.9).value, None);
        assert_eq!(stryd_power_zone(130.1).value, None);
    }

    #[test]
    fn hr_method_preference_branches() {
        // running-005: <55 prefer Karvonen; ≥70 converged; [55,70) unstated.
        assert_eq!(
            hr_method_preference(54.9).value,
            HrMethodPreference::PreferKarvonen
        );
        assert_eq!(hr_method_preference(55.0).value, HrMethodPreference::Unstated);
        assert_eq!(hr_method_preference(69.9).value, HrMethodPreference::Unstated);
        assert_eq!(
            hr_method_preference(70.0).value,
            HrMethodPreference::EitherConverged
        );
    }

    #[test]
    fn run_walk_extension_is_safety_critical() {
        // running-026 (safety_critical true in the KB).
        let x = extend_run_walk_phase(true);
        assert!(x.value);
        assert!(x.confidence.safety_critical);
        assert!(!extend_run_walk_phase(false).value);
    }

    #[test]
    fn single_variable_progression_gate() {
        // running-032: volume OR intensity, never both in one week.
        assert!(single_variable_progression_ok(true, false).value);
        assert!(single_variable_progression_ok(false, true).value);
        assert!(single_variable_progression_ok(false, false).value);
        assert!(!single_variable_progression_ok(true, true).value);
    }

    #[test]
    fn recovery_week_depth_by_mileage() {
        // running-033: general 20–40 %, higher-mileage 10–30 %, lower-mileage
        // up to 50 % (floor unstated); both volume and intensity reduced, one
        // quality session dropped.
        let g = recovery_week_rx(MileageBand::Unspecified).value;
        assert_eq!(g.volume_reduction_frac, (Some(0.20), 0.40));
        let hi = recovery_week_rx(MileageBand::Higher).value;
        assert_eq!(hi.volume_reduction_frac, (Some(0.10), 0.30));
        let lo = recovery_week_rx(MileageBand::Lower).value;
        assert_eq!(lo.volume_reduction_frac, (None, 0.50));
        assert!(g.reduce_intensity && g.drop_quality_session);
    }

    #[test]
    fn distance_taper_defaults_verbatim() {
        // running-038 (Strong, safety-critical): distance-specific defaults.
        let five = distance_taper(GoalDistance::FiveK).expect("5K default");
        assert_eq!(five.value.days, (7, 10));
        assert_eq!(five.value.volume_cut_frac, (0.40, 0.50));
        assert_eq!(five.value.keep_sharp_sessions, Some((1, 2)));
        assert!(five.confidence.safety_critical);
        let ten = distance_taper(GoalDistance::TenK).expect("10K default");
        assert_eq!(ten.value.days, (7, 10));
        let hm = distance_taper(GoalDistance::HalfMarathon).expect("HM default");
        assert_eq!(hm.value.days, (10, 14));
        assert_eq!(hm.value.volume_cut_frac, (0.50, 0.50));
        assert!(!hm.value.progressive);
        let m = distance_taper(GoalDistance::Marathon).expect("marathon default");
        assert_eq!(m.value.days, (14, 21));
        assert_eq!(m.value.volume_cut_frac, (0.40, 0.60));
        assert!(m.value.progressive && m.value.keep_mp_touches_and_short_tempo);
        // Intensity + frequency always held; no defaults stated for
        // general/C25K goals.
        assert!(m.value.hold_intensity && m.value.hold_frequency);
        assert!(distance_taper(GoalDistance::General).is_none());
        assert!(distance_taper(GoalDistance::C25k).is_none());
    }

    #[test]
    fn pace_retest_and_environment_triggers() {
        // running-041: re-test every 4–6 wk.
        assert!(!pace_retest_due(3).value);
        assert!(pace_retest_due(4).value);
        // Race input freshness window ≤6–8 wk.
        assert_eq!(race_input_freshness(6).value, RaceInputFreshness::Fresh);
        assert_eq!(race_input_freshness(7).value, RaceInputFreshness::Marginal);
        assert_eq!(race_input_freshness(8).value, RaceInputFreshness::Marginal);
        assert_eq!(race_input_freshness(9).value, RaceInputFreshness::Stale);
        // Corrections trigger above ~15 °C / ~900 m, flags only, the KB
        // states no magnitudes.
        let t = pace_correction_triggers(15.1, 900.1).value;
        assert!(t.heat && t.altitude);
        let none = pace_correction_triggers(15.0, 900.0).value;
        assert!(!none.heat && !none.altitude);
    }

    #[test]
    fn marathon_gate_trips_under_30km() {
        assert!(marathon_prediction_optimistic(24.0).value);
        assert!(!marathon_prediction_optimistic(32.0).value);
        // Boundary: exactly 30 km is not optimistic.
        assert!(!marathon_prediction_optimistic(30.0).value);
    }

    #[test]
    fn equivalency_agrees_or_ranges() {
        // Within 2 %: midpoint.
        match race_equivalency(3600.0, 3636.0).value {
            Equivalency::Agreed(m) => assert!((m - 3618.0).abs() < 1e-9),
            other => panic!("expected Agreed, got {other:?}"),
        }
        // >2 % apart: range, low first.
        match race_equivalency(3800.0, 3600.0).value {
            Equivalency::Range(lo, hi) => {
                assert!((lo - 3600.0).abs() < 1e-9 && (hi - 3800.0).abs() < 1e-9)
            }
            other => panic!("expected Range, got {other:?}"),
        }
    }
}
