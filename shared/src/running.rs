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
    // running-039: the Riegel/Daniels equivalency combiner (RUN-EQUIV-001), not
    // the VDOT-fitness estimate (RUN-VDOT-001).
    recommend(out, "RUN-EQUIV-001")
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

/// Derate an optimistic marathon finish-time band for an under-mileaged runner
/// (running-008/040, option B, "flag AND derate"). The prediction band
/// `(low_sec, high_sec)` is shifted SLOWER by the same VDOT-point derate that
/// [`vdot_derate_points`] cites for the marathon: the fast bound is slowed by
/// the *minimum* derate and the slow bound by the *maximum*, so the displayed
/// range moves later (pace easier) and widens by the derate's own uncertainty.
/// `base_vdot` (from the runner's recent race) anchors the point→time
/// conversion through [`load::daniels_predict`] at the marathon distance, no
/// new magnitude is invented, it is the same 2–3 VDOT points the caveat states.
/// Carries RUN-VDOT-001 so the adjusted number still travels with its grade
/// (HARD RULE 2). A degenerate band or non-positive VDOT is returned unchanged.
pub fn marathon_derated_band(
    low_sec: f64,
    high_sec: f64,
    base_vdot: f64,
) -> Recommended<(f64, f64)> {
    let derate = vdot_derate_points(GoalDistance::Marathon, true);
    let (min_pts, max_pts) = derate.value;
    let base_time = crate::load::daniels_predict(base_vdot, 42_195.0);
    // Translate the VDOT-point derate into a time slowdown factor via the same
    // Daniels marathon prediction the band is built from; guard degenerate
    // inputs (no valid race → no prediction) by leaving the band untouched.
    let (lo, hi) = if base_vdot <= 0.0 || base_time <= 0.0 || low_sec <= 0.0 {
        (low_sec, high_sec)
    } else {
        let f_min = crate::load::daniels_predict(base_vdot - min_pts, 42_195.0) / base_time;
        let f_max = crate::load::daniels_predict(base_vdot - max_pts, 42_195.0) / base_time;
        (low_sec * f_min, high_sec * f_max)
    };
    Recommended::new((lo, hi), derate.evidence, derate.confidence)
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
    // Clamp `h` to `1.0` before `asin`: for near-antipodal (valid) coordinates
    // rounding can push `h` a hair above 1.0, making `sqrt(h) > 1` and
    // `asin` return NaN. The clamp is standard haversine insurance, cheap for
    // any future non-`qc_track` caller that doesn't already drop NaN-speed legs.
    2.0 * EARTH_RADIUS_M * h.min(1.0).sqrt().asin()
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
/// distance or time across it (I15/B2). An EMPTY `segment_starts`, every legacy
/// (re-anchored) log and every hand-logged run, yields the WHOLE track as a
/// single segment, so every metric routed through this is BIT-IDENTICAL to the
/// pre-segment behaviour. Out-of-range, zero, and duplicate indices are ignored,
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

/// Total track distance in km. Fixes whose reported horizontal accuracy is
/// worse than `max_accuracy_m` are dropped first so GPS noise does not inflate
/// distance. Pure and order-dependent (order = fix order from the shell). The
/// bare form treats the whole track as one segment; see [`track_distance_km_seg`]
/// for the pause-bridge-aware form.
pub fn track_distance_km(points: &[GpsPoint], max_accuracy_m: f32) -> f64 {
    track_distance_km_seg(points, max_accuracy_m, &[])
}

/// [`track_distance_km`] that excludes each pause-bridge leg (a leg entering a
/// [`segments`] boundary) so a paused relocation contributes no distance, the
/// true coordinates stay put (no re-anchoring shift). Empty `segment_starts` is
/// bit-identical to [`track_distance_km`].
pub fn track_distance_km_seg(
    points: &[GpsPoint],
    max_accuracy_m: f32,
    segment_starts: &[u32],
) -> f64 {
    let (usable, is_start) = usable_segments(points, segment_starts, max_accuracy_m);
    let mut total = 0.0;
    for i in 1..usable.len() {
        if is_start[i] {
            continue;
        }
        total += haversine_m(usable[i - 1], usable[i]);
    }
    total / 1000.0
}

/// Elapsed wall time across a track, in minutes, from the first to the last
/// *usable* fix (same accuracy gate as [track_distance_km], so derived pace is
/// consistent). Returns 0.0 for an empty/single-fix or non-monotonic track.
pub fn track_duration_min(points: &[GpsPoint], max_accuracy_m: f32) -> f64 {
    track_duration_min_seg(points, max_accuracy_m, &[])
}

/// [`track_duration_min`] that sums each segment's own first-to-last span, so the
/// pause GAP between segments (a runner stopped, then resumed elsewhere) is not
/// counted as run time. Empty `segment_starts` is bit-identical to
/// [`track_duration_min`].
pub fn track_duration_min_seg(
    points: &[GpsPoint],
    max_accuracy_m: f32,
    segment_starts: &[u32],
) -> f64 {
    segments(points, segment_starts)
        .into_iter()
        .map(|seg| {
            let usable = usable_track(seg, max_accuracy_m);
            match (usable.first(), usable.last()) {
                (Some(f), Some(l)) if l.observed_at > f.observed_at => {
                    (l.observed_at - f.observed_at) as f64 / 60.0
                }
                _ => 0.0,
            }
        })
        .sum()
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
    track_positive_split_pct_seg(points, max_accuracy_m, &[])
}

/// [`track_positive_split_pct`] that treats each pause-bridge leg as zero distance
/// and zero time (exactly as the old re-anchored zero-length bridge did), so the
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
    // Cumulative distance to each fix (cumulative[0] == 0.0); a leg ENTERING a
    // segment start (pause bridge) adds nothing.
    let mut cumulative = Vec::with_capacity(usable.len());
    let mut running_total = 0.0;
    cumulative.push(0.0);
    for i in 1..usable.len() {
        let leg = if is_start[i] {
            0.0
        } else {
            haversine_m(usable[i - 1], usable[i])
        };
        running_total += leg;
        cumulative.push(running_total);
    }
    let total = running_total;
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
    // run's displayed pace, its km-splits, and its VI; wall-clock halves used to
    // disagree with all three on any run with a pause.
    let moving_leg = |i: usize| -> f64 {
        if is_start[i] {
            return 0.0;
        }
        let dt = (usable[i].observed_at - usable[i - 1].observed_at) as f64;
        if dt > 0.0 && !crate::load::is_stopped(haversine_m(usable[i - 1], usable[i]) / dt) {
            dt
        } else {
            0.0
        }
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
/// split is TIMED in MOVING seconds only: a leg whose ground speed is below the
/// auto-pause floor (`load::is_stopped`, <0.5 m/s) or whose timestamps don't
/// advance contributes its distance but NO time, exactly as `moving_duration_min`
/// excludes it from the run's headline pace. A mid-split café/traffic stop
/// therefore cannot make that split read minutes slower than the run's own
/// moving-pace average on the same detail sheet. (Degenerate escape hatch: a
/// split covered entirely below the stop floor has zero moving time and falls
/// back to its wall-clock time so its pace stays finite.) Same accuracy gate as
/// [`track_distance_km`], and the same cumulative-distance walk as
/// [`track_positive_split_pct`]. Pure and order-dependent. Returns an empty vec
/// for a non-positive unit, an empty / single-fix / zero-distance track
/// (nothing to split).
pub fn track_splits(points: &[GpsPoint], max_accuracy_m: f32, unit_m: f64) -> Vec<RunSplit> {
    track_splits_seg(points, max_accuracy_m, unit_m, &[])
}

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
    if unit_m <= 0.0 {
        return Vec::new();
    }
    let (usable, is_start) = usable_segments(points, segment_starts, max_accuracy_m);
    if usable.len() < 2 {
        return Vec::new();
    }
    // Cumulative distance to each fix (cumulative[0] == 0.0), the same walk as
    // `track_positive_split_pct`; a pause-bridge leg adds zero distance, so it
    // behaves downstream exactly like the re-anchored zero-length bridge did.
    let mut cumulative = Vec::with_capacity(usable.len());
    let mut running_total = 0.0;
    cumulative.push(0.0);
    for i in 1..usable.len() {
        let leg = if is_start[i] {
            0.0
        } else {
            haversine_m(usable[i - 1], usable[i])
        };
        running_total += leg;
        cumulative.push(running_total);
    }
    let total = running_total;
    if total <= 0.0 {
        return Vec::new();
    }

    let mut splits = Vec::new();
    let mut index = 1u32;
    let mut boundary = unit_m;
    // Time accumulated into the CURRENT split since the last unit boundary.
    let mut moving_s = 0.0; // moving legs only - the split's pace clock
    let mut wall_s = 0.0; // every leg - fallback for an all-stopped split
    for i in 1..usable.len() {
        let leg_d = cumulative[i] - cumulative[i - 1];
        let dt = ((usable[i].observed_at - usable[i - 1].observed_at) as f64).max(0.0);
        // Same pause gate as `moving_legs` / `moving_duration_min`: legs with
        // non-advancing time or speed below the stop floor are paused time.
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
            wall_s += sub;
            if moving {
                moving_s += sub;
            }
            let dur = if moving_s > 0.0 { moving_s } else { wall_s };
            splits.push(RunSplit {
                index,
                // Full split: distance == unit, so pace IS the (moving) time.
                pace_sec_per_unit: dur,
                cumulative_m: boundary,
                distance_m: unit_m,
                partial: false,
            });
            pos = boundary;
            moving_s = 0.0;
            wall_s = 0.0;
            index += 1;
            boundary = unit_m * index as f64;
        }
        // Remainder of the leg past the last boundary feeds the next split.
        let sub = if leg_d > 0.0 {
            dt * ((cumulative[i] - pos) / leg_d)
        } else {
            dt // zero-distance leg (standstill): all of it is this split's time
        };
        wall_s += sub;
        if moving {
            moving_s += sub;
        }
    }
    // Final partial split: whatever remains after the last full unit.
    let remaining = total - unit_m * (index - 1) as f64;
    if remaining > 1e-6 {
        let dur = if moving_s > 0.0 { moving_s } else { wall_s };
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
/// sides: it no longer misses noisy real intervals (the old 1.15 did) and steady
/// runs stay comfortably clear. An ENGINE HEURISTIC, not a cited threshold (the
/// KB source, Skiba GOVSS, gives no boundary), hence the RUN-INTERVAL-VI-001 chip
/// is graded Weak and its copy measures rather than prescribes.
pub const INTERVAL_VI_THRESHOLD: f64 = 1.10;

/// Graded verdict for whether a run's variability index marks it interval-like
/// (`true`) or steady (`false`). Carries the RUN-INTERVAL-VI-001 evidence so a
/// shell renders it with the same grade chip / why? chrome as any other
/// recommendation, the differentiation is descriptive but still evidence-cited.
pub fn interval_verdict(variability_index: f64) -> Recommended<bool> {
    recommend(
        variability_index >= INTERVAL_VI_THRESHOLD,
        "RUN-INTERVAL-VI-001",
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
fn moving_legs(points: &[GpsPoint], max_accuracy_m: f32, segment_starts: &[u32]) -> Vec<(f64, f64)> {
    // Moving legs are computed WITHIN each segment, so the pause-bridge leg
    // between segments is never formed (a paused relocation is neither moving
    // time nor moving distance). Empty `segment_starts` → one segment → the
    // legs of the whole track, bit-identical to the pre-segment behaviour.
    segments(points, segment_starts)
        .into_iter()
        .flat_map(|seg| {
            usable_track(seg, max_accuracy_m)
                .windows(2)
                .filter_map(|w| {
                    let dt = (w[1].observed_at - w[0].observed_at) as f64;
                    if dt <= 0.0 {
                        return None;
                    }
                    let leg_m = haversine_m(w[0], w[1]);
                    if crate::load::is_stopped(leg_m / dt) {
                        return None;
                    }
                    // Cap distance so speed ≤ the plausible-runner ceiling (jitter).
                    Some((dt, leg_m.min(MAX_PLAUSIBLE_SPEED_MPS * dt)))
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
            }
        }
    }
    if bin_time > 0.0 {
        bins.push((bin_dist / bin_time, bin_time));
    }
    bins
}

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
/// single continuous line drawn through wrong coordinates (I15/B2). Empty
/// `segment_starts` is a single `<trkseg>`, byte-identical to [`export_gpx`].
pub fn export_gpx_seg(points: &[GpsPoint], track_name: &str, segment_starts: &[u32]) -> String {
    let mut s = String::with_capacity(256 + points.len() * 96);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(
        "<gpx version=\"1.1\" creator=\"milestone\" \
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
            s.push_str(&format!("{:.7}", p.lat));
            s.push_str("\" lon=\"");
            s.push_str(&format!("{:.7}", p.lon));
            s.push_str("\"><time>");
            s.push_str(&unix_to_rfc3339_utc(p.observed_at));
            s.push_str("</time></trkpt>\n");
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
    fn haversine_stays_finite_for_near_antipodal_coords() {
        // A demonstrated valid coordinate pair whose rounded haversine `h`
        // exceeds 1.0, so an unclamped `sqrt(h).asin()` returns NaN. The
        // `h.min(1.0)` clamp must keep the distance finite (and ≈ half the
        // Earth's circumference for an antipodal pair).
        let d = haversine_m(pt(-59.13, 0.0, 0), pt(59.1300000000043, 180.0, 0));
        assert!(d.is_finite(), "near-antipodal haversine must be finite, got {d}");
        assert!(d > 0.0, "distance should be positive, got {d}");
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
    fn mid_run_pause_does_not_produce_a_false_positive_split() {
        // Evenly paced ~3.7 m/s throughout, but with a 10-min standstill in the
        // SECOND half (the app's Pause button leaves a ~0-speed bridge leg). With
        // wall-clock halves this read a massive "FADE" (back half ~11× slower);
        // moving-time halves must see it as the even run it is.
        let track = vec![
            pt(0.0, 0.000, 0),
            pt(0.0, 0.001, 30),
            pt(0.0, 0.002, 60),   // ~halfway
            pt(0.0, 0.002, 660),  // 10-min stop, 0 m → excluded
            pt(0.0, 0.003, 690),
            pt(0.0, 0.004, 720),
        ];
        let split = track_positive_split_pct(&track, 30.0).expect("split");
        assert!(split.abs() < 1.0, "paused steady run falsely split: {split}");
    }

    #[test]
    fn positive_split_needs_three_usable_fixes() {
        let track = vec![pt(0.0, 0.0, 0), pt(0.0, 0.001, 10)];
        assert!(track_positive_split_pct(&track, 30.0).is_none());
    }

    /// Even-pace straight track along the equator: constant longitude step +
    /// constant time step ⇒ constant ground speed, so every FULL split must
    /// carry the same per-unit pace regardless of where fixes land relative to
    /// the km/mile boundaries. `legs` × ~111 m each.
    fn even_equator_track(legs: usize, dlon_deg: f64, dt_s: i64) -> Vec<GpsPoint> {
        (0..=legs)
            .map(|i| pt(0.0, i as f64 * dlon_deg, i as i64 * dt_s))
            .collect()
    }

    #[test]
    fn splits_even_pace_all_full_splits_equal_pace() {
        // 50 legs × ~111.19 m ≈ 5.56 km at a constant ~3.71 m/s.
        let track = even_equator_track(50, 0.001, 30);
        let km = track_splits(&track, 30.0, KM_M);
        let full: Vec<_> = km.iter().filter(|s| !s.partial).collect();
        assert_eq!(full.len(), 5, "≈5.56 km → 5 full km splits");
        let p0 = full[0].pace_sec_per_unit;
        for s in &full {
            assert!(
                (s.pace_sec_per_unit - p0).abs() < 0.5,
                "full split {} pace {} vs {p0}",
                s.index,
                s.pace_sec_per_unit
            );
            assert!((s.distance_m - KM_M).abs() < 1e-6);
        }
        // Indices are 1-based and contiguous.
        assert_eq!(
            km.iter().map(|s| s.index).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn splits_mid_run_stop_does_not_inflate_split_pace() {
        // Even-pace equator track (~111.19 m / 30 s legs, ~3.71 m/s) with a
        // 300 s standstill spliced in mid-first-km (after ~556 m): the stopped
        // leg (0 m over 300 s, below the 0.5 m/s auto-pause floor) must be
        // excluded from split 1's TIME, so its pace matches the other full
        // splits (the run's moving pace) instead of reading ~5 min slower -
        // consistent with `moving_duration_min` on the same detail sheet.
        let mut track: Vec<GpsPoint> = (0..=5).map(|i| pt(0.0, i as f64 * 0.001, i * 30)).collect();
        // Standstill: same position as fix 5, 300 s later.
        track.push(pt(0.0, 0.005, 5 * 30 + 300));
        track.extend((6..=50).map(|i| pt(0.0, i as f64 * 0.001, i * 30 + 300)));

        let km = track_splits(&track, 30.0, KM_M);
        let full: Vec<_> = km.iter().filter(|s| !s.partial).collect();
        assert_eq!(full.len(), 5, "≈5.56 km → 5 full km splits");
        // Moving average: 1000 m at ~3.7065 m/s ≈ 269.8 s/km.
        let moving_avg = KM_M / (0.001f64.to_radians() * super::EARTH_RADIUS_M / 30.0);
        for s in &full {
            assert!(
                (s.pace_sec_per_unit - moving_avg).abs() < 2.0,
                "split {} pace {} should sit at the moving average {moving_avg}, \
                 not be inflated by the stop",
                s.index,
                s.pace_sec_per_unit
            );
        }
        // Explicitly: split 1 (which contains the stop) equals split 2.
        assert!(
            (full[0].pace_sec_per_unit - full[1].pace_sec_per_unit).abs() < 0.5,
            "stop split {} vs clean split {}",
            full[0].pace_sec_per_unit,
            full[1].pace_sec_per_unit
        );
    }

    #[test]
    fn splits_all_stopped_split_falls_back_to_wall_clock() {
        // A track covered entirely below the 0.5 m/s stop floor (~111.19 m legs
        // over 300 s each, ~0.37 m/s) has zero moving time; the partial split
        // must fall back to wall-clock so its pace stays finite and non-zero.
        let track: Vec<GpsPoint> = (0..=4).map(|i| pt(0.0, i as f64 * 0.001, i * 300)).collect();
        let km = track_splits(&track, 30.0, KM_M);
        assert_eq!(km.len(), 1);
        assert!(km[0].partial);
        assert!(
            km[0].pace_sec_per_unit > 0.0 && km[0].pace_sec_per_unit.is_finite(),
            "got {}",
            km[0].pace_sec_per_unit
        );
    }

    #[test]
    fn splits_final_partial_is_flagged() {
        // ≈5.56 km → the 6th km split is partial (~0.56 km).
        let track = even_equator_track(50, 0.001, 30);
        let km = track_splits(&track, 30.0, KM_M);
        let last = km.last().expect("a split");
        assert!(last.partial, "final split flagged partial");
        assert!(last.distance_m < KM_M, "partial covers < 1 km");
        assert!(
            km[..km.len() - 1].iter().all(|s| !s.partial),
            "only the last split is partial"
        );
    }

    #[test]
    fn splits_km_and_mi_counts_differ_for_same_track() {
        // Same ≈5.56 km track: 5 full + 1 partial km (6) vs 3 full + 1 partial mi
        // (4), since 5.56 km ≈ 3.45 mi.
        let track = even_equator_track(50, 0.001, 30);
        let km = track_splits(&track, 30.0, KM_M);
        let mi = track_splits(&track, 30.0, MILE_M);
        assert_eq!(km.len(), 6, "km splits");
        assert_eq!(mi.len(), 4, "mile splits");
        assert_eq!(mi.iter().filter(|s| !s.partial).count(), 3, "3 full miles");
        assert!(mi.last().unwrap().partial);
    }

    #[test]
    fn splits_under_one_unit_is_single_partial() {
        // ~445 m: under a km → one partial split, index 1.
        let track = even_equator_track(4, 0.001, 30);
        let km = track_splits(&track, 30.0, KM_M);
        assert_eq!(km.len(), 1);
        assert_eq!(km[0].index, 1);
        assert!(km[0].partial);
        assert!(km[0].distance_m < KM_M);
    }

    #[test]
    fn splits_degenerate_tracks_are_empty() {
        assert!(track_splits(&[], 30.0, KM_M).is_empty(), "empty track");
        assert!(
            track_splits(&[pt(0.0, 0.0, 0)], 30.0, KM_M).is_empty(),
            "single fix"
        );
        // Zero-distance track (all fixes stacked) → nothing to split.
        let stacked = vec![pt(0.0, 0.0, 0), pt(0.0, 0.0, 10), pt(0.0, 0.0, 20)];
        assert!(track_splits(&stacked, 30.0, KM_M).is_empty(), "zero distance");
        // Non-positive unit.
        assert!(track_splits(&even_equator_track(50, 0.001, 30), 30.0, 0.0).is_empty());
    }

    #[test]
    fn variability_index_separates_interval_from_steady_same_average() {
        // Two runs with the SAME total distance (0.004° lon ≈ 445 m) and the SAME
        // duration (120 s) → identical average pace. The current engine rates them
        // the same; the variability index must not.
        // Steady: four equal ~111 m / 30 s segments (~3.71 m/s throughout).
        let steady = vec![
            pt(0.0, 0.000, 0),
            pt(0.0, 0.001, 30),
            pt(0.0, 0.002, 60),
            pt(0.0, 0.003, 90),
            pt(0.0, 0.004, 120),
        ];
        // Interval with JOG recovery (recovery legs stay ABOVE the 0.5 m/s
        // auto-pause floor, so they count as moving time, matching the app's
        // moving-pace base; and the same 445 m / 120 s average holds): two
        // ~200 m / 30 s hard reps (~6.7 m/s) each followed by a ~22 m / 30 s jog
        // (~0.74 m/s). Standing (0 m/s) recovery would be excluded as a pause.
        let interval = vec![
            pt(0.0, 0.0000, 0),
            pt(0.0, 0.0018, 30),
            pt(0.0, 0.0020, 60),
            pt(0.0, 0.0038, 90),
            pt(0.0, 0.0040, 120),
        ];

        // Same average by construction: sanity-check that first.
        let ds = track_distance_km(&steady, 30.0);
        let di = track_distance_km(&interval, 30.0);
        assert!((ds - di).abs() < 0.001, "distances differ: {ds} vs {di}");
        assert_eq!(
            track_duration_min(&steady, 30.0),
            track_duration_min(&interval, 30.0),
        );

        let vi_steady = track_variability_index(&steady, 30.0).expect("steady VI");
        let vi_interval = track_variability_index(&interval, 30.0).expect("interval VI");

        // Steady ≈ 1.0; interval well above it (jog-recovery 4th-power mean ~1.5).
        assert!((vi_steady - 1.0).abs() < 0.03, "steady VI {vi_steady}");
        assert!(vi_interval > 1.4, "interval VI {vi_interval}");
        assert!(
            vi_interval > vi_steady + 0.3,
            "interval {vi_interval} not clearly above steady {vi_steady}",
        );

        // Normalized speed exceeds the plain average for the interval run.
        let ns = normalized_speed_mps(&interval, 30.0).expect("interval NS");
        let avg = di * 1000.0 / (track_duration_min(&interval, 30.0) * 60.0);
        assert!(ns > avg, "NS {ns} should exceed avg {avg}");
    }

    #[test]
    fn variability_index_needs_two_usable_fixes() {
        assert!(normalized_speed_mps(&[pt(0.0, 0.0, 0)], 30.0).is_none());
        assert!(track_variability_index(&[pt(0.0, 0.0, 0)], 30.0).is_none());
    }

    /// Build a DENSE (1 Hz) GPS track from a speed profile `(speed_mps,
    /// duration_s)`, with deterministic lateral position jitter to mimic a real
    /// consumer GPS at ~a few metres accuracy. Forward motion runs along lon at
    /// the equator (1° ≈ 111 320 m); the lateral wobble is `jitter_deg·sin(i·1.3)`
    /// on lat, which makes consecutive per-second leg lengths swing (exactly the
    /// noise that, un-smoothed, blows up the 4th-power mean).
    fn noisy_track(profile: &[(f64, i64)], jitter_deg: f64) -> Vec<GpsPoint> {
        let mut pts = Vec::new();
        let mut lon = 0.0f64;
        let mut t = 0i64;
        let deg_per_m = 1.0 / 111_320.0;
        // Seed the first fix.
        pts.push(GpsPoint { lat: 0.0, lon: 0.0, observed_at: 0, accuracy_m: 5.0 });
        for &(speed, dur) in profile {
            for _ in 0..dur {
                lon += speed * deg_per_m; // advance ~speed metres this second
                t += 1;
                let lat = jitter_deg * (t as f64 * 1.3).sin();
                pts.push(GpsPoint { lat, lon, observed_at: t, accuracy_m: 5.0 });
            }
        }
        pts
    }

    #[test]
    fn steady_noisy_1hz_run_is_not_flagged_interval() {
        // A genuinely STEADY 5-min run at ~3 m/s, sampled 1 Hz with ~3 m lateral
        // GPS jitter. Per-second leg speeds swing wildly; without the rolling
        // window the 4th-power mean reads this as an "interval" (the reported
        // bug). With the window, jitter averages out → VI must stay steady.
        let track = noisy_track(&[(3.0, 300)], 0.000_03);
        let vi = track_variability_index(&track, 30.0).expect("vi");
        assert!(
            vi < INTERVAL_VI_THRESHOLD,
            "steady noisy run wrongly flagged interval: VI {vi}"
        );
    }

    #[test]
    fn short_noisy_1hz_run_is_not_flagged_interval() {
        // The exact failure the user reported: a short ~1 km steady run (≈5.5 min
        // at 3 m/s) sampled 1 Hz with jitter must NOT read as interval.
        let track = noisy_track(&[(3.0, 330)], 0.000_03);
        let vi = track_variability_index(&track, 30.0).expect("vi");
        assert!(vi < INTERVAL_VI_THRESHOLD, "short steady run wrongly flagged: VI {vi}");
    }

    #[test]
    fn interval_noisy_1hz_run_is_flagged() {
        // A real 1 Hz interval session at realistic (~1 m) GPS jitter: 4× (90 s
        // hard @ ~5 m/s, then 90 s jog @ ~2 m/s). The rep structure spans many
        // 30 s windows, so it survives smoothing and flags interval. (Under
        // pathologically heavy high-frequency noise VI can still fall under the
        // cutoff: the metric is Weak-graded; this guards the realistic case.)
        let mut profile = Vec::new();
        for _ in 0..4 {
            profile.push((5.0, 90));
            profile.push((2.0, 90));
        }
        let track = noisy_track(&profile, 0.000_01);
        let vi = track_variability_index(&track, 30.0).expect("vi");
        assert!(
            vi >= INTERVAL_VI_THRESHOLD,
            "interval run not flagged: VI {vi}"
        );
    }

    /// Save-time GPS decimation (shell `RunSession.decimatedTrackForCore`): keep
    /// every `stride`-th fix plus the endpoints. These tests prove the core's
    /// of-record figures survive that lossy thinning at strides 2 and 3: the
    /// shell may hand the core fewer points, and the verdicts must not move.
    fn decimate(points: &[GpsPoint], stride: usize) -> Vec<GpsPoint> {
        assert!(stride >= 1);
        if points.len() < 3 {
            return points.to_vec();
        }
        let mut out: Vec<GpsPoint> = points.iter().step_by(stride).copied().collect();
        let last = *points.last().unwrap();
        if out.last() != Some(&last) {
            out.push(last);
        }
        out
    }

    #[test]
    fn decimation_preserves_the_steady_vi_verdict() {
        // A genuinely steady noisy run reads steady (VI < threshold); thinning it
        // ×2 and ×3 must keep that verdict, fewer points, same call.
        let full = noisy_track(&[(3.0, 300)], 0.000_03);
        for stride in [2usize, 3] {
            let thinned = decimate(&full, stride);
            let vi = track_variability_index(&thinned, 30.0).expect("vi");
            assert!(
                vi < INTERVAL_VI_THRESHOLD,
                "steady run flipped to interval after ×{stride} decimation: VI {vi}"
            );
        }
    }

    #[test]
    fn decimation_preserves_the_interval_vi_verdict() {
        // A real interval session (4× 90 s hard / 90 s jog) flags interval; the
        // 30 s rolling window still gets ~10–15 samples/bin at a 2–3 s cadence,
        // so the verdict survives ×2 and ×3 thinning.
        let mut profile = Vec::new();
        for _ in 0..4 {
            profile.push((5.0, 90));
            profile.push((2.0, 90));
        }
        let full = noisy_track(&profile, 0.000_01);
        for stride in [2usize, 3] {
            let thinned = decimate(&full, stride);
            let vi = track_variability_index(&thinned, 30.0).expect("vi");
            assert!(
                vi >= INTERVAL_VI_THRESHOLD,
                "interval run lost its verdict after ×{stride} decimation: VI {vi}"
            );
        }
    }

    #[test]
    fn decimation_preserves_distance_within_one_percent() {
        // Total distance is the storage-heaviest figure users see; thinning must
        // not move it more than ±1 % at strides 2 and 3. Uses realistic sub-metre
        // GPS jitter (0.000_004 ≈ 0.44 m), a real good-fix running track barely
        // wanders between 1 s samples, so thinning to a 2–3 s cadence keeps the
        // path length. (The heavier 0.000_03/0.000_01 wobble the VI fixtures use
        // is a deliberate per-second speed-swing stress case, whose fixed-freq
        // sine ALIASES under decimation, an artifact of the generator, not a
        // real track shape, so it is not the right fixture for distance parity.)
        let mut profile = Vec::new();
        for _ in 0..4 {
            profile.push((5.0, 90));
            profile.push((2.0, 90));
        }
        for track in [noisy_track(&[(3.0, 600)], 0.000_004), noisy_track(&profile, 0.000_004)] {
            let full_km = track_distance_km(&track, 30.0);
            for stride in [2usize, 3] {
                let thinned_km = track_distance_km(&decimate(&track, stride), 30.0);
                let err = (thinned_km - full_km).abs() / full_km;
                assert!(
                    err <= 0.01,
                    "×{stride} decimation moved distance {err:.4} (>1%): {full_km} → {thinned_km}"
                );
            }
        }
    }

    #[test]
    fn decimation_preserves_the_positive_split_sign() {
        // A run that clearly slows in the back half is a POSITIVE split; one that
        // speeds up is NEGATIVE. Decimation must not flip that sign: the split
        // verdict a runner reads stays the same.
        let positive = noisy_track(&[(4.0, 150), (2.5, 150)], 0.000_01);
        let negative = noisy_track(&[(2.5, 150), (4.0, 150)], 0.000_01);
        for stride in [2usize, 3] {
            let ps_pos = track_positive_split_pct(&decimate(&positive, stride), 30.0).expect("pos");
            let ps_neg = track_positive_split_pct(&decimate(&negative, stride), 30.0).expect("neg");
            assert!(
                ps_pos > 0.0,
                "positive-split run lost its sign after ×{stride}: {ps_pos}"
            );
            assert!(
                ps_neg < 0.0,
                "negative-split run lost its sign after ×{stride}: {ps_neg}"
            );
        }
    }

    #[test]
    fn steady_run_with_a_mid_run_pause_stays_steady() {
        // A steady ~3.7 m/s run with one long standing pause in the middle: the
        // paused leg (a ~0 m/s bridge spanning the stop, exactly what the app's
        // Pause button leaves) must be EXCLUDED, so the run reads steady, not a
        // false "interval" from the depressed elapsed-time average.
        let steady_paused = vec![
            pt(0.0, 0.000, 0),
            pt(0.0, 0.001, 30),   // ~3.7 m/s
            pt(0.0, 0.002, 60),   // ~3.7 m/s
            pt(0.0, 0.002, 660),  // 10-min stop, ~0 m/s → excluded
            pt(0.0, 0.003, 690),  // ~3.7 m/s
            pt(0.0, 0.004, 720),  // ~3.7 m/s
        ];
        let vi = track_variability_index(&steady_paused, 30.0).expect("vi");
        assert!(vi < INTERVAL_VI_THRESHOLD, "paused steady wrongly flagged: VI {vi}");
        assert!((vi - 1.0).abs() < 0.05, "VI {vi} should be ~1.0");
    }

    #[test]
    fn normalized_speed_clamps_gps_jitter_spike() {
        // A single teleport fix (huge distance in 1 s) must be clamped, not allowed
        // to blow up the 4th-power mean. Two clean 30 s segments plus one spike.
        let spiky = vec![
            pt(0.0, 0.000, 0),
            pt(0.0, 0.001, 30),
            pt(5.0, 5.000, 31), // ~780 km in 1 s → clamp to MAX_PLAUSIBLE_SPEED_MPS
            pt(5.0, 5.001, 61),
        ];
        let ns = normalized_speed_mps(&spiky, 30.0).expect("ns");
        assert!(ns <= MAX_PLAUSIBLE_SPEED_MPS + 1e-9, "NS {ns} not clamped");
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
        // marathon_derated_band (running-040/008, option B): the optimistic
        // band is shifted SLOWER, the fast bound by the min derate and the slow
        // bound by the max, so the range moves later and widens, and the
        // result carries the same RUN-VDOT-001 evidence as the derate itself.
        let vdot = crate::load::vdot(10_000.0, 2_520.0); // a real 42:00 10K
        let base_low = 12_000.0;
        let base_high = 12_600.0;
        let band = marathon_derated_band(base_low, base_high, vdot);
        assert!(
            band.value.0 > base_low && band.value.1 > base_high,
            "both bounds slow down: {:?} vs ({base_low}, {base_high})",
            band.value
        );
        assert!(
            band.value.1 - band.value.0 > base_high - base_low,
            "the derate widens the span by its own uncertainty: {:?}",
            band.value
        );
        assert_eq!(
            band.evidence.citation.claim_id.as_deref(),
            Some("RUN-VDOT-001"),
            "the derated number still cites running-040/008 (HARD RULE 2)"
        );
        // A degenerate prediction (no valid race → VDOT 0) is left untouched,
        // never fabricated slower.
        let degen = marathon_derated_band(0.0, 0.0, 0.0);
        assert_eq!(degen.value, (0.0, 0.0));

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

    #[test]
    fn equivalency_cites_the_equivalency_claim_not_vdot() {
        // running-039 is the Riegel/Daniels equivalency combiner (RUN-EQUIV-001),
        // NOT the VDOT-fitness estimate (RUN-VDOT-001). Guards the citation fix.
        let r = race_equivalency(3600.0, 3636.0);
        assert_eq!(
            r.evidence.citation.claim_id.as_deref(),
            Some("RUN-EQUIV-001"),
        );
        assert_eq!(r.evidence.grade, crate::schema::EvidenceGrade::Moderate);
    }

    // ── I15/B2: segment-aware track fns (pause + relocation) ──────────────────

    /// A pause + relocation: segment 1 is four ~111 m legs at the equator, then
    /// the runner relocates ~111 km east and runs segment 2 (four more ~111 m
    /// legs). Index 5 begins segment 2, so the bridge leg (seg1 end → seg2 start)
    /// is a pause bridge. Segment 2 stays at the equator, so re-anchoring it back
    /// onto segment 1 is a pure-longitude translation that preserves haversine
    /// EXACTLY, the parity oracle for the VI/split verdicts.
    fn relocation_fixture() -> Vec<GpsPoint> {
        vec![
            pt(0.0, 0.000, 0),
            pt(0.0, 0.001, 10),
            pt(0.0, 0.002, 20),
            pt(0.0, 0.003, 30),
            pt(0.0, 0.004, 40),
            // 60 s pause + ~111 km relocation east.
            pt(0.0, 1.000, 100),
            pt(0.0, 1.001, 110),
            pt(0.0, 1.002, 120),
            pt(0.0, 1.003, 130),
            pt(0.0, 1.004, 140),
        ]
    }

    /// The equivalent track the OLD shell stored: segment 2 re-anchored onto
    /// segment 1's end (pure-longitude shift at the equator), collapsing the
    /// bridge to zero length. Empty `segment_starts` reproduces the legacy metric.
    fn reanchored_equivalent() -> Vec<GpsPoint> {
        let full = relocation_fixture();
        let off_lon = full[4].lon - full[5].lon; // 0.004 - 1.000
        let mut out = full[..5].to_vec();
        for p in &full[5..] {
            out.push(pt(p.lat, p.lon + off_lon, p.observed_at));
        }
        out
    }

    #[test]
    fn empty_starts_is_bit_identical_to_the_single_track_metrics() {
        // Every track fn's segment-aware form must reduce EXACTLY to the legacy
        // whole-track form when there is no interior boundary, the regression net
        // for legacy re-anchored logs and hand-logged runs. A start of 0 (index 0
        // never has an entering leg) and an out-of-range index are both no-ops.
        let t = reanchored_equivalent(); // one continuous run with slight structure
        for starts in [&[][..], &[0][..], &[999][..]] {
            assert_eq!(
                track_distance_km(&t, 30.0),
                track_distance_km_seg(&t, 30.0, starts),
                "distance parity, starts={starts:?}",
            );
            assert_eq!(
                track_duration_min(&t, 30.0),
                track_duration_min_seg(&t, 30.0, starts),
                "duration parity, starts={starts:?}",
            );
            assert_eq!(
                track_positive_split_pct(&t, 30.0),
                track_positive_split_pct_seg(&t, 30.0, starts),
                "split parity, starts={starts:?}",
            );
            assert_eq!(
                track_splits(&t, 30.0, KM_M),
                track_splits_seg(&t, 30.0, KM_M, starts),
                "km-splits parity, starts={starts:?}",
            );
            assert_eq!(
                normalized_speed_mps(&t, 30.0),
                normalized_speed_mps_seg(&t, 30.0, starts),
                "NS parity, starts={starts:?}",
            );
            assert_eq!(
                track_variability_index(&t, 30.0),
                track_variability_index_seg(&t, 30.0, starts),
                "VI parity, starts={starts:?}",
            );
            assert_eq!(
                export_gpx(&t, "run"),
                export_gpx_seg(&t, "run", starts),
                "gpx parity, starts={starts:?}",
            );
        }
    }

    #[test]
    fn segmented_distance_excludes_the_pause_bridge() {
        let t = relocation_fixture();
        // Without boundaries the ~111 km bridge dominates the sum.
        let naive = track_distance_km(&t, 30.0);
        assert!(naive > 100.0, "bridge should dominate the naive sum: {naive}");
        // With the boundary at index 5 the bridge contributes nothing → only the
        // two ~445 m segments remain (~0.89 km).
        let seg = track_distance_km_seg(&t, 30.0, &[5]);
        assert!((seg - 0.889).abs() < 0.05, "segmented distance {seg}");
        // And it equals the distance of the re-anchored equivalent the old shell
        // stored (bit-identical route length, no shift needed).
        assert!(
            (seg - track_distance_km(&reanchored_equivalent(), 30.0)).abs() < 1e-9,
            "segmented == re-anchored distance",
        );
        // The pause gap (60 s) is not run time: two 40 s segments → 80 s = 4/3 min.
        let dur = track_duration_min_seg(&t, 30.0, &[5]);
        assert!((dur - 80.0 / 60.0).abs() < 1e-9, "segmented duration {dur}");
    }

    #[test]
    fn segmented_vi_and_split_match_the_reanchored_equivalent() {
        let t = relocation_fixture();
        let re = reanchored_equivalent();
        // The re-anchored track is the legacy oracle: the segment-aware metrics on
        // the TRUE coords must equal it (equatorial relocation ⇒ exact).
        assert_eq!(
            track_variability_index_seg(&t, 30.0, &[5]),
            track_variability_index(&re, 30.0),
            "VI parity vs re-anchored",
        );
        assert_eq!(
            track_positive_split_pct_seg(&t, 30.0, &[5]),
            track_positive_split_pct(&re, 30.0),
            "split parity vs re-anchored",
        );
        // Raw splits agree to f64 rounding (the equatorial re-anchor translation is
        // exact only up to the last ULP, so compare per field with a tolerance).
        let seg_splits = track_splits_seg(&t, 30.0, KM_M, &[5]);
        let re_splits = track_splits(&re, 30.0, KM_M);
        assert_eq!(seg_splits.len(), re_splits.len(), "same split count");
        for (a, b) in seg_splits.iter().zip(&re_splits) {
            assert_eq!(a.index, b.index);
            assert_eq!(a.partial, b.partial);
            assert!((a.pace_sec_per_unit - b.pace_sec_per_unit).abs() < 1e-6);
            assert!((a.cumulative_m - b.cumulative_m).abs() < 1e-6);
            assert!((a.distance_m - b.distance_m).abs() < 1e-6);
        }
    }

    #[test]
    fn gpx_emits_one_trkseg_per_segment_with_true_coords() {
        let t = relocation_fixture();
        let gpx = export_gpx_seg(&t, "paused run", &[5]);
        // Two recording segments → two <trkseg> blocks with a gap between.
        assert_eq!(gpx.matches("<trkseg>").count(), 2, "two segments\n{gpx}");
        assert_eq!(gpx.matches("</trkseg>").count(), 2);
        // TRUE coordinates: segment 2's real relocated longitude (1.000…) is
        // present, NOT re-anchored back onto segment 1 (0.004…).
        assert!(gpx.contains("lon=\"1.0000000\""), "true seg-2 coord\n{gpx}");
        // Every real fix is still emitted (10 points across the two segments).
        assert_eq!(gpx.matches("<trkpt").count(), 10);
        // Empty starts → a single <trkseg>, byte-identical to the bare export.
        let flat = export_gpx_seg(&t, "paused run", &[]);
        assert_eq!(flat.matches("<trkseg>").count(), 1);
        assert_eq!(flat, export_gpx(&t, "paused run"));
    }
}

