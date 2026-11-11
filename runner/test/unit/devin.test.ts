// Devin threads: Devin's dialect of the ACP adapter, against hand-made
// updates shaped like `devin acp` 3000.12.32's (docs/devin-plan.md) and a
// fake agent process. Nothing here runs Devin.
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { z } from 'zod';
import { devinAdapter, devinEffortOut, devinMode, devinModels } from '../../src/adapters/devin/adapter.ts';
import type { GrokSession } from '../../src/adapters/grok/adapter.ts';
import { GrokNormalizer, commandsFrom, DEVIN_TERMINAL_ONLY, devinToolOf } from '../../src/adapters/grok/normalize.ts';
import { fileContext } from '../../src/adapters/context.ts';
import { McpHttpServer } from '../../src/mcp-http.ts';
import { SessionNotFoundError, type AdapterOutput, type SessionSink, type StartSessionOptions, type ToolApprovalAnswer, type ToolApprovalRequest, type TurnResult } from '../../src/adapters/types.ts';
import { ProcessRegistry } from '../../src/proc.ts';
import { nullLogger } from '../../src/log.ts';
import type { Account } from '../../src/protocol.ts';
import type { ToolDefinition } from '../../src/adapters/codex/tools.ts';
import { FakeCodex, type FakeConn } from '../helpers/fake-app-server.ts';
import { PATIENCE_MS } from '../helpers/patience.ts';

const dirs: string[] = [];
function tmp(prefix = 'tb-devin-'): string {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  dirs.push(d);
  return d;
}
let shared: Promise<FakeCodex> | undefined;
function fake(): Promise<FakeCodex> {
  shared ??= FakeCodex.create();
  return shared;
}
after(async () => {
  if (shared) await (await shared).close();
  for (const d of dirs) fs.rmSync(d, { recursive: true, force: true });
});

async function waitFor(cond: () => boolean, ms = PATIENCE_MS): Promise<void> {
  const end = Date.now() + ms;
  while (!cond()) {
    if (Date.now() > end) throw new Error('timed out waiting');
    await new Promise((r) => setTimeout(r, 5));
  }
}

const events = (outs: AdapterOutput[]) => outs.flatMap((o) => (o.kind === 'event' ? [o.event] : []));

function norm(cwd = tmp()) {
  return { n: new GrokNormalizer(fileContext(cwd, path.join(cwd, 'media'), path.join(cwd, 'blobs')), { prefix: 'd', name: 'Devin', toolOf: devinToolOf, terminalOnly: DEVIN_TERMINAL_ONLY }), cwd };
}

const meta = (name: string, extra: Record<string, unknown> = {}) => ({ 'cognition.ai/inferenceToolName': name, ...extra });

// ---- Tools and updates

test("Devin's tools take the names Threadbear knows", () => {
  const t = (u: Record<string, unknown>) => devinToolOf(u).tool;
  assert.deepEqual(t({ title: 'Ran echo', kind: 'execute', rawInput: { command: 'echo hi' }, _meta: meta('exec') }), { name: 'Bash', kind: 'command', input: { command: 'echo hi' } });
  assert.deepEqual(t({ title: 'Read file', kind: 'read', rawInput: { file_path: '/w/a.txt' }, _meta: meta('read') }), { name: 'Read', kind: 'read', input: { file_path: '/w/a.txt' } });
  assert.deepEqual(t({ kind: 'edit', rawInput: { content: 'x', file_path: '/w/a.txt' }, _meta: meta('write') }), { name: 'Write', kind: 'write', input: { file_path: '/w/a.txt', content: 'x' } });
  assert.deepEqual(t({ kind: 'edit', rawInput: { file_path: '/w/a.txt', old_string: 'x', new_string: 'y' }, _meta: meta('edit') }), { name: 'Edit', kind: 'edit', input: { file_path: '/w/a.txt', old_string: 'x', new_string: 'y' } });
  const mcp = t({ title: 'Calling say_hello from threadbear', rawInput: { who: 'B' }, _meta: { 'cognition.ai/toolName': 'mcp__threadbear__say_hello', 'cognition.ai/inferenceToolName': 'mcp__threadbear__say_hello' } });
  assert.equal(mcp.name, 'mcp__threadbear__say_hello');
  assert.deepEqual(mcp.input, { who: 'B' });
  assert.equal(t({ title: 'Listed MCP tools', kind: 'search', rawInput: {}, _meta: meta('mcp_list_tools') }).kind, 'search');
});

