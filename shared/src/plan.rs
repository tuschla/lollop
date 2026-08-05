//! Program synthesis (MIGRATION-PLAN Phase 6 / B3): compose the dormant graded
//! band functions (`strength::dup_day_rx`, `strength::loading_rx`,
//! `hypertrophy::intermediate_default_program`, `running::workout_rx`,
//! `running::goal_week_plan`, `hypertrophy::meso_structure`) into a concrete,
//! dated [`Program`] of `Recommended<Prescription>` for the user's profile.
//!
//! Pure and deterministic: no clock, no IO, no randomness. Time enters `view()`
//! as `observed_at`/`epoch_day` event data; this module only lays out a generic
//! Mon..Sun microcycle at 0-based `day` offsets, `app.rs` maps those offsets to
//! calendar days and applies readiness/safety downstream (HARD RULE 3).
//!
//! HARD RULE 1/2: every prescription is graded with the SAME registry claim id
//! that produced its band (STR-DUP-001, STR-INTENT-001, HYP-DEFAULT-PROG-001,
//! RUN-*). No new training claim is invented; loads are honest arithmetic
//! (%1RM × the user's logged e1RM) computed in `app.rs` at render time. When no
//! e1RM anchor exists the lift is prescribed by RIR, never a fabricated load.
//! The `MarketingMyth` choke point in `Recommended::new` is untouched.

use std::collections::BTreeSet;

use crate::app::Profile;
use crate::hypertrophy;
use crate::individualization::{TrainingAge, training_age_from_cadence};
use crate::running::{self, GoalDistance, RunWorkoutRx};
use crate::schema::{
    ConfidenceTag, Evidence, Goal, LiftIntensity, LiftPrescription, LiftSessionType, MesoPhase,
    Mesocycle, Prescription, Program, Recommended, RunIntensity, RunPrescription, RunSessionType,
    RunVolume, Session, SessionType, VdotBand,
};
use crate::strength::{self, DupDay, LiftGoal};

/// Anchors derived from the user's logged history in `app.rs::view()`. Turns the
/// %1RM bands into real kg and lets the plan lead with the user's OWN exercises
/// (owner quick-pick rule): a lift the user has logged carries an e1RM anchor and
/// is prescribed by load; anything unlogged falls back to RIR.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Anchors {
    /// (exercise name, best e1RM kg) over the trailing window, in the order the
    /// caller prefers (most-recent-first). Case-insensitive matched on name.
    pub lift_e1rm: Vec<(String, f64)>,
    /// Longest COMPLETED run (km) in the trailing 30-day window, derived in
    /// `app.rs::build_run_anchors` from `model.runs`. `None` when no run has been
    /// logged in the window. This is the running analogue of `lift_e1rm`: just as
    /// a logged e1RM anchors a lift's %1RM load to demonstrated capacity, this
    /// anchors the long run to the athlete's demonstrated recent distance.
    ///
    /// Window + safety rationale: it uses the SAME 30-day window and predicate as
    /// the `RUN-SPIKE-001` spike baseline (`spike_baseline_km`). Prescribing a
    /// long run AT this value is 0 % over the 30-day longest run = no
    /// single-session spike (RUN-SPIKE-001 flags only >10 % over that baseline),
    /// so anchoring the long run here is safe by the same rule that governs
    /// progression BEYOND it. KB: RUN-SPIKE-001 (Frandsen et al. 2025, BJSM).
    pub longest_recent_run_km: Option<f64>,
    /// Measured average weekly running volume (km) = total km run in the trailing
    /// 28 days ÷ 4, derived in `app.rs::build_run_anchors`. `None` when no run has
    /// been logged in the window. A measured FACT about the athlete (not a
    /// recommendation), used only as the *input volume* to the existing KB-cited
    /// long-run share rule, never as a claim itself. KB: RUN-LONGRUN-001
    /// (running-016 single-run ≤25 % of weekly volume).
    pub recent_weekly_km: Option<f64>,
}

impl Anchors {
    /// Best logged e1RM for an exercise, if any (case-insensitive). B3: only a
    /// POSITIVE e1RM counts as an anchor, a 0/negative value never anchors a
    /// load (it would render "@ 0 kg").
    pub fn e1rm_for(&self, exercise: &str) -> Option<f64> {
        self.lift_e1rm
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(exercise))
            .map(|(_, v)| *v)
            .filter(|v| *v > 0.0)
    }
}

/// Grade a synthesized value with an existing registry claim (HARD RULE 2). The
/// claim id is always one the band function already carried, never invented.
fn graded<T>(value: T, claim_id: &str) -> Recommended<T> {
    let c = crate::evidence::claim(claim_id).expect("known plan claim id");
    Recommended::new(value, c.to_evidence(), c.to_confidence_tag())
}

/// Grade with an explicit evidence/confidence pair reused from a band function.
fn graded_with<T>(value: T, evidence: Evidence, confidence: ConfidenceTag) -> Recommended<T> {
    Recommended::new(value, evidence, confidence)
}

/// The evidence-cited weekly program for this profile, anchored to logged data.
///
/// Returns `None` when the profile raises any onboarding gate (HARD RULE 3 -
/// the engine never programs through a medical deferral) or when the profile
/// describes neither lifting nor running (nothing to plan).
pub fn synthesize(profile: &Profile, anchors: &Anchors, _start_epoch_day: i64) -> Option<Program> {
    if profile.health.any_gate() {
        return None;
    }
    let ta = training_age_from_cadence(profile.progression_cadence).value;
    // `weekly_sets` (planned weekly sets PER MUSCLE, the guided-setup answer) only
    // gates whether lifting is programmed at all; it deliberately does NOT resize
    // the per-session `sets` band. A KB-honest wire from a per-muscle weekly target
    // to per-EXERCISE per-session sets would need a muscle→exercise volume-split
    // model (which muscles each catalog lift trains, and a weekly frequency to
    // divide by) that the knowledge base does not provide (HARD RULE 1). Absent
    // that, sets stay on the graded `loading_rx`/`dup_day_rx` bands; `weekly_sets`
    // is a presence flag only. (`age_years` is likewise unused here, display/
    // prefill-only per the Profile doc, owner-scope, no rule branches on it.)
    let lifting = profile.weekly_sets > 0;
    let running = profile.running_days_per_week > 0;
    if !lifting && !running {
        return None;
    }

    let mut sessions: Vec<Session> = Vec::new();

    // Lift days on Mon/Wed/Fri (pure) or Mon/Thu (hybrid, to leave room for the
    // running load, the concurrent lower-lift cap the guidance already states).
    let n_lift: usize = if !lifting {
        0
    } else if running {
        2
    } else {
        3
    };
    let lift_days: Vec<u16> = match n_lift {
        3 => vec![0, 2, 4],
        2 => vec![0, 3],
        _ => vec![],
    };
    let lift_names = main_lifts(anchors);
    for (i, &day) in lift_days.iter().enumerate() {
        sessions.push(lift_session(profile, anchors, ta, &lift_names, i, day));
    }

    // Running days on the remaining slots, long run late in the week.
    if running {
        let weekly_km = effective_weekly_km(profile, anchors);
        let longest_recent = anchors.longest_recent_run_km;
        let slots = run_week_slots(profile, &lift_days, weekly_km);
        let n_run_days = slots.len();
        for (day, kind) in slots {
            sessions.push(run_session(
                kind,
                day,
                weekly_km,
                longest_recent,
                n_run_days,
            ));
        }
    }

    // Everything else is a planned rest day (a real session in the week strip).
    let used: BTreeSet<u16> = sessions.iter().map(|s| s.day).collect();
    for d in 0u16..7 {
        if !used.contains(&d) {
            sessions.push(Session {
                session_type: SessionType::Rest,
                day: d,
                prescriptions: Vec::new(),
            });
        }
    }
    sessions.sort_by_key(|s| s.day);

    let (goal, name) = program_goal_name(profile, lifting, running);
    // Accumulation block shape from the KB meso structure (HYP-MESO-STRUCT-001).
    let meso = hypertrophy::meso_structure().value;
    let weeks = meso.accumulation_weeks.0.max(1);
    Some(Program {
        id: "current".into(),
        name,
        goal,
        mesocycles: vec![Mesocycle {
            phase: MesoPhase::Build,
            weeks,
            sessions,
        }],
    })
}

/// The top-level goal + program name shown in the summary card.
fn program_goal_name(profile: &Profile, lifting: bool, running: bool) -> (Goal, String) {
    match (lifting, running) {
        (true, true) => (Goal::Hybrid, "Hybrid - strength + running".into()),
        (true, false) => match profile.lift_goal {
            LiftGoal::MaxStrength => (Goal::Strength, "Strength block".into()),
            LiftGoal::Power => (Goal::Power, "Power block".into()),
            LiftGoal::Hypertrophy => (Goal::Hypertrophy, "Hypertrophy block".into()),
        },
        (false, true) => match profile.goal_distance {
            GoalDistance::General | GoalDistance::C25k => {
                (Goal::GeneralEndurance, "Running base".into())
            }
            GoalDistance::FiveK => (Goal::RunningRace { distance_km: 5.0 }, "5K plan".into()),
            GoalDistance::TenK => (Goal::RunningRace { distance_km: 10.0 }, "10K plan".into()),
            GoalDistance::HalfMarathon => (
                Goal::RunningRace { distance_km: 21.1 },
                "Half-marathon plan".into(),
            ),
            GoalDistance::Marathon => (
                Goal::RunningRace { distance_km: 42.2 },
                "Marathon plan".into(),
            ),
        },
        (false, false) => (Goal::GeneralEndurance, "Training plan".into()),
    }
}

