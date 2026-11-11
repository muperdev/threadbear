import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

// The palette's commands come with the first `threads.commands` answer, so
// everything here waits for what it reads.
const split = (list: string) => list.split(' | ').map((s) => s.trim());

When('I type {string} in the composer, replacing what\'s there', async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'type', text, replace: true });
});

When('I press the down arrow', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'arrow', dir: 'down' });
});

When('I press the up arrow', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'arrow', dir: 'up' });
});

When('I press Tab', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'tab' });
});

When('I press Escape', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'escape' });
});

// The tooltip each category's button has (SlashPaletteView.categoryTip).
const categoryTips: Record<string, string> = {
  All: 'Every command this thread can run',
  Threadbear: "Threadbear's own, which change this thread's settings",
  'Claude Code': "Claude Code's own commands",
  Skills: 'Your skills and this folder\'s',
  Recent: 'The ones you picked lately',
};

When('I click the palette\'s {string} category', async function (this: ThreadbearWorld, category: string) {
  const tip = categoryTips[category];
  assert.ok(tip, `no category called ${category}`);
  await this.until('the palette open', (d) => d.palette);
  await this.app.do({ cmd: 'click', tip });
});

Then('the palette is open on {string}', async function (this: ThreadbearWorld, category: string) {
  await this.until(`the palette open on ${category}`, (d) => d.palette?.category === category);
});

Then('the palette is closed', async function (this: ThreadbearWorld) {
  await this.until('the palette closed', (d) => !d.palette);
});

Then('the palette\'s categories say {string}', async function (this: ThreadbearWorld, list: string) {
  const wanted = split(list);
  await this.until(`the categories ${list}`, (d) => {
    const shown = d.palette?.categories.map((c) => `${c.title} ${c.count}`) ?? [];
    return shown.length === wanted.length && shown.every((s, i) => s === wanted[i]);
  });
});

Then('the palette lists {string} first', async function (this: ThreadbearWorld, command: string) {
  await this.until(`the palette led by ${command}`, (d) => d.palette?.commands[0] === command);
});

Then('the palette has the command {string}', async function (this: ThreadbearWorld, command: string) {
  await this.until(`${command} in the palette`, (d) => d.palette?.commands.includes(command));
});

Then('the palette\'s commands are {string}', async function (this: ThreadbearWorld, list: string) {
  const wanted = split(list);
  await this.until(`the commands ${list}`, (d) => {
    const shown = d.palette?.commands ?? [];
    return shown.length === wanted.length && shown.every((s, i) => s === wanted[i]);
  });
});

Then('the palette\'s pick is {string}', async function (this: ThreadbearWorld, command: string) {
  await this.until(`${command} picked`, (d) => d.palette?.picked === command);
});

Then('the palette\'s details say the usage {string}', async function (this: ThreadbearWorld, usage: string) {
  await this.until(`the usage ${usage}`, (d) => d.palette?.detail?.usage === usage);
});

Then('the palette\'s details say they\'re from {string}', async function (this: ThreadbearWorld, origin: string) {
  await this.until(`from ${origin}`, (d) => d.palette?.detail?.origin === origin);
});
