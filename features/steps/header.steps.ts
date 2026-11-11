// The thread's bar (title, back and forward, share, the ... menu), its banners
// and the notes that float over the panel.
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

Then("the panel bar's title says {string}", async function (this: ThreadbearWorld, title: string) {
  await this.until(`the header saying "${title}"`, (d) => d.header?.title === title);
});

When('I start renaming the open thread', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'rename' });
});

Then("the panel bar's title is a field", async function (this: ThreadbearWorld) {
  await this.until('the title being edited', (d) => d.header?.renaming === true);
});

When('I rename the open thread to {string}', async function (this: ThreadbearWorld, title: string) {
  await this.app.do({ cmd: 'rename', title });
});

Then("the back button's tip says {string}", async function (this: ThreadbearWorld, tip: string) {
  await this.until(`the back button's tip "${tip}"`, (d) => d.header?.backTip === tip);
});

Then("the forward button's tip says {string}", async function (this: ThreadbearWorld, tip: string) {
  await this.until(`the forward button's tip "${tip}"`, (d) => d.header?.forwardTip === tip);
});

When('I click the back button', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  assert.ok(d.header?.backTip, 'no back button tip in the dump');
  await this.app.do({ cmd: 'click', tip: d.header.backTip });
});

Then('the open thread is {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" open`, (d) => d.selection?.title === title);
});

When('I choose Copy thread id from the ... menu', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'copy-thread-id' });
});

Then("the clipboard holds the open thread's id", async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  const id = d.selection?.id;
  const out = await this.app.do({ cmd: 'clipboard' });
  assert.ok(id && out.includes(id), `clipboard: ${out}, expected ${id}`);
});

When('I make a share link for the open thread', async function (this: ThreadbearWorld) {
  this.shareResult = await this.app.do({ cmd: 'share', access: 'link' });
});

Then('the share popover says the thread is shared with a link', async function (this: ThreadbearWorld) {
  await this.until('the share button lit', (d) => d.header?.shared === true);
  const out = await this.app.do({ cmd: 'share' });
  assert.match(out, /access link/, out);
  assert.match(out, /link http/, out);
});

When('an error {string} happens', async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'error', text });
});

Then('the error bar says {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`the error bar saying "${text}"`, (d) => d.error === text);
});

When('I dismiss the error bar', async function (this: ThreadbearWorld) {
  // The automation's `error` with no text is the Swift app's Dismiss; a real
  // click on a SwiftUI button in the error bar doesn't return in a scripted
  // run (the view it pressed goes away).
  await this.app.do({ cmd: 'error' });
});

Then("there's no error bar", async function (this: ThreadbearWorld) {
  await this.until('the error bar gone', (d) => !d.error);
});

Then('the banner says the runner is restarting or reconnecting', async function (this: ThreadbearWorld) {
  await this.until('the restart banner', (d) => /restarting|Reconnecting/.test(d.banner?.text ?? ''));
});

Then("there's no banner once the runner is back", async function (this: ThreadbearWorld) {
  await this.until('the banner gone', (d) => d.connection === 'connected' && !d.banner, 30_000);
});

Then('the Archived note names {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`the Archived note naming "${title}"`, (d) => {
    const id = d.archivedNote;
    return id && (this.thread(d, title)?.id === id || d.selection?.id === id);
  });
});

When('I press Undo on the Archived note', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'undo-archive' });
});

Then("there's no Archived note", async function (this: ThreadbearWorld) {
  await this.until('the note gone', (d) => !d.archivedNote);
});

When('I click the Archived note', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'open-archived' });
});

When('I open the last thread in the Inbox', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  const inbox = d.sidebar.sections.find((s) => s.name === 'Inbox')?.threads ?? [];
  assert.ok(inbox.length > 0, 'nothing in the Inbox');
  await this.open(inbox[inbox.length - 1].title);
});

Then("a note says I've reached the end of the inbox", async function (this: ThreadbearWorld) {
  await this.until('the end-of-inbox note', (d) => d.inboxEndNote === 'end');
});

Then('the limit line starts {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`a limit line starting "${words}"`, (d) => d.limit?.text?.startsWith(words));
});

Then("the limit line's button says {string}", async function (this: ThreadbearWorld, title: string) {
  await this.until(`the limit line's button "${title}"`, (d) => d.limit?.button === title);
});

Then('the model chip is marked {string}', async function (this: ThreadbearWorld, badge: string) {
  await this.until(`the model chip marked "${badge}"`, (d) => d.composer.chipBadge === badge);
});
