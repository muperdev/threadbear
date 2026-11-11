#!/bin/bash
# Runs a command from this checkout on another machine, so the Rust builds and
# the spec runs stay off the Mac you're working on (AGENTS.md).
#
#   desktop/scripts/remote.sh <ssh host> [--desk] -- <command...>
#
# It copies the checkout, committed or not, less its build folders, to a folder
# of its own under ~/.cache/threadbear-check/gpui there (so two checkouts at once
# don't overwrite each other), and runs the command from that folder, with Rust
# and Node on the PATH, npm packages installed the first time, and cargo's target
# folder kept between runs. --desk runs it on Tinker Desk's desktop (Desk Runner),
# which is what a window needs there.
#
# Everything lives under ~/.cache/threadbear-check, as scripts/check-remote.sh
# has it, so the host still looks like one with no Threadbear on it. Tinker
# Desk is also where clean installs get tried.
set -euo pipefail
host="${1:?usage: remote.sh <ssh host> [--desk] -- <command>}"; shift
desk=0
if [ "${1:-}" = "--desk" ]; then desk=1; shift; fi
[ "${1:-}" != "--" ] || shift
[ $# -gt 0 ] || { echo "no command given" >&2; exit 2; }
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
base=".cache/threadbear-check/gpui"
key="$(printf '%s' "$root" | shasum | cut -c1-10)"
src="$base/src-$key"
target="$base/target-$key"

# A checkout that hasn't been run for a few days gives its folders back: a
# target is many GB and the machines are short of disk. The bare src and target
# are from before each checkout had its own. The target is touched as well as
# the src because cargo working inside it doesn't change its time, and a folder
# in daily use would otherwise look a few days old.
ssh "$host" "mkdir -p $src $target; touch $src $target; find $base -mindepth 1 -maxdepth 1 \\( -name 'src-*' -o -name 'target-*' -o -name src -o -name target \\) -mtime +3 ! -name 'src-$key' ! -name 'target-$key' -exec rm -rf {} + 2>/dev/null; true"
# And a host short of disk gets room back sooner, or refuses the run.
ssh "$host" "cd $base && bash -s -- $key" <"$root/desktop/scripts/trim-remote-cache.sh"
# Named by hand, not read from .gitignore: rsync takes those rules from the
# receiving side when it decides what to delete, which removed part of a
# Swift build and a node_modules here on 9 Nov.
rsync -az --delete --exclude .git --exclude node_modules --exclude .build --exclude target --exclude reports \
  --exclude .fixtures --exclude .swiftpm --exclude dist --exclude DerivedData --exclude .DS_Store \
  "$root/" "$host:$src/"

printf -v cmd '%q ' "$@"
read -r -d '' prelude <<PRELUDE || true
set -e
cd "\$HOME/$src"
# rsync just set this folder's time to the Mac's, and the age limit above reads it.
touch .
# So trim-remote-cache.sh can tell this target's in use for the whole run. Its
# output goes nowhere, or ssh would wait for it after the command ends.
# The mkdir is for a host so full the one above failed, until the trim made room.
# || true because this inherits set -e, and a touch on a full disk mustn't end it.
mkdir -p "\$HOME/$target"
( while kill -0 \$\$ 2>/dev/null; do touch "\$HOME/$target/.in-use" || true; sleep 60; done ) >/dev/null 2>&1 </dev/null &
. runner/scripts/find-node.sh
NODE="\$(threadbear_find_node)"
export PATH="\${NODE%/*}:\$PATH"
if [ -x "\$HOME/$base/cargo/bin/cargo" ]; then
  export CARGO_HOME="\$HOME/$base/cargo" RUSTUP_HOME="\$HOME/$base/rustup"
  export PATH="\$CARGO_HOME/bin:\$PATH"
elif [ -x "\$HOME/.cargo/bin/cargo" ]; then
  export PATH="\$HOME/.cargo/bin:\$PATH"
fi
# A target folder per checkout. Cargo's names for path crates ignore where the
# checkout is, so with a shared one each checkout's build replaced the other's.
export CARGO_TARGET_DIR="\$HOME/$target" CARGO_HTTP_MULTIPLEXING=false CARGO_NET_RETRY=10
export TMPDIR="\$HOME/$base/tmp"
mkdir -p "\$TMPDIR"
# What the specs need, the first time this folder's used.
for dir in features app/Tools/mock-runner runner; do
  [ -d "\$dir/node_modules" ] || npm ci --silent --prefix "\$dir" >/dev/null 2>&1 || true
done
PRELUDE

if [ "$desk" = 1 ]; then
  script=".cache/threadbear-check/gpui/run-$$.sh"
  ssh "$host" "cat > $script" <<<"$prelude
$cmd"
  # Desk Runner needs the screen awake for anything it draws.
  ssh "$host" "caffeinate -u -t 2 >/dev/null 2>&1 || true; ~/.local/bin/desk \"bash \\\$HOME/$script\"; rc=\$?; rm -f $script; exit \$rc"
else
  ssh "$host" "bash -s" <<<"$prelude
$cmd"
fi
