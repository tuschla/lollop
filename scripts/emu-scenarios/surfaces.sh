#!/usr/bin/env bash
# On-device checks for lollop's system surfaces (planning/design/
# visual-design-system.md § 10), each with explicit PASS/FAIL rows and evidence:
#   links   app.milestone.action.OPEN with dest=today|log|track|log_set (+ an
#           unknown value), cold start and warm (onNewIntent)
#   tile    Quick Settings tile added with `cmd statusbar add-tile`: label per
#           day state (lift / run / rest / hold / stale) and a real tap on it
#           opening the right screen
#   widget  Today widget placed through the launcher's widget picker: run day,
#           lift day, hold, a date change (stale text), and its three tap targets
#   run     run notification during a fed GPS run: GPS… / moving time / Paused /
#           No GPS chip (status-bar chip read from the SystemUI window with the
#           shade closed), promoted flag, tap → tracker, tile "Run · recording"
#   guided  a guided run/walk (C25k plan): ProgressStyle step bar
#   rest    rest-timer countdown after a set in the loop: chronometer chip,
#           "Resting · …" / "Next: …", tap → set editor, rest-done alert
#   restbg  (opt-in) rest-done alert with the app cached, frozen or killed, with
#           SCHEDULE_EXACT_ALARM allowed (on time) and denied (may be late, must
#           arrive); the loop's exact-alarm notice
#   tilefresh (opt-in, run alone on a freshly booted emulator) the tile's
#           run-day, stale-day and recording cases with no force-stop or
#           `pm clear` after the tile is added
# The Live Update checks (chip, promoted flag, toggle) need API 36+; below that
# the same flows check the plain notification and that the Profile notice stays
# hidden.
#
#   scripts/emu-scenarios/surfaces.sh [--avd e37] [--port 5582] [--apk PATH]
#                                     [--only "links tile widget run guided rest toggle restbg tilefresh"]
#                                     [--keep-emulator]
#
# Outputs: a new folder per run, ~/.cache/emu-surfaces/<avd>/<YYYYmmdd-HHMMSS>/
# (REPORT.md, checks.tsv, logcat.txt, logcat-scan.json, shots/, ui/, logs/;
# earlier runs are kept). The device clock is pinned per check
# (Mon 2026-09-21 = run day, Tue 22 = rest day, Wed 23 = lift day for the seeded
# hybrid profile, Europe/Berlin). Exit 0 when no check FAILed. The emulator is
# stopped at the end unless --keep-emulator or it was already running.
#
# API 37 AVDs: an AVD created by cmdline-tools < 20 has `target=android-0` in
# ~/.android/avd/<avd>.ini; the emulator then skips its GL feature setup and
# surfaceflinger aborts in a loop ("hasReadColorBufferDma"), which breaks
# screencap and restarts system_server. The boot step detects this and stops;
# fix the .ini to `target=android-37.0`.
set -uo pipefail

cd "$(dirname "$0")/../.."
REPO_ROOT="$(pwd)"
HELP="$REPO_ROOT/scripts/emu-scenarios"
APP_ID="app.milestone"
AVD="e37"
PORT=5582
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
ONLY="links tile widget run guided rest toggle"
KEEP_EMU=0
TZ0="Europe/Berlin"
RUN_DAY="2026-09-21"
REST_DAY="2026-09-22"
LIFT_DAY="2026-09-23"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --avd) AVD="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --apk) APK="$2"; shift 2 ;;
    --only) ONLY="$2"; shift 2 ;;
    --keep-emulator) KEEP_EMU=1; shift ;;
    -h|--help) sed -n '2,42p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

SERIAL="emulator-$PORT"
OUT="$HOME/.cache/emu-surfaces/$AVD/$(date +%Y%m%d-%H%M%S)"
SDK="$(sed -n 's/^sdk\.dir=//p' "$REPO_ROOT/android/local.properties" 2>/dev/null | tail -1)"
SDK="${SDK:-${ANDROID_HOME:-/home/lt/android-home}}"
ADB="$SDK/platform-tools/adb"
EMULATOR="$SDK/emulator/emulator"
[[ -f "$APK" ]] || { echo "APK not found: $APK" >&2; exit 2; }

[[ -e "$OUT" ]] && { echo "output folder exists: $OUT" >&2; exit 2; }
mkdir -p "$OUT"/{shots,ui,logs}
: >"$OUT/checks.tsv"
# shellcheck source=lib.sh
source "$HELP/lib.sh"
EV() { python3 "$HELP/events.py" "$@"; }
FEEDER_PID=""
STARTED_EMU=0
API=0

# ── Emulator ─────────────────────────────────────────────────────────────────
boot_emulator() {
  if adb_ get-state >/dev/null 2>&1; then
    log "$SERIAL already online — using it"
  else
    log "booting AVD $AVD on port $PORT"
    ANDROID_SDK_ROOT="$SDK" ANDROID_HOME="$SDK" "$EMULATOR" -avd "$AVD" -port "$PORT" \
      -no-window -no-audio -no-boot-anim -no-snapshot -gpu swiftshader_indirect -read-only \
      -memory 3072 >"$OUT/logs/emulator.log" 2>&1 &
    STARTED_EMU=1
  fi
  local tries=0
  until [[ "$(sh_ getprop sys.boot_completed)" == "1" ]]; do
    tries=$((tries + 1)); [[ $tries -lt 150 ]] || { log "emulator boot timed out"; exit 1; }
    sleep 2
  done
  adb_ root >/dev/null 2>&1; sleep 2; adb_ wait-for-device
  until [[ "$(sh_ getprop sys.boot_completed)" == "1" ]]; do sleep 2; done
  # boot_completed is not enough: a surfaceflinger abort loop starts after it.
  local sf; sf="$(sh_ pidof surfaceflinger)"; sleep 20
  if [[ "$(sh_ pidof surfaceflinger)" != "$sf" ]]; then
    log "surfaceflinger restarted after boot ($(sh_ logcat -d -b crash | grep -m1 'Abort message' || true));" \
      "check target= in ~/.android/avd/$AVD.ini (see the header)"
    exit 1
  fi
}
stop_emulator() {
  [[ $KEEP_EMU -eq 1 ]] && return 0
  log "stopping emulator $SERIAL"
  adb_ emu kill >/dev/null 2>&1 || true
}
# `adb install` streams over the package service; on some API 37 images that
# fails with "Broken pipe" while a pushed `pm install` works.
install_apk() {
  adb_ install -r "$APK" >/dev/null 2>&1 && return 0
  adb_ push "$APK" /data/local/tmp/surfaces.apk >/dev/null && sh_ pm install -r /data/local/tmp/surfaces.apk | grep -q Success
}

