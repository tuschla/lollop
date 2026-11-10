//! Readiness assembly helpers: performance-signal expiry, provisional/autoreg
//! source rows, the Today headline, baseline echoes, and the readiness signal
//! groups.

use crate::app::evidence_views::{to_view, to_view_with};
use crate::app::format::monday_of;
use crate::app::logged_days::{DeloadDays, LoggedDays, lift_deload_days, plan_week_index};
use crate::app::model::{LoggedSet, Model};
use crate::app::run_metrics::RunFacts;
use crate::app::view_types::{
    AdjustmentView, BaselineStatusView, EvidenceTagView, FeedbackView, HeadlineKind, HoldReason,
    HoldResolutionView, ReadinessSignalView, SessionPlanView, SessionStatus, SignalGroupView,
    TodayHeadlineView,
};
use crate::autoreg;
use crate::day::{is_undated, local_day};
use crate::feedback::{self, FeedbackCategory};
use crate::profile::Profile;
use crate::schema::{
    Adjustment, AdjustmentCall, AdjustmentKind, CheckinInput, HealthScreen, ReadinessInput,
    ReadinessSignal, Recommended, SafetyTier, ScreenGate,
};
use crate::strength::LiftGoal;

/// Human label for one still-collecting check-in channel. The shell phrases
/// the progress row itself from `kind`/`have`/`need`; no training claim
/// (HARD RULE 1/2).
pub(super) fn to_baseline_status_view(s: &autoreg::BaselineStatus) -> BaselineStatusView {
    let label = match s.signal {
        ReadinessSignal::WellnessZ => "Sleep, soreness, fatigue, stress & mood",
        ReadinessSignal::HrvLnRmssd => "HRV (rMSSD)",
        ReadinessSignal::RestingHr => "Resting HR",
        _ => "Readiness",
    };
    BaselineStatusView {
        signal: s.signal,
        label: label.to_string(),
        have: s.have as u32,
        need: s.need as u32,
        kind: s.kind,
    }
}

/// Readiness window (days) after which a performance signal that can drive a
/// load INCREASE is treated as stale and dropped. An engine heuristic (the KB
/// gives no expiry parameter), chosen conservatively (a fortnight). Only the
/// increase-driving signals expire (see [`is_expirable_perf_signal`]); protective
/// signals never do (HARD RULE 3).
pub(super) const PERF_SIGNAL_EXPIRY_DAYS: i64 = 14;

/// Whether a reading drives a load increase (RPE below target, e1RM ratio > 1,
/// bar velocity). Only those expire; protective readings never do (HARD RULE 3).
pub(super) fn is_expirable_perf_signal(input: &ReadinessInput) -> bool {
    match input.signal {
        // RPE below target proposes an increase; on/above target holds or reduces.
        ReadinessSignal::Rpe => input.value < 0.0,
        // e1RM ratio above baseline proposes an increase; at/below baseline is
        // stable or a protective decline (reduce/deload) → keep.
        ReadinessSignal::EstimatedOneRm => input.value > 1.0,
        // Recorded-only VBT-increase input; no protective BarVelocity gate exists.
        ReadinessSignal::BarVelocity => true,
        _ => false,
    }
}

/// feedback-040 provisional framing: present until ~14 distinct dated days of
/// logged data (readiness, sets, runs) exist. A run counts on the local day it
/// STARTED (`RunFacts::started`), the day it is credited everywhere else.
pub(super) fn build_provisional(model: &Model, facts: &RunFacts) -> Option<AdjustmentView> {
    use std::collections::BTreeSet;
    let offset = model.today_utc_offset_sec;
    let mut days: BTreeSet<i64> = model
        .inputs
        .iter()
        .map(|i| i.observed_at)
        .chain(model.sets.iter().map(|s| s.observed_at))
        .filter(|t| !is_undated(*t))
        .map(|t| local_day(t, offset))
        .chain(
            model
                .runs
                .iter()
                .zip(&facts.started)
                .filter(|(r, _)| !is_undated(r.observed_at))
                .map(|(_, &start)| local_day(start, offset)),
        )
        .collect();
    // Count check-in days only within the derivation's reach of the NEWEST
    // check-in: an older check-in never affects the derived baselines, so it
    // does not count towards the provisional "N so far".
    days.extend(autoreg::checkin_days_in_reach(&model.checkins, offset));
    let n = days.len().min(usize::from(u16::MAX)) as u16;
    let prov = feedback::provisional_until_baseline(n);
    prov.value.then(|| {
        // One plain line: the loads are typical starting values that adapt.
        let text = if n == 0 {
            "Starting weights are typical values; they adapt as you log.".to_string()
        } else {
            format!(
                "Starting weights are typical values; they adapt as you log (day {n} of about {}).",
                feedback::PROVISIONAL_BASELINE_DAYS
            )
        };
        to_view_with(text, &prov)
    })
}

