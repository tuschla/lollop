//! Lift-day synthesis: pattern-balanced exercise selection, the isolation /
//! movement taxonomy, goal-aware DUP emphasis, and volume-sized, evidence-graded
//! lift prescriptions.

use crate::app::Profile;
use crate::evidence::{ClaimId, claims};
use crate::hypertrophy;
use crate::individualization::TrainingAge;
use crate::schema::{
    LiftIntensity, LiftPrescription, LiftSessionType, Prescription, Recommended, Session,
    SessionType,
};
use crate::strength::{self, DupDay, LiftGoal, LoadingRx};

use super::Anchors;

/// The core movement patterns a lift day covers (STR-SESSION-BALANCE-001):
/// squat / hinge / push / pull, vertical presses and pulls folded into `Push` /
/// `Pull`; the catalog fills in [`CORE_PATTERN_FILLS`] are the KB's horizontal
/// defaults. The cited principle is that multi-joint compounds do not fully
/// activate every muscle (Schoenfeld 2019, Moderate). The pattern list, its name
/// keywords ([`movement_pattern`], [`is_isolation_lift`]) and the per-day lift
/// count are EXPERT-OPINION, flagged in the claim's `contradicting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MovementPattern {
    Squat,
    Hinge,
    Push,
    Pull,
}

/// Classify an exercise name into a core movement pattern, conservatively:
/// whole-word, case-insensitive keyword matching (a plural token matches its
/// singular needle). Anything ambiguous (most isolation work, curls, raises,
/// calf work) is `None`; a logged unclassifiable lift is still scheduled.
pub(crate) fn movement_pattern(name: &str) -> Option<MovementPattern> {
    with_name_tokens(name, |tokens| {
        let has = |kw: &str| has_phrase(tokens, kw);
        movement_pattern_of(&has)
    })
}

/// Lowercase ASCII word tokens of an exercise name, folded first
/// ([`crate::names::fold`]: "İncline" → "incline"), split on every
/// non-alphanumeric ASCII char, empties dropped, handed to `f`.
fn with_name_tokens<R>(name: &str, f: impl FnOnce(&[&str]) -> R) -> R {
    let lower = crate::names::fold(name);
    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    f(&tokens)
}

/// Whole-word phrase match: the needle's tokens must appear as a contiguous
/// run ("row" never fires inside "throw"/"narrow"; "bench press" only as an
/// adjacent pair), and "squats"/"presses"/"rows" match their singular needle.
fn has_phrase(tokens: &[&str], kw: &str) -> bool {
    let needle = || {
        kw.split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty())
    };
    let len = needle().count();
    let token_matches = |t: &str, n: &str| {
        t == n || t.strip_suffix('s') == Some(n) || t.strip_suffix("es") == Some(n)
    };
    len > 0
        && tokens
            .windows(len)
            .any(|w| w.iter().zip(needle()).all(|(t, n)| token_matches(t, n)))
}

fn movement_pattern_of(has: &dyn Fn(&str) -> bool) -> Option<MovementPattern> {
    // Order matters: hinge before squat (a "Romanian deadlift" is a hinge, not a
    // squat), and both before push (so "leg press" reads as squat, not press).
    if has("deadlift")
        || has("rdl")
        || has("good morning")
        || has("hip thrust")
        || has("hip hinge")
        || has("nordic")
        || has("kettlebell swing")
        || has("back extension")
        || has("hyperextension")
    {
        return Some(MovementPattern::Hinge);
    }
    if has("squat") || has("leg press") || has("lunge") || has("step up") || has("hack") {
        return Some(MovementPattern::Squat);
    }
    // Rows + vertical pulls (folded into Pull). Checked before Push so nothing
    // here needs to contain "press".
    if has("row")
        || has("pulldown")
        || has("pull down")
        || has("lat pull")
        || has("pull up")
        || has("pullup")
        || has("chin up")
        || has("chinup")
    {
        return Some(MovementPattern::Pull);
    }
    // Horizontal + vertical presses and their bodyweight analogues.
    if has("bench")
        || has("press")
        || has("push up")
        || has("pushup")
        || has("dip")
        || has("chest fly")
        || has("pec deck")
    {
        return Some(MovementPattern::Push);
    }
    None
}

/// KB core movement patterns + the File-03 catalog compound that FILLS each when
/// the user's log leaves it uncovered (exercise_entry-known names → real long-
/// length / SFR lookups). These are the KB's named horizontal defaults; using
/// them turns a one-pattern (e.g. leg-heavy) log into a balanced week
/// (STR-SESSION-BALANCE-001).
const CORE_PATTERN_FILLS: &[(MovementPattern, &str)] = &[
    (MovementPattern::Squat, "Back squat"),
    (MovementPattern::Hinge, "Romanian deadlift"),
    (MovementPattern::Push, "Bench press"),
    (MovementPattern::Pull, "Cable row"),
];

