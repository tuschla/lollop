#!/usr/bin/env bash
# On-device upgrade path: what a user made in an older debug build survives
# `adb install -r` of a newer one (same debug key, data kept).
#   1. OLD: wipe the app, install OLD, boot a seeded profile (the seed_basic log:
#      OLD's boot generates the plan), then through OLD's UI: two sets (set
#      editor), a GPS run (tracker + feeder.py), a pain report (Log › Report pain:
#      left knee, dull ache, 3/10); the Today widget placed and the Quick Settings
#      tile added.
#   2. MID (optional, a build with the session reminder): install -r, then
#      Profile › Session reminder on.
#   3. NEW: install -r, then before and after the first launch: the event log
#      keeps every old event, History lists the same sets and runs, the plan is
#      the same (no new GeneratePlan), no guided setup, the pain report still
#      adjusts today, the widget stays bound and shows today, the tile keeps its
#      label, the reminder switch and its alarm survive.
#   4. Mid-run: a fresh OLD install records a GPS run, NEW is installed over it
#      while recording; the next launch offers to resume the run from OLD's
#      sidecar and saves it once.
#
#   scripts/emu-scenarios/upgrade.sh --old OLD.apk [--mid MID.apk] --new NEW.apk
#                                    [--avd e34] [--port 5580] [--keep-emulator]
#
# Outputs: ~/.cache/emu-upgrade/<avd>/<YYYYmmdd-HHMMSS>/ (REPORT.md, checks.tsv,
# logcat.txt, shots/, ui/, logs/). Clock pinned to Mon 2026-09-21 08:00
# Europe/Berlin (a run day for the seed). Exit 0 when no check FAILed.
set -uo pipefail

cd "$(dirname "$0")/../.."
REPO_ROOT="$(pwd)"
HELP="$REPO_ROOT/scripts/emu-scenarios"
APP_ID="app.milestone"
AVD="e34"
PORT=5580
OLD="" MID="" NEW=""
KEEP_EMU=0
TZ0="Europe/Berlin"
PIN_DATE="2026-09-21"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --avd) AVD="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --old) OLD="$2"; shift 2 ;;
    --mid) MID="$2"; shift 2 ;;
    --new) NEW="$2"; shift 2 ;;
    --keep-emulator) KEEP_EMU=1; shift ;;
    -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
for a in "$OLD" "$NEW" ${MID:+"$MID"}; do [[ -f "$a" ]] || { echo "APK not found: '$a'" >&2; exit 2; }; done

SERIAL="emulator-$PORT"
OUT="$HOME/.cache/emu-upgrade/$AVD/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT"/{shots,ui,logs,seed}
: >"$OUT/checks.tsv"
# shellcheck source=lib.sh
source "$HELP/lib.sh"
EV() { python3 "$HELP/events.py" "$@"; }
S=upgrade

