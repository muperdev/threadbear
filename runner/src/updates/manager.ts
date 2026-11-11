// Keeps the Claude, Codex, Grok and Devin CLIs up to date ("E. CLI updates" in
// docs/step4-plan.md). It checks an hour apart, installs by each provider's
// policy, never while a runner restart is pending, and reports everything
// through onChange so no RPC waits on an install.
//
// Updating is safe while turns run. Claude's installer keeps each version
// in its own file. Codex runs from Threadbear's own versioned copies
// (codex.ts), which nothing else touches. After an update the runner
// releases idle sessions on the old version (hooks.releaseIdle), and the
// rest move as their turns end; every new process starts the new version.
// Grok's own `grok update` swaps a link to a file per version, as Claude's
// does, and Devin's versions are folders of their own (grok.ts, devin.ts).
import fs from 'node:fs';
import path from 'node:path';
import type { Clock, TimerHandle } from '../clock.ts';
import type { Config } from '../config.ts';
import type { Logger } from '../log.ts';
import { paths } from '../config.ts';
import type { CliPolicy, CliStatus, Provider } from '../protocol.ts';

// The CLIs Threadbear checks and updates. Grok and Devin are listed only
// once they're installed: most copies never run them. An ACP agent's CLI
// is reported and updated only when asked (acpStatus), never on the hour.
export const MANAGED_CLIS = ['claude', 'codex', 'grok', 'devin'] as const;
const LISTED_WHEN_INSTALLED: readonly ManagedCli[] = ['grok', 'devin'];
export type ManagedCli = (typeof MANAGED_CLIS)[number];
export function isManagedCli(p: Provider): p is ManagedCli {
  return (MANAGED_CLIS as readonly string[]).includes(p);
}
import { claudeChannel, claudeInstalledVersion, claudeLatestVersion, runClaudeUpdate } from './claude.ts';
import { grokInstalledVersion, grokLatestVersion, runGrokUpdate } from './grok.ts';
import { cleanupDevinVersions, devinInstall, devinInstalledVersion, devinLatestVersion, runDevinUpdate, updatableInstall } from './devin.ts';
import { CodexCopies, NPM_LATEST, codexPackageVersion, type CodexResolver, type SurfaceCheck } from './codex.ts';
import { VERSION_RE, compareVersions, defaultExec, defaultFetchText, type Exec, type FetchText } from './exec.ts';
import { cliName } from '../providers.ts';
import { UpdateHistory, type ProviderState, type UpdatesFile } from './history.ts';

export const FIRST_CHECK_MS = 60_000;
export const CHECK_JITTER_MS = 5 * 60_000;
export const LOOKUP_TIMEOUT_MS = 5_000;
export const RUN_TIMEOUT_MS = 5 * 60_000;
export const CLAUDE_LOCK_RETRY_MS = 10 * 60_000;
export const CLAUDE_LOCK_TRIES = 3;
export const RESTART_RETRY_MS = 60_000;
export const KEEP_DEVIN_VERSIONS = 2;

export type CliUpdatesHooks = {
  restartPending(): boolean;
  // Release sessions of that provider that are idle (no turn, approvals,
  // queued messages or background work) and not on keepVersion.
  releaseIdle(p: Provider, keepVersion: string): void;
  versionsInUse(p: Provider): { version: string; threadIds: string[] }[];
  // The real binary paths the runner's live Codex processes started.
  codexBinariesInUse(): string[];
  // The same for Devin (its version folders), so cleanup leaves them.
  devinBinariesInUse?(): string[];
};

export type CliUpdatesDeps = {
  cfg: Config;
  log: Logger;
  clock: Clock;
  hooks: CliUpdatesHooks;
  // From piece A (adapters/codex/appserver.ts and surface.ts): finding the
  // native binary in an install, the schema gate, and the version the
  // runner was tested against (installed when the latest fails the gate on
  // a first start).
  codex: { resolve: CodexResolver; checkSurface: SurfaceCheck; testedVersion: string };
  // A runner.notice warning (a version that keeps failing to start).
  notice?: (level: 'warning' | 'info', message: string) => void;
  exec?: Exec;
  fetchText?: FetchText;
  env?: NodeJS.ProcessEnv;
  execPath?: string; // the runner's Node: npm and the fallback codex sit next to it
  random?: () => number;
  // The ACP agents, with the experiment's switch.
  acp?: { on(): boolean; providers: Provider[]; binary(p: Provider): string | undefined; updateArgs(p: Provider): string[] };
};

