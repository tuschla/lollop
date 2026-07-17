//! Training-load & running-analytics math core (knowledge-base File 07:
//! "Data Harvesting & Analytics - Ingestion Spec").
//!
//! Pure, deterministic, IO-free formula functions on `f64` primitives. Every
//! formula below is transcribed verbatim from File 07's "Formulas" section;
//! the exact expression + source is reproduced in each function's doc-comment.
//! Constants are copied verbatim from the spec, do not "tidy" them.
//!
//! Most functions here are pure math and return raw scalars, so they are NOT
//! wrapped in `Recommended<T>`: they compute a number, they do not prescribe.
//! Only functions that emit a prescriptive *judgement* (e.g. the decoupling
//! band verdict) reference the evidence registry via
//! [`crate::evidence::claim`]. Canonical claim_ids used here:
//! `LOAD-TRIMP-001`, `RUN-DECOUPLE-001`, `RUN-GAP-001`, `RUN-SPIKE-001`,
//! and the hard-blocked myth `LOAD-ACWR-001` (see the ACWR note near the end).

// ---------------------------------------------------------------------------
// TRIMP (Banister)
// ---------------------------------------------------------------------------

/// Banister TRIMP for one session. Claim: `LOAD-TRIMP-001`.
///
/// File 07: `TRIMP = duration_min × HRr × k·e^(b·HRr)`,
/// `HRr = (HRavg − HRrest)/(HRmax − HRrest)`; men k=0.64, b=1.92; women
/// k=0.86, b=1.67 (e ≈ 2.718). `[Moderate]`
///
/// `hr_ratio` is the pre-computed fractional heart-rate reserve `HRr` (0..1).
/// `sex_factor` selects the (k, b) pair via [`banister_sex_factors`].
pub fn banister_trimp(duration_min: f64, hr_ratio: f64, sex_factor: SexFactor) -> f64 {
    let (k, b) = banister_sex_factors(sex_factor);
    duration_min * hr_ratio * k * (b * hr_ratio).exp()
}

/// Sex selector for the Banister TRIMP (k, b) coefficients (File 07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SexFactor {
    /// men k=0.64, b=1.92
    Male,
    /// women k=0.86, b=1.67
    Female,
}

/// Verbatim Banister TRIMP coefficients per File 07: men k=0.64, b=1.92;
/// women k=0.86, b=1.67. Returns `(k, b)`.
pub const fn banister_sex_factors(sex: SexFactor) -> (f64, f64) {
    match sex {
        SexFactor::Male => (0.64, 1.92),
        SexFactor::Female => (0.86, 1.67),
    }
}

/// Fractional heart-rate reserve `HRr = (HRavg − HRrest)/(HRmax − HRrest)`
/// (File 07, Banister TRIMP). Denominator guarded to avoid div-by-zero.
pub fn hr_reserve_fraction(hr_avg: f64, hr_rest: f64, hr_max: f64) -> f64 {
    let denom = hr_max - hr_rest;
    if denom.abs() < f64::EPSILON {
        return 0.0;
    }
    (hr_avg - hr_rest) / denom
}

// ---------------------------------------------------------------------------
// TSS / rTSS / hrTSS (Coggan)
// ---------------------------------------------------------------------------

/// Coggan power TSS. Claim: `LOAD-TRIMP-001`.
///
/// File 07: `TSS = (sec × NP × IF)/(FTP × 3600) × 100`, `IF = NP/FTP`.
/// 1 h at FTP = 100 TSS. `[Moderate]`
///
/// `normalized_power` = NP (4th-root mean of 30-s rolling-avg power),
/// `intensity_factor` = IF = NP/FTP (pass explicitly; see
/// [`intensity_factor`]). FTP guarded against zero.
pub fn coggan_tss(
    duration_sec: f64,
    normalized_power: f64,
    intensity_factor: f64,
    ftp: f64,
) -> f64 {
    if ftp.abs() < f64::EPSILON {
        return 0.0;
    }
    (duration_sec * normalized_power * intensity_factor) / (ftp * 3600.0) * 100.0
}

/// Running rTSS. Claim: `LOAD-TRIMP-001`.
///
/// File 07: `rTSS = (sec × NGP × IF)/(FTPa × 3600) × 100`, `IF = NGP/FTPa`
/// (speeds in m/s; NGP = Normalized Graded Pace, FTPa = Functional Threshold
/// Pace). `[Moderate]`
///
/// `normalized_graded_pace` = NGP, `functional_threshold_pace` = FTPa (same
/// speed units, m/s). Identical algebraic form to [`coggan_tss`].
pub fn rtss(
    duration_sec: f64,
    normalized_graded_pace: f64,
    intensity_factor: f64,
    functional_threshold_pace: f64,
) -> f64 {
    if functional_threshold_pace.abs() < f64::EPSILON {
        return 0.0;
    }
    (duration_sec * normalized_graded_pace * intensity_factor)
        / (functional_threshold_pace * 3600.0)
        * 100.0
}