/// Main lift slots, leading with the user's own logged exercises (each carrying
/// an e1RM anchor → a load), filled with File-03 catalog defaults (→ RIR) when
/// history is thin. Up to 3 to keep the hero card readable.
fn main_lifts(anchors: &Anchors) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (name, _) in &anchors.lift_e1rm {
        if !out.iter().any(|n| same_movement(n, name)) {
            out.push(name.clone());
        }
        if out.len() >= 3 {
            break;
        }
    }
    // File-03 catalog compound names (exercise_entry-known) as fallbacks. Use
    // `same_movement` (not exact-case) so a logged "Back squat" isn't scheduled
    // AGAIN as the catalog "Barbell back squat", the same lift twice on one day.
    for def in ["Barbell back squat", "Bench press", "Romanian deadlift"] {
        if out.len() >= 3 {
            break;
        }
        if !out.iter().any(|n| same_movement(n, def)) {
            out.push(def.to_string());
        }
    }
    out.truncate(3);
    out
}

/// Whether two exercise names denote the same movement for dedup purposes.
/// Conservative: case-insensitive after stripping EQUIPMENT qualifiers only
/// (barbell/dumbbell/kettlebell/cable/machine/smith) and collapsing whitespace,
/// so "Back squat" ≡ "Barbell back squat" and "Cable row" ≡ "Row", while distinct
/// movements stay separate. Only equipment words are stripped, never POSITIONAL
/// / movement modifiers ("front"/"back"/"romanian"/"overhead"), because those
/// distinguish genuinely different lifts (Front vs Back squat, Deadlift vs
/// Romanian deadlift). Consequence: a bare "Squat" vs "Back squat" still counts
/// as two (they could be different lifts); closing that safely needs a KB-backed
/// movement-synonym lexicon, which the KB does not provide (HARD RULE 1), so it is
/// left as a rare cosmetic dup rather than risk false-merging distinct lifts.
fn same_movement(a: &str, b: &str) -> bool {
    fn norm(name: &str) -> String {
        let mut s = name.to_ascii_lowercase();
        for equip in ["barbell", "dumbbell", "kettlebell", "cable", "machine", "smith"] {
            s = s.replace(equip, "");
        }
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }
    norm(a) == norm(b)
}

/// The DUP emphasis for a lift day, GOAL-AWARE (M12). The classic DUP week
/// undulates Heavy → Power → Hypertrophy (File 02 strength-023), but a hybrid
/// athlete has only 2 lift days, so a naive `[Heavy, Power, Hypertrophy][i % 3]`
/// gives a HYPERTROPHY-goal lifter Heavy + Power and ZERO 6–12-rep work. Lead
/// the undulation with the day the athlete's GOAL calls for, so a hypertrophy
/// goal always gets at least one hypertrophy-range (8–12 rep) day even at 2 lift
/// days; a max-strength / power goal keeps the classic order unchanged.
fn dup_emphasis(goal: LiftGoal, day_index: usize) -> DupDay {
    let base = match goal {
        LiftGoal::Hypertrophy => [DupDay::Hypertrophy, DupDay::Heavy, DupDay::Power],
        // MaxStrength and Power keep the original ordering (byte-identical).
        LiftGoal::MaxStrength | LiftGoal::Power => {
            [DupDay::Heavy, DupDay::Power, DupDay::Hypertrophy]
        }
    };
    base[day_index % 3]
}

/// The loading goal a DUP emphasis targets, so its inter-set REST comes from the
/// band the day actually prescribes (M12). File 02 strength-023's Heavy day is a
/// max-strength quality (85–90 %), the Power day a power quality (50–70 % fast),
/// the Hypertrophy day a hypertrophy quality (70–75 %), each has its own rest
/// (`loading_rx`): 180–300 s for heavy/power, 30–90 s for hypertrophy. Attaching
/// the *goal's* band minimum to every day (the old bug) put 30 s rest on 85 %
/// triples for a hypertrophy-goal lifter, wrong data.
fn emphasis_goal(emphasis: DupDay) -> LiftGoal {
    match emphasis {
        DupDay::Heavy => LiftGoal::MaxStrength,
        DupDay::Power => LiftGoal::Power,
        DupDay::Hypertrophy => LiftGoal::Hypertrophy,
    }
}

/// Build one lift day. Intermediate+ undulate via `dup_day_rx`; novices run a
/// linear `loading_rx` day. An anchored lift is prescribed by %1RM; an unlogged
/// one by RIR (never an invented load, HARD RULE 1).
fn lift_session(
    profile: &Profile,
    anchors: &Anchors,
    ta: TrainingAge,
    lift_names: &[String],
    day_index: usize,
    day: u16,
) -> Session {
    let goal = profile.lift_goal;
    let emphasis = dup_emphasis(goal, day_index);
    let lift_type = match emphasis {
        DupDay::Heavy => LiftSessionType::MaxEffort,
        DupDay::Power => LiftSessionType::DynamicEffort,
        DupDay::Hypertrophy => LiftSessionType::Repetition,
    };
    // DUP undulates the SAME primary lift across the week (Mon heavy, Wed power,
    // Fri hypertrophy, File 02 strength-023), so every lift day leads with the
    // user's top lift (the anchored one, when logged); a rotating accessory fills
    // the second slot.
    let mut prescriptions = Vec::new();
    let primary = &lift_names[0];
    prescriptions.push(lift_prescription(goal, ta, emphasis, primary, anchors));
    if lift_names.len() >= 2 {
        let rest = &lift_names[1..];
        let secondary = &rest[day_index % rest.len()];
        prescriptions.push(lift_prescription(goal, ta, emphasis, secondary, anchors));
    }
    Session {
        session_type: SessionType::Lift(lift_type),
        day,
        prescriptions,
    }
}

/// One lift's evidence-cited prescription. DUP (%1RM) when anchored + non-novice;
/// linear `loading_rx` (%1RM anchored, RIR unanchored) otherwise.
fn lift_prescription(
    goal: LiftGoal,
    ta: TrainingAge,
    emphasis: DupDay,
    exercise: &str,
    anchors: &Anchors,
) -> Recommended<Prescription> {
    let anchored = anchors.e1rm_for(exercise).is_some();
    let dup = anchored && ta != TrainingAge::Novice;
    let load = strength::loading_rx(goal);
    // Rest comes from the band the day ACTUALLY prescribes (M12): a DUP day rests
    // per its emphasis (Heavy/Power = 180 s, Hypertrophy = 30 s), never per the
    // goal band minimum. A linear (non-DUP) day IS the goal's `loading_rx`, so
    // its rest is that band's.
    let rest_sec = if dup {
        strength::loading_rx(emphasis_goal(emphasis)).value.rest_sec.0
    } else {
        load.value.rest_sec.0
    };

    let (intensity, sets, reps) = if dup {
        // Daily-undulating heavy/power/hypertrophy day (STR-DUP-001): higher
        // sets, lower reps, %1RM at the conservative low end of the band.
        let d = strength::dup_day_rx(emphasis);
        (
            LiftIntensity::PercentOneRm(d.value.pct_1rm.0 as f32),
            d.value.sets.1,
            d.value.reps.0,
        )
    } else if anchored && load.value.pct_1rm.0 > 0 {
        // Novice linear day, anchored → %1RM at the conservative LOW end of the
        // band (B1: never the ceiling; matches the DUP path's low-end choice).
        (
            LiftIntensity::PercentOneRm(load.value.pct_1rm.0 as f32),
            load.value.sets.0,
            load.value.reps.1,
        )
    } else {
        // No anchor (or a band whose low end is 0 %, e.g. the Power band's
        // (0, 95): never prescribe "@ 0 kg", B3) → RIR at the conservative HIGH
        // end of the band (more reps in reserve = the safer end; B1: was the
        // aggressive `rir.0`).
        (
            LiftIntensity::Rir(load.value.rir.1),
            load.value.sets.0,
            load.value.reps.1,
        )
    };

    let pres = Prescription::Lift(LiftPrescription {
        exercise: exercise.to_string(),
        sets,
        reps,
        intensity,
        rest_sec,
        tempo: None,
        velocity_loss_pct: None,
    });

    // Evidence travels with the claim that produced the band actually used.
    if dup {
        graded(pres, "STR-DUP-001")
    } else {
        graded_with(pres, load.evidence.clone(), load.confidence.clone())
    }
}

