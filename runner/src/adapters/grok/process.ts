// One `grok agent --no-leader stdio` process and the ACP JSON-RPC it speaks
// over stdio (docs/grok-plan.md, "The wire"). One per live Grok thread, as
// Codex: closing stdin ends it, with its MCP servers and running commands.
// --no-leader keeps it from joining the shared leader process the Grok app
// may run, so its tools stay in this process group.
import fs from 'node:fs';
import type { ChildProcess } from 'node:child_process';
import type { Account } from '../../protocol.ts';
import type { Logger } from '../../log.ts';
import { groupSpawner, killGroup, type ProcessRegistry } from '../../proc.ts';
import { RUNNER_VERSION } from '../../config.ts';
import { cliEnv } from '../claude/adapter.ts';
import { LineReader, MAX_LINE_BYTES } from '../codex/appserver.ts';

export type RequestId = number | string;

// Variables a Grok or Codex session sets for the commands it runs: a runner
// started from inside one would pass them on.
const INHERITED_VARS = ['GROK_HOME', 'GROK_SANDBOX', 'GROK_AGENT', 'GROK_AGENT_SECRET', 'CODEX_HOME', 'CODEX_THREAD_ID', 'CODEX_SANDBOX'];

export function grokEnv(account: Account, threadEnv: Record<string, string> | undefined, base: NodeJS.ProcessEnv = process.env): Record<string, string | undefined> {
  const env = cliEnv(account, base);
  delete env.CLAUDE_CONFIG_DIR;
  delete env.CLAUDE_AGENT_SDK_CLIENT_APP;
  for (const k of INHERITED_VARS) delete env[k];
  env.GROK_HOME = account.configDir;
  return { ...env, ...threadEnv };
}

export class GrokRpcError extends Error {
  readonly code: number;
  readonly data: unknown;
  constructor(method: string, body: { code?: unknown; message?: unknown; data?: unknown } | undefined) {
    super(typeof body?.message === 'string' ? body.message : `${method} failed`);
    this.name = 'GrokRpcError';
    this.code = typeof body?.code === 'number' ? body.code : -32603;
    this.data = body?.data;
  }
  // What Grok said in data.code ("FS_NOT_FOUND" for a session it doesn't have).
  get detailCode(): string | undefined {
    const c = (this.data as { code?: unknown } | undefined)?.code;
    return typeof c === 'string' ? c : undefined;
  }
}

export class GrokExitedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'GrokExitedError';
  }
}

export type GrokTimings = {
  requestMs: number; // per request (not session/prompt, which lasts the turn)
  sessionMs: number; // session/new and session/resume, which start MCP servers
  spawnRetryMs: number;
  spawnRetryForMs: number;
  closeWaitMs: number; // after closing stdin, before killing the group
};
export const DEFAULT_GROK_TIMINGS: GrokTimings = {
  requestMs: 30_000,
  sessionMs: 120_000,
  spawnRetryMs: 1000,
  spawnRetryForMs: 10_000,
  closeWaitMs: 5000,
};

export type GrokProcessOptions = {
  binary: string;
  args?: string[]; // extra `grok agent` arguments (grokArgs), before `stdio`
  // Another ACP agent (Devin): its whole argument list, its environment,
  // its name in messages, and its version when initialize doesn't say.
  spawnArgs?: string[];
  envFor?: (account: Account, threadEnv: Record<string, string> | undefined) => Record<string, string | undefined>;
  name?: string;
  knownVersion?: string;
  account: Account;
  env?: Record<string, string>;
  cwd: string;
  registry: ProcessRegistry;
  log: Logger;
  label: string;
  timings?: Partial<GrokTimings>;
};

export type InitializeResult = {
  protocolVersion?: number;
  agentCapabilities?: {
    loadSession?: boolean;
    promptCapabilities?: { image?: boolean };
    sessionCapabilities?: { resume?: unknown };
    mcpCapabilities?: { http?: boolean };
  };
  _meta?: { agentVersion?: string };
};

const NO_TIMEOUT = new Set(['session/prompt']);
const SESSION_METHODS = new Set(['session/new', 'session/resume', 'session/load']);

export class GrokProcess {
  readonly label: string;
  version?: string;
  pid?: number;
  onRequest: ((id: RequestId, method: string, params: unknown) => void) | null = null;
  onNotification: ((method: string, params: unknown) => void) | null = null;
  onExit: ((error?: Error) => void) | null = null;

  private opts: GrokProcessOptions;
  private t: GrokTimings;
  private child?: ChildProcess;
  private abort?: AbortController;
  private nextId = 1;
  private pending = new Map<RequestId, { method: string; resolve: (v: unknown) => void; reject: (e: Error) => void; timer?: NodeJS.Timeout }>();
  private stderrTail: string[] = [];
  private exited = true;
  private exitWaiters: (() => void)[] = [];
  private closing = false;

