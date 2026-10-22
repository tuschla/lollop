//! Hypertrophy-goal week tests: 1–3 RIR every day,
//! direct work for every trainable muscle, delivered fractional volume, double
//! progression, rest and rep windows by exercise class.

use super::*;
use crate::hypertrophy::{self as hyp_kb, ExerciseClass};
use crate::plan::{BlockPosition, LastWorking, hypertrophy_week_targets, lift_block_position};
use crate::schema::LiftSessionType;

fn hyp_profile(weekly_sets: u8) -> Profile {
    let mut p = base_profile();
    p.lift_goal = LiftGoal::Hypertrophy;
    p.weekly_sets = weekly_sets;
    p
}

/// Every lift prescription of a program with its session type.
fn lifts(prog: &Program) -> Vec<(LiftSessionType, LiftPrescription, crate::evidence::ClaimId)> {
    let mut out = Vec::new();
    for s in &prog.mesocycle.sessions {
        if let SessionType::Lift(lt) = s.session_type {
            for rx in &s.prescriptions {
                if let Prescription::Lift(l) = &rx.value {
                    out.push((lt, l.clone(), rx.claim_id()));
                }
            }
        }
    }
    out
}

fn worked(name: &str, kg: f64, sets: &[(u32, f64)]) -> LastWorking {
    LastWorking {
        exercise: name.to_string(),
        weight_kg: kg,
        sets: sets.to_vec(),
        age_days: 2,
    }
}

/// Load weeks (not the lift recovery week) among the first `n` week indices.
fn load_weeks(p: &Profile, n: u32) -> Vec<u32> {
    (0..n)
        .filter(|&w| !lift_block_position(p, w).recovery)
        .collect()
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
        let prog = synthesize(&p, &anchored("Back Squat", 140.0), 0).unwrap();
        let all = lifts(&prog);
        assert!(!all.is_empty());
        for (lt, l, _) in &all {
            assert_eq!(*lt, LiftSessionType::Repetition, "{l:?}");
            assert!(l.velocity_loss_pct.is_none(), "no speed sets: {l:?}");
        }
    }
}