/// Which autoregulation signal source is active (autoreg-047/048): HRV rolling
/// when HRV data is usable, else subjective + performance, else
/// performance-only with loads held. `None` until any readiness input exists.
/// `hrv_suspended` = the HRV channel is suspended by the artifact guard
/// (autoreg-049/050), which falls back to check-ins + logged sets.
pub(super) fn build_autoreg_source(
    inputs: &[ReadinessInput],
    hrv_baseline_active: bool,
    hrv_suspended: bool,
    offset: i64,
) -> Option<AdjustmentView> {
    if inputs.is_empty() && !hrv_suspended {
        return None;
    }
    let latest_day = inputs
        .iter()
        .map(|i| local_day(i.observed_at, offset))
        .max()
        .unwrap_or(0);
    let hrv_today = inputs.iter().any(|i| {
        i.signal == ReadinessSignal::HrvLnRmssd && local_day(i.observed_at, offset) == latest_day
    });
    let recent_hrv = inputs
        .iter()
        .filter(|i| {
            i.signal == ReadinessSignal::HrvLnRmssd
                && local_day(i.observed_at, offset) >= latest_day - 7
        })
        .count()
        .min(255) as u8;
    let subjective = inputs.iter().any(|i| {
        matches!(
            i.signal,
            ReadinessSignal::WellnessZ | ReadinessSignal::Soreness
        )
    });
    // An active baseline is collapsed into one `HrvLnRmssd` input dated at the
    // newest HRV day (`autoreg::derive_readiness`), so it counts as usable HRV
    // even when that day is not the latest.
    let src = autoreg::autoreg_source(hrv_today || hrv_baseline_active, recent_hrv, subjective);
    // One plain line: what the weights follow right now.
    let text = match src.value {
        _ if hrv_suspended => {
            "HRV readings look unreliable, so weights follow your check-ins and logged sets."
                .to_string()
        }
        // ready-upd-001: on lift days HRV only pauses progression; runs ease.
        autoreg::AutoregSource::HrvRolling => format!(
            "Your {}-day HRV trend can pause progress and ease runs; lift weights follow your logged sets.",
            autoreg::HRV_ROLLING_DAYS
        ),
        autoreg::AutoregSource::SubjectivePlusPerformance => {
            "Weights follow your check-ins and logged sets.".to_string()
        }
        autoreg::AutoregSource::PerformanceOnlyHold => {
            "Weights stay as planned until you add a Morning check-in.".to_string()
        }
    };
    Some(to_view_with(text, &src))
}

/// Whether any active readiness adjustment suppresses a load/volume increase: a
/// protective (load-reducing / rest / defer) recommendation is in scope, or
/// training is blocked outright. An increase row must not displace the
/// prescription while this holds (the readiness↔review reconciliation and the
/// review's own increase rows share this predicate).
pub(super) fn suppression_active(recs: &[AdjustmentCall], train_blocked: bool) -> bool {
    train_blocked
        || recs.iter().any(|r| {
            matches!(
                r.value.base(),
                Adjustment::ReduceLoadPct(_)
                    | Adjustment::Deload { .. }
                    | Adjustment::CapRpe(_)
                    | Adjustment::CapTopEnd { .. }
                    | Adjustment::ReduceVolumePct(_)
                    | Adjustment::DowngradeSession
                    | Adjustment::ModifyAndMonitor { .. }
                    | Adjustment::StopItems { .. }
                    | Adjustment::ReturnCheck
                    | Adjustment::RestDay
                    | Adjustment::Stop
                    | Adjustment::Defer { .. }
            )
        })
}

