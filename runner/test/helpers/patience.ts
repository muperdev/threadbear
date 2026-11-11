// How long a test waits for something that should happen.
//
// Agents' builds and tests run at background priority (taskpolicy -b, Mike's
// rule for the M5), which keeps them on the efficiency cores with every other
// thread's check. There a test's node child can take 20 s just to start, so
// limits of 3 to 20 s failed tests that were fine, a different handful each
// run, and `tb land` refused for no real reason (2 Nov).
//
// Every wait here ends the moment its condition holds, so a long limit costs
// nothing when a test passes, only time on one that's really broken. Waits
// that expect nothing to happen, like f.next(800), keep their short limits.
import fs from 'node:fs';
import os from 'node:os';
import type { TestContext } from 'node:test';

export const PATIENCE_MS = 60_000;

// For a test of a real file watch. On Linux a user's inotify instances run
// out (128 by default) and fs.watch throws, which the code takes as "no
// watch" and relies on its timed look. The test can't tell anything then, so
// it skips and says why, rather than failing a deploy (9 Nov).
export function noFileWatches(t: TestContext): boolean {
  try {
    fs.watch(os.tmpdir()).close();
    return false;
  } catch (e) {
    const code = (e as NodeJS.ErrnoException).code;
    if (code !== 'EMFILE' && code !== 'ENOSPC') throw e;
    t.skip(`this machine has no file watches left (${code})`);
    return true;
  }
}

// Waits until cond holds or PATIENCE_MS is up. It doesn't throw: the assert
// after it says what went wrong better than "timed out" would.
export async function patiently(cond: () => boolean, pollMs = 20): Promise<void> {
  const end = Date.now() + PATIENCE_MS;
  while (!cond() && Date.now() < end) await new Promise((r) => setTimeout(r, pollMs));
}
