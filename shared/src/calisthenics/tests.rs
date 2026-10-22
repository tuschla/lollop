use super::*;
use std::collections::HashSet;

#[test]
fn fifteen_ladders_all_nonempty() {
    assert_eq!(LADDERS.len(), 15);
    for l in LADDERS {
        assert!(!l.tiers.is_empty(), "{:?} has no tiers", l.id);
    }
}

#[test]
fn every_ladder_id_resolves() {
    for id in [
        HorizontalPush,
        Dip,
        VerticalPush,
        HorizontalPull,
        VerticalPull,
        Squat,
        Hinge,
        Core,
        Handstand,
        FrontLever,
        Planche,
        BackLever,
        MuscleUp,
        Jump,
        PlyoPush,
    ] {
        assert_eq!(ladder(id).id, id);
    }
}

#[test]
fn tier_names_unique_across_all_ladders() {
    // Log matching must be unambiguous (tier_by_name): unique even after
    // the case/punctuation/plural normalization the lookup applies.
    let mut seen = HashSet::new();
    for l in LADDERS {
        for t in l.tiers {
            assert!(
                seen.insert(name_key(t.name)),
                "duplicate tier name: {:?}",
                t.name
            );
        }
    }
}

#[test]
fn aliases_point_at_real_rungs_and_never_shadow_a_canonical_name() {
    let canonical: HashSet<String> = LADDERS
        .iter()
        .flat_map(|l| l.tiers.iter().map(|t| name_key(t.name)))
        .collect();
    let mut seen = HashSet::new();
    for &(alias, id, rung) in ALIASES {
        assert!(tier(id, rung).is_some(), "alias {alias:?} → missing rung");
        let k = name_key(alias);
        assert!(
            !canonical.contains(&k),
            "alias {alias:?} duplicates a rung name"
        );
        assert!(seen.insert(k), "duplicate alias {alias:?}");
    }
}

#[test]
fn legacy_labels_name_their_rung_and_display_as_it() {
    for &(old, new) in LEGACY_NAMES {
        let (id, rung) = tier_by_name(old).unwrap_or_else(|| panic!("{old:?} matches no rung"));
        assert_eq!(tier(id, rung).map(|t| t.name), Some(new), "{old:?}");
        assert_eq!(crate::hypertrophy::canonical_name(old), new);
        assert_eq!(crate::hypertrophy::canonical_name(&old.to_uppercase()), new);
    }
}

#[test]
fn tier_by_name_matches_common_aliases() {
    assert_eq!(tier_by_name("Pull ups"), Some((VerticalPull, 4)));
    assert_eq!(tier_by_name("Pull-up"), Some((VerticalPull, 4)));
    assert_eq!(tier_by_name("pullups"), Some((VerticalPull, 4)));
    assert_eq!(tier_by_name("Chin-ups"), Some((VerticalPull, 4)));
    assert_eq!(tier_by_name("Push-ups"), Some((HorizontalPush, 4)));
    assert_eq!(tier_by_name("pushup"), Some((HorizontalPush, 4)));
    assert_eq!(tier_by_name("Dips"), Some((Dip, 3)));
    assert_eq!(tier_by_name("Pistol"), Some((Squat, 5)));
    assert_eq!(tier_by_name("Front lever tuck"), Some((FrontLever, 0)));
    // Plurals of canonical names resolve too.
    assert_eq!(tier_by_name("Archer push-ups"), Some((HorizontalPush, 7)));
    assert_eq!(tier_by_name("scapular pull"), Some((VerticalPull, 1)));
    // A barbell lift is not a rung.
    assert_eq!(tier_by_name("Bench press"), None);
    assert_eq!(tier_by_name("Back squat"), None);
    assert_eq!(tier_by_name("  "), None);
}

#[test]
fn every_load_and_graded_claim_is_citable() {
    for l in LADDERS {
        for t in l.tiers {
            assert!(
                !t.load_claim.entry().is_blocked(),
                "load claim {:?} is a myth",
                t.load_claim
            );
        }
    }
    for id in [
        claims::CAL_PROG_TRIGGER_001,
        claims::CAL_GATE_NUMBERS_001,
        claims::CAL_EFAIL_SAFE_001,
        claims::CAL_INTENSITY_FLOOR_001,
        claims::CAL_INTENSITY_001,
        claims::CAL_TEMPLATE_001,
        claims::CAL_GOAL_RIR_001,
        claims::CAL_RIR_ACC_001,
        claims::CAL_PLACE_001,
        claims::CAL_REENTRY_001,
        claims::CAL_EQUIP_001,
        claims::CAL_POWER_001,
        claims::CAL_POWER_LADDER_001,
        claims::CAL_PROG_TEMPO_001,
        claims::CAL_PROG_STATIC_001,
        claims::CAL_E1RM_001,
        claims::CAL_LOAD_PULL_001,
        claims::STR_LOADPROG_001,
        claims::STR_PWR_001,
        claims::PLYO_001,
    ] {
        assert!(!id.entry().is_blocked(), "graded claim {id:?} is a myth");
    }
}

