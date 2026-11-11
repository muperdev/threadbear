// worktrees: tb land (helpers/worktrees-harness.ts says why it's several files).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { makeHarness, tmpDir } from '../helpers/fakes.ts';
import { findWorktreePatch } from '../../src/patches.ts';
import { cleanUpWorktree, git, land, landTargetFromGit, makeWorktree, overwrittenFiles } from '../../src/worktrees.ts';
import { RpcServer } from '../../src/rpc.ts';
import { readToken } from '../../src/config.ts';
import { nullLogger } from '../../src/log.ts';
import { PATCH, addBorrowScript, capture, commit, harness, isLocked, lines, makeBranch, makeRepo, notes, runTb, sh, startTb, threadWithWorktree, until, write } from '../helpers/worktrees-harness.ts';

// --------------------------------------------------------------- landing

test('the Land chip can wait beyond the ordinary command timeout', async () => {
  const h = makeHarness(undefined, {}, { actionShell: ['/bin/sh', '-c'], actionTimeoutMs: 20 });
  const { thread } = h.runner.create({ text: 'Go', accountId: 'claude-3', cwd: tmpDir() });
  const action = { id: 'worktree.land', title: 'Land', kind: 'command' as const, patch: 'worktree', command: 'sleep 0.1; echo checked' };
  h.runner.worktreeAction = () => action;
  assert.equal((await h.runner.runAction(thread.id, action.id)).ok, true);
  // A project action cannot opt into the built-in landing's longer limit.
  h.runner.worktreeAction = () => ({ ...action, patch: 'project' });
  assert.equal((await h.runner.runAction(thread.id, action.id)).ok, false);
});

test('land serializes separate CLIs before rebasing, so each checks the commit it lands once', async () => {
  const { root, main } = makeRepo(PATCH(`{ check: "./check.sh" }`));
  commit(main, { 'check.sh': `#!/bin/sh
echo checked >> '${root}/checks'
if test -f first.txt && ! test -f second.txt; then
  touch '${root}/checking'
  while ! test -f '${root}/release'; do sleep 0.05; done
fi
if test -f second.txt; then test -f first.txt && test "$1" = --latest; fi
` }, 'Add check');
  sh(main, 'chmod +x check.sh && git add check.sh && git commit -qm executable');
  const a = makeBranch(main, 'first');
  const b = makeBranch(main, 'second');
  commit(a.path, { 'first.txt': 'first\n', '.threadbear/patches/checks.ts': PATCH(`{ check: "./check.sh --latest" }`) }, 'First');
  commit(b.path, { 'second.txt': 'second\n' }, 'Second');
  const env = { THREADBEAR_HOME: path.join(root, 'no-runner'), THREADBEAR_THREAD_ID: undefined };
  const first = runTb(['land'], a.path, env);
  await until(() => fs.existsSync(path.join(root, 'checking')), 'first check');
  const second = startTb(['land'], b.path, env);
  try {
    await until(() => second.output().includes('Waiting for another landing'), 'second CLI waiting for landing');
  } finally {
    write(root, { release: '' });
  }
  const [one, two] = await Promise.all([first, second.done]);
  assert.equal(one.code, 0, one.out);
  assert.equal(two.code, 0, two.out);
  assert.match(two.out, /Waiting for another landing/);
  assert.equal(fs.readFileSync(path.join(root, 'checks'), 'utf8'), 'checked\nchecked\n');
  assert.equal(sh(main, 'git rev-parse main'), sh(b.path, 'git rev-parse HEAD'));
  assert.ok(fs.existsSync(path.join(main, 'first.txt')));
  assert.ok(fs.existsSync(path.join(main, 'second.txt')));
});

test('land refuses a branch changed by the check instead of merging an unchecked commit', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  commit(t.path, { 'first.txt': 'first\n' }, 'First');
  const before = sh(main, 'git rev-parse main');
  const c = capture();
  assert.equal(await land({ ...t, check: 'echo unchecked > extra.txt && git add extra.txt && git commit -qm unchecked' }, c.opts), 1);
  assert.match(c.err.join('\n'), /changed while the check ran/);
  assert.equal(sh(main, 'git rev-parse main'), before);
});

