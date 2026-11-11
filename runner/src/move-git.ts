// A moving thread's git work (docs/protocol.md, "Moving a thread to another
// runner", Git). On the runner it leaves, the branch's commits that
// `origin` doesn't have, and one more commit of every uncommitted and
// untracked file, go in a git bundle. On the runner it goes to, the bundle
// is fetched into the thread's new worktree, the branch set to where it
// was, and that last commit's changes put back as uncommitted, so the
// agent finds the worktree as it left it. `.gitignore`d files stay behind.
import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { untrustedGit } from './safe-git.ts';

export type GitMeta = {
  branch: string; // as it was, "HEAD" when detached
  head: string;
  // The commit holding the uncommitted and untracked changes, on `head`.
  wip?: string;
  // Where the bundle starts: what S's origin/<default> has. Absent: the
  // whole history is in it (no origin).
  base?: string;
  origin?: string;
  subdir: string; // the thread's folder inside the repo, '' at its top
};

const HEAD_REF = 'refs/threadbear/move/head';
const WIP_REF = 'refs/threadbear/move/wip';
const MOVE_REFS = 'refs/threadbear/move/';

type Run = { ok: boolean; stdout: string; stderr: string };

// git in `cwd` that the repo's own config can't make run anything: no
// filters, hooks, fsmonitor, signing or diff programs, no fetching but
// where a step fetches on purpose (safe-git.ts untrustedGit). The repo a
// thread works in may have come from an archive with its own .git/config,
// and a move's preview reads it before anything is trusted.
function run(cwd: string, args: string[], env: Record<string, string> = {}, network?: 'bundle' | 'remote'): Promise<Run> {
  return untrustedGit(cwd, args, { env, ...(network ? { network } : {}) });
}

async function out(cwd: string, args: string[], env?: Record<string, string>): Promise<string | undefined> {
  const r = await run(cwd, args, env);
  return r.ok ? r.stdout.trim() : undefined;
}

function why(r: Run): string {
  return (
    r.stderr
      .split('\n')
      .map((l) => l.replace(/^(fatal|error): /, '').trim())
      .find(Boolean) ?? 'git failed'
  );
}

// Commits Threadbear makes for a move are its own, whatever git's config says.
const AUTHOR = {
  GIT_AUTHOR_NAME: 'Threadbear',
  GIT_AUTHOR_EMAIL: 'move@threadbear.invalid',
  GIT_COMMITTER_NAME: 'Threadbear',
  GIT_COMMITTER_EMAIL: 'move@threadbear.invalid',
};

export async function repoTop(cwd: string): Promise<string | undefined> {
  return out(cwd, ['rev-parse', '--show-toplevel']);
}

export async function originUrl(cwd: string): Promise<string | undefined> {
  return (await out(cwd, ['remote', 'get-url', 'origin'])) || undefined;
}

/**
 * `url` with nothing in it that could be a password or token: an http(s)
 * address loses its whole user part (GitHub takes a token as the user
 * alone), ssh:// its password, and the query and fragment go. What a move
 * says about a thread's repo goes to the other runner and through the
 * backend, which only need to know which repo it is (security review,
 * 10 Nov 2026: `https://user:github_pat_...@github.com/...` went whole to
 * the runner a preview asked). The runner it goes to clones with its own
 * credentials.
 */
