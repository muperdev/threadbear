import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

// What the composer's dump says about the open card and its picks.
type WithCard = Dump & { decisions?: string[] | null; composer: Dump['composer'] & { card?: string[] | null } };

const cardLines = (d: Dump): string[] => (d as WithCard).composer.card ?? [];

function decisions(d: Dump): string[] {
  return (d as WithCard).decisions ?? [];
}

Then('the recap box says {string}', async function (this: ThreadbearWorld, line: string) {
  await this.until(`the recap box saying "${line}"`, (d) => ((d.composer as { recap?: string[] | null }).recap ?? []).includes(line));
});

Then('the card in the composer says {string}', async function (this: ThreadbearWorld, heading: string) {
  await this.until(`the card saying "${heading}"`, (d) => cardLines(d)[0] === heading);
});

Then('the card in the composer asks {string}', async function (this: ThreadbearWorld, ask: string) {
  await this.until(`the card asking "${ask}"`, (d) => cardLines(d).some((l) => l.endsWith(ask)));
});

Then('the card offers the follow-up {string}', async function (this: ThreadbearWorld, followUp: string) {
  await this.until(`the card offering "${followUp}"`, (d) => cardLines(d).some((l) => l.includes(followUp)));
});

Then('the follow-up {string} is ticked', async function (this: ThreadbearWorld, followUp: string) {
  await this.until(`"${followUp}" ticked`, (d) => cardLines(d).some((l) => l.startsWith('[x] ') && l.includes(followUp)));
});

When('I pick answer {int} of ask {int} on the card', async function (this: ThreadbearWorld, choice: number, ask: number) {
  await this.app.do({ cmd: 'decide', pick: `${ask}:${choice}` });
});

When('I write {string} as my own answer to ask {int}', async function (this: ThreadbearWorld, words: string, ask: number) {
  await this.app.do({ cmd: 'decide', write: `${ask}:${words}` });
});

When('I send my picks', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'decide', send: true });
});

Then('ask {int} on the card is answered {string}', async function (this: ThreadbearWorld, ask: number, answer: string) {
  await this.until(`ask ${ask} answered "${answer}"`, (d) => decisions(d).some((l) => l.includes(`${ask}. `) && l.endsWith(`-> ${answer}`)));
});

async function openCardRow(world: ThreadbearWorld): Promise<string> {
  const d = await world.app.dump();
  const row = d.transcript.rows.find((r) => r.kind === 'asks');
  assert.ok(row, 'no card row in the transcript');
  return row.id;
}

When('I press {string} on the card', async function (this: ThreadbearWorld, action: string) {
  await this.app.do({ cmd: 'ask', row: await openCardRow(this), action });
});

When('I tick follow-up {int} on the card', async function (this: ThreadbearWorld, number: number) {
  await this.app.do({ cmd: 'ask', row: await openCardRow(this), action: `next:${number - 1}` });
});
