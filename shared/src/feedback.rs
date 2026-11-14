//! Feedback & coaching-communication decision core (knowledge-base File 05).
//!
//! Message-category selection. Given observed session
//! signals it decides *which* feedback category to emit; the app renders the
//! copy for it.
//!
//! The overriding rule is feedback-042: safety gates run first. Any
//! CONCERN_INJURY / CONCERN_BEHAVIOR / CONCERN_RECOVERY (and the dangerous
//! single-session progression warning) short-circuits execution evaluation and
//! suppresses every competing praise/progression message in the same cycle
//! (File 05 §6.5).

use crate::evidence::claims;
use crate::schema::Recommended;

/// The message category the engine emits for a session (File 05 decision tree,
/// §6.1).
#[derive(
    serde::Serialize, serde::Deserialize, crate::wire::Wire, Debug, Clone, Copy, PartialEq, Eq,
)]
pub enum FeedbackCategory {
    /// Bone-stress red flag: stop + refer (feedback-032/033/034). Suppresses praise.
    ConcernInjury,
    /// >=2 NFOR/overtraining signals over >=1-2 wks (feedback-036). Suppresses praise.
    ConcernRecovery,
    /// Compulsive / unhealthy pattern (feedback-039). Suppresses praise.
    ConcernBehavior,
    /// Low-energy-availability warning sign reported on the recurring RED-S
    /// screen (missed/irregular periods; low libido / fewer morning erections):
    /// route to the clinician referral (REDS-SCREEN-001). Suppresses praise.
    ConcernEnergy,
    /// Single-session distance spike: warn, never praise (feedback-037). Suppresses praise.
    DangerousProgression,
    /// Easy day run too hard, or positive split on even effort (feedback-011/016).
    IntensityDiscipline,
    /// Durability / good pacing affirmed (feedback-012/017).
    PositiveExecution,
    /// Mild aerobic fatigue noted, non-corrective (feedback-013).
    InformationalNeutral,
    /// Autonomy-supportive correction with rationale + choice (feedback-014/020/021).
    CorrectiveProcess,
    /// Target hit at/below target cost, cue planned progression (feedback-015/019).
    PositiveMastery,
    /// Missed target on a genuine off day, no guilt (feedback-018/025).
    ContextualBadDay,
    /// Effort well under target, invite added load (feedback-022).
    ProgressionNudge,
}

impl FeedbackCategory {
    /// True for categories that must suppress every competing praise/progression
    /// message in the same cycle (File 05 §6.5; feedback-042).
    pub fn suppresses_competing_praise(self) -> bool {
        matches!(
            self,
            FeedbackCategory::ConcernInjury
                | FeedbackCategory::ConcernRecovery
                | FeedbackCategory::ConcernBehavior
                | FeedbackCategory::ConcernEnergy
                | FeedbackCategory::DangerousProgression
        )
    }
}

/// Which condition produced a [`FeedbackCategory::CorrectiveProcess`] verdict, so
/// the card copy can be trigger-specific (KB File 05 §6.2) rather than one
/// generic corrective line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectiveTrigger {
    /// Lifting reps not completed (feedback-021).
    MissedTarget,
    /// Lifting reps met but at RIR ~0 against a 2-3 target (feedback-020).
    RepsMetAtRir0,
    /// Aerobic decoupling >10% on an intended-aerobic run (feedback-014).
    DecouplingHigh,
}

/// The safety screens of a weekly check-in, the part of the review wire
/// payload the safety gate reads (`SessionReview` flattens it, so these are
/// top-level wire fields). A new safety screen is added here once.
#[derive(
    serde::Serialize,
    serde::Deserialize,
    crate::wire::Wire,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Default,
    crate::sanitize::Sanitize,
)]
pub struct SafetyScreen {
    /// Localized bone pain worsening through effort, night/rest pain, high-risk
    /// site, or mechanical joint symptoms (feedback-032/033/034).
    pub bone_pain_red_flag: bool,
    /// Compulsive pattern: training through pain, distress at missed sessions,
    /// rapidly escalating volume (feedback-039).
    pub compulsive_flag: bool,
    /// Count of co-occurring NFOR signals (feedback-036; >=2 fires); the legacy
    /// wire count, overridden by `overtraining_signs` when present.
    pub overtraining_signal_count: u8,
    /// How long the NFOR signals have co-occurred, in weeks (feedback-036
    /// duration condition: ">=2 signals over >=1-2 wks; do not fire on a
    /// single noisy night"). `None` = duration untracked by the shell, the
    /// gate then errs protective and still fires on count alone; `Some(w)`
    /// requires w >= 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overtraining_signal_weeks: Option<f64>,
    /// Which of the four listed overtraining signs were ticked. `Some` → the
    /// effective signal count is the ticked count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overtraining_signs: Option<OvertrainingSigns>,
    /// Weekly RED-S screen positive (File 13 ready-upd-005, REDS-SCREEN-001):
    /// missed or longer cycles over the last 3 months (not explained by
    /// pregnancy or contraception), or noticeably lower sex drive / fewer
    /// morning erections for several weeks. Routes to the RED-S referral
    /// feedback and the deficit refusal, never a programming variable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reds_signal_reported: bool,
}

