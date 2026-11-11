// Which code this runner is, and the release bookkeeping in
// <home>/runner/state.json that launch.mjs and deploy-runner.sh share with
// it (docs/dev-tools.md). The LaunchAgent runs a snapshot under
// <home>/runner/releases/<id>/ with a release.json beside src/; a runner
// started from a checkout has none and reports itself as "+source".
import fs from 'node:fs';
import path from 'node:path';
import { atomicWriteFileSync, readJsonSync } from './fsutil.ts';
import type { Logger } from './log.ts';

// branch is the branch HEAD was on when it was built (older releases have
// none), so a release from a feature branch is easy to spot. checkout is
// the repo it was built from and prefix the runner's folder in it
// ("runner/"); tree is what the release holds of that folder (update.ts
// compares it with main's). Releases built before these were recorded have
// none, and a dirty build has no tree.
export type ReleaseJson = { id: string; commit: string; dirty: boolean; builtAt: string; node: string; branch?: string; checkout?: string; prefix?: string; tree?: string };

export type RunnerState = {
  current: string;
  previous: string | null;
  lastGood: string | null;
  // launch.mjs's record of each start; clean once that start listened and
  // then exited 0, which never counts towards a crash loop.
  boots: { release: string; at: string; pid?: number; clean?: boolean }[];
  // deploy-runner.sh's: when current was last activated. launch.mjs only
  // counts that release's boots since then.
  activated?: { release: string; at: string };
  // reportedAt is the runner's own: set once the notice has gone out, so a
  // rollback is announced by the first runner after it and no other.
  rolledBack?: { from: string; to: string; at: string; reportedAt?: string };
};

// The folder holding src/, package.json and (in a release) release.json.
const CODE_ROOT = path.resolve(import.meta.dirname, '..');

export function readRelease(root = CODE_ROOT): ReleaseJson | undefined {
  const r = readJsonSync<ReleaseJson>(path.join(root, 'release.json'));
  return r && typeof r.id === 'string' && r.id ? r : undefined;
}

// The Threadbear checkout this runner's code came from: the release's
// record of it, else the checkout it's running from. Its own patches are
// always trusted (patch-trust.ts).
export function ownCheckout(root = CODE_ROOT): string {
  return readRelease(root)?.checkout ?? path.dirname(root);
}

export function runnerVersion(root = CODE_ROOT): { version: string; release?: string; branch?: string } {
  const pkg = readJsonSync<{ version?: string }>(path.join(root, 'package.json'));
  const base = typeof pkg?.version === 'string' ? pkg.version : '0.0.0';
  const r = readRelease(root);
  if (!r) return { version: `${base}+source` };
  return typeof r.branch === 'string' && r.branch ? { version: `${base}+${r.id}`, release: r.id, branch: r.branch } : { version: `${base}+${r.id}`, release: r.id };
}

export const RELEASE = runnerVersion();

export function statePath(home: string): string {
  return path.join(home, 'runner', 'state.json');
}

export function readState(home: string): RunnerState | undefined {
  const s = readJsonSync<RunnerState>(statePath(home));
  return s && typeof s === 'object' && !Array.isArray(s) ? s : undefined;
}

// Read, change and write back in one go, keeping fields this runner doesn't
// know about. A missing or unreadable state.json is left alone: that's a
// runner run from source, or a sandbox home with no releases.
function updateState(home: string, change: (s: RunnerState) => boolean): boolean {
  const s = readState(home);
  if (!s) return false;
  if (!change(s)) return false;
  atomicWriteFileSync(statePath(home), JSON.stringify(s, null, 2) + '\n');
  return true;
}

// The release <home>/runner/current names, if there's a link.
export function currentLink(home: string): string | undefined {
  try {
    return path.basename(fs.readlinkSync(path.join(home, 'runner', 'current')));
  } catch {
    return undefined;
  }
}

export function releaseExists(home: string, id: string): boolean {
  return fs.existsSync(path.join(home, 'runner', 'releases', id, 'src', 'main.ts'));
}

