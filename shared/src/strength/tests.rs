use super::*;
use crate::schema::EvidenceGrade;

#[test]
fn lombardi_and_backoff_and_vl() {
    assert!((e1rm_lombardi(100.0, 1) - 100.0).abs() < 1e-9);
    assert!(e1rm_lombardi(100.0, 5) > 100.0);
    let b = rpe_anchored_back_off().value;
    assert_eq!((b.top_set_rpe, b.drop_frac), (8, (0.10, 0.15)));
    assert_eq!(vl_termination_threshold(LiftGoal::MaxStrength).value, 0.20);
    // Power splits out to its own 10% VL, distinct from the 20% max-strength group.
    assert_eq!(vl_termination_threshold(LiftGoal::Power).value, 0.10);
    assert_eq!(vl_termination_threshold(LiftGoal::Hypertrophy).value, 0.40);
}

#[test]
fn periodization_phase_tables() {
    let base = linear_phase_rx(LinearPhase::Base).value;
    assert_eq!(
        (base.pct_1rm, base.sets, base.reps, base.weeks),
        (Some((67, 75)), Some((3, 5)), Some((8, 12)), (1, 4))
    );
    assert_eq!(linear_phase_rx(LinearPhase::Taper).value.pct_1rm, None);
    // Linear Peak is "90-95%+": the intensity top end is open.
    let peak = linear_phase_rx(LinearPhase::Peak).value;
    assert!(peak.pct_top_open);
    assert!(!base.pct_top_open);
    assert!(!block_phase_rx(BlockPhase::Transmutation).value.pct_top_open);
    let acc = block_phase_rx(BlockPhase::Accumulation).value;
    assert_eq!(
        (acc.pct_1rm, acc.sets, acc.reps),
        (Some((65, 80)), Some((3, 5)), Some((6, 10)))
    );
    let real = block_phase_rx(BlockPhase::Realization).value;
    assert_eq!((real.pct_1rm, real.reps), (None, Some((1, 3))));
}

#[test]
fn depth_jump_gate_is_safety_critical() {
    let g = depth_jump_ready(150.0, 100.0);
    assert!(g.value);
    assert!(!depth_jump_ready(140.0, 100.0).value);
    assert!(!depth_jump_ready(200.0, 0.0).value);
    assert!(g.safety_critical());
    assert!(g.contested().is_some());
}

#[test]
fn power_peaking_specifics() {
    assert_eq!(deadlift_peak_days_out().value, (10, 14));
    // strength-034: heavy-CA rest is ~5-7 min (>=5), not the overall 3-7
    // window.
    assert_eq!(pap_rest_window_min().value, (5, 7));
    let ol = olympic_derivative_rx().value;
    assert_eq!((ol.pct_1rm, ol.reps, ol.sets), ((85, 100), (1, 3), (3, 5)));
    // "85-100%+": the top of the band is open-ended.
    assert!(ol.pct_top_open);
    assert!(ol.early_in_session);
    // KB-stated velocity-biased variant loads only.
    assert_eq!(ol.velocity_variant_pcts_1rm, &[30, 45, 65, 80]);
    assert_eq!(ol.jump_shrug_pct_bodymass, 30);
    // Rest comes from the KB POWER column (3-5 min).
    assert_eq!(ol.rest_sec, (180, 300));
}

#[test]
fn loading_bands_match_file02() {
    let s = loading_rx(LiftGoal::MaxStrength).value;
    assert_eq!(
        (s.pct_1rm, s.reps, s.rir),
        ((80, 100), (1, 5), Some((1, 3)))
    );
    let h = loading_rx(LiftGoal::Hypertrophy).value;
    assert_eq!(
        (h.pct_1rm, h.reps, h.rest_sec, h.rir),
        ((65, 85), (6, 12), (30, 120), Some((0, 3)))
    );
    // Strength intensity floor sits above hypertrophy's.
    assert!(
        loading_rx(LiftGoal::MaxStrength).value.pct_1rm.0
            > loading_rx(LiftGoal::Hypertrophy).value.pct_1rm.0
    );
    // Power carries strength-002's own Moderate grade (STR-PWR-001), not
    // the Strong intensity-primacy claim.
    let p = loading_rx(LiftGoal::Power);
    assert_eq!(p.grade(), EvidenceGrade::Moderate);
    assert_eq!(p.claim_id(), claims::STR_PWR_001);
    // Power band is the envelope of the KB spectrum (0-60% ballistic,
    // 30-70% loaded power, 70-95% pulls), not one collapsed class band.
    assert_eq!(p.value.pct_1rm, (0, 95));
    // The Power band carries no KB-sanctioned numeric RIR.
    assert!(p.value.rir.is_none(), "power has no numeric RIR band");
}