#[test]
fn measured_fractions_carry_load_claims_inferred_carry_inferred() {
    for l in LADDERS {
        for t in l.tiers {
            match t.bw_fraction {
                Some(_) => {
                    assert_ne!(
                        t.load_claim, INF,
                        "measured {:?} must not be inferred",
                        t.name
                    );
                    assert_eq!(
                        t.load_claim.entry().grade,
                        crate::schema::EvidenceGrade::Moderate,
                        "measured {:?} claim must be Moderate",
                        t.name
                    );
                }
                None => assert_eq!(
                    t.load_claim, INF,
                    "inferred {:?} must carry INFERRED",
                    t.name
                ),
            }
        }
    }
}

#[test]
fn skill_ladders_are_leave_rir_and_hold_biased() {
    for l in LADDERS.iter().filter(|l| l.skill) {
        for t in l.tiers {
            assert!(
                matches!(t.failure, FailurePolicy::LeaveRir(_)),
                "skill rung {:?} must leave RIR (CAL-EFAIL-SAFE-001)",
                t.name
            );
        }
    }
    // Pure static/balance tracks are entirely hold-based; the front-lever and
    // planche skill tracks are hold-based lever rungs with one rep-based
    // dynamic weighted top; muscle-up is the one fully rep-based skill track.
    for id in [Handstand, BackLever] {
        assert!(
            ladder(id).tiers.iter().all(|t| t.target.is_timed()),
            "{id:?} all holds"
        );
    }
    for id in [FrontLever, Planche] {
        let ts = ladder(id).tiers;
        assert!(
            ts[..ts.len() - 1].iter().all(|t| t.target.is_timed()),
            "{id:?} lever rungs are holds"
        );
        assert!(
            !ts[ts.len() - 1].target.is_timed(),
            "{id:?} weighted top is dynamic"
        );
    }
    assert!(
        ladder(MuscleUp).tiers.iter().all(|t| !t.target.is_timed()),
        "muscle-up is rep-based"
    );
}

#[test]
fn eccentric_rungs_leave_rir() {
    // CAL-EFAIL-SAFE-001: eccentric ("Negative"/"negative") rungs are never
    // trained to momentary failure — they must leave reps in reserve. Covers
    // "Negative pull-up", "Negative Nordic curl", "Negative muscle-up".
    for l in LADDERS.iter() {
        for t in l.tiers {
            if t.name.to_ascii_lowercase().contains("negative") {
                assert!(
                    matches!(t.failure, FailurePolicy::LeaveRir(_)),
                    "eccentric rung {:?} must leave RIR (CAL-EFAIL-SAFE-001)",
                    t.name
                );
            }
        }
    }
}

#[test]
fn tier_by_name_is_case_insensitive_and_exact() {
    assert_eq!(tier_by_name("full push-up"), Some((HorizontalPush, 4)));
    assert_eq!(tier_by_name("  Full pull-up "), Some((VerticalPull, 4)));
    assert_eq!(tier_by_name("Negative muscle-up"), Some((MuscleUp, 2)));
    assert_eq!(tier_by_name("nope"), None);
}

#[test]
fn advance_trigger_is_band_top() {
    let t = tier(HorizontalPush, 4).unwrap();
    assert_eq!(t.target.top(), 8);
    assert_eq!(t.target.floor(), 5);
    let fl = tier(FrontLever, 0).unwrap();
    assert_eq!(fl.target.top(), 15);
    assert!(fl.target.is_timed());
}

#[test]
fn rir_band_is_rung_dependent() {
    let s = LiftGoal::MaxStrength;
    // Easy high-rep self-limiting rung → approach failure.
    assert_eq!(rir_band(HorizontalPush, 0, s, false).unwrap().value, (0, 1));
    // Mid self-limiting rung → RIR 1-2.
    assert_eq!(rir_band(HorizontalPush, 4, s, false).unwrap().value, (1, 2));
    // Skill / LeaveRir(1) rung → (1, 2) via CAL-EFAIL-SAFE-001.
    let r = rir_band(FrontLever, 0, s, false).unwrap();
    assert_eq!(r.value, (1, 2));
    assert_eq!(r.claim_id(), claims::CAL_EFAIL_SAFE_001);
    // Ballistic work states no numeric RIR.
    assert!(rir_band(Jump, 0, LiftGoal::Power, false).is_none());
}

