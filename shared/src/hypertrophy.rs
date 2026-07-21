//! Hypertrophy programming calculators (knowledge-base File 03 -
//! Evidence-Graded Hypertrophy Programming Logic).
//!
//! Pure, deterministic look-ups + arithmetic: per-muscle weekly volume
//! landmarks (Table 1), rep/load prescription by exercise class (Table 2),
//! volume→frequency mapping (Table 3), rest defaults (Table 5), the RP
//! accumulation set-ramp and RIR schedule. No IO, no clocks, no randomness.
//!
//! Also: load interchangeability + effort/RIR rules (hyp-012/017/018/020/021/
//! 022/023), exercise selection + Table 4 substitution (hyp-027/028/029/030),
//! mesocycle structure + progression drivers (hyp-031/033/037/044), tempo and
//! supersets (hyp-040/041), and the intermediate default program (hyp-043).
//!
//! Numbers transcribed verbatim from File 03. Every prescriptive value is
//! wrapped in [`Recommended`] via [`recommend`], which forces attached evidence
//! and confidence from the compile-time registry (`crate::evidence`). Claim ids:
//! HYP-VOL-001, HYP-LANDMARKS-001, HYP-REPLOAD-001, HYP-FREQ-001, HYP-REST-001,
//! HYP-RIR-RAMP-001, plus the "File 03 rule claims - task 17" block in
//! `crate::evidence` (HYP-LOADRANGE/VOLRAMP-SAFE/SKILL-RIR/RIR-DEFAULT/RIR-ACC/
//! FAIL-SAFE/CUT-OBJ/VEL-CHECK/SFR/LENGTHSEL/SUBST/PAIN-SWAP/MESO-STRUCT/
//! DOUBLEPROG/LAYOFF/TEMPO/SUPERSET/DEFAULT-PROG/SPEC-BLOCK-001).

use crate::evidence;
use crate::individualization::TrainingAge;
use crate::schema::Recommended;

/// Build a `Recommended<T>` from a registry claim id (must exist).
fn recommend<T>(value: T, claim_id: &str) -> Recommended<T> {
    let e = evidence::claim(claim_id).expect("known hypertrophy claim");
    Recommended::new(value, e.to_evidence(), e.to_confidence_tag())
}

// ---------------------------------------------------------------------------
// 1. Per-muscle weekly volume landmarks (File 03 Table 1; HYP-LANDMARKS-001)
// ---------------------------------------------------------------------------

/// Weekly direct-set landmarks for one muscle (intermediate lifter).
/// Population starting points, not validated constants (HYP-LANDMARKS-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeLandmarks {
    pub muscle: &'static str,
    /// Maintenance Volume, weekly sets to retain muscle.
    pub mv: u8,
    /// Minimum Effective Volume, mesocycle start point.
    pub mev: u8,
    /// Maximum Adaptive Volume, the productive climb range (lo, hi).
    pub mav: (u8, u8),
    /// Maximum Recoverable Volume, recovery ceiling.
    pub mrv: u8,
}

/// Table 1, transcribed verbatim from File 03 (RP framework, ExpertOpinion).
/// `mrv` uses the "N+" lower bound (e.g. "22+" → 22).
pub static LANDMARKS: &[VolumeLandmarks] = &[
    VolumeLandmarks {
        muscle: "chest",
        mv: 8,
        mev: 10,
        mav: (12, 20),
        mrv: 22,
    },
    VolumeLandmarks {
        muscle: "back",
        mv: 8,
        mev: 10,
        mav: (14, 22),
        mrv: 25,
    },
    VolumeLandmarks {
        muscle: "quads",
        mv: 6,
        mev: 8,
        mav: (12, 18),
        mrv: 20,
    },
    VolumeLandmarks {
        muscle: "hamstrings",
        mv: 4,
        mev: 6,
        mav: (10, 16),
        mrv: 20,
    },
    VolumeLandmarks {
        muscle: "glutes",
        mv: 0,
        mev: 0,
        mav: (4, 12),
        mrv: 16,
    },
    VolumeLandmarks {
        muscle: "side delts",
        mv: 6,
        mev: 8,
        mav: (16, 22),
        mrv: 26,
    },
    VolumeLandmarks {
        muscle: "rear delts",
        mv: 0,
        mev: 6,
        mav: (12, 18),
        mrv: 22,
    },
    VolumeLandmarks {
        muscle: "biceps",
        mv: 4,
        mev: 6,
        mav: (14, 20),
        mrv: 26,
    },
    VolumeLandmarks {
        muscle: "triceps",
        mv: 4,
        mev: 6,
        mav: (10, 14),
        mrv: 18,
    },
    VolumeLandmarks {
        muscle: "calves",
        mv: 4,
        mev: 6,
        mav: (8, 16),
        mrv: 20,
    },
    VolumeLandmarks {
        muscle: "abs",
        mv: 0,
        mev: 0,
        mav: (10, 16),
        mrv: 20,
    },
];

/// Look up landmarks by muscle name (case-insensitive). `None` if unknown.
pub fn landmarks_for(muscle: &str) -> Option<&'static VolumeLandmarks> {
    LANDMARKS
        .iter()
        .find(|l| l.muscle.eq_ignore_ascii_case(muscle))
}

// ---------------------------------------------------------------------------
// 2. Rep/load by exercise class (File 03 Table 2; HYP-REPLOAD-001)
// ---------------------------------------------------------------------------

/// Exercise class driving the rep/load window (File 03 Table 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExerciseClass {
    /// Squat, deadlift, press, row, manages joint/systemic fatigue per rep.
    HeavyCompound,
    /// Machine / moderate compound, balances tension and volume.
    ModerateCompound,
    /// Curls, raises, extensions, lighter loads, higher reps.
    Isolation,
}

/// Rep range + %1RM window for a hypertrophy set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepLoad {
    /// Target reps per set (min, max).
    pub reps: (u8, u8),
    /// %1RM band (min, max).
    pub pct_1rm: (u8, u8),
}

/// Rep/load prescription for an exercise class (File 03 Table 2; HYP-REPLOAD-001).
pub fn rep_load(class: ExerciseClass) -> Recommended<RepLoad> {
    let rl = match class {
        ExerciseClass::HeavyCompound => RepLoad {
            reps: (5, 10),
            pct_1rm: (75, 85),
        },
        ExerciseClass::ModerateCompound => RepLoad {
            reps: (8, 15),
            pct_1rm: (65, 75),
        },
        ExerciseClass::Isolation => RepLoad {
            reps: (12, 25),
            pct_1rm: (50, 70),
        },
    };
    recommend(rl, "HYP-REPLOAD-001")
}

/// Between-set rest window in seconds for an exercise class (File 03 Table 5;
/// HYP-REST-001). Compounds rest longer to preserve per-set volume; the goal is
/// keeping >=90% of first-set reps, not the clock itself.
///
/// Table 5 has exactly two resistance rows: "Heavy compound: 2–3 min" and
/// "Machine/isolation: 1–2 min". Table 2 files the moderate class as
/// "moderate compounds / machines", so [`ExerciseClass::ModerateCompound`]
/// takes the machine row (1–2 min), no extrapolated third window.
pub fn rest_sec_for(class: ExerciseClass) -> Recommended<(u16, u16)> {
    let window = match class {
        ExerciseClass::HeavyCompound => (120, 180),
        ExerciseClass::ModerateCompound | ExerciseClass::Isolation => (60, 120),
    };
    recommend(window, "HYP-REST-001")
}

// ---------------------------------------------------------------------------
// 3. Volume→frequency mapping (File 03 Table 3; HYP-FREQ-001)
// ---------------------------------------------------------------------------

/// Suggested weekly frequency and per-session set spread for a weekly volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrequencyRx {
    /// Sessions per week for this muscle (min, max).
    pub freq: (u8, u8),
    /// Sets per session (min, max).
    pub per_session: (u8, u8),
}

