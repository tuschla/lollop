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
//! wrapped in [`Recommended`] via [`Recommended::new`], which forces attached evidence
//! and confidence from the compile-time registry (`crate::evidence`). Claim ids:
//! HYP-VOL-001, HYP-LANDMARKS-001, HYP-REPLOAD-001, HYP-FREQ-001, HYP-REST-001,
//! HYP-REST-002, HYP-RIR-RAMP-001, HYP-VOL-DOSE-001, plus the "File 03
//! (hypertrophy) rule claims" block in `crate::evidence` (HYP-LOADRANGE/
//! VOLRAMP-SAFE/SKILL-RIR/RIR-DEFAULT/RIR-ACC/FAIL-SAFE/CUT-OBJ/VEL-CHECK/SFR/
//! LENGTHSEL/SUBST/PAIN-SWAP/MESO-STRUCT/DOUBLEPROG/LAYOFF/TEMPO/SUPERSET/
//! DEFAULT-PROG/SPEC-BLOCK-001). Section 7b adds the any-name class / muscle
//! resolution, fractional delivered volume, block targets, specialization and
//! double progression the hypertrophy week (`plan::hypertrophy`) is built on.

use crate::evidence;
use crate::evidence::claims;
use crate::individualization::TrainingAge;
use crate::schema::Recommended;

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

/// Bridge the Table 4 exercise-muscle vocabulary onto the Table 1 landmark keys:
/// Table 4 names some heads (upper chest, lats/mid-back, gastroc) that Table 1
/// folds into a real group containment (chest, back, calves). Lookup plumbing,
/// not a KB claim. Unmapped names pass through unchanged.
///
/// Delts are NOT folded (HARD RULE 1): Table 1 has ONLY "Side delts" and
/// "Rear delts" rows — the front delt is a distinct head, not contained in the
/// side delts. So only explicit side-delt phrasing resolves to the Side delts
/// row and explicit rear-delt phrasing to the Rear delts row (both already via
/// the identity pass-through); the generic "delts" (e.g. Overhead press) and
/// "front delt" carry no Table 1 row and fall through to `None`, i.e. the honest
/// no-landmark fallback rather than borrowing Side delts' permissive band.
fn landmark_key(muscle: &str) -> &str {
    if muscle.eq_ignore_ascii_case("upper chest") {
        "chest"
    } else if muscle.eq_ignore_ascii_case("lats") || muscle.eq_ignore_ascii_case("mid-back") {
        "back"
    } else if muscle.eq_ignore_ascii_case("gastrocnemius") {
        "calves"
    } else {
        muscle
    }
}

/// Resolve an exercise (Table 4 name) to its primary (first-listed) muscle's
/// Table 1 volume landmarks (HYP-LANDMARKS-001). `None` when the exercise is
/// unknown to Table 4 or its primary muscle has no Table 1 entry (callers then
/// fall back to the generic training-age band). A secondary muscle never lends
/// its landmark row when the primary carries none: borrowing that band would
/// overstate direct volume for the primary target (e.g. Overhead press's
/// primary "delts" has no Table 1 row, so it falls through to `None` rather
/// than borrowing triceps).
pub fn landmarks_for_exercise(exercise: &str) -> Option<&'static VolumeLandmarks> {
    let entry = exercise_entry(exercise)?;
    let primary = entry.primary_muscles.first()?;
    landmarks_for(landmark_key(primary))
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
    evidence::graded(rl, claims::HYP_REPLOAD_001)
}

/// Between-set rest window in seconds for an exercise class (File 03 Table 5,
/// refined by the published Singer 2024 meta-analysis; HYP-REST-002, Moderate).
/// Rest longer than 60 s gives a small growth benefit (more reps kept per set);
/// beyond ~90 s the extra benefit is uncertain. So machine/isolation work and
/// moderate compounds take a ~90 s floor (Table 5 "Machine/isolation: 1–2 min",
/// time-constrained floor "≥90 s") and heavy compounds 2–3 min. The goal is
/// keeping >=90% of first-set reps, not the clock itself.
pub fn rest_sec_for(class: ExerciseClass) -> Recommended<(u16, u16)> {
    let window = match class {
        ExerciseClass::HeavyCompound => (120, 180),
        ExerciseClass::ModerateCompound | ExerciseClass::Isolation => (90, 120),
    };
    evidence::graded(window, claims::HYP_REST_002)
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
            // per_session floor of 1 is inferred: KB Table 3 gives only the
            // "≤6-8" per-session ceiling for this band, no floor.
            per_session: (1, 8),
        }
    } else if weekly_sets <= 18 {
        FrequencyRx {
            freq: (2, 3),
            per_session: (5, 8),
        }
    } else {
        // Table 3 tops out at 26 weekly sets; 27-31 (fractional-ceiling volumes)
        // reuse this top 18-26 row, extrapolated, not KB-stated.
        FrequencyRx {
            freq: (3, 3),
            per_session: (6, 9),
        }
    };
    evidence::graded(rx, claims::HYP_FREQ_001)
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
/// contested, contradicted by Enes 2024). Linear interpolation, floored, so
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
    evidence::graded(ramp, claims::HYP_SETRAMP_001)
}

