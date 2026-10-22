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

/// The core movement patterns a lift day should COVER (STR-SESSION-BALANCE-001):
/// squat / hinge / horizontal push / horizontal pull. Vertical presses and
/// vertical pulls are folded into `Push` / `Pull` for COVERAGE purposes (an
/// overhead press still trains the push musculature), while the catalog FILLS in
/// [`CORE_PATTERN_FILLS`] use the KB's named horizontal defaults.
///
/// This pattern TAXONOMY is an EXPERT-OPINION practitioner framework, no RCT
/// validates a specific movement-pattern list (flagged in the claim's
/// `contradicting`, GRADE good-practice-statement precedent). The CITABLE part is
/// the PRINCIPLE that a session must cover distinct patterns because multi-joint
/// compounds do NOT fully activate every muscle (Schoenfeld et al. 2019, Sports
/// 7(7):177, PMID 31336594 → STR-SESSION-BALANCE-001, Moderate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MovementPattern {
    Squat,
    Hinge,
    Push,
    Pull,
}

/// Classify an exercise NAME into a core movement pattern, CONSERVATIVELY: only
/// names that map confidently return `Some`; anything ambiguous (most isolation
/// work, curls, raises, calf work) returns `None` and is treated as
/// unclassifiable (a logged unclassifiable lift is still scheduled, never
/// dropped). Case-insensitive whole-word matching over a small keyword set (a
/// plural token matches its singular needle); this keyword taxonomy is
/// EXPERT-OPINION (see [`MovementPattern`]); the principle it serves is cited
/// (STR-SESSION-BALANCE-001).
pub(crate) fn movement_pattern(name: &str) -> Option<MovementPattern> {
    with_name_tokens(name, |tokens| {
        let has = |kw: &str| has_phrase(tokens, kw);
        movement_pattern_of(&has)
    })
}

/// Lowercase ASCII word tokens of an exercise name (split on every
/// non-alphanumeric ASCII char, empties dropped), handed to `f`.
fn with_name_tokens<R>(name: &str, f: impl FnOnce(&[&str]) -> R) -> R {
    let lower = name.to_ascii_lowercase();
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

/// Exercises per lift day (STR-SESSION-BALANCE-001): the DUP-undulated primary
/// compound + accessories covering the other core patterns. ~3-4 is enough
/// compounds to span squat/hinge/push/pull in a full-body day; the EXACT count is
/// EXPERT-OPINION (no RCT prescribes a specific N, flagged in the claim's
/// `contradicting`).
const MAX_LIFTS_PER_DAY: usize = 4;

/// Main lift slots, PATTERN-BALANCED (STR-SESSION-BALANCE-001, Moderate): the day
/// spans distinct movement patterns rather than stacking one, so a leg-heavy log
/// (or the squat-led catalog default) still yields a balanced week. Leads with
/// the user's own logged exercises (each carrying an e1RM anchor → a load),
/// then FILLS the missing core patterns from the File-03
/// catalog (→ RIR). The coverage PRINCIPLE is cited (compounds under-activate
/// some muscles, Schoenfeld 2019); the pattern TAXONOMY + the ~3-4 count are
/// EXPERT-OPINION, flagged on the claim. The hinge fill is goal-aware: a
/// strength or power goal trains the competition deadlift (STR-COMP-ANCHOR-001).
pub(super) fn main_lifts(anchors: &Anchors, goal: LiftGoal) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut covered: Vec<MovementPattern> = Vec::new();
    // 1. Lead with the user's OWN logged lifts (anchored
    //    load), but keep PATTERN BALANCE: schedule a logged lift when it opens a
    //    NEW core pattern (or is unclassifiable, never dropped), and SKIP a
    //    second logged lift that only repeats an already-covered pattern (that
    //    repeat is what unbalances a leg-heavy log). `same_movement`
    //    dedup still applies (never the same lift twice on one day).
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
    // The logged lifts were picked most-recent-first (the newest of each
    // pattern wins), but the session lists them in the plan's order, squat →
    // hinge → push → pull, then anything unclassified, so the main compound
    // leads instead of whichever lift happened to be logged last (tomorrow's
    // card never runs in reverse of today's). The fills below already follow
    // that order. Stable: ties keep their recency order.
    out.sort_by_key(|n| match movement_pattern(n) {
        Some(MovementPattern::Squat) => 0u8,
        Some(MovementPattern::Hinge) => 1,
        Some(MovementPattern::Push) => 2,
        Some(MovementPattern::Pull) => 3,
        None => 4,
    });
    // 2. FILL the missing core patterns from the File-03 catalog. Use
    //    `same_movement` (not exact-case) so a logged "Smith machine back squat"
    //    isn't scheduled AGAIN as the catalog "Back squat", the same lift twice on one day.
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
    // 3. The DUP primary is `out[0]`, undulated at %1RM on EVERY lift day, so it
    //    MUST be a COMPOUND with a RELIABLE e1RM: isolation lifts are
    //    e1RM-unreliable (File 02 strength-006; STR-E1RM-CHECK-001, catalog OR
    //    keyword-named), and an unclassifiable lift carries no core movement
    //    pattern, so neither is a lift the week should be built around
    //    (STR-SESSION-BALANCE-001). Promote the first lift that is BOTH
    //    e1RM-reliable AND pattern-classified to lead; the catalog fills above
    //    are all compounds, so one normally qualifies. A log of nothing but
    //    isolation/unclassifiable work (with the core patterns already covered)
    //    leaves none → keep the most-recent lift as the sensible fallback.
    let leads = |n: &String| e1rm_reliable_name(n) && movement_pattern(n).is_some();
    if out.first().is_some_and(|n| !leads(n))
        && let Some(idx) = out.iter().position(leads)
    {
        out[..=idx].rotate_right(1);
    }
    out
}

