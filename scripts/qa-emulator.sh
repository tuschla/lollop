#!/usr/bin/env bash
# On-device UI/UX smoke QA on a headless emulator. Complements the host-JVM
# Robolectric suite (scripts/build.sh runs that): this exercises the REAL APK on a
# REAL Android runtime, so it catches what the JVM shadow can't — actual rendering,
# the packaged arm64/x86_64 libshared.so, and process-level crashes/ANRs.
#
#   scripts/qa-emulator.sh                 # full run: boot AVD, install, seed,
#                                          # screenshot, monkey, connected tests, stop
#   scripts/qa-emulator.sh <avd-name>      # use a specific AVD (default: e34)
#   scripts/qa-emulator.sh --check         # dry-run: validate SDK/AVD/seed wiring
#                                          # WITHOUT starting the emulator (CI lint)
#
# Requires: an Android SDK with an emulator + a created AVD, KVM (/dev/kvm) for
# acceptable speed, and the JDK/cargo toolchain scripts/build.sh needs. Never runs
# during the offline QA task (RAM is tight); it is the on-device counterpart.
set -euo pipefail

cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"

APP_ID="app.milestone"
AVD="e34"
DRY_RUN=0
# A dedicated console port, so the run never talks to (or kills) an emulator the
# developer already has open on the default 5554.
EMU_PORT=5580
SERIAL="emulator-$EMU_PORT"
QA_OUT="$REPO_ROOT/android/app/build/qa"

for arg in "$@"; do
  case "$arg" in
    --check) DRY_RUN=1 ;;
    --*) echo "unknown flag: $arg" >&2; exit 2 ;;
    *) AVD="$arg" ;;
  esac
done

log() { printf '==> %s\n' "$*"; }
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

# ── SDK / tool resolution ────────────────────────────────────────────────────
# sdk.dir from android/local.properties (the app's configured SDK), else env.
sdk_dir() {
  local props="$REPO_ROOT/android/local.properties" dir=""
  if [[ -f "$props" ]]; then
    dir="$(sed -n 's/^sdk\.dir=//p' "$props" | tail -1)"
  fi
  echo "${dir:-${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}}"
}

SDK="$(sdk_dir)"
[[ -n "$SDK" ]] || die "no Android SDK: set sdk.dir in android/local.properties or \$ANDROID_HOME"
EMULATOR="$SDK/emulator/emulator"
ADB="$SDK/platform-tools/adb"

# ── Pinned clock ─────────────────────────────────────────────────────────────
# The most recent Monday 10:00 (local): Monday is a lift day in the seeded plan,
# so Today always has a session to log regardless of the weekday the smoke runs.
PINNED_NOW="$(date -d "$(date -d "-$(( ($(date +%u) + 6) % 7 )) days" +%F) 10:00" +%s)"

# ── Seed event log (same ndjson wire the app persists) ───────────────────────
# A set-up strength lifter with one squat + one run the day before the pinned
# Monday, so Today shows the plan hero and History shows entries.
build_seed() {
  local yesterday
  yesterday="$((PINNED_NOW - 86400))"
  cat <<EOF
{"SetProfile":{"progression_cadence":"WeekToWeek","lift_goal":"MaxStrength","goal_distance":"General","concurrent_goal":"Strength","weekly_sets":12,"running_days_per_week":0,"running_km_per_week":0.0,"advanced":false,"endurance_intensity_pct_vo2max":75.0,"bodyweight_kg":85.0,"age_years":30.0}}
{"LogSet":{"exercise":"Back Squat","weight_kg":120.0,"reps":3,"rpe":8.0,"observed_at":${yesterday},"entry_id":${yesterday}000}}
{"LogRun":{"distance_km":5.0,"duration_min":27.5,"hr_pct_max":0.0,"longest_recent_km":0.0,"observed_at":${yesterday},"entry_id":${yesterday}001}}
EOF
}

