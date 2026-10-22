//! Individualization & load-management core (knowledge-base File 08).
//!
//! Pure, deterministic rules for tailoring the program to training age and for
//! scaling load up/down or re-entering after a layoff. Safety deferrals from
//! File 08 §5 live in the evidence registry + `schema::SafetyTier`; this module
//! holds the *non-safety* individualization arithmetic.
//!
//! Training-age strength defaults are transcribed verbatim from File 08
//! Table 1.1 (indiv-002/003/004, Rhea 2003 → STR-TRAGE-001). Scaling order and
//! re-entry brackets come from File 08 §2/§3.4 (scaling-028/029, Table 3.4b →
//! REENTRY-001).

use crate::evidence;
use crate::evidence::claims;
use crate::schema::{Adjustment, HealthScreen, Recommended};

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
    evidence::graded(age, claims::INDIV_TRAGE_001)
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
    evidence::graded(d, claims::STR_TRAGE_001)
}

/// How disproportionately added volume returns for this athlete along the
/// File 08 indiv-006 dose-response gradient (1→4-set effect-size gain +1.12
/// untrained vs +0.70 advanced): the less trained the lifter, the more each
/// added set buys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeSensitivity {
    /// Novice: largest marginal return per added set (indiv-006 ~+1.12 ES).
    High,
    /// Intermediate: middling return, between novice and advanced — its own
    /// honest tier, never collapsed into the novice "high" bucket.
    Moderate,
    /// Advanced: smallest marginal return per added set (indiv-006 ~+0.70 ES).
    Low,
}

/// Volume sensitivity for a training age along the indiv-006 gradient
/// (untrained > intermediate > advanced). STR-TRAGE-001.
pub fn volume_sensitivity(age: TrainingAge) -> Recommended<VolumeSensitivity> {
    let s = match age {
        TrainingAge::Novice => VolumeSensitivity::High,
        TrainingAge::Intermediate => VolumeSensitivity::Moderate,
        TrainingAge::Advanced => VolumeSensitivity::Low,
    };
    evidence::graded(s, claims::STR_TRAGE_001)
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
    evidence::graded(
        [
            ScaleLever::AccessoryVolume,
            ScaleLever::SetsTowardMev,
            ScaleLever::Frequency,
            ScaleLever::SecondaryQuality,
            ScaleLever::IntensityAndMainCompounds,
        ],
        claims::SCALE_DOWN_001,
    )
}

/// Ordered scale-UP hierarchy: add volume and frequency before intensity, add
/// secondary quality only once the primary is progressing (File 08 scaling-029).
/// The inverse priority to scale-down. SCALE-UP-001 (ExpertOpinion ordering,
/// unlike the Strong scale-down evidence).
pub fn scale_up_order() -> Recommended<[ScaleLever; 5]> {
    evidence::graded(
        [
            ScaleLever::SetsTowardMev,
            ScaleLever::Frequency,
            ScaleLever::AccessoryVolume,
            ScaleLever::IntensityAndMainCompounds,
            ScaleLever::SecondaryQuality,
        ],
        claims::SCALE_UP_001,
    )
}

/// Minimum weekly exposures per muscle to preserve when consolidating frequency
/// (File 08 scaling-028). SCALE-DOWN-001.
pub fn min_muscle_exposures_per_week() -> Recommended<u8> {
    evidence::graded(2, claims::SCALE_DOWN_001)
}

// ---------------------------------------------------------------------------
// 4. Re-entry after a layoff (File 08 Table 3.4b; REENTRY-001)
// ---------------------------------------------------------------------------

/// Conservative resistance re-entry prescription after a training gap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReEntry {
    /// Fraction of prior working loads to start at. `None` when the KB gives no
    /// number (>8 wk layoff): callers drop the e1RM anchor and progress like a
    /// novice instead of scaling the load.
    pub load_frac: Option<f64>,
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
            load_frac: Some(1.00),
            ramp_weeks: (0, 0),
            treat_as_novice: false,
        }
    } else if weeks_off < 2.0 {
        ReEntry {
            load_frac: Some(0.90),
            ramp_weeks: (1, 1),
            treat_as_novice: false,
        }
    } else if weeks_off < 4.0 {
        ReEntry {
            load_frac: Some(0.825),
            ramp_weeks: (1, 2),
            treat_as_novice: false,
        }
    } else if weeks_off <= 8.0 {
        ReEntry {
            load_frac: Some(0.70),
            ramp_weeks: (2, 4),
            treat_as_novice: false,
        }
    } else {
        // >8 wk: KB Table 3.4b states no load fraction, "treat as novice
        // re-entry; re-establish technique, rebuild over 4-6+ wk". So `None`:
        // callers drop the e1RM anchor and progress like a novice, never scale
        // the load by an invented number (HARD RULE 1).
        ReEntry {
            load_frac: None,
            ramp_weeks: (4, 6),
            treat_as_novice: true,
        }
    };
    evidence::graded(r, claims::REENTRY_001)
}

