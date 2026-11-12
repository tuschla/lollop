#!/usr/bin/env bash
# R8 gate for the minified release build: the classes Android or a library reaches
# BY NAME at run time must survive shrinking with the members it calls. A miss
# compiles, passes every debug/Robolectric test and crashes only the release APK
# (ClassNotFoundException, NoSuchMethodException, "Failed to create an instance",
# UnsatisfiedLinkError).
#
# Checks on the release APK (its dex through dexdump, its manifest through aapt2,
# its libshared.so) and R8's mapping:
#   jni        app.milestone.core.JniCore keeps its name and a NATIVE method for
#              every Java_app_milestone_core_JniCore_* symbol libshared.so exports
#   manifest   every activity / service / receiver / provider and every
#              androidx.startup initializer of the manifest exists under its
#              manifest name with a public no-arg constructor
#   room       every Room database implementation (`*_Impl` extending
#              androidx.room.RoomDatabase; WorkManager's WorkDatabase_Impl, pulled
#              in by Glance) keeps the no-arg constructor Room calls reflectively
#   work       every WorkManager InputMerger keeps its no-arg constructor and every
#              ListenableWorker its (Context, WorkerParameters) one: WorkManager
#              creates both from the class name stored with the work request
#              (Glance renders the widget in a SessionWorker)
# A group that finds nothing to check (no Room database, merger or worker, say,
# because R8 merged or renamed a base class) fails.
#
#   scripts/check-release-r8.sh
#
# Checks the outputs of the last `gradlew :app:assembleRelease` (the APK that
# outputs/apk/release/output-metadata.json names, outputs/mapping/release/
# mapping.txt); scripts/build.sh and CI's release job run it after that build.
# Exit 0 when every check passes. Needs the Android SDK (build-tools dexdump and
# aapt2) and python3.
set -euo pipefail

. "$(dirname "$0")/lib/env.sh"
BUILD_DIR="$REPO_ROOT/android/app/build"
case "${1:-}" in
  "") ;;
  -h|--help) awk 'NR>1&&/^#/{print;next}NR>1{exit}' "$0"; exit 0 ;;
  *) echo "unknown option: $1" >&2; exit 2 ;;
esac

DEXDUMP="$(build_tool dexdump)" || exit 2
AAPT2="$(build_tool aapt2)" || exit 2

APKS="$BUILD_DIR/outputs/apk/release"
[[ -f "$APKS/output-metadata.json" ]] || { echo "no release APK listing: $APKS/output-metadata.json" >&2; exit 2; }
APK="$APKS/$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["elements"][0]["outputFile"])' "$APKS/output-metadata.json")"
MAPPING="$BUILD_DIR/outputs/mapping/release/mapping.txt"
[[ -f "$APK" ]] || { echo "no release APK: $APK" >&2; exit 2; }
[[ -f "$MAPPING" ]] || { echo "no R8 mapping: $MAPPING" >&2; exit 2; }

OUT="$BUILD_DIR/outputs/r8-check"
mkdir -p "$OUT"
"$DEXDUMP" "$APK" >"$OUT/dexdump.txt" 2>/dev/null
"$AAPT2" dump xmltree --file AndroidManifest.xml "$APK" >"$OUT/manifest.txt"
echo "==> checking $APK"

python3 - "$OUT/dexdump.txt" "$OUT/manifest.txt" "$APK" "$MAPPING" <<'PY'
import re, struct, sys, zipfile

dump, manifest, apk, mapping = sys.argv[1:5]

# R8 renames a superclass nothing keeps (androidx.room.RoomDatabase itself): the
# mapping's class lines (`original -> renamed:`) give the original names back.
original = {}
for line in open(mapping, encoding="utf-8", errors="replace"):
    if line[0] not in " #":
        m = re.match(r"(\S+) -> (\S+):$", line.rstrip("\n"))
        if m:
            original[m.group(2)] = m.group(1)