/// The fill for a core pattern under a lifting goal. A max-strength or power
/// goal fills the hinge with the conventional deadlift, the lift whose 1RM it
/// is training (KB strength-035, STR-COMP-ANCHOR-001: anchor strength work on
/// the competition lift; movement/ROM specificity); hypertrophy keeps the
/// File-03 catalog's Romanian deadlift (long-length hamstring work).
fn core_fill(pat: MovementPattern, catalog: &'static str, goal: LiftGoal) -> &'static str {
    match (pat, goal) {
        (MovementPattern::Hinge, LiftGoal::MaxStrength | LiftGoal::Power) => "Deadlift",
        _ => catalog,
    }
}

/// Exercises per lift day: the DUP primary plus accessories covering the other
/// core patterns; ~3-4 spans squat/hinge/push/pull (STR-SESSION-BALANCE-001).
const MAX_LIFTS_PER_DAY: usize = 4;

/// A lift day's exercises, pattern-balanced (STR-SESSION-BALANCE-001): the
/// user's logged lifts first (each anchored → a load), one per core pattern,
/// then catalog fills for the missing patterns (→ RIR). A strength or power goal
/// fills the hinge with the competition deadlift (STR-COMP-ANCHOR-001).
pub(super) fn main_lifts(anchors: &Anchors, goal: LiftGoal) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut covered: Vec<MovementPattern> = Vec::new();
    // 1. Logged lifts, newest first: one per core pattern (a repeat would
    //    unbalance the day), unclassifiable lifts always kept, never the same
    //    movement twice.
    for (name, _) in &anchors.lift_e1rm {
        if out.len() >= MAX_LIFTS_PER_DAY {
            break;
        }
        if out.iter().any(|n| same_movement(n, name)) {
            continue;
        }
        match movement_pattern(name) {
            Some(pat) => {
                if covered.contains(&pat) {
                    continue; // don't stack a pattern - balance by default
                }
                covered.push(pat);
                out.push(name.clone());
            }
            // Unclassifiable logged lift: conservative classification never drops
            // it (it covers no core pattern, so it can't clash with one).
            None => out.push(name.clone()),
        }
    }
    // List in plan order (squat → hinge → push → pull → unclassified) so the
    // main compound leads, not whichever lift was logged last. Stable: ties keep
    // recency order; the fills below follow the same order.
    out.sort_by_key(|n| match movement_pattern(n) {
        Some(MovementPattern::Squat) => 0u8,
        Some(MovementPattern::Hinge) => 1,
        Some(MovementPattern::Push) => 2,
        Some(MovementPattern::Pull) => 3,
        None => 4,
    });
    // 2. Fill the missing core patterns from the catalog; `same_movement` keeps
    //    a logged "Smith machine back squat" from repeating as "Back squat".
    for &(pat, catalog) in CORE_PATTERN_FILLS {
        let def = core_fill(pat, catalog, goal);
        if out.len() >= MAX_LIFTS_PER_DAY {
            break;
        }
        if covered.contains(&pat) {
            continue;
        }
        if out.iter().any(|n| same_movement(n, def)) {
            continue;
        }
        out.push(def.to_string());
        covered.push(pat);
    }
    // 3. The DUP primary `out[0]` is undulated at %1RM every lift day, so it must
    //    be a pattern-classified lift with a reliable e1RM (not isolation,
    //    strength-006 / STR-E1RM-CHECK-001). Promote the first such lift; with
    //    none, the most recent lift stays first.
    let leads = |n: &String| !is_isolation_lift(n) && movement_pattern(n).is_some();
    if out.first().is_some_and(|n| !leads(n))
        && let Some(idx) = out.iter().position(leads)
    {
        out[..=idx].rotate_right(1);
    }
    out
}

#[cfg(test)]
thread_local! {
    /// Test-only: calls of the exercise-NAME classifiers ([`is_isolation_lift`],
    /// [`same_movement`], [`movement_key`], `calisthenics::tier_by_name`), the
    /// per-set work the app's `SetIndex` exists to avoid. A deterministic
    /// proxy for view time (`view_time_stays_bounded…`).
    pub(crate) static NAME_CLASSIFICATIONS: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

/// Counts one `NAME_CLASSIFICATIONS` call (test builds only; a no-op else).
pub(crate) fn count_name_classification() {
    #[cfg(test)]
    NAME_CLASSIFICATIONS.with(|c| c.set(c.get() + 1));
}

/// Whether a lift is an isolation movement, whose estimated 1RM is unreliable
/// (File 02 strength-006; STR-E1RM-CHECK-001). The one isolation taxonomy
/// (`hypertrophy::classify` defers to it): the Table 4 catalog class, then
/// [`ISOLATION_NEEDLES_STRONG`] (isolation even inside a compound pattern:
/// "Chest fly", "Pec deck"), then, for a name with no compound pattern,
/// [`ISOLATION_NEEDLES_WEAK`] ("leg extension" yes, "back extension" no) or a
/// single-muscle fragment ([`hypertrophy::isolation_muscle`]: "Skull crusher",
/// "Face pull", "Crunch").
pub(crate) fn is_isolation_lift(name: &str) -> bool {
    count_name_classification();
    if hypertrophy::exercise_entry(name)
        .is_some_and(|e| e.class == hypertrophy::ExerciseClass::Isolation)
    {
        return true;
    }
    with_name_tokens(name, |tokens| {
        let has = |kw: &str| has_phrase(tokens, kw);
        ISOLATION_NEEDLES_STRONG.iter().any(|kw| has(kw))
            || (movement_pattern_of(&has).is_none()
                && (ISOLATION_NEEDLES_WEAK.iter().any(|kw| has(kw))
                    || hypertrophy::isolation_muscle(name).is_some()))
    })
}

/// Single-joint needles that classify a name as isolation even when it also
/// matches a compound movement pattern.
const ISOLATION_NEEDLES_STRONG: &[&str] = &["curl", "fly", "flye", "flies", "shrug", "pec deck"];

/// Needles that also appear in compound movements, so they classify only a name
/// with no compound movement pattern.
const ISOLATION_NEEDLES_WEAK: &[&str] = &["raise", "extension"];

/// Whether two exercise names denote the same movement (dedup, completion):
/// their [`movement_key`]s match, so "Cable row" ≡ "Row" ≡ "Row (barbell/cable/
/// machine)" and "Push-up" ≡ "Push up", while "Front squat" ≠ "Back squat" and
/// "Deadlift" ≠ "Romanian deadlift". A bare "Squat" and "Back squat" stay two
/// lifts: merging them would need a movement-synonym lexicon the KB lacks.
pub(crate) fn same_movement(a: &str, b: &str) -> bool {
    count_name_classification();
    let (mut x, mut y) = (movement_tokens(a), movement_tokens(b));
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some(p), Some(q)) if crate::names::fold_eq(p, q) => {}
            _ => return false,
        }
    }
}

/// The normalized token sequence [`same_movement`] compares: two names are the
/// same movement iff their keys are equal, so callers that match one name
/// against many can compute each key once and hash or compare it. A legacy
/// catalog label ("Hack squat / leg press") keys as its current display name
/// ("Hack squat"), so history logged under the old label stays the same lift.
pub(crate) fn movement_key(name: &str) -> Vec<String> {
    count_name_classification();
    movement_tokens(name).map(crate::names::fold).collect()
}

/// The tokens of [`movement_key`], unfolded and unallocated: the canonical
/// name split on every non-alphanumeric char, empties and equipment words
/// dropped (compared with [`crate::names::fold_eq`]).
fn movement_tokens(name: &str) -> impl Iterator<Item = &str> {
    const EQUIPMENT: [&str; 7] = [
        "barbell",
        "dumbbell",
        "dumbbells",
        "kettlebell",
        "cable",
        "machine",
        "smith",
    ];
    hypertrophy::canonical_name(name)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty() && !EQUIPMENT.iter().any(|e| crate::names::fold_eq(t, e)))
}

/// Heavy lifting: at or above 80 % 1RM, the floor of the max-strength band a
/// main lift needs ≥2×/week (STR-HEAVYFREQ-001; File 02 strength-001
/// 80–100 %). Read by the CAP-3 heavy-leg spacing (`schedule::session_load`,
/// HYB-CAP-001) and the meet taper's heavy squat/bench cut.
pub(super) const HEAVY_PCT_1RM: f32 = 80.0;

