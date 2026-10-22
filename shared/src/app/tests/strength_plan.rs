//! The strength plan: next session and anchors (memoized, preserved across
//! regeneration, RIR fallback), anchor liveness under continued training,
//! non-catalog isolation gating, the display-unit plate grid, the power-day
//! velocity cue, prefill targets and honest calisthenics session titles.

use super::*;
use crate::plan::{
    Anchors, e1rm_reliable_name, is_isolation_lift, movement_pattern, same_movement, synthesize,
};
use crate::schema::Prescription;

/// A dated bench set with no recorded RPE (so `effective_reps` == `reps`).
fn bench(weight_kg: f64, reps: u32, observed_at: i64, entry_id: u64) -> Event {
    with_entry_id(
        log_set("Bench", weight_kg, reps, 0.0, observed_at),
        entry_id,
    )
}

// ── an anchor stays alive while same-movement training continues ──

#[test]
fn anchor_survives_while_same_movement_training_continues() {
    // A 50-day-old heavy bench set (e1RM ≈ 116.7). WITH bridging, weekly 12-rep
    // sets (each above the 10-rep reliability cap, so none itself anchors) keep
    // the movement's detrain clock alive up to yesterday, holding the anchor at
    // full credit; WITHOUT them the same anchor ages 50 days and decays ~70%.
    // A Monday (epoch day 0 is a Thursday), so the plan's first week is not
    // all before the plan started.
    let mon = 501_i64;
    let build = |bridge: bool| -> Model {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(strength_profile()), &mut model)
            .expect_only_render();
        app.update(bench(100.0, 5, (mon - 50) * DAY_SEC, 1), &mut model)
            .expect_only_render();
        if bridge {
            for (k, d) in [43_i64, 36, 29, 22, 15, 8, 1].iter().enumerate() {
                app.update(
                    bench(60.0, 12, (mon - d) * DAY_SEC, 10 + k as u64),
                    &mut model,
                )
                .expect_only_render();
            }
        }
        app.update(
            Event::GeneratePlan {
                start_epoch_day: mon,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::SetToday {
                epoch_day: mon,
                utc_offset_sec: 0,
            },
            &mut model,
        )
        .expect_only_render();
        model
    };
    // Read the (deterministic) Heavy-day bench load: 85% of the anchor.
    let heavy_bench_load = |model: &Model| -> Option<f64> {
        let app = Engine;
        app.view(model)
            .week_plan
            .into_iter()
            .filter(|s| s.title == "Heavy day")
            .flat_map(|s| s.items.into_iter())
            .find(|i| i.exercise.to_lowercase().contains("bench") && i.load_kg.is_some())
            .and_then(|i| i.load_kg)
    };
    let bridged = heavy_bench_load(&build(true)).expect("the bench is anchored on the heavy set");
    let lapsed = heavy_bench_load(&build(false));
    // 0.85 × 116.7 ≈ 99 → 97.5 kg on the grid: near full, not decayed.
    assert!(
        bridged > 90.0,
        "continued training must hold the anchored load near full, got {bridged}"
    );
    // Control: with no bridging work the 50-day-old anchor is scaled to the
    // 4–8 wk detraining credit (70 %, DETRAIN-STR-001) — modest, never zero.
    let lapsed = lapsed.expect("a 50-day-old anchor is scaled, not dropped");
    assert!(
        lapsed < bridged && lapsed >= (bridged * 0.70 - 2.5),
        "an untrained 50-day-old anchor loads at ~70 % of the trained one: {lapsed} vs {bridged}"
    );
}

// ── anchored novice hypertrophy reps stay inside the e1RM window ──

