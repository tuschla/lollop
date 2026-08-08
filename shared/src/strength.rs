//! Strength & power calculators (knowledge-base File 02, Strength & Power
//! Resistance-Training Programming Rules).
//!
//! Pure, deterministic math: e1RM estimators, RPE/RIR mapping, reps→%1RM
//! estimation, and Prilepin volume governance. No IO. Values that make a
//! prescriptive recommendation are wrapped in [`Recommended`] via
//! [`recommend`], which forces attached evidence + confidence.
//!
//! All numbers transcribed verbatim from File 02 (rules strength-005,
//! strength-007, strength-011 and the Prilepin's-chart verbatim table). Claim
//! ids come from the canonical registry in `crate::evidence`.

use crate::evidence::graded;
use crate::schema::Recommended;

// ---------------------------------------------------------------------------
// 1. Estimated 1RM (File 02 strength-005)
// ---------------------------------------------------------------------------

/// Epley estimated 1RM: `weight * (1 + reps/30)` (File 02 strength-005).
/// Domain: reps >= 1 (the e1RM formulas are undefined below one rep).
pub fn e1rm_epley(weight: f64, reps: u32) -> f64 {
    debug_assert!(reps >= 1, "e1RM formulas are defined for reps >= 1");
    weight * (1.0 + reps as f64 / 30.0)
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
/// ([`BRZYCKI_MAX_REPS`]) so the return is always finite and positive; the
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
/// registry AUTOREG-RIR-001). Clamped at 10.
pub fn rir_to_rpe(rir: f64) -> f64 {
    let rpe = RPE_RIR_ANCHOR - rir;
    if rpe > RPE_RIR_ANCHOR { RPE_RIR_ANCHOR } else { rpe }
}

// ---------------------------------------------------------------------------
// 3. Reps → %1RM estimate (File 02 strength-005, Epley inverse)
// ---------------------------------------------------------------------------

/// Estimated %1RM for a maximal set of `reps`, as the Epley inverse
/// `100 / (1 + reps/30)` (File 02 strength-005). ESTIMATE ONLY: accurate at
/// 2–10 reps (±5%), diverges ±15–20% above 10 reps (strength-005/006).
pub fn est_pct_1rm_from_reps(reps: u32) -> f64 {
    100.0 / (1.0 + reps as f64 / 30.0)
}

// ---------------------------------------------------------------------------
// 4. Prilepin's chart (File 02 strength-011, verbatim table)
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
    graded(ok, "STR-PRILEPIN-001")
}

// ---------------------------------------------------------------------------
// 5. Loading prescription bands (File 02 strength-001/002/003; STR-INTENT-001)
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
    /// Reps in reserve target (min, max); power stops before velocity decay.
    pub rir: (u8, u8),
}

/// Loading bands by goal (File 02 strength-001/002/003). Strength 80-100% /
/// 1-5 / 1-3 RIR (STR-INTENT-001, Strong); power a velocity-biased spectrum
/// stopped well short of failure (strength-002 → STR-PWR-001, Moderate);
/// hypertrophy 65-85% / 6-12 / 0-3 RIR.
///
/// The Power band is the ENVELOPE of the KB's load spectrum (0-60%
/// ballistic/jump; 30-70% loaded power; 70-95% weightlifting pulls → 0-95%
/// overall). Per-exercise-class bands come from [`power_load_spectrum`]
/// (strength-030); never prescribe the whole envelope to one exercise. The
/// KB states only "never to failure; high RIR" for power: the numeric `rir`
/// band here is an expert-opinion encoding of "high RIR" (claim STR-PWR-RIR-001),
/// not a KB number.
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
            rir: (1, 3),
        },
        LiftGoal::Power => LoadingRx {
            pct_1rm: (0, 95),
            reps: (1, 5),
            sets: (3, 6),
            rest_sec: (180, 300),
            rir: (3, 5),
        },
        LiftGoal::Hypertrophy => LoadingRx {
            pct_1rm: (65, 85),
            reps: (6, 12),
            sets: (3, 6),
            rest_sec: (30, 120),
            rir: (0, 3),
        },
    };
    let claim_id = match goal {
        LiftGoal::Power => "STR-PWR-001",
        LiftGoal::MaxStrength | LiftGoal::Hypertrophy => "STR-INTENT-001",
    };
    graded(rx, claim_id)
}

// ---------------------------------------------------------------------------
// 6. Load progression (File 02 strength-012/014; STR-2FOR2-001 / STR-PCTPROG-001)
// ---------------------------------------------------------------------------

/// 2-for-2 rule (File 02 strength-012): increase load once the athlete beats the
/// goal by >=2 reps on the last set in 2 consecutive sessions. STR-2FOR2-001
/// (safety-critical per the KB: caps how fast load may ramp).
pub fn two_for_two_met(reps_over_goal_last_set: u8, consecutive_sessions: u8) -> Recommended<bool> {
    graded(reps_over_goal_last_set >= 2 && consecutive_sessions >= 2,
    "STR-2FOR2-001",)
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
    graded(inc, "STR-PCTPROG-001")
}

/// Whether a stall triggers a deload / model switch (File 02 strength-039):
/// (estimated) 1RM flat for >=2 weeks despite adequate recovery. STR-STALL-001.
pub fn stall_triggers_deload(weeks_stalled: u8, recovery_adequate: bool) -> Recommended<bool> {
    graded(weeks_stalled >= 2 && recovery_adequate, "STR-STALL-001")
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
    graded(model, "STR-MODEL-001")
}

// ---------------------------------------------------------------------------
// 7. Taper (File 02 strength-026/027; TAPER-001)
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

/// Best-evidenced peaking taper: cut volume 41-60% exponentially over ~2 weeks
/// while holding intensity and frequency (File 02 strength-026; TAPER-001).
pub fn taper_rx() -> Recommended<TaperRx> {
    graded(TaperRx {
        volume_reduction_frac: (0.41, 0.60),
        duration_days: (8, 14),
        hold_intensity: true,
    },
    "TAPER-001",)
}

// ---------------------------------------------------------------------------
// 8. Plyometric volume caps (File 02 strength-032; PLYO-001)
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
    graded(cap, "PLYO-001")
}

// ---------------------------------------------------------------------------
// 9. Power/peaking specifics (File 02 strength-029/031/034)
// ---------------------------------------------------------------------------

/// Days before a test/meet to schedule the last true near-max deadlift, given
/// its high systemic fatigue cost (File 02 strength-029). Returns (min, max)
/// days out. STR-DLPEAK-001 (safety-critical per the KB).
pub fn deadlift_peak_days_out() -> Recommended<(u8, u8)> {
    graded((10, 14), "STR-DLPEAK-001")
}

/// The conditioning activity opening a PAP/PAPE contrast pair (File 02
/// strength-034). Rest windows differ by CA type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditioningActivity {
    /// Heavy lift CA, e.g. back squat 3-5RM (85-90%).
    HeavyLift,
    /// Plyometric CA (potentiates much earlier).
    Plyometric,
}

/// PAP/PAPE contrast rest window in minutes for a HEAVY conditioning activity
/// (File 02 strength-034): ~5-7 min (>=5; overall optimal window ~3-7 min).
/// Stronger athletes potentiate more and earlier; abort if the explosive set
/// is slower than baseline. STR-PAP-001 (Moderate; Seitz & Haff). For the
/// plyometric-CA window use [`pap_rest_window_min_for`].
pub fn pap_rest_window_min() -> Recommended<(u8, u8)> {
    graded((5, 7), "STR-PAP-001")
}

/// PAP/PAPE contrast rest window in minutes by conditioning-activity type
/// (File 02 strength-034): heavy CAs ~5-7 min (>=5), plyometric CAs ~0.3-4
/// min. STR-PAP-001 (Moderate; Seitz & Haff 2016).
pub fn pap_rest_window_min_for(ca: ConditioningActivity) -> Recommended<(f64, f64)> {
    let window = match ca {
        ConditioningActivity::HeavyLift => (5.0, 7.0),
        ConditioningActivity::Plyometric => (0.3, 4.0),
    };
    graded(window, "STR-PAP-001")
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
    graded(OlympicDerivativeRx {
        pct_1rm: (85, 100),
        pct_top_open: true,
        reps: (1, 3),
        sets: (3, 5),
        rest_sec: (180, 300),
        early_in_session: true,
        velocity_variant_pcts_1rm: &[30, 45, 65, 80],
        jump_shrug_pct_bodymass: 30,
    },
    "STR-OLY-001",)
}

// ---------------------------------------------------------------------------
// 10. Lombardi e1RM (File 02 strength-005, third estimator)
// ---------------------------------------------------------------------------

/// Lombardi estimated 1RM: `weight * reps^0.10` (File 02 strength-005). Same
/// 1–10 rep accuracy window (±5%) as Epley/Brzycki; treat as approximate.
/// Domain: reps >= 1 (the e1RM formulas are undefined below one rep).
pub fn e1rm_lombardi(weight: f64, reps: u32) -> f64 {
    debug_assert!(reps >= 1, "e1RM formulas are defined for reps >= 1");
    weight * (reps as f64).powf(0.10)
}

// ---------------------------------------------------------------------------
// 11. RPE-anchored top set + back-offs (File 02 strength-015; STR-BACKOFF-001)
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
    graded(BackOffRx {
        top_set_rpe: 8,
        drop_frac: (0.10, 0.15),
    },
    "STR-BACKOFF-001",)
}

