#!/usr/bin/env bash
# Full build for the lollop app, the one entry point for building anything.
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
#       signed by scripts/sign-release.sh: the release key when $MILESTONE_KEYSTORE is set)
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
SIGNED_APK="$(scripts/sign-release.sh)"

echo "==> Done"
ls -la android/app/build/outputs/apk/debug/app-debug.apk "$SIGNED_APK"

if [[ $RUN_QA == 1 ]]; then
  echo "==> On-device UI/UX smoke (emulator)"
  QA_SKIP_BUILD=1 scripts/qa-emulator.sh
fi
