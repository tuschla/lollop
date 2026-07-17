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
    Recommended {
        value,
        evidence: e.to_evidence(),
        confidence: e.to_confidence_tag(),
    }
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
/// hybrid-009; CONC-INTERF-001). Only trained lifters (>1 yr) show the small
/// trained-lower-body 1RM decrement; moderately/untrained show none.
pub fn expect_lower_strength_interference(training_age_years: f64) -> Recommended<bool> {
    recommend(training_age_years > 1.0, "CONC-INTERF-001")
}

/// Expect strength/hypertrophy attenuation when endurance dosing is high
/// (File 10 hybrid-014; CONC-INTERF-001): frequency above 3–4 d/wk OR intensity
/// above 80 % VO2max. `true` = expect measurable interference; cap endurance or
/// lower lifting-gain expectations. Uses the >3 d/wk edge (the lower, more
/// protective bound of the "3–4" range).
pub fn interference_expected(
    endurance_days_per_week: u8,
    endurance_intensity_pct_vo2max: f64,
) -> Recommended<bool> {
    let hit = endurance_days_per_week > 3 || endurance_intensity_pct_vo2max > 80.0;
    recommend(hit, "CONC-INTERF-001")
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
pub fn maintenance_dose_fraction() -> Recommended<f64> {
    recommend(1.0 / 3.0, "HYB-CAP-001")
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
/// (File 10 hybrid-023; SAFE-BSI-001, safety-critical). Resistance training is
/// protective when energy availability is adequate. `true` = heighten monitoring.
pub fn bsi_surveillance_flag(running_km_per_week: f64) -> Recommended<bool> {
    recommend(running_km_per_week > 64.0, "SAFE-BSI-001")
}

/// Combined-load running progression guard (File 10 hybrid-021, safety layer):
/// keep weekly running-volume growth ≤ ~10 %/wk to bound stacked mechanical +
/// systemic load. Returns whether next week's volume is within the cap.
///
/// NOTE: the ACWR "sweet spot" (0.8–1.3) from this rule is deliberately NOT
/// encoded, it is the hard-blocked `LOAD-ACWR-001` myth (mathematically
/// coupled; see `load.rs`). The ≤10 %/wk ramp is the guardrail we act on.
pub fn combined_load_progression_ok(current_km: f64, next_km: f64) -> Recommended<bool> {
    let ok = if current_km <= 0.0 {
        false
    } else {
        (next_km - current_km) / current_km <= 0.10 + 1e-9
    };
    recommend(ok, "RUN-PROGRESS-001")
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
/// `true` = insert a deload / recovery block. Cited to overtraining
/// evidence (`SAFE-OTS-001`, Strong).
pub fn combined_fatigue_deload(red_flag_count: u8, weeks_persisted: u8) -> Recommended<bool> {
    recommend(red_flag_count >= 2 && weeks_persisted >= 1, "SAFE-OTS-001")
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
}
