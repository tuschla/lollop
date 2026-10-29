#!/usr/bin/env bash
# Deterministic on-device scenario suite for lollop (app.milestone) on the AVD
# `e34`: system-state changes a host JVM can't reproduce, each with explicit
# PASS/FAIL checks and captured evidence (UI-tree XML, screenshots, event logs).
#
#   locale      per-app locale switches while open and mid-editor (de-DE, ar RTL,
#               ja) and mid-run; the Auto distance unit follows the locale
#   time        24 h vs 12 h, clock change mid-day, time-zone change across
#               midnight (backgrounded / foregrounded), TZ + clock jump mid-run
#   appearance  dark/light toggle mid-editor, font scale 0.85/1.3/2.0 and display
#               size: the set editor still logs a set
#   perms       location / notification permission revoked mid-run → relaunch,
#               resume, grant
#   power       battery saver on: launch + a screen-off stretch mid-run
#   death       process death mid-editor (am kill) and mid-run (am kill under a
#               foreground service, am crash, SIGKILL), relaunch + resume
#   lowmem      am send-trim-memory foreground / background / mid-run
#   update      install -r over itself with data (idle and mid-run)
#   backup      Export → clear data → Import through the system file picker
#   longlog     5-year LongHistorySeed log: cold start + heap at the AVD's default
#               heap limit, then boot/History/export/import under a 128 MB
#               dalvik.vm.heapgrowthlimit (zygote restarted, proven by HeapProbe)
#
#   scripts/emulator-scenarios.sh [--only "locale time ..."] [--keep-emulator]
#                                 [--regen-seed]
#
# Debug APK only: the release APK ships arm64-v8a alone (build.gradle.kts
# abiFilters), so on this x86_64 AVD its core runs under libndk_translation, where
# every JNI call returns null ("Core: event rejected" + "core view() returned
# null" in logcat): it can't be measured here.
#
# GPS is fed with `adb emu geo fix` (scripts/emu-scenarios/feeder.py, 1 Hz,
# constant 3 m/s ellipse). The device clock is pinned to Mon 2026-09-21 08:00
# Europe/Berlin before every scenario. Outputs: ~/.cache/emu-scenarios/
# (REPORT.md, checks.tsv, measurements.tsv, logcat.txt, shots/, ui/, logs/).
# Exit status 0 when no check FAILed. The emulator is stopped at the end unless
# --keep-emulator (or it was already running when the script started).
# Needs: Android SDK (emulator, adb, build-tools d8), JDK (javac; gradle once for
# the 5-year seed, cached in ~/.cache/emu-scenarios/seed/), python3 + Pillow;
# tesseract with a Latin-script language for the ar bidi check (else INFO/skip).
# One full run takes ~40 min.
set -uo pipefail

cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"
HELP="$REPO_ROOT/scripts/emu-scenarios"
APP_ID="app.milestone"
SERIAL="emulator-5580"
AVD="e34"
OUT="$HOME/.cache/emu-scenarios"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
ONLY="locale time appearance perms power death lowmem update backup longlog"
KEEP_EMU=0
REGEN=0
PIN_DATE="2026-09-21"
TZ0="Europe/Berlin"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --only) ONLY="$2"; shift 2 ;;
    --keep-emulator) KEEP_EMU=1; shift ;;
    --regen-seed) REGEN=1; shift ;;
    -h|--help) sed -n '2,41p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

[[ -f "$APK" ]] || { echo "APK not found: $APK (scripts/build.sh --no-tests)" >&2; exit 2; }