/// Where a declared layoff's re-entry ramp stands `days_since_return` days
/// after training resumed (KB reentry-002, REENTRY-RAMP-001): the bracket's
/// starting fraction ramps linearly back to full load over the bracket's ramp
/// window, `frac(t) = frac0 + (1 − frac0) · min(1, t / ramp_weeks_max)`. It
/// applies only to loads anchored BEFORE the layoff and replaces (never stacks
/// with) detraining decay of those anchors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReentryRamp {
    /// Load fraction for pre-layoff anchors today; `None` for a >8 wk layoff
    /// (no KB fraction: pre-layoff anchors are dropped while the window runs).
    pub frac: Option<f64>,
    /// Length of the ramp window in days (`ramp_weeks.1 × 7`).
    pub ramp_days: i64,
    /// `true` while the window is still running (`t < ramp_days`); `false`
    /// once it has ended — the layoff then no longer shapes any load.
    pub active: bool,
    /// The >8 wk "treat as a fresh novice" bracket.
    pub treat_as_novice: bool,
}

/// Evaluate the re-entry ramp for `weeks_off` declared weeks off, `t =
/// days_since_return` days after the return (a future return date counts as
/// day 0). REENTRY-RAMP-001 (ExpertOpinion ramp shape over the REENTRY-001
/// brackets; Bosquet 2013 detraining dose-response).
pub fn reentry_ramp(weeks_off: f64, days_since_return: i64) -> Recommended<ReentryRamp> {
    let re = resistance_reentry(weeks_off).value;
    let ramp_days = re.ramp_weeks.1 as i64 * 7;
    let t = days_since_return.max(0);
    let active = ramp_days > 0 && t < ramp_days && re.load_frac.is_none_or(|f| f < 1.0);
    let frac = re.load_frac.map(|f0| {
        if ramp_days <= 0 {
            1.0
        } else {
            f0 + (1.0 - f0) * (t as f64 / ramp_days as f64).min(1.0)
        }
    });
    evidence::graded(
        ReentryRamp {
            frac,
            ramp_days,
            active,
            treat_as_novice: re.treat_as_novice,
        },
        claims::REENTRY_RAMP_001,
    )
}

// ---------------------------------------------------------------------------
// 5. Progression & nutrition (File 08 load-031/indiv-008/013/020; DBLPROG/MASTERS/DEFICIT)
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
    evidence::graded(
        LoadIncrement {
            upper_kg: 2.5,
            lower_kg: 5.0,
        },
        claims::DBLPROG_001,
    )
}

/// Double progression: once the top of the rep range is hit on every set, add
/// load and drop to the range bottom (File 08 load-031). Returns `true` when the
/// engine should add load this session. DBLPROG-001.
pub fn double_progression_add_load(top_of_range_all_sets: bool) -> Recommended<bool> {
    evidence::graded(top_of_range_all_sets, claims::DBLPROG_001)
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
    evidence::graded(
        ProteinTarget {
            g_per_kg: (1.2, 1.6),
        },
        claims::MASTERS_001,
    )
}

/// Lean-mass-preserving deficit protein target: 1.8-2.7 g/kg/day, hold intensity
/// and cut volume toward MEV (File 08 indiv-020; DEFICIT-001).
///
/// SAFETY GUARD (File 08 safety-022, safety-critical): a caloric deficit
/// requested alongside any RED-S/disordered-eating signal is refused here, not
/// only in the global autoreg deferral, so no call path can obtain a deficit
/// target past a RED-S flag. With `reds_signal_present` the value is `None`,
/// cited to the RED-S deferral claim (SAFE-REDS-001, Strong, safety-critical).
pub fn deficit_protein_target(reds_signal_present: bool) -> Recommended<Option<ProteinTarget>> {
    if reds_signal_present {
        evidence::graded(None, claims::SAFE_REDS_001)
    } else {
        evidence::graded(
            Some(ProteinTarget {
                g_per_kg: (1.8, 2.7),
            }),
            claims::DEFICIT_001,
        )
    }
}

/// Masters (65+) per-meal protein dose in g/kg bodyweight to overcome anabolic
/// resistance (File 08 indiv-013): ~0.4 g/kg per meal (vs the lower per-meal
/// dose sufficient in younger adults). MASTERS-001.
pub fn masters_protein_per_meal() -> Recommended<f64> {
    evidence::graded(0.4, claims::MASTERS_001)
}

/// The WHO 5-17 weekly activity reference (File 08 youth-pa-001): an average
/// of 60 min/day of moderate-to-vigorous activity across the week, and
/// vigorous plus muscle- and bone-strengthening activity on at least 3 days.
/// Information and tracking only, never a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YouthActivityTargets {
    pub daily_minutes: u32,
    /// `daily_minutes` × 7: the daily average over one Mon–Sun week.
    pub weekly_minutes: u32,
    /// Days a week with vigorous activity, and (counted separately) days with
    /// muscle- and bone-strengthening activity.
    pub days_per_week: u8,
}

