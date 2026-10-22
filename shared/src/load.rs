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
//! band verdict) are graded with a registry claim (`claims::*`). Canonical
//! claim_ids used here:
//! `LOAD-TRIMP-001`, `LOAD-SRPE-001`, `RUN-DECOUPLE-001`, `RUN-GAP-001`, `RUN-SPIKE-001`,
//! and the hard-blocked myth `LOAD-ACWR-001` (see the ACWR note near the end).

use crate::evidence::claims;
use crate::schema::Recommended;

// ---------------------------------------------------------------------------
// Efficiency Factor & aerobic decoupling
// ---------------------------------------------------------------------------

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

/// Classify a decoupling percentage into its Friel band (`RUN-DECOUPLE-001`),
/// graded: a prescriptive judgement ships with its evidence.
pub fn decoupling_band(decoupling_pct: f64) -> Recommended<DecouplingBand> {
    let band = if decoupling_pct < DECOUPLING_SOUND_PCT {
        DecouplingBand::SoundBase
    } else if !decoupling_high(decoupling_pct) {
        DecouplingBand::BuildBase
    } else {
        DecouplingBand::Insufficient
    };
    crate::evidence::graded(band, claims::RUN_DECOUPLE_001)
}

/// Decoupling percentage below which the aerobic base counts as sound
/// (`RUN-DECOUPLE-001`, Friel `<5%`).
pub const DECOUPLING_SOUND_PCT: f64 = 5.0;

/// Decoupling percentage at which the base counts as insufficient / the effort
/// as above the aerobic threshold (`RUN-DECOUPLE-001`).
pub const DECOUPLING_HIGH_PCT: f64 = 10.0;

/// Whether a decoupling percentage is in the flag band. ONE boundary for every
/// consumer: the readiness strip, the session-review feedback and the
/// base-fitness band must agree on a drift of exactly 10.0, and an EF ratio
/// like 1.0/0.9 yielding 9.9999…% from float error still counts as the flag.
pub fn decoupling_high(pct: f64) -> bool {
    pct >= DECOUPLING_HIGH_PCT - 1e-9
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
/// Guards non-positive inputs (returns 0.0) since ratios/powers are undefined,
/// and non-finite outputs: an absurd goal distance (`d2` ~1e280 m) overflows the
/// power to `±inf`/`NaN`, so a non-finite prediction is collapsed to the same
/// degenerate `0.0` sentinel. Callers route a `<= 0.0` prediction to the "need a
/// valid recent race" path rather than leaking `Inf` into the view.
pub fn riegel_predict(t1_sec: f64, d1: f64, d2: f64, exponent: f64) -> f64 {
    if t1_sec <= 0.0 || d1 <= 0.0 || d2 <= 0.0 {
        return 0.0;
    }
    let t2 = t1_sec * (d2 / d1).powf(exponent);
    if t2.is_finite() { t2 } else { 0.0 }
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
// HRmax estimators & HR zones (File 07)
// ---------------------------------------------------------------------------

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
// Critical Speed protocol validity (File 04 running-009, RUN-CS-PROTOCOL-001)
// ---------------------------------------------------------------------------

/// One maximal effort submitted to the Critical Speed protocol (running-009).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CsEffort {
    pub distance_m: f64,
    pub time_sec: f64,
}

/// The fitted 2-parameter linear model `D = CS·t + D′` (running-009):
/// CS = slope (m/s), D′ = intercept (distance reserve, m).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CsFit {
    pub cs_m_per_s: f64,
    pub d_prime_m: f64,
}

/// Why a set of efforts fails the running-009 protocol validity gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsProtocolViolation {
    /// Fewer than 2 efforts, the linear model needs at least two points.
    TooFewEfforts,
    /// More than 5 efforts (KB: "2–5 maximal efforts").
    TooManyEfforts,
    /// An effort shorter than 2 min or longer than 30 min. KB marks these
    /// explicitly invalid ("invalid <2 min or >30 min"). Note the KB also
    /// states a 2–20 min headline window yet an ideal pair of one 3–8 min +
    /// one 12–30 min effort, and does not reconcile the 20- vs 30-min upper
    /// bound; the gate rejects only the explicitly-invalid bounds.
    DurationOutOfRange,
    /// All efforts share the same duration, no slope can be fitted.
    DegenerateDurations,
    /// A non-positive fitted slope (CS ≤ 0): the efforts imply speed does not
    /// increase with duration, so D′ and the whole linear model are meaningless.
    NonPositiveSlope,
    /// Fitted D′ < 0: the KB reads a negative distance reserve as evidence of
    /// non-maximal trials, so the estimate is rejected.
    NegativeDPrime,
}

