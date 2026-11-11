import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';
import { pane, row, section, sections } from '../support/pane.ts';

// The menu's items, the app's word for Reveal in Finder aside: GPUI's
// is the system's own on Windows and Linux.
const wanted = (list: string) => list.split(' | ').map((s) => s.trim());
const sameItem = (a: string, b: string) => a === b || (b === 'Reveal in Finder' && /^(Reveal in Finder|Show in folder)$/.test(a));

Then('Highlights has the headings {string}', async function (this: ThreadbearWorld, list: string) {
  const expected = wanted(list);
  await this.until(`headings ${list}`, (d) => {
    const titles = sections(d).map((s) => s.title);
    return expected.length === titles.length && expected.every((t, i) => titles[i] === t);
  });
});

Then('Highlights has no heading {string}', async function (this: ThreadbearWorld, title: string) {
  // Waits for the section list to have come first, so an empty dump can't pass.
  const d = await this.until('Highlights to be drawn', (x) => (sections(x).length > 0 || pane(x)?.empty) && x);
  assert.equal(section(d, title), undefined, `there's a "${title}" heading`);
});

Then('Highlights says {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`Highlights saying "${words}"`, (d) => pane(d)?.empty === words);
});

When('I fold the {string} heading in Highlights', async function (this: ThreadbearWorld, key: string) {
  await this.until('Highlights to be drawn', (d) => sections(d).length > 0);
  await this.app.do({ cmd: 'collapse', highlightSection: key });
});

Then('the {string} heading in Highlights is folded and counts {int}', async function (this: ThreadbearWorld, title: string, count: number) {
  await this.until(`"${title}" folded and counting ${count}`, (d) => {
    const s = section(d, title);
    return s && s.folded === true && s.count === count && s.rows.length === 0;
  });
});

Then('Highlights opens with the point {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`the brief starting "${words}"`, (d) => pane(d)?.brief?.[0]?.startsWith(words));
});

Then('the {string} highlight {string} has the note {string}', async function (this: ThreadbearWorld, heading: string, title: string, note: string) {
  await this.until(`"${title}" with its note`, (d) => row(d, heading, title)?.note === note);
});

Then('the {string} highlight {string} sits in {string}', async function (this: ThreadbearWorld, heading: string, title: string, detail: string) {
  await this.until(`"${title}" in "${detail}"`, (d) => row(d, heading, title)?.detail === detail);
});

Then('the {string} highlight {string} says {string}', async function (this: ThreadbearWorld, heading: string, title: string, word: string) {
  await this.until(`"${title}" saying "${word}"`, (d) => row(d, heading, title)?.trailing === word);
});

Then('the {string} highlight {string} has the sha {string}', async function (this: ThreadbearWorld, heading: string, title: string, sha: string) {
  await this.until(`"${title}" with the sha ${sha}`, (d) => row(d, heading, title)?.lead === sha);
});

Then('the {string} highlight {string} is listed', async function (this: ThreadbearWorld, heading: string, title: string) {
  await this.until(`"${title}" under "${heading}"`, (d) => row(d, heading, title));
});

Then('the {string} highlight {string} comes first', async function (this: ThreadbearWorld, heading: string, title: string) {
  await this.until(`"${title}" first under "${heading}"`, (d) => section(d, heading)?.rows[0]?.title === title);
});

Then('the {string} highlight {string} is marked {string}', async function (this: ThreadbearWorld, heading: string, title: string, symbol: string) {
  await this.until(`"${title}" marked ${symbol}`, (d) => row(d, heading, title)?.symbol === symbol);
});

Then('Highlights doesn\'t list {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`no "${title}"`, (d) => sections(d).length > 0 && !sections(d).some((s) => s.rows.some((r) => r.title === title)));
});

When('I open the edited files in Highlights', async function (this: ThreadbearWorld) {
  await this.until('Changes to be drawn', (d) => section(d, 'Changes'));
  await this.app.do({ cmd: 'highlight', edited: true });
});

Then('the ask {string} is not ticked', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" unticked`, (d) => row(d, 'Needs you', title)?.ticked === false);
});

Then('the ask {string} is ticked', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" ticked`, (d) => row(d, 'Needs you', title)?.ticked === true);
});

Then('the {string} heading in Highlights counts {int} when folded', async function (this: ThreadbearWorld, heading: string, count: number) {
  await this.until(`"${heading}" counting ${count}`, (d) => section(d, heading)?.count === count);
});

When('I tick the ask {string}', async function (this: ThreadbearWorld, title: string) {
  const index = await this.until(`the ask "${title}"`, (d) => {
    const i = section(d, 'Needs you')?.rows.findIndex((r) => r.title === title) ?? -1;
    return i >= 0 ? i + 1 : false;
  });
  await this.app.do({ cmd: 'highlight', tick: index - 1 });
});

When('I ask why the thread is in the project {string}', async function (this: ThreadbearWorld, title: string) {
  const key = await this.until(`the project "${title}"`, (d) => section(d, title)?.key);
  this.lastAnswer = await this.app.do({ cmd: 'project-why', id: key.replace('project:', '') });
});

Then('it says {string}', function (this: ThreadbearWorld, words: string) {
  assert.ok(this.lastAnswer?.includes(words), `not in: ${this.lastAnswer}`);
});

Then('it offers {string}', function (this: ThreadbearWorld, list: string) {
  for (const item of list.split(', ')) assert.ok(this.lastAnswer?.includes(item), `no ${item} in: ${this.lastAnswer}`);
});

When('I right-click the highlight {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`a highlight called "${title}"`, (d) => sections(d).some((s) => s.rows.some((r) => r.title === title || r.lead === title)));
  // A commit is named by its sha in the feature, the app looks it up by title.
  const d = await this.app.dump();
  const named = sections(d).flatMap((s) => s.rows).find((r) => r.title === title || r.lead === title)!.title;
  const out = await this.app.do({ cmd: 'highlight', menu: named });
  this.menuTitles = out.replace(/^(\w+ )?ok /, '').split(' | ');
});

Then('its menu offers {string}', function (this: ThreadbearWorld, list: string) {
  const items = this.menuTitles ?? [];
  for (const item of wanted(list)) assert.ok(items.some((i) => sameItem(i, item)), `no "${item}" in: ${items.join(' | ')}`);
});

When('I choose {string} from the menu of the highlight {string}', async function (this: ThreadbearWorld, item: string, title: string) {
  await this.until(`a highlight called "${title}"`, (d) => sections(d).some((s) => s.rows.some((r) => r.title === title)));
  await this.app.do({ cmd: 'highlight', choose: item, row: title });
});
