//! Readiness and review adjustments on the rendered session: caps, load cuts,
//! holds, and the prefill targets that follow them.

use super::*;
use crate::schema::PainKind;

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

/// `signal` at `value`, suppressed for `streak` days, reported on `day`.
fn reported(signal: ReadinessSignal, value: f64, streak: u8, day: i64) -> Event {
    Event::SubmitReadiness(ReadinessInput {
        observed_at: day * DAY_SEC + 60,
        streak,
        ..input(signal, value)
    })
}

/// One session's e1RM 7 % under its baseline (autoreg-006): the e1RM gate
/// cuts the load 5 % and stops every set one rep earlier.
fn e1rm_drop(day: i64) -> Event {
    reported(ReadinessSignal::EstimatedOneRm, 0.93, 0, day)
}

/// A 3/10 stable tendon report at `location` on `day`.
fn tendon_pain(location: Option<&str>, day: i64) -> Event {
    let mut r = pain_row(1.0, Some(PainKind::TendonLoadRelated), day * DAY_SEC + 60);
    if let Some(p) = r.pain.as_mut() {
        p.location = location.map(str::to_string);
    }
    Event::SubmitReadiness(r)
}

/// The item of `ns` for `exercise`.
fn item<'a>(ns: &'a SessionPlanView, exercise: &str) -> &'a PrescriptionView {
    ns.items
        .iter()
        .find(|i| i.exercise == exercise)
        .unwrap_or_else(|| panic!("{exercise} in {ns:?}"))
}

#[test]
fn cap_rpe_notes_only_the_items_it_relabels() {
    // The e1RM gate's RPE cap relabels only the RIR-prescribed lifts. The
    // %e1RM squat it never touches keeps its effort and carries only the
    // load cut's note, not a cap note for a change that didn't happen.
    let (app, mut model) = planned_strength_model(true);
    app.update(e1rm_drop(MON), &mut model).expect_only_render();
    let ns = app.view(&model).next_session.expect("today's session");
    let bench = item(&ns, "Bench press");
    assert_eq!(bench.intensity_label, "RIR 4", "RIR 3 capped one rep");
    assert_eq!(bench.adjusted_note, "stop 1 rep earlier than planned");
    let squat = item(&ns, "Back Squat");
    assert_eq!(squat.target_rpe, Some(7.0), "the %e1RM effort is untouched");
    assert_eq!(squat.adjusted_note, "5% less weight today");
}

