//! Review-driven rows: the weekly check-in's deloads and deferrals, the session
//! review's execution, monitoring and volume rows, and the filters that drop
//! a row today's session already settles.

use super::*;
use crate::feedback;
use std::borrow::Cow;

/// Claim behind the overtraining recovery week, so its row can be recognized.
pub(in crate::app) const RECOVERY_WEEK_CLAIM: ClaimId = claims::HYB_DELOAD_001;

/// Set cut of the overtraining recovery week, % (STR-DELOAD-VOL-001: about
/// half the sets).
const RECOVERY_WEEK_VOLUME_CUT_PCT: f32 = 50.0;

/// Load cut of the overtraining recovery week, % (STR-DELOAD-VOL-001: loads up
/// to 10 % lower). The lighter-week row quotes it.
pub(super) const RECOVERY_WEEK_LOAD_CUT_PCT: f32 = 10.0;

/// Week-level deload triggers carried by a weekly check-in. Distinct from the
/// readiness adjustments (which react to a single session's markers), these
/// fire off accumulated weekly fatigue counts. Each dormant autoreg fn returns
/// `None` until its threshold is crossed, so an unset field or a sub-threshold
/// count contributes nothing.
pub(in crate::app) fn review_deloads(r: &SessionReview) -> Vec<Recommended<Adjustment>> {
    review_deloads_with_reasons(r)
        .into_iter()
        .map(|(rec, _)| rec)
        .collect()
}

/// [`review_deloads`] with, for each deload, the plain-language trigger that
/// fired it, so the row says WHY (several triggers share one claim and one
/// adjustment shape, so the reason cannot be recovered from the rec alone).
/// `None` = the adjustment's own description already says it.
fn review_deloads_with_reasons(
    r: &SessionReview,
) -> Vec<(Recommended<Adjustment>, Option<Cow<'static, str>>)> {
    let mut out = Vec::new();
    if let Some(n) = r.rpe_load_gap_sessions
        && let Some(d) = autoreg::deload_from_rpe_load_gap(n)
    {
        out.push((
            d,
            Some(
                format!(
                    "because 2+ sessions this week needed weights {}% or more below plan to reach the planned effort",
                    autoreg::RPE_LOAD_GAP_DELOAD_PCT
                )
                .into(),
            ),
        ));
    }
    if let Some(v) = r.weekly_velocity_drop_m_s
        && let Some(d) = autoreg::deload_from_velocity_drop(v)
    {
        out.push((
            d,
            Some(
                format!(
                    "because your bar speed on the same weight dropped more than {} m/s this week",
                    autoreg::VBT_WEEKLY_DROP_DELOAD_M_S
                )
                .into(),
            ),
        ));
    }
    if let Some(n) = r.failed_key_sessions
        && let Some(d) = autoreg::deload_from_failed_sessions(n)
    {
        out.push((
            d,
            Some("because 2+ hard sessions weren't finished this week".into()),
        ));
    }
    // autoreg-025: the at/above-MRV sign cluster (qualitative) → deload.
    if let Some(d) = autoreg::mrv_signs_deload(r.mrv_sign_cluster) {
        out.push((
            d,
            Some("because of signs you're doing more than you can recover from (aching joints, stalling, poor sleep, low motivation)".into()),
        ));
    }
    // autoreg-024: RPE creep +1 at the same loads AND wellness z ≤ −1 for ≥3
    // days → the standard 1-week deload.
    if let Some(days) = r.wellness_z_low_days
        && let Some(d) = autoreg::deload_from_rpe_creep_and_wellness(r.rpe_creep_plus_one, days)
    {
        out.push((
            d,
            Some("because sessions felt harder at the same weights while your check-ins stayed low for 3+ days".into()),
        ));
    }
    // autoreg-028 second trigger: a single-day lnRMSSD z < −1 with a ≥2-day
    // downtrend → downgrade the session even before the 7-day average clears
    // the SWC band.
    if let (Some(z), Some(days)) = (r.hrv_single_day_z, r.hrv_downtrend_days)
        && let Some(d) = autoreg::hrv_single_day_downgrade(z, days)
    {
        out.push((d, None));
    }
    // ≥2 overtraining signs lasting a week or more → an easier week now
    // (feedback-036 "consider an easy week"; HYB-DELOAD-001 / Meeusen 2013:
    // deload when ≥2 overreaching red flags persist; running-034's
    // unscheduled down week). The same predicate as the recovery-concern card,
    // so the card and the lighter plan always appear together. Shape: the
    // standard 1-week deload, about half the sets and loads up to 10 % lower
    // (STR-DELOAD-VOL-001).
    if feedback::overtraining_sustained(overtraining_count(r), r.overtraining_signal_weeks) {
        out.push((
            graded(
                Adjustment::Deload {
                    volume_reduction_pct: RECOVERY_WEEK_VOLUME_CUT_PCT,
                    load_reduction_pct: RECOVERY_WEEK_LOAD_CUT_PCT,
                    weeks: 1,
                },
                RECOVERY_WEEK_CLAIM,
            ),
            None,
        ));
    }
    // Overtraining escalations (File 08 safety-042 > File 06 autoreg-042): a
    // ≥2-week unexplained decline that survived a deload is the Strong
    // rest-and-defer rule; otherwise the ≥2-domain NFOR cluster mandates a
    // recovery block + professional referral. At most one fires: the
    // stronger-graded rule wins when both conditions hold.
    let decline_weeks = r.decline_weeks.unwrap_or(0);
    let domains = r.suppressed_wellness_domains.unwrap_or(0);
    if let Some(d) =
        autoreg::unexplained_decline_rest_defer(decline_weeks, domains >= 1, r.despite_deload)
    {
        out.push((d, None));
    } else if let Some(d) = autoreg::nfor_cluster_defer(decline_weeks, domains) {
        out.push((d, None));
    }
    // Weekly red-flag answers defer exactly like the same signs entered as
    // readiness signals: bone-stress signs stop impact loading with an urgent
    // referral (safety-040); a positive RED-S screen or compulsive exercise
    // (safety-049's signal list; ready-upd-005 "the existing RED-S referral
    // route") reduces training stress and defers to a professional. Normal
    // load progression never runs under either (HARD RULE 3).
    if r.bone_pain_red_flag {
        out.push((autoreg::bone_stress_referral(), None));
    }
    if r.reds_signal_reported || r.compulsive_flag {
        out.push((autoreg::reds_referral(), None));
    }
    out
}

