use super::*;
use crate::load::{daniels_predict, daniels_speed_for_vo2, daniels_vo2, pace_sec_per_km_at, vdot};

#[test]
fn tanaka_at_age_30_is_about_187() {
    // 208 - 0.7*30 = 187.0
    let hr = hr_max_tanaka(30.0);
    assert!((hr - 187.0).abs() < 1e-9, "got {hr}");
}

#[test]
fn three_zone_boundaries() {
    assert_eq!(classify_three_zone(70.0), ThreeZone::Z1);
    assert_eq!(classify_three_zone(81.9), ThreeZone::Z1);
    // 82 is the LT1 boundary → Z2 (inclusive lower edge).
    assert_eq!(classify_three_zone(82.0), ThreeZone::Z2);
    assert_eq!(classify_three_zone(87.9), ThreeZone::Z2);
    // 88 is the LT2 boundary → Z3 (inclusive to the hard zone), so a run at
    // the prescribed Tempo floor (0.88) counts as threshold work.
    assert_eq!(classify_three_zone(88.0), ThreeZone::Z3);
    assert_eq!(classify_three_zone(88.1), ThreeZone::Z3);
    assert_eq!(classify_three_zone(95.0), ThreeZone::Z3);
}

#[test]
fn vdot_table_values_verbatim() {
    assert_eq!(vdot_band_hr_pct(VdotBand::Easy), (65.0, 79.0));
    assert_eq!(vdot_band_vo2max_pct(VdotBand::Marathon), (80.0, Some(84.0)));
    assert_eq!(vdot_band_hr_pct(VdotBand::Threshold), (88.0, 92.0));
    assert!(!vdot_band_uses_hr(VdotBand::Repetition));
    assert!(vdot_band_uses_hr(VdotBand::Threshold));
    // R is >100 %VO2max: open upper bound.
    assert_eq!(vdot_band_vo2max_pct(VdotBand::Repetition).1, None);
}

#[test]
fn long_run_cap_pass_and_fail() {
    let long_run = |km: f64, weekly: f64| check_volume_caps(km, 0.0, 0.0, weekly).value.long_run;
    // 12 km of a 50 km week = 24 % ≤ 25 %: pass.
    assert!(!long_run(12.0, 50.0));
    // Exactly 25 %: pass (cap is inclusive).
    assert!(!long_run(12.5, 50.0));
    // 15 km of 50 km = 30 %: fail.
    assert!(long_run(15.0, 50.0));
    // Zero weekly volume cannot satisfy a share cap.
    assert!(long_run(5.0, 0.0));
}

#[test]
fn spike_true_just_above_10pct_false_at_or_below() {
    // 10 % over exactly: not a spike (rule is strictly >10 %).
    assert!(!single_session_spike(22.0, 20.0));
    assert!(single_session_spike(22.01, 20.0));
    assert!(!single_session_spike(18.0, 20.0));
    // No history → any real session errs to spike.
    assert!(single_session_spike(5.0, 0.0));
}

#[test]
fn hr_zone_recalc_due_from_four_weeks() {
    assert!(!hr_zone_recalc_due(3).value);
    assert!(hr_zone_recalc_due(4).value);
}

