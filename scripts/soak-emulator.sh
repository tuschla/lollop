#!/usr/bin/env bash
# Long-term on-device soak test: hunts crashes, ANRs, leaks and tracking bugs that
# only show over a long session on a REAL Android runtime.
#
#   (a) seeded monkey passes restricted to app.milestone (several seeds, chunked so
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
#     --keep-emulator       leave the emulator running at the end
#
# Exit status: 0 when every check passed, 1 when any check failed (see REPORT.md).
# Requires: Android SDK (sdk.dir in android/local.properties or $ANDROID_HOME),
# python3, an x86_64 AVD with Google Play services (fused location) and KVM.
set -euo pipefail

cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"
APP_ID="app.milestone"

SERIAL="emulator-5580"
AVD="e34"
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
KEEP_EMU=0

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
    -h|--help) sed -n '2,36p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
[[ "$GPS_MINUTES" -ge 8 ]] || { echo "--gps-minutes must be >= 8" >&2; exit 2; }

log() { printf '[%s] ==> %s\n' "$(date +%T)" "$*" | tee -a "$OUT/soak.log"; }
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

sdk_dir() {
  local props="$REPO_ROOT/android/local.properties" dir=""
  [[ -f "$props" ]] && dir="$(sed -n 's/^sdk\.dir=//p' "$props" | tail -1)"
  echo "${dir:-${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}}"
}
SDK="$(sdk_dir)"
[[ -n "$SDK" ]] || die "no Android SDK: set sdk.dir in android/local.properties or \$ANDROID_HOME"
ADB="$SDK/platform-tools/adb"
EMULATOR="$SDK/emulator/emulator"
EMU_PORT="${SERIAL#emulator-}"
adb_() { "$ADB" -s "$SERIAL" "$@"; }
command -v python3 >/dev/null || die "python3 is required"
[[ -f "$APK" ]] || die "APK not found: $APK (build it with scripts/build.sh --no-tests)"

