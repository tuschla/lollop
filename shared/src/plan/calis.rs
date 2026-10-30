//! Calisthenics day synthesis: leverage-ladder rung resolution and graded
//! skill / pattern / power prescriptions (CAL-*).

use crate::app::Profile;
use crate::calisthenics::{self, FailurePolicy, Kit};
use crate::evidence;
use crate::evidence::claims;
use crate::individualization::{TrainingAge, training_age_from_cadence};
use crate::schema::{
    CalisthenicsPrescription, LadderId, LiftSessionType, Prescription, Recommended, Session,
    SessionType, TempoBridge,
};
use crate::strength::LiftGoal;

use super::lift::sized_sets;
use super::{Anchors, CalisAnchor, DayRegion};

/// The tempo bridge on the easier rung after a failed step up
/// (CAL-PROG-TEMPO-001 / CAL-PROG-BRIDGE-001): 3 s lowering + 1 s pause, so a rep
/// takes ~5 s including the lift, inside the 0.5–8 s/rep window (>10 s is
/// inferior for growth, so the bridge never approaches it).
const TEMPO_BRIDGE: TempoBridge = TempoBridge {
    eccentric_sec: 3,
    pause_sec: 1,
};

/// Fixed working sets for a skill hold (KB "4-5 × 5-10 s"); volume-discounted, so
/// it bypasses `sized_sets` (CAL-VOLCOUNT-SKILL-001).
const SKILL_SETS: u8 = 4;

/// The region of lift day `i` (`Full` past the end of `regions`).
fn region_at(regions: &[DayRegion], i: usize) -> DayRegion {
    regions.get(i).copied().unwrap_or(DayRegion::Full)
}

/// The lift days of the week before day `i`.
fn days_before(regions: &[DayRegion], i: usize) -> &[DayRegion] {
    &regions[..i.min(regions.len())]
}

/// Whether a lift day of this week trains core (CAL-FREQ-VOL-002: every pattern
/// ≥2×/wk): every Lower day; on Full days every day when the week has ≤ 2
/// exposures, otherwise alternate Full days, so a 3-day week keeps 6–7 items.
fn core_on(regions: &[DayRegion], day_index: usize, n_exposures: usize) -> bool {
    match region_at(regions, day_index) {
        DayRegion::Lower => true,
        DayRegion::Upper => false,
        DayRegion::Full => {
            n_exposures <= 2
                || days_before(regions, day_index)
                    .iter()
                    .filter(|r| matches!(r, DayRegion::Full))
                    .count()
                    .is_multiple_of(2)
        }
    }
}

/// How one ladder item is dosed.
#[derive(Clone, Copy)]
enum Dose {
    /// Dynamic pattern: `weekly_sets` per pattern split over the `sessions`
    /// that train it this week (CAL-FREQ-VOL-002), into the goal template.
    Pattern { weekly_sets: u8, sessions: usize },
    /// Skill hold track: fixed working sets, full rest between holds.
    Skill,
    /// Ballistic power item (CAL-POWER-001): contact-capped sets, 3 min rest.
    Ballistic,
}

struct Ctx<'a> {
    profile: &'a Profile,
    anchors: &'a Anchors,
    kit: Kit,
    ta: TrainingAge,
    template: calisthenics::CalisTemplate,
}

