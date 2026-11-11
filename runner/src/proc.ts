import fs from 'node:fs';
import path from 'node:path';
import { spawn, execFile, execFileSync } from 'node:child_process';
import type { SpawnOptions, SpawnedProcess } from '@anthropic-ai/claude-agent-sdk';
import type { Logger } from './log.ts';
import { atomicWriteFileSync, readJsonSync } from './fsutil.ts';

// A process group, with its leader's start time so a recycled pid is never
// mistaken for it. lstart is '' when the leader had already exited when the
// group was noted.
type Group = { pgid: number; lstart: string };
// threadId: the thread (or a job's label) the process was started for, which
// the RPC server asks about (peer.ts).
type Entry = { pid: number; command: string; startedAt: string; lstart?: string; groups?: Group[]; threadId?: string };

const SCAN_MS = 5000;

// Every CLI the runner starts gets its own process group (detached), so the
// CLI and anything it spawned in that group (MCP servers) can be stopped
// together. Tool commands are different: the CLI runs each Bash command in
// a process group of its own, so while CLIs are live the runner also notes
// which groups their descendants are in. Everything is recorded in
// run/children.json. If a CLI dies without cleaning up, or the runner dies,
// those groups are stopped too, and only those.
export class ProcessRegistry {
  private file: string;
  private log: Logger;
  private entries = new Map<number, Entry>();
  private scanTimer?: NodeJS.Timeout;
  private scanning = false;

  // `name` is children.json for the runner that owns the data dir. A
  // draining runner (docs/protocol.md, "Handovers") moves its list to
  // children-<pid>.json, so the new runner's sweep leaves its CLIs alone,
  // and sweeps that file only if it dies.
  constructor(runDir: string, log: Logger, name = 'children.json') {
    this.file = path.join(runDir, name);
    this.log = log;
  }

  // Moves the list to another file in the same folder.
  moveTo(name: string): void {
    const old = this.file;
    this.file = path.join(path.dirname(old), name);
    this.save();
    try {
      fs.rmSync(old, { force: true });
    } catch {
      // the new runner's sweep finds an old copy empty of anything alive
    }
  }

  // Removes the list (a draining runner that has finished).
  removeFile(): void {
    try {
      fs.rmSync(this.file, { force: true });
    } catch {
      // gone
    }
  }

  private save(): void {
    try {
      atomicWriteFileSync(this.file, JSON.stringify([...this.entries.values()], null, 1));
    } catch (e) {
      this.log.warn('could not save children.json', { error: String(e) });
    }
  }

  add(pid: number, command: string, threadId?: string): void {
    const lstart = processInfo(pid)?.lstart;
    this.entries.set(pid, { pid, command, startedAt: new Date().toISOString(), ...(lstart ? { lstart } : {}), groups: [], ...(threadId ? { threadId } : {}) });
    this.save();
    this.scanTimer ??= setInterval(() => void this.scan(), SCAN_MS).unref();
  }

  remove(pid: number): void {
    if (this.entries.delete(pid)) this.save();
    if (this.entries.size === 0 && this.scanTimer) {
      clearInterval(this.scanTimer);
      this.scanTimer = undefined;
    }
  }

  pids(): number[] {
    return [...this.entries.keys()];
  }

  // The thread whose CLI ran a tool command in this process group (noted by
  // scan), or whose CLI leads it. For a process that's lost its parents.
  threadOfGroup(pgid: number): string | undefined {
    for (const e of this.entries.values()) {
      if (e.threadId && (e.pid === pgid || e.groups?.some((g) => g.pgid === pgid))) return e.threadId;
    }
    return undefined;
  }

  // The thread a live CLI was started for: this runner's own (gone from the
  // map when it exits), or one a draining runner still has (its list is kept
  // beside this one, where a recycled pid wouldn't have the same start time).
  threadOfPid(pid: number): string | undefined {
    const own = this.entries.get(pid);
    if (own) return own.threadId;
    let names: string[];
    try {
      names = fs.readdirSync(path.dirname(this.file)).filter((n) => /^children-\d+\.json$/.test(n));
    } catch {
      return undefined;
    }
    for (const n of names) {
      const entries = readJsonSync<Entry[]>(path.join(path.dirname(this.file), n));
      const e = Array.isArray(entries) ? entries.find((x) => x?.pid === pid) : undefined;
      if (e?.threadId && (!e.lstart || processInfo(pid)?.lstart === e.lstart)) return e.threadId;
    }
    return undefined;
  }

