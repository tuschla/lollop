//! Post-session coaching: the session-review / weekly-check-in split, review
//! age/expiry, review-driven deloads and adjustment rows, session execution
//! derived from logged sets and runs, mastery (PR) detection, the feedback
//! card, the "session done" summary, the trend row and the "last time" line on
//! the next session.

mod exec;
mod last_time;
mod mastery;
mod rows;
mod session_feedback;
mod split;
mod trend;

pub(super) use exec::{DerivedExec, build_session_done, derive_session_exec};
pub(super) use last_time::annotate_last_time;
pub(super) use mastery::detect_mastery;
pub(super) use rows::{
    ReviewCtx, next_time_add_superseded, remaining_sets_call_spent, review_views,
};
pub(super) use session_feedback::{
    FeedbackCtx, build_feedback, feedback_track_split, latest_run_spike, link_win_advice,
};
pub(super) use split::{merged_review, review_adjustment_expired, review_age_days};
pub(super) use trend::build_trend;
