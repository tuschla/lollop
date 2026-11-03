#!/usr/bin/env bash
# Build the core's C ABI (shared/src/ffi/c_abi.rs) as a static library,
# <target-dir>/<triple>/<profile>/libshared.a, for the iOS app and the
# Kotlin/Native bridge tests (kmp/core-bridge, which calls this script).
#
#   scripts/build-ios-core.sh                      # aarch64-apple-ios + aarch64-apple-ios-sim, release
#   scripts/build-ios-core.sh x86_64-apple-ios     # any triple(s)
#   scripts/build-ios-core.sh --test-hooks x86_64-unknown-linux-gnu
#                                                  # debug profile + lollop_*_for_test hooks
#
# `cargo rustc --crate-type staticlib` replaces shared's [lib] crate types for
# this one build, so no Apple linker is needed and the Android cdylib build is
# untouched. Works on Linux: a staticlib is an archive, not a link.
# Needs `rustup target add <triple>` for each triple.
# Runs under macOS's bash 3.2 too, where an empty array trips `set -u`: hence
# a profile string instead of an array.
set -euo pipefail
cd "$(dirname "$0")/.."

profile=release
features=c-abi
if [[ "${1:-}" == "--test-hooks" ]]; then
    shift
    profile=dev
    features=c-abi,test-hooks
fi
if [[ $# -eq 0 ]]; then
    set -- aarch64-apple-ios aarch64-apple-ios-sim
fi

for target in "$@"; do
    cargo rustc -p shared --lib --locked --profile "$profile" --target "$target" \
        --features "$features" --crate-type staticlib
done
