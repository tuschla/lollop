//! Hypertrophy goal end to end: logged sets → last working sets → double
//! progression on the card; delivered sets per muscle on the program card; the
//! volume planner agrees with the plan.

use super::*;

fn hyp_profile() -> Profile {
    Profile {
        lift_goal: LiftGoal::Hypertrophy,
        ..base_profile()
    }
}

/// Set `i` of a session on epoch-day `day`, three minutes apart from 01:00.
fn set_at(exercise: &str, weight_kg: f64, reps: u32, rpe: f64, day: i64, i: i64) -> Event {
    log_set(
        exercise,
        weight_kg,
        reps,
        rpe,
        day * DAY_SEC + 3_600 + i * 180,
    )
}

/// Plan a hypertrophy week starting Monday `MON` after logging `sets` the day
/// before, and return the view.
fn planned_with(sets: &[(f64, u32, f64)]) -> ViewModel {
    let history = sets
        .iter()
        .enumerate()
        .map(|(i, &(kg, reps, rpe))| set_at("Lat pulldown", kg, reps, rpe, MON - 1, i as i64));
    let (app, model) = planned_on(hyp_profile(), history, MON);
    app.view(&model)
}

fn pulldown(vm: &ViewModel) -> PrescriptionView {
    vm.week_plan
        .iter()
        .flat_map(|s| s.items.iter())
        .find(|i| i.exercise == "Lat pulldown")
        .cloned()
        .expect("the logged pulldown is in the week")
}

#[test]
fn hypertrophy_card_adds_load_after_top_of_range() {
    // 3×12 at 60 kg, RPE 7 (3 RIR = week 1's target) on an 8–12 machine
    // compound → next session 62.5 kg, back to 8 reps.
    let vm = planned_with(&[(60.0, 12, 7.0), (60.0, 12, 7.0), (60.0, 12, 7.0)]);
    let it = pulldown(&vm);
    assert_eq!(it.load_kg, Some(62.5), "{it:?}");
    assert_eq!((it.reps_low, it.reps_high), (8, 12));
    assert!(it.summary.contains("8–12"), "{}", it.summary);
    assert!(it.why.basis.contains("goes up"), "{}", it.why.basis);
}

#[test]
fn hypertrophy_card_holds_load_below_top() {
    // 3×10 at RPE 7, week 1's 3-RIR target: same load, one rep more.
    let vm = planned_with(&[(60.0, 10, 7.0), (60.0, 10, 7.0), (60.0, 10, 7.0)]);
    let it = pulldown(&vm);
    assert_eq!(it.load_kg, Some(60.0), "{it:?}");
    assert_eq!((it.reps_low, it.reps_high), (11, 12));
    assert!(it.why.basis.contains("same weight"), "{}", it.why.basis);
}

#[test]
fn last_working_takes_the_top_load_sets_of_the_newest_session() {
    // The card carries the newest session's top-load sets (60 kg × 10, 9):
    // not the older session's 50 kg, not the lighter back-off set.
    let sets = [
        (50.0, 10, 7.0, POS_MON - 3),
        (60.0, 10, 7.0, POS_MON - 1),
        (60.0, 9, 7.0, POS_MON - 1),
        (40.0, 15, 7.0, POS_MON - 1),
    ];
    let it = hyp_card(hyp_profile(), "Lat pulldown", &sets, POS_MON);
    assert_eq!(it.load_kg, Some(60.0));
    assert_eq!(it.anchored_on, "Last: 60 kg × 10, 9");
    // A month-old last session after weeks without any lifting is carried at
    // the 4–8 wk re-entry 70 % (60 → 42 → 40 kg); a day older the card no
    // longer carries a load at all.
    let carried = |back: i64| {
        hyp_card(
            hyp_profile(),
            "Lat pulldown",
            &[(60.0, 10, 7.0, POS_MON - back)],
            POS_MON,
        )
        .load_kg
    };
    assert_eq!(carried(30), Some(40.0));
    assert_eq!(carried(31), None);
}

#[test]
fn program_card_shows_delivered_sets_per_muscle() {
    let vm = planned_with(&[]);
    let prog = vm.program.expect("program card");
    let row = |m: &str| {
        prog.muscle_volume
            .iter()
            .find(|r| r.muscle == m)
            .unwrap_or_else(|| panic!("{m} row: {:?}", prog.muscle_volume))
            .clone()
    };
    for m in [
        "Chest",
        "Back",
        "Quads",
        "Side delts",
        "Biceps",
        "Calves",
        "Triceps",
    ] {
        let r = row(m);
        assert!(r.target_sets > 0, "{r:?}");
        assert!(
            (r.delivered_sets - f32::from(r.target_sets)).abs() <= 1.0,
            "{r:?}"
        );
        assert!(r.label.contains("target"), "{r:?}");
    }
    // Pressing counts half a set toward the triceps, and the card says so.
    assert!(
        row("Triceps").label.contains("from compound lifts"),
        "{:?}",
        row("Triceps")
    );
    // No Table 4 exercise trains the rear delts, and the note says so.
    assert!(
        prog.muscle_volume_note.contains("rear delts"),
        "{}",
        prog.muscle_volume_note
    );
}

