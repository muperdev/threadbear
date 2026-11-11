// upstream: what is too big to fetch, and release keys (helpers/upstream-harness.ts says why it's several files).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { tmpDir } from '../helpers/fakes.ts';
import { FakeClock } from '../../src/clock.ts';
import { type GitOptions, MAC_SK_PROVIDER, TOUCH_SIGN_TIMEOUT_MS, touchKey, gitAsync, SETTLE_MS, signersPath } from '../../src/upstream.ts';
import { ADD_DELAY_MS, keyStatePath, revokedPath } from '../../src/release-keys.ts';
import { LATER, alive, commit, git, kept, key, keyLine, objectsIn, pinnedKeys, pubOf, release, signedTag, upstreamFor, world } from '../helpers/upstream-harness.ts';

test('a fetch bigger than the limit is stopped, and brings nothing into the checkout', async () => {
  const w = world();
  assert.equal((await upstreamFor(w.theirs, w.home).check()).release?.tag, 'release/2026-10-01');
  git(w.mike, 'checkout', '-q', '-b', 'big');
  fs.writeFileSync(path.join(w.mike, 'big.bin'), crypto.randomBytes(3 * 1024 * 1024));
  git(w.mike, 'add', 'big.bin');
  git(w.mike, 'commit', '-q', '-m', 'big');
  git(w.mike, 'tag', '-a', '-m', 'unsigned', 'release/2026-11-07');
  git(w.mike, 'push', '-q', 'origin', 'refs/tags/release/2026-11-07');
  const before = objectsIn(w.theirs);
  const s = await upstreamFor(w.theirs, w.home, new FakeClock(LATER), { stageMaxBytes: 1024 * 1024 }).check();
  assert.equal(s.state, 'failed');
  assert.match(s.reason ?? '', /more than 1 MB/);
  assert.equal(s.release, undefined);
  assert.deepEqual([...objectsIn(w.theirs)].filter((o) => !before.has(o)), []);
  assert.deepEqual(fs.readdirSync(path.join(w.home, 'runner', 'upstream-fetch')), []);
});

