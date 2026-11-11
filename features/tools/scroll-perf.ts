// How smoothly the GPUI app scrolls a long thread under a trackpad, from GPUI's
// own frame profiler: frames shown, how many were late (a hitch you can see),
// and how long their work took. Real gestures through the window server, so
// only on a machine nobody's using, and a release build with frame-stats:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- bash -c 'cd desktop &&
//     cargo build --release -p threadbear-gpui --features automation,frame-stats &&
//     cd .. && TB_GPUI_PROFILE=release node features/tools/scroll-perf.ts [fixtures] [thread id]'
//
// With no fixtures it makes a long made-up thread (desktop/scripts/make-long-thread.mjs).
// It saves a CPU profile (`sample`) of the second round of flings and says where.
import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep } from '../support/driver.ts';
import { assertInFront, swiftTool, windowFrame, work } from './real-screen.ts';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
let fixtures = process.argv[2];
let id = process.argv[3];
if (!fixtures) {
  fixtures = path.join(work, 'fixtures');
  // And 60 others, so the thread list is as full as a real one.
  execFileSync(process.execPath, [path.join(root, 'desktop/scripts/make-long-thread.mjs'), fixtures, '1500', '60']);
  id = 'long_thread';
}
process.env.TB_MOCK_FIXTURES = fixtures;
const gesture = swiftTool('post-gesture');
const dragger = swiftTool('realdrag');

type Screen = { hz: number; drawn: number; presented: number; late: number; missedRefreshes: number; workMs: Record<string, number>; drawMs: Record<string, number> };
function show(what: string, stats: { screens: Screen[] }) {
  console.log(`\n${what}`);
  if (!stats.screens.length) console.log('  no frames recorded');
  for (const s of stats.screens) {
    const pct = s.presented ? Math.round((100 * s.late) / s.presented) : 0;
    console.log(`  ${s.hz} Hz screen: ${s.presented} frames shown, ${s.late} late (${pct}%), ${s.missedRefreshes} refreshes missed`);
    console.log(`    work per frame (ms): median ${s.workMs.p50}, 90% ${s.workMs.p90}, 99% ${s.workMs.p99}, worst ${s.workMs.max}`);
    console.log(`    draw per frame (ms): median ${s.drawMs.p50}, 90% ${s.drawMs.p90}, 99% ${s.drawMs.p99}, worst ${s.drawMs.max}`);
    console.log(`    a 120 Hz screen allows 8.3 ms a frame, 60 Hz 16.7`);
  }
}

const app = new GpuiDriver();
await app.start();
try {
  for (let i = 0; i < 400; i += 1) {
    const d = await app.dump();
    if (d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length)) break;
    await sleep(50);
  }
  id ??= (await app.dump()).sidebar.sections.flatMap((s) => s.threads)[0].id;
  await app.do({ cmd: 'open', id });
  for (let i = 0; i < 400 && (await app.dump()).transcript.state !== 'loaded'; i += 1) await sleep(50);
  await app.do({ cmd: 'load-all' }, 60);
  await sleep(1500);
  console.log(`${(await app.dump()).transcript.rows.length} rows in the thread`);

  const [x, y, w, h] = windowFrame(app.pid!);
  console.log(`window ${w} x ${h}`);
  assertInFront(app.pid!, [[x + 640, y + h - 50], [x + 620, y + Math.round(h / 2)]]);
  // In front and key: a click in the composer, which only focuses it.
  execFileSync(dragger, [String(x + 640), String(y + h - 50), String(x + 640), String(y + h - 50), '0', '']);
  await sleep(800);
  const cx = x + 620;
  const cy = y + Math.round(h / 2);

  // A fling: fingers move 30 times, 16 ms apart, then lift with momentum.
  const fling = (dy: number) => execFileSync(gesture, [String(cx), String(cy), '0', ...Array(30).fill(`0,${dy}`), 'M']);

  // `do` hands back JSON as text.
  const stats = async () => JSON.parse(await app.do({ cmd: 'frame-stats', reset: true }));
  await stats();
  // Up through rows it hasn't drawn yet (it opens at the bottom), then back down.
  for (let i = 0; i < 8; i += 1) fling(30);
  for (let i = 0; i < 8; i += 1) fling(-30);
  show('First round: flings up through rows not drawn before, then back down', await stats());

  const profile = path.join(fs.realpathSync(process.env.TB_PROFILE_DIR ?? '/tmp'), 'tb-scroll-sample.txt');
  const sampler = spawn('/usr/bin/sample', [String(app.pid), '6', '-file', profile], { stdio: 'ignore' });
  // Listened for now: the flings below block, and it may well be done first.
  const sampled = new Promise((done) => sampler.on('exit', done));
  await sleep(300);
  for (let i = 0; i < 8; i += 1) fling(30);
  for (let i = 0; i < 8; i += 1) fling(-30);
  show('Second round: the same flings again', await stats());
  await sampled;
  console.log(`\nCPU profile of the second round: ${profile}`);
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
} finally {
  await app.stop();
  process.exit();
}
