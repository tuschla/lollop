//! Strength & power calculators (knowledge-base File 02, Strength & Power
//! Resistance-Training Programming Rules).
//!
//! Pure, deterministic math: e1RM estimators, RPE/RIR mapping, reps→%1RM
//! estimation, and Prilepin volume governance. No IO. Values that make a
//! prescriptive recommendation are wrapped in [`Recommended`] via
//! [`Recommended::new`], which forces attached evidence + confidence.
//!
//! All numbers transcribed verbatim from File 02 (rules strength-005,
//! strength-007, strength-011 and the Prilepin's-chart verbatim table). Claim
//! ids come from the canonical registry in `crate::evidence`.

use crate::evidence::claims;
use crate::evidence::graded;
use crate::schema::Recommended;

// ---------------------------------------------------------------------------
// 1. Estimated 1RM (File 02 strength-005)
// ---------------------------------------------------------------------------

/// Epley estimated 1RM: `weight * (1 + reps/30)` (File 02 strength-005).
/// Domain: reps >= 1 (the e1RM formulas are undefined below one rep).
///
/// A single (`reps == 1`) IS the 1RM: it returns `weight` unchanged, matching
/// [`est_pct_1rm`]'s "a genuine single-rep max reads as its own 100 %" (File 02
/// strength-005). The raw formula's +3.3 % at one rep would bias every anchor
/// built from a single upward.
pub fn e1rm_epley(weight: f64, reps: u32) -> f64 {
    debug_assert!(reps >= 1, "e1RM formulas are defined for reps >= 1");
    if reps <= 1 {
        return weight;
    }
    weight * (1.0 + reps as f64 / 30.0)
}

/// Epley e1RM over a FRACTIONAL effective rep count (reps performed plus a
/// possibly half-point RIR). `≤ 1` is a single (identity, as [`e1rm_epley`]);
/// `≥ 2` is plain Epley; between 1 and 2 it interpolates linearly between those
/// two KB-anchored points so a single at RPE 9.5 (1.5 effective reps) is not
/// rounded up to a double. Pure arithmetic over strength-005, no new claim.
pub fn e1rm_epley_frac(weight: f64, effective_reps: f64) -> f64 {
    if effective_reps <= 1.0 {
        weight
    } else if effective_reps < 2.0 {
        weight * (1.0 + (effective_reps - 1.0) * (2.0 / 30.0))
    } else {
        weight * (1.0 + effective_reps / 30.0)
    }
}

/// %1RM a set to failure at `effective_reps` represents: the inverse of
/// [`e1rm_epley_frac`] (File 02 strength-005), so a single (≤ 1 effective rep)
/// reads as its own 100 % and the two never disagree. The one Epley inverse
/// every %1RM-from-reps estimate delegates to.
pub fn pct_1rm_at_effective_reps(effective_reps: f64) -> f64 {
    100.0 / e1rm_epley_frac(1.0, effective_reps)
}

/// Highest rep count for which the Brzycki denominator `37 − reps` stays
/// positive. At 37 reps it is zero (division by zero → +∞) and above it goes
/// negative (a negative 1RM); the estimator is only defined below 37 reps.
const BRZYCKI_MAX_REPS: u32 = 36;

/// Brzycki estimated 1RM: `weight * 36 / (37 - reps)` (File 02 strength-005).
///
/// Domain-gated: the raw formula is undefined at 37 reps and returns +∞ / a
/// negative 1RM at or above it, and this is a `pub` fn callable with an
/// unvalidated rep count. Reps are clamped into the formula's valid domain
/// (`BRZYCKI_MAX_REPS`) so the return is always finite and positive; the
/// estimate is only *accurate* in the 1–10 window noted in
/// strength-005/strength-006. Results at reps ≤ 36 are unchanged.
/// Domain: reps >= 1 (the e1RM formulas are undefined below one rep).
pub fn e1rm_brzycki(weight: f64, reps: u32) -> f64 {
    debug_assert!(reps >= 1, "e1RM formulas are defined for reps >= 1");
    let reps = reps.min(BRZYCKI_MAX_REPS);
    weight * 36.0 / (37.0 - reps as f64)
}

// ---------------------------------------------------------------------------
// 2. RPE ↔ RIR mapping (File 02 strength-007; registry AUTOREG-RIR-001)
// ---------------------------------------------------------------------------

/// Zourdos RPE↔RIR anchor: RPE 10 = 0 RIR, each RIR = −1 RPE (File 02
/// strength-007). Single source for both directions of the mapping so they
/// cannot desync.
const RPE_RIR_ANCHOR: f64 = 10.0;

/// Reps in reserve from RPE via the Zourdos anchor: RPE 10 = 0 RIR, each RIR
/// = −1 RPE (File 02 strength-007; registry AUTOREG-RIR-001). Clamped at 0.
pub fn rpe_to_rir(rpe: f64) -> f64 {
    let rir = RPE_RIR_ANCHOR - rpe;
    if rir < 0.0 { 0.0 } else { rir }
}

/// RPE from reps in reserve, inverse of [`rpe_to_rir`] (File 02 strength-007;
/// registry AUTOREG-RIR-001). Clamped to [0, 10].
pub fn rir_to_rpe(rir: f64) -> f64 {
    (RPE_RIR_ANCHOR - rir).clamp(0.0, RPE_RIR_ANCHOR)
}

/// Reps in reserve at or above which a self-rated set is OPEN-ENDED: the top
/// cell of the logging scale ("4+ reps left", RPE ≤ 6). Reps-in-reserve
/// reports are trusted only within about 1–3 reps of failure; farther out the
/// error exceeds 2 reps (File 05 feedback "trust only within 1–3 reps of
/// failure"; File 06 RPE accuracy "reliable 0 vs 2 RIR, not 6 vs 8 RIR";
/// File 03 hypertrophy-020), so such a rating is a lower bound, never an exact
/// reserve.
pub const OPEN_ENDED_RIR: f64 = 4.0;

