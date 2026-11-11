// Grok Build's own install: `~/.local/bin/grok` links to `~/.grok/bin/grok`,
// which links to `~/.grok/downloads/grok-<v>-<platform>`, one file per
// version. `grok update` downloads the new one and swaps the link, so a
// running Grok keeps the file it started from, as Claude's does. `grok
// update --check --json` says what's newest without installing anything
// (checked on grok 1.0.77, 9 Nov 2026).
import fs from 'node:fs';
import path from 'node:path';
import { VERSION_RE, type Exec } from './exec.ts';

// The version a file in ~/.grok/downloads is, from its name.
const DOWNLOAD_RE = /^grok-(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)-(?:macos|linux|windows)/;

// The version new sessions start: the links' final file, or `grok
// --version` for an install that isn't one. Undefined when there's no grok.
export async function grokInstalledVersion(grokPath: string, exec: Exec): Promise<string | undefined> {
  try {
    const v = DOWNLOAD_RE.exec(path.basename(fs.realpathSync(grokPath)))?.[1];
    if (v && VERSION_RE.test(v)) return v;
  } catch {
    return undefined; // not there
  }
  const r = await exec(grokPath, ['--version'], { timeoutMs: 10_000 });
  return r.code === 0 ? /(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)/.exec(r.stdout)?.[1] : undefined;
}

// The newest version on Grok's own channel, as `grok update --check
// --json` reports it ({currentVersion, latestVersion, updateAvailable, ...}).
export async function grokLatestVersion(grokPath: string, exec: Exec, env: NodeJS.ProcessEnv, timeoutMs: number): Promise<string | undefined> {
  const r = await exec(grokPath, ['update', '--check', '--json'], { env, timeoutMs });
  if (r.code !== 0) throw new Error(r.error ?? `grok update --check exited ${r.code}`);
  const line = r.stdout.split('\n').find((l) => l.trim().startsWith('{'));
  const latest = line ? (JSON.parse(line) as { latestVersion?: unknown }).latestVersion : undefined;
  return typeof latest === 'string' && VERSION_RE.test(latest) ? latest : undefined;
}

export type GrokUpdateResult =
  | { kind: 'updated'; from?: string; to: string; output: string }
  | { kind: 'unchanged'; version?: string; output: string }
  | { kind: 'failed'; message: string; output: string };

// Runs `grok update` (or `--version <v>` for one by name) and reads the
// links afterwards.
export async function runGrokUpdate(o: { grokPath: string; cwd: string; exec: Exec; env?: NodeJS.ProcessEnv; timeoutMs: number; version?: string }): Promise<GrokUpdateResult> {
  const before = await grokInstalledVersion(o.grokPath, o.exec);
  fs.mkdirSync(o.cwd, { recursive: true });
  const args = ['update', ...(o.version ? ['--version', o.version] : [])];
  const r = await o.exec(o.grokPath, args, { cwd: o.cwd, env: o.env, timeoutMs: o.timeoutMs });
  const output = `${r.stdout}${r.stderr}`;
  const after = await grokInstalledVersion(o.grokPath, o.exec);
  if (after && after !== before) return { kind: 'updated', from: before, to: after, output };
  if (r.code !== 0) return { kind: 'failed', message: r.error ?? `grok update exited ${r.code}`, output };
  return { kind: 'unchanged', version: after, output };
}
