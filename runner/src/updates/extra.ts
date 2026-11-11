// Updates for CLIs Threadbear doesn't run itself but you use alongside it
// (Grok, Muse, a Codex on your PATH), from runner.json's cliUpdates.extra:
//
//   "cliUpdates": { "extra": [
//     { "name": "grok", "command": ["~/.local/bin/grok", "update"] },
//     { "name": "muse", "command": ["~/.local/bin/muse"], "env": { "MUSE_LAUNCHER_INSTALL": "1" } }
//   ] }
//
// Each runs once a day, one at a time, a minute after the runner starts
// if its day is up. Mike's Token Stats app did this until 9 Nov 2026; it
// moved here so that app could go. Claude and Codex for threads stay with
// manager.ts, which knows their versions and what's running on them; these
// are just commands, so nothing here reads a version or a login.
import os from 'node:os';
import path from 'node:path';
import type { Clock, TimerHandle } from '../clock.ts';
import type { Logger } from '../log.ts';
import { atomicWriteFileSync, readJsonSync } from '../fsutil.ts';
import { defaultExec, type Exec } from './exec.ts';

export type ExtraUpdate = { name: string; command: string[]; env?: Record<string, string> };

export const FIRST_RUN_MS = 60_000;
export const DAY_MS = 24 * 60 * 60_000;
export const RUN_TIMEOUT_MS = 10 * 60_000;

type LastRun = { at: number; ok: boolean; code: number; output?: string };
type StateFile = Record<string, LastRun>;

// runner.json's cliUpdates.extra, keeping only entries with a name and a
// command as a list of strings (a leading ~/ is your home). THREADBEAR_CLI_UPDATES=off
// turns these off too: tests and dev runners never install anything.
export function extraUpdatesFrom(raw: unknown, envValue: string | undefined, home = os.homedir()): ExtraUpdate[] {
  if (envValue === 'off' || !Array.isArray(raw)) return [];
  const out: ExtraUpdate[] = [];
  for (const e of raw) {
    if (!e || typeof e !== 'object') continue;
    const { name, command, env } = e as Record<string, unknown>;
    if (typeof name !== 'string' || !name.trim()) continue;
    if (!Array.isArray(command) || !command.length || !command.every((c) => typeof c === 'string' && c)) continue;
    const u: ExtraUpdate = { name: name.trim(), command: command.map((c, i) => (i === 0 && c.startsWith('~/') ? path.join(home, c.slice(2)) : c)) };
    if (env && typeof env === 'object' && !Array.isArray(env)) {
      const vars = Object.fromEntries(Object.entries(env).filter((v): v is [string, string] => typeof v[1] === 'string'));
      if (Object.keys(vars).length) u.env = vars;
    }
    out.push(u);
  }
  return out;
}

export type ExtraUpdatesDeps = {
  home: string; // the runner's data dir
  log: Logger;
  clock: Clock;
  extra: ExtraUpdate[];
  exec?: Exec;
  env?: NodeJS.ProcessEnv;
};

export class ExtraUpdates {
  private d: ExtraUpdatesDeps;
  private exec: Exec;
  private file: string;
  private timer?: TimerHandle;
  private work?: Promise<void>;
  private stopped = false;

  constructor(d: ExtraUpdatesDeps) {
    this.d = d;
    this.exec = d.exec ?? defaultExec;
    this.file = path.join(d.home, 'extra-updates.json');
  }

  start(): void {
    if (!this.d.extra.length) return;
    this.stopped = false;
    this.schedule(FIRST_RUN_MS);
  }

  stop(): void {
    this.stopped = true;
    this.timer?.cancel();
    this.timer = undefined;
  }

  async settled(): Promise<void> {
    while (this.work) await this.work;
  }

  private schedule(ms: number): void {
    this.timer?.cancel();
    this.timer = this.d.clock.setTimeout(() => {
      this.timer = undefined;
      if (this.stopped || this.work) return;
      this.work = this.runDue().finally(() => {
        this.work = undefined;
        if (!this.stopped) this.schedule(this.untilNextDue());
      });
    }, ms);
  }

  private state(): StateFile {
    return readJsonSync<StateFile>(this.file) ?? {};
  }

  private untilNextDue(): number {
    const st = this.state();
    const now = this.d.clock.now();
    const next = Math.min(...this.d.extra.map((u) => (st[u.name]?.at ?? 0) + DAY_MS));
    // At least a minute, so a clock that jumps back can't spin this.
    return Math.max(FIRST_RUN_MS, next - now);
  }

  private async runDue(): Promise<void> {
    for (const u of this.d.extra) {
      if (this.stopped) return;
      const last = this.state()[u.name];
      if (last && this.d.clock.now() - last.at < DAY_MS) continue;
      const r = await this.exec(u.command[0], u.command.slice(1), {
        env: { ...(this.d.env ?? process.env), ...u.env },
        timeoutMs: RUN_TIMEOUT_MS,
      });
      const ok = r.code === 0;
      const output = (r.error ?? `${r.stdout}\n${r.stderr}`).trim().slice(-500);
      if (ok) this.d.log.info('cli updates: extra update ran', { name: u.name });
      else this.d.log.warn('cli updates: extra update failed', { name: u.name, code: r.code, output });
      const st = this.state();
      st[u.name] = { at: this.d.clock.now(), ok, code: r.code, ...(output ? { output } : {}) };
      atomicWriteFileSync(this.file, JSON.stringify(st, null, 2) + '\n');
    }
  }
}
