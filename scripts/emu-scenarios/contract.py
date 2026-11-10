"""Print the app facts the device scripts need as bash assignments, read from
the app's own sources (lib.sh evals the output; nothing here is a copy):

  APP_ID                      android/app/build.gradle.kts applicationId
  COMP[<class>]               manifest components as "<app id>/<class>"
  STR[<key>]                  every string resource (STR[<key>.<quantity>] for plurals)
  RX[<key>]                   an anchored regex of each string with format arguments
                              (%N$d → [0-9]+, %N$s → .+)
  DEST[<OpenDest>]            the open-intent destinations (OpenDest.wire)
  the constants in CONSTS     Kotlin `const val`s, by name per file

Exits 1, naming what is missing, when a source no longer holds a fact.
"""
import os
import re
import shlex
import sys
import xml.etree.ElementTree as ET

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
MAIN = "android/lollop/src/androidMain"
KT = f"{MAIN}/kotlin/app/milestone"
COMMON = "android/lollop/src/commonMain/kotlin/app/milestone"
ANDROID_NS = "{http://schemas.android.com/apk/res/android}"

# shell name: (file, Kotlin const name)
CONSTS = {
    "EVENT_LOG_FILE": (f"{COMMON}/KeyValueStore.kt", "EVENT_LOG_NAME"),
    "ACTIVE_RUN_FILE": (f"{COMMON}/ActiveRunStore.kt", "FILE"),
    "SNAPSHOT_FILE": (f"{KT}/widget/WidgetSnapshotStore.kt", "FILE_NAME"),
    "ACTION_OPEN": (f"{KT}/OpenIntents.android.kt", "ACTION_OPEN"),
    "EXTRA_DEST": (f"{KT}/OpenIntents.android.kt", "EXTRA_DEST"),
    "ACTION_REMINDER": (f"{KT}/SessionReminder.android.kt", "ACTION_FIRE"),
    "ACTION_REST_END": (f"{KT}/RestAlarm.android.kt", "ACTION_REST_END"),
    "RUN_NOTIF_ID": (f"{KT}/RunTrackingService.kt", "NOTIF_ID"),
    "REST_NOTIF_ID": (f"{KT}/RestAlarm.android.kt", "COUNTDOWN_NOTIF_ID"),
    "REST_DONE_NOTIF_ID": (f"{KT}/RestAlarm.android.kt", "DONE_NOTIF_ID"),
    "REST_DONE_CHANNEL": (f"{KT}/RestAlarm.android.kt", "CHANNEL_DONE"),
    "REMINDER_NOTIF_ID": (f"{KT}/SessionReminder.android.kt", "NOTIF_ID"),
    "REMINDER_CHANNEL": (f"{KT}/SessionReminder.android.kt", "CHANNEL"),
    "REMINDER_PREFS": (f"{KT}/SessionReminder.android.kt", "PREFS"),
    "REMINDED_DAYS_KEY": (f"{KT}/SessionReminder.android.kt", "KEY_REMINDED_DAYS"),
    "STEP_BAR_UNITS": (f"{KT}/RunTrackingService.kt", "STEP_BAR_UNITS"),
}

missing = []


def src(path):
    return open(os.path.join(ROOT, path), encoding="utf-8").read()


def one(rx, path, what):
    hits = re.findall(rx, src(path))
    if len(hits) != 1:
        missing.append(f"{what}: {len(hits)} matches in {path}")
        return ""
    return hits[0]


def res_text(e):
    """A resource value as the app shows it (Android's quoting and escapes undone)."""
    t = "".join(e.itertext())
    if len(t) >= 2 and t[0] == t[-1] == '"':
        t = t[1:-1]
    return re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t"}.get(m.group(1), m.group(1)), t)


def regex(s):
    out = []
    for part in re.split(r"(%\d+\$[sd])", s.replace("%%", "%")):
        if re.fullmatch(r"%\d+\$d", part):
            out.append("[0-9]+")
        elif re.fullmatch(r"%\d+\$s", part):
            out.append(".+")
        else:
            out.append(re.sub(r"([\\.^$*+?{}\[\]()|])", r"\\\1", part))
    return "^" + "".join(out) + "$"


lines = []
q = shlex.quote
app_id = one(r'applicationId\s*=\s*"([^"]+)"', "android/app/build.gradle.kts", "applicationId")
namespace = one(r'namespace\s*=\s*"([^"]+)"', "android/lollop/build.gradle.kts", "namespace")
lines.append(f"APP_ID={q(app_id)}")

manifest = ET.parse(os.path.join(ROOT, MAIN, "AndroidManifest.xml")).getroot()
lines.append("declare -gA COMP=()")
for tag in ("activity", "service", "receiver"):
    for e in manifest.iter(tag):
        name = e.get(ANDROID_NS + "name")
        if name.startswith("."):
            full = namespace + name
            lines.append(f"COMP[{name[1:].split('.')[-1]}]={q(app_id + '/' + full)}")

for var, (path, name) in CONSTS.items():
    v = one(rf"(?m)\bconst val {name}\s*=\s*(\"[^\"]*\"|-?\d+)\s*$", path, name)
    lines.append(f"{var}={q(v.strip(chr(34)))}")

enum = one(r"(?s)enum class OpenDest\(val wire: String\) \{(.*?);", f"{COMMON}/OpenIntents.kt", "OpenDest")
lines.append("declare -gA DEST=()")
for name, wire in re.findall(r"(\w+)\(\"(\w+)\"\)", enum):
    lines.append(f"DEST[{name}]={q(wire)}")

lines.append("declare -gA STR=() RX=()")
res = os.path.join(ROOT, MAIN, "res", "values")
for f in sorted(os.listdir(res)):
    if not f.startswith("strings"):
        continue
    for e in ET.parse(os.path.join(res, f)).getroot():
        items = [(e.get("name"), e)] if e.tag == "string" else \
            [(f"{e.get('name')}.{i.get('quantity')}", i) for i in e] if e.tag == "plurals" else []
        for key, node in items:
            v = res_text(node)
            lines.append(f"STR[{key}]={q(v)}")
            if re.search(r"%\d+\$[sd]", v):
                lines.append(f"RX[{key}]={q(regex(v))}")

if missing:
    sys.exit("contract.py: app sources changed: " + "; ".join(missing))
print("\n".join(lines))
