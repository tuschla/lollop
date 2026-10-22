#!/usr/bin/env bash
# Seeded monkey stress pass against the installed app that FAILS on a Java crash,
# a native abort (the Rust core runs in-process over JNI) or an ANR in this app.
# Crashes of other processes are ignored.
#
#   scripts/monkey-gate.sh [adb-serial]
#
# Env: ADB (default `adb`), QA_OUT (default `.`) for monkey.txt / monkey-logcat.txt.
set -euo pipefail

APP_ID="app.milestone"
ADB="${ADB:-adb}"
OUT="${QA_OUT:-.}"
SERIAL_ARGS=()
[[ -n "${1:-}" ]] && SERIAL_ARGS=(-s "$1")
adb_() { "$ADB" "${SERIAL_ARGS[@]}" "$@"; }

mkdir -p "$OUT"
echo "==> running a seeded monkey pass (500 events)"
adb_ logcat -c
adb_ shell monkey -p "$APP_ID" -s 42 --throttle 150 --pct-syskeys 0 --ignore-timeouts 500 \
  >"$OUT/monkey.txt" 2>&1 || true
adb_ logcat -d >"$OUT/monkey-logcat.txt"

# `Process: <app>` heads a Java FATAL EXCEPTION, `>>> <app> <<<` a native
# tombstone (JNI abort, SIGSEGV/SIGABRT in libshared.so); monkey itself reports
# `// CRASH:` / `// NOT RESPONDING:` for the package it drives.
if grep -E "// (CRASH|NOT RESPONDING): $APP_ID" "$OUT/monkey.txt" \
  || grep -E "Process: $APP_ID|>>> $APP_ID <<<|ANR in $APP_ID" "$OUT/monkey-logcat.txt"; then
  echo "ERROR: monkey found a crash/ANR in $APP_ID (see $OUT/monkey.txt, $OUT/monkey-logcat.txt)" >&2
  exit 1
fi
echo "==> monkey pass clean"
