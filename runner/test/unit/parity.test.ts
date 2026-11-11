// Every request about a thread says where it works besides this runner's own
// threads (src/parity.ts), and each claim is checked against the code: the
// relay (another runner's thread from the Mac, through Convex) either answers
// it or refuses it as "not possible through Convex yet", and a phone `via` is
// a function convex/phone.ts has. Pictures to Hetzner, a decision card's notes
// and send_message to a cloud thread each went missing there on 8 Nov without
// anyone deciding they should; a new request now can't land without a row.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { THREAD_REQUESTS, parityDoc } from '../../src/parity.ts';
import { THREAD_METHODS } from '../../src/rpc.ts';
import { Relay } from '../../src/relay.ts';
import { FakeClock } from '../../src/clock.ts';
import { nullLogger } from '../../src/log.ts';

const T = 'thread-on-bruce';
const REFUSED = /isn't possible through Convex yet/;

function relay(): Relay {
  const at = new Date(0).toISOString();
  return new Relay({
    // No subscribe: a queued command answers at once as pending, so nothing waits.
    source: () => ({ key: 'owner', apiVersion: 999, upload: async () => 'st', post: async (_kind, fn) => (fn === 'relay:queueCommand' ? 'cmd1' : fn === 'relay:remoteMessages' ? { messages: [], hasMore: false } : null) }),
    remote: () => ({
      runners: [{ id: 'bruce', name: 'bruce', online: true, lastSeenAt: at }],
      threads: [{ runnerId: 'bruce', threadId: T, title: 'A thread', cwd: '/x', accountId: 'a', provider: 'claude', status: 'idle', needsYou: false, unread: true, preview: '', createdAt: at, lastActivityAt: at }],
    }),
    readPicture: () => ({ bytes: new Uint8Array([1]), mediaType: 'image/png' }),
    subscribed: () => false,
    publish: () => {},
    clock: new FakeClock(),
    log: nullLogger,
  });
}

// What each request is sent with, where it needs something.
const PARAMS: Record<string, Record<string, unknown>> = {
  'threads.send': { text: 'x' },
  'buttons.press': { buttonId: 'abcd1234' },
  'threads.update': { title: 't' },
  'threads.heldBody': { seq: 8 },
  'threads.hibernateWhen': { said: 'when the build is done' },
  'threads.share': { access: 'link' },
  'approvals.respond': { approvalId: 'a', decision: 'allow' },
  'secrets.answer': { approvalId: 'a', decline: true },
};

// Whether the relay refuses the request as not possible through Convex. One
// that's still going after a moment (waiting on that runner) was taken.
async function refused(method: string): Promise<boolean> {
  const p = relay().call(T, method, { threadId: T, ...(PARAMS[method] ?? {}) });
  const outcome = await Promise.race([p.then(() => 'ok', (e: Error) => (REFUSED.test(String(e?.message)) ? 'refused' : 'ok')), new Promise((r) => setTimeout(() => r('ok'), 50))]);
  p.catch(() => {});
  return outcome === 'refused';
}

test('every thread request has a row saying where it works, and nothing else does', () => {
  assert.deepEqual(Object.keys(THREAD_REQUESTS).sort(), [...THREAD_METHODS].sort(), 'add a row to src/parity.ts for each new thread request (or take out one that went)');
  for (const [method, row] of Object.entries(THREAD_REQUESTS)) {
    assert.ok(row.what.trim(), `${method} says what it is`);
    for (const w of [row.relay, row.phone]) if (typeof w === 'object' && 'no' in w) assert.ok(w.no.trim().endsWith('.'), `${method}: a "no" says why, as a sentence`);
  }
});

test("the relay answers exactly the requests whose row says it does", async () => {
  for (const [method, row] of Object.entries(THREAD_REQUESTS)) {
    const says = row.relay === 'yes';
    assert.equal(!(await refused(method)), says, says ? `${method}: the row says another runner's thread can do it through Convex, but relay.ts refuses it` : `${method}: relay.ts answers it now, so its row can say yes`);
  }
});

test("a phone row names a function the phone and the website really have", () => {
  const src = fs.readFileSync(path.join(import.meta.dirname, '../../../convex/phone.ts'), 'utf8');
  const fns = new Set([...src.matchAll(/^export const (\w+) = (?:mutation|query)\(/gm)].map((m) => m[1]!));
  for (const [method, row] of Object.entries(THREAD_REQUESTS)) {
    if ('via' in row.phone) assert.ok(fns.has(row.phone.via), `${method}: convex/phone.ts has no ${row.phone.via}`);
  }
});

test('docs/parity.md is made from the rows (npm run parity-docs)', () => {
  const file = path.join(import.meta.dirname, '../../../docs/parity.md');
  if (process.env.UPDATE_PARITY_DOCS === '1') fs.writeFileSync(file, parityDoc());
  assert.equal(fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '', parityDoc(), 'run npm run parity-docs in runner/ and commit docs/parity.md');
});
