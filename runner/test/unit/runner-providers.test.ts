// Piece C of step 4: threads across Claude and Codex, Codex-style adapter
// flags, usage and accounts.updated, and the hooks CLI updates call. All
// with fake adapters (fakes.ts: `h.codex` has Codex's flags).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { CODEX_ACCOUNT, FakeAccounts, FakeAdapter, makeHarness, tick, tmpDir, withCodex } from '../helpers/fakes.ts';
import { checkSummary } from '../helpers/protocol-check.ts';
import { SessionNotFoundError } from '../../src/adapters/types.ts';
import { writeRunnerJson } from '../../src/config.ts';
import type { Account, CliStatus } from '../../src/protocol.ts';
import type { CliControl, UsageSink } from '../../src/runner.ts';
import { RpcError } from '../../src/protocol.ts';

const SEC = 1000;

function codexHarness(cfg: Parameters<typeof makeHarness>[1] = {}, deps: Parameters<typeof makeHarness>[2] = {}, accounts: Account[] = withCodex()) {
  const fake = new FakeAccounts(accounts);
  const h = makeHarness(undefined, cfg, { accounts: fake, ...deps });
  return { ...h, fake };
}

const notes = (h: { events(id: string): any[] }, id: string): string[] => h.events(id).filter((e) => e.type === 'system.note').map((e) => e.text);

// ------------------------------------------------------- providers on create

test('a thread on a Codex account is a Codex thread, with the runner\'s Codex defaults', () => {
  const h = codexHarness({ defaults: { cwd: '', model: 'claude-haiku-5-5', models: { codex: 'gpt-6-luna' }, efforts: { codex: 'low' } } });
  h.cfg.defaults.cwd = path.join(h.home, 'work');
  const { thread } = h.runner.create({ text: 'hi', accountId: 'codex-main' });
  assert.deepEqual(checkSummary(thread), []);
  assert.equal(thread.provider, 'codex');
  assert.equal(thread.model, 'gpt-6-luna');
  assert.equal(thread.effort, 'low');
  assert.equal(h.adapter.sessions.length, 0, 'never the Claude adapter');
  const s = h.codex.last;
  assert.equal(s.opts.account.id, 'codex-main');
  assert.equal(s.opts.model, 'gpt-6-luna');
  assert.equal(s.opts.effort, 'low');
  // A Claude thread keeps Claude's default and no Codex effort.
  const c = h.runner.create({ text: 'hi', accountId: 'claude-3' }).thread;
  assert.equal(c.provider, 'claude');
  assert.equal(c.model, 'claude-haiku-5-5');
  assert.equal(c.effort, undefined);
  const hello = h.runner.hello();
  assert.deepEqual(hello.defaults.models, { codex: 'gpt-6-luna' });
  assert.deepEqual(hello.defaults.efforts, { codex: 'low' });
  assert.equal(hello.defaults.model, 'claude-haiku-5-5');
});

