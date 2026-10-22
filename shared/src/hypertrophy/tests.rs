use super::*;
use crate::schema::EvidenceGrade;

#[test]
fn landmarks_lookup_is_case_insensitive_and_verbatim() {
    let chest = landmarks_for("Chest").expect("chest present");
    assert_eq!(chest.mev, 10);
    assert_eq!(chest.mav, (12, 20));
    assert_eq!(chest.mrv, 22);
    assert_eq!(landmarks_for("QUADS").unwrap().mv, 6);
    assert!(landmarks_for("tail").is_none());
}

#[test]
fn delts_are_not_folded_into_side_delts() {
    // HARD RULE 1: KB Table 1 (03-hypertrophy.md:514-515) has ONLY "Side
    // delts" and "Rear delts" rows. Explicit side/rear phrasing resolves to
    // its own row (identity pass-through)...
    assert!(landmarks_for(landmark_key("side delts")).is_some());
    assert!(landmarks_for(landmark_key("rear delts")).is_some());
    // ...but the front delt is a distinct head and generic "delts" spans all
    // three: neither carries a Table 1 row, so both fall through to None
    // (the honest no-landmark fallback), never borrowing Side delts.
    assert!(landmarks_for(landmark_key("front delt")).is_none());
    assert!(landmarks_for(landmark_key("delts")).is_none());

    // Lateral raise (primary = side delts) still lands the Side delts band.
    let lr = landmarks_for_exercise("Lateral raise").expect("side delts row");
    assert_eq!((lr.mev, lr.mrv), (8, 26));
    // Overhead press's primary muscle is "delts", which carries no Table 1
    // row. Rather than borrowing a secondary muscle's band (triceps), the
    // lookup returns None so the caller falls back to the training-age band.
    assert!(landmarks_for_exercise("Overhead press").is_none());
}

#[test]
fn rep_load_windows_match_table_2() {
    let heavy = rep_load(ExerciseClass::HeavyCompound);
    assert_eq!(heavy.value.reps, (5, 10));
    assert_eq!(heavy.value.pct_1rm, (75, 85));
    assert_eq!(heavy.grade(), EvidenceGrade::Moderate);
    assert_eq!(rep_load(ExerciseClass::Isolation).value.reps, (12, 25));
}

#[test]
fn rest_windows_split_compound_vs_isolation() {
    assert_eq!(rest_sec_for(ExerciseClass::HeavyCompound).value, (120, 180));
    // HYP-REST-002 (Singer 2024): >60 s helps, ~90 s floor for
    // isolation/machine work and the moderate (machine) class.
    assert_eq!(rest_sec_for(ExerciseClass::Isolation).value, (90, 120));
    assert_eq!(
        rest_sec_for(ExerciseClass::ModerateCompound).value,
        (90, 120)
    );
    assert_eq!(
        rest_sec_for(ExerciseClass::Isolation).claim_id(),
        claims::HYP_REST_002
    );
}

#[test]
fn frequency_bands_follow_table_3() {
    assert_eq!(frequency_for_weekly_sets(8).value.freq, (1, 2));
    assert_eq!(frequency_for_weekly_sets(14).value.freq, (2, 3));
    assert_eq!(frequency_for_weekly_sets(24).value.freq, (3, 3));
    // Frequency evidence is Strong (2x/week beats 1x).
    assert_eq!(frequency_for_weekly_sets(14).grade(), EvidenceGrade::Strong);
}

#[test]
fn set_ramp_matches_rp_worked_example() {
    let ramp = weekly_set_ramp(10, 20, 4);
    assert_eq!(ramp.value, vec![10, 13, 16, 20]);
    // Degenerate cases.
    assert_eq!(weekly_set_ramp(10, 20, 1).value, vec![10]);
    assert!(weekly_set_ramp(10, 20, 0).value.is_empty());
    // MRV below MEV clamps flat.
    assert_eq!(weekly_set_ramp(12, 8, 3).value, vec![12, 12, 12]);
}

