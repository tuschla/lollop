# shellcheck shell=bash
# Variables here (dates, AVD, FINISH, ...) are read by the scripts that source it.
# shellcheck disable=SC2034
# The device-test library of scripts/emulator-scenarios.sh, qa-emulator.sh,
# soak-emulator.sh and scripts/emu-scenarios/{surfaces,upgrade}.sh
# (also usable from an interactive shell: `source scripts/emu-scenarios/lib.sh`).
# The app's ids, file names, notification ids, actions and strings come from its
# sources through contract.py (APP_ID, COMP, STR, RX, DEST, EVENT_LOG_FILE, ...).
# Device functions need SERIAL; logging and evidence need OUT (out_init).

HELP="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source-path=SCRIPTDIR source=../lib/env.sh
source "$HELP/../lib/env.sh"
_contract="$(python3 "$HELP/contract.py")" || { echo "contract.py: the app contract could not be read" >&2; exit 1; }
eval "$_contract"
unset _contract
ADB="$ANDROID_HOME/platform-tools/adb"
EMULATOR="$ANDROID_HOME/emulator/emulator"
APP_FILES="/data/data/$APP_ID/files"

# The scripted week (Europe/Berlin): seed_basic logs on the Sunday, and its
# first boot on the Monday starts the plan there (run Mon, rest Tue, lift Wed).
TZ0="Europe/Berlin"
SEED_DAY="2026-09-20"
RUN_DAY="2026-09-21"
REST_DAY="2026-09-22"
LIFT_DAY="2026-09-23"
# The AVD and console port every script boots unless told otherwise (a dedicated
# port, so a run never talks to an emulator open on the default 5554). e37 runs
# the API 37 image the app compiles against, and surfaces.sh's Live Update checks
# need API 36+. For an Android 14 pass: --avd e34 --port 5580.
AVD="e37"
PORT=5582
KEEP_EMU=0
# Scenario phase markers: mark() writes them to the device log (scan_logcat.py
# attributes logcat lines by them) and to $OUT/.phase (current_phase).
PHASE_TAG="EMUSCN"

out_init() {
  OUT="$1"
  mkdir -p "$OUT"/{shots,ui,logs}
  : >"$OUT/checks.tsv"; : >"$OUT/observations.tsv"
}
log() { printf '[%s] ==> %s\n' "$(date +%T)" "$*" | tee -a "$OUT/run.log" >&2; }
die() { log "ERROR: $*"; exit 1; }
# check PASS|FAIL SCENARIO NAME DETAIL: one checks.tsv row, echoed to the log.
check() {
  local d="${4//$'\t'/ }"; d="${d//$'\n'/ ⏎ }"
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$d" >>"$OUT/checks.tsv"
  log "[$1] $2 · $3: $d"
}
# expect SCENARIO NAME DETAIL CMD...: PASS when CMD succeeds, else FAIL.
expect() {
  local sc="$1" nm="$2" de="$3"; shift 3
  if "$@"; then check PASS "$sc" "$nm" "$de"; else check FAIL "$sc" "$nm" "$de"; fi
}
# observe SCENARIO WHAT VALUE: one observations.tsv row (a measurement or a device
# state the report records), never a check.
observe() {
  local v="${3//$'\t'/ }"; v="${v//$'\n'/ ⏎ }"
  printf '%s\t%s\t%s\n' "$1" "$2" "$v" >>"$OUT/observations.tsv"
  log "observe $1 · $2 = $v"
}
# config KEY VALUE: one "key: value" line of config.txt (the report's header).
config() { printf '%-10s %s\n' "$1:" "$2" >>"$OUT/config.txt"; }
config_run() {
  config date "$(date -Is)"
  config serial "$SERIAL"
  config git "$(git -C "$REPO_ROOT" rev-parse --short HEAD)$(git -C "$REPO_ROOT" diff --quiet || echo ' (dirty tree)')"
}
mark() {
  echo "$1" >"$OUT/.phase"
  adb_ shell log -t "$PHASE_TAG" "phase $1" || log "phase marker '$1' not written to the device log"
  log "── $1"
}
current_phase() { cat "$OUT/.phase"; }

# wait_for SECONDS CMD...: run CMD until it succeeds, at most SECONDS (1 s apart).
wait_for() {
  local end=$((SECONDS + $1)); shift
  until "$@"; do
    ((SECONDS < end)) || return 1
    sleep 1
  done
}
# settled CMD...: CMD's output once two runs 1 s apart agree (at most 10 runs).
settled() {
  local a b i
  a="$("$@")"
  for i in 1 2 3 4 5 6 7 8 9; do
    sleep 1; b="$("$@")"
    [[ "$a" == "$b" ]] && break
    a="$b"
  done
  printf '%s\n' "$a"
}
# python3 node.py over stdin: q [within ID] MODE [TEXT [ANCHOR]] (node.py's modes).
q() { python3 "$HELP/node.py" "$@"; }
# A string resource with its %N$s / %N$d arguments filled: fmt KEY ARG...
fmt() {
  local s="${STR[$1]}" i=1 a; shift
  for a in "$@"; do s="${s//"%$i\$s"/$a}"; s="${s//"%$i\$d"/$a}"; i=$((i + 1)); done
  printf '%s' "${s//%%/%}"
}

adb_() { "$ADB" -s "$SERIAL" "$@"; }
sh_() { adb_ shell "$@" 2>/dev/null | tr -d '\r'; }
dev_now() { sh_ date +%s; }
# "W H" of the display in pixels.
screen_size() { sh_ wm size | sed -n 's/.*: *\([0-9]*\)x\([0-9]*\).*/\1 \2/p' | tail -1; }
epoch_day() { echo $(( $(TZ=UTC date -d "$1" +%s) / 86400 )); }