# dexdump: per class its superclass and (name, type, access) of each method.
classes = {}
cur = None
section = None
pending = None
for line in open(dump, encoding="utf-8", errors="replace"):
    m = re.match(r"\s+Class descriptor\s+: 'L(.+);'", line)
    if m:
        cur = {"super": None, "flags": [], "methods": []}
        classes[m.group(1).replace("/", ".")] = cur
        section = None
        continue
    if cur is None:
        continue
    m = re.match(r"\s+Access flags\s+: 0x[0-9a-f]+ \((.*)\)", line)
    if m and section is None:
        cur["flags"] = m.group(1).split()
        continue
    m = re.match(r"\s+Superclass\s+: 'L(.+);'", line)
    if m:
        cur["super"] = m.group(1).replace("/", ".")
        continue
    if re.match(r"\s+(Direct|Virtual) methods\s+-", line):
        section = "methods"
        continue
    if re.match(r"\s+(Static|Instance) fields\s+-", line):
        section = "fields"
        continue
    if section != "methods":
        continue
    m = re.match(r"\s+name\s+: '(.+)'", line)
    if m:
        pending = [m.group(1), None]
        continue
    m = re.match(r"\s+type\s+: '(.+)'", line)
    if m and pending:
        pending[1] = m.group(1)
        continue
    m = re.match(r"\s+access\s+: 0x[0-9a-f]+ \((.*)\)", line)
    if m and pending:
        cur["methods"].append((pending[0], pending[1], m.group(1).split()))
        pending = None

failures = []
passes = 0
def check(ok, what):
    global passes
    if ok:
        passes += 1
    else:
        failures.append(what)
        print(f"FAIL  {what}")

# A group whose class search comes back empty would otherwise pass with no checks.
def found(group, items, what):
    check(bool(items), f"{group}: found at least one {what} to check")

def has_public_noarg_init(cls):
    return any(n == "<init>" and t == "()V" and "PUBLIC" in a for n, t, a in classes[cls]["methods"])

def extends(cls, base):
    seen = set()
    while cls in classes and cls not in seen:
        seen.add(cls)
        cls = classes[cls]["super"]
        if original.get(cls, cls) == base:
            return True
    return False

# jni: every JniCore symbol a packaged libshared.so exports has a NATIVE method on
# the unrenamed JniCore class.
def dynamic_symbols(elf):
    """The defined dynamic symbols of a 64-bit little-endian ELF file."""
    if elf[:6] != b"\x7fELF\x02\x01":
        return set()
    shoff, = struct.unpack_from("<Q", elf, 0x28)
    shentsize, shnum = struct.unpack_from("<HH", elf, 0x3A)
    sections = [struct.unpack_from("<IIQQQQIIQQ", elf, shoff + i * shentsize) for i in range(shnum)]
    names = set()
    for _, kind, _, _, off, size, link, _, _, entsize in sections:
        if kind != 11:  # SHT_DYNSYM
            continue
        strtab = sections[link][4]
        for sym in range(off, off + size, entsize):
            name, _, _, shndx = struct.unpack_from("<IBBH", elf, sym)
            if shndx:
                start = strtab + name
                names.add(elf[start:elf.index(b"\0", start)].decode())
    return names

JNI_CLASS = "app.milestone.core.JniCore"
PREFIX = "Java_" + JNI_CLASS.replace(".", "_") + "_"
with zipfile.ZipFile(apk) as z:
    libs = [z.read(n) for n in z.namelist() if re.fullmatch(r"lib/[^/]+/libshared\.so", n)]
found("jni", libs, "lib/<abi>/libshared.so in the APK")
exports = sorted({s[len(PREFIX):] for elf in libs for s in dynamic_symbols(elf) if s.startswith(PREFIX)})
found("jni", exports, f"{PREFIX}* symbol exported by libshared.so")
check(JNI_CLASS in classes, f"jni: class {JNI_CLASS} kept under its JNI name")
core = classes.get(JNI_CLASS, {"methods": []})
for name in exports:
    check(any(n == name and "NATIVE" in a for n, _, a in core["methods"]),
          f"jni: {JNI_CLASS}.{name} is a kept native method ({PREFIX}{name})")

