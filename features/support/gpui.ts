// The GPUI app: the debug build with the `automation` feature, on the mock
// runner, driven over a local socket that only that build opens. On a Linux
// server with no display it gets a virtual one.
import { spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { sleep, type Driver, type Dump } from './driver.ts';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const exe = process.platform === 'win32' ? 'threadbear-gpui.exe' : 'threadbear-gpui';
// TB_GPUI_PROFILE=release for a release build (the perf tools).
const profile = process.env.TB_GPUI_PROFILE ?? 'debug';
const binary = process.env.TB_GPUI_BIN ?? path.join(process.env.CARGO_TARGET_DIR ?? path.join(root, 'desktop/target'), profile, exe);

async function waitForFile(file: string, what: string, ms: number): Promise<void> {
  const until = Date.now() + ms;
  while (!fs.existsSync(file)) {
    if (Date.now() > until) throw new Error(`${what} didn't start`);
    await sleep(50);
  }
}

export class GpuiDriver implements Driver {
  readonly name = 'gpui' as const;
  private home = '';
  private children: ChildProcess[] = [];
  private socket?: net.Socket;
  private token = '';
  private buffer = '';
  private waiting: ((line: string) => void)[] = [];
  private log = '';

  private appProcess?: ChildProcess;
  private display?: string;

  private child(cmd: string, args: string[], env: NodeJS.ProcessEnv): ChildProcess {
    // A process group of its own: `dbus-run-session`'s bus and the portals it
    // starts are grandchildren that outlive a plain kill, and the cloud box
    // ended up with thousands of them (9 Nov, "can't spawn worker thread").
    const c = spawn(cmd, args, { env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'pipe'], detached: process.platform !== 'win32' });
    c.stdout?.on('data', (d) => (this.log += d));
    c.stderr?.on('data', (d) => (this.log += d));
    this.children.push(c);
    return c;
  }

  /** Stops a child and everything it started: the whole process group it leads (see `child`). */
  private killChild(c: ChildProcess): void {
    if (c.pid && process.platform !== 'win32') {
      try {
        process.kill(-c.pid, 'SIGTERM');
        return;
      } catch {
        // Already gone.
      }
    }
    c.kill();
  }

  /** Starts the app itself and connects to its automation socket. */
  private async launchApp(): Promise<void> {
    const file = path.join(this.home, `automation-${process.pid}.json`);
    fs.rmSync(file, { force: true });
    const env: NodeJS.ProcessEnv = {
      THREADBEAR_HOME: this.home,
      THREADBEAR_AUTOMATION: file,
    };
    if (process.platform === 'linux' && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) {
      // A server: a virtual screen, and a session bus of its own for the
      // desktop settings and portals the app asks for.
      this.display ??= `:${100 + (process.pid % 400)}`;
      if (!this.children.some((c) => c.spawnargs[0] === 'Xvfb')) {
        this.child('Xvfb', [this.display, '-screen', '0', '1400x900x24'], {});
        await sleep(400);
      }
      this.appProcess = this.child('dbus-run-session', ['--', binary], { ...env, DISPLAY: this.display, NO_AT_BRIDGE: '1', LIBGL_ALWAYS_SOFTWARE: '1' });
    } else {
      this.appProcess = this.child(binary, [], env);
    }
    try {
      await waitForFile(file, 'The app', 30_000);
    } catch (e) {
      throw new Error(`${(e as Error).message}. It said:\n${this.log.slice(-4000)}`);
    }
    const { port, token } = JSON.parse(fs.readFileSync(file, 'utf8')) as { port: number; token: string };
    this.token = token;
    this.socket?.destroy();
    this.waiting = [];
    this.buffer = '';
    this.socket = await new Promise<net.Socket>((resolve, reject) => {
      const s = net.connect(port, '127.0.0.1', () => resolve(s));
      s.once('error', reject);
    });
    this.socket.setEncoding('utf8');
    this.socket.on('data', (chunk: string) => {
      this.buffer += chunk;
      let at: number;
      while ((at = this.buffer.indexOf('\n')) >= 0) {
        const line = this.buffer.slice(0, at);
        this.buffer = this.buffer.slice(at + 1);
        this.waiting.shift()?.(line);
      }
    });
  }

  async relaunch(): Promise<void> {
    this.socket?.destroy();
    this.socket = undefined;
    const old = this.appProcess;
    if (old) {
      const gone = new Promise<void>((done) => old.once('exit', () => done()));
      this.killChild(old);
      await Promise.race([gone, sleep(4000)]);
      this.children = this.children.filter((c) => c !== old);
    }
    await this.launchApp();
  }

  async start(): Promise<void> {
    if (!fs.existsSync(binary)) throw new Error(`No GPUI app at ${binary}: cargo build -p threadbear-gpui --features automation`);
    // TB_RUNNER_HOME points at a real runner's data folder (a dev runner's), for
    // a one-off check that the app works with the real thing and not just the mock.
    this.home = process.env.TB_RUNNER_HOME ?? fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-spec-'));
    const ready = path.join(this.home, 'mock-ready');
    // TB_MOCK_FIXTURES swaps the demo threads for that folder's, for one-off checks.
    // TB_MOCK_REMOTE: a scenario about other machines (@remote-threads).
    const fixtures = process.env.TB_MOCK_FIXTURES ? ['--fixtures', process.env.TB_MOCK_FIXTURES] : [];
    if (!process.env.TB_RUNNER_HOME) {
      this.child(process.execPath, [path.join(root, 'app/Tools/mock-runner/server.mjs'), '--data-dir', this.home, '--port', '0', '--quiet', '--ready-file', ready, ...fixtures], { TB_MOCK_NO_REMOTE: process.env.TB_MOCK_REMOTE ? '' : '1' });
      await waitForFile(ready, 'The mock runner', 15_000);
    }
    await this.launchApp();
  }

  /** The app's process, for tools that need to find its window. */
  get pid(): number | undefined {
    return this.appProcess?.pid;
  }

  private ask(body: Record<string, unknown>, timeoutSeconds: number): Promise<Record<string, unknown>> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`${body.cmd} timed out. The app said:\n${this.log.slice(-2000)}`)), timeoutSeconds * 1000);
      this.waiting.push((line) => {
        clearTimeout(timer);
        resolve(JSON.parse(line) as Record<string, unknown>);
      });
      this.socket?.write(`${JSON.stringify({ ...body, token: this.token })}\n`);
    });
  }

  async do(cmd: Record<string, unknown>, timeoutSeconds = 30): Promise<string> {
    const r = await this.ask(cmd, timeoutSeconds);
    if (r.error) throw new Error(`${cmd.cmd}: ${r.error as string}`);
    return typeof r.result === 'string' ? r.result : JSON.stringify(r.result ?? '');
  }

  async dump(): Promise<Dump> {
    const r = await this.ask({ cmd: 'dump' }, 30);
    if (r.error) throw new Error(`dump: ${r.error as string}\n${this.log.slice(-3000)}`);
    return r.result as Dump;
  }

  async shot(file: string): Promise<void> {
    await this.do({ cmd: 'screenshot', path: file });
  }

  async stop(): Promise<void> {
    this.socket?.destroy();
    for (const c of this.children.reverse()) {
      this.killChild(c);
      // dbus-run-session's children can outlive it holding these pipes open,
      // and Node then waits on them for ever.
      c.stdout?.destroy();
      c.stderr?.destroy();
    }
    await sleep(100);
    if (this.home && !process.env.TB_RUNNER_HOME) fs.rmSync(this.home, { recursive: true, force: true });
  }
}
