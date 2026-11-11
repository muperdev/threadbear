// Small jobs over ACP (models/acp-oneshot.ts), against the pretend agent, and
// the pool guard that decides who gets them (models/index.ts).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { ACP_AGENTS, opencodeEnv, type AcpAgent } from '../../src/adapters/acp/agents.ts';
import { acpAsker, promptBlocks, type Asker } from '../../src/models/acp-oneshot.ts';
import { Models } from '../../src/models/index.ts';
import { providerInfo } from '../../src/providers.ts';
import type { Account, Pool } from '../../src/protocol.ts';
import { nullLogger } from '../../src/log.ts';
import { ProcessRegistry } from '../../src/proc.ts';
import { FakeClock } from '../../src/clock.ts';

const FAKE = path.join(import.meta.dirname, '..', 'helpers', 'fake-acp-agent.mjs');
const account: Account = { id: 'opencode-main', provider: 'opencode', title: 'OpenCode', configDir: '/x' };

function rig(agent: AcpAgent = ACP_AGENTS.opencode, env: Record<string, string> = {}, binary = FAKE) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-oneshot-')));
  const logFile = path.join(root, 'log');
  const ask = acpAsker({ agent, binary: () => binary, on: () => true, registry: new ProcessRegistry(path.join(root, 'run'), nullLogger), log: nullLogger, env: { PATH: process.env.PATH, FAKE_ACP_LOG: logFile, ...env }, timings: { closeWaitMs: 200 } });
  const log = (): Record<string, unknown>[] => (fs.existsSync(logFile) ? fs.readFileSync(logFile, 'utf8').trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
  return { root, ask, log };
}

test('it answers, with no MCP servers, in the read-only mode', async () => {
  const r = rig();
  const res = await r.ask({ account, user: 'hello', cwd: r.root, timeoutMs: 8000 });
  assert.deepEqual(res, { ok: true, text: 'all done' });
  assert.deepEqual((r.log().find((l) => l.newSession) as { newSession: { mcpServers: unknown[] } }).newSession.mcpServers, []);
  assert.ok(r.log().some((l) => (l.set as { mode?: string } | undefined)?.mode === 'plan'));
});

test('the prompt never goes in the arguments or the environment', async () => {
  // A script standing in for the agent that writes what it was started with.
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-argv-')));
  const seen = path.join(root, 'seen');
  const wrap = path.join(root, 'wrap');
  // Quoted: the runner's release builds run from under "Application Support".
  fs.writeFileSync(wrap, `#!/bin/sh\necho "$@" > "${seen}"\nenv >> "${seen}"\nexec "${FAKE}" "$@"\n`, { mode: 0o755 });
  const r = rig(ACP_AGENTS.opencode, {}, wrap);
  const res = await r.ask({ account, user: 'SECRET-THREAD-TEXT', system: 'SECRET-SYSTEM', cwd: r.root, timeoutMs: 8000 });
  assert.equal(res.ok, true);
  const started = fs.readFileSync(seen, 'utf8');
  assert.doesNotMatch(started, /SECRET/);
  assert.match(String(r.log().find((l) => l.prompt)?.prompt), /SECRET-THREAD-TEXT/, 'it did reach the agent, over stdin');
});

test('every tool the agent tries is refused', async () => {
  const r = rig();
  const file = path.join(r.root, 'a.txt');
  const res = await r.ask({ account, user: `write ${file} hi`, cwd: r.root, timeoutMs: 8000 });
  assert.equal(res.ok, true);
  assert.equal(fs.existsSync(file), false);
});

test('small jobs get an agent told to deny every tool', () => {
  const conf = JSON.parse(opencodeEnv({ noTools: true }).OPENCODE_CONFIG_CONTENT);
  assert.equal(conf.permission['*'], 'deny');
  assert.equal(conf.permission.read, undefined, 'nothing is left open');
  assert.equal(conf.agent.plan.permission['*'], 'deny');
  assert.equal(opencodeEnv({ noTools: true }).OPENCODE_DISABLE_PROJECT_CONFIG, '1');
});

test('the wrong program gets no text', async () => {
  const r = rig(ACP_AGENTS.cursor, { FAKE_ACP_NAME: 'Grok' });
  const res = await r.ask({ account, user: 'SECRET', cwd: r.root, timeoutMs: 8000 });
  assert.equal(res.ok, false);
  assert.ok(!r.log().some((l) => l.prompt), 'never sent');
});

test('a hung agent times out and is killed', async () => {
  const r = rig();
  const res = await r.ask({ account, user: 'hang', cwd: r.root, timeoutMs: 400 });
  assert.equal(res.ok, false);
  assert.equal((res as { timedOut: boolean }).timedOut, true);
});

test('a schema is asked for in words, as ACP has no way to require it', () => {
  const b = promptBlocks({ user: 'x', jsonSchema: { type: 'object' } }) as { text: string }[];
  assert.match(b[0].text, /only JSON/);
  assert.equal(b[1].text, 'x', 'the text to work on stays a block of its own');
});

