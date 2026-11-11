// move_thread with a runner, from a thread that asks first (security
// review, 10 Nov 2026): the card named the runner as the agent typed it
// ("cloud"), and the name was looked up only after Allow. Another Mac
// renaming itself "cloud" while the card was up got the thread, as an
// exact name beat "Hetzner Cloud" containing the word. Now the runner is
// picked before the card, shown by name and id, and the move goes to that
// id, checked again as it goes.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeHarness, tick, tmpDir } from '../helpers/fakes.ts';
import { buildThreadbearTools, type RemoteControl } from '../../src/threadbear-mcp.ts';
import { pickRunner, stillRunner } from '../../src/moves.ts';
import type { PermissionMode } from '../../src/protocol.ts';

const legit = { id: 'legitcloud01', name: 'Hetzner Cloud' };
const main = { id: 'main00000001', name: 'M5' };
const evil = { id: 'evilrunner01', name: 'Evil Box' };

test('a word that is one runner\'s whole name and part of another\'s names neither', () => {
  assert.deepEqual(pickRunner('cloud', [main, legit, evil]), legit);
  assert.deepEqual(pickRunner('legitcloud01', [main, legit, evil]), legit);
  assert.deepEqual(pickRunner('m5', [main, legit, evil]), main);
  const renamed = [main, legit, { ...evil, name: 'cloud' }];
  const r = pickRunner('cloud', renamed);
  assert.ok('problem' in r, JSON.stringify(r));
  assert.match('problem' in r ? r.problem : '', /Hetzner Cloud \(legitcloud01\) or cloud \(evilrunner01\)/);
  // Its id still names it, and so does the whole name no other contains.
  assert.deepEqual(pickRunner('legitcloud01', renamed), legit);
  assert.deepEqual(pickRunner('Hetzner Cloud', renamed), legit);
  assert.ok('problem' in pickRunner('nowhere', renamed));
  assert.ok('problem' in pickRunner('  ', renamed));
});

test('as the move goes, the runner must still be there under the name the card showed', () => {
  assert.deepEqual(stillRunner('legitcloud01', 'Hetzner Cloud', [main, legit]), legit);
  assert.throws(() => stillRunner('legitcloud01', 'Hetzner Cloud', [main]), /isn't there any more/);
  assert.throws(() => stillRunner('legitcloud01', 'Hetzner Cloud', [main, { ...legit, name: 'cloud' }]), /is called cloud now/);
});

function setup(mode: PermissionMode) {
  const h = makeHarness(tmpDir());
  const { thread: me } = h.runner.create({ text: 'Me', accountId: 'claude-3', permissionMode: mode });
  h.adapter.last.init();
  let runners = [main, legit, evil];
  const moved: unknown[] = [];
  const remote: RemoteControl = {
    list: async () => ({ runners: [], threads: [] }) as never,
    call: async () => ({}),
    runners: async () => runners,
    move: async (p) => {
      moved.push(p);
      return `Moving it to ${p.runnerName}.`;
    },
  };
  const list = buildThreadbearTools(h.runner, me.id, () => h.clock.now(), undefined, undefined, undefined, { sleep: async () => {}, remote: () => remote });
  const call = async (args: Record<string, unknown>) => {
    const t = list.find((x) => x.name === 'move_thread')!;
    const r = (await t.handler(args as never, {})) as { content: { text: string }[]; isError?: boolean };
    return { text: r.content.map((c) => c.text).join('\n'), error: Boolean(r.isError) };
  };
  const cards = () => h.events(me.id).filter((e) => e.type === 'approval.requested') as any[];
  return { h, me, moved, call, cards, rename: (to: typeof runners) => (runners = to) };
}

test('ask: the card names the runner by name and id, and a rename while it is up changes nothing', async () => {
  const s = setup('ask');
  const p = s.call({ runner: 'cloud' });
  let card: any;
  for (let i = 0; i < 50 && !card; i++) {
    await tick();
    card = s.cards().at(-1);
  }
  assert.ok(card, 'a card');
  assert.match(card.title, /to the runner Hetzner Cloud \(legitcloud01\)/);
  assert.equal(card.input.runner, 'legitcloud01');
  // The other Mac takes the word while the card is up.
  s.rename([main, legit, { ...evil, name: 'cloud' }]);
  s.h.runner.respondApproval({ threadId: s.me.id, approvalId: card.approvalId, decision: 'allow' });
  const r = await p;
  assert.equal(r.error, false, r.text);
  assert.deepEqual(s.moved, [{ threadId: s.me.id, runnerId: 'legitcloud01', runnerName: 'Hetzner Cloud' }]);
});

test('ask: an ambiguous word is refused before any card', async () => {
  const s = setup('ask');
  s.rename([main, legit, { ...evil, name: 'cloud' }]);
  const r = await s.call({ runner: 'cloud' });
  assert.equal(r.error, true);
  assert.match(r.text, /could be/);
  assert.equal(s.cards().length, 0);
  assert.deepEqual(s.moved, []);
});

test('full: no card, and the move still goes by the id picked', async () => {
  const s = setup('full');
  const r = await s.call({ runner: 'Hetzner' });
  assert.equal(r.error, false, r.text);
  assert.equal(s.cards().length, 0);
  assert.deepEqual(s.moved, [{ threadId: s.me.id, runnerId: 'legitcloud01', runnerName: 'Hetzner Cloud' }]);
});