/// Readiness/profile context a [`review_views`] pass needs alongside the review:
/// the review's own deloads, the readiness adjustments in scope (for suppression),
/// whether training is blocked, the session review's age, the profile, and the
/// core-derived HRV / wellness suppression streaks.
pub(in crate::app) struct ReviewCtx<'a> {
    pub(in crate::app) recs: &'a [Recommended<Adjustment>],
    pub(in crate::app) readiness_recs: &'a [Recommended<Adjustment>],
    pub(in crate::app) train_blocked: bool,
    pub(in crate::app) review_age: Option<i64>,
    pub(in crate::app) profile: Option<&'a Profile>,
    pub(in crate::app) derived_hrv_streak: Option<u8>,
    pub(in crate::app) derived_wellness_streak: Option<u8>,
}

/// One review deload as a row, saying which trigger fired it. The overtraining
/// recovery week reads in plain words (it is the action behind the recovery
/// card, so one trigger shows one card plus the lighter plan, never three
/// restatements); its running clause appears only for a profile that runs.
fn review_rec_view(
    r: &Recommended<Adjustment>,
    reason: Option<&str>,
    runs: bool,
) -> AdjustmentView {
    let mut v = if r.claim_id() == RECOVERY_WEEK_CLAIM {
        let what = if runs {
            format!(
                "about half the sets and half the running, up to {}% less weight, and every run easy",
                RECOVERY_WEEK_LOAD_CUT_PCT
            )
        } else {
            format!(
                "about half the sets and up to {}% less weight",
                RECOVERY_WEEK_LOAD_CUT_PCT
            )
        };
        to_view_with(
            format!(
                "Lighter week: {what}. Signs you're not recovering have lasted a week or more."
            ),
            r,
        )
    } else {
        match reason {
            Some(why) => to_view_with(format!("{}, {why}", describe(&r.value)), r),
            None => to_view(r),
        }
    };
    v.tier = adjustment_tier(r);
    v
}

