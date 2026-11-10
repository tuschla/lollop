//! The week-level calls a weekly check-in carries (deloads, deferrals and
//! referrals from accumulated weekly fatigue counts and red flags), each with
//! the trigger that fired it, and the check-in's RED-S signal test.

use std::borrow::Cow;

use crate::app::model::SessionReview;
use crate::autoreg;
use crate::evidence::{ClaimId, claims};
use crate::schema::{Adjustment, Recommended};

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
pub(super) fn review_deloads_with_reasons(
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
                    "because {}+ sessions this week needed weights {}% or more below plan to reach the planned effort",
                    autoreg::RPE_LOAD_GAP_DELOAD_SESSIONS,
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
            Some(
                format!(
                    "because {}+ hard sessions weren't finished this week",
                    autoreg::FAILED_KEY_SESSIONS_DELOAD
                )
                .into(),
            ),
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
            Some(
                format!(
                    "because sessions felt harder at the same weights while your check-ins stayed low for {}+ days",
                    autoreg::RPE_CREEP_WELLNESS_LOW_DAYS
                )
                .into(),
            ),
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
    if r.safety.overtraining_sustained() {
        out.push((
            Recommended::new(
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
    if r.safety.bone_pain_red_flag {
        out.push((autoreg::bone_stress_referral(), None));
    }
    if weekly_reds_signal(r) {
        out.push((autoreg::reds_referral(), None));
    }
    out
}

/// A weekly check-in carries a RED-S signal: a positive RED-S screen or
/// compulsive exercise (safety-049's signal list). One test for the referral
/// row above and the app's RED-S-dependent refusals (safety-022 deficit,
/// calculators), so no channel can defer as RED-S without them.
pub(in crate::app) fn weekly_reds_signal(r: &SessionReview) -> bool {
    r.safety.reds_signal_reported || r.safety.compulsive_flag
}