/// Effective weekly running volume (km) for plan synthesis. Normally
/// `max(profile.running_km_per_week, anchors.recent_weekly_km)`: the stated value
/// is a static guided-setup heuristic (days × level-km) that never updates; the
/// anchor is the athlete's MEASURED 28-day average. Measured volume is a fact,
/// not a recommendation, so feeding it into the existing KB-cited long-run share
/// rule (RUN-LONGRUN-001, running-016) is legitimate: the higher of the two is
/// taken so a runner who logs more than their profile claims gets a plan sized to
/// reality, while a runner who logs less than their (aspirational) profile is
/// still served their intended volume (the profile is a floor).
///
/// M13 exception, post-layoff decay: when the profile says the athlete is
/// RETURNING from a break (`weeks_off = Some(w > 0)`, the REENTRY-001 signal),
/// the stated figure is a stale PRE-layoff (aspirational) number and must NOT
/// floor the re-entry plan: a 40 km/wk profile after a month off should not
/// keep prescribing a 40 km/wk-sized plan. In that case the MEASURED reality
/// governs when it is known. With no logged runs to measure yet, there is no
/// data to decay toward, so the stated figure still stands (the graduated
/// re-entry ramp itself is REENTRY-001's job in `app.rs`, out of plan scope).
fn effective_weekly_km(profile: &Profile, anchors: &Anchors) -> f64 {
    let stated = profile.running_km_per_week.max(0.0);
    let returning = profile.weeks_off.map_or(false, |w| w > 0.0);
    match anchors.recent_weekly_km {
        // Re-entry: measured mileage overrides a stale pre-layoff figure even
        // when it is LOWER (the profile stops being a floor after a break).
        Some(measured) if returning => measured,
        // Normal: the profile is a floor; measured wins only when it is higher.
        Some(measured) if measured > stated => measured,
        _ => stated,
    }
}

/// Allocate running days onto the week's free slots (not a lift day), placing the
/// long run late in the week and the quality sessions mid-week. Session counts +
/// quality budget come from `running::goal_week_plan` (RUN-WORKOUT-001).
fn run_week_slots(
    profile: &Profile,
    lift_days: &[u16],
    weekly_km: f64,
) -> Vec<(u16, RunSessionType)> {
    let gwp = running::goal_week_plan(profile.goal_distance, profile.advanced).value;
    let free: Vec<u16> = (0u16..7).filter(|d| !lift_days.contains(d)).collect();
    let want = (profile.running_days_per_week as usize)
        .min(gwp.sessions_per_week.1 as usize)
        .min(free.len())
        .max(1);
    // Evenly pick `want` days from the free slots.
    let chosen: Vec<u16> = if want >= free.len() {
        free.clone()
    } else {
        let step = free.len() as f64 / want as f64;
        (0..want)
            .map(|i| free[((i as f64 + 0.5) * step) as usize % free.len()])
            .collect::<BTreeSet<u16>>()
            .into_iter()
            .collect()
    };
    let n = chosen.len();

    // Quality (hard) budget: honour the cited running-024 goal table
    // (`quality_per_week`, e.g. 2 for 5K/10K/HM) wherever the run-day count
    // physically allows it, rather than the old `n/3` heuristic that capped a
    // 4-run 5K week at 1 quality while the table it cites says 2. The only
    // structural limit is reserving the long run + at least one EASY day (80/20
    // polarization: the app's own coaching): so `quality <= n - 2`, which also
    // keeps a 2-run week at long + easy (never long + tempo, zero easy).
    let hard_budget = gwp.quality_per_week.1 as usize;
    let max_quality = n.saturating_sub(2);
    let quality = hard_budget.min(max_quality);

    // Compose the session TYPES: 1 long + `quality` quality days + easy fill.
    let mut kinds: Vec<RunSessionType> = Vec::with_capacity(n);
    kinds.push(RunSessionType::LongRun);
    for q in 0..quality {
        // First quality slot = Tempo (threshold). A second quality slot is a
        // VO2max Interval ONLY when the weekly volume actually SUPPORTS >=3 reps
        // within the cited <=8% weekly-volume cap (M11): below that the interval
        // rep-floor would break its own RUN-INTERVAL-001 cap, so substitute an
        // easy (Recovery) run instead of emitting a cap-violating interval.
        let k = if q == 0 {
            RunSessionType::Tempo
        } else if interval_supported(weekly_km) {
            RunSessionType::Interval
        } else {
            RunSessionType::Recovery
        };
        kinds.push(k);
    }
    while kinds.len() < n {
        kinds.push(RunSessionType::Recovery);
    }

    // Place: long run on the LAST chosen day; SPREAD the quality (hard) days
    // across the remaining days so two hard sessions never land back-to-back
    // (80/20 polarization), easy (Recovery) fills the gaps.
    let mut days_sorted = chosen.clone();
    days_sorted.sort_unstable();
    let long_day = *days_sorted.last().unwrap();
    let rest_days: Vec<u16> = days_sorted
        .iter()
        .copied()
        .filter(|d| *d != long_day)
        .collect();
    let n_rest = rest_days.len();

    let quality_kinds: Vec<RunSessionType> =
        kinds.iter().copied().filter(|k| is_quality(*k)).collect();
    let mut slots: Vec<RunSessionType> = vec![RunSessionType::Recovery; n_rest];
    if !quality_kinds.is_empty() && n_rest > 0 {
        let step = n_rest as f64 / quality_kinds.len() as f64;
        let mut used: BTreeSet<usize> = BTreeSet::new();
        for (i, qk) in quality_kinds.iter().enumerate() {
            let mut pos = (((i as f64 + 0.5) * step) as usize).min(n_rest - 1);
            while used.contains(&pos) {
                pos = (pos + 1) % n_rest;
            }
            used.insert(pos);
            slots[pos] = *qk;
        }
    }

    let mut out: Vec<(u16, RunSessionType)> = vec![(long_day, RunSessionType::LongRun)];
    out.extend(rest_days.iter().copied().zip(slots.iter().copied()));
    out
}

/// A hard (quality) run day, Tempo or Interval. The long run is easy-paced
/// (E→E+) and does not count as quality here.
fn is_quality(kind: RunSessionType) -> bool {
    matches!(kind, RunSessionType::Tempo | RunSessionType::Interval)
}

/// Natural VO2max-interval rep count the weekly volume supports BEFORE any
/// rep-floor clamp: `floor(<=8% weekly-volume cap ÷ mid rep distance)`
/// (RUN-INTERVAL-001). This is the honest count the cap allows.
fn interval_reps_natural(weekly_km: f64) -> i64 {
    let s = running::vo2max_interval_rx().value;
    let rep_dist_m = ((s.rep_distance_m.0 as f64 + s.rep_distance_m.1 as f64) / 2.0).max(1.0);
    (s.weekly_cap_frac * weekly_km * 1000.0 / rep_dist_m).floor() as i64
}

/// True when the weekly volume supports a real VO2max-interval session, i.e.
/// at least 3 reps fit WITHIN the cited <=8% weekly-volume cap (M11). Below this
/// the plan must not prescribe intervals (the rep-floor would exceed the cap the
/// card cites); it substitutes an easy run.
fn interval_supported(weekly_km: f64) -> bool {
    interval_reps_natural(weekly_km) >= 3
}

/// One run day's evidence-cited prescription, translated from `workout_rx` bands.
/// `weekly_km` sizes volume; `longest_recent_km` is the demonstrated-capacity
/// ceiling / spike baseline; `n_run_days` is how many days the week actually
/// runs (for the long run's ≤2×-daily-average guardrail).
fn run_session(
    kind: RunSessionType,
    day: u16,
    weekly_km: f64,
    longest_recent_km: Option<f64>,
    n_run_days: usize,
) -> Session {
    let rx = running::run_workout_rx(kind);
    let pres = Prescription::Run(translate_run(
        kind,
        &rx.value,
        weekly_km,
        longest_recent_km,
        n_run_days,
    ));
    // HARD RULE 2 honesty: cite whichever rule actually BINDS the long-run
    // distance (see `long_run_decision`). When the demonstrated-capacity anchor
    // or the RUN-SPIKE-001 safety ceiling set the figure, cite RUN-SPIKE-001
    // (prescribing at/under the 30-day longest run = no single-session spike);
    // when the 25 % weekly share OR the ≤2×-daily-average guardrail set it, cite
    // RUN-LONGRUN-001 (running-016). The honest basis text, anchored to your
    // longest recent run, share guideline exceeded, is attached in
    // `app.rs::flatten_prescription`, keyed off the RUN-SPIKE-001 claim id.
    // (Precedent: `cap_run_item_easy` re-points evidence to the decision that
    // drove the figure.)
    let session = if kind == RunSessionType::LongRun
        && long_run_decision(weekly_km, longest_recent_km, n_run_days)
            .1
            .cites_spike()
    {
        graded(pres, "RUN-SPIKE-001")
    } else {
        graded_with(pres, rx.evidence.clone(), rx.confidence.clone())
    };
    Session {
        session_type: SessionType::Run(kind),
        day,
        prescriptions: vec![session],
    }
}

/// Conservative target from a `u16` band: the MIDPOINT of a two-sided band (B1:
/// never the ceiling) or the low value of a low-only band. A ceiling-only band
/// gives no defensible interior target (no floor to average against), so it
/// propagates `None`: the caller falls back to a safe default rather than an
/// invented fraction of the ceiling. `None` for an open band.
fn mid_u16(band: (Option<u16>, Option<u16>)) -> Option<u16> {
    match band {
        (Some(lo), Some(hi)) => Some(((lo as u32 + hi as u32) / 2) as u16),
        (Some(lo), None) => Some(lo),
        (None, Some(_)) => None,
        (None, None) => None,
    }
}

