import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

// A row's text, read from `dump`: the same fields on both apps (AppDump's
// `Thread`, `Threadbear::sidebar_dump` in the GPUI app).
Then('the row {string} has the tile {string}', async function (this: ThreadbearWorld, title: string, tile: string) {
  await this.until(`"${title}" with the tile ${tile}`, (d) => this.thread(d, title)?.tile === tile);
});

Then('the row {string} is in {string}', async function (this: ThreadbearWorld, title: string, place: string) {
  await this.until(`"${title}" in ${place}`, (d) => this.thread(d, title)?.place === place);
});

const indexIn = (d: Dump, name: string, title: string) => d.sidebar.sections.find((s) => s.name === name)?.threads.findIndex((t) => t.title === title) ?? -1;

Given('I note where {string} is in the {string} section', async function (this: ThreadbearWorld, title: string, name: string) {
  const d = await this.until(`"${title}" under ${name}`, (x) => indexIn(x, name, title) >= 0 && x);
  this.notedIndex = indexIn(d, name, title);
});

Then('{string} is still in the same place in the {string} section', async function (this: ThreadbearWorld, title: string, name: string) {
  await this.until(`"${title}" at ${this.notedIndex} in ${name}`, (d) => indexIn(d, name, title) === this.notedIndex);
});

Then('the row {string} has the origin mark {string}', async function (this: ThreadbearWorld, title: string, symbol: string) {
  await this.until(`"${title}" with the mark ${symbol}`, (d) => this.thread(d, title)?.origin === symbol);
});

Then('the row {string} has no origin mark', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" with no mark`, (d) => {
    const t = this.thread(d, title);
    return t && !t.origin;
  });
});

Then('the row {string} has a blue dot', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" with a dot`, (d) => this.thread(d, title)?.mark === 'dot');
});

Then('the row {string} has a tick', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" with a tick`, (d) => this.thread(d, title)?.mark === 'tick');
});

Then('the row {string} has no mark', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" with no mark`, (d) => {
    const t = this.thread(d, title);
    return t && !t.mark;
  });
});

Then('the row {string} has a bold title', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" bold`, (d) => this.thread(d, title)?.bold === true);
});

Then('the row {string} has a plain title', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" plain`, (d) => this.thread(d, title)?.bold === false);
});

Then('the row {string} shows a running timer', async function (this: ThreadbearWorld, title: string) {
  // Under Running it's just the time: "9s", "4m", "1h 2m".
  await this.until(`"${title}" with a timer`, (d) => /^\d+[smh]/.test(this.thread(d, title)?.trailing ?? ''));
});

Then('the row {string} says {string} on the right', async function (this: ThreadbearWorld, title: string, words: string) {
  await this.until(`"${title}" saying ${words}`, (d) => this.thread(d, title)?.trailing?.startsWith(words));
});

Then('the row {string} shows when it was last active', async function (this: ThreadbearWorld, title: string) {
  // "now", "5m", "3h", "Mon", "12 Oct".
  const pattern = /^(now|\d+[mh]|[A-Z][a-z]{2}|\d{1,2} [A-Z][a-z]{2}( \d{4})?)$/;
  await this.until(`"${title}" with a time`, (d) => pattern.test(this.thread(d, title)?.trailing ?? ''));
});

Then('the row {string} lists the sub-agent {string}', async function (this: ThreadbearWorld, title: string, sub: string) {
  await this.until(`"${title}" listing the sub-agent ${sub}`, (d) => this.thread(d, title)?.subagents?.some((s) => s.title === sub));
});

Then('the row {string} lists the sub-agent {string} indented by {int}', async function (this: ThreadbearWorld, title: string, sub: string, depth: number) {
  await this.until(`${sub} at depth ${depth}`, (d) => this.thread(d, title)?.subagents?.some((s) => s.title === sub && s.depth === depth));
});

Then('the sub-agent {string} of {string} says {string}', async function (this: ThreadbearWorld, sub: string, title: string, detail: string) {
  await this.until(`${sub} saying ${detail}`, (d) => this.thread(d, title)?.subagents?.some((s) => s.title === sub && s.detail === detail));
});

When('I star the thread {string}', async function (this: ThreadbearWorld, title: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  await this.app.do({ cmd: 'star', id: t.id });
});

async function hibernate(world: ThreadbearWorld, title: string, seconds: number) {
  const t = await world.until(`a thread called "${title}"`, (d) => world.thread(d, title));
  assert.ok(t);
  await world.app.do({ cmd: 'snooze', id: t.id, seconds });
}

When('I hibernate the thread {string} for {int} second(s)', async function (this: ThreadbearWorld, title: string, seconds: number) {
  await hibernate(this, title, seconds);
});

When('I hibernate the thread {string} for {int} hour(s)', async function (this: ThreadbearWorld, title: string, hours: number) {
  await hibernate(this, title, hours * 3600);
});
