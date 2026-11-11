// A moving thread's bundle (src/move-pack.ts) and its sealing
// (secrets.ts sealBytes): what goes in comes out, and nothing in a bundle
// can write outside the folder it's unpacked into.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';
import { archive, chunks, readTree, safeEntryPath, sealInfo, unarchive, writeTree } from '../../src/move-pack.ts';
import { SecretStore, sealBytes } from '../../src/secrets.ts';

const tmp = () => fs.mkdtempSync(path.join(os.tmpdir(), 'tb-move-pack-'));

test('a folder packs and unpacks as it was, without its symlinks or the names left out', () => {
  const from = tmp();
  fs.mkdirSync(path.join(from, 'blobs/ab'), { recursive: true });
  fs.writeFileSync(path.join(from, 'thread.json'), '{"id":"t"}');
  fs.writeFileSync(path.join(from, 'events.ndjson'), '{"seq":1}\n');
  fs.writeFileSync(path.join(from, 'blobs/ab/cd.bin'), Buffer.from([0, 1, 2, 255]));
  fs.mkdirSync(path.join(from, 'panels'));
  fs.writeFileSync(path.join(from, 'panels/x.html'), 'rebuilt anyway');
  fs.symlinkSync('/etc/passwd', path.join(from, 'blobs/link'));
  const entries = readTree(from, 'thread', ['panels']);
  assert.deepEqual(entries.map((e) => e.path), ['thread/blobs/ab/cd.bin', 'thread/events.ndjson', 'thread/thread.json']);
  const back = unarchive(archive([...entries, { path: 'move.json', data: Buffer.from('{}') }]));
  assert.deepEqual(back.map((e) => [e.path, e.data.toString('base64')]), [...entries.map((e) => [e.path, e.data.toString('base64')]), ['move.json', 'e30=']]);
  const to = tmp();
  assert.equal(writeTree(back, 'thread', to), 3);
  assert.deepEqual(fs.readFileSync(path.join(to, 'blobs/ab/cd.bin')), Buffer.from([0, 1, 2, 255]));
  assert.equal(fs.readFileSync(path.join(to, 'thread.json'), 'utf8'), '{"id":"t"}');
  assert.equal(fs.existsSync(path.join(to, 'move.json')), false, 'only what was under the prefix');
});

test('paths in a bundle are plain relative ones', () => {
  for (const ok of ['a', 'a/b.txt', 'thread/blobs/x y.png']) assert.ok(safeEntryPath(ok), ok);
  for (const bad of ['', '/etc/passwd', '../x', 'a/../../x', 'a//b', './a', 'a\\b', 'a\0b', 'x'.repeat(2000)]) assert.equal(safeEntryPath(bad), false, bad);
  assert.throws(() => archive([{ path: '../escape', data: Buffer.alloc(1) }]));
});

test('a damaged or hand-made bundle is refused', () => {
  const good = archive([{ path: 'a', data: Buffer.from('hi') }]);
  assert.throws(() => unarchive(Buffer.from('not gzip')), /damaged/);
  assert.throws(() => unarchive(zlib.gzipSync(Buffer.from('TBMOVE2\n....'))), /damaged/);
  // A header naming a path that climbs out, or one file twice.
  const forged = (header: unknown, body = Buffer.alloc(0)) => {
    const json = Buffer.from(JSON.stringify(header));
    const len = Buffer.alloc(4);
    len.writeUInt32BE(json.length);
    return zlib.gzipSync(Buffer.concat([Buffer.from('TBMOVE1\n'), len, json, body]));
  };
  assert.throws(() => unarchive(forged([{ path: '../../.ssh/authorized_keys', size: 1 }], Buffer.from('x'))), /bad path/);
  assert.throws(() => unarchive(forged([{ path: 'a', size: 1 }, { path: 'a', size: 1 }], Buffer.from('xy'))), /bad path/);
  assert.throws(() => unarchive(forged([{ path: 'a', size: 5 }], Buffer.from('x'))), /bad size/);
  assert.throws(() => unarchive(forged([{ path: 'a', size: 1 }], Buffer.from('xy'))), /trailing/);
  assert.equal(unarchive(good)[0]!.data.toString(), 'hi');
});

test('unpacking never writes through a symlink', () => {
  const to = tmp();
  const outside = tmp();
  fs.symlinkSync(outside, path.join(to, 'blobs'));
  assert.throws(() => writeTree([{ path: 'thread/blobs/evil', data: Buffer.from('x') }], 'thread', to), /isn't a folder/);
  fs.symlinkSync(path.join(outside, 'target'), path.join(to, 'f'));
  assert.throws(() => writeTree([{ path: 'thread/f', data: Buffer.from('x') }], 'thread', to), /isn't a file/);
  assert.deepEqual(fs.readdirSync(outside), []);
});

test('a bundle sealed to a runner opens only there, and only as its move', () => {
  const t = new SecretStore(path.join(tmp(), 'secrets'));
  const other = new SecretStore(path.join(tmp(), 'secrets'));
  const data = Buffer.from('the whole thread '.repeat(1000));
  const sealed = sealBytes(t.publicKey(), data, sealInfo('m1'));
  assert.deepEqual(t.openBytes(sealed, sealInfo('m1')), data);
  assert.throws(() => t.openBytes(sealed, sealInfo('m2')), /couldn't be opened/);
  assert.throws(() => other.openBytes(sealed, sealInfo('m1')), /couldn't be opened/);
  const tampered = Buffer.from(sealed);
  tampered[60] ^= 1;
  assert.throws(() => t.openBytes(tampered, sealInfo('m1')), /couldn't be opened/);
});

test('chunks join back into the bundle', () => {
  const buf = Buffer.alloc(25, 7);
  const parts = chunks(buf, 10);
  assert.deepEqual(parts.map((p) => p.length), [10, 10, 5]);
  assert.deepEqual(Buffer.concat(parts), buf);
  assert.equal(chunks(Buffer.alloc(0)).length, 1);
});
