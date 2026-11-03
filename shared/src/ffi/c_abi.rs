//! C ABI over the platform-neutral bridge API ([`crate::ffi`]), feature
//! `c-abi`; the header is `shared/include/lollop.h` (cbindgen,
//! `shared/cbindgen.toml`).
//!
//! The iOS app links the `staticlib` build of this crate (`libshared.a`,
//! scripts/build-ios-core.sh) and calls it from Kotlin/Native through cinterop
//! (`kmp/core-bridge`). Every function here is a thin wrapper: it turns C
//! pointers into slices, calls the `ffi` function the Android JNI export
//! calls, and hands the answer back as a core-owned byte buffer. The wire is
//! the JNI one: UTF-8 JSON in, UTF-8 JSON out (GPX text for
//! [`lollop_export_gpx`]).
//!
//! Memory: every [`LollopBytes`] a function returns is owned by the caller and
//! released with exactly one [`lollop_bytes_free`]. An empty answer is
//! `{NULL, 0}`; freeing it is a no-op. Input pointers are borrowed for the
//! call only.
//!
//! Panics: the `ffi` functions catch core panics and return an `{"error":…}`
//! object or an empty answer, so nothing unwinds into the caller. This module
//! itself calls nothing that panics.
//!
//! Threading: callable from any thread; see the `ffi` note on keeping
//! view-producing calls on one thread.

use crate::ffi;

/// A byte buffer the core allocated and the caller now owns. Release it with
/// [`lollop_bytes_free`]. `{NULL, 0}` is the empty answer.
#[repr(C)]
pub struct LollopBytes {
    pub data: *mut u8,
    pub len: usize,
}

/// A byte slice the caller owns, borrowed for one call (one event-log line for
/// [`lollop_compact_log`]).
#[repr(C)]
pub struct LollopSlice {
    pub data: *const u8,
    pub len: usize,
}

/// The `{NULL, 0}` empty answer.
const EMPTY: LollopBytes = LollopBytes {
    data: std::ptr::null_mut(),
    len: 0,
};

impl LollopBytes {
    fn from_vec(bytes: Vec<u8>) -> Self {
        if bytes.is_empty() {
            return EMPTY;
        }
        let boxed = bytes.into_boxed_slice();
        let len = boxed.len();
        Self {
            data: Box::into_raw(boxed).cast::<u8>(),
            len,
        }
    }
}

/// Borrow `len` bytes at `data`; a null pointer or zero length is empty.
///
/// # Safety
/// A non-null `data` must point to `len` readable bytes that stay valid and
/// unmodified for `'a`.
unsafe fn borrow<'a>(data: *const u8, len: usize) -> &'a [u8] {
    if data.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the caller's contract above.
        unsafe { std::slice::from_raw_parts(data, len) }
    }
}

/// Process one event's wire JSON (`Core.update`). Returns the effect-request
/// array, the empty answer for a dropped (malformed or unknown) event, or a
/// `{"error":{"kind":"panic_reset",…}}` object after a caught core panic, in
/// which case the core was replaced and the shell replays its log.
///
/// # Safety
/// `event` must point to `len` readable bytes (or be null with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lollop_update(event: *const u8, len: usize) -> LollopBytes {
    // SAFETY: forwarded caller contract.
    LollopBytes::from_vec(ffi::process_event(unsafe { borrow(event, len) }))
}

/// The current `ViewModel` JSON (`Core.view`), or a non-resetting
/// `{"error":{"kind":"panic",…}}` object.
#[unsafe(no_mangle)]
pub extern "C" fn lollop_view() -> LollopBytes {
    LollopBytes::from_vec(ffi::current_view())
}

/// Parse a `.fit` file's bytes into the tracker-import JSON or an `{"error":…}`
/// object (`Core.parseFit`).
///
/// # Safety
/// `data` must point to `len` readable bytes (or be null with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lollop_parse_fit(data: *const u8, len: usize) -> LollopBytes {
    // SAFETY: forwarded caller contract.
    LollopBytes::from_vec(ffi::parse_fit_json(unsafe { borrow(data, len) }))
}

/// The figures a `LogRunTrack` event will be saved with,
/// `{"distance_km":D,"duration_min":M}`, or an `{"error":…}` object
/// (`Core.trackFigures`).
///
/// # Safety
/// `event` must point to `len` readable bytes (or be null with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lollop_track_figures(event: *const u8, len: usize) -> LollopBytes {
    // SAFETY: forwarded caller contract.
    LollopBytes::from_vec(ffi::track_figures_json(unsafe { borrow(event, len) }))
}

/// The GPX document for one logged run, by its `(entry_id, observed_at)`
/// identity (`Core.exportGpx`); empty when the run has no exportable route.
/// `entry_id` is the shell's signed 64-bit id, bit-cast like the JNI export.
#[unsafe(no_mangle)]
pub extern "C" fn lollop_export_gpx(entry_id: i64, observed_at: i64) -> LollopBytes {
    LollopBytes::from_vec(ffi::export_gpx(entry_id as u64, observed_at).into_bytes())
}

/// The compaction verdict JSON for the event log's `count` lines
/// (`Core.compactLog`); empty when the core can't answer, and the shell then
/// keeps every line. A line that is not UTF-8 is read lossily (invalid bytes
/// become U+FFFD).
///
/// # Safety
/// `lines` must point to `count` [`LollopSlice`]s (or be null with `count` 0),
/// each pointing to its `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lollop_compact_log(
    lines: *const LollopSlice,
    count: usize,
) -> LollopBytes {
    let lines: &[LollopSlice] = if lines.is_null() || count == 0 {
        &[]
    } else {
        // SAFETY: the caller's contract above.
        unsafe { std::slice::from_raw_parts(lines, count) }
    };
    let verdict = ffi::compact_log_json(|| -> Result<_, std::convert::Infallible> {
        Ok(lines
            .iter()
            .map(|line| {
                // SAFETY: the caller's contract above.
                let bytes = unsafe { borrow(line.data, line.len) };
                crate::log::LineFacts::parse(&String::from_utf8_lossy(bytes))
            })
            .collect())
    });
    verdict.map_or(EMPTY, |json| LollopBytes::from_vec(json.into_bytes()))
}

/// Release a buffer a `lollop_*` function returned. A `{NULL, 0}` buffer is a
/// no-op.
///
/// # Safety
/// `bytes` must come unmodified from a `lollop_*` function and not have been
/// freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lollop_bytes_free(bytes: LollopBytes) {
    if bytes.data.is_null() {
        return;
    }
    // SAFETY: `data`/`len` are the parts of the boxed slice `from_vec` leaked.
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(bytes.data, bytes.len)) });
}

/// Replace the core with an empty one, so each bridge test starts clean.
#[cfg(feature = "test-hooks")]
#[unsafe(no_mangle)]
pub extern "C" fn lollop_reset_for_test() {
    ffi::reset_bridge();
}

/// Run a core call that panics while holding the core lock and return the
/// firewall's answer: `reset` true takes the `update` path (`panic_reset`,
/// core replaced), false the `view` path (`panic`, core kept).
#[cfg(feature = "test-hooks")]
#[unsafe(no_mangle)]
pub extern "C" fn lollop_panic_for_test(reset: bool) -> LollopBytes {
    LollopBytes::from_vec(ffi::panic_for_test(reset))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