/// Conservative target from an `f64` band (km): band midpoint / low value.
fn mid_f64(band: (Option<f64>, Option<f64>)) -> Option<f64> {
    match band {
        (Some(lo), Some(hi)) => Some((lo + hi) / 2.0),
        (Some(lo), None) => Some(lo),
        (None, Some(hi)) => Some(hi * 0.4),
        (None, None) => None,
    }
}

/// Conservative %HRmax target: the LOW end of a two-sided band (B1: never the
/// ceiling); a ceiling-only band's ceiling IS its "stay under" target.
fn hr_low(band: (Option<f64>, Option<f64>)) -> Option<f64> {
    match band {
        (Some(lo), _) => Some(lo),
        (None, Some(hi)) => Some(hi),
        (None, None) => None,
    }
}

/// Conservative intensity: for an HR-governed session use the low end of the
/// %HRmax band; otherwise (pace-governed) the session's VDOT band.
fn conservative_intensity(kind: RunSessionType, rx: &RunWorkoutRx) -> RunIntensity {
    if rx.hr_governed {
        if let Some(lo) = hr_low(rx.pct_hr_max) {
            return RunIntensity::HrPercentMax((lo * 100.0) as f32);
        }
    }
    RunIntensity::Vdot(vdot_band_for(kind))
}

/// Turn a `RunWorkoutRx` band into a concrete `RunPrescription` (B1). Intervals
/// emit per-rep structure (`repeats`), the long run scales to the user's weekly
/// volume, and continuous runs take conservative/midpoint band values, never a
/// band ceiling.
fn translate_run(
    kind: RunSessionType,
    rx: &RunWorkoutRx,
    weekly_km: f64,
    longest_recent_km: Option<f64>,
    n_run_days: usize,
) -> RunPrescription {
    match kind {
        RunSessionType::Interval => interval_prescription(weekly_km),
        RunSessionType::LongRun => {
            long_run_prescription(rx, weekly_km, longest_recent_km, n_run_days)
        }
        _ => {
            // Continuous run (Recovery / Tempo / RacePace / Hills): conservative
            // volume + conservative intensity, no repeats. Any DISTANCE volume is
            // capped at the spike threshold over the demonstrated baseline (H3)
            // so the plan never prescribes a run its own RUN-SPIKE-001 gate would
            // flag as a dangerous progression.
            let capped_km = mid_f64(rx.distance_km).and_then(|km| cap_km_to_spike(km, longest_recent_km));
            let volume = if let Some(km) = capped_km {
                RunVolume::DistanceKm(km as f32)
            } else if let Some(m) = mid_u16(rx.duration_min) {
                // No distance band, OR a positive sub-1 km baseline whose spike
                // ceiling floors below 1 km: fall back to a conservative easy
                // DURATION rather than a sub-1 km distance the gate would flag.
                RunVolume::DurationMin(m)
            } else {
                RunVolume::DurationMin(30)
            };
            RunPrescription {
                volume,
                intensity: conservative_intensity(kind, rx),
                repeats: None,
            }
        }
    }
}

/// Cap a prescribed run DISTANCE (km) at the RUN-SPIKE-001 threshold over the
/// demonstrated 30-day-longest baseline (H3): never prescribe more than 10 % over
/// the athlete's longest recent run, floored to whole km so the shell's
/// `"{:.0} km"` render also stays at/under the ≤10 % gate.
///
/// The log-time RUN-SPIKE-001 gate flags any session >10 % over ANY POSITIVE
/// 30-day longest, so the ceiling binds for any positive baseline, including a
/// sub-1 km post-injury walk-jog history. When the floored ceiling drops below
/// 1 km (a sub-1 km baseline) no whole-km distance can sit at/under the gate, so
/// this returns `None` and the caller falls back to a DURATION prescription
/// rather than emit a sub-1 km (or 0 km) distance the gate would flag and the
/// `"{:.0} km"` render can't honor. With no baseline (`None`, a no-history
/// profile) the distance is unchanged: the volume-derived sizing stands and the
/// log-time flag handles first runs honestly.
fn cap_km_to_spike(km: f64, longest_recent_km: Option<f64>) -> Option<f64> {
    match longest_recent_km {
        Some(baseline) if baseline > 0.0 => {
            let ceiling = (1.10 * baseline).floor();
            (ceiling >= 1.0).then(|| km.min(ceiling))
        }
        _ => Some(km),
    }
}

/// Which KB rule set the long-run distance decides the honest face citation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LongRunBind {
    /// The 25 % weekly-share target governed (running-016). → RUN-LONGRUN-001.
    Volume,
    /// The demonstrated-capacity anchor set the value, within safety bounds. It
    /// exceeds the 25 % share but is 0 % over the 30-day longest run (no spike).
    /// → RUN-SPIKE-001.
    Demonstrated,
    /// The ≤2×-daily-average guardrail (running-016) limited the run BELOW
    /// demonstrated capacity: the athlete's weekly volume can't yet support the
    /// full demonstrated distance every week. → RUN-LONGRUN-001.
    DailyAvgCap,
    /// The RUN-SPIKE-001 safety ceiling (≤10 % over the demonstrated longest run)
    /// capped a higher volume/capacity want. → RUN-SPIKE-001.
    SpikeCeiling,
}

impl LongRunBind {
    /// Whether the binding rule is the single-session spike rule (RUN-SPIKE-001)
    /// rather than the ≤25 % share / ≤2×-daily-average guardrails (RUN-LONGRUN-001).
    fn cites_spike(self) -> bool {
        matches!(self, LongRunBind::Demonstrated | LongRunBind::SpikeCeiling)
    }
}

/// Decide the long-run distance (km, floored) and which KB rule binds it (H3/H4).
///
/// The demonstrated longest recent run is a capacity CEILING, not a weekly
/// target: it may justify a long run ABOVE the plain 25 % weekly share, but only
/// up to the KB's own guardrails -
///  - `RUN-LONGRUN-001` ≤25 % weekly share: `floor(0.25 × weekly_km)`, the base;
///  - `RUN-LONGRUN-001` ≤2×-daily-average (running-016): the long run must not
///    exceed `2 × (weekly_km / run days)`, so the FULL demonstrated distance is
///    never prescribed weekly when it dwarfs current volume;
///  - `RUN-SPIKE-001` ≤10 % over the 30-day longest run: a hard safety ceiling on
///    the whole thing, so the plan never prescribes a run its own log-time spike
///    gate would flag.
///
/// With no logged run history (`longest_recent_km == None`) this is byte-identical
/// to the old volume-only rule: `floor(0.25 × weekly_km)`, bind = Volume.
fn long_run_decision(
    weekly_km: f64,
    longest_recent_km: Option<f64>,
    n_run_days: usize,
) -> (f64, LongRunBind) {
    let volume_share = (0.25 * weekly_km).floor().max(0.0);
    let Some(baseline) = longest_recent_km.filter(|d| *d > 0.0) else {
        // Log-less profile: volume-only rule, unchanged.
        return (volume_share, LongRunBind::Volume);
    };
    // The log-time RUN-SPIKE-001 gate flags any session >10 % over ANY POSITIVE
    // 30-day longest, so the spike ceiling must bind for any positive baseline -
    // including a sub-1 km post-injury walk-jog history the demonstrated-capacity
    // path below exempts. Floored on the floored baseline to match the gate's
    // whole-km basis and the shell render (identical to the old `(1.10 × dem)`
    // for a >=1 km baseline, since `dem = baseline.floor()`).
    let spike_ceiling = (1.10 * baseline.floor()).floor();

    // Demonstrated CAPACITY (a ceiling that may RAISE the run above the 25 %
    // share) is only meaningful at >=1 km: a sub-1 km run demonstrates no
    // multi-km capacity. Below it the 25 % share stands, but the spike ceiling
    // (0 km here) still caps a run the log-time gate would flag → RUN-SPIKE-001.
    if baseline < 1.0 {
        return if spike_ceiling < volume_share {
            (spike_ceiling, LongRunBind::SpikeCeiling)
        } else {
            (volume_share, LongRunBind::Volume)
        };
    }
    let dem = baseline.floor();

    // ≤2×-daily-average guardrail (running-016): a bound only when we know the
    // run-day count and have positive volume.
    let daily_avg_cap = if n_run_days > 0 && weekly_km > 0.0 {
        Some((2.0 * weekly_km / n_run_days as f64).floor())
    } else {
        None
    };

    // Demonstrated capacity may raise the run above the share, bounded by the
    // daily-average guardrail.
    let raised_by_capacity = match daily_avg_cap {
        Some(cap) => dem.min(cap),
        None => dem,
    };
    let want = volume_share.max(raised_by_capacity);
    // Hard spike ceiling over the whole thing.
    let target = want.min(spike_ceiling);

    let bind = if target < want {
        // The spike ceiling limited a higher volume/capacity want.
        LongRunBind::SpikeCeiling
    } else if raised_by_capacity > volume_share {
        // Demonstrated capacity set the value above the 25 % share.
        match daily_avg_cap {
            Some(cap) if dem > cap => LongRunBind::DailyAvgCap,
            _ => LongRunBind::Demonstrated,
        }
    } else {
        LongRunBind::Volume
    };
    (target, bind)
}