#[test]
fn rir_band_never_drops_toward_failure_as_a_ladder_gets_harder() {
    // A high-rep rung above a low-rep rung is not an "easy" rung: hanging
    // leg raise (8–12) above the knee ab-wheel (5–10) leaves 1–2 reps.
    assert!(!easy_rung(Core, 4));
    assert_eq!(
        rir_band(Core, 4, LiftGoal::MaxStrength, false)
            .unwrap()
            .value,
        (1, 2)
    );
    // Strength / power goals and every novice band. (A hypertrophy goal
    // takes self-limiting rungs to 0–1 while an eccentric leave-RIR rung
    // below them keeps 1–2 — CAL-EFAIL-SAFE-001 — so it is exempt.)
    for (goal, novice) in [
        (LiftGoal::MaxStrength, false),
        (LiftGoal::Power, false),
        (LiftGoal::MaxStrength, true),
        (LiftGoal::Hypertrophy, true),
    ] {
        for l in LADDERS.iter().filter(|l| !l.power) {
            let mut prev = 0;
            for idx in 0..l.tiers.len() as u8 {
                if l.tiers[idx as usize].target.is_timed() {
                    continue;
                }
                let lo = rir_band(l.id, idx, goal, novice).unwrap().value.0;
                assert!(
                    lo >= prev,
                    "{:?} rung {idx} asks for fewer reps left than an easier rung \
                     ({goal:?}, novice {novice})",
                    l.id
                );
                prev = lo;
            }
        }
    }
}

#[test]
fn novices_get_the_novice_band_on_every_rep_rung() {
    for goal in [LiftGoal::MaxStrength, LiftGoal::Hypertrophy] {
        let wall = rir_band(HorizontalPush, 0, goal, true).unwrap();
        assert_eq!(wall.value, NOVICE_RIR);
        assert_eq!(NOVICE_RIR, (2, 3));
        assert_eq!(wall.claim_id(), claims::HYP_RIR_ACC_001);
        assert_eq!(
            rir_band(HorizontalPush, 4, goal, true).unwrap().value,
            (2, 3)
        );
        // A leave-RIR rung keeps its safety face, never below the novice band.
        let neg = rir_band(VerticalPull, 3, goal, true).unwrap();
        assert_eq!(neg.value, (2, 3));
        assert_eq!(neg.claim_id(), claims::CAL_EFAIL_SAFE_001);
    }
}

#[test]
fn nordic_and_one_arm_rungs_are_never_to_failure() {
    // CAL-EFAIL-SAFE-001 names the Nordic; §2.8 discourages failure on
    // one-arm work.
    for (id, idx) in [
        (Hinge, 4),
        (HorizontalPush, 8),
        (HorizontalPull, 6),
        (VerticalPull, 6),
    ] {
        assert_eq!(tier(id, idx).unwrap().failure, FailurePolicy::LeaveRir(1));
        let r = rir_band(id, idx, LiftGoal::Hypertrophy, false).unwrap();
        assert_eq!(r.value, (1, 2));
        assert_eq!(r.claim_id(), claims::CAL_EFAIL_SAFE_001);
    }
    assert_eq!(tier(Hinge, 4).unwrap().name, "Assisted Nordic curl");
}

#[test]
fn hypertrophy_calisthenics_self_limiting_rir_is_zero_to_one() {
    let r = rir_band(HorizontalPush, 4, LiftGoal::Hypertrophy, false).unwrap();
    assert_eq!(r.value, (0, 1));
    assert_eq!(r.claim_id(), claims::CAL_GOAL_RIR_001);
}

#[test]
fn hypertrophy_band_widens_to_6_12() {
    let full = tier(HorizontalPush, 4).unwrap();
    assert_eq!(
        working_target(full, LiftGoal::Hypertrophy, false),
        TierTarget::Reps { low: 6, high: 12 }
    );
    // Every hypertrophy rep band sits inside the 6–15 template-B window.
    for l in LADDERS.iter().filter(|l| !l.skill && !l.power) {
        for t in l
            .tiers
            .iter()
            .filter(|t| t.failure == FailurePolicy::SelfLimiting)
        {
            if let TierTarget::Reps { low, high } = working_target(t, LiftGoal::Hypertrophy, false)
            {
                assert!(
                    low >= 6 && (12..=15).contains(&high),
                    "{}: {low}-{high}",
                    t.name
                );
            }
        }
    }
}

