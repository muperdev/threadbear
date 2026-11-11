// A team's hub (convex/team.ts, docs/multiplayer-design.md), run against the
// in-memory backend: who can make, join and leave a team, what a member key
// opens, and how projects, cards and messages move between members.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call, load, memoryBackend, outcome } from '../helpers/convex-memory.ts';

const OWNER = 'owner-key';
const { sha256 } = load('lib') as { sha256: (s: string) => Promise<string> };
const team = load('team');
const sync = load('sync');
const access = load('teamAccess');

const HOST_KEY = 'tbm_host_key_0123456789abcdef';
const SAM_KEY = 'tbm_sam_key_0123456789abcdef00';
const CODE = 'invite-code-0123456789';

async function setup() {
  process.env.RUNNER_KEY = OWNER;
  delete process.env.RUNNER_KEY_NEXT;
  const b = memoryBackend();
  await call(b.ctx, team.create, { ownerKey: OWNER, name: 'Collabute', memberId: 'm_host0001', memberName: 'Behrooz', keyHash: await sha256(HOST_KEY) });
  return b;
}

async function withSam() {
  const b = await setup();
  await call(b.ctx, team.invite, { key: HOST_KEY, codeHash: await sha256(CODE) });
  await call(b.ctx, team.join, { codeHash: await sha256(CODE), memberId: 'm_sam00001', name: 'Sam', keyHash: await sha256(SAM_KEY) });
  return b;
}

const project = (id = 'tp_0001abcd', extra: Record<string, unknown> = {}) => ({ id, title: 'Collabute', goals: 'G', state: 'S', notes: 'N', links: [], repos: ['git@github.com:collabute/gateway.git'], ...extra });

test('only the owner key makes a team, and only once', async () => {
  process.env.RUNNER_KEY = OWNER;
  const b = memoryBackend();
  const args = { name: 'T', memberId: 'm_host0001', memberName: 'B', keyHash: await sha256(HOST_KEY) };
  assert.equal(await outcome(call(b.ctx, team.create, { ...args, ownerKey: 'nope' })), 'auth');
  assert.equal(await outcome(call(b.ctx, team.create, { ...args, ownerKey: OWNER })), 'ok');
  assert.equal(await outcome(call(b.ctx, team.create, { ...args, ownerKey: OWNER })), 'exists');
  assert.equal(b.rows('teamMembers')[0]!.host, true);
  // Only the hash is kept.
  assert.ok(!JSON.stringify(b.rows('teamMembers')).includes(HOST_KEY));
});

test('an invite is good for one join, and only the host makes them', async () => {
  const b = await withSam();
  assert.equal(await outcome(call(b.ctx, team.join, { codeHash: await sha256(CODE), memberId: 'm_ana00001', name: 'Ana', keyHash: await sha256('tbm_ana_key_0123456789abcdef') })), 'auth');
  assert.equal(await outcome(call(b.ctx, team.invite, { key: SAM_KEY, codeHash: await sha256('another-code-01') })), 'forbidden');
  assert.equal(await outcome(call(b.ctx, team.invite, { key: 'not-a-member-key-at-all', codeHash: await sha256('another-code-01') })), 'auth');
});

test('an expired invite is refused, and a name already on the team', async () => {
  const b = await setup();
  await call(b.ctx, team.invite, { key: HOST_KEY, codeHash: await sha256('c1-0000000000'), days: 1 });
  b.rows('teamInvites')[0]!.expiresAt = Date.now() - 1;
  assert.equal(await outcome(call(b.ctx, team.join, { codeHash: await sha256('c1-0000000000'), memberId: 'm_x0000001', name: 'X', keyHash: await sha256('k'.repeat(30)) })), 'auth');
  await call(b.ctx, team.invite, { key: HOST_KEY, codeHash: await sha256('c2-0000000000') });
  assert.equal(await outcome(call(b.ctx, team.join, { codeHash: await sha256('c2-0000000000'), memberId: 'm_x0000001', name: ' behrooz ', keyHash: await sha256('k'.repeat(30)) })), 'exists');
});

test('a member key opens nothing the runners and phones use', async () => {
  const b = await withSam();
  // sync.ts checks keys against RUNNER_KEY and runnerKeys alone.
  assert.equal(await outcome(call(b.ctx, sync.heartbeat ?? sync.listThreads, { key: SAM_KEY, runnerId: 'm5runner0001' })), 'auth');
  // And the owner key is not a member key.
  assert.equal(await outcome(call(b.ctx, team.state, { key: OWNER + 'x'.repeat(20) })), 'auth');
});

