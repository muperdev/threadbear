import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { tmpDir } from '../helpers/fakes.ts';
import { ADD_DELAY_MS, KeysBusy, applyKeyChanges, keyOf, keyStatePath, readRevoked, signerKey } from '../../src/release-keys.ts';

// A real key: its public line and its "type base64".
function newKey(dir: string, name: string): { priv: string; line: string; key: string } {
  const priv = path.join(dir, name);
  spawnSync('ssh-keygen', ['-q', '-t', 'ed25519', '-N', '', '-C', name, '-f', priv]);
  const pub = fs.readFileSync(`${priv}.pub`, 'utf8').trim();
  return { priv, line: `mike@example.com namespaces="git" ${pub}`, key: pub.split(' ').slice(0, 2).join(' ') };
}

function setup(n: number, lead = '') {
  const home = tmpDir('tb-keys-');
  const keys = Array.from({ length: n }, (_, i) => newKey(home, `k${i}`));
  const signers = path.join(home, 'upstream-signers');
  fs.writeFileSync(signers, lead + keys.map((k) => k.line).join('\n') + '\n');
  const pinned = () => fs.readFileSync(signers, 'utf8').split('\n').map(keyOf).filter(Boolean);
  return { home, keys, signers, pinned };
}

test('signerKey reads the key out of an SSH signature', () => {
  const { keys, home } = setup(1);
  const msg = path.join(home, 'm');
  fs.writeFileSync(msg, 'hi');
  spawnSync('ssh-keygen', ['-Y', 'sign', '-n', 'git', '-f', keys[0]!.priv, msg]);
  assert.equal(signerKey(fs.readFileSync(`${msg}.sig`, 'utf8')), keys[0]!.key);
});

// Codex's review, 9 Nov 2026: with [A, B, C, D], C revoking A, B and D
// took D too, as taking A and B out moved D below C.
test('a key revokes only older keys, by where each was when the release was read', () => {
  const { home, keys, signers, pinned } = setup(4);
  const [a, b, c, d] = keys as [ReturnType<typeof newKey>, ReturnType<typeof newKey>, ReturnType<typeof newKey>, ReturnType<typeof newKey>];
  const r = applyKeyChanges({ home, signers, signer: c.key, announced: [], revoke: [a.key, b.key, d.key], tag: 't', now: 0 });
  assert.deepEqual(r.revoked, [a.key, b.key]);
  assert.deepEqual(r.refused, [d.key]);
  assert.deepEqual(pinned(), [c.key, d.key]);
  assert.deepEqual(readRevoked(home).sort(), [a.key, b.key].sort());
});

test('a new key gets the principal of the first real key line, past a comment', () => {
  const { home, keys, signers, pinned } = setup(1, '# Release keys\n');
  const next = newKey(home, 'next');
  applyKeyChanges({ home, signers, signer: keys[0]!.key, announced: [next.key], revoke: [], tag: 't', now: 0 });
  const r = applyKeyChanges({ home, signers, signer: keys[0]!.key, announced: [next.key], revoke: [], tag: 't', now: ADD_DELAY_MS + 1 });
  assert.deepEqual(r.added, [next.key]);
  assert.deepEqual(pinned(), [keys[0]!.key, next.key]);
  assert.match(fs.readFileSync(signers, 'utf8'), new RegExp(`^mike@example\\.com namespaces="git" ${next.key.replace(/[+/]/g, '\\$&')}$`, 'm'));
});

test("one runner at a time changes the keys: another's lock makes it wait for the next check, a dead runner's doesn't", () => {
  const { home, keys, signers } = setup(2);
  const lock = path.join(home, 'upstream-keys.lock');
  const go = () => applyKeyChanges({ home, signers, signer: keys[1]!.key, announced: [], revoke: [keys[0]!.key], tag: 't', now: 0 });
  // One with no owner to read, from just now: left alone.
  fs.writeFileSync(lock, '');
  assert.throws(go, KeysBusy);
  assert.equal(readRevoked(home).length, 0, 'nothing changed');
  // Old enough to be a dead runner's: taken over.
  const old = new Date(Date.now() - 10 * 60_000);
  fs.utimesSync(lock, old, old);
  assert.deepEqual(go().revoked, [keys[0]!.key]);
  assert.ok(!fs.existsSync(lock));
  assert.ok(fs.existsSync(keyStatePath(home)));
});

test("a lock is taken over only from a runner that's gone, never from a live one", () => {
  const { home, keys, signers } = setup(2);
  const lock = path.join(home, 'upstream-keys.lock');
  const go = () => applyKeyChanges({ home, signers, signer: keys[1]!.key, announced: [], revoke: [keys[0]!.key], tag: 't', now: 0 });
  // Held by a live process (this one's parent stands in for another
  // runner), however old: left alone.
  fs.writeFileSync(lock, `${process.ppid} other`);
  const old = new Date(Date.now() - 10 * 60_000);
  fs.utimesSync(lock, old, old);
  assert.throws(go, KeysBusy);
  assert.equal(fs.readFileSync(lock, 'utf8'), `${process.ppid} other`, 'still theirs');
  // Held by a pid that isn't running: taken over.
  fs.writeFileSync(lock, '999999 gone');
  assert.deepEqual(go().revoked, [keys[0]!.key]);
  assert.ok(!fs.existsSync(lock));
});
