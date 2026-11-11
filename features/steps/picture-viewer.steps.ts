import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

When('I click picture {int} of the row of {int} pictures', async function (this: ThreadbearWorld, number: number, count: number) {
  await this.until('a row of pictures', (d) => d.transcript.rows.some((r) => r.kind === 'images' || r.kind === 'user'));
  await sleep(300);
  await this.app.do({ cmd: 'tile', index: number - 1, count });
});

// The Swift app's own Quick Look would come over the desktop, so `viewer`
// says what a tile's click opened; in GPUI it's the real overlay.
async function viewerSays(world: ThreadbearWorld, pattern: RegExp): Promise<void> {
  const deadline = Date.now() + 15_000;
  for (;;) {
    const out = await world.app.do({ cmd: 'viewer' });
    if (pattern.test(out)) return;
    if (Date.now() > deadline) assert.fail(`viewer says "${out}", not ${pattern}`);
    await sleep(120);
  }
}

Then('the viewer shows picture {int} of {int} {string}', async function (this: ThreadbearWorld, number: number, total: number, file: string) {
  await viewerSays(this, new RegExp(`showing ${number} of ${total} .*${file.replace('.', '\\.')}$`));
});

When('I step the viewer forward', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'viewer', step: 1 });
});

When('I press Escape in the viewer', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'menu-key', key: 'escape' });
});

Then('the viewer is closed', async function (this: ThreadbearWorld) {
  await viewerSays(this, /closed/);
});
