//! Logged-row identity: which stored set, run or activity an edit event
//! (`DeleteEntry`, `AmendSet`, `AmendRun`, `TrimRunTrack`, `AmendRunMeta`)
//! names, and which logging events store a row at all. `update` locates rows
//! with [`find_entry`]; compaction pairs an edit with its log line by the same
//! [`EntryRef::names`].

use super::{EntryKind, Event};

/// A row's identity: the shell-assigned `entry_id`, and its `observed_at` for
/// a legacy row logged without an id (`id == 0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EntryRef {
    pub(crate) id: u64,
    pub(crate) observed_at: i64,
}

impl EntryRef {
    /// Whether `row` is the row this reference names: the same id, or for a
    /// legacy reference (`id == 0`) a legacy row with the same `observed_at`.
    pub(crate) fn names(self, row: EntryRef) -> bool {
        if self.id != 0 {
            row.id == self.id
        } else {
            row.id == 0 && row.observed_at == self.observed_at
        }
    }
}

/// A stored row an edit can name.
pub(crate) trait LoggedEntry {
    fn entry(&self) -> EntryRef;
}

/// Index of the newest row `target` names.
pub(crate) fn find_entry<T: LoggedEntry>(rows: &[T], target: EntryRef) -> Option<usize> {
    rows.iter().rposition(|r| target.names(r.entry()))
}

/// Longest single activity `LogActivity` accepts, minutes (10 h).
pub(crate) const ACTIVITY_MAX_MINUTES: f64 = 600.0;

/// Whether a `LogActivity` stores a row: minutes in (0, 600].
pub(crate) fn activity_is_valid(minutes: f64) -> bool {
    minutes > 0.0 && minutes <= ACTIVITY_MAX_MINUTES
}

/// Whether a `LogSet` / `AmendSet` stores a row: a non-negative weight and
/// either reps or a hold time (the e1RM formulas need reps >= 1).
pub(crate) fn set_is_valid(weight_kg: f64, reps: u32, hold_sec: Option<u16>) -> bool {
    weight_kg >= 0.0 && (reps > 0 || hold_sec.is_some())
}

/// What an event does to a logged row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum EntryOp {
    /// `LogSet`, `LogRun`, `LogRunTrack`, `LogActivity`: stores a new row.
    Log,
    /// `AmendSet`, `AmendRun`: replaces the row it names.
    Amend,
    /// `TrimRunTrack`, `AmendRunMeta`: edits the row it names in place.
    Trim,
    /// `DeleteEntry`: removes the row it names.
    Delete,
}

/// The row facts of one entry event.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EntryEvent {
    pub(crate) kind: EntryKind,
    pub(crate) op: EntryOp,
    /// The row identity the event carries: a log's or amend's new row, the
    /// row a trim edits, the row a delete names.
    pub(crate) row: EntryRef,
    /// Whether the event leaves a row stored under `row`: false for a log or
    /// amend that fails its validity gate (it stores nothing), and for a
    /// delete.
    pub(crate) stored: bool,
    /// The row an amend, trim or delete names.
    pub(crate) target: Option<EntryRef>,
}

impl Event {
    /// The row facts of a logging or edit event; `None` for every other
    /// event. Expects a [`Event::normalized`] event.
    pub(crate) fn entry_event(&self) -> Option<EntryEvent> {
        let at = |id: u64, observed_at: i64| EntryRef { id, observed_at };
        let (kind, op, row, stored, target) = match *self {
            Event::LogSet {
                entry_id,
                observed_at,
                weight_kg,
                reps,
                hold_sec,
                ..
            } => (
                EntryKind::Set,
                EntryOp::Log,
                at(entry_id, observed_at),
                set_is_valid(weight_kg, reps, hold_sec),
                None,
            ),
            Event::AmendSet {
                entry_id,
                observed_at,
                observed_at_fallback,
                weight_kg,
                reps,
                hold_sec,
                ..
            } => (
                EntryKind::Set,
                EntryOp::Amend,
                at(entry_id, observed_at),
                set_is_valid(weight_kg, reps, hold_sec),
                Some(at(entry_id, observed_at_fallback)),
            ),
            Event::LogRun {
                entry_id,
                observed_at,
                ..
            }
            | Event::LogRunTrack {
                entry_id,
                observed_at,
                ..
            } => (
                EntryKind::Run,
                EntryOp::Log,
                at(entry_id, observed_at),
                true,
                None,
            ),
            Event::AmendRun {
                entry_id,
                observed_at,
                observed_at_fallback,
                ..
            } => (
                EntryKind::Run,
                EntryOp::Amend,
                at(entry_id, observed_at),
                true,
                Some(at(entry_id, observed_at_fallback)),
            ),
            Event::TrimRunTrack {
                entry_id,
                observed_at_fallback,
                ..
            }
            | Event::AmendRunMeta {
                entry_id,
                observed_at_fallback,
                ..
            } => {
                let run = at(entry_id, observed_at_fallback);
                (EntryKind::Run, EntryOp::Trim, run, true, Some(run))
            }
            Event::LogActivity {
                entry_id,
                observed_at,
                minutes,
                ..
            } => (
                EntryKind::Activity,
                EntryOp::Log,
                at(entry_id, observed_at),
                activity_is_valid(minutes),
                None,
            ),
            Event::DeleteEntry {
                kind,
                entry_id,
                observed_at_fallback,
            } => {
                let named = at(entry_id, observed_at_fallback);
                (kind, EntryOp::Delete, named, false, Some(named))
            }
            _ => return None,
        };
        Some(EntryEvent {
            kind,
            op,
            row,
            stored,
            target,
        })
    }
}
