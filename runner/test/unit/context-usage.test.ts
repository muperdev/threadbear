// What fills a thread's context (docs/protocol.md, "Context breakdown"):
// asked of the thread's live Claude CLI when the composer's ring opens,
// kept with the thread for when the CLI has gone, never a CLI started for
// it. Fake adapters (fakes.ts); the SDK answer's mapping is at the end.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { FakeAccounts, makeHarness, tick, withCodex } from '../helpers/fakes.ts';
import { claudeContextUsage } from '../../src/adapters/claude/adapter.ts';
import type { ContextUsage } from '../../src/protocol.ts';

const MIN = 60_000;

function harness() {
  return makeHarness(undefined, {}, { accounts: new FakeAccounts(withCodex()) });
}

const breakdown = (messages: number): Omit<ContextUsage, 'at'> => ({
  model: 'claude-haiku-5-5',
  totalTokens: 10_000 + messages,
  maxTokens: 200_000,
  categories: [
    { id: 'system_prompt', name: 'System prompt', tokens: 10_000, kind: 'used' },
    { id: 'messages', name: 'Messages', tokens: messages, kind: 'used' },
    { id: 'autocompact_buffer', name: 'Autocompact buffer', tokens: 33_000, kind: 'buffer' },
    { id: 'free_space', name: 'Free space', tokens: 157_000 - messages, kind: 'free' },
    { id: 'mcp_tools_deferred', name: 'MCP tools (deferred)', tokens: 90_000, kind: 'deferred' },
  ],
});

function liveThread(h: ReturnType<typeof harness>) {
  const { thread } = h.runner.create({ text: 'hi', accountId: 'claude-main' });
  const s = h.adapter.last;
  s.init();
  s.result();
  let asked = 0;
  let messages = 500;
  s.contextUsage = async () => {
    asked++;
    return breakdown(messages);
  };
  return { thread, s, asked: () => asked, setMessages: (n: number) => (messages = n) };
}

test('a live Claude thread asks its CLI, stamps the time and keeps it with the thread', async () => {
  const h = harness();
  const { thread, asked } = liveThread(h);
  const r = await h.runner.contextUsage(thread.id);
  assert.equal(asked(), 1);
  assert.equal(r.live, true);
  assert.equal(r.usage?.totalTokens, 10_500);
  assert.equal(r.usage?.at, new Date(h.clock.now()).toISOString());
  assert.deepEqual(r.usage?.categories.map((c) => c.id), ['system_prompt', 'messages', 'autocompact_buffer', 'free_space', 'mcp_tools_deferred']);
  // In thread.json, for after the CLI has gone or the runner restarted.
  assert.equal(h.store.load(thread.id)?.contextUsage?.totalTokens, 10_500);
});

test('each ask goes to the CLI afresh; two at once share one ask', async () => {
  const h = harness();
  const { thread, asked, setMessages } = liveThread(h);
  const [a, b] = await Promise.all([h.runner.contextUsage(thread.id), h.runner.contextUsage(thread.id)]);
  assert.equal(asked(), 1);
  assert.deepEqual(a, b);
  setMessages(9_000);
  h.clock.advance(MIN);
  const c = await h.runner.contextUsage(thread.id);
  assert.equal(asked(), 2);
  assert.equal(c.usage?.totalTokens, 19_000);
});

test('once the CLI has gone it answers the kept one and starts no CLI', async () => {
  const h = harness();
  const { thread } = liveThread(h);
  const first = await h.runner.contextUsage(thread.id);
  h.clock.advance(30 * MIN);
  await tick();
  assert.equal(h.runner.isLive(thread.id), false);
  const later = await h.runner.contextUsage(thread.id);
  assert.equal(later.live, false);
  assert.deepEqual(later.usage, first.usage);
  assert.equal(h.adapter.sessions.length, 1, 'no CLI started for it');
});

