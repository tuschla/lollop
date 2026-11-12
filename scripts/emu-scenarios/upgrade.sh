#!/usr/bin/env bash
# `expect ... eval '<check>'` evaluates the check when expect runs it.
# shellcheck disable=SC2016
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
#      Quick Settings with today's face, the reminder switch and its alarm survive,
#      and the profile takes the distance unit the old build showed, once.
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

# shellcheck source-path=SCRIPTDIR source=lib.sh
source "$(dirname "$0")/lib.sh"
OLD="" MID="" NEW=""

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
out_init "$HOME/.cache/emu-upgrade/$AVD/$(date +%Y%m%d-%H%M%S)"
S=upgrade
SEED="$OUT/logs/seed-basic.ndjson"
HERO="${STR[main_todays_session]}"

log_two_sets() {
  open_set_editor && editor_fill || return 1
  tap_log_set || return 1
  tap_log_set || return 1
  evidence old-sets
  close_editor
}
old_run() {
  start_feeder old-run
  start_run || return 1
  sleep 60
  tap_node eq "${STR[track_btn_stop]}" && wait_text eq "${STR[track_btn_save_run]}" 15 || return 1
  evidence old-run-finish
  finish_run || return 1
  stop_feeder
}
report_pain() {
  local r p
  tap_node eq "${STR[main_log]}" || return 1
  tap_node eq "${STR[log_report_pain]}" || return 1
  wait_text eq "${STR[log_pain_where]}" 10 || return 1
  tap_node eq "${STR[log_pain_area_left_knee]}" || return 1
  tap_node eq "${STR[log_pain_char_not_sure]}" || return 1
  tap_node eq "${STR[log_pain_char_tendon]}" || return 1
  # Severity: an 11-step 0–10 row; 3 is the 4th of 11 cells.
  r="$(find_node eq "${STR[log_pain_severity]}" optional)" || return 1
  read -r -a p <<<"$r"; tap_xy $(( p[2] + (p[4] - p[2]) * 7 / 22 )) "${p[1]}"
  evidence old-pain-sheet
  r="$(find_node eq "${STR[log_report_pain]}" "${STR[pain_trend_worsening]}")" || return 1
  read -r -a p <<<"$r"; tap_xy "${p[0]}" "${p[1]}"
  wait_gone eq "${STR[log_pain_where]}" 10
}
# reminder_row NAME: Profile scrolled to the Session reminder switch and its Time
# row, dumped as ui/NAME.xml.
reminder_row() {
  tap_nav "${STR[main_dest_profile]}"
  scroll_to eq "${STR[reminder_label]}" 12 || return 1
  swipe_content
  evidence "$1"
}
reminder_on() {
  reminder_row mid-reminder-before || return 1
  q on "${STR[reminder_label]}" <"$OUT/ui/mid-reminder-before.xml" && return 0
  tap_node eq "${STR[reminder_label]}"
  wait_for 5 selected "${STR[reminder_label]}"
  evidence mid-reminder-on
  q on "${STR[reminder_label]}" <"$OUT/ui/mid-reminder-on.xml"
}
reminder_is_on() { reminder_row "$1" && q on "${STR[reminder_label]}" <"$OUT/ui/$1.xml"; }
install_r() { # APK TAG: the PASS/FAIL row "install -r TAG"
  local r ok
  r="$(install_apk "$1" -r)"; ok=$?
  expect $S "install -r $2" "$r ($(basename "$1"), $(sha256sum "$1" | cut -c1-12)); version $(sh_ dumpsys package "$APP_ID" | sed -n 's/.*lastUpdateTime=//p' | head -1)" \
    test "$ok" -eq 0
}
facts() { echo "sets=$(EV count "$1" LogSet) runs=$(EV count "$1" LogRun) gps=$(EV count "$1" LogRunTrack) readiness=$(EV count "$1" SubmitReadiness) plans=$(EV count "$1" GeneratePlan) clearplan=$(EV count "$1" ClearPlan)"; }
old_install() {
  adb_ uninstall "$APP_ID" >/dev/null || log "$APP_ID was not installed"
  install_apk "$OLD" >/dev/null || { check FAIL $S "install old" "$(basename "$OLD") did not install"; return 1; }
  baseline_device; grant_perms; pin_clock "$RUN_DAY"
  seed_app "$SEED" || return 1
  launch && wait_text pre "$HERO" 40
}

trap session_cleanup EXIT
config_run
config old "$OLD ($(sha256sum "$OLD" | cut -c1-16))"
[[ -n "$MID" ]] && config mid "$MID ($(sha256sum "$MID" | cut -c1-16))"
config new "$NEW ($(sha256sum "$NEW" | cut -c1-16))"
session_start "$AVD" 2048
seed_basic "$SEED_DAY" 2 >"$SEED" || die "seed_basic failed"

mark old-install
expect $S "old build boots the seeded profile" "Today rendered" old_install
evidence old-01-today

