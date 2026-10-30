"""Parse `dumpsys notification --noredact` (stdin) into one JSON object per posted
notification of a package.

argv: package [id]
Prints a JSON list (or the one record with that notification id, `null` when it
is not posted) of {"id", "channel", "category", "vis", "flags": [...], "when",
"actions": [title, ...], "extras": {name: value}}.
`extras` keeps the dump's value text ("String (Tracking run)" → "Tracking run",
"Boolean (true)" → "true"); `flags` are names (an API 34 dump's hex is decoded).
Only the first record per id is kept: the same key can also appear in the
snooze / history sections further down.
"""
import json
import re
import sys

# android.app.Notification FLAG_* bits; API 34 dumps print flags as hex.
FLAGS = {0x1: "SHOW_LIGHTS", 0x2: "ONGOING_EVENT", 0x4: "INSISTENT", 0x8: "ONLY_ALERT_ONCE",
         0x10: "AUTO_CANCEL", 0x20: "NO_CLEAR", 0x40: "FOREGROUND_SERVICE", 0x80: "HIGH_PRIORITY",
         0x100: "LOCAL_ONLY", 0x200: "GROUP_SUMMARY", 0x400: "AUTOGROUP_SUMMARY", 0x800: "CAN_COLORIZE",
         0x1000: "BUBBLE", 0x2000: "NO_DISMISS", 0x4000: "FSI_REQUESTED_BUT_DENIED",
         0x8000: "USER_INITIATED_JOB", 0x10000: "LIFETIME_EXTENDED_BY_DIRECT_REPLY",
         0x20000: "SILENT", 0x40000: "PROMOTED_ONGOING"}


def flag_names(s):
    if not s.startswith("0x"):
        return s.split("|")
    v = int(s, 16)
    return [n for b, n in FLAGS.items() if v & b] + ([hex(v & ~sum(FLAGS))] if v & ~sum(FLAGS) else [])


pkg = sys.argv[1]
want = int(sys.argv[2]) if len(sys.argv) > 2 else None
text = sys.stdin.read()
records = []
cur = None
in_extras = False
for line in text.splitlines():
    m = re.match(r"\s*NotificationRecord\(0x[0-9a-f]+: pkg=(\S+) user=\S+ id=(-?\d+) .*?channel=(\S+)", line)
    if m:
        cur = None
        in_extras = False
        if m.group(1) == pkg:
            head = lambda k: (re.search(rf" {k}=(\S+?)[ )]", line) or [None, None])[1]
            cur = {"id": int(m.group(2)), "channel": m.group(3), "category": head("category"), "vis": head("vis"),
                   "flags": [], "when": None, "actions": [], "extras": {}}
            if all(r["id"] != cur["id"] for r in records):
                records.append(cur)
            else:
                cur = None
        continue
    if cur is None:
        continue
    s = line.strip()
    if re.match(r"^(AppSettings|mEnabledListeners|Snoozed|Notification history)", s):
        cur = None
        continue
    m = re.match(r'^\[\d+\] "(.*)" -> PendingIntent', s)
    if m and not in_extras:
        cur["actions"].append(m.group(1))
    elif s.startswith("flags=") and not cur["flags"]:
        cur["flags"] = flag_names(s[6:])
    elif s.startswith("when=") and cur["when"] is None:
        cur["when"] = int(s[5:].split("/")[0])
    elif s == "extras={":
        in_extras = True
    elif in_extras and s == "}":
        in_extras = False
    elif in_extras:
        m = re.match(r"([\w.]+)=(?:\w[\w.$\[\]]* \((.*)\)|(null))$", s)
        if m:
            cur["extras"][m.group(1)] = m.group(2) if m.group(2) is not None else None
if want is None:
    print(json.dumps(records, ensure_ascii=False))
else:
    print(json.dumps(next((r for r in records if r["id"] == want), None), ensure_ascii=False))