#[test]
fn strength_band_unchanged() {
    for l in LADDERS {
        for t in l.tiers.iter().filter(|t| !t.straight_arm) {
            assert_eq!(working_target(t, LiftGoal::MaxStrength, false), t.target);
        }
    }
}

#[test]
fn leave_rir_rungs_unchanged_for_hypertrophy() {
    let ring_dip = tier(Dip, 4).unwrap();
    assert_eq!(
        working_target(ring_dip, LiftGoal::Hypertrophy, false),
        ring_dip.target
    );
    assert_eq!(
        rir_band(Dip, 4, LiftGoal::Hypertrophy, false)
            .unwrap()
            .value,
        rir_band(Dip, 4, LiftGoal::MaxStrength, false)
            .unwrap()
            .value
    );
    // A loaded self-limiting rung keeps its table band too.
    let pull = tier(VerticalPull, 4).unwrap();
    assert_eq!(
        working_target(pull, LiftGoal::Hypertrophy, true),
        pull.target
    );
}

#[test]
fn lever_prescription_working_band_tops_at_10s() {
    let tuck = tier(FrontLever, 0).unwrap();
    assert_eq!(
        working_target(tuck, LiftGoal::MaxStrength, false),
        TierTarget::HoldSec { low: 5, high: 10 }
    );
    assert_eq!(test_hold_sec(tuck), Some(15));
    assert_eq!(advance_top(tuck, LiftGoal::MaxStrength, false), 15);
    let straddle = tier(FrontLever, 2).unwrap();
    assert_eq!(
        working_target(straddle, LiftGoal::MaxStrength, false),
        TierTarget::HoldSec { low: 5, high: 7 }
    );
    assert_eq!(test_hold_sec(straddle), Some(10));
    // A non-straight-arm hold has no separate test.
    assert_eq!(test_hold_sec(tier(Core, 0).unwrap()), None);
}

#[test]
fn skip_threshold_is_one_and_a_half_band_tops_and_never_on_unsafe_rungs() {
    let wall = tier(HorizontalPush, 0).unwrap();
    assert_eq!(skip_threshold(wall, LiftGoal::Hypertrophy).value, Some(18));
    // Strength goal: >15 reps already counts as far too easy.
    assert_eq!(
        skip_threshold(tier(Hinge, 0).unwrap(), LiftGoal::MaxStrength).value,
        Some(16)
    );
    assert_eq!(skip_threshold(wall, LiftGoal::MaxStrength).value, Some(16));
    // Straight-arm, leave-RIR, timed and ballistic rungs never multi-skip.
    assert_eq!(
        skip_threshold(tier(FrontLever, 4).unwrap(), LiftGoal::MaxStrength).value,
        None
    );
    assert_eq!(
        skip_threshold(tier(Dip, 4).unwrap(), LiftGoal::MaxStrength).value,
        None
    );
    assert_eq!(
        skip_threshold(tier(Core, 0).unwrap(), LiftGoal::MaxStrength).value,
        None
    );
    assert_eq!(
        skip_threshold(tier(Jump, 0).unwrap(), LiftGoal::Power).value,
        None
    );
}

#[test]
fn equipment_filters_rungs() {
    let none = Kit {
        pull_up_bar: false,
        dip_bars: false,
        rings: false,
    };
    assert!(!ladder_available(VerticalPull, none));
    assert!(ladder_available(HorizontalPull, none));
    assert!(rung_available(Dip, 0, none) && !rung_available(Dip, 3, none));
    assert_eq!(
        available_above(HorizontalPull, 3, none),
        None,
        "rings/bar rows only"
    );
    assert_eq!(
        available_above(Core, 3, none),
        Some(5),
        "hanging leg raise skipped"
    );
    assert_eq!(nearest_available(Dip, 3, none), Some(0));
    let rings = Kit {
        rings: true,
        ..none
    };
    assert!(ladder_available(VerticalPull, rings) && rung_available(Dip, 3, rings));
    assert!((0..8).all(|i| rung_available(VerticalPull, i, Kit::ALL)));
}

#[test]
fn reentry_brackets() {
    assert_eq!(reentry(10).value.steps, 0);
    assert_eq!(reentry(14).value.steps, 1);
    assert_eq!(
        reentry(56).value,
        Reentry {
            steps: 1,
            retest: false
        }
    );
    assert_eq!(
        reentry(57).value,
        Reentry {
            steps: 2,
            retest: true
        }
    );
    assert_eq!(reentry(30).claim_id(), claims::CAL_REENTRY_001);
}

