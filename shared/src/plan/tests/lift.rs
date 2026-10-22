//! Lift-day selection, DUP emphasis and lift prescription tests.

use super::*;
use crate::plan::lift::{lift_prescription, sized_sets};
use crate::schema::LiftSessionType;

#[test]
fn anchored_lift_is_percent_based_unanchored_is_rir() {
    let p = base_profile();
    // Anchored: the logged lift is prescribed by %1RM.
    let prog = synthesize(&p, &anchored("Back Squat", 100.0), 0).unwrap();
    let anchored_pct = prog.mesocycle.sessions.iter().any(|s| {
        s.prescriptions.iter().any(|rx| {
            matches!(
                &rx.value,
                Prescription::Lift(l)
                    if l.exercise.eq_ignore_ascii_case("Back Squat")
                        && matches!(l.intensity, LiftIntensity::PercentOneRm(_))
            )
        })
    });
    assert!(anchored_pct, "anchored lift should be %1RM");

    // No anchor at all → every lift is RIR-based (no invented load).
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    let any_pct = prog.mesocycle.sessions.iter().any(|s| {
        s.prescriptions.iter().any(|rx| {
            matches!(
                &rx.value,
                Prescription::Lift(l) if matches!(l.intensity, LiftIntensity::PercentOneRm(_))
            )
        })
    });
    assert!(!any_pct, "with no anchor no lift may carry a %1RM load");
}

#[test]
fn unanchored_lift_uses_the_conservative_rir_end() {
    // MaxStrength RIR band is (1, 3); the conservative (safer) end is 3, not
    // the aggressive 1.
    let p = base_profile();
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    let mut saw = false;
    for s in &prog.mesocycle.sessions {
        for rx in &s.prescriptions {
            if let Prescription::Lift(l) = &rx.value
                && let LiftIntensity::Rir(n) = l.intensity
            {
                saw = true;
                assert_eq!(n, 3, "RIR must be the conservative high end");
            }
        }
    }
    assert!(saw, "an unanchored strength plan should prescribe by RIR");
}

#[test]
fn unanchored_power_lifts_get_actionable_prescriptions() {
    // Power carries no KB numeric RIR and an unanchored lift has
    // no e1RM to take a percentage of. On a POWER day it is a calibration set
    // the athlete can execute (one set of 5 at RPE 8, STR-E1RM-CHECK-001) —
    // never "50 % of an unknown e1RM". On a strength-base day it is the
    // max-strength band by RIR (STR-BASE-POWER-001).
    let block = BlockCtx {
        n_lift_days: 3,
        week_in_block: 1,
        block_weeks: 4,
        slot: 0,
        week_regions: WeekRegions::full(3),
    };
    let power = lift_prescription(
        LiftGoal::Power,
        TrainingAge::Novice,
        DupDay::Power,
        "Back Squat",
        &Anchors::default(),
        0,
        block,
    );
    let Prescription::Lift(l) = &power.value else {
        panic!("expected a lift prescription")
    };
    assert!(
        matches!(l.intensity, LiftIntensity::Rpe(r) if r == 8.0),
        "unanchored power-day lift is a calibration set at RPE 8: {:?}",
        l.intensity
    );
    assert_eq!((l.sets, l.reps), (1, 5));
    assert_eq!(power.claim_id(), claims::STR_E1RM_CHECK_001);
    let heavy = lift_prescription(
        LiftGoal::Power,
        TrainingAge::Novice,
        DupDay::Heavy,
        "Back Squat",
        &Anchors::default(),
        0,
        block,
    );
    let Prescription::Lift(l) = &heavy.value else {
        panic!("expected a lift prescription")
    };
    assert!(
        matches!(l.intensity, LiftIntensity::Rir(3)),
        "{:?}",
        l.intensity
    );
    assert_eq!(heavy.claim_id(), claims::STR_BASE_POWER_001);
}

#[test]
fn post_return_anchor_loads_normally_during_a_novice_reentry_window() {
    // `build_plan_anchors` drops PRE-layoff anchors while a >8 wk re-entry
    // window runs (KB Table 3.4b: treat as novice), so any anchor that
    // reaches `lift_prescription` was logged after the return and loads by
    // %1RM like any other (the novice flag is not a standing cap).
    let anchors = Anchors {
        lift_e1rm: vec![("Back Squat".into(), 100.0)],
        reentry_novice: true,
        ..Anchors::default()
    };
    let rx = lift_prescription(
        LiftGoal::MaxStrength,
        TrainingAge::Intermediate,
        DupDay::Heavy,
        "Back Squat",
        &anchors,
        12,
        BlockCtx {
            n_lift_days: 3,
            week_in_block: 1,
            block_weeks: 4,
            slot: 0,
            week_regions: WeekRegions::full(3),
        },
    );
    let Prescription::Lift(l) = &rx.value else {
        panic!("expected a lift prescription")
    };
    assert!(
        matches!(l.intensity, LiftIntensity::PercentOneRm(_)),
        "{:?}",
        l.intensity
    );
}

fn lift_prescriptions(prog: &Program) -> Vec<(LiftSessionType, LiftPrescription)> {
    let mut out = Vec::new();
    for s in &prog.mesocycle.sessions {
        if let SessionType::Lift(lt) = s.session_type {
            for rx in &s.prescriptions {
                if let Prescription::Lift(l) = &rx.value {
                    out.push((lt, l.clone()));
                }
            }
        }
    }
    out
}

// ── DUP must respect lift_goal + rest must match the day prescribed ──

#[test]
fn hypertrophy_goal_hybrid_gets_a_hypertrophy_range_day() {
    // Hybrid = 2 lift days. A hypertrophy-goal DUP lifter must get at least one
    // 6–12-rep day (a [Heavy, Power] cycle would give zero).
    let mut p = runner(GoalDistance::HalfMarathon, 3, 30.0);
    p.weekly_sets = 12;
    p.lift_goal = LiftGoal::Hypertrophy;
    let prog = synthesize(&p, &anchored("Back Squat", 120.0), 0).unwrap();
    let has_hyp_range = lift_prescriptions(&prog)
        .iter()
        .any(|(_, l)| l.reps >= 6 && l.reps <= 12);
    assert!(
        has_hyp_range,
        "a hypertrophy-goal lifter must get at least one 6–12-rep day"
    );
}

