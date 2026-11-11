#!/bin/bash
# Checks a Codex CLI's app-server protocol against what Threadbear relies on
# (src/adapters/codex/surface.ts): the methods, item types and fields the
# Codex adapter and normaliser use. E's update gate runs the same check
# before activating a new Codex; run this by hand to see what an update
# would change.
#
#   runner/scripts/codex-schema-check.sh [codex]
#
# codex defaults to THREADBEAR_CODEX_PATH, else the codex next to the
# runner's Node. `app-server generate-ts` is offline and needs no login; it
# runs with a temp CODEX_HOME so it can't touch ~/.codex.
#
# Exit status: 0 when nothing Threadbear uses is missing (new names are
# listed but fine), 1 when something is missing, 2 when it couldn't check.
set -euo pipefail

# THREADBARE_* was the old name for these variables: each stands in for its new name.
for v in $(compgen -e | grep '^THREADBARE_' || true); do n="THREADBEAR_${v#THREADBARE_}"; [ -n "${!n+x}" ] || export "$n=${!v}"; done
RUNNER="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Node: THREADBEAR_NODE, else the node on PATH, else nvm's newest
# Node 24 or newer, else Homebrew's (runner/scripts/find-node.sh).
. "$RUNNER/scripts/find-node.sh"
NODE="$(threadbear_find_node)" || exit 1
# Codex: as given, else the one beside the Node (npm's global install puts it
# there), else the one on PATH. Beside the Node alone missed it whenever the
# Node found wasn't the one Codex was installed with (Codex's review, 9 Nov).
CODEX="${1:-${THREADBEAR_CODEX_PATH:-}}"
if [ -z "$CODEX" ]; then
  CODEX="$(dirname "$NODE")/codex"
  [ -x "$CODEX" ] || CODEX="$(command -v codex 2>/dev/null || printf '%s' "$CODEX")"
fi

if [ ! -x "$CODEX" ]; then
  echo "No codex at $CODEX" >&2
  exit 2
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/codex-schema.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/home" "$WORK/ts"

# The launcher needs node on PATH.
export PATH="$(dirname "$NODE"):$PATH"
VERSION="$(CODEX_HOME="$WORK/home" "$CODEX" --version 2>/dev/null | awk '{print $NF}' || true)"
if ! CODEX_HOME="$WORK/home" "$CODEX" app-server generate-ts --experimental --out "$WORK/ts" >"$WORK/gen.log" 2>&1; then
  echo "codex app-server generate-ts failed:" >&2
  tail -20 "$WORK/gen.log" >&2
  exit 2
fi

"$NODE" --input-type=module -e '
const { checkSurface, testedVersion } = await import(process.argv[1]);
const { missing, added } = checkSurface(process.argv[2]);
const version = process.argv[3] || "unknown";
console.log(`Codex ${version} against the surface tested on ${testedVersion}:`);
if (!missing.length && !added.length) console.log("  everything Threadbear uses is there, and nothing new.");
for (const m of missing) console.log(`  missing: ${m}`);
for (const a of added) console.log(`  new: ${a}`);
process.exit(missing.length ? 1 : 0);
' "$RUNNER/src/adapters/codex/surface.ts" "$WORK/ts" "$VERSION"