#[test]
fn vdot_derate_and_goal_plan() {
    assert_eq!(
        vdot_derate_points(GoalDistance::HalfMarathon, true).value,
        (1.0, 1.5)
    );
    assert_eq!(
        vdot_derate_points(GoalDistance::Marathon, true).value,
        (2.0, 3.0)
    );
    assert_eq!(
        vdot_derate_points(GoalDistance::FiveK, true).value,
        (0.0, 0.0)
    );
    assert_eq!(
        vdot_derate_points(GoalDistance::Marathon, false).value,
        (0.0, 0.0)
    );
    let c = goal_week_plan(GoalDistance::C25k, false).value;
    assert_eq!((c.sessions_per_week, c.quality_per_week), ((3, 3), (0, 0)));
    // running-025 defines no long run for C25K: the share is 0, not 25-30%.
    assert_eq!(c.long_run_share, (0.0, 0.0));
    assert_eq!(
        goal_week_plan(GoalDistance::FiveK, false)
            .value
            .quality_per_week,
        (2, 2)
    );
    assert_eq!(
        goal_week_plan(GoalDistance::Marathon, true)
            .value
            .sessions_per_week,
        (5, 7)
    );
    assert_eq!(
        goal_week_plan(GoalDistance::Marathon, false)
            .value
            .sessions_per_week,
        (4, 6)
    );
    // marathon_derated_band (running-040/008, option B): the optimistic
    // band is shifted SLOWER, the fast bound by the min derate and the slow
    // bound by the max, so the range moves later and widens, and the
    // result carries the same RUN-VDOT-001 evidence as the derate itself.
    let vdot = crate::load::vdot(10_000.0, 2_520.0); // a real 42:00 10K
    let base_low = 12_000.0;
    let base_high = 12_600.0;
    let band = marathon_derated_band(base_low, base_high, vdot);
    assert!(
        band.value.0 > base_low && band.value.1 > base_high,
        "both bounds slow down: {:?} vs ({base_low}, {base_high})",
        band.value
    );
    assert!(
        band.value.1 - band.value.0 > base_high - base_low,
        "the derate widens the span by its own uncertainty: {:?}",
        band.value
    );
    assert_eq!(
        band.claim_id(),
        claims::RUN_VDOT_001,
        "the derated number still cites running-040/008 (HARD RULE 2)"
    );
    // A degenerate prediction (no valid race → VDOT 0) is left untouched,
    // never fabricated slower.
    let degen = marathon_derated_band(0.0, 0.0, 0.0);
    assert_eq!(degen.value, (0.0, 0.0));
    // A VDOT the 2–3 point derate would take to zero keeps its band: no
    // bound collapses to a 0:00 finish.
    let tiny = marathon_derated_band(36_000.0, 40_000.0, 2.5);
    assert_eq!(tiny.value, (36_000.0, 40_000.0));

    let p = c25k_plan().value;
    assert_eq!(
        (
            p.runs_per_week,
            p.rest_day_between,
            p.repeat_hard_week_allowed
        ),
        (3, true, true)
    );
}

#[test]
fn distribution_and_progression_guards() {
    assert!(easy_share_floor_ok(0.80).value);
    assert!(!easy_share_floor_ok(0.79).value);
    assert_eq!(novice_volume_bump_hold_weeks(0.5).value, Some((2, 3)));
    assert_eq!(novice_volume_bump_hold_weeks(3.0).value, None);
}

#[test]
fn maf_cap_adjusts() {
    assert_eq!(maf_cap_bpm(40.0, MafAdjustment::None).value, 140.0);
    assert_eq!(
        maf_cap_bpm(40.0, MafAdjustment::EliteImproving).value,
        145.0
    );
    assert_eq!(maf_cap_bpm(40.0, MafAdjustment::Returning).value, 135.0);
    assert_eq!(maf_cap_bpm(40.0, MafAdjustment::Overtrained).value, 130.0);
    // Weak + contested per global CQ-12 (MAF vs measured LT1).
    assert!(maf_cap_bpm(40.0, MafAdjustment::None).contested().is_some());
}

#[test]
fn distribution_models_keep_easy_at_80() {
    let pyr = intensity_distribution(DistributionModel::Pyramidal).value;
    assert_eq!((pyr.easy_pct, pyr.moderate_pct, pyr.hard_pct), (80, 15, 5));
    let pol = intensity_distribution(DistributionModel::Polarized).value;
    assert_eq!((pol.easy_pct, pol.moderate_pct, pol.hard_pct), (80, 5, 15));
    // Phase mapping: base pyramidal, peak polarized.
    assert_eq!(distribution_for_phase(MesoPhase::Base).value.hard_pct, 5);
    assert_eq!(distribution_for_phase(MesoPhase::Peak).value.hard_pct, 15);
}