#[test]
fn dup_rest_comes_from_the_day_prescribed_not_the_goal_band() {
    // Hypertrophy-goal lifter (now its own week, no DUP heavy/power day): a
    // heavy compound rests per its EXERCISE CLASS (HYP-REST-002: 120 s), never
    // the 30 s floor of the hypertrophy loading band, and isolation work rests
    // at least 90 s. The anchor uses the catalog's own name so the class
    // resolves from Table 4.
    let mut p = runner(GoalDistance::HalfMarathon, 3, 30.0);
    p.weekly_sets = 12;
    p.lift_goal = LiftGoal::Hypertrophy;
    let prog = synthesize(&p, &anchored("Back squat", 120.0), 0).unwrap();
    let lifts = lift_prescriptions(&prog);
    assert!(
        lifts
            .iter()
            .all(|(lt, _)| *lt == LiftSessionType::Repetition),
        "a hypertrophy week has no heavy/power DUP day: {lifts:?}"
    );
    let squat = lifts
        .iter()
        .find(|(_, l)| l.exercise.eq_ignore_ascii_case("Back squat"))
        .expect("the logged squat is scheduled");
    assert_eq!(
        squat.1.rest_sec, 120,
        "a heavy compound on a hypertrophy day rests 120 s, not 30 s"
    );
    for (_, l) in &lifts {
        assert!(
            l.rest_sec >= 90,
            "{} rests {} s (< 90)",
            l.exercise,
            l.rest_sec
        );
    }
    // An isolation lift on a strength DUP hypertrophy-emphasis day also takes
    // the class window (HYP-REST-002: ~90 s, not 60 s).
    let raise = lift_prescription(
        LiftGoal::Hypertrophy,
        TrainingAge::Intermediate,
        DupDay::Hypertrophy,
        "Lateral raise",
        &anchored("Lateral raise", 12.0),
        12,
        BlockCtx {
            n_lift_days: 3,
            week_in_block: 1,
            block_weeks: 4,
            slot: 0,
            week_regions: WeekRegions::full(3),
        },
    );
    match &raise.value {
        Prescription::Lift(l) => assert_eq!(
            l.rest_sec, 90,
            "an isolation lift rests 90 s on a hypertrophy day"
        ),
        _ => panic!("expected a lift prescription"),
    }
}

#[test]
fn goal_weighted_dup_orders() {
    // A max-strength goal pairs Heavy with a second ≥80 % day
    // (STR-HEAVYFREQ-001), so even 2 lift days give two heavy exposures.
    assert_eq!(dup_emphasis(LiftGoal::MaxStrength, false, 0), DupDay::Heavy);
    assert_eq!(
        dup_emphasis(LiftGoal::MaxStrength, false, 1),
        DupDay::HeavyVolume
    );
    assert_eq!(
        dup_emphasis(LiftGoal::MaxStrength, false, 2),
        DupDay::Hypertrophy
    );
    // A weak/novice power athlete gets two heavy days + one power.
    assert_eq!(dup_emphasis(LiftGoal::Power, true, 0), DupDay::Heavy);
    assert_eq!(dup_emphasis(LiftGoal::Power, true, 1), DupDay::Power);
    assert_eq!(dup_emphasis(LiftGoal::Power, true, 2), DupDay::HeavyVolume);
    // A strong power athlete keeps the classic order.
    assert_eq!(dup_emphasis(LiftGoal::Power, false, 2), DupDay::Hypertrophy);
    // Hypertrophy leads with a hypertrophy day.
    assert_eq!(
        dup_emphasis(LiftGoal::Hypertrophy, false, 0),
        DupDay::Hypertrophy
    );
}

fn lift_type_on_day(prog: &Program, day: u16) -> Option<LiftSessionType> {
    prog.mesocycle
        .sessions
        .iter()
        .find_map(|s| match s.session_type {
            SessionType::Lift(lt) if s.day == day => Some(lt),
            _ => None,
        })
}

fn has_sunday_long_run(prog: &Program) -> bool {
    prog.mesocycle
        .sessions
        .iter()
        .any(|s| s.session_type == SessionType::Run(RunSessionType::LongRun) && s.day == 6)
}

#[test]
fn sunday_long_run_hypertrophy_goal_has_no_heavy_or_power_day() {
    // Hybrid lift days with a Sunday long run: a hypertrophy goal runs its own
    // week (HYP-PERIOD-001), so every lift day is a muscle-building day and no
    // heavy-triple or speed day exists to place.
    let mut p = runner(GoalDistance::HalfMarathon, 3, 30.0);
    p.weekly_sets = 12;
    p.lift_goal = LiftGoal::Hypertrophy;
    let prog = synthesize(&p, &anchored("Back Squat", 120.0), 0).unwrap();
    assert!(has_sunday_long_run(&prog), "test needs a Sunday long run");
    let lift_days: Vec<LiftSessionType> =
        (0..7).filter_map(|d| lift_type_on_day(&prog, d)).collect();
    assert!(!lift_days.is_empty(), "a hybrid hypertrophy week lifts");
    assert!(
        lift_days
            .iter()
            .all(|lt| *lt == LiftSessionType::Repetition),
        "{lift_days:?}"
    );
}

#[test]
fn sunday_long_run_max_strength_goal_has_two_heavy_days() {
    // A max-strength hybrid's two lift days are both ≥80 % days (Heavy +
    // HeavyVolume, STR-HEAVYFREQ-001) with Heavy on Thursday, the day with
    // the most hours after the Sunday long run.
    let mut p = runner(GoalDistance::HalfMarathon, 3, 30.0);
    p.weekly_sets = 12;
    p.lift_goal = LiftGoal::MaxStrength;
    let prog = synthesize(&p, &anchored("Back Squat", 120.0), 0).unwrap();
    assert!(has_sunday_long_run(&prog), "test needs a Sunday long run");
    let lift_days: Vec<LiftSessionType> =
        (0..7).filter_map(|d| lift_type_on_day(&prog, d)).collect();
    assert_eq!(lift_days.len(), 2, "{lift_days:?}");
    assert!(
        lift_days.iter().all(|lt| *lt == LiftSessionType::MaxEffort),
        "both hybrid lift days are heavy days: {lift_days:?}"
    );
}

#[test]
fn lateral_raise_on_heavy_day_is_prescribed_12_to_25_reps() {
    // An isolation lift on a DUP Heavy day is not undulated at %1RM off an
    // unreliable e1RM: it takes the 12–25-rep isolation window by RIR.
    let pres = lift_prescription(
        LiftGoal::MaxStrength,
        TrainingAge::Intermediate,
        DupDay::Heavy,
        "Lateral raise",
        &anchored("Lateral raise", 12.0),
        12,
        BlockCtx {
            n_lift_days: 3,
            week_in_block: 1,
            block_weeks: 4,
            slot: 0,
            week_regions: WeekRegions::full(3),
        },
    );
    let Prescription::Lift(l) = &pres.value else {
        panic!("expected a lift prescription")
    };
    assert!(
        matches!(l.intensity, LiftIntensity::Rir(_)),
        "isolation is prescribed by RIR, not %1RM: {:?}",
        l.intensity
    );
    assert!(
        (12..=25).contains(&l.reps),
        "reps {} must land in the 12–25 isolation window",
        l.reps
    );
}

