// Moving a thread to another runner, on each runner (docs/protocol.md,
// "Moving a thread to another runner"). The backend's threadMoves rows say
// whose turn it is (convex/moves.ts); this watches the ones this runner is
// in and takes its step on each, so a restart carries on from the state:
//
//   S (has the thread)          T (takes it)
//   asking:   describe it   ->  plan where it goes, say ready
//   ready:    wait for the turn, freeze it
//   packing:  pack, seal, upload
//                               unpacking: download, open, place it beside
//                                          the live threads, then the switch
//                               claimed:   adopt it, say done
//   done:     put its copy away
//
// failed or cancelled before the switch: S unfreezes it, T throws its
// staging away. The runner-specific work is MovesHost's (runner/moves.ts).
import type { PackEntry } from './move-pack.ts';
import { archive, chunks, MAX_SEALED_BYTES, sealInfo, sha256, unarchive, biggest } from './move-pack.ts';
import { sealBytes } from './secrets.ts';
import { withoutCredentials } from './move-git.ts';

export type MoveState = 'asking' | 'ready' | 'packing' | 'unpacking' | 'claimed' | 'done' | 'failed' | 'cancelled';

// What S says about the thread, for T to plan from (moves:moveInfo).
export type MoveInfo = {
  title: string;
  cwd: string;
  home: string;
  provider: string;
  accountId: string;
  email?: string;
  model?: string;
  effort?: string;
  permissionMode: string;
  origin?: string;
  repoTop?: string;
  subdir?: string;
  worktree: boolean;
  sessionId?: string;
  schedules: number;
};

// A MoveInfo from what came over the relay (a `moves.preview` call), with
// only its own fields and each of the right type; undefined if a needed one
// is missing. The backend is owner-only for calls, but isn't trusted to
// have checked the shape.
export function moveInfoFrom(raw: unknown): MoveInfo | undefined {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return undefined;
  const o = raw as Record<string, unknown>;
  const s = (k: string) => (typeof o[k] === 'string' ? (o[k] as string) : undefined);
  const title = s('title') ?? '';
  const cwd = s('cwd');
  const home = s('home');
  const provider = s('provider');
  const accountId = s('accountId');
  const permissionMode = s('permissionMode');
  if (!cwd || !home || !provider || !accountId || !permissionMode) return undefined;
  const info: MoveInfo = { title, cwd, home, provider, accountId, permissionMode, worktree: o.worktree === true, schedules: typeof o.schedules === 'number' && Number.isFinite(o.schedules) ? o.schedules : 0 };
  for (const k of ['email', 'model', 'effort', 'origin', 'repoTop', 'subdir', 'sessionId'] as const) {
    const v = s(k);
    if (v !== undefined) info[k] = v;
  }
  return info;
}

// The runner `asked` names for move_thread: its id, its whole name, or a
// word in just one runner's name, or why none. A name is the runner's own
// word (another Mac's heartbeat sets it, and can change it while a card
// is up), so a word that's one runner's whole name and part of another's
// is ambiguous rather than the whole-name one: "cloud" with "Hetzner
// Cloud" and a runner that renamed itself "cloud" names neither. The move
// then goes by the id this picks, never the name again (security review,
// 10 Nov 2026).
export function pickRunner(asked: string, all: readonly { id: string; name: string }[]): { id: string; name: string } | { problem: string } {
  const want = asked.trim().toLowerCase();
  const byId = all.find((r) => r.id === asked.trim());
  if (byId) return byId;
  const matches = want ? all.filter((r) => r.name.toLowerCase().includes(want)) : [];
  const listed = all.map((r) => `${r.name} (${r.id})`).join(', ');
  if (matches.length === 1) return matches[0]!;
  if (!matches.length) return { problem: `No runner called ${asked}. There are: ${listed}.` };
  return { problem: `${asked} could be ${matches.map((r) => `${r.name} (${r.id})`).join(' or ')}: name one by its id or its whole name, with no other runner's name containing it.` };
}

