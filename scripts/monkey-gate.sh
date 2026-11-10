#!/usr/bin/env bash
# Seeded monkey stress pass (500 events, seed 42) against the installed app that
# FAILS on a Java crash, a native abort (the Rust core runs in-process over JNI)
# or an ANR in this app — reported by monkey or found in the logcat scan — or on
# a monkey run that ends early. Crashes of other processes are ignored.
#
#   scripts/monkey-gate.sh [adb-serial]
#
# The device: the given serial, else the one device adb sees. Outputs in $QA_OUT
# (default android/app/build/monkey-gate): REPORT.md, checks.tsv, monkey/,
# logcat.txt, logcat-scan.json.
set -uo pipefail

# shellcheck source-path=SCRIPTDIR source=emu-scenarios/lib.sh
source "$(dirname "$0")/emu-scenarios/lib.sh"
out_init "${QA_OUT:-$REPO_ROOT/android/app/build/monkey-gate}"
SERIAL="${1:-$("$ADB" get-serialno)}"
[[ -n "$SERIAL" ]] || die "no device"

trap session_cleanup EXIT
config_run
adb_ logcat -c
logcat_capture
mark monkey
monkey_run "$OUT/monkey" 42 500 150 500
monkey_checks monkey "seed 42" "$OUT/monkey"
session_end "Monkey gate"