/// Whether a logged RPE carries an exact reps-in-reserve reading: recorded
/// (`> 0`) and inside the trusted window, below [`OPEN_ENDED_RIR`]. An
/// unrated or open-ended set is a lower bound on reps-to-failure.
pub fn rpe_is_exact(rpe: f64) -> bool {
    rpe > 0.0 && rpe_to_rir(rpe) < OPEN_ENDED_RIR
}

/// Effective reps-to-failure for a set logged at a given RPE: the reps
/// performed plus the reps left in reserve, kept FRACTIONAL (RPE 9.5 = 0.5 RIR,
/// never rounded up to a whole extra rep; File 02 strength-007).
/// RPE→RIR uses the Zourdos anchor. `rpe == 0` means "not recorded" and an
/// open-ended rating ([`OPEN_ENDED_RIR`], "4+ reps left") carries no trusted
/// reserve: both return the reps as performed, a LOWER BOUND on the true
/// reps-to-failure, never a failure claim. So one easy-feeling set can never
/// inflate an estimated max by an untrusted 4+ reps.
pub fn effective_reps_f(reps: u32, rpe: f64) -> f64 {
    if rpe_is_exact(rpe) {
        reps as f64 + rpe_to_rir(rpe)
    } else {
        reps as f64
    }
}

/// Whole effective reps (floor of [`effective_reps_f`]) for rep-window gates
/// such as the >10-rep e1RM reliability cap (strength-006).
pub fn effective_reps(reps: u32, rpe: f64) -> u32 {
    effective_reps_f(reps, rpe).floor() as u32
}

/// Epley e1RM that accounts for the logged RPE by first extending the set to
/// its fractional effective reps-to-failure (see [`effective_reps_f`]).
pub fn e1rm_epley_rpe(weight: f64, reps: u32, rpe: f64) -> f64 {
    e1rm_epley_frac(weight, effective_reps_f(reps, rpe))
}

/// Estimated %1RM for a set of `reps` at the logged RPE: the Epley inverse
/// `100 / (1 + effective_reps/30)` over [`effective_reps_f`] (File 02
/// strength-005), the exact inverse of [`e1rm_epley_rpe`], so the two never
/// disagree. ESTIMATE ONLY: accurate at 2–10 reps (±5%), diverges ±15–20% above
/// 10 reps (strength-005/006).
///
/// Two documented clamps at the domain edges, applied in ALL builds:
/// - `reps == 0` is the degenerate sentinel (a %1RM is undefined) → `0.0`.
/// - a single to failure is a true 1RM → `100.0`. The raw Epley inverse has a
///   known r=1 discontinuity (it yields 96.77 %, not 100 %), so a genuine
///   single-rep max must read as its own 100 %, never sub-max (strength-005).
pub fn est_pct_1rm(reps: u32, rpe: f64) -> f64 {
    if reps == 0 {
        return 0.0;
    }
    pct_1rm_at_effective_reps(effective_reps_f(reps, rpe))
}

/// Reps still in reserve when `reps` are performed at `pct_1rm` % of the e1RM,
/// read off the Epley model (inverse of strength-005: max reps at p% =
/// `30·(100/p − 1)`). Fractional and unclamped; callers clamp to their domain.
pub fn expected_rir_at_pct(pct_1rm: f64, reps: u32) -> f64 {
    if pct_1rm <= 0.0 {
        return f64::INFINITY;
    }
    30.0 * (100.0 / pct_1rm - 1.0) - reps as f64
}

/// The effort target a %1RM prescription implies: the Epley-model reps in
/// reserve ([`expected_rir_at_pct`]) rounded and clamped to 1–4 RIR, never 0, so
/// a percentage set is never sent to failure (the strength band is 1–3 RIR,
/// STR-INTENT-001; 4 = the KB RPE↔RIR table's floor, strength-007). Used as the
/// "~N in reserve" guard and the log editor's RPE hint: a day below one's e1RM
/// shows up as a set harder than this target.
pub fn effort_target_rir(pct_1rm: f64, reps: u32) -> u8 {
    expected_rir_at_pct(pct_1rm, reps).round().clamp(1.0, 4.0) as u8
}

// ---------------------------------------------------------------------------
// 3. Prilepin's chart (File 02 strength-011, verbatim table)
// ---------------------------------------------------------------------------

/// One row of Prilepin's chart: an intensity band with its rep-per-set range,
/// optimal total reps, and total-rep range (File 02 verbatim table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrilepinRow {
    /// Inclusive lower %1RM bound for this band.
    pub pct_min: u8,
    /// Inclusive upper %1RM bound for this band.
    pub pct_max: u8,
    /// Reps per set (min, max).
    pub reps_per_set: (u8, u8),
    /// Optimal total reps for the session at this intensity.
    pub optimal_total: u16,
    /// Acceptable total-rep range (min, max).
    pub total_range: (u16, u16),
}