type UpdateOptions = { version?: string; force?: boolean; bootstrap?: boolean };

const ACP_UPDATE_TIMEOUT_MS = 5 * 60_000;

export class CliUpdates {
  private d: CliUpdatesDeps;
  private cfg: Config;
  private exec: Exec;
  private fetchText: FetchText;
  private env: NodeJS.ProcessEnv;
  private execPath: string;
  private history: UpdateHistory;
  private copies: CodexCopies;
  private st: UpdatesFile;
  private installed: Partial<Record<Provider, string>> = {};
  private running: Partial<Record<Provider, Promise<void>>> = {};
  private work = new Set<Promise<void>>();
  private timers = new Map<string, TimerHandle>();
  private listeners = new Set<(clis: CliStatus[]) => void>();
  private retryOptions: Partial<Record<Provider, UpdateOptions>> = {};
  private stopped = false;
  // The Claude version before the newest update, for the "go back" hint.
  private lastClaude?: string;
  // Grok's and Devin's, the same.
  private previous: Partial<Record<ManagedCli, string>> = {};

  constructor(d: CliUpdatesDeps) {
    this.d = d;
    this.cfg = d.cfg;
    this.exec = d.exec ?? defaultExec;
    this.fetchText = d.fetchText ?? defaultFetchText;
    this.env = d.env ?? process.env;
    this.execPath = d.execPath ?? process.execPath;
    this.history = new UpdateHistory(d.cfg.home);
    this.st = this.history.read();
    this.copies = new CodexCopies({
      dir: path.join(d.cfg.home, 'tools', 'codex'),
      exec: this.exec,
      npmPath: path.join(path.dirname(this.execPath), 'npm'),
      resolve: d.codex.resolve,
      checkSurface: d.codex.checkSurface,
      tmpDir: paths.run(d.cfg.home),
      env: this.env,
      now: () => d.clock.now(),
    });
    // A previous update that was running when the runner stopped didn't
    // finish; say so rather than show it running forever.
    for (const p of Object.keys(this.st) as Provider[]) {
      const u = this.st[p]?.update;
      if (u && (u.state === 'running' || u.state === 'waiting')) this.st[p]!.update = { ...u, state: 'failed', message: 'The runner stopped before this finished.' };
    }
  }

  start(): void {
    this.stopped = false;
    if (this.managedCodex() && this.copies.cleanPartials().length) this.d.log.info('cli updates: removed an unfinished Codex install');
    this.track('claude', this.refreshInstalled());
    for (const p of this.acpProviders()) this.track(p, this.refreshAcp(p));
    this.schedule('check', FIRST_CHECK_MS);
  }

  stop(): void {
    this.stopped = true;
    for (const t of this.timers.values()) t.cancel();
    this.timers.clear();
  }

  // Resolves once nothing is checking or installing (for tests and for a
  // clean shutdown).
  async settled(): Promise<void> {
    while (this.work.size) await Promise.allSettled([...this.work]);
  }

  // What the next Codex process should start: the binary set by hand, else
  // Threadbear's own `current`, else (before the first copy is installed,
  // or with updates off, as in tests and dev runners) the codex next to the
  // runner's own Node. Never ~/.local/bin/codex.
  codexBinary(): string {
    if (this.cfg.codexPath) return this.cfg.codexPath;
    if (!this.configOff('codex') && this.copies.currentVersion()) return this.copies.currentLink;
    return this.fallbackCodex();
  }

  list(): CliStatus[] {
    return [...this.listed().map((p) => this.status(p)), ...this.acpProviders().map((p) => this.acpStatus(p))];
  }