/// Intensity Factor `IF = normalized / threshold` (File 07: `IF = NP/FTP`,
/// and for running `IF = NGP/FTPa`). Threshold guarded against zero.
pub fn intensity_factor(normalized: f64, threshold: f64) -> f64 {
    if threshold.abs() < f64::EPSILON {
        return 0.0;
    }
    normalized / threshold
}

// ---------------------------------------------------------------------------
// Efficiency Factor & aerobic decoupling
// ---------------------------------------------------------------------------

/// Efficiency Factor. Claim: `RUN-DECOUPLE-001` (same family).
///
/// File 07: `EF = NGP (or NP) / HRavg` over steady aerobic effort.
/// `[Expert-opinion]`
///
/// `normalized_output` = NGP or NP; `avg_hr` = mean HR (bpm). HR guarded.
pub fn efficiency_factor(normalized_output: f64, avg_hr: f64) -> f64 {
    if avg_hr.abs() < f64::EPSILON {
        return 0.0;
    }
    normalized_output / avg_hr
}

/// Aerobic decoupling (Pa:HR / Pw:HR), in percent. Claim: `RUN-DECOUPLE-001`.
///
/// File 07: split steady effort in half;
/// `decoupling% = (EF_firsthalf − EF_secondhalf)/EF_firsthalf × 100`.
/// Friel: <5% = sound aerobic base; 5–9.99% = build base 3–6 wks;
/// ≥10% = above aerobic threshold / insufficient endurance. Valid for
/// efforts ≥ ~20 min (ideally 60–120). `[Expert-opinion]`
///
/// A positive result means EF fell in the second half (fatigue/drift);
/// negative means it rose (a "coupling"). `first_half_ef` guarded.
pub fn aerobic_decoupling(first_half_ef: f64, second_half_ef: f64) -> f64 {
    if first_half_ef.abs() < f64::EPSILON {
        return 0.0;
    }
    (first_half_ef - second_half_ef) / first_half_ef * 100.0
}

/// Friel decoupling band verdict for a computed `decoupling_pct`.
///
/// Bands per File 07 / `RUN-DECOUPLE-001`: `<5%` sound base, `5–9.99%` build
/// base, `>=10%` insufficient endurance / above aerobic threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecouplingBand {
    /// < 5%: sound aerobic base.
    SoundBase,
    /// 5.0–9.99%: build aerobic base 3–6 weeks.
    BuildBase,
    /// >= 10%: above aerobic threshold / insufficient endurance, flag.
    Insufficient,
}

/// Classify a decoupling percentage into its Friel band (`RUN-DECOUPLE-001`).
///
/// This is a prescriptive judgement, so it also surfaces the evidence entry.
/// Returns the band plus the `Evidence`/`ConfidenceTag` the caller must ship
/// alongside any advice derived from it. Falls back to a bare band if the
/// registry lookup ever fails (it should not for a canonical id).
pub fn decoupling_band(
    decoupling_pct: f64,
) -> (
    DecouplingBand,
    Option<crate::schema::Evidence>,
    Option<crate::schema::ConfidenceTag>,
) {
    // Epsilon at the 10% boundary: EF ratios like 1.0/0.9 yield 9.9999…%
    // from float error, which must still count as the ">=10%" flag band.
    let band = if decoupling_pct < 5.0 {
        DecouplingBand::SoundBase
    } else if decoupling_pct < 10.0 - 1e-9 {
        DecouplingBand::BuildBase
    } else {
        DecouplingBand::Insufficient
    };
    match crate::evidence::claim("RUN-DECOUPLE-001") {
        Some(entry) => (
            band,
            Some(entry.to_evidence()),
            Some(entry.to_confidence_tag()),
        ),
        None => (band, None, None),
    }
}

// ---------------------------------------------------------------------------
// Grade-Adjusted Pace (Minetti 2002)
// ---------------------------------------------------------------------------