/// Prilepin's chart, transcribed verbatim from File 02 (strength-011).
///
/// | %1RM   | Reps/set | Optimal total | Total range |
/// |--------|----------|---------------|-------------|
/// | <70%   | 3–6      | 24            | 18–30       |
/// | 70–80% | 3–6      | 18            | 12–24       |
/// | 80–90% | 2–4      | 15            | 10–20       |
/// | >90%   | 1–2      | 7             | 4–10        |
///
/// Band bounds are encoded as half-open on the verbatim breakpoints (69, 79,
/// 89, 100) so every intensity 0–100% resolves to exactly one row. The final
/// row transcribes the KB's ">90%" band, which has NO upper bound in the
/// source, supra-maximal intensities (>100%, e.g. accommodating resistance or
/// overload work) resolve to it via [`prilepin_for`]; `pct_max: 100` is only
/// the table-encoding sentinel, not a KB ceiling.
pub static PRILEPIN: &[PrilepinRow] = &[
    PrilepinRow {
        pct_min: 0,
        pct_max: 69,
        reps_per_set: (3, 6),
        optimal_total: 24,
        total_range: (18, 30),
    },
    PrilepinRow {
        pct_min: 70,
        pct_max: 79,
        reps_per_set: (3, 6),
        optimal_total: 18,
        total_range: (12, 24),
    },
    PrilepinRow {
        pct_min: 80,
        pct_max: 89,
        reps_per_set: (2, 4),
        optimal_total: 15,
        total_range: (10, 20),
    },
    PrilepinRow {
        pct_min: 90,
        pct_max: 100,
        reps_per_set: (1, 2),
        optimal_total: 7,
        total_range: (4, 10),
    },
];

/// Look up the Prilepin row governing a given %1RM (File 02 strength-011).
/// Returns `None` for negative intensities. The <70% band is treated as a
/// floor, matching the verbatim "<70%" row; intensities above 100% resolve to
/// the ">90%" row because the KB's top band is unbounded above (supra-maximal
/// work, e.g. accommodating resistance).
pub fn prilepin_for(pct_1rm: f64) -> Option<&'static PrilepinRow> {
    if !pct_1rm.is_finite() || pct_1rm < 0.0 {
        return None;
    }
    // Round to nearest whole percent for band membership; the ">90%" row is
    // unbounded above in the source, so clamp supra-max intensities onto it.
    let pct = (pct_1rm.round() as i64).min(100);
    PRILEPIN
        .iter()
        .find(|row| pct >= row.pct_min as i64 && pct <= row.pct_max as i64)
}

/// Whether `total_reps` falls within the Prilepin total-rep range for the band
/// at `pct_1rm` (File 02 strength-011 volume governor, STR-PRILEPIN-001).
/// `false` if the intensity is invalid (negative/non-finite) or the volume is
/// outside the band's window.
pub fn prilepin_volume_ok(pct_1rm: f64, total_reps: u16) -> Recommended<bool> {
    let ok = match prilepin_for(pct_1rm) {
        Some(row) => total_reps >= row.total_range.0 && total_reps <= row.total_range.1,
        None => false,
    };
    graded(ok, claims::STR_PRILEPIN_001)
}

// ---------------------------------------------------------------------------
// 4. Loading prescription bands (File 02 strength-001/002/003; STR-INTENT-001)
// ---------------------------------------------------------------------------

/// The training quality a loading prescription targets (File 02 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LiftGoal {
    MaxStrength,
    Power,
    Hypertrophy,
}

/// A loading prescription: intensity, reps, sets, rest, and proximity to failure
/// (File 02 strength-001/002/003, verbatim bands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadingRx {
    /// Working intensity as %1RM (min, max).
    pub pct_1rm: (u8, u8),
    /// Reps per set (min, max).
    pub reps: (u8, u8),
    /// Sets (min, max).
    pub sets: (u8, u8),
    /// Inter-set rest in seconds (min, max).
    pub rest_sec: (u16, u16),
    /// Reps-in-reserve target (min, max); `None` when the KB states no numeric
    /// RIR (Power: "never to failure; high RIR" with no number).
    pub rir: Option<(u8, u8)>,
}

/// Loading bands by goal (File 02 strength-001/002/003). Strength 80-100% /
/// 1-5 / 1-3 RIR (STR-INTENT-001, Strong); power a velocity-biased spectrum
/// stopped well short of failure (strength-002 → STR-PWR-001, Moderate);
/// hypertrophy 65-85% / 6-12 / 0-3 RIR.
///
/// The Power band is the ENVELOPE of the KB's load spectrum (0-60%
/// ballistic/jump; 30-70% loaded power; 70-95% weightlifting pulls → 0-95%
/// overall); never prescribe the whole envelope to one exercise. The
/// KB states only "never to failure; high RIR" for power with NO number, so the
/// Power band's `rir` is `None` (the old numeric 3-5 was an invented encoding);
/// callers state the qualitative "stop before bar speed drops" instruction.
///
/// Hypertrophy `rest_sec` is `(30, 120)`: strength-003's `parameters:` line
/// gives `rest_sec 30-120`, and the statement's headline "30-90 s" carries the
/// KB's own "up to 2 min" extension.
///
/// MaxStrength/Power `sets: (3, 6)` truncates the KB's open-ended "3-6+" (the
/// file's `pct_top_open` convention is not extended to `LoadingRx`).
pub fn loading_rx(goal: LiftGoal) -> Recommended<LoadingRx> {
    let rx = match goal {
        LiftGoal::MaxStrength => LoadingRx {
            pct_1rm: (80, 100),
            reps: (1, 5),
            sets: (3, 6),
            rest_sec: (180, 300),
            rir: Some((1, 3)),
        },
        LiftGoal::Power => LoadingRx {
            pct_1rm: (0, 95),
            reps: (1, 5),
            sets: (3, 6),
            rest_sec: (180, 300),
            rir: None,
        },
        LiftGoal::Hypertrophy => LoadingRx {
            pct_1rm: (65, 85),
            reps: (6, 12),
            sets: (3, 6),
            rest_sec: (30, 120),
            rir: Some((0, 3)),
        },
    };
    let claim_id = match goal {
        LiftGoal::Power => claims::STR_PWR_001,
        LiftGoal::MaxStrength | LiftGoal::Hypertrophy => claims::STR_INTENT_001,
    };
    graded(rx, claim_id)
}

// ---------------------------------------------------------------------------
// 5. Load progression (File 02 strength-012/014; STR-2FOR2-001 / STR-PCTPROG-001)
// ---------------------------------------------------------------------------

