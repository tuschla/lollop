//! Calisthenics ladder-day synthesis tests.

use super::*;
use crate::app::{CalisEquipment, ResistanceModality};
use crate::schema::{CalisthenicsPrescription, LiftSessionType, TierTarget};

fn calis_profile() -> Profile {
    Profile {
        resistance_modality: ResistanceModality::Calisthenics,
        weekly_sets: 10,
        running_days_per_week: 0,
        ..base_profile()
    }
}

fn anchor(ladder: LadderId, tier: u8) -> CalisAnchor {
    CalisAnchor {
        ladder,
        tier,
        advance_ready: false,
        advance_steps: 1,
        regress: false,
        bridged: false,
        last_added_kg: 0.0,
        active: true,
        needs_effort_rating: false,
        reentry_steps: 0,
        retest: false,
        placed: false,
        last_loaded_set: None,
    }
}

fn with(anchors: Vec<CalisAnchor>) -> Anchors {
    Anchors {
        calisthenics: anchors,
        ..Default::default()
    }
}

fn lift_days(prog: &Program) -> Vec<&Session> {
    prog.mesocycle
        .sessions
        .iter()
        .filter(|s| matches!(s.session_type, SessionType::Lift(_)))
        .collect()
}

fn calis(rx: &Recommended<Prescription>) -> &CalisthenicsPrescription {
    match &rx.value {
        Prescription::Calisthenics(c) => c,
        other => panic!("expected a calisthenics item, got {other:?}"),
    }
}

/// The first prescribed item on `ladder` anywhere in the week.
fn item(prog: &Program, ladder: LadderId) -> Option<&Recommended<Prescription>> {
    lift_days(prog)
        .into_iter()
        .flat_map(|s| s.prescriptions.iter())
        .find(|p| calis(p).ladder == ladder)
}

fn claim(rx: &Recommended<Prescription>) -> crate::evidence::ClaimId {
    rx.claim_id()
}

#[test]
fn calisthenics_profile_synthesizes_pattern_balanced_ladder_days() {
    let prog = synthesize(&calis_profile(), &Anchors::default(), 0).expect("a plan is synthesized");
    let days = lift_days(&prog);
    assert_eq!(days.len(), 3, "pure calisthenics = 3 full-body lift days");
    for s in &days {
        assert_eq!(
            s.session_type,
            SessionType::Lift(LiftSessionType::Calisthenics)
        );
    }
    // Day 0: no logged history → no skill items; every plane of push and pull,
    // squat, hinge and core, all at the easiest rung and flagged unplaced.
    let day0 = days.iter().find(|s| s.day == 0).expect("day-0 session");
    let ladders: Vec<LadderId> = day0.prescriptions.iter().map(|p| calis(p).ladder).collect();
    assert_eq!(
        ladders,
        vec![
            LadderId::HorizontalPush,
            LadderId::Dip,
            LadderId::VerticalPull,
            LadderId::HorizontalPull,
            LadderId::Squat,
            LadderId::Hinge,
            LadderId::Core,
        ],
        "day-0 pattern order"
    );
    for p in &day0.prescriptions {
        let c = calis(p);
        assert_eq!(c.tier, 0, "every pattern starts at rung 0 with no history");
        assert!(c.unplaced, "an unplaced ladder invites placement");
    }
}

/// Count the week's items per movement family.
fn family_counts(prog: &Program) -> [usize; 7] {
    let mut n = [0usize; 7];
    for s in lift_days(prog) {
        for p in &s.prescriptions {
            let idx = match calis(p).ladder {
                LadderId::HorizontalPush => 0,
                LadderId::Dip | LadderId::VerticalPush => 1,
                LadderId::VerticalPull => 2,
                LadderId::HorizontalPull => 3,
                LadderId::Squat => 4,
                LadderId::Hinge => 5,
                LadderId::Core => 6,
                _ => continue,
            };
            n[idx] += 1;
        }
    }
    n
}

#[test]
fn calisthenics_every_pattern_at_least_twice_weekly() {
    // CAL-FREQ-VOL-002: every pattern ≥2×/wk, on the 3-day plan AND the 2-day
    // hybrid plan.
    let hybrid = Profile {
        running_days_per_week: 3,
        running_km_per_week: 20.0,
        ..calis_profile()
    };
    for p in [calis_profile(), hybrid] {
        let prog = synthesize(&p, &Anchors::default(), 0).expect("plan");
        let counts = family_counts(&prog);
        assert!(
            counts.iter().all(|&c| c >= 2),
            "every family ≥2×/wk (push-h, push-v, pull-v, pull-h, squat, hinge, core): {counts:?}"
        );
    }
}