/// Minetti et al. (2002) energy cost of running (J·kg⁻¹·m⁻¹).
/// Claim: `RUN-GAP-001`. Downhill unreliable (see `CQ-08`).
///
/// File 07 (verbatim), `i` = gradient fraction (−0.45…+0.45):
/// `Cr(i) = 155.4·i⁵ − 30.4·i⁴ − 43.3·i³ + 46.3·i² + 19.5·i + 3.6`
/// with `Cr(0) = 3.6`. Valid −45%…+45%; **downhill error up to 3×**.
/// `[Moderate]` uphill, `[Weak]` steep downhill.
///
/// `gradient` is the fractional grade `i` (e.g. 0.10 = +10%), NOT percent.
pub fn minetti_energy_cost(gradient: f64) -> f64 {
    let i = gradient;
    155.4 * i.powi(5) - 30.4 * i.powi(4) - 43.3 * i.powi(3) + 46.3 * i.powi(2) + 19.5 * i + 3.6
}

/// Minetti flat-ground energy cost `Cr(0) = 3.6` (File 07). Verbatim constant.
pub const MINETTI_FLAT_COST: f64 = 3.6;

/// Grade-adjusted pace factor via Minetti. Claim: `RUN-GAP-001`.
///
/// File 07: adjustment factor `= Cr(i)/Cr(0)`, `Cr(0) = 3.6`;
/// `GAP speed = actual speed × factor`. Valid −45%…+45%; **downhill error up
/// to 3×** (`CQ-08`, do not trust steep-downhill output; flag it).
///
/// `gradient` is the fractional grade `i` (not percent).
pub fn grade_adjusted_pace_factor(gradient: f64) -> f64 {
    minetti_energy_cost(gradient) / MINETTI_FLAT_COST
}

/// Simplified quadratic GAP adjustment (File 07, Hashiri.AI, `[Expert-opinion]`).
///
/// File 07: `adjustment(g%) = 1 + 0.033·g + 0.0025·g²` (g in **percent**).
/// Provided verbatim as an alternative to [`grade_adjusted_pace_factor`];
/// note this one takes grade in percent, not fraction.
pub fn quadratic_gap_factor(gradient_pct: f64) -> f64 {
    let g = gradient_pct;
    1.0 + 0.033 * g + 0.0025 * g * g
}

// ---------------------------------------------------------------------------
// Riegel race-time prediction (Riegel 1981)
// ---------------------------------------------------------------------------

/// Riegel endurance exponent by weekly training volume (File 07, verbatim):
/// Elite/100+ km ≈ 1.04; 60–100 km ≈ 1.06; 30–60 km ≈ 1.09; <30 km ≈ 1.12.
/// Higher mileage → flatter fatigue slope → smaller exponent. `[Moderate]`
pub fn riegel_exponent(weekly_km: f64) -> f64 {
    if weekly_km >= 100.0 {
        1.04
    } else if weekly_km >= 60.0 {
        1.06
    } else if weekly_km >= 30.0 {
        1.09
    } else {
        1.12
    }
}

/// Riegel race-time prediction: `t2 = t1·(d2/d1)^exponent` (Riegel 1981,
/// *American Scientist* 69(3):285–290). Predicts time over `d2` from a known
/// `t1` over `d1` (same distance units; times in seconds). Default exponent
/// 1.06; use [`riegel_exponent`] to pick by weekly volume. `[Moderate]`
///
/// Guards non-positive inputs (returns 0.0) since ratios/powers are undefined.
pub fn riegel_predict(t1_sec: f64, d1: f64, d2: f64, exponent: f64) -> f64 {
    if t1_sec <= 0.0 || d1 <= 0.0 || d2 <= 0.0 {
        return 0.0;
    }
    t1_sec * (d2 / d1).powf(exponent)
}

// ---------------------------------------------------------------------------
// Impulse-response: EWMA, CTL, ATL, TSB
// ---------------------------------------------------------------------------

/// Exponentially-weighted moving average of daily training load (File 07,
/// impulse-response / fitness–fatigue bookkeeping).
///
/// File 07 exponential smoothing: `s_t = α·x_t + (1−α)·s_{t−1}`, with
/// `α = 1 − exp(−Δt/τ)`. Here `Δt = 1 day` and `τ = time_constant_days`, so
/// `α = 1 − exp(−1/τ)`. `[Moderate]` for CTL/ATL bookkeeping.
///
/// `prev` = yesterday's smoothed value `s_{t−1}`, `today_load` = `x_t`.
pub fn ewma(prev: f64, today_load: f64, time_constant_days: f64) -> f64 {
    if time_constant_days.abs() < f64::EPSILON {
        return today_load;
    }
    let alpha = 1.0 - (-1.0 / time_constant_days).exp();
    alpha * today_load + (1.0 - alpha) * prev
}

/// Chronic Training Load (fitness): EWMA of daily TSS, τ = 42 d (File 07).
pub fn ctl(prev_ctl: f64, today_load: f64) -> f64 {
    ewma(prev_ctl, today_load, 42.0)
}