// The runner pickRunner chose, as the move goes: still there, under the
// name the card showed, or it throws and nothing moves.
export function stillRunner(runnerId: string, runnerName: string, all: readonly { id: string; name: string }[]): { id: string; name: string } {
  const to = all.find((r) => r.id === runnerId);
  if (!to) throw new Error(`The runner ${runnerName} (${runnerId}) isn't there any more, so the thread stays here.`);
  if (to.name !== runnerName) throw new Error(`The runner ${runnerId} is called ${to.name} now, not ${runnerName} as it was when the move was asked for, so the thread stays here. Ask again if it's still the one.`);
  return to;
}

// What T makes again of a moved thread's schedules (the bundle's
// schedules.json): only when and what each sends, to the moved thread.
// The bundle is the old runner's word, and that runner may be one of the
// owner's extra Macs, so nothing that runs or approves anything comes
// across: no shell check (Scheduler.create approves a check it's given
// without a card), no folder, login, model or mode (a schedule that sends
// to a thread runs on the thread's), no other runner. A check is set
// again on the runner that runs it, with its own card (security review,
// 10 Nov 2026). `dropped` counts the checks left off, for the thread's note.
export type MovedSchedule = { text: string; title?: string; cron?: string; at?: string; timezone?: string; paused?: boolean; createdBy?: string };
export function movedSchedules(raw: unknown, threadId: string): { schedules: MovedSchedule[]; dropped: number } {
  const schedules: MovedSchedule[] = [];
  let dropped = 0;
  for (const item of Array.isArray(raw) ? raw.slice(0, 100) : []) {
    if (!item || typeof item !== 'object' || Array.isArray(item)) continue;
    const o = item as Record<string, unknown>;
    const str = (k: string, max: number) => (typeof o[k] === 'string' && (o[k] as string).trim() && (o[k] as string).length <= max ? (o[k] as string) : undefined);
    if (o.check !== undefined || o.checkCard !== undefined) dropped++;
    const text = str('text', 100_000);
    const cron = str('cron', 200);
    const at = str('at', 100);
    if (!text || !cron === !at) continue;
    const title = str('title', 200);
    const timezone = str('timezone', 100);
    schedules.push({ text, ...(title ? { title } : {}), ...(cron ? { cron } : { at: at! }), ...(timezone ? { timezone } : {}), ...(o.paused === true ? { paused: true } : {}), ...(o.createdBy === threadId ? { createdBy: threadId } : {}) });
  }
  return { schedules, dropped };
}

// What the main Mac sends the runner a preview asks (threads.movePreview,
// a relay `moves.preview` call): only what planMove reads, and what a
// runner from before this needs to read it (moveInfoFrom wants a title,
// login and mode), scrubbed as sync scrubs. A relay command isn't scrubbed
// on its way (the phone's words go as typed), so this is where (security
// review, 10 Nov 2026).
export function previewInfo(info: MoveInfo, scrub: <T>(v: T) => T): Record<string, unknown> {
  const out: Record<string, unknown> = {
    title: '',
    cwd: info.cwd,
    home: info.home,
    provider: info.provider,
    accountId: info.accountId,
    permissionMode: info.permissionMode,
    worktree: info.worktree,
    schedules: 0,
    ...(info.email ? { email: info.email } : {}),
    ...(info.origin ? { origin: withoutCredentials(info.origin) } : {}),
    ...(info.repoTop ? { repoTop: info.repoTop } : {}),
    ...(info.subdir !== undefined ? { subdir: info.subdir } : {}),
  };
  return scrub(out);
}

// Where T will run it (moves:moveReady).
export type MovePlan = {
  cwd: string; // the folder, or for a worktree the repo it's made from (+ subdir)
  accountId: string;
  worktree?: { repo: string; subdir: string };
  clone?: { url: string; to: string };
  leftBehind: string[];
};

export type MoveRow = {
  moveId: string;
  threadId: string;
  from: string;
  to: string;
  state: MoveState;
  cwd?: string;
  accountId?: string;
  info?: MoveInfo;
  plan?: MovePlan;
  key?: string;
  sha256?: string;
  bytes?: number;
  chunks?: number;
  note?: string;
  createdAt: number;
  updatedAt: number;
  frozenAt?: number;
};

