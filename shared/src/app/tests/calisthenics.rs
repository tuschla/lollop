//! Calisthenics coaching loop end to end: ladder state replay (regress and
//! re-qualify, exploratory sets, skips, effort gate, re-entry), rung placement,
//! aliases, weighted rungs and the picker views.

use super::*;

/// A Monday ten weeks after the shared fixture's `MON` (epoch day 4). Every
/// history here is logged weeks before "today", and a timestamp at or below 0
/// is an UNDATED legacy row (no session day, no break clock), so the fixture
/// Monday would turn the layoff histories into undated rows. Shadows the
/// glob-imported `MON`.
const MON: i64 = super::MON + 70;

/// [`super::planned_calisthenics`] on this file's later `MON`.
fn planned_calisthenics() -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(calisthenics_profile()), &mut model)
        .expect_only_render();
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
    (app, model)
}

fn place(ladder: Lad, rung: u8, at: i64) -> Event {
    Event::SetCalisthenicsRung {
        ladder,
        rung,
        observed_at: at,
    }
}

fn log_sets(
    app: &Engine,
    model: &mut Model,
    exercise: &str,
    n: usize,
    reps: u32,
    rpe: f64,
    day: i64,
) {
    for _ in 0..n {
        app.update(log_set(exercise, 0.0, reps, rpe, day * DAY_SEC), model)
            .expect_only_render();
    }
}

fn push_item(app: &Engine, model: &Model) -> PrescriptionView {
    cal_item(&app.view(model), Lad::HorizontalPush).expect("a horizontal-push item")
}

#[test]
fn calisthenics_regress_then_requalify_advances_again() {
    let (app, mut model) = planned_calisthenics();
    // Full push-up below its 5-rep floor on 2 sessions → step down, bridged.
    log_sets(&app, &mut model, "Full push-up", 3, 3, 10.0, MON - 6);
    log_sets(&app, &mut model, "Full push-up", 3, 3, 10.0, MON - 5);
    let it = push_item(&app, &model);
    assert_eq!(it.exercise, "Knee push-up");
    assert!(
        it.summary.contains("3 s down + 1 s pause"),
        "tempo bridge: {}",
        it.summary
    );
    // Knee push-up at the top of its band on 2 sessions → Full push-up again
    // (a max-ever anchor would freeze the ladder at Knee push-up).
    log_sets(&app, &mut model, "Knee push-up", 3, 8, 8.0, MON - 3);
    log_sets(&app, &mut model, "Knee push-up", 3, 8, 8.0, MON - 2);
    let it = push_item(&app, &model);
    assert_eq!(
        it.exercise, "Full push-up",
        "the ladder re-qualifies upward"
    );
    assert!(
        !it.summary.contains("pause"),
        "no bridge after stepping back up"
    );
}

#[test]
fn one_exploratory_harder_set_does_not_anchor() {
    let (app, mut model) = planned_calisthenics();
    log_sets(&app, &mut model, "Bodyweight squat", 3, 10, 8.0, MON - 5);
    // One curious pistol attempt, well below its 5-rep floor.
    log_sets(&app, &mut model, "Pistol squat", 1, 2, 10.0, MON - 3);
    let sq = cal_item(&app.view(&model), Lad::Squat).unwrap();
    assert_eq!(
        sq.exercise, "Bodyweight squat",
        "a failed attempt does not move the ladder"
    );
    // A harder rung done for at least its floor IS a self-placement.
    log_sets(
        &app,
        &mut model,
        "Bulgarian split squat",
        1,
        10,
        8.0,
        MON - 2,
    );
    let sq = cal_item(&app.view(&model), Lad::Squat).unwrap();
    assert_eq!(sq.exercise, "Bulgarian split squat");
}

#[test]
fn calisthenics_far_over_band_top_skips_ahead_in_one_session() {
    let (app, mut model) = planned_calisthenics();
    log_sets(&app, &mut model, "Wall push-up", 3, 25, 9.0, MON - 1);
    let it = push_item(&app, &model);
    let rung = it.calis.as_ref().unwrap().rung;
    assert!(
        rung >= 2,
        "3×25 wall push-ups skip at least two steps (got {})",
        it.exercise
    );
}

#[test]
fn straight_arm_rung_never_multi_skips() {
    let (app, mut model) = planned_calisthenics();
    // Very long tuck holds on 1 session: no skip on a straight-arm rung.
    for _ in 0..4 {
        app.update(
            cal_hold("Tuck front lever", 60, 7.0, (MON - 1) * DAY_SEC),
            &mut model,
        )
        .expect_only_render();
    }
    let fl = cal_item(&app.view(&model), Lad::FrontLever).unwrap();
    assert_eq!(fl.exercise, "Tuck front lever");
}