  // Groups the CLI's tool commands ran in that are still running.
  toolGroups(pid: number): number[] {
    return (this.entries.get(pid)?.groups ?? []).filter((g) => sameGroup(g)).map((g) => g.pgid);
  }

  // Notes the process groups of every live CLI's descendants.
  async scan(): Promise<void> {
    if (this.scanning || this.entries.size === 0) return;
    this.scanning = true;
    try {
      const table = await psTable();
      const kids = new Map<number, number[]>();
      const pgidOf = new Map<number, number>();
      for (const r of table) {
        pgidOf.set(r.pid, r.pgid);
        const list = kids.get(r.ppid) ?? [];
        list.push(r.pid);
        kids.set(r.ppid, list);
      }
      const found = new Map<Entry, Set<number>>();
      for (const e of this.entries.values()) {
        const known = new Set((e.groups ?? []).map((g) => g.pgid));
        const fresh = new Set<number>();
        const stack = [...(kids.get(e.pid) ?? [])];
        const seen = new Set<number>();
        while (stack.length) {
          const p = stack.pop()!;
          if (seen.has(p)) continue;
          seen.add(p);
          const pg = pgidOf.get(p);
          if (pg && pg !== e.pid && pg > 1 && !known.has(pg)) fresh.add(pg);
          stack.push(...(kids.get(p) ?? []));
        }
        if (fresh.size) found.set(e, fresh);
      }
      if (found.size === 0) return;
      for (const [e, pgids] of found) {
        if (!this.entries.has(e.pid)) continue; // exited while ps ran
        for (const pgid of pgids) {
          e.groups ??= [];
          e.groups.push({ pgid, lstart: processInfo(pgid)?.lstart ?? '' });
        }
      }
      this.save();
    } catch (e) {
      this.log.debug('process scan failed', { error: String(e) });
    } finally {
      this.scanning = false;
    }
  }

  // Stops groups recorded by a previous runner that are still alive and
  // waits until they're gone, so a resumed session never runs beside the
  // CLI it replaces. A group is only touched if its leader is the process
  // that was recorded (same start time), or the leader has exited and the
  // group lives on (a pid isn't reused while its group exists).
  async sweepOrphans(graceMs = 3000): Promise<number> {
    const previous = readJsonSync<Entry[]>(this.file) ?? [];
    const targets: number[] = [];
    for (const e of previous) {
      if (!e || typeof e.pid !== 'number') continue;
      if (sameGroup({ pgid: e.pid, lstart: e.lstart ?? '' }, e.lstart ? undefined : path.basename(e.command ?? ''))) targets.push(e.pid);
      for (const g of e.groups ?? []) if (sameGroup(g)) targets.push(g.pgid);
    }
    for (const pgid of targets) {
      this.log.warn('stopping a process group left over from a previous runner', { pgid });
      killGroup(pgid, 'SIGTERM');
    }
    await waitGone(targets, graceMs);
    const stubborn = targets.filter((p) => groupAlive(p));
    for (const pgid of stubborn) killGroup(pgid, 'SIGKILL');
    await waitGone(stubborn, 2000);
    this.entries.clear();
    this.save();
    return targets.length;
  }

  // SIGTERM every live group, wait up to graceMs, then SIGKILL stragglers.
  async killAll(graceMs = 3000): Promise<void> {
    const pids = this.pids();
    const groups = pids.flatMap((pid) => [pid, ...this.toolGroups(pid)]);
    for (const pgid of groups) killGroup(pgid, 'SIGTERM');
    await waitGone(groups, graceMs);
    for (const pgid of groups) if (groupAlive(pgid)) killGroup(pgid, 'SIGKILL');
    this.entries.clear();
    this.save();
  }
}

async function waitGone(pgids: number[], ms: number): Promise<void> {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline && pgids.some((p) => groupAlive(p))) {
    await new Promise((r) => setTimeout(r, 100));
  }
}

export function killGroup(pgid: number, signal: NodeJS.Signals): boolean {
  try {
    process.kill(-pgid, signal);
    return true;
  } catch {
    return false;
  }
}

export function groupAlive(pgid: number): boolean {
  try {
    process.kill(-pgid, 0);
    return true;
  } catch {
    return false;
  }
}

