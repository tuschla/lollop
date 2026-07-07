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
        Some(entry) => (band, Some(entry.to_evidence()), Some(entry.to_confidence_tag())),
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
#[deprecated(
    note = "LOAD-ACWR-001 is a MarketingMyth (hard-blocked). ACWR is \
            mathematically coupled and does not predict injury. Use \
            week-to-week ramp <~10%/wk and RUN-SPIKE-001 instead."
)]
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
        assert!(efficiency_factor(180.0, 0.0).abs() < f64::EPSILON, "guard div0");

        // Banister TRIMP positive and sex factors differ.
        let hrr = hr_reserve_fraction(150.0, 50.0, 190.0);
        assert!(hrr > 0.0 && hrr < 1.0, "HRr in (0,1), got {hrr}");
        let male = banister_trimp(60.0, hrr, SexFactor::Male);
        let female = banister_trimp(60.0, hrr, SexFactor::Female);
        assert!(male > 0.0 && female > 0.0, "TRIMP must be positive");
        assert!((male - female).abs() > f64::EPSILON, "sex factors must differ");
    }
}
