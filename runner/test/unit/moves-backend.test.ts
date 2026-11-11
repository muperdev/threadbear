// Moving a thread to another runner in the backend (convex/moves.ts,
// docs/protocol.md "Moving a thread to another runner"), against the
// in-memory database: who may take each step, and the switch handing the
// thread and everything under it to the new runner.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call, load, memoryBackend, outcome, THREAD_BASICS } from '../helpers/convex-memory.ts';

const OWNER = 'owner-key';
const BRUCE_KEY = 'tbr_bruce';
const CLOUD_KEY = 'tbr_cloud';
const M5 = 'm5runner0001';
const BRUCE = 'cb921b2447ea';
const CLOUD = 'befe8df8975c';
const SEAL = 'A'.repeat(43) + '=';
const SHA = 'a'.repeat(64);

const { sha256 } = load('lib') as { sha256: (s: string) => Promise<string> };
const sync = load('sync');
const moves = load('moves');

async function setup() {
  process.env.RUNNER_KEY = OWNER;
  delete process.env.RUNNER_KEY_NEXT;
  process.env.CONVEX_SITE_URL = 'https://site.example';
  const b = memoryBackend();
  await b.ctx.db.insert('runnerKeys', { runnerId: BRUCE, keyHash: await sha256(BRUCE_KEY), name: 'bruce', createdAt: 1 });
  await b.ctx.db.insert('runnerKeys', { runnerId: CLOUD, keyHash: await sha256(CLOUD_KEY), name: 'cloud', createdAt: 1 });
  for (const [runnerId, name] of [
    [M5, 'M5'],
    [BRUCE, 'bruce'],
    [CLOUD, 'cloud'],
  ] as const) {
    await b.ctx.db.insert('runners', { runnerId, name, version: '1', lastSeenAt: Date.now(), startedAt: new Date().toISOString(), draining: false, threadCount: 0 });
  }
  const fn = (f: unknown, args: Record<string, unknown>) => call(b.ctx, f as never, args);
  const as = (key: string) => ({
    start: (threadId: string, to: string, extra: Record<string, unknown> = {}) => fn(moves.startMove, { key, threadId, to, ...extra }),
    cancel: (moveId: string) => fn(moves.cancelMove, { key, moveId }),
    list: (runnerId: string) => fn(moves.moves, { key, runnerId }),
    info: (moveId: string, info: unknown = { cwd: '/x' }) => fn(moves.moveInfo, { key, moveId, info }),
    ready: (moveId: string, plan: unknown = { cwd: '/x' }, sealKey = SEAL) => fn(moves.moveReady, { key, moveId, plan, sealKey }),
    frozen: (moveId: string) => fn(moves.moveFrozen, { key, moveId }),
    chunkUrl: (moveId: string) => fn(moves.moveChunkUrl, { key, moveId }),
    packed: (moveId: string, chunks: string[]) => fn(moves.movePacked, { key, moveId, chunks, sha256: SHA, bytes: 1000 }),
    chunks: (moveId: string) => fn(moves.moveChunks, { key, moveId }),
    claim: (moveId: string) => fn(moves.moveClaim, { key, moveId }),
    done: (moveId: string) => fn(moves.moveDone, { key, moveId }),
    failed: (moveId: string, note = 'no room') => fn(moves.moveFailed, { key, moveId, note }),
    upsert: (...threads: Record<string, unknown>[]) => fn(sync.upsertThreads, { key, threads }),
    append: (threadId: string, text: string, seq = 1) => fn(sync.appendMessages, { key, messages: [{ threadId, seq, role: 'assistant', text, at: 'now' }] }),
  });
  // Runs whatever the switch scheduled (moves:rehomeRows), to the end.
  const rehome = async () => {
    for (let i = 0; i < 100; i++) {
      const next = b.scheduled.findIndex((s: any) => s.fn.endsWith('rehomeRows'));
      if (next < 0) return;
      const [job] = b.scheduled.splice(next, 1) as { args: Record<string, unknown> }[];
      await fn(moves.rehomeRows, job!.args);
    }
    throw new Error('rehomeRows never finished');
  };
  // A runner key's chunk upload through /runner/upload, as http.ts does it.
  const uploadChunk = async (url: string, id: string) => {
    const ticket = new URL(url).searchParams.get('ticket')!;
    b.file(id);
    return fn(sync.redeemUpload, { ticket, storageId: id, bytes: 9 * 1024 * 1024 });
  };
  return { ...b, rehome, uploadChunk, owner: as(OWNER), bruce: as(BRUCE_KEY), cloud: as(CLOUD_KEY) };
}

