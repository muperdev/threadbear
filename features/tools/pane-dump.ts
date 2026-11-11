// Prints what the pane beside the thread draws, for working on panes:
//   TB_APP=swift node features/tools/pane-dump.ts <thread title> ['<json command>' ...]
// Commands run in order first, as in shot.ts ({"sleep":500} waits).
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { sleep, type Driver } from '../support/driver.ts';

const [title, ...rest] = process.argv.slice(2);
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
  await sleep(1500);
  for (const raw of rest) {
    const step = JSON.parse(raw.replaceAll('{id}', id)) as Record<string, unknown>;
    if (typeof step.sleep === 'number') await sleep(step.sleep);
    else console.log(await app.do(step));
    await sleep(300);
  }
  await sleep(800);
  const d = (await app.dump()) as unknown as { panels?: unknown; actions?: unknown; transcript: { rows: { kind: string; text: string; id: string }[] } };
  console.log(d.transcript.rows.map((r) => r.id + " " + r.kind + ": " + r.text.slice(0, 80)).join("\n"));
  console.log(JSON.stringify({ panels: d.panels, actions: d.actions }, null, 1));
} finally {
  await app.stop();
}