/// The DUP emphasis for a lift day, GOAL-AWARE (`day_index % 3`, the day's
/// exposure index within its region, `plan::region_exposure_index`):
/// - MaxStrength: Heavy → HeavyVolume → Hypertrophy. Each main lift gets ≥80 %
///   1RM in ≥2 sessions a week at 2+ full-body days or an upper/lower split;
///   the lighter day adds to the heavy ones rather than replacing one
///   (STR-HEAVYFREQ-001; strength-001 80–100 %).
/// - Power, strong athlete: the classic Heavy → Power → Hypertrophy week
///   (strength-023).
/// - Power, novice or below the ~1.5×BW squat benchmark (`power_base`):
///   Heavy → Power → HeavyVolume — two heavy days and one power day, because
///   weaker athletes gain jump/sprint performance about as much from heavy
///   strength work and far more 1RM (STR-BASE-POWER-001).
///
/// A Hypertrophy goal runs its own week (`plan::hypertrophy`) and never reads
/// this; it takes the MaxStrength row.
pub(super) fn dup_emphasis(goal: LiftGoal, power_base: bool, day_index: usize) -> DupDay {
    let base = match goal {
        LiftGoal::MaxStrength | LiftGoal::Hypertrophy => {
            [DupDay::Heavy, DupDay::HeavyVolume, DupDay::Hypertrophy]
        }
        LiftGoal::Power if power_base => [DupDay::Heavy, DupDay::Power, DupDay::HeavyVolume],
        LiftGoal::Power => [DupDay::Heavy, DupDay::Power, DupDay::Hypertrophy],
    };
    base[day_index % 3]
}

/// Whether a Power-goal athlete should run the strength-weighted week
/// (STR-BASE-POWER-001): a novice, or anyone whose back-squat e1RM is not yet
/// shown to reach the ~1.5× bodyweight benchmark (the depth-jump prerequisite,
/// PLYO-PREREQ-001; ExpertOpinion as a threshold). Unknown bodyweight or no
/// back-squat anchor counts as "not yet shown". Always `false` for other goals.
pub(super) fn power_needs_strength_base(
    profile: &Profile,
    anchors: &Anchors,
    ta: TrainingAge,
) -> bool {
    profile.lift_goal == LiftGoal::Power
        && (ta == TrainingAge::Novice || !squat_benchmark_met(profile, anchors))
}

/// Back-squat working e1RM ≥ 1.5× bodyweight (`strength::depth_jump_ready`).
fn squat_benchmark_met(profile: &Profile, anchors: &Anchors) -> bool {
    match (profile.bodyweight_kg, squat_e1rm(anchors)) {
        (Some(bw), Some(sq)) => strength::depth_jump_ready(sq, bw).value,
        _ => false,
    }
}

/// The working e1RM of the athlete's anchored barbell back squat.
fn squat_e1rm(anchors: &Anchors) -> Option<f64> {
    anchors
        .lift_e1rm
        .iter()
        .find(|(n, _)| is_back_squat(n))
        .and_then(|(n, _)| anchors.working_e1rm_for(n))
}

/// The barbell back squat, the PLYO-PREREQ-001 benchmark lift (File 02
/// strength-033). A hack, goblet, split or front squat is another movement, and
/// a "smith" / "machine" token is rejected before the equipment-blind
/// [`same_movement`] match. Shared by the plan's depth-jump gate and the lift
/// audit's.
pub(crate) fn is_back_squat(name: &str) -> bool {
    !name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|t| t.eq_ignore_ascii_case("smith") || t.eq_ignore_ascii_case("machine"))
        && same_movement("Barbell back squat", name)
}

/// The loading goal a DUP emphasis targets, so its inter-set REST comes from the
/// band the day actually prescribes. File 02 strength-023's Heavy day is a
/// max-strength quality (85–90 %), the Power day a power quality (50–70 % fast),
/// the Hypertrophy day a hypertrophy quality (70–75 %); each has its own rest
/// (`loading_rx`): 180–300 s for heavy/power, 30–120 s for hypertrophy.
/// Attaching the *goal's* band minimum to every day would put 30 s rest on
/// 85 % triples for a hypertrophy-goal lifter.
fn emphasis_goal(emphasis: DupDay) -> LiftGoal {
    match emphasis {
        DupDay::Heavy | DupDay::HeavyVolume => LiftGoal::MaxStrength,
        DupDay::Power => LiftGoal::Power,
        DupDay::Hypertrophy => LiftGoal::Hypertrophy,
    }
}

/// Block/session context threaded from `synthesize` into the lift builders: how
/// many times each muscle is trained per week (`n_lift_days`, the volume-split
/// denominator: 2-day 2, 3-day 3, 4-day upper/lower 2, 5-day 3), the 1-based
/// week within the accumulation block, the block length, this day's 0-based
/// position among the week's lift days in weekday order (`slot`), and the
/// region every lift day of the week trains (`week_regions`, LIFT-DAYS-001
/// templates), so this day's region is `week_regions.as_slice()[slot]`.
#[derive(Clone, Copy)]
pub(super) struct BlockCtx {
    pub(super) n_lift_days: usize,
    pub(super) week_in_block: u8,
    pub(super) block_weeks: u8,
    pub(super) slot: usize,
    pub(super) week_regions: super::WeekRegions,
}