# Keep the generated seed + compiled probe across runs; everything else is per run.
mkdir -p "$OUT"
find "$OUT" -mindepth 1 -maxdepth 1 ! -name seed ! -name probe -exec rm -rf {} +
mkdir -p "$OUT"/{shots,ui,logs,seed,probe}
: >"$OUT/checks.tsv"
: >"$OUT/measurements.tsv"
# shellcheck source=emu-scenarios/lib.sh
source "$HELP/lib.sh"
measure() { printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$OUT/measurements.tsv"; log "measure $1 · $2 = $3"; }
EV() { python3 "$HELP/events.py" "$@"; }
dev_now() { sh_ date +%s; }

# ── App state ────────────────────────────────────────────────────────────────
# reset_app seeded|fresh [log-file]: wipe data, baseline, (seed) and land on Today.
# The default seed: 2 run days, a squat and a 5 km run on Sun 2026-09-20, so
# Today (Mon) is an Easy run day.
reset_app() {
  stop_feeder
  adb_ shell am force-stop "$APP_ID"
  adb_ shell pm clear "$APP_ID" >/dev/null
  baseline_device
  grant_perms
  pin_clock "$PIN_DATE"
  if [[ "$1" == seeded ]]; then
    launch; sleep 4; adb_ shell am force-stop "$APP_ID"
    local f="${2:-$OUT/seed/basic.ndjson}"
    [[ -n "${2:-}" ]] || seed_basic 2026-09-20 2 >"$f"
    push_log "$f" || return 1
    launch
    wait_text pre "TODAY'S SESSION" 40
  else
    launch; sleep 5
  fi
}

# ── Set editor ───────────────────────────────────────────────────────────────
open_set_editor() {
  tap_node eq Log || return 1; sleep 2
  tap_node eq "Log set" || return 1
  wait_text eq "Exercise" 8 || return 1
  # No exercise chosen yet: the search field shows its placeholder.
  if has_text eq "Search exercises"; then tap_node eq "Back Squat" || return 1; sleep 2; fi
  return 0
}
# Tap MODE TEXT inside the editor, scrolling its content up to reveal it (large
# fonts / display sizes push the weight and reps rows below the pinned footer).
tap_scroll() {
  local i b
  for i in 1 2 3 4 5 6; do
    tap_node "$1" "$2" 1 2>/dev/null && return 0
    b="$(find_node scroll)" || b="0 450 1080 1050"
    read -r -a b <<<"$b"
    adb_ shell input swipe 540 $(( b[3] - 40 )) 540 $(( b[1] + (b[3] - b[1]) / 4 )) 400; sleep 1
  done
  return 1
}
# 120 → 122.5 kg, 5 reps, 2 reps left (RPE 8).
editor_fill() {
  tap_scroll eq "Add 2.5 kg" || return 1
  tap_scroll eq "Reps, 5" || return 1
  tap_scroll eq "2 reps left" || return 1
  sleep 1
}
editor_has_values() { # editor_has_values WEIGHT
  local x
  x="$(dump_ui)" || return 1
  python3 "$HELP/node.py" eq "Weight, $1 kg" <<<"$x" >/dev/null &&
    python3 "$HELP/node.py" on "Reps, 5" <<<"$x" && python3 "$HELP/node.py" on "2 reps left" <<<"$x"
}
close_editor() {
  if has_text eq "Done for now"; then tap_node eq "Done for now"
  else tap_node eq Close; fi
  sleep 2
  has_text eq Discard && { tap_node eq Discard; sleep 1; }
  # An editor opened from the Log sheet returns to it: dismiss that too.
  has_text eq "Exercise, weight, reps" && { adb_ shell input keyevent 4; sleep 2; }
  return 0
}
# The editor's "Logged" time chip ("Today · 8:00 AM"), scrolled into view.
editor_chip() {
  local i l
  for i in 1 2 3 4 5; do
    l="$(node_label re '^Today · ')"
    [[ -n "$l" ]] && { echo "$l"; return 0; }
    adb_ shell input swipe 540 1000 540 450 300; sleep 1
  done
  return 1
}

# ── Tracked runs ─────────────────────────────────────────────────────────────
start_run() {
  tap_node eq Log || return 1; sleep 2
  tap_node eq "Track run (GPS)" || return 1
  wait_text eq Start 20 || return 1
  sleep 4
  tap_node eq Start || return 1
  wait_text eq Stop 20
}
# Stop → Finish sheet (summary into $FINISH) → Save run.
FINISH=""
finish_run() {
  FINISH=""
  if has_text eq Stop; then tap_node eq Stop; fi
  wait_text eq "Save run" 15 || return 1
  FINISH="$(node_label re '^[0-9.]+ (mi|km) · ')"
  tap_node eq "Save run" || return 1
  sleep 5
  ! has_text eq "Save run"
}
# "0:59 moving" → 59
moving_sec() { python3 -c "import re,sys;m=re.search(r'(?:(\d+):)?(\d+):(\d\d) moving',sys.argv[1]);print(int(m.group(1) or 0)*3600+int(m.group(2))*60+int(m.group(3)) if m else '')" "$1"; }
dismiss_crash_dialog() {
  local i
  for i in 1 2 3; do
    if has_text eq "Close app"; then tap_node eq "Close app"; sleep 1; return 0; fi
    sleep 1
  done
  return 0
}
pid_gone() { local i; for i in $(seq 1 "${1:-10}"); do [[ -z "$(app_pid)" ]] && return 0; sleep 1; done; return 1; }

# ═════════════════════════════════════════════════════════════════════════════
# Scenarios
# ═════════════════════════════════════════════════════════════════════════════
set_locale() { sh_ cmd locale set-app-locales "$APP_ID" ${1:+--locales "$1"} >/dev/null; sleep 4; }
plan_unit() { node_label re '^[0-9.]+ (mi|km)$' | awk '{print $NF}'; }
# ocr_check SCENARIO NAME EVIDENCE-TAG MODE TEXT: the rendered (OCR) text of the
# node equals its logical text (whitespace-collapsed). Needs tesseract + PIL;
# without them the check is reported INFO/skipped.
ocr_check() {
  local r rc
  r="$(python3 "$HELP/ocr.py" "$OUT/shots/$3.png" "$OUT/ui/$3.xml" "$4" "$5" "$OUT/logs" 2>/dev/null)"; rc=$?
  if [[ $rc -eq 3 ]]; then check INFO "$1" "$2" "skipped: tesseract with a Latin-script language not installed"; return; fi
  local logical="${r%%$'\t'*}" seen="${r#*$'\t'}"
  expect "$1" "$2" "tree text '$logical' · rendered (OCR of $3.png) '$seen'" test -n "$r" -a "$logical" = "$seen"
}

sc_locale() {
  local S=locale u pid0 W rr p n0 n1 x
  reset_app seeded || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  evidence locale-01-en-today
  u="$(plan_unit)"
  expect $S "en-US: Auto distance unit is miles" "plan distance unit '$u'" test "$u" = mi
  pid0="$(app_pid)"
  set_locale de-DE
  evidence locale-02-de-today
  ocr_check $S "de-DE: plan distance renders as written (OCR control)" locale-02-de-today re '^[0-9.]+ (km|mi)$'
  expect $S "de-DE applied while open, same process" "pid $pid0 → $(app_pid), focus $(focus)" \
    test "$(app_pid)" = "$pid0" -a -n "$pid0"
  u="$(plan_unit)"
  expect $S "de-DE: Auto distance unit switches to km" "plan distance unit '$u' (Units.kt localeDistanceUnit)" test "$u" = km

  mark locale-mid-editor
  set_locale en-US
  open_set_editor && editor_fill || check FAIL $S "open set editor" "editor controls not found"
  evidence locale-03-en-editor
  set_locale de-DE
  evidence locale-04-de-editor
  expect $S "editor survives a de-DE switch with its values" "122.5 kg · 5 reps · 2 left still selected" editor_has_values 122.5
  check INFO $S "de-DE weight rendering" "$(node_label pre 'Weight, ') · $(node_label re '^[0-9]+([.,][0-9]+)?$')"
  tap_node re '^Log set [0-9]+$'; sleep 3
  pull_log "$OUT/logs/locale-de.ndjson"
  x="$(EV lastset "$OUT/logs/locale-de.ndjson")"
  expect $S "set logged under de-DE keeps the decimal" "last LogSet $x" test "$x" = "Back Squat|122.5|5|8.0"

  mark locale-ar
  set_locale ar
  evidence locale-05-ar-editor
  expect $S "editor survives an ar (RTL) switch" "after logging, 'Done for now' still on screen" has_text eq "Done for now"
  close_editor
  sleep 2
  evidence locale-06-ar-today
  # The app ships English strings only but declares supportsRtl: in an RTL locale
  # an English value must still read left-to-right ("5.5 km", not "km 5.5").
  ocr_check $S "ar: English plan distance renders left-to-right" locale-06-ar-today re '^[0-9.]+ (km|mi)$'
  check INFO $S "ar: coach-note paragraph as rendered" "$(python3 "$HELP/ocr.py" "$OUT/shots/locale-06-ar-today.png" "$OUT/ui/locale-06-ar-today.xml" re '^[A-Z].*\.$' "$OUT/logs" 2>/dev/null | tr '\t' '|')"
  W="$(sh_ wm size | sed -n 's/.*: *\([0-9]*\)x.*/\1/p' | tail -1)"
  rr="$(find_node eq Today)"; read -r -a p <<<"${rr:-0 0}"
  expect $S "ar: bottom navigation mirrored (Today on the right)" "Today nav centre x=${p[0]} of $W" test "${p[0]}" -gt $((W / 2))
  rr="$(find_node eq Log)"; read -r -a p <<<"${rr:-9999 0}"
  expect $S "ar: Log FAB mirrored to the left" "Log FAB centre x=${p[0]} of $W" test "${p[0]}" -lt $((W / 2))
  check INFO $S "ar: text nodes outside the screen" "$(dump_ui | python3 "$HELP/node.py" overflow | head -5 | tr '\n' ';')"

  mark locale-ja
  set_locale ja-JP
  expect $S "ja-JP applied while open" "focus $(focus)" app_up
  evidence locale-07-ja-today
  tap_nav History; sleep 3; evidence locale-08-ja-history
  tap_nav Profile; sleep 3; evidence locale-09-ja-profile
  check INFO $S "ja: text nodes outside the screen" "$(dump_ui | python3 "$HELP/node.py" overflow | head -5 | tr '\n' ';')"
  tap_nav Today; sleep 2

  mark locale-mid-run
  set_locale en-US
  start_feeder locale
  if start_run; then
    sleep 15; n0="$(sidecar_points)"
    set_locale de-DE
    sleep 15; n1="$(sidecar_points)"
    evidence locale-10-de-mid-run
    expect $S "run keeps recording through a locale switch" "sidecar fixes $n0 → $n1, service fg: $(svc_fg && echo yes || echo no)" \
      eval 'svc_fg && [[ $n1 -gt $n0 ]]'
    finish_run || check FAIL $S "finish run" "Save run flow failed"
    pull_log "$OUT/logs/locale-run.ndjson"
    x="$(EV count "$OUT/logs/locale-run.ndjson" LogRunTrack)"
    expect $S "exactly one tracked run saved" "$x LogRunTrack (finish: $FINISH)" test "$x" = 1
  else
    check FAIL $S "start tracked run" "tracker Start not reachable"
  fi
  stop_feeder
  set_locale ""
  u="$(plan_unit)"
  check INFO $S "locale reset to system" "plan distance unit '$u'"
}

sc_time() {
  local S=time chip x pid0 t0 t1 rec ms facts wall
  reset_app seeded || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  adb_ shell settings put system time_12_24 24
  open_set_editor; chip="$(editor_chip)"; evidence time-01-24h-editor
  expect $S "24 h setting: editor time chip" "'$chip'" eval '[[ "$chip" =~ ^Today\ ·\ [0-9]{2}:[0-9]{2}$ ]]'
  close_editor
  adb_ shell settings put system time_12_24 12
  open_set_editor; chip="$(editor_chip)"; evidence time-02-12h-editor
  expect $S "12 h setting: editor time chip" "'$chip'" eval '[[ "$chip" =~ ^Today\ ·\ [0-9]{1,2}:[0-9]{2}[^0-9]*[AP]M$ ]]'
  close_editor

  mark time-clock-jump
  pin_clock "$PIN_DATE" 13:30
  sleep 2
  open_set_editor; chip="$(editor_chip)"; evidence time-03-clock-1330
  expect $S "clock set to 13:30 while open: new editor stamps 1:3x PM" "'$chip'" eval '[[ "$chip" =~ 1:3[0-9][^0-9]*PM$ ]]'
  close_editor

  mark time-tz-background
  pin_clock "$PIN_DATE" 23:30
  adb_ shell input keyevent 3; sleep 2
  sh_ cmd alarm set-timezone Asia/Tokyo >/dev/null   # Tue 06:30 local
  sleep 2; launch; sleep 5
  evidence time-04-tokyo-resumed
  x="$(node_label re '^Today, ')"
  expect $S "TZ Berlin 23:30 → Tokyo (Tue 06:30) while backgrounded: Today moves to Tuesday" "week strip '$x'" \
    eval '[[ "$x" == "Today, Tuesday"* ]]'
  mark time-tz-foreground
  sh_ cmd alarm set-timezone "$TZ0" >/dev/null       # back to Mon 23:3x, app in front
  sleep 5
  evidence time-05-berlin-foreground
  x="$(node_label re '^Today, ')"
  check INFO $S "TZ Tokyo → Berlin while foregrounded (no resume)" "week strip '$x' (device local $(sh_ date '+%a %H:%M'))"
  adb_ shell input keyevent 3; sleep 2; launch; sleep 4
  x="$(node_label re '^Today, ')"
  expect $S "after a resume the strip follows the device day again" "week strip '$x'" eval '[[ "$x" == "Today, Monday"* ]]'

  mark time-mid-run
  pin_clock "$PIN_DATE"
  tap_nav Today; sleep 2
  start_feeder time
  if start_run; then
    t0=$(date +%s); pid0="$(app_pid)"
    sleep 25
    sh_ cmd alarm set-timezone Asia/Tokyo >/dev/null
    sleep 10
    sh_ cmd alarm set-time "$(( ($(dev_now) + 3600) * 1000 ))" >/dev/null
    sleep 25
    evidence time-06-mid-run-after-jump
    tap_node eq Stop; t1=$(date +%s); rec=$((t1 - t0))
    finish_run || check FAIL $S "finish run" "Save run flow failed"
    evidence time-07-after-save
    ms="$(moving_sec "$FINISH")"
    expect $S "TZ change + clock +1 h mid-run: finish-sheet moving time ≈ recorded time" \
      "'$FINISH' → ${ms}s vs ${rec}s Start→Stop on the host" eval '[[ -n "$ms" && $ms -le $((rec + 5)) && $ms -ge $((rec - 25)) ]]'
    pull_log "$OUT/logs/time-run.ndjson"
    facts="$(EV track "$OUT/logs/time-run.ndjson")"
    wall="$(python3 -c "import json,sys;f=json.loads(sys.argv[1]);print(f['last_at']-f['first_at'])" "$facts")"
    expect $S "saved run: one track, moving seconds from fixes ≈ recorded" "$facts" \
      python3 -c "import json,sys;f=json.loads(sys.argv[1]);sys.exit(0 if f['tracks']==1 and abs(f['moving_s']-$rec)<=30 else 1)" "$facts"
    check INFO $S "saved run first→last fix span (includes the +1 h wall-clock jump)" "${wall}s; process $pid0 → $(app_pid)"
    tap_nav History; sleep 3; scroll_to re '^[0-9.]+ (mi|km) · ' 6; evidence time-08-history
    x="$(node_label re '^[0-9.]+ (mi|km) · ')"
    local fm; fm="$(python3 -c "import re,sys;m=re.search(r'(\d+:\d\d) moving',sys.argv[1]);print(m.group(1) if m else '')" "$FINISH")"
    expect $S "History row moving time equals the finish sheet" "row '$x' vs finish '$FINISH'" \
      eval '[[ -n "$fm" && "${x##* · }" == "$fm" ]]'
  else
    check FAIL $S "start tracked run" "tracker Start not reachable"
  fi
  stop_feeder
  pin_clock "$PIN_DATE"
}

screen_lum() { adb_ exec-out screencap | python3 "$HELP/lum.py" | cut -d' ' -f1; }
# The set editor still works end-to-end: open, +2.5, Log set → one more LogSet.
editor_logs_a_set() { # tag
  local n0 n1=-1 r p H ok=1
  pull_log "$OUT/logs/$1-before.ndjson"; n0="$(EV count "$OUT/logs/$1-before.ndjson" LogSet)"
  if open_set_editor && tap_scroll eq "Add 2.5 kg"; then
    sleep 1; evidence "$1-editor"
    H="$(sh_ wm size | sed -n 's/.*x\([0-9]*\)$/\1/p' | tail -1)"
    if r="$(find_node re '^Log set [0-9]+$')"; then
      read -r -a p <<<"$r"
      if [[ ${p[5]} -le $H && ${p[4]} -gt ${p[2]} ]]; then
        tap_top re '^Log set [0-9]+$'; sleep 3
        pull_log "$OUT/logs/$1-after.ndjson"; n1="$(EV count "$OUT/logs/$1-after.ndjson" LogSet)"
        ok=0
      fi
    fi
  else
    evidence "$1-editor-failed"
  fi
  close_editor
  [[ $ok -eq 0 && $n1 -eq $((n0 + 1)) ]]
}

sc_appearance() {
  local S=appearance l fs d
  reset_app seeded || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  l="$(screen_lum)"; evidence appearance-01-light
  expect $S "light system theme renders light (Theme mode System)" "mean luma $l" test "$l" -gt 150
  open_set_editor && editor_fill
  adb_ shell cmd uimode night yes >/dev/null; sleep 5
  l="$(screen_lum)"; evidence appearance-02-dark-editor
  expect $S "dark toggle mid-editor: dark rendering" "mean luma $l" test "$l" -lt 90
  expect $S "dark toggle mid-editor keeps the editor values" "122.5 kg · 5 · 2 left" editor_has_values 122.5
  adb_ shell cmd uimode night no >/dev/null; sleep 5
  evidence appearance-03-light-editor
  expect $S "light toggle back mid-editor keeps the editor values" "122.5 kg · 5 · 2 left" editor_has_values 122.5
  close_editor
  for fs in 0.85 1.3 2.0; do
    mark "appearance-font-$fs"
    adb_ shell settings put system font_scale "$fs"; sleep 4
    evidence "appearance-font-$fs-today"
    expect $S "font scale $fs: app up" "focus $(focus)" app_up
    expect $S "font scale $fs: set editor logs a set" "Log set button on screen and tappable" editor_logs_a_set "font-$fs"
    tap_nav Profile; sleep 2
    expect $S "font scale $fs: Profile › Export reachable" "scrolled to 'Export my data'" scroll_to eq "Export my data" 10
    evidence "appearance-font-$fs-profile"
    tap_nav Today; sleep 2
  done
  adb_ shell settings put system font_scale 1.0
  for d in 560 320; do
    mark "appearance-density-$d"
    adb_ shell wm density "$d" >/dev/null; sleep 5
    evidence "appearance-density-$d-today"
    expect $S "display density $d: set editor logs a set" "Log set button on screen and tappable" editor_logs_a_set "density-$d"
  done
  mark appearance-worst
  adb_ shell wm density 560 >/dev/null; adb_ shell settings put system font_scale 2.0; sleep 5
  evidence appearance-worst-today
  expect $S "font 2.0 + density 560: set editor logs a set" "Log set button on screen and tappable" editor_logs_a_set worst
  check INFO $S "font 2.0 + density 560: text nodes outside the screen" "$(dump_ui | python3 "$HELP/node.py" overflow | head -5 | tr '\n' ';')"
  adb_ shell wm density reset >/dev/null; adb_ shell settings put system font_scale 1.0
}

sc_perms() {
  local S=perms n0 n1 x
  reset_app seeded || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  start_feeder perms
  if ! start_run; then check FAIL $S "start tracked run" "tracker Start not reachable"; stop_feeder; return; fi
  sleep 20; n0="$(sidecar_points)"
  mark perms-revoke-location
  adb_ shell pm revoke "$APP_ID" android.permission.ACCESS_FINE_LOCATION
  adb_ shell pm revoke "$APP_ID" android.permission.ACCESS_COARSE_LOCATION
  pid_gone 8 && check INFO $S "revoking location kills the process (platform)" "pid gone"
  launch
  expect $S "relaunch after revoke offers to resume the run" "'Resume your run?' dialog" wait_text eq "Resume your run?" 20
  evidence perms-01-resume-prompt
  tap_node eq Resume; sleep 4
  evidence perms-02-after-resume
  expect $S "resume without location shows the permission rationale (no crash)" "focus $(focus)" \
    eval 'app_up && has_text eq Continue'
  tap_node eq Continue; sleep 3
  expect $S "Continue opens the system location prompt" "focus $(focus)" wait_text eq "While using the app" 10
  tap_node eq "While using the app"; sleep 20
  n1="$(sidecar_points)"
  evidence perms-03-after-grant
  x="$(ui_texts | tr '\n' ' ' | cut -c1-300)"
  expect $S "after granting location from the resume flow the run records again" \
    "sidecar fixes $n0 → $n1; service fg: $(svc_fg && echo yes || echo no); screen: $x" eval 'svc_fg && [[ $n1 -gt $n0 ]]'
  finish_run || check FAIL $S "finish run" "Save run flow failed"
  pull_log "$OUT/logs/perms-loc.ndjson"
  x="$(EV track "$OUT/logs/perms-loc.ndjson")"
  expect $S "the interrupted run is saved once with its pre-revoke fixes" "$x (sidecar had $n0)" \
    python3 -c "import json,sys;f=json.loads(sys.argv[1]);sys.exit(0 if f.get('tracks')==1 and f['points']>=$n0 else 1)" "$x"

  mark perms-revoke-notifications
  tap_nav Today; sleep 2
  if ! start_run; then check FAIL $S "start second run" "tracker Start not reachable"; stop_feeder; return; fi
  sleep 15; n0="$(sidecar_points)"
  adb_ shell pm revoke "$APP_ID" android.permission.POST_NOTIFICATIONS
  pid_gone 8 && check INFO $S "revoking notifications kills the process (platform)" "pid gone"
  launch
  expect $S "relaunch after notification revoke offers to resume" "'Resume your run?'" wait_text eq "Resume your run?" 20
  tap_node eq Resume; sleep 15
  n1="$(sidecar_points)"
  evidence perms-04-notif-revoked-resumed
  expect $S "run resumes recording without notification permission" "sidecar fixes $n0 → $n1; service fg: $(svc_fg && echo yes || echo no)" \
    eval 'svc_fg && [[ $n1 -gt $n0 ]]'
  finish_run || check FAIL $S "finish run" "Save run flow failed"
  pull_log "$OUT/logs/perms-notif.ndjson"
  x="$(EV count "$OUT/logs/perms-notif.ndjson" LogRunTrack)"
  expect $S "two tracked runs saved in total" "$x LogRunTrack" test "$x" = 2
  stop_feeder
  grant_perms
}

sc_power() {
  local S=power n0 n1 off on gap x
  reset_app seeded || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  adb_ shell dumpsys battery unplug >/dev/null
  adb_ shell settings put global low_power 1; sleep 2
  sh_ dumpsys power | grep -iE "battery ?saver|mSettingBatterySaver|low_power" >"$OUT/logs/power-state.txt"
  check INFO $S "battery saver state" "settings low_power=$(sh_ settings get global low_power); $(grep -m2 -i 'saver' "$OUT/logs/power-state.txt" | tr -s ' ' | tr '\n' ';')"
  adb_ shell am force-stop "$APP_ID"; launch
  expect $S "cold launch under battery saver renders Today" "Today hero within 30 s" wait_text pre "TODAY'S SESSION" 30
  evidence power-01-today
  start_feeder power
  if start_run; then
    sleep 15; n0="$(sidecar_points)"
    off="$(dev_now)"; adb_ shell input keyevent 223; sleep 45
    on="$(dev_now)"; wake; launch; sleep 4
    n1="$(sidecar_points)"
    pull_sidecar "$OUT/logs/power-sidecar.ndjson"
    gap="$(python3 - "$OUT/logs/power-sidecar.ndjson" "$off" "$on" <<'PY'
import json, sys
ts = [json.loads(l)["observed_at"] for l in open(sys.argv[1]) if '"lat"' in l]
lo, hi = int(sys.argv[2]), int(sys.argv[3])
inside = [lo] + [t for t in ts if lo <= t <= hi] + [hi]
print(max(b - a for a, b in zip(inside, inside[1:])))
PY
)"
    evidence power-02-after-screen-off
    expect $S "battery saver + 45 s screen off mid-run: service stays foreground" "service fg: $(svc_fg && echo yes || echo no)" svc_fg
    expect $S "battery saver + screen off: fixes keep arriving (max gap ≤ 30 s)" "sidecar $n0 → $n1 fixes; max gap ${gap}s in [$off,$on]" \
      test "${gap:-999}" -le 30
    finish_run || check FAIL $S "finish run" "Save run flow failed"
    pull_log "$OUT/logs/power-run.ndjson"
    x="$(EV count "$OUT/logs/power-run.ndjson" LogRunTrack)"
    expect $S "run saved under battery saver" "$x LogRunTrack (finish: $FINISH)" test "$x" = 1
  else
    check FAIL $S "start tracked run" "tracker Start not reachable"
  fi
  stop_feeder
  adb_ shell settings put global low_power 0
  adb_ shell dumpsys battery reset >/dev/null
}

