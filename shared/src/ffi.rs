//! JSON FFI bridge for native shells.
//!
//! Exposes the crux [`Engine`](crate::app::Engine) over a JSON-serialised
//! [`Bridge`](crux_core::bridge::Bridge) so a native shell can drive the same
//! side-effect-free core. The wire format is JSON: events in, effect-requests +
//! view-model out. Reuses the existing `serde` derives, no Facet typegen
//! required.
//!
//! This module is the ONE platform-neutral bridge API: [`process_event`],
//! [`current_view`], [`parse_fit_json`], [`track_figures_json`],
//! [`export_gpx`], [`compact_log_json`] and [`reset_bridge`]. Two thin bindings
//! wrap it and add no logic of their own: the Android JNI exports
//! (`Java_app_milestone_Core_*`, the `android` module below) and the C ABI
//! (`lollop_*`, the [`c_abi`] module, linked into the iOS app as a staticlib).
//!
//! Flow for the shell:
//!   1. `update(eventJson)` → JSON array of effect requests (only `Render`).
//!   2. `view()` → JSON `ViewModel` reflecting the new state.
//!
//! Panic firewall: these functions are called across an `extern "system"` /
//! `extern "C"` boundary, where a Rust unwind is undefined behaviour and, since
//! Rust 1.81, aborts the whole process: an app crash the shell can never
//! intercept. So every op runs through [`guarded`], and no shell input (a
//! poisoned lock, a malformed event, a core panic) unwinds across that
//! boundary; it degrades to a dropped event, an empty answer or an
//! [`ErrorEnvelope`] instead.
//!
//! Threading: every function is safe to call from any thread (the bridge sits
//! behind a mutex), but the core's derived-view memos are thread-local, so a
//! shell keeps its view-producing calls on one thread or each thread hop
//! rebuilds them from scratch.

// The firewall contract: nothing in this module may panic on its own, so
// fallible calls are matched, never unwrapped.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use crux_core::{
    Core,
    bridge::{Bridge, BridgeError, JsonFfiFormat},
};
use serde::Serialize;

use crate::app::Engine;
use crate::fit::FitError;

type JsonBridge = Bridge<Engine, JsonFfiFormat>;

/// Why an op answered an [`ErrorEnvelope`]; serialised as the envelope's
/// `kind`. The shell keys on `panic_reset` (replay the log into the fresh
/// core) and on the FIT kinds (per-file import reason); any other kind reads
/// as a generic failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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
    /// [`FitError::NotFit`].
    NotFit,
    /// [`FitError::Damaged`].
    Damaged,
    /// [`FitError::NoTrack`] and [`FitError::TooShort`] (context `too_short`).
    NoTrack,
    /// [`FitError::NotARun`] (context: the sport).
    NotARun,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.serialize(f)
    }
}

/// One failed op: the body of `{"error":{"kind":K,"context":C,"message":M}}`.
#[derive(Debug, Serialize)]
pub(crate) struct CoreError {
    kind: ErrorKind,
    context: Cow<'static, str>,
    message: Cow<'static, str>,
}

impl CoreError {
    fn new(
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

    fn unserializable(context: &'static str, err: &serde_json::Error) -> Self {
        Self::new(ErrorKind::Unserializable, context, err.to_string())
    }
}

impl From<&FitError> for ErrorKind {
    fn from(err: &FitError) -> Self {
        match err {
            FitError::NotFit => ErrorKind::NotFit,
            FitError::Damaged => ErrorKind::Damaged,
            FitError::NoTrack | FitError::TooShort => ErrorKind::NoTrack,
            FitError::NotARun(_) => ErrorKind::NotARun,
        }
    }
}

impl From<FitError> for CoreError {
    fn from(err: FitError) -> Self {
        Self::new(ErrorKind::from(&err), err.context().to_owned(), "")
    }
}

/// `{"error":{…}}`: `error` is the only top-level key, so the serialised text
/// starts with `{"error"`, the prefix `CoreWire.kt`'s `isCoreErrorEnvelope`
/// checks, and an envelope is never mistaken for an effect-request array or a
/// `ViewModel`.
#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    error: &'a CoreError,
}

