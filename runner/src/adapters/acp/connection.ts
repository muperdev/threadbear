// One ACP agent process (Agent Client Protocol, agentclientprotocol.com):
// newline-delimited JSON-RPC 2.0 over its stdio, in its own process group.
import type { ChildProcess } from 'node:child_process';
import { LineReader, MAX_LINE_BYTES, type OversizedLine } from '../codex/appserver.ts';
import { RUNNER_VERSION } from '../../config.ts';
import type { Logger } from '../../log.ts';
import { groupAlive, groupSpawner, killGroup, type ProcessRegistry } from '../../proc.ts';

export type RequestId = number | string;

// Numbered as ACP's errors are; -32000 is its auth_required.
export const AUTH_REQUIRED = -32000;

export class AcpRpcError extends Error {
  readonly code: number;
  readonly data: unknown;
  constructor(method: string, body: { code?: unknown; message?: unknown; data?: unknown }) {
    super(typeof body?.message === 'string' ? body.message : `${method} failed`);
    this.name = 'AcpRpcError';
    this.code = typeof body?.code === 'number' ? body.code : -32603;
    this.data = body?.data;
  }
}

export class AcpExitedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'AcpExitedError';
  }
}

export type InitializeResult = {
  protocolVersion?: number;
  agentCapabilities?: {
    loadSession?: boolean;
    mcpCapabilities?: { http?: boolean; sse?: boolean };
    promptCapabilities?: { image?: boolean; embeddedContext?: boolean };
    sessionCapabilities?: { resume?: unknown; close?: unknown };
  };
  agentInfo?: { name?: string; version?: string } | null;
  authMethods?: { id?: string; name?: string }[];
};

export type AcpTimings = {
  requestMs: number; // an ordinary request
  sessionMs: number; // session/new, load and resume, which start MCP servers
  closeWaitMs: number; // after closing stdin, before killing the group
};
export const DEFAULT_ACP_TIMINGS: AcpTimings = { requestMs: 30_000, sessionMs: 90_000, closeWaitMs: 2_000 };

export type AcpProcessOptions = {
  binary: string;
  args: string[];
  env: Record<string, string | undefined>;
  cwd: string;
  registry: ProcessRegistry;
  log: Logger;
  label: string; // the thread id, or "models:opencode-main", for log lines
  maxLineBytes?: number;
  timings?: Partial<AcpTimings>;
};

// The ACP version this client speaks; initialize's answer must match it.
export const ACP_VERSION = 1;

export class AcpProcess {
  // Set by the session. A request from the agent with no handler is refused
  // at once, so the agent never waits on nobody.
  onRequest: ((id: RequestId, method: string, params: unknown) => void) | null = null;
  onNotification: ((method: string, params: unknown) => void) | null = null;
  onExit: ((error?: Error) => void) | null = null;
  version?: string;

  private opts: AcpProcessOptions;
  private t: AcpTimings;
  private child?: ChildProcess;
  private pid?: number;
  private abort?: AbortController;
  private nextId = 1;
  private pending = new Map<RequestId, { method: string; resolve: (v: unknown) => void; reject: (e: Error) => void; timer?: NodeJS.Timeout }>();
  private stderrTail: string[] = [];
  private exited = true;
  private exitWaiters: (() => void)[] = [];
  private closing = false;

  constructor(opts: AcpProcessOptions) {
    this.opts = opts;
    this.t = { ...DEFAULT_ACP_TIMINGS, ...opts.timings };
  }

  get alive(): boolean {
    return !this.exited;
  }

  stderrLines(): string[] {
    return [...this.stderrTail];
  }