#[test]
fn two_for_two_and_progression() {
    assert!(two_for_two_met(2, 2).value);
    assert!(!two_for_two_met(2, 1).value);
    assert!(!two_for_two_met(1, 3).value);
    // strength-012 is safety-critical in the KB (caps load ramp speed).
    assert!(two_for_two_met(2, 2).safety_critical());
    assert_eq!(weekly_pct_increment(true).value, (0.01, 0.025));
    assert_eq!(weekly_pct_increment(false).value, (0.025, 0.05));
    assert!(stall_triggers_deload(3, true).value);
    assert!(!stall_triggers_deload(3, false).value);
    assert!(!stall_triggers_deload(1, true).value);
}

#[test]
fn periodization_by_training_age() {
    use crate::individualization::TrainingAge;
    assert_eq!(
        periodization_model(TrainingAge::Novice).value,
        PeriodizationModel::Linear
    );
    assert_eq!(
        periodization_model(TrainingAge::Intermediate).value,
        PeriodizationModel::Dup
    );
    assert_eq!(
        periodization_model(TrainingAge::Advanced).value,
        PeriodizationModel::Block
    );
    // strength-010 is a Moderate, uncontested synthesis (STR-MODEL-001) -
    // its grade must not be inflated to PERIOD-001's Strong.
    let m = periodization_model(TrainingAge::Novice);
    assert_eq!(m.grade(), EvidenceGrade::Moderate);
    assert!(m.contested().is_none());
    // Block phases stay contested via the global model-superiority CQ-03.
    let b = block_phase_rx(BlockPhase::Accumulation);
    assert_eq!(b.contested(), Some("CQ-03"));
}

#[test]
fn taper_holds_intensity() {
    let t = taper_rx().value;
    assert_eq!(t.volume_reduction_frac, (0.41, 0.60));
    assert_eq!(t.duration_days, (8, 14));
    assert!(t.hold_intensity);
}

#[test]
fn plyo_caps_rise_with_level() {
    use crate::individualization::TrainingAge;
    assert_eq!(plyo_foot_contact_cap(TrainingAge::Novice).value, (80, 100));
    assert_eq!(
        plyo_foot_contact_cap(TrainingAge::Advanced).value,
        (120, 140)
    );
    assert!(
        plyo_foot_contact_cap(TrainingAge::Advanced).value.1
            > plyo_foot_contact_cap(TrainingAge::Novice).value.1
    );
}

#[test]
fn epley_100kg_5reps_is_about_116_7() {
    let e = e1rm_epley(100.0, 5);
    assert!((e - 116.666_666).abs() < 1e-3, "got {e}");
}

#[test]
fn brzycki_sanity() {
    // 100 kg x 5 -> 100 * 36 / 32 = 112.5
    let b = e1rm_brzycki(100.0, 5);
    assert!((b - 112.5).abs() < 1e-6, "got {b}");
    // A single rep must equal (near) the load itself: 100 * 36 / 36 = 100.
    assert!((e1rm_brzycki(100.0, 1) - 100.0).abs() < 1e-6);
    // Higher reps estimate a higher 1RM than fewer reps at equal load.
    assert!(e1rm_brzycki(100.0, 8) > e1rm_brzycki(100.0, 3));
}

