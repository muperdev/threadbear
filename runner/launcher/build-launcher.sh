#!/bin/bash
# Builds "Threadbear Runner.app", the small app the runner's LaunchAgent
# runs (main.swift says why there is one), and puts it at the path given.
#
#   launcher/build-launcher.sh <path/to/Threadbear Runner.app>   build it
#   launcher/build-launcher.sh --check <path>  exit 0 if the app there was
#                                              built from these sources
#                                              (says why not otherwise)
#   launcher/build-launcher.sh --hash          print the sources' hash
#
# install-runner.sh runs --check first and only builds when the sources
# changed or the app is missing, so the app is rebuilt rarely. Its
# signature is what macOS ties privacy grants to, so it is signed the way
# app/build-app.sh signs the Mac app: with the first Apple Development
# identity (THREADBEAR_SIGN_IDENTITY to pick one), or ad hoc with the
# designated requirement pinned to the bundle id, so that a rebuild keeps
# the same identity either way.
#
# The old app, if there was one, is left next to the new one as
# .<name>.old for the caller to remove, or to put back if the install it
# is part of fails.
set -euo pipefail

# THREADBARE_* was the old name for these variables: each stands in for its new name.
for v in $(compgen -e | grep '^THREADBARE_' || true); do n="THREADBEAR_${v#THREADBARE_}"; [ -n "${!n+x}" ] || export "$n=${!v}"; done
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_NAME="Threadbear Runner"
BUNDLE_ID="com.mikerosoft.threadbear.runner"
ICON_SOURCE="$HERE/../../app/icons/threadbear.png"

# Everything that goes into the app: a change to any of these means a
# rebuild. The icon isn't in it on purpose (a new icon isn't worth
# resigning the app for; the next source change picks it up).
sources_hash() {
  cat "$HERE/main.swift" "$HERE/Info.plist" "$HERE/build-launcher.sh" | shasum -a 256 | cut -d' ' -f1
}

MODE="build"
case "${1:-}" in
  --hash) sources_hash; exit 0 ;;
  --check) MODE="check"; shift ;;
  -h|--help) awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"; exit 0 ;;
esac
FINAL_DIR="${1:?give the path to build $APP_NAME.app at}"
case "$FINAL_DIR" in /*) ;; *) FINAL_DIR="$PWD/$FINAL_DIR" ;; esac
FINAL_DIR="${FINAL_DIR%/}"
EXE="$FINAL_DIR/Contents/MacOS/$APP_NAME"
BUILD_JSON="$FINAL_DIR/Contents/Resources/build.json"

if [ "$MODE" = "check" ]; then
  if [ ! -x "$EXE" ]; then echo "$FINAL_DIR isn't built yet."; exit 1; fi
  built="$(sed -n 's/.*"sources": *"\([0-9a-f]*\)".*/\1/p' "$BUILD_JSON" 2>/dev/null | head -n 1)"
  if [ "$built" != "$(sources_hash)" ]; then echo "$FINAL_DIR was built from older sources."; exit 1; fi
  if ! codesign --verify "$FINAL_DIR" >/dev/null 2>&1; then echo "$FINAL_DIR's signature doesn't check out."; exit 1; fi
  exit 0
fi

if ! command -v swiftc >/dev/null 2>&1; then
  echo "swiftc isn't on PATH; $APP_NAME.app needs Xcode or its command line tools (xcode-select --install)." >&2
  exit 1
fi

PARENT="$(dirname "$FINAL_DIR")"
BASE="$(basename "$FINAL_DIR")"
STAGE_DIR="$PARENT/.$BASE.staging"
OLD_DIR="$PARENT/.$BASE.old"
mkdir -p "$PARENT"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/Contents/MacOS" "$STAGE_DIR/Contents/Resources"

echo "Building $APP_NAME.app..."
# One file, so swiftc on its own: no package, no .build folder.
swiftc -O -module-name ThreadbearRunner -o "$STAGE_DIR/Contents/MacOS/$APP_NAME" "$HERE/main.swift"
cp "$HERE/Info.plist" "$STAGE_DIR/Contents/Info.plist"

if [ -f "$ICON_SOURCE" ]; then
  ICONSET="$(mktemp -d)/threadbear.iconset"
  mkdir -p "$ICONSET"
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$ICON_SOURCE" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$ICON_SOURCE" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
  done
  iconutil -c icns "$ICONSET" -o "$STAGE_DIR/Contents/Resources/threadbear.icns"
  rm -rf "$(dirname "$ICONSET")"
fi

COMMIT="$(git -C "$HERE" rev-parse HEAD 2>/dev/null || true)"
printf '{\n  "sources": "%s",\n  "commit": "%s",\n  "builtAt": "%s"\n}\n' \
  "$(sources_hash)" "$COMMIT" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$STAGE_DIR/Contents/Resources/build.json"

SIGN_IDENTITY="${THREADBEAR_SIGN_IDENTITY:-$({
  security find-identity -v -p codesigning 2>/dev/null \
    | awk '/"Apple Development:/ { print $2; exit }'
} || true)}"
SIGNING_REQUIREMENTS=()
if [ -z "$SIGN_IDENTITY" ]; then
  SIGN_IDENTITY="-"
  SIGNING_REQUIREMENTS=(--requirements "=designated => identifier \"$BUNDLE_ID\"")
fi
# swiftc signs the binary ad hoc as it builds; taking that off first keeps
# codesign from saying "replacing existing signature" on every build.
codesign --remove-signature "$STAGE_DIR/Contents/MacOS/$APP_NAME" 2>/dev/null || true
codesign --force --timestamp=none --sign "$SIGN_IDENTITY" \
  ${SIGNING_REQUIREMENTS[@]+"${SIGNING_REQUIREMENTS[@]}"} "$STAGE_DIR" >/dev/null

# Two renames: the old app is gone for an instant, never half there. A
# running launcher keeps its old executable open, so it isn't disturbed.
rm -rf "$OLD_DIR"
if [ -e "$FINAL_DIR" ]; then mv "$FINAL_DIR" "$OLD_DIR"; fi
mv "$STAGE_DIR" "$FINAL_DIR"
echo "Built $FINAL_DIR (signed: $SIGN_IDENTITY)"
