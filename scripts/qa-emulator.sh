#!/usr/bin/env bash
# On-device UI/UX smoke QA on a headless emulator. Complements the host-JVM
# Robolectric suite (scripts/build.sh runs that): this exercises the REAL APK on a
# REAL Android runtime, so it catches what the JVM shadow can't — actual rendering,
# the packaged arm64/x86_64 libshared.so, and process-level crashes/ANRs.
#
#   scripts/qa-emulator.sh                 # full run: boot AVD, build + install,
#                                          # seed, screenshot, set flow, monkey, stop
#   scripts/qa-emulator.sh <avd-name>      # use a specific AVD (default: e34)
#
# QA_SKIP_BUILD=1 installs the existing debug APK without rebuilding it
# (scripts/build.sh --qa sets it after its own build).
# Requires: an Android SDK with an emulator + a created AVD, KVM (/dev/kvm) for
# acceptable speed, and the JDK/cargo toolchain scripts/build.sh needs. Never runs
# during the offline QA task (RAM is tight); it is the on-device counterpart.
set -euo pipefail

cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"

APP_ID="app.milestone"
AVD="e34"
# A dedicated console port, so the run never talks to (or kills) an emulator the
# developer already has open on the default 5554.
SERIAL="emulator-5580"
QA_OUT="$REPO_ROOT/android/app/build/qa"
OUT="$QA_OUT"

for arg in "$@"; do
  case "$arg" in
    --*) echo "unknown flag: $arg" >&2; exit 2 ;;
    *) AVD="$arg" ;;
  esac
done

mkdir -p "$QA_OUT"
# shellcheck source=emu-scenarios/lib.sh
source "$REPO_ROOT/scripts/emu-scenarios/lib.sh"

# ── Pinned clock ─────────────────────────────────────────────────────────────
# The most recent Monday, 10:00: Monday is a lift day in the seeded plan, so Today
# always has a session to log regardless of the weekday the smoke runs.
PIN_DAY="$(date -d "-$(( ($(date +%u) + 6) % 7 )) days" +%F)"

# ── App install + seed ───────────────────────────────────────────────────────
install_app() {
  if [[ "${QA_SKIP_BUILD:-0}" != 1 ]]; then
    log "building the debug APK"
    scripts/build.sh --no-tests
  fi
  log "installing the debug APK"
  adb_ install -r "$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
}

# A set-up strength lifter (no run days) with a squat + a run the day before the
# pinned Monday, so Today shows the plan hero and History shows entries.
seed_app() {
  log "seeding the scenario event log"
  # Launch once so the app data dir exists, then stop it.
  launch
  sleep 4
  adb_ shell am force-stop "$APP_ID"
  seed_basic "$(date -d "$PIN_DAY -1 day" +%F)" 0 >"$QA_OUT/seed.ndjson"
  push_log "$QA_OUT/seed.ndjson" || die "could not push the seed event log"
}

# ── Screenshots of the key screens ───────────────────────────────────────────
screenshot() { adb_ exec-out screencap -p > "$QA_OUT/$1.png"; }

capture_screens() {
  log "capturing key-screen screenshots into $QA_OUT"
  launch
  sleep 5
  screenshot today
  # Bottom-nav taps by proportional position (3-tab bar): History (center),
  # Profile (right). Coordinates derive from the live display size so they hold
  # across densities/resolutions.
  read -r W H < <(adb_ shell wm size | sed -n 's/.*: *\([0-9]*\)x\([0-9]*\).*/\1 \2/p')
  local y=$((H * 97 / 100))
  adb_ shell input tap $((W / 2)) "$y"; sleep 2; screenshot history
  adb_ shell input tap $((W * 5 / 6)) "$y"; sleep 2; screenshot profile
}

# Tap the first on-screen node whose text/content-desc starts with $1, located
# through the accessibility tree (uiautomator dump) so it is layout-independent.
tap_text() {
  tap_node pre "$1" || die "no on-screen node starting with '$1'"
}

# The owner's gym-floor flow on a real runtime: log the first prescribed set from
# Today, keep the editor open for the next set, then back on Today the item shows
# its first slot filled.
log_set_flow() {
  log "driving the per-set logging flow"
  adb_ shell am force-stop "$APP_ID"
  launch
  sleep 5
  tap_text "Log set 1 of"; sleep 2; screenshot set-loop
  # The editor's pinned primary carries the same "Log set 1 of N" label; the sheet
  # is its own window, so the dump only sees the editor's copy.
  tap_text "Log set 1 of"; sleep 2; screenshot set-loop-after-save
  tap_text "Finish"; sleep 2; screenshot today-after-set
  has_text re '^1 of [0-9]+ sets? done' || die "after logging a set the first slot is not marked done"
  log "per-set flow OK"
}

# ── Monkey stress: fail on a crash, native abort or ANR in the app ───────────
monkey_pass() {
  ADB="$ADB" QA_OUT="$QA_OUT" scripts/monkey-gate.sh "$SERIAL"
}

# ── Full run ─────────────────────────────────────────────────────────────────
trap stop_emulator EXIT
boot_emulator "$AVD" "$QA_OUT/emulator.log"
baseline_device
log "pinning the device clock to $PIN_DAY 10:00 $TZ0"
pin_clock "$PIN_DAY" 10:00
install_app
seed_app
capture_screens
log_set_flow
monkey_pass
log "on-device QA complete — screenshots in $QA_OUT"