/// How long a positive weekly RED-S screen keeps the deficit refusal armed
/// (REDS-SCREEN-001): the 4 weekly check-ins after it, an engine window for a
/// recurring weekly screen (conservative: it only withholds a deficit).
pub(super) const REDS_SCREEN_WINDOW_DAYS: i64 = 28;

/// Whether an adjustment takes training off today (stop, rest day, deferral).
pub(super) fn blocks_training(r: &AdjustmentCall) -> bool {
    matches!(
        r.value,
        Adjustment::Stop | Adjustment::RestDay | Adjustment::Defer { .. }
    )
}

/// Why training is on hold (contract C2), the most specific reason first: a
/// profile health gate (it stops the plan itself), else the reason of the
/// blocking call itself (`SafetyState::hold`). `None` without a hold.
pub(super) fn hold_reason(
    hold: Option<&AdjustmentCall>,
    health: Option<&HealthScreen>,
) -> Option<HoldReason> {
    let hold = hold?;
    if let Some(h) = health.filter(|h| h.any_gate()) {
        return Some(if h.youth {
            HoldReason::Youth
        } else if h.pregnant || h.pregnancy_warning_sign {
            HoldReason::Pregnancy
        } else if h.symptomatic_any() {
            HoldReason::Symptomatic
        } else if h.parq_followup_uncleared() {
            HoldReason::Parq
        } else if h.injury_any() {
            HoldReason::InjuryRehab
        } else if h.reds_signal {
            HoldReason::Reds
        } else {
            HoldReason::AcuteIllnessProfile
        });
    }
    Some(match hold.kind() {
        AdjustmentKind::RedsReferral | AdjustmentKind::Screen(ScreenGate::Reds) => HoldReason::Reds,
        kind => match kind.tier() {
            SafetyTier::Pain => HoldReason::Pain,
            SafetyTier::Illness => HoldReason::Illness,
            _ => HoldReason::MedicalSignal,
        },
    })
}

/// The check-in history the derivation reads: HRV readings logged before a
/// `RestartHrvBaseline` are blanked (a new device / timing, HRV-SOURCE-001).
pub(super) fn checkins_for_derivation(model: &Model) -> std::borrow::Cow<'_, [CheckinInput]> {
    match model.hrv_baseline_from {
        None => std::borrow::Cow::Borrowed(&model.checkins),
        Some(from) => std::borrow::Cow::Owned(
            model
                .checkins
                .iter()
                .map(|c| {
                    let mut c = c.clone();
                    if c.observed_at < from {
                        c.hrv_rmssd_ms = None;
                    }
                    c
                })
                .collect(),
        ),
    }
}

/// A rendered adjustment row and what the Today headline ranks it on.
pub(in crate::app) struct RankedRow {
    pub(in crate::app) view: AdjustmentView,
    /// The call's §5 safety-ladder rung; `None` for an untiered row.
    pub(in crate::app) tier: Option<SafetyTier>,
    /// A graded "no change" verdict carrying no real `Adjustment`: it renders
    /// but never headlines.
    pub(in crate::app) neutral: bool,
}

impl RankedRow {
    /// An untiered row that may headline.
    pub(in crate::app) fn plain(view: AdjustmentView) -> Self {
        RankedRow {
            view,
            tier: None,
            neutral: false,
        }
    }
}

/// Every source the Today headline ranks, consulted in priority order by
/// [`build_headline`].
pub(super) struct HeadlineInputs<'a> {
    pub(super) train_blocked: bool,
    /// Onboarding safety gates (medical referral).
    pub(super) gates: &'a [AdjustmentCall],
    /// Readiness recommendations.
    pub(super) recommended: &'a [AdjustmentCall],
    /// Post-session review recommendations.
    pub(super) review_recs: &'a [AdjustmentCall],
    pub(super) adjustments: &'a [RankedRow],
    pub(super) review_adjustments: &'a [RankedRow],
    pub(super) feedback: Option<&'a FeedbackView>,
    /// The next planned session (the prescription rung).
    pub(super) prescription: Option<&'a SessionPlanView>,
}

