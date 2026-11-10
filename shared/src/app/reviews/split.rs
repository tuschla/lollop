//! The merged review the row builders read (weekly check-in in force plus
//! the current session review), and review age and expiry. The split of one
//! payload into its two parts lives with the payload
//! ([`crate::event::SessionReview`]).

use crate::day::{is_undated, local_day};
use crate::event::SessionReview;
use crate::schema::Adjustment;

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
        m.take_session_fields(s);
        m.observed_at = s.observed_at.max(m.observed_at);
    }
    Some(m)
}

/// Days a session-execution note (manual or derived) stays current: transient
/// coaching on ONE session expires after a week so a stale verdict stops
/// leading. Safety signals never expire here.
pub(super) const FEEDBACK_FRESH_DAYS: i64 = 7;

/// Whether `day` is at most `days` before `today`; true without a clock or a
/// date.
pub(super) fn fresh_within(today: Option<i64>, day: Option<i64>, days: i64) -> bool {
    match (today, day) {
        (Some(t), Some(d)) => t.saturating_sub(d) <= days,
        _ => true,
    }
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
