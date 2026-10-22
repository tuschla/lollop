//! The lift view and session audit: e1RM cross-check, Prilepin rows only for
//! strength work, and the audit buckets by local day.

use super::*;

#[test]
fn hypertrophy_day_as_prescribed_gets_no_prilepin_row() {
    // Prilepin's optimal-volume table is a strength/power construct. A 4×8 @ RPE 8
    // back squat is hypertrophy work (sets of >6 reps) and gets no Prilepin row.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    for _ in 0..4 {
        app.update(
            log_set("Barbell back squat", 100.0, 8, 8.0, 100 * WEEK),
            &mut model,
        )
        .expect_only_render();
    }
    let audit = app.view(&model).lift_audit;
    assert!(
        !audit.iter().any(|r| r.summary.contains("of your max is")),
        "an 8-rep hypertrophy set is outside Prilepin's table: {:?}",
        audit.iter().map(|r| &r.summary).collect::<Vec<_>>()
    );
}

#[test]
fn lift_view_cross_checks_e1rm_across_formulas_when_reliable() {
    let app = Engine;
    let mut model = Model::default();
    app.update(log_set("Cross Check Squat", 100.0, 5, 8.0, 0), &mut model)
        .expect_only_render();
    // 12 reps is past the strength-006 reliability cap → no range.
    app.update(log_set("Cross Check Squat", 60.0, 12, 8.0, 0), &mut model)
        .expect_only_render();

    let lifts = app.view(&model).lifts;
    let check = lifts[0].cross_check.as_ref().expect("range for 5 reps");
    assert_eq!(check.formulas, 3);
    assert!(check.low_kg <= check.high_kg);
    // At RPE 8 the set is 7 effective reps (5 + 2 in reserve), the same reps
    // the headline uses: Brzycki 100×36/30 = 120.0 (the low), Epley 123.3
    // (the high, = the headline), Lombardi ≈ 121.5 between them.
    assert!((check.low_kg - 120.0).abs() < 0.1, "low {}", check.low_kg);
    assert!(
        (check.high_kg - 123.3).abs() < 0.1,
        "high {}",
        check.high_kg
    );
    assert!(
        check.low_kg <= lifts[0].e1rm_kg && lifts[0].e1rm_kg <= check.high_kg,
        "the headline {} sits inside its own range",
        lifts[0].e1rm_kg
    );
    assert!(!check.evidence.citation.is_empty());
    assert!(lifts[1].cross_check.is_none(), "12 reps is unreliable");
}

#[test]
fn lift_audit_flags_only_checks_that_ask_for_a_change() {
    let app = Engine;
    let mut model = Model::default();
    let mut p = sample_profile();
    p.bodyweight_kg = Some(80.0);
    app.update(Event::SetProfile(p.clone()), &mut model)
        .expect_only_render();
    let squat_set = || log_set("Barbell back squat", 140.0, 5, 8.5, 100 * WEEK);
    // 3 × 5 @ ~86 %1RM → 15 total reps, inside the Prilepin 80–90 window, and a
    // squat e1RM (Epley ≈163 kg) above 1.5× an 80 kg bodyweight: both checks
    // pass, so the audit says nothing.
    for _ in 0..3 {
        app.update(squat_set(), &mut model).expect_only_render();
    }
    let audit = app.view(&model).lift_audit;
    assert!(
        audit.is_empty(),
        "passed checks emit no rows: {:?}",
        audit.iter().map(|r| &r.summary).collect::<Vec<_>>()
    );

    // 6 × 5 → 30 total reps, past the 10–20 window: one plain row, no jargon.
    for _ in 0..3 {
        app.update(squat_set(), &mut model).expect_only_render();
    }
    let audit = app.view(&model).lift_audit;
    let volume = audit
        .iter()
        .find(|r| r.summary.contains("30 reps"))
        .expect("out-of-range volume row");
    assert!(
        volume.summary.contains("more than the usual 10–20"),
        "{}",
        volume.summary
    );
    assert!(!volume.summary.contains("Prilepin"), "{}", volume.summary);
    assert!(
        !volume.evidence.safety_critical,
        "a volume note is not a safety call"
    );

    // A 120 kg bodyweight puts the squat under 1.5× BW: the unmet gate stays a
    // safety call.
    p.bodyweight_kg = Some(120.0);
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    let audit = app.view(&model).lift_audit;
    let dj = audit
        .iter()
        .find(|r| r.summary.contains("depth jumps"))
        .expect("depth-jump hold row");
    assert!(
        dj.summary.starts_with("Hold off on depth jumps"),
        "{}",
        dj.summary
    );
    assert!(dj.evidence.safety_critical, "{}", dj.summary);
}

