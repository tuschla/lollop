//! Readiness through the view: the morning check-in builds a baseline, each
//! signal row cites the rule that judged it (no composite score), and red
//! flags reach their tier.

use super::*;

fn checkin_ev(day: i64, sleep: u8, sore: u8, mood: u8) -> Event {
    Event::SubmitCheckin(CheckinInput {
        observed_at: day * DAY_SEC + 100,
        sleep_quality: Some(sleep),
        soreness: Some(sore),
        mood: Some(mood),
        fatigue: None,
        stress: None,
        resting_hr_bpm: None,
        hrv_rmssd_ms: None,
    })
}

#[test]
fn a_single_checkin_shows_building_baseline_not_a_fabricated_z() {
    // The acceptance path: a fresh user does one morning check-in (sleep 2
    // / soreness 4 / mood 3). Today must show an honest "collecting baseline"
    // state and NO derived readiness signal, never a made-up z-score, and
    // the user is never asked to enter one.
    let app = Engine;
    let mut model = Model::default();
    app.update(checkin_ev(0, 2, 4, 3), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert!(!vm.train_blocked);
    // The check-in is echoed so the shell can rehydrate + show it's recorded.
    let echo = vm.checkin_today.expect("checkin echoed");
    assert_eq!(echo.sleep_quality, Some(2));
    assert_eq!(echo.soreness, Some(4));
    // Honest collecting status, no derived WellnessZ row yet.
    let status = vm
        .baseline_status
        .iter()
        .find(|b| b.signal == ReadinessSignal::WellnessZ)
        .expect("wellness baseline status");
    assert_eq!(status.have, 1);
    assert_eq!(status.need, 7);
    assert_eq!(status.kind, crate::autoreg::BaselineKind::Collecting);
    assert!(
        !vm.readiness_summary
            .iter()
            .any(|r| r.signal == ReadinessSignal::WellnessZ),
        "no fabricated wellness z before the baseline exists"
    );
}

#[test]
fn a_floor_morning_before_the_baseline_is_very_low_not_below_normal() {
    // Day 1, a catastrophic check-in (sleep 1 / soreness 5 / mood 1): the
    // absolute floor fires the single-day hold, but with "building your
    // baseline, 1 of 7" beside it there is no normal to be below yet.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetToday {
            epoch_day: 0,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(checkin_ev(0, 1, 5, 1), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(
        vm.baseline_status
            .iter()
            .any(|b| b.signal == ReadinessSignal::WellnessZ && b.have == 1)
    );
    let w = vm
        .readiness_summary
        .iter()
        .find(|r| r.signal == ReadinessSignal::WellnessZ)
        .expect("the floor reading surfaces");
    assert_eq!(w.state, "very low today");
    let call = vm
        .adjustments
        .iter()
        .find(|a| a.summary.contains("check-in"))
        .expect("the floor still holds the session");
    assert!(call.summary.contains("very low"), "{}", call.summary);
    assert!(!call.summary.contains("your normal"), "{}", call.summary);
    // The reading stays in force into the next day, so the reason names no day.
    let reason = call.summary.split(':').next().unwrap_or_default();
    assert!(!reason.contains("today"), "{}", call.summary);
    assert!(
        !vm.today_headline.summary.contains("your normal"),
        "{}",
        vm.today_headline.summary
    );
}

#[test]
fn seven_checkins_build_a_baseline_and_surface_a_human_derived_state() {
    // Repeat the check-in across enough days (backdated) and the baseline
    // kicks in: the core derives a WellnessZ from the history and surfaces a
    // per-signal STATE in the summary, still no z-entry by the user.
    let app = Engine;
    let mut model = Model::default();
    // Six good, slightly varying baseline days, then a rough seventh.
    for d in 0..6 {
        let ev = if d % 2 == 0 {
            checkin_ev(d, 5, 2, 4)
        } else {
            checkin_ev(d, 4, 3, 4)
        };
        app.update(ev, &mut model).expect_only_render();
    }
    app.update(checkin_ev(6, 2, 4, 3), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    // Baseline is built → the collecting status is gone for wellness…
    assert!(
        !vm.baseline_status
            .iter()
            .any(|b| b.signal == ReadinessSignal::WellnessZ),
        "wellness baseline is ready, no longer collecting"
    );
    // …and a human-derived wellness state now appears, judged by the SAME
    // KB rule (WELLNESS-001), carrying its evidence: no bare recommendation.
    let w = vm
        .readiness_summary
        .iter()
        .find(|r| r.signal == ReadinessSignal::WellnessZ)
        .expect("derived wellness row");
    assert_eq!(
        w.state, "below normal",
        "rough morning vs baseline is below normal"
    );
    assert!(
        w.evidence.grade.is_some(),
        "derived state carries the rule's evidence"
    );
    // The derived signal drives today's call (a downgrade-class adjustment).
    assert_eq!(vm.today_headline.kind, HeadlineKind::Adjustment);
}

#[test]
fn checkins_and_old_readiness_logs_coexist() {
    // Backward compat: the retained check-in history and the legacy
    // day-scoped raw-signal path both feed the rules; ClearReadiness must
    // NOT wipe the multi-day check-in baseline.
    let app = Engine;
    let mut model = Model::default();
    for d in 0..7 {
        app.update(checkin_ev(d, 4, 3, 4), &mut model)
            .expect_only_render();
    }
    // A legacy raw advanced-mode signal on top.
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Soreness, 6.0)),
        &mut model,
    )
    .expect_only_render();
    // Clearing readiness drops the raw signal but keeps the check-in history.
    app.update(Event::ClearReadiness, &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert!(
        vm.checkin_today.is_some(),
        "ClearReadiness must not wipe the check-in history"
    );
    assert!(
        !vm.readiness_summary
            .iter()
            .any(|r| r.signal == ReadinessSignal::Soreness),
        "the manually-logged raw Soreness was cleared"
    );
    // ClearCheckins is the only thing that drops the baseline.
    app.update(Event::ClearCheckins, &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.checkin_today.is_none());
    assert!(vm.baseline_status.is_empty());
}

#[test]
fn medical_red_flags_surface_referral_tier_through_the_view() {
    // The Android shell can now submit the medical-referral signals
    // (Illness/RedS/CardiacRedFlag/BoneStress). Lock the full event→view
    // contract those buttons depend on: each must raise the top
    // `MedicalReferral` tier and block training, so the SafetyBanner shows a
    // DO-NOT-TRAIN state rather than silently swallowing a red flag.
    for signal in [
        ReadinessSignal::RedS,
        ReadinessSignal::CardiacRedFlag,
        ReadinessSignal::BoneStress,
    ] {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SubmitReadiness(input(signal, 1.0)), &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        assert_eq!(
            vm.safety_tier,
            Some(SafetyTier::MedicalReferral),
            "{signal:?} must raise the referral tier"
        );
        assert!(vm.train_blocked, "{signal:?} must block training");
    }
}

#[test]
fn illness_severity_bands_map_to_tier_through_the_view() {
    // The Android ReadinessEditor sends Illness as a severity value (0 none,
    // 1 above-neck, 2 below-neck/fever) via its IllnessLevel picker. Lock the
    // full event→view contract that picker depends on: above-neck raises the
    // Illness tier without blocking (a downgraded session), while below-neck/
    // fever must block training outright. A drift in the value bands would let
    // the shell submit a fever as a mere downgrade.
    let app = Engine;
    let mut above = Model::default();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Illness, 1.0)),
        &mut above,
    )
    .expect_only_render();
    let vm = app.view(&above);
    assert_eq!(vm.safety_tier, Some(SafetyTier::Illness));
    assert!(
        !vm.train_blocked,
        "above-neck illness downgrades but must not block training"
    );

    let mut fever = Model::default();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Illness, 2.0)),
        &mut fever,
    )
    .expect_only_render();
    let vm = app.view(&fever);
    assert_eq!(vm.safety_tier, Some(SafetyTier::Illness));
    assert!(
        vm.train_blocked,
        "below-neck/fever illness must block training"
    );
}

