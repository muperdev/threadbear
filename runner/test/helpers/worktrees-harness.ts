// A worktree per thread (docs/thread-worktrees.md): the patch's `worktree`,
// making one when a thread starts, `tb land`, and cleaning up. Real git, in
// temp repos.
// The helpers shared by the worktrees-*.test.ts files. It's one suite split
// across files because tests in one file run one after another, and this one
// was started last and held the whole unit suite up by itself (9 Nov).
import { test } from 'node:test';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync, spawn } from 'node:child_process';
import { makeHarness, tmpDir, type Harness } from './fakes.ts';
import { git, gitLimits, land, type LandTarget } from '../../src/worktrees.ts';
import { PATIENCE_MS } from './patience.ts';
import { PatchTrust } from '../../src/patch-trust.ts';

// Commits need a name, and Mike's own git config (signing, hooks) mustn't
// reach the temp repos. Each test file runs in its own process.
Object.assign(process.env, {
  GIT_CONFIG_GLOBAL: '/dev/null',
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'Test',
  GIT_AUTHOR_EMAIL: 'test@example.com',
  GIT_COMMITTER_NAME: 'Test',
  GIT_COMMITTER_EMAIL: 'test@example.com',
});

export const TB = path.join(import.meta.dirname, '..', '..', 'bin', 'tb');

export function sh(cwd: string, cmd: string): string {
  return execFileSync('/bin/sh', ['-c', cmd], { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
}

export function write(dir: string, files: Record<string, string>): void {
  for (const [name, text] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(dir, name)), { recursive: true });
    fs.writeFileSync(path.join(dir, name), text);
  }
}

export const PATCH = (w: string) => `export default { worktree: ${w} };\n`;

// A repo at <root>/proj on main, with a runner/ folder and the given patch.
export function makeRepo(patch = PATCH(`{ setup: "echo setting up in $PWD; touch .setup-ran" }`)): { root: string; main: string } {
  const root = fs.realpathSync(tmpDir('tb-wt-'));
  const main = path.join(root, 'proj');
  fs.mkdirSync(main);
  sh(main, 'git init -q -b main');
  write(main, {
    '.gitignore': 'node_modules\n.setup-ran\n.build\n',
    'README.md': 'one\ntwo\nthree\n',
    'runner/a.ts': 'export const a = 1;\n',
    ...(patch ? { '.threadbear/patches/checks.ts': patch } : {}),
  });
  sh(main, 'git add -A && git commit -qm init');
  return { root, main };
}

// A thread/ worktree of `main` by hand, for the landing tests.
export function makeBranch(main: string, id8 = 'abcd1234'): LandTarget {
  const wt = path.join(path.dirname(main), 'proj-threads', id8);
  sh(main, `git worktree add -q -b thread/${id8} ${wt} main`);
  return { path: wt, branch: `thread/${id8}`, mainCheckout: main };
}

export function commit(dir: string, files: Record<string, string>, message: string): void {
  write(dir, files);
  sh(dir, `git add -A && git commit -qm "${message}"`);
}

export function capture() {
  const out: string[] = [];
  const err: string[] = [];
  return { out, err, opts: { out: (l: string) => out.push(l), err: (l: string) => err.push(l) } };
}

