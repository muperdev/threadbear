// Shared projects (team.ts, docs/multiplayer-design.md): two runners, each
// with its own threads and projects, on one team whose hub is the real
// convex/team.ts running against the in-memory backend.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { makeHarness, tick, tmpDir } from '../helpers/fakes.ts';
import { call, load, memoryBackend } from '../helpers/convex-memory.ts';
import { Projects } from '../../src/projects.ts';
import { Team, cardRef, cardStatus, formatInvite, normalRepo, parseInvite, type Hub } from '../../src/team.ts';
import { buildThreadbearTools, type TeamControl } from '../../src/threadbear-mcp.ts';
import { nullLogger } from '../../src/log.ts';
import type { ThreadSummary } from '../../src/protocol.ts';

const OWNER = 'owner-key';
const HUB_URL = 'https://hub-team.convex.cloud';
const teamFns = load('team');
const access = load('teamAccess');

// The hub: posts run convex/team.ts's handlers, and every subscription is
// run again after each mutation, as Convex would.
function memoryHub() {
  process.env.RUNNER_KEY = OWNER;
  delete process.env.RUNNER_KEY_NEXT;
  const b = memoryBackend();
  const subs = new Set<{ fn: string; args: Record<string, unknown>; onUpdate: (v: unknown) => void; onError: (e: Error) => void }>();
  const rerun = async () => {
    for (const s of [...subs]) {
      try {
        s.onUpdate(await call(b.ctx, teamFns[s.fn.split(':')[1]!], s.args));
      } catch (e) {
        s.onError(e as Error);
      }
    }
  };
  const hub = (): Hub => ({
    post: async (kind, fn, args) => {
      const r = await call(b.ctx, teamFns[fn.split(':')[1]!], args);
      if (kind === 'mutation') await rerun();
      return r;
    },
    subscriber: {
      subscribe: (fn, args, onUpdate, onError) => {
        const s = { fn, args, onUpdate, onError };
        subs.add(s);
        void call(b.ctx, teamFns[fn.split(':')[1]!], args).then(onUpdate, onError);
        return () => subs.delete(s);
      },
      close: () => {},
    },
  });
  return { b, hub };
}

// One person's runner: its own threads and projects, and a Team on the hub.
function person(hubOf: () => Hub, remotes: Record<string, string> = {}) {
  const home = tmpDir();
  const root = tmpDir();
  const projects = new Projects({ home, clock: undefined as never, log: nullLogger, userHome: root });
  const h = makeHarness(home, {}, { projects });
  (projects as unknown as { clock: unknown }).clock = h.clock;
  projects.start(h.runner);
  const repo = (name: string, remote: string) => {
    const dir = path.join(root, name);
    fs.mkdirSync(path.join(dir, '.git'), { recursive: true });
    remotes[dir] = remote;
    return dir;
  };
  const team = new Team({
    home,
    clock: h.clock,
    log: nullLogger,
    on: () => true,
    projects,
    ownBackend: () => ({ url: HUB_URL, key: OWNER }),
    hub: hubOf,
    remoteOf: async (f) => normalRepo(remotes[f]),
    searchRoots: () => [root],
    host: {
      threads: () => h.runner.list(['inbox', 'snoozed', 'archived']),
      thread: (id) => (h.runner.has(id) ? h.runner.get(id) : undefined),
      note: (id, text) => void h.runner.editThread(id, () => {}, { note: text }),
      send: (id, text) => void h.runner.send(id, text),
      create: (p) => h.runner.create({ text: p.text, cwd: p.cwd ?? root, title: p.title, projectIds: p.projectIds, accountId: 'claude-3' }).thread,
      publish: (n) => h.runner.publish(n),
      onNotification: (fn) => h.runner.onNotification(fn),
      defaultCwd: () => root,
    },
  });
  projects.useTeamBrief((rec) => team.brief(rec));
  team.start();
  const control = (): TeamControl => ({
    joined: () => team.joined(),
    isCardRef: (r) => team.isCardRef(r),
    sendToCard: (p) => team.sendToCard(p),
    handOver: (p) => team.handOver(p),
    linesFor: (id) => team.projectLines(projects.record(id)!),
    logNote: (id, text, t) => team.logNote(id, text, t),
    memberNames: () => team.members().filter((m) => !m.me).map((m) => m.name),
  });
  const notes = (threadId: string) => h.events(threadId).filter((e) => e.type === 'system.note').map((e) => e.text as string);
  // Lets queued cards and project changes go up.
  const settle = async () => {
    for (let i = 0; i < 3; i++) {
      h.clock.advance(11_000);
      await tick(10);
    }
  };
  return { h, home, root, projects, team, repo, control, notes, settle };
}