/// Whether a lift's estimated 1RM is reliable enough to anchor the DUP primary's
/// %1RM loads: isolation lifts are excluded because their e1RM is unreliable
/// (File 02 strength-006; STR-E1RM-CHECK-001, Moderate). Delegates to
/// [`is_isolation_lift`], which covers BOTH the File-03 catalog isolation rows
/// and the many real-world isolation names outside the 16-row catalog.
pub(crate) fn e1rm_reliable_name(name: &str) -> bool {
    !is_isolation_lift(name)
}

/// Whether a lift is an ISOLATION movement whose estimated 1RM is unreliable
/// (File 02 strength-006; STR-E1RM-CHECK-001). Two sources: the File-03 Table-4
/// catalog class (authoritative for the 16 known rows), then a keyword taxonomy
/// for the many isolation names outside the catalog. UNAMBIGUOUS needles
/// (`ISOLATION_NEEDLES_STRONG`) name a single-joint movement outright and
/// classify regardless of any compound pattern the name also matches ("Chest
/// fly" / "Pec deck" read as Push in `movement_pattern`, yet are isolation).
/// AMBIGUOUS needles (`ISOLATION_NEEDLES_WEAK`) fire only when the name carries
/// NO compound pattern, so a posterior-chain "back extension" (a hinge) stays a
/// compound while "leg extension" reads as isolation. Single source shared by
/// every isolation gate — `e1rm_reliable_name`, the DUP `is_isolation` gate,
/// `build_plan_anchors`' anchor skip, the e1RM cross-check, and the Prilepin
/// audit — so no two sites disagree. The taxonomy is EXPERT-OPINION (mirroring
/// `movement_pattern`), serving the cited reliability principle.
pub(crate) fn is_isolation_lift(name: &str) -> bool {
    if hypertrophy::exercise_entry(name)
        .is_some_and(|e| e.class == hypertrophy::ExerciseClass::Isolation)
    {
        return true;
    }
    isolation_keyword(name, ISOLATION_NEEDLES_STRONG)
        || (movement_pattern(name).is_none() && isolation_keyword(name, ISOLATION_NEEDLES_WEAK))
}

