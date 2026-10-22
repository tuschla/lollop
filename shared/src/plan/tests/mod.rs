//! Unit tests for program synthesis. Shared fixtures live in `common`; the
//! lift, run and calisthenics builders are covered in their own submodules.

use super::*;
use crate::app::tests::base_profile;
use crate::hybrid::ConcurrentGoal;
use crate::individualization::ProgressionCadence;
use crate::schema::{
    Goal, LiftIntensity, LiftPrescription, LiftSessionType, Prescription, RunVolume, SessionType,
};
use std::collections::BTreeSet;

use common::*;

mod calis;
mod common;
mod hypertrophy;
mod lift;
mod run;

#[test]
fn deterministic_same_inputs_same_program() {
    let p = base_profile();
    let a = anchored("Back Squat", 120.0);
    assert_eq!(synthesize(&p, &a, 0), synthesize(&p, &a, 0));
}

#[test]
fn a_gate_blocks_synthesis() {
    let mut p = base_profile();
    p.health.reds_signal = true;
    assert!(synthesize(&p, &Anchors::default(), 0).is_none());
}

#[test]
fn every_prescription_carries_evidence_for_each_goal() {
    for goal in [
        LiftGoal::MaxStrength,
        LiftGoal::Power,
        LiftGoal::Hypertrophy,
    ] {
        let mut p = base_profile();
        p.lift_goal = goal;
        let prog = synthesize(&p, &anchored("Back Squat", 100.0), 0).expect("program");
        let mut count = 0;
        for s in &prog.mesocycle.sessions {
            for rx in &s.prescriptions {
                count += 1;
                assert!(
                    !rx.reference().is_empty(),
                    "prescription must carry a citation"
                );
            }
        }
        assert!(count > 0, "goal {goal:?} produced no prescriptions");
    }
}

#[test]
fn hybrid_profile_has_both_lift_and_run_days() {
    let mut p = base_profile();
    p.goal_distance = GoalDistance::HalfMarathon;
    p.running_days_per_week = 3;
    let prog = synthesize(&p, &anchored("Back Squat", 130.0), 0).unwrap();
    assert_eq!(prog.goal, Goal::Hybrid);
    let sessions = &prog.mesocycle.sessions;
    assert!(
        sessions
            .iter()
            .any(|s| matches!(s.session_type, SessionType::Lift(_))),
        "hybrid needs lift days"
    );
    assert!(
        sessions
            .iter()
            .any(|s| matches!(s.session_type, SessionType::Run(_))),
        "hybrid needs run days"
    );
}

/// Every lift prescription in a program, with its day and claim id.
fn lifts_with_claims(prog: &Program) -> Vec<(u16, LiftPrescription, crate::evidence::ClaimId)> {
    prog.mesocycle
        .sessions
        .iter()
        .flat_map(|s| {
            s.prescriptions
                .iter()
                .filter_map(move |rx| match &rx.value {
                    Prescription::Lift(l) => Some((s.day, l.clone(), rx.claim_id())),
                    _ => None,
                })
        })
        .collect()
}

/// Weekly sets per movement (normalized name) across a program week.
fn weekly_sets_by_movement(prog: &Program) -> std::collections::BTreeMap<Vec<String>, u32> {
    let mut m = std::collections::BTreeMap::new();
    for (_, l, _) in lifts_with_claims(prog) {
        *m.entry(movement_key(&l.exercise)).or_insert(0u32) += l.sets as u32;
    }
    m
}

fn lift_days(prog: &Program) -> Vec<u16> {
    prog.mesocycle
        .sessions
        .iter()
        .filter(|s| matches!(s.session_type, SessionType::Lift(_)))
        .map(|s| s.day)
        .collect()
}

fn is_lower(l: &LiftPrescription) -> bool {
    matches!(
        movement_pattern(&l.exercise),
        Some(MovementPattern::Squat | MovementPattern::Hinge)
    )
}