const thread = (threadId: string, runnerId: string, extra: Record<string, unknown> = {}) => ({ threadId, runnerId, ...THREAD_BASICS, title: 'T', view: 'inbox', needsYou: false, ...extra });
const moveOf = (s: Awaited<ReturnType<typeof setup>>, moveId: string) => s.rows('threadMoves').find((m) => m.moveId === moveId)!;

test('only the main Mac starts a move, to an online runner other than the thread’s, one at a time', async () => {
  const s = await setup();
  await s.owner.upsert(thread('T1', M5));
  assert.equal(await outcome(s.bruce.start('T1', BRUCE)), 'forbidden');
  assert.equal(await outcome(s.owner.start('T1', M5)), 'badArgs');
  assert.equal(await outcome(s.owner.start('NOPE', BRUCE)), 'notFound');
  assert.equal(await outcome(s.owner.start('T1', 'nobody')), 'notFound');
  const cloud = s.rows('runners').find((r) => r.runnerId === CLOUD)!;
  cloud.lastSeenAt = Date.now() - 10 * 60_000;
  assert.equal(await outcome(s.owner.start('T1', CLOUD)), 'conflict');
  assert.equal(await outcome(s.owner.start('T1', BRUCE, { cwd: 'a\nb' })), 'badArgs');
  const { moveId } = await s.owner.start('T1', BRUCE, { cwd: '/Users/mike/dev/x' });
  assert.deepEqual(
    { from: moveOf(s, moveId).from, to: moveOf(s, moveId).to, state: moveOf(s, moveId).state, cwd: moveOf(s, moveId).cwd },
    { from: M5, to: BRUCE, state: 'asking', cwd: '/Users/mike/dev/x' },
  );
  assert.equal(await outcome(s.owner.start('T1', BRUCE)), 'conflict');
});