test("a command's output comes with the updates while it runs; its exit code decides", () => {
  const { n } = norm();
  n.setSession('s1');
  const call = { sessionUpdate: 'tool_call', toolCallId: 'c1', title: 'Listed ./', kind: 'execute', rawInput: { command: 'ls' }, _meta: meta('exec') };
  const outs = [
    ...n.handle(call),
    ...n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'in_progress', content: [{ type: 'content', content: { type: 'text', text: 'a.txt' } }], _meta: meta('exec') }),
    ...n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'in_progress', content: [{ type: 'content', content: { type: 'text', text: 'a.txt\nhi.txt' } }], _meta: { ...meta('exec'), terminal_exit: { exit_code: 0 } } }),
    ...n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'completed', _meta: meta('exec') }),
  ];
  const deltas = outs.flatMap((o) => (o.kind === 'delta' ? [o.delta] : []));
  assert.deepEqual(
    deltas.map((d) => (d as { text: string }).text),
    ['a.txt', '\nhi.txt'],
  );
  const result = events(outs).find((e) => e.type === 'tool.result') as { ok: boolean; text: string };
  assert.deepEqual([result.ok, result.text], [true, 'a.txt\nhi.txt']);

  const failed = [
    ...n.handle({ ...call, toolCallId: 'c2', rawInput: { command: 'false' } }),
    ...n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c2', status: 'in_progress', content: [{ type: 'content', content: { type: 'text', text: 'boom' } }], _meta: { ...meta('exec'), terminal_exit: { exit_code: 1 } } }),
    ...n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c2', status: 'completed', _meta: meta('exec') }),
  ];
  const r2 = events(failed).find((e) => e.type === 'tool.result') as { ok: boolean; text: string };
  assert.deepEqual([r2.ok, r2.text], [false, 'boom']);
});

test("an edit shows Devin's diff; the context meter and the turn's usage come from ACP's own fields", () => {
  const { n, cwd } = norm();
  n.setSession('s1');
  const file = path.join(cwd, 'a.txt');
  fs.writeFileSync(file, 'x');
  const diffContent = [{ type: 'diff', path: file, oldText: 'x', newText: 'y' }];
  const outs = [
    ...n.handle({ sessionUpdate: 'tool_call', toolCallId: 'e1', title: 'Edit file', kind: 'edit', content: diffContent, rawInput: { file_path: file, old_string: 'x', new_string: 'y' }, _meta: meta('edit') }),
    ...n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'e1', status: 'in_progress', content: diffContent, _meta: meta('edit') }),
  ];
  const diff = events(outs).find((e) => e.type === 'edit.diff') as { path: string; hunks: unknown[] };
  assert.equal(diff.path, file);
  assert.ok(diff.hunks.length > 0);
  const ctx = n.handle({ sessionUpdate: 'usage_update', used: 30371, size: 262144 });
  assert.deepEqual(ctx, [{ kind: 'context', contextTokens: 30371, contextWindow: 262144 }]);
  const end = n.finish({ stopReason: 'end_turn', usage: { totalTokens: 35608, inputTokens: 35604, outputTokens: 4, cachedReadTokens: 128 } } as never, 10);
  const r = end.find((o) => o.kind === 'result') as Extract<AdapterOutput, { kind: 'result' }>;
  assert.deepEqual(r.result.usage, { inputTokens: 35604, cachedInputTokens: 128, cacheWriteTokens: 0, outputTokens: 4, reasoningTokens: 0 });
  const refused = n.finish({ stopReason: 'refusal' }, 1).find((o) => o.kind === 'result') as Extract<AdapterOutput, { kind: 'result' }>;
  assert.equal(refused.result.error, 'Devin refused to go on with this.');
});

test("Devin's commands, without its login, modes and terminal-only ones", () => {
  const names = commandsFrom([{ name: 'login' }, { name: 'compact', description: 'Compact' }, { name: 'plan' }, { name: 'recap' }, { name: 'bypass' }], DEVIN_TERMINAL_ONLY).map((c) => c.name);
  assert.deepEqual(names, ['compact', 'recap']);
});

const CONFIG = [
  { id: 'mode', category: 'mode', type: 'select', currentValue: 'accept-edits', options: [{ value: 'accept-edits', name: 'Code' }, { value: 'plan', name: 'Plan' }, { value: 'bypass', name: 'Bypass Permissions' }] },
  { id: 'model', category: 'model', type: 'select', currentValue: 'swe-2-high', options: [{ value: 'swe-2-high', name: 'SWE-2 High' }, { value: 'swe-2-flash-medium', name: 'SWE-2 Flash Medium', description: 'Fast' }] },
  { id: 'thought_level', category: 'thought_level', type: 'select', currentValue: 'high', options: [{ value: 'medium' }, { value: 'high' }, { value: 'max' }] },
];