test('state shows the team, its members and shared projects with their log', async () => {
  const b = await withSam();
  await call(b.ctx, team.shareProject, { key: HOST_KEY, project: project() });
  await call(b.ctx, team.addLog, { key: SAM_KEY, projectId: 'tp_0001abcd', text: 'We keep the schema change behind a flag.', threadTitle: 'Schema' });
  const s = await call(b.ctx, team.state, { key: SAM_KEY });
  assert.equal(s.team.name, 'Collabute');
  assert.equal(s.me.name, 'Sam');
  assert.deepEqual(s.members.map((m: { name: string }) => m.name).sort(), ['Behrooz', 'Sam']);
  assert.equal(s.projects.length, 1);
  assert.deepEqual(s.projects[0].repos, ['github.com/collabute/gateway']);
  assert.equal(s.projects[0].log[0].text, 'We keep the schema change behind a flag.');
  assert.ok(!JSON.stringify(s).includes('keyHash'));
});

test('any member updates a shared project; a write that changes nothing is skipped', async () => {
  const b = await withSam();
  await call(b.ctx, team.shareProject, { key: HOST_KEY, project: project() });
  const r = await call(b.ctx, team.updateProject, { key: SAM_KEY, id: 'tp_0001abcd', state: 'Reviews ship Friday', addRepos: ['https://github.com/collabute/desktop'] });
  assert.equal(r.changed, true);
  const row = b.rows('teamProjects')[0]!;
  assert.equal(row.state, 'Reviews ship Friday');
  assert.equal(row.updatedBy, 'm_sam00001');
  assert.deepEqual(row.repos, ['github.com/collabute/gateway', 'github.com/collabute/desktop']);
  assert.equal((await call(b.ctx, team.updateProject, { key: SAM_KEY, id: 'tp_0001abcd', state: 'Reviews ship Friday' })).changed, false);
  assert.equal(await outcome(call(b.ctx, team.updateProject, { key: SAM_KEY, id: 'tp_0001abcd', goals: 'x'.repeat(3000) })), 'badArgs');
});

test('only whoever shared it or the host stops sharing it, and its cards and log go', async () => {
  const b = await withSam();
  await call(b.ctx, team.shareProject, { key: HOST_KEY, project: project() });
  await call(b.ctx, team.putCards, { key: SAM_KEY, cards: [{ projectId: 'tp_0001abcd', threadId: 'thread0001', title: 'T', status: 'working' }] });
  assert.equal(await outcome(call(b.ctx, team.unshareProject, { key: SAM_KEY, id: 'tp_0001abcd' })), 'forbidden');
  await call(b.ctx, team.unshareProject, { key: HOST_KEY, id: 'tp_0001abcd' });
  assert.equal(b.rows('teamCards').length, 0);
  assert.equal((await call(b.ctx, team.state, { key: SAM_KEY })).projects.length, 0);
});

test('cards: each member writes only their own, and a gone thread takes its card', async () => {
  const b = await withSam();
  await call(b.ctx, team.shareProject, { key: HOST_KEY, project: project() });
  await call(b.ctx, team.putCards, { key: SAM_KEY, cards: [{ projectId: 'tp_0001abcd', threadId: 'thread0001', title: 'Review feature', status: 'working', headline: 'Wiring the API', repo: 'git@github.com:collabute/desktop.git' }] });
  await call(b.ctx, team.putCards, { key: HOST_KEY, cards: [{ projectId: 'tp_0001abcd', threadId: 'thread0001', title: 'Same id, other member', status: 'idle' }] });
  const live = await call(b.ctx, team.live, { key: HOST_KEY });
  assert.equal(live.cards.length, 2);
  const sams = live.cards.find((c: { memberId: string }) => c.memberId === 'm_sam00001');
  assert.equal(sams.repo, 'github.com/collabute/desktop');
  await call(b.ctx, team.putCards, { key: SAM_KEY, cards: [{ projectId: 'tp_0001abcd', threadId: 'thread0001', title: 'Review feature', status: 'needsThem' }] });
  assert.equal(b.rows('teamCards').find((c) => c.memberId === 'm_sam00001')!.status, 'needsThem');
  await call(b.ctx, team.putCards, { key: SAM_KEY, cards: [], gone: [{ projectId: 'tp_0001abcd', threadId: 'thread0001' }] });
  assert.deepEqual(b.rows('teamCards').map((c) => c.memberId), ['m_host0001']);
  assert.equal(await outcome(call(b.ctx, team.putCards, { key: SAM_KEY, cards: [{ projectId: 'tp_0001abcd', threadId: 't2xxxxxx', title: 'x', status: 'flying' }] })), 'badArgs');
});

