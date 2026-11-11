import { Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';

When('the runner restarts', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'restart', params: {} });
});

Then('the app is connected again', async function (this: ThreadbearWorld) {
  await this.until('the app connected', (d) => d.connection === 'connected', 30_000);
});

Then('the transcript still shows the thread {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" still open with its rows`, (d) => d.selection?.title === title && d.transcript.rows.length > 0);
});