/// One full-body / upper / lower calisthenics day:
/// 1. on a Power goal, ballistic items FIRST while fresh (CAL-POWER-001): a jump
///    ladder on lower-body days, an explosive push-up ladder on upper-body days;
/// 2. skill work (≤2 items, upper-body days only — CAL-TEMPLATE-001);
/// 3. pattern items so every plane trains ≥2×/wk (CAL-FREQ-VOL-002): upper days
///    carry horizontal push, one vertical-push-family item (dip and vertical
///    push alternate by upper-day ordinal), vertical pull and horizontal pull;
///    lower days carry squat, hinge and core (core alternates on Full-only weeks
///    of 3+ days).
///
/// `day_index` is this day's 0-based slot among the week's lift days,
/// `regions[day_index]` its region, `n_exposures` how often each pattern is
/// trained per week (the weekly-set split divisor). Equipment the athlete lacks
/// re-allocates a slot to the other plane of the same family (CAL-EQUIP-001):
/// no bar/rings → horizontal pull carries the vertical-pull volume; no dip
/// bars/rings → vertical push takes every vertical-push-family slot.
pub(super) fn calisthenics_session(
    profile: &Profile,
    anchors: &Anchors,
    day_index: usize,
    day: u16,
    n_exposures: usize,
    regions: &[DayRegion],
) -> Session {
    let goal = profile.lift_goal;
    let ctx = Ctx {
        profile,
        anchors,
        kit: Kit::from_profile(profile),
        ta: training_age_from_cadence(profile.progression_cadence).value,
        template: calisthenics::session_template(goal).value,
    };
    let region = region_at(regions, day_index);
    let upper = region.has_upper();
    let lower = region.has_lower();
    let upper_ordinal = days_before(regions, day_index)
        .iter()
        .filter(|r| r.has_upper())
        .count();
    let n = n_exposures.max(1);
    let weekly = profile.weekly_sets;
    let mut prescriptions: Vec<Recommended<Prescription>> = Vec::new();

    // 1. Power: ballistic items first.
    if goal == LiftGoal::Power {
        for (id, on) in [(LadderId::Jump, lower), (LadderId::PlyoPush, upper)] {
            if on && let Some(p) = ladder_prescription(&ctx, id, Dose::Ballistic) {
                prescriptions.push(p);
            }
        }
    }

    // 2. Skill items, ≤2/day on upper-body days: round-robin over the ACTIVE
    // skill-track anchors the equipment allows, in LadderId order, taking
    // [2*upper_ordinal % n ..] wrapping (deterministic).
    if upper {
        let skill_anchors: Vec<&CalisAnchor> = calisthenics::LADDERS
            .iter()
            .filter(|l| l.skill && calisthenics::ladder_available(l.id, ctx.kit))
            .filter_map(|l| anchors.calis_for(l.id).filter(|a| a.active))
            .collect();
        let k = skill_anchors.len();
        for j in 0..2.min(k) {
            let a = skill_anchors[(2 * upper_ordinal + j) % k];
            if let Some(p) = ladder_prescription(&ctx, a.ladder, Dose::Skill) {
                prescriptions.push(p);
            }
        }
    }

    // 3. Pattern items.
    let pattern = |weekly_sets: u8, sessions: usize| Dose::Pattern {
        weekly_sets,
        sessions,
    };
    let mut ids: Vec<(LadderId, Dose)> = Vec::new();
    if upper {
        ids.push((LadderId::HorizontalPush, pattern(weekly, n)));
        let vpush = if ctx.kit.dip_support() && upper_ordinal.is_multiple_of(2) {
            LadderId::Dip
        } else {
            LadderId::VerticalPush
        };
        ids.push((vpush, pattern(weekly, n)));
        let hang = calisthenics::ladder_available(LadderId::VerticalPull, ctx.kit);
        if hang {
            ids.push((LadderId::VerticalPull, pattern(weekly, n)));
        }
        let hpull_weekly = if hang {
            weekly
        } else {
            weekly.saturating_mul(2)
        };
        ids.push((LadderId::HorizontalPull, pattern(hpull_weekly, n)));
    }
    if lower {
        ids.push((LadderId::Squat, pattern(weekly, n)));
        ids.push((LadderId::Hinge, pattern(weekly, n)));
        if core_on(regions, day_index, n_exposures) {
            let core_sessions = (0..regions.len().max(day_index + 1))
                .filter(|&i| core_on(regions, i, n_exposures))
                .count()
                .max(1);
            ids.push((LadderId::Core, pattern(weekly, core_sessions)));
        }
    }
    for (id, dose) in ids {
        if let Some(p) = ladder_prescription(&ctx, id, dose) {
            prescriptions.push(p);
        }
    }

    // A calisthenics day is `Lift(Calisthenics)`, not a barbell
    // Heavy/Power/Volume day.
    Session {
        session_type: SessionType::Lift(LiftSessionType::Calisthenics),
        day,
        prescriptions,
        dup_day: None,
    }
}

