import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { ThreadbearWorld } from '../support/world.ts';

// A tiny real picture, for the steps that need a file (the runner reads it).
const samplePicture = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'support', 'sample.png');

When('I type {string} in the composer', async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'type', text });
});

When('I press Return', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'return' });
});

When('I press Shift-Return', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'return', shift: true });
});

When('I send {string}', async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'send', text });
});

// `menu-key` is the Mac app's Stop item and the GPUI app's Stop binding.
When('I stop the turn', async function (this: ThreadbearWorld) {
  await this.until('the thread running', (d) => d.selection?.status === 'running');
  await this.app.do({ cmd: 'menu-key', key: '.', modifiers: ['cmd'] });
});

Then('the composer is empty', async function (this: ThreadbearWorld) {
  await this.until('an empty composer', (d) => d.composer.text === '');
});

Then('the composer says {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`the composer saying "${text}"`, (d) => d.composer.text === text);
});

When('I paste {string} into the composer', async function (this: ThreadbearWorld, words: string) {
  await this.app.do({ cmd: 'paste', text: words.replace(/\\n/g, '\n') });
});

Then('the composer says the lines {string}', async function (this: ThreadbearWorld, lines: string) {
  const wanted = lines.replace(/\\n/g, '\n');
  await this.until(`the composer saying ${JSON.stringify(wanted)}`, (d) => d.composer.text === wanted);
});

Then('the composer holds a new line after {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`"${text}" and a new line`, (d) => d.composer.text === `${text}\n`);
});

Then('the reply streams in and the thread goes back to idle', async function (this: ThreadbearWorld) {
  await this.until('the mock\'s reply', (d) => d.transcript.rows.some((r) => r.kind === 'assistant' && r.text.startsWith('You said:')), 30_000);
  await this.until('the thread idle again', (d) => d.selection?.status === 'idle', 30_000);
});

Then('the thread isn\'t running any more', async function (this: ThreadbearWorld) {
  await this.until('the thread stopped', (d) => d.selection?.status !== 'running', 30_000);
});

When('I paste the picture into the composer', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'paste-image', path: samplePicture });
});

// A point in the thread's transcript on both apps' windows.
When('I drop the picture on the transcript', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'drop', path: samplePicture, x: 650, y: 300 });
});

Then('the composer holds {int} picture(s)', async function (this: ThreadbearWorld, count: number) {
  await this.until(`${count} picture(s) in the composer`, (d) => d.composer.images === count);
});

Then('hovering chip {int} shows its picture', async function (this: ThreadbearWorld, number: number) {
  const result = await this.app.do({ cmd: 'composer-peek', number });
  assert.match(result, /ok/);
});
