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
// 7. Intensity distribution (prescriptive): File 04 §"distribution" table
// ---------------------------------------------------------------------------

/// Which easy/moderate/hard split model to run (File 04 running-014/016).
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

/// Target easy/moderate/hard split for a distribution model. Rule running-014/016.
///
/// Both models keep ~80% easy; they trade the moderate↔hard balance. 80/20 is a
/// POPULATION optimum (Seiler), individualize ±5–10% via TT re-tests.
pub fn intensity_distribution(model: DistributionModel) -> Recommended<IntensityDistribution> {
    let d = match model {
        DistributionModel::Pyramidal => IntensityDistribution { easy_pct: 80, moderate_pct: 15, hard_pct: 5 },
        DistributionModel::Polarized => IntensityDistribution { easy_pct: 80, moderate_pct: 5, hard_pct: 15 },
    };
    recommend(d, "RUN-DIST-001")
}

/// Distribution model for a mesocycle phase: pyramidal early, polarized near
/// race (File 04 running-016/052; engine default for contested CQ-01).
pub fn distribution_for_phase(phase: MesoPhase) -> Recommended<IntensityDistribution> {
    let model = match phase {
        MesoPhase::Base | MesoPhase::Build | MesoPhase::Deload => DistributionModel::Pyramidal,
        MesoPhase::Peak | MesoPhase::Taper => DistributionModel::Polarized,
    };
    intensity_distribution(model)
}

// ---------------------------------------------------------------------------
// 8. Quality-session spacing (prescriptive validator): File 04 running-027
// ---------------------------------------------------------------------------

/// Quality-session governance limits (File 04 running-027 / §"Volume caps").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityLimits {
    /// Max hard/quality sessions per week.
    pub max_per_week: u8,
    /// Minimum spacing between quality sessions, hours.
    pub min_spacing_hours: u8,
    /// Whether two Z3 sessions on consecutive days are allowed (never, non-elite).
    pub allow_consecutive_z3: bool,
}

/// Canonical quality limits: ≤3/week, ≥48 h apart, no back-to-back Z3. Rule running-027.
pub fn quality_limits() -> Recommended<QualityLimits> {
    recommend(
        QualityLimits { max_per_week: 3, min_spacing_hours: 48, allow_consecutive_z3: false },
        "RUN-DIST-001",
    )
}

/// True when a week's quality plan respects the caps: ≤3 sessions, ≥48 h gaps,
/// and no consecutive-Z3 stacking (File 04 running-027).
pub fn quality_plan_ok(sessions_per_week: u8, min_gap_hours: u8, has_consecutive_z3: bool) -> Recommended<bool> {
    let limits = quality_limits().value;
    let ok = sessions_per_week <= limits.max_per_week
        && min_gap_hours >= limits.min_spacing_hours
        && !(has_consecutive_z3 && !limits.allow_consecutive_z3);
    recommend(ok, "RUN-DIST-001")
}

// ---------------------------------------------------------------------------
// 9. Weekly mileage progression (prescriptive): File 04 running-043/044
// ---------------------------------------------------------------------------

/// Safe single-week volume-increase cap as a fraction, by training age
/// (File 04 running-043). Novice (<1 yr) tolerates up to +10 %; experienced
/// runners are held to ~+5 %. NOT the discredited hard "10 % rule", a ceiling.
pub fn weekly_increase_cap_frac(training_age_years: f64) -> Recommended<f64> {
    let cap = if training_age_years < 1.0 { 0.10 } else { 0.05 };
    recommend(cap, "RUN-PROGRESS-001")
}

/// True when next week's volume stays within the training-age increase cap
/// (File 04 running-043). A non-positive current volume cannot be ratioed → false.
pub fn weekly_increase_ok(current_km: f64, next_km: f64, training_age_years: f64) -> Recommended<bool> {
    let cap = weekly_increase_cap_frac(training_age_years).value;
    let ok = if current_km <= 0.0 {
        false
    } else {
        (next_km - current_km) / current_km <= cap + 1e-9
    };
    recommend(ok, "RUN-PROGRESS-001")
}

/// Flag elevated injury risk when weekly distance rises >30 % across two weeks
/// (File 04 running-030; Nielsen 2014 ~1.6× risk). `true` = flag. A non-positive
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
// 10. Deload cadence (prescriptive): File 04 running-045
// ---------------------------------------------------------------------------

/// Load:recovery cycle prescription (File 04 running-045; RUN-DELOAD-001).
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
/// and intensity 20–40 % (File 04 running-045).
pub fn deload_cadence(conservative: bool) -> Recommended<DeloadCadence> {
    let load_weeks = if conservative { 2 } else { 3 };
    recommend(
        DeloadCadence { load_weeks, recovery_weeks: 1, reduction_frac: (0.20, 0.40) },
        "RUN-DELOAD-001",
    )
}

