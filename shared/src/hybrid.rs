//! Concurrent / hybrid training interference logic (knowledge-base File 10 -
//! Resistance + Running, Evidence-Graded Knowledge Base).
//!
//! Pure, deterministic scheduling rules: intra-session ordering, inter-session
//! spacing, the Section-D override caps (CAP-1/3/5), and the reciprocal
//! running-economy lifting dose. No IO, no clocks, no randomness.
//!
//! Interference is quality-specific and modest (Schumann 2022): explosive power
//! is attenuated most, maximal lower-body strength lightly (in men, HYB-SEX-001),
//! whole-muscle hypertrophy essentially spared. Every prescriptive value is
//! wrapped in [`Recommended`] via [`Recommended::new`] with evidence from
//! `crate::evidence`. Claim ids: CONC-ORDER-001, CONC-SEP-001, HYB-CAP-001,
//! HYB-RUNCAP-001, CONC-RE-001, HYB-SEX-001, HYB-THRESH-001, HYB-MAINT-001,
//! HYB-BSI-001, HYB-PROG-001, HYB-DELOAD-001, HYB-DURATION-001, HYB-SCHED-001,
//! HYB-CHO-001, HYB-PHASE-001, HYB-EA-001, HYB-TENDON-001.

use crate::evidence;
use crate::evidence::claims;
use crate::schema::Recommended;

// ---------------------------------------------------------------------------
// 1. Intra-session ordering (File 10 hybrid-005/006/008; CONC-ORDER-001)
// ---------------------------------------------------------------------------

/// The co-primary goal governing a concurrent day's ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConcurrentGoal {
    Strength,
    Power,
    Hypertrophy,
    EndurancePriority,
}

/// How to sequence lift + run when they share a session (or whether to).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionOrder {
    /// Resistance before endurance (strength/hypertrophy goals).
    LiftFirst,
    /// Endurance first is acceptable (endurance-priority; order-insensitive).
    RunFirst,
    /// Do not train both in one session: power attenuates same-session.
    ForbidSameSession,
}

/// Same-session ordering for a goal (File 10 hybrid-005/006/008; CONC-ORDER-001).
/// Power forbids same-session entirely (CAP-4); strength/hypertrophy lift first;
/// endurance-priority may run first.
pub fn same_session_order(goal: ConcurrentGoal) -> Recommended<SessionOrder> {
    let order = match goal {
        ConcurrentGoal::Power => SessionOrder::ForbidSameSession,
        ConcurrentGoal::Strength | ConcurrentGoal::Hypertrophy => SessionOrder::LiftFirst,
        ConcurrentGoal::EndurancePriority => SessionOrder::RunFirst,
    };
    evidence::graded(order, claims::CONC_ORDER_001)
}

// ---------------------------------------------------------------------------
// 2. Inter-session spacing (File 10 hybrid-007/012; CONC-SEP-001 / HYB-CAP-001)
// ---------------------------------------------------------------------------

/// Acute same-day separation targets between opposing qualities (hours).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSpacing {
    /// Ideal minimum gap (AMPK normalizes ~3 h; 6 h matches 24 h for strength).
    pub ideal_min_hours: u8,
    /// Ideal upper window bound.
    pub ideal_max_hours: u8,
    /// Hard fallback minimum when 6 h is impossible.
    pub fallback_min_hours: u8,
}

/// Default acute spacing: 6–24 h ideal, ≥3 h fallback (File 10 hybrid-007;
/// CONC-SEP-001).
pub fn session_spacing() -> Recommended<SessionSpacing> {
    evidence::graded(
        SessionSpacing {
            ideal_min_hours: 6,
            ideal_max_hours: 24,
            fallback_min_hours: 3,
        },
        claims::CONC_SEP_001,
    )
}

/// Minimum hours between a heavy leg day and a hard/long run, both directions
/// (File 10 CAP-3 / hybrid-012: residual fatigue lasts 24–48 h; HYB-CAP-001).
pub const HEAVY_LEG_RUN_GAP_H: f64 = 24.0;

