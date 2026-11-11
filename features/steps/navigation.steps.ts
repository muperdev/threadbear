// Back and forward, Next Inbox Thread, shortcuts.json and the menu bar.
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { okValue } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

When("I press the mouse's {string} button", async function (this: ThreadbearWorld, button: string) {
  await this.app.do({ cmd: 'mouse-button', button });
});

When('I open the first thread in the Inbox', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  const inbox = d.sidebar.sections.find((s) => s.name === 'Inbox')?.threads ?? [];
  assert.ok(inbox.length > 1, 'too little in the Inbox');
  await this.open(inbox[0].title);
});

Then('the second thread in the Inbox is open', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  const inbox = d.sidebar.sections.find((s) => s.name === 'Inbox')?.threads ?? [];
  const second = inbox[1].id;
  await this.until('the second inbox thread to open', (now) => now.selection?.id === second);
});

When('shortcuts.json gives {string} the key {string}', async function (this: ThreadbearWorld, command: string, key: string) {
  const out = await this.app.do({ cmd: 'data-dir' });
  const dir = okValue(out);
  assert.ok(fs.existsSync(dir), `no data folder: ${dir}`);
  fs.writeFileSync(path.join(dir, 'shortcuts.json'), JSON.stringify({ [command]: key }));
});

Then('the {string} menu has {string} on {string}', async function (this: ThreadbearWorld, menu: string, title: string, key: string) {
  let last = '';
  const deadline = Date.now() + 8000;
  for (;;) {
    const out = JSON.parse(okValue(await this.app.do({ cmd: 'menus' }))) as { title: string; items: { title: string; key?: string }[] }[];
    const item = out.find((m) => m.title === menu)?.items.find((i) => i.title === title);
    last = JSON.stringify(item ?? null);
    if (item && item.key === key) return;
    assert.ok(Date.now() < deadline, `${title} in ${menu}: ${last}`);
    await new Promise((r) => setTimeout(r, 200));
  }
});

Then('the {string} menu has {string}', async function (this: ThreadbearWorld, menu: string, title: string) {
  const out = JSON.parse(okValue(await this.app.do({ cmd: 'menus' }))) as { title: string; items: { title: string }[] }[];
  assert.ok(out.find((m) => m.title === menu)?.items.some((i) => i.title === title), `${title} isn't in the ${menu} menu: ${JSON.stringify(out.find((m) => m.title === menu)?.items.map((i) => i.title))}`);
});

// The app menu is the first one, whatever it's called (the dev build's is
// named for its instance).
Then('the app menu has {string} on {string}', async function (this: ThreadbearWorld, title: string, key: string) {
  const out = JSON.parse(okValue(await this.app.do({ cmd: 'menus' }))) as { title: string; items: { title: string; key?: string }[] }[];
  const item = out[0]?.items.find((i) => i.title === title);
  assert.equal(item?.key, key, `${title} in the app menu: ${JSON.stringify(item ?? null)}`);
});

Then("the open thread isn't {string}", async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" to be gone from the panel`, (d) => d.selection?.title !== title);
});
