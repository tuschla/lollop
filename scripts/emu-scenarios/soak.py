"""The soak run's own analysis (scripts/soak-emulator.sh). argv: cmd [args]

  route REC SEED DIR       a deterministic 1 Hz route + action schedule for a run
                           recorded REC seconds: DIR/timeline.tsv (feeder.py's
                           "t lon lat alt knots state cum_m"), DIR/schedule.env
                           (bash, seconds after the feed starts), DIR/expected.json
  analyze LOG EXPECTED T0 FEED CORE FINISH HISTORY OUT
                           the saved tracked run (event log LOG) against the fed
                           route, the core's figures (CORE: track_figures_of_record
                           output, may be missing) and the Finish sheet / History
                           row text → OUT (result.json)
  checks RESULT CHECKS     the tracked run's rows (scenario "gps") into checks.tsv
  memory SAMPLES CHECKS    the memory / service rows (scenario "memory")
  sections OUT             OUT/sections/*.md for report.py: monkey passes, tracked
                           run, memory samples
"""
import csv
import json
import math
import os
import random
import sys

from events import events, hav, kind, moving
from feeder import ALT, A, B, KNOTS, LAT0, LON0, M_LAT, M_LON
from parse import distance, duration


def route(rec, seed, out):
    rng = random.Random(seed)
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
        if t < move0:
            return "pre"
        if t >= stop - stand_tail:
            return "end"
        for name, a, b in stands:
            if a <= t < b:
                return name
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
        return pts[lo][0] + f * (pts[hi][0] - pts[lo][0]), pts[lo][1] + f * (pts[hi][1] - pts[lo][1])

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
            if start <= t <= stop:
                true_m += v
        else:
            v = 0.0
            en, nn = 0.97 * en + rng.gauss(0, 0.05), 0.97 * nn + rng.gauss(0, 0.05)
        x, y = at(s)
        p = {"lat": LAT0 + (y + nn) / M_LAT, "lon": LON0 + (x + en) / M_LON}
        alt = ALT + 4 * math.sin(2 * math.pi * s / lap)
        if prev is not None and start < t <= stop:
            d = hav(prev, p)
            if d >= 0.5:          # the core's moving floor (< 0.5 m/s = stopped)
                fed_m += d
                moving_s += 1
                cm += d
        prev = p
        rows.append(f"{t}\t{p['lon']:.7f}\t{p['lat']:.7f}\t{alt:.1f}\t{v * KNOTS:.2f}\t{st}\t{cm:.1f}")

    with open(f"{out}/timeline.tsv", "w") as f:
        f.write("\n".join(rows) + "\n")
    with open(f"{out}/schedule.env", "w") as f:
        f.write("".join(f"{k}={int(v)}\n" for k, v in sched.items()))
    json.dump({"true_moving_m": round(true_m, 1), "fed_moving_m": round(fed_m, 1),
               "moving_s": moving_s, "recorded_s": rec, "lap_m": round(lap, 1),
               "schedule": sched}, open(f"{out}/expected.json", "w"), indent=1)


def analyze(logp, expp, t0, feedp, corep, finish, history, outp):
    t0 = float(t0)
    exp = json.load(open(expp))
    ui = {"finish": finish, "finish_km": distance(finish), "finish_moving_s": duration(finish.partition(" · ")[2]),
          "history": history, "history_km": distance(history), "history_s": duration(history.rpartition(" · ")[2])}
    runs = [e["LogRunTrack"] for e in events(logp) if kind(e) == "LogRunTrack"]
    res = {"expected": exp, "ui": ui, "tracked_runs_in_log": len(runs)}
    if runs:
        r = runs[-1]
        p = r["points"]
        mov, dist = moving(p, set(r.get("segment_starts", [])))
        gaps = [(p[i]["observed_at"] - p[i - 1]["observed_at"], p[i - 1]["observed_at"]) for i in range(1, len(p))]
        sch = exp["schedule"]

        def window(a, b):
            lo, hi = t0 + sch[a], t0 + sch[b]
            inside = [q["observed_at"] for q in p if lo <= q["observed_at"] <= hi]
            g = max((y - x for x, y in zip(inside, inside[1:])), default=None)
            return {"points": len(inside), "max_gap_s": g, "span_s": int(hi - lo)}

        res.update(
            points=len(p), usable_points=sum(q.get("accuracy_m", 99) <= 30 for q in p),
            segment_starts=r.get("segment_starts", []), observed_at=r.get("observed_at"),
            entry_id=r.get("entry_id"), workout_type=r.get("workout_type"), session_rpe=r.get("session_rpe"),
            first_fix_t=round(p[0]["observed_at"] - t0, 1), last_fix_t=round(p[-1]["observed_at"] - t0, 1),
            log_moving_m=round(dist, 1), log_moving_s=int(mov),
            max_gap_s=max(gaps)[0] if gaps else None,
            max_gap_at_t=round(max(gaps)[1] - t0, 1) if gaps else None,
            windows={"screen_off": window("SCREEN_FROM", "SCREEN_TO"),
                     "background": window("BG_FROM", "BG_TO"),
                     "pocket_doze": window("POCKET_FROM", "POCKET_TO")},
            core=json.load(open(corep)) if os.path.exists(corep) else None,
        )
    res["feeder_failures"] = sum("\tFAIL" in line for line in open(feedp))
    json.dump(res, open(outp, "w"), indent=1)


