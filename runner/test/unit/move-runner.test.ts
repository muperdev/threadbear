// A thread moving between two real Runners (runner/moves.ts), in temp
// homes with fake CLIs: packed on one, planned, placed and adopted on the
// other, its session carried on there, and the old copy put away.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { FakeAccounts, makeHarness, tick, tmpDir } from '../helpers/fakes.ts';
import { projectDirName } from '../../src/transcripts.ts';
import { realpath } from '../../src/worktrees.ts';
import type { MoveRow } from '../../src/moves.ts';
import type { Account } from '../../src/protocol.ts';

function machine(accounts: Account[]) {
  return makeHarness(tmpDir('tb-move-home-'), {}, { accounts: new FakeAccounts(accounts) });
}

function row(threadId: string, extra: Partial<MoveRow> = {}): MoveRow {
  return { moveId: 'ab12cd34ef56', threadId, from: 'S1', to: 'T1', state: 'unpacking', createdAt: 1, updatedAt: 1, ...extra };
}

test('a thread moves whole: its log, its queue and its Claude session carry on in the other runner', async () => {
  const work = tmpDir('tb-move-work-');
  const sDir = tmpDir('tb-move-claude-s-');
  const tDir = tmpDir('tb-move-claude-t-');
  const s = machine([{ id: 'claude-main', provider: 'claude', title: 'S login', email: 'mike@example.com', configDir: sDir }]);
  const t = machine([
    { id: 'claude-9', provider: 'claude', title: 'other', email: 'other@example.com', configDir: tmpDir() },
    { id: 'claude-1', provider: 'claude', title: 'T login', email: 'Mike@example.com', configDir: tDir },
  ]);
  const id = s.runner.create({ text: 'Fix the tests', accountId: 'claude-main', cwd: work }).thread.id;
  const cli = s.adapter.last;
  cli.init();
  cli.text('m1:0', 'On it.');
  cli.result('completed');
  await tick();
  const rec = s.runner.threads.get(id)!.rec;
  assert.ok(rec.sessionUsed && rec.nativeSessionId);
  const sid = rec.nativeSessionId!;
  // The CLI's own transcript, where Claude keeps it on S.
  const sProject = path.join(sDir, 'projects', projectDirName(realpath(work)));
  fs.mkdirSync(path.join(sProject, sid, 'subagents'), { recursive: true });
  fs.writeFileSync(path.join(sProject, `${sid}.jsonl`), '{"type":"user"}\n');
  fs.writeFileSync(path.join(sProject, sid, 'subagents', 'agent-1.jsonl'), '{}\n');

  // Frozen for the move: a message waits rather than start a turn.
  assert.equal(s.runner.moveIdle(id), true);
  s.runner.setMoving(id, { moveId: 'ab12cd34ef56', to: 'T1', toName: 'cloud', state: 'packing' });
  assert.equal(s.runner.get(id).moving?.toName, 'cloud');
  const before = s.adapter.sessions?.length;
  s.runner.send(id, 'and the docs too');
  await tick();
  assert.equal(s.adapter.sessions?.length, before, 'no turn started while it moves');
  assert.equal(s.runner.threads.get(id)!.rec.queue.length, 1);

  const info = (await s.runner.describeForMove(id, 0))!;
  assert.equal(info.email, 'mike@example.com');
  assert.equal(info.sessionId, sid);
  const r = row(id, { info });
  const plan = await t.runner.planMove(r);
  assert.ok(!('problem' in plan), JSON.stringify(plan));
  if ('problem' in plan) return;
  assert.equal(plan.accountId, 'claude-1', 'the same email, whatever its case');
  assert.equal(plan.cwd, work);

  const packed = await s.runner.packForMove(id, r);
  const paths = packed.entries.map((e) => e.path);
  assert.ok(paths.includes('thread/thread.json') && paths.includes('thread/events.ndjson'));
  assert.ok(paths.includes(`transcript/${sid}.jsonl`) && paths.includes(`transcript/${sid}/subagents/agent-1.jsonl`));
  r.plan = plan;
  await t.runner.placeMove(r, packed.entries);
  assert.equal(t.runner.threads.has(id), false, 'not one of its threads before the switch');

  const sLast = s.store.log(id).lastSeq;
  const adopted = await t.runner.adoptMoved(r);
  assert.equal(adopted.adopted, true);
  await tick();
  const moved = t.runner.threads.get(id)!.rec;
  assert.equal(moved.accountId, 'claude-1');
  assert.equal(moved.cwd, work);
  // Its whole log came, with one line saying where from.
  const notes = t.store.log(id).read(1, t.store.log(id).lastSeq).filter((e: any) => e.type === 'system.note').map((e: any) => e.text);
  assert.ok(notes.some((n: string) => n.startsWith('Moved from S1 to T1, in ')), notes.join(' | '));
  assert.ok(t.store.log(id).lastSeq > sLast);
  // The transcript is where T's login keeps it, and the queued message
  // resumed the same session there.
  const tProject = path.join(tDir, 'projects', projectDirName(realpath(work)));
  assert.equal(fs.readFileSync(path.join(tProject, `${sid}.jsonl`), 'utf8'), '{"type":"user"}\n');
  assert.ok(fs.existsSync(path.join(tProject, sid, 'subagents', 'agent-1.jsonl')));
  assert.equal(t.adapter.last.opts.resume, true);
  assert.equal(t.adapter.last.opts.sessionId, sid);
  assert.equal(t.adapter.last.opts.account.id, 'claude-1');
  // Adopting again does nothing.
  assert.deepEqual(await t.runner.adoptMoved(r), { adopted: false });

  await s.runner.putAwayMoved(id);
  assert.equal(s.runner.threads.has(id), false);
  assert.equal(fs.existsSync(path.join(s.home, 'threads', id)), false);
  assert.equal(fs.readdirSync(path.join(s.home, 'moved')).length, 1);
  assert.ok(s.notes.some((n) => n.method === 'thread.deleted' && (n.params as { threadId: string }).threadId === id));
});

