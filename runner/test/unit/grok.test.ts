// Grok threads (docs/grok-plan.md): the normaliser on updates shaped like
// grok 1.0.77's, the MCP server the threadbear tools go through, and the
// session against a fake `grok agent stdio` (the Codex tests' relay).
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { z } from 'zod';
import { GrokAdapter, GrokSession, grokModelChoices, promptBlocks, ruleFor } from '../../src/adapters/grok/adapter.ts';
import { GrokNormalizer, commandsFrom, errorKind, mapTool } from '../../src/adapters/grok/normalize.ts';
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
function tmp(prefix = 'tb-grok-'): string {
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
  return { n: new GrokNormalizer(fileContext(cwd, path.join(cwd, 'media'), path.join(cwd, 'blobs')), { prefix: 'g' }), cwd };
}

const bashCall = (id: string, command: string) => ({
  sessionUpdate: 'tool_call',
  toolCallId: id,
  title: 'run_terminal_command',
  rawInput: { command, description: 'Say hi' },
  _meta: { 'x.ai/tool': { version: 1, name: 'run_terminal_command', kind: 'execute', namespace: 'grok_build', label: 'Run Command', read_only: false } },
});

// ---- Mapping tools

test('Grok tools take the names Threadbear knows', () => {
  assert.deepEqual(mapTool('run_terminal_command', { command: 'ls', description: 'List' }), { name: 'Bash', kind: 'command', input: { command: 'ls', description: 'List' } });
  assert.deepEqual(mapTool('search_replace', { file_path: '/a', old_string: 'x', new_string: 'y' }), { name: 'Edit', kind: 'edit', input: { file_path: '/a', old_string: 'x', new_string: 'y' } });
  assert.deepEqual(mapTool('search_replace', { file_path: '/a', old_string: '', new_string: 'hi\n' }), { name: 'Write', kind: 'write', input: { file_path: '/a', content: 'hi\n' } });
  assert.deepEqual(mapTool('read_file', { variant: 'ReadFile', target_file: '/a' }), { name: 'Read', kind: 'read', input: { file_path: '/a' } });
  assert.equal(mapTool('todo_write', { todos: [] }).name, 'TodoWrite');
  assert.deepEqual(mapTool('end_turn', { headline: 'x' }, { name: 'end_turn', namespace: 'threadbear' }), { name: 'mcp__threadbear__end_turn', kind: 'mcp', input: { headline: 'x' } });
  assert.deepEqual(mapTool('grep', { pattern: 'x' }, { name: 'grep', kind: 'search', namespace: 'grok_build' }), { name: 'grep', kind: 'search', input: { pattern: 'x' } });
});

test("MCP tools reached through Grok's tool search show as the tool itself", () => {
  assert.deepEqual(mapTool('use_tool', { variant: 'UseTool', tool_name: 'threadbear__end_turn', tool_input: { headline: 'Done', needs: 'nothing' } }, { name: 'use_tool', kind: 'use_tool', namespace: 'grok_build' }), {
    name: 'mcp__threadbear__end_turn',
    kind: 'mcp',
    input: { headline: 'Done', needs: 'nothing' },
  });
  assert.deepEqual(mapTool('search_tool', { query: 'end_turn threadbear', limit: 5 }), { name: 'ToolSearch', kind: 'other', input: { query: 'end_turn threadbear' } });
  const { n } = norm();
  n.handle({ sessionUpdate: 'tool_call', toolCallId: 'u1', title: 'use_tool', rawInput: { tool_name: 'threadbear__end_turn', tool_input: { headline: 'Done' } }, _meta: { 'x.ai/tool': { name: 'use_tool', kind: 'use_tool', namespace: 'grok_build' } } });
  const done = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'u1', status: 'completed', rawOutput: { type: 'MCP', tool_name: 'end_turn', server_name: 'threadbear', output: { OkayOutput: 'Noted.' } } });
  assert.deepEqual(events(done), [{ type: 'tool.result', itemId: 'u1', ok: true, text: 'Noted.', truncated: false, images: [] }]);
});