#[test]
fn hybrid_calisthenics_includes_vertical_push() {
    let hybrid = Profile {
        running_days_per_week: 3,
        running_km_per_week: 20.0,
        ..calis_profile()
    };
    let prog = synthesize(&hybrid, &Anchors::default(), 0).expect("plan");
    assert!(
        item(&prog, LadderId::VerticalPush).is_some(),
        "vertical push on a 2-day plan"
    );
    assert!(
        item(&prog, LadderId::Dip).is_some(),
        "dips alternate with it"
    );
}

#[test]
fn no_pull_bar_profile_never_prescribes_vertical_pull() {
    let none = Profile {
        calis_equipment: Some(CalisEquipment::default()),
        ..calis_profile()
    };
    // Even with a logged pull-up rung and an active front-lever track.
    let anchors = with(vec![
        anchor(LadderId::VerticalPull, 4),
        anchor(LadderId::FrontLever, 0),
        anchor(LadderId::HorizontalPull, 3),
        anchor(LadderId::Dip, 3),
    ]);
    let prog = synthesize(&none, &anchors, 0).expect("plan");
    for s in lift_days(&prog) {
        for p in &s.prescriptions {
            let c = calis(p);
            assert!(
                crate::calisthenics::rung_available(
                    c.ladder,
                    c.tier,
                    crate::calisthenics::Kit {
                        pull_up_bar: false,
                        dip_bars: false,
                        rings: false,
                    }
                ),
                "{} needs equipment the athlete lacks",
                c.exercise
            );
        }
    }
    assert!(item(&prog, LadderId::VerticalPull).is_none());
    assert!(item(&prog, LadderId::FrontLever).is_none());
    assert!(
        item(&prog, LadderId::Dip).is_none(),
        "vertical push takes the dip slot"
    );
    // The row carries the vertical-pull volume (CAL-EQUIP-001).
    let full = synthesize(&calis_profile(), &anchors, 0).expect("plan");
    let rows = calis(item(&prog, LadderId::HorizontalPull).unwrap()).sets;
    let rows_full = calis(item(&full, LadderId::HorizontalPull).unwrap()).sets;
    assert!(rows >= rows_full, "rows {rows} ≥ {rows_full}");
    // Ring row (rung 4) needs rings: the advance stops at the feet-elevated row.
    let mut ready = anchor(LadderId::HorizontalPull, 3);
    ready.advance_ready = true;
    let prog = synthesize(&none, &with(vec![ready]), 0).expect("plan");
    assert_eq!(
        calis(item(&prog, LadderId::HorizontalPull).unwrap()).tier,
        3
    );
}

#[test]
fn hypertrophy_goal_gets_6_to_15_reps_at_0_to_1_rir() {
    let hyp = Profile {
        lift_goal: LiftGoal::Hypertrophy,
        ..calis_profile()
    };
    let anchors = with(vec![anchor(LadderId::HorizontalPush, 4)]);
    let prog = synthesize(&hyp, &anchors, 0).expect("plan");
    let push = item(&prog, LadderId::HorizontalPush).unwrap();
    let c = calis(push);
    assert_eq!(c.exercise, "Full push-up");
    assert_eq!(c.target, TierTarget::Reps { low: 6, high: 12 });
    assert_eq!(c.rir, (0, 1));
    assert_eq!(claim(push), claims::CAL_GOAL_RIR_001);
    for s in lift_days(&prog) {
        for p in &s.prescriptions {
            let c = calis(p);
            let t = crate::calisthenics::tier(c.ladder, c.tier).unwrap();
            if let TierTarget::Reps { low, high } = c.target
                && t.failure == crate::calisthenics::FailurePolicy::SelfLimiting
            {
                assert!(
                    (6..=15).contains(&low) && (6..=15).contains(&high),
                    "{}",
                    c.exercise
                );
                assert_eq!(c.rir, (0, 1), "{}", c.exercise);
            }
        }
    }
    // The strength goal keeps the table band and RIR 1–2.
    let prog = synthesize(&calis_profile(), &anchors, 0).expect("plan");
    let c = calis(item(&prog, LadderId::HorizontalPush).unwrap());
    assert_eq!(c.target, TierTarget::Reps { low: 5, high: 8 });
    assert_eq!(c.rir, (1, 2));
}

