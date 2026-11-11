// Which thread a process on this Mac belongs to, for the RPC server (docs/
// protocol.md, "Who's really calling"). Credentials alone can't say: anything
// a thread's shell runs can read the token file or wipe its own key from the
// environment. The kernel can say what it's a child of, so the server looks at
// the far end of the connection and walks up its parents to a thread's CLI.
import { execFile } from 'node:child_process';
import { lsofPath } from './handover.ts';
import { psTable, type ProcessRegistry } from './proc.ts';
import type { Logger } from './log.ts';

export type PeerDeps = {
  registry: Pick<ProcessRegistry, 'threadOfPid'> & Partial<Pick<ProcessRegistry, 'threadOfGroup'>>;
  // A thread this runner or a draining one really has (a registry entry can
  // be a job's label, like models:<account>).
  known: (threadId: string) => boolean;
  log: Logger;
  // Tests pass stand-ins.
  lsof?: (port: number) => Promise<number[]>;
  table?: () => Promise<{ pid: number; ppid: number; pgid?: number }[]>;
};

const LOOKUP_MS = 3000;

// A slow ps mustn't hold a connection's frames back for long (rpc.ts queues them).
function deadline<T>(p: Promise<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error('process lookup timed out')), LOOKUP_MS);
    p.then((v) => { clearTimeout(t); resolve(v); }, (e) => { clearTimeout(t); reject(e); });
  });
}

// The processes with an established TCP connection using this port.
function lsofPids(port: number): Promise<number[]> {
  return new Promise((resolve, reject) => {
    execFile(lsofPath(), ['-nP', `-iTCP:${port}`, '-sTCP:ESTABLISHED', '-Fp'], { timeout: LOOKUP_MS }, (err, stdout) => {
      // lsof exits 1 when nothing matched; what it printed still counts.
      if (err && (err as { code?: unknown }).code !== 1 && !stdout) return reject(err);
      resolve([...stdout.matchAll(/^p(\d+)$/gm)].map((m) => Number(m[1])));
    });
  });
}

// The thread the connection from this client port belongs to, or undefined
// when it isn't under a thread's CLI or the lookup can't tell (no lsof, on
// a machine without it): then the credential presented decides.
export async function threadOfPort(port: number, deps: PeerDeps): Promise<string | undefined> {
  try {
    const pids = await (deps.lsof ?? lsofPids)(port);
    const peer = pids.find((p) => p !== process.pid);
    if (peer === undefined) return undefined;
    const table = await deadline((deps.table ?? psTable)());
    const parents = new Map(table.map((r) => [r.pid, r.ppid]));
    const seen = new Set<number>();
    for (let p: number | undefined = peer; p !== undefined && p > 1 && !seen.has(p); p = parents.get(p)) {
      seen.add(p);
      const t = deps.registry.threadOfPid(p);
      if (t && deps.known(t)) return t;
    }
    // Left its parents behind (a nohup or a backgrounded subshell), but still
    // in a process group a thread's tool command ran in.
    const group = table.find((r) => r.pid === peer)?.pgid;
    const byGroup = group ? deps.registry.threadOfGroup?.(group) : undefined;
    if (byGroup && deps.known(byGroup)) return byGroup;
    return undefined;
  } catch (e) {
    deps.log.warn('could not tell which process connected to the RPC', { error: String(e) });
    return undefined;
  }
}
