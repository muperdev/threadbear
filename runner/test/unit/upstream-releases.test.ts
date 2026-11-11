// upstream: finding and verifying releases (helpers/upstream-harness.ts says why it's several files).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { FakeClock } from '../../src/clock.ts';
import { type CiRun, bareRepoArgs, ciVerdict, FETCH_INTERVAL_MS, FIRST_LOOK_MS, SETTLE_MS, cleanNotes, parseImportant, parseNotes, repoKey, signersPath, splitSignedTag, tagFor } from '../../src/upstream.ts';
import { LATER, asGitHub, commit, fakeGpg, git, kept, key, release, signedTag, upstreamFor, waitFor, withVictimGpg, world } from '../helpers/upstream-harness.ts';

test('repoKey: the same repo however its URL is written', () => {
  const k = 'github.com/mikecann/threadbear';
  for (const u of ['https://github.com/mikecann/threadbear.git', 'https://github.com/mikecann/threadbear', 'git@github.com:mikecann/threadbear.git', 'ssh://git@github.com/mikecann/threadbear', 'https://x-access-token@github.com/MikeCann/threadbear/']) {
    assert.equal(repoKey(u), k, u);
  }
  assert.notEqual(repoKey('https://github.com/someone/threadbear.git'), k);
});

test('tagFor: release/ and the local date', () => {
  assert.equal(tagFor(new Date(2026, 9, 5, 23, 59)), 'release/2026-10-05');
});

