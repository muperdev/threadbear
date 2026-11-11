import path from 'node:path';
import fs from 'node:fs';
// One thread, read three ways, to check they agree (scenarios.test.ts):
//
//   here    this runner's own events and summary, as the Mac app reads them
//   relay   another runner's thread from the Mac, through Convex (relay.ts)
//   phone   the iPhone app and the website, through Convex (convex/phone.ts)
//
// A real Runner with a fake CLI plays the thread. Its real ConvexSync writes
// it through the backend's real functions into the in-memory Convex
// (convex-memory.ts, which checks validators and the schema as a deployment
// does), and the relay and the phone read it back through theirs. So a field
// that one copy along the way drops (the schema, the phone's ask, the relay's
// rebuild of a card) shows as the routes disagreeing, not as a bug on Mike's
// backend. Nothing here touches the network or a model, so it's as quick and
// steady as a unit test.
import { makeHarness } from './fakes.ts';
import { call, load, memoryBackend } from './convex-memory.ts';
import { ConvexSync } from '../../src/sync.ts';
import { Relay } from '../../src/relay.ts';
import { nullLogger } from '../../src/log.ts';
import type { ImageRef } from '../../src/protocol.ts';
import type { Post } from '../../src/sync.ts';

const OWNER = 'owner-key';
const TOKEN = 'phone-token';
const RUNNER = 'bruce0000001';
const END_TURN = 'mcp__threadbear__end_turn';

// What a person sees of a thread, on any of the three: its card and its messages.
export type Ask = { action: string; options?: string[]; optionNotes?: string[]; suggested?: number; links?: { title: string; url: string }[]; run?: { command: string; buttonId?: string; cwd?: string } };
export type View = {
  needs?: string;
  asks: Ask[];
  next: string[];
  recap?: string;
  messages: { role: 'user' | 'assistant'; text: string; pictures: number }[];
};

export type Play = {
  /** The agent writes text. */
  say(text: string): void;
  /** The agent calls end_turn with these args, and the turn ends. */
  endTurn(input: Record<string, unknown>): void;
  /** Mike sends a message, with pictures (paths of real files) if any. */
  send(text: string, images?: ImageRef[]): void;
  /** The librarian writes its note on the thread (with the recap box on). */
  librarianWrites(text: string, purpose: string): void;
  /** Turns on Run on the card, for the next end_turn. */
  runAsks(): void;
};

export async function threeWays(play: (p: Play) => void | Promise<void>) {
  process.env.RUNNER_KEY = OWNER;
  delete process.env.RUNNER_KEY_NEXT;
  process.env.CONVEX_SITE_URL = 'https://site.example';
  const b = memoryBackend();
  const mods: Record<string, Record<string, any>> = { sync: load('sync'), relay: load('relay'), phone: load('phone') };
  const refused: string[] = [];
  const post = async (_kind: string, fn: string, args: Record<string, unknown>) => {
    const [m, name] = fn.split(':');
    const f = mods[m!]?.[name!];
    if (!f) throw new Error(`routes: no backend function ${fn}`);
    try {
      return await call(b.ctx, f, args);
    } catch (e) {
      // The runner retries or skips what fails; the scenario says so.
      refused.push(`${fn}: ${String((e as Error)?.message ?? e)}`);
      throw e;
    }
  };
  const { h, sync, ids } = await playInto(post as never, { key: OWNER, runnerId: RUNNER, name: 'bruce' }, [play]);
  const thread = { id: ids[0]! };

  const id = thread.id;
  const at = new Date(h.clock.now()).toISOString();
  const relay = new Relay({
    source: () => ({ key: OWNER, post: post as never, apiVersion: 999 }),
    remote: () => ({ runners: [{ id: RUNNER, name: 'bruce', online: true, lastSeenAt: at }], threads: [{ runnerId: RUNNER, threadId: id, title: 't', cwd: '/x', accountId: 'a', provider: 'claude', status: 'idle', needsYou: false, unread: true, preview: '', createdAt: at, lastActivityAt: at }] }),
    subscribed: () => false,
    publish: () => {},
    clock: h.clock,
    log: nullLogger,
  });
  const { sha256 } = load('lib') as { sha256: (s: string) => Promise<string> };
  await b.ctx.db.insert('tokens', { hash: await sha256(TOKEN), name: 'iphone', kind: 'phone', expiresAt: Date.now() + 1e9, createdAt: Date.now() });

  const here = (): View => {
    const t = h.runner.get(id);
    const events = h.runner.events(id, {}).events as Record<string, any>[];
    return { needs: t.needs, asks: (t.asks ?? []) as Ask[], next: lastNext(events), recap: t.recap, messages: messagesOf(events) };
  };
  const viaRelay = async (): Promise<View> => {
    // A turn's end is held back until that runner's row has said so for 4 s.
    await relay.call(id, 'threads.events', { threadId: id });
    h.clock.advance(5000);
    const { events } = (await relay.call(id, 'threads.events', { threadId: id })) as { events: Record<string, any>[] };
    const end = events.filter((e) => e.type === 'turn.completed').at(-1);
    // The sidebar's row, as the Mac reads another runner's thread from it.
    const rows = (await b.ctx.db.query('threadIndex').collect()) as unknown as { threadId: string; recap?: string }[];
    return { needs: end?.needs, asks: (end?.asks ?? []) as Ask[], next: (end?.next ?? []) as string[], recap: rows.find((r) => r.threadId === id)?.recap, messages: messagesOf(events) };
  };
  const viaPhone = async (): Promise<View> => {
    const r = (await call(b.ctx, mods.phone.thread!, { token: TOKEN, threadId: id })) as { thread: Record<string, any>; messages: { role: 'user' | 'assistant'; text: string }[] };
    return { needs: r.thread.needs, asks: (r.thread.asks ?? []) as Ask[], next: (r.thread.next ?? []) as string[], recap: r.thread.recap, messages: r.messages.map((m) => ({ role: m.role, text: m.text, pictures: 0 })) };
  };
  const views = { here: here(), relay: await viaRelay(), phone: await viaPhone() };
  await sync.stop();
  return { views, refused, backend: b, threadId: id };
}