# ── Device state ─────────────────────────────────────────────────────────────
pin_clock() { # pin_clock YYYY-MM-DD [HH:MM]
  sh_ cmd alarm set-timezone "$TZ0" >/dev/null
  sh_ cmd alarm set-time "$(TZ="$TZ0" date -d "$1 ${2:-08:00}" +%s)000" >/dev/null
}
baseline_device() {
  adb_ shell settings put global auto_time 0
  adb_ shell settings put global auto_time_zone 0
  adb_ shell settings put system time_12_24 12
  adb_ shell settings put system screen_off_timeout 1800000
  sh_ cmd location set-location-enabled true >/dev/null
  adb_ shell cmd statusbar collapse >/dev/null 2>&1 || true
  wake
  adb_ shell input keyevent 82 >/dev/null 2>&1 || true
}
# The seeded hybrid lifter-runner (2 run days, 12 weekly sets) with a squat and a
# 5 km run on Sun 2026-09-20: Mon run, Tue rest, Wed lift.
seed_basic() {
  local y; y="$(TZ="$TZ0" date -d "2026-09-20 09:00" +%s)"
  cat <<EOF
{"SetProfile":{"progression_cadence":"WeekToWeek","lift_goal":"MaxStrength","goal_distance":"General","concurrent_goal":"Strength","weekly_sets":12,"running_days_per_week":2,"running_km_per_week":20.0,"advanced":false,"endurance_intensity_pct_vo2max":75.0,"bodyweight_kg":85.0,"age_years":30.0}}
{"LogSet":{"exercise":"Back Squat","weight_kg":120.0,"reps":3,"rpe":8.0,"observed_at":${y},"entry_id":${y}000}}
{"LogRun":{"distance_km":5.0,"duration_min":27.5,"hr_pct_max":0.0,"longest_recent_km":0.0,"observed_at":$((y + 3600)),"entry_id":$((y + 3600))001}}
EOF
}
# seed_basic plus a pain report on DAY 07:30: training paused.
seed_hold() { # seed_hold DAY
  seed_basic
  echo "{\"SubmitReadiness\":{\"signal\":\"Pain\",\"value\":1.0,\"observed_at\":$(TZ="$TZ0" date -d "$1 07:30" +%s),\"streak\":0,\"pain\":null,\"effort_min\":null}}"
}
# A beginner runner on a couch-to-5k plan: run/walk bouts (a guided run).
seed_c25k() {
  local y; y="$(TZ="$TZ0" date -d "2026-09-20 09:00" +%s)"
  cat <<EOF
{"SetProfile":{"progression_cadence":"WeekToWeek","lift_goal":"MaxStrength","goal_distance":"C25k","concurrent_goal":"EndurancePriority","weekly_sets":0,"running_days_per_week":3,"running_km_per_week":5.0,"advanced":false,"endurance_intensity_pct_vo2max":70.0,"bodyweight_kg":80.0,"age_years":30.0}}
{"LogRun":{"distance_km":2.0,"duration_min":20.0,"hr_pct_max":0.0,"longest_recent_km":0.0,"observed_at":${y},"entry_id":${y}001}}
EOF
}
# The app writes GeneratePlan{start_epoch_day} on the first boot of a seeded
# profile, so every seed boots on Mon (the plan starts there: Mon run, Tue rest,
# Wed lift) and the clock then moves to the wanted day.
# goto_day DAY: relaunch the app on DAY.
goto_day() {
  adb_ shell am force-stop "$APP_ID"
  pin_clock "$1"
  launch
  wait_text eq Today 40 && sleep 3
}
# seed_boot DAY SEED-FN [ARG]: push the seed, boot it on Mon, land on DAY.
seed_boot() {
  local day="$1"; shift
  adb_ shell am force-stop "$APP_ID"
  "$@" >"$OUT/logs/seed-$1.ndjson"
  push_log "$OUT/logs/seed-$1.ndjson" >/dev/null 2>&1 || return 1
  goto_day "$RUN_DAY" || return 1
  [[ "$day" == "$RUN_DAY" ]] || goto_day "$day"
}
# fresh_app DAY SEED-FN [ARG]: wipe data, then seed_boot.
fresh_app() {
  stop_feeder
  adb_ shell am force-stop "$APP_ID"
  adb_ shell pm clear "$APP_ID" >/dev/null
  baseline_device
  grant_perms
  pin_clock "$RUN_DAY"
  launch; sleep 4
  seed_boot "$@"
}
# reseed DAY SEED-FN [ARG]: seed_boot without `pm clear` (keeps a placed widget
# bound and the app out of the stopped state, so it still gets broadcasts).
reseed() { stop_feeder; seed_boot "$@"; }
snapshot() { adb_ exec-out cat "$APP_FILES/widget-today.json" 2>/dev/null; }
snap_field() { snapshot | python3 -c "import json,sys;d=json.load(sys.stdin);v=d;[v:=v[k] if isinstance(v,dict) else v[int(k)] for k in sys.argv[1].split('.')];print(v)" "$1" 2>/dev/null; }

# ── Screens ──────────────────────────────────────────────────────────────────
# Which app screen is in front: today | log | track | log_set | other | none.
screen() {
  local x; sleep 1
  [[ "$(focus)" == "$APP_ID"* ]] || { echo none; return; }
  x="$(dump_ui)" || { echo other; return; }
  local q="python3 $HELP/node.py"
  if $q eq Exercise <<<"$x" >/dev/null; then echo log_set
  elif $q eq "Track run (GPS)" <<<"$x" >/dev/null && $q eq "Log set" <<<"$x" >/dev/null; then echo log
  elif $q eq "© OpenStreetMap" <<<"$x" >/dev/null; then echo track
  elif $q on Today <<<"$x" >/dev/null; then echo today
  else echo other; fi
}
# wait_screen NAME [seconds]
wait_screen() { local i s=""; for i in $(seq 1 "${2:-10}"); do s="$(screen)"; [[ "$s" == "$1" ]] && return 0; sleep 1; done; echo "$s" >"$OUT/.last_screen"; return 1; }
last_screen() { cat "$OUT/.last_screen" 2>/dev/null; }

# ── System UI ────────────────────────────────────────────────────────────────
# Every window (status bar, shade, launcher, app) into ui/NAME.xml.
wdump() {
  local i
  for i in 1 2 3; do
    adb_ shell uiautomator dump --windows /sdcard/surf.xml >/dev/null 2>&1 &&
      adb_ exec-out cat /sdcard/surf.xml >"$OUT/ui/$1.xml" 2>/dev/null &&
      grep -q '</displays>' "$OUT/ui/$1.xml" && return 0
    sleep 1
  done
  return 1
}
wfind() { python3 "$HELP/node.py" "$2" "$3" <"$OUT/ui/$1.xml"; } # wfind DUMP MODE TEXT
# The status-bar chip text (SystemUI `ongoing_activity_chip_content`), dumped as ui/NAME.xml.
chip() { wdump "$1" && wfind "$1" ridtext ongoing_activity_chip_content 2>/dev/null; }
# Screenshot + its status-bar strip (shots/NAME.png, shots/NAME-statusbar.png).
sb_shot() {
  shot "$1"
  python3 -c "from PIL import Image;Image.open('$OUT/shots/$1.png').crop((0,0,1080,150)).save('$OUT/shots/$1-statusbar.png')" 2>/dev/null || true
}
notif() { sh_ dumpsys notification --noredact | tee "$OUT/logs/$1-notif.txt" | python3 "$HELP/notif.py" "$APP_ID" "$2" >"$OUT/logs/$1-notif.json"; cat "$OUT/logs/$1-notif.json"; }
# jq-lite over a notif() record: nfield NAME EXPR (python, record bound to r).
nfield() { python3 -c "import json,sys;r=json.load(open(sys.argv[1]));print(eval(sys.argv[2]) if r else '')" "$OUT/logs/$1-notif.json" "$2" 2>/dev/null; }
promoted() { nfield "$1" "'PROMOTED_ONGOING' in r['flags']"; }
# tap_shade NAME MODE TEXT [ROW]: open the notification shade and tap a node in it
# (the expand command is retried: it is sometimes dropped right after a dump).
# ROW: when the node is missing, expand the collapsed notification whose title
# starts with ROW first (below API 36 an ongoing notification sits collapsed
# under others, actions hidden). Last resort: OCR the first word of TEXT on the
# shade screenshot (shots/NAME.png) — on API 37 a counting-down chronometer in
# the shade keeps the UI from going idle, and uiautomator then leaves the shade
# window out of the dump.
tap_shade() {
  local r="" p e i
  for i in 1 2 3; do
    adb_ shell cmd statusbar expand-notifications; sleep 2
    wdump "$1" || continue
    r="$(wfind "$1" "$2" "$3")" && break
    r=""
    if [[ -n "${4:-}" ]] && e="$(wfind "$1" expand "$4")"; then
      read -r -a p <<<"$e"; tap_xy "${p[0]}" "${p[1]}"; sleep 2
      wdump "$1" && r="$(wfind "$1" "$2" "$3")" && break
      r=""
    fi
  done
  if [[ -z "$r" ]]; then
    shot "$1"
    r="$(ocr_find "$1" "${3%% *}")" && log "tap_shade: '${3%% *}' not in ui/$1.xml; located by OCR at $r on shots/$1.png"
  fi
  [[ -n "$r" ]] || { adb_ shell cmd statusbar collapse; return 1; }
  read -r -a p <<<"$r"; tap_xy "${p[0]}" "${p[1]}"; sleep 3
}
# ocr_find SHOT WORD: "cx cy" of the first word OCR'd as exactly WORD on
# shots/SHOT.png (tesseract; any installed Latin-script language data).
ocr_find() {
  local lang
  lang="$(tesseract --list-langs 2>/dev/null | grep -m1 -xE 'eng|afr|deu|fra')" || return 1
  tesseract "$OUT/shots/$1.png" - --psm 11 -l "$lang" tsv 2>/dev/null |
    awk -F'\t' -v w="$2" '$12 == w { print int($7 + $9 / 2), int($8 + $10 / 2); found = 1; exit } END { exit !found }'
}
home() { adb_ shell input keyevent 3; sleep 2; }
mmss() { python3 -c "import re,sys;m=re.fullmatch(r'(?:(\d+):)?(\d+):(\d\d)',sys.argv[1].strip());print(int(m.group(1) or 0)*3600+int(m.group(2))*60+int(m.group(3)) if m else -1)" "$1"; }

