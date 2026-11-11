// deploy.mjs's build steps (scripts/deploy-steps.mjs) with fake processes in
// place of npm: what a step says when its process never ran or was killed,
// and when the unit tests get a second go.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { tmpDir } from '../helpers/fakes.ts';

// A .mjs with no types, so imported by URL (typed any) rather than by name.
const steps = await import(pathToFileURL(path.join(import.meta.dirname, '..', '..', 'scripts', 'deploy-steps.mjs')).href);

type Run = { status: number | null; signal?: string | null; error?: Error; output?: string };

// Fake spawnSync: plays the runs in order, writing each one's output to the
// step's log as the real process would.
function fakeIo(runs: Run[]) {
  const said: string[] = [];
  const calls: { cmd: string; args: string[] }[] = [];
  const io = {
    tmpdir: tmpDir(),
    say: (s: string) => said.push(s),
    write: (s: string) => said.push(s),
    error: () => {},
    // No sleeps, rather than pmset's real count, unless a test says.
    slept: () => 0,
    spawn: (cmd: string, args: string[], opts: { stdio: [unknown, number, number] }) => {
      calls.push({ cmd, args });
      const r = runs.shift();
      assert.ok(r, `no run left for ${cmd} ${args.join(' ')}`);
      if (r.output) fs.writeSync(opts.stdio[1], r.output);
      return { status: r.status, signal: r.signal ?? null, error: r.error };
    },
  };
  return { io, said, calls };
}

const c = { npm: '/opt/node/bin/npm', node: '/opt/node/bin/node' };

