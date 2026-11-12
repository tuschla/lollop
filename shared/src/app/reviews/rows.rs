//! Review-driven rows: the weekly check-in's deloads and deferrals, the session
//! review's execution, monitoring and volume rows, and the filters that drop
//! a row today's session already settles.

use crate::app::evidence_views::{describe, to_view, to_view_with};
use crate::app::plan_views::PlanRow;
use crate::app::readiness::{RankedRow, suppression_active};
use crate::app::review_deloads::RECOVERY_WEEK_LOAD_CUT_PCT;
use crate::app::reviews::split::FEEDBACK_FRESH_DAYS;
use crate::app::view_types::AdjustmentView;
use crate::event::SessionReview;
use crate::evidence::claims;
use crate::profile::Profile;
use crate::running::GoalDistance;
use crate::schema::{Adjustment, AdjustmentCall, AdjustmentKind, DeloadTrigger, Recommended};
use crate::{autoreg, hybrid, hypertrophy, individualization, load, strength};

/// Readiness/profile context a [`review_views`] pass needs alongside the review:
/// the review's own deloads, the readiness adjustments in scope (for suppression),
/// whether training is blocked, the session review's age, the profile, and the
/// core-derived HRV / wellness suppression streaks.
pub(in crate::app) struct ReviewCtx<'a> {
    pub(in crate::app) recs: &'a [AdjustmentCall],
    pub(in crate::app) readiness_recs: &'a [AdjustmentCall],
    pub(in crate::app) train_blocked: bool,
    pub(in crate::app) review_age: Option<i64>,
    pub(in crate::app) profile: Option<&'a Profile>,
    pub(in crate::app) derived_hrv_streak: Option<u8>,
    pub(in crate::app) derived_wellness_streak: Option<u8>,
}

/// One review call as a row; a weekly deload says which trigger fired it.
fn review_rec_row(r: &AdjustmentCall, runs: bool) -> RankedRow {
    let view = match r.kind() {
        AdjustmentKind::WeeklyDeload(trigger) => {
            to_view_with(weekly_deload_summary(r, trigger, runs), r)
        }
        _ => to_view(r),
    };
    RankedRow {
        view,
        tier: r.tier(),
        neutral: false,
    }
}

/// The review's rows in order.
struct Rows(Vec<RankedRow>);

impl Rows {
    /// A row that may headline.
    fn push(&mut self, view: AdjustmentView) {
        self.0.push(RankedRow::plain(view));
    }

    /// A row that headlines only when it is not `neutral`.
    fn push_verdict(&mut self, view: AdjustmentView, neutral: bool) {
        self.0.push(RankedRow {
            neutral,
            ..RankedRow::plain(view)
        });
    }
}

/// A weekly deload's call, ending on the trigger that fired it. The
/// overtraining recovery week reads in plain words (it is the action behind
/// the recovery card, so one trigger shows one card plus the lighter plan,
/// never three restatements); its running clause appears only for a profile
/// that runs.
fn weekly_deload_summary(r: &AdjustmentCall, trigger: DeloadTrigger, runs: bool) -> String {
    let why = match trigger {
        DeloadTrigger::RecoveryWeek => {
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
            return format!(
                "Lighter week: {what}. Signs you're not recovering have lasted a week or more."
            );
        }
        DeloadTrigger::RpeLoadGap => format!(
            "because {}+ sessions this week needed weights {}% or more below plan to reach the planned effort",
            autoreg::RPE_LOAD_GAP_DELOAD_SESSIONS,
            autoreg::RPE_LOAD_GAP_DELOAD_PCT
        ),
        DeloadTrigger::VelocityDrop => format!(
            "because your bar speed on the same weight dropped more than {} m/s this week",
            autoreg::VBT_WEEKLY_DROP_DELOAD_M_S
        ),
        DeloadTrigger::FailedSessions => format!(
            "because {}+ hard sessions weren't finished this week",
            autoreg::FAILED_KEY_SESSIONS_DELOAD
        ),
        DeloadTrigger::MrvSigns => "because of signs you're doing more than you can recover from (aching joints, stalling, poor sleep, low motivation)".into(),
        DeloadTrigger::RpeCreep => format!(
            "because sessions felt harder at the same weights while your check-ins stayed low for {}+ days",
            autoreg::RPE_CREEP_WELLNESS_LOW_DAYS
        ),
    };
    format!("{}, {why}", describe(&r.value))
}

