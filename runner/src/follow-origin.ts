// A second machine following GitHub's main (docs/dev-tools.md, "The runner
// updating itself"). `followOrigin: true` in runner.json says this machine's
// main is GitHub's: work lands on the main Mac and reaches here through
// origin, so the runner fetches origin itself, builds origin/main, and moves
// the local main along for the threads that start from it.
//
// Off by default, as whatever origin has gets built and run with no signature
// check, and a stranger's copy may well have Mike's repo as its origin
// (docs/bringing-in-updates.md brings that in only when asked).
//
// Mike, 9 Nov 2026: bruce sat three days behind and the Pixel 96 commits, each
// on a hand-made pull job failing quietly behind one commit a fix-update
// thread had made on that machine's main. So a local commit can no longer hold
// the runner back (it builds origin/main regardless), it's never thrown away,
// and it's said loudly (tb status, the app's Updates panel, the runner row).
import { execFile, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { readJsonSync } from './fsutil.ts';
import { currentLink, readRelease } from './release.ts';

export const ORIGIN_MAIN = 'refs/remotes/origin/main';
const LOCAL_MAIN = 'refs/heads/main';
// A fetch that hangs (a Mac half asleep, a dead network) mustn't hold the
// updater's check, which everything else here waits behind.
const GIT_TIMEOUT_MS = 2 * 60_000;
// Shown in a heartbeat and the app, so kept to a line or two.
const MAX_ERROR = 300;

export type OriginProblem = {
  // fetch: GitHub couldn't be reached. diverged: main here has commits GitHub's
  // doesn't. stuck: main is behind and git wouldn't move it (uncommitted
  // changes in the way, a rebase under way).
  kind: 'fetch' | 'diverged' | 'stuck';
  message: string;
};

// ref: what to build, when there's an origin/main to build (it may be from
// the last good fetch).
export type OriginResult = { ref?: string; problem?: OriginProblem };

export function followOriginOn(home: string): boolean {
  return readJsonSync<{ followOrigin?: unknown }>(path.join(home, 'runner.json'))?.followOrigin === true;
}

// Git's error in a line or two, without a token someone put in a remote's
// URL: this goes in the runner row, and tb status gets pasted around.
export function cleanGitError(text: string): string {
  const oneLine = text.replace(/:\/\/[^@/\s'"]+@/g, '://').split('\n').map((l) => l.trim()).filter(Boolean).join(' ');
  return oneLine.length > MAX_ERROR ? `${oneLine.slice(0, MAX_ERROR - 3)}...` : oneLine;
}

type Run = { ok: boolean; out: string; err: string };

function run(cwd: string, args: string[]): Promise<Run> {
  return new Promise((resolve) => {
    execFile(
      'git',
      ['-C', cwd, ...args],
      {
        encoding: 'utf8',
        maxBuffer: 16 * 1024 * 1024,
        timeout: GIT_TIMEOUT_MS,
        // Nobody's there to answer a prompt for a password or a host key.
        env: { ...process.env, GIT_TERMINAL_PROMPT: '0', GIT_SSH_COMMAND: process.env.GIT_SSH_COMMAND ?? 'ssh -o BatchMode=yes' },
      },
      (err, stdout, stderr) => resolve({ ok: !err, out: String(stdout).trim(), err: String(stderr).trim() || (err ? err.message : '') }),
    );
  });
}

async function rev(cwd: string, ref: string): Promise<string | undefined> {
  const r = await run(cwd, ['rev-parse', '--verify', '-q', `${ref}^{commit}`]);
  return r.ok && r.out ? r.out : undefined;
}

// The worktree with main checked out, if any: moving a branch that's checked
// out without updating its files would make every file look changed.
async function worktreeOnMain(checkout: string): Promise<string | undefined> {
  const r = await run(checkout, ['worktree', 'list', '--porcelain']);
  if (!r.ok) return undefined;
  for (const block of r.out.split('\n\n')) {
    const lines = block.split('\n');
    if (lines.includes(`branch ${LOCAL_MAIN}`)) return lines.find((l) => l.startsWith('worktree '))?.slice('worktree '.length);
  }
  return undefined;
}

// Fetches GitHub's main and brings the checkout's main to it when that's a
// fast-forward. Never rewrites or resets anything: a local commit or a local
// edit is someone's work, and it stays where it is and gets said.
export async function followOrigin(checkout: string): Promise<OriginResult> {
  // Spelled out with the `+`, as a clone's fetch refspec can be narrower and
  // a rewritten main on GitHub shouldn't stop this one for good.
  const fetched = await run(checkout, ['fetch', '-q', 'origin', `+${LOCAL_MAIN}:${ORIGIN_MAIN}`]);
  const tip = await rev(checkout, ORIGIN_MAIN);
  const fetchProblem: OriginProblem | undefined = fetched.ok ? undefined : { kind: 'fetch', message: cleanGitError(fetched.err) || 'git fetch origin failed' };
  if (!tip) return { problem: fetchProblem ?? { kind: 'fetch', message: 'origin has no main to follow' } };
  const out: OriginResult = { ref: ORIGIN_MAIN, ...(fetchProblem ? { problem: fetchProblem } : {}) };
  const local = await rev(checkout, LOCAL_MAIN);
  if (!local || local === tip) return out;
  const fastForward = await run(checkout, ['merge-base', '--is-ancestor', local, tip]);
  if (!fastForward.ok) return { ...out, problem: await divergedProblem(checkout, tip) };
  const wt = await worktreeOnMain(checkout);
  const moved = wt ? await run(wt, ['merge', '-q', '--ff-only', tip]) : await moveUnoccupiedMain(checkout, local, tip);
  if (moved.ok) return out;
  return { ...out, problem: { kind: 'stuck', message: `main in ${wt ?? checkout} couldn't move to GitHub's main, so new threads here start from the old one: ${cleanGitError(moved.err)}` } };
}

// git branch -f to main's own commit changes nothing, but git refuses it for
// a branch being rebased or bisected, which the worktree list doesn't show.
// The move itself is update-ref, which fails if main moved since we looked.
async function moveUnoccupiedMain(checkout: string, local: string, tip: string): Promise<Run> {
  const free = await run(checkout, ['branch', '-f', 'main', local]);
  return free.ok ? run(checkout, ['update-ref', LOCAL_MAIN, tip, local]) : free;
}

async function divergedProblem(checkout: string, tip: string): Promise<OriginProblem> {
  const counted = await run(checkout, ['rev-list', '--count', LOCAL_MAIN, `^${tip}`]);
  const n = Number(counted.out) || 1;
  const newest = await run(checkout, ['log', '-1', '--format=%h %s', LOCAL_MAIN, `^${tip}`]);
  const what = `${n} commit${n === 1 ? '' : 's'} GitHub's main doesn't (${cleanGitError(newest.out)})`;
  return {
    kind: 'diverged',
    message: `main in ${checkout} has ${what}, so new threads here start from it. The runner still updates from GitHub. Keep the work on a branch and put main back: git branch kept-work main && git reset --keep origin/main (on main in that folder).`,
  };
}

// When the oldest commit on `ref` that this release hasn't got first changed
// what a release holds (RELEASE_PATHS): how long the runner has been missing
// something that runs. Undefined when nothing's missing, or `since` isn't in
// this repo so it can't be counted from.
export async function oldestMissingAt(checkout: string, prefix: string, since: string, ref: string): Promise<number | undefined> {
  // Listed here rather than imported so update.ts can import this file.
  const paths = ['src', 'bin', 'package.json', 'package-lock.json'].map((p) => prefix + p);
  const r = await run(checkout, ['log', '--reverse', '--format=%ct', `${since}..${ref}`, '--', ...paths]);
  if (!r.ok || !r.out) return undefined;
  const at = Number(r.out.split('\n')[0]) * 1000;
  return Number.isFinite(at) && at > 0 ? at : undefined;
}

// What makes two folders the same repo: a release can record a linked
// worktree as its checkout (deploy.mjs), and tb land names the main one.
function repoOf(dir: string): string {
  let at = dir;
  const r = spawnSync('git', ['-C', dir, 'rev-parse', '--path-format=absolute', '--git-common-dir'], { encoding: 'utf8' });
  if (r.status === 0 && r.stdout.trim()) at = r.stdout.trim();
  try {
    return fs.realpathSync(at);
  } catch {
    return path.resolve(at);
  }
}

// Why `tb land` won't land in `mainCheckout`, or undefined when it can. tb
// land only moves the local main, and on this machine that main has to stay
// GitHub's, so a landing here would only pull the two apart. That's how
// a07a9f01 got on bruce. Only for the repo the runner follows: another repo's
// main is its own.
export function landRefusal(home: string, mainCheckout: string): string | undefined {
  if (!followOriginOn(home)) return undefined;
  const id = currentLink(home);
  const checkout = id ? readRelease(path.join(home, 'runner', 'releases', id))?.checkout : undefined;
  if (!checkout || repoOf(checkout) !== repoOf(mainCheckout)) return undefined;
  return "This machine follows GitHub's main (followOrigin in runner.json), so nothing lands here: it would only leave its main apart from GitHub's. Give the work to the main Mac to land instead: create_thread with runner: \"main\". If this machine can push a branch of its own (git push origin HEAD:<branch>, with a name the repo's rules let this machine push), name that branch in the brief; if not, put the whole diff in it.";
}
