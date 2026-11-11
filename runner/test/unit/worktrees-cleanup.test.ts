// worktrees: cleaning up and teardowns (helpers/worktrees-harness.ts says why it's several files).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { makeHarness, tmpDir } from '../helpers/fakes.ts';
import { checkSequence, checkSummary } from '../helpers/protocol-check.ts';
import { automaticTrust, findWorktreePatch } from '../../src/patches.ts';
import { cleanUpWorktree, git, gitLimits, land } from '../../src/worktrees.ts';
import { PatchTrust } from '../../src/patch-trust.ts';
import { HUNG_GIT_LIMIT_MS, PATCH, TEARDOWN, capture, commit, harness, lines, makeBranch, makeRepo, notes, sh, swapDuringGit, teardowns, threadWithWorktree, trustedTeardownRepo, until, wrapGit, write } from '../helpers/worktrees-harness.ts';

test('archiving a thread whose branch is all on main removes its worktree and branch; it comes back in the main checkout', async () => {
  const { root, main } = makeRepo();
  const h = harness();
  const { id, w } = await threadWithWorktree(h, main);
  commit(w.path, { 'runner/b.ts': '2\n' }, 'Landed work');
  assert.equal(await land({ path: w.path, branch: w.branch, mainCheckout: main }, capture().opts), 0);
  write(w.path, { 'scratch.txt': 'untracked, so --force\n' });
  h.runner.update({ threadId: id, archived: true });
  await until(() => notes(h, id).some((n) => n.startsWith('Removed')), 'the cleanup');
  assert.equal(notes(h, id).at(-1), `Removed this thread's worktree and its branch ${w.branch}: everything on it is on main.`);
  assert.ok(!fs.existsSync(w.path));
  assert.ok(!fs.existsSync(path.join(root, 'proj-threads')), 'the empty -threads folder goes too');
  assert.equal(sh(main, `git branch --list ${w.branch}`), '');
  const summary = h.runner.get(id);
  assert.equal(summary.worktree, undefined);
  assert.ok(!summary.actions?.some((a) => a.id === 'worktree.land'));
  assert.deepEqual(checkSummary(summary), []);

  // Sent a message: unarchived, and on in the main checkout's runner/.
  h.runner.send(id, 'More please');
  await until(() => h.adapter.sessions.length === 2, 'a new CLI');
  assert.equal(h.adapter.last.opts.cwd, path.join(main, 'runner'));
  assert.equal(h.runner.get(id).cwd, path.join(main, 'runner'));
  assert.ok(notes(h, id).includes(`This thread's worktree was removed when it was archived, so it carries on in the main checkout, ${path.join(main, 'runner')}.`));
  const sent = h.adapter.last.sent.at(-1)!.text;
  // After the handoff note (the test harness can't carry the transcript).
  assert.match(sent, new RegExp(`\n\n\\[Threadbear\\] Your worktree at ${w.path} was removed after its work landed on main\\. You're now in the main checkout`));
  assert.match(sent, /More please$/);
  assert.doesNotMatch(h.adapter.last.opts.instructions ?? '', /own git worktree/, 'a fresh session is no longer told about it');
});

test("archiving a thread with work that isn't on main keeps the worktree and branch, and says how to land or drop them", async () => {
  const { main } = makeRepo();
  const h = harness();
  const { id, w } = await threadWithWorktree(h, main);
  commit(w.path, { 'runner/b.ts': '2\n' }, 'Not landed');
  h.runner.update({ threadId: id, archived: true });
  await until(() => notes(h, id).some((n) => n.includes(' stay: ')), 'the cleanup');
  assert.equal(
    notes(h, id).at(-1),
    `This thread's worktree at ${w.path} and its branch ${w.branch} stay: ${w.branch} has commits that aren't on main. ` +
      `To land it, run tb land in ${w.path} (or bring the thread back and press Land). ` +
      `To drop it, git -C ${main} worktree remove --force ${w.path} && git -C ${main} branch -D ${w.branch}.`,
  );
  assert.ok(fs.existsSync(w.path));
  assert.ok(h.runner.get(id).worktree, 'still shown');
});