test("a running command's output streams as deltas, and its result is the output without Grok's exit line", () => {
  const { n } = norm();
  n.handle(bashCall('c1', 'cat a; ls'));
  const a = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'in_progress', content: [{ type: 'content', content: { type: 'text', text: 'hi\n' } }], rawOutput: { type: 'Bash', output_for_prompt: 'hi\n' } });
  const b = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'in_progress', content: [{ type: 'content', content: { type: 'text', text: 'hi\nhello.txt\n' } }], rawOutput: { type: 'Bash', output_for_prompt: 'hi\nhello.txt\n' } });
  assert.deepEqual(a, [{ kind: 'delta', delta: { type: 'tool.output.delta', itemId: 'c1', text: 'hi\n' } }]);
  assert.deepEqual(b, [{ kind: 'delta', delta: { type: 'tool.output.delta', itemId: 'c1', text: 'hello.txt\n' } }]);
  const done = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'completed', content: [{ type: 'content', content: { type: 'text', text: 'hi\nhello.txt\n' } }], rawOutput: { type: 'Bash', output_for_prompt: 'exit: 0\nhi\nhello.txt\n', exit_code: 0 } });
  assert.equal((events(done)[0] as { text: string }).text, 'hi\nhello.txt\n');
});

test('an Always allow rule is the command for a shell call, the tool for anything else', () => {
  assert.equal(ruleFor(mapTool('run_terminal_command', { command: 'npm test' })), 'Bash(npm test)');
  assert.equal(ruleFor(mapTool('search_replace', { file_path: '/a', old_string: 'x', new_string: 'y' })), 'Edit');
});

// ---- The normaliser

test('message chunks stream as deltas and end as one assistant.text when a tool starts', () => {
  const { n } = norm();
  const a = n.handle({ sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: 'Thinking' } });
  const b = n.handle({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Hel' } });
  const c = n.handle({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'lo' } });
  const d = n.handle(bashCall('c1', 'echo hi'));
  assert.deepEqual(a, [{ kind: 'delta', delta: { type: 'reasoning.delta', itemId: 'g-r1', text: 'Thinking' } }]);
  assert.deepEqual(events(b), [{ type: 'reasoning', itemId: 'g-r1', text: 'Thinking' }]);
  assert.equal(c.length, 1);
  assert.deepEqual(events(d), [
    { type: 'assistant.text', itemId: 'g-m2', text: 'Hello' },
    { type: 'tool.call', itemId: 'c1', name: 'Bash', kind: 'command', title: 'echo hi', input: { command: 'echo hi', description: 'Say hi' } },
  ]);
});

test('a shell call ends with its output; a non-zero exit is a failure', () => {
  const { n } = norm();
  n.handle(bashCall('c1', 'echo hi'));
  const ok = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'completed', rawOutput: { type: 'Bash', output_for_prompt: 'hi\nexit: 0\n', exit_code: 0 } });
  assert.deepEqual(events(ok), [{ type: 'tool.result', itemId: 'c1', ok: true, text: 'hi\nexit: 0\n', truncated: false, images: [] }]);
  n.handle(bashCall('c2', 'false'));
  const bad = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c2', status: 'completed', rawOutput: { type: 'Bash', output_for_prompt: 'exit: 1\n', exit_code: 1 } });
  assert.equal((events(bad)[0] as { ok: boolean }).ok, false);
  n.handle(bashCall('c3', 'true'));
  const quiet = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c3', status: 'completed', rawOutput: { type: 'Bash', output_for_prompt: 'exit: 0\n', exit_code: 0 } });
  assert.equal((events(quiet)[0] as { text: string }).text, '');
});