/// 2-for-2 rule (File 02 strength-012): increase load once the athlete beats the
/// goal by >=2 reps on the last set in 2 consecutive sessions. STR-2FOR2-001
/// (safety-critical per the KB: caps how fast load may ramp).
pub fn two_for_two_met(reps_over_goal_last_set: u8, consecutive_sessions: u8) -> Recommended<bool> {
    graded(
        reps_over_goal_last_set >= 2 && consecutive_sessions >= 2,
        claims::STR_2FOR2_001,
    )
}

/// Percentage auto-progression per successful week (File 02 strength-014):
/// lower-body +2.5-5%, upper-body +1-2.5% of load. Returns (min, max) fraction.
/// STR-PCTPROG-001.
pub fn weekly_pct_increment(upper_body: bool) -> Recommended<(f64, f64)> {
    let inc = if upper_body {
        (0.01, 0.025)
    } else {
        (0.025, 0.05)
    };
    graded(inc, claims::STR_PCTPROG_001)
}

/// Largest single load increase STR-LOADPROG-001 sanctions, as a fraction of
/// the current load (File 13 strpow-add-005: "+2–10 % once 1–2 reps over the
/// prescribed number"). A fixed plate step above this on a light load is more
/// than the rule allows.
pub const MAX_LOAD_STEP_FRAC: f64 = 0.10;

/// Whether a stall triggers a deload / model switch (File 02 strength-039):
/// (estimated) 1RM flat for >=2 weeks despite adequate recovery. STR-STALL-001.
pub fn stall_triggers_deload(weeks_stalled: u8, recovery_adequate: bool) -> Recommended<bool> {
    graded(
        weeks_stalled >= 2 && recovery_adequate,
        claims::STR_STALL_001,
    )
}

// ---------------------------------------------------------------------------
// 6b. Periodization model selection (File 02 strength-009/010; STR-MODEL-001)
// ---------------------------------------------------------------------------

/// A periodization model (File 02 strength-010/021-024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodizationModel {
    /// High-volume/low-intensity → low-volume/high-intensity across mesocycles.
    Linear,
    /// Daily-undulating: vary intensity/rep focus session-to-session.
    Dup,
    /// Accumulation → transmutation → realization concentrated blocks.
    Block,
    /// Max-effort / dynamic-effort / repetition rotation (advanced only).
    Conjugate,
}

/// Select a periodization model by training age (File 02 strength-010): novice →
/// linear; intermediate → DUP or block; advanced → block or conjugate. No single
/// model is hard-coded as superior (strength-009). Returns the primary default
/// per level. STR-MODEL-001 (Moderate).
pub fn periodization_model(
    level: crate::individualization::TrainingAge,
) -> Recommended<PeriodizationModel> {
    use crate::individualization::TrainingAge;
    let model = match level {
        TrainingAge::Novice => PeriodizationModel::Linear,
        TrainingAge::Intermediate => PeriodizationModel::Dup,
        TrainingAge::Advanced => PeriodizationModel::Block,
    };
    graded(model, claims::STR_MODEL_001)
}

// ---------------------------------------------------------------------------
// 6. Taper (File 02 strength-026/027; TAPER-001)
// ---------------------------------------------------------------------------

/// Peaking taper prescription (File 02 strength-026; TAPER-001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TaperRx {
    /// Exponential volume reduction (min, max) as fractions.
    pub volume_reduction_frac: (f64, f64),
    /// Taper duration in days (min, max).
    pub duration_days: (u8, u8),
    /// Intensity and frequency are held, not reduced.
    pub hold_intensity: bool,
}

/// Volume reduction band of the best-evidenced taper, as fractions of the
/// pre-taper load (Bosquet 2007 meta-analysis: 41–60 %; File 02 strength-026,
/// running-037; TAPER-001).
pub const TAPER_VOLUME_CUT_FRAC: (f64, f64) = (0.41, 0.60);

/// Best-evidenced peaking taper: cut volume 41-60% exponentially over ~2 weeks
/// while holding intensity and frequency (File 02 strength-026; TAPER-001).
pub fn taper_rx() -> Recommended<TaperRx> {
    graded(
        TaperRx {
            volume_reduction_frac: TAPER_VOLUME_CUT_FRAC,
            duration_days: (8, 14),
            hold_intensity: true,
        },
        claims::TAPER_001,
    )
}

// ---------------------------------------------------------------------------
// 7. Plyometric volume caps (File 02 strength-032; PLYO-001)
// ---------------------------------------------------------------------------

/// Foot-contact ceiling per plyometric session by training level (File 02
/// strength-032; PLYO-001). Returns (min, max) foot contacts. Progress volume
/// OR intensity, never both.
pub fn plyo_foot_contact_cap(
    level: crate::individualization::TrainingAge,
) -> Recommended<(u16, u16)> {
    use crate::individualization::TrainingAge;
    let cap = match level {
        TrainingAge::Novice => (80, 100),
        TrainingAge::Intermediate => (100, 120),
        TrainingAge::Advanced => (120, 140),
    };
    graded(cap, claims::PLYO_001)
}

/// The low-dose plyometric band the KB also rates effective (File 02
/// strength-032 parameters: "low-dose 40–60" foot contacts per session), well
/// inside every level's cap ([`plyo_foot_contact_cap`]). PLYO-001.
pub fn plyo_low_dose_contacts() -> Recommended<(u16, u16)> {
    graded((40, 60), claims::PLYO_001)
}

// ---------------------------------------------------------------------------
// 8. Power/peaking specifics (File 02 strength-029/031/034)
// ---------------------------------------------------------------------------

/// Days before a test/meet to schedule the last true near-max deadlift, given
/// its high systemic fatigue cost (File 02 strength-029). Returns (min, max)
/// days out. STR-DLPEAK-001 (safety-critical per the KB).
pub fn deadlift_peak_days_out() -> Recommended<(u8, u8)> {
    graded((10, 14), claims::STR_DLPEAK_001)
}

