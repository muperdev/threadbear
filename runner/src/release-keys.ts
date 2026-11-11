// Replacing and revoking the key Mike's releases are signed with
// (docs/protocol.md, "Mike's releases"; security review finding 3, 9 Nov
// 2026). Setup pins the key once (<home>/upstream-signers) and a change to
// the repo's copy never replaces it, so a new key has to come the way a
// release does: signed by a key the fork already trusts.
//
// A release's commit carries the key list as files, covered by its signed
// tag like the rest of the commit:
//   scripts/release-signers        the keys that sign releases, one
//                                  allowed-signers line each (as setup pins)
//   scripts/release-keys-revoked   keys that must never count again, one
//                                  public key per line
//
// The newest release a fork accepts (signed by a pinned key, its commit on
// upstream's main) is read for both, and:
// - A key in release-signers that isn't pinned waits ADD_DELAY_MS from when
//   this runner first saw it, then joins the pinned file. A key that leaked
//   could announce one of its own, so the wait is the owner's (and Mike's)
//   time to see it in the log and revoke the key that announced it, which
//   drops the waiting one with it.
// - A key in release-keys-revoked comes out of the pinned file at once and
//   is kept in <home>/upstream-revoked, which ssh-keygen is handed on every
//   check (-r), and which setup and a later announcement can never undo. A
//   key can only be revoked by a release signed with that key itself or
//   with one pinned after it, so a leaked old key can't revoke the key that
//   replaced it.
//
// Mike's rotation, then: add the new key to scripts/release-signers and let
// a release signed with the old key carry it; after the wait, sign with the
// new key; then list the old key in scripts/release-keys-revoked.
import fs from 'node:fs';
import path from 'node:path';

export const ADD_DELAY_MS = 7 * 24 * 60 * 60_000;
export const SIGNERS_FILE = 'scripts/release-signers';
export const REVOKED_FILE = 'scripts/release-keys-revoked';

const KEY_TYPE = /^(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp(256|384|521)|sk-ssh-ed25519@openssh\.com|sk-ecdsa-sha2-nistp256@openssh\.com)$/;
const B64 = /^[A-Za-z0-9+/]+={0,2}$/;

export function revokedPath(home: string): string {
  return path.join(home, 'upstream-revoked');
}

export function keyStatePath(home: string): string {
  return path.join(home, 'upstream-keys.json');
}

// "type base64" from an allowed-signers line or a public key line, or
// undefined. Options before the key (namespaces="git") are passed over; a
// quoted option with spaces in it isn't something Mike's files have.
export function keyOf(line: string): string | undefined {
  const words = line.trim().split(/\s+/);
  if (!words[0] || words[0].startsWith('#')) return undefined;
  for (let i = 0; i + 1 < words.length; i++) {
    if (KEY_TYPE.test(words[i]!) && B64.test(words[i + 1]!)) return `${words[i]} ${words[i + 1]}`;
  }
  return undefined;
}

export function keysIn(text: string): string[] {
  return [...new Set(text.split('\n').map(keyOf).filter((k): k is string => Boolean(k)))];
}

// The key that made an SSH signature (git's armored SSHSIG): the public key
// is in the signed blob, right after the magic and the version.
export function signerKey(armor: string): string | undefined {
  const body = armor.replace(/-----(BEGIN|END) SSH SIGNATURE-----/g, '').replace(/\s+/g, '');
  let buf: Buffer;
  try {
    buf = Buffer.from(body, 'base64');
  } catch {
    return undefined;
  }
  if (buf.length < 14 || buf.subarray(0, 6).toString('latin1') !== 'SSHSIG') return undefined;
  const len = buf.readUInt32BE(10);
  const blob = buf.subarray(14, 14 + len);
  if (blob.length !== len || len < 4) return undefined;
  const typeLen = blob.readUInt32BE(0);
  const type = blob.subarray(4, 4 + typeLen).toString('latin1');
  return KEY_TYPE.test(type) ? `${type} ${blob.toString('base64')}` : undefined;
}

type Pending = { key: string; by: string; seenAt: number; tag: string };
type KeyState = { pending: Pending[] };

function readState(home: string): KeyState {
  try {
    const s = JSON.parse(fs.readFileSync(keyStatePath(home), 'utf8')) as KeyState;
    return { pending: Array.isArray(s.pending) ? s.pending.filter((p) => typeof p?.key === 'string' && typeof p.by === 'string' && typeof p.seenAt === 'number') : [] };
  } catch {
    return { pending: [] };
  }
}

function writeAtomic(file: string, text: string): void {
  const tmp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, text, { mode: 0o644 });
  fs.renameSync(tmp, file);
}

export function readRevoked(home: string): string[] {
  try {
    return keysIn(fs.readFileSync(revokedPath(home), 'utf8'));
  } catch {
    return [];
  }
}

export type KeyChange = { added: string[]; revoked: string[]; waiting: string[]; refused: string[] };

// One runner at a time changes the key files (a runner handing over to the
// next runs two checks at once): both reading the same files and writing
// what each worked out could put a revoked key back (Codex's review, 9 Nov
// 2026). A lock older than LOCK_STALE_MS was left by a runner that died.
const LOCK_STALE_MS = 60_000;
export class KeysBusy extends Error {}

// The lock is a file holding its owner ("pid random"), written whole
// before link() puts it in place, so it never exists without one. It's
// taken over only when its owner is gone (that pid isn't running), by
// moving it aside and checking it was still that owner's: two runners
// finding the same dead lock can't both end up in, and neither can take a
// live one (Codex's review). At most two runners share a data folder (one
// handing over to the next); a third arriving inside the moment a lock is
// moved aside and given back could slip in, which needs three at once and
// a runner that died holding the lock, so it isn't guarded further.
function ownerAlive(owner: string | undefined): boolean {
  const pid = Number(owner?.split(' ')[0]);
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === 'EPERM';
  }
}

