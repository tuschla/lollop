#!/usr/bin/env bash
#   scripts/upgrade-fixtures/generate.sh [SHA ...]
# Regenerate shared/tests/fixtures/upgrade/ (read by shared/tests/upgrade_logs.rs
# and android/.../UpgradeFixturesTest.kt) from the OLD builds themselves, one per
# SHA (default: every <sha>.log.ndjson the directory holds):
#   1. a detached worktree per old commit in ~/.cache/upg-<sha>;
#   2. the old build's generator runs in that worktree's JVM tests:
#      UpgradeFixtureGen-203ab56.kt, else UpgradeFixtureGen.kt with the lines
#      tagged `// only:<other sha>` dropped. It writes the old shell's
#      Event.toJson() lines (raw.ndjson), the file after the old boot's
#      EventLog.load (log.ndjson), the old export (backup.ndjson) and, for
#      203ab56 and 4728546, an interrupted run's sidecar (active-run.ndjson);
#   3. upg_expected.rs runs in that worktree's core: it replays the old log and
#      writes what the old app showed (history.json; the whole view for 4728546).
# The generators write to $UPG_GEN_DIR/$UPG_SHA/ (default ~/.cache/upgrade-fixtures/gen).
set -euo pipefail
# shellcheck source-path=SCRIPTDIR source=../lib/env.sh
source "$(dirname "$0")/../lib/env.sh"
cd "$REPO_ROOT"
HERE="$REPO_ROOT/scripts/upgrade-fixtures"
DEST="$REPO_ROOT/shared/tests/fixtures/upgrade"
export UPG_GEN_DIR="${UPG_GEN_DIR:-$HOME/.cache/upgrade-fixtures/gen}"
GEN="$UPG_GEN_DIR"

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

shas=("$@")
if [[ ${#shas[@]} -eq 0 ]]; then
  for f in "$DEST"/*.log.ndjson; do
    f="$(basename "$f" .log.ndjson)"
    [[ $f == *-* ]] || shas+=("$f")
  done
fi
for sha in "${shas[@]}"; do
  wt="$HOME/.cache/upg-$sha"
  [[ -d "$wt" ]] || git worktree add --detach "$wt" "$sha"
  if [[ -f "$REPO_ROOT/android/local.properties" ]]; then cp "$REPO_ROOT/android/local.properties" "$wt/android/"; fi
  generator "$sha" >"$wt/android/app/src/test/java/app/milestone/UpgradeFixtureGen.kt"
  cp "$HERE/upg_expected.rs" "$wt/shared/tests/"
  # 901d239 compacts through the core over JNI: its JVM test needs the host core.
  case $sha in
    4728546) skip=(-x buildHostCore) ;;
    *) skip=() ;; # 203ab56 has no host core task
  esac
  (cd "$wt/android" && UPG_SHA="$sha" CARGO_TARGET_DIR="$HOME/.cache/upg-target-$sha" \
    ./gradlew -q --max-workers="$GRADLE_MAX_WORKERS" testDebugUnitTest --tests app.milestone.UpgradeFixtureGen "${skip[@]}")
  full=(); [[ $sha == 4728546 ]] && full=(UPG_FULL_VIEW=1)
  (cd "$wt" && env "${full[@]}" UPG_SHA="$sha" CARGO_TARGET_DIR="$HOME/.cache/upg-target-$sha" cargo test -q -p shared --test upg_expected)
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
