// scripts/check-remote.sh against fake hosts (helpers/check-remote-harness.ts
// says how, and why it's several files): hosts that stop answering, connections lost, and runs past their limits.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { GiB, SCRIPT, macDf, setup, shortHome } from '../helpers/check-remote-harness.ts';

// THREADBEAR_NODE: find-node.sh finds the Node nvm installed in its home.
function onHost(checkExit: number, df: string, opts: { hang?: boolean; env?: Record<string, string> } = {}) {
  const home = shortHome();
  const base = path.join(home, '.cache', 'threadbear-check');
  const copy = path.join(base, 'runs', '1-1-0');
  fs.mkdirSync(path.join(copy, 'scripts'), { recursive: true });
  fs.copyFileSync(SCRIPT, path.join(copy, 'scripts', 'check-remote.sh'));
  fs.copyFileSync(path.join(path.dirname(SCRIPT), 'limit-step.mjs'), path.join(copy, 'scripts', 'limit-step.mjs'));
  fs.mkdirSync(path.join(copy, 'runner', 'scripts'), { recursive: true });
  fs.copyFileSync(path.join(path.dirname(SCRIPT), '..', 'runner', 'scripts', 'find-node.sh'), path.join(copy, 'runner', 'scripts', 'find-node.sh'));
  fs.writeFileSync(path.join(copy, 'check.sh'), `#!/bin/bash\necho made >"$TMPDIR/big"\n${opts.hang ? 'sleep 60\n' : ''}exit ${checkExit}\n`, { mode: 0o755 });
  const nodeBin = path.join(home, '.nvm', 'versions', 'node', 'v24.0.0', 'bin');
  fs.mkdirSync(nodeBin, { recursive: true });
  fs.symlinkSync(process.execPath, path.join(nodeBin, 'node'));
  fs.writeFileSync(path.join(nodeBin, 'df'), `#!/bin/bash\n[ "$*" = "-Pk ." ] || exit 9\ncat <<'END'\n${df}END\n`, { mode: 0o755 });
  const empty = path.join(home, 'empty-bin');
  fs.mkdirSync(empty, { recursive: true });
  const env: Record<string, string | undefined> = { ...process.env, HOME: home, PATH: empty, ...opts.env };
  delete env.THREADBEAR_NODE;
  delete env.NVM_DIR;
  const r = spawnSync('/bin/bash', [path.join(copy, 'scripts', 'check-remote.sh'), '--on-host', '1-1-0'], { encoding: 'utf8', env });
  return { r, base, kept: path.join(base, 'kept-1-1-0', 'free-kb') };
}

test('--on-host records the free space of a failed run for this Mac, before clearing up', () => {
  const { r, base, kept } = onHost(3, macDf(GiB));
  assert.equal(r.status, 3, r.stdout + r.stderr);
  assert.equal(fs.readFileSync(kept, 'utf8').trim(), String(GiB));
  assert.ok(!fs.existsSync(path.join(base, 'tmp-1-1-0')), 'temp files gone');
  assert.ok(!fs.existsSync(path.join(base, 'src')), 'copy gone');
});

test('--on-host records nothing when the run passed', () => {
  const { r, kept } = onHost(0, macDf(GiB));
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.ok(!fs.existsSync(kept));
});

// A host that stops answering partway: 8 Nov, Tinker Desk stopped taking ssh
// connections and a check sat in its ssh for 79 minutes, holding every landing.
const quickPing = { THREADBEAR_CHECK_PING_SECS: '1', THREADBEAR_CHECK_PING_MISSES: '2' };

test('a host that stops answering during the check is dropped, and the next one runs it', () => {
  const s = setup({ h1: { state: 'free', hang: true }, h2: { state: 'free' } });
  const started = Date.now();
  const r = s.run('h1,h2', 3600, '', quickPing);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.ok(Date.now() - started < 60_000, 'not left to wait out the hang');
  assert.match(r.stderr, /h1 stopped answering over ssh \(2 tries in a row, 1 s apart, couldn't log in\); trying the others\./);
  assert.match(r.stdout, /ran on h2 now/);
  assert.match(r.ssh, /^-O exit h1$/m, "h1's shared connection is closed");
});

test('with no other host, one that stops answering ends as unreachable, so check.sh runs it here', () => {
  const s = setup({ h1: { state: 'free', hang: true } });
  const r = s.run('h1', 3600, '', quickPing);
  assert.equal(r.status, 255, r.stdout + r.stderr);
  assert.match(r.stderr, /h1 stopped answering/);
  assert.match(r.stderr, /None of h1 can run this check now\./);
});

test('a host that stops answering during the copy is dropped too', () => {
  const s = setup({ h1: { state: 'free', hangCopy: true }, h2: { state: 'free' } });
  const r = s.run('h1,h2', 3600, '', quickPing);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stderr, /h1 stopped answering/);
  assert.match(r.stdout, /ran on h2 now/);
});

test('a host that is slow but still answers its pings is left to finish', () => {
  // Every ping logs in, so a check that's merely quiet is never cut off.
  const s = setup({ h1: { state: 'free' } });
  const r = s.run('h1', 3600, 'sleep 4', quickPing);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.doesNotMatch(r.stderr, /stopped answering/);
  assert.match(r.ssh, /^h1 true$/m, 'it was pinged while it ran');
});

test('a run past the overall limit is stopped, and not as unreachable', () => {
  const s = setup({ h1: { state: 'free' } });
  const r = s.run('h1', 3600, 'sleep 30', { THREADBEAR_CHECK_PING_SECS: '1', THREADBEAR_CHECK_MAX_SECS: '3' });
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stderr, /This check has been going for 0 minutes, waiting in line included, so it was stopped\./);
});

test('a connection lost during the check goes to the next host, and with none to this Mac (255)', () => {
  const s = setup({ h1: { state: 'free', drop: true }, h2: { state: 'free' } });
  const r = s.run('h1,h2', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stderr, /Lost the connection to h1 during the check; trying the others\./);
  assert.match(r.stdout, /ran on h2 now/);
  const alone = setup({ h1: { state: 'free', drop: true } }).run('h1', 3600);
  assert.equal(alone.status, 255, alone.stdout + alone.stderr);
});

test('--on-host stops a run past its own limit, so a host this Mac gave up on frees its lock', () => {
  const started = Date.now();
  const { r } = onHost(0, macDf(100 * GiB), { hang: true, env: { THREADBEAR_CHECK_HOST_MAX_SECS: '2' } });
  // Not 124, which this Mac takes for a host that dropped out.
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.ok(Date.now() - started < 40_000);
  assert.match(r.stdout, /limit-step: error: still going after 2 s/);
});

test('a hung check that ignores SIGTERM is still killed, and the next host runs it', () => {
  const s = setup({ h1: { state: 'free', stubborn: true }, h2: { state: 'free' } });
  const started = Date.now();
  const r = s.run('h1,h2', 3600, '', quickPing);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.ok(Date.now() - started < 60_000, `took ${Date.now() - started} ms`);
  assert.match(r.stdout, /ran on h2 now/);
});
