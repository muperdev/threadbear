// A picture of one thread in whichever app TB_APP says, for comparing them.
//
//   TB_APP=gpui node features/tools/shot.ts <thread title> <out.png> ['<json command or list>' ...]
//
// Commands after the output file run in order before the picture is taken, each
// as the apps' automation takes them (`{"cmd":"expand","row":"g:"}`, or
// `{"cmd":"collapse","section":"inbox"}` to bring a section into view), or
// `{"sleep":500}`. "{id}" in one is the opened thread's id.
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { sleep, type Driver } from '../support/driver.ts';

const [title, out, ...rest] = process.argv.slice(2);
const app: Driver = (process.env.TB_APP ?? 'gpui') === 'swift' ? new SwiftDriver() : new GpuiDriver();
await app.start();
try {
  let id = '';
  for (let i = 0; i < 300 && !id; i += 1) {
    const d = await app.dump();
    id = d.sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === title)?.id ?? '';
    if (!id) await sleep(50);
  }
  await app.do({ cmd: 'open', id });
  for (let i = 0; i < 200; i += 1) {
    const d = await app.dump();
    if (d.transcript.state === 'loaded' && d.selection?.id === id) break;
    await sleep(50);
  }
  await sleep(1200);
  for (const raw of rest) {
    const parsed = JSON.parse(raw.replaceAll('{id}', id)) as Record<string, unknown> | Record<string, unknown>[];
    for (const step of Array.isArray(parsed) ? parsed : [parsed]) {
      if (typeof step.sleep === 'number') await sleep(step.sleep);
      else console.log(await app.do(step));
      await sleep(200);
    }
  }
  await sleep(800);
  await app.shot(out);
  console.log(`saved ${out}`);
} finally {
  await app.stop();
}