# The app window's UI tree (uiautomator dump first waits for the UI to go idle).
dump_ui() {
  local x i
  for i in 1 2 3 4; do
    x="$(adb_ exec-out uiautomator dump /dev/tty 2>/dev/null)"
    [[ "$x" == *"</hierarchy>"* ]] && { printf '%s' "${x%%UI hierchary dumped*}"; return 0; }
    sleep 1
  done
  return 1
}
find_node() { dump_ui | q "$@"; }
ui_texts() { dump_ui | q texts; }
has_text() { find_node "$@" >/dev/null 2>&1; } # has_text MODE TEXT
lacks_text() { ! has_text "$@"; }
# wait_text MODE TEXT [seconds]. A system "<app> isn't responding" dialog (System UI
# or the launcher on a loaded host, the first minutes after boot) covers the window
# being waited for: it is answered with Wait.
wait_text() {
  local end=$((SECONDS + ${3:-15})) x w
  while :; do
    x="$(dump_ui)"
    q "$1" "$2" <<<"$x" >/dev/null 2>&1 && return 0
    if q re ".+ isn't responding$" <<<"$x" >/dev/null 2>&1 && w="$(q eq Wait <<<"$x")"; then
      log "system ANR dialog answered with Wait"; tap_xy "${w%% *}" "$(cut -d' ' -f2 <<<"$w")"
    fi
    ((SECONDS < end)) || return 1
  done
}
# wait_gone MODE TEXT [seconds]
wait_gone() { wait_for "${3:-10}" lacks_text "$1" "$2"; }
tap_xy() { adb_ shell input tap "$1" "$2"; }
# tap_node MODE TEXT [seconds]: tap the node once it is on screen (default 4 s).
tap_node() {
  local r p end=$((SECONDS + ${3:-4}))
  until r="$(find_node "$1" "$2")"; do
    ((SECONDS < end)) || { log "no on-screen node ($1) '$2'"; return 1; }
  done
  read -r -a p <<<"$r"
  tap_xy "${p[0]}" "${p[1]}"
}
# Tap near the TOP edge of a node: a bottom-anchored button can sit partly under
# the navigation bar's touch region.
tap_top() {
  local r p
  r="$(find_node "$1" "$2")" || { log "no on-screen node ($1) '$2'"; return 1; }
  read -r -a p <<<"$r"
  tap_xy "${p[0]}" $(( p[3] + 20 ))
}
# Bottom-nav item by label: tap the icon above the label (the label's lower edge
# can sit under the system navigation bar's touch region).
tap_nav() {
  local r p
  r="$(find_node eq "$1")" || { log "no nav item '$1'"; return 1; }
  read -r -a p <<<"$r"
  tap_xy "${p[0]}" $(( p[3] - 60 > p[1] - 40 ? p[1] - 40 : p[3] - 60 ))
  wait_for 5 selected "$1"
}
# selected TEXT: the node labelled TEXT (or its parent, grandparent, a child) is
# checked or selected.
selected() { dump_ui | q on "$1" >/dev/null; }
_discard_or_closed() {
  local x; x="$(dump_ui)"
  q eq "${STR[log_discard_confirm]}" <<<"$x" >/dev/null || ! q eq "${STR[log_field_exercise]}" <<<"$x" >/dev/null
}
# The label (last field) of the first node matching MODE TEXT, or empty.
node_label() { find_node "$1" "$2" 2>/dev/null | cut -d' ' -f7-; }
# swipe_content: one upward swipe inside the largest vertically scrollable node
# (else the whole screen), from 3/4 to 1/4 of its height: reveals what is below.
swipe_content() {
  local b
  b="$(find_node scroll)" || b="0 0 $(screen_size)"
  read -r -a b <<<"$b"
  adb_ shell input swipe $(( (b[0] + b[2]) / 2 )) $(( b[1] + (b[3] - b[1]) * 3 / 4 )) \
    $(( (b[0] + b[2]) / 2 )) $(( b[1] + (b[3] - b[1]) / 4 )) 350
}
# scroll_to MODE TEXT [swipes]: swipe until MODE TEXT is on screen (default 8 swipes).
scroll_to() {
  local i
  for ((i = 0; i < ${3:-8}; i++)); do
    has_text "$1" "$2" && return 0
    swipe_content
  done
  has_text "$1" "$2"
}
shot() { adb_ exec-out screencap -p >"$OUT/shots/$1.png" || log "screencap $1 failed"; }
evidence() { shot "$1"; dump_ui >"$OUT/ui/$1.xml" || log "UI dump $1 failed"; }