rm -rf "$OUT"
mkdir -p "$OUT"/{shots,monkey,gps,py}
PY="$OUT/py"
CHECKS="$OUT/checks.tsv"   # one line per check: PASS|FAIL|INFO <tab> name <tab> detail
: >"$CHECKS"
check() { printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$CHECKS"; log "[$1] $2: $3"; }

# ── Python helpers (route synthesis, fix feeder, UI-tree lookup, analysis) ───
cat >"$PY/route.py" <<'PY'
"""Deterministic 1 Hz route + action schedule for the tracked-run soak.

argv: rec_sec seed out_dir. Writes timeline.tsv (t lon lat alt knots state cum_m),
schedule.env (bash vars, seconds after the feeder starts) and expected.json."""
import json, math, random, sys

rec, seed, out = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
rng = random.Random(seed)
LAT0, LON0, ALT0 = 52.5145, 13.3500, 35.0            # Berlin Tiergarten
A, B = 500.0, 300.0                                   # loop half-axes, m (≈2.55 km lap)
clamp = lambda v, lo, hi: max(lo, min(hi, v))

PRE = 25                       # standing pre-roll for a GPS lock, then Start
start, stop = PRE, PRE + rec
move0 = start + 10             # first stride 10 s after Start (tap timing slack)
stand_tail = 20                # runner stops 20 s before the Stop tap
phases = [
    ("SCREEN", clamp(0.10 * rec, 30, 180)),
    ("LIGHT", clamp(0.033 * rec, 20, 60)),
    ("BG", clamp(0.10 * rec, 30, 180)),
    ("SHADE", 20),
    ("PAUSE", clamp(0.067 * rec, 30, 120) + 24),
    ("POCKET", clamp(0.12 * rec, 40, 240)),
    ("NPAUSE", clamp(0.033 * rec, 20, 60) + 40),
]
gap = (stop - stand_tail - move0 - sum(d for _, d in phases)) / (len(phases) + 1)
assert gap >= 10, "run too short for the action schedule"
sched, t = {}, move0 + gap
for name, dur in phases:
    sched[name + "_FROM"], sched[name + "_TO"] = int(t), int(t + dur)
    t += dur + gap
sched.update(START_AT=start, STOP_AT=stop, END_AT=stop + 120,
             PAUSE_TAP=sched["PAUSE_FROM"] + 12, RESUME_TAP=sched["PAUSE_TO"] - 12,
             NPAUSE_TAP=sched["NPAUSE_FROM"] + 20, NRESUME_TAP=sched["NPAUSE_TO"] - 20)
stands = [("light", sched["LIGHT_FROM"], sched["LIGHT_TO"]),
          ("pause", sched["PAUSE_FROM"], sched["PAUSE_TO"]),
          ("npause", sched["NPAUSE_FROM"], sched["NPAUSE_TO"])]

def state(t):
    if t < move0: return "pre"
    if t >= stop - stand_tail: return "end"
    for name, a, b in stands:
        if a <= t < b: return name
    return "run"

# Ellipse arc-length table.
N = 20000
pts = [(A * math.cos(2 * math.pi * i / N), B * math.sin(2 * math.pi * i / N)) for i in range(N + 1)]
cum = [0.0]
for i in range(1, N + 1):
    cum.append(cum[-1] + math.dist(pts[i - 1], pts[i]))
lap = cum[-1]
def at(s):
    s %= lap
    lo, hi = 0, N
    while hi - lo > 1:
        mid = (lo + hi) // 2
        lo, hi = (mid, hi) if cum[mid] <= s else (lo, mid)
    f = (s - cum[lo]) / (cum[hi] - cum[lo])
    return (pts[lo][0] + f * (pts[hi][0] - pts[lo][0]), pts[lo][1] + f * (pts[hi][1] - pts[lo][1]))

def hav(a, b):
    la1, la2 = math.radians(a[0]), math.radians(b[0])
    dla, dlo = la2 - la1, math.radians(b[1] - a[1])
    h = math.sin(dla / 2) ** 2 + math.cos(la1) * math.cos(la2) * math.sin(dlo / 2) ** 2
    return 2 * 6371000.0 * math.asin(min(1.0, math.sqrt(h)))

s, en, nn = 0.0, 0.0, 0.0
true_m, fed_m, moving_s, cm = 0.0, 0.0, 0, 0.0
prev = None
rows = []
for t in range(0, sched["END_AT"] + 1):
    st = state(t)
    if st == "run":
        v = 3.0 + 0.25 * math.sin(2 * math.pi * t / 300) + 0.15 * math.sin(2 * math.pi * t / 47)
        s += v
        en, nn = 0.85 * en + rng.gauss(0, 0.35), 0.85 * nn + rng.gauss(0, 0.35)
        if start <= t <= stop: true_m += v
    else:
        v = 0.0
        en, nn = 0.97 * en + rng.gauss(0, 0.05), 0.97 * nn + rng.gauss(0, 0.05)
    x, y = at(s)
    x, y = x + en, y + nn
    lat = LAT0 + y / 111320.0
    lon = LON0 + x / (111320.0 * math.cos(math.radians(LAT0)))
    alt = ALT0 + 4 * math.sin(2 * math.pi * s / lap)
    p = (lat, lon)
    if prev is not None and start < t <= stop:
        d = hav(prev, p)
        if d >= 0.5:          # the core's moving floor (< 0.5 m/s = stopped)
            fed_m += d; moving_s += 1; cm += d
    prev = p
    rows.append(f"{t}\t{lon:.7f}\t{lat:.7f}\t{alt:.1f}\t{v * 1.943844:.2f}\t{st}\t{cm:.1f}")

with open(f"{out}/timeline.tsv", "w") as f:
    f.write("\n".join(rows) + "\n")
with open(f"{out}/schedule.env", "w") as f:
    f.write("".join(f"{k}={int(v)}\n" for k, v in sched.items()))
json.dump({"true_moving_m": round(true_m, 1), "fed_moving_m": round(fed_m, 1),
           "moving_s": moving_s, "recorded_s": rec, "lap_m": round(lap, 1),
           "schedule": sched}, open(f"{out}/expected.json", "w"), indent=1)
PY

cat >"$PY/feeder.py" <<'PY'
"""Feed timeline.tsv to the emulator GNSS at 1 Hz: argv adb serial t0 timeline log."""
import subprocess, sys, time
adb, serial, t0, path, logp = sys.argv[1], sys.argv[2], float(sys.argv[3]), sys.argv[4], sys.argv[5]
with open(path) as f, open(logp, "w", buffering=1) as log:
    for line in f:
        t, lon, lat, alt, kn, st, _ = line.rstrip("\n").split("\t")
        delay = t0 + int(t) - time.time()
        if delay > 0:
            time.sleep(delay)
        r = subprocess.run([adb, "-s", serial, "emu", "geo", "fix", lon, lat, alt, "8", kn],
                           capture_output=True, text=True, timeout=10)
        ok = r.returncode == 0 and "OK" in r.stdout
        log.write(f"{t}\t{time.time():.2f}\t{st}\t{'ok' if ok else 'FAIL ' + (r.stdout + r.stderr).strip()}\n")
PY

cat >"$PY/node.py" <<'PY'
"""Read a uiautomator dump on stdin. argv: mode text [anchor].
mode eq|pre|re: print "cx cy x1 y1 x2 y2 <label>" of the first node whose text or
content-desc equals / starts with / regex-matches text (only nodes AFTER the first
node labelled exactly `anchor`, when given); texts: print every label."""
import re, sys, xml.etree.ElementTree as ET
mode, want = sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else ""
anchor = sys.argv[3] if len(sys.argv) > 3 else None
seen = anchor is None
raw = sys.stdin.read()
i, j = raw.find("<?xml"), raw.rfind("</hierarchy>")
if i < 0 or j < 0:
    sys.exit(2)
root = ET.fromstring(raw[i:j + 12])
for n in root.iter("node"):
    for k in ("text", "content-desc"):
        v = n.get(k, "")
        if not v:
            continue
        if not seen:
            seen = v == anchor
            continue
        if mode == "texts":
            print(f"{k}={v}")
            continue
        if (mode == "eq" and v == want) or (mode == "pre" and v.startswith(want)) \
                or (mode == "re" and re.search(want, v)):
            x1, y1, x2, y2 = map(int, re.findall(r"\d+", n.get("bounds")))
            print((x1 + x2) // 2, (y1 + y2) // 2, x1, y1, x2, y2, v)
            sys.exit(0)
sys.exit(0 if mode == "texts" else 1)
PY

cat >"$PY/analyze_run.py" <<'PY'
"""Compare the saved tracked run with the fed route.
argv: event_log.ndjson expected.json t0 feeder.log ui.json out.json"""
import json, math, sys
logp, expp, t0, feedp, uip, outp = sys.argv[1:7]
t0 = float(t0)
exp = json.load(open(expp)); ui = json.load(open(uip))
runs = []
for line in open(logp):
    line = line.strip()
    if not line:
        continue
    try:
        ev = json.loads(line)
    except ValueError:
        continue
    if isinstance(ev, dict) and "LogRunTrack" in ev:
        runs.append(ev["LogRunTrack"])
res = {"expected": exp, "ui": ui, "tracked_runs_in_log": len(runs)}
def hav(a, b):
    la1, la2 = math.radians(a["lat"]), math.radians(b["lat"])
    dla, dlo = la2 - la1, math.radians(b["lon"] - a["lon"])
    h = math.sin(dla / 2) ** 2 + math.cos(la1) * math.cos(la2) * math.sin(dlo / 2) ** 2
    return 2 * 6371000.0 * math.asin(min(1.0, math.sqrt(h)))
def leg(a, b):
    dt = b["observed_at"] - a["observed_at"]
    if dt <= 0:
        return None
    d = hav(a, b)
    return None if d / dt < 0.5 else (dt, d)
def core_qc(points, starts):
    """Replica of the core's of-record derivation (app/run_views/qc.rs qc_track +
    running::track_distance_km_seg + moving_duration_min + the track_splits_seg
    clock), so the History figures can be told apart from a display bug."""
    out, still, ost, standing, pend = [], [], [], None, False
    for i, p in enumerate(points):
        pend = pend or i in starts
        if p.get("accuracy_m", 99) > 30:
            continue
        if not pend and out:
            last = out[-1]; dt = p["observed_at"] - last["observed_at"]
            if dt <= 0:
                continue
            d = hav(last, p)
            if d / dt > 12:
                continue
            if d < 2.5:
                if leg(last, p) is None:
                    standing = p
                continue
        if pend and out:
            ost.append(len(out))
        pend = False; out.append(p); still.append(standing); standing = None
    bounds, prev, segs = sorted(set(ost)), 0, []
    for b in bounds:
        segs.append((prev, b)); prev = b
    segs.append((prev, len(out)))
    dist = head_s = split_s = 0.0
    for a, b in segs:
        for i in range(a + 1, b):
            l = leg(out[i - 1], out[i])
            if l:
                dist += min(l[1], 12 * l[0]); split_s += l[0]
            l2 = leg(still[i] or out[i - 1], out[i])
            if l2:
                head_s += l2[0]
    return {"distance_m": round(dist, 1), "moving_s": int(head_s), "split_clock_s": int(split_s),
            "survivors": len(out)}
if runs:
    r = runs[-1]
    pts = [p for p in r["points"] if p.get("accuracy_m", 99) <= 30]
    seg = set(r.get("segment_starts", []))
    dist = mov = 0.0
    gaps = []
    for i in range(1, len(r["points"])):
        a, b = r["points"][i - 1], r["points"][i]
        dt = b["observed_at"] - a["observed_at"]
        gaps.append((dt, a["observed_at"]))
        if i in seg or dt <= 0:
            continue
        d = hav(a, b)
        if d / dt >= 0.5:
            dist += d; mov += dt
    sch = exp["schedule"]
    def window(a, b):
        lo, hi = t0 + sch[a], t0 + sch[b]
        inside = [p["observed_at"] for p in r["points"] if lo <= p["observed_at"] <= hi]
        g = max((y - x for x, y in zip(inside, inside[1:])), default=None)
        return {"points": len(inside), "max_gap_s": g, "span_s": int(hi - lo)}
    first, last = r["points"][0]["observed_at"], r["points"][-1]["observed_at"]
    res.update(
        points=len(r["points"]), usable_points=len(pts), segment_starts=r.get("segment_starts", []),
        observed_at=r.get("observed_at"), entry_id=r.get("entry_id"),
        workout_type=r.get("workout_type"), session_rpe=r.get("session_rpe"),
        first_fix_t=round(first - t0, 1), last_fix_t=round(last - t0, 1),
        log_moving_m=round(dist, 1), log_moving_s=int(mov),
        max_gap_s=max(gaps)[0] if gaps else None,
        max_gap_at_t=round(max(gaps)[1] - t0, 1) if gaps else None,
        windows={"screen_off": window("SCREEN_FROM", "SCREEN_TO"),
                 "background": window("BG_FROM", "BG_TO"),
                 "pocket_doze": window("POCKET_FROM", "POCKET_TO")},
        core=core_qc(r["points"], seg),
    )
fails = 0
for line in open(feedp):
    fails += "\tFAIL" in line
res["feeder_failures"] = fails
json.dump(res, open(outp, "w"), indent=1)
print(json.dumps(res, indent=1))
PY

cat >"$PY/scan_logcat.py" <<'PY'
"""Scan a threadtime logcat for app crashes/ANRs/StrictMode/errors. argv: logcat pids_file out.json"""
import collections, json, re, sys
path, pidsp, outp = sys.argv[1:4]
APP = "app.milestone"
lines = open(path, errors="replace").read().splitlines()
pids = set(open(pidsp).read().split()) if pidsp else set()
for l in lines:
    m = re.search(r"Start proc (\d+):" + re.escape(APP) + r"[/ ]", l)
    if m:
        pids.add(m.group(1))
    m = re.search(r"am_proc_start: \[\d+,(\d+),\d+," + re.escape(APP) + r",", l)
    if m:
        pids.add(m.group(1))
rx = re.compile(r"^\S+ \S+\s+(\d+)\s+(\d+) ([VDIWEF]) (.*?)\s*: (.*)$")
cats = {k: [] for k in ("fatal", "native", "anr", "strictmode", "fgs", "app_errors", "lollop", "jank")}
errs = collections.Counter()
for i, l in enumerate(lines):
    m = rx.match(l)
    pid, lvl, tag, msg = (m.group(1), m.group(3), m.group(4), m.group(5)) if m else ("", "", "", l)
    ctx = lambda n=25: "\n".join(lines[i:i + n])
    if "FATAL EXCEPTION" in l and (pid in pids or APP in "\n".join(lines[i:i + 3])):
        cats["fatal"].append(ctx(40))
    elif f">>> {APP} <<<" in l or ("Fatal signal" in l and pid in pids):
        cats["native"].append(ctx(30))
    elif re.search(r"ANR in " + re.escape(APP), l) or ("am_anr" in l and APP in l):
        cats["anr"].append(ctx(20))
    elif "StrictMode" in l and (pid in pids or APP in l):
        cats["strictmode"].append(ctx(12))
    elif re.search(r"ForegroundServiceDidNotStart|ForegroundServiceStartNotAllowed|RemoteServiceException|"
                   r"startForegroundService\(\) did not then call", l):
        cats["fgs"].append(ctx(15))
    if re.search(r"lollop|milestone", l, re.I) and lvl in ("E", "F"):
        cats["lollop"].append(l)
    if pid in pids and lvl in ("E", "F"):
        key = tag + ": " + re.sub(r"\d+", "N", msg)[:160]
        errs[key] += 1
    if pid in pids and (re.search(r"Skipped \d+ frames", msg) or msg.startswith("Davey!")):
        cats["jank"].append(l)
res = {k: len(v) for k, v in cats.items()}
res["app_pids"] = sorted(pids)
res["top_app_errors"] = errs.most_common(25)
res["excerpts"] = {k: v[:4] for k, v in cats.items() if k not in ("jank",)}
res["jank_examples"] = cats["jank"][:5]
json.dump(res, open(outp, "w"), indent=1)
PY

cat >"$PY/report.py" <<'PY'
"""Assemble REPORT.md from the soak outputs. argv: out_dir"""
import csv, json, os, sys
out = sys.argv[1]
L = []
w = L.append
cfg = open(f"{out}/config.txt").read()
checks = [l.rstrip("\n").split("\t") for l in open(f"{out}/checks.tsv") if l.strip()]
fails = [c for c in checks if c[0] == "FAIL"]
w("# lollop on-device soak report\n")
w("```\n" + cfg + "```\n")
w(f"**Verdict: {'FAIL' if fails else 'PASS'}** — {sum(c[0]=='PASS' for c in checks)} passed, "
  f"{len(fails)} failed, {sum(c[0]=='INFO' for c in checks)} info.\n")
w("## Checks\n\n| | check | detail |\n|---|---|---|")
for c in checks:
    w(f"| {c[0]} | {c[1]} | {c[2].replace('|', '/')} |")
w("")
mp = f"{out}/monkey/summary.jsonl"
if os.path.exists(mp):
    w("## Monkey passes\n\n| seed | start state | events req | injected | chunks | crashes | ANRs | escapes | minutes | relaunch after |\n|---|---|---|---|---|---|---|---|---|---|")
    for l in open(mp):
        m = json.loads(l)
        w(f"| {m['seed']} | {m['state']} | {m['events']} | {m['injected']} | {m['chunks']} | {m['crashes']} | "
          f"{m['anrs']} | {m['escapes']} | {m['minutes']} | {m['relaunch']} |")
    w("")
gp = f"{out}/gps/result.json"
if os.path.exists(gp):
    g = json.load(open(gp)); e = g["expected"]; ui = g["ui"]
    w("## Tracked run vs fed route\n")
    c = g.get("core") or {}
    w("| quantity | fed route | saved track, raw moving legs | saved track, of-record (core QC replica) | finish sheet | History row |\n|---|---|---|---|---|---|")
    w(f"| moving distance (km) | {e['fed_moving_m']/1000:.3f} (path {e['true_moving_m']/1000:.3f}) | "
      f"{g.get('log_moving_m', 0)/1000:.3f} | {c.get('distance_m', 0)/1000:.3f} | {ui.get('finish_km')} | {ui.get('history_km')} |")
    w(f"| moving time (s) | {e['moving_s']} | {g.get('log_moving_s')} | {c.get('moving_s')} (splits clock {c.get('split_clock_s')}) | "
      f"{ui.get('finish_moving_s')} | {ui.get('history_s')} |")
    w(f"\nFinish sheet: `{ui.get('finish')}` · History row: `{ui.get('history')}`")
    w(f"\npoints saved: {g.get('points')} (usable {g.get('usable_points')}), segment_starts: {g.get('segment_starts')}, "
      f"largest fix gap: {g.get('max_gap_s')} s at t={g.get('max_gap_at_t')} s, feeder failures: {g.get('feeder_failures')}\n")
    w("| window | span s | fixes kept | max gap s |\n|---|---|---|---|")
    for k, v in (g.get("windows") or {}).items():
        w(f"| {k} | {v['span_s']} | {v['points']} | {v['max_gap_s']} |")
    w("\nLive stat readouts (`live.csv`):\n\n```\n" + open(f"{out}/gps/live.csv").read() + "```\n")
sp = f"{out}/samples.csv"
if os.path.exists(sp):
    rows = list(csv.DictReader(open(sp)))
    w("## Memory / service samples\n")
    w("| phase | n | PSS first→last (MB) | PSS max | Java max | Native max | threads max | fds max | pids | svc fg samples |\n|---|---|---|---|---|---|---|---|---|---|")
    order = []
    for r in rows:
        if r["phase"] not in order:
            order.append(r["phase"])
    num = lambda r, k: int(r[k]) if r[k].isdigit() else None
    for ph in order:
        rs = [r for r in rows if r["phase"] == ph]
        pss = [num(r, "pss_kb") for r in rs if num(r, "pss_kb")]
        mx = lambda k: max((num(r, k) or 0 for r in rs), default=0)
        fg = sum(r["svc_fg"] == "1" for r in rs)
        if not pss:
            w(f"| {ph} | {len(rs)} | – | – | – | – | – | – | {len({r['pid'] for r in rs})} | {fg}/{len(rs)} |")
            continue
        w(f"| {ph} | {len(rs)} | {pss[0]/1024:.0f}→{pss[-1]/1024:.0f} | {max(pss)/1024:.0f} | {mx('java_kb')/1024:.0f} | "
          f"{mx('native_kb')/1024:.0f} | {mx('threads')} | {mx('fds')} | {len({r['pid'] for r in rs if r['pid']})} | {fg}/{len(rs)} |")
    w("")
lp = f"{out}/logcat-scan.json"
if os.path.exists(lp):
    s = json.load(open(lp))
    w("## Logcat scan\n")
    w(" · ".join(f"{k}: {s[k]}" for k in ("fatal", "native", "anr", "strictmode", "fgs", "lollop", "app_errors", "jank")))
    w("\nApp pids seen: " + ", ".join(s["app_pids"]) + "\n")
    if s["top_app_errors"]:
        w("Top app-process E/F lines (normalised):\n\n```")
        for k, n in s["top_app_errors"]:
            w(f"{n:6d}  {k}")
        w("```")
    for k, ex in s["excerpts"].items():
        if ex:
            w(f"\n### {k} excerpts\n\n```\n" + "\n---\n".join(e if isinstance(e, str) else str(e) for e in ex)[:6000] + "\n```")
w("\n## Artifacts\n")
for root, _, files in sorted(os.walk(out)):
    if "/py" in root:
        continue
    for f in sorted(files):
        w(f"- `{os.path.relpath(os.path.join(root, f), out)}`")
open(f"{out}/REPORT.md", "w").write("\n".join(L) + "\n")
PY

# ── Device helpers ───────────────────────────────────────────────────────────
dump_ui() {
  local x i
  for i in 1 2 3; do
    x="$(adb_ exec-out uiautomator dump /dev/tty 2>/dev/null || true)"
    [[ "$x" == *"</hierarchy>"* ]] && { printf '%s' "$x"; return 0; }
    sleep 1
  done
  return 1
}
# find_node MODE TEXT → "cx cy x1 y1 x2 y2 label" (MODE eq|pre|re)
find_node() { dump_ui | python3 "$PY/node.py" "$1" "$2"; }
ui_texts() { dump_ui | python3 "$PY/node.py" texts; }
tap_node() { # tap_node MODE TEXT [retries]
  local r p i
  for i in $(seq 1 "${3:-3}"); do
    if r="$(find_node "$1" "$2")"; then
      read -r -a p <<<"$r"
      adb_ shell input tap "${p[0]}" "${p[1]}"
      return 0
    fi
    sleep 1
  done
  log "no on-screen node ($1) '$2'"
  return 1
}
# Bottom-nav item: the label's lower edge can sit under the system nav window's
# touch region on some configurations, so tap the icon above the label.
tap_nav() {
  local r p
  r="$(find_node eq "$1")" || { log "no nav item '$1'"; return 1; }
  read -r -a p <<<"$r"
  adb_ shell input tap "${p[0]}" $((p[3] - 60))
}
# notif_action ACTION TITLE: open the shade and tap ACTION on the app notification
# titled TITLE. A silent ongoing notification is shown collapsed (no action row),
# so expand it first through its own "Expand" chevron.
notif_action() {
  local r p
  adb_ shell cmd statusbar expand-notifications; sleep 3
  tap_node eq "$1" 1 2>/dev/null && return 0
  r="$(dump_ui | python3 "$PY/node.py" eq "Expand" "$2")" || { log "no notification '$2'"; return 1; }
  read -r -a p <<<"$r"
  adb_ shell input tap "${p[0]}" "${p[1]}"; sleep 2
  tap_node eq "$1"
}
shot() { adb_ exec-out screencap -p >"$OUT/shots/$1.png" || true; }
focus() { adb_ shell dumpsys window 2>/dev/null | sed -n 's/.*mCurrentFocus=Window{[^ ]* [^ ]* \([^ }]*\)}.*/\1/p' | head -1; }
launch() { adb_ shell am start -n "$APP_ID/.MainActivity" >/dev/null 2>&1 || true; }
app_pid() { adb_ shell pidof "$APP_ID" 2>/dev/null | tr -d '\r' || true; }
mark() { echo "$1" >"$OUT/.phase"; adb_ shell log -t SOAK "phase $1" >/dev/null 2>&1 || true; log "phase: $1"; }

boot_emulator() {
  if adb_ get-state >/dev/null 2>&1; then
    log "$SERIAL already online — using it"
    return
  fi
  [[ -x "$EMULATOR" ]] || die "emulator not found at $EMULATOR"
  log "booting AVD $AVD on port $EMU_PORT"
  ANDROID_SDK_ROOT="$SDK" ANDROID_HOME="$SDK" "$EMULATOR" -avd "$AVD" -port "$EMU_PORT" \
    -no-window -no-audio -no-boot-anim -no-snapshot -gpu swiftshader_indirect -read-only \
    -memory 2048 >"$OUT/emulator.log" 2>&1 &
  local tries=0
  until [[ "$(adb_ shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == "1" ]]; do
    tries=$((tries + 1)); [[ $tries -lt 150 ]] || die "emulator boot timed out"
    sleep 2
  done
}

stop_emulator() {
  [[ $KEEP_EMU -eq 1 ]] && return 0
  log "stopping emulator $SERIAL"
  adb_ emu kill >/dev/null 2>&1 || true
}

grant_perms() {
  local p
  for p in ACCESS_FINE_LOCATION ACCESS_COARSE_LOCATION POST_NOTIFICATIONS; do
    adb_ shell pm grant "$APP_ID" "android.permission.$p" >/dev/null 2>&1 || true
  done
}

# A set-up hybrid athlete (3 run days / 20 km a week) with one manual 5 km run
# yesterday; the same ndjson wire the app persists and replays on launch.
seed_log() {
  local y=$(( $(date +%s) - 86400 ))
  cat <<EOF
{"SetProfile":{"progression_cadence":"WeekToWeek","lift_goal":"MaxStrength","goal_distance":"General","concurrent_goal":"Strength","weekly_sets":12,"running_days_per_week":3,"running_km_per_week":20.0,"advanced":false,"endurance_intensity_pct_vo2max":75.0,"bodyweight_kg":85.0,"age_years":30.0}}
{"LogRun":{"distance_km":5.0,"duration_min":27.5,"hr_pct_max":0.0,"longest_recent_km":0.0,"observed_at":${y},"entry_id":${y}001}}
EOF
}

# reset_app seeded|fresh: wipe app data, re-grant, optionally seed the event log.
reset_app() {
  adb_ shell am force-stop "$APP_ID"
  adb_ shell pm clear "$APP_ID" >/dev/null
  grant_perms
  if [[ "$1" == seeded ]]; then
    launch; sleep 4; adb_ shell am force-stop "$APP_ID"
    seed_log | adb_ shell "run-as $APP_ID sh -c 'mkdir -p files && cat > files/event-log.ndjson'"
  fi
}

pull_log() { adb_ shell "run-as $APP_ID cat files/event-log.ndjson" >"$1" 2>/dev/null || true; }

# ── Sampler: meminfo + tracking service state every SAMPLE_SEC ──────────────
sampler() {
  set +e # best-effort probes: a failed adb call must never end the sampler
  echo "epoch,phase,pid,pss_kb,java_kb,native_kb,graphics_kb,threads,fds,svc,svc_fg,notif" >"$OUT/samples.csv"
  while :; do
    local now ph pid mem pss java native gfx thr fds svc fg notif sv
    now=$(date +%s); ph="$(cat "$OUT/.phase" 2>/dev/null || echo init)"
    pid="$(app_pid)"; pid="${pid%% *}"
    pss=""; java=""; native=""; gfx=""; thr=""; fds=""
    if [[ -n "$pid" ]]; then
      mem="$(adb_ shell dumpsys meminfo "$APP_ID" 2>/dev/null | tr -d '\r')"
      pss="$(sed -n 's/^ *TOTAL PSS: *\([0-9]*\).*/\1/p' <<<"$mem" | head -1)"
      java="$(sed -n 's/^ *Java Heap: *\([0-9]*\).*/\1/p' <<<"$mem" | head -1)"
      native="$(sed -n 's/^ *Native Heap: *\([0-9]*\).*/\1/p' <<<"$mem" | head -1)"
      gfx="$(sed -n 's/^ *Graphics: *\([0-9]*\).*/\1/p' <<<"$mem" | head -1)"
      thr="$(adb_ shell "run-as $APP_ID cat /proc/$pid/status" 2>/dev/null | sed -n 's/^Threads:[[:space:]]*//p' | tr -d '\r')"
      fds="$(adb_ shell "run-as $APP_ID ls /proc/$pid/fd" 2>/dev/null | wc -l | tr -d ' ')"
    fi
    sv="$(adb_ shell dumpsys activity services "$APP_ID/.RunTrackingService" 2>/dev/null | tr -d '\r')"
    svc=0; fg=0
    grep -q 'ServiceRecord{' <<<"$sv" && svc=1
    grep -q 'isForeground=true' <<<"$sv" && fg=1
    notif="$(adb_ shell dumpsys notification --noredact 2>/dev/null | tr -d '\r' \
      | awk -v app="$APP_ID" '/NotificationRecord\(/{inapp=index($0,"pkg=" app)>0} inapp&&/android.title=/{sub(/.*android.title=[A-Za-z]* \(/,""); sub(/\)$/,""); print; exit}')"
    echo "$now,$ph,$pid,$pss,$java,$native,$gfx,$thr,$fds,$svc,$fg,\"${notif//\"/}\"" >>"$OUT/samples.csv"
    sleep "$SAMPLE_SEC"
  done
}

# ── (a) Monkey ───────────────────────────────────────────────────────────────
monkey_pass() { # seed events state
  local seed="$1" events="$2" state="$3" dir="$OUT/monkey/seed-$1"
  mkdir -p "$dir"
  mark "monkey-$seed"
  reset_app "$state"
  launch; sleep 5
  # Keep the pass inside the app: no shade pulls, no Home/Recents from the nav bar.
  adb_ shell cmd statusbar send-disable-flag statusbar-expansion home recents >/dev/null 2>&1 || true
  local chunks=$(( (events + CHUNK - 1) / CHUNK )) i n injected=0 crashes=0 anrs=0 escapes=0 f t_start
  t_start=$(date +%s)
  for ((i = 0; i < chunks; i++)); do
    n=$CHUNK; [[ $(( (i + 1) * CHUNK )) -le $events ]] || n=$(( events - i * CHUNK ))
    # Deterministic per-chunk seed derived from the pass seed.
    timeout 1200 "$ADB" -s "$SERIAL" shell monkey -p "$APP_ID" -s $(( seed * 1000 + i )) \
      --throttle "$THROTTLE" --pct-syskeys 2 --pct-appswitch 1 --pct-anyevent 0 \
      --ignore-crashes --ignore-timeouts --ignore-security-exceptions --monitor-native-crashes \
      -v "$n" >"$dir/chunk-$i.txt" 2>&1 || true
    injected=$(( injected + $(sed -n 's/.*Events injected: \([0-9]*\).*/\1/p' "$dir/chunk-$i.txt" | tail -1 | grep -o '[0-9]*' || echo 0) ))
    crashes=$(( crashes + $(grep -c "// CRASH: $APP_ID" "$dir/chunk-$i.txt" || true) ))
    anrs=$(( anrs + $(grep -c "// NOT RESPONDING: $APP_ID" "$dir/chunk-$i.txt" || true) ))
    f="$(focus)"
    if [[ "$f" != "$APP_ID"* ]]; then
      escapes=$((escapes + 1)); echo "chunk $i: focus=$f" >>"$dir/escapes.txt"
      adb_ shell input keyevent 4 >/dev/null 2>&1 || true
      launch; sleep 2
    fi
  done
  adb_ shell cmd statusbar send-disable-flag none >/dev/null 2>&1 || true
  adb_ shell cmd statusbar collapse >/dev/null 2>&1 || true
  shot "monkey-$seed-end"
  pull_log "$dir/event-log.ndjson"
  # Cold relaunch: replaying whatever the monkey logged must not crash the app.
  local relaunch=ok pid
  adb_ shell am force-stop "$APP_ID"; sleep 1
  launch; sleep 8
  pid="$(app_pid)"; f="$(focus)"
  [[ -n "$pid" && "$f" == "$APP_ID"* ]] || relaunch="FAILED (pid='$pid' focus='$f')"
  shot "monkey-$seed-relaunch"
  local minutes=$(( ($(date +%s) - t_start) / 60 ))
  printf '{"seed":%s,"state":"%s","events":%s,"injected":%s,"chunks":%s,"crashes":%s,"anrs":%s,"escapes":%s,"minutes":%s,"relaunch":"%s","log_lines":%s}\n' \
    "$seed" "$state" "$events" "$injected" "$chunks" "$crashes" "$anrs" "$escapes" "$minutes" "$relaunch" \
    "$(wc -l <"$dir/event-log.ndjson" | tr -d ' ')" >>"$OUT/monkey/summary.jsonl"
  [[ $crashes -eq 0 && $anrs -eq 0 ]] && check PASS "monkey seed $seed" "$injected events ($state), no crash/ANR reported by monkey" \
    || check FAIL "monkey seed $seed" "$crashes crash(es), $anrs ANR(s) in $injected events — see monkey/seed-$seed"
  [[ "$relaunch" == ok ]] && check PASS "relaunch after monkey $seed" "replayed $(wc -l <"$dir/event-log.ndjson" | tr -d ' ') logged events" \
    || check FAIL "relaunch after monkey $seed" "$relaunch"
  [[ $escapes -eq 0 ]] || check INFO "monkey $seed left the app" "$escapes chunk(s); relaunched (see escapes.txt)"
}

# ── (b) Tracked run ──────────────────────────────────────────────────────────
G="$OUT/gps"
T0=0
at() { # at T: sleep until T seconds after the feeder start
  local d; d=$(python3 -c "import time;print(max(0.0,$T0+$1-time.time()))")
  sleep "$d"
}
now_t() { python3 -c "import time;print(int(time.time()-$T0))"; }
# Read the live stat sheet (content-desc "Moving time … Distance X miles …") into live.csv.
live() {
  local r t cum
  t="$(now_t)"
  r="$(find_node pre "Moving time" 2>/dev/null || true)"
  cum="$(awk -F'\t' -v t="$t" '$1==t{print $7}' "$G/timeline.tsv")"
  echo "$t,$1,\"${r#* * * * * * }\",$cum" >>"$G/live.csv"
}
to_km() { # "3.21 mi" / "5.20 km" → km
  python3 -c "import re,sys;m=re.search(r'([0-9.]+)\s*(mi|km)',sys.argv[1]);print(round(float(m.group(1))*(1.609344 if m.group(2)=='mi' else 1),3) if m else '')" "$1"
}
to_sec() { # first h:mm:ss / m:ss → seconds
  python3 -c "import re,sys;m=re.search(r'\b(?:(\d+):)?(\d+):(\d\d)\b',sys.argv[1]);print(int(m.group(1) or 0)*3600+int(m.group(2))*60+int(m.group(3)) if m else '')" "$1"
}
wake() { adb_ shell input keyevent 224; sleep 1; adb_ shell wm dismiss-keyguard >/dev/null 2>&1 || true; sleep 1; }
svc_fg() { adb_ shell dumpsys activity services "$APP_ID/.RunTrackingService" 2>/dev/null | grep -q 'isForeground=true'; }

gps_run() {
  local rec=$(( GPS_MINUTES * 60 )) pid0 pid1 summary
  mark "gps-setup"
  python3 "$PY/route.py" "$rec" 7 "$G"
  # shellcheck disable=SC1091
  source "$G/schedule.env"
  echo "t,label,stats,expected_cum_m" >"$G/live.csv"
  reset_app seeded
  launch; sleep 5
  local first; first="$(sed -n '1p' "$G/timeline.tsv")"
  read -r _ lon lat alt _ <<<"$first"
  adb_ emu geo fix "$lon" "$lat" "$alt" 8 0 >/dev/null
  tap_node eq "Log" && sleep 2
  tap_node eq "Track run (GPS)" || die "tracker entry not found"
  sleep 3
  T0="$(python3 -c 'import time;print(time.time())')"
  echo "$T0" >"$G/t0"
  python3 "$PY/feeder.py" "$ADB" "$SERIAL" "$T0" "$G/timeline.tsv" "$G/feeder.log" &
  FEEDER_PID=$!
  mark "gps-pre"
  at "$START_AT"; shot gps-01-before-start
  tap_node eq "Start" || die "Start not found"
  sleep 3; pid0="$(app_pid)"
  svc_fg && check PASS "service foreground after Start" "pid $pid0" || check FAIL "service foreground after Start" "RunTrackingService not foreground"
  mark "gps-recording"
  at "$((SCREEN_FROM - 8))"; live before-screen-off
  at "$SCREEN_FROM"; mark "gps-screen-off"; adb_ shell input keyevent 223
  at "$SCREEN_TO"; wake; mark "gps-recording"; sleep 2; live after-screen-on; shot gps-02-after-screen-on
  at "$LIGHT_FROM"; mark "gps-traffic-light"
  at "$LIGHT_TO"; mark "gps-recording"
  at "$BG_FROM"; mark "gps-background"; adb_ shell input keyevent 3
  at "$BG_TO"; launch; mark "gps-recording"; sleep 3; live after-foreground; shot gps-03-after-foreground
  at "$SHADE_FROM"; mark "gps-shade"
  adb_ shell cmd statusbar expand-notifications; sleep 3; shot gps-04-shade
  ui_texts >"$G/shade-texts.txt" || true
  adb_ shell cmd statusbar collapse
  at "$SHADE_TO"; mark "gps-recording"
  at "$PAUSE_TAP"; mark "gps-paused-in-app"
  tap_node eq "Pause" || check FAIL "in-app pause" "Pause control not found"
  sleep 2; live paused-in-app; shot gps-05-paused
  at "$RESUME_TAP"
  tap_node eq "Resume" || check FAIL "in-app resume" "Resume control not found"
  mark "gps-recording"; sleep 2; live resumed-in-app
  at "$POCKET_FROM"; mark "gps-pocket-doze"
  adb_ shell input keyevent 3; sleep 1; adb_ shell input keyevent 223; sleep 1
  adb_ shell dumpsys battery unplug >/dev/null
  adb_ shell dumpsys deviceidle force-idle >"$G/doze.txt" 2>&1 || true
  at "$POCKET_TO"
  adb_ shell dumpsys deviceidle unforce >/dev/null 2>&1 || true
  adb_ shell dumpsys battery reset >/dev/null
  wake; launch; mark "gps-recording"; sleep 3; live after-pocket-doze; shot gps-06-after-pocket
  at "$NPAUSE_TAP"; mark "gps-paused-notification"
  notif_action "Pause" "Tracking run" && { sleep 2; shot gps-07-shade-paused; } \
    || check FAIL "notification pause" "no Pause action on the tracking notification"
  adb_ shell cmd statusbar collapse; sleep 2; live paused-notification
  at "$NRESUME_TAP"
  notif_action "Resume" "Run paused" || check FAIL "notification resume" "no Resume action on the paused notification"
  adb_ shell cmd statusbar collapse; mark "gps-recording"; sleep 2; live resumed-notification
  at "$((STOP_AT - 10))"; live before-stop
  at "$STOP_AT"; mark "gps-finish"
  pid1="$(app_pid)"
  [[ "$pid0" == "$pid1" ]] && check PASS "process survived the run" "pid $pid0 throughout" \
    || check FAIL "process survived the run" "pid changed $pid0 → $pid1 (killed/restarted mid-run)"
  svc_fg && check PASS "service foreground at Stop" "after screen-off, background, Doze, shade" \
    || check FAIL "service foreground at Stop" "RunTrackingService not foreground at the end of the run"
  tap_node eq "Stop" || die "Stop not found"
  sleep 3; shot gps-08-finish-sheet
  summary="$(find_node re '^[0-9.]+ (mi|km) · ' | cut -d' ' -f7- || true)"
  echo "$summary" >"$G/finish-summary.txt"
  tap_node eq "Run type, Easy" || true
  tap_node eq "Save run" || die "Save run not found"
  sleep 5; shot gps-09-after-save
  mark "gps-saved"
  sleep 3
  svc_fg && check FAIL "service stopped after save" "RunTrackingService still foreground after Save" \
    || check PASS "service stopped after save" "no foreground tracking service"
  kill "$FEEDER_PID" 2>/dev/null || true; wait "$FEEDER_PID" 2>/dev/null || true
  pull_log "$G/event-log.ndjson"
  # History › newest run row → run detail.
  mark "gps-history"
  local hist="" row
  tap_nav "History"; sleep 3
  for _ in 1 2 3 4 5 6; do
    row="$(find_node re '^(?!Track|Log)[A-Z][A-Za-z]*( [A-Za-z]+)? run$' || true)"
    [[ -n "$row" ]] && break
    adb_ shell input swipe 540 1400 540 700 400; sleep 2
  done
  if [[ -n "$row" ]]; then
    ui_texts >"$G/history-texts.txt" || true
    # The row's own figures: the first "X mi · pace · time" line after its title.
    hist="$(awk -v t="${row#* * * * * * }" 'f && /^text=[0-9.]+ (mi|km) · /{sub(/^text=/,""); print; exit} $0=="text=" t{f=1}' "$G/history-texts.txt")"
    shot gps-10-history-row
    read -r -a p <<<"$row"; adb_ shell input tap "${p[0]}" "${p[1]}"
    sleep 4; shot gps-11-run-detail
    ui_texts >"$G/detail-texts.txt" || true
    adb_ shell input swipe 540 1500 540 500 400; sleep 2; shot gps-12-run-detail-scrolled
    ui_texts >>"$G/detail-texts.txt" || true
    adb_ shell input swipe 540 1500 540 500 400; sleep 2; shot gps-13-run-detail-bottom
  else
    check FAIL "history row" "tracked run not found in History"
  fi
  echo "$hist" >"$G/history-row.txt"
  local fkm fs hkm hs
  fkm="$(to_km "$summary")"; fs="$(to_sec "${summary#*·}")"; hkm="$(to_km "$hist")"; hs="$(to_sec "$(awk -F' · ' '{print $NF}' <<<"$hist")")"
  printf '{"finish":"%s","finish_km":"%s","finish_moving_s":"%s","history":"%s","history_km":"%s","history_s":"%s"}\n' \
    "$summary" "$fkm" "$fs" "$hist" "$hkm" "$hs" >"$G/ui.json"
  python3 "$PY/analyze_run.py" "$G/event-log.ndjson" "$G/expected.json" "$T0" "$G/feeder.log" "$G/ui.json" "$G/result.json" >/dev/null
  python3 - "$G/result.json" "$CHECKS" <<'PY'
import json, sys
g = json.load(open(sys.argv[1])); e = g["expected"]; ui = g["ui"]
out = open(sys.argv[2], "a")
def chk(ok, name, detail):
    out.write(f"{'PASS' if ok else 'FAIL'}\t{name}\t{detail}\n")
chk(g["tracked_runs_in_log"] == 1, "exactly one tracked run saved", f"{g['tracked_runs_in_log']} LogRunTrack event(s) in the log")
if g["tracked_runs_in_log"]:
    c = g["core"]
    fed, recm = e["fed_moving_m"], c["distance_m"]
    chk(abs(recm - fed) <= 0.03 * fed, "saved distance vs fed route",
        f"of-record {recm/1000:.3f} km vs fed {fed/1000:.3f} km ({(recm/fed-1)*100:+.1f}%; raw moving legs {g['log_moving_m']/1000:.3f} km)")
    chk(abs(c["moving_s"] - e["moving_s"]) <= max(45, 0.03 * e["moving_s"]), "saved moving time vs fed route",
        f"of-record {c['moving_s']} s vs fed {e['moving_s']} s")
    chk(abs(c["split_clock_s"] - c["moving_s"]) <= 2, "splits clock equals headline moving time",
        f"splits sum {c['split_clock_s']} s vs headline {c['moving_s']} s (core replica)")
    for k, v in g["windows"].items():
        chk(v["max_gap_s"] is not None and v["max_gap_s"] <= 30, f"fixes kept while {k}", f"{v['points']} fixes over {v['span_s']} s, max gap {v['max_gap_s']} s")
    # Display rounding: 0.01 mi ≈ 16 m, so a correct readout is within ~9 m.
    if ui["history_km"]:
        chk(abs(float(ui["history_km"]) * 1000 - recm) <= 9, "History row shows the of-record distance",
            f"{ui['history']} vs {recm/1000:.3f} km of record")
    else:
        chk(False, "History row shows the of-record distance", "no History row figures readable")
    if ui["history_s"]:
        chk(abs(int(ui["history_s"]) - c["moving_s"]) <= 1, "History row shows the of-record moving time",
            f"{ui['history_s']} s vs {c['moving_s']} s of record")
    chk(ui["finish_km"] != "" and ui["finish_km"] == ui["history_km"], "Finish sheet distance equals the saved run",
        f"Finish '{ui['finish']}' vs History '{ui['history']}'")
    chk(ui["finish_moving_s"] != "" and ui["finish_moving_s"] == ui["history_s"], "Finish sheet moving time equals the saved run",
        f"{ui['finish_moving_s']} s vs {ui['history_s']} s")
chk(g["feeder_failures"] == 0, "geo fix feeder", f"{g['feeder_failures']} failed injections")
PY
}

# ── Main ─────────────────────────────────────────────────────────────────────
{
  echo "date:        $(date -Is)"
  echo "serial/avd:  $SERIAL / $AVD"
  echo "apk:         $APK ($(date -r "$APK" -Is))"
  echo "git:         $(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null)$(git -C "$REPO_ROOT" diff --quiet 2>/dev/null || echo ' (dirty tree)')"
  echo "monkey:      seeds [$SEEDS], events $EVENTS_FIRST first / $EVENTS_OTHER other, throttle ${THROTTLE} ms, chunk $CHUNK"
  echo "gps run:     ${GPS_MINUTES} min recorded, 1 Hz geo fix"
  echo "sampling:    every ${SAMPLE_SEC} s"
} >"$OUT/config.txt"

boot_emulator
trap 'kill ${SAMPLER_PID:-} ${LOGCAT_PID:-} ${FEEDER_PID:-} 2>/dev/null || true; adb_ shell cmd statusbar send-disable-flag none >/dev/null 2>&1 || true; stop_emulator' EXIT
adb_ shell input keyevent 82 >/dev/null 2>&1 || true
adb_ shell settings put global stay_on_while_plugged_in 0 >/dev/null 2>&1 || true
log "installing $APK"
adb_ install -r "$APK" >/dev/null
echo "device:      $(adb_ shell getprop ro.product.model | tr -d '\r'), API $(adb_ shell getprop ro.build.version.sdk | tr -d '\r')" >>"$OUT/config.txt"
mark "init"
adb_ logcat -c || true
adb_ logcat -G 16M >/dev/null 2>&1 || true
adb_ logcat -v threadtime -b main,system,crash,events >"$OUT/logcat.txt" 2>&1 &
LOGCAT_PID=$!
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
sleep 2
kill "$SAMPLER_PID" "$LOGCAT_PID" 2>/dev/null || true
wait "$SAMPLER_PID" "$LOGCAT_PID" 2>/dev/null || true

# ── (d) Logcat scan + (c) memory checks ──────────────────────────────────────
cut -d, -f3 "$OUT/samples.csv" | grep -E '^[0-9]+$' | sort -u >"$OUT/pids.txt" || true
python3 "$PY/scan_logcat.py" "$OUT/logcat.txt" "$OUT/pids.txt" "$OUT/logcat-scan.json"
python3 - "$OUT/logcat-scan.json" "$OUT/samples.csv" "$CHECKS" <<'PY'
import csv, json, sys
s = json.load(open(sys.argv[1])); rows = list(csv.DictReader(open(sys.argv[2])))
out = open(sys.argv[3], "a")
def chk(ok, name, detail, info=False):
    out.write(f"{'INFO' if info else ('PASS' if ok else 'FAIL')}\t{name}\t{detail}\n")
for k, label in (("fatal", "Java FATAL EXCEPTION"), ("native", "native crash"), ("anr", "ANR"),
                 ("fgs", "foreground-service failure")):
    chk(s[k] == 0, f"logcat: {label}", f"{s[k]} occurrence(s)")
chk(s["strictmode"] == 0, "logcat: StrictMode", f"{s['strictmode']} violation line(s)", info=s["strictmode"] == 0)
chk(True, "logcat: app E/F lines", f"{sum(n for _, n in s['top_app_errors'])} in top groups, {s['lollop']} mentioning lollop/milestone; jank lines {s['jank']}", info=True)
# Memory growth over the tracked run: PSS of the recording phases, first vs last vs max.
rec = [int(r["pss_kb"]) for r in rows if r["phase"].startswith("gps-") and r["phase"] not in ("gps-setup", "gps-pre", "gps-history", "gps-saved") and r["pss_kb"].isdigit()]
if len(rec) >= 4:
    q = max(1, len(rec) // 4)
    early, late = sorted(rec[:q])[q // 2], sorted(rec[-q:])[q // 2]
    growth = (late - early) / early
    chk(growth <= 0.25, "PSS growth during tracked run", f"median first quarter {early/1024:.0f} MB → last quarter {late/1024:.0f} MB ({growth*100:+.0f}%), max {max(rec)/1024:.0f} MB")
mon = [int(r["pss_kb"]) for r in rows if r["phase"].startswith("monkey-") and r["pss_kb"].isdigit()]
if mon:
    chk(max(mon) < 400 * 1024, "PSS ceiling during monkey", f"max {max(mon)/1024:.0f} MB over {len(mon)} samples")
rs = [r for r in rows if r["phase"] in ("gps-screen-off", "gps-background", "gps-pocket-doze", "gps-traffic-light", "gps-paused-in-app", "gps-paused-notification")]
if rs:
    fg = sum(r["svc_fg"] == "1" for r in rs)
    chk(fg == len(rs), "service foreground while screen off / backgrounded / Doze / paused", f"{fg}/{len(rs)} samples foreground")
PY
python3 "$PY/report.py" "$OUT"
cp "$OUT/REPORT.md" "$(dirname "$OUT")/REPORT-auto.md" 2>/dev/null || true
log "report: $OUT/REPORT.md"
! grep -q '^FAIL' "$CHECKS"
