//! Strength coaching: e1RM anchor windowing/aliasing/detraining, the layoff
//! re-entry ramp, the e1RM-drop load cut, and the lift card's effort target,
//! plates and "last time" line.

use super::*;
use crate::plan::DerateCause;
use crate::schema::{LiftIntensity, LiftPrescription, Prescription};

fn set(exercise: &str, weight_kg: f64, reps: u32, rpe: f64, day: i64) -> LoggedSet {
    LoggedSet {
        hold_sec: None,
        exercise: exercise.into(),
        weight_kg,
        reps,
        rpe,
        observed_at: day * DAY_SEC + 3_600,
        entry_id: day as u64,
    }
}

fn anchors_at(sets: &[LoggedSet], ctx: AnchorCtx, today: i64) -> crate::plan::Anchors {
    build_plan_anchors(
        sets.iter().filter(|s| s.observed_at / DAY_SEC < today),
        ctx,
        today,
        0,
    )
}

fn layoff(weeks_off: f64, returned: i64) -> AnchorCtx {
    AnchorCtx {
        layoff: Some(Layoff {
            weeks_off,
            returned_epoch_day: Some(returned),
        }),
        ..Default::default()
    }
}

#[test]
fn one_rep_set_e1rm_equals_load() {
    // A single is its own 1RM (no +3.3 % Epley inflation).
    let a = anchors_at(
        &[set("Back Squat", 140.0, 1, 0.0, 10)],
        AnchorCtx::default(),
        11,
    );
    assert_eq!(a.e1rm_for("Back Squat"), Some(140.0));
}

#[test]
fn anchor_tracks_recent_window_not_all_time_max() {
    // One optimistic old best must not pin every later load: the anchor is the
    // best of the last 3 sessions / 21 days.
    let mut sets = vec![set("Back Squat", 120.0, 5, 8.0, 0)];
    for d in [30, 37, 44, 51] {
        sets.push(set("Back Squat", 100.0, 5, 8.0, d));
    }
    let a = anchors_at(&sets, AnchorCtx::default(), 52);
    let e = a.e1rm_for("Back Squat").unwrap();
    let recent = (crate::strength::e1rm_epley_rpe(100.0, 5, 8.0) * 10.0).round() / 10.0;
    assert_eq!(e, recent, "anchor = recent window best, not the day-0 PR");
}

#[test]
fn anchor_merges_same_movement_names() {
    let sets = [
        set("Back squat", 100.0, 5, 8.0, 0),
        set("Barbell back squat", 110.0, 5, 8.0, 2),
    ];
    let a = anchors_at(&sets, AnchorCtx::default(), 3);
    assert_eq!(
        a.lift_e1rm.len(),
        1,
        "one anchor per movement: {:?}",
        a.lift_e1rm
    );
    let want = (crate::strength::e1rm_epley_rpe(110.0, 5, 8.0) * 10.0).round() / 10.0;
    assert_eq!(a.e1rm_for("Back squat"), Some(want));
    assert_eq!(a.e1rm_for("Barbell back squat"), Some(want));
}

#[test]
fn unrated_compliant_sets_hold_the_anchor_instead_of_failing_it() {
    // An unrated set is a lower bound, not a set to failure. A
    // rated 100×5 @ RPE 8 anchors ~123 kg; later unrated working sets at the
    // prescribed ~85 % (3 reps) keep that anchor instead of dragging it to
    // the reps-as-failure estimate (~94 kg).
    let mut sets = vec![set("Back Squat", 100.0, 5, 8.0, 0)];
    for d in [7, 14, 21, 28] {
        sets.push(set("Back Squat", 105.0, 3, 0.0, d));
    }
    let a = anchors_at(&sets, AnchorCtx::default(), 29);
    let rated = (crate::strength::e1rm_epley_rpe(100.0, 5, 8.0) * 10.0).round() / 10.0;
    assert_eq!(a.e1rm_for("Back Squat"), Some(rated));
}

#[test]
fn speed_set_without_rpe_does_not_create_or_move_an_anchor() {
    // Unrated sets under 70 % of the anchor are speed work.
    let mut sets = vec![set("Back Squat", 100.0, 5, 8.0, 0)];
    for d in [3, 7, 10, 14] {
        sets.push(set("Back Squat", 60.0, 3, 0.0, d));
    }
    let a = anchors_at(&sets, AnchorCtx::default(), 15);
    let rated = (crate::strength::e1rm_epley_rpe(100.0, 5, 8.0) * 10.0).round() / 10.0;
    assert_eq!(a.e1rm_for("Back Squat"), Some(rated));
}