function readOwner(file: string): string | undefined {
  try {
    return fs.readFileSync(file, 'utf8').trim() || undefined;
  } catch {
    return undefined;
  }
}

function withKeyLock<T>(home: string, fn: () => T): T {
  const lock = path.join(home, 'upstream-keys.lock');
  const me = `${process.pid} ${Math.random().toString(36).slice(2)}`;
  const busy = () => new KeysBusy('another runner is changing the release keys');
  const take = (): boolean => {
    const tmp = `${lock}.${me.replace(' ', '-')}`;
    fs.writeFileSync(tmp, me);
    try {
      fs.linkSync(tmp, lock);
      return true;
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code === 'EEXIST') return false;
      throw e;
    } finally {
      fs.rmSync(tmp, { force: true });
    }
  };
  if (!take()) {
    const owner = readOwner(lock);
    // One whose owner can't be read is left for LOCK_STALE_MS (it may be
    // a lock from an older runner), then counts as dead.
    let old = false;
    try {
      old = Date.now() - fs.statSync(lock).mtimeMs > LOCK_STALE_MS;
    } catch {
      old = true;
    }
    if (owner ? ownerAlive(owner) : !old) throw busy();
    const aside = `${lock}.dead-${me.replace(' ', '-')}`;
    try {
      fs.renameSync(lock, aside);
    } catch {
      throw busy();
    }
    if (readOwner(aside) !== owner) {
      // Someone took it between the look and the move: put it back only if
      // the place is still free, never over a newer lock.
      try {
        fs.linkSync(aside, lock);
      } catch {
        // taken again meanwhile; theirs stands
      }
      fs.rmSync(aside, { recursive: true, force: true });
      throw busy();
    }
    fs.rmSync(aside, { recursive: true, force: true });
    if (!take()) throw busy();
  }
  try {
    return fn();
  } finally {
    if (readOwner(lock) === me) fs.rmSync(lock, { force: true });
  }
}

// Applies what a verified release's commit says about the keys (above).
// `signer` is the key that signed that release, `announced` the keys in its
// release-signers, `revoke` those in its release-keys-revoked.
export function applyKeyChanges(o: { home: string; signers: string; signer: string; announced: string[]; revoke: string[]; tag: string; now: number }): KeyChange {
  return withKeyLock(o.home, () => applyLocked(o));
}

function applyLocked(o: { home: string; signers: string; signer: string; announced: string[]; revoke: string[]; tag: string; now: number }): KeyChange {
  const out: KeyChange = { added: [], revoked: [], waiting: [], refused: [] };
  let lines = fs.readFileSync(o.signers, 'utf8').split('\n').filter((l) => l.trim());
  const revoked = new Set(readRevoked(o.home));
  const state = readState(o.home);
  // Each key's place as the file was when this began: taking keys out as
  // it goes mustn't move a newer key below the signer (Codex's review:
  // C revoking A, B and then D got D too).
  const rank = new Map<string, number>();
  lines.forEach((l, i) => {
    const k = keyOf(l);
    if (k && !rank.has(k)) rank.set(k, i);
  });
  const at = (k: string) => rank.get(k) ?? -1;
  const pinnedNow = (k: string) => lines.some((l) => keyOf(l) === k);
  const signerAt = at(o.signer);
  if (signerAt < 0) return out; // only a pinned key's release says anything

  for (const r of o.revoke) {
    if (revoked.has(r)) continue;
    const waiting = state.pending.find((p) => p.key === r);
    // Itself, or a key pinned after it; one still waiting, by the key that
    // announced it or a later one.
    const may = r === o.signer || (at(r) >= 0 ? signerAt > at(r) : waiting ? at(waiting.by) >= 0 && signerAt >= at(waiting.by) : false);
    if (!may) {
      if (at(r) >= 0 || waiting) out.refused.push(r);
      continue;
    }
    revoked.add(r);
    out.revoked.push(r);
    lines = lines.filter((l) => keyOf(l) !== r);
    // What it announced and is still waiting goes with it.
    state.pending = state.pending.filter((p) => p.key !== r && p.by !== r);
  }

  // The principal of the first real key line (a comment above it isn't one).
  const principal = lines.find((l) => keyOf(l))?.trim().split(/\s+/)[0] || 'threadbear-releases';
  for (const a of o.announced) {
    if (revoked.has(a) || pinnedNow(a) || state.pending.some((p) => p.key === a)) continue;
    if (revoked.has(o.signer)) continue;
    state.pending.push({ key: a, by: o.signer, seenAt: o.now, tag: o.tag });
  }
  const still: Pending[] = [];
  for (const p of state.pending) {
    if (revoked.has(p.key) || revoked.has(p.by) || !pinnedNow(p.by)) continue; // its announcer is gone
    if (o.now - p.seenAt >= ADD_DELAY_MS) {
      lines.push(`${principal} namespaces="git" ${p.key}`);
      out.added.push(p.key);
    } else {
      still.push(p);
      out.waiting.push(p.key);
    }
  }
  state.pending = still;

  writeAtomic(o.signers, lines.join('\n') + '\n');
  writeAtomic(revokedPath(o.home), [...revoked].join('\n') + (revoked.size ? '\n' : ''));
  writeAtomic(keyStatePath(o.home), JSON.stringify(state, null, 2) + '\n');
  return out;
}