/// Map weekly sets/muscle to a frequency + per-session spread (File 03 Table 3;
/// HYP-FREQ-001). Bands: ≤10 → 1–2×; 11–18 → 2–3×; >18 → 3×.
pub fn frequency_for_weekly_sets(weekly_sets: u8) -> Recommended<FrequencyRx> {
    let rx = if weekly_sets <= 10 {
        FrequencyRx {
            freq: (1, 2),
            per_session: (1, 8),
        }
    } else if weekly_sets <= 18 {
        FrequencyRx {
            freq: (2, 3),
            per_session: (5, 8),
        }
    } else {
        FrequencyRx {
            freq: (3, 3),
            per_session: (6, 9),
        }
    };
    recommend(rx, "HYP-FREQ-001")
}

// ---------------------------------------------------------------------------
// 4. RP accumulation drivers (File 03 hyp-032/019)
// ---------------------------------------------------------------------------

/// Largest allowed week-over-week set increment: hyp-001 climbs "~2–4
/// sets/week", and hyp-011 (SAFETY, HYP-VOLRAMP-SAFE-001) forbids abrupt
/// jumps toward MRV, so no ramp step may exceed +4 sets/week.
pub const MAX_WEEKLY_SET_INCREMENT: u8 = 4;

/// Weekly set counts ramping from `mev` to `mrv` over `weeks` accumulation
/// weeks (File 03 hyp-001/032; HYP-SETRAMP-001, ExpertOpinion RP scheme,
/// contested CQ-F03-04, contradicted by Enes 2024). Linear interpolation, floored, so
/// the RP worked example (MEV 10 → MRV 20 over 4 wk) yields `[10, 13, 16, 20]`.
/// `weeks == 0` → empty; `weeks == 1` → `[mev]`; `mrv < mev` clamps to `mev`.
///
/// Steps are capped at [`MAX_WEEKLY_SET_INCREMENT`] (+4 sets/week, hyp-001
/// upper increment; hyp-011 no-abrupt-jump SAFETY guard), so a short block
/// with a wide MEV→MRV span tops out below MRV rather than jumping to it -
/// hyp-011: never jump straight to MRV.
pub fn weekly_set_ramp(mev: u8, mrv: u8, weeks: u8) -> Recommended<Vec<u8>> {
    let ramp = if weeks == 0 {
        Vec::new()
    } else if weeks == 1 {
        vec![mev]
    } else {
        let top = mrv.max(mev);
        let span = (top - mev) as f64;
        let last = (weeks - 1) as f64;
        let mut prev = mev;
        (0..weeks)
            .map(|i| {
                let linear = mev + (span * i as f64 / last) as u8;
                let capped = linear.min(prev.saturating_add(MAX_WEEKLY_SET_INCREMENT));
                prev = capped;
                capped
            })
            .collect()
    };
    recommend(ramp, "HYP-SETRAMP-001")
}

/// hyp-011 no-abrupt-jump guard (HYP-VOLRAMP-SAFE-001, ExpertOpinion, SAFETY):
/// `true` when a proposed week-over-week weekly-set change exceeds the hyp-001
/// +2–4 sets/week climb (i.e. > +4), which raises injury risk and must be
/// rejected. The KB states no numeric bound of its own; the +4 cap is
/// hyp-001's upper increment.
pub fn abrupt_volume_jump(prev_weekly_sets: u8, next_weekly_sets: u8) -> Recommended<bool> {
    recommend(
        next_weekly_sets.saturating_sub(prev_weekly_sets) > MAX_WEEKLY_SET_INCREMENT,
        "HYP-VOLRAMP-SAFE-001",
    )
}

/// Highest RIR the KB schedule ever prescribes: the hyp-019 ramp starts at 4,
/// and hyp-020 caps RIR reporting accuracy at 0–5 (error >2 reps beyond),
/// so extrapolating a longer block to >4 starting RIR is not supported.
pub const MAX_SCHEDULED_RIR: u8 = 4;

/// Reps-in-reserve target for `week` of a `block_weeks`-long accumulation block
/// (File 03 hyp-019; HYP-RIR-RAMP-001). RIR descends to 1 in the final week:
/// a 4-week block gives week 1→4, 2→3, 3→2, 4→1. `None` outside `1..=block_weeks`.
///
/// The KB schedule is 4 weeks; longer blocks hold [`MAX_SCHEDULED_RIR`] (4)
/// in the early weeks instead of extrapolating higher, hyp-020: RIR is only
/// ~±1-rep accurate at 0–5 RIR, so >5 RIR targets would be unreliable.
pub fn rir_for_week(week: u8, block_weeks: u8) -> Option<Recommended<u8>> {
    if week == 0 || week > block_weeks {
        return None;
    }
    let rir = (block_weeks - week + 1).min(MAX_SCHEDULED_RIR);
    Some(recommend(rir, "HYP-RIR-RAMP-001"))
}

// ---------------------------------------------------------------------------
// 5. Volume ceilings, training-age MEV, and recovery scaling (File 03
//    hypertrophy-003/004/006/007/008/009/010/045/025)
// ---------------------------------------------------------------------------

/// Growth-target ceiling: outcome superiority is undetectable beyond ~31
/// fractional weekly sets/muscle (File 03 hypertrophy-003; Pelland 2025). Never
/// program a growth target above this.
pub const WEEKLY_FRACTIONAL_SET_CEILING: u8 = 31;

/// Per-session cap: ~11 fractional sets/muscle; beyond this, redistribute work
/// to another session rather than adding sets (File 03 hypertrophy-004).
pub const PER_SESSION_SET_CEILING: u8 = 11;

/// Minimum sets per exercise, multiple sets beat single sets ~+40% ES
/// (File 03 hypertrophy-006; Krieger 2010).
pub const MIN_SETS_PER_EXERCISE: u8 = 2;

/// The weekly-set target above which a muscle's work must be split across ≥2
/// sessions (File 03 hypertrophy-025).
pub const WEEKLY_SPLIT_THRESHOLD: u8 = 12;

/// Clamp a proposed weekly growth-target set count to the ~31-set ceiling
/// (File 03 hypertrophy-003).
pub fn cap_weekly_growth_target(weekly_sets: u8) -> Recommended<u8> {
    recommend(
        weekly_sets.min(WEEKLY_FRACTIONAL_SET_CEILING),
        "HYP-VOL-001",
    )
}

/// MEV weekly-set band per muscle by training age (File 03 hypertrophy-007;
/// HYP-MEV-AGE-001, Weak): beginner 6–10, intermediate 10–18, advanced
/// 12–20(+). Returns `(lo, hi)`.
pub fn mev_sets_by_training_age(age: TrainingAge) -> Recommended<(u8, u8)> {
    let band = match age {
        TrainingAge::Novice => (6, 10),
        TrainingAge::Intermediate => (10, 18),
        TrainingAge::Advanced => (12, 20),
    };
    recommend(band, "HYP-MEV-AGE-001")
}

/// Next-mesocycle weekly-set target given growth/recovery signals (File 03
/// hypertrophy-008; HYP-MESO-ADD-001, ExpertOpinion): if a muscle is not growing while recovery is easy, add +2
/// sets (still below MEV); otherwise hold. Capped at the growth ceiling.
pub fn next_meso_weekly_sets(
    current_weekly_sets: u8,
    not_growing: bool,
    recovering_easily: bool,
) -> Recommended<u8> {
    let next = if not_growing && recovering_easily {
        (current_weekly_sets + 2).min(WEEKLY_FRACTIONAL_SET_CEILING)
    } else {
        current_weekly_sets
    };
    recommend(next, "HYP-MESO-ADD-001")
}

/// At/over-MRV deload gate (File 03 hypertrophy-009; HYP-MRV-DELOAD-001,
/// Moderate): weekly sets > ~20/muscle
/// with regressing performance OR aching joints → treat as over MRV and deload.
pub fn over_mrv_deload(
    weekly_sets: u8,
    performance_down: bool,
    joint_ache: bool,
) -> Recommended<bool> {
    recommend(
        weekly_sets > 20 && (performance_down || joint_ache),
        "HYP-MRV-DELOAD-001",
    )
}

/// Recovery-adjusted weekly volume (File 03 hypertrophy-010/045;
/// HYP-RECOVOL-001, ExpertOpinion): in a deficit
/// or with poor sleep/high stress, scale weekly sets to 70–80% of the base and
/// reduce failure frequency. Returns the `(lo, hi)` adjusted set counts; when
/// recovery is high the base passes through unchanged.
pub fn recovery_adjusted_volume(
    base_weekly_sets: u8,
    low_recovery: bool,
) -> Recommended<(f64, f64)> {
    let b = base_weekly_sets as f64;
    let range = if low_recovery {
        (b * 0.70, b * 0.80)
    } else {
        (b, b)
    };
    recommend(range, "HYP-RECOVOL-001")
}