#[test]
fn novice_anchored_hypertrophy_reps_stay_in_the_reliable_window() {
    let profile = Profile {
        lift_goal: LiftGoal::Hypertrophy,
        progression_cadence: ProgressionCadence::EverySession, // → Novice
        ..strength_profile()
    };
    let anchors = Anchors {
        lift_e1rm: vec![("Bench".to_string(), 116.7)],
        ..Anchors::default()
    };
    let prog = synthesize(&profile, &anchors, 0).expect("a strength profile plans");
    let bench = prog
        .mesocycle
        .sessions
        .iter()
        .flat_map(|s| &s.prescriptions)
        .find_map(|rx| match &rx.value {
            Prescription::Lift(l) if l.exercise.eq_ignore_ascii_case("Bench") => Some(l.clone()),
            _ => None,
        })
        .expect("an anchored bench prescription");
    // A plan-following set must stay inside the reliable e1RM window so it can
    // itself anchor: a band-high 12-rep set (effective reps ≥12) sits above the
    // cap and cannot refresh the anchor, so the cap keeps reps ≤ 10.
    assert!(
        bench.reps <= crate::strength::E1RM_RELIABLE_REP_CAP as u8,
        "anchored hypertrophy reps ({}) must be ≤ {} to anchor a plan-following set",
        bench.reps,
        crate::strength::E1RM_RELIABLE_REP_CAP
    );
    // …but still a hypertrophy-range set, not collapsed below the band.
    assert!(
        bench.reps >= 6,
        "reps stay inside the 6–12 hypertrophy band"
    );
}

// ── non-catalog isolation lifts don't anchor / lead the DUP week ──

#[test]
fn non_catalog_isolation_names_are_e1rm_unreliable() {
    for iso in [
        "Biceps curl",
        "Hammer curl",
        "Triceps pushdown",
        "Cable pressdown",
        "Lateral raise",
        "Front raise",
        "Cable fly",
        "Dumbbell flye",
        "Leg extension",
        "Triceps kickback",
        "Barbell shrug",
        // Strong needles classify even when the name also reads as a compound
        // (Push) pattern: Chest fly / Pec deck / plural flies.
        "Chest fly",
        "Pec deck",
        "Dumbbell flies",
    ] {
        assert!(is_isolation_lift(iso), "{iso} should read as isolation");
        assert!(!e1rm_reliable_name(iso), "{iso} e1RM is unreliable");
    }
    for compound in [
        "Barbell back squat",
        "Bench press",
        "Romanian deadlift",
        "Cable row",
        "Overhead press",
    ] {
        assert!(!is_isolation_lift(compound), "{compound} is a compound");
        assert!(e1rm_reliable_name(compound), "{compound} e1RM is reliable");
    }
    // A posterior-chain back extension is a HINGE, never mislabeled isolation
    // even though "extension" is an isolation keyword.
    assert!(movement_pattern("Back extension").is_some());
    assert!(!is_isolation_lift("Back extension"));
    assert!(!is_isolation_lift("Hyperextension"));
}

#[test]
fn a_lone_isolation_log_does_not_lead_the_dup_week() {
    let profile = Profile {
        lift_goal: LiftGoal::MaxStrength,
        ..strength_profile()
    };
    // Each name is the athlete's only logged lift; none may lead the DUP week.
    // "Chest fly"/"Pec deck" also carry a Push pattern, so they exercise the
    // unconditional strong-needle gate, not just the pattern-None arm.
    for iso in ["Biceps curl", "Chest fly", "Pec deck"] {
        let anchors = Anchors {
            lift_e1rm: vec![(iso.to_string(), 40.0)],
            ..Anchors::default()
        };
        let prog = synthesize(&profile, &anchors, 0).expect("a strength profile plans");
        let primary = prog
            .mesocycle
            .sessions
            .iter()
            .find_map(|s| match s.session_type {
                SessionType::Lift(_) => s.prescriptions.first(),
                _ => None,
            })
            .and_then(|rx| match &rx.value {
                Prescription::Lift(l) => Some(l.exercise.clone()),
                _ => None,
            })
            .expect("a lift day has a primary");
        assert!(
            !same_movement(&primary, iso),
            "{iso} must not lead the DUP week (primary was {primary})"
        );
        assert!(
            movement_pattern(&primary).is_some() && e1rm_reliable_name(&primary),
            "the DUP primary must be a pattern-classified compound, got {primary}"
        );
    }
}

// ── loads round DOWN onto the display-unit plate grid ──

