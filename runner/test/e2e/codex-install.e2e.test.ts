// The real npm path for Threadbear's own Codex copies (src/updates/): two
// real versions installed from the registry with `npm install --prefix`
// into a temp THREADBEAR_HOME, an app-server started on the first with a
// temp CODEX_HOME, the second activated under it, and the running process
// checked to carry on. It needs the network (about 2 x 340 MB) and sends no
// prompts. Nothing touches a real install: not the nvm global prefix,
// ~/.local/bin/codex, ~/.local/bin/claude or Claude's versions folder.
//
// Run with `npm run test:e2e` (./check.sh --e2e). TB_E2E_SKIP_CODEX=1
// skips it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { CliUpdates } from '../../src/updates/manager.ts';
import { realClock } from '../../src/clock.ts';
import { nullLogger } from '../../src/log.ts';
import { testConfig } from '../helpers/fakes.ts';

const OLD = process.env.TB_E2E_CODEX_OLD ?? '0.163.0';
const NEW = process.env.TB_E2E_CODEX_NEW ?? '0.164.0';
const skip = process.env.TB_E2E_SKIP_CODEX === '1' ? 'TB_E2E_SKIP_CODEX=1' : false;

// Stand-ins for piece A's resolveCodexBinary and checkSurface, so this test
// runs on its own: the platform package's native binary, found the way the
// npm launcher finds it, and a gate that only looks for two method names.
function resolveNative(start: string): { command: string; native: boolean } {
  const launcher = path.join(fs.realpathSync(start), 'node_modules', '@openai', 'codex', 'bin', 'codex.js');
  try {
    const pkg = createRequire(launcher).resolve('@openai/codex-darwin-arm64/package.json');
    const bin = path.join(path.dirname(pkg), 'vendor', 'aarch64-apple-darwin', 'bin', 'codex');
    fs.accessSync(bin, fs.constants.X_OK);
    return { command: bin, native: true };
  } catch {
    return { command: launcher, native: false };
  }
}
function checkSurface(dir: string): { missing: string[]; added: string[] } {
  const text = fs.readdirSync(dir, { recursive: true, encoding: 'utf8' }).filter((f) => f.endsWith('.ts')).map((f) => fs.readFileSync(path.join(dir, f), 'utf8')).join('\n');
  return { missing: ['"turn/start"', '"turn/completed"'].filter((m) => !text.includes(m)).map((m) => m.slice(1, -1)), added: [] };
}

// A minimal app-server client: one JSON object per line, no "jsonrpc".
class AppServer {
  private proc: ChildProcessWithoutNullStreams;
  private buf = '';
  private waiting = new Map<number, (msg: any) => void>();
  constructor(binary: string, env: NodeJS.ProcessEnv) {
    this.proc = spawn(binary, ['app-server'], { env, stdio: ['pipe', 'pipe', 'pipe'] });
    this.proc.stdout.setEncoding('utf8');
    this.proc.stdout.on('data', (d: string) => {
      this.buf += d;
      let nl: number;
      while ((nl = this.buf.indexOf('\n')) >= 0) {
        const line = this.buf.slice(0, nl);
        this.buf = this.buf.slice(nl + 1);
        try {
          const msg = JSON.parse(line);
          if (typeof msg.id === 'number' && !msg.method) this.waiting.get(msg.id)?.(msg);
          else if (msg.id !== undefined && msg.method) this.send({ id: msg.id, error: { code: -32601, message: 'not handled' } });
        } catch {
          // Not a message.
        }
      }
    });
    this.proc.stderr.resume();
  }
  send(msg: object): void {
    this.proc.stdin.write(JSON.stringify(msg) + '\n');
  }
  request(id: number, method: string, params: object): Promise<any> {
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => reject(new Error(`${method}: no reply in 30s`)), 30_000);
      this.waiting.set(id, (msg) => {
        clearTimeout(t);
        resolve(msg);
      });
      this.send({ id, method, params });
    });
  }
  async close(): Promise<number | null> {
    const exited = new Promise<number | null>((r) => this.proc.once('exit', r));
    this.proc.stdin.end();
    const t = setTimeout(() => this.proc.kill('SIGKILL'), 5000);
    const code = await exited;
    clearTimeout(t);
    return code;
  }
}

