// Moving a thread to another runner (docs/protocol.md, "Moving a thread to
// another runner", API 64). The main Mac's runner (the owner key) starts a
// move; after that each runner watches the moves it's in (`moves`) and
// takes its step when the state says it's its turn: the runner that has the
// thread (S) describes it (moveInfo), the runner taking it (T) says it can
// (moveReady), the runner that has it (S) freezes
// it, packs it and uploads it sealed to T's key (moveFrozen, moveChunkUrl,
// movePacked), and T downloads, unpacks and takes it (moveChunks,
// moveClaim, moveDone). moveClaim is the one mutation where the thread
// changes hands, so at any moment one runner has it.
//
// Runners never talk to each other, and nothing here can read the bundle:
// it's sealed on S to the key T put on the move.
import { ConvexError, v } from 'convex/values';
import { internalMutation, mutation, query, type MutationCtx, type QueryCtx } from './_generated/server';
import type { Doc, Id } from './_generated/dataModel';
import { internal } from './_generated/api';
import { checkRunnerId, forbidden, hasLiveKey, runnerAuth, sha256, withoutCredentials } from './lib';
import { buryIndexRows, countBodyBytes, countMessageBytes, MAX_RUNNER_UPLOAD_BYTES, moveView, storageInUse, storedBytes } from './sync';
import { docBytes, type Caller } from './runnerAccess';

// A runner heard from this recently is online (as phone.ts has it).
const ONLINE_MS = 3 * 60_000;
// A move T hasn't answered fails after this; one frozen and not yet claimed
// after MOVE_FROZEN_MS (expireMoves). Ended moves are listed for a day and
// kept a week.
export const MOVE_ASK_MS = 10 * 60_000;
export const MOVE_FROZEN_MS = 30 * 60_000;
export const MOVE_LISTED_MS = 24 * 60 * 60_000;
const MOVE_KEPT_MS = 7 * 24 * 60 * 60_000;
// 12 chunks of at most 10 MB: the 100 MB a bundle may be, with room.
export const MAX_MOVE_CHUNKS = 12;
const MOVE_TICKETS = MAX_MOVE_CHUNKS * 2;
const PLAN_MAX_BYTES = 16 * 1024;
const NOTE_CHARS = 500;
const PICK_CHARS = 1024;
const LISTED = 50;
// Rows handed over a run at a time (rehomeRows), as a thread can have more
// than one mutation may read.
const REHOME_PAGE = 200;

type Move = Doc<'threadMoves'>;
type MoveState = Move['state'];
const GOING: readonly MoveState[] = ['asking', 'ready', 'packing', 'unpacking', 'claimed'];
const BEFORE_CLAIM: readonly MoveState[] = ['asking', 'ready', 'packing', 'unpacking'];

const bad = (message: string) => new ConvexError({ kind: 'badArgs', message });
const conflict = (message: string) => new ConvexError({ kind: 'conflict', message });

async function moveRow(ctx: QueryCtx | MutationCtx, moveId: string): Promise<Move> {
  const m = await ctx.db
    .query('threadMoves')
    .withIndex('by_moveId', (q) => q.eq('moveId', moveId))
    .unique();
  if (!m) throw new ConvexError({ kind: 'notFound', message: 'no such move' });
  return m;
}

// The caller is the move's runner on that side: a runner key its own, the
// owner key one with no key of its own (lib.ts actAs's rule).
async function mustBe(ctx: QueryCtx | MutationCtx, caller: Caller, m: Move, side: 'from' | 'to' | 'either'): Promise<void> {
  const ids = side === 'either' ? [m.from, m.to] : [m[side]];
  if (!caller.owner) {
    if (!ids.includes(caller.runnerId)) throw forbidden(`act on a move ${side === 'from' ? 'from' : side === 'to' ? 'to' : 'between'} other runners`);
    return;
  }
  for (const id of ids) if (!(await hasLiveKey(ctx, id))) return;
  throw forbidden('act on a move between runners with keys of their own with the main key');
}