# ── Run helpers ──────────────────────────────────────────────────────────────
start_feeder() {
  stop_feeder; rm -f "$OUT/feed.stop"
  python3 "$HELP/feeder.py" "$ADB" "$SERIAL" "$OUT/logs/feed-$1.log" "$OUT/feed.stop" &
  FEEDER_PID=$!; sleep 3
}
stop_feeder() {
  [[ -n "$FEEDER_PID" ]] || return 0
  touch "$OUT/feed.stop"; wait "$FEEDER_PID" 2>/dev/null || true; FEEDER_PID=""
}
open_tracker() {
  tap_node eq Log || return 1; sleep 2
  tap_node eq "Track run (GPS)" || return 1
  wait_text eq Start 20
}
finish_run() {
  launch; sleep 2
  has_text eq Stop || { tap_node pre "Recording" 2 || true; sleep 2; }
  tap_node eq Stop || return 1
  wait_text eq "Save run" 15 || return 1
  tap_node eq "Save run"; sleep 4
  ! has_text eq "Save run"
}

# ── Tile helpers ─────────────────────────────────────────────────────────────
TILE="$APP_ID/.QuickTileService"
# Open Quick Settings and dump it as ui/NAME.xml; echo the tile's label. The QS
# tile node carries the label as its text (content-desc adds the state). The
# panel binds the tile when it opens (onStartListening), so the label is read
# again 2 s after it first shows to get the settled state.
tile_label() {
  local i l re='^(Log|Log set|Start run|Run · recording)$'
  adb_ shell cmd statusbar expand-settings; sleep 3
  for i in 1 2 3; do
    wdump "$1" || continue
    l="$(wfind "$1" re "$re" | cut -d' ' -f7-)"
    if [[ -n "$l" ]]; then
      sleep 2
      wdump "$1" && l="$(wfind "$1" re "$re" | cut -d' ' -f7-)"
      [[ -n "$l" ]] && { echo "$l"; return 0; }
    fi
    # The added tile lands on the last QS page.
    adb_ shell input swipe 900 700 150 700 300; sleep 2
  done
  return 1
}
# add_tile: `cmd statusbar add-tile`, then wait (max ~40 s) until the tile shows
# on the QS panel (it takes a while to bind on API 34).
add_tile() {
  local i
  sh_ cmd statusbar add-tile "$TILE" >/dev/null
  for i in 1 2 3 4 5 6; do
    sleep 3
    tile_label tile-added >/dev/null && { adb_ shell cmd statusbar collapse; return 0; }
    adb_ shell cmd statusbar collapse
  done
  log "tile $TILE not on the QS panel after add-tile"
  return 1
}
tap_tile() { # tap_tile DUMP LABEL (QS already open from tile_label)
  local r p
  r="$(wfind "$1" eq "$2")" || return 1
  read -r -a p <<<"$r"; tap_xy "${p[0]}" "${p[1]}"; sleep 3
}
# tile_case NAME EXPECTED-LABEL EXPECTED-SCREEN: label check, then a real tap.
tile_case() {
  local S="${TILE_S:-tile}" l s
  adb_ shell cmd statusbar collapse; home
  l="$(tile_label "tile-$1")"
  shot "tile-$1-qs"
  # With the panel open: the tile service's binding (activity manager) and
  # System UI's record of the tile (its cached state and listening flag).
  sh_ dumpsys activity services "$TILE" >"$OUT/logs/tile-$1-service.txt"
  sh_ dumpsys activity service com.android.systemui | grep -i -B2 -A8 "$APP_ID" >"$OUT/logs/tile-$1-sysui.txt"
  expect $S "$1: tile label" "label '$l' (want '$2'); ui/tile-$1.xml shots/tile-$1-qs.png" test "$l" = "$2"
  if [[ -n "$l" ]] && tap_tile "tile-$1" "$l"; then
    wait_screen "$3" 10; s="$(screen)"
    evidence "tile-$1-tap"
    expect $S "$1: tile tap opens $3" "screen '$s'; shots/tile-$1-tap.png" test "$s" = "$3"
  else
    check FAIL $S "$1: tile tap opens $3" "tile not found on the QS panel"
  fi
  adb_ shell cmd statusbar collapse
}

# ═════════════════════════════════════════════════════════════════════════════
sc_links() {
  local S=links d want s ex
  fresh_app "$LIFT_DAY" seed_basic || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  ex="$(snap_field items.0.name)"
  for d in today log track log_set bogus; do
    case "$d" in log) want=log ;; track) want=track ;; log_set) want=log_set ;; *) want=today ;; esac
    mark "links-cold-$d"
    adb_ shell am force-stop "$APP_ID"; sleep 1
    adb_ shell am start -a app.milestone.action.OPEN -n "$APP_ID/.MainActivity" --es dest "$d" >/dev/null
    wait_screen "$want" 25; s="$(screen)"; evidence "links-cold-$d"
    expect $S "cold start dest=$d opens $want" "screen '$s'; shots/links-cold-$d.png" test "$s" = "$want"
    if [[ "$d" == log_set ]]; then
      expect $S "log_set prefills today's open lift item" "want 'Change exercise: $ex'; ui/links-cold-log_set.xml" \
        eval '[[ -n "$ex" ]] && python3 "$HELP/node.py" eq "Change exercise: $ex" <"$OUT/ui/links-cold-log_set.xml" >/dev/null'
    fi
    mark "links-warm-$d"
    adb_ shell am force-stop "$APP_ID"; launch; wait_screen today 25
    adb_ shell am start -a app.milestone.action.OPEN -n "$APP_ID/.MainActivity" --es dest "$d" >/dev/null
    wait_screen "$want" 15; s="$(screen)"; evidence "links-warm-$d"
    expect $S "warm (onNewIntent) dest=$d opens $want" "screen '$s'; shots/links-warm-$d.png" test "$s" = "$want"
  done
  # A rotation must not navigate again: open log, close the sheet, rotate.
  mark links-rotate
  adb_ shell am force-stop "$APP_ID"
  adb_ shell am start -a app.milestone.action.OPEN -n "$APP_ID/.MainActivity" --es dest log >/dev/null
  wait_screen log 25; adb_ shell input keyevent 4; sleep 2
  adb_ shell settings put system accelerometer_rotation 0
  adb_ shell settings put system user_rotation 1; sleep 4
  s="$(screen)"; evidence links-rotated
  adb_ shell settings put system user_rotation 0; sleep 3
  expect $S "rotation after an open intent does not reopen the sheet" "screen '$s'; shots/links-rotated.png" test "$s" = today
}

