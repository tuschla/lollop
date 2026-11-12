//! Every operation the shells call on the core, declared once with what it
//! takes and answers. Both entry points take an op's code ([`Op::code`]) with
//! its input: the C ABI's `lollop_call` / `lollop_call_lines` and the JNI's
//! `JniCore.call` / `JniCore.callLines`. `shared/wiregen` generates the
//! shells' `CoreBridge` (one method per op) and the Kotlin wire types the
//! ops exchange ([`roots`]) from this table; the bridge answers each op
//! through its [`sig`] type, so a handler that reads or answers another type
//! does not compile. Adding an op is one entry here, its arm in the bridge's
//! dispatch, and its callers.
//!
//! An op's input is `json name: T` (`T`'s JSON), `bytes name` (raw bytes),
//! `lines name` (a text's lines) or nothing; its answer `json T` or `bytes`.

use crate::wire::{Registry, Shape, Wire};
use crate::{app, envelope, event, import, log, strength};

/// What an op takes from the shell, named as the shell's parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    None,
    Bytes(&'static str),
    /// A text's lines, each read as UTF-8 on its own (the event log, which
    /// would not fit in memory twice as one buffer).
    Lines(&'static str),
}

/// An op's input and answer types: `In` is decoded from the shell's JSON and
/// `Out` encoded as the answer when they are JSON ([`Raw`] otherwise).
pub trait Signature {
    const OP: Op;
    type In: ?Sized;
    type Out: ?Sized;
}

/// Input or answer bytes that are not JSON (a run file, the GPX document), or
/// lines.
pub enum Raw {}

/// The named type `shape` is, or lists.
fn root_name(shape: &Shape) -> Option<&'static str> {
    match shape {
        Shape::Named(name) => Some(name),
        Shape::List(inner) | Shape::Option(inner) => root_name(inner),
        _ => None,
    }
}

macro_rules! ops {
    ($($(#[doc = $doc:literal])+
       $op:ident($($kind:ident $param:ident $(: $in:ty)?)?) -> $out_kind:ident $($out:ty)?,)+) => {
        /// A bridge operation. Every op answers bytes: its payload, or the
        /// `{"error":{"kind":…,"context":…,"message":…}}` envelope of its
        /// failure.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum Op {
            $($(#[doc = $doc])+ $op,)+
        }

        impl Op {
            /// Every op, in code order.
            pub const ALL: &'static [Op] = &[$(Op::$op),+];

            pub const fn name(self) -> &'static str {
                match self {
                    $(Op::$op => stringify!($op),)+
                }
            }

            pub const fn param(self) -> Param {
                match self {
                    $(Op::$op => ops!(@param $($kind $param)?),)+
                }
            }

            pub const fn doc(self) -> &'static str {
                match self {
                    $(Op::$op => concat!($($doc, "\n"),+),)+
                }
            }

            /// The shapes of the op's JSON input and answer; `None` for raw
            /// bytes, lines or no input.
            pub fn payloads(self, reg: &mut Registry) -> (Option<Shape>, Option<Shape>) {
                match self {
                    $(Op::$op => (
                        ops!(@shape reg $($kind $($in)?)?),
                        ops!(@shape reg $out_kind $($out)?),
                    ),)+
                }
            }
        }

        /// One [`Signature`] per op, named as the op.
        pub mod sig {
            use super::*;

            $(
                pub enum $op {}

                impl Signature for $op {
                    const OP: Op = Op::$op;
                    type In = ops!(@type $($kind $($in)?)?);
                    type Out = ops!(@type $out_kind $($out)?);
                }
            )+
        }
    };
    (@param) => { Param::None };
    (@param json $param:ident) => { Param::Bytes(stringify!($param)) };
    (@param bytes $param:ident) => { Param::Bytes(stringify!($param)) };
    (@param lines $param:ident) => { Param::Lines(stringify!($param)) };
    (@type) => { () };
    (@type json $t:ty) => { $t };
    (@type bytes) => { Raw };
    (@type lines) => { Raw };
    (@shape $reg:ident) => { None };
    (@shape $reg:ident json $t:ty) => { Some(<$t as Wire>::shape($reg)) };
    (@shape $reg:ident bytes) => { None };
    (@shape $reg:ident lines) => { None };
}