test('codex copies: install two real versions; a running app-server keeps working across the swap', { skip, timeout: 15 * 60_000 }, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-codex-install-'));
  const home = path.join(root, 'home');
  const codexHome = path.join(root, 'codex-home');
  fs.mkdirSync(path.join(home, 'run'), { recursive: true });
  fs.mkdirSync(codexHome, { recursive: true });
  const env: NodeJS.ProcessEnv = { ...process.env, CODEX_HOME: codexHome };
  for (const k of Object.keys(env)) if (/^THREADB(EAR|ARE)_/.test(k) || k === 'CLAUDE_CONFIG_DIR' || (k.startsWith('CODEX_') && k !== 'CODEX_HOME')) delete env[k];
  const released: string[] = [];
  const u = new CliUpdates({
    cfg: testConfig(home, { cliUpdates: { claude: 'off', codex: 'auto', grok: 'off', devin: 'off', checkMinutes: 60, keepCodexVersions: 2 } }),
    log: nullLogger,
    clock: realClock,
    env,
    hooks: { restartPending: () => false, releaseIdle: (_p, v) => void released.push(v), versionsInUse: () => [], codexBinariesInUse: () => [] },
    codex: { resolve: resolveNative, checkSurface, testedVersion: NEW },
  });
  const tools = path.join(home, 'tools', 'codex');
  let server: AppServer | undefined;
  try {
    u.update('codex', { version: OLD });
    await u.settled();
    const first = u.list().find((c) => c.provider === 'codex')!;
    assert.equal(first.update?.state, 'succeeded', first.update?.message ?? '');
    assert.equal(first.installedVersion, OLD);
    const oldBinary = resolveNative(u.codexBinary());
    assert.equal(oldBinary.native, true, 'the native binary behind the launcher');
    assert.ok(fs.realpathSync(oldBinary.command).startsWith(fs.realpathSync(path.join(tools, OLD))));

    server = new AppServer(oldBinary.command, env);
    const init = await server.request(1, 'initialize', { clientInfo: { name: 'threadbear-e2e', title: 'Threadbear e2e', version: '0.0.0' }, capabilities: { experimentalApi: true } });
    assert.match(String(init.result?.userAgent), new RegExp(`/${OLD.replaceAll('.', '\\.')} `));
    server.send({ method: 'initialized' });

    u.update('codex', { version: NEW });
    await u.settled();
    const second = u.list().find((c) => c.provider === 'codex')!;
    assert.equal(second.update?.state, 'succeeded', second.update?.message ?? '');
    assert.deepEqual([second.installedVersion, second.previousVersion], [NEW, OLD]);
    assert.deepEqual(released, [OLD, NEW]);
    assert.equal(fs.readlinkSync(path.join(tools, 'current')), NEW);
    assert.ok(!fs.readdirSync(tools).some((n) => n.endsWith('.partial')));

    // The old process still answers, from files nothing touched.
    const loaded = await server.request(2, 'thread/loaded/list', {});
    assert.ok(loaded.result, JSON.stringify(loaded.error));
    assert.equal(await server.close(), 0);
    server = undefined;

    // A new one starts the new version.
    const fresh = new AppServer(resolveNative(u.codexBinary()).command, env);
    const init2 = await fresh.request(1, 'initialize', { clientInfo: { name: 'threadbear-e2e', title: 'Threadbear e2e', version: '0.0.0' }, capabilities: { experimentalApi: true } });
    assert.match(String(init2.result?.userAgent), new RegExp(`/${NEW.replaceAll('.', '\\.')} `));
    await fresh.close();
  } finally {
    await server?.close();
    // Retried: an app-server that has just exited can still be writing into
    // its temp CODEX_HOME for a moment (ENOTEMPTY seen once under check.sh).
    fs.rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
