#!/usr/bin/env bash
# Full build for the lollop app, the ONE entry point for building anything.
#
#   scripts/build.sh            # everything: core tests → jniLibs → Android tests → debug + release APKs
#   scripts/build.sh --no-tests # same, skipping cargo test + gradle unit tests (quick rebuild)
#   scripts/build.sh --qa       # after the normal build, run the on-device
#                               # emulator UI/UX smoke (scripts/qa-emulator.sh)
#
# The gradle unit-test step ALSO runs the offline UI/UX QA suite (Robolectric +
# Roborazzi driving the real Compose UI over the real Rust core). Its gradle task
# builds the host `libshared.so` (shared crate `jni-host` feature) automatically
# before the tests, so the UI tests always have a real core to load.
# Outputs:
#   android/app/build/outputs/apk/debug/app-debug.apk                        (debug-signed, installable)
#   android/app/build/outputs/apk/release/lollop-release.apk                 (minified arm64-v8a APK,
#       signed with $MILESTONE_KEYSTORE / $MILESTONE_KS_PASS / $MILESTONE_KS_ALIAS when set; the
#       env var names predate the lollop rename and are kept so existing signing setups work)
#   android/app/build/outputs/apk/release/lollop-release-debugsigned.apk     (same APK signed with the
#       DEBUG key when no keystore is set: sideload/testing only, never distributable)
#
# Toolchain expectations: Arch system rust (no rustup),
# cargo-ndk, NDK r30, JDK 21 at /usr/lib/jvm/java-21-openjdk, Android SDK at
# /opt/android-sdk. The Rust core MUST be rebuilt via cargo-ndk before gradle -
# gradle only packages whatever .so files already sit in jniLibs/, so skipping
# that step ships a stale core silently.
set -euo pipefail
cd "$(dirname "$0")/.."

RUN_TESTS=1
RUN_QA=0
for arg in "$@"; do
  case "$arg" in
    --no-tests) RUN_TESTS=0 ;;
    --qa) RUN_QA=1 ;;
    *) echo "unknown flag: $arg" >&2; exit 2 ;;
  esac
done

export JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk}"
SDK="${ANDROID_HOME:-/opt/android-sdk}"
APKSIGNER="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)/apksigner"

echo "==> Rust core: build$([[ $RUN_TESTS == 1 ]] && echo ' + test')"
cargo build
[[ $RUN_TESTS == 1 ]] && cargo test

echo "==> jniLibs: cargo-ndk release, arm64-v8a (all builds) + x86_64 (debug/emulator only)"
cargo ndk -o android/app/src/main/jniLibs -t arm64-v8a -t x86_64 build -p shared --release

echo "==> Android: $([[ $RUN_TESTS == 1 ]] && echo 'unit tests (incl. UI/UX QA) + ')debug + release APKs"
GRADLE_TASKS=(assembleDebug assembleRelease)
# testDebugUnitTest runs the offline UI/UX QA (Robolectric); verifyRoborazziDebug
# gates the committed screenshot baselines. Both build the host core first.
[[ $RUN_TESTS == 1 ]] && GRADLE_TASKS=(testDebugUnitTest verifyRoborazziDebug "${GRADLE_TASKS[@]}")
(cd android && ./gradlew -q "${GRADLE_TASKS[@]}")

echo "==> Signing release APK"
REL_DIR=android/app/build/outputs/apk/release
if [[ -n "${MILESTONE_KEYSTORE:-}" ]]; then
  "$APKSIGNER" sign \
    --ks "$MILESTONE_KEYSTORE" --ks-pass "pass:$MILESTONE_KS_PASS" --ks-key-alias "$MILESTONE_KS_ALIAS" \
    --out "$REL_DIR/lollop-release.apk" \
    "$REL_DIR/app-release-unsigned.apk"
  SIGNED_APK="$REL_DIR/lollop-release.apk"
else
  "$APKSIGNER" sign \
    --ks ~/.android/debug.keystore --ks-pass pass:android \
    --out "$REL_DIR/lollop-release-debugsigned.apk" \
    "$REL_DIR/app-release-unsigned.apk"
  SIGNED_APK="$REL_DIR/lollop-release-debugsigned.apk"
  echo "WARNING: release APK signed with the debug keystore — not distributable"
fi

echo "==> Done"
ls -la android/app/build/outputs/apk/debug/app-debug.apk "$SIGNED_APK"

if [[ $RUN_QA == 1 ]]; then
  echo "==> On-device UI/UX smoke (emulator)"
  scripts/qa-emulator.sh
fi