test('the main Mac to Bruce: each step is its own runner’s, and the switch hands everything over', async () => {
  const s = await setup();
  await s.owner.upsert(thread('T1', M5, { share: { slug: 'slug1', access: 'link' } }));
  await s.owner.append('T1', 'hello', 1);
  await s.owner.append('T1', 'there', 2);
  await s.ctx.db.insert('threadIndex', { runnerId: M5, threadId: 'T1', title: 'T', cwd: '/x', accountId: 'a', provider: 'claude', status: 'idle', needsYou: false, unread: false, preview: '', createdAt: 'x', lastActivityAt: 'x' });
  await s.ctx.db.insert('pocketCards', { key: 'k1', title: 'Code', text: '1234', threadId: 'T1', updatedAt: 'x', runnerId: M5 });
  const img = s.file('img1');
  await s.ctx.db.insert('replyImages', { threadId: 'T1', seq: 2, path: 'a.png', storageId: img, mediaType: 'image/png' });

  const { moveId } = await s.owner.start('T1', BRUCE);
  // Bruce sees it; Cloud doesn't, and can't ask for Bruce's.
  assert.deepEqual((await s.bruce.list(BRUCE)).map((m: any) => [m.moveId, m.state]), [[moveId, 'asking']]);
  assert.deepEqual(await s.cloud.list(CLOUD), []);
  assert.equal(await outcome(s.cloud.list(BRUCE)), 'forbidden');

  // S describes it first; only S can, and only T says it's ready, with a real key and a small plan.
  assert.equal(await outcome(s.bruce.ready(moveId)), 'conflict');
  assert.equal(await outcome(s.bruce.info(moveId)), 'forbidden');
  assert.equal(await outcome(s.owner.info(moveId, ['x'])), 'badArgs');
  await s.owner.info(moveId);
  assert.equal(await outcome(s.cloud.ready(moveId)), 'forbidden');
  assert.equal(await outcome(s.bruce.ready(moveId, { cwd: '/x' }, 'short')), 'badArgs');
  assert.equal(await outcome(s.bruce.ready(moveId, { big: 'x'.repeat(20_000) })), 'badArgs');
  assert.equal(await outcome(s.bruce.frozen(moveId)), 'forbidden');
  await s.bruce.ready(moveId);
  await s.bruce.ready(moveId); // a retry is fine
  assert.equal(moveOf(s, moveId).state, 'ready');

  // S freezes and packs; T can't do S's steps or claim early.
  assert.equal(await outcome(s.bruce.frozen(moveId)), 'forbidden');
  assert.equal(await outcome(s.owner.chunkUrl(moveId)), 'conflict');
  await s.owner.frozen(moveId);
  assert.equal(await s.owner.chunkUrl(moveId), 'https://upload.example/');
  assert.equal(await outcome(s.bruce.claim(moveId)), 'conflict');
  const chunk = s.file('chunk1');
  assert.equal(await outcome(s.owner.packed(moveId, [chunk, chunk])), 'badArgs');
  assert.equal(await outcome(s.owner.packed(moveId, ['missing'])), 'badArgs');
  await s.owner.packed(moveId, [chunk]);
  assert.equal(moveOf(s, moveId).state, 'unpacking');
  assert.equal(await outcome(s.cloud.chunks(moveId)), 'forbidden');
  assert.deepEqual(await s.bruce.chunks(moveId), ['https://files.example/chunk1']);
  // Before the switch the thread is still M5's: Bruce can't write it.
  assert.equal(await outcome(s.bruce.upsert(thread('T1', BRUCE))), 'forbidden');

  await s.bruce.claim(moveId);
  await s.bruce.claim(moveId); // a retry is fine
  const row = s.rows('threads').find((t) => t.threadId === 'T1')!;
  assert.equal(row.runnerId, BRUCE);
  assert.equal(row.share.slug, 'slug1');
  assert.equal(s.rows('threadIndex').find((r) => r.runnerId === M5 && r.threadId === 'T1')!.deleted, true);
  assert.equal(s.rows('pocketCards').find((c) => c.key === 'k1')!.runnerId, BRUCE);
  await s.rehome();
  // Its history is Bruce's now, counted against Bruce, and so is its picture.
  assert.deepEqual(s.rows('messages').filter((m) => m.threadId === 'T1').map((m) => [m.text, m.runnerId]), [['hello', BRUCE], ['there', BRUCE]]);
  assert.ok(s.rows('runnerStorage').find((r) => r.runnerId === BRUCE)!.messageBytes > 0);
  assert.equal(s.rows('storageOwners').find((o) => o.storageId === img)!.runnerId, BRUCE);

  // Bruce writes it from now on, its share link and all; its next message goes on.
  assert.equal(await outcome(s.bruce.upsert(thread('T1', BRUCE, { share: { slug: 'slug1', access: 'link' } }))), 'ok');
  await s.bruce.append('T1', 'moved', 3);
  assert.deepEqual(s.rows('messages').filter((m) => m.threadId === 'T1').map((m) => m.text), ['hello', 'there', 'moved']);
  // A command for it goes to Bruce now, not M5.
  assert.equal(await outcome(call(s.ctx, load('relay').queueCommand as never, { key: OWNER, runnerId: M5, threadId: 'T1', kind: 'send', args: { text: 'hi' } })), 'forbidden');

  assert.equal(await outcome(s.bruce.failed(moveId)), 'conflict');
  await s.bruce.done(moveId);
  assert.equal(moveOf(s, moveId).state, 'done');
  assert.ok(s.deleted.includes(chunk), 'the chunk goes when the move ends');
  assert.equal(moveOf(s, moveId).chunks, undefined);
});

test('Bruce to the main Mac: chunks come through /runner/upload, charged with a move’s worth of room on top of Bruce’s picture allowance', async () => {
  const s = await setup();
  await s.bruce.upsert(thread('T2', BRUCE));
  await s.bruce.append('T2', 'from bruce', 1);
  const { moveId } = await s.owner.start('T2', M5);
  await s.bruce.info(moveId);
  await s.owner.ready(moveId);
  await s.bruce.frozen(moveId);
  // Bruce's stored allowance is spent: a picture would wait, a chunk has
  // a move's worth of room on top, and is charged while the move holds it.
  s.rows('runnerStorage').find((r) => r.runnerId === BRUCE)!.bytes = 150 * 1024 * 1024;
  const url = await s.bruce.chunkUrl(moveId);
  assert.match(url, /^https:\/\/site\.example\/runner\/upload\?ticket=[0-9a-f]{48}$/);
  assert.equal(await s.uploadChunk(url, 'c1'), true);
  assert.equal(s.rows('runnerStorage').find((r) => r.runnerId === BRUCE)!.bytes, 159 * 1024 * 1024);
  // Only its own uploads go on the move.
  const someoneElses = s.file('owners-file');
  assert.equal(await outcome(s.bruce.packed(moveId, [someoneElses])), 'forbidden');
  await s.bruce.packed(moveId, ['c1']);
  await s.owner.claim(moveId);
  await s.rehome();
  const row = s.rows('threads').find((t) => t.threadId === 'T2')!;
  assert.equal(row.runnerId, M5);
  // The owner's rows carry no runnerId, and Bruce's allowance has them back.
  assert.equal(s.rows('messages').find((m) => m.threadId === 'T2')!.runnerId, undefined);
  assert.equal(s.rows('runnerStorage').find((r) => r.runnerId === BRUCE)!.messageBytes, 0);
  // Bruce can't write it any more.
  assert.equal(await outcome(s.bruce.upsert(thread('T2', BRUCE))), 'forbidden');
  await s.bruce.append('T2', 'late', 2);
  assert.deepEqual(s.rows('messages').filter((m) => m.threadId === 'T2').map((m) => m.text), ['from bruce']);
  await s.owner.done(moveId);
  assert.ok(s.deleted.includes('c1'));
  assert.equal(s.rows('storageOwners').some((o) => o.storageId === 'c1'), false);
  assert.equal(s.rows('runnerStorage').find((r) => r.runnerId === BRUCE)!.bytes, 150 * 1024 * 1024, 'given back when the move ends');
});

