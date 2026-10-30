#!/usr/bin/env bash
#   scripts/upgrade-fixtures/generate.sh
# Regenerate shared/tests/fixtures/upgrade/ (read by shared/tests/upgrade_logs.rs
# and android/.../UpgradeFixturesTest.kt) from the OLD builds themselves:
#   1. a detached worktree per old commit in ~/.cache/upg-<sha>;
#   2. the old build's generator runs in that worktree's JVM tests:
#      UpgradeFixtureGen-203ab56.kt, else UpgradeFixtureGen.kt with the lines
#      tagged `// only:<other sha>` dropped. It writes the old shell's
#      Event.toJson() lines (raw.ndjson), the file after the old boot's
#      EventLog.load (log.ndjson), the old export (backup.ndjson) and, for
#      203ab56 and 4728546, an interrupted run's sidecar (active-run.ndjson);
#   3. upg_expected.rs runs in that worktree's core: it replays the old log and
#      writes what the old app showed (history.json; the whole view for 4728546).
# The generators write to $UPG_GEN_DIR/<sha>/ (default ~/.cache/upgrade-fixtures/gen).
set -euo pipefail
cd "$(dirname "$0")/../.."
REPO="$(pwd)"
HERE="$REPO/scripts/upgrade-fixtures"
DEST="$REPO/shared/tests/fixtures/upgrade"
export UPG_GEN_DIR="${UPG_GEN_DIR:-$HOME/.cache/upgrade-fixtures/gen}"
GEN="$UPG_GEN_DIR"
export JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk}"

# generator SHA: the Kotlin generator source for that build.
generator() {
  if [[ -f "$HERE/UpgradeFixtureGen-$1.kt" ]]; then
    cat "$HERE/UpgradeFixtureGen-$1.kt"
  else
    awk -v sha="$1" 'match($0, / *\/\/ only:[0-9a-f]+$/) {
        if (substr($0, RSTART) !~ ("only:" sha "$")) next
        $0 = substr($0, 1, RSTART - 1)
      } { print }' "$HERE/UpgradeFixtureGen.kt"
  fi
}

for sha in 203ab56 4728546 901d239; do
  wt="$HOME/.cache/upg-$sha"
  [[ -d "$wt" ]] || git worktree add --detach "$wt" "$sha"
  # The SDK can also come from $ANDROID_HOME.
  if [[ -f "$REPO/android/local.properties" ]]; then cp "$REPO/android/local.properties" "$wt/android/"; fi
  generator "$sha" >"$wt/android/app/src/test/java/app/milestone/UpgradeFixtureGen.kt"
  cp "$HERE/upg_expected.rs" "$wt/shared/tests/"
  # 901d239 compacts through the core over JNI: its JVM test needs the host core.
  case $sha in
    4728546) skip=(-x buildHostCore) ;;
    *) skip=() ;; # 203ab56 has no host core task
  esac
  (cd "$wt/android" && CARGO_TARGET_DIR="$HOME/.cache/upg-target-$sha" \
    ./gradlew -q --max-workers=2 testDebugUnitTest --tests app.milestone.UpgradeFixtureGen "${skip[@]}")
  full=(); [[ $sha == 4728546 ]] && full=(UPG_FULL_VIEW=1)
  (cd "$wt" && env "${full[@]}" CARGO_TARGET_DIR="$HOME/.cache/upg-target-$sha" cargo test -q -p shared --test upg_expected)
  cp "$GEN/$sha/log.ndjson" "$DEST/$sha.log.ndjson"
  cp "$GEN/$sha/history.json" "$DEST/$sha.history.json"
done
cp "$GEN/203ab56/active-run.ndjson" "$DEST/203ab56.active-run.ndjson"
cp "$GEN/203ab56/plan-removed.ndjson" "$DEST/203ab56-plan-removed.log.ndjson"
cp "$GEN/4728546/active-run.ndjson" "$DEST/4728546.active-run.ndjson"
# The old export of a log that never held the retired calculator lines (no
# Android shell encoded them): the same backup minus those lines.
grep -v -E '^(\{"(ComputeCooper|ComputeCriticalSpeed|ComputeApre)"|"Clear(Cooper|CriticalSpeed|Apre)")' \
  "$GEN/4728546/backup.ndjson" >"$DEST/4728546.backup.ndjson"
echo "wrote $DEST"
