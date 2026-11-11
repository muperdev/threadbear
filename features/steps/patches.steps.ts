import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';
import { pane } from '../support/pane.ts';

type WithActions = Dump & { actions?: { id: string; title: string; kind: string; running: boolean }[] };

When('I show the {string} section of the pane', async function (this: ThreadbearWorld, id: string) {
  await this.app.do({ cmd: 'panel', id });
  await this.until(`the pane to be showing ${id}`, (d) => pane(d)?.section === id);
});

const patch = (d: Dump, id: string) => pane(d)?.patches?.find((p) => p.id === id);

Then('the patch {string} comes from {string}', async function (this: ThreadbearWorld, id: string, source: string) {
  await this.until(`${id} from ${source}`, (d) => patch(d, id)?.source === source);
});

Then('the patch {string} gives {string}', async function (this: ThreadbearWorld, id: string, gives: string) {
  await this.until(`${id} giving "${gives}"`, (d) => patch(d, id)?.gives === gives);
});

Then('the patch {string} is waiting to be trusted, saying {string}', async function (this: ThreadbearWorld, id: string, words: string) {
  await this.until(`${id} waiting`, (d) => patch(d, id)?.waiting === words);
});

When('I trust the patch {string}', async function (this: ThreadbearWorld, id: string) {
  await this.app.do({ cmd: 'trust', id });
});

Then('the list panel has the heading {string} with the rows {string}', async function (this: ThreadbearWorld, heading: string, rows: string) {
  const expected = rows.split(' | ');
  await this.until(`"${heading}" with ${rows}`, (d) => {
    const found = pane(d)?.list?.find((s) => s.title === heading);
    const titles = found?.rows.map((r) => r.title) ?? [];
    return titles.length === expected.length && expected.every((t, i) => t === titles[i]);
  });
});

Then('the list row {string} says {string} and {string}', async function (this: ThreadbearWorld, title: string, trailing: string, detail: string) {
  await this.until(`"${title}" saying ${trailing} and ${detail}`, (d) => {
    const row = pane(d)?.list?.flatMap((s) => s.rows).find((r) => r.title === title);
    return row && row.trailing === trailing && row.detail === detail;
  });
});

Then('the image panel shows a picture', async function (this: ThreadbearWorld) {
  await this.until('a picture in the image panel', (d) => /^picture \d+x\d+/.test(pane(d)?.image ?? ''));
});

Then('the chip {string} is offered', async function (this: ThreadbearWorld, id: string) {
  await this.until(`the chip ${id}`, (d) => (d as WithActions).actions?.some((a) => a.id === id));
});

When('I run the chip {string}', async function (this: ThreadbearWorld, id: string) {
  await this.app.do({ cmd: 'action', id }, 20);
});

Then('the transcript shows a line about {string}', async function (this: ThreadbearWorld, words: string) {
  const row = await this.until(`a row mentioning "${words}"`, (d) => d.transcript.rows.find((r) => r.text.includes(words)));
  assert.ok(row);
});
