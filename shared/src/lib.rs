//! lollop's training core: the deterministic, evidence-cited engine every
//! shell renders. The rules below are referenced throughout this crate by
//! number ("HARD RULE 1" and so on); they are defined here and nowhere else.
//!
//! **HARD RULE 1 — the knowledge base is authoritative.** Nothing enters the
//! engine that the knowledge base does not state. No invented thresholds, no
//! extrapolated bands, no fabricated inputs: an unanswered item stays absent
//! rather than being filled with a plausible default, and where the KB gives no
//! number the engine states the gap instead of inventing one.
//!
//! **HARD RULE 2 — every recommendation carries its evidence.** A value that
//! advises the athlete travels as `Recommended<T>` with an `Evidence{grade,
//! citation}` and a `ConfidenceTag` (`evidence::graded`, keyed by claim id), so
//! a shell can always show how strongly a number is backed and which claim
//! produced it. When two claims could face a figure, the badge names the one
//! that actually set it. A claim graded `MarketingMyth` is hard-blocked: it can
//! never reach a prescription.
//!
//! **HARD RULE 3 — safety overrides goals.** The engine never programs through
//! injury, under-fuelling or a medical red flag. Safety gates run before and
//! above any goal logic, and the `schema::SafetyTier` ladder resolves competing
//! signals in a fixed order: medical referral > pain > illness > objective
//! performance > subjective multi-day > HRV trend > single-day marker. A
//! blocking tier blanks the prescription rather than softening it.
//!
//! **Determinism.** The core holds no clock, no randomness and no IO. Time
//! enters only as event data (`observed_at`, the shell's `SetToday` epoch-day
//! and UTC offset), so the same event log always replays to the same view.

pub mod app;
pub mod autoreg;
pub mod calisthenics;
pub mod evidence;
pub mod feedback;
pub mod ffi;
pub mod fit;
pub mod hybrid;
pub mod hypertrophy;
pub mod individualization;
pub mod load;
pub mod log;
pub mod plan;
pub mod running;
pub mod schema;
pub mod strength;

/// Seconds per day: the one deterministic day bucket every module shares.
pub(crate) const DAY_SEC: i64 = 86_400;

/// LOCAL epoch day for a UTC `observed_at`, shifted by the shell's
/// `today_utc_offset_sec`. The single day-bucketing convention shared by the plan
/// strip's day attribution, the weekly running report, the lift/provisional
/// audits, readiness derivation and compaction, so a set logged 23:30 local lands
/// on the SAME calendar day everywhere. Carries the same DST/travel limitation as
/// the plan strip: past entries are bucketed with TODAY's offset (the only offset
/// on the wire). Saturating, so a corrupt wire stamp cannot overflow.
pub(crate) fn local_day(observed_at: i64, offset: i64) -> i64 {
    observed_at.saturating_add(offset).div_euclid(DAY_SEC)
}

pub use app::{
    ActivityKind, ActivityResultView, AdjustmentView, ApreInputView, BaselineStatusView,
    CheckinEchoView, CsEffortIn, Decouple, E1rmRangeView, Effect, Engine, EntryKind, Event,
    FeedbackView, GradeDefView, GuidanceView, HrMaxView, HrZoneInputView, HypertrophyInputView,
    IntervalExec, IntervalVerdictView, LiftExec, LiftResultView, Model, PrescriptionView, Profile,
    ProgramSummaryView, ProteinFigureView, ProteinInputView, RacePredictionView,
    ReadinessSignalView, ResistanceModality, RunResultView, RunSplitView, SessionPlanView,
    SessionReview, SignalGroupView, SplitVerdictView, TodayHeadlineView, TrainingLoadView,
    ViewModel, WhyView, YouthActivityView,
};
pub use crux_core::Core;
pub use log::compact_event_log;
