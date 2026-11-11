// Works out the parity table from what actually passed.
//
//   node parity.mjs [reports folder]
//
// Each machine's run leaves reports/<app>-<os>.json. This reads those, the tags
// in the .feature files and the rows of docs/gpui-port/features.md, and rewrites
// the table between the markers in docs/gpui-port/PARITY.md.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const reports = path.resolve(process.argv[2] ?? path.join(here, 'reports'));
const columns = [
  { key: 'swift-mac', title: 'Swift', app: 'swift' },
  { key: 'gpui-mac', title: 'Mac', app: 'gpui' },
  { key: 'gpui-windows', title: 'Windows', app: 'gpui' },
  { key: 'gpui-linux', title: 'Linux', app: 'gpui' },
];

// The rows, in features.md's order, under their headings.
const rows = [];
let heading = '';
for (const line of fs.readFileSync(path.join(here, '../docs/gpui-port/features.md'), 'utf8').split('\n')) {
  const h = /^## (.+)/.exec(line);
  if (h) heading = h[1];
  const r = /^\| `([a-z0-9.\-_]+)` \| ([^|]+) \|/.exec(line);
  if (r) rows.push({ id: r[1], name: r[2].trim(), heading });
}

// Every scenario and its tags, from the files rather than a run, so one that
// no machine has run yet still counts as there.
const scenarios = [];
for (const f of fs.readdirSync(here).filter((n) => n.endsWith('.feature')).sort()) {
  const lines = fs.readFileSync(path.join(here, f), 'utf8').split('\n');
  lines.forEach((line, i) => {
    const m = /^\s*Scenario(?: Outline)?: (.+)/.exec(line);
    if (!m) return;
    const tags = (lines[i - 1] ?? '').trim().startsWith('@') ? lines[i - 1].trim().split(/\s+/).map((t) => t.slice(1)) : [];
    scenarios.push({ feature: f, name: m[1].trim(), tags });
  });
}

function results(key) {
  const file = path.join(reports, `${key}.json`);
  if (!fs.existsSync(file)) return undefined;
  const out = new Map();
  for (const feature of JSON.parse(fs.readFileSync(file, 'utf8'))) {
    for (const el of feature.elements ?? []) {
      if (el.type !== 'scenario') continue;
      const passed = el.steps.every((s) => s.result?.status === 'passed' || s.result?.status === 'skipped');
      const anyRun = el.steps.some((s) => s.result?.status === 'passed' && !s.hidden);
      out.set(`${path.basename(feature.uri)}: ${el.name}`, passed && anyRun);
    }
  }
  return out;
}

const runs = Object.fromEntries(columns.map((c) => [c.key, results(c.key)]));

function cell(column, row) {
  const covering = scenarios.filter((s) => s.tags.includes(row.id));
  if (!covering.length) return '';
  const run = runs[column.key];
  let seen = false;
  let pending = false;
  for (const s of covering) {
    if (s.tags.includes(`${column.app}-pending`) || s.tags.includes('manual')) {
      pending = true;
      continue;
    }
    const got = run?.get(`${s.feature}: ${s.name}`);
    if (got === false) return 'FAIL';
    if (got === true) seen = true;
    else pending = true;
  }
  if (pending) return seen ? 'partly' : 'pending';
  return seen ? 'pass' : '';
}

const specced = rows.filter((r) => scenarios.some((s) => s.tags.includes(r.id)));
const lines = [];
lines.push(`${specced.length} of ${rows.length} features have a spec so far, across ${scenarios.length} scenarios.`);
lines.push('');
const counts = columns.map((c) => {
  const run = runs[c.key];
  if (!run) return `${c.title}: not run`;
  const passed = [...run.values()].filter(Boolean).length;
  return `${c.title}: ${passed} of ${scenarios.length}`;
});
lines.push(`Scenarios passing: ${counts.join(', ')}.`);
lines.push('');
let last = '';
for (const row of specced) {
  if (row.heading !== last) {
    last = row.heading;
    lines.push('', `### ${row.heading}`, '', `| Feature | ${columns.map((c) => c.title).join(' | ')} |`, `|---|${columns.map(() => '---').join('|')}|`);
  }
  lines.push(`| \`${row.id}\` ${row.name} | ${columns.map((c) => cell(c, row)).join(' | ')} |`);
}

const out = path.join(here, '../docs/gpui-port/PARITY.md');
const text = fs.readFileSync(out, 'utf8');
const start = '<!-- parity:start -->';
const end = '<!-- parity:end -->';
const a = text.indexOf(start);
const b = text.indexOf(end);
if (a < 0 || b < 0) throw new Error('PARITY.md has lost its markers');
fs.writeFileSync(out, `${text.slice(0, a + start.length)}\n\n${lines.join('\n')}\n\n${text.slice(b)}`);
console.log(`PARITY.md: ${specced.length} features, ${counts.join(', ')}`);
