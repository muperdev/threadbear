// The activity bar and the right rail.
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

const shortcut: Record<string, string> = { Threads: '⌘1', Scheduled: '⌘2', Projects: '⌘3', Librarian: '⌘4' };

When('I click {string} in the activity bar', async function (this: ThreadbearWorld, title: string) {
  // The button's tooltip names it with its key, as in the Swift app.
  await this.app.do({ cmd: 'click', tip: `${title} (${shortcut[title] ?? ''})` });
});

When('I click {string} in the right rail', async function (this: ThreadbearWorld, title: string) {
  await this.app.do({ cmd: 'click', tip: title });
});

Then('the sidebar shows the {string} view', async function (this: ThreadbearWorld, mode: string) {
  await this.until(`the sidebar's ${mode} view`, (d) => d.sidebar.mode === mode);
});

Then("the activity bar's {string} button is lit", async function (this: ThreadbearWorld, title: string) {
  await this.until(`${title} lit in the activity bar`, (d) => d.rails?.activity.find((b) => b.title === title)?.active === true);
});

Then("the activity bar's {string} button shows how many threads are in the Inbox", async function (this: ThreadbearWorld, title: string) {
  const d = await this.app.dump();
  const inbox = d.sidebar.sections.find((s) => s.name === 'Inbox')?.threads.length ?? 0;
  const button = d.rails?.activity.find((b) => b.title === title);
  assert.equal(button?.badge, inbox, `${title} says ${button?.badge}, the Inbox has ${inbox}`);
});

Then('the right rail has a {string} button', async function (this: ThreadbearWorld, title: string) {
  await this.until(`a ${title} button in the right rail`, (d) => d.rails?.right.some((b) => b.title === title));
});

Then('the {string} button in the right rail is lit', async function (this: ThreadbearWorld, title: string) {
  await this.until(`${title} lit in the right rail`, (d) => d.rails?.right.find((b) => b.title === title)?.active === true);
});

Then('the {string} button in the right rail has a dot', async function (this: ThreadbearWorld, title: string) {
  await this.until(`a dot on ${title}`, (d) => d.rails?.right.find((b) => b.title === title)?.dot === true);
});

Then('the {string} button in the right rail has no dot', async function (this: ThreadbearWorld, title: string) {
  await this.until(`no dot on ${title}`, (d) => d.rails?.right.find((b) => b.title === title)?.dot === false);
});