async function twoOnATeam() {
  const { b, hub } = memoryHub();
  const remotes: Record<string, string> = {};
  const host = person(hub, remotes);
  const sam = person(hub, remotes);
  await host.team.create({ name: 'Collabute', memberName: 'Behrooz' });
  const { invite } = await host.team.invite();
  await sam.team.join({ invite, name: 'Sam' });
  await tick(10);
  return { b, host, sam };
}

test('invites print and parse, and a bad one is refused', () => {
  const inv = formatInvite('https://x-y-1.convex.cloud/', 'abcdefghijklmnopqrstuv');
  assert.equal(inv, 'tbteam:https://x-y-1.convex.cloud#abcdefghijklmnopqrstuv');
  assert.deepEqual(parseInvite(inv), { url: 'https://x-y-1.convex.cloud', code: 'abcdefghijklmnopqrstuv' });
  assert.throws(() => parseInvite('https://example.com'), /not a team invite/);
});

test("the runner's remotes match the hub's", () => {
  for (const r of ['git@github.com:a/b.git', 'https://github.com/A/B', 'ssh://git@gitlab.com:22/x/y/z.git', '/local/path', '']) assert.equal(normalRepo(r), access.normalRepo(r), r);
});

test('a card says what its thread is doing', () => {
  const t = (x: Partial<ThreadSummary>) => ({ status: 'idle', needsYou: false, ...x }) as ThreadSummary;
  assert.equal(cardStatus(t({ status: 'running' })), 'working');
  assert.equal(cardStatus(t({ needsYou: true })), 'needsThem');
  assert.equal(cardStatus(t({ status: 'waiting' })), 'needsThem');
  assert.equal(cardStatus(t({ archivedAt: 'x', status: 'running' })), 'done');
  assert.equal(cardStatus(t({})), 'idle');
  assert.equal(cardRef('Sam Lee', 'abcdef1234567'), 'team:sam-lee:abcdef12');
});

test('making, inviting and joining: both see the team, and keys stay on their Macs', async () => {
  const { b, host, sam } = await twoOnATeam();
  assert.deepEqual(host.team.state().members.map((m) => m.name).sort(), ['Behrooz', 'Sam']);
  assert.equal(sam.team.state().name, 'Collabute');
  assert.equal(sam.team.state().me?.name, 'Sam');
  const saved = JSON.parse(fs.readFileSync(path.join(sam.home, 'secrets/team.json'), 'utf8'));
  assert.equal((fs.statSync(path.join(sam.home, 'secrets/team.json')).mode & 0o777).toString(8), '600');
  assert.ok(!JSON.stringify(b.rows('teamMembers')).includes(saved.key));
});

test('a shared project stays in step both ways, folders stay each person\'s own', async () => {
  const { host, sam } = await twoOnATeam();
  const hostRepo = host.repo('gateway', 'git@github.com:collabute/gateway.git');
  const p = host.projects.create({ title: 'Collabute', folders: [hostRepo], goals: 'Ship reviews', state: 'Designing', by: 'mike' });
  await host.team.share({ projectId: p.id });
  const shared = host.team.state().projects[0]!;
  assert.deepEqual(shared.repos, ['github.com/collabute/gateway']);
  // Sam's clone is somewhere else on his Mac.
  const samRepo = sam.repo('work/gw', 'https://github.com/collabute/gateway');
  const added = await sam.team.add({ sharedId: shared.id });
  const samP = sam.projects.record(added.projectId)!;
  assert.deepEqual(samP.folders, [samRepo]);
  assert.equal(samP.goals, 'Ship reviews');
  // Sam's agent moves the state on: Behrooz's copy follows.
  sam.projects.update(samP.id, { state: 'API done, UI next', by: 'agent' });
  await sam.settle();
  await host.settle();
  assert.equal(host.projects.record(p.id)!.state, 'API done, UI next');
  // And back the other way, without an echo.
  host.projects.update(p.id, { goals: 'Ship reviews by Friday' });
  await host.settle();
  await sam.settle();
  assert.equal(sam.projects.record(samP.id)!.goals, 'Ship reviews by Friday');
  assert.equal(sam.projects.record(samP.id)!.state, 'API done, UI next');
});