/// Acute Training Load (fatigue): EWMA of daily TSS, τ = 7 d (File 07).
pub fn atl(prev_atl: f64, today_load: f64) -> f64 {
    ewma(prev_atl, today_load, 7.0)
}

/// Training Stress Balance / form. File 07: `TSB (form) = CTL − ATL`
/// (yesterday's). `[Weak]` for prediction.
pub fn tsb(ctl: f64, atl: f64) -> f64 {
    ctl - atl
}

// ---------------------------------------------------------------------------
// Ingest unit normalization (File 07 "Formulas")
// ---------------------------------------------------------------------------

/// FIT semicircles → decimal degrees: `degrees = semicircles × (180 / 2^31)`
/// (File 07). `2^31` is exact in `f64`, so this is loss-free for i32 input.
pub fn semicircles_to_degrees(semicircles: i32) -> f64 {
    semicircles as f64 * (180.0 / 2_147_483_648.0)
}

/// FIT per-foot cadence → cadence in steps/min:
/// `steps/min = (cadence + fractional_cadence) × 2` (File 07).
pub fn cadence_to_steps_per_min(cadence: f64, fractional_cadence: f64) -> f64 {
    (cadence + fractional_cadence) * 2.0
}

/// Instantaneous grade `g = Δaltitude / Δhorizontal_distance` (File 07).
///
/// Per the ingest spec, slope is forced to 0 when the horizontal step is under
/// 5 m (GPS noise dominates below that), matching the ascent-hysteresis rule.
pub fn grade_fraction(delta_altitude_m: f64, delta_horizontal_m: f64) -> f64 {
    if delta_horizontal_m.abs() < 5.0 {
        return 0.0;
    }
    delta_altitude_m / delta_horizontal_m
}

// ---------------------------------------------------------------------------
// HRmax estimators & HR zones (File 07)
// ---------------------------------------------------------------------------

/// Population HRmax estimators (File 07, SEE ≈ ±10 bpm). Tanaka is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HrMaxFormula {
    /// `220 − age` `[Weak]`.
    Fox,
    /// `208 − 0.7·age` (default) `[Strong]`, Tanaka 2001.
    Tanaka,
    /// `206 − 0.88·age` `[Moderate]`, Gulati 2010.
    Gulati,
    /// `211 − 0.64·age` `[Moderate]`, Nes 2013 (HUNT).
    Nes,
    /// `207 − 0.7·age` `[Moderate]`, Gellish 2007.
    Gellish,
}

/// Estimate HRmax from age via the selected formula (File 07, verbatim
/// coefficients). Fox=220−age; Tanaka=208−0.7a; Gulati=206−0.88a;
/// Nes=211−0.64a; Gellish=207−0.7a.
pub fn hr_max_estimate(age_years: f64, formula: HrMaxFormula) -> f64 {
    match formula {
        HrMaxFormula::Fox => 220.0 - age_years,
        HrMaxFormula::Tanaka => 208.0 - 0.7 * age_years,
        HrMaxFormula::Gulati => 206.0 - 0.88 * age_years,
        HrMaxFormula::Nes => 211.0 - 0.64 * age_years,
        HrMaxFormula::Gellish => 207.0 - 0.7 * age_years,
    }
}

/// Karvonen target HR: `target = HRrest + frac·(HRmax − HRrest)` (File 07).
/// `frac` is the desired %HR-reserve as a fraction (0..1). `[Moderate]`
pub fn karvonen_target_hr(hr_rest: f64, hr_max: f64, frac: f64) -> f64 {
    hr_rest + frac * (hr_max - hr_rest)
}

/// Cooper 12-min test VO2max: `VO2max ≈ (d_meters − 504.9)/44.73` (File 07).
/// `[Moderate]`
pub fn cooper_vo2max(distance_m_12min: f64) -> f64 {
    (distance_m_12min - 504.9) / 44.73
}

// ---------------------------------------------------------------------------
// Critical Speed / D′ (File 07, 2-parameter hyperbolic)
// ---------------------------------------------------------------------------

/// Critical Speed from two maximal efforts (distance m, time s):
/// `CS = (D2 − D1)/(T2 − T1)` (File 07). Guards equal times. `[Moderate]`
pub fn critical_speed(d1_m: f64, t1_sec: f64, d2_m: f64, t2_sec: f64) -> f64 {
    let dt = t2_sec - t1_sec;
    if dt.abs() < f64::EPSILON {
        return 0.0;
    }
    (d2_m - d1_m) / dt
}

/// Anaerobic distance capacity D′ from the linear model `D = CS·t + D′`,
/// i.e. `D′ = D − CS·t` (File 07). Pass a single effort and its CS.
pub fn d_prime(distance_m: f64, critical_speed_ms: f64, time_sec: f64) -> f64 {
    distance_m - critical_speed_ms * time_sec
}