// ---------------------------------------------------------------------------
// Workout-type prescription table (running-014 … running-022)
// ---------------------------------------------------------------------------

/// Prescription for a single run session type. Rules running-014 … running-022.
///
/// `pct_hr_max` is the target heart-rate band as a fraction of HRmax. When
/// `hr_governed` is true the session is paced by HR (aerobic Recovery/Long/
/// Tempo/RacePace); when false the effort is too short or too intense for HR to
/// settle, so it is governed by pace/effort and `pct_hr_max` is only a coarse
/// ceiling (Interval/Strides/Hills).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunWorkoutRx {
    pub pct_hr_max: (f64, f64),
    pub rpe: (u8, u8),
    pub duration_min: (u16, u16),
    pub hr_governed: bool,
}

/// Look up the prescription band for a run session type. Rules running-014…022.
pub fn run_workout_rx(kind: RunSessionType) -> Recommended<RunWorkoutRx> {
    use RunSessionType::*;
    let rx = match kind {
        // running-014 Recovery
        Recovery => RunWorkoutRx {
            pct_hr_max: (0.65, 0.76),
            rpe: (2, 3),
            duration_min: (20, 40),
            hr_governed: true,
        },
        // running-016 Long Run
        LongRun => RunWorkoutRx {
            pct_hr_max: (0.65, 0.80),
            rpe: (3, 5),
            duration_min: (60, 150),
            hr_governed: true,
        },
        // running-018 Tempo / Threshold
        Tempo => RunWorkoutRx {
            pct_hr_max: (0.88, 0.92),
            rpe: (6, 7),
            duration_min: (20, 40),
            hr_governed: true,
        },
        // running-017 Marathon-pace segments
        RacePace => RunWorkoutRx {
            pct_hr_max: (0.80, 0.85),
            rpe: (5, 6),
            duration_min: (30, 120),
            hr_governed: true,
        },
        // running-019 VO2max intervals (pace-governed, HR lags)
        Interval => RunWorkoutRx {
            pct_hr_max: (0.95, 1.00),
            rpe: (8, 9),
            duration_min: (3, 5),
            hr_governed: false,
        },
        // R-pace reps: >100 %VO2max speed/economy work, effort-governed.
        Repetition => RunWorkoutRx {
            pct_hr_max: (0.0, 0.0),
            rpe: (8, 9),
            duration_min: (0, 2),
            hr_governed: false,
        },
        // running-020 Strides (neuromuscular, HR irrelevant)
        Strides => RunWorkoutRx {
            pct_hr_max: (0.0, 0.0),
            rpe: (6, 7),
            duration_min: (0, 1),
            hr_governed: false,
        },
        // running-021 / running-022 Hill sprints & long hill reps
        Hills => RunWorkoutRx {
            pct_hr_max: (0.90, 1.00),
            rpe: (7, 9),
            duration_min: (0, 4),
            hr_governed: false,
        },
    };
    recommend(rx, "RUN-WORKOUT-001")
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

/// Prefer Karvonen (%HRR) over %HRmax when resting HR is low, where the two
/// methods diverge substantially (running-005): true below RHR 55; at RHR ≥70
/// they converge and either is acceptable. RUN-HRMAX-001.
pub fn prefer_karvonen(resting_hr_bpm: f64) -> Recommended<bool> {
    recommend(resting_hr_bpm < 55.0, "RUN-HRMAX-001")
}

/// Whether HR training zones are due for recalculation (running-006): recompute
/// every 4–6 weeks off a measured max HR. Due once ≥4 weeks have elapsed.
/// Safety-relevant, stale zones misplace every prescription. RUN-HRMAX-001.
pub fn hr_zone_recalc_due(weeks_since_recalc: u8) -> Recommended<bool> {
    recommend(weeks_since_recalc >= 4, "RUN-HRMAX-001")
}

// ---------------------------------------------------------------------------
// VDOT derate & goal-distance session plan (running-008/024/025)
// ---------------------------------------------------------------------------

/// A training goal by target race distance (running-024 goal table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        GoalDistance::General => GoalWeekPlan { sessions_per_week: (3, 5), quality_per_week: (0, 1), long_run_share: lr },
        GoalDistance::C25k => GoalWeekPlan { sessions_per_week: (3, 3), quality_per_week: (0, 0), long_run_share: lr },
        GoalDistance::FiveK | GoalDistance::TenK | GoalDistance::HalfMarathon => {
            GoalWeekPlan { sessions_per_week: (4, 6), quality_per_week: (2, 2), long_run_share: lr }
        }
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
/// 10–12); repeat any too-hard week without penalty. RUN-WORKOUT-001.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct C25kPlan {
    pub runs_per_week: u8,
    /// Program length in weeks (nominal, extended).
    pub weeks: (u8, u8),
    pub rest_day_between: bool,
    /// A too-hard week may be repeated without penalty.
    pub repeat_hard_week_allowed: bool,
}