#[test]
fn lift_anchor_after_44_days_is_70pct_not_50pct() {
    // 44 days without the lift scales the working load to 70 %
    // (DETRAIN-STR-001), never a linear-to-zero 50 %.
    let a = anchors_at(
        &[set("Back Squat", 100.0, 5, 8.0, 0)],
        AnchorCtx::default(),
        44,
    );
    let d = a.load_frac_for("Back Squat").expect("a detraining derate");
    assert_eq!(d.frac, 0.70);
    assert!(matches!(
        d.cause,
        DerateCause::Detraining { days_since: 44 }
    ));
    // Past 8 weeks the anchor is dropped (fresh, novice-style start).
    let gone = anchors_at(
        &[set("Back Squat", 100.0, 5, 8.0, 0)],
        AnchorCtx::default(),
        60,
    );
    assert!(gone.e1rm_for("Back Squat").is_none());
}

#[test]
fn declared_layoff_and_inactivity_do_not_stack() {
    // A declared 6 wk layoff returning on day 44: the pre-layoff anchor takes
    // the re-entry fraction INSTEAD of the detraining credit (0.70, not 0.49).
    let a = anchors_at(&[set("Back Squat", 100.0, 5, 8.0, 0)], layoff(6.0, 44), 44);
    let d = a.load_frac_for("Back Squat").expect("a re-entry derate");
    assert_eq!(d.frac, 0.70);
    assert!(matches!(d.cause, DerateCause::Reentry { .. }));
    assert_eq!(a.lift_derates.len(), 1, "one derate, never stacked");
}

#[test]
fn six_weeks_off_starts_near_70pct_and_ramps_to_full() {
    // 6 wk off → ~70 % on the return day, ramping to 100 %
    // over the 4-wk window; the athlete trains the lift lightly meanwhile.
    let returned = 50;
    let mut sets = vec![set("Back Squat", 100.0, 5, 8.0, 0)];
    for d in [returned, returned + 7, returned + 14, returned + 21] {
        sets.push(set("Back Squat", 70.0, 5, 0.0, d));
    }
    let frac_on = |today: i64| {
        anchors_at(&sets, layoff(6.0, returned), today)
            .load_frac_for("Back Squat")
            .map_or(1.0, |d| d.frac)
    };
    assert_eq!(frac_on(returned), 0.70);
    let mid = frac_on(returned + 14);
    assert!((mid - 0.85).abs() < 1e-3, "half-way up the ramp: {mid}");
    assert_eq!(frac_on(returned + 28), 1.0, "full load once the ramp ends");
    let pre = (crate::strength::e1rm_epley_rpe(100.0, 5, 8.0) * 10.0).round() / 10.0;
    assert_eq!(
        anchors_at(&sets, layoff(6.0, returned), returned + 28).e1rm_for("Back Squat"),
        Some(pre),
        "the pre-layoff anchor is back at 100 %"
    );
}

#[test]
fn post_return_anchor_is_unscaled_and_long_layoff_window_ends() {
    // A heavier post-return set wins unscaled; a >8 wk layoff sets pre-layoff
    // anchors aside only while its window runs.
    let sets = [
        set("Back Squat", 100.0, 5, 8.0, 0),
        set("Back Squat", 95.0, 5, 8.0, 101),
    ];
    let a = anchors_at(&sets, layoff(12.0, 100), 102);
    assert!(a.reentry_novice);
    let post = (crate::strength::e1rm_epley_rpe(95.0, 5, 8.0) * 10.0).round() / 10.0;
    assert_eq!(a.e1rm_for("Back Squat"), Some(post));
    assert!(
        a.load_frac_for("Back Squat").is_none(),
        "post-return anchors are unscaled"
    );
    let later = anchors_at(&sets, layoff(12.0, 100), 100 + 43);
    assert!(
        !later.reentry_novice,
        "the novice window ends with the ramp"
    );
}

