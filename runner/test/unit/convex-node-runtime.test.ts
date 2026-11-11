// Convex bundles each file in convex/ for its own runtime, and refuses a
// deploy where a file without "use node" reaches Node's built-ins, or
// imports a file that has it. The typecheck doesn't see either, so the
// backend deploy at 38209ef failed on convex/pushSeal.ts (9 Nov).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, existsSync } from 'node:fs';
import { builtinModules } from 'node:module';
import { join } from 'node:path';

const root = join(import.meta.dirname, '../../..');
const builtins = new Set(builtinModules.filter((m) => !m.startsWith('_')));

for (const dir of ['convex', 'relay/convex']) {
  test(`${dir}: Node built-ins only in "use node" files, and those only imported by each other`, () => {
    const full = join(root, dir);
    if (!existsSync(full)) return;
    const files = readdirSync(full).filter((f) => /\.tsx?$/.test(f) && !f.endsWith('.d.ts'));
    const src = new Map(files.map((f) => [f.replace(/\.tsx?$/, ''), readFileSync(join(full, f), 'utf8')]));
    const isNode = (s: string) => /^\s*(\/\/[^\n]*\n\s*)*["']use node["']/.test(s);
    const problems: string[] = [];
    for (const [name, s] of src) {
      const node = isNode(s);
      for (const m of s.matchAll(/^\s*import\s[^'"]*['"]([^'"]+)['"]/gm)) {
        const spec = m[1];
        const bare = spec.replace(/^node:/, '').split('/')[0];
        if (!node && (spec.startsWith('node:') || builtins.has(bare))) problems.push(`${name} imports ${spec} without "use node"`);
        const local = spec.match(/^\.\/([\w.-]+)$/)?.[1];
        if (!node && local && src.has(local) && isNode(src.get(local)!)) problems.push(`${name} imports "use node" file ${local}`);
      }
    }
    assert.deepEqual(problems, []);
  });
}