/// Flatten the review's week-level triggers for the shell: the deload/deferral
/// adjustments plus the autoreg-032 threshold re-test note (a progression cue,
/// not an `Adjustment`, so it is rendered with its own summary).
pub(in crate::app) fn review_views(r: &SessionReview, ctx: ReviewCtx) -> Vec<AdjustmentView> {
    let mut out = week_trigger_rows(r, &ctx);
    // running-034 / hybrid-026 (≥2 overtraining signals → down week) are carried
    // by the recovery-week deload (`review_deloads`), which the plan folds into
    // the next sessions; no separate text rows restate it.
    push_execution_rows(&mut out, r, &ctx);
    push_monitoring_rows(&mut out, r, &ctx);
    push_hypertrophy_rows(&mut out, r);
    // Profile-dependent review rows: over-MRV, recovery-scaled volume,
    // modality substitution.
    if let Some(p) = ctx.profile {
        push_profile_rows(&mut out, r, p);
    }
    push_stall_row(&mut out, r);
    out
}

/// The week-level trigger rows: each in-force review deload with the trigger
/// that fired it, then the autoreg-032 threshold re-test note.
fn week_trigger_rows(r: &SessionReview, ctx: &ReviewCtx) -> Vec<AdjustmentView> {
    // `ctx.recs` is the in-force (unexpired) subsequence of this check-in's
    // deloads, in the same order: pair each with its trigger reason in order,
    // so two identical deloads from different triggers keep their own reason.
    let mut reasons = review_deloads_with_reasons(r);
    // No profile → unknown: say what the plan does to runs, conservatively.
    let runs = ctx.profile.is_none_or(|p| p.running_days_per_week > 0);
    let mut next = 0;
    let mut out: Vec<AdjustmentView> = ctx
        .recs
        .iter()
        .map(|rec| {
            let reason = reasons[next..]
                .iter()
                .position(|(cand, _)| cand == rec)
                .and_then(|k| {
                    let found = reasons[next + k].1.take();
                    next += k + 1;
                    found
                });
            review_rec_view(rec, reason.as_deref(), runs)
        })
        .collect();
    if let Some(weeks) = r.pace_at_hr_improved_weeks {
        let retest = autoreg::threshold_retest_due(true, weeks);
        if retest.value {
            let mut row = to_view_with(
                "Your pace at the same heart rate has improved for 2+ weeks. Re-test with a race or time trial to update your paces"
                    .to_string(),
                &retest,
            );
            // A progression cue, not a prescription adjustment: never headline it.
            row.neutral = true;
            out.push(row);
        }
    }
    out
}

