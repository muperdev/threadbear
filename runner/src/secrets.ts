// Secrets an agent asks Mike for (docs/protocol.md, "Secrets"): where an
// answer is written, the askpass helper beside it, the 24-hour sweep, and
// the key pair the phone seals its answers to. The value lives only in the
// file: nothing here logs it, and errors never include it.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

export const SECRET_TTL_MS = 24 * 60 * 60 * 1000;
export const SECRET_TIMEOUT_DEFAULT_MIN = 10;
export const SECRET_TIMEOUT_MAX_MIN = 60;
// What the card says the value is for, so it can't promise more than the
// file delivers: the agent's commands can read it.
export const SECRET_PRIVACY_NOTE = "Kept out of chat, in a private file this agent's commands can read for 24 hours";

const FILE_NAME = /^[0-9a-f]{32}(\.askpass)?$/;
const KEY_FILE = 'secret-box.key';
const ASKED_DIR = 'asked';

// X25519 public keys as SubjectPublicKeyInfo DER: this fixed prefix, then
// the 32 raw bytes. Node takes and gives keys in DER, the phone in raw.
const SPKI_PREFIX = Buffer.from('302a300506032b656e032100', 'hex');

export type Sealed = { epk: string; nonce: string; box: string };
export type SecretFiles = { file: string; askpass: string };

export class SecretStore {
  private key?: crypto.KeyObject;
  private sweepTimer?: NodeJS.Timeout;

  private readonly secretsDir: string;
  private readonly now: () => number;
  // Told each answer as it's written, in memory only: the runner masks it from
  // what goes to Convex from this moment (src/secret-values.ts).
  onWritten?: (value: string) => void;

  constructor(secretsDir: string, now: () => number = Date.now) {
    this.secretsDir = secretsDir;
    this.now = now;
  }

  get dir(): string {
    return path.join(this.secretsDir, ASKED_DIR);
  }

  // Writes an answer to a new owner-only file and its askpass helper.
  // `wx` never follows or replaces an existing path.
  write(value: string): SecretFiles {
    fs.mkdirSync(this.dir, { recursive: true, mode: 0o700 });
    fs.chmodSync(this.dir, 0o700);
    const name = crypto.randomBytes(16).toString('hex');
    const file = path.join(this.dir, name);
    const askpass = `${file}.askpass`;
    this.onWritten?.(value);
    fs.writeFileSync(file, value, { flag: 'wx', mode: 0o600 });
    try {
      // sudo -A, ssh and git run the helper and read the password from
      // what it prints, so the value never goes on a command line.
      fs.writeFileSync(askpass, `#!/bin/sh\nexec /bin/cat '${file}'\n`, { flag: 'wx', mode: 0o700 });
    } catch (e) {
      fs.rmSync(file, { force: true });
      throw e;
    }
    return { file, askpass };
  }

  // Removes files older than a day. Returns how many went.
  sweep(): number {
    let names: string[];
    try {
      names = fs.readdirSync(this.dir);
    } catch {
      return 0;
    }
    let removed = 0;
    for (const name of names) {
      if (!FILE_NAME.test(name)) continue;
      const p = path.join(this.dir, name);
      try {
        if (this.now() - fs.statSync(p).mtimeMs <= SECRET_TTL_MS) continue;
        fs.rmSync(p, { force: true });
        removed++;
      } catch {
        // gone already
      }
    }
    return removed;
  }

  startSweeping(onError: (e: unknown) => void): void {
    const run = () => {
      try {
        this.sweep();
      } catch (e) {
        onError(e);
      }
    };
    run();
    this.sweepTimer = setInterval(run, 60 * 60 * 1000);
    this.sweepTimer.unref?.();
  }

  stop(): void {
    if (this.sweepTimer) clearInterval(this.sweepTimer);
  }

  private privateKey(): crypto.KeyObject {
    if (this.key) return this.key;
    const file = path.join(this.secretsDir, KEY_FILE);
    try {
      this.key = crypto.createPrivateKey(fs.readFileSync(file, 'utf8'));
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code !== 'ENOENT') throw e;
      const { privateKey } = crypto.generateKeyPairSync('x25519');
      fs.mkdirSync(this.secretsDir, { recursive: true, mode: 0o700 });
      try {
        fs.writeFileSync(file, privateKey.export({ type: 'pkcs8', format: 'pem' }), { flag: 'wx', mode: 0o600 });
        this.key = privateKey;
      } catch (err) {
        // Another start wrote one first: use that.
        if ((err as NodeJS.ErrnoException).code !== 'EEXIST') throw err;
        this.key = crypto.createPrivateKey(fs.readFileSync(file, 'utf8'));
      }
    }
    return this.key;
  }

  // The public key the phone seals to, raw and base64.
  publicKey(): string {
    return rawPublic(crypto.createPublicKey(this.privateKey())).toString('base64');
  }

  open(sealed: Sealed, approvalId: string): string {
    return openSealed(this.privateKey(), sealed, approvalId);
  }

  // A moving thread's bundle, sealed to this runner with sealBytes.
  openBytes(sealed: Buffer, info: string): Buffer {
    return openBytes(this.privateKey(), sealed, info);
  }
}