export async function until(check: () => boolean, what: string, ms = PATIENCE_MS): Promise<void> {
  const end = Date.now() + ms;
  while (!check()) {
    if (Date.now() > end) throw new Error(`Timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 10));
  }
}

export function harness(): Harness {
  return makeHarness(undefined, {}, { actionShell: ['/bin/sh', '-c'], actionTimeoutMs: 10_000 });
}

// Puts a git ahead of the real one on PATH that takes half a second over
// `worktree remove`, until the function it returns is called. Tests that
// look at a cleanup while git is still removing the folder need it: on a
// fast disk the removal finished before they looked (SeeThruHead's fork,
// 7 Nov).
export function slowWorktreeRemoval(): () => void {
  const realGit = sh('/', 'command -v git');
  const bin = tmpDir('tb-slow-git-');
  fs.writeFileSync(path.join(bin, 'git'), `#!/bin/sh\ncase " $* " in *" worktree remove "*) sleep 0.5 ;; esac\nexec '${realGit}' "$@"\n`, { mode: 0o755 });
  const before = process.env.PATH;
  process.env.PATH = `${bin}${path.delimiter}${before}`;
  return () => { process.env.PATH = before; };
}

export function runTb(args: string[], cwd: string, env: Record<string, string | undefined>): Promise<{ code: number | null; out: string }> {
  return startTb(args, cwd, env).done;
}

export function startTb(args: string[], cwd: string, env: Record<string, string | undefined>) {
  // Not the thread these tests run in, if they do: tb land refuses
  // another thread's branch. A test that wants one passes it.
  const full: Record<string, string | undefined> = { ...process.env };
  delete full.THREADBEAR_THREAD_ID;
  delete full.THREADBARE_THREAD_ID;
  Object.assign(full, env);
  for (const [k, v] of Object.entries(env)) if (v === undefined) delete full[k];
  const p = spawn(process.execPath, [TB, ...args], { cwd, env: full, stdio: ['ignore', 'pipe', 'pipe'] });
  let out = '';
  p.stdout.on('data', (d) => (out += d));
  p.stderr.on('data', (d) => (out += d));
  const done = new Promise<{ code: number | null; out: string }>((resolve) => {
    p.on('exit', (code) => resolve({ code, out }));
  });
  return { child: p, done, output: () => out };
}

// A copy of the repo's borrow-worktree.sh in `main`, with a setup that does nothing.
export function addBorrowScript(main: string): void {
  fs.copyFileSync(path.join(import.meta.dirname, '..', '..', '..', 'borrow-worktree.sh'), path.join(main, 'borrow-worktree.sh'));
  write(main, { 'worktree-setup.sh': '#!/bin/sh\n' });
  sh(main, 'chmod +x borrow-worktree.sh worktree-setup.sh && git add -A && git commit -qm scripts');
}

// ------------------------------------------------------------- cleaning up

// A thread with a worktree whose first turn has finished.
export async function threadWithWorktree(h: Harness, main: string) {
  const started = h.adapter.sessions.length;
  const { thread } = h.runner.create({ text: 'Go', accountId: 'claude-3', cwd: path.join(main, 'runner') });
  await until(() => h.adapter.sessions.length === started + 1, 'the CLI to start');
  const s = h.adapter.last;
  s.init();
  s.result();
  const w = h.runner.get(thread.id).worktree!;
  return { id: thread.id, w };
}

export const notes = (h: Harness, id: string) => h.events(id).filter((e) => e.type === 'system.note').map((e) => e.text as string);

// -------------------------------------------------------------- teardown

// A teardown that writes where it ran, and "there" while the worktree's
// files still are, to a log outside the repo.
export const TEARDOWN = (log: string, then = '') => PATCH(`{ teardown: "echo $PWD $(test -f runner/a.ts && echo there) >> ${log}${then}" }`);
export const lines = (log: string) => (fs.existsSync(log) ? fs.readFileSync(log, 'utf8').trim().split('\n') : []);
export const teardowns = (h: Harness, id: string) => h.events(id).filter((e) => e.type === 'action.run' && e.trigger === 'worktree.teardown');

// A repo whose patch's teardown is ./teardown.sh, writing where it ran to
// `log`, committed and trusted for real (not TRUST_ALL), with a harness on
// that trust.
export function trustedTeardownRepo(log: string) {
  const { main } = makeRepo(PATCH(`{ teardown: "./teardown.sh" }`));
  commit(main, { 'teardown.sh': `#!/bin/sh\necho "torn down $PWD" >> ${log}\n` }, 'teardown');
  fs.chmodSync(path.join(main, 'teardown.sh'), 0o755);
  sh(main, 'git add -A && git commit -qm modes');
  const trust = new PatchTrust();
  trust.trustCurrent([path.join(main, '.threadbear/patches/checks.ts')]);
  const h = makeHarness(undefined, {}, { actionShell: ['/bin/sh', '-c'], actionTimeoutMs: 10_000, patchTrust: trust });
  return { main, trust, h };
}

// Puts a git ahead of the real one on PATH that, the first time it's asked
// for `match` (a word in its arguments), first swaps the worktree folder for
// a link to the main checkout, as if someone did while the runner waited on
// git. Until the function it returns is called.
export function swapDuringGit(match: string, wt: string, main: string): () => void {
  const realGit = sh('/', 'command -v git');
  const bin = tmpDir('tb-swap-git-');
  const done = path.join(bin, 'swapped');
  fs.writeFileSync(
    path.join(bin, 'git'),
    `#!/bin/sh\ncase " $* " in *" ${match} "*) if [ ! -e '${done}' ]; then touch '${done}'; mv '${wt}' '${wt}-moved'; ln -s '${main}' '${wt}'; fi ;; esac\nexec '${realGit}' "$@"\n`,
    { mode: 0o755 },
  );
  const before = process.env.PATH;
  process.env.PATH = `${bin}${path.delimiter}${before}`;
  return () => { process.env.PATH = before; };
}

// A .git swapped for a fifo (or any git that hangs) used to hold the cleanup,
// and everything waiting on it, for ever.
// A git ahead of the real one on PATH whose `case` arm for `match` (a word
// in its arguments) runs `body` (with $REAL the real git), and git's time
// limit set to `limitMs`, until the function it returns is called.
// A limit for the tests whose wrapped git hangs (sleep 30): it applies to
// every git call the code makes, not only the hung one, so it has to leave
// room for the ordinary ones through the wrapper. At 300 ms, `tb land`'s
// first rev-parse timed out on a loaded check host and at background
// priority, and read as "isn't a git checkout" (9 Nov).
export const HUNG_GIT_LIMIT_MS = 3000;

export function wrapGit(match: string, body: string, limitMs = gitLimits.timeoutMs): () => void {
  const realGit = sh('/', 'command -v git');
  const bin = tmpDir('tb-wrap-git-');
  fs.writeFileSync(path.join(bin, 'git'), `#!/bin/sh\nREAL='${realGit}'\ncase " $* " in *" ${match} "*) ${body} ;; esac\nexec "$REAL" "$@"\n`, { mode: 0o755 });
  const before = process.env.PATH;
  const limit = gitLimits.timeoutMs;
  process.env.PATH = `${bin}${path.delimiter}${before}`;
  gitLimits.timeoutMs = limitMs;
  return () => {
    process.env.PATH = before;
    gitLimits.timeoutMs = limit;
  };
}

// ---------------------------------------------------------------- spares

export const KEEP = PATCH(`{ setup: "touch .setup-ran", keep: ["app/.build"] }`);

export const isLocked = (main: string, wt: string) => sh(main, 'git worktree list --porcelain').split('\n\n').some((b) => b.includes(`worktree ${wt}\n`) && b.includes('locked threadbear spare'));

// ---- turn.end hooks and an archive's cleanup (7 Nov): the hook was still
// running in the worktree when archiving removed it, so a passing typecheck
// "failed" on missing files and woke the thread that had just been archived.

export const HOOK_PATCH = (run: string, tell = 'failures') => PATCH(`{ setup: "true" }, hooks: [{ title: "Check", on: "turn.end", run: ${JSON.stringify(run)}, edited: "**/*.ts", tell: ${JSON.stringify(tell)} }]`);

// Ends a turn that edited a .ts file, with end_turn asking for the archive,
// so the hook is running as the archive happens.
export async function archiveAfterEdit(h: Harness, id: string): Promise<void> {
  await h.runner.patchesIdle();
  const s = h.adapter.last;
  h.runner.send(id, 'Finish up');
  s.out({ kind: 'event', event: { type: 'edit.diff', itemId: 'e1', path: 'a.ts', hunks: [] } });
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 't1', name: 'mcp__threadbear__end_turn', kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', archive: true } } });
  s.text('m1', 'All landed.');
  s.result('completed');
}

export const hookRuns = (h: Harness, id: string) => h.events(id).filter((e) => e.type === 'action.run' && e.trigger === 'turn.end');
export const toldAgent = (h: Harness, id: string) => h.events(id).filter((e) => e.type === 'user.message' && e.origin === 'runner');