export function withoutCredentials(url: string): string {
  const m = /^([a-z][a-z0-9+.-]*:\/\/)([^/?#]*@)?([^?#]*)(?:[?#].*)?$/i.exec(url.trim());
  if (!m) return url.trim();
  const [, scheme, userinfo, rest] = m;
  if (!userinfo) return `${scheme}${rest}`;
  if (/^(https?|ftps?):\/\//i.test(scheme!)) return `${scheme}${rest}`;
  const user = userinfo.slice(0, -1).split(':')[0];
  return `${scheme}${user ? `${user}@` : ''}${rest}`;
}

// A repo T may clone for a moving thread: the URL is the old runner's
// word, and that runner may be one of the owner's extra Macs, so only an
// https or ssh address (as GitHub gives them) and never one git would read
// as an option.
export function cloneableUrl(url: string): boolean {
  if (!url || url.length > 2000 || /[\s\0\\]/.test(url) || url.startsWith('-') || url.includes('::')) return false;
  // scheme://[user@]host[:port]/path, each part plain, none starting with -.
  const full = /^(https|ssh):\/\/(?:([A-Za-z0-9._~%+-]+)@)?([A-Za-z0-9._-]+)(?::(\d{1,5}))?\/[^/].*$/i.exec(url);
  if (full) return !full[2]?.startsWith('-') && !full[3]!.startsWith('-') && !full[3]!.startsWith('.');
  // user@host:path, as GitHub gives ssh ones.
  const scp = /^([A-Za-z0-9._~+][A-Za-z0-9._~+-]*)@([A-Za-z0-9][A-Za-z0-9._-]*):([^:-][^:]*)$/.exec(url);
  return !!scp;
}

/** Clones `url` to `to` (a folder that isn't there yet), as untrustedGit does. */
export async function cloneRepo(url: string, to: string): Promise<{ ok: true } | { ok: false; reason: string }> {
  if (!cloneableUrl(url)) return { ok: false, reason: `Threadbear only clones a moving thread's repo from an https or ssh address, not ${JSON.stringify(url.slice(0, 200))}.` };
  const r = await untrustedGit(path.dirname(to), ['clone', '--quiet', '--no-recurse-submodules', '--', url, to], { network: 'remote' });
  return r.ok ? { ok: true } : { ok: false, reason: `Couldn't clone ${url} to ${to}: ${why(r)}` };
}

/**
 * The same repo on two machines, from their `origin` URLs:
 * git@github.com:mikecann/threadbear.git, https://github.com/mikecann/threadbear
 * and ssh://git@github.com/mikecann/threadbear.git are one repo.
 */
export function repoKey(url: string): string {
  let s = url.trim().replace(/\/+$/, '').replace(/\.git$/, '');
  const scp = /^[^/@]+@([^:/]+):(.+)$/.exec(s);
  if (scp) s = `${scp[1]}/${scp[2]}`;
  else s = s.replace(/^[a-z+]+:\/\//i, '').replace(/^[^@/]+@/, '');
  return s.toLowerCase();
}

// Where S's origin's default branch is, as S last fetched it.
async function originDefault(cwd: string): Promise<string | undefined> {
  const sym = await out(cwd, ['symbolic-ref', '--quiet', 'refs/remotes/origin/HEAD']);
  for (const ref of [sym, 'refs/remotes/origin/main', 'refs/remotes/origin/master']) {
    if (ref && (await out(cwd, ['rev-parse', '--verify', '--quiet', `${ref}^{commit}`]))) return ref;
  }
  return undefined;
}

/** Files changed in a checkout, as `git status` names them (for what stays behind). */
export async function changedFiles(cwd: string, max = 20): Promise<string[]> {
  // Not trimmed: the first line's leading space is part of its status.
  // A submodule's own config isn't read, so git doesn't look inside one.
  const r = await run(cwd, ['status', '--porcelain=v1', '--untracked-files=all', '--ignore-submodules=all']);
  if (!r.ok) return [];
  return r.stdout
    .split('\n')
    .map((l) => l.slice(3).trim())
    .filter(Boolean)
    .slice(0, max);
}

/**
 * Bundles the work in `worktree` into `bundleFile`. No bundle (undefined
 * `bundle`) when there's nothing origin doesn't have: T's worktree from
 * origin is the same.
 */
export async function packGit(worktree: string, cwd: string, bundleFile: string): Promise<{ ok: true; meta: GitMeta; bundle?: string } | { ok: false; reason: string }> {
  const top = await repoTop(worktree);
  if (!top) return { ok: false, reason: `${worktree} isn't a git checkout.` };
  const head = await out(top, ['rev-parse', '--verify', 'HEAD^{commit}']);
  if (!head) return { ok: false, reason: `${top} has no commits.` };
  const branch = (await out(top, ['rev-parse', '--abbrev-ref', 'HEAD'])) ?? 'HEAD';
  const origin = await originUrl(top);
  const def = origin ? await originDefault(top) : undefined;
  const base = def ? await out(top, ['merge-base', 'HEAD', def]) : undefined;
  const rel = path.relative(top, cwd);
  const subdir = rel && !rel.startsWith('..') ? rel.split(path.sep).join('/') : '';

  // Every file as it is now, tracked or not, in a tree of its own: a
  // throwaway index, so the checkout's own staging is left alone.
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-move-index-'));
  let wip: string | undefined;
  try {
    const env = { GIT_INDEX_FILE: path.join(tmp, 'index'), ...AUTHOR };
    let r = await run(top, ['read-tree', 'HEAD'], env);
    if (r.ok) r = await run(top, ['add', '-A', '.'], env);
    if (!r.ok) return { ok: false, reason: `Couldn't read the uncommitted changes in ${top}: ${why(r)}` };
    const tree = await out(top, ['write-tree'], env);
    const headTree = await out(top, ['rev-parse', 'HEAD^{tree}']);
    if (!tree) return { ok: false, reason: `Couldn't read the uncommitted changes in ${top}.` };
    if (tree !== headTree) {
      wip = await out(top, ['commit-tree', tree, '-p', head, '-m', 'Threadbear move: uncommitted changes'], env);
      if (!wip) return { ok: false, reason: `Couldn't save the uncommitted changes in ${top}.` };
    }
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  const meta: GitMeta = { branch, head, ...(wip ? { wip } : {}), ...(base ? { base } : {}), ...(origin ? { origin: withoutCredentials(origin) } : {}), subdir };
  if (base === head && !wip) return { ok: true, meta };

  // A bundle names refs, so the tips get ones of their own for as long as
  // it takes. With head at the base only the wip commit goes (git leaves
  // the head ref out), and T has head from origin.
  await run(top, ['update-ref', HEAD_REF, head]);
  if (wip) await run(top, ['update-ref', WIP_REF, wip]);
  try {
    const r = await run(top, ['bundle', 'create', bundleFile, HEAD_REF, ...(wip ? [WIP_REF] : []), ...(base ? [`^${base}`] : [])]);
    if (!r.ok) return { ok: false, reason: `Couldn't bundle the work in ${top}: ${why(r)}` };
  } finally {
    await run(top, ['update-ref', '-d', HEAD_REF]);
    if (wip) await run(top, ['update-ref', '-d', WIP_REF]);
  }
  return { ok: true, meta, bundle: bundleFile };
}

/**
 * Puts the work back in `worktree`, a checkout of the same repo on the
 * thread's branch: fetches the bundle (fetching origin first if the
 * commits it starts from aren't there), moves the branch to `head` and
 * leaves `wip`'s changes uncommitted.
 */
export async function unpackGit(worktree: string, meta: GitMeta, bundleFile?: string): Promise<{ ok: true } | { ok: false; reason: string }> {
  const fetchBundle = () => run(worktree, ['fetch', '--quiet', '--no-tags', bundleFile!, `+${MOVE_REFS}*:${MOVE_REFS}*`], {}, 'bundle');
  if (bundleFile) {
    let r = await fetchBundle();
    if (!r.ok) {
      // Most likely the commits it starts from: they're on origin.
      await run(worktree, ['fetch', '--quiet', 'origin'], {}, 'remote');
      r = await fetchBundle();
    }
    if (!r.ok) return { ok: false, reason: `Couldn't bring the git work in: ${why(r)}` };
  } else if (!(await out(worktree, ['rev-parse', '--verify', '--quiet', `${meta.head}^{commit}`]))) {
    await run(worktree, ['fetch', '--quiet', 'origin'], {}, 'remote');
  }
  try {
    if (!(await out(worktree, ['rev-parse', '--verify', '--quiet', `${meta.head}^{commit}`]))) return { ok: false, reason: `This machine's copy of the repo doesn't have ${meta.head.slice(0, 12)}, and origin didn't either.` };
    let r = await run(worktree, ['reset', '--quiet', '--hard', meta.head]);
    if (!r.ok) return { ok: false, reason: `Couldn't put the branch where it was: ${why(r)}` };
    if (meta.wip) {
      // What the uncommitted work brings under a node_modules folder would
      // be untracked here, which trust reads as installed packages and
      // doesn't hash, so a trusted patch could import them unchecked. The
      // old runner's `add -A` leaves ignored files behind, so an honest
      // move never has any (Codex's review, 10 Nov 2026).
      const brought = await run(worktree, ['diff', '--name-only', '-z', '--no-renames', '--no-ext-diff', '--no-textconv', meta.head, meta.wip]);
      if (!brought.ok) return { ok: false, reason: `Couldn't read the uncommitted changes: ${why(brought)}` };
      const packaged = brought.stdout.split('\0').find((f) => f.split('/').some((part) => part.toLowerCase() === 'node_modules'));
      if (packaged) return { ok: false, reason: `Its uncommitted changes include ${packaged}, under node_modules, which a move doesn't carry: installed packages are set up again on this machine.` };
      // The worktree as the wip commit has it, then the index back at head:
      // its changes are uncommitted again, new files untracked again.
      r = await run(worktree, ['read-tree', '-u', '--reset', meta.wip]);
      if (r.ok) r = await run(worktree, ['reset', '--quiet', meta.head]);
      if (!r.ok) return { ok: false, reason: `Couldn't put the uncommitted changes back: ${why(r)}` };
    }
    return { ok: true };
  } finally {
    const refs = await out(worktree, ['for-each-ref', '--format=%(refname)', MOVE_REFS]);
    for (const ref of (refs ?? '').split('\n').filter(Boolean)) await run(worktree, ['update-ref', '-d', ref]);
  }
}