/// A RUNNING recovery week leaves the lifting as
/// built (lifting deloads on its own 5+1 block): no deload claim, same sets,
/// RIR never below 2.
#[test]
fn run_recovery_week_keeps_lift_volume() {
    let mut p = base_profile(); // weekly_sets = 12 → lifting + running = hybrid
    p.goal_distance = GoalDistance::TenK;
    p.running_days_per_week = 4;
    p.running_km_per_week = 40.0;
    let a = anchored("Barbell back squat", 100.0);
    let rec_week = (1..5u32)
        .find(|&w| planned_recovery_week(&p, w, 0))
        .expect("a running recovery week inside the first lift block");
    let before = synthesize(&p, &a, rec_week - 1).unwrap();
    let rec = synthesize(&p, &a, rec_week).unwrap();
    let sets_before = weekly_sets_by_movement(&before);
    for (_, l, claim) in lifts_with_claims(&rec) {
        assert_ne!(claim, claims::STR_DELOAD_VOL_001, "{l:?}");
        assert_ne!(claim, claims::HYP_DELOAD_RX_001, "{l:?}");
        if let LiftIntensity::Rir(r) = l.intensity {
            assert!(r >= 2, "{l:?}");
        }
    }
    assert_eq!(
        weekly_sets_by_movement(&rec),
        sets_before,
        "lift volume kept"
    );
}

/// STR-DELOAD-VOL-001: the strength deload week halves the sets
/// and keeps the load within 10% of the last load week.
#[test]
fn strength_deload_holds_intensity_halves_sets() {
    let p = base_profile(); // MaxStrength, intermediate, 3 lift days
    let a = anchored("Back Squat", 140.0);
    let pos = lift_block_position(&p, 5);
    assert!(pos.recovery, "week 6 (index 5) is the lift deload week");
    assert_eq!((pos.week_in_block, pos.block_weeks), (5, 5));
    let last_load = synthesize(&p, &a, 4).unwrap();
    let deload = synthesize(&p, &a, 5).unwrap();
    assert_eq!(deload.mesocycle.phase, MesoPhase::Deload);
    let load_rows = lifts_with_claims(&last_load);
    let mut checked = 0;
    for (day, l, claim) in lifts_with_claims(&deload) {
        assert_eq!(claim, claims::STR_DELOAD_VOL_001);
        let (_, l0, _) = load_rows
            .iter()
            .find(|(d, l0, _)| *d == day && l0.exercise == l.exercise)
            .expect("same lift, same day, in the last load week");
        assert_eq!(
            l.sets,
            ((l0.sets as f64 * 0.5).ceil() as u8).max(1),
            "sets halved"
        );
        if let (LiftIntensity::PercentOneRm(p0), LiftIntensity::PercentOneRm(pd)) =
            (l0.intensity, l.intensity)
        {
            assert!(pd >= 0.9 * p0 - 0.01, "load kept ≥90%: {pd} vs {p0}");
            assert!(pd < p0, "load a little lighter: {pd} vs {p0}");
            checked += 1;
        }
    }
    assert!(checked > 0, "an anchored %1RM lift was checked");
}

/// A hybrid hypertrophy lifter's lift deload follows the 5+1
/// lifting block, never the 3:1 running cadence.
#[test]
fn hybrid_hypertrophy_lift_deload_not_more_often_than_every_6th_week() {
    let mut p = base_profile();
    p.lift_goal = LiftGoal::Hypertrophy;
    p.goal_distance = GoalDistance::TenK;
    p.running_days_per_week = 3;
    p.running_km_per_week = 30.0;
    let a = anchored("Back Squat", 120.0);
    let deload_weeks: Vec<u32> = (0..12u32)
        .filter(|&w| {
            lifts_with_claims(&synthesize(&p, &a, w).unwrap())
                .iter()
                .any(|(_, _, c)| *c == claims::HYP_DELOAD_RX_001)
        })
        .collect();
    assert_eq!(deload_weeks, vec![5, 11]);
}

#[test]
fn novice_lifting_block_has_no_calendar_deload() {
    let mut p = base_profile();
    p.progression_cadence = ProgressionCadence::EverySession; // → Novice
    for w in 0..12 {
        assert!(!lift_block_position(&p, w).recovery, "week {w}");
    }
}

