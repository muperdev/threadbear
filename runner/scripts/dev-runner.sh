#!/bin/bash
# Runs this working tree's runner in the foreground on a throwaway data dir,
# to try a change without touching the installed runner.
#
#   scripts/dev-runner.sh [--home DIR] [--port N] [--keep]
#
#   --home DIR   data dir (default: a new temp folder). The real one, and any
#                data dir an installed runner uses, are refused.
#   --port N     port (default 0: a free one, written to <home>/runner.json
#                where tb and the app find it)
#   --keep       keep the temp data dir when it stops (it holds the threads'
#                conversations and runner.log; by default a temp one is
#                deleted on Ctrl-C or kill, and kept if the runner failed)
#
# It writes <home>/tb, a tb that always talks to this runner (an exported
# THREADBEAR_HOME doesn't last past one command in an agent's shell), and
# prints the app/Tools/dev-app.sh command for a debug app against it. On a
# new data dir, threads default to the folder <home>/work (under $TMPDIR
# when <home> isn't), the login claude-main (TB_E2E_ACCOUNT picks another)
# and model claude-haiku-5-5, and Codex threads to gpt-6-luna at low effort
# (runner.json's defaults; keepAccount keeps new threads on that login rather
# than the one with the most room), not your real folders or the
# account's own model, so a thread started from a debug app's composer is
# a cheap one.
# It never updates a CLI (cliUpdates off): Codex runs from the npm copy
# next to its Node. Like launchd, it starts the runner again when it
# exits 0 (runner.restart, tb restart), and stops when it exits with an
# error. Ctrl-C, or `kill <pid>` with the pid it prints, stops it and the
# CLIs it started.
set -euo pipefail

# THREADBARE_* was the old name for these variables: each stands in for its new name.
for v in $(compgen -e | grep '^THREADBARE_' || true); do n="THREADBEAR_${v#THREADBARE_}"; [ -n "${!n+x}" ] || export "$n=${!v}"; done
RUNNER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Node: THREADBEAR_NODE, else the node on PATH, else nvm's newest
# Node 24 or newer, else Homebrew's (runner/scripts/find-node.sh).
. "$RUNNER_DIR/scripts/find-node.sh"
NODE="$(threadbear_find_node)" || exit 1
DEFAULT_HOME="$HOME/Library/Application Support/Threadbear"
DATA_HOME=""
PORT=0
KEEP=0

while [ $# -gt 0 ]; do
  case "$1" in
    --home) DATA_HOME="${2:?--home needs a folder}"; shift ;;
    --port) PORT="${2:?--port needs a number}"; shift ;;
    --keep) KEEP=1 ;;
    -h|--help) awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
  shift
done


# The folder with symlinks resolved, or as given if it doesn't exist yet.
real_dir() {
  if [ -d "$1" ]; then (cd "$1" && pwd -P); else printf '%s' "${1%/}"; fi
}

MADE_HOME=0
if [ -z "$DATA_HOME" ]; then
  TMP_ROOT="${TMPDIR:-/tmp}"
  DATA_HOME="$(mktemp -d "${TMP_ROOT%/}/threadbear-dev.XXXXXX")"
  MADE_HOME=1
