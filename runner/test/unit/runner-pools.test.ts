// Pools in the runner (docs/protocol.md, "Pools"), with fake adapters and
// the fake clock: picking a pool and login on create (the named model's
// provider decides), the move before a turn, the move after a usageLimit
// failure and its one carry-on, the wait and its timer (across a restart),
// the spill to another provider, pinned threads, threads.update poolId,
// and pools.list / pools.save.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { FakeAccounts, makeHarness, tick, tmpDir, withCodex } from '../helpers/fakes.ts';
import { checkSummary } from '../helpers/protocol-check.ts';
import { readRunnerJson, writeRunnerJson } from '../../src/config.ts';
import type { Account, Pool } from '../../src/protocol.ts';
import { RpcError } from '../../src/protocol.ts';
import type { UsageReading } from '../../src/usage.ts';
import { CARRY_ON_BACK, CARRY_ON_MOVED, type UsageSink } from '../../src/runner.ts';

const MIN = 60_000;
const START = Date.parse('2026-10-26T12:00:00.000Z');

// Usage with a 5-hour window at `pct`, resetting `resetsIn` minutes after
// the fake clock's start.
function room(pct: number, resetsIn?: number, blocked = false): Account['usage'] {
  return {
    windows: [{ kind: 'five_hour', title: '5-hour', remainingPercent: pct, ...(resetsIn !== undefined ? { resetsAt: new Date(START + resetsIn * MIN).toISOString() } : {}) }],
    readAt: new Date(START).toISOString(),
    source: 'live',
    ...(blocked ? { blocked: true } : {}),
  };
}

function poolHarness(o: { accounts?: Account[]; pools?: Pool[]; copy?: boolean } = {}) {
  const fake = new FakeAccounts(o.accounts ?? withCodex());
  const records: [string, UsageReading][] = [];
  const usage: UsageSink = { record: (id, r) => void records.push([id, r]) };
  const copies: string[][] = [];
  const h = makeHarness(undefined, o.pools ? { pools: o.pools } : {}, {
    accounts: fake,
    usage,
    copyTranscript: (fc, tc, f, t, sid) => (copies.push([fc, tc, f, t, sid]), o.copy ?? true),
  });
  const setUsage = (id: string, usage: Account['usage']) => {
    fake.base = fake.base.map((a) => (a.id === id ? { ...a, usage } : a));
  };
  return { ...h, fake, records, copies, setUsage };
}

const notes = (h: { events(id: string): any[] }, id: string): string[] => h.events(id).filter((e) => e.type === 'system.note').map((e) => e.text);
const userMessages = (h: { events(id: string): any[] }, id: string) => h.events(id).filter((e) => e.type === 'user.message');
const limit = (s: { result: (...a: any[]) => void }) => s.result('failed', undefined, "You've hit your usage limit", { errorKind: 'usageLimit' });

// ------------------------------------------------------- create

test('a new thread with nothing named is a pool thread on the login with the most room', () => {
  const h = poolHarness();
  h.setUsage('claude-main', room(40));
  h.setUsage('claude-3', room(90));
  const hello = h.runner.hello();
  assert.deepEqual(hello.pools.map((p) => [p.id, p.accountIds]), [
    ['claude', ['claude-main', 'claude-3']],
    ['codex', ['codex-main']],
  ]);
  assert.equal(hello.defaults.poolId, 'claude');
  const { thread } = h.runner.create({ text: 'x' });
  assert.deepEqual(checkSummary(thread), []);
  assert.equal(thread.poolId, 'claude');
  assert.equal(thread.accountId, 'claude-3');
  // An accountId pins, and wins over a poolId.
  const pinned = h.runner.create({ text: 'y', accountId: 'claude-main', poolId: 'codex' }).thread;
  assert.equal(pinned.poolId, undefined);
  assert.equal(pinned.accountId, 'claude-main');
  assert.throws(() => h.runner.create({ text: 'z', poolId: 'nope' }), (e: RpcError) => e.code === -32602 && /No pool called nope/.test(e.message));
});

