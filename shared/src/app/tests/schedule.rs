//! Week scheduling at the app level: moving a session (ADH-RESCHED-001),
//! completion-gated plan weeks (ADH-PROGRESS-001) and planned-vs-done
//! consistency (ADH-SELFMON-001).

use super::*;

fn row(app: &Engine, model: &Model, day: i64) -> SessionPlanView {
    app.view(model)
        .week_plan
        .into_iter()
        .find(|s| s.epoch_day == day)
        .expect("a week-strip row for the day")
}

fn move_session(app: &Engine, model: &mut Model, from: i64, to: i64) {
    app.update(
        Event::MoveSession {
            from_epoch_day: from,
            to_epoch_day: to,
        },
        model,
    )
    .expect_only_render();
}

#[test]
fn move_session_moves_todays_session_and_blocks_a_spacing_violation() {
    // Pure lifter, full-body Mon/Wed/Fri; today is Monday.
    let (app, mut model) = planned_strength_model(true);
    let monday = row(&app, &model, MON);
    assert!(!monday.items.is_empty(), "Monday is a lift day");
    // Only Sunday keeps ~48 h from the Wednesday/Friday full-body days.
    assert_eq!(monday.move_targets, vec![MON + 6]);

    // Saturday sits the day after Friday's full-body day: refused, with a reason.
    move_session(&app, &mut model, MON, MON + 5);
    let monday = row(&app, &model, MON);
    assert!(
        !monday.items.is_empty(),
        "a refused move leaves the session in place"
    );
    let note = monday.move_note.expect("the refusal is explained");
    assert!(note.contains("back-to-back"), "{note}");
    assert!(row(&app, &model, MON + 5).items.is_empty());

    // Sunday is fine: the session moves, Monday becomes a rest day.
    move_session(&app, &mut model, MON, MON + 6);
    let vm = app.view(&model);
    let monday = vm.week_plan.iter().find(|s| s.epoch_day == MON).unwrap();
    assert_eq!(monday.session_type, SessionType::Rest);
    assert_eq!(monday.move_note, None);
    let sunday = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON + 6)
        .unwrap();
    assert!(!sunday.items.is_empty(), "the session now lands on Sunday");
    assert_eq!(sunday.moved_from_epoch_day, Some(MON));
    // The next session is Wednesday's (Monday is rest now, Sunday is later).
    assert_eq!(vm.next_session.expect("next").epoch_day, MON + 2);

    // ClearPlan drops the moves with the plan.
    app.update(Event::ClearPlan, &mut model)
        .expect_only_render();
    assert!(model.session_moves.is_empty());
}

