// How the GPUI app copes with a very long thread: time to open it, and how long
// a frame takes while it's scrolled. Stage 0's first risk check.
//
//   node features/tools/long-thread.ts <folder of a made-up thread>
//
// desktop/scripts/make-long-thread.mjs makes the folder. Run it on a machine with
// a real GPU (Tinker Desk), or the frame times are software rendering's.
import { GpuiDriver } from '../support/gpui.ts';
import { sleep } from '../support/driver.ts';

process.env.TB_MOCK_FIXTURES = process.argv[2];
const app = new GpuiDriver();
await app.start();
const started = Date.now();
for (;;) {
  const d = await app.dump();
  if (d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length)) break;
  await sleep(50);
}
console.log(`connected and listed in ${Date.now() - started} ms`);

const id = 'long_thread';
let t = Date.now();
await app.do({ cmd: 'open', id });
for (;;) {
  const d = await app.dump();
  if (d.transcript.state === 'loaded') {
    console.log(`first page open in ${Date.now() - t} ms, ${d.transcript.rows.length} rows`);
    break;
  }
  await sleep(20);
}
t = Date.now();
const firstCount = (await app.dump()).transcript.rows.length;
await app.do({ cmd: 'load-all' });
for (;;) {
  const d = await app.dump();
  if (d.transcript.rows.length > firstCount) {
    console.log(`everything loaded and folded in ${Date.now() - t} ms, ${d.transcript.rows.length} rows`);
    break;
  }
  await sleep(50);
}
await sleep(500);
console.log('scrolling, one frame per step:', await app.do({ cmd: 'page-scroll', steps: 240, dy: 40 }, 120));
console.log('faster scrolling:', await app.do({ cmd: 'page-scroll', steps: 240, dy: 400 }, 120));
await app.stop();
