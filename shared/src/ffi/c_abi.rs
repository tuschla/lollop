//! The C entry points, feature `c-abi`; the header is
//! `shared/include/lollop.h` (cbindgen, `shared/cbindgen.toml`).
//!
//! The iOS app links the `staticlib` build of this crate (`libshared.a`,
//! scripts/build-ios-core.sh) and calls it from Kotlin/Native through cinterop
//! (`android/core-bridge`). [`lollop_call`] and [`lollop_call_lines`] take an
//! op's code (`shared::ffi::ops::Op`) and its input and answer it as a
//! core-owned byte buffer: UTF-8 JSON (GPX text for the GPX export).
//!
//! Memory: every [`LollopBytes`] a function returns is owned by the caller and
//! released with exactly one [`lollop_bytes_free`]. An empty answer is
//! `{NULL, 0}`; freeing it is a no-op. Input pointers are borrowed for the
//! call only.

use std::borrow::Cow;

use super::bridge::{self, Input};

/// A byte buffer the core allocated and the caller now owns. Release it with
/// [`lollop_bytes_free`]. `{NULL, 0}` is the empty answer.
#[repr(C)]
pub struct LollopBytes {
    pub data: *mut u8,
    pub len: usize,
}

/// A byte slice the caller owns, borrowed for one call (one line for
/// [`lollop_call_lines`]).
#[repr(C)]
pub struct LollopSlice {
    pub data: *const u8,
    pub len: usize,
}

impl LollopBytes {
    fn from_vec(bytes: Vec<u8>) -> Self {
        if bytes.is_empty() {
            return Self {
                data: std::ptr::null_mut(),
                len: 0,
            };
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

/// Answer op `op` on the `len` bytes at `input`.
///
/// # Safety
/// `input` must point to `len` readable bytes (or be null with `len` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lollop_call(op: u32, input: *const u8, len: usize) -> LollopBytes {
    // SAFETY: forwarded caller contract.
    let input = unsafe { borrow(input, len) };
    LollopBytes::from_vec(bridge::call(op, Input::Bytes(Cow::Borrowed(input))))
}

/// Answer op `op` on the `count` lines at `lines`. A line that is not UTF-8
/// fails the op.
///
/// # Safety
/// `lines` must point to `count` [`LollopSlice`]s (or be null with `count` 0),
/// each pointing to its `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lollop_call_lines(
    op: u32,
    lines: *const LollopSlice,
    count: usize,
) -> LollopBytes {
    let lines: &[LollopSlice] = if lines.is_null() || count == 0 {
        &[]
    } else {
        // SAFETY: the caller's contract above.
        unsafe { std::slice::from_raw_parts(lines, count) }
    };
    let mut read = lines.iter().map(|line| {
        // SAFETY: the caller's contract above.
        let bytes = unsafe { borrow(line.data, line.len) };
        std::str::from_utf8(bytes)
            .map(Cow::Borrowed)
            .map_err(|err| err.to_string())
    });
    LollopBytes::from_vec(bridge::call(op, Input::Lines(&mut read)))
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

/// Panic while holding the core and return the firewall's answer: `reset`
/// true takes the update path (`panic_reset`, core replaced), false the view
/// path (`panic`, core kept).
#[cfg(feature = "test-hooks")]
#[unsafe(no_mangle)]
pub extern "C" fn lollop_panic_for_test(reset: bool) -> LollopBytes {
    LollopBytes::from_vec(bridge::panic_for_test(reset))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
