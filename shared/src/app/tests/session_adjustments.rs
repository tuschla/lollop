//! Readiness and review adjustments on the rendered session: caps, load cuts,
//! holds, and the prefill targets that follow them.

use super::*;

#[test]
fn readiness_downgrade_caps_the_shown_session() {
    let (app, mut model) = planned_strength_model(true);
    // Baseline load before any readiness input.
    let base = app
        .view(&model)
        .next_session
        .unwrap()
        .items
        .iter()
        .find(|i| i.exercise.eq_ignore_ascii_case("Back Squat"))
        .unwrap()
        .load_kg
        .unwrap();

    // RPE 2 over target → autoreg ReduceLoadPct(10) (AUTOREG-RIR-001).
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Rpe, 2.0)),
        &mut model,
    )
    .expect_only_render();

    let ns = app.view(&model).next_session.unwrap();
    assert_eq!(
        ns.status,
        SessionStatus::Adjusted,
        "the session is marked adjusted"
    );
    assert!(
        ns.adjustment.is_some(),
        "the adjustment's evidence rides along"
    );
    let sq = ns
        .items
        .iter()
        .find(|i| i.exercise.eq_ignore_ascii_case("Back Squat"))
        .unwrap();
    assert!(
        sq.load_kg.unwrap() < base,
        "readiness must cap the top-end: {} !< {}",
        sq.load_kg.unwrap(),
        base
    );
    assert!(
        !sq.adjusted_note.is_empty(),
        "the cap is explained on the item"
    );
}

#[test]
fn cap_rpe_notes_only_the_items_it_relabels() {
    // CapRpe relabels only RIR-prescribed lifts. A run (or a %1RM lift) it
    // never touches must NOT get a spurious "cap RPE" note: that note is the
    // item-level explanation of a change that, for those items, didn't happen.
    let lift = PrescriptionView {
        exercise: "Back Squat".into(),
        intensity_label: "RIR 3".into(),
        intensity: PlanIntensity::Rir { rir: 3 },
        ..Default::default()
    };
    let run = PrescriptionView {
        exercise: String::new(),
        intensity_label: "Easy pace".into(),
        ..Default::default()
    };
    let mut ns = SessionPlanView {
        items: vec![lift, run],
        ..Default::default()
    };
    let recs = vec![graded(Adjustment::CapRpe(1), claims::AUTOREG_E1RM_GATE_001)];
    apply_adjustments_to_session(&mut ns, &recs, false);

    assert_eq!(
        ns.items[0].intensity_label, "RIR 4",
        "the RIR lift is capped"
    );
    assert!(
        !ns.items[0].adjusted_note.is_empty(),
        "the relabeled lift carries the cap note"
    );
    assert!(
        ns.items[1].adjusted_note.is_empty(),
        "the untouched run must not get a false cap note"
    );
    assert_eq!(
        ns.items[1].intensity_label, "Easy pace",
        "the run is unchanged"
    );
}

#[test]
fn a_load_cut_rewrites_the_percent_e1rm_label() {
    // After rescaling load_kg, a "{n}% e1RM" label is rewritten to show the
    // cut, and the kg matches. RIR-style labels are left untouched.
    let lift = PrescriptionView {
        exercise: "Back Squat".into(),
        sets: 5,
        reps_low: 3,
        reps_high: 3,
        load_kg: Some(100.0),
        intensity_label: "80% e1RM".into(),
        intensity: PlanIntensity::PercentE1rm {
            pct: 80,
            reduced_from: None,
        },
        ..Default::default()
    };
    let rir_lift = PrescriptionView {
        exercise: "Chin-up".into(),
        sets: 3,
        reps_low: 8,
        reps_high: 8,
        load_kg: Some(20.0),
        intensity_label: "RIR 2".into(),
        intensity: PlanIntensity::Rir { rir: 2 },
        ..Default::default()
    };
    let mut ns = SessionPlanView {
        items: vec![lift, rir_lift],
        ..Default::default()
    };
    let recs = vec![graded(
        Adjustment::Deload {
            volume_reduction_pct: 0.0,
            load_reduction_pct: 10.0,
            weeks: 1,
        },
        claims::RUN_DELOAD_001,
    )];
    apply_adjustments_to_session(&mut ns, &recs, false);
    assert_eq!(ns.items[0].intensity_label, "72% e1RM (reduced from 80%)");
    assert_eq!(
        ns.items[0].load_kg,
        Some(round_load_grid(100.0 * 0.90, false))
    );
    let cut_kg = fmt_weight(round_load_grid(100.0 * 0.90, false), false);
    assert!(
        ns.items[0].summary.contains(&format!("@ {cut_kg}")),
        "the refreshed summary carries the cut load: {}",
        ns.items[0].summary
    );
    // A non-percent (RIR) label is untouched by the cut.
    assert_eq!(
        ns.items[1].intensity_label, "RIR 2",
        "RIR labels are left untouched"
    );
}

