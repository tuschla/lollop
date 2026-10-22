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

// Everything above `mod tests` is glue consumed only by the JNI exports in
// `mod android` (android target or the `jni-host` QA feature) and by the tests,
// so each item carries that exact cfg: a plain host build compiles none of it,
// and a helper that genuinely becomes unused is still flagged.
#[cfg(any(target_os = "android", feature = "jni-host", test))]
use std::sync::{LazyLock, Mutex, MutexGuard};

#[cfg(any(target_os = "android", feature = "jni-host", test))]
use crux_core::{
    Core,
    bridge::{Bridge, JsonFfiFormat},
};
#[cfg(any(target_os = "android", feature = "jni-host", test))]
use serde::Serialize;

#[cfg(any(target_os = "android", feature = "jni-host", test))]
use crate::app::Engine;

#[cfg(any(target_os = "android", feature = "jni-host", test))]
type JsonBridge = Bridge<Engine, JsonFfiFormat>;

#[cfg(any(target_os = "android", feature = "jni-host", test))]
static BRIDGE: LazyLock<Mutex<JsonBridge>> =
    LazyLock::new(|| Mutex::new(Bridge::new(Core::default())));

/// The plain-string FIT error envelope, used when the parser panics or its
/// payload cannot be produced. Kotlin reads any `{"error":…}` without an
/// object body as a damaged file.
#[cfg(any(target_os = "android", feature = "jni-host", test))]
const FIT_FALLBACK_ERR: &str = r#"{"error":"Couldn't read this FIT file"}"#;

/// Lock the bridge, recovering the guard even if the mutex was poisoned.
///
/// Panicking on a poisoned lock would unwind across the JNI boundary (see the
/// module-level panic-firewall note). The bridge holds plain serde model data
/// with no cross-field lock invariant, so reusing the inner value after a
/// poison is safe; recover it rather than crash.
#[cfg(any(target_os = "android", feature = "jni-host", test))]
fn lock_bridge() -> MutexGuard<'static, JsonBridge> {
    BRIDGE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Replace the global bridge with a fresh `Engine` bridge, discarding a
/// poisoned or otherwise wedged core.
///
/// A panic inside an `update` arm poisons crux's *inner* model `RwLock` (held
/// under the [`Core`]'s own `RwLock`, not the outer [`Mutex`] this module
/// recovers), so every later `update`/`view` would re-panic on that inner
/// `.expect(...)` and the session would freeze for the process lifetime. The
/// panic firewall therefore swaps in a fresh bridge (see [`catch_panic`]); the
/// shell then replays the persisted event log into the clean core (the
/// offending event was never persisted), exactly like a cold-start restore.
/// Also the reset hook host-side test isolation uses.
#[cfg(any(target_os = "android", feature = "jni-host", test))]
pub(crate) fn reset_bridge() {
    *lock_bridge() = Bridge::new(Core::default());
}

/// `{"error":{"kind":K,"context":C,"message":M}}`: the one error object shape
/// shared by the panic firewall and the FIT import. `error` is the only
/// top-level key, so the serialized text starts with `{"error"`, the prefix
/// `Core.kt`'s `isCoreErrorEnvelope` checks.
#[cfg(any(target_os = "android", feature = "jni-host", test))]
#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

#[cfg(any(target_os = "android", feature = "jni-host", test))]
#[derive(Serialize)]
struct ErrorBody<'a> {
    kind: &'a str,
    context: &'a str,
    message: &'a str,
}

/// Serialize the error envelope. Serializing three strings cannot fail; the
/// fallback only keeps this path free of `unwrap`.
#[cfg(any(target_os = "android", feature = "jni-host", test))]
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
#[cfg(any(target_os = "android", feature = "jni-host", test))]
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
#[cfg(any(target_os = "android", feature = "jni-host", test))]
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
/// surfaces as the non-resetting `panic` error object (see [`catch_panic`]):
/// `view` only takes the model READ guard, which neither can poison, so the
/// bridge stays healthy and is not replaced. Never a process abort.
#[cfg(any(target_os = "android", feature = "jni-host", test))]
fn current_view() -> Vec<u8> {
    catch_panic("view", false, || {
        let mut out = Vec::new();
        match lock_bridge().view(&mut out) {
            Ok(()) => out,
            Err(err) => error_envelope("panic", "view", &format!("view should serialize: {err:?}")),
        }
    })
}

