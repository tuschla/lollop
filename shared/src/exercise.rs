//! The one exercise-name taxonomy: legacy catalog labels, the File 03 Table 4
//! exercise catalog and Table 2 exercise class, core movement patterns, the
//! isolation / barbell / Olympic classes and the movement identity every plan,
//! review and readiness rule matches names with. String folding itself lives
//! in `crate::names`.

use crate::memo::{MemoId, memoized};

/// Table 4's earlier label of the Cable row, also matched as a seated row by
/// the pain-region allow-list.
pub const LEGACY_CABLE_ROW: &str = "Row (barbell/cable/machine)";

/// The earlier labels each catalog row used to display (Table 4's verbatim
/// labels and retired display names), paired with
/// its current display name. Logged history keeps whatever name the set was
/// logged under, so these keep resolving to the same row and the same movement
/// ([`canonical_name`], [`movement_key`]). Each pair keeps the keyword
/// classes the old label had (movement pattern, isolation, barbell, and the
/// pain-region sparing words), so no rule reads the new name differently.
pub const LEGACY_NAMES: &[(&str, &str)] = &[
    ("Barbell back squat", "Back squat"),
    ("Hack squat / leg press", "Hack squat"),
    ("Cable fly / pec deck", "Chest fly"),
    ("Pull-up / lat pulldown", "Lat pulldown"),
    (LEGACY_CABLE_ROW, "Cable row"),
    (
        "Overhead cable/DB triceps ext",
        "Overhead triceps extension",
    ),
    ("Incline DB curl", "Incline dumbbell curl"),
];

/// The calisthenics progression tables' row labels (`calis-progression.md`),
/// the earlier names of those rungs, paired with each rung's current name.
/// They stay in the calisthenics alias table so they keep matching their rung;
/// display goes through [`canonical_name`], so a set logged under an old label
/// shows under the current name. "Nordic negative" is left out: its current
/// name ("Negative Nordic curl") reads as isolation to the keyword rules.
pub const CALIS_LEGACY_NAMES: &[(&str, &str)] = &[
    ("Feet-elevated / decline push-up", "Decline push-up"),
    ("Parallette / straight-bar dip", "Straight-bar dip"),
    ("Single-leg RDL", "Single-leg Romanian deadlift"),
    ("Plank (front)", "Front plank"),
    ("Advanced tuck (open hips)", "Advanced tuck front lever"),
];

/// The current display name for an earlier catalog label: a Table 4 row's
/// legacy label ([`LEGACY_NAMES`]) or a calisthenics rung's KB table label
/// ([`CALIS_LEGACY_NAMES`]), case- and whitespace-insensitive.
/// `None` for any other name. Ingest stores a set logged under an old label
/// under this name, so every view shows the current one.
pub fn renamed(name: &str) -> Option<&'static str> {
    let key = crate::names::FoldKey::new(name.trim());
    LEGACY_NAMES
        .iter()
        .chain(CALIS_LEGACY_NAMES)
        .find(|&&(old, _)| key.is(old))
        .map(|&(_, new)| new)
}

/// The current catalog display name for an earlier label ([`renamed`]); any
/// other name is returned unchanged.
pub fn canonical_name(name: &str) -> &str {
    renamed(name).unwrap_or(name)
}

/// The core movement patterns a lift day covers (STR-SESSION-BALANCE-001):
/// squat / hinge / push / pull, vertical presses and pulls folded into `Push` /
/// `Pull`; the KB's defaults for a missing pattern are the horizontal ones. The cited principle is that multi-joint compounds do not fully
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

impl MovementPattern {
    pub(crate) fn is_lower(self) -> bool {
        matches!(self, MovementPattern::Squat | MovementPattern::Hinge)
    }

    pub(crate) fn is_upper(self) -> bool {
        matches!(self, MovementPattern::Push | MovementPattern::Pull)
    }
}

/// Classify an exercise name into a core movement pattern, conservatively:
/// whole-word, case-insensitive keyword matching (a plural token matches its
/// singular needle). Anything ambiguous (most isolation work, curls, raises,
/// calf work) is `None`; a logged unclassifiable lift is still scheduled.
pub(crate) fn movement_pattern(name: &str) -> Option<MovementPattern> {
    name_class(name).pattern
}