/// Explicit KB-invalid duration bounds (running-009): "invalid <2 min or >30 min".
pub const CS_EFFORT_MIN_SEC: f64 = 120.0;
pub const CS_EFFORT_MAX_SEC: f64 = 1800.0;

/// Ordinary least-squares fit of `D = CS·t + D′` over ≥2 efforts (running-009:
/// "CS = slope; D′ = distance reserve"). Pure math, no protocol judgement -
/// returns `None` when a slope is undefined (<2 points or all-equal durations).
/// For exactly two efforts this reduces to the two-point model
/// `CS = (D₂ − D₁)/(T₂ − T₁)`, `D′ = D₁ − CS·T₁` (File 07).
pub fn cs_linear_fit(efforts: &[CsEffort]) -> Option<CsFit> {
    if efforts.len() < 2 {
        return None;
    }
    let n = efforts.len() as f64;
    let mean_t = efforts.iter().map(|e| e.time_sec).sum::<f64>() / n;
    let mean_d = efforts.iter().map(|e| e.distance_m).sum::<f64>() / n;
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    for e in efforts {
        let dt = e.time_sec - mean_t;
        sxx += dt * dt;
        sxy += dt * (e.distance_m - mean_d);
    }
    if sxx.abs() < f64::EPSILON {
        return None;
    }
    let cs = sxy / sxx;
    Some(CsFit {
        cs_m_per_s: cs,
        d_prime_m: mean_d - cs * mean_t,
    })
}

/// Advisory pairing check (running-009): the KB's ideal protocol has one
/// effort of 3–8 min AND one of 12–30 min. Returns true when both windows are
/// covered. The KB's other pairing cautions, too-similar (1500 m + mile) and
/// too-spread (800 m + HM), carry NO numeric thresholds in the KB, only those
/// examples, so this ideal-window predicate is the only implementable pairing
/// gate; it is advisory (not a hard validity failure).
pub fn cs_pairing_ideal(efforts: &[CsEffort]) -> bool {
    let short = efforts
        .iter()
        .any(|e| (180.0..=480.0).contains(&e.time_sec));
    let long = efforts
        .iter()
        .any(|e| (720.0..=1800.0).contains(&e.time_sec));
    short && long
}

