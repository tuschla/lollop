//! Post-session coaching: the session-review / weekly-check-in split, review
//! age/expiry, review-driven deloads and adjustment rows, session execution
//! derived from logged sets and runs, mastery (PR) detection, the feedback
//! card, the "session done" summary, the trend row and the "last time" line on
//! the next session.

use super::*;

mod exec;
// The card child shadows the glob-imported `crate::feedback` engine in this
// module; children that call the engine import it by path.
mod feedback;
mod last_time;
mod mastery;
mod rows;
mod split;
mod trend;

pub(super) use exec::*;
pub(super) use feedback::*;
pub(super) use last_time::*;
pub(super) use mastery::*;
pub(super) use rows::*;
pub(super) use split::*;
pub(super) use trend::*;
// `log.rs` compaction reaches these through `crate::app`.
pub(crate) use split::{submit_review_sets_session, submit_review_sets_weekly};

/// Days a session-execution note (manual or derived) stays current: transient
/// coaching on ONE session expires after a week so a stale verdict stops
/// leading. Safety signals never expire here.
pub(super) const FEEDBACK_FRESH_DAYS: i64 = 7;

/// The e1RM change that clears day-to-day noise: ±5 % (autoreg-006/007, Helms
/// 2018). A new best counts only beyond it; the trend row bands on it too.
const E1RM_NOISE_FRAC: f64 = 0.05;
