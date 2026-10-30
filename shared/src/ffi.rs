//! JSON FFI bridge for native shells.
//!
//! Exposes the crux [`Engine`](crate::app::Engine) over a JSON-serialised
//! [`Bridge`](crux_core::bridge::Bridge) so a native
//! shell (Android/Kotlin via JNI) can drive the same side-effect-free core.
//! The wire format is JSON: events in, effect-requests +
//! view-model out. Reuses the existing `serde` derives, no Facet typegen
//! required.
//!
//! Flow for the shell:
//!   1. `update(eventJson)` → JSON array of effect requests (only `Render`).
//!   2. `view()` → JSON `ViewModel` reflecting the new state.
//!
//! Panic firewall: these exports are called across a JNI `extern "system"`
//! boundary, where a Rust unwind is undefined behaviour and, since Rust 1.81,
//! aborts the whole process: an app crash the shell can never intercept. So
//! every core call is wrapped such that no shell input (a poisoned lock, a
//! malformed event, a core panic) unwinds across that boundary; it degrades to
//! a dropped event or a structured error object instead.

// The firewall contract: nothing in this module may panic on its own, so
// fallible calls are matched, never unwrapped.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use crux_core::{
    Core,
    bridge::{Bridge, JsonFfiFormat},
};
use serde::Serialize;

use crate::app::Engine;

type JsonBridge = Bridge<Engine, JsonFfiFormat>;

static BRIDGE: LazyLock<Mutex<JsonBridge>> =
    LazyLock::new(|| Mutex::new(Bridge::new(Core::default())));

/// The plain-string FIT error envelope, used when the FIT payload cannot be
/// produced or the JNI glue fails. Kotlin reads any `{"error":…}` without an
/// object body as a damaged file.
const FIT_FALLBACK_ERR: &str = r#"{"error":"Couldn't read this FIT file"}"#;

/// Lock the bridge, recovering the guard even if the mutex was poisoned.
///
/// Panicking on a poisoned lock would unwind across the JNI boundary (see the
/// module-level panic-firewall note). The bridge holds plain serde model data
/// with no cross-field lock invariant, so reusing the inner value after a
/// poison is safe; recover it rather than crash.
fn lock_bridge() -> MutexGuard<'static, JsonBridge> {
    BRIDGE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Replace the global bridge with a fresh `Engine` bridge, discarding a
/// poisoned core (see [`catch_panic`]). Also the reset hook for host-side test
/// isolation.
fn reset_bridge() {
    *lock_bridge() = Bridge::new(Core::default());
}

/// `{"error":{"kind":K,"context":C,"message":M}}`: the one error object shape
/// shared by the panic firewall and the FIT import. `error` is the only
/// top-level key, so the serialized text starts with `{"error"`, the prefix
/// `Core.kt`'s `isCoreErrorEnvelope` checks.
#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    kind: &'a str,
    context: &'a str,
    message: &'a str,
}

/// Serialize the error envelope. Serializing three strings cannot fail; the
/// fallback only keeps this path free of `unwrap`.
fn error_envelope(kind: &str, context: &str, message: &str) -> Vec<u8> {
    serde_json::to_vec(&ErrorEnvelope {
        error: ErrorBody {
            kind,
            context,
            message,
        },
    })
    .unwrap_or_else(|_| br#"{"error":"error envelope unavailable"}"#.to_vec())
}

/// Run a core call with a panic firewall: any panic is caught and converted to
/// a structured error JSON object instead of unwinding across the JNI boundary
/// (see the module-level panic-firewall note).
///
/// Shape: `{"error":{"kind":"panic_reset","context":"update","message":"..."}}`:
/// an object, never an array, so a shell distinguishes it from the effect-request
/// array / `ViewModel` object by its top-level `"error"` key.
///
/// `reset` governs whether the bridge is replaced on a caught panic: the
/// `update` path passes `true`, because a panic inside an update arm
/// runs under crux's model-`RwLock` WRITE guard and poisons it, so every later
/// call would re-panic on the `.expect(...)` and freeze the session — replacing
/// the bridge and returning `panic_reset` lets the shell replay the persisted log
/// into a clean core. The `view` path passes `false`: `Core::view` only takes the
/// READ guard, which a panic does not poison, so the model is still healthy and a
/// reset would be pure loss — it returns the non-resetting `panic` kind, which the
/// shell already handles by keeping the last good view. `AssertUnwindSafe` is
/// sound: the only state crossing the boundary is the global bridge.
fn catch_panic(context: &str, reset: bool, f: impl FnOnce() -> Vec<u8>) -> Vec<u8> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(out) => out,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic payload");
            let kind = if reset {
                reset_bridge();
                "panic_reset"
            } else {
                "panic"
            };
            error_envelope(kind, context, msg)
        }
    }
}