/// The single highest-priority call for today (usability-ia-spec §7). The
/// first rung that applies wins:
/// 1. `SafetyHold` when training is blocked: the blocking onboarding gate,
///    then readiness stop, then review deferral.
/// 2. Referral feedback: safety-critical and in an injury, energy or behavior
///    concern category.
/// 3. The best non-neutral adjustment, when it is at `SafetyTier::Illness` or
///    above.
/// 4. Any other safety-critical feedback (e.g. a run-load spike).
/// 5. The best non-neutral adjustment below `Illness` (or untiered).
/// 6. `Prescription`: the next planned session, unless it is blocked.
/// 7. Any other feedback, then `AllClear`.
pub(super) fn build_headline(inputs: &HeadlineInputs<'_>) -> TodayHeadlineView {
    let &HeadlineInputs {
        train_blocked,
        gates,
        recommended,
        review_recs,
        adjustments,
        review_adjustments,
        feedback,
        prescription,
    } = inputs;
    let from_adjustment = |kind: HeadlineKind, v: &AdjustmentView| TodayHeadlineView {
        kind,
        summary: v.summary.clone(),
        evidence: v.evidence.clone(),
        why: v.why.clone(),
    };
    let from_feedback = |fb: &FeedbackView| TodayHeadlineView {
        kind: HeadlineKind::Feedback,
        summary: fb.message.clone(),
        evidence: fb.evidence.clone(),
        why: fb.why.clone(),
    };
    if train_blocked {
        let blocks = |r: &&AdjustmentCall| blocks_training(r);
        // Same dominance order view() uses for the tier: onboarding gates
        // (medical referral), then readiness stops, then review deferrals.
        if let Some(stop) = gates
            .iter()
            .find(blocks)
            .or_else(|| recommended.iter().find(blocks))
            .or_else(|| review_recs.iter().find(blocks))
        {
            return from_adjustment(HeadlineKind::SafetyHold, &to_view(stop));
        }
    }
    // A medical-referral feedback card (bone-stress signs, RED-S, compulsive
    // exercise) is a referral, not a programming tweak: it outranks every
    // training rung below. Kind stays "feedback" so shells render it unchanged.
    let referral = |fb: &FeedbackView| {
        fb.evidence.safety_critical
            && matches!(
                fb.category,
                FeedbackCategory::ConcernInjury
                    | FeedbackCategory::ConcernEnergy
                    | FeedbackCategory::ConcernBehavior
            )
    };
    if let Some(fb) = feedback
        && referral(fb)
    {
        return from_feedback(fb);
    }
    // The best adjustment ranks on the §5 ladder first (HARD RULE 3), then
    // safety-critical, grade and confidence; the FIRST of equals wins, so the
    // rule layer's own ordering (pain responses lead) decides ties.
    let rank = |v: &RankedRow| {
        (
            v.tier,
            v.view.evidence.safety_critical,
            v.view.evidence.grade,
        )
    };
    let best = adjustments
        .iter()
        .chain(review_adjustments.iter())
        // Skip neutral verdict rows (graded "no change" calls carrying no real
        // Adjustment); a stale review must not headline "hold planned loads"
        // over the prescription.
        .filter(|v| !v.neutral)
        .fold(None::<&RankedRow>, |best, v| match best {
            Some(b)
                if rank(v)
                    .cmp(&rank(b))
                    .then(
                        v.view
                            .evidence
                            .confidence
                            .total_cmp(&b.view.evidence.confidence),
                    )
                    .is_le() =>
            {
                Some(b)
            }
            _ => Some(v),
        });
    // Other safety-critical feedback (a recovery concern, a run-load spike)
    // outranks every adjustment below Illness.
    let critical_feedback = feedback.filter(|fb| fb.evidence.safety_critical);
    if let Some(v) = best {
        if let Some(fb) = critical_feedback
            && v.tier < Some(SafetyTier::Illness)
        {
            return from_feedback(fb);
        }
        return from_adjustment(HeadlineKind::Adjustment, &v.view);
    }
    if let Some(fb) = critical_feedback {
        return from_feedback(fb);
    }
    // The session card states the session, so this rung carries no summary.
    if let Some(ns) = prescription
        && ns.status != SessionStatus::Blocked
        && let Some(first) = ns.items.first()
    {
        return TodayHeadlineView {
            kind: HeadlineKind::Prescription,
            summary: String::new(),
            evidence: first.evidence.clone(),
            why: first.why.clone(),
        };
    }
    // Only non-critical feedback reaches here.
    if let Some(fb) = feedback {
        return from_feedback(fb);
    }
    TodayHeadlineView {
        kind: HeadlineKind::AllClear,
        // States the absence of any triggered rule, not a graded claim, so
        // no evidence tag is attached (empty grade; shells render no chip).
        summary: "Train as planned today.".to_string(),
        ..TodayHeadlineView::default()
    }
}

