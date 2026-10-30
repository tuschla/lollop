#!/usr/bin/env bash
# R8 gate for the minified release build: the classes Android or a library reaches
# BY NAME at run time must survive shrinking with the members it calls. A miss
# compiles, passes every debug/Robolectric test and crashes only the release APK
# (ClassNotFoundException, NoSuchMethodException, "Failed to create an instance",
# UnsatisfiedLinkError).
#
# Checks on the R8 output dex (dexdump) and the merged release manifest:
#   jni        app.milestone.Core keeps its name and a NATIVE method for every
#              Java_app_milestone_Core_* export in shared/src/ffi.rs (resetForTest
#              is test-only and may be stripped)
#   manifest   every activity / service / receiver / provider and every
#              androidx.startup initializer of the merged manifest exists under
#              its manifest name with a public no-arg constructor
#   room       every Room database implementation (`*_Impl` extending
#              androidx.room.RoomDatabase; WorkManager's WorkDatabase_Impl, pulled
#              in by Glance) keeps the no-arg constructor Room calls reflectively
#   work       every WorkManager InputMerger keeps its no-arg constructor and every
#              ListenableWorker its (Context, WorkerParameters) one: WorkManager
#              creates both from the class name stored with the work request
#              (Glance renders the widget in a SessionWorker)
# A selected group that finds nothing to check (no Room database, merger or
# worker, say, because R8 merged or renamed a base class) fails.
#
#   scripts/check-release-r8.sh [--build-dir DIR] [--no-build] [--init-script FILE]
#                               [--only "jni manifest room work"]
#
# --build-dir   the app module's build directory (default android/app/build);
#               required with --init-script
# --no-build    check the existing R8 output instead of running
#               `gradlew :app:minifyReleaseWithR8`
# --init-script passed to gradle (-I), e.g. one that moves the build directory
# --only        run only these check groups (default: all); an unknown group
#               exits 2
# Exit 0 when every check passes. Needs the Android SDK (build-tools dexdump),
# python3 and the JDK gradle needs. CI runs it in the release-r8 job
# (.github/workflows/ci.yml).
set -euo pipefail

cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"
BUILD_DIR="$REPO_ROOT/android/app/build"
BUILD=1
ONLY="jni manifest room work"
GRADLE_ARGS=()
BUILD_DIR_SET=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --build-dir) BUILD_DIR="$2"; BUILD_DIR_SET=1; shift 2 ;;
    --no-build) BUILD=0; shift ;;
    --init-script) GRADLE_ARGS+=(-I "$2"); shift 2 ;;
    --only) ONLY="$2"; shift 2 ;;
    -h|--help) awk 'NR>1&&/^#/{print;next}NR>1{exit}' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
[[ -n "${ONLY// }" ]] || { echo "--only needs at least one group: jni manifest room work" >&2; exit 2; }
for g in $ONLY; do
  case "$g" in
    jni|manifest|room|work) ;;
    *) echo "unknown check group: $g (groups: jni manifest room work)" >&2; exit 2 ;;
  esac
done
# An init script may move the build directory; the check must read the same one.
[[ ${#GRADLE_ARGS[@]} -eq 0 || $BUILD_DIR_SET -eq 1 ]] \
  || { echo "--init-script needs --build-dir (the app build directory it sets)" >&2; exit 2; }

SDK=""
[[ -f android/local.properties ]] && SDK="$(sed -n 's/^sdk\.dir=//p' android/local.properties | tail -1)"
SDK="${SDK:-${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}}"
[[ -n "$SDK" ]] || { echo "no Android SDK: set sdk.dir in android/local.properties or \$ANDROID_HOME" >&2; exit 2; }
# Newest build-tools that ships dexdump (a preview or partial install may not).
DEXDUMP=""
for d in $(ls -d "$SDK"/build-tools/* 2>/dev/null | sort -V -r); do
  [[ -x "$d/dexdump" ]] && { DEXDUMP="$d/dexdump"; break; }
done
[[ -n "$DEXDUMP" ]] || { echo "dexdump not found under $SDK/build-tools: $(ls "$SDK"/build-tools 2>/dev/null | tr '\n' ' ')" >&2; exit 2; }

if [[ $BUILD -eq 1 ]]; then
  echo "==> gradle :app:minifyReleaseWithR8"
  (cd android && JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk}" \
    ./gradlew -q "${GRADLE_ARGS[@]}" --max-workers=2 :app:minifyReleaseWithR8)
fi

shopt -s nullglob
DEX=("$BUILD_DIR"/intermediates/dex/release/minifyReleaseWithR8/*.dex)
shopt -u nullglob
MANIFEST="$BUILD_DIR/intermediates/merged_manifests/release/processReleaseManifest/AndroidManifest.xml"
[[ ${#DEX[@]} -gt 0 ]] || { echo "no R8 dex under $BUILD_DIR/intermediates/dex/release/minifyReleaseWithR8" >&2; exit 2; }
[[ -f "$MANIFEST" ]] || { echo "no merged release manifest: $MANIFEST" >&2; exit 2; }

OUT="$BUILD_DIR/outputs/r8-check"
MAPPING="$BUILD_DIR/intermediates/mapping/release/minifyReleaseWithR8/mapping.txt"
[[ -f "$MAPPING" ]] || { echo "no R8 mapping: $MAPPING" >&2; exit 2; }
mkdir -p "$OUT"
"$DEXDUMP" "${DEX[@]}" >"$OUT/dexdump.txt" 2>/dev/null
echo "==> checking ${#DEX[@]} dex file(s) against $MANIFEST"

python3 - "$OUT/dexdump.txt" "$MANIFEST" "$REPO_ROOT/shared/src/ffi.rs" "$MAPPING" "$ONLY" <<'PY'
import re, sys
import xml.etree.ElementTree as ET

dump, manifest, ffi, mapping, only = sys.argv[1:6]
groups = set(only.split())
ANDROID = "{http://schemas.android.com/apk/res/android}"

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
    if what.split(":")[0] not in groups:
        return
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

# jni: every Rust export has a NATIVE method on the unrenamed Core class.
exports = sorted(set(re.findall(r"fn Java_app_milestone_Core_(\w+)", open(ffi).read())) - {"resetForTest"})
found("jni", exports, "Java_app_milestone_Core_* export in shared/src/ffi.rs")
check("app.milestone.Core" in classes, "jni: class app.milestone.Core kept under its JNI name")
core = classes.get("app.milestone.Core", {"methods": []})
for name in exports:
    check(any(n == name and "NATIVE" in a for n, _, a in core["methods"]),
          f"jni: app.milestone.Core.{name} is a kept native method (Java_app_milestone_Core_{name})")

# manifest: components and androidx.startup initializers.
root = ET.parse(manifest).getroot()
pkg = root.get("package") or "app.milestone"
def fq(name):
    if name.startswith("."):
        return pkg + name
    return name if "." in name else f"{pkg}.{name}"
app = root.find("application")
names = []
if app.get(ANDROID + "name"):
    names.append(("application", fq(app.get(ANDROID + "name"))))
for tag in ("activity", "service", "receiver", "provider"):
    for el in app.findall(tag):
        names.append((tag, fq(el.get(ANDROID + "name"))))
for prov in app.findall("provider"):
    for md in prov.findall("meta-data"):
        if md.get(ANDROID + "value") == "androidx.startup":
            names.append(("startup initializer", fq(md.get(ANDROID + "name"))))
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