#[test]
fn brzycki_high_reps_stay_finite() {
    // At/above 37 reps the raw denominator (37 − reps) is zero/negative,
    // which would return +∞ or a negative 1RM. The domain clamp keeps every
    // rep count finite and positive (BUGS: e1rm_brzycki unbounded).
    let at_cap = e1rm_brzycki(100.0, BRZYCKI_MAX_REPS); // reps = 36
    assert!(at_cap.is_finite() && at_cap > 0.0, "got {at_cap}");
    for reps in [37u32, 40, 100] {
        let b = e1rm_brzycki(100.0, reps);
        assert!(b.is_finite() && b > 0.0, "reps {reps} gave {b}");
        // Above the domain the estimate saturates at the cap value rather
        // than exploding.
        assert_eq!(b, at_cap, "reps {reps} should clamp to the cap");
    }
    // The clamp must not perturb results inside the valid domain.
    assert!((e1rm_brzycki(100.0, 5) - 112.5).abs() < 1e-6);
}

#[test]
fn rpe_to_rir_anchor() {
    assert_eq!(rpe_to_rir(8.0), 2.0);
    assert_eq!(rpe_to_rir(10.0), 0.0);
    // Clamp: overshoot stays at 0 RIR.
    assert_eq!(rpe_to_rir(11.0), 0.0);
    // Round trip.
    assert_eq!(rir_to_rpe(2.0), 8.0);
    assert_eq!(rir_to_rpe(0.0), 10.0);
    // Clamp: a RIR beyond the anchor floors RPE at 0.
    assert_eq!(rir_to_rpe(12.0), 0.0);
}

#[test]
fn est_pct_from_reps_clamps_domain_edges_and_estimates() {
    // A true 1RM reads as its own 100 %, not the Epley inverse's 96.77 %
    // (the known r=1 discontinuity is clamped; File 02 strength-005).
    assert_eq!(est_pct_1rm(1, 0.0), 100.0);
    // The reps==0 sentinel returns 0.0 in ALL builds — no debug_assert
    // panics before it (this test compiles/runs in debug too).
    assert_eq!(est_pct_1rm(0, 0.0), 0.0);
    // The Epley inverse governs ≥2 reps: 100 / (1 + 2/30) = 93.75 %.
    assert!((est_pct_1rm(2, 0.0) - 93.75).abs() < 1e-2);
    // Monotonic decrease with more reps (above the r=1 clamp).
    assert!(est_pct_1rm(10, 0.0) < est_pct_1rm(3, 0.0));
}

#[test]
fn prilepin_for_70pct_returns_correct_row() {
    let row = prilepin_for(70.0).expect("70% resolves");
    assert_eq!(row.pct_min, 70);
    assert_eq!(row.pct_max, 79);
    assert_eq!(row.reps_per_set, (3, 6));
    assert_eq!(row.optimal_total, 18);
    assert_eq!(row.total_range, (12, 24));
    // Boundaries and out-of-range.
    assert_eq!(prilepin_for(95.0).unwrap().optimal_total, 7);
    assert_eq!(prilepin_for(50.0).unwrap().optimal_total, 24);
    assert!(prilepin_for(-1.0).is_none());
    assert!(prilepin_for(f64::NAN).is_none());
    // The KB's ">90%" band has no upper bound: supra-max intensities
    // (accommodating resistance / overload work) resolve to it.
    assert_eq!(prilepin_for(101.0).unwrap().optimal_total, 7);
    assert_eq!(prilepin_for(125.0).unwrap().pct_min, 90);
}

#[test]
fn prilepin_volume_pass_and_fail() {
    // 85% band range is 10-20 total reps.
    assert!(prilepin_volume_ok(85.0, 15).value); // optimal
    assert!(prilepin_volume_ok(85.0, 10).value); // lower bound
    assert!(prilepin_volume_ok(85.0, 20).value); // upper bound
    assert!(!prilepin_volume_ok(85.0, 9).value); // under
    assert!(!prilepin_volume_ok(85.0, 25).value); // over
    // Supra-max resolves to the >90% band (4-10 total reps).
    assert!(prilepin_volume_ok(105.0, 7).value);
    assert!(!prilepin_volume_ok(105.0, 15).value);
    assert!(!prilepin_volume_ok(-5.0, 15).value); // invalid intensity
    // The governor carries the Prilepin claim (Moderate, contested CQ-03).
    let g = prilepin_volume_ok(85.0, 15);
    assert_eq!(g.claim_id(), claims::STR_PRILEPIN_001);
    assert_eq!(g.grade(), EvidenceGrade::Moderate);
    assert!(g.contested().is_some());
}