fn long_run_prescription(
    rx: &RunWorkoutRx,
    weekly_km: f64,
    longest_recent_km: Option<f64>,
    n_run_days: usize,
) -> RunPrescription {
    let (target_km, _) = long_run_decision(weekly_km, longest_recent_km, n_run_days);
    let volume = if target_km >= 1.0 {
        RunVolume::DistanceKm(target_km as f32)
    } else {
        // Very low weekly volume (<4 km/wk) AND no demonstrated run: a 1 km floor
        // would EXCEED 25% and contradict the card's own running-016 cap citation.
        // Prescribe a short easy duration instead of a cap-violating distance.
        RunVolume::DurationMin(30)
    };
    RunPrescription {
        volume,
        intensity: conservative_intensity(RunSessionType::LongRun, rx),
        repeats: None,
    }
}

/// VO2max intervals emitted as PER-REP structure (B1: not a whole-session
/// 100%-HRmax block). Rep duration/distance are the KB band midpoints; the rep
/// COUNT is unstated in the KB (HARD RULE 1), so it is DERIVED from the stated
/// <=8% weekly-volume cap and the mid rep distance (honest arithmetic), clamped
/// 3-8. Intensity is the Interval VDOT band (interval work is pace-governed -
/// HR lags, so no %HRmax target).
fn interval_prescription(weekly_km: f64) -> RunPrescription {
    let s = running::vo2max_interval_rx().value;
    let rep_min = ((s.rep_duration_min.0 + s.rep_duration_min.1) / 2).max(1);
    // Only ever reached when `interval_supported(weekly_km)` (>=3 natural reps
    // fit within the <=8% cap, M11), so the lower clamp never forces a
    // cap-violating floor; the upper clamp (8) only trims below the cap.
    let reps = interval_reps_natural(weekly_km).clamp(3, 8) as u8;
    RunPrescription {
        volume: RunVolume::DurationMin(rep_min.saturating_mul(reps as u16)),
        intensity: RunIntensity::Vdot(VdotBand::Interval),
        repeats: Some((reps, RunVolume::DurationMin(rep_min))),
    }
}

/// Map a session type to its Daniels VDOT band (used only when the KB gives no
/// %HRmax ceiling for that session type).
fn vdot_band_for(kind: RunSessionType) -> VdotBand {
    match kind {
        RunSessionType::Recovery | RunSessionType::LongRun => VdotBand::Easy,
        RunSessionType::Tempo => VdotBand::Threshold,
        RunSessionType::Interval | RunSessionType::Hills => VdotBand::Interval,
        RunSessionType::Repetition | RunSessionType::Strides => VdotBand::Repetition,
        RunSessionType::RacePace => VdotBand::Marathon,
    }
}

