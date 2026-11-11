// runner.json's defaults.permissionMode: the mode a thread that names none
// starts on (the phone, the website, tb new, schedules without one), and
// what hello tells the app's composer to start on. Unset is Full access, so
// an install from before it changes nothing; setup writes "auto" into a new
// install's runner.json (scripts/setup.mjs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { makeHarness, testConfig, tmpDir } from '../helpers/fakes.ts';
import { loadConfig, writeRunnerJson } from '../../src/config.ts';
import { startOwnThread } from '../../src/own-threads.ts';

test('config: defaults.permissionMode is read from runner.json; unset, plan or nonsense is no default', () => {
  const home = tmpDir();
  assert.equal(loadConfig({ THREADBEAR_HOME: home }).defaults.permissionMode, undefined);
  for (const mode of ['full', 'ask', 'auto'] as const) {
    writeRunnerJson(home, { defaults: { permissionMode: mode } });
    assert.equal(loadConfig({ THREADBEAR_HOME: home }).defaults.permissionMode, mode);
  }
  // Plan first waits for Mike to approve a plan, so a thread nobody's
  // watching (a schedule, one from the phone) would just sit there.
  for (const bad of ['plan', 'yolo', 3]) {
    writeRunnerJson(home, { defaults: { permissionMode: bad as never } });
    assert.equal(loadConfig({ THREADBEAR_HOME: home }).defaults.permissionMode, undefined, String(bad));
  }
});

test('a thread that names no mode starts on the default; a named one wins', () => {
  const h = makeHarness(undefined, { defaults: { cwd: '', permissionMode: 'auto' } });
  h.cfg.defaults.cwd = path.join(h.home, 'work');
  assert.equal(h.runner.create({ text: 'hi' }).thread.permissionMode, 'auto');
  assert.equal(h.runner.create({ text: 'hi', origin: 'schedule', scheduleId: 'sc1' }).thread.permissionMode, 'auto', 'a schedule with no mode too');
  assert.equal(h.runner.create({ text: 'hi', permissionMode: 'full' }).thread.permissionMode, 'full');
  assert.equal(h.runner.create({ text: 'hi', permissionMode: 'ask' }).thread.permissionMode, 'ask');
  assert.equal(h.runner.hello().defaults.permissionMode, 'auto');
});

test('with no default, a thread that names no mode gets Full access, and hello says nothing', () => {
  const h = makeHarness();
  assert.equal(h.runner.create({ text: 'hi' }).thread.permissionMode, 'full');
  assert.equal(h.runner.hello().defaults.permissionMode, undefined);
  assert.ok(!('permissionMode' in h.runner.hello().defaults));
});

// Mike, 9 Nov 2026 (the security review's finding 8): the threads the
// runner starts by itself are handed words from upstream or a build log and
// run code nobody has read, so they get the install's own mode, not Full
// access whatever it is.
test("the runner's own threads start on the install's default mode", () => {
  for (const mode of ['auto', 'ask', 'full'] as const) {
    const home = tmpDir();
    const h = makeHarness(home, { ...testConfig(home), defaults: { cwd: path.join(home, 'work'), permissionMode: mode } });
    const notice = h.runner.noticeOnMac(undefined, 'A dialog', 'A dialog is waiting');
    assert.equal(h.runner.get(notice).permissionMode, mode, `the note about this Mac, on ${mode}`);
    // Bringing in Mike's release and fixing a failed update (main.ts) start
    // through this.
    const own = startOwnThread(h.runner, { text: 'Bring it in', title: "Bring in Mike's release", why: "Mike's new releases" }, path.join(home, 'work'));
    assert.equal(h.runner.get(own).permissionMode, mode, `a bring-in or update-fixer thread, on ${mode}`);
    assert.deepEqual(h.runner.get(own).startedBy, { kind: 'runner', label: 'Threadbear', why: "Mike's new releases" });
  }
  // An install from before 9 Nov has no default: Full access, as before.
  const h = makeHarness();
  assert.equal(h.runner.get(startOwnThread(h.runner, { text: 'x', title: 'x', why: 'x' }, undefined)).permissionMode, 'full');
  assert.equal(h.runner.get(h.runner.noticeOnMac(undefined, 'A dialog', 'x')).permissionMode, 'full');
});

test("main.ts starts the bring-in and update-fixer threads through startOwnThread, naming no mode", () => {
  // main() wires live services, so it isn't run here: its source is read
  // instead, so a hard-coded mode coming back fails.
  const src = fs.readFileSync(path.join(import.meta.dirname, '../../src/main.ts'), 'utf8');
  assert.match(src, /startOwnThread\(runner, r, updateRelease\?\.checkout\)/);
  assert.match(src, /startOwnThread\(runner, r, thisRelease\?\.checkout\)/);
  assert.doesNotMatch(src, /permissionMode: 'full'/);
});