#[test]
fn prilepin_table_nonempty_and_sorted() {
    assert!(!PRILEPIN.is_empty());
    assert_eq!(PRILEPIN.len(), 4);
    // Ascending, non-overlapping, gap-free bands.
    for w in PRILEPIN.windows(2) {
        assert!(w[0].pct_max < w[1].pct_min);
        assert_eq!(w[0].pct_max + 1, w[1].pct_min);
    }
    // Full 0-100 coverage.
    assert_eq!(PRILEPIN.first().unwrap().pct_min, 0);
    assert_eq!(PRILEPIN.last().unwrap().pct_max, 100);
}

#[test]
fn recommend_attaches_registered_evidence() {
    let rx = graded(LiftDummy, claims::AUTOREG_RIR_001);
    assert_eq!(rx.grade(), EvidenceGrade::Strong);
    assert_eq!(rx.claim_id(), claims::AUTOREG_RIR_001);
}

#[derive(Debug, PartialEq)]
struct LiftDummy;

// -- File 02 rules --

#[test]
fn e1rm_reliability_gate_and_cross_check() {
    // strength-006: reliable up to 10 reps on compounds.
    assert!(e1rm_reliable(10, false).value);
    assert!(!e1rm_reliable(11, false).value); // >10 reps unreliable
    assert!(!e1rm_reliable(5, true).value); // isolation unreliable
    assert!(!e1rm_reliable(0, false).value); // no reps, no estimate
    // Cross-check spans >=2 formulas and brackets the single estimates.
    let c = e1rm_cross_check(100.0, 5, 0.0, false)
        .value
        .expect("reliable");
    assert!(c.formulas_used >= 2);
    assert!(c.low_kg <= e1rm_epley(100.0, 5) && e1rm_epley(100.0, 5) <= c.high_kg);
    assert!(c.low_kg <= e1rm_brzycki(100.0, 5) && e1rm_brzycki(100.0, 5) <= c.high_kg);
    assert!(c.low_kg <= c.high_kg);
    // Unreliability gate returns None instead of a number.
    assert!(e1rm_cross_check(100.0, 11, 0.0, false).value.is_none());
    assert!(e1rm_cross_check(100.0, 5, 0.0, true).value.is_none());
    assert!(e1rm_cross_check(0.0, 5, 0.0, false).value.is_none());
    assert!(e1rm_cross_check(100.0, 0, 8.0, false).value.is_none());
    // Preferred test sets are 3-6 reps.
    assert_eq!(e1rm_test_set_reps().value, (3, 6));
    // Evidence pin: Moderate, DiStasio.
    let r = e1rm_reliable(5, false);
    assert_eq!(r.grade(), EvidenceGrade::Moderate);
    assert_eq!(r.claim_id(), claims::STR_E1RM_CHECK_001);
}

#[test]
fn dup_days_match_kb_example_week() {
    // strength-023 verbatim: heavy 3-5x3-5 @85-90; power 3-6x3-5 @50-70
    // fast; hypertrophy 3-4x8-12 @70-75.
    let h = dup_day_rx(DupDay::Heavy).value;
    assert_eq!((h.pct_1rm, h.sets, h.reps), ((85, 90), (3, 5), (3, 5)));
    assert!(!h.max_velocity_intent);
    let p = dup_day_rx(DupDay::Power).value;
    assert_eq!((p.pct_1rm, p.sets, p.reps), ((50, 70), (3, 6), (3, 5)));
    assert!(p.max_velocity_intent);
    let hy = dup_day_rx(DupDay::Hypertrophy).value;
    assert_eq!((hy.pct_1rm, hy.sets, hy.reps), ((70, 75), (3, 4), (8, 12)));
    // Moderate (mixed), contested via the global model-superiority CQ-03.
    let r = dup_day_rx(DupDay::Heavy);
    assert_eq!(r.grade(), EvidenceGrade::Moderate);
    assert_eq!(r.contested(), Some("CQ-03"));
    assert!(!r.safety_critical());
}

