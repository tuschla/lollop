"""Read the app's event-log ndjson. argv: cmd log [args]

  count LOG KIND        number of events whose variant is KIND
  lastset LOG           "<exercise>|<weight_kg>|<reps>|<rpe>" of the last LogSet
  track LOG             JSON facts of the last LogRunTrack: points, first/last
                        observed_at, max_gap_s, observed_at, segment_starts,
                        utc_offset_sec (moving figures: the core's, via
                        shared/tests/track_figures_of_record.rs)
  canon LOG             every event re-serialized with sorted keys, one per line,
                        in log order (SetToday — the shell clock — dropped)
"""
import json
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


def track_facts(path):
    t = [e["LogRunTrack"] for e in events(path) if kind(e) == "LogRunTrack"]
    if not t:
        return {}
    r = t[-1]
    pts = r.get("points", [])
    seg = set(r.get("segment_starts", []))
    gaps = [0] + [pts[i]["observed_at"] - pts[i - 1]["observed_at"] for i in range(1, len(pts))]
    return {"tracks": len(t), "points": len(pts), "first_at": pts[0]["observed_at"] if pts else None,
            "last_at": pts[-1]["observed_at"] if pts else None, "max_gap_s": max(gaps), "observed_at": r.get("observed_at"),
            "segment_starts": sorted(seg), "utc_offset_sec": r.get("utc_offset_sec")}


if __name__ == "__main__":
    cmd, path = sys.argv[1], sys.argv[2]
    if cmd == "count":
        print(sum(kind(e) == sys.argv[3] for e in events(path)))
    elif cmd == "lastset":
        s = [e["LogSet"] for e in events(path) if kind(e) == "LogSet"]
        print("" if not s else f"{s[-1].get('exercise')}|{s[-1].get('weight_kg')}|{s[-1].get('reps')}|{s[-1].get('rpe')}")
    elif cmd == "track":
        print(json.dumps(track_facts(path)))
    elif cmd == "canon":
        for e in events(path):
            if kind(e) != "SetToday":
                print(json.dumps(e, sort_keys=True))
    else:
        sys.exit(f"unknown cmd {cmd}")