#[test]
fn rated_flat_sessions_report_a_stall_unrated_ones_never_do() {
    // STR-STALL-001 input: ≥2 rated sessions failing to beat the anchor over
    // 3 weeks → 3 weeks stalled; the same history logged unrated is no stall.
    let rated: Vec<LoggedSet> = [0, 7, 14, 21]
        .iter()
        .map(|&d| set("Back Squat", 100.0, 5, 8.0, d))
        .collect();
    let a = anchors_at(&rated, AnchorCtx::default(), 22);
    assert_eq!(a.lift_stall_weeks, vec![("Back Squat".to_string(), 3)]);
    let unrated: Vec<LoggedSet> = [0, 7, 14, 21]
        .iter()
        .map(|&d| set("Back Squat", 100.0, 5, 0.0, d))
        .collect();
    assert!(
        anchors_at(&unrated, AnchorCtx::default(), 22)
            .lift_stall_weeks
            .is_empty()
    );
}

#[test]
fn missed_target_cuts_next_load_5pct_unless_off_day() {
    // AUTOREG-E1RM-GATE-001: a session whose rated e1RM lands
    // >5 % under the baseline cuts that lift's next working load ~5 %.
    let sets = [
        set("Back Squat", 100.0, 5, 8.0, 0),
        set("Back Squat", 100.0, 5, 8.0, 7),
        set("Back Squat", 100.0, 5, 10.0, 14),
    ];
    let a = anchors_at(&sets, AnchorCtx::default(), 15);
    let d = a.load_frac_for("Back Squat").expect("an e1RM-drop cut");
    assert_eq!(d.frac, 0.95);
    assert!(matches!(d.cause, DerateCause::E1rmDrop));
    let off = AnchorCtx {
        off_day: Some(14),
        ..Default::default()
    };
    assert!(
        anchors_at(&sets, off, 15)
            .load_frac_for("Back Squat")
            .is_none(),
        "a session marked as an off day keeps the load"
    );
}

fn pct_rx(pct: f32, reps: u8, velocity: Option<u8>) -> Recommended<Prescription> {
    graded(
        Prescription::Lift(LiftPrescription {
            exercise: "Back Squat".into(),
            sets: 4,
            reps,
            intensity: LiftIntensity::PercentOneRm(pct),
            rest_sec: 180,
            velocity_loss_pct: velocity,
            reps_max: None,
            load_kg: None,
        }),
        claims::STR_DUP_001,
    )
}

#[test]
fn percent_item_carries_expected_target_rpe_and_plates() {
    // 85 % × 3 implies ~2 reps in reserve → RPE 8 on the face and the
    // editor hint. The barbell load comes with its plates.
    let anchors = crate::plan::Anchors {
        lift_e1rm: vec![("Back Squat".into(), 100.0)],
        ..Default::default()
    };
    let it = flatten_prescription(&pct_rx(85.0, 3, None), &anchors, None, false);
    assert_eq!(it.load_kg, Some(85.0));
    assert_eq!(it.target_rpe, Some(8.0));
    assert!(it.summary.contains("~2 reps left"), "{}", it.summary);
    assert!(
        !it.summary.contains("e1RM"),
        "the load, not the %, is on the face: {}",
        it.summary
    );
    assert_eq!(it.plates_per_side, vec![20.0, 10.0, 2.5]);
}

#[test]
fn power_item_face_shows_move_fast_without_a_device_number() {
    // The speed cue is on the face and needs no device.
    let anchors = crate::plan::Anchors {
        lift_e1rm: vec![("Back Squat".into(), 100.0)],
        ..Default::default()
    };
    let it = flatten_prescription(&pct_rx(50.0, 3, Some(10)), &anchors, None, false);
    assert_eq!(it.effort_cue, "move fast");
    assert!(it.summary.contains("move fast"), "{}", it.summary);
    assert!(it.why.basis.contains("visibly slows"), "{}", it.why.basis);
    assert!(!it.why.basis.contains("~10%"), "{}", it.why.basis);
}

#[test]
fn derated_card_names_the_reason_and_cites_detraining() {
    let anchors = crate::plan::Anchors {
        lift_e1rm: vec![("Back Squat".into(), 100.0)],
        lift_derates: vec![crate::plan::LiftDerate {
            exercise: "Back Squat".into(),
            frac: 0.70,
            cause: DerateCause::Detraining { days_since: 44 },
        }],
        ..Default::default()
    };
    let it = flatten_prescription(&pct_rx(80.0, 5, None), &anchors, None, false);
    assert_eq!(
        it.load_kg,
        Some(55.0),
        "100 × 0.80 × 0.70 = 56 → 55 on the grid"
    );
    assert!(it.why.basis.contains("scaled to 70%"), "{}", it.why.basis);
    assert_eq!(
        it.evidence.grade,
        Some(EvidenceGrade::Moderate),
        "re-pointed to DETRAIN-STR-001"
    );
}

