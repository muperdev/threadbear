// A real check of every other machine's runner, through this Mac's live
// runner and Convex, the way the Mac app reaches them by default. For each
// runner online (Hetzner, bruce, the Pixel) it starts a throwaway Haiku
// thread there, sends it a red square, checks the agent saw red, checks the
// decision card it ends with keeps what each answer does on the way back,
// and archives the thread. Then it checks the backend is on the version this
// runner expects and that no machine says it can't follow GitHub's main. One line per check; exits 1 if any failed.
//
// The unit tests (scenarios.test.ts, parity.test.ts) prove the code agrees
// with itself; this proves the real machines do: a release that didn't
// build, a backend that wasn't deployed, a key that broke. It's what found
// nothing wrong by hand on 8 Nov after pictures to other runners landed.
// It costs a few Haiku turns a run; the daily schedule runs it.
//
//   node runner/scripts/canary.ts [--runner <name>]   (THREADBEAR_HOME for another data dir)
//
// The daily schedule runs it as its check: an agent starts only when it
// prints, which is only when something failed.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { parseArgs } from 'node:util';
import { RunnerClient } from '../src/client.ts';
import { loadConfig, readToken, runnerUrl } from '../src/config.ts';
import { makePng } from '../test/helpers/png.ts';
import { forTerminal } from '../src/terminal-text.ts';

type Ev = { seq: number; type: string; text?: string; asks?: { action: string; options?: string[]; optionNotes?: string[] }[] };
type Runner = { id: string; name: string; online: boolean };
type Login = { id: string; provider: string; models?: { id: string }[] };

// Network blips worth trying a read again for (read(), below).
const BLIP = /fetch failed|ECONNRESET|ETIMEDOUT|ENOTFOUND|EAI_AGAIN|socket hang up|aborted due to timeout/i;

// Each turn's wait; two turns per runner, all runners at once, inside a schedule check's 5 minutes.
const TURN_MS = 2 * 60_000;
const BRIEF = [
  'This is an automatic check of Threadbear (the daily canary); nobody is reading it live.',
  "Don't run any tools apart from end_turn.",
  'Reply with just the word ready, then call end_turn with needs "nothing".',
  'My next message has a picture in it. Then reply with just its colour, in one word, and call end_turn with',
  'needs "you" and asks: [{ "question": "Is that the colour?", "options": [{ "label": "Yes", "does": "It is that colour" }, { "label": "No", "does": "It is another colour" }] }].',
].join(' ');