test('land releases its lock when a CLI is killed during the check', async () => {
  const { root, main } = makeRepo(PATCH(`{ check: "touch ../checking; while ! test -f ../release; do sleep 0.05; done" }`));
  const t = makeBranch(main);
  commit(t.path, { 'first.txt': 'first\n' }, 'First');
  const env = { THREADBEAR_HOME: path.join(root, 'no-runner'), THREADBEAR_THREAD_ID: undefined };
  const first = startTb(['land'], t.path, env);
  try {
    await until(() => fs.existsSync(path.join(t.path, '../checking')), 'CLI checking');
  } finally {
    first.child.kill('SIGKILL');
    await first.done;
  }
  // Acquire while the orphan check is still alive, so a pipe inherited
  // by that check cannot make a leaked lock look like a passing test.
  const next = land(t, capture().opts);
  // The limit covers the whole landing (lockf, node, a rebase and a dozen
  // git calls), not just the lock. A leaked lock waits for ever, so a long
  // limit still catches it; 10s failed a release build on a busy Mac (9 Nov).
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    assert.equal(await Promise.race([next, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error('Killed CLI kept its landing lock')), 90_000);
    })]), 0);
  } finally {
    clearTimeout(timer);
    write(path.dirname(t.path), { release: '' });
    await next;
  }
  assert.equal(sh(main, 'git rev-parse main'), sh(t.path, 'git rev-parse HEAD'));
});

test('land refuses tracked edits made by the check, and releases the lock on failure', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  commit(t.path, { 'first.txt': 'first\n' }, 'First');
  const before = sh(main, 'git rev-parse main');
  const c = capture();
  assert.equal(await land({ ...t, check: 'echo edited > first.txt' }, c.opts), 1);
  assert.match(c.err.join('\n'), /changed while the check ran/);
  assert.equal(sh(main, 'git rev-parse main'), before);
  sh(t.path, 'git restore first.txt');
  assert.equal(await land(t, capture().opts), 0);
});

// A release collects Important: with git's trailer parser, which only reads
// a message's last paragraph (9 Nov: seven security fixes went out unflagged).
test('land refuses an Important: line git does not read as a trailer, and lands one it does', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  write(t.path, { 'first.txt': 'first\n' });
  sh(t.path, `git add -A && git commit -q -m "Fix one" -m "Body." -m "Important: something a fork should hear" -m "Co-Authored-By: A <a@example.com>"`);
  const before = sh(main, 'git rev-parse main');
  const c = capture();
  assert.equal(await land(t, c.opts), 1);
  assert.match(c.err.join('\n'), /Fix one/);
  assert.match(c.err.join('\n'), /same last paragraph as Co-Authored-By/);
  assert.equal(sh(main, 'git rev-parse main'), before);
  // Fixed: Important: beside Co-Authored-By in the last paragraph.
  sh(t.path, `git commit -q --amend -m "Fix one" -m "Body." -m "Important: something a fork should hear
Co-Authored-By: A <a@example.com>"`);
  assert.equal(sh(t.path, `git log -1 --format='%(trailers:key=Important,valueonly)'`), 'something a fork should hear');
  const ok = capture();
  assert.equal(await land(t, ok.opts), 0, ok.err.join('\n'));
});

test('land ignores commits with no Important: line and ones already on main', async () => {
  const { main } = makeRepo();
  sh(main, `git commit -q --allow-empty -m "Old" -m "Important: old and misplaced" -m "Co-Authored-By: A <a@example.com>"`);
  const t = makeBranch(main);
  commit(t.path, { 'first.txt': 'first\n' }, 'First');
  const c = capture();
  assert.equal(await land(t, c.opts), 0, c.err.join('\n'));
});

