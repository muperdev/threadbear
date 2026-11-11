// AcpAdapter against a pretend agent (helpers/fake-acp-agent.mjs) that
// behaves as OpenCode was seen to: it runs tools unasked unless told otherwise.
import { test, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { AcpAdapter, AcpSession, EDIT_RULE, modelChoices } from '../../src/adapters/acp/adapter.ts';
import { ACP_AGENTS, acpAccounts, isCursorBinary, opencodeEnv } from '../../src/adapters/acp/agents.ts';
import type { AcpAgent } from '../../src/adapters/acp/agents.ts';
import { AcpMcpServer } from '../../src/adapters/acp/mcp-http.ts';
import { commandRule } from '../../src/adapters/codex/approvals.ts';
import { remoteCardShowsAll } from '../../src/tools.ts';
import type { AdapterOutput, SessionSink, StartSessionOptions, ToolApprovalAnswer, ToolApprovalRequest } from '../../src/adapters/types.ts';
import type { Account, PermissionMode } from '../../src/protocol.ts';
import { nullLogger } from '../../src/log.ts';
import { ProcessRegistry } from '../../src/proc.ts';
import { z } from 'zod';

const FAKE = path.join(import.meta.dirname, '..', 'helpers', 'fake-acp-agent.mjs');

// Real folders on the temp volume, as adapters compare and resolve them.
function tmp(): string {
  return fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-acp-')));
}

const made: string[] = [];
const sessions: { end(o?: { hard?: boolean }): Promise<void> }[] = [];
afterEach(async () => {
  await Promise.all(sessions.splice(0).map((s) => s.end({ hard: true }).catch(() => undefined)));
  for (const d of made.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

function rig(o: { mode?: PermissionMode; agent?: AcpAgent; env?: Record<string, string>; opts?: Partial<StartSessionOptions>; deny?: boolean; answer?: (r: ToolApprovalRequest) => ToolApprovalAnswer | Promise<ToolApprovalAnswer>; on?: boolean } = {}) {
  const root = tmp();
  made.push(root);
  const cwd = path.join(root, 'work');
  fs.mkdirSync(cwd);
  const logFile = path.join(root, 'agent.log');
  const account: Account = { id: 'opencode-main', provider: 'opencode', title: 'OpenCode', configDir: root };
  const outputs: AdapterOutput[] = [];
  const cards: ToolApprovalRequest[] = [];
  let exitedWith: Error | undefined | null = null;
  let exitWaiters: (() => void)[] = [];
  const results: (() => void)[] = [];
  const sink: SessionSink = {
    output(out) {
      outputs.push(out);
      if (out.kind === 'result') results.splice(0).forEach((f) => f());
    },
    async requestApproval(req) {
      cards.push(req);
      return o.answer ? o.answer(req) : { decision: o.deny ? 'deny' : 'allow' };
    },
    async requestQuestion() {
      return { answers: null };
    },
    exited(e) {
      exitedWith = e;
      exitWaiters.splice(0).forEach((f) => f());
    },
  };
  const agent: AcpAgent = o.agent ?? ACP_AGENTS.opencode;
  const adapter = new AcpAdapter({
    agent,
    binary: () => FAKE,
    on: () => o.on !== false,
    registry: new ProcessRegistry(path.join(root, 'run'), nullLogger),
    log: nullLogger,
    env: { PATH: process.env.PATH, FAKE_ACP_LOG: logFile, ...o.env },
    timings: { closeWaitMs: 300 },
  });
  const start = (): AcpSession => {
    const s = adapter.start(
      { threadId: 't1', cwd, account, permissionMode: o.mode ?? 'ask', sessionId: 'unused', resume: false, mediaDir: path.join(root, 'media'), blobsDir: path.join(root, 'blobs'), tmpDir: root, ...o.opts },
      sink,
    ) as AcpSession;
    sessions.push(s);
    return s;
  };
  const nextResult = () => new Promise<void>((r) => results.push(r));
  const exited = () => (exitedWith !== null ? Promise.resolve() : new Promise<void>((r) => exitWaiters.push(r)));
  const log = (): Record<string, unknown>[] => (fs.existsSync(logFile) ? fs.readFileSync(logFile, 'utf8').trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
  return { root, cwd, adapter, start, cards, outputs, nextResult, exited, log, account, exitError: () => exitedWith };
}

async function until(cond: () => boolean, ms = 3000): Promise<void> {
  const end = Date.now() + ms;
  while (!cond()) {
    if (Date.now() > end) throw new Error('timed out waiting');
    await new Promise((r) => setTimeout(r, 10));
  }
}

const results = (out: AdapterOutput[]) => out.filter((o): o is Extract<AdapterOutput, { kind: 'result' }> => o.kind === 'result').map((o) => o.result);
const events = (out: AdapterOutput[], type: string) => out.flatMap((o) => (o.kind === 'event' && o.event.type === type ? [o.event as never] : []));

async function turn(r: ReturnType<typeof rig>, s: AcpSession, text: string, id = 'turn-1'): Promise<void> {
  const done = r.nextResult();
  s.send({ id, text, images: [] });
  await done;
}

// ---- Asking

test('Ask: a write waits for the card and runs only once allowed', async () => {
  const r = rig({ mode: 'ask' });
  const s = r.start();
  const file = path.join(r.cwd, 'a.txt');
  await turn(r, s, `write ${file} hi`);
  assert.equal(r.cards.length, 1, 'the agent asked, and the card was put up');
  assert.equal(r.cards[0].name, 'apply_patch');
  assert.equal(r.cards[0].title, 'Edit a.txt');
  assert.equal(fs.readFileSync(file, 'utf8'), 'hi');
  assert.deepEqual((r.log().find((l) => l.answered) as { answered: unknown }).answered, { outcome: { outcome: 'selected', optionId: 'once' } });
});

test('Ask: denying a card stops the write', async () => {
  const r = rig({ mode: 'ask', deny: true });
  const s = r.start();
  const file = path.join(r.cwd, 'a.txt');
  await turn(r, s, `write ${file} hi`);
  assert.equal(fs.existsSync(file), false);
  assert.deepEqual((r.log().find((l) => l.answered) as { answered: unknown }).answered, { outcome: { outcome: 'selected', optionId: 'reject' } });
});

// The test Mike asked for: without the row's env the pretend agent, like the
// real one, writes with no request at all (the assertion below then fails).
for (const mode of ['ask', 'auto', 'plan'] as PermissionMode[]) {
  test(`${mode}: the row forces asking, and a folder's own config can't turn it off`, async () => {
    const r = rig({ mode, deny: true });
    fs.writeFileSync(path.join(r.cwd, 'opencode.json'), JSON.stringify({ permission: { edit: 'allow', bash: 'allow', '*': 'allow' } }));
    const s = r.start();
    const file = path.join(r.cwd, 'a.txt');
    await turn(r, s, `write ${file} hi`);
    assert.equal(r.cards.length, 1, `${mode} must ask`);
    assert.equal(fs.existsSync(file), false);
  });
}

test('the row turns the folder config, plugins and the agent\'s updates off', () => {
  const env = opencodeEnv();
  assert.equal(env.OPENCODE_DISABLE_PROJECT_CONFIG, '1');
  assert.equal(env.OPENCODE_DISABLE_AUTOUPDATE, '1');
  const conf = JSON.parse(env.OPENCODE_CONFIG_CONTENT);
  assert.equal(conf.permission['*'], 'ask', 'a tool no one has heard of asks too');
  assert.equal(conf.agent.build.permission['*'], 'ask', "an agent's own rules outrank the top level");
  assert.equal(conf.agent.plan.permission['*'], 'ask');
  assert.ok(ACP_AGENTS.opencode.args.includes('--pure'));
});

test("a folder's plugin doesn't run", async () => {
  const r = rig({ mode: 'full' });
  fs.mkdirSync(path.join(r.cwd, '.opencode', 'plugins'), { recursive: true });
  const marker = path.join(r.root, 'ran');
  fs.writeFileSync(path.join(r.cwd, '.opencode', 'plugins', 'run.txt'), marker);
  const s = r.start();
  await turn(r, s, 'hello');
  assert.equal(fs.existsSync(marker), false);
});

test('a thread\'s own environment cannot take the forced asking off', async () => {
  const r = rig({ mode: 'ask', deny: true, opts: { env: { OPENCODE_CONFIG_CONTENT: '{"permission":{"*":"allow"}}', OPENCODE_DISABLE_PROJECT_CONFIG: '0' } } });
  fs.writeFileSync(path.join(r.cwd, 'opencode.json'), JSON.stringify({ permission: { '*': 'allow' } }));
  const s = r.start();
  const file = path.join(r.cwd, 'a.txt');
  await turn(r, s, `write ${file} hi`);
  assert.equal(r.cards.length, 1);
  assert.equal(fs.existsSync(file), false);
});

test('Full runs without a card, but still through a request answered once', async () => {
  const r = rig({ mode: 'full' });
  const s = r.start();
  const file = path.join(r.cwd, 'a.txt');
  await turn(r, s, `write ${file} hi`);
  assert.equal(r.cards.length, 0);
  assert.equal(fs.readFileSync(file, 'utf8'), 'hi');
  assert.deepEqual((r.log().find((l) => l.answered) as { answered: unknown }).answered, { outcome: { outcome: 'selected', optionId: 'once' } });
});

test('leaving Full takes effect on the very next request', async () => {
  const r = rig({ mode: 'full', deny: true });
  const s = r.start();
  await turn(r, s, 'hello');
  await s.setPermissionMode('ask');
  const file = path.join(r.cwd, 'a.txt');
  await turn(r, s, `write ${file} hi`, 'turn-2');
  assert.equal(r.cards.length, 1);
  assert.equal(fs.existsSync(file), false);
});

test("an agent's own \"always\" is never picked: Always allow stays Threadbear's", async () => {
  const r = rig({ mode: 'ask', answer: () => ({ decision: 'allowAlways' }) });
  const s = r.start();
  await turn(r, s, `write ${path.join(r.cwd, 'a.txt')} hi`);
  assert.deepEqual((r.log().find((l) => l.answered) as { answered: unknown }).answered, { outcome: { outcome: 'selected', optionId: 'once' } });
  assert.ok(r.outputs.some((o) => o.kind === 'allowRules' && o.rules.includes(EDIT_RULE)));
  await turn(r, s, `write ${path.join(r.cwd, 'b.txt')} hi`, 'turn-2');
  assert.equal(r.cards.length, 1, 'the second edit needed no card');
});

test('no plain allow on offer: it is cancelled, never allowed', async () => {
  const r = rig({ mode: 'full', env: { FAKE_ACP_NO_ONCE: '1' } });
  const s = r.start();
  const file = path.join(r.cwd, 'a.txt');
  await turn(r, s, `write ${file} hi`);
  assert.equal(fs.existsSync(file), false);
  assert.deepEqual((r.log().find((l) => l.answered) as { answered: unknown }).answered, { outcome: { outcome: 'cancelled' } });
});

// ---- Files that run as code

for (const [name, rel] of [['a patch', '.threadbear/patches/x.ts'], ['Claude settings', '.claude/settings.json'], ['an MCP config', '.mcp.json']] as const) {
  test(`editing ${name} always asks, with no Always allow, past an earlier one`, async () => {
    const r = rig({ mode: 'ask', opts: { allowRules: [EDIT_RULE] } });
    fs.mkdirSync(path.dirname(path.join(r.cwd, rel)), { recursive: true });
    const s = r.start();
    await turn(r, s, `write ${path.join(r.cwd, rel)} code`);
    assert.equal(r.cards.length, 1);
    assert.equal(r.cards[0].canAlwaysAllow, false);
    assert.match(r.cards[0].title, /runs this file as code/);
  });
}

test('runsAsCode from the runner counts too, and a path outside the names', async () => {
  const r = rig({ mode: 'ask', opts: { runsAsCode: (f) => f.endsWith('helper.ts') } });
  const s = r.start();
  await turn(r, s, `write ${path.join(r.cwd, 'helper.ts')} code`);
  assert.equal(r.cards[0].canAlwaysAllow, false);
});

// ---- Commands

test('a command that can run other code gets no Always allow', async () => {
  const r = rig({ mode: 'ask' });
  const s = r.start();
  await turn(r, s, 'exec npm test');
  assert.equal(r.cards[0].name, 'shell');
  assert.equal(r.cards[0].canAlwaysAllow, false);
});

test('a plain command can be always allowed, exactly', async () => {
  const r = rig({ mode: 'ask', answer: () => ({ decision: 'allowAlways' }) });
  const s = r.start();
  await turn(r, s, 'exec ls -la');
  assert.equal(r.cards[0].canAlwaysAllow, true);
  await turn(r, s, 'exec ls -la', 'turn-2');
  assert.equal(r.cards.length, 1);
  await turn(r, s, 'exec ls -l', 'turn-3');
  assert.equal(r.cards.length, 2, 'a different command asks');
});

// ---- Refusals

test('a thread in the sandbox is refused', () => {
  const r = rig({ opts: { confined: true } });
  assert.throws(() => r.start(), /runs in a sandbox, and OpenCode can't/);
});

test('with the experiment off it refuses to start', () => {
  const r = rig({ on: false });
  assert.throws(() => r.start(), /acpAgents experiment/);
});

test("the wrong program under the agent's name is refused", async () => {
  const r = rig({ env: { FAKE_ACP_NAME: 'Grok' }, agent: ACP_AGENTS.cursor });
  r.start();
  await r.exited();
  assert.match(String(r.exitError()), /isn't Cursor/);
});

test('an ACP version it does not speak is refused', async () => {
  const r = rig({ env: { FAKE_ACP_VERSION: '2' } });
  r.start();
  await r.exited();
  assert.match(String(r.exitError()), /version 2/);
});

test('needing a login says how', async () => {
  const r = rig({ env: { FAKE_ACP_NEEDS_LOGIN: '1' } });
  r.start();
  await r.exited();
  assert.match(String(r.exitError()), /opencode auth login/);
});

test('Cursor fails closed if it can\'t be put in a read-only mode', async () => {
  const r = rig({ mode: 'ask', env: { FAKE_ACP_NAME: 'Cursor', FAKE_ACP_NO_SET_MODE: '1' }, agent: ACP_AGENTS.cursor });
  r.start();
  await r.exited();
  assert.match(String(r.exitError()), /ask mode/);
});

test('Cursor runs Ask threads in its read-only mode', async () => {
  const r = rig({ mode: 'ask', env: { FAKE_ACP_NAME: 'Cursor' }, agent: ACP_AGENTS.cursor });
  const s = r.start();
  await turn(r, s, 'hello');
  assert.ok(r.log().some((l) => (l.set as { mode?: string } | undefined)?.mode === 'ask'));
});

test('resume: a session the agent lost starts fresh', async () => {
  const r = rig({ opts: { resume: true, sessionId: 'gone' } });
  r.start();
  await r.exited();
  assert.equal((r.exitError() as Error).name, 'SessionNotFoundError');
});

test('resume goes through session/resume, with no first-message preamble', async () => {
  const r = rig({ opts: { resume: true, sessionId: 'ses-old', instructions: 'SECRET BRIEF' } });
  const s = r.start();
  await turn(r, s, 'hello');
  assert.ok(r.log().some((l) => l.resumed === 'ses-old'));
  assert.ok(!r.log().some((l) => String(l.prompt ?? '').includes('SECRET BRIEF')));
});

// ---- Turns

test('a new session gets its instructions before the first message only', async () => {
  const r = rig({ opts: { instructions: 'THE BRIEF' } });
  const s = r.start();
  await turn(r, s, 'hello');
  await turn(r, s, 'hello', 'turn-2');
  const prompts = r.log().filter((l) => l.prompt).map((l) => String(l.prompt));
  assert.match(prompts[0], /THE BRIEF/);
  assert.doesNotMatch(prompts[1], /THE BRIEF/);
});

test('the reply, its usage and cost come through; messages sent during a turn run next', async () => {
  const r = rig({ mode: 'full' });
  const s = r.start();
  const done = r.nextResult();
  s.send({ id: 'a', text: 'hello', images: [] });
  s.send({ id: 'b', text: 'hello', images: [] });
  await done;
  const [first] = results(r.outputs);
  assert.equal(first.outcome, 'completed');
  assert.equal(first.answers, 'a');
  assert.equal(first.queuedTurns, 1);
  assert.equal(first.totalCostUsd, 0.25);
  await until(() => results(r.outputs).length === 2);
  assert.equal(results(r.outputs)[1].answers, 'b');
  assert.equal((events(r.outputs, 'assistant.text')[0] as { text: string }).text, 'all done');
  assert.ok(r.outputs.some((o) => o.kind === 'context' && o.contextTokens === 1000 && o.contextWindow === 200000));
  assert.ok(r.outputs.some((o) => o.kind === 'cliVersion' && o.version === '9.9.9'));
});

test('Plan first: the last reply is proposed as the plan', async () => {
  const r = rig({ mode: 'plan' });
  const s = r.start();
  await turn(r, s, 'hello');
  assert.ok(r.log().some((l) => (l.set as { mode?: string } | undefined)?.mode === 'plan'));
  assert.ok(r.outputs.some((o) => o.kind === 'planProposed' && o.text === 'all done'));
});

test('Stop cancels the turn and answers an open card as cancelled', async () => {
  const seen: { signal?: AbortSignal } = {};
  const r = rig({
    mode: 'ask',
    answer: (req) =>
      new Promise((res) => {
        seen.signal = req.signal;
        req.signal.addEventListener('abort', () => res({ decision: 'expired' }));
      }),
  });
  const s = r.start();
  const file = path.join(r.cwd, 'a.txt');
  s.send({ id: 'a', text: `write ${file} hi`, images: [] });
  await until(() => r.cards.length === 1);
  await s.interrupt();
  await until(() => r.log().some((l) => l.cancelled));
  assert.equal(seen.signal?.aborted, true);
  assert.equal(fs.existsSync(file), false);
  assert.deepEqual((r.log().find((l) => l.answered) as { answered: unknown }).answered, { outcome: { outcome: 'cancelled' } });
});

test('models come from session/new', async () => {
  const r = rig();
  const models = await r.adapter.models(r.account);
  assert.deepEqual(models.map((m) => m.id), ['fake/one', 'fake/two']);
});

test('modelChoices reads named groups too', () => {
  const opts = [{ category: 'model', options: [{ group: 'a', options: [{ value: 'x', name: 'X' }] }, { value: 'y' }] }];
  assert.deepEqual(modelChoices(opts).map((m) => [m.id, m.title]), [['x', 'X'], ['y', 'y']]);
});

// ---- Which program is it

test('a binary counts as Cursor only by where it really lives', () => {
  const d = tmp();
  made.push(d);
  const real = path.join(d, 'cursor-agent', 'versions', '1', 'cursor-agent');
  fs.mkdirSync(path.dirname(real), { recursive: true });
  fs.writeFileSync(real, '#!/bin/sh\n', { mode: 0o755 });
  const grok = path.join(d, 'grok');
  fs.writeFileSync(grok, '#!/bin/sh\n', { mode: 0o755 });
  const good = path.join(d, 'good');
  fs.symlinkSync(real, good);
  const bad = path.join(d, 'agent');
  fs.symlinkSync(grok, bad);
  assert.equal(isCursorBinary(good), true);
  assert.equal(isCursorBinary(bad), false, "an `agent` that's Grok's isn't Cursor");
});

test("Cursor isn't offered when the only `agent` is another program's", () => {
  const home = tmp();
  made.push(home);
  fs.mkdirSync(path.join(home, '.local', 'bin'), { recursive: true });
  fs.mkdirSync(path.join(home, '.cursor'));
  fs.writeFileSync(path.join(home, '.cursor', 'cli-config.json'), '{}');
  const grok = path.join(home, 'grok');
  fs.writeFileSync(grok, '#!/bin/sh\n', { mode: 0o755 });
  fs.symlinkSync(grok, path.join(home, '.local', 'bin', 'agent'));
  assert.deepEqual(acpAccounts({ home, pathEnv: path.join(home, '.local', 'bin') }).map((a) => a.id), []);
  const dir = path.join(home, '.local', 'share', 'cursor-agent', 'versions', '1');
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'cursor-agent'), '#!/bin/sh\n', { mode: 0o755 });
  fs.rmSync(path.join(home, '.local', 'bin', 'agent'));
  fs.symlinkSync(path.join(dir, 'cursor-agent'), path.join(home, '.local', 'bin', 'agent'));
  assert.deepEqual(acpAccounts({ home, pathEnv: path.join(home, '.local', 'bin') }).map((a) => a.id), ['cursor-main']);
});

// ---- The MCP endpoint

function mcp(o: { mode?: PermissionMode; answer?: ToolApprovalAnswer['decision'] } = {}) {
  const calls: string[] = [];
  const cards: ToolApprovalRequest[] = [];
  const tool = (name: string, readOnly = false) => ({
    name,
    description: name,
    inputSchema: { what: z.string().optional() },
    annotations: readOnly ? { readOnlyHint: true } : undefined,
    handler: async () => {
      calls.push(name);
      return { content: [{ type: 'text', text: 'ok' }] };
    },
  });
  const server = new AcpMcpServer([tool('delete_thread'), tool('list_threads', true), tool('create_thread')], {
    log: nullLogger,
    label: 't',
    requestApproval: async (r) => {
      cards.push(r);
      return { decision: o.answer ?? 'allow' };
    },
    cwd: () => '/tmp',
    mode: () => o.mode ?? 'full',
    rules: () => [],
    addRules: () => undefined,
  });
  return { server, calls, cards };
}

async function post(url: string, body: unknown, headers: Record<string, string>): Promise<{ status: number; json?: any }> {
  const u = new URL(url);
  return new Promise((resolve, reject) => {
    const req = http.request({ host: u.hostname, port: u.port, path: u.pathname, method: 'POST', headers: { 'content-type': 'application/json', ...headers } }, (res) => {
      let d = '';
      res.on('data', (c) => (d += c));
      res.on('end', () => resolve({ status: res.statusCode ?? 0, json: d.startsWith('{') ? JSON.parse(d) : undefined }));
    });
    req.on('error', reject);
    req.end(JSON.stringify(body));
  });
}

const callBody = (name: string) => ({ jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name, arguments: {} } });

test('MCP: delete_thread asks even in Full', async () => {
  const m = mcp({ mode: 'full', answer: 'deny' });
  const url = await m.server.listen();
  const r = await post(url, callBody('delete_thread'), { authorization: `Bearer ${m.server.token}` });
  m.server.close();
  assert.equal(m.cards.length, 1);
  assert.equal(m.cards[0].canAlwaysAllow, false);
  assert.deepEqual(m.calls, [], 'denied: it never ran');
  assert.equal(r.json.result.isError, true);
});

test('MCP: Ask puts up a card for a tool that changes something, not for a read', async () => {
  const m = mcp({ mode: 'ask' });
  const url = await m.server.listen();
  const auth = { authorization: `Bearer ${m.server.token}` };
  await post(url, callBody('list_threads'), auth);
  assert.equal(m.cards.length, 0);
  await post(url, callBody('create_thread'), auth);
  m.server.close();
  assert.equal(m.cards.length, 1);
  assert.deepEqual(m.calls, ['list_threads', 'create_thread']);
});

test('MCP: no token, a wrong token, a browser or a rebound Host are all turned away', async () => {
  const m = mcp();
  const url = await m.server.listen();
  const port = new URL(url).port;
  assert.equal((await post(url, callBody('list_threads'), {})).status, 401);
  assert.equal((await post(url, callBody('list_threads'), { authorization: 'Bearer nope' })).status, 401);
  const good = { authorization: `Bearer ${m.server.token}` };
  assert.equal((await post(url, callBody('list_threads'), { ...good, origin: 'https://evil.example' })).status, 403);
  assert.equal((await post(url, callBody('list_threads'), { ...good, host: `evil.example:${port}` })).status, 403);
  assert.deepEqual(m.calls, []);
  const ok = await post(url, callBody('list_threads'), good);
  m.server.close();
  assert.equal(ok.status, 200);
  assert.deepEqual(m.calls, ['list_threads']);
});

test('MCP: tools/list and an unknown tool answer as MCP says', async () => {
  const m = mcp();
  const url = await m.server.listen();
  const auth = { authorization: `Bearer ${m.server.token}` };
  const list = await post(url, { jsonrpc: '2.0', id: 2, method: 'tools/list' }, auth);
  assert.deepEqual(list.json.result.tools.map((t: { name: string }) => t.name), ['delete_thread', 'list_threads', 'create_thread']);
  const nope = await post(url, callBody('nope'), auth);
  m.server.close();
  assert.equal(nope.json.result.isError, true);
});

// ---- Each of these is something a review got past the first version

function fakeRequest(r: ReturnType<typeof rig>, s: AcpSession, call: Record<string, unknown>, options = [{ optionId: 'once', kind: 'allow_once' }, { optionId: 'reject', kind: 'reject_once' }]) {
  const replies: unknown[] = [];
  const proc = (s as unknown as { proc: { respond(id: unknown, v: unknown): void; onRequest: (id: number, m: string, p: unknown) => void } }).proc;
  const real = proc.respond.bind(proc);
  proc.respond = (id, v) => {
    if (id === 900) replies.push(v);
    else real(id, v);
  };
  proc.onRequest(900, 'session/request_permission', { toolCall: { toolCallId: 'x', ...call }, options });
  return replies;
}

async function ready(r: ReturnType<typeof rig>, s: AcpSession) {
  await turn(r, s, 'hello', 'first');
}

test('a code file named only in the tool\'s arguments still asks, past an earlier Always allow', async () => {
  const r = rig({ mode: 'ask', opts: { allowRules: [EDIT_RULE] }, deny: true });
  const s = r.start();
  await ready(r, s);
  fakeRequest(r, s, { kind: 'edit', locations: [{ path: 'notes.txt' }], rawInput: { filePath: path.join(r.cwd, '.claude', 'settings.json') } });
  await until(() => r.cards.length === 1);
  assert.equal(r.cards[0].canAlwaysAllow, false);
});

test('a move to a code file asks, by its destination', async () => {
  const r = rig({ mode: 'ask', opts: { allowRules: [EDIT_RULE] }, deny: true });
  const s = r.start();
  await ready(r, s);
  fakeRequest(r, s, { kind: 'move', locations: [{ path: path.join(r.cwd, 'a.txt') }], rawInput: { source: 'a.txt', destination: path.join(r.cwd, '.mcp.json') } });
  await until(() => r.cards.length === 1);
  assert.match(r.cards[0].title, /runs this file as code/);
});

test('an edit naming no file asks every time', async () => {
  const r = rig({ mode: 'ask', opts: { allowRules: [EDIT_RULE] }, deny: true });
  const s = r.start();
  await ready(r, s);
  fakeRequest(r, s, { kind: 'edit', locations: [] });
  await until(() => r.cards.length === 1);
  assert.equal(r.cards[0].canAlwaysAllow, false);
});

test('a saved command rule covers its own folder only, and the card says which', async () => {
  const r = rig({ mode: 'ask', deny: true });
  const s = r.start();
  await ready(r, s);
  const project = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-acp-elsewhere-')));
  fakeRequest(r, s, { kind: 'execute', rawInput: { command: 'ls -la', workdir: project } });
  await until(() => r.cards.length === 1);
  assert.equal((r.cards[0].input as { cwd: string }).cwd, project);
  // One that isn't there can't be bound to anything: shown as given, never always allowed.
  fakeRequest(r, s, { toolCallId: 'y', kind: 'execute', rawInput: { command: 'ls -la', workdir: '/elsewhere/project' } });
  await until(() => r.cards.length === 2);
  assert.equal((r.cards[1].input as { workdir: string }).workdir, '/elsewhere/project');
  assert.equal(r.cards[1].canAlwaysAllow, false);
  fs.rmSync(project, { recursive: true, force: true });
});

// Daybreak, 10 Nov (finding 69): an Always allow for a command in the
// thread's folder covered the same command anywhere. A rule is for that
// command in the folder it really runs in; a call that names two folders, or
// carries anything else that could change what runs, isn't covered by one.
test('a saved command rule covers that folder only: another, two at once, or anything else that changes what runs asks, showing it', async () => {
  const r = rig({ mode: 'ask', deny: true });
  const s = r.start();
  await s.setPermissionMode('ask');
  const command = 'rm -rf generated';
  // Rules are kept for the folder as the disk has it (tmp is under a link on macOS).
  const home = fs.realpathSync(r.cwd);
  (s as unknown as { rules: Set<string> }).rules.add(commandRule(['rm', '-rf', 'generated'], home));
  await ready(r, s);
  const allowed = { outcome: { outcome: 'selected', optionId: 'once' } };
  // Its own folder, unnamed or named: the rule answers, no card.
  for (const [id, rawInput] of [['a1', { command, description: 'clean up' }], ['a2', { command, workdir: r.cwd, timeout: 60 }], ['a3', { command, cwd: '.' }]] as const) {
    const replies = fakeRequest(r, s, { toolCallId: id, kind: 'execute', rawInput });
    await until(() => replies.length === 1);
    assert.deepEqual(replies[0], allowed, id);
  }
  assert.equal(r.cards.length, 0);
  // Another folder inside the project: a card, saying where.
  const other = path.join(home, 'packages', 'app');
  fs.mkdirSync(path.join(other, 'deep'), { recursive: true });
  const b = fakeRequest(r, s, { toolCallId: 'b', kind: 'execute', rawInput: { command, workdir: other } });
  await until(() => b.length === 1);
  assert.equal(r.cards.length, 1);
  assert.equal((r.cards[0].input as { cwd: string }).cwd, other);
  assert.equal(r.cards[0].canAlwaysAllow, true, 'a rule for that folder can be saved');
  // Two folders that disagree: which one it runs in can't be known.
  const c = fakeRequest(r, s, { toolCallId: 'c', kind: 'execute', rawInput: { command, workdir: r.cwd, cwd: '/elsewhere' } });
  await until(() => c.length === 1);
  assert.equal(r.cards.length, 2);
  assert.equal(r.cards[1].canAlwaysAllow, false);
  assert.deepEqual(r.cards[1].input, { command, workdir: r.cwd, cwd: '/elsewhere' });
  // Through a link: `link/..` is packages/app on the disk, though it reads as
  // the thread's folder, so the rule doesn't cover it (Codex's review, 10 Nov).
  fs.symlinkSync(path.join(other, 'deep'), path.join(home, 'link'));
  for (const [id, workdir] of [['l1', path.join(r.cwd, 'link') + '/..'], ['l2', 'link/..']] as const) {
    const before: number = r.cards.length;
    const replies = fakeRequest(r, s, { toolCallId: id, kind: 'execute', rawInput: { command, workdir } });
    await until(() => replies.length === 1);
    assert.notDeepEqual(replies[0], allowed, id);
    assert.equal(r.cards.length, before + 1, id);
    assert.equal(r.cards.at(-1)!.canAlwaysAllow, false, `${id}: which folder it is depends on who reads the path`);
  }
  // A rule saved for a link's folder doesn't follow the link somewhere new.
  (s as unknown as { rules: Set<string> }).rules.add(commandRule(['rm', '-rf', 'generated'], path.join(other, 'deep')));
  const viaLink = fakeRequest(r, s, { toolCallId: 'l3', kind: 'execute', rawInput: { command, workdir: path.join(home, 'link') } });
  await until(() => viaLink.length === 1);
  assert.deepEqual(viaLink[0], allowed, 'the link, where it points now, is the folder the rule is for');
  fs.rmSync(path.join(home, 'link'));
  fs.symlinkSync(path.join(home, 'packages'), path.join(home, 'link'));
  const moved = fakeRequest(r, s, { toolCallId: 'l4', kind: 'execute', rawInput: { command, workdir: path.join(home, 'link') } });
  await until(() => moved.length === 1);
  assert.notDeepEqual(moved[0], allowed, 'pointed somewhere new, it asks');
  // Anything else that could change what runs: an env, a folder by another name.
  for (const [id, extra] of [['d1', { env: { PATH: '/tmp/evil' } }], ['d2', { path: '/elsewhere' }], ['d3', { workdir: ['/a'] }]] as const) {
    const before: number = r.cards.length;
    const replies = fakeRequest(r, s, { toolCallId: id, kind: 'execute', rawInput: { command, ...extra } });
    await until(() => replies.length === 1);
    assert.notDeepEqual(replies[0], allowed, id);
    assert.equal(r.cards.length, before + 1, id);
    const card = r.cards.at(-1)!;
    assert.equal(card.canAlwaysAllow, false, id);
    for (const k of Object.keys(extra)) assert.ok(k in card.input, `${id}: ${k} on the card`);
  }
});

// Codex's second look, 10 Nov: the card shows the folder a link resolves to,
// so a link pointed somewhere else while the card waits isn't allowed there.
test('a folder that changes while its card waits is refused, and the card shows the link as given', async () => {
  let swap = () => {};
  const r = rig({ mode: 'ask', answer: async () => (swap(), { decision: 'allowAlways' }) });
  const s = r.start();
  await s.setPermissionMode('ask');
  await ready(r, s);
  const home = fs.realpathSync(r.cwd);
  fs.mkdirSync(path.join(home, 'safe'));
  fs.mkdirSync(path.join(home, 'valuable'));
  const link = path.join(home, 'link');
  fs.symlinkSync(path.join(home, 'safe'), link);
  swap = () => {
    fs.rmSync(link);
    fs.symlinkSync(path.join(home, 'valuable'), link);
  };
  const replies = fakeRequest(r, s, { toolCallId: 't1', kind: 'execute', rawInput: { command: 'rm -rf generated', workdir: link } });
  await until(() => replies.length === 1);
  assert.deepEqual(r.cards[0].input, { command: 'rm -rf generated', cwd: path.join(home, 'safe'), workdir: link });
  assert.deepEqual(replies[0], { outcome: { outcome: 'selected', optionId: 'reject' } });
  assert.ok(r.outputs.some((o: any) => o.kind === 'event' && o.event.type === 'system.note' && /changed while it waited/.test(o.event.text)));
  assert.equal((s as unknown as { rules: Set<string> }).rules.size, 0, 'no rule saved for it');
  // Left alone, the same answer allows it.
  swap = () => {};
  const again = fakeRequest(r, s, { toolCallId: 't2', kind: 'execute', rawInput: { command: 'rm -rf generated', workdir: link } });
  await until(() => again.length === 1);
  assert.deepEqual(again[0], { outcome: { outcome: 'selected', optionId: 'once' } });
});

test('a saved command rule does not cover a file that runs as code', async () => {
  const r = rig({ mode: 'ask', deny: true });
  const s = r.start();
  const argv = ['ls', '-la'];
  await s.setPermissionMode('ask');
  (s as unknown as { rules: Set<string> }).rules.add(commandRule(argv, r.cwd));
  await ready(r, s);
  fakeRequest(r, s, { kind: 'execute', locations: [{ path: path.join(r.cwd, '.mcp.json') }], rawInput: { command: 'ls -la' } });
  await until(() => r.cards.length === 1);
  assert.equal(r.cards[0].canAlwaysAllow, false);
});

test('Stop during the setup before a prompt: the prompt is never sent', async () => {
  const r = rig({ mode: 'full', env: { FAKE_ACP_SLOW_SET: '300' } });
  const s = r.start();
  await ready(r, s);
  const before = r.log().filter((l) => l.prompt).length;
  await s.setModel('fake/two');
  const done = r.nextResult();
  s.send({ id: 'stopme', text: 'hello', images: [] });
  await until(() => (s as unknown as { turn?: unknown }).turn !== undefined);
  await s.interrupt();
  await done;
  assert.equal(r.log().filter((l) => l.prompt).length, before);
  assert.equal(results(r.outputs).at(-1)!.outcome, 'interrupted');
});

test('Stop with a message waiting gives one result, when the turn is over', async () => {
  const r = rig({ mode: 'ask', answer: (req) => new Promise((res) => req.signal.addEventListener('abort', () => res({ decision: 'expired' }))) });
  const s = r.start();
  const file = path.join(r.cwd, 'a.txt');
  const done = r.nextResult();
  s.send({ id: 'a', text: `write ${file} hi`, images: [] });
  s.send({ id: 'b', text: 'hello', images: [] });
  await until(() => r.cards.length === 1);
  await s.interrupt();
  assert.equal(results(r.outputs).length, 0, 'nothing reported while A is still going');
  await done;
  const [only] = results(r.outputs);
  assert.deepEqual(only.answered, ['a', 'b']);
  assert.equal(only.answers, 'a');
});

test('a mode change during a turn reaches the agent straight away', async () => {
  const r = rig({ mode: 'full', env: { FAKE_ACP_NAME: 'Cursor' }, agent: ACP_AGENTS.cursor });
  const s = r.start();
  await ready(r, s);
  await s.setPermissionMode('ask');
  assert.ok(r.log().some((l) => (l.set as { mode?: string } | undefined)?.mode === 'ask'));
});

test('a malformed request is cancelled, never left to hang the turn', async () => {
  const r = rig({ mode: 'ask' });
  const s = r.start();
  await ready(r, s);
  const replies = fakeRequest(r, s, { kind: 'edit', locations: [{ path: path.join(r.cwd, 'a') }], content: [{ type: 'diff', path: path.join(r.cwd, 'a'), oldText: 123, newText: 'x' }] });
  await until(() => r.cards.length === 1 || replies.length === 1);
  assert.ok(r.cards.length === 1 || (replies[0] as { outcome: { outcome: string } }).outcome.outcome === 'cancelled');
});

test('with the experiment turned off, a live session stops taking turns', async () => {
  let on = true;
  const r = rig({ mode: 'full' });
  (r.adapter as unknown as { deps: { on: () => boolean } }).deps.on = () => on;
  const s = r.start();
  await ready(r, s);
  on = false;
  await turn(r, s, 'hello', 'off');
  assert.match(String(results(r.outputs).at(-1)!.error), /acpAgents/);
});

test('MCP: a cancelled call closes its card', async () => {
  let signal: AbortSignal | undefined;
  const server = new AcpMcpServer([{ name: 'create_thread', description: 'x', inputSchema: {}, handler: async () => ({ content: [] }) }], {
    log: nullLogger,
    label: 't',
    requestApproval: (req) =>
      new Promise((res) => {
        signal = req.signal;
        req.signal.addEventListener('abort', () => res({ decision: 'expired' }));
      }),
    cwd: () => '/tmp',
    mode: () => 'ask',
    rules: () => [],
    addRules: () => undefined,
  });
  const url = await server.listen();
  const auth = { authorization: `Bearer ${server.token}` };
  const call = post(url, callBody('create_thread'), auth);
  await until(() => signal !== undefined);
  await post(url, { jsonrpc: '2.0', method: 'notifications/cancelled', params: { requestId: 1 } }, auth);
  const r = await call;
  server.close();
  assert.equal(signal?.aborted, true);
  assert.equal(r.json.result.isError, true);
});

test('MCP: a card can never approve less than the call runs', async () => {
  let cards = 0;
  const calls: string[] = [];
  const server = new AcpMcpServer([{ name: 'send_message', description: 'x', inputSchema: { text: z.string() }, handler: async () => (calls.push('ran'), { content: [] }) }], {
    log: nullLogger,
    label: 't',
    requestApproval: async () => (cards++, { decision: 'allow' }),
    cwd: () => '/tmp',
    mode: () => 'ask',
    rules: () => [],
    addRules: () => undefined,
  });
  const url = await server.listen();
  const huge = 'x'.repeat(2_000_000);
  const r = await post(url, { jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name: 'send_message', arguments: { text: huge } } }, { authorization: `Bearer ${server.token}` });
  server.close();
  assert.equal(cards, 0, 'no card that would have been cut short');
  assert.deepEqual(calls, []);
  assert.equal(r.json.result.isError, true);
});

for (const [what, rawInput] of [
  ['a nested list of changes', { changes: [{ path: '.mcp.json', newText: 'x' }] }],
  ['a patch body', { patchText: '*** Begin Patch\n*** Update File: .mcp.json\n@@\n-a\n+b\n*** End Patch' }],
  ['a unified diff', { patch: '--- a/.mcp.json\n+++ b/.mcp.json\n@@ -1 +1 @@\n-a\n+b\n' }],
  ['a document key', { document: '.mcp.json', old_string: 'a', new_string: 'b' }],
] as const) {
  test(`a code file inside ${what} still asks, past an earlier Always allow`, async () => {
    const r = rig({ mode: 'ask', opts: { allowRules: [EDIT_RULE] }, deny: true });
    const s = r.start();
    await ready(r, s);
    fakeRequest(r, s, { kind: 'edit', locations: [{ path: 'notes.txt' }], rawInput });
    await until(() => r.cards.length === 1);
    assert.match(r.cards[0].title, /runs this file as code/);
    assert.equal(r.cards[0].canAlwaysAllow, false);
  });
}

test('an edit of a shape nobody has checked asks every time, and offers no Always allow', async () => {
  const r = rig({ mode: 'ask', opts: { allowRules: [EDIT_RULE] }, deny: true });
  const s = r.start();
  await ready(r, s);
  fakeRequest(r, s, { kind: 'edit', locations: [{ path: 'notes.txt' }], rawInput: { filePath: 'notes.txt', extra: { nested: 1 } } });
  await until(() => r.cards.length === 1);
  assert.equal(r.cards[0].canAlwaysAllow, false);
});

test('a code file an earlier update named still counts', async () => {
  const r = rig({ mode: 'ask', opts: { allowRules: [EDIT_RULE] }, deny: true });
  const s = r.start();
  await ready(r, s);
  r.outputs.length = 0;
  (s as unknown as { notification(m: string, p: unknown): void }).notification('session/update', { update: { sessionUpdate: 'tool_call', toolCallId: 'x', title: 'write', kind: 'edit', status: 'in_progress', locations: [{ path: path.join(r.cwd, '.mcp.json') }], rawInput: { filePath: path.join(r.cwd, '.mcp.json') } } });
  fakeRequest(r, s, { kind: 'edit', locations: [{ path: 'notes.txt' }], rawInput: { filePath: 'notes.txt' } });
  await until(() => r.cards.length === 1);
  assert.match(r.cards[0].title, /runs this file as code/);
});

test("a path only in an earlier update's arguments still counts", async () => {
  const r = rig({ mode: 'ask', opts: { allowRules: [EDIT_RULE] }, deny: true });
  const s = r.start();
  await ready(r, s);
  (s as unknown as { notification(m: string, p: unknown): void }).notification('session/update', { update: { sessionUpdate: 'tool_call', toolCallId: 'x', title: 'write', kind: 'edit', status: 'in_progress', rawInput: { filePath: path.join(r.cwd, '.mcp.json'), content: 'x' } } });
  fakeRequest(r, s, { kind: 'edit', locations: [{ path: 'notes.txt' }] });
  await until(() => r.cards.length === 1);
  assert.match(r.cards[0].title, /runs this file as code/);
});

test("a tool of no known kind can't be allowed from the phone, as its arguments may not all show", async () => {
  const r = rig({ mode: 'ask', deny: true });
  const s = r.start();
  await ready(r, s);
  fakeRequest(r, s, { kind: 'other', title: 'Send email', rawInput: { to: 'a@example.com', body: 'not shown' } });
  await until(() => r.cards.length === 1);
  assert.ok(r.cards[0].name.startsWith('mcp__'));
  assert.equal(remoteCardShowsAll(r.cards[0].name, r.cards[0].input, r.cwd, r.cards[0].title), false);
});

test('a quick Full then Ask ends on Ask', async () => {
  const r = rig({ mode: 'ask', env: { FAKE_ACP_NAME: 'Cursor', FAKE_ACP_SLOW_SET: '60' }, agent: ACP_AGENTS.cursor });
  const s = r.start();
  await ready(r, s);
  await Promise.all([s.setPermissionMode('full'), s.setPermissionMode('ask')]);
  const modes = r.log().flatMap((l) => ((l.set as { mode?: string } | undefined)?.mode ? [(l.set as { mode: string }).mode] : []));
  assert.equal(modes.at(-1), 'ask');
});

test('a whole path is needed: a relative one could be a repo\'s own program', () => {
  const d = tmp();
  made.push(d);
  const bin = path.join(d, 'x');
  fs.writeFileSync(bin, '#!/bin/sh\n', { mode: 0o755 });
  assert.equal(ACP_AGENTS.opencode.findBinary({ explicit: bin, home: d, pathEnv: '' }), bin);
  assert.equal(ACP_AGENTS.opencode.findBinary({ explicit: './x', home: d, pathEnv: '' }), undefined);
  assert.equal(ACP_AGENTS.cursor.findBinary({ explicit: './x', home: d, pathEnv: '' }), undefined);
});

test('no protocol version in initialize is refused', async () => {
  const r = rig({ env: { FAKE_ACP_VERSION: 'none' } });
  r.start();
  await r.exited();
  assert.match(String(r.exitError()), /ACP version/);
});
