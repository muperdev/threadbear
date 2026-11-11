// The helpers shared by the upstream-*.test.ts files. It's one suite split
// across files because tests in one file run one after another, and this one
// was started last and held the whole unit suite up by itself (9 Nov).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { tmpDir } from './fakes.ts';
import { FakeClock } from '../../src/clock.ts';
import { nullLogger } from '../../src/log.ts';
import { Upstream, signersPath } from '../../src/upstream.ts';
import { PATIENCE_MS } from './patience.ts';

// Commits and tags in the throwaway repos need a name, and must not pick
// up a signing setup from whoever runs the tests.
export const GIT_ENV = { ...process.env, GIT_AUTHOR_NAME: 'Mike', GIT_AUTHOR_EMAIL: 'mike@example.com', GIT_COMMITTER_NAME: 'Mike', GIT_COMMITTER_EMAIL: 'mike@example.com', GIT_CONFIG_GLOBAL: '/dev/null' };
// The runner's own git (the signed tag) needs a name too: with no global
// config, git on macOS makes one up from the hostname, but on Linux it
// refuses ("Committer identity unknown").
Object.assign(process.env, { GIT_CONFIG_GLOBAL: '/dev/null', GIT_COMMITTER_NAME: 'Mike', GIT_COMMITTER_EMAIL: 'mike@example.com' });

export function git(cwd: string, ...args: string[]): string {
  const r = spawnSync('git', ['-C', cwd, '-c', 'commit.gpgsign=false', '-c', 'tag.gpgsign=false', ...args], { encoding: 'utf8', env: GIT_ENV });
  if (r.status !== 0) throw new Error(`git ${args.join(' ')}: ${r.stderr}`);
  return r.stdout.trim();
}

export function key(dir: string, name: string): { priv: string; signers: string } {
  const priv = path.join(dir, name);
  const r = spawnSync('ssh-keygen', ['-q', '-t', 'ed25519', '-N', '', '-C', name, '-f', priv]);
  assert.equal(r.status, 0, String(r.stderr));
  const signers = path.join(dir, `${name}.signers`);
  fs.writeFileSync(signers, `mike@example.com namespaces="git" ${fs.readFileSync(`${priv}.pub`, 'utf8').split(' ').slice(0, 2).join(' ')}\n`);
  return { priv, signers };
}

export function commit(repo: string, file: string, subject: string): string {
  fs.writeFileSync(path.join(repo, file), subject);
  git(repo, 'add', file);
  git(repo, 'commit', '-q', '-m', subject);
  return git(repo, 'rev-parse', 'HEAD');
}

export function signedTag(repo: string, tag: string, priv: string, rev = 'HEAD'): void {
  git(repo, '-c', 'gpg.format=ssh', '-c', `user.signingkey=${priv}`, 'tag', '-s', '-m', `Threadbear ${tag}`, tag, rev);
}

// Mike's repo (a bare one on "GitHub", and his checkout) and someone's fork
// of it, cloned, with origin as their fork and upstream as Mike's.
export function world() {
  const root = tmpDir('tb-upstream-');
  const mikeKey = key(root, 'mike-key');
  const mikeBare = path.join(root, 'mike.git');
  const forkBare = path.join(root, 'fork.git');
  const mike = path.join(root, 'mike');
  const theirs = path.join(root, 'theirs');
  git(root, 'init', '-q', '--bare', '-b', 'main', mikeBare);
  git(root, 'clone', '-q', mikeBare, mike);
  git(mike, 'checkout', '-q', '-b', 'main');
  commit(mike, 'a.txt', 'First');
  signedTag(mike, 'release/2026-10-01', mikeKey.priv);
  git(mike, 'push', '-q', 'origin', 'main', '--tags');
  git(root, 'clone', '-q', '--bare', mikeBare, forkBare);
  git(root, 'clone', '-q', forkBare, theirs);
  git(theirs, 'remote', 'add', 'upstream', mikeBare);
  const home = path.join(root, 'home');
  fs.mkdirSync(home);
  fs.copyFileSync(mikeKey.signers, signersPath(home));
  return { root, mikeKey, mike, theirs, home };
}

// The clock after every release date the tests use: a release dated after
// today (and a couple of days' slack) isn't looked at.
export const LATER = Date.parse('2026-12-31T00:00:00Z');

// origin as GitHub, still fetching from and pushing to the bare repo: the
// runner reads the configured URL, git goes through the insteadOf.
export function asGitHub(repo: string, root: string): void {
  git(repo, 'config', 'remote.origin.url', 'https://github.com/mikecann/threadbear.git');
  git(repo, 'config', `url.${path.join(root, 'mike.git')}.insteadOf`, 'https://github.com/mikecann/threadbear.git');
}

export function upstreamFor(checkout: string, home: string, clock = new FakeClock(LATER), extra = {}) {
  return new Upstream({ home, checkout, clock, log: nullLogger, ...extra });
}

