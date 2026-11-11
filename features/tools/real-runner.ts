// Checks the GPUI app against a real runner, not the mock: a dev runner's data
// folder, one small turn on the cheapest model. The mock can drift from the real
// protocol, and this is what would show it.
//
//   TB_RUNNER_HOME=<dev runner home> node features/tools/real-runner.ts <thread id>
import { GpuiDriver } from '../support/gpui.ts';
import { sleep } from '../support/driver.ts';

const id = process.argv[2];
const app = new GpuiDriver();
await app.start();
try {
  for (let i = 0; i < 300; i += 1) {
    const d = await app.dump();
    if (d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.some((t) => t.id === id))) break;
    await sleep(100);
  }
  await app.do({ cmd: 'open', id });
  for (let i = 0; i < 100; i += 1) {
    const d = await app.dump();
    if (d.transcript.state === 'loaded') break;
    await sleep(100);
  }
  const before = (await app.dump()).transcript.rows.length;
  console.log(`open with ${before} rows`);
  await app.do({ cmd: 'send', text: 'Reply with just the word pong.' });
  const started = Date.now();
  for (;;) {
    const d = await app.dump();
    const reply = d.transcript.rows.filter((r) => r.kind === 'assistant').at(-1);
    if (reply && d.selection?.status === 'idle' && d.transcript.rows.length > before + 1 && !d.transcript.rows.some((r) => r.kind === 'working')) {
      console.log(`reply after ${Date.now() - started} ms: ${JSON.stringify(reply.text)}`);
      console.log(d.transcript.rows.map((r) => r.kind).join(' '));
      break;
    }
    if (Date.now() - started > 120_000) {
      console.log('no reply in time:', JSON.stringify(d.transcript.rows.map((r) => [r.kind, r.text]).slice(-6)));
      process.exitCode = 1;
      break;
    }
    await sleep(250);
  }
} finally {
  await app.stop();
  process.exit(process.exitCode ?? 0);
}