/// Flatten the review's week-level triggers for the shell: the deload/deferral
/// adjustments plus the autoreg-032 threshold re-test note (a progression cue,
/// not an `Adjustment`, so it is rendered with its own summary).
pub(in crate::app) fn review_views(r: &SessionReview, ctx: ReviewCtx) -> Vec<RankedRow> {
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
    out.0
}

/// The week-level trigger rows: each in-force review deload with the trigger
/// that fired it, then the autoreg-032 threshold re-test note.
fn week_trigger_rows(r: &SessionReview, ctx: &ReviewCtx) -> Rows {
    // No profile → unknown: say what the plan does to runs, conservatively.
    let runs = ctx
        .profile
        .is_none_or(|p| crate::profile::PlanProfile::new(p).runs());
    let mut out = Rows(
        ctx.recs
            .iter()
            .map(|rec| review_rec_row(rec, runs))
            .collect(),
    );
    if let Some(weeks) = r.pace_at_hr_improved_weeks {
        let retest = autoreg::threshold_retest_due(weeks);
        if retest.value {
            let row = to_view_with(
                format!(
                    "Your pace at the same heart rate has improved for {}+ weeks. Re-test with a race or time trial to update your paces",
                    autoreg::PACE_RETEST_WEEKS
                ),
                &retest,
            );
            // A progression cue, not a prescription adjustment: never headline it.
            out.push_verdict(row, true);
        }
    }
    out
}

