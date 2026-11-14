#!/usr/bin/env bash
# `expect ... eval '<check>'` evaluates the check when expect runs it.
# shellcheck disable=SC2016
# Deterministic on-device scenario suite for lollop (app.milestone) on the AVD
# `e34`: system-state changes a host JVM can't reproduce, each with explicit
# PASS/FAIL checks and captured evidence (UI-tree XML, screenshots, event logs).
#
#   locale      per-app locale switches while open and mid-editor (de-DE, ar RTL,
#               ja) and mid-run; the profile's distance unit stays through them
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
#                                 [--regen-seed] [--avd e34] [--port 5580]
#                                 [--apk PATH] [--out DIR]
#
# --only takes scenario names separated by spaces or commas.
# Default APK: the debug build. The shipped release APK carries arm64-v8a alone
# (android/app/build.gradle.kts abiFilters), so on this x86_64 AVD its core runs
# under libndk_translation, where every JNI call returns null ("Core: event
# rejected" + "core view() returned null" in logcat): it can't be measured here.
# For the minified release build, assemble it with the x86_64 core
# (`./gradlew :app:assembleRelease -PqaX86Release=true`), sign it with the debug key,
# uninstall the debug app (no debuggable state carries over) and pass --apk.
#
# GPS is fed with `adb emu geo fix` (scripts/emu-scenarios/feeder.py, 1 Hz,
# constant 3 m/s ellipse). The device clock is pinned to Mon 2026-09-21 08:00
# Europe/Berlin before every scenario. Outputs: ~/.cache/emu-scenarios/ or --out
# (REPORT.md, checks.tsv, observations.tsv, logcat.txt, shots/, ui/, logs/).
# Exit status 0 when no check FAILed. The emulator is stopped at the end unless
# --keep-emulator (or it was already running when the script started).
# Needs: Android SDK (emulator, adb, build-tools d8), JDK (javac; gradle once for
# the 5-year seed, cached in ~/.cache/emu-scenarios/seed/), cargo (the core's
# figures of the time scenario's saved run), python3 + Pillow, and tesseract with
# a Latin-script language (the locale scenario's OCR checks).
# One full run takes ~40 min.
set -uo pipefail

# shellcheck source-path=SCRIPTDIR source=emu-scenarios/lib.sh
source "$(dirname "$0")/emu-scenarios/lib.sh"
OUT="$HOME/.cache/emu-scenarios"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
ONLY="locale time appearance perms power death lowmem update backup longlog"
REGEN=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --only) ONLY="${2//,/ }"; shift 2 ;;
    --keep-emulator) KEEP_EMU=1; shift ;;
    --regen-seed) REGEN=1; shift ;;
    --avd) AVD="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --apk) APK="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    -h|--help) awk 'NR>1&&/^#/{print;next}NR>1{exit}' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
SERIAL="emulator-$PORT"

[[ -f "$APK" ]] || { echo "APK not found: $APK (scripts/build.sh --no-tests)" >&2; exit 2; }

mkdir -p "$OUT"
find "$OUT" -mindepth 1 -maxdepth 1 ! -name seed ! -name probe -exec rm -rf {} +
out_init "$OUT"
mkdir -p "$OUT"/{seed,probe}

# reset_app [LOG]: wipe the app and boot it on LOG (default: the seed_basic log,
# 2 run days, a squat and a 5 km run on Sun SEED_DAY, so Today, Mon RUN_DAY, is an
# Easy run day); succeeds once Today's session is on screen.
reset_app() {
  local f="${1:-$OUT/seed/basic.ndjson}"
  [[ -n "${1:-}" ]] || seed_basic "$SEED_DAY" 2 >"$f" || return 1
  fresh_app "$RUN_DAY" "$f" && wait_text pre "${STR[main_todays_session]}" 40
}
ui_settled() { wait_for 10 app_up; settled dump_ui >/dev/null; }

# The editor's "Logged" time chip ("Today · 8:00 AM"), scrolled into view.
editor_chip() {
  local i l
  for i in 1 2 3 4 5; do
    l="$(node_label pre "${STR[log_today]} · ")"
    [[ -n "$l" ]] && { echo "$l"; return 0; }
    swipe_content
  done
  return 1
}

resumed_and_recording() { [[ "$(sidecar_points)" -gt "$1" ]] && svc_fg; }

set_locale() { sh_ cmd locale set-app-locales "$APP_ID" ${1:+--locales "$1"} >/dev/null; ui_settled; }
# A figure and its unit: the app keeps them together with a no-break space.
FIG_UNIT='^[0-9.]+[ \u00a0]'
plan_unit() { local l; l="$(node_label re "${FIG_UNIT}(mi|km)\$")"; echo "${l##*[!a-z]}"; }
# set_distance_unit mi|km: Profile › Distance units, then back to Today.
set_distance_unit() {
  tap_nav "${STR[main_dest_profile]}"
  scroll_to eq "${STR[main_label_distance_units]}" 6 && tap_node eq "${STR[main_unit_$1]}" && wait_for 5 selected "${STR[main_unit_$1]}"
  tap_nav "${STR[main_dest_today]}"
}
# ocr_check SCENARIO NAME EVIDENCE-TAG MODE TEXT: the rendered (OCR) text of the
# node equals its logical text (whitespace-collapsed).
ocr_check() {
  local r rc
  r="$(python3 "$HELP/ocr.py" "$OUT/shots/$3.png" "$OUT/ui/$3.xml" "$4" "$5" "$OUT/logs" 2>/dev/null)"; rc=$?
  if [[ $rc -eq 3 ]]; then check FAIL "$1" "$2" "tesseract with a Latin-script language not installed"; return; fi
  local logical="${r%%$'\t'*}" seen="${r#*$'\t'}"
  expect "$1" "$2" "tree text '$logical' · rendered (OCR of $3.png) '$seen'" test -n "$r" -a "$logical" = "$seen"
}
# no_overflow SCENARIO CONFIG: no text node of the screen lies outside it.
no_overflow() {
  local x; x="$(dump_ui | q overflow | head -5 | tr '\n' ';')"
  expect "$1" "$2: no text node outside the screen" "${x:-none}" test -z "$x"
}

