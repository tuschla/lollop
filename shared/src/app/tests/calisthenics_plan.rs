//! Calisthenics on the plan: rung advance and pain suppression, the skill
//! track and tendon gate, safety cues, timed holds, prefill targets, and how
//! readiness caps and pain clears act on a ladder.

use super::*;

/// Two distinct local days (offset 0), 3 clean top-of-band "Full push-up"
/// sets each → 2 consecutive qualifying sessions → `advance_ready` true with
/// no pain gate. HorizontalPush rung 4 is reps 5–8, so reps=8 is the band top
/// and rpe 8.0 (RIR 2) is clean.
fn two_qualifying_pushup_sessions() -> Vec<LoggedSet> {
    let mut sets = Vec::new();
    for (day, base_id) in [(100i64, 0u64), (101, 10)] {
        for i in 0..3u64 {
            sets.push(LoggedSet {
                hold_sec: None,
                exercise: "Full push-up".into(),
                weight_kg: 0.0,
                reps: 8,
                rpe: 8.0,
                observed_at: day * DAY_SEC + i as i64,
                entry_id: base_id + i,
            });
        }
    }
    sets
}

/// Calisthenics anchors for the two-day push-up fixture (days 100–101), as of
/// day 101 on a strength goal.
fn pushup_anchors(sets: &[LoggedSet], inputs: &[ReadinessInput]) -> Vec<crate::plan::CalisAnchor> {
    build_calisthenics_anchors(
        sets.iter(),
        &[],
        inputs,
        crate::strength::LiftGoal::MaxStrength,
        Some(101),
        0,
    )
}

#[test]
fn calisthenics_plan_advances_a_rung_then_pain_suppresses_it() {
    let (app, mut model) = planned_calisthenics();
    // Two distinct qualifying days at Full push-up (rung 4): 3×8 @ RPE 8 (RIR
    // 2). Both are PAST days so nothing is logged TODAY — today's tile keeps
    // showing the prescribed push rung (a logged-today rung drops off the tile).
    for day in [MON - 2, MON - 1] {
        for _ in 0..3 {
            app.update(
                log_set("Full push-up", 0.0, 8, 8.0, day * DAY_SEC),
                &mut model,
            )
            .expect_only_render();
        }
    }
    let vm = app.view(&model);
    let push = cal_item(&vm, Lad::HorizontalPush).expect("a horizontal-push item");
    assert_eq!(
        push.exercise, "Decline push-up",
        "top-of-band on 2 sessions advances the tier"
    );
    assert_eq!(
        push.reps_low, 5,
        "the advanced rung resets to the band floor"
    );

    // A tolerable (non-blocking) tendon-pain report DATED YESTERDAY (MON-1)
    // suppresses difficulty progression. It is in today's prescribing
    // history (cutoff MON-1), so it governs today's rung. (A report dated TODAY
    // would NOT rewrite today's already-shown rung — the day-stability rule, covered by
    // the completion suite.)
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::Pain,
            value: 1.0,
            observed_at: (MON - 1) * DAY_SEC,
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
    let vm = app.view(&model);
    assert!(!vm.train_blocked, "tolerable pain must not block training");
    let push = cal_item(&vm, Lad::HorizontalPush).expect("a horizontal-push item");
    assert_eq!(
        push.exercise, "Full push-up",
        "joint pain suppresses the advance (rung stays 4)"
    );
}