test("models and effort levels come from the session's config options", () => {
  const m = devinModels({ configOptions: CONFIG });
  assert.equal(m.current, 'swe-2-high');
  assert.deepEqual(m.rows, [
    { id: 'swe-2-high', model: 'swe-2-high', title: 'SWE-2 High', effortLevels: ['medium', 'high', 'max'] },
    { id: 'swe-2-flash-medium', model: 'swe-2-flash-medium', title: 'SWE-2 Flash Medium', description: 'Fast', effortLevels: ['medium', 'high', 'max'] },
  ]);
  assert.equal(devinEffortOut('xhigh'), 'max');
  assert.deepEqual((['full', 'ask', 'auto', 'plan'] as const).map(devinMode), ['bypass', 'accept-edits', 'smart', 'plan']);
});

// ---- The session, against a fake devin

const account: Account = { id: 'devin-main', provider: 'devin', title: 'Devin · devin-main', configDir: '/work/devin-data' };

function recorder() {
  const outputs: AdapterOutput[] = [];
  const approvals: ToolApprovalRequest[] = [];
  let exitResolve!: (e: Error | undefined) => void;
  const exited = new Promise<Error | undefined>((r) => (exitResolve = r));
  const state = { approve: async (_req: ToolApprovalRequest): Promise<ToolApprovalAnswer> => ({ decision: 'allow' }) };
  const sink: SessionSink = {
    output: (o) => outputs.push(o),
    requestApproval: (req) => {
      approvals.push(req);
      return state.approve(req);
    },
    requestQuestion: async () => ({ answers: null }),
    exited: (e) => exitResolve(e),
  };
  return { sink, outputs, approvals, exited, state, results: (): TurnResult[] => outputs.filter((o): o is Extract<AdapterOutput, { kind: 'result' }> => o.kind === 'result').map((o) => o.result), kinds: () => outputs.map((o) => o.kind) };
}

function echoTool(): ToolDefinition {
  return { name: 'echo', description: 'Says it back', inputSchema: { text: z.string() }, handler: async (args: { text: string }) => ({ content: [{ type: 'text', text: `said ${args.text}` }] }) };
}

async function open(o: Partial<StartSessionOptions> & { tools?: ToolDefinition[] } = {}) {
  const f = await fake();
  const rec = recorder();
  const home = tmp();
  const mcp = new McpHttpServer(nullLogger);
  const { tools, ...rest } = o;
  const adapter = devinAdapter({
    devinBinary: () => f.bin,
    registry: new ProcessRegistry(home, nullLogger),
    log: nullLogger,
    mcp,
    toolsFor: () => tools ?? [],
    timings: { spawnRetryForMs: PATIENCE_MS, closeWaitMs: 2000 },
  });
  const opts: StartSessionOptions = {
    threadId: 'tb-1',
    cwd: home,
    account,
    permissionMode: 'full',
    sessionId: 'placeholder',
    resume: false,
    mediaDir: path.join(home, 'media'),
    blobsDir: path.join(home, 'blobs'),
    tmpDir: home,
    env: { THREADBEAR_THREAD_ID: 'tb-1' },
    ...rest,
  };
  const session = adapter.start(opts, rec.sink) as GrokSession;
  const conn = await f.next();
  const req = await conn.request('initialize');
  conn.respond(req.id, { protocolVersion: 1, agentCapabilities: { loadSession: true, mcpCapabilities: { http: true } }, agentInfo: { name: 'affogato', version: '0.0.0-dev' } });
  return { session, conn, rec, home, mcp, opts, adapter };
}

// session/new's answer, then the mode the thread's permission mode asks
// for (none when Devin already starts in it).
async function newSession(h: Awaited<ReturnType<typeof open>>, mode: string | null = 'bypass') {
  const req = await h.conn.request('session/new');
  h.conn.respond(req.id, { sessionId: 'sweet-otter', modes: { currentModeId: 'accept-edits' }, configOptions: CONFIG });
  if (mode) {
    const m = await h.conn.request('session/set_config_option');
    assert.deepEqual(m.params, { sessionId: 'sweet-otter', configId: 'mode', value: mode });
    h.conn.respond(m.id, { configOptions: CONFIG });
  }
  await waitFor(() => h.rec.kinds().includes('session'));
  return req;
}

async function close(h: Awaited<ReturnType<typeof open>>) {
  await h.session.end();
  await h.mcp.close();
}