#[test]
fn calisthenics_placement_seeds_starting_rung() {
    let (app, mut model) = planned_calisthenics();
    // Setup placement on day 0 (today): today's item starts there.
    app.update(
        place(Lad::HorizontalPush, 4, MON * DAY_SEC + 60),
        &mut model,
    )
    .expect_only_render();
    let it = push_item(&app, &model);
    assert_eq!(it.exercise, "Full push-up");
    let c = it.calis.clone().unwrap();
    assert_eq!(c.ladder, Lad::HorizontalPush);
    assert_eq!(c.rung, 4);
    assert_eq!(
        c.easier.as_ref().map(|r| r.name.as_str()),
        Some("Knee push-up")
    );
    assert_eq!(
        c.harder.as_ref().map(|r| r.name.as_str()),
        Some("Decline push-up")
    );
    // "Easier" on the item: the same day's item follows the choice.
    app.update(
        place(Lad::HorizontalPush, 3, MON * DAY_SEC + 120),
        &mut model,
    )
    .expect_only_render();
    assert_eq!(push_item(&app, &model).exercise, "Knee push-up");
    // An out-of-range rung is ignored.
    app.update(
        place(Lad::HorizontalPush, 99, MON * DAY_SEC + 180),
        &mut model,
    )
    .expect_only_render();
    assert_eq!(push_item(&app, &model).exercise, "Knee push-up");
    // Placements live in the SETS family: ClearSets clears them.
    app.update(Event::ClearSets, &mut model)
        .expect_only_render();
    assert_eq!(push_item(&app, &model).exercise, "Wall push-up");
}

#[test]
fn placement_event_round_trips_and_compacts_with_the_sets_family() {
    let wire = r#"{"SetCalisthenicsRung":{"ladder":"VerticalPull","rung":4,"observed_at":100}}"#;
    let ev: Event = serde_json::from_str(wire).expect("placement decodes");
    assert_eq!(ev, place(Lad::VerticalPull, 4, 100));
    let kept = crate::log::compact_event_log(vec![
        place(Lad::VerticalPull, 2, 50),
        place(Lad::VerticalPull, 4, 100),
    ]);
    assert_eq!(kept.len(), 2, "every placement is history");
    let cleared =
        crate::log::compact_event_log(vec![place(Lad::VerticalPull, 4, 100), Event::ClearSets]);
    assert!(
        !cleared
            .iter()
            .any(|e| matches!(e, Event::SetCalisthenicsRung { .. })),
        "a later ClearSets supersedes earlier placements: {cleared:?}"
    );
}

#[test]
fn pull_up_alias_counts_for_the_vertical_pull_ladder() {
    let (app, mut model) = planned_calisthenics();
    log_sets(&app, &mut model, "Pull-ups", 3, 5, 8.0, MON - 1);
    let vm = app.view(&model);
    let pull = cal_item(&vm, Lad::VerticalPull).unwrap();
    assert_eq!(
        pull.exercise, "Full pull-up",
        "not Dead hang: the alias anchored the ladder"
    );
    let ladder = vm
        .calisthenics_ladders
        .iter()
        .find(|l| l.ladder == Lad::VerticalPull)
        .unwrap();
    assert_eq!(ladder.current_rung, Some(4));
}

#[test]
fn floor_rung_rpe10_at_top_still_advances() {
    let (app, mut model) = planned_calisthenics();
    // Wall push-up (8–12) is a near-failure rung: RPE 10 at the top is compliance.
    log_sets(&app, &mut model, "Wall push-up", 3, 12, 10.0, MON - 2);
    log_sets(&app, &mut model, "Wall push-up", 3, 12, 10.0, MON - 1);
    assert_eq!(push_item(&app, &model).exercise, "Incline push-up (high)");
}

#[test]
fn unrecorded_rpe_at_band_top_does_not_advance_dynamic_rung() {
    let (app, mut model) = planned_calisthenics();
    log_sets(&app, &mut model, "Full push-up", 3, 8, 0.0, MON - 2);
    log_sets(&app, &mut model, "Full push-up", 3, 8, 0.0, MON - 1);
    let it = push_item(&app, &model);
    assert_eq!(it.exercise, "Full push-up", "an unrated set is not 'clean'");
    assert!(it.calis.as_ref().unwrap().needs_effort_rating);
    assert!(
        it.why.improves.contains("Rate how hard the last set felt"),
        "{}",
        it.why.improves
    );
}

#[test]
fn unrecorded_rpe_two_over_top_advances() {
    let (app, mut model) = planned_calisthenics();
    log_sets(&app, &mut model, "Full push-up", 3, 10, 0.0, MON - 2);
    log_sets(&app, &mut model, "Full push-up", 3, 10, 0.0, MON - 1);
    assert_eq!(push_item(&app, &model).exercise, "Decline push-up");
}