  private get name(): string {
    return this.opts.name ?? 'grok';
  }

  constructor(opts: GrokProcessOptions) {
    this.opts = opts;
    this.label = opts.label;
    this.version = opts.knownVersion;
    this.t = { ...DEFAULT_GROK_TIMINGS, ...opts.timings };
  }

  get alive(): boolean {
    return !this.exited;
  }

  // Spawns and initializes, retrying a spawn that fails straight away for a
  // few seconds (an update swapping the binary).
  async start(): Promise<InitializeResult> {
    const deadline = Date.now() + this.t.spawnRetryForMs;
    for (;;) {
      if (this.closing) throw new GrokExitedError(`${this.name} was closed while it started`);
      try {
        const init = await this.startOnce();
        if (this.closing) {
          await this.stop();
          throw new GrokExitedError(`${this.name} was closed while it started`);
        }
        return init;
      } catch (e) {
        const code = (e as NodeJS.ErrnoException).code;
        const retryable = e instanceof GrokExitedError || code === 'ENOENT' || code === 'EACCES';
        if (!retryable || this.closing || Date.now() + this.t.spawnRetryMs > deadline) throw e;
        this.opts.log.info('grok did not start; retrying', { threadId: this.label, error: (e as Error).message });
        await new Promise((r) => setTimeout(r, this.t.spawnRetryMs));
      }
    }
  }

  private startOnce(): Promise<InitializeResult> {
    const { opts } = this;
    this.abort = new AbortController();
    this.stderrTail = [];
    const spawn = groupSpawner(opts.registry, opts.log, (line) => this.stderr(line), opts.label);
    const child = spawn({
      command: opts.binary,
      args: opts.spawnArgs ?? ['agent', '--no-leader', ...(opts.args ?? []), 'stdio'],
      cwd: opts.cwd,
      env: (opts.envFor ?? grokEnv)(opts.account, opts.env),
      signal: this.abort.signal,
    } as never) as unknown as ChildProcess;
    this.child = child;
    this.pid = child.pid;
    this.exited = false;
    return new Promise<InitializeResult>((resolve, reject) => {
      let settled = false;
      const fail = (e: Error) => {
        if (settled) return;
        settled = true;
        reject(e);
      };
      child.on('error', (e) => {
        this.markExited();
        fail(e);
      });
      child.stdin?.on('error', () => {
        // EPIPE after it exits: the exit handler reports it.
      });
      const reader = new LineReader(MAX_LINE_BYTES, (line) => this.onLine(line), (info) => {
        opts.log.warn('skipped a line from grok over the size guard', { threadId: opts.label, bytes: info.bytes, method: info.method });
      });
      child.stdout?.on('data', (d: Buffer) => reader.push(d));
      child.once('exit', (code, signal) => {
        const tail = this.stderrTail.slice(-5).join('\n');
        this.markExited();
        const what = `${this.name} exited (${signal ?? `code ${code}`})${tail ? `\n${tail}` : ''}`;
        for (const p of this.pending.values()) {
          if (p.timer) clearTimeout(p.timer);
          p.reject(new GrokExitedError(what));
        }
        this.pending.clear();
        if (!settled) return fail(new GrokExitedError(what));
        const onExit = this.onExit;
        this.onExit = null;
        onExit?.(this.closing ? undefined : new Error(what));
      });
      this.request<InitializeResult>('initialize', {
        protocolVersion: 1,
        clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
        clientInfo: { name: 'threadbear', title: 'Threadbear', version: RUNNER_VERSION },
      })
        .then((init) => {
          const v = init?._meta?.agentVersion;
          if (typeof v === 'string') this.version = v;
          settled = true;
          resolve(init);
        })
        .catch(fail);
    });
  }

  private markExited(): void {
    this.exited = true;
    const ws = this.exitWaiters;
    this.exitWaiters = [];
    ws.forEach((w) => w());
  }

  private stderr(line: string): void {
    if (!line.trim()) return;
    this.stderrTail.push(line);
    if (this.stderrTail.length > 20) this.stderrTail.shift();
    this.opts.log.debug('grok stderr', { threadId: this.label, line: line.slice(0, 500) });
  }

  stderrLines(): string[] {
    return [...this.stderrTail];
  }

  // THREADBEAR_GROK_TRACE=<file>: every line both ways appended there, for
  // working out what a new grok sends. Off unless set (a dev runner's).
  private trace(dir: 'in' | 'out', line: string): void {
    const file = process.env.THREADBEAR_GROK_TRACE;
    if (!file) return;
    try {
      fs.appendFileSync(file, `${JSON.stringify({ t: this.label, dir })}\t${line.trimEnd()}\n`);
    } catch {
      // a trace is best effort
    }
  }

