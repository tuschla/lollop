//! Feedback & coaching-communication decision core (knowledge-base File 05).
//!
//! Pure, deterministic message-category selection. Given observed session
//! signals it decides *which* feedback category to emit; rendering the actual
//! copy (slot-filling the templates in File 05 §6.2) is a shell concern.
//!
//! The overriding rule is feedback-042: **safety gates run first**. Any
//! CONCERN_INJURY / CONCERN_BEHAVIOR / CONCERN_RECOVERY (and the dangerous
//! single-session progression warning) short-circuits execution evaluation and
//! suppresses every competing praise/progression message in the same cycle
//! (File 05 §6.5). Contested metrics (ACWR, the 10% rule) may never become a
//! hard injury claim, the registry hard-blocks them.
//!
//! Claim ids: FEEDBACK-001, FB-PROCESS-GOAL-001, AUTOREG-RIR-001, RUN-DECOUPLE-001,
//! RUN-SPIKE-001, SAFE-BSI-001, SAFE-OTS-001, SAFE-REDS-001, LOAD-ACWR-001,
//! MYTH-POSITIVITY.

use crate::evidence;
use crate::evidence::claims;
use crate::schema::Recommended;

// ---------------------------------------------------------------------------
// Category taxonomy (File 05 §6.1)
// ---------------------------------------------------------------------------

/// The message category the engine emits for a session (File 05 decision tree).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
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
/// the shell can pick trigger-specific copy (KB File 05 §6.2) rather than one
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

// ---------------------------------------------------------------------------
// 1. Safety gate: runs first (feedback-042)
// ---------------------------------------------------------------------------

/// Observed safety signals, checked before any execution evaluation.
#[derive(Debug, Clone, Copy, Default)]
pub struct SafetySignals {
    /// Localized bone pain worsening through effort, night/rest pain, high-risk
    /// site, or mechanical joint symptoms (feedback-032/033/034).
    pub bone_pain_red_flag: bool,
    /// Compulsive pattern: training through pain, distress at missed sessions,
    /// rapidly escalating volume (feedback-039).
    pub compulsive_flag: bool,
    /// A positive answer on the recurring RED-S screen (REDS-SCREEN-001):
    /// missed/irregular periods, or low libido / fewer morning erections.
    pub reds_signal_reported: bool,
    /// Count of co-occurring NFOR signals over >=1-2 wks (feedback-036; >=2 fires).
    pub overtraining_signal_count: u8,
    /// How long the NFOR signals have co-occurred, in weeks (feedback-036
    /// duration condition: ">=2 signals over >=1-2 wks; do not fire on a
    /// single noisy night"). `None` = duration untracked by the shell, the
    /// gate then errs protective and still fires on count alone; `Some(w)`
    /// requires w >= 1.
    pub overtraining_weeks: Option<f64>,
    /// Single-session distance over the prior-30-day longest, as a fraction
    /// (feedback-037; >0.10 warns, never praises).
    pub single_session_spike_frac: Option<f64>,
}

/// The feedback-036 overtraining condition: ≥2 co-occurring signs (worse
/// performance, low mood, higher resting HR / lower HRV, worse sleep) that have
/// lasted at least a week. `weeks == None` = the duration was not reported, and
/// the gate errs protective and fires on the count alone. The ONE predicate the
/// recovery concern and the recovery-week deload share, so the card and the
/// lighter plan always appear together.
pub fn overtraining_sustained(signal_count: u8, weeks: Option<f64>) -> bool {
    signal_count >= 2 && weeks.is_none_or(|w| w >= 1.0)
}