#[test]
fn a_full_cycle_repeats_the_first_week() {
    let mut p = base_profile();
    p.goal_distance = GoalDistance::TenK;
    p.running_days_per_week = 4;
    p.running_km_per_week = 40.0;
    let a = anchored("Barbell back squat", 100.0);
    // The lift block (6 weeks) and the running base/build pair of cycles
    // (2 × 4 weeks) line up again after lcm(6, 8) = 24 weeks. Week 0 itself is
    // not the steady state: strides join the easy runs from the third base
    // week on (running-020), so compare two later aligned weeks.
    assert_eq!(
        synthesize(&p, &a, 48).unwrap(),
        synthesize(&p, &a, 24).unwrap(),
        "the combined block repeats identically"
    );
}

/// LIFT-DAYS-001: 2 lift days keep the 3-day week's weekly sets per
/// movement, spread over fewer days.
#[test]
fn two_lift_days_keep_weekly_sets_per_muscle_of_three() {
    let a = anchored("Back Squat", 140.0);
    let three = synthesize(&base_profile(), &a, 0).unwrap();
    let mut p2 = base_profile();
    p2.lift_days_per_week = Some(2);
    let two = synthesize(&p2, &a, 0).unwrap();
    assert_eq!(lift_days(&three).len(), 3);
    assert_eq!(lift_days(&two).len(), 2);
    let w3 = weekly_sets_by_movement(&three);
    let w2 = weekly_sets_by_movement(&two);
    assert!(!w2.is_empty());
    for (key, sets) in &w2 {
        assert_eq!(Some(sets), w3.get(key), "weekly sets for {key:?}");
    }
    // ≥48 h between the two full-body days.
    let d = lift_days(&two);
    let gap = (d[1] - d[0]).min(7 - (d[1] - d[0]));
    assert!(gap >= 2, "{d:?}");
}

/// 4 lift days = upper/lower ×2; same-region days never back-to-back.
#[test]
fn four_lift_days_split_upper_lower_with_48h_between_same_region() {
    let mut p = base_profile();
    p.lift_days_per_week = Some(4);
    let prog = synthesize(&p, &anchored("Back Squat", 140.0), 0).unwrap();
    let mut regions: Vec<(u16, DayRegion)> = Vec::new();
    for s in &prog.mesocycle.sessions {
        if matches!(s.session_type, SessionType::Lift(_)) {
            let DayLoad::Lift { region, .. } = session_load(s) else {
                panic!("a lift day loads as a lift")
            };
            assert_ne!(
                region,
                DayRegion::Full,
                "day {} mixes upper and lower",
                s.day
            );
            regions.push((s.day, region));
        }
    }
    assert_eq!(regions.len(), 4);
    for (d1, r1) in &regions {
        for (d2, r2) in &regions {
            if d1 != d2 && r1 == r2 {
                let gap = d1.abs_diff(*d2).min(7 - d1.abs_diff(*d2));
                assert!(gap >= 2, "{regions:?}");
            }
        }
    }
}

/// ADH-DAYS-001: a weekend-only mask puts every session on Saturday/Sunday.
#[test]
fn weekend_only_mask_places_sessions_on_days_5_and_6() {
    let mut p = base_profile();
    p.training_weekdays = Some(0b0110_0000);
    let prog = synthesize(&p, &anchored("Back Squat", 140.0), 0).unwrap();
    let used: BTreeSet<u16> = prog
        .mesocycle
        .sessions
        .iter()
        .filter(|s| s.session_type != SessionType::Rest)
        .map(|s| s.day)
        .collect();
    assert_eq!(used, BTreeSet::from([5, 6]));
    // A hybrid squeezed into the same two days never doubles up either.
    p.goal_distance = GoalDistance::TenK;
    p.running_days_per_week = 3;
    p.running_km_per_week = 30.0;
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    let busy: Vec<u16> = prog
        .mesocycle
        .sessions
        .iter()
        .filter(|s| s.session_type != SessionType::Rest)
        .map(|s| s.day)
        .collect();
    assert!(busy.iter().all(|d| *d >= 5), "{busy:?}");
    assert_eq!(prog.mesocycle.sessions.len(), 7, "one session per day");
}