fn movement_pattern_uncached(name: &str) -> Option<MovementPattern> {
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

/// What an exercise NAME says, derived once per distinct name and memoized
/// ([`MemoId::ExerciseNames`]): plan synthesis and the views ask about the
/// same few names thousands of times.
struct NameClass {
    key: Vec<String>,
    isolation: bool,
    pattern: Option<MovementPattern>,
}

fn name_class(name: &str) -> std::rc::Rc<NameClass> {
    memoized(MemoId::ExerciseNames, name, || {
        std::rc::Rc::new(NameClass {
            key: movement_key_uncached(name),
            isolation: is_isolation_uncached(name),
            pattern: movement_pattern_uncached(name),
        })
    })
}

/// Whether a lift is an isolation movement, whose estimated 1RM is unreliable
/// (File 02 strength-006; STR-E1RM-CHECK-001). The one isolation taxonomy
/// ([`classify`] defers to it): the Table 4 catalog class, then
/// [`ISOLATION_NEEDLES_STRONG`] (isolation even inside a compound pattern:
/// "Chest fly", "Pec deck"), then, for a name with no compound pattern,
/// [`ISOLATION_NEEDLES_WEAK`] ("leg extension" yes, "back extension" no) or a
/// single-muscle fragment ([`isolation_muscle`]: "Skull crusher",
/// "Face pull", "Crunch").
pub(crate) fn is_isolation_lift(name: &str) -> bool {
    name_class(name).isolation
}

fn is_isolation_uncached(name: &str) -> bool {
    if exercise_entry(name).is_some_and(|e| e.class == ExerciseClass::Isolation) {
        return true;
    }
    with_name_tokens(name, |tokens| {
        let has = |kw: &str| has_phrase(tokens, kw);
        ISOLATION_NEEDLES_STRONG.iter().any(|kw| has(kw))
            || (movement_pattern_of(&has).is_none()
                && (ISOLATION_NEEDLES_WEAK.iter().any(|kw| has(kw))
                    || isolation_muscle(name).is_some()))
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
    name_class(a).key == name_class(b).key
}

/// The normalized token sequence [`same_movement`] compares: two names are the
/// same movement iff their keys are equal, so callers that match one name
/// against many can compute each key once and hash or compare it. A legacy
/// catalog label ("Hack squat / leg press") keys as its current display name
/// ("Hack squat"), so history logged under the old label stays the same lift.
pub(crate) fn movement_key(name: &str) -> Vec<String> {
    name_class(name).key.clone()
}

fn movement_key_uncached(name: &str) -> Vec<String> {
    movement_tokens(name).map(crate::names::fold).collect()
}

/// The tokens of [`movement_key`], unfolded and unallocated: the canonical
/// name split on every non-alphanumeric char, empties and equipment words
/// dropped (compared with [`crate::names::FoldKey`]).
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
    canonical_name(name)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| {
            let key = crate::names::FoldKey::new(t);
            !t.is_empty() && !EQUIPMENT.iter().any(|e| key.is(e))
        })
}

/// The barbell back squat, the PLYO-PREREQ-001 benchmark lift (File 02
/// strength-033). A hack, goblet, split or front squat is another movement, and
/// a "smith" / "machine" token is rejected before the equipment-blind
/// [`same_movement`] match. Shared by the plan's depth-jump gate and the lift
/// audit's.
pub(crate) fn is_back_squat(name: &str) -> bool {
    !with_name_tokens(name, |t| {
        t.iter().any(|w| matches!(*w, "smith" | "machine"))
    }) && same_movement("Barbell back squat", name)
}

/// Whether a lift is upper-body for the weekly %1RM increment
/// (STR-PCTPROG-001: +1 % upper vs +2.5 % lower). Push/Pull patterns are upper;
/// Squat/Hinge lower; an unclassifiable lift takes the smaller, upper-body step
/// (the conservative tie-break).
pub(crate) fn is_upper_body(name: &str) -> bool {
    !movement_pattern(name).is_some_and(MovementPattern::is_lower)
}

/// Whether a lift is an Olympic weightlifting derivative (clean / snatch /
/// jerk variants, high pulls, jump shrugs). Only ever programmed when the
/// athlete already logs one (the skill prerequisite).
pub(crate) fn is_olympic_derivative(name: &str) -> bool {
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

/// Whether a lift starts each rep from the floor (a deadlift, a clean or
/// snatch from the floor, a Pendlay row; not a Romanian, stiff-leg or hang
/// variant): its first warm-up is the bar with one full plate a side
/// ([`crate::strength::warm_up_ramp`]).
pub(crate) fn starts_from_floor(name: &str) -> bool {
    let e = crate::names::fold(name);
    let has = |k: &str| e.contains(k);
    !(has("romanian") || has("rdl") || has("stiff") || has("hang"))
        && (has("deadlift") || has("clean") || has("snatch") || has("pendlay"))
}

/// Which heavy competition lift the meet taper stops on its own schedule: a conventional/sumo deadlift (not a
/// Romanian or stiff-legged one, STR-DLPEAK-001), and a squat-pattern or any
/// bench lift (strength-029).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompetitionLift {
    pub(crate) deadlift: bool,
    pub(crate) squat_or_bench: bool,
}

pub(crate) fn competition_lift(name: &str) -> CompetitionLift {
    let (deadlift, bench) = with_name_tokens(name, |t| {
        let prefixed = |p: &str| t.iter().any(|w| w.starts_with(p));
        (
            has_phrase(t, "deadlift") && !prefixed("romanian") && !prefixed("stiff"),
            prefixed("bench"),
        )
    });
    CompetitionLift {
        deadlift,
        squat_or_bench: movement_pattern(name) == Some(MovementPattern::Squat) || bench,
    }
}

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

use Equipment::{Barbell, Bodyweight, Cable, Dumbbell, Machine};

/// File 03 Table 4, major exercise → primary muscle → equipment map,
/// transcribed verbatim (16 rows). Each row carries ONE plain sentence-case
/// display name; Table 4's own multi-option labels ("Hack squat / leg press")
/// live in [`LEGACY_NAMES`], so a set logged under one still resolves to
/// its row. Bias tiers: "High (deep)" / "High if seated upright" / "High at top" →
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

/// Look up a Table 4 row by exercise name (case-insensitive; a legacy label
/// resolves to its row).
pub fn exercise_entry(name: &str) -> Option<&'static ExerciseEntry> {
    let name = canonical_name(name);
    EXERCISES
        .iter()
        .find(|e| crate::names::fold_eq(e.name, name))
}

