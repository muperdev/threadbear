import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

// The mock's threads on "bruce" (@remote-threads turns them on, world.ts).

Then('the row {string} is on the runner {string}', async function (this: ThreadbearWorld, title: string, runner: string) {
  await this.until(`"${title}" on ${runner}`, (d) => this.thread(d, title)?.runner === runner);
});

Then('the row {string} has the machine tile', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" with the machine tile`, (d) => this.thread(d, title)?.tile === 'runner');
});

Then('the row {string} isn\'t on another runner', async function (this: ThreadbearWorld, title: string) {
  const d = await this.app.dump();
  const t = this.thread(d, title);
  assert.ok(t, `no row ${title}`);
  assert.ok(!t.runner, `"${title}" says it's on ${t.runner}`);
});

Then('the composer is on the runner {string}', async function (this: ThreadbearWorld, runner: string) {
  await this.until(`the composer on ${runner}`, (d) => d.composer.runner === runner);
});

When('I make {string} the default runner', async function (this: ThreadbearWorld, runner: string) {
  await this.app.do({ cmd: 'composer-menu', menu: 'runner', setDefault: runner });
});

Given('a move would leave nothing behind', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'moveLeftBehind', params: { lines: [] } });
});

// `move` asks like Move to ▸ that runner does, once the runner list has come.
When('I ask to move the thread {string} to {string}', async function (this: ThreadbearWorld, title: string, runner: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  let lastError: unknown;
  for (let tries = 0; tries < 50; tries += 1) {
    try {
      await this.app.do({ cmd: 'move', id: t.id, runner });
      return;
    } catch (e) {
      lastError = e;
      await sleep(150);
    }
  }
  throw lastError;
});

Then('the move sheet says {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`the sheet "${title}"`, (d) => d.moveSheet?.title === title);
});

Then('the move sheet warns {string}', async function (this: ThreadbearWorld, line: string) {
  await this.until(`the warning "${line}"`, (d) => d.moveSheet?.leftBehind?.includes(line));
});

Then('the move sheet offers the folder {string}', async function (this: ThreadbearWorld, folder: string) {
  await this.until(`the folder ${folder}`, (d) => d.moveSheet?.folder === folder);
});

Then('the move sheet\'s login is {string}', async function (this: ThreadbearWorld, login: string) {
  await this.until(`the login ${login}`, (d) => d.moveSheet?.login === login);
});

When('I press Move in the move sheet for {string}', async function (this: ThreadbearWorld, runner: string) {
  await this.until('the sheet ready to move', (d) => d.moveSheet?.canMove === true);
  // Past the sheet's entrance animation, so the click lands on the button
  // where it's about to settle (GPUI's dialog slides in).
  await sleep(1200);
  await this.app.do({ cmd: 'click', tip: `Move it to ${runner} once this turn ends` });
});

Then('the move sheet is closed', async function (this: ThreadbearWorld) {
  await this.until('the sheet gone', (d) => !d.moveSheet);
});

Then('the move sheet stayed closed', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  assert.ok(!d.moveSheet, 'the sheet came up');
});
