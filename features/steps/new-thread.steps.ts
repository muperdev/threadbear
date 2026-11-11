import { Given, Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';

// Lists in the scenarios use " | ", as menu titles and lines can have commas.
const split = (list: string) => list.split(' | ').map((s) => s.trim());

When('I start a new thread', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'new' });
  await this.until('the new-thread page', (d) => !d.selection && d.composer.draft === 'new');
});

When('I start a new thread in the project {string}', async function (this: ThreadbearWorld, id: string) {
  await this.app.do({ cmd: 'new', project: id });
  await this.until('the new-thread page', (d) => !d.selection && d.composer.draft === 'new');
});

When('I press Command-N', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'menu-key', key: 'n', modifiers: ['cmd'] });
});

Then('the new-thread page asks {string}', async function (this: ThreadbearWorld, heading: string) {
  await this.until(`the heading "${heading}"`, (d) => d.newThread?.heading === heading);
});

Then('the note under the box says {string}', async function (this: ThreadbearWorld, note: string) {
  await this.until(`the note "${note}"`, (d) => d.newThread?.note === note);
});

Then('the new-thread page names the project {string}', async function (this: ThreadbearWorld, project: string) {
  await this.until(`the project chip "${project}"`, (d) => d.newThread?.project === project);
});

Then('the room cards say {string}', async function (this: ThreadbearWorld, list: string) {
  const wanted = split(list);
  await this.until(`the cards ${list}`, (d) => {
    const rooms = d.newThread?.rooms ?? [];
    return rooms.length === wanted.length && rooms.every((r, i) => `${r.label}: ${r.line}` === wanted[i]);
  });
});

Then('the room card {string} is ringed', async function (this: ThreadbearWorld, label: string) {
  await this.until(`the ${label} card ringed`, (d) => d.newThread?.rooms?.find((r) => r.label === label)?.selected === true);
});

When('I pin the new thread to the login {string}', async function (this: ThreadbearWorld, account: string) {
  await this.app.do({ cmd: 'options', account });
});

When('I choose the folder {string} for the new thread', async function (this: ThreadbearWorld, cwd: string) {
  await this.app.do({ cmd: 'options', cwd });
});

Given('the runner takes its time to start threads', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'slow', params: { ms: 2500 } });
});

Then('the box is starting a thread with {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`the box greyed with "${text}"`, (d) => d.composer.creating === true && d.composer.text === text, 10_000);
});

Then('a thread called {string} opens', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" open`, (d) => d.selection?.title === title && d.transcript.state === 'loaded', 20_000);
});

Then('the thread\'s folder is {string}', async function (this: ThreadbearWorld, cwd: string) {
  await this.until(`the folder ${cwd}`, (d) => d.composer.cwd === cwd);
});

Then('the row {string} says it\'s in {string}', async function (this: ThreadbearWorld, title: string, place: string) {
  await this.until(`"${title}" in ${place}`, (d) => this.thread(d, title)?.place === place);
});
