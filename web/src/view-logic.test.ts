// node --test web/src/view-logic.test.ts
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { lastWorkedOn, recapDue, recapParts, bytesWords, statsFresh, isOnline, matchesSchedule, runnersShown, runnerState, scheduleSections, sinceWords, tightest, untilWords } from './view-logic.ts';

const NOW = Date.parse('2026-11-05T10:00:00Z');
const ago = (ms: number) => new Date(NOW - ms).toISOString();
const MIN = 60_000;
const run = (name: string, ms: number, draining = false) => ({ runnerId: name, name, lastSeenAt: ago(ms), draining });

test('online runners come first, and one gone for a week is left out', () => {
  const rows = [run('zed', MIN), run('asleep', 10 * MIN), run('bruce', MIN), run('old', 8 * 24 * 60 * MIN)];
  assert.deepEqual(runnersShown(rows, NOW).map((r) => r.name), ['bruce', 'zed', 'asleep']);
  assert.ok(isOnline(run('a', 2 * MIN), NOW));
  assert.ok(!isOnline(run('a', 4 * MIN), NOW));
});

test('a runner says how it is and when it was last heard from', () => {
  assert.equal(runnerState(run('a', 12_000), NOW), 'Online, heard from 12s ago');
  assert.equal(runnerState(run('a', 12_000, true), NOW), 'Updating, heard from 12s ago');
  assert.equal(runnerState(run('a', 3 * 3600_000), NOW), 'Offline, last heard from 3h ago');
});

test('times read short', () => {
  assert.equal(sinceWords(NOW - 90_000, NOW), '1m');
  assert.equal(sinceWords(NOW + 5000, NOW), '0s');
  assert.equal(untilWords(ago(-20 * MIN), NOW), 'in 20m');
  assert.equal(untilWords(ago(-49 * 3600_000), NOW), 'in 2d');
  assert.equal(untilWords(ago(MIN), NOW), 'due');
  assert.equal(untilWords(undefined, NOW), '');
});

test('schedules go under the Mac\'s headings, in its orders', () => {
  const s = (id: string, o = {}) => ({ id, title: id, whenText: 'x', ...o });
  const out = scheduleSections([
    s('later', { nextRunAt: '2026-11-06T00:00:00Z' }),
    s('soon', { nextRunAt: '2026-11-05T11:00:00Z' }),
    s('none'),
    s('resting', { paused: true }),
    s('awake', { paused: true }),
    s('ran', { finishedAt: '2026-11-01T00:00:00Z' }),
    s('ranLater', { finishedAt: '2026-11-02T00:00:00Z', paused: true }),
  ]);
  assert.deepEqual(out.active.map((x) => x.id), ['soon', 'later', 'none']);
  assert.deepEqual(out.paused.map((x) => x.id), ['awake', 'resting']);
  assert.deepEqual(out.done.map((x) => x.id), ['ranLater', 'ran']);
});

test('schedule search looks in the title, the time and the message', () => {
  const s = { title: 'Morning brief', whenText: 'Every weekday at 7:00', text: 'Read TASK.md' };
  assert.ok(matchesSchedule(s, ' MORNING '));
  assert.ok(matchesSchedule(s, 'weekday'));
  assert.ok(matchesSchedule(s, 'task.md'));
  assert.ok(matchesSchedule(s, ''));
  assert.ok(!matchesSchedule(s, 'evening'));
});

test('a login\'s room is its tightest window', () => {
  assert.equal(tightest([{ title: '5h', remainingPercent: 80 }, { title: 'week', remainingPercent: 30 }])?.title, 'week');
  assert.equal(tightest([]), undefined);
  assert.equal(tightest(undefined), undefined);
});

test('sizes read short', () => {
  const GB = 1024 ** 3;
  assert.equal(bytesWords(6.94 * GB), '6.9 GB');
  assert.equal(bytesWords(212.4 * GB), '212 GB');
  assert.equal(bytesWords(1536 * GB), '1.5 TB');
});

test('figures are fresh for a minute', () => {
  assert.ok(statsFresh({ at: ago(30_000) }, NOW));
  assert.ok(!statsFresh({ at: ago(90_000) }, NOW));
});

test('the recap box shows on a quiet idle thread, until put away for that quiet spell', () => {
  const t = { recap: 'For this.\n- Done', status: 'idle', lastActivityAt: ago(12 * 60 * MIN) };
  assert.equal(recapDue(t, null, NOW), true);
  assert.equal(recapDue({ ...t, lastActivityAt: ago(11 * 60 * MIN) }, null, NOW), false);
  assert.equal(recapDue({ ...t, status: 'running' }, null, NOW), false);
  assert.equal(recapDue({ ...t, archivedAt: ago(MIN) }, null, NOW), false);
  assert.equal(recapDue(t, t.lastActivityAt, NOW), false);
  assert.equal(recapDue(t, ago(99 * 60 * MIN), NOW), true);
  assert.deepEqual(recapParts('For this.\n- One\n- Two'), { purpose: 'For this.', points: ['One', 'Two'] });
  assert.deepEqual(recapParts('- One'), { points: ['One'] });
  assert.equal(lastWorkedOn(ago(14 * 60 * MIN), NOW), 'Last worked on 14 hours ago');
  assert.equal(lastWorkedOn(ago(72 * 60 * MIN), NOW), 'Last worked on 3 days ago');
});