fi
case "$DATA_HOME" in /*) ;; *) DATA_HOME="$PWD/$DATA_HOME" ;; esac
if [ "$(real_dir "$DATA_HOME")" = "$(real_dir "$DEFAULT_HOME")" ]; then
  echo "That's the real runner's data dir. Leave --home out to get a temp one." >&2
  exit 1
fi
mkdir -p "$DATA_HOME"
DATA_HOME="$(real_dir "$DATA_HOME")"
# A data dir with releases belongs to an installed runner (a sandbox
# install, say); its lock would stop this one anyway, but say why.
if [ -e "$DATA_HOME/runner/launch.mjs" ]; then
  echo "$DATA_HOME belongs to an installed runner. Use another --home." >&2
  exit 1
fi

if [ ! -d "$RUNNER_DIR/node_modules/@anthropic-ai/claude-agent-sdk" ] || [ ! -d "$RUNNER_DIR/node_modules/ws" ]; then
  echo "Installing the runner's packages..."
  (cd "$RUNNER_DIR" && PATH="$(dirname "$NODE"):$PATH" npm ci)
fi

LOG="$DATA_HOME/runner.log"
TB="$DATA_HOME/tb"
WORK="$DATA_HOME/work"
# Agent Archive skips transcripts from folders under $TMPDIR (Mike's
# privacy rules since 29 Oct), so a try-out thread doesn't land in his
# archive. A --home elsewhere, like /tmp, gets a work folder under $TMPDIR.
TMP_REAL="$(real_dir "${TMPDIR:-/tmp}")"
case "$(real_dir "$DATA_HOME")/" in
  "$TMP_REAL"/*) ;;
  *) [ -e "$DATA_HOME/runner.json" ] || WORK="$(mktemp -d "${TMP_REAL%/}/threadbear-dev-work.XXXXXX")" ;;
esac
# The runner's defaults are your real folder, the last account used and
# the account's own model (Fable with a 1M context, say); a thread
# made to try a change belongs in a scratch folder on the cheapest model,
# whether tb or a debug app's composer starts it. (The account and model
# are the ones the end-to-end tests use.)
IN_WORK=""
if [ ! -e "$DATA_HOME/runner.json" ]; then
  mkdir -p "$WORK"
  # With CLI updates off, the runner takes the codex beside its Node's real
  # file (process.execPath, past links: Homebrew's bin/node is in its
  # Cellar), which isn't there when Codex was installed elsewhere: then the
  # one on PATH, as codexPath (Codex's review, 9 Nov 2026).
  DEV_CODEX=""
  [ -n "${THREADBEAR_CODEX_PATH:-}" ] || DEV_CODEX="$(command -v codex 2>/dev/null || true)"
  "$NODE" -e 'const fs = require("fs"), path = require("path");
  const beside = fs.existsSync(path.join(path.dirname(process.execPath), "codex"));
  fs.writeFileSync(process.argv[1], JSON.stringify({
    defaults: { cwd: process.argv[2], accountId: process.argv[3], keepAccount: true, model: "claude-haiku-5-5", models: { codex: "gpt-6-luna" }, efforts: { codex: "low" } },
    cliUpdates: { claude: "off", codex: "off" },
    ...(process.argv[4] && !beside ? { codexPath: process.argv[4] } : {}),
  }, null, 2) + "\n")' \
    "$DATA_HOME/runner.json" "$WORK" "${TB_E2E_ACCOUNT:-claude-main}" "$DEV_CODEX"
  IN_WORK="   (in $(printf '%q' "$WORK"), on claude-haiku-5-5)"
fi
# THREADBEAR_PORT and THREADBEAR_URL go too: tb then reads the port this
# runner wrote to runner.json. THREADBEAR_TB is what tb prints in the
# commands it suggests (restart --cancel, events), so pasting one reaches
# this runner rather than the live one a bare tb in a thread reaches.
{
  echo '#!/bin/bash'
  echo '# tb for the dev runner in this folder (scripts/dev-runner.sh wrote it).'
  printf 'exec env -u THREADBEAR_DATA_DIR -u THREADBEAR_URL -u THREADBEAR_PORT -u THREADBEAR_LOG THREADBEAR_HOME=%q THREADBEAR_TB=%q %q %q "$@"\n' \
    "$DATA_HOME" "$TB" "$NODE" "$RUNNER_DIR/bin/tb"
} > "$TB"
chmod +x "$TB"

echo "Dev runner: $RUNNER_DIR (working tree)"
if [ "$MADE_HOME" = 1 ] && [ "$KEEP" = 0 ]; then
  echo "  data: $DATA_HOME (deleted when it stops; --keep keeps it)"
else
  echo "  data: $DATA_HOME"
fi
echo "  log:  $LOG (also shown here)"
echo "  pid:  $$"
echo "For tb:      $(printf '%q' "$TB") status"
echo "A thread:    $(printf '%q' "$TB") new \"say hi\" --model claude-haiku-5-5 --wait$IN_WORK"
echo "On Codex:    $(printf '%q' "$TB") new \"say hi\" --account codex-main --model gpt-6-luna --effort low --wait"
echo "For the app: $(printf '%q' "$(cd "$RUNNER_DIR/.." && pwd)/app/Tools/dev-app.sh") start --home $(printf '%q' "$DATA_HOME")"
echo "Stop it:     kill $$   (never pkill: the real runner runs src/main.ts too)"
echo

cd "$RUNNER_DIR"
CHILD=""
# A temp data dir holds the dev threads' conversations and log. Left behind
# after every session, they piled up in $TMPDIR with nothing saying so.
forget_home() {
  if [ "$MADE_HOME" = 1 ] && [ "$KEEP" = 0 ]; then
    rm -rf "$DATA_HOME"
    echo "Deleted its data dir, $DATA_HOME."
  else
    echo "Its data (threads, runner.log) is in $DATA_HOME; rm -rf it when you're done."
  fi
}
stop() {
  trap - INT TERM HUP
  if [ -n "$CHILD" ]; then
    kill -TERM "$CHILD" 2>/dev/null || true
    wait "$CHILD" 2>/dev/null || true
  fi
  echo "The dev runner stopped."
  forget_home
  exit 0
}
trap stop INT TERM HUP

# Inside a thread these name the live runner and that thread; the dev
# runner sets its own for the CLIs it starts.
while :; do
  env -u THREADBEAR_DATA_DIR -u THREADBEAR_URL -u THREADBEAR_THREAD_ID -u THREADBEAR_RUNNER_VERSION -u THREADBEAR_RUNNER_HOME \
    THREADBEAR_HOME="$DATA_HOME" THREADBEAR_PORT="$PORT" THREADBEAR_LOG="$LOG" THREADBEAR_LOG_STDERR=1 THREADBEAR_CLI_UPDATES=off THREADBEAR_TOKEN_TALLY=off \
    "$NODE" src/main.ts &
  CHILD=$!
  CODE=0
  wait "$CHILD" || CODE=$?
  CHILD=""
  if [ "$CODE" != 0 ]; then
    echo "The dev runner exited with $CODE; not starting it again." >&2
    # Kept, since its log says why.
    echo "Its data (threads, runner.log) is in $DATA_HOME; rm -rf it when you're done." >&2
    exit "$CODE"
  fi
  # Exit 0 is a restart (runner.restart), which launchd would answer by
  # starting it again; so does this.
  echo "The dev runner restarted itself; starting it again."
  sleep 1
done
