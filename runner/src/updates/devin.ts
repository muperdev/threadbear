// The Devin CLI's own install (cli.devin.ai/install.sh, read 9 Nov 2026):
// `~/.local/bin/devin` links to `<data>/devin/cli/_versions/current/bin/devin`,
// `current` links to the version's folder, and each version is a folder of
// its own. `devin update` needs a terminal (headless, it exits 130), so
// Threadbear updates it the way the installer does: the manifest at
// static.devin.ai names the newest version and each platform's bundle and
// sha256; the bundle is checked, unpacked beside the others, and `current`
// is swapped in one rename. A running Devin keeps the folder it started from.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { VERSION_RE, type Exec, type FetchText } from './exec.ts';

export const DEVIN_BASE_URL = 'https://static.devin.ai/cli';

export type DevinInstall = {
  versionsDir: string; // <data>/devin/cli/_versions
  version: string; // the one `current` points at
  distribution?: string; // the installer's marker: curl-bash, homebrew, ...
};

// Where the binary really is, when it's the installer's layout:
// `<versionsDir>/<version>/bin/devin`. Undefined for any other install
// (Homebrew's, a path set by hand) or none.
export function devinInstall(devinPath: string): DevinInstall | undefined {
  let real: string;
  try {
    real = fs.realpathSync(devinPath);
  } catch {
    return undefined;
  }
  const versionDir = path.dirname(path.dirname(real));
  const versionsDir = path.dirname(versionDir);
  const version = path.basename(versionDir);
  if (path.basename(versionsDir) !== '_versions' || !VERSION_RE.test(version)) return undefined;
  let distribution: string | undefined;
  try {
    distribution = fs.readFileSync(path.join(versionDir, 'distribution'), 'utf8').trim() || undefined;
  } catch {
    // an older install without the marker is the plain one
  }
  return { versionsDir, version, ...(distribution ? { distribution } : {}) };
}

// The version new sessions start, from the layout or `devin --version`.
export async function devinInstalledVersion(devinPath: string, exec: Exec): Promise<string | undefined> {
  const install = devinInstall(devinPath);
  if (install) return install.version;
  if (!fs.existsSync(devinPath)) return undefined;
  const r = await exec(devinPath, ['--version'], { timeoutMs: 10_000 });
  return r.code === 0 ? /(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)/.exec(r.stdout)?.[1] : undefined;
}

// Only the plain curl|bash install reads this manifest; enterprise,
// federal and Homebrew installs have their own channels.
export function updatableInstall(install: DevinInstall | undefined): install is DevinInstall {
  return Boolean(install && (!install.distribution || install.distribution === 'curl-bash'));
}

export function devinTarget(platform: NodeJS.Platform = process.platform, arch: string = process.arch): string | undefined {
  const a = arch === 'arm64' ? 'aarch64' : arch === 'x64' ? 'x86_64' : undefined;
  if (!a) return undefined;
  if (platform === 'darwin') return `${a}-apple-darwin`;
  if (platform === 'linux') return `${a}-unknown-linux`;
  return undefined;
}

type Manifest = { version: string; url: string; sha256: string };

export async function devinManifest(fetchText: FetchText, timeoutMs: number, version?: string, target = devinTarget()): Promise<Manifest> {
  const raw = JSON.parse(await fetchText(`${DEVIN_BASE_URL}/${version ?? 'current'}/manifest.json`, timeoutMs)) as {
    version?: unknown;
    platforms?: Record<string, { url?: unknown; sha256?: unknown }>;
  };
  const v = raw.version;
  if (typeof v !== 'string' || !VERSION_RE.test(v)) throw new Error("Devin's manifest has no version");
  const p = target ? raw.platforms?.[target] : undefined;
  if (typeof p?.url !== 'string' || typeof p.sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(p.sha256)) throw new Error(`Devin has no bundle for ${target ?? process.platform}`);
  if (!p.url.startsWith(`${DEVIN_BASE_URL}/`)) throw new Error(`Devin's bundle is somewhere unexpected: ${p.url}`);
  return { version: v, url: p.url, sha256: p.sha256 };
}

export async function devinLatestVersion(fetchText: FetchText, timeoutMs: number): Promise<string | undefined> {
  return (await devinManifest(fetchText, timeoutMs)).version;
}

export type DevinUpdateResult =
  | { kind: 'updated'; from?: string; to: string; output: string }
  | { kind: 'unchanged'; version?: string; output: string }
  | { kind: 'failed'; message: string; output: string };