sc_tile() {
  local S=tile
  fresh_app "$LIFT_DAY" seed_basic || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  add_tile || check FAIL $S "tile added to Quick Settings" "not on the QS panel ~40 s after add-tile; ui/tile-added.xml"
  home
  tile_case lift-day "Log set" log_set
  reseed "$RUN_DAY" seed_basic; home
  tile_case run-day "Start run" track
  reseed "$REST_DAY" seed_basic; home
  tile_case rest-day "Log" log
  reseed "$RUN_DAY" seed_hold "$RUN_DAY"; home
  expect $S "hold seed pauses training" "snapshot kind '$(snap_field kind)'" test "$(snap_field kind)" = Paused
  tile_case hold "Log" log
  # The stored day is Mon; the clock moves to Wed with the app in the background.
  reseed "$RUN_DAY" seed_basic; home
  pin_clock "$LIFT_DAY"; sleep 4
  tile_case stale-day "Log" log
}

# The stale-day and recording tile cases with no force-stop or `pm clear` once
# the tile is added (sc_tile reseeds with a force-stop between cases): the seed
# is pushed onto the fresh install before its first launch. Meant for a freshly
# booted emulator: `--only tilefresh` with the emulator not running.
sc_tilefresh() {
  local S=tilefresh TILE_S=tilefresh pid
  check INFO $S "fresh boot" "device uptime $(sh_ cut -d. -f1 /proc/uptime) s at the start; APK installed by this run"
  baseline_device; grant_perms
  pin_clock "$RUN_DAY"
  seed_basic >"$OUT/logs/seed-tilefresh.ndjson"
  push_log "$OUT/logs/seed-tilefresh.ndjson" >/dev/null 2>&1 || { check FAIL $S "seed pushed" "push_log failed"; return; }
  launch
  wait_text eq Today 40 || { evidence tilefresh-boot; check FAIL $S "seeded boot" "Today never rendered"; return; }
  sleep 3; home
  add_tile || check FAIL $S "tile added to Quick Settings" "not on the QS panel ~40 s after add-tile; ui/tile-added.xml"
  tile_case run-day "Start run" track
  # The stored day is Mon; the clock moves to Wed with the app in the background.
  home
  pid="$(app_pid)"
  pin_clock "$LIFT_DAY"; sleep 4
  expect $S "stale-day: same app process, never stopped" "pid '$pid' → '$(app_pid)'" eval '[[ -n "$pid" && "$(app_pid)" == "$pid" ]]'
  tile_case stale-day "Log" log
  # Recording, started from the Log sheet the tile just opened.
  if tap_node eq "Track run (GPS)" && wait_text eq Start 20; then
    start_feeder tilefresh
    tap_node eq Start
    if wait_text eq Stop 30; then
      tile_case recording "Run · recording" track
      finish_run || check FAIL $S "finish and save the run" "Stop → Save run failed"
    else
      evidence tilefresh-no-stop; check FAIL $S "run recording" "no Stop after Start; shots/tilefresh-no-stop.png"
    fi
    stop_feeder
  else
    evidence tilefresh-no-tracker; check FAIL $S "open the tracker from the Log sheet" "shots/tilefresh-no-tracker.png"
  fi
}

# ── Widget ───────────────────────────────────────────────────────────────────
LAUNCHER="com.google.android.apps.nexuslauncher"
# drag X1 Y1 X2 Y2: a long-press drag as raw motion events (the launcher's
# widget-picker preview starts a drag only after a held touch; `input
# draganddrop` does not hold long enough on API 34).
drag() {
  local i
  adb_ shell input motionevent DOWN "$1" "$2"; sleep 2
  for i in 1 2 3 4 5 6 7 8; do
    adb_ shell input motionevent MOVE $(( $1 + ($3 - $1) * i / 8 )) $(( $2 + ($4 - $2) * i / 8 ))
  done
  sleep 1
  adb_ shell input motionevent UP "$3" "$4"
}
# Place the Today widget through the launcher's picker on a reset home screen
# (launcher data cleared, so a long-press lands on empty grid): long-press home →
# Widgets → search "lollop" → Today → tap the preview → "Add Today widget"
# (API 35+), else (API 34: no Add button) drag the preview image
# (`widget_preview_container`) onto the home grid.
place_widget() {
  local r p preview='^[0-9]+ wide by [0-9]+ high$'
  adb_ shell pm clear "$LAUNCHER" >/dev/null 2>&1; sleep 3
  home; home; sleep 3
  evidence widget-home-clean
  adb_ shell input swipe 540 1300 540 1300 1500; sleep 2
  tap_node eq Widgets || return 1; sleep 3
  if tap_node eq Search 2; then sleep 1; adb_ shell input text lollop; sleep 3; fi
  scroll_to eq lollop 6 || return 1
  has_text re "$preview" || { tap_node eq lollop; sleep 2; }
  has_text re "$preview" || { tap_node eq Today 2; sleep 2; }
  evidence widget-picker
  has_text eq "Add Today widget" || { tap_node re "$preview" 2; sleep 2; }
  evidence widget-picker-preview
  if has_text eq "Add Today widget"; then
    tap_node eq "Add Today widget"; sleep 4
  else
    r="$(find_node rid widget_preview_container)" || r="$(find_node rid widget_preview)" || return 1
    read -r -a p <<<"$r"
    drag "${p[0]}" "${p[1]}" 540 800; sleep 4
  fi
  # The launcher leaves the new widget in resize mode (no idle UI state for
  # uiautomator until it is dismissed).
  home
  evidence widget-placed
  sh_ dumpsys appwidget | grep -q "cmp:ComponentInfo{$APP_ID/$APP_ID.widget.TodayWidgetReceiver}" &&
    wait_text eq "Open Today in lollop" 15
}
# widget_texts NAME: the widget's labels (home screen dump) one per line.
widget_texts() {
  home; sleep 2
  evidence "$1"
  python3 "$HELP/node.py" texts <"$OUT/ui/$1.xml" | sed -n '/content-desc=Open Today in lollop/,$p' | head -12
}
# widget_tap NAME LABEL WANT-SCREEN
widget_tap() {
  local S=widget s
  home; sleep 1
  if tap_node eq "$2" 3; then
    wait_screen "$3" 15; s="$(screen)"
  else
    s="(no '$2' on the widget)"
  fi
  evidence "widget-tap-$1"
  expect $S "tap '$2' opens $3" "screen '$s'; shots/widget-tap-$1.png" test "$s" = "$3"
  adb_ shell am force-stop "$APP_ID"; launch; wait_screen today 20; home
}
sc_widget() {
  local S=widget t
  fresh_app "$RUN_DAY" seed_basic || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  home
  if ! place_widget; then
    evidence widget-place-failed
    check FAIL $S "widget placed on the home screen" "picker flow failed; shots/widget-picker.png shots/widget-place-failed.png"
    return
  fi
  check PASS $S "widget placed on the home screen" "dumpsys appwidget lists TodayWidgetReceiver; shots/widget-picker.png"
  t="$(widget_texts widget-run-day)"
  expect $S "run day: title, run line, Track run + Log" "$t; shots/widget-run-day.png" \
    eval 'grep -qx "text=Easy run" <<<"$t" && grep -q "^text=Run · " <<<"$t" && grep -qx "text=Track run" <<<"$t" && grep -qx "text=Log" <<<"$t"'
  widget_tap run-track "Track run" track
  widget_tap run-log "Log" log
  widget_tap run-body "Open Today in lollop" today

  reseed "$LIFT_DAY" seed_basic; home
  t="$(widget_texts widget-lift-day)"
  expect $S "lift day: lift title and items, Log only" "snapshot title '$(snap_field title)'; $t; shots/widget-lift-day.png" \
    eval 'grep -qx "text=$(snap_field title)" <<<"$t" && grep -qx "text=Log" <<<"$t" && ! grep -qx "text=Track run" <<<"$t"'

  reseed "$RUN_DAY" seed_hold "$RUN_DAY"; home
  t="$(widget_texts widget-hold)"
  expect $S "hold: Training paused + reason, Log" "$t; shots/widget-hold.png" \
    eval 'grep -qx "text=Training paused" <<<"$t" && grep -qx "text=Log" <<<"$t"'

  # Date change with the app in the background (not force-stopped: a stopped app
  # gets no TIME_SET broadcast).
  reseed "$RUN_DAY" seed_basic; home
  mark widget-date-change
  pin_clock "$REST_DAY"; sleep 6
  t="$(widget_texts widget-stale)"
  expect $S "date change: 'Open lollop to update' + Log" "snapshot stale=$(snap_field stale); $t; shots/widget-stale.png" \
    eval 'grep -qx "text=Open lollop to update" <<<"$t" && grep -qx "text=Log" <<<"$t"'
  launch; wait_screen today 20; home
  t="$(widget_texts widget-after-open)"
  expect $S "opening the app refreshes it to the new day" "$t; shots/widget-after-open.png" \
    eval '! grep -qx "text=Open lollop to update" <<<"$t" && grep -q "Tue, Sep 22" <<<"$t"'
}