#[test]
fn set_ramp_never_jumps_more_than_four_sets_per_week() {
    // hyp-001 increment cap (+2–4/wk) + hyp-011 no-abrupt-jump guard:
    // a short block over a wide span must not emit +10/wk steps or land
    // straight on MRV.
    let ramp = weekly_set_ramp(10, 30, 3).value;
    assert_eq!(ramp, vec![10, 14, 18]);
    for w in ramp.windows(2) {
        assert!(w[1] - w[0] <= MAX_WEEKLY_SET_INCREMENT);
    }
    // Ramps within the cap are untouched (RP worked example covered above).
    let ok = weekly_set_ramp(10, 20, 4).value;
    assert_eq!(ok, vec![10, 13, 16, 20]);
}

#[test]
fn abrupt_volume_jump_guard_is_safety_flagged() {
    assert!(abrupt_volume_jump(10, 20).value);
    assert!(!abrupt_volume_jump(10, 14).value);
    assert!(!abrupt_volume_jump(14, 10).value); // reductions are not jumps
    let g = abrupt_volume_jump(10, 20);
    assert!(g.safety_critical());
    assert_eq!(g.grade(), EvidenceGrade::ExpertOpinion);
}

#[test]
fn growth_ceiling_and_min_sets() {
    assert_eq!(WEEKLY_FRACTIONAL_SET_CEILING, 31);
    assert_eq!(MIN_SETS_PER_EXERCISE, 2);
    assert_eq!(cap_weekly_growth_target(40).value, 31);
    assert_eq!(cap_weekly_growth_target(20).value, 20);
}

#[test]
fn mev_scales_with_training_age() {
    assert_eq!(mev_sets_by_training_age(TrainingAge::Novice).value, (6, 10));
    assert_eq!(
        mev_sets_by_training_age(TrainingAge::Intermediate).value,
        (10, 18)
    );
    assert_eq!(
        mev_sets_by_training_age(TrainingAge::Advanced).value,
        (12, 20)
    );
}

#[test]
fn next_meso_adds_two_when_stalled_and_fresh() {
    assert_eq!(next_meso_weekly_sets(12, true, true).value, 14);
    assert_eq!(next_meso_weekly_sets(12, true, false).value, 12);
    assert_eq!(next_meso_weekly_sets(30, true, true).value, 31); // capped
}

#[test]
fn over_mrv_and_recovery_scaling() {
    // Unknown/unspecified muscle → generic RP ~20-set anchor.
    assert!(over_mrv_deload("", 22, true, false).value);
    assert!(over_mrv_deload("", 22, false, true).value);
    assert!(!over_mrv_deload("", 18, true, true).value); // under the 20 anchor
    assert!(!over_mrv_deload("", 22, false, false).value); // no symptom
    // Per-muscle MRV (Table 1): glutes deload above 16, side delts above 26.
    assert!(over_mrv_deload("glutes", 17, true, false).value);
    assert!(!over_mrv_deload("glutes", 16, true, false).value);
    assert!(!over_mrv_deload("side delts", 22, true, false).value);
    assert!(over_mrv_deload("side delts", 27, true, false).value);
    // "lats" routes through landmark_key -> back key, so its MRV is back's
    // 25 (Table 1), NOT the generic 20-set anchor. Pin the boundary.
    let back_mrv = landmarks_for("back").unwrap().mrv; // 25
    assert!(!over_mrv_deload("lats", back_mrv, true, false).value); // at MRV, no deload
    assert!(over_mrv_deload("lats", back_mrv + 1, true, false).value); // over back MRV
    assert!(!over_mrv_deload("lats", 21, true, false).value); // above 20 anchor, still under back MRV
    // Low recovery scales to 70–80%.
    let (lo, hi) = recovery_adjusted_volume(20, true).value;
    assert!((lo - 14.0).abs() < 1e-9 && (hi - 16.0).abs() < 1e-9);
    assert_eq!(recovery_adjusted_volume(20, false).value, (20.0, 20.0));
}

#[test]
fn joint_pain_shifts_to_light_high_reps() {
    let rx = joint_pain_rep_shift().value;
    assert_eq!(rx.reps, (12, 25));
    assert_eq!(rx.pct_1rm, (50, 70));
}

