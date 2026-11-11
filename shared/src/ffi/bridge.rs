//! The core behind the entry points: [`CoreBridge::call`] answers one [`Op`].
//!
//! Panic firewall: the entry points are `extern "system"` / `extern "C"`
//! functions, where a Rust unwind aborts the whole process. Every op runs
//! through [`guarded`], so no shell input (a malformed event, a core panic)
//! unwinds across that boundary; it becomes an error envelope instead.
//!
//! Threading: the core and its derived-view memos (`crate::memo`, per thread)
//! live on one thread the bridge owns. Every op that reads or changes the
//! core runs there, whichever thread calls, so the memos stay warm and the
//! core is never touched by two threads. Ops that read no core state run on
//! the caller's thread.

// The firewall contract: nothing in this module may panic on its own, so
// fallible calls are matched, never unwrapped.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::any::Any;
use std::borrow::Cow;
use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::sync::{LazyLock, mpsc};

use crux_core::bridge::{Bridge, BridgeError, JsonFfiFormat};
use crux_core::{App, Command, Core};

use super::ops::Op;
use crate::app::{Effect, Engine, Model, ViewModel};
use crate::envelope::{
    CoreError, ErrorEnvelope, ErrorKind, ImportedHr, ProfileDraft, RunRef, RunTrack, ThinnedTrack,
    TrackFigures, TrackThinning, WarmUpQuery,
};
use crate::event::Event;
use crate::log::LineFacts;

impl CoreError {
    pub(crate) fn new(
        kind: ErrorKind,
        context: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            kind,
            context: context.into(),
            message: message.into(),
        }
    }

    fn bad_input(op: Op, message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(ErrorKind::BadInput, op.context(), message)
    }

    fn unserializable(op: Op, err: &impl ToString) -> Self {
        Self::new(ErrorKind::Unserializable, op.context(), err.to_string())
    }
}

impl Op {
    /// The op's name as an error's `context`: `export_gpx` for `ExportGpx`.
    pub(super) fn context(self) -> String {
        let mut out = String::new();
        for (i, c) in self.name().chars().enumerate() {
            if c.is_ascii_uppercase() && i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        }
        out
    }
}

fn error_envelope(err: &CoreError) -> Vec<u8> {
    serde_json::to_vec(&ErrorEnvelope { error: err }).unwrap_or_default()
}

/// The bytes an op answers: its payload, or the envelope of its error.
pub(super) fn answer(result: Result<Vec<u8>, CoreError>) -> Vec<u8> {
    result.unwrap_or_else(|err| error_envelope(&err))
}

/// The answer to op `code` when its input could not be read.
pub(crate) fn unreadable_input(code: u32, message: String) -> Vec<u8> {
    let context = Op::from_code(code).map_or(Cow::Borrowed("call"), |op| op.context().into());
    error_envelope(&CoreError::new(ErrorKind::BadInput, context, message))
}

/// The panic firewall: a panic in `f` becomes an [`ErrorKind::Panic`] error
/// for `op` instead of unwinding across the FFI boundary.
///
/// `AssertUnwindSafe` is sound: the only state an op can leave half-updated
/// is the core, which the update path replaces after a panic
/// ([`CoreBridge::on_core`]).
fn guarded<T>(op: Op, f: impl FnOnce() -> Result<T, CoreError>) -> Result<T, CoreError> {
    std::panic::catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|payload| {
        Err(CoreError::new(
            ErrorKind::Panic,
            op.context(),
            panic_message(payload),
        ))
    })
}

fn panic_message(payload: Box<dyn Any + Send>) -> Cow<'static, str> {
    match payload.downcast::<String>() {
        Ok(message) => Cow::Owned(*message),
        Err(payload) => Cow::Borrowed(
            payload
                .downcast_ref::<&'static str>()
                .copied()
                .unwrap_or("unknown panic payload"),
        ),
    }
}

/// What an entry point hands an op.
pub(crate) enum Input<'a> {
    /// Owned when the binding already copied it out of the shell (JNI), so an
    /// op that runs on the core's thread takes it without another copy.
    Bytes(Cow<'a, [u8]>),
    /// The lines, each read when the op takes it; an unreadable line is the
    /// error's message.
    Lines(&'a mut dyn Iterator<Item = Result<Cow<'a, str>, String>>),
}