#[test]
fn quality_plan_respects_caps() {
    // 3 sessions, 48h apart, no back-to-back Z3: ok.
    assert!(quality_plan_ok(3, 48, false).value);
    assert!(!quality_plan_ok(4, 48, false).value);
    assert!(!quality_plan_ok(2, 24, false).value);
    // Consecutive Z3 never allowed.
    assert!(!quality_plan_ok(2, 48, true).value);
}

#[test]
fn weekly_increase_cap_scales_with_training_age() {
    assert!((weekly_increase_cap_frac(0.5).value - 0.10).abs() < 1e-9);
    assert!((weekly_increase_cap_frac(3.0).value - 0.05).abs() < 1e-9);
    // Novice +10% ok, experienced +10% not.
    assert!(weekly_increase_ok(50.0, 55.0, 0.5).value);
    assert!(!weekly_increase_ok(50.0, 55.0, 3.0).value);
    // Zero base cannot be ratioed.
    assert!(!weekly_increase_ok(0.0, 10.0, 0.5).value);
}

#[test]
fn two_week_spike_flags_above_30pct() {
    // +30% exactly: not flagged (rule is strictly >30%).
    assert!(!two_week_increase_flag(40.0, 52.0).value);
    assert!(two_week_increase_flag(40.0, 52.1).value);
    // No history errs to flag.
    assert!(two_week_increase_flag(0.0, 10.0).value);
}

#[test]
fn deload_cadence_defaults_and_conservative() {
    let default = deload_cadence(false).value;
    assert_eq!((default.load_weeks, default.recovery_weeks), (3, 1));
    assert_eq!(deload_cadence(true).value.load_weeks, 2);
    let (lo, hi) = default.reduction_frac;
    assert!((lo - 0.20).abs() < 1e-9 && (hi - 0.40).abs() < 1e-9);
}

#[test]
fn check_volume_caps_flags_every_violation() {
    let ok = check_volume_caps(12.0, 4.0, 3.0, 50.0).value;
    assert!(!(ok.long_run || ok.threshold || ok.interval));
    // Long run 40 %, threshold 16 %, interval 6 %: the first two are set.
    let bad = check_volume_caps(20.0, 8.0, 3.0, 50.0).value;
    assert_eq!(
        bad,
        CapViolations {
            long_run: true,
            threshold: true,
            interval: false
        }
    );
}

#[test]
fn workout_rx_hr_governance_split() {
    // Aerobic sessions are HR-governed.
    assert!(workout_rx(RunSessionType::Recovery).value.hr_governed);
    assert!(workout_rx(RunSessionType::LongRun).value.hr_governed);
    assert!(workout_rx(RunSessionType::Tempo).value.hr_governed);
    assert!(workout_rx(RunSessionType::RacePace).value.hr_governed);
    assert!(workout_rx(RunSessionType::Easy).value.hr_governed);
    // Short/max efforts are effort-governed, HR lags.
    assert!(!workout_rx(RunSessionType::Interval).value.hr_governed);
    assert!(!workout_rx(RunSessionType::Repetition).value.hr_governed);
    assert!(!workout_rx(RunSessionType::Strides).value.hr_governed);
    assert!(!workout_rx(RunSessionType::Hills).value.hr_governed);
}

#[test]
fn workout_rx_bands_verbatim() {
    // running-018 Tempo 88–92 %HRmax, RPE 6–7, 20–40 min continuous.
    let t = workout_rx(RunSessionType::Tempo).value;
    assert_eq!(t.pct_hr_max, (Some(0.88), Some(0.92)));
    assert_eq!(t.rpe, Some((6, 7)));
    assert_eq!(t.duration_min, (Some(20), Some(40)));
    // VDOT-table Interval 97–100 %HRmax (matches `vdot_band_hr_pct`), 3–5 min.
    let i = workout_rx(RunSessionType::Interval).value;
    assert_eq!(i.pct_hr_max, (Some(0.97), Some(1.00)));
    assert_eq!(i.duration_min, (Some(3), Some(5)));
    assert_eq!(i.rpe, Some((8, 9)));
}