/// Session-execution rows: the bar-speed verdict, the "today" set-count and
/// stop calls, the two-session cut hold and the interval pace cut.
fn push_execution_rows(out: &mut Rows, r: &SessionReview, ctx: &ReviewCtx) {
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
                        "Bar speed up more than {} m/s on the same weight: raise working weights",
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
                        "Bar speed down more than {} m/s on the same weight: reduce working weights",
                        autoreg::VBT_RELIABILITY_M_S
                    ),
                    false,
                ),
            };
            out.push_verdict(to_view_with(text, &v), neutral);
        }
    }

    // "Today" execution calls (add/drop a set, end the exercise, slow the
    // remaining reps) only mean something during that session: gone from the
    // day after (an undated or clockless review keeps them).
    let same_day = ctx.review_age.is_none_or(|a| a < 1);

    // autoreg-011/012: first-work-set outcome → add/drop/hold the set count;
    // wellness is normal only with no low-recovery tally and no derived downgrade.
    if same_day && let (Some(met), Some(rpe_delta)) = (r.first_set_reps_met, r.first_set_rpe_delta)
    {
        let wellness_normal =
            !r.low_recovery && !suppression_active(ctx.readiness_recs, ctx.train_blocked);
        let a = autoreg::set_volume_action(met, rpe_delta, wellness_normal);
        // HoldPlanned keeps the set count; AddSet needs `wellness_normal`, so never neutral.
        let (text, neutral) = match a.value {
            autoreg::SetVolumeAction::AddSet => (
                "First set on target and check-in normal: add a set today",
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
        row.suggests_end_exercise = matches!(a.value, autoreg::SetVolumeAction::DropLastSet);
        out.push_verdict(row, neutral);
    }

    // autoreg-013 via the lift execution the review already carries: reserve
    // at/below target means the target effort is reached, cut remaining sets.
    if same_day && let Some(l) = &r.lift {
        let rpe_actual = strength::rir_to_rpe(f64::from(l.rir_actual));
        let rpe_target = strength::rir_to_rpe(f64::from(l.rir_target));
        if rpe_actual >= rpe_target && !l.reps_met {
            let mut row = to_view_with(
                "Target effort reached before the planned reps: stop here and tap End exercise"
                    .to_string(),
                &Recommended::new((), claims::AUTOREG_SETVOL_001),
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
            &Recommended::new((), claims::AUTOREG_SETVOL_001),
        ));
    }

    // autoreg-031: ≥2 interval reps over target RPE / HR cap → slow the rest.
    if same_day
        && let Some(n) = r.interval_reps_over_target
        && let Some(cut) = autoreg::interval_pace_autoreg(n)
    {
        out.push(to_view_with(
            format!(
                "{}+ interval reps were harder than the target: slow the remaining reps ~{:.0}%",
                autoreg::INTERVAL_REPS_OVER_TARGET_CUT,
                cut.value * 100.0
            ),
            &cut,
        ));
    }
}

/// Monitoring rows: the easy-pace HR cap, HRV reliability and suppression, the
/// wellness/resting-HR streak and the aerobic-decoupling band.
fn push_monitoring_rows(out: &mut Rows, r: &SessionReview, ctx: &ReviewCtx) {
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
            format!(
                "{} of your last {} HRV readings look unreliable. Until clean readings return, plans follow how you feel and perform",
                autoreg::HRV_UNRELIABLE_SUSPEND,
                autoreg::HRV_QC_LOOKBACK
            ),
            &Recommended::new((), claims::AUTOREG_FALLBACK_001),
        ));
    }

    // autoreg-034: multi-day lnRMSSD suppression → recovery day / easy block.
    // The core-derived HRV streak is the source of truth; fall back to
    // the shell-supplied count only when no check-in history derived one.
    if let Some(days) = ctx.derived_hrv_streak.or(r.hrv_suppressed_days) {
        let rec = autoreg::hrv_suppressed_recovery_day(days);
        if rec.value {
            out.push(to_view_with(
                format!(
                    "HRV has been below your normal on most of the last {}+ days. Take a recovery day or a few easy days.",
                    autoreg::HRV_ESCALATE_STREAK
                ),
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
                format!(
                    "You've felt run down for {}+ days and your resting heart rate is rising: take 1–3 easy days or cross-train",
                    autoreg::WELLNESS_RHR_EASY_DAYS
                ),
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
                "Heart-rate drift {:.1}% (under {}%): heart rate stayed steady at this pace",
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
        // A "sound aerobic base" verdict is informational, not a prescription
        // change: never let it headline over the session.
        let neutral = matches!(band.value, load::DecouplingBand::SoundBase);
        out.push_verdict(to_view_with(text, &band), neutral);
    }
}

/// Hypertrophy rows: accumulated overreaching triggers and a set-to-set rep
/// drop.
fn push_hypertrophy_rows(out: &mut Rows, r: &SessionReview) {
    // hypertrophy-035: ≥2 accumulated overreaching triggers → deload now.
    if let Some(n) = r.hypertrophy_deload_triggers {
        let d = hypertrophy::deload_indicated(n);
        if d.value {
            out.push(to_view_with(
                format!(
                    "{}+ signs of doing too much: take the lighter week now",
                    hypertrophy::DELOAD_TRIGGER_MIN
                ),
                &d,
            ));
        }
    }

    // hypertrophy-039: >10% set-to-set rep drop → lengthen rest.
    if let Some(frac) = r.rep_drop_frac {
        let rest = hypertrophy::increase_rest_on_rep_drop(frac);
        if rest.value {
            out.push(to_view_with(
                format!(
                    "Reps fell more than {:.0}% from one set to the next. Rest longer between sets to keep your reps up",
                    hypertrophy::REP_DROP_REST_FRAC * 100.0
                ),
                &rest,
            ));
        }
    }
}

/// Rows that need the profile: over-MRV, recovery-scaled volume and modality
/// substitution.
fn push_profile_rows(out: &mut Rows, r: &SessionReview, p: &Profile) {
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
                "You're not recovering well. Cut this week to {:.0}–{:.0} sets ({:.0}–{:.0}% of {}) and take fewer sets to failure",
                adj.value.0,
                adj.value.1,
                hypertrophy::LOW_RECOVERY_VOLUME_FRAC.0 * 100.0,
                hypertrophy::LOW_RECOVERY_VOLUME_FRAC.1 * 100.0,
                p.weekly_sets
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
fn push_stall_row(out: &mut Rows, r: &SessionReview) {
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
    r: &AdjustmentCall,
    in_session: &[AdjustmentCall],
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
                    && crate::exercise::same_movement(e, exercise))
        })
}

