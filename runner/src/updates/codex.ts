// Threadbear's own Codex copies, which nothing else installs into:
//
//   <home>/tools/codex/
//     0.164.0/        an `npm install --prefix` of @openai/codex@0.164.0
//     0.165.0/
//     current -> 0.165.0
//     state.json      { current, previous, hold?, installs: [...] }
//     .lock           the pid of the runner installing
//
// The shared npm copy is reinstalled daily by Token Stats and at any time by
// T3's "Update all", which swaps the files under a running Codex's helpers.
// A version folder here never changes once it's there: running sessions
// keep theirs, and new ones start whatever `current` points at.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { atomicWriteFileSync, readJsonSync } from '../fsutil.ts';
import { VERSION_RE, compareVersions, type Exec } from './exec.ts';

export const CODEX_PACKAGE = '@openai/codex';
export const NPM_LATEST = 'https://registry.npmjs.org/@openai/codex/latest';
export const INSTALL_TIMEOUT_MS = 5 * 60_000;

// A's resolver (adapters/codex/appserver.ts resolveCodexBinary): from an
// install folder, a launcher or codex.js to the native binary behind it.
export type CodexResolver = (start: string) => { command: string; native: boolean; packageRoot?: string };
// A's schema gate (adapters/codex/surface.ts checkSurface): what the runner
// relies on that a generated schema lacks, and what's new in it.
export type SurfaceCheck = (generatedDir: string) => { missing: string[]; added: string[] };

export type CodexHold = { version: string; reason: string; missing?: string[] };
export type CodexState = {
  current?: string;
  previous?: string;
  hold?: CodexHold;
  installs: { version: string; at: string; result: 'activated' | 'held' | 'failed'; message?: string }[];
};

export type InstallOutcome =
  | { kind: 'ready'; added: string[]; output: string }
  | { kind: 'held'; missing: string[]; added: string[]; output: string }
  | { kind: 'failed'; message: string; output: string };

export type CodexCopiesOptions = {
  dir: string; // <home>/tools/codex
  exec: Exec;
  npmPath: string; // the npm next to the runner's Node
  resolve: CodexResolver;
  checkSurface: SurfaceCheck;
  tmpDir?: string;
  env?: NodeJS.ProcessEnv;
  now?: () => number;
};

export class CodexCopies {
  readonly dir: string;
  private o: CodexCopiesOptions;

  constructor(o: CodexCopiesOptions) {
    this.o = o;
    this.dir = o.dir;
  }

  get currentLink(): string {
    return path.join(this.dir, 'current');
  }

  state(): CodexState {
    const s = readJsonSync<CodexState>(path.join(this.dir, 'state.json'));
    return { installs: [], ...s };
  }

  private saveState(s: CodexState): void {
    // The newest 50 installs are plenty to see what happened.
    atomicWriteFileSync(path.join(this.dir, 'state.json'), JSON.stringify({ ...s, installs: s.installs.slice(-50) }, null, 2) + '\n');
  }

  // The version `current` points at, from the link itself.
  currentVersion(): string | undefined {
    try {
      const v = path.basename(fs.readlinkSync(this.currentLink));
      return VERSION_RE.test(v) && fs.existsSync(path.join(this.dir, v)) ? v : undefined;
    } catch {
      return undefined;
    }
  }

  has(version: string): boolean {
    return VERSION_RE.test(version) && fs.existsSync(path.join(this.dir, version, 'node_modules', ...CODEX_PACKAGE.split('/'), 'package.json'));
  }

  versions(): string[] {
    try {
      return fs.readdirSync(this.dir).filter((n) => VERSION_RE.test(n) && fs.statSync(path.join(this.dir, n)).isDirectory()).sort(compareVersions);
    } catch {
      return [];
    }
  }

  // One install at a time per home, across runners: a dev runner on the
  // same home must never install over this one. A lock whose pid is gone
  // is stale and taken over.
  lock(): boolean {
    fs.mkdirSync(this.dir, { recursive: true });
    const file = path.join(this.dir, '.lock');
    for (let i = 0; i < 2; i++) {
      try {
        fs.writeFileSync(file, String(process.pid), { flag: 'wx' });
        return true;
      } catch {
        const pid = Number(fs.readFileSync(file, 'utf8').trim());
        if (pid === process.pid) return true;
        if (pid > 0 && alive(pid)) return false;
        fs.rmSync(file, { force: true });
      }
    }
    return false;
  }