/// Joint-pain rep shift (File 03 hypertrophy-016; HYP-PAIN-SHIFT-001, Strong,
/// safety-critical): on joint pain at heavy loads, move that muscle's work to
/// 12–25 reps at lighter load (50–70% 1RM); hypertrophy is preserved via load
/// interchangeability.
pub fn joint_pain_rep_shift() -> Recommended<RepLoad> {
    recommend(
        RepLoad {
            reps: (12, 25),
            pct_1rm: (50, 70),
        },
        "HYP-PAIN-SHIFT-001",
    )
}

/// Whether a weekly-set target must be split across ≥2 sessions (File 03
/// hypertrophy-025; HYP-SPLIT-001, Moderate): true once weekly sets exceed
/// ~12/muscle.
pub fn needs_session_split(weekly_sets: u8) -> Recommended<bool> {
    recommend(weekly_sets > WEEKLY_SPLIT_THRESHOLD, "HYP-SPLIT-001")
}

/// Whether per-session volume exceeds the ~11-set cap and warrants adding a
/// session rather than more sets (File 03 hypertrophy-004; HYP-SESSCAP-001,
/// Moderate).
pub fn per_session_over_cap(session_sets: u8) -> Recommended<bool> {
    recommend(session_sets > PER_SESSION_SET_CEILING, "HYP-SESSCAP-001")
}

/// Deload gate from accumulated overreaching triggers (File 03 hypertrophy-035;
/// HYP-DELOAD-TRIG-001, Moderate):
/// deload now once ≥2 triggers appear (performance decrement, RIR drift to 0,
/// persistent joint/tendon aches, disrupted sleep, elevated RHR, mood drop) -
/// otherwise follow the preplanned 4–8 week schedule.
pub fn deload_indicated(trigger_count: u8) -> Recommended<bool> {
    recommend(trigger_count >= 2, "HYP-DELOAD-TRIG-001")
}

/// A one-week hypertrophy deload prescription (File 03 hypertrophy-036).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeloadRx {
    /// Fraction of working sets to keep (~MV, roughly half).
    pub sets_fraction: f64,
    /// Reps-in-reserve to hold (min, max+).
    pub rir: (u8, u8),
    /// Load as a fraction of working weight (lo, hi).
    pub load_frac_of_working: (f64, f64),
}

/// The standard hypertrophy deload week: ~half the sets, 2–4+ RIR, loads
/// ~60–70% of working weight, movement patterns kept (File 03 hypertrophy-036;
/// HYP-DELOAD-RX-001, ExpertOpinion).
pub fn deload_rx() -> Recommended<DeloadRx> {
    recommend(
        DeloadRx {
            sets_fraction: 0.50,
            rir: (2, 4),
            load_frac_of_working: (0.60, 0.70),
        },
        "HYP-DELOAD-RX-001",
    )
}

/// Increase rest when per-set reps fall > ~10% set-to-set (File 03
/// hypertrophy-039): the mechanism is preserving per-set volume, not rest
/// duration itself. `true` = lengthen the rest interval.
pub fn increase_rest_on_rep_drop(rep_drop_frac: f64) -> Recommended<bool> {
    recommend(rep_drop_frac > 0.10, "HYP-REST-001")
}

// ---------------------------------------------------------------------------
// 6. Load interchangeability + effort/RIR rules (File 03
//    hypertrophy-012/017/018/020/021/022/023)
// ---------------------------------------------------------------------------

/// Load-interchangeability window (File 03 hypertrophy-012; HYP-LOADRANGE-001,
/// Strong, contested CQ-F03-02): hypertrophy is equivalent across ~30–85% 1RM
/// (~5–30+ reps) when sets are taken close to failure. The rep upper bound is
/// the KB's "30+" lower bound; heavy loading still favors strength.
pub fn interchangeable_load_range() -> Recommended<RepLoad> {
    recommend(
        RepLoad {
            reps: (5, 30),
            pct_1rm: (30, 85),
        },
        "HYP-LOADRANGE-001",
    )
}

/// `true` when a load sits below the ~30% 1RM floor under which hypertrophy
/// underperforms (File 03 hypertrophy-012; HYP-LOADRANGE-001).
pub fn load_below_effective_floor(pct_1rm: u8) -> Recommended<bool> {
    recommend(pct_1rm < 30, "HYP-LOADRANGE-001")
}

/// Technique-protecting floors for high-skill/high-stability exercises
/// (File 03 hypertrophy-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighSkillGuard {
    /// Keep reps at or above this (KB: reps ≥5).
    pub min_reps: u8,
    /// Stop at or above this RIR band (KB: ≥1–2 RIR).
    pub min_rir: (u8, u8),
}

/// High-skill exercise guard (File 03 hypertrophy-017; HYP-SKILL-RIR-001,
/// ExpertOpinion, SAFETY): reps ≥5 and stop at ≥1–2 RIR to protect technique.
pub fn high_skill_guard() -> Recommended<HighSkillGuard> {
    recommend(
        HighSkillGuard {
            min_reps: 5,
            min_rir: (1, 2),
        },
        "HYP-SKILL-RIR-001",
    )
}

/// Default working proximity to failure (File 03 hypertrophy-018;
/// HYP-RIR-DEFAULT-001, Moderate, contested CQ-02 train-to-failure): most sets
/// at 1–3 RIR; true failure neither required nor superior enough to justify
/// its fatigue. Returns `(min, max)` RIR.
pub fn default_rir_band() -> Recommended<(u8, u8)> {
    recommend((1, 3), "HYP-RIR-DEFAULT-001")
}

/// RPE from RIR via the KB identity `RPE = 10 − RIR` (File 03 hypertrophy-018
/// parameters). Pure conversion, not itself a recommendation; saturates at 0.
pub fn rpe_from_rir(rir: u8) -> u8 {
    10u8.saturating_sub(rir)
}

/// How trustworthy a reported RIR is (File 03 hypertrophy-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RirReliability {
    /// Close to failure (0–5 RIR): accurate to ~±1 rep.
    WithinOneRep,
    /// Far from failure (>5 RIR): error exceeds 2 reps.
    ErrorOverTwoReps,
}

/// RIR accuracy model (File 03 hypertrophy-020; HYP-RIR-ACC-001, Moderate):
/// ±1-rep accuracy only at 0–5 RIR; beyond that the error exceeds 2 reps.
pub fn rir_reliability(reported_rir: u8) -> Recommended<RirReliability> {
    let r = if reported_rir <= 5 {
        RirReliability::WithinOneRep
    } else {
        RirReliability::ErrorOverTwoReps
    };
    recommend(r, "HYP-RIR-ACC-001")
}

/// Novice starting proximity (File 03 hypertrophy-020; HYP-RIR-ACC-001):
/// start at 3–4 RIR and calibrate against actual failure. Returns `(min, max)`.
pub fn novice_start_rir() -> Recommended<(u8, u8)> {
    recommend((3, 4), "HYP-RIR-ACC-001")
}

/// Whether training to true failure (0 RIR) is permitted (File 03
/// hypertrophy-021; HYP-FAIL-SAFE-001, ExpertOpinion, SAFETY): reserved for
/// machines and isolation where failure is safe; never on heavy free-weight
/// compounds (e.g. unspotted squat/bench). Anything that is neither a machine
/// nor isolation is conservatively denied.
pub fn failure_allowed(class: ExerciseClass, machine: bool) -> Recommended<bool> {
    recommend(
        machine || matches!(class, ExerciseClass::Isolation),
        "HYP-FAIL-SAFE-001",
    )
}

/// Effort-signal weighting on a cut (File 03 hypertrophy-022; HYP-CUT-OBJ-001,
/// Moderate): `true` = weight objective rep count and bar speed over perceived
/// effort, because RPE inflates in a deficit. No numeric thresholds stated.
pub fn trust_objective_over_rpe(cutting: bool) -> Recommended<bool> {
    recommend(cutting, "HYP-CUT-OBJ-001")
}

