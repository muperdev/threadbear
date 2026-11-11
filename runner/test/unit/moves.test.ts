// A move from start to end (src/moves.ts), two runners' Moves against the
// backend's own functions in memory (convex/moves.ts), with fake hosts: who
// does what in each state, and what each side is left with when it works,
// when the thread is busy, and when it fails.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { call, load, memoryBackend, THREAD_BASICS } from '../helpers/convex-memory.ts';
import { Moves, type MoveRow, type MovesBackend, type MovesHost, type MovePlan, type ThreadMoving } from '../../src/moves.ts';
import type { PackEntry } from '../../src/move-pack.ts';
import { SecretStore } from '../../src/secrets.ts';

const OWNER = 'owner-key';
const BRUCE_KEY = 'tbr_bruce';
const M5 = 'm5runner0001';
const BRUCE = 'cb921b2447ea';

const { sha256 } = load('lib') as { sha256: (s: string) => Promise<string> };
const sync = load('sync');
const moves = load('moves');

async function world() {
  process.env.RUNNER_KEY = OWNER;
  process.env.CONVEX_SITE_URL = 'https://site.example';
  const b = memoryBackend();
  await b.ctx.db.insert('runnerKeys', { runnerId: BRUCE, keyHash: await sha256(BRUCE_KEY), name: 'bruce', createdAt: 1 });
  for (const [runnerId, name] of [
    [M5, 'M5'],
    [BRUCE, 'bruce'],
  ]) {
    await b.ctx.db.insert('runners', { runnerId, name, version: '1', lastSeenAt: Date.now(), startedAt: new Date().toISOString(), draining: false, threadCount: 0 });
  }
  const stored = new Map<string, Buffer>();
  const listeners: (() => Promise<void>)[] = [];
  let n = 0;
  const backendFor = (key: string, runnerId: string): MovesBackend => {
    const refresh = async () => {
      for (const l of listeners) await l();
    };
    return {
      watch(onMoves) {
        const l = async () => onMoves((await call(b.ctx, moves.moves, { key, runnerId })) as MoveRow[]);
        listeners.push(l);
        void l();
        return () => listeners.splice(listeners.indexOf(l), 1);
      },
      async call(fn, args) {
        const out = await call(b.ctx, moves[fn], { key, ...args });
        // Rehoming runs as the scheduler would, after the switch.
        for (let job; (job = b.scheduled.findIndex((s: any) => s.fn.endsWith('rehomeRows'))) >= 0; ) {
          const [j] = b.scheduled.splice(job, 1) as { args: Record<string, unknown> }[];
          await call(b.ctx, moves.rehomeRows, j!.args);
        }
        setImmediate(() => void refresh());
        return out;
      },
      async upload(url, bytes) {
        const id = `file${++n}`;
        b.file(id);
        stored.set(id, Buffer.from(bytes));
        if (url.includes('ticket=')) assert.equal(await call(b.ctx, sync.redeemUpload, { ticket: new URL(url).searchParams.get('ticket'), storageId: id, bytes: bytes.length }), true);
        return id;
      },
      async download(url) {
        return stored.get(url.split('/').pop()!)!;
      },
    };
  };
  return { b, backendFor };
}

const names: Record<string, string> = { [M5]: 'M5', [BRUCE]: 'bruce' };
const log = { info() {}, warn() {} };

type Source = { idle: boolean; present: boolean; putAway: number; seen: (ThreadMoving | undefined)[]; host: Partial<MovesHost> };
function source(threadId: string): Source {
  const seen: (ThreadMoving | undefined)[] = [];
  const h: Source = {
    idle: true,
    present: true,
    putAway: 0,
    seen,
    host: {
      async describe(id: string) {
        return id === threadId ? { title: 'T', cwd: '/home/mike/dev/x', home: '/home/mike', provider: 'claude', accountId: 'claude-1', permissionMode: 'full', worktree: false, schedules: 0 } : undefined;
      },
      idle: () => h.idle,
      setMoving: (_id: string, m: ThreadMoving | undefined) => void seen.push(m),
      async pack(): Promise<{ entries: PackEntry[]; leftBehind: string[] }> {
        return { entries: [{ path: 'thread/thread.json', data: Buffer.from('{"id":"T1"}') }], leftBehind: ['a dev server'] };
      },
      async putAway() {
        h.putAway++;
        h.present = false;
      },
      has: () => h.present,
    },
  };
  return h;
}