/// True when a heavy leg day and a hard/long run are at least
/// [`HEAVY_LEG_RUN_GAP_H`] apart (File 10 CAP-3 / hybrid-012).
pub fn heavy_leg_run_gap_ok(hours_between: f64) -> Recommended<bool> {
    evidence::graded(hours_between >= HEAVY_LEG_RUN_GAP_H, claims::HYB_CAP_001)
}

// ---------------------------------------------------------------------------
// 3. Override caps (File 10 Section D; HYB-CAP-001)
// ---------------------------------------------------------------------------

/// Default endurance-day ceiling when strength/hypertrophy/power is co-primary:
/// 3 d/wk (File 10 CAP-5 / hybrid-013, re-graded by 13-hybrid-update hyb-upd-003).
/// A cautious default, NOT an evidence threshold: concurrent-session frequency
/// did not moderate strength or muscle outcomes in recent meta-analyses
/// (HYB-RUNCAP-001, ExpertOpinion). The plan keeps per-run distance instead of
/// lengthening the remaining runs when this trims days (hybrid-004).
pub fn endurance_frequency_cap() -> Recommended<u8> {
    evidence::graded(3, claims::HYB_RUNCAP_001)
}

/// True when weekly endurance days stay within the co-primary default (≤3 d/wk).
pub fn endurance_frequency_ok(days_per_week: u8) -> Recommended<bool> {
    evidence::graded(
        days_per_week <= endurance_frequency_cap().value,
        claims::HYB_RUNCAP_001,
    )
}

/// Lower-body lifting override when running is high (File 10 CAP-1 / hybrid-015).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LowerLiftCap {
    /// Max lower-body lifting sessions per week under the cap.
    pub max_lower_sessions: u8,
    /// Lower-body hypertrophy volume reduction range (low, high) as fractions.
    pub volume_reduction_frac: (f64, f64),
}

/// Apply the running-volume lower-lift cap when running ≥4 d/wk OR ≥40 km/wk
/// (File 10 CAP-1 / hybrid-015): cap lower lifting ≤2/wk and cut lower
/// hypertrophy volume ~20–33 %. `None` when neither trigger fires.
pub fn lower_lift_cap(
    running_days_per_week: u8,
    running_km_per_week: f64,
) -> Option<Recommended<LowerLiftCap>> {
    if running_days_per_week >= 4 || running_km_per_week >= 40.0 {
        Some(evidence::graded(
            LowerLiftCap {
                max_lower_sessions: 2,
                volume_reduction_frac: (0.20, 0.33),
            },
            claims::HYB_CAP_001,
        ))
    } else {
        None
    }
}

/// Weekly lower-body set count after the CAP-1 cut: the midpoint of the KB's
/// 20–33 % reduction (≈27 %), rounded, never below one set per lower-body
/// exposure the week still carries (HYB-CAP-001; HYB-MAINT-002: one set per
/// exercise at the same relative load holds strength).
pub fn capped_lower_weekly_sets(weekly_sets: u32, cap: &LowerLiftCap, min_sets: u32) -> u32 {
    let mid = (cap.volume_reduction_frac.0 + cap.volume_reduction_frac.1) / 2.0;
    ((weekly_sets as f64 * (1.0 - mid)).round() as u32).max(min_sets)
}

// ---------------------------------------------------------------------------
// 4. Reciprocal lifting dose + interference expectations
// ---------------------------------------------------------------------------

/// Weekly lifting-session dose to retain in a running-priority plan (File 10
/// hybrid-010; CONC-RE-001). Strength training improves running economy 2–8 %
/// and does not harm VO2max, keep 2–3 sessions/week. Returns (min, max).
pub fn maintenance_lift_sessions() -> Recommended<(u8, u8)> {
    evidence::graded((2, 3), claims::CONC_RE_001)
}

/// How likely concurrent running is to blunt lower-body maximal-strength gains
/// for this athlete (13-hybrid-update hyb-upd-001 / -002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowerStrengthInterference {
    /// Small lower-body strength blunting is possible (seen in men; sex unstated
    /// is treated the same, the cautious reading).
    PossibleSmall,
    /// No blunting has been shown in women (few studies).
    NotShownInWomen,
}