test('step: a process that never started says why, in the error and its log', () => {
  const { io } = fakeIo([{ status: null, error: Object.assign(new Error('spawn /opt/node/bin/npm EAGAIN'), { code: 'EAGAIN' }) }]);
  assert.throws(
    () => steps.step('Unit tests', c.npm, ['test'], {}, io),
    (e: Error & { logFile: string }) => {
      assert.match(e.message, /Unit tests failed \(npm couldn't run: spawn \/opt\/node\/bin\/npm EAGAIN\); the whole output is in \S+$/);
      assert.match(fs.readFileSync(e.logFile, 'utf8'), /npm couldn't run: spawn \/opt\/node\/bin\/npm EAGAIN/);
      return true;
    },
  );
});

test('step: a killed process says which signal', () => {
  const { io } = fakeIo([{ status: null, signal: 'SIGKILL' }]);
  assert.throws(() => steps.step('Unit tests', c.npm, ['test'], {}, io), /Unit tests failed \(npm was stopped by SIGKILL\)/);
});

test('step: a plain failure says the exit code', () => {
  const { io } = fakeIo([{ status: 1, output: 'boom\n' }]);
  assert.throws(() => steps.step('Typecheck', c.npm, ['run', 'typecheck'], {}, io), /Typecheck failed \(npm exited with 1\); the whole output is in /);
});

test('unitTests: a run that failed without naming a test file gets the whole suite once more', () => {
  // As on 4 Nov (THREADBEAR-M): npm test died in 0.1s with an empty log.
  const { io, said, calls } = fakeIo([{ status: null, signal: 'SIGKILL' }, { status: 0 }]);
  steps.unitTests(c, {}, io);
  assert.deepEqual(calls.map((x) => [x.cmd, ...x.args]), [[c.npm, 'test'], [c.npm, 'test']]);
  assert.ok(said.some((s) => /running them once more/.test(s)), said.join('\n'));
});

test('unitTests: a break that fails both whole runs still stops the build', () => {
  const { io, calls } = fakeIo([{ status: 1, output: 'Error: Cannot find module\n' }, { status: 1, output: 'Error: Cannot find module\n' }]);
  assert.throws(() => steps.unitTests(c, {}, io), /Unit tests, again failed \(npm exited with 1\)/);
  assert.equal(calls.length, 2);
});

test('unitTests: failed files are rerun alone, not the whole suite', () => {
  const output = 'not ok 1 - thing\ntest at test/unit/flaky.test.ts:12:3\n';
  const { io, said, calls } = fakeIo([{ status: 1, output }, { status: 0 }]);
  steps.unitTests(c, {}, io);
  assert.equal(calls[1].cmd, c.node);
  assert.ok(calls[1].args.includes('test/unit/flaky.test.ts'));
  assert.ok(calls[1].args.includes('--test-concurrency=1'));
  assert.ok(said.some((s) => /^Flaky: test\/unit\/flaky\.test\.ts/.test(s)), said.join('\n'));
});

test('unitTests: a test that fails twice stops the build', () => {
  const output = 'test at test/unit/broken.test.ts:1:1\n';
  const { io } = fakeIo([{ status: 1, output }, { status: 1, output }]);
  assert.throws(() => steps.unitTests(c, {}, io), /Rerunning test\/unit\/broken\.test\.ts failed \(node exited with 1\)/);
});

test('unitTests: a rerun of many failed files still gets a log file it can open', () => {
  // 7 Nov: twelve files made the log's name too long, and the rerun never ran.
  const files = Array.from({ length: 12 }, (_, i) => `test/unit/a-fairly-long-test-file-name-${i}.test.ts`);
  const output = files.map((f) => `test at ${f}:1:1\n`).join('');
  const { io, said, calls } = fakeIo([{ status: 1, output }, { status: 0 }]);
  steps.unitTests(c, {}, io);
  assert.equal(calls.length, 2);
  assert.ok(files.every((f) => calls[1].args.includes(f)));
  assert.ok(said.some((s) => /^Flaky: /.test(s)), said.join('\n'));
});

// 90 s was too short with the Mac at a load of 94 on 18 cores (6 Nov): the
// release was fine and started in 7 s once it eased. The wait goes on while
// the Mac is that busy, up to a cap, and gives up at 90 s otherwise.
test('the smoke test waits longer for a release to listen while the Mac is overloaded', () => {
  assert.equal(steps.smokeTimedOut(89_000, 0.5), false);
  assert.equal(steps.smokeTimedOut(91_000, 0.5), true);
  assert.equal(steps.smokeTimedOut(91_000, 1.0), true);
  assert.equal(steps.smokeTimedOut(91_000, 5.2), false);
  assert.equal(steps.smokeTimedOut(9 * 60_000, 5.2), false);
  assert.equal(steps.smokeTimedOut(10 * 60_000 + 1, 5.2), true);
});

// Its hello was asked once, and on a busy Mac the handshake after a release
// said it was listening timed out and failed a healthy one (a fork's, 8 Nov).
test('the smoke test asks for hello again within the wait it had to start', async () => {
  let now = 0;
  let asked = 0;
  const p = await steps.smokeHello({
    probe: async () => { now += 7_000; return ++asked < 3 ? { state: asked === 1 ? 'unknown' : 'down' } : { state: 'up', version: 'v' }; },
    waited: () => now, load: () => 0.5, exited: () => null, sleep: async () => { now += 250; },
  });
  assert.equal(p.version, 'v');
  assert.equal(asked, 3);
});

test('the smoke test stops asking for hello when the release exits or its wait runs out', async () => {
  const io = { probe: async () => ({ state: 'unknown' }), waited: () => 91_000, load: () => 0.5, exited: () => null, sleep: async () => {} };
  await assert.rejects(steps.smokeHello(io), /hello got no answer after 91 s/);
  await assert.rejects(steps.smokeHello({ ...io, exited: () => ({ code: 1, signal: null }) }), /it exited \(1\) before hello/);
  // Overloaded, it keeps asking past 90 s, up to the same ten minutes.
  let asked = 0;
  const up = await steps.smokeHello({ ...io, load: () => 5, probe: async () => (++asked === 1 ? { state: 'unknown' } : { state: 'up' }) });
  assert.equal(up.state, 'up');
  await assert.rejects(steps.smokeHello({ ...io, waited: () => 600_001, load: () => 5 }), /hello got no answer after 600 s/);
});

// A Mac that sleeps mid-build stops the tests' clocks but not the wall
// clock's, so a step that failed across a sleep says nothing about the code.
// It isn't rerun (the rerun would be a second guess at the same thing) and
// update.ts tries the commit again later rather than marking it failed.
test('a step that failed while the Mac slept through it is tried again later, not taken as a failure', () => {
  const { io, calls } = fakeIo([{ status: 1, output: 'test at test/unit/upstream.test.ts:227:1\n' }]);
  let n = 4;
  assert.throws(
    () => steps.unitTests(c, {}, { ...io, slept: () => n++ }),
    (e: Error) => e instanceof steps.SleptThrough && /^The Mac slept during Unit tests \(npm exited with 1; the output is in \S+\), so it's tried again later$/.test(e.message),
  );
  assert.equal(calls.length, 1, 'no rerun');
});

test("a step that failed with no sleep, or where the sleep count can't be read, is a failure as before", () => {
  for (const slept of [() => 4, () => undefined]) {
    const { io } = fakeIo([{ status: 1 }]);
    assert.throws(() => steps.step('Typecheck', c.npm, [], {}, { ...io, slept }), (e: Error) => e instanceof steps.StepFailed && /npm exited with 1/.test(e.message));
  }
  // And a step that passed across a sleep passed.
  const { io } = fakeIo([{ status: 0 }]);
  let n = 0;
  steps.step('Typecheck', c.npm, [], {}, { ...io, slept: () => n++ });
});

test('checkHostsFrom reads runner.json the way check.sh does', () => {
  assert.deepEqual(steps.checkHostsFrom({ checkHost: 'tinker-desk' }), ['tinker-desk']);
  assert.deepEqual(steps.checkHostsFrom({ checkHost: 'tinker-desk', checkHosts: ['tinker-desk', 'cloud'] }), ['tinker-desk', 'cloud']);
  // Not a plain host name (an option, a shell word): dropped, as check.sh does.
  assert.deepEqual(steps.checkHostsFrom({ checkHosts: ['-oProxyCommand=x', 'a b', 'cloud', 7] }), ['cloud']);
  assert.deepEqual(steps.checkHostsFrom({}), []);
  assert.deepEqual(steps.checkHostsFrom(undefined), []);
});

// 9 Nov: every self-update ran the whole unit suite on Mike's M5.
function withCheckSh() {
  const root = tmpDir();
  fs.writeFileSync(path.join(root, 'check.sh'), '#!/bin/bash\n');
  return root;
}

test('remoteChecks hands the runner checks to check.sh --runner with the hosts and the deploy\'s Node', () => {
  const root = withCheckSh();
  const { io, calls } = fakeIo([{ status: 0 }]);
  let env: Record<string, string> | undefined;
  const spawn = io.spawn;
  const io2 = {
    ...io,
    node: '/opt/node/bin/node',
    spawn: (cmd: string, args: string[], opts: any) => {
      env = opts.env;
      assert.equal(opts.cwd, root);
      return spawn(cmd, args, opts);
    },
  };
  assert.equal(steps.remoteChecks(['tinker-desk', 'cloud'], root, { cwd: '/elsewhere', env: { PATH: '/bin' } }, io2), true);
  assert.deepEqual(calls, [{ cmd: path.join(root, 'check.sh'), args: ['--runner'] }]);
  assert.equal(env?.THREADBEAR_CHECK_HOST, 'tinker-desk,cloud');
  assert.equal(env?.THREADBEAR_NODE, '/opt/node/bin/node');
  assert.equal(env?.PATH, '/bin');
});

test('remoteChecks runs a failed check once more, and a second failure fails the update', () => {
  const fail = { status: 1, output: 'Checking on tinker-desk\nFAIL  runner unit tests   80s  /tmp/x.log\n' };
  const flaky = fakeIo([fail, { status: 0 }]);
  assert.equal(steps.remoteChecks(['tinker-desk'], withCheckSh(), { env: {} }, flaky.io), true);
  assert.equal(flaky.calls.length, 2);
  const broken = fakeIo([fail, fail]);
  assert.throws(() => steps.remoteChecks(['tinker-desk'], withCheckSh(), { env: {} }, broken.io), (e: unknown) => e instanceof steps.StepFailed && !(e instanceof steps.ChecksUnavailable));
  // A check that failed before any step said FAIL (check.sh itself broken) is still a failure.
  const early = fakeIo([{ status: 2, output: 'check.sh: syntax error' }, { status: 2 }]);
  assert.throws(() => steps.remoteChecks(['tinker-desk'], withCheckSh(), { env: {} }, early.io), (e: unknown) => e instanceof steps.StepFailed && !(e instanceof steps.ChecksUnavailable));
});

test('remoteChecks does not rerun a failure that ran on this Mac', () => {
  const { io, calls } = fakeIo([{ status: 1, output: 'None of tinker-desk can run this check now.\nSo the checks run on this Mac instead, at low priority.\nFAIL  runner unit tests\n' }]);
  assert.throws(() => steps.remoteChecks(['tinker-desk'], withCheckSh(), { env: {} }, io), steps.StepFailed);
  assert.equal(calls.length, 1);
});

test('remoteChecks takes the check hosts\' busy exit as their trouble, on either try', () => {
  let env: Record<string, string> | undefined;
  const first = fakeIo([{ status: steps.UNAVAILABLE_EXIT, output: 'No check host has room for this check\n' }]);
  const spawn = first.io.spawn;
  const io = { ...first.io, spawn: (cmd: string, args: string[], opts: any) => ((env = opts.env), spawn(cmd, args, opts)) };
  assert.throws(() => steps.remoteChecks(['tinker-desk'], withCheckSh(), { env: {} }, io), steps.ChecksUnavailable);
  assert.equal(first.calls.length, 1);
  assert.equal(env?.THREADBEAR_CHECK_BUSY_EXIT, String(steps.UNAVAILABLE_EXIT));
  const second = fakeIo([{ status: 1, output: 'FAIL  runner unit tests\n' }, { status: steps.UNAVAILABLE_EXIT }]);
  assert.throws(() => steps.remoteChecks(['tinker-desk'], withCheckSh(), { env: {} }, second.io), steps.ChecksUnavailable);
  assert.equal(second.calls.length, 2);
});

test('remoteChecks leaves the checks here with no host, or no check.sh', () => {
  const { io, calls } = fakeIo([]);
  assert.equal(steps.remoteChecks([], tmpDir(), { env: {} }, io), false);
  assert.equal(steps.remoteChecks(['tinker-desk'], tmpDir(), { env: {} }, io), false);
  assert.deepEqual(calls, []);
});

// A deploy run by hand moves over to the Node the installed runner runs on
// (deploy.mjs, Codex's review, 9 Nov 2026), read from what the LaunchAgent
// or systemd unit starts.
test('nodeBeforeLaunch: the Node a LaunchAgent or unit starts launch.mjs with', () => {
  const launch = '/Users/m/Library/Application Support/Threadbear/runner/launch.mjs';
  const node = '/Users/m/.local/node-24/bin/node';
  // A Mac: the Runner app, then the Node, then launch.mjs.
  assert.equal(steps.nodeBeforeLaunch(['/Users/m/Applications/Threadbear Runner.app/Contents/MacOS/Threadbear Runner', node, launch], launch), node);
  // Linux: the Node, then launch.mjs.
  assert.equal(steps.nodeBeforeLaunch(['/usr/bin/node', launch], launch), '/usr/bin/node');
  // Nothing before it, a relative name, or not launch.mjs at all (a checkout's main.ts).
  assert.equal(steps.nodeBeforeLaunch([launch], launch), undefined);
  assert.equal(steps.nodeBeforeLaunch(['node', launch], launch), undefined);
  assert.equal(steps.nodeBeforeLaunch([node, '/Users/m/dev/threadbear/runner/src/main.ts'], launch), undefined);
  assert.equal(steps.nodeBeforeLaunch([], launch), undefined);
  // Compared as the caller says (deploy.mjs resolves links).
  assert.equal(steps.nodeBeforeLaunch([node, '/private/x/launch.mjs'], '/x/launch.mjs', (a: string, b: string) => a.replace('/private', '') === b), node);
});
