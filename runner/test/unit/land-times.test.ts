// tb land keeps each check's step times in a log (docs/thread-worktrees.md,
// "Where a landing's time goes"): a passing check prints nothing of its own,
// so on 9 Nov finding out where landings were spending their time took
// scraping transcripts and running timed checks by hand.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';
import { tmpDir } from '../helpers/fakes.ts';
import { land, parseCheckTimes, LAND_TIMES_KEEP, LAND_TIMES_MAX_BYTES, type LandTarget } from '../../src/worktrees.ts';

const sh = (cwd: string, cmd: string) => execSync(cmd, { cwd, encoding: 'utf8' }).trim();

function repo(): { main: string; t: LandTarget; log: string } {
  const root = tmpDir('land-times-');
  const main = path.join(root, 'proj');
  fs.mkdirSync(main);
  sh(main, 'git init -q -b main && git config user.email t@t && git config user.name t && echo a > a && git add a && git commit -qm base');
  const wt = path.join(root, 'wt');
  sh(main, `git worktree add -q -b thread/abcd1234 ${wt} main`);
  fs.writeFileSync(path.join(wt, 'b'), 'b\n');
  sh(wt, 'git add b && git commit -qm "Add b"');
  return { main, t: { path: wt, branch: 'thread/abcd1234', mainCheckout: main }, log: path.join(main, '.git', 'threadbear-land-times.log') };
}

function capture() {
  const out: string[] = [];
  const err: string[] = [];
  return { out, err, opts: { out: (l: string) => out.push(l), err: (l: string) => err.push(l) } };
}

const entries = (log: string) => fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));

test('parseCheckTimes reads check.sh lines: host, copy, steps, skips and a step that needed a second run', () => {
  const text = [
    'Checking on tinker-desk (copied in 2s; THREADBEAR_CHECK_HOST= ./check.sh --local runs here)',
    'ok    runner typecheck        1s',
    'ok    runner unit tests     304s  (passed on a second run of the files that failed:)',
    '        ✖ a test that needed it (6622.234ms)',
    'FAIL  web tests               7s  /tmp/logs/web-tests.log',
    'skip  app release build    (nothing in app/ or dock/ changed; --all runs it)',
  ].join('\n');
  assert.deepEqual(parseCheckTimes(text), {
    host: 'tinker-desk',
    copySeconds: 2,
    steps: [
      { name: 'runner typecheck', status: 'ok', seconds: 1 },
      { name: 'runner unit tests', status: 'ok', seconds: 304, secondRun: ['a test that needed it'] },
      { name: 'web tests', status: 'FAIL', seconds: 7 },
      { name: 'app release build', status: 'skip' },
    ],
  });
});

test('parseCheckTimes copes with a copy already on the host, and with a check that prints no steps', () => {
  assert.deepEqual(parseCheckTimes('Checking on cloud (its copy is already there)\n'), { host: 'cloud', steps: [] });
  assert.deepEqual(parseCheckTimes('anything else a repo prints\nok then\n'), { steps: [] });
});

test('a landing logs its check, and what it prints is unchanged', async () => {
  const { main, t, log } = repo();
  const c = capture();
  assert.equal(await land({ ...t, check: 'echo "ok    runner typecheck        3s"' }, c.opts), 0, c.err.join('\n'));
  assert.equal(c.out[0], 'Running the check: echo "ok    runner typecheck        3s"');
  assert.match(c.out[1], /^The check passed \(\d+ s\)\.$/);
  assert.equal(c.out.length, 4);
  const [e] = entries(log);
  assert.equal(e.branch, 'thread/abcd1234');
  assert.equal(e.commit, sh(main, 'git rev-parse main'));
  assert.equal(e.ok, true);
  assert.equal(typeof e.seconds, 'number');
  assert.equal(typeof e.waitedSeconds, 'number');
  assert.deepEqual(e.steps, [{ name: 'runner typecheck', status: 'ok', seconds: 3 }]);
  assert.ok(!Number.isNaN(Date.parse(e.at)));
  assert.equal(e.dryRun, undefined);
});

test('a failed check is logged too, with its exit code, and a dry run says so', async () => {
  const { main, t, log } = repo();
  const before = sh(main, 'git rev-parse main');
  assert.equal(await land({ ...t, check: 'echo "FAIL  web tests   7s  /x.log"; exit 3' }, capture().opts), 1);
  assert.equal(sh(main, 'git rev-parse main'), before);
  const [failed] = entries(log);
  assert.equal(failed.ok, false);
  assert.equal(failed.exit, 3);
  assert.deepEqual(failed.steps, [{ name: 'web tests', status: 'FAIL', seconds: 7 }]);
  assert.equal(await land({ ...t, check: 'true' }, { ...capture().opts, dryRun: true }), 0);
  assert.equal(entries(log)[1].dryRun, true);
});

test('a log that cannot be written never stops a landing', async () => {
  const { main, t, log } = repo();
  fs.mkdirSync(log);
  const c = capture();
  assert.equal(await land({ ...t, check: 'true' }, c.opts), 0, c.err.join('\n'));
  assert.equal(sh(main, 'git log -1 --format=%s main'), 'Add b');
});

test('the log is cut back to the newest entries once it gets big', async () => {
  const { t, log } = repo();
  const old = JSON.stringify({ at: '2026-01-01T00:00:00Z', pad: 'x'.repeat(200) });
  const lines = Math.ceil(LAND_TIMES_MAX_BYTES / old.length) + 10;
  fs.writeFileSync(log, `${old}\n`.repeat(lines));
  assert.equal(await land({ ...t, check: 'true' }, capture().opts), 0);
  const kept = entries(log);
  assert.equal(kept.length, LAND_TIMES_KEEP);
  assert.equal(kept.at(-1).branch, 'thread/abcd1234');
});
