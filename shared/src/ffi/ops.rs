//! Every operation the shells call on the core, declared once. Both entry
//! points take an op's code ([`Op::code`]) with its input: the C ABI's
//! `lollop_call` / `lollop_call_lines` and the JNI's `JniCore.call` /
//! `JniCore.callLines`. `shared/wiregen` generates the shells' `CoreBridge`
//! (one method per op) from this table, so adding an op is one entry here,
//! its arm in the bridge's dispatch, and its callers.

/// What an op takes from the shell, named as the shell's parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    /// Nothing.
    None,
    /// One byte buffer.
    Bytes(&'static str),
    /// A text's lines, each read as UTF-8 on its own (the event log, which
    /// would not fit in memory twice as one buffer).
    Lines(&'static str),
}

macro_rules! ops {
    ($($(#[doc = $doc:literal])+ $op:ident($($kind:ident $param:ident)?),)+) => {
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

            /// The variant's name.
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

            /// The op's documentation, one line per doc line.
            pub const fn doc(self) -> &'static str {
                match self {
                    $(Op::$op => concat!($($doc, "\n"),+),)+
                }
            }
        }
    };
    (@param) => { Param::None };
    (@param $kind:ident $param:ident) => { Param::$kind(stringify!($param)) };
}

ops! {
    /// One event's wire JSON in; the effect-request array (`[...]`) out, no
    /// bytes for a dropped (malformed or unknown) event, or a `panic_reset`
    /// error after a caught core panic (the core was replaced: replay the log).
    Update(Bytes event),
    /// The current `ViewModel` JSON, or a `panic` error that leaves the core
    /// intact.
    View(),
    /// A `.fit` file's bytes in; the tracker-import JSON out, or the error of
    /// a file that is no run (kinds `not_fit`, `damaged`, `no_track`,
    /// `too_short`, `not_a_run`). Reads no core state.
    ParseFit(Bytes fit),
    /// A `LogRunTrack` event's wire JSON in; the figures it will be saved with
    /// (`{"distance_km":D,"duration_min":M}`) out. Reads no core state.
    TrackFigures(Bytes event),
    /// A run's `{"entry_id":I,"observed_at":T}` in; the GPX document of its
    /// route out, or no bytes for a run with no exportable route or an
    /// unknown run.
    ExportGpx(Bytes run),
    /// The event log's lines in; the compaction verdict JSON out. Reads no
    /// core state.
    CompactLog(Lines lines),
    /// Replace the core with an empty one, as a fresh process starts it.
    Reset(),
}

impl Op {
    /// The code an entry point takes for this op.
    pub const fn code(self) -> u32 {
        self as u32
    }

    /// The op with code `code`.
    pub fn from_code(code: u32) -> Option<Op> {
        Self::ALL.get(usize::try_from(code).ok()?).copied()
    }
}
