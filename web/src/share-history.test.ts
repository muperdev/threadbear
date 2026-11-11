import { test } from 'node:test';
import assert from 'node:assert/strict';
import { empty, firstSeq, hasMore, LOADED_MAX, nextBefore, OLDER_MAX_BYTES, OLDER_MAX_PICTURES, OLDER_MAX_ROWS, pictures, rowBytes, rows, takeLive, takeOlder, type History } from './share-history.ts';

type R = { seq: number };
type P = { seq: number; url: string };
const range = (a: number, b: number) => Array.from({ length: b - a + 1 }, (_, i) => a + i);
const page = (a: number, b: number, more: boolean, pics: number[] = []) => ({
  messages: range(a, b).map((seq) => ({ seq })),
  images: pics.map((seq) => ({ seq, url: `https://x/${seq}.png` })),
  hasMore: more,
});
const seqs = (h: History<R, P>) => rows(h).map((m) => m.seq);

// The live page never grows: a bigger one would be read again on every new
// row (37.6 MB of Convex I/O in a day on the phone, 8 Nov).
test('load earlier reads a chunk before the shown rows, never a bigger live page', () => {
  assert.equal(nextBefore(empty<R, P>()), undefined);
  assert.equal(nextBefore(takeLive(empty<R, P>(), page(601, 1000, true))), 601);
  assert.equal(nextBefore(takeLive(empty<R, P>(), page(1, 400, false))), undefined);
});

// Going back is never cut off by the cap: what the reader loads is room on
// top of it (Codex's review: a cap that stopped Load earlier left a long
// thread's start out of reach for good).
test('chunks go above the live page until the start', () => {
  let h = takeLive(empty<R, P>(), page(7101, 7500, true));
  h = takeOlder(h, page(6901, 7100, true, [6910]), 7101)!;
  assert.deepEqual(seqs(h), range(6901, 7500));
  assert.equal(nextBefore(h), 6901);
  h = takeOlder(h, page(1, 6900, false), 6901)!;
  assert.equal(rows(h).length, 7500);
  assert.equal(hasMore(h), false);
  assert.equal(nextBefore(h), undefined);
  assert.deepEqual(pictures(h).map((p) => p.seq), [6910]);
});

// After loading, rows sliding off the live page still only get the cap on
// top of what was loaded; the oldest go, and can be loaded again.
test('a stream after loading earlier keeps within what was loaded plus the cap', () => {
  let h = takeLive(empty<R, P>(), page(3001, 3400, true));
  h = takeOlder(h, page(2001, 3000, true), 3001)!;
  for (let i = 1; i <= 400; i++) h = takeLive(h, page(3001 + i * 10, 3400 + i * 10, true));
  assert.equal(h.older.length, 1000 + OLDER_MAX_ROWS);
  assert.equal(h.older[h.older.length - 1]!.seq, h.live!.messages[0]!.seq - 1);
  assert.equal(hasMore(h), true);
  assert.equal(nextBefore(h), h.older[0]!.seq);
});

// Codex's second review: rows the cap let go, read again (the phone loads
// by itself when Load earlier comes into view), earned more room each time,
// so a runner could grow it without anyone asking. They earn nothing now,
// and a chunk that brings nothing that stays ends loading.
test('rows let go and read again earn no room, and loading stops', () => {
  let h = takeLive(empty<R, P>(), page(1, 400, true));
  for (let i = 1; i <= 300; i++) h = takeLive(h, page(1 + i * 10, 400 + i * 10, true));
  assert.equal(h.older.length, OLDER_MAX_ROWS);
  const before = nextBefore(h)!;
  // The rows just below, which were let go, come back.
  h = takeOlder(h, page(before - 40, before - 1, true), before)!;
  assert.equal(h.older.length, OLDER_MAX_ROWS);
  assert.equal(hasMore(h), false);
  assert.equal(nextBefore(h), undefined);
});

// However far back the reader goes, what loading adds stops at LOADED_MAX
// times the caps.
test('loading earlier adds room only up to its ceiling', () => {
  const top = 100_000;
  let h = takeLive(empty<R, P>(), page(top - 399, top, true));
  for (let i = 0; i < 60 && nextBefore(h) !== undefined; i++) {
    const before = nextBefore(h)!;
    h = takeOlder(h, page(before - 1000, before - 1, true), before)!;
  }
  assert.ok(h.older.length <= OLDER_MAX_ROWS * (LOADED_MAX + 1), `kept ${h.older.length}`);
  assert.ok(h.older.length >= OLDER_MAX_ROWS * LOADED_MAX);
  assert.equal(hasMore(h), false);
});

