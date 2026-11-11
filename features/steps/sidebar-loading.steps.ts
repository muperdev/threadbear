import { Then } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';

Then('the sidebar is showing loading rows', async function (this: ThreadbearWorld) {
  await this.until('the loading rows', (d) => d.sidebar.loading === true, 20_000);
});

Then('the sidebar then lists {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" after loading`, (d) => d.sidebar.loading === false && this.thread(d, title), 20_000);
});
