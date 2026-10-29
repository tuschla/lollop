# shellcheck shell=bash
# Device helpers for scripts/emulator-scenarios.sh, scripts/emu-scenarios/
# surfaces.sh, scripts/qa-emulator.sh and scripts/soak-emulator.sh (sourced; also
# usable from an interactive shell: `source scripts/emu-scenarios/lib.sh`).
#   Needs: SERIAL, APP_ID, OUT (an existing dir), TZ0; the Android SDK from sdk.dir
#   in android/local.properties, else $ANDROID_HOME / $ANDROID_SDK_ROOT.

: "${APP_ID:=app.milestone}"
: "${SERIAL:=emulator-5580}"
: "${HELP:=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
: "${OUT:=$HOME/.cache/emu-scenarios}"
: "${TZ0:=Europe/Berlin}"

log() { printf '[%s] ==> %s\n' "$(date +%T)" "$*" | tee -a "$OUT/run.log" >&2; }
die() { log "ERROR: $*"; exit 1; }

SDK="$(sed -n 's/^sdk\.dir=//p' "$HELP/../../android/local.properties" 2>/dev/null | tail -1)"
SDK="${SDK:-${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}}"
[[ -n "$SDK" ]] || die "no Android SDK: set sdk.dir in android/local.properties or \$ANDROID_HOME"
: "${ADB:=$SDK/platform-tools/adb}"
EMULATOR="$SDK/emulator/emulator"

adb_() { "$ADB" -s "$SERIAL" "$@"; }
sh_() { adb_ shell "$@" 2>/dev/null | tr -d '\r'; }

# check PASS|FAIL|INFO SCENARIO NAME DETAIL — one TSV row, echoed to the log.
check() {
  local d="${4//$'\t'/ }"; d="${d//$'\n'/ ⏎ }"
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$d" >>"$OUT/checks.tsv"
  log "[$1] $2 · $3: $d"
}
# expect SCENARIO NAME DETAIL CMD... — PASS when CMD succeeds, else FAIL.
expect() {
  local sc="$1" nm="$2" de="$3"; shift 3
  if "$@"; then check PASS "$sc" "$nm" "$de"; else check FAIL "$sc" "$nm" "$de"; fi
}