  private onLine(line: Buffer): void {
    this.trace('in', line.toString('utf8'));
    let msg: Record<string, unknown>;
    try {
      msg = JSON.parse(line.toString('utf8'));
    } catch {
      this.opts.log.warn('unparseable line from grok', { threadId: this.label, bytes: line.length });
      return;
    }
    if (!msg || typeof msg !== 'object') return;
    const hasId = typeof msg.id === 'number' || typeof msg.id === 'string';
    if (typeof msg.method === 'string') {
      if (hasId) this.dispatchRequest(msg.id as RequestId, msg.method, msg.params);
      else this.onNotification?.(msg.method, msg.params);
      return;
    }
    if (!hasId) return;
    const p = this.pending.get(msg.id as RequestId);
    if (!p) return;
    this.pending.delete(msg.id as RequestId);
    if (p.timer) clearTimeout(p.timer);
    if (msg.error) p.reject(new GrokRpcError(p.method, msg.error as never));
    else p.resolve(msg.result);
  }

  private dispatchRequest(id: RequestId, method: string, params: unknown): void {
    if (this.onRequest) {
      try {
        this.onRequest(id, method, params);
      } catch (e) {
        this.opts.log.error('grok request handler threw', { threadId: this.label, method, error: String(e) });
        this.respondError(id, -32603, 'Threadbear could not handle this request.');
      }
      return;
    }
    // No session attached (a models probe): a permission request is
    // cancelled, anything else refused.
    if (method === 'session/request_permission') this.respond(id, { outcome: { outcome: 'cancelled' } });
    else this.respondError(id, -32601, `Threadbear doesn't handle ${method}.`);
  }

  private write(msg: Record<string, unknown>): boolean {
    const stdin = this.child?.stdin;
    if (this.exited || !stdin || stdin.destroyed || stdin.writableEnded) return false;
    const line = JSON.stringify({ jsonrpc: '2.0', ...msg });
    this.trace('out', line);
    stdin.write(line + '\n');
    return true;
  }

  request<T = unknown>(method: string, params?: unknown, timeoutMs?: number): Promise<T> {
    const id = this.nextId++;
    const ms = timeoutMs ?? (NO_TIMEOUT.has(method) ? 0 : SESSION_METHODS.has(method) ? this.t.sessionMs : this.t.requestMs);
    return new Promise<T>((resolve, reject) => {
      if (this.exited) return reject(new GrokExitedError(`${this.name} is not running`));
      let timer: NodeJS.Timeout | undefined;
      if (ms > 0) {
        timer = setTimeout(() => {
          this.pending.delete(id);
          reject(new Error(`${this.name === 'grok' ? 'Grok' : this.name} didn't answer ${method} within ${Math.round(ms / 1000)} s`));
        }, ms);
        timer.unref();
      }
      this.pending.set(id, { method, resolve: resolve as (v: unknown) => void, reject, timer });
      const msg: Record<string, unknown> = { id, method };
      if (params !== undefined) msg.params = params;
      if (!this.write(msg)) {
        if (timer) clearTimeout(timer);
        this.pending.delete(id);
        reject(new GrokExitedError(`${this.name} is not running`));
      }
    });
  }

  notify(method: string, params?: unknown): void {
    const msg: Record<string, unknown> = { method };
    if (params !== undefined) msg.params = params;
    this.write(msg);
  }

  respond(id: RequestId, result: unknown): void {
    this.write({ id, result });
  }

  respondError(id: RequestId, code: number, message: string): void {
    this.write({ id, error: { code, message } });
  }

  private waitExit(ms: number): Promise<boolean> {
    if (this.exited) return Promise.resolve(true);
    return new Promise((resolve) => {
      const t = setTimeout(() => resolve(false), ms);
      t.unref();
      this.exitWaiters.push(() => {
        clearTimeout(t);
        resolve(true);
      });
    });
  }

  // Ends it: stdin closed, then the group killed if it hasn't gone.
  async close(): Promise<void> {
    this.closing = true;
    await this.stop();
  }

  private async stop(): Promise<void> {
    if (this.exited) return;
    this.child?.stdin?.end();
    if (await this.waitExit(this.t.closeWaitMs)) return;
    this.opts.log.warn('grok did not exit after stdin closed; killing it', { threadId: this.label, pid: this.pid });
    if (this.pid) killGroup(this.pid, 'SIGKILL');
    await this.waitExit(2000);
  }

  kill(): void {
    this.closing = true;
    if (this.exited) return;
    if (this.pid) killGroup(this.pid, 'SIGKILL');
    this.abort?.abort();
  }
}