#[test]
fn resting_hr_plus_ten_blocks_training_through_the_view() {
    // ready-upd-002 (AUTOREG-RHR-NOISE-001): ONE +10 morning is likely noise →
    // an easier day plus the illness check, no block; stop level on 2
    // consecutive mornings forces the rest day, which trips train_blocked at
    // the Illness tier. A single +5 bpm reading stays a no-op.
    let app = Engine;
    let mut single = Model::default();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::RestingHr, 10.0)),
        &mut single,
    )
    .expect_only_render();
    let vm = app.view(&single);
    assert!(
        !vm.train_blocked,
        "one +10 morning must not force a rest day"
    );
    assert_eq!(vm.safety_tier, Some(SafetyTier::SingleDayMarker));
    assert!(vm.illness_check_prompt, "the neck-check is asked for");

    let mut blocked = Model::default();
    app.update(
        Event::SubmitReadiness(ReadinessInput {
            streak: 2,
            ..input(ReadinessSignal::RestingHr, 10.0)
        }),
        &mut blocked,
    )
    .expect_only_render();
    let vm = app.view(&blocked);
    assert_eq!(vm.safety_tier, Some(SafetyTier::Illness));
    assert!(
        vm.train_blocked,
        "RHR +10 on 2 mornings must block training"
    );

    let mut downgrade = Model::default();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::RestingHr, 5.0)),
        &mut downgrade,
    )
    .expect_only_render();
    let vm = app.view(&downgrade);
    assert!(
        !vm.train_blocked,
        "a single RHR +5 bpm day must not block training"
    );
}