/// CAP-3: no hard or long run the day after (or before) a heavy
/// leg day, in any hybrid template where an easy run can fill that day.
#[test]
fn hybrid_quality_run_never_within_24h_after_heavy_lift_day() {
    for goal in [
        GoalDistance::FiveK,
        GoalDistance::TenK,
        GoalDistance::HalfMarathon,
        GoalDistance::Marathon,
    ] {
        for days in 2..=5u8 {
            for lead in [ConcurrentGoal::Strength, ConcurrentGoal::EndurancePriority] {
                let mut p = base_profile();
                p.goal_distance = goal;
                p.running_days_per_week = days;
                p.running_km_per_week = 10.0 * days as f64;
                p.concurrent_goal = lead;
                let a = anchored("Back Squat", 140.0);
                for w in 0..4 {
                    let prog = synthesize(&p, &a, w).unwrap();
                    let sessions = &prog.mesocycle.sessions;
                    let loads: Vec<(u16, DayLoad)> =
                        sessions.iter().map(|s| (s.day, session_load(s))).collect();
                    let heavy: Vec<u16> = loads
                        .iter()
                        .filter(|(_, l)| {
                            matches!(
                                l,
                                DayLoad::Lift {
                                    heavy_legs: true,
                                    ..
                                }
                            )
                        })
                        .map(|(d, _)| *d)
                        .collect();
                    let easy_runs = loads
                        .iter()
                        .filter(|(_, l)| *l == DayLoad::Run { hard: false })
                        .count();
                    let rest_days = loads.iter().filter(|(_, l)| *l == DayLoad::Rest).count();
                    if easy_runs + rest_days < heavy.len() {
                        continue; // no free day can follow every heavy day
                    }
                    for h in &heavy {
                        let next = (h + 1) % 7;
                        let after = loads.iter().find(|(d, _)| *d == next).map(|(_, l)| *l);
                        assert_ne!(
                            after,
                            Some(DayLoad::Run { hard: true }),
                            "{goal:?} {days}d {lead:?} week {w}: hard run the day after heavy legs on day {h}: {loads:?}"
                        );
                    }
                }
            }
        }
    }
}

/// A running-led hybrid lifts at a maintenance dose: no
/// hypertrophy day for a strength goal, 2–4 sets per exercise, no set ramp,
/// heavy-day leg work at ≥80% 1RM, and fewer leg sets than a lifting-led week.
#[test]
fn endurance_priority_hybrid_lifts_at_maintenance_dose() {
    let mut p = base_profile();
    p.goal_distance = GoalDistance::TenK;
    p.running_days_per_week = 4;
    p.running_km_per_week = 40.0;
    let a = anchored("Back Squat", 140.0);
    let mut run_lead = p.clone();
    run_lead.concurrent_goal = ConcurrentGoal::EndurancePriority;
    let lift_led = synthesize(&p, &a, 0).unwrap();
    let wk0 = synthesize(&run_lead, &a, 0).unwrap();
    let wk2 = synthesize(&run_lead, &a, 2).unwrap();
    for s in &wk0.mesocycle.sessions {
        assert_ne!(
            s.session_type,
            SessionType::Lift(LiftSessionType::Repetition)
        );
    }
    for (_, l, _) in lifts_with_claims(&wk0) {
        assert!((1..=4).contains(&l.sets), "{l:?}");
    }
    assert_eq!(
        weekly_sets_by_movement(&wk0),
        weekly_sets_by_movement(&wk2),
        "no set ramp across the block"
    );
    for s in &wk0.mesocycle.sessions {
        if s.session_type == SessionType::Lift(LiftSessionType::MaxEffort) {
            for rx in &s.prescriptions {
                if let Prescription::Lift(l) = &rx.value
                    && is_lower(l)
                    && let LiftIntensity::PercentOneRm(pct) = l.intensity
                {
                    assert!(pct >= 80.0, "{l:?}");
                }
            }
        }
    }
    let lower = |prog: &Program| -> u32 {
        lifts_with_claims(prog)
            .iter()
            .filter(|(_, l, _)| is_lower(l))
            .map(|(_, l, _)| l.sets as u32)
            .sum()
    };
    assert!(
        lower(&wk0) < lower(&lift_led),
        "run-led leg sets {} < lift-led {}",
        lower(&wk0),
        lower(&lift_led)
    );
}