// ---------------------------------------------------------------------------
// 12. Velocity-loss set termination by goal (File 02 strength-018; AUTOREG-VL-001)
// ---------------------------------------------------------------------------

/// Velocity-loss fraction at which to terminate the set for a given goal (File
/// 02 strength-018): ≥20% VL for strength/power (preserves CMJ/1RM), ≥40% VL
/// permitted for hypertrophy. AUTOREG-VL-001.
pub fn vl_termination_threshold(goal: LiftGoal) -> Recommended<f64> {
    let vl = match goal {
        LiftGoal::MaxStrength | LiftGoal::Power => 0.20,
        LiftGoal::Hypertrophy => 0.40,
    };
    graded(vl, "AUTOREG-VL-001")
}

// ---------------------------------------------------------------------------
// 13. Periodization phase tables (File 02 strength-021 linear / 022 block)
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
    graded(rx, "STR-LINEAR-001")
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
    graded(rx, "STR-BLOCK-001")
}

// ---------------------------------------------------------------------------
// 14. Depth-jump readiness gate (File 02 strength-033; SAFETY, ExpertOpinion)
// ---------------------------------------------------------------------------

/// SAFETY gate: require a ~1.5× bodyweight back-squat before high-intensity
/// depth jumps (File 02 strength-033; claim PLYO-PREREQ-001). ExpertOpinion
/// prerequisite, contested (CQ-F02-05), but `safety_critical`, landing loads
/// are injurious without the strength/mechanics base. Also requires
/// landing-mechanics competence, which this numeric gate does not capture;
/// callers must verify it separately.
pub fn depth_jump_ready(squat_1rm: f64, bodyweight: f64) -> Recommended<bool> {
    let ready = bodyweight > 0.0 && squat_1rm >= 1.5 * bodyweight;
    graded(ready, "PLYO-PREREQ-001")
}

// ---------------------------------------------------------------------------
// 15. e1RM reliability gate + cross-check (File 02 strength-006)
// ---------------------------------------------------------------------------

/// Rep count above which estimated 1RM is unreliable (File 02 strength-006).
pub const E1RM_RELIABLE_REP_CAP: u32 = 10;

/// Minimum number of e1RM formulas to cross-check (File 02 strength-006).
pub const E1RM_MIN_CROSS_CHECK_FORMULAS: usize = 2;

/// Whether an estimated 1RM from this set is reliable (File 02 strength-006):
/// unreliable above 10 reps or on isolation lifts. STR-E1RM-CHECK-001
/// (Moderate; DiStasio 2014).
pub fn e1rm_reliable(reps: u32, isolation_lift: bool) -> Recommended<bool> {
    graded(reps >= 1 && reps <= E1RM_RELIABLE_REP_CAP && !isolation_lift,
    "STR-E1RM-CHECK-001",)
}

/// A cross-checked e1RM: agreement band across >=2 formulas (File 02
/// strength-006).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct E1rmCrossCheck {
    /// Lowest estimate across the formulas.
    pub low_kg: f64,
    /// Highest estimate across the formulas.
    pub high_kg: f64,
    /// Number of formulas used (>= [`E1RM_MIN_CROSS_CHECK_FORMULAS`]).
    pub formulas_used: u8,
}

/// Cross-check estimated 1RM across >=2 formulas (Epley, Brzycki, Lombardi;
/// File 02 strength-005/006). Returns `None` when the estimate is unreliable
/// per strength-006 (0 reps, >10 reps, or an isolation lift), callers should
/// then prefer a 3-6 rep test set ([`e1rm_test_set_reps`]).
/// STR-E1RM-CHECK-001 (Moderate; DiStasio 2014).
pub fn e1rm_cross_check(
    weight: f64,
    reps: u32,
    isolation_lift: bool,
) -> Recommended<Option<E1rmCrossCheck>> {
    let reliable = reps >= 1 && reps <= E1RM_RELIABLE_REP_CAP && !isolation_lift;
    let check = if reliable && weight.is_finite() && weight > 0.0 {
        let estimates = [
            e1rm_epley(weight, reps),
            e1rm_brzycki(weight, reps),
            e1rm_lombardi(weight, reps),
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
    graded(check, "STR-E1RM-CHECK-001")
}

/// Preferred test-set rep range when e1RM is unreliable (File 02 strength-006:
/// "prefer 3-6 rep test sets"). STR-E1RM-CHECK-001.
pub fn e1rm_test_set_reps() -> Recommended<(u8, u8)> {
    graded((3, 6), "STR-E1RM-CHECK-001")
}

// ---------------------------------------------------------------------------
// 16. Fixed-% vs RPE/RIR selection (File 02 strength-008; contested CQ-F02-01)
// ---------------------------------------------------------------------------

/// How working load is prescribed (File 02 strength-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPrescriptionMode {
    /// Fixed percentage of (estimated) 1RM.
    FixedPercent,
    /// RPE/RIR-autoregulated loading.
    RpeRir,
}

/// Select the load-prescription mode (File 02 strength-008): fixed % for
/// novices, teaching, or when no monitoring is available; RPE/RIR
/// autoregulation for intermediate/advanced and fatigue-sensitive phases.
/// Contested (File 02 local CQ-01 → CQ-F02-01: both are effective, RPE holds a
/// small non-significant edge). STR-LOADSEL-001 (Moderate; Helms 2018).
pub fn load_prescription_mode(
    level: crate::individualization::TrainingAge,
    monitoring_available: bool,
) -> Recommended<LoadPrescriptionMode> {
    use crate::individualization::TrainingAge;
    let mode = if matches!(level, TrainingAge::Novice) || !monitoring_available {
        LoadPrescriptionMode::FixedPercent
    } else {
        LoadPrescriptionMode::RpeRir
    };
    graded(mode, "STR-LOADSEL-001")
}

// ---------------------------------------------------------------------------
// 17. Velocity zones + MVT (File 02 strength-016, verbatim zone table)
// ---------------------------------------------------------------------------

/// A velocity-based training quality (File 02 velocity-zone table, Mann /
/// Jovanovic-Flanagan continuum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VelocityZone {
    /// Absolute/max strength: 0.15-0.50 m/s, ~90-100% 1RM.
    AbsoluteStrength,
    /// Accelerative/strength: 0.50-0.75 m/s, ~80-90% 1RM.
    AccelerativeStrength,
    /// Strength-speed: 0.75-1.00 m/s, ~55-80% 1RM.
    StrengthSpeed,
    /// Speed-strength: 1.00-1.30 m/s, ~30-55% 1RM.
    SpeedStrength,
    /// Speed: >1.30 m/s, <30% 1RM.
    Speed,
}

/// One row of the velocity-zone table (File 02, verbatim; "approximate; zone
/// boundaries individual").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VelocityZoneRow {
    pub zone: VelocityZone,
    /// Bar speed in m/s (lower bound, upper bound). `None` upper = open
    /// (">1.30 m/s" for Speed).
    pub bar_speed_ms: (f64, Option<f64>),
    /// Typical %1RM (lower, upper). `None` lower = open ("<30%" for Speed).
    pub pct_1rm: (Option<u8>, Option<u8>),
}

/// Velocity-zone table, transcribed verbatim from File 02 (strength-016).
pub static VELOCITY_ZONES: &[VelocityZoneRow] = &[
    VelocityZoneRow {
        zone: VelocityZone::AbsoluteStrength,
        bar_speed_ms: (0.15, Some(0.50)),
        pct_1rm: (Some(90), Some(100)),
    },
    VelocityZoneRow {
        zone: VelocityZone::AccelerativeStrength,
        bar_speed_ms: (0.50, Some(0.75)),
        pct_1rm: (Some(80), Some(90)),
    },
    VelocityZoneRow {
        zone: VelocityZone::StrengthSpeed,
        bar_speed_ms: (0.75, Some(1.00)),
        pct_1rm: (Some(55), Some(80)),
    },
    VelocityZoneRow {
        zone: VelocityZone::SpeedStrength,
        bar_speed_ms: (1.00, Some(1.30)),
        pct_1rm: (Some(30), Some(55)),
    },
    VelocityZoneRow {
        zone: VelocityZone::Speed,
        bar_speed_ms: (1.30, None),
        pct_1rm: (None, Some(30)),
    },
];

/// Look up the velocity-zone row for a training quality (File 02 strength-016).
/// Zone boundaries are approximate and individual, set zones from the
/// athlete's own load-velocity relationship where possible (MCV maps inversely
/// and near-perfectly to %1RM, R² ≈0.98 bench). STR-VZONE-001 (Moderate;
/// González-Badillo & Sánchez-Medina 2010).
pub fn velocity_zone_rx(zone: VelocityZone) -> Recommended<VelocityZoneRow> {
    let row = *VELOCITY_ZONES
        .iter()
        .find(|r| r.zone == zone)
        .expect("every VelocityZone has a table row");
    graded(row, "STR-VZONE-001")
}

/// A lift with a KB-stated minimum velocity threshold (File 02 strength-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MvtLift {
    /// Bench press: MVT ~0.15 m/s (0.16 ± 0.04).
    BenchPress,
    /// Back squat: MVT ~0.30 m/s.
    BackSquat,
}

