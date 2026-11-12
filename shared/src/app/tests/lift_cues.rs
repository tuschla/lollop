//! In-session lift cues, readiness holds and lift statuses.

use super::*;
use crate::schema::{PainDetail, PainKind, PainTrend};

/// A set whose exercise [`log`] fills in.
fn set(weight_kg: f64, reps: u32, rpe: f64, at: i64) -> Event {
    log_set("", weight_kg, reps, rpe, at)
}

/// Log `ev` (a [`set`]) as `exercise`, keyed by its timestamp.
fn log(app: &Engine, model: &mut Model, exercise: &str, ev: Event) {
    let Event::LogSet {
        weight_kg,
        reps,
        rpe,
        observed_at,
        ..
    } = ev
    else {
        unreachable!()
    };
    app.update(
        with_entry_id(
            log_set(exercise, weight_kg, reps, rpe, observed_at),
            observed_at as u64,
        ),
        model,
    )
    .expect_only_render();
}

/// A low-HRV morning on `MON`: holds lift progression (AUTOREG-HRV-LIFT-001).
fn report_low_hrv(app: &Engine, model: &mut Model) {
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            observed_at: MON * DAY_SEC + 60,
            ..input(ReadinessSignal::HrvLnRmssd, -0.8)
        }),
        model,
    )
    .expect_only_render();
}

/// Today's first anchored (loaded) lift item.
fn loaded_item(app: &Engine, model: &Model) -> PrescriptionView {
    app.view(model)
        .next_session
        .expect("today's session")
        .items
        .into_iter()
        .find(|i| i.load_kg.is_some())
        .expect("an anchored lift")
}

fn rows_for<'a>(vm: &'a ViewModel, exercise: &str) -> Vec<&'a AdjustmentView> {
    vm.adjustments
        .iter()
        .filter(|a| a.exercise.as_deref() == Some(exercise))
        .collect()
}

#[test]
fn an_easy_first_set_above_the_plan_never_offers_a_lighter_add() {
    // The add is taken off the heavier of the planned load and the set
    // that felt easy, so the chip never lowers the bar below what was lifted.
    let (app, mut model) = planned_strength_model(true);
    let it = loaded_item(&app, &model);
    let planned = it.load_kg.unwrap();
    let first = planned + 10.0;
    let reps = u32::from(it.reps_low.max(1));
    log(
        &app,
        &mut model,
        &it.exercise,
        set(first, reps, 5.0, MON * DAY_SEC + 3_600),
    );
    let vm = app.view(&model);
    let add = rows_for(&vm, &it.exercise)
        .into_iter()
        .find(|a| a.summary.contains("felt easier"))
        .expect("an in-session add for the easy first set");
    let to = add.suggested_load_kg.expect("the add carries a load");
    assert!(
        to > first,
        "the add goes above the {first} kg set, got {to}"
    );

    // Already at that load: nothing left to add, so no add cue at all.
    log(
        &app,
        &mut model,
        &it.exercise,
        set(to, reps, 0.0, MON * DAY_SEC + 3_900),
    );
    let vm = app.view(&model);
    assert!(
        !rows_for(&vm, &it.exercise)
            .iter()
            .any(|a| a.summary.contains("felt easier")),
        "{:?}",
        vm.adjustments
    );
}

#[test]
fn a_later_short_set_withdraws_the_first_set_add() {
    // A set that missed reps after an easy first set overrides "add".
    let (app, mut model) = planned_strength_model(true);
    let it = loaded_item(&app, &model);
    let load = it.load_kg.unwrap();
    let reps = u32::from(it.reps_low.max(2));
    log(
        &app,
        &mut model,
        &it.exercise,
        set(load, reps, 5.0, MON * DAY_SEC + 3_600),
    );
    log(
        &app,
        &mut model,
        &it.exercise,
        set(load, reps - 1, 0.0, MON * DAY_SEC + 3_900),
    );
    let vm = app.view(&model);
    assert!(
        !rows_for(&vm, &it.exercise)
            .iter()
            .any(|a| a.summary.contains("felt easier")),
        "{:?}",
        vm.adjustments
    );
    let after = vm
        .next_session
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.exercise == it.exercise)
        .unwrap();
    assert!(
        !after.adjusted_note.contains("felt easy"),
        "{}",
        after.adjusted_note
    );
}