/// hyp-011 no-abrupt-jump guard (HYP-VOLRAMP-SAFE-001, ExpertOpinion, SAFETY):
/// `true` when a proposed week-over-week weekly-set change exceeds the hyp-001
/// +2–4 sets/week climb (i.e. > +4), which raises injury risk and must be
/// rejected. The KB states no numeric bound of its own; the +4 cap is
/// hyp-001's upper increment.
pub fn abrupt_volume_jump(prev_weekly_sets: u8, next_weekly_sets: u8) -> Recommended<bool> {
    evidence::graded(
        next_weekly_sets.saturating_sub(prev_weekly_sets) > MAX_WEEKLY_SET_INCREMENT,
        claims::HYP_VOLRAMP_SAFE_001,
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
    Some(evidence::graded(rir, claims::HYP_RIR_RAMP_001))
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
    evidence::graded(
        weekly_sets.min(WEEKLY_FRACTIONAL_SET_CEILING),
        claims::HYP_VOL_001,
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
    evidence::graded(band, claims::HYP_MEV_AGE_001)
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
        // Saturating add: a caller-supplied current-set count near u8::MAX
        // must not debug-overflow (it is capped at the ceiling below anyway).
        current_weekly_sets
            .saturating_add(2)
            .min(WEEKLY_FRACTIONAL_SET_CEILING)
    } else {
        current_weekly_sets
    };
    evidence::graded(next, claims::HYP_MESO_ADD_001)
}

/// Generic per-muscle MRV anchor (~20 weekly sets, the RP framework's figure)
/// for a muscle with no Table 1 landmarks (File 03 hypertrophy-009;
/// HYP-MRV-DELOAD-001).
pub const GENERIC_MRV_SETS: u8 = 20;

/// At/over-MRV deload gate (File 03 hypertrophy-009; HYP-MRV-DELOAD-001,
/// Moderate): weekly sets above the muscle's own MRV (Table 1 / `LANDMARKS`,
/// HYP-LANDMARKS-001, e.g. glutes 16, side delts/biceps 26) with regressing
/// performance OR aching joints → treat as over MRV and deload. An
/// unknown/unspecified `muscle` (e.g. a generic per-muscle prompt with no
/// specific muscle in scope) falls back to [`GENERIC_MRV_SETS`].
pub fn over_mrv_deload(
    muscle: &str,
    weekly_sets: u8,
    performance_down: bool,
    joint_ache: bool,
) -> Recommended<bool> {
    let mrv = landmarks_for(landmark_key(muscle)).map_or(GENERIC_MRV_SETS, |l| l.mrv);
    evidence::graded(
        weekly_sets > mrv && (performance_down || joint_ache),
        claims::HYP_MRV_DELOAD_001,
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
    evidence::graded(range, claims::HYP_RECOVOL_001)
}

/// Joint-pain rep shift (File 03 hypertrophy-016; HYP-PAIN-SHIFT-001, Strong,
/// safety-critical): on joint pain at heavy loads, move that muscle's work to
/// 12–25 reps at lighter load (50–70% 1RM); hypertrophy is preserved via load
/// interchangeability.
pub fn joint_pain_rep_shift() -> Recommended<RepLoad> {
    evidence::graded(
        RepLoad {
            reps: (12, 25),
            pct_1rm: (50, 70),
        },
        claims::HYP_PAIN_SHIFT_001,
    )
}

/// Whether a weekly-set target must be split across ≥2 sessions (File 03
/// hypertrophy-025; HYP-SPLIT-001, ExpertOpinion: the lead source is a
/// preprint): true once weekly sets exceed ~12/muscle.
pub fn needs_session_split(weekly_sets: u8) -> Recommended<bool> {
    evidence::graded(weekly_sets > WEEKLY_SPLIT_THRESHOLD, claims::HYP_SPLIT_001)
}

/// Whether per-session volume exceeds the ~11-set cap and warrants adding a
/// session rather than more sets (File 03 hypertrophy-004; HYP-SESSCAP-001,
/// ExpertOpinion: the lead source is a preprint).
pub fn per_session_over_cap(session_sets: u8) -> Recommended<bool> {
    evidence::graded(
        session_sets > PER_SESSION_SET_CEILING,
        claims::HYP_SESSCAP_001,
    )
}

/// Deload gate from accumulated overreaching triggers (File 03 hypertrophy-035;
/// HYP-DELOAD-TRIG-001, Moderate):
/// deload now once ≥2 triggers appear (performance decrement, RIR drift to 0,
/// persistent joint/tendon aches, disrupted sleep, elevated RHR, mood drop) -
/// otherwise follow the preplanned 4–8 week schedule.
pub fn deload_indicated(trigger_count: u8) -> Recommended<bool> {
    evidence::graded(trigger_count >= 2, claims::HYP_DELOAD_TRIG_001)
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
    evidence::graded(
        DeloadRx {
            sets_fraction: 0.50,
            rir: (2, 4),
            load_frac_of_working: (0.60, 0.70),
        },
        claims::HYP_DELOAD_RX_001,
    )
}

/// Increase rest when per-set reps fall > ~10% set-to-set (File 03
/// hypertrophy-039): the mechanism is preserving per-set volume, not rest
/// duration itself. `true` = lengthen the rest interval.
pub fn increase_rest_on_rep_drop(rep_drop_frac: f64) -> Recommended<bool> {
    evidence::graded(rep_drop_frac > 0.10, claims::HYP_REST_001)
}

// ---------------------------------------------------------------------------
// 6. Load interchangeability + effort/RIR rules (File 03
//    hypertrophy-012/017/018/020/021/022/023)
// ---------------------------------------------------------------------------

/// Load-interchangeability window (File 03 hypertrophy-012; HYP-LOADRANGE-001,
/// Strong, contested): hypertrophy is equivalent across ~30–85% 1RM
/// (~5–30+ reps) when sets are taken close to failure. The rep upper bound is
/// the KB's "30+" lower bound; heavy loading still favors strength.
pub fn interchangeable_load_range() -> Recommended<RepLoad> {
    evidence::graded(
        RepLoad {
            reps: (5, 30),
            pct_1rm: (EFFECTIVE_LOAD_FLOOR_PCT, 85),
        },
        claims::HYP_LOADRANGE_001,
    )
}

/// %1RM below which hypertrophy underperforms: the low end of the ~30–85 %
/// interchangeable window (File 03 hypertrophy-012; HYP-LOADRANGE-001).
pub const EFFECTIVE_LOAD_FLOOR_PCT: u8 = 30;

/// `true` when a load sits below [`EFFECTIVE_LOAD_FLOOR_PCT`], the floor under
/// which hypertrophy underperforms (File 03 hypertrophy-012; HYP-LOADRANGE-001).
pub fn load_below_effective_floor(pct_1rm: u8) -> Recommended<bool> {
    evidence::graded(
        pct_1rm < EFFECTIVE_LOAD_FLOOR_PCT,
        claims::HYP_LOADRANGE_001,
    )
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
    evidence::graded(
        HighSkillGuard {
            min_reps: 5,
            min_rir: (1, 2),
        },
        claims::HYP_SKILL_RIR_001,
    )
}

/// Default working proximity to failure (File 03 hypertrophy-018;
/// HYP-RIR-DEFAULT-001, Moderate, contested CQ-02 train-to-failure): most sets
/// at 1–3 RIR; true failure neither required nor superior enough to justify
/// its fatigue. Returns `(min, max)` RIR.
pub fn default_rir_band() -> Recommended<(u8, u8)> {
    evidence::graded((1, 3), claims::HYP_RIR_DEFAULT_001)
}

/// How trustworthy a reported RIR is (File 03 hypertrophy-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RirReliability {
    /// Close to failure (0–5 RIR): accurate to ~±1 rep.
    WithinOneRep,
    /// Far from failure (>5 RIR): error exceeds 2 reps.
    ErrorOverTwoReps,
}

/// Highest reported RIR still accurate to ~±1 rep (File 03 hypertrophy-020;
/// HYP-RIR-ACC-001).
pub const RIR_RELIABLE_MAX: u8 = 5;

/// RIR accuracy model (File 03 hypertrophy-020; HYP-RIR-ACC-001, Moderate):
/// ±1-rep accuracy only at 0–[`RIR_RELIABLE_MAX`] RIR; beyond that the error
/// exceeds 2 reps.
pub fn rir_reliability(reported_rir: u8) -> Recommended<RirReliability> {
    let r = if reported_rir <= RIR_RELIABLE_MAX {
        RirReliability::WithinOneRep
    } else {
        RirReliability::ErrorOverTwoReps
    };
    evidence::graded(r, claims::HYP_RIR_ACC_001)
}

/// Novice starting proximity (File 03 hypertrophy-020; HYP-RIR-ACC-001):
/// start at 3–4 RIR and calibrate against actual failure. Returns `(min, max)`.
pub fn novice_start_rir() -> Recommended<(u8, u8)> {
    evidence::graded((3, 4), claims::HYP_RIR_ACC_001)
}

/// Whether training to true failure (0 RIR) is permitted (File 03
/// hypertrophy-021; HYP-FAIL-SAFE-001, ExpertOpinion, SAFETY): reserved for
/// machines and isolation where failure is safe; never on heavy free-weight
/// compounds (e.g. unspotted squat/bench). Anything that is neither a machine
/// nor isolation is conservatively denied.
pub fn failure_allowed(class: ExerciseClass, machine: bool) -> Recommended<bool> {
    evidence::graded(
        machine || matches!(class, ExerciseClass::Isolation),
        claims::HYP_FAIL_SAFE_001,
    )
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

    /// Stability when performed with the given available equipment: true when
    /// this exercise itself offers a machine/cable path present in `used`
    /// (machine/cable paths are externally stabilized; free-weight/bodyweight
    /// are not — derived from hypertrophy-027's "stable isolation / machines /
    /// cables" grouping). Per-candidate: unlike a constant "is any machine
    /// available", this discriminates so a pain-driven swap ranks a stabilized
    /// option above an unstable one for the SAME available equipment.
    pub fn stable_with(&self, used: &[Equipment]) -> bool {
        self.equipment
            .iter()
            .any(|eq| used.contains(eq) && matches!(eq, Equipment::Machine | Equipment::Cable))
    }
}

use Equipment::{Barbell, Bodyweight, Cable, Dumbbell, Machine};

/// File 03 Table 4, major exercise → primary muscle → equipment map,
/// transcribed verbatim (16 rows). Each row carries ONE plain sentence-case
/// display name; Table 4's own multi-option labels ("Hack squat / leg press")
/// live in [`LEGACY_NAMES`], so a set logged under one still resolves to its
/// row. Bias tiers: "High (deep)" / "High if seated upright" / "High at top" →
/// High (note kept); "Low–moderate" → LowToModerate; "Low (cable better at
/// bottom)" → Low.
pub static EXERCISES: &[ExerciseEntry] = &[
    ExerciseEntry {
        name: "Back squat",
        primary_muscles: &["quads", "glutes"],
        equipment: &[Barbell],
        class: ExerciseClass::HeavyCompound,
        long_length: LongLengthBias::Moderate,
        long_length_note: "Moderate",
    },
    ExerciseEntry {
        name: "Hack squat",
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
        name: "Chest fly",
        primary_muscles: &["chest"],
        equipment: &[Cable, Machine],
        class: ExerciseClass::Isolation,
        long_length: LongLengthBias::High,
        long_length_note: "High",
    },
    ExerciseEntry {
        name: "Lat pulldown",
        primary_muscles: &["lats"],
        equipment: &[Bodyweight, Cable, Machine],
        class: ExerciseClass::ModerateCompound,
        long_length: LongLengthBias::High,
        long_length_note: "High at top",
    },
    ExerciseEntry {
        name: "Cable row",
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
        name: "Overhead triceps extension",
        primary_muscles: &["triceps"],
        equipment: &[Cable, Dumbbell],
        class: ExerciseClass::Isolation,
        long_length: LongLengthBias::High,
        long_length_note: "High",
    },
    ExerciseEntry {
        name: "Incline dumbbell curl",
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

/// The earlier labels each catalog row used to display (Table 4's verbatim
/// labels and retired display names), paired with
/// its current display name. Logged history keeps whatever name the set was
/// logged under, so these keep resolving to the same row and the same movement
/// ([`canonical_name`], `plan::movement_key`). Each pair keeps the keyword
/// classes the old label had (movement pattern, isolation, barbell, and the
/// pain-region sparing words), so no rule reads the new name differently.
pub const LEGACY_NAMES: &[(&str, &str)] = &[
    ("Barbell back squat", "Back squat"),
    ("Hack squat / leg press", "Hack squat"),
    ("Cable fly / pec deck", "Chest fly"),
    ("Pull-up / lat pulldown", "Lat pulldown"),
    ("Row (barbell/cable/machine)", "Cable row"),
    (
        "Overhead cable/DB triceps ext",
        "Overhead triceps extension",
    ),
    ("Incline DB curl", "Incline dumbbell curl"),
];

/// The current display name for an earlier catalog label: a Table 4 row's
/// legacy label ([`LEGACY_NAMES`]) or a calisthenics rung's KB table label
/// ([`crate::calisthenics::LEGACY_NAMES`]), case- and whitespace-insensitive.
/// `None` for any other name. Ingest stores a set logged under an old label
/// under this name, so every view shows the current one.
pub fn renamed(name: &str) -> Option<&'static str> {
    let trimmed = name.trim();
    LEGACY_NAMES
        .iter()
        .chain(crate::calisthenics::LEGACY_NAMES)
        .find(|&&(old, _)| old.eq_ignore_ascii_case(trimmed))
        .map(|&(_, new)| new)
}

/// The current catalog display name for an earlier label ([`renamed`]); any
/// other name is returned unchanged.
pub fn canonical_name(name: &str) -> &str {
    renamed(name).unwrap_or(name)
}

/// Look up a Table 4 row by exercise name (case-insensitive; a legacy label
/// resolves to its row).
pub fn exercise_entry(name: &str) -> Option<&'static ExerciseEntry> {
    let name = canonical_name(name);
    EXERCISES.iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

/// A Table 4 row for any spelling of a catalog movement: exact (case-
/// insensitive) name first, then the same movement after equipment/punctuation
/// normalization (`plan::same_movement`), so "Barbell bench press" resolves to
/// the "Bench press" row. Lookup plumbing, not a KB claim.
pub fn catalog_entry(name: &str) -> Option<&'static ExerciseEntry> {
    exercise_entry(name).or_else(|| {
        let key = crate::plan::movement_key(name);
        EXERCISES
            .iter()
            .zip(CATALOG_MOVEMENT_KEYS.iter())
            .find(|(_, k)| **k == key)
            .map(|(e, _)| e)
    })
}

/// Each [`EXERCISES`] row's movement key, aligned by index, computed once.
static CATALOG_MOVEMENT_KEYS: std::sync::LazyLock<Vec<Vec<String>>> =
    std::sync::LazyLock::new(|| {
        EXERCISES
            .iter()
            .map(|e| crate::plan::movement_key(e.name))
            .collect()
    });

// ---------------------------------------------------------------------------
// 7b. Any-name exercise class + muscle resolution, fractional volume
//     accounting, block targets, specialization, double progression
//     (File 03 Tables 1–4, hyp-001/019/020/033/043/044; 13-hypertrophy-update)
// ---------------------------------------------------------------------------

/// Name fragments of machine- or cable-guided compounds. Table 2 files
/// "moderate compounds / machines" together, so a compound carrying one of
/// these reads as [`ExerciseClass::ModerateCompound`]. Classification plumbing
/// (expert-opinion keyword taxonomy, like `plan::movement_pattern`).
const MACHINE_NEEDLES: &[&str] = &[
    "machine",
    "leg press",
    "hack",
    "pulldown",
    "pull-down",
    "pull down",
    "cable",
    "smith",
    "pec deck",
    "sled",
];

/// Table 2 exercise class for ANY exercise name: the Table 4
/// row's own class when the name is in the catalog; otherwise an isolation
/// name is [`ExerciseClass::Isolation`], a machine/cable compound is
/// [`ExerciseClass::ModerateCompound`], and any other name with a core
/// movement pattern ("Front squat", "Deadlift", "Dumbbell bench press") is a
/// [`ExerciseClass::HeavyCompound`]. An unclassifiable name takes the moderate
/// class (mid-window reps, ~90 s rest), never the heavy one.
pub fn classify(name: &str) -> ExerciseClass {
    if let Some(e) = catalog_entry(name) {
        return e.class;
    }
    if crate::plan::is_isolation_lift(name) {
        return ExerciseClass::Isolation;
    }
    let lower = name.to_ascii_lowercase();
    if crate::plan::movement_pattern(name).is_some() {
        if MACHINE_NEEDLES.iter().any(|n| lower.contains(n)) {
            ExerciseClass::ModerateCompound
        } else {
            ExerciseClass::HeavyCompound
        }
    } else if ISOLATION_MUSCLE_NEEDLES
        .iter()
        .any(|(needle, _)| lower.contains(needle))
    {
        ExerciseClass::Isolation
    } else {
        ExerciseClass::ModerateCompound
    }
}

/// Double-progression rep RANGE per exercise class: each
/// sits inside its Table 2 window (HYP-REPLOAD-001: heavy 5–10, moderate 8–15,
/// isolation 12–25) and the default program's compound 5–10 / isolation 10–20
/// (HYP-DEFAULT-PROG-001). Heavy compounds stay ≤10 reps (no 12-rep barbell
/// squats or RDLs); isolation tops out at 20, away from the 25-rep edge where
/// reps-in-reserve estimates are least accurate (hyp-020).
pub fn rep_range_for(class: ExerciseClass) -> Recommended<(u8, u8)> {
    let range = match class {
        ExerciseClass::HeavyCompound => (6, 10),
        ExerciseClass::ModerateCompound => (8, 12),
        ExerciseClass::Isolation => (12, 20),
    };
    evidence::graded(range, claims::HYP_REPLOAD_001)
}

/// Isolation / single-muscle name fragments → the Table 1 muscle they train
/// directly. Ordered: the first match wins, so "leg curl" (hamstrings) beats
/// the generic "curl" (biceps) and "reverse fly" (rear delts) beats "fly"
/// (chest). Mirrors Table 4's own muscle assignments for its isolation rows;
/// expert-opinion lookup plumbing for names outside the 16-row catalog.
const ISOLATION_MUSCLE_NEEDLES: &[(&str, &str)] = &[
    ("leg curl", "hamstrings"),
    ("hamstring curl", "hamstrings"),
    ("nordic", "hamstrings"),
    ("leg extension", "quads"),
    ("calf", "calves"),
    ("calves", "calves"),
    ("rear delt", "rear delts"),
    ("reverse fly", "rear delts"),
    ("reverse flye", "rear delts"),
    ("face pull", "rear delts"),
    ("lateral raise", "side delts"),
    ("side raise", "side delts"),
    ("tricep", "triceps"),
    ("pushdown", "triceps"),
    ("pressdown", "triceps"),
    ("skull", "triceps"),
    ("kickback", "triceps"),
    ("curl", "biceps"),
    ("pec deck", "chest"),
    ("fly", "chest"),
    ("flye", "chest"),
    ("flies", "chest"),
    ("hip thrust", "glutes"),
    ("glute", "glutes"),
    ("crunch", "abs"),
    ("sit-up", "abs"),
    ("leg raise", "abs"),
];

/// Name fragments of a VERTICAL press: Table 4's "Overhead press" trains the
/// delts (no Table 1 row) with triceps as the listed secondary, not the chest.
const VERTICAL_PRESS_NEEDLES: &[&str] = &["overhead", "shoulder", "military", "ohp", "push press"];

/// The Table 1 muscles an exercise trains and the weight each set counts
/// toward that muscle's weekly volume: the first-listed (primary) muscle 1.0,
/// every other listed muscle 0.5 (fractional counting; the 0.5
/// allocation weight is the expert-opinion convention the volume-mapping KB
/// marks as such, with Pelland 2026 favouring fractional counting).
///
/// Catalog rows use their verbatim Table 4 muscle lists (folded onto Table 1
/// keys; a head with no Table 1 row, e.g. the front delt, counts nowhere).
/// Names outside the catalog resolve by an isolation keyword first, then by
/// core movement pattern mirroring the catalog compound of that pattern
/// (squat → quads + ½ glutes, hinge → hamstrings + ½ glutes, horizontal push →
/// chest + ½ triceps, vertical push → ½ triceps, pull → back). An
/// unresolvable name trains no Table 1 muscle (empty), never a guess.
pub fn muscle_credits(name: &str) -> Vec<(&'static str, f32)> {
    let mut out: Vec<(&'static str, f32)> = Vec::new();
    if let Some(e) = catalog_entry(name) {
        for (i, m) in e.primary_muscles.iter().enumerate() {
            add_credit(&mut out, m, if i == 0 { 1.0 } else { 0.5 });
        }
        return out;
    }
    let lower = name.to_ascii_lowercase();
    if let Some((_, m)) = ISOLATION_MUSCLE_NEEDLES
        .iter()
        .find(|(needle, _)| lower.contains(needle))
    {
        add_credit(&mut out, m, 1.0);
        return out;
    }
    use crate::plan::MovementPattern;
    match crate::plan::movement_pattern(name) {
        Some(MovementPattern::Squat) => {
            add_credit(&mut out, "quads", 1.0);
            add_credit(&mut out, "glutes", 0.5);
        }
        Some(MovementPattern::Hinge) => {
            add_credit(&mut out, "hamstrings", 1.0);
            add_credit(&mut out, "glutes", 0.5);
        }
        Some(MovementPattern::Push) => {
            if !VERTICAL_PRESS_NEEDLES.iter().any(|n| lower.contains(n)) {
                add_credit(&mut out, "chest", 1.0);
            }
            add_credit(&mut out, "triceps", 0.5);
        }
        Some(MovementPattern::Pull) => add_credit(&mut out, "back", 1.0),
        _ => {}
    }
    out
}

/// Add one muscle credit (folded onto its Table 1 row), keeping the larger
/// weight when a muscle is listed twice (Row: mid-back 1.0 + lats 0.5 → back
/// 1.0). A muscle with no Table 1 row is skipped.
fn add_credit(out: &mut Vec<(&'static str, f32)>, muscle: &str, w: f32) {
    if let Some(lm) = landmarks_for(landmark_key(muscle)) {
        match out.iter_mut().find(|(m, _)| *m == lm.muscle) {
            Some(e) => e.1 = e.1.max(w),
            None => out.push((lm.muscle, w)),
        }
    }
}

/// The Table 1 muscle an exercise trains DIRECTLY (its weight-1.0 credit), if
/// any. An overhead press (delts carry no Table 1 row) has none.
pub fn primary_muscle(name: &str) -> Option<&'static str> {
    muscle_credits(name)
        .into_iter()
        .find(|(_, w)| *w >= 1.0)
        .map(|(m, _)| m)
}

/// Fractional weekly sets a week of `(exercise, sets)` items delivers to every
/// Table 1 muscle, in Table 1 order: each set counts 1.0
/// toward its primary muscle and 0.5 toward each listed secondary
/// ([`muscle_credits`]). The volume principle it serves is HYP-VOL-DOSE-001.
pub fn delivered_weekly_sets<'a>(
    items: impl IntoIterator<Item = (&'a str, u8)>,
) -> Recommended<Vec<(&'static str, f32)>> {
    let mut out: Vec<(&'static str, f32)> = LANDMARKS.iter().map(|l| (l.muscle, 0.0)).collect();
    for (name, sets) in items {
        for (m, w) in muscle_credits(name) {
            if let Some(e) = out.iter_mut().find(|(k, _)| *k == m) {
                e.1 += w * f32::from(sets);
            }
        }
    }
    evidence::graded(out, claims::HYP_VOL_DOSE_001)
}

/// Resolve a user-entered muscle (e.g. `"Side delts"`, `"lats"`) onto its
/// Table 1 landmark row. `None` for an unknown name.
pub fn resolve_muscle(raw: &str) -> Option<&'static VolumeLandmarks> {
    landmarks_for(landmark_key(raw.trim()))
}

/// How a muscle's weekly volume is set this block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeRole {
    /// The athlete's stated weekly target, bounded by the muscle's landmarks.
    Standard,
    /// Specialization priority muscle: more sets than the stated target.
    Priority,
    /// Held at maintenance volume (MV) while another muscle is specialized.
    Maintenance,
}

/// Volume role of `muscle` under an optional specialization choice
/// (HYP-SPEC-BLOCK-001, ExpertOpinion): the KB gates specialization to ADVANCED
/// trainees (stalled at ~15 sets), so for anyone else, or an unknown muscle,
/// every muscle keeps its standard target.
pub fn volume_role(
    muscle: &str,
    specialization: Option<&str>,
    ta: TrainingAge,
) -> Recommended<VolumeRole> {
    match specialization_target(specialization, ta) {
        Some(lm) if lm.muscle.eq_ignore_ascii_case(muscle) => {
            evidence::graded(VolumeRole::Priority, claims::HYP_SPEC_BLOCK_001)
        }
        Some(_) => evidence::graded(VolumeRole::Maintenance, claims::HYP_SPEC_BLOCK_001),
        None => evidence::graded(VolumeRole::Standard, claims::HYP_VOL_DOSE_001),
    }
}

/// The specialized muscle's landmark row when a specialization block applies
/// (advanced training age + a known muscle), else `None`.
pub fn specialization_target(
    specialization: Option<&str>,
    ta: TrainingAge,
) -> Option<&'static VolumeLandmarks> {
    if ta != TrainingAge::Advanced {
        return None;
    }
    specialization.and_then(resolve_muscle)
}