#[test]
fn clean_inputs_leave_training_open() {
    let app = Engine;
    let mut model = Model::default();

    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Rpe, 0.0)),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.safety_tier, None);
    assert!(!vm.train_blocked);
    assert!(vm.adjustments.is_empty());
    assert_eq!(vm.input_count, 1);
}

#[test]
fn a_single_deep_low_wellness_day_downgrades_via_the_single_day_arm() {
    // autoreg-030 single-day arm: a one-off WellnessZ ≤ −1.5 eases the session
    // (no block, no safety tier); the multi-day arm still needs two consecutive
    // z ≤ −1 days.
    let app = Engine;
    let mut model = Model::default();

    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::WellnessZ, -1.5)),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    assert!(!vm.train_blocked);
    assert_eq!(vm.safety_tier, None);
    assert!(
        vm.adjustments.iter().any(|a| a
            .summary
            .starts_with("Your check-in is well below your normal")),
        "a single z ≤ −1.5 day holds the top end via the single-day arm: {:?}",
        vm.adjustments
            .iter()
            .map(|a| &a.summary)
            .collect::<Vec<_>>()
    );
}

#[test]
fn clear_resets_inputs() {
    let app = Engine;
    let mut model = Model::default();

    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)),
        &mut model,
    )
    .expect_only_render();
    app.update(Event::ClearReadiness, &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.input_count, 0);
    assert!(!vm.train_blocked);
}

#[test]
fn hrv_ending_yesterday_with_active_baseline_reports_rolling_gate() {
    // The derived HRV channel collapses a whole rolling baseline into
    // ONE input dated at the newest HRV day. With HRV ending yesterday and a
    // wellness-only check-in today, input-counting saw a single HRV input and
    // mislabelled the active baseline as "No usable HRV". The source must
    // report the rolling gate that actually drives the autoreg-028 downgrade.
    let app = Engine;
    let mut model = Model::default();

    // Days 0..=21: full check-ins carrying HRV → 22 distinct HRV reading-days
    // spanning 21 days back a mature, active rolling baseline.
    for day in 0..=21 {
        app.update(
            Event::SubmitCheckin(CheckinInput {
                observed_at: day * DAY_SEC + 100,
                sleep_quality: Some(3),
                soreness: Some(2),
                mood: Some(3),
                fatigue: None,
                stress: None,
                resting_hr_bpm: None,
                hrv_rmssd_ms: Some(50.0),
            }),
            &mut model,
        )
        .expect_only_render();
    }
    // Day 22: a wellness-only check-in, so the newest check-in day (22) is
    // later than the newest HRV day (21, "yesterday").
    app.update(
        Event::SubmitCheckin(CheckinInput {
            observed_at: 22 * DAY_SEC + 100,
            sleep_quality: Some(3),
            soreness: Some(2),
            mood: Some(3),
            fatigue: None,
            stress: None,
            resting_hr_bpm: None,
            hrv_rmssd_ms: None,
        }),
        &mut model,
    )
    .expect_only_render();

    let src = app.view(&model).autoreg_source.expect("source row");
    assert!(
        src.summary.contains("7-day HRV trend"),
        "active baseline with HRV ending yesterday must report the rolling gate: {}",
        src.summary
    );
    assert!(
        !src.summary.contains("No usable HRV"),
        "must not mislabel an active baseline as no usable HRV: {}",
        src.summary
    );
}

#[test]
fn readiness_summary_states_cite_the_judging_rule() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::HrvLnRmssd, -1.0)),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::RestingHr, 12.0)),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert_eq!(vm.readiness_summary.len(), 3);

    let hrv = vm
        .readiness_summary
        .iter()
        .find(|s| s.signal == ReadinessSignal::HrvLnRmssd)
        .unwrap();
    assert_eq!(hrv.state, "below normal");
    assert_eq!(hrv.group, SignalGroup::Metric);
    assert!(
        hrv.evidence.grade.is_some(),
        "state judged by a rule carries its evidence"
    );
    assert!(!hrv.evidence.citation.is_empty());

    let rhr = vm
        .readiness_summary
        .iter()
        .find(|s| s.signal == ReadinessSignal::RestingHr)
        .unwrap();
    // ready-upd-002: one +12 morning is an easier day + illness check, cited
    // to the safety-critical noise rule.
    assert!(rhr.state.contains("check for illness"), "{}", rhr.state);
    assert!(rhr.evidence.safety_critical);

    let pain = vm
        .readiness_summary
        .iter()
        .find(|s| s.signal == ReadinessSignal::Pain)
        .unwrap();
    assert_eq!(pain.group, SignalGroup::RedFlag);
    assert!(pain.state.contains("stop"), "{}", pain.state);
}

