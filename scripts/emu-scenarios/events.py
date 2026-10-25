"""Read the app's event-log ndjson. argv: cmd log [args]

  count LOG KIND        number of events whose variant is KIND
  lastset LOG           "<exercise>|<weight_kg>|<reps>|<rpe>" of the last LogSet
  track LOG             JSON facts of the last LogRunTrack: points, first/last
                        observed_at, moving_s / moving_m (legs >= 0.5 m/s, segment
                        starts skipped), max_gap_s, observed_at
  canon LOG             every event re-serialized with sorted keys, one per line,
                        in log order (SetToday — the shell clock — dropped)
  points SIDECAR        number of fix lines in an active-run sidecar
"""
import json
import math
import sys


def events(path):
    for line in open(path, errors="replace"):
        line = line.strip()
        if not line:
            continue
        try:
            yield json.loads(line)
        except ValueError:
            yield {"__unparsable__": line[:80]}


def kind(e):
    return e if isinstance(e, str) else next(iter(e), "")


def hav(a, b):
    la1, la2 = math.radians(a["lat"]), math.radians(b["lat"])
    dla, dlo = la2 - la1, math.radians(b["lon"] - a["lon"])
    h = math.sin(dla / 2) ** 2 + math.cos(la1) * math.cos(la2) * math.sin(dlo / 2) ** 2
    return 2 * 6371000.0 * math.asin(min(1.0, math.sqrt(h)))


cmd, path = sys.argv[1], sys.argv[2]
if cmd == "count":
    print(sum(kind(e) == sys.argv[3] for e in events(path)))
elif cmd == "lastset":
    s = [e["LogSet"] for e in events(path) if kind(e) == "LogSet"]
    print("" if not s else f"{s[-1].get('exercise')}|{s[-1].get('weight_kg')}|{s[-1].get('reps')}|{s[-1].get('rpe')}")
elif cmd == "track":
    t = [e["LogRunTrack"] for e in events(path) if kind(e) == "LogRunTrack"]
    if not t:
        print("{}")
        sys.exit(0)
    r = t[-1]
    pts = r.get("points", [])
    seg = set(r.get("segment_starts", []))
    mov_s = mov_m = 0.0
    gaps = [0]
    for i in range(1, len(pts)):
        dt = pts[i]["observed_at"] - pts[i - 1]["observed_at"]
        gaps.append(dt)
        if i in seg or dt <= 0:
            continue
        d = hav(pts[i - 1], pts[i])
        if d / dt >= 0.5:
            mov_s += dt
            mov_m += d
    print(json.dumps({"tracks": len(t), "points": len(pts), "first_at": pts[0]["observed_at"] if pts else None,
                      "last_at": pts[-1]["observed_at"] if pts else None, "moving_s": int(mov_s),
                      "moving_m": round(mov_m, 1), "max_gap_s": max(gaps), "observed_at": r.get("observed_at"),
                      "segment_starts": sorted(seg), "utc_offset_sec": r.get("utc_offset_sec")}))
elif cmd == "canon":
    for e in events(path):
        if kind(e) != "SetToday":
            print(json.dumps(e, sort_keys=True))
elif cmd == "points":
    n = 0
    for e in events(path):
        n += isinstance(e, dict) and "lat" in e
    print(n)
else:
    sys.exit(f"unknown cmd {cmd}")