/// The Couch-to-5K default plan (running-025). RUN-WORKOUT-001.
pub fn c25k_plan() -> Recommended<C25kPlan> {
    recommend(
        C25kPlan { runs_per_week: 3, weeks: (9, 12), rest_day_between: true, repeat_hard_week_allowed: true },
        "RUN-WORKOUT-001",
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
/// toward measured LT1 when data exist. Weak/contested (CQ-03). RUN-MAF-001.
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
    let hold = if training_age_years < 1.0 { Some((2, 3)) } else { None };
    recommend(hold, "RUN-PROGRESS-001")
}

/// Insert an unscheduled down week when ≥2 overtraining signals fire
/// (running-034): elevated RHR >5–7 bpm ≥3 days, HRV trending down, RPE rising,
/// disrupted sleep/soreness/mood, or standard-workout performance down >3–5%.
/// Safety-relevant recovery guard. RUN-DELOAD-001.
pub fn unscheduled_deload(overtraining_signal_count: u8) -> Recommended<bool> {
    recommend(overtraining_signal_count >= 2, "RUN-DELOAD-001")
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
    fn hr_method_and_recalc() {
        assert!(prefer_karvonen(50.0).value);
        assert!(!prefer_karvonen(55.0).value);
        assert!(!prefer_karvonen(72.0).value);
        assert!(!hr_zone_recalc_due(3).value);
        assert!(hr_zone_recalc_due(4).value);
    }

    #[test]
    fn vdot_derate_and_goal_plan() {
        assert_eq!(vdot_derate_points(GoalDistance::HalfMarathon, true).value, (1.0, 1.5));
        assert_eq!(vdot_derate_points(GoalDistance::Marathon, true).value, (2.0, 3.0));
        assert_eq!(vdot_derate_points(GoalDistance::FiveK, true).value, (0.0, 0.0));
        assert_eq!(vdot_derate_points(GoalDistance::Marathon, false).value, (0.0, 0.0));
        let c = goal_week_plan(GoalDistance::C25k, false).value;
        assert_eq!((c.sessions_per_week, c.quality_per_week), ((3, 3), (0, 0)));
        assert_eq!(goal_week_plan(GoalDistance::FiveK, false).value.quality_per_week, (2, 2));
        assert_eq!(goal_week_plan(GoalDistance::Marathon, true).value.sessions_per_week, (5, 7));
        assert_eq!(goal_week_plan(GoalDistance::Marathon, false).value.sessions_per_week, (4, 6));
        let p = c25k_plan().value;
        assert_eq!((p.runs_per_week, p.rest_day_between, p.repeat_hard_week_allowed), (3, true, true));
    }

    #[test]
    fn distribution_and_progression_guards() {
        assert!(easy_share_floor_ok(0.80).value);
        assert!(!easy_share_floor_ok(0.79).value);
        assert_eq!(default_counting_method(true).value, IntensityCountingMethod::SessionGoal);
        assert_eq!(default_counting_method(false).value, IntensityCountingMethod::TimeInZone);
        assert_eq!(novice_volume_bump_hold_weeks(0.5).value, Some((2, 3)));
        assert_eq!(novice_volume_bump_hold_weeks(3.0).value, None);
        assert!(unscheduled_deload(2).value);
        assert!(!unscheduled_deload(1).value);
    }

    #[test]
    fn maf_cap_adjusts() {
        assert_eq!(maf_cap_bpm(40.0, MafAdjustment::None).value, 140.0);
        assert_eq!(maf_cap_bpm(40.0, MafAdjustment::EliteImproving).value, 145.0);
        assert_eq!(maf_cap_bpm(40.0, MafAdjustment::Returning).value, 135.0);
        assert_eq!(maf_cap_bpm(40.0, MafAdjustment::Overtrained).value, 130.0);
        // Weak + contested per CQ-03.
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
        // Short/max efforts are effort-governed, HR lags.
        assert!(!run_workout_rx(RunSessionType::Interval).value.hr_governed);
        assert!(!run_workout_rx(RunSessionType::Strides).value.hr_governed);
        assert!(!run_workout_rx(RunSessionType::Hills).value.hr_governed);
    }

    #[test]
    fn workout_rx_bands_verbatim() {
        // running-018 Tempo 88–92 %HRmax, RPE 6–7.
        let t = run_workout_rx(RunSessionType::Tempo).value;
        assert_eq!(t.pct_hr_max, (0.88, 0.92));
        assert_eq!(t.rpe, (6, 7));
        // running-019 Interval 95–100 %HRmax, 3–5 min reps.
        let i = run_workout_rx(RunSessionType::Interval).value;
        assert_eq!(i.pct_hr_max, (0.95, 1.00));
        assert_eq!(i.duration_min, (3, 5));
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
