//! The bridge the native shells drive: the crux [`Engine`](crate::app::Engine)
//! behind a JSON wire. Every operation is declared once in [`ops`]; two entry
//! points take an op's code and its input and add no logic of their own: the
//! Android JNI exports (`Java_app_milestone_core_JniCore_*`, `jni`) and the
//! C ABI (`lollo_*`, `c_abi`, linked into the iOS app as a staticlib).
//! `bridge` answers each op. [`constants`] are the Rust constants the shells
//! read.

pub mod constants;
pub mod ops;

#[cfg(any(ffi_surface, test))]
mod bridge;

#[cfg(any(feature = "c-abi", test))]
pub mod c_abi;

#[cfg(any(target_os = "android", feature = "jni-host"))]
mod jni;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