/// Build one lift day. Intermediate+ undulate via `dup_day_rx`; novices run a
/// linear `loading_rx` day. An anchored lift is prescribed by %1RM; an unlogged
/// one by RIR (never an invented load, HARD RULE 1). `week_in_block` /
/// `block_weeks` locate the day inside the accumulation block, so the same
/// weekday progresses across the block instead of repeating week 1.
pub(super) fn lift_session(
    profile: &Profile,
    anchors: &Anchors,
    ta: TrainingAge,
    lift_names: &[String],
    day_index: usize,
    day: u16,
    block: BlockCtx,
) -> Session {
    // A hypertrophy goal runs its own week (`plan::hypertrophy`).
    if super::hypertrophy::uses_double_progression(profile) {
        return super::hypertrophy::hypertrophy_session(profile, anchors, ta, day, block);
    }
    let goal = profile.lift_goal;
    let power_base = power_needs_strength_base(profile, anchors, ta);
    let emphasis = dup_emphasis(goal, power_base, day_index);
    let power_day = emphasis == DupDay::Power;
    let mut prescriptions = Vec::new();
    // A power day leads with a ballistic jump, done fresh
    // (STR-POWER-SELECT-001): a bodyweight jump is the exercise whose optimal
    // power load is ~0 % 1RM, the end of the load spectrum barbell lifts can't
    // reach.
    if power_day {
        prescriptions.push(ballistic_item(profile, anchors, ta));
    }
    // DUP undulates the SAME lifts across the week (File 02 strength-023).
    // Accessories COVER the other core movement patterns on the same full-body
    // day (STR-SESSION-BALANCE-001, Moderate): `lift_names` was built
    // pattern-balanced by `main_lifts`, so scheduling ALL of it keeps each
    // session balanced; each lift carries its own honest set-face via
    // `lift_prescription`. On a power day a logged Olympic derivative moves to
    // the front, right after the jumps (strength-031: placed early).
    let mut order: Vec<&String> = lift_names.iter().collect();
    if power_day {
        order.sort_by_key(|n| !is_olympic_derivative(n));
    }
    for name in order {
        prescriptions.push(lift_prescription(
            goal,
            ta,
            emphasis,
            name,
            anchors,
            profile.weekly_sets,
            block,
        ));
    }
    // The title names what the day actually prescribes: the DUP
    // label only when the lead lift really undulates by %1RM; a novice linear
    // day or an effort-prescribed day is a uniform "Strength day", and a
    // power day (jumps + fast or calibration sets) stays a "Power day".
    let undulates = ta != TrainingAge::Novice
        && lift_names
            .first()
            .is_some_and(|n| anchors.e1rm_for(n).is_some());
    let lift_type = if power_day {
        LiftSessionType::DynamicEffort
    } else if undulates {
        match emphasis {
            DupDay::Heavy | DupDay::HeavyVolume => LiftSessionType::MaxEffort,
            DupDay::Power => LiftSessionType::DynamicEffort,
            DupDay::Hypertrophy => LiftSessionType::Repetition,
        }
    } else {
        LiftSessionType::FullBody
    };
    Session {
        session_type: SessionType::Lift(lift_type),
        day,
        prescriptions,
        dup_day: undulates.then_some(emphasis),
    }
}

/// What [`sized_sets`] decided: the per-session working sets, plus whether the
/// accumulation ramp rather than the stated weekly target produced that count
/// (the evidence face follows the claim that set the number).
pub(super) struct SizedSets {
    pub(super) per_session: u8,
    ramped: bool,
}

/// Per-session working sets from the stated weekly per-muscle target
/// (`weekly_sets`). The target is bounded to the primary muscle's Table 1
/// MEV..MRV (HYP-VOL-DOSE-001, HYP-LANDMARKS-001; without a Table 1 row the
/// training-age MEV, HYP-MEV-AGE-001, and the generic ~20-set MRV,
/// HYP-MRV-DELOAD-001), ramped across the block toward the muscle's MAV high
/// (HYP-VOL-CAP-001, HYP-SETRAMP-001; week 1 is the stated target), split evenly
/// over the `n_lift_days` that train it (EXPERT-OPINION, flagged on
/// HYP-VOL-DOSE-001), then clamped to the per-exercise set limits
/// (HYP-SESSCAP-001) and the day's own `band`. `ramped` tells the caller which
/// claim set the number.
pub(super) fn sized_sets(
    weekly_sets: u8,
    ta: TrainingAge,
    n_lift_days: usize,
    band: (u8, u8),
    landmarks: Option<&hypertrophy::VolumeLandmarks>,
    week_in_block: u8,
    block_weeks: u8,
) -> SizedSets {
    // Bound the weekly target to the muscle's MEV..MRV.
    let mev_floor = landmarks
        .map(|l| l.mev)
        .unwrap_or_else(|| hypertrophy::mev_sets_by_training_age(ta).value.0);
    let mrv_ceiling = landmarks.map_or(hypertrophy::GENERIC_MRV_SETS, |l| l.mrv);
    let start = weekly_sets.clamp(mev_floor, mrv_ceiling);
    // This week's point on the ramp; `weekly_set_ramp` opens at `start`, and a
    // stated target above the ramp top is held.
    let ramp_top = landmarks.map_or(mrv_ceiling, |l| l.mav.1);
    let ramp = hypertrophy::weekly_set_ramp(start, ramp_top, block_weeks).value;
    let bounded_weekly = ramp
        .get((week_in_block.max(1) - 1) as usize)
        .copied()
        .unwrap_or(start);
    // Even split across the lift days that train the primary muscle.
    let n = n_lift_days.max(1) as f64;
    let split = |weekly: u8| -> u8 {
        // Per-session saturation cap + minimum, then into the day's own band.
        ((weekly as f64 / n).round() as u8)
            .clamp(
                hypertrophy::MIN_SETS_PER_EXERCISE,
                hypertrophy::PER_SESSION_SET_CEILING,
            )
            .clamp(band.0, band.1)
    };
    let per_session = split(bounded_weekly);
    SizedSets {
        per_session,
        // Only claim the ramp when it actually moved the prescribed count: a
        // week whose extra weekly volume vanishes in the split or the band clamp
        // is still the stated-target number.
        ramped: per_session != split(start),
    }
}