#[test]
fn recovery_rx_has_open_lower_hr_and_both_ceilings() {
    // running-014: KB gives NO lower HR bound, <76 %HRmax AND <70 %HRR
    // ceilings, pace ≥20 % slower than MP with no upper slowness bound,
    // RPE 2–3, 20–40 min.
    let r = workout_rx(RunSessionType::Recovery);
    assert_eq!(r.value.pct_hr_max, (None, Some(0.76)));
    assert_eq!(r.value.pct_hrr_max, Some(0.70));
    assert_eq!(r.value.pct_slower_than_mp, (Some(20.0), None));
    assert_eq!(r.value.rpe, Some((2, 3)));
    assert_eq!(r.value.duration_min, (Some(20), Some(40)));
    assert_eq!(r.claim_id(), claims::RUN_RECOVERY_001);
}

#[test]
fn easy_ga_is_distinct_from_recovery() {
    // running-015: E pace 15–25 % slower than MP, 65–79 %HRmax, RPE 3–4,
    // 30–90 min, a different band set from Recovery on every axis.
    let e = workout_rx(RunSessionType::Easy);
    assert_eq!(e.value.pct_hr_max, (Some(0.65), Some(0.79)));
    assert_eq!(e.value.pct_slower_than_mp, (Some(15.0), Some(25.0)));
    assert_eq!(e.value.rpe, Some((3, 4)));
    assert_eq!(e.value.duration_min, (Some(30), Some(90)));
    assert_eq!(e.claim_id(), claims::RUN_EASY_001);
    assert_ne!(e.value, workout_rx(RunSessionType::Recovery).value);
}

#[test]
fn long_run_rx_has_no_duration_floor() {
    // running-016: no duration floor in the KB; soft time-cap ~2:00–2:30 →
    // 150-min outer ceiling; pace MP −10–20 %; 65–80 %HRmax; RPE 3–5.
    let lr = workout_rx(RunSessionType::LongRun).value;
    assert_eq!(lr.duration_min, (None, Some(150)));
    assert_eq!(lr.pct_hr_max, (Some(0.65), Some(0.80)));
    assert_eq!(lr.pct_slower_than_mp, (Some(10.0), Some(20.0)));
    assert_eq!(lr.rpe, Some((3, 5)));
}

#[test]
fn race_pace_rx_is_km_blocks_not_minutes() {
    // running-017: 8–26 km blocks (KB states km, not minutes), M pace,
    // 80–85 %HRmax, RPE 5–6. No weekly M cap exists (VDOT M row: "-").
    let m = workout_rx(RunSessionType::RacePace).value;
    assert_eq!(m.distance_km, (Some(8.0), Some(26.0)));
    assert_eq!(m.duration_min, (None, None));
    assert_eq!(m.pct_hr_max, (Some(0.80), Some(0.85)));
    assert_eq!(m.pct_slower_than_mp, (Some(0.0), Some(0.0)));
    assert_eq!(m.rpe, Some((5, 6)));
}

#[test]
fn repetition_rx_carries_only_kb_backed_parameters() {
    // VDOT R row only: reps ≤2 min, pace-not-HR. RPE is NOT stated in the
    // KB and no HR band exists.
    let r = workout_rx(RunSessionType::Repetition);
    assert_eq!(r.value.rpe, None);
    assert_eq!(r.value.pct_hr_max, (None, None));
    assert_eq!(r.value.duration_min, (None, Some(2)));
    assert!(!r.value.hr_governed);
    assert_eq!(r.claim_id(), claims::RUN_REPETITION_001);
}