#[test]
fn calisthenics_layoff_steps_rung_down_and_cites_reentry() {
    let (app, mut model) = planned_calisthenics();
    log_sets(&app, &mut model, "Full push-up", 3, 6, 8.0, MON - 40);
    log_sets(&app, &mut model, "Full push-up", 3, 6, 8.0, MON - 38);
    let it = push_item(&app, &model);
    assert_eq!(
        it.exercise, "Knee push-up",
        "5+ weeks off → one step easier"
    );
    assert!(
        it.why.basis.contains("haven't trained this for a while"),
        "{}",
        it.why.basis
    );
    assert_eq!(
        it.evidence.grade,
        Some(EvidenceGrade::ExpertOpinion),
        "cites CAL-REENTRY-001"
    );
}

#[test]
fn straight_arm_layoff_resets_acclimation_window() {
    let (app, mut model) = planned_calisthenics();
    // 3 weekly sessions, a 3-week break, then 4 weekly sessions: the newest
    // session is 56 days after the first, but only 21 days after the return.
    for back in [60, 53, 46, 25, 18, 11, 4] {
        app.update(
            cal_hold("Tuck front lever", 15, 7.0, (MON - back) * DAY_SEC),
            &mut model,
        )
        .expect_only_render();
    }
    let fl = cal_item(&app.view(&model), Lad::FrontLever).unwrap();
    assert_eq!(
        fl.exercise, "Tuck front lever",
        "the tendon window restarted on return"
    );
}

