#!/usr/bin/env bash
# `expect ... eval '<check>'` evaluates the check when expect runs it.
# shellcheck disable=SC2016
# On-device checks for lollop's system surfaces (planning/design/
# visual-design-system.md § 10), each with explicit PASS/FAIL rows and evidence:
#   links   the app's open intent with dest=today|log|track|log_set (+ an
#           unknown value), cold start and warm (onNewIntent)
#   tile    Quick Settings tile added with `cmd statusbar add-tile`: label per
#           day state (lift / run / rest / hold / stale) and a real tap on it
#           opening the right screen
#   widget  Today widget placed through the launcher's widget picker at its
#           default size, the checks following the size the launcher gave it:
#           run day (Start run, + Log from three cells), lift day, hold, a date
#           change (stale text), and its three tap targets
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
#   tilelock (opt-in) the tile tapped over the PIN keyguard asks for the PIN,
#           then opens its screen (app in the background, process killed, a
#           lift day)
#   reminder (opt-in, ~40 min, reboots the device) the daily session reminder:
#           Profile switch + time picker, the posted reminder (title, line, one
#           action) on a run day and a lift day, its action from the shade and
#           from the lock screen (PIN), which takes it down, content tap →
#           Today, at most one a day, nothing on a rest day / under a hold /
#           once done, removed when the session is done or moved, the
#           snapshot-ahead path, a time zone change and a reboot.
#           Uses its own week (Mon 2026-11-02 …): a clock pinned before the
#           system image's build date is moved to it at boot.
# The Live Update checks (chip, promoted flag, toggle) need API 36+; below that
# the same flows check the plain notification and that the Profile notice stays
# hidden.
#
#   scripts/emu-scenarios/surfaces.sh [--avd e37] [--port 5582] [--apk PATH]
#                                     [--only "links tile widget run guided rest toggle restbg tilefresh tilelock reminder"]
#                                     [--keep-emulator]
#
# --only takes scenario names separated by spaces or commas.
# Outputs: a new folder per run, ~/.cache/emu-surfaces/<avd>/<YYYYmmdd-HHMMSS>/
# (REPORT.md, checks.tsv, logcat.txt, logcat-scan.json, shots/, ui/, logs/;
# earlier runs are kept). The device clock is pinned per check
# (Mon 2026-09-21 = run day, Tue 22 = rest day, Wed 23 = lift day for the seeded
# hybrid profile, Europe/Berlin). Exit 0 when no check FAILed. The emulator is
# stopped at the end unless --keep-emulator or it was already running.
set -uo pipefail

# shellcheck source-path=SCRIPTDIR source=lib.sh
source "$(dirname "$0")/lib.sh"
AVD="e37"
PORT=5582
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
ONLY="links tile widget run guided rest toggle"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --avd) AVD="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --apk) APK="$2"; shift 2 ;;
    --only) ONLY="${2//,/ }"; shift 2 ;;
    --keep-emulator) KEEP_EMU=1; shift ;;
    -h|--help) awk 'NR>1&&/^#/{print;next}NR>1{exit}' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

SERIAL="emulator-$PORT"
[[ -f "$APK" ]] || { echo "APK not found: $APK" >&2; exit 2; }
OUT_DIR="$HOME/.cache/emu-surfaces/$AVD/$(date +%Y%m%d-%H%M%S)"
[[ -e "$OUT_DIR" ]] && { echo "output folder exists: $OUT_DIR" >&2; exit 2; }
out_init "$OUT_DIR"

# seed_basic plus a pain report on DAY 07:30: training paused.
seed_hold() { # seed_hold DAY
  seed_basic "$SEED_DAY" 2 &&
    EV event SubmitReadiness signal=Pain value=1.0 streak=0 pain=null effort_min=null \
      observed_at="$(TZ="$TZ0" date -d "$1 07:30" +%s)"
}
# A beginner runner on a couch-to-5k plan: run/walk bouts (a guided run).
seed_c25k() {
  local y; y="$(TZ="$TZ0" date -d "$SEED_DAY 09:00" +%s)"
  seed_profile goal_distance=C25k concurrent_goal=EndurancePriority weekly_sets=0 \
    running_days_per_week=3 running_km_per_week=5.0 bodyweight_kg=80.0 &&
    seed_run 2.0 20.0 "$y"
}
# seed SEED-FN [ARG]: the seed's event log as logs/seed-SEED-FN.ndjson; echoes its path.
seed() { "$@" >"$OUT/logs/seed-$1.ndjson" && echo "$OUT/logs/seed-$1.ndjson"; }
# fresh DAY SEED-FN [ARG]: wipe the app, seed it, boot it on Mon, land on DAY.
fresh() { local day="$1"; shift; fresh_app "$day" "$(seed "$@")"; }
# reseed DAY SEED-FN [ARG]: the same without `pm clear` (keeps a placed widget
# bound and the app out of the stopped state, so it still gets broadcasts).
reseed() {
  local day="$1"; shift
  stop_feeder
  stop_app
  push_log "$(seed "$@")" || return 1
  boot_week "$day"
}
snap_stale() { [[ "$(snap_field stale)" == True ]]; }
snap_run_detail() { snapshot | python3 -c "import json,sys;print(next(i['detail'] for i in json.load(sys.stdin)['items'] if i['kind']=='Run'))"; }

# Which app screen is in front: today | log | track | log_set (the open intent's
# destinations) | other | none.
screen() {
  local x
  [[ "$(focus)" == "$APP_ID"* ]] || { echo none; return; }
  x="$(dump_ui)" || { echo other; return; }
  if q eq "${STR[log_field_exercise]}" <<<"$x" >/dev/null; then echo log_set
  elif q eq "${STR[main_track_run_title]}" <<<"$x" >/dev/null && q eq "${STR[main_log_set_title]}" <<<"$x" >/dev/null; then echo log
  elif q eq "${STR[track_map_attribution]}" <<<"$x" >/dev/null; then echo track
  elif q on "${STR[main_dest_today]}" <<<"$x" >/dev/null; then echo today
  else echo other; fi
}
on_screen() { [[ "$(screen)" == "$1" ]]; }
wait_screen() { wait_for "${2:-10}" on_screen "$1"; } # wait_screen NAME [seconds]
# Close the set editor with its X (not "Done for now").
editor_x() { tap_node eq "${STR[main_close]}" 2 && dismiss_editor; }
landscape() { dump_ui | grep -o 'bounds="\[0,0\]\[[0-9]*,[0-9]*\]"' | head -1 | awk -F'[],[]' '{ exit !($5 > $6) }'; }
portrait() { ! landscape; }
screen_off() { ! screen_on; }
bouncer_up() { sh_ dumpsys window | grep -E 'mBouncerVisible=true|bouncer_visible' >/dev/null; }
settings_up() { [[ "$(focus)" == com.android.settings* ]]; }

# notif_is NAME ID FIELD VALUE: the app's notification ID has extras FIELD == VALUE
# (saved as logs/NAME-notif.*).
notif_is() { notif "$1" "$2" >/dev/null; [[ "$(nfield "$1" "r['extras'].get('$3')")" == "$4" ]]; }
# notif_clock NAME ID: its chip text (android.shortCriticalText) is a running time.
notif_clock() { notif "$1" "$2" >/dev/null; [[ $(duration "$(nfield "$1" "r['extras'].get('android.shortCriticalText')")") -gt 0 ]]; }

tap_tile() { # tap_tile DUMP LABEL (QS already open from tile_label)
  local r p
  r="$(tile_node "$1" "$2")" || return 1
  read -r -a p <<<"$r"; tap_xy "${p[0]}" "${p[1]}"
}
# tile_case NAME EXPECTED-LABEL EXPECTED-SCREEN [EXPECTED-SUBTITLE]: label (and,
# when given, the subtitle under it; "-" for none) check, then a real tap.
tile_case() {
  local S="${TILE_S:-tile}" l s sub
  adb_ shell cmd statusbar collapse; home
  l="$(tile_label "tile-$1")"
  shot "tile-$1-qs"
  # With the panel open: the tile service's binding (activity manager) and
  # System UI's record of the tile (its cached state and listening flag).
  sh_ dumpsys activity services "${COMP[QuickTileService]}" >"$OUT/logs/tile-$1-service.txt"
  sh_ dumpsys activity service com.android.systemui | grep -i -B2 -A8 "$APP_ID" >"$OUT/logs/tile-$1-sysui.txt"
  expect "$S" "$1: tile label" "label '$l' (want '$2'); ui/tile-$1.xml shots/tile-$1-qs.png" test "$l" = "$2"
  if [[ -n "${4:-}" && -n "$l" ]]; then
    sub="$(cat "$OUT/ui/tile-$1.subtitle")"
    expect "$S" "$1: tile subtitle" "subtitle '$sub' (want '${4/#-/}'); ui/tile-$1.xml" test "$sub" = "${4/#-/}"
  fi
  if [[ -n "$l" ]] && tap_tile "tile-$1" "$l"; then
    wait_screen "$3" 10; s="$(screen)"
    evidence "tile-$1-tap"
    expect "$S" "$1: tile tap opens $3" "screen '$s'; shots/tile-$1-tap.png" test "$s" = "$3"
  else
    check FAIL "$S" "$1: tile tap opens $3" "tile not found on the QS panel"
  fi
  adb_ shell cmd statusbar collapse
}