#[test]
fn load_step_is_the_low_end_of_two_to_ten_percent_on_the_plate_grid() {
    assert_eq!(load_step_kg(None, 0.0, false).value, 1.25);
    // 2.5% of 80 kg = 2.0 → rounded up to 2.5.
    assert_eq!(load_step_kg(Some(80.0), 0.0, false).value, 2.5);
    // 2.5% of 100 kg = 2.5.
    assert_eq!(load_step_kg(Some(80.0), 20.0, false).value, 2.5);
}

#[test]
fn pounds_profile_load_step_lands_on_the_two_and_a_half_lb_grid() {
    let lb = |kg: f64| kg / 0.45359237;
    // No bodyweight: one 2.5 lb plate.
    assert!((lb(load_step_kg(None, 0.0, true).value) - 2.5).abs() < 1e-9);
    // 2.5% of 100 kg = 2.5 kg = 5.51 lb → up to 7.5 lb, never "+5.5 lb".
    assert!((lb(load_step_kg(Some(100.0), 0.0, true).value) - 7.5).abs() < 1e-9);
    // An on-grid load never loses a plate to float error when floored.
    let five = 5.0 * 0.45359237;
    assert!((lb(floor_to_load_grid(five - LOAD_GRID_LB_IN_KG, true)) - 2.5).abs() < 1e-9);
    assert_eq!(floor_to_load_grid(2.4, false), 1.25);
}

#[test]
fn weighted_ladder_set_yields_system_load_e1rm() {
    // Pull-up: bodyweight + added (CAL-LOAD-PULL-001), Epley at RPE 10.
    let e = system_load_e1rm(VerticalPull, 4, 80.0, 20.0, 5, 10.0).unwrap();
    assert_eq!(
        e.value,
        (crate::strength::e1rm_epley_rpe(100.0, 5, 10.0) * 10.0).round() / 10.0
    );
    assert_eq!(e.claim_id(), claims::CAL_E1RM_001);
    // Unloaded, no fraction, or too many reps → none.
    assert!(system_load_e1rm(VerticalPull, 4, 80.0, 0.0, 5, 10.0).is_none());
    assert!(system_load_e1rm(Dip, 5, 80.0, 20.0, 5, 10.0).is_none());
    assert!(system_load_e1rm(VerticalPull, 4, 80.0, 20.0, 15, 10.0).is_none());
    // The leverage branch has no system-load model.
    assert!(system_load_e1rm(VerticalPull, 5, 80.0, 20.0, 5, 10.0).is_none());
}

#[test]
fn ballistic_sets_respect_the_contact_cap() {
    for l in LADDERS.iter().filter(|l| l.power) {
        for t in l.tiers {
            assert_eq!(t.failure, FailurePolicy::Ballistic);
            let sets = ballistic_sets(t).value as u32;
            let contacts = sets * t.target.top() as u32 * if t.unilateral { 2 } else { 1 };
            assert!(
                sets >= 3 && contacts <= PLYO_CONTACT_CAP,
                "{}: {sets} sets",
                t.name
            );
        }
    }
}

#[test]
fn effective_load_only_for_measured() {
    let full = tier(HorizontalPush, 4).unwrap();
    assert_eq!(
        effective_load_kg(full, 80.0, 0.0),
        Some((0.64 + 0.75) / 2.0 * 80.0)
    );
    let inferred = tier(Dip, 3).unwrap();
    assert_eq!(effective_load_kg(inferred, 80.0, 0.0), None);
}

#[test]
fn gates_and_templates() {
    assert!(!tier_advance_met(1).value);
    assert!(tier_advance_met(2).value);
    assert!(!static_acclimation_ok(27).value);
    assert!(static_acclimation_ok(28).value);
    assert!(!muscle_up_gate_met(11, 12).value);
    assert!(muscle_up_gate_met(12, 12).value);
    assert_eq!(
        session_template(LiftGoal::MaxStrength).value,
        CalisTemplate {
            sets: (3, 5),
            rest_sec: 180
        }
    );
    assert_eq!(
        session_template(LiftGoal::Hypertrophy).value,
        CalisTemplate {
            sets: (3, 4),
            rest_sec: 90
        }
    );
}

#[test]
fn weighted_rungs_match_plan() {
    assert!(weighted_ok(VerticalPull, 4));
    assert!(!weighted_ok(VerticalPull, 3));
    // KB §3.5: the leverage branch advances by rung, its one-arm top loads.
    assert!(!weighted_ok(VerticalPull, 5));
    assert!(!weighted_ok(VerticalPull, 6));
    assert!(weighted_ok(VerticalPull, 7));
    assert!(!weighted_ok(Handstand, 3));
    assert!(weighted_ok(Dip, 5));
    assert!(!weighted_ok(Dip, 4));
}