#[test]
fn a_finished_exercise_carries_no_remaining_sets_cue() {
    // A hard first set cuts the remaining sets; once every set is logged
    // there are no remaining sets, so the cue goes.
    let (app, mut model) = planned_strength_model(true);
    let it = loaded_item(&app, &model);
    let load = it.load_kg.unwrap();
    let reps = u32::from(it.reps_low.max(1));
    log(
        &app,
        &mut model,
        &it.exercise,
        set(load, reps, 10.0, MON * DAY_SEC + 3_600),
    );
    let vm = app.view(&model);
    assert!(
        rows_for(&vm, &it.exercise)
            .iter()
            .any(|a| a.summary.contains("remaining sets")),
        "a hard first set cuts the rest: {:?}",
        vm.adjustments
    );
    for k in 1..i64::from(it.sets.max(1)) {
        log(
            &app,
            &mut model,
            &it.exercise,
            set(load, reps, 0.0, MON * DAY_SEC + 3_600 + 300 * k),
        );
    }
    let vm = app.view(&model);
    let done = vm
        .next_session
        .as_ref()
        .and_then(|ns| ns.items.iter().find(|i| i.exercise == it.exercise))
        .is_none_or(|i| i.done);
    assert!(done, "every set logged");
    assert!(
        !rows_for(&vm, &it.exercise)
            .iter()
            .any(|a| a.summary.contains("remaining sets")),
        "{:?}",
        vm.adjustments
    );
}

#[test]
fn stacked_holds_apply_once_and_read_as_one_call() {
    // Two lift-holding signals (a low-HRV week and a low check-in today) add
    // one rep in reserve, not two, and their rows merge into one call naming
    // both.
    let (app, mut model) = planned_strength_model(true);
    report_low_hrv(&app, &mut model);
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            observed_at: MON * DAY_SEC + 60,
            ..input(
                ReadinessSignal::WellnessZ,
                crate::autoreg::WELLNESS_SINGLE_DAY_Z,
            )
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let bench = hero_item(&vm, "Bench press");
    assert_eq!(
        bench.intensity_label, "RIR 4",
        "RIR 3 one rep earlier, not two"
    );
    assert_eq!(bench.adjusted_note, "no weight added, stop 1 rep earlier");
    let holds: Vec<&str> = vm
        .adjustments
        .iter()
        .map(|a| a.summary.as_str())
        .filter(|s| s.contains("lifts add no weight"))
        .collect();
    assert_eq!(holds.len(), 1, "the second hold merges into the first row");
    assert!(
        holds[0].starts_with("HRV has been below your normal range this week and your check-in"),
        "{}",
        holds[0]
    );
    assert!(
        holds[0].contains("lifts add no weight and stop 1 rep earlier"),
        "{}",
        holds[0]
    );
}

/// A sharp left-knee pain report on `MON` at `at`: stops the lifts that load
/// the knee (SAFE-PAIN-STRUCT-001).
fn knee_pain(at: i64) -> Event {
    Event::SubmitReadiness(ReadinessInput {
        observed_at: at,
        pain: Some(PainDetail {
            kind: PainKind::SharpJoint,
            severity: Some(6),
            trend: PainTrend::Stable,
            persists: false,
            location: Some("Left knee".into()),
        }),
        ..input(ReadinessSignal::Pain, 1.0)
    })
}

/// Today's item for `exercise`.
fn hero_item<'a>(vm: &'a ViewModel, exercise: &str) -> &'a PrescriptionView {
    vm.next_session
        .as_ref()
        .and_then(|ns| ns.items.iter().find(|i| i.exercise == exercise))
        .unwrap_or_else(|| panic!("{exercise} today"))
}

