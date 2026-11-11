import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';
import { pane, previewLine } from '../support/pane.ts';

// `folder`'s answer ends with the names; its summary reads "(list, 5 rows,
// sort name up, rows drawn 5): docs, logs, ...".
const summary = (d: Parameters<typeof pane>[0]) => pane(d)?.folder ?? '';
const names = (d: Parameters<typeof pane>[0]) => summary(d).split('): ').slice(1).join('): ').split(', ').filter(Boolean);

async function showFiles(world: ThreadbearWorld) {
  await world.app.do({ cmd: 'panel', id: 'threadbear.files' });
  await world.until('the folder browser to be showing', (d) => pane(d)?.section === 'threadbear.files' && names(d).length > 0);
}

Given('the thread\'s folder is showing in the pane for {string}', async function (this: ThreadbearWorld, title: string) {
  await this.open(title);
  await showFiles(this);
});

Then('the folder lists {string} first', async function (this: ThreadbearWorld, list: string) {
  const wanted = list.split(', ');
  await this.until(`the folder listing ${list}`, (d) => wanted.every((name, i) => names(d)[i] === name));
});

Then('the folder says it\'s sorted by {string}', async function (this: ThreadbearWorld, how: string) {
  await this.until(`the folder sorted by ${how}`, (d) => summary(d).includes(`sort ${how}`));
});

When('I sort the folder by {string} descending', async function (this: ThreadbearWorld, key: string) {
  await this.app.do({ cmd: 'folder', sort: key, ascending: false });
});

When('I expand the folder row {string}', async function (this: ThreadbearWorld, name: string) {
  await this.app.do({ cmd: 'folder', expand: name });
});

Then('the folder shows {int} rows in all', async function (this: ThreadbearWorld, rows: number) {
  await this.until(`${rows} rows drawn`, (d) => summary(d).includes(`rows drawn ${rows}`));
});

When('I double-click the folder row {string}', async function (this: ThreadbearWorld, name: string) {
  await this.until(`a row called ${name}`, (d) => names(d).includes(name));
  await this.app.do({ cmd: 'folder', open: name });
});

Then('the pane is browsing the folder {string}', async function (this: ThreadbearWorld, name: string) {
  await this.until(`the pane browsing ${name}`, (d) => previewLine(d)?.startsWith('folder ') && previewLine(d)?.includes(`/${name}`));
});

Then('the pane shows the thread\'s folder again', async function (this: ThreadbearWorld) {
  await this.until('the thread\'s folder to be showing again', (d) => names(d)[0] === 'docs' && !previewLine(d)?.includes('/logs'));
});

Then('the path bar ends with {string}', async function (this: ThreadbearWorld, words: string) {
  const answer = await this.app.do({ cmd: 'folder', pathBar: true });
  assert.ok(answer.endsWith(words), `path bar reads: ${answer}`);
});

When('I right-click the folder row {string}', async function (this: ThreadbearWorld, name: string) {
  const answer = await this.app.do({ cmd: 'folder', menu: name });
  this.menuTitles = answer.replace(/^(\w+ )?ok /, '').split(' | ');
});

Then('the folder menu offers {string}', function (this: ThreadbearWorld, list: string) {
  const items = this.menuTitles ?? [];
  for (const item of list.split(' | ')) {
    // A rule the Mac app draws, and Windows' or Linux's word for Finder.
    const found = items.some((i) => i === item || (item === 'Reveal in Finder' && /^(Reveal in Finder|Show in folder)$/.test(i)) || (item === 'Open in Finder' && /^(Open in Finder|Open in its app)$/.test(i)));
    assert.ok(found, `no "${item}" in: ${items.join(' | ')}`);
  }
});
