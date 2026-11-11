#!/bin/bash
# Runs the BearDock demo on a real display and drives it with scripted events,
# saving screenshots. For a remote machine's desktop (Tinker Desk):
#
#   desktop/scripts/remote.sh tinker-desk --desk -- \
#     bash desktop/dock/shots.sh <folder under TMPDIR> <light|dark> '<events>'
#
# <events> is a BEARDOCK_EVENTS script (see src/bin/dock-demo/script.rs); write
# `shot OUT/name.png` for each picture and they land in the named folder.
# Build first: `cargo build -p threadbear-dock --features automation --bin dock-demo`.
set -euo pipefail
name="$1"; appearance="$2"; events="$3"
out="${TMPDIR:-/tmp}/$name"
mkdir -p "$out"
demo="${CARGO_TARGET_DIR:-$(cd "$(dirname "$0")/.." && pwd)/target}/debug/dock-demo"
script="${events//OUT/$out}"
# A leftover window from a run that crashed mustn't confuse the next one.
export BEARDOCK_FRESH=1 BEARDOCK_QUIT=1 BEARDOCK_APPEARANCE="$appearance" BEARDOCK_EVENTS="$script"
perl -e 'alarm shift; exec @ARGV' 60 "$demo"
echo "shots in $out:"
ls -la "$out"