/// One lift's evidence-cited prescription. DUP (%1RM) when anchored + non-novice;
/// linear `loading_rx` (%1RM anchored, RIR unanchored) otherwise. `week_in_block`
/// is 1-based. Progression across the block:
/// - Strength/power goals ramp the %1RM inside each day's cited band
///   (`strength::pct_for_week`, STR-PCTPROG-001: +2.5 %/wk lower body, +1 %/wk
///   upper body) and HOLD the week-1 set count — intensity, not hypertrophy
///   volume, is the strength lever (STR-INTENT-001), so the MRV set ramp
///   (HYP-SETRAMP-001) stays hypertrophy-only.
/// - A hypertrophy-emphasis lift keeps the volume ramp / RIR walk-down.
///
/// A Power goal's non-power days are strength-base days prescribed from the
/// max-strength band (STR-BASE-POWER-001). On a power day an unanchored lift is
/// a calibration set (one set of 5 at RPE 8, STR-E1RM-CHECK-001), never "50 % of
/// an unknown e1RM", and a logged Olympic derivative is loaded at its own
/// optimum (≥70 %, STR-POWER-SELECT-001).
pub(super) fn lift_prescription(
    goal: LiftGoal,
    ta: TrainingAge,
    emphasis: DupDay,
    exercise: &str,
    anchors: &Anchors,
    weekly_sets: u8,
    block: BlockCtx,
) -> Recommended<Prescription> {
    // A >8 wk layoff's pre-layoff anchors are already set aside by
    // `build_plan_anchors` (KB Table 3.4b: treat as novice), so an anchor here
    // is always one the athlete may load from.
    let anchored = anchors.e1rm_for(exercise).is_some();
    let dup = anchored && ta != TrainingAge::Novice;
    let power_day = emphasis == DupDay::Power;
    // A Power goal trains its strength base on every non-power day, so a
    // Power `day_goal` is exactly a Power goal's power day.
    let day_goal = if goal == LiftGoal::Power && !power_day {
        LiftGoal::MaxStrength
    } else {
        goal
    };
    let load = strength::loading_rx(day_goal);
    let ctx = DoseCtx {
        exercise,
        anchors,
        ta,
        goal,
        day_goal,
        emphasis,
        weekly_sets,
        block,
        load: load.value,
        dup,
        // Isolation lifts have unreliable e1RMs (File 02 strength-006; the SAME
        // `is_isolation_lift` predicate that gates the anchor and the DUP
        // primary), so on a DUP day they are prescribed in their own hypertrophy
        // rep window by RIR rather than undulated at %1RM off an untrustworthy
        // anchor.
        is_isolation: dup && is_isolation_lift(exercise),
        olympic: power_day && anchored && is_olympic_derivative(exercise),
        upper: is_upper_body(exercise),
        strength_goal: matches!(goal, LiftGoal::MaxStrength | LiftGoal::Power),
    };
    let power_goal_day = day_goal == LiftGoal::Power;
    let dose = if ctx.olympic {
        olympic_dose()
    } else if dup {
        ctx.dup_dose()
    } else if anchored && power_goal_day {
        ctx.novice_power_dose()
    } else if anchored && ctx.load.pct_1rm.0 > 0 {
        ctx.novice_linear_dose()
    } else if power_goal_day {
        calibration_dose()
    } else if let Some(rir) = ctx.load.rir {
        ctx.unanchored_rir_dose(rir)
    } else {
        // A band with neither a %1RM floor nor a RIR (only the Power band,
        // whose power days are handled above): the calibration set again.
        calibration_dose()
    };

    // Power/speed sets carry the velocity intent on the prescription itself
    // (File 02 strength-023 "50–70% FAST"; AUTOREG-VL-001). The flattener turns
    // it into the device-free "move fast; end the set when a rep visibly slows"
    // cue; the ~10 % figure is kept for velocity-tracking shells.
    let velocity_loss_pct = dose
        .velocity_intent
        .then(|| (strength::vl_termination_threshold(LiftGoal::Power).value * 100.0).round() as u8);
    let pres = Prescription::Lift(LiftPrescription {
        exercise: exercise.to_string(),
        sets: dose.sets,
        reps: dose.reps,
        intensity: dose.intensity,
        rest_sec: ctx.rest_sec(),
        velocity_loss_pct,
        reps_max: None,
        load_kg: None,
    });

    // The badge never reads stronger than the claim that produced the figure.
    if let Some(id) = dose.claim {
        crate::evidence::graded(pres, id)
    } else if dup && dose.set_ramped {
        crate::evidence::graded(pres, claims::HYP_SETRAMP_001)
    } else if dup {
        crate::evidence::graded(pres, claims::HYP_VOL_DOSE_001)
    } else if goal == LiftGoal::Power {
        crate::evidence::graded(pres, claims::STR_PWR_001)
    } else if dose.rir_ramped {
        crate::evidence::graded(pres, claims::HYP_RIR_RAMP_001)
    } else {
        load.with_value(pres)
    }
}

/// What one [`lift_prescription`] arm decided: the set face, plus the
/// provenance the evidence face cites (the path that produced the number).
struct LiftDose {
    intensity: LiftIntensity,
    sets: u8,
    reps: u8,
    /// Power/speed sets: the prescription carries the velocity-loss cue.
    velocity_intent: bool,
    /// The claim that moved this week's number, when a specific rule (not the
    /// loading band) did.
    claim: Option<ClaimId>,
    /// The accumulation ramp, not the stated weekly target, set the DUP count.
    set_ramped: bool,
    /// The hypertrophy RIR walk-down moved the RIR off the band's high end.
    rir_ramped: bool,
}

impl LiftDose {
    fn new(intensity: LiftIntensity, sets: u8, reps: u8) -> Self {
        Self {
            intensity,
            sets,
            reps,
            velocity_intent: false,
            claim: None,
            set_ramped: false,
            rir_ramped: false,
        }
    }
}

/// The per-lift inputs every [`lift_prescription`] arm reads.
struct DoseCtx<'a> {
    exercise: &'a str,
    anchors: &'a Anchors,
    ta: TrainingAge,
    goal: LiftGoal,
    /// The band the day trains: a Power goal's non-power days are max-strength
    /// days (STR-BASE-POWER-001).
    day_goal: LiftGoal,
    emphasis: DupDay,
    weekly_sets: u8,
    block: BlockCtx,
    /// `strength::loading_rx(day_goal)`.
    load: LoadingRx,
    /// Anchored and non-novice: the lift undulates by %1RM.
    dup: bool,
    /// An isolation lift on a DUP day.
    is_isolation: bool,
    /// A logged Olympic derivative on a power day.
    olympic: bool,
    upper: bool,
    strength_goal: bool,
}