// ---- The pool guard

const claude: Account = { id: 'claude-main', provider: 'claude', title: 'c', configDir: '/c' };
const codex: Account = { id: 'codex-main', provider: 'codex', title: 'x', configDir: '/x' };
const pool = (id: string, accounts: Account[]): { pool: Pool; accounts: Account[] } => ({ pool: { id, title: id, accountIds: accounts.map((a) => a.id) }, accounts });

function models(o: { pools: Record<string, { pool: Pool; accounts: Account[] }>; poolId?: string | null; claudePath?: string; askers?: Partial<Record<string, Asker>>; askCli?: never }) {
  const claudeAsked: string[] = [];
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-models-'));
  const m = new Models({
    home,
    clock: new FakeClock(),
    log: nullLogger,
    settings: () => ({ privateFolders: [], ...(o.poolId !== undefined ? { pool: o.poolId } : {}) }),
    poolHost: { pool: (id) => o.pools[id], ranDry: () => undefined },
    claudePath: o.claudePath,
    askCli: async (req) => {
      claudeAsked.push(req.account.id);
      return { ok: true, text: 'from claude' };
    },
    askers: o.askers as never,
  });
  return { m, claudeAsked };
}

const answer = (said: string, seen: string[] = []): Asker => async (req) => {
  seen.push(req.account.id);
  return { ok: true, text: said };
};

test('the default claude pool never quietly becomes another agent', async () => {
  const seen: string[] = [];
  const { m } = models({ pools: { claude: pool('claude', [claude]) }, askers: { opencode: answer('nope', seen) } });
  const r = await m.summarize({ feature: 't', text: 'x' });
  assert.equal(r.ok, false);
  assert.equal((r as { reason: string }).reason, 'off', 'no claude binary: off, not on to OpenCode');
  assert.deepEqual(seen, []);
});

test('with no way to ask, a pool says off before taking a slot', async () => {
  const { m } = models({ pools: { x: pool('x', [account]) }, poolId: 'x' });
  const r = await m.summarize({ feature: 't', text: 'x' });
  assert.equal((r as { reason: string }).reason, 'no-pool');
});

test('an OpenCode pool answers once it has been picked', async () => {
  const seen: string[] = [];
  const { m, claudeAsked } = models({ pools: { oc: pool('oc', [account]) }, poolId: 'oc', askers: { opencode: answer('words', seen) } });
  const r = await m.summarize({ feature: 't', text: 'x' });
  assert.equal(r.ok, true);
  assert.equal((r as { value: string }).value, 'words');
  assert.deepEqual(seen, ['opencode-main']);
  assert.deepEqual(claudeAsked, []);
});

test('a mixed pool uses Claude when it has one', async () => {
  const seen: string[] = [];
  const { m, claudeAsked } = models({ pools: { mix: pool('mix', [account, claude]) }, poolId: 'mix', claudePath: '/c', askers: { opencode: answer('words', seen) } });
  await m.summarize({ feature: 't', text: 'x' });
  assert.deepEqual(claudeAsked, ['claude-main']);
  assert.deepEqual(seen, []);
});

test('Codex logins still answer nothing', async () => {
  const { m } = models({ pools: { x: pool('x', [codex]) }, poolId: 'x', claudePath: '/c', askers: {} });
  const r = await m.summarize({ feature: 't', text: 'x' });
  assert.equal((r as { reason: string }).reason, 'no-pool');
});

test('Cursor takes no small jobs, as its read tools cannot be turned off', async () => {
  assert.equal(providerInfo('cursor').smallJobs, false);
  assert.equal(providerInfo('opencode').smallJobs, true);
  // Daybreak, 10 Nov (finding 68): an asker for it, however made, never
  // starts it or sends it the text.
  const r = rig(ACP_AGENTS.cursor);
  const res = await r.ask({ account: { id: 'cursor-main', provider: 'cursor', title: 'Cursor', configDir: '/x' }, user: 'SECRET', cwd: r.root, timeoutMs: 8000 });
  assert.equal(res.ok, false);
  assert.match((res as { message: string }).message, /can't take small jobs/);
  assert.deepEqual(r.log(), [], 'never started');
});

test('a read-only mode the agent did not confirm means no text is sent', async () => {
  const r = rig(ACP_AGENTS.opencode, { FAKE_ACP_IGNORE_MODE: '1' });
  const res = await r.ask({ account, user: 'SECRET', cwd: r.root, timeoutMs: 8000 });
  assert.equal(res.ok, false);
  assert.ok(!r.log().some((l) => l.prompt));
});

test('the folder it runs in is made if it is not there', async () => {
  const r = rig();
  const res = await r.ask({ account, user: 'hello', cwd: path.join(r.root, 'not', 'yet'), timeoutMs: 8000 });
  assert.equal(res.ok, true);
});