focus() { sh_ dumpsys window | sed -n 's/.*mCurrentFocus=Window{[^ ]* [^ ]* \([^ }]*\)}.*/\1/p' | head -1; }
# A force-stop or `pm clear` removes the app's task, and the system kills the
# package's processes once that task's 1 s destroy timeout runs out
# (ActivityTaskSupervisor KILL_TASK_PROCESSES_TIMEOUT_MS, "Killing …: remove
# task" in logcat): a start inside that second loses its new process. stop_app
# sets the end of that window (µs); a start waits for it.
_TASK_KILL_END=0
_now_us() { echo "${EPOCHREALTIME//[.,]/}"; }
_am_start() {
  local r w=$(( _TASK_KILL_END - $(_now_us) ))
  (( w <= 0 )) || sleep "$((w / 1000000)).$(printf '%06d' $((w % 1000000)))"
  r="$(sh_ am start "$@")"
  [[ "$r" != *Error* ]] || { log "am start $*: $r"; return 1; }
}
# _start_up AM-START-ARGS...: start the activity and wait (15 s) until the app
# is up; when it is not, start it once more.
_start_up() {
  _am_start "$@" && wait_for 15 app_up && return 0
  log "start: app not up ($(focus)), starting again"
  _am_start "$@" && wait_for 15 app_up
}
launch() { _start_up -n "${COMP[MainActivity]}"; }
# open_dest DEST: the app's open intent (DEST: a DEST[...] wire value or any other).
open_dest() { _start_up -a "$ACTION_OPEN" -n "${COMP[MainActivity]}" --es "$EXTRA_DEST" "$1"; }
app_pid() { sh_ pidof "$APP_ID"; return 0; }
app_up() { [[ -n "$(app_pid)" && "$(focus)" == "$APP_ID"* ]]; }
# pid_released PID: the system no longer hosts process PID. `pidof` loses a killed
# process at once, but ActivityManager keeps its record until it hears of the death;
# a launch before then goes to the dead process's activity and is finished with it.
pid_released() { [[ "$(app_pid)" != "$1" ]] && ! sh_ dumpsys activity processes "$APP_ID" | grep " $1:$APP_ID/" >/dev/null; }
# stop_app [HOW [PERMISSION...]]: end the app's process and return once the
# system has let go of it (pid_released); 1 when it still hosts it after 10 s.
# HOW: force-stop (default); clear (force-stop + `pm clear`); kill (`am kill`,
# a background process only); sigkill (kill -9, the low-memory killer's way);
# crash (`am crash`); revoke PERMISSION... (android.permission.PERMISSION, which
# the platform kills a running app for).
stop_app() {
  local how="${1:-force-stop}" pid p
  shift $(($# > 0))
  pid="$(app_pid)"
  case "$how" in
    force-stop|clear)
      adb_ shell am force-stop "$APP_ID"
      [[ "$how" == force-stop ]] || adb_ shell pm clear "$APP_ID" >/dev/null
      _TASK_KILL_END=$(( $(_now_us) + 1500000 )) ;;
    kill) adb_ shell am kill "$APP_ID" ;;
    sigkill) [[ -z "$pid" ]] || adb_ shell kill -9 "$pid" ;;
    crash)
      adb_ shell am crash "$APP_ID"
      # The crash dialog shows for a crash within 2 min of the app's previous one
      # (AppErrors MIN_CRASH_INTERVAL), and the process lives until it is answered.
      if wait_text eq "Close app" 3; then tap_node eq "Close app"; fi ;;
    revoke) for p in "$@"; do adb_ shell pm revoke "$APP_ID" "android.permission.$p"; done ;;
    *) log "stop_app: unknown HOW '$how'"; return 2 ;;
  esac
  [[ -z "$pid" ]] || wait_for 10 pid_released "$pid" || { log "stop_app $how: process $pid still hosted"; return 1; }
}
grant_perms() {
  local p
  for p in ACCESS_FINE_LOCATION ACCESS_COARSE_LOCATION POST_NOTIFICATIONS; do
    adb_ shell pm grant "$APP_ID" "android.permission.$p" || log "pm grant $p failed"
  done
}
# Predicates over device output use `grep >/dev/null`, not `grep -q`: an early
# exit would break the pipe and pipefail would turn a match into a failure.
svc_fg() { sh_ dumpsys activity services "${COMP[RunTrackingService]}" | grep 'isForeground=true' >/dev/null; }
# App files through the root shell (works for the non-debuggable release APK too;
# boot_emulator runs `adb root`).
# app_file NAME: the app's file NAME on stdout; 1 when it is absent.
app_file() { adb_ shell "[ -f $APP_FILES/$1 ]" && adb_ exec-out cat "$APP_FILES/$1"; }
pull_log() { app_file "$EVENT_LOG_FILE" >"$1" || log "no $EVENT_LOG_FILE on the device"; }
# push_log FILE: FILE as the app's event log (owner, mode and SELinux label set).
push_log() {
  local uid r
  r="$(adb_ push "$1" /data/local/tmp/emu-event-log.ndjson 2>&1)" || { log "push_log: $r"; return 1; }
  uid="$(sh_ stat -c %u "/data/data/$APP_ID")"
  r="$(adb_ shell "mkdir -p $APP_FILES && cp /data/local/tmp/emu-event-log.ndjson $APP_FILES/$EVENT_LOG_FILE \
    && chown -R $uid:$uid $APP_FILES && chmod 600 $APP_FILES/$EVENT_LOG_FILE && restorecon -R $APP_FILES \
    && rm /data/local/tmp/emu-event-log.ndjson" 2>&1)" || { log "push_log: $r"; return 1; }
}
log_lines() { app_file "$EVENT_LOG_FILE" | grep -c .; return 0; }
sidecar_points() { app_file "$ACTIVE_RUN_FILE" | grep -c '"lat"'; return 0; }
pull_sidecar() { app_file "$ACTIVE_RUN_FILE" >"$1" || log "no $ACTIVE_RUN_FILE on the device"; }
snapshot() { app_file "$SNAPSHOT_FILE"; }
# jfield PATH: a dotted path ("items.0.name") of the JSON on stdin; empty when absent.
jfield() {
  python3 -c "import json,sys
v=json.load(sys.stdin)
for k in sys.argv[1].split('.'): v=v[int(k)] if isinstance(v,list) else v[k]
print(v)" "$1" 2>/dev/null
}
snap_field() { snapshot | jfield "$1"; }
# The app wrote its widget snapshot for DAY, not marked stale: it replayed its
# log and rendered that day.
snapshot_for() { [[ "$(snap_field epochDay)" == "$(epoch_day "$1")" && "$(snap_field stale)" == False ]]; }
EV() { python3 "$HELP/events.py" "$@"; }
# install_apk APK [pm install flags, e.g. -r]: `adb install`, else a pushed
# `pm install` (on some API 37 images `adb install` fails with "Broken pipe").
# Echoes the status line; succeeds on a "Success" line (adb 37 prints "Install
# command complete in N ms" after it).
install_apk() {
  local apk="$1" out; shift
  out="$(adb_ install "$@" "$apk" 2>&1)"
  if ! grep -qx Success <<<"$out"; then
    adb_ push "$apk" /data/local/tmp/emu-install.apk >/dev/null &&
      out="$(sh_ pm install "$@" /data/local/tmp/emu-install.apk 2>&1)"
  fi
  grep -x Success <<<"$out" || { tail -1 <<<"$out"; return 1; }
}
# Launch time (ms) of the last "Displayed <app>/..." line in the logcat buffer.
displayed_ms() { sh_ logcat -d -s ActivityTaskManager:I | grep "Displayed $APP_ID/" | python3 "$HELP/parse.py" displayed; }
# meminfo FIELD...: parse.py meminfo's FIELDs (KB) of the app's `dumpsys meminfo`.
meminfo() { sh_ dumpsys meminfo "$APP_ID" | python3 "$HELP/parse.py" meminfo "$@"; }
# duration TEXT: seconds of the first duration in TEXT, -1 when none (parse.py).
duration() { python3 "$HELP/parse.py" duration "$1"; }

wipe_app() {
  stop_feeder
  stop_app clear
  baseline_device
  grant_perms
}
# seed_app FILE: start the app once (its data directory), stop it and put FILE in
# place as its event log.
seed_app() {
  launch || log "first launch before the seed: app not up ($(focus))"
  stop_app
  push_log "$1"
}
# boot_day DAY: relaunch the app with the clock on DAY; succeeds once it shows
# Today and wrote its snapshot for DAY.
boot_day() {
  stop_app
  pin_clock "$1"
  launch && wait_text eq "${STR[main_dest_today]}" 40 && wait_for 15 snapshot_for "$1"
}
# boot_week DAY: boot_day RUN_DAY (a first boot starts the plan that day), then
# boot_day DAY.
boot_week() {
  boot_day "$RUN_DAY" || return 1
  [[ "$1" == "$RUN_DAY" ]] || boot_day "$1"
}
# fresh_app DAY SEED-FILE: wipe the app, seed it and boot_week DAY.
fresh_app() {
  wipe_app
  pin_clock "$RUN_DAY"
  seed_app "$2" || return 1
  boot_week "$1"
}
# Seed events in the core's own encoding (EV event: the wire fixture's line of
# that kind); a field a seed doesn't set keeps the fixture's value.
# seed_profile KEY=VALUE...: a 30-year-old's SetProfile (not advanced, no sex or
# resting HR stated), progressing week to week toward maximal strength.
seed_profile() {
  EV event SetProfile progression_cadence=WeekToWeek lift_goal=MaxStrength advanced=false \
    female=null age_years=30.0 resting_hr_bpm=null "$@"
}
# seed_set EXERCISE KG REPS AT: a set at RPE 8 at unix time AT.
seed_set() {
  EV event LogSet exercise="$1" weight_kg="$2" reps="$3" rpe=8.0 hold_sec=null observed_at="$4" entry_id="${4}000"
}
# seed_run KM MINUTES AT: a logged run, no heart rate, type or effort, at AT.
seed_run() {
  EV event LogRun distance_km="$1" duration_min="$2" hr_pct_max=0.0 longest_recent_km=0.0 \
    workout_type=null session_rpe=null avg_hr_bpm=null observed_at="$3" entry_id="${3}001"
}
# seed_basic DAY RUN_DAYS: a set-up lifter (12 weekly sets) with RUN_DAYS run days
# (20 km a week when RUN_DAYS > 0), a Back Squat at DAY 09:00 and a 5 km run at
# DAY 10:00 ($TZ0), as the ndjson event log the app replays on launch.
seed_basic() {
  local y km=0.0
  y="$(TZ="$TZ0" date -d "$1 09:00" +%s)"
  if [[ "$2" -gt 0 ]]; then km=20.0; fi
  seed_profile goal_distance=General concurrent_goal=Strength weekly_sets=12 \
    running_days_per_week="$2" running_km_per_week="$km" bodyweight_kg=85.0 &&
    seed_set "Back Squat" 120.0 3 "$y" &&
    seed_run 5.0 27.5 $((y + 3600))
}