sc_death() {
  local S=death n0 n1 x l0 l1 pid0 tc facts
  reset_app seeded || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  mark death-editor-kill
  open_set_editor && editor_fill
  adb_ shell input keyevent 3; sleep 2
  adb_ shell am kill "$APP_ID"
  pid_gone 5 && check INFO $S "am kill of the backgrounded app" "process gone" || check FAIL $S "am kill of the backgrounded app" "pid $(app_pid) still alive"
  launch; sleep 6
  evidence death-01-editor-restored
  expect $S "set editor restored with its values after process death" "122.5 kg · 5 · 2 left" editor_has_values 122.5
  close_editor

  mark death-editor-crash
  l0="$(log_lines)"
  open_set_editor && editor_fill
  adb_ shell am crash "$APP_ID" >/dev/null 2>&1; sleep 3
  dismiss_crash_dialog
  launch; sleep 6
  l1="$(log_lines)"
  evidence death-02-after-crash
  expect $S "relaunch after am crash mid-editor" "focus $(focus)" app_up
  pull_log "$OUT/logs/death-crash.ndjson"
  expect $S "event log intact after the crash" "lines $l0 → $l1; unparsable: $(EV count "$OUT/logs/death-crash.ndjson" __unparsable__)" \
    eval '[[ $l1 -ge $l0 && "$(EV count "$OUT/logs/death-crash.ndjson" __unparsable__)" == 0 ]]'
  close_editor

  mark death-run-am-kill
  tap_nav Today; sleep 2
  start_feeder death
  if ! start_run; then check FAIL $S "start tracked run" "tracker Start not reachable"; stop_feeder; return; fi
  sleep 15; pid0="$(app_pid)"
  adb_ shell am kill "$APP_ID"; sleep 3
  expect $S "am kill mid-run: foreground service keeps the process" "pid $pid0 → $(app_pid); fg $(svc_fg && echo yes || echo no)" \
    eval '[[ "$(app_pid)" == "$pid0" ]] && svc_fg'

  mark death-run-am-crash
  n0="$(sidecar_points)"; tc="$(dev_now)"
  adb_ shell am crash "$APP_ID" >/dev/null 2>&1; sleep 3
  dismiss_crash_dialog
  launch
  expect $S "relaunch after am crash mid-run offers to resume" "'Resume your run?'" wait_text eq "Resume your run?" 20
  evidence death-03-resume-prompt
  tap_node eq Resume; sleep 15
  n1="$(sidecar_points)"
  expect $S "resumed run records again after am crash" "sidecar $n0 → $n1; fg $(svc_fg && echo yes || echo no)" eval 'svc_fg && [[ $n1 -gt $n0 ]]'
  finish_run || check FAIL $S "finish run" "Save run flow failed"
  pull_log "$OUT/logs/death-run1.ndjson"
  facts="$(EV track "$OUT/logs/death-run1.ndjson")"
  expect $S "saved run spans the crash (fixes before and after)" "$facts; crash at $tc" \
    python3 -c "import json,sys;f=json.loads(sys.argv[1]);sys.exit(0 if f.get('tracks')==1 and f['first_at']<$tc<f['last_at'] else 1)" "$facts"

  mark death-run-sigkill
  tap_nav Today; sleep 2
  if start_run; then
    sleep 15; n0="$(sidecar_points)"; pid0="$(app_pid)"
    adb_ shell kill -9 "$pid0"; sleep 3
    check INFO $S "SIGKILL mid-run (low-memory-killer stand-in)" "pid $pid0 → '$(app_pid)'"
    launch
    expect $S "relaunch after SIGKILL mid-run offers to resume" "'Resume your run?'" wait_text eq "Resume your run?" 20
    tap_node eq Resume; sleep 12
    n1="$(sidecar_points)"
    expect $S "resumed run records again after SIGKILL" "sidecar $n0 → $n1; fg $(svc_fg && echo yes || echo no)" eval 'svc_fg && [[ $n1 -gt $n0 ]]'
    finish_run || check FAIL $S "finish run after SIGKILL" "Save run flow failed"
    pull_log "$OUT/logs/death-run2.ndjson"
    x="$(EV count "$OUT/logs/death-run2.ndjson" LogRunTrack)"
    expect $S "two runs saved, no duplicate from recovery" "$x LogRunTrack" test "$x" = 2
  else
    check FAIL $S "start second run" "tracker Start not reachable"
  fi
  stop_feeder
}