test('tb land refuses a check patch that cannot load after rebasing', async () => {
  const { root, main } = makeRepo(PATCH(`{ check: "true" }`));
  const t = makeBranch(main);
  commit(t.path, { 'first.txt': 'first\n' }, 'First');
  commit(main, { '.threadbear/patches/checks.ts': 'export default { worktree: { check: ' }, 'Broken check');
  const before = sh(main, 'git rev-parse main');
  const result = await runTb(['land'], t.path, { THREADBEAR_HOME: path.join(root, 'no-runner'), THREADBEAR_THREAD_ID: undefined });
  assert.equal(result.code, 1, result.out);
  assert.match(result.out, /Couldn't read the repo's patches for its check/);
  assert.equal(sh(main, 'git rev-parse main'), before);
});

test('land refuses untracked inputs created during validation', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  commit(t.path, { 'first.txt': 'first\n' }, 'First');
  const before = sh(main, 'git rev-parse main');
  const c = capture();
  assert.equal(await land({ ...t, check: 'echo extra > extra.txt' }, c.opts), 1);
  assert.match(c.err.join('\n'), /changed while the check ran/);
  assert.equal(sh(main, 'git rev-parse main'), before);
});

test('land: rebases on main, runs the check in the worktree, fast-forwards main and says what landed', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  commit(t.path, { 'runner/b.ts': 'export const b = 2;\n' }, 'Add b');
  // main moved on meanwhile, in another file.
  commit(main, { 'other.txt': 'x\n' }, 'Other work');
  const c = capture();
  const code = await land({ ...t, check: 'test -f runner/b.ts && test -f other.txt && echo checked' }, c.opts);
  assert.equal(code, 0, c.err.join('\n'));
  assert.deepEqual(c.err, []);
  assert.equal(c.out[0], 'Running the check: test -f runner/b.ts && test -f other.txt && echo checked');
  assert.match(c.out[1], /^The check passed \(\d+ s\)\.$/);
  assert.equal(c.out[2], `Landed on main in ${main}:`);
  assert.match(c.out[3], /^[0-9a-f]+ Add b$/);
  assert.equal(c.out.length, 4, 'only what landed, not main\'s own commit');
  assert.equal(sh(main, 'git log -1 --format=%s main'), 'Add b');
  assert.ok(fs.existsSync(path.join(main, 'runner/b.ts')), "main's working tree has it");
  assert.equal(sh(main, 'git rev-parse main'), sh(t.path, 'git rev-parse HEAD'));

  // The thread carries on: the next land lands only what's new.
  const again = capture();
  assert.equal(await land(t, again.opts), 0);
  assert.deepEqual(again.out, [`Nothing to land: ${t.branch} has no commits that aren't on main.`]);
  commit(t.path, { 'runner/c.ts': '3\n' }, 'Add c');
  const third = capture();
  assert.equal(await land(t, third.opts), 0);
  assert.equal(third.out.length, 2);
  assert.match(third.out[1], /Add c$/);
});

test('land refuses with uncommitted changes in the worktree, and says which files', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  commit(t.path, { 'runner/b.ts': '2\n' }, 'Add b');
  write(t.path, { 'README.md': 'changed\n', 'new.txt': 'untracked\n' });
  const before = sh(main, 'git rev-parse main');
  const c = capture();
  assert.equal(await land({ ...t, check: 'touch ran-check' }, c.opts), 1);
  const text = c.err.join('\n');
  assert.match(text, new RegExp(`^There are uncommitted changes in ${t.path}, so nothing landed:\n  M README.md\n  \\?\\? new.txt\nCommit them`));
  assert.equal(sh(main, 'git rev-parse main'), before);
  assert.ok(!fs.existsSync(path.join(t.path, 'ran-check')), 'the check never ran');
});