def checks(resp, checksp):
    g = json.load(open(resp))
    e, ui = g["expected"], g["ui"]
    out = open(checksp, "a")

    def chk(ok, name, detail):
        out.write(f"{'PASS' if ok else 'FAIL'}\tgps\t{name}\t{detail}\n")

    chk(g["tracked_runs_in_log"] == 1, "exactly one tracked run saved", f"{g['tracked_runs_in_log']} LogRunTrack event(s) in the log")
    chk(g.get("workout_type") == "Easy", "run type chosen on Finish is saved", f"workout_type {g.get('workout_type')!r}")
    if g["tracked_runs_in_log"] and not g["core"]:
        chk(False, "core figures of the saved run", "cargo test --test track_figures_of_record failed: gps/core-cargo.log")
    elif g["tracked_runs_in_log"]:
        c = g["core"]
        fed, recm = e["fed_moving_m"], c["distance_m"]
        chk(abs(recm - fed) <= 0.03 * fed, "saved distance vs fed route",
            f"of-record {recm/1000:.3f} km vs fed {fed/1000:.3f} km ({(recm/fed-1)*100:+.1f}%; raw moving legs {g['log_moving_m']/1000:.3f} km)")
        chk(abs(c["moving_s"] - e["moving_s"]) <= max(45, 0.03 * e["moving_s"]), "saved moving time vs fed route",
            f"of-record {c['moving_s']:.0f} s vs fed {e['moving_s']} s")
        chk(abs(c["split_clock_s"] - c["moving_s"]) <= 2, "splits clock equals headline moving time",
            f"km splits sum {c['split_clock_s']:.1f} s vs headline {c['moving_s']:.0f} s (core)")
        for k, v in g["windows"].items():
            chk(v["max_gap_s"] is not None and v["max_gap_s"] <= 30, f"fixes kept while {k}",
                f"{v['points']} fixes over {v['span_s']} s, max gap {v['max_gap_s']} s")
        # Display rounding: 0.01 mi ≈ 16 m, so a correct readout is within ~9 m.
        chk(ui["history_km"] >= 0 and abs(ui["history_km"] * 1000 - recm) <= 9, "History row shows the of-record distance",
            f"'{ui['history']}' vs {recm/1000:.3f} km of record")
        chk(ui["history_s"] >= 0 and abs(ui["history_s"] - c["moving_s"]) <= 1, "History row shows the of-record moving time",
            f"{ui['history_s']} s vs {c['moving_s']} s of record")
        chk(ui["finish_km"] >= 0 and ui["finish_km"] == ui["history_km"], "Finish sheet distance equals the saved run",
            f"Finish '{ui['finish']}' vs History '{ui['history']}'")
        chk(ui["finish_moving_s"] >= 0 and ui["finish_moving_s"] == ui["history_s"], "Finish sheet moving time equals the saved run",
            f"{ui['finish_moving_s']} s vs {ui['history_s']} s")
    chk(g["feeder_failures"] == 0, "geo fix feeder", f"{g['feeder_failures']} failed injections")