/// The resolved rung for one ladder item plus everything the view explains.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    rung: u8,
    added_kg: f64,
    tempo_bridge: Option<TempoBridge>,
    reentry_steps: u8,
    retest: bool,
    substituted: bool,
    unplaced: bool,
    skipped: bool,
    needs_effort_rating: bool,
}

/// A tempo bridge only fits an unloaded self-limiting REP rung (a hold or a
/// leave-RIR eccentric rung is its own bridge).
fn tempo_for(id: LadderId, rung: u8) -> Option<TempoBridge> {
    calisthenics::tier(id, rung)
        .filter(|t| t.failure == FailurePolicy::SelfLimiting && !t.target.is_timed())
        .map(|_| TEMPO_BRIDGE)
}

/// One added-load step for this profile (STR-LOADPROG-001).
fn load_step(ctx: &Ctx, added: f64) -> f64 {
    calisthenics::load_step_kg(ctx.profile.bodyweight_kg, added, ctx.profile.weight_lb).value
}

/// The lightest load rung `r` is ever prescribed at: a "Weighted …" rung starts
/// at the first load step, every other rung at bodyweight.
fn min_load(ctx: &Ctx, id: LadderId, r: u8) -> f64 {
    if calisthenics::tier(id, r).is_some_and(calisthenics::inherently_loaded) {
        load_step(ctx, 0.0)
    } else {
        0.0
    }
}

/// Re-entry after a break from the ladder (CAL-REENTRY-001): step down `steps`
/// available rungs. A loaded rung first sheds its load as one step, unless the
/// rung's lightest prescription (a "Weighted …" rung's first load step) is no
/// lighter, when it steps down a rung instead. The load is always dropped.
fn reentry_rung(ctx: &Ctx, id: LadderId, base: u8, added: f64, steps: u8) -> u8 {
    let steps = if added > 0.0 && min_load(ctx, id, base) < added {
        steps - 1
    } else {
        steps
    };
    let mut rung = base;
    for _ in 0..steps {
        if let Some(b) = calisthenics::available_below(id, rung, ctx.kit) {
            rung = b;
        }
    }
    rung
}

/// Advance, returning `(rung, added_kg)`. On a load-capable rung
/// (`weighted_ok`): stay and add one load step, or start at the first load
/// step when unloaded (STR-LOADPROG-001); the pull-up fork resolves to load,
/// the leverage branch stays one "Harder" choice away. Elsewhere step up
/// `steps` available rungs; a skip stops short of a leave-RIR rung.
fn advance(ctx: &Ctx, id: LadderId, base: u8, added: f64, steps: u8) -> (u8, f64) {
    if calisthenics::weighted_ok(id, base) {
        let added = if added > 0.0 && ctx.profile.weight_lb {
            // A converted-from-kg load lands back on the 2.5 lb grid (always
            // still above the old load: the step is at least one plate).
            calisthenics::floor_to_load_grid(added + load_step(ctx, added), true)
        } else if added > 0.0 {
            added + load_step(ctx, added)
        } else {
            load_step(ctx, 0.0)
        };
        return (base, added);
    }
    let mut rung = base;
    for k in 0..steps.max(1) {
        let Some(up) = calisthenics::available_above(id, rung, ctx.kit) else {
            break;
        };
        if k > 0
            && calisthenics::tier(id, up).is_none_or(|t| t.failure != FailurePolicy::SelfLimiting)
        {
            break;
        }
        rung = up;
    }
    (rung, added)
}

/// Regress, returning `(rung, added_kg, tempo_bridge)`: a loaded rung sheds one
/// load step when that is actually lighter; else step down one rung with a
/// tempo bridge (CAL-PROG-TEMPO-001; KB §2.2 "step down one rung").
fn regress(ctx: &Ctx, id: LadderId, base: u8, added: f64) -> (u8, f64, Option<TempoBridge>) {
    let lighter = if added > 0.0 {
        calisthenics::floor_to_load_grid(
            (added - load_step(ctx, added)).max(0.0),
            ctx.profile.weight_lb,
        )
    } else {
        0.0
    };
    let stay_load = if lighter > 0.0 {
        lighter
    } else {
        min_load(ctx, id, base)
    };
    if added > 0.0 && stay_load < added {
        (base, lighter, None)
    } else if let Some(b) = calisthenics::available_below(id, base, ctx.kit) {
        (b, added, tempo_for(id, b))
    } else {
        (base, added, None)
    }
}

