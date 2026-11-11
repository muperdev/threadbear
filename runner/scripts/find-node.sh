# Sourced by the repo's shell scripts to pick the Node they run on:
#
#   . "<repo>/runner/scripts/find-node.sh"
#   NODE="$(threadbear_find_node)" || exit 1
#
# It prints the path, or says on stderr what's needed and fails.
#
# In order:
#   1. THREADBEAR_NODE, as given. The runner sets it in every thread to the
#      Node it runs on itself (threadEnv in runner/src/restart.ts), so a
#      thread's checks, tb land and deploys use the same Node as the runner.
#   2. `node` on PATH.
#   3. The newest Node 24 or newer installed by nvm, as an ssh session to a
#      check host has no nvm on PATH.
#   4. Homebrew's, for the same reason.
#
# Until 9 Nov 2026 every script fell back to Mike's own
# ~/.nvm/versions/node/v24.22.1, so in anyone else's copy (Homebrew's Node,
# another nvm version) check.sh and tb land stopped at "No Node binary"
# unless THREADBEAR_NODE was set. A fork's commit said so.

# Patch trust follows what a script runs (runner/src/script-trust.ts), and
# can't follow node run with code (-e) or with no script (--version reads
# nothing, but it can't know that). So nothing here runs node: the one on
# PATH is taken as it is, as before, and nvm's are told by their folder
# (v24.1.0). install-runner.sh checks the version of the one it pins.
# Whether version $1 (24.10.0) is newer than $2.
threadbear_newer() {
  local a1="${1%%.*}" a3="${1##*.}" b1="${2%%.*}" b3="${2##*.}" a2 b2
  a2="${1#*.}"
  a2="${a2%%.*}"
  b2="${2#*.}"
  b2="${b2%%.*}"
  [ "$a1" -gt "$b1" ] || { [ "$a1" -eq "$b1" ] && { [ "$a2" -gt "$b2" ] || { [ "$a2" -eq "$b2" ] && [ "$a3" -gt "$b3" ]; }; }; }
}

threadbear_find_node() {
  local c v
  if [ -n "${THREADBEAR_NODE:-}" ]; then
    if [ -x "$THREADBEAR_NODE" ] && [ ! -d "$THREADBEAR_NODE" ]; then
      printf '%s' "$THREADBEAR_NODE"
      return 0
    fi
    echo "THREADBEAR_NODE is $THREADBEAR_NODE, which isn't a program. Point it at a Node 24 or newer, or unset it to use the node on PATH." >&2
    return 1
  fi
  c="$(command -v node 2>/dev/null || true)"
  if [ -n "$c" ]; then
    printf '%s' "$c"
    return 0
  fi
  # nvm's folders are v<major>.<minor>.<patch>: the newest 24 or newer, by
  # number (v24.10.0 is newer than v24.9.0).
  local nvm="${NVM_DIR:-$HOME/.nvm}/versions/node" best="" best_v=""
  for c in "$nvm"/v*/bin/node; do
    v="${c#"$nvm"/v}"
    v="${v%/bin/node}"
    case "$v" in *[!0-9.]* | *..* | .* | *.) continue ;; esac
    case "$v" in *.*.*.*) continue ;; *.*.*) ;; *) continue ;; esac
    [ "${v%%.*}" -ge 24 ] || continue
    [ -x "$c" ] && [ ! -d "$c" ] || continue
    if [ -z "$best" ] || threadbear_newer "$v" "$best_v"; then
      best="$c"
      best_v="$v"
    fi
  done
  if [ -n "$best" ]; then
    printf '%s' "$best"
    return 0
  fi
  for c in /opt/homebrew/bin/node /usr/local/bin/node; do
    if [ -x "$c" ]; then
      printf '%s' "$c"
      return 0
    fi
  done
  echo "Threadbear's scripts need Node 24 or newer, and found no node on PATH, in nvm or in Homebrew. Install it (https://nodejs.org, brew install node, or nvm install 24), or set THREADBEAR_NODE to one." >&2
  return 1
}
