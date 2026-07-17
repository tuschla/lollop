//! Individualization & load-management core (knowledge-base File 08).
//!
//! Pure, deterministic rules for tailoring the program to training age and for
//! scaling load up/down or re-entering after a layoff. Safety deferrals from
//! File 08 §5 live in the evidence registry + `schema::SafetyTier`; this module
//! holds the *non-safety* individualization arithmetic.
//!
//! Training-age strength defaults are transcribed verbatim from File 08
//! Table 1.1 (indiv-002/003/004, Rhea 2003 → STR-TRAGE-001). Scaling order,
//! re-entry brackets, and detraining rates come from File 08 §2/§3.4
//! (scaling-028/029, load-036/037 → DETRAIN-001).

use crate::evidence;
use crate::schema::Recommended;

/// Build a `Recommended<T>` from a registry claim id (must exist).
fn recommend<T>(value: T, claim_id: &str) -> Recommended<T> {
    let e = evidence::claim(claim_id).expect("known individualization claim");
    Recommended {
        value,
        evidence: e.to_evidence(),
        confidence: e.to_confidence_tag(),
    }
}

// ---------------------------------------------------------------------------
// 1. Training age (File 08 indiv-001; STR-TRAGE-001)
// ---------------------------------------------------------------------------

/// Training age classified by *progression history*, not self-label (indiv-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingAge {
    /// Progresses load essentially every session (~first 3-6 months).
    Novice,
    /// Progresses week-to-week.
    Intermediate,
    /// Progresses month-to-month in periodized blocks.
    Advanced,
}

/// Observed cadence at which the lifter can still add load (indiv-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProgressionCadence {
    EverySession,
    WeekToWeek,
    MonthToMonth,
}

/// Classify training age from the fastest cadence the lifter still progresses at
/// (File 08 indiv-001; STR-TRAGE-001).
pub fn training_age_from_cadence(cadence: ProgressionCadence) -> Recommended<TrainingAge> {
    let age = match cadence {
        ProgressionCadence::EverySession => TrainingAge::Novice,
        ProgressionCadence::WeekToWeek => TrainingAge::Intermediate,
        ProgressionCadence::MonthToMonth => TrainingAge::Advanced,
    };
    recommend(age, "STR-TRAGE-001")
}

// ---------------------------------------------------------------------------
// 2. Strength defaults by training age (File 08 Table 1.1; STR-TRAGE-001)
// ---------------------------------------------------------------------------

/// Population-average strength starting points by training age (File 08
/// indiv-002/003/004, Table 1.1). Autoregulate away from these defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrengthDefaults {
    /// Optimal working intensity as %1RM (60 / 80 / 85).
    pub intensity_pct_1rm: u8,
    /// Sessions per muscle per week (3 / 2 / 2).
    pub freq_per_muscle: u8,
    /// Working sets per muscle (4 / 4 / 8 for athletes).
    pub sets_per_muscle: u8,
}

/// Strength intensity/frequency/volume defaults for a training age (File 08
/// Table 1.1; indiv-002/003/004; STR-TRAGE-001).
pub fn strength_defaults(age: TrainingAge) -> Recommended<StrengthDefaults> {
    let d = match age {
        TrainingAge::Novice => StrengthDefaults {
            intensity_pct_1rm: 60,
            freq_per_muscle: 3,
            sets_per_muscle: 4,
        },
        TrainingAge::Intermediate => StrengthDefaults {
            intensity_pct_1rm: 80,
            freq_per_muscle: 2,
            sets_per_muscle: 4,
        },
        TrainingAge::Advanced => StrengthDefaults {
            intensity_pct_1rm: 85,
            freq_per_muscle: 2,
            sets_per_muscle: 8,
        },
    };
    recommend(d, "STR-TRAGE-001")
}

/// Whether added volume returns disproportionately more for this athlete
/// (indiv-006: 1→4-set ES gain +1.12 untrained vs +0.70 advanced). True for
/// novices/intermediates, who are more volume-sensitive. STR-TRAGE-001.
pub fn high_volume_sensitivity(age: TrainingAge) -> Recommended<bool> {
    recommend(!matches!(age, TrainingAge::Advanced), "STR-TRAGE-001")
}

// ---------------------------------------------------------------------------
// 3. Scaling hierarchy (File 08 scaling-028/029; DETRAIN-001)
// ---------------------------------------------------------------------------

