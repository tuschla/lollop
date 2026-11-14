//! Lift-day synthesis: pattern-balanced exercise selection, goal-aware DUP
//! emphasis, and volume-sized, evidence-graded lift prescriptions.

use std::collections::BTreeMap;

use crate::evidence::{ClaimId, claims};
use crate::exercise::{
    MovementPattern, is_back_squat, is_isolation_lift, is_olympic_derivative, is_upper_body,
    movement_key, movement_pattern, same_movement,
};
use crate::hypertrophy;
use crate::individualization::TrainingAge;
use crate::profile::{PlanProfile, Profile};
use crate::schema::{
    DupDay, LiftIntensity, LiftPrescription, LiftSessionType, Prescription, Recommended, RxBasis,
    Session, SessionType,
};
use crate::strength::{self, LiftGoal, LoadingRx};

use super::anchors::LiftAnchors;
use super::day::LiftDay;
use super::regions::WeekRegions;
use super::volume::sized_sets;

/// KB core movement patterns + the compound that FILLS each when the user's
/// log leaves it uncovered (exercise_entry-known names → real long-length /
/// SFR lookups); using them turns a one-pattern (e.g. leg-heavy) log into a
/// balanced week (STR-SESSION-BALANCE-001). The hinge is the conventional
/// deadlift: these days train a max-strength or power goal, so the hinge is
/// the lift whose 1RM they build (KB strength-035, STR-COMP-ANCHOR-001:
/// anchor strength work on the competition lift; movement/ROM specificity).
const CORE_PATTERN_FILLS: &[(MovementPattern, &str)] = &[
    (MovementPattern::Squat, "Back squat"),
    (MovementPattern::Hinge, "Deadlift"),
    (MovementPattern::Push, "Bench press"),
    (MovementPattern::Pull, "Cable row"),
];

/// Exercises per lift day: the DUP primary plus accessories covering the other
/// core patterns; ~3-4 spans squat/hinge/push/pull (STR-SESSION-BALANCE-001).
const MAX_LIFTS_PER_DAY: usize = 4;

