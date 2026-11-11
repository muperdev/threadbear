#!/bin/zsh
# The iPhone's half of the parity run (scripts/parity/run.sh): builds the app
# for the simulator, pairs it with the run's throwaway backend (a Debug build
# takes a local http one, `-ThreadbearPair`), opens each scenario's thread
# and saves a picture of it. Its own simulator, "Threadbear Parity", shut
# down afterwards unless it was already up.
#
#   scripts/parity/ios-shots.sh <convex url> <seed.json> <out dir>
set -euo pipefail
URL=$1 SEED=$2 OUT=$3
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
mkdir -p "$OUT"
NAME="Threadbear Parity"
SIM=$(xcrun simctl list devices available | grep -F "    $NAME (" | grep -oE "[0-9A-F-]{36}" | head -1 || true)
if [[ -z "$SIM" ]]; then
  BASE=$(xcrun simctl list devices available | grep -E "iPhone 1[6-8] Pro( Max)? \(" | grep -oE "[0-9A-F-]{36}" | head -1)
  SIM=$(xcrun simctl clone "$BASE" "$NAME")
fi
if ! xcrun simctl list devices booted | grep -qF "($SIM)"; then
  trap 'xcrun simctl shutdown "$SIM" 2>/dev/null || true' EXIT
  xcrun simctl boot "$SIM"
fi
xcrun simctl bootstatus "$SIM" -b >/dev/null
DD="$HOME/.cache/threadbear-parity/ios-dd"
(cd "$ROOT/ios" && xcodebuild -project Threadbear.xcodeproj -scheme Threadbear -configuration Debug -destination "id=$SIM" \
  -derivedDataPath "$DD" build | grep -E " error:|BUILD (SUCCEEDED|FAILED)")
xcrun simctl ui "$SIM" appearance dark
xcrun simctl status_bar "$SIM" override --time 9:41 --batteryLevel 100 --batteryState charged --cellularBars 4
xcrun simctl uninstall "$SIM" com.mikecann.threadbear 2>/dev/null || true
xcrun simctl install "$SIM" "$DD/Build/Products/Debug-iphonesimulator/Threadbear.app"
TOKEN=$(/usr/bin/plutil -extract token raw -o - "$SEED" 2>/dev/null || python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["token"])' "$SEED")
LINK="threadbear://pair?url=$(python3 -c 'import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1], safe=""))' "$URL")&token=$TOKEN"
i=0
for id in $(python3 -c 'import json,sys; [print(t["threadId"]) for t in json.load(open(sys.argv[1]))["threads"]]' "$SEED"); do
  i=$((i + 1))
  xcrun simctl launch --terminate-running-process "$SIM" com.mikecann.threadbear -ThreadbearPair "$LINK" -ThreadbearOpen "$id" >/dev/null
  # Pairing, the live query and the thread's first page.
  sleep 8
  xcrun simctl io "$SIM" screenshot "$OUT/iphone-$i.png" >/dev/null 2>&1
  echo "$OUT/iphone-$i.png"
done