#[test]
fn a_week_with_too_few_sessions_repeats_without_penalty() {
    let (app, mut model) = planned_strength_model(true);
    // A whole week passes with nothing logged: the plan holds week 1.
    app.update(
        Event::SetToday {
            epoch_day: MON + 7,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert_eq!(vm.program.as_ref().unwrap().week, 1, "the week repeats");
    let c = vm.consistency.expect("consistency view");
    assert!(c.repeating_week);
    assert!(
        c.notes
            .iter()
            .any(|n| n.summary.starts_with("Repeating this plan week")
                && n.evidence.grade == Some(EvidenceGrade::ExpertOpinion))
    );
    assert!(
        c.notes
            .iter()
            .any(|n| n.summary.starts_with("No sessions last week")),
        "a silent week prompts the re-entry setting"
    );

    // Two of the three planned days done → the week counts, the plan moves on.
    for day in [MON, MON + 2] {
        app.update(
            log_set("Back Squat", 100.0, 3, 7.0, day * DAY_SEC + 12 * 3_600),
            &mut model,
        )
        .expect_only_render();
    }
    let vm = app.view(&model);
    assert_eq!(
        vm.program.as_ref().unwrap().week,
        2,
        "2 of 3 completes the week"
    );
    let c = vm.consistency.expect("consistency view");
    assert!(!c.repeating_week);
    assert_eq!(c.weeks.len(), 1);
    assert_eq!((c.weeks[0].done, c.weeks[0].planned), (2, 3));
    assert_eq!(c.share_pct, Some(66));
}

#[test]
fn consistency_counts_done_vs_planned_with_no_streak_or_loss_copy() {
    let (app, model) = planned_strength_model(true);
    let c = app.view(&model).consistency.expect("consistency view");
    assert_eq!((c.this_week_done, c.this_week_planned), (0, 3));
    assert_eq!(
        c.notes[0].summary,
        "0 of 3 planned sessions done this week."
    );
    assert_eq!(c.notes[0].evidence.grade, Some(EvidenceGrade::Moderate));
    for n in &c.notes {
        let s = n.summary.to_lowercase();
        for banned in ["streak", "lost", "fail", "guilt", "behind"] {
            assert!(!s.contains(banned), "{banned:?} in {s}");
        }
    }
}

#[test]
fn missed_key_session_emits_reschedule_guidance_without_guilt() {
    let (app, mut model) = planned_strength_model(true);
    app.update(
        Event::SetToday {
            epoch_day: MON + 1,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let monday = vm.week_plan.iter().find(|s| s.epoch_day == MON).unwrap();
    assert_eq!(monday.status, SessionStatus::Missed);
    assert_eq!(monday.move_targets, vec![MON + 6], "make it up on Sunday");
    let c = vm.consistency.expect("consistency view");
    let note = c
        .notes
        .iter()
        .find(|n| n.summary.starts_with("Missed Monday's"))
        .expect("missed-session coaching");
    assert!(
        note.summary.contains("Move it to Sunday"),
        "{}",
        note.summary
    );
    // The reasoning sits behind the "?", not on the face.
    assert!(
        !note.summary.contains("Never double up"),
        "{}",
        note.summary
    );
    assert!(
        note.why.basis.contains("Never double up"),
        "{}",
        note.why.basis
    );
    assert!(!note.evidence.citation.is_empty());
}

#[test]
fn set_profile_cleans_implausible_planning_fields_and_the_view_renders() {
    let app = Engine;
    let mut model = Model::default();
    let poisoned = Profile {
        goal_distance: GoalDistance::TenK,
        running_days_per_week: 3,
        running_km_per_week: 30.0,
        lift_days_per_week: Some(9),
        training_weekdays: Some(0x80),
        goal_race_epoch_day: Some(i64::MAX),
        meet_epoch_day: Some(i64::MIN),
        layoff_returned_epoch_day: Some(9_000_000),
        recent_race: Some(RecentRace {
            distance_km: f64::NAN,
            time_sec: 3000,
            epoch_day: None,
        }),
        stated_longest_recent_km: Some(f64::INFINITY),
        specialization_muscle: Some("spleen".into()),
        ..strength_profile()
    };
    app.update(Event::SetProfile(poisoned), &mut model)
        .expect_only_render();
    let p = model.profile.clone().expect("profile stored");
    assert_eq!(p.lift_days_per_week, None);
    assert_eq!(p.training_weekdays, None, "no weekday bit left → any day");
    assert_eq!(p.goal_race_epoch_day, None);
    assert_eq!(p.meet_epoch_day, None);
    assert_eq!(p.layoff_returned_epoch_day, None);
    assert_eq!(p.recent_race, None);
    assert_eq!(p.stated_longest_recent_km, None);
    assert_eq!(p.specialization_muscle, None);
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
    let vm = app.view(&model);
    assert_eq!(vm.week_plan.len(), 7, "the plan still renders");
    assert!(serde_json::to_string(&vm).is_ok());

    // Plausible values survive (a faster-than-2:00/km race and a stray high
    // weekday bit are cleaned; the rest is kept, the muscle canonicalized).
    let fine = Profile {
        lift_days_per_week: Some(4),
        training_weekdays: Some(0x80 | 0b0110_0000),
        goal_race_epoch_day: Some(20_600),
        recent_race: Some(RecentRace {
            distance_km: 10.0,
            time_sec: 3000,
            epoch_day: Some(i64::MAX),
        }),
        stated_longest_recent_km: Some(12.5),
        specialization_muscle: Some(" Glutes ".into()),
        ..strength_profile()
    };
    app.update(Event::SetProfile(fine), &mut model)
        .expect_only_render();
    let p = model.profile.clone().expect("profile stored");
    assert_eq!(p.lift_days_per_week, Some(4));
    assert_eq!(p.training_weekdays, Some(0b0110_0000));
    assert_eq!(p.goal_race_epoch_day, Some(20_600));
    let race = p.recent_race.expect("a plausible race is kept");
    assert_eq!(
        (race.distance_km, race.time_sec, race.epoch_day),
        (10.0, 3000, None)
    );
    assert_eq!(p.stated_longest_recent_km, Some(12.5));
    assert_eq!(p.specialization_muscle.as_deref(), Some("glutes"));
    let too_fast = Profile {
        recent_race: Some(RecentRace {
            distance_km: 10.0,
            time_sec: 600, // 1:00/km
            epoch_day: None,
        }),
        ..strength_profile()
    };
    app.update(Event::SetProfile(too_fast), &mut model)
        .expect_only_render();
    assert_eq!(model.profile.as_ref().unwrap().recent_race, None);
}

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
fn a_session_with_no_free_day_left_explains_why_it_cannot_move() {
    // Full-body Mon/Wed/Fri, today Wednesday: Thursday and Saturday sit next to
    // Friday's full-body day and Sunday next to Monday's, so nothing works.
    let (app, mut model) = planned_strength_model(true);
    set_today(&app, &mut model, MON + 2);
    let ns = app.view(&model).next_session.expect("today's session");
    assert_eq!(ns.epoch_day, MON + 2);
    assert!(ns.move_targets.is_empty(), "{:?}", ns.move_targets);
    let reason = ns
        .move_blocked_reason
        .expect("the empty picker is explained");
    assert!(reason.contains("back-to-back"), "{reason}");
    assert!(reason.contains("Thursday"), "{reason}");
    // A done or rest day offers no move and no reason.
    let tue = row(&app, &model, MON + 1);
    assert!(tue.move_targets.is_empty() && tue.move_blocked_reason.is_none());
}

#[test]
fn days_before_a_midweek_plan_start_are_neither_planned_nor_counted() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON + 2,
        },
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, MON + 2);
    let vm = app.view(&model);
    for day in [MON, MON + 1] {
        let r = vm.week_plan.iter().find(|s| s.epoch_day == day).unwrap();
        assert_eq!(r.status, SessionStatus::BeforePlan, "day {day}");
        assert!(r.items.is_empty() && r.move_targets.is_empty());
    }
    let strip = vm
        .week_plan
        .iter()
        .filter(|s| s.session_type != SessionType::Rest && !s.items.is_empty())
        .count();
    let c = vm.consistency.expect("consistency view");
    assert_eq!(
        usize::from(c.this_week_planned),
        strip,
        "the header counts exactly the training days the strip shows"
    );
    assert_eq!(
        c.notes[0].summary,
        format!("0 of {strip} planned sessions done this week.")
    );
}

#[test]
fn a_swapped_run_and_lift_credit_both_planned_sessions() {
    // A hybrid week; find two adjacent planned days, one lift and one run, that
    // leave a later day in the same week to look back from. The plan then
    // starts on the first of them, so no earlier missed day competes for the
    // swapped work.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, MON);
    let week = app.view(&model).week_plan;
    let kind = |day: i64| {
        week.iter()
            .find(|s| s.epoch_day == day)
            .map(|s| s.session_type)
            .unwrap_or_default()
    };
    let (lift_day, run_day) = (MON..MON + 5)
        .find_map(|d| match (kind(d), kind(d + 1)) {
            (SessionType::Lift(_), SessionType::Run(_)) => Some((d, d + 1)),
            (SessionType::Run(_), SessionType::Lift(_)) => Some((d + 1, d)),
            _ => None,
        })
        .expect("a hybrid week places a lift next to a run");
    let first = lift_day.min(run_day);
    app.update(Event::ClearPlan, &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: first,
        },
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, first);
    let week = app.view(&model).week_plan;
    let kind = |day: i64| {
        week.iter()
            .find(|s| s.epoch_day == day)
            .map(|s| s.session_type)
            .unwrap_or_default()
    };
    assert!(
        matches!(kind(lift_day), SessionType::Lift(_))
            && matches!(kind(run_day), SessionType::Run(_)),
        "the layout holds from a mid-week start"
    );
    let lift_items: Vec<(String, u8, u32, f64)> = week
        .iter()
        .find(|s| s.epoch_day == lift_day)
        .unwrap()
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
    assert!(!lift_items.is_empty());
    // The lift is done on the run day, the run on the lift day.
    for (ex, sets, reps, kg) in &lift_items {
        for k in 0..i64::from(*sets) {
            let at = run_day * DAY_SEC + 12 * 3_600 + k * 60;
            app.update(log_set(ex, *kg, *reps, 8.0, at), &mut model)
                .expect_only_render();
        }
    }
    app.update(
        log_run_at(30.0, 180.0, 70.0, lift_day * DAY_SEC + 13 * 3_600),
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, lift_day.max(run_day) + 1);
    let vm = app.view(&model);
    let lift = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == lift_day)
        .unwrap();
    let run = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == run_day)
        .unwrap();
    assert!(
        lift.status.is_done(),
        "the planned lift is credited: {:?} {:?}",
        lift.status,
        lift.items
            .iter()
            .map(|i| (&i.exercise, i.sets, i.sets_done, i.load_kg))
            .collect::<Vec<_>>()
    );
    assert_eq!(lift.credited_from_epoch_day, Some(run_day));
    assert!(
        run.status.is_done(),
        "the planned run is credited: {:?}",
        run.status
    );
    assert_eq!(run.credited_from_epoch_day, Some(lift_day));
    let c = vm.consistency.expect("consistency view");
    assert_eq!(c.this_week_done, 2, "both swapped sessions count");
    assert!(
        !c.notes.iter().any(|n| n.summary.starts_with("Missed")),
        "{:?}",
        c.notes
    );
}