/// Velocity cross-check on failure proximity (File 03 hypertrophy-023;
/// HYP-VEL-CHECK-001, Moderate): last-rep bar-speed slowdown is an objective
/// signal of nearing failure. The KB states no numeric velocity threshold, so
/// the shell/caller decides what counts as a slowdown; `true` = near failure.
pub fn near_failure_from_last_rep_slowdown(last_rep_slowed: bool) -> Recommended<bool> {
    recommend(last_rep_slowed, "HYP-VEL-CHECK-001")
}

// ---------------------------------------------------------------------------
// 7. Exercise selection + substitution (File 03 hypertrophy-027/028/029/030;
//    Table 4)
// ---------------------------------------------------------------------------

/// Equipment category used by the Table 4 map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Equipment {
    Barbell,
    Dumbbell,
    Machine,
    Cable,
    Bodyweight,
}

/// Long-muscle-length bias tier from Table 4 (ordered: later = longer-length).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LongLengthBias {
    Low,
    LowToModerate,
    Moderate,
    High,
}

/// Stimulus-to-fatigue tier (File 03 hypertrophy-027, categorical).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SfrTier {
    Low,
    High,
}

/// One row of File 03 Table 4 (exercise → primary muscle → equipment →
/// long-length bias), transcribed verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExerciseEntry {
    pub name: &'static str,
    /// Primary muscles, lowercase as in Table 4.
    pub primary_muscles: &'static [&'static str],
    pub equipment: &'static [Equipment],
    /// Exercise class per Table 2's own examples (squat/deadlift/press/row =
    /// heavy compound; machines = moderate; curls/raises/flys/extensions =
    /// isolation).
    pub class: ExerciseClass,
    pub long_length: LongLengthBias,
    /// Verbatim Table 4 qualifier (e.g. "High (deep)", "High at top").
    pub long_length_note: &'static str,
}

impl ExerciseEntry {
    /// SFR tier when performed on the given equipment (File 03 hypertrophy-027,
    /// categorical): machines, cables, and stable isolation are high-SFR;
    /// heavy deadlifts / high-skill free weights are low-SFR.
    pub fn sfr_tier_with(&self, used: &[Equipment]) -> SfrTier {
        let machine_or_cable = used
            .iter()
            .any(|e| matches!(e, Equipment::Machine | Equipment::Cable));
        if machine_or_cable || matches!(self.class, ExerciseClass::Isolation) {
            SfrTier::High
        } else {
            SfrTier::Low
        }
    }

    /// Stability when performed on the given equipment: machine/cable paths
    /// are externally stabilized; free-weight/bodyweight are not (derived from
    /// hypertrophy-027's "stable isolation / machines / cables" grouping).
    pub fn stable_with(&self, used: &[Equipment]) -> bool {
        used.iter()
            .any(|e| matches!(e, Equipment::Machine | Equipment::Cable))
    }
}

use Equipment::{Barbell, Bodyweight, Cable, Dumbbell, Machine};

/// File 03 Table 4, major exercise → primary muscle → equipment map,
/// transcribed verbatim (16 rows). Bias tiers: "High (deep)" / "High if
/// seated upright" / "High at top" → High (note kept); "Low–moderate" →
/// LowToModerate; "Low (cable better at bottom)" → Low.
pub static EXERCISES: &[ExerciseEntry] = &[
    ExerciseEntry {
        name: "Barbell back squat",
        primary_muscles: &["quads", "glutes"],
        equipment: &[Barbell],
        class: ExerciseClass::HeavyCompound,
        long_length: LongLengthBias::Moderate,
        long_length_note: "Moderate",
    },
    ExerciseEntry {
        name: "Hack squat / leg press",
        primary_muscles: &["quads"],
        equipment: &[Machine],
        class: ExerciseClass::ModerateCompound,
        long_length: LongLengthBias::High,
        long_length_note: "High (deep)",
    },
    ExerciseEntry {
        name: "Leg extension",
        primary_muscles: &["quads"],
        equipment: &[Machine],
        class: ExerciseClass::Isolation,
        long_length: LongLengthBias::High,
        long_length_note: "High if seated upright",
    },
    ExerciseEntry {
        name: "Romanian deadlift",
        primary_muscles: &["hamstrings", "glutes"],
        equipment: &[Barbell, Dumbbell],
        class: ExerciseClass::HeavyCompound,
        long_length: LongLengthBias::High,
        long_length_note: "High",
    },
    ExerciseEntry {
        name: "Seated leg curl",
        primary_muscles: &["hamstrings"],
        equipment: &[Machine],
        class: ExerciseClass::Isolation,
        long_length: LongLengthBias::High,
        long_length_note: "High",
    },
    ExerciseEntry {
        name: "Hip thrust",
        primary_muscles: &["glutes"],
        equipment: &[Barbell, Machine],
        class: ExerciseClass::ModerateCompound,
        long_length: LongLengthBias::LowToModerate,
        long_length_note: "Low–moderate",
    },
    ExerciseEntry {
        name: "Bench press",
        primary_muscles: &["chest", "triceps", "front delt"],
        equipment: &[Barbell, Dumbbell],
        class: ExerciseClass::HeavyCompound,
        long_length: LongLengthBias::Moderate,
        long_length_note: "Moderate",
    },
    ExerciseEntry {
        name: "Incline press",
        primary_muscles: &["upper chest"],
        equipment: &[Barbell, Dumbbell, Machine],
        class: ExerciseClass::HeavyCompound,
        long_length: LongLengthBias::Moderate,
        long_length_note: "Moderate",
    },
    ExerciseEntry {
        name: "Cable fly / pec deck",
        primary_muscles: &["chest"],
        equipment: &[Cable, Machine],
        class: ExerciseClass::Isolation,
        long_length: LongLengthBias::High,
        long_length_note: "High",
    },
    ExerciseEntry {
        name: "Pull-up / lat pulldown",
        primary_muscles: &["lats"],
        equipment: &[Bodyweight, Cable, Machine],
        class: ExerciseClass::ModerateCompound,
        long_length: LongLengthBias::High,
        long_length_note: "High at top",
    },
    ExerciseEntry {
        name: "Row (barbell/cable/machine)",
        primary_muscles: &["mid-back", "lats"],
        equipment: &[Barbell, Dumbbell, Machine, Cable, Bodyweight],
        class: ExerciseClass::HeavyCompound,
        long_length: LongLengthBias::Moderate,
        long_length_note: "Moderate",
    },
    ExerciseEntry {
        name: "Overhead press",
        primary_muscles: &["delts", "triceps"],
        equipment: &[Barbell, Dumbbell, Machine],
        class: ExerciseClass::HeavyCompound,
        long_length: LongLengthBias::Moderate,
        long_length_note: "Moderate",
    },
    ExerciseEntry {
        name: "Lateral raise",
        primary_muscles: &["side delts"],
        equipment: &[Dumbbell, Cable, Machine],
        class: ExerciseClass::Isolation,
        long_length: LongLengthBias::Low,
        long_length_note: "Low (cable better at bottom)",
    },
    ExerciseEntry {
        name: "Overhead cable/DB triceps ext",
        primary_muscles: &["triceps"],
        equipment: &[Cable, Dumbbell],
        class: ExerciseClass::Isolation,
        long_length: LongLengthBias::High,
        long_length_note: "High",
    },
    ExerciseEntry {
        name: "Incline DB curl",
        primary_muscles: &["biceps"],
        equipment: &[Dumbbell],
        class: ExerciseClass::Isolation,
        long_length: LongLengthBias::High,
        long_length_note: "High",
    },
    ExerciseEntry {
        name: "Standing calf raise",
        primary_muscles: &["gastrocnemius"],
        equipment: &[Machine, Bodyweight],
        class: ExerciseClass::Isolation,
        long_length: LongLengthBias::High,
        long_length_note: "High",
    },
];

/// Case-insensitive muscle match: exact, or the query as a substring of the
/// listed muscle ("back" ⊂ "mid-back", "chest" ⊂ "upper chest"). Lookup
/// plumbing only, not a KB claim.
fn muscle_matches(query: &str, listed: &str) -> bool {
    let q = query.to_ascii_lowercase();
    let l = listed.to_ascii_lowercase();
    l == q || l.contains(&q)
}