#[test]
fn a_do_not_train_hold_blocks_the_prescription() {
    let (app, mut model) = planned_strength_model(true);
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    assert!(vm.train_blocked, "a bare pain report holds training");
    let ns = vm
        .next_session
        .expect("the next session still renders, but blocked");
    assert_eq!(ns.status, SessionStatus::Blocked);
    assert!(
        ns.items.is_empty(),
        "no load numbers are shown through a hold"
    );
    // The safety hold, never the prescription, owns the headline.
    assert_eq!(vm.today_headline.kind, HeadlineKind::SafetyHold);
}

// ── A wrapped next session keeps a clean prescription ──────────────────────
#[test]
fn low_wrapped_next_session_is_not_folded_with_todays_adjustment() {
    // Strength week lifts on days 0/2/4 (Mon/Wed/Fri). With today at day 5
    // (Sat) every training day is behind us, so next_session WRAPS to next
    // Monday (+7). Today's transient readiness downgrade must NOT paint that
    // future-dated session: it would be a wrong-week deload.
    let (app, mut model) = planned_strength_model(true);
    app.update(
        Event::SetToday {
            epoch_day: MON + 5,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    // A single-day HRV suppression → non-blocking DowngradeSession for TODAY.
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::HrvLnRmssd,
            value: -2.0,
            observed_at: (MON + 5) * DAY_SEC,
            streak: 0,
            pain: None,
            effort_min: None,
        }),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    assert!(!vm.train_blocked, "a downgrade must not block training");
    let ns = vm.next_session.expect("a wrapped next session renders");
    assert!(
        ns.epoch_day > MON + 5,
        "the next session wrapped past today"
    );
    // Clean prescription: status stays "next" (never "adjusted"), no fold.
    assert_eq!(
        ns.status,
        SessionStatus::Next,
        "a future-dated session is not folded/adjusted"
    );
    assert!(
        ns.adjustment.is_none(),
        "no adjustment folded into a wrong-week session"
    );
    for it in ns.items.iter() {
        assert!(
            it.adjusted_note.is_empty(),
            "a future-dated item must stay un-adjusted, got note: {}",
            it.adjusted_note
        );
    }
}

#[test]
fn low_wrapped_next_session_still_blanks_under_a_safety_hold() {
    // The blocking/safety path is unchanged: a hold blanks the wrapped
    // session regardless of its date (HARD RULE 3).
    let (app, mut model) = planned_strength_model(true);
    app.update(
        Event::SetToday {
            epoch_day: MON + 5,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    // Generic pain report → hard Stop → train_blocked.
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::Pain,
            value: 1.0,
            observed_at: (MON + 5) * DAY_SEC,
            streak: 0,
            pain: None,
            effort_min: None,
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.train_blocked, "generic pain blocks training");
    let ns = vm.next_session.expect("a wrapped next session renders");
    assert!(
        ns.epoch_day > MON + 5,
        "the next session wrapped past today"
    );
    assert_eq!(
        ns.status,
        SessionStatus::Blocked,
        "a hold blanks even a future-dated session"
    );
    assert!(ns.items.is_empty(), "a blocked session shows no items");
}

#[test]
fn downgrade_caps_a_run_item_not_just_a_note() {
    // A running plan + an HRV downgrade must actually cap the run target.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(Profile {
            weekly_sets: 0,
            running_days_per_week: 4,
            running_km_per_week: 40.0,
            goal_distance: GoalDistance::TenK,
            ..strength_profile()
        }),
        &mut model,
    )
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
    // A multi-day HRV downtrend → DowngradeSession.
    for d in 0..3 {
        app.update(
            Event::SubmitReadiness(ReadinessInput {
                signal: ReadinessSignal::HrvLnRmssd,
                value: -2.0,
                observed_at: (MON - d) * DAY_SEC,
                streak: 3,
                pain: None,
                effort_min: None,
            }),
            &mut model,
        )
        .expect_only_render();
    }
    // The plan's anchored first day (today) holds the session, a run.
    let ns = app.view(&model).next_session.expect("today's session");
    assert_eq!(ns.status, SessionStatus::Adjusted, "the downgrade applies");
    let run = ns
        .items
        .iter()
        .find(|i| i.exercise.is_empty())
        .expect("a running plan's session carries a run item");
    assert!(
        run.intensity_label.to_lowercase().contains("easy"),
        "a downgraded run's intensity target must be capped to easy: {}",
        run.intensity_label
    );
    assert!(
        !run.adjusted_note.is_empty(),
        "the run carries an adjusted note"
    );
    // The chip must cite the DECISION (the HRV downgrade), not the hard-run
    // band. HRV multi-day downtrend routes through AUTOREG-HRV-001 → its
    // citation mentions HRV, and the why? basis reflects the downgrade, not a
    // Tempo/Interval band.
    assert!(
        run.evidence.citation.to_lowercase().contains("hrv")
            || run
                .why
                .basis
                .to_lowercase()
                .contains("changed to an easy run"),
        "downgraded run must cite the adjustment, not the original band: \
             grade={:?} citation={} basis={}",
        run.evidence.grade,
        run.evidence.citation,
        run.why.basis,
    );
}

