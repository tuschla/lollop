#!/usr/bin/env bash
# Full build for the milestone app, the ONE entry point for building anything.
#
#   scripts/build.sh            # everything: core tests → jniLibs → Android tests → debug + release APKs
#   scripts/build.sh --no-tests # same, skipping cargo test + gradle unit tests (quick rebuild)
#
# Outputs:
#   android/app/build/outputs/apk/debug/app-debug.apk                        (debug-signed, installable)
#   android/app/build/outputs/apk/release/milestone-release-debugsigned.apk  (minified universal APK,
#       signed with the DEBUG key, fine for sideload/testing; a store/F-Droid
#       release needs a real keystore in place of the apksigner step below)
#
# Toolchain expectations: Arch system rust (no rustup),
# cargo-ndk, NDK r29, JDK 21 at /usr/lib/jvm/java-21-openjdk, Android SDK at
# /opt/android-sdk. The Rust core MUST be rebuilt via cargo-ndk before gradle -
# gradle only packages whatever .so files already sit in jniLibs/, so skipping
# that step ships a stale core silently.
set -euo pipefail
cd "$(dirname "$0")/.."

RUN_TESTS=1
[[ "${1:-}" == "--no-tests" ]] && RUN_TESTS=0

export JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk}"
SDK="${ANDROID_HOME:-/opt/android-sdk}"
APKSIGNER="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)/apksigner"

echo "==> Rust core: build$([[ $RUN_TESTS == 1 ]] && echo ' + test')"
cargo build
[[ $RUN_TESTS == 1 ]] && cargo test

echo "==> jniLibs: cargo-ndk release, arm64-v8a + x86_64"
cargo ndk -o android/app/src/main/jniLibs -t arm64-v8a -t x86_64 build -p shared --release

echo "==> Android: $([[ $RUN_TESTS == 1 ]] && echo 'unit tests + ')debug + release APKs"
GRADLE_TASKS=(assembleDebug assembleRelease)
[[ $RUN_TESTS == 1 ]] && GRADLE_TASKS=(testDebugUnitTest "${GRADLE_TASKS[@]}")
(cd android && ./gradlew -q "${GRADLE_TASKS[@]}")

echo "==> Signing release APK (debug key - sideload/testing signature)"
REL_DIR=android/app/build/outputs/apk/release
"$APKSIGNER" sign \
  --ks ~/.android/debug.keystore --ks-pass pass:android \
  --out "$REL_DIR/milestone-release-debugsigned.apk" \
  "$REL_DIR/app-release-unsigned.apk"

echo "==> Done"
ls -la android/app/build/outputs/apk/debug/app-debug.apk "$REL_DIR/milestone-release-debugsigned.apk"