impl SafetyScreen {
    /// The effective overtraining-sign count: the number of listed signs
    /// ticked when the check-in carries them, else the bare count (legacy
    /// wire).
    pub fn signal_count(&self) -> u8 {
        self.overtraining_signs
            .map_or(self.overtraining_signal_count, |s| s.count())
    }

    /// [`overtraining_sustained`] over this screen's signs and duration.
    pub fn overtraining_sustained(&self) -> bool {
        overtraining_sustained(self.signal_count(), self.overtraining_signal_weeks)
    }
}

/// The four overtraining signs the weekly check-in asks about (feedback-036:
/// performance decrement, mood disturbance, elevated resting HR / suppressed
/// HRV, sleep deterioration). When a check-in carries this, the signal count
/// IS the number of signs ticked (it overrides the bare
/// `overtraining_signal_count`), so "a sign" always means one of these four.
#[derive(
    serde::Serialize,
    serde::Deserialize,
    crate::wire::Wire,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    crate::sanitize::Sanitize,
)]
#[serde(default)]
pub struct OvertrainingSigns {
    pub worse_performance: bool,
    pub low_mood: bool,
    pub higher_resting_hr: bool,
    pub worse_sleep: bool,
}

impl OvertrainingSigns {
    /// How many of the four signs are ticked.
    pub fn count(&self) -> u8 {
        [
            self.worse_performance,
            self.low_mood,
            self.higher_resting_hr,
            self.worse_sleep,
        ]
        .iter()
        .filter(|b| **b)
        .count() as u8
    }
}

/// Observed safety signals, checked before any execution evaluation: the
/// weekly check-in's screens plus the session's distance spike.
#[derive(Debug, Clone, Copy, Default)]
pub struct SafetySignals {
    pub screen: SafetyScreen,
    /// Single-session distance over the prior-30-day longest, as a fraction
    /// (feedback-037; >0.10 warns, never praises).
    pub single_session_spike_frac: Option<f64>,
}

/// Co-occurring overtraining signs that raise the feedback-036 concern.
pub const OVERTRAINING_MIN_SIGNS: u8 = 2;
/// Weeks the signs must have lasted for the feedback-036 concern (the KB's
/// "over >=1-2 wks; do not fire on a single noisy night", lower end).
pub const OVERTRAINING_MIN_WEEKS: f64 = 1.0;

/// The feedback-036 overtraining condition: ≥ [`OVERTRAINING_MIN_SIGNS`]
/// co-occurring signs (worse performance, low mood, higher resting HR / lower
/// HRV, worse sleep) that have lasted at least [`OVERTRAINING_MIN_WEEKS`].
/// `weeks == None` = the duration was not reported, and the gate errs
/// protective and fires on the count alone. The ONE predicate the recovery
/// concern and the recovery-week deload share, so the card and the lighter
/// plan always appear together.
pub fn overtraining_sustained(signal_count: u8, weeks: Option<f64>) -> bool {
    signal_count >= OVERTRAINING_MIN_SIGNS && weeks.is_none_or(|w| w >= OVERTRAINING_MIN_WEEKS)
}