// Daybreak, 10 Nov: a runner writing its own shared thread a few rows at a
// time, under the live page's size so the pages always overlap, and nobody
// pressing Load earlier. 251 pages of 400 rows of 8 KiB, 40 new each time,
// kept 10,400 rows (81 MiB); now `older` stays within its caps, and every
// update stays cheap.
test('a paced stream of big rows keeps older within its byte cap', () => {
  const text = 'x'.repeat(8192);
  const big = (a: number, b: number) => ({ messages: range(a, b).map((seq) => ({ seq, text })), images: [], hasMore: true });
  type B = { seq: number; text: string };
  let h = empty<B, P>();
  const start = process.cpuUsage();
  for (let i = 0; i <= 250; i++) h = takeLive(h, big(1 + i * 40, 400 + i * 40));
  const cpu = process.cpuUsage(start);
  const bytes = h.older.reduce((n, m) => n + rowBytes(m), 0);
  assert.ok(bytes <= OLDER_MAX_BYTES, `older holds ${bytes} bytes`);
  assert.ok(h.older.length < 1100, `older kept ${h.older.length} rows`);
  // Contiguous with the live page: the newest old rows are the ones kept.
  assert.equal(h.older[h.older.length - 1]!.seq, h.live!.messages[0]!.seq - 1);
  // What went can be loaded again.
  assert.equal(nextBefore(h), h.older[0]!.seq);
  assert.ok((cpu.user + cpu.system) / 1000 < 100, `took ${(cpu.user + cpu.system) / 1000} ms of CPU`);
});

test('a long stream of small rows keeps older within its row cap', () => {
  let h = empty<R, P>();
  for (let i = 0; i < 1000; i++) h = takeLive(h, page(1 + i * 10, 400 + i * 10, true));
  assert.equal(h.older.length, OLDER_MAX_ROWS);
  assert.equal(h.older[h.older.length - 1]!.seq, h.live!.messages[0]!.seq - 1);
});

// Pictures between rows (seqs no row has) count with the row before them,
// so a stream of pictures can't grow `older` past its bytes either.
test('pictures between rows count against the cap', () => {
  const url = `https://x/${'p'.repeat(100_000)}.png`;
  let h = empty<R, P>();
  for (let i = 0; i < 300; i++) {
    const a = 1 + i * 1000;
    h = takeLive(h, { messages: [{ seq: a }, { seq: a + 1000 }], images: [{ seq: a + 500, url: `${url}?${i}` }], hasMore: true });
  }
  const bytes = h.older.reduce((n, m) => n + rowBytes(m), 0) + h.olderImages.reduce((n, p) => n + rowBytes(p), 0);
  assert.ok(bytes <= OLDER_MAX_BYTES, `older holds ${bytes} bytes`);
  assert.ok(h.olderImages.length < 100);
  assert.ok(h.olderImages.every((p) => p.seq >= h.older[0]!.seq));
});

// Codex's review: a picture at a seq below the only row, with no older rows,
// was added to `older` again on every live answer (a title change is
// enough to send one), N copies after N answers.
test('a picture before every row is kept once however many answers come', () => {
  const one = { messages: [{ seq: 10 }], images: [{ seq: 1, url: 'https://x/1.png' }], hasMore: false };
  let h = empty<R, P>();
  for (let i = 0; i < 1000; i++) h = takeLive(h, one);
  assert.equal(h.olderImages.length, 1);
  assert.deepEqual(pictures(h).map((p) => p.seq), [1]);
});

test('pictures with no older rows are capped too', () => {
  let h = empty<R, P>();
  for (let i = 0; i < 3000; i++) h = takeLive(h, { messages: [{ seq: 1_000_000 }], images: [{ seq: i, url: `https://x/${i}.png` }], hasMore: false });
  assert.equal(h.older.length, 0);
  assert.ok(h.olderImages.length <= OLDER_MAX_PICTURES, `kept ${h.olderImages.length}`);
  // The newest are the ones kept.
  assert.equal(Math.max(...h.olderImages.map((p) => p.seq)), 2998);
});

test('rows that slide off the live page stay, before and after a chunk', () => {
  let h = takeLive(empty<R, P>(), page(601, 1000, true, [601]));
  h = takeLive(h, page(603, 1002, true));
  assert.equal(firstSeq(h), 601);
  assert.deepEqual(seqs(h), range(601, 1002));
  h = takeOlder(h, page(401, 600, true), 601)!;
  h = takeLive(h, page(604, 1003, true));
  assert.deepEqual(seqs(h), range(401, 1003));
  assert.deepEqual(pictures(h).map((p) => p.seq), [601]);
});

test('no overlap with the answer before starts again from the newest', () => {
  let h = takeLive(empty<R, P>(), page(601, 1000, true));
  h = takeOlder(h, page(401, 600, true), 601)!;
  h = takeLive(h, page(1101, 1500, true));
  assert.deepEqual(seqs(h), range(1101, 1500));
  assert.equal(nextBefore(h), 1101);
});

test('a live page reaching further back takes its rows back from the chunks', () => {
  let h = takeLive(empty<R, P>(), page(601, 1000, true, [605]));
  h = takeLive(h, page(611, 1010, true));
  h = takeLive(h, page(590, 1010, true, [605]));
  assert.deepEqual(seqs(h), range(590, 1010));
  assert.deepEqual(pictures(h).map((p) => p.seq), [605]);
});

test('a chunk that comes too late is dropped', () => {
  let h = takeLive(empty<R, P>(), page(601, 1000, true));
  h = takeOlder(h, page(401, 600, true), 601)!;
  assert.equal(takeOlder(h, page(401, 600, true), 601), undefined);
});