/// Unambiguous single-joint needles: a name containing one is isolation even
/// when it also matches a compound movement pattern.
const ISOLATION_NEEDLES_STRONG: &[&str] = &[
    "curl",
    "fly",
    "flye",
    "flies",
    "pushdown",
    "pressdown",
    "kickback",
    "shrug",
    "pec deck",
];

/// Needles that also appear in compound movements, so they classify only a name
/// with no compound movement pattern ("lateral raise" / "leg extension" yes,
/// "back extension" no).
const ISOLATION_NEEDLES_WEAK: &[&str] = &["raise", "extension"];

/// Whole-word keyword match over a needle list, with the same token windowing +
/// plural tolerance `movement_pattern` uses: a multi-word needle ("pec deck")
/// matches a contiguous token run, and "curls"/"raises" match their singular.
fn isolation_keyword(name: &str, needles: &[&str]) -> bool {
    with_name_tokens(name, |tokens| {
        needles.iter().any(|kw| has_phrase(tokens, kw))
    })
}

/// Whether two exercise names denote the same movement for dedup and completion
/// purposes. TOKEN-normalized: lowercase, split on every non-alphanumeric char,
/// drop equipment tokens (barbell/dumbbell/dumbbells/kettlebell/cable/machine/
/// smith) and empty tokens, then compare the remaining token sequences. So
/// "Cable row" ≡ "Row" ≡ "Barbell row" ≡ "Row (barbell/cable/machine)" and
/// "Push-up" ≡ "Push up", while POSITIONAL / movement modifiers are kept, so
/// "Front squat" ≠ "Back squat" and "Deadlift" ≠ "Romanian deadlift" stay
/// distinct lifts. Splitting on punctuation (rather than substring stripping)
/// drops the separator glyphs in an equipment-qualified name like
/// "Row (barbell/cable/machine)", so it matches a plain "Row" log.
/// A bare "Squat" vs "Back squat" still counts as two (they could be different
/// lifts); closing that safely needs a KB-backed movement-synonym lexicon the
/// KB does not provide (HARD RULE 1), so it is left as a rare cosmetic dup
/// rather than risk false-merging distinct lifts.
pub(crate) fn same_movement(a: &str, b: &str) -> bool {
    let (mut x, mut y) = (movement_tokens(a), movement_tokens(b));
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some(p), Some(q)) if p.eq_ignore_ascii_case(q) => {}
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
    movement_tokens(name).map(str::to_ascii_lowercase).collect()
}

/// The tokens of [`movement_key`], unlowered and unallocated: the canonical
/// name split on every non-alphanumeric char, empties and equipment words
/// dropped (compared ASCII-case-insensitively).
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
        .filter(|t| !t.is_empty() && !EQUIPMENT.iter().any(|e| t.eq_ignore_ascii_case(e)))
}

/// Heavy lifting: at or above 80 % 1RM, the floor of the max-strength band a
/// main lift needs ≥2×/week (STR-HEAVYFREQ-001; File 02 strength-001
/// 80–100 %). Read by the CAP-3 heavy-leg spacing (`schedule::session_load`,
/// HYB-CAP-001) and the meet taper's heavy squat/bench cut.
pub(super) const HEAVY_PCT_1RM: f32 = 80.0;

