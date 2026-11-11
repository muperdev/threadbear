#!/bin/zsh
# Builds and opens the BearDock demo. BEARDOCK_FRESH=1 ignores the saved layout.
set -euo pipefail
cd "$(dirname "$0")"
../scripts/one-build.sh swift build --product DockDemo  # one Threadbear build at a time
exec .build/debug/DockDemo