/// A lever the engine manipulates when scaling training stress. Ordered from
/// most-disposable to most-protected (File 08 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleLever {
    /// Accessory / isolation volume, cut first, add last.
    AccessoryVolume,
    /// Sets per muscle toward MEV (never below maintenance).
    SetsTowardMev,
    /// Training frequency (keep >=2 exposures/muscle/wk).
    Frequency,
    /// Secondary-quality work (the non-priority modality).
    SecondaryQuality,
    /// Intensity / load on main compounds, removed last, added last.
    IntensityAndMainCompounds,
}

/// Ordered scale-DOWN hierarchy: shed accessory volume first, protect intensity
/// and main compounds last (File 08 scaling-028; Mujika & Padilla 2000 -
/// adaptations retained if intensity held while volume is cut). DETRAIN-001.
pub fn scale_down_order() -> Recommended<[ScaleLever; 5]> {
    recommend(
        [
            ScaleLever::AccessoryVolume,
            ScaleLever::SetsTowardMev,
            ScaleLever::Frequency,
            ScaleLever::SecondaryQuality,
            ScaleLever::IntensityAndMainCompounds,
        ],
        "DETRAIN-001",
    )
}

/// Ordered scale-UP hierarchy: add volume and frequency before intensity, add
/// secondary quality only once the primary is progressing (File 08 scaling-029).
/// The inverse priority to scale-down. DETRAIN-001.
pub fn scale_up_order() -> Recommended<[ScaleLever; 5]> {
    recommend(
        [
            ScaleLever::SetsTowardMev,
            ScaleLever::Frequency,
            ScaleLever::AccessoryVolume,
            ScaleLever::IntensityAndMainCompounds,
            ScaleLever::SecondaryQuality,
        ],
        "DETRAIN-001",
    )
}

/// Minimum weekly exposures per muscle to preserve when consolidating frequency
/// (File 08 scaling-028). DETRAIN-001.
pub fn min_muscle_exposures_per_week() -> Recommended<u8> {
    recommend(2, "DETRAIN-001")
}

// ---------------------------------------------------------------------------
// 4. Re-entry after a layoff (File 08 Table 3.4b; DETRAIN-001)
// ---------------------------------------------------------------------------

/// Conservative resistance re-entry prescription after a training gap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReEntry {
    /// Fraction of prior working loads to start at.
    pub load_frac: f64,
    /// Weeks to ramp back to full load (min, max).
    pub ramp_weeks: (u8, u8),
    /// True once the layoff is long enough to treat the lifter as a novice again.
    pub treat_as_novice: bool,
}

/// Resistance re-entry bracket by weeks off (File 08 Table 3.4b, conservative
/// ExpertOpinion default extrapolated from Mujika & Padilla 2000). Hold
/// intensity where possible, rebuild volume. DETRAIN-001.
pub fn resistance_reentry(weeks_off: f64) -> Recommended<ReEntry> {
    let r = if weeks_off < 1.0 {
        ReEntry {
            load_frac: 1.00,
            ramp_weeks: (0, 0),
            treat_as_novice: false,
        }
    } else if weeks_off < 2.0 {
        ReEntry {
            load_frac: 0.90,
            ramp_weeks: (1, 1),
            treat_as_novice: false,
        }
    } else if weeks_off < 4.0 {
        ReEntry {
            load_frac: 0.825,
            ramp_weeks: (1, 2),
            treat_as_novice: false,
        }
    } else if weeks_off < 8.0 {
        ReEntry {
            load_frac: 0.70,
            ramp_weeks: (2, 4),
            treat_as_novice: false,
        }
    } else {
        ReEntry {
            load_frac: 0.50,
            ramp_weeks: (4, 6),
            treat_as_novice: true,
        }
    };
    recommend(r, "DETRAIN-001")
}

// ---------------------------------------------------------------------------
// 5. Detraining timelines (File 08 Table 3.4a; descriptive data, not advice)
// ---------------------------------------------------------------------------

/// A trainable quality and how fast it decays without exposure (File 08
/// Table 3.4a). Raw descriptive rates, not wrapped, as they prescribe nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetrainRate {
    pub quality: &'static str,
    /// Approximate weeks until meaningful loss begins.
    pub onset_weeks: f64,
    /// Illustrative loss note.
    pub note: &'static str,
}

