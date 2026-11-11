// How setup self-hosts the backend (scripts/self-hosted-plan.mjs): on the
// tailnet or on this Mac only, which a later run keeps, and the container
// settings for each, never published beyond loopback.
import { test } from 'node:test';
import assert from 'node:assert/strict';

type Flags = { selfHosted?: boolean; local?: boolean; tailnet?: boolean };
type Plan = { selfHosted?: boolean; mode?: 'local' | 'tailnet'; from?: string; fail?: string };
type Container = { env: Record<string, string>; runArgs: string[]; cmd: string[] };
// Plain JS with no types, as setup runs before any packages are installed.
const planModule: string = '../../../scripts/self-hosted-plan.mjs';
const m = (await import(planModule)) as {
  selfHostedPlan: (p: { flags?: Flags; joining?: boolean; adminKeyFile?: boolean; syncUrl?: string }) => Plan;
  backendContainer: (mode: string, tailnet?: { name: string; ip: string }) => Container;
  origins: (mode: string, name?: string) => { url: string; site: string; dashboard: string };
  isLoopbackUrl: (u: string) => boolean;
};

const LOCAL_URL = 'http://127.0.0.1:13210';
const TAILNET_URL = 'https://mac.tail1.ts.net:3210';

test('a first setup self-hosts only when asked, on the tailnet unless --local', () => {
  assert.deepEqual(m.selfHostedPlan({}), { selfHosted: false });
  assert.deepEqual(m.selfHostedPlan({ flags: { selfHosted: true } }), { selfHosted: true, mode: 'tailnet' });
  assert.deepEqual(m.selfHostedPlan({ flags: { selfHosted: true, local: true } }), { selfHosted: true, mode: 'local' });
  // --local on its own means self-hosted too.
  assert.deepEqual(m.selfHostedPlan({ flags: { local: true } }), { selfHosted: true, mode: 'local' });
  assert.ok(m.selfHostedPlan({ flags: { local: true, tailnet: true } }).fail);
});

test('setup run again keeps the way it was set up, and only a flag moves it', () => {
  // No flags: the admin key file says self-hosted, the loopback sync.url says local.
  assert.deepEqual(m.selfHostedPlan({ adminKeyFile: true, syncUrl: LOCAL_URL }), { selfHosted: true, mode: 'local' });
  assert.deepEqual(m.selfHostedPlan({ adminKeyFile: true, syncUrl: TAILNET_URL }), { selfHosted: true, mode: 'tailnet' });
  // Plain --self-hosted on a local Mac stays local: an agent rerunning the
  // docs' command doesn't put it on a (work) tailnet.
  assert.deepEqual(m.selfHostedPlan({ flags: { selfHosted: true }, adminKeyFile: true, syncUrl: LOCAL_URL }), { selfHosted: true, mode: 'local' });
  assert.deepEqual(m.selfHostedPlan({ flags: { tailnet: true }, adminKeyFile: true, syncUrl: LOCAL_URL }), { selfHosted: true, mode: 'tailnet', from: 'local' });
  assert.deepEqual(m.selfHostedPlan({ flags: { selfHosted: true, local: true }, adminKeyFile: true, syncUrl: TAILNET_URL }), { selfHosted: true, mode: 'local', from: 'tailnet' });
  // An admin key with no sync.url: a first setup stopped partway. Without a
  // flag it asks rather than guess the tailnet; with one it carries on.
  assert.ok(m.selfHostedPlan({ adminKeyFile: true }).fail);
  assert.ok(m.selfHostedPlan({ flags: { selfHosted: true }, adminKeyFile: true }).fail);
  assert.deepEqual(m.selfHostedPlan({ flags: { local: true }, adminKeyFile: true }), { selfHosted: true, mode: 'local' });
  // A loopback sync.url without the admin key file isn't a self-hosted Mac
  // (a dev deployment, say): setup doesn't take it for one.
  assert.deepEqual(m.selfHostedPlan({ syncUrl: LOCAL_URL }), { selfHosted: false });
});

test('a Mac joining another has no backend, and the self-hosted flags are refused there', () => {
  assert.deepEqual(m.selfHostedPlan({ joining: true, adminKeyFile: false }), { selfHosted: false });
  for (const flags of [{ selfHosted: true }, { local: true }, { tailnet: true }]) assert.ok(m.selfHostedPlan({ joining: true, flags }).fail);
});

test('every port either backend publishes is on 127.0.0.1, never the LAN', () => {
  for (const c of [m.backendContainer('local'), m.backendContainer('tailnet', { name: 'mac.tail1.ts.net', ip: '100.64.0.1' })]) {
    const published = c.runArgs.flatMap((a, i) => (c.runArgs[i - 1] === '-p' ? [a] : []));
    assert.equal(published.length, 2);
    for (const p of published) assert.match(p, /^127\.0\.0\.1:\d+:\d+$/);
    assert.ok(!c.runArgs.includes('--network') && !c.runArgs.includes('--net'));
  }
});

test('a local backend has one origin for the browser, the runner and the container itself', () => {
  const c = m.backendContainer('local');
  const o = m.origins('local');
  assert.equal(o.url, LOCAL_URL);
  assert.equal(c.env.CONVEX_CLOUD_ORIGIN, o.url);
  assert.equal(c.env.CONVEX_SITE_ORIGIN, o.site);
  // Published on the same port inside as out, so the origin reaches the
  // backend from inside the container too (the website's stored files).
  assert.ok(c.runArgs.includes('127.0.0.1:13210:13210') && c.runArgs.includes('127.0.0.1:13211:13211'));
  const script = c.cmd.join(' ');
  assert.match(script, /--port 13210/);
  assert.match(script, /--site-proxy-port 13211/);
  // It stops rather than run on the image's own ports if the script it edits changes.
  assert.match(script, /grep -q/);
  assert.ok(!c.runArgs.includes('--add-host'));
  assert.ok(Object.values(o).every(m.isLoopbackUrl));
});

test('a tailnet backend is unchanged by the local way (7 Nov containers aren\'t made again)', () => {
  const c = m.backendContainer('tailnet', { name: 'mac.tail1.ts.net', ip: '100.64.0.1' });
  assert.deepEqual(c.cmd, []);
  assert.deepEqual(c.runArgs, ['-p', '127.0.0.1:13210:3210', '-p', '127.0.0.1:13211:3211', '-v', 'threadbear-convex-data:/convex/data', '--add-host', 'mac.tail1.ts.net:100.64.0.1']);
  assert.deepEqual(c.env, { CONVEX_CLOUD_ORIGIN: TAILNET_URL, CONVEX_SITE_ORIGIN: 'https://mac.tail1.ts.net:3211', DISABLE_BEACON: 'true' });
  assert.equal(m.origins('tailnet', 'mac.tail1.ts.net').url, TAILNET_URL);
});

test('only loopback URLs count as local', () => {
  for (const u of [LOCAL_URL, 'http://localhost:13210', 'http://[::1]:13210']) assert.ok(m.isLoopbackUrl(u), u);
  for (const u of [TAILNET_URL, 'https://happy-otter-123.convex.cloud', 'http://127.0.0.1.evil.com', 'not a url']) assert.ok(!m.isLoopbackUrl(u), u);
});
