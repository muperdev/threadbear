// runner/scripts/find-node.sh, which every one of the repo's shell scripts
// uses to pick its Node. Until 9 Nov 2026 each fell back to Mike's own
// ~/.nvm/versions/node/v24.22.1, so in anyone else's copy check.sh and
// tb land stopped at "No Node binary" (a fork's commit said so).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { tmpDir } from '../helpers/fakes.ts';
import { reachOf } from '../../src/script-trust.ts';
import { followOptions } from '../../src/patch-trust.ts';

const REPO = path.resolve(import.meta.dirname, '..', '..', '..');
const FINDER = path.join(REPO, 'runner', 'scripts', 'find-node.sh');

// A home and an empty folder for PATH, so nothing of this Mac's is found
// unless a test puts it there. The finder uses only bash's builtins.
function sandbox() {
  const root = fs.realpathSync(tmpDir());
  const home = path.join(root, 'home');
  const bin = path.join(root, 'bin');
  fs.mkdirSync(home, { recursive: true });
  fs.mkdirSync(bin, { recursive: true });
  return { root, home, bin };
}

function find(env: Record<string, string>) {
  return spawnSync('/bin/bash', ['-c', '. "$1"; threadbear_find_node', 'find-node', FINDER], { encoding: 'utf8', env });
}

function exe(file: string) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, '#!/bin/sh\n', { mode: 0o755 });
  return file;
}

// Homebrew's is the last place it looks, and these tests can't hide it.
const homebrew = ['/opt/homebrew/bin/node', '/usr/local/bin/node'].some((f) => fs.existsSync(f));

test('THREADBEAR_NODE is taken as given, before PATH', () => {
  const s = sandbox();
  exe(path.join(s.bin, 'node'));
  const mine = exe(path.join(s.root, 'elsewhere', 'node24'));
  const r = find({ HOME: s.home, PATH: s.bin, THREADBEAR_NODE: mine });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.stdout, mine);
});

test('a THREADBEAR_NODE that is not a program fails and says so, rather than using another', () => {
  const s = sandbox();
  exe(path.join(s.bin, 'node'));
  for (const bad of [path.join(s.root, 'missing'), s.root]) {
    const r = find({ HOME: s.home, PATH: s.bin, THREADBEAR_NODE: bad });
    assert.equal(r.status, 1);
    assert.equal(r.stdout, '');
    assert.match(r.stderr, new RegExp(`THREADBEAR_NODE is ${bad.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}, which isn't a program`));
  }
});

test('without THREADBEAR_NODE, the node on PATH', () => {
  const s = sandbox();
  const onPath = exe(path.join(s.bin, 'node'));
  exe(path.join(s.home, '.nvm', 'versions', 'node', 'v26.0.0', 'bin', 'node'));
  const r = find({ HOME: s.home, PATH: s.bin });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.stdout, onPath);
});

test("with none on PATH (an ssh session), nvm's newest Node 24 or newer, by number", () => {
  const s = sandbox();
  const nvm = path.join(s.home, '.nvm', 'versions', 'node');
  exe(path.join(nvm, 'v23.11.0', 'bin', 'node'));
  exe(path.join(nvm, 'v24.9.0', 'bin', 'node'));
  const want = exe(path.join(nvm, 'v24.10.0', 'bin', 'node'));
  // Not a Node: no program there, a folder in its place, a name nvm doesn't make.
  fs.mkdirSync(path.join(nvm, 'v30.0.0', 'bin'), { recursive: true });
  fs.mkdirSync(path.join(nvm, 'v31.0.0', 'bin', 'node'), { recursive: true });
  exe(path.join(nvm, 'v99.0.0-rc.1', 'bin', 'node'));
  exe(path.join(nvm, 'v99.0', 'bin', 'node'));
  const r = find({ HOME: s.home, PATH: s.bin });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.stdout, want);
  // NVM_DIR moves it.
  const other = path.join(s.root, 'nvm-elsewhere');
  const there = exe(path.join(other, 'versions', 'node', 'v24.1.0', 'bin', 'node'));
  const r2 = find({ HOME: s.home, PATH: s.bin, NVM_DIR: other });
  assert.equal(r2.stdout, there, r2.stderr);
});

