//! The rules `update` applies and compaction mirrors that are not about one
//! logged row: event families and their clears, plan spans, which readiness
//! input a removal takes, and the end-marker key.

use super::Event;
use crate::schema::{ReadinessInput, ReadinessSignal};

/// An event family: the events whose effect one `Clear<F>` resets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Family {
    Readiness,
    /// Sets, plus the end markers and rung placements that qualify them.
    Sets,
    Runs,
    Activities,
    Profile,
    /// Session review and weekly check-in. Their clears reset only the
    /// current session review or weekly check-in, never the weekly history.
    Review,
    RacePrediction,
    HypertrophyPlan,
    Protein,
    HrZones,
    Checkins,
    /// The plan request and its session moves.
    Plan,
    /// The shell's clock (`SetToday`); never cleared.
    Today,
}

impl Event {
    /// The family this event belongs to, and whether it is that family's
    /// clear. Exhaustive, so a new event does not compile until it is placed.
    pub(crate) fn family(&self) -> (Family, bool) {
        use Family as F;
        match self {
            Event::SubmitReadiness(_)
            | Event::RemoveReadiness { .. }
            | Event::ResolveHold { .. } => (F::Readiness, false),
            Event::ClearReadiness => (F::Readiness, true),
            Event::LogSet { .. }
            | Event::AmendSet { .. }
            | Event::SetExerciseEnded { .. }
            | Event::SetCalisthenicsRung { .. } => (F::Sets, false),
            Event::ClearSets => (F::Sets, true),
            Event::LogRun { .. }
            | Event::LogRunTrack { .. }
            | Event::AmendRun { .. }
            | Event::TrimRunTrack { .. }
            | Event::AmendRunMeta { .. } => (F::Runs, false),
            Event::ClearRuns => (F::Runs, true),
            Event::LogActivity { .. } => (F::Activities, false),
            Event::ClearActivities => (F::Activities, true),
            // A delete joins the family it targets, so a later clear sweeps it.
            Event::DeleteEntry { kind, .. } => (Family::of_entry(*kind), false),
            Event::SetProfile(_) => (F::Profile, false),
            Event::ClearProfile => (F::Profile, true),
            Event::SubmitReview(_) | Event::SubmitWeeklyCheckin(_) => (F::Review, false),
            Event::ClearReview | Event::ClearWeeklyCheckin => (F::Review, true),
            Event::PredictRace { .. } => (F::RacePrediction, false),
            Event::ClearRacePrediction => (F::RacePrediction, true),
            Event::PlanHypertrophyMeso { .. } => (F::HypertrophyPlan, false),
            Event::ClearHypertrophyPlan => (F::HypertrophyPlan, true),
            Event::ComputeProtein { .. } => (F::Protein, false),
            Event::ClearProtein => (F::Protein, true),
            Event::ComputeHrZones { .. } => (F::HrZones, false),
            Event::ClearHrZones => (F::HrZones, true),
            Event::SubmitCheckin(_) | Event::RestartHrvBaseline { .. } => (F::Checkins, false),
            Event::ClearCheckins => (F::Checkins, true),
            Event::GeneratePlan { .. } | Event::MoveSession { .. } => (F::Plan, false),
            Event::ClearPlan => (F::Plan, true),
            Event::SetToday { .. } => (F::Today, false),
        }
    }
}

impl Family {
    pub(crate) fn of_entry(kind: super::EntryKind) -> Self {
        match kind {
            super::EntryKind::Set => Family::Sets,
            super::EntryKind::Run => Family::Runs,
            super::EntryKind::Activity => Family::Activities,
        }
    }
}

/// What a plan event does given whether a plan is in force.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PlanStep {
    /// `GeneratePlan` with no plan: the plan starts.
    Start,
    /// `GeneratePlan` while a plan exists (the shell sends one every launch):
    /// the original start stands.
    KeepOriginal,
    /// `ClearPlan` with a plan: the plan ends and its span is recorded.
    Close,
    /// `ClearPlan` with no plan: nothing to remove.
    NoPlan,
}

impl PlanStep {
    /// The step a `GeneratePlan` (`generate`) or `ClearPlan` takes.
    pub(crate) fn of(generate: bool, plan_open: bool) -> Self {
        match (generate, plan_open) {
            (true, false) => PlanStep::Start,
            (true, true) => PlanStep::KeepOriginal,
            (false, true) => PlanStep::Close,
            (false, false) => PlanStep::NoPlan,
        }
    }
}

/// The input a `RemoveReadiness` of `signal` takes out of the active
/// `inputs`: for Pain the report the verdict reads
/// ([`crate::autoreg::worst_active_pain`]), so the undo removes what the
/// athlete sees; otherwise, or with no active pain report, the newest input
/// of the signal.
pub(crate) fn readiness_removal(
    inputs: &[ReadinessInput],
    signal: ReadinessSignal,
) -> Option<usize> {
    let newest = || inputs.iter().rposition(|i| i.signal == signal);
    if signal != ReadinessSignal::Pain {
        return newest();
    }
    crate::autoreg::worst_active_pain(inputs)
        .and_then(|w| inputs.iter().position(|i| std::ptr::eq(i, w)))
        .or_else(newest)
}

/// The key a `SetExerciseEnded` marker is last-write-wins under: the
/// exercise's movement ([`crate::exercise::movement_key`], so equipment words
/// and legacy catalog labels don't split it) and the session day.
pub(crate) fn end_marker_key(exercise: &str, epoch_day: i64) -> (Vec<String>, i64) {
    (crate::exercise::movement_key(exercise), epoch_day)
}

/// The key compaction supersedes a `SetExerciseEnded` line under: its
/// [`end_marker_key`] refined by the exact (folded) name, so two lines share
/// it only when `update` replaces one marker with the other. Lines naming one
/// movement in different words (equipment, legacy label) are both kept:
/// replay drops the earlier marker anyway.
pub(crate) fn end_marker_line_key(exercise: &str, epoch_day: i64) -> EndMarkerLineKey {
    (
        end_marker_key(exercise, epoch_day),
        crate::names::exercise_key(exercise),
    )
}

pub(crate) type EndMarkerLineKey = ((Vec<String>, i64), String);