/// Validity-gated Critical Speed estimate (running-009 → `RUN-CS-PROTOCOL-001`,
/// Moderate; Galbraith/Nicolò 2018). Wraps [`cs_linear_fit`] in the KB's
/// protocol gates: 2–5 efforts, per-effort duration within the explicit
/// invalid bounds (≥2 min, ≤30 min), a fittable slope, and a non-negative D′
/// (negative D′ = non-maximal trials → estimate rejected). Returns the first
/// violation found, in the order listed on [`CsProtocolViolation`].
///
/// The 2–20 min headline window vs 30-min ideal-pair upper bound is a KB
/// discrepancy (documented on the claim); see [`cs_pairing_ideal`] for the
/// advisory ideal-pair check.
pub fn critical_speed_checked(
    efforts: &[CsEffort],
) -> Recommended<Result<CsFit, CsProtocolViolation>> {
    let out = if efforts.len() < 2 {
        Err(CsProtocolViolation::TooFewEfforts)
    } else if efforts.len() > 5 {
        Err(CsProtocolViolation::TooManyEfforts)
    } else if efforts
        .iter()
        .any(|e| e.time_sec < CS_EFFORT_MIN_SEC || e.time_sec > CS_EFFORT_MAX_SEC)
    {
        Err(CsProtocolViolation::DurationOutOfRange)
    } else {
        match cs_linear_fit(efforts) {
            None => Err(CsProtocolViolation::DegenerateDurations),
            Some(fit) if fit.cs_m_per_s <= 0.0 => Err(CsProtocolViolation::NonPositiveSlope),
            Some(fit) if fit.d_prime_m < 0.0 => Err(CsProtocolViolation::NegativeDPrime),
            Some(fit) => Ok(fit),
        }
    };
    crate::evidence::graded(out, claims::RUN_CS_PROTOCOL_001)
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
/// there. The search brackets 1 s … 12 h; a target VDOT outside a plausible human
/// range is clamped to that bracket rather than diverging.
pub fn daniels_predict(vdot_target: f64, distance_m: f64) -> f64 {
    if vdot_target <= 0.0 || distance_m <= 0.0 {
        return 0.0;
    }
    // vdot(d, t) decreases monotonically in t, so bisect: when the midpoint's
    // VDOT still exceeds the target the time is too short (raise the low bound).
    let mut lo = 1.0; // 1 s - faster than any real finish
    // 12 h covers every walkable marathon; a tighter cap would return an
    // optimistic bound for low-VDOT marathons and neutralize the running-040
    // derate for exactly its target population.
    let mut hi = 12.0 * 3600.0;
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

/// Running speed (m/min) whose Daniels & Gilbert oxygen cost equals `vo2`:
/// the positive root of `0.000104·v² + 0.182258·v − (4.60 + vo2) = 0`, the
/// inverse of [`daniels_vo2`]. `None` for a non-finite/non-positive target.
pub fn daniels_speed_for_vo2(vo2: f64) -> Option<f64> {
    if !vo2.is_finite() || vo2 <= 0.0 {
        return None;
    }
    let (a, b, c) = (0.000104, 0.182258, -(4.60 + vo2));
    let disc = b * b - 4.0 * a * c;
    let v = (-b + disc.sqrt()) / (2.0 * a);
    (v.is_finite() && v > 0.0).then_some(v)
}

/// Training pace (seconds per km) for a runner of `vdot` working at
/// `pct_vo2max` percent of VO2max (running-007: paces come from the CURRENT
/// VDOT of a recent race). Inverts [`daniels_vo2`] at `pct/100 × VDOT`.
/// `None` for non-positive inputs. `[Moderate]`
pub fn pace_sec_per_km_at(vdot: f64, pct_vo2max: f64) -> Option<f64> {
    if !vdot.is_finite() || vdot <= 0.0 || !pct_vo2max.is_finite() || pct_vo2max <= 0.0 {
        return None;
    }
    daniels_speed_for_vo2(vdot * pct_vo2max / 100.0).map(|v| 60_000.0 / v)
}

/// Pace band (fast, slow) in seconds per km for a Daniels VDOT band, from the
/// band's %VO2max limits (`running::vdot_band_vo2max_pct`, running-007 table).
/// Easy is the exception: its pace comes from marathon pace, 15–25 % slower
/// (running-015, the claim every easy prescription cites), because the 59–74
/// %VO2max row inverted through the oxygen-cost equation lands only ~7–9 %
/// slower than MP, well faster than Daniels' published E paces.
/// Repetition is ">100 %VO2max" with no stated upper limit, so it returns the
/// 100 % pace for both ends, read as "at least this fast". `[Moderate]`
pub fn vdot_band_pace_sec_per_km(vdot: f64, band: crate::schema::VdotBand) -> Option<(f64, f64)> {
    if matches!(band, crate::schema::VdotBand::Easy) {
        return crate::running::mp_relative_pace_sec_per_km(
            vdot,
            crate::running::RunWorkout::EasyGeneralAerobic,
        );
    }
    let (lo_pct, hi_pct) = crate::running::vdot_band_vo2max_pct(band);
    let slow = pace_sec_per_km_at(vdot, lo_pct)?;
    let fast = match hi_pct {
        Some(hi) => pace_sec_per_km_at(vdot, hi)?,
        None => slow,
    };
    Some((fast, slow))
}

// ---------------------------------------------------------------------------
// Session-RPE load (Foster 2001; KB 13-run-feedback-load load-srpe-001)
// ---------------------------------------------------------------------------

/// Session-RPE training load in arbitrary units: `duration_min × CR-10`
/// (Foster et al. 2001; `LOAD-SRPE-001`). A SEPARATE series from the HR-based
/// TRIMP (the two are not interchangeable on interval sessions), never summed
/// into it. The rating is clamped to the 0–10 scale; non-positive or
/// non-finite minutes yield 0.
pub fn srpe_load_au(duration_min: f64, session_rpe: u8) -> f64 {
    if !duration_min.is_finite() || duration_min <= 0.0 {
        return 0.0;
    }
    duration_min * f64::from(session_rpe.min(10))
}

// ---------------------------------------------------------------------------
// Zonal TRIMP variants (File 07)
// ---------------------------------------------------------------------------

/// Lucia TRIMP: 3 zones (below VT1, VT1–VT2, above VT2) weighted 1/2/3
/// (File 07). `minutes[0]` = low zone … `minutes[2]` = high zone. `[Moderate]`
pub fn lucia_trimp(minutes_per_zone: [f64; 3]) -> f64 {
    minutes_per_zone
        .iter()
        .enumerate()
        .map(|(i, m)| m * (i as f64 + 1.0))
        .sum()
}

// ---------------------------------------------------------------------------
// Ingest data-quality gates (File 07 "Sanity / QC")
// ---------------------------------------------------------------------------

/// A GPS ground speed above this (m/s) is impossible for a runner and is treated
/// as jitter (File 07 "reject samples implying >12 m/s"). SINGLE SOURCE OF TRUTH
/// for the ingest QC gate here, the core normalized-speed clamp
/// (`running::MAX_PLAUSIBLE_SPEED_MPS` re-exports this), and the shell live-jitter
/// guard, all three gate the same physical thing, so they share the KB value.
pub const MAX_PLAUSIBLE_SPEED_MPS: f64 = 12.0;

/// GPS speed sanity gate: reject samples implying an implausible runner speed
/// (File 07). Returns true when the speed is plausible (≤ [`MAX_PLAUSIBLE_SPEED_MPS`]).
pub fn gps_speed_plausible(speed_m_s: f64) -> bool {
    speed_m_s <= MAX_PLAUSIBLE_SPEED_MPS
}

/// GPS point-acceptance gate (Apple pattern, File 07): accept a new fix only if
/// it moved ≥ 2.5 m AND the implied vertical rate is ≤ 5 m/s.
pub fn gps_point_accept(distance_moved_m: f64, vertical_rate_m_s: f64) -> bool {
    distance_moved_m >= 2.5 && vertical_rate_m_s.abs() <= 5.0
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
        // A finite prediction round-trips normally.
        assert!(riegel_predict(1200.0, 5000.0, 10_000.0, 1.06).is_finite());
        // An absurd goal distance overflows the power to ±inf → collapse to the
        // degenerate 0.0 sentinel, never leak Inf/NaN to the caller.
        assert_eq!(riegel_predict(1200.0, 5000.0, 1e300, 1.06), 0.0);
    }

    #[test]
    fn vdot_50_threshold_pace_matches_daniels_table() {
        // Daniels' table: VDOT 50 → T pace ≈ 4:15/km (255 s). The T band is
        // 83–88 %VO2max; its fast end (88 %) is the table's T pace.
        let (fast, slow) = vdot_band_pace_sec_per_km(50.0, crate::schema::VdotBand::Threshold)
            .expect("T band pace");
        assert!((fast - 255.0).abs() <= 3.0, "T fast end {fast}");
        assert!(slow > fast && slow < 275.0, "T slow end {slow}");
        // Easy sits well slower than threshold, 15–25 % slower than marathon
        // pace (running-015).
        let (e_fast, e_slow) =
            vdot_band_pace_sec_per_km(50.0, crate::schema::VdotBand::Easy).unwrap();
        assert!(e_fast > slow && e_slow > e_fast, "E {e_fast}..{e_slow}");
        let mp = daniels_predict(50.0, 42_195.0) / 42.195;
        for e in [e_fast, e_slow] {
            let pct = (e / mp - 1.0) * 100.0;
            assert!(
                (15.0 - 1e-6..=25.0 + 1e-6).contains(&pct),
                "E {e} is {pct}% slower than MP {mp}"
            );
        }
        assert!((e_fast / mp - 1.15).abs() < 1e-9 && (e_slow / mp - 1.25).abs() < 1e-9);
        // The inversion round-trips through daniels_vo2.
        let v = daniels_speed_for_vo2(40.0).unwrap();
        assert!((daniels_vo2(v) - 40.0).abs() < 1e-9);
        // Repetition has no upper %VO2max: both ends are the 100 % pace.
        let (r_fast, r_slow) =
            vdot_band_pace_sec_per_km(50.0, crate::schema::VdotBand::Repetition).unwrap();
        assert_eq!(r_fast, r_slow);
        assert_eq!(pace_sec_per_km_at(0.0, 88.0), None);
    }

    #[test]
    fn srpe_load_is_minutes_times_cr10() {
        assert_eq!(srpe_load_au(40.0, 7), 280.0);
        assert_eq!(srpe_load_au(30.0, 15), 300.0, "rating clamps to 10");
        assert_eq!(srpe_load_au(-5.0, 7), 0.0);
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
    fn daniels_marathon_bracket_covers_slow_vdots() {
        // A VDOT-20 marathon finishes past 6 h; the 12 h bracket returns the
        // true (slower) prediction, not a clamped optimistic bound.
        let t = daniels_predict(20.0, 42_195.0);
        assert!(
            t > 6.0 * 3600.0 && t < 12.0 * 3600.0,
            "expected 6–12 h, got {t}"
        );
        assert!(
            (vdot(42_195.0, t) - 20.0).abs() < 0.5,
            "round-trips to VDOT 20, got {}",
            vdot(42_195.0, t)
        );
        // The running-040 marathon derate holds for its target population: at
        // VDOT 22 the band is non-degenerate.
        let band = crate::running::marathon_derated_band(4.0 * 3600.0, 4.5 * 3600.0, 22.0);
        assert!(
            band.value.0 < band.value.1,
            "derated band f_min < f_max: {:?}",
            band.value
        );
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
    fn karvonen_and_cooper() {
        // 70% HRR between 50 and 190 → 50 + 0.7*140 = 148.
        assert!((karvonen_target_hr(50.0, 190.0, 0.7) - 148.0).abs() < 1e-9);
        // Cooper at 2400 m: (2400-504.9)/44.73 ≈ 42.37.
        assert!((cooper_vo2max(2400.0) - 42.366).abs() < 0.01);
    }

    // -- Critical Speed protocol validity (running-009 / RUN-CS-PROTOCOL-001) --

    fn eff(distance_m: f64, time_sec: f64) -> CsEffort {
        CsEffort {
            distance_m,
            time_sec,
        }
    }

    #[test]
    fn cs_fit_two_points_matches_two_point_formula() {
        // 1000 m @ 200 s and 3000 m @ 660 s (both within 2–30 min? 200 s > 120 s ok,
        // 660 s ok) → identical CS/D′ as the closed-form two-point version.
        let efforts = [eff(1000.0, 200.0), eff(3000.0, 660.0)];
        let fit = cs_linear_fit(&efforts).expect("fit");
        // Closed-form two-point model: CS = 2000/460 ≈ 4.3478 m/s, D′ = 1000 − CS·200.
        let cs2: f64 = (3000.0 - 1000.0) / (660.0 - 200.0);
        assert!((cs2 - 4.347826).abs() < 1e-5, "got {cs2}");
        assert!((fit.cs_m_per_s - cs2).abs() < 1e-9);
        assert!((fit.d_prime_m - (1000.0 - cs2 * 200.0)).abs() < 1e-9);
        // Gated path accepts it too.
        let checked = critical_speed_checked(&efforts).value.expect("valid");
        assert!((checked.cs_m_per_s - cs2).abs() < 1e-9);
    }

    #[test]
    fn cs_gate_effort_count_bounds() {
        // KB: 2–5 maximal efforts.
        assert_eq!(
            critical_speed_checked(&[eff(1000.0, 240.0)]).value,
            Err(CsProtocolViolation::TooFewEfforts)
        );
        let six = [eff(1000.0, 240.0); 6];
        assert_eq!(
            critical_speed_checked(&six).value,
            Err(CsProtocolViolation::TooManyEfforts)
        );
    }

    #[test]
    fn cs_gate_duration_bounds_verbatim() {
        // KB explicit invalid bounds: <2 min or >30 min.
        let too_short = [eff(500.0, 119.0), eff(3000.0, 660.0)];
        assert_eq!(
            critical_speed_checked(&too_short).value,
            Err(CsProtocolViolation::DurationOutOfRange)
        );
        let too_long = [eff(1000.0, 240.0), eff(9000.0, 1801.0)];
        assert_eq!(
            critical_speed_checked(&too_long).value,
            Err(CsProtocolViolation::DurationOutOfRange)
        );
        // Exactly 2 min and exactly 30 min are NOT in the invalid ranges.
        let edges = [eff(700.0, 120.0), eff(7000.0, 1800.0)];
        assert!(critical_speed_checked(&edges).value.is_ok());
    }

    #[test]
    fn cs_gate_rejects_negative_d_prime_and_degenerate() {
        // Same duration twice: no slope.
        let flat = [eff(1000.0, 300.0), eff(1100.0, 300.0)];
        assert_eq!(
            critical_speed_checked(&flat).value,
            Err(CsProtocolViolation::DegenerateDurations)
        );
        // First trial non-maximal (way too slow for its duration) → fitted
        // line has negative intercept (D′ < 0) → rejected per KB.
        let non_maximal = [eff(600.0, 200.0), eff(3000.0, 660.0)];
        let fit = cs_linear_fit(&non_maximal).expect("fit exists");
        assert!(
            fit.d_prime_m < 0.0,
            "premise: D′ negative, got {}",
            fit.d_prime_m
        );
        assert_eq!(
            critical_speed_checked(&non_maximal).value,
            Err(CsProtocolViolation::NegativeDPrime)
        );
    }

    #[test]
    fn cs_gate_rejects_non_positive_slope() {
        // Efforts implying speed DECREASES with duration → non-positive slope,
        // which makes D′ (and the whole model) meaningless. Checked before the
        // D′ sign gate.
        let backwards = [eff(3000.0, 200.0), eff(1000.0, 660.0)];
        assert!(
            cs_linear_fit(&backwards).expect("fit").cs_m_per_s < 0.0,
            "premise: CS < 0"
        );
        assert_eq!(
            critical_speed_checked(&backwards).value,
            Err(CsProtocolViolation::NonPositiveSlope)
        );
        // Equal distances over different times → a flat (CS = 0) slope, also rejected.
        let flat_speed = [eff(2000.0, 200.0), eff(2000.0, 400.0)];
        assert_eq!(
            critical_speed_checked(&flat_speed).value,
            Err(CsProtocolViolation::NonPositiveSlope)
        );
    }

    #[test]
    fn cs_pairing_ideal_windows() {
        // Ideal: one 3–8 min + one 12–30 min effort.
        assert!(cs_pairing_ideal(&[eff(1200.0, 300.0), eff(5000.0, 1200.0)]));
        // Two short efforts: not ideal.
        assert!(!cs_pairing_ideal(&[eff(1200.0, 300.0), eff(1500.0, 400.0)]));
        // Two long efforts: not ideal.
        assert!(!cs_pairing_ideal(&[
            eff(5000.0, 1200.0),
            eff(6000.0, 1500.0)
        ]));
    }

    #[test]
    fn cs_checked_carries_moderate_evidence() {
        let r = critical_speed_checked(&[eff(1000.0, 200.0), eff(3000.0, 660.0)]);
        assert_eq!(r.claim_id(), claims::RUN_CS_PROTOCOL_001);
        assert!((r.confidence() - 0.65).abs() < f32::EPSILON);
        assert!(!r.safety_critical());
    }

    #[test]
    fn vdot_is_reasonable_for_a_5k() {
        // 5000 m in 20:00 (1200 s) → VDOT in a sane distance-runner range.
        let v = vdot(5000.0, 1200.0);
        assert!(v > 40.0 && v < 60.0, "VDOT out of range: {v}");
        assert_eq!(vdot(5000.0, 0.0), 0.0);
    }
}