/// A Table 4 row for any spelling of a catalog movement: exact (case-
/// insensitive) name first, then the same movement after equipment/punctuation
/// normalization ([`same_movement`]), so "Barbell bench press" resolves to
/// the "Bench press" row. Lookup plumbing, not a KB claim.
pub(crate) fn catalog_entry(name: &str) -> Option<&'static ExerciseEntry> {
    exercise_entry(name).or_else(|| {
        let key = movement_key(name);
        EXERCISES
            .iter()
            .zip(CATALOG_MOVEMENT_KEYS.iter())
            .find(|(_, k)| **k == key)
            .map(|(e, _)| e)
    })
}

static CATALOG_MOVEMENT_KEYS: std::sync::LazyLock<Vec<Vec<String>>> =
    std::sync::LazyLock::new(|| {
        EXERCISES
            .iter()
            .map(|e| movement_key_uncached(e.name))
            .collect()
    });

/// Name fragments of machine- or cable-guided compounds. Table 2 files
/// "moderate compounds / machines" together, so a compound carrying one of
/// these reads as [`ExerciseClass::ModerateCompound`]. Classification plumbing
/// (expert-opinion keyword taxonomy, like [`movement_pattern`]).
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

/// Table 2 exercise class for ANY exercise name: the Table 4 row's own class
/// when the name is in the catalog; otherwise an isolation name
/// ([`is_isolation_lift`]) is [`ExerciseClass::Isolation`], a
/// machine/cable compound is [`ExerciseClass::ModerateCompound`], and any other
/// name with a core movement pattern ("Front squat", "Deadlift", "Dumbbell bench
/// press") is a [`ExerciseClass::HeavyCompound`]. An unclassifiable name takes
/// the moderate class (mid-window reps, ~90 s rest), never the heavy one.
pub fn classify(name: &str) -> ExerciseClass {
    if let Some(e) = catalog_entry(name) {
        return e.class;
    }
    if is_isolation_lift(name) {
        return ExerciseClass::Isolation;
    }
    let lower = crate::names::fold(name);
    if movement_pattern(name).is_some() && !MACHINE_NEEDLES.iter().any(|n| lower.contains(n)) {
        ExerciseClass::HeavyCompound
    } else {
        ExerciseClass::ModerateCompound
    }
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
    ("flies", "chest"),
    ("hip thrust", "glutes"),
    ("glute", "glutes"),
    ("crunch", "abs"),
    ("sit-up", "abs"),
    ("leg raise", "abs"),
];

/// The Table 1 muscle an isolation-named exercise trains directly: the first
/// [`ISOLATION_MUSCLE_NEEDLES`] fragment the name contains.
pub(crate) fn isolation_muscle(name: &str) -> Option<&'static str> {
    let lower = crate::names::fold(name);
    ISOLATION_MUSCLE_NEEDLES
        .iter()
        .find(|(needle, _)| lower.contains(needle))
        .map(|&(_, muscle)| muscle)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