test("the board and the brief: each sees the other's threads, not their own", async () => {
  const { host, sam } = await twoOnATeam();
  const p = host.projects.create({ title: 'Collabute', by: 'mike' });
  await host.team.share({ projectId: p.id });
  const { projectId: samP } = await sam.team.add({ sharedId: host.team.state().projects[0]!.id });
  const t = host.h.runner.create({ text: 'Build the review feature', cwd: host.root, accountId: 'claude-3', projectIds: [p.id] }).thread;
  host.h.runner.update({ threadId: t.id, title: 'Review feature' } as never);
  await host.settle();
  await sam.settle();
  const cards = sam.team.state().cards.filter((c) => !c.mine);
  assert.equal(cards.length, 1);
  assert.equal(cards[0]!.memberName, 'Behrooz');
  assert.equal(cards[0]!.id, cardRef('Behrooz', t.id));
  // A thread of Sam's in his copy is told about it when it starts.
  const brief = sam.projects.brief({ projects: [{ id: samP, by: 'mike', at: 'x' }] } as never)!;
  assert.match(brief, /Shared with the Collabute team: Behrooz/);
  assert.match(brief, new RegExp(cards[0]!.id));
  // Behrooz's own brief doesn't list his own thread.
  assert.doesNotMatch(host.projects.brief({ projects: [{ id: p.id, by: 'mike', at: 'x' }] } as never)!, /Review feature/);
});