#[test]
fn weighted_rung_first_prescription_suggests_smallest_plate() {
    let p = Profile {
        bodyweight_kg: Some(80.0),
        ..calis_profile()
    };
    // Full pull-up qualified unloaded → the fork resolves to load: same rung,
    // first load step (2.5% of 80 kg → 2.5 kg on the 1.25 kg grid).
    let mut pull = anchor(LadderId::VerticalPull, 4);
    pull.advance_ready = true;
    // Ring dip qualified → the next rung IS the weighted dip: it starts loaded.
    let mut dip = anchor(LadderId::Dip, 4);
    dip.advance_ready = true;
    let prog = synthesize(&p, &with(vec![pull, dip]), 0).expect("plan");
    let c = calis(item(&prog, LadderId::VerticalPull).unwrap());
    assert_eq!(c.exercise, "Full pull-up");
    assert_eq!(c.added_kg, Some(2.5));
    assert!(c.load_capable);
    let d = calis(item(&prog, LadderId::Dip).unwrap());
    assert_eq!(d.exercise, "Weighted dip");
    assert_eq!(d.added_kg, Some(2.5));
    assert!(d.load_capable);
    // A bodyweight rung is not load-capable.
    let prog = synthesize(&p, &with(vec![anchor(LadderId::HorizontalPush, 4)]), 0).unwrap();
    let h = calis(item(&prog, LadderId::HorizontalPush).unwrap());
    assert!(!h.load_capable);
    assert_eq!(h.added_kg, None);
}

#[test]
fn loaded_pullup_trigger_raises_added_kg_not_rung() {
    let p = Profile {
        bodyweight_kg: Some(80.0),
        ..calis_profile()
    };
    let mut pull = anchor(LadderId::VerticalPull, 4);
    pull.advance_ready = true;
    pull.last_added_kg = 20.0;
    let prog = synthesize(&p, &with(vec![pull]), 0).expect("plan");
    let rx = item(&prog, LadderId::VerticalPull).unwrap();
    let c = calis(rx);
    assert_eq!(
        c.exercise, "Full pull-up",
        "the belt load never carries into L-sit pull-ups"
    );
    assert_eq!(c.added_kg, Some(22.5), "+2.5% of the 100 kg system load");
    assert_eq!(c.rir, (1, 2));
    assert_eq!(claim(rx), claims::CAL_EFAIL_SAFE_001);
    assert!(rx.safety_critical());
    assert_eq!(
        c.harder,
        Some(5),
        "the leverage branch stays one Harder choice away"
    );
    // A missed floor while loaded sheds load, not the rung.
    let mut heavy = anchor(LadderId::VerticalPull, 4);
    heavy.regress = true;
    heavy.last_added_kg = 20.0;
    let prog = synthesize(&p, &with(vec![heavy]), 0).expect("plan");
    let c = calis(item(&prog, LadderId::VerticalPull).unwrap());
    assert_eq!(c.tier, 4);
    assert_eq!(c.added_kg, Some(17.5));
}

#[test]
fn failed_new_rung_prescribes_tempo_bridge_capped_at_8s() {
    let mut a = anchor(LadderId::HorizontalPush, 5);
    a.regress = true;
    let prog = synthesize(&calis_profile(), &with(vec![a]), 0).expect("plan");
    let rx = item(&prog, LadderId::HorizontalPush).unwrap();
    let c = calis(rx);
    assert_eq!(c.exercise, "Full push-up");
    let b = c.tempo_bridge.expect("a tempo bridge on the easier rung");
    assert!(b.eccentric_sec + b.pause_sec < 8, "≤8 s per rep");
    assert_eq!(claim(rx), claims::CAL_PROG_TEMPO_001);
    // The bridge persists while the rung was entered by a regress.
    let mut held = anchor(LadderId::HorizontalPush, 4);
    held.bridged = true;
    let prog = synthesize(&calis_profile(), &with(vec![held]), 0).expect("plan");
    assert!(
        calis(item(&prog, LadderId::HorizontalPush).unwrap())
            .tempo_bridge
            .is_some()
    );
    // A leave-RIR eccentric rung is its own bridge: no tempo on it.
    let mut pull = anchor(LadderId::VerticalPull, 4);
    pull.regress = true;
    let prog = synthesize(&calis_profile(), &with(vec![pull]), 0).expect("plan");
    let c = calis(item(&prog, LadderId::VerticalPull).unwrap());
    assert_eq!(c.exercise, "Negative pull-up");
    assert!(c.tempo_bridge.is_none());
}