/// Session-execution rows: the bar-speed verdict, the "today" set-count and
/// stop calls, the two-session cut hold and the interval pace cut.
fn push_execution_rows(out: &mut Vec<AdjustmentView>, r: &SessionReview, ctx: &ReviewCtx) {
    // autoreg-008/009 (VBT): reference-load velocity delta → daily-1RM verdict.
    if let Some(delta) = r.mcv_delta_m_s {
        let v = autoreg::vbt_daily_readiness(delta);
        // An IncreaseLoad verdict never displaces the prescription while any
        // suppression is active (neutral then), and expires with the
        // session-execution note after a week. Hold = bar speed inside the
        // reliability band, a graded "no change" that is always neutral.
        let increase_expired = matches!(v.value, autoreg::VbtReadiness::IncreaseLoad)
            && ctx.review_age.is_some_and(|a| a >= FEEDBACK_FRESH_DAYS);
        if !increase_expired {
            let (text, neutral) = match v.value {
                autoreg::VbtReadiness::IncreaseLoad => (
                    format!(
                        "Bar speed up more than {} m/s on the same weight: you're stronger today, raise working weights",
                        autoreg::VBT_RELIABILITY_M_S
                    ),
                    suppression_active(ctx.readiness_recs, ctx.train_blocked),
                ),
                autoreg::VbtReadiness::Hold => (
                    format!(
                        "Bar speed within ±{} m/s of normal: hold planned weights",
                        autoreg::VBT_RELIABILITY_M_S
                    ),
                    true,
                ),
                autoreg::VbtReadiness::ReduceLoad => (
                    format!(
                        "Bar speed down more than {} m/s on the same weight: you're weaker today, reduce working weights",
                        autoreg::VBT_RELIABILITY_M_S
                    ),
                    false,
                ),
            };
            let mut row = to_view_with(text, &v);
            row.neutral = neutral;
            out.push(row);
        }
    }

    // "Today" execution calls (add/drop a set, end the exercise, slow the
    // remaining reps) only mean something during that session: gone from the
    // day after (an undated or clockless review keeps them).
    let same_day = ctx.review_age.is_none_or(|a| a < 1);

    // autoreg-011/012: first-work-set outcome → add/drop/hold the set count.
    // Wellness-normal must satisfy BOTH the review's own low-recovery tally AND
    // the core's derived suppression (a downgrade from wellness z / RHR / HRV /
    // pain in this same view): otherwise "add a set today, wellness normal" can
    // render directly beneath the engine's own "easier session", contradicting it
    // and misapplying autoreg-011 (whose third condition is objectively false).
    if same_day && let (Some(met), Some(rpe_delta)) = (r.first_set_reps_met, r.first_set_rpe_delta)
    {
        let wellness_normal =
            !r.low_recovery && !suppression_active(ctx.readiness_recs, ctx.train_blocked);
        let a = autoreg::set_volume_action(met, rpe_delta, wellness_normal);
        // HoldPlanned = keep the planned set count: a graded "no change" verdict.
        // AddSet is only produced when `wellness_normal` holds (which implies no
        // active suppression), so it never coincides with a downgrade — never
        // neutral. A stale review still can't headline it over the prescription.
        let (text, neutral) = match a.value {
            autoreg::SetVolumeAction::AddSet => (
                "First set went well and you feel normal: add a set today",
                false,
            ),
            autoreg::SetVolumeAction::DropLastSet => (
                "First set short or harder than planned: drop the last planned set, then tap End exercise",
                false,
            ),
            autoreg::SetVolumeAction::HoldPlanned => {
                ("Do the planned sets: no change to the set count", true)
            }
        };
        let mut row = to_view_with(text.to_string(), &a);
        row.neutral = neutral;
        row.suggests_end_exercise = matches!(a.value, autoreg::SetVolumeAction::DropLastSet);
        out.push(row);
    }

    // autoreg-013 via the lift execution the review already carries: reserve
    // at/below target means the target effort is reached, cut remaining sets.
    // strength::rir_to_rpe supplies the RIR→RPE mapping.
    if same_day && let Some(l) = &r.lift {
        let rpe_actual = strength::rir_to_rpe(f64::from(l.rir_actual));
        let rpe_target = strength::rir_to_rpe(f64::from(l.rir_target));
        if autoreg::rpe_stop_reached(rpe_actual, rpe_target) && !l.reps_met {
            let mut row = to_view_with(
                "Target effort reached before the planned reps: stop here and tap End exercise"
                    .to_string(),
                &graded((), claims::AUTOREG_SETVOL_001),
            );
            row.suggests_end_exercise = true;
            out.push(row);
        }
    }

    // autoreg-014: two consecutive sessions needing set cuts → hold volume.
    if r.cut_last_two_sessions {
        out.push(to_view_with(
            "Sets cut in two sessions in a row on this lift: hold weekly volume, don't add sets"
                .to_string(),
            &graded((), claims::AUTOREG_SETVOL_001),
        ));
    }

    // autoreg-031: ≥2 interval reps over target RPE / HR cap → slow the rest.
    if same_day
        && let Some(n) = r.interval_reps_over_target
        && let Some(cut) = autoreg::interval_pace_autoreg(n)
    {
        out.push(to_view_with(
            format!(
                "2+ interval reps were harder than the target: slow the remaining reps ~{:.0}%",
                cut.value * 100.0
            ),
            &cut,
        ));
    }
}

