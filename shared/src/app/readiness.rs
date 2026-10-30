//! Readiness assembly helpers: performance-signal expiry, provisional/autoreg
//! source rows, the Today headline, baseline echoes, and the readiness signal
//! groups.

use super::*;

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

/// End-of-local-today boundary, unix seconds: a row with `observed_at`
/// beyond it is FUTURE-dated (a mis-imported 2030 GPX) and must not anchor
/// "current" windows (weekly-report cur-week, CTL/ATL last day). Same local-day
/// boundary the run anchors / `session_logged` use. `None` when the shell has not
/// supplied "today" (no clock in the core) → no guard.
pub(super) fn end_of_local_today_sec(model: &Model) -> Option<i64> {
    model.today_epoch_day.map(|d| {
        d.saturating_add(1)
            .saturating_mul(DAY_SEC)
            .saturating_sub(model.today_utc_offset_sec)
            .saturating_sub(1)
    })
}

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
        .filter(|t| *t > 0)
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
    // Count check-in days only within the baseline window of the NEWEST check-in:
    // an older check-in never affects the derived baselines, so it does not count
    // towards the provisional "N so far".
    let newest_checkin_day = model
        .checkins
        .iter()
        .filter(|c| !is_undated(c.observed_at))
        .map(|c| local_day(c.observed_at, offset))
        .max();
    for c in &model.checkins {
        if !is_undated(c.observed_at) {
            let d = local_day(c.observed_at, offset);
            if newest_checkin_day.is_none_or(|n| n - d < autoreg::LONG_BASELINE_WINDOW_DAYS) {
                days.insert(d);
            }
        }
    }
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
        }
        // ready-upd-001: on lift days HRV only pauses progression; runs ease.
        autoreg::AutoregSource::HrvRolling => {
            "Your 7-day HRV trend can pause progress and ease runs; lift weights follow your logged sets."
        }
        autoreg::AutoregSource::SubjectivePlusPerformance => {
            "Weights follow your check-ins and logged sets."
        }
        autoreg::AutoregSource::PerformanceOnlyHold => {
            "Weights stay as planned until you add a Morning check-in."
        }
    };
    Some(to_view_with(text.to_string(), &src))
}