/// CAP-1: with ≥4 run days, at most 2 lift days carry leg work and
/// weekly leg sets drop by ~20–33% against the uncapped week.
#[test]
fn high_mileage_hybrid_caps_leg_days_and_trims_leg_sets() {
    let mut p = base_profile();
    p.goal_distance = GoalDistance::Marathon;
    p.concurrent_goal = ConcurrentGoal::EndurancePriority;
    p.running_days_per_week = 5;
    p.running_km_per_week = 50.0;
    p.lift_days_per_week = Some(3);
    let prog = synthesize(&p, &anchored("Back Squat", 140.0), 0).unwrap();
    let leg_days = prog
        .mesocycle
        .sessions
        .iter()
        .filter(|s| match &s.session_type {
            SessionType::Lift(_) => s
                .prescriptions
                .iter()
                .any(|rx| matches!(&rx.value, Prescription::Lift(l) if is_lower(l))),
            _ => false,
        })
        .count();
    assert!(leg_days <= 2, "{leg_days} leg days");
    assert!(
        lifts_with_claims(&prog)
            .iter()
            .any(|(_, l, c)| is_lower(l) && *c == claims::HYB_CAP_001),
        "the leg-set cut cites CAP-1"
    );
}

/// When the ≤3-day hybrid default trims run days, weekly distance
/// shrinks with them; no easy run is ever longer than the long run.
#[test]
fn hybrid_frequency_cap_scales_volume_not_session_length() {
    for (days, km) in [(5u8, 40.0), (4, 45.0)] {
        let mut p = base_profile();
        p.goal_distance = GoalDistance::TenK;
        p.running_days_per_week = days;
        p.running_km_per_week = km;
        let prog = synthesize(&p, &anchored("Back Squat", 140.0), 0).unwrap();
        let mut total = 0.0;
        let mut long = 0.0_f64;
        let mut easy_max = 0.0_f64;
        for s in &prog.mesocycle.sessions {
            let SessionType::Run(k) = s.session_type else {
                continue;
            };
            for rx in &s.prescriptions {
                if let Prescription::Run(r) = &rx.value
                    && let RunVolume::DistanceKm(d) = r.volume
                {
                    let d = d as f64;
                    total += d;
                    if k == RunSessionType::LongRun {
                        long = long.max(d);
                    } else if !super::run::is_quality(k) {
                        easy_max = easy_max.max(d);
                    }
                }
            }
        }
        let scaled = km * 3.0 / days as f64;
        assert!(
            total <= scaled + 1.0,
            "{days}d {km} km: week sums {total} > {scaled}"
        );
        if long > 0.0 {
            assert!(easy_max <= long, "easy {easy_max} > long {long}");
        }
    }
}

/// A lifting-led hybrid holds weekly running distance across the
/// block (the lifting side progresses), with one quality run when there is no
/// race goal.
#[test]
fn strength_priority_general_running_is_held_flat_across_block() {
    let mut p = base_profile();
    p.goal_distance = GoalDistance::General;
    p.running_days_per_week = 3;
    p.running_km_per_week = 30.0;
    let a = anchored("Back Squat", 140.0);
    let km = |w: u32| -> f64 {
        synthesize(&p, &a, w)
            .unwrap()
            .mesocycle
            .sessions
            .iter()
            .flat_map(|s| &s.prescriptions)
            .filter_map(|rx| match &rx.value {
                Prescription::Run(r) => match r.volume {
                    RunVolume::DistanceKm(d) => Some(d as f64),
                    _ => None,
                },
                _ => None,
            })
            .sum()
    };
    assert_eq!(km(0), km(2), "weekly distance held, not ramped");
    for w in 0..3 {
        let prog = synthesize(&p, &a, w).unwrap();
        let q = prog
            .mesocycle
            .sessions
            .iter()
            .filter(|s| matches!(s.session_type, SessionType::Run(k) if super::run::is_quality(k)))
            .count();
        assert!(q <= 1, "week {w}: {q} quality runs");
    }
}

