// A moving thread's git work (src/move-git.ts), with real repos in temp
// folders: the branch, commits only the old machine's main has, and every
// uncommitted change come across; ignored files don't.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { changedFiles, packGit, repoKey, unpackGit } from '../../src/move-git.ts';

const tmp = () => fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-move-git-')));
const git = (cwd: string, ...args: string[]) => execFileSync('git', ['-C', cwd, '-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'init.defaultBranch=main', '-c', 'advice.detachedHead=false', ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
const write = (dir: string, file: string, text: string) => {
  fs.mkdirSync(path.dirname(path.join(dir, file)), { recursive: true });
  fs.writeFileSync(path.join(dir, file), text);
};

// origin with one commit, cloned as S's main checkout and as T's.
function repos() {
  const root = tmp();
  const seed = path.join(root, 'seed');
  fs.mkdirSync(seed);
  git(seed, 'init', '-q');
  write(seed, 'a.txt', 'a\n');
  write(seed, 'gone.txt', 'old\n');
  write(seed, '.gitignore', 'node_modules/\n');
  write(seed, 'app/readme.md', 'app\n');
  git(seed, 'add', '-A');
  git(seed, 'commit', '-qm', 'A');
  const origin = path.join(root, 'origin.git');
  execFileSync('git', ['clone', '-q', '--bare', seed, origin]);
  const s = path.join(root, 's');
  const t = path.join(root, 't');
  execFileSync('git', ['clone', '-q', origin, s]);
  execFileSync('git', ['clone', '-q', origin, t]);
  return { root, origin, s, t };
}

test("a worktree's commits, S's unpushed main and every uncommitted change come across", async () => {
  const { root, s, t } = repos();
  // S's main has a commit origin doesn't (tb land never pushes).
  write(s, 'landed.txt', 'only on S\n');
  git(s, 'add', '-A');
  git(s, 'commit', '-qm', 'M');
  const sw = path.join(root, 's-threads/slot-1');
  git(s, 'worktree', 'add', '-q', '-b', 'thread/abcd1234', sw, 'main');
  write(sw, 'b.txt', 'b\n');
  git(sw, 'add', '-A');
  git(sw, 'commit', '-qm', 'B');
  // Uncommitted: a change, a staged change, a deletion, a new file, and an ignored one.
  write(sw, 'a.txt', 'a changed\n');
  write(sw, 'app/readme.md', 'staged\n');
  git(sw, 'add', 'app/readme.md');
  fs.rmSync(path.join(sw, 'gone.txt'));
  write(sw, 'new/file.txt', 'untracked\n');
  write(sw, 'node_modules/x.js', 'ignored\n');
  const before = git(sw, 'rev-parse', 'HEAD');
  const stagedBefore = git(sw, 'diff', '--cached', '--name-only');

  const bundle = path.join(root, 'move.bundle');
  const packed = await packGit(sw, path.join(sw, 'app'), bundle);
  assert.ok(packed.ok, !packed.ok ? packed.reason : '');
  if (!packed.ok) return;
  assert.equal(packed.meta.branch, 'thread/abcd1234');
  assert.equal(packed.meta.head, before);
  assert.equal(packed.meta.subdir, 'app');
  assert.ok(packed.meta.wip && packed.meta.base);
  assert.equal(packed.bundle, bundle);
  // S's checkout is just as it was: same HEAD, same staging, no move refs.
  assert.equal(git(sw, 'rev-parse', 'HEAD'), before);
  assert.equal(git(sw, 'diff', '--cached', '--name-only'), stagedBefore);
  assert.equal(git(s, 'for-each-ref', 'refs/threadbear'), '');

  // T makes the thread's worktree from its own main, as for a new thread.
  const tw = path.join(root, 't-threads/slot-3');
  git(t, 'worktree', 'add', '-q', '-b', 'thread/abcd1234', tw, 'main');
  const r = await unpackGit(tw, packed.meta, bundle);
  assert.ok(r.ok, !r.ok ? r.reason : '');
  assert.equal(git(tw, 'rev-parse', 'HEAD'), before);
  assert.equal(git(tw, 'rev-parse', '--abbrev-ref', 'HEAD'), 'thread/abcd1234');
  assert.equal(fs.readFileSync(path.join(tw, 'landed.txt'), 'utf8'), 'only on S\n');
  assert.equal(fs.readFileSync(path.join(tw, 'a.txt'), 'utf8'), 'a changed\n');
  assert.equal(fs.readFileSync(path.join(tw, 'app/readme.md'), 'utf8'), 'staged\n');
  assert.equal(fs.existsSync(path.join(tw, 'gone.txt')), false);
  assert.equal(fs.readFileSync(path.join(tw, 'new/file.txt'), 'utf8'), 'untracked\n');
  assert.equal(fs.existsSync(path.join(tw, 'node_modules/x.js')), false, 'ignored files stay behind');
  // Uncommitted again (the staging itself isn't kept), new files untracked again.
  const status = ' ' + git(tw, 'status', '--porcelain'); // the helper trims the first line's leading space
  assert.match(status, /^ M a\.txt$/m);
  assert.match(status, /^ M app\/readme\.md$/m);
  assert.match(status, /^ D gone\.txt$/m);
  assert.match(status, /^\?\? new\/$/m);
  assert.equal(git(t, 'for-each-ref', 'refs/threadbear'), '');
});

test('nothing to carry makes no bundle, and T is left at the same commit', async () => {
  const { root, s, t } = repos();
  const sw = path.join(root, 'sw');
  git(s, 'worktree', 'add', '-q', '-b', 'thread/x', sw, 'origin/main');
  const packed = await packGit(sw, sw, path.join(root, 'b.bundle'));
  assert.ok(packed.ok);
  if (!packed.ok) return;
  assert.equal(packed.bundle, undefined);
  assert.equal(fs.existsSync(path.join(root, 'b.bundle')), false);
  const tw = path.join(root, 'tw');
  git(t, 'worktree', 'add', '-q', '-b', 'thread/x', tw, 'main');
  assert.deepEqual(await unpackGit(tw, packed.meta), { ok: true });
  assert.equal(git(tw, 'rev-parse', 'HEAD'), packed.meta.head);
});

test('only uncommitted changes, on a branch origin already has', async () => {
  const { root, s, t } = repos();
  const sw = path.join(root, 'sw');
  git(s, 'worktree', 'add', '-q', '-b', 'thread/y', sw, 'origin/main');
  write(sw, 'a.txt', 'edited\n');
  const packed = await packGit(sw, sw, path.join(root, 'y.bundle'));
  assert.ok(packed.ok && packed.bundle && packed.meta.wip);
  if (!packed.ok) return;
  const tw = path.join(root, 'tw');
  git(t, 'worktree', 'add', '-q', '-b', 'thread/y', tw, 'main');
  assert.deepEqual(await unpackGit(tw, packed.meta, packed.bundle), { ok: true });
  assert.equal(fs.readFileSync(path.join(tw, 'a.txt'), 'utf8'), 'edited\n');
  assert.equal(git(tw, 'status', '--porcelain'), 'M a.txt'); // ' M', trimmed
});

test('a repo with no origin sends its whole history', async () => {
  const root = tmp();
  const s = path.join(root, 's');
  fs.mkdirSync(s);
  git(s, 'init', '-q');
  write(s, 'x.txt', '1\n');
  git(s, 'add', '-A');
  git(s, 'commit', '-qm', 'one');
  const packed = await packGit(s, s, path.join(root, 'all.bundle'));
  assert.ok(packed.ok && packed.bundle);
  if (!packed.ok) return;
  assert.equal(packed.meta.base, undefined);
  assert.equal(packed.meta.origin, undefined);
  const t = path.join(root, 't');
  fs.mkdirSync(t);
  git(t, 'init', '-q');
  assert.deepEqual(await unpackGit(t, packed.meta, packed.bundle), { ok: true });
  assert.equal(fs.readFileSync(path.join(t, 'x.txt'), 'utf8'), '1\n');
});

test("a bundle whose base the new machine can't get says so", async () => {
  const { root, s } = repos();
  write(s, 'z.txt', 'z\n');
  git(s, 'add', '-A');
  git(s, 'commit', '-qm', 'Z');
  const packed = await packGit(s, s, path.join(root, 'z.bundle'));
  assert.ok(packed.ok && packed.bundle);
  if (!packed.ok) return;
  // A different repo altogether.
  const other = path.join(root, 'other');
  fs.mkdirSync(other);
  git(other, 'init', '-q');
  write(other, 'q', 'q');
  git(other, 'add', '-A');
  git(other, 'commit', '-qm', 'q');
  const r = await unpackGit(other, packed.meta, packed.bundle);
  assert.equal(r.ok, false);
  assert.match(!r.ok ? r.reason : '', /Couldn't bring the git work in/);
});

test('one repo under its several origin URLs', () => {
  const k = repoKey('https://github.com/mikecann/threadbear');
  for (const u of ['git@github.com:mikecann/threadbear.git', 'https://github.com/mikecann/threadbear.git', 'ssh://git@github.com/mikecann/threadbear.git', 'https://x-access-token@github.com/MikeCann/threadbear/']) assert.equal(repoKey(u), k, u);
  assert.notEqual(repoKey('git@github.com:mikecann/other.git'), k);
});

test('what changed in a shared checkout, for what stays behind', async () => {
  const { s } = repos();
  write(s, 'a.txt', 'x\n');
  write(s, 'n.txt', 'n\n');
  assert.deepEqual((await changedFiles(s)).sort(), ['a.txt', 'n.txt']);
});
