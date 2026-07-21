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
use crate::schema::{Adjustment, HealthScreen, Recommended};

/// Build a `Recommended<T>` from a registry claim id (must exist).
fn recommend<T>(value: T, claim_id: &str) -> Recommended<T> {
    let e = evidence::claim(claim_id).expect("known individualization claim");
    Recommended::new(value, e.to_evidence(), e.to_confidence_tag())
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
/// (File 08 indiv-001; INDIV-TRAGE-001, ExpertOpinion, Rippetoe cadence
/// heuristic, not the Strong dose-response claim).
pub fn training_age_from_cadence(cadence: ProgressionCadence) -> Recommended<TrainingAge> {
    let age = match cadence {
        ProgressionCadence::EverySession => TrainingAge::Novice,
        ProgressionCadence::WeekToWeek => TrainingAge::Intermediate,
        ProgressionCadence::MonthToMonth => TrainingAge::Advanced,
    };
    recommend(age, "INDIV-TRAGE-001")
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
// 3. Scaling hierarchy (File 08 scaling-028/029; SCALE-DOWN-001 / SCALE-UP-001)
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
/// adaptations retained if intensity held while volume is cut). SCALE-DOWN-001
/// (Strong).
pub fn scale_down_order() -> Recommended<[ScaleLever; 5]> {
    recommend(
        [
            ScaleLever::AccessoryVolume,
            ScaleLever::SetsTowardMev,
            ScaleLever::Frequency,
            ScaleLever::SecondaryQuality,
            ScaleLever::IntensityAndMainCompounds,
        ],
        "SCALE-DOWN-001",
    )
}

/// Ordered scale-UP hierarchy: add volume and frequency before intensity, add
/// secondary quality only once the primary is progressing (File 08 scaling-029).
/// The inverse priority to scale-down. SCALE-UP-001 (ExpertOpinion ordering,
/// unlike the Strong scale-down evidence).
pub fn scale_up_order() -> Recommended<[ScaleLever; 5]> {
    recommend(
        [
            ScaleLever::SetsTowardMev,
            ScaleLever::Frequency,
            ScaleLever::AccessoryVolume,
            ScaleLever::IntensityAndMainCompounds,
            ScaleLever::SecondaryQuality,
        ],
        "SCALE-UP-001",
    )
}

/// Minimum weekly exposures per muscle to preserve when consolidating frequency
/// (File 08 scaling-028). SCALE-DOWN-001.
pub fn min_muscle_exposures_per_week() -> Recommended<u8> {
    recommend(2, "SCALE-DOWN-001")
}

// ---------------------------------------------------------------------------
// 4. Re-entry after a layoff (File 08 Table 3.4b; REENTRY-001)
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
/// intensity where possible, rebuild volume. REENTRY-001 (ExpertOpinion, not
/// the Moderate detraining-timeline claim).
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
    recommend(r, "REENTRY-001")
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
///
/// SAFETY GUARD (File 08 safety-022, safety-critical): "IF caloric deficit
/// requested AND any RED-S/disordered-eating signal present THEN do NOT
/// prescribe the deficit; route to RED-S deferral." (The KB's "safety-035"
/// cross-ref is a numbering bug: the RED-S absolute rule is safety-049.)
/// The refusal lives *inside* this function, not only in the global autoreg
/// deferral, so no call path can obtain a deficit target past a RED-S flag:
/// with `reds_signal_present` the value is `None` and the row is cited to the
/// RED-S deferral claim (SAFE-REDS-001, Strong, safety-critical), reduce/rest
/// training stress and defer to a physician / registered dietitian /
/// mental-health professional.
pub fn deficit_protein_target(reds_signal_present: bool) -> Recommended<Option<ProteinTarget>> {
    if reds_signal_present {
        recommend(None, "SAFE-REDS-001")
    } else {
        recommend(
            Some(ProteinTarget {
                g_per_kg: (1.8, 2.7),
            }),
            "DEFICIT-001",
        )
    }
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
///
/// Carries ONLY what the KB states (HARD RULE 1). The KB gives NO intensity/
/// pace reduction factor or percentage for any environment, NO temperature/
/// WBGT/humidity trigger, NO hydration quantity, and NO altitude
/// acclimatization day count ("until acclimatized" only), so those are
/// qualitative flags / `Option`s here, never invented numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvironmentModifier {
    /// Reduce absolute intensity/pace (qualitative, the KB states no factor).
    pub reduce_intensity: bool,
    /// Progressive acclimatization window in days, `(min, max)`. Stated only
    /// for heat (~10–14 days, safety-024); altitude has NO stated day count
    /// ("depressed performance until acclimatized") → `None`.
    pub acclimatization_days: Option<(u8, u8)>,
    /// Extend the warm-up (cold, indiv-025, the ONLY cold guidance stated).
    pub extend_warm_up: bool,
    /// Hard STOP on heat-illness signs: confusion, cessation of sweating,
    /// dizziness (safety-024, safety-critical).
    pub stop_on_heat_illness_signs: bool,
}

/// The training environment the session is performed in.
/// Serde derives: crosses the JSON FFI as an optional profile field (bare
/// variant name), so a shell can declare heat/altitude/cold context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Environment {
    Heat,
    /// Altitude above the [`ALTITUDE_THRESHOLD_M`] trigger.
    Altitude,
    Cold,
    Neutral,
}

/// Altitude trigger threshold in metres (File 08 indiv-025: ">~2,500 m").
/// The only altitude number the KB states (besides the pregnancy avoid-list's
/// identical 2,500 m in safety-047).
pub const ALTITUDE_THRESHOLD_M: f64 = 2_500.0;

/// Environment-specific modifier (File 08 §1.5; ENV-001, ExpertOpinion -
/// safety-critical via the heat-illness STOP branch). Heat: reduce intensity,
/// acclimatize ~10–14 days, hydrate, shift to cooler time of day, STOP on
/// heat-illness signs. Altitude (>~2,500 m): reduce absolute intensity until
/// acclimatized (no day count stated). Cold: extend warm-up, nothing else.
pub fn environment_modifier(env: Environment) -> Recommended<EnvironmentModifier> {
    let none = EnvironmentModifier {
        reduce_intensity: false,
        acclimatization_days: None,
        extend_warm_up: false,
        stop_on_heat_illness_signs: false,
    };
    let m = match env {
        Environment::Heat => EnvironmentModifier {
            reduce_intensity: true,
            acclimatization_days: Some((10, 14)),
            stop_on_heat_illness_signs: true,
            ..none
        },
        Environment::Altitude => EnvironmentModifier {
            reduce_intensity: true,
            ..none
        },
        Environment::Cold => EnvironmentModifier {
            extend_warm_up: true,
            ..none
        },
        Environment::Neutral => none,
    };
    recommend(m, "ENV-001")
}

// ---------------------------------------------------------------------------
// 8. Stage-0 onboarding gates (File 08 onboard-050; safety-011/044/045/046/048)
// ---------------------------------------------------------------------------

/// Verbatim pregnancy warning-sign list (File 08 safety-046): any of these →
/// STOP and DEFER. Data for shells to display; the engine consumes the
/// [`HealthScreen::pregnancy_warning_sign`] flag.
pub static PREGNANCY_WARNING_SIGNS: &[&str] = &[
    "vaginal bleeding",
    "amniotic fluid leakage",
    "regular painful contractions",
    "dyspnea before exertion",
    "dizziness/faintness",
    "headache",
    "chest pain",
    "calf pain/swelling",
    "muscle weakness affecting balance",
    "decreased fetal movement",
];

/// Verbatim pregnancy contraindication conditions (File 08 safety-046) → DEFER.
pub static PREGNANCY_CONTRAINDICATIONS: &[&str] = &[
    "placenta previa after 26 wk",
    "preeclampsia/gestational hypertension",
    "incompetent cervix",
    "severe anemia",
];

/// Pregnancy avoid-list (File 08 safety-047; SAFE-PREG-AVOID-001, Strong,
/// safety-critical). Every flag is a verbatim KB item; the altitude bound is
/// the only number stated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PregnancyPrecautions {
    pub avoid_prolonged_supine: bool,
    pub avoid_overheating: bool,
    pub avoid_contact_fall_risk: bool,
    pub avoid_scuba: bool,
    /// Avoid exercise at high altitude above this many metres (>2,500 m).
    pub avoid_altitude_above_m: f64,
    /// Avoid breath-holding (Valsalva) during strength work.
    pub avoid_valsalva: bool,
}

