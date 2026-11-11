// A move's git in a repo whose .git came with it (an archive, a shared
// drive), with a config that names programs for git to run: filters
// picked by its .gitattributes, hooks, a signing program, an fsmonitor, a
// remote's upload-pack. None may run at any step of a move (security
// review, 10 Nov 2026: a move's preview ran `git status` in a shared
// checkout and the repo's clean filter ran as the owner).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { changedFiles, cloneableUrl, originUrl, packGit, repoTop, unpackGit } from '../../src/move-git.ts';
import { parseConfigList, repoProofConfig, untrustedGit } from '../../src/safe-git.ts';

const tmp = () => fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-move-planted-')));
const git = (cwd: string, ...args: string[]) => execFileSync('git', ['-C', cwd, '-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'init.defaultBranch=main', ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
const write = (dir: string, file: string, text: string) => {
  fs.mkdirSync(path.dirname(path.join(dir, file)), { recursive: true });
  fs.writeFileSync(path.join(dir, file), text);
};

// Programs that note they ran in `marker`, by name, and pass input through.
function programs(root: string) {
  const marker = path.join(root, 'ran.log');
  const prog = (name: string, body = 'cat') => {
    const f = path.join(root, 'bin', name);
    fs.mkdirSync(path.dirname(f), { recursive: true });
    fs.writeFileSync(f, `#!/bin/sh\necho ${name} >> '${marker}'\n${body}\n`, { mode: 0o755 });
    return f;
  };
  const ran = () => (fs.existsSync(marker) ? fs.readFileSync(marker, 'utf8').split('\n').filter(Boolean) : []);
  return { prog, ran, marker };
}

// Plants every program a repo's config can name for the commands a move
// runs, in `repo` (its .git/config and .gitattributes) and its hooks.
function plant(repo: string, prog: (name: string, body?: string) => string) {
  const hooks = path.join(path.dirname(prog('x')), 'hooks');
  fs.mkdirSync(hooks, { recursive: true });
  for (const h of ['reference-transaction', 'post-index-change', 'post-checkout', 'pre-auto-gc', 'post-merge']) {
    for (const dir of [hooks, path.join(git(repo, 'rev-parse', '--git-common-dir').replace(/^(?!\/)/, `${repo}/`), 'hooks')]) {
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(path.join(dir, h), `#!/bin/sh\nexec '${prog(`hook-${h}`, 'cat >/dev/null; exit 0')}'\n`, { mode: 0o755 });
    }
  }
  const set = (k: string, v: string) => git(repo, 'config', k, v);
  set('filter.evil.clean', prog('clean'));
  set('filter.evil.smudge', prog('smudge'));
  set('filter.evil.required', 'true');
  // A long-running process filter, as git-lfs's is.
  set('filter.evilproc.process', prog('process', 'exit 1'));
  set('diff.evil.textconv', prog('textconv'));
  set('core.fsmonitor', prog('fsmonitor', 'exit 1'));
  set('core.hooksPath', hooks);
  set('commit.gpgSign', 'true');
  set('gpg.program', prog('gpg', 'exit 1'));
  set('remote.origin.uploadpack', prog('uploadpack', 'exit 1'));
  set('core.askPass', prog('askpass', 'exit 1'));
  set('credential.helper', `!${prog('credential', 'exit 1')}`);
  set('core.alternateRefsCommand', prog('alternate-refs', 'exit 0'));
  set('submodule.recurse', 'true');
}

// origin, S's checkout and T's, each with programs planted in its config.
function repos() {
  const root = tmp();
  const { prog, ran } = programs(root);
  const seed = path.join(root, 'seed');
  fs.mkdirSync(seed);
  git(seed, 'init', '-q');
  write(seed, '.gitattributes', 'filtered.txt filter=evil diff=evil\nproc.txt filter=evilproc\n');
  write(seed, 'filtered.txt', 'one\n');
  write(seed, 'proc.txt', 'one\n');
  write(seed, 'plain.txt', 'one\n');
  git(seed, 'add', '-A');
  git(seed, 'commit', '-qm', 'A');
  const origin = path.join(root, 'origin.git');
  execFileSync('git', ['clone', '-q', '--bare', seed, origin]);
  const s = path.join(root, 's');
  const t = path.join(root, 't');
  execFileSync('git', ['clone', '-q', origin, s]);
  execFileSync('git', ['clone', '-q', origin, t]);
  return { root, origin, s, t, prog, ran };
}

test('the planted programs really run under plain git, so the checks below mean something', () => {
  const { s, t, prog, ran } = repos();
  plant(s, prog);
  plant(t, prog);
  write(s, 'filtered.txt', 'two\n');
  write(s, 'proc.txt', 'two\n');
  try {
    execFileSync('git', ['-C', s, 'status', '--porcelain'], { stdio: 'pipe' });
  } catch {
    // The process filter fails; what matters is that git started them.
  }
  try {
    execFileSync('git', ['-C', t, 'fetch', 'origin'], { stdio: 'pipe' });
  } catch {
    // upload-pack is planted to fail
  }
  for (const p of ['clean', 'process', 'fsmonitor', 'uploadpack']) assert.ok(ran().includes(p), `${p} ran: ${ran().join(', ')}`);
});

test("an archive's planted clean filter doesn't run when a move's preview looks for uncommitted changes", async () => {
  const { root, s, prog, ran } = repos();
  plant(s, prog);
  // The whole repo, .git and all, through a tarball, as Daybreak's proof
  // did: the extracted index's stat data is stale, so git rehashes.
  const tarball = path.join(root, 'repo.tgz');
  execFileSync('tar', ['-czf', tarball, '-C', root, 's']);
  const out = path.join(root, 'extracted');
  fs.mkdirSync(out);
  execFileSync('tar', ['-xzf', tarball, '-C', out]);
  const repo = path.join(out, 's');
  assert.equal(await repoTop(repo), repo);
  assert.ok((await originUrl(repo))?.endsWith('origin.git'));
  write(repo, 'untracked.txt', 'new\n');
  assert.deepEqual(await changedFiles(repo), ['untracked.txt']);
  // A tracked file that changed too: still found, still no program.
  write(repo, 'filtered.txt', 'two\n');
  write(repo, 'proc.txt', 'two\n');
  assert.deepEqual((await changedFiles(repo)).sort(), ['filtered.txt', 'proc.txt', 'untracked.txt']);
  assert.deepEqual(ran(), []);
});

test("packing and unpacking a worktree run none of either repo's programs, and the work still comes across", async () => {
  const { root, s, t, prog, ran } = repos();
  // origin moves on after T cloned it: T has to fetch origin for the
  // bundle's base, through its own planted upload-pack unless it's the owner's.
  write(s, 'plain.txt', 'two\n');
  git(s, 'commit', '-qam', 'A2');
  git(s, 'push', '-q', 'origin', 'main');
  const a2 = git(s, 'rev-parse', 'HEAD');
  const sw = path.join(root, 's-threads/slot-1');
  git(s, 'worktree', 'add', '-q', '-b', 'thread/abcd1234', sw, 'main');
  const tw = path.join(root, 't-threads/slot-1');
  git(t, 'worktree', 'add', '-q', '-b', 'thread/abcd1234', tw, 'main');
  write(sw, 'filtered.txt', 'committed\n');
  git(sw, 'commit', '-qam', 'B');
  plant(s, prog);
  plant(t, prog);
  // The local origin T fetches from, with programs of its own for its
  // upload-pack to run.
  const { origin } = { origin: path.join(root, 'origin.git') };
  git(origin, 'config', 'uploadpack.packObjectsHook', prog('pack-objects-hook', 'exit 1'));
  git(origin, 'config', 'core.fsmonitor', prog('origin-fsmonitor', 'exit 1'));
  write(sw, 'filtered.txt', 'uncommitted\n');
  write(sw, 'proc.txt', 'uncommitted\n');
  write(sw, 'new.txt', 'untracked\n');

  const bundle = path.join(root, 'move.bundle');
  const packed = await packGit(sw, sw, bundle);
  assert.ok(packed.ok, !packed.ok ? packed.reason : '');
  if (!packed.ok) return;
  assert.ok(packed.meta.wip);
  const r = await unpackGit(tw, packed.meta, bundle);
  assert.ok(r.ok, !r.ok ? r.reason : '');
  assert.equal(packed.meta.base, a2);
  assert.equal(git(t, 'cat-file', '-t', a2), 'commit', 'T fetched origin for it');
  assert.equal(fs.readFileSync(path.join(tw, 'filtered.txt'), 'utf8'), 'uncommitted\n');
  assert.equal(fs.readFileSync(path.join(tw, 'proc.txt'), 'utf8'), 'uncommitted\n');
  assert.equal(fs.readFileSync(path.join(tw, 'new.txt'), 'utf8'), 'untracked\n');
  assert.deepEqual(ran(), []);
});

test('git config in the runner\'s environment can\'t put a filter back for a move', async () => {
  const { s, prog, ran } = repos();
  plant(s, prog);
  write(s, 'filtered.txt', 'two\n');
  const saved = { ...process.env };
  try {
    Object.assign(process.env, { GIT_CONFIG_COUNT: '1', GIT_CONFIG_KEY_0: 'filter.evil.clean', GIT_CONFIG_VALUE_0: prog('clean'), GIT_CONFIG_PARAMETERS: `'filter.evil.clean'='${prog('clean')}'` });
    assert.deepEqual(await changedFiles(s), ['filtered.txt']);
  } finally {
    for (const k of Object.keys(process.env)) if (!(k in saved)) delete process.env[k];
    Object.assign(process.env, saved);
  }
  assert.deepEqual(ran(), []);
});

test("a filter name git's -c can't reach fails closed rather than running git", async () => {
  const { s, prog, ran } = repos();
  git(s, 'config', 'filter.a=b.clean', prog('clean'));
  write(s, '.gitattributes', 'filtered.txt filter=a=b\n');
  write(s, 'filtered.txt', 'two\n');
  const r = await untrustedGit(s, ['status', '--porcelain']);
  assert.equal(r.ok, false);
  assert.match(r.stderr, /can't switch off/);
  assert.deepEqual(await changedFiles(s), []);
  assert.deepEqual(ran(), []);
});

test("the owner's own transport programs stay, the repo's are swapped for them", () => {
  const list = [
    'global\0credential.https://github.com.helper\n',
    'global\0credential.https://github.com.helper\n!gh auth git-credential',
    'global\0filter.lfs.process\ngit-lfs filter-process',
    'local\0credential.helper\n!evil',
    'local\0core.sshcommand\nevil-ssh',
    '',
  ].join('\0');
  const c = repoProofConfig(parseConfigList(list), 'remote');
  const sets = c.filter((_, i) => c[i - 1] === '-c');
  const at = (s: string) => sets.indexOf(s);
  // The list emptied, then the owner's helpers again in their order.
  assert.ok(at('credential.helper=') >= 0 && at('credential.https://github.com.helper=') > at('credential.helper='));
  assert.ok(at('credential.https://github.com.helper=!gh auth git-credential') > at('credential.https://github.com.helper='));
  assert.ok(!sets.some((s) => s.includes('evil')));
  assert.ok(sets.includes('core.sshcommand=ssh'));
  // Even the owner's git-lfs filter is off: lfs runs what a repo names too.
  assert.ok(sets.includes('filter.lfs.process=') && sets.includes('filter.lfs.required=false'));
  // No network unless asked for, and then only https, ssh and file.
  const quiet = repoProofConfig(parseConfigList(list)).filter((_, i, a) => a[i - 1] === '-c');
  assert.ok(quiet.includes('protocol.file.allow=never') && quiet.includes('protocol.https.allow=never'));
  assert.ok(sets.includes('protocol.ext.allow=never') && sets.includes('protocol.https.allow=always'));
  // Without `remote`, the transport programs aren't touched: nothing fetches.
  assert.ok(!quiet.includes('core.sshcommand=ssh'));
  // A protocol the repo turns on by name is turned off by name.
  const named = repoProofConfig(parseConfigList(['local\0protocol.foo.allow\nalways', 'local\0protocol.ftp.allow\nalways', 'local\0protocol.https.allow\nnever', ''].join('\0')), 'remote').filter((_, i, a) => a[i - 1] === '-c');
  assert.ok(named.includes('protocol.foo.allow=never') && named.includes('protocol.ftp.allow=never'));
  assert.ok(named.indexOf('protocol.https.allow=always') >= 0);
});

test('a moving thread\'s repo is cloned only from an https or ssh address', () => {
  for (const u of ['https://github.com/mikecann/threadbear.git', 'git@github.com:mikecann/threadbear.git', 'ssh://git@github.com/mikecann/threadbear', 'ssh://git@ssh.github.com:443/o/r.git', 'https://gitlab.example.com/group/sub/r', 'git@github_work:mikecann/x.git']) assert.ok(cloneableUrl(u), u);
  for (const u of ['--upload-pack=touch /tmp/x', '-uevil', 'ext::sh -c touch% /tmp/x', 'file:///etc', '/Users/mike/secret', 'git@github.com:-oProxyCommand=x', 'https://-oProxyCommand=x/y', 'ssh://git@-oProxyCommand=touch${IFS}/tmp/pwn/repo', 'ssh://-git@host/x', '-git@host:x', 'https://github.com', 'ssh://git@host:22/x\\y', 'fd::17', 'http://example.com/x', 'git@host:a b', '']) assert.equal(cloneableUrl(u), false, u);
});

// Codex's review, 10 Nov 2026: packages in a move's uncommitted work would
// be untracked here, which trust reads as installed and doesn't hash.
test("a move whose uncommitted work brings files under node_modules doesn't unpack", async () => {
  const { root, s, t } = repos();
  const sw = path.join(root, 's-threads/slot-1');
  git(s, 'worktree', 'add', '-q', '-b', 'thread/abcd1234', sw, 'main');
  const tw = path.join(root, 't-threads/slot-1');
  git(t, 'worktree', 'add', '-q', '-b', 'thread/abcd1234', tw, 'main');
  write(sw, 'node_modules/p/index.js', 'evil()\n');
  write(sw, 'plain.txt', 'changed\n');
  const packed = await packGit(sw, sw, path.join(root, 'move.bundle'));
  assert.ok(packed.ok, !packed.ok ? packed.reason : '');
  if (!packed.ok) return;
  const r = await unpackGit(tw, packed.meta, packed.bundle);
  assert.equal(r.ok, false);
  assert.match(!r.ok ? r.reason : '', /node_modules\/p\/index\.js, under node_modules/);
  assert.equal(fs.existsSync(path.join(tw, 'node_modules', 'p', 'index.js')), false);
});