/// Most working sets one hypertrophy lift day is programmed with (KB
/// hypertrophy-046, HYP-VOL-CAP-001): the four core
/// movement patterns a lift day covers × 8, the per-session top of Table 3's
/// 10–18 weekly-set row. Muscles are filled big-first, so a day at the cap
/// trims the small isolation muscles, never the main compounds.
pub const SESSION_WORKING_SET_CAP: u8 = 32;

/// The weekly set count a muscle's accumulation ramp climbs to under `role`:
/// the top of its productive (MAV) range by default, its MRV only for a
/// specialization priority muscle (HYP-VOL-CAP-001; HYP-SPEC-BLOCK-001), its
/// MV when held for another muscle's specialization.
pub fn ramp_ceiling(lm: &VolumeLandmarks, role: VolumeRole) -> u8 {
    match role {
        VolumeRole::Standard => lm.mav.1,
        VolumeRole::Priority => lm.mrv,
        VolumeRole::Maintenance => lm.mv,
    }
}

/// Weekly set targets for one muscle across a `weeks`-long accumulation
/// block: the single source both the plan and the volume
/// planner calculator read, so they can never disagree.
///
/// - `Standard`: week 1 is the stated target bounded to the muscle's own
///   MEV..MRV (Table 1, HYP-LANDMARKS-001); later weeks climb along
///   [`weekly_set_ramp`] (≤ +4 sets/week) to the top of the muscle's MAV range,
///   never its MRV (HYP-VOL-CAP-001: per-muscle ceilings do not stack across
///   the whole body). A stated target already above MAV high is held, not
///   ramped. A muscle whose MEV is 0 (glutes, abs) gets no direct target:
///   compound work covers it.
/// - `Priority`: starts one maximum ramp step (+4, hyp-001/011) above the
///   stated target, bounded to MEV..MRV, then ramps to MRV (HYP-SPEC-BLOCK-001).
/// - `Maintenance`: held at the muscle's MV every week (HYP-SPEC-BLOCK-001).
pub fn weekly_targets(
    lm: &VolumeLandmarks,
    stated: u8,
    role: VolumeRole,
    weeks: u8,
) -> Recommended<Vec<u8>> {
    let top = ramp_ceiling(lm, role);
    match role {
        VolumeRole::Maintenance => {
            evidence::graded(vec![lm.mv; weeks as usize], claims::HYP_SPEC_BLOCK_001)
        }
        VolumeRole::Priority => {
            let start = stated
                .saturating_add(MAX_WEEKLY_SET_INCREMENT)
                .clamp(lm.mev, lm.mrv);
            evidence::graded(
                weekly_set_ramp(start, top, weeks).value,
                claims::HYP_SPEC_BLOCK_001,
            )
        }
        VolumeRole::Standard if lm.mev == 0 => {
            evidence::graded(vec![0; weeks as usize], claims::HYP_LANDMARKS_001)
        }
        VolumeRole::Standard => evidence::graded(
            weekly_set_ramp(stated.clamp(lm.mev, lm.mrv), top, weeks).value,
            claims::HYP_VOL_CAP_001,
        ),
    }
}

