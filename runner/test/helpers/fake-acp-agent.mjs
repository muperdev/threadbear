#!/usr/bin/env node
// A pretend ACP agent for tests, which follows what OpenCode 1.18.23 was seen
// to do on 9 Nov 2026: by default it runs tools without asking; config in
// OPENCODE_CONFIG_CONTENT makes it ask, a folder's opencode.json outranks that
// unless OPENCODE_DISABLE_PROJECT_CONFIG is set, and a folder's plugin runs
// when a session opens. A prompt's words say what it does:
//   "write <path> <text>"  a write through its tool (asks or not, as above)
//   "exec <command>"       the same for a shell command
//   "hello"                just answers
// FAKE_ACP_LOG: where each request is appended, as JSON lines.
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';

const env = process.env;
const logFile = env.FAKE_ACP_LOG;
const log = (m) => {
  if (logFile) fs.appendFileSync(logFile, `${JSON.stringify(m)}\n`);
};
const send = (m) => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', ...m })}\n`);
const AGENT = env.FAKE_ACP_NAME ?? 'OpenCode';
let nextOwn = 0; // from 0, as OpenCode's are
const waiting = new Map();
const ask = (method, params) =>
  new Promise((resolve) => {
    const id = nextOwn++;
    waiting.set(id, resolve);
    send({ id, method, params });
  });

const sessions = new Map();
let mode = 'build';
let model = 'fake/one';
const configOptions = () => [
  { id: 'model', name: 'Model', category: 'model', type: 'select', currentValue: model, options: [{ value: 'fake/one', name: 'Fake One' }, { value: 'fake/two', name: 'Fake Two' }] },
  { id: 'mode', name: 'Mode', category: 'mode', type: 'select', currentValue: mode, options: [{ value: 'build', name: 'build' }, { value: 'plan', name: 'plan' }, { value: 'ask', name: 'ask' }, { value: 'agent', name: 'agent' }] },
];

function asksFor(cwd) {
  if (env.OPENCODE_DISABLE_PROJECT_CONFIG !== '1') {
    try {
      if (JSON.parse(fs.readFileSync(path.join(cwd, 'opencode.json'), 'utf8')).permission) return false; // the folder's config wins
    } catch {
      // no such file
    }
  }
  try {
    const conf = JSON.parse(env.OPENCODE_CONFIG_CONTENT ?? '{}');
    return conf.permission?.['*'] === 'ask' || conf.permission?.edit === 'ask';
  } catch {
    return false;
  }
}

function deniesAll() {
  try {
    return JSON.parse(env.OPENCODE_CONFIG_CONTENT ?? '{}').permission?.['*'] === 'deny';
  } catch {
    return false;
  }
}

function openedSession(cwd) {
  if (env.OPENCODE_DISABLE_PROJECT_CONFIG !== '1' && !process.argv.includes('--pure')) {
    const plugin = path.join(cwd, '.opencode', 'plugins', 'run.txt');
    if (fs.existsSync(plugin)) fs.writeFileSync(fs.readFileSync(plugin, 'utf8').trim(), 'plugin ran');
  }
}

const update = (sessionId, u) => send({ method: 'session/update', params: { sessionId, update: u } });

async function runTool(sessionId, cwd, kind, input, locations, run) {
  const toolCallId = `call-${Math.random().toString(36).slice(2)}`;
  update(sessionId, { sessionUpdate: 'tool_call', toolCallId, title: kind === 'edit' ? 'write' : 'bash', kind, status: 'pending', locations: [], rawInput: {} });
  update(sessionId, { sessionUpdate: 'tool_call_update', toolCallId, status: 'in_progress', kind, locations, rawInput: input });
  if (deniesAll()) {
    update(sessionId, { sessionUpdate: 'tool_call_update', toolCallId, status: 'failed', kind, rawOutput: { error: 'denied by config' } });
    return;
  }
  if (asksFor(cwd)) {
    const r = await ask('session/request_permission', {
      sessionId,
      toolCall: { toolCallId, title: locations[0]?.path ?? input.command, kind, status: 'pending', locations, rawInput: input, ...(kind === 'edit' ? { content: [{ type: 'diff', path: locations[0].path, oldText: '', newText: input.content }] } : {}) },
      options: env.FAKE_ACP_NO_ONCE
        ? [{ optionId: 'always', kind: 'allow_always', name: 'Always allow' }]
        : [
            { optionId: 'once', kind: 'allow_once', name: 'Allow once' },
            { optionId: 'always', kind: 'allow_always', name: 'Always allow' },
            { optionId: 'reject', kind: 'reject_once', name: 'Reject' },
          ],
    });
    log({ answered: r });
    if (r.outcome?.outcome !== 'selected' || r.outcome.optionId === 'reject') {
      update(sessionId, { sessionUpdate: 'tool_call_update', toolCallId, status: 'failed', kind, content: [{ type: 'content', content: { type: 'text', text: 'rejected' } }], rawOutput: { error: 'rejected' } });
      return;
    }
  }
  run();
  update(sessionId, { sessionUpdate: 'tool_call_update', toolCallId, status: 'completed', kind, content: [{ type: 'content', content: { type: 'text', text: 'done' } }] });
}