// T's answer for the Mac's Move sheet (threads.movePreview): the folder
// and login it would use, its logins to pick from, what won't come, and
// what stops the move (empty when it can go). `worktree`: the thread gets
// a new worktree of its own in `cwd`'s repo; `clone`: that repo is cloned
// there first.
export type MovePreview = {
  cwd?: string;
  accountId?: string;
  accounts: { id: string; provider: string; title: string; email?: string }[];
  leftBehind: string[];
  problems: string[];
  worktree?: boolean;
  clone?: string;
};

// What ThreadSummary.moving says on S while it goes.
export type ThreadMoving = { moveId: string; to: string; toName: string; state: MoveState; note?: string };

export type MovesBackend = {
  // Every update of the moves this runner is in (moves:moves).
  watch(onMoves: (rows: MoveRow[]) => void, onError: (e: unknown) => void): () => void;
  call(fn: 'moveInfo' | 'moveReady' | 'moveFrozen' | 'moveChunkUrl' | 'movePacked' | 'moveChunks' | 'moveClaim' | 'moveDone' | 'moveFailed' | 'moveDropChunks', args: Record<string, unknown>): Promise<unknown>;
  upload(url: string, bytes: Buffer): Promise<string>;
  download(url: string): Promise<Buffer>;
};

export type MovesHost = {
  // S
  describe(threadId: string): Promise<MoveInfo | undefined>;
  idle(threadId: string): boolean;
  setMoving(threadId: string, moving: ThreadMoving | undefined): void;
  // Releases the thread's CLI and gathers what goes (its files, the
  // transcript, git work, schedules); `leftBehind` is what won't.
  pack(threadId: string, row: MoveRow): Promise<{ entries: PackEntry[]; leftBehind: string[] }>;
  putAway(threadId: string, row: MoveRow): Promise<void>;
  has(threadId: string): boolean;
  // T
  plan(row: MoveRow): Promise<MovePlan | { problem: string }>;
  // Places it beside the live threads (its staging folder, worktree, git
  // work and transcript), not yet one of them. Throws to fail the move.
  place(row: MoveRow, entries: PackEntry[]): Promise<void>;
  // After the switch: makes it one of this runner's threads. Idempotent.
  adopt(row: MoveRow): Promise<void>;
  // The move failed or was cancelled: what place() made goes.
  discard(row: MoveRow): Promise<void>;
  sealKey(): string;
  open(sealed: Buffer, info: string): Buffer;
  runnerName(runnerId: string): string;
};

export type MovesDeps = {
  self: () => string;
  on: () => boolean;
  backend: () => MovesBackend | undefined;
  host: MovesHost;
  log: { info(m: string, f?: Record<string, unknown>): void; warn(m: string, f?: Record<string, unknown>): void };
  // How often S looks again while a thread finishes its turn.
  idleCheckMs?: number;
  setTimer?: (fn: () => void, ms: number) => { cancel(): void };
  // Every row as it's heard, for the main Mac to show a move coming to it of
  // a thread its sidebar lists as another runner's (main.ts remoteMoving).
  seen?: (row: MoveRow) => void;
};

const ENDED: MoveState[] = ['done', 'failed', 'cancelled'];
// A failed move's note stays on the thread this long.
export const FAILED_SHOWN_MS = 60_000;

export class Moves {
  private d: MovesDeps;
  private rows = new Map<string, MoveRow>();
  // Steps in flight, by move: one at a time each.
  private working = new Set<string>();
  // Steps done here (moveId:state), so an update that hasn't caught up yet
  // doesn't start the same one again.
  private done = new Set<string>();
  private stop?: () => void;
  private idleTimer?: { cancel(): void };

  constructor(deps: MovesDeps) {
    this.d = deps;
  }

  start(): void {
    if (this.stop || !this.d.on()) return;
    const backend = this.d.backend();
    if (!backend) return;
    this.stop = backend.watch(
      (rows) => this.update(rows),
      (e) => this.d.log.warn('watching moves failed', { error: String(e) }),
    );
  }