#[test]
fn readiness_summary_all_clear_rows_carry_no_fabricated_evidence() {
    let app = Engine;
    let mut model = Model::default();
    // A withdrawn/absent pain report ("clear") and a recorded-only signal
    // judge nothing: their evidence fields must stay empty rather than
    // borrowing a rule's citation they did not run through.
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Pain, 0.0)),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::BarVelocity, 0.5)),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let pain = vm
        .readiness_summary
        .iter()
        .find(|s| s.signal == ReadinessSignal::Pain)
        .unwrap();
    assert_eq!(pain.state, "clear");
    assert!(pain.evidence.grade.is_none());
    let bv = vm
        .readiness_summary
        .iter()
        .find(|s| s.signal == ReadinessSignal::BarVelocity)
        .unwrap();
    assert_eq!(bv.state, "recorded");
    assert!(bv.evidence.grade.is_none());
}

#[test]
fn signal_groups_metadata_fences_the_red_flag_block() {
    let app = Engine;
    let vm = app.view(&Model::default());
    assert_eq!(vm.signal_groups.len(), 15);
    for red in [
        ReadinessSignal::Pain,
        ReadinessSignal::Illness,
        ReadinessSignal::RedS,
        ReadinessSignal::CardiacRedFlag,
        ReadinessSignal::BoneStress,
    ] {
        assert_eq!(
            vm.signal_groups
                .iter()
                .find(|g| g.signal == red)
                .unwrap()
                .group,
            SignalGroup::RedFlag
        );
    }
    // Order contract: every metric precedes every red flag, so a shell can
    // divide exactly where the group changes.
    let first_red = vm
        .signal_groups
        .iter()
        .position(|g| g.group == SignalGroup::RedFlag)
        .unwrap();
    assert!(
        vm.signal_groups[..first_red]
            .iter()
            .all(|g| g.group == SignalGroup::Metric)
    );
    assert!(
        vm.signal_groups[first_red..]
            .iter()
            .all(|g| g.group == SignalGroup::RedFlag)
    );
}

#[test]
fn maxed_checkin_soreness_downgrades_and_reads_high_on_the_strip() {
    // A 5/5 check-in soreness rescales onto the KB's 7-point scale (→ 7.0),
    // clearing the downgrade gate and rendering a non-normal strip row.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitCheckin(CheckinInput {
            observed_at: 100,
            sleep_quality: None,
            soreness: Some(5),
            mood: None,
            fatigue: None,
            stress: None,
            resting_hr_bpm: None,
            hrv_rmssd_ms: None,
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let sore = vm
        .readiness_summary
        .iter()
        .find(|r| r.signal == ReadinessSignal::Soreness)
        .expect("a soreness strip row");
    assert_ne!(
        sore.state, "normal",
        "a 5/5 check-in soreness is not normal"
    );
    // Every lift-holding signal of the morning reads as one call naming each.
    assert!(
        vm.adjustments.iter().any(|a| a
            .summary
            .to_lowercase()
            .contains("you reported being very sore")),
        "a maxed check-in soreness holds the lifts' top end (§5: keep systemic intensity)"
    );
}

#[test]
fn a_soreness_answer_from_an_earlier_day_never_says_today() {
    // Sunday's maxed soreness is still in force on Tuesday (its 2-day window),
    // so the hold stays, but neither the call nor the strip row may claim the
    // soreness is today's.
    let sunday = 20_000i64;
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitCheckin(CheckinInput {
            observed_at: sunday * DAY_SEC + 8 * 3600,
            sleep_quality: None,
            soreness: Some(5),
            mood: None,
            fatigue: None,
            stress: None,
            resting_hr_bpm: None,
            hrv_rmssd_ms: None,
        }),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: sunday + 2,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let hold = vm
        .adjustments
        .iter()
        .find(|a| {
            a.summary
                .to_lowercase()
                .contains("reported being very sore")
        })
        .expect("the soreness hold is still in force two days on");
    assert!(
        !hold.summary.to_lowercase().contains("today"),
        "{}",
        hold.summary
    );
    let sore = vm
        .readiness_summary
        .iter()
        .find(|r| r.signal == ReadinessSignal::Soreness)
        .expect("a soreness strip row");
    assert!(!sore.from_today);
    assert!(!sore.state.contains("today"), "{}", sore.state);
}