test('an old-shape client sending sonnet for a Codex account gets the Codex default and a note', () => {
  const h = codexHarness();
  // No runner default: the account's own, by its title.
  const { thread } = h.runner.create({ text: 'hi', accountId: 'codex-main', model: 'sonnet' });
  assert.equal(thread.model, undefined);
  assert.equal(h.codex.last.opts.model, undefined);
  assert.deepEqual(notes(h, thread.id), ["sonnet isn't a Codex model, so this thread uses GPT-6.1-Sol."]);
  // With one: that, and the note is under the first turn.
  h.cfg.defaults.models = { codex: 'gpt-6-luna' };
  const b = h.runner.create({ text: 'hi', accountId: 'codex-main', model: 'claude-opus-5-5' }).thread;
  assert.equal(b.model, 'gpt-6-luna');
  const note = h.events(b.id).find((e) => e.type === 'system.note');
  assert.equal(note.text, "claude-opus-5-5 isn't a Codex model, so this thread uses GPT-6-Luna.");
  assert.equal(note.turnId, h.events(b.id).find((e) => e.type === 'turn.started').turnId);
  // A Codex id the account doesn't list is dropped too; a listed one isn't.
  assert.equal(h.runner.create({ text: 'x', accountId: 'codex-main', model: 'gpt-9-nope' }).thread.model, 'gpt-6-luna');
  assert.equal(h.runner.create({ text: 'x', accountId: 'codex-main', model: 'gpt-6.1-sol' }).thread.model, 'gpt-6.1-sol');
  // A Codex id on a Claude thread gives way to Claude's default.
  const c = h.runner.create({ text: 'x', accountId: 'claude-3', model: 'gpt-6-luna' }).thread;
  assert.equal(c.provider, 'claude');
  assert.equal(c.model, undefined);
  assert.match(notes(h, c.id)[0], /^gpt-6-luna isn't a Claude model/);
  // Anything Claude-shaped stays on Claude, listed or not.
  assert.equal(h.runner.create({ text: 'x', accountId: 'claude-3', model: 'opus[1m]' }).thread.model, 'opus[1m]');
});

test('before a Codex account lists its models, any gpt- id goes and nothing else does', () => {
  const h = codexHarness({}, {}, withCodex(null));
  assert.equal(h.runner.create({ text: 'x', accountId: 'codex-main', model: 'gpt-7-new' }).thread.model, 'gpt-7-new');
  const t = h.runner.create({ text: 'x', accountId: 'codex-main', model: 'haiku' }).thread;
  assert.equal(t.model, undefined);
  assert.match(notes(h, t.id)[0], /haiku isn't a Codex model, so this thread uses gpt-6\.1-sol\./);
});

test('ultra is dropped from a Claude thread, and an effort the model lacks from a Codex one', () => {
  const h = codexHarness();
  const c = h.runner.create({ text: 'x', accountId: 'claude-3', effort: 'ultra' }).thread;
  assert.equal(c.effort, undefined);
  assert.equal(h.adapter.last.opts.effort, undefined);
  assert.deepEqual(notes(h, c.id), ["Ultra effort is Codex only, so this thread uses the model's own level."]);
  // Sol offers ultra; luna stops at max.
  assert.equal(h.runner.create({ text: 'x', accountId: 'codex-main', model: 'gpt-6.1-sol', effort: 'ultra' }).thread.effort, 'ultra');
  const l = h.runner.create({ text: 'x', accountId: 'codex-main', model: 'gpt-6-luna', effort: 'ultra' }).thread;
  assert.equal(l.effort, undefined);
  assert.deepEqual(notes(h, l.id), ["GPT-6-Luna doesn't offer Ultra effort, so this thread uses the model's own level."]);
  // The account's default model decides when the thread names none.
  assert.equal(h.runner.create({ text: 'x', accountId: 'codex-main', effort: 'ultra' }).thread.effort, 'ultra');
});

test('a Codex account with no Codex adapter is refused, not run on Claude', () => {
  // Only Claude's adapter, as on step4 before the Codex adapter is wired.
  const h2 = makeHarness(undefined, {}, { accounts: new FakeAccounts(withCodex()), adapters: { claude: new FakeAdapter() } });
  assert.throws(() => h2.runner.create({ text: 'x', accountId: 'codex-main' }), (e: RpcError) => e.code === -32602 && /can't run Codex threads/.test(e.message));
  const { thread } = h2.runner.create({ text: 'x', accountId: 'claude-3' });
  assert.throws(() => h2.runner.update({ threadId: thread.id, accountId: 'codex-main' }), /can't run Codex threads/);
  // And it's never the default for a new thread.
  writeRunnerJson(h2.home, { defaults: { accountId: 'codex-main' } });
  assert.equal(h2.runner.hello().defaults.accountId, 'claude-main');
});

// ------------------------------------------------------------------ rebuild

test('a rebuilt thread takes its provider from the default account (codex-main)', async () => {
  const h = codexHarness();
  const { thread } = h.runner.create({ text: 'Fix the tests', accountId: 'codex-main' });
  const s = h.codex.last;
  s.init('019a-codex-thread', 'gpt-6.1-sol');
  s.text('m1', 'Done.');
  s.result();
  fs.writeFileSync(path.join(h.home, 'threads', thread.id, 'thread.json'), '');
  // runner.json remembers codex-main as the last account used.
  const h2 = h.restart();
  const t = h2.runner.get(thread.id);
  assert.equal(t.accountId, 'codex-main');
  assert.equal(t.provider, 'codex');
  h2.runner.send(thread.id, 'carry on');
  await tick();
  assert.equal(h2.adapter.sessions.length, 0, 'not the Claude CLI with CLAUDE_CONFIG_DIR=~/.codex');
  assert.equal(h2.codex.last.opts.account.id, 'codex-main');
  assert.match(h2.codex.last.sent[0].text, /earlier session couldn't be found/);
});

// ------------------------------------------------------- update across providers

test('moving a Claude thread to codex-main switches provider, resets settings and hands off', async () => {
  const h = codexHarness({ defaults: { cwd: '', models: { codex: 'gpt-6-luna' }, efforts: { codex: 'low' } } });
  h.cfg.defaults.cwd = path.join(h.home, 'work');
  const { thread } = h.runner.create({ text: 'first', accountId: 'claude-3', model: 'opus', effort: 'max' });
  const s = h.adapter.last;
  s.init();
  s.out({ kind: 'allowRules', rules: ['Bash(npm test)'] });
  s.text('m1', 'Hello from Claude');
  s.out({ kind: 'cliVersion', version: '2.1.311' });
  s.result();
  const t = h.runner.update({ threadId: thread.id, accountId: 'codex-main' }).thread;
  assert.equal(t.provider, 'codex');
  assert.equal(t.model, 'gpt-6-luna');
  assert.equal(t.effort, 'low');
  assert.ok(s.ended, 'the Claude CLI is released');
  h.runner.send(thread.id, 'next');
  await tick();
  const c = h.codex.last;
  assert.equal(c.opts.resume, false);
  assert.deepEqual(c.opts.allowRules, [], 'Claude-syntax rules are gone');
  assert.match(c.sent[0].text, /^\[Threadbear handoff\] This conversation moved from the claude-3 login|from the three@example\.com login/);
  assert.match(c.sent[0].text, /Assistant: Hello from Claude[\s\S]*next$/);
  // Claude's version means nothing for Codex: no "was 2.1.311" note.
  c.out({ kind: 'cliVersion', version: '0.164.0' });
  assert.ok(!notes(h, thread.id).some((n) => /from this turn on/.test(n)));
  // Settings named in the same call are kept, after the check.
  const b = h.runner.create({ text: 'x', accountId: 'claude-3' }).thread;
  h.adapter.last.init();
  h.adapter.last.result();
  const moved = h.runner.update({ threadId: b.id, accountId: 'codex-main', model: 'sonnet', effort: 'high' }).thread;
  assert.equal(moved.model, 'gpt-6-luna');
  assert.equal(moved.effort, 'high');
  assert.ok(notes(h, b.id).some((n) => n.startsWith("sonnet isn't a Codex model")));
});

test('Claude keeps carrying its transcript on a folder move; it never tries for Codex', async () => {
  const copies: string[] = [];
  const h = codexHarness({}, { copyTranscript: (_c, _d, _f, to) => (copies.push(to), true) });
  const other = tmpDir();
  const { thread } = h.runner.create({ text: 'x', accountId: 'claude-3' });
  h.adapter.last.init();
  h.adapter.last.result();
  h.runner.update({ threadId: thread.id, cwd: other });
  h.runner.send(thread.id, 'y');
  await tick();
  assert.equal(copies.length, 1);
  assert.equal(h.adapter.last.opts.resume, true);
});

// ------------------------------------------------------------- folder moves

test('a same-account folder move on Codex carries on in the same live session', async () => {
  const h = codexHarness();
  const other = tmpDir();
  const { thread } = h.runner.create({ text: 'x', accountId: 'codex-main' });
  const s = h.codex.last;
  s.init('019a-1', 'gpt-6.1-sol');
  s.result();
  h.runner.update({ threadId: thread.id, cwd: other });
  await tick();
  assert.deepEqual(s.cwds, [path.resolve(other)]);
  assert.equal(s.ended, false, 'not released');
  h.runner.send(thread.id, 'next');
  await tick();
  assert.equal(h.codex.sessions.length, 1, 'same session');
  assert.equal(s.sent.at(-1)!.text, 'next', 'no handoff');
  const ns = notes(h, thread.id);
  assert.equal(ns.at(-1), `Moved to ${path.resolve(other)}. The session carries on there.`);
  assert.ok(!ns.includes('Resumed session'));
});

test('a folder move on a released Codex session resumes there on the next turn', async () => {
  const h = codexHarness();
  const other = tmpDir();
  const { thread } = h.runner.create({ text: 'x', accountId: 'codex-main' });
  const s = h.codex.last;
  s.init('019a-2', 'gpt-6.1-sol');
  s.result();
  h.clock.advance(31 * 60 * SEC); // idle release
  await tick();
  assert.equal(h.runner.isLive(thread.id), false);
  h.runner.update({ threadId: thread.id, cwd: other });
  h.runner.send(thread.id, 'next');
  await tick();
  const s2 = h.codex.last;
  assert.notEqual(s2, s);
  assert.equal(s2.opts.resume, true);
  assert.equal(s2.opts.sessionId, '019a-2');
  assert.equal(s2.opts.cwd, path.resolve(other));
  assert.equal(s2.sent[0].text, 'next');
  assert.equal(notes(h, thread.id).at(-1), `Moved to ${path.resolve(other)}. The session carries on there.`);
});

// ------------------------------------------------------------ next-turn note

test('a Codex settings change during a turn says it applies from the next turn', () => {
  const h = codexHarness();
  const other = tmpDir();
  const { thread } = h.runner.create({ text: 'x', accountId: 'codex-main' });
  const s = h.codex.last;
  s.init('019a-3', 'gpt-6.1-sol');
  h.runner.update({ threadId: thread.id, permissionMode: 'ask' });
  assert.equal(notes(h, thread.id).at(-1), 'Ask before edits applies from the next turn. This turn carries on as it started.');
  h.runner.update({ threadId: thread.id, model: 'gpt-6-luna', effort: 'low', cwd: other });
  assert.equal(
    notes(h, thread.id).at(-1),
    `The move to ${path.resolve(other)}, GPT-6-Luna and Low effort apply from the next turn. This turn carries on as it started.`,
  );
  assert.deepEqual(s.modes, ['ask']);
  assert.deepEqual(s.models, ['gpt-6-luna']);
  // Between turns there's nothing to say.
  s.result();
  const before = notes(h, thread.id).length;
  h.runner.update({ threadId: thread.id, permissionMode: 'full' });
  assert.equal(notes(h, thread.id).length, before);
  // Claude takes changes straight away, so it never says it.
  const c = h.runner.create({ text: 'x', accountId: 'claude-3' }).thread;
  h.adapter.last.init();
  h.runner.update({ threadId: c.id, permissionMode: 'ask' });
  assert.deepEqual(notes(h, c.id), []);
});

// ------------------------------------------------------------ sessions

test('a Codex session that can\'t be found starts fresh once, with the handoff', async () => {
  const h = codexHarness();
  const { thread } = h.runner.create({ text: 'remember pineapple', accountId: 'codex-main' });
  const s = h.codex.last;
  s.init('019a-gone', 'gpt-6.1-sol');
  s.text('m1', 'Remembered.');
  s.result();
  h.clock.advance(31 * 60 * SEC);
  await tick();
  const { turnId } = h.runner.send(thread.id, 'what word?');
  await tick();
  const r = h.codex.last;
  assert.equal(r.opts.resume, true);
  r.sink.exited(new SessionNotFoundError('thread 019a-gone not found'));
  await tick();
  const fresh = h.codex.last;
  assert.notEqual(fresh, r);
  assert.equal(fresh.opts.resume, false);
  assert.equal(fresh.sent[0].id, turnId);
  assert.match(fresh.sent[0].text, /^\[Threadbear handoff\] The earlier session couldn't be found, so here's where the thread got to\./);
  assert.match(fresh.sent[0].text, /User: remember pineapple\n\nAssistant: Remembered\.[\s\S]*\[End of handoff\. The new message follows\.\]\n\nwhat word\?$/);
  assert.ok(notes(h, thread.id).includes("Couldn't resume the earlier session, so this is a new one."));
  fresh.init('019a-new', 'gpt-6.1-sol');
  fresh.result();
  assert.equal(h.events(thread.id).at(-1).outcome, 'completed');
  // Once the fresh one ran, a later loss gets its own fresh start.
  h.clock.advance(31 * 60 * SEC);
  await tick();
  h.runner.send(thread.id, 'again');
  await tick();
  h.codex.last.sink.exited(new SessionNotFoundError());
  await tick();
  assert.equal(h.codex.last.opts.resume, false);
});

test('the old Claude "no conversation found" still starts fresh, with the handoff after an earlier turn', async () => {
  const h = codexHarness();
  const { thread } = h.runner.create({ text: 'one', accountId: 'claude-3' });
  const s = h.adapter.last;
  s.init();
  s.sink.exited(new Error('boom'));
  // The first turn failed before the CLI wrote anything.
  h.runner.send(thread.id, 'two');
  await tick();
  const r = h.adapter.last;
  assert.equal(r.opts.resume, true);
  r.sink.exited(new Error('No conversation found with session ID x'));
  await tick();
  assert.equal(h.adapter.last.opts.resume, false);
  // The failed turn counts as an earlier turn, so the handoff goes with it.
  assert.match(h.adapter.last.sent[0].text, /earlier session couldn't be found[\s\S]*two$/);
});

test('cliVersion is kept, and a new session on another version says so', async () => {
  const h = codexHarness();
  const { thread } = h.runner.create({ text: 'x', accountId: 'codex-main' });
  const s = h.codex.last;
  s.init('019a-v', 'gpt-6.1-sol');
  s.out({ kind: 'cliVersion', version: '0.164.0' });
  s.out({ kind: 'context', contextTokens: 24177, contextWindow: 258400 });
  s.result();
  const t = h.runner.get(thread.id);
  assert.equal(t.contextTokens, 24177);
  assert.equal(t.contextWindow, 258400);
  assert.deepEqual(checkSummary(t), []);
  assert.deepEqual(h.runner.versionsInUse('codex'), [{ version: '0.164.0', threadIds: [thread.id] }]);
  assert.deepEqual(notes(h, thread.id), []);
  h.clock.advance(31 * 60 * SEC);
  await tick();
  const { turnId } = h.runner.send(thread.id, 'next');
  await tick();
  const s2 = h.codex.last;
  s2.init('019a-v', 'gpt-6.1-sol');
  s2.out({ kind: 'cliVersion', version: '0.165.0' });
  const note = h.events(thread.id).filter((e) => e.type === 'system.note').at(-1);
  assert.equal(note.text, 'Codex 0.165.0 from this turn on (was 0.164.0)');
  assert.equal(note.turnId, turnId);
  s2.out({ kind: 'cliVersion', version: '0.165.0' });
  assert.equal(notes(h, thread.id).filter((n) => /from this turn on/.test(n)).length, 1);
});

test('limits go to the usage store with the thread\'s account', () => {
  const recorded: [string, unknown][] = [];
  const usage: UsageSink = { record: (id, r) => void recorded.push([id, r]) };
  const h = codexHarness({}, { usage });
  h.runner.create({ text: 'x', accountId: 'codex-main' });
  const windows = [{ kind: 'weekly' as const, title: 'Weekly', remainingPercent: 78 }];
  h.codex.last.out({ kind: 'limits', windows, plan: 'Pro Lite' });
  assert.deepEqual(recorded, [['codex-main', { windows, plan: 'Pro Lite' }]]);
});

test('no wake grace on an adapter that never wakes after background work', () => {
  const h = codexHarness();
  const { thread } = h.runner.create({ text: 'x', accountId: 'codex-main' });
  const s = h.codex.last;
  s.init('019a-bg', 'gpt-6.1-sol');
  s.out({ kind: 'tasks', tasks: [{ id: 'item-1', type: 'shell', kind: 'shell', description: 'sleep 8; echo finished' }] });
  s.result();
  assert.equal(h.runner.get(thread.id).background?.length, 1);
  assert.equal(h.runner.hasIdleTimer(thread.id), false, 'background work holds the CLI');
  // The late result comes with no turn, and the list empties.
  s.out({ kind: 'event', event: { type: 'tool.result', itemId: 'item-1', ok: true, text: 'finished', truncated: false, images: [] } });
  s.out({ kind: 'tasks', tasks: [] });
  const t = h.runner.get(thread.id);
  assert.equal(t.background, undefined, 'nothing shown as just finished');
  assert.equal(t.status, 'idle');
  assert.deepEqual(h.runner.backgroundWork(), []);
  assert.equal(h.runner.hasIdleTimer(thread.id), true, 'the idle countdown starts again');
  assert.equal(h.events(thread.id).find((e) => e.type === 'tool.result').turnId, undefined);
});

// --------------------------------------------------------------- steering

test('a Codex-style steer: two messages answered by one CLI turn, inside one runner turn', () => {
  const h = codexHarness();
  const { thread, turnId } = h.runner.create({ text: 'first', accountId: 'codex-main' });
  const s = h.codex.last;
  s.init('019a-s', 'gpt-6.1-sol');
  const second = h.runner.send(thread.id, 'also this');
  assert.equal(second.turnId, turnId, 'it joins the running turn');
  const id2 = s.sent[1].id!;
  s.result('completed', undefined, undefined, { answers: turnId, answered: [turnId, id2], queuedTurns: 0 });
  const done = h.events(thread.id).filter((e) => e.type === 'turn.completed');
  assert.equal(done.length, 1);
  assert.equal(done[0].turnId, turnId);
  assert.ok(!notes(h, thread.id).some((n) => /dropped/.test(n)));
});

test('a refused Codex steer: held, counted in queuedTurns, answered by a second CLI turn in the same runner turn', () => {
  const h = codexHarness();
  const { thread, turnId } = h.runner.create({ text: 'first', accountId: 'codex-main' });
  const s = h.codex.last;
  s.init('019a-s2', 'gpt-6.1-sol');
  h.runner.send(thread.id, 'also this');
  const id2 = s.sent[1].id!;
  s.result('completed', undefined, undefined, { answers: turnId, answered: [turnId], queuedTurns: 1 });
  assert.equal(h.runner.get(thread.id).status, 'running', 'the runner turn goes on');
  assert.equal(h.events(thread.id).filter((e) => e.type === 'turn.completed').length, 0);
  s.text('m2', 'And this too.');
  s.result('completed', undefined, undefined, { answers: id2, answered: [id2], queuedTurns: 0 });
  const done = h.events(thread.id).filter((e) => e.type === 'turn.completed');
  assert.equal(done.length, 1);
  assert.equal(done[0].turnId, turnId);
  assert.equal(h.runner.get(thread.id).status, 'idle');
});

// --------------------------------------------------------- CLI update hooks

function live(h: ReturnType<typeof codexHarness>, version: string, nativeId: string) {
  const { thread } = h.runner.create({ text: 'x', accountId: 'codex-main' });
  const s = h.codex.last;
  s.init(nativeId, 'gpt-6.1-sol');
  s.out({ kind: 'cliVersion', version });
  return { thread, s };
}

test('releaseIdle releases only idle sessions on another version, never stopping work', async () => {
  const h = codexHarness();
  const idle = live(h, '0.164.0', 'a');
  idle.s.result();
  const current = live(h, '0.165.0', 'b');
  current.s.result();
  const bg = live(h, '0.164.0', 'c');
  bg.s.out({ kind: 'tasks', tasks: [{ id: 't', type: 'shell', kind: 'shell', description: 'npm run dev' }] });
  bg.s.result();
  const asking = live(h, '0.164.0', 'd');
  void asking.s.sink.requestApproval({ name: 'shell', title: 'touch b', input: {}, canAlwaysAllow: true, signal: new AbortController().signal });
  const queued = live(h, '0.164.0', 'e');
  h.runner.send(queued.thread.id, 'queued', [], { queue: true });
  const claude = h.runner.create({ text: 'x', accountId: 'claude-3' });
  h.adapter.last.init();
  h.adapter.last.result();
  h.runner.releaseIdle('codex', '0.165.0');
  await tick();
  assert.equal(idle.s.ended, true);
  assert.equal(idle.s.endedHard, false, 'the soft release');
  assert.equal(current.s.ended, false, 'already on the new version');
  assert.equal(bg.s.ended, false, 'background work survives');
  assert.equal(asking.s.ended, false, 'a pending approval survives');
  assert.equal(queued.s.ended, false, 'a turn with a queued message survives');
  assert.equal(h.adapter.last.ended, false, 'another provider is left alone');
  assert.equal(h.runner.isLive(claude.thread.id), true);
  // The background shell finishing lets that one go too, softly.
  bg.s.out({ kind: 'tasks', tasks: [] });
  await tick();
  assert.equal(bg.s.ended, true);
  assert.equal(bg.s.endedHard, false);
});

test('a session on an old version is released when its turn ends, and the next message resumes', async () => {
  const h = codexHarness();
  const { thread, s } = live(h, '0.164.0', 'old');
  h.runner.releaseIdle('codex', '0.165.0');
  assert.equal(s.ended, false, 'mid-turn');
  // A message sent meanwhile still goes into the running turn.
  h.runner.send(thread.id, 'into this turn');
  assert.equal(s.sent.at(-1)!.text, 'into this turn');
  s.result('completed', undefined, undefined, { answered: [s.sent[0].id!, s.sent[1].id!], queuedTurns: 0 });
  await tick();
  h.runner.send(thread.id, 'after the update');
  await tick();
  assert.equal(s.ended, true);
  assert.equal(s.endedHard, false);
  const s2 = h.codex.last;
  assert.notEqual(s2, s);
  assert.equal(s2.opts.resume, true);
  assert.equal(s2.sent[0].text, 'after the update');
  s2.out({ kind: 'cliVersion', version: '0.165.0' });
  s2.result();
  await tick();
  assert.equal(s2.ended, false, 'on the wanted version it stays');
});

test('versionsInUse and codexBinariesInUse report the live sessions and processes', () => {
  const h = codexHarness();
  const a = live(h, '0.164.0', 'a');
  const b = live(h, '0.164.0', 'b');
  const c = live(h, '0.165.0', 'c');
  assert.deepEqual(h.runner.versionsInUse('codex'), [
    { version: '0.164.0', threadIds: [a.thread.id, b.thread.id] },
    { version: '0.165.0', threadIds: [c.thread.id] },
  ]);
  assert.deepEqual(h.runner.versionsInUse('claude'), []);
  h.codex.binaries = ['/home/tools/codex/0.164.0/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex'];
  const hooks = h.runner.cliHooks(() => true);
  assert.deepEqual(hooks.codexBinariesInUse(), h.codex.binaries);
  assert.equal(hooks.restartPending(), true);
  assert.deepEqual(hooks.versionsInUse('codex').length, 2);
});

// ----------------------------------------------------- models and accounts

test('a login that appears later is noticed within 10 s and its models asked for', async () => {
  const h = codexHarness({}, {}, [...withCodex()].filter((a) => a.provider === 'claude'));
  const asked: string[] = [];
  h.codex.models = async (a) => (asked.push(a.id), [{ id: 'gpt-6-luna', model: 'gpt-6-luna', title: 'GPT-6-Luna' }]);
  await tick();
  const updates = () => h.notes.filter((n) => n.method === 'accounts.updated');
  assert.equal(updates().length, 0);
  h.fake.base = withCodex(null);
  h.clock.advance(10 * SEC);
  await tick();
  assert.deepEqual(asked, ['codex-main']);
  const last = updates().at(-1);
  assert.ok(last && last.method === 'accounts.updated');
  assert.ok(h.runner.hello().accounts.find((a) => a.id === 'codex-main')?.models?.length);
  // Nothing new on the next rescans.
  h.clock.advance(30 * SEC);
  await tick();
  assert.deepEqual(asked, ['codex-main']);
});

test('accounts.updated: the first at once, then at most every 5 s, ending on the latest list', () => {
  let onUsage: (ids: string[]) => void = () => {};
  const usage: UsageSink = { record() {}, onChange: (fn) => ((onUsage = fn), () => {}) };
  const h = codexHarness({}, { usage });
  const sent = () => h.notes.filter((n) => n.method === 'accounts.updated').map((n) => (n as any).params.accounts as Account[]);
  const setNote = (note: string) => {
    h.fake.base = h.fake.base.map((a) => (a.id === 'claude-3' ? { ...a, note } : a));
    onUsage(['claude-3']);
  };
  setNote('one');
  assert.equal(sent().length, 1);
  h.clock.advance(1 * SEC);
  setNote('two');
  h.clock.advance(1 * SEC);
  setNote('three');
  assert.equal(sent().length, 1, 'held');
  h.clock.advance(3 * SEC);
  assert.equal(sent().length, 2);
  assert.equal(sent()[1].find((a) => a.id === 'claude-3')?.note, 'three');
  h.clock.advance(20 * SEC);
  assert.equal(sent().length, 2, 'nothing more without a change');
  // A rescan finding the same list says nothing either.
  h.runner.rescanAccounts();
  assert.equal(sent().length, 2);
});

test('the default account is the one with the most room, worked out again as usage moves', () => {
  const room = (pct: number): Account['usage'] => ({ windows: [{ kind: 'five_hour', title: '5-hour', remainingPercent: pct }], readAt: '2026-10-26T12:00:00Z', source: 'live' });
  const base = withCodex().map((a) => ({ ...a, usage: room(a.id === 'claude-main' ? 80 : 40) }));
  const h = codexHarness({}, {}, base);
  writeRunnerJson(h.home, { defaults: { accountId: 'claude-3' } });
  assert.equal(h.runner.hello().defaults.accountId, 'claude-main');
  h.fake.base = base.map((a) => (a.id === 'claude-3' ? { ...a, usage: room(95) } : a));
  assert.equal(h.runner.hello().defaults.accountId, 'claude-3');
  // tb new, create_thread and schedules go through create with no account.
  const t = h.runner.create({ text: 'x' }).thread;
  assert.equal(t.accountId, 'claude-3');
  // A dev runner's keepAccount stays put.
  writeRunnerJson(h.home, { defaults: { accountId: 'claude-main', keepAccount: true } });
  assert.equal(h.runner.hello().defaults.accountId, 'claude-main');
  // A test can swap the picker.
  const seen: (string | undefined)[] = [];
  const h2 = codexHarness({}, { pickAccount: (_a, last) => (seen.push(last), 'codex-main') });
  assert.equal(h2.runner.hello().defaults.accountId, 'codex-main');
  assert.deepEqual(seen, ['claude-main']);
});

// ------------------------------------------------------- CLI statuses

function fakeClis(start: CliStatus[]) {
  let list = start;
  const fns: ((c: CliStatus[]) => void)[] = [];
  const clis: CliControl = {
    list: () => list,
    check: () => list,
    update: () => list,
    setPolicy: () => list,
    onChange: (fn) => (fns.push(fn), () => {}),
  };
  return { clis, emit: (next: CliStatus[]) => ((list = next), fns.forEach((f) => f(next))) };
}

const codexStatus = (v: string): CliStatus => ({ provider: 'codex', path: `/t/codex/${v}`, managedBy: 'threadbear', installedVersion: v, status: 'current', policy: 'auto', inUse: [] });

test('CLI statuses reach hello and clis.updated, and a Codex update asks for the models again', async () => {
  const h = codexHarness();
  const asked: string[] = [];
  h.codex.models = async (a) => (asked.push(a.id), []);
  await tick();
  asked.length = 0;
  // E works out what's installed after it's attached: not an update.
  const unknown: CliStatus = { ...codexStatus('0.164.0'), status: 'unknown' };
  delete unknown.installedVersion;
  const f = fakeClis([unknown]);
  h.runner.attachClis(f.clis);
  f.emit([codexStatus('0.164.0')]);
  await tick();
  assert.deepEqual(asked, []);
  assert.deepEqual(h.runner.hello().clis, [codexStatus('0.164.0')]);
  // A check that changed nothing installed: told, but no new probe.
  f.emit([{ ...codexStatus('0.164.0'), latestVersion: '0.165.0', status: 'behind' }]);
  await tick();
  assert.deepEqual(asked, []);
  f.emit([codexStatus('0.165.0')]);
  await tick();
  assert.deepEqual(asked, ['codex-main']);
  const told = h.notes.filter((n) => n.method === 'clis.updated');
  assert.equal(told.length, 3);
  assert.equal((told[2] as any).params.clis[0].installedVersion, '0.165.0');
  // A first copy arriving where Codex was missing asks too.
  const h2 = codexHarness();
  const asked2: string[] = [];
  h2.codex.models = async (a) => (asked2.push(a.id), []);
  await tick();
  asked2.length = 0;
  const g = fakeClis([{ ...unknown, status: 'missing' }]);
  h2.runner.attachClis(g.clis);
  g.emit([codexStatus('0.164.0')]);
  await tick();
  assert.deepEqual(asked2, ['codex-main']);
});

// ------------------------------------------------------- archive and delete

test('archiving or deleting a Codex thread archives it in Codex too', async () => {
  const h = codexHarness();
  const a = live(h, '0.164.0', '019a-archive');
  a.s.result();
  h.runner.update({ threadId: a.thread.id, archived: true });
  await tick();
  assert.deepEqual(h.codex.archived, [{ accountId: 'codex-main', nativeSessionId: '019a-archive' }]);
  // Archiving again isn't another call.
  h.runner.update({ threadId: a.thread.id, archived: true });
  await tick();
  assert.equal(h.codex.archived.length, 1);
  const d = live(h, '0.164.0', '019a-delete');
  d.s.result();
  h.runner.delete(d.thread.id);
  await tick();
  assert.deepEqual(h.codex.archived.at(-1), { accountId: 'codex-main', nativeSessionId: '019a-delete' });
  // A failure is only logged.
  h.codex.archive = async () => {
    throw new Error('codex is gone');
  };
  const e = live(h, '0.164.0', '019a-fails');
  e.s.result();
  h.runner.delete(e.thread.id);
  await tick();
  // Claude has no archive: nothing to do.
  const c = h.runner.create({ text: 'x', accountId: 'claude-3' }).thread;
  h.adapter.last.init();
  h.adapter.last.result();
  h.runner.delete(c.id);
  assert.equal(CODEX_ACCOUNT.id, 'codex-main');
});

test('sessions tell CliUpdates whether they got going, on the version new sessions start on', async () => {
  const h = codexHarness();
  const f = fakeClis([codexStatus('0.165.0')]);
  const told: [string, string | undefined, boolean][] = [];
  f.clis.sessionStarted = (p, v, ok) => void told.push([p, v, ok]);
  h.runner.attachClis(f.clis);
  // Died before reporting a version: a failed start on the installed one.
  const { thread } = h.runner.create({ text: 'x', accountId: 'codex-main' });
  h.codex.last.sink.exited(new Error('codex exited before initialize'));
  // Got going: reported once, with the version it runs.
  h.runner.send(thread.id, 'again');
  await tick();
  const s = h.codex.last;
  s.init('019a-ok', 'gpt-6.1-sol');
  s.out({ kind: 'cliVersion', version: '0.165.0' });
  s.out({ kind: 'cliVersion', version: '0.165.0' });
  s.result();
  // A missing session to resume isn't the CLI failing.
  h.clock.advance(31 * 60 * SEC);
  await tick();
  h.runner.send(thread.id, 'third');
  await tick();
  h.codex.last.sink.exited(new SessionNotFoundError());
  await tick();
  assert.deepEqual(told, [
    ['codex', '0.165.0', false],
    ['codex', '0.165.0', true],
  ]);
});

// ------------------------------------------------------- review fixes

test('a failed turn keeps its errorKind on turn.completed', () => {
  const h = codexHarness();
  const { thread } = h.runner.create({ text: 'x', accountId: 'codex-main' });
  const s = h.codex.last;
  s.init('019a-limit', 'gpt-6.1-sol');
  s.result('failed', undefined, "You've hit your usage limit.", { errorKind: 'usageLimit' });
  // A pinned thread's wait for its login adds a note after it.
  const done = h.events(thread.id).findLast((e: any) => e.type === 'turn.completed');
  assert.equal(h.events(thread.id).at(-1).type, 'system.note');
  assert.equal(done.outcome, 'failed');
  assert.equal(done.errorKind, 'usageLimit');
  // Claude's too, and only on a failure.
  const c = h.runner.create({ text: 'x', accountId: 'claude-3' }).thread;
  h.adapter.last.init();
  h.adapter.last.result('failed', 0.01, 'prompt is too long', { errorKind: 'contextWindow' });
  assert.equal(h.events(c.id).at(-1).errorKind, 'contextWindow');
  const ok = h.runner.create({ text: 'x', accountId: 'claude-3' }).thread;
  h.adapter.last.init();
  h.adapter.last.result('completed', 0.01, undefined, { errorKind: 'usageLimit' });
  assert.equal(h.events(ok.id).at(-1).errorKind, undefined);
});

test('with keepAccount, using another account once leaves the default where it is', () => {
  const h = codexHarness();
  writeRunnerJson(h.home, { defaults: { accountId: 'claude-3', keepAccount: true } });
  h.runner.create({ text: 'x', accountId: 'codex-main' });
  const json = JSON.parse(fs.readFileSync(path.join(h.home, 'runner.json'), 'utf8'));
  assert.equal(json.defaults.accountId, 'claude-3');
  const next = h.runner.create({ text: 'y' }).thread;
  assert.equal(next.accountId, 'claude-3');
  assert.equal(next.provider, 'claude');
  // Without it, the last one used is remembered as before.
  writeRunnerJson(h.home, { defaults: { accountId: 'claude-3' } });
  h.runner.create({ text: 'z', accountId: 'claude-main' });
  assert.equal(JSON.parse(fs.readFileSync(path.join(h.home, 'runner.json'), 'utf8')).defaults.accountId, 'claude-main');
});

test('archiving during a turn archives in Codex only once the live CLI has exited', async () => {
  const h = codexHarness();
  h.codex.slowExit = true;
  const { thread } = h.runner.create({ text: 'x', accountId: 'codex-main' });
  const s = h.codex.last;
  s.init('019a-busy', 'gpt-6.1-sol');
  h.runner.update({ threadId: thread.id, archived: true });
  await tick();
  assert.equal(s.interrupts, 1, 'the turn is stopped first');
  assert.deepEqual(h.codex.archived, [], 'not while the turn runs');
  s.result('interrupted');
  await tick();
  assert.equal(s.ended, true, 'released after the turn');
  assert.deepEqual(h.codex.archived, [], 'not while the CLI is still exiting');
  s.exit();
  await tick();
  assert.deepEqual(h.codex.archived, [{ accountId: 'codex-main', nativeSessionId: '019a-busy' }]);

  // A CLI that dies by itself counts as gone too.
  const b = h.runner.create({ text: 'x', accountId: 'codex-main' }).thread;
  const sb = h.codex.last;
  sb.init('019a-dies', 'gpt-6.1-sol');
  h.runner.update({ threadId: b.id, archived: true });
  await tick();
  sb.sink.exited(new Error('codex crashed'));
  await tick();
  assert.deepEqual(h.codex.archived.at(-1), { accountId: 'codex-main', nativeSessionId: '019a-dies' });

  // Brought back before the CLI exited: not archived in Codex.
  const c = h.runner.create({ text: 'x', accountId: 'codex-main' }).thread;
  const sc = h.codex.last;
  sc.init('019a-back', 'gpt-6.1-sol');
  h.runner.update({ threadId: c.id, archived: true });
  sc.result('interrupted');
  await tick();
  h.runner.update({ threadId: c.id, archived: false });
  sc.exit();
  await tick();
  const archived: { nativeSessionId: string }[] = h.codex.archived;
  assert.ok(!archived.some((a) => a.nativeSessionId === '019a-back'));
});

// ---------------------------------------------------------------- fast mode

test('fast mode: on a Claude thread it goes to the CLI at start and live, and a turn the CLI ran at standard speed says why, once', async () => {
  const h = codexHarness();
  const { thread } = h.runner.create({ text: 'hi', accountId: 'claude-3', fastMode: true });
  assert.equal(thread.fastMode, true);
  assert.deepEqual(checkSummary(thread), []);
  const s = h.adapter.last;
  assert.equal(s.opts.fastMode, true);
  s.init();
  assert.equal(h.runner.update({ threadId: thread.id, fastMode: false }).thread.fastMode, undefined);
  h.runner.update({ threadId: thread.id, fastMode: true });
  // The same again changes nothing.
  h.runner.update({ threadId: thread.id, fastMode: true });
  assert.deepEqual(s.fastModes, [false, true]);
  s.out({ kind: 'fastMode', state: 'off', reason: 'extra_usage_disabled' });
  s.result();
  s.out({ kind: 'fastMode', state: 'off', reason: 'extra_usage_disabled' });
  s.out({ kind: 'fastMode', state: 'on' });
  assert.deepEqual(notes(h, thread.id).filter((n) => /fast mode/i.test(n)), ["Fast mode isn't available (usage credits are off for this login), so this thread runs at standard speed."]);
  // Not asked for: nothing to say.
  const plain = h.runner.create({ text: 'hi', accountId: 'claude-3' }).thread;
  assert.equal(plain.fastMode, undefined);
  h.adapter.last.out({ kind: 'fastMode', state: 'off', reason: 'free' });
  assert.deepEqual(notes(h, plain.id).filter((n) => /fast mode/i.test(n)), []);
});

test("fast mode: a Codex thread never has it, a model that doesn't offer it drops it, and a move to Codex clears it", async () => {
  const h = codexHarness();
  h.adapter.models = async () => [
    { id: 'opus', model: 'claude-opus-5-5', title: 'Opus 5.5', fastMode: true },
    { id: 'sonnet', model: 'claude-sonnet-5-5', title: 'Sonnet 5.5' },
  ];
  await h.runner.probeModels();
  const codex = h.runner.create({ text: 'hi', accountId: 'codex-main', fastMode: true }).thread;
  assert.equal(codex.fastMode, undefined);
  assert.equal(h.codex.last.opts.fastMode, undefined);
  assert.equal(h.runner.update({ threadId: codex.id, fastMode: true }).thread.fastMode, undefined);
  const sonnet = h.runner.create({ text: 'hi', accountId: 'claude-3', model: 'sonnet', fastMode: true }).thread;
  assert.equal(sonnet.fastMode, undefined);
  assert.ok(notes(h, sonnet.id).includes("Sonnet 5.5 doesn't have fast mode, so this thread runs at standard speed."));
  const opus = h.runner.create({ text: 'hi', accountId: 'claude-3', model: 'opus', fastMode: true }).thread;
  assert.equal(opus.fastMode, true);
  const s = h.adapter.last;
  s.init();
  // Onto a model without it: off, and the live CLI told.
  assert.equal(h.runner.update({ threadId: opus.id, model: 'sonnet' }).thread.fastMode, undefined);
  assert.deepEqual(s.fastModes, [false]);
  h.runner.update({ threadId: opus.id, model: 'opus', fastMode: true });
  assert.equal(h.runner.get(opus.id).fastMode, true);
  assert.equal(h.runner.update({ threadId: opus.id, accountId: 'codex-main' }).thread.fastMode, undefined);
});