sc_links() {
  local S=links d want s ex
  fresh "$LIFT_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  ex="$(snap_field items.0.name)"
  for d in "${DEST[Today]}" "${DEST[Log]}" "${DEST[Track]}" "${DEST[LogSet]}" bogus; do
    if [[ "$d" == bogus ]]; then want="${DEST[Today]}"; else want="$d"; fi
    mark "links-cold-$d"
    stop_app
    open_dest "$d"
    wait_screen "$want" 25; s="$(screen)"; evidence "links-cold-$d"
    expect $S "cold start dest=$d opens $want" "screen '$s'; shots/links-cold-$d.png" test "$s" = "$want"
    if [[ "$d" == "${DEST[LogSet]}" ]]; then
      expect $S "log_set prefills today's open lift item" "want '$(fmt log_change_exercise_cd "$ex")'; ui/links-cold-$d.xml" \
        eval '[[ -n "$ex" ]] && q eq "$(fmt log_change_exercise_cd "$ex")" <"$OUT/ui/links-cold-$d.xml" >/dev/null'
    fi
    mark "links-warm-$d"
    stop_app; launch; wait_screen today 25
    open_dest "$d"
    wait_screen "$want" 15; s="$(screen)"; evidence "links-warm-$d"
    expect $S "warm (onNewIntent) dest=$d opens $want" "screen '$s'; shots/links-warm-$d.png" test "$s" = "$want"
  done
  # A rotation must not navigate again: open log, close the sheet, rotate.
  mark links-rotate
  stop_app
  open_dest "${DEST[Log]}"
  wait_screen log 25; adb_ shell input keyevent 4; wait_screen today 5
  adb_ shell settings put system accelerometer_rotation 0
  adb_ shell settings put system user_rotation 1; wait_for 10 landscape
  s="$(screen)"; evidence links-rotated
  adb_ shell settings put system user_rotation 0; wait_for 10 portrait
  expect $S "rotation after an open intent does not reopen the sheet" "screen '$s'; shots/links-rotated.png" test "$s" = today
}

sc_tile() {
  local S=tile
  fresh "$LIFT_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  add_tile || check FAIL $S "tile added to Quick Settings" "not on the QS panel ~40 s after add-tile; ui/tile-added.xml"
  home
  tile_case lift-day "${STR[tile_log_set]}" log_set "$(snap_field title)"
  reseed "$RUN_DAY" seed_basic "$SEED_DAY" 2; home
  tile_case run-day "${STR[tile_start_run]}" track "$(snap_field title)"
  reseed "$REST_DAY" seed_basic "$SEED_DAY" 2; home
  tile_case rest-day "${STR[tile_log]}" log "${STR[widget_rest_day]}"
  reseed "$RUN_DAY" seed_hold "$RUN_DAY"; home
  expect $S "hold seed pauses training" "snapshot kind '$(snap_field kind)'" test "$(snap_field kind)" = Paused
  tile_case hold "${STR[tile_log]}" log "${STR[main_status_on_hold]}"
  # The stored day is Mon; the clock moves to Wed with the app in the background.
  reseed "$RUN_DAY" seed_basic "$SEED_DAY" 2; home
  pin_clock "$LIFT_DAY"; wait_for 10 snap_stale
  tile_case stale-day "${STR[tile_log]}" log -
}

# The stale-day and recording tile cases with no force-stop or `pm clear` once
# the tile is added (sc_tile reseeds with a force-stop between cases): the seed
# is pushed onto the fresh install before its first launch. Meant for a freshly
# booted emulator: `--only tilefresh` with the emulator not running.
sc_tilefresh() {
  local S=tilefresh TILE_S=tilefresh pid
  observe $S "device uptime at the start (fresh boot; APK installed by this run)" "$(sh_ cut -d. -f1 /proc/uptime) s"
  baseline_device; grant_perms
  pin_clock "$RUN_DAY"
  push_log "$(seed seed_basic "$SEED_DAY" 2)" || { check FAIL $S "seed pushed" "push_log failed"; return; }
  launch
  { wait_text eq "${STR[main_dest_today]}" 40 && wait_for 15 snapshot_for "$RUN_DAY"; } ||
    { evidence tilefresh-boot; check FAIL $S "seeded boot" "Today never rendered"; return; }
  home
  add_tile || check FAIL $S "tile added to Quick Settings" "not on the QS panel ~40 s after add-tile; ui/tile-added.xml"
  tile_case run-day "${STR[tile_start_run]}" track
  # The stored day is Mon; the clock moves to Wed with the app in the background.
  home
  pid="$(app_pid)"
  pin_clock "$LIFT_DAY"; wait_for 10 snap_stale
  expect $S "stale-day: same app process, never stopped" "pid '$pid' → '$(app_pid)'" eval '[[ -n "$pid" && "$(app_pid)" == "$pid" ]]'
  tile_case stale-day "${STR[tile_log]}" log
  # Recording, started from the Log sheet the tile just opened.
  if tap_node eq "${STR[main_track_run_title]}" && wait_text eq "${STR[track_btn_start]}" 20; then
    start_feeder tilefresh
    tap_node eq "${STR[track_btn_start]}"
    if wait_text eq "${STR[track_btn_stop]}" 30; then
      tile_case recording "${STR[tile_run_recording]}" track
      finish_run || check FAIL $S "finish and save the run" "Stop → Save run failed"
    else
      evidence tilefresh-no-stop; check FAIL $S "run recording" "no Stop after Start; shots/tilefresh-no-stop.png"
    fi
    stop_feeder
  else
    evidence tilefresh-no-tracker; check FAIL $S "open the tracker from the Log sheet" "shots/tilefresh-no-tracker.png"
  fi
}

# lock_with_pin: set the PIN, turn the screen off and on again (keyguard up).
lock_with_pin() {
  sh_ locksettings set-pin $REM_PIN >/dev/null
  adb_ shell input keyevent 26; wait_for 5 screen_off
  adb_ shell input keyevent 224; wait_for 5 screen_on
}
# tilelock_case NAME LABEL WANT-SCREEN: lock the device with a PIN, open Quick
# Settings over the keyguard and tap the tile labelled LABEL; System UI asks for
# the PIN (API 34+: it starts the tile's PendingIntent dismissing the keyguard;
# below 34: TileService.unlockAndRun), then the app must open WANT-SCREEN.
# Clears the PIN.
tilelock_case() {
  local d="tilelock-$1" r="" p s bouncer="" want="$3"
  home
  lock_with_pin
  adb_ shell cmd statusbar expand-settings
  wait_for 5 _tile_seen "$d"; shot "$d"
  r="$(tile_node "$d" "$2")" || r="$(ocr_find "$d" "${2%% *}")" || r=""
  if [[ -n "$r" ]]; then
    read -r -a p <<<"$r"; tap_xy "${p[0]}" "${p[1]}"
    wait_for 5 bouncer_up
    sh_ dumpsys window >"$OUT/logs/$d-bouncer-window.txt"
    bouncer="$(grep -m1 -oE '(isKeyguardShowing|mKeyguardShowing)=true' "$OUT/logs/$d-bouncer-window.txt"); focus $(focus)"
    adb_ shell input text $REM_PIN; adb_ shell input keyevent 66
    wait_screen "$3" 10; s="$(screen)"
  else
    s="(no '$2' tile over the keyguard; ui/$d.xml)"
  fi
  evidence "$d-unlocked"
  sh_ locksettings clear --old $REM_PIN >/dev/null
  expect "$S" "$1: tile '$2' on the lock screen asks for the PIN, then opens $3" \
    "before the PIN: '${bouncer:-tile not tapped}'; after: screen '$s'; shots/$d.png logs/$d-bouncer-window.txt shots/$d-unlocked.png" \
    eval '[[ "$bouncer" == *KeyguardShowing=true* && "$bouncer" != *"focus $APP_ID"* && "$s" == "$want" ]]'
  adb_ shell cmd statusbar collapse
}
# The tile from the lock screen: with the app in the background, after its
# process was killed, and on a lift day.
sc_tilelock() {
  local S=tilelock n
  fresh "$RUN_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  add_tile || check FAIL $S "tile added to Quick Settings" "not on the QS panel ~40 s after add-tile; ui/tile-added.xml"
  tilelock_case background "${STR[tile_start_run]}" track
  adb_ shell input keyevent 4; home
  stop_app sigkill
  tilelock_case killed "${STR[tile_start_run]}" track
  adb_ shell input keyevent 4
  reseed "$LIFT_DAY" seed_basic "$SEED_DAY" 2
  tilelock_case lift-day "${STR[tile_log_set]}" log_set
}