# ── Helpers (as in emulator-scenarios.sh / surfaces.sh) ──────────────────────
home() { adb_ shell input keyevent 3; sleep 2; }
wdump() {
  local i
  for i in 1 2 3; do
    adb_ shell uiautomator dump --windows /sdcard/upg.xml >/dev/null 2>&1 &&
      adb_ exec-out cat /sdcard/upg.xml >"$OUT/ui/$1.xml" 2>/dev/null &&
      grep -q '</displays>' "$OUT/ui/$1.xml" && return 0
    sleep 1
  done
  return 1
}
wfind() { python3 "$HELP/node.py" "$2" "$3" <"$OUT/ui/$1.xml"; }
snapshot() { adb_ exec-out cat "$APP_FILES/widget-today.json" 2>/dev/null; }
snap_kind() { python3 -c 'import json,sys;print(json.load(open(sys.argv[1])).get("kind",""))' "$1" 2>/dev/null; }
rem_alarm() {
  sh_ dumpsys alarm | grep -A3 'walarm\*:app.milestone.action.SESSION_REMINDER' |
    sed -n 's/.*origWhen=\([0-9-]* [0-9:]*\).*/\1/p' | head -1
}
widget_bound() { sh_ dumpsys appwidget | grep -q "cmp:ComponentInfo{$APP_ID/$APP_ID.widget.TodayWidgetReceiver}"; }
widget_texts() { # NAME: the widget's labels on the home screen
  home; sleep 2
  evidence "$1"
  python3 "$HELP/node.py" texts <"$OUT/ui/$1.xml" | sed -n '/content-desc=Open Today in lollop/,$p' | head -12
}
TILE="$APP_ID/.QuickTileService"
tile_label() {
  local i l re='^(Log|Log set|Start run|Run · recording)$'
  adb_ shell cmd statusbar expand-settings; sleep 3
  for i in 1 2 3; do
    wdump "$1" || continue
    l="$(wfind "$1" re "$re" | cut -d' ' -f7-)"
    [[ -n "$l" ]] && { sleep 2; wdump "$1" && l="$(wfind "$1" re "$re" | cut -d' ' -f7-)"; }
    [[ -n "$l" ]] && { adb_ shell cmd statusbar collapse; echo "$l"; return 0; }
    adb_ shell input swipe 900 700 150 700 300; sleep 2
  done
  adb_ shell cmd statusbar collapse
  return 1
}
add_tile() {
  local i
  sh_ cmd statusbar add-tile "$TILE" >/dev/null
  for i in 1 2 3 4 5 6; do sleep 3; tile_label tile-added >/dev/null && return 0; done
  return 1
}
LAUNCHER="com.google.android.apps.nexuslauncher"
drag() {
  local i
  adb_ shell input motionevent DOWN "$1" "$2"; sleep 2
  for i in 1 2 3 4 5 6 7 8; do
    adb_ shell input motionevent MOVE $(( $1 + ($3 - $1) * i / 8 )) $(( $2 + ($4 - $2) * i / 8 ))
  done
  sleep 1
  adb_ shell input motionevent UP "$3" "$4"
}
place_widget() {
  local r p preview='^[0-9]+ wide by [0-9]+ high$'
  adb_ shell pm clear "$LAUNCHER" >/dev/null 2>&1; sleep 3
  home; home; sleep 3
  adb_ shell input swipe 540 1300 540 1300 1500; sleep 2
  tap_node eq Widgets || return 1; sleep 3
  if tap_node eq Search 2; then sleep 1; adb_ shell input text lollop; sleep 3; fi
  scroll_to eq lollop 6 || return 1
  has_text re "$preview" || { tap_node eq lollop; sleep 2; }
  has_text re "$preview" || { tap_node eq Today 2; sleep 2; }
  has_text eq "Add Today widget" || { tap_node re "$preview" 2; sleep 2; }
  if has_text eq "Add Today widget"; then
    tap_node eq "Add Today widget"; sleep 4
  else
    r="$(find_node rid widget_preview_container)" || r="$(find_node rid widget_preview)" || return 1
    read -r -a p <<<"$r"
    drag "${p[0]}" "${p[1]}" 540 800; sleep 4
  fi
  home
  evidence widget-placed
  widget_bound && wait_text eq "Open Today in lollop" 15
}
open_set_editor() {
  tap_node eq Log || return 1; sleep 2
  tap_node eq "Log set" || return 1
  wait_text eq "Exercise" 8 || return 1
  if has_text eq "Search exercises"; then tap_node eq "Back Squat" || return 1; sleep 2; fi
}
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
close_editor() {
  if has_text eq "Done for now"; then tap_node eq "Done for now"; else tap_node eq Close; fi
  sleep 2
  has_text eq Discard && { tap_node eq Discard; sleep 1; }
  has_text eq "Exercise, weight, reps" && { adb_ shell input keyevent 4; sleep 2; }
  return 0
}
log_two_sets() {
  open_set_editor || return 1
  tap_scroll eq "Add 2.5 kg" || return 1
  tap_scroll eq "Reps, 5" || return 1
  tap_scroll eq "2 reps left" || return 1
  tap_top re '^Log set [0-9]+$' || return 1; sleep 3
  tap_top re '^Log set [0-9]+$' || return 1; sleep 3
  evidence old-sets
  close_editor
}
gps_run() {
  start_feeder old-run
  tap_node eq Log || return 1; sleep 2
  tap_node eq "Track run (GPS)" || return 1
  wait_text eq Start 20 || return 1
  sleep 4; tap_node eq Start || return 1
  wait_text eq Stop 20 || return 1
  sleep 60
  tap_node eq Stop || return 1
  wait_text eq "Save run" 15 || return 1
  evidence old-run-finish
  tap_node eq "Save run" || return 1; sleep 5
  stop_feeder
  ! has_text eq "Save run"
}
report_pain() {
  local r p
  tap_node eq Log || return 1; sleep 2
  tap_node eq "Report pain" || return 1
  wait_text eq "Where does it hurt?" 10 || return 1
  tap_node eq "Left knee" || return 1; sleep 1
  tap_node eq "Not sure" || return 1; sleep 2
  tap_node eq "Dull ache that builds with use" || return 1; sleep 2
  # Severity: an 11-step 0–10 row; 3 is the 4th of 11 cells.
  r="$(find_node eq "How bad is it?" optional)" || return 1
  read -r -a p <<<"$r"; tap_xy $(( p[2] + (p[4] - p[2]) * 7 / 22 )) "${p[1]}"; sleep 1
  evidence old-pain-sheet
  r="$(find_node eq "Report pain" "Worse the morning after training")" || return 1
  read -r -a p <<<"$r"; tap_xy "${p[0]}" "${p[1]}"; sleep 4
  ! has_text eq "Where does it hurt?"
}
reminder_on() {
  tap_nav Profile; sleep 2
  scroll_to eq "Session reminder" 12 || return 1
  adb_ shell input swipe 540 1450 540 950 350; sleep 1
  python3 "$HELP/node.py" on "Session reminder" < <(dump_ui) && return 0
  tap_node eq "Session reminder"; sleep 2
  evidence mid-reminder-on
  python3 "$HELP/node.py" on "Session reminder" < <(dump_ui)
}
reminder_is_on() {
  tap_nav Profile; sleep 2
  scroll_to eq "Session reminder" 12 || return 1
  adb_ shell input swipe 540 1450 540 950 350; sleep 1
  evidence "$1"
  python3 "$HELP/node.py" on "Session reminder" <"$OUT/ui/$1.xml"
}
install_r() { # APK TAG
  local r
  r="$(adb_ install -r "$1" 2>&1 | tail -1)"
  [[ "$r" == Success ]] || { adb_ push "$1" /data/local/tmp/upg.apk >/dev/null; r="$(sh_ pm install -r /data/local/tmp/upg.apk | tail -1)"; }
  check INFO $S "install -r $2" "$r ($(basename "$1"), $(sha256sum "$1" | cut -c1-12)); version $(sh_ dumpsys package "$APP_ID" | sed -n 's/.*lastUpdateTime=//p' | head -1)"
  [[ "$r" == Success ]]
}
# Facts of an event log: sets, runs, GPS runs, pain reports, plan lines.
facts() { echo "sets=$(EV count "$1" LogSet) runs=$(EV count "$1" LogRun) gps=$(EV count "$1" LogRunTrack) readiness=$(EV count "$1" SubmitReadiness) plans=$(EV count "$1" GeneratePlan) clearplan=$(EV count "$1" ClearPlan)"; }