#[test]
fn round_load_grid_floors_on_the_display_unit_grid() {
    // kg users: 2.5 kg grid, rounded DOWN (34 → 32.5, never up to 35).
    assert_eq!(round_load_grid(34.0, false), 32.5);
    assert_eq!(round_load_grid(85.0, false), 85.0);
    // lb users ≥ 50 lb: 5 lb grid; below 50 lb: 2.5 lb grid; always DOWN.
    let on_grid = |kg: f64, step: f64| {
        let lb = kg * LB_PER_KG;
        ((lb / step).round() * step - lb).abs() < 1e-4
    };
    assert!(on_grid(round_load_grid(85.0, true), 5.0)); // 187.4 lb → 185 lb
    assert!(
        round_load_grid(85.0, true) <= 85.0 + 1e-9,
        "rounding down never exceeds the prescribed load"
    );
    assert!(on_grid(round_load_grid(20.0, true), 2.5)); // 44.1 lb → 42.5 lb
}

#[test]
fn lb_user_gets_a_loadable_prescription() {
    let app = Engine;
    let mut model = Model::default();
    let profile = Profile {
        weight_lb: true,
        ..strength_profile()
    };
    app.update(Event::SetProfile(profile), &mut model)
        .expect_only_render();
    app.update(bench(100.0, 5, (MON - 1) * DAY_SEC, 1), &mut model)
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
    let load_kg = app
        .view(&model)
        .week_plan
        .into_iter()
        .filter(|s| s.title == "Heavy day")
        .flat_map(|s| s.items.into_iter())
        .find(|i| i.exercise.to_lowercase().contains("bench") && i.load_kg.is_some())
        .and_then(|i| i.load_kg)
        .expect("an anchored heavy bench");
    let lb = load_kg * LB_PER_KG;
    // The working load lands on the US 5 lb plate grid (the wire stays kg), so a
    // lb user never sees a fractional-lb load no plate combination can make.
    assert!(
        ((lb / 5.0).round() * 5.0 - lb).abs() < 1e-4,
        "load {lb} lb must sit on the 5 lb grid"
    );
    // Rounding DOWN never exceeds the prescribed 85% of the logged e1RM anchor
    // (a 100 kg × 5 set estimates ≈ 116.7 kg; the plan anchors off that).
    let anchor = (crate::strength::e1rm_epley(100.0, 5) * 10.0).round() / 10.0;
    assert!(
        load_kg <= anchor * 0.85 + 1e-9,
        "load {load_kg} kg must not overshoot 85% of the e1RM anchor {anchor}"
    );
}

// ── the power day surfaces its velocity intent ──

#[test]
fn power_day_prescriptions_carry_the_velocity_intent() {
    let profile = Profile {
        lift_goal: LiftGoal::Power,
        progression_cadence: ProgressionCadence::WeekToWeek, // → Intermediate → DUP
        ..strength_profile()
    };
    let anchors = Anchors {
        lift_e1rm: vec![("Bench".to_string(), 100.0)],
        ..Anchors::default()
    };
    let prog = synthesize(&profile, &anchors, 0).expect("a strength profile plans");
    let day_lift = |want: SessionType| {
        prog.mesocycle
            .sessions
            .iter()
            .find(|s| s.session_type == want)
            .and_then(|s| {
                s.prescriptions.iter().find_map(|rx| match &rx.value {
                    Prescription::Lift(l) if l.exercise == "Bench" => Some(l.clone()),
                    _ => None,
                })
            })
    };
    let power = day_lift(SessionType::Lift(LiftSessionType::DynamicEffort))
        .expect("a power day with the anchored bench");
    // AUTOREG-VL-001 power VL-termination threshold = 10%.
    assert_eq!(
        power.velocity_loss_pct,
        Some(10),
        "a power day must surface the velocity-loss intent, not read as an easy 50% day"
    );
    let heavy = day_lift(SessionType::Lift(LiftSessionType::MaxEffort))
        .expect("a heavy day with the anchored bench");
    assert_eq!(
        heavy.velocity_loss_pct, None,
        "a heavy day is not a velocity day"
    );
}

// ── calisthenics days are titled honestly ──

#[test]
fn calisthenics_days_are_titled_honestly() {
    let prog = synthesize(&calisthenics_profile(), &Anchors::default(), 0)
        .expect("a calisthenics profile plans");
    let lift = prog
        .mesocycle
        .sessions
        .iter()
        .find(|s| matches!(s.session_type, SessionType::Lift(_)))
        .expect("a calisthenics training day");
    assert_eq!(
        lift.session_type,
        SessionType::Lift(LiftSessionType::Calisthenics)
    );
    // Honest, user-facing title — not a borrowed barbell "Heavy day".
    assert_eq!(session_title(lift.session_type), "Calisthenics");
    // The shell's discipline parsing still keys on the "Lift" prefix.
    assert!(lift.session_type.wire_name().starts_with("Lift"));
}

