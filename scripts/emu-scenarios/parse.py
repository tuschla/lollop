"""Parse device and UI text for the device scripts. argv: cmd [args]

  duration TEXT      seconds of the first duration in TEXT, clock form ("1:02:03",
                     "0:59") or spoken ("1 minute 5 seconds", what the UI tree
                     carries for a duration); -1 when there is none
  meminfo FIELD...   the FIELDs (KB) of a `dumpsys meminfo <app>` on stdin, space
                     separated: pss, java, native, graphics (App Summary) and
                     dalvik_alloc, native_alloc (Heap Alloc column); exit 1 when
                     one is missing
  displayed          ms of the last "Displayed <app>/..." launch line on stdin
                     (`logcat -s ActivityTaskManager:I`); exit 1 when there is none
"""
import re
import sys

UNIT_S = {"h": 3600, "m": 60, "s": 1}


def duration(s):
    m = re.search(r"\b(?:(\d+):)?(\d+):(\d\d)\b", s)
    if m:
        return int(m.group(1) or 0) * 3600 + int(m.group(2)) * 60 + int(m.group(3))
    m = re.search(r"(?:\d+ (?:hours?|minutes?|seconds?)\b ?)+", s)
    if m:
        return sum(int(n) * UNIT_S[u[0]] for n, u in re.findall(r"(\d+) (\w+)", m.group(0)))
    return -1


def distance(s):
    m = re.search(r"([0-9.]+)\s*(km|mi|kilometers?|miles?)\b", s)
    return round(float(m.group(1)) * (1.609344 if m.group(2).startswith("mi") else 1), 3) if m else -1


MEMINFO = {
    "pss": r"^\s*TOTAL PSS:\s*(\d+)",
    "java": r"^\s*Java Heap:\s*(\d+)",
    "native": r"^\s*Native Heap:\s*(\d+)",
    "graphics": r"^\s*Graphics:\s*(\d+)",
    "dalvik_alloc": r"^\s*Dalvik Heap\s.*?(\d+)\s+\d+\s*$",
    "native_alloc": r"^\s*Native Heap\s.*?(\d+)\s+\d+\s*$",
}

if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "duration":
        print(duration(sys.argv[2]))
    elif cmd == "meminfo":
        text = sys.stdin.read()
        vals = [re.search(MEMINFO[f], text, re.M) for f in sys.argv[2:]]
        if not all(vals):
            sys.exit(1)
        print(*(v.group(1) for v in vals))
    elif cmd == "displayed":
        m = None
        for m in re.finditer(r"\+(?:(\d+)s)?(\d+)ms", sys.stdin.read()):
            pass
        if m is None:
            sys.exit(1)
        print(int(m.group(1) or 0) * 1000 + int(m.group(2)))
    else:
        sys.exit(f"unknown cmd {cmd}")