open_set_editor() {
  tap_node eq "${STR[main_log]}" || return 1
  tap_node eq "${STR[main_log_set_title]}" || return 1
  wait_text eq "${STR[log_field_exercise]}" 8 || return 1
  # No exercise chosen yet: the search field shows its hint. Choose Back Squat.
  if has_text eq "${STR[log_exercise_search_hint]}"; then tap_node eq "Back Squat" || return 1; fi
  # The weight row of the chosen exercise (below the fold at large font and
  # display sizes).
  scroll_to pre "${STR[log_field_weight]}, " 4
}
# tap_scroll MODE TEXT: tap MODE TEXT inside the editor, scrolling its content to
# reveal it (large fonts / display sizes push the weight and reps rows below the
# pinned footer).
tap_scroll() {
  local i
  for i in 1 2 3 4 5 6; do
    tap_node "$1" "$2" 1 2>/dev/null && return 0
    swipe_content
  done
  return 1
}
REPS5="${STR[log_field_reps]}, 5"
LEFT2="$(fmt log_effort_cd 2)"
# editor_fill: on the Back Squat editor, +2.5 kg, 5 reps, 2 reps left.
editor_fill() {
  tap_scroll eq "$(fmt ui_cd_adjust_add 2.5 kg)" || return 1
  tap_scroll eq "$REPS5" || return 1
  tap_scroll eq "$LEFT2"
}
# editor_has_values WEIGHT: the editor shows WEIGHT kg with editor_fill's reps
# and reps left selected.
editor_has_values() {
  local x
  x="$(dump_ui)" || return 1
  q eq "${STR[log_field_weight]}, $1 kg" <<<"$x" >/dev/null && q on "$REPS5" <<<"$x" && q on "$LEFT2" <<<"$x"
}
close_editor() {
  if has_text eq "${STR[log_set_done]}"; then tap_node eq "${STR[log_set_done]}"
  else tap_node eq "${STR[main_close]}"; fi
  dismiss_editor
}
# dismiss_editor: after a tap on the editor's Close or Done, confirm Discard when
# asked; an editor opened from the Log sheet returns to it: dismiss that too.
dismiss_editor() {
  wait_for 5 _discard_or_closed
  if has_text eq "${STR[log_discard_confirm]}"; then
    tap_node eq "${STR[log_discard_confirm]}"; wait_gone eq "${STR[log_discard_confirm]}" 5
  fi
  if has_text eq "${STR[main_log_set_sub]}"; then
    adb_ shell input keyevent 4; wait_gone eq "${STR[main_log_set_sub]}" 5
  fi
  return 0
}
set_count() { app_file "$EVENT_LOG_FILE" | EV count /dev/stdin LogSet; }
sets_above() { [[ "$(set_count)" -gt "$1" ]]; }
# The editor's primary: "Log set N" or "Log set N of M".
LOG_SET_RX="${RX[log_primary_set_n]}|${RX[log_primary_set_n_of_n]}"
# tap_log_set: tap the editor's primary near its top edge; succeeds once the
# event log holds one more LogSet.
tap_log_set() {
  local n0; n0="$(set_count)"
  tap_top re "$LOG_SET_RX" || return 1
  wait_for 10 sets_above "$n0"
}

# Log › Track run (GPS): the tracker's pre-start screen.
open_tracker() {
  tap_node eq "${STR[main_log]}" || return 1
  tap_node eq "${STR[main_track_run_title]}" || return 1
  wait_text eq "${STR[track_btn_start]}" 20
}
# open_tracker, then Start once the locate preview has its first fix.
start_run() {
  open_tracker || return 1
  wait_text re "${RX[track_pill_fresh_lock]}|^${STR[track_cd_pill_fresh_lock]}\$" 20 || log "start_run: no GPS lock shown"
  tap_node eq "${STR[track_btn_start]}" || return 1
  wait_text eq "${STR[track_btn_stop]}" 20
}
# finish_run: Stop (bringing the tracker to the front first when needed) → the
# Finish sheet → Save run. FINISH: the sheet's spoken summary ("3.21 kilometers ·
# 23 minutes 41 seconds moving · … average"). Succeeds when the sheet is gone; a
# Log sheet the tracker returns to is dismissed.
FINISH=""
finish_run() {
  local x
  FINISH=""
  app_up || launch
  if ! has_text eq "${STR[track_btn_save_run]}"; then
    has_text eq "${STR[track_btn_stop]}" || tap_node pre "${STR[track_state_recording]}" 2
    tap_node eq "${STR[track_btn_stop]}" 4 || return 1
    wait_text eq "${STR[track_btn_save_run]}" 15 || return 1
  fi
  FINISH="$(node_label re "${RX[track_finish_summary]}")"
  tap_node eq "${STR[track_btn_save_run]}" || return 1
  wait_gone eq "${STR[track_btn_save_run]}" 10 || return 1
  x="$(dump_ui)"
  if q eq "${STR[main_log_set_sub]}" <<<"$x" >/dev/null; then
    adb_ shell input keyevent 4; wait_gone eq "${STR[main_log_set_sub]}" 5
  fi
  return 0
}

