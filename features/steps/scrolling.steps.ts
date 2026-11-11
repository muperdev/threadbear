import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

const reading = new WeakMap<object, string[]>();

Then('the view is at the bottom', async function (this: ThreadbearWorld) {
  await this.until('the view at the bottom', (d) => d.transcript.state === 'loaded' && d.transcript.atBottom);
});

Then('the view isn\'t at the bottom', async function (this: ThreadbearWorld) {
  await this.until('the view not at the bottom', (d) => d.transcript.atBottom === false);
});

Then('the view stays at the bottom', async function (this: ThreadbearWorld) {
  await this.until('streaming to begin', (d) => d.transcript.rows.some((r) => r.kind === 'working'));
  // Several looks over the turn's first second or two: pinned the whole time.
  for (let i = 0; i < 6; i += 1) {
    const d = await this.app.dump();
    assert.equal(d.transcript.atBottom, true, `not at the bottom after ${i} looks`);
    await new Promise((done) => setTimeout(done, 200));
  }
});

Then('there are older rows to page in', async function (this: ThreadbearWorld) {
  await this.until('older rows above', (d) => d.transcript.hasMoreAbove === true);
});

const rowsLoaded = new WeakMap<object, number>();

/** Rows on screen right now, in order. */
async function onScreen(world: ThreadbearWorld): Promise<string[]> {
  const d = await world.app.dump();
  return d.transcript.rows.filter((r) => r.onScreen).map((r) => r.id);
}

async function scrollToFirstRow(world: ThreadbearWorld): Promise<void> {
  const d = await world.app.dump();
  rowsLoaded.set(world, d.transcript.rows.length);
  assert.ok(d.transcript.rows.length > 0, 'no rows');
  // The first user message: a work group at the page boundary takes a new id
  // when the older page merges into it, so it's no good for finding again.
  const first = d.transcript.rows.find((r) => r.kind === 'user')?.id ?? d.transcript.rows[0].id;
  await world.app.do({ cmd: 'scroll', row: first, offset: 0 });
  // By id: older rows may land above it the moment the view gets near the top.
  await world.until(`row ${first} on screen`, (dump) => dump.transcript.rows.find((r) => r.id === first)?.onScreen === true);
}

When('I scroll up to the first row', async function (this: ThreadbearWorld) {
  await scrollToFirstRow(this);
});

When('I scroll up to the first row and read the rows on screen', async function (this: ThreadbearWorld) {
  await scrollToFirstRow(this);
  await new Promise((done) => setTimeout(done, 500));
  // The middle of the screen, a few rows in, away from the very edges.
  const seen = await onScreen(this);
  assert.ok(seen.length > 0, 'nothing on screen');
  reading.set(this, seen.slice(Math.max(0, Math.floor(seen.length / 2) - 1), Math.floor(seen.length / 2) + 2));
});

Then('older rows arrive above', async function (this: ThreadbearWorld) {
  const before = rowsLoaded.get(this) ?? 0;
  await this.until(`more than ${before} rows`, (d) => d.transcript.rows.length > before);
});

Then('what I was reading is still on screen', async function (this: ThreadbearWorld) {
  const ids = reading.get(this) ?? [];
  assert.ok(ids.length > 0, 'nothing was read');
  // Paging takes a moment to land; what matters is what's on screen after it.
  await new Promise((done) => setTimeout(done, 1200));
  await this.until(`rows ${ids.join(', ')} on screen`, (d) => {
    const shown = new Set(d.transcript.rows.filter((r) => r.onScreen).map((r) => r.id));
    return ids.some((id) => shown.has(id));
  });
});