#[test]
fn a_held_lift_goes_no_heavier_than_already_lifted_for_its_reps() {
    // ready-upd-001 ("no load added"): a pain stop holds the lifts it spares
    // at or under the heaviest weight already lifted for the prescribed
    // reps: the bench planned at 82.5 kg stays at the 80 kg already benched.
    let (app, mut model) = planned_on(
        strength_profile(),
        [
            log_set("Back Squat", 120.0, 3, 8.0, (MON - 1) * DAY_SEC),
            log_set("Bench press", 80.0, 5, 8.0, (MON - 1) * DAY_SEC),
        ],
        MON,
    );
    let planned = hero_item(&app.view(&model), "Bench press").load_kg;
    assert_eq!(planned, Some(82.5), "the plan adds load");
    app.update(knee_pain(MON * DAY_SEC + 60), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    let bench = hero_item(&vm, "Bench press");
    assert_eq!(bench.load_kg, Some(80.0));
    assert_eq!(bench.adjusted_note, "no weight added");
}

#[test]
fn a_hold_caps_today_from_the_recent_sets_of_matching_reps() {
    // The cap comes from same-movement sets of at least the
    // prescribed reps before today.
    let (app, mut model) = planned_strength_model(true);
    let it = loaded_item(&app, &model);
    let load = it.load_kg.unwrap();
    let reps = u32::from(it.reps_low.max(1));
    let lifted = load - 10.0;
    log(
        &app,
        &mut model,
        &it.exercise,
        set(lifted, reps, 8.0, (MON - 2) * DAY_SEC),
    );
    let before = loaded_item(&app, &model).load_kg.unwrap();
    assert!(before > lifted, "the plan adds load: {before} vs {lifted}");
    report_low_hrv(&app, &mut model);
    let held = app
        .view(&model)
        .next_session
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.exercise == it.exercise)
        .unwrap();
    // The pre-plan 120 kg × 3 baseline counts only for a ≤3-rep target.
    let cap = if reps <= 3 { lifted.max(120.0) } else { lifted };
    assert_eq!(held.load_kg, Some(before.min(cap)), "planned {before}");
}

#[test]
fn short_reps_at_the_planned_load_read_done_short() {
    // Missed reps at the planned load are "short", not "lighter".
    let (app, mut model) = planned_strength_model(true);
    let it = loaded_item(&app, &model);
    let load = it.load_kg.unwrap();
    let reps = u32::from(it.reps_low);
    let rpe = it.target_rpe.unwrap_or(7.0);
    for k in 0..i64::from(it.sets) {
        let r = if k + 1 == i64::from(it.sets) {
            reps - 1
        } else {
            reps
        };
        log(
            &app,
            &mut model,
            &it.exercise,
            set(load, r, rpe, MON * DAY_SEC + 3_600 + 300 * k),
        );
    }
    let vm = app.view(&model);
    let done = hero_item(&vm, &it.exercise);
    assert_eq!(done.status, ItemStatus::DoneShort, "{:?}", done.logged_sets);
}

#[test]
fn a_short_set_on_the_hero_marks_its_logged_set_short() {
    // The logged set carries `short`, the load-independent flag.
    let (app, mut model) = planned_strength_model(true);
    let it = loaded_item(&app, &model);
    let load = it.load_kg.unwrap();
    let reps = u32::from(it.reps_low.max(2));
    log(
        &app,
        &mut model,
        &it.exercise,
        set(load, reps - 1, 0.0, MON * DAY_SEC + 3_600),
    );
    let after = loaded_item(&app, &model);
    let s = &after.logged_sets[0];
    assert!(s.qualifies && s.short && !s.lighter, "{s:?}");
}

#[test]
fn a_pain_report_after_work_reads_stopped_on_the_hero() {
    let (app, mut model) = planned_strength_model(true);
    let it = loaded_item(&app, &model);
    let load = it.load_kg.unwrap();
    let reps = u32::from(it.reps_low.max(1));
    log(
        &app,
        &mut model,
        &it.exercise,
        set(load, reps, 0.0, MON * DAY_SEC + 3_600),
    );
    app.update(knee_pain(MON * DAY_SEC + 3_700), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    let squat = hero_item(&vm, &it.exercise);
    assert_eq!(squat.status, ItemStatus::StoppedPain, "{squat:?}");
}

#[test]
fn the_two_heavy_days_of_a_week_read_apart() {
    // The 5–6-rep second heavy exposure is the "Heavy volume day".
    let (app, model) = planned_strength_model(true);
    let titles: Vec<String> = app
        .view(&model)
        .week_plan
        .into_iter()
        .filter(|s| matches!(s.session_type, SessionType::Lift(_)))
        .map(|s| s.title)
        .collect();
    assert!(titles.iter().any(|t| t == "Heavy day"), "{titles:?}");
    assert!(titles.iter().any(|t| t == "Heavy volume day"), "{titles:?}");
}

#[test]
fn a_hybrid_plan_is_named_for_its_lifting_goal() {
    // A muscle-goal hybrid reads "Muscle building + running".
    let name = |p: Profile| {
        let (app, model) = planned_on(p, [], MON);
        app.view(&model).program.expect("a program").name
    };
    assert_eq!(
        name(Profile {
            lift_goal: LiftGoal::Hypertrophy,
            ..sample_profile()
        }),
        "Muscle building + running"
    );
    assert_eq!(
        name(Profile {
            lift_goal: LiftGoal::Power,
            ..calisthenics_profile()
        }),
        "Calisthenics power"
    );
}

#[test]
fn a_row_with_no_bar_or_rings_says_to_use_a_table() {
    // CAL-EQUIP-001: "table/inverted rows for pulling".
    let p = Profile {
        calis_equipment: Some(CalisEquipment {
            pull_up_bar: false,
            dip_bars: false,
            rings: false,
        }),
        ..calisthenics_profile()
    };
    let (app, mut model) = planned_on(p, [], MON);
    let row = |model: &Model| {
        app.view(model)
            .next_session
            .expect("today's session")
            .items
            .into_iter()
            .find(|it| {
                crate::calisthenics::tier_by_name(&it.exercise).map(|(l, _)| l)
                    == Some(Lad::HorizontalPull)
            })
            .expect("a row today")
    };
    let view = row(&model);
    assert_eq!(view.effort_cue, "use a sturdy table edge");
    assert!(view.summary.contains("sturdy table"), "{}", view.summary);
    assert_eq!(
        view.evidence,
        EvidenceTagView::of(&Recommended::new((), claims::CAL_EQUIP_001))
    );
    // The compact row shows only the lead, so the setup rides it, also after
    // an adjustment (the e1RM gate's RPE cap) re-renders the face.
    assert!(
        view.face_lead.ends_with("· use a sturdy table edge"),
        "{}",
        view.face_lead
    );
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            observed_at: MON * DAY_SEC + 60,
            ..input(ReadinessSignal::EstimatedOneRm, 0.93)
        }),
        &mut model,
    )
    .expect_only_render();
    let eased = row(&model);
    assert_ne!(
        eased.face_detail, view.face_detail,
        "the cap re-rendered the face"
    );
    assert!(
        eased.face_lead.ends_with("· use a sturdy table edge"),
        "{}",
        eased.face_lead
    );
    assert_eq!(
        eased.face_detail.matches("sturdy table").count(),
        1,
        "{}",
        eased.face_detail
    );
}