test('cleanUpWorktree keeps a worktree with uncommitted changes to tracked files', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  write(t.path, { 'README.md': 'edited\n' });
  const rec = { repo: 'proj', mainCheckout: main, path: t.path, branch: t.branch, base: sh(main, 'git rev-parse main') };
  assert.deepEqual(await cleanUpWorktree(rec), { removed: false, reason: 'the worktree has uncommitted changes' });
  assert.ok(fs.existsSync(t.path));
  sh(t.path, 'git checkout -- README.md');
  assert.deepEqual(await cleanUpWorktree(rec), { removed: true });
  assert.ok(!fs.existsSync(t.path));
  assert.equal(sh(main, `git branch --list ${t.branch}`), '');
});

test('deleting a thread removes a landed worktree; an unlanded one stays, with a notice', async () => {
  const { main } = makeRepo();
  const h = harness();
  const a = await threadWithWorktree(h, main);
  h.runner.delete(a.id);
  // The branch goes after the folder.
  await until(() => !fs.existsSync(a.w.path) && sh(main, `git branch --list ${a.w.branch}`) === '', 'the worktree and branch to go');

  const b = await threadWithWorktree(h, main);
  commit(b.w.path, { 'runner/b.ts': '2\n' }, 'Not landed');
  h.runner.delete(b.id);
  await until(() => h.notes.some((n) => n.method === 'runner.notice'), 'the notice');
  const notice = h.notes.find((n) => n.method === 'runner.notice') as { params: { message: string } };
  assert.match(notice.params.message, new RegExp(`^"Go" was deleted, but its worktree at ${b.w.path} and branch ${b.w.branch} stay: ${b.w.branch.replace('/', '\\/')} has commits that aren't on main\\. To land it, run tb land in ${b.w.path}\\. To drop it`));
  assert.ok(fs.existsSync(b.w.path));
});

test('archiving runs the teardown in the worktree root before the cleanup, landed or not', async () => {
  const log = path.join(fs.realpathSync(tmpDir('tb-td-')), 'log');
  const { main } = makeRepo(TEARDOWN(log));
  const h = harness();
  const kept = await threadWithWorktree(h, main);
  commit(kept.w.path, { 'runner/b.ts': '2\n' }, 'Not landed');
  h.runner.update({ threadId: kept.id, archived: true });
  await until(() => notes(h, kept.id).some((n) => n.includes(' stay: ')), 'the cleanup');
  const [run] = teardowns(h, kept.id);
  assert.equal(run?.title, 'Tear down the worktree');
  assert.equal(run?.patch, 'checks');
  assert.equal(run?.actionId, 'checks.worktree-teardown');
  assert.equal(run?.ok, true);
  assert.equal(run?.turnId, undefined);
  assert.match(run?.command ?? '', /^echo \$PWD/);
  assert.ok(fs.existsSync(kept.w.path), 'the unlanded worktree stays');
  assert.deepEqual(checkSequence(h.events(kept.id)), []);

  const landed = await threadWithWorktree(h, main);
  h.runner.update({ threadId: landed.id, archived: true });
  await until(() => notes(h, landed.id).some((n) => n.startsWith('Removed')), 'the cleanup');
  const events = h.events(landed.id);
  const ran = events.findIndex((e) => e.type === 'action.run' && e.trigger === 'worktree.teardown');
  const removed = events.findIndex((e) => e.type === 'system.note' && String(e.text).startsWith('Removed'));
  assert.ok(ran >= 0 && ran < removed, 'the teardown comes before the cleanup');
  assert.ok(!fs.existsSync(landed.w.path));
  assert.deepEqual(lines(log), [`${kept.w.path} there`, `${landed.w.path} there`], 'each ran in its worktree root while its files were there');
});