test('naming a model picks its provider: claude-opus-5-5 with no account never lands on Codex', () => {
  const h = poolHarness();
  // Codex was last used, so its pool is the default.
  writeRunnerJson(h.home, { defaults: { accountId: 'codex-main' } });
  assert.equal(h.runner.hello().defaults.poolId, 'codex');
  const opus = h.runner.create({ text: 'x', model: 'claude-opus-5-5' }).thread;
  assert.equal(opus.provider, 'claude');
  assert.equal(opus.poolId, 'claude');
  assert.equal(opus.model, 'claude-opus-5-5', 'the model is kept');
  assert.equal(h.adapter.last.opts.model, 'claude-opus-5-5');
  assert.deepEqual(notes(h, opus.id), []);
  // Nothing named: the default pool, Codex.
  const plain = h.runner.create({ text: 'y' }).thread;
  assert.equal(plain.provider, 'codex');
  assert.equal(plain.poolId, 'codex');
  // A Codex model from a Claude default goes the other way.
  writeRunnerJson(h.home, { defaults: { accountId: 'claude-3' } });
  const luna = h.runner.create({ text: 'z', model: 'gpt-6-luna' }).thread;
  assert.equal(luna.provider, 'codex');
  assert.equal(luna.model, 'gpt-6-luna');
});

test("a folder named in a pool's folders starts its threads there; a named pool is remembered", () => {
  const convex = tmpDir();
  const pools: Pool[] = [
    { id: 'claude', title: 'Claude', accountIds: ['claude-main'] },
    { id: 'work', title: 'Work', accountIds: ['claude-3'], folders: [convex] },
    { id: 'codex', title: 'Codex', accountIds: ['codex-main'] },
  ];
  const h = poolHarness({ pools });
  const inside = path.join(convex, 'app');
  fs.mkdirSync(inside);
  const t = h.runner.create({ text: 'x', cwd: inside }).thread;
  assert.equal(t.poolId, 'work');
  assert.equal(t.accountId, 'claude-3');
  assert.equal(readRunnerJson(h.home).defaults?.poolId, undefined, "a folder's pool isn't a choice");
  h.runner.create({ text: 'y', poolId: 'codex' });
  assert.equal(readRunnerJson(h.home).defaults?.poolId, 'codex');
  assert.equal(h.runner.hello().defaults.poolId, 'codex');
  assert.equal(h.runner.create({ text: 'z' }).thread.poolId, 'codex');
});

// ------------------------------------------------------- before a turn

test('before a turn: a login under the minimum moves the thread, carrying the transcript between Claude logins', async () => {
  const h = poolHarness();
  h.setUsage('claude-main', room(40));
  h.setUsage('claude-3', room(90));
  const { thread } = h.runner.create({ text: 'x' });
  const s = h.adapter.last;
  s.init();
  s.result();
  // claude-3 drops under 10%: the next message moves first.
  h.setUsage('claude-3', room(5, 130));
  h.runner.send(thread.id, 'next');
  await tick();
  const s2 = h.adapter.last;
  assert.notEqual(s2, s);
  assert.equal(s2.opts.account.id, 'claude-main');
  assert.equal(s2.opts.resume, true, 'resumes the carried transcript');
  assert.equal(s2.opts.sessionId, s.opts.sessionId);
  assert.equal(s2.sent[0].text, 'next', 'no handoff note');
  assert.deepEqual(h.copies, [['/tmp/three', '/tmp/main', h.cfg.defaults.cwd, h.cfg.defaults.cwd, s.opts.sessionId]]);
  const n = notes(h, thread.id);
  assert.match(n[0], /^Moved to main@example\.com: three@example\.com is down to 5% of its 5-hour limit, which resets /);
  assert.equal(n[1], 'Resumed the session on main@example.com, with the whole conversation.');
  assert.equal(h.runner.get(thread.id).accountId, 'claude-main');
  assert.equal(h.runner.get(thread.id).poolId, 'claude', 'still a pool thread');
});

