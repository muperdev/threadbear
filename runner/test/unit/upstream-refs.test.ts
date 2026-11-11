// upstream: stray refs, updates and concurrent checks (helpers/upstream-harness.ts says why it's several files).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { tmpDir } from '../helpers/fakes.ts';
import { FakeClock } from '../../src/clock.ts';
import { gitAsync } from '../../src/upstream.ts';
import { GIT_ENV, LATER, alive, commit, git, hangingUpstream, kept, objectsIn, release, signedTag, upstreamFor, world } from '../helpers/upstream-harness.ts';

// A runner from before this check fetched straight into the verified
// namespace, so a fork can have an unsigned tag sitting there.
test('an unsigned ref an older runner left among the releases is taken out, and a signed one takes its name', async () => {
  const w = world();
  git(w.mike, 'checkout', '-q', '-b', 'evil');
  const evil = commit(w.mike, 'evil.txt', 'Attacker code');
  git(w.mike, 'tag', '-a', '-m', 'unsigned', 'release/2026-11-02', evil);
  git(w.mike, 'push', '-q', 'origin', 'refs/tags/release/2026-11-02');
  git(w.mike, 'checkout', '-q', 'main');
  // What the old fetch did.
  git(w.theirs, 'fetch', '-q', '--no-tags', 'upstream', '+refs/tags/release/*:refs/threadbear/upstream/*');
  assert.equal(kept(w.theirs, '2026-11-02'), evil);
  const leftover = git(w.theirs, 'rev-parse', 'refs/threadbear/upstream/2026-11-02');
  git(w.mike, 'push', '-q', '-f', 'origin', ':refs/tags/release/2026-11-02');
  const s = await upstreamFor(w.theirs, w.home).check();
  assert.equal(s.release?.tag, 'release/2026-10-01');
  assert.equal(kept(w.theirs, '2026-11-02'), '', 'taken out, so nothing merges it by name');

  // Left over under a name upstream now has a real release for.
  git(w.theirs, 'update-ref', 'refs/threadbear/upstream/2026-11-03', leftover);
  const real = commit(w.mike, 'd.txt', 'Real');
  signedTag(w.mike, 'release/2026-11-03', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  const t = await upstreamFor(w.theirs, w.home).check();
  assert.equal(t.release?.commit, real);
  assert.equal(kept(w.theirs, '2026-11-03'), real);
});

test('with no ssh-keygen to ask, the check fails and keeps the releases it verified', async () => {
  const w = world();
  const up = upstreamFor(w.theirs, w.home);
  assert.equal((await up.check()).release?.tag, 'release/2026-10-01');
  const bin = path.join(w.root, 'bin');
  fs.mkdirSync(bin);
  fs.symlinkSync(spawnSync('/usr/bin/which', ['git'], { encoding: 'utf8' }).stdout.trim(), path.join(bin, 'git'));
  const before = process.env.PATH;
  process.env.PATH = bin;
  let s;
  try {
    // A runner just started: nothing it remembers.
    s = await upstreamFor(w.theirs, w.home).check();
  } finally {
    process.env.PATH = before;
  }
  assert.equal(s.state, 'failed');
  assert.match(s.reason ?? '', /ssh-keygen/);
  assert.ok(kept(w.theirs, '2026-10-01'), 'not taken out as unsigned');
});

// From Codex's review of the fix: what an older runner left among the
// releases is taken out whatever its name, before anything is copied in.
test('every unsigned ref an older runner left among the releases goes, even older ones and ones in the way of a name', async () => {
  const w = world();
  git(w.mike, 'checkout', '-q', '-b', 'evil');
  const evil = commit(w.mike, 'evil.txt', 'Attacker code');
  git(w.mike, 'tag', '-a', '-m', 'unsigned', 'release/2026-10-30', evil);
  git(w.mike, 'tag', '-a', '-m', 'unsigned', 'release/2026-11-06/poison', evil);
  git(w.mike, 'push', '-q', 'origin', 'refs/tags/release/2026-10-30', 'refs/tags/release/2026-11-06/poison');
  git(w.theirs, 'fetch', '-q', '--no-tags', 'upstream', '+refs/tags/release/*:refs/threadbear/upstream/*');
  git(w.mike, 'push', '-q', 'origin', ':refs/tags/release/2026-11-06/poison');
  git(w.mike, 'tag', '-d', 'release/2026-11-06/poison');
  git(w.mike, 'checkout', '-q', 'main');
  const real = commit(w.mike, 'd.txt', 'Real');
  signedTag(w.mike, 'release/2026-11-06', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', 'refs/tags/release/2026-11-06');
  const s = await upstreamFor(w.theirs, w.home).check();
  assert.equal(s.state, 'available', s.reason ?? '');
  assert.equal(s.release?.commit, real);
  assert.equal(kept(w.theirs, '2026-10-30'), '', 'an older unsigned one goes too');
  assert.equal(kept(w.theirs, '2026-11-06/poison'), '');
});

// Codex's review: upstream can push any number of tags, and each one made
// the runner read (and maybe ssh-keygen) it on every check.
test('a flood of tags upstream costs a check a few reads, once', async () => {
  const w = world();
  const tip = git(w.mike, 'rev-parse', 'HEAD');
  const fake = spawnSync('git', ['-C', w.mike, 'hash-object', '-t', 'tag', '-w', '--stdin'], { input: `object ${tip}\ntype commit\ntag release/2099-01-01\ntagger M <m@e> 1 +0000\n\nx\n-----BEGIN SSH SIGNATURE-----\nAAAA\n-----END SSH SIGNATURE-----\n`, encoding: 'utf8' }).stdout.trim();
  const lines: string[] = [];
  for (let i = 0; i < 150; i++) lines.push(`create refs/tags/release/junk-${i} ${tip}`);
  for (let i = 1; i <= 28; i++) lines.push(`create refs/tags/release/2099-02-${String(i).padStart(2, '0')} ${fake}`);
  for (let i = 1; i <= 28; i++) lines.push(`create refs/tags/release/2026-09-${String(i).padStart(2, '0')} ${fake}`);
  // Date-named, pointing straight at a commit: turned down without a read.
  for (let d = Date.parse('1990-01-01'); d < Date.parse('1991-01-01'); d += 86_400_000) lines.push(`create refs/tags/release/${new Date(d).toISOString().slice(0, 10)} ${tip}`);
  // Tag objects dated after Mike's newest: read, but only so many a check.
  for (let i = 7; i <= 31; i++) lines.push(`create refs/tags/release/2026-12-${String(i).padStart(2, '0')} ${fake}`);
  assert.equal(spawnSync('git', ['-C', w.mike, 'update-ref', '--stdin'], { input: lines.join('\n') + '\n' }).status, 0);
  const real = commit(w.mike, 'b.txt', 'Real');
  signedTag(w.mike, 'release/2026-11-06', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  let reads = 0;
  const { gitBytes } = await import('../../src/upstream.ts');
  const up = upstreamFor(w.theirs, w.home, new FakeClock(LATER), { gitBytes: (cwd: string, args: string[]) => (reads++, gitBytes(cwd, args)) });
  // 25 fakes dated after it: 20 a check, then the real one.
  let s = await up.check();
  assert.equal(reads, 20);
  assert.equal(s.release, undefined, 'not reached yet, nor the older one');
  reads = 0;
  s = await up.check();
  assert.equal(s.release?.commit, real);
  assert.ok(reads <= 8, `read ${reads} tags`);
  reads = 0;
  await up.check();
  assert.equal(reads, 0, 'nothing read again');
});

// Codex's second pass: a read that failed was kept as "not signed", and
// in the sweep took a good release out.
test('a read that fails is not taken for an unsigned tag, nor remembered', async () => {
  const w = world();
  const { gitBytes } = await import('../../src/upstream.ts');
  let fail = false;
  const up = upstreamFor(w.theirs, w.home, new FakeClock(LATER), { gitBytes: (cwd: string, args: string[]) => (fail ? Promise.reject(new Error('spawn git EAGAIN')) : gitBytes(cwd, args)) });
  const fresh = upstreamFor(w.theirs, w.home, new FakeClock(LATER), { gitBytes: (cwd: string, args: string[]) => (fail ? Promise.reject(new Error('spawn git EAGAIN')) : gitBytes(cwd, args)) });
  assert.equal((await up.check()).release?.tag, 'release/2026-10-01');
  fail = true;
  const s = await fresh.check();
  assert.equal(s.state, 'failed');
  assert.ok(kept(w.theirs, '2026-10-01'), 'the sweep left it');
  fail = false;
  assert.equal((await fresh.check()).release?.tag, 'release/2026-10-01');
});

test('an unsigned ref an older runner left goes even while upstream cannot be fetched', async () => {
  const w = world();
  git(w.mike, 'checkout', '-q', '-b', 'evil');
  const evil = commit(w.mike, 'evil.txt', 'Attacker code');
  git(w.mike, 'tag', '-a', '-m', 'unsigned', 'release/2026-11-06', evil);
  git(w.mike, 'push', '-q', 'origin', 'refs/tags/release/2026-11-06');
  git(w.theirs, 'fetch', '-q', '--no-tags', 'upstream', '+refs/tags/release/*:refs/threadbear/upstream/*');
  git(w.theirs, 'remote', 'set-url', 'upstream', path.join(w.root, 'gone.git'));
  const s = await upstreamFor(w.theirs, w.home).check();
  assert.equal(s.state, 'failed');
  assert.equal(kept(w.theirs, '2026-11-06'), '');
});

// Codex's third pass: a tag past the read buffer failed every check.
test('a huge tag is turned down unread, and one an older runner left is taken out', async () => {
  const w = world();
  const big = path.join(w.root, 'big.txt');
  fs.writeFileSync(big, 'x'.repeat(17 * 1024 * 1024));
  git(w.mike, 'tag', '-a', '-F', big, 'release/2026-11-05');
  git(w.mike, 'push', '-q', 'origin', 'refs/tags/release/2026-11-05');
  git(w.theirs, 'fetch', '-q', '--no-tags', 'upstream', '+refs/tags/release/*:refs/threadbear/upstream/*');
  const real = commit(w.mike, 'd.txt', 'Real');
  signedTag(w.mike, 'release/2026-11-04', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', 'refs/tags/release/2026-11-04');
  let reads = 0;
  const { gitBytes } = await import('../../src/upstream.ts');
  const s = await upstreamFor(w.theirs, w.home, new FakeClock(LATER), { gitBytes: (cwd: string, args: string[]) => (reads++, gitBytes(cwd, args)) }).check();
  assert.equal(s.release?.commit, real, s.reason ?? '');
  assert.equal(kept(w.theirs, '2026-11-05'), '');
  assert.ok(reads <= 3);
});

test('tb updates and the bring-in thread name the verified commit to merge', async () => {
  const { formatUpstream } = await import('../../src/tb-dev.ts');
  const commitId = 'a'.repeat(40);
  const text = formatUpstream({ state: 'available', release: { tag: 'release/2026-10-30', commit: commitId, changes: 1, subjects: ['One'] } }, Date.now());
  assert.match(text, new RegExp(`git merge[^\\n]*${commitId}`));
});

test('tb updates: what to bring in, or that there is nothing', async () => {
  const { formatUpstream } = await import('../../src/tb-dev.ts');
  const now = Date.parse('2026-10-30T10:00:00Z');
  const checkedAt = '2026-10-30T09:00:00Z';
  const text = formatUpstream({ state: 'available', checkedAt, release: { tag: 'release/2026-10-30', commit: 'abc', changes: 3, subjects: ['Third', 'Second'] } }, now);
  assert.match(text, /^Mike's releases: release\/2026-10-30 has 3 changes this copy's main doesn't, checked 1h/);
  assert.match(text, /\n {2}Third\n {2}Second\n {2}… and 1 more\n/);
  assert.match(text, /bringing-in-updates\.md/);
  assert.match(formatUpstream({ state: 'idle', checkedAt, release: { tag: 'release/2026-10-30', commit: 'abc', changes: 0, subjects: [] } }, now), /^Mike's releases: up to date with release\/2026-10-30, checked/);
  assert.equal(formatUpstream({ state: 'off', reason: 'upstream is this repo', published: { tag: 'release/2026-10-30', at: checkedAt } }, now), "Mike's releases: off (upstream is this repo)\nLast release tagged here: release/2026-10-30");
});

test('upstream with thousands of tags: only a bounded few are fetched, and none that fail to verify stay', async () => {
  const w = world();
  assert.equal((await upstreamFor(w.theirs, w.home).check()).release?.tag, 'release/2026-10-01');
  const base = git(w.mike, 'rev-parse', 'HEAD');
  const tree = git(w.mike, 'rev-parse', `${base}^{tree}`);
  // Fakes with commits of their own, so any that came over would show.
  const fakes: string[] = [];
  const fake = (msg: string) => {
    const c = spawnSync('git', ['-C', w.mike, 'commit-tree', tree, '-p', base, '-m', msg], { encoding: 'utf8', env: GIT_ENV }).stdout.trim();
    fakes.push(c);
    return c;
  };
  const lines: string[] = [];
  // Thousands of names, most not dates, and dates years ahead.
  for (let i = 0; i < 3000; i++) lines.push(`create refs/tags/release/junk-${i} ${base}`);
  for (let d = Date.parse('2027-01-10'); d < Date.parse('2030-01-01'); d += 86_400_000) lines.push(`create refs/tags/release/${new Date(d).toISOString().slice(0, 10)} ${base}`);
  // Every date after the last release that isn't the real one: a name
  // straight at a commit of its own (never fetched)...
  for (let d = Date.parse('2026-10-02'); d <= Date.parse('2026-12-06'); d += 86_400_000) {
    const name = new Date(d).toISOString().slice(0, 10);
    if (name !== '2026-11-06') lines.push(`create refs/tags/release/${name} ${fake(name)}`);
  }
  assert.equal(spawnSync('git', ['-C', w.mike, 'update-ref', '--stdin'], { input: lines.join('\n') + '\n' }).status, 0);
  // ...or an unsigned tag, newer than the real one, on a commit of its own.
  for (let i = 7; i <= 31; i++) git(w.mike, 'tag', '-a', '-m', 'unsigned', `release/2026-12-${String(i).padStart(2, '0')}`, fake(`tag ${i}`));
  const real = commit(w.mike, 'b.txt', 'Real');
  signedTag(w.mike, 'release/2026-11-06', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  const before = objectsIn(w.theirs);
  const up = upstreamFor(w.theirs, w.home);
  // 25 unsigned tags newer than it: 20 a check, then the real one.
  let s = await up.check();
  assert.equal(s.release?.tag, 'release/2026-10-01');
  s = await up.check();
  assert.equal(s.release?.commit, real, s.reason ?? '');
  const after = objectsIn(w.theirs);
  assert.deepEqual(fakes.filter((f) => after.has(f)), [], "no unsigned tag's or name's commit is in the checkout");
  // What came in is the release and its history: a commit, a tree, a blob and the tag.
  assert.ok(after.size - before.size <= 4, `${after.size - before.size} objects came in`);
  assert.equal(git(w.theirs, 'for-each-ref', 'refs/threadbear/upstream-incoming/'), '');
  assert.deepEqual(fs.readdirSync(path.join(w.home, 'runner', 'upstream-fetch')), [], 'the staging folder is gone');
});

for (const when of ['list', 'fetch'] as const) {
  test(`an upstream that hangs ${when === 'list' ? 'listing its tags' : 'sending them'} fails the check by its deadline, and leaves nothing running or behind`, async () => {
    const w = world();
    const up = upstreamFor(w.theirs, w.home, new FakeClock(LATER), { fetchTimeoutMs: 1500 });
    assert.equal((await up.check()).release?.tag, 'release/2026-10-01');
    fs.rmSync(path.join(w.root, 'calls'), { force: true });
    commit(w.mike, 'b.txt', 'Newer');
    signedTag(w.mike, 'release/2026-11-06', w.mikeKey.priv);
    git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
    const pids = hangingUpstream(w, when);
    const before = objectsIn(w.theirs);
    const t0 = Date.now();
    const s = await up.check();
    assert.ok(Date.now() - t0 < 15_000, `took ${Date.now() - t0} ms`);
    assert.equal(s.state, 'failed');
    assert.match(s.reason ?? '', /timed out/);
    assert.equal(s.release?.tag, 'release/2026-10-01', 'keeps the release it knew of');
    await new Promise((r) => setTimeout(r, 200));
    for (const pid of pids()) assert.ok(!alive(pid), `pid ${pid} still running`);
    assert.deepEqual([...objectsIn(w.theirs)].filter((o) => !before.has(o)), []);
    assert.deepEqual(fs.readdirSync(path.join(w.home, 'runner', 'upstream-fetch')), []);
  });
}

test('gitAsync stops a git command at its deadline, and what it started', async () => {
  const dir = tmpDir('tb-git-deadline-');
  const pidFile = path.join(dir, 'pid');
  const t0 = Date.now();
  await assert.rejects(gitAsync(dir, ['-c', `alias.hang=!sleep 600 & echo $! >${pidFile}; wait`, 'hang'], { timeoutMs: 500 }), /timed out/);
  assert.ok(Date.now() - t0 < 5000);
  await new Promise((r) => setTimeout(r, 200));
  assert.ok(!alive(Number(fs.readFileSync(pidFile, 'utf8'))), 'the sleep it started is gone');
  assert.equal(await gitAsync(dir, ['--version']).then((v) => v.startsWith('git version')), true);
});

// Codex's review of the bounds.
test("fetch.prune and fetch.pruneTags in the fork's config change none of its refs", async () => {
  const w = world();
  git(w.theirs, 'tag', 'mine');
  git(w.theirs, 'config', 'fetch.prune', 'true');
  git(w.theirs, 'config', 'fetch.pruneTags', 'true');
  git(w.theirs, 'config', 'remote.upstream.prune', 'true');
  const real = commit(w.mike, 'b.txt', 'Real');
  signedTag(w.mike, 'release/2026-11-06', w.mikeKey.priv);
  git(w.mike, 'tag', '-a', '-m', 'unsigned', 'release/2026-11-07');
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  const refsBefore = git(w.theirs, 'for-each-ref', '--format=%(refname) %(objectname)', 'refs/tags', 'refs/heads', 'refs/remotes');
  const s = await upstreamFor(w.theirs, w.home).check();
  assert.equal(s.release?.commit, real, s.reason ?? '');
  assert.equal(git(w.theirs, 'for-each-ref', '--format=%(refname) %(objectname)', 'refs/tags', 'refs/heads', 'refs/remotes'), refsBefore);
  assert.equal(git(w.theirs, 'fsck', '--connectivity-only', '--no-dangling'), '');
});

test('two checks at once, as when a runner hands over to the next, each stage on their own', async () => {
  const w = world();
  const real = commit(w.mike, 'b.txt', 'Real');
  signedTag(w.mike, 'release/2026-11-06', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  const [a, b] = await Promise.all([upstreamFor(w.theirs, w.home).check(), upstreamFor(w.theirs, w.home).check()]);
  assert.equal(a.release?.commit, real, a.reason ?? '');
  assert.equal(b.release?.commit, real, b.reason ?? '');
  assert.deepEqual(fs.readdirSync(path.join(w.home, 'runner', 'upstream-fetch')), []);
});
