//! Hypertrophy-goal lift week: every lift day is a
//! muscle-building day at 1–3 reps in reserve, sized so each muscle receives
//! the athlete's stated weekly sets (fractional counting), with isolation and
//! long-muscle-length work, class-appropriate reps and rest, and double
//! progression off the athlete's own last working sets.
//!
//! Why not the strength DUP week: linear and daily-undulating plans build
//! similar muscle (HYP-PERIOD-001), and the DUP power/speed day ends sets far
//! from failure (HYP-RIR-DOSE-001, HYP-VL-001), a weak growth stimulus. So the
//! week keeps one quality (1–3 RIR) and varies exercises, not intensities.
//!
//! Pure and deterministic: the whole week is allocated from the profile, the
//! anchors and the block position each call, then the requested day's share is
//! emitted, so every day of the week agrees with every other.

use crate::app::{Profile, ResistanceModality};
use crate::evidence::claims;
use crate::hypertrophy::{self, DoubleProgression, ExerciseClass, VolumeRole};
use crate::individualization::TrainingAge;
use crate::schema::{
    LiftIntensity, LiftPrescription, LiftSessionType, Prescription, Recommended, Session,
    SessionType,
};
use crate::strength::{self, LiftGoal};

use super::lift::{BlockCtx, is_isolation_lift, movement_key, same_movement};
use super::{Anchors, DayRegion, MovementPattern, movement_pattern};

/// Whether a profile's lift days run this hypertrophy week with double
/// progression: a Hypertrophy goal on external loads. (A calisthenics profile
/// runs its leverage ladders instead.)
pub(crate) fn uses_double_progression(profile: &Profile) -> bool {
    profile.lift_goal == LiftGoal::Hypertrophy
        && profile.resistance_modality == ResistanceModality::Weights
}

/// Days a last working load stays current enough to carry forward: the same
/// 30-day full-credit window the e1RM anchor uses before detraining decay
/// starts (DETRAIN-001). Older sessions fall back to the e1RM anchor (which
/// decays) or to effort-only prescription.
pub(crate) const HYP_CARRY_DAYS: i64 = 30;

/// Table 1 muscles in sizing order: the big compound-trained muscles first, so
/// their secondary (½-set) credit is known before the small muscles that
/// receive it are sized (bench → triceps, squat/RDL → glutes).
const MUSCLE_ORDER: &[&str] = &[
    "quads",
    "hamstrings",
    "chest",
    "back",
    "glutes",
    "side delts",
    "rear delts",
    "triceps",
    "biceps",
    "calves",
    "abs",
];

/// Upper-body Table 1 muscles (trained on Upper or Full days); every other
/// muscle is lower body.
const UPPER_MUSCLES: &[&str] = &[
    "chest",
    "back",
    "side delts",
    "rear delts",
    "triceps",
    "biceps",
];

/// Default exercises per muscle, File 03 Table 4 rows by their display names,
/// favouring the high stimulus-to-fatigue and long-muscle-length options
/// (HYP-SFR-001, HYP-LENGTHSEL-001): the hack squat over the barbell squat,
/// the pulldown before the row, plus seated leg curl, chest fly, overhead
/// triceps extension, incline curl and standing calf raise. The athlete's own
/// logged lifts for a muscle come first and replace a default of the same kind.
/// Table 4 lists no rear-delt or ab exercise, so none is invented (a logged
/// one is still scheduled).
const DEFAULT_OPTIONS: &[(&str, &[&str])] = &[
    ("quads", &["Hack squat", "Leg extension"]),
    ("hamstrings", &["Romanian deadlift", "Seated leg curl"]),
    ("chest", &["Bench press", "Chest fly"]),
    ("back", &["Lat pulldown", "Cable row"]),
    ("glutes", &["Hip thrust"]),
    ("side delts", &["Lateral raise"]),
    ("triceps", &["Overhead triceps extension"]),
    ("biceps", &["Incline dumbbell curl"]),
    ("calves", &["Standing calf raise"]),
];

/// Exercises per muscle: 1–2 (HYP-DEFAULT-PROG-001 `exercises_per_muscle`).
const MAX_OPTIONS_PER_MUSCLE: usize = 2;

/// Logged lifts that train no Table 1 muscle directly (an overhead press, a
/// carry) are kept in the plan, never dropped; at most this many of the most
/// recent, each at the hypertrophy loading band's own set count.
const MAX_UNMAPPED_LOGGED: usize = 2;

/// One muscle's weekly target this week (shared with the program card).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MuscleWeekTarget {
    pub(crate) muscle: &'static str,
    /// Weekly working sets this week (0 = no direct target).
    pub(crate) target: u8,
    pub(crate) role: VolumeRole,
}