# ── Run notification ─────────────────────────────────────────────────────────
RUN_ID=1
sc_run() {
  local S=run c1 c2 t1 t2 n
  fresh_app "$RUN_DAY" seed_basic || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  # No feed yet: the emulator GNSS repeats its default fix, so the locate preview
  # gets a lock and a "Start on move" run stays armed until the feed moves.
  open_tracker || { check FAIL $S "open tracker" "Log → Track run (GPS) failed"; return; }
  sleep 4
  notif run-locate $RUN_ID >/dev/null
  expect $S "locate preview: title 'Finding GPS…', chip text 'GPS…'" \
    "title '$(nfield run-locate "r['extras'].get('android.title')")', shortCriticalText '$(nfield run-locate "r['extras'].get('android.shortCriticalText')")'; logs/run-locate-notif.txt" \
    eval '[[ "$(nfield run-locate "r[\"extras\"].get(\"android.title\")")" == "Finding GPS…" && "$(nfield run-locate "r[\"extras\"].get(\"android.shortCriticalText\")")" == "GPS…" ]]'
  live_promoted run-locate "locate preview"
  mark run-armed
  scroll_to eq "Start on move" 4 && tap_node eq "Start on move"; sleep 1
  tap_node eq Start || { check FAIL $S "start run" "no Start"; return; }
  sleep 4; evidence run-armed-app
  home
  c1="$(chip run-chip-armed)"; sb_shot run-chip-armed; notif run-armed $RUN_ID >/dev/null
  expect $S "armed (Start on move, standing still): title 'Waiting to start', chip text 'Waiting'" \
    "title '$(nfield run-armed "r['extras'].get('android.title')")', shortCriticalText '$(nfield run-armed "r['extras'].get('android.shortCriticalText')")'" \
    eval '[[ "$(nfield run-armed "r[\"extras\"].get(\"android.title\")")" == "Waiting to start" && "$(nfield run-armed "r[\"extras\"].get(\"android.shortCriticalText\")")" == Waiting ]]'
  [[ $API -ge 36 ]] && expect $S "armed: status-bar chip reads 'Waiting'" "SystemUI chip '$c1'; shots/run-chip-armed-statusbar.png" test "$c1" = Waiting
  mark run-recording
  start_feeder run
  for n in $(seq 1 30); do
    [[ "$(notif run-wait $RUN_ID | python3 -c "import json,sys;r=json.load(sys.stdin);print(r['extras'].get('android.title') if r else '')")" == "Tracking run" ]] && break
    sleep 1
  done
  expect $S "moving feed starts the armed run" "title 'Tracking run' after ~${n}s of feed" test "$n" -lt 30
  sleep 8
  c1="$(chip run-chip-1)"; sb_shot run-chip-1; notif run-rec-1 $RUN_ID >/dev/null
  sleep 10
  c2="$(chip run-chip-2)"; sb_shot run-chip-2; notif run-rec-2 $RUN_ID >/dev/null
  t1="$(nfield run-rec-1 "r['extras'].get('android.shortCriticalText')")"
  t2="$(nfield run-rec-2 "r['extras'].get('android.shortCriticalText')")"
  expect $S "recording: notification chip text is the moving time and advances" \
    "shortCriticalText '$t1' → '$t2' (10 s apart), title '$(nfield run-rec-2 "r['extras'].get('android.title')")', text '$(nfield run-rec-2 "r['extras'].get('android.text')")'" \
    eval '[[ $(mmss "$t1") -ge 0 && $(mmss "$t2") -gt $(mmss "$t1") ]]'
  live_promoted run-rec-2 "recording"
  if [[ $API -ge 36 ]]; then
    expect $S "recording: status-bar chip (shade closed) shows the moving time, advancing" \
      "SystemUI chip '$c1' → '$c2'; shots/run-chip-1-statusbar.png shots/run-chip-2-statusbar.png ui/run-chip-*.xml" \
      eval '[[ $(mmss "$c1") -ge 0 && $(mmss "$c2") -gt $(mmss "$c1") ]]'
  else
    expect $S "API $API: no status-bar chip, plain ongoing notification" "chip '$c2'; flags $(nfield run-rec-2 "'|'.join(r['flags'])")" \
      eval '[[ -z "$c2" ]] && [[ "$(nfield run-rec-2 "\"ONGOING_EVENT\" in r[\"flags\"]")" == True ]]'
  fi
  # Tile while recording.
  if sh_ dumpsys activity services "$TILE" >/dev/null 2>&1; then
    add_tile
    tile_case recording "Run · recording" track
    home
  fi
  mark run-pause
  if tap_shade run-shade-pause eq Pause "Tracking run"; then
    adb_ shell cmd statusbar collapse; sleep 3
    c1="$(chip run-chip-paused)"; sb_shot run-chip-paused; notif run-paused $RUN_ID >/dev/null
    expect $S "paused (notification action): title 'Run paused', chip text 'Paused'" \
      "title '$(nfield run-paused "r['extras'].get('android.title')")', shortCriticalText '$(nfield run-paused "r['extras'].get('android.shortCriticalText')")'" \
      eval '[[ "$(nfield run-paused "r[\"extras\"].get(\"android.shortCriticalText\")")" == Paused && "$(nfield run-paused "r[\"extras\"].get(\"android.title\")")" == "Run paused" ]]'
    [[ $API -ge 36 ]] && expect $S "paused: status-bar chip reads 'Paused'" "SystemUI chip '$c1'; shots/run-chip-paused-statusbar.png" test "$c1" = Paused
    tap_shade run-shade-resume eq Resume "Run paused"; adb_ shell cmd statusbar collapse; sleep 3
  else
    check FAIL $S "pause from the notification" "no Pause action in the shade; ui/run-shade-pause.xml"
  fi
  # The emulator GNSS repeats the last fix every ~2 s after the feed stops, so a
  # lost signal is location turned off (the service ignores provider-disabled).
  mark run-signal-lost
  sh_ cmd location set-location-enabled false; sleep 22
  c1="$(chip run-chip-nogps)"; sb_shot run-chip-nogps; notif run-nogps $RUN_ID >/dev/null
  expect $S "GPS signal lost (location off 22 s): chip text 'No GPS'" \
    "shortCriticalText '$(nfield run-nogps "r['extras'].get('android.shortCriticalText')")', text '$(nfield run-nogps "r['extras'].get('android.text')")'" \
    test "$(nfield run-nogps "r['extras'].get('android.shortCriticalText')")" = "No GPS"
  [[ $API -ge 36 ]] && expect $S "signal lost: status-bar chip reads 'No GPS'" "SystemUI chip '$c1'; shots/run-chip-nogps-statusbar.png" test "$c1" = "No GPS"
  sh_ cmd location set-location-enabled true; sleep 8
  notif run-regained $RUN_ID >/dev/null
  t1="$(nfield run-regained "r['extras'].get('android.shortCriticalText')")"
  expect $S "signal back: chip returns to the moving time" "shortCriticalText '$t1'" eval '[[ $(mmss "$t1") -gt 0 ]]'
  mark run-tap
  home
  if tap_shade run-shade-tap eq "Tracking run"; then
    wait_screen track 10; n="$(screen)"; evidence run-tap
    expect $S "tapping the notification opens the tracker" "screen '$n'; shots/run-tap.png" test "$n" = track
  else
    check FAIL $S "tapping the notification opens the tracker" "no 'Tracking run' in the shade; ui/run-shade-tap.xml"
  fi
  expect $S "finish and save the run" "Stop → Save run" finish_run
  stop_feeder
  notif run-after $RUN_ID >/dev/null
  expect $S "notification gone after the run is saved" "$(cat "$OUT/logs/run-after-notif.json")" test "$(cat "$OUT/logs/run-after-notif.json")" = null
}
# live_promoted DUMP WHAT: the promoted flag on API 36+, its absence below.
live_promoted() {
  local p; p="$(promoted "$1")"
  if [[ $API -ge 36 ]]; then
    expect run "$2: FLAG_PROMOTED_ONGOING set (Live Update)" "flags $(nfield "$1" "'|'.join(r['flags'])"); requestPromotedOngoing $(nfield "$1" "r['extras'].get('android.requestPromotedOngoing')")" test "$p" = True
  else
    expect run "$2: posts as a plain ongoing notification (API $API)" "flags $(nfield "$1" "'|'.join(r['flags'])")" test "$p" = False
  fi
}

