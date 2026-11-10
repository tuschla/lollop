#!/usr/bin/env bash
# Build the core's C ABI (shared/src/ffi/c_abi.rs) as a static library,
# <target-dir>/<triple>/<profile dir>/libshared.a, for the iOS app and the
# Kotlin/Native bridge tests. The Gradle build (android/build-logic, RustCore)
# calls it per triple; profiles and triples come from scripts/lib/rust-core.env.
#
#   scripts/build-ios-core.sh                      # the iOS triples, app profile
#   scripts/build-ios-core.sh x86_64-unknown-linux-gnu  # any triple(s)
#   scripts/build-ios-core.sh --test-hooks x86_64-unknown-linux-gnu
#                                                  # test profile + lollop_*_for_test hooks
#
# `cargo rustc --crate-type staticlib` replaces shared's [lib] crate types for
# this one build, so no Apple linker is needed and the Android cdylib build is
# untouched. Works on Linux: a staticlib is an archive, not a link.
# Runs under macOS's bash 3.2 too, where an empty array trips `set -u`: hence
# a profile string instead of an array.
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/lib/rust-core.env

profile=$RUST_PROFILE_APP
features=c-abi
if [[ "${1:-}" == "--test-hooks" ]]; then
    shift
    profile=$RUST_PROFILE_TESTS
    features=c-abi,test-hooks
fi
if [[ $# -eq 0 ]]; then
    set -- "$RUST_TRIPLE_iosArm64" "$RUST_TRIPLE_iosSimulatorArm64"
fi

for target in "$@"; do
    cargo rustc -p shared --lib --locked --profile "$profile" --target "$target" \
        --features "$features" --crate-type staticlib
done
