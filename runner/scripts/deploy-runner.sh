#!/bin/bash
# Builds a runner release from this checkout and switches the live runner to
# it, restarting it once no thread is running. --help lists the steps and
# options; docs/dev-tools.md has the whole design. The work is in
# deploy.mjs; this only picks the Node. In a thread that's the one the
# runner runs on (it passes THREADBEAR_NODE on), and install-runner.sh
# passes the one it puts in the plist, so the checks and the release run on
# the same Node as the runner. By hand it's the one find-node.sh finds.
# Either way deploy.mjs moves over to the Node the installed runner starts
# releases with, if that's another (THREADBEAR_DEPLOY_NODE_GIVEN, which
# install-runner.sh sets, says not to).
#
#   scripts/deploy-runner.sh             build, smoke test, switch, restart
#   scripts/deploy-runner.sh --list      show the releases
#   scripts/deploy-runner.sh --rollback  back to the previous release
#
# THREADBEAR_LAUNCHCTL replaces launchctl (the tests stub it).
set -euo pipefail

# THREADBARE_* was the old name for these variables: each stands in for its new name.
for v in $(compgen -e | grep '^THREADBARE_' || true); do n="THREADBEAR_${v#THREADBARE_}"; [ -n "${!n+x}" ] || export "$n=${!v}"; done
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$HERE/find-node.sh"
NODE="$(threadbear_find_node)" || exit 1
exec "$NODE" "$HERE/deploy.mjs" "$@"