/// Look up a Table 4 row by exercise name (case-insensitive).
pub fn exercise_entry(name: &str) -> Option<&'static ExerciseEntry> {
    EXERCISES.iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

/// Long-muscle-length bias for a Table 4 exercise (File 03 hypertrophy-028;
/// HYP-LENGTHSEL-001, Moderate, Wolf 2025 notes the effect shrinks in
/// trained subjects). `None` if the exercise is not in Table 4.
pub fn long_length_bias(exercise: &str) -> Option<Recommended<LongLengthBias>> {
    exercise_entry(exercise).map(|e| recommend(e.long_length, "HYP-LENGTHSEL-001"))
}

/// Bodyweight fallbacks when no equipment match exists (File 03
/// hypertrophy-029, verbatim: quads→Bulgarian/sissy; chest→push-up/dip;
/// back→inverted row/pull-up; hams→Nordic).
fn bodyweight_fallback(muscle: &str) -> Option<&'static str> {
    let m = muscle.to_ascii_lowercase();
    match m.as_str() {
        "quads" => Some("Bulgarian split squat / sissy squat"),
        "chest" => Some("Push-up / dip"),
        "back" => Some("Inverted row / pull-up"),
        "hams" | "hamstrings" => Some("Nordic curl"),
        _ => None,
    }
}

/// Rank Table 4 candidates for `muscle` on `available` equipment and return
/// the best substitute (File 03 hypertrophy-029; HYP-SUBST-001, ExpertOpinion):
/// filter to same primary muscle + available equipment, rank long-length bias
/// > SFR > stability (ties keep table order), else fall back to the KB's
/// bodyweight variants. `None` when nothing matches and no fallback exists.
pub fn substitute_exercise(
    muscle: &str,
    available: &[Equipment],
) -> Recommended<Option<&'static str>> {
    let pick = ranked_candidates(muscle, available, None)
        .first()
        .map(|e| e.name)
        .or_else(|| bodyweight_fallback(muscle));
    recommend(pick, "HYP-SUBST-001")
}

/// Shared filter+rank used by hyp-029 substitution and the hyp-030 pain swap:
/// same primary muscle + available equipment, ordered long-length > SFR >
/// stability (hyp-029 ranking), ties keeping Table 4 order.
fn ranked_candidates(
    muscle: &str,
    available: &[Equipment],
    exclude: Option<&str>,
) -> Vec<&'static ExerciseEntry> {
    let mut out: Vec<(&'static ExerciseEntry, Vec<Equipment>)> = EXERCISES
        .iter()
        .filter(|e| exclude.is_none_or(|x| !e.name.eq_ignore_ascii_case(x)))
        .filter(|e| {
            e.primary_muscles
                .iter()
                .any(|m| muscle_matches(muscle, m))
        })
        .map(|e| {
            let usable: Vec<Equipment> = e
                .equipment
                .iter()
                .copied()
                .filter(|eq| available.contains(eq))
                .collect();
            (e, usable)
        })
        .filter(|(_, usable)| !usable.is_empty())
        .collect();
    // Stable sort keeps Table 4 order on ties.
    out.sort_by_key(|(e, usable)| {
        std::cmp::Reverse((
            e.long_length,
            e.sfr_tier_with(usable),
            e.stable_with(usable),
        ))
    });
    out.into_iter().map(|(e, _)| e).collect()
}

/// Movement-pain substitution (File 03 hypertrophy-030; HYP-PAIN-SWAP-001,
/// ExpertOpinion, SAFETY): on movement-specific joint pain, swap to a
/// same-muscle exercise with higher stability (different resistance profile).
/// Candidates sharing the painful exercise's first primary muscle are ranked
/// stability-first, then long-length bias, then SFR. `None` when the painful
/// exercise is unknown or nothing else fits the available equipment.
pub fn pain_driven_swap(
    painful_exercise: &str,
    available: &[Equipment],
) -> Recommended<Option<&'static str>> {
    let pick = exercise_entry(painful_exercise).and_then(|painful| {
        let muscle = painful.primary_muscles.first()?;
        let mut ranked = ranked_candidates(muscle, available, Some(painful.name));
        // Stability outranks everything for a pain-driven swap.
        ranked.sort_by_key(|e| std::cmp::Reverse(e.stable_with(available)));
        ranked.first().map(|e| e.name)
    });
    recommend(pick, "HYP-PAIN-SWAP-001")
}

// ---------------------------------------------------------------------------
// 8. Mesocycle macro-structure + progression drivers (File 03
//    hypertrophy-031/033/037/044)
// ---------------------------------------------------------------------------

/// Mesocycle macro-structure (File 03 hypertrophy-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MesoStructure {
    /// Accumulation length in weeks (min, max).
    pub accumulation_weeks: (u8, u8),
    /// Deload length in weeks.
    pub deload_weeks: u8,
    /// Deload cadence: deload every this many weeks (min, max).
    pub deload_cadence_weeks: (u8, u8),
}

/// Standard mesocycle shape (File 03 hypertrophy-031; HYP-MESO-STRUCT-001,
/// Moderate): 4–6 weeks accumulation + 1 deload week (3:1 to 6:1), deloading
/// every 4–8 weeks.
pub fn meso_structure() -> Recommended<MesoStructure> {
    recommend(
        MesoStructure {
            accumulation_weeks: (4, 6),
            deload_weeks: 1,
            deload_cadence_weeks: (4, 8),
        },
        "HYP-MESO-STRUCT-001",
    )
}

/// Smallest practical load increment for double progression (File 03
/// hypertrophy-033: "~2.5 kg/5 lb").
pub const SMALLEST_LOAD_INCREMENT_KG: f64 = 2.5;

/// Next-session target from the double-progression driver.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DoubleProgressionStep {
    pub reps: u8,
    pub load_kg: f64,
    /// `true` when the load was bumped and reps restarted at the range bottom.
    pub load_increased: bool,
}

/// Double-progression overload driver (File 03 hypertrophy-033;
/// HYP-DOUBLEPROG-001, ExpertOpinion, contested CQ-F03-04): fix a rep range +
/// RIR, add reps weekly; at the top of the range add the smallest increment
/// (~2.5 kg) and restart at the range bottom, holding volume constant.
pub fn double_progression_next(
    rep_range: (u8, u8),
    last_reps: u8,
    load_kg: f64,
) -> Recommended<DoubleProgressionStep> {
    let step = if last_reps >= rep_range.1 {
        DoubleProgressionStep {
            reps: rep_range.0,
            load_kg: load_kg + SMALLEST_LOAD_INCREMENT_KG,
            load_increased: true,
        }
    } else {
        DoubleProgressionStep {
            reps: (last_reps + 1).min(rep_range.1),
            load_kg,
            load_increased: false,
        }
    };
    recommend(step, "HYP-DOUBLEPROG-001")
}

/// The KB's worked double-progression RIR band (File 03 hypertrophy-033
/// example: 10–15 reps @ 2–0 RIR). Returns `(min, max)` RIR.
pub fn double_progression_rir() -> Recommended<(u8, u8)> {
    recommend((0, 2), "HYP-DOUBLEPROG-001")
}

/// Post-layoff MEV reduction flag (File 03 hypertrophy-037; HYP-LAYOFF-001,
/// ExpertOpinion): after a layoff, restart at a lower MEV because landmarks
/// are temporarily reduced. The KB states no numeric reduction factor -
/// documented gap; the engine only flags that MEV must be lowered.
pub fn layoff_reduces_mev(returning_from_layoff: bool) -> Recommended<bool> {
    recommend(returning_from_layoff, "HYP-LAYOFF-001")
}

/// Weekly-set plateau at which an advanced trainee qualifies for a
/// specialization block (File 03 hypertrophy-044: "not responding to ~15 sets").
pub const SPECIALIZATION_STALL_WEEKLY_SETS: u8 = 15;

