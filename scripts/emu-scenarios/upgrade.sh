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
#      adjusts today, the widget stays bound and shows today, the tile stays on
#      Quick Settings with today's face, the reminder switch and its alarm survive.
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

cd "$(dirname "$0")/../.." || exit 1
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
    -h|--help) awk 'NR>1&&/^#/{print;next}NR>1{exit}' "$0"; exit 0 ;;
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
S=upgrade

# ── Helpers (the shared device, tile and widget helpers are in lib.sh) ───────
snap_kind() { python3 -c 'import json,sys;print(json.load(open(sys.argv[1])).get("kind",""))' "$1" 2>/dev/null; }
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
  r="$(install_apk "$1" -r)"
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
install_apk "$OLD" >/dev/null || check FAIL $S "install old" "$(basename "$OLD") did not install"
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
OLD_TILE="$(tile_label old-04-tile)"; adb_ shell cmd statusbar collapse
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
# Every entry the old build logged is still there (boot compaction drops only
# superseded lines).
for k in LogSet LogRun LogRunTrack SubmitReadiness; do
  was="$(EV count "$BEFORE" "$k")"; now="$(EV count "$OUT/logs/new.ndjson" "$k")"
  expect $S "new build keeps every $k" "$was before, $now after" test "$now" -eq "$was"
done
# Compaction drops a SetProfile a later edit supersedes (log.rs rule 2) but never
# the last one: that one must survive unchanged.
sp="$(grep '^{"SetProfile"' "$OUT/logs/before.canon" | tail -1)"
expect $S "new build keeps the current SetProfile" "$(cut -c1-200 <<<"$sp")" grep -Fxq "$sp" "$OUT/logs/new.canon"
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
NEW_TILE="$(tile_label new-05-tile)"; adb_ shell cmd statusbar collapse
# Today's face from the new snapshot (tileFace in QuickTileService.kt reads
# WidgetSnapshotStore.today(): not stale and dated the device's today). The old
# build's label is only reported: 4728546 and 901d239 can show the manifest label
# ("Log") until their next redraw.
WANT_TILE="$(python3 -c "
import datetime,json,sys,zoneinfo
d=json.load(open(sys.argv[1]))
today=datetime.datetime.fromtimestamp(int(sys.argv[2]),zoneinfo.ZoneInfo(sys.argv[3])).date()-datetime.date(1970,1,1)
k='' if d.get('stale') or d.get('epochDay')!=today.days else d.get('kind')
print('Start run' if k=='Run' and d.get('trackable') else 'Log set' if k=='Lift' and not d.get('sessionDone') else 'Log')" "$OUT/logs/new-snapshot.json" "$(adb_ shell date +%s | tr -d '\r')" "$TZ0" 2>/dev/null)"
expect $S "tile still on Quick Settings, with today's face" "old '$OLD_TILE' → new '$NEW_TILE' (today's face '$WANT_TILE')" test -n "$NEW_TILE" -a "$NEW_TILE" = "$WANT_TILE"
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
install_apk "$OLD" >/dev/null || check FAIL $S "mid-run: install old" "$(basename "$OLD") did not install"
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

logcat_checks
python3 "$HELP/report.py" "$OUT" >/dev/null 2>&1 || true
log "checks: $OUT/checks.tsv"
! grep -q '^FAIL' "$OUT/checks.tsv"
