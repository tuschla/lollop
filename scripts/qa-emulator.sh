#!/usr/bin/env bash
# On-device UI/UX smoke QA on a headless emulator. Complements the host-JVM
# Robolectric suite (scripts/build.sh runs that): this runs the real APK on a
# real Android runtime, so it catches what the JVM shadows can't: the actual
# rendering, the packaged arm64/x86_64 libshared.so, and process-level crashes/ANRs.
#
#   scripts/qa-emulator.sh [--avd e37] [--port 5582]
#
# Boots the AVD headless on the port unless emulator-<port> is already online
# (CI's emulator: --port 5554), builds + installs the debug APK, seeds a log,
# screenshots, logs a set, runs monkey. QA_SKIP_BUILD=1 installs the existing
# debug APK without rebuilding it (scripts/build.sh --qa sets it after its own
# build).
# Requires: an Android SDK with an emulator + a created AVD, KVM (/dev/kvm) for
# acceptable speed, and the JDK/cargo toolchain the Gradle build needs. Outputs
# in android/app/build/qa/ (REPORT.md, checks.tsv, shots/, monkey/, logcat.txt).
set -uo pipefail

# shellcheck source-path=SCRIPTDIR source=emu-scenarios/lib.sh
source "$(dirname "$0")/emu-scenarios/lib.sh"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --avd) AVD="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
SERIAL="emulator-$PORT"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
out_init "$REPO_ROOT/android/app/build/qa"
S=smoke

# The most recent Monday, 10:00: Monday is a lift day in the seeded plan, so Today
# always has a session to log regardless of the weekday the smoke runs.
PIN_DAY="$(date -d "-$(( ($(date +%u) + 6) % 7 )) days" +%F)"

capture_screens() {
  { launch && wait_text eq "${STR[main_dest_today]}" 30; } || die "Today never rendered"
  shot today
  tap_nav "${STR[main_dest_history]}"; shot history
  tap_nav "${STR[main_dest_profile]}"; shot profile
}

# The owner's gym-floor flow on a real runtime: log the first prescribed set from
# Today, keep the editor open for the next set, then back on Today the item shows
# its first slot filled.
log_set_flow() {
  stop_app
  launch || return 1
  tap_node pre "$(fmt plan_log_set_n_of_n 1 '')" 30 || return 1
  wait_text eq "${STR[log_field_exercise]}" 10 || return 1
  shot set-loop
  # The editor's pinned primary carries the same "Log set 1 of N" label; the sheet
  # is its own window, so the dump only sees the editor's copy.
  tap_node pre "$(fmt log_primary_set_n_of_n 1 '')" || return 1
  wait_text re "^$(fmt log_primary_set_n_of_n 2 '[0-9]+')\$|^${STR[log_primary_finish_session]}\$" 10 || return 1
  shot set-loop-after-save
  tap_node eq "${STR[log_primary_finish_session]}" || return 1
  wait_gone eq "${STR[log_field_exercise]}" 10 || return 1
  shot today-after-set
  has_text re "^$(fmt plan_sets_done_cd.one 1 '[0-9]+')\$|^$(fmt plan_sets_done_cd.other 1 '[0-9]+')\$"
}

trap session_cleanup EXIT
config_run
config apk "$APK"
session_start "$AVD"
baseline_device
log "pinning the device clock to $PIN_DAY 10:00 $TZ0"
pin_clock "$PIN_DAY" 10:00
if [[ "${QA_SKIP_BUILD:-0}" != 1 ]]; then
  log "building the debug APK"
  (cd "$REPO_ROOT/android" && ./gradlew --max-workers="$GRADLE_MAX_WORKERS" :app:assembleDebug) || die "debug build failed"
fi
install_apk "$APK" -r || die "install failed"
seed_basic "$(date -d "$PIN_DAY -1 day" +%F)" 0 >"$OUT/logs/seed.ndjson" || die "seed_basic failed"
seed_app "$OUT/logs/seed.ndjson" || die "could not push the seed event log"
capture_screens
expect $S "per-set logging flow: the first slot is marked done" "shots/set-loop*.png shots/today-after-set.png" log_set_flow
mark monkey
monkey_run "$OUT/monkey" 42 500 150 500
monkey_checks $S monkey "$OUT/monkey"
session_end "On-device smoke"