dump_ui() {
  local x i
  for i in 1 2 3 4; do
    x="$(adb_ exec-out uiautomator dump /dev/tty 2>/dev/null || true)"
    [[ "$x" == *"</hierarchy>"* ]] && { printf '%s' "${x%%UI hierchary dumped*}"; return 0; }
    sleep 1
  done
  return 1
}
find_node() { dump_ui | python3 "$HELP/node.py" "$@"; }
ui_texts() { dump_ui | python3 "$HELP/node.py" texts; }
has_text() { find_node "$@" >/dev/null 2>&1; } # has_text MODE TEXT
# wait_text MODE TEXT [seconds]
wait_text() {
  local i
  for i in $(seq 1 "${3:-15}"); do
    has_text "$1" "$2" && return 0
    sleep 1
  done
  return 1
}
tap_xy() { adb_ shell input tap "$1" "$2"; }
# tap_node MODE TEXT [retries]
tap_node() {
  local r p i
  for i in $(seq 1 "${3:-4}"); do
    if r="$(find_node "$1" "$2")"; then
      read -r -a p <<<"$r"
      tap_xy "${p[0]}" "${p[1]}"
      return 0
    fi
    sleep 1
  done
  log "no on-screen node ($1) '$2'"
  return 1
}
# Bottom-nav item by label: tap the icon above the label (the label's lower edge
# can sit under the system navigation bar's touch region).
tap_nav() {
  local r p
  r="$(find_node eq "$1")" || { log "no nav item '$1'"; return 1; }
  read -r -a p <<<"$r"
  tap_xy "${p[0]}" $(( p[3] - 60 > p[1] - 40 ? p[1] - 40 : p[3] - 60 ))
}
# Scroll down until MODE TEXT is on screen (max N swipes); then tap it when $4 = tap.
scroll_to() {
  local i
  for i in $(seq 1 "${3:-8}"); do
    has_text "$1" "$2" && return 0
    adb_ shell input swipe 540 1450 540 650 350
    sleep 1
  done
  has_text "$1" "$2"
}
scroll_top() { local i; for i in 1 2 3 4 5 6; do adb_ shell input swipe 540 500 540 1500 150; done; sleep 1; }
shot() { adb_ exec-out screencap -p >"$OUT/shots/$1.png" 2>/dev/null || true; }
# Save the UI tree + a screenshot under NAME (evidence for a check).
evidence() { shot "$1"; dump_ui >"$OUT/ui/$1.xml" 2>/dev/null || true; }
focus() { sh_ dumpsys window | sed -n 's/.*mCurrentFocus=Window{[^ ]* [^ ]* \([^ }]*\)}.*/\1/p' | head -1; }
launch() { adb_ shell am start -n "$APP_ID/.MainActivity" >/dev/null 2>&1 || true; }
app_pid() { sh_ pidof "$APP_ID" || true; }
# The app process is alive and its activity holds window focus.
app_up() { [[ -n "$(app_pid)" && "$(focus)" == "$APP_ID"* ]]; }
# $OUT/.phase holds the current phase (the soak sampler reads it).
mark() { echo "$1" >"$OUT/.phase"; adb_ shell log -t EMUSCN "phase $1" >/dev/null 2>&1 || true; log "── $1"; }
grant_perms() {
  local p
  for p in ACCESS_FINE_LOCATION ACCESS_COARSE_LOCATION POST_NOTIFICATIONS; do
    adb_ shell pm grant "$APP_ID" "android.permission.$p" >/dev/null 2>&1 || true
  done
}
wake() { adb_ shell input keyevent 224; sleep 1; adb_ shell wm dismiss-keyguard >/dev/null 2>&1 || true; sleep 1; }
svc_fg() { sh_ dumpsys activity services "$APP_ID/.RunTrackingService" | grep -q 'isForeground=true'; }
# App files through the root shell (works for the non-debuggable release APK too;
# the script runs `adb root` first).
APP_FILES="/data/data/$APP_ID/files"
pull_log() { adb_ exec-out cat "$APP_FILES/event-log.ndjson" >"$1" 2>/dev/null || true; }
push_log() {
  local uid
  adb_ push "$1" /data/local/tmp/emu-event-log.ndjson >/dev/null || return 1
  uid="$(sh_ stat -c %u "/data/data/$APP_ID")"
  adb_ shell "mkdir -p $APP_FILES && cp /data/local/tmp/emu-event-log.ndjson $APP_FILES/event-log.ndjson \
    && chown -R $uid:$uid $APP_FILES && chmod 600 $APP_FILES/event-log.ndjson && restorecon -R $APP_FILES \
    && rm /data/local/tmp/emu-event-log.ndjson"
}
# Fixes in the crash-durable run sidecar (0 when absent).
sidecar_points() { adb_ exec-out cat "$APP_FILES/active-run.ndjson" 2>/dev/null | grep -c '"lat"' || true; }
pull_sidecar() { adb_ exec-out cat "$APP_FILES/active-run.ndjson" >"$1" 2>/dev/null || true; }
# Tap near the TOP edge of a node: a bottom-anchored button can sit partly under
# the navigation bar's touch region.
tap_top() {
  local r p
  r="$(find_node "$1" "$2")" || { log "no on-screen node ($1) '$2'"; return 1; }
  read -r -a p <<<"$r"
  tap_xy "${p[0]}" $(( p[3] + 20 ))
}
# The label (last field) of the first node matching MODE TEXT, or empty.
node_label() { find_node "$1" "$2" 2>/dev/null | cut -d' ' -f7-; }
# Last "Displayed app.milestone/..." launch time in ms from the logcat buffer.
displayed_ms() {
  sh_ logcat -d -s ActivityTaskManager:I | grep "Displayed $APP_ID/" | tail -1 \
    | python3 -c "import re,sys;m=re.search(r'\+(?:(\d+)s)?(\d+)ms',sys.stdin.read());print(int(m.group(1) or 0)*1000+int(m.group(2)) if m else '')"
}
# Echo "pss java native" KB from dumpsys meminfo.
meminfo_kb() {
  local m
  m="$(sh_ dumpsys meminfo "$APP_ID")"
  printf '%s %s %s\n' \
    "$(sed -n 's/^ *TOTAL PSS: *\([0-9]*\).*/\1/p' <<<"$m" | head -1)" \
    "$(sed -n 's/^ *Java Heap: *\([0-9]*\).*/\1/p' <<<"$m" | head -1)" \
    "$(sed -n 's/^ *Native Heap: *\([0-9]*\).*/\1/p' <<<"$m" | head -1)"
}
# Count of event-log lines on device.
log_lines() { adb_ exec-out cat "$APP_FILES/event-log.ndjson" 2>/dev/null | grep -c . || true; }

