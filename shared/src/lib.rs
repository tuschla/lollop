//! lollop's training core: the deterministic, evidence-cited engine every
//! shell renders. The rules below are referenced throughout this crate by
//! number ("HARD RULE 1" and so on); `planning/CLAUDE.md` holds the full list.
//!
//! **HARD RULE 1 — the knowledge base is authoritative.** Nothing enters the
//! engine that the knowledge base does not state. No invented thresholds, no
//! extrapolated bands, no fabricated inputs: an unanswered item stays absent
//! rather than being filled with a plausible default, and where the KB gives no
//! number the engine states the gap instead of inventing one.
//!
//! **HARD RULE 2 — every recommendation carries its evidence.** A value that
//! advises the athlete travels as `Recommended<T>` tagged with its `ClaimId`
//! (`evidence::graded`), whose registry entry holds the grade and citation, so
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
#[cfg(any(target_os = "android", feature = "jni-host", test))]
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

/// LOCAL epoch day for a UTC `observed_at` shifted by `offset` seconds. The one
/// day-bucketing convention the app views share, so a set logged 23:30 local
/// lands on the same calendar day everywhere. Saturating, so a corrupt wire
/// stamp cannot overflow.
pub(crate) fn local_day(observed_at: i64, offset: i64) -> i64 {
    observed_at.saturating_add(offset).div_euclid(DAY_SEC)
}

/// Whether a wire `observed_at` stamp carries no date: legacy lines decode it
/// as 0, and a non-positive stamp names no real day. The one predicate every
/// dated-row filter shares, so a stamp is never dated for one check and
/// undated for another.
pub(crate) fn is_undated(observed_at: i64) -> bool {
    observed_at <= 0
}

/// Convert a count of days since the unix epoch (1970-01-01) to a civil
/// `(year, month, day)`. Howard Hinnant, "chrono-Compatible Low-Level Date
/// Algorithms" (public domain).
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub use app::{
    ActivityKind, ActivityResultView, AdjustmentView, BaselineStatusView, Decouple, E1rmRangeView,
    Effect, Engine, EntryKind, Event, FeedbackView, GradeDefView, GuidanceView, HrMaxView,
    HrZoneInputView, HypertrophyInputView, IntervalExec, IntervalVerdictView, LiftExec,
    LiftResultView, Model, PrescriptionView, Profile, ProgramSummaryView, ProteinFigureView,
    ProteinInputView, RacePredictionView, ReadinessSignalView, ResistanceModality, RunResultView,
    RunSplitView, SessionPlanView, SessionReview, SignalGroupView, SplitVerdictView,
    TodayHeadlineView, TrainingLoadView, ViewModel, WhyView, YouthActivityView,
};
pub use crux_core::Core;
pub use log::compact_event_log;
