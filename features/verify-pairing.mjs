// Fails when a .feature file has no step file of its own, or a scenario's
// tag doesn't name a real row in docs/gpui-port/features.md. As in Let's
// Build It: steps/<name>.steps.ts owns <name>.feature, and files starting
// with _ don't own one.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const features = fs.readdirSync(here).filter((f) => f.endsWith('.feature'));
const steps = fs.readdirSync(path.join(here, 'steps')).filter((f) => f.endsWith('.steps.ts') && !f.startsWith('_'));
const errors = [];

for (const s of steps) {
  const name = s.replace(/\.steps\.ts$/, '');
  if (!features.includes(`${name}.feature`)) errors.push(`steps/${s} has no ${name}.feature`);
}
for (const f of features) {
  if (!steps.includes(f.replace(/\.feature$/, '.steps.ts'))) errors.push(`${f} has no step file (steps/${f.replace(/\.feature$/, '.steps.ts')})`);
}

// Every scenario says which features.md row it covers, so the parity table
// can be worked out from what passes.
const ids = new Set([...fs.readFileSync(path.join(here, '../docs/gpui-port/features.md'), 'utf8').matchAll(/^\| `([a-z0-9.\-_]+)` \|/gm)].map((m) => m[1]));
const notIds = new Set(['manual', 'wip', 'swift-pending', 'gpui-pending', 'long-thread', 'remote-threads', 'quiet-thread', 'project-decisions']);
for (const f of features) {
  const lines = fs.readFileSync(path.join(here, f), 'utf8').split('\n');
  lines.forEach((line, i) => {
    if (/^\s*Scenario( Outline)?:/.test(line)) {
      const tags = (lines[i - 1] ?? '').trim().startsWith('@') ? lines[i - 1].trim().split(/\s+/) : [];
      const named = tags.map((t) => t.slice(1)).filter((t) => !notIds.has(t));
      if (!named.length) errors.push(`${f}:${i + 1} has no features.md id tag`);
      for (const t of named) if (!ids.has(t)) errors.push(`${f}:${i} tags @${t}, which isn't in docs/gpui-port/features.md`);
    }
  });
}

if (errors.length) {
  console.error(`Feature files don't line up:\n\n${errors.join('\n')}`);
  process.exit(1);
}
console.log(`${features.length} feature files, each with its steps, and every scenario names a features.md row.`);