test('land: a rebase conflict is undone and names the files', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  commit(t.path, { 'README.md': 'one\nTWO from the thread\nthree\n' }, 'Thread edit');
  commit(main, { 'README.md': 'one\nTWO on main\nthree\n' }, 'Main edit');
  const branchBefore = sh(t.path, 'git rev-parse HEAD');
  const mainBefore = sh(main, 'git rev-parse main');
  const c = capture();
  assert.equal(await land(t, c.opts), 1);
  const text = c.err.join('\n');
  assert.match(text, /^Rebasing thread\/abcd1234 on main ran into conflicts in:\n  README.md\nThe rebase was undone/);
  assert.match(text, /run tb land again\.$/);
  assert.equal(sh(t.path, 'git rev-parse HEAD'), branchBefore, 'the branch is as it was');
  assert.equal(sh(t.path, 'git status --porcelain'), '', 'no rebase left in progress');
  assert.equal(sh(main, 'git rev-parse main'), mainBefore);
});

test("land never rebases a branch with a merge on it (Mike's release), so his commits keep their ids", async () => {
  const { main } = makeRepo();
  // Mike's release: a commit of his that main doesn't have.
  sh(main, 'git checkout -q -b mike && echo his > his.txt && git add his.txt && git commit -qm "Mike\'s change" && git checkout -q main');
  const his = sh(main, 'git rev-parse mike');
  const t = makeBranch(main);
  sh(t.path, 'git merge -q --no-ff --no-edit mike');
  const c = capture();
  assert.equal(await land(t, c.opts), 0, c.err.join('\n'));
  sh(main, `git merge-base --is-ancestor ${his} main`);
  assert.equal(c.out.length, 2, 'what landed is the merge, not each of his commits');
  assert.match(c.out[1], /Merge branch 'mike'/);

  // Main moved meanwhile: main is merged in, still no rebase.
  sh(main, 'git checkout -q mike && echo more > more.txt && git add more.txt && git commit -qm "More of his" && git checkout -q main');
  const more = sh(main, 'git rev-parse mike');
  sh(t.path, 'git merge -q --no-ff --no-edit mike');
  commit(main, { 'other.txt': 'x\n' }, 'Other work');
  const again = capture();
  assert.equal(await land(t, again.opts), 0, again.err.join('\n'));
  sh(main, `git merge-base --is-ancestor ${more} main`);
  assert.ok(fs.existsSync(path.join(main, 'other.txt')) && fs.existsSync(path.join(main, 'more.txt')));
});

test('land: a failing check stops it with the last lines of output, and main stays put', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  commit(t.path, { 'runner/b.ts': '2\n' }, 'Add b');
  const before = sh(main, 'git rev-parse main');
  const c = capture();
  const check = 'for i in $(seq 1 60); do echo line $i; done; echo "1 test failed" >&2; exit 3';
  assert.equal(await land({ ...t, check }, c.opts), 1);
  const text = c.err.join('\n');
  assert.match(text, /^The check failed \(exit 3\), so main wasn't touched\. The last lines of its output:\n\n/);
  assert.match(text, /line 60\n1 test failed\n\nFix it, commit, and run tb land again\.$/);
  assert.doesNotMatch(text, /line 1\n/, 'only the end of a long output');
  assert.equal(sh(main, 'git rev-parse main'), before);
});

test("land refuses when main's checkout has uncommitted changes in files it would change, and leaves them", async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  commit(t.path, { 'README.md': 'one\ntwo\nthree\nfour\n' }, 'Thread edit');
  write(main, { 'README.md': 'someone else is editing\n' });
  const before = sh(main, 'git rev-parse main');
  const c = capture();
  assert.equal(await land(t, c.opts), 1);
  assert.equal(
    c.err.join('\n'),
    `The main checkout at ${main} has uncommitted changes in files this would change, so nothing landed:\n  README.md\n` +
      'Commit or move those changes there (someone else may be working on them), then run tb land again.',
  );
  assert.equal(sh(main, 'git rev-parse main'), before);
  assert.equal(fs.readFileSync(path.join(main, 'README.md'), 'utf8'), 'someone else is editing\n');
  // Changes elsewhere in main's checkout don't stand in the way.
  write(main, { 'README.md': 'one\ntwo\nthree\n', 'scratch.txt': 'mine\n' });
  sh(main, 'git checkout -- README.md');
  assert.equal(await land(t, capture().opts), 0);
  assert.equal(sh(main, 'git log -1 --format=%s'), 'Thread edit');
});

