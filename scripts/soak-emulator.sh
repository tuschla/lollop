#!/usr/bin/env bash
# Long-term on-device soak test: hunts crashes, ANRs, leaks and tracking bugs that
# only show over a long session on a real Android runtime.
#
#   (a) seeded monkey passes restricted to the app (several seeds, chunked so
#       the app is relaunched if a stray event ever leaves it), each followed by a
#       cold relaunch that replays the event log the monkey produced;
#   (b) a long GPS-tracked run: a deterministic, realistic route fed through
#       `adb emu geo fix` at 1 Hz while the run survives screen off/on, the app
#       backgrounded/foregrounded, a pocketed phone under forced Doze, the
#       notification shade, an in-app pause, a notification-action pause and a
#       traffic-light stop; then Finish + Save, and the saved run (event log +
#       History row + run detail) is compared with the fed route;
#   (c) `dumpsys meminfo` + the tracking foreground-service state sampled
#       throughout (samples.csv);
#   (d) a full-session logcat scanned for FATAL EXCEPTION, native aborts, ANRs,
#       StrictMode, foreground-service failures and app error lines;
#   (e) a summary report ($OUT/REPORT.md) with PASS/FAIL checks.
#
#   scripts/soak-emulator.sh [options]
#     --serial S            adb serial (default emulator-5580)
#     --avd NAME            AVD booted when the serial is offline (default e34)
#     --out DIR             output directory, wiped per run (default /tmp/Soak/run)
#     --apk PATH            APK to install (default: the debug APK)
#     --seeds "A B C"       monkey seeds (default "42 1337 9001")
#     --events N            monkey events for the first seed (default 20000)
#     --events-other N      monkey events for every other seed (default 5000)
#     --throttle MS         monkey throttle (default 60)
#     --chunk N             monkey events per chunk (default 1000)
#     --gps-minutes N       recorded length of the tracked run, >= 8 (default 30)
#     --sample-sec N        memory/service sample period (default 20)
#     --skip-monkey | --skip-gps
#     --keep-emulator       leave the emulator this script booted running at the end
#
# Exit status: 0 when every check passed, 1 when any check failed (see REPORT.md).
# Requires: Android SDK (sdk.dir in android/local.properties or $ANDROID_HOME),
# python3, cargo (the core's figures of the saved run:
# shared/examples/track_figures), KVM and an x86_64 google_apis AVD
# (Google Play services for fused location; adb root, which lib.sh's event-log
# push/pull use).
set -uo pipefail

# shellcheck source-path=SCRIPTDIR source=emu-scenarios/lib.sh
source "$(dirname "$0")/emu-scenarios/lib.sh"
SERIAL="emulator-$PORT"
OUT="/tmp/Soak/run"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
SEEDS="42 1337 9001"
EVENTS_FIRST=20000
EVENTS_OTHER=5000
THROTTLE=60
CHUNK=1000
GPS_MINUTES=30
SAMPLE_SEC=20
DO_MONKEY=1
DO_GPS=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --serial) SERIAL="$2"; shift 2 ;;
    --avd) AVD="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --apk) APK="$2"; shift 2 ;;
    --seeds) SEEDS="$2"; shift 2 ;;
    --events) EVENTS_FIRST="$2"; shift 2 ;;
    --events-other) EVENTS_OTHER="$2"; shift 2 ;;
    --throttle) THROTTLE="$2"; shift 2 ;;
    --chunk) CHUNK="$2"; shift 2 ;;
    --gps-minutes) GPS_MINUTES="$2"; shift 2 ;;
    --sample-sec) SAMPLE_SEC="$2"; shift 2 ;;
    --skip-monkey) DO_MONKEY=0; shift ;;
    --skip-gps) DO_GPS=0; shift ;;
    --keep-emulator) KEEP_EMU=1; shift ;;
    -h|--help) awk 'NR>1&&/^#/{print;next}NR>1{exit}' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
[[ "$GPS_MINUTES" -ge 8 ]] || { echo "--gps-minutes must be >= 8" >&2; exit 2; }
[[ -f "$APK" ]] || { echo "APK not found: $APK (build it with scripts/build.sh --no-tests)" >&2; exit 2; }

rm -rf "$OUT"
out_init "$OUT"
mkdir -p "$OUT/monkey" "$OUT/gps"
SOAK=(python3 "$HELP/soak.py")