test('a message waits in the recipient\'s inbox, and the sender hears how it was answered', async () => {
  const b = await withSam();
  await call(b.ctx, team.shareProject, { key: HOST_KEY, project: project() });
  const msg = { id: 'tm_00000001', projectId: 'tp_0001abcd', kind: 'message', to: 'm_host0001', toThread: 'thread0009', fromThread: 'thread0001', fromTitle: 'Schema', text: 'Hold the migration until my schema lands.' };
  await call(b.ctx, team.send, { key: SAM_KEY, message: msg });
  assert.equal(await outcome(call(b.ctx, team.send, { key: SAM_KEY, message: msg })), 'exists');
  assert.equal(await outcome(call(b.ctx, team.send, { key: SAM_KEY, message: { ...msg, id: 'tm_00000002', to: 'm_sam00001' } })), 'badArgs');
  const inbox = (await call(b.ctx, team.live, { key: HOST_KEY })).inbox;
  assert.equal(inbox.length, 1);
  assert.equal(inbox[0].from, 'm_sam00001');
  // Not Sam's to answer, and not with a handover's answer.
  assert.equal(await outcome(call(b.ctx, team.answer, { key: SAM_KEY, id: 'tm_00000001', status: 'passed' })), 'forbidden');
  assert.equal(await outcome(call(b.ctx, team.answer, { key: HOST_KEY, id: 'tm_00000001', status: 'taken' })), 'forbidden');
  await call(b.ctx, team.answer, { key: HOST_KEY, id: 'tm_00000001', status: 'passed' });
  assert.equal((await call(b.ctx, team.live, { key: HOST_KEY })).inbox.length, 0);
  const answered = (await call(b.ctx, team.live, { key: SAM_KEY })).answered;
  assert.equal(answered[0].status, 'passed');
  await call(b.ctx, team.told, { key: SAM_KEY, ids: ['tm_00000001'] });
  assert.equal((await call(b.ctx, team.live, { key: SAM_KEY })).answered.length, 0);
});

test('a handover marks the card, and declining it clears the mark', async () => {
  const b = await withSam();
  await call(b.ctx, team.shareProject, { key: HOST_KEY, project: project() });
  await call(b.ctx, team.putCards, { key: SAM_KEY, cards: [{ projectId: 'tp_0001abcd', threadId: 'thread0001', title: 'On-call', status: 'idle' }] });
  await call(b.ctx, team.send, { key: SAM_KEY, message: { id: 'tm_00000003', projectId: 'tp_0001abcd', kind: 'handover', to: 'm_host0001', fromThread: 'thread0001', fromTitle: 'On-call', text: 'Two incidents open.', repo: 'https://github.com/collabute/infra', branch: 'fix/alarms', links: [{ title: 'PR', url: 'https://github.com/collabute/infra/pull/9' }] } });
  assert.equal(b.rows('teamCards')[0]!.handedTo, 'm_host0001');
  assert.equal(b.rows('teamMessages')[0]!.repo, 'github.com/collabute/infra');
  await call(b.ctx, team.answer, { key: HOST_KEY, id: 'tm_00000003', status: 'declined', answer: 'On leave this week' });
  assert.equal(b.rows('teamCards')[0]!.handedTo, undefined);
});

test('leaving drops your cards and answers what was waiting for you', async () => {
  const b = await withSam();
  await call(b.ctx, team.shareProject, { key: HOST_KEY, project: project() });
  await call(b.ctx, team.putCards, { key: SAM_KEY, cards: [{ projectId: 'tp_0001abcd', threadId: 'thread0001', title: 'T', status: 'working' }] });
  await call(b.ctx, team.send, { key: HOST_KEY, message: { id: 'tm_00000004', projectId: 'tp_0001abcd', kind: 'message', to: 'm_sam00001', fromThread: 'thread0002', fromTitle: 'X', text: 'hi' } });
  assert.equal(await outcome(call(b.ctx, team.leave, { key: HOST_KEY })), 'forbidden');
  await call(b.ctx, team.leave, { key: SAM_KEY });
  assert.equal(b.rows('teamCards').length, 0);
  assert.equal(b.rows('teamMessages')[0]!.status, 'dismissed');
  assert.equal(await outcome(call(b.ctx, team.state, { key: SAM_KEY })), 'auth');
});

test('the host removes a member, who then has no way in', async () => {
  const b = await withSam();
  assert.equal(await outcome(call(b.ctx, team.remove, { key: SAM_KEY, memberId: 'm_host0001' })), 'forbidden');
  await call(b.ctx, team.remove, { key: HOST_KEY, memberId: 'm_sam00001' });
  assert.equal(await outcome(call(b.ctx, team.live, { key: SAM_KEY })), 'auth');
});

test('remotes normalise to one form', () => {
  for (const r of ['git@github.com:Collabute/Gateway.git', 'https://github.com/collabute/gateway', 'ssh://git@github.com/collabute/gateway.git', 'https://user@github.com/collabute/gateway/']) {
    assert.equal(access.normalRepo(r), 'github.com/collabute/gateway', r);
  }
  assert.equal(access.normalRepo('/Users/me/repo'), undefined);
});