// True if the group is alive and still the one that was recorded.
function sameGroup(g: Group, commandHint?: string): boolean {
  if (!g || typeof g.pgid !== 'number' || g.pgid <= 1 || !groupAlive(g.pgid)) return false;
  const leader = processInfo(g.pgid);
  if (!leader) return true; // leader gone; the group's members are the original ones
  if (leader.pgid !== g.pgid) return false;
  if (g.lstart) return leader.lstart === g.lstart;
  // Old records with no start time fall back to the command name.
  return Boolean(commandHint) && leader.command.includes(commandHint!);
}

const C_LOCALE = { ...process.env, LC_ALL: 'C' };

export function processInfo(pid: number): { pgid: number; lstart: string; command: string } | null {
  try {
    const out = execFileSync('/bin/ps', ['-o', 'pgid=,lstart=,command=', '-p', String(pid)], { encoding: 'utf8', env: C_LOCALE }).trim();
    // lstart is five fields in the C locale: "Mon Oct 26 15:04:05 2026".
    const m = /^(\d+)\s+(\S+\s+\S+\s+\d+\s+[\d:]+\s+\d+)\s+(.*)$/.exec(out);
    return m ? { pgid: Number(m[1]), lstart: m[2].replace(/\s+/g, ' '), command: m[3] } : null;
  } catch {
    return null;
  }
}

export function psTable(): Promise<{ pid: number; ppid: number; pgid: number }[]> {
  return new Promise((resolve, reject) => {
    execFile('/bin/ps', ['-axo', 'pid=,ppid=,pgid='], { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }, (err, stdout) => {
      if (err) return reject(err);
      const rows: { pid: number; ppid: number; pgid: number }[] = [];
      for (const line of stdout.split('\n')) {
        const m = /^\s*(\d+)\s+(\d+)\s+(\d+)/.exec(line);
        if (m) rows.push({ pid: Number(m[1]), ppid: Number(m[2]), pgid: Number(m[3]) });
      }
      resolve(rows);
    });
  });
}

// spawnClaudeCodeProcess implementation for the SDK. threadId goes in the
// spawn and exit log lines, so `tb logs --thread` shows a CLI crashing.
export function groupSpawner(
  registry: ProcessRegistry,
  log: Logger,
  onStderr: (line: string) => void,
  threadId?: string,
): (opts: SpawnOptions) => SpawnedProcess {
  return (opts) => {
    const child = spawn(opts.command, opts.args, {
      cwd: opts.cwd,
      env: opts.env as NodeJS.ProcessEnv,
      stdio: ['pipe', 'pipe', 'pipe'],
      detached: true,
    });
    const pid = child.pid;
    if (pid) registry.add(pid, opts.command, threadId);
    log.info('spawned CLI', { threadId, pid, cwd: opts.cwd });
    let buf = '';
    child.stderr?.setEncoding('utf8');
    child.stderr?.on('data', (d: string) => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        onStderr(buf.slice(0, i));
        buf = buf.slice(i + 1);
      }
    });
    // The SDK fires this after closing stdin and a short grace period.
    opts.signal.addEventListener(
      'abort',
      () => {
        if (!pid || child.exitCode !== null) return;
        killGroup(pid, 'SIGTERM');
        setTimeout(() => {
          if (groupAlive(pid)) killGroup(pid, 'SIGKILL');
        }, 3000).unref();
      },
      { once: true },
    );
    child.once('exit', (code, signal) => {
      log.info('CLI exited', { threadId, pid, code, signal });
      if (!pid) return;
      // Stop anything the CLI left running in its group.
      killGroup(pid, 'SIGTERM');
      // A CLI that ended normally has stopped its own tool commands. One
      // that crashed or was killed hasn't, and they'd run on for ever.
      if (code !== 0) {
        const left = registry.toolGroups(pid);
        if (left.length) {
          log.warn('stopping tool commands a CLI left behind', { threadId, pid, groups: left });
          for (const g of left) killGroup(g, 'SIGTERM');
          setTimeout(() => {
            for (const g of left) if (groupAlive(g)) killGroup(g, 'SIGKILL');
          }, 3000).unref();
        }
      }
      registry.remove(pid);
    });
    return child as unknown as SpawnedProcess;
  };
}