test('a fork sees a newer signed release from Mike, with what main is missing', async () => {
  const w = world();
  const up = upstreamFor(w.theirs, w.home);
  let s = await up.check();
  assert.equal(s.state, 'idle', s.reason ?? '');
  assert.equal(s.release?.tag, 'release/2026-10-01');
  assert.equal(s.release?.changes, 0);

  commit(w.mike, 'b.txt', 'Second');
  const third = commit(w.mike, 'c.txt', 'Third');
  signedTag(w.mike, 'release/2026-10-30', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  s = await up.check();
  assert.equal(s.state, 'available');
  assert.equal(s.release?.tag, 'release/2026-10-30');
  assert.equal(s.release?.commit, third);
  assert.equal(s.release?.changes, 2);
  assert.deepEqual(s.release?.subjects, ['Third', 'Second']);
  assert.ok(s.checkedAt);
  // Their own tags are left alone: upstream's go in a namespace of their own.
  assert.equal(git(w.theirs, 'tag', '--list', 'release/2026-10-30'), '');

  // Once main has it, there's nothing to bring in.
  git(w.theirs, 'merge', '-q', '--no-edit', 'refs/threadbear/upstream/2026-10-30');
  s = await up.check();
  assert.equal(s.state, 'idle');
  assert.equal(s.release?.changes, 0);
});

// Claude's desktop terminal sets safe.bareRepository=explicit in
// GIT_CONFIG_PARAMETERS, and git then refuses a bare repo it finds by
// looking from inside it, as the check's staging folder was (a fork's
// commit, 8 Nov 2026: every check failed there).
test('the check works under safe.bareRepository=explicit, as in Claude\'s desktop terminal', async () => {
  const w = world();
  commit(w.mike, 'b.txt', 'Second');
  const third = commit(w.mike, 'c.txt', 'Third');
  signedTag(w.mike, 'release/2026-10-30', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  const before = process.env.GIT_CONFIG_PARAMETERS;
  process.env.GIT_CONFIG_PARAMETERS = "'safe.bareRepository'='explicit'";
  try {
    // The setting really is on: git refuses a bare repo found from inside.
    const bare = path.join(w.home, 'a-bare-repo');
    git(w.home, 'init', '-q', '--bare', bare);
    assert.notEqual(spawnSync('git', ['-C', bare, 'rev-parse', '--git-dir'], { encoding: 'utf8' }).status, 0);
    assert.deepEqual(bareRepoArgs(bare), ['--git-dir', path.resolve(bare)]);
    assert.deepEqual(bareRepoArgs(w.theirs), [], 'a checkout is found as usual');

    const s = await upstreamFor(w.theirs, w.home).check();
    assert.equal(s.state, 'available', s.reason ?? '');
    assert.equal(s.release?.tag, 'release/2026-10-30');
    assert.equal(s.release?.commit, third);
  } finally {
    if (before === undefined) delete process.env.GIT_CONFIG_PARAMETERS;
    else process.env.GIT_CONFIG_PARAMETERS = before;
  }
});

test('commits Mike marks Important: come with the release, and lead his tag message', async () => {
  const w = world();
  const seen: string[] = [];
  const up = upstreamFor(w.theirs, w.home, new FakeClock(LATER), { onChecked: (s: { state: string }) => seen.push(s.state) });
  commit(w.mike, 'b.txt', 'Ordinary');
  fs.writeFileSync(path.join(w.mike, 'c.txt'), 'x');
  git(w.mike, 'add', 'c.txt');
  git(w.mike, 'commit', '-q', '-m', 'Scrub secrets\n\nWhy it matters.\n\nImportant: keeps API keys out of Convex');
  const fix = git(w.mike, 'rev-parse', 'HEAD');
  signedTag(w.mike, 'release/2026-11-06', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  const s = await up.check();
  assert.deepEqual(s.release?.important, [{ commit: fix, why: 'keeps API keys out of Convex' }]);
  assert.deepEqual(seen, ['available']);
  assert.deepEqual(parseImportant(`abc\x1f\ndef\x1fa reason\n`), [{ commit: 'def', why: 'a reason' }]);
});

test('only tags signed with the pinned key, under their own name, count', async () => {
  const w = world();
  const stranger = key(w.root, 'stranger');
  commit(w.mike, 'b.txt', 'Signed by someone else');
  signedTag(w.mike, 'release/2026-10-30', stranger.priv);
  commit(w.mike, 'c.txt', 'Not signed at all');
  git(w.mike, 'tag', '-a', '-m', 'unsigned', 'release/2026-10-31');
  // A real signed tag for the 29th, pushed again as if it were the 1st.
  git(w.mike, 'update-ref', 'refs/tags/release/2026-11-01', git(w.mike, 'rev-parse', 'refs/tags/release/2026-10-01'));
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  const s = await upstreamFor(w.theirs, w.home).check();
  assert.equal(s.state, 'idle');
  assert.equal(s.release?.tag, 'release/2026-10-01');
});

test('off without an upstream remote, when upstream is this repo, or with no pinned key', async () => {
  const w = world();
  assert.match((await upstreamFor(w.mike, w.home).check()).reason ?? '', /no upstream remote/);
  git(w.mike, 'remote', 'add', 'upstream', git(w.mike, 'remote', 'get-url', 'origin'));
  assert.match((await upstreamFor(w.mike, w.home).check()).reason ?? '', /upstream is this repo/);
  fs.rmSync(signersPath(w.home));
  const s = await upstreamFor(w.theirs, w.home).check();
  assert.equal(s.state, 'off');
  assert.match(s.reason ?? '', /no release key pinned/);
});

test('a fetch that fails says so and keeps the release it knew of', async () => {
  const w = world();
  commit(w.mike, 'b.txt', 'Second');
  signedTag(w.mike, 'release/2026-10-30', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  const up = upstreamFor(w.theirs, w.home);
  assert.equal((await up.check()).state, 'available');
  git(w.theirs, 'remote', 'set-url', 'upstream', path.join(w.root, 'gone.git'));
  const s = await up.check();
  assert.equal(s.state, 'failed');
  assert.equal(s.release?.tag, 'release/2026-10-30');
});

test('the timer looks a while after start, then every six hours', async () => {
  const w = world();
  const clock = new FakeClock();
  let changes = 0;
  const up = upstreamFor(w.theirs, w.home, clock, { onChange: () => changes++ });
  up.start();
  assert.equal(up.status().state, 'off');
  clock.advance(FIRST_LOOK_MS);
  await waitFor(() => up.status().state === 'idle');
  assert.ok(changes >= 1);
  assert.equal(clock.pending(), 1);
  up.stop();
  assert.equal(clock.pending(), 0);
  void FETCH_INTERVAL_MS;
});

test("Mike's runner tags main once a day, signed, once it has settled", async () => {
  const w = world();
  const tip = commit(w.mike, 'b.txt', 'Newer work');
  git(w.mike, 'push', '-q', 'origin', 'main');
  const committed = Number(git(w.mike, 'log', '-1', '--format=%ct')) * 1000;
  let settled = false;
  const clock = new FakeClock(committed + SETTLE_MS / 2);
  const up = upstreamFor(w.mike, w.home, clock, { publish: { publish: true, signingKey: w.mikeKey.priv }, runningMainSettled: () => settled });

  assert.equal(await up.publishOnce(), undefined, 'the runner has not settled');
  settled = true;
  assert.equal(await up.publishOnce(), undefined, "main's tip is too new");
  clock.advance(SETTLE_MS);
  const tag = await up.publishOnce();
  assert.equal(tag, tagFor(new Date(clock.now())));
  assert.equal(git(w.mike, 'rev-parse', `${tag}^{commit}`), tip);
  assert.match(git(w.mike, 'cat-file', 'tag', tag!), /- Newer work/);
  // Pushed, and signed so that a fork's runner takes it.
  assert.ok(git(w.mike, 'ls-remote', 'origin', `refs/tags/${tag}`));
  assert.equal((await upstreamFor(w.theirs, w.home, clock).check()).release?.tag, tag);
  assert.equal(up.status().published?.tag, tag);

  // Not again the same day, and not on a later day with nothing new.
  assert.equal(await up.publishOnce(), undefined);
  clock.advance(24 * 60 * 60_000);
  assert.equal(await up.publishOnce(), undefined);
});

test("release notes: Mike's runner writes them into the tag, and a fork reads each release's", async () => {
  const w = world();
  const prompts: string[] = [];
  let reply: string | undefined = '- Threads can be pinned\n- Search is quicker';
  let failPush = true;
  commit(w.mike, 'b.txt', 'Pinning: a pin button on rows\n\nBecause Mike asked.');
  git(w.mike, 'push', '-q', 'origin', 'main');
  const committed = Number(git(w.mike, 'log', '-1', '--format=%ct')) * 1000;
  const clock = new FakeClock(committed + 2 * SETTLE_MS);
  const up = upstreamFor(w.mike, w.home, clock, {
    publish: { publish: true, signingKey: w.mikeKey.priv }, runningMainSettled: () => true,
    writeNotes: async (commits: string) => { prompts.push(commits); if (reply === 'throw') throw new Error('no room'); return reply; },
    git: async (cwd: string, args: string[]) => {
      if (args[0] === 'push' && args.some((a) => a.startsWith('refs/tags/')) && failPush) { failPush = false; throw new Error('push offline'); }
      return git(cwd, ...args);
    },
  });
  await assert.rejects(up.publishOnce(), /push offline/);
  const first = await up.publishOnce();
  assert.equal(prompts.length, 1, 'a failed push reuses the draft for the same commits');
  assert.match(prompts[0], /Pinning: a pin button on rows\nBecause Mike asked\./);
  const message = git(w.mike, 'cat-file', 'tag', first!);
  assert.match(message, /What's new:\n- Threads can be pinned\n- Search is quicker\n\nEvery change:\n- Pinning: a pin button on rows/);

  // The next day, the model fails: the release goes out without notes.
  commit(w.mike, 'c.txt', 'Tidy the tests');
  git(w.mike, 'push', '-q', 'origin', 'main');
  clock.advance(24 * 60 * 60_000 + 2 * SETTLE_MS);
  reply = 'throw';
  const second = await up.publishOnce();
  assert.ok(second && second !== first);
  assert.doesNotMatch(git(w.mike, 'cat-file', 'tag', second), /What's new:/);

  // A fork behind both sees the first's notes (the second has none).
  const s = await upstreamFor(w.theirs, w.home, clock).check();
  assert.equal(s.release?.tag, second);
  assert.deepEqual(s.release?.notes, [{ tag: second, lines: ['Tidy the tests'] }, { tag: first, lines: ['Threads can be pinned', 'Search is quicker'] }]);
  // Once main has the first, only what's newer counts.
  git(w.theirs, 'merge', '-q', '--no-edit', git(w.mike, 'rev-parse', `${first}^{commit}`));
  assert.deepEqual((await upstreamFor(w.theirs, w.home, clock).check()).release?.notes, [{ tag: second, lines: ['Tidy the tests'] }]);
});

test('release notes exclude signed tags outside the offered history and unsigned notes', async () => {
  const w = world();
  commit(w.mike, 'main.txt', 'Main work');
  const newest = git(w.mike, 'rev-parse', 'HEAD');
  git(w.mike, 'checkout', '-q', '-b', 'side', 'HEAD~1');
  commit(w.mike, 'side.txt', 'Unmerged side work');
  git(w.mike, '-c', 'gpg.format=ssh', '-c', `user.signingkey=${w.mikeKey.priv}`, 'tag', '-s', '-m', "What's new:\n- Not in the offered release", 'release/2026-11-01');
  git(w.mike, 'tag', '-a', '-m', "What's new:\n- Unsigned notes", 'release/2026-11-02');
  git(w.mike, 'checkout', '-q', 'main');
  git(w.mike, '-c', 'gpg.format=ssh', '-c', `user.signingkey=${w.mikeKey.priv}`, 'tag', '-s', '-m', "What's new:\n- Main work", 'release/2026-11-03', newest);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  const s = await upstreamFor(w.theirs, w.home).check();
  assert.deepEqual(s.release?.notes, [{ tag: 'release/2026-11-03', lines: ['Main work'] }]);
});

test('parseNotes and cleanNotes', () => {
  assert.deepEqual(parseNotes("Threadbear 2026-11-06\n\nImportant: x\n\nWhat's new:\n- One\n- Two\n\nEvery change:\n- a"), ['One', 'Two']);
  assert.deepEqual(parseNotes('Threadbear 2026-11-01\n\n- an old one with only subjects'), []);
  assert.deepEqual(cleanNotes('Here you go:\n- One — and more\n* Two\nnot a line'), ['One, and more', 'Two']);
  assert.deepEqual(cleanNotes(' NONE '), []);
  assert.deepEqual(cleanNotes(undefined), []);
  assert.deepEqual(cleanNotes('- ' + 'x'.repeat(1000)), ['x'.repeat(200)]);
});

test("ciVerdict: every workflow's latest attempt has to pass", () => {
  const run = (workflow: string, status: string, conclusion: string | null, attempt = 1): CiRun => ({ workflow, name: `wf ${workflow}`, status, conclusion, attempt });
  assert.equal(ciVerdict([run('1', 'completed', 'success'), run('2', 'completed', 'skipped')]), undefined);
  assert.match(ciVerdict([])!, /hasn't run/);
  assert.match(ciVerdict([run('1', 'completed', 'success'), run('2', 'in_progress', null)])!, /still running/);
  assert.equal(ciVerdict([run('1', 'completed', 'failure'), run('2', 'completed', 'cancelled')]), '"wf 1", "wf 2" didn\'t pass on it');
  // A red run re-run green passes; a green one re-run red doesn't.
  assert.equal(ciVerdict([run('1', 'completed', 'failure', 1), run('1', 'completed', 'success', 2)]), undefined);
  assert.match(ciVerdict([run('1', 'completed', 'success', 1), run('1', 'completed', 'failure', 2)])!, /didn't pass/);
});

test("Mike's runner holds a release until GitHub's checks pass on that commit", async () => {
  const w = world();
  asGitHub(w.mike, w.root);
  fs.mkdirSync(path.join(w.mike, '.github', 'workflows'), { recursive: true });
  const tip = commit(w.mike, '.github/workflows/build.yml', 'Build on GitHub');
  git(w.mike, 'push', '-q', 'origin', 'main');
  const committed = Number(git(w.mike, 'log', '-1', '--format=%ct')) * 1000;
  const clock = new FakeClock(committed + 2 * SETTLE_MS);
  let runs: CiRun[] | Error = [{ workflow: '7', name: 'Builds on Xcode 26', status: 'completed', conclusion: 'failure', attempt: 1 }];
  const asked: string[] = [];
  let notesCalls = 0;
  const up = upstreamFor(w.mike, w.home, clock, {
    publish: { publish: true, signingKey: w.mikeKey.priv },
    runningMainSettled: () => true,
    writeNotes: async () => { notesCalls++; return "- Checks keep releases buildable"; },
    ciRuns: async (repo: string, sha: string) => {
      asked.push(`${repo} ${sha}`);
      if (runs instanceof Error) throw runs;
      return runs;
    },
  });

  assert.equal(await up.publishOnce(), undefined, 'a red build holds it');
  assert.deepEqual(asked, [`mikecann/threadbear ${tip}`]);
  assert.equal(up.status().held?.commit, tip);
  assert.match(up.status().held!.why, /"Builds on Xcode 26" didn't pass/);
  assert.equal(git(w.mike, 'tag', '--list', 'release/2026-12*'), '');
  // A check of upstream in between leaves it saying so.
  await up.check();
  assert.ok(up.status().held);

  runs = new Error('offline');
  assert.equal(await up.publishOnce(), undefined, "checks it can't read hold it too");
  assert.match(up.status().held!.why, /couldn't read GitHub's checks/);

  assert.equal(notesCalls, 0, 'notes generation waits for the exact commit checks too');
  runs = [{ workflow: '7', name: 'Builds on Xcode 26', status: 'completed', conclusion: 'success', attempt: 2 }];
  const tag = await up.publishOnce();
  assert.equal(tag, tagFor(new Date(clock.now())));
  assert.equal(git(w.mike, 'rev-parse', `${tag}^{commit}`), tip);
  assert.equal(up.status().held, undefined);
  assert.equal(up.status().published?.tag, tag);
  assert.equal(notesCalls, 1);
  assert.match(git(w.mike, "cat-file", "tag", tag!), /What's new:\n- Checks keep releases buildable/);
});

test("Mike's runner pushes a settled main GitHub doesn't have, then waits for its checks", async () => {
  const w = world();
  asGitHub(w.mike, w.root);
  fs.mkdirSync(path.join(w.mike, '.github', 'workflows'), { recursive: true });
  const tip = commit(w.mike, '.github/workflows/build.yml', 'Build on GitHub');
  // Landed here, never pushed.
  assert.notEqual(git(w.mike, 'ls-remote', 'origin', 'refs/heads/main').split('\t')[0], tip);
  const committed = Number(git(w.mike, 'log', '-1', '--format=%ct')) * 1000;
  const up = upstreamFor(w.mike, w.home, new FakeClock(committed + 2 * SETTLE_MS), { publish: { publish: true, signingKey: w.mikeKey.priv }, runningMainSettled: () => true, ciRuns: async () => assert.fail('not asked yet') });
  assert.equal(await up.publishOnce(), undefined);
  assert.equal(git(w.mike, 'ls-remote', 'origin', 'refs/heads/main').split('\t')[0], tip, 'pushed');
  assert.match(up.status().held!.why, /just pushed main/);

  // A main that's gone a different way from GitHub's isn't pushed over it.
  const w2 = world();
  asGitHub(w2.mike, w2.root);
  commit(w2.mike, 'x.txt', 'Mine');
  const other = path.join(w2.root, 'other');
  git(w2.root, 'clone', '-q', path.join(w2.root, 'mike.git'), other);
  commit(other, 'y.txt', 'Theirs');
  git(other, 'push', '-q', 'origin', 'HEAD:main');
  fs.mkdirSync(path.join(w2.mike, '.github', 'workflows'), { recursive: true });
  commit(w2.mike, '.github/workflows/build.yml', 'Build on GitHub');
  const at = Number(git(w2.mike, 'log', '-1', '--format=%ct')) * 1000;
  const up2 = upstreamFor(w2.mike, w2.home, new FakeClock(at + 2 * SETTLE_MS), { publish: { publish: true, signingKey: w2.mikeKey.priv }, runningMainSettled: () => true });
  assert.equal(await up2.publishOnce(), undefined);
  assert.match(up2.status().held!.why, /different ways/);
  assert.match(git(other, 'log', '-1', '--format=%s', 'origin/main'), /Theirs/);
});

test("a main whose origin isn't GitHub can't show its checks, so it waits", async () => {
  const w = world();
  fs.mkdirSync(path.join(w.mike, '.github', 'workflows'), { recursive: true });
  commit(w.mike, '.github/workflows/build.yml', 'Build on GitHub');
  git(w.mike, 'push', '-q', 'origin', 'main');
  const committed = Number(git(w.mike, 'log', '-1', '--format=%ct')) * 1000;
  const up = upstreamFor(w.mike, w.home, new FakeClock(committed + 2 * SETTLE_MS), { publish: { publish: true, signingKey: w.mikeKey.priv }, runningMainSettled: () => true, ciRuns: async () => assert.fail('not asked') });
  assert.equal(await up.publishOnce(), undefined);
  assert.match(up.status().held!.why, /isn't on GitHub/);
});

test('publishing needs a signing key', async () => {
  const w = world();
  const up = upstreamFor(w.mike, w.home, new FakeClock(Date.now() + 2 * SETTLE_MS), { publish: { publish: true, signingKey: path.join(w.root, 'missing') }, runningMainSettled: () => true });
  assert.equal(await up.publishOnce(), undefined);
});

// Gautam's scan, 6 Nov 2026, F5: verify-tag with gpg.format=ssh still hands
// a PGP or X.509 signature to gpg and the whole keyring, so an attacker who
// controls upstream got a release past the pinned key with their own key.
test('a release tag signed with OpenPGP or X.509 never counts, whatever gpg says about it', async () => {
  const w = world();
  const pgp = fakeGpg(w.root, 'PGP SIGNATURE');
  const x509 = fakeGpg(w.root, 'SIGNED MESSAGE');
  commit(w.mike, 'evil.txt', 'Attacker code');
  git(w.mike, '-c', 'gpg.format=openpgp', '-c', `gpg.program=${pgp}`, '-c', 'user.signingkey=MALLORY', 'tag', '-s', '-m', 'Threadbear 2026-12-01', 'release/2026-12-01');
  git(w.mike, '-c', 'gpg.format=x509', '-c', `gpg.x509.program=${x509}`, '-c', 'user.signingkey=MALLORY', 'tag', '-s', '-m', 'Threadbear 2026-12-02', 'release/2026-12-02');
  assert.match(git(w.mike, 'cat-file', 'tag', 'release/2026-12-01'), /-----BEGIN PGP SIGNATURE-----/);
  assert.match(git(w.mike, 'cat-file', 'tag', 'release/2026-12-02'), /-----BEGIN SIGNED MESSAGE-----/);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  // The victim's gpg would call both good.
  const s = await withVictimGpg({ 'gpg.program': pgp, 'gpg.openpgp.program': pgp, 'gpg.x509.program': x509 }, async () => {
    assert.equal(spawnSync('git', ['-C', w.mike, 'verify-tag', 'release/2026-12-01'], { env: process.env }).status, 0, 'gpg calls the PGP tag good');
    return upstreamFor(w.theirs, w.home).check();
  });
  assert.equal(s.release?.tag, 'release/2026-10-01');
  assert.equal(s.state, 'idle');
  assert.equal(spawnSync('git', ['-C', w.theirs, 'rev-parse', '-q', '--verify', 'refs/threadbear/upstream/2026-12-01']).status, 1, 'never copied where a bring-in merges from');
});

test('splitSignedTag: only one SSH signature block, at the end, under the four headers git writes', () => {
  const w = world();
  const raw = spawnSync('git', ['-C', w.mike, 'cat-file', 'tag', 'release/2026-10-01']).stdout as Buffer;
  const text = raw.toString('latin1');
  const ok = splitSignedTag(raw, 'release/2026-10-01');
  assert.ok(ok, 'a tag as Mike signs it');
  assert.equal(ok.commit, git(w.mike, 'rev-parse', 'release/2026-10-01^{commit}'));
  assert.ok(ok.date);
  assert.equal(ok.payload.length + Buffer.byteLength(ok.signature, 'latin1'), raw.length, 'payload and signature are the whole object');
  const sig = text.slice(text.indexOf('-----BEGIN SSH'));
  const pgp = '-----BEGIN PGP SIGNATURE-----\n\nAAAA\n-----END PGP SIGNATURE-----\n';
  const bad: [string, string][] = [
    ['another name', text],
    ['a PGP block after Mike\'s', text + pgp],
    ['a PGP block before it', text.replace(sig, pgp + sig)],
    ['an X.509 block in the message', text.replace(sig, '-----BEGIN SIGNED MESSAGE-----\nAAAA\n-----END SIGNED MESSAGE-----\n' + sig)],
    ['a second SSH block', text + sig],
    ['no signature', text.slice(0, text.indexOf('-----BEGIN SSH'))],
    ['a header added', text.replace('\ntagger ', '\ngpgsig-sha256 x\ntagger ')],
    ['a tag of a tree', text.replace('type commit', 'type tree')],
    ['text after the signature', text + 'more\n'],
  ];
  for (const [what, t] of bad) assert.equal(splitSignedTag(Buffer.from(t, 'latin1'), what === 'another name' ? 'release/2026-10-02' : 'release/2026-10-01'), undefined, what);
  // Armor mentioned in a commit subject (publishOnce starts each with "- ")
  // isn't a block, so it doesn't stop a release.
  const subject = Buffer.from(text.replace(sig, '- Reject -----BEGIN PGP SIGNATURE----- tags\n' + sig), 'latin1');
  assert.ok(splitSignedTag(subject, 'release/2026-10-01'));
});

// F31: the fetch force-moved refs/threadbear/upstream/<date>, the ref a
// bring-in thread merges, to whatever upstream served, before any check.
test('a release swapped upstream after it verified never moves the ref a bring-in merges', async () => {
  const w = world();
  const good = commit(w.mike, 'b.txt', 'Second');
  signedTag(w.mike, 'release/2026-10-30', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  const up = upstreamFor(w.theirs, w.home);
  assert.equal((await up.check()).release?.commit, good);

  // The attacker force-pushes the same name, unsigned, at their own commit.
  git(w.mike, 'checkout', '-q', '-b', 'evil');
  const evil = commit(w.mike, 'evil.txt', 'Attacker code');
  git(w.mike, 'tag', '-f', '-a', '-m', 'Threadbear 2026-10-30', 'release/2026-10-30', evil);
  git(w.mike, 'push', '-q', '-f', 'origin', 'refs/tags/release/2026-10-30');
  const s = await up.check();
  assert.equal(kept(w.theirs, '2026-10-30'), good, 'the verified ref keeps the commit that verified');
  assert.equal(s.release?.tag, 'release/2026-10-30');
  assert.equal(s.release?.commit, good);
  // What upstream serves now never reached the checkout: it was staged
  // outside it, didn't verify, and went (Daybreak, 7 Nov 2026).
  assert.equal(git(w.theirs, 'for-each-ref', 'refs/threadbear/upstream-incoming/'), '');
  assert.notEqual(spawnSync('git', ['-C', w.theirs, 'cat-file', '-e', evil]).status, 0, 'the unsigned tag\'s commit is not in the checkout');
});

test('a release name that verified keeps its first tag even if a signed one replaces it upstream', async () => {
  const w = world();
  const first = commit(w.mike, 'b.txt', 'Second');
  signedTag(w.mike, 'release/2026-10-30', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  const up = upstreamFor(w.theirs, w.home);
  await up.check();
  const other = commit(w.mike, 'c.txt', 'Third');
  git(w.mike, '-c', 'gpg.format=ssh', '-c', `user.signingkey=${w.mikeKey.priv}`, 'tag', '-f', '-s', '-m', 'again', 'release/2026-10-30', other);
  git(w.mike, 'push', '-q', '-f', 'origin', 'main', 'refs/tags/release/2026-10-30');
  const s = await up.check();
  assert.equal(kept(w.theirs, '2026-10-30'), first);
  assert.equal(s.release?.commit, first);
});
