// A second machine following GitHub's main by itself (follow-origin.ts).
// Real git in throwaway repos: an "origin", and a checkout cloned from it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { tmpDir } from '../helpers/fakes.ts';
import { cleanGitError, followOrigin, ORIGIN_MAIN, oldestMissingAt, followOriginOn, landRefusal } from '../../src/follow-origin.ts';
import { syncClone } from '../../src/update.ts';

const env = {
  ...process.env,
  GIT_CONFIG_GLOBAL: '/dev/null',
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'Follow Test',
  GIT_AUTHOR_EMAIL: 'follow-test@example.com',
  GIT_COMMITTER_NAME: 'Follow Test',
  GIT_COMMITTER_EMAIL: 'follow-test@example.com',
};

function git(cwd: string, ...args: string[]): string {
  const r = spawnSync('git', ['-C', cwd, ...args], { encoding: 'utf8', env });
  assert.equal(r.status, 0, `git ${args.join(' ')}: ${r.stderr}`);
  return r.stdout.trim();
}

function write(repo: string, file: string, text: string): void {
  fs.mkdirSync(path.dirname(path.join(repo, file)), { recursive: true });
  fs.writeFileSync(path.join(repo, file), text);
}

function commit(repo: string, msg: string, file = 'runner/src/main.ts', text = `${msg}\n`): string {
  write(repo, file, text);
  git(repo, 'add', '-A');
  git(repo, 'commit', '-q', '-m', msg);
  return git(repo, 'rev-parse', 'HEAD');
}

// `upstream` stands for GitHub; `checkout` is the machine's clone of it.
function rig() {
  const upstream = tmpDir('tb-origin-up-');
  git(upstream, 'init', '-q', '-b', 'main');
  // Pushing isn't what's tested: commits are made straight in `upstream`.
  write(upstream, 'runner/bin/tb', '#!/bin/sh\n');
  write(upstream, 'runner/package.json', '{"name":"x"}\n');
  write(upstream, 'runner/package-lock.json', '{"name":"x","lockfileVersion":3}\n');
  const first = commit(upstream, 'first');
  const checkout = path.join(tmpDir('tb-origin-co-'), 'threadbear');
  const r = spawnSync('git', ['clone', '-q', upstream, checkout], { encoding: 'utf8', env });
  assert.equal(r.status, 0, r.stderr);
  return { upstream, checkout, first };
}

test('a new commit on GitHub fast-forwards the checkout, and the ref to build is origin/main', async () => {
  const { upstream, checkout } = rig();
  const tip = commit(upstream, 'second');
  const res = await followOrigin(checkout);
  assert.equal(res.problem, undefined);
  assert.equal(res.ref, ORIGIN_MAIN);
  assert.equal(git(checkout, 'rev-parse', 'refs/heads/main'), tip);
  assert.equal(git(checkout, 'rev-parse', 'HEAD'), tip);
  // The working tree moved with it.
  assert.equal(fs.readFileSync(path.join(checkout, 'runner/src/main.ts'), 'utf8'), 'second\n');
});

test('up to date says nothing and changes nothing', async () => {
  const { checkout, first } = rig();
  const res = await followOrigin(checkout);
  assert.equal(res.problem, undefined);
  assert.equal(git(checkout, 'rev-parse', 'refs/heads/main'), first);
});

test('a commit made only on this machine is kept, said loudly, and the runner still builds GitHub', async () => {
  // What stranded bruce for three days: a07a9f01 on its local main.
  const { upstream, checkout } = rig();
  const stray = commit(checkout, 'Patch trust tests: a slow check', 'runner/README.md', 'stray\n');
  const tip = commit(upstream, 'on github');
  const res = await followOrigin(checkout);
  assert.equal(res.problem?.kind, 'diverged');
  assert.match(res.problem!.message, /1 commit/);
  assert.match(res.problem!.message, /Patch trust tests/);
  assert.match(res.problem!.message, /git reset --keep origin\/main/);
  // Nothing was thrown away.
  assert.equal(git(checkout, 'rev-parse', 'refs/heads/main'), stray);
  // And the clone gets GitHub's main, not the stray one.
  assert.equal(res.ref, ORIGIN_MAIN);
  const dir = path.join(tmpDir('tb-origin-clone-'), 'src');
  const main = await syncClone(checkout, 'runner/', dir, res.ref);
  assert.equal(main.commit, tip);
  assert.ok(!fs.existsSync(path.join(dir, 'runner/README.md')), "the stray commit's file isn't built");
});