#[test]
fn narrow_grip_bench_is_not_a_pull() {
    assert_ne!(
        movement_pattern("Narrow grip bench"),
        Some(MovementPattern::Pull)
    );
}

#[test]
fn medicine_ball_throw_is_not_a_pull() {
    assert_ne!(
        movement_pattern("Medicine ball throw"),
        Some(MovementPattern::Pull)
    );
}

#[test]
fn seated_cable_row_is_a_pull() {
    assert_eq!(
        movement_pattern("Seated cable row"),
        Some(MovementPattern::Pull)
    );
}

#[test]
fn plural_exercise_names_keep_their_movement_pattern() {
    assert_eq!(
        movement_pattern("Back squats"),
        Some(MovementPattern::Squat)
    );
    assert_eq!(
        movement_pattern("Leg presses"),
        Some(MovementPattern::Squat)
    );
    assert_eq!(movement_pattern("Pull ups"), Some(MovementPattern::Pull));
}

// ── weekly_sets → per-session sets (volume dose-response + expert-opinion split) ──

/// (exercise, sets, claim_id) for every LIFT prescription in a program.
fn lift_sets_and_claims(prog: &Program) -> Vec<(String, u8, Option<crate::evidence::ClaimId>)> {
    let mut out = Vec::new();
    for s in &prog.mesocycle.sessions {
        for rx in &s.prescriptions {
            if let Prescription::Lift(l) = &rx.value {
                out.push((l.exercise.clone(), l.sets, Some(rx.claim_id())));
            }
        }
    }
    out
}

#[test]
fn sized_sets_two_layers_hand_computed() {
    use crate::individualization::TrainingAge;
    // No landmarks (exercise outside Table 4) → training-age fallback:
    // Intermediate MEV floor = 10, generic MRV ceiling = 20. Layer 2 split =
    // round(bounded_weekly / n_lift_days), clamped to [MIN_SETS_PER_EXERCISE=2,
    // PER_SESSION_SET_CEILING=11] then into the band.

    // Headline: 12 sets/muscle/week, 3 lift days → 12/3 = 4/session (in band).
    // Every case is WEEK 1 of the block, where the ramp opens at the stated
    // target, so these are the un-ramped figures.
    assert_eq!(
        sized_sets(12, TrainingAge::Intermediate, 3, (3, 5), None, 1, 4).per_session,
        4
    );

    // Below MEV: a stated 6 is raised to the intermediate MEV floor 10 first,
    // 10/3 = 3.33 → round 3, in band (3,5) → 3 (never sub-MEV volume).
    assert_eq!(
        sized_sets(6, TrainingAge::Intermediate, 3, (3, 5), None, 1, 4).per_session,
        3
    );

    // Above MRV: a stated 30 is capped to the generic MRV 20 (diminishing
    // returns), 20/3 = 6.67 → round 7, then the day's own hypertrophy band
    // (3,4) caps it at 4 - never scaled to an absurd per-session count.
    assert_eq!(
        sized_sets(30, TrainingAge::Intermediate, 3, (3, 4), None, 1, 4).per_session,
        4
    );

    // 2 lift days (hybrid): 12/2 = 6, band (3,6) → 6.
    assert_eq!(
        sized_sets(12, TrainingAge::Intermediate, 2, (3, 6), None, 1, 4).per_session,
        6
    );

    // Per-session cap (HYP-SESSCAP-001 ~11) binds before a huge open band:
    // MRV 20 over 1 lift day = 20 → clamped to 11 → band (3, 15) → 11.
    assert_eq!(
        sized_sets(30, TrainingAge::Intermediate, 1, (3, 15), None, 1, 4).per_session,
        11
    );
}

#[test]
fn sized_sets_uses_per_muscle_landmarks_over_the_generic_band() {
    use crate::individualization::TrainingAge;
    // With the primary muscle's own Table 1 landmarks the floor and
    // ceiling are per-muscle, not the flat MEV-10 / MRV-20 fallback.
    let glutes = crate::hypertrophy::landmarks_for("glutes"); // MEV 0, MRV 16
    let side_delts = crate::hypertrophy::landmarks_for("side delts"); // MEV 8, MRV 26

    // Glutes MEV is 0, so a stated 6 is NOT raised to the generic floor 10;
    // 6/1 = 6, open band → 6 (glute-focused work is not force-floored).
    assert_eq!(
        sized_sets(6, TrainingAge::Intermediate, 1, (2, 11), glutes, 1, 4).per_session,
        6
    );
    // Side delts MRV is 26, so a stated 26 is NOT clamped to the generic 20;
    // 26/2 = 13 → per-session cap 11, open band → 11 (not clamped to the generic MRV 20).
    assert_eq!(
        sized_sets(26, TrainingAge::Intermediate, 2, (2, 15), side_delts, 1, 4).per_session,
        11
    );
    // Glutes MRV 16 still caps above it: 20 → 16, /1 = 16 → cap 11.
    assert_eq!(
        sized_sets(20, TrainingAge::Intermediate, 1, (2, 15), glutes, 1, 4).per_session,
        11
    );
}

#[test]
fn default_12_sets_dup_primary_is_four_and_cites_volume_dose_response() {
    // base_profile: MaxStrength, Intermediate (WeekToWeek), weekly_sets = 12,
    // no running → 3 lift days [Heavy, Power, Hypertrophy]. The anchored primary
    // (Back Squat) is DUP + volume-sized: 12/3 = 4/session on every lift day,
    // and the badge cites the Moderate volume dose-response principle.
    let prog = synthesize(&base_profile(), &anchored("Back Squat", 100.0), 0).unwrap();
    let primary: Vec<_> = lift_sets_and_claims(&prog)
        .into_iter()
        .filter(|(ex, _, _)| ex.eq_ignore_ascii_case("Back Squat"))
        .collect();
    assert!(!primary.is_empty(), "the DUP primary must be scheduled");
    for (ex, sets, claim) in &primary {
        assert_eq!(*sets, 4, "{ex} DUP sets = 12/3 = 4");
        assert_eq!(
            *claim,
            Some(claims::HYP_VOL_DOSE_001),
            "a volume-sized DUP prescription cites the dose-response principle"
        );
    }
}