/// Detraining timelines, most-protected to least (File 08 Table 3.4a).
pub static DETRAINING: &[DetrainRate] = &[
    DetrainRate {
        quality: "strength",
        onset_weeks: 8.0,
        note: "~7-12% loss over 8-12wk; most protected (neural)",
    },
    DetrainRate {
        quality: "hypertrophy",
        onset_weeks: 3.0,
        note: "slow loss; myonuclei aid re-gain (muscle memory)",
    },
    DetrainRate {
        quality: "vo2max",
        onset_weeks: 2.0,
        note: "fastest; ~6-20% over ~4wk in highly trained",
    },
    DetrainRate {
        quality: "power",
        onset_weeks: 1.0,
        note: "fades within ~1wk of zero exposure",
    },
];

// ---------------------------------------------------------------------------
// 6. Progression & nutrition (File 08 load-031/indiv-008/013/020; DBLPROG/MASTERS/DEFICIT)
// ---------------------------------------------------------------------------

/// Novice linear load increment when all prescribed reps are completed
/// (File 08 indiv-008, Table 1.1). Upper +2.5 kg, lower +5 kg. DBLPROG-001.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadIncrement {
    /// kg to add to upper-body lifts next session.
    pub upper_kg: f64,
    /// kg to add to lower-body lifts next session.
    pub lower_kg: f64,
}

/// Novice linear-progression load bump when the session was completed
/// (File 08 indiv-008; DBLPROG-001).
pub fn novice_load_increment() -> Recommended<LoadIncrement> {
    recommend(
        LoadIncrement {
            upper_kg: 2.5,
            lower_kg: 5.0,
        },
        "DBLPROG-001",
    )
}

/// Double progression: once the top of the rep range is hit on every set, add
/// load and drop to the range bottom (File 08 load-031). Returns `true` when the
/// engine should add load this session. DBLPROG-001.
pub fn double_progression_add_load(top_of_range_all_sets: bool) -> Recommended<bool> {
    recommend(top_of_range_all_sets, "DBLPROG-001")
}

/// Daily protein target as g/kg bodyweight (min, max) for a goal context
/// (File 08 indiv-013/020).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProteinTarget {
    pub g_per_kg: (f64, f64),
}

/// Masters (65+) protein target: 1.2-1.6 g/kg/day for anabolic resistance
/// (File 08 indiv-013; MASTERS-001).
pub fn masters_protein_target() -> Recommended<ProteinTarget> {
    recommend(
        ProteinTarget {
            g_per_kg: (1.2, 1.6),
        },
        "MASTERS-001",
    )
}

/// Lean-mass-preserving deficit protein target: 1.8-2.7 g/kg/day, hold intensity
/// and cut volume toward MEV (File 08 indiv-020; DEFICIT-001).
pub fn deficit_protein_target() -> Recommended<ProteinTarget> {
    recommend(
        ProteinTarget {
            g_per_kg: (1.8, 2.7),
        },
        "DEFICIT-001",
    )
}

/// Masters (65+) per-meal protein dose in g/kg bodyweight to overcome anabolic
/// resistance (File 08 indiv-013): ~0.4 g/kg per meal (vs the lower per-meal
/// dose sufficient in younger adults). MASTERS-001.
pub fn masters_protein_per_meal() -> Recommended<f64> {
    recommend(0.4, "MASTERS-001")
}

/// Novice linear-progression stall response (File 08 indiv-009, Starting
/// Strength). `None` until a lift misses target reps for ≥3 consecutive
/// sessions *with adequate sleep/food*; then deload that lift 10% and re-ramp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoviceStallOutcome {
    /// Fraction to deload the stalling lift (0.10).
    pub deload_frac: f64,
    /// Isolate the stalling lift only, do not touch other lifts.
    pub scope_single_lift: bool,
    /// If it stalls again after the re-ramp, transition THAT lift to
    /// intermediate weekly progression.
    pub transition_to_intermediate: bool,
}

/// Decide the novice stall response (File 08 indiv-009). Fires only after 3
/// consecutive failed sessions with adequate recovery; `stalled_again_after_reramp`
/// escalates to an intermediate-progression transition for that lift alone.
/// Rippetoe/Starting Strength (ExpertOpinion); linear-progression governance
/// under DBLPROG-001.
pub fn novice_stall_action(
    consecutive_failed_sessions: u8,
    adequate_recovery: bool,
    stalled_again_after_reramp: bool,
) -> Recommended<Option<NoviceStallOutcome>> {
    let outcome = if consecutive_failed_sessions >= 3 && adequate_recovery {
        Some(NoviceStallOutcome {
            deload_frac: 0.10,
            scope_single_lift: true,
            transition_to_intermediate: stalled_again_after_reramp,
        })
    } else {
        None
    };
    recommend(outcome, "DBLPROG-001")
}

