//! The bridge's wire payloads besides events and the view model: the error
//! envelope every op can answer, the GPX export's run reference and the track
//! figures preview (`shared::ffi`).

use std::borrow::Cow;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::wire::Wire;

/// Why an op answered an [`ErrorEnvelope`]; serialised as the envelope's
/// `kind`. The shell keys on `panic_reset` (replay the log into the fresh
/// core) and on the FIT kinds (per-file import reason); any other kind reads
/// as a generic failure.
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
    /// `FitError::NotFit`.
    NotFit,
    /// `FitError::Damaged`.
    Damaged,
    /// `FitError::NoTrack`.
    NoTrack,
    /// `FitError::TooShort`.
    TooShort,
    /// `FitError::NotARun` (context: the sport).
    NotARun,
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

/// The run the GPX export reads, by the `(entry_id, observed_at)` identity
/// the delete/amend matcher uses.
#[derive(Debug, Deserialize, Wire)]
pub struct RunRef {
    pub entry_id: u64,
    pub observed_at: i64,
}

/// The distance and moving time a `LogRunTrack` will be saved with.
#[derive(Debug, Serialize, Wire)]
pub struct TrackFigures {
    pub distance_km: f64,
    pub duration_min: f64,
}