/// Static signal→group metadata for every readiness signal, in picker order
/// (metrics before the red-flag block).
pub(super) fn build_signal_groups() -> std::borrow::Cow<'static, [SignalGroupView]> {
    static CACHE: std::sync::LazyLock<Vec<SignalGroupView>> = std::sync::LazyLock::new(|| {
        autoreg::ALL_SIGNALS
            .iter()
            .map(|&s| SignalGroupView {
                signal: s,
                group: autoreg::signal_group(s),
            })
            .collect()
    });
    std::borrow::Cow::Borrowed(&CACHE)
}

/// Logged sets dated on or before local day `through` as performance-derivation
/// inputs. Holds and undated legacy rows are left out (a hold has
/// no e1RM, an undated set has no session day), and planned deload weeks are
/// skipped ([`crate::plan::lift_deload_week`]).
pub(super) fn perf_sets<'s>(
    sets: &'s [LoggedSet],
    through: i64,
    offset: i64,
    deloads: DeloadDays,
) -> Vec<autoreg::PerfSet<'s>> {
    sets.iter()
        .filter(|s| s.hold_sec.is_none() && !is_undated(s.observed_at))
        .filter(|s| {
            let day = local_day(s.observed_at, offset);
            day <= through && !deloads.contains(day)
        })
        .map(|s| autoreg::PerfSet {
            exercise: &s.exercise,
            weight_kg: s.weight_kg,
            reps: s.reps,
            rpe: s.rpe,
            observed_at: s.observed_at,
        })
        .collect()
}

/// The autoreg-029 saturation guard is armed by the profile's
/// manual "hard training block" flag OR automatically in the block's peak
/// accumulation week (the last load week before the planned recovery week),
/// where unusually high HRV is more likely saturation than freshness
/// (AUTOREG-HRV-SAT-001, Plews; Nuuttila 2024). Never in a recovery week.
pub(super) fn effective_high_load_block(
    model: &Model,
    days: &LoggedDays,
    profile: &Profile,
    day: i64,
) -> bool {
    if profile.high_load_block {
        return true;
    }
    let Some(idx) = plan_week_index(model, days, monday_of(day)) else {
        return false;
    };
    crate::plan::last_load_week(&crate::profile::PlanProfile::new(profile), idx)
}

/// Whether a row stamped `at` is dated on or before local day `day` (an undated
/// legacy row always counts).
fn dated_through(at: i64, day: i64, offset: i64) -> bool {
    is_undated(at) || local_day(at, offset) <= day
}