/// Run the safety gates in priority order (Pain > Energy > Behavior >
/// Recovery > dangerous progression), returning the first concern that fires.
/// Only one card shows, so the Strong RED-S referral ranks above the
/// compulsive-exercise card: a user with both still gets the dietitian /
/// clinician referral. `None` when the session is safety-clear and execution
/// evaluation may proceed.
pub fn safety_gate(s: SafetySignals) -> Option<Recommended<FeedbackCategory>> {
    if s.screen.bone_pain_red_flag {
        return Some(Recommended::new(
            FeedbackCategory::ConcernInjury,
            claims::SAFE_BSI_001,
        ));
    }
    if s.screen.reds_signal_reported {
        // REDS-SCREEN-001 (Strong, safety-critical): any positive answer on
        // the recurring low-energy-availability screen goes to the referral.
        return Some(Recommended::new(
            FeedbackCategory::ConcernEnergy,
            claims::REDS_SCREEN_001,
        ));
    }
    if s.screen.compulsive_flag {
        // feedback-039: ExpertOpinion, safety-critical (suppresses praise).
        return Some(Recommended::new(
            FeedbackCategory::ConcernBehavior,
            claims::FB_BEHAVIOR_001,
        ));
    }
    if s.screen.overtraining_sustained() {
        // feedback-036: Moderate (Meeusen 2013), safety-critical. The >=1-2 wk
        // duration condition keeps a single noisy night from firing; an
        // untracked duration still fires on the signal count (protective).
        return Some(Recommended::new(
            FeedbackCategory::ConcernRecovery,
            claims::FB_RECOVERY_001,
        ));
    }
    if s.single_session_spike_frac
        .is_some_and(|f| f > crate::running::SPIKE_FRAC)
    {
        // running-029 hard rule: block/flag, never praise (feedback-037).
        return Some(Recommended::new(
            FeedbackCategory::DangerousProgression,
            claims::RUN_SPIKE_BLOCK_001,
        ));
    }
    None
}

/// Categorize a completed lifting set from reps-met + RIR vs target
/// (feedback-019/020/021/022; FB-RIR-001). Trust RIR most within 1-3 reps
/// of failure and in experienced lifters.
///
/// feedback-019: PositiveMastery ONLY when reps are met AND `rir_actual >=
/// rir_target`, the set cost no more than planned. feedback-020: reps met at
/// RIR ~0 against a 2-3 target → CorrectiveProcess (hold or slightly drop
/// load). In between (below-target RIR but not at failure) → neutral process
/// tone, never praise-with-progression.
pub fn lifting_feedback(
    reps_met: bool,
    rir_actual: u8,
    rir_target: u8,
) -> (Recommended<FeedbackCategory>, Option<CorrectiveTrigger>) {
    let (cat, trigger) = if !reps_met {
        // Missed reps: data, not failure, propose load adjustment.
        (
            FeedbackCategory::CorrectiveProcess,
            Some(CorrectiveTrigger::MissedTarget),
        )
    } else if rir_actual == 0 && rir_target >= 2 {
        // Reps met but cost far more than planned: hold/drop load.
        (
            FeedbackCategory::CorrectiveProcess,
            Some(CorrectiveTrigger::RepsMetAtRir0),
        )
    } else if rir_actual >= 4 && rir_target <= 2 {
        // Well under target intensity: room to add load.
        (FeedbackCategory::ProgressionNudge, None)
    } else if rir_actual >= rir_target {
        // Reps met at/below planned cost, mastery, cue planned progression.
        (FeedbackCategory::PositiveMastery, None)
    } else {
        // Harder than planned but short of failure, neutral process note.
        (FeedbackCategory::InformationalNeutral, None)
    };
    (Recommended::new(cat, claims::FB_RIR_001), trigger)
}

/// Running interval/threshold mastery (feedback-015; FB-INTERVAL-MASTERY-001,
/// Moderate): when every rep hit its target pace at or below the target RPE,
/// emit POSITIVE_MASTERY confirming the intended adaptation.
/// `None` when either condition misses: other arms then evaluate the session.
pub fn interval_mastery_feedback(
    target_paces_met: bool,
    rpe_at_or_below_target: bool,
) -> Option<Recommended<FeedbackCategory>> {
    (target_paces_met && rpe_at_or_below_target).then(|| {
        Recommended::new(
            FeedbackCategory::PositiveMastery,
            claims::FB_INTERVAL_MASTERY_001,
        )
    })
}

/// A genuine off day: target pace missed but RPE was very high (feedback-018/025).
/// Attributes to normal variation, never guilt; the stimulus still counts.
/// FB-BADDAY-001 (ExpertOpinion).
pub fn bad_day_feedback() -> Recommended<FeedbackCategory> {
    Recommended::new(FeedbackCategory::ContextualBadDay, claims::FB_BADDAY_001)
}

/// Categorize aerobic decoupling (Pa:HR) on an intended-aerobic run
/// (feedback-012/013/014; RUN-DECOUPLE-001). Gated to cool, steady sub-threshold
/// efforts (feedback-043), `None` when the context is confounded (heat, surges,
/// bad data) so no decoupling message fires.
pub fn decoupling_feedback(
    drift_pct: f64,
    cool_steady_context: bool,
) -> Option<(Recommended<FeedbackCategory>, Option<CorrectiveTrigger>)> {
    if !cool_steady_context {
        return None;
    }
    // Same bands as `load::decoupling_band` (`DECOUPLING_SOUND_PCT`,
    // `decoupling_high`), so the feedback card and the base-fitness row never
    // disagree on the same drift.
    let (cat, trigger) = if drift_pct < crate::load::DECOUPLING_SOUND_PCT {
        (FeedbackCategory::PositiveExecution, None)
    } else if !crate::load::decoupling_high(drift_pct) {
        (FeedbackCategory::InformationalNeutral, None)
    } else {
        (
            FeedbackCategory::CorrectiveProcess,
            Some(CorrectiveTrigger::DecouplingHigh),
        )
    };
    Some((Recommended::new(cat, claims::RUN_DECOUPLE_001), trigger))
}