impl DoseCtx<'_> {
    /// Rest comes from the band the day ACTUALLY prescribes: a Heavy/Power DUP
    /// day rests per its emphasis (180 s), never per the goal band minimum. A
    /// hypertrophy-emphasis set rests per the EXERCISE CLASS (HYP-REST-002:
    /// 120 s heavy compounds, 90 s otherwise, catalog-unknown treated as a
    /// moderate compound), not the 30 s floor of the hypertrophy loading band —
    /// a heavy compound cannot hold ≥90% of its first-set reps on 30 s.
    fn rest_sec(&self) -> u16 {
        let hypertrophy_emphasis = if self.dup {
            self.emphasis == DupDay::Hypertrophy
        } else {
            self.day_goal == LiftGoal::Hypertrophy
        };
        if self.is_isolation {
            hypertrophy::rest_sec_for(hypertrophy::ExerciseClass::Isolation)
                .value
                .0
        } else if self.olympic {
            strength::olympic_derivative_rx().value.rest_sec.0
        } else if hypertrophy_emphasis {
            hypertrophy::rest_sec_for(
                hypertrophy::exercise_entry(self.exercise)
                    .map(|e| e.class)
                    .unwrap_or(hypertrophy::ExerciseClass::ModerateCompound),
            )
            .value
            .0
        } else if self.dup {
            strength::loading_rx(emphasis_goal(self.emphasis))
                .value
                .rest_sec
                .0
        } else {
            self.load.rest_sec.0
        }
    }

    /// Daily-undulating day: %1RM/reps from the day's cited band; the
    /// per-session `sets` are VOLUME-SIZED from the stated weekly per-muscle
    /// target (`sized_sets`), clamped into the band's own sets range.
    fn dup_dose(&self) -> LiftDose {
        let d = strength::dup_day_rx(self.emphasis).value;
        let BlockCtx {
            n_lift_days,
            week_in_block,
            block_weeks,
            ..
        } = self.block;
        // Bound the volume sizing by THIS lift's primary-muscle landmarks (MEV
        // floor / MRV ceiling); an exercise outside Table 4 falls back to the
        // training-age band inside `sized_sets`.
        let landmarks = hypertrophy::landmarks_for_exercise(self.exercise);
        // Strength/power goals hold the week-1 set count across the block.
        let sizing_week = if self.strength_goal { 1 } else { week_in_block };
        let sized = sized_sets(
            self.weekly_sets,
            self.ta,
            n_lift_days,
            d.sets,
            landmarks,
            sizing_week,
            block_weeks,
        );
        let mut dose = if self.is_isolation {
            let rir = hypertrophy::rir_for_week(week_in_block, block_weeks)
                .map(|r| r.value)
                .unwrap_or(hypertrophy::MAX_SCHEDULED_RIR);
            LiftDose::new(
                LiftIntensity::Rir(rir),
                sized.per_session,
                // The isolation class range top (12–20, HYP-REPLOAD-001), not
                // the 25-rep edge where RIR estimates are least accurate.
                hypertrophy::rep_range_for(hypertrophy::ExerciseClass::Isolation)
                    .value
                    .1,
            )
        } else {
            let mut claim = None;
            let pct = if self.strength_goal {
                let week = earned_week(self.anchors, self.exercise, week_in_block);
                let p = strength::pct_for_week(d.pct_1rm, week, self.upper, d.reps.0).value;
                if p > d.pct_1rm.0 {
                    claim = Some(claims::STR_PCTPROG_001);
                }
                p
            } else {
                d.pct_1rm.0
            };
            LiftDose {
                velocity_intent: d.max_velocity_intent,
                claim,
                ..LiftDose::new(
                    LiftIntensity::PercentOneRm(pct as f32),
                    sized.per_session,
                    d.reps.0,
                )
            }
        };
        dose.set_ramped = sized.ramped;
        dose
    }

    /// Novice power day, anchored: the power band's low end (50 %, ramping),
    /// fast, never to failure (STR-PWR-001 band, strength-023 power day).
    fn novice_power_dose(&self) -> LiftDose {
        let d = strength::dup_day_rx(DupDay::Power).value;
        let week = earned_week(self.anchors, self.exercise, self.block.week_in_block);
        let p = strength::pct_for_week(d.pct_1rm, week, self.upper, d.reps.0).value;
        LiftDose {
            velocity_intent: true,
            claim: Some(if p > d.pct_1rm.0 {
                claims::STR_PCTPROG_001
            } else {
                claims::STR_PWR_001
            }),
            ..LiftDose::new(
                LiftIntensity::PercentOneRm(p as f32),
                self.load.sets.0,
                d.reps.0,
            )
        }
    }

    /// Novice linear day, anchored → %1RM from the conservative LOW end of the
    /// band. Reps are capped at E1RM_RELIABLE_REP_CAP (10) so a plan-following
    /// set stays inside the reliable e1RM window (File 02 strength-006) and can
    /// itself refresh the anchor. A strength day ramps the load week over week
    /// (STR-PCTPROG-001: novices progress linearly, strength-010/014),
    /// 80 → 82 → 83 % for a lower-body ×5, held where 5 reps would leave under
    /// 1 in reserve, and never below the load of a completed last session
    /// ([`novice_hold_pct`]).
    fn novice_linear_dose(&self) -> LiftDose {
        let load = &self.load;
        let reps = load.reps.1.min(strength::E1RM_RELIABLE_REP_CAP as u8);
        let mut claim = None;
        let pct = if self.day_goal == LiftGoal::MaxStrength {
            let week = earned_week(self.anchors, self.exercise, self.block.week_in_block);
            let p = strength::pct_for_week(load.pct_1rm, week, self.upper, reps).value;
            if p > load.pct_1rm.0 {
                claim = Some(claims::STR_PCTPROG_001);
            } else if self.goal == LiftGoal::Power {
                claim = Some(claims::STR_BASE_POWER_001);
            }
            p
        } else {
            load.pct_1rm.0
        };
        // A completed session is never followed by a lighter one: novice
        // linear progression adds load after a session whose prescribed reps
        // were all done and deloads a lift only after repeated misses (File 08
        // indiv-008/009; STR-PCTPROG-001 "per successful week"). A set logged
        // at its target effort cannot raise the estimate the percentage reads,
        // and each %1RM load rounds DOWN onto the plate grid, so without this
        // hold the load would slide one plate at a time across completed weeks.
        let pct = match novice_hold_pct(self.anchors, self.exercise, reps) {
            Some(hold) if hold > pct => {
                claim = Some(claims::STR_PCTPROG_001);
                hold
            }
            _ => pct,
        };
        // A novice's load never jumps past the novice step cap over their last
        // session of the lift (File 02 strength-040, STR-1RMTEST-001: lower
        // body ≤10 %, upper body ≤5 %), however far a fresh estimate moved.
        let pct = match novice_jump_cap_pct(self.anchors, self.exercise, reps, self.upper) {
            Some(cap) if cap < pct => {
                claim = Some(claims::STR_1RMTEST_001);
                cap
            }
            _ => pct,
        };
        LiftDose {
            claim,
            ..LiftDose::new(LiftIntensity::PercentOneRm(pct as f32), load.sets.0, reps)
        }
    }

    /// No anchor → RIR from the band. A hypertrophy goal walks it down across
    /// the accumulation block (HYP-RIR-RAMP-001: 4 → 1 by the final week, held
    /// inside the band); any other goal keeps the conservative HIGH end (more
    /// reps in reserve = the safer end).
    fn unanchored_rir_dose(&self, (rir_lo, rir_hi): (u8, u8)) -> LiftDose {
        let rir = if self.day_goal == LiftGoal::Hypertrophy {
            hypertrophy::rir_for_week(self.block.week_in_block, self.block.block_weeks)
                .map(|r| r.value)
                .unwrap_or(rir_hi)
                .clamp(rir_lo, rir_hi)
        } else {
            rir_hi
        };
        LiftDose {
            rir_ramped: rir != rir_hi,
            claim: (self.goal == LiftGoal::Power).then_some(claims::STR_BASE_POWER_001),
            ..LiftDose::new(LiftIntensity::Rir(rir), self.load.sets.0, self.load.reps.1)
        }
    }
}

