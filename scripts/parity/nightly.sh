#!/bin/bash
# The nightly parity run, from Mike's main Mac: copies main to Tinker Desk,
# runs scripts/parity/run.sh there in its desktop session (the simulator and
# the debug Mac app need one), and brings the pictures back to
# ~/Library/Caches/Threadbear/parity/<time>/ (index.html shows them side by
# side). The last 7 runs are kept.
#
#   scripts/parity/nightly.sh          run it now and wait (about 3 minutes warm)
#   scripts/parity/nightly.sh --check  for the schedule's check (5 minutes at
#                                      most): prints a finished run's problems
#                                      once, else starts a run in the
#                                      background and prints nothing
#
# The schedule runs --check at 2:00 and 4:00: the first starts the run, the
# second reports it, so a thread starts only when something didn't show.
set -uo pipefail
# The Mac to run it on: THREADBEAR_PARITY_HOST, else the first check host in
# runner.json (checkHosts, as check.sh reads it). It needs Xcode, Chrome and
# Desk Runner (~/.local/bin/desk) for the simulator and the debug app.
RUNNER_JSON="${THREADBEAR_HOME:-$HOME/Library/Application Support/Threadbear}/runner.json"
HOST=${THREADBEAR_PARITY_HOST:-$(/usr/bin/python3 -c 'import json,sys
try:
  j=json.load(open(sys.argv[1])); h=j.get("checkHosts") or j.get("checkHost") or []
  h=[h] if isinstance(h,str) else h
  print(h[0] if h else "")
except Exception: print("")' "$RUNNER_JSON")}
[ -n "$HOST" ] || { echo "No Mac to run it on: set THREADBEAR_PARITY_HOST or checkHosts in $RUNNER_JSON"; exit 1; }
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
KEEP="$HOME/Library/Caches/Threadbear/parity"
REMOTE=.cache/threadbear-parity
mkdir -p "$KEEP"
SSH=(ssh -o BatchMode=yes -o ConnectTimeout=10)

if [ "${1:-}" = "--check" ]; then
  latest=$(ls -d "$KEEP"/20* 2>/dev/null | sort | tail -1)
  if [ -n "$latest" ] && [ -f "$latest/results.json" ] && [ ! -f "$latest/.reported" ]; then
    touch "$latest/.reported"
    problems=$(/usr/bin/python3 -c 'import json,sys; [print(f) for f in json.load(open(sys.argv[1]))["failures"]]' "$latest/results.json" 2>/dev/null)
    [ -n "$problems" ] && printf 'The parity run of %s found:\n%s\nIts pictures: %s/index.html\n' "$(basename "$latest")" "$problems" "$latest"
    exit 0
  fi
  if [ -n "$latest" ] && [ -f "$latest/.failed" ] && [ ! -f "$latest/.reported" ]; then
    touch "$latest/.reported"
    printf "The parity run of %s didn't finish:\n%s\n" "$(basename "$latest")" "$(tail -20 "$latest/run.log")"
    exit 0
  fi
  # One at a time: a run still going is left to finish.
  [ -f "$KEEP/.running" ] && kill -0 "$(cat "$KEEP/.running")" 2>/dev/null && exit 0
  nohup "$0" > /dev/null 2>&1 &
  exit 0
fi

echo $$ > "$KEEP/.running"
trap 'rm -f "$KEEP/.running"' EXIT
stamp=$(date +%Y-%m-%d-%H%M)
dest="$KEEP/$stamp"
mkdir -p "$dest"
# What's committed on main, never a working tree another thread is editing
# (THREADBEAR_PARITY_REF names another commit, to try a change first).
main=$(git -C "$ROOT" rev-parse "${THREADBEAR_PARITY_REF:-main}")
echo "main at $main" > "$dest/run.log"
stage=$(mktemp -d)
git -C "$ROOT" archive "$main" | tar -x -C "$stage"
# Into the same folder each night, so packages and Swift builds stay warm;
# --delete keeps it to main's files, apart from what builds make.
"${SSH[@]}" "$HOST" "mkdir -p $REMOTE/src" &&
  rsync -a --delete --exclude node_modules --exclude .build --exclude .parity-packages -e "${SSH[*]}" "$stage/" "$HOST:$REMOTE/src/" >> "$dest/run.log" 2>&1
copied=$?
rm -rf "$stage"
if [ $copied -ne 0 ]; then
  echo "couldn't copy main to $HOST" >> "$dest/run.log"
  touch "$dest/.failed"
  exit 1
fi
"${SSH[@]}" "$HOST" "~/.local/bin/desk 'cd ~/$REMOTE/src && bash scripts/parity/run.sh ~/$REMOTE/out > ~/$REMOTE/run.log 2>&1; echo exit \$? >> ~/$REMOTE/run.log'" >/dev/null 2>&1
rsync -a -e "${SSH[*]}" "$HOST:$REMOTE/out/" "$dest/" >/dev/null 2>&1
"${SSH[@]}" "$HOST" "grep -v '^dev-app: screencapture' ~/$REMOTE/run.log" >> "$dest/run.log" 2>&1
[ -f "$dest/results.json" ] || touch "$dest/.failed"
# The last 7 runs.
ls -d "$KEEP"/20* 2>/dev/null | sort -r | tail -n +8 | while read -r old; do rm -rf "$old"; done
echo "$dest/index.html"
[ -f "$dest/results.json" ] && /usr/bin/python3 -c 'import json,sys; f=json.load(open(sys.argv[1]))["failures"]; [print(x) for x in f]; sys.exit(1 if f else 0)' "$dest/results.json"
