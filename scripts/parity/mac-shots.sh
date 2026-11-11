#!/bin/bash
# The Mac app's half of the parity run (scripts/parity/run.sh): a dev runner
# on the run's throwaway backend with its owner key, so the seeded threads
# are another runner's to it and reach the app through Convex (relay.ts), as
# a Hetzner or bruce thread does on Mike's Mac. A debug app on that runner
# opens each scenario's thread, checks its rows show the words the scenario
# must show, and saves a picture. Prints JSON: each scenario's file and the
# words it didn't find.
#
#   scripts/parity/mac-shots.sh <convex url> <owner key> <seed.json> <out dir>
set -euo pipefail
URL=$1 KEY=$2 SEED=$3 OUT=$4
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
mkdir -p "$OUT"
work=$(mktemp -d "${TMPDIR:-/tmp}/tb-parity-mac.XXXXXX")
home="$work/home"
mkdir -p "$home/secrets"
# The main Mac's runner of a little fleet: the owner key reads other runners' threads.
printf '{ "sync": { "enabled": true, "url": "%s", "name": "Parity Mac" } }\n' "$URL" > "$home/runner.json"
printf '%s\n' "$KEY" > "$home/secrets/convex-key"
chmod 600 "$home/secrets/convex-key"
"$ROOT/runner/scripts/dev-runner.sh" --home "$home" > "$work/runner.log" 2>&1 &
runner=$!
app=""
cleanup() {
  [ -n "$app" ] && "$ROOT/app/Tools/dev-app.sh" stop --id "$app" >/dev/null 2>&1 || true
  kill "$runner" 2>/dev/null || true
  wait "$runner" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT
for _ in $(seq 1 60); do [ -x "$home/tb" ] && break; sleep 1; done
[ -x "$home/tb" ] || { echo "the dev runner didn't start:" >&2; tail -20 "$work/runner.log" >&2; exit 1; }
app=parity
"$ROOT/app/Tools/dev-app.sh" start --home "$home" --appearance dark --id "$app" >"$work/app.log" 2>&1 || { echo "the debug app didn't start:" >&2; tail -20 "$work/app.log" >&2; app=""; exit 1; }

node - "$SEED" "$OUT" "$ROOT/app/Tools/dev-app.sh" "$app" <<'EOF'
const { execFileSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const [seedFile, out, devApp, id] = process.argv.slice(2);
const seed = JSON.parse(fs.readFileSync(seedFile, 'utf8'));
const run = (...args) => execFileSync(devApp, [...args, '--id', id], { encoding: 'utf8' });
const results = [];
for (const [i, t] of seed.threads.entries()) {
  let missing = t.shows;
  // The relay's list and the thread's rows take a moment to come through Convex.
  for (let tries = 0; tries < 20 && missing.length; tries++) {
    try {
      run('do', JSON.stringify({ cmd: 'open', id: t.threadId }));
      const dump = run('dump');
      missing = t.shows.filter((w) => !dump.includes(w));
    } catch {}
    if (missing.length) execFileSync('sleep', ['1']);
  }
  const file = `mac-${i + 1}.png`;
  run('shot', path.join(out, file));
  results.push({ name: t.name, file, missing });
}
console.log(JSON.stringify(results, null, 2));
EOF