/// Advanced specialization gate (File 03 hypertrophy-044; HYP-SPEC-BLOCK-001,
/// ExpertOpinion): `true` when an advanced trainee is not growing at ~15
/// weekly sets, then raise one muscle's volume and drop the others to MV.
pub fn specialization_indicated(
    age: TrainingAge,
    weekly_sets: u8,
    not_growing: bool,
) -> Recommended<bool> {
    recommend(
        matches!(age, TrainingAge::Advanced)
            && weekly_sets >= SPECIALIZATION_STALL_WEEKLY_SETS
            && not_growing,
        "HYP-SPEC-BLOCK-001",
    )
}

// ---------------------------------------------------------------------------
// 9. Tempo + time-efficiency (File 03 hypertrophy-040/041)
// ---------------------------------------------------------------------------

/// Controlled-tempo prescription (File 03 hypertrophy-040).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoRx {
    /// Total rep duration window in seconds (KB: ~0.5–8 s).
    pub rep_duration_s: (f64, f64),
    /// Concentric window in seconds (KB: ~1–2 s).
    pub concentric_s: (f64, f64),
    /// Eccentric window in seconds (KB: ~2–3 s).
    pub eccentric_s: (f64, f64),
}

/// Rep-tempo window (File 03 hypertrophy-040; HYP-TEMPO-001, Moderate):
/// controlled ~0.5–8 s/rep (1–2 s concentric, 2–3 s eccentric); tempo has
/// minimal effect on hypertrophy within this window.
pub fn tempo_rx() -> Recommended<TempoRx> {
    recommend(
        TempoRx {
            rep_duration_s: (0.5, 8.0),
            concentric_s: (1.0, 2.0),
            eccentric_s: (2.0, 3.0),
        },
        "HYP-TEMPO-001",
    )
}

/// Superslow gate (File 03 hypertrophy-040; HYP-TEMPO-001): `true` for rep
/// durations >10 s, which force load reduction and are inferior, avoid.
pub fn tempo_is_superslow(rep_duration_s: f64) -> Recommended<bool> {
    recommend(rep_duration_s > 10.0, "HYP-TEMPO-001")
}

/// Time-saving superset prescription (File 03 hypertrophy-041).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupersetRx {
    /// Pair antagonist / non-competing movements only.
    pub antagonist_pairing: bool,
    /// Keep at least this much rest for the working muscle (KB: ≥90 s,
    /// accepting a small volume loss).
    pub min_rest_sec: u16,
}

/// Antagonist supersets when time-limited (File 03 hypertrophy-041;
/// HYP-SUPERSET-001, Moderate): `Some` only under time pressure, antagonist /
/// non-competing pairings with ≥90 s effective rest; `None` otherwise.
pub fn time_limited_superset(time_limited: bool) -> Recommended<Option<SupersetRx>> {
    let rx = time_limited.then_some(SupersetRx {
        antagonist_pairing: true,
        min_rest_sec: 90,
    });
    recommend(rx, "HYP-SUPERSET-001")
}

// ---------------------------------------------------------------------------
// 10. Default program synthesis (File 03 hypertrophy-043)
// ---------------------------------------------------------------------------

/// Intermediate default hypertrophy program (File 03 hypertrophy-043).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultProgramRx {
    /// Sessions per muscle per week.
    pub frequency_per_week: u8,
    /// Weekly sets per muscle at MEV (min, max).
    pub weekly_sets: (u8, u8),
    /// Per-session cap for one muscle.
    pub max_session_sets: u8,
    /// Compound rep window.
    pub compound_reps: (u8, u8),
    /// Isolation rep window.
    pub isolation_reps: (u8, u8),
    /// Week-1 RIR, descending ~1/week…
    pub week1_rir: u8,
    /// …to this final-week RIR.
    pub final_rir: u8,
    /// Compound rest window in seconds.
    pub compound_rest_sec: (u16, u16),
    /// Isolation rest window in seconds.
    pub isolation_rest_sec: (u16, u16),
    /// High-SFR / long-length exercises per muscle (min, max).
    pub exercises_per_muscle: (u8, u8),
    /// Deload arrives in week 5–6 (min, max).
    pub deload_week: (u8, u8),
}

