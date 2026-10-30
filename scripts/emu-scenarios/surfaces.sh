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
#   reminder (opt-in, ~30 min, reboots the device) the daily session reminder:
#           Profile switch + time picker, the posted reminder (title, line, one
#           action) on a run day and a lift day, its action from the shade and
#           from the lock screen (PIN), content tap → Today, nothing on a rest
#           day / under a hold / once done, removed when the session is done or
#           moved, the snapshot-ahead path, a time zone change and a reboot.
#           Uses its own week (Mon 2026-11-02 …): a clock pinned before the
#           system image's build date is moved to it at boot.
# The Live Update checks (chip, promoted flag, toggle) need API 36+; below that
# the same flows check the plain notification and that the Profile notice stays
# hidden.
#
#   scripts/emu-scenarios/surfaces.sh [--avd e37] [--port 5582] [--apk PATH]
#                                     [--only "links tile widget run guided rest toggle restbg tilefresh reminder"]
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
    -h|--help) sed -n '2,50p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

SERIAL="emulator-$PORT"
OUT="$HOME/.cache/emu-surfaces/$AVD/$(date +%Y%m%d-%H%M%S)"
[[ -f "$APK" ]] || { echo "APK not found: $APK" >&2; exit 2; }

[[ -e "$OUT" ]] && { echo "output folder exists: $OUT" >&2; exit 2; }
mkdir -p "$OUT"/{shots,ui,logs}
: >"$OUT/checks.tsv"
# shellcheck source=lib.sh
source "$HELP/lib.sh"
EV() { python3 "$HELP/events.py" "$@"; }
API=0

# ── Emulator ─────────────────────────────────────────────────────────────────
# boot_completed is not enough: a surfaceflinger abort loop starts after it.
check_surfaceflinger() {
  local sf; sf="$(sh_ pidof surfaceflinger)"; sleep 20
  if [[ "$(sh_ pidof surfaceflinger)" != "$sf" ]]; then
    log "surfaceflinger restarted after boot ($(sh_ logcat -d -b crash | grep -m1 'Abort message' || true));" \
      "check target= in ~/.android/avd/$AVD.ini (see the header)"
    exit 1
  fi
}
# `adb install` streams over the package service; on some API 37 images that
# fails with "Broken pipe" while a pushed `pm install` works.
install_apk() {
  adb_ install -r "$APK" >/dev/null 2>&1 && return 0
  adb_ push "$APK" /data/local/tmp/surfaces.apk >/dev/null && sh_ pm install -r /data/local/tmp/surfaces.apk | grep -q Success
}

