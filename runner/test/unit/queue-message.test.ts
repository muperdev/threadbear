// threads.send with `queue: true` (the Mac app's Cmd+Enter): while a turn
// runs the message waits and starts its own turn when that one ends,
// rather than steering into it; threads.unqueue takes one back before it
// goes (docs/protocol.md, threads.send and threads.unqueue).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeHarness } from '../helpers/fakes.ts';
import { RpcServer } from '../../src/rpc.ts';
import { RunnerClient } from '../../src/client.ts';
import { nullLogger } from '../../src/log.ts';

function running() {
  const h = makeHarness();
  const { thread, turnId } = h.runner.create({ text: 'Fix the tests', accountId: 'claude-3' });
  const s = h.adapter.last;
  s.init();
  return { h, thread, turnId, s };
}

const messages = (h: ReturnType<typeof makeHarness>, id: string) =>
  h.events(id).filter((e) => e.type === 'user.message').map((e) => ({ text: e.text, turnId: e.turnId }));

test('a queued message waits for the running turn, then starts its own', () => {
  const { h, thread, turnId, s } = running();
  const r = h.runner.send(thread.id, 'then update the docs', [], { queue: true });
  assert.equal(r.queued, true);
  assert.notEqual(r.turnId, turnId, 'it gets a turn of its own');
  assert.equal(s.sent.length, 1, 'nothing went into the running turn');
  assert.deepEqual(messages(h, thread.id), [{ text: 'Fix the tests', turnId }], 'not in the transcript yet');
  s.result('completed');
  const next = h.adapter.last;
  assert.equal(next.sent.at(-1)!.text, 'then update the docs');
  assert.deepEqual(messages(h, thread.id).at(-1), { text: 'then update the docs', turnId: r.turnId });
});

test('without a running turn a queued send just starts one', () => {
  const { h, thread, s } = running();
  s.result('completed');
  const r = h.runner.send(thread.id, 'next', [], { queue: true });
  assert.equal(r.queued, undefined);
  assert.equal(h.runner.get(thread.id).status, 'running');
  assert.deepEqual(messages(h, thread.id).at(-1), { text: 'next', turnId: r.turnId });
});

test('a plain send still steers into the running turn and says nothing of a queue', () => {
  const { h, thread, turnId, s } = running();
  const r = h.runner.send(thread.id, 'also this');
  assert.deepEqual(r, { turnId });
  assert.equal(s.sent.at(-1)!.text, 'also this');
});

test('unqueue takes a waiting message out so it never goes', () => {
  const { h, thread, s } = running();
  const a = h.runner.send(thread.id, 'first queued', [], { queue: true });
  const b = h.runner.send(thread.id, 'second queued', [], { queue: true });
  h.runner.unqueue(thread.id, a.turnId);
  assert.deepEqual(h.runner.mustGet(thread.id).rec.queue.map((q) => q.text), ['second queued']);
  s.result('completed');
  assert.deepEqual(messages(h, thread.id).slice(1).map((m) => m.text), ['second queued']);
  // Started now: too late to take back.
  assert.throws(() => h.runner.unqueue(thread.id, b.turnId), (e: { code: number }) => e.code === -32602);
  assert.throws(() => h.runner.unqueue(thread.id, 'never-queued'), (e: { code: number }) => e.code === -32602);
});

test('over RPC: queue and unqueue', async () => {
  const { h, thread, s } = running();
  const server = new RpcServer({ host: '127.0.0.1', port: 0, token: 'test-token-0123456789', runner: h.runner, log: nullLogger });
  const { port } = await server.listen();
  const c = await RunnerClient.connect(`ws://127.0.0.1:${port}/rpc`, 'test-token-0123456789');
  try {
    const r = await c.call('threads.send', { threadId: thread.id, text: 'later', queue: true });
    assert.equal(r.queued, true);
    assert.equal(s.sent.length, 1);
    assert.deepEqual(await c.call('threads.unqueue', { threadId: thread.id, turnId: r.turnId }), {});
    assert.equal(h.runner.mustGet(thread.id).rec.queue.length, 0);
    await assert.rejects(c.call('threads.unqueue', { threadId: thread.id, turnId: r.turnId }), (e: { code: number }) => e.code === -32602);
  } finally {
    c.close();
    await server.close();
  }
});