test("tags too big to read are remembered, so they can't hold a real release out", async () => {
  const w = world();
  const big = path.join(w.root, 'big.txt');
  fs.writeFileSync(big, 'x'.repeat(1024 * 1024 + 10));
  for (let i = 7; i <= 27; i++) git(w.mike, 'tag', '-a', '-F', big, `release/2026-12-${String(i).padStart(2, '0')}`);
  const real = commit(w.mike, 'b.txt', 'Real');
  signedTag(w.mike, 'release/2026-11-06', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  const up = upstreamFor(w.theirs, w.home);
  let s = await up.check();
  for (let i = 0; i < 2 && s.release?.commit !== real; i++) s = await up.check();
  assert.equal(s.release?.commit, real, s.reason ?? '');
});

test('a server that lists commits as tags (forged peeled lines) still lets a real release through', async () => {
  const w = world();
  const base = git(w.mike, 'rev-parse', 'HEAD');
  const lines: string[] = [];
  for (let i = 7; i <= 31; i++) lines.push(`create refs/tags/release/2026-12-${String(i).padStart(2, '0')} ${base}`);
  assert.equal(spawnSync('git', ['-C', w.mike, 'update-ref', '--stdin'], { input: lines.join('\n') + '\n' }).status, 0);
  const real = commit(w.mike, 'b.txt', 'Real');
  signedTag(w.mike, 'release/2026-11-06', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'main', '--tags');
  // ls-remote as such a server would answer: a peeled line for every name.
  const forge = async (cwd: string, args: string[], opts?: Parameters<typeof gitAsync>[2]) => {
    const out = await gitAsync(cwd, args, opts);
    if (args[0] !== 'ls-remote') return out;
    return out.split('\n').flatMap((l) => (l.endsWith('^{}') || !l ? [l] : [l, `${l.split('\t')[0]}\t${l.split('\t')[1]}^{}`])).join('\n');
  };
  const up = upstreamFor(w.theirs, w.home, new FakeClock(LATER), { git: forge });
  let s = await up.check();
  for (let i = 0; i < 2 && s.release?.commit !== real; i++) s = await up.check();
  assert.equal(s.release?.commit, real, s.reason ?? '');
});

test("gitAsync's watcher stops a command it says is over its limit, and what it started", async () => {
  const dir = tmpDir('tb-git-watch-');
  const pidFile = path.join(dir, 'pid');
  let asked = 0;
  await assert.rejects(
    gitAsync(dir, ['-c', `alias.hang=!sleep 600 & echo $! >${pidFile}; wait`, 'hang'], { timeoutMs: 60_000, over: () => (++asked >= 2 ? 'fetched more than 1 MB' : undefined) }),
    /fetched more than 1 MB/,
  );
  await new Promise((r) => setTimeout(r, 200));
  assert.ok(!alive(Number(fs.readFileSync(pidFile, 'utf8'))));
});

// Security review finding 3 (c), 9 Nov 2026: a key used to sign a tag on a
// commit that never went through main gets nowhere.
test("a signed release whose commit isn't on upstream's main doesn't count, until main has it", async () => {
  const w = world();
  const up = upstreamFor(w.theirs, w.home);
  assert.equal((await up.check()).release?.tag, 'release/2026-10-01');
  git(w.mike, 'checkout', '-q', '-b', 'side');
  const side = commit(w.mike, 'side.txt', 'Never reviewed');
  signedTag(w.mike, 'release/2026-10-30', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'refs/tags/release/2026-10-30');
  let s = await up.check();
  assert.equal(s.release?.tag, 'release/2026-10-01', 'the signed tag off main is left out');
  assert.equal(s.state, 'idle');
  assert.equal(git(w.theirs, 'for-each-ref', 'refs/threadbear/upstream/2026-10-30'), '', 'and never kept');
  // Once main has it, the same tag counts.
  git(w.mike, 'checkout', '-q', 'main');
  git(w.mike, 'merge', '-q', '--ff-only', 'side');
  git(w.mike, 'push', '-q', 'origin', 'main');
  s = await up.check();
  assert.equal(s.release?.tag, 'release/2026-10-30');
  assert.equal(s.release?.commit, side);
});

test('a new release key announced by a release the old key signed counts after a week, then signs releases', async () => {
  const w = world();
  const clock = new FakeClock(LATER);
  const up = upstreamFor(w.theirs, w.home, clock);
  const next = key(w.root, 'next-key');
  release(w, 'release/2026-10-30', w.mikeKey.priv, { 'scripts/release-signers': `${keyLine(w.mikeKey)}\n${keyLine(next)}\n` });
  let s = await up.check();
  assert.equal(s.release?.tag, 'release/2026-10-30');
  assert.deepEqual(pinnedKeys(w.home), [pubOf(w.mikeKey)], 'not straight away');
  assert.ok(fs.readFileSync(keyStatePath(w.home), 'utf8').includes(pubOf(next).split(' ')[1]!), 'waiting');
  // Signed with the new key inside the week: not yet.
  release(w, 'release/2026-11-01', next.priv);
  s = await up.check();
  assert.equal(s.release?.tag, 'release/2026-10-30');
  clock.advance(ADD_DELAY_MS + 60_000);
  await up.check();
  assert.deepEqual(pinnedKeys(w.home), [pubOf(w.mikeKey), pubOf(next)]);
  s = await up.check();
  assert.equal(s.release?.tag, 'release/2026-11-01', 'the new key signs releases now');
});

test('a revoked release key is refused from then on, and only it or a newer key can revoke it', async () => {
  const w = world();
  const clock = new FakeClock(LATER);
  const up = upstreamFor(w.theirs, w.home, clock);
  const next = key(w.root, 'next-key');
  // Both pinned, the old one first (as after a rotation's week).
  fs.writeFileSync(signersPath(w.home), `${keyLine(w.mikeKey)}\n${keyLine(next)}\n`);
  // The old key can't revoke the newer one.
  release(w, 'release/2026-10-30', w.mikeKey.priv, { 'scripts/release-keys-revoked': `${pubOf(next)}\n` });
  assert.equal((await up.check()).release?.tag, 'release/2026-10-30');
  assert.deepEqual(pinnedKeys(w.home), [pubOf(w.mikeKey), pubOf(next)]);
  // The newer key revokes the old one.
  release(w, 'release/2026-11-01', next.priv, { 'scripts/release-keys-revoked': `${pubOf(w.mikeKey)}\n` });
  assert.equal((await up.check()).release?.tag, 'release/2026-11-01');
  assert.deepEqual(pinnedKeys(w.home), [pubOf(next)]);
  assert.ok(fs.readFileSync(revokedPath(w.home), 'utf8').includes(pubOf(w.mikeKey)));
  // A release the old key signs no longer counts, even put back in the
  // pinned file by hand: ssh-keygen is handed the revoked list too.
  release(w, 'release/2026-11-02', w.mikeKey.priv);
  assert.equal((await up.check()).release?.tag, 'release/2026-11-01');
  fs.writeFileSync(signersPath(w.home), `${keyLine(next)}\n${keyLine(w.mikeKey)}\n`);
  assert.equal((await up.check()).release?.tag, 'release/2026-11-01');
  // And a later announcement can't bring it back.
  fs.writeFileSync(signersPath(w.home), `${keyLine(next)}\n`);
  release(w, 'release/2026-11-03', next.priv, { 'scripts/release-signers': `${keyLine(next)}\n${keyLine(w.mikeKey)}\n`, 'scripts/release-keys-revoked': '' });
  await up.check();
  clock.advance(ADD_DELAY_MS + 60_000);
  await up.check();
  assert.deepEqual(pinnedKeys(w.home), [pubOf(next)]);
});

test('a key announced by a key that is then revoked never counts', async () => {
  const w = world();
  const clock = new FakeClock(LATER);
  const up = upstreamFor(w.theirs, w.home, clock);
  const next = key(w.root, 'next-key');
  const thief = key(w.root, 'thief-key');
  fs.writeFileSync(signersPath(w.home), `${keyLine(w.mikeKey)}\n${keyLine(next)}\n`);
  // Someone with the old key announces one of their own...
  release(w, 'release/2026-10-30', w.mikeKey.priv, { 'scripts/release-signers': `${keyLine(w.mikeKey)}\n${keyLine(next)}\n${keyLine(thief)}\n` });
  await up.check();
  // ...and inside the week Mike revokes the old key with the newer one.
  release(w, 'release/2026-11-01', next.priv, { 'scripts/release-signers': `${keyLine(next)}\n`, 'scripts/release-keys-revoked': `${pubOf(w.mikeKey)}\n` });
  await up.check();
  clock.advance(ADD_DELAY_MS + 60_000);
  await up.check();
  assert.deepEqual(pinnedKeys(w.home), [pubOf(next)]);
  release(w, 'release/2026-11-02', thief.priv);
  assert.equal((await up.check()).release?.tag, 'release/2026-11-01');
});

// Finding 3 (b): a Secure Enclave key asks for Touch ID on every signature.
// The hourly look tries only while Mike is at the Mac, waits a bounded
// time, and holds the release for the next look if he doesn't touch it.
test("Mike's runner signs with a Touch ID key only while he's at the Mac, and holds the release when it isn't given", async () => {
  const w = world();
  commit(w.mike, 'b.txt', 'Newer work');
  git(w.mike, 'push', '-q', 'origin', 'main');
  const committed = Number(git(w.mike, 'log', '-1', '--format=%ct')) * 1000;
  // A Secure Enclave key's handle: its .pub says sk-.
  const handle = path.join(w.root, 'id_ecdsa_sk_rk');
  fs.writeFileSync(handle, 'handle\n');
  fs.writeFileSync(`${handle}.pub`, 'sk-ecdsa-sha2-nistp256@openssh.com AAAAInNrLWVjZHNh threadbear-release\n');
  assert.equal(touchKey(handle), true);
  assert.equal(touchKey(w.mikeKey.priv), false, 'a plain key file, as a fork of a fork keeps');
  let here = false;
  const signs: GitOptions[] = [];
  const up = upstreamFor(w.mike, w.home, new FakeClock(committed + 2 * SETTLE_MS), {
    publish: { publish: true, signingKey: handle },
    runningMainSettled: () => true,
    atMac: async () => here,
    git: (cwd: string, args: string[], opts: GitOptions = {}) => {
      if (args.includes('-s') && args.includes('tag')) {
        signs.push(opts);
        // No touch: ssh-keygen gives up, git fails, no tag is made.
        return Promise.reject(new Error('Couldn\'t sign message: device not found?'));
      }
      return gitAsync(cwd, args, opts);
    },
  });
  assert.equal(await up.publishOnce(), undefined);
  assert.equal(signs.length, 0, 'not tried with nobody at the Mac');
  assert.match(up.status().held?.why ?? '', /at the Mac to sign it with Touch ID/);
  here = true;
  assert.equal(await up.publishOnce(), undefined);
  assert.equal(signs.length, 1);
  assert.equal(signs[0]!.timeoutMs, TOUCH_SIGN_TIMEOUT_MS);
  assert.equal(signs[0]!.env?.SSH_SK_PROVIDER, MAC_SK_PROVIDER);
  assert.match(up.status().held?.why ?? '', /Touch ID wasn't given/);
  assert.equal(git(w.mike, 'tag', '--list', 'release/*').split('\n').filter((t) => t !== 'release/2026-10-01' && t).length, 0, 'no tag left behind');
});

// Codex's review of (c): a signed side-branch tag an older runner kept,
// dated after every real release, held every real one out.
test("a kept release whose commit isn't on upstream's main is taken out, and doesn't hold real ones back", async () => {
  const w = world();
  const up = upstreamFor(w.theirs, w.home);
  await up.check();
  git(w.mike, 'checkout', '-q', '-b', 'side');
  commit(w.mike, 'side.txt', 'Never reviewed');
  signedTag(w.mike, 'release/2026-12-01', w.mikeKey.priv);
  git(w.mike, 'push', '-q', 'origin', 'refs/tags/release/2026-12-01');
  git(w.mike, 'checkout', '-q', 'main');
  // As a runner from before the main check kept it.
  git(w.theirs, 'fetch', '-q', 'upstream', 'refs/tags/release/2026-12-01:refs/threadbear/upstream/2026-12-01');
  release(w, 'release/2026-11-01', w.mikeKey.priv);
  const s = await up.check();
  assert.equal(s.release?.tag, 'release/2026-11-01');
  assert.equal(git(w.theirs, 'for-each-ref', 'refs/threadbear/upstream/2026-12-01'), '');
});

// Codex's review of (d): a release that revokes the key it's signed with
// was still offered (and could start a bring-in) until the next check.
test('a release that revokes its own signing key is not offered', async () => {
  const w = world();
  const up = upstreamFor(w.theirs, w.home);
  const next = key(w.root, 'next-key');
  fs.writeFileSync(signersPath(w.home), `${keyLine(w.mikeKey)}\n${keyLine(next)}\n`);
  release(w, 'release/2026-10-30', next.priv);
  assert.equal((await up.check()).release?.tag, 'release/2026-10-30');
  const offered: (string | undefined)[] = [];
  const up2 = upstreamFor(w.theirs, w.home, new FakeClock(LATER), { onChecked: (st: { release?: { tag: string } }) => offered.push(st.release?.tag) });
  release(w, 'release/2026-11-01', next.priv, { 'scripts/release-keys-revoked': `${pubOf(next)}\n` });
  const s = await up2.check();
  assert.notEqual(s.release?.tag, 'release/2026-11-01');
  assert.ok(!offered.includes('release/2026-11-01'));
  assert.deepEqual(pinnedKeys(w.home), [pubOf(w.mikeKey)]);
});

// Codex's second look: another runner's check (a handover) revoking the key
// while this check reads the release must stop it being offered too.
test('a key another runner revokes during a check stops that check offering its release', async () => {
  const w = world();
  const next = key(w.root, 'next-key');
  fs.writeFileSync(signersPath(w.home), `${keyLine(w.mikeKey)}\n${keyLine(next)}\n`);
  release(w, 'release/2026-10-30', w.mikeKey.priv);
  release(w, 'release/2026-11-01', next.priv);
  let done = false;
  const offered: (string | undefined)[] = [];
  const up = upstreamFor(w.theirs, w.home, new FakeClock(LATER), {
    onChecked: (st: { release?: { tag: string } }) => offered.push(st.release?.tag),
    git: async (cwd: string, args: string[], opts?: GitOptions) => {
      // As this check reads the release's key files, the other runner
      // revokes `next` (it holds a newer release that does).
      if (!done && args[0] === 'show' && String(args[1]).endsWith(':scripts/release-signers')) {
        done = true;
        fs.writeFileSync(signersPath(w.home), `${keyLine(w.mikeKey)}\n`);
        fs.writeFileSync(path.join(w.home, 'upstream-revoked'), `${pubOf(next)}\n`);
      }
      return gitAsync(cwd, args, opts);
    },
  });
  const s = await up.check();
  assert.equal(s.release?.tag, 'release/2026-10-30');
  assert.ok(!offered.includes('release/2026-11-01'));
});
