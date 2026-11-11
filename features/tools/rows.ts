// What one thread's rows are in either app, for writing scenarios against them.
//
//   TB_APP=swift node features/tools/rows.ts <thread title> ['<steps json>']
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { sleep, type Driver } from '../support/driver.ts';

const [title, stepsJson] = process.argv.slice(2);
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
  await sleep(800);
  for (const step of JSON.parse(stepsJson || '[]') as Record<string, unknown>[]) {
    if (typeof step.sleep === 'number') await sleep(step.sleep);
    else console.log(await app.do(step));
  }
  await sleep(300);
  const d = await app.dump();
  const t = d.transcript as Record<string, unknown>;
  console.log(`threadId=${t.threadId} state=${t.state} atBottom=${t.atBottom} hasMoreAbove=${t.hasMoreAbove} rows=${d.transcript.rows.length}`);
  for (const r of d.transcript.rows as Record<string, unknown>[]) {
    console.log(`${String(r.id).padEnd(22)} ${String(r.kind).padEnd(10)} ${r.expanded ?? '-'} h=${r.height ?? '?'} ${r.onScreen ? '[ON]' : ''} ${String(r.text).slice(0, 80)}`);
  }
} finally {
  await app.stop();
}