// Points current at a release the way launch.mjs and deploy-runner.sh do
// (a new link renamed over the old one, so it always names a whole
// release), and records it in state.json. A cancelled deploy restart uses
// it to put the running release back.
export function pointCurrentAt(home: string, id: string): void {
  const link = path.join(home, 'runner', 'current');
  const tmp = `${link}.${process.pid}.tmp`;
  fs.rmSync(tmp, { force: true });
  fs.symlinkSync(path.join('releases', id), tmp);
  fs.renameSync(tmp, link);
  updateState(home, (s) => {
    s.current = id;
    return true;
  });
}

// Tells launch.mjs this release starts and stays up, so it is never rolled
// back from and is the one to go back to.
export function markGood(home: string, releaseId: string | undefined, log: Logger): boolean {
  if (!releaseId) return false;
  try {
    const changed = updateState(home, (s) => {
      if (s.lastGood === releaseId) return false;
      s.lastGood = releaseId;
      return true;
    });
    if (changed) log.info('marked this release good', { release: releaseId });
    return changed;
  } catch (e) {
    log.warn('could not mark this release good', { release: releaseId, error: String(e) });
    return false;
  }
}

// The message for a rollback launch.mjs did and nobody has reported yet,
// and the release it went back from. Marks it reported, so the next runner
// doesn't say it again.
export function takeRollbackNotice(home: string, log: Logger, now = Date.now()): { message: string; from: string } | undefined {
  let out: { message: string; from: string } | undefined;
  try {
    updateState(home, (s) => {
      const rb = s.rolledBack;
      if (!rb || typeof rb !== 'object' || rb.reportedAt) return false;
      out = { message: `Release ${rb.from} kept crashing at startup, so the runner went back to ${rb.to}.`, from: String(rb.from) };
      rb.reportedAt = new Date(now).toISOString();
      return true;
    });
  } catch (e) {
    // Better to say it twice than not at all.
    log.warn('could not mark the rollback reported', { error: String(e) });
  }
  if (out) log.warn('running an earlier release after a rollback', { message: out.message });
  return out;
}

// A release counts as good once it has been listening this long: launch.mjs
// rolls back one that boots 3 times in 2 minutes without getting here.
// THREADBEAR_GOOD_AFTER_MS shortens it for tests.
export const GOOD_AFTER_MS = 60_000;

// Returns a cancel for main.ts's shutdown: a runner that is stopping (one
// whose shutdown hangs, say) hasn't shown it stays up. then runs once it
// has, release or not.
export function markGoodLater(home: string, releaseId: string | undefined, log: Logger, env: NodeJS.ProcessEnv = process.env, then?: () => void): () => void {
  const fromEnv = env.THREADBEAR_GOOD_AFTER_MS ? Number(env.THREADBEAR_GOOD_AFTER_MS) : NaN;
  const ms = Number.isFinite(fromEnv) && fromEnv >= 0 ? fromEnv : GOOD_AFTER_MS;
  const timer = setTimeout(() => {
    markGood(home, releaseId, log);
    then?.();
  }, ms);
  timer.unref();
  return () => clearTimeout(timer);
}

// A runner that won't start because runner.json is broken (config.ts,
// RunnerJsonError) isn't a broken release. Left as an ordinary failed boot,
// three of them make launch.mjs go back to the last good release, and one
// from before 9 Nov reads a broken runner.json as {} and writes over it,
// the loss the refusal is there to stop. So this boot is marked the way
// launch.mjs marks a clean stop, which planBoot doesn't count. The file is
// launch.mjs's to write, so only this process's own boot is touched, and
// only once launch.mjs has written it (it does just after the spawn).
export async function markConfigStop(home: string, pid = process.pid, waitMs = 3000, sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))): Promise<boolean> {
  const mark = (): 'marked' | 'already' | 'none' => {
    let out: 'marked' | 'already' | 'none' = 'none';
    try {
      updateState(home, (s) => {
        const boot = Array.isArray(s.boots) ? s.boots.findLast((b) => b && b.pid === pid) : undefined;
        if (!boot) return false;
        if (boot.clean) {
          out = 'already';
          return false;
        }
        boot.clean = true;
        out = 'marked';
        return true;
      });
    } catch {
      // state.json unreadable or unwritable: nothing to be done from here
    }
    return out;
  };
  let found = false;
  for (let waited = 0; ; waited += 100) {
    if (mark() !== 'none') {
      found = true;
      break;
    }
    if (waited >= waitMs) return false;
    await sleep(100);
  }
  // launch.mjs may be writing state.json at the same moment (marking a
  // draining runner's exit clean during a handover) from a copy it read
  // before this mark, and would put the boot back as failed. Look again a
  // little later and mark it again if so.
  for (let i = 0; i < 3; i++) {
    await sleep(waitMs ? 200 : 0);
    mark();
  }
  return found;
}