#[test]
fn no_missed_session_nudge_while_a_hold_is_active() {
    let (app, mut model) = planned_strength_model(true);
    set_today(&app, &mut model, MON + 1);
    let has_nudge = |app: &Engine, model: &Model| {
        app.view(model)
            .consistency
            .expect("consistency view")
            .notes
            .iter()
            .any(|n| n.summary.starts_with("Missed Monday's"))
    };
    assert!(has_nudge(&app, &model), "Monday was missed");
    // Flu reported: training is on hold, so no make-up suggestion.
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::Illness,
            value: 2.0,
            observed_at: (MON + 1) * DAY_SEC + 3_600,
            streak: 0,
            pain: None,
            effort_min: None,
        }),
        &mut model,
    )
    .expect_only_render();
    assert!(!has_nudge(&app, &model));
}

#[test]
fn a_new_plan_never_opens_on_a_rest_day() {
    // Whatever weekday setup finishes on, with "Any days" the
    // plan's first day holds a session.
    for profile in [strength_profile(), sample_profile()] {
        for start in MON..MON + 7 {
            let app = Engine;
            let mut model = Model::default();
            app.update(Event::SetProfile(profile.clone()), &mut model)
                .expect_only_render();
            app.update(
                Event::GeneratePlan {
                    start_epoch_day: start,
                },
                &mut model,
            )
            .expect_only_render();
            set_today(&app, &mut model, start);
            let first = row(&app, &model, start);
            assert!(
                !first.items.is_empty(),
                "start {start}: {:?}",
                first.session_type
            );
            let vm = app.view(&model);
            assert_eq!(vm.next_session.expect("next").epoch_day, start);
        }
    }
}

