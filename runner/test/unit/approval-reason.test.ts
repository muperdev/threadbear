import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeHarness, tick } from '../helpers/fakes.ts';
import { checkEvent } from '../helpers/protocol-check.ts';

// David, 8 Nov 2026: a card says why the CLI asked (Auto's safety check, a
// rule, a hook), so he can judge it without reading the agent's working.
test("approval.requested carries the CLI's reason, and leaves it out when there's none", async () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'Deploy the pages', accountId: 'claude-3', permissionMode: 'auto' });
  const s = h.adapter.last;
  s.init();
  const signal = new AbortController().signal;
  void s.sink.requestApproval({ toolUseId: 't1', name: 'Bash', title: './deploy.sh', input: { command: './deploy.sh' }, canAlwaysAllow: true, signal, reason: "Auto mode's safety check couldn't clear this: Deploys to production." });
  void s.sink.requestApproval({ toolUseId: 't2', name: 'Bash', title: 'ls', input: { command: 'ls' }, canAlwaysAllow: true, signal });
  await tick();
  const cards = h.events(thread.id).filter((e) => e.type === 'approval.requested');
  assert.equal(cards[0].reason, "Auto mode's safety check couldn't clear this: Deploys to production.");
  assert.equal('reason' in cards[1], false);
  for (const c of cards) assert.deepEqual(checkEvent(c), []);
});