/// The pregnancy avoid-list (File 08 safety-047; ACOG 804). Surfaced whenever
/// the profile reports pregnancy, alongside the safety-045 deferral.
pub fn pregnancy_precautions() -> Recommended<PregnancyPrecautions> {
    recommend(
        PregnancyPrecautions {
            avoid_prolonged_supine: true,
            avoid_overheating: true,
            avoid_contact_fall_risk: true,
            avoid_scuba: true,
            avoid_altitude_above_m: 2_500.0,
            avoid_valsalva: true,
        },
        "SAFE-PREG-AVOID-001",
    )
}

/// Run the Stage-0 onboarding gates (File 08 onboard-050: screen → route →
/// classify) over the health screen, returning EVERY deferral that fires, each
/// cited to its own safety rule. An empty vec = screen clear, programming may
/// proceed. Deterministic order: acute pregnancy warning first, then the
/// onboard-050 route order (PAR-Q+ → pregnancy → injury → pediatric → RED-S).
///
/// safety-000 global precedence: none of these deferrals may ever be
/// overridden to satisfy a user's stated goal, safety > goals (HARD RULE 3).
pub fn onboarding_gates(s: &HealthScreen) -> Vec<Recommended<Adjustment>> {
    let mut out = Vec::new();
    if s.pregnancy_warning_sign {
        // safety-046: warning signs → STOP and DEFER.
        out.push(recommend(
            Adjustment::Defer {
                reason: "Pregnancy warning sign reported - STOP exercising now and contact your obstetric provider."
                    .into(),
            },
            "SAFE-PREG-WARN-001",
        ));
    }
    if s.parq_positive && !s.medically_cleared {
        // safety-044 + onboard-050: positive PAR-Q+/ACSM screen → medical
        // clearance gate before any prescription.
        out.push(recommend(
            Adjustment::Defer {
                reason: "Positive PAR-Q+/ACSM screen (cardiovascular, metabolic, or renal condition; uncontrolled hypertension; recent surgery; or acute illness) - medical clearance is required before programming."
                    .into(),
            },
            "SAFE-CVD-001",
        ));
    }
    if s.pregnant {
        // safety-045: no autonomous prescription/progression in pregnancy;
        // provider clearance + individualization. The ~150 min/wk moderate
        // figure is the KB's reference target for uncomplicated pregnancy -
        // surfaced as context, NOT an engine prescription.
        out.push(recommend(
            Adjustment::Defer {
                reason: "Pregnancy - the engine does not autonomously prescribe or progress load; train under provider clearance and individual guidance (reference for uncomplicated pregnancy: ~150 min/wk moderate activity)."
                    .into(),
            },
            "SAFE-PREG-001",
        ));
    }
    if s.injury_or_rehab {
        // safety-048: the engine never prescribes rehabilitation.
        out.push(recommend(
            Adjustment::Defer {
                reason: "Current injury under care, recent surgery, or active rehab - defer to your physician/physiotherapist; general programming resumes on clearance."
                    .into(),
            },
            "SAFE-INJURY-001",
        ));
    }
    if s.youth {
        // safety-011: pediatric/adolescent, supervision + technique-first;
        // never autonomous maximal loading or 1RM testing.
        out.push(recommend(
            Adjustment::Defer {
                reason: "Child/adolescent user - train only under qualified supervision, technique-first; the engine will not prescribe maximal loading or 1RM testing."
                    .into(),
            },
            "SAFE-PEDS-001",
        ));
    }
    if s.reds_signal {
        // safety-049 absolute rule (via onboard-050 screening): never a
        // programming variable: reduce/rest and defer.
        out.push(recommend(
            Adjustment::Defer {
                reason: "RED-S / disordered-eating signal - reduce or rest training stress and defer to a physician, registered dietitian, or mental-health professional."
                    .into(),
            },
            "SAFE-REDS-001",
        ));
    }
    out
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
        assert_eq!(
            deficit_protein_target(false).value.expect("no RED-S").g_per_kg,
            (1.8, 2.7)
        );
        // Deficit protein guidance is Strong (Helms/Longland).
        assert!((deficit_protein_target(false).confidence.score - 0.90).abs() < f32::EPSILON);
        assert_eq!(masters_protein_per_meal().value, 0.4);
    }

    #[test]
    fn deficit_refused_inside_the_fn_when_reds_signal_present() {
        // File 08 safety-022: deficit request + RED-S signal → the target fn
        // itself refuses (no call path can obtain a deficit past the flag) and
        // the refusal is cited to the RED-S deferral, safety-critical.
        let blocked = deficit_protein_target(true);
        assert!(blocked.value.is_none(), "no deficit target under RED-S");
        assert_eq!(
            blocked.evidence.citation.claim_id.as_deref(),
            Some("SAFE-REDS-001")
        );
        assert!(blocked.confidence.safety_critical);
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
        // Heat: reduce intensity (no KB factor), acclimatize 10–14 days, hard
        // heat-illness stop.
        let heat = environment_modifier(Environment::Heat).value;
        assert!(heat.reduce_intensity);
        assert_eq!(heat.acclimatization_days, Some((10, 14)));
        assert!(heat.stop_on_heat_illness_signs);
        assert!(!heat.extend_warm_up);
        // Altitude: reduce intensity until acclimatized; the KB states NO day
        // count for altitude (unlike heat), and no illness stop.
        let alt = environment_modifier(Environment::Altitude).value;
        assert!(alt.reduce_intensity);
        assert_eq!(alt.acclimatization_days, None, "no altitude day count in KB");
        assert!(!alt.stop_on_heat_illness_signs);
        assert_eq!(ALTITUDE_THRESHOLD_M, 2_500.0);
        // Cold: extend warm-up only; the KB states nothing else for cold.
        let cold = environment_modifier(Environment::Cold).value;
        assert!(cold.extend_warm_up);
        assert!(!cold.reduce_intensity);
        assert_eq!(cold.acclimatization_days, None);
        // Neutral leaves the prescription unchanged.
        let neutral = environment_modifier(Environment::Neutral).value;
        assert!(!neutral.reduce_intensity && !neutral.extend_warm_up);
        // ENV-001 is safety-critical via the heat STOP branch.
        assert!(
            environment_modifier(Environment::Heat)
                .confidence
                .safety_critical
        );
    }

    // --- Stage-0 onboarding gates (File 08 onboard-050) ---

    #[test]
    fn clear_health_screen_yields_no_gates() {
        assert!(onboarding_gates(&HealthScreen::default()).is_empty());
        assert!(!HealthScreen::default().any_gate());
    }

    #[test]
    fn each_screen_flag_defers_with_its_own_safety_claim() {
        let cases: &[(fn(&mut HealthScreen), &str)] = &[
            (|s| s.youth = true, "SAFE-PEDS-001"),
            (|s| s.parq_positive = true, "SAFE-CVD-001"),
            (|s| s.pregnant = true, "SAFE-PREG-001"),
            (|s| s.pregnancy_warning_sign = true, "SAFE-PREG-WARN-001"),
            (|s| s.injury_or_rehab = true, "SAFE-INJURY-001"),
            (|s| s.reds_signal = true, "SAFE-REDS-001"),
        ];
        for (set, claim_id) in cases {
            let mut s = HealthScreen::default();
            set(&mut s);
            let gates = onboarding_gates(&s);
            assert_eq!(gates.len(), 1, "{claim_id}: exactly one gate fires");
            assert!(
                matches!(gates[0].value, Adjustment::Defer { .. }),
                "{claim_id}: gate must defer to a professional"
            );
            assert_eq!(
                gates[0].evidence.citation.claim_id.as_deref(),
                Some(*claim_id)
            );
            assert!(
                gates[0].confidence.safety_critical,
                "{claim_id}: every onboarding gate is safety-critical"
            );
            assert!(s.any_gate());
        }
    }

    #[test]
    fn medical_clearance_clears_only_the_parq_gate() {
        // safety-044: clearance re-opens programming after a positive screen…
        let cleared = HealthScreen {
            parq_positive: true,
            medically_cleared: true,
            ..HealthScreen::default()
        };
        assert!(onboarding_gates(&cleared).is_empty());
        // …but pregnancy keeps deferring autonomous prescription regardless
        // (safety-045: provider clearance AND individualization, the engine
        // still must not autonomously prescribe/progress load).
        let pregnant_cleared = HealthScreen {
            pregnant: true,
            medically_cleared: true,
            ..HealthScreen::default()
        };
        assert_eq!(onboarding_gates(&pregnant_cleared).len(), 1);
    }

    #[test]
    fn pediatric_gate_names_the_prohibitions() {
        // safety-011: no autonomous maximal loading or 1RM testing.
        let s = HealthScreen {
            youth: true,
            ..HealthScreen::default()
        };
        let gates = onboarding_gates(&s);
        match &gates[0].value {
            Adjustment::Defer { reason } => {
                assert!(reason.contains("supervision"));
                assert!(reason.contains("1RM"));
            }
            other => panic!("expected Defer, got {other:?}"),
        }
        // Strong evidence (Lloyd/Faigenbaum consensus), never overstated.
        assert!((gates[0].confidence.score - 0.90).abs() < f32::EPSILON);
    }

    #[test]
    fn pregnancy_gate_emits_safe_preg_001_with_reference_target() {
        // safety-045: SAFE-PREG-001 actually emitted; the ~150 min/wk figure is
        // a reference target only, phrased as such.
        let s = HealthScreen {
            pregnant: true,
            ..HealthScreen::default()
        };
        let gates = onboarding_gates(&s);
        assert_eq!(
            gates[0].evidence.citation.claim_id.as_deref(),
            Some("SAFE-PREG-001")
        );
        match &gates[0].value {
            Adjustment::Defer { reason } => {
                assert!(reason.contains("150 min/wk"));
                assert!(reason.contains("reference"));
            }
            other => panic!("expected Defer, got {other:?}"),
        }
    }

    #[test]
    fn injury_gate_is_expert_opinion_yet_safety_critical() {
        // safety-048 is the lowest-graded File 08 safety rule (ExpertOpinion,
        // 0.30) but still safety-critical: the grade is never inflated to make
        // the stop look better-evidenced.
        let s = HealthScreen {
            injury_or_rehab: true,
            ..HealthScreen::default()
        };
        let g = &onboarding_gates(&s)[0];
        assert!((g.confidence.score - 0.30).abs() < f32::EPSILON);
        assert!(g.confidence.safety_critical);
    }

    #[test]
    fn multiple_flags_all_surface_with_warning_sign_first() {
        let s = HealthScreen {
            pregnant: true,
            pregnancy_warning_sign: true,
            injury_or_rehab: true,
            ..HealthScreen::default()
        };
        let gates = onboarding_gates(&s);
        assert_eq!(gates.len(), 3, "every fired gate must surface");
        assert_eq!(
            gates[0].evidence.citation.claim_id.as_deref(),
            Some("SAFE-PREG-WARN-001"),
            "acute warning sign leads"
        );
    }

    #[test]
    fn pregnancy_precautions_verbatim_avoid_list() {
        let p = pregnancy_precautions();
        assert!(p.value.avoid_prolonged_supine);
        assert!(p.value.avoid_overheating);
        assert!(p.value.avoid_contact_fall_risk);
        assert!(p.value.avoid_scuba);
        assert_eq!(p.value.avoid_altitude_above_m, 2_500.0);
        assert!(p.value.avoid_valsalva);
        assert_eq!(
            p.evidence.citation.claim_id.as_deref(),
            Some("SAFE-PREG-AVOID-001")
        );
        assert!(p.confidence.safety_critical);
        // Warning-sign / contraindication lists carry the verbatim KB items.
        assert_eq!(PREGNANCY_WARNING_SIGNS.len(), 10);
        assert_eq!(PREGNANCY_CONTRAINDICATIONS.len(), 4);
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
