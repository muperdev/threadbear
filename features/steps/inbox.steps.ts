import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

const section = (d: Dump, name: string) => d.sidebar.sections.find((s) => s.name === name);
const listed = (d: Dump, title: string) => d.sidebar.sections.some((s) => s.threads.some((t) => t.title === title));

Then('the {string} section lists {string}', async function (this: ThreadbearWorld, name: string, title: string) {
  await this.until(`"${title}" under ${name}`, (d) => section(d, name)?.threads.some((t) => t.title === title));
});

Then('the {string} section doesn\'t list {string}', async function (this: ThreadbearWorld, name: string, title: string) {
  await this.until(`no "${title}" under ${name}`, (d) => !section(d, name)?.threads.some((t) => t.title === title));
});

Then('the {string} section is folded', async function (this: ThreadbearWorld, name: string) {
  await this.until(`${name} folded`, (d) => section(d, name)?.collapsed === true);
});

Then('{string} comes before {string} in the {string} section', async function (this: ThreadbearWorld, first: string, second: string, name: string) {
  const d = await this.app.dump();
  const titles = section(d, name)?.threads.map((t) => t.title) ?? [];
  const a = titles.indexOf(first);
  const b = titles.indexOf(second);
  assert.ok(a >= 0 && b >= 0 && a < b, `in ${name}: ${titles.join(' | ')}`);
});

Then('{string} needs me', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" needing me`, (d) => this.thread(d, title)?.needsYou);
});

Then('{string} doesn\'t need me', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" not needing me`, (d) => this.thread(d, title)?.needsYou === false);
});

Given('{string} is unread', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" unread`, (d) => this.thread(d, title)?.unread);
});

Then('{string} is no longer unread', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" read`, (d) => this.thread(d, title)?.unread === false);
});

// A scripted Swift app is never in front, and only the front window marks a
// thread as read.
Given('the window is in front', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'focus' });
});

Given('I search the threads for {string}', async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'search', text });
});

When('I clear the search', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'search', text: '' });
});

Then('the sidebar lists {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" in the sidebar`, (d) => listed(d, title));
});

Then('the sidebar doesn\'t list {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`no "${title}" in the sidebar`, (d) => !listed(d, title));
});

When('I archive the open thread', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'archive' });
});

When('I hibernate the open thread for an hour', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'snooze', seconds: 3600 });
});