#[test]
fn a_load_cut_rewrites_the_percent_e1rm_label() {
    // A lighter week (−10 % load) rewrites the "85% e1RM" label to show the
    // cut, with the kg to match: 117.5 kg less 10 %, on the 2.5 kg grid.
    // RIR-style labels are left untouched.
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
    let ns = app.view(&model).next_session.expect("today's session");
    let squat = item(&ns, "Back Squat");
    assert_eq!(squat.intensity_label, "77% e1RM (reduced from 85%)");
    assert_eq!(squat.load_kg, Some(105.0));
    assert!(
        squat.summary.contains("@ 105 kg"),
        "the refreshed summary carries the cut load: {}",
        squat.summary
    );
    assert_eq!(
        item(&ns, "Bench press").intensity_label,
        "RIR 3",
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
    assert!(
        vm.hold_reason.is_some(),
        "a bare pain report holds training"
    );
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

#[test]
fn low_wrapped_next_session_is_not_folded_with_todays_adjustment() {
    // Strength week lifts on days 0/2/4 (Mon/Wed/Fri). With today at day 5
    // (Sat) every training day is behind us, so next_session WRAPS to next
    // Monday (+7). Today's transient readiness downgrade must NOT paint that
    // future-dated session: it would be a wrong-week deload.
    let (app, mut model) = planned_strength_model(true);
    set_today(&app, &mut model, MON + 5);
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
    assert!(
        vm.hold_reason.is_none(),
        "a downgrade must not block training"
    );
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
    set_today(&app, &mut model, MON + 5);
    // Generic pain report → hard Stop → training held.
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
    assert!(vm.hold_reason.is_some(), "generic pain blocks training");
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
    set_today(&app, &mut model, MON);
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
    let (app, mut model) = planned_strength_model(true);
    app.update(e1rm_drop(MON), &mut model).expect_only_render();
    let ns = app.view(&model).next_session.expect("today's session");
    let bench = item(&ns, "Bench press");
    assert_eq!(bench.intensity_label, "RIR 4", "the RIR label is capped up");
    assert_eq!(
        bench.target_rpe,
        Some(6.0),
        "CapRpe resyncs target_rpe to the capped RIR, not the pre-cap 7.0"
    );

    // A multi-day HRV downgrade eases an aggressive "RIR 0–1" (RPE 9.0) rung
    // one level; it lowers, never raises, the target.
    let (app, mut model) = planned_calisthenics();
    let streak = crate::autoreg::HRV_ESCALATE_STREAK;
    for d in 0..i64::from(streak) {
        app.update(
            reported(ReadinessSignal::HrvLnRmssd, -2.0, streak, MON - d),
            &mut model,
        )
        .expect_only_render();
    }
    let ns = app.view(&model).next_session.expect("today's session");
    let push = item(&ns, "Wall push-up");
    assert_eq!(push.intensity_label, "RIR 1–2");
    assert_eq!(
        push.target_rpe,
        Some(8.0),
        "a downgraded item's prefill RPE follows its eased band"
    );
}

/// Back Squat anchored at 117.5 kg and Bench press at 82.5 kg (5×3 at 85 % of
/// their estimates), with 80 kg × 5 the most benched for the reps.
fn squat_and_bench_plan() -> (Engine, Model) {
    planned_on(
        strength_profile(),
        [
            log_set("Back Squat", 120.0, 3, 8.0, (MON - 1) * DAY_SEC),
            log_set("Bench press", 80.0, 5, 8.0, (MON - 1) * DAY_SEC),
        ],
        MON,
    )
}

#[test]
fn tolerable_knee_tendon_pain_eases_only_the_lifts_that_load_the_knee() {
    // A 3/10 stable knee tendon report with a low-HRV week eases the leg lifts
    // (no load, a face that says what to load) and holds the upper body at
    // the most already lifted, each one rep more in reserve, never stacked.
    let (app, mut model) = squat_and_bench_plan();
    app.update(tendon_pain(Some("Right knee"), MON), &mut model)
        .expect_only_render();
    app.update(
        reported(ReadinessSignal::HrvLnRmssd, -0.8, 0, MON),
        &mut model,
    )
    .expect_only_render();
    let ns = app.view(&model).next_session.expect("today's session");
    let squat = item(&ns, "Back Squat");
    assert_eq!(squat.load_kg, None);
    assert_eq!(squat.face_lead, "5×3 · pick a weight for ~4 reps left");
    assert_eq!(
        item(&ns, "Deadlift").face_lead,
        "3×5 · pick a weight for ~4 reps left"
    );
    let bench = item(&ns, "Bench press");
    assert_eq!(bench.load_kg, Some(80.0), "held at the most benched");
    assert_eq!(bench.face_lead, "5×3 @ 80 kg");
    assert_eq!(bench.target_rpe, Some(6.0), "one hold, not stacked");
    let row = item(&ns, "Cable row");
    assert_eq!(
        (row.face_lead.as_str(), row.intensity_label.as_str()),
        ("3×5", "RIR 4"),
        "a seated cable row spares the knee, one rep earlier"
    );
    let call = &ns.adjustment.as_ref().expect("adjusted").summary;
    assert!(
        call.contains("Right knee") && call.ends_with("Today: Back Squat and Deadlift."),
        "{call}"
    );
}

#[test]
fn an_unlocated_tendon_report_still_eases_every_lift() {
    let (app, mut model) = squat_and_bench_plan();
    app.update(tendon_pain(None, MON), &mut model)
        .expect_only_render();
    let ns = app.view(&model).next_session.expect("today's session");
    assert!(ns.items.iter().all(|it| it.load_kg.is_none()), "{ns:?}");
    assert!(
        ns.items
            .iter()
            .all(|it| it.face_lead.contains("pick a weight")),
        "{ns:?}"
    );
}

#[test]
fn cap_rpe_on_a_day_it_cannot_change_does_not_mark_it_adjusted() {
    // The e1RM gate's load cut and RPE cap touch no run: on a run day the
    // session is a genuine no-op, so no "adjusted" chip and no item note.
    let (app, mut model) = planned_on(running_profile(), [], MON);
    let before = app.view(&model).next_session.expect("today's run");
    app.update(e1rm_drop(MON), &mut model).expect_only_render();
    let vm = app.view(&model);
    assert!(!vm.adjustments.is_empty(), "the e1RM call is live");
    let after = vm.next_session.expect("today's run");
    assert_eq!(after.status, SessionStatus::Next);
    assert!(after.adjustment.is_none(), "{:?}", after.adjustment);
    assert_eq!(before.items, after.items, "the run is untouched");
}

/// The calisthenics view fixture (today 20003, an all-calisthenics hero) with
/// one readiness input that touches no calisthenics item: the hero is
/// unchanged and not marked adjusted.
fn assert_unchanged_and_not_adjusted(input: &str) {
    let mut model = snapshot_fixture_model(include_str!(
        "../../../tests/fixtures/view_snapshot/calisthenics.events.ndjson"
    ));
    let before = Engine.view(&model).next_session.expect("hero");
    assert_eq!(before.status, SessionStatus::Next);
    let event: Event = serde_json::from_str(input).unwrap();
    let _ = Engine.update(event, &mut model);
    let vm = Engine.view(&model);
    assert!(!vm.adjustments.is_empty(), "the readiness call is live");
    let after = vm.next_session.expect("hero");
    assert_eq!(before.items, after.items, "the call changes no item");
    assert_eq!(
        after.status,
        SessionStatus::Next,
        "adjustment = {:?}",
        after.adjustment
    );
}

/// Aerobic decoupling (RUN_DECOUPLE_001) leaves lifts alone, and the day has
/// no run.
#[test]
fn decoupling_on_a_lift_only_day_does_not_mark_it_adjusted() {
    // 2024-10-07 08:00 UTC = local day 20003.
    assert_unchanged_and_not_adjusted(
        r#"{"SubmitReadiness":{"signal":"AerobicDecoupling","value":14.0,"observed_at":1728288000,"streak":1,"pain":null,"effort_min":40.0}}"#,
    );
}

/// A session-wide RPE load cut (AUTOREG_RIR_001) on a day with no prescribed
/// weight (calisthenics rungs carry none).
#[test]
fn load_cut_on_a_day_without_loads_does_not_mark_it_adjusted() {
    assert_unchanged_and_not_adjusted(
        r#"{"SubmitReadiness":{"signal":"Rpe","value":2.0,"observed_at":1728288000,"streak":1,"pain":null,"effort_min":null}}"#,
    );
}