test('with no CLI and nothing kept there is no breakdown', async () => {
  const h = harness();
  const { thread } = h.runner.create({ text: 'hi', accountId: 'claude-main' });
  const s = h.adapter.last;
  s.init();
  s.result();
  h.clock.advance(30 * MIN);
  await tick();
  const r = await h.runner.contextUsage(thread.id);
  assert.deepEqual(r, { live: false });
  assert.equal(h.adapter.sessions.length, 1);
});

test('a CLI that fails to answer gives the kept one, with a note', async () => {
  const h = harness();
  const { thread, s } = liveThread(h);
  const first = await h.runner.contextUsage(thread.id);
  s.contextUsage = async () => {
    throw new Error('control request failed');
  };
  h.clock.advance(MIN);
  const r = await h.runner.contextUsage(thread.id);
  assert.equal(r.live, true);
  assert.deepEqual(r.usage, first.usage);
  assert.match(r.note ?? '', /Claude didn't answer/);
});

test('a new native session (Claude\'s /clear) drops the kept one', async () => {
  const h = harness();
  const { thread, s } = liveThread(h);
  await h.runner.contextUsage(thread.id);
  // The CLI's /clear carries on in a session with a new id.
  h.runner.send(thread.id, '/clear');
  await tick();
  s.init('after-clear');
  s.result();
  h.clock.advance(30 * MIN);
  await tick();
  const r = await h.runner.contextUsage(thread.id);
  assert.equal(r.usage, undefined);
});

test('a Codex thread has no breakdown', async () => {
  const h = harness();
  const { thread } = h.runner.create({ text: 'hi', accountId: 'codex-main' });
  const c = h.codex.last;
  c.init('codex-session');
  c.result();
  const r = await h.runner.contextUsage(thread.id);
  assert.deepEqual(r, { live: false });
});

test("claudeContextUsage keeps the rows and totals, and what the app doesn't know is left to it", () => {
  const u = claudeContextUsage({
    categories: [
      { id: 'messages', name: 'Messages', tokens: 628, color: 'purple', kind: 'used' },
      { name: 'Something new', tokens: 12, color: 'x', kind: 'used' },
      { id: 'free_space', name: 'Free space', tokens: 999_360, color: 'y', kind: 'free' },
      { name: 'broken', tokens: 'lots', kind: 'used' },
    ],
    totalTokens: 640,
    maxTokens: 1_000_000,
    rawMaxTokens: 1_000_000,
    percentage: 0,
    model: 'claude-haiku-5-5',
    gridRows: [],
  } as never);
  assert.deepEqual(u, {
    model: 'claude-haiku-5-5',
    totalTokens: 640,
    maxTokens: 1_000_000,
    categories: [
      { id: 'messages', name: 'Messages', tokens: 628, kind: 'used' },
      { name: 'Something new', tokens: 12, kind: 'used' },
      { id: 'free_space', name: 'Free space', tokens: 999_360, kind: 'free' },
    ],
  });
});

test('an answer that comes after the thread was deleted brings nothing back', async () => {
  const h = harness();
  const { thread, s } = liveThread(h);
  let answer!: (u: Omit<ContextUsage, 'at'>) => void;
  s.contextUsage = () => new Promise((r) => (answer = r));
  const asked = h.runner.contextUsage(thread.id);
  h.runner.delete(thread.id);
  answer(breakdown(500));
  assert.deepEqual(await asked, { live: false });
  assert.equal(h.store.load(thread.id), undefined, 'no thread.json written back');
});

test('a thread moved to Codex shows no Claude breakdown, nor one waiting on a move', async () => {
  const h = harness();
  const { thread } = liveThread(h);
  await h.runner.contextUsage(thread.id);
  h.runner.update({ threadId: thread.id, accountId: 'claude-3' });
  h.clock.advance(30 * MIN);
  await tick();
  assert.equal((await h.runner.contextUsage(thread.id)).usage, undefined, 'a move to another login is waiting');
  h.runner.update({ threadId: thread.id, accountId: 'codex-main' });
  assert.deepEqual(await h.runner.contextUsage(thread.id), { live: false });
});