/// Expected lower-body strength interference by SEX, the moderator with
/// meta-analytic support (HYB-SEX-001, Moderate: men SMD −0.43, women 0.08).
/// Training status is deliberately NOT an input: it is not a reliable moderator
/// (HYB-TRAINED-001, Weak), so no one is told they are "spared" for being
/// untrained. Upper body, muscle size and aerobic gains are unaffected either way.
pub fn expect_lower_strength_interference(
    female: Option<bool>,
) -> Recommended<LowerStrengthInterference> {
    let v = if female == Some(true) {
        LowerStrengthInterference::NotShownInWomen
    } else {
        LowerStrengthInterference::PossibleSmall
    };
    evidence::graded(v, claims::HYB_SEX_001)
}

/// Expect strength/hypertrophy attenuation when endurance dosing is high
/// (File 10 hybrid-014; HYB-THRESH-001, Moderate, Baar 2014/Jones 2013,
/// contested CQ-06): frequency above 3–4 d/wk OR intensity
/// above 80 % VO2max. `true` = expect measurable interference; cap endurance or
/// lower lifting-gain expectations. Uses the >3 d/wk edge (the lower, more
/// protective bound of the "3–4" range). Both inputs come from the athlete's
/// OWN plan, never a self-report: the run days the week actually schedules and
/// the hardest scheduled run's %VO2max (Daniels band floor, e.g. threshold
/// 83 %, intervals 95 %).
pub fn interference_expected(
    endurance_days_per_week: u8,
    peak_session_pct_vo2max: f64,
) -> Recommended<bool> {
    let hit = endurance_days_per_week > 3 || peak_session_pct_vo2max > 80.0;
    evidence::graded(hit, claims::HYB_THRESH_001)
}

/// Peak strength/power mesocycle running override (File 10 hybrid-016 / CAP-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeakPhaseRunCap {
    /// Max easy running sessions per week during the peak block (range).
    pub max_easy_runs_per_week: (u8, u8),
    /// Hard intervals are removed for the block.
    pub allow_intervals: bool,
    /// Minimum hours a long run must clear a heavy-lower day.
    pub long_run_min_gap_hours: u8,
}

/// During a peak strength/power mesocycle, cap running to ≤2–3 easy sessions/wk,
/// remove hard intervals, and keep long runs ≥24 h from heavy-lower days
/// (File 10 hybrid-016 / CAP-2; HYB-CAP-001).
pub fn peak_phase_run_cap() -> Recommended<PeakPhaseRunCap> {
    evidence::graded(
        PeakPhaseRunCap {
            max_easy_runs_per_week: (2, 3),
            allow_intervals: false,
            long_run_min_gap_hours: 24,
        },
        claims::HYB_CAP_001,
    )
}

/// Maintenance dose as a fraction of the improvement dose (File 10 hybrid-017 /
/// CAP-7): a quality being *maintained* rather than *improved* needs ~1/3 of the
/// improvement volume (≈2 sessions/wk, low volume) to free recovery for the
/// priority. Returns the multiplier to apply to the improvement dose.
/// HYB-MAINT-001 (ExpertOpinion, CAP-7 has no named primary source).
pub fn maintenance_dose_fraction() -> Recommended<f64> {
    evidence::graded(1.0 / 3.0, claims::HYB_MAINT_001)
}

/// Per-exercise working sets when lifting is the MAINTAINED side of a hybrid
/// week (running leads, or a runner's supplemental strength): about a third of
/// the improvement dose (HYB-MAINT-001), kept inside the KB's running-
/// maintenance band of 2–4 heavy sets per exercise at ≥80 % 1RM (File 10
/// hybrid-017 / CAP-7). Intensity is held, only volume drops (HYB-MAINT-002).
pub fn maintenance_sets_per_exercise(improvement_sets: u8) -> Recommended<u8> {
    let third = (improvement_sets as f64 * maintenance_dose_fraction().value).ceil() as u8;
    evidence::graded(third.clamp(2, 4), claims::HYB_MAINT_001)
}

