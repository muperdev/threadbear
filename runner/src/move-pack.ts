// The bundle a moving thread travels in (docs/protocol.md, "Moving a thread
// to another runner"): a flat archive of its files, gzipped, sealed to the
// new runner's key (secrets.ts sealBytes) and cut into chunks small enough
// for a runner key's uploads. Its own format rather than tar, so packing
// and unpacking are the same on macOS and Linux, and unpacking can refuse
// anything that isn't a plain file at a plain relative path.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';

const MAGIC = Buffer.from('TBMOVE1\n', 'utf8');
// A runner key's upload is at most 10 MB; a chunk leaves room.
export const CHUNK_BYTES = 9 * 1024 * 1024;
// What a move may carry, sealed (convex/moves.ts MAX_MOVE_CHUNKS of them).
export const MAX_SEALED_BYTES = 100 * 1024 * 1024;
// Unpacked, at most this: a gzip that grows past it is refused.
export const MAX_UNPACKED_BYTES = 1024 * 1024 * 1024;
const MAX_ENTRIES = 100_000;
const MAX_PATH = 1024;

export type PackEntry = { path: string; data: Buffer; mode?: number };

// A path in the archive: relative, forward slashes, no `..`, nothing empty.
export function safeEntryPath(p: string): boolean {
  if (!p || p.length > MAX_PATH || p.startsWith('/') || p.includes('\\') || p.includes('\0')) return false;
  return p.split('/').every((seg) => seg !== '' && seg !== '.' && seg !== '..');
}

export function archive(entries: PackEntry[]): Buffer {
  const header = entries.map((e) => {
    if (!safeEntryPath(e.path)) throw new Error(`can't pack ${e.path}`);
    return { path: e.path, size: e.data.length, ...(e.mode !== undefined ? { mode: e.mode & 0o777 } : {}) };
  });
  const json = Buffer.from(JSON.stringify(header), 'utf8');
  const len = Buffer.alloc(4);
  len.writeUInt32BE(json.length);
  return zlib.gzipSync(Buffer.concat([MAGIC, len, json, ...entries.map((e) => e.data)]));
}

export function unarchive(packed: Buffer): PackEntry[] {
  const fail = (why: string) => new Error(`The moved thread's bundle is damaged (${why}).`);
  let raw: Buffer;
  try {
    raw = zlib.gunzipSync(packed, { maxOutputLength: MAX_UNPACKED_BYTES });
  } catch {
    throw fail('not a gzip, or too big');
  }
  if (raw.length < MAGIC.length + 4 || !raw.subarray(0, MAGIC.length).equals(MAGIC)) throw fail('not a move bundle');
  const len = raw.readUInt32BE(MAGIC.length);
  let header: unknown;
  try {
    header = JSON.parse(raw.subarray(MAGIC.length + 4, MAGIC.length + 4 + len).toString('utf8'));
  } catch {
    throw fail('no header');
  }
  if (!Array.isArray(header) || header.length > MAX_ENTRIES) throw fail('bad header');
  const out: PackEntry[] = [];
  const seen = new Set<string>();
  let at = MAGIC.length + 4 + len;
  for (const h of header as { path?: unknown; size?: unknown; mode?: unknown }[]) {
    if (typeof h?.path !== 'string' || !safeEntryPath(h.path) || seen.has(h.path)) throw fail('a bad path');
    if (!Number.isSafeInteger(h.size) || (h.size as number) < 0 || at + (h.size as number) > raw.length) throw fail('a bad size');
    seen.add(h.path);
    const mode = typeof h.mode === 'number' && Number.isInteger(h.mode) ? h.mode & 0o777 : undefined;
    out.push({ path: h.path, data: raw.subarray(at, at + (h.size as number)), ...(mode !== undefined ? { mode } : {}) });
    at += h.size as number;
  }
  if (at !== raw.length) throw fail('trailing bytes');
  return out;
}

/**
 * Every plain file under `dir`, as entries under `prefix`. Symlinks and
 * anything else that isn't a file or folder are left out, and so are the
 * top-level names in `skip`.
 */
export function readTree(dir: string, prefix: string, skip: string[] = []): PackEntry[] {
  const out: PackEntry[] = [];
  const walk = (abs: string, rel: string, top: boolean) => {
    let names: fs.Dirent[];
    try {
      names = fs.readdirSync(abs, { withFileTypes: true });
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code === 'ENOENT') return;
      throw e;
    }
    for (const d of names.sort((a, b) => (a.name < b.name ? -1 : 1))) {
      if (top && skip.includes(d.name)) continue;
      const a = path.join(abs, d.name);
      const r = rel ? `${rel}/${d.name}` : d.name;
      if (d.isDirectory()) walk(a, r, false);
      else if (d.isFile()) out.push({ path: `${prefix}/${r}`, data: fs.readFileSync(a), mode: fs.statSync(a).mode });
    }
  };
  walk(dir, '', true);
  return out;
}

/**
 * Writes the entries under `prefix` into `dir`, the prefix taken off.
 * Never writes through a symlink or outside `dir`; an existing file is
 * replaced. Returns how many it wrote.
 */
export function writeTree(entries: PackEntry[], prefix: string, dir: string): number {
  const root = path.resolve(dir);
  fs.mkdirSync(root, { recursive: true, mode: 0o700 });
  let n = 0;
  for (const e of entries) {
    if (!e.path.startsWith(`${prefix}/`)) continue;
    const rel = e.path.slice(prefix.length + 1);
    if (!safeEntryPath(rel)) throw new Error(`can't unpack ${e.path}`);
    const dest = path.join(root, ...rel.split('/'));
    if (!dest.startsWith(root + path.sep)) throw new Error(`can't unpack ${e.path}`);
    // Each folder on the way must be a real folder of ours, not a link out.
    let at = root;
    for (const seg of rel.split('/').slice(0, -1)) {
      at = path.join(at, seg);
      const st = fs.lstatSync(at, { throwIfNoEntry: false });
      if (!st) fs.mkdirSync(at, { mode: 0o700 });
      else if (!st.isDirectory()) throw new Error(`can't unpack ${e.path}: ${seg} isn't a folder`);
    }
    const st = fs.lstatSync(dest, { throwIfNoEntry: false });
    if (st && !st.isFile()) throw new Error(`can't unpack ${e.path} over something that isn't a file`);
    fs.writeFileSync(dest, e.data, { mode: (e.mode ?? 0o600) & 0o777 });
    n++;
  }
  return n;
}

export function sha256(buf: Buffer): string {
  return crypto.createHash('sha256').update(buf).digest('hex');
}

export function chunks(buf: Buffer, size = CHUNK_BYTES): Buffer[] {
  const out: Buffer[] = [];
  for (let at = 0; at < buf.length; at += size) out.push(buf.subarray(at, at + size));
  return out.length ? out : [buf];
}

// What a move seals its bundle under (secrets.ts sealBytes), so a bundle
// opens only as the move it was made for.
export function sealInfo(moveId: string): string {
  return `threadbear move v1 ${moveId}`;
}

// The biggest entries, for saying what made a bundle too big.
export function biggest(entries: PackEntry[], n = 3): string {
  return [...entries]
    .sort((a, b) => b.data.length - a.data.length)
    .slice(0, n)
    .map((e) => `${e.path} (${Math.round(e.data.length / 1024 / 1024)} MB)`)
    .join(', ');
}
