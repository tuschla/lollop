//! Completion-contract regressions: a logged set qualifies for a
//! prescription on movement + one rep/hold second + a work-set load (short and
//! lighter sets count, warm-ups don't; RPE never gates); sets are
//! attributed to a session day (04:00 / 3 h midnight chaining; runs by start);
//! today's readiness/profile edits never rewrite an already-shown day; deload
//! volume folds into the required set count; `SetExerciseEnded` ends/reopens an
//! item; and the hero keeps done items flagged. A past day judged done stays
//! done whatever later readiness removals, reviews or clears do.

use super::*;

/// Log every prescribed lift exercise's sets for `epoch_day` (each at its
/// prescribed reps and a qualifying weight), timestamped at `at`, until the day
/// reads done. Run/rest days are left untouched.
fn complete_lift_day(app: &Engine, model: &mut Model, epoch_day: i64, at: i64) {
    let mut guard = 0;
    loop {
        let remaining: Vec<(String, u32, f64)> = {
            let vm = app.view(model);
            let Some(day) = vm.week_plan.iter().find(|s| s.epoch_day == epoch_day) else {
                return;
            };
            if day.status == SessionStatus::Done {
                break;
            }
            day.items
                .iter()
                .filter(|i| !i.done && !i.exercise.is_empty())
                .map(|i| {
                    (
                        i.exercise.clone(),
                        i.reps_low.max(1) as u32,
                        i.load_kg.unwrap_or(100.0),
                    )
                })
                .collect()
        };
        if remaining.is_empty() {
            break;
        }
        for (ex, reps, w) in &remaining {
            app.update(log_set(ex, *w, *reps, 8.0, at), model)
                .expect_only_render();
        }
        guard += 1;
        assert!(guard < 40, "lift day {epoch_day} should complete");
    }
}

/// The first non-run item on Monday, with its (exercise, sets, reps_low, load).
fn mon_first_item(app: &Engine, model: &Model) -> (String, u8, u32, Option<f64>) {
    let vm = app.view(model);
    let it = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .expect("Monday present")
        .items
        .iter()
        .find(|i| !i.exercise.is_empty())
        .expect("a lift item on Monday");
    (
        it.exercise.clone(),
        it.sets,
        it.reps_low.max(1) as u32,
        it.load_kg,
    )
}

#[test]
fn one_as_prescribed_set_counts_but_does_not_complete_a_multiset_item() {
    let (app, mut model) = planned_strength_model(true);
    let (ex, sets, reps, load) = mon_first_item(&app, &model);
    assert!(sets > 1, "need a multi-set item, got {sets}");
    let w = load.unwrap_or(100.0);
    app.update(log_set(&ex, w, reps, 8.0, MON * DAY_SEC), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    let it = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .unwrap()
        .items
        .iter()
        .find(|i| i.exercise == ex)
        .expect("the item");
    assert_eq!(it.sets_done, 1, "one qualifying set is counted");
    assert!(!it.done, "one of several prescribed sets is not done");
    assert_eq!(it.logged_sets.len(), 1, "the one set is listed");
    assert!(
        it.logged_sets[0].qualifies,
        "an as-prescribed set qualifies"
    );
}

#[test]
fn warmups_do_not_qualify_or_complete_an_anchored_lift() {
    // planned_strength_model(true) anchors "Back Squat" from a logged e1RM, so
    // its item carries a real load. Five 20 kg empty-bar warm-ups must NOT count.
    let (app, mut model) = planned_strength_model(true);
    let (ex, sets, reps, load) = {
        let vm = app.view(&model);
        let it = vm
            .week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .unwrap()
            .items
            .iter()
            .find(|i| i.load_kg.is_some())
            .expect("an anchored (load-carrying) item");
        (
            it.exercise.clone(),
            it.sets,
            it.reps_low.max(1) as u32,
            it.load_kg.unwrap(),
        )
    };
    // Log `sets` empty-bar warm-ups (20 kg, high reps).
    for _ in 0..sets {
        app.update(
            log_set(&ex, 20.0, reps.max(10), 8.0, MON * DAY_SEC),
            &mut model,
        )
        .expect_only_render();
    }
    let it = {
        let vm = app.view(&model);
        vm.week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .unwrap()
            .items
            .iter()
            .find(|i| i.exercise == ex)
            .cloned()
            .expect("the item")
    };
    assert_eq!(it.sets_done, 0, "empty-bar warm-ups do not qualify");
    assert!(!it.done, "warm-ups alone never complete an anchored lift");
    assert!(
        it.logged_sets.iter().all(|s| !s.qualifies),
        "every warm-up is flagged non-qualifying"
    );

    // A working set at the prescribed load DOES qualify.
    app.update(log_set(&ex, load, reps, 8.0, MON * DAY_SEC), &mut model)
        .expect_only_render();
    let it = {
        let vm = app.view(&model);
        vm.week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .unwrap()
            .items
            .iter()
            .find(|i| i.exercise == ex)
            .cloned()
            .unwrap()
    };
    assert_eq!(it.sets_done, 1, "a working set at the load qualifies");
}

#[test]
fn a_cable_row_completes_the_equipment_qualified_row_fill() {
    let (app, mut model) = planned_strength_model(false);
    // Find a horizontal-pull "Row" item anywhere in the week (the plan fill
    // names it "Cable row").
    let (row_day, ex, sets, reps) = {
        let vm = app.view(&model);
        let mut found = None;
        for s in &vm.week_plan {
            for it in &s.items {
                if !it.exercise.is_empty() && crate::plan::same_movement(&it.exercise, "Row") {
                    found = Some((
                        s.epoch_day,
                        it.exercise.clone(),
                        it.sets,
                        it.reps_low.max(1) as u32,
                    ));
                }
            }
        }
        found.expect("a Row item is scheduled")
    };
    assert!(
        crate::plan::same_movement("Cable row", &ex),
        "'Cable row' is the same movement as {ex:?}"
    );
    // Make the Row day today so its completion is computed (the plan window is
    // start..=today).
    app.update(
        Event::SetToday {
            epoch_day: row_day,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let at = row_day * DAY_SEC;
    for _ in 0..sets {
        app.update(log_set("Cable row", 60.0, reps, 8.0, at), &mut model)
            .expect_only_render();
    }
    let it = {
        let vm = app.view(&model);
        vm.week_plan
            .iter()
            .find(|s| s.epoch_day == row_day)
            .unwrap()
            .items
            .iter()
            .find(|i| i.exercise == ex)
            .cloned()
            .expect("the Row item")
    };
    assert_eq!(it.sets_done, sets, "cable rows complete the row fill");
    assert!(it.done, "the Row item reads done");
}

#[test]
fn sets_crossing_midnight_complete_the_day_they_started() {
    let (app, mut model) = planned_strength_model(false);
    // Yesterday morning: today is the day AFTER Monday.
    app.update(
        Event::SetToday {
            epoch_day: MON + 1,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let (ex, sets, reps, _load) = mon_first_item(&app, &model);
    assert!(sets >= 2, "need >=2 sets to split across midnight");
    // All but one set at 23:50 Monday; the last at 00:06 Tuesday (16 min later,
    // < 04:00) chains back to Monday's session.
    let late = MON * DAY_SEC + 23 * 3_600 + 50 * 60;
    let early = (MON + 1) * DAY_SEC + 6 * 60;
    for _ in 0..(sets - 1) {
        app.update(log_set(&ex, 100.0, reps, 8.0, late), &mut model)
            .expect_only_render();
    }
    app.update(log_set(&ex, 100.0, reps, 8.0, early), &mut model)
        .expect_only_render();

    let it = {
        let vm = app.view(&model);
        vm.week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .unwrap()
            .items
            .iter()
            .find(|i| i.exercise == ex)
            .cloned()
            .expect("the item")
    };
    assert_eq!(
        it.sets_done, sets,
        "the after-midnight set counts toward the day it started"
    );
    assert!(it.done, "the item is complete on Monday");
}

#[test]
fn a_short_jog_does_not_complete_a_long_run_but_a_near_full_run_does() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(running_profile()), &mut model)
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
    let long_day = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| s.title == "Long run")
        .expect("a long run day")
        .epoch_day;
    // Make the long-run day today so its completion is computed.
    app.update(
        Event::SetToday {
            epoch_day: long_day,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let at = long_day * DAY_SEC + 9 * 3_600;

    // A 1 km jog: below 80 % of the prescribed distance.
    app.update(log_run_at(1.0, 6.0, 70.0, at), &mut model)
        .expect_only_render();
    let done_after_jog = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| s.epoch_day == long_day)
        .unwrap()
        .items[0]
        .done;
    assert!(!done_after_jog, "a 1 km jog does not complete a long run");

    // Replace it with a 4 km run (>= 80 % of the 4.8 km long run: the 30 %
    // share under 64 km/wk, RUN-LONGRUN-TARGET-001).
    app.update(Event::ClearRuns, &mut model)
        .expect_only_render();
    app.update(log_run_at(4.0, 24.0, 70.0, at), &mut model)
        .expect_only_render();
    let row = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| s.epoch_day == long_day)
        .unwrap()
        .clone();
    assert!(
        row.items[0].done,
        "a run within 80 % of the target completes it (logged {} km)",
        row.items[0].run_logged_km
    );
}

#[test]
fn tendon_pain_today_leaves_todays_calisthenics_rung_and_done_unchanged() {
    let (app, mut model) = planned_calisthenics();
    // Two past qualifying push sessions advance today's rung past Full push-up.
    for day in [MON - 2, MON - 1] {
        for _ in 0..3 {
            app.update(
                log_set("Full push-up", 0.0, 8, 8.0, day * DAY_SEC),
                &mut model,
            )
            .expect_only_render();
        }
    }
    let rung = cal_item(&app.view(&model), Lad::HorizontalPush).expect("a push item");
    assert_eq!(rung.exercise, "Decline push-up", "advanced");
    // Finish today's (advanced) rung.
    let reps = rung.reps_high.max(rung.reps_low) as u32;
    for _ in 0..rung.sets {
        app.update(
            log_set(&rung.exercise, 0.0, reps, 8.0, MON * DAY_SEC),
            &mut model,
        )
        .expect_only_render();
    }
    let before = cal_item(&app.view(&model), Lad::HorizontalPush).unwrap();
    assert!(before.done, "today's rung is complete");

    // A tolerable tendon-pain report DATED TODAY must not rewrite today's rung
    // (that would orphan the just-logged sets).
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::Pain,
            value: 1.0,
            observed_at: MON * DAY_SEC,
            streak: 0,
            pain: Some(crate::schema::PainDetail {
                kind: crate::schema::PainKind::TendonLoadRelated,
                severity: Some(3),
                trend: crate::schema::PainTrend::Stable,
                persists: false,
                location: Some("Left elbow".into()),
            }),
            effort_min: None,
        }),
        &mut model,
    )
    .expect_only_render();
    let after = cal_item(&app.view(&model), Lad::HorizontalPush).unwrap();
    assert_eq!(
        after.exercise, before.exercise,
        "today's rung is unchanged by today's pain"
    );
    assert!(
        after.done,
        "today's done state is unchanged by today's pain"
    );
}