/// Substitute a low-impact modality (cycling/rowing) for part of aerobic volume
/// when interference symptoms appear AND running is not mandatory (File 10
/// hybrid-018 / CAP-6; HYB-CAP-001). `true` = swap part of the run volume.
pub fn substitute_modality(
    interference_symptoms: bool,
    running_optional: bool,
) -> Recommended<bool> {
    evidence::graded(
        interference_symptoms && running_optional,
        claims::HYB_CAP_001,
    )
}

/// Weekly running volume (km) above which bone-stress-injury surveillance is
/// raised (~40 mi/wk; File 10 hybrid-023; HYB-BSI-001).
pub const BSI_SURVEILLANCE_KM_PER_WEEK: f64 = 64.0;

/// Raise bone-stress-injury surveillance when weekly running exceeds
/// [`BSI_SURVEILLANCE_KM_PER_WEEK`] (File 10 hybrid-023; HYB-BSI-001, Moderate,
/// Warden 2021, safety-critical surveillance trigger, distinct from the Strong
/// SAFE-BSI-001 stop-loading deferral once a BSI is suspected). Resistance
/// training is protective when energy availability is adequate. `true` =
/// heighten monitoring.
pub fn bsi_surveillance_flag(running_km_per_week: f64) -> Recommended<bool> {
    evidence::graded(
        running_km_per_week > BSI_SURVEILLANCE_KM_PER_WEEK,
        claims::HYB_BSI_001,
    )
}

/// Week-over-week running-volume growth cap under a combined lifting + running
/// load (File 10 hybrid-021: "≤ ~10 %/wk"; HYB-PROG-001).
pub const COMBINED_RAMP_CAP_FRAC: f64 = 0.10;

/// Combined-load running progression guard (File 10 hybrid-021, safety layer):
/// keep weekly running-volume growth ≤ [`COMBINED_RAMP_CAP_FRAC`] to bound
/// stacked mechanical + systemic load. Returns whether next week's volume is
/// within the cap.
///
/// NOTE: the ACWR "sweet spot" (0.8–1.3) from this rule is deliberately NOT
/// encoded, it is the hard-blocked `LOAD-ACWR-001` myth (mathematically
/// coupled; see `load.rs`). The ≤10 %/wk ramp is the guardrail we act on.
/// HYB-PROG-001 (Moderate, safety-critical, contested CQ-05).
pub fn combined_load_progression_ok(current_km: f64, next_km: f64) -> Recommended<Option<bool>> {
    // No baseline volume → the growth ratio is undefined, so the guard is
    // NOT-APPLICABLE (None), not a failure: a first week of running must not read
    // as a spike.
    let ok = crate::running::ratio_increase_within(current_km, next_km, COMBINED_RAMP_CAP_FRAC);
    evidence::graded(ok, claims::HYB_PROG_001)
}

// ---------------------------------------------------------------------------
// 5. Interference moderators, scheduling & phase policy (File 10 hybrid-004/
//    011/019/020)
// ---------------------------------------------------------------------------

/// Wilson 2012 interference moderator correlations (File 10 hybrid-004;
/// HYB-DURATION-001, Moderate, contested CQ-06, the exact frequency/duration
/// onset threshold is undefined, so these are *relative weights*, never
/// cutoffs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InterferenceModerators {
    /// Endurance-frequency correlation band with lifting outcomes (r).
    pub frequency_r: (f64, f64),
    /// Per-session continuous-duration correlation band (r); the −0.75 end is
    /// the hypertrophy outcome.
    pub duration_r: (f64, f64),
    /// Continuous session duration is the single strongest moderator.
    pub duration_is_strongest: bool,
}

/// Interference scales with endurance frequency and (most strongly) continuous
/// per-session duration (File 10 hybrid-004). Prefer shortening endurance
/// sessions over cutting frequency when protecting lifting adaptations.
pub fn interference_moderators() -> Recommended<InterferenceModerators> {
    evidence::graded(
        InterferenceModerators {
            frequency_r: (-0.35, -0.26),
            duration_r: (-0.75, -0.29),
            duration_is_strongest: true,
        },
        claims::HYB_DURATION_001,
    )
}