#[test]
fn no_plan_means_no_next_session() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.next_session.is_none(), "no plan → no next session");
    assert!(vm.week_plan.is_empty());
    assert!(vm.program.is_none());
}

#[test]
fn plan_views_are_memoized_across_unrelated_events() {
    // The week strip, the next session and the program summary are a pure
    // function of the history, profile, readiness inputs, date and block
    // position. An event touching none of them (a protein calculation) must not
    // re-synthesize the week — that is eight `plan::synthesize` passes, each with
    // its own anchor derivation over the whole history.
    let (app, mut model) = planned_strength_model(true);
    for (ex, kg) in [("Back Squat", 100.0), ("Bench press", 60.0)] {
        app.update(log_set(ex, kg, 5, 8.0, MON * DAY_SEC), &mut model)
            .expect_only_render();
    }
    let first = app.view(&model);
    let before = PLAN_SYNTH_BUILDS.with(|c| c.get());
    app.update(
        Event::ComputeProtein {
            bodyweight_kg: 80.0,
            masters: false,
            deficit: false,
        },
        &mut model,
    )
    .expect_only_render();
    let second = app.view(&model);
    let after = PLAN_SYNTH_BUILDS.with(|c| c.get());
    assert_eq!(
        after - before,
        0,
        "an unrelated event must not re-synthesize the week"
    );
    assert_eq!(
        first.week_plan, second.week_plan,
        "the week strip is stable"
    );
    assert_eq!(
        first.next_session, second.next_session,
        "the next session is stable"
    );
    assert_eq!(first.program, second.program, "the summary is stable");

    // Logging a set DOES change the history the plan reads, so the snapshots are
    // rebuilt rather than served stale.
    app.update(
        log_set("Back Squat", 110.0, 5, 8.0, MON * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    let _ = app.view(&model);
    assert!(
        PLAN_SYNTH_BUILDS.with(|c| c.get()) > after,
        "new history must re-synthesize the week"
    );
}

#[test]
fn regenerating_a_plan_preserves_the_original_anchor() {
    // The shell auto-fires GeneratePlan on launch; re-firing with a LATER day
    // must NOT re-date the plan (else the week strip sits at "week 1" forever
    // and logged done/missed days flip back to "planned"). Only ClearPlan then
    // GeneratePlan makes a new anchor.
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
    // A later launch re-fires with a different "today".
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON + 14,
        },
        &mut model,
    )
    .expect_only_render();
    assert_eq!(
        model.plan_request.as_ref().map(|p| p.start_epoch_day),
        Some(MON),
        "re-firing GeneratePlan must keep the ORIGINAL anchor",
    );
    // ClearPlan then GeneratePlan DOES re-anchor (a deliberate fresh plan).
    app.update(Event::ClearPlan, &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON + 14,
        },
        &mut model,
    )
    .expect_only_render();
    assert_eq!(
        model.plan_request.as_ref().map(|p| p.start_epoch_day),
        Some(MON + 14),
        "after ClearPlan, GeneratePlan takes the new day",
    );
}

#[test]
fn plan_leads_with_an_anchored_prescription() {
    let (app, model) = planned_strength_model(true);
    let vm = app.view(&model);

    let ns = vm.next_session.expect("a plan yields a next session");
    assert_eq!(ns.status, SessionStatus::Next);
    assert_eq!(vm.week_plan.len(), 7, "a full week is dated");
    assert!(vm.program.is_some(), "the program summary is present");

    // The logged Back Squat is prescribed by %1RM → a concrete load.
    let sq = ns
        .items
        .iter()
        .find(|i| i.exercise.eq_ignore_ascii_case("Back Squat"))
        .expect("the user's own lift leads the session");
    let load = sq.load_kg.expect("an anchored lift shows a kg load");
    assert!(load > 0.0, "load must be positive: {load}");
    assert!(
        !sq.anchored_on.is_empty(),
        "the honesty line names the e1RM"
    );
    assert!(
        sq.evidence.grade.is_some(),
        "HARD RULE 2: evidence travels with it"
    );

    // The headline leads with the prescription.
    assert_eq!(vm.today_headline.kind, HeadlineKind::Prescription);
    assert!(vm.today_headline.summary.is_empty());
}