test('an edit shows its diff against the file before it runs, then its result', () => {
  const { n, cwd } = norm();
  const file = path.join(cwd, 'notes.md');
  fs.writeFileSync(file, 'a\none\nc\n');
  n.handle({ sessionUpdate: 'tool_call', toolCallId: 'e1', title: 'search_replace', rawInput: { file_path: file, old_string: 'one', new_string: 'two' } });
  const preview = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'e1', kind: 'edit', title: `Edit \`${file}\``, content: [{ type: 'diff', path: file, oldText: 'one', newText: 'two' }] });
  const diff = events(preview).find((e) => e.type === 'edit.diff') as { path: string; hunks: { oldStart: number; lines: string[] }[] };
  assert.equal(diff.path, file);
  assert.equal(diff.hunks[0].oldStart, 1);
  assert.ok(diff.hunks[0].lines.includes('-one') && diff.hunks[0].lines.includes('+two') && diff.hunks[0].lines.includes(' a'));
  fs.writeFileSync(file, 'a\ntwo\nc\n');
  const done = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'e1', status: 'completed', content: [{ type: 'diff', path: file, oldText: 'one', newText: 'two' }], rawOutput: { type: 'SearchReplace', EditsApplied: { tool_output_for_prompt: 'The file has been updated successfully.' } } });
  assert.deepEqual(events(done), [{ type: 'tool.result', itemId: 'e1', ok: true, text: 'The file has been updated successfully.', truncated: false, images: [] }]);
});

test('a diff seen only at the end is placed by the line Grok gives', () => {
  const { n, cwd } = norm();
  n.handle({ sessionUpdate: 'tool_call', toolCallId: 'e2', title: 'search_replace', rawInput: { file_path: path.join(cwd, 'x'), old_string: 'one', new_string: 'two' } });
  const done = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'e2', status: 'completed', content: [{ type: 'diff', path: path.join(cwd, 'x'), oldText: 'one', newText: 'two', _meta: { details: [{ old_line: 12, new_line: 12 }] } }] });
  const diff = events(done).find((e) => e.type === 'edit.diff') as { hunks: { oldStart: number; newStart: number }[] };
  assert.equal(diff.hunks[0].oldStart, 12);
  assert.equal(diff.hunks[0].newStart, 12);
});

test('a read returns the file as Grok read it; a plan becomes plan.updated', () => {
  const { n } = norm();
  n.handle({ sessionUpdate: 'tool_call', toolCallId: 'r1', title: 'read_file', rawInput: { target_file: '/x/notes.md' } });
  const read = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'r1', status: 'completed', content: [{ type: 'content', content: { type: 'text', text: '1→two\n' } }], rawOutput: { type: 'ReadFile', FileContent: { content: '1→two\n', raw_output: 'two\n' } } });
  assert.equal((events(read)[0] as { text: string }).text, 'two\n');
  const plan = n.handle({ sessionUpdate: 'plan', entries: [{ content: 'A', status: 'completed' }, { content: 'B', status: 'in_progress' }, { content: 'C', status: 'pending' }] });
  assert.deepEqual(events(plan), [{ type: 'plan.updated', entries: [{ text: 'A', status: 'completed' }, { text: 'B', status: 'in_progress' }, { text: 'C', status: 'pending' }] }]);
});

test('model changes, context and the turn result with its usage', () => {
  const { n } = norm();
  n.setSession('s1', 'grok-4.7');
  n.contextWindow = 256000;
  assert.deepEqual(n.handle({ sessionUpdate: 'model_changed', model_id: 'grok-4.6', reasoning_effort: 'medium' }), [
    { kind: 'session', nativeSessionId: 's1', model: 'grok-4.6' },
    { kind: 'effort', level: 'medium' },
  ]);
  assert.deepEqual(n.handle({ sessionUpdate: 'response_completed', usage: { input_tokens: 500, output_tokens: 20, cache_read_input_tokens: 25000, cache_creation_input_tokens: 0 } }), [{ kind: 'context', contextTokens: 25520, contextWindow: 256000 }]);
  n.handle({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'done' } });
  const outs = n.finish({ stopReason: 'end_turn', _meta: { usage: { inputTokens: 51683, outputTokens: 94, totalTokens: 51777, cachedReadTokens: 27776, cacheCreationTokens: 0, reasoningTokens: 56 } } }, 1200);
  assert.deepEqual(events(outs), [{ type: 'assistant.text', itemId: 'g-m1', text: 'done' }]);
  const r = outs.find((o): o is Extract<AdapterOutput, { kind: 'result' }> => o.kind === 'result')!.result;
  assert.deepEqual(r, { outcome: 'completed', durationMs: 1200, usage: { inputTokens: 51683, cachedInputTokens: 27776, cacheWriteTokens: 0, outputTokens: 94, reasoningTokens: 56 } });
});