// ---------------------------------------------------------------------------
// VDOT / Daniels & Gilbert (File 07)
// ---------------------------------------------------------------------------

/// Daniels & Gilbert VO2 at running speed `v` (m/min):
/// `VO2 = −4.60 + 0.182258·v + 0.000104·v²` (File 07 verbatim).
pub fn daniels_vo2(v_m_per_min: f64) -> f64 {
    let v = v_m_per_min;
    -4.60 + 0.182258 * v + 0.000104 * v * v
}

/// Daniels & Gilbert fraction of VO2max sustainable for `t` minutes:
/// `%max = 0.8 + 0.1894393·e^(−0.012778·t) + 0.2989558·e^(−0.1932605·t)`
/// (File 07 verbatim).
pub fn daniels_pct_max(t_min: f64) -> f64 {
    0.8 + 0.1894393 * (-0.012778 * t_min).exp() + 0.2989558 * (-0.1932605 * t_min).exp()
}

/// VDOT from a race of `distance_m` in `time_sec`: `VDOT = VO2(v)/%max(t)`,
/// with `v = d/t` in m/min (File 07). Guards non-positive time. `[Moderate]`
pub fn vdot(distance_m: f64, time_sec: f64) -> f64 {
    if time_sec <= 0.0 {
        return 0.0;
    }
    let t_min = time_sec / 60.0;
    let v = distance_m / t_min;
    let pct = daniels_pct_max(t_min);
    if pct.abs() < f64::EPSILON {
        return 0.0;
    }
    daniels_vo2(v) / pct
}