/// Whether `r` is an item-scoped call about a lift's REMAINING sets today (a
/// load cut or an effort cap) on an exercise `today` already shows finished:
/// nothing is left for it to change. A "next time" add stays.
pub(in crate::app) fn remaining_sets_call_spent(r: &AdjustmentCall, today: &PlanRow) -> bool {
    let Adjustment::ForItem {
        exercise,
        adjustment,
    } = &r.value
    else {
        return false;
    };
    !matches!(adjustment.base(), Adjustment::IncreaseLoadPct(_))
        && today.items.iter().any(|it| {
            it.done && it.is_lift() && crate::exercise::same_movement(&it.exercise, exercise)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_rows_state_the_engine_thresholds() {
        let r = SessionReview {
            hrv_unreliable_last_three: Some(2),
            hrv_suppressed_days: Some(3),
            wellness_suppressed_days: Some(2),
            rhr_rising: true,
            hypertrophy_deload_triggers: Some(2),
            rep_drop_frac: Some(0.2),
            low_recovery: true,
            ..SessionReview::default()
        };
        let p = Profile {
            weekly_sets: 20,
            ..crate::profile::test_support::base_profile()
        };
        let ctx = ReviewCtx {
            recs: &[],
            readiness_recs: &[],
            train_blocked: false,
            review_age: None,
            profile: Some(&p),
            derived_hrv_streak: None,
            derived_wellness_streak: None,
        };
        let rows = review_views(&r, ctx);
        let got: Vec<&str> = rows.iter().map(|v| v.view.summary.as_str()).collect();
        for want in [
            "2 of your last 3 HRV readings look unreliable. Until clean readings return, plans follow how you feel and perform",
            "HRV has been below your normal on most of the last 3+ days. Take a recovery day or a few easy days.",
            "You've felt run down for 2+ days and your resting heart rate is rising: take 1–3 easy days or cross-train",
            "2+ signs of doing too much: take the lighter week now",
            "Reps fell more than 10% from one set to the next. Rest longer between sets to keep your reps up",
            "You're not recovering well. Cut this week to 14–16 sets (70–80% of 20) and take fewer sets to failure",
        ] {
            assert!(got.contains(&want), "{want:?} missing from {got:#?}");
        }
    }

    /// A bar-speed increase stays out of the headline while a readiness
    /// downgrade is active (autoreg-044), and is a candidate once it is not.
    #[test]
    fn a_bar_speed_increase_is_neutral_under_a_downgrade() {
        let r = SessionReview {
            mcv_delta_m_s: Some(0.10),
            ..SessionReview::default()
        };
        let downgrade =
            [AdjustmentKind::HrvLower.call(Adjustment::DowngradeSession, claims::HRV_001)];
        let increase_row = |readiness_recs: &[AdjustmentCall]| {
            review_views(
                &r,
                ReviewCtx {
                    recs: &[],
                    readiness_recs,
                    train_blocked: false,
                    review_age: None,
                    profile: None,
                    derived_hrv_streak: None,
                    derived_wellness_streak: None,
                },
            )
            .into_iter()
            .find(|row| row.view.summary.contains("raise working weights"))
            .expect("the increase row renders")
        };
        assert!(increase_row(&downgrade).neutral);
        assert!(!increase_row(&[]).neutral);
    }
}