#[test]
fn the_first_set_clause_goes_once_a_set_is_logged() {
    // "Log your first work set…" is spent once one is logged today.
    let (app, mut model) = planned_strength_model(true);
    report_low_hrv(&app, &mut model);
    let clause = "Log your first work set";
    let has_clause = |vm: &ViewModel| vm.adjustments.iter().any(|a| a.summary.contains(clause));
    assert!(
        has_clause(&app.view(&model)),
        "the hold asks for the first set"
    );
    let it = loaded_item(&app, &model);
    let reps = u32::from(it.reps_low.max(1));
    log(
        &app,
        &mut model,
        &it.exercise,
        set(it.load_kg.unwrap(), reps, 8.0, MON * DAY_SEC + 3_600),
    );
    let vm = app.view(&model);
    assert!(!has_clause(&vm), "{:?}", vm.adjustments);
    assert!(
        vm.adjustments
            .iter()
            .any(|a| a.summary.contains("lifts add no weight")),
        "the hold itself stays: {:?}",
        vm.adjustments
    );
}

#[test]
fn the_first_set_clause_stays_spent_once_the_session_is_finished() {
    // A finished session hands the hero to the next training day; the clause
    // still reads today's own row.
    let (app, mut model) = planned_strength_model(true);
    report_low_hrv(&app, &mut model);
    let clause = "Log your first work set";
    let has_clause = |vm: &ViewModel| vm.adjustments.iter().any(|a| a.summary.contains(clause));
    let vm = app.view(&model);
    assert!(has_clause(&vm), "the hold asks for the first set");
    let ns = vm.next_session.expect("today's session");
    assert_eq!(ns.epoch_day, MON);
    let mut at = MON * DAY_SEC + 3_600;
    for it in ns.items.iter().filter(|it| !it.is_run()) {
        for _ in 0..it.sets {
            let reps = u32::from(it.reps_high.max(1));
            log(
                &app,
                &mut model,
                &it.exercise,
                set(it.load_kg.unwrap_or(60.0), reps, 7.0, at),
            );
            at += 120;
        }
    }
    let vm = app.view(&model);
    assert_ne!(
        vm.next_session.as_ref().map(|s| s.epoch_day),
        Some(MON),
        "the finished session hands the hero on"
    );
    assert!(!has_clause(&vm), "{:?}", vm.adjustments);
}

