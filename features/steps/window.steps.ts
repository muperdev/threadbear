// The window's panels, the zoom and what's remembered between launches.
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

Then('the panels are laid out as {string}', async function (this: ThreadbearWorld, layout: string) {
  await this.until(`the layout "${layout}"`, (d) => d.dock === layout);
});

Then("the panels aren't laid out as {string}", async function (this: ThreadbearWorld, layout: string) {
  await this.until('the layout to change', (d) => d.dock !== undefined && d.dock !== layout);
});

When('I move the {string} panel to the {string} edge', async function (this: ThreadbearWorld, panel: string, edge: string) {
  await this.app.do({ cmd: 'dock-move', panel, edge });
});

When('I choose {string} from the menu bar', async function (this: ThreadbearWorld, title: string) {
  await this.app.do({ cmd: 'menu-item', title });
});

When('I move the window and quit the app', function () {
  // By hand (see the feature file): this step only exists so the scenario reads.
  return 'pending';
});

When('I open the app again', function () {
  return 'pending';
});

Then('the window is where it was', function () {
  return 'pending';
});

// By hand, or features/tools/window-drag.ts on a desktop nobody's using.
When('I drag the window by the bar over the thread, by its title and beside it', function () {
  return 'pending';
});

Then('the window moves with the pointer each time', function () {
  return 'pending';
});

Then('a click on the title starts renaming the thread', function () {
  return 'pending';
});

When('the app is quit and opened again', async function (this: ThreadbearWorld) {
  await this.app.relaunch();
  await this.until('the app connected again, with its threads', (d) => d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length > 0), 30_000);
});

When('I press {string} with {string}', async function (this: ThreadbearWorld, key: string, modifiers: string) {
  await this.app.do({ cmd: 'menu-key', key, modifiers: modifiers.split(',').map((m) => m.trim()).filter(Boolean) });
});

When('I set the zoom to {int} percent', async function (this: ThreadbearWorld, percent: number) {
  await this.app.do({ cmd: 'zoom', percent });
});

When('I choose Actual Size', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'zoom', to: 'reset' });
});

Then('the zoom is {int} percent', async function (this: ThreadbearWorld, percent: number) {
  await this.until(`the zoom at ${percent}%`, (d) => d.zoom === percent);
});

// The reply: its text wraps to the window, so it grows with the zoom and
// stays in view (GPUI's list follows its tail, and zooming can push rows above
// off it). Noted by id, so a changed row list still compares the same one.
Given("I note how tall a row on screen is", async function (this: ThreadbearWorld) {
  const row = await this.until('a drawn reply on screen', (d) => d.transcript.rows.find((r) => r.kind === 'assistant' && r.onScreen && r.height));
  this.notedRow = { id: row.id, height: row.height ?? 0 };
});

Then('that row is taller than it was', async function (this: ThreadbearWorld) {
  const noted = this.notedRow;
  assert.ok(noted, 'no row noted');
  const after = await this.until('the row to have grown', (d) => {
    const h = d.transcript.rows.find((r) => r.id === noted.id)?.height;
    return h && h > noted.height ? h : undefined;
  });
  assert.ok(after > noted.height, `${after} is not taller than ${noted.height}`);
});