/// A logged Olympic derivative on a power day sits at its own optimum (power
/// clean ≥70 % 1RM, Soriano 2015 → STR-POWER-SELECT-001), 3 sets of 3 fast reps
/// (strength-031: 3–5 × 1–3), early in the session.
fn olympic_dose() -> LiftDose {
    let o = strength::olympic_derivative_rx().value;
    LiftDose {
        velocity_intent: true,
        claim: Some(claims::STR_POWER_SELECT_001),
        ..LiftDose::new(
            LiftIntensity::PercentOneRm(OLYMPIC_POWER_PCT as f32),
            o.sets.0,
            o.reps.1,
        )
    }
}

/// An unanchored lift on a power day has no e1RM to take a percentage of, so
/// it is a calibration set the athlete can execute — work up to one set of 5
/// at RPE 8 (a 3–6 rep test set, STR-E1RM-CHECK-001). Logging it anchors every
/// later load. Never "50 % of an unknown e1RM".
fn calibration_dose() -> LiftDose {
    let (lo, hi) = strength::e1rm_test_set_reps().value;
    LiftDose {
        claim: Some(claims::STR_E1RM_CHECK_001),
        ..LiftDose::new(
            LiftIntensity::Rpe(CALIBRATION_RPE),
            1,
            CALIBRATION_REPS.clamp(lo, hi),
        )
    }
}

/// The block week whose %1RM step this lift has earned. strength-014
/// (STR-PCTPROG-001) adds the step "per SUCCESSFUL week": when the lift's last
/// session had a set rated past RPE 9 (under one rep in reserve, below the
/// 1–3 RIR strength band, STR-INTENT-001), this week's step is withheld and the
/// previous week's percentage repeats. Unrated sets cannot show a miss.
fn earned_week(anchors: &Anchors, exercise: &str, week_in_block: u8) -> u8 {
    let missed = anchors
        .last_working_for(exercise)
        .is_some_and(|w| w.sets.iter().any(|&(_, rpe)| rpe > 9.0));
    if missed {
        week_in_block.saturating_sub(1).max(1)
    } else {
        week_in_block
    }
}

/// Highest whole %1RM of a novice's anchor that keeps this session within the
/// novice step cap over the lift's last session (File 02 strength-040,
/// STR-1RMTEST-001: upper body 2.5–5 %, lower body 5–10 %; the top of the
/// band). The last top set is rep-matched to today's reps on the Epley model
/// (strength-005) first, so a heavier triple after a set of 8 is not mistaken
/// for a jump. `None` without an anchor or a loaded last session.
fn novice_jump_cap_pct(anchors: &Anchors, exercise: &str, reps: u8, upper: bool) -> Option<u8> {
    let e1 = anchors.e1rm_for(exercise)?;
    let last = anchors
        .last_working_for(exercise)
        .filter(|w| w.weight_kg > 0.0)?;
    let last_reps = last.sets.iter().map(|&(r, _)| r).max()?;
    let base = last.weight_kg * strength::e1rm_epley(1.0, last_reps.max(1))
        / strength::e1rm_epley(1.0, u32::from(reps.max(1)));
    let jump = strength::novice_load_jump_cap_frac(upper).value.1;
    Some((100.0 * base * (1.0 + jump) / e1).floor().clamp(0.0, 100.0) as u8)
}

/// Lowest whole %1RM of a novice's anchor that repeats the lift's last working
/// load, when that session was completed: every working set reached today's
/// `reps` and none was rated past RPE 9 (the miss [`earned_week`] withholds a
/// step for; an unrated set cannot show one). `None` without an anchor, a
/// loaded last session, or when it was not completed. The percentage is
/// rounded UP so the plate-grid floor of anchor × % lands on the load lifted.
fn novice_hold_pct(anchors: &Anchors, exercise: &str, reps: u8) -> Option<u8> {
    let e1 = anchors.e1rm_for(exercise)?;
    let last = anchors
        .last_working_for(exercise)
        .filter(|w| w.weight_kg > 0.0)?;
    let completed = !last.sets.is_empty()
        && last
            .sets
            .iter()
            .all(|&(r, rpe)| r >= u32::from(reps) && rpe <= 9.0);
    completed.then(|| {
        (100.0 * last.weight_kg / e1 - 1e-6)
            .ceil()
            .clamp(0.0, 100.0) as u8
    })
}

/// Calibration set for an unanchored lift on a power day: one set of 5 at
/// RPE 8 (inside the 3–6 rep test-set window, STR-E1RM-CHECK-001; RPE 8 = 2
/// reps in reserve, inside the 1–3 RIR strength band, STR-INTENT-001).
const CALIBRATION_REPS: u8 = 5;
const CALIBRATION_RPE: f32 = 8.0;

/// Power-clean-type optimum on a power day: ≥70 % 1RM (Soriano 2015,
/// STR-POWER-SELECT-001), the low end of that optimum.
const OLYMPIC_POWER_PCT: u8 = 70;

/// Whether a lift is upper-body for the weekly %1RM increment
/// (STR-PCTPROG-001: +1 % upper vs +2.5 % lower). Push/Pull patterns are upper;
/// Squat/Hinge lower; an unclassifiable lift takes the smaller, upper-body step
/// (the conservative tie-break).
fn is_upper_body(name: &str) -> bool {
    !matches!(
        movement_pattern(name),
        Some(MovementPattern::Squat | MovementPattern::Hinge)
    )
}

/// Whether a lift is an Olympic weightlifting derivative (clean / snatch /
/// jerk variants, high pulls, jump shrugs). Only ever programmed when the
/// athlete already logs one — the skill prerequisite.
fn is_olympic_derivative(name: &str) -> bool {
    with_name_tokens(name, |t| {
        ["clean", "snatch", "jerk", "high pull", "jump shrug"]
            .iter()
            .any(|kw| has_phrase(t, kw))
    })
}

