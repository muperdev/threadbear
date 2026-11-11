import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';
import { pane, previewLine } from '../support/pane.ts';

When('I click the highlight {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`a highlight called "${title}"`, (d) => pane(d)?.sections?.some((s) => s.rows.some((r) => r.title === title)));
  await this.app.do({ cmd: 'highlight', open: title });
});

When('I open {string} in the thread\'s folder in the pane', async function (this: ThreadbearWorld, relative: string) {
  const d = await this.until('the composer to know its folder', (x) => (x.composer as unknown as { cwd?: string }).cwd && x);
  const cwd = (d.composer as unknown as { cwd: string }).cwd.replace(/\/$/, '');
  await this.app.do({ cmd: 'preview', path: `${cwd}/${relative}` });
});

Then('the pane is showing the file {string}', async function (this: ThreadbearWorld, relative: string) {
  await this.until(`the preview line to name ${relative}`, (d) => previewLine(d)?.startsWith('file ') && previewLine(d)?.endsWith(relative));
});

When('I press Back in the pane', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'preview', back: true });
});

Then('the pane shows Highlights', async function (this: ThreadbearWorld) {
  await this.until('Highlights to be showing, with no preview over it', (d) => !previewLine(d) && pane(d)?.section === 'threadbear.highlights');
});

Then('the pane shows a {string} page with the words {string}', async function (this: ThreadbearWorld, kind: string, words: string) {
  await this.until(`a ${kind} preview with "${words}"`, (d) => pane(d)?.previewKind === kind && pane(d)?.previewText?.includes(words));
});

Then('the pane\'s page has the words {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`the page to have "${words}"`, (d) => pane(d)?.previewText?.includes(words));
});

Then('the pane shows a picture', async function (this: ThreadbearWorld) {
  await this.until('a picture in the pane', (d) => pane(d)?.previewKind === 'picture' && /\d+x\d+/.test(pane(d)?.previewText ?? ''));
});

When('I copy the picture in the pane', async function (this: ThreadbearWorld) {
  this.lastAnswer = await this.app.do({ cmd: 'preview', copyImage: true });
});

Then('it was copied as a picture', function (this: ThreadbearWorld) {
  assert.match(this.lastAnswer ?? '', /copied (png|public\.png)/);
});
