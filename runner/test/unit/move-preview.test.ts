// The Mac's Move sheet asks before anything starts (threads.movePreview):
// the thread's runner describes it (`moves.describe`) and the target plans
// it (`moves.preview`), each through a relay `call`. These are the hops: the
// backend's allow-list, the description's shape check, and the target's plan.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { load } from '../helpers/convex-memory.ts';
import { FakeAccounts, makeHarness, tmpDir } from '../helpers/fakes.ts';
import { moveInfoFrom } from '../../src/moves.ts';

const { commandArgs } = load('relay') as { commandArgs: (kind: string, a: unknown) => Record<string, unknown> };

test("the backend lets the Move sheet's two calls through, and only the description's own fields", () => {
  assert.deepEqual(commandArgs('call', { method: 'moves.describe', params: { anything: 'else' } }), { method: 'moves.describe', params: {} });
  const out = commandArgs('call', {
    method: 'moves.preview',
    params: { info: { title: 't', cwd: '/c', home: '/h', provider: 'claude', accountId: 'a', permissionMode: 'full', worktree: true, schedules: 2, secret: 'no', email: 5 }, cwd: '/there', extra: 1 },
  });
  assert.deepEqual(out, { method: 'moves.preview', params: { info: { title: '', cwd: '/c', home: '/h', provider: 'claude', accountId: 'a', permissionMode: 'full', worktree: true, schedules: 2 }, cwd: '/there' } });
  assert.throws(() => commandArgs('call', { method: 'threads.events', params: {} }), /can be asked of another runner/);
});

test("a description that came over the relay keeps only what it should, or none if it's short", () => {
  assert.equal(moveInfoFrom({ title: 't', cwd: '/c' }), undefined);
  assert.equal(moveInfoFrom('nope'), undefined);
  const info = moveInfoFrom({ title: 't', cwd: '/c', home: '/h', provider: 'claude', accountId: 'a', permissionMode: 'full', origin: 'git@x:y.git', worktree: true, schedules: 'two', junk: 1 });
  assert.deepEqual(info, { title: 't', cwd: '/c', home: '/h', provider: 'claude', accountId: 'a', permissionMode: 'full', worktree: true, schedules: 0, origin: 'git@x:y.git' });
});

test("the target's preview gives its guess and its logins, and says what would stop the move before it starts", async () => {
  const t = makeHarness(tmpDir('tb-preview-home-'), {}, {
    accounts: new FakeAccounts([
      { id: 'claude-1', provider: 'claude', title: 'Claude', configDir: tmpDir(), email: 'mike@example.com' },
      { id: 'claude-2', provider: 'claude', title: 'Claude 2', configDir: tmpDir() },
      { id: 'codex-1', provider: 'codex', title: 'Codex', configDir: tmpDir() },
    ]),
  });
  const base = { title: 'x', home: '/Users/mike', accountId: 'a', permissionMode: 'full', schedules: 0, provider: 'claude' };

  // A folder that's there: its guess, the same email's login, and only Claude logins to pick.
  const folder = tmpDir('tb-preview-folder-');
  const ok = await t.runner.previewMove({ info: { ...base, cwd: folder, email: 'Mike@example.com', worktree: false }, self: 'T1', cwd: folder });
  assert.deepEqual(ok.problems, []);
  assert.equal(ok.cwd, folder);
  assert.equal(ok.accountId, 'claude-1');
  assert.deepEqual(ok.accounts.map((a) => a.id), ['claude-1', 'claude-2']);

  // Nowhere to go: the problem, and still the logins.
  const none = await t.runner.previewMove({ info: { ...base, cwd: '/Users/mike/nowhere-at-all', worktree: false }, self: 'T1' });
  assert.equal(none.problems.length, 1);
  assert.match(none.problems[0]!, /There's no folder/);
  assert.equal(none.accounts.length, 2);

  // A worktree thread into a copy of its repo with no worktree patch: said now,
  // not after the thread has frozen and packed.
  const repo = tmpDir('tb-preview-repo-');
  execFileSync('git', ['-C', repo, 'init', '-q']);
  execFileSync('git', ['-C', repo, 'remote', 'add', 'origin', 'https://github.com/mikecann/foo']);
  const wt = await t.runner.previewMove({ info: { ...base, cwd: '/Users/mike/dev/foo', origin: 'git@github.com:mikecann/foo.git', repoTop: '/Users/mike/dev/foo', subdir: '', worktree: true }, self: 'T1', cwd: repo });
  assert.equal(wt.worktree, true);
  assert.equal(wt.problems.length, 1);
  assert.match(wt.problems[0]!, /no worktree patch/);
});