#[test]
fn a_short_lift_note_names_the_day_and_expires_on_its_next_session() {
    // "Came up short on <lift> on <day>: k of n sets (did instead of
    // planned)", gone once that lift's next session is today.
    let (app, mut model) = planned_strength_model(true);
    let week = app.view(&model).week_plan;
    let lift_days: Vec<&SessionPlanView> = week
        .iter()
        .filter(|s| matches!(s.session_type, SessionType::Lift(_)))
        .collect();
    let first = lift_days[0];
    assert_eq!(first.epoch_day, MON);
    let it = first
        .items
        .iter()
        .find(|i| i.load_kg.is_some())
        .expect("an anchored lift")
        .clone();
    let load = it.load_kg.unwrap();
    let lighter = load - 5.0;
    let reps = u32::from(it.reps_low);
    let sets = i64::from(it.sets);
    for k in 0..(sets + 1) / 2 {
        log(
            &app,
            &mut model,
            &it.exercise,
            set(lighter, reps, 0.0, MON * DAY_SEC + 3_600 + 300 * k),
        );
    }
    // Every other exercise of the day done as planned, so the day is done.
    for (j, other) in first.items.iter().enumerate() {
        if other.exercise == it.exercise || other.is_run() {
            continue;
        }
        let kg = other.load_kg.or(other.suggested_load_kg).unwrap_or(40.0);
        for k in 0..i64::from(other.sets) {
            let at = MON * DAY_SEC + 7_200 + 3_600 * j as i64 + 300 * k;
            log(
                &app,
                &mut model,
                &other.exercise,
                set(kg, u32::from(other.reps_low.max(1)), 0.0, at),
            );
        }
    }
    let next = lift_days
        .iter()
        .skip(1)
        .find(|s| s.items.iter().any(|i| i.exercise == it.exercise))
        .expect("the lift again later in the week")
        .epoch_day;
    let off_day = (MON + 1..next)
        .find(|d| !lift_days.iter().any(|s| s.epoch_day == *d))
        .expect("a day between the two lift days");
    let feedback_on = |model: &mut Model, day: i64| {
        set_today(&app, model, day);
        app.view(model)
            .feedback
            .map(|f| f.message)
            .unwrap_or_default()
    };
    let msg = feedback_on(&mut model, off_day);
    let lb = |kg: f64| fmt_weight(kg, false);
    let expected = format!(
        "Came up short on {} on Monday: {} of {} sets ({reps} reps at {} instead of {} at {})",
        it.exercise,
        (sets + 1) / 2,
        it.sets,
        lb(lighter),
        it.reps_low,
        lb(load)
    );
    assert!(msg.starts_with(&expected), "{msg}\nexpected: {expected}");
    let msg = feedback_on(&mut model, next);
    assert!(!msg.contains("Came up short"), "{msg}");
}

#[test]
fn the_hero_hold_line_drops_the_first_set_clause_once_a_set_is_logged() {
    // The session's own hold line follows the readiness row.
    let (app, mut model) = planned_strength_model(true);
    report_low_hrv(&app, &mut model);
    let clause = "Log your first work set";
    let hero_line = |vm: &ViewModel| {
        vm.next_session
            .as_ref()
            .and_then(|ns| ns.adjustment.as_ref())
            .map(|a| a.summary.clone())
            .unwrap_or_default()
    };
    assert!(hero_line(&app.view(&model)).contains(clause));
    let it = loaded_item(&app, &model);
    log(
        &app,
        &mut model,
        &it.exercise,
        set(
            it.load_kg.unwrap(),
            u32::from(it.reps_low.max(1)),
            8.0,
            MON * DAY_SEC + 3_600,
        ),
    );
    let vm = app.view(&model);
    let line = hero_line(&vm);
    assert!(!line.is_empty() && !line.contains(clause), "{line}");
    let row = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .and_then(|s| s.adjustment.as_ref())
        .map(|a| a.summary.as_str())
        .unwrap_or_default();
    assert!(!row.contains(clause), "{row}");
}

