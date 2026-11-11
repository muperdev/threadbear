// worktrees: the patch, and a new thread getting a worktree (helpers/worktrees-harness.ts says why it's several files).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { makeHarness, tmpDir } from '../helpers/fakes.ts';
import { checkSequence, checkSummary } from '../helpers/protocol-check.ts';
import { findWorktreePatch, readPatchModule } from '../../src/patches.ts';
import { git, land, makeWorktree } from '../../src/worktrees.ts';
import { RpcServer } from '../../src/rpc.ts';
import { RunnerClient } from '../../src/client.ts';
import { readToken } from '../../src/config.ts';
import { nullLogger } from '../../src/log.ts';
import { PatchTrust } from '../../src/patch-trust.ts';
import { PATCH, commit, harness, makeRepo, sh, until, write } from '../helpers/worktrees-harness.ts';

// --------------------------------------------------------------- patches

test('a patch can carry worktree with setup and check; anything malformed is the file\'s error', () => {
  const ok = readPatchModule('checks', 'project', '/x/checks.ts', { worktree: { setup: 'npm ci', check: './check.sh --quick' } });
  assert.equal(ok.info.error, undefined);
  assert.deepEqual(ok.worktree, { setup: 'npm ci', check: './check.sh --quick' });
  assert.deepEqual(ok.actions, []);
  assert.deepEqual(readPatchModule('w', 'project', '/x/w.ts', { worktree: {} }).worktree, {});
  assert.deepEqual(readPatchModule('k', 'project', '/x/k.ts', { worktree: { keep: ['app/.build', 'dock/.build'] } }).worktree, { keep: ['app/.build', 'dock/.build'] });
  assert.deepEqual(readPatchModule('t', 'project', '/x/t.ts', { worktree: { teardown: './scripts/teardown.sh' } }).worktree, { teardown: './scripts/teardown.sh' });
  assert.equal(readPatchModule('a', 'project', '/x/a.ts', { actions: [] }).worktree, undefined, 'none unless asked for');
  for (const [worktree, error] of [
    [[], 'worktree should be an object with setup and/or check.'],
    [true, 'worktree should be an object with setup and/or check.'],
    [{ setup: 3 }, 'worktree.setup should be a command.'],
    [{ check: '  ' }, 'worktree.check should be a command.'],
    [{ teardown: 5 }, 'worktree.teardown should be a command.'],
    [{ keep: 'app/.build' }, 'worktree.keep should be a list of folders inside the repo, like "app/.build".'],
    [{ keep: [] }, 'worktree.keep should be a list of folders inside the repo, like "app/.build".'],
    [{ keep: ['/abs/.build'] }, 'worktree.keep should be a list of folders inside the repo, like "app/.build".'],
    [{ keep: ['../up'] }, 'worktree.keep should be a list of folders inside the repo, like "app/.build".'],
  ] as const) {
    const bad = readPatchModule('b', 'project', '/x/b.ts', { worktree });
    assert.equal(bad.info.error, error);
    assert.equal(bad.worktree, undefined);
  }
});

test('the nearest patch with a worktree wins; home\'s patches and broken files never count', async () => {
  const home = fs.realpathSync(tmpDir());
  write(home, {
    '.threadbear/patches/everywhere.ts': PATCH(`{ setup: "from home" }`),
    'dev/repo/.threadbear/patches/a-actions.ts': `export default { actions: [{ title: "Hi", run: "echo hi" }] };`,
    'dev/repo/.threadbear/patches/b-broken.ts': `export default { worktree: { setup: 4 } };`,
    'dev/repo/.threadbear/patches/c-wt.ts': PATCH(`{ setup: "outer", check: "outer check" }`),
    'dev/repo/pkg/.threadbear/patches/inner.ts': PATCH(`{ check: "inner check" }`),
    'dev/plain/x.txt': '',
  });
  const inner = await findWorktreePatch(path.join(home, 'dev/repo/pkg'), home);
  assert.deepEqual(inner, { check: 'inner check', patch: 'inner', file: path.join(home, 'dev/repo/pkg/.threadbear/patches/inner.ts'), folder: path.join(home, 'dev/repo/pkg') });
  const outer = await findWorktreePatch(path.join(home, 'dev/repo'), home);
  assert.equal(outer?.patch, 'c-wt');
  assert.equal(outer?.setup, 'outer');
  assert.equal(outer?.folder, path.join(home, 'dev/repo'));
  assert.equal(await findWorktreePatch(path.join(home, 'dev/plain'), home), undefined, "home's own patch folder isn't a repo's choice");
});

