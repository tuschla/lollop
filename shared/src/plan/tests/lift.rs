//! Lift-day selection, DUP emphasis and lift prescription tests.

use super::*;
use crate::exercise::{MovementPattern, is_barbell_lift, same_movement};
use crate::schema::LiftSessionType;

#[test]
fn anchored_lift_is_percent_based_unanchored_is_rir() {
    let p = base_profile();
    // Anchored: the logged lift is prescribed by %1RM.
    let prog = synthesize(&PlanProfile::new(&p), &anchored("Back Squat", 100.0), 0).unwrap();
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
    let prog = synthesize(&PlanProfile::new(&p), &Anchors::default(), 0).unwrap();
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
    let prog = synthesize(&PlanProfile::new(&p), &Anchors::default(), 0).unwrap();
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
    // the athlete can execute (one set of 5 at RPE 8, STR-E1RM-CHECK-001),
    // never "50 % of an unknown e1RM". On a strength-base day it is the
    // max-strength band by RIR (STR-BASE-POWER-001).
    let p = Profile {
        lift_goal: LiftGoal::Power,
        progression_cadence: ProgressionCadence::EverySession, // → Novice
        ..base_profile()
    };
    let prog = synthesize(&PlanProfile::new(&p), &Anchors::default(), 0).unwrap();
    let squats: Vec<_> = lift_rx(&prog)
        .into_iter()
        .filter(|(_, l, _)| movement_pattern(&l.exercise) == Some(MovementPattern::Squat))
        .collect();
    assert!(!squats.is_empty(), "the week squats");
    for (s, l, claim) in squats {
        if s.session_type == SessionType::Lift(LiftSessionType::DynamicEffort) {
            assert!(
                matches!(l.intensity, LiftIntensity::Rpe(r) if r == 8.0),
                "unanchored power-day lift is a calibration set at RPE 8: {:?}",
                l.intensity
            );
            assert_eq!((l.sets, l.reps), (1, 5));
            assert_eq!(claim, claims::STR_E1RM_CHECK_001);
        } else {
            assert!(
                matches!(l.intensity, LiftIntensity::Rir(3)),
                "{:?}",
                l.intensity
            );
            assert_eq!(claim, claims::STR_BASE_POWER_001);
        }
    }
}

#[test]
fn post_return_anchor_loads_normally_during_a_novice_reentry_window() {
    // `build_plan_anchors` drops PRE-layoff anchors while a >8 wk re-entry
    // window runs (KB Table 3.4b: treat as novice), so any anchor that
    // reaches the plan was logged after the return and loads by %1RM like
    // any other (the novice flag is not a standing cap).
    let mut anchors = anchored("Back Squat", 100.0);
    anchors.lift.reentry_novice = true;
    let prog = synthesize(&PlanProfile::new(&base_profile()), &anchors, 0).unwrap();
    let squats: Vec<_> = lift_rx(&prog)
        .into_iter()
        .filter(|(_, l, _)| l.exercise == "Back Squat")
        .collect();
    assert!(!squats.is_empty(), "the anchored squat is scheduled");
    for (_, l, _) in squats {
        assert!(
            matches!(l.intensity, LiftIntensity::PercentOneRm(_)),
            "{:?}",
            l.intensity
        );
    }
}

/// The anchored lateral raise on the strength week's `emphasis` day: an
/// anchored squat leads, so the week undulates.
fn lateral_raise_on(emphasis: DupDay) -> LiftPrescription {
    let a = anchored_many(&[("Back squat", 140.0), ("Lateral raise", 12.0)]);
    let prog = synthesize(&PlanProfile::new(&base_profile()), &a, 0).unwrap();
    lift_rx(&prog)
        .into_iter()
        .find(|(s, l, _)| s.dup_day == Some(emphasis) && l.exercise == "Lateral raise")
        .map(|(_, l, _)| l.clone())
        .expect("the lateral raise on that day")
}

#[test]
fn isolation_lift_rests_per_its_class_on_a_hypertrophy_emphasis_day() {
    // An isolation lift on a strength DUP hypertrophy-emphasis day takes the
    // class window (HYP-REST-002: 90 s), not the hypertrophy band's 30 s floor.
    assert_eq!(
        lateral_raise_on(DupDay::Hypertrophy).rest_sec,
        90,
        "an isolation lift rests 90 s on a hypertrophy day"
    );
}