test('a new Devin session: `devin acp`, then session/new, the mode, session and effort', async () => {
  const h = await open();
  try {
    assert.deepEqual(h.conn.hello.argv, ['acp']);
    assert.equal(h.conn.hello.env.THREADBEAR_THREAD_ID, 'tb-1');
    assert.equal(h.conn.hello.env.GROK_HOME, undefined);
    const req = await newSession(h);
    assert.equal(req.params.cwd, h.home);
    assert.equal(req.params._meta, undefined);
    assert.deepEqual(
      h.rec.outputs.filter((o) => o.kind !== 'event'),
      [
        { kind: 'session', nativeSessionId: 'sweet-otter', model: 'swe-2-high' },
        { kind: 'effort', level: 'high' },
      ],
    );
    assert.equal(h.adapter.provider, 'devin');
  } finally {
    await close(h);
  }
});

test("Threadbear's instructions lead the first prompt only, as Devin ignores _meta rules", async () => {
  const h = await open({ tools: [echoTool()], instructions: 'The project brief.' });
  try {
    const req = await newSession(h);
    const servers = req.params.mcpServers as { type: string; name: string }[];
    assert.deepEqual(servers.map((s) => [s.type, s.name]), [['http', 'threadbear']]);
    h.session.send({ id: 't1', text: 'say hi', images: [] });
    const p = await h.conn.request('session/prompt');
    const blocks = p.params.prompt as { type: string; text: string }[];
    assert.equal(blocks.length, 2);
    assert.match(blocks[0].text, /^<threadbear_instructions>\n[\s\S]*end_turn[\s\S]*The project brief\.\n<\/threadbear_instructions>/);
    assert.deepEqual(blocks[1], { type: 'text', text: 'say hi' });
    h.conn.respond(p.id, { stopReason: 'end_turn', usage: { inputTokens: 10, outputTokens: 2 } });
    await waitFor(() => h.rec.results().length === 1);
    h.session.send({ id: 't2', text: 'again', images: [] });
    const p2 = await h.conn.request('session/prompt');
    assert.deepEqual(p2.params.prompt, [{ type: 'text', text: 'again' }]);
    h.conn.respond(p2.id, { stopReason: 'end_turn' });
    await waitFor(() => h.rec.results().length === 2);
  } finally {
    await close(h);
  }
});

test("changing the thread's permission mode sets Devin's", async () => {
  const h = await open({ permissionMode: 'ask' });
  try {
    await newSession(h, null);
    const changed = h.session.setPermissionMode('plan');
    const m = await h.conn.request('session/set_config_option');
    assert.deepEqual(m.params, { sessionId: 'sweet-otter', configId: 'mode', value: 'plan' });
    h.conn.respond(m.id, { configOptions: CONFIG });
    await changed;
  } finally {
    await close(h);
  }
});

test("a resume loads the session and drops Devin's replay of it", async () => {
  const h = await open({ resume: true, sessionId: 'sweet-otter' });
  try {
    const req = await h.conn.request('session/load');
    assert.equal(req.params.sessionId, 'sweet-otter');
    h.conn.notify('session/update', { sessionId: 'sweet-otter', update: { sessionUpdate: 'user_message_chunk', content: { type: 'text', text: 'old question' } } });
    h.conn.notify('session/update', { sessionId: 'sweet-otter', update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'old answer' } } });
    await new Promise((r) => setTimeout(r, 50));
    h.conn.respond(req.id, { modes: { currentModeId: 'bypass' }, configOptions: CONFIG });
    await waitFor(() => h.rec.kinds().includes('session'));
    assert.deepEqual(h.rec.outputs.filter((o) => o.kind === 'delta' || o.kind === 'event'), []);
    h.session.send({ id: 't1', text: 'new question', images: [] });
    const p = await h.conn.request('session/prompt');
    assert.deepEqual(p.params.prompt, [{ type: 'text', text: 'new question' }]);
    h.conn.notify('session/update', { sessionId: 'sweet-otter', update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'new answer' } } });
    h.conn.respond(p.id, { stopReason: 'end_turn' });
    await waitFor(() => h.rec.results().length === 1);
    const texts = events(h.rec.outputs).flatMap((e) => (e.type === 'assistant.text' ? [e.text] : []));
    assert.deepEqual(texts, ['new answer']);
  } finally {
    await close(h);
  }
});

test("a resume of a session Devin doesn't have ends with SessionNotFoundError", async () => {
  const h = await open({ resume: true, sessionId: 'gone' });
  const req = await h.conn.request('session/load');
  h.conn.write({ jsonrpc: '2.0', id: req.id, error: { code: -32016, message: 'Session not found', data: { 'cognition.ai/errorKind': 'session_not_found', 'cognition.ai/retryable': false } } });
  const err = await h.rec.exited;
  assert.ok(err instanceof SessionNotFoundError);
  await h.mcp.close();
});