# widget_tap NAME LABEL WANT-SCREEN
widget_tap() {
  local S=widget s
  home
  if tap_node eq "$2" 3; then
    wait_screen "$3" 15; s="$(screen)"
  else
    s="(no '$2' on the widget)"
  fi
  evidence "widget-tap-$1"
  expect $S "tap '$2' opens $3" "screen '$s'; shots/widget-tap-$1.png" test "$s" = "$3"
  stop_app; launch; wait_screen today 20; home
}
sc_widget() {
  local S=widget t title open_today="${STR[widget_open_today_cd]}" log="${STR[tile_log]}"
  fresh "$RUN_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  home
  if ! place_widget; then
    evidence widget-place-failed
    check FAIL $S "widget placed on the home screen" "picker flow failed; shots/widget-picker.png shots/widget-place-failed.png"
    return
  fi
  check PASS $S "widget placed on the home screen" "dumpsys appwidget lists TodayWidgetReceiver; shots/widget-picker.png"
  t="$(widget_texts widget-run-day)"
  # What the widget shows follows the size the launcher gave it (TodayWidget.kt
  # WidgetSizes; visual-design-system § 10): Log beside the primary from three
  # cells (200 dp wide), the date line from two rows (184 dp tall).
  local ww=0 wh=0 wide=0 dated=0
  read -r ww wh <<<"$(widget_dp widget-run-day)"
  (( ww >= 200 )) && wide=1
  (( wh >= 184 )) && dated=1
  observe $S "widget size" "${ww} × ${wh} dp: Log beside the primary $( ((wide)) && echo yes || echo no), date line $( ((dated)) && echo yes || echo no); shots/widget-run-day.png"
  # The run line: "Run · 8 km · 5:40–6:00 /km", or on a card with room the run's
  # volume as the hero; a detail that is only a figure ("3.4 mi") is the hero
  # alone (visual-design-system § 10, Home screen widget, Hero). The figure and
  # its unit are glued with a no-break space, in the snapshot and on the widget.
  local fig tn run_line
  fig="$(snap_run_detail)"
  # shellcheck disable=SC2034 # read by the check below
  tn="${t//$'\xc2\xa0'/ }"
  title="$(snap_field title)"
  # shellcheck disable=SC2034 # read by the check below
  run_line="$(fmt widget_item_line "${STR[main_run]}" '')"
  expect $S "run day: title, run line (or its figure as the hero), ${STR[tile_start_run]} (+ $log at ${ww} dp)" "title '$title'; run detail '$fig'; $t; shots/widget-run-day.png" \
    eval 'grep -qxF "text=$title" <<<"$t" && { grep -qF "text=$run_line" <<<"$tn" || { [[ -n "$fig" ]] && grep -qxF "text=$fig" <<<"$t"; }; } && grep -qxF "text=${STR[tile_start_run]}" <<<"$t" && { ((!wide)) || grep -qxF "content-desc=$log" <<<"$t"; }'
  widget_tap run-track "${STR[tile_start_run]}" track
  if ((wide)); then widget_tap run-log "$log" log; else observe $S "tap '$log'" "not checked: no $log beside the primary at ${ww} dp wide"; fi
  widget_tap run-body "$open_today" today

  reseed "$LIFT_DAY" seed_basic "$SEED_DAY" 2
  t="$(widget_texts widget-lift-day)"
  title="$(snap_field title)"
  expect $S "lift day: lift title and items, ${STR[tile_log_set]} (+ $log at ${ww} dp)" "snapshot title '$title'; $t; shots/widget-lift-day.png" \
    eval 'grep -qxF "text=$title" <<<"$t" && grep -qxF "text=${STR[tile_log_set]}" <<<"$t" && { ((!wide)) || grep -qxF "content-desc=$log" <<<"$t"; } && ! grep -qxF "text=${STR[tile_start_run]}" <<<"$t"'

  reseed "$RUN_DAY" seed_hold "$RUN_DAY"
  t="$(widget_texts widget-hold)"
  expect $S "hold: ${STR[main_status_on_hold]}, no reason, $log" "$t; shots/widget-hold.png" \
    eval 'grep -qxF "text=${STR[main_status_on_hold]}" <<<"$t" && grep -qxF "text=${STR[widget_paused_body]}" <<<"$t" && ! grep -q "pain" <<<"$t" && grep -qxF "text=$log" <<<"$t"'

  # Date change with the app in the background (not force-stopped: a stopped app
  # gets no TIME_SET broadcast).
  reseed "$RUN_DAY" seed_basic "$SEED_DAY" 2; home
  mark widget-date-change
  pin_clock "$REST_DAY"; wait_for 10 snap_stale
  t="$(widget_texts widget-stale)"
  expect $S "date change: '${STR[widget_open_to_update]}' + $log" "snapshot stale=$(snap_field stale); $t; shots/widget-stale.png" \
    eval 'grep -qxF "text=${STR[widget_open_to_update]}" <<<"$t" && grep -qxF "text=$log" <<<"$t"'
  launch; wait_screen today 20; wait_for 15 snapshot_for "$REST_DAY"
  t="$(widget_texts widget-after-open)"
  # The new day: its date in the overline (from two rows), else its title.
  local rest_date
  # shellcheck disable=SC2034 # read by the check below
  rest_date="$(date -d "$REST_DAY" '+%a, %b %-d')"
  expect $S "opening the app refreshes it to the new day" "$t; shots/widget-after-open.png" \
    eval '! grep -qxF "text=${STR[widget_open_to_update]}" <<<"$t" && if ((dated)); then grep -qiF "$rest_date" <<<"$t"; else grep -qxF "text=${STR[widget_rest_day]}" <<<"$t"; fi'
}