/// Lowest RIR a NOVICE is programmed to: novices start at 3–4 RIR and
/// calibrate (hyp-020, HYP-RIR-ACC-001), so their ramp stops at 2, inside the
/// 1–3 default band (HYP-RIR-DEFAULT-001) but never at its 1-RIR edge.
pub const NOVICE_RIR_FLOOR: u8 = 2;

/// Target reps in reserve for week `week` of a `block_weeks`-long block:
/// the hyp-019 ramp (RIR falls ~1/week to 1) held inside
/// the 1–3 default band (HYP-RIR-DEFAULT-001; growth rises closer to failure,
/// HYP-RIR-DOSE-001), so a 4-week block runs 3, 3, 2, 1. A novice runs the
/// novice calibration ramp instead: week 1 at the top of the 3–4 RIR start (4),
/// one less each week, held at [`NOVICE_RIR_FLOOR`] (HYP-RIR-ACC-001), e.g.
/// 4, 3, 2, 2. A week outside the block reads as week 1.
pub fn block_rir(week: u8, block_weeks: u8, novice: bool) -> Recommended<u8> {
    let raw = rir_for_week(week, block_weeks)
        .map(|r| r.value)
        .unwrap_or(MAX_SCHEDULED_RIR);
    if novice {
        let week = if week == 0 || week > block_weeks {
            1
        } else {
            week
        };
        let start = novice_start_rir().value.1;
        let rir = start.saturating_sub(week - 1).max(NOVICE_RIR_FLOOR);
        return evidence::graded(rir, claims::HYP_RIR_ACC_001);
    }
    let (lo, hi) = default_rir_band().value;
    let rir = raw.clamp(lo, hi);
    let week1 = rir_for_week(1, block_weeks)
        .map(|r| r.value)
        .unwrap_or(MAX_SCHEDULED_RIR)
        .clamp(lo, hi);
    if rir == week1 {
        evidence::graded(rir, claims::HYP_RIR_DEFAULT_001)
    } else {
        evidence::graded(rir, claims::HYP_RIR_RAMP_001)
    }
}