#[test]
fn goal_weighted_dup_orders() {
    let emphases = |p: Profile, squat: f64| -> Vec<Option<DupDay>> {
        let prog = synthesize(&PlanProfile::new(&p), &anchored("Back Squat", squat), 0).unwrap();
        lift_sessions(&prog).iter().map(|s| s.dup_day).collect()
    };
    // A max-strength goal pairs Heavy with a second ≥80 % day
    // (STR-HEAVYFREQ-001), so even 2 lift days give two heavy exposures.
    assert_eq!(
        emphases(base_profile(), 140.0),
        [
            Some(DupDay::Heavy),
            Some(DupDay::HeavyVolume),
            Some(DupDay::Hypertrophy)
        ]
    );
    // A power athlete whose squat is below ~1.5× bodyweight gets two heavy
    // days + one power (STR-BASE-POWER-001).
    let power = Profile {
        lift_goal: LiftGoal::Power,
        bodyweight_kg: Some(100.0),
        ..base_profile()
    };
    assert_eq!(
        emphases(power.clone(), 140.0),
        [
            Some(DupDay::Heavy),
            Some(DupDay::Power),
            Some(DupDay::HeavyVolume)
        ]
    );
    // A strong power athlete keeps the classic order (strength-023).
    assert_eq!(
        emphases(power, 160.0),
        [
            Some(DupDay::Heavy),
            Some(DupDay::Power),
            Some(DupDay::Hypertrophy)
        ]
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
    let prog = synthesize(&PlanProfile::new(&p), &anchored("Back Squat", 120.0), 0).unwrap();
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
    let prog = synthesize(&PlanProfile::new(&p), &anchored("Back Squat", 120.0), 0).unwrap();
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
    let l = lateral_raise_on(DupDay::Heavy);
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

/// Per-session sets of `lift` on each DUP day of the default strength week
/// at `weekly_sets`, an anchored squat leading.
fn dup_sets(lift: &str, weekly_sets: u8) -> Vec<(Option<DupDay>, u8)> {
    let p = Profile {
        weekly_sets,
        ..base_profile()
    };
    let a = anchored_many(&[("Back squat", 140.0), (lift, 100.0)]);
    let prog = synthesize(&PlanProfile::new(&p), &a, 0).unwrap();
    lift_rx(&prog)
        .into_iter()
        .filter(|(_, l, _)| l.exercise == lift)
        .map(|(s, l, _)| (s.dup_day, l.sets))
        .collect()
}

#[test]
fn dup_sets_split_the_weekly_target_and_hold_each_days_band() {
    // The deadlift has no Table 1 muscle row, so the generic MRV (20,
    // HYP-MRV-DELOAD-001) bounds it. 12 sets/muscle/week over 3 lift days is
    // 4 a session on every day.
    assert_eq!(
        dup_sets("Deadlift", 12),
        [
            (Some(DupDay::Heavy), 4),
            (Some(DupDay::HeavyVolume), 4),
            (Some(DupDay::Hypertrophy), 4)
        ]
    );
    // A stated 30 never scales to an absurd per-session count: each day's own
    // band caps it (Heavy 3–5, HeavyVolume 3–6, Hypertrophy 3–4).
    assert_eq!(
        dup_sets("Deadlift", 30),
        [
            (Some(DupDay::Heavy), 5),
            (Some(DupDay::HeavyVolume), 6),
            (Some(DupDay::Hypertrophy), 4)
        ]
    );
}

#[test]
fn dup_sets_use_the_primary_muscles_own_mrv_over_the_generic_one() {
    // Glutes' Table 1 MRV is 16: a stated 20 is bounded to 16, 16/3 = 5.33 →
    // 5 on the 3–6-set HeavyVolume day, where the deadlift's generic MRV 20
    // leaves 20/3 → 7 → 6.
    let heavy_volume = |lift| {
        dup_sets(lift, 20)
            .into_iter()
            .find(|(d, _)| *d == Some(DupDay::HeavyVolume))
            .map(|(_, sets)| sets)
            .expect("a HeavyVolume day")
    };
    assert_eq!(heavy_volume("Hip thrust"), 5);
    assert_eq!(heavy_volume("Deadlift"), 6);
}

#[test]
fn default_12_sets_dup_primary_is_four_and_cites_volume_dose_response() {
    // base_profile: MaxStrength, Intermediate (WeekToWeek), weekly_sets = 12,
    // no running → 3 lift days [Heavy, Power, Hypertrophy]. The anchored primary
    // (Back Squat) is DUP + volume-sized: 12/3 = 4/session on every lift day,
    // and the badge cites the Moderate volume dose-response principle.
    let prog = synthesize(
        &PlanProfile::new(&base_profile()),
        &anchored("Back Squat", 100.0),
        0,
    )
    .unwrap();
    let primary: Vec<_> = lift_rx(&prog)
        .into_iter()
        .filter(|(_, l, _)| l.exercise.eq_ignore_ascii_case("Back Squat"))
        .collect();
    assert!(!primary.is_empty(), "the DUP primary must be scheduled");
    for (_, l, claim) in primary {
        assert_eq!(l.sets, 4, "{} DUP sets = 12/3 = 4", l.exercise);
        assert_eq!(
            claim,
            claims::HYP_VOL_DOSE_001,
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
            &synthesize(&PlanProfile::new(&p), &a, week_index).unwrap(),
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
    // Across every load week of the block, for an intermediate (DUP) and a
    // novice (linear) strength lifter, each %1RM prescription leaves ≥1 rep in
    // reserve on the Epley model (STR-INTENT-001 / HYP-FAIL-SAFE-001): 6×5 @
    // 88 % can never be emitted.
    let a = anchored_many(&[
        ("Back Squat", 140.0),
        ("Bench press", 100.0),
        ("Deadlift", 170.0),
        ("Row", 90.0),
    ]);
    let mut novice = base_profile();
    novice.progression_cadence = ProgressionCadence::EverySession;
    for p in [base_profile(), novice] {
        for week_index in 0..5 {
            let prog = synthesize(&PlanProfile::new(&p), &a, week_index).unwrap();
            for (_, l, _) in lift_rx(&prog) {
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
        &synthesize(&PlanProfile::new(&p), a, 1).unwrap(),
        LiftSessionType::MaxEffort,
    )
    .0
    .intensity
    {
        LiftIntensity::PercentOneRm(x) => x,
        other => panic!("heavy day is %1RM: {other:?}"),
    };
    let last = |rpe: f64| worked("Back Squat", 85.0, &[(3, 8.0), (3, rpe)]);
    let mut ok = anchored("Back Squat", 100.0);
    ok.lift.last_working = vec![last(8.5)];
    let mut missed = ok.clone();
    missed.lift.last_working = vec![last(10.0)];
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
    a.lift.last_working = vec![worked("Back Squat", 70.0, &[(5, 0.0); 3])];
    let prog = synthesize(&PlanProfile::new(&p), &a, 0).unwrap();
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
            &synthesize(&PlanProfile::new(&p), &a, week_index).unwrap(),
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
        let prog = synthesize(&PlanProfile::new(&p), &a, week_index).unwrap();
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
    let a = anchored_many(&[
        ("Back Squat", 140.0),
        ("Bench press", 100.0),
        ("Deadlift", 170.0),
    ]);
    let mut hybrid = runner(GoalDistance::HalfMarathon, 3, 30.0);
    hybrid.weekly_sets = 12;
    for p in [base_profile(), hybrid] {
        let prog = synthesize(&PlanProfile::new(&p), &a, 0).unwrap();
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
    // actionable (RIR or a calibration RPE, never a bare % of nothing), and
    // the power day opens with a countermovement jump within the contact cap.
    let mut p = base_profile();
    p.lift_goal = LiftGoal::Power;
    p.progression_cadence = ProgressionCadence::EverySession;
    let prog = synthesize(&PlanProfile::new(&p), &Anchors::default(), 0).unwrap();
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
    let prog = synthesize(&PlanProfile::new(&p), &anchored("Back Squat", 140.0), 0).unwrap();
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
        let prog = synthesize(&PlanProfile::new(&p), &anchored("Back Squat", squat), 0).unwrap();
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
fn depth_jump_gate_reads_only_the_barbell_back_squat() {
    // PLYO-PREREQ-001 names the back squat: a hack, goblet, split or
    // Smith-machine squat at 1.6× BW never qualifies for depth jumps.
    let mut p = base_profile();
    p.lift_goal = LiftGoal::Power;
    p.bodyweight_kg = Some(80.0);
    let depth_jumps = |squat: &str| {
        lift_rx(&synthesize(&PlanProfile::new(&p), &anchored(squat, 130.0), 0).unwrap())
            .iter()
            .any(|(_, l, _)| l.exercise.starts_with("Depth jump"))
    };
    for squat in [
        "Hack squat",
        "Goblet squat",
        "Bulgarian split squat",
        "Smith machine squat",
        "Smith machine back squat",
    ] {
        assert!(!depth_jumps(squat), "{squat} qualified for depth jumps");
    }
    assert!(depth_jumps("Barbell back squat"));
}

#[test]
fn isolation_lifts_are_never_loaded_by_percent() {
    // Skull crushers, face pulls, crunches and sit-ups are isolation in the one
    // taxonomy `exercise::classify` defers to, so on a strength plan an
    // anchored one takes the isolation RIR window, never a %1RM × 3.
    let p = base_profile();
    for iso in ["Skull crusher", "Face pull", "Crunch", "Sit-up"] {
        assert!(is_isolation_lift(iso), "{iso}");
        assert_eq!(
            crate::exercise::classify(iso),
            crate::exercise::ExerciseClass::Isolation,
            "{iso}"
        );
        let prog = synthesize(
            &PlanProfile::new(&p),
            &anchored_many(&[("Back squat", 150.0), (iso, 40.0)]),
            0,
        )
        .unwrap();
        let rx: Vec<_> = lift_rx(&prog)
            .into_iter()
            .filter(|(_, l, _)| l.exercise == iso)
            .collect();
        assert!(!rx.is_empty(), "{iso} scheduled");
        for (_, l, _) in rx {
            assert!(
                !matches!(l.intensity, LiftIntensity::PercentOneRm(_)),
                "{iso}: {:?} x {}",
                l.intensity,
                l.reps
            );
        }
    }
}

#[test]
fn strength_goal_hinge_fill_is_deadlift() {
    // The competition lift fills the hinge for strength/power
    // (STR-COMP-ANCHOR-001), not the catalog's Romanian deadlift.
    for lift_goal in [LiftGoal::MaxStrength, LiftGoal::Power] {
        let p = Profile {
            lift_goal,
            ..base_profile()
        };
        let prog = synthesize(&PlanProfile::new(&p), &Anchors::default(), 0).unwrap();
        let names: Vec<&str> = lift_rx(&prog)
            .iter()
            .map(|(_, l, _)| l.exercise.as_str())
            .collect();
        assert!(names.contains(&"Deadlift"), "{lift_goal:?}: {names:?}");
        assert!(
            !names.contains(&"Romanian deadlift"),
            "{lift_goal:?}: {names:?}"
        );
    }
}

#[test]
fn meet_taper_cuts_volume_holds_intensity_and_spares_the_last_days() {
    // ≤14 days out volume drops 41–60 % with intensity held
    // (TAPER-001); no heavy deadlift inside 10 days (STR-DLPEAK-001); no
    // lifting in the final 2 days (STR-PEAK-001).
    let a = Anchors {
        week_start_epoch_day: Some(MON0),
        ..anchored_many(&[("Back Squat", 140.0), ("Deadlift", 170.0)])
    };
    let week = |meet: Option<i64>| {
        let p = Profile {
            meet_epoch_day: meet,
            ..base_profile()
        };
        synthesize(&PlanProfile::new(&p), &a, 0).unwrap()
    };
    let normal = week(None);
    // Meet on the next Monday: this Monday is 7 days out, Wednesday 5,
    // Friday 3.
    let tapered = week(Some(MON0 + 7));
    let taper_rows = lift_rx(&tapered);
    assert!(!taper_rows.is_empty(), "the taper week still lifts");
    for (s, l, claim) in taper_rows {
        let heavy_pull = l.exercise == "Deadlift"
            && matches!(l.intensity, LiftIntensity::PercentOneRm(p)
                if crate::strength::is_heavy_pct(f64::from(p)));
        assert!(!heavy_pull, "no heavy deadlift inside 10 days");
        let (_, orig, _) = lift_rx(&normal)
            .into_iter()
            .find(|(s0, l0, _)| s0.day == s.day && l0.exercise == l.exercise)
            .expect("same lift before the taper");
        assert_eq!(l.intensity, orig.intensity, "intensity held");
        assert!(
            l.sets < orig.sets || orig.sets == 1,
            "volume cut: {} → {}",
            orig.sets,
            l.sets
        );
        assert_eq!(claim, claims::TAPER_001);
    }
    // Meet 2 days after Monday: Monday's lift day becomes rest.
    let monday = |prog: &Program| prog.mesocycle.sessions.iter().find(|s| s.day == 0).cloned();
    assert!(matches!(
        monday(&normal).map(|s| s.session_type),
        Some(SessionType::Lift(_))
    ));
    let close = monday(&week(Some(MON0 + 2))).unwrap();
    assert_eq!(close.session_type, SessionType::Rest);
    assert!(close.prescriptions.is_empty());
}

#[test]
fn meet_taper_keeps_a_light_deadlift() {
    // STR-DLPEAK-001 times the last near-max pull; the 70 % Friday deadlift
    // 6 days out stays (with its volume cut).
    let anchors = Anchors {
        week_start_epoch_day: Some(20_000),
        ..anchored_many(&[
            ("Back squat", 150.0),
            ("Deadlift", 200.0),
            ("Bench press", 100.0),
        ])
    };
    let friday_lifts = |meet: Option<i64>| -> Vec<(String, LiftIntensity)> {
        let p = Profile {
            meet_epoch_day: meet,
            ..base_profile()
        };
        let prog = synthesize(&PlanProfile::new(&p), &anchors, 0).unwrap();
        let fri = prog.mesocycle.sessions.iter().find(|s| s.day == 4).unwrap();
        fri.prescriptions
            .iter()
            .filter_map(|rx| match &rx.value {
                Prescription::Lift(l) => Some((l.exercise.clone(), l.intensity)),
                _ => None,
            })
            .collect()
    };
    let plain = friday_lifts(None);
    assert!(
        plain
            .iter()
            .any(|(n, i)| n == "Deadlift"
                && matches!(i, LiftIntensity::PercentOneRm(p) if *p < 80.0)),
        "without a meet Friday carries a light (<80 %) deadlift: {plain:?}"
    );
    let tapered = friday_lifts(Some(20_010));
    assert!(
        tapered.iter().any(|(n, _)| n == "Deadlift"),
        "light deadlift 6 days out was dropped: {tapered:?}"
    );
}

#[test]
fn meet_date_without_taper_keeps_the_deload_week() {
    // The taper replaces the deload only for barbell MaxStrength/Power; a
    // hypertrophy or calisthenics lifter with a meet date still deloads.
    let dated = Anchors {
        week_start_epoch_day: Some(20_000),
        ..Default::default()
    };
    for base in [
        Profile {
            lift_goal: LiftGoal::Hypertrophy,
            ..base_profile()
        },
        Profile {
            resistance_modality: crate::profile::ResistanceModality::Calisthenics,
            ..base_profile()
        },
    ] {
        // Week index 5 is the deload week of the (5, 6) lifting block.
        let plain = synthesize(&PlanProfile::new(&base), &dated, 5).unwrap();
        assert_eq!(plain.mesocycle.phase, MesoPhase::Deload, "{base:?}");
        let with_meet = Profile {
            meet_epoch_day: Some(20_010),
            ..base.clone()
        };
        let prog = synthesize(&PlanProfile::new(&with_meet), &dated, 5).unwrap();
        assert_eq!(prog.mesocycle.phase, MesoPhase::Deload, "{base:?}");
    }
}

/// The meet taper shapes sessions 0–14 days out, so a Mon/Wed/Fri deload week
/// whose Monday is 20 days before the meet has no session it reaches: it keeps
/// its deload, and `lift_deload_week` says so.
#[test]
fn a_week_the_meet_taper_misses_keeps_its_deload() {
    let mut p = base_profile();
    p.lift_days_per_week = Some(3);
    let mut a = anchored("Back Squat", 140.0);
    a.lift.e1rm.push(("Bench Press".into(), 100.0));
    a.lift.e1rm.push(("Deadlift".into(), 180.0));
    a.week_start_epoch_day = Some(MON0);
    let sets = |prog: &Program| -> u32 {
        prog.mesocycle
            .sessions
            .iter()
            .flat_map(|s| &s.prescriptions)
            .map(|rx| match &rx.value {
                Prescription::Lift(l) => u32::from(l.sets),
                _ => 0,
            })
            .sum()
    };
    let deload_week = 5;
    assert!(lift_block_position(&PlanProfile::new(&p), deload_week).recovery);
    let plain = sets(&synthesize(&PlanProfile::new(&p), &a, deload_week).unwrap());
    assert!(plain < sets(&synthesize(&PlanProfile::new(&p), &a, deload_week - 1).unwrap()));
    for (meet_after_monday, deload) in [(20, true), (14, false)] {
        p.meet_epoch_day = Some(MON0 + meet_after_monday);
        let prog = synthesize(&PlanProfile::new(&p), &a, deload_week).unwrap();
        assert_eq!(
            prog.mesocycle.phase == MesoPhase::Deload,
            deload,
            "meet {meet_after_monday} days after the deload week's Monday"
        );
        assert_eq!(
            lift_deload_week(&PlanProfile::new(&p), deload_week, MON0),
            deload
        );
        if deload {
            assert_eq!(sets(&prog), plain);
        }
    }
}

#[test]
fn load_weeks_ramp_unanchored_hypertrophy_rir() {
    // Unanchored lifts are RIR-prescribed. A hypertrophy block walks RIR from
    // 3 (the top of the 1–3 default band) down to 1 in the final load week
    // (HYP-RIR-RAMP-001) instead of repeating week 1's effort all block.
    let mut p = base_profile();
    p.weekly_sets = 10;
    p.lift_goal = LiftGoal::Hypertrophy;
    let load_weeks = u32::from(lift_block_position(&PlanProfile::new(&p), 0).block_weeks);
    let rep_day = |week_index: u32| {
        primary_lift_on(
            &synthesize(&PlanProfile::new(&p), &Anchors::default(), week_index).unwrap(),
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
fn high_weekly_sets_never_scales_to_an_absurd_per_session_count() {
    // A stated 40 sets/muscle/week is bounded to MRV 20; split over 3 lift days
    // = 6.67 → 7, then each day's own band caps it: Heavy (3,5)→5, Power
    // (3,6)→6, Hypertrophy (3,4)→4. No lift ever exceeds its band or the
    // per-session cap.
    let mut p = base_profile();
    p.weekly_sets = 40;
    let prog = synthesize(&PlanProfile::new(&p), &anchored("Back Squat", 100.0), 0).unwrap();
    for (_, l, _) in lift_rx(&prog) {
        assert!(
            l.sets <= crate::hypertrophy::PER_SESSION_SET_CEILING,
            "{} over the ~11 cap",
            l.exercise
        );
        assert!(
            l.sets <= 6,
            "{} exceeded its intensity band ceiling: {}",
            l.exercise,
            l.sets
        );
    }
}

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
    // Three logged lifts, ALL squat-pattern.
    anchored_many(&[
        ("Back squat", 140.0),
        ("Front squat", 110.0),
        ("Leg press", 200.0),
    ])
}

#[test]
fn sessions_list_the_main_compound_first_whatever_was_logged_last() {
    // Anchors arrive most-recent-first: the row was logged last and the squat
    // first. The plan still lists squat → hinge → push → pull, so the next
    // session never runs in reverse of the session just done.
    let anchors = anchored_many(&[
        ("Cable row", 70.0),
        ("Bench press", 75.0),
        ("Deadlift", 160.0),
        ("Back squat", 140.0),
    ]);
    let prog = synthesize(&PlanProfile::new(&base_profile()), &anchors, 0).unwrap();
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
    use crate::autoreg::item_spared_by_pain;
    use crate::schema::PainRegion;
    let calis = crate::exercise::CALIS_LEGACY_NAMES;
    for &(old, new) in crate::exercise::LEGACY_NAMES.iter().chain(calis) {
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
            assert!(crate::exercise::exercise_entry(old).is_some_and(|e| e.name == new));
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
    let anchors = anchored_many(&[("Leg curl", 40.0), ("Deadlift", 200.0)]);
    let prog = synthesize(&PlanProfile::new(&base_profile()), &anchors, 0).unwrap();
    let lifts: Vec<&str> = lift_rx(&prog)
        .iter()
        .map(|(_, l, _)| l.exercise.as_str())
        .collect();
    assert!(
        lifts.contains(&"Deadlift"),
        "the anchored deadlift is not displaced by the leg curl: {lifts:?}"
    );
    assert!(
        lifts.contains(&"Leg curl"),
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
    let anchors = anchored_many(&[("Lateral raise", 12.0), ("Bench press", 100.0)]);
    let prog = synthesize(&PlanProfile::new(&base_profile()), &anchors, 0).unwrap();
    let days = lift_sessions(&prog);
    assert!(!days.is_empty());
    for s in days {
        assert!(
            matches!(&s.prescriptions[0].value, Prescription::Lift(l)
                if l.exercise == "Bench press"
                    && matches!(l.intensity, LiftIntensity::PercentOneRm(_))),
            "the reliable compound leads the DUP primary slot, not the isolation lift: {s:?}"
        );
    }
    let raises: Vec<_> = lift_rx(&prog)
        .into_iter()
        .filter(|(_, l, _)| l.exercise == "Lateral raise")
        .collect();
    assert!(
        !raises.is_empty(),
        "the isolation lift is still scheduled as an accessory, never dropped"
    );
    for (_, l, _) in raises {
        assert!(matches!(l.intensity, LiftIntensity::Rir(_)), "{l:?}");
    }
}

#[test]
fn a_leg_only_log_still_covers_push_and_pull_with_three_to_four_exercises() {
    // Hybrid (2 lift days) whose log is ALL squat-pattern. The day must cover
    // push + pull (not all legs) and carry 3-4 exercises.
    let mut p = base_profile();
    p.goal_distance = GoalDistance::HalfMarathon;
    p.running_days_per_week = 3; // → 2 lift days
    let prog = synthesize(&PlanProfile::new(&p), &leg_only_anchors(), 0).unwrap();
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
    let prog = synthesize(&PlanProfile::new(&p), &Anchors::default(), 0).unwrap();
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
    // A pure lifter's day carries 3-4 exercises, balanced.
    let prog = synthesize(
        &PlanProfile::new(&base_profile()),
        &anchored("Back squat", 120.0),
        0,
    )
    .unwrap();
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
    // No exercise is left unsized: every lift
    // prescription carries a sets count within [MIN, PER_SESSION_SET_CEILING],
    // and the anchored primary is still volume-sized (12/3 = 4) + dose-cited.
    let prog = synthesize(
        &PlanProfile::new(&base_profile()),
        &anchored("Back squat", 120.0),
        0,
    )
    .unwrap();
    let lifts = lift_rx(&prog);
    assert!(
        lifts.len() >= 4,
        "a balanced 3-lift-day pure-lifting week has many prescriptions: {}",
        lifts.len()
    );
    for (_, l, _) in &lifts {
        assert!(
            l.sets >= crate::hypertrophy::MIN_SETS_PER_EXERCISE,
            "{} sized below the minimum: {}",
            l.exercise,
            l.sets
        );
        assert!(
            l.sets <= crate::hypertrophy::PER_SESSION_SET_CEILING,
            "{} sized over the per-session cap: {}",
            l.exercise,
            l.sets
        );
    }
    assert!(
        lifts.iter().any(
            |(_, l, claim)| l.exercise.eq_ignore_ascii_case("Back squat")
                && l.sets == 4
                && *claim == claims::HYP_VOL_DOSE_001
        ),
        "the anchored primary is volume-sized (4) and cites the dose-response"
    );
}
