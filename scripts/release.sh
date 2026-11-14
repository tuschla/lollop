#!/usr/bin/env bash
# Cuts a release: sets the version in app-version.xcconfig, bumps the build
# number, commits, tags vVERSION and pushes both. The tag starts
# .github/workflows/release.yml, which builds the signed APK and the .ipa and
# publishes them as a GitHub Release.
#
#   scripts/release.sh 0.2.0
#
# Run it on an up-to-date main whose CI is green.
set -euo pipefail
cd "$(dirname "$0")/.."

[[ $# == 1 && $1 =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "usage: $0 MAJOR.MINOR.PATCH" >&2; exit 2; }
VERSION=$1
TAG="v$VERSION"

[[ $(git rev-parse --abbrev-ref HEAD) == main ]] || { echo "not on main" >&2; exit 1; }
git diff --quiet HEAD || { echo "uncommitted changes" >&2; exit 1; }
git fetch -q origin main --tags
[[ $(git rev-parse HEAD) == $(git rev-parse origin/main) ]] || { echo "main isn't origin/main" >&2; exit 1; }
! git rev-parse -q --verify "refs/tags/$TAG" >/dev/null || { echo "$TAG exists" >&2; exit 1; }

BUILD=$(( $(sed -n 's/^CURRENT_PROJECT_VERSION *= *//p' app-version.xcconfig) + 1 ))
sed -i -e "s/^MARKETING_VERSION *=.*/MARKETING_VERSION = $VERSION/" \
       -e "s/^CURRENT_PROJECT_VERSION *=.*/CURRENT_PROJECT_VERSION = $BUILD/" app-version.xcconfig
git commit -q -m "Release $VERSION" app-version.xcconfig
git tag -a "$TAG" -m "lollo $VERSION"
git push -q origin main "$TAG"
echo "pushed $TAG (build $BUILD): https://github.com/tuschla/lollo/actions/workflows/release.yml"