/// Run the safety gates in priority order (Pain > Energy > Behavior >
/// Recovery > dangerous progression), returning the first concern that fires.
/// Only one card shows, so the Strong RED-S referral ranks above the
/// compulsive-exercise card: a user with both still gets the dietitian /
/// clinician referral. `None` when the session is safety-clear and execution
/// evaluation may proceed.
pub fn safety_gate(s: SafetySignals) -> Option<Recommended<FeedbackCategory>> {
    if s.bone_pain_red_flag {
        return Some(evidence::graded(
            FeedbackCategory::ConcernInjury,
            claims::SAFE_BSI_001,
        ));
    }
    if s.reds_signal_reported {
        // REDS-SCREEN-001 (Strong, safety-critical): any positive answer on
        // the recurring low-energy-availability screen goes to the referral.
        return Some(evidence::graded(
            FeedbackCategory::ConcernEnergy,
            claims::REDS_SCREEN_001,
        ));
    }
    if s.compulsive_flag {
        // feedback-039: ExpertOpinion, safety-critical (suppresses praise).
        return Some(evidence::graded(
            FeedbackCategory::ConcernBehavior,
            claims::FB_BEHAVIOR_001,
        ));
    }
    if overtraining_sustained(s.overtraining_signal_count, s.overtraining_weeks) {
        // feedback-036: Moderate (Meeusen 2013), safety-critical. The >=1-2 wk
        // duration condition keeps a single noisy night from firing; an
        // untracked duration still fires on the signal count (protective).
        return Some(evidence::graded(
            FeedbackCategory::ConcernRecovery,
            claims::FB_RECOVERY_001,
        ));
    }
    if s.single_session_spike_frac
        .is_some_and(|f| f > crate::running::SPIKE_FRAC)
    {
        // running-029 hard rule: block/flag, never praise (feedback-037).
        return Some(evidence::graded(
            FeedbackCategory::DangerousProgression,
            claims::RUN_SPIKE_BLOCK_001,
        ));
    }
    None
}

// ---------------------------------------------------------------------------
// 2. Execution evaluation: lifting (feedback-019/020/021/022)
// ---------------------------------------------------------------------------

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
    (evidence::graded(cat, claims::FB_RIR_001), trigger)
}

/// Running interval/threshold mastery (feedback-015; FB-INTERVAL-MASTERY-001,
/// Moderate): when every rep hit its target pace at or below the target RPE,
/// emit POSITIVE_MASTERY confirming the intended adaptation ("You hit every
/// rep at target and it cost RPE {rpe}. Exactly the adaptation we want.").
/// `None` when either condition misses: other arms then evaluate the session.
pub fn interval_mastery_feedback(
    target_paces_met: bool,
    rpe_at_or_below_target: bool,
) -> Option<Recommended<FeedbackCategory>> {
    (target_paces_met && rpe_at_or_below_target).then(|| {
        evidence::graded(
            FeedbackCategory::PositiveMastery,
            claims::FB_INTERVAL_MASTERY_001,
        )
    })
}

/// A genuine off day: target pace missed but RPE was very high (feedback-018/025).
/// Attributes to normal variation, never guilt; the stimulus still counts.
/// FB-BADDAY-001 (ExpertOpinion).
pub fn bad_day_feedback() -> Recommended<FeedbackCategory> {
    evidence::graded(FeedbackCategory::ContextualBadDay, claims::FB_BADDAY_001)
}

// ---------------------------------------------------------------------------
// 3. Execution evaluation: running decoupling (feedback-012/013/014/043)
// ---------------------------------------------------------------------------

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
    Some((evidence::graded(cat, claims::RUN_DECOUPLE_001), trigger))
}

// ---------------------------------------------------------------------------
// 3b. Running intensity discipline (feedback-011/016)
// ---------------------------------------------------------------------------

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
        Some(evidence::graded(
            FeedbackCategory::IntensityDiscipline,
            claims::RUN_DIST_001,
        ))
    } else {
        None
    }
}

/// Percent a run's back half may slow before it counts as a positive split worth
/// flagging. Shared so the coaching cue here and the descriptive run-summary note
/// (`app::to_run_view`) draw the same line: a run exactly at the threshold must
/// not show the note without the cue, or vice versa.
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
        evidence::graded(FeedbackCategory::IntensityDiscipline, claims::FB_PACING_001)
    } else {
        evidence::graded(FeedbackCategory::PositiveExecution, claims::FB_PACING_001)
    }
}

// ---------------------------------------------------------------------------
// 4. Top-level resolution: safety short-circuits execution (feedback-042)
// ---------------------------------------------------------------------------

/// Resolve the cycle's single feedback category: run the safety gate first and,
/// if any concern fires, emit it (suppressing the supplied execution praise).
/// Otherwise emit the execution category, defaulting to an informational,
/// competence-affirming note when execution produced nothing (feedback-042/009).
pub fn resolve_feedback(
    safety: SafetySignals,
    execution: Option<Recommended<FeedbackCategory>>,
) -> Recommended<FeedbackCategory> {
    if let Some(concern) = safety_gate(safety) {
        return concern;
    }
    execution.unwrap_or_else(|| {
        evidence::graded(FeedbackCategory::InformationalNeutral, claims::FEEDBACK_001)
    })
}