impl<'a> Input<'a> {
    fn bytes(self, op: Op) -> Result<Cow<'a, [u8]>, CoreError> {
        match self {
            Input::Bytes(bytes) => Ok(bytes),
            Input::Lines(_) => Err(CoreError::bad_input(op, "takes bytes, not lines")),
        }
    }

    fn lines(
        self,
        op: Op,
    ) -> Result<&'a mut dyn Iterator<Item = Result<Cow<'a, str>, String>>, CoreError> {
        match self {
            Input::Lines(lines) => Ok(lines),
            Input::Bytes(_) => Err(CoreError::bad_input(op, "takes lines, not bytes")),
        }
    }
}

/// [`Engine`] over a model the session shares with the GPX export, which
/// reads the logged runs between events.
#[derive(Default)]
struct BridgeApp;

impl App for BridgeApp {
    type Event = Event;
    type Model = Rc<RefCell<Model>>;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Self::Model) -> Command<Effect, Event> {
        Engine.update(event, &mut model.borrow_mut())
    }

    fn view(&self, model: &Self::Model) -> ViewModel {
        Engine.view(&model.borrow())
    }
}

/// The core on the bridge's thread: crux's JSON bridge, and the model it
/// shares with the GPX export.
pub(super) struct Session {
    bridge: Bridge<BridgeApp, JsonFfiFormat>,
    model: Rc<RefCell<Model>>,
}

impl Session {
    fn new() -> Self {
        let model = Rc::<RefCell<Model>>::default();
        Self {
            bridge: Bridge::new(Core::new_with(BridgeApp, Rc::clone(&model))),
            model,
        }
    }

    /// An event serde rejects (malformed, or an unknown variant) is dropped:
    /// the answer is empty and the core unchanged. The realistic trigger is
    /// log replay: the shell re-feeds every persisted event on launch, so a
    /// line written by a future app version must be skipped, not turned into
    /// a startup crash-loop.
    fn update(&self, event: &[u8]) -> Result<Vec<u8>, CoreError> {
        let mut out = Vec::new();
        match self.bridge.update(event, &mut out) {
            Ok(()) => Ok(out),
            Err(BridgeError::DeserializeEvent(_)) => Ok(Vec::new()),
            Err(err) => Err(CoreError::unserializable(Op::Update, &err)),
        }
    }

    fn view(&self) -> Result<Vec<u8>, CoreError> {
        let mut out = Vec::new();
        self.bridge
            .view(&mut out)
            .map_err(|err| CoreError::unserializable(Op::View, &err))?;
        Ok(out)
    }

    fn run_ref(op: Op, run: &[u8]) -> Result<RunRef, CoreError> {
        serde_json::from_slice(run).map_err(|err| CoreError::bad_input(op, err.to_string()))
    }

    fn export_gpx(&self, run: &[u8]) -> Result<Vec<u8>, CoreError> {
        let run = Self::run_ref(Op::ExportGpx, run)?;
        let gpx = crate::app::export_run_gpx(&self.model.borrow(), run.entry_id, run.observed_at);
        Ok(gpx.into_bytes())
    }

    fn run_track(&self, run: &[u8]) -> Result<Vec<u8>, CoreError> {
        let run = Self::run_ref(Op::RunTrack, run)?;
        let track = RunTrack {
            segments: crate::app::run_track_segments(
                &self.model.borrow(),
                run.entry_id,
                run.observed_at,
            ),
        };
        serde_json::to_vec(&track).map_err(|err| CoreError::unserializable(Op::RunTrack, &err))
    }
}

type Job = Box<dyn FnOnce(&mut Session) + Send>;

/// One crux core on its own thread: the process-wide [`BRIDGE`] the entry
/// points drive, or a test's own.
pub(crate) struct CoreBridge {
    /// Jobs for the core's thread; `None` when the thread could not start.
    jobs: Option<mpsc::Sender<Job>>,
}

impl CoreBridge {
    pub(crate) fn new() -> Self {
        let (jobs, queue) = mpsc::channel::<Job>();
        let started = std::thread::Builder::new()
            .name("lollop-core".to_owned())
            .spawn(move || {
                let mut session = Session::new();
                for job in queue {
                    job(&mut session);
                }
            });
        Self {
            jobs: started.is_ok().then_some(jobs),
        }
    }