/// Schedule the highest-priority quality when freshest, at the start of the
/// week or immediately after a rest day (File 10 hybrid-011; HYB-SCHED-001,
/// ExpertOpinion). `true` = the given slot is a "freshest" slot for the
/// priority quality.
pub fn priority_quality_when_freshest(week_start: bool, after_rest_day: bool) -> Recommended<bool> {
    evidence::graded(week_start || after_rest_day, claims::HYB_SCHED_001)
}

/// Double (AM/PM) day carbohydrate rule (File 10 hybrid-019 / CAP-8;
/// HYB-CHO-001, Weak, Baar 2014): fully refuel CHO between the endurance and
/// lifting sessions, because low glycogen amplifies AMPK activation and
/// interference. `true` = the refuel note applies to this day.
pub fn double_day_cho_refuel(am_pm_double_day: bool) -> Recommended<bool> {
    evidence::graded(am_pm_double_day, claims::HYB_CHO_001)
}

/// Training phase for interference policy (File 10 hybrid-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridPhase {
    /// General preparation: separate qualities to minimize interference.
    General,
    /// Specific/event phase: deliberately combine qualities, accepting some
    /// interference for sport-specific transfer.
    SpecificEvent,
}

/// Phase interference policy (File 10 hybrid-020; HYB-PHASE-001, ExpertOpinion).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseInterferencePolicy {
    /// Combine strength + endurance qualities within sessions/days.
    pub combine_qualities: bool,
    /// Accept some interference as the price of sport-specific transfer.
    pub accept_interference: bool,
    /// Hybrid-race weekly split, `(strength sessions, endurance sessions)`
    /// low–high (2–3 strength + 3–4 endurance), stated for the specific phase
    /// template; `None` in the general phase (no split stated).
    pub weekly_split: Option<((u8, u8), (u8, u8))>,
}

/// Periodize interference by phase (File 10 hybrid-020): a general phase
/// separates qualities; a specific/event phase deliberately combines them
/// (strength-endurance hybrids at moderate load / high rep / minimal rest).
pub fn phase_interference_policy(phase: HybridPhase) -> Recommended<PhaseInterferencePolicy> {
    let p = match phase {
        HybridPhase::General => PhaseInterferencePolicy {
            combine_qualities: false,
            accept_interference: false,
            weekly_split: None,
        },
        HybridPhase::SpecificEvent => PhaseInterferencePolicy {
            combine_qualities: true,
            accept_interference: true,
            weekly_split: Some(((2, 3), (3, 4))),
        },
    };
    evidence::graded(p, claims::HYB_PHASE_001)
}

// ---------------------------------------------------------------------------
// 6. Safety-layer guards (File 10 hybrid-024/025)
// ---------------------------------------------------------------------------

/// Energy-availability guard (File 10 hybrid-024; HYB-EA-001, ExpertOpinion,
/// safety-critical): guard against RED-S/LEA, with heightened vigilance for
/// the higher-risk cohorts, high-volume endurance, leaner, and female
/// athletes. `true` = heightened LEA vigilance for this athlete; the guard
/// itself (adequate fueling) applies to everyone. Actual RED-S signals route
/// to the absolute deferral (SAFE-REDS-001), not this monitor.
pub fn energy_availability_guard(
    high_volume_endurance: bool,
    lean: bool,
    female: bool,
) -> Recommended<bool> {
    evidence::graded(high_volume_endurance || lean || female, claims::HYB_EA_001)
}