test('deleting a thread runs its teardown before the worktree goes; a failing one says so in a notice', async () => {
  const dir = fs.realpathSync(tmpDir('tb-td-'));
  const { main } = makeRepo(TEARDOWN(path.join(dir, 'log')));
  const h = harness();
  const a = await threadWithWorktree(h, main);
  h.runner.delete(a.id);
  await until(() => !fs.existsSync(a.w.path), 'the worktree to go');
  assert.deepEqual(lines(path.join(dir, 'log')), [`${a.w.path} there`]);
  assert.equal(h.notes.some((n) => n.method === 'runner.notice'), false, 'nothing to say when it worked');

  const failing = makeRepo(TEARDOWN(path.join(dir, 'log2'), '; echo it broke; exit 3'));
  const h2 = harness();
  const b = await threadWithWorktree(h2, failing.main);
  h2.runner.delete(b.id);
  await until(() => h2.notes.some((n) => n.method === 'runner.notice'), 'the notice');
  const notice = h2.notes.find((n) => n.method === 'runner.notice') as { params: { message: string } };
  assert.match(notice.params.message, /^"Go" was deleted, but its worktree's teardown \(echo \$PWD .*\) failed \(exit code 3\): it broke$/);
  await until(() => !fs.existsSync(b.w.path), 'the worktree to go anyway');
  assert.deepEqual(lines(path.join(dir, 'log2')), [`${b.w.path} there`]);
});

test('a failing teardown is shown and the cleanup still happens', async () => {
  const dir = fs.realpathSync(tmpDir('tb-td-'));
  const { main } = makeRepo(TEARDOWN(path.join(dir, 'log'), '; echo no deployment to remove; exit 3'));
  const h = harness();
  const { id, w } = await threadWithWorktree(h, main);
  h.runner.update({ threadId: id, archived: true });
  await until(() => notes(h, id).some((n) => n.startsWith('Removed')), 'the cleanup');
  const [run] = teardowns(h, id);
  assert.equal(run?.ok, false);
  assert.equal(run?.exitCode, 3);
  assert.match(run?.text ?? '', /no deployment to remove/);
  assert.ok(!fs.existsSync(w.path));
  assert.equal(sh(main, `git branch --list ${w.branch}`), '');
});

test("an untrusted teardown doesn't run, and says so; a trusted one runs its hashed script", async () => {
  const dir = fs.realpathSync(tmpDir('tb-td-'));
  const log = path.join(dir, 'log');
  const { main } = makeRepo(PATCH(`{ teardown: "./teardown.sh" }`));
  commit(main, { 'teardown.sh': `#!/bin/sh\necho "torn down $PWD" >> ${log}\n` }, 'teardown');
  fs.chmodSync(path.join(main, 'teardown.sh'), 0o755);
  sh(main, 'git add -A && git commit -qm modes');
  const trust = new PatchTrust();
  trust.trustCurrent([path.join(main, '.threadbear/patches/checks.ts')]);
  const h = makeHarness(undefined, {}, { actionShell: ['/bin/sh', '-c'], actionTimeoutMs: 10_000, patchTrust: trust });

  const a = await threadWithWorktree(h, main);
  h.runner.update({ threadId: a.id, archived: true });
  await until(() => notes(h, a.id).some((n) => n.startsWith('Removed')), 'the cleanup');
  assert.equal(teardowns(h, a.id)[0]?.command, './teardown.sh');
  assert.deepEqual(lines(log), [`torn down ${a.w.path}`], 'from the worktree, which is the patch\'s folder');

  // A commit on the branch changes what the teardown runs: that's a patch
  // version nobody trusted, so it never runs by itself.
  const b = await threadWithWorktree(h, main);
  commit(b.w.path, { 'teardown.sh': `#!/bin/sh\necho evil >> ${log}\n` }, 'Change the teardown');
  h.runner.update({ threadId: b.id, archived: true });
  await until(() => notes(h, b.id).some((n) => n.includes(' stay: ')), 'the cleanup');
  assert.equal(teardowns(h, b.id).length, 0);
  assert.ok(
    notes(h, b.id).includes("Didn't run the worktree's teardown: its copy of the checks patch isn't a version you've trusted. Trust it in Patches, then run the teardown yourself if you want it."),
    notes(h, b.id).join('\n'),
  );
  assert.deepEqual(lines(log), [`torn down ${a.w.path}`]);
});

test('the teardown runs once per archive: a delete after it skips it, and a thread brought back is told and tears down again', async () => {
  const dir = fs.realpathSync(tmpDir('tb-td-'));
  const log = path.join(dir, 'log');
  const { main } = makeRepo(TEARDOWN(log));
  const h = harness();
  const { id, w } = await threadWithWorktree(h, main);
  commit(w.path, { 'runner/b.ts': '2\n' }, 'Not landed');
  h.runner.update({ threadId: id, archived: true });
  await until(() => notes(h, id).some((n) => n.includes(' stay: ')), 'the cleanup');
  assert.equal(lines(log).length, 1);
  assert.ok(h.store.load(id)?.worktree?.tornDownAt, 'remembered, so a runner picking the cleanup up again skips it');

  // Brought back: on in its worktree, told what the teardown may have taken.
  h.runner.send(id, 'More please');
  await until(() => h.adapter.sessions.length === 2, 'a new CLI');
  assert.equal(h.adapter.last.opts.cwd, path.join(w.path, 'runner'));
  assert.ok(notes(h, id).includes("This thread's worktree was torn down when it was archived (its patch's teardown ran), so anything its setup made outside the folder may be gone. It carries on in the worktree."));
  assert.match(h.adapter.last.sent.at(-1)!.text, /\[Threadbear\] Your worktree's teardown ran when this thread was archived/);
  assert.equal(h.store.load(id)?.worktree?.tornDownAt, undefined);
  h.adapter.last.init();
  h.adapter.last.result();

  h.runner.update({ threadId: id, archived: true });
  await until(() => notes(h, id).filter((n) => n.includes(' stay: ')).length === 2, 'the second cleanup');
  assert.equal(lines(log).length, 2, 'archived again: torn down again');
  h.runner.delete(id);
  await until(() => h.notes.some((n) => n.method === 'runner.notice'), "the notice that the worktree stays");
  assert.equal(lines(log).length, 2, 'already torn down for this archive');
});

// Codex's review of the teardown, 9 Nov: a patch file that's a link into
// another folder was hashed with its scripts followed from the folder it's
// linked to, but its commands run from the folder holding the link. So a
// script's own `./helper.sh` ran from beside the link, unhashed. (Beside
// the repo's top it happened to be hashed anyway, which hid this from the
// first test of it.) Setup, hooks and actions are bound the same way.
test('a symlinked patch has its scripts followed from where they run, so a helper beside the link is hashed', async () => {
  const dir = fs.realpathSync(tmpDir('tb-td-'));
  const log = path.join(dir, 'log');
  const { main } = makeRepo('');
  write(main, {
    'nested/.threadbear/patches/p.ts': PATCH(`{ setup: "./cleanup.sh", teardown: "./cleanup.sh" }`),
    'nested/cleanup.sh': '#!/bin/sh\n./helper.sh\n',
    'nested/helper.sh': `#!/bin/sh\necho nested >> ${log}\n`,
    'sub/helper.sh': `#!/bin/sh\necho beside >> ${log}\n`,
  });
  for (const f of ['nested/cleanup.sh', 'nested/helper.sh', 'sub/helper.sh']) fs.chmodSync(path.join(main, f), 0o755);
  fs.mkdirSync(path.join(main, 'sub/.threadbear/patches'), { recursive: true });
  fs.symlinkSync(path.join(main, 'nested/.threadbear/patches/p.ts'), path.join(main, 'sub/.threadbear/patches/p.ts'));
  fs.symlinkSync(path.join(main, 'nested/cleanup.sh'), path.join(main, 'sub/cleanup.sh'));
  const patch = path.join(main, 'sub/.threadbear/patches/p.ts');
  const trust = new PatchTrust();
  trust.trustCurrent([patch]);
  const found = await findWorktreePatch(path.join(main, 'sub'), dir, new Map(), undefined, trust);
  assert.ok(found?.teardownRun && found.setupRun);
  // What runs is the helper beside the link, as the command runs there...
  sh(main, found.teardownRun);
  assert.deepEqual(lines(log), ['beside']);
  // ...so changing it is a version nobody trusted, and the check just
  // before a bound command runs says so too.
  fs.writeFileSync(path.join(main, 'sub/helper.sh'), `#!/bin/sh\necho evil >> ${log}\n`);
  assert.equal(await findWorktreePatch(path.join(main, 'sub'), dir, new Map(), undefined, trust), undefined);
  for (const run of [found.teardownRun, found.setupRun]) {
    const by = automaticTrust(run);
    assert.ok(by);
    assert.equal(trust.verify(by.file, by.hash), false);
  }
});

test("a worktree folder swapped for a link to the main checkout gets no teardown, and the main checkout is left alone", async () => {
  const dir = fs.realpathSync(tmpDir('tb-td-'));
  const log = path.join(dir, 'log');
  const { main, h } = trustedTeardownRepo(log);
  const { id, w } = await threadWithWorktree(h, main);
  fs.renameSync(w.path, `${w.path}-moved`);
  fs.symlinkSync(main, w.path);
  h.runner.update({ threadId: id, archived: true });
  const rt = (h.runner as any).threads.get(id).rt;
  await until(() => rt.archivePending === false && !rt.cleaning, 'the cleanup');
  assert.deepEqual(lines(log), [], 'no teardown, in the main checkout or anywhere');
  assert.equal(teardowns(h, id).length, 0);
  assert.equal(sh(main, 'git rev-parse --abbrev-ref HEAD'), 'main', 'the main checkout is still on main');
  assert.equal(sh(main, 'git status --porcelain'), '');
  assert.ok(fs.existsSync(path.join(main, 'README.md')));
  assert.ok(fs.existsSync(path.join(`${w.path}-moved`, 'runner/a.ts')), 'the moved worktree is left alone too');
});

// Codex's second look, 9 Nov: the folder was checked before an await, so a
// swap while git listed the worktrees passed, and the teardown ran in main.
test('a worktree folder swapped for a link while the cleanup waits on git gets no teardown', async () => {
  const dir = fs.realpathSync(tmpDir('tb-td-'));
  const log = path.join(dir, 'log');
  const { main, h } = trustedTeardownRepo(log);
  const { id, w } = await threadWithWorktree(h, main);
  const restore = swapDuringGit('list', w.path, main);
  try {
    h.runner.update({ threadId: id, archived: true });
    const rt = (h.runner as any).threads.get(id).rt;
    await until(() => rt.archivePending === false && !rt.cleaning, 'the cleanup');
  } finally {
    restore();
  }
  assert.ok(fs.lstatSync(w.path).isSymbolicLink(), 'the swap happened');
  assert.deepEqual(lines(log), []);
  assert.equal(teardowns(h, id).length, 0);
  assert.equal(sh(main, 'git rev-parse --abbrev-ref HEAD'), 'main');
  assert.equal(sh(main, 'git status --porcelain'), '');
});

test("a worktree folder swapped for a link while the cleanup reads its status never has main's checkout detached or removed", async () => {
  const { main } = makeRepo(PATCH(`{ keep: ["app/.build"] }`));
  const h = harness();
  const { id, w } = await threadWithWorktree(h, main);
  const restore = swapDuringGit('status', w.path, main);
  try {
    h.runner.update({ threadId: id, archived: true });
    const rt = (h.runner as any).threads.get(id).rt;
    await until(() => rt.archivePending === false && !rt.cleaning, 'the cleanup');
  } finally {
    restore();
  }
  assert.ok(fs.lstatSync(w.path).isSymbolicLink(), 'the swap happened');
  assert.equal(sh(main, 'git rev-parse --abbrev-ref HEAD'), 'main', 'main is still checked out in the main checkout');
  assert.ok(fs.existsSync(path.join(main, 'README.md')));
  assert.equal(sh(`${w.path}-moved`, 'git rev-parse --abbrev-ref HEAD'), w.branch, 'the real worktree is left as it was');
});

// Codex's third look, 9 Nov: the removal accepted the folder on any branch,
// so a switch while status ran had --force take edits made on the new one.
test('a worktree switched to another branch while the cleanup reads its status is left alone, edits and all', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  write(t.path, { 'scratch.txt': 'untracked, so it would be --force\n' });
  const restore = wrapGit('status', `out=$("$REAL" "$@"); code=$?; "$REAL" -C '${t.path}' checkout -q -b other; echo edit > '${t.path}/after.txt'; printf '%s\\n' "$out"; exit $code`);
  let result;
  try {
    const rec = { repo: 'proj', mainCheckout: main, path: t.path, branch: t.branch, base: sh(main, 'git rev-parse main') };
    result = await cleanUpWorktree(rec);
  } finally {
    restore();
  }
  assert.deepEqual(result, { removed: false, reason: `its folder ${t.path} changed while it was being cleaned up, so it was left alone` });
  assert.equal(fs.readFileSync(path.join(t.path, 'after.txt'), 'utf8'), 'edit\n');
  assert.equal(sh(t.path, 'git rev-parse --abbrev-ref HEAD'), 'other');
});

test("a git that times out checking the branch is there stops the cleanup, rather than reading as no branch", async () => {
  const { main } = makeRepo(PATCH(`{ keep: ["app/.build"] }`));
  const t = makeBranch(main);
  commit(t.path, { 'runner/b.ts': '2\n' }, 'Not landed');
  const restore = wrapGit('--verify', 'exec sleep 30', HUNG_GIT_LIMIT_MS);
  let result;
  try {
    const rec = { repo: 'proj', mainCheckout: main, path: t.path, branch: t.branch, base: sh(main, 'git rev-parse main'), keep: ['app/.build'] };
    result = await cleanUpWorktree(rec);
  } finally {
    restore();
  }
  assert.deepEqual(result, { removed: false, reason: `git couldn't tell whether ${t.branch} is still there` });
  assert.equal(sh(t.path, 'git rev-parse --abbrev-ref HEAD'), t.branch, 'not detached or pooled');
  assert.ok(!sh(main, 'git worktree list --porcelain').includes('locked'));
});

test("tb land counts a git that times out reading what's to land as a failure, not as nothing to land", async () => {
  for (const [match, said] of [['log', /^Couldn't read what thread\/abcd1234 has to land/], ['rev-list', /^Couldn't read thread\/abcd1234's history/]] as const) {
    const { main } = makeRepo();
    const t = makeBranch(main);
    commit(t.path, { 'runner/b.ts': '2\n' }, 'Work');
    const before = sh(main, 'git rev-parse main');
    const c = capture();
    const restore = wrapGit(match, 'exec sleep 30', HUNG_GIT_LIMIT_MS);
    let code;
    try {
      code = await land(t, c.opts);
    } finally {
      restore();
    }
    assert.equal(code, 1, match);
    assert.ok(!c.out.some((l) => l.startsWith('Nothing to land')), match);
    assert.ok(c.err.some((l) => said.test(l)), `${match}: ${c.err.join('\n')}`);
    assert.equal(sh(main, 'git rev-parse main'), before, `${match}: main didn't move`);
    assert.equal(sh(t.path, 'git rev-list --count main..HEAD'), '1', `${match}: the branch is as it was`);
  }
});

test('git gives up after its time limit, and that counts as a failure', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  const realGit = sh('/', 'command -v git');
  const bin = tmpDir('tb-hang-git-');
  fs.writeFileSync(path.join(bin, 'git'), `#!/bin/sh\ncase " $* " in *" status "*) exec sleep 30 ;; esac\nexec '${realGit}' "$@"\n`, { mode: 0o755 });
  const before = process.env.PATH;
  const limit = gitLimits.timeoutMs;
  process.env.PATH = `${bin}${path.delimiter}${before}`;
  gitLimits.timeoutMs = 300;
  try {
    const started = Date.now();
    const r = await git(t.path, ['status', '--porcelain']);
    assert.equal(r.ok, false);
    assert.ok(Date.now() - started < 10_000);
    const rec = { repo: 'proj', mainCheckout: main, path: t.path, branch: t.branch, base: sh(main, 'git rev-parse main') };
    assert.deepEqual(await cleanUpWorktree(rec), { removed: false, reason: `git couldn't read ${t.path}` });
    assert.ok(fs.existsSync(t.path));
  } finally {
    process.env.PATH = before;
    gitLimits.timeoutMs = limit;
  }
});

test("a worktree's patch swapped for a fifo doesn't hold the runner up: the teardown is skipped, saying so, and the cleanup goes on", async () => {
  const dir = fs.realpathSync(tmpDir('tb-td-'));
  const log = path.join(dir, 'log');
  const { main, h } = trustedTeardownRepo(log);
  const { id, w } = await threadWithWorktree(h, main);
  const copy = path.join(w.path, '.threadbear/patches/checks.ts');
  fs.rmSync(copy);
  execFileSync('mkfifo', [copy]);
  h.runner.update({ threadId: id, archived: true });
  await until(() => notes(h, id).some((n) => n.includes(' stay: ')), 'the cleanup');
  assert.ok(notes(h, id).some((n) => n.startsWith("Didn't run the worktree's teardown: ")), notes(h, id).join('\n'));
  assert.deepEqual(lines(log), []);
});

// Daybreak, 10 Nov (finding 79): the patch committed as a link to /dev/zero
// is a source with no end; reading it whole to look for "teardown" would
// fill memory and hold the runner. It's skipped as not a plain file, at once.
test("a worktree's patch swapped for a link to /dev/zero is skipped at once, without reading it", async () => {
  const dir = fs.realpathSync(tmpDir('tb-td-'));
  const log = path.join(dir, 'log');
  const { main, h } = trustedTeardownRepo(log);
  const { id, w } = await threadWithWorktree(h, main);
  const copy = path.join(w.path, '.threadbear/patches/checks.ts');
  fs.rmSync(copy);
  fs.symlinkSync('/dev/zero', copy);
  const started = process.cpuUsage();
  h.runner.update({ threadId: id, archived: true });
  await until(() => notes(h, id).some((n) => n.includes(' stay: ')), 'the cleanup');
  const cpu = process.cpuUsage(started);
  assert.ok(notes(h, id).some((n) => n.startsWith("Didn't run the worktree's teardown: ")), notes(h, id).join('\n'));
  assert.deepEqual(lines(log), []);
  assert.ok(cpu.user + cpu.system < 2_000_000, `cpu ${cpu.user + cpu.system}us`);
});

test('a setup or teardown that ignores SIGTERM still ends at the timeout, so neither the first turn nor the cleanup waits for ever', async () => {
  const STUCK = `trap '' TERM; while :; do sleep 0.2; done`;
  const td = makeRepo(PATCH(`{ teardown: "${STUCK}" }`));
  const h = makeHarness(undefined, {}, { actionShell: ['/bin/sh', '-c'], actionTimeoutMs: 300 });
  const { id, w } = await threadWithWorktree(h, td.main);
  h.runner.update({ threadId: id, archived: true });
  await until(() => notes(h, id).some((n) => n.startsWith('Removed')), 'the cleanup, past the teardown');
  assert.deepEqual(teardowns(h, id).map((e) => e.ok), [false]);
  assert.match(teardowns(h, id)[0].text, /Stopped after/);
  assert.ok(!fs.existsSync(w.path));

  const su = makeRepo(PATCH(`{ setup: "${STUCK}" }`));
  const h2 = makeHarness(undefined, {}, { actionShell: ['/bin/sh', '-c'], actionTimeoutMs: 300 });
  const { thread } = h2.runner.create({ text: 'Go', accountId: 'claude-3', cwd: su.main });
  await until(() => h2.adapter.sessions.length === 1, 'the first turn, past the setup');
  const setup = h2.events(thread.id).find((e) => e.type === 'action.run' && e.trigger === 'worktree.setup');
  assert.equal(setup?.ok, false);
  assert.match(setup?.text ?? '', /Stopped after/);
});

test('with keep, the teardown still runs before the worktree is kept as a spare', async () => {
  const dir = fs.realpathSync(tmpDir('tb-td-'));
  const log = path.join(dir, 'log');
  const { main } = makeRepo(PATCH(`{ teardown: "echo $PWD $(test -f runner/a.ts && echo there) >> ${log}", keep: ["app/.build"] }`));
  const h = harness();
  const { id, w } = await threadWithWorktree(h, main);
  h.runner.update({ threadId: id, archived: true });
  await until(() => notes(h, id).some((n) => n.includes('for the next thread')), 'the cleanup');
  assert.equal(teardowns(h, id).length, 1);
  assert.deepEqual(lines(log), [`${w.path} there`]);
  assert.match(sh(main, 'git worktree list --porcelain'), /locked threadbear spare/);
});

test('a handover waits for a running teardown as part of the cleanup', async () => {
  const dir = fs.realpathSync(tmpDir('tb-td-'));
  const log = path.join(dir, 'log');
  const { main } = makeRepo(PATCH(`{ teardown: "sleep 0.4; echo $PWD >> ${log}" }`));
  const h = harness();
  const { id, w } = await threadWithWorktree(h, main);
  h.runner.update({ threadId: id, archived: true });
  const rt = (h.runner as any).threads.get(id).rt;
  await until(() => rt.archivePending === false, 'the cleanup to start');
  assert.ok(rt.cleaning, 'the teardown counts as the cleanup');
  const handed: string[] = [];
  await h.runner.beginDrain([], { onHandedOver: (t) => handed.push(t) });
  await until(() => handed.includes(id), 'the handover');
  assert.deepEqual(lines(log), [w.path]);
  const saved = JSON.parse(fs.readFileSync(path.join(h.home, 'threads', id, 'thread.json'), 'utf8'));
  assert.ok(saved.worktree.removedAt, 'saved once the cleanup after it finished');
  assert.equal(teardowns(h, id).length, 1);
});