#[test]
fn calisthenics_skill_track_schedules_first_and_the_tendon_gate_paces_it() {
    let (app, mut model) = planned_calisthenics();
    // Two clean 15 s tuck-front-lever holds on 2 days within the SAME week.
    // Dated BEFORE today: a day's prescription is synthesized from history
    // logged strictly before it, so today's own sets never move today's rungs.
    app.update(
        cal_hold("Tuck front lever", 15, 7.0, (MON - 3) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        cal_hold("Tuck front lever", 15, 7.0, (MON - 1) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    // The skill item is scheduled FIRST (fresh) on its lift day.
    let day = vm
        .week_plan
        .iter()
        .find(|s| {
            s.items.iter().any(|it| {
                crate::calisthenics::tier_by_name(&it.exercise).map(|(l, _)| l)
                    == Some(Lad::FrontLever)
            })
        })
        .expect("a lift day carrying the skill item");
    assert_eq!(
        crate::calisthenics::tier_by_name(&day.items[0].exercise).map(|(l, _)| l),
        Some(Lad::FrontLever),
        "skill work is scheduled first"
    );
    let fl = cal_item(&vm, Lad::FrontLever).expect("a front-lever item");
    assert!(fl.timed_hold, "a lever rung is a timed hold");
    assert_eq!(
        fl.exercise, "Tuck front lever",
        "the tendon rate-limit blocks the advance (< 28 days at tier)"
    );

    // Re-log weekly sessions spanning >= 28 days at the tier. (Two sessions 29
    // days apart would be a break: CAL-REENTRY-001 restarts a straight-arm
    // rung's acclimation window after 2+ weeks off.) The fixture `MON` is epoch
    // day 4, so a month back would be undated (<= 0) rows that never start the
    // tendon clock: move "today" ten weeks on so every session is dated.
    let later = MON + 70;
    app.update(Event::ClearSets, &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: later,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: later,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    for back in [30, 23, 16, 9, 1] {
        app.update(
            cal_hold("Tuck front lever", 15, 7.0, (later - back) * DAY_SEC),
            &mut model,
        )
        .expect_only_render();
    }
    let vm = app.view(&model);
    let fl = cal_item(&vm, Lad::FrontLever).expect("a front-lever item");
    assert_eq!(
        fl.exercise, "Advanced tuck front lever",
        "once acclimated (>= 28 days) the next lever tier is prescribed"
    );
}

#[test]
fn calisthenics_safety_cues_surface_as_graded_rows() {
    // The wrist / tendon / Valsalva cues are wired to their
    // safety-critical claims and surface as evidence-cited guidance rows,
    // not as un-cited card basis text. Log a wrist-gated (handstand) and a
    // straight-arm (front-lever) rung so both gates arm.
    let (app, mut model) = planned_calisthenics();
    app.update(cal_hold("Wall walk", 10, 7.0, MON * DAY_SEC), &mut model)
        .expect_only_render();
    app.update(
        cal_hold("Tuck front lever", 15, 7.0, MON * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let row = |needle: &str| {
        vm.lift_audit
            .iter()
            .find(|g| g.summary.contains(needle))
            .unwrap_or_else(|| panic!("missing cue row: {needle}"))
            .clone()
    };
    // Wrist gate → CAL-INJURY-WRIST-001 (Moderate, safety-critical).
    let wrist = row("bending the wrist back under load is pain-free");
    assert_eq!(wrist.evidence.grade, Some(EvidenceGrade::Moderate));
    assert!(
        wrist.evidence.safety_critical,
        "wrist gate is a safety-critical cue"
    );
    assert!(
        wrist.summary.contains("wrist circles"),
        "actionable prep: {}",
        wrist.summary
    );
    // Tendon rate-limit → CAL-TENDON-001 (Strong, safety-critical).
    let tendon = row("elbow tendons, which adapt slowly");
    assert_eq!(tendon.evidence.grade, Some(EvidenceGrade::Strong));
    assert!(tendon.evidence.safety_critical);
    // Valsalva breathing → CAL-BP-VALSALVA-001 (Moderate, safety-critical).
    let breath = row("Keep breathing through every hold");
    assert_eq!(breath.evidence.grade, Some(EvidenceGrade::Moderate));
    assert!(breath.evidence.safety_critical);
    // Plain language — no internal gate codes or engine jargon.
    for g in &vm.lift_audit {
        let s = &g.summary;
        assert!(
            !(1..=8).any(|n| s.contains(&format!("G{n}:"))) && !s.contains("engine default"),
            "internal jargon in user copy: {s}"
        );
    }
}

#[test]
fn calisthenics_muscle_up_gate_blocks_then_opens() {
    let (app, mut model) = planned_calisthenics();
    app.update(
        log_set("Negative muscle-up", 0.0, 3, 8.0, (MON - 1) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set("Full pull-up", 0.0, 8, 8.0, (MON - 1) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set("Full parallel-bar dip", 0.0, 8, 8.0, (MON - 1) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(
        cal_item(&vm, Lad::MuscleUp).is_none(),
        "no muscle-up track while the pull-up/dip count gate is unmet"
    );
    assert!(
        vm.lift_audit.iter().any(|g| g
            .summary
            .contains("Muscle-up work starts at 12 strict pull-ups")),
        "the muscle-up count gate guidance row is surfaced"
    );

    // Raise both bests to 12 → the gate opens and the track schedules.
    app.update(
        log_set("Full pull-up", 0.0, 12, 8.0, (MON - 1) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set("Full parallel-bar dip", 0.0, 12, 8.0, (MON - 1) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(
        cal_item(&vm, Lad::MuscleUp).is_some(),
        "the muscle-up track schedules once 12 pull-ups + 12 dips are logged"
    );
}

#[test]
fn calisthenics_hold_set_round_trips_renders_and_yields_no_e1rm() {
    let app = Engine;
    let mut model = Model::default();
    // Wire JSON WITH the hold_sec field.
    let with_hold: Event = serde_json::from_str(
            r#"{"LogSet":{"exercise":"Front plank","weight_kg":0.0,"reps":1,"rpe":7.0,"observed_at":100,"entry_id":1,"hold_sec":30}}"#,
        )
        .expect("LogSet with hold_sec decodes");
    app.update(with_hold, &mut model).expect_only_render();
    // Wire JSON WITHOUT the field (back-compat: decodes as None).
    let no_hold: Event = serde_json::from_str(
            r#"{"LogSet":{"exercise":"Bench","weight_kg":100.0,"reps":5,"rpe":8.0,"observed_at":200,"entry_id":2}}"#,
        )
        .expect("LogSet without hold_sec still decodes");
    app.update(no_hold, &mut model).expect_only_render();

    let vm = app.view(&model);
    let plank = vm
        .lifts
        .iter()
        .find(|l| l.exercise == "Front plank")
        .expect("the plank row renders");
    assert_eq!(plank.hold_sec, Some(30));
    assert!(
        plank.summary.contains("30 s hold"),
        "history row: {}",
        plank.summary
    );
    // Tonnage is load × reps of a reps set; a hold lifts none.
    assert_eq!(plank.tonnage_kg, 0.0);
    let bench = vm
        .lifts
        .iter()
        .find(|l| l.exercise == "Bench")
        .expect("the bench row renders");
    assert_eq!(bench.tonnage_kg, 500.0);

    // A ladder-named hold set never anchors an e1RM; a barbell set still does.
    let anchors = build_plan_anchors(
        model.sets.iter(),
        AnchorCtx {
            calisthenics: true,
            ..Default::default()
        },
        0,
        0,
    );
    assert!(
        anchors.e1rm_for("Front plank").is_none(),
        "no e1RM for a hold set"
    );
    assert!(
        anchors.e1rm_for("Bench").is_some(),
        "a barbell set still anchors an e1RM"
    );
}

#[test]
fn amending_only_hold_sec_refreshes_the_lift_summary() {
    // hold_sec is part of the lift-view fingerprint, so an AmendSet
    // that changes ONLY the hold seconds invalidates the memo and the
    // refreshed history summary shows the new hold, not the cached old one.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        with_entry_id(cal_hold("Front plank", 30, 7.0, 100), 1),
        &mut model,
    )
    .expect_only_render();
    // Render lift views once (populates the fingerprint-keyed memo).
    let before = app
        .view(&model)
        .lifts
        .iter()
        .find(|l| l.exercise == "Front plank")
        .expect("plank row")
        .summary
        .clone();
    assert!(
        before.contains("30 s hold"),
        "initial hold renders: {before}"
    );
    // AmendSet changing ONLY hold_sec (same id/exercise/weight/reps/rpe/date).
    app.update(
        Event::AmendSet {
            hold_sec: Some(45),
            entry_id: 1,
            exercise: "Front plank".into(),
            weight_kg: 0.0,
            reps: 1,
            rpe: 7.0,
            observed_at: 100,
            observed_at_fallback: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let after = app
        .view(&model)
        .lifts
        .iter()
        .find(|l| l.exercise == "Front plank")
        .expect("plank row")
        .summary
        .clone();
    assert!(
        after.contains("45 s hold"),
        "the amended hold must refresh: {after}"
    );
    assert!(
        !after.contains("30 s hold"),
        "the stale hold must be gone: {after}"
    );
}

#[test]
fn a_downgrade_on_a_calisthenics_hold_shows_no_zero_reps_or_rir_advice() {
    // Folding a DowngradeSession (e.g. an HRV downgrade) onto a
    // timed-hold calisthenics item renders hold-appropriate copy — never
    // "×0" (reps_low is 0 for a hold) and never RIR / "reps in reserve"
    // advice; a dynamic item still shows its rep RANGE.
    let hold = PrescriptionView {
        exercise: "Tuck front lever".into(),
        sets: 4,
        reps_low: 0,
        reps_high: 0,
        load_kg: None,
        intensity_label: "clean hold, stop before your form breaks".into(),
        rep_volume: "5–15 s hold".into(),
        timed_hold: true,
        ..Default::default()
    };
    let dynamic = PrescriptionView {
        exercise: "Full push-up".into(),
        sets: 3,
        reps_low: 5,
        reps_high: 8,
        load_kg: None,
        intensity_label: "RIR 1–2".into(),
        timed_hold: false,
        ..Default::default()
    };
    let mut ns = SessionPlanView {
        items: vec![hold, dynamic],
        ..Default::default()
    };
    let recs = vec![graded(Adjustment::DowngradeSession, claims::HRV_001)];
    apply_adjustments_to_session(&mut ns, &recs, false);
    let h = &ns.items[0];
    assert!(!h.summary.contains("×0"), "no ×0 on a hold: {}", h.summary);
    assert!(
        h.summary.contains("5–15 s hold"),
        "the hold volume renders: {}",
        h.summary
    );
    assert!(
        !h.intensity_label.to_lowercase().contains("rir")
            && !h.intensity_label.contains("reps in reserve"),
        "hold copy carries no RIR advice: {}",
        h.intensity_label
    );
    // The dynamic item still renders its rep range (never ×0).
    assert!(
        ns.items[1].summary.contains("3×5–8"),
        "dynamic rep range renders: {}",
        ns.items[1].summary
    );
}

#[test]
fn calisthenics_ladder_work_is_absent_from_the_prilepin_audit() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(calisthenics_profile()), &mut model)
        .expect_only_render();
    // Weighted pull-up work (ladder-named) yields no e1RM/Prilepin audit row.
    for _ in 0..5 {
        app.update(log_set("Full pull-up", 10.0, 5, 8.0, DAY_SEC), &mut model)
            .expect_only_render();
    }
    // A barbell lift logged the same day IS audited exactly as today.
    app.update(log_set("Bench press", 60.0, 5, 8.0, DAY_SEC), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(
        !vm.lift_audit
            .iter()
            .any(|g| g.summary.contains("Full pull-up")),
        "ladder work carries no Prilepin/e1RM audit row"
    );
    assert!(
        vm.lift_audit
            .iter()
            .any(|g| g.summary.contains("Bench press")),
        "a barbell lift is still audited"
    );
}

#[test]
fn calisthenics_weighted_handoff_is_leave_rir_and_safety_critical() {
    let (app, mut model) = planned_calisthenics();
    // A weighted Full pull-up (rung 4, a weighted rung; actual added load):
    // reclassified to weighted work — leave RIR, safety_critical, "+ N kg"
    // in the summary (CAL-EFAIL-SAFE-001).
    app.update(
        log_set("Full pull-up", 10.0, 5, 8.0, (MON - 1) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let pull = cal_item(&vm, Lad::VerticalPull).expect("a vertical-pull item");
    assert_eq!(pull.exercise, "Full pull-up");
    assert!(
        pull.evidence.safety_critical,
        "loaded work carries the safety chip"
    );
    assert_eq!(
        pull.intensity_label, "RIR 1–2",
        "weighted work leaves reps in reserve"
    );
    assert!(
        pull.summary.contains("+ 10 kg"),
        "summary shows the added load: {}",
        pull.summary
    );
}

#[test]
fn calisthenics_prescription_views_carry_hold_and_added_prefill() {
    // A timed rung carries its hold band in seconds and a Some(0.0) added
    // load (bodyweight); a dynamic RIR 1–2 rung maps its conservative (higher
    // RIR) bound to RPE 8.0.
    let (app, model) = planned_calisthenics();
    let items: Vec<_> = app
        .view(&model)
        .week_plan
        .iter()
        .flat_map(|s| s.items.clone())
        .collect();
    let hold = items
        .iter()
        .find(|it| it.timed_hold)
        .expect("a timed-hold rung in the plan");
    assert!(hold.hold_low_sec > 0, "a timed rung carries its hold floor");
    assert!(
        hold.hold_high_sec >= hold.hold_low_sec,
        "the hold band is ordered: {}..{}",
        hold.hold_low_sec,
        hold.hold_high_sec
    );
    assert_eq!(
        hold.added_kg,
        Some(0.0),
        "a bodyweight rung's added load is Some(0.0), never None"
    );
    let dynamic = items
        .iter()
        .find(|it| !it.timed_hold && it.intensity_label == "RIR 1–2")
        .expect("a dynamic RIR 1–2 rung in the plan");
    assert_eq!(
        dynamic.target_rpe,
        Some(8.0),
        "RIR 1–2 maps its higher (conservative) bound to RPE 8.0"
    );
    assert_eq!(
        dynamic.added_kg,
        Some(0.0),
        "a bodyweight rung's added load is Some(0.0)"
    );
    assert!(dynamic.reps_low > 0, "a dynamic rung carries a rep floor");
}

#[test]
fn cap_rpe_caps_a_calisthenics_rir_band() {
    // CapRpe must cap a "RIR lo–hi" dynamic rung off the STRUCTURED field
    // (not by parsing the label), raising both ends and resyncing the prefill
    // RPE to the new conservative (high) bound.
    let dynamic = PrescriptionView {
        exercise: "Full push-up".into(),
        sets: 3,
        reps_low: 5,
        reps_high: 8,
        intensity_label: "RIR 1–2".into(),
        intensity: PlanIntensity::RirRange { lo: 1, hi: 2 },
        target_rpe: Some(crate::strength::rir_to_rpe(2.0)),
        ..Default::default()
    };
    let mut ns = SessionPlanView {
        items: vec![dynamic],
        ..Default::default()
    };
    apply_adjustments_to_session(
        &mut ns,
        &[graded(Adjustment::CapRpe(1), claims::AUTOREG_E1RM_GATE_001)],
        false,
    );
    assert_eq!(
        ns.items[0].intensity_label, "RIR 2–3",
        "both band ends shift up by the cap"
    );
    assert_eq!(
        ns.items[0].intensity,
        PlanIntensity::RirRange { lo: 2, hi: 3 },
        "the structured band is capped, not just the label"
    );
    assert_eq!(
        ns.items[0].target_rpe,
        Some(crate::strength::rir_to_rpe(3.0)),
        "the prefill RPE resyncs to the new conservative (high) bound"
    );
    assert_eq!(
        ns.status,
        SessionStatus::Adjusted,
        "a real rung change marks the session adjusted"
    );
}

#[test]
fn cap_rpe_over_a_holds_only_session_does_not_mark_adjusted() {
    // CapRpe skips prose/hold items; a holds-only session is a genuine
    // no-op and must NOT show an "adjusted" chip over an unchanged top-end.
    let hold = PrescriptionView {
        exercise: "Tuck front lever".into(),
        sets: 4,
        timed_hold: true,
        rep_volume: "5–15 s hold".into(),
        intensity_label: "clean hold, stop before your form breaks".into(),
        intensity: PlanIntensity::Prose,
        ..Default::default()
    };
    let mut ns = SessionPlanView {
        items: vec![hold],
        ..Default::default()
    };
    apply_adjustments_to_session(
        &mut ns,
        &[graded(Adjustment::CapRpe(1), claims::AUTOREG_E1RM_GATE_001)],
        false,
    );
    assert_ne!(
        ns.status,
        SessionStatus::Adjusted,
        "a holds-only CapRpe is a no-op, not 'adjusted'"
    );
    assert!(
        ns.adjustment.is_none(),
        "no adjustment chip when nothing changed"
    );
    assert_eq!(
        ns.items[0].intensity_label, "clean hold, stop before your form breaks",
        "the hold copy is untouched"
    );
    assert!(
        ns.items[0].adjusted_note.is_empty(),
        "no cap note on an unchanged hold"
    );
}

#[test]
fn doms_only_inputs_leave_calisthenics_advance_ready() {
    // A Doms report is normal training discomfort (File 08 Table 4.1
    // "continue") and must NOT freeze ladder progression.
    let sets = two_qualifying_pushup_sessions();
    let base = pushup_anchors(&sets, &[]);
    let ready = |anchors: &[crate::plan::CalisAnchor]| {
        anchors
            .iter()
            .find(|a| a.ladder == crate::schema::LadderId::HorizontalPush)
            .expect("push-up anchor")
            .advance_ready
    };
    assert!(ready(&base), "baseline (no pain) is ready to advance");

    let doms = vec![pain_row(
        1.0,
        Some(crate::schema::PainKind::Doms),
        200 * DAY_SEC,
    )];
    let with_doms = pushup_anchors(&sets, &doms);
    assert!(
        ready(&with_doms),
        "a DOMS report must not freeze advance_ready"
    );
}

#[test]
fn a_pain_clear_after_pain_re_enables_calisthenics_advance() {
    // An active pain report freezes advance; a later pain-CLEAR row
    // (value 0, the LATEST reading) re-opens it; an "any Pain signal ever"
    // gate would never re-enable.
    let sets = two_qualifying_pushup_sessions();
    let ready = |anchors: &[crate::plan::CalisAnchor]| {
        anchors
            .iter()
            .find(|a| a.ladder == crate::schema::LadderId::HorizontalPush)
            .expect("push-up anchor")
            .advance_ready
    };

    let pain = vec![pain_row(1.0, None, 200 * DAY_SEC)];
    assert!(
        !ready(&pushup_anchors(&sets, &pain)),
        "active pain freezes advance"
    );

    // A later value-0 clear is the latest Pain reading → the gate re-opens.
    let cleared = vec![
        pain_row(1.0, None, 200 * DAY_SEC),
        pain_row(0.0, None, 201 * DAY_SEC),
    ];
    assert!(
        ready(&pushup_anchors(&sets, &cleared)),
        "a pain-clear re-enables advance"
    );
}
