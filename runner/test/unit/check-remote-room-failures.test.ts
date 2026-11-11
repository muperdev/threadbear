// scripts/check-remote.sh against fake hosts (helpers/check-remote-harness.ts
// says how, and why it's several files): failures on a host short of disk, and hosts that can't say how much it has.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { tmpDir } from '../helpers/fakes.ts';
import { GiB, macDf, setup } from '../helpers/check-remote-harness.ts';

test("a df answer that can't be used doesn't stop the check, and is never run or trusted as room", () => {
  const marker = path.join(tmpDir('check-remote-marker-'), 'ran');
  const answers = ['', 'not a disk\n', macDf('$(touch ' + marker + ')'), macDf('12x'), macDf('1'.repeat(40)), macDf('-5'), macDf('٣٣')];
  for (const df of answers) {
    const s = setup({ h1: { state: 'free', df } });
    const r = s.run('h1', 3600);
    assert.equal(r.status, 0, `${JSON.stringify(df)}: ${r.stdout}${r.stderr}`);
    assert.match(r.stderr, /Couldn't read how much disk h1 has free; trying it anyway\./, JSON.stringify(df));
    assert.deepEqual(r.copies, ['h1'], JSON.stringify(df));
  }
  assert.ok(!fs.existsSync(marker));
  // A leading zero isn't octal: 0900 would be an error there.
  const s = setup({ h1: { state: 'free', df: macDf('0900') } });
  const r = s.run('h1', 3600);
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stderr, /h1 has 0\.0 GiB free/);
});

test('a check that fails on a host left short of room says so last', () => {
  const s = setup({ h1: { state: 'free', dfAfter: macDf(0.5 * GiB) } });
  const r = s.run('h1', 3600, '', { RUN_FAILS: '1' });
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.equal(r.stderr.trim().split('\n').at(-1), 'h1 had only 0.5 GiB free when this failed, and this check wants 8 GiB, which may be why. Free some space there and run it again.');
});

test('a check that fails with plenty of room, or that passes on a low host, says nothing about it', () => {
  const failed = setup({ h1: { state: 'free' } });
  const f = failed.run('h1', 3600, '', { RUN_FAILS: '1' });
  assert.equal(f.status, 1, f.stdout + f.stderr);
  assert.doesNotMatch(f.stderr, /GiB/);

  const passed = setup({ h1: { state: 'free', dfAfter: macDf(GiB) } });
  const p = passed.run('h1', 3600);
  assert.equal(p.status, 0, p.stdout + p.stderr);
  assert.doesNotMatch(p.stderr, /GiB/);
});

test('a copy that fails on a host short of room says so, and ends as no room rather than 255', () => {
  const s = setup({ h1: { state: 'free', dfAfter: macDf(0.2 * GiB) } });
  const r = s.run('h1', 3600, '', { RSYNC_FAILS_ON: 'h1' });
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stderr, /Couldn't copy this checkout to h1 \(it has 0\.2 GiB free, and this check wants 8 GiB\); trying the others\./);
  assert.match(r.stderr, /No check host has room for this check: h1 has 0\.2 GiB free, and it wants 8 GiB\./);
  assert.deepEqual(r.left('h1'), []);
});

// The runner's self-update tells the hosts' trouble from a failed check by
// this exit (deploy-steps.mjs, remoteChecks); a run that went over its time
// while checking stays 1, as a hung test is the check's own failure.
test('with THREADBEAR_CHECK_BUSY_EXIT, no room exits with it, and a run over time while checking does not', () => {
  const low = setup({ h1: { state: 'free', dfAfter: macDf(0.2 * GiB) } });
  const r = low.run('h1', 3600, '', { RSYNC_FAILS_ON: 'h1', THREADBEAR_CHECK_BUSY_EXIT: '69' });
  assert.equal(r.status, 69, r.stdout + r.stderr);
  assert.match(r.stderr, /No check host has room for this check/);
  // 8 s, not less: the limit counts from the script's start, and with the other
  // files running beside this one a 3 s limit failed this test (9 Nov). A run
  // stopped before its check begins reads as still waiting.
  const limits = { THREADBEAR_CHECK_PING_SECS: '1', THREADBEAR_CHECK_MAX_SECS: '8', THREADBEAR_CHECK_BUSY_EXIT: '69' };
  // Started: --on-host has moved its copy out of runs/ (the fake ssh doesn't
  // run --on-host, so the peek does the move's part), then a step hangs.
  const hung = setup({ h1: { state: 'free' } }).run('h1', 3600, 'rm -rf "$PWD"; sleep 30', limits);
  assert.equal(hung.status, 1, hung.stdout + hung.stderr);
  // Still waiting for the host's lock: its copy is where it was sent.
  const queued = setup({ h1: { state: 'free' } }).run('h1', 3600, 'sleep 30', limits);
  assert.equal(queued.status, 69, queued.stdout + queued.stderr);
});