/// The DUP emphasis for a lift day, GOAL-AWARE (`day_index % 3`):
/// - Hypertrophy leads with its hypertrophy day, so even 2 lift days carry one
///   6–12-rep day.
/// - MaxStrength: Heavy → HeavyVolume → Hypertrophy. Each main lift gets ≥80 %
///   1RM in ≥2 sessions a week at 2 OR 3+ lift days; the lighter day adds to
///   the heavy ones rather than replacing one (STR-HEAVYFREQ-001; strength-001
///   80–100 %).
/// - Power, strong athlete: the classic Heavy → Power → Hypertrophy week
///   (strength-023).
/// - Power, novice or below the ~1.5×BW squat benchmark (`power_base`):
///   Heavy → Power → HeavyVolume — two heavy days and one power day, because
///   weaker athletes gain jump/sprint performance about as much from heavy
///   strength work and far more 1RM (STR-BASE-POWER-001).
pub(super) fn dup_emphasis(goal: LiftGoal, power_base: bool, day_index: usize) -> DupDay {
    let base = match goal {
        LiftGoal::Hypertrophy => [DupDay::Hypertrophy, DupDay::Heavy, DupDay::Power],
        LiftGoal::MaxStrength => [DupDay::Heavy, DupDay::HeavyVolume, DupDay::Hypertrophy],
        LiftGoal::Power if power_base => [DupDay::Heavy, DupDay::Power, DupDay::HeavyVolume],
        LiftGoal::Power => [DupDay::Heavy, DupDay::Power, DupDay::Hypertrophy],
    };
    base[day_index % 3]
}

/// Whether a Power-goal athlete should run the strength-weighted week
/// (STR-BASE-POWER-001): a novice, or anyone whose squat e1RM is not yet shown
/// to reach the ~1.5× bodyweight benchmark (the same cut-off as the depth-jump
/// prerequisite, PLYO-PREREQ-001; ExpertOpinion as a threshold). Unknown
/// bodyweight or no squat anchor counts as "not yet shown". Always `false` for
/// other goals.
pub(super) fn power_needs_strength_base(
    profile: &Profile,
    anchors: &Anchors,
    ta: TrainingAge,
) -> bool {
    profile.lift_goal == LiftGoal::Power
        && (ta == TrainingAge::Novice || !squat_benchmark_met(profile, anchors))
}

/// Squat working e1RM ≥ 1.5× bodyweight (`strength::depth_jump_ready`).
fn squat_benchmark_met(profile: &Profile, anchors: &Anchors) -> bool {
    match (profile.bodyweight_kg, squat_e1rm(anchors)) {
        (Some(bw), Some(sq)) => strength::depth_jump_ready(sq, bw).value,
        _ => false,
    }
}

