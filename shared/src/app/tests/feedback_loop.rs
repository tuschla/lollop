//! Coaching feedback loop: weekly check-in vs session review, derived session
//! feedback, personal bests, recovery week, copy with numbers, trend, "last
//! time".

use super::*;

/// A dated Monday (`mon0_weekday == 0`) so reviews take their dated paths.
const FB_MON: i64 = 20_353;

fn today(app: &Engine, model: &mut Model, epoch_day: i64) {
    app.update(
        Event::SetToday {
            epoch_day,
            utc_offset_sec: 0,
        },
        model,
    )
    .expect_only_render();
}

/// Every lift item of today's hero: (exercise, sets, reps, load).
fn hero_items(app: &Engine, model: &Model) -> Vec<(String, u8, u32, f64)> {
    app.view(model)
        .next_session
        .expect("a hero session")
        .items
        .iter()
        .filter(|i| !i.exercise.is_empty())
        .map(|i| {
            (
                i.exercise.clone(),
                i.sets,
                u32::from(i.reps_low.max(1)),
                i.load_kg.unwrap_or(100.0),
            )
        })
        .collect()
}

fn hero_squat_load(app: &Engine, model: &Model) -> f64 {
    app.view(model)
        .next_session
        .expect("hero")
        .items
        .iter()
        .find(|i| i.exercise == "Back Squat")
        .and_then(|i| i.load_kg)
        .expect("anchored squat load")
}

// ── A session review never wipes the weekly check-in ──

