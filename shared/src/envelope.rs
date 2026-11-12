//! The bridge's wire payloads besides events and the view model: the error
//! envelope every op can answer, the run reference of the GPX export and the
//! route fixes, the row an Undo re-logs, the track figures preview, the
//! warm-up ramp's query, the recorder's track thinning and an imported run's
//! heart rate (`shared::ffi`).

use std::borrow::Cow;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::wire::Wire;

/// Why an op answered an [`ErrorEnvelope`]; serialised as the envelope's
/// `kind`. The shell keys on `panic_reset` (replay the log into the fresh
/// core); any other kind reads as a generic failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Wire)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// A core panic on the update path; the core was replaced with an empty one.
    PanicReset,
    /// A core panic that left the core intact.
    Panic,
    /// The answer could not be serialised.
    Unserializable,
    /// The shell's input is not what the op takes.
    BadInput,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.serialize(f)
    }
}

/// One failed op: the body of `{"error":{"kind":K,"context":C,"message":M}}`.
#[derive(Debug, Serialize, Wire)]
pub struct CoreError {
    pub kind: ErrorKind,
    pub context: Cow<'static, str>,
    pub message: Cow<'static, str>,
}

/// `{"error":{…}}`: `error` is the only top-level key, so an envelope is never
/// mistaken for an effect-request array, a `ViewModel` or any op's payload.
#[derive(Serialize, Wire)]
pub struct ErrorEnvelope<'a> {
    pub error: &'a CoreError,
}

/// The run the GPX export and the route fixes read, by the
/// `(entry_id, observed_at)` identity the delete/amend matcher uses.
#[derive(Debug, Deserialize, Wire)]
pub struct RunRef {
    pub entry_id: u64,
    pub observed_at: i64,
}

/// The logged row an Undo puts back, by the `(entry_id, observed_at)` identity
/// the delete/amend matcher uses.
#[derive(Debug, Deserialize, Wire)]
pub struct RelogQuery {
    pub kind: crate::event::EntryKind,
    pub entry_id: u64,
    pub observed_at: i64,
    /// An edit's Undo: the `observed_at` the edit saves the row under, which
    /// the restoring amend names it by. `None`: a delete's Undo.
    #[serde(default)]
    pub amended_at: Option<i64>,
}

/// The distance and moving time a `LogRunTrack` will be saved with.
#[derive(Debug, Serialize, Wire)]
pub struct TrackFigures {
    pub distance_km: f64,
    pub duration_min: f64,
}

/// A logged run's route fixes, one list per recording segment: the QC'd route
/// the GPX export writes, for the shell's map and trim. Empty for a run with
/// no route or an unknown run.
#[derive(Debug, Serialize, Wire)]
pub struct RunTrack {
    pub segments: Vec<Vec<crate::gps::GpsPoint>>,
}

/// The lift the set screen asks the warm-up ramp for: its exercise, the work
/// set's load (wire kg) and the display unit whose plate grid the steps land
/// on.
#[derive(Debug, Deserialize, Wire)]
pub struct WarmUpQuery {
    pub exercise: String,
    pub work_kg: f64,
    pub weight_lb: bool,
}

/// A recorded track the shell thins for its save or a crash recovery: how
/// many fixes it holds, the most to keep and the indices of the fixes that
/// opened a recording segment.
#[derive(Debug, Deserialize, Wire)]
pub struct TrackThinning {
    pub fixes: u32,
    pub max_points: u32,
    #[serde(default)]
    pub segment_starts: Vec<u32>,
}

/// A [`TrackThinning`]'s answer: the indices of the fixes kept (ascending),
/// and the segment starts as positions among them.
#[derive(Debug, Serialize, Wire)]
pub struct ThinnedTrack {
    pub kept: Vec<u32>,
    pub segment_starts: Vec<u32>,
}

/// A profile the shell is still editing (guided setup, Profile): the answers
/// the safety gates and the setup's choices read, before any `SetProfile`.
#[derive(Debug, Default, Deserialize, Wire)]
pub struct ProfileDraft {
    #[serde(default)]
    pub health: crate::schema::HealthScreen,
    /// Applied to `health` first: the answer the user just gave.
    #[serde(default)]
    pub answer: Option<crate::schema::HealthAnswer>,
    /// A stated age, used when no birthday is given.
    #[serde(default)]
    pub age_years: Option<f64>,
    #[serde(default)]
    pub birth_epoch_day: Option<i64>,
    /// The shell's local epoch day, which a birthday's age is counted to.
    #[serde(default)]
    pub today: Option<i64>,
    #[serde(default)]
    pub calis_equipment: Option<crate::profile::CalisEquipment>,
}

/// What a [`ProfileDraft`] means to the core: the gates it would raise and
/// the PAR-Q+ answers as the form shows them.
#[derive(Debug, PartialEq, Serialize, Wire)]
pub struct DraftFacts {
    /// The draft's health screen with its `answer` applied.
    pub health: crate::schema::HealthScreen,
    /// The PAR-Q+ questions answered Yes, in form order.
    pub parq_yes: Vec<crate::schema::ParqQuestion>,
    /// The first symptom question (Q2/Q3) answered Yes.
    pub first_symptom: Option<crate::schema::ParqQuestion>,
    /// A doctor flag from the old single toggle, with no follow-up answer.
    pub legacy_parq: bool,
    /// The health answers hold training (the age gate apart).
    pub health_defers: bool,
    /// The birthday's whole years on `today`, else the stated age.
    pub age_years: Option<f64>,
    /// Under 18: the pediatric hold (SAFE-PEDS-001).
    pub youth: bool,
    /// The stated equipment has something to hang from (every pull-up rung
    /// needs it); `None` when no equipment is stated.
    pub can_hang: Option<bool>,
}