test("with no transcript to carry, the next turn starts afresh with the handoff note", async () => {
  const work = tmpDir('tb-move-work-');
  const s = machine([{ id: 'claude-main', provider: 'claude', title: 'S', configDir: tmpDir() }]);
  const t = machine([{ id: 'claude-1', provider: 'claude', title: 'T', configDir: tmpDir() }]);
  const id = s.runner.create({ text: 'hello', accountId: 'claude-main', cwd: work }).thread.id;
  s.adapter.last.init();
  s.adapter.last.result('completed');
  await tick();
  const r = row(id, { info: (await s.runner.describeForMove(id, 0))! });
  const plan = await t.runner.planMove(r);
  assert.ok(!('problem' in plan));
  r.plan = plan as Exclude<typeof plan, { problem: string }>;
  await t.runner.placeMove(r, (await s.runner.packForMove(id, r)).entries);
  await t.runner.adoptMoved(r);
  t.runner.send(id, 'carry on');
  await tick();
  assert.equal(t.adapter.last.opts.resume, false);
  assert.notEqual(t.adapter.last.opts.sessionId, s.adapter.last.opts.sessionId);
  assert.match(t.adapter.last.sent.at(-1)?.text ?? '', /hello/);
});

test('the plan says why a thread has nowhere to go, and finds its repo by origin', async () => {
  const t = machine([{ id: 'claude-1', provider: 'claude', title: 'T', configDir: tmpDir() }]);
  const base = { title: 'x', home: '/Users/mike', accountId: 'a', permissionMode: 'full', worktree: false, schedules: 0 };
  const plan = (info: Record<string, unknown>, extra: Partial<MoveRow> = {}) => t.runner.planMove(row('t', { info: { ...base, ...info } as never, ...extra }));
  assert.deepEqual(await plan({ cwd: '/Users/mike/nowhere-at-all', provider: 'claude' }), { problem: "There's no folder " + path.join(process.env.HOME!, 'nowhere-at-all') + ' on T1. Pick one to move it into.' });
  assert.deepEqual(await plan({ cwd: '/x', provider: 'codex' }), { problem: 'T1 has no Codex login.' });
  assert.deepEqual(await plan({ cwd: '/x', provider: 'claude' }, { accountId: 'nope' }), { problem: 'T1 has no login nope.' });

  // A repo T has a thread in, under a different path from S's.
  const repo = tmpDir('tb-move-repo-');
  execFileSync('git', ['-C', repo, 'init', '-q']);
  execFileSync('git', ['-C', repo, 'remote', 'add', 'origin', 'https://github.com/mikecann/foo']);
  fs.mkdirSync(path.join(repo, 'app'));
  t.runner.create({ text: 'x', accountId: 'claude-1', cwd: repo });
  const found = await plan({ cwd: '/Users/mike/dev/foo/app', provider: 'claude', origin: 'git@github.com:mikecann/foo.git', repoTop: '/Users/mike/dev/foo', subdir: 'app' });
  assert.ok(!('problem' in found));
  assert.equal(!('problem' in found) && found.cwd, path.join(realpath(repo), 'app'));
  // Nowhere it knows, but S's path under this home has room: cloned there.
  const home = process.env.HOME!;
  const parent = tmpDir('tb-move-parent-');
  const cloned = await plan({ cwd: `/Users/mike/x/new/app`, provider: 'claude', origin: 'git@github.com:mikecann/other.git', repoTop: path.join(parent, 'new'), subdir: 'app', home: '/nowhere' });
  assert.ok(!('problem' in cloned) && cloned.clone?.to === path.join(parent, 'new'), JSON.stringify(cloned));
  void home;
});