# ── Emulator lifecycle ───────────────────────────────────────────────────────
boot_emulator() {
  [[ -x "$EMULATOR" ]] || die "emulator not found at $EMULATOR"
  "$EMULATOR" -list-avds | grep -qx "$AVD" || die "AVD '$AVD' not found (create it with avdmanager)"
  [[ -e /dev/kvm ]] || log "WARNING: /dev/kvm missing — the emulator will be very slow"
  if "$ADB" -s "$SERIAL" get-state >/dev/null 2>&1; then
    die "$SERIAL is already running; stop it first"
  fi
  log "booting AVD '$AVD' (headless, port $EMU_PORT)"
  # The emulator resolves the AVD's system image through ANDROID_SDK_ROOT, not its
  # own install path; a stale environment value would point it at another SDK.
  ANDROID_SDK_ROOT="$SDK" ANDROID_HOME="$SDK" "$EMULATOR" -avd "$AVD" -port "$EMU_PORT" \
    -no-window -no-audio -no-boot-anim -no-snapshot -gpu swiftshader_indirect -read-only &
  EMU_PID=$!
  log "waiting for boot to complete"
  local tries=0
  until [[ "$("$ADB" -s "$SERIAL" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == "1" ]]; do
    kill -0 "$EMU_PID" 2>/dev/null || die "emulator exited during boot (see its output above)"
    tries=$((tries + 1))
    [[ $tries -lt 150 ]] || die "emulator boot timed out"
    sleep 2
  done
  "$ADB" -s "$SERIAL" shell input keyevent 82 >/dev/null 2>&1 || true # dismiss keyguard
  log "pinning the device clock to $(date -d @"$PINNED_NOW")"
  "$ADB" -s "$SERIAL" root >/dev/null
  "$ADB" -s "$SERIAL" wait-for-device
  "$ADB" -s "$SERIAL" shell settings put global auto_time 0
  "$ADB" -s "$SERIAL" shell date "$(date -d @"$PINNED_NOW" +%m%d%H%M%Y.%S)" >/dev/null
}

stop_emulator() {
  [[ -n "${EMU_PID:-}" ]] || return 0
  log "shutting emulator down"
  "$ADB" -s "$SERIAL" emu kill >/dev/null 2>&1 || true
  # The console kill is not implemented on every emulator build; fall back to
  # signalling the process this script started, never anything else.
  local i
  for i in $(seq 1 15); do
    kill -0 "$EMU_PID" 2>/dev/null || break
    sleep 1
  done
  kill "$EMU_PID" 2>/dev/null || true
  wait "$EMU_PID" 2>/dev/null || true
}

# ── App install + seed ───────────────────────────────────────────────────────
install_app() {
  log "building + installing the debug APK"
  scripts/build.sh --no-tests
  "$ADB" -s "$SERIAL" install -r \
    "$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
}

seed_app() {
  log "seeding the scenario event log"
  # Launch once so the app data dir + files/ exist, then stop it.
  "$ADB" -s "$SERIAL" shell monkey -p "$APP_ID" -c android.intent.category.LAUNCHER 1 >/dev/null 2>&1
  sleep 4
  "$ADB" -s "$SERIAL" shell am force-stop "$APP_ID"
  # `adb shell` joins its arguments into ONE device command line, so the run-as
  # command must be a single quoted string; the seed streams in over stdin into
  # the sandboxed files dir the app replays on boot.
  build_seed | "$ADB" -s "$SERIAL" shell \
    "run-as $APP_ID sh -c 'mkdir -p files && cat > files/event-log.ndjson'"
}

# ── Screenshots of the key screens ───────────────────────────────────────────
screenshot() { "$ADB" -s "$SERIAL" exec-out screencap -p > "$QA_OUT/$1.png"; }

capture_screens() {
  mkdir -p "$QA_OUT"
  log "capturing key-screen screenshots into $QA_OUT"
  "$ADB" -s "$SERIAL" shell monkey -p "$APP_ID" -c android.intent.category.LAUNCHER 1 >/dev/null 2>&1
  sleep 5
  screenshot today
  # Bottom-nav taps by proportional position (3-tab bar): History (center),
  # Profile (right). Coordinates derive from the live display size so they hold
  # across densities/resolutions.
  read -r W H < <("$ADB" -s "$SERIAL" shell wm size | sed -n 's/.*: *\([0-9]*\)x\([0-9]*\).*/\1 \2/p')
  local y=$((H * 97 / 100))
  "$ADB" -s "$SERIAL" shell input tap $((W / 2)) "$y"; sleep 2; screenshot history
  "$ADB" -s "$SERIAL" shell input tap $((W * 5 / 6)) "$y"; sleep 2; screenshot profile
}

# Tap the first on-screen node whose text/content-desc starts with $1, located
# through the accessibility tree (uiautomator dump) so it is layout-independent.
tap_text() {
  local xml bounds
  xml="$("$ADB" -s "$SERIAL" exec-out uiautomator dump /dev/tty 2>/dev/null)"
  bounds="$(printf '%s' "$xml" | grep -oE "(text|content-desc)=\"$1[^\"]*\"[^>]*bounds=\"\[[0-9]+,[0-9]+\]\[[0-9]+,[0-9]+\]\"" \
    | head -1 | sed -E 's/.*bounds="\[([0-9]+),([0-9]+)\]\[([0-9]+),([0-9]+)\]".*/\1 \2 \3 \4/')"
  [[ -n "$bounds" ]] || die "no on-screen node starting with '$1'"
  read -r x1 y1 x2 y2 <<<"$bounds"
  "$ADB" -s "$SERIAL" shell input tap $(((x1 + x2) / 2)) $(((y1 + y2) / 2))
}

# The owner's gym-floor flow on a real runtime: log the first prescribed set from
# Today, keep the editor open for the next set, then back on Today the item shows
# its first slot filled.
log_set_flow() {
  log "driving the per-set logging flow"
  "$ADB" -s "$SERIAL" shell am force-stop "$APP_ID"
  "$ADB" -s "$SERIAL" shell monkey -p "$APP_ID" -c android.intent.category.LAUNCHER 1 >/dev/null 2>&1
  sleep 5
  tap_text "Log set 1 of"; sleep 2; screenshot set-loop
  # The editor's pinned primary carries the same "Log set 1 of N" label; the sheet
  # is its own window, so the dump only sees the editor's copy.
  tap_text "Log set 1 of"; sleep 2; screenshot set-loop-after-save
  tap_text "Finish"; sleep 2; screenshot today-after-set
  "$ADB" -s "$SERIAL" exec-out uiautomator dump /dev/tty 2>/dev/null | grep -qE 'content-desc="1 of [0-9]+ sets done' \
    || die "after logging a set the first slot is not marked done"
  log "per-set flow OK"
}

# ── Monkey stress: fail on a crash, native abort or ANR in the app ───────────
monkey_pass() {
  ADB="$ADB" QA_OUT="$QA_OUT" scripts/monkey-gate.sh "$SERIAL"
}

connected_tests() {
  log "running connectedDebugAndroidTest"
  (cd "$REPO_ROOT/android" && ANDROID_SERIAL="$SERIAL" ./gradlew --no-daemon connectedDebugAndroidTest)
}

# ── Dry-run (no emulator): validate wiring only ──────────────────────────────
if [[ $DRY_RUN -eq 1 ]]; then
  log "DRY RUN — SDK=$SDK"
  log "emulator: $EMULATOR $( [[ -x "$EMULATOR" ]] && echo '(present)' || echo '(MISSING)')"
  log "adb: $ADB $( [[ -x "$ADB" ]] && echo '(present)' || echo '(MISSING)')"
  log "AVD requested: $AVD"
  log "seed event log preview:"
  build_seed
  log "dry run OK (emulator not started)"
  exit 0
fi

# ── Full run ─────────────────────────────────────────────────────────────────
trap stop_emulator EXIT
boot_emulator
install_app
seed_app
capture_screens
log_set_flow
monkey_pass
connected_tests
log "on-device QA complete — screenshots in $QA_OUT"
