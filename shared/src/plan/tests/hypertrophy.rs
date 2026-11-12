//! Hypertrophy-goal week tests: 1–3 RIR every day,
//! direct work for every trainable muscle, delivered fractional volume, double
//! progression, rest and rep windows by exercise class.

use super::*;
use crate::exercise::{EXERCISES, ExerciseClass, classify};
use crate::hypertrophy as hyp_kb;
use crate::plan::hypertrophy_week_targets;
use crate::schema::LiftSessionType;
use crate::strength::expected_rir_at_pct;

fn hyp_profile(weekly_sets: u8) -> Profile {
    let mut p = base_profile();
    p.lift_goal = LiftGoal::Hypertrophy;
    p.weekly_sets = weekly_sets;
    p
}

/// Weekly fractional sets per Table 1 muscle the program delivers.
fn delivered(prog: &Program) -> Vec<(&'static str, f32)> {
    hyp_kb::delivered_weekly_sets(
        lift_rx(prog)
            .iter()
            .map(|(_, l, _)| (l.exercise.as_str(), l.sets)),
    )
    .value
}

fn muscle(delivered: &[(&'static str, f32)], m: &str) -> f32 {
    delivered
        .iter()
        .find(|(k, _)| *k == m)
        .unwrap_or_else(|| panic!("{m} in {delivered:?}"))
        .1
}

#[test]
fn hypertrophy_goal_never_schedules_power_day() {
    for running_days in [0u8, 3] {
        let mut p = hyp_profile(12);
        if running_days > 0 {
            p.goal_distance = GoalDistance::HalfMarathon;
            p.running_days_per_week = running_days;
            p.running_km_per_week = 30.0;
        }
        let prog = synthesize(&PlanProfile::new(&p), &anchored("Back Squat", 140.0), 0).unwrap();
        let all = lift_rx(&prog);
        assert!(!all.is_empty());
        for (s, l, _) in &all {
            assert_eq!(
                s.session_type,
                SessionType::Lift(LiftSessionType::Repetition),
                "{l:?}"
            );
            assert!(l.velocity_loss_pct.is_none(), "no speed sets: {l:?}");
        }
    }
}

#[test]
fn hypertrophy_goal_anchored_sets_within_1_to_3_rir() {
    // A %1RM set's implied reserve comes from the engine's Epley model.
    let p = hyp_profile(12);
    let anchors = anchored_many(&[("Bench press", 100.0), ("Romanian deadlift", 140.0)]);
    let mut saw_pct = false;
    for w in 0..8 {
        let prog = synthesize(&PlanProfile::new(&p), &anchors, w).unwrap();
        if prog.mesocycle.phase == MesoPhase::Deload {
            continue;
        }
        for (_, l, _) in lift_rx(&prog) {
            match l.intensity {
                LiftIntensity::Rir(n) => assert!((1..=3).contains(&n), "week {w}: {l:?}"),
                LiftIntensity::PercentOneRm(pct) => {
                    saw_pct = true;
                    let rir = expected_rir_at_pct(f64::from(pct), u32::from(l.reps));
                    assert!(
                        (0.5..=3.5).contains(&rir),
                        "week {w}: {} at {pct:.1}% × {} ≈ {rir:.1} RIR",
                        l.exercise,
                        l.reps
                    );
                }
                other => panic!("unexpected intensity {other:?}"),
            }
        }
    }
    assert!(saw_pct, "anchored compounds start from their e1RM");
}

#[test]
fn novice_hypertrophy_anchored_rir_within_3_to_4_week1() {
    let mut p = hyp_profile(10);
    p.progression_cadence = ProgressionCadence::EverySession; // → Novice
    let prog = synthesize(&PlanProfile::new(&p), &anchored("Bench press", 100.0), 0).unwrap();
    for (_, l, _) in lift_rx(&prog) {
        let rir = match l.intensity {
            LiftIntensity::Rir(n) => f64::from(n),
            LiftIntensity::PercentOneRm(pct) => {
                expected_rir_at_pct(f64::from(pct), u32::from(l.reps))
            }
            other => panic!("unexpected intensity {other:?}"),
        };
        assert!(
            (2.5..=4.5).contains(&rir),
            "novice week 1 starts at 3–4 RIR: {} ≈ {rir:.1}",
            l.exercise
        );
    }
}

#[test]
fn novice_hypertrophy_effort_ramps_across_block() {
    let mut p = hyp_profile(10);
    p.progression_cadence = ProgressionCadence::EverySession;
    let block: Vec<u8> = (0..u32::from(LIFT_LOAD_WEEKS))
        .map(|w| {
            let prog = synthesize(&PlanProfile::new(&p), &Anchors::default(), w).unwrap();
            match lift_rx(&prog)[0].1.intensity {
                LiftIntensity::Rir(n) => n,
                other => panic!("unanchored is RIR: {other:?}"),
            }
        })
        .collect();
    assert!(block.len() >= 3, "{block:?}");
    assert_eq!(block[0], 4, "novices start at 4 RIR: {block:?}");
    assert!(block.windows(2).all(|w| w[1] <= w[0]), "{block:?}");
    assert!(block.last().unwrap() < &block[0], "effort ramps: {block:?}");
    assert!(
        block.iter().all(|&r| r >= 2),
        "never below 2 RIR: {block:?}"
    );
}

#[test]
fn hypertrophy_week_hits_every_table1_muscle_directly() {
    let prog = synthesize(&PlanProfile::new(&hyp_profile(12)), &Anchors::default(), 0).unwrap();
    let direct: Vec<&'static str> = lift_rx(&prog)
        .iter()
        .filter_map(|(_, l, _)| hyp_kb::primary_muscle(&l.exercise))
        .collect();
    for m in [
        "chest",
        "back",
        "quads",
        "hamstrings",
        "side delts",
        "triceps",
        "biceps",
        "calves",
    ] {
        assert!(direct.contains(&m), "{m} gets no direct sets: {direct:?}");
    }
}

#[test]
fn hypertrophy_week_includes_long_length_isolation_work() {
    let prog = synthesize(&PlanProfile::new(&hyp_profile(12)), &Anchors::default(), 0).unwrap();
    let names: Vec<&str> = lift_rx(&prog)
        .iter()
        .map(|(_, l, _)| l.exercise.as_str())
        .collect();
    for ex in [
        "Seated leg curl",
        "Overhead triceps extension",
        "Incline dumbbell curl",
        "Lateral raise",
        "Standing calf raise",
    ] {
        assert!(names.contains(&ex), "{ex} missing: {names:?}");
    }
    // High-SFR compound fills replace the barbell squat when none is logged.
    assert!(names.contains(&"Hack squat"), "{names:?}");
    assert!(!names.contains(&"Back squat"), "{names:?}");
    // One plain sentence-case name per exercise: no "a / b" or "(x/y/z)" labels.
    for n in &names {
        assert!(!n.contains('/') && !n.contains('('), "{n}");
    }
}

#[test]
fn logged_isolation_lift_is_scheduled_by_rir() {
    let anchors = Anchors {
        lift: crate::plan::LiftAnchors {
            last_working: vec![
                worked("Lateral raise", 10.0, &[(15, 8.0), (15, 8.0), (14, 8.5)]),
                worked("Hammer curl", 14.0, &[(12, 8.0), (12, 8.0)]),
            ],
            ..Default::default()
        },
        ..Default::default()
    };
    let prog = synthesize(&PlanProfile::new(&hyp_profile(12)), &anchors, 0).unwrap();
    let all = lift_rx(&prog);
    let raise = all
        .iter()
        .find(|(_, l, _)| l.exercise == "Lateral raise")
        .expect("the logged lateral raise stays in the plan");
    assert!(
        matches!(raise.1.intensity, LiftIntensity::Rir(_)),
        "{raise:?}"
    );
    assert_eq!(raise.1.load_kg, Some(10.0), "its own load carries forward");
    assert!(
        all.iter().any(|(_, l, _)| l.exercise == "Hammer curl"),
        "a logged non-catalog curl replaces the default curl"
    );
    assert!(
        !all.iter()
            .any(|(_, l, _)| l.exercise == "Incline dumbbell curl")
    );
}

#[test]
fn hypertrophy_delivered_volume_tracks_stated_target() {
    // Every muscle gets the weekly target the program card shows for the
    // block's first week on the days the plan lifts.
    let anchored_mix = anchored_many(&[("Barbell back squat", 140.0), ("Bench press", 100.0)]);
    for weekly in [8u8, 12, 16] {
        for lift_days in [None, Some(2u8)] {
            for anchors in [Anchors::default(), anchored_mix.clone()] {
                let mut p = hyp_profile(weekly);
                p.lift_days_per_week = lift_days;
                let pp = PlanProfile::new(&p);
                let prog = synthesize(&pp, &anchors, 0).unwrap();
                let delivered = delivered(&prog);
                let days = lift_sessions(&prog).len();
                for t in hypertrophy_week_targets(&pp, 1, LIFT_LOAD_WEEKS, days) {
                    // Rear delts have no Table 4 exercise; MEV-0 muscles have no
                    // direct target.
                    if t.target == 0 || t.muscle == "rear delts" {
                        continue;
                    }
                    let got = muscle(&delivered, t.muscle);
                    assert!(
                        (got - f32::from(t.target)).abs() <= 1.0,
                        "weekly {weekly}, days {lift_days:?}: {} delivered {got} vs target {}",
                        t.muscle,
                        t.target
                    );
                }
            }
        }
    }
}

#[test]
fn fractional_count_half_for_synergists() {
    let d = hyp_kb::delivered_weekly_sets([
        ("Bench press", 4u8),
        ("Bench press", 4),
        ("Bench press", 4),
    ])
    .value;
    let get = |m: &str| d.iter().find(|(k, _)| *k == m).unwrap().1;
    assert_eq!(get("chest"), 12.0);
    assert_eq!(get("triceps"), 6.0);
    assert_eq!(get("side delts"), 0.0);
}

#[test]
fn hypertrophy_load_rises_after_top_of_range() {
    // "Lat pulldown" is a machine compound → 8–12 reps. Every set at the top
    // (12) within the target effort → +2.5 kg, back to the range bottom.
    let anchors = Anchors {
        lift: crate::plan::LiftAnchors {
            last_working: vec![worked(
                "Lat pulldown",
                60.0,
                &[(12, 7.0), (12, 7.0), (12, 7.0)],
            )],
            ..Default::default()
        },
        ..Default::default()
    };
    let prog = synthesize(&PlanProfile::new(&hyp_profile(12)), &anchors, 0).unwrap();
    let (_, l, claim) = lift_rx(&prog)
        .into_iter()
        .find(|(_, l, _)| l.exercise == "Lat pulldown")
        .expect("the logged pulldown is scheduled");
    assert_eq!(l.load_kg, Some(62.5));
    assert_eq!((l.reps, l.reps_max), (8, Some(12)));
    assert_eq!(claim, claims::HYP_DOUBLEPROG_001);
}

#[test]
fn hypertrophy_load_holds_below_top() {
    let anchors = Anchors {
        lift: crate::plan::LiftAnchors {
            // 10 reps at RPE 7, week 1's 3-RIR target.
            last_working: vec![worked(
                "Lat pulldown",
                60.0,
                &[(10, 7.0), (10, 7.0), (10, 7.0)],
            )],
            ..Default::default()
        },
        ..Default::default()
    };
    let prog = synthesize(&PlanProfile::new(&hyp_profile(12)), &anchors, 0).unwrap();
    let (_, l, _) = lift_rx(&prog)
        .into_iter()
        .find(|(_, l, _)| l.exercise == "Lat pulldown")
        .unwrap();
    assert_eq!(l.load_kg, Some(60.0), "same load");
    assert_eq!(l.reps, 11, "aim one rep higher");
}

#[test]
fn top_of_range_at_failure_holds_the_load() {
    // 12 reps at RPE 10 (0 RIR) overshot the target effort: not ready to add.
    // At the 2-RIR target those sets were 10 reps, so the next asks 11.
    let step = hyp_kb::double_progression((8, 12), 2, &[(12, 10.0), (12, 10.0)]).value;
    assert_eq!(step, hyp_kb::DoubleProgression::Hold { target_reps: 11 });
    // Unrated sets never block progression.
    let step = hyp_kb::double_progression((8, 12), 2, &[(12, 0.0), (13, 0.0)]).value;
    assert_eq!(step, hyp_kb::DoubleProgression::AddLoad);
    // A far-off rep scheme re-derives the load instead.
    let step = hyp_kb::double_progression((8, 12), 2, &[(3, 9.0)]).value;
    assert_eq!(step, hyp_kb::DoubleProgression::Rederive);
}

#[test]
fn a_harder_last_session_asks_its_reps_at_the_new_effort() {
    // A new block opens at 3 RIR after a 1-RIR week (HYP-RIR-RAMP-001): 11
    // reps at RPE 9 were 9 reps at 3 RIR, so the load holds for 10, not 12
    // (14 reps' worth at that effort, out of reach).
    let step = hyp_kb::double_progression((8, 12), 3, &[(11, 9.0), (11, 9.0)]).value;
    assert_eq!(step, hyp_kb::DoubleProgression::Hold { target_reps: 10 });
    // Strength-phase sets of 5 at RPE 9 are 3 reps at 3 RIR, more than 2
    // under a 6–10 range: re-derive the load (a lifter switched to
    // hypertrophy was asked 6 reps at 3 RIR with the 5-rep load, long_horizon
    // `intermediate_lifter_strength_then_hypertrophy`).
    let step = hyp_kb::double_progression((6, 10), 3, &[(5, 9.0); 7]).value;
    assert_eq!(step, hyp_kb::DoubleProgression::Rederive);
    // Half a rep harder than the target is on target (hyp-020): unchanged.
    let step = hyp_kb::double_progression((8, 12), 2, &[(10, 8.5), (10, 8.0)]).value;
    assert_eq!(step, hyp_kb::DoubleProgression::Hold { target_reps: 11 });
}

#[test]
fn hypertrophy_rest_never_below_90s() {
    for e in EXERCISES {
        assert!(hyp_kb::rest_sec_for(e.class).value.0 >= 90, "{}", e.name);
    }
    let prog = synthesize(&PlanProfile::new(&hyp_profile(12)), &Anchors::default(), 0).unwrap();
    for (_, l, _) in lift_rx(&prog) {
        assert!(l.rest_sec >= 90, "{} rests {} s", l.exercise, l.rest_sec);
        if classify(&l.exercise) == ExerciseClass::HeavyCompound {
            assert!(l.rest_sec >= 120, "{} rests {} s", l.exercise, l.rest_sec);
        }
    }
}

#[test]
fn unknown_compound_rests_like_heavy_compound() {
    assert_eq!(classify("Front squat"), ExerciseClass::HeavyCompound);
    assert_eq!(hyp_kb::rest_sec_for(classify("Front squat")).value.0, 120);
    assert_eq!(classify("Leg press"), ExerciseClass::ModerateCompound);
    assert_eq!(classify("Hammer curl"), ExerciseClass::Isolation);
}

#[test]
fn hypertrophy_heavy_compound_reps_within_5_to_10_and_isolation_off_the_edge() {
    let anchors = anchored_many(&[("Barbell back squat", 140.0), ("Romanian deadlift", 140.0)]);
    let prog = synthesize(&PlanProfile::new(&hyp_profile(12)), &anchors, 0).unwrap();
    let rxs = lift_rx(&prog);
    assert!(!rxs.is_empty(), "a hypertrophy week prescribes lifts");
    for (_, l, _) in rxs {
        let top = l.reps_max.unwrap_or(l.reps);
        match classify(&l.exercise) {
            ExerciseClass::HeavyCompound => {
                assert!((5..=10).contains(&l.reps) && top <= 10, "{l:?}")
            }
            ExerciseClass::Isolation => {
                assert!(l.reps >= 12 && top < 25, "{l:?}")
            }
            ExerciseClass::ModerateCompound => assert!((8..=15).contains(&l.reps), "{l:?}"),
        }
    }
}

#[test]
fn session_title_matches_prescriptions() {
    let prog = synthesize(&PlanProfile::new(&hyp_profile(12)), &Anchors::default(), 0).unwrap();
    let lift_titles: Vec<LiftSessionType> = prog
        .mesocycle
        .sessions
        .iter()
        .filter_map(|s| match s.session_type {
            SessionType::Lift(lt) => Some(lt),
            _ => None,
        })
        .collect();
    assert!(
        !lift_titles.is_empty(),
        "a hypertrophy week has lift sessions"
    );
    assert!(
        lift_titles
            .iter()
            .all(|lt| *lt == LiftSessionType::Repetition),
        "no Heavy/Power title: {lift_titles:?}"
    );
}

#[test]
fn specialization_raises_priority_muscle_and_drops_others_to_mv() {
    let mut p = hyp_profile(12);
    p.progression_cadence = ProgressionCadence::MonthToMonth; // → Advanced
    p.specialization_muscle = Some("Side delts".into());
    let prog = synthesize(&PlanProfile::new(&p), &Anchors::default(), 0).unwrap();
    let spec = delivered(&prog);
    assert_eq!(muscle(&spec, "side delts"), 16.0, "12 + one +4 step");
    assert_eq!(muscle(&spec, "chest"), 8.0, "chest MV");
    assert_eq!(muscle(&spec, "quads"), 6.0, "quads MV");
    // Lateral raises carry the extra volume.
    let raises: u32 = lift_rx(&prog)
        .iter()
        .filter(|(_, l, _)| l.exercise == "Lateral raise")
        .map(|(_, l, _)| u32::from(l.sets))
        .sum();
    assert_eq!(raises, 16);

    // Not advanced → no specialization: every muscle keeps the stated target
    // (on a 5-day week, where the whole-session cap does not bind).
    p.progression_cadence = ProgressionCadence::WeekToWeek;
    p.lift_days_per_week = Some(5);
    let plain = delivered(&synthesize(&PlanProfile::new(&p), &Anchors::default(), 0).unwrap());
    assert_eq!(muscle(&plain, "side delts"), 12.0);
    assert_eq!(muscle(&plain, "chest"), 12.0);
}

#[test]
fn default_ramp_tops_out_at_mav_high_and_days_respect_the_session_cap() {
    // HYP-VOL-CAP-001: a stated-10 week ramps the big muscles to the top of
    // their productive range (MAV high, never MRV) by the last load week, and
    // no day carries more than the whole-session cap.
    let last_load_week = u32::from(LIFT_LOAD_WEEKS) - 1;
    let week = |lift_days_per_week: Option<u8>, w: u32| {
        let p = Profile {
            lift_days_per_week,
            ..hyp_profile(10)
        };
        synthesize(&PlanProfile::new(&p), &Anchors::default(), w).unwrap()
    };
    // Week 1 is still exactly the stated target for a big muscle.
    assert_eq!(muscle(&delivered(&week(None, 0)), "chest"), 10.0);
    // Five lift days leave the session cap room for the whole ramp.
    let top = delivered(&week(Some(5), last_load_week));
    for m in ["chest", "back", "quads", "hamstrings", "side delts"] {
        let lm = hyp_kb::landmarks_for(m).unwrap();
        assert_eq!(muscle(&top, m), f32::from(lm.mav.1), "{m} ends at MAV high");
    }
    for &(m, sets) in &top {
        let mrv = hyp_kb::landmarks_for(m).unwrap().mrv;
        assert!(sets < f32::from(mrv), "{m}: {sets} sets reach MRV {mrv}");
    }
    // The default 3-day week holds every day under the session cap.
    let cap = u32::from(hyp_kb::SESSION_WORKING_SET_CAP);
    let three = week(None, last_load_week);
    let days = lift_sessions(&three);
    assert_eq!(days.len(), 3);
    for s in days {
        let sets: u32 = s
            .prescriptions
            .iter()
            .filter_map(|rx| match &rx.value {
                Prescription::Lift(l) => Some(u32::from(l.sets)),
                _ => None,
            })
            .sum();
        assert!(sets <= cap, "day {}: {sets} sets > {cap}", s.day);
    }
}

#[test]
fn the_specialization_priority_muscle_still_ramps_to_mrv() {
    let lm = hyp_kb::landmarks_for("side delts").unwrap();
    let ramp = hyp_kb::weekly_targets(lm, 12, hyp_kb::VolumeRole::Priority, 5).value;
    assert_eq!(*ramp.last().unwrap(), lm.mrv);
    let std = hyp_kb::weekly_targets(lm, 12, hyp_kb::VolumeRole::Standard, 5).value;
    assert_eq!(*std.last().unwrap(), lm.mav.1);
}