/// Daniels race-time prediction: the finish time (seconds) over `distance_m`
/// that a runner of the given `vdot` would run. Inverts [`vdot`], for a fixed
/// distance, `vdot(d, t)` is strictly decreasing in `t` (a slower time implies a
/// lower VDOT), so a bisection on time converges to the unique matching finish.
/// Pair with [`riegel_predict`] and combine via `running::race_equivalency` so a
/// single method's false precision is never presented alone (File 07). `[Moderate]`
///
/// Guards non-positive inputs (returns 0.0) since VDOT/distance are undefined
/// there. The search brackets 1 s … 6 h; a target VDOT outside a plausible human
/// range is clamped to that bracket rather than diverging.
pub fn daniels_predict(vdot_target: f64, distance_m: f64) -> f64 {
    if vdot_target <= 0.0 || distance_m <= 0.0 {
        return 0.0;
    }
    // vdot(d, t) decreases monotonically in t, so bisect: when the midpoint's
    // VDOT still exceeds the target the time is too short (raise the low bound).
    let mut lo = 1.0; // 1 s - faster than any real finish
    let mut hi = 6.0 * 3600.0; // 6 h - slower than any race we predict
    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        if vdot(distance_m, mid) > vdot_target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

// ---------------------------------------------------------------------------
// Zonal TRIMP variants & hrTSS (File 07)
// ---------------------------------------------------------------------------

/// Edwards (zonal) TRIMP: `Σ(minutes_in_zone_i × i)` for 5 zones weighted 1–5
/// (File 07). `minutes[0]` is zone 1 (weight 1) … `minutes[4]` is zone 5. `[Moderate]`
pub fn edwards_trimp(minutes_per_zone: [f64; 5]) -> f64 {
    minutes_per_zone
        .iter()
        .enumerate()
        .map(|(i, m)| m * (i as f64 + 1.0))
        .sum()
}

/// Lucia TRIMP: 3 zones (below VT1, VT1–VT2, above VT2) weighted 1/2/3
/// (File 07). `minutes[0]` = low zone … `minutes[2]` = high zone. `[Moderate]`
pub fn lucia_trimp(minutes_per_zone: [f64; 3]) -> f64 {
    minutes_per_zone
        .iter()
        .enumerate()
        .map(|(i, m)| m * (i as f64 + 1.0))
        .sum()
}

/// hrTSS fallback: `Σ(minutes_in_zone × IF²)` scaled so 60 min at threshold
/// (IF 1.0) = 100 (File 07: time-in-HR-zone weighted by each zone's intensity
/// factor). IF is squared, not linear, so the fallback lands on the same
/// scale as TSS/rTSS, which are themselves quadratic in the intensity ratio
/// (rTSS ∝ NGP²/FTPa²). A linear weighting would match only at threshold and
/// systematically under-count time spent above it. Pass paired (minutes, IF)
/// per zone. `[Weak]`
pub fn hr_tss(zone_minutes_and_if: &[(f64, f64)]) -> f64 {
    let weighted: f64 = zone_minutes_and_if
        .iter()
        .map(|(m, if_)| m * if_ * if_)
        .sum();
    weighted / 60.0 * 100.0
}

// ---------------------------------------------------------------------------
// Ingest data-quality gates (File 07 "Sanity / QC")
// ---------------------------------------------------------------------------

/// GPS speed sanity gate: reject samples implying >12 m/s for a runner
/// (File 07). Returns true when the speed is plausible (≤ 12 m/s).
pub fn gps_speed_plausible(speed_m_s: f64) -> bool {
    speed_m_s <= 12.0
}

/// GPS point-acceptance gate (Apple pattern, File 07): accept a new fix only if
/// it moved ≥ 2.5 m AND the implied vertical rate is ≤ 5 m/s.
pub fn gps_point_accept(distance_moved_m: f64, vertical_rate_m_s: f64) -> bool {
    distance_moved_m >= 2.5 && vertical_rate_m_s.abs() <= 5.0
}

/// HR-jump plausibility gate: reject changes exceeding ±20 bpm/s (File 07).
/// Returns true when the per-second delta is plausible.
pub fn hr_jump_plausible(delta_bpm_per_s: f64) -> bool {
    delta_bpm_per_s.abs() <= 20.0
}

/// Stop detection for pace/load exclusion: speed < 0.5 m/s counts as stopped
/// (File 07 auto-pause rule).
pub fn is_stopped(speed_m_s: f64) -> bool {
    speed_m_s < 0.5
}

// ---------------------------------------------------------------------------
// ACWR: HARD-BLOCKED MYTH (LOAD-ACWR-001). DO NOT USE AS A PREDICTOR.
// ---------------------------------------------------------------------------
//
// The acute:chronic workload ratio (`ACWR = acute 7-day load / chronic
// 28-day load`, Gabbett 2016 "sweet spot" 0.8–1.3) is graded `MarketingMyth`
// in the evidence registry under `LOAD-ACWR-001` and is HARD-BLOCKED. It must
// NEVER be surfaced as advice or used to gate any decision.
//
// Reason (File 07 + File 09): the ratio is mathematically coupled, its
// numerator is a component of its denominator, so the "sweet spot" is a
// statistical artefact. It performed no better than random data; Enright 2020
// (RCT) found no injury reduction; Impellizzeri et al. 2019 filed a retraction
// request and Lolli et al. 2019 documented the coupling. Use week-to-week
// ramp (< ~10%/wk, and the `RUN-SPIKE-001` single-session distance-spike
// signal) as the guardrail instead, see `RUN-SPIKE-001`.
//
// A single function is provided ONLY so callers cannot silently reinvent it;
// it is `#[deprecated]`, never called internally, and returns the raw ratio
// with no interpretation. Do not build on it.

/// DO NOT USE. Hard-blocked myth `LOAD-ACWR-001` (mathematical coupling;
/// Impellizzeri/Lolli 2019). Present only to document the block; unused.
#[deprecated(note = "LOAD-ACWR-001 is a MarketingMyth (hard-blocked). ACWR is \
            mathematically coupled and does not predict injury. Use \
            week-to-week ramp <~10%/wk and RUN-SPIKE-001 instead.")]
#[allow(dead_code)]
pub fn acwr_do_not_use(acute_7day_load: f64, chronic_28day_load: f64) -> f64 {
    if chronic_28day_load.abs() < f64::EPSILON {
        return 0.0;
    }
    acute_7day_load / chronic_28day_load
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riegel_predicts_longer_races_slower() {
        // 5 km in 20:00 (1200 s) → 10 km should take >2× (fatigue exponent >1).
        let t10 = riegel_predict(1200.0, 5.0, 10.0, 1.06);
        assert!(
            t10 > 2400.0,
            "10k should exceed a doubled 5k pace, got {t10}"
        );
        // Verbatim: 1200 * 2^1.06 ≈ 2502 s.
        assert!((t10 - 2502.0).abs() < 5.0, "got {t10}");
        // Exponent bands by weekly volume.
        assert!((riegel_exponent(120.0) - 1.04).abs() < 1e-9);
        assert!((riegel_exponent(70.0) - 1.06).abs() < 1e-9);
        assert!((riegel_exponent(40.0) - 1.09).abs() < 1e-9);
        assert!((riegel_exponent(10.0) - 1.12).abs() < 1e-9);
        // Non-positive inputs guarded.
        assert_eq!(riegel_predict(0.0, 5.0, 10.0, 1.06), 0.0);
    }

    #[test]
    fn daniels_predict_inverts_vdot() {
        // A known race defines a VDOT; predicting that same distance at that VDOT
        // must reproduce the race time (the inverse round-trips).
        let d = 5000.0;
        let t = 1200.0; // 5 km in 20:00
        let v = vdot(d, t);
        let back = daniels_predict(v, d);
        assert!((back - t).abs() < 1.0, "round-trip within 1 s, got {back}");
        // Same fitness over a longer distance must take longer, and by more than
        // a pure distance scaling (endurance fades with duration).
        let t10 = daniels_predict(v, 10_000.0);
        assert!(t10 > 2.0 * t, "10k must exceed doubled 5k time, got {t10}");
        // Non-positive inputs guarded.
        assert_eq!(daniels_predict(0.0, 5000.0), 0.0);
        assert_eq!(daniels_predict(50.0, 0.0), 0.0);
    }

    #[test]
    fn tsb_is_ctl_minus_atl() {
        assert!((tsb(80.0, 55.0) - 25.0).abs() < f64::EPSILON);
        assert!((tsb(40.0, 70.0) - (-30.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn ewma_monotonic_sanity() {
        // Constant load above the current smoothed value must pull it up,
        // but never overshoot the applied load in a single step.
        let start = 50.0;
        let load = 100.0;
        let step1 = ewma(start, load, 42.0);
        let step2 = ewma(step1, load, 42.0);
        assert!(step1 > start, "EWMA should rise toward higher load");
        assert!(step2 > step1, "EWMA should keep rising under constant load");
        assert!(step1 < load, "single step must not overshoot the load");
        assert!(step2 < load, "still below the asymptote");
        // Faster time constant reacts more strongly on the same input.
        let fast = ewma(start, load, 7.0);
        assert!(fast > step1, "shorter tau reacts faster");
    }

    #[test]
    fn aerobic_decoupling_ten_percent() {
        // EF drops from 1.0 to 0.9 -> (1.0 - 0.9)/1.0 * 100 = 10%.
        let d = aerobic_decoupling(1.0, 0.9);
        assert!((d - 10.0).abs() < 1e-9, "expected ~10%, got {d}");
        // And that lands in the Insufficient (flag) band.
        let (band, ev, tag) = decoupling_band(d);
        assert_eq!(band, DecouplingBand::Insufficient);
        assert!(ev.is_some() && tag.is_some(), "registry entry must resolve");
    }

    #[test]
    fn minetti_flat_ground_cost() {
        // Cr(0) must equal the verbatim flat-ground constant 3.6.
        let c = minetti_energy_cost(0.0);
        assert!((c - 3.6).abs() < 1e-9, "Cr(0) should be 3.6, got {c}");
        // And the GAP factor at flat grade is exactly 1.0.
        let f = grade_adjusted_pace_factor(0.0);
        assert!((f - 1.0).abs() < 1e-9, "flat GAP factor should be 1.0");
    }

    #[test]
    fn quadratic_gap_matches_verbatim_formula() {
        // File 07: adjustment(g%) = 1 + 0.033·g + 0.0025·g² (g in percent).
        // Flat is exactly 1.0.
        assert!(
            (quadratic_gap_factor(0.0) - 1.0).abs() < 1e-9,
            "flat must be 1.0"
        );
        // +10% grade: 1 + 0.33 + 0.25 = 1.58 (pins both coefficients).
        assert!(
            (quadratic_gap_factor(10.0) - 1.58).abs() < 1e-9,
            "got {}",
            quadratic_gap_factor(10.0)
        );
        // Uphill costs more than flat; the quadratic term keeps downhill (−g)
        // above 1.0 too (it models energy cost, not a pace credit).
        assert!(
            quadratic_gap_factor(5.0) > 1.0,
            "uphill factor must exceed 1.0"
        );
    }

    #[test]
    fn tss_is_positive_for_real_session() {
        // 1 h at FTP (NP = FTP, IF = 1.0) is defined as exactly 100 TSS.
        let ftp = 250.0;
        let np = 250.0;
        let if_ = intensity_factor(np, ftp);
        let tss = coggan_tss(3600.0, np, if_, ftp);
        assert!(tss > 0.0, "TSS must be positive");
        assert!((tss - 100.0).abs() < 1e-6, "1h @ FTP == 100 TSS, got {tss}");
        // rTSS shares the form: 1h @ FTPa == 100.
        let rt = rtss(3600.0, 3.5, intensity_factor(3.5, 3.5), 3.5);
        assert!((rt - 100.0).abs() < 1e-6, "1h @ FTPa == 100 rTSS, got {rt}");
    }

    #[test]
    fn efficiency_factor_and_trimp_sanity() {
        // EF = normalized_output / avg_hr.
        let ef = efficiency_factor(180.0, 150.0);
        assert!((ef - 1.2).abs() < 1e-9, "EF should be 1.2, got {ef}");
        assert!(
            efficiency_factor(180.0, 0.0).abs() < f64::EPSILON,
            "guard div0"
        );

        // Banister TRIMP positive and sex factors differ.
        let hrr = hr_reserve_fraction(150.0, 50.0, 190.0);
        assert!(hrr > 0.0 && hrr < 1.0, "HRr in (0,1), got {hrr}");
        let male = banister_trimp(60.0, hrr, SexFactor::Male);
        let female = banister_trimp(60.0, hrr, SexFactor::Female);
        assert!(male > 0.0 && female > 0.0, "TRIMP must be positive");
        assert!(
            (male - female).abs() > f64::EPSILON,
            "sex factors must differ"
        );
    }

    #[test]
    fn semicircles_round_trip_and_cadence() {
        // 2^31 semicircles == 180 degrees exactly.
        assert!((semicircles_to_degrees(i32::MIN) + 180.0).abs() < 1e-9);
        assert!(semicircles_to_degrees(0).abs() < 1e-12);
        // FIT cadence 85 rpm/foot → 170 steps/min.
        assert!((cadence_to_steps_per_min(85.0, 0.0) - 170.0).abs() < 1e-9);
    }

    #[test]
    fn grade_zeroes_out_tiny_horizontal_steps() {
        assert!((grade_fraction(2.0, 40.0) - 0.05).abs() < 1e-9);
        // Under 5 m horizontal → forced flat.
        assert_eq!(grade_fraction(2.0, 4.0), 0.0);
    }

    #[test]
    fn hrmax_estimators_verbatim() {
        assert!((hr_max_estimate(30.0, HrMaxFormula::Tanaka) - 187.0).abs() < 1e-9);
        assert!((hr_max_estimate(30.0, HrMaxFormula::Fox) - 190.0).abs() < 1e-9);
        assert!((hr_max_estimate(40.0, HrMaxFormula::Gulati) - 170.8).abs() < 1e-9);
        assert!((hr_max_estimate(40.0, HrMaxFormula::Nes) - 185.4).abs() < 1e-9);
    }

    #[test]
    fn karvonen_and_cooper() {
        // 70% HRR between 50 and 190 → 50 + 0.7*140 = 148.
        assert!((karvonen_target_hr(50.0, 190.0, 0.7) - 148.0).abs() < 1e-9);
        // Cooper at 2400 m: (2400-504.9)/44.73 ≈ 42.37.
        assert!((cooper_vo2max(2400.0) - 42.366).abs() < 0.01);
    }

    #[test]
    fn critical_speed_and_d_prime() {
        // 1000 m @ 200 s and 3000 m @ 660 s → CS = 2000/460 ≈ 4.3478 m/s.
        let cs = critical_speed(1000.0, 200.0, 3000.0, 660.0);
        assert!((cs - 4.347826).abs() < 1e-5, "got {cs}");
        // D′ from the first effort: 1000 - CS*200.
        let dp = d_prime(1000.0, cs, 200.0);
        assert!((dp - (1000.0 - cs * 200.0)).abs() < 1e-9);
        // Equal times guarded.
        assert_eq!(critical_speed(1.0, 5.0, 2.0, 5.0), 0.0);
    }

    #[test]
    fn vdot_is_reasonable_for_a_5k() {
        // 5000 m in 20:00 (1200 s) → VDOT in a sane distance-runner range.
        let v = vdot(5000.0, 1200.0);
        assert!(v > 40.0 && v < 60.0, "VDOT out of range: {v}");
        assert_eq!(vdot(5000.0, 0.0), 0.0);
    }

    #[test]
    fn zonal_trimp_and_hrtss() {
        // Edwards: 10 min in each of 5 zones → 10*(1+2+3+4+5) = 150.
        assert!((edwards_trimp([10.0; 5]) - 150.0).abs() < 1e-9);
        // Lucia: 10 min each of 3 zones → 10*(1+2+3) = 60.
        assert!((lucia_trimp([10.0; 3]) - 60.0).abs() < 1e-9);
        // hrTSS: 60 min at threshold IF 1.0 → 100.
        assert!((hr_tss(&[(60.0, 1.0)]) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn ingest_quality_gates() {
        assert!(gps_speed_plausible(6.0));
        assert!(!gps_speed_plausible(13.0));
        assert!(gps_point_accept(3.0, 2.0));
        assert!(!gps_point_accept(1.0, 2.0)); // moved too little
        assert!(!gps_point_accept(3.0, 6.0)); // vertical rate too high
        assert!(hr_jump_plausible(15.0));
        assert!(!hr_jump_plausible(25.0));
        assert!(is_stopped(0.3));
        assert!(!is_stopped(0.8));
    }
}