/// Conservative dual-progression guard (File 10 hybrid-025; HYB-TENDON-001,
/// Weak, safety-critical): the concurrent-training effect on tendon stiffness
/// has NO direct study, when high running volume and heavy lifting would
/// progress in the same week, err conservative (progress one, hold the other).
/// `true` = the combination is aggressive; hold one modality's progression.
pub fn conservative_dual_progression(
    progressing_running_volume: bool,
    progressing_heavy_lifting: bool,
) -> Recommended<bool> {
    evidence::graded(
        progressing_running_volume && progressing_heavy_lifting,
        claims::HYB_TENDON_001,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_forbids_same_session_for_power() {
        assert_eq!(
            same_session_order(ConcurrentGoal::Power).value,
            SessionOrder::ForbidSameSession
        );
        assert_eq!(
            same_session_order(ConcurrentGoal::Strength).value,
            SessionOrder::LiftFirst
        );
        assert_eq!(
            same_session_order(ConcurrentGoal::Hypertrophy).value,
            SessionOrder::LiftFirst
        );
        assert_eq!(
            same_session_order(ConcurrentGoal::EndurancePriority).value,
            SessionOrder::RunFirst
        );
    }

    #[test]
    fn spacing_defaults_are_6_24_fallback_3() {
        let s = session_spacing().value;
        assert_eq!(
            (s.ideal_min_hours, s.ideal_max_hours, s.fallback_min_hours),
            (6, 24, 3)
        );
        // CAP-3: 24h gap between heavy legs and hard runs.
        assert!(heavy_leg_run_gap_ok(24.0).value);
        assert!(!heavy_leg_run_gap_ok(20.0).value);
    }

    #[test]
    fn endurance_frequency_cap_is_three_and_cites_the_cautious_default() {
        assert_eq!(endurance_frequency_cap().value, 3);
        assert!(endurance_frequency_ok(3).value);
        assert!(!endurance_frequency_ok(4).value);
        assert_eq!(endurance_frequency_cap().claim_id(), claims::HYB_RUNCAP_001);
    }

    #[test]
    fn lower_lift_cap_triggers_on_high_running() {
        // 4 d/wk triggers.
        let by_days = lower_lift_cap(4, 20.0).expect("4 d/wk triggers");
        assert_eq!(by_days.value.max_lower_sessions, 2);
        assert_eq!(by_days.value.volume_reduction_frac, (0.20, 0.33));
        // 40 km/wk triggers even at 3 days.
        assert!(lower_lift_cap(3, 40.0).is_some());
        // Below both triggers: no cap.
        assert!(lower_lift_cap(3, 30.0).is_none());
    }

    #[test]
    fn lower_cap_trims_about_a_quarter_of_lower_sets() {
        let cap = lower_lift_cap(4, 20.0).unwrap().value;
        // 12 weekly lower sets × (1 − 0.265) = 8.8 → 9.
        assert_eq!(capped_lower_weekly_sets(12, &cap, 1), 9);
        assert_eq!(capped_lower_weekly_sets(1, &cap, 1), 1);
    }

    #[test]
    fn maintenance_sets_are_a_third_inside_two_to_four() {
        assert_eq!(maintenance_sets_per_exercise(4).value, 2);
        assert_eq!(maintenance_sets_per_exercise(10).value, 4);
        assert_eq!(maintenance_sets_per_exercise(1).value, 2);
    }

    #[test]
    fn reciprocal_dose_and_interference_expectations() {
        assert_eq!(maintenance_lift_sessions().value, (2, 3));
    }

    #[test]
    fn lower_strength_interference_uses_sex_moderator() {
        assert_eq!(
            expect_lower_strength_interference(Some(false)).value,
            LowerStrengthInterference::PossibleSmall
        );
        assert_eq!(
            expect_lower_strength_interference(None).value,
            LowerStrengthInterference::PossibleSmall
        );
        assert_eq!(
            expect_lower_strength_interference(Some(true)).value,
            LowerStrengthInterference::NotShownInWomen
        );
        assert_eq!(
            expect_lower_strength_interference(None).claim_id(),
            claims::HYB_SEX_001
        );
    }

    #[test]
    fn interference_expected_on_high_freq_or_intensity() {
        // Frequency edge: >3 d/wk trips it.
        assert!(interference_expected(4, 60.0).value);
        assert!(!interference_expected(3, 60.0).value);
        // Intensity edge: >80 %VO2max trips it even at low frequency.
        assert!(interference_expected(2, 85.0).value);
        assert!(!interference_expected(2, 80.0).value);
    }

    #[test]
    fn peak_phase_cap_removes_intervals() {
        let c = peak_phase_run_cap().value;
        assert_eq!(c.max_easy_runs_per_week, (2, 3));
        assert!(!c.allow_intervals);
        assert_eq!(c.long_run_min_gap_hours, 24);
    }

    #[test]
    fn maintenance_dose_is_one_third() {
        assert!((maintenance_dose_fraction().value - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn modality_substitution_needs_both_conditions() {
        assert!(substitute_modality(true, true).value);
        assert!(!substitute_modality(true, false).value); // running mandatory
        assert!(!substitute_modality(false, true).value); // no symptoms
    }
    #[test]
    fn combined_load_progression_caps_at_ten_percent() {
        assert_eq!(combined_load_progression_ok(50.0, 55.0).value, Some(true)); // +10%
        assert_eq!(combined_load_progression_ok(50.0, 60.0).value, Some(false)); // +20%
        assert_eq!(combined_load_progression_ok(0.0, 5.0).value, None); // no baseline → N/A
    }

    #[test]
    fn bsi_surveillance_above_64km() {
        let flag = bsi_surveillance_flag(70.0);
        assert!(flag.value);
        assert!(flag.safety_critical());
        assert!(!bsi_surveillance_flag(50.0).value);
    }

    // --- hybrid-004/011/019/020/024/025 ---

    #[test]
    fn duration_is_the_strongest_interference_moderator() {
        let m = interference_moderators();
        assert!(m.value.duration_is_strongest);
        // Verbatim Wilson 2012 bands: frequency r −0.26..−0.35, duration
        // r −0.29..−0.75 (hypertrophy end).
        assert_eq!(m.value.frequency_r, (-0.35, -0.26));
        assert_eq!(m.value.duration_r, (-0.75, -0.29));
        assert_eq!(m.claim_id(), claims::HYB_DURATION_001);
        // Contested: onset threshold undefined (File 10 CQ-02 → global CQ-06).
        assert!(m.contested().is_some());
        assert_eq!(m.contested(), Some("CQ-06"));
    }

    #[test]
    fn priority_quality_scheduled_when_freshest() {
        assert!(priority_quality_when_freshest(true, false).value);
        assert!(priority_quality_when_freshest(false, true).value);
        assert!(!priority_quality_when_freshest(false, false).value);
    }

    #[test]
    fn double_day_gets_cho_refuel_note_at_weak_grade() {
        let r = double_day_cho_refuel(true);
        assert!(r.value);
        assert!(!double_day_cho_refuel(false).value);
        // Registered at the rule entry's Weak floor (CAP-8 table says
        // Weak-Moderate; never rounded up).
        assert!((r.confidence() - 0.40).abs() < f32::EPSILON);
        assert_eq!(r.claim_id(), claims::HYB_CHO_001);
    }

    #[test]
    fn phase_policy_separates_general_combines_specific() {
        let g = phase_interference_policy(HybridPhase::General).value;
        assert!(!g.combine_qualities && !g.accept_interference);
        assert_eq!(g.weekly_split, None);
        let s = phase_interference_policy(HybridPhase::SpecificEvent).value;
        assert!(s.combine_qualities && s.accept_interference);
        // Hybrid-race split: 2-3 strength + 3-4 endurance sessions/wk.
        assert_eq!(s.weekly_split, Some(((2, 3), (3, 4))));
    }

    #[test]
    fn energy_availability_guard_flags_risk_cohorts() {
        assert!(energy_availability_guard(true, false, false).value);
        assert!(energy_availability_guard(false, true, false).value);
        assert!(energy_availability_guard(false, false, true).value);
        assert!(!energy_availability_guard(false, false, false).value);
        let g = energy_availability_guard(false, false, true);
        assert!(g.safety_critical());
        assert_eq!(g.claim_id(), claims::HYB_EA_001);
        // ExpertOpinion (File 10 Section E, uncited): never overstated.
        assert!((g.confidence() - 0.30).abs() < f32::EPSILON);
    }

    #[test]
    fn dual_progression_guard_fires_only_on_simultaneous_progression() {
        assert!(conservative_dual_progression(true, true).value);
        assert!(!conservative_dual_progression(true, false).value);
        assert!(!conservative_dual_progression(false, true).value);
        let g = conservative_dual_progression(true, true);
        assert!(g.safety_critical());
        // Weak: an evidence *gap* drives the conservatism (Baar 2014).
        assert!((g.confidence() - 0.40).abs() < f32::EPSILON);
        assert_eq!(g.claim_id(), claims::HYB_TENDON_001);
    }
}