test('a copy that fails on a host short of room still goes on to one with room', () => {
  const s = setup({ h1: { state: 'free', dfAfter: macDf(0.2 * GiB) }, h2: { state: 'free' } });
  const r = s.run('h1,h2', 3600, '', { RSYNC_FAILS_ON: 'h1' });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stdout, /ran on h2 now/);
});

test('a copy that fails with plenty of room is still just unreachable: 255', () => {
  const s = setup({ h1: { state: 'free' } });
  const r = s.run('h1', 3600, '', { RSYNC_FAILS_ON: 'h1' });
  assert.equal(r.status, 255, r.stdout + r.stderr);
  assert.match(r.stderr, /Couldn't copy this checkout to h1; trying the others\./);
  assert.match(r.stderr, /None of h1 can run this check now\./);
});

// Codex's review of the above.
test("a disk too full for the probe's own mkdir is no room, not unreachable", () => {
  const s = setup({ h1: { state: 'free', df: macDf(0.1 * GiB) } });
  fs.writeFileSync(path.join(s.fake, 'h1', 'probe-fails'), '');
  const r = s.run('h1', 3600);
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stderr, /No check host has room for this check: h1 has 0\.1 GiB free/);
  assert.deepEqual(r.copies, []);
});

test('a probe that fails with room to spare, or a host ssh cannot reach, is still unreachable: 255', () => {
  const s = setup({ h1: { state: 'free' } });
  fs.writeFileSync(path.join(s.fake, 'h1', 'probe-fails'), '');
  for (const hosts of ['h1', 'nope']) {
    const r = s.run(hosts, 3600);
    assert.equal(r.status, 255, r.stdout + r.stderr);
    assert.match(r.stderr, new RegExp(`Couldn't reach ${hosts} over ssh`));
  }
});

test('a slow copy that loses the race to a check that fills the disk moves on, not waits there', () => {
  const s = setup({ h1: { state: 'free', lose: 1, dfAfter: macDf(0.5 * GiB) }, h2: { state: 'free' } });
  const r = s.run('h1,h2', 0);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stderr, /Passing over h1: it has 0\.5 GiB free/);
  assert.match(r.stdout, /ran on h2 now/);
  assert.deepEqual(r.copies, ['h1', 'h2']);
  assert.deepEqual(r.left('h1'), []);
});

test('a failure is judged by the disk as it failed, not after its temp files went', () => {
  const s = setup({ h1: { state: 'free' } });
  const r = s.run('h1', 3600, '', { RUN_FAILS: '1', KEPT_KB: String(GiB) });
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stderr.trim().split('\n').at(-1)!, /^h1 had only 1\.0 GiB free when this failed/);

  // A write the full disk cut short leaves it empty: ask the host now instead.
  const empty = setup({ h1: { state: 'free', dfAfter: macDf(0.5 * GiB) } });
  const e = empty.run('h1', 3600, '', { RUN_FAILS: '1', KEPT_EMPTY: '1' });
  assert.equal(e.status, 1, e.stdout + e.stderr);
  assert.match(e.stderr, /h1 had only 0\.5 GiB free when this failed/);
});

// The host's side for real: a scratch copy whose check.sh fails, with temp
// files that are gone by the end, and a df that sits in the folder --on-host
// puts first on PATH (the Node's). As over ssh, nothing on PATH and no