#[test]
fn a_later_failed_set_gets_a_stop_or_drop_cue() {
    // A set after the first that went to failure (missed
    // reps) while sets remain cues "end here or drop the weight"; a good set
    // after it clears the cue.
    let (app, mut model) = planned_strength_model(true);
    let it = loaded_item(&app, &model);
    assert!(it.sets >= 4, "room for three sets and more: {}", it.sets);
    let load = it.load_kg.unwrap();
    let reps = u32::from(it.reps_low.max(2));
    let rpe = it.target_rpe.unwrap_or(8.0);
    let stop = |vm: &ViewModel| {
        rows_for(vm, &it.exercise)
            .iter()
            .any(|a| a.summary.contains("went to failure"))
    };
    log(
        &app,
        &mut model,
        &it.exercise,
        set(load, reps, rpe, MON * DAY_SEC + 3_600),
    );
    assert!(
        !stop(&app.view(&model)),
        "a first set as planned: no stop cue"
    );
    log(
        &app,
        &mut model,
        &it.exercise,
        set(load, reps - 1, 10.0, MON * DAY_SEC + 3_900),
    );
    let vm = app.view(&model);
    assert!(stop(&vm), "{:?}", vm.adjustments);
    let cue = rows_for(&vm, &it.exercise)
        .into_iter()
        .find(|a| a.summary.contains("went to failure"))
        .expect("the stop cue");
    assert!(
        cue.suggests_end_exercise,
        "the set loop offers End exercise: {cue:?}"
    );
    let item = vm
        .next_session
        .as_ref()
        .and_then(|ns| ns.items.iter().find(|i| i.exercise == it.exercise))
        .expect("the lift");
    assert!(
        item.adjusted_note.contains("end the exercise here"),
        "{}",
        item.adjusted_note
    );
    log(
        &app,
        &mut model,
        &it.exercise,
        set(load, reps, rpe, MON * DAY_SEC + 4_200),
    );
    assert!(
        !stop(&app.view(&model)),
        "a good set after it clears the cue"
    );
}

/// Monday's session with its first lift's first set logged hard and the rest
/// of it at the cue load, every other item as shown. Returns the cut lift.
fn finish_at_the_cue_load(app: &Engine, model: &mut Model) -> PrescriptionView {
    let it = loaded_item(app, model);
    let load = it.load_kg.unwrap();
    let reps = u32::from(it.reps_low.max(1));
    log(
        app,
        model,
        &it.exercise,
        set(load, reps, 10.0, MON * DAY_SEC + 3_600),
    );
    let cue = rows_for(&app.view(model), &it.exercise)
        .iter()
        .find_map(|a| a.suggested_load_kg)
        .expect("the cut carries its load");
    assert!(cue < load - 1.0, "{cue} vs {load}");
    let mut at = MON * DAY_SEC + 3_900;
    for _ in 1..it.sets {
        log(app, model, &it.exercise, set(cue, reps, 8.0, at));
        at += 300;
    }
    let others = app.view(model).next_session.expect("still today").items;
    for other in others.iter().filter(|o| o.exercise != it.exercise) {
        if other.is_run() {
            app.update(log_run_at(30.0, 180.0, 70.0, at), model)
                .expect_only_render();
            at += 300;
            continue;
        }
        let kg = other.load_kg.unwrap_or(50.0);
        for _ in 0..other.sets.max(1) {
            log(
                app,
                model,
                &other.exercise,
                set(kg, u32::from(other.reps_low.max(1)), 8.0, at),
            );
            at += 300;
        }
    }
    it
}

#[test]
fn a_lift_finished_at_the_cue_load_reads_done() {
    // A hard first set cuts the remaining sets; the rest done
    // at the cue load completes the lift, also once the session is finished
    // and no longer the hero.
    let (app, mut model) = planned_strength_model(true);
    let it = finish_at_the_cue_load(&app, &mut model);
    let vm = app.view(&model);
    let row = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .expect("Monday");
    let lift = row
        .items
        .iter()
        .find(|i| i.exercise == it.exercise)
        .expect("the cut lift");
    assert_eq!(lift.status, ItemStatus::Done, "{:?}", lift.logged_sets);
    let done = vm.session_done.expect("session done");
    let line = done
        .items
        .iter()
        .find(|i| i.exercise == it.exercise)
        .expect("its line");
    assert_eq!(line.status, ItemStatus::Done, "{line:?}");
}

#[test]
fn an_adjusted_today_reads_the_same_in_the_hero_and_its_strip_row() {
    // A readiness-adjusted day whose lift is logged above its target: the
    // first-set re-judge eases the lift on the hero, and today's strip row
    // is that same session, not the row before the re-judge.
    let (app, mut model) = planned_strength_model(true);
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            observed_at: MON * DAY_SEC + 60,
            ..input(ReadinessSignal::Rpe, 2.0)
        }),
        &mut model,
    )
    .expect_only_render();
    let it = loaded_item(&app, &model);
    let rpe = it.target_rpe.expect("a target") + 1.0;
    let mut at = MON * DAY_SEC + 3_600;
    for _ in 0..it.sets {
        log(
            &app,
            &mut model,
            &it.exercise,
            set(it.load_kg.unwrap(), u32::from(it.reps_low.max(1)), rpe, at),
        );
        at += 240;
    }
    let vm = app.view(&model);
    let hero = vm.next_session.as_ref().expect("today's session");
    assert_eq!(hero.epoch_day, MON);
    assert_eq!(hero.status, SessionStatus::Adjusted);
    let row = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .expect("Monday");
    let face = |s: &SessionPlanView| {
        s.items
            .iter()
            .find(|i| i.exercise == it.exercise)
            .map(|i| (i.target_rpe, i.face_detail.clone()))
    };
    assert_eq!(face(row), face(hero));
    assert_eq!(
        serde_json::to_string(row).unwrap(),
        serde_json::to_string(hero).unwrap()
    );
}