# ── Emulator lifecycle ───────────────────────────────────────────────────────
EMU_PID=""
KEEP_EMU="${KEEP_EMU:-0}"
# boot_emulator AVD LOG [MEMORY_MB]: use $SERIAL when it is already online, else
# boot AVD headless on the serial's port (output to LOG). Then wait for the boot
# and restart adbd as root (ignored on images without root).
boot_emulator() {
  local avd="$1" logf="$2" mem="${3:-}" tries=0
  if adb_ get-state >/dev/null 2>&1; then
    log "$SERIAL already online — using it"
  else
    [[ -x "$EMULATOR" ]] || die "emulator not found at $EMULATOR"
    "$EMULATOR" -list-avds | grep -qx "$avd" || die "AVD '$avd' not found (create it with avdmanager)"
    [[ -e /dev/kvm ]] || log "WARNING: /dev/kvm missing — the emulator will be very slow"
    log "booting AVD $avd on port ${SERIAL#emulator-}"
    # The emulator finds the AVD's system image through ANDROID_SDK_ROOT, not its
    # own install path; a stale environment value would point it at another SDK.
    ANDROID_SDK_ROOT="$SDK" ANDROID_HOME="$SDK" "$EMULATOR" -avd "$avd" -port "${SERIAL#emulator-}" \
      -no-window -no-audio -no-boot-anim -no-snapshot -gpu swiftshader_indirect -read-only \
      ${mem:+-memory "$mem"} >"$logf" 2>&1 &
    EMU_PID=$!
  fi
  until [[ "$(sh_ getprop sys.boot_completed)" == "1" ]]; do
    [[ -z "$EMU_PID" ]] || kill -0 "$EMU_PID" 2>/dev/null || die "emulator exited during boot (see $logf)"
    tries=$((tries + 1)); [[ $tries -lt 150 ]] || die "emulator boot timed out"
    sleep 2
  done
  adb_ root >/dev/null 2>&1 || true
  sleep 2; adb_ wait-for-device
  until [[ "$(sh_ getprop sys.boot_completed)" == "1" ]]; do sleep 2; done
}
# Stop the emulator boot_emulator started, unless KEEP_EMU=1. One that was
# already running is left alone.
stop_emulator() {
  [[ -n "$EMU_PID" && "$KEEP_EMU" -ne 1 ]] || return 0
  log "stopping emulator $SERIAL"
  adb_ emu kill >/dev/null 2>&1 || true
  # The console kill is not implemented on every emulator build; then signal the
  # process boot_emulator started, never anything else.
  local i
  for i in $(seq 1 15); do
    kill -0 "$EMU_PID" 2>/dev/null || break
    sleep 1
  done
  kill "$EMU_PID" 2>/dev/null || true
  wait "$EMU_PID" 2>/dev/null || true
}

# ── Device state ─────────────────────────────────────────────────────────────
# pin_clock DAY [HH:MM] [TZ]: the device clock and zone (default 08:00, $TZ0).
pin_clock() {
  local tz="${3:-$TZ0}"
  sh_ cmd alarm set-timezone "$tz" >/dev/null
  sh_ cmd alarm set-time "$(TZ="$tz" date -d "$1 ${2:-08:00}" +%s)000" >/dev/null
}
# The same device state before every scenario.
baseline_device() {
  adb_ shell settings put global auto_time 0
  adb_ shell settings put global auto_time_zone 0
  adb_ shell settings put system time_12_24 12
  adb_ shell settings put system font_scale 1.0
  adb_ shell settings put system screen_off_timeout 1800000
  adb_ shell wm density reset >/dev/null
  adb_ shell cmd uimode night no >/dev/null
  adb_ shell settings put global low_power 0
  adb_ shell dumpsys battery reset >/dev/null
  sh_ cmd location set-location-enabled true >/dev/null
  adb_ shell cmd statusbar collapse >/dev/null 2>&1 || true
  sh_ cmd locale set-app-locales "$APP_ID" >/dev/null
  wake
  adb_ shell input keyevent 82 >/dev/null 2>&1 || true
}
# seed_basic DAY RUN_DAYS: a set-up lifter (12 weekly sets) with RUN_DAYS run days
# (20 km a week when RUN_DAYS > 0), a Back Squat at DAY 09:00 and a 5 km run at
# DAY 10:00 ($TZ0), as the ndjson event log the app replays on launch.
seed_basic() {
  local y km=0.0
  y="$(TZ="$TZ0" date -d "$1 09:00" +%s)"
  if [[ "$2" -gt 0 ]]; then km=20.0; fi
  cat <<EOF
{"SetProfile":{"progression_cadence":"WeekToWeek","lift_goal":"MaxStrength","goal_distance":"General","concurrent_goal":"Strength","weekly_sets":12,"running_days_per_week":$2,"running_km_per_week":${km},"advanced":false,"endurance_intensity_pct_vo2max":75.0,"bodyweight_kg":85.0,"age_years":30.0}}
{"LogSet":{"exercise":"Back Squat","weight_kg":120.0,"reps":3,"rpe":8.0,"observed_at":${y},"entry_id":${y}000}}
{"LogRun":{"distance_km":5.0,"duration_min":27.5,"hr_pct_max":0.0,"longest_recent_km":0.0,"observed_at":$((y + 3600)),"entry_id":$((y + 3600))001}}
EOF
}

# ── GPS feed ─────────────────────────────────────────────────────────────────
# start_feeder NAME: feeder.py's 1 Hz route until stop_feeder (log $OUT/logs/feed-NAME.log).
FEEDER_PID=""
start_feeder() {
  stop_feeder
  rm -f "$OUT/feed.stop"
  python3 "$HELP/feeder.py" "$ADB" "$SERIAL" "$OUT/logs/feed-$1.log" "$OUT/feed.stop" &
  FEEDER_PID=$!
  sleep 3
}
stop_feeder() {
  [[ -n "$FEEDER_PID" ]] || return 0
  touch "$OUT/feed.stop"
  wait "$FEEDER_PID" 2>/dev/null || true
  FEEDER_PID=""
}
