//! Emits `cfg(ffi_surface)` where the shells' bridge (`shared::ffi`) and the
//! core helpers only it calls are compiled: an Android target, or the `ffi`
//! feature (`jni-host` and `c-abi` enable it). Unit tests add `test` at each
//! gate (`any(ffi_surface, test)`): a build script cannot see `cfg(test)`.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rustc-check-cfg=cfg(ffi_surface)");
    // crux 0.20's `#[effect(typegen)]`, which derives the bridge's `EffectFFI`,
    // also emits items under `cfg(feature = "typegen")` for crux's own type
    // generation. The shells' types come from `shared/wiregen`, so the feature
    // does not exist; this only declares the name.
    println!("cargo::rustc-check-cfg=cfg(feature, values(\"typegen\"))");
    let android = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "android");
    if android || std::env::var_os("CARGO_FEATURE_FFI").is_some() {
        println!("cargo::rustc-cfg=ffi_surface");
    }
}
