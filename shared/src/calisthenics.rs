//! Calisthenics (bodyweight) resistance modality — data tables + pure rules
//! (knowledge-base `extracted/11-calisthenics.md`; source reports in
//! `knowledge-base/calisthenics/`).
//!
//! Data-first, in the style of `strength.rs`: the 13 leverage ladders are
//! transcribed rung-for-rung from `calis-progression.md` §3.1-§3.10 (names,
//! rep/hold bands, `/side`), and the load fractions from `calis-load.md` §1
//! (MEASURED force-plate rungs carry a `CAL-LOAD-*` claim; every other rung is
//! `CAL-LOAD-INFERRED-001`, no invented number). Two power ladders (jumps,
//! explosive push-ups) follow `13-calisthenics-addendum.md` cal-add-005/006.
//! No IO, no clock, no randomness.
//!
//! The intensity axis is leverage tier (primary) → added load (top of ladder) →
//! RIR/RPE (within-tier) — rep-max-at-tier replaces %1RM (CAL-INTENSITY-001).
//! Advance trigger = the working band's TOP on 2 consecutive qualifying sessions
//! (CAL-PROG-TRIGGER-001, contested CQ-F11-01), or one session far over the top
//! (CAL-PLACE-001); straight-arm lever tiers are additionally rate-limited on a
//! tendon timescale (CAL-TENDON-001). Goal shapes the band and effort
//! (CAL-GOAL-RIR-001), equipment filters the rungs (CAL-EQUIP-001).

use crate::evidence::claims;
use crate::evidence::graded;
use crate::schema::Recommended;
use crate::strength::LiftGoal;

// schema owns the plain wire enums (so it need not depend on evidence); the
// calisthenics module is their public home for the rest of the engine.
pub use crate::schema::{LadderId, TierTarget};

use FailurePolicy::{Ballistic, LeaveRir, SelfLimiting};
use LadderId::*;
use TierTarget::{HoldSec, Reps};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Failure policy for a rung (CAL-EFAIL-SAFE-001): momentary failure is only
/// allowed on self-limiting movements; skill/lever/weighted/eccentric rungs
/// must leave reps in reserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePolicy {
    /// Failure allowed — the movement fails safely (push-up, dip, squat, row).
    SelfLimiting,
    /// Leave at least this many reps in reserve (safety_critical set).
    LeaveRir(u8),
    /// Ballistic power work (jumps, plyometric push-ups): maximal intent, a few
    /// reps, stopped before speed drops, never to failure (STR-PWR-001). The KB
    /// states no numeric RIR for it, so none is encoded.
    Ballistic,
}

/// One rung of a ladder, transcribed verbatim from the KB tables.
#[derive(Debug, Clone, Copy)]
pub struct Tier {
    /// Canonical name; the currency for log matching + quick-pick.
    pub name: &'static str,
    /// Per-rung working band (KB table verbatim); its top is the advance trigger.
    pub target: TierTarget,
    /// `/side` rungs (unilateral); the shell logs one set per side.
    pub unilateral: bool,
    /// Measured %BW range at the hands/stance leg; `None` = mechanically
    /// inferred (no number is ever surfaced for `None`).
    pub bw_fraction: Option<(f64, f64)>,
    /// `CAL-LOAD-PUSHUP-001` … for a measured fraction, else `CAL-LOAD-INFERRED-001`.
    pub load_claim: crate::evidence::ClaimId,
    /// Failure policy at this rung (CAL-EFAIL-SAFE-001).
    pub failure: FailurePolicy,
    /// Pike/HSPU/handstand/planche rungs — pain-free loaded wrist extension
    /// (CAL-INJURY-WRIST-001).
    pub wrist_gate: bool,
    /// Planche/front-lever/back-lever rungs — tendon rate-limit
    /// (CAL-TENDON-001).
    pub straight_arm: bool,
}

/// One leverage ladder (a movement pattern's ordered rungs).
#[derive(Debug, Clone)]
pub struct Ladder {
    pub id: LadderId,
    pub pattern: &'static str,
    /// Skill track: hold-based/opt-in, scheduled first, volume-discounted
    /// (CAL-VOLCOUNT-SKILL-001). Dynamic resistance ladders are `false`.
    pub skill: bool,
    pub tiers: &'static [Tier],
    /// Rungs where `added_kg` is legitimate: the terminal rung for most
    /// ladders; `[4, 7]` for `VerticalPull` (KB §3.5: weighted pull-up is
    /// reachable at the full-rep rung in parallel with the one-arm leverage
    /// branch, whose rungs 5–6 advance by rung, then the one-arm top takes
    /// load); empty for skill tracks except the `MuscleUp`/`FrontLever`/`Planche`
    /// weighted tops.
    pub weighted_rungs: &'static [u8],
    /// Power (ballistic) ladder, scheduled first on a bodyweight Power goal
    /// (CAL-POWER-001). Never part of the pattern rotation or the skill round-robin.
    pub power: bool,
}

/// Goal-specific working-set template (CAL-TEMPLATE-001; calis-programming.md §1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalisTemplate {
    /// Working-set count band (min, max) before volume sizing.
    pub sets: (u8, u8),
    /// Between-set rest, seconds.
    pub rest_sec: u16,
}

impl TierTarget {
    /// Band top — the tier-advance trigger (reps or hold seconds).
    pub fn top(&self) -> u16 {
        match *self {
            Reps { high, .. } => high as u16,
            HoldSec { high, .. } => high,
        }
    }
    /// Band floor — reset target after a tier advance.
    pub fn floor(&self) -> u16 {
        match *self {
            Reps { low, .. } => low as u16,
            HoldSec { low, .. } => low,
        }
    }
    /// True for a timed (hold-seconds) band.
    pub fn is_timed(&self) -> bool {
        matches!(self, HoldSec { .. })
    }
}

// ---------------------------------------------------------------------------
// Ladder tables (calis-progression.md §3.1-§3.10). `mk` = Tier constructor:
// (name, target, unilateral, bw_fraction, load_claim, failure, wrist_gate,
// straight_arm). Load fractions from calis-load.md §1 (plan §3).
// ---------------------------------------------------------------------------

const INF: crate::evidence::ClaimId = claims::CAL_LOAD_INFERRED_001;
const PUSHUP: crate::evidence::ClaimId = claims::CAL_LOAD_PUSHUP_001;
const ROW: crate::evidence::ClaimId = claims::CAL_LOAD_ROW_001;
const SPLITSQ: crate::evidence::ClaimId = claims::CAL_LOAD_SPLITSQ_001;
const PISTOL: crate::evidence::ClaimId = claims::CAL_LOAD_PISTOL_001;