sc_lowmem() {
  local S=lowmem pid0 r n0 n1 x
  reset_app seeded || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  pid0="$(app_pid)"
  r="$(sh_ am send-trim-memory "$APP_ID" RUNNING_CRITICAL)"
  sleep 3; evidence lowmem-01-fg-critical
  expect $S "RUNNING_CRITICAL in foreground: app keeps running" "'$r'; pid $pid0 → $(app_pid)" eval 'app_up && [[ "$(app_pid)" == "$pid0" ]]'
  open_set_editor && editor_fill
  adb_ shell input keyevent 3; sleep 3
  r="$(sh_ am send-trim-memory "$APP_ID" COMPLETE)"
  sleep 2; launch; sleep 4
  evidence lowmem-02-bg-complete
  expect $S "TRIM_MEMORY_COMPLETE in background: editor values kept" "'$r'" editor_has_values 122.5
  close_editor
  tap_nav Today; sleep 2
  start_feeder lowmem
  if start_run; then
    sleep 10; n0="$(sidecar_points)"
    r="$(sh_ am send-trim-memory "$APP_ID" RUNNING_CRITICAL)"
    sleep 15; n1="$(sidecar_points)"
    expect $S "RUNNING_CRITICAL mid-run: still recording" "'$r'; sidecar $n0 → $n1; fg $(svc_fg && echo yes || echo no)" eval 'svc_fg && [[ $n1 -gt $n0 ]]'
    finish_run || check FAIL $S "finish run" "Save run flow failed"
    pull_log "$OUT/logs/lowmem-run.ndjson"
    x="$(EV count "$OUT/logs/lowmem-run.ndjson" LogRunTrack)"
    expect $S "run saved after trim" "$x LogRunTrack" test "$x" = 1
  else
    check FAIL $S "start tracked run" "tracker Start not reachable"
  fi
  stop_feeder
}