def memory(samplesp, checksp):
    rows = list(csv.DictReader(open(samplesp)))
    out = open(checksp, "a")

    def chk(ok, name, detail):
        out.write(f"{'PASS' if ok else 'FAIL'}\tmemory\t{name}\t{detail}\n")

    # PSS of the recording phases: median of the first vs the last quarter.
    rec = [int(r["pss_kb"]) for r in rows if r["phase"].startswith("gps-")
           and r["phase"] not in ("gps-setup", "gps-pre", "gps-history", "gps-saved") and r["pss_kb"].isdigit()]
    if len(rec) >= 4:
        q = max(1, len(rec) // 4)
        early, late = sorted(rec[:q])[q // 2], sorted(rec[-q:])[q // 2]
        growth = (late - early) / early
        chk(growth <= 0.25, "PSS growth during tracked run",
            f"median first quarter {early/1024:.0f} MB → last quarter {late/1024:.0f} MB ({growth*100:+.0f}%), max {max(rec)/1024:.0f} MB")
    mon = [int(r["pss_kb"]) for r in rows if r["phase"].startswith("monkey-") and r["pss_kb"].isdigit()]
    if mon:
        chk(max(mon) < 400 * 1024, "PSS ceiling during monkey", f"max {max(mon)/1024:.0f} MB over {len(mon)} samples")
    rs = [r for r in rows if r["phase"] in ("gps-screen-off", "gps-background", "gps-pocket-doze", "gps-traffic-light",
                                             "gps-paused-in-app", "gps-paused-notification")]
    if rs:
        fg = sum(r["svc_fg"] == "1" for r in rs)
        chk(fg == len(rs), "service foreground while screen off / backgrounded / Doze / paused", f"{fg}/{len(rs)} samples foreground")


def sections(out):
    os.makedirs(f"{out}/sections", exist_ok=True)
    mp = f"{out}/monkey/summary.jsonl"
    if os.path.exists(mp):
        L = ["## Monkey passes\n", "| seed | start state | events req | injected | chunks finished | crashes | ANRs | escapes | minutes | relaunch after |",
             "|---|---|---|---|---|---|---|---|---|---|"]
        for l in open(mp):
            m = json.loads(l)
            L.append(f"| {m['seed']} | {m['state']} | {m['events']} | {m['injected']} | {m['finished']}/{m['chunks']} | {m['crashes']} | "
                     f"{m['anrs']} | {m['escapes']} | {m['minutes']} | {m['relaunch']} |")
        open(f"{out}/sections/10-monkey.md", "w").write("\n".join(L) + "\n")
    gp = f"{out}/gps/result.json"
    if os.path.exists(gp):
        g = json.load(open(gp))
        e, ui, c = g["expected"], g["ui"], g.get("core") or {}
        L = ["## Tracked run vs fed route\n",
             "| quantity | fed route | saved track, raw moving legs | saved track, of-record (core) | finish sheet | History row |",
             "|---|---|---|---|---|---|",
             f"| moving distance (km) | {e['fed_moving_m']/1000:.3f} (path {e['true_moving_m']/1000:.3f}) | "
             f"{g.get('log_moving_m', 0)/1000:.3f} | {c.get('distance_m', 0)/1000:.3f} | {ui['finish_km']} | {ui['history_km']} |",
             f"| moving time (s) | {e['moving_s']} | {g.get('log_moving_s')} | {c.get('moving_s')} (km splits clock {c.get('split_clock_s')}) | "
             f"{ui['finish_moving_s']} | {ui['history_s']} |",
             f"\nFinish sheet: `{ui['finish']}` · History row: `{ui['history']}`",
             f"\npoints saved: {g.get('points')} (usable {g.get('usable_points')}), segment_starts: {g.get('segment_starts')}, "
             f"largest fix gap: {g.get('max_gap_s')} s at t={g.get('max_gap_at_t')} s, feeder failures: {g.get('feeder_failures')}\n",
             "| window | span s | fixes kept | max gap s |", "|---|---|---|---|"]
        for k, v in (g.get("windows") or {}).items():
            L.append(f"| {k} | {v['span_s']} | {v['points']} | {v['max_gap_s']} |")
        L.append("\nLive stat readouts (`gps/live.csv`):\n\n```\n" + open(f"{out}/gps/live.csv").read() + "```")
        open(f"{out}/sections/20-gps.md", "w").write("\n".join(L) + "\n")
    sp = f"{out}/samples.csv"
    if os.path.exists(sp):
        rows = list(csv.DictReader(open(sp)))
        L = ["## Memory / service samples\n",
             "| phase | n | PSS first→last (MB) | PSS max | Java max | Native max | threads max | fds max | pids | svc fg samples |",
             "|---|---|---|---|---|---|---|---|---|---|"]
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
            pids = len({r["pid"] for r in rs if r["pid"]})
            if not pss:
                L.append(f"| {ph} | {len(rs)} | – | – | – | – | – | – | {pids} | {fg}/{len(rs)} |")
                continue
            L.append(f"| {ph} | {len(rs)} | {pss[0]/1024:.0f}→{pss[-1]/1024:.0f} | {max(pss)/1024:.0f} | {mx('java_kb')/1024:.0f} | "
                     f"{mx('native_kb')/1024:.0f} | {mx('threads')} | {mx('fds')} | {pids} | {fg}/{len(rs)} |")
        open(f"{out}/sections/30-samples.md", "w").write("\n".join(L) + "\n")


if __name__ == "__main__":
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd == "route":
        route(int(args[0]), int(args[1]), args[2])
    elif cmd == "analyze":
        analyze(*args)
    elif cmd == "checks":
        checks(*args)
    elif cmd == "memory":
        memory(*args)
    elif cmd == "sections":
        sections(*args)
    else:
        sys.exit(f"unknown cmd {cmd}")