#[test]
fn a_held_lift_restates_its_load_in_the_why_basis() {
    // The session `?` panel reads the held load, not the planned one. A
    // heavier single lifts the plan above the 120 kg × 3 baseline, which stays
    // the most lifted for the squat's triples, so a low-HRV hold keeps 120 kg.
    let (app, mut model) = planned_strength_model(true);
    let squat = loaded_item(&app, &model).exercise;
    log(
        &app,
        &mut model,
        &squat,
        set(170.0, 1, 10.0, (MON - 2) * DAY_SEC),
    );
    let planned = loaded_item(&app, &model).load_kg.unwrap();
    assert!(planned > 120.0, "{planned}");
    report_low_hrv(&app, &mut model);
    let held = loaded_item(&app, &model);
    assert_eq!(held.load_kg, Some(120.0));
    let basis = &held.why.basis;
    assert!(
        basis.contains(&format!(": {planned} kg planned, held at 120 kg today")),
        "{basis}"
    );
    assert!(!basis.contains(&format!(": {planned} kg.")), "{basis}");
}

#[test]
fn a_lighter_week_restates_sets_and_reps_left_in_the_why_basis() {
    // The `?` basis restates the sets and the reps left with the weight, so
    // it reads the prescription the face shows: a lighter week halves the
    // squat's 5×3 and takes 10 % off its 117.5 kg, a low-HRV hold leaves one
    // more rep in reserve than its ~3.
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
    report_low_hrv(&app, &mut model);
    let it = loaded_item(&app, &model);
    assert_eq!(it.face_lead, "3×3 @ 105 kg");
    assert!(
        it.face_detail.ends_with("~4 reps left"),
        "{}",
        it.face_detail
    );
    assert!(
        it.why.basis.starts_with(
            "3×3 at 85% of your estimated max (140 kg): 117.5 kg planned, 105 kg today. Stop each set with ~4 reps left."
        ),
        "{}",
        it.why.basis
    );
}

/// The load the session `?` panel states for today in a lift's why? basis
/// ("…: 100 kg." or "…: 100 kg planned, [held at] 95 kg today").
fn panel_load(basis: &str) -> &str {
    if let Some((_, rest)) = basis.split_once(" planned, ") {
        let rest = rest.strip_prefix("held at ").unwrap_or(rest);
        return rest.split_once(" today").expect(basis).0;
    }
    let (_, rest) = basis.split_once(": ").expect(basis);
    rest.split_once(". ")
        .map_or(rest.trim_end_matches('.'), |(l, _)| l)
}

/// Every loaded lift of today's hero: the panel states the hero's load and
/// its last-time set.
fn assert_panel_states_hero_loads(model: &Model) {
    let ns = Engine.view(model).next_session.expect("hero");
    let mut checked = 0;
    for it in ns.items.iter().filter(|i| !i.is_run()) {
        let Some(kg) = it.load_kg else { continue };
        let basis = &it.why.basis;
        assert_eq!(
            panel_load(basis),
            fmt_weight(kg, false),
            "{}: {basis}",
            it.exercise
        );
        // A face whose "Last: 60 kg × 8" line names a single set already shows
        // it, so the basis never repeats it.
        let last_on_face = it.anchored_on.starts_with("Last: ") && !it.anchored_on.contains(',');
        if let Some(last) = it.last_set.as_ref().filter(|_| !last_on_face) {
            let last_time = format!("Last time: {} ×", fmt_weight(last.weight_kg, false));
            assert!(basis.contains(&last_time), "{}: {basis}", it.exercise);
        }
        // The same sets×reps and reps left as the face, not the plan's
        // pre-adjustment ones.
        let lead = it.face_lead.split(" @ ").next().unwrap_or_default();
        assert!(
            basis.starts_with(&format!("{lead} ")),
            "{}: {lead} vs {basis}",
            it.exercise
        );
        if let Some(n) = it
            .face_detail
            .split('~')
            .nth(1)
            .and_then(|s| s.split(' ').next())
        {
            assert!(
                basis.contains(&format!("about {n} rep")) || basis.contains(&format!("~{n} rep")),
                "{}: ~{n} vs {basis}",
                it.exercise
            );
        }
        checked += 1;
    }
    assert!(checked >= 1, "a loaded lift on the hero");
}