/// Whether any active readiness adjustment suppresses a load/volume increase: a
/// protective (load-reducing / rest / defer) recommendation is in scope, or
/// training is blocked outright. An increase row must not displace the
/// prescription while this holds (the readiness↔review reconciliation and the
/// review's own increase rows share this predicate).
pub(super) fn suppression_active(recs: &[Recommended<Adjustment>], train_blocked: bool) -> bool {
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

/// Claims behind a pain response (the File 08 pain ladder).
fn is_pain_claim(c: ClaimId) -> bool {
    matches!(
        c,
        claims::SAFE_PAIN_001 | claims::SAFE_PAIN_STRUCT_001 | claims::SAFE_TENDON_001
    )
}

/// Claims behind an illness response (neck check, return-to-training, head cold).
fn is_illness_claim(c: ClaimId) -> bool {
    matches!(
        c,
        claims::ILLNESS_NECK_001 | claims::ILLNESS_RTT_001 | claims::ILLNESS_COLD_RUN_001
    )
}

/// The File 06 §5 ladder tier a recommendation belongs to, for ranking the
/// Today headline: the same ladder `autoreg::resolve_safety_in` raises, keyed
/// by the rule that emitted the call. `None` for calls off the ladder (e.g. a
/// load increase).
pub(super) fn adjustment_tier(r: &Recommended<Adjustment>) -> Option<SafetyTier> {
    let tier = match (r.value.base(), r.claim_id()) {
        (Adjustment::Defer { .. }, _) => SafetyTier::MedicalReferral,
        (_, c) if is_pain_claim(c) => SafetyTier::Pain,
        (_, c) if is_illness_claim(c) || c == claims::RETURN_HOLD_001 => SafetyTier::Illness,
        (Adjustment::RestDay, claims::AUTOREG_RHR_NOISE_001) => SafetyTier::Illness,
        (Adjustment::IncreaseLoadPct(_), _) => return None,
        (Adjustment::DowngradeSession, claims::WELLNESS_001) => SafetyTier::SubjectiveMultiDay,
        (Adjustment::DowngradeSession, claims::HRV_001) => SafetyTier::HrvTrend,
        (Adjustment::CapTopEnd { trigger }, _) if *trigger == ReadinessSignal::HrvLnRmssd => {
            SafetyTier::HrvTrend
        }
        (Adjustment::CapTopEnd { .. }, _) => SafetyTier::SingleDayMarker,
        (_, claims::AUTOREG_RHR_NOISE_001 | claims::AUTOREG_RHR_DOWN_001) => {
            SafetyTier::SingleDayMarker
        }
        (
            Adjustment::ReduceLoadPct(_)
            | Adjustment::Deload { .. }
            | Adjustment::CapRpe(_)
            | Adjustment::DowngradeSession,
            _,
        ) => SafetyTier::ObjectivePerformance,
        _ => return None,
    };
    Some(tier)
}

/// Whether an adjustment takes training off today (stop, rest day, deferral).
pub(super) fn blocks_training(r: &Recommended<Adjustment>) -> bool {
    matches!(
        r.value,
        Adjustment::Stop | Adjustment::RestDay | Adjustment::Defer { .. }
    )
}

/// Why training is on hold (contract C2), the most specific reason first: a
/// profile health gate (it stops the plan itself), else the reason of the
/// blocking call itself (`SafetyState::hold`). `None` without a hold.
pub(super) fn hold_reason(
    hold: Option<&Recommended<Adjustment>>,
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
    let claim = hold.claim_id();
    Some(if claim == claims::SAFE_REDS_001 {
        HoldReason::Reds
    } else if is_pain_claim(claim) {
        HoldReason::Pain
    } else if is_illness_claim(claim) || matches!(hold.value, Adjustment::RestDay) {
        HoldReason::Illness
    } else {
        HoldReason::MedicalSignal
    })
}

/// Content fingerprint of the check-in history (every field the derivation
/// reads), so a memo over derived readiness misses when any check-in changes.
pub(super) fn checkins_fingerprint(checkins: &[CheckinInput]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    checkins.len().hash(&mut h);
    for c in checkins {
        c.observed_at.hash(&mut h);
        c.sleep_quality.hash(&mut h);
        c.soreness.hash(&mut h);
        c.mood.hash(&mut h);
        c.fatigue.hash(&mut h);
        c.stress.hash(&mut h);
        c.resting_hr_bpm.map(f64::to_bits).hash(&mut h);
        c.hrv_rmssd_ms.map(f64::to_bits).hash(&mut h);
    }
    h.finish()
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

/// Every source the Today headline ranks, consulted in priority order by
/// [`build_headline`].
pub(super) struct HeadlineInputs<'a> {
    pub(super) train_blocked: bool,
    /// Onboarding safety gates (medical referral).
    pub(super) gates: &'a [Recommended<Adjustment>],
    /// Readiness recommendations.
    pub(super) recommended: &'a [Recommended<Adjustment>],
    /// Post-session review recommendations.
    pub(super) review_recs: &'a [Recommended<Adjustment>],
    pub(super) adjustments: &'a [AdjustmentView],
    pub(super) review_adjustments: &'a [AdjustmentView],
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
        let blocks = |r: &&Recommended<Adjustment>| blocks_training(r);
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
    let rank = |v: &AdjustmentView| (v.tier, v.evidence.safety_critical, v.evidence.grade);
    let best = adjustments
        .iter()
        .chain(review_adjustments.iter())
        // Skip neutral verdict rows (graded "no change" calls carrying no real
        // Adjustment); a stale review must not headline "hold planned loads"
        // over the prescription.
        .filter(|v| !v.neutral)
        .fold(None::<&AdjustmentView>, |best, v| match best {
            Some(b)
                if rank(v)
                    .cmp(&rank(b))
                    .then(v.evidence.confidence.total_cmp(&b.evidence.confidence))
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
        return from_adjustment(HeadlineKind::Adjustment, v);
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
    let (load_weeks, cycle_len) = crate::plan::cycle_shape(profile);
    load_weeks >= 2 && cycle_len > 0 && idx % cycle_len == load_weeks - 1
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
    goal: Option<Goal>,
    high_load_block: bool,
    as_of: Option<i64>,
    offset: i64,
}

impl ReadinessPicture {
    pub(super) fn rule_ctx(&self) -> autoreg::RuleContext<'_> {
        autoreg::RuleContext {
            goal: self.goal.as_ref(),
            high_load_block: self.high_load_block,
            today_epoch_day: self.as_of,
            utc_offset_sec: self.offset,
            rhr: self.derived.rhr,
            perf: &self.perf,
            wellness_collecting: self
                .derived
                .collecting
                .iter()
                .any(|b| b.signal == ReadinessSignal::WellnessZ),
        }
    }

    /// The rules' adjustments plus, with a clock, the first session after a
    /// resolved pain or illness hold (the easy session or the return check);
    /// `days` dates each logged run by its start.
    pub(super) fn adjustments(
        &self,
        model: &Model,
        days: &LoggedDays,
    ) -> Vec<Recommended<Adjustment>> {
        let mut recs = autoreg::adjustments_in(&self.inputs, &self.rule_ctx());
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
    let goal = profile.map(|p| match p.lift_goal {
        LiftGoal::MaxStrength => Goal::Strength,
        LiftGoal::Power => Goal::Power,
        LiftGoal::Hypertrophy => Goal::Hypertrophy,
    });
    // autoreg-029 guard: the manual "hard block" flag, or auto-armed in the
    // block's peak accumulation week.
    let high_load_block = match (profile, as_of) {
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
        goal,
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
) -> Option<Recommended<Adjustment>> {
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
    let ended_day = if ended_at > 0 {
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
        .filter(|t| *t > 0)
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
