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
//! Claim ids: FEEDBACK-001, GOAL-PROCESS-001, AUTOREG-RIR-001, RUN-DECOUPLE-001,
//! RUN-SPIKE-001, SAFE-BSI-001, SAFE-OTS-001, SAFE-REDS-001, LOAD-ACWR-001,
//! MYTH-POSITIVITY.

use crate::evidence;
use crate::schema::Recommended;

/// Build a `Recommended<T>` from a registry claim id (must exist).
fn recommend<T>(value: T, claim_id: &str) -> Recommended<T> {
    let e = evidence::claim(claim_id).expect("known feedback claim");
    Recommended {
        value,
        evidence: e.to_evidence(),
        confidence: e.to_confidence_tag(),
    }
}

// ---------------------------------------------------------------------------
// Category taxonomy (File 05 §6.1)
// ---------------------------------------------------------------------------

/// The message category the engine emits for a session (File 05 decision tree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackCategory {
    /// Bone-stress red flag: stop + refer (feedback-032/033/034). Suppresses praise.
    ConcernInjury,
    /// >=2 NFOR/overtraining signals over >=1-2 wks (feedback-036). Suppresses praise.
    ConcernRecovery,
    /// Compulsive / unhealthy pattern (feedback-039). Suppresses praise.
    ConcernBehavior,
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
                | FeedbackCategory::DangerousProgression
        )
    }
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
    /// Count of co-occurring NFOR signals over >=1-2 wks (feedback-036; >=2 fires).
    pub overtraining_signal_count: u8,
    /// Single-session distance over the prior-30-day longest, as a fraction
    /// (feedback-037; >0.10 warns, never praises).
    pub single_session_spike_frac: Option<f64>,
}

/// Run the safety gates in priority order (Pain > Behavior > Recovery >
/// dangerous progression), returning the first concern that fires. `None` when
/// the session is safety-clear and execution evaluation may proceed.
pub fn safety_gate(s: SafetySignals) -> Option<Recommended<FeedbackCategory>> {
    if s.bone_pain_red_flag {
        return Some(recommend(FeedbackCategory::ConcernInjury, "SAFE-BSI-001"));
    }
    if s.compulsive_flag {
        return Some(recommend(
            FeedbackCategory::ConcernBehavior,
            "SAFE-REDS-001",
        ));
    }
    if s.overtraining_signal_count >= 2 {
        return Some(recommend(FeedbackCategory::ConcernRecovery, "SAFE-OTS-001"));
    }
    if s.single_session_spike_frac.is_some_and(|f| f > 0.10) {
        return Some(recommend(
            FeedbackCategory::DangerousProgression,
            "RUN-SPIKE-001",
        ));
    }
    None
}

// ---------------------------------------------------------------------------
// 2. Execution evaluation: lifting (feedback-019/020/021/022)
// ---------------------------------------------------------------------------

/// Categorize a completed lifting set from reps-met + RIR vs target
/// (feedback-019/020/021/022; AUTOREG-RIR-001). Trust RIR most within 1-3 reps
/// of failure and in experienced lifters.
pub fn lifting_feedback(
    reps_met: bool,
    rir_actual: u8,
    rir_target: u8,
) -> Recommended<FeedbackCategory> {
    let cat = if !reps_met {
        // Missed reps: data, not failure, propose load adjustment.
        FeedbackCategory::CorrectiveProcess
    } else if rir_actual == 0 && rir_target >= 2 {
        // Reps met but cost far more than planned: hold/drop load.
        FeedbackCategory::CorrectiveProcess
    } else if rir_actual >= 4 && rir_target <= 2 {
        // Well under target intensity: room to add load.
        FeedbackCategory::ProgressionNudge
    } else {
        // Reps met at/near target cost, mastery, cue planned progression.
        FeedbackCategory::PositiveMastery
    };
    recommend(cat, "AUTOREG-RIR-001")
}