#[test]
fn the_session_panel_states_the_hero_loads_through_holds_and_cues() {
    // The `?` panel's per-lift load and "last time" are the
    // hero's, as planned, under a hold, and after an in-session cut.
    let (app, mut model) = planned_strength_model(true);
    let it = loaded_item(&app, &model);
    let load = it.load_kg.unwrap();
    let reps = u32::from(it.reps_low.max(1));
    assert!(reps > 1, "a single never caps a {reps}-rep item");
    // A heavier single lifts the estimated max (and the plan) above the
    // 120 kg × 3 baseline, which stays the hold cap for the item's reps.
    log(
        &app,
        &mut model,
        &it.exercise,
        set(170.0, 1, 10.0, (MON - 2) * DAY_SEC),
    );
    assert_panel_states_hero_loads(&model);
    let planned_kg = loaded_item(&app, &model).load_kg.unwrap();

    report_low_hrv(&app, &mut model);
    let held = app.view(&model);
    let held_kg = held
        .next_session
        .as_ref()
        .and_then(|ns| ns.items.iter().find(|i| i.exercise == it.exercise))
        .and_then(|i| i.load_kg)
        .unwrap();
    assert!(
        held_kg < planned_kg,
        "held {held_kg} < planned {planned_kg}"
    );
    assert_panel_states_hero_loads(&model);

    let (app, mut model) = planned_strength_model(true);
    log(
        &app,
        &mut model,
        &it.exercise,
        set(load, reps, 10.0, MON * DAY_SEC + 3_600),
    );
    let cut_kg = app
        .view(&model)
        .next_session
        .as_ref()
        .and_then(|ns| ns.items.iter().find(|i| i.exercise == it.exercise))
        .and_then(|i| i.load_kg)
        .unwrap();
    assert!(cut_kg < load, "the hard first set cuts the rest: {cut_kg}");
    assert_panel_states_hero_loads(&model);
}

#[test]
fn a_later_set_spends_the_first_set_add() {
    // "Your first set felt easier" is the call for the next
    // set; once later sets are logged without the add, it is withdrawn.
    let (app, mut model) = planned_strength_model(true);
    let it = loaded_item(&app, &model);
    let load = it.load_kg.unwrap();
    let reps = u32::from(it.reps_low.max(1));
    let easier = |vm: &ViewModel| {
        rows_for(vm, &it.exercise)
            .iter()
            .any(|a| a.summary.contains("felt easier"))
    };
    log(
        &app,
        &mut model,
        &it.exercise,
        set(load, reps, 5.0, MON * DAY_SEC + 3_600),
    );
    assert!(
        easier(&app.view(&model)),
        "an easy first set offers the add"
    );
    log(
        &app,
        &mut model,
        &it.exercise,
        set(load, reps, 5.0, MON * DAY_SEC + 3_900),
    );
    let vm = app.view(&model);
    assert!(!easier(&vm), "{:?}", vm.adjustments);
    let item = vm
        .next_session
        .as_ref()
        .and_then(|ns| ns.items.iter().find(|i| i.exercise == it.exercise))
        .expect("the lift");
    assert!(
        !item.adjusted_note.contains("felt easy"),
        "{}",
        item.adjusted_note
    );
}

/// On an eased day the face says "pick a weight for ~N reps left"; the `?`
/// basis states the same reserve, not the planned harder one.
#[test]
fn eased_items_basis_states_the_face_reps_left() {
    let model = snapshot_fixture_model(include_str!(
        "../../../tests/fixtures/view_snapshot/pain_hold.events.ndjson"
    ));
    let ns = Engine.view(&model).next_session.expect("a session today");
    let mut checked = 0;
    for it in &ns.items {
        let Some(n) = it
            .face_lead
            .split("pick a weight for ~")
            .nth(1)
            .and_then(|s| s.split(' ').next())
        else {
            continue;
        };
        checked += 1;
        assert!(
            it.why.basis.contains(&format!("~{n} rep")),
            "{}: face ~{n} reps left, basis: {}",
            it.exercise,
            it.why.basis
        );
    }
    assert!(checked >= 1, "an eased pick-a-weight item today");
}
