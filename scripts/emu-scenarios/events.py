"""Read the app's event-log ndjson, or write one event. argv: cmd args

  count LOG KIND        number of events whose variant is KIND
  lastset LOG           "<exercise>|<weight_kg>|<reps>|<rpe>" of the last LogSet
  track LOG             JSON facts of the last LogRunTrack: points, first/last
                        observed_at, max_gap_s, observed_at, segment_starts,
                        utc_offset_sec (moving figures: the core's, via
                        shared/examples/track_figures)
  canon LOG             every event re-serialized with sorted keys, one per line,
                        in log order (SetToday, the shell clock, dropped)
  event KIND KEY=VALUE...
                        one KIND event line: the first KIND line of the core's
                        wire fixture (FIXTURE, which the wire gates keep in the
                        core's current encoding) with each KEY set to VALUE (JSON,
                        else a string); exit 1 for a KIND or KEY the fixture
                        lacks
"""
import json
import sys
from pathlib import Path

FIXTURE = Path(__file__).resolve().parents[2] / "shared/tests/fixtures/view_snapshot/hybrid.events.ndjson"


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


def event(kind_name, assignments):
    line = next((e for e in events(FIXTURE) if kind(e) == kind_name), None)
    if line is None:
        sys.exit(f"event: no {kind_name} line in {FIXTURE}")
    fields = line[kind_name]
    for a in assignments:
        key, sep, raw = a.partition("=")
        if not sep or key not in fields:
            sys.exit(f"event: {kind_name} has no field '{key}' ({FIXTURE.name})")
        try:
            value = json.loads(raw)
        except ValueError:
            value = raw
        # A whole number for a float field keeps the core's float form.
        if isinstance(fields[key], float) and type(value) is int:
            value = float(value)
        fields[key] = value
    return json.dumps(line, separators=(",", ":"))


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "event":
        print(event(sys.argv[2], sys.argv[3:]))
        sys.exit()
    path = sys.argv[2]
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
