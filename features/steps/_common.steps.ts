// Steps more than one feature uses. Files starting with _ don't own a
// feature (features/README.md).
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

Given('the app is connected to a runner with the demo threads', async function (this: ThreadbearWorld) {
  await this.until('the app connected, with its threads listed', (d) => d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length > 0), 30_000);
});

When('I open the thread {string}', async function (this: ThreadbearWorld, title: string) {
  await this.open(title);
});

// `then.starts` is on a dump's row text, which the Swift app cuts short.
Then('the transcript shows my message {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`my message "${words}"`, (d) => d.transcript.rows.some((r) => r.kind === 'user' && r.text.startsWith(words)));
});

Then('the transcript doesn\'t show my message {string}', async function (this: ThreadbearWorld, words: string) {
  const d = await this.app.dump();
  assert.equal(d.transcript.rows.some((r) => r.kind === 'user' && r.text.startsWith(words)), false, 'it was sent');
});

Then('the transcript shows a reply starting {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`a reply starting "${words}"`, (d) => d.transcript.rows.some((r) => r.kind === 'assistant' && r.text.startsWith(words)));
});
