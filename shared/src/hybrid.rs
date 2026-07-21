//! Concurrent / hybrid training interference logic (knowledge-base File 10 -
//! Resistance + Running, Evidence-Graded Knowledge Base).
//!
//! Pure, deterministic scheduling rules: intra-session ordering, inter-session
//! spacing, the Section-D override caps (CAP-1/3/5), and the reciprocal
//! running-economy lifting dose. No IO, no clocks, no randomness.
//!
//! Interference is quality-specific and modest (Schumann 2022): explosive power
//! is attenuated most, maximal strength lightly in trained lifters only, whole-
//! muscle hypertrophy essentially spared. Every prescriptive value is wrapped in
//! [`Recommended`] via [`recommend`] with evidence from `crate::evidence`.
//! Claim ids: CONC-ORDER-001, CONC-SEP-001, CONC-INTERF-001, CONC-RE-001,
//! HYB-CAP-001, SAFE-BSI-001, RUN-PROGRESS-001, SAFE-OTS-001.

use crate::evidence;
use crate::schema::Recommended;

/// Build a `Recommended<T>` from a registry claim id (must exist).
fn recommend<T>(value: T, claim_id: &str) -> Recommended<T> {
    let e = evidence::claim(claim_id).expect("known hybrid claim");
    Recommended::new(value, e.to_evidence(), e.to_confidence_tag())
}

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
    recommend(order, "CONC-ORDER-001")
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
    recommend(
        SessionSpacing {
            ideal_min_hours: 6,
            ideal_max_hours: 24,
            fallback_min_hours: 3,
        },
        "CONC-SEP-001",
    )
}

/// True when a heavy leg day and a hard/long run are ≥24 h apart, both
/// directions (File 10 CAP-3 / hybrid-012; residual fatigue 24–48 h).
pub fn heavy_leg_run_gap_ok(hours_between: f64) -> Recommended<bool> {
    recommend(hours_between >= 24.0, "HYB-CAP-001")
}

// ---------------------------------------------------------------------------
// 3. Override caps (File 10 Section D; HYB-CAP-001)
// ---------------------------------------------------------------------------

/// Endurance frequency ceiling when strength/hypertrophy is co-primary:
/// ≤3 d/wk (File 10 CAP-5 / hybrid-013). Each day beyond 3 raises attenuation.
pub fn endurance_frequency_cap() -> Recommended<u8> {
    recommend(3, "HYB-CAP-001")
}