#[test]
fn weighted_rung_view_is_load_capable_and_shows_system_e1rm() {
    let (app, mut model) = planned_calisthenics();
    app.update(
        Event::SetProfile(Profile {
            bodyweight_kg: Some(80.0),
            ..calisthenics_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set("Full pull-up", 10.0, 5, 8.0, (MON - 1) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    let pull = cal_item(&app.view(&model), Lad::VerticalPull).unwrap();
    assert!(pull.calis.as_ref().unwrap().load_capable);
    assert_eq!(pull.added_kg, Some(10.0));
    assert!(
        pull.anchored_on.contains("estimated 1-rep max"),
        "system-load e1RM line: {}",
        pull.anchored_on
    );
    // A pure bodyweight rung is not load-capable.
    let push = push_item(&app, &model);
    assert!(!push.calis.as_ref().unwrap().load_capable);
    assert_eq!(push.added_kg, Some(0.0));
}

#[test]
fn ladder_picker_view_lists_every_ladder_and_greys_out_missing_equipment() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(Profile {
            calis_equipment: Some(CalisEquipment::default()),
            ..calisthenics_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert_eq!(
        vm.calisthenics_ladders.len(),
        crate::calisthenics::LADDERS.len()
    );
    let pull = vm
        .calisthenics_ladders
        .iter()
        .find(|l| l.ladder == Lad::VerticalPull)
        .unwrap();
    assert!(pull.rungs.iter().all(|r| !r.available), "no bar, no rings");
    let push = vm
        .calisthenics_ladders
        .iter()
        .find(|l| l.ladder == Lad::HorizontalPush)
        .unwrap();
    assert_eq!(push.rungs[4].name, "Full push-up");
    assert_eq!(push.rungs[4].target, "5–8 reps");
    assert!(push.rungs.iter().all(|r| r.available));
    assert!(
        vm.lift_audit
            .iter()
            .any(|g| g.summary.contains("No pull-up bar or rings")),
        "the equipment swap is explained"
    );
    // A Weights profile has no ladder list.
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    assert!(app.view(&model).calisthenics_ladders.is_empty());
}

#[test]
fn any_set_under_the_floor_on_two_sessions_regresses() {
    let (app, mut model) = planned_calisthenics();
    // 6/4/3 never completes 3×5 (KB §2.2), on 2 sessions → step down.
    for day in [MON - 6, MON - 5] {
        for reps in [6, 4, 3] {
            app.update(
                log_set("Full push-up", 0.0, reps, 10.0, day * DAY_SEC),
                &mut model,
            )
            .expect_only_render();
        }
    }
    assert_eq!(push_item(&app, &model).exercise, "Knee push-up");
}

#[test]
fn one_bad_session_after_an_advance_does_not_regress() {
    let (app, mut model) = planned_calisthenics();
    log_sets(&app, &mut model, "Full push-up", 3, 8, 8.0, MON - 6);
    log_sets(&app, &mut model, "Full push-up", 3, 8, 8.0, MON - 5);
    let decline = "Decline push-up";
    assert_eq!(push_item(&app, &model).exercise, decline);
    // One missed floor at the new rung: the KB gives it 2 sessions.
    log_sets(&app, &mut model, decline, 3, 3, 10.0, MON - 3);
    assert_eq!(push_item(&app, &model).exercise, decline);
    // The second one steps down.
    log_sets(&app, &mut model, decline, 3, 3, 10.0, MON - 2);
    assert_eq!(push_item(&app, &model).exercise, "Full push-up");
}

#[test]
fn straight_arm_rung_advances_on_two_weekly_test_days_between_working_days() {
    let (app, mut model) = planned_calisthenics();
    // Weekly 15 s test holds (T) between 8 s working-hold days (W), over 28+
    // days: the newest session is a working day, the last two tests passed.
    for (back, sec) in [
        (30, 15),
        (26, 8),
        (23, 15),
        (19, 8),
        (16, 15),
        (12, 8),
        (9, 15),
        (5, 8),
        (2, 8),
    ] {
        app.update(
            cal_hold("Tuck front lever", sec, 7.0, (MON - back) * DAY_SEC),
            &mut model,
        )
        .expect_only_render();
    }
    let fl = cal_item(&app.view(&model), Lad::FrontLever).unwrap();
    assert_eq!(fl.exercise, "Advanced tuck front lever");
}

#[test]
fn harder_mid_session_keeps_the_sets_logged_after_it() {
    let (app, mut model) = planned_calisthenics();
    let d = (MON - 2) * DAY_SEC;
    for _ in 0..3 {
        app.update(log_set("Knee push-up", 0.0, 8, 8.0, d + 100), &mut model)
            .expect_only_render();
    }
    // "Harder" mid-session, then the rest of the session at the new rung.
    app.update(place(Lad::HorizontalPush, 4, d + 200), &mut model)
        .expect_only_render();
    for _ in 0..3 {
        app.update(log_set("Full push-up", 0.0, 8, 8.0, d + 300), &mut model)
            .expect_only_render();
    }
    log_sets(&app, &mut model, "Full push-up", 3, 8, 8.0, MON - 1);
    assert_eq!(
        push_item(&app, &model).exercise,
        "Decline push-up",
        "the post-Harder sets are the first of 2 qualifying sessions"
    );
}

#[test]
fn undated_placement_does_not_start_the_tendon_clock_at_epoch() {
    let (app, mut model) = planned_calisthenics();
    app.update(place(Lad::FrontLever, 0, 0), &mut model)
        .expect_only_render();
    for back in [9, 2] {
        app.update(
            cal_hold("Tuck front lever", 15, 7.0, (MON - back) * DAY_SEC),
            &mut model,
        )
        .expect_only_render();
    }
    let fl = cal_item(&app.view(&model), Lad::FrontLever).unwrap();
    assert_eq!(
        fl.exercise, "Tuck front lever",
        "7 dated days at the rung are not the 28-day acclimation"
    );
}

#[test]
fn load_capable_pull_up_coaching_promises_load_not_the_next_rung() {
    let (app, mut model) = planned_calisthenics();
    log_sets(&app, &mut model, "Full pull-up", 3, 5, 8.0, MON - 1);
    let pull = cal_item(&app.view(&model), Lad::VerticalPull).unwrap();
    assert!(pull.calis.as_ref().unwrap().load_capable);
    let basis = &pull.why.basis;
    assert!(basis.contains("moves you up to adding weight"), "{basis}");
    assert!(!basis.contains("moves you up to L-sit"), "{basis}");
    assert!(
        basis.contains("starts added weight after one session"),
        "{basis}"
    );
    assert!(!basis.contains("skip ahead"), "{basis}");
}

#[test]
fn placement_beyond_the_equipment_moves_to_a_feasible_rung_and_says_so() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(Profile {
            calis_equipment: Some(CalisEquipment::default()),
            ..calisthenics_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    app.update(place(Lad::VerticalPull, 4, MON * DAY_SEC + 60), &mut model)
        .expect_only_render();
    app.update(place(Lad::Dip, 3, MON * DAY_SEC + 60), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    let ladder = |id: Lad| {
        vm.calisthenics_ladders
            .iter()
            .find(|l| l.ladder == id)
            .unwrap()
            .clone()
    };
    let pull = ladder(Lad::VerticalPull);
    assert_eq!(
        pull.current_rung, None,
        "no rung fits without a bar or rings"
    );
    assert_eq!(pull.placed_rung, Some(4));
    assert!(
        pull.equipment_note.contains("needs a pull-up bar or rings"),
        "{}",
        pull.equipment_note
    );
    let dip = ladder(Lad::Dip);
    assert_eq!(dip.current_rung, Some(0));
    assert_eq!(dip.placed_rung, Some(3));
    assert!(
        dip.equipment_note.contains("you start at Bench dip"),
        "{}",
        dip.equipment_note
    );
    // A rung the equipment allows is not moved.
    let push = ladder(Lad::HorizontalPush);
    assert_eq!(push.placed_rung, None);
    assert!(push.equipment_note.is_empty());
}