#[test]
fn one_rm_test_gate_is_safety_critical() {
    let ready = OneRmTestContext {
        technically_proficient: true,
        adequately_recovered: true,
        warmed_up: true,
        is_novice: false,
        supervised: false,
        spinal_loading: false,
        bracing_competent: false,
    };
    assert!(one_rm_test_allowed(ready).value);
    // Each readiness gate blocks on its own.
    for ctx in [
        OneRmTestContext {
            technically_proficient: false,
            ..ready
        },
        OneRmTestContext {
            adequately_recovered: false,
            ..ready
        },
        OneRmTestContext {
            warmed_up: false,
            ..ready
        },
        // Novice without supervision.
        OneRmTestContext {
            is_novice: true,
            ..ready
        },
        // Spinal loading without bracing competence.
        OneRmTestContext {
            spinal_loading: true,
            ..ready
        },
    ] {
        assert!(!one_rm_test_allowed(ctx).value, "{ctx:?} must block");
    }
    // Novice + supervision and spinal + bracing pass.
    assert!(
        one_rm_test_allowed(OneRmTestContext {
            is_novice: true,
            supervised: true,
            ..ready
        })
        .value
    );
    assert!(
        one_rm_test_allowed(OneRmTestContext {
            spinal_loading: true,
            bracing_competent: true,
            ..ready
        })
        .value
    );
    // Novice jump caps: upper 2.5-5%, lower 5-10%.
    assert_eq!(novice_load_jump_cap_frac(true).value, (0.025, 0.05));
    assert_eq!(novice_load_jump_cap_frac(false).value, (0.05, 0.10));
    // Safety-critical, conservatively ExpertOpinion (KB: EO/Moderate 0.40).
    let r = one_rm_test_allowed(ready);
    assert!(r.safety_critical());
    assert_eq!(r.grade(), EvidenceGrade::ExpertOpinion);
    assert_eq!(r.claim_id(), claims::STR_1RMTEST_001);
}

#[test]
fn epley_single_is_identity() {
    // A single IS the 1RM, never +3.3 %.
    assert_eq!(e1rm_epley(140.0, 1), 140.0);
    assert_eq!(e1rm_epley_rpe(140.0, 1, 0.0), 140.0);
    assert_eq!(e1rm_epley_rpe(140.0, 1, 10.0), 140.0);
    assert_eq!(est_pct_1rm(1, 10.0), 100.0);
}

#[test]
fn half_point_rpe_not_rounded_up() {
    // RPE 9.5 = 0.5 RIR, not a whole extra rep.
    let half = e1rm_epley_rpe(100.0, 5, 9.5);
    assert!((half - 100.0 * (1.0 + 5.5 / 30.0)).abs() < 1e-9, "{half}");
    assert!(half < e1rm_epley_rpe(100.0, 5, 9.0));
    // A single at 9.5 interpolates between the single and the double.
    let single = e1rm_epley_rpe(100.0, 1, 9.5);
    assert!(single > 100.0 && single < e1rm_epley(100.0, 2), "{single}");
    // est_pct_1rm is the exact inverse.
    assert!((est_pct_1rm(5, 9.5) * half / 100.0 - 100.0).abs() < 1e-9);
}

#[test]
fn pct_for_week_ramps_inside_the_band() {
    let heavy: Vec<u8> = (1..=4)
        .map(|w| pct_for_week((85, 90), w, false, 3).value)
        .collect();
    assert_eq!(
        heavy,
        vec![85, 87, 88, 88],
        "a triple stops at 88 % (1 RIR)"
    );
    let upper: Vec<u8> = (1..=4)
        .map(|w| pct_for_week((85, 90), w, true, 3).value)
        .collect();
    assert_eq!(upper, vec![85, 86, 87, 88]);
    let heavy_volume: Vec<u8> = (1..=5)
        .map(|w| pct_for_week((80, 90), w, false, 5).value)
        .collect();
    assert_eq!(heavy_volume, vec![80, 82, 83, 83, 83], "×5 stops at 83 %");
    assert_eq!(
        pct_for_week((80, 100), 3, false, 5).claim_id(),
        claims::STR_PCTPROG_001
    );
}