/// %1RM at which `reps` leave `rir` reps in reserve, from the Epley model the
/// engine uses everywhere ([`crate::strength::pct_1rm_at_effective_reps`] at
/// `reps + rir`), so a first exposure to a lift anchored only by its e1RM lands
/// on the target effort: 8 @ 2 RIR → 75 %, 6 @ 2 → 79 %. Pure arithmetic over
/// strength-005; `f32` for the `PercentOneRm` wire field.
pub fn pct_1rm_for_reps_in_reserve(reps: u8, rir: u8) -> f32 {
    crate::strength::pct_1rm_at_effective_reps(f64::from(reps) + f64::from(rir)) as f32
}

/// Smallest load step once the top of the rep range is reached
/// (HYP-DOUBLEPROG-001: "~2.5 kg/5 lb").
pub const SMALLEST_LOAD_STEP_KG: f64 = 2.5;

/// The double-progression load step from a working load of `working_kg`: the
/// smallest step ([`SMALLEST_LOAD_STEP_KG`]), but never more than
/// STR-LOADPROG-001's +10 % of the load ([`crate::strength::MAX_LOAD_STEP_FRAC`]),
/// so a 5 kg lateral raise steps to 5.5 kg, not a 50 % jump to 7.5 kg.
pub fn load_step_kg(working_kg: f64) -> f64 {
    SMALLEST_LOAD_STEP_KG.min(working_kg.max(0.0) * crate::strength::MAX_LOAD_STEP_FRAC)
}