#[test]
fn plates_per_side_greedy_kg_and_lb() {
    assert_eq!(
        plates_per_side(117.5, false),
        vec![20.0, 20.0, 5.0, 2.5, 1.25]
    );
    assert!(plates_per_side(20.0, false).is_empty());
    // 225 lb on a 45 lb bar = 90 lb per side.
    assert_eq!(plates_per_side(225.0 / LB_PER_KG, true), vec![45.0, 45.0]);
}

#[test]
fn rir_item_carries_last_set_and_a_suggested_load() {
    // An effort-prescribed lift shows the athlete's last top
    // set from a prior day and starts from its weight — one plate step up
    // here, because 60 kg × 8 @ RPE 7 beat the 5-rep / 3-RIR target by 3 reps
    // (STR-LOADPROG-001).
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    app.update(
        with_entry_id(log_set("Row", 60.0, 8, 7.0, (MON - 1) * DAY_SEC + 3_600), 1),
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
    let item = app
        .view(&model)
        .week_plan
        .into_iter()
        .flat_map(|s| s.items.into_iter())
        .find(|i| crate::plan::same_movement(&i.exercise, "Row") && i.load_kg.is_none())
        .expect("an effort-prescribed row item");
    let last = item.last_set.expect("last time is shown");
    assert_eq!((last.weight_kg, last.reps), (60.0, 8));
    assert_eq!(item.suggested_load_kg, Some(62.5));
    assert!(
        item.anchored_on.starts_with("Last: "),
        "{}",
        item.anchored_on
    );
}

/// An effort-prescribed (3×`reps` at 3 RIR) row item after applying the last
/// set: one 60 kg × 8 @ RPE 7 row logged `age_days` before day 100.
fn rir_row_item(anchors: &crate::plan::Anchors, age_days: i64, reps: u8) -> PrescriptionView {
    let today = 100;
    let rx = graded(
        Prescription::Lift(LiftPrescription {
            exercise: "Row".into(),
            sets: 3,
            reps,
            intensity: LiftIntensity::Rir(3),
            rest_sec: 120,
            velocity_loss_pct: None,
            reps_max: None,
            load_kg: None,
        }),
        claims::STR_INTENT_001,
    );
    let mut items = vec![flatten_prescription(&rx, anchors, None, false)];
    let mut model = Model::default();
    model.sets.push(set("Row", 60.0, 8, 7.0, today - age_days));
    apply_last_sets(
        &mut items,
        &model,
        anchors,
        &[Some(today - age_days)],
        today,
        false,
    );
    items.remove(0)
}

#[test]
fn no_start_weight_after_a_break_the_plan_treats_as_a_fresh_start() {
    // A3: past the 8-week horizon (DETRAIN-STR-001) or inside a declared
    // >8 wk re-entry (REENTRY-001: treat as novice) the plan carries no load,
    // so "Start from …" must not bring the pre-break weight back.
    let none = crate::plan::Anchors::default();
    let old = rir_row_item(&none, 70, 5);
    assert!(old.last_set.is_some(), "last time is still shown");
    assert_eq!(old.suggested_load_kg, None);
    let reentry = crate::plan::Anchors {
        reentry_novice: true,
        ..Default::default()
    };
    assert_eq!(rir_row_item(&reentry, 3, 5).suggested_load_kg, None);
    // Control: a recent set that beat the 5-rep / 3-RIR target by 3 reps
    // suggests one step up (STR-LOADPROG-001).
    assert_eq!(rir_row_item(&none, 3, 5).suggested_load_kg, Some(62.5));
}

#[test]
fn start_weight_after_a_six_week_gap_is_derated() {
    // A3: 31–56 days since the set → the detraining credit (70 %) scales the
    // suggestion, never the full pre-gap load.
    let item = rir_row_item(&crate::plan::Anchors::default(), 40, 5);
    assert_eq!(item.suggested_load_kg, Some(40.0), "60 × 0.70 = 42 → 40");
    assert!(item.why.basis.contains("70%"), "{}", item.why.basis);
}

#[test]
fn start_weight_is_lightened_for_a_higher_rep_target() {
    // A3: 60 kg × 8 @ RPE 7 is 11 reps of capacity; 12 reps at 3 RIR needs
    // 15. Epley scales it to 60 × (1 + 11/30) / (1 + 15/30) ≈ 54.7 → 52.5.
    let item = rir_row_item(&crate::plan::Anchors::default(), 3, 12);
    assert_eq!(item.suggested_load_kg, Some(52.5));
}

#[test]
fn an_undeclared_gap_ramps_back_instead_of_jumping_to_full_load() {
    // A2: squat e1RM 150 on days 0/3/7, then nothing until day 47. Day 47 is
    // prescribed at the 70 % detraining credit; the light session logged that
    // day must not restore 100 % on day 48 (REENTRY-001 4–8 wk: 70 %, ramp
    // 2–4 weeks). Rated or not, the next load stays on the re-entry ramp.
    let base: Vec<LoggedSet> = [0, 3, 7]
        .iter()
        .map(|&d| set("Back Squat", 125.0, 5, 9.0, d))
        .collect();
    let on_47 = anchors_at(&base, AnchorCtx::default(), 47);
    assert_eq!(on_47.e1rm_for("Back Squat"), Some(150.0));
    let d47 = on_47
        .load_frac_for("Back Squat")
        .expect("detraining credit");
    assert_eq!(d47.frac, 0.70);
    let heavy_47 = round_load_grid(150.0 * 0.85 * d47.frac, false);
    assert_eq!(heavy_47, 87.5);
    for rpe in [0.0, 7.0] {
        let mut sets = base.clone();
        for _ in 0..3 {
            sets.push(set("Back Squat", heavy_47, 3, rpe, 47));
        }
        let on_48 = anchors_at(&sets, AnchorCtx::default(), 48);
        let d = on_48
            .load_frac_for("Back Squat")
            .unwrap_or_else(|| panic!("rpe {rpe}: still on the ramp"));
        assert!(matches!(d.cause, DerateCause::Reentry { .. }), "{d:?}");
        assert!(d.frac < 0.75, "rpe {rpe}: {}", d.frac);
        let e = on_48.e1rm_for("Back Squat").unwrap();
        let heavy_48 = round_load_grid(e * 0.85 * d.frac, false);
        assert!(
            heavy_48 <= heavy_47 * 1.10,
            "rpe {rpe}: {heavy_47} → {heavy_48}"
        );
    }
}

#[test]
fn an_open_ended_effort_set_is_a_lower_bound_for_the_anchor() {
    // C11: "4+ reps left" (RPE 6) counts its reps as performed, like an
    // unrated set: it can raise the anchor only by what was actually lifted
    // and never cuts it.
    let base = set("Back Squat", 100.0, 5, 8.0, 0);
    let rated = (crate::strength::e1rm_epley_rpe(100.0, 5, 8.0) * 10.0).round() / 10.0;
    let easy_heavy = [base.clone(), set("Back Squat", 110.0, 5, 6.0, 3)];
    let a = anchors_at(&easy_heavy, AnchorCtx::default(), 4);
    let lower_bound = (crate::strength::e1rm_epley(110.0, 5) * 10.0).round() / 10.0;
    assert_eq!(a.e1rm_for("Back Squat"), Some(lower_bound));
    let easy_light = [base, set("Back Squat", 90.0, 5, 6.0, 3)];
    let a = anchors_at(&easy_light, AnchorCtx::default(), 4);
    assert_eq!(a.e1rm_for("Back Squat"), Some(rated));
    assert!(
        a.load_frac_for("Back Squat").is_none(),
        "never an e1RM-drop cut"
    );
}

#[test]
fn warm_up_sets_do_not_silence_the_prilepin_audit() {
    // B3: a warm-up ramp (STR-WARMUP-001) is not working volume. 5×3 at the
    // top load is 15 reps above 90 % (Prilepin 4–10): the row must fire even
    // though the warm-ups would pull the day's mean intensity under 70 %.
    let mut model = Model::default();
    let mut push = |kg: f64, reps: u32, i: i64| {
        let mut s = set("Back squat", kg, reps, 0.0, 10);
        s.observed_at += i;
        s.entry_id = 100 + i as u64;
        model.sets.push(s);
    };
    push(20.0, 5, 0);
    push(40.0, 5, 1);
    push(60.0, 3, 2);
    for i in 3..8 {
        push(100.0, 3, i);
    }
    let rows = build_lift_audit(&ViewCtx::new(&model));
    assert!(
        rows.iter()
            .any(|r| r.summary.starts_with("Back squat: 15 reps")),
        "{rows:?}"
    );
}

#[test]
fn a_deload_lift_day_is_titled_as_the_lighter_version() {
    let rx = |claim: ClaimId| {
        graded(
            Prescription::Lift(LiftPrescription {
                exercise: "Back Squat".into(),
                sets: 3,
                reps: 3,
                intensity: LiftIntensity::PercentOneRm(76.5),
                rest_sec: 180,
                velocity_loss_pct: None,
                reps_max: None,
                load_kg: None,
            }),
            claim,
        )
    };
    let session = |claim: ClaimId| crate::schema::Session {
        session_type: SessionType::Lift(LiftSessionType::MaxEffort),
        day: 0,
        prescriptions: vec![rx(claim)],
        dup_day: None,
    };
    assert_eq!(
        plan_session_title(&session(claims::STR_DELOAD_VOL_001)),
        "Heavy day (lighter week)"
    );
    assert_eq!(
        plan_session_title(&session(claims::STR_DUP_001)),
        "Heavy day"
    );
}

#[test]
fn weights_render_once_rounded_in_the_display_unit() {
    // lb: whole or .5 (no "264.6 lb"); kg: rounded before the whole test, so
    // 99.97 kg is "100 kg", never "100.0 kg".
    assert_eq!(fmt_weight(120.0, true), "264.5 lb");
    assert_eq!(fmt_weight(290.0 / LB_PER_KG, true), "290 lb");
    assert_eq!(fmt_weight(99.97, false), "100 kg");
    assert_eq!(fmt_weight(92.5, false), "92.5 kg");
}

#[test]
fn twenty_rep_leg_press_does_not_anchor_a_pct_1rm_load() {
    // A 20-rep set is outside the reliable e1RM window (>10 effective reps), so
    // it never anchors a %1RM working load: the plan stays on RIR (effort)
    // prescriptions instead of a fabricated load.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    app.update(
        log_set("Leg press", 200.0, 20, 8.0, (MON - 1) * DAY_SEC),
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
    let ns = app
        .view(&model)
        .next_session
        .expect("a plan yields a next session");
    for it in ns.items.iter().filter(|i| !i.exercise.is_empty()) {
        assert!(
            it.load_kg.is_none(),
            "no lift may carry a %1RM load off a 20-rep set: {} @ {:?}",
            it.exercise,
            it.load_kg
        );
    }
}

#[test]
fn stale_pr_does_not_revive_on_a_light_set() {
    // A 14-month-old 140 kg PR has fully detrained; a recent light 60 kg set must
    // not inherit its credit. The working load anchors off the 60 kg set.
    let app = Engine;
    let mut model = Model::default();
    // A Monday far enough out that a 400-day-old PR keeps a positive (dated)
    // timestamp — a non-positive stamp reads as undated and skips decay.
    let mon = 403_i64;
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    app.update(
        log_set_id("Back squat", 140.0, 1, (mon - 400) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set_id("Back squat", 60.0, 2, (mon - 1) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
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
    let ns = app
        .view(&model)
        .next_session
        .expect("a plan yields a next session");
    let squat = ns
        .items
        .iter()
        .find(|i| i.exercise.to_lowercase().contains("squat") && i.load_kg.is_some())
        .expect("an anchored squat item");
    let load = squat.load_kg.unwrap();
    assert!(
        load < 90.0,
        "load {load} must derive from the recent 60 kg set, not the stale 140 kg PR"
    );
}

#[test]
fn declared_layoff_derates_the_lift_load_not_the_logged_best() {
    // After a declared layoff the WORKING LOAD is derated (REENTRY-RAMP-001),
    // but the e1RM anchor stays the logged value so the honesty line never
    // lies: build_plan_anchors keeps the e1RM and carries a per-lift derate.
    let sets = [LoggedSet {
        hold_sec: None,
        exercise: "Back Squat".into(),
        weight_kg: 140.0,
        reps: 5,
        rpe: 8.0,
        observed_at: 1_000,
        entry_id: 1,
    }];
    let fresh = build_plan_anchors(sets.iter(), AnchorCtx::default(), 0, 0);
    let returning = build_plan_anchors(
        sets.iter(),
        AnchorCtx {
            layoff: Some(Layoff {
                weeks_off: 6.0,
                returned_epoch_day: None,
            }),
            ..Default::default()
        },
        0,
        0,
    );
    assert_eq!(
        fresh.lift_e1rm[0].1, returning.lift_e1rm[0].1,
        "the e1RM anchor is the logged value either way"
    );
    assert!(fresh.lift_derates.is_empty());
    assert_eq!(
        returning.load_frac_for("Back Squat").map(|d| d.frac),
        Some(0.70),
        "6 wk off → 0.70 re-entry fraction on day 0 of the ramp"
    );
    assert!(matches!(
        returning.load_frac_for("Back Squat").unwrap().cause,
        crate::plan::DerateCause::Reentry { .. }
    ));

    // Flatten an anchored %1RM prescription with vs without the derate.
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
    let anchors_full = crate::plan::Anchors {
        lift_e1rm: vec![("Back Squat".into(), 100.0)],
        ..Default::default()
    };
    let anchors_re = crate::plan::Anchors {
        lift_e1rm: vec![("Back Squat".into(), 100.0)],
        lift_derates: vec![crate::plan::LiftDerate {
            exercise: "Back Squat".into(),
            frac: 0.70,
            cause: crate::plan::DerateCause::Reentry {
                day: 0,
                ramp_days: 28,
            },
        }],
        ..Default::default()
    };
    let full = flatten_prescription(&rx, &anchors_full, None, false);
    let re = flatten_prescription(&rx, &anchors_re, None, false);
    assert_eq!(full.load_kg, Some(round_load_grid(100.0 * 0.80, false)));
    assert_eq!(
        re.load_kg,
        Some(round_load_grid(100.0 * 0.80 * 0.70, false))
    );
    assert!(
        re.load_kg.unwrap() < full.load_kg.unwrap(),
        "the layoff load is lighter"
    );
    assert!(
        re.anchored_on.contains("100 kg (your recent best)"),
        "the logged best stays honest: {}",
        re.anchored_on
    );
    assert_eq!(
        re.evidence.grade,
        Some(EvidenceGrade::ExpertOpinion),
        "derated load re-points to REENTRY-RAMP-001"
    );
    assert_eq!(
        full.evidence.grade,
        Some(EvidenceGrade::Strong),
        "the full load keeps its loading-band grade"
    );
    // The why-panel note must match the re-pointed chip, not the stale band.
    assert!(
        re.why.grade_note.contains("Expert opinion"),
        "grade_note follows the re-pointed chip: {}",
        re.why.grade_note
    );
    assert!(
        !re.why.grade_note.contains("Strong evidence"),
        "no stale Strong note on a derated card"
    );
    assert!(
        full.why.grade_note.contains("Strong evidence"),
        "the full card keeps its band note: {}",
        full.why.grade_note
    );
}

#[test]
fn a_stale_pr_is_dropped_from_the_plan_anchors() {
    // A 14-month-old PR no longer reflects current strength, so it must not
    // anchor a %1RM working load (it decays to zero credit and drops out,
    // leaving a RIR prescription). A recent PR of the same load still anchors.
    let today = 500i64;
    let stale = [LoggedSet {
        hold_sec: None,
        exercise: "Back Squat".into(),
        weight_kg: 140.0,
        reps: 1,
        rpe: 0.0,
        observed_at: (today - 420) * DAY_SEC, // ~14 months ago
        entry_id: 1,
    }];
    let a = build_plan_anchors(stale.iter(), AnchorCtx::default(), today, 0);
    assert!(
        a.e1rm_for("Back Squat").is_none(),
        "a 14-month-old PR no longer anchors a %1RM load"
    );

    let recent = [LoggedSet {
        observed_at: (today - 5) * DAY_SEC,
        ..stale[0].clone()
    }];
    let a2 = build_plan_anchors(recent.iter(), AnchorCtx::default(), today, 0);
    assert!(a2.e1rm_for("Back Squat").is_some(), "a recent PR anchors");
}

#[test]
fn a_weighted_timed_hold_produces_no_plan_anchor() {
    // A weighted hold (e.g. "Weighted plank" at 20 kg) has no e1RM — a hold
    // is isometric time, not a rep-max (CAL-E1RM-001). Without a hold guard,
    // e1rm_epley(20, reps) > 0 (even reps==0 → 20) would fabricate a %1RM plan
    // anchor. The guard skips it on a WEIGHTS profile too (calisthenics=false),
    // where the ladder-name check alone never catches a non-ladder weighted hold.
    let sets = [
        LoggedSet {
            hold_sec: Some(30),
            exercise: "Weighted plank".into(),
            weight_kg: 20.0,
            reps: 0,
            rpe: 7.0,
            observed_at: 100,
            entry_id: 1,
        },
        LoggedSet {
            hold_sec: None,
            exercise: "Back Squat".into(),
            weight_kg: 140.0,
            reps: 5,
            rpe: 8.0,
            observed_at: 200,
            entry_id: 2,
        },
    ];
    let anchors = build_plan_anchors(sets.iter(), AnchorCtx::default(), 0, 0);
    assert!(
        anchors.e1rm_for("Weighted plank").is_none(),
        "a weighted hold must not fabricate a %1RM anchor"
    );
    assert!(
        anchors.e1rm_for("Back Squat").is_some(),
        "a normal barbell set still anchors an e1RM"
    );
}

#[test]
fn long_layoff_drops_the_anchor_and_represcribes_as_novice() {
    // A >8 wk layoff carries NO KB load fraction (Table 3.4b) and directs a
    // fresh-novice re-entry, so while the re-entry window runs
    // `build_plan_anchors` sets `reentry_novice` and sets the pre-layoff anchor
    // aside: `lift_prescription` prescribes the lift by RIR (technique first),
    // never a scaled %e1RM load (HARD RULE 1). The card still cites the
    // re-entry reason.
    let sets = [LoggedSet {
        hold_sec: None,
        exercise: "Back Squat".into(),
        weight_kg: 140.0,
        reps: 5,
        rpe: 8.0,
        observed_at: 1_000,
        entry_id: 1,
    }];
    let ctx = AnchorCtx {
        layoff: Some(Layoff {
            weeks_off: 12.0,
            returned_epoch_day: None,
        }),
        ..Default::default()
    };
    let anchors = build_plan_anchors(sets.iter(), ctx, 0, 0);
    assert!(
        anchors.reentry_novice,
        "12 wk off → treat as a fresh novice"
    );
    assert!(
        anchors.e1rm_for("Back Squat").is_none() && anchors.lift_derates.is_empty(),
        "no invented load fraction beyond 8 wk: the pre-layoff anchor is set aside"
    );

    // >8 wk re-entry: the pipeline prescribes by RIR, not a %1RM anchor.
    let pres = Prescription::Lift(crate::schema::LiftPrescription {
        exercise: "Back Squat".into(),
        sets: 3,
        reps: 5,
        intensity: LiftIntensity::Rir(3),
        rest_sec: 180,
        velocity_loss_pct: None,
        reps_max: None,
        load_kg: None,
    });
    let rx = graded(pres, claims::STR_INTENT_001);
    let item = flatten_prescription(&rx, &anchors, None, false);
    // No anchor-derived %e1RM load: a novice re-entry trains by RIR (HARD RULE 1).
    assert_eq!(item.load_kg, None, "no %e1RM load on a novice re-entry");
    assert!(
        item.intensity_label.starts_with("RIR"),
        "novice re-entry is prescribed by RIR, not % e1RM: {}",
        item.intensity_label
    );
    assert!(
        item.anchored_on.is_empty(),
        "no logged-best load line when the anchor is set aside: {}",
        item.anchored_on
    );
    // The card re-points to REENTRY-001 (ExpertOpinion), citing the reason.
    assert_eq!(
        item.evidence.grade,
        Some(EvidenceGrade::ExpertOpinion),
        "re-points to REENTRY-001"
    );
    assert!(
        item.why.grade_note.contains("Expert opinion"),
        "grade_note follows the re-pointed chip: {}",
        item.why.grade_note
    );
}

// ── A 0 kg set never anchors the plan (no "@ 0 kg") ──
#[test]
fn zero_weight_set_does_not_anchor_a_load() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    // Only a 0 kg "set" is logged for this exercise, dated YESTERDAY so it is
    // part of the history today's prescription is synthesized from (a set logged
    // today never anchors today).
    app.update(
        log_set_id("Back Squat", 0.0, 1, (MON - 1) * DAY_SEC),
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
    let vm = app.view(&model);
    let ns = vm.next_session.expect("a plan renders");
    for it in ns.items.iter().filter(|i| !i.exercise.is_empty()) {
        assert!(
            it.load_kg.is_none_or(|kg| kg > 0.0),
            "a 0 kg set must never anchor a load: {}",
            it.summary
        );
        assert!(!it.summary.contains("@ 0 kg"), "no @ 0 kg: {}", it.summary);
    }
}