# reset_app seeded|fresh: wipe the app, set the device clock to the host's (the
# seed's dates and the fed route's schedule are host-clock based; a scenario
# script may have left the emulator on a pinned clock), optionally seed the
# event log (3 run days, a squat and a 5 km run yesterday).
reset_app() {
  wipe_app
  sh_ cmd alarm set-timezone "$TZ0" >/dev/null
  sh_ cmd alarm set-time "$(date +%s)000" >/dev/null
  [[ "$1" == seeded ]] || return 0
  seed_basic "$(date -d yesterday +%F)" 3 >"$OUT/logs/seed.ndjson" || die "seed_basic failed"
  seed_app "$OUT/logs/seed.ndjson" || die "could not push the seed event log"
}

sampler() {
  echo "epoch,phase,pid,pss_kb,java_kb,native_kb,graphics_kb,threads,fds,svc,svc_fg,notif" >"$OUT/samples.csv"
  while :; do
    local now ph pid m=() thr="" fds="" sv svc=0 fg=0 title
    now=$(date +%s); ph="$(current_phase)"
    pid="$(app_pid)"; pid="${pid%% *}"
    if [[ -n "$pid" ]]; then
      read -r -a m <<<"$(meminfo pss java native graphics)"
      thr="$(sh_ "run-as $APP_ID cat /proc/$pid/status" | sed -n 's/^Threads:[[:space:]]*//p')"
      fds="$(sh_ "run-as $APP_ID ls /proc/$pid/fd" | wc -l | tr -d ' ')"
    fi
    sv="$(sh_ dumpsys activity services "${COMP[RunTrackingService]}")"
    grep -q 'ServiceRecord{' <<<"$sv" && svc=1
    grep -q 'isForeground=true' <<<"$sv" && fg=1
    title="$(sh_ dumpsys notification --noredact | python3 "$HELP/notif.py" "$APP_ID" "$RUN_NOTIF_ID" |
      python3 -c "import json,sys;r=json.load(sys.stdin);print((r or {}).get('extras',{}).get('android.title') or '')")"
    echo "$now,$ph,$pid,${m[0]:-},${m[1]:-},${m[2]:-},${m[3]:-},$thr,$fds,$svc,$fg,\"${title//\"/}\"" >>"$OUT/samples.csv"
    sleep "$SAMPLE_SEC"
  done
}

monkey_pass() { # seed events state
  local seed="$1" events="$2" state="$3" dir="$OUT/monkey/seed-$1" t_start=$SECONDS relaunch=ok m
  mark "monkey-$seed"
  reset_app "$state"
  launch || log "monkey $seed: app not up before the pass ($(focus))"
  monkey_run "$dir" "$seed" "$events" "$THROTTLE" "$CHUNK"
  monkey_checks monkey "seed $seed ($state)" "monkey/seed-$seed"
  shot "monkey-$seed-end"
  pull_log "$dir/event-log.ndjson"
  # Cold relaunch: replaying whatever the monkey logged must not crash the app.
  stop_app
  { launch && dump_ui >/dev/null && app_up; } || relaunch="FAILED (pid='$(app_pid)' focus='$(focus)')"
  shot "monkey-$seed-relaunch"
  expect monkey "relaunch after monkey $seed" "$relaunch; replayed $(grep -c . "$dir/event-log.ndjson") logged events" test "$relaunch" = ok
  read -r -a m <<<"$MONKEY"
  printf '{"seed":%s,"state":"%s","events":%s,"injected":%s,"crashes":%s,"anrs":%s,"escapes":%s,"finished":%s,"chunks":%s,"minutes":%s,"relaunch":"%s"}\n' \
    "$seed" "$state" "$events" "${m[0]}" "${m[1]}" "${m[2]}" "${m[3]}" "${m[4]}" "${m[5]}" $(( (SECONDS - t_start) / 60 )) "$relaunch" \
    >>"$OUT/monkey/summary.jsonl"
}

G="$OUT/gps"
T0=0
at() { # at T: sleep until T seconds after the feed start (the route's schedule)
  sleep "$(python3 -c "import time;print(max(0.0,$T0+$1-time.time()))")"
}
now_t() { python3 -c "import time;print(int(time.time()-$T0))"; }
# live LABEL: the tracker's live stat readout (its spoken content-desc) into live.csv.
live() {
  local t r cum
  t="$(now_t)"
  r="$(node_label re "${RX[track_cd_stats]}")"
  cum="$(awk -F'\t' -v t="$t" '$1==t{print $7}' "$G/timeline.tsv")"
  echo "$t,$1,\"$r\",$cum" >>"$G/live.csv"
}
# shellcheck disable=SC2329 # called through expect/wait_for
svc_stopped() { ! svc_fg; }