/// Minimum velocity threshold in m/s (File 02 strength-016, verbatim: "~0.15
/// m/s bench (0.16 ± 0.04), ~0.30 m/s squat"). STR-VZONE-001.
pub fn mvt_ms(lift: MvtLift) -> Recommended<f64> {
    let mvt = match lift {
        MvtLift::BenchPress => 0.15,
        MvtLift::BackSquat => 0.30,
    };
    graded(mvt, "STR-VZONE-001")
}

// ---------------------------------------------------------------------------
// 18. Load-velocity-profile e1RM (File 02 strength-017; HARD deadlift guard)
// ---------------------------------------------------------------------------

/// Lifts for load-velocity-profile 1RM estimation (File 02 strength-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LvpLift {
    BenchPress,
    BackSquat,
    /// HARD guard: deadlift LVP must NOT predict 1RM (Lake et al.) -
    /// [`lvp_e1rm`] always returns `None` for it.
    Deadlift,
}

/// Standard error of the LVP 1RM estimate, as % of 1RM (File 02 strength-017:
/// SEE ≈9.8%, monitoring only, never a testing substitute).
pub const LVP_SEE_PCT_1RM: f64 = 9.8;

/// Incremental loads in the velocity profile (min, max): 4-7, with 5-7
/// recommended (File 02 strength-017).
pub const LVP_PROFILE_LOADS: (u8, u8) = (4, 7);

/// Recommended incremental-load count (min, max) within [`LVP_PROFILE_LOADS`].
pub const LVP_PROFILE_LOADS_PREFERRED: (u8, u8) = (5, 7);

/// Estimate 1RM from an incremental-load velocity profile extrapolated to the
/// lift's MVT (File 02 strength-017). `points` are `(load_kg, mean_concentric
/// _velocity_ms)` pairs from the profiling session.
///
/// Returns `None` when the estimate is not permitted or degenerate:
/// - HARD guard: `LvpLift::Deadlift` NEVER yields an estimate (Lake et al.:
///   deadlift LVP must not predict 1RM);
/// - fewer than 4 profile points (KB: 4-7 loads, recommend 5-7);
/// - a non-negative load-velocity slope or a non-finite/non-positive
///   extrapolation (no valid inverse relationship).
///
/// MONITORING ONLY: SEE ≈9.8% of 1RM ([`LVP_SEE_PCT_1RM`]), never use as a
/// tested max. STR-LVP-001 (Moderate; Jovanovic & Flanagan; Greig 2023).
pub fn lvp_e1rm(lift: LvpLift, points: &[(f64, f64)]) -> Recommended<Option<f64>> {
    let estimate = lvp_e1rm_inner(lift, points);
    graded(estimate, "STR-LVP-001")
}

fn lvp_e1rm_inner(lift: LvpLift, points: &[(f64, f64)]) -> Option<f64> {
    // HARD guard (strength-017 parameters, verbatim): "deadlift LVP MUST NOT
    // predict 1RM".
    let mvt = match lift {
        LvpLift::Deadlift => return None,
        LvpLift::BenchPress => 0.15,
        LvpLift::BackSquat => 0.30,
    };
    if points.len() < LVP_PROFILE_LOADS.0 as usize {
        return None;
    }
    if points
        .iter()
        .any(|(l, v)| !l.is_finite() || !v.is_finite() || *l <= 0.0)
    {
        return None;
    }
    // Least-squares fit velocity = a + b·load, then extrapolate to MVT.
    let n = points.len() as f64;
    let mean_l = points.iter().map(|(l, _)| l).sum::<f64>() / n;
    let mean_v = points.iter().map(|(_, v)| v).sum::<f64>() / n;
    let sxx = points.iter().map(|(l, _)| (l - mean_l).powi(2)).sum::<f64>();
    let sxy = points
        .iter()
        .map(|(l, v)| (l - mean_l) * (v - mean_v))
        .sum::<f64>();
    if sxx == 0.0 {
        return None;
    }
    let b = sxy / sxx;
    // Velocity must fall as load rises; otherwise the profile is invalid.
    if b >= 0.0 {
        return None;
    }
    let a = mean_v - b * mean_l;
    let e1rm = (mvt - a) / b;
    if !e1rm.is_finite() || e1rm <= 0.0 {
        return None;
    }
    Some(e1rm)
}

// ---------------------------------------------------------------------------
// 19. First-rep bar-speed daily autoregulation (File 02 strength-019)
// ---------------------------------------------------------------------------

/// Daily load adjustment from first-rep bar speed (File 02 strength-019).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DailyLoadAdjustment {
    /// First-rep velocity above the target-zone upper bound → add load.
    Increase,
    /// Within the target zone → keep the planned load.
    Hold,
    /// Below the target-zone lower bound → reduce load.
    Decrease,
}

/// Autoregulate today's load from the first rep's mean concentric velocity
/// against the target zone (File 02 strength-019): add load if the first-rep
/// velocity exceeds the zone's upper bound, reduce it if below the lower
/// bound. Rationale: true 1RM varies ±18% (≈36% total) day-to-day, so fixed
/// percentages chase a moving target. The open-topped Speed zone can never
/// trigger an increase. STR-VBT-DAILY-001 (Moderate; Jovanović & Flanagan
/// 2014).
pub fn first_rep_load_adjustment(
    first_rep_mcv_ms: f64,
    target: VelocityZone,
) -> Recommended<DailyLoadAdjustment> {
    let row = VELOCITY_ZONES
        .iter()
        .find(|r| r.zone == target)
        .expect("every VelocityZone has a table row");
    let (lo, hi) = row.bar_speed_ms;
    let adj = if first_rep_mcv_ms < lo {
        // Bar slower than the zone floor: the load is too heavy today.
        DailyLoadAdjustment::Decrease
    } else if matches!(hi, Some(h) if first_rep_mcv_ms > h) {
        // Bar faster than the zone ceiling: the load is too light today.
        DailyLoadAdjustment::Increase
    } else {
        DailyLoadAdjustment::Hold
    };
    graded(adj, "STR-VBT-DAILY-001")
}

// ---------------------------------------------------------------------------
// 20. DUP day parameters (File 02 strength-023; contested CQ-03)
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
/// ([`dup_lift_frequency_per_week`]). Contested (CQ-03 model superiority;
/// Harries/Grgic meta-analyses find no consistent DUP advantage).
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
    };
    graded(rx, "STR-DUP-001")
}

/// Per-lift weekly frequency DUP is best supported at (File 02 strength-023:
/// ">=2-3×/wk"); (min, max) of the KB band, min being the floor. STR-DUP-001.
pub fn dup_lift_frequency_per_week() -> Recommended<(u8, u8)> {
    graded((2, 3), "STR-DUP-001")
}

// ---------------------------------------------------------------------------
// 21. Conjugate/Westside parameters (File 02 strength-024; SAFETY, advanced
//     only; contested CQ-F02-04)
// ---------------------------------------------------------------------------

/// Minimum barbell-training years before conjugate/Westside is offered
/// (File 02 strength-024: "advanced lifters (≥2–3 yr barbell training)").
/// 2.0 is the KB band's floor; strength-010 classes advanced as 3+ yr.
pub const CONJUGATE_MIN_TRAINING_YEARS: f64 = 2.0;

/// Conjugate/Westside structural parameters (File 02 strength-024 +
/// periodization-models section, verbatim).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConjugateRx {
    /// Training days per week.
    pub days_per_week: u8,
    /// Competition lifts as a fraction of total work (~20%).
    pub competition_lift_work_frac: f64,
    /// Accessories as a fraction of total work (~80%).
    pub accessory_work_frac: f64,
    /// Max Effort: work up to a 1-3RM at 90%+ (min %1RM; open-ended top).
    pub me_pct_1rm_min: u8,
    /// Max Effort rep-max window (1-3RM).
    pub me_rep_max: (u8, u8),
    /// Rotate the ME variation ~weekly for elite lifters.
    pub me_rotate_weeks_elite: u8,
    /// Rotate every 2-3 weeks for raw/intermediate lifters.
    pub me_rotate_weeks_raw: (u8, u8),
    /// Dynamic Effort squat/deadlift bar load %1RM (50-60%).
    pub de_squat_dl_pct_1rm: (u8, u8),
    /// Accommodating resistance on DE squat/DL: +20-25% band/chain.
    pub de_band_chain_pct: (u8, u8),
    /// Dynamic Effort bench bar load %1RM (~40-60%).
    pub de_bench_pct_1rm: (u8, u8),
    /// DE reps per set (1-3, fast).
    pub de_reps: (u8, u8),
    /// DE wave length in weeks (3-week pendulum).
    pub de_wave_weeks: u8,
}

/// Conjugate/Westside prescription, gated to advanced lifters (File 02
/// strength-024, SAFETY-critical gate): returns `None` below
/// [`CONJUGATE_MIN_TRAINING_YEARS`] of barbell training, the KB reserves the
/// model for advanced lifters (≥2-3 yr) and it must never be offered below
/// that. Contested (CQ-F02-04: strong practice record, weak controlled
/// evidence, little direct RCT support). STR-CONJ-001 (ExpertOpinion;
/// Simmons).
pub fn conjugate_rx(barbell_training_years: f64) -> Recommended<Option<ConjugateRx>> {
    let rx = if barbell_training_years.is_finite()
        && barbell_training_years >= CONJUGATE_MIN_TRAINING_YEARS
    {
        Some(ConjugateRx {
            days_per_week: 4,
            competition_lift_work_frac: 0.20,
            accessory_work_frac: 0.80,
            me_pct_1rm_min: 90,
            me_rep_max: (1, 3),
            me_rotate_weeks_elite: 1,
            me_rotate_weeks_raw: (2, 3),
            de_squat_dl_pct_1rm: (50, 60),
            de_band_chain_pct: (20, 25),
            de_bench_pct_1rm: (40, 60),
            de_reps: (1, 3),
            de_wave_weeks: 3,
        })
    } else {
        None
    };
    graded(rx, "STR-CONJ-001")
}