function mustBeIn(m: Move, states: readonly MoveState[], what: string): void {
  if (!states.includes(m.state)) throw conflict(`The move is ${m.state}, so it can't ${what}.`);
}

function randomId(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

async function runnerName(ctx: QueryCtx | MutationCtx, runnerId: string): Promise<string> {
  return (await ctx.db.query('runners').withIndex('by_runnerId', (q) => q.eq('runnerId', runnerId)).unique())?.name ?? runnerId;
}

// The chunks go whichever way a move ends: nothing else names them. Every
// upload made for the move (sync.ts redeemUpload names each with it), not
// just those movePacked listed, and each runner gets its bytes back.
async function dropChunks(ctx: MutationCtx, m: Move): Promise<void> {
  const ids = new Set<Id<'_storage'>>(m.chunks ?? []);
  for (const row of await ctx.db.query('storageOwners').withIndex('by_move', (q) => q.eq('moveId', m.moveId)).take(MOVE_TICKETS * 2)) ids.add(row.storageId);
  for (const id of ids) {
    if (await ctx.db.system.get(id)) await ctx.storage.delete(id);
    for (const row of await ctx.db.query('storageOwners').withIndex('by_storageId', (q) => q.eq('storageId', id)).collect()) {
      await ctx.db.delete(row._id);
      if (!row.bytes) continue;
      const usage = await ctx.db.query('runnerStorage').withIndex('by_runnerId', (q) => q.eq('runnerId', row.runnerId)).unique();
      if (usage) await ctx.db.patch(usage._id, { bytes: Math.max(0, usage.bytes - row.bytes) });
    }
  }
}

async function end(ctx: MutationCtx, m: Move, state: 'done' | 'failed' | 'cancelled', note?: string): Promise<void> {
  await dropChunks(ctx, m);
  await ctx.db.patch(m._id, { state, chunks: undefined, updatedAt: Date.now(), ...(note ? { note: note.slice(0, NOTE_CHARS) } : {}) });
}

function cleanPick(value: string | undefined, what: string): string | undefined {
  if (value === undefined) return undefined;
  if (!value.trim() || value.length > PICK_CHARS || /[\u0000-\u001f]/.test(value)) throw bad(`${what} must be 1 to ${PICK_CHARS} characters, with no control characters`);
  return value;
}

const listedMove = v.object({
  moveId: v.string(),
  threadId: v.string(),
  from: v.string(),
  to: v.string(),
  state: v.string(),
  cwd: v.optional(v.string()),
  accountId: v.optional(v.string()),
  info: v.optional(v.any()),
  plan: v.optional(v.any()),
  key: v.optional(v.string()),
  sha256: v.optional(v.string()),
  bytes: v.optional(v.number()),
  chunks: v.optional(v.number()),
  note: v.optional(v.string()),
  createdAt: v.number(),
  updatedAt: v.number(),
  frozenAt: v.optional(v.number()),
});

function listed(m: Move) {
  const { _id, _creationTime, chunks, ...rest } = m;
  return { ...rest, ...(chunks ? { chunks: chunks.length } : {}) };
}

/** Starts a move: the main Mac's runner, for Mike or one of its agents. */
export const startMove = mutation({
  args: { key: v.string(), threadId: v.string(), to: v.string(), cwd: v.optional(v.string()), accountId: v.optional(v.string()) },
  returns: v.object({ moveId: v.string() }),
  handler: async (ctx, { key, ...a }) => {
    await runnerAuth(ctx, key, 'startMove');
    checkRunnerId(a.to);
    const cwd = cleanPick(a.cwd, 'cwd');
    const accountId = cleanPick(a.accountId, 'accountId');
    const t = await ctx.db
      .query('threads')
      .withIndex('by_threadId', (q) => q.eq('threadId', a.threadId))
      .unique();
    if (!t) throw new ConvexError({ kind: 'notFound', message: 'no such thread' });
    if (t.runnerId === a.to) throw bad('The thread is on that runner already.');
    const r = await ctx.db
      .query('runners')
      .withIndex('by_runnerId', (q) => q.eq('runnerId', a.to))
      .unique();
    if (!r) throw new ConvexError({ kind: 'notFound', message: 'no such runner' });
    const now = Date.now();
    if (r.draining || now - r.lastSeenAt > ONLINE_MS) throw conflict(`${r.name} is offline.`);
    for (const m of await ctx.db.query('threadMoves').withIndex('by_thread', (q) => q.eq('threadId', a.threadId)).collect()) {
      if (GOING.includes(m.state)) throw conflict('This thread is already moving.');
    }
    const moveId = randomId();
    await ctx.db.insert('threadMoves', { moveId, threadId: a.threadId, from: t.runnerId, to: a.to, state: 'asking', ...(cwd ? { cwd } : {}), ...(accountId ? { accountId } : {}), createdAt: now, updatedAt: now });
    return { moveId };
  },
});

/** Mike changed his mind, before anything was unpacked. */
export const cancelMove = mutation({
  args: { key: v.string(), moveId: v.string() },
  returns: v.null(),
  handler: async (ctx, { key, moveId }) => {
    await runnerAuth(ctx, key, 'cancelMove');
    const m = await moveRow(ctx, moveId);
    if (m.state === 'cancelled') return null;
    mustBeIn(m, ['asking', 'ready', 'packing'], 'be cancelled');
    await end(ctx, m, 'cancelled', 'Cancelled.');
    return null;
  },
});

/** The moves a runner is in: still going, or ended in the last day. */
export const moves = query({
  args: { key: v.string(), runnerId: v.string() },
  returns: v.array(listedMove),
  handler: async (ctx, { key, runnerId }) => {
    const caller = await runnerAuth(ctx, key, 'moves');
    checkRunnerId(runnerId);
    if (!caller.owner && caller.runnerId !== runnerId) throw forbidden(`act as runner ${runnerId}`);
    const since = Date.now() - MOVE_LISTED_MS;
    const out = new Map<string, Move>();
    const fromHere = await ctx.db
      .query('threadMoves')
      .withIndex('by_from', (q) => q.eq('from', runnerId).gte('updatedAt', since))
      .order('desc')
      .take(LISTED);
    const toHere = await ctx.db
      .query('threadMoves')
      .withIndex('by_to', (q) => q.eq('to', runnerId).gte('updatedAt', since))
      .order('desc')
      .take(LISTED);
    for (const m of [...fromHere, ...toHere]) out.set(m.moveId, m);
    return [...out.values()].sort((a, b) => a.createdAt - b.createdAt).map(listed);
  },
});

function smallObject(x: unknown, what: string): void {
  if (x === null || typeof x !== 'object' || Array.isArray(x) || docBytes(x) > PLAN_MAX_BYTES) throw bad(`${what} must be an object of at most ${PLAN_MAX_BYTES} bytes`);
}

/** S describes the thread (its folder, repo, login), for T to plan from. */
export const moveInfo = mutation({
  args: { key: v.string(), moveId: v.string(), info: v.any() },
  returns: v.null(),
  handler: async (ctx, { key, moveId, info }) => {
    const caller = await runnerAuth(ctx, key, 'moveInfo');
    const m = await moveRow(ctx, moveId);
    await mustBe(ctx, caller, m, 'from');
    mustBeIn(m, ['asking'], 'be described');
    if (m.info !== undefined) return null;
    smallObject(info, 'info');
    // Which repo, never a password or token in its address: T reads this
    // (an older runner sends it whole).
    const o = info as Record<string, unknown>;
    const safe = typeof o.origin === 'string' ? { ...o, origin: withoutCredentials(o.origin) } : info;
    await ctx.db.patch(m._id, { info: safe, updatedAt: Date.now() });
    return null;
  },
});

const SEAL_KEY = /^[A-Za-z0-9+/]{43}=$/;

/** T can take the thread: where and on what (`plan`), and the key to seal it to. */
export const moveReady = mutation({
  args: { key: v.string(), moveId: v.string(), plan: v.any(), sealKey: v.string() },
  returns: v.null(),
  handler: async (ctx, { key, moveId, plan, sealKey }) => {
    const caller = await runnerAuth(ctx, key, 'moveReady');
    const m = await moveRow(ctx, moveId);
    await mustBe(ctx, caller, m, 'to');
    if (m.state === 'ready' && m.key === sealKey) return null;
    mustBeIn(m, ['asking'], 'be made ready');
    if (!SEAL_KEY.test(sealKey)) throw bad('sealKey must be a raw X25519 key in base64');
    if (m.info === undefined) throw conflict("The move can't be made ready before its thread is described.");
    smallObject(plan, 'plan');
    await ctx.db.patch(m._id, { state: 'ready', plan, key: sealKey, updatedAt: Date.now() });
    return null;
  },
});

/** S's thread has finished its turn and is frozen: packing starts. */
export const moveFrozen = mutation({
  args: { key: v.string(), moveId: v.string() },
  returns: v.null(),
  handler: async (ctx, { key, moveId }) => {
    const caller = await runnerAuth(ctx, key, 'moveFrozen');
    const m = await moveRow(ctx, moveId);
    await mustBe(ctx, caller, m, 'from');
    if (m.state === 'packing') return null;
    mustBeIn(m, ['ready'], 'start packing');
    const now = Date.now();
    await ctx.db.patch(m._id, { state: 'packing', frozenAt: now, updatedAt: now });
    return null;
  },
});

/**
 * Where S uploads one chunk. A runner key's goes through /runner/upload
 * with a ticket for this move, outside its picture allowances (the move
 * bounds it, and its chunks go when it ends); the owner's is Convex's own.
 */
export const moveChunkUrl = mutation({
  args: { key: v.string(), moveId: v.string() },
  returns: v.string(),
  handler: async (ctx, { key, moveId }) => {
    const caller = await runnerAuth(ctx, key, 'moveChunkUrl');
    const m = await moveRow(ctx, moveId);
    await mustBe(ctx, caller, m, 'from');
    mustBeIn(m, ['packing'], 'take chunks');
    if (caller.owner) return ctx.storage.generateUploadUrl();
    const tickets = await ctx.db.query('uploadTickets').withIndex('by_move', (q) => q.eq('moveId', moveId)).take(MOVE_TICKETS + 1);
    if (tickets.length >= MOVE_TICKETS) throw new ConvexError({ kind: 'tooMany', message: `A move may have at most ${MOVE_TICKETS} upload URLs` });
    const site = process.env.CONVEX_SITE_URL;
    if (!site) throw new ConvexError({ kind: 'config', message: 'CONVEX_SITE_URL is not set' });
    const ticket = randomId() + randomId().slice(0, 16);
    await ctx.db.insert('uploadTickets', { ticketHash: await sha256(ticket), runnerId: caller.runnerId, ...(caller.keyId ? { keyId: caller.keyId as Id<'runnerKeys'> } : {}), issuedAt: Date.now(), moveId });
    return `${site.replace(/\/+$/, '')}/runner/upload?ticket=${ticket}`;
  },
});

/** S has uploaded the sealed bundle: T's turn to unpack it. */
export const movePacked = mutation({
  args: { key: v.string(), moveId: v.string(), chunks: v.array(v.id('_storage')), sha256: v.string(), bytes: v.number() },
  returns: v.null(),
  handler: async (ctx, { key, ...a }) => {
    const caller = await runnerAuth(ctx, key, 'movePacked');
    const m = await moveRow(ctx, a.moveId);
    await mustBe(ctx, caller, m, 'from');
    if (m.state === 'unpacking' && m.sha256 === a.sha256) return null;
    mustBeIn(m, ['packing'], 'be packed');
    if (a.chunks.length < 1 || a.chunks.length > MAX_MOVE_CHUNKS) throw bad(`a move has 1 to ${MAX_MOVE_CHUNKS} chunks`);
    if (new Set(a.chunks).size !== a.chunks.length) throw bad('a chunk is named twice');
    if (!/^[0-9a-f]{64}$/.test(a.sha256)) throw bad('sha256 must be 64 hex characters');
    if (!Number.isSafeInteger(a.bytes) || a.bytes < 1 || a.bytes > MAX_MOVE_CHUNKS * MAX_RUNNER_UPLOAD_BYTES) throw bad('bytes is out of range');
    for (const id of a.chunks) {
      if (!(await ctx.db.system.get(id))) throw bad('a chunk is not stored');
      if (!caller.owner) {
        const owners = await ctx.db.query('storageOwners').withIndex('by_storageId', (q) => q.eq('storageId', id)).collect();
        if (!owners.some((o) => o.runnerId === caller.runnerId)) throw forbidden("use a stored file its runner didn't upload");
      }
    }
    await ctx.db.patch(m._id, { state: 'unpacking', chunks: a.chunks, sha256: a.sha256, bytes: a.bytes, updatedAt: Date.now() });
    return null;
  },
});

/**
 * The main Mac's uploads for a move that ended before movePacked named
 * them (the other runner failed it while they went up): its own uploads
 * are Convex's, with no record dropChunks could find, so its runner names
 * them. Only once the move has ended, only files no move still going
 * names, and nothing else uses.
 */
export const moveDropChunks = mutation({
  args: { key: v.string(), moveId: v.string(), chunks: v.array(v.id('_storage')) },
  returns: v.null(),
  handler: async (ctx, { key, moveId, chunks }) => {
    await runnerAuth(ctx, key, 'moveDropChunks');
    const m = await moveRow(ctx, moveId);
    if (GOING.includes(m.state)) throw conflict(`The move is ${m.state}, so its chunks stay.`);
    if (chunks.length > MOVE_TICKETS) throw bad(`at most ${MOVE_TICKETS} chunks`);
    for (const id of chunks) {
      if (!(await ctx.db.system.get(id))) continue;
      if (await ctx.db.query('storageOwners').withIndex('by_storageId', (q) => q.eq('storageId', id)).first()) continue;
      if (await storageInUse(ctx, id)) continue;
      let named = false;
      for (const state of GOING) {
        for (const other of await ctx.db.query('threadMoves').withIndex('by_state', (q) => q.eq('state', state)).take(100)) if (other.chunks?.includes(id)) named = true;
      }
      if (!named) await ctx.storage.delete(id);
    }
    return null;
  },
});

/** Where T downloads the chunks from, in order. */
export const moveChunks = query({
  args: { key: v.string(), moveId: v.string() },
  returns: v.array(v.string()),
  handler: async (ctx, { key, moveId }) => {
    const caller = await runnerAuth(ctx, key, 'moveChunks');
    const m = await moveRow(ctx, moveId);
    await mustBe(ctx, caller, m, 'to');
    mustBeIn(m, ['unpacking', 'claimed'], 'be downloaded');
    const urls: string[] = [];
    for (const id of m.chunks ?? []) {
      const url = await ctx.storage.getUrl(id);
      if (!url) throw conflict('A chunk has gone.');
      urls.push(url);
    }
    return urls;
  },
});

/**
 * The switch: from here the thread is T's. Its row, its Pocket cards and
 * its share link go to T, S's sidebar row for it goes, and its mirrored
 * rows follow in runs of their own (rehomeRows), so its history stays on
 * the phone. Commands for it go to T from now on, and S's writes under
 * the id are refused.
 */
export const moveClaim = mutation({
  args: { key: v.string(), moveId: v.string() },
  returns: v.null(),
  handler: async (ctx, { key, moveId }) => {
    const caller = await runnerAuth(ctx, key, 'moveClaim');
    const m = await moveRow(ctx, moveId);
    await mustBe(ctx, caller, m, 'to');
    if (m.state === 'claimed') return null;
    mustBeIn(m, ['unpacking'], 'be claimed');
    const t = await ctx.db
      .query('threads')
      .withIndex('by_threadId', (q) => q.eq('threadId', m.threadId))
      .unique();
    if (!t || t.runnerId !== m.from) {
      await end(ctx, m, 'failed', `The thread isn't on ${await runnerName(ctx, m.from)} any more.`);
      return null;
    }
    const now = Date.now();
    await ctx.db.patch(t._id, { runnerId: m.to, pendingApproval: undefined, updatedMs: now });
    await moveView(ctx, m.from, t.view, undefined);
    await moveView(ctx, m.to, undefined, t.view);
    await buryIndexRows(ctx, m.threadId, m.to);
    for (const c of await ctx.db.query('pocketCards').withIndex('by_runnerId', (q) => q.eq('runnerId', m.from)).collect()) {
      if (c.threadId === m.threadId) await ctx.db.patch(c._id, { runnerId: m.to, updatedAt: new Date(now).toISOString() });
    }
    await ctx.db.patch(m._id, { state: 'claimed', updatedAt: now });
    await ctx.scheduler.runAfter(0, internal.moves.rehomeRows, { threadId: m.threadId, from: m.from, to: m.to, table: 0, cursor: null });
    return null;
  },
});

/** T has the thread running: the move is over. */
export const moveDone = mutation({
  args: { key: v.string(), moveId: v.string() },
  returns: v.null(),
  handler: async (ctx, { key, moveId }) => {
    const caller = await runnerAuth(ctx, key, 'moveDone');
    const m = await moveRow(ctx, moveId);
    await mustBe(ctx, caller, m, 'to');
    if (m.state === 'done') return null;
    mustBeIn(m, ['claimed'], 'be done');
    await end(ctx, m, 'done');
    return null;
  },
});

/** S or T can't go on: the thread stays where it was. Only before the switch. */
export const moveFailed = mutation({
  args: { key: v.string(), moveId: v.string(), note: v.string() },
  returns: v.null(),
  handler: async (ctx, { key, moveId, note }) => {
    const caller = await runnerAuth(ctx, key, 'moveFailed');
    const m = await moveRow(ctx, moveId);
    await mustBe(ctx, caller, m, 'either');
    if (m.state === 'failed' || m.state === 'cancelled') return null;
    mustBeIn(m, BEFORE_CLAIM, 'fail');
    await end(ctx, m, 'failed', note.replace(/[\u0000-\u001f]+/g, ' ').trim() || 'The move failed.');
    return null;
  },
});

const REHOME_TABLES = ['messages', 'eventBodies', 'shareImages', 'replyImages', 'heldPictures'] as const;

async function countStoredBytes(ctx: MutationCtx, runnerId: string, delta: number): Promise<void> {
  const usage = await ctx.db.query('runnerStorage').withIndex('by_runnerId', (q) => q.eq('runnerId', runnerId)).unique();
  if (usage) await ctx.db.patch(usage._id, { bytes: Math.max(0, usage.bytes + delta) });
  else if (delta > 0) await ctx.db.insert('runnerStorage', { runnerId, bytes: delta });
}

// A picture's file to T: a runner key's upload record (and what it counts
// against) moves to T, or goes if T is the owner's, whose files have none;
// one with no record gets one if T has a key, so T can delete it.
async function rehomeFile(ctx: MutationCtx, storageId: Id<'_storage'>, from: string, to: string, toKeyed: boolean): Promise<void> {
  const rows = await ctx.db.query('storageOwners').withIndex('by_storageId', (q) => q.eq('storageId', storageId)).collect();
  if (!rows.length) {
    if (toKeyed) await ctx.db.insert('storageOwners', { storageId, runnerId: to, at: Date.now(), bytes: 0, attachedAt: Date.now() });
    return;
  }
  for (const row of rows) {
    if (row.runnerId !== from) continue;
    const bytes = row.bytes ?? 0;
    await countStoredBytes(ctx, from, -bytes);
    if (toKeyed) {
      await ctx.db.patch(row._id, { runnerId: to });
      await countStoredBytes(ctx, to, bytes);
    } else await ctx.db.delete(row._id);
  }
}

/**
 * A moved thread's mirrored rows to T, a page at a time: a runner key's
 * messages and tool output carry its runnerId and count against its
 * allowances (CAP), the owner's carry none.
 */
export const rehomeRows = internalMutation({
  args: { threadId: v.string(), from: v.string(), to: v.string(), table: v.number(), cursor: v.union(v.string(), v.null()) },
  returns: v.null(),
  handler: async (ctx, { threadId, from, to, table, cursor }) => {
    const name = REHOME_TABLES[table];
    if (!name) return null;
    const fromKeyed = await hasLiveKey(ctx, from);
    const toKeyed = await hasLiveKey(ctx, to);
    // Every one of these tables has by_thread_seq, from threadId.
    const page = await ctx.db
      .query(name as 'messages')
      .withIndex('by_thread_seq', (q) => q.eq('threadId', threadId))
      .paginate({ numItems: REHOME_PAGE, cursor });
    for (const row of page.page as Record<string, any>[]) {
      if (name === 'messages' || name === 'eventBodies') {
        // Only S's: a row under another runner's id isn't S's to give.
        const owner = row.runnerId as string | undefined;
        if (owner !== undefined ? owner !== from : fromKeyed) continue;
        const { _id, _creationTime, runnerId: _r, ...rest } = row;
        const count = name === 'messages' ? countMessageBytes : countBodyBytes;
        if (owner !== undefined) await count(ctx, from, -storedBytes(row));
        if (toKeyed) {
          await ctx.db.patch(_id, { runnerId: to });
          await count(ctx, to, docBytes({ ...rest, runnerId: to }));
        } else await ctx.db.patch(_id, { runnerId: undefined });
      } else await rehomeFile(ctx, row.storageId, from, to, toKeyed);
    }
    const next = page.isDone ? { table: table + 1, cursor: null } : { table, cursor: page.continueCursor };
    if (REHOME_TABLES[next.table]) await ctx.scheduler.runAfter(0, internal.moves.rehomeRows, { threadId, from, to, ...next });
    return null;
  },
});

/**
 * A move T never answered fails after MOVE_ASK_MS, one frozen and not
 * claimed after MOVE_FROZEN_MS, so a thread isn't left frozen on S for
 * good. Ended moves go after a week. The cron, every few minutes.
 */
export const expireMoves = internalMutation({
  args: {},
  returns: v.null(),
  handler: async (ctx) => {
    const now = Date.now();
    for (const m of await ctx.db.query('threadMoves').withIndex('by_state', (q) => q.eq('state', 'asking').lt('updatedAt', now - MOVE_ASK_MS)).take(50)) {
      await end(ctx, m, 'failed', `${await runnerName(ctx, m.to)} didn't answer.`);
    }
    for (const state of ['packing', 'unpacking'] as const) {
      // A handful at most: one per thread moving.
      for (const m of await ctx.db.query('threadMoves').withIndex('by_state', (q) => q.eq('state', state)).take(100)) {
        if ((m.frozenAt ?? m.updatedAt) < now - MOVE_FROZEN_MS) await end(ctx, m, 'failed', 'The move took more than 30 minutes.');
      }
    }
    for (const state of ['done', 'failed', 'cancelled'] as const) {
      for (const m of await ctx.db.query('threadMoves').withIndex('by_state', (q) => q.eq('state', state).lt('updatedAt', now - MOVE_KEPT_MS)).take(100)) await ctx.db.delete(m._id);
    }
    return null;
  },
});