# ── Main ─────────────────────────────────────────────────────────────────────
{
  echo "date:   $(date -Is)"
  echo "serial: $SERIAL (AVD $AVD)"
  echo "old:    $OLD ($(sha256sum "$OLD" | cut -c1-16))"
  [[ -n "$MID" ]] && echo "mid:    $MID ($(sha256sum "$MID" | cut -c1-16))"
  echo "new:    $NEW ($(sha256sum "$NEW" | cut -c1-16))"
} >"$OUT/config.txt"
boot_emulator "$AVD" "$OUT/logs/emulator.log" 2048
trap 'touch "$OUT/feed.stop"; kill ${LOGCAT_PID:-} 2>/dev/null; stop_emulator' EXIT
echo "device: $(sh_ getprop ro.product.model), API $(sh_ getprop ro.build.version.sdk)" >>"$OUT/config.txt"
adb_ logcat -c 2>/dev/null || true
adb_ logcat -v threadtime -b main,system,crash,events >"$OUT/logcat.txt" 2>&1 &
LOGCAT_PID=$!

mark old-install
adb_ uninstall "$APP_ID" >/dev/null 2>&1 || true
adb_ install "$OLD" >/dev/null 2>&1 || { adb_ push "$OLD" /data/local/tmp/upg.apk >/dev/null; sh_ pm install /data/local/tmp/upg.apk >/dev/null; }
baseline_device; grant_perms; pin_clock "$PIN_DATE"
launch; sleep 5; adb_ shell am force-stop "$APP_ID"
seed_basic 2026-09-20 2 >"$OUT/seed/basic.ndjson"
push_log "$OUT/seed/basic.ndjson"
launch
expect $S "old build boots the seeded profile" "Today rendered" wait_text pre "TODAY'S SESSION" 40
evidence old-01-today

mark old-data
expect $S "old build: two sets through the set editor" "" log_two_sets
expect $S "old build: a GPS run through the tracker" "" gps_run
expect $S "old build: a pain report through Log › Report pain" "" report_pain
tap_nav Today; sleep 2; evidence old-02-today-after
pull_log "$OUT/logs/old.ndjson"
check INFO $S "old log" "$(facts "$OUT/logs/old.ndjson")"
home
expect $S "old build: widget placed" "" place_widget
OLD_WIDGET="$(widget_texts old-03-widget)"; echo "$OLD_WIDGET" >"$OUT/logs/old-widget.txt"
expect $S "old build: tile added" "" add_tile
OLD_TILE="$(tile_label old-04-tile)"
check INFO $S "old tile label" "'$OLD_TILE'"
snapshot >"$OUT/logs/old-snapshot.json"
launch; wait_text eq Today 20