sc_update() {
  local S=update m0 m1 u r x
  reset_app seeded || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  tap_nav Profile; sleep 2
  scroll_to eq "Distance units" 6 && tap_node eq km; sleep 2
  tap_nav Today; sleep 2
  open_set_editor && editor_fill && tap_node re '^Log set [0-9]+$'; sleep 3; close_editor
  pull_log "$OUT/logs/update-before.ndjson"; m0="$(md5sum <"$OUT/logs/update-before.ndjson" | cut -c1-12)"
  r="$(adb_ install -r "$APK" 2>&1 | tail -1)"
  check INFO $S "install -r over the running app" "$r; pid after: '$(app_pid)'"
  pull_log "$OUT/logs/update-after.ndjson"; m1="$(md5sum <"$OUT/logs/update-after.ndjson" | cut -c1-12)"
  expect $S "event log byte-identical across the update" "md5 $m0 → $m1 ($(grep -c . "$OUT/logs/update-after.ndjson") lines)" test "$m0" = "$m1"
  launch
  expect $S "app boots after the update" "Today rendered" wait_text pre "TODAY'S SESSION" 30
  u="$(plan_unit)"; evidence update-01-after
  expect $S "settings survive the update (distance unit km)" "plan distance unit '$u'" test "$u" = km

  mark update-mid-run
  start_feeder update
  if start_run; then
    sleep 15
    r="$(adb_ install -r "$APK" 2>&1 | tail -1)"
    launch
    expect $S "relaunch after an update mid-run offers to resume" "'$r'" wait_text eq "Resume your run?" 20
    evidence update-02-resume-prompt
    tap_node eq Resume; sleep 12
    expect $S "run resumes after the update" "fg $(svc_fg && echo yes || echo no)" svc_fg
    finish_run || check FAIL $S "finish run" "Save run flow failed"
    pull_log "$OUT/logs/update-run.ndjson"
    x="$(EV count "$OUT/logs/update-run.ndjson" LogRunTrack)"
    expect $S "run saved once after the update" "$x LogRunTrack" test "$x" = 1
  else
    check FAIL $S "start tracked run" "tracker Start not reachable"
  fi
  stop_feeder
}