/**
 * Plays each of `plays` as a thread on one Runner with a fake CLI, and
 * writes them all through `post` with that runner's real ConvexSync: the
 * in-memory backend here, or a real one (scripts/parity-seed.ts). Returns
 * the threads' ids in order; the caller stops `sync`.
 */
export async function playInto(post: Post, runner: { key: string; runnerId: string; name: string }, plays: ((p: Play) => void | Promise<void>)[]) {
  const h = makeHarness();
  const sync = new ConvexSync({ config: { enabled: true, url: 'https://x.convex.cloud', name: runner.name, runnerId: runner.runnerId }, key: runner.key, home: h.cfg.home, runner: h.runner, version: '0.1.0+test', release: 'rel1', startedAt: '2026-10-27T00:00:00.000Z', clock: h.clock, log: nullLogger, post, upload: async () => 'st' } as never);
  const ids: string[] = [];
  for (const play of plays) {
    const { thread } = h.runner.create({ text: 'Start', accountId: 'claude-3' });
    ids.push(thread.id);
    const s = h.adapter.last;
    s.init();
    let n = 0;
    await play({
      say: (text) => s.text(`m${++n}`, text),
      endTurn: (input) => {
        const id = `e${++n}`;
        s.out({ kind: 'event', event: { type: 'tool.call', itemId: id, name: END_TURN, kind: 'mcp', title: 'threadbear: end_turn', input } });
        s.out({ kind: 'event', event: { type: 'tool.result', itemId: id, ok: true, text: 'ok', truncated: false, images: [] } });
        s.result('completed');
      },
      send: (text, images = []) => {
        h.runner.send(thread.id, text, images, {});
        h.adapter.last.init();
      },
      runAsks: () => {
        const file = path.join(h.cfg.home, 'runner.json');
        const json = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : {};
        fs.writeFileSync(file, JSON.stringify({ ...json, experiments: { ...json.experiments, runAsks: true } }));
      },
      librarianWrites: (text, purpose) => {
        const file = path.join(h.cfg.home, 'runner.json');
        const json = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : {};
        fs.writeFileSync(file, JSON.stringify({ ...json, experiments: { ...json.experiments, recapBox: true } }));
        h.clock.advance(6000); // past the runner's short memory of the switch
        h.runner.setBrief(thread.id, text, purpose);
      },
    });
  }
  sync.start();
  await sync.flushNow();
  await sync.flushNow();
  // The sidebar's index (thread-index.ts), which another runner lists the
  // threads from, writes on a timer of its own; the fake clock only moves
  // when told.
  for (let i = 0; i < 5; i++) {
    h.clock.advance(1500);
    await new Promise((r) => setTimeout(r, 20));
  }
  return { h, sync, ids };
}

// The done card's follow-ups: the last turn's end.
function lastNext(events: Record<string, any>[]): string[] {
  return (events.filter((e) => e.type === 'turn.completed').at(-1)?.next ?? []) as string[];
}

// Mike's messages and the agent's text, in order; tool rows left out.
function messagesOf(events: Record<string, any>[]): View['messages'] {
  return events
    .filter((e) => e.type === 'user.message' || e.type === 'assistant.text')
    .map((e) => ({ role: e.type === 'user.message' ? ('user' as const) : ('assistant' as const), text: String(e.text ?? ''), pictures: Array.isArray(e.images) ? e.images.length : 0 }));
}