#[test]
fn cruise_intervals_verbatim() {
    // running-018: 3–15 min reps, rest ~1 min or 20–25 %, total ≤10 %.
    let c = cruise_interval_rx().value;
    assert_eq!(c.rep_duration_min, (3, 15));
    assert!((c.rest_approx_min - 1.0).abs() < 1e-9);
    assert_eq!(c.rest_frac, (0.20, 0.25));
    assert!((c.weekly_cap_frac - 0.10).abs() < 1e-9);
}

#[test]
fn strides_and_hill_sprints_verbatim() {
    let s = strides_rx().value;
    assert_eq!(
        (s.rep_sec, s.reps, s.recovery_sec, s.per_week, s.rpe),
        ((15, 30), (4, 8), (45, 120), (1, 3), (6, 7))
    );
    let h = hill_sprint_rx().value;
    assert_eq!((h.rep_sec, h.reps), ((8, 20), (4, 10)));
    assert_eq!(h.effort_pct, (90.0, 95.0));
    assert_eq!(h.grade_pct, (6.0, 10.0));
    assert_eq!(h.recovery_approx_sec, 120);
    assert_eq!(h.rpe, 9);
    // Weekly frequency is NOT stated in the KB → never invented.
    assert_eq!(h.per_week, None);
    assert!(h.on_easy_days);
    // Placement contradiction (Lydiard vs Hudson) must ride along.
    assert!(!hill_sprint_rx().evidence().contradicting.is_empty());
}

#[test]
fn long_run_guardrails() {
    // running-016: LR ≤ 2× average daily run.
    assert!(long_run_within_daily_avg(16.0, 8.0).value);
    assert!(!long_run_within_daily_avg(16.1, 8.0).value);
    // No history errs safe.
    assert!(!long_run_within_daily_avg(10.0, 0.0).value);
    // long_run_share_default() carries RUN-LONGRUN-001 / running-016's 25–30 %.
    assert_eq!(long_run_share_default().value, (0.25, 0.30));
    // goal_week_plan uses the 25–30% band (running-016 over running-024's
    // uncited 20% lower bound).
    assert_eq!(
        goal_week_plan(GoalDistance::General, false)
            .value
            .long_run_share,
        (0.25, 0.30)
    );
}

#[test]
fn hr_method_preference_branches() {
    // running-005: <55 prefer Karvonen; ≥70 converged; [55,70) unstated.
    assert_eq!(
        hr_method_preference(54.9).value,
        HrMethodPreference::PreferKarvonen
    );
    assert_eq!(
        hr_method_preference(55.0).value,
        HrMethodPreference::Unstated
    );
    assert_eq!(
        hr_method_preference(69.9).value,
        HrMethodPreference::Unstated
    );
    assert_eq!(
        hr_method_preference(70.0).value,
        HrMethodPreference::EitherConverged
    );
}

#[test]
fn recovery_week_depth_by_mileage() {
    // running-033: general 20–40 %, higher-mileage 10–30 %, lower-mileage
    // up to 50 % (floor unstated); both volume and intensity reduced, one
    // quality session dropped.
    let g = recovery_week_rx(MileageBand::Unspecified).value;
    assert_eq!(g.volume_reduction_frac, (Some(0.20), 0.40));
    let hi = recovery_week_rx(MileageBand::Higher).value;
    assert_eq!(hi.volume_reduction_frac, (Some(0.10), 0.30));
    let lo = recovery_week_rx(MileageBand::Lower).value;
    assert_eq!(lo.volume_reduction_frac, (None, 0.50));
    assert!(g.reduce_intensity && g.drop_quality_session);
}