/// Process a JSON-encoded event, returning the JSON effect-request array.
///
/// A malformed or unknown-variant event is dropped rather than propagated (see
/// the module-level panic-firewall note). The realistic trigger is log replay:
/// the shell re-feeds every persisted event on launch, so a line written by a
/// future app version (a renamed/removed `Event` variant) must be skipped, not
/// turned into an unrecoverable startup crash-loop. Core state is left
/// unchanged and the shell renders the prior view.
///
/// A *panic* inside the core (as opposed to a serde error) is caught by
/// [`catch_panic`] and returned as a structured error object.
fn process_event(event: &[u8]) -> Vec<u8> {
    catch_panic("update", true, || {
        let mut out = Vec::new();
        if lock_bridge().update(event, &mut out).is_err() {
            out.clear();
        }
        out
    })
}

/// Serialize the current view model as JSON. A panic or a serialization error
/// returns the non-resetting `panic` error object (see [`catch_panic`]).
fn current_view() -> Vec<u8> {
    catch_panic("view", false, || {
        let mut out = Vec::new();
        match lock_bridge().view(&mut out) {
            Ok(()) => out,
            Err(err) => error_envelope("panic", "view", &format!("view should serialize: {err:?}")),
        }
    })
}

/// Parse `.fit` bytes ([`crate::fit::parse_fit`], then
/// [`crate::fit::import_gate`]) into the tracker-import JSON. Pure ingestion: no
/// bridge, model or event log; the shell builds its own `LogRunTrack` event.
/// Success: the [`crate::fit::FitTrack`] serialization. Failure: the
/// [`ErrorEnvelope`] object filled from [`crate::fit::FitError`]. A parser panic
/// returns the non-resetting `panic` object (see [`catch_panic`]).
fn parse_fit_json(bytes: &[u8]) -> Vec<u8> {
    catch_panic("parse_fit", false, || {
        match crate::fit::parse_fit(bytes)
            .and_then(|track| crate::fit::import_gate(&track).map(|()| track))
        {
            Ok(track) => {
                serde_json::to_vec(&track).unwrap_or_else(|_| FIT_FALLBACK_ERR.as_bytes().to_vec())
            }
            Err(err) => error_envelope(err.kind(), err.context(), ""),
        }
    })
}

