// A stand-in for `codex app-server`, for the Codex adapter's unit tests.
//
// Run as a script, it's a relay: a real child process that the adapter
// spawns, talks JSON lines to on stdin and stdout, and that exits when its
// stdin closes, as Codex does. Everything it reads goes to the test over a
// Unix socket, and everything the test sends comes back out on stdout. So
// the test drives the wire from inside its own process (answering
// requests, sending notifications, replaying fixture lines, sending raw
// bytes, exiting), while the adapter sees a real process.
//
// Imported, it's the test's side: FakeCodex makes the socket and a `codex`
// script that starts the relay, and hands out one FakeConn per process.
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { PATIENCE_MS } from './patience.ts';

const SELF = fileURLToPath(import.meta.url);

type Msg = Record<string, unknown>;

// ---- The relay (the child process)

function relay(): void {
  const sock = net.createConnection(process.env.FAKE_CODEX_SOCKET!);
  const keep = ['CODEX_HOME', 'GROK_HOME', 'CODEX_MANAGED_BY_NPM', 'CODEX_MANAGED_PACKAGE_ROOT', 'CODEX_INTERNAL_ORIGINATOR_OVERRIDE', 'CODEX_THREAD_ID', 'THREADBEAR_THREAD_ID', 'CLAUDECODE', 'CLAUDE_CONFIG_DIR'];
  const env: Record<string, string | undefined> = {};
  for (const k of keep) env[k] = process.env[k];
  sock.write(JSON.stringify({ $fake: 'hello', pid: process.pid, argv: process.argv.slice(2), cwd: process.cwd(), env }) + '\n');
  let inBuf = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (d: string) => {
    inBuf += d;
    let i;
    while ((i = inBuf.indexOf('\n')) >= 0) {
      const line = inBuf.slice(0, i);
      inBuf = inBuf.slice(i + 1);
      if (line.trim()) sock.write(JSON.stringify({ $fake: 'in', line }) + '\n');
    }
  });
  // Like Codex: stdin closing ends the process.
  process.stdin.on('end', () => {
    sock.end(() => process.exit(0));
  });
  let sBuf = '';
  sock.setEncoding('utf8');
  sock.on('data', (d: string) => {
    sBuf += d;
    let i;
    while ((i = sBuf.indexOf('\n')) >= 0) {
      const line = sBuf.slice(0, i);
      sBuf = sBuf.slice(i + 1);
      const cmd = JSON.parse(line) as { $fake: string; data?: string; code?: number };
      if (cmd.$fake === 'out') process.stdout.write(cmd.data!);
      else if (cmd.$fake === 'exit') process.stdout.write('', () => process.exit(cmd.code ?? 0));
      else if (cmd.$fake === 'stderr') process.stderr.write(cmd.data!);
    }
  });
  sock.on('close', () => process.exit(0));
  sock.on('error', () => process.exit(3));
}

if (process.argv[1] === SELF) relay();

// ---- The test's side

export type Hello = { pid: number; argv: string[]; cwd: string; env: Record<string, string | undefined> };

export class FakeConn {
  readonly hello: Hello;
  // Every message the client sent, in order.
  readonly received: Msg[] = [];
  // Answers requests by method without the test waiting for them.
  auto = new Map<string, (params: unknown, msg: Msg) => unknown>();
  readonly closed: Promise<void>;
  private sock: net.Socket;
  private waiters: { pred: (m: Msg) => boolean; resolve: (m: Msg) => void }[] = [];
  private unclaimed: Msg[] = [];
  private nextServerId = 0;
  private serverWaits = new Map<string | number, (m: Msg) => void>();

  constructor(sock: net.Socket, hello: Hello, closed: Promise<void>) {
    this.sock = sock;
    this.hello = hello;
    this.closed = closed;
    this.auto.set('account/read', () => ({ account: { type: 'chatgpt', email: 'someone@example.com', planType: 'prolite' } }));
    this.auto.set('account/rateLimits/read', () => ({
      ordinaryUsageAllowed: true,
      rateLimits: { limitId: 'codex', limitName: null, primary: { usedPercent: 5, windowDurationMins: 10080, resetsAt: 1793599387 }, secondary: null, credits: null, planType: 'prolite', rateLimitReachedType: null },
      rateLimitsByLimitId: null,
    }));
  }

  // Called by FakeCodex for each line the client wrote.
  take(m: Msg): void {
    this.received.push(m);
    const isRequest = typeof m.method === 'string' && m.id !== undefined;
    if (!isRequest && m.id !== undefined) {
      const w = this.serverWaits.get(m.id as string);
      if (w) {
        this.serverWaits.delete(m.id as string);
        w(m);
        return;
      }
    }
    if (isRequest) {
      const handler = this.auto.get(m.method as string);
      if (handler) {
        this.respond(m.id as number, handler(m.params, m));
        return;
      }
    }
    const i = this.waiters.findIndex((w) => w.pred(m));
    if (i >= 0) {
      const [w] = this.waiters.splice(i, 1);
      w.resolve(m);
    } else {
      this.unclaimed.push(m);
    }
  }

  // The next message (not answered automatically) matching pred.
  next(pred: (m: Msg) => boolean = () => true, ms = 5000): Promise<Msg> {
    const i = this.unclaimed.findIndex(pred);
    if (i >= 0) return Promise.resolve(this.unclaimed.splice(i, 1)[0]);
    return new Promise((resolve, reject) => {
      const w = { pred, resolve: (m: Msg) => (clearTimeout(t), resolve(m)) };
      const t = setTimeout(() => {
        this.waiters = this.waiters.filter((x) => x !== w);
        reject(new Error(`fake codex: nothing matched within ${ms} ms; unclaimed: ${JSON.stringify(this.unclaimed.map((u) => u.method ?? u.id))}`));
      }, ms);
      this.waiters.push(w);
    });
  }

