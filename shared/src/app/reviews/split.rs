//! Session review vs weekly check-in: one review payload splits into its
//! per-session part and its weekly part, so a routine session review never
//! overwrites the weekly check-in; plus review age and expiry.

use super::*;

/// Copy every PER-SESSION field of `src` into `dst`: the single list of
/// session fields (one session's execution context). Every field NOT listed
/// here belongs to the weekly check-in (week-level counts, clinical screens),
/// so a new review field defaults to the protective, persistent store.
fn take_session_fields(dst: &mut SessionReview, src: &SessionReview) {
    dst.single_session_spike_frac = src.single_session_spike_frac;
    dst.lift = src.lift.clone();
    dst.interval = src.interval.clone();
    dst.decoupling = src.decoupling.clone();
    dst.easy_frac_time_above_vt1 = src.easy_frac_time_above_vt1;
    dst.positive_split_pct = src.positive_split_pct;
    dst.mcv_delta_m_s = src.mcv_delta_m_s;
    dst.first_set_reps_met = src.first_set_reps_met;
    dst.first_set_rpe_delta = src.first_set_rpe_delta;
    dst.rep_drop_frac = src.rep_drop_frac;
    dst.interval_reps_over_target = src.interval_reps_over_target;
    dst.can_hold_easy_pace_under_hr_cap = src.can_hold_easy_pace_under_hr_cap;
    dst.hrv_unreliable_last_three = src.hrv_unreliable_last_three;
    dst.planned_hard = src.planned_hard;
    dst.bad_day = src.bad_day;
}

fn blank_review(observed_at: i64) -> SessionReview {
    SessionReview {
        observed_at,
        ..SessionReview::default()
    }
}

/// The per-session part of a review payload (weekly fields at their defaults).
pub(in crate::app) fn session_part(r: &SessionReview) -> SessionReview {
    let mut s = blank_review(r.observed_at);
    take_session_fields(&mut s, r);
    s
}

/// The weekly part of a review payload (session fields at their defaults).
pub(in crate::app) fn weekly_part(r: &SessionReview) -> SessionReview {
    let mut w = r.clone();
    take_session_fields(&mut w, &SessionReview::default());
    w
}

fn has_weekly_fields(r: &SessionReview) -> bool {
    weekly_part(r) != blank_review(r.observed_at)
}

fn has_session_fields(r: &SessionReview) -> bool {
    session_part(r) != blank_review(r.observed_at)
}

/// Whether a `SubmitReview` payload replaces the session review: it carries
/// execution context, or carries nothing weekly (a bare review is a session
/// review). Shared with log compaction, which may drop a `ClearReview` only
/// when a later payload replaces the session review anyway.
pub(crate) fn submit_review_sets_session(r: &SessionReview) -> bool {
    has_session_fields(r) || !has_weekly_fields(r)
}

/// Whether a `SubmitReview` payload replaces the weekly check-in: it carries
/// any weekly/clinical field (the legacy weekly sheet sends its week counts
/// even when they are 0, so an all-"no" weekly answer still lands here).
pub(crate) fn submit_review_sets_weekly(r: &SessionReview) -> bool {
    has_weekly_fields(r)
}

/// The weekly check-in and the session review merged into one payload, the
/// shape the row builders read: weekly fields from the check-in in force,
/// session fields from the current session review.
pub(in crate::app) fn merged_review(
    weekly: Option<&SessionReview>,
    session: Option<&SessionReview>,
) -> Option<SessionReview> {
    if weekly.is_none() && session.is_none() {
        return None;
    }
    let mut m = weekly.cloned().unwrap_or_default();
    if let Some(s) = session {
        take_session_fields(&mut m, s);
        m.observed_at = s.observed_at.max(m.observed_at);
    }
    Some(m)
}

/// The effective overtraining-sign count: the number of listed signs ticked
/// when the check-in carries them, else the bare count (legacy wire).
pub(in crate::app) fn overtraining_count(r: &SessionReview) -> u8 {
    r.overtraining_signs
        .map_or(r.overtraining_signal_count, |s| s.count())
}

/// Whether a wire `observed_at` stamp carries no date: legacy lines decode it
/// as 0, and a non-positive stamp names no real day. The one predicate every
/// review-age, review-window and retraction check shares, so a stamp is never
/// dated for one of them and undated for another.
pub(in crate::app) fn is_undated(observed_at: i64) -> bool {
    observed_at <= 0
}

/// Days since the review was recorded, in local time; `None` when the review is
/// undated ([`is_undated`], so it has no age to expire against) or when the
/// shell has supplied no clock. Consumed by the review-expiry checks.
pub(in crate::app) fn review_age_days(
    r: &SessionReview,
    today_epoch_day: Option<i64>,
    offset: i64,
) -> Option<i64> {
    if is_undated(r.observed_at) {
        return None;
    }
    today_epoch_day.map(|t| t.saturating_sub(local_day(r.observed_at, offset)))
}

/// Whether a week-scoped review adjustment has aged out: a `Deload { weeks }`
/// expires after `7 × weeks` days; a single-day call (the autoreg-028
/// single-day HRV `DowngradeSession`) after its own day. Defer and every other
/// adjustment never expire here — protective rows (Defer/Stop/RestDay and the
/// safety-flag feedback) are exempt by design. An undated or clockless review
/// (`age == None`) never expires.
pub(in crate::app) fn review_adjustment_expired(adj: &Adjustment, age: Option<i64>) -> bool {
    match (adj, age) {
        (Adjustment::Deload { weeks, .. }, Some(a)) => a >= 7 * i64::from(*weeks),
        (Adjustment::DowngradeSession, Some(a)) => a >= 1,
        _ => false,
    }
}