/// The File 03 intermediate default program (hypertrophy-043;
/// HYP-DEFAULT-PROG-001, Moderate synthesis): each muscle 2×/week at MEV
/// (~8–10 sets/week, ≤~8/session), compounds 5–10 reps / isolation 10–20,
/// RIR 3→1 (−1/week), rest 2–3 min compounds / 1–2 min isolation, controlled
/// tempo (see [`tempo_rx`]), 1–2 high-SFR long-length exercises per muscle,
/// deload week 5–6.
pub fn intermediate_default_program() -> Recommended<DefaultProgramRx> {
    recommend(
        DefaultProgramRx {
            frequency_per_week: 2,
            weekly_sets: (8, 10),
            max_session_sets: 8,
            compound_reps: (5, 10),
            isolation_reps: (10, 20),
            week1_rir: 3,
            final_rir: 1,
            compound_rest_sec: (120, 180),
            isolation_rest_sec: (60, 120),
            exercises_per_muscle: (1, 2),
            deload_week: (5, 6),
        },
        "HYP-DEFAULT-PROG-001",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::EvidenceGrade;

    #[test]
    fn landmarks_lookup_is_case_insensitive_and_verbatim() {
        let chest = landmarks_for("Chest").expect("chest present");
        assert_eq!(chest.mev, 10);
        assert_eq!(chest.mav, (12, 20));
        assert_eq!(chest.mrv, 22);
        assert_eq!(landmarks_for("QUADS").unwrap().mv, 6);
        assert!(landmarks_for("tail").is_none());
    }

    #[test]
    fn rep_load_windows_match_table_2() {
        let heavy = rep_load(ExerciseClass::HeavyCompound);
        assert_eq!(heavy.value.reps, (5, 10));
        assert_eq!(heavy.value.pct_1rm, (75, 85));
        assert_eq!(heavy.evidence.grade, EvidenceGrade::Moderate);
        assert_eq!(rep_load(ExerciseClass::Isolation).value.reps, (12, 25));
    }

    #[test]
    fn rest_windows_split_compound_vs_isolation() {
        assert_eq!(rest_sec_for(ExerciseClass::HeavyCompound).value, (120, 180));
        assert_eq!(rest_sec_for(ExerciseClass::Isolation).value, (60, 120));
        // Table 5 "Machine/isolation: 1–2 min" covers the moderate class
        // (Table 2 files it as "moderate compounds / machines").
        assert_eq!(
            rest_sec_for(ExerciseClass::ModerateCompound).value,
            (60, 120)
        );
    }

    #[test]
    fn frequency_bands_follow_table_3() {
        assert_eq!(frequency_for_weekly_sets(8).value.freq, (1, 2));
        assert_eq!(frequency_for_weekly_sets(14).value.freq, (2, 3));
        assert_eq!(frequency_for_weekly_sets(24).value.freq, (3, 3));
        // Frequency evidence is Strong (2x/week beats 1x).
        assert_eq!(
            frequency_for_weekly_sets(14).evidence.grade,
            EvidenceGrade::Strong
        );
    }

    #[test]
    fn set_ramp_matches_rp_worked_example() {
        let ramp = weekly_set_ramp(10, 20, 4);
        assert_eq!(ramp.value, vec![10, 13, 16, 20]);
        // Degenerate cases.
        assert_eq!(weekly_set_ramp(10, 20, 1).value, vec![10]);
        assert!(weekly_set_ramp(10, 20, 0).value.is_empty());
        // MRV below MEV clamps flat.
        assert_eq!(weekly_set_ramp(12, 8, 3).value, vec![12, 12, 12]);
    }

    #[test]
    fn set_ramp_never_jumps_more_than_four_sets_per_week() {
        // hyp-001 increment cap (+2–4/wk) + hyp-011 no-abrupt-jump guard:
        // a short block over a wide span must not emit +10/wk steps or land
        // straight on MRV.
        let ramp = weekly_set_ramp(10, 30, 3).value;
        assert_eq!(ramp, vec![10, 14, 18]);
        for w in ramp.windows(2) {
            assert!(w[1] - w[0] <= MAX_WEEKLY_SET_INCREMENT);
        }
        // Ramps within the cap are untouched (RP worked example covered above).
        let ok = weekly_set_ramp(10, 20, 4).value;
        assert_eq!(ok, vec![10, 13, 16, 20]);
    }

    #[test]
    fn abrupt_volume_jump_guard_is_safety_flagged() {
        assert!(abrupt_volume_jump(10, 20).value);
        assert!(!abrupt_volume_jump(10, 14).value);
        assert!(!abrupt_volume_jump(14, 10).value); // reductions are not jumps
        let g = abrupt_volume_jump(10, 20);
        assert!(g.confidence.safety_critical);
        assert_eq!(g.evidence.grade, EvidenceGrade::ExpertOpinion);
    }

    #[test]
    fn growth_ceiling_and_min_sets() {
        assert_eq!(WEEKLY_FRACTIONAL_SET_CEILING, 31);
        assert_eq!(MIN_SETS_PER_EXERCISE, 2);
        assert_eq!(cap_weekly_growth_target(40).value, 31);
        assert_eq!(cap_weekly_growth_target(20).value, 20);
    }

    #[test]
    fn mev_scales_with_training_age() {
        assert_eq!(mev_sets_by_training_age(TrainingAge::Novice).value, (6, 10));
        assert_eq!(
            mev_sets_by_training_age(TrainingAge::Intermediate).value,
            (10, 18)
        );
        assert_eq!(
            mev_sets_by_training_age(TrainingAge::Advanced).value,
            (12, 20)
        );
    }

    #[test]
    fn next_meso_adds_two_when_stalled_and_fresh() {
        assert_eq!(next_meso_weekly_sets(12, true, true).value, 14);
        assert_eq!(next_meso_weekly_sets(12, true, false).value, 12);
        assert_eq!(next_meso_weekly_sets(30, true, true).value, 31); // capped
    }

    #[test]
    fn over_mrv_and_recovery_scaling() {
        assert!(over_mrv_deload(22, true, false).value);
        assert!(over_mrv_deload(22, false, true).value);
        assert!(!over_mrv_deload(18, true, true).value); // under 20 sets
        assert!(!over_mrv_deload(22, false, false).value); // no symptom
        // Low recovery scales to 70–80%.
        let (lo, hi) = recovery_adjusted_volume(20, true).value;
        assert!((lo - 14.0).abs() < 1e-9 && (hi - 16.0).abs() < 1e-9);
        assert_eq!(recovery_adjusted_volume(20, false).value, (20.0, 20.0));
    }

    #[test]
    fn joint_pain_shifts_to_light_high_reps() {
        let rx = joint_pain_rep_shift().value;
        assert_eq!(rx.reps, (12, 25));
        assert_eq!(rx.pct_1rm, (50, 70));
    }

    #[test]
    fn split_triggers_and_deload() {
        assert!(needs_session_split(13).value);
        assert!(!needs_session_split(12).value);
        assert!(per_session_over_cap(12).value);
        assert!(!per_session_over_cap(11).value);
        assert!(deload_indicated(2).value);
        assert!(!deload_indicated(1).value);
        let d = deload_rx().value;
        assert!((d.sets_fraction - 0.50).abs() < 1e-9);
        assert_eq!(d.rir, (2, 4));
        assert_eq!(d.load_frac_of_working, (0.60, 0.70));
        assert!(increase_rest_on_rep_drop(0.12).value);
        assert!(!increase_rest_on_rep_drop(0.08).value);
    }

    #[test]
    fn rir_ramp_descends_to_one() {
        assert_eq!(rir_for_week(1, 4).unwrap().value, 4);
        assert_eq!(rir_for_week(2, 4).unwrap().value, 3);
        assert_eq!(rir_for_week(3, 4).unwrap().value, 2);
        assert_eq!(rir_for_week(4, 4).unwrap().value, 1);
        assert!(rir_for_week(0, 4).is_none());
        assert!(rir_for_week(5, 4).is_none());
        // ExpertOpinion-graded schedule.
        assert_eq!(
            rir_for_week(1, 4).unwrap().evidence.grade,
            EvidenceGrade::ExpertOpinion
        );
    }

    #[test]
    fn rir_ramp_never_extrapolates_past_kb_schedule() {
        // hyp-020: RIR only ±1-rep accurate at 0–5; the hyp-019 schedule
        // starts at 4. Longer blocks hold 4, never 5+.
        assert_eq!(rir_for_week(1, 8).unwrap().value, 4);
        assert_eq!(rir_for_week(4, 8).unwrap().value, 4);
        assert_eq!(rir_for_week(5, 8).unwrap().value, 4);
        assert_eq!(rir_for_week(6, 8).unwrap().value, 3);
        assert_eq!(rir_for_week(8, 8).unwrap().value, 1);
        for block in 1..=12u8 {
            for week in 1..=block {
                assert!(rir_for_week(week, block).unwrap().value <= MAX_SCHEDULED_RIR);
            }
        }
    }

    // -- hyp-012 --
    #[test]
    fn load_is_interchangeable_30_to_85_pct() {
        let r = interchangeable_load_range();
        assert_eq!(r.value.reps, (5, 30));
        assert_eq!(r.value.pct_1rm, (30, 85));
        assert_eq!(r.evidence.grade, EvidenceGrade::Strong);
        assert!(r.confidence.contested);
        assert_eq!(
            r.confidence.contested_question_ref.as_deref(),
            Some("CQ-F03-02")
        );
        assert!(load_below_effective_floor(29).value);
        assert!(!load_below_effective_floor(30).value);
    }

    // -- hyp-017 --
    #[test]
    fn high_skill_lifts_keep_reps_and_rir_floors() {
        let g = high_skill_guard();
        assert_eq!(g.value.min_reps, 5);
        assert_eq!(g.value.min_rir, (1, 2));
        assert!(g.confidence.safety_critical);
        assert_eq!(g.evidence.grade, EvidenceGrade::ExpertOpinion);
    }

    // -- hyp-018 --
    #[test]
    fn default_rir_is_one_to_three_and_contested() {
        let d = default_rir_band();
        assert_eq!(d.value, (1, 3));
        assert_eq!(d.evidence.grade, EvidenceGrade::Moderate);
        assert!(d.confidence.contested);
        assert_eq!(d.confidence.contested_question_ref.as_deref(), Some("CQ-02"));
        // RPE = 10 − RIR identity.
        assert_eq!(rpe_from_rir(2), 8);
        assert_eq!(rpe_from_rir(0), 10);
        assert_eq!(rpe_from_rir(12), 0); // saturates
    }

    // -- hyp-020 --
    #[test]
    fn rir_accuracy_degrades_far_from_failure() {
        assert_eq!(rir_reliability(0).value, RirReliability::WithinOneRep);
        assert_eq!(rir_reliability(5).value, RirReliability::WithinOneRep);
        assert_eq!(rir_reliability(6).value, RirReliability::ErrorOverTwoReps);
        assert_eq!(novice_start_rir().value, (3, 4));
        assert_eq!(
            novice_start_rir().evidence.grade,
            EvidenceGrade::Moderate
        );
    }

    // -- hyp-021 --
    #[test]
    fn failure_reserved_for_machines_and_isolation() {
        // Machines and isolation: allowed.
        assert!(failure_allowed(ExerciseClass::Isolation, false).value);
        assert!(failure_allowed(ExerciseClass::HeavyCompound, true).value); // e.g. hack squat
        // Free-weight compounds (unspotted squat/bench): never.
        assert!(!failure_allowed(ExerciseClass::HeavyCompound, false).value);
        assert!(!failure_allowed(ExerciseClass::ModerateCompound, false).value);
        let f = failure_allowed(ExerciseClass::HeavyCompound, false);
        assert!(f.confidence.safety_critical);
    }

    // -- hyp-022/023 --
    #[test]
    fn cut_trusts_objective_signals_and_velocity_cross_check() {
        assert!(trust_objective_over_rpe(true).value);
        assert!(!trust_objective_over_rpe(false).value);
        assert_eq!(
            trust_objective_over_rpe(true).evidence.grade,
            EvidenceGrade::Moderate
        );
        assert!(near_failure_from_last_rep_slowdown(true).value);
        assert!(!near_failure_from_last_rep_slowdown(false).value);
    }

    // -- hyp-027/028 + Table 4 --
    #[test]
    fn table_4_is_complete_and_verbatim() {
        assert_eq!(EXERCISES.len(), 16);
        let slc = exercise_entry("Seated leg curl").unwrap();
        assert_eq!(slc.primary_muscles, &["hamstrings"]);
        assert_eq!(slc.equipment, &[Machine]);
        assert_eq!(slc.long_length, LongLengthBias::High);
        let lr = exercise_entry("Lateral raise").unwrap();
        assert_eq!(lr.long_length, LongLengthBias::Low);
        assert_eq!(lr.long_length_note, "Low (cable better at bottom)");
        assert_eq!(
            exercise_entry("Hip thrust").unwrap().long_length,
            LongLengthBias::LowToModerate
        );
    }

    #[test]
    fn sfr_favors_machines_cables_and_stable_isolation() {
        let squat = exercise_entry("Barbell back squat").unwrap();
        assert_eq!(squat.sfr_tier_with(&[Barbell]), SfrTier::Low);
        let hack = exercise_entry("Hack squat / leg press").unwrap();
        assert_eq!(hack.sfr_tier_with(&[Machine]), SfrTier::High);
        // Stable isolation is high-SFR even on free weights.
        let curl = exercise_entry("Incline DB curl").unwrap();
        assert_eq!(curl.sfr_tier_with(&[Dumbbell]), SfrTier::High);
    }

    #[test]
    fn long_length_bias_lookup_is_graded_moderate() {
        let b = long_length_bias("Seated leg curl").unwrap();
        assert_eq!(b.value, LongLengthBias::High);
        assert_eq!(b.evidence.grade, EvidenceGrade::Moderate);
        assert!(long_length_bias("Zercher carry").is_none());
    }

    // -- hyp-029 --
    #[test]
    fn substitution_filters_ranks_and_falls_back_to_bodyweight() {
        // Machine-only hamstrings → seated leg curl (RDL is barbell/DB).
        assert_eq!(
            substitute_exercise("hamstrings", &[Machine]).value,
            Some("Seated leg curl")
        );
        // Machine-only quads: hack squat and leg extension are both
        // long-length High + high-SFR; table order breaks the tie.
        assert_eq!(
            substitute_exercise("quads", &[Machine]).value,
            Some("Hack squat / leg press")
        );
        // Barbell chest: bench vs incline (both Moderate) → table order.
        assert_eq!(
            substitute_exercise("chest", &[Barbell]).value,
            Some("Bench press")
        );
        // "back" matches Table 4's "mid-back" (Row is the only such entry).
        assert_eq!(
            substitute_exercise("back", &[Cable]).value,
            Some("Row (barbell/cable/machine)")
        );
        // No equipment → KB bodyweight fallbacks.
        assert_eq!(
            substitute_exercise("quads", &[]).value,
            Some("Bulgarian split squat / sissy squat")
        );
        assert_eq!(substitute_exercise("hamstrings", &[]).value, Some("Nordic curl"));
        assert_eq!(substitute_exercise("chest", &[]).value, Some("Push-up / dip"));
        assert_eq!(
            substitute_exercise("back", &[]).value,
            Some("Inverted row / pull-up")
        );
        // No match, no fallback.
        assert_eq!(substitute_exercise("neck", &[]).value, None);
        assert_eq!(
            substitute_exercise("quads", &[Machine]).evidence.grade,
            EvidenceGrade::ExpertOpinion
        );
    }

    // -- hyp-030 --
    #[test]
    fn pain_swaps_to_same_muscle_higher_stability() {
        // Painful barbell squat, machine available → stabilized quad exercise.
        let swap = pain_driven_swap("Barbell back squat", &[Machine]);
        assert_eq!(swap.value, Some("Hack squat / leg press"));
        assert!(swap.confidence.safety_critical);
        // Unknown exercise → no swap.
        assert_eq!(pain_driven_swap("Zercher carry", &[Machine]).value, None);
        // Stability-first: with everything available the machine path outranks
        // free-weight candidates for a painful free-weight bench press.
        let all = [Barbell, Dumbbell, Machine, Cable, Bodyweight];
        let bench_swap = pain_driven_swap("Bench press", &all).value.unwrap();
        let entry = exercise_entry(bench_swap).unwrap();
        assert!(entry.stable_with(&all));
    }

    // -- hyp-031 --
    #[test]
    fn meso_structure_is_4_to_6_plus_1_deload() {
        let m = meso_structure().value;
        assert_eq!(m.accumulation_weeks, (4, 6));
        assert_eq!(m.deload_weeks, 1);
        assert_eq!(m.deload_cadence_weeks, (4, 8));
        assert_eq!(meso_structure().evidence.grade, EvidenceGrade::Moderate);
    }

    // -- hyp-033 --
    #[test]
    fn double_progression_adds_reps_then_load() {
        // Below the top: add a rep, hold load.
        let mid = double_progression_next((10, 15), 12, 40.0).value;
        assert_eq!(mid.reps, 13);
        assert!((mid.load_kg - 40.0).abs() < 1e-9);
        assert!(!mid.load_increased);
        // Top of range: +2.5 kg, restart at the bottom.
        let top = double_progression_next((10, 15), 15, 40.0).value;
        assert_eq!(top.reps, 10);
        assert!((top.load_kg - 42.5).abs() < 1e-9);
        assert!(top.load_increased);
        assert_eq!(double_progression_rir().value, (0, 2));
        let d = double_progression_next((10, 15), 15, 40.0);
        assert!(d.confidence.contested);
        assert_eq!(
            d.confidence.contested_question_ref.as_deref(),
            Some("CQ-F03-04")
        );
    }

    // -- hyp-037 --
    #[test]
    fn layoff_flags_reduced_mev() {
        assert!(layoff_reduces_mev(true).value);
        assert!(!layoff_reduces_mev(false).value);
        assert_eq!(
            layoff_reduces_mev(true).evidence.grade,
            EvidenceGrade::ExpertOpinion
        );
    }

    // -- hyp-040 --
    #[test]
    fn tempo_window_and_superslow_gate() {
        let t = tempo_rx().value;
        assert_eq!(t.rep_duration_s, (0.5, 8.0));
        assert_eq!(t.concentric_s, (1.0, 2.0));
        assert_eq!(t.eccentric_s, (2.0, 3.0));
        assert!(tempo_is_superslow(10.5).value);
        assert!(!tempo_is_superslow(8.0).value);
        assert_eq!(tempo_rx().evidence.grade, EvidenceGrade::Moderate);
    }

    // -- hyp-041 --
    #[test]
    fn supersets_only_when_time_limited() {
        let rx = time_limited_superset(true).value.unwrap();
        assert!(rx.antagonist_pairing);
        assert_eq!(rx.min_rest_sec, 90);
        assert_eq!(time_limited_superset(false).value, None);
    }

    // -- hyp-043 --
    #[test]
    fn intermediate_default_program_matches_synthesis() {
        let p = intermediate_default_program();
        assert_eq!(p.value.frequency_per_week, 2);
        assert_eq!(p.value.weekly_sets, (8, 10));
        assert_eq!(p.value.max_session_sets, 8);
        assert_eq!(p.value.compound_reps, (5, 10));
        assert_eq!(p.value.isolation_reps, (10, 20));
        assert_eq!(p.value.week1_rir, 3);
        assert_eq!(p.value.final_rir, 1);
        assert_eq!(p.value.compound_rest_sec, (120, 180));
        assert_eq!(p.value.isolation_rest_sec, (60, 120));
        assert_eq!(p.value.exercises_per_muscle, (1, 2));
        assert_eq!(p.value.deload_week, (5, 6));
        assert_eq!(p.evidence.grade, EvidenceGrade::Moderate);
    }

    // -- hyp-044 --
    #[test]
    fn specialization_gate_requires_advanced_stalled_at_15() {
        assert!(specialization_indicated(TrainingAge::Advanced, 15, true).value);
        assert!(!specialization_indicated(TrainingAge::Intermediate, 15, true).value);
        assert!(!specialization_indicated(TrainingAge::Advanced, 14, true).value);
        assert!(!specialization_indicated(TrainingAge::Advanced, 15, false).value);
        assert_eq!(
            specialization_indicated(TrainingAge::Advanced, 15, true)
                .evidence
                .grade,
            EvidenceGrade::ExpertOpinion
        );
    }
}