  unlock(): void {
    const file = path.join(this.dir, '.lock');
    try {
      if (Number(fs.readFileSync(file, 'utf8').trim()) === process.pid) fs.rmSync(file, { force: true });
    } catch {
      // Not ours, or gone.
    }
  }

  // An install that died part way leaves a <v>.partial folder.
  cleanPartials(): string[] {
    const removed: string[] = [];
    try {
      for (const n of fs.readdirSync(this.dir)) {
        if (!n.endsWith('.partial')) continue;
        fs.rmSync(path.join(this.dir, n), { recursive: true, force: true });
        removed.push(n);
      }
    } catch {
      // No folder yet.
    }
    return removed;
  }

  // Installs a version into <v>.partial, checks it runs and still speaks the
  // protocol the runner uses, then renames it to <v>. A held version is
  // kept too, so "Update anyway" needn't download it again; only activate()
  // makes a version current.
  async install(version: string, opts: { force?: boolean } = {}): Promise<InstallOutcome> {
    if (!VERSION_RE.test(version)) return { kind: 'failed', message: `Not a version: ${version}`, output: '' };
    fs.mkdirSync(this.dir, { recursive: true });
    const partial = path.join(this.dir, `${version}.partial`);
    fs.rmSync(partial, { recursive: true, force: true });
    let output = '';
    try {
      const env = { ...(this.o.env ?? process.env) };
      // Public npm, whatever ~/.npmrc says: a company registry's expired token
      // gave E401 here, and nothing in tools/codex pins it like runner/.npmrc.
      // The runner's own Node first, so npm and the package's scripts run on it.
      env.PATH = `${path.dirname(this.o.npmPath)}${path.delimiter}${env.PATH ?? ''}`;
      const npm = await this.o.exec(this.o.npmPath, ['install', '--prefix', partial, '--no-save', '--no-fund', '--no-audit', '--registry', 'https://registry.npmjs.org/', `${CODEX_PACKAGE}@${version}`], {
        cwd: this.dir,
        env,
        timeoutMs: INSTALL_TIMEOUT_MS,
      });
      output += npm.stdout + npm.stderr;
      if (npm.code !== 0) return this.fail(partial, `npm install failed${npm.error ? `: ${npm.error}` : ` (exit ${npm.code})`}`, output);

      const verify = await this.verify(partial, version);
      output += verify.output;
      if (!verify.binary) return this.fail(partial, verify.message, output);

      let gate: { missing: string[]; added: string[] } = { missing: [], added: [] };
      if (!opts.force) {
        const g = await this.gate(verify.binary);
        output += g.output;
        if (!g.result) return this.fail(partial, g.message, output);
        gate = g.result;
      }
      const final = path.join(this.dir, version);
      fs.rmSync(final, { recursive: true, force: true });
      fs.renameSync(partial, final);
      if (gate.missing.length) return { kind: 'held', missing: gate.missing, added: gate.added, output };
      return { kind: 'ready', added: gate.added, output };
    } catch (e) {
      return this.fail(partial, (e as Error).message, output);
    }
  }

  private fail(partial: string, message: string, output: string): InstallOutcome {
    fs.rmSync(partial, { recursive: true, force: true });
    return { kind: 'failed', message, output };
  }

  // The native binary is found, prints the version and runs app-server.
  private async verify(folder: string, version: string): Promise<{ binary?: string; message: string; output: string }> {
    let found: ReturnType<CodexResolver>;
    try {
      found = this.o.resolve(folder);
    } catch (e) {
      return { message: `Codex's binary wasn't found: ${(e as Error).message}`, output: '' };
    }
    if (!found.native) return { message: "Codex's native binary wasn't found in the install", output: '' };
    const v = await this.o.exec(found.command, ['--version'], { timeoutMs: 30_000 });
    if (v.code !== 0 || !v.stdout.includes(version)) return { message: `codex --version said ${JSON.stringify((v.stdout || v.stderr).trim().slice(0, 80))}, not ${version}`, output: v.stdout + v.stderr };
    const help = await this.o.exec(found.command, ['app-server', '--help'], { timeoutMs: 30_000 });
    if (help.code !== 0) return { message: `codex app-server --help failed (exit ${help.code})`, output: help.stdout + help.stderr };
    return { binary: found.command, message: '', output: '' };
  }