  close(): void {
    this.stop?.();
    this.stop = undefined;
    this.idleTimer?.cancel();
  }

  // Threads leaving this runner whose rows it must no longer write: past
  // the switch they're the other runner's. Only the thread's latest move
  // past its switch counts, so one moved away and back again is this
  // runner's again.
  gone(threadId: string): boolean {
    const last = this.lastSwitched(threadId);
    return !!last && last.from === this.d.self();
  }

  // A thread's latest move that got past its switch, of those heard of.
  private lastSwitched(threadId: string): MoveRow | undefined {
    let last: MoveRow | undefined;
    for (const m of this.rows.values()) {
      if (m.threadId !== threadId || (m.state !== 'claimed' && m.state !== 'done')) continue;
      if (!last || m.createdAt > last.createdAt) last = m;
    }
    return last;
  }

  // The move a thread here is in, for ThreadSummary.moving.
  movingOf(threadId: string): MoveRow | undefined {
    for (const m of this.rows.values()) if (m.threadId === threadId && !ENDED.includes(m.state)) return m;
    return undefined;
  }

  update(rows: MoveRow[]): void {
    for (const r of rows) this.rows.set(r.moveId, r);
    for (const r of rows) this.d.seen?.(r);
    for (const r of rows) void this.step(r);
  }

  private async step(row: MoveRow): Promise<void> {
    const me = this.d.self();
    const key = `${row.moveId}:${row.state}`;
    if (this.working.has(row.moveId) || this.done.has(key)) return;
    this.working.add(row.moveId);
    try {
      if (row.from === me) await this.asSource(row);
      else if (row.to === me) await this.asTarget(row);
      this.done.add(key);
    } catch (e) {
      // Not yet (the turn isn't over, the thread isn't described): the
      // next update or look tries again.
      if (e instanceof Waiting) return;
      const note = e instanceof Error ? e.message : String(e);
      this.d.log.warn('a move step failed', { moveId: row.moveId, state: row.state, error: note });
      if (!ENDED.includes(row.state) && row.state !== 'claimed') {
        await this.backend()
          .call('moveFailed', { moveId: row.moveId, note })
          .catch((err) => this.d.log.warn("couldn't fail a move", { moveId: row.moveId, error: String(err) }));
      }
      this.done.add(key);
    } finally {
      this.working.delete(row.moveId);
    }
  }

  private backend(): MovesBackend {
    const b = this.d.backend();
    if (!b) throw new Error("This runner isn't connected to Threadbear's backend.");
    return b;
  }

  private moving(row: MoveRow, note?: string): ThreadMoving {
    return { moveId: row.moveId, to: row.to, toName: this.d.host.runnerName(row.to), state: row.state, ...(note ? { note } : {}) };
  }