#[test]
fn distance_taper_defaults_verbatim() {
    // running-038 (Strong, safety-critical): distance-specific defaults.
    let five = distance_taper(GoalDistance::FiveK).expect("5K default");
    assert_eq!(five.value.days, (7, 10));
    assert_eq!(five.value.volume_cut_frac, (0.40, 0.50));
    assert_eq!(five.value.keep_sharp_sessions, Some((1, 2)));
    assert!(five.safety_critical());
    let ten = distance_taper(GoalDistance::TenK).expect("10K default");
    assert_eq!(ten.value.days, (7, 10));
    let hm = distance_taper(GoalDistance::HalfMarathon).expect("HM default");
    assert_eq!(hm.value.days, (10, 14));
    assert_eq!(hm.value.volume_cut_frac, (0.50, 0.50));
    assert!(!hm.value.progressive);
    let m = distance_taper(GoalDistance::Marathon).expect("marathon default");
    assert_eq!(m.value.days, (14, 21));
    assert_eq!(m.value.volume_cut_frac, (0.40, 0.60));
    assert!(m.value.progressive && m.value.keep_mp_touches_and_short_tempo);
    // Intensity + frequency always held; no defaults stated for
    // general/C25K goals.
    assert!(m.value.hold_intensity && m.value.hold_frequency);
    assert!(distance_taper(GoalDistance::General).is_none());
    assert!(distance_taper(GoalDistance::C25k).is_none());
}

#[test]
fn pace_retest_and_environment_triggers() {
    // running-041: re-test every 4–6 wk.
    assert!(!pace_retest_due(3).value);
    assert!(pace_retest_due(4).value);
    // Race input freshness window ≤6–8 wk.
    assert_eq!(race_input_freshness(6).value, RaceInputFreshness::Fresh);
    assert_eq!(race_input_freshness(7).value, RaceInputFreshness::Marginal);
    assert_eq!(race_input_freshness(8).value, RaceInputFreshness::Marginal);
    assert_eq!(race_input_freshness(9).value, RaceInputFreshness::Stale);
    // Corrections trigger above ~15 °C / ~900 m, flags only, the KB
    // states no magnitudes.
    let t = pace_correction_triggers(15.1, 900.1).value;
    assert!(t.heat && t.altitude);
    let none = pace_correction_triggers(15.0, 900.0).value;
    assert!(!none.heat && !none.altitude);
}

#[test]
fn marathon_gate_trips_under_30km() {
    assert!(marathon_prediction_optimistic(24.0).value);
    assert!(!marathon_prediction_optimistic(32.0).value);
    // Boundary: exactly 30 km is not optimistic.
    assert!(!marathon_prediction_optimistic(30.0).value);
}

#[test]
fn equivalency_agrees_or_ranges() {
    // Within 2 %: midpoint.
    match race_equivalency(3600.0, 3636.0).value {
        Equivalency::Agreed(m) => assert!((m - 3618.0).abs() < 1e-9),
        other => panic!("expected Agreed, got {other:?}"),
    }
    // >2 % apart: range, low first.
    match race_equivalency(3800.0, 3600.0).value {
        Equivalency::Range(lo, hi) => {
            assert!((lo - 3600.0).abs() < 1e-9 && (hi - 3800.0).abs() < 1e-9)
        }
        other => panic!("expected Range, got {other:?}"),
    }
}

#[test]
fn equivalency_guards_the_zero_sentinel() {
    // A 0.0 prediction (undefined) must not divide-by-zero into Inf/NaN; the
    // span is returned as a plain range instead.
    for (r, d) in [(0.0, 3600.0), (3600.0, 0.0), (0.0, 0.0)] {
        match race_equivalency(r, d).value {
            Equivalency::Range(lo, hi) => {
                assert!(lo.is_finite() && hi.is_finite(), "no Inf/NaN: {lo}, {hi}");
            }
            Equivalency::Agreed(m) => panic!("a sentinel input must range, got Agreed({m})"),
        }
    }
}

#[test]
fn equivalency_cites_the_equivalency_claim_not_vdot() {
    // running-039 is the Riegel/Daniels equivalency combiner (RUN-EQUIV-001),
    // NOT the VDOT-fitness estimate (RUN-VDOT-001).
    let r = race_equivalency(3600.0, 3636.0);
    assert_eq!(r.claim_id(), claims::RUN_EQUIV_001,);
    assert_eq!(r.grade(), crate::evidence::EvidenceGrade::Moderate);
}