  // The binary each CLI's new sessions start.
  private binaryOf(p: ManagedCli): string {
    if (p === 'claude') return this.cfg.claudePath;
    if (p === 'grok') return this.cfg.grokPath;
    if (p === 'devin') return this.cfg.devinPath;
    return this.codexBinary();
  }

  check(p?: Provider): CliStatus[] {
    if (p && !isManagedCli(p)) {
      if (this.acpProviders().includes(p)) this.track(p, this.refreshAcp(p));
      return this.list();
    }
    for (const prov of p ? [p] : this.listed()) this.track(prov, this.runCheck(prov, true));
    if (!p) for (const prov of this.acpProviders()) this.track(prov, this.refreshAcp(prov));
    return this.list();
  }

  update(p: Provider, o: { version?: string; force?: boolean } = {}): CliStatus[] {
    if (!isManagedCli(p)) {
      if (this.acpProviders().includes(p) && !this.configOff(p)) this.track(p, this.updateAcp(p));
      return this.list();
    }
    this.startUpdate(p, o);
    return this.list();
  }

  setPolicy(p: Provider, policy: CliPolicy): CliStatus[] {
    if (!isManagedCli(p)) return this.list();
    this.ps(p).policy = policy;
    this.save();
    this.changed();
    if (policy === 'auto') this.track(p, this.runCheck(p, false));
    return this.list();
  }