/// Serialise the envelope. An enum and two strings cannot fail to serialise;
/// the literal only keeps this path free of `unwrap`.
fn error_envelope(err: &CoreError) -> Vec<u8> {
    serde_json::to_vec(&ErrorEnvelope { error: err })
        .unwrap_or_else(|_| br#"{"error":"error envelope unavailable"}"#.to_vec())
}

/// The bytes a JSON op answers: its payload, or the envelope of its error.
fn answer(result: Result<Vec<u8>, CoreError>) -> Vec<u8> {
    result.unwrap_or_else(|err| error_envelope(&err))
}

/// The panic firewall every op runs through: a panic in `op` is caught and
/// becomes a [`ErrorKind::Panic`] error for `context` instead of unwinding
/// across the FFI boundary.
///
/// `AssertUnwindSafe` is sound: the state an op can leave half-updated is the
/// bridge, whose lock recovers from poison and whose update path replaces the
/// core after a panic ([`CoreBridge::update_with`]), and the GPX registry,
/// which every view rebuilds.
fn guarded<T>(
    context: &'static str,
    op: impl FnOnce() -> Result<T, CoreError>,
) -> Result<T, CoreError> {
    std::panic::catch_unwind(AssertUnwindSafe(op)).unwrap_or_else(|payload| {
        Err(CoreError::new(
            ErrorKind::Panic,
            context,
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

/// One crux core behind the bridge's JSON wire: the process-wide [`BRIDGE`]
/// the shells drive, or a test's own.
pub(crate) struct CoreBridge {
    bridge: Mutex<JsonBridge>,
}

impl CoreBridge {
    pub(crate) fn new() -> Self {
        Self {
            bridge: Mutex::new(Bridge::new(Core::default())),
        }
    }

    /// Lock the bridge, recovering the guard even if the mutex was poisoned.
    ///
    /// Panicking on a poisoned lock would unwind across the FFI boundary. The
    /// bridge holds plain serde model data with no cross-field lock invariant,
    /// so reusing the inner value after a poison is safe.
    fn lock(&self) -> MutexGuard<'_, JsonBridge> {
        self.bridge.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Replace the core with an empty one, as a fresh process has it.
    pub(crate) fn reset(&self) {
        *self.lock() = Bridge::new(Core::default());
    }

    /// Process a JSON-encoded event, returning the JSON effect-request array.
    ///
    /// An event serde rejects (malformed, or an unknown variant) is dropped:
    /// the answer is empty and the core unchanged. The realistic trigger is
    /// log replay: the shell re-feeds every persisted event on launch, so a
    /// line written by a future app version must be skipped, not turned into
    /// a startup crash-loop.
    pub(crate) fn process_event(&self, event: &[u8]) -> Vec<u8> {
        self.update_with(|bridge| {
            let mut out = Vec::new();
            match bridge.update(event, &mut out) {
                Ok(()) => Ok(out),
                Err(BridgeError::DeserializeEvent(_)) => Ok(Vec::new()),
                Err(err) => Err(CoreError::new(
                    ErrorKind::Unserializable,
                    "update",
                    err.to_string(),
                )),
            }
        })
    }

    /// The current view model as JSON.
    pub(crate) fn view(&self) -> Vec<u8> {
        self.view_with(|bridge| {
            let mut out = Vec::new();
            bridge.view(&mut out).map_err(|err| {
                CoreError::new(ErrorKind::Unserializable, "view", err.to_string())
            })?;
            Ok(out)
        })
    }

    /// The update path: `op` runs on the locked bridge. A panic there runs
    /// under crux's model-`RwLock` WRITE guard and poisons it, so every later
    /// call would re-panic on its `.expect(...)` and freeze the session; the
    /// core is replaced instead and the answer is `panic_reset`, on which the
    /// shell replays its persisted log into the clean core.
    fn update_with(&self, op: impl FnOnce(&JsonBridge) -> Result<Vec<u8>, CoreError>) -> Vec<u8> {
        answer(guarded("update", || op(&self.lock())).map_err(|err| {
            if err.kind != ErrorKind::Panic {
                return err;
            }
            self.reset();
            CoreError {
                kind: ErrorKind::PanicReset,
                ..err
            }
        }))
    }

    /// The view path: `op` runs on the locked bridge. `Core::view` takes only
    /// the model's READ guard, which a panic does not poison, so the core is
    /// kept and the answer is the non-resetting `panic` kind; the shell keeps
    /// its last good view.
    fn view_with(&self, op: impl FnOnce(&JsonBridge) -> Result<Vec<u8>, CoreError>) -> Vec<u8> {
        answer(guarded("view", || op(&self.lock())))
    }
}

static BRIDGE: LazyLock<CoreBridge> = LazyLock::new(CoreBridge::new);

/// [`CoreBridge::reset`] on the shells' core. Android calls it only in the
/// host-JVM QA build; the iOS app before replaying an imported log in process.
pub fn reset_bridge() {
    BRIDGE.reset();
}

/// [`CoreBridge::process_event`] on the shells' core. The answer is the effect
/// request array, empty for a dropped event, or an [`ErrorEnvelope`]:
/// `panic_reset` after a caught core panic, `unserializable` when the requests
/// could not be serialised.
pub fn process_event(event: &[u8]) -> Vec<u8> {
    BRIDGE.process_event(event)
}

/// [`CoreBridge::view`] on the shells' core: the `ViewModel` JSON, or an
/// [`ErrorEnvelope`] (`panic`, `unserializable`) that leaves the core intact.
pub fn current_view() -> Vec<u8> {
    BRIDGE.view()
}

/// Parse `.fit` bytes ([`crate::fit::parse_fit`], then
/// [`crate::fit::import_gate`]) into the tracker-import JSON. Pure ingestion: no
/// bridge, model or event log; the shell builds its own `LogRunTrack` event.
/// Success: the [`crate::fit::FitTrack`] serialization. Failure: the
/// [`ErrorEnvelope`] of the [`FitError`], or of a caught parser panic.
pub fn parse_fit_json(bytes: &[u8]) -> Vec<u8> {
    answer(guarded("parse_fit", || {
        let track = crate::fit::parse_fit(bytes)?;
        crate::fit::import_gate(&track)?;
        serde_json::to_vec(&track).map_err(|err| CoreError::unserializable("parse_fit", &err))
    }))
}

/// The figures a `LogRunTrack` will be saved with, before it is sent:
/// `{"distance_km":D,"duration_min":M}` from [`crate::app::preview_track_figures`]
/// (the same QC and derivation the saved run's card uses), so the tracker's
/// Finish review shows exactly what History will list. Input is the event's
/// own wire JSON, the bytes Save then sends; anything else (or a panic)
/// returns the [`ErrorEnvelope`]. Pure: no bridge, model or log.
pub fn track_figures_json(event: &[u8]) -> Vec<u8> {
    #[derive(Serialize)]
    struct Figures {
        distance_km: f64,
        duration_min: f64,
    }
    const CONTEXT: &str = "track_figures";
    answer(guarded(CONTEXT, || {
        let event = serde_json::from_slice::<crate::event::Event>(event)
            .map_err(|err| CoreError::new(ErrorKind::BadInput, CONTEXT, err.to_string()))?;
        let crate::event::Event::LogRunTrack {
            points,
            segment_starts,
            ..
        } = event
        else {
            return Err(CoreError::new(
                ErrorKind::BadInput,
                CONTEXT,
                "not a LogRunTrack event",
            ));
        };
        let (distance_km, duration_min) =
            crate::app::preview_track_figures(points, &segment_starts);
        serde_json::to_vec(&Figures {
            distance_km,
            duration_min,
        })
        .map_err(|err| CoreError::unserializable(CONTEXT, &err))
    }))
}

/// The GPX 1.1 document for ONE logged run, located by the same
/// `(entry_id, observed_at)` identity the delete/amend matcher uses (a legacy
/// `entry_id` of 0 is disambiguated by `observed_at`). The shell calls this only
/// for the run it inspects (its `has_gpx` flag gates the map / share
/// affordances). Empty for a hand-entered run, an unknown id, a track with no
/// exportable route, or a caught panic: the shell shares any non-blank answer
/// as a `.gpx` file, so an error envelope would be shared as a route.
///
/// Reads the registry the last [`current_view`] populated; the shell always
/// renders the run list before it can tap a run to export.
pub fn export_gpx(entry_id: u64, observed_at: i64) -> String {
    guarded("export_gpx", || {
        Ok(crate::app::export_run_gpx(entry_id, observed_at))
    })
    .unwrap_or_default()
}

/// Compact the shell's event log ([`crate::log::compact_lines`]): `read_lines`
/// yields one [`crate::log::LineFacts`] per log line (each parsed as it is
/// read, so a multi-year GPS log is never held decoded), and the answer is the
/// [`crate::log::Compaction`] verdict as JSON. `None` when a line can't be
/// read, the verdict can't be serialised or a panic is caught: the shell then
/// keeps every line. Not an error envelope: the shell decodes the answer with
/// every verdict field defaulted, so an envelope would read as a verdict.
pub fn compact_log_json<E>(
    read_lines: impl FnOnce() -> Result<Vec<crate::log::LineFacts>, E>,
) -> Option<String> {
    const CONTEXT: &str = "compact_log";
    guarded(CONTEXT, || {
        let facts = read_lines().map_err(|_| {
            CoreError::new(ErrorKind::BadInput, CONTEXT, "a log line could not be read")
        })?;
        serde_json::to_string(&crate::log::compact_lines(&facts))
            .map_err(|err| CoreError::unserializable(CONTEXT, &err))
    })
    .ok()
}

/// Test hook for a shell's bridge tests (feature `test-hooks`, never in an app
/// build): panic while holding the bridge lock, exactly the poison case the
/// firewall guards. `reset` selects the update path (`panic_reset`, core
/// replaced) or the view path (`panic`, core kept).
#[cfg(feature = "test-hooks")]
pub fn panic_for_test(reset: bool) -> Vec<u8> {
    if reset {
        BRIDGE.update_with(|_| panic!("test-hooks panic"))
    } else {
        BRIDGE.view_with(|_| panic!("test-hooks panic"))
    }
}

#[cfg(any(feature = "c-abi", test))]
pub mod c_abi;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

#[cfg(any(target_os = "android", feature = "jni-host"))]
mod android {
    //! Each export runs its body through `EnvUnowned::with_env`, which wraps it
    //! in its own `catch_unwind`, and resolves the outcome with
    //! `LogErrorAndDefault`. The bodies always return `Ok`, so the policy only
    //! ever sees a panic escaping the JNI glue itself, which it logs and maps to
    //! a null return. Not `ThrowRuntimeExAndDefault`: `Core.kt` never catches
    //! exceptions from these calls. Core panics never reach the policy:
    //! [`super::guarded`] turns them into error payloads first.

    use super::{
        CoreError, ErrorKind, compact_log_json, current_view, error_envelope, export_gpx,
        parse_fit_json, process_event, track_figures_json,
    };
    use crate::log::LineFacts;
    use jni::errors::LogErrorAndDefault;
    use jni::objects::{JByteArray, JClass, JObjectArray, JString};
    use jni::sys::jlong;
    use jni::{Env, EnvUnowned};

    /// Test-only bridge reset for the host-JVM UI/UX QA suite, so each
    /// Robolectric scenario starts from an empty core. Gated on `jni-host`
    /// alone, so it never ships in a device APK.
    #[cfg(feature = "jni-host")]
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_milestone_Core_resetForTest(
        _env: EnvUnowned<'_>,
        _class: JClass<'_>,
    ) {
        super::reset_bridge();
    }

    /// Build a JNI byte array from `out`, or null when the allocation fails
    /// (e.g. OOM on a huge payload). The failed allocation leaves a throwable
    /// pending, which would throw in Kotlin on return; it is cleared, so the
    /// answer is the null every `Core.kt` native already declares and handles.
    fn byte_array_or_null<'local>(env: &mut Env<'local>, out: &[u8]) -> JByteArray<'local> {
        env.byte_array_from_slice(out).unwrap_or_else(|_| {
            env.exception_clear();
            JByteArray::default()
        })
    }

    /// Build a JNI string from `s`, or null when the allocation fails, with the
    /// same cleared throwable as [`byte_array_or_null`]. A very large payload
    /// (a huge FIT import, a long GPX track) is a realistic trigger.
    fn string_or_null<'local>(env: &mut Env<'local>, s: &str) -> JString<'local> {
        env.new_string(s).unwrap_or_else(|_| {
            env.exception_clear();
            JString::default()
        })
    }

    /// Run the JSON op `f` on the input bytes and return its answer as a JNI
    /// string. A failed byte-array read (jni clears that throwable) answers the
    /// `bad_input` envelope for `context`.
    fn json_from_bytes<'local>(
        env: &mut Env<'local>,
        input: &JByteArray<'local>,
        context: &'static str,
        f: fn(&[u8]) -> Vec<u8>,
    ) -> JString<'local> {
        let json = match env.convert_byte_array(input) {
            Ok(bytes) => f(&bytes),
            Err(err) => error_envelope(&CoreError::new(
                ErrorKind::BadInput,
                context,
                err.to_string(),
            )),
        };
        // serde_json writes UTF-8, so this borrows and never replaces a byte.
        string_or_null(env, &String::from_utf8_lossy(&json))
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_milestone_Core_update<'local>(
        mut env: EnvUnowned<'local>,
        _class: JClass<'local>,
        event: JByteArray<'local>,
    ) -> JByteArray<'local> {
        env.with_env(|env| -> jni::errors::Result<_> {
            // A failure to read the event byte array (e.g. OOM copying a large
            // event; jni clears that throwable itself) is treated exactly like
            // a dropped event: no effect requests.
            let out = match env.convert_byte_array(&event) {
                Ok(bytes) => process_event(&bytes),
                Err(_) => Vec::new(),
            };
            Ok(byte_array_or_null(env, &out))
        })
        .resolve::<LogErrorAndDefault>()
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_milestone_Core_view<'local>(
        mut env: EnvUnowned<'local>,
        _class: JClass<'local>,
    ) -> JByteArray<'local> {
        env.with_env(|env| -> jni::errors::Result<_> {
            Ok(byte_array_or_null(env, &current_view()))
        })
        .resolve::<LogErrorAndDefault>()
    }

    /// Pure FIT-file parsing helper, deliberately NOT wired through
    /// `update`/`view`: no model, no event log. Takes the raw `.fit` file
    /// bytes, returns the [`parse_fit_json`] JSON string (segments of fixes
    /// on success, `{"error":...}` on failure). The shell converts a success
    /// payload into its existing `LogRunTrack` event itself.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_milestone_Core_parseFit<'local>(
        mut env: EnvUnowned<'local>,
        _class: JClass<'local>,
        fit_bytes: JByteArray<'local>,
    ) -> JString<'local> {
        env.with_env(|env| -> jni::errors::Result<_> {
            Ok(json_from_bytes(
                env,
                &fit_bytes,
                "parse_fit",
                parse_fit_json,
            ))
        })
        .resolve::<LogErrorAndDefault>()
    }

    /// The GPX 1.1 document for ONE logged run ([`export_gpx`]); an empty
    /// string for a run with no exportable route or a caught core panic
    /// (`with_env`'s own catch would return null instead).
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_milestone_Core_exportGpx<'local>(
        mut env: EnvUnowned<'local>,
        _class: JClass<'local>,
        entry_id: jlong,
        observed_at: jlong,
    ) -> JString<'local> {
        env.with_env(|env| -> jni::errors::Result<_> {
            // `entry_id`/`observed_at` arrive as the shell's Long fields; a
            // bit-cast to the core's u64/i64 identity is exact (ids are positive
            // epoch-millis).
            let gpx = export_gpx(entry_id as u64, observed_at);
            Ok(string_or_null(env, &gpx))
        })
        .resolve::<LogErrorAndDefault>()
    }

    /// Pure preview of a track's of-record figures ([`track_figures_json`]),
    /// NOT wired through `update`/`view`: takes a `LogRunTrack` event's wire
    /// JSON, returns `{"distance_km":…,"duration_min":…}` or `{"error":…}`.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_milestone_Core_trackFigures<'local>(
        mut env: EnvUnowned<'local>,
        _class: JClass<'local>,
        event: JByteArray<'local>,
    ) -> JString<'local> {
        env.with_env(|env| -> jni::errors::Result<_> {
            Ok(json_from_bytes(
                env,
                &event,
                "track_figures",
                track_figures_json,
            ))
        })
        .resolve::<LogErrorAndDefault>()
    }

    /// Compact the shell's event log ([`compact_log_json`]): takes its lines,
    /// returns the [`crate::log::Compaction`] verdict as JSON. Each line is
    /// read and parsed in its own local frame. Null when a line can't be read
    /// or the core can't answer: the shell then keeps every line.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_milestone_Core_compactLog<'local>(
        mut env: EnvUnowned<'local>,
        _class: JClass<'local>,
        lines: JObjectArray<'local, JString<'local>>,
    ) -> JString<'local> {
        env.with_env(|env| -> jni::errors::Result<_> {
            let verdict = compact_log_json(|| -> jni::errors::Result<_> {
                let n = lines.len(env)?;
                let mut facts = Vec::with_capacity(n);
                for i in 0..n {
                    let line = env.with_local_frame(1, |env| -> jni::errors::Result<_> {
                        lines.get_element(env, i)?.try_to_string(env)
                    })?;
                    facts.push(LineFacts::parse(&line));
                }
                Ok(facts)
            });
            match verdict {
                Some(json) => Ok(string_or_null(env, &json)),
                None => {
                    // A failed line read can leave its throwable pending.
                    env.exception_clear();
                    Ok(JString::default())
                }
            }
        })
        .resolve::<LogErrorAndDefault>()
    }
}