#[test]
fn split_triggers_and_deload() {
    assert!(needs_session_split(13).value);
    assert!(!needs_session_split(12).value);
    assert!(per_session_over_cap(12).value);
    assert!(!per_session_over_cap(11).value);
    assert!(deload_indicated(2).value);
    assert!(!deload_indicated(1).value);
    let d = deload_rx().value;
    assert!((d.sets_fraction - 0.50).abs() < 1e-9);
    assert_eq!(d.rir, (2, 4));
    assert_eq!(d.load_frac_of_working, (0.60, 0.70));
    assert!(increase_rest_on_rep_drop(0.12).value);
    assert!(!increase_rest_on_rep_drop(0.08).value);
}

#[test]
fn rir_ramp_descends_to_one() {
    assert_eq!(rir_for_week(1, 4).unwrap().value, 4);
    assert_eq!(rir_for_week(2, 4).unwrap().value, 3);
    assert_eq!(rir_for_week(3, 4).unwrap().value, 2);
    assert_eq!(rir_for_week(4, 4).unwrap().value, 1);
    assert!(rir_for_week(0, 4).is_none());
    assert!(rir_for_week(5, 4).is_none());
    // ExpertOpinion-graded schedule.
    assert_eq!(
        rir_for_week(1, 4).unwrap().grade(),
        EvidenceGrade::ExpertOpinion
    );
}

#[test]
fn rir_ramp_never_extrapolates_past_kb_schedule() {
    // hyp-020: RIR only ±1-rep accurate at 0–5; the hyp-019 schedule
    // starts at 4. Longer blocks hold 4, never 5+.
    assert_eq!(rir_for_week(1, 8).unwrap().value, 4);
    assert_eq!(rir_for_week(4, 8).unwrap().value, 4);
    assert_eq!(rir_for_week(5, 8).unwrap().value, 4);
    assert_eq!(rir_for_week(6, 8).unwrap().value, 3);
    assert_eq!(rir_for_week(8, 8).unwrap().value, 1);
    for block in 1..=12u8 {
        for week in 1..=block {
            assert!(rir_for_week(week, block).unwrap().value <= MAX_SCHEDULED_RIR);
        }
    }
}

// -- hyp-012 --
#[test]
fn load_is_interchangeable_30_to_85_pct() {
    let r = interchangeable_load_range();
    assert_eq!(r.value.reps, (5, 30));
    assert_eq!(r.value.pct_1rm, (30, 85));
    assert_eq!(r.grade(), EvidenceGrade::Strong);
    assert!(r.contested().is_some());
    assert_eq!(r.contested(), Some("CQ-F03-02"));
    assert!(load_below_effective_floor(29).value);
    assert!(!load_below_effective_floor(30).value);
}

// -- hyp-017 --
#[test]
fn high_skill_lifts_keep_reps_and_rir_floors() {
    let g = high_skill_guard();
    assert_eq!(g.value.min_reps, 5);
    assert_eq!(g.value.min_rir, (1, 2));
    assert!(g.safety_critical());
    assert_eq!(g.grade(), EvidenceGrade::ExpertOpinion);
}

// -- hyp-018 --
#[test]
fn default_rir_is_one_to_three_and_contested() {
    let d = default_rir_band();
    assert_eq!(d.value, (1, 3));
    assert_eq!(d.grade(), EvidenceGrade::Moderate);
    assert!(d.contested().is_some());
    assert_eq!(d.contested(), Some("CQ-02"));
}

// -- hyp-020 --
#[test]
fn rir_accuracy_degrades_far_from_failure() {
    assert_eq!(rir_reliability(0).value, RirReliability::WithinOneRep);
    assert_eq!(rir_reliability(5).value, RirReliability::WithinOneRep);
    assert_eq!(rir_reliability(6).value, RirReliability::ErrorOverTwoReps);
    assert_eq!(novice_start_rir().value, (3, 4));
    assert_eq!(novice_start_rir().grade(), EvidenceGrade::Moderate);
}