// ── The lift audit buckets sets by the shell's LOCAL day ──
#[test]
fn lift_audit_buckets_sets_by_local_day() {
    // In a -3h zone a set logged 23:30 LOCAL Monday is 02:30 UTC Tuesday, and
    // a set at 20:00 LOCAL Monday stays on UTC Monday. All bucket to LOCAL
    // Monday, so the Prilepin audit sums them (25 reps, past the 10–20 window,
    // so a row is emitted). The old raw-UTC audit split them across UTC Mon/Tue
    // (15 + 10 reps, each inside the window, so no row at all).
    let app = Engine;
    let mut model = Model::default();
    let off = -3 * 3600;
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    let late = MON * DAY_SEC + 23 * 3600 + 30 * 60 - off; // 23:30 local Mon
    let early = MON * DAY_SEC + 20 * 3600 - off; // 20:00 local Mon
    for (id, at) in [
        (1, late),
        (2, early),
        (3, late + 300),
        (4, late + 600),
        (5, early + 300),
    ] {
        app.update(log_set_id("Back Squat", 120.0, id, at), &mut model)
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
            epoch_day: MON + 2,
            utc_offset_sec: off,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    // Audit: every set buckets on local Monday → 5 × 5 = 25 reps.
    let squat = vm
        .lift_audit
        .iter()
        .find(|r| r.summary.contains("Back Squat"))
        .expect("squat Prilepin audit row");
    assert!(
        squat.summary.contains("25 reps"),
        "all same-local-day sets are counted: {}",
        squat.summary
    );
}

// ── The Session audit sums only the latest lifting day ──
#[test]
fn lift_audit_sums_only_the_latest_lifting_day() {
    // 4×5 Back squat yesterday, then 5×5 today (one set under a different casing).
    // The Session audit reports only today's 25 reps as ONE merged exercise, never
    // the 45-rep cross-session sum (yesterday's 20 alone is inside the window).
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    for i in 0..4u64 {
        app.update(
            log_set_id("Back squat", 100.0, i + 1, MON * DAY_SEC + 12 * 3600),
            &mut model,
        )
        .expect_only_render();
    }
    app.update(
        log_set_id("Back squat", 100.0, 11, (MON + 1) * DAY_SEC + 12 * 3600),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set_id(
            "Back squat",
            100.0,
            12,
            (MON + 1) * DAY_SEC + 12 * 3600 + 60,
        ),
        &mut model,
    )
    .expect_only_render();
    for (id, name, t) in [
        (13, "back squat", 120),
        (14, "Back squat", 180),
        (15, "Back squat", 240),
    ] {
        app.update(
            log_set_id(name, 100.0, id, (MON + 1) * DAY_SEC + 12 * 3600 + t),
            &mut model,
        )
        .expect_only_render();
    }
    app.update(
        Event::SetToday {
            epoch_day: MON + 1,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let rows: Vec<_> = vm
        .lift_audit
        .iter()
        .filter(|r| {
            r.summary.to_lowercase().contains("back squat") && r.summary.contains("reps at")
        })
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "today's sets (both casings) merge to one exercise row: {:#?}",
        vm.lift_audit
    );
    assert!(
        rows[0].summary.contains("25 reps"),
        "only today's 5×5 is counted: {}",
        rows[0].summary
    );
    assert!(
        !rows[0].summary.contains("45 reps"),
        "the audit must not sum across days: {}",
        rows[0].summary
    );
}

// ── An unrecorded RPE fabricates no RIR row or summary clause ──
#[test]
fn an_unrecorded_rpe_fabricates_no_rir() {
    // A set logged with no RPE (0.0) must not fabricate a "~10 RIR" audit row nor
    // an "@RPE0.0 / 10.0 RIR" lift summary.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        with_entry_id(
            log_set("Bench", 100.0, 5, 0.0, MON * DAY_SEC + 12 * 3600),
            1,
        ),
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
    assert!(
        !vm.lift_audit
            .iter()
            .any(|r| r.summary.contains("Past 5 reps left")),
        "no RIR-reliability row from an unrecorded RPE: {:?}",
        vm.lift_audit.iter().map(|r| &r.summary).collect::<Vec<_>>()
    );
    let lift = vm
        .lifts
        .iter()
        .find(|l| l.exercise == "Bench")
        .expect("bench lift present");
    assert!(
        lift.summary.contains("RPE not recorded"),
        "summary states the unrecorded RPE: {}",
        lift.summary
    );
    assert!(
        !lift.summary.contains("RIR"),
        "no fabricated RIR: {}",
        lift.summary
    );
    assert!(
        !lift.summary.contains("@RPE"),
        "no fabricated @RPE: {}",
        lift.summary
    );
}

#[test]
fn weights_profile_still_audits_a_barbell_lift_sharing_a_tier_name() {
    // Regression guard: a Weights user logging a loaded "Bulgarian split squat"
    // (also a calisthenics rung name) must keep its e1RM anchor + audit row —
    // the ladder-name suppression is gated on the Calisthenics modality only.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    // 5 × 5 = 25 reps sits past the 10–20 window, so the audit row is emitted.
    for _ in 0..5 {
        app.update(
            log_set("Bulgarian split squat", 60.0, 5, 8.0, DAY_SEC),
            &mut model,
        )
        .expect_only_render();
    }
    let vm = app.view(&model);
    assert!(
        vm.lift_audit
            .iter()
            .any(|g| g.summary.contains("Bulgarian split squat")),
        "a Weights user's barbell lift is still audited"
    );
    let anchors = build_plan_anchors(model.sets.iter(), AnchorCtx::default(), 0, 0);
    assert!(
        anchors.e1rm_for("Bulgarian split squat").is_some(),
        "a Weights user's barbell lift still anchors an e1RM"
    );
}