/// PAP/PAPE contrast rest window in minutes for a HEAVY conditioning activity
/// (File 02 strength-034): ~5-7 min (>=5; overall optimal window ~3-7 min).
/// Stronger athletes potentiate more and earlier; abort if the explosive set
/// is slower than baseline. STR-PAP-001 (Moderate; Seitz & Haff).
pub fn pap_rest_window_min() -> Recommended<(u8, u8)> {
    graded((5, 7), claims::STR_PAP_001)
}

/// Olympic-lift pulling-derivative prescription (File 02 strength-031). Only
/// fields the KB states, no invented rest/RIR numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OlympicDerivativeRx {
    /// %1RM of the FULL lift (min, nominal max). The KB band is "85-100%+":
    /// the top is open-ended (see [`Self::pct_top_open`]).
    pub pct_1rm: (u8, u8),
    /// True: the intensity top end is open ("100%+" of the full-lift 1RM,
    /// since derivative strength can exceed the full lift's).
    pub pct_top_open: bool,
    /// Reps per set (min, max).
    pub reps: (u8, u8),
    /// Sets (min, max).
    pub sets: (u8, u8),
    /// Inter-set rest in seconds (min, max). Not stated by strength-031
    /// itself; transcribed from the KB master loading table's POWER column
    /// ("3-5 min, full ATP-PCr recovery"), which covers weightlifting pulls
    /// (strength-002/030 place pulls in the power spectrum).
    pub rest_sec: (u16, u16),
    /// Place early in the session (KB: "session placement early").
    pub early_in_session: bool,
    /// Velocity-biased variants go lighter: hang power clean / high pull
    /// studied at 30/45/65/80% 1RM (KB parameters, verbatim).
    pub velocity_variant_pcts_1rm: &'static [u8],
    /// Jump shrug load ~30% of body mass (KB parameters, verbatim).
    pub jump_shrug_pct_bodymass: u8,
}

/// Olympic-lift pulling-derivative loading (File 02 strength-031): 3-5 sets ×
/// 1-3 reps at 85-100%+ of full-lift 1RM (open-ended top; lighter for
/// velocity-biased variants), placed early in the session. STR-OLY-001
/// (Moderate; Suchomel).
pub fn olympic_derivative_rx() -> Recommended<OlympicDerivativeRx> {
    graded(
        OlympicDerivativeRx {
            pct_1rm: (85, 100),
            pct_top_open: true,
            reps: (1, 3),
            sets: (3, 5),
            rest_sec: (180, 300),
            early_in_session: true,
            velocity_variant_pcts_1rm: &[30, 45, 65, 80],
            jump_shrug_pct_bodymass: 30,
        },
        claims::STR_OLY_001,
    )
}

// ---------------------------------------------------------------------------
// 9. Lombardi e1RM (File 02 strength-005, third estimator)
// ---------------------------------------------------------------------------

/// Lombardi estimated 1RM: `weight * reps^0.10` (File 02 strength-005). Same
/// 1–10 rep accuracy window (±5%) as Epley/Brzycki; treat as approximate.
/// Domain: reps >= 1 (the e1RM formulas are undefined below one rep).
pub fn e1rm_lombardi(weight: f64, reps: u32) -> f64 {
    debug_assert!(reps >= 1, "e1RM formulas are defined for reps >= 1");
    weight * (reps as f64).powf(0.10)
}

// ---------------------------------------------------------------------------
// 10. RPE-anchored top set + back-offs (File 02 strength-015; STR-BACKOFF-001)
// ---------------------------------------------------------------------------

/// An RPE-anchored top-set / back-off scheme (File 02 strength-015).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackOffRx {
    /// Target RPE for the top set (~8).
    pub top_set_rpe: u8,
    /// Load drop for back-off sets as a fraction of the top set (min, max).
    pub drop_frac: (f64, f64),
}

/// Top set at RPE ~8, back-off sets dropped 10–15% of the top-set load (File 02
/// strength-015). STR-BACKOFF-001 (Moderate).
pub fn rpe_anchored_back_off() -> Recommended<BackOffRx> {
    graded(
        BackOffRx {
            top_set_rpe: 8,
            drop_frac: (0.10, 0.15),
        },
        claims::STR_BACKOFF_001,
    )
}

// ---------------------------------------------------------------------------
// 11. Velocity-loss set termination by goal (File 02 strength-018; AUTOREG-VL-001)
// ---------------------------------------------------------------------------

/// Velocity-loss fraction at which to terminate the set for a given goal (File
/// 02 strength-018, reconciled with autoreg-010): ≥10% VL for power (preserves
/// peak bar velocity), ≥20% VL for max strength (preserves 1RM), ≥40% VL
/// permitted for hypertrophy. AUTOREG-VL-001. The KB sources conflicted —
/// strength-018 grouped power with max-strength at 20% while autoreg-010 states
/// "10% power"; VBT literature terminates power work at ~10% VL, so Power is
/// split out here to 0.10. This is the SINGLE SOURCE of the goal→VL map;
/// `autoreg::vl_threshold_pct` delegates to it (as whole-percent) so the two
/// files can never drift.
pub fn vl_termination_threshold(goal: LiftGoal) -> Recommended<f64> {
    let vl = match goal {
        LiftGoal::Power => 0.10,
        LiftGoal::MaxStrength => 0.20,
        LiftGoal::Hypertrophy => 0.40,
    };
    graded(vl, claims::AUTOREG_VL_001)
}

// ---------------------------------------------------------------------------
// 12. Periodization phase tables (File 02 strength-021 linear / 022 block)
// ---------------------------------------------------------------------------

