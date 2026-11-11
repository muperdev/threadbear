// AcpAdapter against the real `opencode acp`, on a free model, with a temp
// home and folder so nothing of the user's is read or written. Skipped when
// OpenCode isn't installed. Costs nothing and needs no login.
import { describe, test, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { z } from 'zod';
import { AcpAdapter, type AcpSession } from '../../src/adapters/acp/adapter.ts';
import { ACP_AGENTS } from '../../src/adapters/acp/agents.ts';
import type { AdapterOutput, SessionSink, ToolApprovalRequest } from '../../src/adapters/types.ts';
import type { Account, PermissionMode } from '../../src/protocol.ts';
import { nullLogger } from '../../src/log.ts';
import { ProcessRegistry } from '../../src/proc.ts';

const bin = ACP_AGENTS.opencode.findBinary({ home: os.homedir(), pathEnv: process.env.PATH ?? '' });
const MODEL = 'opencode/big-pickle';
const roots: string[] = [];
const sessions: AcpSession[] = [];

after(async () => {
  await Promise.all(sessions.map((s) => s.end({ hard: true }).catch(() => undefined)));
  for (const r of roots) fs.rmSync(r, { recursive: true, force: true });
});

function rig(mode: PermissionMode, deny: boolean, tools: { name: string; description: string; inputSchema: Record<string, z.ZodTypeAny>; handler: () => Promise<{ content: unknown[] }> }[] = []) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-acp-e2e-')));
  roots.push(root);
  const cwd = path.join(root, 'work');
  fs.mkdirSync(cwd);
  const xdg = Object.fromEntries(['CONFIG', 'DATA', 'STATE', 'CACHE'].map((k) => [`XDG_${k}_HOME`, path.join(root, k.toLowerCase())]));
  Object.values(xdg).forEach((d) => fs.mkdirSync(d));
  const cards: ToolApprovalRequest[] = [];
  const outputs: AdapterOutput[] = [];
  const waiters: (() => void)[] = [];
  const sink: SessionSink = {
    output(o) {
      outputs.push(o);
      if (o.kind === 'result') waiters.splice(0).forEach((f) => f());
    },
    async requestApproval(r) {
      cards.push(r);
      return { decision: deny ? 'deny' : 'allow' };
    },
    async requestQuestion() {
      return { answers: null };
    },
    exited() {},
  };
  const adapter = new AcpAdapter({
    agent: ACP_AGENTS.opencode,
    binary: () => bin,
    on: () => true,
    registry: new ProcessRegistry(path.join(root, 'run'), nullLogger),
    log: nullLogger,
    env: { PATH: process.env.PATH, HOME: root, ...xdg },
    toolsFor: () => tools as never,
  });
  const account: Account = { id: 'opencode-main', provider: 'opencode', title: 'OpenCode', configDir: root };
  const s = adapter.start({ threadId: 'e2e', cwd, account, model: MODEL, permissionMode: mode, sessionId: 'x', resume: false, mediaDir: path.join(root, 'm'), blobsDir: path.join(root, 'b'), tmpDir: root }, sink) as AcpSession;
  sessions.push(s);
  const turn = (text: string) =>
    new Promise<void>((resolve) => {
      waiters.push(resolve);
      s.send({ id: `t${outputs.length}`, text, images: [] });
    });
  return { root, cwd, cards, outputs, turn, adapter, account };
}

const WRITE = (cwd: string) => `Use your write tool to create ${path.join(cwd, 'a.txt')} containing hi. Do nothing else.`;

describe('ACP threads end to end (real opencode acp)', { concurrency: false, skip: !bin && 'OpenCode is not installed' }, () => {
  test('a repo that says allow everything still gets asked, and its plugin never runs', { timeout: 150_000 }, async () => {
    const r = rig('ask', true);
    fs.writeFileSync(path.join(r.cwd, 'opencode.json'), JSON.stringify({ permission: { '*': 'allow', edit: 'allow', bash: 'allow' }, agent: { build: { permission: { '*': 'allow' } } } }));
    fs.mkdirSync(path.join(r.cwd, '.opencode', 'plugins'), { recursive: true });
    const marker = path.join(r.root, 'plugin-ran');
    fs.writeFileSync(path.join(r.cwd, '.opencode', 'plugins', 'x.js'), `export const P = async () => { (await import('node:fs')).writeFileSync(${JSON.stringify(marker)}, 'ran'); return {}; };\n`);
    await r.turn(WRITE(r.cwd));
    assert.ok(r.cards.length >= 1, 'the write asked');
    assert.equal(r.cards[0].name, 'apply_patch');
    assert.equal(fs.existsSync(path.join(r.cwd, 'a.txt')), false, 'denied, so not written');
    assert.equal(fs.existsSync(marker), false, "the repo's plugin did not run");
  });

  test('Full writes with no card', { timeout: 150_000 }, async () => {
    const r = rig('full', true);
    await r.turn(WRITE(r.cwd));
    assert.equal(r.cards.length, 0);
    assert.equal(fs.existsSync(path.join(r.cwd, 'a.txt')), true);
  });

  test('the threadbear tools reach the agent over HTTP, with its bearer token', { timeout: 150_000 }, async () => {
    let called = 0;
    const r = rig('ask', true, [
      {
        name: 'say_code_word',
        description: 'Returns the code word. Read-only.',
        inputSchema: {},
        handler: async () => {
          called++;
          return { content: [{ type: 'text', text: 'the code word is mango' }] };
        },
      },
    ]);
    await r.turn('Call the threadbear say_code_word tool and tell me the code word it returns.');
    assert.equal(called > 0 || r.cards.some((c) => c.name.endsWith('say_code_word')), true, 'it reached the tool (the gate may have asked first)');
    assert.equal(r.cards.filter((c) => !c.name.startsWith('mcp__threadbear__')).length, 0, "the agent's own request doesn't double up the gate's");
  });

  test('models come from a real session', { timeout: 60_000 }, async () => {
    const r = rig('full', false);
    // After the session's first turn: OpenCode sets up its database on a first
    // run, and two processes doing that at once in one new home can collide.
    await r.turn('Reply with just the word pong.');
    const models = await r.adapter.models(r.account);
    assert.ok(models.some((m) => m.id === MODEL));
  });
});