/// The NHS Couch to 5K bouts, (run s, walk s) for each run of weeks 1–9
/// (extracted/13-running-update.md:101); a single bout is a continuous run.
pub(crate) const NHS_C25K: [[&[(u32, u32)]; 3]; 9] = {
    const WK1: &[(u32, u32)] = &[(60, 90); 8];
    const WK2: &[(u32, u32)] = &[(90, 120); 6];
    const WK3: &[(u32, u32)] = &[(90, 90), (180, 180), (90, 90), (180, 180)];
    const WK4: &[(u32, u32)] = &[(180, 90), (300, 150), (180, 90), (300, 0)];
    [
        [WK1, WK1, WK1],
        [WK2, WK2, WK2],
        [WK3, WK3, WK3],
        [WK4, WK4, WK4],
        [
            &[(300, 180), (300, 180), (300, 0)],
            &[(480, 300), (480, 0)],
            &[(1200, 0)],
        ],
        [
            &[(300, 180), (480, 180), (300, 0)],
            &[(600, 180), (600, 0)],
            &[(1500, 0)],
        ],
        [&[(1500, 0)], &[(1500, 0)], &[(1500, 0)]],
        [&[(1680, 0)], &[(1680, 0)], &[(1680, 0)]],
        [&[(1800, 0)], &[(1800, 0)], &[(1800, 0)]],
    ]
};

#[test]
fn c25k_matches_the_nhs_bout_table() {
    let bouts = |w: u8, r: u8| -> Vec<(u32, u32)> {
        c25k_session_rx(w, r)
            .value
            .bouts
            .iter()
            .map(|b| (u32::from(b.run_sec), u32::from(b.walk_sec)))
            .collect()
    };
    for (w, week) in (1..).zip(NHS_C25K) {
        for (r, want) in (1..).zip(week) {
            assert_eq!(bouts(w, r), want, "week {w} run {r}");
        }
    }
    assert_eq!(
        bouts(12, 1),
        vec![(1800, 0)],
        "a repeat past week 9 stays 30 min"
    );
    for w in 1..=9 {
        assert_eq!(c25k_session_rx(w, 1).value.warmup_walk_min, 5);
    }
    // Weeks 1-2 keep the uniform run/walk volume; continuous weeks are minutes.
    assert_eq!(
        c25k_session_rx(1, 1).value.volume(),
        RunVolume::RunWalk {
            run_sec: 60,
            walk_sec: 90,
            rounds: 8
        }
    );
    assert_eq!(
        c25k_session_rx(9, 1).value.volume(),
        RunVolume::DurationMin(30)
    );
    assert_eq!(c25k_session_rx(1, 1).claim_id(), claims::RUN_C25K_002);
}

#[test]
fn race_phases_run_base_build_peak_taper_in_order() {
    use MesoPhase::*;
    let ph = |g, w| race_phase(g, w).unwrap().value;
    assert_eq!(taper_weeks(GoalDistance::TenK), 2);
    assert_eq!(taper_weeks(GoalDistance::Marathon), 3);
    assert_eq!(taper_weeks(GoalDistance::General), 0);
    assert_eq!(ph(GoalDistance::TenK, 20), Base);
    assert_eq!(ph(GoalDistance::TenK, 10), Build);
    assert_eq!(ph(GoalDistance::TenK, 4), Peak);
    assert_eq!(ph(GoalDistance::TenK, 1), Taper);
    assert_eq!(ph(GoalDistance::TenK, 0), Taper);
    assert!(race_phase(GoalDistance::General, 3).is_none());
    // Bosquet: every taper week cuts 41-60 %, deepening toward the race.
    let c1 = taper_volume_cut(GoalDistance::TenK, 1, 2).unwrap().value;
    let c2 = taper_volume_cut(GoalDistance::TenK, 2, 2).unwrap().value;
    assert!((0.41..=0.60).contains(&c1) && (0.41..=0.60).contains(&c2));
    assert!(c2 > c1, "progressive taper: {c1} then {c2}");
}

