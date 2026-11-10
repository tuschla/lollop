//! Emits `cfg(ffi_surface)` where the shells' bridge (`shared::ffi`) and the
//! core helpers only it calls are compiled: an Android target, or the `ffi`
//! feature (`jni-host` and `c-abi` enable it). Unit tests add `test` at each
//! gate (`any(ffi_surface, test)`): a build script cannot see `cfg(test)`.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rustc-check-cfg=cfg(ffi_surface)");
    let android = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "android");
    if android || std::env::var_os("CARGO_FEATURE_FFI").is_some() {
        println!("cargo::rustc-cfg=ffi_surface");
    }
}