test('only an nvm Node older than 24 is not used', { skip: homebrew && 'Homebrew has a node here' }, () => {
  const s = sandbox();
  exe(path.join(s.home, '.nvm', 'versions', 'node', 'v22.3.0', 'bin', 'node'));
  const r = find({ HOME: s.home, PATH: s.bin });
  assert.equal(r.status, 1);
  assert.equal(r.stdout, '');
  assert.match(r.stderr, /need Node 24 or newer, and found no node on PATH, in nvm or in Homebrew/);
  assert.match(r.stderr, /THREADBEAR_NODE/);
});

test('a script that fails to find one stops, and says what it needs', { skip: homebrew && 'Homebrew has a node here' }, () => {
  const s = sandbox();
  const script = path.join(s.root, 'uses-node.sh');
  fs.writeFileSync(script, `. ${JSON.stringify(FINDER)}\nNODE="$(threadbear_find_node)" || exit 1\necho "ran with $NODE"\n`);
  const r = spawnSync('/bin/bash', [script], { encoding: 'utf8', env: { HOME: s.home, PATH: s.bin } });
  assert.equal(r.status, 1);
  assert.doesNotMatch(r.stdout, /ran with/);
  assert.match(r.stderr, /Node 24 or newer/);
});

// Patch trust follows what a script runs (worktree-setup.sh runs by itself
// when a thread starts). The finder must never be what it can't follow: no
// command named by a variable, no node -e, no process substitution. The
// scripts that source it each had a problem of their own before it (check.sh
// runs "$NODE", worktree-setup.sh a package script after a cd), so what's
// checked for them is that it's followed and hashed, and adds nothing.
test('patch trust follows find-node.sh all the way, and it adds no problem to the scripts that source it', () => {
  const r = reachOf(FINDER, { ...followOptions(REPO), folder: REPO });
  assert.equal(r.problem, undefined, r.problem ?? "");
  for (const script of ['worktree-setup.sh', 'check.sh', 'runner/scripts/deploy-runner.sh']) {
    const reach = reachOf(path.join(REPO, script), { ...followOptions(REPO), folder: REPO });
    assert.ok(reach.files.includes(FINDER), `${script} sources it, and the hash covers it`);
    assert.doesNotMatch(reach.problem ?? '', /find-node\.sh/, `${script}: ${reach.problem ?? ''}`);
  }
  // A script that only sources it and runs nothing else is followed with no
  // problem, so nothing in it can be one.
  const root = fs.realpathSync(tmpDir());
  fs.mkdirSync(path.join(root, 'runner', 'scripts'), { recursive: true });
  fs.copyFileSync(FINDER, path.join(root, 'runner', 'scripts', 'find-node.sh'));
  fs.writeFileSync(path.join(root, 'setup.sh'), '#!/bin/bash\nset -euo pipefail\nROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"\n. "$ROOT/runner/scripts/find-node.sh"\nNODE="$(threadbear_find_node)" || exit 1\necho "$NODE"\n', { mode: 0o755 });
  spawnSync('git', ['init', '-q', root]);
  const only = reachOf(path.join(root, 'setup.sh'), { ...followOptions(root), folder: root });
  assert.equal(only.problem, undefined, only.problem ?? '');
  assert.ok(only.files.includes(path.join(root, 'runner', 'scripts', 'find-node.sh')));
});

// No script, test or doc pins a Node to one machine's nvm version again.
// docs/step4-plan.md is history (where Codex was on 1 Nov), and
// update.test.ts uses such a path as a sample of a binary that isn't an app.
test("nothing in the repo names one nvm Node version's path", () => {
  const r = spawnSync('git', ['grep', '-n', '-I', '-E', String.raw`\.nvm/versions/node/v[0-9]+\.[0-9]+\.[0-9]+/bin`, '--', '.',
    ':!docs/step4-plan.md', ':!runner/test/unit/update.test.ts', ':!runner/test/unit/find-node.test.ts'], { cwd: REPO, encoding: 'utf8' });
  assert.equal(r.stdout, '', `pinned Node paths:\n${r.stdout}`);
  assert.equal(r.status, 1, r.stderr);
});