sc_run() {
  local S=run c1 c2 t1 t2 t0 RUN=$RUN_NOTIF_ID
  fresh "$RUN_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  # No feed yet: the emulator GNSS repeats its default fix, so the locate preview
  # gets a lock and a "Start on move" run stays armed until the feed moves.
  open_tracker || { check FAIL $S "open tracker" "Log → Track run (GPS) failed"; return; }
  wait_for 10 notif_is run-locate "$RUN" android.title "${STR[track_notif_locate_title]}"
  expect $S "locate preview: title '${STR[track_notif_locate_title]}', chip text '${STR[live_chip_locating]}'" \
    "title '$(nfield run-locate "r['extras'].get('android.title')")', shortCriticalText '$(nfield run-locate "r['extras'].get('android.shortCriticalText')")'; logs/run-locate-notif.txt" \
    eval '[[ "$(nfield run-locate "r[\"extras\"].get(\"android.title\")")" == "${STR[track_notif_locate_title]}" && "$(nfield run-locate "r[\"extras\"].get(\"android.shortCriticalText\")")" == "${STR[live_chip_locating]}" ]]'
  live_promoted run-locate "locate preview"
  mark run-armed
  scroll_to eq "${STR[track_gate_moving]}" 4 && tap_node eq "${STR[track_gate_moving]}" && wait_for 3 selected "${STR[track_gate_moving]}"
  tap_node eq "${STR[track_btn_start]}" || { check FAIL $S "start run" "no Start"; return; }
  wait_text eq "${STR[track_pill_armed_moving]}" 10; evidence run-armed-app
  home
  wait_for 10 notif_is run-armed "$RUN" android.title "${STR[track_notif_armed_title]}"
  c1="$(chip run-chip-armed)"; sb_shot run-chip-armed
  expect $S "armed (Start on move, standing still): title '${STR[track_notif_armed_title]}', chip text '${STR[live_chip_waiting]}'" \
    "title '$(nfield run-armed "r['extras'].get('android.title')")', shortCriticalText '$(nfield run-armed "r['extras'].get('android.shortCriticalText')")'" \
    eval '[[ "$(nfield run-armed "r[\"extras\"].get(\"android.title\")")" == "${STR[track_notif_armed_title]}" && "$(nfield run-armed "r[\"extras\"].get(\"android.shortCriticalText\")")" == "${STR[live_chip_waiting]}" ]]'
  [[ $API -ge 36 ]] && expect $S "armed: status-bar chip reads '${STR[live_chip_waiting]}'" "SystemUI chip '$c1'; shots/run-chip-armed-statusbar.png" test "$c1" = "${STR[live_chip_waiting]}"
  mark run-recording
  start_feeder run
  t0=$SECONDS
  wait_for 30 notif_is run-wait "$RUN" android.title "${STR[track_notif_recording_title]}"
  expect $S "moving feed starts the armed run" "title '${STR[track_notif_recording_title]}' after ~$((SECONDS - t0))s of feed" \
    test "$(nfield run-wait "r['extras'].get('android.title')")" = "${STR[track_notif_recording_title]}"
  sleep 8
  c1="$(chip run-chip-1)"; sb_shot run-chip-1; notif run-rec-1 "$RUN" >/dev/null
  sleep 10
  c2="$(chip run-chip-2)"; sb_shot run-chip-2; notif run-rec-2 "$RUN" >/dev/null
  t1="$(nfield run-rec-1 "r['extras'].get('android.shortCriticalText')")"
  t2="$(nfield run-rec-2 "r['extras'].get('android.shortCriticalText')")"
  expect $S "recording: notification chip text is the moving time and advances" \
    "shortCriticalText '$t1' → '$t2' (10 s apart), title '$(nfield run-rec-2 "r['extras'].get('android.title')")', text '$(nfield run-rec-2 "r['extras'].get('android.text')")'" \
    eval '[[ $(duration "$t1") -ge 0 && $(duration "$t2") -gt $(duration "$t1") ]]'
  live_promoted run-rec-2 "recording"
  if [[ $API -ge 36 ]]; then
    expect $S "recording: status-bar chip (shade closed) shows the moving time, advancing" \
      "SystemUI chip '$c1' → '$c2'; shots/run-chip-1-statusbar.png shots/run-chip-2-statusbar.png ui/run-chip-*.xml" \
      eval '[[ $(duration "$c1") -ge 0 && $(duration "$c2") -gt $(duration "$c1") ]]'
  else
    expect $S "API $API: no status-bar chip, plain ongoing notification" "chip '$c2'; flags $(nfield run-rec-2 "'|'.join(r['flags'])")" \
      eval '[[ -z "$c2" ]] && [[ "$(nfield run-rec-2 "\"ONGOING_EVENT\" in r[\"flags\"]")" == True ]]'
  fi
  add_tile
  tile_case recording "${STR[tile_run_recording]}" track
  home
  mark run-pause
  if tap_shade run-shade-pause eq "${STR[track_action_pause]}" "${STR[track_notif_recording_title]}"; then
    adb_ shell cmd statusbar collapse
    wait_for 10 notif_is run-paused "$RUN" android.title "${STR[track_notif_paused_title]}"
    c1="$(chip run-chip-paused)"; sb_shot run-chip-paused
    expect $S "paused (notification action): title '${STR[track_notif_paused_title]}', chip text '${STR[live_chip_paused]}'" \
      "title '$(nfield run-paused "r['extras'].get('android.title')")', shortCriticalText '$(nfield run-paused "r['extras'].get('android.shortCriticalText')")'" \
      eval '[[ "$(nfield run-paused "r[\"extras\"].get(\"android.shortCriticalText\")")" == "${STR[live_chip_paused]}" && "$(nfield run-paused "r[\"extras\"].get(\"android.title\")")" == "${STR[track_notif_paused_title]}" ]]'
    [[ $API -ge 36 ]] && expect $S "paused: status-bar chip reads '${STR[live_chip_paused]}'" "SystemUI chip '$c1'; shots/run-chip-paused-statusbar.png" test "$c1" = "${STR[live_chip_paused]}"
    tap_shade run-shade-resume eq "${STR[track_action_resume]}" "${STR[track_notif_paused_title]}"; adb_ shell cmd statusbar collapse
    wait_for 10 notif_is run-resumed "$RUN" android.title "${STR[track_notif_recording_title]}"
  else
    check FAIL $S "pause from the notification" "no Pause action in the shade; ui/run-shade-pause.xml"
  fi
  # The emulator GNSS repeats the last fix every ~2 s after the feed stops, so a
  # lost signal is location turned off (the service ignores provider-disabled).
  mark run-signal-lost
  sh_ cmd location set-location-enabled false
  wait_for 22 notif_is run-nogps "$RUN" android.shortCriticalText "${STR[live_chip_no_gps]}"
  c1="$(chip run-chip-nogps)"; sb_shot run-chip-nogps
  expect $S "GPS signal lost (location off, within 22 s): chip text '${STR[live_chip_no_gps]}'" \
    "shortCriticalText '$(nfield run-nogps "r['extras'].get('android.shortCriticalText')")', text '$(nfield run-nogps "r['extras'].get('android.text')")'" \
    test "$(nfield run-nogps "r['extras'].get('android.shortCriticalText')")" = "${STR[live_chip_no_gps]}"
  [[ $API -ge 36 ]] && expect $S "signal lost: status-bar chip reads '${STR[live_chip_no_gps]}'" "SystemUI chip '$c1'; shots/run-chip-nogps-statusbar.png" test "$c1" = "${STR[live_chip_no_gps]}"
  sh_ cmd location set-location-enabled true
  expect $S "signal back: chip returns to the moving time" "logs/run-regained-notif.json" wait_for 15 notif_clock run-regained "$RUN"
  mark run-tap
  home
  if tap_shade run-shade-tap eq "${STR[track_notif_recording_title]}"; then
    wait_screen track 10; t1="$(screen)"; evidence run-tap
    expect $S "tapping the notification opens the tracker" "screen '$t1'; shots/run-tap.png" test "$t1" = track
  else
    check FAIL $S "tapping the notification opens the tracker" "no '${STR[track_notif_recording_title]}' in the shade; ui/run-shade-tap.xml"
  fi
  expect $S "finish and save the run" "Stop → Save run" finish_run
  stop_feeder
  notif run-after "$RUN" >/dev/null
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
  local S=guided d found="" RUN=$RUN_NOTIF_ID
  fresh "$RUN_DAY" seed_c25k || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  for d in "$RUN_DAY" $(for i in 1 2 3 4 5 6; do date -d "$RUN_DAY +$i day" +%F; done); do
    [[ "$d" == "$RUN_DAY" ]] || boot_day "$d"
    [[ "$(snap_field kind)" == Run && "$(snap_field trackable)" == True ]] && { found=$d; break; }
  done
  [[ -n "$found" ]] || { check FAIL $S "a C25k run day this week" "no run day found"; return; }
  check PASS $S "a C25k run day this week" "$found: $(snap_field title)"
  start_feeder guided
  open_tracker || { check FAIL $S "open tracker" "failed"; return; }
  wait_text re "${RX[track_pill_fresh_lock]}|^${STR[track_cd_pill_fresh_lock]}\$" 20; evidence guided-tracker
  tap_node eq "${STR[track_btn_start]}" || { check FAIL $S "start run" "no Start"; return; }
  wait_text eq "${STR[track_btn_stop]}" 20; sleep 12
  home
  notif guided "$RUN" >/dev/null
  shade_open guided-shade; shot guided-shade; adb_ shell cmd statusbar collapse
  if [[ $API -ge 36 ]]; then
    # STEP_BAR_UNITS per step; past 15 steps the bar is one segment (platform limit).
    expect $S "guided run: ProgressStyle step bar, $STEP_BAR_UNITS units per step (one segment past 15 steps)" \
      "template $(nfield guided "r['extras'].get('android.template')"); progress $(nfield guided "r['extras'].get('android.progress')")/$(nfield guided "r['extras'].get('android.progressMax')") = $(( $(nfield guided "int(r['extras'].get('android.progressMax') or 0)") / STEP_BAR_UNITS )) steps; segments $(nfield guided "r['extras'].get('android.progressSegments')"); text '$(nfield guided "r['extras'].get('android.text')")'; shots/guided-shade.png" \
      eval '[[ "$(nfield guided "r[\"extras\"].get(\"android.template\")")" == *ProgressStyle* && $(nfield guided "int(r[\"extras\"].get(\"android.progressMax\") or 0) % $STEP_BAR_UNITS") -eq 0 && $(nfield guided "int(r[\"extras\"].get(\"android.progressMax\") or 0)") -gt 0 ]]'
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

# The rest notification's line: "Next: Set N of M · …".
NEXT_SET_RX="^$(fmt log_rest_notif_next "$(fmt log_set_header_n_of_n '[0-9]+' '[0-9]+')")"
rest_done_posted() { notif_is "$1" "$REST_DONE_NOTIF_ID" android.title "${STR[log_rest_notif_done_title]}"; }
sc_rest() {
  local S=rest c1 c2 s left
  fresh "$LIFT_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  # Exact alarms allowed, so the rest-done alert is due at the rest end (sc_restbg
  # covers the inexact case).
  sh_ appops set "$APP_ID" SCHEDULE_EXACT_ALARM allow
  # The set editor at today's plan item (the chooser's plain "Log set" row opens
  # it blank, with no prescribed rest).
  open_dest "${DEST[LogSet]}"
  wait_screen log_set 15 || { evidence rest-no-editor; check FAIL $S "open set editor" "dest=log_set did not open it"; return; }
  tap_log_set || { evidence rest-no-primary; check FAIL $S "log a set" "no 'Log set N' button, or no LogSet logged"; return; }
  evidence rest-loop
  notif rest-inapp "$REST_NOTIF_ID" >/dev/null
  expect $S "rest readout on screen: no countdown notification" "$(cat "$OUT/logs/rest-inapp-notif.json")" test "$(cat "$OUT/logs/rest-inapp-notif.json")" = null
  editor_x
  home
  wait_for 5 eval 'notif rest-1 "$REST_NOTIF_ID" | grep -qv "^null$"'
  c1="$(chip rest-chip-1)"; sb_shot rest-chip-1
  sleep 6
  c2="$(chip rest-chip-2)"; sb_shot rest-chip-2
  expect $S "countdown posted: '$(fmt log_rest_notif_title …)', chronometer counting down to the rest end" \
    "channel $(nfield rest-1 "r['channel']"); title '$(nfield rest-1 "r['extras'].get('android.title')")'; showChronometer $(nfield rest-1 "r['extras'].get('android.showChronometer')"); countDown $(nfield rest-1 "r['extras'].get('android.chronometerCountDown')"); when-now $(( $(nfield rest-1 "r['when']") / 1000 - $(dev_now) )) s" \
    eval '[[ "$(nfield rest-1 "r[\"extras\"].get(\"android.title\")")" =~ ${RX[log_rest_notif_title]} && "$(nfield rest-1 "r[\"extras\"].get(\"android.showChronometer\")")" == true && "$(nfield rest-1 "r[\"extras\"].get(\"android.chronometerCountDown\")")" == true && $(nfield rest-1 "r[\"when\"]") -gt $(dev_now)000 ]]'
  expect $S "card names the next set: 'Next: Set N of M · …'" "text '$(nfield rest-1 "r['extras'].get('android.text')")'" \
    eval '[[ "$(nfield rest-1 "r[\"extras\"].get(\"android.text\")")" =~ $NEXT_SET_RX ]]'
  if [[ $API -ge 36 ]]; then
    expect $S "countdown: FLAG_PROMOTED_ONGOING set" "flags $(nfield rest-1 "'|'.join(r['flags'])")" test "$(promoted rest-1)" = True
    expect $S "status-bar chip (shade closed) counts down" "SystemUI chip '$c1' → '$c2' (6 s apart); shots/rest-chip-1-statusbar.png shots/rest-chip-2-statusbar.png" \
      eval '[[ $(duration "$c1") -gt 0 && $(duration "$c2") -lt $(duration "$c1") ]]'
  else
    expect $S "countdown (API $API): plain ongoing notification" "flags $(nfield rest-1 "'|'.join(r['flags'])"); chip '$c1'" \
      eval '[[ "$(promoted rest-1)" == False && -z "$c1" ]]'
  fi
  mark rest-tap
  # Tap the content line: on API 34 a tap on the collapsed card's title line
  # (title · chronometer) opens nothing.
  if tap_shade rest-shade re "$NEXT_SET_RX"; then
    wait_screen log_set 10; s="$(screen)"; evidence rest-tap
    expect $S "tapping the countdown opens the set editor" "screen '$s'; shots/rest-tap.png" test "$s" = log_set
  else
    check FAIL $S "tapping the countdown opens the set editor" "no countdown in the shade; ui/rest-shade.xml"
  fi
  # The editor is open again (readout on screen); leave with X and wait out the rest.
  editor_x
  home
  left=$(( $(nfield rest-1 "r['when']") / 1000 - $(dev_now) ))
  # The shade steps can outlast the rest (slow dumps); then check right away.
  if [[ $left -lt 400 ]]; then
    wait_for $(( (left > 0 ? left : 0) + 10 )) rest_done_posted rest-done
    notif rest-done-countdown "$REST_NOTIF_ID" >/dev/null; c1="$(chip rest-chip-done)"; sb_shot rest-done
    expect $S "rest end: '${STR[log_rest_notif_done_title]}' alert replaces the countdown, not promoted, chip gone" \
      "channel $(nfield rest-done "r['channel']"); title '$(nfield rest-done "r['extras'].get('android.title')")'; flags $(nfield rest-done "'|'.join(r['flags'])"); countdown $(cat "$OUT/logs/rest-done-countdown-notif.json"); chip '$c1'" \
      eval '[[ "$(nfield rest-done "r[\"channel\"]")" == "$REST_DONE_CHANNEL" && "$(promoted rest-done)" == False && "$(cat "$OUT/logs/rest-done-countdown-notif.json")" == null && -z "$c1" ]]'
  else
    check FAIL $S "rest end" "not checked: the rest's ${left}s left is more than this case waits"
  fi
}

# The app process: "frozen" (cgroup v2 freezer), "cached" (a cch entry in the
# LRU list), "gone" or "alive".
proc_state() {
  local p; p="$(app_pid)"
  [[ -n "$p" ]] || { echo gone; return; }
  [[ "$(sh_ cat "/sys/fs/cgroup/uid_*/pid_$p/cgroup.events" | sed -n 's/^frozen //p')" == 1 ]] && { echo frozen; return; }
  sh_ dumpsys activity processes | grep -E "Proc #.* $p:$APP_ID" | grep ' cch' >/dev/null && { echo cached; return; }
  echo alive
}
bg_state() { case "$(proc_state)" in cached|frozen|gone) return 0 ;; *) return 1 ;; esac; }
# rest_bg CASE MODE [kill]: set the SCHEDULE_EXACT_ALARM app-op to MODE (allow |
# deny), log a set in the loop (a rest starts), go home with the loop open and
# wait until the app is cached or frozen; with `kill`, `am kill` the cached
# process instead (the alarm then starts a new one). Then wait for "Rest done":
# it must arrive, within 30 s of the rest end with exact alarms, at any time up
# to 15 min after it without. Arrival = the alert's post time ("when") against
# the countdown's rest end ("when").
rest_bg() {
  local S=restbg c="$1" mode="$2" kill="${3:-}" end pid0 pid1 st="" t0 now want delay
  mark "restbg-$c"
  stop_app
  sh_ appops set "$APP_ID" SCHEDULE_EXACT_ALARM "$mode"
  launch; wait_text eq "${STR[main_dest_today]}" 40
  open_dest "${DEST[LogSet]}"
  wait_screen log_set 15 || { evidence "restbg-$c-no-editor"; check FAIL $S "$c: open set editor" "dest=log_set did not open it"; return; }
  tap_log_set || { evidence "restbg-$c-no-primary"; check FAIL $S "$c: log a set" "no 'Log set N' button, or no LogSet logged"; return; }
  evidence "restbg-$c-loop"
  sh_ dumpsys alarm >"$OUT/logs/restbg-$c-alarm.txt"
  observe $S "$c: app-op and alarm" "SCHEDULE_EXACT_ALARM '$(sh_ appops get "$APP_ID" SCHEDULE_EXACT_ALARM | head -1)'; $(grep -A4 "$ACTION_REST_END\|RestEndReceiver" "$OUT/logs/restbg-$c-alarm.txt" | grep -m2 -oE 'type=[^ ]+|window=[^ ]+|exactAllowReason=[^ ]+|flags=[^ ]+' | tr '\n' ' '); logs/restbg-$c-alarm.txt"
  if [[ "$mode" == deny && $API -ge 31 ]]; then
    expect $S "$c: exact alarms off: the loop shows the notice" "ui/restbg-$c-loop.xml shots/restbg-$c-loop.png" has_text eq "${STR[log_exact_alarm_caption]}"
  else
    expect $S "$c: exact alarms allowed: no notice" "ui/restbg-$c-loop.xml" lacks_text eq "${STR[log_exact_alarm_caption]}"
  fi
  home
  notif "restbg-$c-count" "$REST_NOTIF_ID" >/dev/null
  end="$(nfield "restbg-$c-count" "r['when']")"
  [[ -n "$end" ]] || { check FAIL $S "$c: countdown posted" "$(cat "$OUT/logs/restbg-$c-count-notif.json")"; return; }
  # Another app in between, so lollop is not kept as the "previous" app.
  adb_ shell am start -a android.settings.SETTINGS >/dev/null; wait_for 5 settings_up; home
  pid0="$(app_pid)"
  t0=$SECONDS
  wait_for 120 bg_state; st="$(proc_state)"
  if [[ -n "$kill" ]]; then
    # A widget or tile broadcast may start a new process right after the kill:
    # the check is that the cached pid died.
    stop_app kill
    expect $S "$c: cached process killed (am kill)" "pid $pid0 → '$(app_pid)' (state '$st' before); $(sh_ logcat -d -b events | grep "am_kill.*$pid0," | tail -1)" \
      eval '[[ -n "$pid0" ]] && pid_released "$pid0"'
  else
    expect $S "$c: app cached or frozen in the background before the rest end" \
      "state '$st' $((SECONDS - t0)) s after home; rest end in $(( end / 1000 - $(dev_now) )) s" eval '[[ "$st" == cached || "$st" == frozen ]] && [[ $(( end / 1000 )) -gt $(dev_now) ]]'
  fi
  # The state 10 s before the rest end (the freezer acts a little after the app
  # is cached), when the alarm is still pending.
  now="$(dev_now)"; [[ $(( end / 1000 - 10 - now )) -gt 0 ]] && sleep $(( end / 1000 - 10 - now ))
  st="$(proc_state)"
  sh_ dumpsys activity processes | grep -E "Proc #.*:$APP_ID" >"$OUT/logs/restbg-$c-proc.txt"
  observe $S "$c: app 10 s before the rest end" "state '$st' (pid '$(app_pid)'); logs/restbg-$c-proc.txt"
  if [[ "$mode" == allow ]]; then want=30; else want=900; fi
  wait_for $(( end / 1000 + want + 5 - $(dev_now) )) rest_done_posted "restbg-$c-done"
  sb_shot "restbg-$c-done"
  pid1="$(app_pid)"
  if [[ "$(nfield "restbg-$c-done" "r['extras'].get('android.title')")" == "${STR[log_rest_notif_done_title]}" ]]; then
    delay=$(( ( $(nfield "restbg-$c-done" "r['when']") - end ) / 1000 ))
    expect $S "$c: '${STR[log_rest_notif_done_title]}' arrives in the background (app-op $mode), ${delay} s after the rest end (limit ${want} s)" \
      "text '$(nfield "restbg-$c-done" "r['extras'].get('android.text')")'; channel $(nfield "restbg-$c-done" "r['channel']"); pid $pid0 → ${pid1:-none}; logs/restbg-$c-done-notif.txt shots/restbg-$c-done.png" \
      eval '[[ "$(nfield "restbg-$c-done" "r[\"channel\"]")" == "$REST_DONE_CHANNEL" && $delay -ge -1 && $delay -le $want ]]'
  else
    check FAIL $S "$c: '${STR[log_rest_notif_done_title]}' arrives in the background (app-op $mode)" "nothing ${want} s after the rest end; $(cat "$OUT/logs/restbg-$c-done-notif.json")"
  fi
  sh_ logcat -d | grep -E "AlarmManager.*$APP_ID|$APP_ID.*(frozen|freez)" | tail -5 >"$OUT/logs/restbg-$c-alarm-logcat.txt"
}
sc_restbg() {
  fresh "$LIFT_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL restbg "seeded boot" "Today never rendered"; return; }
  rest_bg exact allow
  rest_bg inexact deny
  rest_bg killed allow kill
  sh_ appops set "$APP_ID" SCHEDULE_EXACT_ALARM default
}