    pub(crate) fn call(&self, code: u32, input: Input<'_>) -> Vec<u8> {
        match Op::from_code(code) {
            Some(op) => answer(self.dispatch(op, input)),
            None => unreadable_input(code, format!("no op has code {code}")),
        }
    }

    fn dispatch(&self, op: Op, input: Input<'_>) -> Result<Vec<u8>, CoreError> {
        match op {
            Op::Update => {
                let event = input.bytes(op)?.into_owned();
                self.on_core(op, move |session| session.update(&event))
            }
            Op::View => self.on_core(op, |session| session.view()),
            Op::ParseRunFile => {
                let file = input.bytes(op)?;
                guarded(op, || parse_run_file(&file))
            }
            Op::TrackFigures => {
                let event = input.bytes(op)?;
                guarded(op, || track_figures(&event))
            }
            Op::ExportGpx => {
                let run = input.bytes(op)?.into_owned();
                self.on_core(op, move |session| session.export_gpx(&run))
            }
            Op::RunTrack => {
                let run = input.bytes(op)?.into_owned();
                self.on_core(op, move |session| session.run_track(&run))
            }
            Op::CompactLog => {
                let lines = input.lines(op)?;
                guarded(op, || compact_log(lines))
            }
            Op::Reset => self.on_core(op, |session| {
                *session = Session::new();
                Ok(Vec::new())
            }),
            Op::ExerciseKeys => {
                let names = input.lines(op)?;
                guarded(op, || exercise_keys(names))
            }
            Op::WarmUpRamp => {
                let query = input.bytes(op)?;
                guarded(op, || warm_up_ramp(&query))
            }
            Op::DraftFacts => {
                let draft = input.bytes(op)?;
                guarded(op, || draft_facts(&draft))
            }
            Op::ThinTrack => {
                let track = input.bytes(op)?;
                guarded(op, || thin_track(&track))
            }
            Op::ImportedHrPct => {
                let hr = input.bytes(op)?;
                guarded(op, || imported_hr_pct(&hr))
            }
        }
    }

    /// Run `f` on the core's thread and wait for its answer. `f` owns what it
    /// reads (`'static`): the thread outlives every call. A panic in an
    /// update runs under crux's model-`RwLock` WRITE guard and poisons it, so
    /// every later call would re-panic and freeze the session: the core is
    /// replaced instead and the answer is `panic_reset`, on which the shell
    /// replays its persisted log into the clean core. Any other op's panic
    /// takes only the READ guard and keeps the core.
    pub(super) fn on_core(
        &self,
        op: Op,
        f: impl FnOnce(&mut Session) -> Result<Vec<u8>, CoreError> + Send + 'static,
    ) -> Result<Vec<u8>, CoreError> {
        let stopped = || {
            CoreError::new(
                ErrorKind::Panic,
                op.context(),
                "the core's thread is not running",
            )
        };
        let jobs = self.jobs.as_ref().ok_or_else(stopped)?;
        let (reply, answer) = mpsc::sync_channel(1);
        let job: Job = Box::new(move |session| {
            let result = guarded(op, || f(session)).map_err(|err| {
                if op != Op::Update || err.kind != ErrorKind::Panic {
                    return err;
                }
                *session = Session::new();
                CoreError {
                    kind: ErrorKind::PanicReset,
                    ..err
                }
            });
            let _ = reply.send(result);
        });
        jobs.send(job).map_err(|_| stopped())?;
        answer.recv().map_err(|_| stopped())?
    }
}

static BRIDGE: LazyLock<CoreBridge> = LazyLock::new(CoreBridge::new);

pub(crate) fn call(code: u32, input: Input<'_>) -> Vec<u8> {
    BRIDGE.call(code, input)
}

fn parse_run_file(bytes: &[u8]) -> Result<Vec<u8>, CoreError> {
    serde_json::to_vec(&crate::import::import_run_file(bytes))
        .map_err(|err| CoreError::unserializable(Op::ParseRunFile, &err))
}

