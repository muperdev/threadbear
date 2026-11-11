#!/bin/bash
# The parity run, on the host it was copied to (Tinker Desk; nightly.sh
# copies it there and runs this in its desktop session). It plays every
# scenario (runner/test/helpers/scenario-list.ts) into a throwaway Convex
# backend, then has the real website, iPhone app and Mac app read them:
# the website in headless Chrome, the iPhone app in a simulator, and the
# Mac app as another runner's threads through Convex, the way Mike sees a
# Hetzner or bruce thread. Each surface's picture of each scenario goes in
# <out>, with index.html showing them side by side, and results.json saying
# which words each surface didn't show. Exits 1 when any surface missed any.
#
# The unit tests compare the same scenarios in memory on every landing
# (scenarios.test.ts); this is what they can't see: the screens. Its first
# runs (9 Nov) found the website and the iPhone drawing a decision card
# without what each answer does.
#
#   scripts/parity/run.sh <out dir>
set -uo pipefail
OUT=$1
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
. "$ROOT/runner/scripts/find-node.sh"
NODE="$(threadbear_find_node)" || exit 1
export PATH="${NODE%/*}:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin"
rm -rf "$OUT"
mkdir -p "$OUT"
work=$(mktemp -d "${TMPDIR:-/tmp}/tb-parity.XXXXXX")
PORT=23210 SITE_PORT=23211 WEB_PORT=23280
URL="http://127.0.0.1:$PORT"
KEY="parity-$(openssl rand -hex 12)"
backend="" preview=""
cleanup() {
  [ -n "$preview" ] && kill "$preview" 2>/dev/null
  [ -n "$backend" ] && kill "$backend" 2>/dev/null
  wait 2>/dev/null
  rm -rf "$work"
}
trap cleanup EXIT
say() { echo "$(date '+%H:%M:%S') $*"; }

cd "$ROOT"
# The checkout's packages, once per lockfile.
stamp=$(cat package-lock.json runner/package-lock.json 2>/dev/null | shasum | cut -c1-12)
if [ "$(cat .parity-packages 2>/dev/null)" != "$stamp" ]; then
  say "installing packages"
  npm ci --no-audit --no-fund --silent && (cd runner && npm ci --no-audit --no-fund --silent) && echo "$stamp" > .parity-packages || { echo "npm ci failed"; exit 1; }
fi

# A throwaway backend: Convex's own local binary (what `npx convex dev
# --local` runs), on ports of its own, with its data in the run's folder.
BIN=$(ls -d "$HOME"/.cache/convex/binaries/*/convex-local-backend 2>/dev/null | sort | tail -1)
[ -n "$BIN" ] || { echo "no convex-local-backend in ~/.cache/convex/binaries: run 'npx convex dev --local' there once"; exit 1; }
SECRET=$(openssl rand -hex 32)
mkdir -p "$work/convex"
(cd "$work/convex" && exec "$BIN" --instance-name threadbear-parity --instance-secret "$SECRET" --port $PORT --site-proxy-port $SITE_PORT \
  --convex-origin "$URL" --convex-site "http://127.0.0.1:$SITE_PORT" db.sqlite3 > backend.log 2>&1) &
backend=$!
for _ in $(seq 1 60); do curl -sf "$URL/version" >/dev/null && break; sleep 0.5; done
curl -sf "$URL/version" >/dev/null || { echo "the backend didn't start:"; tail -20 "$work/convex/backend.log"; exit 1; }
export CONVEX_SELF_HOSTED_URL="$URL" CONVEX_SELF_HOSTED_ADMIN_KEY="$("$BIN" keygen admin-key --instance-name threadbear-parity --instance-secret "$SECRET")"
say "deploying the backend's functions"
npx convex deploy -y > "$work/deploy.log" 2>&1 || { echo "convex deploy failed:"; tail -20 "$work/deploy.log"; exit 1; }
npx convex env set RUNNER_KEY "$KEY" >/dev/null && npx convex env set PUSH_RELAY off >/dev/null

say "playing the scenarios into it"
node runner/scripts/parity-seed.ts "$URL" "$KEY" > "$OUT/seed.json" 2> "$work/seed.log" || { echo "seeding failed:"; tail -20 "$work/seed.log"; exit 1; }

say "the website"
VITE_CONVEX_URL="$URL" npx vite build --config web/vite.config.ts --outDir "$work/web" --emptyOutDir > "$work/web-build.log" 2>&1 || { echo "the website didn't build:"; tail -20 "$work/web-build.log"; exit 1; }
npx vite preview --config web/vite.config.ts --outDir "$work/web" --host 127.0.0.1 --port $WEB_PORT --strictPort > "$work/preview.log" 2>&1 &
preview=$!
for _ in $(seq 1 40); do curl -sf "http://127.0.0.1:$WEB_PORT/" >/dev/null && break; sleep 0.5; done
node scripts/parity/web-shots.mjs "http://127.0.0.1:$WEB_PORT" "$OUT/seed.json" "$OUT" > "$OUT/web.json" 2> "$work/web.log" || { echo "web shots failed:"; tail -20 "$work/web.log"; echo '[]' > "$OUT/web.json"; }

say "the iPhone app"
zsh scripts/parity/ios-shots.sh "$URL" "$OUT/seed.json" "$OUT" > "$work/ios.log" 2>&1 || { echo "iPhone shots failed:"; tail -20 "$work/ios.log"; }

say "the Mac app, reading them as another runner's"
bash scripts/parity/mac-shots.sh "$URL" "$KEY" "$OUT/seed.json" "$OUT" 2> "$work/mac.log" | sed -n '/^\[/,$p' > "$OUT/mac.json" || true
[ -s "$OUT/mac.json" ] || { echo "Mac shots failed:"; grep -v "^dev-app: screencapture" "$work/mac.log" | tail -20; echo '[]' > "$OUT/mac.json"; }

node scripts/parity/gallery.mjs "$OUT"
