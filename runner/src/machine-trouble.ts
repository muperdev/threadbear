// Whether this machine is short of something a release build needs, asked
// after a build failed. A failure in the code stays until main changes
// (update.ts), which is wrong for one the machine caused: on 9 Nov Hetzner's
// runner ran out of file watches, two tests failed in every build, and when
// the watches came back it sat 13 hours behind main because nothing under
// runner/ had changed. The tests' output never mentioned watches, only
// assertions, so this asks the machine rather than reading the log.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';

const MIN_FREE_BYTES = 2 ** 30;
const OUT_OF_WATCHES = new Set(['EMFILE', 'ENFILE', 'ENOSPC']);
const OUT_OF_PROCESSES = new Set(['EAGAIN', 'ENOMEM', 'EMFILE', 'ENFILE']);

export type MachineProbes = {
  // Throws what fs.watch would.
  watch: (dir: string) => void;
  // Undefined when it can't be told.
  freeBytes: (dir: string) => number | undefined;
  // The error code from starting a program, undefined when it started.
  run: () => string | undefined;
};

// In a process of its own: Node shares one inotify instance per event loop,
// so a watch in the runner, which already holds one, succeeds with the
// user's instances all gone. A build's tests are new processes and can't.
const WATCH_SCRIPT = "try { require('fs').watch(process.argv[1]).close() } catch (e) { console.log(e.code) }";
export function watchInNewProcess(dir: string, run: typeof spawnSync = spawnSync): void {
  const r = run(process.execPath, ['-e', WATCH_SCRIPT, dir], { encoding: 'utf8', timeout: 10_000 });
  const code = String(r.stdout ?? '').trim();
  // Not starting at all is for the run probe to say.
  if (code) throw Object.assign(new Error(`fs.watch: ${code}`), { code });
}

const realProbes: MachineProbes = {
  watch: (dir) => watchInNewProcess(dir),
  freeBytes: (dir) => {
    try {
      const s = fs.statfsSync(dir);
      return s.bavail * s.bsize;
    } catch {
      return undefined;
    }
  },
  run: () => (spawnSync(process.execPath, ['--version'], { timeout: 10_000 }).error as NodeJS.ErrnoException | undefined)?.code,
};

// What's wrong, worded to follow "this machine", or undefined. Only what's
// really out counts: anything else a probe says is no evidence either way,
// and a failure not blamed on the machine is treated as the code's.
export function machineTrouble(home: string, probes: MachineProbes = realProbes): string | undefined {
  try {
    probes.watch(os.tmpdir());
  } catch (e) {
    const code = (e as NodeJS.ErrnoException).code;
    if (code && OUT_OF_WATCHES.has(code)) return `has no file watches left (${code})`;
  }
  for (const dir of [home, os.tmpdir()]) {
    const free = probes.freeBytes(dir);
    if (free !== undefined && free < MIN_FREE_BYTES) return `has under 1 GB free on ${dir}`;
  }
  const code = probes.run();
  if (code && OUT_OF_PROCESSES.has(code)) return `can't start new processes (${code})`;
  return undefined;
}