  onChange(fn: (clis: CliStatus[]) => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  // The runner tells this whether a session on `version` got as far as the
  // CLI's init (Claude) or initialize (Codex). Two failures in a row on a
  // version it just moved to send a warning, once.
  sessionStarted(p: Provider, version: string | undefined, ok: boolean): void {
    if (!version || !isManagedCli(p)) return;
    const s = this.ps(p);
    if (ok) {
      if (s.startFailures?.version === version) {
        delete s.startFailures;
        this.save();
      }
      return;
    }
    const f = s.startFailures?.version === version ? s.startFailures : { version, count: 0 };
    f.count++;
    s.startFailures = f;
    if (f.count >= 2 && !f.warned) {
      f.warned = true;
      const prev = p === 'codex' ? this.copies.state().previous : undefined;
      const message =
        p === 'claude'
          ? `Claude Code ${version} failed to start twice. To go back: claude install ${this.lastClaude ?? '<version>'}`
          : p === 'codex'
            ? `Codex ${version} failed to start twice.${prev ? ` Go back to ${prev} in Usage.` : ''}`
            : `${p === 'grok' ? 'Grok Build' : 'Devin'} ${version} failed to start twice.${this.previous[p] ? ` To go back: ${p === 'grok' ? `grok update --version ${this.previous[p]}` : `Update to ${this.previous[p]} from Usage`}` : ''}`;
      this.d.notice?.('warning', message);
      this.history.append({ at: this.iso(), provider: p, action: 'notice', ok: false, version, message });
    }
    this.save();
  }

  // ---- status

  // ---- The ACP agents (adapters/acp/): only what can be known without a
  // version feed. Never updated without being asked.

  private acpProviders(): Provider[] {
    const acp = this.d.acp;
    return acp?.on() ? acp.providers.filter((p) => acp.binary(p)) : [];
  }

  private acpStatus(p: Provider): CliStatus {
    const installedVersion = this.installed[p];
    const s = this.st[p] ?? {};
    return {
      provider: p,
      path: this.d.acp!.binary(p) ?? '',
      managedBy: 'cli',
      status: 'unknown',
      policy: 'off',
      canUpdateNow: true,
      inUse: this.d.hooks.versionsInUse(p),
      ...(installedVersion ? { installedVersion } : {}),
      ...(s.update ? { update: { ...s.update } } : {}),
    };
  }

  private async refreshAcp(p: Provider): Promise<void> {
    const binary = this.d.acp?.binary(p);
    if (!binary) return;
    const r = await this.exec(binary, ['--version'], { timeoutMs: LOOKUP_TIMEOUT_MS });
    const version = r.code === 0 ? /^\S{1,60}/.exec(r.stdout.trim())?.[0] : undefined;
    if (version && this.installed[p] !== version) {
      this.installed[p] = version;
      this.changed();
    }
  }

  private async updateAcp(p: Provider): Promise<void> {
    const binary = this.d.acp?.binary(p);
    if (!binary || this.ps(p).update?.state === 'running') return;
    this.setUpdate(p, { state: 'running', message: `Updating ${cliName(p)}` });
    await this.refreshAcp(p);
    const before = this.installed[p];
    const r = await this.exec(binary, this.d.acp!.updateArgs(p), { timeoutMs: ACP_UPDATE_TIMEOUT_MS });
    await this.refreshAcp(p);
    const now = this.installed[p];
    if (r.code !== 0) {
      this.history.append({ at: this.iso(), provider: p, action: 'update', ok: false, message: 'failed', output: r.stderr || r.stdout });
      return this.finish(p, { state: 'failed', message: (r.stderr || r.stdout).trim().split('\n').at(-1)?.slice(0, 200) || `${cliName(p)} didn't update.` });
    }
    this.history.append({ at: this.iso(), provider: p, action: 'update', ok: true, ...(now ? { version: now } : {}), message: `from ${before ?? 'unknown'}` });
    this.finish(p, { state: now && now !== before ? 'succeeded' : 'unchanged', ...(now ? { targetVersion: now } : {}) });
    if (now && now !== before) this.d.hooks.releaseIdle(p, now);
  }

  private status(p: ManagedCli): CliStatus {
    const s = this.st[p] ?? {};
    const installedVersion = this.installed[p];
    const latestVersion = s.latestVersion;
    const codexState = p === 'codex' && this.managedCodex() ? this.copies.state() : undefined;
    const hold = codexState?.hold;
    const binary = this.binaryOf(p);
    let status: CliStatus['status'];
    if (!installedVersion) status = fs.existsSync(binary) ? 'unknown' : 'missing';
    else if (hold && latestVersion === hold.version && compareVersions(installedVersion, hold.version) < 0) status = 'held';
    else if (!latestVersion) status = 'unknown';
    else status = compareVersions(installedVersion, latestVersion) < 0 ? 'behind' : 'current';
    const out: CliStatus = {
      provider: p,
      path: binary,
      managedBy: this.managedBy(p),
      status,
      policy: this.policy(p),
      inUse: this.d.hooks.versionsInUse(p),
    };
    if (installedVersion) out.installedVersion = installedVersion;
    if (latestVersion) out.latestVersion = latestVersion;
    if (codexState?.previous && this.copies.has(codexState.previous)) out.previousVersion = codexState.previous;
    if (p === 'devin' && this.previous.devin && this.previous.devin !== installedVersion) out.previousVersion = this.previous.devin;
    if (s.checkedAt) out.checkedAt = s.checkedAt;
    if (hold) out.hold = { ...hold };
    if (s.update) out.update = { ...s.update };
    return out;
  }

  // Claude and Grok update themselves when asked; Threadbear installs
  // Codex's copies and Devin's versions. A Codex set by hand, or a Devin
  // from Homebrew or set by hand, is left alone.
  private managedBy(p: ManagedCli): CliStatus['managedBy'] {
    if (p === 'claude' || p === 'grok') return 'cli';
    if (p === 'devin') return updatableInstall(devinInstall(this.cfg.devinPath)) ? 'threadbear' : 'none';
    return this.cfg.codexPath ? 'none' : 'threadbear';
  }

  private listed(): ManagedCli[] {
    return MANAGED_CLIS.filter((p) => !LISTED_WHEN_INSTALLED.includes(p) || fs.existsSync(this.binaryOf(p)));
  }

  // THREADBEAR_CLI_UPDATES=off (tests, dev runners) and runner.json's own
  // "off" can't be turned on from the app; a Codex binary set by hand is
  // never updated, nor a Devin Threadbear can't update.
  private policy(p: ManagedCli): CliPolicy {
    if (this.configOff(p)) return 'off';
    if (p === 'codex' && this.cfg.codexPath) return 'off';
    if (p === 'devin' && this.managedBy('devin') === 'none') return 'off';
    return this.st[p]?.policy ?? this.cfg.cliUpdates[p] ?? 'off';
  }

  private configOff(p: Provider): boolean {
    return this.env.THREADBEAR_CLI_UPDATES === 'off' || this.cfg.cliUpdates[p] === 'off';
  }

  private managedCodex(): boolean {
    return !this.cfg.codexPath;
  }

  private fallbackCodex(): string {
    return path.join(path.dirname(this.execPath), 'codex');
  }

  private async refreshInstalled(): Promise<void> {
    const claude = await claudeInstalledVersion(this.cfg.claudePath, this.exec);
    const bin = this.codexBinary();
    const codex = bin === this.copies.currentLink ? this.copies.currentVersion() : codexPackageVersion(bin);
    const grok = fs.existsSync(this.cfg.grokPath) ? await grokInstalledVersion(this.cfg.grokPath, this.exec) : undefined;
    const devin = await devinInstalledVersion(this.cfg.devinPath, this.exec);
    const changed = claude !== this.installed.claude || codex !== this.installed.codex || grok !== this.installed.grok || devin !== this.installed.devin;
    this.installed = { ...this.installed, claude, codex, grok, devin };
    if (claude) this.lastClaude ??= claude;
    if (changed) this.changed();
  }

  // ---- checking

  private schedule(name: string, ms: number): void {
    if (this.stopped) return;
    this.timers.get(name)?.cancel();
    this.timers.set(
      name,
      this.d.clock.setTimeout(() => {
        this.timers.delete(name);
        if (name === 'check') {
          for (const p of this.listed()) this.track(p, this.runCheck(p, false));
          for (const prov of this.acpProviders()) this.track(prov, this.refreshAcp(prov));
          const jitter = Math.floor((this.d.random ?? Math.random)() * CHECK_JITTER_MS);
          this.schedule('check', this.cfg.cliUpdates.checkMinutes * 60_000 + jitter);
        } else if (name.startsWith('retry:')) {
          const [, p, attempt] = name.split(':') as [string, ManagedCli, string];
          const o = this.retryOptions[p] ?? {};
          delete this.retryOptions[p];
          this.startUpdate(p, o, Number(attempt));
        }
      }, ms),
    );
  }

  private async runCheck(p: ManagedCli, asked: boolean): Promise<void> {
    await this.refreshInstalled();
    const policy = this.policy(p);
    if (policy === 'off' && !asked) return;
    if (this.env.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC) return;
    try {
      const latest = await this.latestOf(p);
      const s = this.ps(p);
      if (typeof latest === 'string' && VERSION_RE.test(latest)) {
        if (latest !== s.latestVersion) this.history.append({ at: this.iso(), provider: p, action: 'check', ok: true, version: latest });
        s.latestVersion = latest;
      }
      s.checkedAt = this.iso();
      this.save();
      this.changed();
    } catch (e) {
      // The last known value stays.
      this.d.log.info('cli updates: version check failed', { provider: p, error: (e as Error).message });
    }
    if (this.policy(p) !== 'auto') return;
    const latest = this.st[p]?.latestVersion;
    if (p === 'codex' && this.managedCodex() && !this.copies.currentVersion()) {
      // First start with updates on: install a copy of our own.
      this.startUpdate('codex', { bootstrap: true });
      return;
    }
    const installed = this.installed[p];
    if (!latest || !installed || compareVersions(installed, latest) >= 0) return;
    if (p === 'codex' && this.copies.state().hold?.version === latest) return;
    this.startUpdate(p, {});
  }

  private async latestOf(p: ManagedCli): Promise<unknown> {
    if (p === 'claude') return claudeLatestVersion(this.fetchText, claudeChannel(this.cfg.claudeMainDir), LOOKUP_TIMEOUT_MS);
    // Grok asks its own update server; it takes a few seconds more than a fetch.
    if (p === 'grok') return fs.existsSync(this.cfg.grokPath) ? grokLatestVersion(this.cfg.grokPath, this.exec, this.env, 4 * LOOKUP_TIMEOUT_MS) : undefined;
    if (p === 'devin') return devinLatestVersion(this.fetchText, LOOKUP_TIMEOUT_MS);
    return (JSON.parse(await this.fetchText(NPM_LATEST, LOOKUP_TIMEOUT_MS)) as { version?: unknown }).version;
  }

  // ---- updating

  private startUpdate(p: ManagedCli, o: UpdateOptions, attempt = 1): void {
    if (this.running[p] || this.stopped) return;
    const target = o.version ?? this.st[p]?.latestVersion;
    if (this.d.hooks.restartPending()) {
      this.setUpdate(p, { state: 'waiting', targetVersion: target, waitingFor: ['the runner restart'] });
      this.retryOptions[p] = o;
      this.schedule(`retry:${p}:${attempt}`, RESTART_RETRY_MS);
      return;
    }
    this.setUpdate(p, { state: 'running', targetVersion: target });
    const run = p === 'claude' ? this.updateClaude(attempt) : p === 'codex' ? this.updateCodex(o) : this.updateOwn(p, o);
    const promise = run.catch((e: Error) => {
      this.d.log.error('cli updates: update failed', { provider: p, error: e.message });
      this.finish(p, { state: 'failed', targetVersion: target, message: e.message });
    });
    this.running[p] = promise;
    this.track(
      p,
      promise.finally(() => {
        delete this.running[p];
      }),
    );
  }

  private async updateClaude(attempt: number): Promise<void> {
    const before = this.installed.claude;
    const r = await runClaudeUpdate({ claudePath: this.cfg.claudePath, cwd: paths.run(this.cfg.home), exec: this.exec, env: this.env, timeoutMs: RUN_TIMEOUT_MS });
    const target = this.st.claude?.latestVersion;
    if (r.kind === 'updated') {
      this.lastClaude = r.from ?? before;
      this.installed.claude = r.to;
      this.history.append({ at: this.iso(), provider: 'claude', action: 'update', ok: true, version: r.to, message: `from ${r.from ?? 'unknown'}`, output: r.output });
      this.finish('claude', { state: 'succeeded', targetVersion: r.to });
      this.d.hooks.releaseIdle('claude', r.to);
    } else if (r.kind === 'locked') {
      // Another Claude (T3, Token Stats, a terminal) holds the installer's
      // lock. Nothing happened; try again in a while.
      this.history.append({ at: this.iso(), provider: 'claude', action: 'update', ok: false, message: 'installer busy', output: r.output });
      if (attempt < CLAUDE_LOCK_TRIES) {
        this.setUpdate('claude', { state: 'waiting', targetVersion: target, waitingFor: ['another Claude Code process'], message: 'Another Claude Code process is installing. Trying again in 10 minutes.' });
        this.schedule(`retry:claude:${attempt + 1}`, CLAUDE_LOCK_RETRY_MS);
      } else this.finish('claude', { state: 'failed', targetVersion: target, message: `Another Claude Code process kept the installer busy ${CLAUDE_LOCK_TRIES} times.` });
    } else if (r.kind === 'unchanged') {
      this.history.append({ at: this.iso(), provider: 'claude', action: 'update', ok: true, version: r.version, message: 'unchanged', output: r.output });
      this.finish('claude', { state: 'unchanged', targetVersion: r.version });
    } else {
      this.history.append({ at: this.iso(), provider: 'claude', action: 'update', ok: false, message: r.message, output: r.output });
      this.finish('claude', { state: 'failed', targetVersion: target, message: r.message });
    }
    await this.refreshInstalled();
  }

  // Grok (its own `grok update`) and Devin (its installer's steps, here).
  private async updateOwn(p: 'grok' | 'devin', o: UpdateOptions): Promise<void> {
    const before = this.installed[p];
    const r =
      p === 'grok'
        ? await runGrokUpdate({ grokPath: this.cfg.grokPath, cwd: paths.run(this.cfg.home), exec: this.exec, env: this.env, timeoutMs: RUN_TIMEOUT_MS, version: o.version })
        : await runDevinUpdate({ devinPath: this.cfg.devinPath, exec: this.exec, fetchText: this.fetchText, timeoutMs: RUN_TIMEOUT_MS, lookupMs: LOOKUP_TIMEOUT_MS, version: o.version });
    const target = o.version ?? this.st[p]?.latestVersion;
    if (r.kind === 'updated') {
      this.previous[p] = r.from ?? before;
      this.installed[p] = r.to;
      this.history.append({ at: this.iso(), provider: p, action: 'update', ok: true, version: r.to, message: `from ${r.from ?? 'unknown'}`, output: r.output });
      this.finish(p, { state: 'succeeded', targetVersion: r.to });
      this.d.hooks.releaseIdle(p, r.to);
      if (p === 'devin') this.track(p, this.cleanupDevin());
    } else if (r.kind === 'unchanged') {
      this.history.append({ at: this.iso(), provider: p, action: 'update', ok: true, version: r.version, message: 'unchanged', output: r.output });
      this.finish(p, { state: 'unchanged', targetVersion: r.version });
    } else {
      this.history.append({ at: this.iso(), provider: p, action: 'update', ok: false, message: r.message, output: r.output });
      this.finish(p, { state: 'failed', targetVersion: target, message: r.message });
    }
    await this.refreshInstalled();
  }

  // Devin's old version folders (185 MB each) go once nothing runs them.
  private async cleanupDevin(): Promise<void> {
    const ps = await this.exec('/bin/ps', ['-axo', 'args='], { timeoutMs: 10_000 });
    if (ps.code !== 0) return;
    const inUse = (this.d.hooks.devinBinariesInUse?.() ?? []).join('\n');
    const removed = cleanupDevinVersions(this.cfg.devinPath, KEEP_DEVIN_VERSIONS, `${ps.stdout}\n${inUse}`);
    if (removed.length) this.history.append({ at: this.iso(), provider: 'devin', action: 'cleanup', ok: true, message: `removed ${removed.join(', ')}` });
  }

  private async updateCodex(o: UpdateOptions): Promise<void> {
    if (!this.managedCodex()) {
      this.finish('codex', { state: 'failed', message: 'codexPath is set by hand, so Threadbear leaves that Codex alone.' });
      return;
    }
    const target = o.version ?? this.st.codex?.latestVersion ?? (o.bootstrap ? this.d.codex.testedVersion : undefined);
    if (!target) {
      this.finish('codex', { state: 'failed', message: "The latest Codex version isn't known yet." });
      return;
    }
    if (!this.copies.lock()) {
      this.finish('codex', { state: 'failed', targetVersion: target, message: 'Another runner on this home is installing Codex.' });
      return;
    }
    try {
      await this.installCodex(target, o);
    } finally {
      this.copies.unlock();
    }
  }

  private async installCodex(target: string, o: UpdateOptions): Promise<void> {
    const state = this.copies.state();
    // "Go back": an older version asked for by name. The one left behind
    // is held, or the next check would put it straight back from its kept
    // folder. A newer one still goes through the gate; force installs it.
    const from = this.copies.currentVersion();
    const wentBackFrom = o.version !== undefined && from && compareVersions(target, from) < 0 ? from : undefined;
    if (this.copies.currentVersion() === target) {
      this.finish('codex', { state: 'unchanged', targetVersion: target });
      return;
    }
    // A kept version ("Go back", or a held one being forced) is already
    // here: no download, and it passed the checks when it was installed.
    if (this.copies.has(target) && (o.force || state.hold?.version !== target)) {
      this.activateCodex(target, [], wentBackFrom);
      return;
    }
    if (this.copies.has(target) && state.hold?.version === target) {
      this.finish('codex', { state: 'unchanged', targetVersion: target, message: `Codex ${target} is held back because ${state.hold.reason}.` });
      return;
    }
    this.setUpdate('codex', { state: 'running', targetVersion: target, message: `Installing Codex ${target}` });
    const r = await this.copies.install(target, { force: o.force });
    if (r.kind === 'failed') {
      this.copies.cleanPartials();
      this.copies.recordFailure(target, r.message);
      this.history.append({ at: this.iso(), provider: 'codex', action: 'install', ok: false, version: target, message: r.message, output: r.output });
      // On a first start, a latest that won't install still leaves the
      // tested version to try.
      if (o.bootstrap && target !== this.d.codex.testedVersion && !this.copies.currentVersion()) return this.installCodex(this.d.codex.testedVersion, { bootstrap: false });
      this.finish('codex', { state: 'failed', targetVersion: target, message: r.message });
      return;
    }
    this.history.append({ at: this.iso(), provider: 'codex', action: 'install', ok: true, version: target, output: r.output });
    if (r.kind === 'held') {
      const reason = 'the app-server protocol changed';
      this.copies.setHold({ version: target, reason, missing: r.missing });
      this.history.append({ at: this.iso(), provider: 'codex', action: 'hold', ok: false, version: target, message: reason, missing: r.missing, added: r.added });
      this.d.log.warn('cli updates: Codex held back', { version: target, missing: r.missing });
      if (o.bootstrap && target !== this.d.codex.testedVersion && !this.copies.currentVersion()) return this.installCodex(this.d.codex.testedVersion, { bootstrap: false });
      this.finish('codex', { state: 'unchanged', targetVersion: target, message: `Codex ${target} is held back because ${reason}.` });
      return;
    }
    this.activateCodex(target, r.added, wentBackFrom);
  }

  private activateCodex(version: string, added: string[], wentBackFrom?: string): void {
    const { previous } = this.copies.activate(version);
    if (wentBackFrom) {
      const reason = 'you went back from it';
      this.copies.setHold({ version: wentBackFrom, reason });
      this.history.append({ at: this.iso(), provider: 'codex', action: 'hold', ok: true, version: wentBackFrom, message: reason });
    }
    this.installed.codex = version;
    this.history.append({ at: this.iso(), provider: 'codex', action: 'activate', ok: true, version, message: previous ? `from ${previous}` : undefined, ...(added.length ? { added } : {}) });
    this.finish('codex', { state: 'succeeded', targetVersion: version });
    this.d.hooks.releaseIdle('codex', version);
    this.track('codex', this.cleanupCodex());
  }

  // Old versions go once nothing runs them; the next activation or start
  // tries again for one still in use.
  private async cleanupCodex(): Promise<void> {
    const ps = await this.exec('/bin/ps', ['-axo', 'args='], { timeoutMs: 10_000 });
    if (ps.code !== 0) return; // without the process list, nothing is safe to delete
    const removed = this.copies.cleanup(this.cfg.cliUpdates.keepCodexVersions, this.d.hooks.codexBinariesInUse(), ps.stdout);
    if (removed.length) this.history.append({ at: this.iso(), provider: 'codex', action: 'cleanup', ok: true, message: `removed ${removed.join(', ')}` });
  }

  // ---- state

  private ps(p: Provider): ProviderState {
    return (this.st[p] ??= {});
  }

  private setUpdate(p: Provider, u: NonNullable<CliStatus['update']>): void {
    this.ps(p).update = stripUndefined(u);
    this.save();
    this.changed();
  }

  private finish(p: Provider, u: NonNullable<CliStatus['update']>): void {
    this.setUpdate(p, { ...u, finishedAt: this.iso() });
  }

  private save(): void {
    try {
      this.history.write(this.st);
    } catch (e) {
      this.d.log.warn('cli updates: could not save updates.json', { error: (e as Error).message });
    }
  }

  private changed(): void {
    const clis = this.list();
    for (const fn of this.listeners) {
      try {
        fn(clis);
      } catch (e) {
        this.d.log.error('cli updates: listener failed', { error: (e as Error).message });
      }
    }
  }

  private track(_p: Provider, promise: Promise<void>): void {
    const tracked = promise.catch((e: Error) => this.d.log.error('cli updates: failed', { error: e.message }));
    this.work.add(tracked);
    void tracked.finally(() => this.work.delete(tracked));
  }

  private iso(): string {
    return new Date(this.d.clock.now()).toISOString();
  }
}

function stripUndefined<T extends Record<string, unknown>>(o: T): T {
  return Object.fromEntries(Object.entries(o).filter(([, v]) => v !== undefined)) as T;
}