test('a cancelled turn is interrupted and closes the calls still open', () => {
  const { n } = norm();
  n.handle(bashCall('c1', 'sleep 20'));
  const outs = n.finish({ stopReason: 'cancelled' }, 10);
  assert.deepEqual(events(outs), [{ type: 'tool.result', itemId: 'c1', ok: false, text: 'Interrupted', truncated: false, images: [] }]);
  assert.equal((outs.at(-1) as Extract<AdapterOutput, { kind: 'result' }>).result.outcome, 'interrupted');
});

test('errors are sorted into the kinds the app offers fixes for', () => {
  assert.equal(errorKind('429 Too Many Requests: rate limit reached'), 'usageLimit');
  assert.equal(errorKind('Unauthorized: please sign in again'), 'auth');
  assert.equal(errorKind('maximum context length exceeded'), 'contextWindow');
  assert.equal(errorKind('connection reset by peer'), 'network');
  assert.equal(errorKind('something else'), undefined);
});

test("Grok's commands, without the ones only its terminal app has", () => {
  const cmds = commandsFrom([
    { name: 'compact', description: 'Compress conversation history', input: null },
    { name: 'always-approve', description: 'Toggle always-approve', input: { hint: 'on|off' } },
    { name: 'deep-research', description: 'Research', input: { hint: 'question' } },
  ]);
  assert.deepEqual(cmds, [
    { name: 'compact', source: 'cli', description: 'Compress conversation history' },
    { name: 'deep-research', source: 'cli', description: 'Research', argumentHint: 'question' },
  ]);
});

test('model rows from initialize, with their effort levels', () => {
  const rows = grokModelChoices([
    {
      modelId: 'grok-4.7',
      name: 'Grok 4.7',
      description: "SpaceXAI's latest frontier model",
      _meta: { totalContextTokens: 256000, supportsReasoningEffort: true, reasoningEffort: 'high', reasoningEfforts: [{ id: 'xhigh', value: 'xhigh' }, { id: 'high', value: 'high', default: true }, { id: 'medium', value: 'medium' }, { id: 'low', value: 'low' }] },
    },
    { modelId: 'grok-4.5', name: 'Grok 4.5', _meta: {} },
  ]);
  assert.deepEqual(rows, [
    { id: 'grok-4.7', model: 'grok-4.7', title: 'Grok 4.7', description: "SpaceXAI's latest frontier model", effortLevels: ['low', 'medium', 'high', 'xhigh'], defaultEffort: 'high' },
    { id: 'grok-4.5', model: 'grok-4.5', title: 'Grok 4.5' },
  ]);
});

test('prompt blocks: words, then messages sent together joined, pictures as images', () => {
  const d = tmp();
  const img = path.join(d, 'p.png');
  fs.writeFileSync(img, Buffer.from([1, 2, 3]));
  const blocks = promptBlocks([
    { id: 'a', text: 'look [Picture 1]', images: [{ path: img, mediaType: 'image/png' }] },
    { id: 'b', text: 'and this', images: [] },
  ]);
  assert.deepEqual(blocks, [
    { type: 'text', text: 'look [Picture 1]' },
    { type: 'image', mimeType: 'image/png', data: Buffer.from([1, 2, 3]).toString('base64') },
    { type: 'text', text: '\n\n' },
    { type: 'text', text: 'and this' },
  ]);
});