/// The DUP primary (first prescription) of a given lift day, with the claim
/// its evidence face carries.
fn primary_lift_on(
    prog: &Program,
    lt: LiftSessionType,
) -> (LiftPrescription, Option<crate::evidence::ClaimId>) {
    prog.mesocycle
        .sessions
        .iter()
        .find(|s| s.session_type == SessionType::Lift(lt))
        .and_then(|s| s.prescriptions.first())
        .map(|rx| match &rx.value {
            Prescription::Lift(l) => (l.clone(), Some(rx.claim_id())),
            other => panic!("a lift day's first prescription is a lift: {other:?}"),
        })
        .expect("the lift day is scheduled")
}

#[test]
fn strength_heavy_pct_ramps_across_block() {
    // A compliant intermediate's heavy %1RM rises week over week
    // inside the cited 85–90 % band (STR-PCTPROG-001: +2.5 %/wk lower body),
    // so the prescribed LOAD rises with it.
    let mut p = base_profile();
    p.weekly_sets = 10;
    let a = anchored("Back Squat", 100.0);
    let pct = |week_index: u32| {
        let (l, claim) = primary_lift_on(
            &synthesize(&p, &a, week_index).unwrap(),
            LiftSessionType::MaxEffort,
        );
        match l.intensity {
            LiftIntensity::PercentOneRm(x) => (x, claim),
            other => panic!("heavy day is %1RM: {other:?}"),
        }
    };
    let weeks: Vec<(f32, Option<crate::evidence::ClaimId>)> = (0..4).map(pct).collect();
    for w in weeks.windows(2) {
        assert!(w[1].0 >= w[0].0, "non-decreasing: {weeks:?}");
    }
    assert!(
        weeks[3].0 > weeks[0].0,
        "week 4 heavier than week 1: {weeks:?}"
    );
    assert_eq!(weeks[0].0, 85.0);
    assert!(weeks[3].0 <= 90.0, "clamped to the band high: {weeks:?}");
    assert_eq!(weeks[3].1, Some(claims::STR_PCTPROG_001));
}

#[test]
fn no_load_week_prescribes_a_percent_set_below_one_rep_in_reserve() {
    // A1: across every load week of the block, for an intermediate (DUP) and a
    // novice (linear) strength lifter, each %1RM prescription leaves ≥1 rep in
    // reserve on the Epley model (STR-INTENT-001 / HYP-FAIL-SAFE-001): 6×5 @
    // 88 % can never be emitted.
    let a = Anchors {
        lift_e1rm: vec![
            ("Back Squat".into(), 140.0),
            ("Bench press".into(), 100.0),
            ("Deadlift".into(), 170.0),
            ("Row".into(), 90.0),
        ],
        ..Default::default()
    };
    let mut novice = base_profile();
    novice.progression_cadence = ProgressionCadence::EverySession;
    for p in [base_profile(), novice] {
        for week_index in 0..5 {
            let prog = synthesize(&p, &a, week_index).unwrap();
            for (_, l) in lift_prescriptions(&prog) {
                if let LiftIntensity::PercentOneRm(pct) = l.intensity {
                    let rir =
                        crate::strength::expected_rir_at_pct(f64::from(pct), u32::from(l.reps));
                    assert!(
                        rir >= 1.0,
                        "week {week_index}: {} {}×{} @ {pct}% → {rir:.2} RIR",
                        l.exercise,
                        l.sets,
                        l.reps
                    );
                }
            }
        }
    }
}

#[test]
fn a_missed_last_session_withholds_this_weeks_percent_step() {
    // strength-014 (STR-PCTPROG-001): +2.5 % per SUCCESSFUL week. A last
    // session with a set past RPE 9 repeats last week's percentage.
    let mut p = base_profile();
    p.weekly_sets = 10;
    let heavy_pct = |a: &Anchors| match primary_lift_on(
        &synthesize(&p, a, 1).unwrap(),
        LiftSessionType::MaxEffort,
    )
    .0
    .intensity
    {
        LiftIntensity::PercentOneRm(x) => x,
        other => panic!("heavy day is %1RM: {other:?}"),
    };
    let last = |rpe: f64| crate::plan::LastWorking {
        exercise: "Back Squat".into(),
        weight_kg: 85.0,
        sets: vec![(3, 8.0), (3, rpe)],
        age_days: 2,
    };
    let mut ok = anchored("Back Squat", 100.0);
    ok.last_working = vec![last(8.5)];
    let mut missed = ok.clone();
    missed.last_working = vec![last(10.0)];
    assert_eq!(heavy_pct(&ok), 87.0, "week 2 steps up from 85 %");
    assert_eq!(
        heavy_pct(&missed),
        85.0,
        "a set at RPE 10 holds week 1's 85 %"
    );
}

#[test]
fn a_novice_load_never_jumps_past_the_novice_step_cap() {
    // strength-040 (STR-1RMTEST-001): a lower-body novice step is at most
    // +10 % over the last session, however far the estimate moved. Last time
    // 70 kg × 5; an e1RM of 100 would put 80 % at 80 kg (+14 %).
    let mut p = base_profile();
    p.progression_cadence = ProgressionCadence::EverySession;
    let mut a = anchored("Back Squat", 100.0);
    a.last_working = vec![crate::plan::LastWorking {
        exercise: "Back Squat".into(),
        weight_kg: 70.0,
        sets: vec![(5, 0.0); 3],
        age_days: 2,
    }];
    let prog = synthesize(&p, &a, 0).unwrap();
    let rx = prog
        .mesocycle
        .sessions
        .iter()
        .flat_map(|s| s.prescriptions.iter())
        .find(|rx| matches!(&rx.value, Prescription::Lift(l) if l.exercise == "Back Squat"))
        .expect("the squat is scheduled");
    let Prescription::Lift(l) = &rx.value else {
        unreachable!()
    };
    assert!(
        matches!(l.intensity, LiftIntensity::PercentOneRm(x) if x == 77.0),
        "70 × 1.10 = 77 kg: {:?}",
        l.intensity
    );
    assert_eq!(rx.claim_id(), claims::STR_1RMTEST_001);
}