/// Share of an easy run's duration above the Zone-2/VT1 ceiling beyond which
/// intensity discipline is flagged (feedback-011: "more than ~25%"; RUN-DIST-001).
pub const EASY_RUN_ABOVE_VT1_MAX_FRAC: f64 = 0.25;

/// Easy-run intensity discipline (feedback-011): on an easy run where mean HR sat
/// above the Zone-2/VT1 ceiling for more than [`EASY_RUN_ABOVE_VT1_MAX_FRAC`] of
/// the duration, emit INTENSITY_DISCIPLINE, easy days build the aerobic base.
/// `None` otherwise. RUN-DIST-001.
pub fn easy_run_intensity_discipline(
    frac_time_above_vt1: f64,
) -> Option<Recommended<FeedbackCategory>> {
    if frac_time_above_vt1 > EASY_RUN_ABOVE_VT1_MAX_FRAC {
        Some(Recommended::new(
            FeedbackCategory::IntensityDiscipline,
            claims::RUN_DIST_001,
        ))
    } else {
        None
    }
}

/// Percent a run's back half may slow before it counts as a positive split worth
/// flagging. Shared so the coaching cue here and the descriptive run-summary
/// note draw the same line: a run exactly at the threshold must not show the
/// note without the cue, or vice versa.
pub const POSITIVE_SPLIT_FLAG_PCT: f64 = 3.0;

/// Pacing verdict for a measured split (feedback-016/017; FB-PACING-001,
/// Moderate, Hanley 2016; Smyth 2018; Abbiss & Laursen 2008).
///
/// A positive split beyond ~3% on an even-effort run emits
/// INTENSITY_DISCIPLINE, advising an easier start toward an even-to-negative
/// split (feedback-016). An even or negative split (at/below the threshold)
/// emits POSITIVE_EXECUTION, "textbook pacing discipline" (feedback-017).
pub fn positive_split_discipline(second_half_slower_pct: f64) -> Recommended<FeedbackCategory> {
    if second_half_slower_pct > POSITIVE_SPLIT_FLAG_PCT {
        Recommended::new(FeedbackCategory::IntensityDiscipline, claims::FB_PACING_001)
    } else {
        Recommended::new(FeedbackCategory::PositiveExecution, claims::FB_PACING_001)
    }
}

/// Resolve the cycle's single feedback category from the [`safety_gate`]
/// verdict: a concern wins (suppressing the supplied execution praise).
/// Otherwise emit the execution category, defaulting to an informational,
/// competence-affirming note when execution produced nothing (feedback-042/009).
pub fn resolve_feedback(
    concern: Option<Recommended<FeedbackCategory>>,
    execution: Option<Recommended<FeedbackCategory>>,
) -> Recommended<FeedbackCategory> {
    concern.or(execution).unwrap_or_else(|| {
        Recommended::new(FeedbackCategory::InformationalNeutral, claims::FEEDBACK_001)
    })
}

/// Clinician-prompt copy appended to a female user's bone-stress referral
/// (feedback-035). Gentle, no self-diagnosis, the referral itself stays the
/// message; this only adds the menstrual/nutrition discussion prompt.
pub const BSI_FEMALE_PROMPT: &str = "Also tell them about your periods and how much you eat. Under-eating and missed or irregular periods raise the risk of a bone stress injury.";

/// Whether to append the menstrual/nutrition clinician prompt (feedback-035;
/// FB-BSI-FEMALE-001, ExpertOpinion, safety-critical): fires only for a
/// female user who was actually routed to a bone-stress referral
/// (CONCERN_INJURY). Defers to the human professional, never a diagnosis.
pub fn bsi_menstrual_nutrition_prompt(
    female_user: bool,
    bsi_referral_fired: bool,
) -> Recommended<bool> {
    Recommended::new(female_user && bsi_referral_fired, claims::FB_BSI_FEMALE_001)
}