// ---------------------------------------------------------------------------
// 7. Time budget, substitution & environment (File 08 indiv-023/026, safety-024)
// ---------------------------------------------------------------------------

/// Weekly frequency a muscle can be *maintained* on under time pressure
/// (File 08 indiv-026: ~1x/wk holds a muscle; protect frequency + intensity,
/// cut accessory volume first). TIMECAP-001.
pub fn maintenance_frequency_per_week() -> Recommended<u8> {
    recommend(1, "TIMECAP-001")
}

/// Substitution rule for limited equipment: substitute the movement pattern and
/// compensate lighter loads with higher reps near failure (File 08 indiv-023).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubstitutionRule {
    /// Keep the same movement pattern, swap the implement.
    pub match_movement_pattern: bool,
    /// Take the lighter-load sets closer to failure to preserve hypertrophy.
    pub compensate_with_reps_near_failure: bool,
}

/// Movement-pattern substitution default for home/minimal equipment
/// (File 08 indiv-023; SUBST-001).
pub fn substitution_rule() -> Recommended<SubstitutionRule> {
    recommend(
        SubstitutionRule {
            match_movement_pattern: true,
            compensate_with_reps_near_failure: true,
        },
        "SUBST-001",
    )
}

/// Environmental training modifier (File 08 §1.5 Table; safety-024/indiv-025).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvironmentModifier {
    /// Multiply prescribed intensity/pace by this factor (1.0 = unchanged).
    pub intensity_factor: f64,
    /// Days of progressive acclimatization advised before full load.
    pub acclimatization_days: u8,
    /// True when heat-illness signs (confusion, no sweating, dizziness) mean STOP.
    pub stop_on_illness_signs: bool,
}

/// The training environment the session is performed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Heat,
    /// Altitude above ~2,500 m.
    Altitude,
    Cold,
    Neutral,
}