mark old-data
expect $S "old build: two sets through the set editor" "" log_two_sets
expect $S "old build: a GPS run through the tracker" "" old_run
expect $S "old build: a pain report through Log › Report pain" "" report_pain
tap_nav "${STR[main_dest_today]}"; evidence old-02-today-after
pull_log "$OUT/logs/old.ndjson"
observe $S "old log" "$(facts "$OUT/logs/old.ndjson")"
home
expect $S "old build: widget placed" "" place_widget
OLD_WIDGET="$(widget_texts old-03-widget)"; echo "$OLD_WIDGET" >"$OUT/logs/old-widget.txt"
expect $S "old build: tile added" "" add_tile
OLD_TILE="$(tile_label old-04-tile)"; adb_ shell cmd statusbar collapse
observe $S "old tile label" "'$OLD_TILE'"
snapshot >"$OUT/logs/old-snapshot.json"
launch; wait_text eq "${STR[main_dest_today]}" 20

if [[ -n "$MID" ]]; then
  mark mid
  install_r "$MID" mid
  launch
  expect $S "mid build boots on the old data" "Today rendered" wait_text pre "$HERO" 40
  expect $S "mid build: Session reminder switched on" "" reminder_on
  MID_ALARM="$(rem_alarm)"
  observe $S "mid reminder alarm" "'$MID_ALARM'"
  tap_nav "${STR[main_dest_today]}"
  pull_log "$OUT/logs/mid.ndjson"
  observe $S "mid log" "$(facts "$OUT/logs/mid.ndjson")"
  snapshot >"$OUT/logs/mid-snapshot.json"
fi
[[ -n "$MID" ]] && BEFORE="$OUT/logs/mid.ndjson" || BEFORE="$OUT/logs/old.ndjson"

mark new
# The app in the background: Android restarts the top app's screen right after
# an update (API 37), and that boot's first edit (the unit pin) would land before
# the checks below. The mid-run part covers an update under the open app.
home
install_r "$NEW" new
# Before the first launch of the new build: what the system kept for it.
expect $S "widget still bound after the update (before launch)" "dumpsys appwidget" wait_for 10 widget_bound
W0="$(widget_texts new-01-widget-before-launch)"; echo "$W0" >"$OUT/logs/new-widget-before-launch.txt"
observe $S "widget before launch" "$(tr '\n' '|' <<<"$W0")"
if [[ -n "$MID" ]]; then
  wait_for 10 alarm_set
  A0="$(rem_alarm)"
  expect $S "reminder alarm kept or re-armed by MY_PACKAGE_REPLACED (before launch)" "mid '$MID_ALARM' → '$A0'" test -n "$A0"
fi
pull_log "$OUT/logs/new-before-launch.ndjson"
EV canon "$BEFORE" >"$OUT/logs/before.canon"
EV canon "$OUT/logs/new-before-launch.ndjson" >"$OUT/logs/new-before-launch.canon"
expect $S "update leaves the event log as it was" "" cmp -s "$OUT/logs/before.canon" "$OUT/logs/new-before-launch.canon"

launch
expect $S "new build opens on Today (no guided setup)" "Today rendered" wait_text pre "$HERO" 40
expect $S "no guided setup over the old data" "" lacks_text eq "${STR[setup_skip]}"
evidence new-02-today
expect $S "the old pain report still eases today" "ui/new-02-today.xml" q eq "PAIN · LEFT KNEE" <"$OUT/ui/new-02-today.xml"
pull_log "$OUT/logs/new.ndjson"
observe $S "new log" "$(facts "$OUT/logs/new.ndjson")"
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
p0="$(EV count "$BEFORE" GeneratePlan)"; p1="$(EV count "$OUT/logs/new.ndjson" GeneratePlan)"
expect $S "plan kept: no new GeneratePlan on the first launch" "$p0 → $p1; $(grep -o '"GeneratePlan":{[^}]*}' "$OUT/logs/new.ndjson" | tail -1)" test "$p1" -eq "$p0"
tap_nav "${STR[main_dest_history]}"; evidence new-03-history
tap_nav "${STR[main_dest_today]}"
snapshot >"$OUT/logs/new-snapshot.json"
python3 - "$OUT/logs/old-snapshot.json" "$OUT/logs/new-snapshot.json" >"$OUT/logs/snapshot-diff.txt" <<'PY'
import json, sys
a, b = (json.load(open(p)) for p in sys.argv[1:3])
for k in sorted(set(a) | set(b)):
    if a.get(k) != b.get(k):
        print(f"{k}: {json.dumps(a.get(k))[:200]} -> {json.dumps(b.get(k))[:200]}")
