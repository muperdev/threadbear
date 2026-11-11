import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { tmpDir } from '../helpers/fakes.ts';

// Security review finding 3 (a), 9 Nov 2026, and Codex's review of it: the
// bring-in thread works a release's merge out with scripts/merge-preview.sh
// before the owner says go, and nothing configured anywhere runs on the
// release's files while it does: not a merge driver an attribute names,
// not merge.default, not one from .git/info/attributes.
const SCRIPT = path.join(import.meta.dirname, '../../../scripts/merge-preview.sh');
const ENV = { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' };
const git = (cwd: string, ...args: string[]) => {
  const r = spawnSync('git', ['-C', cwd, '-c', 'commit.gpgsign=false', ...args], { encoding: 'utf8', env: ENV });
  assert.equal(r.status, 0, r.stderr);
  return r.stdout.trim();
};

test('the merge preview shows clashes and changes, and runs no merge driver from any config or attributes', () => {
  const root = tmpDir('tb-preview-');
  const repo = path.join(root, 'r');
  fs.mkdirSync(repo);
  git(repo, 'init', '-q', '-b', 'main');
  fs.writeFileSync(path.join(repo, '.gitattributes'), '*.sh merge=preview diff=preview\n');
  fs.writeFileSync(path.join(repo, 'a.sh'), 'base\n');
  git(repo, 'add', '.');
  git(repo, 'commit', '-qm', 'base');
  git(repo, 'branch', '-q', 'release');
  fs.writeFileSync(path.join(repo, 'a.sh'), 'theirs\n');
  git(repo, 'commit', '-qam', 'their change');
  git(repo, 'checkout', '-q', 'release');
  fs.writeFileSync(path.join(repo, 'a.sh'), 'mike\n');
  fs.writeFileSync(path.join(repo, 'b.txt'), 'new\n');
  git(repo, 'add', '.');
  git(repo, 'commit', '-qm', "Mike's change");
  const release = git(repo, 'rev-parse', 'HEAD');
  git(repo, 'checkout', '-q', 'main');
  const marker = path.join(root, 'ran');
  // A script, so its path is the whole command: a `;` would start a
  // comment in a config file written by hand (the template's, below).
  const driver = path.join(root, 'driver.sh');
  fs.writeFileSync(driver, `#!/bin/sh\necho ran >> '${marker}'\nexit 1\n`, { mode: 0o755 });
  git(repo, 'config', 'merge.preview.driver', driver);
  git(repo, 'config', 'merge.custom.driver', driver);
  git(repo, 'config', 'merge.default', 'custom');
  git(repo, 'config', 'diff.preview.command', driver);
  git(repo, 'config', 'diff.preview.textconv', driver);
  fs.mkdirSync(path.join(repo, '.git', 'info'), { recursive: true });
  fs.writeFileSync(path.join(repo, '.git', 'info', 'attributes'), '* merge=preview diff=preview\n');
  // And a git template that would copy one into any new repo (Codex's
  // third look: the preview's own throwaway repo).
  const template = path.join(root, 'template');
  fs.mkdirSync(path.join(template, 'info'), { recursive: true });
  fs.writeFileSync(path.join(template, 'config'), `[merge "custom"]\n\tdriver = ${driver}\n[merge]\n\tdefault = custom\n`);
  fs.writeFileSync(path.join(template, 'info', 'attributes'), '* merge=custom\n');
  // Plain git would run it, so the check below means something.
  spawnSync('git', ['-C', repo, 'merge-tree', '--write-tree', 'main', release], { env: ENV });
  assert.ok(fs.existsSync(marker), 'plain merge-tree runs the driver');
  fs.rmSync(marker);
  const r = spawnSync(SCRIPT, [release], { cwd: repo, encoding: 'utf8', env: { ...ENV, GIT_TEMPLATE_DIR: template } });
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /a\.sh/);
  assert.match(r.stdout, /clashes/);
  assert.match(r.stdout, /b\.txt/);
  assert.ok(!fs.existsSync(marker), 'no driver ran');
  assert.equal(git(repo, 'status', '--porcelain'), '', 'nothing checked out or changed');
  assert.equal(git(repo, 'rev-parse', 'HEAD'), git(repo, 'rev-parse', 'main'));
});