/// Monitoring rows: the easy-pace HR cap, HRV reliability and suppression, the
/// wellness/resting-HR streak and the aerobic-decoupling band.
fn push_monitoring_rows(out: &mut Vec<AdjustmentView>, r: &SessionReview, ctx: &ReviewCtx) {
    // autoreg-033: the HR cap governs easy pace, not the watch pace.
    if let Some(can_hold) = r.can_hold_easy_pace_under_hr_cap {
        let slow = autoreg::slow_easy_pace_if_over_cap(can_hold);
        if slow.value {
            out.push(to_view_with(
                "Your easy pace pushes your heart rate over its easy-day limit. Slow down: on easy days, heart rate decides the pace"
                    .to_string(),
                &slow,
            ));
        }
    }

    // autoreg-050: ≥2 unreliable HRV readings in the last 3 → suspend gating.
    if let Some(n) = r.hrv_unreliable_last_three
        && autoreg::suspend_hrv_gating(n)
    {
        out.push(to_view_with(
            "2 of your last 3 HRV readings look unreliable. Until clean readings return, plans follow how you feel and perform".to_string(),
            &graded((), claims::AUTOREG_FALLBACK_001),
        ));
    }

    // autoreg-034: multi-day lnRMSSD suppression → recovery day / easy block.
    // The core-derived HRV streak is the source of truth; fall back to
    // the shell-supplied count only when no check-in history derived one.
    if let Some(days) = ctx.derived_hrv_streak.or(r.hrv_suppressed_days) {
        let rec = autoreg::hrv_suppressed_recovery_day(days);
        if rec.value {
            out.push(to_view_with(
                "HRV has been below your normal on most of the last 3+ days. Take a recovery day or a few easy days."
                    .to_string(),
                &rec,
            ));
        }
    }

    // autoreg-035: suppressed wellness + rising RHR → 1–3 easy days. Prefer the
    // core-derived wellness streak, falling back to the shell count (mirrors the
    // autoreg-034 HRV branch above).
    if let Some(days) = ctx.derived_wellness_streak.or(r.wellness_suppressed_days) {
        let easy = autoreg::wellness_rhr_multiday_easy(days, r.rhr_rising);
        if easy.value {
            out.push(to_view_with(
                "You've felt run down for 2+ days and your resting heart rate is rising: take 1–3 easy days or cross-train".to_string(),
                &easy,
            ));
        }
    }

    // RUN-DECOUPLE-001: Friel band verdict for a measured aerobic-decoupling
    // percentage, only in the valid context (cool, steady, sub-threshold,
    // long enough), same gate the feedback resolver applies.
    if let Some(d) = &r.decoupling
        && d.cool_steady_context
    {
        let band = load::decoupling_band(d.drift_pct);
        let text = match band.value {
            load::DecouplingBand::SoundBase => format!(
                "Heart-rate drift {:.1}% (under {}%): your endurance is solid at this pace",
                d.drift_pct,
                load::DECOUPLING_SOUND_PCT
            ),
            load::DecouplingBand::BuildBase => format!(
                "Heart-rate drift {:.1}% ({}–{}%): keep building endurance with easy runs for another 3–6 weeks",
                d.drift_pct,
                load::DECOUPLING_SOUND_PCT,
                load::DECOUPLING_HIGH_PCT
            ),
            load::DecouplingBand::Insufficient => format!(
                "Heart-rate drift {:.1}% (≥{}%): the pace was too hard for this distance. Slow down on runs this long until the drift drops",
                d.drift_pct,
                load::DECOUPLING_HIGH_PCT
            ),
        };
        let mut v = to_view_with(text, &band);
        // A "sound aerobic base" verdict is informational, not a prescription
        // change: never let it headline over the session.
        v.neutral = matches!(band.value, load::DecouplingBand::SoundBase);
        out.push(v);
    }
}

/// Hypertrophy rows: accumulated overreaching triggers and a set-to-set rep
/// drop.
fn push_hypertrophy_rows(out: &mut Vec<AdjustmentView>, r: &SessionReview) {
    // hypertrophy-035: ≥2 accumulated overreaching triggers → deload now.
    if let Some(n) = r.hypertrophy_deload_triggers {
        let d = hypertrophy::deload_indicated(n);
        if d.value {
            out.push(to_view_with(
                "2+ signs of doing too much: take the lighter week now rather than waiting for the scheduled one".to_string(),
                &d,
            ));
        }
    }

    // hypertrophy-039: >10% set-to-set rep drop → lengthen rest.
    if let Some(frac) = r.rep_drop_frac {
        let rest = hypertrophy::increase_rest_on_rep_drop(frac);
        if rest.value {
            out.push(to_view_with(
                "Reps fell more than 10% from one set to the next. Rest longer between sets to keep your reps up"
                    .to_string(),
                &rest,
            ));
        }
    }
}

