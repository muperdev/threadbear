// What setup does for a self-hosted backend (scripts/setup.mjs, "a
// self-hosted backend"; docs/self-hosting.md), worked out apart from doing
// it so a unit test can check it (runner/test/unit/self-hosted-plan.test.ts).
// Node built-ins only, like setup (and secure-url.ts, which has none).
import { isLoopbackHost } from '../runner/src/secure-url.ts';
//
// Two ways to reach it:
//   tailnet  `tailscale serve` puts it on this Mac's tailnet name, for the
//            phone and other Macs (setup --self-hosted)
//   local    loopback only, no Tailscale: this Mac alone, no phone
//            (setup --self-hosted --local). For someone whose work keeps
//            them on its own tailnet, as Tailscale has one on at a time.

// Tailnet port, then this Mac's loopback port. Not 3210 on loopback: that's
// where `npx convex dev` runs a local deployment.
export const PORTS = { api: [3210, 13210], site: [3211, 13211], dashboard: [6791, 16791] };
export const ADMIN_URL = `http://127.0.0.1:${PORTS.api[1]}`;
export const DATA_VOLUME = 'threadbear-convex-data';

// A URL on this Mac's loopback: a local backend's sync.url.
export function isLoopbackUrl(raw) {
  let u;
  try {
    u = new URL(raw);
  } catch {
    return false;
  }
  return isLoopbackHost(u.hostname);
}

// Whether setup self-hosts, and which way. The flags win; with none, an
// earlier self-hosted setup carries on as it was (its admin key file says
// self-hosted, runner.json's loopback sync.url says local).
//
// Plain --self-hosted on a Mac that's already local stays local: moving to
// the tailnet takes --tailnet, so an agent running setup again from the
// docs never puts the backend on a tailnet it wasn't meant for (the one
// this is for is a work tailnet).
//
//   flags    { selfHosted, local, tailnet } from the command line
//   joining  this Mac joins another's Threadbear (--existing, or a runner key)
//   adminKeyFile  secrets/convex-self-hosted.env is here
//   syncUrl  runner.json's sync.url, if any
//
// Returns { selfHosted: false } or { selfHosted: true, mode, from? } (from:
// the mode it had, when this moves it), or { fail } with why.
export function selfHostedPlan({ flags = {}, joining = false, adminKeyFile = false, syncUrl }) {
  const { local = false, tailnet = false } = flags;
  if (local && tailnet) return { fail: '--local and --tailnet pick opposite ways to reach the backend; use one.' };
  const asked = Boolean(flags.selfHosted || local || tailnet);
  if (joining) {
    if (asked) return { fail: 'A Mac joining another Mac\'s Threadbear (--existing) has no backend of its own, so --self-hosted, --local and --tailnet don\'t apply here.' };
    return { selfHosted: false };
  }
  if (!asked && !adminKeyFile) return { selfHosted: false };
  // Only a Mac that's self-hosted already has a way it was reached.
  const had = adminKeyFile && syncUrl ? (isLoopbackUrl(syncUrl) ? 'local' : 'tailnet') : undefined;
  // An admin key but no sync.url: a first setup stopped partway, and which
  // way it was going isn't written anywhere. Guessing the tailnet could put
  // it on a work one, so it asks (cross-lab review, 10 Nov).
  if (adminKeyFile && !syncUrl && !local && !tailnet) return { fail: 'An earlier self-hosted setup stopped before it finished, so setup can\'t tell which way it was going. Run it again with --self-hosted --local (this Mac only) or --self-hosted --tailnet.' };
  const mode = local ? 'local' : tailnet ? 'tailnet' : (had ?? 'tailnet');
  return { selfHosted: true, mode, ...(had && had !== mode ? { from: had } : {}) };
}

// The backend container's settings for each way. Every port is published on
// 127.0.0.1 only, never the LAN: the tailnet way puts them on the tailnet
// with `tailscale serve`, and the local way leaves them on this Mac.
//
// The backend fetches its own stored files (the website, from the
// static-hosting component) through CONVEX_CLOUD_ORIGIN, from inside the
// container, and hands the same origin to browsers in file URLs. So that
// origin has to work in both places:
//   tailnet  https://<mac>.<tailnet>.ts.net:3210, with --add-host so the
//            container reaches that name through the Mac's tailscale serve
//   local    http://127.0.0.1:13210, which inside the container is the
//            container itself. The image binds 3210 and 3211 inside, fixed
//            in its run_backend.sh (a second --port is refused), so the
//            container runs a copy of that script with 13210 and 13211, and
//            publishes them on the same numbers. It stops, saying why, if a
//            new image's script no longer has the lines it changes.
//            Pointing it at host.docker.internal instead was no good: file
//            URLs would name a host this Mac's browser can't find.
export function backendContainer(mode, tailnet = {}) {
  const volume = ['-v', `${DATA_VOLUME}:/convex/data`];
  if (mode === 'local') {
    const [, api] = PORTS.api;
    const [, site] = PORTS.site;
    const script = [
      'set -e',
      `sed -e 's/--port 3210 /--port ${api} /' -e 's/--site-proxy-port 3211 /--site-proxy-port ${site} /' ./run_backend.sh > /tmp/run_backend.sh`,
      `grep -q -- '--port ${api} ' /tmp/run_backend.sh && grep -q -- '--site-proxy-port ${site} ' /tmp/run_backend.sh || { echo "threadbear: this image's run_backend.sh no longer sets --port 3210 and --site-proxy-port 3211, so setup can't move them for a local backend" >&2; exit 1; }`,
      'exec bash /tmp/run_backend.sh',
    ].join('\n');
    return {
      env: { CONVEX_CLOUD_ORIGIN: `http://127.0.0.1:${api}`, CONVEX_SITE_ORIGIN: `http://127.0.0.1:${site}`, DISABLE_BEACON: 'true' },
      runArgs: ['-p', `127.0.0.1:${api}:${api}`, '-p', `127.0.0.1:${site}:${site}`, ...volume, '--entrypoint', '/bin/bash'],
      cmd: ['-c', script],
    };
  }
  const { name, ip } = tailnet;
  return {
    env: { CONVEX_CLOUD_ORIGIN: `https://${name}:${PORTS.api[0]}`, CONVEX_SITE_ORIGIN: `https://${name}:${PORTS.site[0]}`, DISABLE_BEACON: 'true' },
    runArgs: ['-p', `127.0.0.1:${PORTS.api[1]}:3210`, '-p', `127.0.0.1:${PORTS.site[1]}:3211`, ...volume, '--add-host', `${name}:${ip}`],
    cmd: [],
  };
}

// Where the runner, the website and the dashboard reach it.
export function origins(mode, name) {
  if (mode === 'local') {
    const at = (port) => `http://127.0.0.1:${port}`;
    return { url: at(PORTS.api[1]), site: at(PORTS.site[1]), dashboard: at(PORTS.dashboard[1]) };
  }
  const at = (port) => `https://${name}:${port}`;
  return { url: at(PORTS.api[0]), site: at(PORTS.site[0]), dashboard: at(PORTS.dashboard[0]) };
}