/// A per-phase loading prescription. Fields the knowledge base leaves
/// unspecified for a phase are `None` (e.g. block realization gives only reps;
/// linear taper defers to the taper template). No numbers are invented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseRx {
    /// Working intensity %1RM (min, max), when the table specifies it.
    pub pct_1rm: Option<(u8, u8)>,
    /// True when the KB gives the intensity top end as open ("+"), e.g. the
    /// linear Peak block's verbatim "90–95%+", the nominal max in `pct_1rm`
    /// is then a floor for top singles/doubles, not a ceiling.
    pub pct_top_open: bool,
    /// Sets (min, max), when specified.
    pub sets: Option<(u8, u8)>,
    /// Reps per set (min, max), when specified.
    pub reps: Option<(u8, u8)>,
    /// Mesocycle week span for this phase (min, max).
    pub weeks: (u8, u8),
}

/// Linear/step periodization phase (File 02 strength-021 verbatim table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinearPhase {
    /// Hypertrophy base: 67–75%, 3–5×8–12, weeks 1–4.
    Base,
    /// Basic strength build: 80–87%, 4–5×4–6, weeks 5–8.
    Build,
    /// Peak strength: 90–95%+, 3–5×1–3, weeks 9–11.
    Peak,
    /// Taper/test week 12: maintain intensity, per the taper template.
    Taper,
}

/// Linear periodization phase prescription (File 02 strength-021). Taper defers
/// to [`taper_rx`] (pct/sets/reps `None`, maintain intensity). STR-LINEAR-001.
pub fn linear_phase_rx(phase: LinearPhase) -> Recommended<PhaseRx> {
    let rx = match phase {
        LinearPhase::Base => PhaseRx {
            pct_1rm: Some((67, 75)),
            pct_top_open: false,
            sets: Some((3, 5)),
            reps: Some((8, 12)),
            weeks: (1, 4),
        },
        LinearPhase::Build => PhaseRx {
            pct_1rm: Some((80, 87)),
            pct_top_open: false,
            sets: Some((4, 5)),
            reps: Some((4, 6)),
            weeks: (5, 8),
        },
        LinearPhase::Peak => PhaseRx {
            pct_1rm: Some((90, 95)),
            pct_top_open: true,
            sets: Some((3, 5)),
            reps: Some((1, 3)),
            weeks: (9, 11),
        },
        LinearPhase::Taper => PhaseRx {
            pct_1rm: None,
            pct_top_open: false,
            sets: None,
            reps: None,
            weeks: (12, 12),
        },
    };
    graded(rx, claims::STR_LINEAR_001)
}

/// Block periodization phase (File 02 strength-022 verbatim bands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockPhase {
    /// Accumulation: ~65–80%, 3–5×6–10, build work capacity.
    Accumulation,
    /// Transmutation: ~80–90%, 3–6×3–6, sport-specific.
    Transmutation,
    /// Realization: lowest volume, peak/taper, 1–3 reps (intensity/sets
    /// unspecified in the source, deferred to the taper template).
    Realization,
}

/// Block periodization phase prescription (File 02 strength-022); 2–4 wk
/// concentrated blocks. Realization gives only reps in the source. Contested
/// (global CQ-03, model superiority: no meta-analysis confirms block >
/// traditional). STR-BLOCK-001.
pub fn block_phase_rx(phase: BlockPhase) -> Recommended<PhaseRx> {
    let rx = match phase {
        BlockPhase::Accumulation => PhaseRx {
            pct_1rm: Some((65, 80)),
            pct_top_open: false,
            sets: Some((3, 5)),
            reps: Some((6, 10)),
            weeks: (2, 4),
        },
        BlockPhase::Transmutation => PhaseRx {
            pct_1rm: Some((80, 90)),
            pct_top_open: false,
            sets: Some((3, 6)),
            reps: Some((3, 6)),
            weeks: (2, 4),
        },
        BlockPhase::Realization => PhaseRx {
            pct_1rm: None,
            pct_top_open: false,
            sets: None,
            reps: Some((1, 3)),
            weeks: (2, 4),
        },
    };
    graded(rx, claims::STR_BLOCK_001)
}

// ---------------------------------------------------------------------------
// 13. Depth-jump readiness gate (File 02 strength-033; SAFETY, ExpertOpinion)
// ---------------------------------------------------------------------------

/// Back-squat 1RM, as a multiple of bodyweight, required before high-intensity
/// depth jumps (File 02 strength-033; PLYO-PREREQ-001).
pub const DEPTH_JUMP_SQUAT_BW_RATIO: f64 = 1.5;

/// SAFETY gate: require a ~[`DEPTH_JUMP_SQUAT_BW_RATIO`]× bodyweight back-squat
/// before high-intensity depth jumps (File 02 strength-033; claim
/// PLYO-PREREQ-001). ExpertOpinion prerequisite, contested, but
/// `safety_critical`, landing loads are injurious without the
/// strength/mechanics base. Also requires landing-mechanics competence, which
/// this numeric gate does not capture; callers must verify it separately.
pub fn depth_jump_ready(squat_1rm: f64, bodyweight: f64) -> Recommended<bool> {
    let ready = bodyweight > 0.0 && squat_1rm >= DEPTH_JUMP_SQUAT_BW_RATIO * bodyweight;
    graded(ready, claims::PLYO_PREREQ_001)
}

// ---------------------------------------------------------------------------
// 14. e1RM reliability gate + cross-check (File 02 strength-006)
// ---------------------------------------------------------------------------

/// Rep count above which estimated 1RM is unreliable (File 02 strength-006).
pub const E1RM_RELIABLE_REP_CAP: u32 = 10;

/// Whether an estimated 1RM from this set is reliable (File 02 strength-006):
/// unreliable above 10 reps or on isolation lifts. STR-E1RM-CHECK-001
/// (Moderate; DiStasio 2014).
pub fn e1rm_reliable(reps: u32, isolation_lift: bool) -> Recommended<bool> {
    graded(
        (1..=E1RM_RELIABLE_REP_CAP).contains(&reps) && !isolation_lift,
        claims::STR_E1RM_CHECK_001,
    )
}