# Profile › Export my data → system picker SAVE into Downloads. Echoes the path.
export_backup() {
  local name="lollop-$PIN_DATE.ndjson" i s0 s1
  adb_ shell "rm -f /sdcard/Download/lollop-*.ndjson"
  tap_nav Profile; sleep 2
  scroll_to eq "Export my data" 10 || return 1
  tap_node eq "Export my data"
  wait_text eq SAVE 15 || return 1
  has_text eq "$name" || log "picker suggests $(node_label pre lollop-)"
  tap_top eq SAVE
  # A large export streams for a while: wait for the file size to settle.
  s0=-1
  for i in $(seq 1 60); do
    sleep 2
    s1="$(sh_ stat -c %s "/sdcard/Download/$name" 2>/dev/null)"
    [[ -n "$s1" && "$s1" == "$s0" && "$(focus)" == "$APP_ID"* ]] && break
    s0="$s1"
  done
  echo "/sdcard/Download/$name"
}
# Profile › Import my data → pick FILE in Downloads → Replace.
import_backup() {
  local name; name="$(basename "$1")"
  tap_nav Profile; sleep 2
  scroll_to eq "Import my data" 10 || return 1
  tap_node eq "Import my data"
  wait_text eq "$name" 15 || return 1
  tap_node eq "$name"
  wait_text eq "Replace" 60 || return 1
  evidence "import-confirm-$2"
  tap_node eq Replace
  sleep 8
}

sc_backup() {
  local S=backup f c0 c1 c2 x
  reset_app seeded || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  open_set_editor && editor_fill && tap_node re '^Log set [0-9]+$'; sleep 3; close_editor
  pull_log "$OUT/logs/backup-device.ndjson"
  EV canon "$OUT/logs/backup-device.ndjson" >"$OUT/logs/backup-device.canon"
  f="$(export_backup)"
  evidence backup-01-after-export
  adb_ pull "$f" "$OUT/logs/backup-exported.ndjson" >/dev/null 2>&1
  EV canon "$OUT/logs/backup-exported.ndjson" >"$OUT/logs/backup-exported.canon" 2>/dev/null
  expect $S "export writes every logged event" "$(grep -c . "$OUT/logs/backup-exported.canon" 2>/dev/null) exported vs $(grep -c . "$OUT/logs/backup-device.canon") on device" \
    cmp -s "$OUT/logs/backup-device.canon" "$OUT/logs/backup-exported.canon"
  mark backup-clear
  adb_ shell am force-stop "$APP_ID"; adb_ shell pm clear "$APP_ID" >/dev/null
  grant_perms; pin_clock "$PIN_DATE"; launch; sleep 5
  has_text eq Skip && { tap_node eq Skip; sleep 3; }
  expect $S "cleared app starts empty" "log lines $(log_lines)" test "$(log_lines)" -le 1
  import_backup "$f" clear
  evidence backup-02-after-import
  pull_log "$OUT/logs/backup-imported.ndjson"
  EV canon "$OUT/logs/backup-imported.ndjson" >"$OUT/logs/backup-imported.canon"
  expect $S "import restores exactly the exported events" "diff: $(diff "$OUT/logs/backup-device.canon" "$OUT/logs/backup-imported.canon" | head -3 | tr '\n' ' ')" \
    cmp -s "$OUT/logs/backup-device.canon" "$OUT/logs/backup-imported.canon"
  tap_nav Today; sleep 3
  expect $S "imported data drives Today again" "Today's session rendered" wait_text pre "TODAY'S SESSION" 15
  x="$(EV lastset "$OUT/logs/backup-imported.ndjson")"
  check INFO $S "last imported set" "$x"
}