/// Rows that need the profile: over-MRV, recovery-scaled volume and modality
/// substitution.
fn push_profile_rows(out: &mut Vec<AdjustmentView>, r: &SessionReview, p: &Profile) {
    // hypertrophy-009: weekly sets above MRV with regression or aching
    // joints. This review row is a generic per-muscle prompt (no specific
    // muscle in scope), so the empty muscle takes over_mrv_deload's generic
    // ~20-set anchor; a muscle-specific caller would pass the muscle name.
    let over = hypertrophy::over_mrv_deload("", p.weekly_sets, r.performance_down, r.joint_ache);
    if over.value {
        out.push(to_view_with(
            format!(
                "{} weekly sets with {}: more than you can recover from, so take a lighter week",
                p.weekly_sets,
                if r.joint_ache {
                    "aching joints"
                } else {
                    "regressing performance"
                }
            ),
            &over,
        ));
    }

    // hypertrophy-010/045: low recovery scales weekly sets to 70–80%.
    if r.low_recovery {
        let adj = hypertrophy::recovery_adjusted_volume(p.weekly_sets, true);
        out.push(to_view_with(
            format!(
                "You're not recovering well. Cut this week to {:.0}–{:.0} sets (70–80% of {}) and take fewer sets to failure",
                adj.value.0, adj.value.1, p.weekly_sets
            ),
            &adj,
        ));
    }

    // hybrid-018 / CAP-6: interference symptoms + running not mandatory →
    // substitute a low-impact modality. Running is treated as optional only
    // for a General (no-race) goal: a race goal makes running mandatory.
    let running_optional = p.goal_distance == GoalDistance::General;
    let sub = hybrid::substitute_modality(r.interference_symptoms, running_optional);
    if sub.value {
        out.push(to_view_with(
            "Running seems to be holding back your lifting, and you have no race planned: swap some runs for cycling or rowing".to_string(),
            &sub,
        ));
    }
}

/// Starting-Strength stall governance (individualization): 3 straight failed
/// sessions with adequate recovery → 10% single-lift deload; a repeat stall
/// after the re-ramp → move that lift to intermediate progression.
fn push_stall_row(out: &mut Vec<AdjustmentView>, r: &SessionReview) {
    if let Some(n) = r.stall_failed_sessions {
        let stall = individualization::novice_stall_action(
            n,
            r.stall_adequate_recovery,
            r.stalled_again_after_reramp,
        );
        if let Some(o) = stall.value {
            let text = if o.transition_to_intermediate {
                "Stalled again after building back up: move this lift to weekly progress steps"
                    .to_string()
            } else {
                format!(
                    "3 failed sessions in a row with good recovery: lower this lift's weight {:.0}% and build back up",
                    o.deload_frac * 100.0
                )
            };
            out.push(to_view_with(text, &stall));
        }
    }
}

/// Whether `r` is a "next time" load add for a lift that already carries an
/// in-session load add today (`in_session`): the lift shows one cue, the
/// in-session one, never two different percentages at once.
pub(in crate::app) fn next_time_add_superseded(
    r: &Recommended<Adjustment>,
    in_session: &[Recommended<Adjustment>],
) -> bool {
    let Adjustment::ForItem {
        exercise,
        adjustment,
    } = &r.value
    else {
        return false;
    };
    matches!(adjustment.base(), Adjustment::IncreaseLoadPct(_))
        && in_session.iter().any(|s| {
            matches!(&s.value, Adjustment::ForItem { exercise: e, adjustment: a }
                if matches!(a.base(), Adjustment::IncreaseLoadPct(_))
                    && crate::plan::same_movement(e, exercise))
        })
}

/// Whether `r` is an item-scoped call about a lift's REMAINING sets today (a
/// load cut or an effort cap) on an exercise `today` already shows finished:
/// nothing is left for it to change. A "next time" add stays.
pub(in crate::app) fn remaining_sets_call_spent(
    r: &Recommended<Adjustment>,
    today: &SessionPlanView,
) -> bool {
    let Adjustment::ForItem {
        exercise,
        adjustment,
    } = &r.value
    else {
        return false;
    };
    !matches!(adjustment.base(), Adjustment::IncreaseLoadPct(_))
        && today
            .items
            .iter()
            .any(|it| it.done && !it.is_run() && crate::plan::same_movement(&it.exercise, exercise))
}
