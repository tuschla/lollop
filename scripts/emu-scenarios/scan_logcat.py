"""Scan a threadtime logcat for app crashes / ANRs / StrictMode / native aborts.

argv: logcat.txt out.json
Lines are attributed to the scenario phase active when they were logged (the
script writes `EMUSCN: phase <name>` markers). A FATAL EXCEPTION the script
induced on purpose (`am crash` → CrashedByAdbException / "shell-induced") is
counted as `induced`, never as a failure.
"""
import collections
import json
import re
import sys

APP = "app.milestone"
path, outp = sys.argv[1:3]
lines = open(path, errors="replace").read().splitlines()
rx = re.compile(r"^\S+ \S+\s+(\d+)\s+(\d+) ([VDIWEF]) (.*?)\s*: (.*)$")
pids = set()
for l in lines:
    m = re.search(r"Start proc (\d+):" + re.escape(APP) + r"[/ ]", l)
    if m:
        pids.add(m.group(1))
cats = {k: [] for k in ("fatal", "induced", "native", "anr", "strictmode", "oom", "fgs", "app_errors")}
errs = collections.Counter()
phase = "init"
for i, l in enumerate(lines):
    m = rx.match(l)
    pid, lvl, tag, msg = (m.group(1), m.group(3), m.group(4), m.group(5)) if m else ("", "", "", l)
    if tag == "EMUSCN" and msg.startswith("phase "):
        phase = msg[6:]
        continue
    ctx = lambda n=25: "\n".join(lines[i:i + n])
    if "FATAL EXCEPTION" in l and (pid in pids or APP in "\n".join(lines[i:i + 3])):
        block = ctx(40)
        key = "induced" if re.search(r"CrashedByAdbException|shell-induced crash|Crash requested by", block) else "fatal"
        cats[key].append((phase, block))
    elif f">>> {APP} <<<" in l or ("Fatal signal" in l and pid in pids):
        cats["native"].append((phase, ctx(30)))
    elif re.search(r"ANR in " + re.escape(APP), l) or ("am_anr" in l and APP in l):
        cats["anr"].append((phase, ctx(20)))
    elif "StrictMode" in l and (pid in pids or APP in l):
        cats["strictmode"].append((phase, ctx(12)))
    elif pid in pids and re.search(r"OutOfMemoryError|Throwing OutOfMemory", l):
        cats["oom"].append((phase, ctx(20)))
    elif re.search(r"ForegroundServiceDidNotStart|ForegroundServiceStartNotAllowed|"
                   r"startForegroundService\(\) did not then call", l) and APP in "\n".join(lines[i:i + 5]):
        cats["fgs"].append((phase, ctx(15)))
    if pid in pids and lvl in ("E", "F"):
        errs[tag + ": " + re.sub(r"\d+", "N", msg)[:160]] += 1
res = {k: len(v) for k, v in cats.items()}
res["by_phase"] = {k: dict(collections.Counter(p for p, _ in v)) for k, v in cats.items() if v}
res["app_pids"] = sorted(pids)
res["top_app_errors"] = errs.most_common(20)
res["excerpts"] = {k: [f"[{p}]\n{b}" for p, b in v[:3]] for k, v in cats.items() if v}
json.dump(res, open(outp, "w"), indent=1)
print(json.dumps({k: res[k] for k in cats}))