test('local main ahead of GitHub with nothing new there is diverged too', async () => {
  const { checkout } = rig();
  commit(checkout, 'only here', 'runner/README.md', 'x\n');
  assert.equal((await followOrigin(checkout)).problem?.kind, 'diverged');
});

test("uncommitted changes git won't overwrite are said, and left alone", async () => {
  const { upstream, checkout, first } = rig();
  commit(upstream, 'touches main.ts');
  write(checkout, 'runner/src/main.ts', 'half done\n');
  const res = await followOrigin(checkout);
  assert.equal(res.problem?.kind, 'stuck');
  assert.match(res.problem!.message, /main\.ts/);
  assert.equal(git(checkout, 'rev-parse', 'refs/heads/main'), first);
  assert.equal(fs.readFileSync(path.join(checkout, 'runner/src/main.ts'), 'utf8'), 'half done\n');
  // Still knows what to build.
  assert.equal(res.ref, ORIGIN_MAIN);
});

test('uncommitted changes in other files do not stop it', async () => {
  const { upstream, checkout } = rig();
  const tip = commit(upstream, 'touches main.ts');
  write(checkout, 'runner/notes.txt', 'mine\n');
  assert.equal((await followOrigin(checkout)).problem, undefined);
  assert.equal(git(checkout, 'rev-parse', 'refs/heads/main'), tip);
  assert.equal(fs.readFileSync(path.join(checkout, 'runner/notes.txt'), 'utf8'), 'mine\n');
});

test('a checkout on another branch still has its main moved, without touching its files', async () => {
  const { upstream, checkout } = rig();
  git(checkout, 'checkout', '-q', '-b', 'side');
  const before = fs.readFileSync(path.join(checkout, 'runner/src/main.ts'), 'utf8');
  const tip = commit(upstream, 'second');
  assert.equal((await followOrigin(checkout)).problem, undefined);
  assert.equal(git(checkout, 'rev-parse', 'refs/heads/main'), tip);
  assert.equal(git(checkout, 'symbolic-ref', '--short', 'HEAD'), 'side');
  assert.equal(fs.readFileSync(path.join(checkout, 'runner/src/main.ts'), 'utf8'), before);
});

test('main checked out in another worktree moves there', async () => {
  const { upstream, checkout, first } = rig();
  git(checkout, 'checkout', '-q', '-b', 'side');
  const other = path.join(tmpDir('tb-origin-wt-'), 'wt');
  git(checkout, 'worktree', 'add', '-q', other, 'main');
  assert.equal(git(other, 'rev-parse', 'HEAD'), first);
  const tip = commit(upstream, 'second');
  assert.equal((await followOrigin(checkout)).problem, undefined);
  assert.equal(git(other, 'rev-parse', 'HEAD'), tip);
  assert.equal(fs.readFileSync(path.join(other, 'runner/src/main.ts'), 'utf8'), 'second\n');
});

test('main being rebased is left alone and said, though the worktree list shows it detached', async () => {
  const { upstream, checkout } = rig();
  // A rebase of main onto a conflicting commit: it stops partway, HEAD detached.
  git(checkout, 'checkout', '-q', '-b', 'onto');
  commit(checkout, 'onto', 'runner/README.md', 'onto\n');
  git(checkout, 'checkout', '-q', 'main');
  commit(checkout, 'conflicts', 'runner/README.md', 'main\n');
  const before = git(checkout, 'rev-parse', 'refs/heads/main');
  assert.notEqual(spawnSync('git', ['-C', checkout, 'rebase', 'onto'], { encoding: 'utf8', env }).status, 0);
  // GitHub moves on from the main being rebased, so it's a fast-forward.
  git(upstream, 'fetch', '-q', checkout, 'refs/heads/main');
  git(upstream, 'merge', '-q', '--ff-only', 'FETCH_HEAD');
  commit(upstream, 'second');
  const res = await followOrigin(checkout);
  assert.equal(res.problem?.kind, 'stuck');
  assert.equal(git(checkout, 'rev-parse', 'refs/heads/main'), before);
});

test('GitHub out of reach is a fetch problem; what was last fetched is still built', async () => {
  const { upstream, checkout, first } = rig();
  fs.rmSync(upstream, { recursive: true, force: true });
  const res = await followOrigin(checkout);
  assert.equal(res.problem?.kind, 'fetch');
  assert.equal(res.ref, ORIGIN_MAIN);
  assert.equal(git(checkout, 'rev-parse', ORIGIN_MAIN), first);
});

