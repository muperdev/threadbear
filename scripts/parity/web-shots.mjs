// The website's half of the parity run (scripts/parity/run.sh): opens each
// scenario's thread in headless Chrome, signed in with the run's phone token,
// waits until the page shows the words the scenario must show (or 20 s),
// and saves a picture of it. Drives Chrome over its DevTools protocol with
// Node's own WebSocket, so the run needs nothing installed beyond Chrome.
// Prints JSON: each scenario's file and the words it didn't find.
//
//   node scripts/parity/web-shots.mjs <site url> <seed.json> <out dir>
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const [site, seedFile, out] = process.argv.slice(2);
if (!site || !seedFile || !out) {
  console.error('usage: web-shots.mjs <site url> <seed.json> <out dir>');
  process.exit(2);
}
const seed = JSON.parse(fs.readFileSync(seedFile, 'utf8'));
fs.mkdirSync(out, { recursive: true });

const CHROME = ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', '/usr/bin/google-chrome', '/usr/bin/chromium'].find((p) => fs.existsSync(p));
if (!CHROME) throw new Error('no Chrome to take the website\'s pictures with');
const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-parity-chrome-'));
const chrome = spawn(CHROME, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--window-size=1280,900', '--hide-scrollbars', '--force-dark-mode', '--no-first-run', 'about:blank'], { stdio: 'ignore' });

try {
  const port = await waitFor(() => {
    const f = path.join(profile, 'DevToolsActivePort');
    return fs.existsSync(f) ? Number(fs.readFileSync(f, 'utf8').split('\n')[0]) : undefined;
  }, 15000, 'Chrome to start');
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const page = targets.find((t) => t.type === 'page');
  const cdp = await connect(page.webSocketDebuggerUrl);
  await cdp.send('Page.enable');
  await cdp.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: 'dark' }] });
  const results = [];
  for (const [i, t] of seed.threads.entries()) {
    // The token goes in the hash first time; it stays in localStorage after.
    await cdp.send('Page.navigate', { url: `${site}/t/${encodeURIComponent(t.threadId)}#token=${seed.token}` });
    let missing = t.shows;
    const until = Date.now() + 20000;
    while (Date.now() < until) {
      const { result } = await cdp.send('Runtime.evaluate', { expression: 'document.body ? document.body.innerText : ""', returnByValue: true });
      missing = t.shows.filter((w) => !String(result.value).includes(w));
      if (!missing.length) break;
      await new Promise((r) => setTimeout(r, 500));
    }
    // A moment for pictures and fonts.
    await new Promise((r) => setTimeout(r, 800));
    const { data } = await cdp.send('Page.captureScreenshot', { format: 'png' });
    const file = `web-${i + 1}.png`;
    fs.writeFileSync(path.join(out, file), Buffer.from(data, 'base64'));
    results.push({ name: t.name, file, missing });
  }
  cdp.close();
  console.log(JSON.stringify(results, null, 2));
} finally {
  // Chrome goes on writing its profile for a moment after it's told to go.
  const gone = new Promise((r) => chrome.once('exit', r));
  chrome.kill();
  await Promise.race([gone, new Promise((r) => setTimeout(r, 5000))]);
  try {
    fs.rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  } catch {}
}

async function waitFor(get, ms, what) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    const v = get();
    if (v) return v;
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`timed out waiting for ${what}`);
}

// A tiny DevTools client: one socket, calls by id.
function connect(url) {
  const ws = new WebSocket(url);
  let next = 1;
  const pending = new Map();
  ws.onmessage = (m) => {
    const msg = JSON.parse(String(m.data));
    const p = msg.id && pending.get(msg.id);
    if (!p) return;
    pending.delete(msg.id);
    if (msg.error) p.reject(new Error(msg.error.message));
    else p.resolve(msg.result);
  };
  return new Promise((resolve, reject) => {
    ws.onerror = () => reject(new Error(`couldn't reach Chrome at ${url}`));
    ws.onopen = () =>
      resolve({
        send: (method, params = {}) =>
          new Promise((res, rej) => {
            const id = next++;
            pending.set(id, { resolve: res, reject: rej });
            ws.send(JSON.stringify({ id, method, params }));
          }),
        close: () => ws.close(),
      });
  });
}
