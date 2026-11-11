// Pictures of the same threads in the Swift app and the GPUI app, side by side
// on one page, for finding where the GPUI app looks different (slices.md, step
// 6). Each app opens once and goes through every scene. Needs both apps built
// and a desktop, so on Tinker Desk:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- bash -c '(cd app && swift build --disable-keychain --product Threadbear | tail -1) &&
//     (cd desktop && cargo build -p threadbear-gpui --features automation | tail -1) && node features/tools/compare.ts <out folder> [thread title ...]'
//
// With no titles it takes every demo thread in the Inbox. It writes
// <out folder>/index.html; copy the folder back and open that.
// TB_APPEARANCE=light or dark sets both apps' look (the desktop's otherwise).
import fs from 'node:fs';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { sleep, type Driver } from '../support/driver.ts';

const [outArg, ...wanted] = process.argv.slice(2);
if (!outArg) throw new Error('usage: compare.ts <out folder> [thread title ...]');
const out = path.resolve(outArg);
fs.mkdirSync(out, { recursive: true });

const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

async function shoot(name: 'swift' | 'gpui', app: Driver, titles: string[] | null): Promise<string[]> {
  await app.start();
  try {
    let threads: { id: string; title: string }[] = [];
    for (let i = 0; i < 400 && !threads.length; i += 1) {
      const d = await app.dump();
      if (d.connection === 'connected') threads = d.sidebar.sections.flatMap((s) => s.threads);
      if (!threads.length) await sleep(50);
    }
    if (process.env.TB_APPEARANCE) await app.do({ cmd: 'appearance', mode: process.env.TB_APPEARANCE });
    const scenes = titles ?? threads.map((t) => t.title);
    for (const title of scenes) {
      const thread = threads.find((t) => t.title === title);
      if (!thread) {
        console.log(`${name}: no thread called ${title}`);
        continue;
      }
      await app.do({ cmd: 'open', id: thread.id });
      for (let i = 0; i < 200; i += 1) {
        const d = await app.dump();
        if (d.transcript.state === 'loaded' && d.selection?.id === thread.id) break;
        await sleep(50);
      }
      // Pictures, rows measuring themselves, the side pane's fetch.
      await sleep(1500);
      await app.shot(path.join(out, `${slug(title)}-${name}.png`));
      console.log(`${name}: ${title}`);
    }
    return scenes;
  } finally {
    await app.stop();
  }
}

const titles = wanted.length ? wanted : null;
const scenes = await shoot('swift', new SwiftDriver(), titles);
await shoot('gpui', new GpuiDriver(), titles ?? scenes);

const rows = scenes
  .map((title) => {
    const s = slug(title);
    return `<h2>${title}</h2><div class="pair"><figure><img src="${s}-swift.png"><figcaption>Swift</figcaption></figure><figure><img src="${s}-gpui.png"><figcaption>GPUI</figcaption></figure></div>`;
  })
  .join('\n');
fs.writeFileSync(
  path.join(out, 'index.html'),
  `<!doctype html><meta charset="utf-8"><title>Swift and GPUI side by side</title>
<style>body{font:14px -apple-system,sans-serif;background:#111;color:#ddd;margin:20px}h2{font-size:15px;margin:28px 0 8px}
.pair{display:flex;gap:12px}figure{margin:0;flex:1}img{width:100%;border:1px solid #333}figcaption{color:#888;margin-top:4px}</style>
<h1>Swift and GPUI side by side</h1>${rows}`,
);
console.log(`\n${path.join(out, 'index.html')}`);
process.exit(0);