/// True when weekly endurance days stay within the co-primary cap (≤3 d/wk).
pub fn endurance_frequency_ok(days_per_week: u8) -> Recommended<bool> {
    recommend(
        days_per_week <= endurance_frequency_cap().value,
        "HYB-CAP-001",
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
        Some(recommend(
            LowerLiftCap {
                max_lower_sessions: 2,
                volume_reduction_frac: (0.20, 0.33),
            },
            "HYB-CAP-001",
        ))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 4. Reciprocal lifting dose + interference expectations
// ---------------------------------------------------------------------------

/// Weekly lifting-session dose to retain in a running-priority plan (File 10
/// hybrid-010; CONC-RE-001). Strength training improves running economy 2–8 %
/// and does not harm VO2max, keep 2–3 sessions/week. Returns (min, max).
pub fn maintenance_lift_sessions() -> Recommended<(u8, u8)> {
    recommend((2, 3), "CONC-RE-001")
}

/// Whether to expect lower-body strength interference for this athlete (File 10
/// hybrid-009; HYB-TRAINED-001, Moderate, Petré 2021, contested CQ-06). Only
/// trained lifters (>1 yr) show the small trained-lower-body 1RM decrement;
/// moderately/untrained show none.
pub fn expect_lower_strength_interference(training_age_years: f64) -> Recommended<bool> {
    recommend(training_age_years > 1.0, "HYB-TRAINED-001")
}

/// Expect strength/hypertrophy attenuation when endurance dosing is high
/// (File 10 hybrid-014; HYB-THRESH-001, Moderate, Baar 2014/Jones 2013,
/// contested CQ-06): frequency above 3–4 d/wk OR intensity
/// above 80 % VO2max. `true` = expect measurable interference; cap endurance or
/// lower lifting-gain expectations. Uses the >3 d/wk edge (the lower, more
/// protective bound of the "3–4" range).
pub fn interference_expected(
    endurance_days_per_week: u8,
    endurance_intensity_pct_vo2max: f64,
) -> Recommended<bool> {
    let hit = endurance_days_per_week > 3 || endurance_intensity_pct_vo2max > 80.0;
    recommend(hit, "HYB-THRESH-001")
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
    recommend(
        PeakPhaseRunCap {
            max_easy_runs_per_week: (2, 3),
            allow_intervals: false,
            long_run_min_gap_hours: 24,
        },
        "HYB-CAP-001",
    )
}

/// Maintenance dose as a fraction of the improvement dose (File 10 hybrid-017 /
/// CAP-7): a quality being *maintained* rather than *improved* needs ~1/3 of the
/// improvement volume (≈2 sessions/wk, low volume) to free recovery for the
/// priority. Returns the multiplier to apply to the improvement dose.
/// HYB-MAINT-001 (ExpertOpinion, CAP-7 has no named primary source).
pub fn maintenance_dose_fraction() -> Recommended<f64> {
    recommend(1.0 / 3.0, "HYB-MAINT-001")
}

/// Substitute a low-impact modality (cycling/rowing) for part of aerobic volume
/// when interference symptoms appear AND running is not mandatory (File 10
/// hybrid-018 / CAP-6; HYB-CAP-001). `true` = swap part of the run volume.
pub fn substitute_modality(
    interference_symptoms: bool,
    running_optional: bool,
) -> Recommended<bool> {
    recommend(interference_symptoms && running_optional, "HYB-CAP-001")
}

/// Raise bone-stress-injury surveillance when weekly running exceeds ~64 km
/// (File 10 hybrid-023; HYB-BSI-001, Moderate, Warden 2021, safety-critical
/// surveillance trigger, distinct from the Strong SAFE-BSI-001 stop-loading
/// deferral once a BSI is suspected). Resistance training is protective when
/// energy availability is adequate. `true` = heighten monitoring.
pub fn bsi_surveillance_flag(running_km_per_week: f64) -> Recommended<bool> {
    recommend(running_km_per_week > 64.0, "HYB-BSI-001")
}

/// Combined-load running progression guard (File 10 hybrid-021, safety layer):
/// keep weekly running-volume growth ≤ ~10 %/wk to bound stacked mechanical +
/// systemic load. Returns whether next week's volume is within the cap.
///
/// NOTE: the ACWR "sweet spot" (0.8–1.3) from this rule is deliberately NOT
/// encoded, it is the hard-blocked `LOAD-ACWR-001` myth (mathematically
/// coupled; see `load.rs`). The ≤10 %/wk ramp is the guardrail we act on.
/// HYB-PROG-001 (Moderate, safety-critical, contested CQ-05).
pub fn combined_load_progression_ok(current_km: f64, next_km: f64) -> Recommended<bool> {
    let ok = if current_km <= 0.0 {
        false
    } else {
        (next_km - current_km) / current_km <= 0.10 + 1e-9
    };
    recommend(ok, "HYB-PROG-001")
}

/// Combined systemic + mechanical overreaching thresholds (File 10 hybrid-026).
/// A "red flag" is any of: RHR ≥ 5–7 bpm over baseline, HRV 7-day trend down
/// > ~15 % for 3–5 days, or a sleep/mood/performance decline.
pub const HYBRID_RHR_FLAG_BPM: f64 = 5.0;
/// HRV 7-day downtrend fraction that counts as a red flag (File 10 hybrid-026).
pub const HYBRID_HRV_FLAG_DROP_FRAC: f64 = 0.15;

/// Trigger a deload when ≥2 overreaching red flags persist beyond ~1 week
/// (File 10 hybrid-026, safety-critical). `red_flag_count` aggregates the
/// RHR/HRV/subjective flags; `weeks_persisted` is how long they have held.
/// `true` = insert a deload / recovery block. HYB-DELOAD-001 (Moderate,
/// contested CQ-04, Bellenger 2016: resting HRV may not reliably detect
/// overreaching), not the Strong OTS deferral.
pub fn combined_fatigue_deload(red_flag_count: u8, weeks_persisted: u8) -> Recommended<bool> {
    recommend(red_flag_count >= 2 && weeks_persisted >= 1, "HYB-DELOAD-001")
}

// ---------------------------------------------------------------------------
// 5. Interference moderators, scheduling & phase policy (File 10 hybrid-004/
//    011/019/020; Task 19)
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
    recommend(
        InterferenceModerators {
            frequency_r: (-0.35, -0.26),
            duration_r: (-0.75, -0.29),
            duration_is_strongest: true,
        },
        "HYB-DURATION-001",
    )
}

/// Schedule the highest-priority quality when freshest, at the start of the
/// week or immediately after a rest day (File 10 hybrid-011; HYB-SCHED-001,
/// ExpertOpinion). `true` = the given slot is a "freshest" slot for the
/// priority quality.
pub fn priority_quality_when_freshest(
    week_start: bool,
    after_rest_day: bool,
) -> Recommended<bool> {
    recommend(week_start || after_rest_day, "HYB-SCHED-001")
}

/// Double (AM/PM) day carbohydrate rule (File 10 hybrid-019 / CAP-8;
/// HYB-CHO-001, Weak, Baar 2014): fully refuel CHO between the endurance and
/// lifting sessions, because low glycogen amplifies AMPK activation and
/// interference. `true` = the refuel note applies to this day.
pub fn double_day_cho_refuel(am_pm_double_day: bool) -> Recommended<bool> {
    recommend(am_pm_double_day, "HYB-CHO-001")
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
    recommend(p, "HYB-PHASE-001")
}

// ---------------------------------------------------------------------------
// 6. Safety-layer guards (File 10 hybrid-024/025; Task 19)
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
    recommend(high_volume_endurance || lean || female, "HYB-EA-001")
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
    recommend(
        progressing_running_volume && progressing_heavy_lifting,
        "HYB-TENDON-001",
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
    fn endurance_frequency_cap_is_three() {
        assert_eq!(endurance_frequency_cap().value, 3);
        assert!(endurance_frequency_ok(3).value);
        assert!(!endurance_frequency_ok(4).value);
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
    fn reciprocal_dose_and_interference_expectations() {
        assert_eq!(maintenance_lift_sessions().value, (2, 3));
        // Trained lifter expects small interference; novice does not.
        assert!(expect_lower_strength_interference(2.0).value);
        assert!(!expect_lower_strength_interference(0.5).value);
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
        assert!(combined_load_progression_ok(50.0, 55.0).value); // +10%
        assert!(!combined_load_progression_ok(50.0, 60.0).value); // +20%
        assert!(!combined_load_progression_ok(0.0, 5.0).value); // no baseline
    }

    #[test]
    fn combined_fatigue_deload_needs_two_flags_persisting() {
        assert!(combined_fatigue_deload(2, 1).value);
        assert!(!combined_fatigue_deload(1, 2).value); // only one flag
        assert!(!combined_fatigue_deload(3, 0).value); // not persisted
        assert!(combined_fatigue_deload(2, 1).confidence.safety_critical);
    }

    #[test]
    fn bsi_surveillance_above_64km() {
        let flag = bsi_surveillance_flag(70.0);
        assert!(flag.value);
        assert!(flag.confidence.safety_critical);
        assert!(!bsi_surveillance_flag(50.0).value);
    }

    // --- Task 19: hybrid-004/011/019/020/024/025 ---

    #[test]
    fn duration_is_the_strongest_interference_moderator() {
        let m = interference_moderators();
        assert!(m.value.duration_is_strongest);
        // Verbatim Wilson 2012 bands: frequency r −0.26..−0.35, duration
        // r −0.29..−0.75 (hypertrophy end).
        assert_eq!(m.value.frequency_r, (-0.35, -0.26));
        assert_eq!(m.value.duration_r, (-0.75, -0.29));
        assert_eq!(
            m.evidence.citation.claim_id.as_deref(),
            Some("HYB-DURATION-001")
        );
        // Contested: onset threshold undefined (File 10 CQ-02 → global CQ-06).
        assert!(m.confidence.contested);
        assert_eq!(m.confidence.contested_question_ref.as_deref(), Some("CQ-06"));
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
        assert!((r.confidence.score - 0.40).abs() < f32::EPSILON);
        assert_eq!(r.evidence.citation.claim_id.as_deref(), Some("HYB-CHO-001"));
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
        assert!(g.confidence.safety_critical);
        assert_eq!(g.evidence.citation.claim_id.as_deref(), Some("HYB-EA-001"));
        // ExpertOpinion (File 10 Section E, uncited): never overstated.
        assert!((g.confidence.score - 0.30).abs() < f32::EPSILON);
    }

    #[test]
    fn dual_progression_guard_fires_only_on_simultaneous_progression() {
        assert!(conservative_dual_progression(true, true).value);
        assert!(!conservative_dual_progression(true, false).value);
        assert!(!conservative_dual_progression(false, true).value);
        let g = conservative_dual_progression(true, true);
        assert!(g.confidence.safety_critical);
        // Weak: an evidence *gap* drives the conservatism (Baar 2014).
        assert!((g.confidence.score - 0.40).abs() < f32::EPSILON);
        assert_eq!(
            g.evidence.citation.claim_id.as_deref(),
            Some("HYB-TENDON-001")
        );
    }
}