test('a teammate\'s message waits for a pass-on, then reaches the thread; the sender hears', async () => {
  const { host, sam } = await twoOnATeam();
  const p = host.projects.create({ title: 'Collabute', by: 'mike' });
  await host.team.share({ projectId: p.id });
  const { projectId: samP } = await sam.team.add({ sharedId: host.team.state().projects[0]!.id });
  const target = host.h.runner.create({ text: 'Migration', cwd: host.root, accountId: 'claude-3', projectIds: [p.id] }).thread;
  const from = sam.h.runner.create({ text: 'Schema change', cwd: sam.root, accountId: 'claude-3', projectIds: [samP] }).thread;
  await host.settle();
  await sam.settle();
  const ref = cardRef('Behrooz', target.id);
  // Through the agent's own tool.
  const tools = buildThreadbearTools(sam.h.runner as never, from.id, Date.now, undefined, undefined, undefined, { team: sam.control });
  const send = tools.find((x) => x.name === 'send_message')!;
  const out = await send.handler({ threadId: ref, text: 'Hold the migration until my schema lands.' }, {});
  assert.match(JSON.stringify(out), /Sent to Behrooz's thread/);
  await tick(10);
  const waiting = host.team.state().waiting;
  assert.equal(waiting.length, 1);
  assert.equal(waiting[0]!.fromName, 'Sam');
  assert.ok(host.notes(target.id).some((n) => /Sam's agent sent this thread a message/.test(n)));
  const before = host.h.adapter.sessions.flatMap((s) => s.sent.map((x) => JSON.stringify(x))).join('\n');
  assert.doesNotMatch(before, /Hold the migration/);
  await host.team.answer({ id: waiting[0]!.id, action: 'pass' });
  await tick(10);
  const after = host.h.adapter.sessions.flatMap((s) => s.sent.map((x) => JSON.stringify(x))).join('\n');
  assert.match(after, /From Sam's agent/);
  assert.match(after, /Hold the migration/);
  await sam.settle();
  assert.ok(sam.notes(from.id).some((n) => /Behrooz passed your message on/.test(n)));
});

test('a project set to deliver passes messages straight in', async () => {
  const { host, sam } = await twoOnATeam();
  const p = host.projects.create({ title: 'Collabute', by: 'mike' });
  await host.team.share({ projectId: p.id });
  host.team.setDelivery({ projectId: p.id, delivery: 'deliver' });
  await sam.team.add({ sharedId: host.team.state().projects[0]!.id });
  const target = host.h.runner.create({ text: 'Migration', cwd: host.root, accountId: 'claude-3', projectIds: [p.id] }).thread;
  await host.settle();
  await sam.settle();
  await sam.team.sendToCard({ ref: cardRef('Behrooz', target.id), text: 'Schema landed.', fromThreadId: 'samthread01' });
  await tick(20);
  assert.equal(host.team.state().waiting.length, 0);
  assert.match(host.h.adapter.sessions.flatMap((s) => s.sent.map((x) => JSON.stringify(x))).join('\n'), /Schema landed/);
});

test('a handover waits on their Mac; taking it starts a thread in the project with the brief', async () => {
  const { host, sam } = await twoOnATeam();
  const hostRepo = host.repo('infra', 'git@github.com:collabute/infra.git');
  const p = host.projects.create({ title: 'On-call', folders: [hostRepo], by: 'mike' });
  await host.team.share({ projectId: p.id });
  const samRepo = sam.repo('code/infra', 'https://github.com/collabute/infra.git');
  const { projectId: samP } = await sam.team.add({ sharedId: host.team.state().projects[0]!.id });
  const t = host.h.runner.create({ text: 'Alarms', cwd: hostRepo, accountId: 'claude-3', projectIds: [p.id] }).thread;
  await host.settle();
  const tools = buildThreadbearTools(host.h.runner as never, t.id, Date.now, undefined, undefined, undefined, { team: host.control });
  const hand = tools.find((x) => x.name === 'hand_over');
  assert.ok(hand, 'hand_over is there on a team');
  await hand!.handler({ to: 'sam', note: 'Two alarms still flapping; the fix is half done.', links: [{ title: 'PR', url: 'https://github.com/collabute/infra/pull/9' }] }, {});
  await tick(10);
  const w = sam.team.state().waiting;
  assert.equal(w.length, 1);
  assert.equal(w[0]!.kind, 'handover');
  assert.equal(w[0]!.repo, 'github.com/collabute/infra');
  const r = await sam.team.answer({ id: w[0]!.id, action: 'take' });
  const made = sam.h.runner.get(r.threadId!);
  assert.equal(made.cwd, samRepo);
  assert.deepEqual(made.projects?.map((x) => x.id), [samP]);
  assert.match(sam.h.adapter.sessions.flatMap((s) => s.sent.map((x) => JSON.stringify(x))).join('\n'), /Handover from Behrooz[\s\S]*Two alarms still flapping[\s\S]*pull\/9/);
  await host.settle();
  assert.ok(host.notes(t.id).some((n) => /Sam took over this work/.test(n)));
});

test('appendNote on a shared project reaches the team log and teammates\' read_project', async () => {
  const { host, sam } = await twoOnATeam();
  const p = host.projects.create({ title: 'Collabute', by: 'mike' });
  await host.team.share({ projectId: p.id });
  const { projectId: samP } = await sam.team.add({ sharedId: host.team.state().projects[0]!.id });
  await host.team.logNote(p.id, 'Decided: reviews live behind the reviews flag until Friday.');
  await tick(10);
  const lines = sam.team.projectLines(sam.projects.record(samP)!).join('\n');
  assert.match(lines, /Behrooz: Decided: reviews live behind the reviews flag/);
});

test('without a team there is no hand_over, and team: ids are not taken', async () => {
  const solo = person(() => memoryHub().hub());
  const t = solo.h.runner.create({ text: 'x', cwd: solo.root, accountId: 'claude-3' }).thread;
  const tools = buildThreadbearTools(solo.h.runner as never, t.id, Date.now, undefined, undefined, undefined, { team: () => undefined });
  assert.equal(tools.find((x) => x.name === 'hand_over'), undefined);
  assert.equal(solo.team.state().joined, false);
});

test('leaving keeps the linked project as your own', async () => {
  const { host, sam } = await twoOnATeam();
  const p = host.projects.create({ title: 'Collabute', by: 'mike' });
  await host.team.share({ projectId: p.id });
  const { projectId: samP } = await sam.team.add({ sharedId: host.team.state().projects[0]!.id });
  await sam.team.leave();
  assert.equal(sam.team.state().joined, false);
  assert.equal(sam.projects.record(samP)!.team, undefined);
  assert.equal(sam.projects.record(samP)!.title, 'Collabute');
  assert.ok(!fs.existsSync(path.join(sam.home, 'secrets/team.json')));
});