/// Every Table 1 muscle's weekly set target for week `week_in_block` of a
/// `block_weeks` block on `lift_days` lift days, from the
/// SAME `hypertrophy::weekly_targets` the volume planner shows, then fitted to
/// the whole-session cap: when the standard muscles' week would put more than
/// [`hypertrophy::SESSION_WORKING_SET_CAP`] working sets on the average lift
/// day, every standard target is scaled down by one shared factor
/// (HYP-VOL-CAP-001), so the cut is spread and no muscle loses all its work.
/// A specialization block's priority and maintenance targets are never
/// scaled. The plan and the program card both read this, so they agree.
pub(crate) fn hypertrophy_week_targets(
    profile: &Profile,
    ta: TrainingAge,
    week_in_block: u8,
    block_weeks: u8,
    lift_days: usize,
) -> Vec<MuscleWeekTarget> {
    let weeks = block_weeks.max(1);
    let idx = (week_in_block.clamp(1, weeks) - 1) as usize;
    let raw: Vec<MuscleWeekTarget> = MUSCLE_ORDER
        .iter()
        .filter_map(|m| hypertrophy::landmarks_for(m))
        .map(|lm| {
            let role =
                hypertrophy::volume_role(lm.muscle, profile.specialization_muscle.as_deref(), ta)
                    .value;
            let ramp = hypertrophy::weekly_targets(lm, profile.weekly_sets, role, weeks).value;
            MuscleWeekTarget {
                muscle: lm.muscle,
                target: ramp.get(idx).copied().unwrap_or(0),
                role,
            }
        })
        .collect();
    // Fit the standard targets so the week packs under the cap: sized as the
    // plan sizes it, on full-body days with the default exercises (the
    // athlete's own lifts change the split, not the total; each day's cap
    // in `allocate_week` still holds either way). Start from the share of the
    // budget the uncapped week overshoots by, then trim in small steps until
    // no day had to cut a muscle short.
    let days = lift_days.clamp(1, 7);
    let regions = super::WeekRegions::full(days);
    let cap = u32::from(hypertrophy::SESSION_WORKING_SET_CAP);
    let total = |targets: &[MuscleWeekTarget], day_cap: u32| -> u32 {
        allocate_week(&Anchors::default(), regions.as_slice(), targets, day_cap)
            .iter()
            .map(|it| u32::from(it.sets))
            .sum()
    };
    let scaled = |scale: f32| -> Vec<MuscleWeekTarget> {
        raw.iter()
            .map(|t| match t.role {
                VolumeRole::Standard => MuscleWeekTarget {
                    target: (f32::from(t.target) * scale).floor() as u8,
                    ..*t
                },
                _ => *t,
            })
            .collect()
    };
    let fits = |targets: &[MuscleWeekTarget]| total(targets, u32::MAX) == total(targets, cap);
    if fits(&raw) {
        return raw;
    }
    let budget = cap * days as u32;
    let mut scale = (budget as f32 / total(&raw, u32::MAX) as f32).min(1.0);
    let mut out = scaled(scale);
    while scale > 0.0 && !fits(&out) {
        scale -= 0.02;
        out = scaled(scale.max(0.0));
    }
    out
}

/// One allocated exercise occurrence in the week.
struct Item {
    slot: usize,
    name: String,
    sets: u8,
    /// Sort key inside a day: exercise class, then muscle order.
    order: (u8, usize),
}

fn trains(region: DayRegion, muscle: &str) -> bool {
    let upper = UPPER_MUSCLES.contains(&muscle);
    match region {
        DayRegion::Full => true,
        DayRegion::Upper => upper,
        DayRegion::Lower => !upper,
    }
}

/// Whether a lift with no Table 1 primary muscle belongs on a day's region:
/// push/pull on upper days, squat/hinge on lower days, anything else anywhere.
fn region_fits(region: DayRegion, name: &str) -> bool {
    match (region, movement_pattern(name)) {
        (DayRegion::Full, _) | (_, None) => true,
        (DayRegion::Upper, Some(p)) => matches!(p, MovementPattern::Push | MovementPattern::Pull),
        (DayRegion::Lower, Some(p)) => !matches!(p, MovementPattern::Push | MovementPattern::Pull),
    }
}

fn class_rank(class: ExerciseClass) -> u8 {
    match class {
        ExerciseClass::HeavyCompound => 0,
        ExerciseClass::ModerateCompound => 1,
        ExerciseClass::Isolation => 2,
    }
}

