import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

// A chip's menu as text lines: `composer-menu` gives the same list on both
// apps, the Swift one after "ok ".
async function menuLines(world: ThreadbearWorld, menu: string): Promise<string[]> {
  const out = await world.app.do({ cmd: 'composer-menu', menu });
  return JSON.parse(out.slice(out.indexOf('['))) as string[];
}

Then('the {string} menu shows {string} ticked', async function (this: ThreadbearWorld, menu: string, title: string) {
  let lines: string[] = [];
  for (let tries = 0; tries < 40; tries += 1) {
    lines = await menuLines(this, menu);
    if (lines.find((l) => l.includes(title))?.startsWith('[x] ')) return;
    await sleep(150);
  }
  assert.fail(`the ${menu} menu has no ticked "${title}":\n${lines.join('\n')}`);
});

Then('the {string} menu has a row {string}', async function (this: ThreadbearWorld, menu: string, row: string) {
  const lines = await menuLines(this, menu);
  assert.ok(lines.some((l) => l.startsWith(row)), `no row "${row}" in the ${menu} menu:\n${lines.join('\n')}`);
});

When('I pick {string} from the {string} menu', async function (this: ThreadbearWorld, title: string, menu: string) {
  await this.app.do({ cmd: 'composer-menu', menu, click: title });
});

Then('the composer\'s permission mode is {string}', async function (this: ThreadbearWorld, mode: string) {
  await this.until(`permission mode ${mode}`, (d) => (d.composer as { mode?: string }).mode === mode);
});

Then('the model chip says {string}', async function (this: ThreadbearWorld, chip: string) {
  await this.until(`the model chip saying "${chip}"`, (d) => (d.composer as { chip?: string }).chip === chip);
});

Then('the {string} menu says {string} and {string}', async function (this: ThreadbearWorld, menu: string, first: string, second: string) {
  const lines = await menuLines(this, menu);
  assert.deepEqual(lines.slice(0, 2), [first, second]);
});

When('I open the context ring\'s popover', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'composer-menu', menu: 'context', show: true });
});

// The breakdown comes after the runner answers, a moment after the popover
// opens, so this waits for the line.
Then('the context popover shows {string}', async function (this: ThreadbearWorld, line: string) {
  let lines: string[] = [];
  for (let tries = 0; tries < 40; tries += 1) {
    lines = await menuLines(this, 'context');
    if (lines.includes(line)) return;
    await sleep(150);
  }
  assert.fail(`the context popover has no "${line}":\n${lines.join('\n')}`);
});
