#!/usr/bin/env bash
# Signs the release APK of the last `gradlew :app:assembleRelease`.
#
#   scripts/sign-release.sh             # release key if MILESTONE_KEYSTORE is set, else the debug key
#   scripts/sign-release.sh --release   # release key or fail (GitHub Releases)
#
# The release key: MILESTONE_KEYSTORE (keystore file), MILESTONE_KS_PASS,
# MILESTONE_KS_ALIAS (the names predate the app's renames). The owner's copy is
# ~/.local/share/lollop/lollop-release.{jks,pass} (named before the lollo
# rename); CI gets it from the repo secrets. Android installs an update only
# over an APK signed with the same key.
# Prints the signed APK's path:
#   android/app/build/outputs/apk/release/lollo-release.apk               (release key)
#   android/app/build/outputs/apk/release/lollo-release-debugsigned.apk   (debug key: sideload/testing only)
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/lib/env.sh

REQUIRE_RELEASE_KEY=0
for arg in "$@"; do
  case "$arg" in
    --release) REQUIRE_RELEASE_KEY=1 ;;
    *) echo "unknown flag: $arg" >&2; exit 2 ;;
  esac
done

APKSIGNER="$(build_tool apksigner)"
REL_DIR=android/app/build/outputs/apk/release
UNSIGNED="$REL_DIR/app-release-unsigned.apk"
[[ -f $UNSIGNED ]] || { echo "no $UNSIGNED: run gradlew :app:assembleRelease first" >&2; exit 1; }

if [[ -n "${MILESTONE_KEYSTORE:-}" ]]; then
  SIGNED="$REL_DIR/lollo-release.apk"
  "$APKSIGNER" sign \
    --ks "$MILESTONE_KEYSTORE" --ks-pass "pass:$MILESTONE_KS_PASS" --ks-key-alias "$MILESTONE_KS_ALIAS" \
    --out "$SIGNED" "$UNSIGNED"
elif [[ $REQUIRE_RELEASE_KEY == 1 ]]; then
  echo "MILESTONE_KEYSTORE is not set: a release must be signed with the release key" >&2
  exit 1
else
  SIGNED="$REL_DIR/lollo-release-debugsigned.apk"
  "$APKSIGNER" sign --ks ~/.android/debug.keystore --ks-pass pass:android --out "$SIGNED" "$UNSIGNED"
  echo "WARNING: release APK signed with the debug keystore: not distributable" >&2
fi
"$APKSIGNER" verify "$SIGNED"
echo "$SIGNED"
