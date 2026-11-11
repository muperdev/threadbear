// Real mouse drags from along the top of the GPUI window, saying which of them
// move it. A scripted click can't show this: the window move is AppKit's, from a
// real mouse event. Takes over the pointer, so only on a machine nobody's using:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- node features/tools/window-drag.ts
import { execFileSync } from 'node:child_process';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep } from '../support/driver.ts';
import { assertInFront, swiftTool, windowFrame } from './real-screen.ts';

const dragger = swiftTool('realdrag');

const app = new GpuiDriver();
await app.start();
try {
  for (let i = 0; i < 300; i += 1) {
    const d = await app.dump();
    if (d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length)) break;
    await sleep(50);
  }
  const id = (await app.dump()).sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === 'Morning brief')!.id;
  await app.do({ cmd: 'open', id });
  await sleep(1200);
  const frame = () => windowFrame(app.pid!);
  {
    const [x, y] = windowFrame(app.pid!);
    assertInFront(app.pid!, [[x + 700, y + 480], [x + 24, y + 14], [x + 400, y + 14], [x + 1100, y + 14]]);
  }
  // A click on a blank part of the transcript first, to make the window key.
  const [x0, y0, w0, h0] = frame();
  console.log(`window ${w0} x ${h0}`);
  execFileSync(dragger, [String(x0 + 700), String(y0 + 480), String(x0 + 700), String(y0 + 480), '0', '']);
  await sleep(500);
  const spots: [string, number, number][] = [
    ['activity bar, top', 24, 14],
    ['beside the traffic lights', 92, 14],
    ['top of the thread list', 200, 14],
    ['thread bar, over the title', 400, 14],
    ['thread bar, blank part', 620, 14],
    ['thread bar, blank part lower down', 620, 30],
    ['Highlights bar, blank part', 1100, 14],
  ];
  let stayed = 0;
  for (const [name, dx, dy] of spots) {
    const [x, y] = frame();
    // Moving straight away, as a hand does: a press held still is a panel drag.
    execFileSync(dragger, [String(x + dx), String(y + dy), String(x + dx + 70), String(y + dy + 50), '0.1', ''], { env: { ...process.env, RD_PRESS: '0.02' } });
    await sleep(700);
    const [nx, ny] = frame();
    console.log(`${nx !== x || ny !== y ? 'moved' : 'STAYED'}  ${name}  (${x},${y}) -> (${nx},${ny})`);
    if (nx === x && ny === y) stayed += 1;
    if ((await app.dump()).header?.renaming) {
      console.log(`  RENAMING after the drag from ${name}: a drag isn't a click`);
      stayed += 1;
      await app.do({ cmd: 'menu-key', key: 'escape' });
    }
    if (nx !== x || ny !== y) {
      // Back where it was, by the same grip.
      execFileSync(dragger, [String(nx + dx), String(ny + dy), String(x + dx), String(y + dy), '0.1', ''], { env: { ...process.env, RD_PRESS: '0.02' } });
      await sleep(700);
    }
  }
  // A plain click on the title still renames.
  const [x, y] = frame();
  execFileSync(dragger, [String(x + 400), String(y + 14), String(x + 400), String(y + 14), '0', '']);
  await sleep(700);
  const renaming = (await app.dump()).header?.renaming === true;
  console.log(renaming ? 'a click on the title renames' : 'CLICK ON THE TITLE DID NOT RENAME');
  console.log(stayed === 0 && renaming ? 'PASS' : 'FAIL');
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
} finally {
  await app.stop();
  process.exit();
}