# start_feeder NAME [TIMELINE T0]: feeder.py (its ellipse, or TIMELINE from host
# time T0) until stop_feeder; log FEED_LOG=$OUT/logs/feed-NAME.log. Returns once
# the first fix is fed.
FEEDER_PID=""
FEED_LOG=""
start_feeder() {
  stop_feeder
  rm -f "$OUT/feed.stop"
  FEED_LOG="$OUT/logs/feed-$1.log"
  python3 "$HELP/feeder.py" "$ADB" "$SERIAL" "$FEED_LOG" "$OUT/feed.stop" "${@:2}" &
  FEEDER_PID=$!
  [[ $# -gt 1 ]] || wait_for 10 grep -qs $'\tok$' "$FEED_LOG" || log "feeder: no fix accepted (see $FEED_LOG)"
}
stop_feeder() {
  [[ -n "$FEEDER_PID" ]] || return 0
  touch "$OUT/feed.stop"
  wait "$FEEDER_PID"
  FEEDER_PID=""
}

launcher() { sh_ cmd package resolve-activity --brief -a android.intent.action.MAIN -c android.intent.category.HOME | tail -1 | cut -d/ -f1; }
launcher_up() { [[ "$(focus)" == "$(launcher)"* ]]; }
# None of the app's activities is resumed, paused or between states: it is in
# the background (stopped, its state saved) or has no activity.
app_stopped() {
  ! sh_ dumpsys activity activities | sed -n "/Hist .*ActivityRecord{[^}]* $APP_ID\//,/state=/p" |
    grep -E 'state=(INITIALIZING|STARTED|RESUMED|PAUSING|PAUSED|STOPPING)' >/dev/null
}
home() {
  adb_ shell input keyevent 3
  wait_for 5 launcher_up || log "home: launcher not in front ($(focus))"
  wait_for 10 app_stopped || log "home: the app's activity not stopped"
}
# wdump NAME: every window (status bar, shade, launcher, app) into ui/NAME.xml.
wdump() {
  local i
  for i in 1 2 3; do
    adb_ shell uiautomator dump --windows /sdcard/emu-wdump.xml >/dev/null 2>&1 &&
      adb_ exec-out cat /sdcard/emu-wdump.xml >"$OUT/ui/$1.xml" &&
      grep -q '</displays>' "$OUT/ui/$1.xml" && return 0
  done
  return 1
}
wfind() { q "$2" "$3" <"$OUT/ui/$1.xml"; } # wfind DUMP MODE TEXT
# notif NAME ID: the app's notification ID (notif.py; `null` when it is not
# posted), saved as logs/NAME-notif.{txt,json} and printed.
notif() {
  sh_ dumpsys notification --noredact | tee "$OUT/logs/$1-notif.txt" |
    python3 "$HELP/notif.py" "$APP_ID" "$2" >"$OUT/logs/$1-notif.json"
  cat "$OUT/logs/$1-notif.json"
}
# nfield NAME EXPR: EXPR (python, the notif() record bound to r) of logs/NAME-notif.json.
nfield() { python3 -c "import json,sys;r=json.load(open(sys.argv[1]));print(eval(sys.argv[2]) if r else '')" "$OUT/logs/$1-notif.json" "$2" 2>/dev/null; }
# snooze_others: snooze (30 days) every notification other packages have posted.
# System UI posts its keyboard tutorial ("Navigate using your keyboard") ~20 s
# after boot, dated by the real clock: ahead of the pinned one, it ranks first
# on the lock screen and takes the one full slot there.
snooze_others() {
  local k keys
  mapfile -t keys < <(sh_ cmd notification list | grep -v "^[0-9-]*|$APP_ID|")
  for k in "${keys[@]}"; do
    sh_ cmd notification snooze --for $((30 * 86400 * 1000)) "'$k'" >/dev/null
  done
}
promoted() { nfield "$1" "'PROMOTED_ONGOING' in r['flags']"; }
# ocr_find SHOT WORD: "cx cy" of the first word OCR'd as exactly WORD on
# shots/SHOT.png (tesseract; any installed Latin-script language data).
ocr_find() {
  local lang
  lang="$(tesseract --list-langs 2>/dev/null | grep -m1 -xE 'eng|afr|deu|fra')" || return 1
  tesseract "$OUT/shots/$1.png" - --psm 11 -l "$lang" tsv 2>/dev/null |
    awk -F'\t' -v w="$2" '$12 == w { print int($7 + $9 / 2), int($8 + $10 / 2); found = 1; exit } END { exit !found }'
}
shade_open() {
  adb_ shell cmd statusbar expand-notifications
  wait_for 5 shade_expanded && wdump "$1"
}
shade_expanded() { [[ "$(focus)" == NotificationShade* ]]; }
_shade_find() { wdump "$1" && wfind "$1" "$2" "$3" >/dev/null; }
# tap_shade NAME MODE TEXT [ROW]: open the notification shade and tap a node in it
# (the expand command is retried: it is sometimes dropped right after a dump).
# ROW: when the node is missing, expand the collapsed notification whose title
# starts with ROW first (below API 36 an ongoing notification sits collapsed
# under others, actions hidden). Last resort: OCR the first word of TEXT on the
# shade screenshot (shots/NAME.png). On API 37 a counting-down chronometer in
# the shade keeps the UI from going idle, and uiautomator then leaves the shade
# window out of the dump.
tap_shade() {
  local r="" p e i w
  for i in 1 2 3; do
    shade_open "$1" || continue
    r="$(wfind "$1" "$2" "$3")" && break
    r=""
    if [[ -n "${4:-}" ]] && e="$(wfind "$1" expand "$4")"; then
      read -r -a p <<<"$e"; tap_xy "${p[0]}" "${p[1]}"
      wait_for 5 _shade_find "$1" "$2" "$3" && r="$(wfind "$1" "$2" "$3")" && break
      r=""
    fi
  done
  if [[ -z "$r" ]]; then
    shot "$1"
    w="${3#^}"; w="${w%% *}"
    r="$(ocr_find "$1" "$w")" && log "tap_shade: '$w' not in ui/$1.xml; located by OCR at $r on shots/$1.png"
  fi
  [[ -n "$r" ]] || { adb_ shell cmd statusbar collapse; return 1; }
  read -r -a p <<<"$r"; tap_xy "${p[0]}" "${p[1]}"
}
# The status-bar chip text (SystemUI `ongoing_activity_chip_content`), dumped as ui/NAME.xml.
chip() { wdump "$1" && wfind "$1" ridtext ongoing_activity_chip_content 2>/dev/null; }
# sb_shot NAME: shots/NAME.png and its status bar (bounds from ui/NAME.xml) as
# shots/NAME-statusbar.png.
sb_shot() {
  local b
  shot "$1"
  b="$(wfind "$1" rid status_bar 2>/dev/null | cut -d' ' -f3-6)" || b=""
  [[ -n "$b" ]] || { log "sb_shot $1: no status_bar node in ui/$1.xml"; return 0; }
  read -r -a b <<<"$b"
  python3 -c "import sys;from PIL import Image;Image.open(sys.argv[1]).crop(tuple(map(int,sys.argv[3:7]))).save(sys.argv[2])" \
    "$OUT/shots/$1.png" "$OUT/shots/$1-statusbar.png" "${b[@]}"
}
# The reminder alarm's next time ("YYYY-MM-DD HH:MM:SS", device local); empty
# when none is set.
rem_alarm() {
  sh_ dumpsys alarm | grep -A3 "walarm\*:$ACTION_REMINDER" |
    sed -n 's/.*origWhen=\([0-9-]* [0-9:]*\).*/\1/p' | head -1
}
alarm_set() { [[ -n "$(rem_alarm)" ]]; }
# A tile label as Quick Settings shows it: the label, or (API 37's icon-only
# tile, content-desc) "<label>, <subtitle>" when the tile has a subtitle.
TILE_RE="^(${STR[tile_log]}|${STR[tile_log_set]}|${STR[tile_start_run]}|${STR[tile_run_recording]})(, .+)?\$"
# qsfind DUMP MODE TEXT: wfind in the expanded Quick Settings panel of
# ui/DUMP.xml only (the launcher's Today widget has buttons with the tile's
# labels).
qsfind() { q within quick_settings_panel "$2" "$3" <"$OUT/ui/$1.xml"; }
_tile_read() { wdump "$1" && qsfind "$1" re "$TILE_RE" | cut -d' ' -f7-; }
_tile_seen() { [[ -n "$(_tile_read "$1")" ]]; }
# System UI's NotificationShade window (shade, Quick Settings, keyguard) is
# visible: from the first frame of an expand until a collapse has finished.
shade_window_up() {
  sh_ dumpsys window windows |
    awk '/^ *Window #[0-9]+ Window\{.* NotificationShade\}:$/ { w = 1 } w && /isVisible=/ { v = /isVisible=true/; exit } END { exit !v }'
}
shade_window_gone() { ! shade_window_up; }
_qs_shown() { wdump "$1" && q within quick_settings_panel texts <"$OUT/ui/$1.xml" >/dev/null; }
# qs_open NAME: expand Quick Settings, dumped as ui/NAME.xml. An expand sent
# while a collapse is still animating is dropped, so it waits for the shade
# window to go (the keyguard keeps it up), and sends it again while the panel
# stays closed.
qs_open() {
  local i
  keyguard_showing || wait_for 5 shade_window_gone || log "qs_open: the shade window is still up"
  for i in 1 2 3; do
    adb_ shell cmd statusbar expand-settings
    wait_for 5 _qs_shown "$1" && return 0
  done
  log "qs_open: Quick Settings not expanded (ui/$1.xml)"
  return 1
}
# tile_label NAME: open Quick Settings and dump it as ui/NAME.xml; echo the
# tile's label and leave the panel open (collapse it with `cmd statusbar
# collapse`). The subtitle (the widget's title for today) is written to
# ui/NAME.subtitle. The panel binds the tile when it opens (onStartListening),
# so the label is read until two reads agree.
tile_label() {
  local i l w h
  qs_open "$1" || return 1
  read -r w h <<<"$(screen_size)"
  for i in 1 2 3; do
    if wait_for 5 _tile_seen "$1"; then
      l="$(settled _tile_read "$1")"
      if [[ -n "$l" ]]; then
        q within quick_settings_panel texts <"$OUT/ui/$1.xml" | sed -n 's/^[a-z-]*=//p' | grep -E "$TILE_RE" |
          grep -m1 ', ' | sed 's/^[^,]*, //' >"$OUT/ui/$1.subtitle"
        echo "${l%%, *}"; return 0
      fi
    fi
    # The added tile lands on the last QS page.
    adb_ shell input swipe $((w * 5 / 6)) $((h * 3 / 10)) $((w / 7)) $((h * 3 / 10)) 300
  done
  return 1
}
# tile_node DUMP LABEL: "cx cy ..." of the tile labelled LABEL (with or without
# its subtitle) in the Quick Settings panel of ui/DUMP.xml.
# shellcheck disable=SC2016
tile_node() { qsfind "$1" re "^$(printf '%s' "$2" | sed 's/[][\.*^$()+?{}|]/\\&/g')(, .+)?\$"; }
_tile_shown() { tile_label tile-added >/dev/null; local r=$?; adb_ shell cmd statusbar collapse; return $r; }
# add_tile: `cmd statusbar add-tile`, then wait (max ~40 s) until the tile shows
# on the QS panel (it takes a while to bind on API 34).
add_tile() {
  sh_ cmd statusbar add-tile "${COMP[QuickTileService]}" >/dev/null
  wait_for 40 _tile_shown || { log "tile ${COMP[QuickTileService]} not on the QS panel after add-tile"; return 1; }
}
# drag X1 Y1 X2 Y2: a long-press drag as raw motion events (the launcher's
# widget-picker preview starts a drag only after a held touch; `input
# draganddrop` does not hold long enough on API 34): 2 s held, 8 moves, 1 s at
# the target.
drag() {
  local i
  adb_ shell input motionevent DOWN "$1" "$2"; sleep 2
  for i in 1 2 3 4 5 6 7 8; do
    adb_ shell input motionevent MOVE $(( $1 + ($3 - $1) * i / 8 )) $(( $2 + ($4 - $2) * i / 8 ))
  done
  sleep 1
  adb_ shell input motionevent UP "$3" "$4"
}
widget_bound() { sh_ dumpsys appwidget | grep "cmp:ComponentInfo{${COMP[TodayWidgetReceiver]}}" >/dev/null; }
# "cx cy" of the home screen's empty grid (the launcher's workspace node; else
# the middle of the screen).
home_grid() {
  local r w h
  if r="$(find_node rid workspace)"; then cut -d' ' -f1-2 <<<"$r"
  else read -r w h <<<"$(screen_size)"; echo "$((w / 2)) $((h * 55 / 100))"; fi
}
# Place the Today widget through the launcher's picker on a reset home screen
# (launcher data cleared, so a long-press lands on empty grid): long-press home →
# Widgets → search the app → its widget → tap the preview → "Add <widget> widget"
# (API 35+), else (API 34: no Add button) drag the preview image
# (`widget_preview_container`) onto the home grid.
place_widget() {
  local r p g preview='^[0-9]+ wide by [0-9]+ high$' name="${STR[app_name]}" label="${STR[widget_overline_today]}"
  adb_ shell pm clear "$(launcher)" >/dev/null
  home
  evidence widget-home-clean
  read -r -a g <<<"$(home_grid)"
  adb_ shell input swipe "${g[0]}" "${g[1]}" "${g[0]}" "${g[1]}" 1500
  tap_node eq Widgets 5 || return 1
  if wait_text eq Search 5 && tap_node eq Search 2; then
    adb_ shell input text "$name"
  fi
  scroll_to eq "$name" 6 || return 1
  wait_text re "$preview" 2 || { tap_node eq "$name"; wait_text re "$preview" 5; } ||
    { tap_node eq "$label" 2; wait_text re "$preview" 5; }
  evidence widget-picker
  has_text eq "Add $label widget" || { tap_node re "$preview" 2; wait_text eq "Add $label widget" 5; }
  evidence widget-picker-preview
  if has_text eq "Add $label widget"; then
    tap_node eq "Add $label widget"
  else
    r="$(find_node rid widget_preview_container)" || r="$(find_node rid widget_preview)" || return 1
    read -r -a p <<<"$r"
    drag "${p[0]}" "${p[1]}" "${g[0]}" "${g[1]}"
  fi
  wait_for 10 widget_bound || return 1
  # The launcher leaves the new widget in resize mode (no idle UI state for
  # uiautomator until it is dismissed).
  home
  evidence widget-placed
  wait_text eq "${STR[widget_open_today_cd]}" 15
}
_widget_texts() { dump_ui | q texts | sed -n "/content-desc=${STR[widget_open_today_cd]}/,\$p" | head -12; }
# widget_texts NAME: the widget's labels (home screen dump) one per line, once
# they stop changing.
widget_texts() {
  home
  settled _widget_texts
  evidence "$1"
}
# widget_dp NAME: the size the launcher gave the widget, "W H" in whole dp, from
# its root node in the dump saved under NAME.
widget_dp() {
  local d b
  d="$(sh_ wm density | sed -n 's/.*: //p' | tail -1)"
  b="$(q eq "${STR[widget_open_today_cd]}" <"$OUT/ui/$1.xml")" || return 1
  awk -v d="$d" '{ printf "%d %d\n", ($5 - $3) * 160 / d, ($6 - $4) * 160 / d }' <<<"$b"
}

MONKEY_ACTIVE=0
# monkey_run DIR SEED EVENTS THROTTLE CHUNK: a seeded monkey pass restricted to
# the app, CHUNK events per monkey call (chunk N seeded SEED*1000+N), with System
# UI expansion, Home and Recents disabled; a chunk that leaves the app is
# followed by Back and a relaunch. Output: DIR/chunk-N.txt, DIR/escapes.txt and
# MONKEY="injected crashes anrs escapes finished-chunks chunks".
MONKEY=""
monkey_run() {
  local dir="$1" seed="$2" events="$3" throttle="$4" chunk="$5" chunks i n f injected=0 crashes=0 anrs=0 escapes=0 fin=0
  mkdir -p "$dir"
  chunks=$(( (events + chunk - 1) / chunk ))
  adb_ shell cmd statusbar send-disable-flag statusbar-expansion home recents || log "monkey: System UI flags not set"
  MONKEY_ACTIVE=1
  for ((i = 0; i < chunks; i++)); do
    n=$chunk; [[ $(( (i + 1) * chunk )) -le $events ]] || n=$(( events - i * chunk ))
    timeout 1200 "$ADB" -s "$SERIAL" shell monkey -p "$APP_ID" -s $(( seed * 1000 + i )) \
      --throttle "$throttle" --pct-syskeys 2 --pct-appswitch 1 --pct-anyevent 0 \
      --ignore-crashes --ignore-timeouts --ignore-security-exceptions --monitor-native-crashes \
      -v "$n" >"$dir/chunk-$i.txt" 2>&1 || log "monkey chunk $i exited $? (see $dir/chunk-$i.txt)"
    injected=$(( injected + $(sed -n 's/.*Events injected: \([0-9]*\).*/\1/p' "$dir/chunk-$i.txt" | tail -1 | grep . || echo 0) ))
    crashes=$(( crashes + $(grep -c "// CRASH: $APP_ID" "$dir/chunk-$i.txt") ))
    anrs=$(( anrs + $(grep -c "// NOT RESPONDING: $APP_ID" "$dir/chunk-$i.txt") ))
    # Monkey prints this last line (with -v) only after injecting every event; a
    # chunk that aborted early (no activity, wrong package, adb error) has none.
    grep -q '// Monkey finished' "$dir/chunk-$i.txt" && fin=$((fin + 1))
    f="$(focus)"
    if [[ "$f" != "$APP_ID"* ]]; then
      escapes=$((escapes + 1)); echo "chunk $i: focus=$f" >>"$dir/escapes.txt"
      adb_ shell input keyevent 4
      launch
    fi
  done
  monkey_restore
  MONKEY="$injected $crashes $anrs $escapes $fin $chunks"
}
monkey_restore() {
  adb_ shell cmd statusbar send-disable-flag none || log "monkey: System UI flags not cleared"
  adb_ shell cmd statusbar collapse
  MONKEY_ACTIVE=0
}
# monkey_checks SCENARIO NAME DIR: checks.tsv rows of the last monkey_run.
monkey_checks() {
  local injected crashes anrs escapes fin chunks
  read -r injected crashes anrs escapes fin chunks <<<"$MONKEY"
  expect "$1" "$2: no crash or ANR reported by monkey" "$crashes crash(es), $anrs ANR(s) in $injected events; $3" \
    test "$crashes" -eq 0 -a "$anrs" -eq 0
  expect "$1" "$2: every chunk ran to the end" "$fin of $chunks chunks finished; $3" test "$fin" -eq "$chunks"
  [[ $escapes -eq 0 ]] || observe "$1" "$2: left the app" "$escapes chunk(s); relaunched ($3/escapes.txt)"
}

EMU_PID=""
LOGCAT_PID=""
API=0
# boot_emulator AVD LOG [MEMORY_MB]: use $SERIAL when it is already online, else
# boot AVD headless on the serial's port (output to LOG). Then wait for the boot
# and restart adbd as root (where the image allows it).
boot_emulator() {
  local avd="$1" logf="$2" mem="${3:-}"
  if adb_ get-state >/dev/null 2>&1; then
    log "$SERIAL already online: using it"
  else
    [[ -x "$EMULATOR" ]] || die "emulator not found at $EMULATOR"
    "$EMULATOR" -list-avds | grep -qx "$avd" || die "AVD '$avd' not found (create it with avdmanager)"
    # An AVD created by cmdline-tools < 20 has `target=android-0`: the emulator
    # then skips its GL feature setup and surfaceflinger aborts in a loop
    # ("hasReadColorBufferDma"), which breaks screencap and restarts system_server.
    ! grep -qx 'target=android-0' "$HOME/.android/avd/$avd.ini" ||
      die "$HOME/.android/avd/$avd.ini has target=android-0: set it to the image's API (e.g. target=android-37.0)"
    [[ -e /dev/kvm ]] || log "WARNING: /dev/kvm missing: the emulator will be very slow"
    log "booting AVD $avd on port ${SERIAL#emulator-}"
    # The emulator finds the AVD's system image through ANDROID_SDK_ROOT, not its
    # own install path; a stale environment value would point it at another SDK.
    ANDROID_SDK_ROOT="$ANDROID_HOME" "$EMULATOR" -avd "$avd" -port "${SERIAL#emulator-}" \
      -no-window -no-audio -no-boot-anim -no-snapshot -gpu swiftshader_indirect -read-only \
      ${mem:+-memory "$mem"} >"$logf" 2>&1 &
    EMU_PID=$!
  fi
  wait_for 300 _booted || die "emulator boot timed out (see $logf)"
  root_adbd
}
# root_adbd: restart adbd as root (where the image allows it) and wait until the
# device is back.
root_adbd() {
  local r; r="$(adb_ root 2>&1)"
  case "$r" in
    *"cannot run as root"*) log "adb root: $r" ;;
    *) wait_for 60 _root_booted || die "adbd did not come back as root: $r" ;;
  esac
}
_booted() {
  [[ -z "$EMU_PID" ]] || kill -0 "$EMU_PID" 2>/dev/null || die "emulator exited during boot"
  [[ "$(sh_ getprop sys.boot_completed)" == 1 ]]
}
_root_booted() { [[ "$(sh_ id -u)" == 0 ]] && _booted; }
_emu_gone() { ! kill -0 "$EMU_PID" 2>/dev/null; }
# Stop the emulator boot_emulator started, unless KEEP_EMU=1. One that was
# already running is left alone.
stop_emulator() {
  [[ -n "$EMU_PID" && "$KEEP_EMU" -ne 1 ]] || return 0
  log "stopping emulator $SERIAL"
  adb_ emu kill >/dev/null || log "console kill refused"
  # The console kill is not implemented on every emulator build; then signal the
  # process boot_emulator started, never anything else.
  wait_for 15 _emu_gone || kill "$EMU_PID"
  wait "$EMU_PID"
}
logcat_capture() {
  adb_ logcat -v threadtime -b main,system,crash,events >>"$OUT/logcat.txt" 2>&1 &
  LOGCAT_PID=$!
}
# session_start AVD [MEMORY_MB]: boot (or reuse) the emulator, record the device
# in config.txt (API), clear the device log and start its capture. The caller
# installs `trap session_cleanup EXIT` first.
session_start() {
  config avd "$1"
  boot_emulator "$1" "$OUT/logs/emulator.log" "${2:-}"
  API="$(sh_ getprop ro.build.version.sdk)"
  config device "$(sh_ getprop ro.product.model), API $API ($(sh_ getprop ro.build.version.release)), heapgrowthlimit $(sh_ getprop dalvik.vm.heapgrowthlimit)"
  adb_ logcat -c
  adb_ logcat -G 16M || log "logcat buffer size not set"
  logcat_capture
}
session_cleanup() {
  stop_feeder
  [[ -z "$LOGCAT_PID" ]] || kill "$LOGCAT_PID" 2>/dev/null
  [[ "$MONKEY_ACTIVE" -eq 0 ]] || monkey_restore
  stop_emulator
}
# logcat_checks: stop the logcat capture once it holds the last phase marker,
# scan $OUT/logcat.txt (scan_logcat.py) and add its crash / ANR / OOM / FGS /
# StrictMode rows to checks.tsv. A scan that fails is a FAIL row.
logcat_checks() {
  mark logcat-end
  wait_for 10 grep -q "$PHASE_TAG.*phase logcat-end" "$OUT/logcat.txt" || log "logcat capture lags: scanning what it has"
  kill "$LOGCAT_PID"; wait "$LOGCAT_PID"; LOGCAT_PID=""
  python3 "$HELP/scan_logcat.py" "$APP_ID" "$PHASE_TAG" "$OUT/logcat.txt" "$OUT/logcat-scan.json" >/dev/null ||
    { check FAIL logcat "logcat scan" "scan_logcat.py failed on $OUT/logcat.txt"; return 0; }
  python3 - "$OUT/logcat-scan.json" "$OUT/checks.tsv" <<'PY'
import json, sys
s = json.load(open(sys.argv[1]))
out = open(sys.argv[2], "a")
for k, label in (("fatal", "Java FATAL EXCEPTION (not induced)"), ("native", "native crash"), ("anr", "ANR"),
                 ("oom", "OutOfMemoryError"), ("fgs", "foreground-service failure"), ("strictmode", "StrictMode")):
    out.write(f"{'PASS' if s[k] == 0 else 'FAIL'}\tlogcat\t{label}\t{s[k]} occurrence(s) {json.dumps(s['by_phase'].get(k, {}))}\n")
PY
}
# session_end TITLE: logcat_checks, REPORT.md titled TITLE; succeeds when no
# check FAILed.
session_end() {
  logcat_checks
  python3 "$HELP/report.py" "$1" "$OUT"
  log "report: $OUT/REPORT.md"
  ! grep -q '^FAIL' "$OUT/checks.tsv"
}