#[test]
fn strength_goal_sets_do_not_ramp_to_mrv() {
    // The hypertrophy MRV set ramp (HYP-SETRAMP-001) is not
    // the strength lever; a max-strength goal holds its week-1 set count.
    let mut p = base_profile();
    p.weekly_sets = 10;
    let a = anchored("Back Squat", 100.0);
    let sets = |week_index: u32| {
        primary_lift_on(
            &synthesize(&p, &a, week_index).unwrap(),
            LiftSessionType::MaxEffort,
        )
        .0
        .sets
    };
    assert_eq!(sets(0), sets(2), "strength sets hold across the block");
}

#[test]
fn novice_strength_load_rises_week_over_week() {
    // A compliant novice on linear progression gets a heavier prescription
    // each load week (80 → 82 → 83 % for a lower-body ×5, held where 5 reps
    // would leave under 1 in reserve), titled by what it is ("Strength day").
    let mut p = base_profile();
    p.progression_cadence = ProgressionCadence::EverySession;
    let a = anchored("Back Squat", 100.0);
    let squat_pct = |week_index: u32| {
        let prog = synthesize(&p, &a, week_index).unwrap();
        let s = prog
            .mesocycle
            .sessions
            .iter()
            .find(|s| matches!(s.session_type, SessionType::Lift(_)))
            .expect("a lift day");
        assert_eq!(s.session_type, SessionType::Lift(LiftSessionType::FullBody));
        s.prescriptions
            .iter()
            .find_map(|rx| match &rx.value {
                Prescription::Lift(l) if l.exercise == "Back Squat" => match l.intensity {
                    LiftIntensity::PercentOneRm(x) => Some(x),
                    _ => None,
                },
                _ => None,
            })
            .expect("anchored squat is %1RM")
    };
    let w: Vec<f32> = (0..3).map(squat_pct).collect();
    assert!(w[0] < w[1] && w[1] < w[2], "load rises each week: {w:?}");
    assert_eq!(w[0], 80.0);
}

#[test]
fn max_strength_week_has_two_sessions_at_or_above_80pct() {
    // Each anchored main lift gets ≥80 % 1RM in ≥2
    // sessions a week, at 3 lift days (pure lifting) and at 2 (hybrid).
    let a = Anchors {
        lift_e1rm: vec![
            ("Back Squat".into(), 140.0),
            ("Bench press".into(), 100.0),
            ("Deadlift".into(), 170.0),
        ],
        ..Default::default()
    };
    let mut hybrid = runner(GoalDistance::HalfMarathon, 3, 30.0);
    hybrid.weekly_sets = 12;
    for p in [base_profile(), hybrid] {
        let prog = synthesize(&p, &a, 0).unwrap();
        for lift in ["Back Squat", "Bench press", "Deadlift"] {
            let heavy_days = prog.mesocycle
                .sessions
                .iter()
                .filter(|s| {
                    s.prescriptions.iter().any(|rx| {
                        matches!(&rx.value, Prescription::Lift(l)
                            if l.exercise == lift
                                && matches!(l.intensity, LiftIntensity::PercentOneRm(x) if x >= 80.0))
                    })
                })
                .count();
            assert!(
                heavy_days >= 2,
                "{lift}: {heavy_days} heavy day(s) at {} run days",
                p.running_days_per_week
            );
        }
    }
}

#[test]
fn power_profile_with_no_anchors_gets_executable_work_and_a_jump() {
    // A novice Power user with an empty log sees a
    // strength-weighted week (two strength days, one power day), every lift
    // actionable (RIR or a calibration RPE — never a bare % of nothing), and
    // the power day opens with a countermovement jump within the contact cap.
    let mut p = base_profile();
    p.lift_goal = LiftGoal::Power;
    p.progression_cadence = ProgressionCadence::EverySession;
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    let lift_sessions: Vec<&Session> = prog
        .mesocycle
        .sessions
        .iter()
        .filter(|s| matches!(s.session_type, SessionType::Lift(_)))
        .collect();
    let power_days: Vec<&&Session> = lift_sessions
        .iter()
        .filter(|s| s.session_type == SessionType::Lift(LiftSessionType::DynamicEffort))
        .collect();
    assert_eq!(power_days.len(), 1, "one power day in a strength-base week");
    assert_eq!(
        lift_sessions.len() - power_days.len(),
        2,
        "two strength days"
    );
    let Prescription::Lift(first) = &power_days[0].prescriptions[0].value else {
        panic!("a lift prescription")
    };
    assert_eq!(first.exercise, "Vertical jump");
    assert!(matches!(first.intensity, LiftIntensity::MaxIntent));
    let contacts = first.sets as u16 * first.reps as u16;
    let cap = crate::strength::plyo_foot_contact_cap(TrainingAge::Novice)
        .value
        .1;
    assert!(
        contacts <= cap,
        "{contacts} contacts over the novice cap {cap}"
    );
    for s in &lift_sessions {
        for rx in &s.prescriptions {
            let Prescription::Lift(l) = &rx.value else {
                continue;
            };
            assert!(
                !matches!(l.intensity, LiftIntensity::PercentOneRm(_)),
                "no %1RM without an anchor: {} {:?}",
                l.exercise,
                l.intensity
            );
        }
    }
}

#[test]
fn power_day_with_anchors_gets_loads_and_leads_with_ballistic_item() {
    // A Power profile WITH anchors: the power day carries real %1RM loads
    // (velocity intent) after the jump.
    let mut p = base_profile();
    p.lift_goal = LiftGoal::Power;
    let prog = synthesize(&p, &anchored("Back Squat", 140.0), 0).unwrap();
    let day = prog
        .mesocycle
        .sessions
        .iter()
        .find(|s| s.session_type == SessionType::Lift(LiftSessionType::DynamicEffort))
        .expect("a power day");
    let Prescription::Lift(first) = &day.prescriptions[0].value else {
        panic!("a lift prescription")
    };
    assert!(matches!(first.intensity, LiftIntensity::MaxIntent));
    let squat = day
        .prescriptions
        .iter()
        .find_map(|rx| match &rx.value {
            Prescription::Lift(l) if l.exercise == "Back Squat" => Some(l.clone()),
            _ => None,
        })
        .expect("the anchored squat on the power day");
    assert!(
        matches!(squat.intensity, LiftIntensity::PercentOneRm(x) if (50.0..=70.0).contains(&x))
    );
    assert!(
        squat.velocity_loss_pct.is_some(),
        "speed sets carry the move-fast intent"
    );
}