test('land --dry-run does the checks and says what would land, without moving main', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  commit(t.path, { 'runner/b.ts': '2\n' }, 'Add b');
  const before = sh(main, 'git rev-parse main');
  const c = capture();
  assert.equal(await land({ ...t, check: 'true' }, { ...c.opts, dryRun: true }), 0);
  assert.equal(c.out.at(-2), `Would land on main in ${main}:`);
  assert.match(c.out.at(-1)!, /Add b$/);
  assert.equal(sh(main, 'git rev-parse main'), before);
});

test('overwrittenFiles reads both of git\'s refusals', () => {
  const text =
    'error: Your local changes to the following files would be overwritten by merge:\n\tREADME.md\n\tsrc/a.ts\nPlease commit your changes or stash them before you merge.\n' +
    'error: The following untracked working tree files would be overwritten by merge:\n\tnew.txt\nPlease move or remove them before you merge.\nAborting\n';
  assert.deepEqual(overwrittenFiles(text), ['README.md', 'src/a.ts', 'new.txt']);
});

test('tb land outside a thread goes by git: the thread/ branch, main\'s checkout and the patch\'s check', async () => {
  const { main } = makeRepo(PATCH(`{ check: "echo checking in $PWD" }`));
  const t = makeBranch(main);
  commit(t.path, { 'runner/b.ts': '2\n' }, 'Add b');
  const env = { THREADBEAR_THREAD_ID: undefined, THREADBEAR_HOME: fs.realpathSync(tmpDir()), THREADBEAR_URL: 'ws://127.0.0.1:9/rpc' };
  const dry = await runTb(['land', '--dry-run'], path.join(t.path, 'runner'), env);
  assert.equal(dry.code, 0, dry.out);
  assert.match(dry.out, /^Running the check: echo checking in \$PWD\nThe check passed/);
  assert.match(dry.out, new RegExp(`Would land on main in ${main}:\n[0-9a-f]+ Add b\n$`));
  assert.equal(sh(main, 'git log -1 --format=%s'), 'init');

  const elsewhere = await runTb(['land'], main, env);
  assert.equal(elsewhere.code, 1);
  assert.equal(elsewhere.out, `${main} is on main, not a thread/ branch. tb land runs in a thread's own worktree.\n`);
  const wrongFlag = await runTb(['land', '--json'], t.path, env);
  assert.equal(wrongFlag.code, 2);
  assert.match(wrongFlag.out, /tb land doesn't take --json\. It takes --dry-run\./);
});

test('borrow-worktree.sh hands out a thread/ branch tb land takes, and --give-back deletes it and makes a spare', async () => {
  const { main } = makeRepo(PATCH(`{ check: "true" }`));
  addBorrowScript(main);
  const wt = sh(main, './borrow-worktree.sh 2>/dev/null');
  assert.equal(wt, path.join(path.dirname(main), 'proj-threads', 'slot-1'));
  const branch = sh(wt, 'git rev-parse --abbrev-ref HEAD');
  assert.match(branch, /^thread\/borrow-slot-1-\d{8}-\d{6}$/);

  commit(wt, { 'runner/b.ts': '2\n' }, 'Add b');
  const found = await landTargetFromGit(wt);
  assert.deepEqual(found.target, { path: wt, branch, mainCheckout: main });
  const c = capture();
  const landed = await land({ ...found.target!, check: 'true' }, c.opts);
  assert.equal(landed, 0, c.err.join('\n'));
  assert.equal(sh(main, 'git log -1 --format=%s'), 'Add b');

  assert.match(sh(main, `./borrow-worktree.sh --give-back ${wt}`), /is a spare again/);
  assert.equal(sh(main, `git branch --list '${branch}'`), '');
  assert.match(sh(main, 'git worktree list --porcelain'), /locked threadbear spare/);
  // The next borrow takes that spare, on a branch of its own.
  assert.equal(sh(main, './borrow-worktree.sh 2>/dev/null'), wt);
  assert.match(sh(wt, 'git rev-parse --abbrev-ref HEAD'), /^thread\/borrow-slot-1-/);

  // A worktree detached by hand is refused with what to do.
  sh(wt, 'git checkout -q --detach');
  assert.match((await landTargetFromGit(wt)).reason ?? '', /is detached.*git checkout -b thread\/<name>/);
});

test('a borrowed spare is never given to a new thread until it is given back', async () => {
  const { root, main } = makeRepo(PATCH(`{ setup: "touch .setup-ran", keep: ["app/.build"] }`));
  addBorrowScript(main);
  const patch = (await findWorktreePatch(main, root))!;
  const slot1 = path.join(root, 'proj-threads', 'slot-1');
  const first = await makeWorktree({ cwd: main, threadId: 'aaaaaaaa-1', patch });
  assert.ok(first.ok && first.worktree.path === slot1);
  assert.deepEqual(await cleanUpWorktree(first.worktree), { removed: true, spare: true });

  assert.equal(sh(main, './borrow-worktree.sh 2>/dev/null'), slot1, 'the borrow takes the spare');
  commit(slot1, { 'runner/borrowed.ts': '1\n' }, 'Borrowed work');
  const next = await makeWorktree({ cwd: main, threadId: 'bbbbbbbb-1', patch });
  assert.ok(next.ok);
  assert.notEqual(next.worktree.path, slot1, 'a new thread gets a slot of its own');
  assert.match(sh(slot1, 'git rev-parse --abbrev-ref HEAD'), /^thread\/borrow-slot-1-/);
  assert.equal(sh(slot1, 'git log -1 --format=%s'), 'Borrowed work', 'the borrower\'s work is untouched');

  sh(main, 'git merge -q --ff-only $(git -C ' + slot1 + ' rev-parse HEAD)');
  assert.match(sh(main, `./borrow-worktree.sh --give-back ${slot1}`), /is a spare again/);
  const after = await makeWorktree({ cwd: main, threadId: 'cccccccc-1', patch });
  assert.ok(after.ok);
  assert.equal(after.worktree.path, slot1, 'given back, it is a spare again');
});

test('a thread brought back never carries on in a borrowed worktree made where its own was', async () => {
  const { main } = makeRepo();
  addBorrowScript(main);
  const h = harness();
  const { id, w } = await threadWithWorktree(h, main);
  commit(w.path, { 'runner/b.ts': '2\n' }, 'Landed work');
  assert.equal(await land({ path: w.path, branch: w.branch, mainCheckout: main }, capture().opts), 0);
  h.runner.update({ threadId: id, archived: true });
  await until(() => notes(h, id).some((n) => n.startsWith('Removed')), 'the cleanup');
  // As on 8 Nov: the folder was made again at the same path, here by hand
  // as borrow-worktree.sh makes a new slot (git worktree add, detached).
  sh(main, `git worktree add -q --detach ${w.path} main`);
  write(w.path, { 'runner/borrower.ts': 'not yet committed\n' });

  h.runner.send(id, 'More please');
  await until(() => h.adapter.sessions.length === 2, 'a new CLI');
  assert.equal(h.adapter.last.opts.cwd, path.join(main, 'runner'), 'in the main checkout, not the borrowed folder');
  assert.equal(h.runner.get(id).cwd, path.join(main, 'runner'));
  assert.equal(sh(w.path, 'git rev-parse --abbrev-ref HEAD'), 'HEAD', 'the borrowed worktree is as it was');
  assert.ok(fs.existsSync(path.join(w.path, 'runner/borrower.ts')));
});

test("borrow-worktree.sh's --give-back refuses a slot that's a spare already or someone else's now", async () => {
  const { root, main } = makeRepo(PATCH(`{ setup: "touch .setup-ran", keep: ["app/.build"] }`));
  addBorrowScript(main);
  const patch = (await findWorktreePatch(main, root))!;
  const wt = sh(main, './borrow-worktree.sh 2>/dev/null');
  assert.match(sh(main, `./borrow-worktree.sh --give-back ${wt}`), /is a spare again/);
  assert.throws(() => sh(main, `./borrow-worktree.sh --give-back ${wt}`), /is already a spare/);
  // Taken by a new thread, then given back again by the old borrower.
  const b = await makeWorktree({ cwd: main, threadId: 'bbbbbbbb-1', patch });
  assert.ok(b.ok && b.worktree.path === wt);
  assert.throws(() => sh(main, `./borrow-worktree.sh --give-back ${wt}`), /is on thread\/bbbbbbbb, not a borrowed branch/);
  assert.equal(sh(wt, 'git rev-parse --abbrev-ref HEAD'), 'thread/bbbbbbbb', "the thread's checkout is untouched");
  assert.ok(!isLocked(main, wt));
});

test('a borrow and a new thread at once never take the same spare', async () => {
  const { root, main } = makeRepo(PATCH(`{ setup: "touch .setup-ran", keep: ["app/.build"] }`));
  addBorrowScript(main);
  const patch = (await findWorktreePatch(main, root))!;
  for (let i = 0; i < 3; i++) {
    const spare = await makeWorktree({ cwd: main, threadId: `aaaaaaa${i}-1`, patch });
    assert.ok(spare.ok);
    assert.deepEqual(await cleanUpWorktree(spare.worktree), { removed: true, spare: true });
    const borrow = new Promise<string>((resolve, reject) =>
      execFile('./borrow-worktree.sh', { cwd: main }, (e, out) => (e ? reject(e) : resolve(out.trim()))),
    );
    const [borrowed, made] = await Promise.all([borrow, makeWorktree({ cwd: main, threadId: `bbbbbbb${i}-1`, patch })]);
    assert.ok(made.ok);
    assert.notEqual(borrowed, made.worktree.path, `round ${i}: one spare, two takers`);
  }
});

test("archiving whose branch can't be deleted after the folder went still counts as removed", async () => {
  const { main } = makeRepo();
  const h = harness();
  const { id, w } = await threadWithWorktree(h, main);
  commit(w.path, { 'runner/b.ts': '2\n' }, 'Landed work');
  assert.equal(await land({ path: w.path, branch: w.branch, mainCheckout: main }, capture().opts), 0);
  // A ref lock someone else holds makes git branch -D fail.
  write(main, { [`.git/refs/heads/${w.branch}.lock`]: '' });
  h.runner.update({ threadId: id, archived: true });
  await until(() => notes(h, id).some((n) => n.startsWith('Removed this thread')), 'the cleanup');
  assert.match(notes(h, id).at(-1)!, new RegExp(`but its branch ${w.branch} stays`));
  assert.ok(!fs.existsSync(w.path));
  sh(main, `git worktree add -q --detach ${w.path} main`);
  h.runner.send(id, 'More please');
  await until(() => h.adapter.sessions.length === 2, 'a new CLI');
  assert.equal(h.adapter.last.opts.cwd, path.join(main, 'runner'), 'not the folder made again');
});

test("archiving waits for a patch action that's running in the worktree", async () => {
  const { main } = makeRepo(`export default { worktree: { setup: "true" }, actions: [{ title: "Go", run: "sleep 1; touch ran-here" }] };\n`);
  const h = harness();
  const { id, w } = await threadWithWorktree(h, main);
  commit(w.path, { 'runner/b.ts': '2\n' }, 'Landed work');
  assert.equal(await land({ path: w.path, branch: w.branch, mainCheckout: main }, capture().opts), 0);
  const action = h.runner.runAction(id, 'checks.go');
  await until(() => (h.runner as any).threads.get(id).rt.runningActions?.size === 1, 'the action to start');
  h.runner.update({ threadId: id, archived: true });
  const r = await action;
  assert.equal(r.ok, true, 'the folder was still there for it');
  await until(() => notes(h, id).some((n) => n.startsWith('Removed')), 'the cleanup, once it finished');
  assert.ok(!fs.existsSync(w.path));
});

test("an action never runs in a removed worktree's folder made again for someone else", async () => {
  const { main } = makeRepo(`export default { worktree: { setup: "true" }, actions: [{ title: "Go", run: "touch ran-here" }] };\n`);
  const h = harness();
  const { id, w } = await threadWithWorktree(h, main);
  commit(w.path, { 'runner/b.ts': '2\n' }, 'Landed work');
  assert.equal(await land({ path: w.path, branch: w.branch, mainCheckout: main }, capture().opts), 0);
  h.runner.update({ threadId: id, archived: true });
  await until(() => notes(h, id).some((n) => n.startsWith('Removed')), 'the cleanup');
  sh(main, `git worktree add -q --detach ${w.path} main`);
  await assert.rejects(h.runner.runAction(id, 'checks.go'), /was removed when it was archived/);
  assert.ok(!fs.existsSync(path.join(w.path, 'ran-here')));
  assert.ok(!fs.existsSync(path.join(w.path, 'runner', 'ran-here')));
});

test("tb land in a thread refuses another thread's thread/<id8> branch, but takes its own and borrowed ones", async () => {
  const { main } = makeRepo(PATCH(`{ check: "true" }`));
  const other = makeBranch(main, 'abcd1234');
  const refused = await landTargetFromGit(other.path, 'eeeeeeee-1111');
  assert.equal(refused.target, undefined);
  assert.match(refused.reason ?? '', /thread abcd1234's branch, not this thread's\. Another thread is working in this folder/);
  assert.match(refused.reason ?? '', /THREADBEAR_THREAD_ID= tb land/);
  assert.deepEqual((await landTargetFromGit(other.path, 'abcd1234-5678')).target, other, 'its own');
  assert.deepEqual((await landTargetFromGit(other.path)).target, other, 'outside a thread');
  const borrowed = path.join(path.dirname(main), 'proj-threads', 'slot-1');
  sh(main, `git worktree add -q -b thread/borrow-slot-1-20261108-102000 ${borrowed} main`);
  assert.ok((await landTargetFromGit(borrowed, 'eeeeeeee-1111')).target, 'a borrowed branch');
});

test('tb land inside a thread asks the runner where its worktree is, wherever it runs', async () => {
  const { main } = makeRepo(PATCH(`{ check: "test -f runner/b.ts" }`));
  const h = harness();
  const server = new RpcServer({ host: '127.0.0.1', port: 0, token: readToken(h.home), runner: h.runner, log: nullLogger });
  const { port } = await server.listen();
  try {
    const { thread } = h.runner.create({ text: 'Go', accountId: 'claude-3', cwd: main });
    await until(() => h.adapter.sessions.length === 1, 'the CLI to start');
    const wt = h.runner.get(thread.id).worktree!.path;
    commit(wt, { 'runner/b.ts': '2\n' }, 'Thread work');
    const r = await runTb(['land'], fs.realpathSync(tmpDir()), { THREADBEAR_THREAD_ID: thread.id, THREADBEAR_HOME: h.home, THREADBEAR_URL: `ws://127.0.0.1:${port}/rpc` });
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, new RegExp(`Landed on main in ${main}:\n[0-9a-f]+ Thread work\n$`));
    assert.equal(sh(main, 'git log -1 --format=%s'), 'Thread work');
  } finally {
    await server.close();
  }
});
