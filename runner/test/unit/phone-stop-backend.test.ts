// Stop from the phone and the website (backend API 69). docs/parity.md
// showed Stop working on every Mac but not on the phone: phone:interrupt
// queues the `interrupt` command the main Mac already sends another runner
// (relay.ts), which every runner from API 44 on applies.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call, load, memoryBackend, outcome, THREAD_BASICS } from '../helpers/convex-memory.ts';

const OWNER = 'owner-key';
const BRUCE_KEY = 'tbr_bruce';
const BRUCE = 'cb921b2447ea';
const TOKEN = 'phone-token';

const { sha256 } = load('lib') as { sha256: (s: string) => Promise<string> };
const sync = load('sync');
const phone = load('phone');

test('Stop on the phone queues an interrupt for the thread\'s runner, which takes it', async () => {
  process.env.RUNNER_KEY = OWNER;
  delete process.env.RUNNER_KEY_NEXT;
  const b = memoryBackend();
  await b.ctx.db.insert('runnerKeys', { runnerId: BRUCE, keyHash: await sha256(BRUCE_KEY), name: 'bruce', createdAt: 1 });
  await b.ctx.db.insert('runners', { runnerId: BRUCE, name: 'bruce', version: 'v', startedAt: 'x', draining: false, threadCount: 1, lastSeenAt: Date.now() });
  await b.ctx.db.insert('tokens', { hash: await sha256(TOKEN), name: 'iphone', kind: 'phone', expiresAt: Date.now() + 1e9, createdAt: Date.now() });
  await call(b.ctx, sync.upsertThreads, { key: BRUCE_KEY, threads: [{ ...THREAD_BASICS, threadId: 'B', runnerId: BRUCE, title: 'T', view: 'inbox', needsYou: false, status: 'running' }] });
  const commandId = await call(b.ctx, phone.interrupt, { token: TOKEN, threadId: 'B' });
  const c = b.rows('commands').find((r) => r._id === commandId)!;
  assert.deepEqual({ runnerId: c.runnerId, threadId: c.threadId, kind: c.kind }, { runnerId: BRUCE, threadId: 'B', kind: 'interrupt' });
  // Bruce sees it among its commands, and the phone can watch it to done.
  const pending = (await call(b.ctx, sync.pendingCommands, { key: BRUCE_KEY, runnerId: BRUCE })) as { kind: string }[];
  assert.deepEqual(pending.map((p) => p.kind), ['interrupt']);
  await call(b.ctx, sync.takeCommand, { key: BRUCE_KEY, commandId });
  await call(b.ctx, sync.finishCommand, { key: BRUCE_KEY, commandId, ok: true });
  assert.equal((await call(b.ctx, phone.command, { token: TOKEN, commandId })).state, 'done');
  // A thread that isn't there, or no token, is refused.
  assert.equal(await outcome(call(b.ctx, phone.interrupt, { token: TOKEN, threadId: 'gone' })), 'notFound');
  await assert.rejects(call(b.ctx, phone.interrupt, { token: 'nope', threadId: 'B' }));
});
