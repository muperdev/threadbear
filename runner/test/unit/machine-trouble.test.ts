import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { spawnSync } from 'node:child_process';
import { machineTrouble, watchInNewProcess, type MachineProbes } from '../../src/machine-trouble.ts';

const errno = (code: string) => Object.assign(new Error(code), { code });
const healthy: MachineProbes = { watch: () => {}, freeBytes: () => 50 * 2 ** 30, run: () => undefined };
const check = (over: Partial<MachineProbes>) => machineTrouble('/home', { ...healthy, ...over });

test('a healthy machine has no trouble', () => {
  assert.equal(check({}), undefined);
});

test('running out of file watches is trouble, however it says so', () => {
  for (const code of ['EMFILE', 'ENFILE', 'ENOSPC']) {
    assert.match(check({ watch: () => { throw errno(code); } })!, new RegExp(`no file watches left \\(${code}\\)`));
  }
});

test("a watch failing for some other reason doesn't blame the machine", () => {
  assert.equal(check({ watch: () => { throw errno('EACCES'); } }), undefined);
});

test('a nearly full disk is trouble, naming where', () => {
  assert.match(check({ freeBytes: (d) => (d === '/home' ? 2 ** 20 : 50 * 2 ** 30) })!, /under 1 GB free on \/home/);
});

test("a disk that can't be read says nothing about the machine", () => {
  assert.equal(check({ freeBytes: () => undefined }), undefined);
});

test("not being able to start processes is trouble, and a program that fails isn't", () => {
  assert.match(check({ run: () => 'EAGAIN' })!, /can't start new processes \(EAGAIN\)/);
  assert.equal(check({ run: () => 'ENOENT' }), undefined);
});

test('the watch is tried in a new process, as one in the runner reuses the inotify instance it holds', () => {
  const ran: unknown[][] = [];
  const answer = (stdout: string) => ((...a: unknown[]) => (ran.push(a), { stdout })) as unknown as typeof spawnSync;
  assert.throws(() => watchInNewProcess('/tmp/x', answer('EMFILE\n')), (e: NodeJS.ErrnoException) => e.code === 'EMFILE');
  assert.equal(ran[0]![0], process.execPath);
  assert.deepEqual((ran[0]![1] as string[]).at(-1), '/tmp/x');
  assert.doesNotThrow(() => watchInNewProcess('/tmp/x', answer('')));
});

test('it answers on the real machine without throwing', () => {
  const said = machineTrouble('/');
  assert.ok(said === undefined || typeof said === 'string');
});
