import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';
import type { Row } from '../support/driver.ts';

Then('my message {string} comes before the reply {string}', async function (this: ThreadbearWorld, mine: string, reply: string) {
  const d = await this.app.dump();
  const a = d.transcript.rows.findIndex((r) => r.kind === 'user' && r.text.startsWith(mine));
  const b = d.transcript.rows.findIndex((r) => r.kind === 'assistant' && r.text.startsWith(reply));
  assert.ok(a >= 0 && b >= 0 && a < b, `rows: ${d.transcript.rows.map((r) => r.kind).join(' ')}`);
});

Then('the agent\'s work is folded into a line saying {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`a folded line "${text}"`, (d) => d.transcript.rows.some((r) => r.kind === 'group' && r.text === text && !r.expanded));
});

When('I open the line saying {string}', async function (this: ThreadbearWorld, text: string) {
  const row = await this.until(`a line "${text}"`, (d) => d.transcript.rows.find((r) => r.kind === 'group' && r.text === text));
  await this.app.do({ cmd: 'expand', row: row.id });
});

Then('the transcript shows a step starting {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`a step "${words}"`, (d) => d.transcript.rows.some((r) => r.inGroup && r.text.startsWith(words)));
});

Then('a turn ends with a line saying {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`a footer "${text}"`, (d) => d.transcript.rows.some((r) => r.kind === 'footer' && r.text === text));
});

Then('the transcript has a card saying {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`a changes card "${text}"`, (d) => d.transcript.rows.some((r) => r.kind === 'changes' && r.text === text));
});

Then('the transcript shows an error', async function (this: ThreadbearWorld) {
  await this.until('an error row', (d) => d.transcript.rows.some((r) => r.kind === 'error' && r.text.length > 0));
});

// Rows that open: found by what they say, whatever folded group they're in.
const openable = (r: Row) => ['tool', 'diff', 'reasoning', 'group'].includes(r.kind);
const heights = new WeakMap<object, Map<string, number>>();

/** The row's height, drawn: GPUI only knows a row's height once it's laid out,
 * so the row is scrolled to first. */
async function drawnHeight(world: ThreadbearWorld, id: string): Promise<number> {
  await world.app.do({ cmd: 'scroll', row: id, offset: 0 });
  return world.until(`row ${id} drawn`, (d) => d.transcript.rows.find((r) => r.id === id && r.onScreen !== false)?.height);
}

function remember(world: ThreadbearWorld, key: string, value: number) {
  const seen = heights.get(world) ?? new Map<string, number>();
  seen.set(key, value);
  heights.set(world, seen);
}

function recalled(world: ThreadbearWorld, key: string): number {
  return heights.get(world)?.get(key) ?? 0;
}

When('I open the thinking that starts {string}', async function (this: ThreadbearWorld, words: string) {
  const row = await this.until(`thinking "${words}"`, (d) => d.transcript.rows.find((r) => r.kind === 'reasoning' && r.text.startsWith(words)));
  remember(this, words, await drawnHeight(this, row.id));
  await this.app.do({ cmd: 'expand', row: row.id });
});

Then('the thinking that starts {string} is open', async function (this: ThreadbearWorld, words: string) {
  await this.until(`thinking "${words}" open and taller`, (d) => d.transcript.rows.some((r) => r.kind === 'reasoning' && r.text.startsWith(words) && r.expanded && (r.height ?? 0) > recalled(this, words)));
});

When('I open the step called {string}', async function (this: ThreadbearWorld, title: string) {
  const row = await this.until(`a step "${title}"`, (d) => d.transcript.rows.find((r) => openable(r) && r.text === title));
  remember(this, title, await drawnHeight(this, row.id));
  await this.app.do({ cmd: 'expand', row: row.id });
});

When('I close the step called {string}', async function (this: ThreadbearWorld, title: string) {
  const row = await this.until(`a step "${title}"`, (d) => d.transcript.rows.find((r) => openable(r) && r.text === title));
  await this.app.do({ cmd: 'collapse', row: row.id });
});

