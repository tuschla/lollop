//! lollo's training core: the deterministic, evidence-cited engine every
//! shell renders. The rules below are referenced throughout this crate by
//! number ("HARD RULE 1" and so on); `planning/CLAUDE.md` holds the full list.
//!
//! HARD RULE 1: the knowledge base is authoritative. Nothing enters the
//! engine that the knowledge base does not state. An unanswered item stays
//! absent instead of being filled with a plausible default, and where the KB
//! gives no number the engine states the gap instead of inventing one.
//!
//! HARD RULE 2: every recommendation carries its evidence. A value that
//! advises the athlete travels as `Recommended<T>` tagged with its `ClaimId`
//! (`Recommended::new`), whose registry entry holds the grade and citation, so
//! a shell can always show how strongly a number is backed and which claim
//! produced it. When two claims could face a figure, the badge names the one
//! that set it. A claim graded `MarketingMyth` is hard-blocked: it can never
//! reach a prescription.
//!
//! HARD RULE 3: safety overrides goals. The engine never programs through
//! injury, under-fueling or a medical red flag. Safety gates run before any
//! goal logic, and `schema::SafetyTier` orders competing signals. A blocking
//! tier blanks the prescription; it does not soften it.
//!
//! Determinism: the core holds no clock, no randomness and no IO. Time
//! enters only as event data (`observed_at`, the shell's `SetToday` epoch-day
//! and UTC offset), so the same event log always replays to the same view.

pub mod app;
pub mod autoreg;
pub mod calisthenics;
pub(crate) mod day;
pub mod envelope;
pub mod event;
pub mod evidence;
pub mod exercise;
pub mod feedback;
pub mod ffi;
pub mod fit;
pub mod gps;
pub mod hybrid;
pub mod hypertrophy;
pub mod import;
pub mod individualization;
pub mod load;
pub mod log;
pub(crate) mod memo;
pub(crate) mod names;
pub mod plan;
pub(crate) mod plates;
pub mod profile;
pub mod running;
pub(crate) mod sanitize;
pub mod schema;
pub mod strength;
#[cfg(test)]
pub(crate) mod test_support;
pub mod wire;

pub use app::{Engine, Model};
pub use event::Event;
