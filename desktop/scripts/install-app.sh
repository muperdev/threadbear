#!/bin/bash
# Installs the GPUI app as ~/Applications/Threadbear GPUI.app, beside the Swift
# app, talking to the same runner. Mac only for now. It's an experiment
# (runner/src/experiments/gpuiApp.md): this refuses unless runner.json says
# "experiments": { "gpuiApp": true }.
#
#   desktop/scripts/install-app.sh [--build-on <ssh host>]
#
# With --build-on it builds there (desktop/scripts/remote.sh) and copies the
# result here, for a Mac that shouldn't do a 10-minute Rust build. Otherwise it
# builds on this machine, at low priority.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
build_on=""
if [ "${1:-}" = "--build-on" ]; then build_on="${2:?--build-on needs an ssh host}"; fi

[ "$(uname -s)" = Darwin ] || { echo "Only for a Mac so far: on Linux, run target/release/threadbear-gpui." >&2; exit 2; }

config="${THREADBEAR_HOME:-$HOME/Library/Application Support/Threadbear}/runner.json"
if ! /usr/bin/python3 -I -c 'import json,sys; sys.exit(0 if json.load(open(sys.argv[1])).get("experiments",{}).get("gpuiApp") else 1)' "$config" 2>/dev/null; then
  echo "The GPUI app is an experiment: add \"experiments\": { \"gpuiApp\": true } to $config first." >&2
  exit 1
fi

# View > Show Frame Rate (desktop/gpui/src/frame_stats.rs), for seeing how
# smoothly it draws on this Mac.
features=frame-stats

work="$(mktemp -d "${TMPDIR:-/tmp}/threadbear-gpui-XXXX")"
trap 'rm -rf "$work"' EXIT

if [ -n "$build_on" ]; then
  "$root/desktop/scripts/remote.sh" "$build_on" -- bash -c "cd desktop && nice cargo build --release -p threadbear-gpui --features $features 2>&1 | tail -2"
  key="$(printf '%s' "$root" | shasum | cut -c1-10)"
  scp -q "$build_on:.cache/threadbear-check/gpui/target-$key/release/threadbear-gpui" "$work/threadbear-gpui"
else
  export CARGO_HTTP_MULTIPLEXING=false CARGO_NET_RETRY=10
  (cd "$root/desktop" && taskpolicy -b "$root/scripts/one-build.sh" cargo build --release -p threadbear-gpui --features "$features")
  cp "$root/desktop/target/release/threadbear-gpui" "$work/threadbear-gpui"
fi

bundle="$work/Threadbear GPUI.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$work/threadbear-gpui" "$bundle/Contents/MacOS/threadbear-gpui"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>threadbear-gpui</string>
    <key>CFBundleIdentifier</key><string>com.mikerosoft.threadbear.desktop</string>
    <key>CFBundleName</key><string>Threadbear GPUI</string>
    <key>CFBundleDisplayName</key><string>Threadbear GPUI</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
# The same picture as the Swift app's icon (app/build-app.sh).
iconset="$work/threadbear.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$root/app/icons/threadbear.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$root/app/icons/threadbear.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/threadbear.icns"
/usr/libexec/PlistBuddy -c 'Add :CFBundleIconFile string threadbear' "$bundle/Contents/Info.plist"
codesign --force --sign - "$bundle" 2>/dev/null

mkdir -p "$HOME/Applications"
target="$HOME/Applications/Threadbear GPUI.app"
rm -rf "$target"
ditto "$bundle" "$target"
echo "Installed $target"