// ---------------------------------------------------------------------------
// 22. Wave loading (File 02 strength-025)
// ---------------------------------------------------------------------------

/// Wave-loading set structure (File 02 strength-025, KB example: "3-2-1/3-2-1
/// with load rising each wave").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveLoadingRx {
    /// Reps per set within one wave (KB example wave: 3, 2, 1).
    pub wave_reps: (u8, u8, u8),
    /// Number of waves in the KB example (3-2-1 / 3-2-1 = 2).
    pub waves: u8,
    /// Load rises each successive wave, exploiting acute potentiation.
    pub load_rises_per_wave: bool,
}

/// Wave loading (File 02 strength-025): ascending/descending load waves across
/// sets to exploit acute potentiation. The 3-2-1/3-2-1 structure is the KB's
/// example ("e.g."), not a fixed prescription. STR-WAVE-001
/// (ExpertOpinion/Weak; unstated citation).
pub fn wave_loading_rx() -> Recommended<WaveLoadingRx> {
    graded(WaveLoadingRx {
        wave_reps: (3, 2, 1),
        waves: 2,
        load_rises_per_wave: true,
    },
    "STR-WAVE-001",)
}

// ---------------------------------------------------------------------------
// 23. Strength peaking (File 02 strength-027)
// ---------------------------------------------------------------------------

/// Strength-peaking prescription (File 02 strength-027, verbatim parameters).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakingRx {
    /// Volume reduction (min, max) as fractions (30-70%; study VL range
    /// 31.6-67.0% over 7-28 days).
    pub volume_reduction_frac: (f64, f64),
    /// Maintain or slightly increase intensity: floor ≥85% 1RM (+5% may beat
    /// −10%).
    pub intensity_pct_1rm_min: u8,
    /// Taper duration in weeks (step or exponential), 1-2.
    pub taper_weeks: (u8, u8),
    /// Training cessation before the test/meet, 2-7 days.
    pub cessation_days: (u8, u8),
    /// Highly trained: keep frequency ≥80% of habitual (as a fraction).
    pub freq_frac_min_highly_trained: f64,
    /// Moderately trained: reduce frequency 30-50% (as fractions).
    pub freq_reduction_frac_moderately_trained: (f64, f64),
}

/// Strength peaking (File 02 strength-027): reduce volume 30-70%, maintain or
/// slightly increase intensity ≥85% 1RM, taper (step or exponential) over 1-2
/// weeks, then 2-7 days cessation. Complements the general Bosquet taper
/// ([`taper_rx`], strength-026) with strength-specific bands. STR-PEAK-001
/// (Moderate; Pritchard 2015, Travis 2020/2021).
pub fn peaking_rx() -> Recommended<PeakingRx> {
    graded(PeakingRx {
        volume_reduction_frac: (0.30, 0.70),
        intensity_pct_1rm_min: 85,
        taper_weeks: (1, 2),
        cessation_days: (2, 7),
        freq_frac_min_highly_trained: 0.80,
        freq_reduction_frac_moderately_trained: (0.30, 0.50),
    },
    "STR-PEAK-001",)
}

// ---------------------------------------------------------------------------
// 24. Power load spectrum per exercise class (File 02 strength-030)
// ---------------------------------------------------------------------------

/// Exercise classes on the power force-velocity spectrum (File 02
/// strength-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerExerciseClass {
    /// Jump squat: peak power at ~0% 1RM (body weight; MPV ~1.0 m/s).
    JumpSquat,
    /// Loaded squat power: >30-<70% 1RM.
    LoadedSquatPower,
    /// Power clean: ~40-80% 1RM.
    PowerClean,
    /// Weightlifting pulls: 90-95% 1RM.
    WeightliftingPulls,
}

/// Clean VARIATIONS peak power at ≥70% 1RM (File 02 strength-030 parameters:
/// "power clean 40-80% (≥70% for clean variations)").
pub const CLEAN_VARIATION_MIN_PCT_1RM: u8 = 70;

/// Power training load band %1RM (min, max) per exercise class (File 02
/// strength-030): train power across a load spectrum matched to
/// force-velocity needs, NOT a single optimal load, jump squat ~0%, loaded
/// squat power >30-<70%, power clean ~40-80% (clean variations ≥70%,
/// [`CLEAN_VARIATION_MIN_PCT_1RM`]), weightlifting pulls 90-95%.
/// STR-PWRSPEC-001 (Moderate; Cormie 2007/2011, Soriano 2015).
pub fn power_load_spectrum(class: PowerExerciseClass) -> Recommended<(u8, u8)> {
    let band = match class {
        PowerExerciseClass::JumpSquat => (0, 0),
        PowerExerciseClass::LoadedSquatPower => (30, 70),
        PowerExerciseClass::PowerClean => (40, 80),
        PowerExerciseClass::WeightliftingPulls => (90, 95),
    };
    graded(band, "STR-PWRSPEC-001")
}

// ---------------------------------------------------------------------------
// 25. Plyometric scheduling (File 02 strength-032 remainder; SAFETY)
// ---------------------------------------------------------------------------

/// Plyometric session scheduling and rest (File 02 strength-032 parameters;
/// the foot-contact caps live in [`plyo_foot_contact_cap`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlyoScheduleRx {
    /// Sessions per week (1-3).
    pub sessions_per_week: (u8, u8),
    /// Hours between plyometric sessions (48-72).
    pub session_spacing_hours: (u8, u8),
    /// Rest between sets in seconds (2-3 min).
    pub rest_between_sets_sec: (u16, u16),
    /// Depth jumps: work:rest up to ~1:10 (denominator of the ratio).
    pub depth_jump_work_rest_denominator: u8,
    /// Depth jumps: 5-10 s between reps.
    pub depth_jump_inter_rep_rest_sec: (u8, u8),
}

/// Plyometric scheduling (File 02 strength-032, SAFETY-critical rule):
/// 1-3 sessions/wk with 48-72 h between; rest 2-3 min/set, depth jumps up to
/// ~1:10 work:rest with 5-10 s between reps. Progress volume OR intensity,
/// never both (see [`plyo_foot_contact_cap`] for the contact caps).
/// STR-PLYO-SCHED-001 (Moderate; Potash & Chu 2008).
pub fn plyo_schedule_rx() -> Recommended<PlyoScheduleRx> {
    graded(PlyoScheduleRx {
        sessions_per_week: (1, 3),
        session_spacing_hours: (48, 72),
        rest_between_sets_sec: (120, 180),
        depth_jump_work_rest_denominator: 10,
        depth_jump_inter_rep_rest_sec: (5, 10),
    },
    "STR-PLYO-SCHED-001",)
}

// ---------------------------------------------------------------------------
// 26. Competition-lift anchoring (File 02 strength-035)
// ---------------------------------------------------------------------------

/// A barbell strength sport with defined competition lifts (File 02
/// strength-035).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrengthSport {
    /// Powerlifting: squat / bench press / deadlift.
    Powerlifting,
    /// Weightlifting: snatch / clean & jerk.
    Weightlifting,
}

/// The competition lifts that anchor strength-goal training (File 02
/// strength-035): the competition lift has the highest specificity -
/// adaptations are specific to trained movement, velocity, and ROM, and
/// over-specialized variations improve the variation, not the comp lift.
/// STR-COMP-ANCHOR-001 (Strong principle; Moderate magnitudes).
pub fn competition_lifts(sport: StrengthSport) -> Recommended<&'static [&'static str]> {
    let lifts: &'static [&'static str] = match sport {
        StrengthSport::Powerlifting => &["squat", "bench press", "deadlift"],
        StrengthSport::Weightlifting => &["snatch", "clean & jerk"],
    };
    graded(lifts, "STR-COMP-ANCHOR-001")
}

// ---------------------------------------------------------------------------
// 27. Variation selection (File 02 strength-036)
// ---------------------------------------------------------------------------

/// An axis along which a lift variation can bias a weak point (File 02
/// strength-036: "ROM, stance/grip, tempo, bar").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariationAxis {
    Rom,
    StanceGrip,
    Tempo,
    Bar,
}

/// The variation-bias axes the KB names (File 02 strength-036).
pub static VARIATION_BIAS_AXES: &[VariationAxis] = &[
    VariationAxis::Rom,
    VariationAxis::StanceGrip,
    VariationAxis::Tempo,
    VariationAxis::Bar,
];

/// Whether a variation has best carryover to the target movement (File 02
/// strength-036): carryover is best when stance, grip, and ROM match the
/// target. Use single-joint, muscle-focused accessories for lagging muscles
/// instead of over-specializing variations. STR-VARIATION-001 (Moderate).
pub fn variation_carryover_best(
    stance_matches: bool,
    grip_matches: bool,
    rom_matches: bool,
) -> Recommended<bool> {
    graded(stance_matches && grip_matches && rom_matches,
    "STR-VARIATION-001",)
}

