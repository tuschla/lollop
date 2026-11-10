#!/usr/bin/env bash
# Full build for the lollop app, the ONE entry point for building anything.
#
#   scripts/build.sh            # everything: core tests → Android tests → debug + release APKs
#   scripts/build.sh --no-tests # same, skipping cargo test + gradle unit tests (quick rebuild)
#   scripts/build.sh --qa       # after the normal build, run the on-device
#                               # emulator UI/UX smoke (scripts/qa-emulator.sh)
#
# The gradle unit-test step also runs the offline UI/UX QA suite (Robolectric +
# Roborazzi driving the real Compose UI over the real Rust core).
# Outputs:
#   android/app/build/outputs/apk/debug/app-debug.apk                        (debug-signed, installable)
#   android/app/build/outputs/apk/release/lollop-release.apk                 (minified arm64-v8a APK,
#       signed with $MILESTONE_KEYSTORE / $MILESTONE_KS_PASS / $MILESTONE_KS_ALIAS when set; the
#       env var names predate the lollop rename and are kept so existing signing setups work)
#   android/app/build/outputs/apk/release/lollop-release-debugsigned.apk     (same APK signed with the
#       DEBUG key when no keystore is set: sideload/testing only, never distributable)
#
# Needs rustup (rust-toolchain.toml), cargo-ndk, the NDK, the JDK and the Android
# SDK (scripts/lib/env.sh).
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/lib/env.sh

RUN_TESTS=1
RUN_QA=0
for arg in "$@"; do
  case "$arg" in
    --no-tests) RUN_TESTS=0 ;;
    --qa) RUN_QA=1 ;;
    *) echo "unknown flag: $arg" >&2; exit 2 ;;
  esac
done

APKSIGNER="$(build_tool apksigner)"

if [[ $RUN_TESTS == 1 ]]; then
  echo "==> Rust core: test"
  cargo test --locked
fi

echo "==> Android: $([[ $RUN_TESTS == 1 ]] && echo 'unit tests (incl. UI/UX QA) + ')debug + release APKs"
GRADLE_TASKS=(:app:assembleDebug :app:assembleRelease)
# verifyRoborazziAndroidHostTest compares against the local screenshot
# baselines (gitignored).
[[ $RUN_TESTS == 1 ]] && GRADLE_TASKS=(:lollop:testAndroidHostTest :lollop:verifyRoborazziAndroidHostTest "${GRADLE_TASKS[@]}")
(cd android && ./gradlew -q "${GRADLE_TASKS[@]}")

echo "==> Release R8 keep check"
scripts/check-release-r8.sh

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
  QA_SKIP_BUILD=1 scripts/qa-emulator.sh
fi