PY
observe $S "widget snapshot old → new" "$(tr '\n' '|' <"$OUT/logs/snapshot-diff.txt" | cut -c1-600)"
k0="$(jfield kind <"$OUT/logs/old-snapshot.json")"; k1="$(jfield kind <"$OUT/logs/new-snapshot.json")"
expect $S "widget snapshot keeps today's day kind" "kind $k0 → $k1" test -n "$k1" -a "$k0" = "$k1"
W1="$(widget_texts new-04-widget)"; echo "$W1" >"$OUT/logs/new-widget.txt"
# The new build draws its own widget design, so the old face is only reported;
# the widget must show today from the new snapshot: its title, the run's figure
# (glued to its unit by a no-break space in both) and the tracker's primary.
observe $S "widget face old → new" "old $(tr '\n' '|' <<<"$OLD_WIDGET") / new $(tr '\n' '|' <<<"$W1")"
title1="$(jfield title <"$OUT/logs/new-snapshot.json")"
fig1="$(python3 -c "import json,sys;print(next(i['detail'] for i in json.load(sys.stdin)['items'] if i['kind']=='Run'))" <"$OUT/logs/new-snapshot.json")"
expect $S "widget shows today after the update: title, run figure, ${STR[tile_start_run]}" "title '$title1'; run detail '$fig1'; logs/new-widget.txt shots/new-04-widget.png" \
  eval '[[ -n "$title1" && -n "$fig1" ]] && grep -qxF "text=$title1" <<<"$W1" && grep -qF "$fig1" <<<"$W1" && grep -qxF "text=${STR[tile_start_run]}" <<<"$W1"'
NEW_TILE="$(tile_label new-05-tile)"; adb_ shell cmd statusbar collapse
# Today is the seed's Easy run day (seed_basic), still trackable after the old
# build's run: the tile offers the tracker. The old build's label is only
# reported: 4728546 and 901d239 can show the manifest label ("Log") until their
# next redraw.
expect $S "tile still on Quick Settings, with today's face" "old '$OLD_TILE' → new '$NEW_TILE'" test "$NEW_TILE" = "${STR[tile_start_run]}"
if [[ -n "$MID" ]]; then
  launch; wait_text eq "${STR[main_dest_today]}" 20
  expect $S "Session reminder still on" "shots/new-06-reminder.png" reminder_is_on new-06-reminder
  A1="$(rem_alarm)"
  expect $S "reminder alarm set after the update" "mid '$MID_ALARM' → '$A1'" test -n "$A1"
fi
# A second process start reads what the first one rewrote.
stop_app; launch
expect $S "second launch of the new build" "Today rendered" wait_text pre "$HERO" 40
pull_log "$OUT/logs/new-2.ndjson"
observe $S "log after the second launch" "$(facts "$OUT/logs/new-2.ndjson")"
# The old builds kept the distance unit on the phone (unset: the locale's), and
# their profile states none: the new build's first launch states the unit the
# old build showed, once, as a dated profile edit (bootTurn).
[[ -n "$MID" ]] && SNAP0="$OUT/logs/mid-snapshot.json" || SNAP0="$OUT/logs/old-snapshot.json"
u0="$(python3 -c "import json,sys;print(next(i['detail'] for i in json.load(sys.stdin)['items'] if i['kind']=='Run').split()[-1])" <"$SNAP0")"
units="$(python3 -c "import json,sys;print(' '.join(str(json.loads(l)['SetProfile'].get('distance_unit')) for l in open(sys.argv[1]) if l.startswith('{\"SetProfile\"')))" "$OUT/logs/new-2.ndjson")"
# shellcheck disable=SC2034 # read by the check below
stated="$(tr ' ' '\n' <<<"$units" | grep -cvx None)"
expect $S "the profile takes the unit the old build showed, once" "old run figure unit '$u0'; SetProfile units in the log after two launches: '$units'; today's run figure '$fig1'" \
  eval '[[ -n "$u0" && "$stated" == 1 && "${units##* }" == "${u0^}" && "$fig1" == *"$u0" ]]'

mark mid-run
home
if old_install; then
  start_feeder mid-run
  if start_run; then
    sleep 25
    n0="$(sidecar_points)"
    pull_sidecar "$OUT/logs/mid-run-old-sidecar.ndjson"
    install_r "$NEW" "new over a recording run"
    launch
    expect $S "new build offers to resume the old build's run" "sidecar $n0 fixes" wait_text eq "${STR[main_resume_run_title]}" 20
    evidence mid-run-01-resume-prompt
    tap_node eq "${STR[track_action_resume]}"
    sleep 15
    if tap_node eq "${STR[track_btn_stop]}" && wait_text eq "${STR[track_btn_save_run]}" 15; then
      evidence mid-run-02-finish
      finish_run
    fi
    stop_feeder
    pull_log "$OUT/logs/mid-run.ndjson"
    t="$(EV count "$OUT/logs/mid-run.ndjson" LogRunTrack)"
    pts="$(EV track "$OUT/logs/mid-run.ndjson" | jfield points)"
    expect $S "the resumed run is saved once, with the old build's fixes" "$t LogRunTrack, $pts points (old sidecar $n0)" \
      eval '[[ "$t" == 1 && -n "$pts" && "$pts" -ge "$n0" ]]'
  else
    check FAIL $S "mid-run: start a tracked run in the old build" "tracker Start not reachable"
  fi
  stop_feeder
else
  check FAIL $S "mid-run: old build boots the seed" "Today never rendered"
fi

session_end "Upgrade report"