# ── 5-year history ───────────────────────────────────────────────────────────
FIVE="$OUT/seed/five-year.ndjson"
gen_five_year() {
  [[ -s "$FIVE" && $REGEN -eq 0 ]] && return 0
  log "generating the 5-year LongHistorySeed log (gradle, EmuSeedExportTest)"
  printf '%s\n' 'allprojects { layout.buildDirectory.set(file("${System.getProperty("user.home")}/.cache/emuscenarios-build/${project.name}")) }' \
    >"$HOME/.cache/emuscenarios.init.gradle.kts"
  (cd "$REPO_ROOT/android" && EMU_SEED_OUT="$FIVE" JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk}" \
    ./gradlew --init-script "$HOME/.cache/emuscenarios.init.gradle.kts" --max-workers=2 -q \
    :app:testDebugUnitTest --tests app.milestone.qa.EmuSeedExportTest --rerun) >"$OUT/logs/seed-gradle.log" 2>&1
  [[ -s "$FIVE" ]]
}
# Background sampler: "Dalvik Heap" / "Native Heap" Heap-Alloc (KB) of the app's
# `dumpsys meminfo` table every ~0.5 s into FILE, until FILE.stop exists.
heap_sampler() { # file
  set +e
  while [[ ! -e "$1.stop" ]]; do
    sh_ dumpsys meminfo "$APP_ID" | awk '
      /^ *Dalvik Heap / {d=$(NF-1)} /^ *Native Heap / {n=$(NF-1)}
      END {if (d != "" || n != "") print d+0, n+0}' >>"$1"
    sleep 0.5
  done
}
# boot_long TAG: cold-start the app on the pushed log 3× and measure; the first
# start runs under the heap sampler.
boot_long() {
  local tag="$1" i t0 t1 ms all="" mem hs="$OUT/logs/heap-${1// /_}.txt" hp=""
  for i in 1 2 3; do
    adb_ shell am force-stop "$APP_ID"; sleep 2
    adb_ logcat -c -b main,system 2>/dev/null || true
    if [[ $i -eq 1 ]]; then rm -f "$hs" "$hs.stop"; heap_sampler "$hs" & hp=$!; fi
    t0=$(date +%s%N)
    launch
    if wait_text pre "TODAY'S SESSION" 90; then
      t1=$(date +%s%N)
      sleep 2; ms="$(displayed_ms)"
      all="$all ${ms:-?}"
      measure longlog "$tag cold start #$i: Displayed (ms)" "${ms:-n/a}$([[ $i -eq 1 ]] && echo ' (heap sampler running)')"
      measure longlog "$tag cold start #$i: launch → Today hero in UI dump (ms, ±2 s dump latency)" "$(( (t1 - t0) / 1000000 ))"
    else
      all="$all FAIL"
      measure longlog "$tag cold start #$i" "Today never rendered in 90 s (focus $(focus))"
    fi
    if [[ -n "$hp" ]]; then touch "$hs.stop"; wait "$hp" 2>/dev/null; hp=""; fi
  done
  sleep 8
  mem="$(meminfo_kb)"
  sh_ dumpsys meminfo "$APP_ID" >"$OUT/logs/meminfo-${tag// /_}.txt"
  read -r -a M <<<"$mem"
  measure longlog "$tag meminfo after settle: TOTAL PSS / Java heap / Native heap (MB)" "$(( ${M[0]:-0} / 1024 )) / $(( ${M[1]:-0} / 1024 )) / $(( ${M[2]:-0} / 1024 ))"
  HEAP_PEAK_MB="$(awk 'BEGIN{d=0;n=0} {if ($1>d) d=$1; if ($2>n) n=$2} END{printf "%d %d %d", d/1024, n/1024, NR}' "$hs" 2>/dev/null)"
  read -r -a M <<<"$HEAP_PEAK_MB"
  measure longlog "$tag peak Heap Alloc during cold start #1: Java (Dalvik) / Native (MB, $((M[2])) samples)" "${M[0]} / ${M[1]}"
  JAVA_PEAK_MB="${M[0]}"
  LAST_STARTS="$all"
}
median3() { tr ' ' '\n' <<<"$1" | grep -E '^[0-9]+$' | sort -n | sed -n '2p'; }
# LongHistorySeed.LONGEST_KM = 34.2 km = 21.25 mi (Auto unit follows the locale).
LONGEST_RX='^(34(\.[0-9]+)? km|21(\.[0-9]+)? mi)$'
today_ok() { tap_nav Today; wait_text pre "TODAY'S SESSION" 60; }
history_longest_ok() {
  LONGEST=""
  tap_nav History; sleep 4
  scroll_to re "$LONGEST_RX" 12 && LONGEST="$(node_label re "$LONGEST_RX")"
}
restart_zygote() {
  adb_ shell stop; sleep 2; adb_ shell start
  local i
  sleep 10
  for i in $(seq 1 90); do
    sh_ service check activity | grep -q found && sh_ pm path android | grep -q package && break
    sleep 2
  done
  sleep 15; wake
}
heap_probe() { sh_ app_process -cp /data/local/tmp/heapprobe.dex /system/bin HeapProbe | sed -n 's/^maxMemoryMB=//p'; }