#[test]
fn a_profile_edit_dated_today_leaves_last_mondays_prescription_and_done_unchanged() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
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
    complete_lift_day(&app, &mut model, MON, MON * DAY_SEC);
    // Move to Wednesday: Monday is a completed past day.
    app.update(
        Event::SetToday {
            epoch_day: MON + 2,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let mon_before = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .unwrap()
        .clone();
    assert_eq!(
        mon_before.status,
        SessionStatus::Done,
        "Monday was completed"
    );

    // A goal + weekly-set edit, effective TODAY (Wednesday), must not
    // re-prescribe Monday: its snapshot uses the profile effective that day.
    app.update(
        Event::SetProfile(Profile {
            lift_goal: LiftGoal::Hypertrophy,
            weekly_sets: 22,
            effective_from_epoch_day: Some(MON + 2),
            ..strength_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    let mon_after = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .unwrap()
        .clone();
    assert_eq!(
        mon_after.status,
        SessionStatus::Done,
        "Monday stays done after the edit"
    );
    let sig = |s: &SessionPlanView| -> Vec<(String, u8, bool)> {
        s.items
            .iter()
            .map(|i| (i.exercise.clone(), i.sets, i.done))
            .collect()
    };
    assert_eq!(
        sig(&mon_before),
        sig(&mon_after),
        "Monday's exercises, set counts, and done flags are unchanged"
    );
}

#[test]
fn a_deload_reduced_set_count_completes_the_day_and_stays_done() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
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

    // Monday's prescribed exercises + full set counts, BEFORE any deload.
    let full: Vec<(String, u8, u32)> = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .unwrap()
        .items
        .iter()
        .filter(|i| !i.exercise.is_empty())
        .map(|i| (i.exercise.clone(), i.sets, i.reps_low.max(1) as u32))
        .collect();
    assert!(
        full.iter().any(|(_, sets, _)| *sets >= 2),
        "at least one multi-set exercise so the deload actually reduces it"
    );

    // A review with an rpe-load gap over 2 sessions triggers a standard deload
    // (volume −50 %). Dated today so it is active now and tomorrow.
    app.update(
        Event::SubmitReview(SessionReview {
            rpe_load_gap_sessions: Some(2),
            observed_at: MON * DAY_SEC,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();

    // Log only the REDUCED count of each exercise (round(sets·0.5), min 1).
    for (ex, sets, reps) in &full {
        let reduced = ((*sets as f64 * 0.5).round() as u8).max(1);
        for _ in 0..reduced {
            app.update(log_set(ex, 100.0, *reps, 8.0, MON * DAY_SEC), &mut model)
                .expect_only_render();
        }
    }
    let mon = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .unwrap()
        .clone();
    assert_eq!(
        mon.status,
        SessionStatus::Done,
        "the deload-reduced set count completes the day: {:?}",
        mon.items
            .iter()
            .map(|i| (&i.exercise, i.sets, i.sets_done))
            .collect::<Vec<_>>()
    );

    // The day stays done the next day (the deload is still active).
    app.update(
        Event::SetToday {
            epoch_day: MON + 1,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let mon_next = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .unwrap()
        .clone();
    assert_eq!(
        mon_next.status,
        SessionStatus::Done,
        "a deloaded done day stays done"
    );
}

#[test]
fn set_exercise_ended_ends_and_reopens_an_item_and_clear_sets_clears_it() {
    let (app, mut model) = planned_strength_model(false);
    let (ex, _sets, _reps, _load) = mon_first_item(&app, &model);
    let item = |app: &Engine, model: &Model| -> PrescriptionView {
        app.view(model)
            .week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .unwrap()
            .items
            .iter()
            .find(|i| i.exercise == ex)
            .cloned()
            .expect("the item")
    };
    assert!(!item(&app, &model).done, "unlogged item starts not done");

    app.update(
        Event::SetExerciseEnded {
            exercise: ex.clone(),
            epoch_day: MON,
            ended: true,
            observed_at: MON * DAY_SEC,
        },
        &mut model,
    )
    .expect_only_render();
    let it = item(&app, &model);
    assert!(it.done && it.ended_early, "ending marks the item done");
    assert_eq!(it.sets_done, 0, "ending does not fabricate qualifying sets");
    assert_eq!(
        it.status,
        ItemStatus::Skipped,
        "ended with no work set reads skipped, not done"
    );

    // Reopen.
    app.update(
        Event::SetExerciseEnded {
            exercise: ex.clone(),
            epoch_day: MON,
            ended: false,
            observed_at: MON * DAY_SEC + 10,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(!item(&app, &model).done, "reopening clears the ended state");

    // End again, then ClearSets wipes the marker.
    app.update(
        Event::SetExerciseEnded {
            exercise: ex.clone(),
            epoch_day: MON,
            ended: true,
            observed_at: MON * DAY_SEC + 20,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(item(&app, &model).done, "re-ended");
    app.update(Event::ClearSets, &mut model)
        .expect_only_render();
    let it = item(&app, &model);
    assert!(
        !it.done && !it.ended_early,
        "ClearSets clears the ended marker"
    );
}

/// Monday's first load-carrying (anchored) item: (exercise, sets, reps_low, load).
fn mon_anchored_item(app: &Engine, model: &Model) -> (String, u8, u32, f64) {
    let vm = app.view(model);
    let it = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .unwrap()
        .items
        .iter()
        .find(|i| i.load_kg.is_some())
        .expect("an anchored (load-carrying) item")
        .clone();
    (
        it.exercise,
        it.sets,
        it.reps_low.max(1) as u32,
        it.load_kg.unwrap(),
    )
}

fn mon_item(app: &Engine, model: &Model, ex: &str) -> PrescriptionView {
    app.view(model)
        .week_plan
        .into_iter()
        .find(|s| s.epoch_day == MON)
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.exercise == ex)
        .expect("the item")
}

#[test]
fn a_short_set_and_a_lighter_set_both_count_and_the_item_reads_done_lighter() {
    let (app, mut model) = planned_strength_model(true);
    let (ex, sets, reps, load) = mon_anchored_item(&app, &model);
    assert!(reps > 1 && sets > 2, "need a multi-rep, multi-set item");
    // One rep short at the prescribed load (290 × 4 against 290 × 5).
    app.update(
        log_set(&ex, load, reps - 1, 8.0, MON * DAY_SEC + 3_600),
        &mut model,
    )
    .expect_only_render();
    let it = mon_item(&app, &model, &ex);
    assert_eq!(it.sets_done, 1, "a short set fills its slot");
    assert!(it.logged_sets[0].qualifies && !it.logged_sets[0].lighter);
    assert_eq!(it.status, ItemStatus::InProgress);
    // ~91 % of the prescribed load for more reps (264.6 × 5 against 290 × 3).
    app.update(
        log_set(&ex, 0.91 * load, reps + 2, 8.0, MON * DAY_SEC + 3_700),
        &mut model,
    )
    .expect_only_render();
    let it = mon_item(&app, &model, &ex);
    assert_eq!(it.sets_done, 2, "a lighter work set counts");
    assert!(it.logged_sets[1].qualifies && it.logged_sets[1].lighter);
    for k in 2..i64::from(sets) {
        app.update(
            log_set(&ex, load, reps, 8.0, MON * DAY_SEC + 3_700 + k * 60),
            &mut model,
        )
        .expect_only_render();
    }
    let it = mon_item(&app, &model, &ex);
    assert!(it.done, "every slot filled: {:?}", it.logged_sets);
    assert_eq!(it.status, ItemStatus::DoneLighter);
}

#[test]
fn a_past_day_with_half_its_work_sets_reads_done_lighter_not_missed() {
    let (app, mut model) = planned_strength_model(true);
    let items: Vec<(String, u8, u32, f64)> = day_row(&app, &model, MON)
        .items
        .iter()
        .filter(|i| !i.exercise.is_empty())
        .map(|i| {
            (
                i.exercise.clone(),
                i.sets,
                i.reps_low.max(1) as u32,
                i.load_kg.unwrap_or(100.0),
            )
        })
        .collect();
    let planned: u32 = items.iter().map(|(_, s, ..)| u32::from((*s).max(1))).sum();
    // Log sets item by item until at least half the planned work sets are in.
    let mut logged = 0u32;
    'outer: for (ex, sets, reps, kg) in &items {
        for _ in 0..*sets {
            if 2 * logged >= planned {
                break 'outer;
            }
            app.update(
                log_set(ex, *kg, *reps, 8.0, MON * DAY_SEC + 3_600),
                &mut model,
            )
            .expect_only_render();
            logged += 1;
        }
    }
    set_today(&app, &mut model, MON + 1);
    let mon = day_row(&app, &model, MON);
    assert_eq!(
        mon.status,
        SessionStatus::DoneLighter,
        "{logged} of {planned} work sets: {:?}",
        mon.items
            .iter()
            .map(|i| (&i.exercise, i.sets, i.sets_done))
            .collect::<Vec<_>>()
    );
    let c = app.view(&model).consistency.expect("consistency view");
    assert_eq!(c.this_week_done, 1, "a lighter day still counts as done");
    assert!(
        !c.notes.iter().any(|n| n.summary.starts_with("Missed")),
        "no missed-session nudge for a day that was trained"
    );
}

#[test]
fn the_hero_keeps_a_done_item_flagged_rather_than_dropping_it() {
    let (app, mut model) = planned_strength_model(false);
    let (ex, sets, reps, _load) = mon_first_item(&app, &model);
    for _ in 0..sets {
        app.update(log_set(&ex, 100.0, reps, 8.0, MON * DAY_SEC), &mut model)
            .expect_only_render();
    }
    let ns = app
        .view(&model)
        .next_session
        .expect("a next session on today");
    assert_eq!(ns.epoch_day, MON, "still on today (other items remain)");
    let hero_item = ns
        .items
        .iter()
        .find(|i| i.exercise == ex)
        .expect("the finished item stays on the hero");
    assert!(
        hero_item.done,
        "a finished hero item is flagged done, not dropped"
    );
}

/// Log every lift item of `epoch_day` exactly as SHOWN — its displayed set count
/// at its displayed load (a qualifying weight when unanchored) — timestamped at
/// `at`.
fn log_shown_session(app: &Engine, model: &mut Model, epoch_day: i64, at: i64) {
    let items: Vec<(String, u8, u32, f64)> = {
        let vm = app.view(model);
        let day = vm
            .week_plan
            .iter()
            .find(|s| s.epoch_day == epoch_day)
            .expect("day present");
        day.items
            .iter()
            .filter(|i| !i.exercise.is_empty())
            .map(|i| {
                (
                    i.exercise.clone(),
                    i.sets,
                    i.reps_low.max(1) as u32,
                    i.load_kg.unwrap_or(100.0),
                )
            })
            .collect()
    };
    for (ex, sets, reps, w) in items {
        for _ in 0..sets {
            app.update(log_set(&ex, w, reps, 8.0, at), model)
                .expect_only_render();
        }
    }
}

#[test]
fn following_the_shown_reduced_load_completes_an_anchored_deload_day() {
    // planned_strength_model(true) anchors Back Squat from a logged e1RM, so the
    // deloaded item carries a real (reduced) load. Logging exactly the shown
    // session — the reduced sets at the reduced load — must complete it, not leave
    // it stuck because the bar stayed at the un-reduced prescribed load.
    let (app, mut model) = planned_strength_model(true);
    app.update(
        Event::SubmitReview(SessionReview {
            rpe_load_gap_sessions: Some(2),
            observed_at: MON * DAY_SEC,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let (ex, load, sets, reps) = {
        let ns = app.view(&model).next_session.expect("a hero session");
        let it = ns
            .items
            .iter()
            .find(|i| i.load_kg.is_some())
            .expect("a deload-folded anchored item");
        (
            it.exercise.clone(),
            it.load_kg.unwrap(),
            it.sets,
            it.reps_low.max(1) as u32,
        )
    };
    for _ in 0..sets {
        app.update(log_set(&ex, load, reps, 8.0, MON * DAY_SEC), &mut model)
            .expect_only_render();
    }
    let it = {
        let vm = app.view(&model);
        vm.week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .unwrap()
            .items
            .iter()
            .find(|i| i.exercise == ex)
            .cloned()
            .expect("the anchored item")
    };
    assert_eq!(
        it.sets_done, sets,
        "the shown-load sets qualify: {:?}",
        it.logged_sets
    );
    assert!(
        it.done,
        "following the shown reduced load completes the item"
    );
}

/// A Monday (`mon0_weekday == 0`) whose unix timestamps are positive, so dated
/// inputs, reviews and removals take their dated (windowed) paths.
const POS_MON: i64 = 20_353;

/// Monday's week-strip row.
fn day_row(app: &Engine, model: &Model, epoch_day: i64) -> SessionPlanView {
    app.view(model)
        .week_plan
        .into_iter()
        .find(|s| s.epoch_day == epoch_day)
        .expect("day row present")
}

/// Move the shell's clock to `epoch_day` (UTC).
fn set_today(app: &Engine, model: &mut Model, epoch_day: i64) {
    app.update(
        Event::SetToday {
            epoch_day,
            utc_offset_sec: 0,
        },
        model,
    )
    .expect_only_render();
}

#[test]
fn a_deload_expiring_midweek_keeps_an_already_done_day_done() {
    let (app, mut model) = planned_strength_model_on(POS_MON, true);
    // A review deload dated the previous Wednesday: Deload{weeks:1} is active
    // on [POS_MON-5, POS_MON+2), so it covers this Monday but expires Wednesday.
    app.update(
        Event::SubmitReview(SessionReview {
            rpe_load_gap_sessions: Some(2),
            observed_at: (POS_MON - 5) * DAY_SEC + 12 * 3_600,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    log_shown_session(&app, &mut model, POS_MON, POS_MON * DAY_SEC + 3_600);
    assert_eq!(
        day_row(&app, &model, POS_MON).status,
        SessionStatus::Done,
        "Monday completes on the shown deload session"
    );
    // Advance to Wednesday: today's deload has aged out, but Monday sits inside
    // its window, so Monday must stay done — not flip to missed.
    set_today(&app, &mut model, POS_MON + 2);
    assert!(
        app.view(&model).review_adjustments.is_empty(),
        "the deload has aged out for today"
    );
    assert_eq!(
        day_row(&app, &model, POS_MON).status,
        SessionStatus::Done,
        "a deload expiring midweek must not flip an already-done day to missed"
    );
}

/// Log every lift item shown on `epoch_day` light: its shown set count × reps
/// at `kg`, timestamped at `at`.
fn log_shown_sets_at(app: &Engine, model: &mut Model, epoch_day: i64, kg: f64, at: i64) {
    let items: Vec<(String, u8, u32)> = day_row(app, model, epoch_day)
        .items
        .iter()
        .filter(|i| !i.exercise.is_empty())
        .map(|i| (i.exercise.clone(), i.sets, i.reps_low.max(1) as u32))
        .collect();
    for (ex, sets, reps) in items {
        for _ in 0..sets {
            app.update(log_set(&ex, kg, reps, 8.0, at), model)
                .expect_only_render();
        }
    }
}

/// Log every lift item shown on `epoch_day` at its SHOWN load (or `fallback`
/// kg for an effort-prescribed item): its shown set count × reps at `at`.
fn log_shown_sets_at_shown_load(
    app: &Engine,
    model: &mut Model,
    epoch_day: i64,
    fallback: f64,
    at: i64,
) {
    let items: Vec<(String, u8, u32, f64)> = day_row(app, model, epoch_day)
        .items
        .iter()
        .filter(|i| !i.exercise.is_empty())
        .map(|i| {
            (
                i.exercise.clone(),
                i.sets,
                i.reps_low.max(1) as u32,
                i.load_kg.unwrap_or(fallback),
            )
        })
        .collect();
    for (ex, sets, reps, kg) in items {
        for _ in 0..sets {
            app.update(log_set(&ex, kg, reps, 8.0, at), model)
                .expect_only_render();
        }
    }
}

#[test]
fn removing_a_readiness_input_the_next_day_keeps_the_day_it_shaped_done() {
    // Monday: a head-cold report turns the session into an easy, load-free,
    // half-volume one (autoreg-045); every shown set is logged light (far
    // under the anchored squat bar).
    let (app, mut model) = planned_strength_model_on(POS_MON, true);
    let illness = |value: f64, at: i64| {
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::Illness,
            value,
            observed_at: at,
            streak: 0,
            pain: None,
            effort_min: None,
        })
    };
    app.update(illness(1.0, POS_MON * DAY_SEC + 3_600), &mut model)
        .expect_only_render();
    log_shown_sets_at(&app, &mut model, POS_MON, 60.0, POS_MON * DAY_SEC + 7_200);
    assert_eq!(
        day_row(&app, &model, POS_MON).status,
        SessionStatus::Done,
        "the easy session as shown completes Monday"
    );
    // Tuesday: an all-clear reading, then both illness reports are removed.
    set_today(&app, &mut model, POS_MON + 1);
    let tue = (POS_MON + 1) * DAY_SEC + 3_600;
    app.update(illness(0.0, tue), &mut model)
        .expect_only_render();
    for _ in 0..2 {
        app.update(
            Event::RemoveReadiness {
                signal: ReadinessSignal::Illness,
                observed_at: tue + 60,
            },
            &mut model,
        )
        .expect_only_render();
    }
    let vm = app.view(&model);
    assert!(
        !vm.readiness_summary
            .iter()
            .any(|r| r.signal == ReadinessSignal::Illness),
        "today no longer sees the removed reports"
    );
    let mon = day_row(&app, &model, POS_MON);
    assert_eq!(
        mon.status,
        SessionStatus::Done,
        "a report removed on Tuesday still shaped Monday: {:?}",
        mon.items
            .iter()
            .map(|i| (&i.exercise, i.sets, i.sets_done, i.load_kg))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_review_hrv_downgrade_keeps_its_day_done_the_next_day() {
    // Monday's review carries an HRV single-day drop with a downtrend: the
    // session is downgraded one level and every shown set is logged as shown.
    let (app, mut model) = planned_strength_model_on(POS_MON, true);
    app.update(
        Event::SubmitReview(SessionReview {
            hrv_single_day_z: Some(-1.5),
            hrv_downtrend_days: Some(2),
            observed_at: POS_MON * DAY_SEC + 3_600,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    // A readiness downgrade is "one level": the shown load is
    // ~5 % under the plan, below the full bar's plate step, so logging it
    // completes Monday only through the downgrade fold.
    log_shown_sets_at_shown_load(&app, &mut model, POS_MON, 60.0, POS_MON * DAY_SEC + 7_200);
    assert_eq!(day_row(&app, &model, POS_MON).status, SessionStatus::Done);
    set_today(&app, &mut model, POS_MON + 1);
    let mon = day_row(&app, &model, POS_MON);
    assert_eq!(
        mon.status,
        SessionStatus::Done,
        "the review downgrade still governs Monday: {:?}",
        mon.items
            .iter()
            .map(|i| (&i.exercise, i.sets, i.sets_done, i.load_kg))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_later_review_or_clear_keeps_an_earlier_deload_day_done() {
    // Sunday evening's weekly check-in triggers a deload; Monday is completed
    // exactly as shown (reduced sets at the reduced load).
    let (app, mut model) = planned_strength_model_on(POS_MON, true);
    app.update(
        Event::SubmitReview(SessionReview {
            rpe_load_gap_sessions: Some(2),
            observed_at: (POS_MON - 1) * DAY_SEC + 20 * 3_600,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    log_shown_session(&app, &mut model, POS_MON, POS_MON * DAY_SEC + 3_600);
    assert_eq!(day_row(&app, &model, POS_MON).status, SessionStatus::Done);
    // Tuesday's per-session review carries every weekly field at its default.
    set_today(&app, &mut model, POS_MON + 1);
    app.update(
        Event::SubmitReview(SessionReview {
            observed_at: (POS_MON + 1) * DAY_SEC + 3_600,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(
        vm.review_adjustments
            .iter()
            .any(|a| a.summary.starts_with("Lighter week")),
        "a routine session review never replaces the weekly deload: {:#?}",
        vm.review_adjustments
    );
    assert_eq!(
        day_row(&app, &model, POS_MON).status,
        SessionStatus::Done,
        "a later review never takes the deload back from Monday"
    );
    app.update(Event::ClearReview, &mut model)
        .expect_only_render();
    assert!(
        !app.view(&model).review_adjustments.is_empty(),
        "clearing the session review leaves the weekly deload in force"
    );
    app.update(Event::ClearWeeklyCheckin, &mut model)
        .expect_only_render();
    assert!(app.view(&model).review_adjustments.is_empty());
    assert_eq!(
        day_row(&app, &model, POS_MON).status,
        SessionStatus::Done,
        "clearing the check-in never takes the deload back from Monday"
    );
}

#[test]
fn a_done_deload_row_shows_the_folded_sets_and_load() {
    // The deload hero shows the reduced numbers; once the day is done and the
    // hero moves on, Monday's own row keeps showing them (3/3, not 3/5).
    let (app, mut model) = planned_strength_model_on(POS_MON, true);
    let full = day_row(&app, &model, POS_MON);
    app.update(
        Event::SubmitReview(SessionReview {
            rpe_load_gap_sessions: Some(2),
            observed_at: POS_MON * DAY_SEC + 600,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let shown: Vec<(String, u8, Option<f64>)> = app
        .view(&model)
        .next_session
        .expect("the deloaded hero")
        .items
        .iter()
        .filter(|i| !i.exercise.is_empty())
        .map(|i| (i.exercise.clone(), i.sets, i.load_kg))
        .collect();
    let squat_full = full
        .items
        .iter()
        .find(|i| i.exercise == "Back Squat")
        .expect("anchored squat");
    let squat_shown = shown
        .iter()
        .find(|(ex, ..)| ex == "Back Squat")
        .expect("squat on the hero");
    assert!(
        squat_shown.1 < squat_full.sets && squat_shown.2 < squat_full.load_kg,
        "the deload reduces the squat's sets and load: {squat_full:?} → {squat_shown:?}"
    );
    log_shown_session(&app, &mut model, POS_MON, POS_MON * DAY_SEC + 3_600);
    for today in [POS_MON, POS_MON + 1] {
        set_today(&app, &mut model, today);
        let mon = day_row(&app, &model, POS_MON);
        assert_eq!(mon.status, SessionStatus::Done);
        for (ex, sets, load) in &shown {
            let it = mon
                .items
                .iter()
                .find(|i| &i.exercise == ex)
                .expect("the item");
            assert_eq!(
                (it.sets, it.load_kg, it.sets_done),
                (*sets, *load, *sets),
                "{ex} reads its folded numbers on day {today}"
            );
            assert!(
                it.summary.contains(&format!("{sets}×")),
                "{ex} summary shows the folded set count: {}",
                it.summary
            );
        }
    }
}

#[test]
fn stacked_deloads_take_the_single_deepest_cut_not_compounded() {
    // Two simultaneous 50 % deloads reduce a 5-set item to 3 (the single deepest
    // cut), never 2 (sequential compounding) — otherwise the hero would show the
    // item done at 2 sets while the next day's judgement needs 3 (a done→missed
    // flip). Both the display fold and the completion judgement share one
    // composition.
    let mut ns = SessionPlanView {
        items: vec![PrescriptionView {
            exercise: "Back Squat".into(),
            sets: 5,
            reps_low: 3,
            reps_high: 3,
            load_kg: Some(120.0),
            sets_done: 3,
            ..Default::default()
        }],
        ..Default::default()
    };
    let deload = crate::evidence::graded(
        Adjustment::Deload {
            volume_reduction_pct: 50.0,
            load_reduction_pct: 10.0,
            weeks: 1,
        },
        claims::RUN_DELOAD_001,
    );
    apply_adjustments_to_session(&mut ns, &[deload.clone(), deload], false);
    assert_eq!(
        ns.items[0].sets, 3,
        "the single deepest cut, not compounded to 2"
    );
    assert!(
        ns.items[0].done,
        "3 qualifying sets complete the reduced 3-set item"
    );
}

/// A runner's plan from `MON`, and the first run day's row with a run
/// planned by distance, with that planned km.
fn first_distance_run(app: &Engine, model: &mut Model) -> (i64, f64) {
    app.update(Event::SetProfile(running_profile()), model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        model,
    )
    .expect_only_render();
    set_today(app, model, MON);
    app.view(model)
        .week_plan
        .iter()
        .find_map(|s| {
            let it = s.items.iter().find(|i| i.is_run())?;
            let (km, _) = crate::app::plan_views::run_planned(it);
            km.map(|k| (s.epoch_day, k))
        })
        .expect("a run planned by distance")
}

#[test]
fn a_past_run_at_half_its_plan_reads_done_lighter_not_missed() {
    // 74 % of a planned run is credited like a short lift
    // day; under half stays missed.
    for (share, want) in [
        (0.74, SessionStatus::DoneLighter),
        (0.4, SessionStatus::Partial),
    ] {
        let app = Engine;
        let mut model = Model::default();
        let (day, planned) = first_distance_run(&app, &mut model);
        app.update(
            log_run_at(
                planned * share,
                planned * share * 6.0,
                70.0,
                day * DAY_SEC + 9 * 3_600,
            ),
            &mut model,
        )
        .expect_only_render();
        set_today(&app, &mut model, day + 1);
        let row = day_row(&app, &model, day);
        assert_eq!(row.status, want, "{share} of {planned} km");
        let run = row.items.iter().find(|i| i.is_run()).unwrap();
        assert!(!run.done, "under 80 % never reads fully done");
        if want == SessionStatus::DoneLighter {
            assert_eq!(run.status, ItemStatus::DoneLighter);
        } else {
            assert_eq!(run.status, ItemStatus::InProgress);
        }
    }
}

#[test]
fn session_done_carries_run_numbers_and_says_of_plan_when_short() {
    // The run line is numbers the shell formats in its unit,
    // and "as planned" only when the run reached the plan.
    let app = Engine;
    let mut model = Model::default();
    let (day, planned) = first_distance_run(&app, &mut model);
    set_today(&app, &mut model, day);
    let km = (planned * 0.85 * 10.0).round() / 10.0;
    app.update(
        log_run_at(km, 60.0, 70.0, day * DAY_SEC + 9 * 3_600),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let done = vm.session_done.expect("the run day is done");
    let run = done.items.iter().find(|i| i.exercise == "Run").unwrap();
    assert_eq!(run.run_km, Some(km));
    assert_eq!(run.run_min, Some(60.0));
    assert_eq!(run.run_planned_km, Some(planned));
    assert_eq!(run.run_planned_min, None);
    assert_eq!(run.status, ItemStatus::Done);
    let fb = vm.feedback.expect("feedback");
    assert_eq!(
        fb.message,
        format!("Run done: {km:.1} km of {planned:.1} km in 60 min."),
    );
    assert_eq!(
        fb.message_mi,
        format!(
            "Run done: {} of {} in 60 min.",
            fmt_mi_of_km(km),
            fmt_mi_of_km(planned)
        ),
    );

    // The full distance reads "as planned".
    app.update(Event::ClearRuns, &mut model)
        .expect_only_render();
    app.update(
        log_run_at(planned, 60.0, 70.0, day * DAY_SEC + 9 * 3_600),
        &mut model,
    )
    .expect_only_render();
    let msg = app.view(&model).feedback.expect("feedback").message;
    assert!(msg.ends_with("as planned."), "{msg}");
}

#[test]
fn week_plan_marks_a_fully_logged_day_done() {
    // Today = MON. A session is done only when EVERY prescribed exercise has its
    // prescribed SETS logged (each qualifying: same movement + reps + load, RPE
    // never gates). The hero KEEPS every item — a done exercise stays flagged
    // `done` (the shell collapses it); the day reads done and the hero advances
    // only when all exercises' sets are logged.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
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

    // The full prescribed set for today: each exercise with its set + rep count.
    let items: Vec<(String, u8, u32)> = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .expect("Monday present")
        .items
        .iter()
        .map(|i| (i.exercise.clone(), i.sets, i.reps_low.max(1) as u32))
        .collect();
    assert!(
        items.len() > 1,
        "today's session has several exercises: {items:?}"
    );
    let (ex0, sets0, reps0) = items[0].clone();
    assert!(sets0 > 1, "the first exercise is multi-set: {sets0}");

    // ONE set of the first exercise: NOT done, and the exercise STAYS on the hero.
    app.update(log_set(&ex0, 120.0, reps0, 8.0, MON * DAY_SEC), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert_ne!(
        vm.week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .unwrap()
            .status,
        SessionStatus::Done,
        "one set logged is not a done session"
    );
    let ns = vm
        .next_session
        .as_ref()
        .expect("the hero stays on today's unfinished session");
    assert_eq!(
        ns.epoch_day, MON,
        "the hero does not advance past an unfinished today"
    );
    let hero_ex0 = ns
        .items
        .iter()
        .find(|i| i.exercise == ex0)
        .expect("the partially-logged exercise stays on the hero");
    assert!(!hero_ex0.done, "one set is not done");
    assert_eq!(hero_ex0.sets_done, 1, "one qualifying set counted");

    // Finish the first exercise: log its sets until it reads done. It STAYS on
    // the hero (done items stay, flagged done), and the day is still NOT done.
    let mut guard = 0;
    loop {
        let done = app.view(&model).next_session.as_ref().is_some_and(|n| {
            n.items
                .iter()
                .find(|i| i.exercise == ex0)
                .is_some_and(|i| i.done)
        });
        if done {
            break;
        }
        app.update(log_set(&ex0, 120.0, reps0, 8.0, MON * DAY_SEC), &mut model)
            .expect_only_render();
        guard += 1;
        assert!(
            guard < 30,
            "the first exercise should complete once its sets are logged"
        );
    }
    let vm = app.view(&model);
    assert_ne!(
        vm.week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .unwrap()
            .status,
        SessionStatus::Done,
        "the other exercises are still unlogged"
    );
    let ns = vm.next_session.as_ref().expect("the hero still on today");
    assert_eq!(ns.epoch_day, MON);
    let hero_ex0 = ns
        .items
        .iter()
        .find(|i| i.exercise == ex0)
        .expect("a fully-logged exercise stays on the hero");
    assert!(
        hero_ex0.done,
        "the finished exercise is flagged done, not dropped"
    );

    // Finish every remaining exercise → the day reads done and the hero advances.
    let mut guard = 0;
    loop {
        let (done, remaining): (bool, Vec<(String, u32)>) = {
            let vm = app.view(&model);
            let mon = vm.week_plan.iter().find(|s| s.epoch_day == MON).unwrap();
            let remaining = mon
                .items
                .iter()
                .filter(|i| !i.done)
                .map(|i| (i.exercise.clone(), i.reps_low.max(1) as u32))
                .collect();
            (mon.status == SessionStatus::Done, remaining)
        };
        if done {
            break;
        }
        for (ex, reps) in &remaining {
            app.update(log_set(ex, 100.0, *reps, 8.0, MON * DAY_SEC), &mut model)
                .expect_only_render();
        }
        guard += 1;
        assert!(
            guard < 30,
            "the session should complete once every exercise's sets are logged"
        );
    }
    let vm = app.view(&model);
    let mon = vm.week_plan.iter().find(|s| s.epoch_day == MON).unwrap();
    assert_eq!(
        mon.status,
        SessionStatus::Done,
        "every exercise's sets logged reads done"
    );
    assert!(
        mon.items.iter().all(|i| i.done),
        "every item reads done: {:?}",
        mon.items
            .iter()
            .map(|i| (&i.exercise, i.sets, i.sets_done, i.done))
            .collect::<Vec<_>>()
    );
    let ns = vm
        .next_session
        .as_ref()
        .expect("a next session after a done today");
    assert!(
        ns.epoch_day > MON,
        "the hero advances past a fully-logged today, got {}",
        ns.epoch_day
    );

    // A fully-logged PAST day stays "done": advance the clock to Wednesday.
    app.update(
        Event::SetToday {
            epoch_day: MON + 2,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let mon_row = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .expect("Monday present")
        .clone();
    assert_eq!(
        mon_row.status,
        SessionStatus::Done,
        "a fully-logged past day stays done"
    );
}

#[test]
fn todays_prescription_is_fixed_while_its_sets_are_logged() {
    // A day is prescribed from history logged strictly BEFORE it. Were the
    // week re-synthesized from the new anchors, the day's OWN target would move
    // under its sets (an unanchored lift gains an e1RM → `plan::main_lifts`
    // re-orders the day and the DUP path re-sizes it, e.g. 3 → 5 sets), so the
    // day would never read "done" and the per-item Log action never go away.
    let (app, mut model) = planned_strength_model(false);
    let monday_items = |model: &Model| -> Vec<(String, u8)> {
        app.view(model)
            .week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .expect("Monday present")
            .items
            .iter()
            .map(|i| (i.exercise.clone(), i.sets))
            .collect()
    };
    let before = monday_items(&model);
    assert!(
        before.len() > 1 && before.iter().all(|(_, sets)| *sets > 0),
        "today prescribes several multi-set exercises: {before:?}"
    );

    // Log exactly the prescribed sets of every prescribed exercise.
    for (ex, sets) in &before {
        for _ in 0..*sets {
            app.update(log_set(ex, 100.0, 5, 8.0, MON * DAY_SEC), &mut model)
                .expect_only_render();
        }
    }

    let vm = app.view(&model);
    let mon = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .expect("Monday present");
    assert_eq!(
        mon.status,
        SessionStatus::Done,
        "every prescribed set logged is a done day"
    );
    assert_eq!(
        monday_items(&model),
        before,
        "today's exercises and set counts must not move under today's own sets"
    );
    assert!(
        mon.items.iter().all(|i| i.done),
        "every item reads done (the shell hides its Log action on this flag): {:?}",
        mon.items
            .iter()
            .map(|i| (&i.exercise, i.sets, i.done))
            .collect::<Vec<_>>()
    );
    let ns = vm
        .next_session
        .as_ref()
        .expect("a next session after a done today");
    assert!(
        ns.epoch_day > MON,
        "the hero advances past a fully-logged today, got {}",
        ns.epoch_day
    );
    // Today's sets DO anchor from tomorrow on: the next session is loaded off
    // the e1RM they just demonstrated.
    assert!(
        ns.items.iter().any(|i| !i.anchored_on.is_empty()),
        "today's sets anchor the next session's loads: {:?}",
        ns.items
            .iter()
            .map(|i| (&i.exercise, &i.intensity_label))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_past_day_completes_from_its_own_prescription_when_logged_later() {
    // Retroactive logging (the owner's report: yesterday's session entered
    // today) must complete the day it is dated on. The past day keeps the
    // prescription it was rendered with, so its own sets can never re-size it
    // out of "done".
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    // Today is TUESDAY with nothing logged: Monday is a missed past day.
    app.update(
        Event::SetToday {
            epoch_day: MON + 1,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let monday = |model: &Model| {
        app.view(model)
            .week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .expect("Monday present")
            .clone()
    };
    let before: Vec<(String, u8)> = monday(&model)
        .items
        .iter()
        .map(|i| (i.exercise.clone(), i.sets))
        .collect();
    assert_eq!(
        monday(&model).status,
        SessionStatus::Missed,
        "Monday starts unlogged"
    );
    assert!(before.len() > 1, "Monday prescribes several exercises");

    // Log Monday's work retroactively at local noon on Monday — the timestamp
    // the shell's `planDayObservedAt` sends for a back-dated plan day.
    let mut id = 0u64;
    for (ex, sets) in &before {
        for _ in 0..*sets {
            id += 1;
            app.update(
                log_set_id(ex, 100.0, id, MON * DAY_SEC + 12 * 3600),
                &mut model,
            )
            .expect_only_render();
        }
    }

    let mon = monday(&model);
    assert_eq!(
        mon.status,
        SessionStatus::Done,
        "the retroactively logged day completes"
    );
    assert_eq!(
        mon.items
            .iter()
            .map(|i| (i.exercise.clone(), i.sets))
            .collect::<Vec<_>>(),
        before,
        "a past day keeps the prescription it was rendered with"
    );
    assert!(
        mon.items.iter().all(|i| i.done),
        "every item of the logged past day reads done: {:?}",
        mon.items
            .iter()
            .map(|i| (&i.exercise, i.sets, i.done))
            .collect::<Vec<_>>()
    );
}

#[test]
fn partial_past_lift_day_is_judged_on_its_logged_share_with_per_item_done_flags() {
    // A past training day with one of several exercises finished reads
    // "missed" under half its planned work sets and "done_lighter" at half or
    // more, and the finished exercise carries its own done flag while the
    // rest stay not-done.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
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
    // Finish only the first prescribed exercise on Monday (log its prescribed
    // reps until it reads done; it STAYS on the hero, flagged done).
    let (ex0, reps0) = {
        let vm = app.view(&model);
        let it = &vm
            .week_plan
            .iter()
            .find(|s| s.epoch_day == MON)
            .unwrap()
            .items[0];
        (it.exercise.clone(), it.reps_low.max(1) as u32)
    };
    let mut guard = 0;
    loop {
        let done = app.view(&model).next_session.as_ref().is_some_and(|n| {
            n.epoch_day == MON
                && n.items
                    .iter()
                    .find(|i| i.exercise == ex0)
                    .is_some_and(|i| i.done)
        });
        if done {
            break;
        }
        app.update(log_set(&ex0, 120.0, reps0, 8.0, MON * DAY_SEC), &mut model)
            .expect_only_render();
        guard += 1;
        assert!(guard < 30, "the first exercise should complete");
    }
    // Advance to Wednesday: Monday is now a partially-logged past day.
    app.update(
        Event::SetToday {
            epoch_day: MON + 2,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let mon = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .expect("Monday present");
    let (planned, logged) =
        mon.items
            .iter()
            .filter(|i| !i.exercise.is_empty())
            .fold((0u32, 0u32), |(p, l), i| {
                let required = u32::from(i.sets.max(1));
                (p + required, l + u32::from(i.sets_done).min(required))
            });
    // final-setup F7: under the lighter credit, logged work reads partly
    // done, never missed.
    let expected = if 2 * logged >= planned {
        SessionStatus::DoneLighter
    } else {
        SessionStatus::Partial
    };
    assert_eq!(
        mon.status, expected,
        "{logged} of {planned} work sets on a partly logged past day"
    );
    let done_item = mon
        .items
        .iter()
        .find(|i| i.exercise == ex0)
        .expect("the finished exercise");
    assert!(
        done_item.done,
        "the finished exercise carries its own done flag"
    );
    assert!(
        mon.items.iter().any(|i| i.exercise != ex0 && !i.done),
        "the unlogged exercises stay not-done"
    );
}

#[test]
fn past_run_day_is_done_when_the_run_meets_its_volume() {
    // A run session is complete when the runs attributed to that day clear 80 %
    // of the prescribed volume. A generous run logged mid-morning on the
    // scheduled run day (so its START attributes to that day) marks it done.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(running_profile()), &mut model)
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
    let run_day = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| matches!(s.session_type, SessionType::Run(_)))
        .expect("a scheduled run day")
        .epoch_day;
    // 09:00 start, well clear of any prescribed volume for this profile.
    app.update(
        log_run_at(30.0, 180.0, 70.0, run_day * DAY_SEC + 9 * 3_600),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: MON + 6,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let row = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == run_day)
        .expect("run day present");
    assert_eq!(
        row.status,
        SessionStatus::Done,
        "a past run day is done once the run meets its volume"
    );
    assert!(
        row.items.iter().all(|i| i.done),
        "the run item is marked done"
    );
    assert!(
        row.items[0].run_logged_km > 0.0,
        "the run's km are attributed to its start day"
    );
}

#[test]
fn week_plan_unlogged_today_stays_next() {
    // Today = MON, plan generated but NO set logged: today's training day is
    // still the "next" session, dated today.
    let (app, model) = planned_strength_model(false);
    let vm = app.view(&model);
    let mon_row = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .expect("Monday is in the week strip");
    assert_ne!(
        mon_row.status,
        SessionStatus::Done,
        "an unlogged today is not done"
    );
    let ns = vm.next_session.as_ref().expect("a next session on today");
    assert_eq!(
        ns.status,
        SessionStatus::Next,
        "an unlogged today's session is next"
    );
    assert_eq!(ns.epoch_day, MON, "the next session is dated today");
}

// ── Past-day completeness buckets by the shell's LOCAL day ──
#[test]
fn past_day_completeness_buckets_by_local_day() {
    // Completing Monday's prescribed work with sets timestamped 23:30 UTC on
    // (MON-1) marks Monday done in a +2h zone, because those sets are LOCAL
    // Monday. If the day were bucketed by raw UTC the sets would land on the
    // pre-plan day and Monday would never complete.
    let app = Engine;
    let mut model = Model::default();
    let off = 2 * 3600;
    let boundary = (MON - 1) * DAY_SEC + 23 * 3600 + 30 * 60; // 23:30 UTC, MON-1
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    // Today = Wed local; +2h offset → the boundary set is LOCAL Monday.
    app.update(
        Event::SetToday {
            epoch_day: MON + 2,
            utc_offset_sec: off,
        },
        &mut model,
    )
    .expect_only_render();
    // Complete every prescribed Monday exercise at that boundary timestamp.
    let mut guard = 0;
    loop {
        let remaining: Vec<(String, u32)> = {
            let vm = app.view(&model);
            let mon = vm.week_plan.iter().find(|s| s.epoch_day == MON).unwrap();
            if mon.status == SessionStatus::Done {
                break;
            }
            mon.items
                .iter()
                .filter(|i| !i.done)
                .map(|i| (i.exercise.clone(), i.reps_low.max(1) as u32))
                .collect()
        };
        for (ex, reps) in &remaining {
            app.update(log_set(ex, 120.0, *reps, 8.0, boundary), &mut model)
                .expect_only_render();
        }
        guard += 1;
        assert!(
            guard < 40,
            "Monday completes once every exercise's sets are logged"
        );
    }
    let mon = app
        .view(&model)
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .expect("Monday present")
        .clone();
    assert_eq!(
        mon.status,
        SessionStatus::Done,
        "sets at 23:30 UTC MON-1 bucket to LOCAL Monday (+2h) and complete it"
    );
}

// ── The program week advances with SetToday; a mid-week plan does not
//        back-date the week's earlier days as missed ──
#[test]
fn week_advances_and_pre_start_days_are_not_missed() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    // Plan starts Wednesday (MON+2), mid-week.
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON + 2,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: MON + 2,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    // Monday (before the plan started) is shown but never reads "missed".
    let mon = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .expect("the plan week shows Monday");
    assert_eq!(
        mon.status,
        SessionStatus::BeforePlan,
        "a day before the plan start is not a missed session"
    );
    assert_eq!(vm.program.as_ref().unwrap().week, 1, "week 1 at the start");

    // Two weeks later, with the full week in between completed, the summary
    // reports a later program week (the partial first week always advances).
    complete_plan_weeks(&app, &mut model, MON + 7, 1);
    app.update(
        Event::SetToday {
            epoch_day: MON + 2 + 14,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let vm2 = app.view(&model);
    assert!(
        vm2.program.as_ref().unwrap().week >= 2,
        "the program week must advance with completed weeks: {}",
        vm2.program.as_ref().unwrap().week
    );
}