function rawPublic(k: crypto.KeyObject): Buffer {
  return (k.export({ type: 'spki', format: 'der' }) as Buffer).subarray(SPKI_PREFIX.length);
}

function publicFromRaw(raw: Buffer): crypto.KeyObject {
  if (raw.length !== 32) throw new Error('bad key');
  return crypto.createPublicKey({ key: Buffer.concat([SPKI_PREFIX, raw]), format: 'der', type: 'spki' });
}

function boxKey(shared: Buffer, epk: Buffer, recipient: Buffer, approvalId: string): Buffer {
  return Buffer.from(crypto.hkdfSync('sha256', shared, Buffer.concat([epk, recipient]), Buffer.from(`threadbear secret v1 ${approvalId}`, 'utf8'), 32));
}

// The phone's side, for tests and for anything else that answers through
// the mirror.
export function seal(recipientRawB64: string, value: string, approvalId: string): Sealed {
  const recipient = Buffer.from(recipientRawB64, 'base64');
  const eph = crypto.generateKeyPairSync('x25519');
  const epk = rawPublic(eph.publicKey);
  const shared = crypto.diffieHellman({ privateKey: eph.privateKey, publicKey: publicFromRaw(recipient) });
  const nonce = crypto.randomBytes(12);
  const c = crypto.createCipheriv('aes-256-gcm', boxKey(shared, epk, recipient, approvalId), nonce);
  const box = Buffer.concat([c.update(value, 'utf8'), c.final(), c.getAuthTag()]);
  return { epk: epk.toString('base64'), nonce: nonce.toString('base64'), box: box.toString('base64') };
}

export function openSealed(privateKey: crypto.KeyObject, sealed: Sealed, approvalId: string): string {
  // One message for every failure, so nothing about the input leaks.
  const fail = () => new Error("The phone's answer couldn't be opened. Answer it on the Mac.");
  try {
    const epk = Buffer.from(sealed.epk, 'base64');
    const nonce = Buffer.from(sealed.nonce, 'base64');
    const box = Buffer.from(sealed.box, 'base64');
    if (nonce.length !== 12 || box.length < 17) throw fail();
    const recipient = rawPublic(crypto.createPublicKey(privateKey));
    const shared = crypto.diffieHellman({ privateKey, publicKey: publicFromRaw(epk) });
    const d = crypto.createDecipheriv('aes-256-gcm', boxKey(shared, epk, recipient, approvalId), nonce);
    d.setAuthTag(box.subarray(box.length - 16));
    return Buffer.concat([d.update(box.subarray(0, box.length - 16)), d.final()]).toString('utf8');
  } catch {
    throw fail();
  }
}

// Bytes sealed the same way, for what's too big for a Sealed's base64 (a
// moving thread's bundle, docs/protocol.md "Moving a thread to another
// runner"): the ephemeral key, the nonce, the ciphertext and its tag, one
// after the other. `info` says what it's for, so it opens only as that.
export function sealBytes(recipientRawB64: string, data: Buffer, info: string): Buffer {
  const recipient = Buffer.from(recipientRawB64, 'base64');
  const eph = crypto.generateKeyPairSync('x25519');
  const epk = rawPublic(eph.publicKey);
  const shared = crypto.diffieHellman({ privateKey: eph.privateKey, publicKey: publicFromRaw(recipient) });
  const nonce = crypto.randomBytes(12);
  const c = crypto.createCipheriv('aes-256-gcm', bytesKey(shared, epk, recipient, info), nonce);
  return Buffer.concat([epk, nonce, c.update(data), c.final(), c.getAuthTag()]);
}

function bytesKey(shared: Buffer, epk: Buffer, recipient: Buffer, info: string): Buffer {
  return Buffer.from(crypto.hkdfSync('sha256', shared, Buffer.concat([epk, recipient]), Buffer.from(info, 'utf8'), 32));
}

export function openBytes(privateKey: crypto.KeyObject, sealed: Buffer, info: string): Buffer {
  const fail = () => new Error("The moved thread's bundle couldn't be opened.");
  if (sealed.length < 32 + 12 + 16) throw fail();
  try {
    const epk = sealed.subarray(0, 32);
    const nonce = sealed.subarray(32, 44);
    const recipient = rawPublic(crypto.createPublicKey(privateKey));
    const shared = crypto.diffieHellman({ privateKey, publicKey: publicFromRaw(epk) });
    const d = crypto.createDecipheriv('aes-256-gcm', bytesKey(shared, epk, recipient, info), nonce);
    d.setAuthTag(sealed.subarray(sealed.length - 16));
    return Buffer.concat([d.update(sealed.subarray(44, sealed.length - 16)), d.final()]);
  } catch {
    throw fail();
  }
}

export function isSealed(x: unknown): x is Sealed {
  if (!x || typeof x !== 'object') return false;
  const s = x as Record<string, unknown>;
  return typeof s.epk === 'string' && typeof s.nonce === 'string' && typeof s.box === 'string';
}