# backup_cycle LABEL: Export the loaded 5-year log through the picker, Import it
# back (Replace), then check the app restarted onto the same data. A crash in the
# import is reported with the logcat exception line.
backup_cycle() {
  local S=longlog t="${1// /-}" l0 l1 x s1 s2 oom
  l0="$(log_lines)"
  x="$(export_backup)"
  s1="$(sh_ stat -c %s "$x" 2>/dev/null)"; s2="$(sh_ stat -c %s "$APP_FILES/event-log.ndjson")"
  evidence "longlog-$t-export"
  expect $S "$1: export of the 5-year log completes" "exported ${s1:-0} B vs log $s2 B" \
    eval '[[ -n "$s1" && $s1 -gt 0 ]] && python3 -c "import sys;sys.exit(0 if abs($s1-$s2)<=$s2*0.02 else 1)" && app_up'
  adb_ logcat -c -b crash 2>/dev/null || true
  import_backup "$x" "$t"
  sleep 20
  l1="$(log_lines)"
  evidence "longlog-$t-after-import"
  oom="$(sh_ logcat -d -b crash | grep -m1 -E 'OutOfMemoryError|Exception' | cut -c1-220)"
  [[ -n "$oom" ]] && sh_ logcat -d -b crash >"$OUT/logs/crash-import-$t.txt"
  expect $S "$1: import (Replace) of the 5-year backup completes without a crash" \
    "log lines $l0 → $l1; pid '$(app_pid)'; focus $(focus); crash buffer: ${oom:-none}" \
    eval 'app_up && [[ -z "$oom" && $l1 -ge $((l0 * 98 / 100)) ]]'
  app_up || launch
  expect $S "$1: after the import attempt the app boots onto the full log" "log lines $(log_lines) (before $l0)" \
    eval 'wait_text pre "TODAY'"'"'S SESSION" 60 && [[ $(log_lines) -ge $((l0 * 98 / 100)) ]]'
  ls_import="$(sh_ ls -la "$APP_FILES" | grep -c 'event-log.ndjson.import')"
  [[ "$ls_import" == 0 ]] || check INFO $S "$1: leftover temp file" "$(sh_ ls -la "$APP_FILES" | grep import)"
}
sc_longlog() {
  local S=longlog orig lim med s1 s2 l0 l1 x
  gen_five_year || { check FAIL $S "5-year seed" "generation failed, see logs/seed-gradle.log"; return; }
  measure $S "5-year log" "$(tr '\n' ' ' <"$FIVE.facts")"
  javac --release 11 -d "$OUT/probe" "$HELP/HeapProbe.java" &&
    "$(ls -d "$SDK"/build-tools/*/ | sort -V | tail -1)d8" --min-api 26 --output "$OUT/probe" "$OUT/probe/HeapProbe.class" &&
    adb_ push "$OUT/probe/classes.dex" /data/local/tmp/heapprobe.dex >/dev/null
  check INFO $S "default heap limit" "dalvik.vm.heapgrowthlimit $(sh_ getprop dalvik.vm.heapgrowthlimit); ART maxMemory $(heap_probe) MB"
  # Baseline: the small seed's cold start, for scale.
  reset_app seeded >/dev/null
  boot_long "small seed"
  mark longlog-default-heap
  reset_app seeded "$FIVE" >/dev/null
  boot_long "5-year"
  med="$(median3 "$LAST_STARTS")"
  check INFO $S "5-year log cold start (debug APK)" "Displayed ms:$LAST_STARTS (median ${med:-n/a}); debug build on an emulator, not a phone-speed figure"
  evidence longlog-today
  history_longest_ok; expect $S "History shows the 34.2 km longest run" "record '$LONGEST'" test -n "$LONGEST"
  evidence longlog-history
  backup_cycle "default heap"

  mark longlog-128mb
  orig="$(sh_ getprop dalvik.vm.heapgrowthlimit)"
  adb_ shell setprop dalvik.vm.heapgrowthlimit 128m
  restart_zygote
  lim="$(heap_probe)"
  expect $S "zygote restarted with a 128 MB heap growth limit" "getprop $(sh_ getprop dalvik.vm.heapgrowthlimit); ART maxMemory ${lim} MB (HeapProbe via app_process)" test "$lim" = 128
  reset_app seeded "$FIVE" >/dev/null
  boot_long "5-year @128MB"
  expect $S "@128 MB: 5-year log boots to Today (3 cold starts)" "Displayed ms:$LAST_STARTS; peak Java Heap Alloc ${JAVA_PEAK_MB} MB" \
    eval '! grep -q FAIL <<<"$LAST_STARTS"'
  history_longest_ok; expect $S "@128 MB: History renders the 34.2 km longest run" "record '$LONGEST'" test -n "$LONGEST"
  evidence longlog-128mb-history
  backup_cycle "@128 MB"
  adb_ shell rm -f "$APP_FILES/event-log.ndjson.import"
  adb_ shell setprop dalvik.vm.heapgrowthlimit "${orig:-192m}"
  restart_zygote
}

# ── Main ─────────────────────────────────────────────────────────────────────
{
  echo "date:      $(date -Is)"
  echo "serial:    $SERIAL (AVD $AVD)"
  echo "apk:       $APK ($(date -r "$APK" -Is))"
  echo "git:       $(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null)$(git -C "$REPO_ROOT" diff --quiet 2>/dev/null || echo ' (dirty tree)')"
  echo "scenarios: $ONLY"
  echo "clock:     pinned to $PIN_DATE 08:00 $TZ0 per scenario"
} >"$OUT/config.txt"

boot_emulator "$AVD" "$OUT/logs/emulator.log" 2048
trap 'touch "$OUT/feed.stop"; kill ${LOGCAT_PID:-} 2>/dev/null; stop_emulator' EXIT
echo "device:    $(sh_ getprop ro.product.model), API $(sh_ getprop ro.build.version.sdk), heapgrowthlimit $(sh_ getprop dalvik.vm.heapgrowthlimit)" >>"$OUT/config.txt"
adb_ install -r "$APK" >/dev/null || { log "install failed"; exit 1; }
adb_ logcat -c 2>/dev/null || true
adb_ logcat -G 16M >/dev/null 2>&1 || true
adb_ logcat -v threadtime -b main,system,crash,events >"$OUT/logcat.txt" 2>&1 &
LOGCAT_PID=$!

for sc in $ONLY; do
  mark "scenario-$sc"
  if declare -F "sc_$sc" >/dev/null; then
    "sc_$sc"
  else
    check FAIL "$sc" "unknown scenario" "no function sc_$sc"
  fi
  stop_feeder
  shot "end-$sc"
done
baseline_device
sleep 2
kill "$LOGCAT_PID" 2>/dev/null; wait "$LOGCAT_PID" 2>/dev/null

python3 "$HELP/scan_logcat.py" "$OUT/logcat.txt" "$OUT/logcat-scan.json" >/dev/null
python3 - "$OUT/logcat-scan.json" "$OUT/checks.tsv" <<'PY'
import json, sys
s = json.load(open(sys.argv[1]))
out = open(sys.argv[2], "a")
for k, label in (("fatal", "Java FATAL EXCEPTION (not induced)"), ("native", "native crash"), ("anr", "ANR"),
                 ("oom", "OutOfMemoryError"), ("fgs", "foreground-service failure")):
    out.write(f"{'PASS' if s[k] == 0 else 'FAIL'}\tlogcat\t{label}\t{s[k]} occurrence(s) {json.dumps(s['by_phase'].get(k, {}))}\n")
out.write(f"{'PASS' if s['strictmode'] == 0 else 'INFO'}\tlogcat\tStrictMode\t{s['strictmode']} line(s)\n")
out.write(f"INFO\tlogcat\tinduced crashes (am crash)\t{s['induced']}\n")
PY
python3 "$HELP/report.py" "$OUT"
log "report: $OUT/REPORT.md"
! grep -q '^FAIL' "$OUT/checks.tsv"
