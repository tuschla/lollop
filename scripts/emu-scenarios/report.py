"""Assemble REPORT.md for scripts/emulator-scenarios.sh. argv: out_dir"""
import json
import os
import sys

out = sys.argv[1]
L = []
w = L.append
w("# Emulator scenarios report\n")
if os.path.exists(f"{out}/config.txt"):
    w("```\n" + open(f"{out}/config.txt").read().rstrip() + "\n```\n")
rows = [l.rstrip("\n").split("\t") for l in open(f"{out}/checks.tsv") if l.strip()]
tally = {k: sum(r[0] == k for r in rows) for k in ("PASS", "FAIL", "INFO")}
w(f"**{tally['PASS']} PASS · {tally['FAIL']} FAIL · {tally['INFO']} INFO**\n")
fails = [r for r in rows if r[0] == "FAIL"]
if fails:
    w("## Failures\n")
    for r in fails:
        w(f"- **{r[1]} · {r[2]}** — {r[3]}")
    w("")
w("## Checks by scenario\n")
order = []
for r in rows:
    if r[1] not in order:
        order.append(r[1])
for sc in order:
    w(f"### {sc}\n\n| | check | evidence |\n|---|---|---|")
    for r in rows:
        if r[1] == sc:
            w(f"| {r[0]} | {r[2]} | {r[3].replace('|', '/')} |")
    w("")
mp = f"{out}/measurements.tsv"
if os.path.exists(mp):
    w("## Measurements\n\n| scenario | metric | value |\n|---|---|---|")
    for l in open(mp):
        p = l.rstrip("\n").split("\t")
        if len(p) == 3:
            w(f"| {p[0]} | {p[1]} | {p[2]} |")
    w("")
lp = f"{out}/logcat-scan.json"
if os.path.exists(lp):
    s = json.load(open(lp))
    w("## Logcat scan\n")
    w(" · ".join(f"{k}: {s[k]}" for k in ("fatal", "induced", "native", "anr", "strictmode", "oom", "fgs")))
    w("\nBy phase: `" + json.dumps(s.get("by_phase", {})) + "`\n")
    if s.get("top_app_errors"):
        w("Top app-process E/F lines (digits normalised):\n\n```")
        for k, n in s["top_app_errors"]:
            w(f"{n:6d}  {k}")
        w("```")
    for k, ex in s.get("excerpts", {}).items():
        if k == "induced":
            continue
        w(f"\n### {k} excerpts\n\n```\n" + "\n---\n".join(ex)[:6000] + "\n```")
w("\n## Artifacts\n")
for root, _, files in sorted(os.walk(out)):
    if "/seed" in root or "/probe" in root:
        continue
    for f in sorted(files):
        w(f"- `{os.path.relpath(os.path.join(root, f), out)}`")
open(f"{out}/REPORT.md", "w").write("\n".join(L) + "\n")
print(f"{out}/REPORT.md: {tally}")