#[test]
fn plan_falls_back_to_rir_without_an_anchor() {
    let (app, model) = planned_strength_model(false);
    let vm = app.view(&model);
    let ns = vm.next_session.expect("plan without history still plans");
    assert!(!ns.items.is_empty());
    for it in &ns.items {
        assert!(
            it.load_kg.is_none(),
            "no e1RM anchor → no invented load (HARD RULE 1): {}",
            it.summary
        );
        assert!(
            it.intensity_label.starts_with("RIR"),
            "unanchored lifts are RIR-prescribed: {}",
            it.intensity_label
        );
    }
}

#[test]
fn a_gate_profile_shows_no_plan() {
    let app = Engine;
    let mut model = Model::default();
    let mut p = strength_profile();
    p.health.reds_signal = true; // medical deferral (HARD RULE 3)
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(
        vm.next_session.is_none(),
        "a gated profile never gets a plan"
    );
    assert!(vm.train_blocked, "the deferral holds training");
}

// ── The program week cycles 1..=weeks_total as the block repeats ──
#[test]
fn program_week_cycles_with_the_periodized_block() {
    let (app, mut model) = planned_strength_model(true);
    let prog0 = app.view(&model).program.expect("program present");
    assert_eq!(prog0.week, 1, "in-block week 1 at the start");
    assert_eq!(prog0.phase, MesoPhase::Build, "week 1 is a build week");
    let weeks_total = prog0.weeks_total as i64;
    assert!(weeks_total >= 2, "a cycle is load weeks + a recovery week");

    // The last week of the cycle is the recovery (Deload) week, reached once
    // every earlier week was completed (the plan advances by completed work).
    complete_plan_weeks(&app, &mut model, MON, weeks_total - 1);
    app.update(
        Event::SetToday {
            epoch_day: MON + 7 * (weeks_total - 1),
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let last = app.view(&model).program.expect("program present");
    assert_eq!(last.week, weeks_total as u8, "the last cycle position");
    assert_eq!(
        last.phase,
        MesoPhase::Deload,
        "the last week of the cycle is a recovery week"
    );

    // One full cycle later the block repeats at week 1 (Build).
    complete_plan_weeks(&app, &mut model, MON + 7 * (weeks_total - 1), 1);
    app.update(
        Event::SetToday {
            epoch_day: MON + 7 * weeks_total,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let wrapped = app.view(&model).program.expect("program present");
    assert_eq!(wrapped.week, 1, "the week cycles back to 1");
    assert_eq!(wrapped.phase, MesoPhase::Build, "and back to a build week");
}

#[test]
fn prescription_views_carry_numeric_prefill_targets() {
    // The wire carries a numeric RPE target for prefilling a log entry: an
    // RIR-prescribed lift maps its RIR to an RPE (RIR 3 → RPE 7.0), and a
    // %e1RM-anchored lift carries the effort its percentage implies on the
    // Epley model (strength-005 inverse, 1–4 RIR, never 0) — derived
    // from the prescription, not invented.
    let (app, model) = planned_strength_model(true);
    let ns = app
        .view(&model)
        .next_session
        .expect("a plan yields a next session");
    let anchored = ns
        .items
        .iter()
        .find(|i| i.exercise.eq_ignore_ascii_case("Back Squat"))
        .expect("the anchored %e1RM lift");
    assert!(
        anchored.intensity_label.contains("e1RM"),
        "the anchored lift is %e1RM-prescribed: {}",
        anchored.intensity_label
    );
    let PlanIntensity::PercentE1rm { pct, .. } = &anchored.intensity else {
        panic!("structured %e1RM target: {:?}", anchored.intensity)
    };
    let (pct, reps) = (*pct as f64, anchored.reps_low as u32);
    assert_eq!(
        anchored.target_rpe,
        Some(crate::strength::rir_to_rpe(
            crate::strength::effort_target_rir(pct, reps) as f64
        )),
        "a %e1RM lift carries the effort its percentage implies"
    );
    assert!(
        anchored.target_rpe.is_some_and(|r| r < 10.0),
        "a percentage set is never targeted at failure"
    );
    let rir = ns
        .items
        .iter()
        .find(|i| i.intensity_label == "RIR 3")
        .expect("an unanchored RIR 3 accessory");
    assert_eq!(
        rir.target_rpe,
        Some(crate::strength::rir_to_rpe(3.0)),
        "RIR 3 maps 1:1 to RPE 7.0"
    );
    assert_eq!(
        rir.target_rpe,
        Some(7.0),
        "the mapped value is exactly RPE 7.0"
    );
}

#[test]
fn a_sub_1_25kg_anchor_never_prescribes_at_0kg() {
    // A %1RM load that rounds below the 2.5 kg grid (80% of a 1 kg
    // anchor = 0.8 kg → round_2_5 = 0) must never render "@ 0 kg"; the item
    // drops the load and falls back to an RIR (effort) prescription.
    let pres = Prescription::Lift(crate::schema::LiftPrescription {
        exercise: "Back Squat".into(),
        sets: 3,
        reps: 5,
        intensity: LiftIntensity::PercentOneRm(80.0),
        rest_sec: 180,
        velocity_loss_pct: None,
        reps_max: None,
        load_kg: None,
    });
    let rx = graded(pres, claims::STR_INTENT_001);
    let tiny = crate::plan::Anchors {
        lift_e1rm: vec![("Back Squat".into(), 1.0)],
        ..Default::default()
    };
    let it = flatten_prescription(&rx, &tiny, None, false);
    assert!(
        !it.summary.contains("@ 0"),
        "no '@ 0 kg' on a collapsed anchor: {}",
        it.summary
    );
    assert_eq!(it.load_kg, None, "the collapsed load is dropped, not 0 kg");
    assert!(
        it.intensity_label.starts_with("RIR "),
        "the fallback is an RIR (effort) prescription: {}",
        it.intensity_label
    );
    assert!(
        matches!(it.intensity, PlanIntensity::Rir { .. }),
        "the structured intensity is RIR: {:?}",
        it.intensity
    );
}

#[test]
fn sets_logged_under_a_retired_catalog_name_show_the_current_name() {
    // History logged under the old catalog labels ("Barbell back squat",
    // "Plank (front)") reads under the current names everywhere the view
    // names a lift (history rows, which also feed the editor's quick picks,
    // and the plan), while the old- and new-name sets stay one lift.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    for ev in [
        log_set("Barbell back squat", 120.0, 3, 8.0, (MON - 8) * DAY_SEC),
        log_set("Back squat", 110.0, 3, 8.0, (MON - 1) * DAY_SEC),
        cal_hold("Plank (front)", 30, 7.0, (MON - 1) * DAY_SEC + 600),
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        Event::SetToday {
            epoch_day: MON,
            utc_offset_sec: 0,
        },
    ] {
        app.update(ev, &mut model).expect_only_render();
    }
    let vm = app.view(&model);
    let names: Vec<&str> = vm.lifts.iter().map(|l| l.exercise.as_str()).collect();
    assert!(
        !names.contains(&"Barbell back squat") && !names.contains(&"Plank (front)"),
        "{names:?}"
    );
    assert!(names.contains(&"Front plank"), "{names:?}");
    let squats: Vec<_> = vm
        .lifts
        .iter()
        .filter(|l| l.exercise == "Back squat")
        .collect();
    assert_eq!(squats.len(), 2, "{names:?}");
    let newer = squats
        .iter()
        .max_by_key(|l| l.observed_at)
        .expect("the newer squat");
    assert!(
        newer.e1rm_delta_kg.is_some_and(|d| d < 0.0) && !newer.e1rm_pr,
        "the new-name set compares against the old-name one: {newer:?}"
    );
    let planned: Vec<&str> = vm
        .week_plan
        .iter()
        .flat_map(|s| s.items.iter())
        .map(|i| i.exercise.as_str())
        .filter(|n| !n.is_empty())
        .collect();
    assert!(planned.contains(&"Back squat"), "{planned:?}");
    assert!(
        !planned
            .iter()
            .any(|n| n.eq_ignore_ascii_case("Barbell back squat")),
        "{planned:?}"
    );
}