  // The next request from the client with this method.
  request(method: string, ms?: number): Promise<{ id: number; method: string; params: Record<string, unknown> }> {
    return this.next((m) => m.method === method && m.id !== undefined, ms) as never;
  }

  notification(method: string, ms?: number): Promise<Msg> {
    return this.next((m) => m.method === method && m.id === undefined, ms);
  }

  // Requests not yet claimed with this method (to check nothing was sent).
  pending(method: string): Msg[] {
    return this.unclaimed.filter((m) => m.method === method);
  }

  sent(method: string): Msg[] {
    return this.received.filter((m) => m.method === method);
  }

  write(m: Msg): void {
    this.raw(JSON.stringify(m) + '\n');
  }

  raw(data: string): void {
    this.sock.write(JSON.stringify({ $fake: 'out', data }) + '\n');
  }

  stderr(data: string): void {
    this.sock.write(JSON.stringify({ $fake: 'stderr', data }) + '\n');
  }

  respond(id: number | string, result: unknown): void {
    this.write({ id, result });
  }

  fail(id: number | string, code: number, message: string): void {
    this.write({ id, error: { code, message } });
  }

  notify(method: string, params: unknown): void {
    // Codex puts emittedAtMs next to params.
    this.write({ method, params, emittedAtMs: Date.now() });
  }

  // A request from Codex to the client, with Codex's own ids (from 0).
  serverRequest(method: string, params: unknown, id?: number): Promise<Msg> {
    const rid = id ?? this.nextServerId++;
    const p = new Promise<Msg>((resolve) => this.serverWaits.set(rid, resolve));
    this.write({ id: rid, method, params });
    return p;
  }

  exit(code = 0): void {
    this.sock.write(JSON.stringify({ $fake: 'exit', code }) + '\n');
  }

  // Answers initialize like Codex 0.164.0 and waits for `initialized`.
  async init(version = '0.164.0'): Promise<Msg> {
    const req = await this.request('initialize');
    this.respond(req.id, {
      userAgent: `threadbear/${version} (Mac OS 26.6.2; arm64) unknown (threadbear; 0.1.0)`,
      codexHome: '/work/codex-home',
      platformFamily: 'unix',
      platformOs: 'macos',
    });
    await this.notification('initialized');
    return req;
  }
}

export class FakeCodex {
  readonly dir: string;
  // The `codex` the adapter is told to start.
  readonly bin: string;
  private server: net.Server;
  private conns: FakeConn[] = [];
  private waiters: ((c: FakeConn) => void)[] = [];
  readonly all: FakeConn[] = [];

  private constructor(dir: string, bin: string, server: net.Server) {
    this.dir = dir;
    this.bin = bin;
    this.server = server;
  }

  static async create(): Promise<FakeCodex> {
    // Unix socket paths are short (104 bytes on macOS), so /tmp, not $TMPDIR.
    const dir = fs.mkdtempSync(path.join(os.platform() === 'darwin' ? '/tmp' : os.tmpdir(), 'tb-fcx-'));
    const sockPath = path.join(dir, 's');
    const bin = path.join(dir, 'codex');
    // A shell script, so the adapter spawns it as a native binary would be.
    fs.writeFileSync(bin, `#!/bin/sh\nFAKE_CODEX_SOCKET='${sockPath}' exec '${process.execPath}' '${SELF}' "$@"\n`, { mode: 0o755 });
    const server = net.createServer();
    const fake = new FakeCodex(dir, bin, server);
    server.on('connection', (sock) => fake.accept(sock));
    await new Promise<void>((r) => server.listen(sockPath, r));
    return fake;
  }

  private accept(sock: net.Socket): void {
    let buf = '';
    let conn: FakeConn | undefined;
    let closedResolve!: () => void;
    const closed = new Promise<void>((r) => (closedResolve = r));
    sock.setEncoding('utf8');
    sock.on('close', () => closedResolve());
    sock.on('error', () => undefined);
    sock.on('data', (d: string) => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i);
        buf = buf.slice(i + 1);
        const m = JSON.parse(line) as { $fake: string; line?: string } & Hello;
        if (m.$fake === 'hello') {
          conn = new FakeConn(sock, m, closed);
          this.all.push(conn);
          const w = this.waiters.shift();
          if (w) w(conn);
          else this.conns.push(conn);
        } else if (m.$fake === 'in' && conn) {
          conn.take(JSON.parse(m.line!));
        }
      }
    });
  }

  // The next process the adapter starts.
  next(ms = PATIENCE_MS): Promise<FakeConn> {
    const c = this.conns.shift();
    if (c) return Promise.resolve(c);
    return new Promise((resolve, reject) => {
      const waiter = (conn: FakeConn) => {
        clearTimeout(t);
        resolve(conn);
      };
      // A waiter left behind would swallow the next test's process, and that
      // test would fail too: tests in a file share one fake.
      const t = setTimeout(() => {
        this.waiters = this.waiters.filter((w) => w !== waiter);
        reject(new Error('fake codex: no process started'));
      }, ms);
      this.waiters.push(waiter);
    });
  }

  async close(): Promise<void> {
    for (const c of this.all) c.exit(0);
    await new Promise<void>((r) => this.server.close(() => r()));
    fs.rmSync(this.dir, { recursive: true, force: true });
  }
}