/// The figures a `LogRunTrack` will be saved with, before it is sent:
/// `{"distance_km":D,"duration_min":M}` from [`crate::app::preview_track_figures`]
/// (the same QC and derivation the saved run's card uses), so the tracker's
/// Finish review shows exactly what History will list. Input is the event's
/// own wire JSON, the bytes Save then sends; anything else (or a panic)
/// returns the `{"error":…}` envelope. Pure: no bridge, model or log.
fn track_figures_json(event: &[u8]) -> Vec<u8> {
    #[derive(Serialize)]
    struct Figures {
        distance_km: f64,
        duration_min: f64,
    }
    catch_panic("track_figures", false, || {
        match serde_json::from_slice::<crate::app::Event>(event) {
            Ok(crate::app::Event::LogRunTrack {
                points,
                segment_starts,
                ..
            }) => {
                let (distance_km, duration_min) =
                    crate::app::preview_track_figures(points, &segment_starts);
                serde_json::to_vec(&Figures {
                    distance_km,
                    duration_min,
                })
                .unwrap_or_else(|err| {
                    error_envelope("bad_input", "track_figures", &err.to_string())
                })
            }
            Ok(_) => error_envelope("bad_input", "track_figures", "not a LogRunTrack event"),
            Err(err) => error_envelope("bad_input", "track_figures", &err.to_string()),
        }
    })
}

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
    //! [`super::catch_panic`] turns them into error payloads first.

    use super::{
        FIT_FALLBACK_ERR, current_view, parse_fit_json, process_event, track_figures_json,
    };
    use crate::log::{LineFacts, compact_lines};
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

    /// Build a JNI byte array from `out`. An allocation failure (e.g. OOM on a
    /// huge payload) first retries with an empty array - the smaller allocation
    /// may still succeed - and only then falls back to a null array, a defined
    /// return the shell already handles. Clearing before each fallback
    /// guarantees no throwable is pending on the retry (JNI forbids allocating
    /// with one pending) or on the null return (it would throw in Kotlin).
    fn byte_array_or_null<'local>(env: &mut Env<'local>, out: &[u8]) -> JByteArray<'local> {
        match env.byte_array_from_slice(out) {
            Ok(arr) => arr,
            Err(_) => {
                env.exception_clear();
                env.byte_array_from_slice(&[]).unwrap_or_else(|_| {
                    env.exception_clear();
                    JByteArray::default()
                })
            }
        }
    }

    /// Build a JNI string from `s`, retrying with `fallback` and only then
    /// returning a null string, with the same no-pending-throwable guarantee as
    /// [`byte_array_or_null`]. A very large payload (a huge FIT import, a long
    /// GPX track) is a realistic allocation-failure trigger.
    fn string_or_null<'local>(env: &mut Env<'local>, s: &str, fallback: &str) -> JString<'local> {
        match env.new_string(s) {
            Ok(js) => js,
            Err(_) => {
                env.exception_clear();
                env.new_string(fallback).unwrap_or_else(|_| {
                    env.exception_clear();
                    JString::default()
                })
            }
        }
    }

    /// Run the panic-firewalled `f` on the input bytes and return its JSON as
    /// a JNI string. A failed byte-array read (jni clears that throwable), a
    /// non-UTF-8 result (`serde_json` cannot emit one) or a failed string
    /// allocation degrades to `fallback`.
    fn json_from_bytes<'local>(
        env: &mut Env<'local>,
        input: &JByteArray<'local>,
        f: fn(&[u8]) -> Vec<u8>,
        fallback: &str,
    ) -> JString<'local> {
        let json = match env.convert_byte_array(input) {
            Ok(bytes) => String::from_utf8(f(&bytes)).unwrap_or_else(|_| fallback.to_string()),
            Err(_) => fallback.to_string(),
        };
        string_or_null(env, &json, fallback)
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
            // a malformed/dropped event: no effect requests, the same defined
            // outcome `process_event` returns for an unparseable event.
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
        // `current_view` is itself already panic-firewalled; the only remaining
        // fallible JNI step is building the return array.
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
                parse_fit_json,
                FIT_FALLBACK_ERR,
            ))
        })
        .resolve::<LogErrorAndDefault>()
    }

    /// Build the GPX 1.1 document for ONE logged run on demand, located by
    /// the same `(entry_id, observed_at)` identity the delete/amend matcher uses
    /// (a legacy `entry_id` of 0 is disambiguated by `observed_at`). The shell
    /// calls this only for the run it inspects (its `has_gpx` flag gates the
    /// map / share affordances). Returns an empty string for a hand-entered run,
    /// an unknown id, or a track with no exportable route.
    ///
    /// A core panic degrades to an empty string via the explicit `catch_unwind`
    /// (`with_env`'s own catch would return null instead). The core query reads
    /// the registry the last `view()` populated; the shell always renders the
    /// run list (a `view()`) before it can tap a run to export.
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
            let gpx = std::panic::catch_unwind(|| {
                crate::app::export_run_gpx(entry_id as u64, observed_at)
            })
            .unwrap_or_default();
            Ok(string_or_null(env, &gpx, ""))
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
        const FALLBACK: &str = r#"{"error":"track figures unavailable"}"#;
        env.with_env(|env| -> jni::errors::Result<_> {
            Ok(json_from_bytes(env, &event, track_figures_json, FALLBACK))
        })
        .resolve::<LogErrorAndDefault>()
    }

    /// Compact the shell's event log ([`compact_lines`]): takes its lines,
    /// returns the [`crate::log::Compaction`] verdict as JSON. Each line is
    /// parsed as it is read, so a multi-year GPS log is never held decoded.
    /// Null when a line can't be read or a panic escapes: the shell then keeps
    /// every line.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_milestone_Core_compactLog<'local>(
        mut env: EnvUnowned<'local>,
        _class: JClass<'local>,
        lines: JObjectArray<'local, JString<'local>>,
    ) -> JString<'local> {
        env.with_env(|env| -> jni::errors::Result<_> {
            let verdict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || -> jni::errors::Result<String> {
                    let n = lines.len(env)?;
                    let mut facts = Vec::with_capacity(n);
                    for i in 0..n {
                        let line = env.with_local_frame(1, |env| -> jni::errors::Result<_> {
                            lines.get_element(env, i)?.try_to_string(env)
                        })?;
                        facts.push(LineFacts::parse(&line));
                    }
                    Ok(serde_json::to_string(&compact_lines(&facts)).unwrap_or_default())
                },
            ));
            match verdict {
                Ok(Ok(json)) if !json.is_empty() => Ok(string_or_null(env, &json, "")),
                _ => {
                    env.exception_clear();
                    Ok(JString::default())
                }
            }
        })
        .resolve::<LogErrorAndDefault>()
    }
}