  private async asSource(row: MoveRow): Promise<void> {
    const { host } = this.d;
    const id = row.threadId;
    switch (row.state) {
      case 'asking': {
        if (row.info) return;
        const info = await host.describe(id);
        if (!info) throw new Error(`The thread isn't on ${host.runnerName(row.from)}.`);
        host.setMoving(id, this.moving(row));
        await this.backend().call('moveInfo', { moveId: row.moveId, info });
        return;
      }
      case 'ready': {
        host.setMoving(id, this.moving(row));
        if (!host.idle(id)) {
          this.lookAgain();
          throw new Waiting();
        }
        await this.backend().call('moveFrozen', { moveId: row.moveId });
        return;
      }
      case 'packing': {
        host.setMoving(id, this.moving(row));
        if (!row.key) throw new Error('The move has no key to seal to.');
        const { entries, leftBehind } = await host.pack(id, row);
        entries.push({ path: 'move.json', data: Buffer.from(JSON.stringify({ format: 'threadbear-move-1', moveId: row.moveId, threadId: id, from: row.from, fromName: host.runnerName(row.from), leftBehind })) });
        const sealed = sealBytes(row.key, archive(entries), sealInfo(row.moveId));
        if (sealed.length > MAX_SEALED_BYTES) throw new Error(`The thread is too big to move (${Math.round(sealed.length / 1024 / 1024)} MB, over 100): most of it is ${biggest(entries)}.`);
        const backend = this.backend();
        const ids: string[] = [];
        try {
          for (const part of chunks(sealed)) {
            const url = (await backend.call('moveChunkUrl', { moveId: row.moveId })) as string;
            ids.push(await backend.upload(url, part));
          }
          await backend.call('movePacked', { moveId: row.moveId, chunks: ids, sha256: sha256(sealed), bytes: sealed.length });
        } catch (e) {
          // The move ended meanwhile (the other runner failed it, say): the
          // main Mac's uploads have no record the backend's cleanup could
          // find, so it asks for them to go (a runner key's go with the move
          // by themselves; the backend refuses this for them).
          if (ids.length) await backend.call('moveDropChunks', { moveId: row.moveId, chunks: ids }).catch(() => undefined);
          throw e;
        }
        return;
      }
      case 'unpacking':
      case 'claimed':
        host.setMoving(id, this.moving(row));
        return;
      case 'done':
        // Not if it has come back since (a later move to here): heard again
        // after a restart, an old move's done would put the thread away.
        if (host.has(id) && this.lastSwitched(id)?.moveId === row.moveId) await host.putAway(id, row);
        return;
      case 'failed':
      case 'cancelled': {
        if (!host.has(id)) return;
        // A move that ended long ago (seen again after a restart) says nothing.
        if (Date.now() - row.updatedAt > FAILED_SHOWN_MS) host.setMoving(id, undefined);
        else {
          host.setMoving(id, this.moving(row, row.note));
          (this.d.setTimer ?? defaultTimer)(() => {
            if (this.movingOf(id) === undefined) host.setMoving(id, undefined);
          }, FAILED_SHOWN_MS);
        }
        return;
      }
    }
  }

  private async asTarget(row: MoveRow): Promise<void> {
    const { host } = this.d;
    switch (row.state) {
      case 'asking': {
        // S describes it first: the update that brings that tries again.
        if (!row.info) throw new Waiting();
        const plan = await host.plan(row);
        if ('problem' in plan) throw new Error(plan.problem);
        await this.backend().call('moveReady', { moveId: row.moveId, plan, sealKey: host.sealKey() });
        return;
      }
      case 'unpacking': {
        const backend = this.backend();
        const urls = (await backend.call('moveChunks', { moveId: row.moveId })) as string[];
        const parts: Buffer[] = [];
        for (const url of urls) parts.push(await backend.download(url));
        const sealed = Buffer.concat(parts);
        if (row.sha256 && sha256(sealed) !== row.sha256) throw new Error("The moved thread's bundle didn't arrive whole.");
        const entries = unarchive(host.open(sealed, sealInfo(row.moveId)));
        await host.place(row, entries);
        await backend.call('moveClaim', { moveId: row.moveId });
        return;
      }
      case 'claimed':
        await host.adopt(row);
        await this.backend().call('moveDone', { moveId: row.moveId });
        return;
      case 'failed':
      case 'cancelled':
        await host.discard(row);
        return;
      default:
        return;
    }
  }

  // While a thread finishes its turn: every few seconds, the moves waiting
  // on an idle thread are tried again.
  private lookAgain(): void {
    if (this.idleTimer) return;
    this.idleTimer = (this.d.setTimer ?? defaultTimer)(() => {
      this.idleTimer = undefined;
      for (const r of this.rows.values()) if (r.state === 'ready' && r.from === this.d.self()) void this.step(r);
    }, this.d.idleCheckMs ?? 3000);
  }
}

// A step that can't be taken yet: not a failure.
class Waiting extends Error {
  constructor() {
    super('waiting');
  }
}

function defaultTimer(fn: () => void, ms: number): { cancel(): void } {
  const t = setTimeout(fn, ms);
  t.unref?.();
  return { cancel: () => clearTimeout(t) };
}