#[test]
fn a_rest_day_after_a_quiet_48_h_can_take_the_next_session() {
    // Nothing trained in the last days, so the next session
    // can come forward onto today's rest day.
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
    let mut checked = 0;
    for today in MON + 1..MON + 6 {
        set_today(&app, &mut model, today);
        let vm = app.view(&model);
        let rest_today = vm
            .week_plan
            .iter()
            .any(|s| s.epoch_day == today && s.items.is_empty());
        let Some(next) = vm
            .next_session
            .filter(|n| n.epoch_day > today && n.epoch_day < MON + 7)
        else {
            continue;
        };
        if rest_today {
            assert!(
                next.move_targets.contains(&today),
                "today {today}: {:?} {:?}",
                next.move_targets,
                next.move_blocked_reason
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "the hybrid week has a rest day before a session"
    );
}

#[test]
fn a_session_credited_from_a_lighter_day_reads_done_lighter() {
    // "✓ Mo": the swapped-in run covered 60 % of the plan, so the credited
    // row carries the lighter status, not plain done.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, MON);
    let week = app.view(&model).week_plan;
    let kind = |day: i64| {
        week.iter()
            .find(|s| s.epoch_day == day)
            .map(|s| s.session_type)
            .unwrap_or_default()
    };
    // The run day comes first, so its own plan never sees the run logged
    // the day after it.
    let (lift_day, run_day) = (MON..MON + 5)
        .find_map(|d| match (kind(d), kind(d + 1)) {
            (SessionType::Run(_), SessionType::Lift(_)) => Some((d + 1, d)),
            _ => None,
        })
        .expect("a hybrid week places a run before a lift");
    // Start on the first of the pair, so no earlier missed run day competes
    // for the spare run.
    let first = lift_day.min(run_day);
    app.update(Event::ClearPlan, &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: first,
        },
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, first);
    let run_item = row(&app, &model, run_day)
        .items
        .into_iter()
        .find(|i| i.is_run())
        .expect("the run item");
    let (km, min) = match crate::app::plan_views::run_planned(&run_item) {
        (Some(km), _) => (km * 0.6, km * 0.6 * 6.0),
        (None, Some(min)) => (min * 0.6 / 6.0, min * 0.6),
        (None, None) => panic!("a run with a single planned number"),
    };
    app.update(
        log_run_at(km, min, 70.0, lift_day * DAY_SEC + 13 * 3_600),
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, lift_day.max(run_day) + 1);
    let run = row(&app, &model, run_day);
    assert_eq!(run.credited_from_epoch_day, Some(lift_day));
    assert_eq!(run.status, SessionStatus::DoneLighter);
}