  // Offline: Codex writes out its own protocol, and checkSurface compares it
  // with what the runner relies on.
  private async gate(binary: string): Promise<{ result?: { missing: string[]; added: string[] }; message: string; output: string }> {
    const out = fs.mkdtempSync(path.join(this.o.tmpDir ?? os.tmpdir(), 'tb-codex-schema-'));
    try {
      const g = await this.o.exec(binary, ['app-server', 'generate-ts', '--experimental', '--out', out], { timeoutMs: 60_000 });
      if (g.code !== 0) return { message: `codex app-server generate-ts failed (exit ${g.code})`, output: g.stdout + g.stderr };
      return { result: this.o.checkSurface(out), message: '', output: '' };
    } catch (e) {
      return { message: `The schema check failed: ${(e as Error).message}`, output: '' };
    } finally {
      fs.rmSync(out, { recursive: true, force: true });
    }
  }

  // Points `current` at an installed version by renaming a new link over
  // it, as runner releases do, so there's never a moment without one.
  activate(version: string): { previous?: string } {
    if (!this.has(version)) throw new Error(`Codex ${version} isn't installed`);
    const before = this.currentVersion();
    const tmp = path.join(this.dir, `.current-${process.pid}-${crypto.randomBytes(4).toString('hex')}`);
    fs.symlinkSync(version, tmp);
    fs.renameSync(tmp, this.currentLink);
    const s = this.state();
    if (before && before !== version) s.previous = before;
    if (s.previous === version) s.previous = before !== version ? before : undefined;
    s.current = version;
    if (s.hold && compareVersions(version, s.hold.version) >= 0) delete s.hold;
    s.installs.push({ version, at: new Date(this.now()).toISOString(), result: 'activated' });
    this.saveState(s);
    return { previous: before !== version ? before : undefined };
  }

  setHold(hold: CodexHold): void {
    const s = this.state();
    s.hold = hold;
    s.installs.push({ version: hold.version, at: new Date(this.now()).toISOString(), result: 'held', message: hold.reason });
    this.saveState(s);
  }

  recordFailure(version: string, message: string): void {
    const s = this.state();
    s.installs.push({ version, at: new Date(this.now()).toISOString(), result: 'failed', message });
    this.saveState(s);
  }

  // Deletes versions beyond the newest `keep` (current and previous always
  // stay, and a held one, which "Update anyway" uses), but only ones no
  // process runs: not named by the runner's own Codex processes, and not in
  // any process's arguments (`ps -axo args=`: arguments only, never
  // environments).
  cleanup(keep: number, binariesInUse: string[], processArgs: string): string[] {
    const s = this.state();
    const keepSet = new Set<string>([s.current, s.previous, s.hold?.version, this.currentVersion()].filter((v): v is string => !!v));
    const versions = this.versions().reverse(); // newest first
    for (const v of versions) {
      if (keepSet.size >= keep) break;
      keepSet.add(v);
    }
    const inUse = binariesInUse.flatMap((b) => [b, realpath(b)]);
    const removed: string[] = [];
    for (const v of versions) {
      if (keepSet.has(v)) continue;
      const folder = path.join(this.dir, v);
      const forms = [folder, realpath(folder)];
      const used = inUse.some((b) => forms.some((f) => b === f || b.startsWith(f + path.sep))) || forms.some((f) => processArgs.includes(f + path.sep) || processArgs.split('\n').some((l) => l.trim() === f));
      if (used) continue;
      fs.rmSync(folder, { recursive: true, force: true });
      removed.push(v);
    }
    return removed;
  }

  private now(): number {
    return this.o.now?.() ?? Date.now();
  }
}

// The version of the @openai/codex package a path belongs to: an install
// folder, the npm launcher link, or codex.js.
export function codexPackageVersion(start: string): string | undefined {
  const candidates: string[] = [];
  try {
    if (fs.statSync(start).isDirectory()) candidates.push(path.join(start, 'node_modules', ...CODEX_PACKAGE.split('/'), 'package.json'));
  } catch {
    return undefined;
  }
  let dir = path.dirname(realpath(start));
  for (let i = 0; i < 4; i++) {
    candidates.push(path.join(dir, 'package.json'));
    dir = path.dirname(dir);
  }
  for (const c of candidates) {
    const pkg = readJsonSync<{ name?: string; version?: string }>(c);
    if (pkg?.name === CODEX_PACKAGE && typeof pkg.version === 'string') return pkg.version;
  }
  return undefined;
}

function realpath(p: string): string {
  try {
    return fs.realpathSync(p);
  } catch {
    return p;
  }
}

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === 'EPERM';
  }
}