  // Spawns the process and completes initialize.
  start(): Promise<InitializeResult> {
    const { opts } = this;
    this.abort = new AbortController();
    const spawn = groupSpawner(opts.registry, opts.log, (line) => this.stderr(line), opts.label);
    const child = spawn({ command: opts.binary, args: opts.args, cwd: opts.cwd, env: opts.env, signal: this.abort.signal } as never) as unknown as ChildProcess;
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
        // ENOENT and friends: the spawn failed and no exit follows.
        this.markExited();
        fail(e);
      });
      child.stdin?.on('error', () => {
        // EPIPE when it died; the exit handler reports it.
      });
      const reader = new LineReader(
        opts.maxLineBytes ?? MAX_LINE_BYTES,
        (line) => this.onLine(line),
        (info: OversizedLine) => opts.log.warn('skipped a line from an ACP agent over the size guard', { threadId: opts.label, bytes: info.bytes, method: info.method }),
      );
      child.stdout?.on('data', (d: Buffer) => reader.push(d));
      child.once('exit', (code, signal) => {
        const tail = this.stderrTail.slice(-5).join('\n');
        this.markExited();
        const what = `The agent exited (${signal ?? `code ${code}`})${tail ? `\n${tail}` : ''}`;
        for (const p of this.pending.values()) {
          if (p.timer) clearTimeout(p.timer);
          p.reject(new AcpExitedError(what));
        }
        this.pending.clear();
        if (!settled) return fail(new AcpExitedError(what));
        const onExit = this.onExit;
        this.onExit = null;
        onExit?.(this.closing ? undefined : new Error(what));
      });
      this.request<InitializeResult>('initialize', {
        protocolVersion: ACP_VERSION,
        // Nothing the agent could use to read or write through the client:
        // it works on the files itself, and asks first (AcpSession).
        clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
        clientInfo: { name: 'threadbear', title: 'Threadbear', version: RUNNER_VERSION },
      })
        .then((init) => {
          if (init?.protocolVersion !== ACP_VERSION) {
            throw new Error(`This agent speaks ACP version ${String(init?.protocolVersion)}, and Threadbear speaks ${ACP_VERSION}.`);
          }
          if (typeof init?.agentInfo?.version === 'string') this.version = init.agentInfo.version;
          settled = true;
          resolve(init ?? {});
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
    this.opts.log.debug('acp stderr', { threadId: this.opts.label, line: line.slice(0, 500) });
  }

  private onLine(line: Buffer): void {
    let msg: Record<string, unknown>;
    try {
      msg = JSON.parse(line.toString('utf8'));
    } catch {
      this.opts.log.warn('unparseable line from an ACP agent', { threadId: this.opts.label, bytes: line.length });
      return;
    }
    if (!msg || typeof msg !== 'object') return;
    // An id can be 0: OpenCode numbers its requests from it.
    const hasId = typeof msg.id === 'number' || typeof msg.id === 'string';
    if (typeof msg.method === 'string') {
      if (hasId) this.dispatchRequest(msg.id as RequestId, msg.method, msg.params);
      else this.safely(msg.method, () => this.onNotification?.(msg.method as string, msg.params));
      return;
    }
    if (hasId) {
      const p = this.pending.get(msg.id as RequestId);
      if (!p) return;
      this.pending.delete(msg.id as RequestId);
      if (p.timer) clearTimeout(p.timer);
      if (msg.error) p.reject(new AcpRpcError(p.method, msg.error as { code?: unknown; message?: unknown; data?: unknown }));
      else p.resolve(msg.result);
    }
  }

  // What the agent sends is its own to get wrong: a bad frame is logged, as
  // an exception out of a stream handler would take the runner down.
  private safely(method: string, f: () => void): void {
    try {
      f();
    } catch (e) {
      this.opts.log.error('acp notification handler threw', { threadId: this.opts.label, method, error: String(e) });
    }
  }

  private dispatchRequest(id: RequestId, method: string, params: unknown): void {
    if (this.onRequest) {
      try {
        this.onRequest(id, method, params);
      } catch (e) {
        this.opts.log.error('acp request handler threw', { threadId: this.opts.label, method, error: String(e) });
        this.respondError(id, -32603, 'Threadbear could not handle this request.');
      }
      return;
    }
    this.respondError(id, -32601, `Threadbear doesn't handle ${method} here.`);
  }

  private write(msg: Record<string, unknown>): boolean {
    const stdin = this.child?.stdin;
    if (this.exited || !stdin || stdin.destroyed || stdin.writableEnded) return false;
    stdin.write(JSON.stringify({ jsonrpc: '2.0', ...msg }) + '\n');
    return true;
  }

  // timeoutMs null: no timeout (session/prompt, which lasts as long as the
  // turn does). Left out: requestMs, or sessionMs for the session/ starters.
  request<T = unknown>(method: string, params?: unknown, timeoutMs?: number | null): Promise<T> {
    const id = this.nextId++;
    const starts = method === 'session/new' || method === 'session/load' || method === 'session/resume';
    const ms = timeoutMs === undefined ? (starts ? this.t.sessionMs : this.t.requestMs) : timeoutMs;
    return new Promise<T>((resolve, reject) => {
      if (this.exited) return reject(new AcpExitedError('The agent is not running.'));
      let timer: NodeJS.Timeout | undefined;
      if (ms !== null) {
        timer = setTimeout(() => {
          this.pending.delete(id);
          reject(new Error(`The agent didn't answer ${method} within ${Math.round(ms / 1000)} s`));
        }, ms);
        timer.unref();
      }
      this.pending.set(id, { method, resolve: resolve as (v: unknown) => void, reject, timer });
      const msg: Record<string, unknown> = { id, method };
      if (params !== undefined) msg.params = params;
      if (!this.write(msg)) {
        if (timer) clearTimeout(timer);
        this.pending.delete(id);
        reject(new AcpExitedError('The agent is not running.'));
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

  // Ends stdin; if the agent hasn't gone after closeWaitMs, its group is
  // killed. Agents that don't watch stdin (OpenCode's server loop) rely on
  // that second half.
  async close(): Promise<void> {
    this.closing = true;
    if (this.exited) return;
    this.child?.stdin?.end();
    if (await this.waitExit(this.t.closeWaitMs)) return;
    if (this.pid) killGroup(this.pid, 'SIGTERM');
    if (await this.waitExit(2000)) return;
    if (this.pid) killGroup(this.pid, 'SIGKILL');
    await this.waitExit(2000);
  }

  // Stops the process group straight away (SIGTERM, then SIGKILL).
  kill(): void {
    this.closing = true;
    if (this.exited) return;
    if (!this.pid) return this.abort?.abort();
    const pid = this.pid;
    killGroup(pid, 'SIGTERM');
    setTimeout(() => {
      if (groupAlive(pid)) killGroup(pid, 'SIGKILL');
    }, 3000).unref();
  }
}
