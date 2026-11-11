// scripts/check-remote.sh against fake hosts (helpers/check-remote-harness.ts
// says how, and why it's several files): losing the race for a host, what gets copied, the copy lock and the shared ssh connection.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { GiB, setup, shortHome } from '../helpers/check-remote-harness.ts';

test('a slow copy that loses the race waits on its host with the same copy, even with another free', () => {
  const s = setup({ h1: { state: 'free', lose: 1 }, h2: { state: 'free' } });
  const r = s.run('h1,h2', 0);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stdout, /ran on h1 after waiting from .*\/runs\/[^/]+-0\/scripts/);
  assert.match(r.stdout, /waiting for it there/);
  assert.deepEqual(r.copies, ['h1']);
  // h1 was free first, so h2 was never even probed.
  assert.doesNotMatch(r.ssh, /^h2 /m);
});

test('a quick copy that loses the race chooses again and reuses the copy on the same host', () => {
  const s = setup({ h1: { state: 'free', lose: 1 } });
  const r = s.run('h1', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stdout, /its copy is already there/);
  assert.match(r.stdout, /ran on h1 after waiting from .*-0\/scripts/);
  assert.deepEqual(r.copies, ['h1']);
  assert.deepEqual(r.left('h1'), []);
});

test('a quick copy that loses the race moves to a free host and deletes the first copy', () => {
  const s = setup({ h1: { state: 'free', lose: 1 }, h2: { state: 'free' } });
  const r = s.run('h1,h2', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stdout, /ran on h2 now from .*\/h2\/home\/.*-1\/scripts/);
  assert.deepEqual(r.copies, ['h1', 'h2']);
  assert.deepEqual(r.left('h1'), []);
  assert.deepEqual(r.left('h2'), []);
});