promo_settings() {
  adb_ shell am start -a android.settings.APP_NOTIFICATION_PROMOTION_SETTINGS --es android.provider.extra.APP_PACKAGE "$APP_ID" >/dev/null
  wait_for 5 settings_up
}
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
switch_is() { [[ "$(promo_switch "$1" | cut -d' ' -f3)" == "$2" ]]; } # switch_is DUMP true|false
profile_notice() { # profile_notice NAME → 0 when the notice row is on Profile
  tap_nav "${STR[main_dest_profile]}"
  scroll_to eq "${STR[live_updates_off_caption]}" 10; local rc=$?
  evidence "$1"; return $rc
}
sc_toggle() {
  local S=toggle r p s notice
  fresh "$RUN_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }
  if [[ $API -lt 36 ]]; then
    profile_notice toggle-profile-old
    expect $S "API $API: no Live Updates notice on Profile" "shots/toggle-profile-old.png" lacks_text eq "${STR[live_updates_off_caption]}"
    return
  fi
  promo_settings
  r="$(promo_switch toggle-settings-on)" || { shot toggle-settings-on; check FAIL $S "promotion settings screen" "no switch; ui/toggle-settings-on.xml"; return; }
  shot toggle-settings-on
  read -r -a p <<<"$r"
  [[ "${p[2]}" == true ]] && { tap_xy "${p[0]}" "${p[1]}"; wait_for 5 switch_is toggle-settings-off false; }
  r="$(promo_switch toggle-settings-off)"; shot toggle-settings-off
  expect $S "Live Updates switched off in system settings" "switch checked=$(cut -d' ' -f3 <<<"$r"); shots/toggle-settings-off.png" eval '[[ "$(cut -d" " -f3 <<<"$r")" == false ]]'
  stop_app; launch; wait_screen today 20
  open_tracker
  wait_for 10 eval 'notif toggle-off-run "$RUN_NOTIF_ID" | grep -qv "^null$"'
  expect $S "off: the run notification is not promoted" "flags $(nfield toggle-off-run "'|'.join(r['flags'])")" test "$(promoted toggle-off-run)" = False
  boot_day "$RUN_DAY"
  profile_notice toggle-profile-off
  expect $S "off: Profile shows the Live Updates notice" "shots/toggle-profile-off.png ui/toggle-profile-off.xml" has_text eq "${STR[live_updates_off_caption]}"
  # The notice sits at the bottom of Profile; its button can be below the fold.
  if scroll_to eq "${STR[live_updates_open_settings]}" 3 && tap_node eq "${STR[live_updates_open_settings]}"; then
    wait_for 5 settings_up
    r="$(promo_switch toggle-opened)"; shot toggle-opened
    expect $S "'${STR[live_updates_open_settings]}' opens the system Live Updates screen" "focus $(focus); switch '$r'; shots/toggle-opened.png" \
      eval '[[ "$(focus)" == com.android.settings* && -n "$r" ]]'
    read -r -a p <<<"$r"
    tap_xy "${p[0]}" "${p[1]}"; wait_for 5 switch_is toggle-back-on true
    r="$(promo_switch toggle-back-on)"
    adb_ shell input keyevent 4; wait_for 5 app_up
    s="$(focus)"
    notice=gone; has_text eq "${STR[live_updates_off_caption]}" && notice=shown
    evidence toggle-profile-on
    expect $S "back on + return to the app: the notice is gone" "switch now '$r'; notice $notice; focus $s; shots/toggle-profile-on.png" \
      eval '[[ "$notice" == gone && "$s" == "$APP_ID"* ]]'
  else
    check FAIL $S "'${STR[live_updates_open_settings]}' opens the system Live Updates screen" "no ${STR[live_updates_open_settings]} button; shots/toggle-profile-off.png"
  fi
  # Leave Live Updates on for later checks.
  promo_settings; r="$(promo_switch toggle-final)"; read -r -a p <<<"$r"
  [[ "${p[2]}" == false ]] && tap_xy "${p[0]}" "${p[1]}"
  home
}

