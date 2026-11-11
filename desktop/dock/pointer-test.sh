#!/bin/bash
# The real pointer and keyboard on the BearDock demo: CGEvents through the
# window server, not GPUI's scripted dispatch, so what's tested is what a person
# gets (mouse-down to a real window, events outside it, a real Escape press).
# Takes over the pointer, so it's only for a machine nobody's using: from the
# Mac you're on, run it on Tinker Desk's desktop:
#
#   desktop/scripts/remote.sh tinker-desk --desk -- bash desktop/dock/pointer-test.sh
#
# With `window` as the first argument it's a torn-out window instead: open one
# from the menu, drag its bar back over the dock with the real pointer, drop it
# in. With `out`, a tab dragged past the window's edge: the outline of the new
# window, then the window itself. Saves stills in $TMPDIR/dock-pointer (other
# windows as fNN-w-M.png). Build first:
# `cargo build -p threadbear-dock --features automation --bin dock-demo`.
set -euo pipefail
scenario="${1:-tabs}"
out="${TMPDIR:-/tmp}/dock-pointer"
rm -rf "$out"
mkdir -p "$out"
exec > >(tee -a "$out/log.txt") 2>&1
demo="${CARGO_TARGET_DIR:-$(cd "$(dirname "$0")/.." && pwd)/target}/debug/dock-demo"

cat > "$out/pointer.swift" <<'SWIFT'
import CoreGraphics
import Foundation

// args: originX originY scenario
let ox = Double(CommandLine.arguments[1])!, oy = Double(CommandLine.arguments[2])!
let scenario = CommandLine.arguments.count > 3 ? CommandLine.arguments[3] : "tabs"
let source = CGEventSource(stateID: .hidSystemState)
func at(_ x: Double, _ y: Double) -> CGPoint { CGPoint(x: ox + x, y: oy + y) }
func mouse(_ type: CGEventType, _ p: CGPoint) {
    CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
}
func escape() {
    for down in [true, false] {
        CGEvent(keyboardEventSource: source, virtualKey: 53, keyDown: down)?.post(tap: .cghidEventTap)
    }
}
func pause(_ s: Double) { Thread.sleep(forTimeInterval: s) }
func drag(from a: CGPoint, to b: CGPoint, seconds: Double) {
    let steps = 30
    for i in 1...steps {
        let t = Double(i) / Double(steps)
        mouse(.leftMouseDragged, CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t))
        pause(seconds / Double(steps))
    }
}

if scenario == "window" {
    // A right-click on the Highlights tab, "Open in New Window" in its
    // menu, then its window's title dragged over Transcript and dropped.
    let tab = at(1037, 15), item = at(1134, 60)
    mouse(.mouseMoved, tab); pause(0.3)
    mouse(.rightMouseDown, tab); pause(0.08); mouse(.rightMouseUp, tab); pause(0.7)
    mouse(.mouseMoved, item); pause(0.25)
    mouse(.leftMouseDown, item); pause(0.1); mouse(.leftMouseUp, item); pause(1.2)
    // The new window opens at the group's corner plus (30, 30); its title
    // text is inside the 78-point lights inset.
    let title = at(1010 + 120, 30 + 16), over = at(640, 390)
    mouse(.mouseMoved, title); pause(0.3)
    mouse(.leftMouseDown, title); pause(0.15)
    drag(from: title, to: over, seconds: 0.7)
    pause(1.3)              // still: the window over the compass, see-through
    mouse(.leftMouseUp, over); pause(1.2)
    exit(0)
}
if scenario == "out" {
    // The Worktrees tab, dragged out past the window's right edge, then
    // let go: the outline of the window it would open, then the window.
    let tab = at(1150, 15), away = at(1500, 300)
    mouse(.mouseMoved, tab); pause(0.3)
    mouse(.leftMouseDown, tab); pause(0.15)
    drag(from: tab, to: away, seconds: 0.6)
    pause(1.3)              // still: the outline, following the pointer
    mouse(.leftMouseUp, away); pause(1.2)
    exit(0)
}
// The Highlights tab, top right, to the middle of Transcript: the compass.
let tab = at(1037, 15), middle = at(640, 390), left = at(320, 560)
mouse(.mouseMoved, tab); pause(0.3)
mouse(.leftMouseDown, tab); pause(0.15)
drag(from: tab, to: middle, seconds: 0.5)
pause(1.2)              // stills: the compass, the ghost
escape(); pause(0.4)    // Escape flies it home; the release is ignored
mouse(.leftMouseUp, middle); pause(0.8)
// Again, past the transcript's left side: a side split; let go to dock it.
mouse(.mouseMoved, tab); pause(0.2)
mouse(.leftMouseDown, tab); pause(0.15)
drag(from: tab, to: left, seconds: 0.5)
pause(1.2)              // still: the side highlighted
mouse(.leftMouseUp, left); pause(1.2)
SWIFT

# Compiled first: the demo's clock starts when it launches.
swiftc -O -o "$out/pointer" "$out/pointer.swift"

# A film strip: a still every 300 ms or so while the pointer test runs.
events="wait 1800;pos $out/pos.txt"
for n in 01 02 03 04 05 06 07 08 09 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26; do
  events="$events;wait 300;shot $out/f$n.png;others $out/f$n-w"
done
export BEARDOCK_FRESH=1 BEARDOCK_QUIT=1 BEARDOCK_APPEARANCE="${BEARDOCK_APPEARANCE:-dark}" BEARDOCK_EVENTS="$events"
"$demo" &
demo_pid=$!
trap 'kill "$demo_pid" 2>/dev/null || true' EXIT
for _ in $(seq 60); do
  [ -s "$out/pos.txt" ] && break
  sleep 0.1
done
read -r ox oy < "$out/pos.txt" || true
echo "window at $ox,$oy"
# Keep the demo in front of whatever else is on this desktop.
osascript -e 'tell application "System Events" to set frontmost of first process whose unix id is '"$demo_pid"' to true' >/dev/null 2>&1 || true
sleep 0.3
"$out/pointer" "$ox" "$oy" "$scenario"
wait "$demo_pid" || true
echo "stills in $out"
ls "$out"