test('before a turn: a failed copy falls back to the handoff note; a login with room stays put', async () => {
  const h = poolHarness({ copy: false });
  h.setUsage('claude-main', room(40));
  h.setUsage('claude-3', room(90));
  const { thread } = h.runner.create({ text: 'x' });
  h.adapter.last.init();
  h.adapter.last.text('m1:0', 'Done.');
  h.adapter.last.result();
  // A login with less room than another but over the minimum keeps the thread.
  h.setUsage('claude-3', room(20));
  h.runner.send(thread.id, 'a');
  await tick();
  assert.equal(h.adapter.sessions.length, 1, 'same live CLI');
  h.adapter.last.result();
  h.setUsage('claude-3', room(0, 60, true));
  h.runner.send(thread.id, 'b');
  await tick();
  const s2 = h.adapter.last;
  assert.equal(s2.opts.account.id, 'claude-main');
  assert.equal(s2.opts.resume, false);
  assert.match(s2.sent[0].text, /^\[Threadbear handoff\] This conversation moved from the three@example\.com login to main@example\.com/);
  assert.match(notes(h, thread.id)[0], /three@example\.com hit its 5-hour limit, back at /);
});

test('a pinned thread never moves: at its limit it waits for its own login and carries on there', async () => {
  const h = poolHarness();
  h.setUsage('claude-main', room(90));
  const { thread } = h.runner.create({ text: 'x', accountId: 'claude-3' });
  assert.equal(thread.poolId, undefined);
  h.adapter.last.init();
  h.setUsage('claude-3', room(0, 60, true));
  limit(h.adapter.last);
  await tick();
  assert.equal(h.adapter.sessions.length, 1, 'no carry-on yet');
  const t = h.runner.get(thread.id);
  assert.deepEqual(checkSummary(t), []);
  assert.deepEqual(t.waitingForRoom, { until: new Date(START + 61 * MIN).toISOString(), accountId: 'claude-3' });
  assert.match(notes(h, thread.id).at(-1)!, /^three@example\.com hit its usage limit\. Carrying on at .* when three@example\.com is back\.$/);
  assert.deepEqual(h.records, [], 'not marked by the pool code');
  // Its login resets: it carries on there, never on claude-main.
  h.setUsage('claude-3', room(100, 360));
  h.clock.advance(62 * MIN);
  await tick();
  assert.equal(h.runner.get(thread.id).waitingForRoom, undefined);
  assert.equal(h.adapter.last.opts.account.id, 'claude-3');
  assert.equal(h.adapter.last.sent.at(-1)?.text, CARRY_ON_BACK);
  assert.equal(h.runner.get(thread.id).accountId, 'claude-3');
  assert.deepEqual(notes(h, thread.id).filter((t) => t.startsWith('Moved')), []);
});

test('a pinned wait whose login is still out waits again; a message from Mike ends it', async () => {
  const h = poolHarness();
  const { thread } = h.runner.create({ text: 'x', accountId: 'claude-3' });
  h.adapter.last.init();
  h.setUsage('claude-3', room(0, 30, true));
  limit(h.adapter.last);
  await tick();
  h.clock.advance(32 * MIN);
  await tick();
  assert.match(notes(h, thread.id).at(-1)!, /^three@example\.com is still at its limit\. Carrying on at /);
  assert.ok(h.runner.get(thread.id).waitingForRoom);
  assert.equal(h.adapter.sessions.length, 1);
  h.runner.send(thread.id, 'try now');
  await tick();
  assert.equal(h.runner.get(thread.id).waitingForRoom, undefined);
  assert.equal(h.adapter.last.sent.at(-1)?.text, 'try now');
});