/// Environment-specific intensity/acclimatization modifier (File 08 §1.5;
/// ENV-001). Heat and altitude reduce absolute intensity; heat carries a
/// hard heat-illness stop.
pub fn environment_modifier(env: Environment) -> Recommended<EnvironmentModifier> {
    let m = match env {
        Environment::Heat => EnvironmentModifier {
            intensity_factor: 0.90,
            acclimatization_days: 14,
            stop_on_illness_signs: true,
        },
        Environment::Altitude => EnvironmentModifier {
            intensity_factor: 0.90,
            acclimatization_days: 7,
            stop_on_illness_signs: false,
        },
        Environment::Cold => EnvironmentModifier {
            intensity_factor: 1.00,
            acclimatization_days: 0,
            stop_on_illness_signs: false,
        },
        Environment::Neutral => EnvironmentModifier {
            intensity_factor: 1.00,
            acclimatization_days: 0,
            stop_on_illness_signs: false,
        },
    };
    recommend(m, "ENV-001")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_maps_to_training_age() {
        assert_eq!(
            training_age_from_cadence(ProgressionCadence::EverySession).value,
            TrainingAge::Novice
        );
        assert_eq!(
            training_age_from_cadence(ProgressionCadence::WeekToWeek).value,
            TrainingAge::Intermediate
        );
        assert_eq!(
            training_age_from_cadence(ProgressionCadence::MonthToMonth).value,
            TrainingAge::Advanced
        );
    }

    #[test]
    fn strength_defaults_match_table_1_1() {
        let n = strength_defaults(TrainingAge::Novice).value;
        assert_eq!(
            (n.intensity_pct_1rm, n.freq_per_muscle, n.sets_per_muscle),
            (60, 3, 4)
        );
        let i = strength_defaults(TrainingAge::Intermediate).value;
        assert_eq!(
            (i.intensity_pct_1rm, i.freq_per_muscle, i.sets_per_muscle),
            (80, 2, 4)
        );
        let a = strength_defaults(TrainingAge::Advanced).value;
        assert_eq!(
            (a.intensity_pct_1rm, a.freq_per_muscle, a.sets_per_muscle),
            (85, 2, 8)
        );
        // Defaults carry Strong evidence (Rhea 2003).
        assert!(
            (strength_defaults(TrainingAge::Novice).confidence.score - 0.90).abs() < f32::EPSILON
        );
    }

    #[test]
    fn volume_sensitivity_higher_in_less_trained() {
        assert!(high_volume_sensitivity(TrainingAge::Novice).value);
        assert!(high_volume_sensitivity(TrainingAge::Intermediate).value);
        assert!(!high_volume_sensitivity(TrainingAge::Advanced).value);
    }

    #[test]
    fn scaling_protects_intensity_last_and_first() {
        let down = scale_down_order().value;
        assert_eq!(down[0], ScaleLever::AccessoryVolume);
        assert_eq!(down[4], ScaleLever::IntensityAndMainCompounds);
        // Scale-up adds intensity late, secondary quality last.
        let up = scale_up_order().value;
        assert_eq!(up[0], ScaleLever::SetsTowardMev);
        assert_eq!(up[4], ScaleLever::SecondaryQuality);
        assert_eq!(min_muscle_exposures_per_week().value, 2);
    }

    #[test]
    fn reentry_brackets_scale_with_time_off() {
        assert_eq!(resistance_reentry(0.5).value.load_frac, 1.00);
        assert_eq!(resistance_reentry(1.5).value.load_frac, 0.90);
        assert_eq!(resistance_reentry(3.0).value.load_frac, 0.825);
        assert_eq!(resistance_reentry(6.0).value.load_frac, 0.70);
        let long = resistance_reentry(12.0).value;
        assert_eq!(long.load_frac, 0.50);
        assert!(long.treat_as_novice);
        // Monotonic non-increasing load fraction as time off grows.
        assert!(resistance_reentry(6.0).value.load_frac < resistance_reentry(3.0).value.load_frac);
    }

    #[test]
    fn progression_and_nutrition_targets() {
        let inc = novice_load_increment().value;
        assert_eq!((inc.upper_kg, inc.lower_kg), (2.5, 5.0));
        assert!(double_progression_add_load(true).value);
        assert!(!double_progression_add_load(false).value);
        assert_eq!(masters_protein_target().value.g_per_kg, (1.2, 1.6));
        assert_eq!(deficit_protein_target().value.g_per_kg, (1.8, 2.7));
        // Deficit protein guidance is Strong (Helms/Longland).
        assert!((deficit_protein_target().confidence.score - 0.90).abs() < f32::EPSILON);
        assert_eq!(masters_protein_per_meal().value, 0.4);
    }

    #[test]
    fn novice_stall_response() {
        assert!(novice_stall_action(2, true, false).value.is_none());
        assert!(novice_stall_action(3, false, false).value.is_none());
        let o = novice_stall_action(3, true, false).value.unwrap();
        assert_eq!(
            (
                o.deload_frac,
                o.scope_single_lift,
                o.transition_to_intermediate
            ),
            (0.10, true, false)
        );
        assert!(
            novice_stall_action(3, true, true)
                .value
                .unwrap()
                .transition_to_intermediate
        );
    }

    #[test]
    fn time_substitution_and_environment() {
        assert_eq!(maintenance_frequency_per_week().value, 1);
        let sub = substitution_rule().value;
        assert!(sub.match_movement_pattern && sub.compensate_with_reps_near_failure);
        // Heat cuts intensity and carries a hard illness stop.
        let heat = environment_modifier(Environment::Heat).value;
        assert_eq!(heat.intensity_factor, 0.90);
        assert!(heat.stop_on_illness_signs);
        // Altitude cuts intensity, no illness stop.
        assert_eq!(
            environment_modifier(Environment::Altitude)
                .value
                .intensity_factor,
            0.90
        );
        assert!(
            !environment_modifier(Environment::Altitude)
                .value
                .stop_on_illness_signs
        );
        // Neutral leaves prescription unchanged.
        assert_eq!(
            environment_modifier(Environment::Neutral)
                .value
                .intensity_factor,
            1.00
        );
    }

    #[test]
    fn detraining_ordered_most_to_least_protected() {
        assert_eq!(DETRAINING.first().unwrap().quality, "strength");
        assert_eq!(DETRAINING.last().unwrap().quality, "power");
        // Onset weeks strictly decrease down the list (more protected first).
        for w in DETRAINING.windows(2) {
            assert!(w[0].onset_weeks > w[1].onset_weeks);
        }
    }
}
