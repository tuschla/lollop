# shellcheck shell=bash
# Device helpers for scripts/emulator-scenarios.sh (sourced; also usable from an
# interactive shell: `source scripts/emu-scenarios/lib.sh`).
#   Needs: ADB, SERIAL, APP_ID, OUT, HELP (dir of this file).

: "${APP_ID:=app.milestone}"
: "${SERIAL:=emulator-5580}"
: "${ADB:=/home/lt/android-home/platform-tools/adb}"
: "${HELP:=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
: "${OUT:=$HOME/.cache/emu-scenarios}"

adb_() { "$ADB" -s "$SERIAL" "$@"; }
sh_() { adb_ shell "$@" 2>/dev/null | tr -d '\r'; }

log() { printf '[%s] ==> %s\n' "$(date +%T)" "$*" | tee -a "$OUT/run.log" >&2; }

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
mark() { adb_ shell log -t EMUSCN "phase $1" >/dev/null 2>&1 || true; log "── $1"; }
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