gps_run() {
  local rec=$(( GPS_MINUTES * 60 )) pid0 pid1 summary hist="" row p lon lat alt easy
  mark gps-setup
  "${SOAK[@]}" route "$rec" 7 "$G"
  # shellcheck disable=SC1091
  source "$G/schedule.env"
  echo "t,label,stats,expected_cum_m" >"$G/live.csv"
  reset_app seeded
  { launch && wait_text eq "${STR[main_dest_today]}" 30; } || die "Today never rendered"
  read -r _ lon lat alt _ <<<"$(sed -n '1p' "$G/timeline.tsv")"
  adb_ emu geo fix "$lon" "$lat" "$alt" 8 0 >/dev/null
  open_tracker || die "tracker entry not found"
  T0="$(python3 -c 'import time;print(time.time())')"
  echo "$T0" >"$G/t0"
  start_feeder gps "$G/timeline.tsv" "$T0"
  mark gps-pre
  at "$START_AT"; shot gps-01-before-start
  tap_node eq "${STR[track_btn_start]}" || die "Start not found"
  wait_for 10 svc_fg
  pid0="$(app_pid)"
  expect gps "service foreground after Start" "pid $pid0" svc_fg
  mark gps-recording
  at "$((SCREEN_FROM - 8))"; live before-screen-off
  at "$SCREEN_FROM"; mark gps-screen-off; adb_ shell input keyevent 223
  at "$SCREEN_TO"; wake; mark gps-recording; live after-screen-on; shot gps-02-after-screen-on
  at "$LIGHT_FROM"; mark gps-traffic-light
  at "$LIGHT_TO"; mark gps-recording
  at "$BG_FROM"; mark gps-background; home
  at "$BG_TO"; launch; mark gps-recording; live after-foreground; shot gps-03-after-foreground
  at "$SHADE_FROM"; mark gps-shade
  shade_open gps-04-shade; shot gps-04-shade
  adb_ shell cmd statusbar collapse
  at "$SHADE_TO"; mark gps-recording
  at "$PAUSE_TAP"; mark gps-paused-in-app
  expect gps "in-app pause" "Pause control tapped" tap_node eq "${STR[track_action_pause]}"
  wait_text eq "${STR[track_action_resume]}" 5; live paused-in-app; shot gps-05-paused
  at "$RESUME_TAP"
  expect gps "in-app resume" "Resume control tapped" tap_node eq "${STR[track_action_resume]}"
  mark gps-recording; wait_text eq "${STR[track_action_pause]}" 5; live resumed-in-app
  at "$POCKET_FROM"; mark gps-pocket-doze
  home; adb_ shell input keyevent 223
  adb_ shell dumpsys battery unplug
  adb_ shell dumpsys deviceidle force-idle >"$G/doze.txt" 2>&1 || log "deviceidle force-idle failed (see $G/doze.txt)"
  at "$POCKET_TO"
  adb_ shell dumpsys deviceidle unforce >/dev/null || log "deviceidle unforce failed"
  adb_ shell dumpsys battery reset
  wake; launch; mark gps-recording; live after-pocket-doze; shot gps-06-after-pocket
  at "$NPAUSE_TAP"; mark gps-paused-notification
  expect gps "notification pause" "Pause action on '${STR[track_notif_recording_title]}'" \
    tap_shade gps-07-shade-pause eq "${STR[track_action_pause]}" "${STR[track_notif_recording_title]}"
  shot gps-07-shade-paused
  adb_ shell cmd statusbar collapse; live paused-notification
  at "$NRESUME_TAP"
  expect gps "notification resume" "Resume action on '${STR[track_notif_paused_title]}'" \
    tap_shade gps-07-shade-resume eq "${STR[track_action_resume]}" "${STR[track_notif_paused_title]}"
  adb_ shell cmd statusbar collapse; mark gps-recording; launch; live resumed-notification
  at "$((STOP_AT - 10))"; live before-stop
  at "$STOP_AT"; mark gps-finish
  pid1="$(app_pid)"
  expect gps "process survived the run" "pid $pid0 → $pid1" test "$pid0" = "$pid1"
  expect gps "service foreground at Stop" "after screen-off, background, Doze, shade" svc_fg
  tap_node eq "${STR[track_btn_stop]}" || die "Stop not found"
  wait_text eq "${STR[track_btn_save_run]}" 15 || die "no Finish sheet"
  shot gps-08-finish-sheet
  # A run that follows today's plan opens with its type preselected, and the chip
  # toggles: tap Easy only when it is not already on.
  easy="$(fmt history_workout_type_cd Easy)"
  selected "$easy" || { tap_node eq "$easy"; wait_for 3 selected "$easy"; }
  shot gps-08b-finish-type
  finish_run || die "Save run failed"
  summary="$FINISH"
  shot gps-09-after-save
  mark gps-saved
  expect gps "service stopped after save" "no foreground tracking service" wait_for 10 svc_stopped
  stop_feeder
  pull_log "$G/event-log.ndjson"
  # History › the run's row (its type's title) → run detail.
  mark gps-history
  tap_nav "${STR[main_dest_history]}"
  if scroll_to eq "${STR[history_run_easy]}" 6 && row="$(find_node eq "${STR[history_run_easy]}")"; then
    ui_texts >"$G/history-texts.txt"
    # The row's own figures: the first "X mi · pace · time" line after its title.
    hist="$(awk -v t="${STR[history_run_easy]}" 'f && /^text=[0-9.]+( |\302\240)(mi|km) · /{sub(/^text=/,""); print; exit} $0=="text=" t{f=1}' "$G/history-texts.txt")"
    shot gps-10-history-row
    read -r -a p <<<"$row"; tap_xy "${p[0]}" "${p[1]}"
    wait_text eq "${STR[history_workout_type]}" 10
    shot gps-11-run-detail
    ui_texts >"$G/detail-texts.txt"
    swipe_content; shot gps-12-run-detail-scrolled
    ui_texts >>"$G/detail-texts.txt"
    swipe_content; shot gps-13-run-detail-bottom
  else
    check FAIL gps "history row" "tracked run not found in History"
  fi
  echo "$summary" >"$G/finish-summary.txt"
  echo "$hist" >"$G/history-row.txt"
  # The core's own of-record figures of the saved run (needs cargo; on failure
  # the core checks FAIL with logs in gps/core-cargo.log).
  (cd "$REPO_ROOT" && cargo run -q -p shared --example track_figures -- "$G/event-log.ndjson" "$G/core.json") \
    >"$G/core-cargo.log" 2>&1 || rm -f "$G/core.json"
  "${SOAK[@]}" analyze "$G/event-log.ndjson" "$G/expected.json" "$T0" "$FEED_LOG" "$G/core.json" \
    "$summary" "$hist" "$G/result.json"
  "${SOAK[@]}" checks "$G/result.json" "$OUT/checks.tsv"
}