test("a place that can't finish undoes what it made", async () => {
  const work = tmpDir('tb-move-work-');
  const s = machine([{ id: 'claude-main', provider: 'claude', title: 'S', configDir: tmpDir() }]);
  const t = machine([{ id: 'claude-1', provider: 'claude', title: 'T', configDir: tmpDir() }]);
  const id = s.runner.create({ text: 'hello', accountId: 'claude-main', cwd: work }).thread.id;
  s.adapter.last.init();
  s.adapter.last.result('completed');
  await tick();
  const r = row(id, { plan: { cwd: work, accountId: 'claude-1', worktree: { repo: work, subdir: '' }, leftBehind: [] } });
  await assert.rejects(t.runner.placeMove(r, (await s.runner.packForMove(id, r)).entries), /has no worktree patch/);
  assert.equal(fs.existsSync(path.join(t.home, 'moves', r.moveId)), false);
  assert.equal(t.runner.threads.has(id), false);
});

test('a preview says what the thread would leave behind on this machine', async () => {
  const work = tmpDir('tb-move-work-');
  const git = (...args: string[]) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.com', ...args], { cwd: work, stdio: 'ignore' });
  git('init', '-q');
  fs.writeFileSync(path.join(work, 'a.txt'), 'one');
  git('add', '.');
  git('commit', '-qm', 'one');
  const s = machine([{ id: 'claude-main', provider: 'claude', title: 'S login', configDir: tmpDir() }]);
  const id = s.runner.create({ text: 'Fix it', accountId: 'claude-main', cwd: work }).thread.id;
  assert.deepEqual(await s.runner.leftBehindByMove(id), [], 'nothing to say, so Move to goes straight away');
  fs.writeFileSync(path.join(work, 'a.txt'), 'two');
  const said = await s.runner.leftBehindByMove(id);
  assert.equal(said.length, 1);
  assert.match(said[0], /uncommitted changes in .*share that checkout: a\.txt/);
});