/// The WHO 5-17 activity reference (YOUTH-PA-001, Strong) that the under-18
/// activity card tracks logged activity against.
pub fn youth_activity_targets() -> Recommended<YouthActivityTargets> {
    const DAILY_MINUTES: u32 = 60;
    evidence::graded(
        YouthActivityTargets {
            daily_minutes: DAILY_MINUTES,
            weekly_minutes: DAILY_MINUTES * 7,
            days_per_week: 3,
        },
        claims::YOUTH_PA_001,
    )
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
    evidence::graded(outcome, claims::DBLPROG_001)
}

// ---------------------------------------------------------------------------
// 6. Time budget, substitution & environment (File 08 indiv-023/026, safety-024)
// ---------------------------------------------------------------------------

/// Weekly frequency a muscle can be *maintained* on under time pressure
/// (File 08 indiv-026: ~1x/wk holds a muscle; protect frequency + intensity,
/// cut accessory volume first). TIMECAP-001.
pub fn maintenance_frequency_per_week() -> Recommended<u8> {
    evidence::graded(1, claims::TIMECAP_001)
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
    evidence::graded(
        SubstitutionRule {
            match_movement_pattern: true,
            compensate_with_reps_near_failure: true,
        },
        claims::SUBST_001,
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
    evidence::graded(m, claims::ENV_001)
}

// ---------------------------------------------------------------------------
// 7. Stage-0 onboarding gates (File 08 onboard-050; safety-011/044/045/046/048)
// ---------------------------------------------------------------------------

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
    evidence::graded(
        PregnancyPrecautions {
            avoid_prolonged_supine: true,
            avoid_overheating: true,
            avoid_contact_fall_risk: true,
            avoid_scuba: true,
            avoid_altitude_above_m: ALTITUDE_THRESHOLD_M,
            avoid_valsalva: true,
        },
        claims::SAFE_PREG_AVOID_001,
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
        out.push(evidence::graded(Adjustment::Defer {
            reason: "You reported a pregnancy warning sign. Stop exercising now and contact your provider."
                .into(),
        },
        claims::SAFE_PREG_WARN_001,));
    }
    if s.symptomatic_any() {
        // onboard-051/052 (SCREEN-ACSM-001): chest pain or dizziness/fainting
        // → STOP + DEFER regardless of activity status. Deliberately NOT
        // cleared by `medically_cleared`: a prior clearance answer does not
        // cover a current symptom.
        out.push(evidence::graded(Adjustment::Defer {
            reason: "You reported chest pain, dizziness or fainting. See a doctor before you train, then update your health answers."
                .into(),
        },
        claims::SCREEN_ACSM_001,));
    }
    if s.acute_illness {
        // File 08 acute illness → DEFER; fever / infection = no training
        // (ILLNESS-NECK-001) until the user reports it resolved.
        out.push(evidence::graded(
            Adjustment::Defer {
                reason: "Better means no fever and your symptoms are gone. Then tap I'm better."
                    .into(),
            },
            claims::ILLNESS_NECK_001,
        ));
    }
    if s.parq_followup_uncleared() {
        // safety-044 + onboard-050: positive PAR-Q+/ACSM screen → medical
        // clearance gate before any prescription. Every follow-up Yes must be
        // covered by the clearance (a Yes added later gates again).
        out.push(evidence::graded(Adjustment::Defer {
            reason: "Once a doctor has cleared you, turn on \"Cleared by a doctor\" in your health answers."
                .into(),
        },
        claims::SAFE_CVD_001,));
    }
    if s.pregnant {
        // safety-045: no autonomous prescription/progression in pregnancy;
        // provider clearance + individualization. The ~150 min a week moderate
        // figure is the KB's reference target for uncomplicated pregnancy -
        // surfaced as context, NOT an engine prescription. The hold banner
        // already says lollop won't plan; the card carries the reference.
        out.push(evidence::graded(Adjustment::Defer {
            reason: "In an uncomplicated pregnancy, a common guide is about 150 minutes of moderate activity a week, agreed with your provider."
                .into(),
        },
        claims::SAFE_PREG_001,));
    }
    if s.injury_any() {
        // safety-048: the engine never prescribes rehabilitation.
        out.push(evidence::graded(Adjustment::Defer {
            reason: "You have an injury, recent surgery or ongoing rehab. lollop doesn't plan rehab. Follow your doctor or physio, and update your health answers once they clear you."
                .into(),
        },
        claims::SAFE_INJURY_001,));
    }
    if s.youth {
        // safety-011: pediatric/adolescent, supervision + technique-first;
        // never autonomous maximal loading or 1RM testing. The engine builds
        // no plan at all for this gate (the hold banner says so); the card
        // carries the KB's how-to.
        out.push(evidence::graded(
            Adjustment::Defer {
                reason:
                    "Put technique ahead of weight, and skip near-max weights and 1-rep-max tests."
                        .into(),
            },
            claims::SAFE_PEDS_001,
        ));
    }
    if s.reds_signal {
        // safety-049 absolute rule (via onboard-050 screening): never a
        // programming variable: reduce/rest and defer.
        out.push(evidence::graded(Adjustment::Defer {
            reason: "You reported a sign of under-eating. Ease off training and talk to a doctor, a registered dietitian or a mental-health professional."
                .into(),
        },
        claims::SAFE_REDS_001,));
    }
    out
}

#[cfg(test)]
mod tests;