# ── Device state ─────────────────────────────────────────────────────────────
# The seed (seed_basic "$SEED_DAY" 2): 2 run days, a squat and a 5 km run on
# Sun 2026-09-20, so Mon run, Tue rest, Wed lift.
SEED_DAY="2026-09-20"
# seed_basic plus a pain report on DAY 07:30: training paused.
seed_hold() { # seed_hold DAY
  seed_basic "$SEED_DAY" 2
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
  fresh_app "$LIFT_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
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
  fresh_app "$LIFT_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  add_tile || check FAIL $S "tile added to Quick Settings" "not on the QS panel ~40 s after add-tile; ui/tile-added.xml"
  home
  tile_case lift-day "Log set" log_set
  reseed "$RUN_DAY" seed_basic "$SEED_DAY" 2; home
  tile_case run-day "Start run" track
  reseed "$REST_DAY" seed_basic "$SEED_DAY" 2; home
  tile_case rest-day "Log" log
  reseed "$RUN_DAY" seed_hold "$RUN_DAY"; home
  expect $S "hold seed pauses training" "snapshot kind '$(snap_field kind)'" test "$(snap_field kind)" = Paused
  tile_case hold "Log" log
  # The stored day is Mon; the clock moves to Wed with the app in the background.
  reseed "$RUN_DAY" seed_basic "$SEED_DAY" 2; home
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
  seed_basic "$SEED_DAY" 2 >"$OUT/logs/seed-tilefresh.ndjson"
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
  fresh_app "$RUN_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
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

  reseed "$LIFT_DAY" seed_basic "$SEED_DAY" 2; home
  t="$(widget_texts widget-lift-day)"
  expect $S "lift day: lift title and items, Log only" "snapshot title '$(snap_field title)'; $t; shots/widget-lift-day.png" \
    eval 'grep -qx "text=$(snap_field title)" <<<"$t" && grep -qx "text=Log" <<<"$t" && ! grep -qx "text=Track run" <<<"$t"'

  reseed "$RUN_DAY" seed_hold "$RUN_DAY"; home
  t="$(widget_texts widget-hold)"
  expect $S "hold: Training paused, no reason, Log" "$t; shots/widget-hold.png" \
    eval 'grep -qx "text=Training paused" <<<"$t" && grep -qx "text=Open lollop for details." <<<"$t" && ! grep -q "pain" <<<"$t" && grep -qx "text=Log" <<<"$t"'

  # Date change with the app in the background (not force-stopped: a stopped app
  # gets no TIME_SET broadcast).
  reseed "$RUN_DAY" seed_basic "$SEED_DAY" 2; home
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
  fresh_app "$RUN_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
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
REST_ID=4207      # the countdown
REST_DONE_ID=4209 # the rest-done alert
sc_rest() {
  local S=rest c1 c2 r p s n left
  fresh_app "$LIFT_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
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
    notif rest-done $REST_DONE_ID >/dev/null; notif rest-done-countdown $REST_ID >/dev/null; c1="$(chip rest-chip-done)"; sb_shot rest-done
    expect $S "rest end: 'Rest done' alert replaces the countdown, not promoted, chip gone" \
      "channel $(nfield rest-done "r['channel']"); title '$(nfield rest-done "r['extras'].get('android.title')")'; flags $(nfield rest-done "'|'.join(r['flags'])"); countdown $(cat "$OUT/logs/rest-done-countdown-notif.json"); chip '$c1'" \
      eval '[[ "$(nfield rest-done "r[\"channel\"]")" == rest_done && "$(promoted rest-done)" == False && "$(cat "$OUT/logs/rest-done-countdown-notif.json")" == null && -z "$c1" ]]'
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
    notif "restbg-$c-done" $REST_DONE_ID >/dev/null
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
  fresh_app "$LIFT_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL restbg "seeded boot" "Today never rendered"; return; }
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
  fresh_app "$RUN_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
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

# ── Session reminder ─────────────────────────────────────────────────────────
REM_ID=4301
REM_PIN=1234
REM_ALARM=""
# The reminder alarm's next time ("YYYY-MM-DD HH:MM:SS", device local); empty
# when none is set.
rem_alarm() {
  sh_ dumpsys alarm | grep -A3 'walarm\*:app.milestone.action.SESSION_REMINDER' |
    sed -n 's/.*origWhen=\([0-9-]* [0-9:]*\).*/\1/p' | head -1
}
rem_epoch() { echo $(( $(TZ=UTC date -d "$1" +%s) / 86400 )); }
# rem_want [DAY]: "title<TAB>line<TAB>action" the reminder should show for the
# stored snapshot's own day, or for its day ahead DAY (the copy rules of
# reminderLine / reminderActionLabel in SessionReminder.kt).
rem_want() {
  python3 - "$(snapshot)" "${1:+$(rem_epoch "$1")}" <<'PY'
import json, sys
d = json.loads(sys.argv[1])
day = next(a for a in d["ahead"] if a["epochDay"] == int(sys.argv[2])) if sys.argv[2] else d
items = day["items"]
if day["kind"] == "Run":
    r = next(i for i in items if i["kind"] == "Run" and not i["done"])
    line, act = " · ".join(x for x in (r["detail"], r["pace"]) if x.strip()), "Start run"
else:
    lifts = [i for i in items if i["kind"] == "Lift"]
    op = next(i for i in lifts if not i["done"])
    done = sum(i["done"] for i in lifts)
    if len(lifts) == 1:
        line = f"{op['name']} · {op['detail']}" if op["detail"] else op["name"]
    elif done == 0:
        line = f"{len(lifts)} exercises · {op['name']} first"
    else:
        line = f"{done} of {len(lifts)} done · {op['name']} next"
    act = "Log next set" if done else "Log first set"
print(day["title"], line, act, sep="\t")
PY
}
# The first open lift of the stored snapshot's day.
rem_open_lift() { snapshot | python3 -c "import json,sys;print(next(i['name'] for i in json.load(sys.stdin)['items'] if i['kind']=='Lift' and not i['done']))" 2>/dev/null; }
rem_set_line() { echo "{\"LogSet\":{\"exercise\":\"$1\",\"weight_kg\":$2,\"reps\":$3,\"rpe\":8.0,\"observed_at\":$4,\"entry_id\":${4}000}}"; }
# seed_basic plus the lift day's sets from 06:00, all but the last Cable row set.
seed_rem_lift_left() {
  local t i
  seed_basic "$SEED_DAY" 2
  t="$(TZ="$TZ0" date -d "$LIFT_DAY 06:00" +%s)"
  for i in 1 2 3 4 5; do rem_set_line "Back Squat" 117.5 3 $t; t=$((t + 180)); done
  for i in 1 2 3; do rem_set_line Deadlift 140.0 5 $t; t=$((t + 180)); done
  for i in 1 2 3; do rem_set_line "Bench press" 90.0 5 $t; t=$((t + 180)); done
  for i in 1 2; do rem_set_line "Cable row" 60.0 5 $t; t=$((t + 180)); done
}
# seed_basic plus the run day's run, logged at 07:00.
seed_rem_run_done() {
  local y; y="$(TZ="$TZ0" date -d "$RUN_DAY 07:00" +%s)"
  seed_basic "$SEED_DAY" 2
  echo "{\"LogRun\":{\"distance_km\":5.5,\"duration_min\":31.0,\"hr_pct_max\":0.0,\"longest_recent_km\":0.0,\"observed_at\":$y,\"entry_id\":${y}001}}"
}
# rem_fire NAME DAY [MODE]: background the app and move the clock to DAY
# 07:44:20 (the reminder is set for 07:45), then wait (max 5 min) until the
# alarm has gone off: it then points past DAY 07:45. MODE `keep` leaves app and
# clock alone; `locked` sets a PIN and turns the screen off before the wait (a
# reminder seen in the shade shows only as an icon on the API 37 lock screen).
# Saves the alarm dumps and the notification (logs/rem-NAME-*); 1 when the
# alarm was not set for DAY 07:45 or never went off (REM_ALARM says which).
rem_fire() {
  local i a="" b
  case "${3:-}" in
    keep) ;;
    locked) home; pin_clock "$2" 07:44:20; sh_ locksettings set-pin $REM_PIN >/dev/null; adb_ shell input keyevent 26; sleep 2 ;;
    *) home; pin_clock "$2" 07:44:20; sleep 2 ;;
  esac
  sh_ dumpsys alarm >"$OUT/logs/rem-$1-alarm-before.txt"
  b="$(rem_alarm)"
  [[ "$b" == "$2 07:45"* ]] || { REM_ALARM="not set for $2 07:45 (next '$b'); logs/rem-$1-alarm-before.txt"; return 1; }
  for i in $(seq 1 60); do
    a="$(rem_alarm)"; [[ -n "$a" && "$a" != "$2 07:45"* ]] && break
    sleep 5
  done
  sleep 4
  sh_ dumpsys alarm >"$OUT/logs/rem-$1-alarm-after.txt"
  notif "rem-$1" $REM_ID >/dev/null
  if [[ "${3:-}" != locked ]]; then
    adb_ shell cmd statusbar expand-notifications; sleep 2; shot "rem-$1-shade"; adb_ shell cmd statusbar collapse
  fi
  REM_ALARM="next '$b' → '$a' (~$((i * 5)) s, device $(sh_ "date '+%F %T %Z'")); logs/rem-$1-alarm-{before,after}.txt shots/rem-$1-shade.png"
  [[ -n "$a" && "$a" != "$2 07:45"* ]]
}
# rem_case CASE DAY WANT [MODE]: rem_fire, then the reminder must be posted
# with WANT (rem_want) on the session_reminder channel (category reminder,
# public on the lock screen, exactly one action), or nothing when WANT is empty.
rem_case() {
  local c="$1" n t l a gt gl ga meta
  if ! rem_fire "$c" "$2" "${4:-}"; then
    check FAIL $S "$c: the reminder alarm goes off at 07:45" "$REM_ALARM"; return 1
  fi
  n="$(cat "$OUT/logs/rem-$c-notif.json")"
  if [[ -z "$3" ]]; then
    expect $S "$c: alarm went off, nothing posted" "notification $n; alarm $REM_ALARM" test "$n" = null
    return
  fi
  IFS=$'\t' read -r t l a <<<"$3"
  gt="$(nfield "rem-$c" "r['extras'].get('android.title')")"
  gl="$(nfield "rem-$c" "r['extras'].get('android.text')")"
  ga="$(nfield "rem-$c" "'|'.join(r['actions'])")"
  meta="$(nfield "rem-$c" "' '.join(str(r[k]) for k in ('channel', 'category', 'vis'))")"
  expect $S "$c: reminder posted: '$t' / '$l' / one action '$a'" \
    "title '$gt', text '$gl', actions '$ga', channel category vis '$meta'; alarm $REM_ALARM; logs/rem-$c-notif.txt" \
    eval '[[ "$gt" == "$t" && "$gl" == "$l" && "$ga" == "$a" && "$meta" == "session_reminder reminder PUBLIC" ]]'
}
# rem_lock ACTION TITLE WANT-SCREEN: after rem_fire's `locked` mode, screen on;
# tap ACTION on the reminder on the lock screen and enter the PIN on the
# bouncer. The app must then open WANT-SCREEN. On the way to ACTION: the
# reminder's Expand button when it is collapsed, and the icon shelf when it
# sits there (API 37 shows one notification in full, the rest as icons).
# Clears the PIN.
rem_lock() {
  local e r="" p s="" bouncer="" i d=rem-lock want="$3"
  adb_ shell input keyevent 224; sleep 3
  for i in 1 2 3 4; do
    wdump "$d"; shot "$d"
    r="$(wfind "$d" eq "$1")" && break
    # The keyguard's shade can keep its pre-expansion tree in the dump; the
    # screenshot then shows the action (as in tap_shade).
    r="$(ocr_find "$d" "${1%% *}")" && { log "rem_lock: '$1' located by OCR at $r on shots/$d.png"; break; }
    r=""
    e="$(wfind "$d" expand "$2")" || e="$(wfind "$d" pre "lollop notification")" || break
    read -r -a p <<<"$e"; tap_xy "${p[0]}" "${p[1]}"; sleep 2
    d="rem-lock-$i"
  done
  if [[ -n "$r" ]]; then
    read -r -a p <<<"$r"; tap_xy "${p[0]}" "${p[1]}"; sleep 3
    # The PIN bouncer is a secure window (blank screenshot); its state is read
    # from the window dump: still locked, app not in front, bouncer up.
    sh_ dumpsys window >"$OUT/logs/rem-lock-bouncer-window.txt"
    shot rem-lock-bouncer
    bouncer="$(grep -m1 -oE '(isKeyguardShowing|mKeyguardShowing)=true' "$OUT/logs/rem-lock-bouncer-window.txt") $(grep -m1 -oE 'mBouncerVisible=true|bouncer_visible' "$OUT/logs/rem-lock-bouncer-window.txt"); focus $(focus)"
    [[ "$bouncer" == *KeyguardShowing=true\ * && "$bouncer" != *"focus $APP_ID"* ]] || bouncer="not shown ($bouncer)"
    adb_ shell input text $REM_PIN; adb_ shell input keyevent 66; sleep 4
    wait_screen "$3" 10; s="$(screen)"
  else
    s="(no '$1' on the lock screen; ui/$d.xml)"
  fi
  evidence rem-lock-unlocked
  sh_ locksettings clear --old $REM_PIN >/dev/null
  expect $S "lock screen: '$1' asks for the PIN, then opens $3" \
    "before the PIN: '${bouncer:-action not tapped}'; after: screen '$s'; action on shots/$d.png; logs/rem-lock-bouncer-window.txt shots/rem-lock*.png shots/rem-lock-unlocked.png" \
    eval '[[ "$bouncer" == *KeyguardShowing=true* && "$bouncer" != "not shown"* && "$s" == "$want" ]]'
}
sc_reminder() {
  # Its own week: a clock pinned before the system image's build date (API 37:
  # Oct 2026) is moved forward to it at boot, which the reboot case needs.
  local S=reminder SEED_DAY=2026-11-01 RUN_DAY=2026-11-02 REST_DAY=2026-11-03 LIFT_DAY=2026-11-04
  local w t l a r p s rc ex a0 a1 n i want
  fresh_app "$RUN_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }

  mark reminder-profile
  tap_nav Profile; sleep 2
  scroll_to eq "Session reminder" 12
  # The switch sits at the fold: one more swipe brings up its Time row too.
  adb_ shell input swipe 540 1450 540 950 350; sleep 1
  evidence rem-profile-off
  python3 "$HELP/node.py" on "Session reminder" <"$OUT/ui/rem-profile-off.xml"; rc=$?
  a0="$(rem_alarm)"
  expect $S "Profile › During training: 'Session reminder' off by default, no Time row, no alarm" \
    "switch rc $rc (1 = off); alarm '$a0'; shots/rem-profile-off.png" \
    eval '[[ $rc -eq 1 && -z "$a0" ]] && ! python3 "$HELP/node.py" eq Time <"$OUT/ui/rem-profile-off.xml" >/dev/null'
  tap_node eq "Session reminder"; sleep 2
  evidence rem-profile-on
  python3 "$HELP/node.py" on "Session reminder" <"$OUT/ui/rem-profile-on.xml"; rc=$?
  a1="$(rem_alarm)"
  expect $S "switched on: Time row '7:30 AM', alarm at the next 07:30" \
    "switch rc $rc (0 = on); alarm '$a1' (clock $RUN_DAY 08:00); shots/rem-profile-on.png" \
    eval '[[ $rc -eq 0 && "$a1" == "$REST_DAY 07:30"* ]] && python3 "$HELP/node.py" re "^7:30\sAM$" <"$OUT/ui/rem-profile-on.xml" >/dev/null'
  tap_node eq Time; sleep 2
  evidence rem-picker
  r="$(find_node eq "7 o'clock" PM)" && { read -r -a p <<<"$r"; tap_xy "${p[0]}" "${p[1]}"; sleep 2; }
  tap_node eq "45 minutes"; sleep 1
  evidence rem-picker-set
  tap_node eq OK; sleep 2
  evidence rem-profile-time
  a1="$(rem_alarm)"
  expect $S "time picker (dial 7, then 45, OK): row '7:45 AM', alarm moved to 07:45" \
    "alarm '$a1'; shots/rem-picker.png shots/rem-picker-set.png shots/rem-profile-time.png" \
    eval '[[ "$a1" == "$REST_DAY 07:45"* ]] && python3 "$HELP/node.py" re "^7:45\sAM$" <"$OUT/ui/rem-profile-time.xml" >/dev/null'

  # Run day: posted with the app in the background; its action from the lock
  # screen; a moved session takes it down.
  mark reminder-run
  w="$(rem_want)"; IFS=$'\t' read -r t l a <<<"$w"
  rem_case run "$RUN_DAY" "$w" locked
  mark reminder-lock
  rem_lock "$a" "$t" track
  # Pre-start: Start offered, no Stop (the locate preview may run its service).
  expect $S "run: the action lands on the tracker before the start (not recording)" \
    "Start on screen, no Stop; ui/rem-lock-unlocked.xml" \
    eval 'python3 "$HELP/node.py" eq Start <"$OUT/ui/rem-lock-unlocked.xml" >/dev/null && ! python3 "$HELP/node.py" eq Stop <"$OUT/ui/rem-lock-unlocked.xml" >/dev/null'
  mark reminder-move
  n="$(notif rem-run-kept $REM_ID)"
  # A locating tracker keeps the screen against an open intent: leave it first.
  adb_ shell input keyevent 4; sleep 2
  adb_ shell am start -a app.milestone.action.OPEN -n "$APP_ID/.MainActivity" --es dest today >/dev/null
  wait_screen today 15
  r=""
  if scroll_to eq Move 6 && tap_node eq Move; then
    sleep 2; evidence rem-move-dialog
    r="$(node_label pre "Move to ")"
    [[ -n "$r" ]] && tap_node pre "Move to "
  fi
  sleep 4
  notif rem-moved $REM_ID >/dev/null; evidence rem-moved
  expect $S "plan change (Today › Move › '${r:-no option}'): the posted reminder is removed" \
    "before the move: $([[ "$n" != null ]] && echo posted || echo 'not posted'); after: $(cat "$OUT/logs/rem-moved-notif.json"); snapshot kind '$(snap_field kind)'; shots/rem-move-dialog.png shots/rem-moved.png" \
    eval '[[ "$n" != null && -n "$r" && "$(cat "$OUT/logs/rem-moved-notif.json")" == null && "$(snap_field kind)" == Rest ]]'

  # Lift day: the action from the shade, then a content tap.
  mark reminder-lift
  reseed "$LIFT_DAY" seed_basic "$SEED_DAY" 2
  w="$(rem_want)"; IFS=$'\t' read -r t l a <<<"$w"; ex="$(rem_open_lift)"
  rem_case lift "$LIFT_DAY" "$w"
  s="(no '$a' in the shade)"
  tap_shade rem-lift-action eq "$a" "$t" && { wait_screen log_set 10; s="$(screen)"; }
  evidence rem-lift-action
  expect $S "lift: '$a' (shade) opens the set editor at '$ex'" "screen '$s'; shots/rem-lift-action.png" \
    eval '[[ "$s" == log_set ]] && python3 "$HELP/node.py" eq "Change exercise: $ex" <"$OUT/ui/rem-lift-action.xml" >/dev/null'
  tap_node eq Close 2; sleep 2; has_text eq Discard && { tap_node eq Discard; sleep 1; }
  home
  s="(no '$l' in the shade)"
  tap_shade rem-lift-content eq "$l" "$t" && { wait_screen today 10; s="$(screen)"; }
  evidence rem-lift-content
  n="$(notif rem-lift-tapped $REM_ID)"
  expect $S "content tap opens Today and clears the reminder" "screen '$s'; notification after: $n; shots/rem-lift-content.png" \
    eval '[[ "$s" == today && "$n" == null ]]'

  # Lift day under way: "Log next set"; the last set logged from its editor
  # finishes the session, which takes the reminder down.
  mark reminder-lift-next
  reseed "$LIFT_DAY" seed_rem_lift_left
  w="$(rem_want)"; IFS=$'\t' read -r t l a <<<"$w"; ex="$(rem_open_lift)"
  rem_case lift-next "$LIFT_DAY" "$w"
  s="(no '$a' in the shade)"
  tap_shade rem-next-action eq "$a" "$t" && { wait_screen log_set 10; s="$(screen)"; }
  evidence rem-next-action
  expect $S "lift under way: '$a' opens the set editor at the next exercise '$ex'" "screen '$s'; shots/rem-next-action.png" \
    eval '[[ "$s" == log_set ]] && python3 "$HELP/node.py" eq "Change exercise: $ex" <"$OUT/ui/rem-next-action.xml" >/dev/null'
  n="$(notif rem-next-kept $REM_ID)"
  r="$(node_label re '^Log set [0-9]+( of [0-9]+)?$')"
  tap_top re '^Log set [0-9]+( of [0-9]+)?$'; sleep 4
  evidence rem-next-logged
  notif rem-done-removed $REM_ID >/dev/null
  expect $S "session done (last set, '$r', logged in that editor): the reminder is removed" \
    "before: $([[ "$n" != null ]] && echo posted || echo 'not posted'); after: $(cat "$OUT/logs/rem-done-removed-notif.json"); snapshot sessionDone $(snap_field sessionDone); shots/rem-next-logged.png" \
    eval '[[ "$n" != null && "$(cat "$OUT/logs/rem-done-removed-notif.json")" == null && "$(snap_field sessionDone)" == True ]]'
  tap_node eq Close 2; sleep 2; has_text eq Discard && { tap_node eq Discard; sleep 1; }

  # Nothing on a rest day, under a hold, once the session is done.
  mark reminder-rest
  reseed "$REST_DAY" seed_basic "$SEED_DAY" 2
  expect $S "rest: today is a plan rest day" "snapshot kind '$(snap_field kind)', restDay $(snap_field restDay)" test "$(snap_field kind)" = Rest
  rem_case rest "$REST_DAY" ""
  mark reminder-hold
  reseed "$RUN_DAY" seed_hold "$RUN_DAY"
  expect $S "hold: a pain report pauses training" "snapshot kind '$(snap_field kind)'" test "$(snap_field kind)" = Paused
  rem_case hold "$RUN_DAY" ""
  mark reminder-done
  reseed "$RUN_DAY" seed_rem_run_done
  expect $S "done: today's run is logged" "snapshot kind '$(snap_field kind)', sessionDone $(snap_field sessionDone)" test "$(snap_field sessionDone)" = True
  rem_case done "$RUN_DAY" ""

  # The app last ran on Tue (rest day); the clock moves to Wed morning.
  mark reminder-ahead
  reseed "$REST_DAY" seed_basic "$SEED_DAY" 2
  w="$(rem_want "$LIFT_DAY")"
  rem_case ahead "$LIFT_DAY" "$w"
  expect $S "ahead: posted from Tue's snapshot (its day ahead), app not opened" \
    "snapshot epochDay $(snap_field epochDay) (Tue $(rem_epoch "$REST_DAY")), stale $(snap_field stale); focus '$(focus)'" \
    eval '[[ "$(snap_field epochDay)" == "$(rem_epoch "$REST_DAY")" && "$(focus)" != "$APP_ID"* ]]'

  # Time zone change with the app process alive in the background (not
  # opened): Berlin 08:43 (today's 07:45 has passed) → London 07:43 (it is
  # ahead again).
  mark reminder-tz
  reseed "$LIFT_DAY" seed_basic "$SEED_DAY" 2
  w="$(rem_want)"
  home
  pin_clock "$LIFT_DAY" 08:43; sleep 2; a0="$(rem_alarm)"
  sh_ cmd alarm set-timezone Europe/London >/dev/null; sleep 3; a1="$(rem_alarm)"
  expect $S "time zone change (Berlin 08:43 → London 07:43), app in the background: alarm moved to 07:45 London" \
    "next '$a0' → '$a1'; device $(sh_ "date '+%F %T %Z'")" \
    eval '[[ "$a0" == "$(date -d "$LIFT_DAY +1 day" +%F) 07:45"* && "$a1" == "$LIFT_DAY 07:45"* ]]'
  check INFO $S "tz: today's snapshot after the change (app process alive, not opened)" \
    "stale $(snap_field stale), zone '$(snap_field zone)'; app pid '$(app_pid)'"
  rem_case tz "$LIFT_DAY" "$w" keep
  # The same change with no app process: the receivers' process holds no model,
  # so today's snapshot stays stale and nothing posts until the app is opened.
  mark reminder-tz-cold
  # The force stop takes down the reminder the last case posted (and its alarm,
  # which the launch sets again).
  adb_ shell am force-stop "$APP_ID"
  pin_clock "$LIFT_DAY" 08:43; launch; wait_screen today 20; home; sleep 3
  n="$(app_pid)"; [[ -n "$n" ]] && sh_ kill -9 "$n"; sleep 2
  sh_ cmd alarm set-timezone Europe/London >/dev/null; sleep 3
  check INFO $S "tz-cold: app process killed before the change, not opened (by design nothing posts: today's snapshot is stale)" \
    "pid '$n' killed; snapshot stale $(snap_field stale), zone '$(snap_field zone)'; alarm '$(rem_alarm)'"
  rem_case tz-cold "$LIFT_DAY" "" keep

  # Reboot: the boot broadcast sets the alarm again; it fires without the app.
  # The clock after boot is either the pinned one (API 37: it is past the
  # image's build date) or the host's (API 34); the alarm must be the next 07:45
  # by that clock. A host clock is then moved back to the lift day (its own
  # TIME_SET also sets the alarm) before waiting for it.
  mark reminder-reboot
  pin_clock "$LIFT_DAY" 08:00
  reseed "$LIFT_DAY" seed_basic "$SEED_DAY" 2
  w="$(rem_want)"
  home
  pin_clock "$LIFT_DAY" 07:42; sleep 2; a0="$(rem_alarm)"
  adb_ reboot
  sleep 10; adb_ wait-for-device
  for i in $(seq 1 90); do [[ "$(sh_ getprop sys.boot_completed)" == 1 ]] && break; sleep 2; done
  adb_ root >/dev/null 2>&1; sleep 3; adb_ wait-for-device
  for i in $(seq 1 30); do [[ "$(sh_ getprop sys.boot_completed)" == 1 ]] && break; sleep 2; done
  kill "$LOGCAT_PID" 2>/dev/null
  adb_ logcat -v threadtime -b main,system,crash,events >>"$OUT/logcat.txt" 2>&1 &
  LOGCAT_PID=$!
  # The boot broadcast reaches the app a little after boot_completed.
  for i in $(seq 1 24); do a1="$(rem_alarm)"; [[ -n "$a1" ]] && break; sleep 5; done
  n="$(sh_ "date '+%F %H%M'")"
  if [[ "${n#* }" < 0745 ]]; then want="${n%% *} 07:45"; else want="$(date -d "${n%% *} +1 day" +%F) 07:45"; fi
  expect $S "reboot: the alarm is set again after boot, for the next 07:45 by the device clock (app not opened)" \
    "before '$a0', after boot '$a1' (want '$want', ~$((i * 5)) s after boot_completed); device $(sh_ "date '+%F %T %Z'"), uptime $(sh_ cut -d. -f1 /proc/uptime) s; focus '$(focus)'" \
    eval '[[ "$a1" == "$want"* && "$(focus)" != "$APP_ID"* ]]'
  wake
  if [[ "$want" == "$LIFT_DAY 07:45" ]]; then rem_case reboot "$LIFT_DAY" "$w" keep; else rem_case reboot "$LIFT_DAY" "$w"; fi
}

# ── Main ─────────────────────────────────────────────────────────────────────
{
  echo "date:      $(date -Is)"
  echo "serial:    $SERIAL (AVD $AVD)"
  echo "apk:       $APK ($(date -r "$APK" -Is))"
  echo "git:       $(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null)$(git -C "$REPO_ROOT" diff --quiet 2>/dev/null || echo ' (dirty tree)')"
  echo "scenarios: $ONLY"
} >"$OUT/config.txt"

boot_emulator "$AVD" "$OUT/logs/emulator.log" 3072
trap 'touch "$OUT/feed.stop"; kill ${LOGCAT_PID:-} 2>/dev/null; stop_emulator' EXIT
check_surfaceflinger
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