/// A cross-checked e1RM: agreement band across >=2 formulas (File 02
/// strength-006).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct E1rmCrossCheck {
    /// Lowest estimate across the formulas.
    pub low_kg: f64,
    /// Highest estimate across the formulas.
    pub high_kg: f64,
    /// Number of formulas used (at least two, File 02 strength-006).
    pub formulas_used: u8,
}

/// Cross-check estimated 1RM across >=2 formulas (Epley, Brzycki, Lombardi;
/// File 02 strength-005/006) at the set's EFFECTIVE reps ([`effective_reps_f`]:
/// reps performed plus a trusted reserve), the same reps the headline
/// [`e1rm_epley_rpe`] uses, so the headline always sits inside its own band.
/// Returns `None` when the estimate is unreliable per strength-006 (no reps,
/// more than 10 effective reps, or an isolation lift), the one gate
/// [`e1rm_reliable`] every anchor uses; callers should then prefer a 3-6 rep
/// test set ([`e1rm_test_set_reps`]). STR-E1RM-CHECK-001 (Moderate; DiStasio
/// 2014).
pub fn e1rm_cross_check(
    weight: f64,
    reps: u32,
    rpe: f64,
    isolation_lift: bool,
) -> Recommended<Option<E1rmCrossCheck>> {
    let reliable = reps >= 1 && e1rm_reliable(effective_reps(reps, rpe), isolation_lift).value;
    let check = if reliable && weight.is_finite() && weight > 0.0 {
        let eff = effective_reps_f(reps, rpe);
        let estimates = [
            e1rm_epley_frac(weight, eff),
            // Brzycki and Lombardi at the same fractional reps (strength-005);
            // the gate keeps `eff` below 11, well inside Brzycki's domain.
            weight * 36.0 / (37.0 - eff),
            weight * eff.powf(0.10),
        ];
        let low = estimates.iter().copied().fold(f64::INFINITY, f64::min);
        let high = estimates.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Some(E1rmCrossCheck {
            low_kg: low,
            high_kg: high,
            formulas_used: estimates.len() as u8,
        })
    } else {
        None
    };
    graded(check, claims::STR_E1RM_CHECK_001)
}

/// Preferred test-set rep range when e1RM is unreliable (File 02 strength-006:
/// "prefer 3-6 rep test sets"). STR-E1RM-CHECK-001.
pub fn e1rm_test_set_reps() -> Recommended<(u8, u8)> {
    graded((3, 6), claims::STR_E1RM_CHECK_001)
}

// ---------------------------------------------------------------------------
// 15. DUP day parameters (File 02 strength-023; contested CQ-03)
// ---------------------------------------------------------------------------

/// A DUP training day's focus (File 02 strength-023 example week).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DupDay {
    /// Heavy: 3-5×3-5 @85-90%.
    Heavy,
    /// Power/speed: 3-6×3-5 @50-70%, fast (max concentric intent).
    Power,
    /// Hypertrophy: 3-4×8-12 @70-75%.
    Hypertrophy,
    /// Second heavy exposure of a max-strength (or strength-base power) week:
    /// the block Transmutation band's low end, 80 % ramping inside 80–90 %,
    /// 3–6 sets × 5–6 reps (File 02 strength-022, STR-BLOCK-001). 80 %×5 sits at
    /// ~2.5 reps in reserve; the ramp stops at 83 %, where 5 reps still leave 1
    /// (the 1–3 RIR strength band, STR-INTENT-001; [`pct_for_week`]).
    /// It exists so a strength goal gets ≥80 % 1RM on each main lift in ≥2
    /// sessions a week (STR-HEAVYFREQ-001).
    HeavyVolume,
}

/// One DUP day's loading parameters (File 02 strength-023, verbatim example
/// week).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DupDayRx {
    /// Working intensity %1RM (min, max).
    pub pct_1rm: (u8, u8),
    /// Sets (min, max).
    pub sets: (u8, u8),
    /// Reps per set (min, max).
    pub reps: (u8, u8),
    /// Maximal concentric velocity intent ("fast"; power day only).
    pub max_velocity_intent: bool,
}

/// DUP day prescription (File 02 strength-023: "Mon heavy 3–5×3–5 @85–90%;
/// Wed power/speed 3–6×3–5 @50–70% fast; Fri hypertrophy 3–4×8–12 @70–75%").
/// Best supported for intermediate/advanced training a lift >=2-3×/wk
/// (strength-023). Contested (CQ-03 model superiority; Harries/Grgic
/// meta-analyses find no consistent DUP advantage).
/// STR-DUP-001 (Moderate, mixed; Rhea 2002).
pub fn dup_day_rx(day: DupDay) -> Recommended<DupDayRx> {
    let rx = match day {
        DupDay::Heavy => DupDayRx {
            pct_1rm: (85, 90),
            sets: (3, 5),
            reps: (3, 5),
            max_velocity_intent: false,
        },
        DupDay::Power => DupDayRx {
            pct_1rm: (50, 70),
            sets: (3, 6),
            reps: (3, 5),
            max_velocity_intent: true,
        },
        DupDay::Hypertrophy => DupDayRx {
            pct_1rm: (70, 75),
            sets: (3, 4),
            reps: (8, 12),
            max_velocity_intent: false,
        },
        DupDay::HeavyVolume => DupDayRx {
            pct_1rm: (80, 90),
            sets: (3, 6),
            reps: (5, 6),
            max_velocity_intent: false,
        },
    };
    let claim_id = match day {
        DupDay::HeavyVolume => claims::STR_BLOCK_001,
        DupDay::Heavy | DupDay::Power | DupDay::Hypertrophy => claims::STR_DUP_001,
    };
    graded(rx, claim_id)
}