# manifest: components and androidx.startup initializers. aapt2's `dump xmltree`
# prints "E: tag (line=N)" for an element and "A: name=value" for each of its
# attributes, indented by depth.
def read_manifest(path):
    root, stack = None, []
    for line in open(path, encoding="utf-8"):
        depth = len(line) - len(line.lstrip(" "))
        m = re.match(r"E: (\S+) \(line=\d+\)$", line.strip())
        if m:
            el = {"tag": m.group(1), "depth": depth, "attrs": {}, "children": []}
            while stack and stack[-1]["depth"] >= depth:
                stack.pop()
            if stack:
                stack[-1]["children"].append(el)
            else:
                root = el
            stack.append(el)
            continue
        m = re.match(r'A: (?:http://schemas\.android\.com/apk/res/android:)?(\w+)(?:\(0x[0-9a-f]+\))?="([^"]*)"', line.strip())
        if m and stack:
            stack[-1]["attrs"][m.group(1)] = m.group(2)
    return root

def children(el, tag):
    return [c for c in el["children"] if c["tag"] == tag]

root = read_manifest(manifest)
pkg = root["attrs"]["package"]
def fq(name):
    if name.startswith("."):
        return pkg + name
    return name if "." in name else f"{pkg}.{name}"
app, = children(root, "application")
names = []
if app["attrs"].get("name"):
    names.append(("application", fq(app["attrs"]["name"])))
for tag in ("activity", "service", "receiver", "provider"):
    for el in children(app, tag):
        names.append((tag, fq(el["attrs"]["name"])))
for prov in children(app, "provider"):
    for md in children(prov, "meta-data"):
        if md["attrs"].get("value") == "androidx.startup":
            names.append(("startup initializer", fq(md["attrs"]["name"])))
found("manifest", names, "manifest component")
for kind, cls in names:
    present = cls in classes
    check(present and has_public_noarg_init(cls),
          f"manifest: {kind} {cls} " + ("has a public no-arg constructor" if present else "present in the dex"))

# room: Room instantiates <Database>_Impl by name through its no-arg constructor.
# R8 may merge RoomDatabase into its only subclass (WorkDatabase then extends
# Object), so a `*_Impl` whose superclass is named `*Database` counts too.
def room_impl(c):
    if not c.endswith("_Impl"):
        return False
    sup = classes[c]["super"] or ""
    return extends(c, "androidx.room.RoomDatabase") or original.get(sup, sup).endswith("Database")
impls = [c for c in classes if room_impl(c)]
found("room", impls, "Room database implementation (*_Impl)")
for cls in sorted(impls):
    check(has_public_noarg_init(cls), f"room: {cls} keeps <init>() (Room creates it reflectively)")

# work: InputMerger.fromClassName → newInstance(); WorkerFactory → (Context, WorkerParameters).
# As with Room, R8 may merge the base class away; a concrete class still named
# `*InputMerger` counts too.
def merger(c):
    if "ABSTRACT" in classes[c]["flags"]:
        return False
    return extends(c, "androidx.work.InputMerger") or c.endswith("InputMerger")
mergers = [c for c in classes if merger(c)]
found("work", mergers, "InputMerger")
for cls in sorted(mergers):
    check(has_public_noarg_init(cls), f"work: input merger {cls} keeps <init>() (WorkManager creates it by name)")
WORKER_INIT = "(Landroid/content/Context;Landroidx/work/WorkerParameters;)V"
workers = [c for c in classes if extends(c, "androidx.work.ListenableWorker") and "ABSTRACT" not in classes[c]["flags"]]
found("work", workers, "ListenableWorker")
for cls in sorted(workers):
    check(any(n == "<init>" and t == WORKER_INIT and "PUBLIC" in a for n, t, a in classes[cls]["methods"]),
          f"work: worker {cls} keeps <init>(Context, WorkerParameters) (WorkManager creates it by name)")

print(f"==> {passes} passed, {len(failures)} failed ({len(classes)} classes, {len(names)} manifest entries, "
      f"{len(impls)} Room databases, {len(mergers)} input mergers, {len(workers)} workers)")
sys.exit(1 if failures else 0)
PY