sc_guided() {
  local S=guided d found="" n
  fresh_app "$RUN_DAY" seed_c25k || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  for d in 2026-09-21 2026-09-22 2026-09-23 2026-09-24 2026-09-25 2026-09-26 2026-09-27; do
    [[ "$d" == "$RUN_DAY" ]] || goto_day "$d"
    [[ "$(snap_field kind)" == Run && "$(snap_field trackable)" == True ]] && { found=$d; break; }
  done
  [[ -n "$found" ]] || { check FAIL $S "a C25k run day this week" "no run day found"; return; }
  check INFO $S "C25k run day" "$found: $(snap_field title)"
  start_feeder guided
  open_tracker || { check FAIL $S "open tracker" "failed"; return; }
  sleep 4; evidence guided-tracker
  tap_node eq Start || { check FAIL $S "start run" "no Start"; return; }
  wait_text eq Stop 20; sleep 12
  home
  notif guided $RUN_ID >/dev/null
  adb_ shell cmd statusbar expand-notifications; sleep 2; shot guided-shade; adb_ shell cmd statusbar collapse
  if [[ $API -ge 36 ]]; then
    # 100 units per step; past 15 steps the bar is one segment (platform limit).
    expect $S "guided run: ProgressStyle step bar, 100 units per step (one segment past 15 steps)" \
      "template $(nfield guided "r['extras'].get('android.template')"); progress $(nfield guided "r['extras'].get('android.progress')")/$(nfield guided "r['extras'].get('android.progressMax')") = $(( $(nfield guided "int(r['extras'].get('android.progressMax') or 0)") / 100 )) steps; segments $(nfield guided "r['extras'].get('android.progressSegments')"); text '$(nfield guided "r['extras'].get('android.text')")'; shots/guided-shade.png" \
      eval '[[ "$(nfield guided "r[\"extras\"].get(\"android.template\")")" == *ProgressStyle* && $(nfield guided "int(r[\"extras\"].get(\"android.progressMax\") or 0) % 100") -eq 0 && $(nfield guided "int(r[\"extras\"].get(\"android.progressMax\") or 0)") -gt 0 ]]'
  else
    expect $S "guided run (API $API): plain progress bar fallback" \
      "progress $(nfield guided "r['extras'].get('android.progress')")/$(nfield guided "r['extras'].get('android.progressMax')"); text '$(nfield guided "r['extras'].get('android.text')")'; shots/guided-shade.png" \
      eval '[[ "$(nfield guided "r[\"extras\"].get(\"android.progressMax\")")" -gt 0 ]]'
  fi
  expect $S "guided run: content line leads with the step" "text '$(nfield guided "r['extras'].get('android.text')")'" \
    eval '[[ "$(nfield guided "r[\"extras\"].get(\"android.text\")")" != Moving* ]]'
  finish_run || check FAIL $S "finish run" "Save run flow failed"
  stop_feeder
}

# ── Rest timer ───────────────────────────────────────────────────────────────
REST_ID=4207
sc_rest() {
  local S=rest c1 c2 r p s n left
  fresh_app "$LIFT_DAY" seed_basic || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  # Exact alarms allowed, so the rest-done alert is due at the rest end (sc_restbg
  # covers the inexact case).
  sh_ appops set "$APP_ID" SCHEDULE_EXACT_ALARM allow
  # The set editor at today's plan item (the chooser's plain "Log set" row opens
  # it blank, with no prescribed rest).
  adb_ shell am start -a app.milestone.action.OPEN -n "$APP_ID/.MainActivity" --es dest log_set >/dev/null
  wait_screen log_set 15 || { evidence rest-no-editor; check FAIL $S "open set editor" "dest=log_set did not open it"; return; }
  r="$(find_node re '^Log set [0-9]+( of [0-9]+)?$')" || { evidence rest-no-primary; check FAIL $S "log a set" "no 'Log set N' button"; return; }
  tap_top re '^Log set [0-9]+( of [0-9]+)?$'; sleep 3
  evidence rest-loop
  notif rest-inapp $REST_ID >/dev/null
  expect $S "rest readout on screen: no countdown notification" "$(cat "$OUT/logs/rest-inapp-notif.json")" test "$(cat "$OUT/logs/rest-inapp-notif.json")" = null
  tap_node eq Close; sleep 2
  has_text eq Discard && { tap_node eq Discard; sleep 1; }
  home; sleep 2
  notif rest-1 $REST_ID >/dev/null
  c1="$(chip rest-chip-1)"; sb_shot rest-chip-1
  sleep 6
  c2="$(chip rest-chip-2)"; sb_shot rest-chip-2
  expect $S "countdown posted: 'Resting · <exercise>', chronometer counting down to the rest end" \
    "channel $(nfield rest-1 "r['channel']"); title '$(nfield rest-1 "r['extras'].get('android.title')")'; showChronometer $(nfield rest-1 "r['extras'].get('android.showChronometer')"); countDown $(nfield rest-1 "r['extras'].get('android.chronometerCountDown')"); when-now $(( $(nfield rest-1 "r['when']") / 1000 - $(sh_ date +%s) )) s" \
    eval '[[ "$(nfield rest-1 "r[\"extras\"].get(\"android.title\")")" == "Resting · "* && "$(nfield rest-1 "r[\"extras\"].get(\"android.showChronometer\")")" == true && "$(nfield rest-1 "r[\"extras\"].get(\"android.chronometerCountDown\")")" == true && $(nfield rest-1 "r[\"when\"]") -gt $(sh_ date +%s)000 ]]'
  expect $S "card names the next set: 'Next: Set N of M · …'" "text '$(nfield rest-1 "r['extras'].get('android.text')")'" \
    eval '[[ "$(nfield rest-1 "r[\"extras\"].get(\"android.text\")")" == "Next: Set "* ]]'
  if [[ $API -ge 36 ]]; then
    expect $S "countdown: FLAG_PROMOTED_ONGOING set" "flags $(nfield rest-1 "'|'.join(r['flags'])")" test "$(promoted rest-1)" = True
    expect $S "status-bar chip (shade closed) counts down" "SystemUI chip '$c1' → '$c2' (6 s apart); shots/rest-chip-1-statusbar.png shots/rest-chip-2-statusbar.png" \
      eval '[[ $(mmss "$c1") -gt 0 && $(mmss "$c2") -lt $(mmss "$c1") ]]'
  else
    expect $S "countdown (API $API): plain ongoing notification" "flags $(nfield rest-1 "'|'.join(r['flags'])"); chip '$c1'" \
      eval '[[ "$(promoted rest-1)" == False && -z "$c1" ]]'
  fi
  mark rest-tap
  # Tap the content line: on API 34 a tap on the collapsed card's title line
  # (title · chronometer) opens nothing.
  if tap_shade rest-shade "pre" "Next: Set"; then
    wait_screen log_set 10; s="$(screen)"; evidence rest-tap
    expect $S "tapping the countdown opens the set editor" "screen '$s'; shots/rest-tap.png" test "$s" = log_set
  else
    check FAIL $S "tapping the countdown opens the set editor" "no countdown in the shade; ui/rest-shade.xml"
  fi
  # The editor is open again (readout on screen); leave with X and wait out the rest.
  tap_node eq Close; sleep 2; has_text eq Discard && { tap_node eq Discard; sleep 1; }
  home
  left=$(( $(nfield rest-1 "r['when']") / 1000 - $(sh_ date +%s) ))
  # The shade steps can outlast the rest (slow dumps); then check right away.
  if [[ $left -lt 400 ]]; then
    sleep $(( (left > 0 ? left : 0) + 4 ))
    notif rest-done $REST_ID >/dev/null; c1="$(chip rest-chip-done)"; sb_shot rest-done
    expect $S "rest end: 'Rest done' alert replaces the countdown, not promoted, chip gone" \
      "channel $(nfield rest-done "r['channel']"); title '$(nfield rest-done "r['extras'].get('android.title')")'; flags $(nfield rest-done "'|'.join(r['flags'])"); chip '$c1'" \
      eval '[[ "$(nfield rest-done "r[\"channel\"]")" == rest_done && "$(promoted rest-done)" == False && -z "$c1" ]]'
  else
    check INFO $S "rest end" "rest of ${left}s not waited out"
  fi
}