#[test]
fn weekly_bone_pain_referral_survives_a_later_session_review() {
    let app = Engine;
    let mut model = Model::default();
    today(&app, &mut model, FB_MON + 2);
    app.update(
        Event::SubmitWeeklyCheckin(SessionReview {
            bone_pain_red_flag: true,
            observed_at: FB_MON * DAY_SEC,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    // Tuesday's routine session review: reps met at the planned effort.
    app.update(
        Event::SubmitReview(SessionReview {
            lift: Some(LiftExec {
                reps_met: true,
                rir_actual: 2,
                rir_target: 2,
            }),
            observed_at: (FB_MON + 1) * DAY_SEC,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let fb = vm.feedback.expect("referral still present");
    assert_eq!(fb.category, FeedbackCategory::ConcernInjury);
    assert!(fb.evidence.safety_critical);
    assert!(fb.message.contains("doctor"), "{}", fb.message);
    // The weekly referral also defers training (safety-040): it owns the
    // headline as a safety hold, and the session review does not lift it.
    assert!(vm.train_blocked);
    assert_eq!(vm.safety_tier, Some(SafetyTier::MedicalReferral));
    assert_eq!(vm.today_headline.kind, HeadlineKind::SafetyHold);
    assert!(vm.today_headline.evidence.safety_critical);

    // The legacy single-wire path routes the same way: a SubmitReview that
    // carries the clinical flag lands in the weekly store.
    let mut legacy = Model::default();
    app.update(
        Event::SubmitReview(SessionReview {
            bone_pain_red_flag: true,
            ..SessionReview::default()
        }),
        &mut legacy,
    )
    .expect_only_render();
    app.update(Event::SubmitReview(SessionReview::default()), &mut legacy)
        .expect_only_render();
    assert_eq!(
        app.view(&legacy).feedback.expect("feedback").category,
        FeedbackCategory::ConcernInjury
    );
}

#[test]
fn weekly_deload_stays_in_todays_plan_after_a_session_review() {
    let (app, mut model) = planned_strength_model_on(FB_MON, true);
    let full = hero_squat_load(&app, &model);
    app.update(
        Event::SubmitWeeklyCheckin(SessionReview {
            failed_key_sessions: Some(2),
            rpe_load_gap_sessions: Some(2),
            observed_at: (FB_MON - 1) * DAY_SEC + 20 * 3_600,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let deloaded = hero_squat_load(&app, &model);
    assert!(deloaded < full, "{deloaded} < {full}");
    app.update(
        Event::SubmitReview(SessionReview {
            bad_day: true,
            observed_at: FB_MON * DAY_SEC + 600,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    assert_eq!(
        hero_squat_load(&app, &model),
        deloaded,
        "the session review leaves the weekly deload folded into today"
    );
}

#[test]
fn compulsive_flag_cleared_only_by_a_later_weekly_no() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitWeeklyCheckin(SessionReview {
            compulsive_flag: true,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    app.update(Event::SubmitReview(SessionReview::default()), &mut model)
        .expect_only_render();
    app.update(Event::ClearReview, &mut model)
        .expect_only_render();
    let fb = app.view(&model).feedback.expect("concern persists");
    assert_eq!(fb.category, FeedbackCategory::ConcernBehavior);
    // A later weekly check-in answering "no" lifts it.
    app.update(
        Event::SubmitWeeklyCheckin(SessionReview::default()),
        &mut model,
    )
    .expect_only_render();
    assert!(
        app.view(&model)
            .feedback
            .is_none_or(|f| f.category != FeedbackCategory::ConcernBehavior)
    );
}

// ── Concern copy routes to a professional, never labels ──

fn msg(cat: FeedbackCategory, recovery_week_active: bool) -> String {
    feedback_message(&MessageCtx {
        cat,
        trigger: None,
        lift: None,
        drift_pct: None,
        split_pct: None,
        easy_frac: None,
        spike: None,
        manual_spike: None,
        run: None,
        mastery: None,
        derived_lift_count: 0,
        signs: None,
        signal_count: 2,
        recovery_week_active,
        lighter_next: false,
        double_progression: false,
        plain: true,
        weight_lb: false,
        distance_mi: false,
        today: None,
    })
}

const ALL_CATEGORIES: [FeedbackCategory; 12] = [
    FeedbackCategory::ConcernInjury,
    FeedbackCategory::ConcernRecovery,
    FeedbackCategory::ConcernBehavior,
    FeedbackCategory::ConcernEnergy,
    FeedbackCategory::DangerousProgression,
    FeedbackCategory::IntensityDiscipline,
    FeedbackCategory::PositiveExecution,
    FeedbackCategory::InformationalNeutral,
    FeedbackCategory::CorrectiveProcess,
    FeedbackCategory::PositiveMastery,
    FeedbackCategory::ContextualBadDay,
    FeedbackCategory::ProgressionNudge,
];

#[test]
fn behavior_concern_copy_names_a_professional() {
    let m = msg(FeedbackCategory::ConcernBehavior, false);
    assert!(m.contains("doctor") || m.contains("psychologist"), "{m}");
    let e = msg(FeedbackCategory::ConcernEnergy, false);
    assert!(e.contains("doctor") || e.contains("dietitian"), "{e}");
}

#[test]
fn recovery_concern_copy_has_escalation_path() {
    for active in [true, false] {
        let m = msg(FeedbackCategory::ConcernRecovery, active);
        assert!(m.contains("doctor"), "{m}");
    }
    assert!(msg(FeedbackCategory::ConcernRecovery, true).contains("lighter"));
}

#[test]
fn feedback_copy_carries_no_label_guilt_or_jargon() {
    for cat in ALL_CATEGORIES {
        for active in [true, false] {
            let m = msg(cat, active).to_lowercase();
            for banned in [
                "compulsive",
                "guilt",
                " must ",
                "rir ~0",
                "autoregulat",
                "population default",
                "rpe-stop",
                "nothing to correct",
            ] {
                assert!(!m.contains(banned), "{cat:?} copy contains {banned:?}: {m}");
            }
        }
    }
}

// ── Overtraining → one card + a lighter plan ──

#[test]
fn overtraining_concern_folds_a_deload_into_the_next_session() {
    let (app, mut model) = planned_strength_model_on(FB_MON, true);
    let full = hero_squat_load(&app, &model);
    app.update(
        Event::SubmitWeeklyCheckin(SessionReview {
            overtraining_signs: Some(OvertrainingSigns {
                worse_performance: true,
                worse_sleep: true,
                ..OvertrainingSigns::default()
            }),
            overtraining_signal_weeks: Some(1.5),
            observed_at: FB_MON * DAY_SEC,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    let lighter = hero_squat_load(&app, &model);
    assert!(
        lighter < full,
        "recovery week lowers the load: {lighter} < {full}"
    );
    let fb = vm.feedback.expect("recovery concern");
    assert_eq!(fb.category, FeedbackCategory::ConcernRecovery);
    assert!(fb.message.contains("worse sleep"), "{}", fb.message);
    assert!(fb.message.contains("lighter"), "{}", fb.message);
    assert!(fb.message.contains("doctor"), "{}", fb.message);
}

#[test]
fn single_week_two_signals_under_one_week_does_not_fire_concern() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitWeeklyCheckin(SessionReview {
            overtraining_signal_count: 3,
            overtraining_signal_weeks: Some(0.5),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.feedback.is_none(), "{:?}", vm.feedback);
    assert!(vm.review_adjustments.is_empty());
    // The ticked signs define the count: one sign never fires, whatever the
    // bare count says.
    app.update(
        Event::SubmitWeeklyCheckin(SessionReview {
            overtraining_signal_count: 5,
            overtraining_signs: Some(OvertrainingSigns {
                low_mood: true,
                ..OvertrainingSigns::default()
            }),
            overtraining_signal_weeks: Some(2.0),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    assert!(app.view(&model).feedback.is_none());
}

#[test]
fn concern_recovery_suppresses_duplicate_down_week_rows() {
    // A hybrid athlete (running + lifting): the running-034 and hybrid-026
    // text rows must not restate the same call three times.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(Profile {
            running_days_per_week: 3,
            ..sample_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitWeeklyCheckin(SessionReview {
            overtraining_signal_count: 2,
            overtraining_signal_weeks: Some(1.0),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert_eq!(
        vm.feedback.expect("concern").category,
        FeedbackCategory::ConcernRecovery
    );
    let restating: Vec<_> = vm
        .review_adjustments
        .iter()
        .filter(|a| {
            a.summary.contains("down week")
                || a.summary.contains("Combined-training")
                || a.summary.contains("Lighter week")
        })
        .collect();
    assert_eq!(restating.len(), 1, "{:#?}", vm.review_adjustments);
}

#[test]
fn running_down_week_row_absent_for_a_pure_lifter() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    app.update(
        Event::SubmitWeeklyCheckin(SessionReview {
            overtraining_signal_count: 2,
            overtraining_signal_weeks: Some(1.0),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let vm = app.view(&model);
    assert!(
        !vm.review_adjustments
            .iter()
            .any(|a| a.summary.to_lowercase().contains("run")),
        "{:#?}",
        vm.review_adjustments
    );
}

// ── Feedback derived from the logs, no form ──

#[test]
fn completed_lift_day_emits_feedback_without_a_review() {
    let (app, mut model) = planned_strength_model_on(FB_MON, true);
    assert!(app.view(&model).feedback.is_none());
    for (ex, sets, reps, kg) in hero_items(&app, &model) {
        for _ in 0..sets {
            app.update(
                log_set(&ex, kg, reps, 8.0, FB_MON * DAY_SEC + 3_600),
                &mut model,
            )
            .expect_only_render();
        }
    }
    let vm = app.view(&model);
    assert!(!vm.review_present, "no SubmitReview was sent");
    let done = vm.session_done.expect("today's session summarized");
    assert_eq!(done.epoch_day, FB_MON);
    assert!(done.items.iter().all(|i| i.met), "{:#?}", done.items);
    let fb = vm.feedback.expect("feedback derived from the logged sets");
    assert!(!fb.evidence.safety_critical);
    assert!(
        done.items.iter().any(|i| fb.message.contains(&i.exercise)),
        "the takeaway names a lift with its numbers: {}",
        fb.message
    );
}

#[test]
fn missed_reps_on_logged_sets_yield_corrective_feedback() {
    let (app, mut model) = planned_strength_model_on(FB_MON, true);
    let items = hero_items(&app, &model);
    let (first, first_sets, first_reps, first_kg) = items[0].clone();
    assert!(first_sets > 1, "need a multi-set first item");
    let at = FB_MON * DAY_SEC + 3_600;
    // One qualifying set, then the user ends the exercise early.
    app.update(log_set(&first, first_kg, first_reps, 9.0, at), &mut model)
        .expect_only_render();
    app.update(
        Event::SetExerciseEnded {
            exercise: first.clone(),
            epoch_day: FB_MON,
            ended: true,
            observed_at: at,
        },
        &mut model,
    )
    .expect_only_render();
    for (ex, sets, reps, kg) in items.iter().skip(1) {
        for _ in 0..*sets {
            app.update(log_set(ex, *kg, *reps, 8.0, at), &mut model)
                .expect_only_render();
        }
    }
    let fb = app.view(&model).feedback.expect("derived feedback");
    assert_eq!(
        fb.category,
        FeedbackCategory::CorrectiveProcess,
        "{}",
        fb.message
    );
    assert!(
        fb.message.contains(&format!("Came up short on {first}")),
        "{}",
        fb.message
    );
    assert!(
        fb.message.contains(&format!("1 of {first_sets} sets")),
        "{}",
        fb.message
    );

    // An off day flagged for the same session turns it into the
    // off-day note, never a correction.
    app.update(
        Event::SubmitReview(SessionReview {
            bad_day: true,
            observed_at: FB_MON * DAY_SEC + 7_200,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback");
    assert_eq!(fb.category, FeedbackCategory::ContextualBadDay);
    assert!(!fb.message.contains("guilt"));
}

#[test]
fn derived_rir_target_comes_from_the_prescription() {
    let item = |intensity: PlanIntensity, reps: u8| PrescriptionView {
        exercise: "Back Squat".into(),
        reps_low: reps,
        reps_high: reps,
        intensity,
        ..PrescriptionView::default()
    };
    assert_eq!(target_rir(&item(PlanIntensity::Rir { rir: 3 }, 8)), Some(3));
    assert_eq!(
        target_rir(&item(PlanIntensity::RirRange { lo: 1, hi: 2 }, 10)),
        Some(1)
    );
    // 85 % × 3: ~5 reps possible → 2 in reserve (Epley inverse).
    assert_eq!(
        target_rir(&item(
            PlanIntensity::PercentE1rm {
                pct: 85,
                reduced_from: None
            },
            3
        )),
        Some(2)
    );
    // 70 % × 5 leaves ~8 in reserve: too far from failure to judge.
    assert_eq!(
        target_rir(&item(
            PlanIntensity::PercentE1rm {
                pct: 70,
                reduced_from: None
            },
            5
        )),
        None
    );
    // spot-today #2: a cut %e1RM item keeps the effort its face shows (~1 rep
    // left), never the Epley reserve of its reduced percentage (~3).
    let cut = PrescriptionView {
        target_rpe: Some(9.0),
        ..item(
            PlanIntensity::PercentE1rm {
                pct: 78,
                reduced_from: Some(85),
            },
            5,
        )
    };
    assert_eq!(target_rir(&cut), Some(1));
}

#[test]
fn run_spike_feedback_fires_without_a_review() {
    let app = Engine;
    let mut model = Model::default();
    today(&app, &mut model, 100);
    app.update(log_run(10.0, 60.0, 70.0, 0.0, 40 * DAY_SEC, 1), &mut model)
        .expect_only_render();
    app.update(log_run(10.0, 60.0, 70.0, 0.0, 95 * DAY_SEC, 2), &mut model)
        .expect_only_render();
    app.update(log_run(12.0, 72.0, 70.0, 0.0, 100 * DAY_SEC, 3), &mut model)
        .expect_only_render();
    let fb = app
        .view(&model)
        .feedback
        .expect("spike warning without a review");
    assert_eq!(fb.category, FeedbackCategory::DangerousProgression);
    // The warning states a safe next distance.
    assert!(fb.message.contains("11.0 km"), "{}", fb.message);
}

// ── Personal bests with their numbers ──

#[test]
fn e1rm_pr_beyond_5pct_emits_anchored_mastery() {
    let app = Engine;
    let mut model = Model::default();
    today(&app, &mut model, 100);
    app.update(
        log_set("Back Squat", 100.0, 5, 8.0, 90 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set("Back Squat", 110.0, 5, 8.0, 100 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    let fb = app.view(&model).feedback.expect("mastery feedback");
    assert_eq!(fb.category, FeedbackCategory::PositiveMastery);
    assert!(fb.anchor_mastery);
    assert!(fb.message.contains("New best"), "{}", fb.message);
    assert!(fb.message.contains("110 kg × 5"), "{}", fb.message);
    assert_eq!(
        fb.evidence.citation,
        crate::evidence::graded((), claims::FB_MASTERY_ANCHOR_001).reference()
    );
}

#[test]
fn pr_within_noise_band_is_not_a_pr() {
    let app = Engine;
    let mut model = Model::default();
    today(&app, &mut model, 100);
    app.update(
        log_set("Back Squat", 100.0, 5, 8.0, 90 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set("Back Squat", 103.0, 5, 8.0, 100 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    assert!(
        app.view(&model)
            .feedback
            .is_none_or(|f| !f.message.contains("New best")),
        "a +3% estimate sits inside the ±5% noise band"
    );
}

#[test]
fn spike_run_is_never_praised_as_longest() {
    let app = Engine;
    let mut model = Model::default();
    today(&app, &mut model, 100);
    app.update(log_run(8.0, 48.0, 70.0, 0.0, 30 * DAY_SEC, 1), &mut model)
        .expect_only_render();
    app.update(log_run(10.0, 60.0, 70.0, 0.0, 95 * DAY_SEC, 2), &mut model)
        .expect_only_render();
    app.update(log_run(12.0, 72.0, 70.0, 0.0, 100 * DAY_SEC, 3), &mut model)
        .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback");
    assert!(!fb.message.contains("Longest run"), "{}", fb.message);
    assert_eq!(fb.category, FeedbackCategory::DangerousProgression);

    // Within the 10 % rule, the longest run in 90 days is praised.
    let mut ok = Model::default();
    today(&app, &mut ok, 100);
    app.update(log_run(11.5, 69.0, 70.0, 0.0, 60 * DAY_SEC, 1), &mut ok)
        .expect_only_render();
    app.update(log_run(11.0, 66.0, 70.0, 0.0, 95 * DAY_SEC, 2), &mut ok)
        .expect_only_render();
    app.update(log_run(12.0, 72.0, 70.0, 0.0, 100 * DAY_SEC, 3), &mut ok)
        .expect_only_render();
    let fb = app.view(&ok).feedback.expect("praise");
    assert_eq!(fb.category, FeedbackCategory::PositiveMastery);
    assert!(
        fb.message.contains("Longest run in 90 days: 12.0 km"),
        "{}",
        fb.message
    );
}

#[test]
fn beginner_mastery_copy_has_no_rir_or_e1rm_terms() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    today(&app, &mut model, 100);
    app.update(
        log_set("Back Squat", 100.0, 5, 8.0, 90 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set("Back Squat", 110.0, 5, 8.0, 100 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    let fb = app.view(&model).feedback.expect("mastery");
    assert!(fb.minimize_jargon);
    assert!(
        !fb.message.contains("RIR") && !fb.message.contains("e1RM"),
        "{}",
        fb.message
    );
}

// ── Copy with its numbers, the right branch ──

#[test]
fn split_fade_copy_mentions_split_not_easy_days() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReview(SessionReview {
            positive_split_pct: Some(6.0),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback");
    assert_eq!(fb.category, FeedbackCategory::IntensityDiscipline);
    assert!(fb.message.contains("6%"), "{}", fb.message);
    assert!(fb.message.contains("second half"), "{}", fb.message);
    assert!(!fb.message.contains("Easy days"), "{}", fb.message);
}

#[test]
fn off_day_with_fade_is_bad_day() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReview(SessionReview {
            positive_split_pct: Some(6.0),
            bad_day: true,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    assert_eq!(
        app.view(&model).feedback.expect("feedback").category,
        FeedbackCategory::ContextualBadDay
    );
}

#[test]
fn off_day_with_missed_reps_is_bad_day_not_corrective() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReview(SessionReview {
            lift: Some(LiftExec {
                reps_met: false,
                rir_actual: 0,
                rir_target: 2,
            }),
            bad_day: true,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    assert_eq!(
        app.view(&model).feedback.expect("feedback").category,
        FeedbackCategory::ContextualBadDay
    );
}

#[test]
fn durability_copy_includes_drift_pct() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReview(SessionReview {
            decoupling: Some(Decouple {
                drift_pct: 3.2,
                cool_steady_context: true,
            }),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback");
    assert_eq!(fb.category, FeedbackCategory::PositiveExecution);
    assert!(fb.message.contains("3.2%"), "{}", fb.message);
}

#[test]
fn harder_than_planned_lift_gets_its_own_copy() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReview(SessionReview {
            lift: Some(LiftExec {
                reps_met: true,
                rir_actual: 1,
                rir_target: 3,
            }),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback");
    assert_eq!(fb.category, FeedbackCategory::InformationalNeutral);
    assert!(
        fb.message.contains("felt harder than planned"),
        "{}",
        fb.message
    );
    assert!(
        fb.message.contains("(1 rep left, target 3 reps left)"),
        "{}",
        fb.message
    );
    assert!(!fb.message.contains("in the tank"), "{}", fb.message);
}

// ── Trend from logged data, actionable ──

#[test]
fn four_flat_weeks_of_e1rm_emit_plateau_with_named_lever() {
    // feedback-028: one lever, from the KB rule for the goal. No hypertrophy
    // goal → the strength-stall lever (STR-STALL-001), never "add sets".
    let app = Engine;
    let mut model = Model::default();
    today(&app, &mut model, 200);
    for day in [130, 150, 190] {
        app.update(
            log_set("Back Squat", 100.0, 5, 8.0, day * DAY_SEC),
            &mut model,
        )
        .expect_only_render();
    }
    let trend = app.view(&model).trend.expect("plateau row");
    assert!(trend.summary.contains("held steady"), "{}", trend.summary);
    assert!(trend.summary.contains("lighter week"), "{}", trend.summary);
    assert!(!trend.summary.contains("weekly sets"), "{}", trend.summary);
    assert!(
        trend
            .evidence
            .citation
            .contains(claims::STR_STALL_001.entry().reference())
    );

    // Hypertrophy goal → the +2 sets lever (hypertrophy-008, HYP-MESO-ADD-001).
    app.update(
        Event::SetProfile(Profile {
            lift_goal: LiftGoal::Hypertrophy,
            ..strength_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    let trend = app.view(&model).trend.expect("plateau row");
    assert!(
        trend.summary.contains("add 2 sets a week"),
        "{}",
        trend.summary
    );
}

#[test]
fn rising_e1rm_emits_improving_trend() {
    let app = Engine;
    let mut model = Model::default();
    today(&app, &mut model, 200);
    app.update(
        log_set("Back Squat", 100.0, 5, 8.0, 150 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set("Back Squat", 110.0, 5, 8.0, 190 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    let trend = app.view(&model).trend.expect("improving row");
    assert!(trend.summary.contains("up 10%"), "{}", trend.summary);
}

#[test]
fn decline_with_low_wellness_emits_recovery_first() {
    let app = Engine;
    let mut model = Model::default();
    today(&app, &mut model, 200);
    app.update(
        log_set("Back Squat", 110.0, 5, 8.0, 150 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set("Back Squat", 100.0, 5, 8.0, 190 * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitWeeklyCheckin(SessionReview {
            low_recovery: true,
            observed_at: 195 * DAY_SEC,
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let trend = app.view(&model).trend.expect("decline row");
    assert!(
        trend
            .summary
            .contains("A lighter week is worth trying first"),
        "{}",
        trend.summary
    );
}

// ── What changed since last time ──

#[test]
fn anchored_lift_basis_names_last_set_and_delta() {
    let (app, model) = planned_strength_model_on(FB_MON, true);
    let squat = app
        .view(&model)
        .next_session
        .expect("hero")
        .items
        .into_iter()
        .find(|i| i.exercise == "Back Squat")
        .expect("squat");
    assert!(
        squat
            .why
            .basis
            .contains("Last time: 120 kg × 3 with 2 reps left."),
        "{}",
        squat.why.basis
    );
    if squat.load_kg.is_some_and(|l| (l - 120.0).abs() >= 0.5) {
        assert!(squat.why.basis.contains("Today is "), "{}", squat.why.basis);
    }
}

// ── Compaction: weekly events replay identically ──

#[test]
fn weekly_events_round_trip_the_wire() {
    let ev: Event = serde_json::from_str(
        r#"{"SubmitWeeklyCheckin":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"overtraining_signs":{"worse_sleep":true,"low_mood":true},"overtraining_signal_weeks":1.0,"bad_day":false}}"#,
    )
    .expect("weekly check-in wire parses");
    let app = Engine;
    let mut model = Model::default();
    app.update(ev, &mut model).expect_only_render();
    let clear: Event = serde_json::from_str(r#""ClearWeeklyCheckin""#).expect("unit variant");
    let vm = app.view(&model);
    assert!(vm.weekly_checkin_present);
    assert_eq!(
        vm.feedback.expect("concern").category,
        FeedbackCategory::ConcernRecovery
    );
    app.update(clear, &mut model).expect_only_render();
    assert!(!app.view(&model).weekly_checkin_present);
}

// ── Protective adjustments silence praise and load-increase cues ──

#[test]
fn protective_adjustment_turns_a_progression_nudge_into_hold_the_plan() {
    // autoreg-044 / feedback-042: a set that felt easy normally nudges "add
    // load"; once today's plan is cut (first-set RPE well above target), the
    // card says to keep to the eased plan instead, ungraded, with no number.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReview(SessionReview {
            lift: Some(LiftExec {
                reps_met: true,
                rir_actual: 4,
                rir_target: 2,
            }),
            ..SessionReview::default()
        }),
        &mut model,
    )
    .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback");
    assert_eq!(fb.category, FeedbackCategory::ProgressionNudge);
    assert!(
        !fb.message.contains('%'),
        "no unsourced magnitude: {}",
        fb.message
    );

    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Rpe, 2.0)),
        &mut model,
    )
    .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback");
    assert_eq!(fb.category, FeedbackCategory::InformationalNeutral);
    assert!(fb.message.contains("keep to it"), "{}", fb.message);
    assert!(!fb.message.contains("add load"), "{}", fb.message);
    assert!(fb.evidence.grade.is_none() && fb.evidence.citation.is_empty());
}

#[test]
fn nothing_to_adjust_note_carries_no_grade() {
    // A note stating there is nothing to act on is not a graded claim.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SubmitReview(SessionReview::default()), &mut model)
        .expect_only_render();
    let fb = app.view(&model).feedback.expect("feedback");
    assert_eq!(fb.category, FeedbackCategory::InformationalNeutral);
    assert!(fb.evidence.grade.is_none(), "{:?}", fb.evidence.grade);
}

#[test]
fn a_skipped_exercise_is_not_judged_as_coming_up_short() {
    let lift = |name: &str, status: ItemStatus, done: u8| PrescriptionView {
        exercise: name.into(),
        sets: 6,
        sets_done: done,
        reps_low: 5,
        status,
        ..PrescriptionView::default()
    };
    let week = vec![SessionPlanView {
        epoch_day: 10,
        status: SessionStatus::Done,
        items: vec![
            lift("Bench press", ItemStatus::Skipped, 0),
            lift("Leg press", ItemStatus::SkippedPain, 0),
            lift("Back Squat", ItemStatus::Done, 6),
        ],
        ..SessionPlanView::default()
    }];
    let exec = derive_session_exec(&week, 10).expect("a done day");
    let names: Vec<_> = exec.lifts.iter().map(|l| l.exercise.as_str()).collect();
    assert_eq!(names, ["Back Squat"]);
    assert!(exec.lifts.iter().all(|l| l.reps_met));
}