/// The next-session decision of double progression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoubleProgression {
    /// Every working set reached the top of the range without going past the
    /// target effort: add one load step ([`load_step_kg`]) and restart at the
    /// bottom.
    AddLoad,
    /// Keep the load and aim for `target_reps` per set (one more than last
    /// time, inside the range).
    Hold { target_reps: u8 },
    /// The logged reps sit well outside this range (a different rep scheme):
    /// re-derive the working load for the range bottom at the target effort.
    Rederive,
}

/// Double progression over the last session's working sets at one load
/// (HYP-DOUBLEPROG-001, Weak, Plotkin 2022; STR-LOADPROG-001): work up through
/// `range` at the target RIR, adding a rep per session; once EVERY set reached
/// the top without costing more than the target effort, add the smallest load
/// step and restart at the bottom. `sets` = `(reps, rpe)` per working set
/// (rpe 0 = unrated, which never blocks progression). A logged effort counts
/// as on target within half a rep of the target RIR (RIR reports are only
/// ~±1 rep accurate, hyp-020). Reps more than 2 above the top or more than 2
/// below the bottom mean the load came from another rep scheme → re-derive.
pub fn double_progression(
    range: (u8, u8),
    target_rir: u8,
    sets: &[(u32, f64)],
) -> Recommended<DoubleProgression> {
    let (bottom, top) = range;
    let step = match sets.iter().map(|(r, _)| *r).min() {
        None => DoubleProgression::Hold {
            target_reps: bottom,
        },
        Some(min_reps) => {
            let (bottom32, top32) = (u32::from(bottom), u32::from(top));
            if min_reps > top32 + 2 || min_reps + 2 < bottom32 {
                DoubleProgression::Rederive
            } else if min_reps >= top32 {
                let on_target = sets.iter().all(|(_, rpe)| {
                    *rpe <= 0.0 || crate::strength::rpe_to_rir(*rpe) + 0.5 >= f64::from(target_rir)
                });
                if on_target {
                    DoubleProgression::AddLoad
                } else {
                    DoubleProgression::Hold { target_reps: top }
                }
            } else {
                DoubleProgression::Hold {
                    target_reps: (min_reps + 1).clamp(bottom32, top32) as u8,
                }
            }
        }
    };
    evidence::graded(step, claims::HYP_DOUBLEPROG_001)
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
    evidence::graded(
        MesoStructure {
            accumulation_weeks: (4, 6),
            deload_weeks: 1,
            deload_cadence_weeks: (4, 8),
        },
        claims::HYP_MESO_STRUCT_001,
    )
}

/// Post-layoff MEV reduction flag (File 03 hypertrophy-037; HYP-LAYOFF-001,
/// ExpertOpinion): after a layoff, restart at a lower MEV because landmarks
/// are temporarily reduced. The KB states no numeric reduction factor -
/// documented gap; the engine only flags that MEV must be lowered.
pub fn layoff_reduces_mev(returning_from_layoff: bool) -> Recommended<bool> {
    evidence::graded(returning_from_layoff, claims::HYP_LAYOFF_001)
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
    evidence::graded(
        TempoRx {
            rep_duration_s: (0.5, 8.0),
            concentric_s: (1.0, 2.0),
            eccentric_s: (2.0, 3.0),
        },
        claims::HYP_TEMPO_001,
    )
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
    evidence::graded(
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
        claims::HYP_DEFAULT_PROG_001,
    )
}

#[cfg(test)]
mod tests;