/// The manual readiness inputs dated on or before local day `dated_by` as they
/// stood on local day `shown_on`: every active one, plus every retracted one
/// whose removal fell AFTER `shown_on` (it was still in force that day).
/// Retracted inputs come first: they were submitted before their removal, and
/// the active ones keep their submission order behind them.
pub(super) fn manual_inputs_on(
    model: &Model,
    dated_by: i64,
    shown_on: i64,
    offset: i64,
) -> Vec<ReadinessInput> {
    model
        .retracted_inputs
        .iter()
        .filter(|r| local_day(r.retracted_at, offset) > shown_on)
        .map(|r| &r.input)
        .chain(model.inputs.iter())
        .filter(|i| dated_through(i.observed_at, dated_by, offset))
        .cloned()
        .collect()
}

/// The readiness picture as of local day `as_of` (`None`: the shell has sent
/// no clock, so nothing is date-filtered or expired): the check-in signals
/// derived from the check-ins through that day, the manual inputs as they
/// stood on it (a later removal never re-judges it), logged-set performance
/// through it, and the rule context. Today's view and every past rendered day
/// read this one pipeline.
pub(super) struct ReadinessPicture {
    pub(super) derived: autoreg::DerivedReadiness,
    /// Derived signals first, then the manual inputs, so `latest_input`'s
    /// recency tie-break lets an explicit entry win over a same-instant
    /// derived one; stale increase-driving performance readings dropped.
    pub(super) inputs: Vec<ReadinessInput>,
    perf: Vec<autoreg::MovementPerf>,
    /// The profile's lifting goal (`None`: no profile): selects the
    /// velocity-loss threshold (autoreg-010).
    lift_goal: Option<LiftGoal>,
    /// Arms the autoreg-029 HRV saturation guard.
    high_load_block: bool,
    as_of: Option<i64>,
    offset: i64,
}

impl ReadinessPicture {
    pub(super) fn rule_ctx(&self) -> autoreg::RuleContext<'_> {
        autoreg::RuleContext {
            today_epoch_day: self.as_of,
            utc_offset_sec: self.offset,
            rhr: self.derived.rhr,
            perf: &self.perf,
        }
    }

    /// The wellness composite is still collecting its baseline: a reading that
    /// fires the rule is the absolute floor of a very low morning, not a drop
    /// below a personal normal that doesn't exist yet.
    pub(super) fn wellness_collecting(&self) -> bool {
        self.derived
            .collecting
            .iter()
            .any(|b| b.signal == ReadinessSignal::WellnessZ)
    }

    /// The highest §5 safety tier the readiness calls raise (the return step
    /// after a resolved hold raises none).
    pub(super) fn safety_tier(&self) -> Option<SafetyTier> {
        autoreg::safety_tier_of(&self.readiness_calls())
    }

    /// The readiness rules' calls on these inputs.
    fn readiness_calls(&self) -> Vec<AdjustmentCall> {
        autoreg::adjustments_in(
            &self.inputs,
            &self.rule_ctx(),
            self.lift_goal,
            self.high_load_block,
        )
    }

    /// Whether auto load-adds are withheld (autoreg-044 / autoreg-029).
    pub(super) fn increase_hold(&self) -> bool {
        autoreg::increase_hold(
            &self.inputs,
            &self.rule_ctx(),
            self.lift_goal,
            self.high_load_block,
        )
    }

    /// The per-signal readiness rows, resolving each row's judging claim to its
    /// registry evidence (rows with no judging rule keep an empty tag, they
    /// state facts, not recommendations). The strip surfaces recency-expired
    /// RHR/wellness readings as an "out of date" row, so it reads the stale
    /// inputs too; the rules run only on the fresh ones.
    pub(super) fn signal_rows(&self) -> Vec<ReadinessSignalView> {
        let mut display_inputs = self.inputs.clone();
        display_inputs.extend(self.derived.stale.iter().cloned());
        autoreg::signal_states_in(
            &display_inputs,
            &self.rule_ctx(),
            self.lift_goal,
            self.high_load_block,
            self.wellness_collecting(),
            &self.derived.stale,
        )
        .into_iter()
        .map(|s| {
            let evidence = s
                .claim
                .map(|id| EvidenceTagView::of(&Recommended::new((), id)))
                .unwrap_or_default();
            ReadinessSignalView {
                signal: s.signal,
                group: autoreg::signal_group(s.signal),
                value: s.value,
                streak: s.streak,
                state: s.state,
                detail: s.detail,
                evidence,
                from_today: s.from_today,
                stale: s.stale,
            }
        })
        .collect()
    }

    /// The rules' adjustments plus, with a clock, the first session after a
    /// resolved pain or illness hold (the easy session or the return check);
    /// `days` dates each logged run by its start.
    pub(super) fn adjustments(&self, model: &Model, days: &LoggedDays) -> Vec<AdjustmentCall> {
        let mut recs = self.readiness_calls();
        if let Some(day) = self.as_of {
            recs.extend(return_step_on(model, days, day, self.offset));
        }
        recs
    }
}