/// Whether a lift is loaded on a standard straight barbell, so a plate
/// breakdown applies. Conservative: dumbbell / kettlebell / cable /
/// machine / Smith / trap-bar / single-leg variants are excluded; a name saying
/// "barbell" counts; otherwise the classic barbell lifts (squat, deadlift, RDL,
/// bench, overhead/military press, Pendlay row, Olympic lifts, good morning).
pub(crate) fn is_barbell_lift(name: &str) -> bool {
    with_name_tokens(name, barbell_tokens)
}

fn barbell_tokens(tokens: &[&str]) -> bool {
    let has = |t: &str| has_phrase(tokens, t);
    const NOT_BARBELL: &[&str] = &[
        "dumbbell",
        "db",
        "kettlebell",
        "kb",
        "cable",
        "machine",
        "band",
        "smith",
        "goblet",
        "bulgarian",
        "split",
        "pistol",
        "hack",
        "sissy",
        "jump",
        "landmine",
        "trap",
        "hex",
        "leg",
        "single",
    ];
    if NOT_BARBELL.iter().any(|t| has(t)) {
        return false;
    }
    has("barbell")
        || has("squat")
        || has("deadlift")
        || has("rdl")
        || has("bench")
        || (has("press") && (has("overhead") || has("military") || has("strict")))
        || has("pendlay")
        || has("clean")
        || has("snatch")
        || has("jerk")
        || has("good morning")
}

/// The ballistic opener of a power day (STR-POWER-SELECT-001): a
/// bodyweight countermovement jump — or, once a non-novice has a squat e1RM of
/// ≥1.5× bodyweight (PLYO-PREREQ-001, safety gate), a low-box depth jump.
/// Volume is the KB's low-dose plyometric band floor, 40 foot contacts as 8 × 5
/// (strength-032 / PLYO-001: low-dose 40–60 also effective; well inside every
/// level's cap), with 2 min between sets (STR-PLYO-SCHED-001: 2–3 min). One
/// power day per 3 lift days in the `dup_emphasis` order (one per 2 in the
/// running-led `k % 2` layout) keeps jumps ≤3×/wk.
fn ballistic_item(
    profile: &Profile,
    anchors: &Anchors,
    ta: TrainingAge,
) -> Recommended<Prescription> {
    const REPS: u8 = 5;
    const REST_SEC: u16 = 120;
    let contacts = strength::plyo_low_dose_contacts().value.0;
    let sets = (contacts / REPS as u16).clamp(1, u8::MAX as u16) as u8;
    let depth = ta != TrainingAge::Novice && squat_benchmark_met(profile, anchors);
    let exercise = if depth {
        "Depth jump (low box)"
    } else {
        "Vertical jump"
    };
    crate::evidence::graded(
        Prescription::Lift(LiftPrescription {
            exercise: exercise.to_string(),
            sets,
            reps: REPS,
            intensity: LiftIntensity::MaxIntent,
            rest_sec: REST_SEC,
            velocity_loss_pct: None,
            reps_max: None,
            load_kg: None,
        }),
        claims::STR_POWER_SELECT_001,
    )
}

/// Shape a barbell MaxStrength/Power week toward a strength test / meet.
/// `days_to_meet_from_week_start` = meet day − this week's Monday; each
/// session's own distance is that minus its `day` offset. Lift sessions only;
/// runs and rest days are untouched. For a lift session `d` days out:
/// - `d ≤ 2` (meet day and the 2 days before): no lifting — the KB peak ends
///   with 2–7 days of reduced work/cessation (STR-PEAK-001).
/// - `3 ≤ d ≤ 14`: the taper — volume cut ~41 % (8–14 days out) then ~60 %
///   (3–7 days out), intensity and frequency held (TAPER-001, strength-026);
///   heavy deadlifts stop inside 10 days (STR-DLPEAK-001: last heavy pull 10–14
///   days out) and heavy squat/bench inside 4 days (strength-029: ~4–5 days).
///
/// A session left with nothing becomes a rest day. Beyond 14 days, or after the
/// meet, the week is unchanged.
pub(super) fn apply_meet_taper(sessions: &mut [Session], days_to_meet_from_week_start: i64) {
    let taper = strength::taper_rx().value;
    let dl_cutoff = strength::deadlift_peak_days_out().value.0 as i64;
    const SQUAT_BENCH_CUTOFF: i64 = 4;
    const CESSATION_DAYS: i64 = 2;
    // The taper's final week (3–7 days out) takes the deeper cut.
    const TAPER_DEEP_CUT_DAYS: i64 = 7;
    let taper_days = taper.duration_days.1 as i64;
    let heavy = |i: &LiftIntensity| match *i {
        LiftIntensity::PercentOneRm(p) => p >= HEAVY_PCT_1RM,
        LiftIntensity::Rir(r) => r <= 3,
        LiftIntensity::Rpe(r) => r >= 7.0,
        LiftIntensity::MaxIntent => false,
    };
    for s in sessions.iter_mut() {
        if !matches!(s.session_type, SessionType::Lift(_)) {
            continue;
        }
        let d = days_to_meet_from_week_start - s.day as i64;
        if !(0..=taper_days).contains(&d) {
            continue;
        }
        if d <= CESSATION_DAYS {
            s.session_type = SessionType::Rest;
            s.prescriptions.clear();
            continue;
        }
        let cut = if d <= TAPER_DEEP_CUT_DAYS {
            taper.volume_reduction_frac.1
        } else {
            taper.volume_reduction_frac.0
        };
        s.prescriptions.retain(|rx| {
            let Prescription::Lift(l) = &rx.value else {
                return true;
            };
            let (deadlift, bench) = with_name_tokens(&l.exercise, |t| {
                let prefixed = |p: &str| t.iter().any(|w| w.starts_with(p));
                (
                    has_phrase(t, "deadlift") && !prefixed("romanian") && !prefixed("stiff"),
                    prefixed("bench"),
                )
            });
            let squat_or_bench =
                movement_pattern(&l.exercise) == Some(MovementPattern::Squat) || bench;
            let heavy = heavy(&l.intensity);
            let drop_dl = deadlift && d < dl_cutoff && heavy;
            let drop_sb = squat_or_bench && d <= SQUAT_BENCH_CUTOFF && heavy;
            !(drop_dl || drop_sb)
        });
        for rx in s.prescriptions.iter_mut() {
            if let Prescription::Lift(l) = &rx.value {
                let mut l = l.clone();
                l.sets = ((l.sets as f64 * (1.0 - cut)).round() as u8).max(1);
                *rx = crate::evidence::graded(Prescription::Lift(l), claims::TAPER_001);
            }
        }
        if s.prescriptions.is_empty() {
            s.session_type = SessionType::Rest;
        }
    }
}