// ---------------------------------------------------------------------------
// 28. Weak-point IF/THEN rules (File 02 strength-037, verbatim table)
// ---------------------------------------------------------------------------

/// Sticking points the KB keys weak-point rules to (File 02 strength-037).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickingPoint {
    /// Bench fails off the chest.
    BenchOffChest,
    /// Bench fails at lockout.
    BenchLockout,
    /// Squat fails out of the hole.
    SquatHole,
    /// Squat fails mid/high.
    SquatMidHigh,
    /// Deadlift fails off the floor.
    DeadliftFloor,
    /// Deadlift fails at lockout.
    DeadliftLockout,
}

/// A weak-point fix: main-lift variations plus accessory targets (File 02
/// strength-037).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeakPointFix {
    /// Main-lift variations biasing the sticking point.
    pub variations: &'static [&'static str],
    /// Accessory muscle targets.
    pub accessories: &'static [&'static str],
}

/// Weak-point IF/THEN exercise selection (File 02 strength-037, verbatim
/// rows). STR-WEAKPOINT-001 (ExpertOpinion, Westside-influenced).
pub fn weak_point_fix(sticking_point: StickingPoint) -> Recommended<WeakPointFix> {
    let fix = match sticking_point {
        StickingPoint::BenchOffChest => WeakPointFix {
            variations: &["paused bench", "Spoto press", "incline press"],
            accessories: &["pec", "front delt"],
        },
        StickingPoint::BenchLockout => WeakPointFix {
            variations: &["close-grip bench", "floor press"],
            accessories: &["triceps"],
        },
        StickingPoint::SquatHole => WeakPointFix {
            variations: &["pause squat", "front squat"],
            accessories: &["quad"],
        },
        StickingPoint::SquatMidHigh => WeakPointFix {
            variations: &["low-bar squat"],
            accessories: &["posterior chain"],
        },
        StickingPoint::DeadliftFloor => WeakPointFix {
            variations: &["deficit deadlift"],
            accessories: &["quad", "upper back"],
        },
        StickingPoint::DeadliftLockout => WeakPointFix {
            variations: &["rack pull", "block pull"],
            accessories: &["glute", "hamstring", "upper back"],
        },
    };
    graded(fix, "STR-WEAKPOINT-001")
}

// ---------------------------------------------------------------------------
// 29. Equipment substitution principles (File 02 strength-038)
// ---------------------------------------------------------------------------

/// Equipment-substitution principles (File 02 strength-038). KNOWN GAP: the
/// KB references a per-pattern substitution table ("1st sub → 2nd sub →
/// bodyweight fallback per pattern") but does NOT reproduce it in the
/// extract, so no concrete substitution ladder is implemented here (HARD
/// RULE 1: nothing goes in the engine that is not in the knowledge base).
/// Only the stated principles are encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubstitutionPrinciples {
    /// Preserve the movement pattern of the original lift.
    pub preserve_movement_pattern: bool,
    /// Preserve the velocity/load intent of the original prescription.
    pub preserve_velocity_load_intent: bool,
    /// Order candidate substitutions by specificity to the original.
    pub order_by_specificity: bool,
    /// Prefer free-weight over machine for transfer to free-weight tests.
    pub prefer_free_weight_over_machine: bool,
}

/// Equipment-substitution principles (File 02 strength-038): preserve the
/// movement pattern and velocity/load intent, order substitutions by
/// specificity, prefer free-weight over machine for transfer to free-weight
/// tests. See [`SubstitutionPrinciples`] for the documented table gap.
/// STR-SUBST-EQUIP-001 (ExpertOpinion).
pub fn equipment_substitution_principles() -> Recommended<SubstitutionPrinciples> {
    graded(SubstitutionPrinciples {
        preserve_movement_pattern: true,
        preserve_velocity_load_intent: true,
        order_by_specificity: true,
        prefer_free_weight_over_machine: true,
    },
    "STR-SUBST-EQUIP-001",)
}