// -- hyp-021 --
#[test]
fn failure_reserved_for_machines_and_isolation() {
    // Machines and isolation: allowed.
    assert!(failure_allowed(ExerciseClass::Isolation, false).value);
    assert!(failure_allowed(ExerciseClass::HeavyCompound, true).value); // e.g. hack squat
    // Free-weight compounds (unspotted squat/bench): never.
    assert!(!failure_allowed(ExerciseClass::HeavyCompound, false).value);
    assert!(!failure_allowed(ExerciseClass::ModerateCompound, false).value);
    let f = failure_allowed(ExerciseClass::HeavyCompound, false);
    assert!(f.safety_critical());
}

// -- hyp-027/028 + Table 4 --
#[test]
fn table_4_is_complete_and_verbatim() {
    assert_eq!(EXERCISES.len(), 16);
    let slc = exercise_entry("Seated leg curl").unwrap();
    assert_eq!(slc.primary_muscles, &["hamstrings"]);
    assert_eq!(slc.equipment, &[Machine]);
    assert_eq!(slc.long_length, LongLengthBias::High);
    let lr = exercise_entry("Lateral raise").unwrap();
    assert_eq!(lr.long_length, LongLengthBias::Low);
    assert_eq!(lr.long_length_note, "Low (cable better at bottom)");
    assert_eq!(
        exercise_entry("Hip thrust").unwrap().long_length,
        LongLengthBias::LowToModerate
    );
}

#[test]
fn sfr_favors_machines_cables_and_stable_isolation() {
    let squat = exercise_entry("Back squat").unwrap();
    assert_eq!(squat.sfr_tier_with(&[Barbell]), SfrTier::Low);
    let hack = exercise_entry("Hack squat / leg press").unwrap();
    assert_eq!(hack.sfr_tier_with(&[Machine]), SfrTier::High);
    // Stable isolation is high-SFR even on free weights.
    let curl = exercise_entry("Incline DB curl").unwrap();
    assert_eq!(curl.sfr_tier_with(&[Dumbbell]), SfrTier::High);
}

// -- hyp-031 --
#[test]
fn meso_structure_is_4_to_6_plus_1_deload() {
    let m = meso_structure().value;
    assert_eq!(m.accumulation_weeks, (4, 6));
    assert_eq!(m.deload_weeks, 1);
    assert_eq!(m.deload_cadence_weeks, (4, 8));
    assert_eq!(meso_structure().grade(), EvidenceGrade::Moderate);
}

// -- hyp-037 --
#[test]
fn layoff_flags_reduced_mev() {
    assert!(layoff_reduces_mev(true).value);
    assert!(!layoff_reduces_mev(false).value);
    assert_eq!(
        layoff_reduces_mev(true).grade(),
        EvidenceGrade::ExpertOpinion
    );
}

// -- hyp-043 --
#[test]
fn intermediate_default_program_matches_synthesis() {
    let p = intermediate_default_program();
    assert_eq!(p.value.frequency_per_week, 2);
    assert_eq!(p.value.weekly_sets, (8, 10));
    assert_eq!(p.value.max_session_sets, 8);
    assert_eq!(p.value.compound_reps, (5, 10));
    assert_eq!(p.value.isolation_reps, (10, 20));
    assert_eq!(p.value.week1_rir, 3);
    assert_eq!(p.value.final_rir, 1);
    assert_eq!(p.value.compound_rest_sec, (120, 180));
    assert_eq!(p.value.isolation_rest_sec, (60, 120));
    assert_eq!(p.value.exercises_per_muscle, (1, 2));
    assert_eq!(p.value.deload_week, (5, 6));
    assert_eq!(p.grade(), EvidenceGrade::Moderate);
}