if [[ -n "$MID" ]]; then
  mark mid
  install_r "$MID" mid || check FAIL $S "install -r mid" "see above"
  launch
  expect $S "mid build boots on the old data" "Today rendered" wait_text pre "TODAY'S SESSION" 40
  expect $S "mid build: Session reminder switched on" "" reminder_on
  MID_ALARM="$(rem_alarm)"
  check INFO $S "mid reminder alarm" "'$MID_ALARM'"
  tap_nav Today; sleep 2
  pull_log "$OUT/logs/mid.ndjson"
  check INFO $S "mid log" "$(facts "$OUT/logs/mid.ndjson")"
  snapshot >"$OUT/logs/mid-snapshot.json"
fi
[[ -n "$MID" ]] && BEFORE="$OUT/logs/mid.ndjson" || BEFORE="$OUT/logs/old.ndjson"

mark new
install_r "$NEW" new || check FAIL $S "install -r new" "see above"
sleep 5
# Before the first launch of the new build: what the system kept for it.
expect $S "widget still bound after the update (before launch)" "dumpsys appwidget" widget_bound
W0="$(widget_texts new-01-widget-before-launch)"; echo "$W0" >"$OUT/logs/new-widget-before-launch.txt"
check INFO $S "widget before launch" "$(tr '\n' '|' <<<"$W0")"
if [[ -n "$MID" ]]; then
  A0="$(rem_alarm)"
  expect $S "reminder alarm kept or re-armed by MY_PACKAGE_REPLACED (before launch)" "mid '$MID_ALARM' → '$A0'" test -n "$A0"
fi
pull_log "$OUT/logs/new-before-launch.ndjson"
EV canon "$BEFORE" >"$OUT/logs/before.canon"
EV canon "$OUT/logs/new-before-launch.ndjson" >"$OUT/logs/new-before-launch.canon"
expect $S "update leaves the event log as it was" "" cmp -s "$OUT/logs/before.canon" "$OUT/logs/new-before-launch.canon"

launch
expect $S "new build opens on Today (no guided setup)" "Today rendered" wait_text pre "TODAY'S SESSION" 40
expect $S "no guided setup over the old data" "" eval '! has_text eq Skip'
evidence new-02-today
expect $S "the old pain report still eases today" "ui/new-02-today.xml" python3 "$HELP/node.py" eq "PAIN · LEFT KNEE" <"$OUT/ui/new-02-today.xml"
pull_log "$OUT/logs/new.ndjson"
check INFO $S "new log" "$(facts "$OUT/logs/new.ndjson")"
EV canon "$OUT/logs/new.ndjson" >"$OUT/logs/new.canon"
# Every event the old build logged is still there (boot compaction may drop only
# superseded lines: compare the entry lines).
for k in LogSet LogRun LogRunTrack SubmitReadiness SetProfile; do
  b="$(EV count "$BEFORE" "$k")"; a="$(EV count "$OUT/logs/new.ndjson" "$k")"
  expect $S "new build keeps every $k" "$b before, $a after" test "$a" -eq "$b"
done
p0="$(grep -c '"GeneratePlan"' "$BEFORE")"; p1="$(grep -c '"GeneratePlan"' "$OUT/logs/new.ndjson")"
expect $S "plan kept: no new GeneratePlan on the first launch" "$p0 → $p1; $(grep -o '"GeneratePlan":{[^}]*}' "$OUT/logs/new.ndjson" | tail -1)" test "$p1" -eq "$p0"
tap_nav History; sleep 3; evidence new-03-history
tap_nav Today; sleep 2
snapshot >"$OUT/logs/new-snapshot.json"
python3 - "$OUT/logs/old-snapshot.json" "$OUT/logs/new-snapshot.json" >"$OUT/logs/snapshot-diff.txt" <<'PY'
import json, sys
a, b = (json.load(open(p)) for p in sys.argv[1:3])
for k in sorted(set(a) | set(b)):
    if a.get(k) != b.get(k):
        print(f"{k}: {json.dumps(a.get(k))[:200]} -> {json.dumps(b.get(k))[:200]}")
