//! The Android entry points, `JniCore.call` and `JniCore.callLines`
//! (android/core-bridge): read the op's input from the JVM, answer it through
//! the bridge, and hand the answer back as a byte array.
//!
//! Each export runs its body through `EnvUnowned::with_env`, which wraps it in
//! its own `catch_unwind`, and resolves the outcome with `LogErrorAndDefault`.
//! The bodies always return `Ok`, so the policy only ever sees a panic escaping
//! the JNI glue itself, which it logs and maps to a null return. Core panics
//! never reach it: the bridge turns them into error envelopes first.

use std::borrow::Cow;

use jni::errors::LogErrorAndDefault;
use jni::objects::{JByteArray, JObject, JObjectArray, JString};
use jni::sys::jint;
use jni::{Env, EnvUnowned};

use super::bridge::{self, Input};

/// The answer as a Java byte array, or null when it can't be allocated (a
/// huge GPX track or FIT import): the failed allocation's pending throwable is
/// cleared, so the binding sees the null its signature declares.
fn java_bytes<'local>(env: &mut Env<'local>, out: &[u8]) -> JByteArray<'local> {
    env.byte_array_from_slice(out).unwrap_or_else(|_| {
        env.exception_clear();
        JByteArray::default()
    })
}

/// A negative code names no op.
fn op_code(op: jint) -> u32 {
    u32::try_from(op).unwrap_or(u32::MAX)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_milestone_core_JniCore_call<'local>(
    mut env: EnvUnowned<'local>,
    _this: JObject<'local>,
    op: jint,
    input: JByteArray<'local>,
) -> JByteArray<'local> {
    env.with_env(|env| -> jni::errors::Result<_> {
        let code = op_code(op);
        let out = match env.convert_byte_array(&input) {
            Ok(bytes) => bridge::call(code, Input::Bytes(Cow::Owned(bytes))),
            Err(err) => bridge::unreadable_input(code, err.to_string()),
        };
        Ok(java_bytes(env, &out))
    })
    .resolve::<LogErrorAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_milestone_core_JniCore_callLines<'local>(
    mut env: EnvUnowned<'local>,
    _this: JObject<'local>,
    op: jint,
    lines: JObjectArray<'local, JString<'local>>,
) -> JByteArray<'local> {
    env.with_env(|env| -> jni::errors::Result<_> {
        let code = op_code(op);
        let out = match lines.len(env) {
            Ok(count) => {
                // Each line is read in its own local frame, so the array's
                // strings are never all held as Rust copies at once.
                let mut read = (0..count).map(|i| {
                    env.with_local_frame(1, |env| -> jni::errors::Result<_> {
                        lines.get_element(env, i)?.try_to_string(env)
                    })
                    .map(Cow::Owned)
                    .map_err(|err| err.to_string())
                });
                bridge::call(code, Input::Lines(&mut read))
            }
            Err(err) => bridge::unreadable_input(code, err.to_string()),
        };
        // A failed line read can leave its throwable pending.
        env.exception_clear();
        Ok(java_bytes(env, &out))
    })
    .resolve::<LogErrorAndDefault>()
}