test('no origin at all: nothing to build from there', async () => {
  const { checkout } = rig();
  git(checkout, 'remote', 'remove', 'origin');
  const res = await followOrigin(checkout);
  assert.equal(res.problem?.kind, 'fetch');
  assert.equal(res.ref, undefined);
});

test('a token in a remote URL never reaches the message', () => {
  assert.equal(cleanGitError("fatal: unable to access 'https://x-access-token:ghs_abc123@github.com/mikecann/threadbear.git/': 403"), "fatal: unable to access 'https://github.com/mikecann/threadbear.git/': 403");
  assert.ok(cleanGitError('x\n'.repeat(500)).length <= 300);
});

test('oldestMissingAt: only commits that change what runs count, and the oldest of those', async () => {
  const { upstream, checkout, first } = rig();
  commit(upstream, 'docs', 'docs/guide.md', 'docs\n');
  commit(upstream, 'code');
  git(upstream, 'commit', '-q', '--allow-empty', '-m', 'empty');
  await followOrigin(checkout);
  const at = await oldestMissingAt(checkout, 'runner/', first, ORIGIN_MAIN);
  const expected = Number(git(checkout, 'log', '-1', '--format=%ct', '--grep=^code$', ORIGIN_MAIN)) * 1000;
  assert.equal(at, expected);
  // Nothing that runs has changed since the tip.
  assert.equal(await oldestMissingAt(checkout, 'runner/', git(checkout, 'rev-parse', ORIGIN_MAIN), ORIGIN_MAIN), undefined);
  // A commit this checkout doesn't have can't be counted from.
  assert.equal(await oldestMissingAt(checkout, 'runner/', 'f'.repeat(40), ORIGIN_MAIN), undefined);
});

test('followOriginOn reads runner.json each time, and only true counts', () => {
  const home = tmpDir('tb-origin-home-');
  assert.equal(followOriginOn(home), false);
  fs.writeFileSync(path.join(home, 'runner.json'), '{"followOrigin":true}');
  assert.equal(followOriginOn(home), true);
  fs.writeFileSync(path.join(home, 'runner.json'), '{"followOrigin":"yes"}');
  assert.equal(followOriginOn(home), false);
  fs.writeFileSync(path.join(home, 'runner.json'), 'not json');
  assert.equal(followOriginOn(home), false);
});

test('tb land refuses only on a followOrigin machine, and only for the repo it follows', () => {
  const home = tmpDir('tb-origin-land-');
  const checkout = tmpDir('tb-origin-land-co-');
  git(checkout, 'init', '-q', '-b', 'main');
  commit(checkout, 'first');
  const elsewhere = tmpDir('tb-origin-land-other-');
  const release = (id: string, c?: string) => {
    fs.mkdirSync(path.join(home, 'runner', 'releases', id), { recursive: true });
    fs.writeFileSync(path.join(home, 'runner', 'releases', id, 'release.json'), JSON.stringify({ id, commit: 'abc', checkout: c }));
    fs.rmSync(path.join(home, 'runner', 'current'), { force: true });
    fs.symlinkSync(path.join('releases', id), path.join(home, 'runner', 'current'));
  };
  release('r1', checkout);
  assert.equal(landRefusal(home, checkout), undefined, 'the setting is off');
  fs.writeFileSync(path.join(home, 'runner.json'), '{"followOrigin":true}');
  assert.match(landRefusal(home, checkout) ?? '', /follows GitHub/);
  assert.match(landRefusal(home, checkout) ?? '', /runner: "main"/);
  assert.equal(landRefusal(home, elsewhere), undefined, "another repo's main is its own");
  // A release deployed from a linked worktree names that, and tb land the main one.
  const linked = path.join(tmpDir('tb-origin-land-wt-'), 'wt');
  git(checkout, 'worktree', 'add', '-q', '-b', 'side', linked);
  release('r2', linked);
  assert.match(landRefusal(home, checkout) ?? '', /follows GitHub/);
  release('r3', checkout);
  // The same folder through a link is still the followed checkout.
  const link = path.join(tmpDir('tb-origin-land-ln-'), 'link');
  fs.symlinkSync(checkout, link);
  assert.match(landRefusal(home, link) ?? '', /follows GitHub/);
});