#[test]
fn weekly_ramp_never_prescribes_below_one_rep_in_reserve() {
    // STR-INTENT-001 / HYP-FAIL-SAFE-001: at the reps each DUP day
    // prescribes, no week of a 5-week block implies <1 RIR on the Epley
    // model the engine reads effort from (strength-005).
    for day in [
        DupDay::Heavy,
        DupDay::Power,
        DupDay::Hypertrophy,
        DupDay::HeavyVolume,
    ] {
        let d = dup_day_rx(day).value;
        for reps in [d.reps.0, d.reps.1] {
            for upper in [false, true] {
                for week in 1..=5 {
                    let p = pct_for_week(d.pct_1rm, week, upper, reps).value;
                    let rir = expected_rir_at_pct(f64::from(p), u32::from(reps));
                    assert!(
                        rir >= 1.0,
                        "{day:?} ×{reps} week {week} upper={upper}: {p}% → {rir:.2} RIR"
                    );
                }
            }
        }
    }
}

#[test]
fn open_ended_effort_is_a_lower_bound_not_four_extra_reps() {
    // C11: "4+ reps left" (RPE 6) is outside the trusted 1–3 RIR window,
    // so the set counts its reps as performed, exactly like an unrated set.
    assert!(!rpe_is_exact(6.0));
    assert!(!rpe_is_exact(0.0));
    assert!(rpe_is_exact(7.0));
    assert_eq!(effective_reps_f(5, 6.0), 5.0);
    assert_eq!(e1rm_epley_rpe(100.0, 5, 6.0), e1rm_epley(100.0, 5));
    assert_eq!(effective_reps_f(5, 7.0), 8.0);
}

#[test]
fn cross_check_band_contains_the_rpe_adjusted_e1rm() {
    // 100 kg × 5 @ RPE 7 → 8 effective reps: the band is built on the
    // same reps as the headline, so the headline sits inside it.
    let headline = e1rm_epley_rpe(100.0, 5, 7.0);
    let c = e1rm_cross_check(100.0, 5, 7.0, false)
        .value
        .expect("reliable");
    assert!(
        c.low_kg <= headline && headline <= c.high_kg,
        "{headline} in {}..{}",
        c.low_kg,
        c.high_kg
    );
    // 9 reps @ RPE 6 is a lower bound (9 reps), still inside the window;
    // 9 reps @ RPE 8 is 11 effective reps: unreliable, like the anchor gate.
    assert!(e1rm_cross_check(100.0, 9, 6.0, false).value.is_some());
    assert!(e1rm_cross_check(100.0, 9, 8.0, false).value.is_none());
}

#[test]
fn effort_target_follows_the_epley_model() {
    // 85 % × 3 → ~2.3 reps left → 2 RIR → RPE 8.
    assert_eq!(effort_target_rir(85.0, 3), 2);
    assert_eq!(rir_to_rpe(effort_target_rir(85.0, 3) as f64), 8.0);
    // Never 0: a 90 % triple still shows 1 in reserve.
    assert_eq!(effort_target_rir(90.0, 3), 1);
    // Speed work is capped at the 4-RIR end of the table.
    assert_eq!(effort_target_rir(50.0, 3), 4);
}

#[test]
fn lift_detraining_is_modest_never_zero() {
    // DETRAIN-STR-001: full credit ≤30 d, 70 % at 31–56 d, dropped (fresh
    // novice start, REENTRY-001) past 8 weeks.
    assert_eq!(lift_detrain_credit(30).value, Some(1.0));
    assert_eq!(lift_detrain_credit(44).value, Some(0.70));
    assert_eq!(lift_detrain_credit(56).value, Some(0.70));
    assert_eq!(lift_detrain_credit(57).value, None);
    assert_eq!(lift_detrain_credit(44).claim_id(), claims::DETRAIN_STR_001);
}

#[test]
fn heavy_volume_day_is_the_transmutation_low_end() {
    let hv = dup_day_rx(DupDay::HeavyVolume);
    assert_eq!(hv.value.pct_1rm.0, 80);
    assert_eq!(hv.value.reps.0, 5);
    assert_eq!(hv.claim_id(), claims::STR_BLOCK_001);
}