Then('the step called {string} is open and taller than one line', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" open and tall`, (d) => d.transcript.rows.some((r) => openable(r) && r.text === title && r.expanded && (r.height ?? 0) > 40));
});

Then('the step called {string} is one line again', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" one line`, (d) => d.transcript.rows.some((r) => openable(r) && r.text === title && !r.expanded && (r.height ?? 99) < 40));
});

When('I show every line of the step called {string}', async function (this: ThreadbearWorld, title: string) {
  const row = await this.until(`a step "${title}"`, (d) => d.transcript.rows.find((r) => openable(r) && r.text === title));
  remember(this, `${title}|open`, await drawnHeight(this, row.id));
  await this.app.do({ cmd: 'expand', row: row.id, all: true });
});

Then('the step called {string} got taller', async function (this: ThreadbearWorld, title: string) {
  const before = recalled(this, `${title}|open`);
  await this.until(`"${title}" taller than ${before}`, (d) => d.transcript.rows.some((r) => openable(r) && r.text === title && (r.height ?? 0) > before));
});

When('I open the first file in the card saying {string}', async function (this: ThreadbearWorld, title: string) {
  const row = await this.until(`a changes card "${title}"`, (d) => d.transcript.rows.find((r) => r.kind === 'changes' && r.text === title));
  remember(this, title, await drawnHeight(this, row.id));
  await this.app.do({ cmd: 'expand', row: row.id, file: 0 });
});

Then('the card saying {string} got taller', async function (this: ThreadbearWorld, title: string) {
  const before = recalled(this, title);
  await this.until(`"${title}" taller than ${before}`, (d) => d.transcript.rows.some((r) => r.kind === 'changes' && r.text === title && (r.height ?? 0) > before));
});

Then('the transcript shows {int} rows of pictures', async function (this: ThreadbearWorld, count: number) {
  await this.until(`${count} picture rows`, (d) => d.transcript.rows.filter((r) => r.kind === 'images').length === count);
});

// The replies there before the stream, so the one it makes is told apart from
// them: picking "the newest reply" raced the stream's first words, and on a
// busy machine found an older reply, which never grows (10 Nov).
const repliesBefore = new WeakMap<ThreadbearWorld, Set<string>>();

When('a reply starts streaming in', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  repliesBefore.set(this, new Set(d.transcript.rows.filter((r) => r.kind === 'assistant').map((r) => r.id)));
  await this.app.do({ cmd: 'mock', method: 'stream', params: { threadId: d.selection?.id, hz: 40, seconds: 3 } });
});

Then('the working line shows', async function (this: ThreadbearWorld) {
  await this.until('a working line', (d) => d.transcript.rows.some((r) => r.kind === 'working'));
});

Then('the streaming reply grows', async function (this: ThreadbearWorld) {
  const before = repliesBefore.get(this) ?? new Set<string>();
  const streamed = (rows: Row[]) => rows.find((r) => r.kind === 'assistant' && r.text.length > 0 && !before.has(r.id));
  const first = await this.until('a streaming reply', (d) => streamed(d.transcript.rows));
  // Longer, or taller as it wraps onto new lines: the dump's text is an
  // excerpt (the GPUI app's stops at 100 characters), which a fast stream
  // passes before a slow machine's first look, and then it never grows.
  await this.until('the reply getting longer', (d) =>
    d.transcript.rows.some((r) => r.id === first.id && (r.text.length > first.text.length || (r.height ?? 0) > (first.height ?? 0))),
  );
});

When('I choose {string} from the menu of the reply starting {string}', async function (this: ThreadbearWorld, title: string, words: string) {
  const row = await this.until(`a reply "${words}"`, (d) => d.transcript.rows.find((r) => r.kind === 'assistant' && r.text.startsWith(words)));
  await this.app.do({ cmd: 'menu', row: row.id, click: title });
});

Then('the clipboard holds {string}', async function (this: ThreadbearWorld, words: string) {
  const deadline = Date.now() + 5000;
  let last = '';
  while (Date.now() < deadline) {
    last = await this.app.do({ cmd: 'clipboard' });
    if (last.includes(words)) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`the clipboard says ${last}`);
});