/// A representative registry claim id for the whole program (drives the summary
/// card's evidence chip). Always the dominant branch's own claim.
pub fn summary_claim(profile: &Profile) -> &'static str {
    let lifting = profile.weekly_sets > 0;
    let running = profile.running_days_per_week > 0;
    match (lifting, running) {
        (true, _) => {
            let ta = training_age_from_cadence(profile.progression_cadence).value;
            if ta == TrainingAge::Novice {
                match profile.lift_goal {
                    LiftGoal::Power => "STR-PWR-001",
                    _ => "STR-INTENT-001",
                }
            } else {
                "STR-DUP-001"
            }
        }
        (false, true) => "RUN-WORKOUT-001",
        (false, false) => "HYP-MESO-STRUCT-001",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hybrid::ConcurrentGoal;
    use crate::individualization::ProgressionCadence;
    use crate::schema::{Goal, LiftIntensity, Prescription, SessionType};

    fn base_profile() -> Profile {
        Profile {
            progression_cadence: ProgressionCadence::WeekToWeek,
            lift_goal: LiftGoal::MaxStrength,
            goal_distance: GoalDistance::General,
            concurrent_goal: ConcurrentGoal::Strength,
            weekly_sets: 12,
            running_days_per_week: 0,
            running_km_per_week: 0.0,
            advanced: false,
            endurance_intensity_pct_vo2max: 70.0,
            female: false,
            high_load_block: false,
            health: Default::default(),
            environment: None,
            env_temp_c: None,
            env_altitude_m: None,
            weeks_off: None,
            bodyweight_kg: None,
            age_years: None,
            resting_hr_bpm: None,
            measured_hr_max: None,
        }
    }

    fn anchored(name: &str, e1rm: f64) -> Anchors {
        Anchors {
            lift_e1rm: vec![(name.to_string(), e1rm)],
            ..Default::default()
        }
    }

    #[test]
    fn deterministic_same_inputs_same_program() {
        let p = base_profile();
        let a = anchored("Back Squat", 120.0);
        assert_eq!(synthesize(&p, &a, 0), synthesize(&p, &a, 0));
    }

    #[test]
    fn a_gate_blocks_synthesis() {
        let mut p = base_profile();
        p.health.reds_signal = true;
        assert!(synthesize(&p, &Anchors::default(), 0).is_none());
    }

    #[test]
    fn every_prescription_carries_evidence_for_each_goal() {
        for goal in [LiftGoal::MaxStrength, LiftGoal::Power, LiftGoal::Hypertrophy] {
            let mut p = base_profile();
            p.lift_goal = goal;
            let prog = synthesize(&p, &anchored("Back Squat", 100.0), 0).expect("program");
            let mut count = 0;
            for m in &prog.mesocycles {
                for s in &m.sessions {
                    for rx in &s.prescriptions {
                        count += 1;
                        assert!(
                            !rx.evidence.citation.reference.is_empty(),
                            "prescription must carry a citation"
                        );
                    }
                }
            }
            assert!(count > 0, "goal {goal:?} produced no prescriptions");
        }
    }

    #[test]
    fn anchored_lift_is_percent_based_unanchored_is_rir() {
        let p = base_profile();
        // Anchored: the logged lift is prescribed by %1RM.
        let prog = synthesize(&p, &anchored("Back Squat", 100.0), 0).unwrap();
        let anchored_pct = prog.mesocycles[0].sessions.iter().any(|s| {
            s.prescriptions.iter().any(|rx| {
                matches!(
                    &rx.value,
                    Prescription::Lift(l)
                        if l.exercise.eq_ignore_ascii_case("Back Squat")
                            && matches!(l.intensity, LiftIntensity::PercentOneRm(_))
                )
            })
        });
        assert!(anchored_pct, "anchored lift should be %1RM");

        // No anchor at all → every lift is RIR-based (no invented load).
        let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        let any_pct = prog.mesocycles[0].sessions.iter().any(|s| {
            s.prescriptions.iter().any(|rx| {
                matches!(
                    &rx.value,
                    Prescription::Lift(l) if matches!(l.intensity, LiftIntensity::PercentOneRm(_))
                )
            })
        });
        assert!(!any_pct, "with no anchor no lift may carry a %1RM load");
    }

    #[test]
    fn running_profile_yields_a_long_run() {
        let mut p = base_profile();
        p.weekly_sets = 0;
        p.goal_distance = GoalDistance::TenK;
        p.running_days_per_week = 4;
        let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        assert!(matches!(prog.goal, Goal::RunningRace { .. }));
        let has_long = prog.mesocycles[0]
            .sessions
            .iter()
            .any(|s| s.session_type == SessionType::Run(RunSessionType::LongRun));
        assert!(has_long, "a running week must include a long run");
    }

    #[test]
    fn hybrid_profile_has_both_lift_and_run_days() {
        let mut p = base_profile();
        p.goal_distance = GoalDistance::HalfMarathon;
        p.running_days_per_week = 3;
        let prog = synthesize(&p, &anchored("Back Squat", 130.0), 0).unwrap();
        assert_eq!(prog.goal, Goal::Hybrid);
        let sessions = &prog.mesocycles[0].sessions;
        assert!(
            sessions.iter().any(|s| matches!(s.session_type, SessionType::Lift(_))),
            "hybrid needs lift days"
        );
        assert!(
            sessions.iter().any(|s| matches!(s.session_type, SessionType::Run(_))),
            "hybrid needs run days"
        );
    }

    // ── B1: prescriptions must not take band CEILINGS or violate the KB caps ──
    #[test]
    fn b1_tempo_is_not_the_band_ceiling() {
        let mut p = base_profile();
        p.weekly_sets = 0;
        p.goal_distance = GoalDistance::TenK;
        p.running_days_per_week = 5;
        p.running_km_per_week = 50.0;
        let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        for s in &prog.mesocycles[0].sessions {
            if s.session_type == SessionType::Run(RunSessionType::Tempo) {
                let Prescription::Run(r) = &s.prescriptions[0].value else { panic!() };
                // Tempo band is 88–92 %HRmax / 20–40 min: must NOT be the ceiling.
                if let RunIntensity::HrPercentMax(p) = r.intensity {
                    assert!(p < 92.0, "tempo took the 92% HRmax ceiling: {p}");
                    assert!(p >= 88.0, "tempo below the band: {p}");
                }
                if let RunVolume::DurationMin(m) = r.volume {
                    assert!(m < 40, "tempo took the 40-min ceiling: {m}");
                }
            }
        }
    }

    #[test]
    fn b1_long_run_is_not_always_the_150_min_ceiling() {
        let mut p = base_profile();
        p.weekly_sets = 0;
        p.goal_distance = GoalDistance::HalfMarathon;
        p.running_days_per_week = 4;
        p.running_km_per_week = 40.0;
        let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        let long = prog.mesocycles[0]
            .sessions
            .iter()
            .find(|s| s.session_type == SessionType::Run(RunSessionType::LongRun))
            .unwrap();
        let Prescription::Run(r) = &long.prescriptions[0].value else { panic!() };
        match r.volume {
            // With weekly km known the long run is a distance <=25% of weekly km.
            RunVolume::DistanceKm(km) => {
                assert!(km as f64 <= 0.25 * p.running_km_per_week + 0.01, "long run over 25% weekly: {km}");
                assert!(km > 0.0);
            }
            RunVolume::DurationMin(m) => assert!(m < 150, "long run took the 150-min ceiling: {m}"),
        }
    }

    #[test]
    fn b1_interval_emits_per_rep_structure() {
        let mut p = base_profile();
        p.weekly_sets = 0;
        p.goal_distance = GoalDistance::FiveK;
        p.running_days_per_week = 6;
        p.running_km_per_week = 60.0;
        p.advanced = true;
        let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        let interval = prog.mesocycles[0]
            .sessions
            .iter()
            .find(|s| s.session_type == SessionType::Run(RunSessionType::Interval));
        if let Some(s) = interval {
            let Prescription::Run(r) = &s.prescriptions[0].value else { panic!() };
            let (reps, _) = r.repeats.expect("intervals must carry per-rep structure");
            assert!(reps >= 3, "at least a few reps: {reps}");
            // Not a whole-session 100 %-HRmax block: interval work is VDOT/pace.
            assert!(
                matches!(r.intensity, RunIntensity::Vdot(_)),
                "intervals are pace-governed, not a 100% HRmax block"
            );
        }
    }

    #[test]
    fn b1_hybrid_two_run_week_includes_an_easy_run() {
        let mut p = base_profile();
        p.goal_distance = GoalDistance::HalfMarathon;
        p.running_days_per_week = 2;
        p.running_km_per_week = 30.0;
        let prog = synthesize(&p, &anchored("Back Squat", 130.0), 0).unwrap();
        let run_kinds: Vec<RunSessionType> = prog.mesocycles[0]
            .sessions
            .iter()
            .filter_map(|s| match s.session_type {
                SessionType::Run(k) => Some(k),
                _ => None,
            })
            .collect();
        assert!(
            run_kinds.contains(&RunSessionType::Recovery),
            "a hybrid week must include an easy (Recovery) run (80/20), got {run_kinds:?}"
        );
    }

    #[test]
    fn b1_unanchored_lift_uses_the_conservative_rir_end() {
        // MaxStrength RIR band is (1, 3); the conservative (safer) end is 3, not
        // the aggressive 1 the old code took.
        let p = base_profile();
        let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        let mut saw = false;
        for s in &prog.mesocycles[0].sessions {
            for rx in &s.prescriptions {
                if let Prescription::Lift(l) = &rx.value {
                    if let LiftIntensity::Rir(n) = l.intensity {
                        saw = true;
                        assert_eq!(n, 3, "RIR must be the conservative high end");
                    }
                }
            }
        }
        assert!(saw, "an unanchored strength plan should prescribe by RIR");
    }

    // ── Run history reactivity (long run anchored to demonstrated capacity) ──

    /// Distance (km) of the long run in a synthesized program, if it is prescribed
    /// as a distance (not a duration fallback).
    fn long_run_km(prog: &Program) -> Option<f64> {
        prog.mesocycles[0]
            .sessions
            .iter()
            .find(|s| s.session_type == SessionType::Run(RunSessionType::LongRun))
            .and_then(|s| match &s.prescriptions[0].value {
                Prescription::Run(r) => match r.volume {
                    RunVolume::DistanceKm(km) => Some(km as f64),
                    RunVolume::DurationMin(_) => None,
                },
                _ => None,
            })
    }

    /// The claim id cited by the long-run prescription in a synthesized program.
    fn long_run_claim(prog: &Program) -> Option<String> {
        prog.mesocycles[0]
            .sessions
            .iter()
            .find(|s| s.session_type == SessionType::Run(RunSessionType::LongRun))
            .and_then(|s| s.prescriptions[0].evidence.citation.claim_id.clone())
    }

    fn half_marathoner() -> Profile {
        let mut p = base_profile();
        p.weekly_sets = 0;
        p.goal_distance = GoalDistance::HalfMarathon;
        p.running_days_per_week = 3;
        // Static guided-setup heuristic, a low 16 km/wk that never updates.
        p.running_km_per_week = 16.0;
        p
    }

    #[test]
    fn a_logged_21k_long_run_is_prescribed_not_a_4k_run() {
        // Profile claims only 16 km/wk, but the athlete has logged 21 km runs on
        // recent weekends AND measures 30 km/wk. The long run must anchor toward
        // demonstrated capacity, not the stale profile heuristic (floor(0.25×16)
        // = 4 km, the old bug). It is bounded by the ≤2×-daily-average guardrail.
        let p = half_marathoner(); // running_days_per_week = 3
        let anchors = Anchors {
            longest_recent_run_km: Some(21.0),
            recent_weekly_km: Some(30.0), // measured > stated 16 → effective 30
            ..Default::default()
        };
        let prog = synthesize(&p, &anchors, 0).unwrap();
        let km = long_run_km(&prog).expect("long run is a distance");
        // Hand-compute (3 run days, effective weekly 30, demonstrated 21):
        //   volume_share = floor(0.25×30) = 7
        //   daily_avg_cap = floor(2×30/3) = 20
        //   spike_ceiling = floor(1.10×21) = 23
        //   raised = min(21, 20) = 20 ; want = max(7, 20) = 20 ; target = 20
        //   demonstrated (21) > daily cap (20) → DailyAvgCap binds → RUN-LONGRUN-001
        assert_eq!(km, 20.0, "bounded by ≤2×-daily-average, not the raw 21 km");
        assert!(km >= 21.0 * 0.9, "definitely not a 4 km long run: {km}");
        assert_eq!(
            long_run_claim(&prog).as_deref(),
            Some("RUN-LONGRUN-001"),
            "the ≤2×-daily-average guardrail binds → RUN-LONGRUN-001"
        );
    }

    #[test]
    fn h4_full_demonstrated_distance_is_not_prescribed_weekly_when_it_dwarfs_volume() {
        // H4 flagship: a 21 km race logged, but only 16 km/wk of running (stated,
        // no higher measurement) over 3 run days. The old rule pinned the long run
        // AT 21 km every week (131% of weekly volume). The demonstrated run is a
        // CEILING, not a weekly target: the ≤2×-daily-average guardrail caps it.
        //   volume_share = floor(0.25×16) = 4
        //   daily_avg_cap = floor(2×16/3) = floor(10.67) = 10
        //   spike_ceiling = floor(1.10×21) = 23
        //   raised = min(21, 10) = 10 ; want = max(4, 10) = 10 ; target = 10
        //   demonstrated (21) > daily cap (10) → DailyAvgCap binds → RUN-LONGRUN-001
        let p = half_marathoner(); // stated 16 km/wk, 3 run days
        let anchors = Anchors {
            longest_recent_run_km: Some(21.0),
            recent_weekly_km: None, // nothing measured → effective = stated 16
            ..Default::default()
        };
        let prog = synthesize(&p, &anchors, 0).unwrap();
        assert_eq!(
            long_run_km(&prog),
            Some(10.0),
            "≤2×-daily-average (10) bounds the long run, not the raw demonstrated 21"
        );
        assert_eq!(
            long_run_claim(&prog).as_deref(),
            Some("RUN-LONGRUN-001"),
            "the guardrail that limited it (≤2×-daily-average) is what the card cites"
        );
    }

    #[test]
    fn a_demonstrated_anchored_long_run_within_bounds_cites_the_spike_rule() {
        // Demonstrated capacity sets the long run above the 25% share but WITHIN
        // the ≤2×-daily-average guardrail → cites RUN-SPIKE-001 (at/under the
        // 30-day longest run = no single-session spike), not the share cap.
        // 40 km/wk over 4 run days, demonstrated 14 km:
        //   volume_share = floor(0.25×40) = 10
        //   daily_avg_cap = floor(2×40/4) = 20
        //   spike_ceiling = floor(1.10×14) = 15
        //   raised = min(14, 20) = 14 ; want = max(10, 14) = 14 ; target = 14
        //   demonstrated (14) <= daily cap (20) → Demonstrated binds → RUN-SPIKE-001
        let mut p = half_marathoner();
        p.running_days_per_week = 4;
        p.running_km_per_week = 40.0;
        let anchors = Anchors {
            longest_recent_run_km: Some(14.0),
            recent_weekly_km: Some(40.0),
            ..Default::default()
        };
        let prog = synthesize(&p, &anchors, 0).unwrap();
        assert_eq!(long_run_km(&prog), Some(14.0), "anchored to the 14 km run");
        assert_eq!(
            long_run_claim(&prog).as_deref(),
            Some("RUN-SPIKE-001"),
            "a demonstrated-anchored long run within bounds cites the spike rule"
        );
    }

    #[test]
    fn a_volume_dominated_long_run_still_cites_the_share_cap() {
        // Volume-set long run (no demonstrated run at all): the face citation
        // stays RUN-LONGRUN-001 exactly as before.
        let mut p = half_marathoner();
        p.running_km_per_week = 60.0;
        let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        assert_eq!(long_run_km(&prog), Some(15.0), "floor(0.25×60) = 15 km");
        assert_eq!(
            long_run_claim(&prog).as_deref(),
            Some("RUN-LONGRUN-001"),
            "a volume-dominated long run keeps the ≤25% share citation"
        );
    }

    #[test]
    fn a_low_demonstrated_run_caps_a_higher_volume_target_at_the_spike_ceiling() {
        // H3: measured volume wants a 15 km long run (25% of 60 km/wk), but the
        // athlete's longest recent run is only 12 km: prescribing 15 would be a
        // +25% single-session spike over the 12 km baseline. The RUN-SPIKE-001
        // ceiling caps it and re-points the citation.
        // 60 km/wk, 3 run days, demonstrated 12 km:
        //   volume_share = floor(0.25×60) = 15
        //   daily_avg_cap = floor(2×60/3) = 40
        //   spike_ceiling = floor(1.10×12) = 13
        //   raised = min(12, 40) = 12 ; want = max(15, 12) = 15 ; target = min(15,13) = 13
        //   target (13) < want (15) → SpikeCeiling binds → RUN-SPIKE-001
        let mut p = half_marathoner();
        p.running_km_per_week = 60.0; // 3 run days
        let anchors = Anchors {
            longest_recent_run_km: Some(12.0),
            recent_weekly_km: Some(60.0),
            ..Default::default()
        };
        let prog = synthesize(&p, &anchors, 0).unwrap();
        assert_eq!(
            long_run_km(&prog),
            Some(13.0),
            "capped at ≤10% over the 12 km baseline, not the 15 km volume target"
        );
        assert_eq!(
            long_run_claim(&prog).as_deref(),
            Some("RUN-SPIKE-001"),
            "the spike ceiling limited it → cite the spike rule, not the ≤25% share"
        );
    }

    #[test]
    fn no_run_history_is_byte_identical_to_the_volume_only_plan() {
        // Regression: with no run-history anchors, the plan must be exactly what
        // the old volume-only rule produced (long run = floor(0.25 × stated km)).
        let mut p = half_marathoner();
        p.running_km_per_week = 40.0;
        let no_history = Anchors::default();
        let prog = synthesize(&p, &no_history, 0).unwrap();
        // A "manually" volume-only reference: same profile, anchors carrying only
        // lift data would give the identical program.
        let ref_prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        assert_eq!(prog, ref_prog, "log-less plan must be byte-identical");
        // And concretely: floor(0.25 × 40) = 10 km.
        assert_eq!(long_run_km(&prog), Some(10.0));
    }

    #[test]
    fn stale_history_outside_the_window_is_ignored_by_the_caller() {
        // The plan module only sees the anchors the caller (app.rs) derives from
        // the in-window runs. A run outside the 30/28-day window is simply not
        // present in the anchors → the plan falls back to the volume-only rule.
        // (The window filtering itself is covered in app.rs tests.)
        let mut p = half_marathoner();
        p.running_km_per_week = 40.0;
        let stale_ignored = Anchors {
            longest_recent_run_km: None, // stale 21 km run dropped by the window
            recent_weekly_km: None,
            ..Default::default()
        };
        let prog = synthesize(&p, &stale_ignored, 0).unwrap();
        assert_eq!(
            long_run_km(&prog),
            Some(10.0),
            "outside-window history must not raise the long run"
        );
    }

    #[test]
    fn effective_weekly_km_takes_the_higher_of_stated_and_measured() {
        let mut p = half_marathoner();
        p.running_km_per_week = 16.0;
        // Measured > stated → measured wins.
        let more = Anchors {
            recent_weekly_km: Some(48.0),
            ..Default::default()
        };
        assert_eq!(effective_weekly_km(&p, &more), 48.0);
        // Measured < stated → the profile is a floor, stated wins.
        let less = Anchors {
            recent_weekly_km: Some(8.0),
            ..Default::default()
        };
        assert_eq!(effective_weekly_km(&p, &less), 16.0);
        // No measurement → stated.
        assert_eq!(effective_weekly_km(&p, &Anchors::default()), 16.0);
    }

    #[test]
    fn measured_volume_raises_the_long_run_even_without_a_longest_anchor() {
        // Only measured weekly volume is known (e.g. many short logged runs, none
        // individually "long"): the long run scales to measured volume via the
        // 25 % rule, not the stale 16 km profile heuristic.
        let mut p = half_marathoner();
        p.running_km_per_week = 16.0;
        let anchors = Anchors {
            longest_recent_run_km: None,
            recent_weekly_km: Some(60.0),
            ..Default::default()
        };
        let prog = synthesize(&p, &anchors, 0).unwrap();
        // floor(0.25 × 60) = 15, vs the stale floor(0.25 × 16) = 4.
        assert_eq!(long_run_km(&prog), Some(15.0));
    }

    #[test]
    fn a_sub_1km_baseline_never_prescribes_a_run_its_own_spike_gate_would_flag() {
        // A post-injury history of a single 0.5 km walk-jog. The log-time
        // RUN-SPIKE-001 gate (running::single_session_spike) flags any session
        // >1.10×0.5 = 0.55 km. The planner must not hand out a distance the gate
        // then flags: the <1 km floor previously exempted this positive baseline
        // from the ceiling entirely (the mismatch), so a spiking run slipped out.
        let baseline = 0.5;

        // Continuous-run distance cap: no whole-km distance fits at/under 0.55 km,
        // so the cap declines a distance (→ duration fallback), never a spike.
        assert_eq!(
            cap_km_to_spike(5.0, Some(baseline)),
            None,
            "sub-1 km baseline → duration fallback, not a spiking distance"
        );

        // Long-run decision: a stated 40 km/wk wants floor(0.25×40) = 10 km, but
        // the 0.5 km baseline's spike ceiling (floor(1.10×0) = 0 km) caps it.
        let (target, bind) = long_run_decision(40.0, Some(baseline), 3);
        assert_eq!(target, 0.0, "capped by the 0.5 km spike ceiling, not 10 km");
        assert_eq!(bind, LongRunBind::SpikeCeiling);
        assert!(bind.cites_spike(), "honest citation is RUN-SPIKE-001");
        assert!(
            !running::single_session_spike(target, baseline),
            "the long-run target must sit at/under the log-time gate"
        );

        // A >=1 km baseline still caps to a real distance the gate accepts, and a
        // no-history profile leaves the distance untouched (first-run honesty).
        let capped = cap_km_to_spike(5.0, Some(3.0)).expect("3 km baseline → a distance");
        assert_eq!(capped, 3.0, "floor(1.10×3.0) = 3 km");
        assert!(!running::single_session_spike(capped, 3.0));
        assert_eq!(cap_km_to_spike(5.0, None), Some(5.0));
    }

    #[test]
    fn a_high_volume_target_is_still_capped_by_a_low_demonstrated_run() {
        // A high measured volume wants a 20 km long run (25% of 80 km/wk), but the
        // demonstrated longest recent run is only 12 km. The demonstrated run is a
        // capacity CEILING (H3/H4): prescribing 20 would spike +67% over the 12 km
        // baseline, so the RUN-SPIKE-001 ceiling caps it at floor(1.10×12) = 13.
        // (The old max()-only rule handed out the unsafe 20 km here.)
        let mut p = half_marathoner();
        p.running_km_per_week = 16.0; // 3 run days
        let anchors = Anchors {
            longest_recent_run_km: Some(12.0),
            recent_weekly_km: Some(80.0), // 0.25×80 = 20 > 12
            ..Default::default()
        };
        let prog = synthesize(&p, &anchors, 0).unwrap();
        assert_eq!(long_run_km(&prog), Some(13.0));
        assert_eq!(long_run_claim(&prog).as_deref(), Some("RUN-SPIKE-001"));
    }

    #[test]
    fn a_week_is_exactly_seven_days() {
        let prog = synthesize(&base_profile(), &anchored("Back Squat", 100.0), 0).unwrap();
        let days: BTreeSet<u16> = prog.mesocycles[0]
            .sessions
            .iter()
            .map(|s| s.day)
            .collect();
        assert_eq!(days, (0u16..7).collect::<BTreeSet<u16>>());
    }

    // ── Helpers for the M11/M12/LOW placement tests ──────────────────────────

    fn run_kinds_of(prog: &Program) -> Vec<RunSessionType> {
        prog.mesocycles[0]
            .sessions
            .iter()
            .filter_map(|s| match s.session_type {
                SessionType::Run(k) => Some(k),
                _ => None,
            })
            .collect()
    }

    /// (day, kind) of every run session, sorted by day.
    fn run_days_sorted(prog: &Program) -> Vec<(u16, RunSessionType)> {
        let mut v: Vec<(u16, RunSessionType)> = prog.mesocycles[0]
            .sessions
            .iter()
            .filter_map(|s| match s.session_type {
                SessionType::Run(k) => Some((s.day, k)),
                _ => None,
            })
            .collect();
        v.sort_by_key(|(d, _)| *d);
        v
    }

    fn lift_prescriptions(prog: &Program) -> Vec<(LiftSessionType, LiftPrescription)> {
        let mut out = Vec::new();
        for s in &prog.mesocycles[0].sessions {
            if let SessionType::Lift(lt) = s.session_type {
                for rx in &s.prescriptions {
                    if let Prescription::Lift(l) = &rx.value {
                        out.push((lt, l.clone()));
                    }
                }
            }
        }
        out
    }

    fn runner(goal: GoalDistance, days: u8, km: f64) -> Profile {
        let mut p = base_profile();
        p.weekly_sets = 0;
        p.goal_distance = goal;
        p.running_days_per_week = days;
        p.running_km_per_week = km;
        p
    }

    // ── M11: interval rep-floor must not break its own ≤8% cap ────────────────

    #[test]
    fn m11_interval_dropped_below_the_volume_that_supports_three_reps() {
        // 40 km/wk: 3×1200 m = 3.6 km = 9% > the cited 8% cap. The plan must NOT
        // emit an interval here: it substitutes an easy (Recovery) run instead.
        let p = runner(GoalDistance::FiveK, 5, 40.0);
        let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        let kinds = run_kinds_of(&prog);
        assert!(
            !kinds.contains(&RunSessionType::Interval),
            "40 km/wk cannot support 3×1200 within the 8% cap → no interval, got {kinds:?}"
        );
        // The first quality slot (Tempo) still stands; the second is now easy.
        assert!(kinds.contains(&RunSessionType::Tempo), "tempo still prescribed");
        assert!(kinds.contains(&RunSessionType::Recovery), "easy substitute present");
    }

    #[test]
    fn m11_interval_dropped_for_the_degenerate_low_volume_profile() {
        // Self-inconsistent 6-day / 5 km-wk profile: 3×1200 = 72% of weekly. Never
        // emit an interval: the old clamp(3,8) produced exactly this cap breach.
        let p = runner(GoalDistance::FiveK, 6, 5.0);
        let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        assert!(
            !run_kinds_of(&prog).contains(&RunSessionType::Interval),
            "5 km/wk must not get a VO2max interval"
        );
    }

    #[test]
    fn m11_interval_prescribed_and_within_cap_when_volume_supports_it() {
        // 45 km/wk supports exactly 3×1200 = 3.6 km = 8.0% (≤ cap). Interval is
        // prescribed and its total volume respects the ≤8% weekly cap.
        let p = runner(GoalDistance::FiveK, 5, 45.0);
        let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        let interval = prog.mesocycles[0]
            .sessions
            .iter()
            .find(|s| s.session_type == SessionType::Run(RunSessionType::Interval))
            .expect("45 km/wk supports an interval");
        let Prescription::Run(r) = &interval.prescriptions[0].value else { panic!() };
        let (reps, _) = r.repeats.expect("interval carries reps");
        assert!(reps >= 3, "at least 3 reps: {reps}");
        // ≤8% cap honoured: reps × mid-rep-distance (1200 m) ≤ 0.08 × weekly.
        let interval_km = reps as f64 * 1.2;
        assert!(
            interval_km <= 0.08 * 45.0 + 1e-9,
            "interval volume {interval_km} km exceeds the 8% cap"
        );
    }

    // ── M12: DUP must respect lift_goal + rest must match the day prescribed ──

    #[test]
    fn m12_hypertrophy_goal_hybrid_gets_a_hypertrophy_range_day() {
        // Hybrid = 2 lift days. A hypertrophy-goal DUP lifter must get at least one
        // 6–12-rep day (the old [Heavy, Power] cycle gave zero).
        let mut p = runner(GoalDistance::HalfMarathon, 3, 30.0);
        p.weekly_sets = 12;
        p.lift_goal = LiftGoal::Hypertrophy;
        let prog = synthesize(&p, &anchored("Back Squat", 120.0), 0).unwrap();
        let has_hyp_range = lift_prescriptions(&prog)
            .iter()
            .any(|(_, l)| l.reps >= 6 && l.reps <= 12);
        assert!(
            has_hyp_range,
            "a hypertrophy-goal lifter must get at least one 6–12-rep day"
        );
    }

    #[test]
    fn m12_dup_rest_comes_from_the_day_prescribed_not_the_goal_band() {
        // Hypertrophy-goal DUP lifter: the goal band's rest minimum is 30 s, but a
        // Heavy (85–90%) DUP day must rest per the max-strength band (180 s), not
        // 30 s on triples (the cross-wiring bug).
        let mut p = runner(GoalDistance::HalfMarathon, 3, 30.0);
        p.weekly_sets = 12;
        p.lift_goal = LiftGoal::Hypertrophy;
        let prog = synthesize(&p, &anchored("Back Squat", 120.0), 0).unwrap();
        let lifts = lift_prescriptions(&prog);
        // Heavy (MaxEffort) day → 180 s rest (loading_rx(MaxStrength).rest_sec.0).
        let heavy = lifts
            .iter()
            .find(|(lt, _)| *lt == LiftSessionType::MaxEffort)
            .expect("hypertrophy-goal hybrid still includes a heavy day");
        assert_eq!(
            heavy.1.rest_sec, 180,
            "heavy 85–90% triples must rest 180 s, not the hypertrophy 30 s"
        );
        // Hypertrophy (Repetition) day → 30 s rest (loading_rx(Hypertrophy).rest_sec.0).
        let hyp = lifts
            .iter()
            .find(|(lt, _)| *lt == LiftSessionType::Repetition)
            .expect("hypertrophy-goal hybrid includes a hypertrophy day");
        assert_eq!(hyp.1.rest_sec, 30, "hypertrophy day rests per its own band");
    }

    #[test]
    fn m12_strength_goal_emphasis_unchanged() {
        // A max-strength goal keeps the classic Heavy → Power → Hypertrophy order
        // (day 0 = Heavy). Regression guard against the goal-aware change leaking.
        assert_eq!(dup_emphasis(LiftGoal::MaxStrength, 0), DupDay::Heavy);
        assert_eq!(dup_emphasis(LiftGoal::MaxStrength, 1), DupDay::Power);
        assert_eq!(dup_emphasis(LiftGoal::Power, 0), DupDay::Heavy);
        // Hypertrophy leads with a hypertrophy day.
        assert_eq!(dup_emphasis(LiftGoal::Hypertrophy, 0), DupDay::Hypertrophy);
    }

    // ── M13b: post-layoff weekly volume decays to measured reality ───────────

    #[test]
    fn m13_post_layoff_prefers_measured_over_a_stale_stated_volume() {
        // Returning from a 4-week break: stated 40 km/wk is a stale pre-layoff
        // figure. Measured 10 km/wk must govern (the profile stops being a floor).
        let mut p = runner(GoalDistance::TenK, 4, 40.0);
        p.weeks_off = Some(4.0);
        let anchors = Anchors {
            recent_weekly_km: Some(10.0),
            ..Default::default()
        };
        assert_eq!(effective_weekly_km(&p, &anchors), 10.0);

        // Without a layoff the profile is still a floor (measured < stated → stated).
        let mut p2 = p.clone();
        p2.weeks_off = None;
        assert_eq!(effective_weekly_km(&p2, &anchors), 40.0);

        // Returning but nothing logged yet → no data to decay toward → stated.
        assert_eq!(effective_weekly_km(&p, &Anchors::default()), 40.0);
    }

    // ── LOW: quality budget honours the running-024 table + spacing ──────────

    #[test]
    fn low_quality_budget_honours_the_goal_table_at_four_run_days() {
        // running-024 says 2 quality for 5K; at 4 run days there is room for
        // long + 2 quality + 1 easy, so the plan must schedule 2 (not the old n/3=1).
        let p = runner(GoalDistance::FiveK, 4, 50.0);
        let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        let quality = run_days_sorted(&prog)
            .into_iter()
            .filter(|(_, k)| is_quality(*k))
            .count();
        assert_eq!(quality, 2, "5K at 4 run days honours the 2-quality table");
    }

    #[test]
    fn low_quality_days_are_never_back_to_back() {
        // Two hard days must never land on consecutive run days (80/20 spacing).
        let p = runner(GoalDistance::FiveK, 4, 50.0);
        let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
        let runs = run_days_sorted(&prog);
        for w in runs.windows(2) {
            assert!(
                !(is_quality(w[0].1) && is_quality(w[1].1)),
                "quality days back-to-back on run days {} and {}",
                w[0].0,
                w[1].0
            );
        }
    }

    #[test]
    fn low_mid_u16_ceiling_only_band_propagates_none() {
        // A ceiling-only duration band has no defensible interior target: propagate
        // None (caller falls back to a safe default) rather than an invented
        // fraction of the ceiling (the old 0.4×hi).
        assert_eq!(mid_u16((None, Some(150))), None);
        assert_eq!(mid_u16((Some(20), Some(40))), Some(30));
        assert_eq!(mid_u16((Some(20), None)), Some(20));
        assert_eq!(mid_u16((None, None)), None);
    }
}