/// The athlete's logged movements, most recent first, deduplicated by
/// movement: last working sets first (they cover isolation and bodyweight
/// lifts), then any e1RM anchor not already listed.
fn logged_names(anchors: &Anchors) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut keys: Vec<Vec<String>> = Vec::new();
    let names = anchors
        .last_working
        .iter()
        .map(|w| w.exercise.as_str())
        .chain(anchors.lift_e1rm.iter().map(|(n, _)| n.as_str()));
    for name in names {
        let key = movement_key(name);
        if !keys.contains(&key) {
            keys.push(key);
            out.push(name.to_string());
        }
    }
    out
}

/// Exercise options for one muscle: the athlete's logged lifts whose primary
/// muscle it is, then Table 4 defaults (skipping a default of the same kind,
/// compound or isolation, as a logged lift), at most two.
fn options_for(muscle: &str, logged: &[(String, Option<&'static str>)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (name, primary) in logged {
        if out.len() >= MAX_OPTIONS_PER_MUSCLE {
            break;
        }
        if *primary == Some(muscle) && !out.iter().any(|o| same_movement(o, name)) {
            out.push(name.clone());
        }
    }
    let defaults = DEFAULT_OPTIONS
        .iter()
        .find(|(m, _)| *m == muscle)
        .map_or(&[][..], |(_, d)| *d);
    for d in defaults {
        if out.len() >= MAX_OPTIONS_PER_MUSCLE {
            break;
        }
        let kind = is_isolation_lift(d);
        if out
            .iter()
            .any(|o| same_movement(o, d) || is_isolation_lift(o) == kind)
        {
            continue;
        }
        out.push((*d).to_string());
    }
    out
}

/// Pick `f` distinct slots from `eligible`, each time the least-loaded one
/// (fewest sets already placed), ties to the earlier day.
fn pick_slots(eligible: &[usize], load: &[u32], f: usize) -> Vec<usize> {
    let mut chosen: Vec<usize> = Vec::with_capacity(f);
    for _ in 0..f {
        if let Some(&s) = eligible
            .iter()
            .filter(|s| !chosen.contains(s))
            .min_by_key(|&&s| (load[s], s))
        {
            chosen.push(s);
        }
    }
    chosen.sort_unstable();
    chosen
}

/// Allocate the whole week: which exercise lands on which lift day with how
/// many sets, so each muscle's fractional weekly sets meet its target.
///
/// Per muscle (in [`MUSCLE_ORDER`]): the sets still needed = target − the ½-set
/// credit it already receives from earlier muscles' compounds; spread over the
/// Table 3 frequency for that weekly volume (HYP-FREQ-001), raised so no
/// session exceeds the ~11-set per-muscle cap (HYP-SESSCAP-001) and lowered
/// so no exercise drops below 2 sets (HYP-DEFAULT-PROG-001 / hyp-006);
/// occurrences rotate through the muscle's exercise options and go to the
/// least-loaded eligible days (Upper/Lower/Full regions respected). Weekly
/// volume therefore depends on the stated target, not on the number of lift
/// days (LIFT-DAYS-001).
///
/// Whole-session cap (HYP-VOL-CAP-001): the targets come already fitted to
/// the cap ([`hypertrophy_week_targets`]), and no single day takes more than
/// [`hypertrophy::SESSION_WORKING_SET_CAP`] working sets (a full day accepts
/// no further muscle).
fn plan_week(profile: &Profile, anchors: &Anchors, ta: TrainingAge, block: BlockCtx) -> Vec<Item> {
    let regions: Vec<DayRegion> = {
        let r = block.week_regions.as_slice();
        if r.is_empty() {
            vec![DayRegion::Full]
        } else {
            r.to_vec()
        }
    };
    let targets = hypertrophy_week_targets(
        profile,
        ta,
        block.week_in_block,
        block.block_weeks,
        regions.len(),
    );
    allocate_week(
        anchors,
        &regions,
        &targets,
        u32::from(hypertrophy::SESSION_WORKING_SET_CAP),
    )
}

/// One allocation pass: each muscle sized to its `targets` entry over the
/// days in `regions`, no day filled past `day_cap` working sets.
fn allocate_week(
    anchors: &Anchors,
    regions: &[DayRegion],
    targets: &[MuscleWeekTarget],
    day_cap: u32,
) -> Vec<Item> {
    let n = regions.len();
    let mut load = vec![0u32; n];
    let mut credit = [0.0f32; MUSCLE_ORDER.len()];
    let mut items: Vec<Item> = Vec::new();
    let logged: Vec<(String, Option<&'static str>)> = logged_names(anchors)
        .into_iter()
        .map(|n| {
            let p = hypertrophy::primary_muscle(&n);
            (n, p)
        })
        .collect();

    // 1. Logged lifts with no Table 1 primary muscle: kept, never dropped, at
    //    the hypertrophy loading band's set count, up to twice a week.
    let unmapped_sets = strength::loading_rx(LiftGoal::Hypertrophy).value.sets.0;
    let session_cap = day_cap;
    for (name, _) in logged
        .iter()
        .filter(|(_, p)| p.is_none())
        .take(MAX_UNMAPPED_LOGGED)
    {
        let mut eligible: Vec<usize> = (0..n).filter(|&s| region_fits(regions[s], name)).collect();
        if eligible.is_empty() {
            eligible = (0..n).collect();
        }
        eligible.retain(|&s| load[s].saturating_add(u32::from(unmapped_sets)) <= session_cap);
        let f = eligible.len().min(2);
        for slot in pick_slots(&eligible, &load, f) {
            load[slot] += u32::from(unmapped_sets);
            add_credits(&mut credit, name, None, unmapped_sets);
            items.push(Item {
                slot,
                name: name.clone(),
                sets: unmapped_sets,
                order: (class_rank(hypertrophy::classify(name)), MUSCLE_ORDER.len()),
            });
        }
    }

    // 2. Each muscle, sized to its weekly target.
    let min_sets = u32::from(hypertrophy::MIN_SETS_PER_EXERCISE);
    let cap = u32::from(hypertrophy::PER_SESSION_SET_CEILING);
    for (mi, muscle) in MUSCLE_ORDER.iter().enumerate() {
        let Some(t) = targets.iter().find(|t| t.muscle == *muscle) else {
            continue;
        };
        if t.target == 0 {
            continue;
        }
        let options = options_for(muscle, &logged);
        if options.is_empty() {
            continue;
        }
        let target = f32::from(t.target);
        let need = target - credit[mi];
        if need < 1.0 {
            continue; // indirect work from the compounds already covers it
        }
        let mut total = (need.round() as u32).max(min_sets);
        let mut eligible: Vec<usize> = (0..n).filter(|&s| trains(regions[s], muscle)).collect();
        if eligible.is_empty() {
            eligible = (0..n).collect();
        }
        // A day with no room for even the per-exercise minimum is full.
        eligible.retain(|&s| load[s].saturating_add(min_sets) <= session_cap);
        if eligible.is_empty() {
            continue;
        }
        // Table 3 frequency for this weekly volume (HYP-FREQ-001), floored at
        // the default program's 2×/week per muscle (HYP-DEFAULT-PROG-001) when
        // the days allow; the per-exercise minimum below can still lower it.
        let freq_lo = usize::from(
            hypertrophy::frequency_for_weekly_sets(target as u8)
                .value
                .freq
                .0
                .max(
                    hypertrophy::intermediate_default_program()
                        .value
                        .frequency_per_week,
                ),
        );
        let mut f = freq_lo
            .max(total.div_ceil(cap) as usize)
            .clamp(1, eligible.len());
        f = f.min((total / min_sets).max(1) as usize);
        total = total.min(cap * f as u32);
        let chosen = pick_slots(&eligible, &load, f);
        let f = chosen.len().max(1) as u32;
        let (base, rem) = (total / f, total % f);
        for (j, &slot) in chosen.iter().enumerate() {
            let sets = (base + u32::from((j as u32) < rem))
                .min(session_cap.saturating_sub(load[slot]))
                .min(u32::from(u8::MAX)) as u8;
            let name = &options[j % options.len()];
            load[slot] += u32::from(sets);
            add_credits(&mut credit, name, Some(*muscle), sets);
            items.push(Item {
                slot,
                name: name.clone(),
                sets,
                order: (class_rank(hypertrophy::classify(name)), mi),
            });
        }
    }
    items
}

/// Add an exercise's secondary (½-set) credit to every OTHER Table 1 muscle it
/// trains (`own` = the muscle it was sized for, credited directly).
fn add_credits(credit: &mut [f32], name: &str, own: Option<&str>, sets: u8) {
    for (m, w) in hypertrophy::muscle_credits(name) {
        if Some(m) != own
            && let Some(i) = MUSCLE_ORDER.iter().position(|k| *k == m)
        {
            credit[i] += w * f32::from(sets);
        }
    }
}

/// Build one hypertrophy lift day (`block.slot` of the week). Every day is a
/// muscle-building (`Repetition`) day: no heavy-triple or speed day exists in
/// this week, so the title never promises one.
pub(super) fn hypertrophy_session(
    profile: &Profile,
    anchors: &Anchors,
    ta: TrainingAge,
    day: u16,
    block: BlockCtx,
) -> Session {
    let mut today: Vec<Item> = plan_week(profile, anchors, ta, block)
        .into_iter()
        .filter(|it| it.slot == block.slot)
        .collect();
    today.sort_by_key(|it| it.order);
    Session {
        session_type: SessionType::Lift(LiftSessionType::Repetition),
        day,
        prescriptions: today
            .iter()
            .map(|it| prescribe(&it.name, it.sets, ta, anchors, block))
            .collect(),
        dup_day: None,
    }
}

/// One exercise's prescription: its class rep range (HYP-REPLOAD-001), this
/// week's target reps in reserve (`hypertrophy::block_rir`), class rest
/// (HYP-REST-002) and a load from, in order: double progression off the last
/// working sets (≤30 days old), the e1RM anchor converted to the target effort
/// (first exposure), or none (pick a weight for the target effort).
fn prescribe(
    name: &str,
    sets: u8,
    ta: TrainingAge,
    anchors: &Anchors,
    block: BlockCtx,
) -> Recommended<Prescription> {
    let class = hypertrophy::classify(name);
    let (bottom, top) = hypertrophy::rep_range_for(class).value;
    let rir_rx = hypertrophy::block_rir(
        block.week_in_block,
        block.block_weeks,
        ta == TrainingAge::Novice,
    );
    let rir = rir_rx.value;
    let rest_sec = hypertrophy::rest_sec_for(class).value.0;
    let lift = |reps: u8, intensity: LiftIntensity, load_kg: Option<f32>| {
        Prescription::Lift(LiftPrescription {
            exercise: name.to_string(),
            sets,
            reps,
            intensity,
            rest_sec,
            velocity_loss_pct: None,
            reps_max: Some(top),
            load_kg,
        })
    };
    let by_effort = |rx: Prescription| rir_rx.with_value(rx);

    // A >8 wk layoff re-enters like a novice by effort (REENTRY-001): no load
    // is carried from before the break.
    if !anchors.reentry_novice {
        if let Some(lw) = anchors
            .last_working_for(name)
            .filter(|w| w.age_days <= HYP_CARRY_DAYS)
        {
            let w = lw.weight_kg;
            let step = hypertrophy::double_progression((bottom, top), rir, &lw.sets);
            let load = |kg: f64| (kg > 0.0).then_some(kg as f32);
            return match step.value {
                DoubleProgression::AddLoad if w > 0.0 => crate::evidence::graded(
                    lift(
                        bottom,
                        LiftIntensity::Rir(rir),
                        load(w + hypertrophy::load_step_kg(w)),
                    ),
                    claims::HYP_DOUBLEPROG_001,
                ),
                // Bodyweight: no load to add, so hold the top of the range.
                DoubleProgression::AddLoad => crate::evidence::graded(
                    lift(top, LiftIntensity::Rir(rir), None),
                    claims::HYP_DOUBLEPROG_001,
                ),
                DoubleProgression::Hold { target_reps } => crate::evidence::graded(
                    lift(target_reps, LiftIntensity::Rir(rir), load(w)),
                    claims::HYP_DOUBLEPROG_001,
                ),
                DoubleProgression::Rederive => {
                    // Another rep scheme was logged: estimate the 1RM from the
                    // hardest-to-repeat set (fewest reps; unrated = taken to
                    // failure, the conservative reading) and load the range
                    // bottom at the target effort.
                    let kg = lw
                        .sets
                        .iter()
                        .min_by_key(|(r, _)| *r)
                        .map(|&(r, rpe)| {
                            strength::e1rm_epley_rpe(w, r.max(1), rpe)
                                * f64::from(hypertrophy::pct_1rm_for_reps_in_reserve(bottom, rir))
                                / 100.0
                        })
                        .unwrap_or(0.0);
                    by_effort(lift(bottom, LiftIntensity::Rir(rir), load(kg)))
                }
            };
        }
        if class != ExerciseClass::Isolation && anchors.e1rm_for(name).is_some() {
            // First exposure with only an e1RM anchor: the %1RM that leaves the
            // target reps in reserve at the range bottom (8 @ 2 RIR → 75 %).
            let pct = hypertrophy::pct_1rm_for_reps_in_reserve(bottom, rir);
            return by_effort(lift(bottom, LiftIntensity::PercentOneRm(pct), None));
        }
    }
    by_effort(lift(bottom, LiftIntensity::Rir(rir), None))
}