#[test]
fn planner_ramp_equals_plan_ramp() {
    // The meso planner for the plan's own block shows the ramp the plan runs:
    // week 1 opens at the athlete's own 12 sets fitted to the 32-set day. 12
    // sets for every muscle over 3 full-body days packs past 32 a day, so the
    // plan (and the planner with it) trims it to 11, never down to MEV 10.
    let (app, mut model) = planned_on(hyp_profile(), [], MON);
    let program = app.view(&model).program.expect("program card");
    let chest = program
        .muscle_volume
        .iter()
        .find(|r| r.muscle == "Chest")
        .expect("a chest row")
        .target_sets;
    app.update(
        Event::PlanHypertrophyMeso {
            muscle: "chest".into(),
            weeks: program.weeks_total,
            not_growing: false,
            recovering_easily: false,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert_eq!(vm.hypertrophy_ramp, [11, 11, 12, 12, 13, 13]);
    assert_eq!(
        chest, vm.hypertrophy_ramp[0],
        "the plan's week 1 is the planner's"
    );
    let rows: Vec<&str> = vm
        .hypertrophy_plan
        .iter()
        .map(|r| r.summary.as_str())
        .collect();
    assert!(
        rows.iter().any(|r| r
            .starts_with("Weekly set ramp: 11 → 11 → 12 → 12 → 13 → 13 (trimmed so no lift day")),
        "{rows:?}"
    );
    // The RIR row is the plan's own 1–3 schedule (never 4 for a non-novice).
    let rir = rows
        .iter()
        .find(|r| r.starts_with("Reps left by week:"))
        .expect("RIR row");
    assert!(!rir.contains('4'), "{rir}");
    assert!(rir.ends_with('1'), "{rir}");
}

/// The first `exercise` card of a hypertrophy week planned from `start` after
/// `sets` of it, each `(kg, reps, rpe, epoch day)`.
fn hyp_card(
    profile: Profile,
    exercise: &str,
    sets: &[(f64, u32, f64, i64)],
    start: i64,
) -> PrescriptionView {
    let history = sets
        .iter()
        .enumerate()
        .map(|(i, &(kg, reps, rpe, day))| set_at(exercise, kg, reps, rpe, day, i as i64));
    let (app, model) = planned_on(profile, history, start);
    app.view(&model)
        .week_plan
        .into_iter()
        .flat_map(|s| s.items)
        .find(|i| i.exercise == exercise)
        .unwrap_or_else(|| panic!("a {exercise} card"))
}

#[test]
fn a_held_dumbbell_load_is_carried_exactly_as_logged() {
    // 6 kg × 13 → hold 6 kg for 14; never floored onto the 2.5 kg grid
    // (5 kg) while the card says "same weight as last time".
    let it = hyp_card(
        hyp_profile(),
        "Lateral raise",
        &[(6.0, 13, 7.0, MON - 1)],
        MON,
    );
    assert_eq!(it.load_kg, Some(6.0));
    assert!(it.why.basis.contains("same weight"), "{}", it.why.basis);
    // A 15 lb dumbbell stays 15 lb for a lb profile.
    let lb = Profile {
        weight_lb: true,
        ..hyp_profile()
    };
    let w = 15.0 / LB_PER_KG;
    let it = hyp_card(lb, "Lateral raise", &[(w, 13, 7.0, MON - 1)], MON);
    assert_eq!(it.load_kg, Some(w));
    assert!(it.why.basis.contains("at 15 lb"), "{}", it.why.basis);
}

#[test]
fn a_light_load_steps_up_by_at_most_ten_percent() {
    // The double-progression step is the smallest step capped at
    // STR-LOADPROG-001's +10 %, so a 6 kg raise goes to 6.6 kg, not 8.5 kg.
    let it = hyp_card(
        hyp_profile(),
        "Lateral raise",
        &[(6.0, 20, 7.0, MON - 1)],
        MON,
    );
    let kg = it.load_kg.expect("a stepped load");
    assert!(kg > 6.0 && kg <= 6.6 + 1e-6, "{kg}");
    assert!(it.why.basis.contains("goes up"), "{}", it.why.basis);
}

#[test]
fn an_e1rm_drop_cut_on_a_rep_range_also_leaves_one_more_rep() {
    // autoreg-006 ("cap session at planned RPE − 1 and reduce top-set load
    // ~5%", extracted/06-autoregulation.md:134) on a double-progression item:
    // 60 kg × 7 at RPE 10 after two sessions of 60 kg × 8 at RPE 8 reads
    // 74 kg, 7.5 % under the 80 kg estimate. Seven reps to failure are four at
    // week 1's 3 reps left, so the card holds 60 kg (no re-derive); the cut
    // takes it × 0.95, floored to 55 kg, with one more rep in reserve (3 → 4:
    // RPE 7 → 6), and the card says why and cites AUTOREG-E1RM-GATE-001. The
    // plan starts on a Monday with dated history.
    let sets = [
        (60.0, 8, 8.0, POS_MON - 10),
        (60.0, 8, 8.0, POS_MON - 8),
        (60.0, 7, 10.0, POS_MON - 1),
    ];
    let it = hyp_card(hyp_profile(), "Bench press", &sets, POS_MON);
    assert_eq!((it.load_kg, it.target_rpe), (Some(55.0), Some(6.0)));
    assert!(
        it.evidence.citation.contains("Helms"),
        "{:?}",
        it.evidence.citation
    );
    assert!(
        it.why.basis.contains("under your estimate"),
        "{}",
        it.why.basis
    );
}

#[test]
fn a_load_re_derived_from_a_dropped_session_is_not_cut_again() {
    // 60 kg × 5 at RPE 10 after two sessions of 60 kg × 8 at RPE 8: an e1RM
    // drop (70 kg against 80 kg), and five reps to failure are two at week 1's
    // 3 reps left, too far under 6–10 to hold 60 kg, so the load is re-derived
    // from that session: 70 kg × 76 % ≈ 53 kg. That is already more than the
    // ~5 % cut under the last load (autoreg-006, 57 kg), so it is not cut again
    // to 50 kg; the drop still leaves one more rep in reserve (RPE 6).
    let sets = [
        (60.0, 8, 8.0, POS_MON - 10),
        (60.0, 8, 8.0, POS_MON - 8),
        (60.0, 5, 10.0, POS_MON - 1),
    ];
    let it = hyp_card(hyp_profile(), "Bench press", &sets, POS_MON);
    assert_eq!((it.load_kg, it.target_rpe), (Some(52.5), Some(6.0)));
    assert!(
        it.why.basis.contains("worked out for this rep range"),
        "{}",
        it.why.basis
    );
}

/// The plan start of the deload followers.
const DELOAD_START: i64 = DAY0;

fn four_day_hyp_profile() -> Profile {
    Profile {
        lift_goal: LiftGoal::Hypertrophy,
        concurrent_goal: ConcurrentGoal::Hypertrophy,
        weekly_sets: 16,
        lift_days_per_week: Some(4),
        age_years: Some(29.0),
        ..base_profile()
    }
}

/// An athlete who follows the plan exactly: each day's loads, reps and effort.
struct Follower {
    m: Model,
    id: u64,
    last_kg: std::collections::BTreeMap<String, f64>,
}

impl Follower {
    /// Profile in force from two weeks before [`DELOAD_START`], one set of each
    /// lift logged three days before it, and a plan from [`DELOAD_START`].
    fn new(profile: Profile, lifts: &[(&str, f64)]) -> Self {
        let mut f = Follower {
            m: Model::default(),
            id: 0,
            last_kg: Default::default(),
        };
        f.send(set_today_ev(DELOAD_START - 14));
        f.send(Event::SetProfile(Profile {
            effective_from_epoch_day: Some(DELOAD_START - 14),
            ..profile
        }));
        for (i, (ex, kg)) in lifts.iter().enumerate() {
            let at = (DELOAD_START - 3) * DAY_SEC + 64_800 + 200 * i as i64;
            f.send(log_set(ex, *kg, 5, 8.0, at));
        }
        f.send(set_today_ev(DELOAD_START));
        f.send(Event::GeneratePlan {
            start_epoch_day: DELOAD_START,
        });
        f
    }

    fn send(&mut self, mut ev: Event) {
        self.id += 1;
        if let Event::LogSet { entry_id, .. } = &mut ev {
            *entry_id = self.id;
        }
        let _ = Engine.update(ev, &mut self.m);
    }

    /// Follow the plan exactly on day `d` (loads, reps, target effort).
    fn train(&mut self, d: i64) {
        self.send(set_today_ev(d));
        let Some(ns) = Engine
            .view(&self.m)
            .next_session
            .filter(|s| s.epoch_day == d)
        else {
            return;
        };
        let mut t = d * DAY_SEC + 17 * 3_600;
        for it in ns.items.iter().filter(|i| !i.is_run()) {
            let rpe = it.target_rpe.unwrap_or(8.0);
            let reps = u32::from(it.reps_high.max(it.reps_low).max(1));
            let kg = it
                .load_kg
                .or(it.suggested_load_kg)
                .or_else(|| self.last_kg.get(&it.exercise).copied())
                .unwrap_or(20.0);
            self.last_kg.insert(it.exercise.clone(), kg);
            for _ in 0..it.sets.max(1) {
                self.send(log_set(&it.exercise, kg, reps, rpe, t));
                t += i64::from(it.rest_sec.max(60));
            }
        }
    }

    /// The first prescribed load of `ex` in this week's plan.
    fn first_load(&self, ex: &str) -> Option<f64> {
        Engine
            .view(&self.m)
            .week_plan
            .iter()
            .flat_map(|r| r.items.iter())
            .filter(|it| it.exercise == ex)
            .find_map(|it| it.load_kg)
    }
}

/// HYP-DELOAD-RX-001 / autoreg-022: "Remove plan" and a new plan the week
/// after the deload keep the deload's light sets out of the anchors
/// (`Model::past_plans`), so the new block starts from the working loads.
#[test]
fn a_remade_plan_still_skips_the_last_deload() {
    let mut f = Follower::new(
        four_day_hyp_profile(),
        &[
            ("Back Squat", 100.0),
            ("Bench Press", 75.0),
            ("Deadlift", 130.0),
            ("Overhead Press", 45.0),
            ("Barbell Row", 65.0),
        ],
    );
    // Weeks 1–6: five load weeks and the planned deload (plan week index 5).
    let mut week5 = std::collections::BTreeMap::new();
    for d in DELOAD_START..DELOAD_START + 42 {
        f.train(d);
        if (DELOAD_START + 28..DELOAD_START + 35).contains(&d) {
            week5.extend(f.last_kg.clone());
        }
    }
    let monday = DELOAD_START + 42;
    f.send(set_today_ev(monday));
    let kept = f.first_load("Bench Press").expect("bench on the kept plan");
    f.send(Event::ClearPlan);
    f.send(Event::GeneratePlan {
        start_epoch_day: monday,
    });
    let fresh = f.first_load("Bench Press").expect("bench on the new plan");
    let before = week5["Bench Press"];
    assert!(
        fresh >= 0.9 * before,
        "bench: {before} kg in the last load week, {kept} kg on the kept plan, \
         {fresh} kg on a plan re-created after the deload"
    );
}

/// A novice (no calendar deload) who becomes an intermediate on the
/// Wednesday of plan week index 5 is prescribed a deload Wednesday–Sunday:
/// those days are deload days under their own profile, so the next block
/// starts from the working loads.
#[test]
fn a_mid_week_profile_change_into_a_deload_still_skips_it() {
    let novice = Profile {
        progression_cadence: crate::individualization::ProgressionCadence::EverySession,
        ..four_day_hyp_profile()
    };
    let mut f = Follower::new(
        novice,
        &[
            ("Bench Press", 75.0),
            ("Back Squat", 100.0),
            ("Barbell Row", 65.0),
        ],
    );
    let wed = DELOAD_START + 35 + 2;
    let mut week5 = std::collections::BTreeMap::new();
    let mut phases = std::collections::BTreeMap::new();
    for d in DELOAD_START..DELOAD_START + 42 {
        if d == wed {
            f.send(set_today_ev(d));
            f.send(Event::SetProfile(Profile {
                effective_from_epoch_day: Some(wed),
                ..four_day_hyp_profile()
            }));
        }
        f.train(d);
        if d == wed || d == wed - 2 {
            phases.insert(d, Engine.view(&f.m).program.map(|p| p.phase));
        }
        if (DELOAD_START + 28..DELOAD_START + 35).contains(&d) {
            week5.extend(f.last_kg.clone());
        }
    }
    let deload = |d: i64| phases.get(&d).copied().flatten() == Some(MesoPhase::Deload);
    assert!(deload(wed) && !deload(wed - 2), "week-5 phases {phases:?}");
    f.send(set_today_ev(DELOAD_START + 42));
    let next = f.first_load("Bench Press").expect("bench next week");
    let before = week5["Bench Press"];
    assert!(
        next >= 0.9 * before,
        "bench {before} kg before the deload → {next} kg after it"
    );
}