#[test]
fn depth_jump_absent_below_1_5x_bw() {
    // PLYO-PREREQ-001 safety gate: depth jumps only with a squat ≥1.5× BW.
    let mut p = base_profile();
    p.lift_goal = LiftGoal::Power;
    p.bodyweight_kg = Some(100.0);
    let jump = |squat: f64| {
        let prog = synthesize(&p, &anchored("Back Squat", squat), 0).unwrap();
        prog.mesocycle
            .sessions
            .iter()
            .flat_map(|s| s.prescriptions.iter())
            .find_map(|rx| match &rx.value {
                Prescription::Lift(l) if matches!(l.intensity, LiftIntensity::MaxIntent) => {
                    Some(l.exercise.clone())
                }
                _ => None,
            })
            .expect("a power day jump")
    };
    assert_eq!(jump(140.0), "Vertical jump");
    assert_eq!(jump(160.0), "Depth jump (low box)");
}

#[test]
fn strength_goal_hinge_fill_is_deadlift() {
    // The competition lift fills the hinge for strength/power.
    let lifts = main_lifts(&Anchors::default(), LiftGoal::MaxStrength);
    assert!(lifts.iter().any(|n| n == "Deadlift"), "{lifts:?}");
    assert!(!lifts.iter().any(|n| n == "Romanian deadlift"), "{lifts:?}");
    let hyp = main_lifts(&Anchors::default(), LiftGoal::Hypertrophy);
    assert!(hyp.iter().any(|n| n == "Romanian deadlift"), "{hyp:?}");
}

#[test]
fn meet_taper_cuts_volume_holds_intensity_and_spares_the_last_days() {
    // ≤14 days out volume drops 41–60 % with intensity held
    // (TAPER-001); no heavy deadlift inside 10 days (STR-DLPEAK-001); no
    // lifting in the final 2 days (STR-PEAK-001).
    let a = Anchors {
        lift_e1rm: vec![("Back Squat".into(), 140.0), ("Deadlift".into(), 170.0)],
        ..Default::default()
    };
    let p = base_profile();
    let normal = synthesize(&p, &a, 0).unwrap();
    let mut sessions = normal.mesocycle.sessions.clone();
    // Meet on the Sunday 7 days after this Monday: Mon = 7 d out, Wed = 5,
    // Fri = 3.
    crate::plan::lift::apply_meet_taper(&mut sessions, LiftGoal::MaxStrength, 7);
    for (before, after) in normal.mesocycle.sessions.iter().zip(&sessions) {
        if !matches!(before.session_type, SessionType::Lift(_)) {
            continue;
        }
        for rx in &after.prescriptions {
            let Prescription::Lift(l) = &rx.value else {
                continue;
            };
            assert_ne!(l.exercise, "Deadlift", "no deadlift inside 10 days");
            let orig = before
                .prescriptions
                .iter()
                .find_map(|o| match &o.value {
                    Prescription::Lift(ol) if ol.exercise == l.exercise => Some(ol.clone()),
                    _ => None,
                })
                .expect("same lift before the taper");
            assert_eq!(l.intensity, orig.intensity, "intensity held");
            assert!(
                l.sets < orig.sets || orig.sets == 1,
                "volume cut: {} → {}",
                orig.sets,
                l.sets
            );
            assert_eq!(rx.claim_id(), claims::TAPER_001);
        }
    }
    // Meet in 2 days from Monday: Monday's lift day becomes rest.
    let mut close = normal.mesocycle.sessions.clone();
    crate::plan::lift::apply_meet_taper(&mut close, LiftGoal::MaxStrength, 2);
    let monday = close.iter().find(|s| s.day == 0).unwrap();
    assert_eq!(monday.session_type, SessionType::Rest);
    assert!(monday.prescriptions.is_empty());
}

#[test]
fn load_weeks_ramp_unanchored_hypertrophy_rir() {
    // Unanchored lifts are RIR-prescribed. A hypertrophy block walks RIR from
    // 3 (the top of the 1–3 default band) down to 1 in the final load week
    // (HYP-RIR-RAMP-001) instead of repeating week 1's effort all block.
    let mut p = base_profile();
    p.weekly_sets = 10;
    p.lift_goal = LiftGoal::Hypertrophy;
    let (load_weeks, _) = cycle_shape(&p);
    let rep_day = |week_index: u32| {
        primary_lift_on(
            &synthesize(&p, &Anchors::default(), week_index).unwrap(),
            LiftSessionType::Repetition,
        )
    };
    let (first, first_claim) = rep_day(0);
    let (last, last_claim) = rep_day(load_weeks - 1);
    assert!(
        matches!(first.intensity, LiftIntensity::Rir(3)),
        "week 1 opens at the band's conservative RIR end: {:?}",
        first.intensity
    );
    assert!(
        matches!(last.intensity, LiftIntensity::Rir(1)),
        "the final load week reaches RIR 1: {:?}",
        last.intensity
    );
    assert_eq!(
        first_claim,
        Some(claims::HYP_RIR_DEFAULT_001),
        "an un-ramped week cites the 1–3 RIR default"
    );
    assert_eq!(
        last_claim,
        Some(claims::HYP_RIR_RAMP_001),
        "the ramped RIR cites the RIR schedule"
    );
}

#[test]
fn low_weekly_sets_is_raised_to_the_mev_floor_not_programmed_below() {
    // Intermediate lifter states only 6 sets/muscle/week (below the MEV floor
    // 10). The primary DUP sets are sized from the floored 10, not 6:
    // 10/3 = 3.33 → 3/session (never sub-MEV, HYP-MEV-AGE-001).
    let mut p = base_profile();
    p.weekly_sets = 6;
    let prog = synthesize(&p, &anchored("Back Squat", 100.0), 0).unwrap();
    for (ex, sets, _) in lift_sets_and_claims(&prog) {
        if ex.eq_ignore_ascii_case("Back Squat") {
            assert_eq!(sets, 3, "sub-MEV weekly is raised to the MEV floor first");
        }
    }
}

#[test]
fn high_weekly_sets_never_scales_to_an_absurd_per_session_count() {
    // A stated 40 sets/muscle/week is bounded to MRV 20; split over 3 lift days
    // = 6.67 → 7, then each day's own band caps it: Heavy (3,5)→5, Power
    // (3,6)→6, Hypertrophy (3,4)→4. No lift ever exceeds its band or the
    // per-session cap.
    let mut p = base_profile();
    p.weekly_sets = 40;
    let prog = synthesize(&p, &anchored("Back Squat", 100.0), 0).unwrap();
    for (ex, sets, _) in lift_sets_and_claims(&prog) {
        assert!(
            sets <= crate::hypertrophy::PER_SESSION_SET_CEILING,
            "{ex} over the ~11 cap"
        );
        assert!(
            sets <= 6,
            "{ex} exceeded its intensity band ceiling: {sets}"
        );
    }
}