/// Message verbosity / metric-density personalization (feedback-023/024;
/// FB-VERBOSITY-001, ExpertOpinion). The KB defines only the beginner and
/// advanced profiles; everyone not advanced gets the beginner defaults
/// (conservative: low jargon and a mandatory "why" never hurt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedbackVerbosity {
    /// Cap on takeaways per message (beginner: 1, feedback-023; advanced
    /// messages also lead with a single biggest lever, feedback-024).
    pub max_takeaways: u8,
    /// Cap on metrics surfaced (advanced: 2–3, upper bound carried; the KB
    /// states no beginner metric count, held at the one-takeaway floor).
    pub max_metrics: u8,
    /// Always explain the "why" (mandatory for beginners).
    pub rationale_mandatory: bool,
    /// Minimize jargon (beginners).
    pub minimize_jargon: bool,
}

/// Verbosity/metric-density defaults by experience (feedback-023/024).
pub fn verbosity_for_experience(advanced: bool) -> Recommended<FeedbackVerbosity> {
    let v = if advanced {
        FeedbackVerbosity {
            max_takeaways: 1,
            max_metrics: 3,
            rationale_mandatory: false,
            minimize_jargon: false,
        }
    } else {
        FeedbackVerbosity {
            max_takeaways: 1,
            max_metrics: 1,
            rationale_mandatory: true,
            minimize_jargon: true,
        }
    };
    Recommended::new(v, claims::FB_VERBOSITY_001)
}

/// Which way a measured change went: a rolling multi-week metric versus
/// day-to-day noise, or one set's e1RM versus the previous (wire: lowercase).
/// Any unknown wire value reads as `Flat`, the no-trend default.
#[derive(
    serde::Serialize,
    serde::Deserialize,
    crate::wire::Wire,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    crate::sanitize::Sanitize,
)]
#[serde(rename_all = "lowercase")]
pub enum TrendDirection {
    Up,
    Down,
    #[serde(other)]
    Flat,
}

/// The message a weekly/monthly summary should carry (File 05 §6, trend arm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendSummary {
    /// Rolling metric up beyond noise: celebrate consistency, set next process
    /// goal (feedback-027).
    Improving,
    /// Flat ≥4 wks: reframe as normal consolidation, change ONE variable, protect
    /// self-efficacy (feedback-028).
    Plateau,
    /// Performance down with a load spike or insufficient recovery: recovery-first
    /// message, suggest a deload week (feedback-029).
    LoadExplainedDecline,
    /// Nothing decisive, no trend message this cycle.
    Stable,
}

/// Weeks of a flat rolling trend after which it is reframed as a plateau
/// (feedback-028; FB-TREND-001).
pub const PLATEAU_FLAT_WEEKS: u8 = 4;

/// Resolve the longitudinal trend message (feedback-027/028/029). Load-explained
/// decline (performance down + load spike or low recovery) takes precedence and
/// routes to a recovery-first deload nudge; otherwise an improving rolling trend
/// celebrates consistency, and a flat stretch of [`PLATEAU_FLAT_WEEKS`] or more
/// is reframed as a plateau. FB-TREND-001 (ExpertOpinion, feedback-027/028/029).
pub fn trend_summary(
    direction: TrendDirection,
    weeks_flat: u8,
    performance_down: bool,
    load_spike: bool,
    low_recovery: bool,
) -> Recommended<TrendSummary> {
    let summary = if performance_down && (load_spike || low_recovery) {
        TrendSummary::LoadExplainedDecline
    } else if direction == TrendDirection::Up {
        TrendSummary::Improving
    } else if direction == TrendDirection::Flat && weeks_flat >= PLATEAU_FLAT_WEEKS {
        TrendSummary::Plateau
    } else {
        TrendSummary::Stable
    };
    Recommended::new(summary, claims::FB_TREND_001)
}

/// Days of logged data after which a stable per-user baseline exists and the
/// provisional framing ends. FB-PROVISIONAL-001 (ExpertOpinion, feedback-040).
pub const PROVISIONAL_BASELINE_DAYS: u16 = 14;

/// Whether a recommendation must be framed as a provisional population default
/// still converging on the user (feedback-040): true until a stable per-user
/// baseline exists ([`PROVISIONAL_BASELINE_DAYS`] days of data). Callers should
/// lower the surfaced confidence and show "using population default until
/// N days" copy. FB-PROVISIONAL-001 (ExpertOpinion, feedback-040).
pub fn provisional_until_baseline(days_of_data: u16) -> Recommended<bool> {
    Recommended::new(
        days_of_data < PROVISIONAL_BASELINE_DAYS,
        claims::FB_PROVISIONAL_001,
    )
}

#[cfg(test)]
mod tests;
