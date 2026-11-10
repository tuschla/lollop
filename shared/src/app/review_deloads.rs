//! The week-level calls a weekly check-in carries (deloads, deferrals and
//! referrals from accumulated weekly fatigue counts and red flags), each kinded
//! by the trigger that fired it, and the check-in's RED-S signal test.

use crate::autoreg;
use crate::event::SessionReview;
use crate::evidence::claims;
use crate::schema::{Adjustment, AdjustmentCall, AdjustmentKind, DeloadTrigger};

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
pub(in crate::app) fn review_deloads(r: &SessionReview) -> Vec<AdjustmentCall> {
    let mut out = Vec::new();
    out.extend(
        r.rpe_load_gap_sessions
            .and_then(autoreg::deload_from_rpe_load_gap),
    );
    out.extend(
        r.weekly_velocity_drop_m_s
            .and_then(autoreg::deload_from_velocity_drop),
    );
    out.extend(
        r.failed_key_sessions
            .and_then(autoreg::deload_from_failed_sessions),
    );
    // autoreg-025: the at/above-MRV sign cluster (qualitative) → deload.
    out.extend(autoreg::mrv_signs_deload(r.mrv_sign_cluster));
    // autoreg-024: RPE creep +1 at the same loads AND wellness z ≤ −1 for ≥3
    // days → the standard 1-week deload.
    out.extend(
        r.wellness_z_low_days.and_then(|days| {
            autoreg::deload_from_rpe_creep_and_wellness(r.rpe_creep_plus_one, days)
        }),
    );
    // autoreg-028 second trigger: a single-day lnRMSSD z < −1 with a ≥2-day
    // downtrend → downgrade the session even before the 7-day average clears
    // the SWC band.
    if let (Some(z), Some(days)) = (r.hrv_single_day_z, r.hrv_downtrend_days) {
        out.extend(autoreg::hrv_single_day_downgrade(z, days));
    }
    // ≥2 overtraining signs lasting a week or more → an easier week now
    // (feedback-036 "consider an easy week"; HYB-DELOAD-001 / Meeusen 2013:
    // deload when ≥2 overreaching red flags persist; running-034's
    // unscheduled down week). The same predicate as the recovery-concern card,
    // so the card and the lighter plan always appear together. Shape: the
    // standard 1-week deload, about half the sets and loads up to 10 % lower
    // (STR-DELOAD-VOL-001).
    if r.safety.overtraining_sustained() {
        out.push(
            AdjustmentKind::WeeklyDeload(DeloadTrigger::RecoveryWeek).call(
                Adjustment::Deload {
                    volume_reduction_pct: RECOVERY_WEEK_VOLUME_CUT_PCT,
                    load_reduction_pct: RECOVERY_WEEK_LOAD_CUT_PCT,
                    weeks: 1,
                },
                claims::HYB_DELOAD_001,
            ),
        );
    }
    // Overtraining escalations (File 08 safety-042 > File 06 autoreg-042): a
    // ≥2-week unexplained decline that survived a deload is the Strong
    // rest-and-defer rule; otherwise the ≥2-domain NFOR cluster mandates a
    // recovery block + professional referral. At most one fires: the
    // stronger-graded rule wins when both conditions hold.
    let decline_weeks = r.decline_weeks.unwrap_or(0);
    let domains = r.suppressed_wellness_domains.unwrap_or(0);
    out.extend(
        autoreg::unexplained_decline_rest_defer(decline_weeks, domains >= 1, r.despite_deload)
            .or_else(|| autoreg::nfor_cluster_defer(decline_weeks, domains)),
    );
    // Weekly red-flag answers defer exactly like the same signs entered as
    // readiness signals: bone-stress signs stop impact loading with an urgent
    // referral (safety-040); a positive RED-S screen or compulsive exercise
    // (safety-049's signal list; ready-upd-005 "the existing RED-S referral
    // route") reduces training stress and defers to a professional. Normal
    // load progression never runs under either (HARD RULE 3).
    if r.safety.bone_pain_red_flag {
        out.push(autoreg::bone_stress_referral());
    }
    if weekly_reds_signal(r) {
        out.push(autoreg::reds_referral());
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