// Whether launch.mjs started this runner as a child it supervises (a
// LaunchAgent installed since handovers): THREADBEAR_SUPERVISED and an IPC
// channel to it. Then a handover is possible.
export function supervised(): boolean {
  return process.env.THREADBEAR_SUPERVISED === '1' && typeof process.send === 'function';
}

function sendToSupervisor(msg: unknown): boolean {
  try {
    if (!supervised() || !process.connected) return false;
    process.send!(msg);
    return true;
  } catch {
    return false;
  }
}

// A release that hasn't said it is listening within its time limit is
// stopped by launch.mjs, so one that hangs at startup is rolled back like
// one that crashes. Under the supervisor that's an IPC message; under an
// older launch.mjs (which ran the runner in its own process) a hook it left
// on globalThis. Runners started some other way have neither.
export function tellLaunchListening(): void {
  if (sendToSupervisor({ type: 'listening' })) return;
  const hook = (globalThis as { threadbearLaunch?: { listening?: () => void } }).threadbearLaunch;
  try {
    hook?.listening?.();
  } catch {
    // launch.mjs's bookkeeping must never stop the runner
  }
}

export type HandoverReply = { ok: true; pid: number } | { ok: false; error: string };

// Asks the supervisor to start the current release beside this runner,
// which then only finishes the turns it has. ok:false (not supervised, an
// older launch.mjs, refused, no answer in time) means carry on as before
// with a plain restart.
export function requestHandover(timeoutMs = 10_000): Promise<HandoverReply> {
  if (!supervised()) return Promise.resolve({ ok: false, error: 'not started by a launch.mjs that supervises' });
  return new Promise((resolve) => {
    let done = false;
    const finish = (r: HandoverReply) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      process.off('message', onMessage);
      process.off('disconnect', onGone);
      resolve(r);
    };
    const onMessage = (m: unknown) => {
      const r = m as { type?: unknown; ok?: unknown; pid?: unknown; error?: unknown } | null;
      if (!r || typeof r !== 'object' || r.type !== 'handover') return;
      if (r.ok === true && typeof r.pid === 'number') finish({ ok: true, pid: r.pid });
      else finish({ ok: false, error: typeof r.error === 'string' ? r.error : 'the supervisor refused' });
    };
    const onGone = () => finish({ ok: false, error: 'the supervisor went away' });
    const timer = setTimeout(() => finish({ ok: false, error: `no answer from the supervisor in ${Math.round(timeoutMs / 1000)} s` }), timeoutMs);
    timer.unref();
    process.on('message', onMessage);
    process.on('disconnect', onGone);
    if (!sendToSupervisor({ type: 'handover' })) finish({ ok: false, error: "couldn't reach the supervisor" });
  });
}

// Calls fn once if the supervisor's IPC channel closes (it died), so the
// runner can stop rather than run on with nothing to restart it. Returns
// an unsubscribe. Does nothing when not supervised.
export function onSupervisorGone(fn: () => void): () => void {
  if (!supervised()) return () => {};
  let called = false;
  const handler = () => {
    if (called) return;
    called = true;
    fn();
  };
  process.on('disconnect', handler);
  // A message listener keeps the channel referenced; the runner's server
  // keeps the process alive anyway, so let the channel not hold it.
  (process.channel as { unref?: () => void } | undefined)?.unref?.();
  return () => process.off('disconnect', handler);
}