/// A lift day's exercises, pattern-balanced (STR-SESSION-BALANCE-001): the
/// user's logged lifts first (each anchored → a load), one per core pattern,
/// then [`CORE_PATTERN_FILLS`] for the missing patterns (→ RIR).
pub(super) fn main_lifts(anchors: &LiftAnchors) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut covered: Vec<MovementPattern> = Vec::new();
    // 1. Logged lifts, newest first: one per core pattern (a repeat would
    //    unbalance the day), unclassifiable lifts always kept, never the same
    //    movement twice.
    for (name, _) in &anchors.e1rm {
        if out.len() >= MAX_LIFTS_PER_DAY {
            break;
        }
        if out.iter().any(|n| same_movement(n, name)) {
            continue;
        }
        match movement_pattern(name) {
            Some(pat) => {
                if covered.contains(&pat) {
                    continue;
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
    for &(pat, def) in CORE_PATTERN_FILLS {
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

/// The DUP emphasis for a lift day, GOAL-AWARE (`day_index % 3`, the day's
/// exposure index within its region, `regions::region_exposure_index`):
/// - MaxStrength: Heavy → HeavyVolume → Hypertrophy. Each main lift gets ≥80 %
///   1RM in ≥2 sessions a week at 2+ full-body days or an upper/lower split;
///   the lighter day adds to the heavy ones rather than replacing one
///   (STR-HEAVYFREQ-001; strength-001 80–100 %).
/// - Power, strong athlete: the classic Heavy → Power → Hypertrophy week
///   (strength-023).
/// - Power, novice or below the ~1.5×BW squat benchmark (`power_base`):
///   Heavy → Power → HeavyVolume: two heavy days and one power day, because
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
    goal: LiftGoal,
    profile: &Profile,
    anchors: &LiftAnchors,
    ta: TrainingAge,
) -> bool {
    goal == LiftGoal::Power && (ta == TrainingAge::Novice || !squat_benchmark_met(profile, anchors))
}

/// Back-squat working e1RM ≥ 1.5× bodyweight (`strength::depth_jump_ready`).
fn squat_benchmark_met(profile: &Profile, anchors: &LiftAnchors) -> bool {
    match (profile.bodyweight_kg, squat_e1rm(anchors)) {
        (Some(bw), Some(sq)) => strength::depth_jump_ready(sq, bw).value,
        _ => false,
    }
}

/// The working e1RM of the athlete's anchored barbell back squat.
fn squat_e1rm(anchors: &LiftAnchors) -> Option<f64> {
    anchors
        .e1rm
        .iter()
        .find(|(n, _)| is_back_squat(n))
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

/// Build one strength/power lift day for lift goal `goal`, its DUP emphasis
/// `emphasis_index` into the `dup_emphasis` rotation. Intermediate+ undulate
/// via `dup_day_rx`; novices run a linear `loading_rx` day. An anchored lift
/// is prescribed by %1RM; an unlogged one by RIR (never an invented load, HARD
/// RULE 1). The day's block week locates it inside the accumulation block, so
/// the same weekday progresses across the block instead of repeating week 1.
pub(super) fn lift_session(
    pp: &PlanProfile,
    goal: LiftGoal,
    anchors: &LiftAnchors,
    lift_names: &[String],
    emphasis_index: usize,
    day: &LiftDay,
) -> Session {
    let profile = pp.profile();
    let ta = pp.training_age();
    let power_base = power_needs_strength_base(goal, profile, anchors, ta);
    let emphasis = dup_emphasis(goal, power_base, emphasis_index);
    let power_day = emphasis == DupDay::Power;
    let mut prescriptions = Vec::new();
    // A power day leads with a ballistic jump, done fresh
    // (STR-POWER-SELECT-001): a bodyweight jump is the exercise whose optimal
    // power load is ~0 % 1RM, the end of the load spectrum barbell lifts can't
    // reach.
    if power_day {
        prescriptions.push(ballistic_item(profile, anchors, ta).with_kind(RxBasis::Workout));
    }
    // DUP undulates the SAME lifts across the week (File 02 strength-023).
    // Accessories COVER the other core movement patterns on the same full-body
    // day (STR-SESSION-BALANCE-001, Moderate): `lift_names` was built
    // pattern-balanced by `main_lifts`, so scheduling ALL of it keeps each
    // session balanced; each lift carries its own set face via
    // `lift_prescription`. On a power day a logged Olympic derivative moves to
    // the front, right after the jumps (strength-031: placed early).
    let mut order: Vec<&String> = lift_names.iter().collect();
    if power_day {
        order.sort_by_key(|n| !is_olympic_derivative(n));
    }
    for name in order {
        let rx = lift_prescription(goal, ta, emphasis, name, anchors, profile.weekly_sets, day);
        prescriptions.push(rx.with_kind(RxBasis::Workout));
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
        day: day.day,
        prescriptions,
        dup_day: undulates.then_some(emphasis),
    }
}

/// Weekly working sets per movement (`movement_key`) of `n_days` full-body
/// [`lift_session`] days of `lift_names` for `goal` (DUP rotation `0..n_days`),
/// read off the doses without building the sessions: the default layout's
/// weekly volume that LIFT-DAYS-001 conserves across day counts.
pub(super) fn full_body_weekly_sets(
    pp: &PlanProfile,
    goal: LiftGoal,
    anchors: &LiftAnchors,
    lift_names: &[String],
    n_days: usize,
    week_in_block: u8,
    block_weeks: u8,
) -> BTreeMap<Vec<String>, u32> {
    let profile = pp.profile();
    let ta = pp.training_age();
    let power_base = power_needs_strength_base(goal, profile, anchors, ta);
    let mut sets: BTreeMap<Vec<String>, u32> = BTreeMap::new();
    for k in 0..n_days {
        let day = LiftDay {
            day: k as u16,
            slot: k,
            n_exposures: n_days,
            week_in_block,
            block_weeks,
            regions: WeekRegions::full(n_days),
        };
        let emphasis = dup_emphasis(goal, power_base, k);
        if emphasis == DupDay::Power {
            *sets
                .entry(movement_key(ballistic_exercise(profile, anchors, ta)))
                .or_default() += u32::from(ballistic_sets());
        }
        for name in lift_names {
            let dose =
                DoseCtx::new(goal, ta, emphasis, name, anchors, profile.weekly_sets, &day).dose();
            *sets.entry(movement_key(name)).or_default() += u32::from(dose.sets);
        }
    }
    sets
}

/// One lift's evidence-cited prescription. DUP (%1RM) when anchored + non-novice;
/// linear `loading_rx` (%1RM anchored, RIR unanchored) otherwise. `week_in_block`
/// is 1-based. Progression across the block:
/// - Strength/power goals ramp the %1RM inside each day's cited band
///   (`strength::pct_for_week`, STR-PCTPROG-001: +2.5 %/wk lower body, +1 %/wk
///   upper body) and HOLD the week-1 set count: intensity, not hypertrophy
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
    anchors: &LiftAnchors,
    weekly_sets: u8,
    day: &LiftDay,
) -> Recommended<Prescription> {
    let ctx = DoseCtx::new(goal, ta, emphasis, exercise, anchors, weekly_sets, day);
    let dose = ctx.dose();

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
        Recommended::new(pres, id)
    } else if ctx.dup && dose.set_ramped {
        Recommended::new(pres, claims::HYP_SETRAMP_001)
    } else if ctx.dup {
        Recommended::new(pres, claims::HYP_VOL_DOSE_001)
    } else if goal == LiftGoal::Power {
        Recommended::new(pres, claims::STR_PWR_001)
    } else if dose.rir_ramped {
        Recommended::new(pres, claims::HYP_RIR_RAMP_001)
    } else {
        strength::loading_rx(ctx.day_goal).with_value(pres)
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
    anchors: &'a LiftAnchors,
    ta: TrainingAge,
    goal: LiftGoal,
    /// The band the day trains: a Power goal's non-power days are max-strength
    /// days (STR-BASE-POWER-001).
    day_goal: LiftGoal,
    emphasis: DupDay,
    weekly_sets: u8,
    day: LiftDay,
    /// `strength::loading_rx(day_goal)`.
    load: LoadingRx,
    anchored: bool,
    /// Anchored and non-novice: the lift undulates by %1RM.
    dup: bool,
    /// An isolation lift on a DUP day.
    is_isolation: bool,
    /// A logged Olympic derivative on a power day.
    olympic: bool,
    upper: bool,
    strength_goal: bool,
}

impl<'a> DoseCtx<'a> {
    fn new(
        goal: LiftGoal,
        ta: TrainingAge,
        emphasis: DupDay,
        exercise: &'a str,
        anchors: &'a LiftAnchors,
        weekly_sets: u8,
        day: &LiftDay,
    ) -> Self {
        // A >8 wk layoff's pre-layoff anchors are already set aside by
        // `build_plan_anchors` (KB Table 3.4b: treat as novice), so an anchor
        // here is always one the athlete may load from.
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
        DoseCtx {
            exercise,
            anchors,
            ta,
            goal,
            day_goal,
            emphasis,
            weekly_sets,
            day: *day,
            load: strength::loading_rx(day_goal).value,
            anchored,
            dup,
            // Isolation lifts have unreliable e1RMs (File 02 strength-006; the
            // SAME `is_isolation_lift` predicate that gates the anchor and the
            // DUP primary), so on a DUP day they are prescribed in their own
            // hypertrophy rep window by RIR rather than undulated at %1RM off
            // an untrustworthy anchor.
            is_isolation: dup && is_isolation_lift(exercise),
            olympic: power_day && anchored && is_olympic_derivative(exercise),
            upper: is_upper_body(exercise),
            strength_goal: matches!(goal, LiftGoal::MaxStrength | LiftGoal::Power),
        }
    }

    /// The arm that prescribes this lift.
    fn dose(&self) -> LiftDose {
        let power_goal_day = self.day_goal == LiftGoal::Power;
        if self.olympic {
            olympic_dose()
        } else if self.dup {
            self.dup_dose()
        } else if self.anchored && power_goal_day {
            self.novice_power_dose()
        } else if self.anchored && self.load.pct_1rm.0 > 0 {
            self.novice_linear_dose()
        } else if power_goal_day {
            calibration_dose()
        } else if let Some(rir) = self.load.rir {
            self.unanchored_rir_dose(rir)
        } else {
            // A band with neither a %1RM floor nor a RIR (only the Power band,
            // whose power days are handled above): the calibration set again.
            calibration_dose()
        }
    }

    /// Rest comes from the band the day ACTUALLY prescribes: a Heavy/Power DUP
    /// day rests per its emphasis (180 s), never per the goal band minimum. A
    /// hypertrophy-emphasis set rests per the EXERCISE CLASS (HYP-REST-002:
    /// 120 s heavy compounds, 90 s otherwise, catalog-unknown treated as a
    /// moderate compound), not the 30 s floor of the hypertrophy loading band:
    /// a heavy compound cannot hold ≥90% of its first-set reps on 30 s.
    fn rest_sec(&self) -> u16 {
        let hypertrophy_emphasis = if self.dup {
            self.emphasis == DupDay::Hypertrophy
        } else {
            self.day_goal == LiftGoal::Hypertrophy
        };
        if self.is_isolation {
            hypertrophy::rest_sec_for(crate::exercise::ExerciseClass::Isolation)
                .value
                .0
        } else if self.olympic {
            strength::olympic_derivative_rx().value.rest_sec.0
        } else if hypertrophy_emphasis {
            hypertrophy::rest_sec_for(
                crate::exercise::exercise_entry(self.exercise)
                    .map(|e| e.class)
                    .unwrap_or(crate::exercise::ExerciseClass::ModerateCompound),
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
        let LiftDay {
            n_exposures,
            week_in_block,
            block_weeks,
            ..
        } = self.day;
        // Bound the volume sizing by THIS lift's primary-muscle landmarks (MEV
        // floor / MRV ceiling); an exercise outside Table 4 falls back to the
        // training-age band inside `sized_sets`.
        let landmarks = hypertrophy::landmarks_for_exercise(self.exercise);
        let sizing_week = if self.strength_goal { 1 } else { week_in_block };
        let sized = sized_sets(
            self.weekly_sets,
            self.ta,
            n_exposures,
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
                hypertrophy::rep_range_for(crate::exercise::ExerciseClass::Isolation)
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
        let week = earned_week(self.anchors, self.exercise, self.day.week_in_block);
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
            let week = earned_week(self.anchors, self.exercise, self.day.week_in_block);
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
            hypertrophy::rir_for_week(self.day.week_in_block, self.day.block_weeks)
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
/// it is a calibration set the athlete can execute: work up to one set of 5
/// at RPE 8 (a 3–6 rep test set, STR-E1RM-CHECK-001). Logging it anchors every
/// later load. Never "50 % of an unknown e1RM".
fn calibration_dose() -> LiftDose {
    let (lo, hi) = strength::e1rm_test_set_reps().value;
    LiftDose {
        claim: Some(claims::STR_E1RM_CHECK_001),
        ..LiftDose::new(
            LiftIntensity::Rpe(strength::rir_to_rpe(f64::from(CALIBRATION_RIR)) as f32),
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
fn earned_week(anchors: &LiftAnchors, exercise: &str, week_in_block: u8) -> u8 {
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
fn novice_jump_cap_pct(anchors: &LiftAnchors, exercise: &str, reps: u8, upper: bool) -> Option<u8> {
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
/// loaded last session, or when it was not completed. Novice linear
/// progression adds load after a completed session and deloads a lift only
/// after repeated misses (File 08 indiv-008/009; STR-PCTPROG-001 "per
/// successful week"), but a set done at its target effort cannot raise the
/// estimate the percentage reads, and each %1RM load floors onto the plate
/// grid: without this hold the load would slide one plate at a time across
/// completed weeks. The percentage is rounded UP so the plate-grid floor of
/// anchor × % lands on the load lifted.
fn novice_hold_pct(anchors: &LiftAnchors, exercise: &str, reps: u8) -> Option<u8> {
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
/// 2 reps in reserve (RPE 8; inside the 3–6 rep test-set window,
/// STR-E1RM-CHECK-001, and the 1–3 RIR strength band, STR-INTENT-001).
const CALIBRATION_REPS: u8 = 5;
const CALIBRATION_RIR: u8 = 2;

/// Power-clean-type optimum on a power day: ≥70 % 1RM (Soriano 2015,
/// STR-POWER-SELECT-001), the low end of that optimum.
const OLYMPIC_POWER_PCT: u8 = 70;

/// Reps per set of the power-day ballistic opener.
const BALLISTIC_REPS: u8 = 5;

/// The ballistic opener's sets: the KB's low-dose plyometric band floor, 40
/// foot contacts as 8 × 5 (strength-032 / PLYO-001: low-dose 40–60 also
/// effective; well inside every level's cap).
fn ballistic_sets() -> u8 {
    let contacts = strength::plyo_low_dose_contacts().value.0;
    (contacts / BALLISTIC_REPS as u16).clamp(1, u8::MAX as u16) as u8
}

/// The ballistic opener's exercise: a bodyweight countermovement jump or, once
/// a non-novice has a squat e1RM of ≥1.5× bodyweight (PLYO-PREREQ-001, safety
/// gate), a low-box depth jump.
fn ballistic_exercise(profile: &Profile, anchors: &LiftAnchors, ta: TrainingAge) -> &'static str {
    if ta != TrainingAge::Novice && squat_benchmark_met(profile, anchors) {
        "Depth jump (low box)"
    } else {
        "Vertical jump"
    }
}

/// The ballistic opener of a power day (STR-POWER-SELECT-001):
/// [`ballistic_exercise`] × [`ballistic_sets`], with 2 min between sets
/// (STR-PLYO-SCHED-001: 2–3 min). One power day per 3 lift days in the
/// `dup_emphasis` order (one per 2 in the running-led alternating layout)
/// keeps jumps ≤3×/wk.
fn ballistic_item(
    profile: &Profile,
    anchors: &LiftAnchors,
    ta: TrainingAge,
) -> Recommended<Prescription> {
    const REST_SEC: u16 = 120;
    Recommended::new(
        Prescription::Lift(LiftPrescription {
            exercise: ballistic_exercise(profile, anchors, ta).to_string(),
            sets: ballistic_sets(),
            reps: BALLISTIC_REPS,
            intensity: LiftIntensity::MaxIntent,
            rest_sec: REST_SEC,
            velocity_loss_pct: None,
            reps_max: None,
            load_kg: None,
        }),
        claims::STR_POWER_SELECT_001,
    )
}