REM_ID=$REMINDER_NOTIF_ID
REM_PIN=1234
REM_ALARM=""
# rem_text TITLE LINE ACTION: the "title<TAB>line<TAB>action" a reminder shows (rem_case).
rem_text() { printf '%s\t%s\t%s' "$@"; }
rem_open_lift() { snapshot | python3 -c "import json,sys;print(next(i['name'] for i in json.load(sys.stdin)['items'] if i['kind']=='Lift' and not i['done']))"; }
# seed_basic plus the lift day's sets from 06:00, all but the last Cable row set.
seed_rem_lift_left() {
  local t i
  seed_basic "$SEED_DAY" 2
  t="$(TZ="$TZ0" date -d "$LIFT_DAY 06:00" +%s)"
  for i in 1 2 3 4 5; do seed_set "Back Squat" 117.5 3 "$t"; t=$((t + 180)); done
  for i in 1 2 3; do seed_set Deadlift 140.0 5 "$t"; t=$((t + 180)); done
  for i in 1 2 3; do seed_set "Bench press" 90.0 5 "$t"; t=$((t + 180)); done
  for i in 1 2; do seed_set "Cable row" 60.0 5 "$t"; t=$((t + 180)); done
}
# seed_basic plus the run day's run, logged at 07:00.
seed_rem_run_done() {
  local y; y="$(TZ="$TZ0" date -d "$RUN_DAY 07:00" +%s)"
  seed_basic "$SEED_DAY" 2 && seed_run 5.5 31.0 "$y"
}
# rem_forget: with the app stopped, drop SessionReminder's record of the local
# days it already reminded about (at most one post per day), so that a case can
# replay a day an earlier case posted for. `cat >` keeps the file's owner and
# SELinux label.
rem_forget() {
  local f="/data/data/$APP_ID/shared_prefs/$REMINDER_PREFS.xml"
  stop_app
  adb_ shell "[ ! -f $f ] || { grep -v $REMINDED_DAYS_KEY $f >/data/local/tmp/emu-rem.xml && cat /data/local/tmp/emu-rem.xml >$f && rm /data/local/tmp/emu-rem.xml; }" ||
    log "rem_forget: $f not rewritten"
}
# rem_reseed DAY SEED-FN [ARG]: rem_forget, then reseed.
rem_reseed() { rem_forget; reseed "$@"; }
alarm_at() { [[ "$(rem_alarm)" == "$1"* ]]; }            # alarm_at "DAY HH:MM"
alarm_past() { local a; a="$(rem_alarm)"; [[ -n "$a" && "$a" != "$1"* ]]; }
# rem_fire NAME DAY [MODE]: background the app and move the clock to DAY
# 07:44:20 (the reminder is set for 07:45), then wait (max 5 min) until the
# alarm has gone off: it then points past DAY 07:45. MODE `keep` leaves app and
# clock alone; `locked` sets a PIN and turns the screen off before the wait (a
# reminder seen in the shade shows only as an icon on the API 37 lock screen).
# Saves the alarm dumps and the notification (logs/rem-NAME-*); 1 when the
# alarm was not set for DAY 07:45 or never went off (REM_ALARM says which).
rem_fire() {
  local a b t0
  case "${3:-}" in
    keep) ;;
    locked) home; pin_clock "$2" 07:44:20; sh_ locksettings set-pin $REM_PIN >/dev/null; adb_ shell input keyevent 26; wait_for 5 screen_off ;;
    *) home; pin_clock "$2" 07:44:20 ;;
  esac
  wait_for 10 alarm_at "$2 07:45"
  sh_ dumpsys alarm >"$OUT/logs/rem-$1-alarm-before.txt"
  b="$(rem_alarm)"
  [[ "$b" == "$2 07:45"* ]] || { REM_ALARM="not set for $2 07:45 (next '$b'); logs/rem-$1-alarm-before.txt"; return 1; }
  t0=$SECONDS
  wait_for 300 alarm_past "$2 07:45"
  a="$(rem_alarm)"
  # The receiver posts (or not) right after the alarm: 4 s to do it.
  sleep 4
  sh_ dumpsys alarm >"$OUT/logs/rem-$1-alarm-after.txt"
  notif "rem-$1" "$REM_ID" >/dev/null
  if [[ "${3:-}" != locked ]]; then
    shade_open "rem-$1-shade"; shot "rem-$1-shade"; adb_ shell cmd statusbar collapse
  fi
  REM_ALARM="next '$b' → '$a' (~$((SECONDS - t0)) s, device $(sh_ "date '+%F %T %Z'")); logs/rem-$1-alarm-{before,after}.txt shots/rem-$1-shade.png"
  [[ -n "$a" && "$a" != "$2 07:45"* ]]
}
# rem_case CASE DAY WANT [MODE]: rem_fire, then the reminder must be posted
# with WANT (rem_text) on the session_reminder channel (category reminder,
# public on the lock screen, exactly one action), or nothing when WANT is empty.
rem_case() {
  local c="$1" n t l a gt gl ga meta
  if ! rem_fire "$c" "$2" "${4:-}"; then
    check FAIL "$S" "$c: the reminder alarm goes off at 07:45" "$REM_ALARM"; return 1
  fi
  n="$(cat "$OUT/logs/rem-$c-notif.json")"
  if [[ -z "$3" ]]; then
    expect "$S" "$c: alarm went off, nothing posted" "notification $n; alarm $REM_ALARM" test "$n" = null
    return
  fi
  IFS=$'\t' read -r t l a <<<"$3"
  gt="$(nfield "rem-$c" "r['extras'].get('android.title')")"
  gl="$(nfield "rem-$c" "r['extras'].get('android.text')")"
  ga="$(nfield "rem-$c" "'|'.join(r['actions'])")"
  meta="$(nfield "rem-$c" "' '.join(str(r[k]) for k in ('channel', 'category', 'vis'))")"
  expect "$S" "$c: reminder posted: '$t' / '$l' / one action '$a'" \
    "title '$gt', text '$gl', actions '$ga', channel category vis '$meta'; alarm $REM_ALARM; logs/rem-$c-notif.txt" \
    eval '[[ "$gt" == "$t" && "$gl" == "$l" && "$ga" == "$a" && "$meta" == "$REMINDER_CHANNEL reminder PUBLIC" ]]'
}
# rem_lock ACTION TITLE WANT-SCREEN: after rem_fire's `locked` mode, screen on;
# tap ACTION on the reminder on the lock screen and enter the PIN on the
# bouncer. The app must then open WANT-SCREEN. On the way to ACTION: the
# reminder's Expand button when it is collapsed, and the icon shelf when it
# sits there (API 37 shows one notification in full, the rest as icons).
# The action is looked up after the reminder's title: the Quick Settings tile
# above it can carry the same label ("Start run"). Clears the PIN.
rem_lock() {
  local e r="" p s="" bouncer="" i d=rem-lock want="$3"
  adb_ shell input keyevent 224; wait_for 5 screen_on
  for i in 1 2 3 4; do
    wdump "$d"; shot "$d"
    r="$(q eq "$1" "$2" <"$OUT/ui/$d.xml")" && break
    # The keyguard's shade can keep its pre-expansion tree in the dump; the
    # screenshot then shows the action (as in tap_shade).
    r="$(ocr_find "$d" "${1%% *}")" && { log "rem_lock: '$1' located by OCR at $r on shots/$d.png"; break; }
    r=""
    e="$(wfind "$d" expand "$2")" || e="$(wfind "$d" pre "${STR[app_name]} notification")" || break
    read -r -a p <<<"$e"; tap_xy "${p[0]}" "${p[1]}"
    d="rem-lock-$i"
  done
  if [[ -n "$r" ]]; then
    read -r -a p <<<"$r"; tap_xy "${p[0]}" "${p[1]}"
    wait_for 5 bouncer_up
    # The PIN bouncer is a secure window (blank screenshot); its state is read
    # from the window dump: still locked, app not in front, bouncer up.
    sh_ dumpsys window >"$OUT/logs/rem-lock-bouncer-window.txt"
    shot rem-lock-bouncer
    bouncer="$(grep -m1 -oE '(isKeyguardShowing|mKeyguardShowing)=true' "$OUT/logs/rem-lock-bouncer-window.txt") $(grep -m1 -oE 'mBouncerVisible=true|bouncer_visible' "$OUT/logs/rem-lock-bouncer-window.txt"); focus $(focus)"
    [[ "$bouncer" == *KeyguardShowing=true\ * && "$bouncer" != *"focus $APP_ID"* ]] || bouncer="not shown ($bouncer)"
    adb_ shell input text $REM_PIN; adb_ shell input keyevent 66
    wait_screen "$3" 10; s="$(screen)"
  else
    s="(no '$1' on the lock screen; ui/$d.xml)"
  fi
  evidence rem-lock-unlocked
  sh_ locksettings clear --old $REM_PIN >/dev/null
  expect "$S" "lock screen: '$1' asks for the PIN, then opens $3" \
    "before the PIN: '${bouncer:-action not tapped}'; after: screen '$s'; action on shots/$d.png; logs/rem-lock-bouncer-window.txt shots/rem-lock*.png shots/rem-lock-unlocked.png" \
    eval '[[ "$bouncer" == *KeyguardShowing=true* && "$bouncer" != "not shown"* && "$s" == "$want" ]]'
}
reboot_device() {
  adb_ reboot
  wait_for 60 eval '! _booted'
  wait_for 300 _booted || die "no boot after adb reboot"
  root_adbd
  kill "$LOGCAT_PID"
  logcat_capture
}
sc_reminder() {
  # Its own week: a clock pinned before the system image's build date (API 37:
  # Oct 2026) is moved forward to it at boot, which the reboot case needs.
  local S=reminder SEED_DAY=2026-11-01 RUN_DAY=2026-11-02 REST_DAY=2026-11-03 LIFT_DAY=2026-11-04
  local w lift_w t l a r p s rc ex a0 a1 n want t0 move_rx
  fresh "$RUN_DAY" seed_basic "$SEED_DAY" 2 || { check FAIL $S "seeded boot" "Today never rendered"; return; }

  mark reminder-profile
  tap_nav "${STR[main_dest_profile]}"
  scroll_to eq "${STR[reminder_label]}" 12
  # The switch sits at the fold: one more swipe brings up its Time row too.
  swipe_content
  evidence rem-profile-off
  q on "${STR[reminder_label]}" <"$OUT/ui/rem-profile-off.xml"; rc=$?
  a0="$(rem_alarm)"
  expect $S "Profile › During training: '${STR[reminder_label]}' off by default, no Time row, no alarm" \
    "switch rc $rc (1 = off); alarm '$a0'; shots/rem-profile-off.png" \
    eval '[[ $rc -eq 1 && -z "$a0" ]] && ! q eq "${STR[reminder_time_label]}" <"$OUT/ui/rem-profile-off.xml" >/dev/null'
  tap_node eq "${STR[reminder_label]}"; wait_for 5 selected "${STR[reminder_label]}"
  evidence rem-profile-on
  q on "${STR[reminder_label]}" <"$OUT/ui/rem-profile-on.xml"; rc=$?
  a1="$(rem_alarm)"
  expect $S "switched on: Time row '7:30 AM', alarm at the next 07:30" \
    "switch rc $rc (0 = on); alarm '$a1' (clock $RUN_DAY 08:00); shots/rem-profile-on.png" \
    eval '[[ $rc -eq 0 && "$a1" == "$REST_DAY 07:30"* ]] && q re "^7:30\sAM$" <"$OUT/ui/rem-profile-on.xml" >/dev/null'
  tap_node eq "${STR[reminder_time_label]}"; wait_text eq "7 o'clock" 5
  evidence rem-picker
  r="$(find_node eq "7 o'clock" PM)" && { read -r -a p <<<"$r"; tap_xy "${p[0]}" "${p[1]}"; wait_text eq "45 minutes" 5; }
  tap_node eq "45 minutes"
  evidence rem-picker-set
  tap_node eq "${STR[log_ok]}"; wait_gone eq "${STR[log_ok]}" 5
  evidence rem-profile-time
  a1="$(rem_alarm)"
  expect $S "time picker (dial 7, then 45, OK): row '7:45 AM', alarm moved to 07:45" \
    "alarm '$a1'; shots/rem-picker.png shots/rem-picker-set.png shots/rem-profile-time.png" \
    eval '[[ "$a1" == "$REST_DAY 07:45"* ]] && q re "^7:45\sAM$" <"$OUT/ui/rem-profile-time.xml" >/dev/null'

  # Run day: posted with the app in the background; its action from the lock
  # screen, which takes the reminder down. The seed's Easy run has no pace
  # target: the line is its distance.
  mark reminder-run
  w="$(rem_text "$(snap_field title)" "$(snap_run_detail)" "${STR[reminder_action_start_run]}")"; IFS=$'\t' read -r t l a <<<"$w"
  rem_case run "$RUN_DAY" "$w" locked
  mark reminder-lock
  rem_lock "$a" "$t" track
  # Pre-start: Start offered, no Stop (the locate preview may run its service).
  expect $S "run: the action lands on the tracker before the start (not recording)" \
    "Start on screen, no Stop; ui/rem-lock-unlocked.xml" \
    eval 'q eq "${STR[track_btn_start]}" <"$OUT/ui/rem-lock-unlocked.xml" >/dev/null && ! q eq "${STR[track_btn_stop]}" <"$OUT/ui/rem-lock-unlocked.xml" >/dev/null'
  n="$(notif rem-run-acted "$REM_ID")"
  expect $S "run: the action takes the reminder down" "notification after the action: $n; logs/rem-run-acted-notif.txt" test "$n" = null
  # A locating tracker keeps the screen against an open intent: leave it first.
  adb_ shell input keyevent 4; wait_screen today 5

  # A moved session takes a posted reminder down.
  mark reminder-move
  rem_reseed "$RUN_DAY" seed_basic "$SEED_DAY" 2
  rem_case run-move "$RUN_DAY" "$w"
  n="$(cat "$OUT/logs/rem-run-move-notif.json")"
  open_dest "${DEST[Today]}"
  wait_screen today 15
  r=""
  move_rx="${RX[main_move_to_day]}"
  if scroll_to eq "${STR[main_move_short]}" 6 && tap_node eq "${STR[main_move_short]}" && wait_text re "$move_rx" 5; then
    evidence rem-move-dialog
    r="$(node_label re "$move_rx")"
    [[ -n "$r" ]] && tap_node re "$move_rx"
  fi
  wait_for 10 eval '[[ "$(snap_field kind)" == Rest ]]'
  notif rem-moved "$REM_ID" >/dev/null; evidence rem-moved
  expect $S "plan change (Today › Move › '${r:-no option}'): the posted reminder is removed" \
    "before the move: $([[ "$n" != null ]] && echo posted || echo 'not posted'); after: $(cat "$OUT/logs/rem-moved-notif.json"); snapshot kind '$(snap_field kind)'; shots/rem-move-dialog.png shots/rem-moved.png" \
    eval '[[ "$n" != null && -n "$r" && "$(cat "$OUT/logs/rem-moved-notif.json")" == null && "$(snap_field kind)" == Rest ]]'

  # Lift day: the action from the shade takes the reminder down; the same
  # day's alarm again posts nothing; then a content tap. The seed's lift day has
  # four lifts, Back Squat first (seed_rem_lift_left logs them in that order).
  mark reminder-lift
  rem_reseed "$LIFT_DAY" seed_basic "$SEED_DAY" 2
  w="$(rem_text "$(snap_field title)" "$(fmt reminder_line_exercises.other 4 "Back Squat")" "${STR[reminder_action_log_first_set]}")"
  lift_w="$w"; IFS=$'\t' read -r t l a <<<"$w"; ex="$(rem_open_lift)"
  rem_case lift "$LIFT_DAY" "$w"
  s="(no '$a' in the shade)"
  tap_shade rem-lift-action eq "$a" "$t" && { wait_screen log_set 10; s="$(screen)"; }
  evidence rem-lift-action
  n="$(notif rem-lift-acted "$REM_ID")"
  expect $S "lift: '$a' (shade) opens the set editor at '$ex' and takes the reminder down" \
    "screen '$s'; notification after: $n; shots/rem-lift-action.png" \
    eval '[[ "$s" == log_set && "$n" == null ]] && q eq "$(fmt log_change_exercise_cd "$ex")" <"$OUT/ui/rem-lift-action.xml" >/dev/null'
  editor_x
  mark reminder-once
  reseed "$LIFT_DAY" seed_basic "$SEED_DAY" 2
  rem_case once "$LIFT_DAY" ""
  mark reminder-content
  rem_reseed "$LIFT_DAY" seed_basic "$SEED_DAY" 2
  rem_case content "$LIFT_DAY" "$w"
  home
  s="(no '$l' in the shade)"
  tap_shade rem-lift-content eq "$l" "$t" && { wait_screen today 10; s="$(screen)"; }
  evidence rem-lift-content
  n="$(notif rem-lift-tapped "$REM_ID")"
  expect $S "content tap opens Today and clears the reminder" "screen '$s'; notification after: $n; shots/rem-lift-content.png" \
    eval '[[ "$s" == today && "$n" == null ]]'

  # Lift day under way: "Log next set" opens the editor at the next exercise;
  # the last set logged in an editor opened from the app finishes the session,
  # which takes a posted reminder down.
  mark reminder-lift-next
  rem_reseed "$LIFT_DAY" seed_rem_lift_left
  w="$(rem_text "$t" "$(fmt reminder_line_progress 3 4 "Cable row")" "${STR[reminder_action_log_next_set]}")"; IFS=$'\t' read -r t l a <<<"$w"; ex="$(rem_open_lift)"
  rem_case lift-next "$LIFT_DAY" "$w"
  s="(no '$a' in the shade)"
  tap_shade rem-next-action eq "$a" "$t" && { wait_screen log_set 10; s="$(screen)"; }
  evidence rem-next-action
  n="$(notif rem-next-acted "$REM_ID")"
  expect $S "lift under way: '$a' opens the set editor at the next exercise '$ex' and takes the reminder down" \
    "screen '$s'; notification after: $n; shots/rem-next-action.png" \
    eval '[[ "$s" == log_set && "$n" == null ]] && q eq "$(fmt log_change_exercise_cd "$ex")" <"$OUT/ui/rem-next-action.xml" >/dev/null'
  editor_x
  mark reminder-lift-done
  rem_reseed "$LIFT_DAY" seed_rem_lift_left
  rem_case lift-left "$LIFT_DAY" "$w"
  n="$(cat "$OUT/logs/rem-lift-left-notif.json")"
  open_dest "${DEST[LogSet]}"
  wait_screen log_set 15
  r="$(node_label re "$LOG_SET_RX")"
  tap_log_set
  wait_for 10 eval '[[ "$(snap_field sessionDone)" == True ]]'
  evidence rem-next-logged
  notif rem-done-removed "$REM_ID" >/dev/null
  expect $S "session done (last set, '$r', logged in the editor the app opened): the reminder is removed" \
    "before: $([[ "$n" != null ]] && echo posted || echo 'not posted'); after: $(cat "$OUT/logs/rem-done-removed-notif.json"); snapshot sessionDone $(snap_field sessionDone); shots/rem-next-logged.png" \
    eval '[[ "$n" != null && "$(cat "$OUT/logs/rem-done-removed-notif.json")" == null && "$(snap_field sessionDone)" == True ]]'
  editor_x

  # Nothing on a rest day, under a hold, once the session is done.
  mark reminder-rest
  rem_reseed "$REST_DAY" seed_basic "$SEED_DAY" 2
  expect $S "rest: today is a plan rest day" "snapshot kind '$(snap_field kind)', restDay $(snap_field restDay)" test "$(snap_field kind)" = Rest
  rem_case rest "$REST_DAY" ""
  mark reminder-hold
  rem_reseed "$RUN_DAY" seed_hold "$RUN_DAY"
  expect $S "hold: a pain report pauses training" "snapshot kind '$(snap_field kind)'" test "$(snap_field kind)" = Paused
  rem_case hold "$RUN_DAY" ""
  mark reminder-done
  rem_reseed "$RUN_DAY" seed_rem_run_done
  expect $S "done: today's run is logged" "snapshot kind '$(snap_field kind)', sessionDone $(snap_field sessionDone)" test "$(snap_field sessionDone)" = True
  rem_case "done" "$RUN_DAY" ""

  # The app last ran on Tue (rest day); the clock moves to Wed morning: the lift
  # day's reminder, from Tue's snapshot.
  mark reminder-ahead
  rem_reseed "$REST_DAY" seed_basic "$SEED_DAY" 2
  rem_case ahead "$LIFT_DAY" "$lift_w"
  expect $S "ahead: posted from Tue's snapshot (its day ahead), app not opened" \
    "snapshot epochDay $(snap_field epochDay) (Tue $(epoch_day "$REST_DAY")), stale $(snap_field stale); focus '$(focus)'" \
    eval '[[ "$(snap_field epochDay)" == "$(epoch_day "$REST_DAY")" && "$(focus)" != "$APP_ID"* ]]'

  # Time zone change with the app process alive in the background (not
  # opened): Berlin 08:43 (today's 07:45 has passed) → London 07:43 (it is
  # ahead again).
  mark reminder-tz
  rem_reseed "$LIFT_DAY" seed_basic "$SEED_DAY" 2
  home
  want="$(date -d "$LIFT_DAY +1 day" +%F) 07:45"
  pin_clock "$LIFT_DAY" 08:43; wait_for 10 alarm_at "$want"; a0="$(rem_alarm)"
  sh_ cmd alarm set-timezone Europe/London >/dev/null; wait_for 10 alarm_at "$LIFT_DAY 07:45"; a1="$(rem_alarm)"
  expect $S "time zone change (Berlin 08:43 → London 07:43), app in the background: alarm moved to 07:45 London" \
    "next '$a0' → '$a1'; device $(sh_ "date '+%F %T %Z'")" \
    eval '[[ "$a0" == "$want"* && "$a1" == "$LIFT_DAY 07:45"* ]]'
  observe $S "tz: today's snapshot after the change (app process alive, not opened)" \
    "stale $(snap_field stale), zone '$(snap_field zone)'; app pid '$(app_pid)'"
  rem_case tz "$LIFT_DAY" "$lift_w" keep
  # The same change with no app process: the receivers' process holds no model,
  # so today's snapshot stays stale and nothing posts until the app is opened.
  mark reminder-tz-cold
  # The force stop takes down the reminder the last case posted (and its alarm,
  # which the launch sets again); rem_forget makes the stale snapshot the only
  # reason nothing posts.
  rem_forget
  pin_clock "$LIFT_DAY" 08:43; launch; wait_screen today 20; home
  n="$(app_pid)"; stop_app sigkill
  # The receivers' answer to the change: 3 s.
  sh_ cmd alarm set-timezone Europe/London >/dev/null; sleep 3
  observe $S "tz-cold: app process killed before the change, not opened (by design nothing posts: today's snapshot is stale)" \
    "pid '$n' killed; snapshot stale $(snap_field stale), zone '$(snap_field zone)'; alarm '$(rem_alarm)'"
  rem_case tz-cold "$LIFT_DAY" "" keep

  # Reboot: the boot broadcast sets the alarm again; it fires without the app.
  # The clock after boot is either the pinned one (API 37: it is past the
  # image's build date) or the host's (API 34); the alarm must be the next 07:45
  # by that clock. A host clock is then moved back to the lift day (its own
  # TIME_SET also sets the alarm) before waiting for it.
  mark reminder-reboot
  pin_clock "$LIFT_DAY" 08:00
  rem_reseed "$LIFT_DAY" seed_basic "$SEED_DAY" 2
  home
  pin_clock "$LIFT_DAY" 07:42; wait_for 10 alarm_at "$LIFT_DAY 07:45"; a0="$(rem_alarm)"
  reboot_device
  # The boot broadcast reaches the app a little after boot_completed.
  t0=$SECONDS
  wait_for 120 alarm_set; a1="$(rem_alarm)"
  n="$(sh_ "date '+%F %H%M'")"
  if (( 10#${n#* } < 745 )); then want="${n%% *} 07:45"; else want="$(date -d "${n%% *} +1 day" +%F) 07:45"; fi
  expect $S "reboot: the alarm is set again after boot, for the next 07:45 by the device clock (app not opened)" \
    "before '$a0', after boot '$a1' (want '$want', ~$((SECONDS - t0)) s after boot_completed); device $(sh_ "date '+%F %T %Z'"), uptime $(sh_ cut -d. -f1 /proc/uptime) s; focus '$(focus)'" \
    eval '[[ "$a1" == "$want"* && "$(focus)" != "$APP_ID"* ]]'
  wake
  if [[ "$want" == "$LIFT_DAY 07:45" ]]; then rem_case reboot "$LIFT_DAY" "$lift_w" keep; else rem_case reboot "$LIFT_DAY" "$lift_w"; fi
}

trap session_cleanup EXIT
config_run
config apk "$APK ($(date -r "$APK" -Is))"
config scenarios "$ONLY"
session_start "$AVD" 3072
install_apk "$APK" -r >/dev/null || die "install failed"

for sc in $ONLY; do
  mark "scenario-$sc"
  if declare -F "sc_$sc" >/dev/null; then "sc_$sc"; else check FAIL "$sc" "unknown scenario" "no function sc_$sc"; fi
  stop_feeder
  adb_ shell cmd statusbar collapse
  shot "end-$sc"
done
session_end "System surfaces report"