function target(plan: MovePlan | { problem: string }) {
  const secrets = new SecretStore(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-moves-')));
  const h = {
    placed: [] as PackEntry[],
    adopted: 0,
    discarded: 0,
    host: {
      plan: async () => plan,
      async place(_row: MoveRow, entries: PackEntry[]) {
        h.placed = entries;
      },
      async adopt() {
        h.adopted++;
      },
      async discard() {
        h.discarded++;
      },
      sealKey: () => secrets.publicKey(),
      open: (sealed: Buffer, info: string) => secrets.openBytes(sealed, info),
    } as Partial<MovesHost>,
  };
  return h;
}

async function until(what: () => boolean, label: string) {
  // By the clock, not a count of turns of the event loop: 500 turns is 30ms,
  // which a busy machine's file and crypto work can easily outlast.
  for (const deadline = Date.now() + 15_000; Date.now() < deadline; ) {
    if (what()) return;
    await new Promise((r) => setImmediate(r));
  }
  throw new Error(`never: ${label}`);
}

function runners(w: Awaited<ReturnType<typeof world>>, s: ReturnType<typeof source>, t: ReturnType<typeof target>, timers: (() => void)[] = []) {
  const common = { runnerName: (id: string) => names[id] ?? id };
  const ms = new Moves({ self: () => BRUCE, on: () => true, backend: () => w.backendFor(BRUCE_KEY, BRUCE), host: { ...common, ...s.host } as MovesHost, log, setTimer: (fn) => (timers.push(fn), { cancel() {} }) });
  const mt = new Moves({ self: () => M5, on: () => true, backend: () => w.backendFor(OWNER, M5), host: { ...common, ...t.host } as MovesHost, log, setTimer: (fn) => (timers.push(fn), { cancel() {} }) });
  ms.start();
  mt.start();
  return { ms, mt };
}

const moveRow = (w: Awaited<ReturnType<typeof world>>) => w.b.rows('threadMoves')[0]!;

test('a thread moves from Bruce to the M5, and each side ends as it should', async () => {
  const w = await world();
  await call(w.b.ctx, sync.upsertThreads, { key: BRUCE_KEY, threads: [{ threadId: 'T1', runnerId: BRUCE, ...THREAD_BASICS, title: 'T', view: 'inbox', needsYou: false }] });
  const s = source('T1');
  const t = target({ cwd: '/Users/mike/dev/x', accountId: 'claude-main', leftBehind: [] });
  const { ms } = runners(w, s, t);
  await call(w.b.ctx, moves.startMove, { key: OWNER, threadId: 'T1', to: M5 });
  await until(() => moveRow(w).state === 'done', 'the move is done');
  await until(() => s.putAway === 1, "Bruce's copy put away");
  assert.equal(t.adopted, 1);
  assert.deepEqual(
    t.placed.map((e) => e.path),
    ['thread/thread.json', 'move.json'],
  );
  assert.deepEqual(JSON.parse(t.placed[1]!.data.toString()).leftBehind, ['a dev server']);
  assert.equal(w.b.rows('threads').find((r) => r.threadId === 'T1')!.runnerId, M5);
  // Bruce showed it moving, to the M5, and stopped writing it after the switch.
  assert.ok(s.seen.some((m) => m?.toName === 'M5' && m.state === 'packing'));
  assert.equal(ms.gone('T1'), true);
  // The chunks went with the end of the move.
  assert.equal(moveRow(w).chunks, undefined);
});

test('a busy thread waits for its turn to end before it freezes', async () => {
  const w = await world();
  await call(w.b.ctx, sync.upsertThreads, { key: BRUCE_KEY, threads: [{ threadId: 'T2', runnerId: BRUCE, ...THREAD_BASICS, title: 'T', view: 'inbox', needsYou: false }] });
  const s = source('T2');
  s.idle = false;
  const t = target({ cwd: '/x', accountId: 'a', leftBehind: [] });
  const timers: (() => void)[] = [];
  runners(w, s, t, timers);
  await call(w.b.ctx, moves.startMove, { key: OWNER, threadId: 'T2', to: M5 });
  await until(() => moveRow(w).state === 'ready' && timers.length > 0, 'ready, waiting on the turn');
  await new Promise((r) => setTimeout(r, 20));
  assert.equal(moveRow(w).state, 'ready', 'not frozen while the turn runs');
  s.idle = true;
  timers.shift()!();
  await until(() => moveRow(w).state === 'done', 'done once the turn ended');
});

test("a move the new runner can't take fails, and the thread stays where it was", async () => {
  const w = await world();
  await call(w.b.ctx, sync.upsertThreads, { key: BRUCE_KEY, threads: [{ threadId: 'T3', runnerId: BRUCE, ...THREAD_BASICS, title: 'T', view: 'inbox', needsYou: false }] });
  const s = source('T3');
  const t = target({ problem: 'The M5 has no copy of mikecann/foo.' });
  runners(w, s, t);
  await call(w.b.ctx, moves.startMove, { key: OWNER, threadId: 'T3', to: M5 });
  await until(() => moveRow(w).state === 'failed', 'failed');
  assert.equal(moveRow(w).note, 'The M5 has no copy of mikecann/foo.');
  await until(() => s.seen.at(-1)?.note === 'The M5 has no copy of mikecann/foo.', 'Bruce shows why');
  assert.equal(s.putAway, 0);
  assert.equal(w.b.rows('threads').find((r) => r.threadId === 'T3')!.runnerId, BRUCE);
  await until(() => t.discarded === 1, 'the M5 cleans up');
});

test("a bundle that doesn't arrive whole fails before the switch", async () => {
  const w = await world();
  await call(w.b.ctx, sync.upsertThreads, { key: BRUCE_KEY, threads: [{ threadId: 'T4', runnerId: BRUCE, ...THREAD_BASICS, title: 'T', view: 'inbox', needsYou: false }] });
  const s = source('T4');
  const t = target({ cwd: '/x', accountId: 'a', leftBehind: [] });
  // The M5's download comes back damaged.
  const realFor = w.backendFor;
  w.backendFor = (key, runnerId) => {
    const real = realFor(key, runnerId);
    return runnerId === M5 ? { ...real, download: async (url) => Buffer.concat([await real.download(url), Buffer.from('x')]) } : real;
  };
  runners(w, s, t);
  await call(w.b.ctx, moves.startMove, { key: OWNER, threadId: 'T4', to: M5 });
  await until(() => moveRow(w).state === 'failed', 'failed');
  assert.match(moveRow(w).note, /didn't arrive whole/);
  assert.equal(w.b.rows('threads').find((r) => r.threadId === 'T4')!.runnerId, BRUCE);
  assert.equal(t.adopted, 0);
});

// Moved away and back (Codex's review, 10 Nov 2026): an old move's switch
// marked the thread gone for good, and its done, heard again after a
// restart, put the thread that had come back away again.
test("a thread moved away and back is this runner's again, and the old move's done leaves it alone", async () => {
  let putAway = 0;
  const host = { has: () => true, putAway: async () => void putAway++, setMoving() {}, runnerName: (id: string) => id, adopt: async () => {} } as unknown as MovesHost;
  const row = (moveId: string, from: string, to: string, state: MoveRow['state'], createdAt: number): MoveRow => ({ moveId, threadId: 'T1', from, to, state, createdAt, updatedAt: createdAt });
  const away = row('aaaaaaaa0001', M5, BRUCE, 'done', 1);
  const back = row('aaaaaaaa0002', BRUCE, M5, 'done', 2);
  const ms = new Moves({ self: () => M5, on: () => true, backend: () => undefined, host, log });
  ms.update([away, back]);
  await new Promise((r) => setImmediate(r));
  assert.equal(ms.gone('T1'), false);
  assert.equal(putAway, 0);
  // Only the move away: gone, and put away.
  const once = new Moves({ self: () => M5, on: () => true, backend: () => undefined, host, log });
  once.update([away]);
  await new Promise((r) => setImmediate(r));
  assert.equal(once.gone('T1'), true);
  assert.equal(putAway, 1);
});