// ----------------------------------------------------------- a new thread

test('a new thread in an opted-in repo gets its own worktree, cwd, setup run, brief and Land chip', async () => {
  const { root, main } = makeRepo();
  const h = harness();
  const { thread, turnId } = h.runner.create({ text: 'Fix it', accountId: 'claude-3', cwd: path.join(main, 'runner') });
  const id8 = thread.id.slice(0, 8);
  const wt = path.join(root, 'proj-threads', id8);
  // The CLI starts once the worktree is there and set up.
  await until(() => h.adapter.sessions.length === 1, 'the CLI to start');
  const s = h.adapter.last;
  assert.equal(s.opts.cwd, path.join(wt, 'runner'), 'the same subfolder of the worktree');
  assert.match(s.opts.instructions ?? '', new RegExp(`own git worktree of proj at ${wt}, on the branch thread/${id8}, made from main`));
  assert.match(s.opts.instructions ?? '', /run `tb land` in the worktree/);
  assert.equal(sh(wt, 'git rev-parse --abbrev-ref HEAD'), `thread/${id8}`);
  assert.equal(sh(wt, 'git rev-parse HEAD'), sh(main, 'git rev-parse main'));
  assert.ok(fs.existsSync(path.join(wt, '.setup-ran')), 'setup ran in the worktree root');

  const events = h.events(thread.id);
  assert.deepEqual(events.map((e) => e.type), ['user.message', 'turn.started', 'system.note', 'action.run']);
  assert.deepEqual(checkSequence(events), []);
  assert.equal(events[2].text, `Working in a worktree of its own at ${wt}, on the branch thread/${id8}. tb land puts it on main.`);
  const setup = events[3];
  assert.equal(setup.turnId, turnId);
  assert.equal(setup.trigger, 'worktree.setup');
  assert.equal(setup.title, 'Set up the worktree');
  assert.equal(setup.patch, 'checks');
  assert.equal(setup.ok, true);
  assert.match(setup.text, new RegExp(`setting up in ${wt}\n`));

  const summary = h.runner.get(thread.id);
  assert.deepEqual(checkSummary(summary), []);
  assert.equal(summary.cwd, path.join(wt, 'runner'));
  assert.deepEqual(summary.worktree, { branch: `thread/${id8}`, mainCheckout: main, path: wt });
  await until(() => Boolean(h.runner.get(thread.id).actions), 'the patches to load');
  const land = h.runner.get(thread.id).actions?.find((a) => a.id === 'worktree.land');
  assert.ok(land);
  assert.equal(land.title, 'Land');
  // The path is shell-quoted when it has a space, as the updater's own clone
  // under Application Support does, so allow a closing quote before ` land`.
  assert.match(land.command ?? '', /\/bin\/tb'? land$/);
  assert.ok(h.runner.get(thread.id).patches?.some((p) => p.id === 'worktree' && p.source === 'builtin'));
  const rec = h.store.load(thread.id);
  assert.deepEqual(rec?.worktree, { repo: 'proj', mainCheckout: main, path: wt, branch: `thread/${id8}`, base: sh(main, 'git rev-parse main'), patchFile: '.threadbear/patches/checks.ts' });
  assert.equal(rec?.worktreePending, undefined);
});

test('worktree: false, a folder with no patches, and a repo without main checked out all start where asked', async () => {
  const { main } = makeRepo();
  const h = harness();
  const off = h.runner.create({ text: 'Here', accountId: 'claude-3', cwd: main, worktree: false });
  // Nothing to wait for: the CLI starts at once, as before.
  assert.equal(h.adapter.last.opts.cwd, main);
  assert.equal(h.runner.get(off.thread.id).worktree, undefined);

  const plain = fs.realpathSync(tmpDir());
  h.runner.create({ text: 'Plain', accountId: 'claude-3', cwd: plain });
  assert.equal(h.adapter.last.opts.cwd, plain, 'no patch files: started straight away');

  sh(main, 'git checkout -q -b other');
  const { thread } = h.runner.create({ text: 'No main', accountId: 'claude-3', cwd: main });
  await until(() => h.adapter.sessions.length === 3, 'the CLI to start');
  assert.equal(h.adapter.last.opts.cwd, main);
  assert.equal(h.runner.get(thread.id).worktree, undefined);
  const note = h.events(thread.id).find((e) => e.type === 'system.note');
  assert.equal(note?.text, `Couldn't make a worktree for this thread: no checkout of proj has main checked out. It starts in ${main} instead.`);
  assert.doesNotMatch(h.adapter.last.opts.instructions ?? '', /worktree/);
});

test('a leftover folder or branch of the same name is never reused', async () => {
  const { root, main } = makeRepo();
  const patch = (await findWorktreePatch(main, root))!;
  const id = 'feedface-0000-0000-0000-000000000000';
  fs.mkdirSync(path.join(root, 'proj-threads', 'feedface'), { recursive: true });
  const folder = await makeWorktree({ cwd: main, threadId: id, patch });
  assert.deepEqual(folder, { ok: false, reason: `there's already a folder at ${path.join(root, 'proj-threads', 'feedface')}` });
  fs.rmSync(path.join(root, 'proj-threads'), { recursive: true });
  sh(main, 'git branch thread/feedface');
  assert.deepEqual(await makeWorktree({ cwd: main, threadId: id, patch }), { ok: false, reason: 'the branch thread/feedface already exists' });
  assert.equal(sh(main, 'git worktree list').split('\n').length, 1);

  const notRepo = path.join(fs.realpathSync(tmpDir()), 'plain');
  write(notRepo, { '.threadbear/patches/w.ts': PATCH('{}') });
  const p2 = (await findWorktreePatch(notRepo, path.dirname(notRepo)))!;
  const r = await makeWorktree({ cwd: notRepo, threadId: id, patch: p2 });
  assert.deepEqual(r, { ok: false, reason: `the w patch asks for one, but ${notRepo} isn't in a git repo` });
});

test('a failing setup is shown and the turn starts anyway', async () => {
  const { main } = makeRepo(PATCH(`{ setup: "echo no luck >&2; exit 4" }`));
  const h = harness();
  const { thread } = h.runner.create({ text: 'Go', accountId: 'claude-3', cwd: main });
  await until(() => h.adapter.sessions.length === 1, 'the CLI to start');
  const setup = h.events(thread.id).find((e) => e.type === 'action.run');
  assert.equal(setup.ok, false);
  assert.equal(setup.exitCode, 4);
  assert.equal(setup.text.trim(), 'no luck');
  assert.ok(h.runner.get(thread.id).worktree);
});

// The setup runs in the new worktree, from main's commit, which may not be
// what was trusted in the main checkout (an uncommitted edit there, a
// script changed on main): it's the worktree's own copy of the patch, and
// the scripts it names there, that must be trusted (Daybreak, 5 Nov).
test("setup runs only when the worktree's own copy of the patch is trusted, and it's that copy's setup", async () => {
  const opts = { actionShell: ['/bin/sh', '-c'], actionTimeoutMs: 10_000 };
  const file = (main: string) => path.join(main, '.threadbear/patches/checks.ts');

  // Committed: a setup script that does harm. In the main checkout, not
  // committed: a harmless one, and that's what was trusted.
  const bad = makeRepo(PATCH(`{ setup: "./setup.sh" }`));
  commit(bad.main, { 'setup.sh': '#!/bin/sh\ntouch .evil-ran\n' }, 'setup');
  fs.chmodSync(path.join(bad.main, 'setup.sh'), 0o755);
  fs.writeFileSync(path.join(bad.main, 'setup.sh'), '#!/bin/sh\ntouch .setup-ran\n');
  const trust = new PatchTrust();
  trust.trustCurrent([file(bad.main)]);
  const h = makeHarness(undefined, {}, { ...opts, patchTrust: trust });
  const { thread } = h.runner.create({ text: 'Go', accountId: 'claude-3', cwd: bad.main });
  await until(() => h.adapter.sessions.length === 1, 'the CLI to start');
  const wt = h.runner.get(thread.id).worktree;
  assert.ok(wt, 'the worktree is still made: the main checkout\'s patch is trusted');
  const wtPath = h.store.load(thread.id)!.worktree!.path;
  assert.equal(fs.existsSync(path.join(wtPath, '.evil-ran')), false, "the worktree's setup script didn't run");
  const events = h.events(thread.id);
  assert.equal(events.some((e) => e.type === 'action.run'), false);
  const note = events.filter((e) => e.type === 'system.note').at(-1);
  assert.match(note?.text ?? '', /^Didn't run the worktree's setup \(\.\/setup\.sh\): its copy of the checks patch isn't a version you've trusted\./);

  // Both versions trusted: the worktree's own setup is the one that runs,
  // by the absolute path of the worktree's script.
  const good = makeRepo(PATCH(`{ setup: "./committed.sh" }`));
  commit(good.main, { 'committed.sh': '#!/bin/sh\ntouch .committed-ran\n', 'main.sh': '#!/bin/sh\ntouch .main-ran\n' }, 'scripts');
  fs.chmodSync(path.join(good.main, 'committed.sh'), 0o755);
  fs.chmodSync(path.join(good.main, 'main.sh'), 0o755);
  sh(good.main, 'git add -A && git commit -qm modes');
  const t2 = new PatchTrust();
  t2.trustCurrent([file(good.main)]);
  fs.writeFileSync(file(good.main), PATCH(`{ setup: "./main.sh" }`));
  fs.utimesSync(file(good.main), new Date(Date.now() + 5000), new Date(Date.now() + 5000));
  t2.trustCurrent([file(good.main)]);
  const h2 = makeHarness(undefined, {}, { ...opts, patchTrust: t2 });
  const made = h2.runner.create({ text: 'Go', accountId: 'claude-3', cwd: good.main });
  await until(() => h2.adapter.sessions.length === 1, 'the CLI to start');
  const p2 = h2.store.load(made.thread.id)!.worktree!.path;
  assert.ok(fs.existsSync(path.join(p2, '.committed-ran')));
  assert.equal(fs.existsSync(path.join(p2, '.main-ran')), false);
  assert.equal(h2.events(made.thread.id).find((e) => e.type === 'action.run')?.command, './committed.sh');
});

// The remediation test for Daybreak's finding: changing only the setup
// script needs a fresh trust decision before a new thread runs it, however
// the setup names it. `cd scripts && ./setup.sh` kept its trust while
// scripts/setup.sh changed, as the hash only saw ./setup.sh.
test('changing only the setup script needs trusting again before a thread runs it; a setup that cds first never runs', async () => {
  const opts = { actionShell: ['/bin/sh', '-c'], actionTimeoutMs: 10_000 };
  const file = (main: string) => path.join(main, '.threadbear/patches/checks.ts');

  const cd = makeRepo(PATCH(`{ setup: "cd scripts && ./setup.sh" }`));
  commit(cd.main, { 'scripts/setup.sh': 'touch ../.setup-ran\n' }, 'setup');
  sh(cd.main, 'chmod +x scripts/setup.sh && git add -A && git commit -qm mode');
  const trust = new PatchTrust();
  trust.trustCurrent([file(cd.main)]);
  // A commit changes only the script the trusted command reaches.
  commit(cd.main, { 'scripts/setup.sh': 'touch ../.evil-ran\n' }, 'evil');
  const h = makeHarness(undefined, {}, { ...opts, patchTrust: trust });
  const { thread } = h.runner.create({ text: 'Go', accountId: 'claude-3', cwd: cd.main });
  await until(() => h.adapter.sessions.length === 1, 'the CLI to start');
  assert.equal(h.runner.get(thread.id).worktree, undefined, 'the patch is refused, so no worktree either');
  assert.equal(h.adapter.last.opts.cwd, cd.main);
  assert.equal(h.events(thread.id).some((e) => e.type === 'action.run'), false);
  assert.equal(fs.existsSync(path.join(cd.main, '.evil-ran')), false);
  await h.runner.patchesIdle();
  // The command is followed for the hash too now, so the changed script is
  // asked about; as it stands it'd be refused anyway (patch-trust tests).
  const info = h.runner.get(thread.id).patches?.find((p) => p.id === 'checks');
  assert.equal(info?.untrusted, 'changed');

  // Named as one script: trusted, then a commit changes only the script.
  const one = makeRepo(PATCH(`{ setup: "./scripts/setup.sh" }`));
  commit(one.main, { 'scripts/setup.sh': '#!/bin/sh\ntouch .setup-ran\n' }, 'setup');
  sh(one.main, 'chmod +x scripts/setup.sh && git add -A && git commit -qm mode');
  const t2 = new PatchTrust();
  t2.trustCurrent([file(one.main)]);
  commit(one.main, { 'scripts/setup.sh': '#!/bin/sh\ntouch .evil-ran\n' }, 'evil');
  // The main checkout's patch is trusted no longer, so no worktree, and nothing runs.
  const h2 = makeHarness(undefined, {}, { ...opts, patchTrust: t2 });
  const made = h2.runner.create({ text: 'Go', accountId: 'claude-3', cwd: one.main });
  await until(() => h2.adapter.sessions.length === 1, 'the CLI to start');
  assert.equal(h2.runner.get(made.thread.id).worktree, undefined);
  assert.equal(h2.events(made.thread.id).some((e) => e.type === 'action.run'), false);
  await h2.runner.patchesIdle();
  assert.equal(h2.runner.get(made.thread.id).patches?.find((p) => p.id === 'checks')?.untrusted, 'changed');

  // Trusting the new version is what lets it run.
  t2.trustCurrent([file(one.main)]);
  const h3 = makeHarness(undefined, {}, { ...opts, patchTrust: t2 });
  const third = h3.runner.create({ text: 'Go', accountId: 'claude-3', cwd: one.main });
  await until(() => h3.adapter.sessions.length === 1, 'the CLI to start');
  const p3 = h3.store.load(third.thread.id)!.worktree!.path;
  assert.ok(fs.existsSync(path.join(p3, '.evil-ran')), 'the version trusted now runs');
});

// Daybreak verify-fix (5 Nov): the worktree's copy of a trusted patch
// hashed the same when only a script its setup runs had changed, so the
// changed helper ran before the first turn. Now the helper is in the hash.
test("a worktree whose setup script is unchanged but whose helper changed doesn't run the setup; trusting that version does", async () => {
  const opts = { actionShell: ['/bin/sh', '-c'], actionTimeoutMs: 10_000 };
  const file = (main: string) => path.join(main, '.threadbear/patches/checks.ts');
  const r = makeRepo(PATCH(`{ setup: "./setup.sh" }`));
  // Committed: an unchanged setup.sh that runs a harmful helper. In the main
  // checkout, not committed: a harmless helper, and that's what was trusted.
  commit(r.main, { 'setup.sh': '#!/bin/sh\n"$(dirname "$0")/scripts/helper.sh"\n', 'scripts/helper.sh': '#!/bin/sh\ntouch .evil-ran\n' }, 'setup');
  sh(r.main, 'chmod +x setup.sh scripts/helper.sh && git add -A && git commit -qm modes');
  fs.writeFileSync(path.join(r.main, 'scripts/helper.sh'), '#!/bin/sh\ntouch .setup-ran\n');
  const trust = new PatchTrust();
  trust.trustCurrent([file(r.main)]);
  const h = makeHarness(undefined, {}, { ...opts, patchTrust: trust });
  const { thread } = h.runner.create({ text: 'Go', accountId: 'claude-3', cwd: r.main });
  await until(() => h.adapter.sessions.length === 1, 'the CLI to start');
  const wtPath = h.store.load(thread.id)!.worktree!.path;
  assert.equal(fs.existsSync(path.join(wtPath, '.evil-ran')), false, "the worktree's changed helper didn't run");
  assert.equal(h.events(thread.id).some((e) => e.type === 'action.run'), false);
  const note = h.events(thread.id).filter((e) => e.type === 'system.note').at(-1);
  assert.match(note?.text ?? '', /^Didn't run the worktree's setup \(\.\/setup\.sh\): its copy of the checks patch isn't a version you've trusted\./);

  // Committing the helper that was trusted: the worktree's copy matches,
  // and the setup runs it.
  sh(r.main, 'git add -A && git commit -qm helper');
  const h2 = makeHarness(undefined, {}, { ...opts, patchTrust: trust });
  const made = h2.runner.create({ text: 'Go', accountId: 'claude-3', cwd: r.main });
  await until(() => h2.adapter.sessions.length === 1, 'the CLI to start');
  await until(() => h2.events(made.thread.id).some((e) => e.type === 'action.run'), 'the setup to run');
  assert.ok(fs.existsSync(path.join(h2.store.load(made.thread.id)!.worktree!.path, '.setup-ran')));
});

test('threads.create takes worktree: false over RPC, and refuses a worktree that isn\'t a boolean', async () => {
  const { main } = makeRepo();
  const h = harness();
  const server = new RpcServer({ host: '127.0.0.1', port: 0, token: readToken(h.home), runner: h.runner, log: nullLogger });
  const { port } = await server.listen();
  const c = await RunnerClient.connect(`ws://127.0.0.1:${port}/rpc`, readToken(h.home));
  try {
    await c.call('hello', { client: 'test', version: '1' });
    await assert.rejects(c.call('threads.create', { text: 'x', accountId: 'claude-3', cwd: main, worktree: 'no' }), /worktree must be a boolean/);
    const r = await c.call<{ thread: { id: string; cwd: string } }>('threads.create', { text: 'x', accountId: 'claude-3', cwd: main, worktree: false });
    assert.equal(r.thread.cwd, main);
    assert.equal(h.adapter.last.opts.cwd, main);
    assert.equal(h.store.load(r.thread.id)?.worktreePending, undefined);
  } finally {
    c.close();
    await server.close();
  }
});