test('losing three times in a row waits in line rather than trying forever', () => {
  const s = setup({ h1: { state: 'free', lose: 5, looksFree: true } });
  const r = s.run('h1', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal((r.stdout.match(/choosing again/g) ?? []).length, 3);
  assert.match(r.stdout, /ran on h1 after waiting/);
  assert.deepEqual(r.copies, ['h1']);
});

test('the copy carries the checkout, untracked files included, and .check-changed', () => {
  const s = setup({ h1: { state: 'free' } });
  fs.writeFileSync(path.join(s.repo, 'untracked.txt'), 'u\n');
  fs.writeFileSync(path.join(s.repo, 'ignored.log'), 'i\n');
  fs.writeFileSync(path.join(s.repo, '.gitignore'), '*.log\n');
  fs.writeFileSync(path.join(s.repo, 'tool.sh'), '#!/bin/sh\n', { mode: 0o755 });
  fs.symlinkSync('a.txt', path.join(s.repo, 'link'));
  // docs/ and site/ pictures stay behind; site's JSON goes (TourTests reads it).
  fs.mkdirSync(path.join(s.repo, 'docs'));
  fs.mkdirSync(path.join(s.repo, 'site'));
  fs.writeFileSync(path.join(s.repo, 'docs', 'shot.png'), 'png');
  fs.writeFileSync(path.join(s.repo, 'site', 'tour.json'), '{}');
  const r = s.run('h1', 3600, 'find . -mindepth 1 | sed "s#^\\./##" | sort; [ -x tool.sh ] && echo "tool.sh runs"; echo "link -> $(readlink link)"');
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.deepEqual(r.copies, ['h1']);
  const listed = r.peek.trim().split('\n');
  for (const f of ['a.txt', 'untracked.txt', '.gitignore', 'tool.sh', 'link', 'scripts', 'scripts/check-remote.sh', '.check-changed',
    'tool.sh runs', 'link -> a.txt', 'site/tour.json']) {
    assert.ok(listed.includes(f), `${f} in ${listed.join(',')}`);
  }
  assert.ok(!listed.includes('ignored.log'), listed.join(','));
  assert.ok(!listed.includes('docs/shot.png'), listed.join(','));
});

test('a .check-changed in the checkout never has the real one written through it, and modes are kept', () => {
  const s = setup({ h1: { state: 'free' } });
  const victim = path.join(s.fake, 'victim');
  fs.writeFileSync(victim, 'untouched\n');
  fs.symlinkSync(victim, path.join(s.repo, '.check-changed'));
  fs.writeFileSync(path.join(s.repo, 'shared.sh'), '#!/bin/sh\n');
  fs.chmodSync(path.join(s.repo, 'shared.sh'), 0o775);
  const r = s.run('h1', 3600, '[ -L .check-changed ] && echo "is a link"; cat .check-changed; ls -l shared.sh | cut -c1-10');
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(fs.readFileSync(victim, 'utf8'), 'untouched\n');
  assert.doesNotMatch(r.peek, /is a link/);
  // The changed list (here the untracked files) is what's there.
  assert.match(r.peek, /^shared\.sh$/m);
  // Modes as they are here, not as the receiver's umask would leave them.
  assert.match(r.peek, /-rwxrwxr-x/);
});

// Daybreak, 7 Nov 2026. The folder a host reports for its cache is that
// host's word: it's used only if it's a plain absolute path (letters,
// digits, . _ / -, no . or .. parts) ending in .cache/threadbear-check, and
// it's never part of a program run here.
test("a host whose reported folder isn't a plain absolute path is passed over", () => {
  const s = setup({ h1: { state: 'free' } });
  const odd = ['relative/.cache/threadbear-check', `${s.fake}/x y/.cache/threadbear-check`, `${s.fake}/x'y/.cache/threadbear-check`,
    `${s.fake}/a#b/.cache/threadbear-check`, `${s.fake}/a/../.cache/threadbear-check`, `${s.fake}/a/./.cache/threadbear-check`, `${s.fake}/é/.cache/threadbear-check`];
  for (const base of odd) {
    fs.writeFileSync(path.join(s.fake, 'h1', 'base'), base);
    fs.rmSync(path.join(s.fake, 'copies.log'), { force: true });
    const r = s.run('h1', 3600);
    assert.equal(r.status, 255, `${base}: ${r.stdout}${r.stderr}`);
    assert.match(r.stderr, /Unexpected folder on h1/, base);
    assert.deepEqual(r.copies, [], base);
  }
});

test('a host passed over for its folder leaves the next one to run the check', () => {
  const s = setup({ h1: { state: 'free', base: '/x y/.cache/threadbear-check' }, h2: { state: 'free' } });
  const r = s.run('h1,h2', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stdout, /ran on h2 now/);
  assert.deepEqual(r.copies, ['h2']);
});

test("a failed step's log path is changed to this Mac's copy as plain text", () => {
  const s = setup({ h1: { state: 'free' } });
  const r = s.run('h1', 3600, '', { PRINT_LOG: '1' });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  const local = path.join(s.tmpdir, 'threadbear-check-remote.');
  const log = r.stdout.split('\n').find((l) => l.startsWith('log at '));
  assert.ok(log?.startsWith(`log at ${local}`) && log.endsWith('/step.log'), r.stdout);
  // Only that exact folder: one that differs where a pattern's "." would
  // have matched anything is left as it is.
  const near = r.stdout.split('\n').find((l) => l.startsWith('near '));
  assert.match(near ?? '', /\/Xcache\/threadbear-check\/tmp-[0-9-]+\/threadbear-check\.Ab12\/step\.log$/, r.stdout);
});

// The copy lock: its file in a folder of this user's own, mode 700, and the
// helper's ready and release files in a folder mktemp -d made for this run,
// never names made up in the shared TMPDIR.
const LIST_LOCKS = 'for f in "$TMPDIR"/* "$HOME/.cache/threadbear-check-locks" "$HOME/.cache/threadbear-check-locks"/* "$TMPDIR"/tb-check-lock.*/*; do [ -e "$f" ] || [ -L "$f" ] || continue; ls -ld "$f" | awk \'{print $1}\' | tr "\\n" " "; echo "$f"; done';

test("the copy lock's files are in folders only this user can use", () => {
  const s = setup({ h1: { state: 'free' } });
  const r = s.run('h1', 3600, '', { RSYNC_PEEK: LIST_LOCKS });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  const held = r.rsyncPeek.trim().split('\n').filter(Boolean).map((l) => {
    const [mode, ...rest] = l.split(' ');
    return { mode, file: rest.join(' ') };
  });
  const lockDir = path.join(s.home, '.cache', 'threadbear-check-locks');
  assert.ok(held.some((h) => h.file === lockDir && h.mode.startsWith('drwx------')), r.rsyncPeek);
  assert.ok(held.some((h) => h.file.startsWith(`${lockDir}/copy-`) && h.file.endsWith('-h1.lock')), r.rsyncPeek);
  // Everything of the lock's in TMPDIR is a private folder of this run's.
  for (const h of held.filter((x) => x.file.startsWith(`${s.tmpdir}/tb-check`))) {
    if (path.dirname(h.file) === s.tmpdir) {
      assert.match(path.basename(h.file), /^tb-check-lock\.[A-Za-z0-9]+$/, r.rsyncPeek);
      assert.ok(h.mode.startsWith('drwx------'), r.rsyncPeek);
    } else assert.match(path.dirname(h.file), /\/tb-check-lock\.[A-Za-z0-9]+$/, r.rsyncPeek);
  }
  assert.ok(held.some((h) => /\/tb-check-lock\.[A-Za-z0-9]+$/.test(h.file)), r.rsyncPeek);
  // And the run's folder goes with it.
  assert.deepEqual(fs.readdirSync(s.tmpdir).filter((f) => f.startsWith('tb-check')), []);
});

test("a lock folder that isn't this user's own, mode 700, is never used or repaired", () => {
  for (const make of ['open', 'link'] as const) {
    const s = setup({ h1: { state: 'free' } });
    const lockDir = path.join(s.home, '.cache', 'threadbear-check-locks');
    fs.mkdirSync(path.dirname(lockDir), { recursive: true });
    const elsewhere = path.join(s.home, 'elsewhere');
    fs.mkdirSync(elsewhere, { mode: 0o700 });
    if (make === 'open') {
      fs.mkdirSync(lockDir);
      fs.chmodSync(lockDir, 0o755);
    } else fs.symlinkSync(elsewhere, lockDir);
    const r = s.run('h1', 3600);
    assert.equal(r.status, 1, `${make}: ${r.stdout}${r.stderr}`);
    assert.match(r.stderr, /Couldn't take the lock on this checkout's copy/, make);
    assert.deepEqual(r.copies, [], make);
    if (make === 'open') assert.equal(fs.statSync(lockDir).mode & 0o777, 0o755, 'left as it was');
    assert.deepEqual(fs.readdirSync(elsewhere), []);
  }
});

// ssh's shared connection: its socket in ~/.ssh/tbc, a folder of this
// user's own, never a name in /tmp another account could make first.
test("ssh shares a connection through a socket in this user's own folder", () => {
  const s = setup({ h1: { state: 'free' } });
  const r = s.run('h1', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  const dir = path.join(s.home, '.ssh', 'tbc');
  assert.ok(r.sshOpts.split('\n').includes(`ControlPath=${dir}/%C`), r.sshOpts);
  assert.ok(r.sshOpts.split('\n').includes('ControlMaster=auto'), r.sshOpts);
  assert.doesNotMatch(r.sshOpts, /\/tmp\/tbc-/);
  const st = fs.lstatSync(dir);
  assert.ok(st.isDirectory() && (st.mode & 0o777) === 0o700 && st.uid === process.getuid!());
});

test('without a short private folder for the socket, each ssh connects on its own', () => {
  const longHome = path.join(shortHome(), 'a-folder-name-long-enough-for-the-socket-path');
  fs.mkdirSync(longHome);
  const cases: [string, (home: string) => void][] = [
    ['open', (home) => fs.mkdirSync(path.join(home, '.ssh', 'tbc'), { recursive: true, mode: 0o755 })],
    ['link', (home) => {
      fs.mkdirSync(path.join(home, '.ssh'), { mode: 0o700 });
      fs.mkdirSync(path.join(home, 'real'), { mode: 0o700 });
      fs.symlinkSync(path.join(home, 'real'), path.join(home, '.ssh', 'tbc'));
    }],
    ['long', () => undefined],
  ];
  for (const [what, make] of cases) {
    const home = what === 'long' ? longHome : shortHome();
    make(home);
    const s = setup({ h1: { state: 'free' } }, { home });
    const r = s.run('h1', 3600);
    assert.equal(r.status, 0, `${what}: ${r.stdout}${r.stderr}`);
    const opts = r.sshOpts.split('\n');
    assert.ok(opts.includes('ControlMaster=no') && opts.includes('ControlPath=none'), `${what}: ${r.sshOpts}`);
    assert.ok(!opts.some((o) => o.startsWith('ControlPath=/')), `${what}: ${r.sshOpts}`);
    assert.match(r.stderr, /Not sharing one ssh connection per host/, what);
    if (what === 'open') assert.equal(fs.statSync(path.join(home, '.ssh', 'tbc')).mode & 0o777, 0o755, 'left as it was');
    if (what === 'long') assert.ok(!fs.existsSync(path.join(home, '.ssh', 'tbc')));
  }
});

test('a home with a newline in its name gets no shared connection, even with its folder ready', () => {
  const home = path.join(shortHome(), 'a\nb');
  fs.mkdirSync(path.join(home, '.ssh', 'tbc'), { recursive: true, mode: 0o700 });
  fs.chmodSync(path.join(home, '.ssh'), 0o700);
  fs.chmodSync(path.join(home, '.ssh', 'tbc'), 0o700);
  const s = setup({ h1: { state: 'free' } }, { home });
  const r = s.run('h1', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  const opts = r.sshOpts.split('\n');
  assert.ok(opts.includes('ControlMaster=no') && opts.includes('ControlPath=none'), r.sshOpts);
});

// 7 Nov 2026: Tinker Desk fell under 1 GiB and an --app check died with a
// CodeSign error that said nothing about the disk. A host without room is
// passed over before anything's copied, and a failure says when room's short.
// Something changed, so a --quick run has no Swift to build (with nothing
// changed, check.sh runs everything).