// Security review, 10 Nov 2026: a move's uploads were stored with nothing
// charged and nothing naming the move, so a runner could take its 24
// tickets, upload 10 MB through each, fail the move before movePacked, and
// keep 240 MB that no cleanup ever reached; a ticket also still worked
// after the move ended.
test('a move’s uploads are charged, and all of them go when it fails, listed or not', async () => {
  const s = await setup();
  await s.bruce.upsert(thread('T3', BRUCE));
  const { moveId } = await s.owner.start('T3', M5);
  await s.bruce.info(moveId);
  await s.owner.ready(moveId);
  await s.bruce.frozen(moveId);
  const urls: string[] = [];
  for (let i = 0; i < 24; i++) urls.push(await s.bruce.chunkUrl(moveId));
  assert.equal(await outcome(s.bruce.chunkUrl(moveId)), 'tooMany');
  for (let i = 0; i < 23; i++) assert.equal(await s.uploadChunk(urls[i]!, `x${i}`), true);
  const charged = s.rows('runnerStorage').find((r) => r.runnerId === BRUCE)!.bytes;
  assert.equal(charged, 23 * 9 * 1024 * 1024);
  await s.bruce.failed(moveId);
  for (let i = 0; i < 23; i++) assert.ok(s.deleted.includes(`x${i}`), `x${i} deleted`);
  assert.equal(s.rows('storageOwners').filter((o) => o.runnerId === BRUCE).length, 0);
  assert.equal(s.rows('runnerStorage').find((r) => r.runnerId === BRUCE)!.bytes, 0);
  // The last ticket, from before it failed, stores nothing now.
  const ticket = new URL(urls[23]!).searchParams.get('ticket')!;
  assert.equal(await call(s.ctx, sync.uploadTicketOk as never, { ticket }), false);
  s.file('late');
  assert.equal(await call(s.ctx, sync.redeemUpload as never, { ticket, storageId: 'late', bytes: 1024 }), false);
  assert.equal(s.rows('storageOwners').some((o) => o.storageId === 'late'), false);
});

test('a move’s ticket stores nothing once the move is packed, and the move’s room has a limit', async () => {
  const s = await setup();
  await s.bruce.upsert(thread('T4', BRUCE));
  const { moveId } = await s.owner.start('T4', M5);
  await s.bruce.info(moveId);
  await s.owner.ready(moveId);
  await s.bruce.frozen(moveId);
  const [a, b, c] = [await s.bruce.chunkUrl(moveId), await s.bruce.chunkUrl(moveId), await s.bruce.chunkUrl(moveId)];
  // Already at its allowance and a move's room: this one waits.
  await s.ctx.db.insert('runnerStorage', { runnerId: BRUCE, bytes: (150 + 120) * 1024 * 1024 - 1024 * 1024 });
  assert.equal(await s.uploadChunk(a, 'full'), 'later');
  s.rows('runnerStorage').find((r) => r.runnerId === BRUCE)!.bytes = 0;
  assert.equal(await s.uploadChunk(b, 'p1'), true);
  await s.bruce.packed(moveId, ['p1']);
  assert.equal(await s.uploadChunk(c, 'after'), false);
  assert.equal(s.rows('storageOwners').some((o) => o.storageId === 'after'), false);
});