#[test]
fn hyp_vol_dose_claim_is_registered_moderate() {
    let c = claims::HYP_VOL_DOSE_001.entry();
    assert_eq!(c.grade, crate::schema::EvidenceGrade::Moderate);
    // The expert-opinion allocation caveat rides in its contradicting note
    // (GRADE good-practice-statement precedent).
    assert!(
        c.contradicting.iter().any(|s| s.contains("no validated")),
        "the no-validated-split-formula caveat must be recorded"
    );
}

// ── STR-SESSION-BALANCE-001: movement-pattern coverage + ~3-4 exercises ──

/// Exercise names on the first lift day of a synthesized program, in order.
fn first_lift_day_exercises(prog: &Program) -> Vec<String> {
    prog.mesocycle
        .sessions
        .iter()
        .find(|s| matches!(s.session_type, SessionType::Lift(_)))
        .map(|s| {
            s.prescriptions
                .iter()
                .filter_map(|rx| match &rx.value {
                    Prescription::Lift(l) => Some(l.exercise.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn leg_only_anchors() -> Anchors {
    // Three logged lifts, ALL squat-pattern (the on-device "all legs" defect).
    Anchors {
        lift_e1rm: vec![
            ("Back squat".into(), 140.0),
            ("Front squat".into(), 110.0),
            ("Leg press".into(), 200.0),
        ],
        ..Default::default()
    }
}

#[test]
fn sessions_list_the_main_compound_first_whatever_was_logged_last() {
    // Anchors arrive most-recent-first: the row was logged last and the squat
    // first. The plan still lists squat → hinge → push → pull, so the next
    // session never runs in reverse of the session just done.
    let anchors = Anchors {
        lift_e1rm: vec![
            ("Cable row".into(), 70.0),
            ("Bench press".into(), 75.0),
            ("Deadlift".into(), 160.0),
            ("Back squat".into(), 140.0),
        ],
        ..Default::default()
    };
    assert_eq!(
        main_lifts(&anchors, LiftGoal::MaxStrength),
        vec!["Back squat", "Deadlift", "Bench press", "Cable row"]
    );
    let prog = synthesize(&base_profile(), &anchors, 0).unwrap();
    let names = first_lift_day_exercises(&prog);
    assert_eq!(
        names.first().map(String::as_str),
        Some("Back squat"),
        "{names:?}"
    );
    let rank = |n: &String| match movement_pattern(n) {
        Some(MovementPattern::Squat) => 0,
        Some(MovementPattern::Hinge) => 1,
        Some(MovementPattern::Push) => 2,
        Some(MovementPattern::Pull) => 3,
        None => 4,
    };
    assert!(
        names.windows(2).all(|w| rank(&w[0]) <= rank(&w[1])),
        "{names:?}"
    );
}

#[test]
fn movement_pattern_classifies_the_core_set_conservatively() {
    use MovementPattern::*;
    // Hinge before squat: a deadlift variant is a hinge, not a squat.
    assert_eq!(movement_pattern("Romanian deadlift"), Some(Hinge));
    assert_eq!(movement_pattern("Barbell back squat"), Some(Squat));
    assert_eq!(movement_pattern("Front squat"), Some(Squat));
    // "leg press" reads as squat (not the "press" push keyword).
    assert_eq!(movement_pattern("Leg press"), Some(Squat));
    assert_eq!(movement_pattern("Bench press"), Some(Push));
    // A vertical press folds into Push for coverage.
    assert_eq!(movement_pattern("Overhead press"), Some(Push));
    assert_eq!(movement_pattern("Row (barbell/cable/machine)"), Some(Pull));
    assert_eq!(movement_pattern("Pull-up / lat pulldown"), Some(Pull));
    // Ambiguous / isolation → None (unclassifiable, never mis-mapped).
    assert_eq!(movement_pattern("Lateral raise"), None);
    assert_eq!(movement_pattern("Standing calf raise"), None);
    assert_eq!(movement_pattern("Incline DB curl"), None);
    // Isolation leg work never borrows a compound pattern.
    assert_eq!(movement_pattern("Leg curl"), None);
    assert_eq!(movement_pattern("Hamstring curl"), None);
    assert_eq!(movement_pattern("Leg extension"), None);
    // "Nordic" stays a hinge (eccentric hinge pattern, unflagged).
    assert_eq!(movement_pattern("Nordic curl"), Some(Hinge));
}

#[test]
fn renamed_catalog_rows_read_like_their_old_labels() {
    // Every catalog display name keeps the old label's movement, the classes
    // the keyword rules read (pattern, isolation, barbell plates, pain
    // sparing), and logged history under the old label stays the same lift.
    // Ingest stores an old-label set under the new name, so the rules must
    // read both alike.
    use crate::autoreg::{PainRegion, item_spared_by_pain};
    let calis = crate::calisthenics::LEGACY_NAMES;
    for &(old, new) in crate::hypertrophy::LEGACY_NAMES.iter().chain(calis) {
        assert!(same_movement(old, new), "{old} ≡ {new}");
        assert_eq!(
            movement_pattern(old),
            movement_pattern(new),
            "{old} → {new}"
        );
        assert_eq!(
            is_isolation_lift(old),
            is_isolation_lift(new),
            "{old} → {new}"
        );
        assert_eq!(is_barbell_lift(old), is_barbell_lift(new), "{old} → {new}");
        for region in [
            PainRegion::LowerLimb,
            PainRegion::UpperLimb,
            PainRegion::Back,
        ] {
            assert_eq!(
                item_spared_by_pain(region, old),
                item_spared_by_pain(region, new),
                "{old} → {new} under {region:?} pain"
            );
        }
        if !calis.contains(&(old, new)) {
            assert!(crate::hypertrophy::exercise_entry(old).is_some_and(|e| e.name == new));
        }
        assert!(!new.contains('/') && !new.contains('('), "{new}");
    }
}

#[test]
fn a_logged_isolation_lift_never_displaces_a_compound() {
    // A leg curl logged BEFORE a deadlift must not cover the Hinge pattern
    // and skip the deadlift (an isolation lift displacing the compound). The
    // leg curl is unclassifiable (covers no pattern), so
    // the anchored deadlift is still scheduled and the leg curl is kept.
    let anchors = Anchors {
        lift_e1rm: vec![("Leg curl".into(), 40.0), ("Deadlift".into(), 200.0)],
        ..Default::default()
    };
    let lifts = main_lifts(&anchors, LiftGoal::Hypertrophy);
    assert!(
        lifts.iter().any(|n| n.eq_ignore_ascii_case("Deadlift")),
        "the anchored deadlift is not displaced by the leg curl: {lifts:?}"
    );
    assert!(
        lifts.iter().any(|n| n.eq_ignore_ascii_case("Leg curl")),
        "the unclassifiable leg curl is kept, never dropped: {lifts:?}"
    );
}

#[test]
fn dup_primary_skips_a_more_recently_logged_isolation_lift() {
    // "Lateral raise" is a catalog isolation lift (e1RM-unreliable per
    // STR-E1RM-CHECK-001) logged more recently than the compound bench, so
    // build_plan_anchors sorts it first. The DUP primary (out[0]) is
    // undulated at %1RM every lift day, so it must NOT be the isolation lift:
    // the reliable compound leads, and the isolation lift is still scheduled.
    let anchors = Anchors {
        lift_e1rm: vec![
            ("Lateral raise".into(), 12.0),
            ("Bench press".into(), 100.0),
        ],
        ..Default::default()
    };
    let lifts = main_lifts(&anchors, LiftGoal::MaxStrength);
    assert!(
        lifts[0].eq_ignore_ascii_case("Bench press"),
        "the reliable compound leads the DUP primary slot, not the isolation lift: {lifts:?}"
    );
    assert!(
        lifts
            .iter()
            .any(|n| n.eq_ignore_ascii_case("Lateral raise")),
        "the isolation lift is still scheduled as an accessory, never dropped: {lifts:?}"
    );
}

#[test]
fn a_leg_only_log_still_covers_push_and_pull_with_three_to_four_exercises() {
    // Hybrid (2 lift days) whose log is ALL squat-pattern. The day must cover
    // push + pull (not all legs) and carry 3-4 exercises.
    let mut p = base_profile();
    p.goal_distance = GoalDistance::HalfMarathon;
    p.running_days_per_week = 3; // → 2 lift days
    let prog = synthesize(&p, &leg_only_anchors(), 0).unwrap();
    let names = first_lift_day_exercises(&prog);
    assert!(
        (3..=4).contains(&names.len()),
        "3-4 exercises per lift day, got {names:?}"
    );
    // Covers push and pull, never only leg exercises.
    assert!(
        names
            .iter()
            .any(|n| movement_pattern(n) == Some(MovementPattern::Push)),
        "the balanced day must cover a push: {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| movement_pattern(n) == Some(MovementPattern::Pull)),
        "the balanced day must cover a pull: {names:?}"
    );
    // The anchored primary is still the user's OWN logged squat.
    assert!(
        names[0].eq_ignore_ascii_case("Back squat"),
        "the logged lift still leads: {names:?}"
    );
    // Exactly ONE squat-pattern lift: front squat / leg press are dropped for
    // balance, not stacked into a leg pile.
    let squats = names
        .iter()
        .filter(|n| movement_pattern(n) == Some(MovementPattern::Squat))
        .count();
    assert_eq!(
        squats, 1,
        "only one squat-pattern lift, not a leg pile: {names:?}"
    );
}

#[test]
fn a_log_less_hybrid_gets_a_balanced_default_day() {
    // No history at all: the catalog default must itself span all 4 core
    // patterns (never a squat-led 2-exercise default).
    let mut p = base_profile();
    p.goal_distance = GoalDistance::HalfMarathon;
    p.running_days_per_week = 3;
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    let names = first_lift_day_exercises(&prog);
    assert_eq!(names.len(), 4, "a log-less day fills to 4: {names:?}");
    let mut pats: Vec<MovementPattern> = names.iter().filter_map(|n| movement_pattern(n)).collect();
    pats.sort_by_key(|p| format!("{p:?}"));
    pats.dedup();
    assert_eq!(
        pats.len(),
        4,
        "the default day spans all 4 core patterns: {names:?}"
    );
}

#[test]
fn pure_lifting_day_is_no_longer_the_sparse_two_exercise_session() {
    // The 2-exercise sparsity hit pure lifting too (KB): a pure lifter's day
    // now carries 3-4 exercises, balanced.
    let prog = synthesize(&base_profile(), &anchored("Back squat", 120.0), 0).unwrap();
    let names = first_lift_day_exercises(&prog);
    assert!(
        (3..=4).contains(&names.len()),
        "pure lifting day 3-4 exercises: {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| movement_pattern(n) == Some(MovementPattern::Pull)),
        "a pure lifter's day is balanced, not squat-only: {names:?}"
    );
}

#[test]
fn every_prescribed_lift_still_gets_sized_sets_in_a_balanced_day() {
    // The 2→4-exercise change must not leave any exercise unsized: every lift
    // prescription carries a sets count within [MIN, PER_SESSION_SET_CEILING],
    // and the anchored primary is still volume-sized (12/3 = 4) + dose-cited.
    let prog = synthesize(&base_profile(), &anchored("Back squat", 120.0), 0).unwrap();
    let lifts = lift_sets_and_claims(&prog);
    assert!(
        lifts.len() >= 4,
        "a balanced 3-lift-day pure-lifting week has many prescriptions: {}",
        lifts.len()
    );
    for (ex, sets, _) in &lifts {
        assert!(
            *sets >= crate::hypertrophy::MIN_SETS_PER_EXERCISE,
            "{ex} sized below the minimum: {sets}"
        );
        assert!(
            *sets <= crate::hypertrophy::PER_SESSION_SET_CEILING,
            "{ex} sized over the per-session cap: {sets}"
        );
    }
    assert!(
        lifts
            .iter()
            .any(|(ex, sets, claim)| ex.eq_ignore_ascii_case("Back squat")
                && *sets == 4
                && *claim == Some(claims::HYP_VOL_DOSE_001)),
        "the anchored primary is volume-sized (4) and cites the dose-response"
    );
}

#[test]
fn str_session_balance_claim_is_registered_moderate() {
    let c = claims::STR_SESSION_BALANCE_001.entry();
    assert_eq!(c.grade, crate::schema::EvidenceGrade::Moderate);
    // The expert-opinion count/taxonomy caveat AND the running-emphasis caveat
    // must both ride in the contradicting notes (GRADE good-practice-statement
    // precedent: cite the principle, flag the unstudied parameters).
    assert!(
        c.contradicting.iter().any(|s| s.contains("expert-opinion")),
        "the count/taxonomy expert-opinion caveat must be recorded"
    );
    assert!(
        c.contradicting
            .iter()
            .any(|s| s.contains("running-focused hybrid")),
        "the running-emphasis (balance-is-a-default) caveat must be recorded"
    );
}