function sha256File(file: string): string {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

// Installs `version` (or the newest) into the install's versions folder and
// points `current` at it. Only ever touches that folder.
export async function runDevinUpdate(o: { devinPath: string; exec: Exec; fetchText: FetchText; timeoutMs: number; lookupMs: number; version?: string }): Promise<DevinUpdateResult> {
  const install = devinInstall(o.devinPath);
  if (!updatableInstall(install)) {
    const from = devinInstall(o.devinPath)?.distribution;
    return { kind: 'failed', message: from ? `This Devin came from ${from}, which updates it.` : "Devin isn't installed with its own installer, so Threadbear leaves it alone.", output: '' };
  }
  let m: Manifest;
  try {
    m = await devinManifest(o.fetchText, o.lookupMs, o.version);
  } catch (e) {
    return { kind: 'failed', message: (e as Error).message, output: '' };
  }
  if (m.version === install.version) return { kind: 'unchanged', version: m.version, output: '' };
  const { versionsDir } = install;
  const versionDir = path.join(versionsDir, m.version);
  let output = '';
  if (!fs.existsSync(path.join(versionDir, 'bin', 'devin'))) {
    const downloads = path.join(versionsDir, '_download');
    fs.mkdirSync(downloads, { recursive: true });
    const bundle = path.join(downloads, `${m.version}.tar.gz`);
    const tmp = path.join(versionsDir, `_${m.version}.tmp`);
    try {
      const dl = await o.exec('/usr/bin/curl', ['-fsSL', '--retry', '2', '-o', bundle, m.url], { timeoutMs: o.timeoutMs });
      output += dl.stdout + dl.stderr;
      if (dl.code !== 0) return { kind: 'failed', message: dl.error ?? `the download failed (curl exited ${dl.code})`, output };
      const sum = sha256File(bundle);
      if (sum !== m.sha256) return { kind: 'failed', message: `the download's checksum didn't match (expected ${m.sha256}, got ${sum})`, output };
      fs.rmSync(tmp, { recursive: true, force: true });
      fs.mkdirSync(tmp, { recursive: true });
      const tar = await o.exec('/usr/bin/tar', ['xzf', bundle, '-C', tmp], { timeoutMs: o.timeoutMs });
      output += tar.stdout + tar.stderr;
      if (tar.code !== 0) return { kind: 'failed', message: tar.error ?? `unpacking failed (tar exited ${tar.code})`, output };
      if (!fs.existsSync(path.join(tmp, 'bin', 'devin'))) return { kind: 'failed', message: 'the bundle has no bin/devin', output };
      fs.writeFileSync(path.join(tmp, 'distribution'), `${install.distribution ?? 'curl-bash'}\n`);
      fs.rmSync(versionDir, { recursive: true, force: true });
      fs.renameSync(tmp, versionDir);
    } finally {
      fs.rmSync(bundle, { force: true });
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  }
  const check = await o.exec(path.join(versionDir, 'bin', 'devin'), ['--version'], { timeoutMs: 30_000 });
  output += check.stdout + check.stderr;
  if (check.code !== 0) return { kind: 'failed', message: `Devin ${m.version} didn't start (${check.error ?? `exit ${check.code}`})`, output };
  // `current` swapped in one rename, so nothing ever finds it missing.
  const next = path.join(versionsDir, `.current-${process.pid}`);
  fs.rmSync(next, { force: true });
  fs.symlinkSync(m.version, next);
  fs.renameSync(next, path.join(versionsDir, 'current'));
  return { kind: 'updated', from: install.version, to: m.version, output };
}

// Versions other than the current one and the `keep - 1` newest before it
// go, once no process runs from them. `psArgs` is `ps -axo args=`.
export function cleanupDevinVersions(devinPath: string, keep: number, psArgs: string): string[] {
  const install = devinInstall(devinPath);
  if (!install) return [];
  const { versionsDir, version } = install;
  const versions = fs
    .readdirSync(versionsDir)
    .filter((v) => VERSION_RE.test(v) && v !== version)
    .sort((a, b) => fs.statSync(path.join(versionsDir, b)).mtimeMs - fs.statSync(path.join(versionsDir, a)).mtimeMs);
  const removed: string[] = [];
  for (const v of versions.slice(Math.max(0, keep - 1))) {
    const dir = path.join(versionsDir, v);
    if (psArgs.includes(dir)) continue;
    fs.rmSync(dir, { recursive: true, force: true });
    removed.push(v);
  }
  return removed;
}