/// Resolve the working rung and added load from the ladder's anchor: no anchor
/// → the easiest rung the equipment allows (`unplaced`); otherwise re-entry,
/// advance, regress, or hold (keeping the bridge while the rung was entered by
/// a regress).
///
/// A regress or re-entry never returns more load than last time on the same
/// rung. A rung the equipment cannot support is substituted by the nearest one
/// that it can (CAL-EQUIP-001); `None` when the whole ladder needs missing kit.
fn resolve(id: LadderId, anchor: Option<&CalisAnchor>, ctx: &Ctx) -> Option<Resolved> {
    let kit = ctx.kit;
    let n = calisthenics::ladder(id).tiers.len() as u8;
    let mut res = Resolved {
        rung: 0,
        added_kg: 0.0,
        tempo_bridge: None,
        reentry_steps: 0,
        retest: false,
        substituted: false,
        unplaced: false,
        skipped: false,
        needs_effort_rating: false,
    };
    let Some(a) = anchor else {
        res.rung = calisthenics::nearest_available(id, 0, kit)?;
        res.substituted = res.rung != 0;
        res.unplaced = true;
        return Some(res);
    };
    let base = a.tier.min(n - 1);
    let added = if calisthenics::weighted_ok(id, base) {
        a.last_added_kg.max(0.0)
    } else {
        0.0
    };
    let (mut rung, mut added) = if a.reentry_steps > 0 {
        res.reentry_steps = a.reentry_steps;
        res.retest = a.retest;
        (reentry_rung(ctx, id, base, added, a.reentry_steps), 0.0)
    } else if a.advance_ready {
        let (rung, added) = advance(ctx, id, base, added, a.advance_steps);
        res.skipped = rung > base.saturating_add(1);
        (rung, added)
    } else if a.regress {
        let (rung, added, bridge) = regress(ctx, id, base, added);
        res.tempo_bridge = bridge;
        (rung, added)
    } else {
        res.needs_effort_rating = a.needs_effort_rating;
        if a.bridged {
            res.tempo_bridge = tempo_for(id, base);
        }
        (base, added)
    };
    if !calisthenics::rung_available(id, rung, kit) {
        rung = calisthenics::nearest_available(id, rung, kit)?;
        res.substituted = true;
        res.tempo_bridge = None;
    }
    // Load belongs to the rung it was lifted on: a new rung starts unloaded,
    // unless the rung itself is a "Weighted …" variant (first load step).
    let inherently_loaded =
        calisthenics::tier(id, rung).is_some_and(calisthenics::inherently_loaded);
    if rung != base || !calisthenics::weighted_ok(id, rung) {
        added = 0.0;
    }
    if added <= 0.0 && inherently_loaded {
        added = load_step(ctx, 0.0);
    }
    res.rung = rung;
    res.added_kg = added;
    Some(res)
}

/// Evidence face for one item. Safety first: ballistic power work cites
/// CAL-POWER-001; straight-arm, leave-RIR and loaded rungs the safety_critical
/// CAL-EFAIL-SAFE-001; then the rule that shaped the rung (re-entry, bridge,
/// equipment swap, multi-rung skip); else the hold / novice-RIR / goal-RIR /
/// ladder claim.
fn evidence_claim(
    tier: &calisthenics::Tier,
    res: &Resolved,
    goal: LiftGoal,
    novice: bool,
    table_row: bool,
) -> evidence::ClaimId {
    let loaded = res.added_kg > 0.0;
    if tier.failure == FailurePolicy::Ballistic {
        claims::CAL_POWER_001
    } else if tier.straight_arm || loaded || matches!(tier.failure, FailurePolicy::LeaveRir(_)) {
        claims::CAL_EFAIL_SAFE_001
    } else if res.reentry_steps > 0 {
        claims::CAL_REENTRY_001
    } else if res.tempo_bridge.is_some() {
        claims::CAL_PROG_TEMPO_001
    } else if res.substituted || table_row {
        claims::CAL_EQUIP_001
    } else if res.skipped {
        claims::CAL_PLACE_001
    } else if tier.target.is_timed() {
        claims::CAL_PROG_STATIC_001
    } else if novice {
        claims::HYP_RIR_ACC_001
    } else if goal == LiftGoal::Hypertrophy {
        claims::CAL_GOAL_RIR_001
    } else {
        claims::CAL_PROG_MECHANIC_001
    }
}

