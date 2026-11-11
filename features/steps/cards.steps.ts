import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

Then('an approval card asks to run {string}', async function (this: ThreadbearWorld, command: string) {
  await this.until(`an approval for "${command}"`, (d) => d.transcript.rows.some((r) => r.kind === 'approval' && r.text.includes(command)));
});

When('I press {string}', async function (this: ThreadbearWorld, title: string) {
  await this.app.do({ cmd: 'click', title });
});

Then('the approval card has gone', async function (this: ThreadbearWorld) {
  await this.until('no approval card', (d) => !d.transcript.rows.some((r) => r.kind === 'approval'));
});

Then('the approval for {string} says it was denied', async function (this: ThreadbearWorld, command: string) {
  await this.until(`"${command}" denied`, (d) => d.transcript.rows.some((r) => r.kind === 'approval' && r.text.includes(command) && r.text.endsWith('(deny)')));
});

Then('a question card asks {string}', async function (this: ThreadbearWorld, question: string) {
  await this.until(`a question "${question}"`, (d) => d.transcript.rows.some((r) => r.kind === 'question' && r.text === question));
});

When('I pick the answer {string} on the question card', async function (this: ThreadbearWorld, answer: string) {
  await this.until('a question card', (d) => d.transcript.rows.some((r) => r.kind === 'question'));
  await this.app.do({ cmd: 'click', title: answer });
});

When('I send my answer', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', title: 'Send answer' });
});

Then('the thread ends with a card of things for me to decide', async function (this: ThreadbearWorld) {
  await this.until('a card of asks', (d) => d.transcript.rows.some((r) => r.kind === 'asks'));
});

// Queued calls: "queued" in a message makes the mock runner stack them
// (app/Tools/mock-runner/server.mjs).
When('the agent queues up several edits', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'send', text: 'Please make the queued edits' });
  await this.until('a card with queued calls', (d) => d.transcript.rows.some((r) => r.kind === 'approval'));
});

Then('a card asks to allow the {int} queued calls', async function (this: ThreadbearWorld, count: number) {
  // The card's Allow button says how many; pressing it is the next step,
  // so what's checked here is that the card is showing with its calls.
  const d = await this.until(`a stacked card of ${count} calls`, (dump) => (dump.transcript.rows.some((r) => r.kind === 'approval') ? dump : undefined));
  assert.ok(d.transcript.rows.some((r) => r.kind === 'approval'));
});

Then('a secret card asks for {string}', async function (this: ThreadbearWorld, label: string) {
  await this.until(`a secret card "${label}"`, (d) => d.transcript.rows.some((r) => r.kind === 'secret' && r.text === label));
});

When('I open the last folded work line', async function (this: ThreadbearWorld) {
  const row = await this.until('a folded work line', (d) => [...d.transcript.rows].reverse().find((r) => r.kind === 'group' && !r.expanded));
  await this.app.do({ cmd: 'expand', row: row.id });
});

Then('a review card says {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`a review card "${words}"`, (d) => d.transcript.rows.some((r) => r.kind === 'review' && r.text.includes(words)));
});