trap 'kill "${SAMPLER_PID:-}" 2>/dev/null; session_cleanup' EXIT
config_run
config apk "$APK ($(date -r "$APK" -Is))"
config monkey "seeds [$SEEDS], events $EVENTS_FIRST first / $EVENTS_OTHER other, throttle ${THROTTLE} ms, chunk $CHUNK"
config "gps run" "${GPS_MINUTES} min recorded, 1 Hz geo fix"
config sampling "every ${SAMPLE_SEC} s"
session_start "$AVD" 2048
adb_ shell settings put global stay_on_while_plugged_in 0
log "installing $APK"
install_apk "$APK" -r || die "install failed"
mark init
sampler &
SAMPLER_PID=$!

if [[ $DO_MONKEY -eq 1 ]]; then
  first=1
  for s in $SEEDS; do
    if [[ $first -eq 1 ]]; then monkey_pass "$s" "$EVENTS_FIRST" seeded; first=0
    else monkey_pass "$s" "$EVENTS_OTHER" fresh; fi
  done
fi
[[ $DO_GPS -eq 1 ]] && gps_run
mark "done"
kill "$SAMPLER_PID"; wait "$SAMPLER_PID"
"${SOAK[@]}" memory "$OUT/samples.csv" "$OUT/checks.tsv"
"${SOAK[@]}" sections "$OUT"
session_end "lollop on-device soak report"; rc=$?
cp "$OUT/REPORT.md" "$(dirname "$OUT")/REPORT-auto.md" || log "REPORT.md not copied to $(dirname "$OUT")/REPORT-auto.md"
exit $rc