sc_locale() {
  local S=locale u pid0 W rr p n0 n1 x
  reset_app || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  # The distance unit is the profile's: miles here, so that a unit that followed
  # a locale (de-DE's km) would show.
  set_distance_unit mi
  evidence locale-01-en-today
  u="$(plan_unit)"
  expect $S "the profile's distance unit (mi) shows" "plan distance unit '$u'" test "$u" = mi
  pid0="$(app_pid)"
  set_locale de-DE
  evidence locale-02-de-today
  ocr_check $S "de-DE: plan distance renders as written (OCR control)" locale-02-de-today re "${FIG_UNIT}(km|mi)\$"
  expect $S "de-DE applied while open, same process" "pid $pid0 → $(app_pid), focus $(focus)" \
    test "$(app_pid)" = "$pid0" -a -n "$pid0"
  u="$(plan_unit)"
  expect $S "de-DE: the profile's distance unit stays (mi)" "plan distance unit '$u'" test "$u" = mi

  mark locale-mid-editor
  set_locale en-US
  { open_set_editor && editor_fill; } || check FAIL $S "open set editor" "editor controls not found"
  evidence locale-03-en-editor
  set_locale de-DE
  evidence locale-04-de-editor
  expect $S "editor survives a de-DE switch with its values" "122.5 kg · 5 reps · 2 left still selected" editor_has_values 122.5
  observe $S "de-DE weight rendering" "$(node_label pre "${STR[log_field_weight]}, ") · $(node_label re '^[0-9]+([.,][0-9]+)?$')"
  tap_log_set
  pull_log "$OUT/logs/locale-de.ndjson"
  x="$(EV lastset "$OUT/logs/locale-de.ndjson")"
  expect $S "set logged under de-DE keeps the decimal" "last LogSet $x" test "$x" = "Back Squat|122.5|5|8.0"

  mark locale-ar
  set_locale ar
  evidence locale-05-ar-editor
  expect $S "editor survives an ar (RTL) switch" "after logging, '${STR[log_set_done]}' still on screen" has_text eq "${STR[log_set_done]}"
  close_editor
  evidence locale-06-ar-today
  # The app ships English strings only but declares supportsRtl: in an RTL locale
  # an English value must still read left-to-right ("5.5 km", not "km 5.5").
  ocr_check $S "ar: English plan distance renders left-to-right" locale-06-ar-today re "${FIG_UNIT}(km|mi)\$"
  observe $S "ar: coach-note paragraph as rendered (tree|OCR)" "$(python3 "$HELP/ocr.py" "$OUT/shots/locale-06-ar-today.png" "$OUT/ui/locale-06-ar-today.xml" re '^[A-Z].*\.$' "$OUT/logs" 2>/dev/null | tr '\t' '|')"
  read -r W _ <<<"$(screen_size)"
  rr="$(find_node eq "${STR[main_dest_today]}")"; read -r -a p <<<"${rr:-0 0}"
  expect $S "ar: bottom navigation mirrored (Today on the right)" "Today nav centre x=${p[0]} of $W" test "${p[0]}" -gt $((W / 2))
  rr="$(find_node eq "${STR[main_log]}")"; read -r -a p <<<"${rr:-9999 0}"
  expect $S "ar: Log FAB mirrored to the left" "Log FAB centre x=${p[0]} of $W" test "${p[0]}" -lt $((W / 2))
  no_overflow $S ar

  mark locale-ja
  set_locale ja-JP
  expect $S "ja-JP applied while open" "focus $(focus)" app_up
  evidence locale-07-ja-today
  tap_nav "${STR[main_dest_history]}"; evidence locale-08-ja-history
  tap_nav "${STR[main_dest_profile]}"; evidence locale-09-ja-profile
  no_overflow $S ja
  tap_nav "${STR[main_dest_today]}"

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
  expect $S "locale reset to the system's: the profile's distance unit stays (mi)" "plan distance unit '$u'" test "$u" = mi
}