PY
check INFO $S "widget snapshot old → new" "$(tr '\n' '|' <"$OUT/logs/snapshot-diff.txt" | cut -c1-600)"
k0="$(snap_kind "$OUT/logs/old-snapshot.json")"; k1="$(snap_kind "$OUT/logs/new-snapshot.json")"
expect $S "widget snapshot keeps today's day kind" "kind $k0 → $k1" test -n "$k1" -a "$k0" = "$k1"
W1="$(widget_texts new-04-widget)"; echo "$W1" >"$OUT/logs/new-widget.txt"
expect $S "widget shows the same face after the update" "old $(tr '\n' '|' <<<"$OLD_WIDGET") / new $(tr '\n' '|' <<<"$W1")" test "$OLD_WIDGET" = "$W1"
NEW_TILE="$(tile_label new-05-tile)"
expect $S "tile still on Quick Settings with its label" "'$OLD_TILE' → '$NEW_TILE'" test -n "$NEW_TILE" -a "$NEW_TILE" = "$OLD_TILE"
if [[ -n "$MID" ]]; then
  launch; wait_text eq Today 20
  expect $S "Session reminder still on" "shots/new-06-reminder.png" reminder_is_on new-06-reminder
  A1="$(rem_alarm)"
  expect $S "reminder alarm set after the update" "mid '$MID_ALARM' → '$A1'" test -n "$A1"
fi
# A second process start reads what the first one rewrote.
adb_ shell am force-stop "$APP_ID"; launch
expect $S "second launch of the new build" "Today rendered" wait_text pre "TODAY'S SESSION" 40
pull_log "$OUT/logs/new-2.ndjson"
check INFO $S "log after the second launch" "$(facts "$OUT/logs/new-2.ndjson")"

mark mid-run
home
adb_ uninstall "$APP_ID" >/dev/null 2>&1 || true
adb_ install "$OLD" >/dev/null 2>&1 || { adb_ push "$OLD" /data/local/tmp/upg.apk >/dev/null; sh_ pm install /data/local/tmp/upg.apk >/dev/null; }
grant_perms; pin_clock "$PIN_DATE"
launch; sleep 5; adb_ shell am force-stop "$APP_ID"
push_log "$OUT/seed/basic.ndjson"
launch
if wait_text pre "TODAY'S SESSION" 40; then
  start_feeder mid-run
  tap_node eq Log; sleep 2; tap_node eq "Track run (GPS)"
  if wait_text eq Start 20 && { sleep 4; tap_node eq Start; } && wait_text eq Stop 20; then
    sleep 25
    n0="$(sidecar_points)"
    pull_sidecar "$OUT/logs/mid-run-old-sidecar.ndjson"
    install_r "$NEW" "new over a recording run" || check FAIL $S "install -r new mid-run" "see above"
    launch
    expect $S "new build offers to resume the old build's run" "sidecar $n0 fixes" wait_text eq "Resume your run?" 20
    evidence mid-run-01-resume-prompt
    tap_node eq Resume; sleep 15
    tap_node eq Stop
    if wait_text eq "Save run" 15; then
      evidence mid-run-02-finish
      tap_node eq "Save run"; sleep 5
    fi
    stop_feeder
    pull_log "$OUT/logs/mid-run.ndjson"
    t="$(EV count "$OUT/logs/mid-run.ndjson" LogRunTrack)"
    pts="$(EV track "$OUT/logs/mid-run.ndjson" 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin)["points"])' 2>/dev/null)"
    expect $S "the resumed run is saved once, with the old build's fixes" "$t LogRunTrack, $pts points (old sidecar $n0)" \
      eval '[[ "$t" == 1 && -n "$pts" && "$pts" -ge "$n0" ]]'
  else
    check FAIL $S "mid-run: start a tracked run in the old build" "tracker Start not reachable"
  fi
  stop_feeder
else
  check FAIL $S "mid-run: old build boots the seed" "Today never rendered"
fi

kill "$LOGCAT_PID" 2>/dev/null; wait "$LOGCAT_PID" 2>/dev/null
python3 "$HELP/scan_logcat.py" "$OUT/logcat.txt" "$OUT/logcat-scan.json" >/dev/null 2>&1 &&
  python3 - "$OUT/logcat-scan.json" "$OUT/checks.tsv" <<'PY'
import json, sys
s = json.load(open(sys.argv[1]))
out = open(sys.argv[2], "a")
for k, label in (("fatal", "Java FATAL EXCEPTION"), ("native", "native crash"), ("anr", "ANR")):
    out.write(f"{'PASS' if s[k] == 0 else 'FAIL'}\tlogcat\t{label}\t{s[k]} occurrence(s)\n")
PY
python3 "$HELP/report.py" "$OUT" >/dev/null 2>&1 || true
log "checks: $OUT/checks.tsv"
! grep -q '^FAIL' "$OUT/checks.tsv"
