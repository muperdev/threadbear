// The fake Convex checks what a real deployment checks (test/helpers/
// convex-validate.ts): a function's args and return value against its
// validators, and the rows it writes against convex/schema.ts. On 8 Nov a
// decision card's answer notes reached Convex in every test and were dropped
// on the real backend, because the schema's asks had no optionNotes and the
// fake took anything. With the field taken out of the schema, the test of
// those notes (runner-keys-backend.test.ts) now fails naming it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { v } from 'convex/values';
import { call, load, memoryBackend, THREAD_BASICS } from '../helpers/convex-memory.ts';
import { problem } from '../helpers/convex-validate.ts';

test('values are checked as Convex checks them', () => {
  const ask = v.object({ action: v.string(), options: v.optional(v.array(v.string())), suggested: v.optional(v.number()) });
  assert.equal(problem(ask, { action: 'Ship it?', options: ['Yes', 'No'] }), null);
  assert.match(problem(ask, { action: 'Ship it?', optionNotes: ['a'] })!, /optionNotes: not in the validator/);
  assert.match(problem(ask, { options: [] })!, /action: missing/);
  assert.match(problem(ask, { action: 'x', options: ['a', 2] })!, /options\[1\]: wants a string/);
  assert.match(problem(ask, { action: 'x', suggested: '1' })!, /suggested: wants a number/);
  // Undefined is a field left out, as Convex drops it.
  assert.equal(problem(ask, { action: 'x', suggested: undefined }), null);
  const kind = v.union(v.literal('send'), v.literal('create'));
  assert.equal(problem(kind, 'send'), null);
  assert.match(problem(kind, 'picture')!, /wants "(send|create)"/);
  assert.equal(problem(v.record(v.string(), v.number()), { a: 1 }), null);
  assert.match(problem(v.record(v.string(), v.number()), { a: 'x' })!, /a: wants a number/);
  assert.match(problem(v.int64(), 1)!, /wants an int64/);
  assert.equal(problem(v.id('commands'), 'commands:3'), null);
  assert.match(problem(v.id('commands'), 'threads:3')!, /wants an id of commands, has one of threads/);
  assert.equal(problem(v.id('_storage'), 'pic-a'), null, "a test's storage ids are free text");
  assert.equal(problem(v.null(), null), null);
});

test("call refuses args its function's validators don't allow, and a return value that doesn't fit", async () => {
  const b = memoryBackend();
  const fn = { args: { key: v.string(), n: v.number() }, returns: v.object({ ok: v.boolean() }), handler: async (_ctx: unknown, a: { n: number }) => (a.n > 0 ? { ok: true } : { ok: 'yes' }) };
  assert.deepEqual(await call(b.ctx, fn, { key: 'k', n: 1 }), { ok: true });
  await assert.rejects(call(b.ctx, fn, { key: 'k', n: 1, extra: true }), /args its validators don't allow, at extra/);
  await assert.rejects(call(b.ctx, fn, { key: 'k' }), /at n: missing/);
  await assert.rejects(call(b.ctx, fn, { key: 'k', n: -1 }), /return value its validator doesn't allow, at the return value\.ok/);
  // Nothing returned is null to Convex.
  assert.equal(await call(b.ctx, { args: {}, returns: v.null(), handler: async () => undefined }, {}), undefined);
});

test('a row a function writes has to fit its table; one a test seeds itself needn\'t', async () => {
  load('schema');
  const b = memoryBackend();
  // A test may seed a row from before a rule, missing what's required now.
  await b.ctx.db.insert('commands', { runnerId: 'r', kind: 'send', args: {}, state: 'pending' });
  const writes = (doc: Record<string, unknown>) => ({ args: {}, handler: async (ctx: { db: { insert: (t: string, d: unknown) => Promise<string> } }) => void (await ctx.db.insert('commands', doc)) });
  await call(b.ctx, writes({ runnerId: 'r', kind: 'send', args: {}, state: 'pending', createdAt: 1 }), {});
  await assert.rejects(call(b.ctx, writes({ runnerId: 'r', kind: 'send', args: {}, state: 'pending' }), {}), /a row in commands that convex\/schema\.ts doesn't allow, at createdAt: missing/);
  await assert.rejects(call(b.ctx, writes({ runnerId: 'r', kind: 'teleport', args: {}, state: 'pending', createdAt: 1 }), {}), /at kind: wants/);
  // A patch is checked as the row it leaves.
  const id = b.rows('commands').at(-1)!._id;
  const patches = (p: Record<string, unknown>) => ({ args: {}, handler: async (ctx: { db: { patch: (i: string, d: unknown) => Promise<void> } }) => ctx.db.patch(id, p) });
  await call(b.ctx, patches({ state: 'done' }), {});
  await assert.rejects(call(b.ctx, patches({ madeUp: 1 }), {}), /at madeUp: not in the validator/);
  assert.equal(b.rows('commands').at(-1)!.madeUp, undefined, 'a refused patch changes nothing');
});

test("the backend's real functions are checked end to end: a card field the schema lacks is refused", async () => {
  process.env.RUNNER_KEY = 'owner-key';
  delete process.env.RUNNER_KEY_NEXT;
  const sync = load('sync');
  const b = memoryBackend();
  const row = { ...THREAD_BASICS, threadId: 'T', runnerId: 'm5runner0001', title: 'T', view: 'inbox', needsYou: true };
  await call(b.ctx, sync.upsertThreads, { key: 'owner-key', threads: [{ ...row, asks: [{ action: 'Ship it?', options: ['Yes', 'No'], optionNotes: ['I land it', 'Nothing'] }] }] });
  await assert.rejects(call(b.ctx, sync.upsertThreads, { key: 'owner-key', threads: [{ ...row, asks: [{ action: 'Ship it?', fieldTheSchemaLacks: 1 }] }] }), /threads\[0\]\.asks\[0\]\.fieldTheSchemaLacks: not in the validator/);
});
