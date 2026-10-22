//! Hypertrophy goal end to end: logged sets → last working sets → double
//! progression on the card; delivered sets per muscle on the program card; the
//! volume planner agrees with the plan.

use super::*;
use crate::schema::{LiftIntensity, Prescription};

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
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(hyp_profile()), &mut model)
        .expect_only_render();
    for (i, &(kg, reps, rpe)) in sets.iter().enumerate() {
        app.update(
            set_at("Lat pulldown", kg, reps, rpe, MON - 1, i as i64),
            &mut model,
        )
        .expect_only_render();
    }
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: MON,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
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
    let vm = planned_with(&[(60.0, 10, 8.0), (60.0, 10, 8.0), (60.0, 10, 8.0)]);
    let it = pulldown(&vm);
    assert_eq!(it.load_kg, Some(60.0), "{it:?}");
    assert_eq!((it.reps_low, it.reps_high), (11, 12));
    assert!(it.why.basis.contains("same weight"), "{}", it.why.basis);
}

#[test]
fn last_working_takes_the_top_load_sets_of_the_newest_session() {
    let mk = |kg: f64, reps: u32, day: i64, i: i64| LoggedSet {
        exercise: "Incline DB curl".into(),
        weight_kg: kg,
        reps,
        rpe: 8.0,
        observed_at: day * DAY_SEC + i * 120,
        entry_id: 0,
        hold_sec: None,
    };
    let sets = [
        mk(10.0, 15, 98, 0), // older session, ignored
        mk(12.0, 14, 100, 0),
        mk(12.0, 13, 100, 1),
        mk(8.0, 20, 100, 2), // lighter back-off set, not a working set
    ];
    let refs: Vec<&LoggedSet> = sets.iter().collect();
    let lw = build_last_working(&refs, false, 101, 0);
    assert_eq!(lw.len(), 1);
    assert_eq!(lw[0].weight_kg, 12.0);
    assert_eq!(lw[0].sets, vec![(14, 8.0), (13, 8.0)]);
    assert_eq!(lw[0].age_days, 1);
    // Past the 56-day horizon it drops out.
    assert!(build_last_working(&refs, false, 100 + 57, 0).is_empty());
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
    // No Table 4 exercise trains the rear delts: the note says so plainly.
    assert!(
        prog.muscle_volume_note.contains("rear delts"),
        "{}",
        prog.muscle_volume_note
    );
}

#[test]
fn planner_ramp_equals_plan_ramp() {
    let p = hyp_profile();
    let ta = individualization::training_age_from_cadence(p.progression_cadence).value;
    let bw = crate::plan::lift_block_position(&p, 0).block_weeks;
    let (rows, _) = build_hypertrophy_plan(
        &HypertrophyPlanQuery {
            muscle: "chest".into(),
            weeks: bw,
            not_growing: false,
            recovering_easily: false,
        },
        Some(&p),
        None,
    );
    let days = crate::plan::week_shape(&p).lift_days;
    let plan_ramp: Vec<String> = (1..=bw)
        .map(|w| {
            crate::plan::hypertrophy_week_targets(&p, ta, w, bw, days)
                .iter()
                .find(|t| t.muscle == "chest")
                .unwrap()
                .target
                .to_string()
        })
        .collect();
    let expected = format!("Weekly set ramp: {}", plan_ramp.join(" → "));
    assert!(
        rows.iter().any(|r| r.summary.starts_with(&expected)),
        "planner must show the plan's ramp {expected:?}: {:?}",
        rows.iter().map(|r| &r.summary).collect::<Vec<_>>()
    );
    // Week 1 opens at the athlete's own 12 sets fitted to the 32-set day: 12
    // sets for every muscle over 3 full-body days packs past 32 a day, so the
    // plan (and the planner with it) trims it to 11, never down to MEV 10.
    assert_eq!(plan_ramp[0], "11");
    assert!(
        rows.iter()
            .any(|r| r.summary.contains("trimmed so no lift day")),
        "{:?}",
        rows.iter().map(|r| &r.summary).collect::<Vec<_>>()
    );
    // The RIR row is the plan's own 1–3 schedule (never 4 for a non-novice).
    let rir = rows
        .iter()
        .find(|r| r.summary.starts_with("Reps left by week:"))
        .expect("RIR row");
    assert!(!rir.summary.contains('4'), "{}", rir.summary);
    assert!(rir.summary.ends_with('1'), "{}", rir.summary);
}

fn range_rx(reps: u8, load_kg: f32) -> Recommended<Prescription> {
    graded(
        Prescription::Lift(crate::schema::LiftPrescription {
            exercise: "Lateral raise".into(),
            sets: 3,
            reps,
            intensity: LiftIntensity::Rir(2),
            rest_sec: 60,
            velocity_loss_pct: None,
            reps_max: Some(20),
            load_kg: Some(load_kg),
        }),
        claims::HYP_DOUBLEPROG_001,
    )
}

fn last_raise(kg: f64, reps: u32) -> crate::plan::Anchors {
    crate::plan::Anchors {
        last_working: vec![crate::plan::LastWorking {
            exercise: "Lateral raise".into(),
            weight_kg: kg,
            sets: vec![(reps, 0.0); 3],
            age_days: 3,
        }],
        ..Default::default()
    }
}

#[test]
fn a_held_dumbbell_load_is_carried_exactly_as_logged() {
    // A4: 6 kg × 13 → hold 6 kg for 14; never floored onto the 2.5 kg grid
    // (5 kg) while the card says "same weight as last time".
    let it = flatten_prescription(&range_rx(14, 6.0), &last_raise(6.0, 13), None, false);
    assert_eq!(it.load_kg, Some(6.0));
    assert!(it.why.basis.contains("same weight"), "{}", it.why.basis);
    // A 15 lb dumbbell stays 15 lb for a lb profile.
    let w = 15.0 / LB_PER_KG;
    let it = flatten_prescription(&range_rx(14, w as f32), &last_raise(w, 13), None, true);
    assert_eq!(it.load_kg, Some(w));
}

#[test]
fn a_light_load_steps_up_by_at_most_ten_percent() {
    // B1: the double-progression step is the smallest step capped at
    // STR-LOADPROG-001's +10 %, so a 6 kg raise goes to 6.6 kg, not 8.5 kg.
    assert!((crate::hypertrophy::load_step_kg(6.0) - 0.6).abs() < 1e-9);
    assert_eq!(crate::hypertrophy::load_step_kg(40.0), 2.5);
    let it = flatten_prescription(&range_rx(12, 6.6), &last_raise(6.0, 20), None, false);
    let kg = it.load_kg.expect("a stepped load");
    assert!(kg > 6.0 && kg <= 6.6 + 1e-6, "{kg}");
    assert!(it.why.basis.contains("goes up"), "{}", it.why.basis);
}