#[test]
fn skip_and_reentry_shape_the_rung_and_its_evidence() {
    let mut skip = anchor(LadderId::HorizontalPush, 0);
    skip.advance_ready = true;
    skip.advance_steps = crate::calisthenics::SKIP_STEPS;
    let mut back = anchor(LadderId::Squat, 4);
    back.reentry_steps = 1;
    let prog = synthesize(&calis_profile(), &with(vec![skip, back]), 0).expect("plan");
    let push = item(&prog, LadderId::HorizontalPush).unwrap();
    assert_eq!(calis(push).tier, 2, "a far-too-easy rung skips two steps");
    assert_eq!(claim(push), claims::CAL_PLACE_001);
    let sq = item(&prog, LadderId::Squat).unwrap();
    assert_eq!(calis(sq).tier, 3, "a 2–8 week break steps down one rung");
    assert_eq!(calis(sq).reentry_steps, 1);
    assert_eq!(claim(sq), claims::CAL_REENTRY_001);
}

#[test]
fn lever_prescription_prescribes_working_holds_with_a_separate_test() {
    let prog = synthesize(
        &calis_profile(),
        &with(vec![anchor(LadderId::FrontLever, 0)]),
        0,
    )
    .expect("plan");
    let c = calis(item(&prog, LadderId::FrontLever).unwrap());
    assert_eq!(c.target, TierTarget::HoldSec { low: 5, high: 10 });
    assert_eq!(c.test_hold_sec, Some(15));
    assert_eq!(c.sets, 4);
}

#[test]
fn calisthenics_power_day_leads_with_ballistic_items_capped_contacts() {
    let power = Profile {
        lift_goal: LiftGoal::Power,
        ..calis_profile()
    };
    let prog = synthesize(&power, &Anchors::default(), 0).expect("plan");
    for s in lift_days(&prog) {
        let first: Vec<LadderId> = s
            .prescriptions
            .iter()
            .take(2)
            .map(|p| calis(p).ladder)
            .collect();
        assert_eq!(
            first,
            vec![LadderId::Jump, LadderId::PlyoPush],
            "ballistic work first"
        );
        for p in s.prescriptions.iter().take(2) {
            let c = calis(p);
            assert!(c.ballistic);
            assert_eq!(c.rest_sec, crate::calisthenics::BALLISTIC_REST_SEC);
            assert_eq!(claim(p), claims::CAL_POWER_001);
            let t = crate::calisthenics::tier(c.ladder, c.tier).unwrap();
            let contacts = c.sets as u32 * c.target.top() as u32 * if t.unilateral { 2 } else { 1 };
            assert!(contacts <= crate::calisthenics::PLYO_CONTACT_CAP);
        }
    }
}

#[test]
fn calisthenics_power_title_is_power() {
    let power = Profile {
        lift_goal: LiftGoal::Power,
        ..calis_profile()
    };
    let prog = synthesize(&power, &Anchors::default(), 0).expect("plan");
    assert_eq!(prog.name, "Calisthenics power");
    assert_eq!(prog.goal, Goal::Power);
}

#[test]
fn weighted_dip_regress_at_101_kg_lowers_the_load_by_stepping_down() {
    // 101 kg, last Weighted dip at +2.5 kg: one load step (3.75 kg) would wipe
    // the load and the rung's first step (3.75 kg) is heavier than last time,
    // so the regress steps down a rung instead (KB §2.2).
    let p = Profile {
        bodyweight_kg: Some(101.0),
        ..calis_profile()
    };
    let mut dip = anchor(LadderId::Dip, 5);
    dip.regress = true;
    dip.last_added_kg = 2.5;
    let prog = synthesize(&p, &with(vec![dip]), 0).expect("plan");
    let c = calis(item(&prog, LadderId::Dip).unwrap());
    assert_eq!(c.exercise, "Ring dip");
    assert_eq!(c.added_kg, None);
    // At 80 kg the same regress steps down too, never back to the same +2.5 kg.
    let p80 = Profile {
        bodyweight_kg: Some(80.0),
        ..calis_profile()
    };
    let prog = synthesize(&p80, &with(vec![dip]), 0).expect("plan");
    assert_eq!(calis(item(&prog, LadderId::Dip).unwrap()).tier, 4);
    // A heavier belt sheds one step and stays on the rung.
    let mut heavy = dip;
    heavy.last_added_kg = 10.0;
    let prog = synthesize(&p, &with(vec![heavy]), 0).expect("plan");
    let c = calis(item(&prog, LadderId::Dip).unwrap());
    assert_eq!(c.exercise, "Weighted dip");
    assert_eq!(c.added_kg, Some(6.25));
    // Re-entry after a break never adds load either.
    let mut back = anchor(LadderId::Dip, 5);
    back.reentry_steps = 1;
    back.last_added_kg = 2.5;
    let prog = synthesize(&p, &with(vec![back]), 0).expect("plan");
    let c = calis(item(&prog, LadderId::Dip).unwrap());
    assert_eq!(c.tier, 4);
    assert_eq!(c.added_kg, None);
}

