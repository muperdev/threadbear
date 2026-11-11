// Extra CLI updates (src/updates/extra.ts): the commands in runner.json's
// cliUpdates.extra run once a day each, one at a time, and a failure or a
// restart doesn't make them run again early.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { FakeClock } from '../../src/clock.ts';
import { ExtraUpdates, DAY_MS, FIRST_RUN_MS, extraUpdatesFrom } from '../../src/updates/extra.ts';
import type { Exec } from '../../src/updates/exec.ts';

const log = { info() {}, warn() {}, error() {}, debug() {} } as never;

function setup(results: Record<string, number> = {}) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-extra-'));
  const clock = new FakeClock();
  const calls: { command: string; args: string[]; env?: NodeJS.ProcessEnv }[] = [];
  let running = 0;
  let most = 0;
  const exec: Exec = async (command, args, opts) => {
    calls.push({ command, args, env: opts?.env });
    running++;
    most = Math.max(most, running);
    await new Promise((r) => setImmediate(r));
    running--;
    return { code: results[command] ?? 0, stdout: '', stderr: results[command] ? 'boom' : '' };
  };
  const extra = [
    { name: 'grok', command: ['/bin/grok', 'update'] },
    { name: 'muse', command: ['/bin/muse'], env: { MUSE_LAUNCHER_INSTALL: '1' } },
  ];
  const make = () => new ExtraUpdates({ home, log, clock, extra, exec, env: { PATH: '/usr/bin' } });
  return { home, clock, calls, make, most: () => most };
}

const settle = () => new Promise((r) => setTimeout(r, 20));

test('runs each command a minute after start, one at a time, with its env', async () => {
  const s = setup();
  const u = s.make();
  u.start();
  assert.equal(s.calls.length, 0);
  s.clock.advance(FIRST_RUN_MS);
  await u.settled();
  assert.deepEqual(s.calls.map((c) => [c.command, ...c.args]), [['/bin/grok', 'update'], ['/bin/muse']]);
  assert.equal(s.most(), 1, 'one at a time');
  assert.equal(s.calls[1].env?.MUSE_LAUNCHER_INSTALL, '1');
  assert.equal(s.calls[1].env?.PATH, '/usr/bin', "keeps the runner's own env");
  u.stop();
});

test('runs again a day later, not before, and a restart remembers the last run', async () => {
  const s = setup();
  let u = s.make();
  u.start();
  s.clock.advance(FIRST_RUN_MS);
  await u.settled();
  u.stop();
  assert.equal(s.calls.length, 2);
  // A restart an hour later: nothing runs until the day is up.
  s.clock.advance(60 * 60_000);
  u = s.make();
  u.start();
  s.clock.advance(FIRST_RUN_MS);
  await u.settled();
  assert.equal(s.calls.length, 2);
  s.clock.advance(DAY_MS);
  await settle();
  await u.settled();
  assert.equal(s.calls.length, 4);
  u.stop();
});

test('a failing command is logged and waits a day like the rest', async () => {
  const s = setup({ '/bin/grok': 1 });
  const u = s.make();
  u.start();
  s.clock.advance(FIRST_RUN_MS);
  await u.settled();
  assert.equal(s.calls.length, 2, 'the next one still runs');
  const state = JSON.parse(fs.readFileSync(path.join(s.home, 'extra-updates.json'), 'utf8'));
  assert.equal(state.grok.ok, false);
  assert.equal(state.muse.ok, true);
  u.stop();
});

test('runner.json parsing keeps only well-formed entries, and off turns them all off', () => {
  const raw = [
    { name: 'grok', command: ['~/x/grok', 'update'] },
    { name: 'bad', command: 'grok update' },
    { name: '', command: ['x'] },
    { name: 'muse', command: ['muse'], env: { A: '1', B: 2 } },
    'nope',
  ];
  const got = extraUpdatesFrom(raw, undefined, '/Users/me');
  assert.deepEqual(got, [
    { name: 'grok', command: ['/Users/me/x/grok', 'update'] },
    { name: 'muse', command: ['muse'], env: { A: '1' } },
  ]);
  assert.deepEqual(extraUpdatesFrom(raw, 'off', '/Users/me'), []);
  assert.deepEqual(extraUpdatesFrom(undefined, undefined, '/Users/me'), []);
});