// ---------------------------------------------------------------------------
// 4b. Referral appendix & personalization (feedback-035/023/024/005)
// ---------------------------------------------------------------------------

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
    evidence::graded(female_user && bsi_referral_fired, claims::FB_BSI_FEMALE_001)
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
    /// Lead with the single biggest lever (advanced, but true for both, since
    /// beginners are capped at one takeaway anyway).
    pub lead_with_single_lever: bool,
}

/// Verbosity/metric-density defaults by experience (feedback-023/024).
pub fn verbosity_for_experience(advanced: bool) -> Recommended<FeedbackVerbosity> {
    let v = if advanced {
        FeedbackVerbosity {
            max_takeaways: 1,
            max_metrics: 3,
            rationale_mandatory: false,
            minimize_jargon: false,
            lead_with_single_lever: true,
        }
    } else {
        FeedbackVerbosity {
            max_takeaways: 1,
            max_metrics: 1,
            rationale_mandatory: true,
            minimize_jargon: true,
            lead_with_single_lever: true,
        }
    };
    evidence::graded(v, claims::FB_VERBOSITY_001)
}

/// Whether praise copy must anchor to a concrete mastery experience, a named
/// PR, completed session, or barrier overcome (feedback-005;
/// FB-MASTERY-ANCHOR-001, Strong, Bandura 1997). The rule triggers on the
/// mastery and consistency categories; both resolve to `PositiveMastery` here
/// (a completed planned week is a mastery experience, rendered with its
/// count). `PositiveExecution` (durability/pacing affirmed) is a descriptive
/// observation, and the concern/corrective categories suppress praise outright
/// (feedback-042).
pub fn mastery_anchor_required(category: FeedbackCategory) -> Recommended<bool> {
    let anchored = matches!(category, FeedbackCategory::PositiveMastery);
    evidence::graded(anchored, claims::FB_MASTERY_ANCHOR_001)
}

// ---------------------------------------------------------------------------
// 5. Voice constraints & contested-metric guards
// ---------------------------------------------------------------------------

/// Default framing for any next-step / goal-setting copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalFraming {
    /// Controllable process goal (cadence, pacing discipline, RIR target), default.
    Process,
    /// Outcome/result goal, only per individual goal-efficacy signal.
    Outcome,
}

/// Default all goal framing to controllable process goals (feedback-003;
/// FB-PROCESS-GOAL-001: Moderate and contested, since process vs specific
/// outcome goals is unresolved in the sport literature).
pub fn default_goal_framing() -> Recommended<GoalFraming> {
    evidence::graded(GoalFraming::Process, claims::FB_PROCESS_GOAL_001)
}

// ---------------------------------------------------------------------------
// 6. Longitudinal trend summary (feedback-027/028/029)
// ---------------------------------------------------------------------------

/// Which way a measured change went: a rolling multi-week metric versus
/// day-to-day noise, or one set's e1RM versus the previous (wire: lowercase).
/// Any unknown wire value reads as `Flat`, the no-trend default.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
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
    evidence::graded(summary, claims::FB_TREND_001)
}

// ---------------------------------------------------------------------------
// 7. Tone-by-context & pre-baseline provisional framing (feedback-026/040)
// ---------------------------------------------------------------------------

/// Days of logged data after which a stable per-user baseline exists and the
/// provisional framing ends. FB-PROVISIONAL-001 (ExpertOpinion, feedback-040).
pub const PROVISIONAL_BASELINE_DAYS: u16 = 14;

/// Whether a recommendation must be framed as a provisional population default
/// still converging on the user (feedback-040): true until a stable per-user
/// baseline exists ([`PROVISIONAL_BASELINE_DAYS`] days of data). Callers should
/// lower the surfaced `ConfidenceTag` and show "using population default until
/// N days" copy. FB-PROVISIONAL-001 (ExpertOpinion, feedback-040).
pub fn provisional_until_baseline(days_of_data: u16) -> Recommended<bool> {
    evidence::graded(
        days_of_data < PROVISIONAL_BASELINE_DAYS,
        claims::FB_PROVISIONAL_001,
    )
}

#[cfg(test)]
mod tests;