const { values } = parseArgs({ options: { runner: { type: 'string' } } });
const results: { ok: boolean; line: string }[] = [];
const say = (ok: boolean, line: string) => {
  results.push({ ok, line });
  // Its lines hold what other runners wrote (their names, followProblem,
  // threads' replies): no escape sequences reach the terminal (tb status's
  // rule, terminal-text.ts), and each check is one line.
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${forTerminal(line).replace(/\s+/g, ' ')}`);
};

const cfg = loadConfig(process.env, { readOnly: true });
const client = await RunnerClient.connect(runnerUrl(cfg), readToken(cfg.home));
try {
  await client.call('hello', { client: 'tb-canary', version: '1' });
  const status = (await read('runner.status', {})) as { sync?: { backend?: { apiVersion: number; expected: number } }; behindRunners?: { name: string; problem: string }[] };
  const backend = status.sync?.backend;
  if (backend) say(backend.apiVersion >= backend.expected, `the backend is on API ${backend.apiVersion}, this runner expects ${backend.expected}`);
  else say(false, "this runner hasn't heard the backend's version (is the Convex mirror on?)");
  // A machine that can't follow GitHub's main (follow-origin.ts): what sat
  // unnoticed on bruce and the Pixel for days before this canary found it.
  for (const r of status.behindRunners ?? []) say(false, `${r.name} isn't following GitHub's main properly: ${r.problem}`);

  const remote = (await read('remoteThreads.list', {})) as { runners: Runner[] };
  const runners = remote.runners.filter((r) => !values.runner || r.name === values.runner || r.id === values.runner);
  if (!runners.length) say(false, values.runner ? `no runner called ${values.runner}` : 'no other runners are listed');
  // Side by side, so a run fits the 5 minutes a schedule's check may take.
  await Promise.all(
    runners.map(async (r) => {
      if (!r.online) return say(true, `${r.name}: offline, skipped`);
      await check(r).catch((e) => say(false, `${r.name}: ${(e as Error).message}`));
    }),
  );
} finally {
  client.close();
}
process.exit(results.every((x) => x.ok) ? 0 : 1);

async function check(r: Runner): Promise<void> {
  const logins = (await step("reading its logins", () => read('remoteThreads.logins', { runnerId: r.id }))) as { accounts: Login[] };
  const claude = logins.accounts.find((a) => a.provider === 'claude');
  const haiku = claude?.models?.find((m) => /haiku/i.test(m.id))?.id;
  const made = (await step('starting a thread there', () => client.call('threads.create', { runnerId: r.id, text: BRIEF, title: 'Canary: Threadbear daily check', ...(claude ? { accountId: claude.id } : {}), ...(haiku ? { model: haiku } : {}) }))) as { thread: { id: string } };
  const id = made.thread.id;
  try {
    const first = await turnEnd(id, 0);
    say(/\bready\b/i.test(reply(first.events, first.from)), `${r.name}: a new thread started there and answered`);

    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-canary-'));
    const file = path.join(dir, 'red.png');
    fs.writeFileSync(file, makePng(32, 32, [230, 20, 20]));
    const { image } = (await client.call('media.import', { path: file })) as { image: unknown };
    fs.rmSync(dir, { recursive: true, force: true });
    await client.call('threads.send', { threadId: id, text: 'What colour is this? [Picture 1]', images: [image] });
    const second = await turnEnd(id, first.end);
    const said = reply(second.events, second.from);
    say(/\bred\b/i.test(said), `${r.name}: a picture sent from here reached it (it said ${JSON.stringify(said.slice(0, 40))})`);
    const ask = second.card?.asks?.[0];
    const notes = Boolean(ask && ask.options?.length === 2 && ask.optionNotes?.filter(Boolean).length === 2);
    say(notes, `${r.name}: its decision card came back with what each answer does${notes ? '' : ` (thread ${id}, card ${JSON.stringify(second.card?.asks ?? null)})`}`);
  } finally {
    await client.call('threads.update', { threadId: id, archived: true }).catch((e) => say(false, `${r.name}: couldn't archive the canary thread ${id}: ${(e as Error).message}`));
  }
}

// A read that failed on a network blip (this Mac's runner reaching Convex)
// is tried again, so the canary doesn't cry wolf: it reports what stays
// broken. Only reads: a create or a send tried twice could do it twice.
// BLIP is up top: the checks run (and exit) before the code down here, and a
// const declared below them is still uninitialised when they call read().
async function read<T>(method: string, params: Record<string, unknown>): Promise<T> {
  for (let i = 1; ; i++) {
    try {
      return (await client.call(method, params)) as T;
    } catch (e) {
      if (i >= 4 || !BLIP.test(String((e as Error)?.message))) throw e;
      await new Promise((res) => setTimeout(res, 5000 * i));
    }
  }
}

// A step's error says which step it was.
async function step<T>(what: string, fn: () => Promise<T>): Promise<T> {
  try {
    return await fn();
  } catch (e) {
    throw new Error(`${what}: ${(e as Error).message}`);
  }
}

// Waits for the turn ending after `after` (a seq), through the relay.
async function turnEnd(id: string, after: number): Promise<{ events: Ev[]; from: number; end: number; card?: Ev }> {
  const until = Date.now() + TURN_MS;
  while (Date.now() < until) {
    const { events } = await read<{ events: Ev[] }>('threads.events', { threadId: id });
    const end = events.find((e) => e.type === 'turn.completed' && e.seq > after);
    if (end) return { events, from: after, end: end.seq, card: end };
    await new Promise((res) => setTimeout(res, 3000));
  }
  throw new Error(`no answer within ${TURN_MS / 60_000} minutes`);
}

function reply(events: Ev[], after: number): string {
  return events
    .filter((e) => e.type === 'assistant.text' && e.seq > after)
    .map((e) => e.text ?? '')
    .join(' ')
    .trim();
}