#[test]
fn readiness_adjustments_resync_the_prefill_rpe_target() {
    // An adjusted session is tappable, so its numeric prefill target must
    // follow the readiness-capped label: a tapped tile must never seed an RPE
    // closer to failure than the shown (capped) prescription (HARD RULE 3).
    // CapRpe: "RIR 3" (RPE 7.0) +1 RIR → "RIR 4" (RPE 6.0).
    let lift = PrescriptionView {
        exercise: "Back Squat".into(),
        intensity_label: "RIR 3".into(),
        intensity: PlanIntensity::Rir { rir: 3 },
        target_rpe: Some(crate::strength::rir_to_rpe(3.0)),
        ..Default::default()
    };
    let mut ns = SessionPlanView {
        items: vec![lift],
        ..Default::default()
    };
    apply_adjustments_to_session(
        &mut ns,
        &[graded(Adjustment::CapRpe(1), claims::AUTOREG_E1RM_GATE_001)],
        false,
    );
    assert_eq!(
        ns.items[0].intensity_label, "RIR 4",
        "the RIR label is capped up"
    );
    assert_eq!(
        ns.items[0].target_rpe,
        Some(crate::strength::rir_to_rpe(4.0)),
        "CapRpe resyncs target_rpe to the capped RIR (RPE 6.0), not the pre-cap 7.0"
    );

    // DowngradeSession caps an aggressive RIR 1–2 (RPE 8.0) dynamic item to the
    // easy 3-RIR ceiling (RPE 7.0); it must lower, never raise, the target.
    let aggressive = PrescriptionView {
        exercise: "Full push-up".into(),
        intensity_label: "RIR 1–2".into(),
        intensity: PlanIntensity::RirRange { lo: 1, hi: 2 },
        target_rpe: Some(8.0),
        ..Default::default()
    };
    let mut ns2 = SessionPlanView {
        items: vec![aggressive],
        ..Default::default()
    };
    apply_adjustments_to_session(
        &mut ns2,
        &[graded(Adjustment::DowngradeSession, claims::HRV_001)],
        false,
    );
    assert!(
        ns2.items[0].target_rpe.unwrap() <= crate::strength::rir_to_rpe(3.0),
        "a downgraded item's prefill RPE must not exceed the easy ceiling: {:?}",
        ns2.items[0].target_rpe
    );
}

#[test]
fn tolerable_knee_tendon_pain_eases_only_the_lifts_that_load_the_knee() {
    // final-today F1/#9: a 3/10 stable knee tendon report eases the leg lifts
    // (no load, a face that says what to load) and holds the upper body at
    // its held load with one rep more in reserve, never stacked.
    let mut bench = lift_item("Bench press", 85.0);
    bench.hold_load_cap_kg = Some(80.0);
    let mut ns = session(vec![
        lift_item("Back squat", 120.0),
        bench,
        lift_item("Cable row", 60.0),
    ]);
    let modify = graded(
        Adjustment::ModifyAndMonitor {
            location: "Right knee".into(),
        },
        claims::SAFE_TENDON_001,
    );
    let hrv = graded(
        Adjustment::CapTopEnd {
            trigger: ReadinessSignal::HrvLnRmssd,
        },
        claims::HRV_001,
    );
    apply_adjustments_to_session(&mut ns, &[modify, hrv], false);
    let squat = &ns.items[0];
    assert_eq!(squat.load_kg, None);
    assert_eq!(squat.face_lead, "3×5 · pick a weight for ~3 reps left");
    let bench = &ns.items[1];
    assert_eq!(bench.load_kg, Some(80.0), "held at its cap");
    assert_eq!(
        bench.intensity,
        PlanIntensity::Rir { rir: 3 },
        "one hold, not stacked"
    );
    assert!(bench.face_lead.contains("@ 80 kg"), "{}", bench.face_lead);
    assert!(!bench.pick_load);
    let row = &ns.items[2];
    assert_eq!(
        row.load_kg,
        Some(60.0),
        "a seated cable row spares the knee"
    );
    assert_eq!(row.intensity, PlanIntensity::Rir { rir: 3 });
    assert!(squat.pick_load);
    let call = ns.adjustment.expect("adjusted").summary;
    assert!(
        call.contains("Right knee") && call.ends_with("Today: Back squat."),
        "{call}"
    );
}

#[test]
fn an_unlocated_tendon_report_still_eases_every_lift() {
    let mut ns = session(vec![
        lift_item("Back squat", 120.0),
        lift_item("Bench press", 85.0),
    ]);
    let modify = graded(
        Adjustment::ModifyAndMonitor {
            location: String::new(),
        },
        claims::SAFE_TENDON_001,
    );
    apply_adjustments_to_session(&mut ns, &[modify], false);
    assert!(ns.items.iter().all(|it| it.load_kg.is_none()));
    assert!(
        ns.items
            .iter()
            .all(|it| it.face_lead.contains("pick a weight"))
    );
}