async function prompt(id, p) {
  const s = sessions.get(p.sessionId);
  const text = p.prompt.filter((b) => b.type === 'text').map((b) => b.text).join('\n');
  log({ prompt: text, blocks: p.prompt.map((b) => b.type) });
  const last = text.split('\n').at(-1).trim();
  const [word, ...rest] = last.split(' ');
  if (word === 'hang') return; // answers only session/cancel
  if (word === 'write') await runTool(p.sessionId, s.cwd, 'edit', { filePath: rest[0], content: rest.slice(1).join(' ') }, [{ path: rest[0] }], () => fs.writeFileSync(rest[0], rest.slice(1).join(' ')));
  else if (word === 'exec') await runTool(p.sessionId, s.cwd, 'execute', { command: rest.join(' ') }, [], () => log({ ran: rest.join(' ') }));
  update(p.sessionId, { sessionUpdate: 'agent_message_chunk', messageId: 'm1', content: { type: 'text', text: 'all ' } });
  update(p.sessionId, { sessionUpdate: 'agent_message_chunk', messageId: 'm1', content: { type: 'text', text: 'done' } });
  update(p.sessionId, { sessionUpdate: 'usage_update', used: 1000, size: 200000, cost: { amount: 0.25, currency: 'USD' } });
  send({ id, result: { stopReason: 'end_turn' } });
}

const handlers = {
  initialize: () => ({
    ...(env.FAKE_ACP_VERSION === 'none' ? {} : { protocolVersion: env.FAKE_ACP_VERSION ? Number(env.FAKE_ACP_VERSION) : 1 }),
    agentCapabilities: { loadSession: true, mcpCapabilities: { http: true }, promptCapabilities: { image: true }, sessionCapabilities: { resume: {}, close: {} } },
    authMethods: AGENT === 'Cursor' ? [{ id: 'cursor_login', name: 'Log in' }] : [{ id: 'opencode-login', name: 'Log in' }],
    agentInfo: { name: AGENT, version: '9.9.9' },
  }),
  'session/new': (p) => {
    if (env.FAKE_ACP_NEEDS_LOGIN) throw Object.assign(new Error('Authentication required'), { code: -32000 });
    const sessionId = `ses-${Math.random().toString(36).slice(2)}`;
    sessions.set(sessionId, { cwd: p.cwd });
    openedSession(p.cwd);
    log({ newSession: { cwd: p.cwd, mcpServers: p.mcpServers } });
    return { sessionId, configOptions: configOptions() };
  },
  'session/resume': (p) => {
    if (p.sessionId === 'gone') throw Object.assign(new Error('session not found'), { code: -32002 });
    sessions.set(p.sessionId, { cwd: p.cwd });
    openedSession(p.cwd);
    log({ resumed: p.sessionId });
    return { configOptions: configOptions() };
  },
  'session/close': () => ({}),
  'session/set_config_option': (p) => {
    if (env.FAKE_ACP_NO_SET_MODE && p.configId === 'mode') throw new Error('no such mode');
    if (p.configId === 'mode' && !env.FAKE_ACP_IGNORE_MODE) mode = p.value;
    if (p.configId === 'model') model = p.value;
    log({ set: { [p.configId]: p.value } });
    return { configOptions: configOptions() };
  },
};

readline.createInterface({ input: process.stdin }).on('line', (line) => {
  if (!line.trim()) return;
  const m = JSON.parse(line);
  if (m.id !== undefined && !m.method) {
    waiting.get(m.id)?.(m.result ?? m.error);
    waiting.delete(m.id);
    return;
  }
  if (m.method === 'session/cancel') {
    log({ cancelled: true });
    return;
  }
  if (m.method === 'session/prompt') {
    void prompt(m.id, m.params);
    if (m.params.prompt.some((b) => b.type === 'text' && b.text.trim().split('\n').at(-1) === 'hang')) hanging.push(m.id);
    return;
  }
  const h = handlers[m.method];
  if (m.id === undefined) return;
  if (!h) return send({ id: m.id, error: { code: -32601, message: `no ${m.method}` } });
  try {
    const result = h(m.params);
    if (m.method === 'session/set_config_option' && env.FAKE_ACP_SLOW_SET) return void setTimeout(() => send({ id: m.id, result }), Number(env.FAKE_ACP_SLOW_SET));
    send({ id: m.id, result });
  } catch (e) {
    send({ id: m.id, error: { code: e.code ?? -32603, message: e.message } });
  }
});
const hanging = [];
process.on('SIGTERM', () => process.exit(0));