// A stand-in for gpg (or gpgsm) that signs with made-up armor and says any
// signature is good, as a victim's gpg does for an attacker's key that's in
// their keyring or that it fetched itself (auto-key-retrieve, WKD).
export function fakeGpg(dir: string, armor: 'PGP SIGNATURE' | 'SIGNED MESSAGE'): string {
  const file = path.join(dir, `fake-gpg-${armor.replace(' ', '-')}`);
  fs.writeFileSync(file, `#!/bin/sh
case "$*" in
  *--verify*)
    # Read what git writes in, as gpg does: leaving it unread, git can get
    # a broken pipe and call the tag bad (two runs in three on Linux).
    cat > /dev/null
    printf '[GNUPG:] NEWSIG\n[GNUPG:] GOODSIG 0123456789ABCDEF Mallory <m@example.com>\n[GNUPG:] VALIDSIG 0123456789ABCDEF0123456789ABCDEF01234567 2026-12-01 1764547200 0 4 0 22 10 00 0123456789ABCDEF0123456789ABCDEF01234567\n[GNUPG:] TRUST_FULLY 0 pgp\n'
    exit 0;;
  *)
    cat > /dev/null
    printf -- '-----BEGIN ${armor}-----\n\nAAAAfakefakefake\n-----END ${armor}-----\n'
    printf '\n[GNUPG:] SIG_CREATED D 22 8 00 1764547200 0123456789ABCDEF0123456789ABCDEF01234567\n' >&2
    exit 0;;
esac
`, { mode: 0o755 });
  return file;
}

// The victim's own git config pointing gpg (or gpgsm) at the fake, for as
// long as \`run\` takes: the runner's git reads it, as it would ~/.gitconfig.
export async function withVictimGpg<T>(entries: Record<string, string>, run: () => Promise<T>): Promise<T> {
  const keys = Object.keys(entries);
  const env: Record<string, string> = { GIT_CONFIG_COUNT: String(keys.length) };
  keys.forEach((k, i) => Object.assign(env, { [`GIT_CONFIG_KEY_${i}`]: k, [`GIT_CONFIG_VALUE_${i}`]: entries[k] }));
  const before = Object.fromEntries(Object.keys(env).map((k) => [k, process.env[k]]));
  Object.assign(process.env, env);
  try {
    return await run();
  } finally {
    for (const [k, v] of Object.entries(before)) {
      if (v === undefined) delete process.env[k];
      else process.env[k] = v;
    }
  }
}

export const kept = (repo: string, name: string) => spawnSync('git', ['-C', repo, 'rev-parse', '-q', '--verify', `refs/threadbear/upstream/${name}^{commit}`], { encoding: 'utf8' }).stdout.trim();

export async function waitFor(ok: () => boolean, ms = PATIENCE_MS): Promise<void> {
  const end = Date.now() + ms;
  while (!ok()) {
    if (Date.now() > end) throw new Error('timed out');
    await new Promise((r) => setTimeout(r, 20));
  }
}

// ------------------------------------------- Daybreak, 7 Nov 2026: bounds
// Upstream's tags are listed first, only a bounded few date-named ones are
// fetched, into a folder of their own outside the checkout, every git
// command has a deadline, and what doesn't verify leaves nothing behind.

// Objects in the checkout, loose and packed.
export function objectsIn(repo: string): Set<string> {
  const r = spawnSync('git', ['-C', repo, 'cat-file', '--batch-all-objects', '--batch-check=%(objectname)'], { encoding: 'utf8' });
  return new Set(r.stdout.trim().split('\n').filter(Boolean));
}

// A remote that never answers, standing in for one that's hijacked or down:
// git's upload-pack swapped for a script that hangs, with a child of its own.
export function hangingUpstream(w: ReturnType<typeof world>, when: 'list' | 'fetch') {
  const script = path.join(w.root, 'hang.sh');
  const count = path.join(w.root, 'calls');
  fs.writeFileSync(script, `#!/bin/bash
n=$(( $(cat "${count}" 2>/dev/null || echo 0) + 1 )); echo $n >"${count}"
if [ ${when === 'list' ? 1 : 2} -le $n ]; then sleep 600 & echo $! >>"${w.root}/pids"; echo $$ >>"${w.root}/pids"; wait; fi
exec git-upload-pack "$@"
`, { mode: 0o755 });
  git(w.theirs, 'config', 'remote.upstream.uploadpack', script);
  git(w.theirs, 'config', 'protocol.version', '2');
  return () => fs.readFileSync(path.join(w.root, 'pids'), 'utf8').trim().split('\n').map(Number);
}

export function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

// Finding 3 (d): the key list in a signed release's commit (release-keys.ts).
export function keyLine(k: { signers: string }): string {
  return fs.readFileSync(k.signers, 'utf8').trim();
}
export function pubOf(k: { signers: string }): string {
  return keyLine(k).split(' ').slice(2, 4).join(' ');
}
export function release(w: ReturnType<typeof world>, tag: string, priv: string, files: Record<string, string> = {}): string {
  for (const [f, text] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(w.mike, f)), { recursive: true });
    fs.writeFileSync(path.join(w.mike, f), text);
    git(w.mike, 'add', f);
  }
  const c = commit(w.mike, `${tag.replace("/", "-")}.txt`, `Work for ${tag}`);
  signedTag(w.mike, tag, priv);
  git(w.mike, 'push', '-q', 'origin', 'main', `refs/tags/${tag}`);
  return c;
}
export const pinnedKeys = (home: string) => fs.readFileSync(signersPath(home), 'utf8').trim().split('\n').map((l) => l.split(' ').slice(2, 4).join(' '));
