// docs/forking.md lists every file that holds Mike's iPhone team id and
// app group, for a fork shipping its own phone app. A fork that changed
// them by hand (9 Nov 2026) also had to change PushKey.swift and
// install-phone.sh, which the table didn't name, and a wrong app group
// leaves the notification extension unable to open pushes.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const REPO = path.resolve(import.meta.dirname, '..', '..', '..');

function iPhoneRow(): string {
  const doc = fs.readFileSync(path.join(REPO, 'docs', 'forking.md'), 'utf8');
  const row = doc.split('\n').find((l) => l.startsWith('| iPhone bundle ids, app group, team |'));
  assert.ok(row, "forking.md's iPhone row");
  return row;
}

// Tracked files under ios/ with the team id or the app group in them, apart
// from Mike's own App Store listing and notes.
function filesWith(pattern: string): string[] {
  const r = spawnSync('git', ['grep', '-l', '-I', '-E', pattern, '--', 'ios', ':!ios/appstore', ':!*.md'], { cwd: REPO, encoding: 'utf8' });
  assert.ok(r.status === 0 || r.status === 1, r.stderr);
  return r.stdout.split('\n').filter(Boolean);
}

test("forking.md's iPhone row names every file with the team id or the app group", () => {
  const row = iPhoneRow();
  const files = [...new Set([...filesWith('4DTAZSZL6B'), ...filesWith(String.raw`group\.com\.mikecann\.threadbear`)])];
  assert.ok(files.includes('ios/ThreadbearKit/Sources/ThreadbearKit/PushKey.swift'), 'the search finds them');
  const missing = files.filter((f) => !row.includes(`\`${f}\``));
  assert.deepEqual(missing, [], `name these in docs/forking.md's iPhone row: ${missing.join(', ')}`);
});

test("forking.md's iPhone row names every iPhone script that names the app", () => {
  const row = iPhoneRow();
  const r = spawnSync('git', ['grep', '-l', '-I', '-E', String.raw`com\.mikecann\.threadbear([^.a-zA-Z]|$)|4DTAZSZL6B`, '--', 'ios/scripts'], { cwd: REPO, encoding: 'utf8' });
  const files = r.stdout.split('\n').filter(Boolean);
  assert.ok(files.includes('ios/scripts/install-phone.sh'), 'the search finds them');
  const missing = files.filter((f) => !row.includes(`\`${f}\``));
  assert.deepEqual(missing, [], `name these in docs/forking.md's iPhone row: ${missing.join(', ')}`);
});

// And what it names is still there.
test("every file forking.md's iPhone row names exists", () => {
  const named = [...iPhoneRow().matchAll(/`(ios\/[^`]+)`/g)].map((m) => m[1]!);
  assert.ok(named.length >= 8);
  const gone = named.filter((f) => !fs.existsSync(path.join(REPO, f.replace(/\/$/, ''))));
  assert.deepEqual(gone, []);
});
