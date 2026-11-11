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
    /// A run file's bytes in (FIT, GPX or TCX, sniffed from the content); its
    /// `ImportVerdict` out: the runs to log, or the one reason the file gave
    /// none. Reads no core state.
    ParseRunFile(Bytes file),
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
    /// Exercise names in, one per line; their match keys out, a JSON array of
    /// strings in line order (two spellings of one exercise share a key).
    /// Reads no core state.
    ExerciseKeys(Lines names),
    /// A `WarmUpQuery`'s JSON in; the `WarmUpStep`s before the first work
    /// set at its load out, a JSON array (empty for a lift off the straight
    /// barbell or a load at or below the first step). Reads no core state.
    WarmUpRamp(Bytes query),
    /// A `ProfileDraft`'s JSON in (a profile still being edited, with the
    /// health answer just given); its `DraftFacts` out: the age, the gates it
    /// would raise and the PAR-Q+ answers as the form shows them. Reads no
    /// core state.
    DraftFacts(Bytes draft),
    /// A run's `{"entry_id":I,"observed_at":T}` in; its `RunTrack` out: the
    /// fixes the GPX export writes, one list per recording segment (none for
    /// a run with no track or an unknown run).
    RunTrack(Bytes run),
    /// A `TrackThinning`'s JSON in (a recorded track's fix count, cap and
    /// segment starts); its `ThinnedTrack` out: the fixes a save or a crash
    /// recovery keeps and the segment starts among them. Reads no core state.
    ThinTrack(Bytes track),
    /// An `ImportedHr`'s JSON in; the imported run's `hr_pct_max` out, a
    /// JSON number (0.0 without samples or a measured HRmax). Reads no core
    /// state.
    ImportedHrPct(Bytes hr),
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
