# shellcheck shell=bash
# The build environment every script uses. Source it, don't run it:
#   . "$(dirname "$0")/lib/env.sh"
# Sets and exports:
#   REPO_ROOT           the repository root
#   JAVA_HOME           kept when set, else the system JDK of the version in .java-version
#   ANDROID_HOME        android/local.properties sdk.dir (what Gradle reads), else
#                       $ANDROID_HOME / $ANDROID_SDK_ROOT, else /opt/android-sdk
#   GRADLE_MAX_WORKERS  Gradle --max-workers for runs beside other work: two
#                       workers fit a 32 GB host
# Functions:
#   build_tool NAME              path of NAME in the newest Android build-tools that has it
#   gradle_build_dir_init FILE DIR
#                                writes a Gradle init script (-I FILE) that puts every
#                                project's build directory under DIR/<project name>

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export REPO_ROOT

export JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-$(cat "$REPO_ROOT/.java-version")-openjdk}"

_lollop_sdk=""
if [[ -f "$REPO_ROOT/android/local.properties" ]]; then
  _lollop_sdk="$(sed -n 's/^sdk\.dir=//p' "$REPO_ROOT/android/local.properties" | tail -1)"
fi
export ANDROID_HOME="${_lollop_sdk:-${ANDROID_HOME:-${ANDROID_SDK_ROOT:-/opt/android-sdk}}}"
unset _lollop_sdk

export GRADLE_MAX_WORKERS=2

build_tool() {
  local dir
  for dir in $(ls -d "$ANDROID_HOME"/build-tools/* 2>/dev/null | sort -V -r); do
    if [[ -x "$dir/$1" ]]; then
      echo "$dir/$1"
      return 0
    fi
  done
  echo "$1 not found under $ANDROID_HOME/build-tools: $(ls "$ANDROID_HOME"/build-tools 2>/dev/null | tr '\n' ' ')" >&2
  return 1
}

gradle_build_dir_init() {
  printf 'allprojects { layout.buildDirectory.set(file("%s/${project.name}")) }\n' "$2" >"$1"
}