sc_time() {
  local S=time chip x pid0 t0 t1 rec ms facts wall rs
  reset_app || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  adb_ shell settings put system time_12_24 24
  open_set_editor; chip="$(editor_chip)"; evidence time-01-24h-editor
  expect $S "24 h setting: editor time chip" "'$chip'" eval '[[ "$chip" =~ ^"${STR[log_today]}"\ ·\ [0-9]{2}:[0-9]{2}$ ]]'
  close_editor
  adb_ shell settings put system time_12_24 12
  open_set_editor; chip="$(editor_chip)"; evidence time-02-12h-editor
  expect $S "12 h setting: editor time chip" "'$chip'" eval '[[ "$chip" =~ ^"${STR[log_today]}"\ ·\ [0-9]{1,2}:[0-9]{2}[^0-9]*[AP]M$ ]]'
  close_editor

  mark time-clock-jump
  pin_clock "$RUN_DAY" 13:30
  open_set_editor; chip="$(editor_chip)"; evidence time-03-clock-1330
  expect $S "clock set to 13:30 while open: new editor stamps 1:3x PM" "'$chip'" eval '[[ "$chip" =~ 1:3[0-9][^0-9]*PM$ ]]'
  close_editor

  mark time-tz-background
  pin_clock "$RUN_DAY" 23:30
  home
  sh_ cmd alarm set-timezone Asia/Tokyo >/dev/null   # Tue 06:30 local
  launch; ui_settled
  evidence time-04-tokyo-resumed
  x="$(node_label pre "${STR[main_a11y_today_prefix]}")"
  expect $S "TZ Berlin 23:30 → Tokyo (Tue 06:30) while backgrounded: Today moves to Tuesday" "week strip '$x'" \
    eval '[[ "$x" == "${STR[main_a11y_today_prefix]}Tuesday"* ]]'
  mark time-tz-foreground
  sh_ cmd alarm set-timezone "$TZ0" >/dev/null       # back to Mon 23:3x, app in front
  ui_settled
  evidence time-05-berlin-foreground
  x="$(node_label pre "${STR[main_a11y_today_prefix]}")"
  observe $S "TZ Tokyo → Berlin while foregrounded (no resume): week strip" "'$x' (device local $(sh_ "date '+%a %H:%M'"))"
  home; launch; ui_settled
  x="$(node_label pre "${STR[main_a11y_today_prefix]}")"
  expect $S "after a resume the strip follows the device day again" "week strip '$x'" \
    eval '[[ "$x" == "${STR[main_a11y_today_prefix]}Monday"* ]]'

  mark time-mid-run
  pin_clock "$RUN_DAY"
  tap_nav "${STR[main_dest_today]}"
  start_feeder time
  if start_run; then
    t0=$(date +%s); pid0="$(app_pid)"
    sleep 25
    sh_ cmd alarm set-timezone Asia/Tokyo >/dev/null
    sleep 10
    sh_ cmd alarm set-time "$(( ($(dev_now) + 3600) * 1000 ))" >/dev/null
    sleep 25
    evidence time-06-mid-run-after-jump
    tap_node eq "${STR[track_btn_stop]}"; t1=$(date +%s); rec=$((t1 - t0))
    wait_text eq "${STR[track_btn_save_run]}" 15
    finish_run || check FAIL $S "finish run" "Save run flow failed"
    evidence time-07-after-save
    ms="$(duration "${FINISH#* · }")"
    expect $S "TZ change + clock +1 h mid-run: finish-sheet moving time ≈ recorded time" \
      "'$FINISH' → ${ms}s vs ${rec}s Start→Stop on the host" eval '[[ $ms -ge 0 && $ms -le $((rec + 5)) && $ms -ge $((rec - 25)) ]]'
    pull_log "$OUT/logs/time-run.ndjson"
    facts="$(EV track "$OUT/logs/time-run.ndjson")"
    wall="$(python3 -c "import json,sys;f=json.loads(sys.argv[1]);print(f['last_at']-f['first_at'])" "$facts")"
    # The core's of-record moving time of the saved run (on cargo failure the
    # check FAILs with logs/time-run-core-cargo.log).
    core="$OUT/logs/time-run-core.json"
    (cd "$REPO_ROOT" && cargo run -q -p shared --example track_figures -- "$OUT/logs/time-run.ndjson" "$core") \
      >"$OUT/logs/time-run-core-cargo.log" 2>&1 || rm -f "$core"
    core_s="$( [[ -f "$core" ]] && jfield moving_s <"$core")"
    expect $S "saved run: one track, the core's moving seconds from its fixes ≈ recorded" "$facts; core moving_s ${core_s:-n/a (logs/time-run-core-cargo.log)}" \
      python3 -c "import json,sys;f=json.loads(sys.argv[1]);sys.exit(0 if f['tracks']==1 and sys.argv[2] and abs(float(sys.argv[2])-$rec)<=30 else 1)" "$facts" "$core_s"
    observe $S "saved run first→last fix span (includes the +1 h wall-clock jump)" "${wall}s; process $pid0 → $(app_pid)"
    tap_nav "${STR[main_dest_history]}"; scroll_to re "${FIG_UNIT}(mi|km) · " 6; evidence time-08-history
    x="$(node_label re "${FIG_UNIT}(mi|km) · ")"
    rs="$(duration "${x##* · }")"
    expect $S "History row moving time equals the finish sheet" "row '$x' (${rs}s) vs finish '$FINISH' (${ms}s)" \
      eval '[[ $ms -ge 0 && "$rs" == "$ms" ]]'
  else
    check FAIL $S "start tracked run" "tracker Start not reachable"
  fi
  stop_feeder
  pin_clock "$RUN_DAY"
}

screen_lum() { adb_ exec-out screencap | python3 "$HELP/lum.py" | cut -d' ' -f1; }
lum_below() { [[ "$(screen_lum)" -lt "$1" ]]; }
lum_above() { [[ "$(screen_lum)" -gt "$1" ]]; }
# The set editor still works end-to-end: open, +2.5, Log set → one more LogSet.
editor_logs_a_set() { # tag
  local r p H ok=1
  if open_set_editor && tap_scroll eq "$(fmt ui_cd_adjust_add 2.5 kg)"; then
    evidence "$1-editor"
    read -r _ H <<<"$(screen_size)"
    if r="$(find_node re "$LOG_SET_RX")"; then
      read -r -a p <<<"$r"
      if [[ ${p[5]} -le $H && ${p[4]} -gt ${p[2]} ]]; then
        tap_log_set && ok=0
        pull_log "$OUT/logs/$1-after.ndjson"
      fi
    fi
  else
    evidence "$1-editor-failed"
  fi
  close_editor
  [[ $ok -eq 0 ]]
}

sc_appearance() {
  local S=appearance l fs d
  reset_app || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  l="$(screen_lum)"; evidence appearance-01-light
  expect $S "light system theme renders light (Theme mode System)" "mean luma $l" test "$l" -gt 150
  open_set_editor && editor_fill
  adb_ shell cmd uimode night yes >/dev/null
  wait_for 10 lum_below 90
  l="$(screen_lum)"; evidence appearance-02-dark-editor
  expect $S "dark toggle mid-editor: dark rendering" "mean luma $l" test "$l" -lt 90
  expect $S "dark toggle mid-editor keeps the editor values" "122.5 kg · 5 · 2 left" editor_has_values 122.5
  adb_ shell cmd uimode night no >/dev/null
  wait_for 10 lum_above 150
  evidence appearance-03-light-editor
  expect $S "light toggle back mid-editor keeps the editor values" "122.5 kg · 5 · 2 left" editor_has_values 122.5
  close_editor
  for fs in 0.85 1.3 2.0; do
    mark "appearance-font-$fs"
    adb_ shell settings put system font_scale "$fs"; ui_settled
    evidence "appearance-font-$fs-today"
    expect $S "font scale $fs: app up" "focus $(focus)" app_up
    expect $S "font scale $fs: set editor logs a set" "Log set button on screen and tappable" editor_logs_a_set "font-$fs"
    tap_nav "${STR[main_dest_profile]}"
    expect $S "font scale $fs: Profile › Export reachable" "scrolled to '${STR[profile_export_data]}'" scroll_to eq "${STR[profile_export_data]}" 10
    evidence "appearance-font-$fs-profile"
    tap_nav "${STR[main_dest_today]}"
  done
  adb_ shell settings put system font_scale 1.0
  for d in 560 320; do
    mark "appearance-density-$d"
    adb_ shell wm density "$d"; ui_settled
    evidence "appearance-density-$d-today"
    expect $S "display density $d: set editor logs a set" "Log set button on screen and tappable" editor_logs_a_set "density-$d"
  done
  mark appearance-worst
  adb_ shell wm density 560; adb_ shell settings put system font_scale 2.0; ui_settled
  evidence appearance-worst-today
  expect $S "font 2.0 + density 560: set editor logs a set" "Log set button on screen and tappable" editor_logs_a_set worst
  no_overflow $S "font 2.0 + density 560"
  adb_ shell wm density reset; adb_ shell settings put system font_scale 1.0
}

sc_perms() {
  local S=perms n0 n1 x
  reset_app || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  start_feeder perms
  if ! start_run; then check FAIL $S "start tracked run" "tracker Start not reachable"; stop_feeder; return; fi
  sleep 20; n0="$(sidecar_points)"
  mark perms-revoke-location
  stop_app revoke ACCESS_FINE_LOCATION ACCESS_COARSE_LOCATION &&
    observe $S "revoking location kills the process (platform)" "pid gone"
  launch
  expect $S "relaunch after revoke offers to resume the run" "'${STR[main_resume_run_title]}' dialog" wait_text eq "${STR[main_resume_run_title]}" 20
  evidence perms-01-resume-prompt
  tap_node eq "${STR[track_action_resume]}"
  wait_text eq "${STR[track_btn_continue]}" 10
  evidence perms-02-after-resume
  expect $S "resume without location shows the permission rationale (no crash)" "focus $(focus)" \
    eval 'app_up && has_text eq "${STR[track_btn_continue]}"'
  tap_node eq "${STR[track_btn_continue]}"
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
  tap_nav "${STR[main_dest_today]}"
  if ! start_run; then check FAIL $S "start second run" "tracker Start not reachable"; stop_feeder; return; fi
  sleep 15; n0="$(sidecar_points)"
  stop_app revoke POST_NOTIFICATIONS && observe $S "revoking notifications kills the process (platform)" "pid gone"
  launch
  expect $S "relaunch after notification revoke offers to resume" "'${STR[main_resume_run_title]}'" wait_text eq "${STR[main_resume_run_title]}" 20
  tap_node eq "${STR[track_action_resume]}"; sleep 15
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
  reset_app || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  adb_ shell dumpsys battery unplug
  adb_ shell settings put global low_power 1
  sh_ dumpsys power | grep -iE "battery ?saver|mSettingBatterySaver|low_power" >"$OUT/logs/power-state.txt"
  expect $S "battery saver on" "settings low_power=$(sh_ settings get global low_power); $(grep -m2 -i 'saver' "$OUT/logs/power-state.txt" | tr -s ' ' | tr '\n' ';')" \
    test "$(sh_ settings get global low_power)" = 1
  stop_app; launch
  expect $S "cold launch under battery saver renders Today" "Today hero within 30 s" wait_text pre "${STR[main_todays_session]}" 30
  evidence power-01-today
  start_feeder power
  if start_run; then
    sleep 15; n0="$(sidecar_points)"
    # 45 s with the screen off.
    off="$(dev_now)"; adb_ shell input keyevent 223; sleep 45
    on="$(dev_now)"; wake; launch
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
  adb_ shell dumpsys battery reset
}

sc_death() {
  local S=death n0 x l0 l1 pid0 tc facts
  reset_app || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  mark death-editor-kill
  open_set_editor && editor_fill
  home
  stop_app kill
  expect $S "am kill ends the backgrounded app's process" "pid '$(app_pid)'" test -z "$(app_pid)"
  launch; wait_text eq "${STR[log_field_exercise]}" 15
  evidence death-01-editor-restored
  expect $S "set editor restored with its values after process death" "122.5 kg · 5 · 2 left" editor_has_values 122.5
  close_editor

  mark death-editor-crash
  l0="$(log_lines)"
  open_set_editor && editor_fill
  pid0="$(app_pid)"
  stop_app crash
  launch
  l1="$(log_lines)"
  evidence death-02-after-crash
  expect $S "relaunch after am crash mid-editor" "focus $(focus)" app_up
  pull_log "$OUT/logs/death-crash.ndjson"
  expect $S "event log intact after the crash" "lines $l0 → $l1; unparsable: $(EV count "$OUT/logs/death-crash.ndjson" __unparsable__)" \
    eval '[[ $l1 -ge $l0 && "$(EV count "$OUT/logs/death-crash.ndjson" __unparsable__)" == 0 ]]'
  close_editor

  mark death-run-am-kill
  tap_nav "${STR[main_dest_today]}"
  start_feeder death
  if ! start_run; then check FAIL $S "start tracked run" "tracker Start not reachable"; stop_feeder; return; fi
  sleep 15; pid0="$(app_pid)"
  adb_ shell am kill "$APP_ID"; sleep 3
  expect $S "am kill mid-run: foreground service keeps the process" "pid $pid0 → $(app_pid); fg $(svc_fg && echo yes || echo no)" \
    eval '[[ "$(app_pid)" == "$pid0" ]] && svc_fg'

  mark death-run-am-crash
  n0="$(sidecar_points)"; tc="$(dev_now)"; pid0="$(app_pid)"
  stop_app crash
  launch
  expect $S "relaunch after am crash mid-run offers to resume" "'${STR[main_resume_run_title]}'" wait_text eq "${STR[main_resume_run_title]}" 20
  evidence death-03-resume-prompt
  tap_node eq "${STR[track_action_resume]}"
  expect $S "resumed run records again after am crash" "sidecar from $n0" wait_for 15 resumed_and_recording "$n0"
  finish_run || check FAIL $S "finish run" "Save run flow failed"
  pull_log "$OUT/logs/death-run1.ndjson"
  facts="$(EV track "$OUT/logs/death-run1.ndjson")"
  expect $S "saved run spans the crash (fixes before and after)" "$facts; crash at $tc" \
    python3 -c "import json,sys;f=json.loads(sys.argv[1]);sys.exit(0 if f.get('tracks')==1 and f['first_at']<$tc<f['last_at'] else 1)" "$facts"

  mark death-run-sigkill
  tap_nav "${STR[main_dest_today]}"
  if start_run; then
    sleep 15; n0="$(sidecar_points)"; pid0="$(app_pid)"
    stop_app sigkill
    expect $S "SIGKILL mid-run (low-memory-killer stand-in) ends the process" "pid $pid0 → '$(app_pid)'" pid_released "$pid0"
    launch
    expect $S "relaunch after SIGKILL mid-run offers to resume" "'${STR[main_resume_run_title]}'" wait_text eq "${STR[main_resume_run_title]}" 20
    tap_node eq "${STR[track_action_resume]}"
    expect $S "resumed run records again after SIGKILL" "sidecar from $n0" wait_for 12 resumed_and_recording "$n0"
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
  reset_app || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  pid0="$(app_pid)"
  r="$(sh_ am send-trim-memory "$APP_ID" RUNNING_CRITICAL)"
  sleep 3; evidence lowmem-01-fg-critical
  expect $S "RUNNING_CRITICAL in foreground: app keeps running" "'$r'; pid $pid0 → $(app_pid)" eval 'app_up && [[ "$(app_pid)" == "$pid0" ]]'
  open_set_editor && editor_fill
  home
  r="$(sh_ am send-trim-memory "$APP_ID" COMPLETE)"
  launch; wait_text eq "${STR[log_field_exercise]}" 15
  evidence lowmem-02-bg-complete
  expect $S "TRIM_MEMORY_COMPLETE in background: editor values kept" "'$r'" editor_has_values 122.5
  close_editor
  tap_nav "${STR[main_dest_today]}"
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
  local S=update m0 m1 u r x ok
  reset_app || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  # Kilometers: the seed's profile states no unit, so its first boot gives it the
  # en-US locale's, mi; a reset to the locale's would show.
  set_distance_unit km
  open_set_editor && editor_fill && tap_log_set; close_editor
  pull_log "$OUT/logs/update-before.ndjson"; m0="$(md5sum <"$OUT/logs/update-before.ndjson" | cut -c1-12)"
  r="$(install_apk "$APK" -r)"; ok=$?
  expect $S "install -r over the running app" "'$r'; pid after: '$(app_pid)'" test "$ok" -eq 0
  pull_log "$OUT/logs/update-after.ndjson"; m1="$(md5sum <"$OUT/logs/update-after.ndjson" | cut -c1-12)"
  expect $S "event log byte-identical across the update" "md5 $m0 → $m1 ($(grep -c . "$OUT/logs/update-after.ndjson") lines)" test "$m0" = "$m1"
  launch
  expect $S "app boots after the update" "Today rendered" wait_text pre "${STR[main_todays_session]}" 30
  u="$(plan_unit)"; evidence update-01-after
  expect $S "settings survive the update (distance unit km)" "plan distance unit '$u'" test "$u" = km

  mark update-mid-run
  start_feeder update
  if start_run; then
    sleep 15
    r="$(install_apk "$APK" -r)"
    launch
    expect $S "relaunch after an update mid-run offers to resume" "'$r'" wait_text eq "${STR[main_resume_run_title]}" 20
    evidence update-02-resume-prompt
    tap_node eq "${STR[track_action_resume]}"
    expect $S "run resumes after the update" "fg $(svc_fg && echo yes || echo no)" wait_for 12 svc_fg
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
  local name="lollop-$RUN_DAY.ndjson" i s0=-1 s1
  adb_ shell "rm -f /sdcard/Download/lollop-*.ndjson"
  tap_nav "${STR[main_dest_profile]}"
  scroll_to eq "${STR[profile_export_data]}" 10 || return 1
  tap_node eq "${STR[profile_export_data]}"
  wait_text eq SAVE 15 || return 1
  has_text eq "$name" || log "picker suggests $(node_label pre lollop-)"
  tap_top eq SAVE
  # A large export streams for a while: wait for the file size to settle.
  for i in $(seq 1 60); do
    sleep 2
    s1="$(sh_ stat -c %s "/sdcard/Download/$name")"
    [[ -n "$s1" && "$s1" == "$s0" && "$(focus)" == "$APP_ID"* ]] && break
    s0="$s1"
  done
  echo "/sdcard/Download/$name"
}
import_done() { app_up && ! adb_ shell "[ -f $APP_FILES/$EVENT_LOG_FILE.import ]"; }
# Profile › Import my data → pick FILE in Downloads → Replace; succeeds once the
# import has finished (its temp file is gone, the app is up).
import_backup() {
  local name; name="$(basename "$1")"
  tap_nav "${STR[main_dest_profile]}"
  scroll_to eq "${STR[profile_import_data]}" 10 || return 1
  tap_node eq "${STR[profile_import_data]}"
  wait_text eq "$name" 15 || return 1
  tap_node eq "$name"
  wait_text eq "${STR[main_import_replace_confirm]}" 60 || return 1
  evidence "import-confirm-$2"
  tap_node eq "${STR[main_import_replace_confirm]}"
  wait_gone eq "${STR[main_import_replace_confirm]}" 10 && wait_for 60 import_done
}

sc_backup() {
  local S=backup f
  reset_app || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  open_set_editor && editor_fill && tap_log_set; close_editor
  pull_log "$OUT/logs/backup-device.ndjson"
  EV canon "$OUT/logs/backup-device.ndjson" >"$OUT/logs/backup-device.canon"
  f="$(export_backup)"
  evidence backup-01-after-export
  adb_ pull "$f" "$OUT/logs/backup-exported.ndjson" >/dev/null || log "no exported file $f"
  EV canon "$OUT/logs/backup-exported.ndjson" >"$OUT/logs/backup-exported.canon"
  expect $S "export writes every logged event" "$(grep -c . "$OUT/logs/backup-exported.canon") exported vs $(grep -c . "$OUT/logs/backup-device.canon") on device" \
    cmp -s "$OUT/logs/backup-device.canon" "$OUT/logs/backup-exported.canon"
  mark backup-clear
  stop_app clear
  grant_perms; pin_clock "$RUN_DAY"; launch
  if wait_text eq "${STR[setup_skip]}" 15; then tap_node eq "${STR[setup_skip]}"; wait_gone eq "${STR[setup_skip]}" 10; fi
  expect $S "cleared app starts empty" "log lines $(log_lines)" test "$(log_lines)" -le 1
  import_backup "$f" clear
  evidence backup-02-after-import
  pull_log "$OUT/logs/backup-imported.ndjson"
  EV canon "$OUT/logs/backup-imported.ndjson" >"$OUT/logs/backup-imported.canon"
  expect $S "import restores exactly the exported events" "diff: $(diff "$OUT/logs/backup-device.canon" "$OUT/logs/backup-imported.canon" | head -3 | tr '\n' ' ')" \
    cmp -s "$OUT/logs/backup-device.canon" "$OUT/logs/backup-imported.canon"
  tap_nav "${STR[main_dest_today]}"
  expect $S "imported data drives Today again" "Today's session rendered" wait_text pre "${STR[main_todays_session]}" 15
}

FIVE="$OUT/seed/five-year.ndjson"
gen_five_year() {
  [[ -s "$FIVE" && $REGEN -eq 0 ]] && grep -q '^longest_run_km=' "$FIVE.facts" 2>/dev/null && return 0
  log "generating the 5-year LongHistorySeed log (gradle :lollop:exportLongHistorySeed)"
  gradle_build_dir_init "$OUT/logs/seed.init.gradle.kts" "$HOME/.cache/emuscenarios-build"
  (cd "$REPO_ROOT/android" &&
    ./gradlew --init-script "$OUT/logs/seed.init.gradle.kts" --max-workers="$GRADLE_MAX_WORKERS" -q \
    :lollop:exportLongHistorySeed --args="$FIVE") >"$OUT/logs/seed-gradle.log" 2>&1
  [[ -s "$FIVE" ]]
}
# Background sampler: "Dalvik Heap" / "Native Heap" Heap-Alloc (KB) of the app's
# `dumpsys meminfo` table every ~0.5 s into FILE, until FILE.stop exists.
heap_sampler() { # file
  local m
  while [[ ! -e "$1.stop" ]]; do
    m="$(meminfo dalvik_alloc native_alloc)" && echo "$m" >>"$1"
    sleep 0.5
  done
}
_displayed() { displayed_ms >/dev/null; }
# boot_long TAG: cold-start the app on the pushed log 3× and measure; the first
# start runs under the heap sampler.
boot_long() {
  local tag="$1" i t0 t1 ms all="" hs="$OUT/logs/heap-${1// /_}.txt" hp="" M
  for i in 1 2 3; do
    stop_app
    adb_ logcat -c -b main,system || log "logcat -c failed"
    if [[ $i -eq 1 ]]; then rm -f "$hs" "$hs.stop"; heap_sampler "$hs" & hp=$!; fi
    t0=$(date +%s%N)
    launch
    if wait_text pre "${STR[main_todays_session]}" 90; then
      t1=$(date +%s%N)
      wait_for 5 _displayed; ms="$(displayed_ms)"
      all="$all ${ms:-?}"
      observe longlog "$tag cold start #$i: Displayed (ms)" "${ms:-n/a}$([[ $i -eq 1 ]] && echo ' (heap sampler running)')"
      observe longlog "$tag cold start #$i: launch → Today hero in UI dump (ms, ±2 s dump latency)" "$(( (t1 - t0) / 1000000 ))"
    else
      all="$all FAIL"
      observe longlog "$tag cold start #$i" "Today never rendered in 90 s (focus $(focus))"
    fi
    if [[ -n "$hp" ]]; then touch "$hs.stop"; wait "$hp"; hp=""; fi
  done
  sleep 8
  sh_ dumpsys meminfo "$APP_ID" >"$OUT/logs/meminfo-${tag// /_}.txt"
  read -r -a M <<<"$(python3 "$HELP/parse.py" meminfo pss java native <"$OUT/logs/meminfo-${tag// /_}.txt")"
  observe longlog "$tag meminfo after settle: TOTAL PSS / Java heap / Native heap (MB)" "$(( ${M[0]:-0} / 1024 )) / $(( ${M[1]:-0} / 1024 )) / $(( ${M[2]:-0} / 1024 ))"
  read -r -a M <<<"$(awk 'BEGIN{d=0;n=0} {if ($1>d) d=$1; if ($2>n) n=$2} END{printf "%d %d %d", d/1024, n/1024, NR}' "$hs")"
  observe longlog "$tag peak Heap Alloc during cold start #1: Java (Dalvik) / Native (MB, $((M[2])) samples)" "${M[0]} / ${M[1]}"
  JAVA_PEAK_MB="${M[0]}"
  LAST_STARTS="$all"
}
median3() { tr ' ' '\n' <<<"$1" | grep -E '^[0-9]+$' | sort -n | sed -n '2p'; }
# The longest run of the 5-year log as History shows it (the seed's
# longest_run_km fact: a tracked run): its whole km or mi (whichever unit the
# profile states; one that states none takes the locale's at boot).
longest_rx() {
  local km; km="$(sed -n 's/^longest_run_km=//p' "$FIVE.facts")"
  python3 -c 'import sys;km=float(sys.argv[1]);print(f"^({int(km)}(\\.[0-9]+)?[ \\u00a0]km|{int(km / 1.609344)}(\\.[0-9]+)?[ \\u00a0]mi)$")' "$km"
}
history_longest_ok() {
  local rx; rx="$(longest_rx)"
  LONGEST=""
  tap_nav "${STR[main_dest_history]}"
  scroll_to re "$rx" 12 && LONGEST="$(node_label re "$rx")"
}
framework_up() { sh_ service check activity | grep ': found' >/dev/null && sh_ pm path android | grep package >/dev/null; }
framework_down() { ! sh_ service check activity | grep ': found' >/dev/null; }
restart_zygote() {
  adb_ shell stop
  wait_for 30 framework_down || log "restart_zygote: framework still up after stop"
  adb_ shell start
  wait_for 180 framework_up || die "framework not back after a zygote restart"
  wake
  wait_for 60 launcher_up || log "restart_zygote: launcher not in front ($(focus))"
}
heap_probe() { sh_ app_process -cp /data/local/tmp/heapprobe.dex /system/bin HeapProbe | sed -n 's/^maxMemoryMB=//p'; }

# backup_cycle LABEL: Export the loaded 5-year log through the picker, Import it
# back (Replace), then check the app restarted onto the same data. A crash in the
# import is reported with the logcat exception line.
backup_cycle() {
  local S=longlog t="${1// /-}" l0 l1 x s1 s2 oom
  l0="$(log_lines)"
  x="$(export_backup)"
  s1="$(sh_ stat -c %s "$x")"; s2="$(sh_ stat -c %s "$APP_FILES/$EVENT_LOG_FILE")"
  evidence "longlog-$t-export"
  expect $S "$1: export of the 5-year log completes" "exported ${s1:-0} B vs log $s2 B" \
    eval '[[ -n "$s1" && $s1 -gt 0 ]] && python3 -c "import sys;sys.exit(0 if abs($s1-$s2)<=$s2*0.02 else 1)" && app_up'
  adb_ logcat -c -b crash || log "logcat -c -b crash failed"
  import_backup "$x" "$t"
  l1="$(log_lines)"
  evidence "longlog-$t-after-import"
  oom="$(sh_ logcat -d -b crash | grep -m1 -E 'OutOfMemoryError|Exception' | cut -c1-220)"
  [[ -n "$oom" ]] && sh_ logcat -d -b crash >"$OUT/logs/crash-import-$t.txt"
  expect $S "$1: import (Replace) of the 5-year backup completes without a crash" \
    "log lines $l0 → $l1; pid '$(app_pid)'; focus $(focus); crash buffer: ${oom:-none}" \
    eval 'app_up && [[ -z "$oom" && $l1 -ge $((l0 * 98 / 100)) ]]'
  app_up || launch
  expect $S "$1: after the import attempt the app boots onto the full log" "log lines $(log_lines) (before $l0)" \
    eval 'wait_text pre "${STR[main_todays_session]}" 60 && [[ $(log_lines) -ge $((l0 * 98 / 100)) ]]'
  expect $S "$1: the import leaves no temp file" "$(sh_ ls -la "$APP_FILES" | grep import)" \
    eval '[[ "$(sh_ ls -la "$APP_FILES" | grep -c "$EVENT_LOG_FILE.import")" == 0 ]]'
}
sc_longlog() {
  local S=longlog orig lim med
  gen_five_year || { check FAIL $S "5-year seed" "generation failed, see logs/seed-gradle.log"; return; }
  observe $S "5-year log" "$(tr '\n' ' ' <"$FIVE.facts")"
  { javac --release 11 -d "$OUT/probe" "$HELP/HeapProbe.java" &&
    "$(build_tool d8)" --min-api 26 --output "$OUT/probe" "$OUT/probe/HeapProbe.class" &&
    adb_ push "$OUT/probe/classes.dex" /data/local/tmp/heapprobe.dex >/dev/null; } ||
    check FAIL $S "HeapProbe built and pushed" "javac / d8 / adb push failed"
  observe $S "default heap limit" "dalvik.vm.heapgrowthlimit $(sh_ getprop dalvik.vm.heapgrowthlimit); ART maxMemory $(heap_probe) MB"
  # Baseline: the small seed's cold start, for scale.
  reset_app >/dev/null
  boot_long "small seed"
  mark longlog-default-heap
  reset_app "$FIVE" >/dev/null
  boot_long "5-year"
  med="$(median3 "$LAST_STARTS")"
  expect $S "5-year log boots to Today (3 cold starts)" "Displayed ms:$LAST_STARTS (median ${med:-n/a}; debug APK on an emulator, not a phone-speed figure)" \
    eval '! grep -q FAIL <<<"$LAST_STARTS"'
  evidence longlog-today
  history_longest_ok; expect $S "History shows the 5-year log's longest run" "record '$LONGEST'" test -n "$LONGEST"
  evidence longlog-history
  backup_cycle "default heap"

  mark longlog-128mb
  orig="$(sh_ getprop dalvik.vm.heapgrowthlimit)"
  adb_ shell setprop dalvik.vm.heapgrowthlimit 128m
  restart_zygote
  lim="$(heap_probe)"
  expect $S "zygote restarted with a 128 MB heap growth limit" "getprop $(sh_ getprop dalvik.vm.heapgrowthlimit); ART maxMemory ${lim} MB (HeapProbe via app_process)" test "$lim" = 128
  reset_app "$FIVE" >/dev/null
  boot_long "5-year @128MB"
  expect $S "@128 MB: 5-year log boots to Today (3 cold starts)" "Displayed ms:$LAST_STARTS; peak Java Heap Alloc ${JAVA_PEAK_MB} MB" \
    eval '! grep -q FAIL <<<"$LAST_STARTS"'
  history_longest_ok; expect $S "@128 MB: History renders the longest run" "record '$LONGEST'" test -n "$LONGEST"
  evidence longlog-128mb-history
  backup_cycle "@128 MB"
  adb_ shell rm -f "$APP_FILES/$EVENT_LOG_FILE.import"
  adb_ shell setprop dalvik.vm.heapgrowthlimit "${orig:-192m}"
  restart_zygote
}

trap session_cleanup EXIT
config_run
config apk "$APK ($(date -r "$APK" -Is))"
config scenarios "$ONLY"
config clock "pinned to $RUN_DAY 08:00 $TZ0 per scenario"
session_start "$AVD" 2048
install_apk "$APK" -r >/dev/null || die "install failed"

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
session_end "Emulator scenarios report"