/// A genuine off day: target pace missed but RPE was very high (feedback-018/025).
/// Attributes to normal variation, never guilt; the stimulus still counts.
pub fn bad_day_feedback() -> Recommended<FeedbackCategory> {
    recommend(FeedbackCategory::ContextualBadDay, "FEEDBACK-001")
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
) -> Option<Recommended<FeedbackCategory>> {
    if !cool_steady_context {
        return None;
    }
    let cat = if drift_pct < 5.0 {
        FeedbackCategory::PositiveExecution
    } else if drift_pct <= 10.0 {
        FeedbackCategory::InformationalNeutral
    } else {
        FeedbackCategory::CorrectiveProcess
    };
    Some(recommend(cat, "RUN-DECOUPLE-001"))
}

// ---------------------------------------------------------------------------
// 3b. Running intensity discipline (feedback-011/016)
// ---------------------------------------------------------------------------

/// Easy-run intensity discipline (feedback-011): on an easy run where mean HR sat
/// above the Zone-2/VT1 ceiling for more than ~25% of the duration, emit
/// INTENSITY_DISCIPLINE, easy days build the aerobic base. `None` otherwise.
/// RUN-DIST-001.
pub fn easy_run_intensity_discipline(
    frac_time_above_vt1: f64,
) -> Option<Recommended<FeedbackCategory>> {
    if frac_time_above_vt1 > 0.25 {
        Some(recommend(
            FeedbackCategory::IntensityDiscipline,
            "RUN-DIST-001",
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

/// Pacing discipline (feedback-016): a positive split beyond ~3% on an
/// even-effort run emits INTENSITY_DISCIPLINE, advising an easier start toward an
/// even-to-negative split. `None` when pacing was even/negative. FEEDBACK-001.
pub fn positive_split_discipline(
    second_half_slower_pct: f64,
) -> Option<Recommended<FeedbackCategory>> {
    if second_half_slower_pct > POSITIVE_SPLIT_FLAG_PCT {
        Some(recommend(
            FeedbackCategory::IntensityDiscipline,
            "FEEDBACK-001",
        ))
    } else {
        None
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
    execution.unwrap_or_else(|| recommend(FeedbackCategory::InformationalNeutral, "FEEDBACK-001"))
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
/// GOAL-PROCESS-001).
pub fn default_goal_framing() -> Recommended<GoalFraming> {
    recommend(GoalFraming::Process, "GOAL-PROCESS-001")
}

/// Whether a fixed positive:corrective ratio is enforced. Always false, the
/// 2.9:1 "positivity ratio" is a retracted myth (feedback-007; MYTH-POSITIVITY
/// hard-blocked). Bias positive but never hardcode a number.
pub fn positivity_ratio_enforced() -> bool {
    !evidence::claim("MYTH-POSITIVITY")
        .expect("MYTH-POSITIVITY present")
        .is_blocked()
}

/// Whether ACWR may generate a hard injury-prediction claim. Always false -
/// LOAD-ACWR-001 is a hard-blocked myth; ACWR informs soft load-trend framing
/// only (feedback-030; §6.5).
pub fn acwr_injury_claim_allowed() -> bool {
    !evidence::claim("LOAD-ACWR-001")
        .expect("LOAD-ACWR-001 present")
        .is_blocked()
}

// ---------------------------------------------------------------------------
// 6. Longitudinal trend summary (feedback-027/028/029)
// ---------------------------------------------------------------------------

/// Direction of a rolling multi-week metric versus day-to-day noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendDirection {
    Up,
    Flat,
    Down,
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

/// Resolve the longitudinal trend message (feedback-027/028/029). Load-explained
/// decline (performance down + load spike or low recovery) takes precedence and
/// routes to a recovery-first deload nudge; otherwise an improving rolling trend
/// celebrates consistency, and a ≥4-week flat stretch is reframed as a plateau.
/// FEEDBACK-001.
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
    } else if direction == TrendDirection::Flat && weeks_flat >= 4 {
        TrendSummary::Plateau
    } else {
        TrendSummary::Stable
    };
    recommend(summary, "FEEDBACK-001")
}

// ---------------------------------------------------------------------------
// 7. Tone-by-context & pre-baseline provisional framing (feedback-026/040)
// ---------------------------------------------------------------------------

/// Tone modifier applied to an emitted category by the session's planned intent
/// (feedback-026).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneModifier {
    /// Planned-easy: celebrate restraint; not chasing pace is the win.
    CelebrateRestraint,
    /// Planned-hard: praise completion/effort even if paces were imperfect.
    PraiseEffort,
}

/// Tone modifier by planned session intensity (feedback-026). FEEDBACK-001.
pub fn planned_intensity_tone(planned_hard: bool) -> Recommended<ToneModifier> {
    let tone = if planned_hard {
        ToneModifier::PraiseEffort
    } else {
        ToneModifier::CelebrateRestraint
    };
    recommend(tone, "FEEDBACK-001")
}

/// Whether a recommendation must be framed as a provisional population default
/// still converging on the user (feedback-040): true until a stable per-user
/// baseline exists (~14 days of data). Callers should lower the surfaced
/// `ConfidenceTag` and show "using population default until N days" copy.
/// FEEDBACK-001.
pub fn provisional_until_baseline(days_of_data: u16) -> Recommended<bool> {
    recommend(days_of_data < 14, "FEEDBACK-001")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_intensity_discipline_thresholds() {
        assert!(easy_run_intensity_discipline(0.25).is_none());
        assert_eq!(
            easy_run_intensity_discipline(0.26).unwrap().value,
            FeedbackCategory::IntensityDiscipline
        );
        assert!(positive_split_discipline(3.0).is_none());
        assert_eq!(
            positive_split_discipline(3.1).unwrap().value,
            FeedbackCategory::IntensityDiscipline
        );
    }

    #[test]
    fn trend_summary_precedence_and_bands() {
        // Load-explained decline outranks a nominally-up direction.
        assert_eq!(
            trend_summary(TrendDirection::Up, 0, true, true, false).value,
            TrendSummary::LoadExplainedDecline
        );
        assert_eq!(
            trend_summary(TrendDirection::Down, 0, true, false, true).value,
            TrendSummary::LoadExplainedDecline
        );
        // Improving.
        assert_eq!(
            trend_summary(TrendDirection::Up, 0, false, false, false).value,
            TrendSummary::Improving
        );
        // Plateau needs >=4 flat weeks.
        assert_eq!(
            trend_summary(TrendDirection::Flat, 4, false, false, false).value,
            TrendSummary::Plateau
        );
        assert_eq!(
            trend_summary(TrendDirection::Flat, 3, false, false, false).value,
            TrendSummary::Stable
        );
        // Performance down but no load explanation -> not a recovery-first message.
        assert_eq!(
            trend_summary(TrendDirection::Down, 0, true, false, false).value,
            TrendSummary::Stable
        );
    }

    #[test]
    fn tone_and_provisional_framing() {
        assert_eq!(
            planned_intensity_tone(false).value,
            ToneModifier::CelebrateRestraint
        );
        assert_eq!(
            planned_intensity_tone(true).value,
            ToneModifier::PraiseEffort
        );
        assert!(provisional_until_baseline(0).value);
        assert!(provisional_until_baseline(13).value);
        assert!(!provisional_until_baseline(14).value);
    }

    #[test]
    fn safety_gate_priority_order() {
        // Pain outranks everything.
        let all = SafetySignals {
            bone_pain_red_flag: true,
            compulsive_flag: true,
            overtraining_signal_count: 5,
            single_session_spike_frac: Some(0.9),
        };
        assert_eq!(
            safety_gate(all).unwrap().value,
            FeedbackCategory::ConcernInjury
        );

        // Behavior outranks recovery + progression.
        let behav = SafetySignals {
            bone_pain_red_flag: false,
            compulsive_flag: true,
            overtraining_signal_count: 5,
            single_session_spike_frac: Some(0.9),
        };
        assert_eq!(
            safety_gate(behav).unwrap().value,
            FeedbackCategory::ConcernBehavior
        );

        // Recovery outranks progression.
        let rec = SafetySignals {
            overtraining_signal_count: 2,
            single_session_spike_frac: Some(0.9),
            ..Default::default()
        };
        assert_eq!(
            safety_gate(rec).unwrap().value,
            FeedbackCategory::ConcernRecovery
        );

        // Spike alone.
        let spike = SafetySignals {
            single_session_spike_frac: Some(0.11),
            ..Default::default()
        };
        assert_eq!(
            safety_gate(spike).unwrap().value,
            FeedbackCategory::DangerousProgression
        );

        // Clear session.
        assert!(safety_gate(SafetySignals::default()).is_none());
        // A single NFOR signal does not fire (needs >=2).
        assert!(
            safety_gate(SafetySignals {
                overtraining_signal_count: 1,
                ..Default::default()
            })
            .is_none()
        );
        // Spike at/below 10% does not fire.
        assert!(
            safety_gate(SafetySignals {
                single_session_spike_frac: Some(0.10),
                ..Default::default()
            })
            .is_none()
        );
    }

    #[test]
    fn concern_injury_is_safety_critical() {
        let c = recommend(FeedbackCategory::ConcernInjury, "SAFE-BSI-001");
        assert!(c.confidence.safety_critical);
    }

    #[test]
    fn suppression_set_matches_file05() {
        for c in [
            FeedbackCategory::ConcernInjury,
            FeedbackCategory::ConcernRecovery,
            FeedbackCategory::ConcernBehavior,
            FeedbackCategory::DangerousProgression,
        ] {
            assert!(
                c.suppresses_competing_praise(),
                "{c:?} must suppress praise"
            );
        }
        for c in [
            FeedbackCategory::PositiveMastery,
            FeedbackCategory::PositiveExecution,
            FeedbackCategory::ProgressionNudge,
            FeedbackCategory::CorrectiveProcess,
            FeedbackCategory::InformationalNeutral,
            FeedbackCategory::IntensityDiscipline,
            FeedbackCategory::ContextualBadDay,
        ] {
            assert!(
                !c.suppresses_competing_praise(),
                "{c:?} must not suppress praise"
            );
        }
    }

    #[test]
    fn resolve_suppresses_praise_under_concern() {
        let praise = Some(recommend(
            FeedbackCategory::PositiveMastery,
            "AUTOREG-RIR-001",
        ));
        let injury = SafetySignals {
            bone_pain_red_flag: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_feedback(injury, praise).value,
            FeedbackCategory::ConcernInjury
        );

        // Safety-clear: execution passes through.
        let praise = Some(recommend(
            FeedbackCategory::PositiveMastery,
            "AUTOREG-RIR-001",
        ));
        assert_eq!(
            resolve_feedback(SafetySignals::default(), praise).value,
            FeedbackCategory::PositiveMastery
        );

        // Safety-clear, no execution: informational default.
        assert_eq!(
            resolve_feedback(SafetySignals::default(), None).value,
            FeedbackCategory::InformationalNeutral
        );
    }

    #[test]
    fn lifting_branches() {
        // Missed reps.
        assert_eq!(
            lifting_feedback(false, 2, 2).value,
            FeedbackCategory::CorrectiveProcess
        );
        // Reps met, RIR 0 vs 2-3 target, caution.
        assert_eq!(
            lifting_feedback(true, 0, 3).value,
            FeedbackCategory::CorrectiveProcess
        );
        // Reps met, RIR 4 vs 1-2 target, nudge.
        assert_eq!(
            lifting_feedback(true, 4, 1).value,
            FeedbackCategory::ProgressionNudge
        );
        // Reps met at target cost, mastery.
        assert_eq!(
            lifting_feedback(true, 2, 2).value,
            FeedbackCategory::PositiveMastery
        );
    }

    #[test]
    fn decoupling_gated_and_banded() {
        // Confounded context: no message.
        assert!(decoupling_feedback(3.0, false).is_none());
        // <5% durability.
        assert_eq!(
            decoupling_feedback(3.0, true).unwrap().value,
            FeedbackCategory::PositiveExecution
        );
        // 5-10% neutral.
        assert_eq!(
            decoupling_feedback(7.0, true).unwrap().value,
            FeedbackCategory::InformationalNeutral
        );
        // >10% corrective.
        assert_eq!(
            decoupling_feedback(12.0, true).unwrap().value,
            FeedbackCategory::CorrectiveProcess
        );
    }

    #[test]
    fn contested_metrics_and_goal_default() {
        // Both myths hard-blocked -> guards return false.
        assert!(!positivity_ratio_enforced());
        assert!(!acwr_injury_claim_allowed());
        // Process framing is the default.
        assert_eq!(default_goal_framing().value, GoalFraming::Process);
    }
}