ops! {
    /// One event in; the effect-request array (`[...]`) out, no bytes for a
    /// dropped (malformed or unknown) event, or a `panic_reset` error after a
    /// caught core panic (the core was replaced: replay the log).
    Update(json event: event::Event) -> bytes,
    /// The current view, or a `panic` error that leaves the core intact.
    View() -> json app::ViewModel,
    /// A run file's bytes in (FIT, GPX or TCX, sniffed from the content); the
    /// runs to log, or the one reason the file gave none. Reads no core state.
    ParseRunFile(bytes file) -> json import::ImportVerdict,
    /// A `LogRunTrack` event in; the figures it will be saved with. Reads no
    /// core state.
    TrackFigures(json event: event::Event) -> json envelope::TrackFigures,
    /// A logged run in; the GPX document of its route, or no bytes for a run
    /// with no exportable route or an unknown run.
    ExportGpx(json run: envelope::RunRef) -> bytes,
    /// The event log's lines in; the compaction verdict out. Reads no core
    /// state.
    CompactLog(lines lines) -> json log::Compaction,
    /// Replace the core with an empty one, as a fresh process starts it.
    Reset() -> bytes,
    /// Exercise names in, one per line; their match keys out, in line order
    /// (two spellings of one exercise share a key). Reads no core state.
    ExerciseKeys(lines names) -> json Vec<String>,
    /// A lift and its work set's load in; the warm-up steps before the first
    /// work set (none for a lift off the straight barbell or a load at or
    /// below the first step). Reads no core state.
    WarmUpRamp(json query: envelope::WarmUpQuery) -> json Vec<strength::WarmUpStep>,
    /// A profile still being edited, with the health answer just given; the
    /// age, the gates it would raise and the PAR-Q+ answers as the form shows
    /// them. Reads no core state.
    DraftFacts(json draft: envelope::ProfileDraft) -> json envelope::DraftFacts,
    /// A logged run in; the fixes the GPX export writes, one list per
    /// recording segment (none for a run with no track or an unknown run).
    RunTrack(json run: envelope::RunRef) -> json envelope::RunTrack,
    /// A recorded track's fix count, cap and segment starts in; the fixes a
    /// save or a crash recovery keeps and the segment starts among them.
    /// Reads no core state.
    ThinTrack(json track: envelope::TrackThinning) -> json envelope::ThinnedTrack,
    /// An imported run's heart rate in; its `hr_pct_max` (0.0 without samples
    /// or a measured HRmax). Reads no core state.
    ImportedHrPct(json hr: envelope::ImportedHr) -> json f64,
    /// A logged row in, read before the delete or edit it undoes; the event
    /// that puts it back as it is now: for a delete the logging event (a GPS
    /// run with its recorded track), for an edit the amend that restores it.
    /// `null` for an unknown row, or an edit of a GPS run or an activity.
    RelogEvent(json row: envelope::RelogQuery) -> json Option<event::Event>,
}

impl Op {
    /// The code an entry point takes for this op.
    pub const fn code(self) -> u32 {
        self as u32
    }

    pub fn from_code(code: u32) -> Option<Op> {
        Self::ALL.get(usize::try_from(code).ok()?).copied()
    }
}

/// The named types the shells exchange with the core, registered in `reg`:
/// those an op takes as JSON (`inputs`), those an op answers plus the error
/// envelope every op may answer (`outputs`), and the view (`View`'s answer).
#[derive(Debug, Default)]
pub struct Roots {
    pub inputs: Vec<&'static str>,
    pub outputs: Vec<&'static str>,
    pub view: &'static str,
}

pub fn roots(reg: &mut Registry) -> Roots {
    fn add(list: &mut Vec<&'static str>, shape: Option<Shape>) {
        if let Some(name) = shape.as_ref().and_then(root_name)
            && !list.contains(&name)
        {
            list.push(name);
        }
    }
    let mut roots = Roots::default();
    for &op in Op::ALL {
        let (input, output) = op.payloads(reg);
        if op == Op::View {
            roots.view = output.as_ref().and_then(root_name).unwrap_or_default();
        }
        add(&mut roots.inputs, input);
        add(&mut roots.outputs, output);
    }
    add(
        &mut roots.outputs,
        Some(envelope::ErrorEnvelope::shape(reg)),
    );
    roots
}
