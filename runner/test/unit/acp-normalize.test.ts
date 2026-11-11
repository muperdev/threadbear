// session/update frames as OpenCode 1.18.23 really sent them (9 Nov 2026),
// paths shortened, through the normaliser.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { AcpNormalizer, canonicalToolName } from '../../src/adapters/acp/normalize.ts';
import type { NormalizerContext } from '../../src/adapters/context.ts';

const ctx: NormalizerContext = {
  cwd: '/w',
  readFile: () => null,
  saveImage: (_d, mediaType) => ({ path: '/m/x.png', mediaType }),
  saveBlob: () => '/b',
  copyBlob: () => null,
};
const kinds = (out: { kind: string; event?: { type: string } }[]) => out.map((o) => (o.kind === 'event' ? o.event!.type : o.kind));

test('the words stream as deltas and end as one message', () => {
  const n = new AcpNormalizer(ctx);
  const a = n.handle({ sessionUpdate: 'agent_message_chunk', messageId: 'msg_1', content: { type: 'text', text: 'po' } });
  const b = n.handle({ sessionUpdate: 'agent_message_chunk', messageId: 'msg_1', content: { type: 'text', text: 'ng' } });
  assert.deepEqual(kinds([...a, ...b]), ['delta', 'delta']);
  const end = n.flush();
  assert.deepEqual(kinds(end), ['assistant.text']);
  assert.equal((end[0] as { event: { text: string } }).event.text, 'pong');
  assert.equal(n.takeLastAssistantText(), 'pong');
});

test('a new messageId starts a new message', () => {
  const n = new AcpNormalizer(ctx);
  n.handle({ sessionUpdate: 'agent_message_chunk', messageId: 'a', content: { type: 'text', text: 'one' } });
  const out = n.handle({ sessionUpdate: 'agent_message_chunk', messageId: 'b', content: { type: 'text', text: 'two' } });
  assert.deepEqual(kinds(out), ['assistant.text', 'delta']);
});

test('thinking ends when the words begin', () => {
  const n = new AcpNormalizer(ctx);
  n.handle({ sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: 'hmm' } });
  const out = n.handle({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'hi' } });
  assert.deepEqual(kinds(out), ['reasoning', 'delta']);
});

const pending = { sessionUpdate: 'tool_call', toolCallId: 'c1', title: 'write', kind: 'edit', status: 'pending', locations: [], rawInput: {} };
const running = { sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'in_progress', kind: 'edit', title: 'write', locations: [{ path: '/w/a.txt' }], rawInput: { filePath: '/w/a.txt', content: 'hi' } };

test('a call that opens empty waits for its input, then one row', () => {
  const n = new AcpNormalizer(ctx);
  assert.deepEqual(n.handle(pending), []);
  const out = n.handle(running);
  assert.deepEqual(kinds(out), ['tool.call']);
  const call = (out[0] as { event: { title: string; kind: string; input: unknown } }).event;
  assert.equal(call.title, 'Edit a.txt');
  assert.equal(call.kind, 'edit');
  assert.deepEqual(n.handle({ ...running, status: 'in_progress' }), [], 'and only one row');
});

test('a permission request brings the row and the preview, ahead of the card', () => {
  const n = new AcpNormalizer(ctx);
  n.handle(pending);
  const seen = n.noteCall({ toolCallId: 'c1', title: '/w/a.txt', kind: 'edit', status: 'pending', locations: [{ path: '/w/a.txt' }], rawInput: { filepath: '/w/a.txt', diff: 'x' }, content: [{ type: 'diff', path: '/w/a.txt', oldText: '', newText: 'hi' }] });
  assert.deepEqual(kinds(seen.events), ['tool.call', 'edit.diff']);
  assert.deepEqual(seen.paths, ['/w/a.txt']);
  assert.equal(seen.kind, 'edit');
});

test('a rejected call fails with what was said', () => {
  const n = new AcpNormalizer(ctx);
  n.handle(pending);
  n.handle(running);
  const out = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'failed', kind: 'edit', content: [{ type: 'content', content: { type: 'text', text: 'The user rejected permission to use this specific tool call.' } }], rawOutput: { error: 'x' } });
  assert.deepEqual(kinds(out), ['tool.result']);
  assert.equal((out[0] as { event: { ok: boolean; text: string } }).event.ok, false);
  assert.match((out[0] as { event: { text: string } }).event.text, /rejected/);
});

test('output only in rawOutput still shows', () => {
  const n = new AcpNormalizer(ctx);
  n.handle({ sessionUpdate: 'tool_call', toolCallId: 'c2', title: 'bash', kind: 'execute', status: 'in_progress', rawInput: { command: 'ls' } });
  const out = n.handle({ sessionUpdate: 'tool_call_update', toolCallId: 'c2', status: 'completed', rawOutput: { output: 'a b' } });
  assert.equal((out[0] as { event: { text: string } }).event.text, 'a b');
});

test('whatever is open closes when the agent goes', () => {
  const n = new AcpNormalizer(ctx);
  n.handle(pending);
  n.handle(running);
  const out = n.closeOpen('Interrupted');
  assert.deepEqual(kinds(out), ['tool.result']);
  assert.deepEqual(n.closeOpen('Interrupted'), [], 'once');
});

test('the threadbear tools read as they do on the other providers', () => {
  assert.equal(canonicalToolName('threadbear_end_turn'), 'mcp__threadbear__end_turn');
  assert.equal(canonicalToolName('bash'), 'bash');
  const n = new AcpNormalizer(ctx);
  const out = n.handle({ sessionUpdate: 'tool_call', toolCallId: 'c3', title: 'threadbear_end_turn', status: 'in_progress', rawInput: { summary: 'x' } });
  const call = (out[0] as { event: { name: string; kind: string } }).event;
  assert.equal(call.name, 'mcp__threadbear__end_turn');
  assert.equal(call.kind, 'mcp');
});

test('the plan, and what it does not know about', () => {
  const n = new AcpNormalizer(ctx);
  const plan = n.handle({ sessionUpdate: 'plan', entries: [{ content: 'a', status: 'completed', priority: 'high' }, { content: 'b', status: 'weird' }] });
  assert.deepEqual((plan[0] as { event: { entries: unknown } }).event.entries, [{ text: 'a', status: 'completed' }, { text: 'b', status: 'pending' }]);
  assert.deepEqual(n.handle({ sessionUpdate: 'something_new', x: 1 }), []);
  assert.deepEqual(n.handle('not an object'), []);
});