# pin_clock DAY [HH:MM[:SS]] [TZ]: the device clock and zone (default 08:00, $TZ0).
pin_clock() {
  local tz="${3:-$TZ0}"
  sh_ cmd alarm set-timezone "$tz" >/dev/null
  sh_ cmd alarm set-time "$(TZ="$tz" date -d "$1 ${2:-08:00}" +%s)000" >/dev/null
}
screen_on() { sh_ dumpsys power | grep 'mWakefulness=Awake' >/dev/null; }
keyguard_showing() { sh_ dumpsys window | grep -E '(isKeyguardShowing|mKeyguardShowing)=true' >/dev/null; }
_unlocked() { ! keyguard_showing; }
wake() {
  adb_ shell input keyevent 224
  wait_for 5 screen_on || log "wake: screen still off"
  adb_ shell wm dismiss-keyguard
  wait_for 5 _unlocked || log "wake: keyguard still showing"
}
baseline_device() {
  adb_ shell settings put global auto_time 0
  adb_ shell settings put global auto_time_zone 0
  adb_ shell settings put system time_12_24 12
  adb_ shell settings put system font_scale 1.0
  adb_ shell settings put system screen_off_timeout 1800000
  adb_ shell wm density reset
  adb_ shell cmd uimode night no >/dev/null
  adb_ shell settings put global low_power 0
  adb_ shell dumpsys battery reset
  sh_ cmd location set-location-enabled true >/dev/null
  adb_ shell cmd statusbar collapse
  sh_ cmd locale set-app-locales "$APP_ID" >/dev/null
  wake
  adb_ shell input keyevent 82
}