// ---------------------------------------------------------------------------
// 30. 1RM testing readiness gate (File 02 strength-040; SAFETY)
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
    graded(allowed, "STR-1RMTEST-001")
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
    graded(cap, "STR-1RMTEST-001")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::EvidenceGrade;

    #[test]
    fn lombardi_and_backoff_and_vl() {
        assert!((e1rm_lombardi(100.0, 1) - 100.0).abs() < 1e-9);
        assert!(e1rm_lombardi(100.0, 5) > 100.0);
        let b = rpe_anchored_back_off().value;
        assert_eq!((b.top_set_rpe, b.drop_frac), (8, (0.10, 0.15)));
        assert_eq!(vl_termination_threshold(LiftGoal::MaxStrength).value, 0.20);
        assert_eq!(vl_termination_threshold(LiftGoal::Power).value, 0.20);
        assert_eq!(vl_termination_threshold(LiftGoal::Hypertrophy).value, 0.40);
    }

    #[test]
    fn periodization_phase_tables() {
        let base = linear_phase_rx(LinearPhase::Base).value;
        assert_eq!(
            (base.pct_1rm, base.sets, base.reps, base.weeks),
            (Some((67, 75)), Some((3, 5)), Some((8, 12)), (1, 4))
        );
        assert_eq!(linear_phase_rx(LinearPhase::Taper).value.pct_1rm, None);
        // Linear Peak is "90-95%+": the intensity top end is open.
        let peak = linear_phase_rx(LinearPhase::Peak).value;
        assert!(peak.pct_top_open);
        assert!(!base.pct_top_open);
        assert!(!block_phase_rx(BlockPhase::Transmutation).value.pct_top_open);
        let acc = block_phase_rx(BlockPhase::Accumulation).value;
        assert_eq!(
            (acc.pct_1rm, acc.sets, acc.reps),
            (Some((65, 80)), Some((3, 5)), Some((6, 10)))
        );
        let real = block_phase_rx(BlockPhase::Realization).value;
        assert_eq!((real.pct_1rm, real.reps), (None, Some((1, 3))));
    }

    #[test]
    fn depth_jump_gate_is_safety_critical() {
        let g = depth_jump_ready(150.0, 100.0);
        assert!(g.value);
        assert!(!depth_jump_ready(140.0, 100.0).value);
        assert!(!depth_jump_ready(200.0, 0.0).value);
        assert!(g.confidence.safety_critical);
        assert!(g.confidence.contested);
    }

    #[test]
    fn power_peaking_specifics() {
        assert_eq!(deadlift_peak_days_out().value, (10, 14));
        // strength-034: heavy-CA rest is ~5-7 min (>=5), not the overall 3-7
        // window.
        assert_eq!(pap_rest_window_min().value, (5, 7));
        let ol = olympic_derivative_rx().value;
        assert_eq!((ol.pct_1rm, ol.reps, ol.sets), ((85, 100), (1, 3), (3, 5)));
        // "85-100%+": the top of the band is open-ended.
        assert!(ol.pct_top_open);
        assert!(ol.early_in_session);
        // KB-stated velocity-biased variant loads only.
        assert_eq!(ol.velocity_variant_pcts_1rm, &[30, 45, 65, 80]);
        assert_eq!(ol.jump_shrug_pct_bodymass, 30);
        // Rest comes from the KB POWER column (3-5 min).
        assert_eq!(ol.rest_sec, (180, 300));
    }

    #[test]
    fn pap_rest_window_by_conditioning_activity() {
        // strength-034 per-CA windows: heavy ~5-7 min (>=5), plyo ~0.3-4 min.
        let heavy = pap_rest_window_min_for(ConditioningActivity::HeavyLift);
        assert_eq!(heavy.value, (5.0, 7.0));
        let plyo = pap_rest_window_min_for(ConditioningActivity::Plyometric);
        assert_eq!(plyo.value, (0.3, 4.0));
        // Plyometric CAs potentiate earlier than heavy CAs.
        assert!(plyo.value.0 < heavy.value.0);
        assert_eq!(heavy.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(
            heavy.evidence.citation.claim_id.as_deref(),
            Some("STR-PAP-001")
        );
    }

    #[test]
    fn loading_bands_match_file02() {
        let s = loading_rx(LiftGoal::MaxStrength).value;
        assert_eq!((s.pct_1rm, s.reps, s.rir), ((80, 100), (1, 5), (1, 3)));
        let h = loading_rx(LiftGoal::Hypertrophy).value;
        assert_eq!(
            (h.pct_1rm, h.reps, h.rest_sec, h.rir),
            ((65, 85), (6, 12), (30, 120), (0, 3))
        );
        // Strength intensity floor sits above hypertrophy's.
        assert!(
            loading_rx(LiftGoal::MaxStrength).value.pct_1rm.0
                > loading_rx(LiftGoal::Hypertrophy).value.pct_1rm.0
        );
        // Power carries strength-002's own Moderate grade (STR-PWR-001), not
        // the Strong intensity-primacy claim.
        let p = loading_rx(LiftGoal::Power);
        assert_eq!(p.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(p.evidence.citation.claim_id.as_deref(), Some("STR-PWR-001"));
        // Power band is the envelope of the KB spectrum (0-60% ballistic,
        // 30-70% loaded power, 70-95% pulls), not one collapsed class band.
        assert_eq!(p.value.pct_1rm, (0, 95));
    }

    #[test]
    fn two_for_two_and_progression() {
        assert!(two_for_two_met(2, 2).value);
        assert!(!two_for_two_met(2, 1).value);
        assert!(!two_for_two_met(1, 3).value);
        // strength-012 is safety-critical in the KB (caps load ramp speed).
        assert!(two_for_two_met(2, 2).confidence.safety_critical);
        assert_eq!(weekly_pct_increment(true).value, (0.01, 0.025));
        assert_eq!(weekly_pct_increment(false).value, (0.025, 0.05));
        assert!(stall_triggers_deload(3, true).value);
        assert!(!stall_triggers_deload(3, false).value);
        assert!(!stall_triggers_deload(1, true).value);
    }

    #[test]
    fn periodization_by_training_age() {
        use crate::individualization::TrainingAge;
        assert_eq!(
            periodization_model(TrainingAge::Novice).value,
            PeriodizationModel::Linear
        );
        assert_eq!(
            periodization_model(TrainingAge::Intermediate).value,
            PeriodizationModel::Dup
        );
        assert_eq!(
            periodization_model(TrainingAge::Advanced).value,
            PeriodizationModel::Block
        );
        // strength-010 is a Moderate, uncontested synthesis (STR-MODEL-001) -
        // its grade must not be inflated to PERIOD-001's Strong.
        let m = periodization_model(TrainingAge::Novice);
        assert_eq!(m.evidence.grade, EvidenceGrade::Moderate);
        assert!(!m.confidence.contested);
        // Block phases stay contested via the global model-superiority CQ-03.
        let b = block_phase_rx(BlockPhase::Accumulation);
        assert_eq!(
            b.confidence.contested_question_ref.as_deref(),
            Some("CQ-03")
        );
    }

    #[test]
    fn taper_holds_intensity() {
        let t = taper_rx().value;
        assert_eq!(t.volume_reduction_frac, (0.41, 0.60));
        assert_eq!(t.duration_days, (8, 14));
        assert!(t.hold_intensity);
    }

    #[test]
    fn plyo_caps_rise_with_level() {
        use crate::individualization::TrainingAge;
        assert_eq!(plyo_foot_contact_cap(TrainingAge::Novice).value, (80, 100));
        assert_eq!(
            plyo_foot_contact_cap(TrainingAge::Advanced).value,
            (120, 140)
        );
        assert!(
            plyo_foot_contact_cap(TrainingAge::Advanced).value.1
                > plyo_foot_contact_cap(TrainingAge::Novice).value.1
        );
    }

    #[test]
    fn epley_100kg_5reps_is_about_116_7() {
        let e = e1rm_epley(100.0, 5);
        assert!((e - 116.666_666).abs() < 1e-3, "got {e}");
    }

    #[test]
    fn brzycki_sanity() {
        // 100 kg x 5 -> 100 * 36 / 32 = 112.5
        let b = e1rm_brzycki(100.0, 5);
        assert!((b - 112.5).abs() < 1e-6, "got {b}");
        // A single rep must equal (near) the load itself: 100 * 36 / 36 = 100.
        assert!((e1rm_brzycki(100.0, 1) - 100.0).abs() < 1e-6);
        // Higher reps estimate a higher 1RM than fewer reps at equal load.
        assert!(e1rm_brzycki(100.0, 8) > e1rm_brzycki(100.0, 3));
    }

    #[test]
    fn brzycki_high_reps_stay_finite() {
        // At/above 37 reps the raw denominator (37 − reps) is zero/negative,
        // which would return +∞ or a negative 1RM. The domain clamp keeps every
        // rep count finite and positive (BUGS: e1rm_brzycki unbounded).
        let at_cap = e1rm_brzycki(100.0, BRZYCKI_MAX_REPS); // reps = 36
        assert!(at_cap.is_finite() && at_cap > 0.0, "got {at_cap}");
        for reps in [37u32, 40, 100] {
            let b = e1rm_brzycki(100.0, reps);
            assert!(b.is_finite() && b > 0.0, "reps {reps} gave {b}");
            // Above the domain the estimate saturates at the cap value rather
            // than exploding.
            assert_eq!(b, at_cap, "reps {reps} should clamp to the cap");
        }
        // The clamp must not perturb results inside the valid domain.
        assert!((e1rm_brzycki(100.0, 5) - 112.5).abs() < 1e-6);
    }

    #[test]
    fn rpe_to_rir_anchor() {
        assert_eq!(rpe_to_rir(8.0), 2.0);
        assert_eq!(rpe_to_rir(10.0), 0.0);
        // Clamp: overshoot stays at 0 RIR.
        assert_eq!(rpe_to_rir(11.0), 0.0);
        // Round trip.
        assert_eq!(rir_to_rpe(2.0), 8.0);
        assert_eq!(rir_to_rpe(0.0), 10.0);
    }

    #[test]
    fn est_pct_from_reps_is_estimate() {
        // 1 rep -> 100 / (1 + 1/30) ~= 96.77%
        let p = est_pct_1rm_from_reps(1);
        assert!((p - 96.774).abs() < 1e-2, "got {p}");
        // Monotonic decrease with more reps.
        assert!(est_pct_1rm_from_reps(10) < est_pct_1rm_from_reps(3));
    }

    #[test]
    fn prilepin_for_70pct_returns_correct_row() {
        let row = prilepin_for(70.0).expect("70% resolves");
        assert_eq!(row.pct_min, 70);
        assert_eq!(row.pct_max, 79);
        assert_eq!(row.reps_per_set, (3, 6));
        assert_eq!(row.optimal_total, 18);
        assert_eq!(row.total_range, (12, 24));
        // Boundaries and out-of-range.
        assert_eq!(prilepin_for(95.0).unwrap().optimal_total, 7);
        assert_eq!(prilepin_for(50.0).unwrap().optimal_total, 24);
        assert!(prilepin_for(-1.0).is_none());
        assert!(prilepin_for(f64::NAN).is_none());
        // The KB's ">90%" band has no upper bound: supra-max intensities
        // (accommodating resistance / overload work) resolve to it.
        assert_eq!(prilepin_for(101.0).unwrap().optimal_total, 7);
        assert_eq!(prilepin_for(125.0).unwrap().pct_min, 90);
    }

    #[test]
    fn prilepin_volume_pass_and_fail() {
        // 85% band range is 10-20 total reps.
        assert!(prilepin_volume_ok(85.0, 15).value); // optimal
        assert!(prilepin_volume_ok(85.0, 10).value); // lower bound
        assert!(prilepin_volume_ok(85.0, 20).value); // upper bound
        assert!(!prilepin_volume_ok(85.0, 9).value); // under
        assert!(!prilepin_volume_ok(85.0, 25).value); // over
        // Supra-max resolves to the >90% band (4-10 total reps).
        assert!(prilepin_volume_ok(105.0, 7).value);
        assert!(!prilepin_volume_ok(105.0, 15).value);
        assert!(!prilepin_volume_ok(-5.0, 15).value); // invalid intensity
        // The governor carries the Prilepin claim (Moderate, contested CQ-03).
        let g = prilepin_volume_ok(85.0, 15);
        assert_eq!(
            g.evidence.citation.claim_id.as_deref(),
            Some("STR-PRILEPIN-001")
        );
        assert_eq!(g.evidence.grade, EvidenceGrade::Moderate);
        assert!(g.confidence.contested);
    }

    #[test]
    fn prilepin_table_nonempty_and_sorted() {
        assert!(!PRILEPIN.is_empty());
        assert_eq!(PRILEPIN.len(), 4);
        // Ascending, non-overlapping, gap-free bands.
        for w in PRILEPIN.windows(2) {
            assert!(w[0].pct_max < w[1].pct_min);
            assert_eq!(w[0].pct_max + 1, w[1].pct_min);
        }
        // Full 0-100 coverage.
        assert_eq!(PRILEPIN.first().unwrap().pct_min, 0);
        assert_eq!(PRILEPIN.last().unwrap().pct_max, 100);
    }

    #[test]
    fn recommend_attaches_registered_evidence() {
        let rx = graded(LiftDummy, "AUTOREG-RIR-001");
        assert_eq!(rx.evidence.grade, EvidenceGrade::Strong);
        assert_eq!(
            rx.evidence.citation.claim_id.as_deref(),
            Some("AUTOREG-RIR-001")
        );
    }

    #[test]
    #[should_panic(expected = "known claim")]
    fn recommend_panics_on_unregistered_claim_id() {
        // HARD RULE 2: no fabricated fallback evidence, an unregistered id is
        // a programming error, same contract as every other engine module.
        let _ = graded(LiftDummy, "NOT-A-REAL-ID");
    }

    #[derive(Debug, PartialEq)]
    struct LiftDummy;

    // -- File 02 task-16 rules --

    #[test]
    fn e1rm_reliability_gate_and_cross_check() {
        // strength-006: reliable up to 10 reps on compounds.
        assert!(e1rm_reliable(10, false).value);
        assert!(!e1rm_reliable(11, false).value); // >10 reps unreliable
        assert!(!e1rm_reliable(5, true).value); // isolation unreliable
        assert!(!e1rm_reliable(0, false).value); // no reps, no estimate
        // Cross-check spans >=2 formulas and brackets the single estimates.
        let c = e1rm_cross_check(100.0, 5, false).value.expect("reliable");
        assert!(c.formulas_used as usize >= E1RM_MIN_CROSS_CHECK_FORMULAS);
        assert!(c.low_kg <= e1rm_epley(100.0, 5) && e1rm_epley(100.0, 5) <= c.high_kg);
        assert!(c.low_kg <= e1rm_brzycki(100.0, 5) && e1rm_brzycki(100.0, 5) <= c.high_kg);
        assert!(c.low_kg <= c.high_kg);
        // Unreliability gate returns None instead of a number.
        assert!(e1rm_cross_check(100.0, 11, false).value.is_none());
        assert!(e1rm_cross_check(100.0, 5, true).value.is_none());
        assert!(e1rm_cross_check(0.0, 5, false).value.is_none());
        // Preferred test sets are 3-6 reps.
        assert_eq!(e1rm_test_set_reps().value, (3, 6));
        // Evidence pin: Moderate, DiStasio.
        let r = e1rm_reliable(5, false);
        assert_eq!(r.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(
            r.evidence.citation.claim_id.as_deref(),
            Some("STR-E1RM-CHECK-001")
        );
    }

    #[test]
    fn load_mode_selection_by_level_and_monitoring() {
        use crate::individualization::TrainingAge;
        // strength-008: novices and no-monitoring get fixed %.
        assert_eq!(
            load_prescription_mode(TrainingAge::Novice, true).value,
            LoadPrescriptionMode::FixedPercent
        );
        assert_eq!(
            load_prescription_mode(TrainingAge::Advanced, false).value,
            LoadPrescriptionMode::FixedPercent
        );
        assert_eq!(
            load_prescription_mode(TrainingAge::Intermediate, true).value,
            LoadPrescriptionMode::RpeRir
        );
        assert_eq!(
            load_prescription_mode(TrainingAge::Advanced, true).value,
            LoadPrescriptionMode::RpeRir
        );
        // Contested via the File 02 local CQ-01 (namespaced).
        let m = load_prescription_mode(TrainingAge::Novice, true);
        assert_eq!(m.evidence.grade, EvidenceGrade::Moderate);
        assert!(m.confidence.contested);
        assert_eq!(
            m.confidence.contested_question_ref.as_deref(),
            Some("CQ-F02-01")
        );
    }

    #[test]
    fn velocity_zone_table_matches_kb() {
        // strength-016 verbatim rows.
        assert_eq!(VELOCITY_ZONES.len(), 5);
        let abs = velocity_zone_rx(VelocityZone::AbsoluteStrength).value;
        assert_eq!(abs.bar_speed_ms, (0.15, Some(0.50)));
        assert_eq!(abs.pct_1rm, (Some(90), Some(100)));
        let ss = velocity_zone_rx(VelocityZone::StrengthSpeed).value;
        assert_eq!(ss.bar_speed_ms, (0.75, Some(1.00)));
        assert_eq!(ss.pct_1rm, (Some(55), Some(80)));
        let sp = velocity_zone_rx(VelocityZone::Speed).value;
        assert_eq!(sp.bar_speed_ms, (1.30, None)); // ">1.30 m/s"
        assert_eq!(sp.pct_1rm, (None, Some(30))); // "<30%"
        // Contiguous speed bands: each zone's floor is the previous ceiling.
        for w in VELOCITY_ZONES.windows(2) {
            assert_eq!(Some(w[1].bar_speed_ms.0), w[0].bar_speed_ms.1);
        }
        // MVT constants: bench ~0.15, squat ~0.30 m/s.
        assert_eq!(mvt_ms(MvtLift::BenchPress).value, 0.15);
        assert_eq!(mvt_ms(MvtLift::BackSquat).value, 0.30);
        let z = velocity_zone_rx(VelocityZone::SpeedStrength);
        assert_eq!(z.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(z.evidence.citation.claim_id.as_deref(), Some("STR-VZONE-001"));
    }

    #[test]
    fn lvp_e1rm_extrapolates_but_never_for_deadlift() {
        // Synthetic perfectly linear bench profile: v = 1.05 - 0.006*load
        // → v = 0.15 (bench MVT) at load 150.
        let bench: Vec<(f64, f64)> = [60.0, 80.0, 100.0, 120.0, 130.0]
            .iter()
            .map(|&l| (l, 1.05 - 0.006 * l))
            .collect();
        let est = lvp_e1rm(LvpLift::BenchPress, &bench).value.expect("estimate");
        assert!((est - 150.0).abs() < 1e-6, "got {est}");
        // Squat extrapolates to its own MVT (0.30): v = 1.5 - 0.006*load → 200.
        let squat: Vec<(f64, f64)> = [80.0, 110.0, 140.0, 170.0]
            .iter()
            .map(|&l| (l, 1.5 - 0.006 * l))
            .collect();
        let est = lvp_e1rm(LvpLift::BackSquat, &squat).value.expect("estimate");
        assert!((est - 200.0).abs() < 1e-6, "got {est}");
        // HARD guard: deadlift LVP must not predict 1RM, even with a perfect
        // profile.
        assert!(lvp_e1rm(LvpLift::Deadlift, &squat).value.is_none());
        // <4 profile loads → no estimate (KB: 4-7, recommend 5-7).
        assert!(lvp_e1rm(LvpLift::BenchPress, &bench[..3]).value.is_none());
        // Degenerate profiles (flat/positive slope) → no estimate.
        let flat = [(60.0, 0.8), (80.0, 0.8), (100.0, 0.8), (120.0, 0.8)];
        assert!(lvp_e1rm(LvpLift::BenchPress, &flat).value.is_none());
        // Monitoring-only constants.
        assert_eq!(LVP_SEE_PCT_1RM, 9.8);
        assert_eq!(LVP_PROFILE_LOADS, (4, 7));
        assert_eq!(LVP_PROFILE_LOADS_PREFERRED, (5, 7));
        let r = lvp_e1rm(LvpLift::BenchPress, &bench);
        assert_eq!(r.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(r.evidence.citation.claim_id.as_deref(), Some("STR-LVP-001"));
    }

    #[test]
    fn first_rep_speed_autoregulates_daily_load() {
        use DailyLoadAdjustment::*;
        // strength-019 against the strength-speed zone (0.75-1.00 m/s).
        let z = VelocityZone::StrengthSpeed;
        assert_eq!(first_rep_load_adjustment(1.10, z).value, Increase);
        assert_eq!(first_rep_load_adjustment(0.85, z).value, Hold);
        assert_eq!(first_rep_load_adjustment(0.60, z).value, Decrease);
        // Zone bounds themselves hold.
        assert_eq!(first_rep_load_adjustment(0.75, z).value, Hold);
        assert_eq!(first_rep_load_adjustment(1.00, z).value, Hold);
        // Open-topped Speed zone can only hold or decrease.
        let s = VelocityZone::Speed;
        assert_eq!(first_rep_load_adjustment(2.50, s).value, Hold);
        assert_eq!(first_rep_load_adjustment(1.00, s).value, Decrease);
        let r = first_rep_load_adjustment(0.85, z);
        assert_eq!(r.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(
            r.evidence.citation.claim_id.as_deref(),
            Some("STR-VBT-DAILY-001")
        );
    }

    #[test]
    fn dup_days_match_kb_example_week() {
        // strength-023 verbatim: heavy 3-5x3-5 @85-90; power 3-6x3-5 @50-70
        // fast; hypertrophy 3-4x8-12 @70-75.
        let h = dup_day_rx(DupDay::Heavy).value;
        assert_eq!((h.pct_1rm, h.sets, h.reps), ((85, 90), (3, 5), (3, 5)));
        assert!(!h.max_velocity_intent);
        let p = dup_day_rx(DupDay::Power).value;
        assert_eq!((p.pct_1rm, p.sets, p.reps), ((50, 70), (3, 6), (3, 5)));
        assert!(p.max_velocity_intent);
        let hy = dup_day_rx(DupDay::Hypertrophy).value;
        assert_eq!((hy.pct_1rm, hy.sets, hy.reps), ((70, 75), (3, 4), (8, 12)));
        assert_eq!(dup_lift_frequency_per_week().value, (2, 3));
        // Moderate (mixed), contested via the global model-superiority CQ-03.
        let r = dup_day_rx(DupDay::Heavy);
        assert_eq!(r.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(r.confidence.contested_question_ref.as_deref(), Some("CQ-03"));
        assert!(!r.confidence.safety_critical);
    }

    #[test]
    fn conjugate_is_gated_to_advanced_and_safety_critical() {
        // strength-024: no conjugate below >=2-3 yr barbell training.
        assert!(conjugate_rx(0.0).value.is_none());
        assert!(conjugate_rx(1.9).value.is_none());
        assert!(conjugate_rx(f64::NAN).value.is_none());
        let rx = conjugate_rx(3.0).value.expect("advanced lifter");
        assert_eq!(rx.days_per_week, 4);
        assert_eq!(
            (rx.competition_lift_work_frac, rx.accessory_work_frac),
            (0.20, 0.80)
        );
        assert_eq!((rx.me_pct_1rm_min, rx.me_rep_max), (90, (1, 3)));
        assert_eq!(
            (rx.me_rotate_weeks_elite, rx.me_rotate_weeks_raw),
            (1, (2, 3))
        );
        assert_eq!(rx.de_squat_dl_pct_1rm, (50, 60));
        assert_eq!(rx.de_band_chain_pct, (20, 25));
        assert_eq!(rx.de_bench_pct_1rm, (40, 60));
        assert_eq!((rx.de_reps, rx.de_wave_weeks), ((1, 3), 3));
        // ExpertOpinion, safety-critical gate, contested CQ-F02-04.
        let r = conjugate_rx(3.0);
        assert_eq!(r.evidence.grade, EvidenceGrade::ExpertOpinion);
        assert!(r.confidence.safety_critical);
        assert_eq!(
            r.confidence.contested_question_ref.as_deref(),
            Some("CQ-F02-04")
        );
    }

    #[test]
    fn wave_loading_example_structure() {
        let w = wave_loading_rx().value;
        assert_eq!(w.wave_reps, (3, 2, 1));
        assert_eq!(w.waves, 2);
        assert!(w.load_rises_per_wave);
        let r = wave_loading_rx();
        assert_eq!(r.evidence.grade, EvidenceGrade::ExpertOpinion);
        assert_eq!(r.evidence.citation.claim_id.as_deref(), Some("STR-WAVE-001"));
    }

    #[test]
    fn peaking_bands_match_strength_027() {
        let p = peaking_rx().value;
        assert_eq!(p.volume_reduction_frac, (0.30, 0.70));
        assert_eq!(p.intensity_pct_1rm_min, 85);
        assert_eq!(p.taper_weeks, (1, 2));
        assert_eq!(p.cessation_days, (2, 7));
        assert_eq!(p.freq_frac_min_highly_trained, 0.80);
        assert_eq!(p.freq_reduction_frac_moderately_trained, (0.30, 0.50));
        let r = peaking_rx();
        assert_eq!(r.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(r.evidence.citation.claim_id.as_deref(), Some("STR-PEAK-001"));
    }

    #[test]
    fn power_load_spectrum_per_class() {
        // strength-030: spectrum, not one optimal load.
        assert_eq!(power_load_spectrum(PowerExerciseClass::JumpSquat).value, (0, 0));
        assert_eq!(
            power_load_spectrum(PowerExerciseClass::LoadedSquatPower).value,
            (30, 70)
        );
        assert_eq!(
            power_load_spectrum(PowerExerciseClass::PowerClean).value,
            (40, 80)
        );
        assert_eq!(
            power_load_spectrum(PowerExerciseClass::WeightliftingPulls).value,
            (90, 95)
        );
        assert_eq!(CLEAN_VARIATION_MIN_PCT_1RM, 70);
        // Every class band sits inside the loading_rx Power envelope.
        let envelope = loading_rx(LiftGoal::Power).value.pct_1rm;
        for class in [
            PowerExerciseClass::JumpSquat,
            PowerExerciseClass::LoadedSquatPower,
            PowerExerciseClass::PowerClean,
            PowerExerciseClass::WeightliftingPulls,
        ] {
            let (lo, hi) = power_load_spectrum(class).value;
            assert!(lo >= envelope.0 && hi <= envelope.1);
        }
        let r = power_load_spectrum(PowerExerciseClass::PowerClean);
        assert_eq!(r.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(
            r.evidence.citation.claim_id.as_deref(),
            Some("STR-PWRSPEC-001")
        );
    }

    #[test]
    fn plyo_schedule_matches_strength_032() {
        let s = plyo_schedule_rx().value;
        assert_eq!(s.sessions_per_week, (1, 3));
        assert_eq!(s.session_spacing_hours, (48, 72));
        assert_eq!(s.rest_between_sets_sec, (120, 180)); // 2-3 min
        assert_eq!(s.depth_jump_work_rest_denominator, 10); // ~1:10
        assert_eq!(s.depth_jump_inter_rep_rest_sec, (5, 10));
        // strength-032 is safety-critical in the KB.
        let r = plyo_schedule_rx();
        assert!(r.confidence.safety_critical);
        assert_eq!(r.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(
            r.evidence.citation.claim_id.as_deref(),
            Some("STR-PLYO-SCHED-001")
        );
    }

    #[test]
    fn competition_lifts_anchor_by_sport() {
        assert_eq!(
            competition_lifts(StrengthSport::Powerlifting).value,
            &["squat", "bench press", "deadlift"]
        );
        assert_eq!(
            competition_lifts(StrengthSport::Weightlifting).value,
            &["snatch", "clean & jerk"]
        );
        // Strong specificity principle.
        let r = competition_lifts(StrengthSport::Powerlifting);
        assert_eq!(r.evidence.grade, EvidenceGrade::Strong);
        assert_eq!(
            r.evidence.citation.claim_id.as_deref(),
            Some("STR-COMP-ANCHOR-001")
        );
    }

    #[test]
    fn variation_carryover_requires_full_match() {
        assert!(variation_carryover_best(true, true, true).value);
        assert!(!variation_carryover_best(false, true, true).value);
        assert!(!variation_carryover_best(true, false, true).value);
        assert!(!variation_carryover_best(true, true, false).value);
        assert_eq!(VARIATION_BIAS_AXES.len(), 4); // ROM, stance/grip, tempo, bar
        let r = variation_carryover_best(true, true, true);
        assert_eq!(r.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(
            r.evidence.citation.claim_id.as_deref(),
            Some("STR-VARIATION-001")
        );
    }

    #[test]
    fn weak_point_table_matches_kb_rows() {
        let b = weak_point_fix(StickingPoint::BenchOffChest).value;
        assert_eq!(b.variations, &["paused bench", "Spoto press", "incline press"]);
        assert_eq!(b.accessories, &["pec", "front delt"]);
        let bl = weak_point_fix(StickingPoint::BenchLockout).value;
        assert_eq!(bl.variations, &["close-grip bench", "floor press"]);
        assert_eq!(bl.accessories, &["triceps"]);
        let sh = weak_point_fix(StickingPoint::SquatHole).value;
        assert_eq!(sh.variations, &["pause squat", "front squat"]);
        let sm = weak_point_fix(StickingPoint::SquatMidHigh).value;
        assert_eq!((sm.variations, sm.accessories), (&["low-bar squat"][..], &["posterior chain"][..]));
        let df = weak_point_fix(StickingPoint::DeadliftFloor).value;
        assert_eq!((df.variations, df.accessories), (&["deficit deadlift"][..], &["quad", "upper back"][..]));
        let dl = weak_point_fix(StickingPoint::DeadliftLockout).value;
        assert_eq!(dl.variations, &["rack pull", "block pull"]);
        assert_eq!(dl.accessories, &["glute", "hamstring", "upper back"]);
        // ExpertOpinion, Westside-influenced.
        let r = weak_point_fix(StickingPoint::SquatHole);
        assert_eq!(r.evidence.grade, EvidenceGrade::ExpertOpinion);
        assert_eq!(
            r.evidence.citation.claim_id.as_deref(),
            Some("STR-WEAKPOINT-001")
        );
    }

    #[test]
    fn substitution_principles_only_no_invented_table() {
        // strength-038: the KB names the principles but does not reproduce the
        // per-pattern table, only the principles exist here.
        let s = equipment_substitution_principles().value;
        assert!(s.preserve_movement_pattern);
        assert!(s.preserve_velocity_load_intent);
        assert!(s.order_by_specificity);
        assert!(s.prefer_free_weight_over_machine);
        let r = equipment_substitution_principles();
        assert_eq!(r.evidence.grade, EvidenceGrade::ExpertOpinion);
        assert_eq!(
            r.evidence.citation.claim_id.as_deref(),
            Some("STR-SUBST-EQUIP-001")
        );
        // The registry entry itself records the missing-table gap.
        let entry = crate::evidence::claim("STR-SUBST-EQUIP-001").unwrap();
        assert!(
            entry
                .contradicting
                .iter()
                .any(|c| c.contains("not reproduced")),
            "table gap must stay documented on the claim"
        );
    }

    #[test]
    fn one_rm_test_gate_is_safety_critical() {
        let ready = OneRmTestContext {
            technically_proficient: true,
            adequately_recovered: true,
            warmed_up: true,
            is_novice: false,
            supervised: false,
            spinal_loading: false,
            bracing_competent: false,
        };
        assert!(one_rm_test_allowed(ready).value);
        // Each readiness gate blocks on its own.
        for ctx in [
            OneRmTestContext { technically_proficient: false, ..ready },
            OneRmTestContext { adequately_recovered: false, ..ready },
            OneRmTestContext { warmed_up: false, ..ready },
            // Novice without supervision.
            OneRmTestContext { is_novice: true, ..ready },
            // Spinal loading without bracing competence.
            OneRmTestContext { spinal_loading: true, ..ready },
        ] {
            assert!(!one_rm_test_allowed(ctx).value, "{ctx:?} must block");
        }
        // Novice + supervision and spinal + bracing pass.
        assert!(
            one_rm_test_allowed(OneRmTestContext {
                is_novice: true,
                supervised: true,
                ..ready
            })
            .value
        );
        assert!(
            one_rm_test_allowed(OneRmTestContext {
                spinal_loading: true,
                bracing_competent: true,
                ..ready
            })
            .value
        );
        // Novice jump caps: upper 2.5-5%, lower 5-10%.
        assert_eq!(novice_load_jump_cap_frac(true).value, (0.025, 0.05));
        assert_eq!(novice_load_jump_cap_frac(false).value, (0.05, 0.10));
        // Safety-critical, conservatively ExpertOpinion (KB: EO/Moderate 0.40).
        let r = one_rm_test_allowed(ready);
        assert!(r.confidence.safety_critical);
        assert_eq!(r.evidence.grade, EvidenceGrade::ExpertOpinion);
        assert_eq!(
            r.evidence.citation.claim_id.as_deref(),
            Some("STR-1RMTEST-001")
        );
    }
}
