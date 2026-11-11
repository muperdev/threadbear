import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

/** A list of titles from a dump's text: "ok [...]" or "ok shown true [...]". */
function listIn(result: string): string[] {
  const at = result.indexOf('[');
  assert.ok(at >= 0, `no list in: ${result}`);
  return JSON.parse(result.slice(at)) as string[];
}

// Menu titles can have commas ("Tomorrow, 9 am"), so lists use " | ".
const wanted = (list: string) => list.split(' | ').map((s) => s.trim());

Given('the {string} section is unfolded', async function (this: ThreadbearWorld, key: string) {
  await this.app.do({ cmd: 'expand', section: key });
  await this.until(`${key} open`, (d) => d.sidebar.sections.find((s) => s.name.toLowerCase() === key)?.collapsed === false);
});

// A row below the fold isn't drawn (it's a lazy list, as in the Swift app),
// so this brings a lower section into view.
Given('the {string} section is folded away', async function (this: ThreadbearWorld, key: string) {
  await this.app.do({ cmd: 'collapse', section: key });
  await this.until(`${key} folded`, (d) => d.sidebar.sections.find((s) => s.name.toLowerCase() === key)?.collapsed === true);
});

When('I hover the row {string}', async function (this: ThreadbearWorld, title: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  await this.app.do({ cmd: 'hover', id: t.id });
});

Then('the row {string} offers the buttons {string}', async function (this: ThreadbearWorld, title: string, buttons: string) {
  const expected = buttons.split(', ');
  await this.until(`"${title}" offering ${buttons}`, (d) => {
    const shown = this.thread(d, title)?.hoverButtons;
    // A tip can end with a keyboard shortcut ("Archive (⌘E)") on the Mac.
    return shown && shown.length === expected.length && expected.every((b, i) => shown[i].startsWith(b));
  });
});

When('I press the row button {string}', async function (this: ThreadbearWorld, tip: string) {
  await this.app.do({ cmd: 'click', tipPrefix: tip });
});

Then('no thread is open', async function (this: ThreadbearWorld) {
  await this.until('no selection', (d) => !d.selection);
});

When('I right-click the row {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  this.menuTitles = listIn(await this.app.do({ cmd: 'realMenu', tipPrefix: `${title}\n`, right: true }, 20));
});

Then('the row\'s menu offers {string}', function (this: ThreadbearWorld, list: string) {
  // A submenu ends " >" where the app can say so.
  const shown = (this.menuTitles ?? []).map((t) => t.replace(/ >$/, ''));
  for (const expected of wanted(list)) assert.ok(shown.includes(expected), `${expected} not in: ${shown.join(' | ')}`);
});

Given('a runner restart is waiting', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'restartPending', params: {} });
  // The notice has to reach the app; a menu built before it wouldn't know.
  await new Promise((resolve) => setTimeout(resolve, 1200));
});

When('I open the Hibernate menu for {string}', async function (this: ThreadbearWorld, title: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  await this.app.do({ cmd: 'menu-popover', show: 'hibernate', id: t.id });
});

Then('the menu lists {string}', async function (this: ThreadbearWorld, list: string) {
  const shown = listIn(await this.app.do({ cmd: 'menu-popover' }));
  for (const expected of wanted(list)) assert.ok(shown.includes(expected), `${expected} not in: ${shown.join(' | ')}`);
});

When('I choose {string} in the menu', async function (this: ThreadbearWorld, title: string) {
  await this.app.do({ cmd: 'menu-popover', click: title });
});

When('I ask to wake the thread {string} {string}', async function (this: ThreadbearWorld, title: string, said: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  await this.app.do({ cmd: 'snooze', id: t.id, said });
});