// ---- The MCP server

function echoTool(): ToolDefinition {
  return {
    name: 'echo',
    description: 'Says it back',
    inputSchema: { text: z.string() },
    handler: async (args: { text: string }) => ({ content: [{ type: 'text', text: `said ${args.text}` }] }),
  };
}

test('the MCP server answers initialize, tools/list and tools/call on its secret path only', async () => {
  const mcp = new McpHttpServer(nullLogger);
  const calls: string[] = [];
  const ep = await mcp.add({
    name: 'threadbear',
    tools: [echoTool()],
    call: async ({ name, arguments: args }) => {
      calls.push(name);
      return { text: `said ${String(args.text)}`, isError: false };
    },
  });
  const post = (url: string, body: unknown) => fetch(url, { method: 'POST', headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream' }, body: JSON.stringify(body) });
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const read = async (r: Promise<Response>): Promise<any> => (await r).json();
  try {
    const init = await read(post(ep.url, { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'grok' } } }));
    assert.equal(init.result.serverInfo.name, 'threadbear');
    assert.equal(init.result.protocolVersion, '2025-06-18');
    assert.equal((await post(ep.url, { jsonrpc: '2.0', method: 'notifications/initialized' })).status, 202);
    const list = await read(post(ep.url, { jsonrpc: '2.0', id: 2, method: 'tools/list' }));
    assert.equal(list.result.tools[0].name, 'echo');
    assert.equal(list.result.tools[0].inputSchema.properties.text.type, 'string');
    const call = await read(post(ep.url, { jsonrpc: '2.0', id: 3, method: 'tools/call', params: { name: 'echo', arguments: { text: 'hi' } } }));
    assert.deepEqual(call.result, { content: [{ type: 'text', text: 'said hi' }], isError: false });
    assert.deepEqual(calls, ['echo']);
    const wrong = ep.url.replace(/\/mcp\/.*/, '/mcp/not-the-secret');
    assert.equal((await post(wrong, { jsonrpc: '2.0', id: 4, method: 'tools/list' })).status, 404);
    ep.remove();
    assert.equal((await post(ep.url, { jsonrpc: '2.0', id: 5, method: 'tools/list' })).status, 404);
  } finally {
    await mcp.close();
  }
});

// ---- The session, against a fake grok

const account: Account = { id: 'grok-main', provider: 'grok', title: 'Grok · grok-main', configDir: '/work/grok-home' };

const MODELS = {
  currentModelId: 'grok-4.7',
  availableModels: [
    { modelId: 'grok-4.7', name: 'Grok 4.7', _meta: { totalContextTokens: 256000, supportsReasoningEffort: true, reasoningEfforts: [{ value: 'high', default: true }, { value: 'low' }] } },
    { modelId: 'grok-4.6', name: 'Grok 4.6', _meta: { totalContextTokens: 500000 } },
  ],
};

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
  return {
    sink,
    outputs,
    approvals,
    exited,
    state,
    results: (): TurnResult[] => outputs.filter((o): o is Extract<AdapterOutput, { kind: 'result' }> => o.kind === 'result').map((o) => o.result),
    kinds: () => outputs.map((o) => o.kind),
  };
}

async function initGrok(conn: FakeConn): Promise<void> {
  const req = await conn.request('initialize');
  conn.respond(req.id, { protocolVersion: 1, agentCapabilities: { loadSession: true, mcpCapabilities: { http: true } }, _meta: { agentVersion: '1.0.77', modelState: MODELS } });
}