/// Build the [`ReadinessPicture`] as of `as_of` with `profile` (the one in force
/// that day) selecting the goal-dependent thresholds; no profile → conservative
/// defaults.
pub(super) fn readiness_picture(
    model: &Model,
    days: &LoggedDays,
    profile: Option<&Profile>,
    as_of: Option<i64>,
) -> ReadinessPicture {
    let offset = model.today_utc_offset_sec;
    let lift_goal = profile.map(|p| p.lift_goal);
    // autoreg-029 guard: the manual "hard block" flag, or auto-armed in the
    // block's peak accumulation week. No `as_of` (no shell today yet): the plan
    // start, the week the plan views show.
    let high_load_block = match (profile, as_of.or_else(|| model.today_or_plan_start())) {
        (Some(p), Some(day)) => effective_high_load_block(model, days, p, day),
        (Some(p), None) => p.high_load_block,
        (None, _) => false,
    };
    let all = checkins_for_derivation(model);
    let dated = |c: &CheckinInput| as_of.is_none_or(|d| dated_through(c.observed_at, d, offset));
    let checkins: std::borrow::Cow<'_, [CheckinInput]> = if all.iter().all(dated) {
        all
    } else {
        std::borrow::Cow::Owned(all.iter().filter(|c| dated(c)).cloned().collect())
    };
    let derived = autoreg::derive_readiness(&checkins, offset, as_of);
    let mut inputs = derived.inputs.clone();
    match as_of {
        Some(day) => inputs.extend(manual_inputs_on(model, day, day, offset)),
        None => inputs.extend(model.inputs.iter().cloned()),
    }
    // A "felt easy" performance signal goes stale: a three-week-old RPE −2 must
    // not keep proposing IncreaseLoadPct. Only readings that can drive a load
    // INCREASE expire (dropping them is conservative); a protective reading in
    // the same channel, and every safety signal, never does (HARD RULE 3). The
    // 14-day window is an engine heuristic (no KB parameter). Needs a clock.
    if let Some(day) = as_of {
        inputs.retain(|i| {
            !is_expirable_perf_signal(i)
                || day.saturating_sub(local_day(i.observed_at, offset)) <= PERF_SIGNAL_EXPIRY_DAYS
        });
    }
    // The Strong-graded performance backbone reads the LOGGED sets
    // (top-set e1RM vs the 14-day best), not a hand-typed ratio.
    let perf = as_of
        .map(|day| {
            let deloads = lift_deload_days(model, days);
            let sets = perf_sets(&model.sets, day, offset, deloads);
            autoreg::derive_performance(&sets, day, offset)
        })
        .unwrap_or_default();
    ReadinessPicture {
        derived,
        inputs,
        perf,
        lift_goal,
        high_load_block,
        as_of,
        offset,
    }
}