/// The figures from [`crate::app::preview_track_figures`] (the same QC and
/// derivation the saved run's card uses), so the tracker's Finish review shows
/// exactly what History will list.
fn track_figures(event: &[u8]) -> Result<Vec<u8>, CoreError> {
    const OP: Op = Op::TrackFigures;
    let event = serde_json::from_slice::<Event>(event)
        .map_err(|err| CoreError::bad_input(OP, err.to_string()))?;
    let Event::LogRunTrack {
        points,
        segment_starts,
        ..
    } = event
    else {
        return Err(CoreError::bad_input(OP, "not a LogRunTrack event"));
    };
    let (distance_km, duration_min) = crate::app::preview_track_figures(points, &segment_starts);
    serde_json::to_vec(&TrackFigures {
        distance_km,
        duration_min,
    })
    .map_err(|err| CoreError::unserializable(OP, &err))
}

/// [`crate::log::compact_lines`]: each line is parsed as it is read, so a
/// multi-year GPS log is never held decoded.
fn compact_log(
    lines: &mut dyn Iterator<Item = Result<Cow<'_, str>, String>>,
) -> Result<Vec<u8>, CoreError> {
    let facts = lines
        .map(|line| line.map(|line| LineFacts::parse(&line)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|message| CoreError::bad_input(Op::CompactLog, message))?;
    serde_json::to_vec(&crate::log::compact_lines(&facts))
        .map_err(|err| CoreError::unserializable(Op::CompactLog, &err))
}

fn exercise_keys(
    names: &mut dyn Iterator<Item = Result<Cow<'_, str>, String>>,
) -> Result<Vec<u8>, CoreError> {
    const OP: Op = Op::ExerciseKeys;
    let keys = names
        .map(|name| name.map(|name| crate::names::exercise_key(&name)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|message| CoreError::bad_input(OP, message))?;
    serde_json::to_vec(&keys).map_err(|err| CoreError::unserializable(OP, &err))
}

fn warm_up_ramp(query: &[u8]) -> Result<Vec<u8>, CoreError> {
    const OP: Op = Op::WarmUpRamp;
    let q = serde_json::from_slice::<WarmUpQuery>(query)
        .map_err(|err| CoreError::bad_input(OP, err.to_string()))?;
    let steps = crate::strength::warm_up_ramp(&q.exercise, q.work_kg, q.weight_lb).value;
    serde_json::to_vec(&steps).map_err(|err| CoreError::unserializable(OP, &err))
}

fn draft_facts(draft: &[u8]) -> Result<Vec<u8>, CoreError> {
    const OP: Op = Op::DraftFacts;
    let draft = serde_json::from_slice::<ProfileDraft>(draft)
        .map_err(|err| CoreError::bad_input(OP, err.to_string()))?;
    serde_json::to_vec(&crate::app::draft_facts(draft))
        .map_err(|err| CoreError::unserializable(OP, &err))
}

fn thin_track(track: &[u8]) -> Result<Vec<u8>, CoreError> {
    const OP: Op = Op::ThinTrack;
    let track = serde_json::from_slice::<TrackThinning>(track)
        .map_err(|err| CoreError::bad_input(OP, err.to_string()))?;
    let (kept, segment_starts) = crate::gps::thin_track(
        track.fixes as usize,
        track.max_points as usize,
        &track.segment_starts,
    );
    serde_json::to_vec(&ThinnedTrack {
        kept: kept.into_iter().map(|k| k as u32).collect(),
        segment_starts,
    })
    .map_err(|err| CoreError::unserializable(OP, &err))
}

fn imported_hr_pct(hr: &[u8]) -> Result<Vec<u8>, CoreError> {
    const OP: Op = Op::ImportedHrPct;
    let hr = serde_json::from_slice::<ImportedHr>(hr)
        .map_err(|err| CoreError::bad_input(OP, err.to_string()))?;
    serde_json::to_vec(&crate::import::imported_hr_pct_max(
        hr.avg_hr_bpm,
        hr.measured_hr_max,
    ))
    .map_err(|err| CoreError::unserializable(OP, &err))
}

/// Panic while holding the core, exactly the case the firewall guards, for a
/// shell's bridge tests (feature `test-hooks`, never in an app build): `reset`
/// takes the update path (`panic_reset`, core replaced), else the view path
/// (`panic`, core kept).
#[cfg(feature = "test-hooks")]
pub(crate) fn panic_for_test(reset: bool) -> Vec<u8> {
    let op = if reset { Op::Update } else { Op::View };
    answer(BRIDGE.on_core(op, |_| panic!("test-hooks panic")))
}