#[test]
fn hypertrophy_goal_anchored_sets_within_1_to_3_rir() {
    // Implied reserve of a %1RM set from the engine's Epley model:
    // max reps at p% = 30·(100/p − 1); RIR = that − prescribed reps.
    let p = hyp_profile(12);
    let anchors = Anchors {
        lift_e1rm: vec![
            ("Bench press".into(), 100.0),
            ("Romanian deadlift".into(), 140.0),
        ],
        ..Default::default()
    };
    let mut saw_pct = false;
    for w in load_weeks(&p, 8) {
        let prog = synthesize(&p, &anchors, w).unwrap();
        for (_, l, _) in lifts(&prog) {
            match l.intensity {
                LiftIntensity::Rir(n) => assert!((1..=3).contains(&n), "week {w}: {l:?}"),
                LiftIntensity::PercentOneRm(pct) => {
                    saw_pct = true;
                    let rir = 30.0 * (100.0 / pct as f64 - 1.0) - l.reps as f64;
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
    let prog = synthesize(&p, &anchored("Bench press", 100.0), 0).unwrap();
    for (_, l, _) in lifts(&prog) {
        let rir = match l.intensity {
            LiftIntensity::Rir(n) => n as f64,
            LiftIntensity::PercentOneRm(pct) => 30.0 * (100.0 / pct as f64 - 1.0) - l.reps as f64,
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
    let block_weeks = lift_block_position(&p, 0).block_weeks;
    let block: Vec<u8> = (0..u32::from(block_weeks))
        .map(|w| {
            let prog = synthesize(&p, &Anchors::default(), w).unwrap();
            match lifts(&prog)[0].1.intensity {
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
    let prog = synthesize(&hyp_profile(12), &Anchors::default(), 0).unwrap();
    let direct: Vec<&'static str> = lifts(&prog)
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
    let prog = synthesize(&hyp_profile(12), &Anchors::default(), 0).unwrap();
    let names: Vec<String> = lifts(&prog)
        .into_iter()
        .map(|(_, l, _)| l.exercise)
        .collect();
    for ex in [
        "Seated leg curl",
        "Overhead triceps extension",
        "Incline dumbbell curl",
        "Lateral raise",
        "Standing calf raise",
    ] {
        assert!(names.iter().any(|n| n == ex), "{ex} missing: {names:?}");
    }
    // High-SFR compound fills replace the barbell squat when none is logged.
    assert!(names.iter().any(|n| n == "Hack squat"), "{names:?}");
    assert!(!names.iter().any(|n| n == "Back squat"), "{names:?}");
    // One plain sentence-case name per exercise: no "a / b" or "(x/y/z)" labels.
    for n in &names {
        assert!(!n.contains('/') && !n.contains('('), "{n}");
    }
}

#[test]
fn logged_isolation_lift_is_scheduled_by_rir() {
    let anchors = Anchors {
        last_working: vec![
            worked("Lateral raise", 10.0, &[(15, 8.0), (15, 8.0), (14, 8.5)]),
            worked("Hammer curl", 14.0, &[(12, 8.0), (12, 8.0)]),
        ],
        ..Default::default()
    };
    let prog = synthesize(&hyp_profile(12), &anchors, 0).unwrap();
    let all = lifts(&prog);
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
    let anchored_mix = Anchors {
        lift_e1rm: vec![
            ("Barbell back squat".into(), 140.0),
            ("Bench press".into(), 100.0),
        ],
        ..Default::default()
    };
    for weekly in [8u8, 12, 16] {
        for lift_days in [None, Some(2u8)] {
            for anchors in [Anchors::default(), anchored_mix.clone()] {
                let mut p = hyp_profile(weekly);
                p.lift_days_per_week = lift_days;
                let prog = synthesize(&p, &anchors, 0).unwrap();
                let items: Vec<(String, u8)> = lifts(&prog)
                    .into_iter()
                    .map(|(_, l, _)| (l.exercise, l.sets))
                    .collect();
                let delivered =
                    hyp_kb::delivered_weekly_sets(items.iter().map(|(n, s)| (n.as_str(), *s)))
                        .value;
                let BlockPosition {
                    week_in_block: wib,
                    block_weeks: bw,
                    ..
                } = lift_block_position(&p, 0);
                let ta = crate::individualization::training_age_from_cadence(p.progression_cadence)
                    .value;
                let days = crate::plan::week_shape(&p).lift_days;
                for t in hypertrophy_week_targets(&p, ta, wib, bw, days) {
                    // Rear delts have no Table 4 exercise; MEV-0 muscles have no
                    // direct target.
                    if t.target == 0 || t.muscle == "rear delts" {
                        continue;
                    }
                    let got = delivered.iter().find(|(m, _)| *m == t.muscle).unwrap().1;
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
        last_working: vec![worked(
            "Lat pulldown",
            60.0,
            &[(12, 7.0), (12, 7.0), (12, 7.0)],
        )],
        ..Default::default()
    };
    let prog = synthesize(&hyp_profile(12), &anchors, 0).unwrap();
    let (_, l, claim) = lifts(&prog)
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
        last_working: vec![worked(
            "Lat pulldown",
            60.0,
            &[(10, 8.0), (10, 8.0), (10, 8.0)],
        )],
        ..Default::default()
    };
    let prog = synthesize(&hyp_profile(12), &anchors, 0).unwrap();
    let (_, l, _) = lifts(&prog)
        .into_iter()
        .find(|(_, l, _)| l.exercise == "Lat pulldown")
        .unwrap();
    assert_eq!(l.load_kg, Some(60.0), "same load");
    assert_eq!(l.reps, 11, "aim one rep higher");
}

#[test]
fn top_of_range_at_failure_holds_the_load() {
    // 12 reps at RPE 10 (0 RIR) overshot the target effort: not ready to add.
    let step = hyp_kb::double_progression((8, 12), 2, &[(12, 10.0), (12, 10.0)]).value;
    assert_eq!(step, hyp_kb::DoubleProgression::Hold { target_reps: 12 });
    // Unrated sets never block progression.
    let step = hyp_kb::double_progression((8, 12), 2, &[(12, 0.0), (13, 0.0)]).value;
    assert_eq!(step, hyp_kb::DoubleProgression::AddLoad);
    // A far-off rep scheme re-derives the load instead.
    let step = hyp_kb::double_progression((8, 12), 2, &[(3, 9.0)]).value;
    assert_eq!(step, hyp_kb::DoubleProgression::Rederive);
}

#[test]
fn hypertrophy_rest_never_below_90s() {
    for e in hyp_kb::EXERCISES {
        assert!(hyp_kb::rest_sec_for(e.class).value.0 >= 90, "{}", e.name);
    }
    let prog = synthesize(&hyp_profile(12), &Anchors::default(), 0).unwrap();
    for (_, l, _) in lifts(&prog) {
        assert!(l.rest_sec >= 90, "{} rests {} s", l.exercise, l.rest_sec);
        if hyp_kb::classify(&l.exercise) == ExerciseClass::HeavyCompound {
            assert!(l.rest_sec >= 120, "{} rests {} s", l.exercise, l.rest_sec);
        }
    }
}

#[test]
fn unknown_compound_rests_like_heavy_compound() {
    assert_eq!(
        hyp_kb::classify("Front squat"),
        ExerciseClass::HeavyCompound
    );
    assert_eq!(
        hyp_kb::rest_sec_for(hyp_kb::classify("Front squat"))
            .value
            .0,
        120
    );
    assert_eq!(
        hyp_kb::classify("Leg press"),
        ExerciseClass::ModerateCompound
    );
    assert_eq!(hyp_kb::classify("Hammer curl"), ExerciseClass::Isolation);
}

#[test]
fn hypertrophy_heavy_compound_reps_within_5_to_10_and_isolation_off_the_edge() {
    let anchors = Anchors {
        lift_e1rm: vec![
            ("Barbell back squat".into(), 140.0),
            ("Romanian deadlift".into(), 140.0),
        ],
        ..Default::default()
    };
    let prog = synthesize(&hyp_profile(12), &anchors, 0).unwrap();
    let rxs = lifts(&prog);
    assert!(!rxs.is_empty(), "a hypertrophy week prescribes lifts");
    for (_, l, _) in rxs {
        let top = l.reps_max.unwrap_or(l.reps);
        match hyp_kb::classify(&l.exercise) {
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
    let prog = synthesize(&hyp_profile(12), &Anchors::default(), 0).unwrap();
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
    let ta = crate::individualization::training_age_from_cadence(p.progression_cadence).value;
    assert_eq!(ta, crate::individualization::TrainingAge::Advanced);
    let BlockPosition {
        week_in_block: wib,
        block_weeks: bw,
        ..
    } = lift_block_position(&p, 0);
    let t = hypertrophy_week_targets(&p, ta, wib, bw, 3);
    let get = |m: &str| t.iter().find(|x| x.muscle == m).unwrap().target;
    assert_eq!(get("side delts"), 16, "12 + one +4 step");
    assert_eq!(get("chest"), 8, "chest MV");
    assert_eq!(get("quads"), 6, "quads MV");
    // The plan delivers it: lateral raises carry the extra volume.
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    let raises: u32 = lifts(&prog)
        .iter()
        .filter(|(_, l, _)| l.exercise == "Lateral raise")
        .map(|(_, l, _)| u32::from(l.sets))
        .sum();
    assert_eq!(raises, 16);

    // Not advanced → no specialization: every muscle keeps the stated target
    // (on a 5-day week, where the whole-session cap does not bind).
    p.progression_cadence = ProgressionCadence::WeekToWeek;
    let ta = crate::individualization::training_age_from_cadence(p.progression_cadence).value;
    let t = hypertrophy_week_targets(&p, ta, wib, bw, 5);
    assert_eq!(
        t.iter().find(|x| x.muscle == "side delts").unwrap().target,
        12
    );
    assert_eq!(t.iter().find(|x| x.muscle == "chest").unwrap().target, 12);
}

#[test]
fn default_ramp_tops_out_at_mav_high_and_days_respect_the_session_cap() {
    // HYP-VOL-CAP-001: the stated-10, 3-day full-body week ramps every muscle
    // to the top of its productive range (never its MRV) by week 5, and no day
    // carries more than the whole-session cap.
    let p = hyp_profile(10);
    let ta = crate::individualization::training_age_from_cadence(p.progression_cadence).value;
    let week5 = 4;
    let pos = lift_block_position(&p, week5);
    assert_eq!(
        pos,
        BlockPosition {
            week_in_block: 5,
            block_weeks: 5,
            recovery: false
        },
        "week 5 is the last load week"
    );
    let (wib, bw) = (pos.week_in_block, pos.block_weeks);
    for t in hypertrophy_week_targets(&p, ta, wib, bw, 3) {
        let lm = hyp_kb::landmarks_for(t.muscle).unwrap();
        assert!(
            t.target <= lm.mav.1.max(lm.mev),
            "{} week 5: {} > MAV high {}",
            t.muscle,
            t.target,
            lm.mav.1
        );
    }
    let cap = u32::from(hyp_kb::SESSION_WORKING_SET_CAP);
    let prog = synthesize(&p, &Anchors::default(), week5).unwrap();
    let mut days = 0;
    let mut total = 0;
    for s in &prog.mesocycle.sessions {
        if !matches!(s.session_type, SessionType::Lift(_)) {
            continue;
        }
        let sets: u32 = s
            .prescriptions
            .iter()
            .filter_map(|rx| match &rx.value {
                Prescription::Lift(l) => Some(u32::from(l.sets)),
                _ => None,
            })
            .sum();
        assert!(sets <= cap, "day {}: {sets} sets > {cap}", s.day);
        days += 1;
        total += sets;
    }
    assert_eq!(days, 3);
    assert!(total <= cap * days, "{total} > {cap} × {days}");
    // Week 1 is still exactly the stated target for a big muscle.
    let w1 = lift_block_position(&p, 0).week_in_block;
    let t1 = hypertrophy_week_targets(&p, ta, w1, bw, 3);
    assert_eq!(t1.iter().find(|t| t.muscle == "chest").unwrap().target, 10);
}

#[test]
fn the_specialization_priority_muscle_still_ramps_to_mrv() {
    let lm = hyp_kb::landmarks_for("side delts").unwrap();
    let ramp = hyp_kb::weekly_targets(lm, 12, hyp_kb::VolumeRole::Priority, 5).value;
    assert_eq!(*ramp.last().unwrap(), lm.mrv);
    let std = hyp_kb::weekly_targets(lm, 12, hyp_kb::VolumeRole::Standard, 5).value;
    assert_eq!(*std.last().unwrap(), lm.mav.1);
}