# ── Rest-done alert with the app in the background ───────────────────────────
# The app process: "frozen" (cgroup v2 freezer), "cached" (a cch entry in the
# LRU list), "gone" or "alive".
proc_state() {
  local p; p="$(app_pid)"
  [[ -n "$p" ]] || { echo gone; return; }
  [[ "$(sh_ cat "/sys/fs/cgroup/uid_*/pid_$p/cgroup.events" | sed -n 's/^frozen //p')" == 1 ]] && { echo frozen; return; }
  sh_ dumpsys activity processes | grep -E "Proc #.* $p:$APP_ID" | grep -q ' cch' && { echo cached; return; }
  echo alive
}
# rest_bg CASE MODE [kill]: set the SCHEDULE_EXACT_ALARM app-op to MODE (allow |
# deny), log a set in the loop (a rest starts), go home with the loop open and
# wait until the app is cached or frozen; with `kill`, `am kill` the cached
# process instead (the alarm then starts a new one). Then wait for "Rest done":
# it must arrive, within 30 s of the rest end with exact alarms, at any time up
# to 15 min after it without. Arrival = the alert's post time ("when") against
# the countdown's rest end ("when").
rest_bg() {
  local S=restbg c="$1" mode="$2" kill="${3:-}" end pid0 pid1 st="" i now limit delay want
  mark "restbg-$c"
  adb_ shell am force-stop "$APP_ID"
  sh_ appops set "$APP_ID" SCHEDULE_EXACT_ALARM "$mode"
  launch; wait_text eq Today 40
  adb_ shell am start -a app.milestone.action.OPEN -n "$APP_ID/.MainActivity" --es dest log_set >/dev/null
  wait_screen log_set 15 || { evidence "restbg-$c-no-editor"; check FAIL $S "$c: open set editor" "dest=log_set did not open it"; return; }
  tap_top re '^Log set [0-9]+( of [0-9]+)?$' || { evidence "restbg-$c-no-primary"; check FAIL $S "$c: log a set" "no 'Log set N' button"; return; }
  sleep 3
  evidence "restbg-$c-loop"
  sh_ dumpsys alarm >"$OUT/logs/restbg-$c-alarm.txt"
  check INFO $S "$c: app-op and alarm" "SCHEDULE_EXACT_ALARM '$(sh_ appops get "$APP_ID" SCHEDULE_EXACT_ALARM | head -1)'; $(grep -A4 'app.milestone.action.REST_END\|RestEndReceiver' "$OUT/logs/restbg-$c-alarm.txt" | grep -m2 -oE 'type=[^ ]+|window=[^ ]+|exactAllowReason=[^ ]+|flags=[^ ]+' | tr '\n' ' '); logs/restbg-$c-alarm.txt"
  if [[ "$mode" == deny && $API -ge 31 ]]; then
    expect $S "$c: exact alarms off: the loop shows the notice" "ui/restbg-$c-loop.xml shots/restbg-$c-loop.png" has_text pre "Exact alarms are off"
  else
    expect $S "$c: exact alarms allowed: no notice" "ui/restbg-$c-loop.xml" eval '! has_text pre "Exact alarms are off"'
  fi
  home
  notif "restbg-$c-count" $REST_ID >/dev/null
  end="$(nfield "restbg-$c-count" "r['when']")"
  [[ -n "$end" ]] || { check FAIL $S "$c: countdown posted" "$(cat "$OUT/logs/restbg-$c-count-notif.json")"; return; }
  # Another app in between, so lollop is not kept as the "previous" app.
  adb_ shell am start -a android.settings.SETTINGS >/dev/null 2>&1; sleep 3; home
  pid0="$(app_pid)"
  for i in $(seq 1 24); do
    st="$(proc_state)"; [[ "$st" == cached || "$st" == frozen || "$st" == gone ]] && break
    sleep 5
  done
  if [[ -n "$kill" ]]; then
    adb_ shell am kill "$APP_ID"; sleep 3
    # The kill is asynchronous, and a widget or tile broadcast may start a new
    # process right after it: the check is that the cached pid died.
    expect $S "$c: cached process killed (am kill)" "pid $pid0 → '$(app_pid)' (state '$st' before); $(sh_ logcat -d -b events | grep "am_kill.*$pid0," | tail -1)" \
      eval '[[ -n "$pid0" && "$(app_pid)" != "$pid0" ]]'
  else
    expect $S "$c: app cached or frozen in the background before the rest end" \
      "state '$st' ${i}x5 s after home; rest end in $(( end / 1000 - $(sh_ date +%s) )) s" eval '[[ "$st" == cached || "$st" == frozen ]] && [[ $(( end / 1000 )) -gt $(sh_ date +%s) ]]'
  fi
  # The state 10 s before the rest end (the freezer acts a little after the app
  # is cached), when the alarm is still pending.
  now="$(sh_ date +%s)"; [[ $(( end / 1000 - 10 - now )) -gt 0 ]] && sleep $(( end / 1000 - 10 - now ))
  st="$(proc_state)"
  sh_ dumpsys activity processes | grep -E "Proc #.*:$APP_ID" >"$OUT/logs/restbg-$c-proc.txt"
  check INFO $S "$c: app 10 s before the rest end" "state '$st' (pid '$(app_pid)'); logs/restbg-$c-proc.txt"
  if [[ "$mode" == allow ]]; then want=30; else want=900; fi
  limit=$(( end / 1000 + want + 5 ))
  while :; do
    notif "restbg-$c-done" $REST_ID >/dev/null
    [[ "$(nfield "restbg-$c-done" "r['extras'].get('android.title')")" == "Rest done" ]] && break
    now="$(sh_ date +%s)"; [[ $now -gt $limit ]] && break
    sleep 5
  done
  sb_shot "restbg-$c-done"
  pid1="$(app_pid)"
  if [[ "$(nfield "restbg-$c-done" "r['extras'].get('android.title')")" == "Rest done" ]]; then
    delay=$(( ( $(nfield "restbg-$c-done" "r['when']") - end ) / 1000 ))
    expect $S "$c: 'Rest done' arrives in the background (app-op $mode), ${delay} s after the rest end (limit ${want} s)" \
      "text '$(nfield "restbg-$c-done" "r['extras'].get('android.text')")'; channel $(nfield "restbg-$c-done" "r['channel']"); pid $pid0 → ${pid1:-none}; logs/restbg-$c-done-notif.txt shots/restbg-$c-done.png" \
      eval '[[ "$(nfield "restbg-$c-done" "r[\"channel\"]")" == rest_done && $delay -ge -1 && $delay -le $want ]]'
  else
    check FAIL $S "$c: 'Rest done' arrives in the background (app-op $mode)" "nothing ${want} s after the rest end; $(cat "$OUT/logs/restbg-$c-done-notif.json")"
  fi
  sh_ logcat -d | grep -E "AlarmManager.*$APP_ID|$APP_ID.*(frozen|freez)" | tail -5 >"$OUT/logs/restbg-$c-alarm-logcat.txt"
}
sc_restbg() {
  fresh_app "$LIFT_DAY" seed_basic || { check FAIL restbg "seeded boot" "Today never rendered"; return; }
  rest_bg exact allow
  rest_bg inexact deny
  rest_bg killed allow kill
  sh_ appops set "$APP_ID" SCHEDULE_EXACT_ALARM default
}