test('a move fails or is cancelled only before the switch, and a stuck one fails by itself', async () => {
  const s = await setup();
  await s.owner.upsert(thread('T3', M5), thread('T4', M5), thread('T5', M5));
  const a = (await s.owner.start('T3', BRUCE)).moveId;
  assert.equal(await outcome(s.cloud.failed(a)), 'forbidden');
  await s.bruce.failed(a, 'Bruce has no copy of mikecann/foo.');
  assert.equal(moveOf(s, a).state, 'failed');
  assert.equal(moveOf(s, a).note, 'Bruce has no copy of mikecann/foo.');
  await s.bruce.failed(a); // again is fine
  // A failed move doesn't stop the next.
  const b = (await s.owner.start('T3', BRUCE)).moveId;
  await s.owner.info(b);
  await s.bruce.ready(b);
  await s.owner.frozen(b);
  await s.owner.cancel(b);
  assert.equal(moveOf(s, b).state, 'cancelled');

  const c = (await s.owner.start('T4', BRUCE)).moveId;
  await s.owner.info(c);
  await s.bruce.ready(c);
  await s.owner.frozen(c);
  await s.owner.packed(c, [s.file('k')]);
  assert.equal(await outcome(s.owner.cancel(c)), 'conflict');
  await s.bruce.claim(c);
  assert.equal(await outcome(s.owner.failed(c)), 'conflict');

  // Nobody answered T5's, and another frozen for too long.
  const d = (await s.owner.start('T5', CLOUD)).moveId;
  moveOf(s, d).updatedAt -= 11 * 60_000;
  const e = (await s.owner.start('T3', BRUCE)).moveId;
  await s.owner.info(e);
  await s.bruce.ready(e);
  await s.owner.frozen(e);
  moveOf(s, e).frozenAt -= 31 * 60_000;
  await call(s.ctx, moves.expireMoves as never, {});
  assert.equal(moveOf(s, d).state, 'failed');
  assert.equal(moveOf(s, d).note, "cloud didn't answer.");
  assert.equal(moveOf(s, e).state, 'failed');
  assert.equal(moveOf(s, c).state, 'claimed', 'past the switch, the new runner finishes it');
});

test('the switch fails the move if the thread left its runner meanwhile', async () => {
  const s = await setup();
  await s.bruce.upsert(thread('T6', BRUCE));
  const { moveId } = await s.owner.start('T6', CLOUD);
  await s.bruce.info(moveId);
  await s.cloud.ready(moveId);
  await s.bruce.frozen(moveId);
  const url = await s.bruce.chunkUrl(moveId);
  await s.uploadChunk(url, 'c6');
  await s.bruce.packed(moveId, ['c6']);
  // Removed on Bruce meanwhile.
  await call(s.ctx, sync.removeThreads as never, { key: BRUCE_KEY, threadIds: ['T6'] });
  await s.cloud.claim(moveId);
  assert.equal(moveOf(s, moveId).state, 'failed');
  assert.ok(s.deleted.includes('c6'));
});

// Codex's review, 10 Nov 2026: the main Mac's own chunks go up as Convex
// uploads with no record, so a target that failed the move while they
// went up left them for good. Its runner now names them once it's over.
test('the main Mac drops its own chunks for a move that ended before it was packed', async () => {
  const s = await setup();
  await s.owner.upsert(thread('T5', M5));
  const { moveId } = await s.owner.start('T5', BRUCE);
  await s.owner.info(moveId);
  await s.bruce.ready(moveId);
  await s.owner.frozen(moveId);
  const mine = s.file('mine1');
  const drop = (key: string, chunks: string[]) => call(s.ctx, moves.moveDropChunks as never, { key, moveId, chunks });
  assert.equal(await outcome(drop(OWNER, [mine])), 'conflict', 'not while it goes');
  await s.bruce.failed(moveId, 'gone away');
  assert.equal(await outcome(s.owner.packed(moveId, [mine])), 'conflict');
  assert.equal(await outcome(drop(BRUCE_KEY, [mine])), 'forbidden');
  // A file something else uses, or one a runner key uploaded, stays.
  const used = s.file('used1');
  await s.ctx.db.insert('replyImages', { threadId: 'T5', seq: 1, path: 'a.png', storageId: used, mediaType: 'image/png' });
  const theirs = s.file('theirs1');
  await s.ctx.db.insert('storageOwners', { storageId: theirs, runnerId: BRUCE, at: 1, bytes: 1 });
  await drop(OWNER, [mine, used, theirs]);
  assert.ok(s.deleted.includes('mine1'));
  assert.ok(!s.deleted.includes('used1'));
  assert.ok(!s.deleted.includes('theirs1'));
});