async function open(o: Partial<StartSessionOptions> & { tools?: ToolDefinition[] } = {}) {
  const f = await fake();
  const rec = recorder();
  const home = tmp();
  const mcp = new McpHttpServer(nullLogger);
  const { tools, ...rest } = o;
  const adapter = new GrokAdapter({
    grokBinary: () => f.bin,
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
  await initGrok(conn);
  return { session, conn, rec, home, mcp, opts };
}

async function newSession(h: Awaited<ReturnType<typeof open>>, id = 's1') {
  const req = await h.conn.request('session/new');
  h.conn.respond(req.id, { sessionId: id, models: MODELS, configOptions: [{ id: 'reasoning_effort', currentValue: 'high' }] });
  await waitFor(() => h.rec.kinds().includes('session'));
  return req;
}

async function close(h: Awaited<ReturnType<typeof open>>) {
  await h.session.end();
  await h.mcp.close();
}

test('a new Grok session: spawned with its GROK_HOME, then session/new, session and effort', async () => {
  const h = await open();
  try {
    assert.deepEqual(h.conn.hello.argv, ['agent', '--no-leader', 'stdio']);
    assert.equal(h.conn.hello.env.GROK_HOME, '/work/grok-home');
    assert.equal(h.conn.hello.env.THREADBEAR_THREAD_ID, 'tb-1');
    const req = await newSession(h);
    assert.equal(req.params.cwd, h.home);
    assert.deepEqual(req.params.mcpServers, []);
    assert.deepEqual(
      h.rec.outputs.filter((o) => o.kind !== 'event'),
      [
        { kind: 'session', nativeSessionId: 's1', model: 'grok-4.7' },
        { kind: 'cliVersion', version: '1.0.77' },
        { kind: 'effort', level: 'high' },
      ],
    );
  } finally {
    await close(h);
  }
});

test('the threadbear tools go to Grok as an http MCP server, with their guidance as rules', async () => {
  const h = await open({ tools: [echoTool()], instructions: 'The project brief.' });
  try {
    const req = await newSession(h);
    const servers = req.params.mcpServers as { type: string; name: string; url: string }[];
    assert.equal(servers.length, 1);
    assert.equal(servers[0].type, 'http');
    assert.equal(servers[0].name, 'threadbear');
    const rules = (req.params._meta as { rules: string }).rules;
    assert.match(rules, /end_turn/);
    assert.match(rules, /The project brief\.$/);
    const list = (await (await fetch(servers[0].url, { method: 'POST', body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/list' }) })).json()) as { result: { tools: { name: string }[] } };
    assert.equal(list.result.tools[0].name, 'echo');
  } finally {
    await close(h);
  }
});

test('a model the thread asks for is set on the session before its first turn', async () => {
  const h = await open({ model: 'grok-4.6', effort: 'low' });
  try {
    const req = await h.conn.request('session/new');
    h.conn.respond(req.id, { sessionId: 's1', models: MODELS, configOptions: [{ id: 'reasoning_effort', currentValue: 'high' }] });
    const m = await h.conn.request('session/set_config_option');
    assert.deepEqual(m.params, { sessionId: 's1', configId: 'model', value: 'grok-4.6' });
    h.conn.respond(m.id, { configOptions: [] });
    const e = await h.conn.request('session/set_config_option');
    assert.deepEqual(e.params, { sessionId: 's1', configId: 'reasoning_effort', value: 'low' });
    h.conn.respond(e.id, { configOptions: [] });
    await waitFor(() => h.rec.kinds().includes('session'));
    assert.deepEqual(h.rec.outputs.find((o) => o.kind === 'session'), { kind: 'session', nativeSessionId: 's1', model: 'grok-4.6' });
  } finally {
    await close(h);
  }
});

test('a turn: the prompt, its updates, Full access allows a tool by itself, and the result answers the message', async () => {
  const h = await open();
  try {
    await newSession(h);
    h.session.send({ id: 't1', text: 'say hi', images: [] });
    const p = await h.conn.request('session/prompt');
    assert.deepEqual(p.params, { sessionId: 's1', prompt: [{ type: 'text', text: 'say hi' }] });
    h.conn.notify('session/update', { sessionId: 's1', update: bashCall('c1', 'echo hi') });
    const answer = await h.conn.serverRequest('session/request_permission', {
      sessionId: 's1',
      toolCall: { toolCallId: 'c1', kind: 'execute', title: 'Execute `echo hi`', rawInput: { variant: 'Bash', command: 'echo hi' } },
      options: [{ optionId: 'always-allow', kind: 'allow_always' }, { optionId: 'allow-once', kind: 'allow_once' }, { optionId: 'reject-once', kind: 'reject_once' }],
    });
    assert.deepEqual(answer.result, { outcome: { outcome: 'selected', optionId: 'allow-once' } });
    assert.equal(h.rec.approvals.length, 0);
    h.conn.notify('session/update', { sessionId: 's1', update: { sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'completed', rawOutput: { type: 'Bash', output_for_prompt: 'hi\n', exit_code: 0 } } });
    h.conn.notify('session/update', { sessionId: 's1', update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'hi' } } });
    // Another session's updates (a subagent's) aren't this thread's rows.
    h.conn.notify('session/update', { sessionId: 'other', update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'not ours' } } });
    h.conn.respond(p.id, { stopReason: 'end_turn', _meta: { usage: { inputTokens: 100, outputTokens: 5, totalTokens: 105, cachedReadTokens: 80 } } });
    await waitFor(() => h.rec.results().length === 1);
    const r = h.rec.results()[0];
    assert.equal(r.outcome, 'completed');
    assert.equal(r.answers, 't1');
    assert.deepEqual(r.answered, ['t1']);
    assert.equal((r.usage as { inputTokens: number }).inputTokens, 100);
    const texts = events(h.rec.outputs).filter((e) => e.type === 'assistant.text');
    assert.deepEqual(texts, [{ type: 'assistant.text', itemId: texts[0] && (texts[0] as { itemId: string }).itemId, text: 'hi' }]);
  } finally {
    await close(h);
  }
});

test('Ask before edits: the card decides, and Always allow answers the same command next time', async () => {
  const h = await open({ permissionMode: 'ask' });
  try {
    await newSession(h);
    h.rec.state.approve = async () => ({ decision: 'allowAlways' });
    h.session.send({ id: 't1', text: 'run it', images: [] });
    await h.conn.request('session/prompt');
    const perm = (id: string) => ({
      sessionId: 's1',
      toolCall: { toolCallId: id, kind: 'execute', title: 'Execute `npm test`', rawInput: { variant: 'Bash', command: 'npm test' }, _meta: { 'x.ai/tool': { name: 'run_terminal_command', kind: 'execute', namespace: 'grok_build' } } },
      options: [{ optionId: 'allow-once', kind: 'allow_once' }, { optionId: 'reject-once', kind: 'reject_once' }],
    });
    const first = await h.conn.serverRequest('session/request_permission', perm('c1'));
    assert.deepEqual(first.result, { outcome: { outcome: 'selected', optionId: 'allow-once' } });
    assert.equal(h.rec.approvals.length, 1);
    assert.equal(h.rec.approvals[0].name, 'Bash');
    assert.equal(h.rec.approvals[0].title, 'npm test');
    assert.equal(h.rec.approvals[0].toolUseId, 'c1');
    assert.ok(h.rec.outputs.some((o) => o.kind === 'allowRules' && o.rules[0] === 'Bash(npm test)'));
    // The tool.call went out before its card.
    assert.ok(events(h.rec.outputs).some((e) => e.type === 'tool.call' && e.itemId === 'c1'));
    const second = await h.conn.serverRequest('session/request_permission', perm('c2'));
    assert.deepEqual(second.result, { outcome: { outcome: 'selected', optionId: 'allow-once' } });
    assert.equal(h.rec.approvals.length, 1);
    h.rec.state.approve = async () => ({ decision: 'deny' });
    const third = await h.conn.serverRequest('session/request_permission', { ...perm('c3'), toolCall: { ...perm('c3').toolCall, rawInput: { variant: 'Bash', command: 'rm -rf build' }, _meta: { 'x.ai/tool': { name: 'run_terminal_command', kind: 'execute', namespace: 'grok_build', input: { command: 'rm -rf build' } } } } });
    assert.deepEqual(third.result, { outcome: { outcome: 'selected', optionId: 'reject-once' } });
  } finally {
    await close(h);
  }
});

test('Stop cancels the prompt; the turn ends interrupted', async () => {
  const h = await open();
  try {
    await newSession(h);
    h.session.send({ id: 't1', text: 'sleep', images: [] });
    const p = await h.conn.request('session/prompt');
    h.conn.notify('session/update', { sessionId: 's1', update: bashCall('c1', 'sleep 20') });
    await h.session.interrupt();
    const cancel = await h.conn.notification('session/cancel');
    assert.deepEqual(cancel.params, { sessionId: 's1' });
    h.conn.respond(p.id, { stopReason: 'cancelled' });
    await waitFor(() => h.rec.results().length === 1);
    assert.equal(h.rec.results()[0].outcome, 'interrupted');
    assert.ok(events(h.rec.outputs).some((e) => e.type === 'tool.result' && e.itemId === 'c1' && e.text === 'Interrupted'));
  } finally {
    await close(h);
  }
});

test('messages sent during a turn wait, then go together as the next one', async () => {
  const h = await open();
  try {
    await newSession(h);
    h.session.send({ id: 't1', text: 'one', images: [] });
    const p1 = await h.conn.request('session/prompt');
    h.session.send({ id: 't2', text: 'two', images: [] });
    h.session.send({ id: 't3', text: 'three', images: [] });
    assert.equal(h.conn.pending('session/prompt').length, 0);
    h.conn.respond(p1.id, { stopReason: 'end_turn' });
    const p2 = await h.conn.request('session/prompt');
    assert.deepEqual(p2.params.prompt, [{ type: 'text', text: 'two' }, { type: 'text', text: '\n\n' }, { type: 'text', text: 'three' }]);
    // Grok runs them next without being sent anything more.
    assert.equal(h.rec.results()[0].queuedTurns, 2);
    h.conn.respond(p2.id, { stopReason: 'end_turn' });
    await waitFor(() => h.rec.results().length === 2);
    assert.deepEqual(h.rec.results()[1].answered, ['t2', 't3']);
  } finally {
    await close(h);
  }
});

test("a resume of a session Grok doesn't have ends with SessionNotFoundError", async () => {
  const h = await open({ resume: true, sessionId: 'gone' });
  const req = await h.conn.request('session/resume');
  assert.equal(req.params.sessionId, 'gone');
  h.conn.write({ jsonrpc: '2.0', id: req.id, error: { code: -32603, message: 'Path not found.', data: { code: 'FS_NOT_FOUND' } } });
  const err = await h.rec.exited;
  assert.ok(err instanceof SessionNotFoundError);
  await h.mcp.close();
});

test('a failed prompt fails the turn with its error and kind', async () => {
  const h = await open();
  try {
    await newSession(h);
    h.session.send({ id: 't1', text: 'hi', images: [] });
    const p = await h.conn.request('session/prompt');
    h.conn.fail(p.id, -32603, 'Unauthorized: sign in again');
    await waitFor(() => h.rec.results().length === 1);
    const r = h.rec.results()[0];
    assert.equal(r.outcome, 'failed');
    assert.equal(r.errorKind, 'auth');
    assert.equal(r.answers, 't1');
  } finally {
    await close(h);
  }
});

test('a sandboxed thread is refused', () => {
  const adapter = new GrokAdapter({ grokBinary: () => '/nowhere', registry: new ProcessRegistry(tmp(), nullLogger), log: nullLogger });
  assert.throws(() => adapter.start({ threadId: 't', cwd: '/', account, permissionMode: 'full', sessionId: 'x', resume: false, mediaDir: '/', blobsDir: '/', tmpDir: '/', confined: true }, recorder().sink), /sandbox/);
});