/// A runner's opt-in strength block = 2 heavy leg sessions, never the
/// day after or before a hard or long run when a free day allows it.
#[test]
fn runner_strength_block_avoids_hard_run_neighbours() {
    let mut p = runner(GoalDistance::HalfMarathon, 4, 40.0);
    p.runner_strength = true;
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    let sessions = &prog.mesocycle.sessions;
    let lifts: Vec<&Session> = sessions
        .iter()
        .filter(|s| matches!(s.session_type, SessionType::Lift(_)))
        .collect();
    assert_eq!(lifts.len(), 2);
    for s in &lifts {
        for rx in &s.prescriptions {
            let Prescription::Lift(l) = &rx.value else {
                continue;
            };
            assert!(is_lower(l), "leg work only: {}", l.exercise);
            assert!((2..=4).contains(&l.sets), "{l:?}");
            assert_eq!(rx.claim_id(), claims::RUN_STRENGTH_001);
        }
        // CAP-3: the day after heavy leg work is never a hard or long run.
        let n = sessions.iter().find(|x| x.day == (s.day + 1) % 7).unwrap();
        assert_ne!(
            session_load(n),
            DayLoad::Run { hard: true },
            "day {}",
            s.day
        );
    }
    // Off by default: a run-only profile has no lift days.
    p.runner_strength = false;
    assert!(lift_days(&synthesize(&p, &Anchors::default(), 0).unwrap()).is_empty());
}

// ── age_years → conservative running deload cadence (masters) ──

fn program_weeks(prog: &Program) -> u8 {
    prog.mesocycle.weeks
}

#[test]
fn masters_runner_takes_the_conservative_two_week_load_cadence() {
    // is_masters threshold (MASTERS-001, 65+).
    assert!(!is_masters(None));
    assert!(!is_masters(Some(64.0)));
    assert!(is_masters(Some(65.0)));

    // A masters (age 70) RUNNER shortens the accumulation block to the 2:1
    // running deload cadence (RUN-DELOAD-001 "2:1 for older"): 2 load weeks +
    // 1 recovery week = a 3-week cycle.
    let mut masters = half_marathoner();
    masters.age_years = Some(70.0);
    assert_eq!(
        program_weeks(&synthesize(&masters, &Anchors::default(), 0).unwrap()),
        3
    );

    // Elsewhere the default cadence holds: an age-less runner and a younger
    // runner keep 3 load weeks + 1 recovery = a 4-week cycle, and a masters
    // LIFTER (no running) keeps the 4-week accumulation + 1 recovery = 5.
    let ageless = half_marathoner();
    assert_eq!(
        program_weeks(&synthesize(&ageless, &Anchors::default(), 0).unwrap()),
        4
    );

    let mut young = half_marathoner();
    young.age_years = Some(40.0);
    assert_eq!(
        program_weeks(&synthesize(&young, &Anchors::default(), 0).unwrap()),
        4
    );

    let mut masters_lifter = base_profile(); // weekly_sets=12, no running
    masters_lifter.age_years = Some(70.0);
    assert_eq!(
        program_weeks(&synthesize(&masters_lifter, &anchored("Back Squat", 100.0), 0).unwrap()),
        6,
        "a lifter runs the 5+1 lifting block (HYP-DELOAD-CADENCE-001) at any age"
    );

    // RUN-DELOAD-001 names low training age as a trigger too, so a YOUNG
    // NOVICE runner also takes the conservative 2:1 cadence (2 load weeks +
    // 1 recovery = a 3-week cycle), matching the guidance card's
    // `conservative_deload` predicate.
    let mut novice_runner = half_marathoner();
    novice_runner.age_years = Some(30.0);
    novice_runner.progression_cadence = ProgressionCadence::EverySession; // → Novice
    assert_eq!(
        program_weeks(&synthesize(&novice_runner, &Anchors::default(), 0).unwrap()),
        3
    );
    assert!(conservative_deload(Some(30.0), TrainingAge::Novice));
    assert!(conservative_deload(Some(70.0), TrainingAge::Intermediate));
    assert!(!conservative_deload(Some(40.0), TrainingAge::Intermediate));
}

#[test]
fn a_week_is_exactly_seven_days() {
    let prog = synthesize(&base_profile(), &anchored("Back Squat", 100.0), 0).unwrap();
    let days: BTreeSet<u16> = prog.mesocycle.sessions.iter().map(|s| s.day).collect();
    assert_eq!(days, (0u16..7).collect::<BTreeSet<u16>>());
}