/// Fewest reps in reserve a %1RM strength prescription may imply: the floor of
/// the 1–3 RIR strength band, "rarely to failure" (STR-INTENT-001), and never
/// failure on heavy free-weight compounds (HYP-FAIL-SAFE-001).
pub const MIN_STRENGTH_RIR: u8 = 1;

/// Highest whole %1RM at which `reps` still leave `rir` reps in reserve on the
/// Epley model the engine reads effort from ([`pct_1rm_at_effective_reps`] at
/// `reps + rir`, strength-005, the inverse of [`expected_rir_at_pct`]), floored
/// so the reserve is never rounded away: 5 reps @ 1 RIR → 83 %, 3 @ 1 → 88 %.
pub fn max_pct_leaving_rir(reps: u8, rir: u8) -> u8 {
    pct_1rm_at_effective_reps(f64::from(reps) + f64::from(rir)).floor() as u8
}

/// This week's %1RM inside a day's cited band, progressed across the load
/// weeks of a block (File 02 strength-014, STR-PCTPROG-001: +2.5 % load per
/// successful week for the lower body, +1 % for the upper body — the low,
/// conservative end of each band). Week 1 opens at the band low; each later
/// week multiplies by `(1 + inc)`, clamped to the band high, so a compliant
/// lifter's load rises week over week instead of repeating week 1.
///
/// The rep count is held, so the ramp is also capped where the prescribed
/// `reps` would leave fewer than [`MIN_STRENGTH_RIR`] in reserve
/// ([`max_pct_leaving_rir`]): the band top is never paired with the rep top
/// (strength-022 bands, STR-INTENT-001). Lower body: Heavy ×3
/// 85→87→88→88, HeavyVolume ×5 80→82→83→83, Power ×3 50→51→53→54.
pub fn pct_for_week(
    band: (u8, u8),
    week_in_block: u8,
    upper_body: bool,
    reps: u8,
) -> Recommended<u8> {
    let inc = weekly_pct_increment(upper_body).value.0;
    let steps = week_in_block.max(1) as i32 - 1;
    let pct = (band.0 as f64 * (1.0 + inc).powi(steps))
        .round()
        .clamp(band.0 as f64, band.1 as f64) as u8;
    graded(
        pct.min(max_pct_leaving_rir(reps, MIN_STRENGTH_RIR)),
        claims::STR_PCTPROG_001,
    )
}

/// Working-load credit for a lift anchor whose movement has not been trained
/// for `days_since_trained` days. Full credit inside 30 days (normal training
/// gaps); past that the resistance re-entry bracket for that many weeks off
/// (REENTRY-001, File 08 Table 3.4b: 4–8 wk → 70 %); past 8 weeks `None` — the
/// KB gives no fraction and directs a fresh, novice-style start, so the anchor
/// is dropped. Strength losses over 1–2 months are modest, never a reset to
/// zero (DETRAIN-STR-001, Bosquet 2013), which is why this never tapers to 0.
pub fn lift_detrain_credit(days_since_trained: i64) -> Recommended<Option<f64>> {
    const FULL_CREDIT_DAYS: i64 = 30;
    if days_since_trained <= FULL_CREDIT_DAYS {
        return graded(Some(1.0), claims::DETRAIN_STR_001);
    }
    let re = crate::individualization::resistance_reentry(days_since_trained as f64 / 7.0).value;
    graded(
        if re.treat_as_novice {
            None
        } else {
            re.load_frac
        },
        claims::DETRAIN_STR_001,
    )
}

// ---------------------------------------------------------------------------
// 16. 1RM testing readiness gate (File 02 strength-040; SAFETY)
// ---------------------------------------------------------------------------

/// Readiness context for a 1RM test attempt (File 02 strength-040).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OneRmTestContext {
    /// Technically proficient in the lift.
    pub technically_proficient: bool,
    /// Adequately recovered.
    pub adequately_recovered: bool,
    /// Warmed up.
    pub warmed_up: bool,
    /// Athlete is a novice (novices must be supervised; prefer estimated 1RM
    /// early on).
    pub is_novice: bool,
    /// Supervision is present.
    pub supervised: bool,
    /// The lift loads the spine (squat/deadlift class).
    pub spinal_loading: bool,
    /// Bracing competence is established (required for spinal loading).
    pub bracing_competent: bool,
}

/// SAFETY gate: whether a 1RM test may proceed (File 02 strength-040). Test
/// 1RM only when technically proficient, adequately recovered, and warmed up;
/// novices only under supervision (and should prefer ESTIMATED 1RM early on -
/// see [`e1rm_cross_check`]); spinal loading requires bracing competence.
/// STR-1RMTEST-001 (ExpertOpinion/Moderate, registered conservatively;
/// NSCA; safety-critical).
pub fn one_rm_test_allowed(ctx: OneRmTestContext) -> Recommended<bool> {
    let allowed = ctx.technically_proficient
        && ctx.adequately_recovered
        && ctx.warmed_up
        && (!ctx.is_novice || ctx.supervised)
        && (!ctx.spinal_loading || ctx.bracing_competent);
    graded(allowed, claims::STR_1RMTEST_001)
}

/// Novice load-jump cap between 1RM-test attempts / progression steps, as a
/// fraction of load (File 02 strength-040): upper-body 2.5-5%, lower-body
/// 5-10%. Returns (min, max). STR-1RMTEST-001 (safety-critical).
pub fn novice_load_jump_cap_frac(upper_body: bool) -> Recommended<(f64, f64)> {
    let cap = if upper_body {
        (0.025, 0.05)
    } else {
        (0.05, 0.10)
    };
    graded(cap, claims::STR_1RMTEST_001)
}

#[cfg(test)]
mod tests;