#[test]
fn pull_up_leverage_branch_advances_by_rung_and_full_pull_up_by_load() {
    let p = Profile {
        bodyweight_kg: Some(80.0),
        ..calis_profile()
    };
    let mut lsit = anchor(LadderId::VerticalPull, 5);
    lsit.advance_ready = true;
    let prog = synthesize(&p, &with(vec![lsit]), 0).expect("plan");
    let c = calis(item(&prog, LadderId::VerticalPull).unwrap());
    assert_eq!(c.exercise, "One-arm assisted pull-up");
    assert!(!c.load_capable);
    assert_eq!(c.added_kg, None);
    // The one-arm top takes load again.
    let mut top = anchor(LadderId::VerticalPull, 7);
    top.advance_ready = true;
    let prog = synthesize(&p, &with(vec![top]), 0).expect("plan");
    let c = calis(item(&prog, LadderId::VerticalPull).unwrap());
    assert_eq!(c.tier, 7);
    assert!(c.load_capable);
    assert_eq!(c.added_kg, Some(2.5));
}

#[test]
fn assisted_nordic_leaves_a_rep_and_is_safety_critical() {
    let hyp = Profile {
        lift_goal: LiftGoal::Hypertrophy,
        ..calis_profile()
    };
    let prog = synthesize(&hyp, &with(vec![anchor(LadderId::Hinge, 4)]), 0).expect("plan");
    let rx = item(&prog, LadderId::Hinge).unwrap();
    let c = calis(rx);
    assert_eq!(c.exercise, "Assisted Nordic curl");
    assert_eq!(c.rir, (1, 2));
    assert_eq!(claim(rx), claims::CAL_EFAIL_SAFE_001);
    assert!(rx.safety_critical());
}

#[test]
fn retest_prompt_only_where_a_max_reps_set_can_re_place() {
    let mut push = anchor(LadderId::HorizontalPush, 4);
    push.reentry_steps = 2;
    push.retest = true;
    let mut lever = anchor(LadderId::FrontLever, 3);
    lever.reentry_steps = 2;
    lever.retest = true;
    let prog = synthesize(&calis_profile(), &with(vec![push, lever]), 0).expect("plan");
    let c = calis(item(&prog, LadderId::HorizontalPush).unwrap());
    assert!(c.retest && c.skip_reps.is_some());
    let fl = calis(item(&prog, LadderId::FrontLever).unwrap());
    assert_eq!(fl.reentry_steps, 2);
    assert!(!fl.retest, "no max-reps test on a straight-arm hold");
}

#[test]
fn pounds_profile_added_load_lands_on_the_lb_grid() {
    let p = Profile {
        bodyweight_kg: Some(80.0),
        weight_lb: true,
        ..calis_profile()
    };
    let mut pull = anchor(LadderId::VerticalPull, 4);
    pull.advance_ready = true;
    let prog = synthesize(&p, &with(vec![pull]), 0).expect("plan");
    let kg = calis(item(&prog, LadderId::VerticalPull).unwrap())
        .added_kg
        .unwrap();
    // 2.5% of 80 kg = 4.4 lb → up to the 2.5 lb grid: 5 lb.
    assert!((kg / 0.45359237 - 5.0).abs() < 1e-9, "{kg} kg");
}

#[test]
fn novice_calisthenics_leaves_two_to_three_reps() {
    for goal in [LiftGoal::MaxStrength, LiftGoal::Hypertrophy] {
        let novice = Profile {
            lift_goal: goal,
            progression_cadence: crate::individualization::ProgressionCadence::EverySession,
            ..calis_profile()
        };
        let prog =
            synthesize(&novice, &with(vec![anchor(LadderId::HorizontalPush, 4)]), 0).expect("plan");
        assert_eq!(
            calis(item(&prog, LadderId::HorizontalPush).unwrap()).rir,
            (2, 3)
        );
        // Day-0 easy rungs too: no near-failure set on day 1.
        let prog = synthesize(&novice, &Anchors::default(), 0).expect("plan");
        for s in lift_days(&prog) {
            for p in &s.prescriptions {
                let c = calis(p);
                if !c.target.is_timed() && !c.ballistic {
                    assert!(c.rir.0 >= 2, "{}: {:?}", c.exercise, c.rir);
                }
            }
        }
    }
}