// -- 7b: any-name resolution, fractional volume, block targets --
#[test]
fn muscle_credits_follow_table_4_with_half_sets_for_helpers() {
    assert_eq!(
        muscle_credits("Bench press"),
        vec![("chest", 1.0), ("triceps", 0.5)]
    );
    // Row lists mid-back + lats, both the Table 1 back row: counted once.
    assert_eq!(
        muscle_credits("Row (barbell/cable/machine)"),
        vec![("back", 1.0)]
    );
    // Overhead press trains the delts (no Table 1 row): triceps only, ½.
    assert_eq!(muscle_credits("Overhead press"), vec![("triceps", 0.5)]);
    assert_eq!(primary_muscle("Overhead press"), None);
    // Names outside the catalog resolve by keyword, then movement pattern.
    assert_eq!(primary_muscle("Barbell bench press"), Some("chest"));
    assert_eq!(primary_muscle("Lying leg curl"), Some("hamstrings"));
    assert_eq!(primary_muscle("Hammer curl"), Some("biceps"));
    assert_eq!(primary_muscle("Reverse fly"), Some("rear delts"));
    assert_eq!(primary_muscle("Seated calf raise"), Some("calves"));
    assert_eq!(
        muscle_credits("Front squat"),
        vec![("quads", 1.0), ("glutes", 0.5)]
    );
    assert!(muscle_credits("Farmer carry").is_empty());
}

#[test]
fn block_rir_stays_in_the_default_band_and_novices_ramp_from_4() {
    let rir = |w, n, novice| block_rir(w, n, novice).value;
    assert_eq!(
        (1..=4).map(|w| rir(w, 4, false)).collect::<Vec<_>>(),
        vec![3, 3, 2, 1]
    );
    assert_eq!(
        (1..=5).map(|w| rir(w, 5, false)).collect::<Vec<_>>(),
        vec![3, 3, 3, 2, 1]
    );
    assert_eq!(
        (1..=5).map(|w| rir(w, 5, true)).collect::<Vec<_>>(),
        vec![4, 3, 2, 2, 2]
    );
    assert_eq!(
        block_rir(1, 4, false).claim_id(),
        claims::HYP_RIR_DEFAULT_001
    );
    assert_eq!(block_rir(4, 4, false).claim_id(), claims::HYP_RIR_RAMP_001);
}

#[test]
fn weekly_targets_by_role() {
    let chest = landmarks_for("chest").unwrap();
    let glutes = landmarks_for("glutes").unwrap();
    // Standard: the stated target, bounded to MEV..MRV, then the ramp.
    assert_eq!(
        weekly_targets(chest, 12, VolumeRole::Standard, 4).value[0],
        12
    );
    assert_eq!(
        weekly_targets(chest, 6, VolumeRole::Standard, 4).value[0],
        10
    );
    assert_eq!(
        weekly_targets(chest, 40, VolumeRole::Standard, 4).value[0],
        22
    );
    // MEV 0: compounds cover it, no direct target.
    assert_eq!(
        weekly_targets(glutes, 12, VolumeRole::Standard, 4).value,
        vec![0; 4]
    );
    // Specialization: +4 above the stated target; everyone else at MV.
    assert_eq!(
        weekly_targets(chest, 12, VolumeRole::Priority, 4).value[0],
        16
    );
    assert_eq!(
        weekly_targets(chest, 12, VolumeRole::Maintenance, 4).value,
        vec![8; 4]
    );
    assert_eq!(
        volume_role("chest", Some("Chest"), TrainingAge::Intermediate).value,
        VolumeRole::Standard,
        "specialization is gated to advanced lifters"
    );
    assert_eq!(
        volume_role("chest", Some("lats"), TrainingAge::Advanced).value,
        VolumeRole::Maintenance
    );
    assert_eq!(
        volume_role("back", Some("lats"), TrainingAge::Advanced).value,
        VolumeRole::Priority
    );
}

#[test]
fn rep_ranges_sit_inside_table_2() {
    for class in [
        ExerciseClass::HeavyCompound,
        ExerciseClass::ModerateCompound,
        ExerciseClass::Isolation,
    ] {
        let (lo, hi) = rep_range_for(class).value;
        let window = rep_load(class).value.reps;
        assert!(lo >= window.0 && hi <= window.1, "{class:?}");
        assert!(hi < 25, "never at the 25-rep edge");
    }
    assert_eq!(pct_1rm_for_reps_in_reserve(8, 2).round(), 75.0);
    assert_eq!(pct_1rm_for_reps_in_reserve(6, 2).round(), 79.0);
}