const fn reps(low: u8, high: u8) -> TierTarget {
    TierTarget::Reps { low, high }
}
const fn hold(low: u16, high: u16) -> TierTarget {
    TierTarget::HoldSec { low, high }
}

#[expect(
    clippy::too_many_arguments,
    reason = "const row builder for the ladder tables; arguments follow the column order documented above"
)]
const fn mk(
    name: &'static str,
    target: TierTarget,
    unilateral: bool,
    bw_fraction: Option<(f64, f64)>,
    load_claim: crate::evidence::ClaimId,
    failure: FailurePolicy,
    wrist_gate: bool,
    straight_arm: bool,
) -> Tier {
    Tier {
        name,
        target,
        unilateral,
        bw_fraction,
        load_claim,
        failure,
        wrist_gate,
        straight_arm,
    }
}

// §3.1 Horizontal push (push-up family). Weighted at the terminal (one-arm) rung.
static HORIZONTAL_PUSH_TIERS: &[Tier] = &[
    mk(
        "Wall push-up",
        reps(8, 12),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Incline push-up (high)",
        reps(5, 8),
        false,
        Some((0.41, 0.41)),
        PUSHUP,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Incline push-up (low)",
        reps(5, 8),
        false,
        Some((0.55, 0.55)),
        PUSHUP,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Knee push-up",
        reps(5, 8),
        false,
        Some((0.49, 0.53)),
        PUSHUP,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Full push-up",
        reps(5, 8),
        false,
        Some((0.64, 0.75)),
        PUSHUP,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Decline push-up",
        reps(5, 8),
        false,
        Some((0.70, 0.74)),
        PUSHUP,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Pseudo-planche push-up",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Archer push-up",
        reps(5, 8),
        true,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    // One-arm work leaves reps in reserve (§2.8: failure discouraged on
    // skill/lever/one-arm work — form breakdown and connective-tissue risk).
    mk(
        "One-arm push-up",
        reps(3, 8),
        true,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
];

// §3.2 Dip family.
static DIP_TIERS: &[Tier] = &[
    mk(
        "Bench dip",
        reps(8, 12),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Foot-supported dip",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Straight-bar dip",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Full parallel-bar dip",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Ring dip",
        reps(5, 8),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
    mk(
        "Weighted dip",
        reps(5, 8),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
];

// §3.3 Vertical push (pike → HSPU). All rungs wrist-gated.
static VERTICAL_PUSH_TIERS: &[Tier] = &[
    mk(
        "Pike push-up",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        true,
        false,
    ),
    mk(
        "Elevated pike push-up",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        true,
        false,
    ),
    mk(
        "Wall handstand push-up (partial)",
        reps(3, 8),
        false,
        None,
        INF,
        SelfLimiting,
        true,
        false,
    ),
    mk(
        "Wall handstand push-up (full)",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        true,
        false,
    ),
    mk(
        "Deficit wall handstand push-up",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        true,
        false,
    ),
    mk(
        "Freestanding handstand push-up",
        reps(3, 5),
        false,
        None,
        INF,
        LeaveRir(1),
        true,
        false,
    ),
    mk(
        "Weighted handstand push-up",
        reps(3, 5),
        false,
        None,
        INF,
        LeaveRir(1),
        true,
        false,
    ),
];

// §3.4 Horizontal pull (row family). Body-row rungs carry the measured
// inverted-row fraction (CAL-LOAD-ROW-001, ~69.5-75.7% at the hands).
static HORIZONTAL_PULL_TIERS: &[Tier] = &[
    mk(
        "High-incline row",
        reps(8, 12),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    // Only the horizontal body row matches Vural's measured suspension-row
    // geometry (~69.5-75.7% BW). The inclined (easier) and feet-elevated (~full
    // BW per calis-load.md §1d) rungs are different leverages the study did not
    // measure → inferred, no invented %BW surfaced.
    mk(
        "Inclined body row",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Horizontal body row",
        reps(5, 8),
        false,
        Some((0.695, 0.757)),
        ROW,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Feet-elevated body row",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Ring row",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Archer row",
        reps(5, 8),
        true,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    // One-arm work leaves reps in reserve (§2.8).
    mk(
        "One-arm row",
        reps(3, 8),
        true,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
];

// §3.5 Vertical pull (pull-up family). Weighted pull-up reachable at rung 4
// (Full pull-up) in parallel with the one-arm leverage branch (rungs 5–6
// advance by rung; the one-arm top takes load again).
static VERTICAL_PULL_TIERS: &[Tier] = &[
    mk(
        "Dead hang",
        hold(30, 30),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Scapular pulls",
        reps(8, 12),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Assisted pull-up",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Negative pull-up",
        reps(3, 5),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
    mk(
        "Full pull-up",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "L-sit pull-up",
        reps(5, 8),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    // One-arm work leaves reps in reserve (§2.8).
    mk(
        "One-arm assisted pull-up",
        reps(3, 5),
        true,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
    mk(
        "One-arm pull-up",
        reps(1, 5),
        true,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
];

// §3.6 Squat (knee-dominant). Split-squat family = 0.59 front-leg share; pistol
// = 1.0 BW on the stance leg.
static SQUAT_TIERS: &[Tier] = &[
    mk(
        "Assisted squat",
        reps(8, 12),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Bodyweight squat",
        reps(8, 15),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    // The 0.59 front-leg share is Bulgarian-specific (rear foot elevated, Arakawa
    // 2025); a floor-foot static lunge shares more to the rear leg → inferred.
    mk(
        "Split squat",
        reps(8, 12),
        true,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Bulgarian split squat",
        reps(8, 12),
        true,
        Some((0.59, 0.59)),
        SPLITSQ,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Assisted pistol squat",
        reps(5, 8),
        true,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Pistol squat",
        reps(5, 8),
        true,
        Some((1.0, 1.0)),
        PISTOL,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Weighted pistol squat",
        reps(5, 8),
        true,
        Some((1.0, 1.0)),
        PISTOL,
        LeaveRir(1),
        false,
        false,
    ),
];

// §3.7 Hinge (posterior chain), culminating in the Nordic curl (CAL-PREVENT-NORDIC-001).
static HINGE_TIERS: &[Tier] = &[
    mk(
        "Glute bridge",
        reps(10, 15),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Single-leg glute bridge",
        reps(8, 12),
        true,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Back extension",
        reps(10, 15),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Single-leg Romanian deadlift",
        reps(8, 12),
        true,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    // Every Nordic rung is a "not to failure" movement (CAL-EFAIL-SAFE-001
    // names the Nordic in its disallowed set; safety_critical).
    mk(
        "Assisted Nordic curl",
        reps(5, 8),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
    mk(
        "Negative Nordic curl",
        reps(3, 6),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
    mk(
        "Full Nordic curl",
        reps(3, 6),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
    mk(
        "Weighted Nordic curl",
        reps(3, 6),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
];

// §3.8 Core / anti-extension. Timed rungs progress in seconds.
static CORE_TIERS: &[Tier] = &[
    mk(
        "Front plank",
        hold(20, 45),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Long-lever plank",
        hold(20, 30),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Hollow-body hold",
        hold(15, 30),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Ab-wheel from knees",
        reps(5, 10),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Hanging leg raise",
        reps(8, 12),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Standing ab-wheel",
        reps(3, 6),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
    mk(
        "Dragon flag",
        reps(3, 6),
        false,
        None,
        INF,
        SelfLimiting,
        false,
        false,
    ),
];

// §3.3 skill row — Handstand balance (skill track). All rungs wrist-gated.
static HANDSTAND_TIERS: &[Tier] = &[
    mk(
        "Wall walk",
        hold(5, 15),
        false,
        None,
        INF,
        LeaveRir(1),
        true,
        false,
    ),
    mk(
        "Chest-to-wall handstand",
        hold(15, 30),
        false,
        None,
        INF,
        LeaveRir(1),
        true,
        false,
    ),
    mk(
        "Freestanding kick-up",
        hold(3, 10),
        false,
        None,
        INF,
        LeaveRir(1),
        true,
        false,
    ),
    mk(
        "Freestanding handstand",
        hold(10, 30),
        false,
        None,
        INF,
        LeaveRir(1),
        true,
        false,
    ),
];

// §3.9 Front lever (straight-arm skill track, tendon-rate-limited).
static FRONT_LEVER_TIERS: &[Tier] = &[
    mk(
        "Tuck front lever",
        hold(5, 15),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        true,
    ),
    mk(
        "Advanced tuck front lever",
        hold(5, 15),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        true,
    ),
    mk(
        "Straddle front lever",
        hold(5, 10),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        true,
    ),
    mk(
        "Full front lever",
        hold(5, 10),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        true,
    ),
    mk(
        "Front lever pulls",
        reps(3, 6),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        true,
    ),
];

// §3.9 Planche (straight-arm skill track; wrist-gated + tendon-rate-limited).
static PLANCHE_TIERS: &[Tier] = &[
    mk(
        "Planche lean",
        hold(5, 15),
        false,
        None,
        INF,
        LeaveRir(1),
        true,
        true,
    ),
    mk(
        "Tuck planche",
        hold(5, 15),
        false,
        None,
        INF,
        LeaveRir(1),
        true,
        true,
    ),
    mk(
        "Advanced tuck planche",
        hold(5, 15),
        false,
        None,
        INF,
        LeaveRir(1),
        true,
        true,
    ),
    mk(
        "Straddle planche",
        hold(5, 10),
        false,
        None,
        INF,
        LeaveRir(1),
        true,
        true,
    ),
    mk(
        "Full planche",
        hold(5, 10),
        false,
        None,
        INF,
        LeaveRir(1),
        true,
        true,
    ),
    mk(
        "Planche push-up",
        reps(3, 6),
        false,
        None,
        INF,
        LeaveRir(1),
        true,
        true,
    ),
];

// §3.9 Back lever (easiest straight-arm lever; often the entry skill).
static BACK_LEVER_TIERS: &[Tier] = &[
    mk(
        "German hang",
        hold(5, 15),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        true,
    ),
    mk(
        "Tuck back lever",
        hold(5, 15),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        true,
    ),
    mk(
        "Advanced tuck back lever",
        hold(5, 15),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        true,
    ),
    // Back lever advances uniformly at ~10-15 s clean across the progression
    // (calis-progression.md §3.9), unlike the front-lever/planche end-tier taper.
    mk(
        "Straddle back lever",
        hold(5, 15),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        true,
    ),
    mk(
        "Full back lever",
        hold(5, 15),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        true,
    ),
];

// §3.10 Muscle-up (dynamic combination skill track; the rung-0 prereq GATE is
// NOT a tier — it is [`muscle_up_gate_met`], applied when the calisthenics
// anchors are built).
static MUSCLE_UP_TIERS: &[Tier] = &[
    mk(
        "Explosive pull-up",
        reps(3, 5),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
    mk(
        "Muscle-up transition drill",
        reps(3, 5),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
    mk(
        "Negative muscle-up",
        reps(2, 3),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
    mk(
        "Strict muscle-up",
        reps(1, 5),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
    mk(
        "Weighted muscle-up",
        reps(1, 3),
        false,
        None,
        INF,
        LeaveRir(1),
        false,
        false,
    ),
];

// Power ladders (CAL-POWER-LADDER-001, 13-calisthenics-addendum cal-add-006):
// bodyweight ballistic progressions for a Power goal. Depth jumps are left out:
// they need a ~1.5×BW back squat first (PLYO-PREREQ-001), which a bodyweight
// profile cannot show. 3–5 reps at maximal intent, never to failure
// (STR-PWR-001: 1–5 reps stopped before speed drops).
static JUMP_TIERS: &[Tier] = &[
    mk(
        "Vertical jump",
        reps(3, 5),
        false,
        None,
        INF,
        Ballistic,
        false,
        false,
    ),
    mk(
        "Broad jump",
        reps(3, 5),
        false,
        None,
        INF,
        Ballistic,
        false,
        false,
    ),
    mk(
        "Single-leg hop",
        reps(3, 5),
        true,
        None,
        INF,
        Ballistic,
        false,
        false,
    ),
];

static PLYO_PUSH_TIERS: &[Tier] = &[
    mk(
        "Incline plyometric push-up",
        reps(3, 5),
        false,
        None,
        INF,
        Ballistic,
        false,
        false,
    ),
    mk(
        "Plyometric push-up",
        reps(3, 5),
        false,
        None,
        INF,
        Ballistic,
        false,
        false,
    ),
    mk(
        "Clap push-up",
        reps(3, 5),
        false,
        None,
        INF,
        Ballistic,
        false,
        false,
    ),
];

/// Every leverage ladder (8 dynamic + 5 skill tracks + 2 power ladders). The
/// order is the `LadderId` enum order; skill tracks are round-robined by it in
/// plan synthesis.
pub static LADDERS: &[Ladder] = &LADDER_ROWS;

/// Fixed-size backing array of [`LADDERS`], so [`ladder`]'s constant indices
/// are bounds-checked at compile time.
static LADDER_ROWS: [Ladder; 15] = [
    Ladder {
        id: HorizontalPush,
        pattern: "Push-up",
        skill: false,
        tiers: HORIZONTAL_PUSH_TIERS,
        weighted_rungs: &[8],
        power: false,
    },
    Ladder {
        id: Dip,
        pattern: "Dip",
        skill: false,
        tiers: DIP_TIERS,
        weighted_rungs: &[5],
        power: false,
    },
    Ladder {
        id: VerticalPush,
        pattern: "Overhead push",
        skill: false,
        tiers: VERTICAL_PUSH_TIERS,
        weighted_rungs: &[6],
        power: false,
    },
    Ladder {
        id: HorizontalPull,
        pattern: "Row",
        skill: false,
        tiers: HORIZONTAL_PULL_TIERS,
        weighted_rungs: &[6],
        power: false,
    },
    Ladder {
        id: VerticalPull,
        pattern: "Pull-up",
        skill: false,
        tiers: VERTICAL_PULL_TIERS,
        weighted_rungs: &[4, 7],
        power: false,
    },
    Ladder {
        id: Squat,
        pattern: "Squat",
        skill: false,
        tiers: SQUAT_TIERS,
        weighted_rungs: &[6],
        power: false,
    },
    Ladder {
        id: Hinge,
        pattern: "Hips and hamstrings",
        skill: false,
        tiers: HINGE_TIERS,
        weighted_rungs: &[7],
        power: false,
    },
    Ladder {
        id: Core,
        pattern: "Core",
        skill: false,
        tiers: CORE_TIERS,
        weighted_rungs: &[6],
        power: false,
    },
    Ladder {
        id: Handstand,
        pattern: "Handstand",
        skill: true,
        tiers: HANDSTAND_TIERS,
        weighted_rungs: &[],
        power: false,
    },
    Ladder {
        id: FrontLever,
        pattern: "Front lever",
        skill: true,
        tiers: FRONT_LEVER_TIERS,
        weighted_rungs: &[4],
        power: false,
    },
    Ladder {
        id: Planche,
        pattern: "Planche",
        skill: true,
        tiers: PLANCHE_TIERS,
        weighted_rungs: &[5],
        power: false,
    },
    Ladder {
        id: BackLever,
        pattern: "Back lever",
        skill: true,
        tiers: BACK_LEVER_TIERS,
        weighted_rungs: &[],
        power: false,
    },
    Ladder {
        id: MuscleUp,
        pattern: "Muscle-up",
        skill: true,
        tiers: MUSCLE_UP_TIERS,
        weighted_rungs: &[4],
        power: false,
    },
    Ladder {
        id: Jump,
        pattern: "Jump",
        skill: false,
        tiers: JUMP_TIERS,
        weighted_rungs: &[],
        power: true,
    },
    Ladder {
        id: PlyoPush,
        pattern: "Explosive push-up",
        skill: false,
        tiers: PLYO_PUSH_TIERS,
        weighted_rungs: &[],
        power: true,
    },
];

/// Everyday names for canonical rungs, so a set logged as "Pull-ups", "Dips"
/// or "Pistol" still moves its ladder. Matched with the same normalization as
/// the canonical names (case, punctuation, spacing and a plural "s" ignored);
/// a canonical name always wins over an alias.
static ALIASES: &[(&str, LadderId, u8)] = &[
    ("push-up", HorizontalPush, 4),
    ("press-up", HorizontalPush, 4),
    ("feet-elevated / decline push-up", HorizontalPush, 5),
    ("dip", Dip, 3),
    ("parallel bar dip", Dip, 3),
    ("parallette / straight-bar dip", Dip, 2),
    ("handstand push-up", VerticalPush, 3),
    ("hspu", VerticalPush, 3),
    ("inverted row", HorizontalPull, 2),
    ("australian pull-up", HorizontalPull, 2),
    ("body row", HorizontalPull, 2),
    ("pull-up", VerticalPull, 4),
    ("chin-up", VerticalPull, 4),
    ("strict pull-up", VerticalPull, 4),
    ("pull-up negative", VerticalPull, 3),
    ("eccentric pull-up", VerticalPull, 3),
    ("band assisted pull-up", VerticalPull, 2),
    ("air squat", Squat, 1),
    ("pistol", Squat, 5),
    ("single-leg rdl", Hinge, 3),
    ("nordic negative", Hinge, 5),
    ("nordic", Hinge, 6),
    ("nordic curl", Hinge, 6),
    ("nordic hamstring curl", Hinge, 6),
    ("plank", Core, 0),
    ("plank (front)", Core, 0),
    ("hollow hold", Core, 2),
    ("muscle-up", MuscleUp, 3),
    ("front lever tuck", FrontLever, 0),
    ("advanced tuck (open hips)", FrontLever, 1),
    ("back lever tuck", BackLever, 1),
    ("planche tuck", Planche, 1),
    ("jump squat", Jump, 0),
    ("countermovement jump", Jump, 0),
    ("standing long jump", Jump, 1),
    ("plyo push-up", PlyoPush, 1),
];

/// The KB progression tables' row labels (`calis-progression.md`), the
/// earlier names of these rungs, paired with each rung's current name. They
/// stay in [`ALIASES`] so they keep matching their rung; display goes through
/// `hypertrophy::canonical_name`, so a set logged under an old label shows
/// under the current name. "Nordic negative" is left out: its current name
/// ("Negative Nordic curl") reads as isolation to the keyword rules.
pub const LEGACY_NAMES: &[(&str, &str)] = &[
    ("Feet-elevated / decline push-up", "Decline push-up"),
    ("Parallette / straight-bar dip", "Straight-bar dip"),
    ("Single-leg RDL", "Single-leg Romanian deadlift"),
    ("Plank (front)", "Front plank"),
    ("Advanced tuck (open hips)", "Advanced tuck front lever"),
];

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

/// The ladder for an id. The match is exhaustive over `LadderId` and each
/// arm's constant index is checked against `LADDER_ROWS` at compile time; the
/// `every_ladder_id_resolves` test pins the id-to-row map.
pub fn ladder(id: LadderId) -> &'static Ladder {
    match id {
        HorizontalPush => &LADDER_ROWS[0],
        Dip => &LADDER_ROWS[1],
        VerticalPush => &LADDER_ROWS[2],
        HorizontalPull => &LADDER_ROWS[3],
        VerticalPull => &LADDER_ROWS[4],
        Squat => &LADDER_ROWS[5],
        Hinge => &LADDER_ROWS[6],
        Core => &LADDER_ROWS[7],
        Handstand => &LADDER_ROWS[8],
        FrontLever => &LADDER_ROWS[9],
        Planche => &LADDER_ROWS[10],
        BackLever => &LADDER_ROWS[11],
        MuscleUp => &LADDER_ROWS[12],
        Jump => &LADDER_ROWS[13],
        PlyoPush => &LADDER_ROWS[14],
    }
}

/// The rung at `idx` in `id`'s ladder, if it exists.
pub fn tier(id: LadderId, idx: u8) -> Option<&'static Tier> {
    ladder(id).tiers.get(idx as usize)
}

/// Normalized lookup key for an exercise name: lowercase ASCII alphanumeric
/// tokens with a plural trailing "s" dropped, joined without separators, so
/// "Pull ups", "pull-up" and "PULLUPS" share one key.
fn name_key(name: &str) -> String {
    let mut key = String::with_capacity(name.len());
    for tok in name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
    {
        let lower = tok.to_ascii_lowercase();
        let singular = match lower.strip_suffix('s') {
            Some(s) if s.len() >= 2 && !s.ends_with('s') => s,
            _ => lower.as_str(),
        };
        key.push_str(singular);
    }
    key
}

/// Name key → rung, built once: every canonical rung name first, then the
/// everyday aliases (a canonical key is never overwritten by an alias).
static NAME_INDEX: std::sync::LazyLock<std::collections::HashMap<String, (LadderId, u8)>> =
    std::sync::LazyLock::new(|| {
        let mut m = std::collections::HashMap::new();
        for l in LADDERS {
            for (i, t) in l.tiers.iter().enumerate() {
                m.insert(name_key(t.name), (l.id, i as u8));
            }
        }
        for &(alias, id, rung) in ALIASES {
            m.entry(name_key(alias)).or_insert((id, rung));
        }
        m
    });

/// Match a logged exercise name to `(ladder, rung)`: a canonical rung name or
/// an everyday alias ("Pull-ups" → Full pull-up), ignoring case, punctuation,
/// spacing and a plural "s". Canonical names are unique across ALL ladders, so
/// a hit is unambiguous.
pub fn tier_by_name(name: &str) -> Option<(LadderId, u8)> {
    let key = name_key(name);
    if key.is_empty() {
        return None;
    }
    NAME_INDEX.get(&key).copied()
}

/// True when this rung takes added load: one of the ladder's
/// `weighted_rungs`. An advance on such a rung adds load rather than a rung.
pub fn weighted_ok(id: LadderId, idx: u8) -> bool {
    ladder(id).weighted_rungs.contains(&idx)
}

/// A "Weighted …" rung (weighted dip / HSPU / pistol / Nordic / muscle-up): the
/// rung IS the loaded variant, so prescribing it implies an added load (the
/// plan starts it at the first load step rather than at 0 kg).
pub fn inherently_loaded(tier: &Tier) -> bool {
    tier.name.starts_with("Weighted ")
}

// ---------------------------------------------------------------------------
// Equipment (CAL-EQUIP-001)
// ---------------------------------------------------------------------------

/// Calisthenics equipment on hand, for rung filtering (CAL-EQUIP-001). Rings
/// substitute for a bar (hanging/pulling) and for dip bars (support/dipping).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kit {
    pub pull_up_bar: bool,
    pub dip_bars: bool,
    pub rings: bool,
}

impl Kit {
    /// The athlete's stated equipment (`Profile::calis_equipment`); `None`
    /// (unstated) assumes every piece is available.
    pub fn from_profile(profile: &crate::app::Profile) -> Kit {
        profile.calis_equipment.map_or(Kit::ALL, |e| Kit {
            pull_up_bar: e.pull_up_bar,
            dip_bars: e.dip_bars,
            rings: e.rings,
        })
    }
    /// Every piece available (the default when the athlete states nothing).
    pub const ALL: Kit = Kit {
        pull_up_bar: true,
        dip_bars: true,
        rings: true,
    };
    /// Something to hang and pull from.
    pub fn hang(&self) -> bool {
        self.pull_up_bar || self.rings
    }
    /// Something to support and dip on.
    pub fn dip_support(&self) -> bool {
        self.dip_bars || self.rings
    }
}

/// The equipment a rung needs beyond floor, wall, table and bench.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    /// Something to hang and pull from: a bar or rings.
    Hang,
    /// Something to support and dip on: dip bars or rings.
    DipSupport,
    /// Ring-specific rungs.
    Rings,
}

fn rung_need(id: LadderId, idx: u8) -> Option<Need> {
    match (id, idx) {
        (VerticalPull | FrontLever | BackLever | MuscleUp, _) => Some(Need::Hang),
        (Dip, 4) | (HorizontalPull, 4) => Some(Need::Rings),
        (Dip, 1..=3) | (Dip, 5) => Some(Need::DipSupport),
        (HorizontalPull, 5..=6) | (Core, 4) => Some(Need::Hang),
        _ => None,
    }
}

/// Whether a rung can be done with this equipment. Floor, wall, table and
/// bench variants need nothing; hanging/pulling rungs need a bar or rings;
/// bar-dip rungs need dip bars or rings; ring-specific rungs need rings.
pub fn rung_available(id: LadderId, idx: u8, kit: Kit) -> bool {
    match rung_need(id, idx) {
        None => true,
        Some(Need::Hang) => kit.hang(),
        Some(Need::DipSupport) => kit.dip_support(),
        Some(Need::Rings) => kit.rings,
    }
}

/// What a rung needs, in plain words ("a pull-up bar or rings"); `None` when
/// floor, wall, table or bench is enough.
pub fn rung_equipment(id: LadderId, idx: u8) -> Option<&'static str> {
    rung_need(id, idx).map(|n| match n {
        Need::Hang => "a pull-up bar or rings",
        Need::DipSupport => "dip bars or rings",
        Need::Rings => "rings",
    })
}

/// True when at least one rung of the ladder is doable with this equipment.
pub fn ladder_available(id: LadderId, kit: Kit) -> bool {
    (0..ladder(id).tiers.len() as u8).any(|i| rung_available(id, i, kit))
}

/// The nearest available rung strictly above `from`.
pub fn available_above(id: LadderId, from: u8, kit: Kit) -> Option<u8> {
    (from.saturating_add(1)..ladder(id).tiers.len() as u8).find(|&i| rung_available(id, i, kit))
}

/// The nearest available rung strictly below `from`.
pub fn available_below(id: LadderId, from: u8, kit: Kit) -> Option<u8> {
    (0..from).rev().find(|&i| rung_available(id, i, kit))
}

/// `idx` itself when available, else the nearest easier available rung, else
/// the nearest harder one (`None` when the ladder needs equipment the athlete
/// lacks entirely).
pub fn nearest_available(id: LadderId, idx: u8, kit: Kit) -> Option<u8> {
    if (idx as usize) < ladder(id).tiers.len() && rung_available(id, idx, kit) {
        return Some(idx);
    }
    available_below(id, idx, kit).or_else(|| available_above(id, idx, kit))
}

/// Graded face of an equipment substitution (CAL-EQUIP-001).
pub fn equipment_substitution() -> Recommended<()> {
    graded((), claims::CAL_EQUIP_001)
}

// ---------------------------------------------------------------------------
// Graded rules
// ---------------------------------------------------------------------------

/// Tier-advance trigger (CAL-PROG-TRIGGER-001, contested CQ-F11-01): fires after
/// 2 consecutive qualifying sessions — the calisthenics analogue of
/// `strength::two_for_two_met`.
pub fn tier_advance_met(qualifying_consecutive_sessions: u8) -> Recommended<bool> {
    graded(
        qualifying_consecutive_sessions >= 2,
        claims::CAL_PROG_TRIGGER_001,
    )
}

/// Days a straight-arm lever tier must be held before advancing: the 4–6 wk
/// low end of tendon adaptation (CAL-GATE-NUMBERS-001 engine default,
/// contested CQ-F11-02; CAL-TENDON-001 rate-limit).
pub const STRAIGHT_ARM_ACCLIMATION_DAYS: i64 = 28;

/// Straight-arm static acclimation (CAL-GATE-NUMBERS-001 engine default,
/// contested CQ-F11-02): tendon adapts on a weeks-to-months timescale, so a
/// straight-arm lever tier must be held ≥ [`STRAIGHT_ARM_ACCLIMATION_DAYS`]
/// before advancing (CAL-TENDON-001 rate-limit).
pub fn static_acclimation_ok(days_at_current_tier: i64) -> Recommended<bool> {
    graded(
        days_at_current_tier >= STRAIGHT_ARM_ACCLIMATION_DAYS,
        claims::CAL_GATE_NUMBERS_001,
    )
}

/// Strict pull-ups AND dips, each in a single set, required before muscle-up
/// work (CAL-GATE-NUMBERS-001 engine default, contested CQ-F11-02).
pub const MUSCLE_UP_GATE_REPS: u8 = 12;

/// Muscle-up gate (CAL-GATE-NUMBERS-001 engine default, contested CQ-F11-02):
/// [`MUSCLE_UP_GATE_REPS`] strict pull-ups AND dips in a single set before
/// muscle-up work.
pub fn muscle_up_gate_met(
    best_pullup_session_reps: u8,
    best_dip_session_reps: u8,
) -> Recommended<bool> {
    graded(
        best_pullup_session_reps >= MUSCLE_UP_GATE_REPS
            && best_dip_session_reps >= MUSCLE_UP_GATE_REPS,
        claims::CAL_GATE_NUMBERS_001,
    )
}

/// Novice effort band on every non-ballistic rep rung (File 03 hypertrophy-020,
/// HYP-RIR-ACC-001: novices start well short of failure and calibrate against
/// it), the engine's novice RIR floor and one above — the same "2–3 reps left"
/// a novice sees on weights. It still leaves ≥1–2 RIR on the leave-RIR set
/// (CAL-EFAIL-SAFE-001), so a novice's ladder never asks for MORE effort on a
/// harder rung.
pub const NOVICE_RIR: (u8, u8) = (
    crate::hypertrophy::NOVICE_RIR_FLOOR,
    crate::hypertrophy::NOVICE_RIR_FLOOR + 1,
);

/// Rep-band top at or above which an unloaded self-limiting rung is
/// "high-rep": low-load work that must approach failure
/// (CAL-INTENSITY-FLOOR-001) and where self-reported reps in reserve are least
/// accurate (CAL-RIR-ACC-001, Halperin 2022: worse above 12 reps).
pub const HIGH_REP_BAND_TOP: u16 = 12;

/// True for an "easy high-rep" rung (CAL-INTENSITY-FLOOR-001): an unloaded
/// self-limiting rung whose band reaches ≥ [`HIGH_REP_BAND_TOP`] (or a hold),
/// and so is every easier rung of its ladder. A high-rep rung sitting ABOVE a
/// low-rep rung
/// (hanging leg raise over the knee ab-wheel) is not "easy", so the effort
/// target never drops toward failure as the ladder gets harder.
pub fn easy_rung(id: LadderId, idx: u8) -> bool {
    let tiers = ladder(id).tiers;
    (idx as usize) < tiers.len()
        && tiers[..=idx as usize].iter().all(|t| {
            t.failure == FailurePolicy::SelfLimiting
                && (t.target.is_timed() || t.target.top() >= HIGH_REP_BAND_TOP)
        })
}

/// Rung-, goal- and training-age-dependent intensity band (RIR low, high):
/// - `Ballistic` rungs → `None`: power work states no numeric RIR, only "never
///   to failure; stop before speed drops" (STR-PWR-001);
/// - `LeaveRir(n)` rungs → `(n, n+1)`, CAL-EFAIL-SAFE-001 (skill/lever/weighted/
///   eccentric/one-arm: never to failure, whatever the goal); a novice never
///   below [`NOVICE_RIR`];
/// - self-limiting timed rungs → `(0, 1)`, CAL-INTENSITY-FLOOR-001;
/// - self-limiting rep rungs for a novice → [`NOVICE_RIR`], HYP-RIR-ACC-001;
/// - self-limiting rep rungs on a hypertrophy goal → `(0, 1)`, CAL-GOAL-RIR-001
///   (closer to failure modestly adds growth, not strength; calis template B);
/// - easy high-rep rungs ([`easy_rung`]) → `(0, 1)`, CAL-INTENSITY-FLOOR-001
///   (low-load work must approach failure);
/// - everything else → `(1, 2)`, CAL-INTENSITY-001.
pub fn rir_band(
    id: LadderId,
    idx: u8,
    goal: LiftGoal,
    novice: bool,
) -> Option<Recommended<(u8, u8)>> {
    let tier = tier(id, idx)?;
    match tier.failure {
        FailurePolicy::LeaveRir(n) => {
            let lo = if novice { n.max(NOVICE_RIR.0) } else { n };
            Some(graded((lo, lo + 1), claims::CAL_EFAIL_SAFE_001))
        }
        FailurePolicy::Ballistic => None,
        FailurePolicy::SelfLimiting => Some(if tier.target.is_timed() {
            // A self-limiting TIMED rung (dead hang, plank, hollow) is held to
            // near-failure unconditionally — never compare hold seconds against
            // the rep threshold.
            graded((0, 1), claims::CAL_INTENSITY_FLOOR_001)
        } else if novice {
            graded(NOVICE_RIR, claims::HYP_RIR_ACC_001)
        } else if goal == LiftGoal::Hypertrophy {
            graded((0, 1), claims::CAL_GOAL_RIR_001)
        } else if easy_rung(id, idx) {
            graded((0, 1), claims::CAL_INTENSITY_FLOOR_001)
        } else {
            graded((1, 2), claims::CAL_INTENSITY_001)
        }),
    }
}

/// True when advancing this rung keys on reps/hold achieved ONLY, never on a
/// reported RIR ceiling: ballistic work, and unloaded self-limiting holds or
/// rep bands reaching [`HIGH_REP_BAND_TOP`]+ (`target` = the prescribed working
/// band), where self-reported reps in reserve are least accurate
/// (CAL-RIR-ACC-001).
pub fn performance_gated(tier: &Tier, target: TierTarget, loaded: bool) -> bool {
    match tier.failure {
        FailurePolicy::Ballistic => true,
        FailurePolicy::LeaveRir(_) => false,
        FailurePolicy::SelfLimiting => {
            !loaded && (target.is_timed() || target.top() >= HIGH_REP_BAND_TOP)
        }
    }
}

/// Hypertrophy-goal working-band minimums on unloaded self-limiting rep rungs:
/// floor ≥ 6, top ≥ 12, inside the 6–15-rep calisthenics template B window
/// (CAL-TEMPLATE-001, CAL-GOAL-RIR-001).
const HYPERTROPHY_REPS_MIN: (u8, u8) = (6, 12);

/// Longest submaximal working hold on a straight-arm skill rung, seconds
/// (CAL-PROG-STATIC-001: "~4-5×5-10 s working holds").
const WORKING_HOLD_MAX_SEC: u16 = 10;

/// The working band prescribed at a rung for a goal:
/// - straight-arm skill holds → a submaximal working hold of 5 s up to ~75%
///   of the test hold, capped at `WORKING_HOLD_MAX_SEC` (CAL-PROG-STATIC-001:
///   "~4-5×5-10 s working holds; advance when clean at target time"); the test
///   hold is the table band top ([`test_hold_sec`]);
/// - unloaded self-limiting rep rungs on a hypertrophy goal → widened to the
///   6–15-rep template B window (`HYPERTROPHY_REPS_MIN`; CAL-TEMPLATE-001,
///   CAL-GOAL-RIR-001);
/// - everything else → the KB table band verbatim.
pub fn working_target(tier: &Tier, goal: LiftGoal, loaded: bool) -> TierTarget {
    match tier.target {
        HoldSec { low, high } if tier.straight_arm => HoldSec {
            low,
            high: (high * 3 / 4).min(WORKING_HOLD_MAX_SEC).max(low),
        },
        Reps { low, high }
            if goal == LiftGoal::Hypertrophy
                && !loaded
                && tier.failure == FailurePolicy::SelfLimiting =>
        {
            Reps {
                low: low.max(HYPERTROPHY_REPS_MIN.0),
                high: high.max(HYPERTROPHY_REPS_MIN.1),
            }
        }
        t => t,
    }
}

/// The clean test hold that advances a straight-arm skill rung (the KB band
/// top: ~15 s, ~10 s on straddle/full stages), done once at the end of a
/// session, apart from the submaximal working holds (CAL-PROG-STATIC-001).
pub fn test_hold_sec(tier: &Tier) -> Option<u16> {
    match tier.target {
        HoldSec { high, .. } if tier.straight_arm => Some(high),
        _ => None,
    }
}

/// The reps/hold that count as "top of the band" for advancing: the test hold
/// on a straight-arm skill rung, else the working band's top.
pub fn advance_top(tier: &Tier, goal: LiftGoal, loaded: bool) -> u16 {
    test_hold_sec(tier).unwrap_or_else(|| working_target(tier, goal, loaded).top())
}

/// Reps at or above which a strength/power-goal set has become an endurance
/// stimulus, so the rung is far too easy ("above ~15 reps", CAL-PLACE-001).
const STRENGTH_GOAL_SKIP_REPS: u16 = 16;

/// Multi-rung skip threshold (CAL-PLACE-001, contested CQ-F11-01): reps at or
/// above 1.5× the working band top — or `STRENGTH_GOAL_SKIP_REPS` on a
/// strength/power goal, where such a set has become an endurance stimulus —
/// mean the rung is far too
/// easy, so the plan steps more than one rung after a single session. Only for
/// unloaded self-limiting rep rungs: never on straight-arm, leave-RIR, timed or
/// ballistic work (tendon and failure-safety rules).
pub fn skip_threshold(tier: &Tier, goal: LiftGoal) -> Recommended<Option<u16>> {
    let th = if tier.straight_arm
        || tier.target.is_timed()
        || tier.failure != FailurePolicy::SelfLimiting
    {
        None
    } else {
        let top = working_target(tier, goal, false).top();
        let one_and_half = (top * 3).div_ceil(2);
        Some(match goal {
            LiftGoal::Hypertrophy => one_and_half,
            LiftGoal::MaxStrength | LiftGoal::Power => one_and_half.min(STRENGTH_GOAL_SKIP_REPS),
        })
    };
    graded(th, claims::CAL_PLACE_001)
}

/// Rungs a CAL-PLACE-001 skip moves up.
pub const SKIP_STEPS: u8 = 2;

/// Ladder re-entry after a break from one exercise (CAL-REENTRY-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reentry {
    /// Rungs to step down from the last rung.
    pub steps: u8,
    /// The break is long enough to re-test the starting rung (> 8 weeks).
    pub retest: bool,
}

/// A break shorter than this many days holds the rung (CAL-REENTRY-001: under
/// 2 weeks).
const REENTRY_HOLD_DAYS: i64 = 14;
/// A break longer than this many days re-tests the rung (CAL-REENTRY-001:
/// over 8 weeks).
const REENTRY_RETEST_AFTER_DAYS: i64 = 56;

/// Map the days since a ladder was last trained onto rungs (CAL-REENTRY-001):
/// under 2 weeks hold the rung; 2–8 weeks step down one rung (straight-arm
/// rungs also restart their acclimation window); over 8 weeks re-test, starting
/// two rungs down so the single-session skip (CAL-PLACE-001) can place the
/// athlete back up from one max-reps set.
pub fn reentry(gap_days: i64) -> Recommended<Reentry> {
    let r = if gap_days < REENTRY_HOLD_DAYS {
        Reentry {
            steps: 0,
            retest: false,
        }
    } else if gap_days <= REENTRY_RETEST_AFTER_DAYS {
        Reentry {
            steps: 1,
            retest: false,
        }
    } else {
        Reentry {
            steps: 2,
            retest: true,
        }
    };
    graded(r, claims::CAL_REENTRY_001)
}

/// Plate grid for added calisthenics load (kg): the smallest common plate.
pub const LOAD_GRID_KG: f64 = 1.25;

/// The pounds-profile plate grid in kg: the 2.5 lb small plate (1 lb =
/// 0.45359237 kg exactly), so a lb user never sees a fractional-lb load.
pub const LOAD_GRID_LB_IN_KG: f64 = 2.5 * 0.45359237;

/// The added-load plate grid in kg for the profile's display unit.
pub fn load_grid_kg(weight_lb: bool) -> f64 {
    if weight_lb {
        LOAD_GRID_LB_IN_KG
    } else {
        LOAD_GRID_KG
    }
}

/// `kg` floored onto the profile's plate grid (tolerant of float error, so an
/// on-grid value never drops a plate).
pub fn floor_to_load_grid(kg: f64, weight_lb: bool) -> f64 {
    let grid = load_grid_kg(weight_lb);
    (kg / grid + 1e-9).floor() * grid
}

/// Share of the system load (bodyweight + added) one weighted-rung load step
/// adds: the conservative low end of STR-LOADPROG-001's 2–10%.
pub const LOAD_STEP_SYSTEM_FRAC: f64 = 0.025;

/// One added-load step at a weighted rung (STR-LOADPROG-001: add 2–10% once
/// the target is beaten; the conservative low end): [`LOAD_STEP_SYSTEM_FRAC`] of the system load
/// (bodyweight + added) with a one-plate minimum, rounded UP to the plate grid
/// (1.25 kg, or 2.5 lb for a pounds profile). With no bodyweight on the
/// profile the step is one plate.
pub fn load_step_kg(
    bodyweight_kg: Option<f64>,
    added_kg: f64,
    weight_lb: bool,
) -> Recommended<f64> {
    let grid = load_grid_kg(weight_lb);
    let system = bodyweight_kg.filter(|b| *b > 0.0).unwrap_or(0.0) + added_kg.max(0.0);
    let raw = (LOAD_STEP_SYSTEM_FRAC * system).max(grid);
    // Tolerance so a float-exact grid value (2.5000000000000004) is not bumped
    // a whole plate.
    let step = (raw / grid - 1e-9).ceil() * grid;
    graded(step, claims::STR_LOADPROG_001)
}

/// Effective system load (kg) for a MEASURED-fraction rung:
/// `mid(fraction) × bodyweight + added_kg`. `None` for inferred rungs — never
/// surface an invented number (CAL-LOAD-INFERRED-001).
pub fn effective_load_kg(tier: &Tier, bodyweight_kg: f64, added_kg: f64) -> Option<f64> {
    tier.bw_fraction
        .map(|(lo, hi)| (lo + hi) / 2.0 * bodyweight_kg + added_kg)
}

/// Highest effective rep count a calisthenics e1RM is estimated from
/// (CAL-E1RM-001: "≤ ~12 reps"); distinct from the barbell
/// [`crate::strength::E1RM_RELIABLE_REP_CAP`].
pub const CALIS_E1RM_REP_CAP: u32 = 12;

/// Estimated 1-rep max on TOTAL system load for a loaded ladder set
/// (CAL-E1RM-001: e1RM only for weighted calisthenics, on system load, at
/// ≤ [`CALIS_E1RM_REP_CAP`] reps). System load = measured %BW × bodyweight +
/// added; for the weighted (two-arm) pull-up bodyweight + added, since a
/// pull-up moves ~90–100%
/// of bodyweight (CAL-LOAD-PULL-001). `None` for an unloaded set, too many
/// reps, no bodyweight, or a rung with no known fraction (the one-arm top has
/// none).
pub fn system_load_e1rm(
    id: LadderId,
    idx: u8,
    bodyweight_kg: f64,
    added_kg: f64,
    reps: u32,
    rpe: f64,
) -> Option<Recommended<f64>> {
    if added_kg <= 0.0
        || bodyweight_kg <= 0.0
        || reps == 0
        || crate::strength::effective_reps(reps, rpe) > CALIS_E1RM_REP_CAP
    {
        return None;
    }
    let t = tier(id, idx)?;
    let system = match effective_load_kg(t, bodyweight_kg, added_kg) {
        Some(s) => s,
        None if id == VerticalPull && idx == 4 => bodyweight_kg + added_kg,
        None => return None,
    };
    let e1 = crate::strength::e1rm_epley_rpe(system, reps, rpe);
    (e1 > 0.0 && e1.is_finite()).then(|| graded((e1 * 10.0).round() / 10.0, claims::CAL_E1RM_001))
}

/// Ballistic dosing for a bodyweight Power goal (CAL-POWER-001 dose via
/// STR-PWR-001: 3–6 sets of 1–5 reps, 3–5 min rest, never to failure): 3–5
/// sets, rest 180 s, capped so foot/hand contacts stay under the beginner
/// plyometric ceiling of 80 per item (PLYO-001, safety-critical).
pub fn ballistic_sets(tier: &Tier) -> Recommended<u8> {
    let per_set = tier.target.top() as u32 * if tier.unilateral { 2 } else { 1 };
    let cap = (PLYO_CONTACT_CAP / per_set.max(1)).min(5) as u8;
    graded(cap.max(1), claims::PLYO_001)
}

/// Beginner plyometric contact ceiling per item (PLYO-001: beginner 80–100
/// contacts per session; the low end).
pub const PLYO_CONTACT_CAP: u32 = 80;

/// Rest between ballistic sets, seconds (STR-PWR-001: 3–5 min; the low end).
pub const BALLISTIC_REST_SEC: u16 = 180;

/// Goal → working-set template (CAL-TEMPLATE-001): strength/power = full-body
/// 3-5 sets, 3-5 min rest; hypertrophy = 3-4 sets, 1.5 min rest. Skill items
/// bypass this (fixed 4 sets, rest fully between holds).
pub fn session_template(goal: LiftGoal) -> Recommended<CalisTemplate> {
    let t = match goal {
        LiftGoal::Hypertrophy => CalisTemplate {
            sets: (3, 4),
            rest_sec: 90,
        },
        LiftGoal::MaxStrength | LiftGoal::Power => CalisTemplate {
            sets: (3, 5),
            rest_sec: 180,
        },
    };
    graded(t, claims::CAL_TEMPLATE_001)
}

#[cfg(test)]
mod tests;
