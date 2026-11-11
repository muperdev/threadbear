// Reply buttons and slash commands on another runner's thread from the Mac,
// through Convex (backend API 68; docs/protocol.md, "Other runners' threads
// through the backend"). docs/parity.md showed both working from the phone
// and not from the Mac on a Hetzner or bruce thread. The relay reads a
// message row's buttons as the phone does, and the main Mac queues `press`
// and `slashCommands` for that runner, the commands the phone already sends.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call, load, memoryBackend, outcome, THREAD_BASICS } from '../helpers/convex-memory.ts';

const OWNER = 'owner-key';
const BRUCE_KEY = 'tbr_bruce';
const BRUCE = 'cb921b2447ea';

const { sha256 } = load('lib') as { sha256: (s: string) => Promise<string> };
const sync = load('sync');
const relay = load('relay');

async function setup() {
  process.env.RUNNER_KEY = OWNER;
  delete process.env.RUNNER_KEY_NEXT;
  const b = memoryBackend();
  await b.ctx.db.insert('runnerKeys', { runnerId: BRUCE, keyHash: await sha256(BRUCE_KEY), name: 'bruce', createdAt: 1 });
  await b.ctx.db.insert('backendMarks', { name: 'limits-2026-11-07' });
  await b.ctx.db.insert('backendMarks', { name: 'limits-2026-11-07-done' });
  await b.ctx.db.insert('runners', { runnerId: BRUCE, name: 'bruce', version: 'v', startedAt: 'x', draining: false, threadCount: 1, lastSeenAt: Date.now() });
  await call(b.ctx, sync.upsertThreads, { key: BRUCE_KEY, threads: [{ ...THREAD_BASICS, threadId: 'B', runnerId: BRUCE, title: 'T', view: 'inbox', needsYou: false }] });
  return b;
}

test("a message row's buttons reach the main Mac's relay, as the phone gets them", async () => {
  const b = await setup();
  const buttons = [
    { id: 'abcd1234', title: 'Fix the flaky test', kind: 'start', target: 'threadbear' },
    { id: 'efgh5678', title: 'Tell the deploy thread', kind: 'send', target: 'Deploy', pressedThreadId: 'T9' },
  ];
  await call(b.ctx, sync.appendMessages, { key: BRUCE_KEY, messages: [{ threadId: 'B', seq: 5, role: 'assistant', text: 'Two jobs: [Fix the flaky test](button:abcd1234) [Tell the deploy thread](button:efgh5678)', at: '2026-11-09T00:00:00.000Z', buttons }] });
  const tail = await call(b.ctx, relay.remoteTail, { key: OWNER, threadId: 'B' });
  assert.deepEqual(tail.messages[0].buttons, buttons);
  const page = await call(b.ctx, relay.remoteMessages, { key: OWNER, threadId: 'B' });
  assert.deepEqual(page.messages[0].buttons, buttons);
});

test('the main Mac can press a button and ask for slash commands on another runner\'s thread', async () => {
  const b = await setup();
  const press = await call(b.ctx, relay.queueCommand, { key: OWNER, runnerId: BRUCE, threadId: 'B', kind: 'press', args: { buttonId: 'abcd1234' } });
  assert.deepEqual(b.rows('commands').find((c) => c._id === press)!.args, { buttonId: 'abcd1234', from: 'mac' });
  assert.equal(await outcome(call(b.ctx, relay.queueCommand, { key: OWNER, runnerId: BRUCE, threadId: 'B', kind: 'press', args: { buttonId: 'NOPE' } })), 'badArgs');
  assert.equal(await outcome(call(b.ctx, relay.queueCommand, { key: OWNER, runnerId: BRUCE, threadId: 'B', kind: 'press', args: {} })), 'badArgs');
  const list = await call(b.ctx, relay.queueCommand, { key: OWNER, runnerId: BRUCE, threadId: 'B', kind: 'slashCommands', args: { anything: 'dropped' } });
  assert.deepEqual(b.rows('commands').find((c) => c._id === list)!.args, { from: 'mac' });
  // Both are about a thread.
  assert.equal(await outcome(call(b.ctx, relay.queueCommand, { key: OWNER, runnerId: BRUCE, kind: 'slashCommands', args: {} })), 'badArgs');
  // Bruce finishes the list with its answer, which the Mac reads back.
  await call(b.ctx, sync.takeCommand, { key: BRUCE_KEY, commandId: list });
  await call(b.ctx, sync.finishCommand, { key: BRUCE_KEY, commandId: list, ok: true, result: JSON.stringify([{ name: 'review', description: 'Review the diff' }]) });
  assert.deepEqual((await call(b.ctx, relay.commandState, { key: OWNER, commandId: list })).result, [{ name: 'review', description: 'Review the diff' }]);
  // A press finishes with the thread it started.
  await call(b.ctx, sync.upsertThreads, { key: BRUCE_KEY, threads: [{ ...THREAD_BASICS, threadId: 'NEW', runnerId: BRUCE, title: 'Started', view: 'inbox', needsYou: false }] });
  await call(b.ctx, sync.takeCommand, { key: BRUCE_KEY, commandId: press });
  await call(b.ctx, sync.finishCommand, { key: BRUCE_KEY, commandId: press, ok: true, resultThreadId: 'NEW' });
  assert.equal((await call(b.ctx, relay.commandState, { key: OWNER, commandId: press })).threadId, 'NEW');
  // Only the main Mac's key queues work for another runner.
  assert.equal(await outcome(call(b.ctx, relay.queueCommand, { key: BRUCE_KEY, runnerId: BRUCE, threadId: 'B', kind: 'press', args: { buttonId: 'abcd1234' } })), 'forbidden');
});