/// The working e1RM of the athlete's anchored squat (a squat-pattern lift
/// named "squat", so a leg press or lunge never stands in for it).
fn squat_e1rm(anchors: &Anchors) -> Option<f64> {
    anchors
        .lift_e1rm
        .iter()
        .find(|(n, _)| {
            movement_pattern(n) == Some(MovementPattern::Squat)
                && movement_key(n)
                    .iter()
                    .any(|t| t == "squat" || t == "squats")
        })
        .and_then(|(n, _)| anchors.working_e1rm_for(n))
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
/// templates), so this day's region is `week_regions.region(slot)`.
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
    // A hypertrophy goal does not run the strength DUP week (HYP-PERIOD-001:
    // linear ≈ DUP for growth, so no heavy/power days; HYP-RIR-DOSE-001 /
    // HYP-VL-001: speed sets far from failure are a weak growth stimulus). Its
    // own week builder sizes every muscle's weekly sets and programs each day
    // at 1–3 RIR with double progression (`plan::hypertrophy`).
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
    } else if goal == LiftGoal::Hypertrophy {
        LiftSessionType::Repetition
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

/// Per-session working sets sized from the stated weekly per-muscle target
/// (`weekly_sets`; knowledge-base/weekly-set-volume-to-prescription-mapping.md).
/// TWO LAYERS:
///
/// 1. Volume dose-response: CITABLE (Moderate, `HYP-VOL-DOSE-001`, Pelland 2026 /
///    Schoenfeld 2017): more weekly volume → more growth with diminishing returns.
///    So the weekly target is first BOUNDED to the primary muscle's OWN Table 1
///    landmark range (`HYP-LANDMARKS-001`), floored at that muscle's MEV (below
///    MEV growth is negligible, so a sub-MEV answer is raised to the floor rather
///    than programmed as ineffective volume) and ceilinged at that muscle's MRV
///    (past the optimal anchor returns diminish and beyond MRV is deload
///    territory, not a sizing target, so we never scale linearly to absurd
///    volumes). When the exercise has no Table 1 entry the floor falls back to the
///    training-age MEV low (`HYP-MEV-AGE-001`) and the ceiling to the RP framework's
///    generic ~20-set anchor (`HYP-MRV-DELOAD-001`).
///
/// 2. Allocation / split: EXPERT-OPINION (no validated weekly-target →
///    per-session/per-exercise formula exists; the KB's authoritative source counts
///    1:1 plus "logical rationale and personal expertise"). The primary lift
///    undulates the SAME movement every lift day, so its muscle is trained
///    `n_lift_days` times: the honest even split is
///    `round(bounded_weekly / n_lift_days)`. This arithmetic is the expert-opinion
///    heuristic flagged in `HYP-VOL-DOSE-001`'s `contradicting` note.
///
/// 1b. Accumulation ramp: the bounded weekly target is week 1 of the block, and
///    the following load weeks climb toward the top of the muscle's productive
///    (MAV) range, not its MRV (HYP-VOL-CAP-001; `hypertrophy::weekly_set_ramp`,
///    HYP-SETRAMP-001 shape, ExpertOpinion, capped at
///    `MAX_WEEKLY_SET_INCREMENT` = +4 sets/week by the hyp-011 no-abrupt-jump
///    guard). Week 1 is therefore exactly the user's stated target; only later
///    weeks of the block add volume, and `ramped` reports whether the ramp (not
///    the stated target) produced the returned count so the caller can cite the
///    claim that actually set the number.
///
/// The result is clamped to `[MIN_SETS_PER_EXERCISE, PER_SESSION_SET_CEILING]`
/// (`HYP-SESSCAP-001` ~11) AND into the day's own cited intensity `band` [lo,hi],
/// so a heavy-strength day never inflates past the set ceiling its own claim
/// sanctions, volume only chooses WHERE within the already-cited band it lands.
///
/// ACCESSORIES (documented, not a bypass): with the pattern-balanced full-body
/// day (STR-SESSION-BALANCE-001) every exercise, primary AND accessory, recurs
/// on each lift day, so each is trained `n_lift_days`/week and the even split
/// `weekly/n_lift_days` is the honest per-session count for any ANCHORED lift (it
/// runs the DUP path below). An UNANCHORED accessory (a catalog pattern-fill) is
/// prescribed by RIR at its loading band's own set count, NOT a fabricated
/// per-fill volume attribution the KB does not support (HARD RULE 1), the stated
/// weekly-set target is the PER-MUSCLE figure the user gave for their primary
/// emphasis, not a per-fill quantity.
pub(super) fn sized_sets(
    weekly_sets: u8,
    ta: TrainingAge,
    n_lift_days: usize,
    band: (u8, u8),
    landmarks: Option<&hypertrophy::VolumeLandmarks>,
    week_in_block: u8,
    block_weeks: u8,
) -> SizedSets {
    // Layer 1: bound the weekly per-muscle target to the cited landmark range.
    // The floor and ceiling come from the primary muscle's OWN Table 1 landmarks
    // (HYP-LANDMARKS-001: MEV floor, MRV ceiling — e.g. glutes 0→16, side delts
    // 8→26), so a low-MEV muscle is not forced to a generic floor and a high-MRV
    // muscle is not clamped short. Only when the exercise has no Table 1 entry do
    // we fall back to the training-age MEV band (HYP-MEV-AGE-001) and the RP
    // framework's generic ~20-set MRV anchor (HYP-MRV-DELOAD-001).
    let mev_floor = landmarks
        .map(|l| l.mev)
        .unwrap_or_else(|| hypertrophy::mev_sets_by_training_age(ta).value.0);
    let mrv_ceiling = landmarks.map_or(hypertrophy::GENERIC_MRV_SETS, |l| l.mrv);
    let start = weekly_sets.clamp(mev_floor, mrv_ceiling);
    // Layer 1b: this week's weekly target along the ramp from the stated volume
    // to the top of the muscle's productive range (MAV high, HYP-VOL-CAP-001;
    // the generic ~20-set anchor without Table 1 landmarks, the top of the
    // ~10–20 productive band). `weekly_set_ramp` always opens at its first
    // argument, so week 1 is the stated target; a zero-length block leaves it
    // unchanged, and a stated target above the ramp top is held.
    let ramp_top = landmarks.map_or(mrv_ceiling, |l| l.mav.1);
    let ramp = hypertrophy::weekly_set_ramp(start, ramp_top, block_weeks).value;
    let bounded_weekly = ramp
        .get((week_in_block.max(1) - 1) as usize)
        .copied()
        .unwrap_or(start);
    // Layer 2 (EXPERT-OPINION): even split across the lift days that train the
    // primary muscle. No validated formula, a practitioner heuristic.
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

    // Evidence travels with the claim that set this week's number: a %1RM ramp
    // (STR-PCTPROG-001), the goal-weighted week (STR-HEAVYFREQ-001 /
    // STR-BASE-POWER-001), the calibration test set, the Olympic-derivative
    // optimum; else on the DUP path the volume-sized set count
    // (HYP-VOL-DOSE-001, or the weaker HYP-SETRAMP-001 when the ramp moved it);
    // else the loading band itself. The badge never reads stronger than the
    // claim that produced the figure.
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
    /// hypertrophy-emphasis set rests per the EXERCISE CLASS (HYP-REST-001:
    /// 120 s heavy compounds, 60 s otherwise, catalog-unknown treated as a
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
    /// 1 in reserve.
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
pub(crate) fn is_upper_body(name: &str) -> bool {
    !matches!(
        movement_pattern(name),
        Some(MovementPattern::Squat | MovementPattern::Hinge)
    )
}

/// Whether a lift is an Olympic weightlifting derivative (clean / snatch /
/// jerk variants, high pulls, jump shrugs). Only ever programmed when the
/// athlete already logs one — the skill prerequisite.
pub(crate) fn is_olympic_derivative(name: &str) -> bool {
    let key = movement_key(name);
    let has = |t: &str| key.iter().any(|k| k == t || k.strip_suffix('s') == Some(t));
    has("clean")
        || has("snatch")
        || has("jerk")
        || key.windows(2).any(|w| w[0] == "high" && w[1] == "pull")
        || key.windows(2).any(|w| w[0] == "jump" && w[1] == "shrug")
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
    let has = |t: &str| {
        tokens
            .iter()
            .any(|k| *k == t || k.strip_suffix('s') == Some(t))
    };
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
        || tokens
            .windows(2)
            .any(|w| w[0] == "good" && w[1] == "morning")
}

/// The ballistic opener of a power day (STR-POWER-SELECT-001): a
/// bodyweight countermovement jump — or, once a non-novice has a squat e1RM of
/// ≥1.5× bodyweight (PLYO-PREREQ-001, safety gate), a low-box depth jump.
/// Volume is the KB's low-dose plyometric band floor, 40 foot contacts as 8 × 5
/// (strength-032 / PLYO-001: low-dose 40–60 also effective; well inside every
/// level's cap), with 2 min between sets (STR-PLYO-SCHED-001: 2–3 min). Power
/// days are at most 2 of every 3 lift days and never adjacent in the
/// `dup_emphasis` order, which keeps jumps ≤3×/wk.
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

/// Shape a week toward a strength test / meet.
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
pub(super) fn apply_meet_taper(
    sessions: &mut [Session],
    goal: LiftGoal,
    days_to_meet_from_week_start: i64,
) {
    if !matches!(goal, LiftGoal::MaxStrength | LiftGoal::Power) {
        return;
    }
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
        LiftIntensity::VelocityMs(_) | LiftIntensity::MaxIntent => false,
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
            let drop_dl = deadlift && d < dl_cutoff;
            let drop_sb = squat_or_bench && d <= SQUAT_BENCH_CUTOFF && heavy(&l.intensity);
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