#[test]
fn quality_mix_follows_the_goal_table() {
    use MesoPhase::*;
    use RunSessionType::*;
    let mix = |g, p| quality_mix(g, p).value;
    assert!(mix(GoalDistance::Marathon, Build).contains(&RacePace));
    assert!(mix(GoalDistance::FiveK, Peak).contains(&Repetition));
    assert_eq!(mix(GoalDistance::HalfMarathon, Build)[0], Tempo);
    assert_eq!(mix(GoalDistance::TenK, Base), &[Tempo]);
    assert!(mix(GoalDistance::TenK, Deload).is_empty());
    assert!(mix(GoalDistance::C25k, Build).is_empty());
}

#[test]
fn long_run_share_is_30pct_under_64km() {
    assert_eq!(long_run_share(40.0).value, 0.30);
    assert_eq!(long_run_share(64.0).value, 0.25);
    assert_eq!(
        goal_long_run_target_km(GoalDistance::Marathon)
            .unwrap()
            .value,
        (29.0, 32.0)
    );
}

#[test]
fn vdot_50_threshold_pace_matches_daniels_table() {
    // Daniels' table: VDOT 50 → T pace ≈ 4:15/km (255 s). The T band is
    // 83–88 %VO2max; its fast end (88 %) is the table's T pace.
    let (fast, slow) =
        vdot_band_pace_sec_per_km(50.0, crate::schema::VdotBand::Threshold).expect("T band pace");
    assert!((fast - 255.0).abs() <= 3.0, "T fast end {fast}");
    assert!(slow > fast && slow < 275.0, "T slow end {slow}");
    // Easy sits well slower than threshold, 15–25 % slower than marathon
    // pace (running-015).
    let (e_fast, e_slow) = vdot_band_pace_sec_per_km(50.0, crate::schema::VdotBand::Easy).unwrap();
    assert!(e_fast > slow && e_slow > e_fast, "E {e_fast}..{e_slow}");
    let mp = daniels_predict(50.0, 42_195.0) / 42.195;
    for e in [e_fast, e_slow] {
        let pct = (e / mp - 1.0) * 100.0;
        assert!(
            (15.0 - 1e-6..=25.0 + 1e-6).contains(&pct),
            "E {e} is {pct}% slower than MP {mp}"
        );
    }
    assert!((e_fast / mp - 1.15).abs() < 1e-9 && (e_slow / mp - 1.25).abs() < 1e-9);
    // The inversion round-trips through daniels_vo2.
    let v = daniels_speed_for_vo2(40.0).unwrap();
    assert!((daniels_vo2(v) - 40.0).abs() < 1e-9);
    // Repetition has no upper %VO2max: both ends are the 100 % pace.
    let (r_fast, r_slow) =
        vdot_band_pace_sec_per_km(50.0, crate::schema::VdotBand::Repetition).unwrap();
    assert_eq!(r_fast, r_slow);
    assert_eq!(pace_sec_per_km_at(0.0, 88.0), None);
}

#[test]
fn daniels_marathon_bracket_covers_slow_vdots() {
    // A VDOT-20 marathon finishes past 6 h; the search returns the true
    // (slower) prediction, not a clamped optimistic bound.
    let t = daniels_predict(20.0, 42_195.0);
    assert!(
        t > 6.0 * 3600.0 && t < 12.0 * 3600.0,
        "expected 6–12 h, got {t}"
    );
    assert!(
        (vdot(42_195.0, t) - 20.0).abs() < 0.5,
        "round-trips to VDOT 20, got {}",
        vdot(42_195.0, t)
    );
    // The running-040 marathon derate holds for its target population: at
    // VDOT 22 the band is non-degenerate.
    let band = marathon_derated_band(4.0 * 3600.0, 4.5 * 3600.0, 22.0);
    assert!(
        band.value.0 < band.value.1,
        "derated band f_min < f_max: {:?}",
        band.value
    );
}
