// The parity run's page (scripts/parity/run.sh): each scenario in a row,
// with the Mac app (another runner's thread, through Convex), the iPhone app
// and the website side by side, and the words each didn't show in red. Also
// writes results.json, prints one line per failure, and exits 1 if any
// surface missed a word or a picture.
//
//   node scripts/parity/gallery.mjs <out dir>
import fs from 'node:fs';
import path from 'node:path';

const out = process.argv[2];
const read = (f, d) => {
  try {
    return JSON.parse(fs.readFileSync(path.join(out, f), 'utf8'));
  } catch {
    return d;
  }
};
const seed = read('seed.json', { threads: [] });
const web = read('web.json', []);
const mac = read('mac.json', []);
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
const surfaces = [
  // checked: whether its words are checked. The iPhone's are only looked at.
  ['Mac, another runner', (i) => ({ file: `mac-${i + 1}.png`, missing: mac[i]?.missing, checked: true })],
  ['iPhone', (i) => ({ file: `iphone-${i + 1}.png`, missing: undefined, checked: false })],
  ['Website', (i) => ({ file: `web-${i + 1}.png`, missing: web[i]?.missing, checked: true })],
];
const results = [];
const failures = [];
for (const [i, t] of seed.threads.entries()) {
  const row = { name: t.name, surfaces: {} };
  for (const [label, of] of surfaces) {
    const { file, missing, checked } = of(i);
    const has = fs.existsSync(path.join(out, file));
    row.surfaces[label] = { file: has ? file : null, missing: missing ?? null };
    if (!has) failures.push(`${t.name}: no picture from ${label}`);
    else if (checked && !missing) failures.push(`${t.name}: ${label}'s words weren't checked (its step failed)`);
    else if (missing?.length) failures.push(`${t.name}: ${label} didn't show ${missing.map((m) => JSON.stringify(m)).join(', ')}`);
  }
  results.push(row);
}
if (!seed.threads.length) failures.push('no scenarios were seeded');
fs.writeFileSync(path.join(out, 'results.json'), JSON.stringify({ at: new Date().toISOString(), failures, results }, null, 2));
const cell = (s) =>
  s.file
    ? `<figure><img src="${esc(s.file)}" loading="lazy"><figcaption>${s.missing?.length ? `<b>Missing:</b> ${s.missing.map(esc).join(', ')}` : s.missing ? 'Shows every word' : 'Look at it'}</figcaption></figure>`
    : '<figure class="none">No picture</figure>';
const html = `<!doctype html><meta charset="utf-8"><title>Threadbear parity run</title>
<style>
body{background:#111;color:#ddd;font:14px -apple-system,system-ui,sans-serif;margin:24px}
h1{font-size:20px} h2{font-size:15px;margin:28px 0 8px} .bad{color:#ff6b6b}
.row{display:grid;grid-template-columns:2fr 1fr 2fr;gap:12px;align-items:start}
figure{margin:0} img{width:100%;border-radius:8px;border:1px solid #333}
figcaption{font-size:12px;color:#999;margin-top:4px} figcaption b{color:#ff6b6b} .none{color:#ff6b6b}
.head{display:grid;grid-template-columns:2fr 1fr 2fr;gap:12px;color:#888;font-size:12px}
</style>
<h1>Threadbear parity run, ${esc(new Date().toLocaleString())}</h1>
<p class="${failures.length ? 'bad' : ''}">${failures.length ? `${failures.length} problem${failures.length === 1 ? '' : 's'}:<br>${failures.map(esc).join('<br>')}` : 'Every surface showed every scenario.'}</p>
<div class="head">${surfaces.map(([l]) => `<div>${esc(l)}</div>`).join('')}</div>
${results.map((r) => `<h2>${esc(r.name)}</h2><div class="row">${surfaces.map(([l]) => cell(r.surfaces[l])).join('')}</div>`).join('\n')}
`;
fs.writeFileSync(path.join(out, 'index.html'), html);
for (const f of failures) console.log(f);
process.exit(failures.length ? 1 : 0);