test('carryOnAtReset: off ends the wait and stops the next one; on again waits while still stuck', async () => {
  const h = poolHarness();
  const { thread } = h.runner.create({ text: 'x', accountId: 'claude-3' });
  h.adapter.last.init();
  h.setUsage('claude-3', room(0, 60, true));
  limit(h.adapter.last);
  await tick();
  assert.ok(h.runner.get(thread.id).waitingForRoom);
  // Off: the wait ends, with a note, and the summary says so.
  const off = h.runner.update({ threadId: thread.id, carryOnAtReset: false }).thread;
  assert.deepEqual(checkSummary(off), []);
  assert.equal(off.carryOnAtReset, false);
  assert.equal(off.waitingForRoom, undefined);
  assert.match(notes(h, thread.id).at(-1)!, /^Won't carry on by itself\./);
  h.clock.advance(62 * MIN);
  await tick();
  assert.equal(h.adapter.sessions.length, 1, 'nothing at the reset');
  // On again while the last turn still stands at the limit: it waits.
  h.setUsage('claude-3', room(0, 120, true));
  const on = h.runner.update({ threadId: thread.id, carryOnAtReset: true }).thread;
  assert.equal(on.carryOnAtReset, undefined);
  assert.deepEqual(on.waitingForRoom, { until: new Date(START + 121 * MIN).toISOString(), accountId: 'claude-3' });
  // Off, then a turn that hits the limit doesn't wait at all.
  h.runner.update({ threadId: thread.id, carryOnAtReset: false });
  h.runner.send(thread.id, 'again');
  await tick();
  h.adapter.last.init();
  limit(h.adapter.last);
  await tick();
  assert.equal(h.runner.get(thread.id).waitingForRoom, undefined);
  // A pool thread with nothing left doesn't wait either.
  const p = h.runner.create({ text: 'y', poolId: 'claude' }).thread;
  h.runner.update({ threadId: p.id, carryOnAtReset: false });
  h.setUsage('claude-main', room(0, 60, true));
  h.adapter.last.init();
  limit(h.adapter.last);
  await tick();
  assert.equal(h.runner.get(p.id).waitingForRoom, undefined);
});

// ------------------------------------------------------- during a turn

test('a turn that hits the limit marks the login, moves and carries on once; the carry-on hitting it too waits', async () => {
  const h = poolHarness();
  h.setUsage('claude-main', room(80, 300));
  h.setUsage('claude-3', room(40, 120));
  const { thread } = h.runner.create({ text: 'Fix the tests' });
  assert.equal(thread.accountId, 'claude-main');
  const s = h.adapter.last;
  s.init();
  limit(s);
  await tick();
  // Marked blocked until its window resets, moved, and carried on.
  assert.equal(h.records.length, 1);
  assert.equal(h.records[0][0], 'claude-main');
  assert.equal(h.records[0][1].blocked, true);
  assert.equal(h.records[0][1].blockedUntil, START + 300 * MIN);
  const s2 = h.adapter.last;
  assert.notEqual(s2, s);
  assert.equal(s2.opts.account.id, 'claude-3');
  assert.equal(s2.opts.resume, true);
  assert.equal(s2.sent[0].text, CARRY_ON_MOVED);
  const carry = userMessages(h, thread.id).at(-1);
  assert.equal(carry.origin, 'runner');
  assert.match(notes(h, thread.id)[0], /^Moved to three@example\.com: main@example\.com hit its 5-hour limit, back at /);
  // The new login fails the same way straight away: no loop, a wait.
  s2.init();
  limit(s2);
  await tick();
  assert.equal(h.adapter.sessions.length, 2, 'no third session');
  const t = h.runner.get(thread.id);
  assert.deepEqual(checkSummary(t), []);
  assert.equal(t.waitingForRoom?.accountId, 'claude-3', 'claude-3 is back first');
  assert.equal(t.waitingForRoom?.until, new Date(START + 121 * MIN).toISOString());
  assert.match(notes(h, thread.id).at(-1)!, /^three@example\.com hit its usage limit too, so this thread waits rather than move again\. Carrying on at .* when three@example\.com is back\.$/);
  // At that time it looks again: claude-3 has reset, so it stays and carries on.
  h.setUsage('claude-3', room(100, 420));
  h.clock.advance(122 * MIN);
  await tick();
  assert.equal(h.runner.get(thread.id).waitingForRoom, undefined);
  assert.equal(h.adapter.last.sent.at(-1)?.text, CARRY_ON_BACK);
  assert.equal(h.adapter.last.opts.account.id, 'claude-3');
});

test('nothing in the pool has room: the thread waits, and the wait outlives a restart', async () => {
  const h = poolHarness();
  h.setUsage('claude-main', room(60, 200));
  h.setUsage('claude-3', room(2, 90));
  const { thread } = h.runner.create({ text: 'x' });
  h.adapter.last.init();
  limit(h.adapter.last);
  await tick();
  const w = h.runner.get(thread.id).waitingForRoom;
  assert.deepEqual(w, { until: new Date(START + 91 * MIN).toISOString(), accountId: 'claude-3' });
  assert.match(notes(h, thread.id).at(-1)!, /^Every login in Claude is out of room\. Carrying on at .* when three@example\.com is back\.$/);
  assert.equal(h.adapter.sessions.length, 1);
  // The UsageStore keeps the block across the restart; the fake stands in.
  h.setUsage('claude-main', room(60, 200, true));
  // A new runner on the same data dir (its clock starts at START again).
  const h2 = h.restart();
  assert.deepEqual(h2.runner.get(thread.id).waitingForRoom, w, 'kept in thread.json');
  h.fake.base = h.fake.base.map((a) => (a.id === 'claude-3' ? { ...a, usage: room(100) } : a));
  h2.clock.advance(80 * MIN);
  await tick();
  assert.equal(h2.adapter.sessions.length, 0, 'not yet');
  h2.clock.advance(12 * MIN);
  await tick();
  assert.equal(h2.adapter.sessions.length, 1);
  assert.equal(h2.adapter.last.opts.account.id, 'claude-3');
  assert.equal(h2.adapter.last.sent[0].text, CARRY_ON_MOVED);
  assert.equal(h2.runner.get(thread.id).waitingForRoom, undefined);
});

test('a wait that finds no room waits again; a message from Mike ends it and goes through the before-a-turn check', async () => {
  const h = poolHarness();
  h.setUsage('claude-main', room(0, 30, true));
  h.setUsage('claude-3', room(3));
  const { thread } = h.runner.create({ text: 'x', accountId: undefined });
  h.adapter.last.init();
  limit(h.adapter.last);
  await tick();
  // No reset time for claude-3, claude-main's block resets at 30 min.
  assert.equal(h.runner.get(thread.id).waitingForRoom?.accountId, 'claude-main');
  h.clock.advance(32 * MIN);
  await tick();
  // Still blocked (the reading says so, and it was marked).
  assert.match(notes(h, thread.id).at(-1)!, /^Still no room in Claude\. Carrying on at /);
  assert.ok(h.runner.get(thread.id).waitingForRoom);
  assert.equal(h.adapter.sessions.length, 1);
  // Mike sends something: the wait ends and the turn starts on the best there is.
  h.setUsage('claude-main', room(70));
  h.runner.send(thread.id, 'try now');
  await tick();
  assert.equal(h.runner.get(thread.id).waitingForRoom, undefined);
  assert.equal(h.adapter.last.opts.account.id, 'claude-main');
  assert.equal(h.adapter.last.sent.at(-1)?.text, 'try now');
});

test('spill: with no Claude login left, a mixed pool moves the thread to Codex as a handoff, and it stays there', async () => {
  const pools: Pool[] = [{ id: 'mixed', title: 'Mixed', accountIds: ['claude-3', 'codex-main'] }];
  const h = poolHarness({ pools });
  h.cfg.defaults.models = { codex: 'gpt-6-luna' };
  h.setUsage('claude-3', room(50, 60));
  const { thread } = h.runner.create({ text: 'x', model: 'claude-opus-5-5' });
  assert.equal(thread.poolId, 'mixed');
  assert.equal(thread.provider, 'claude');
  h.adapter.last.init();
  h.adapter.last.text('m1:0', 'Half done.');
  limit(h.adapter.last);
  await tick();
  const t = h.runner.get(thread.id);
  assert.equal(t.provider, 'codex');
  assert.equal(t.accountId, 'codex-main');
  assert.equal(t.model, 'gpt-6-luna', "the provider's default model");
  const cx = h.codex.last;
  assert.equal(cx.opts.resume, false);
  assert.match(cx.sent[0].text, /^\[Threadbear handoff\][\s\S]*Half done\.[\s\S]*Carry on from where you left off\.$/);
  assert.match(notes(h, thread.id)[0], /^Moved to Codex · main@example\.com: three@example\.com hit its 5-hour limit, back at .*\. It's on Codex now, with GPT-6-Luna and a summary of the thread so far\.$/);
  assert.equal(h.copies.length, 0, 'no transcript copy across providers');
  // Claude has room again later: the thread doesn't move back by itself.
  cx.init('019a-1', 'gpt-6-luna');
  cx.result();
  h.setUsage('claude-3', room(100));
  h.clock.advance(120 * MIN);
  h.runner.send(thread.id, 'more');
  await tick();
  assert.equal(h.runner.get(thread.id).provider, 'codex');
  // The idle CLI was released meanwhile; the next one resumes on Codex.
  assert.equal(h.codex.last.opts.account.id, 'codex-main');
  assert.equal(h.codex.last.opts.resume, true);
  assert.equal(h.adapter.sessions.length, 1, 'no Claude session since');
});

// ------------------------------------------------------- update and pools.*

test('threads.update poolId: null pins, a pool moves onto one of its logins, accountId pins, unknown is -32602', async () => {
  const h = poolHarness();
  const { thread } = h.runner.create({ text: 'x' });
  h.adapter.last.init();
  h.adapter.last.result();
  assert.equal(thread.poolId, 'claude');
  assert.equal(h.runner.update({ threadId: thread.id, poolId: null }).thread.poolId, undefined);
  // Into Codex's pool: its current login isn't there, so it moves now.
  const moved = h.runner.update({ threadId: thread.id, poolId: 'codex' }).thread;
  assert.equal(moved.poolId, 'codex');
  assert.equal(moved.accountId, 'codex-main');
  assert.equal(moved.provider, 'codex');
  // Back into Claude's: the Codex login isn't in it either.
  const back = h.runner.update({ threadId: thread.id, poolId: 'claude' }).thread;
  assert.equal(back.provider, 'claude');
  assert.equal(back.poolId, 'claude');
  // Already in it: stays where it is.
  const was = back.accountId;
  assert.equal(h.runner.update({ threadId: thread.id, poolId: 'claude' }).thread.accountId, was);
  const pinned = h.runner.update({ threadId: thread.id, accountId: 'claude-3' }).thread;
  assert.equal(pinned.poolId, undefined);
  assert.throws(() => h.runner.update({ threadId: thread.id, poolId: 'nope' }), (e: RpcError) => e.code === -32602);
});

test('pools.save replaces the list in runner.json, checks it, and tells clients; an empty list goes back to the built-in ones', async () => {
  const h = poolHarness();
  assert.equal(h.runner.listPools().pools[0].builtIn, true);
  const mine: Pool[] = [{ id: 'all', title: 'All', accountIds: ['claude-3', 'codex-main'], folders: ['~/dev'], builtIn: true }];
  const saved = (await h.runner.savePools(mine)).pools;
  assert.deepEqual(saved, [{ id: 'all', title: 'All', accountIds: ['claude-3', 'codex-main'], folders: ['~/dev'] }]);
  assert.deepEqual(readRunnerJson(h.home).pools, saved);
  assert.deepEqual(h.runner.listPools().pools, saved);
  assert.deepEqual(h.runner.hello().pools, saved);
  const told = h.notes.filter((n) => n.method === 'pools.updated');
  assert.equal(told.length, 1);
  assert.deepEqual((told[0] as any).params.pools, saved);
  await assert.rejects(() => h.runner.savePools([{ id: 'x', title: 'X', accountIds: ['claude-9'] }]), (e: RpcError) => e.code === -32602 && /claude-9/.test(e.message));
  await assert.rejects(() => h.runner.savePools([{ id: 'x', title: 'X', accountIds: [] }]), (e: RpcError) => e.code === -32602);
  await h.runner.savePools([]);
  assert.equal(readRunnerJson(h.home).pools, undefined);
  assert.equal(h.runner.listPools().pools[0].id, 'claude');
});

test('a new login changes the built-in pools and clients hear pools.updated', () => {
  const h = poolHarness({ accounts: [withCodex()[0]] });
  h.fake.base = withCodex();
  h.runner.rescanAccounts();
  const told = h.notes.filter((n) => n.method === 'pools.updated');
  assert.equal(told.length, 1);
  assert.deepEqual((told[0] as any).params.pools.map((p: Pool) => p.id), ['claude', 'codex']);
});

test('noted logins get a pool of their own and are never picked for another', () => {
  const accounts = [...withCodex(), { id: 'claude-2', provider: 'claude' as const, title: 'two@example.com', configDir: '/tmp/two', note: 'Convex work only' }];
  const h = poolHarness({ accounts });
  h.setUsage('claude-2', room(100));
  h.setUsage('claude-main', room(0, 60, true));
  h.setUsage('claude-3', room(0, 60, true));
  const pools = h.runner.listPools().pools;
  assert.deepEqual(pools.find((p) => p.id === 'convex')?.accountIds, ['claude-2']);
  const t = h.runner.create({ text: 'x', poolId: 'claude' }).thread;
  assert.notEqual(t.accountId, 'claude-2');
});