/// One ladder item, fully dosed. `None` when the ladder needs equipment the
/// athlete lacks.
fn ladder_prescription(ctx: &Ctx, id: LadderId, dose: Dose) -> Option<Recommended<Prescription>> {
    let anchor = ctx.anchors.calis_for(id);
    let res = resolve(id, anchor, ctx)?;
    let tier = calisthenics::tier(id, res.rung)?;
    let (sets, rest_sec) = match dose {
        // Calisthenics volume is rung-based, not block-ramped, so the sizing runs
        // as week 1 of a single-week block (the ramp is then a no-op); tier names
        // never match the name-keyed landmarks, so `None`.
        Dose::Pattern {
            weekly_sets,
            sessions,
        } => (
            sized_sets(weekly_sets, ctx.ta, sessions, ctx.template.sets, None, 1, 1).per_session,
            ctx.template.rest_sec,
        ),
        Dose::Skill => (SKILL_SETS, 0),
        Dose::Ballistic => (
            calisthenics::ballistic_sets(tier).value,
            calisthenics::BALLISTIC_REST_SEC,
        ),
    };
    let loaded = res.added_kg > 0.0;
    let load_capable = calisthenics::weighted_ok(id, res.rung);
    let ballistic = tier.failure == FailurePolicy::Ballistic;
    // Actual external load on a rung (dip belt / vest / plates) reclassifies the
    // set to weighted work: leave RIR, never blind failure, safety_critical
    // (CAL-EFAIL-SAFE-001); a novice keeps the novice band. Ballistic work has
    // no numeric RIR (see the schema).
    let novice = ctx.ta == TrainingAge::Novice;
    let rir = if loaded {
        if novice {
            calisthenics::NOVICE_RIR
        } else {
            (1, 2)
        }
    } else {
        calisthenics::rir_band(id, res.rung, ctx.profile.lift_goal, novice)
            .map_or((0, 0), |r| r.value)
    };
    let skip_reps = if loaded {
        None
    } else {
        calisthenics::skip_threshold(tier, ctx.profile.lift_goal)
            .value
            .filter(|_| calisthenics::available_above(id, res.rung, ctx.kit).is_some())
    };
    // No bar or rings to row from: the floor-level rows are done under a
    // sturdy table, gripping its edge (CAL-EQUIP-001).
    let table_row = id == LadderId::HorizontalPull && !ctx.kit.hang();
    let pres = Prescription::Calisthenics(CalisthenicsPrescription {
        ladder: id,
        tier: res.rung,
        exercise: tier.name.to_string(),
        sets,
        target: calisthenics::working_target(tier, ctx.profile.lift_goal, loaded),
        rir,
        rest_sec,
        added_kg: load_capable.then_some(res.added_kg),
        load_capable,
        tempo_bridge: res.tempo_bridge,
        test_hold_sec: calisthenics::test_hold_sec(tier),
        ballistic,
        easier: calisthenics::available_below(id, res.rung, ctx.kit),
        harder: calisthenics::available_above(id, res.rung, ctx.kit),
        needs_effort_rating: res.needs_effort_rating,
        reentry_steps: res.reentry_steps,
        // The "one max-reps test set" re-places only through the one-session
        // skip, so it is offered only where a skip threshold exists (never on
        // straight-arm or leave-RIR rungs, where failure is disallowed).
        retest: res.retest && skip_reps.is_some(),
        substituted: res.substituted,
        unplaced: res.unplaced,
        table_row,
        skip_reps,
    });
    Some(evidence::graded(
        pres,
        evidence_claim(tier, &res, ctx.profile.lift_goal, novice, table_row),
    ))
}
