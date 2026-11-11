#!/bin/bash
# Builds an app and runs behaviour specs or takes a picture of it, for use
# through remote.sh:
#
#   desktop/scripts/remote.sh cloud -- desktop/scripts/spec.sh [--swift] [tag expression]
#   desktop/scripts/remote.sh tinker-desk --desk -- desktop/scripts/spec.sh [--swift] --shot "<thread title>" out.png
#
# The GPUI binary is copied somewhere private before the run: remote.sh shares
# one cargo target folder between checkouts, so another thread's build can
# replace target/debug/threadbear-gpui while specs are running against it.
set -euo pipefail
app=gpui
if [ "${1:-}" = "--swift" ]; then app=swift; shift; fi
shot=""; out=""; tags=""
if [ "${1:-}" = "--shot" ]; then shot="${2:?--shot needs a thread title}"; out="${3:?--shot needs an output file}"; else tags="${1:-}"; fi
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
if [ "$app" = swift ]; then
  (cd app && swift build --disable-keychain --product Threadbear 2>&1 | tail -1)
else
  (cd desktop && cargo build -p threadbear-gpui --features automation 2>&1 | tail -2)
  private="${TMPDIR:-/tmp}/spec-gpui-$$"
  mkdir -p "$private"
  cp "${CARGO_TARGET_DIR:-desktop/target}/debug/threadbear-gpui" "$private/threadbear-gpui"
  export TB_GPUI_BIN="$private/threadbear-gpui"
  trap 'rm -rf "$private"' EXIT
fi
cd features
if [ -n "$shot" ]; then
  TB_APP="$app" node tools/shot.ts "$shot" "$out" "${@:4}"
else
  TB_APP="$app" TB_TAGS="$tags" node_modules/.bin/cucumber-js 2>&1 | tail -80
fi