# ── Live Updates toggle ──────────────────────────────────────────────────────
promo_settings() { adb_ shell am start -a android.settings.APP_NOTIFICATION_PROMOTION_SETTINGS --es android.provider.extra.APP_PACKAGE "$APP_ID" >/dev/null 2>&1; sleep 3; }
# The switch on the "Live updates" row (the intent opens the app's notification
# settings, whose first switch is "All lollop notifications"); echo "cx cy checked".
promo_switch() {
  wdump "$1" || return 1
  python3 - "$OUT/ui/$1.xml" <<'PY'
import re, sys, xml.etree.ElementTree as ET
raw = open(sys.argv[1], errors="replace").read()
root = ET.fromstring(raw[raw.find("<?xml"):raw.rfind("</displays>") + 11])
B = lambda n: list(map(int, re.findall(r"-?\d+", n.get("bounds"))))
nodes = [n for n in root.iter("node") if n.get("package") == "com.android.settings"]
label = next((n for n in nodes if n.get("text") == "Live updates"), None)
if label is None:
    sys.exit(1)
ly = B(label)
best = None
for n in nodes:
    if n.get("checkable") != "true":
        continue
    b = B(n); cy = (b[1] + b[3]) // 2
    if ly[1] - 40 <= cy <= ly[3] + 140 and (best is None or abs(cy - ly[3]) < best[0]):
        best = (abs(cy - ly[3]), (b[0] + b[2]) // 2, cy, n.get("checked"))
if best is None:
    sys.exit(1)
print(best[1], best[2], best[3])
PY
}
profile_notice() { # profile_notice NAME → 0 when the notice row is on Profile
  tap_nav Profile; sleep 2
  scroll_to pre "Live updates are off." 10; local rc=$?
  evidence "$1"; return $rc
}
sc_toggle() {
  local S=toggle r p s
  fresh_app "$RUN_DAY" seed_basic || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  if [[ $API -lt 36 ]]; then
    profile_notice toggle-profile-old
    expect $S "API $API: no Live Updates notice on Profile" "shots/toggle-profile-old.png" eval '! has_text pre "Live updates are off."'
    return
  fi
  promo_settings
  r="$(promo_switch toggle-settings-on)" || { shot toggle-settings-on; check FAIL $S "promotion settings screen" "no switch; ui/toggle-settings-on.xml"; return; }
  shot toggle-settings-on
  read -r -a p <<<"$r"
  [[ "${p[2]}" == true ]] && { tap_xy "${p[0]}" "${p[1]}"; sleep 2; }
  r="$(promo_switch toggle-settings-off)"; shot toggle-settings-off
  expect $S "Live Updates switched off in system settings" "switch checked=$(cut -d' ' -f3 <<<"$r"); shots/toggle-settings-off.png" eval '[[ "$(cut -d" " -f3 <<<"$r")" == false ]]'
  adb_ shell am force-stop "$APP_ID"; launch; wait_screen today 20
  open_tracker; sleep 3
  notif toggle-off-run $RUN_ID >/dev/null
  expect $S "off: the run notification is not promoted" "flags $(nfield toggle-off-run "'|'.join(r['flags'])")" test "$(promoted toggle-off-run)" = False
  goto_day "$RUN_DAY"
  profile_notice toggle-profile-off
  expect $S "off: Profile shows the Live Updates notice" "shots/toggle-profile-off.png ui/toggle-profile-off.xml" has_text pre "Live updates are off."
  # The notice sits at the bottom of Profile; its button can be below the fold.
  if scroll_to eq "Open settings" 3 && tap_node eq "Open settings"; then
    sleep 3
    r="$(promo_switch toggle-opened)"; shot toggle-opened
    expect $S "'Open settings' opens the system Live Updates screen" "focus $(focus); switch '$r'; shots/toggle-opened.png" \
      eval '[[ "$(focus)" == com.android.settings* && -n "$r" ]]'
    read -r -a p <<<"$r"
    tap_xy "${p[0]}" "${p[1]}"; sleep 2
    r="$(promo_switch toggle-back-on)"
    adb_ shell input keyevent 4; sleep 3
    wait_screen other 5 || true
    s="$(focus)"
    has_text pre "Live updates are off."; local still=$?
    evidence toggle-profile-on
    expect $S "back on + return to the app: the notice is gone" "switch now '$r'; focus $s; shots/toggle-profile-on.png" \
      eval '[[ $still -ne 0 && "$s" == "$APP_ID"* ]]'
  else
    check FAIL $S "'Open settings' opens the system Live Updates screen" "no Open settings button; shots/toggle-profile-off.png"
  fi
  # Leave Live Updates on for later checks.
  promo_settings; r="$(promo_switch toggle-final)"; read -r -a p <<<"$r"
  [[ "${p[2]}" == false ]] && tap_xy "${p[0]}" "${p[1]}"
  adb_ shell input keyevent 3
}

# ── Main ─────────────────────────────────────────────────────────────────────
{
  echo "date:      $(date -Is)"
  echo "serial:    $SERIAL (AVD $AVD)"
  echo "apk:       $APK ($(date -r "$APK" -Is))"
  echo "git:       $(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null)$(git -C "$REPO_ROOT" diff --quiet 2>/dev/null || echo ' (dirty tree)')"
  echo "scenarios: $ONLY"
} >"$OUT/config.txt"

boot_emulator
[[ $STARTED_EMU -eq 1 ]] || KEEP_EMU=1
trap 'touch "$OUT/feed.stop"; kill ${LOGCAT_PID:-} 2>/dev/null; stop_emulator' EXIT
API="$(sh_ getprop ro.build.version.sdk)"
echo "device:    $(sh_ getprop ro.product.model), API $API ($(sh_ getprop ro.build.version.release))" >>"$OUT/config.txt"
install_apk || { log "install failed"; exit 1; }
adb_ logcat -c 2>/dev/null || true
adb_ logcat -G 16M >/dev/null 2>&1 || true
adb_ logcat -v threadtime -b main,system,crash,events >"$OUT/logcat.txt" 2>&1 &
LOGCAT_PID=$!

for sc in $ONLY; do
  mark "scenario-$sc"
  if declare -F "sc_$sc" >/dev/null; then "sc_$sc"; else check FAIL "$sc" "unknown scenario" "no function sc_$sc"; fi
  stop_feeder
  adb_ shell cmd statusbar collapse >/dev/null 2>&1
  shot "end-$sc"
done
sleep 2
kill "$LOGCAT_PID" 2>/dev/null; wait "$LOGCAT_PID" 2>/dev/null

python3 "$HELP/scan_logcat.py" "$OUT/logcat.txt" "$OUT/logcat-scan.json" >/dev/null
python3 - "$OUT/logcat-scan.json" "$OUT/checks.tsv" <<'PY'
import json, sys
s = json.load(open(sys.argv[1]))
out = open(sys.argv[2], "a")
for k, label in (("fatal", "Java FATAL EXCEPTION"), ("native", "native crash"), ("anr", "ANR"),
                 ("oom", "OutOfMemoryError"), ("fgs", "foreground-service failure")):
    out.write(f"{'PASS' if s[k] == 0 else 'FAIL'}\tlogcat\t{label}\t{s[k]} occurrence(s) {json.dumps(s['by_phase'].get(k, {}))}\n")
out.write(f"{'PASS' if s['strictmode'] == 0 else 'INFO'}\tlogcat\tStrictMode\t{s['strictmode']} line(s)\n")
PY
python3 "$HELP/report.py" "$OUT"
sed -i '1s/.*/# System surfaces report/' "$OUT/REPORT.md"
log "report: $OUT/REPORT.md"
! grep -q '^FAIL' "$OUT/checks.tsv"