/// Parse Garmin/ANT `.fit` activity bytes ([`crate::fit::parse_fit`]) into a
/// JSON payload for the Android shell's tracker import. This is deliberately
/// NOT routed through the bridge/model/event log, it is pure data ingestion
/// (mirrors how `running.rs` treats an already-decoded `GpsPoint` track), so
/// the shell converts the result into its own `LogRunTrack` event itself.
/// Nothing here is recommendation-bearing, so no `Evidence` is attached
/// (evidence gating covers coaching claims, not raw sensor ingest).
///
/// Success: [`crate::fit::FitTrack`]'s serialization,
/// `{"segments":[[{"lat":..,"lon":..,"time_sec":..,"hr_bpm":123,"accuracy_m":8.0}, ...], ...]}`
/// (`hr_bpm` and `accuracy_m` are each omitted when absent, never a fabricated
/// 0/null placeholder, `accuracy_m` is present only when the FIT file actually
/// recorded `record.gps_accuracy`, so the shell can tell a real device figure
/// from "unknown" and apply its own sentinel; see `Gpx.kt`'s `importedRunEvent`).
/// Failure: `{"error":{"kind":K,"context":C,"message":M}}`, the same object shape
/// as the panic envelope ([`crate::fit::FitError::kind`]: `not_fit`, `damaged`,
/// `no_track`, `not_a_run`; `context` is the recorded sport in plain lowercase
/// words for `not_a_run`, e.g. `"cycling"`, `"too_short"` for a `no_track` whose
/// track fails [`crate::fit::import_gate`], else empty; `message` is the
/// plain-language toast). A panic inside the parser degrades to the
/// plain-string [`FIT_FALLBACK_ERR`] form.
#[cfg(any(target_os = "android", feature = "jni-host", test))]
fn parse_fit_json(bytes: &[u8]) -> Vec<u8> {
    // FIT parsing never touches the bridge, so a fitparser panic must NOT reset
    // the engine (only update resets the bridge): catch it here and return
    // the plain FIT error envelope, leaving the model untouched.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || match crate::fit::parse_fit(bytes)
            .and_then(|track| crate::fit::import_gate(&track).map(|()| track))
        {
            Ok(track) => {
                serde_json::to_vec(&track).unwrap_or_else(|_| FIT_FALLBACK_ERR.as_bytes().to_vec())
            }
            Err(err) => error_envelope(err.kind(), err.context(), &err.message()),
        },
    ))
    .unwrap_or_else(|_| FIT_FALLBACK_ERR.as_bytes().to_vec())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

#[cfg(any(target_os = "android", feature = "jni-host"))]
mod android {
    //! Every fallible JNI step degrades in place rather than `.expect()`-panicking
    //! across the `extern "system"` boundary (since Rust 1.81 an unwind out of
    //! such a fn aborts the process, which the shell can never intercept).
    //!
    //! Each export runs its body through `EnvUnowned::with_env`, which wraps it
    //! in its own `catch_unwind`, and resolves the outcome with
    //! `LogErrorAndDefault`. The bodies always return `Ok`, so the policy only
    //! ever sees a panic escaping the JNI glue itself, which it logs and maps to
    //! a null return. Not `ThrowRuntimeExAndDefault`: `Core.kt` never catches
    //! exceptions from these calls. Core panics never reach the policy:
    //! [`super::catch_panic`] turns them into error payloads first and, on the
    //! update path, resets the bridge, which `with_env`'s generic catch cannot do.

    use super::{FIT_FALLBACK_ERR, current_view, parse_fit_json, process_event};
    use jni::errors::LogErrorAndDefault;
    use jni::objects::{JByteArray, JClass, JString};
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
        // Every fallible step here, reading the JNI byte array (e.g. OOM on the
        // 32 MB FIT cap; jni clears that throwable itself), parsing the FIT,
        // and allocating the return JNI string, degrades to the shell's
        // `{"error":…}` envelope. `parse_fit_json` is itself panic-firewalled,
        // so a FIT-parser panic is caught inside it.
        env.with_env(|env| -> jni::errors::Result<_> {
            let json: String = match env.convert_byte_array(&fit_bytes) {
                // `serde_json` only emits UTF-8, so this cannot fail; degrade to
                // the error envelope rather than unwrap so the firewall holds
                // regardless.
                Ok(bytes) => String::from_utf8(parse_fit_json(&bytes))
                    .unwrap_or_else(|_| FIT_FALLBACK_ERR.to_string()),
                Err(_) => FIT_FALLBACK_ERR.to_string(),
            };
            Ok(string_or_null(env, &json, FIT_FALLBACK_ERR))
        })
        .resolve::<LogErrorAndDefault>()
    }

    /// Build the GPX 1.1 document for ONE logged run on demand, located by
    /// the same `(entry_id, observed_at)` identity the delete/amend matcher uses
    /// (a legacy `entry_id` of 0 is disambiguated by `observed_at`). The list
    /// payload no longer carries a per-run GPX string — that was a ~0.5 MB blob
    /// re-serialized over JNI + re-parsed on every event; the shell calls this
    /// only for the one run it inspects (its `has_gpx` flag already gates the map
    /// / share affordances). Returns an empty string for a hand-entered run, an
    /// unknown id, or a track with no exportable route.
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
}