/// ready-upd-003/004: the first-session-back adjustment in force on
/// local `day`. A pain or illness hold must have ended on or before `day` (the
/// user's dated "I'm better" / "I'm pain-free", or a newer all-clear report),
/// no hold may be active now, and no session (a logged set or run) may have
/// been done on a day from the end of the hold up to, but not including,
/// `day`: one session back is easy (or the post-illness return check), then
/// the plan resumes. A run counts on the day it STARTED (`days`, the plan
/// completion attribution), so one begun before the hold's last day and
/// finished after midnight is not a session back (safety first). Inputs and
/// retractions are taken as they stood on `day`, so a later retraction never
/// re-judges an earlier day.
pub(super) fn return_step_on(
    model: &Model,
    days: &LoggedDays,
    day: i64,
    offset: i64,
) -> Option<AdjustmentCall> {
    let dated_by = |at: i64| dated_through(at, day, offset);
    let timeline: Vec<autoreg::TimedInput> = model
        .inputs
        .iter()
        .filter(|i| dated_by(i.observed_at))
        .map(|input| autoreg::TimedInput {
            input,
            retracted_at: None,
        })
        .chain(
            model
                .retracted_inputs
                .iter()
                .filter(|r| dated_by(r.input.observed_at))
                .map(|r| autoreg::TimedInput {
                    input: &r.input,
                    // A removal dated after `day` was not yet in force on it.
                    retracted_at: dated_by(r.retracted_at).then_some(r.retracted_at),
                }),
        )
        .collect();
    let (kind, ended_at) = autoreg::last_hold_resolution(&timeline)?;
    let ended_day = if !is_undated(ended_at) {
        local_day(ended_at, offset)
    } else {
        i64::MIN
    };
    if ended_day > day {
        return None;
    }
    let trained_since = model
        .sets
        .iter()
        .map(|s| s.observed_at)
        .filter(|t| !is_undated(*t))
        .map(|t| local_day(t, offset))
        .chain(days.run_days().iter().flatten().copied())
        .any(|d| d >= ended_day && d < day);
    (!trained_since).then(|| autoreg::return_adjustment(kind))
}

/// The banner's resolve action per active hold: pain first
/// (it outranks illness on the ladder), then illness. Illness is ONE action
/// covering both ways the user can say they are ill: an active illness report
/// and the profile's "I'm ill right now" answer (`profile_ill`), so a single
/// "I've recovered" / "I'm better" ends both. Its label follows the profile's
/// gate copy ("Then tap I'm better") when that answer is on; only a resolved
/// report leads into the easier return session (ready-upd-003). "No fever" is
/// part of the recovery check only for a fever-class illness (a below-neck or
/// fever report, or the profile's ill answer: ready-upd-004); a head cold
/// clears once its symptoms are gone (ready-upd-008).
pub(super) fn hold_resolutions(
    inputs: &[ReadinessInput],
    profile_ill: bool,
) -> Vec<HoldResolutionView> {
    let mut out = Vec::new();
    if autoreg::pain_hold_active(inputs) {
        // A scoped stop leaves the rest of the session on, so its line names
        // what stays out rather than saying training is paused.
        let paused_text = if autoreg::pain_blocks_training(inputs) {
            "Training is paused until you're pain-free through the full movement."
        } else {
            "The exercises that load the painful area stay out until you're pain-free through the full movement."
        };
        out.push(HoldResolutionView {
            signal: ReadinessSignal::Pain,
            paused_text: paused_text.into(),
            action_label: "I'm pain-free now".into(),
            confirm_text:
                "Confirm only if you're pain-free through the full movement. The plan restarts with an easier session."
                    .into(),
            ends_profile_illness: false,
        });
    }
    let reported = autoreg::illness_active(inputs);
    if reported || profile_ill {
        let action_label = if profile_ill {
            "I'm better"
        } else {
            "I've recovered"
        };
        let fever = profile_ill || autoreg::illness_stop(inputs);
        // A head cold eases training but does not pause it (ready-upd-008).
        let (paused_text, check) = if fever {
            (
                "Training is paused until your symptoms are gone and you have no fever.",
                "Confirm only if your symptoms are gone and you have no fever.",
            )
        } else {
            (
                "Sessions stay short and easy until your symptoms are gone.",
                "Confirm only if your symptoms are gone.",
            )
        };
        let next = if reported {
            "The plan restarts with an easier session."
        } else {
            "Your plan picks up again from today."
        };
        out.push(HoldResolutionView {
            signal: ReadinessSignal::Illness,
            paused_text: paused_text.into(),
            action_label: action_label.into(),
            confirm_text: format!("{check} {next}"),
            ends_profile_illness: profile_ill,
        });
    }
    out
}
