// Grok's and Devin's updates against stub installs laid out as theirs are
// (grok.ts, devin.ts). Devin's bundle is a real tarball built here; its
// "download" is a copy, so nothing touches the network or a real install.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { grokInstalledVersion, grokLatestVersion, runGrokUpdate } from '../../src/updates/grok.ts';
import { DEVIN_BASE_URL, cleanupDevinVersions, devinInstall, devinInstalledVersion, devinTarget, runDevinUpdate } from '../../src/updates/devin.ts';
import { CliUpdates } from '../../src/updates/manager.ts';
import { defaultExec, type Exec } from '../../src/updates/exec.ts';
import { FakeClock } from '../../src/clock.ts';
import { nullLogger } from '../../src/log.ts';
import { testConfig, tmpDir } from '../helpers/fakes.ts';

function script(file: string, body: string): void {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, `#!/bin/sh\n${body}\n`, { mode: 0o755 });
}

// R/.grok/downloads/grok-<v>-macos-aarch64, R/.grok/bin/grok -> one, and
// R/bin/grok -> R/.grok/bin/grok, as the real install. `update` swaps the
// inner link to 1.0.78 (or --version's).
function fakeGrok(root: string): string {
  const downloads = path.join(root, '.grok', 'downloads');
  const inner = path.join(root, '.grok', 'bin', 'grok');
  for (const v of ['1.0.77', '1.0.78', '1.0.70']) {
    script(
      path.join(downloads, `grok-${v}-macos-aarch64`),
      `case "$1" in
  --version) echo "grok ${v} (abc)";;
  update)
    if [ "$2" = "--check" ]; then echo '{"currentVersion":"${v}","latestVersion":"1.0.78","updateAvailable":true}'; exit 0; fi
    to=1.0.78; [ "$2" = "--version" ] && to="$3"
    [ -f "${downloads}/grok-$to-macos-aarch64" ] || { echo "no such version" >&2; exit 2; }
    ln -sfn "../downloads/grok-$to-macos-aarch64" "${inner}"; echo "updated to $to";;
esac`,
    );
  }
  fs.mkdirSync(path.dirname(inner), { recursive: true });
  fs.symlinkSync('../downloads/grok-1.0.77-macos-aarch64', inner);
  const outer = path.join(root, 'bin', 'grok');
  fs.mkdirSync(path.dirname(outer), { recursive: true });
  fs.symlinkSync(inner, outer);
  return outer;
}

test('grok: the installed version comes from the link, latest from update --check --json', async () => {
  const root = tmpDir('tb-grok-');
  const grok = fakeGrok(root);
  assert.equal(await grokInstalledVersion(grok, defaultExec), '1.0.77');
  assert.equal(await grokLatestVersion(grok, defaultExec, process.env, 10_000), '1.0.78');
  assert.equal(await grokInstalledVersion(path.join(root, 'nope'), defaultExec), undefined);
});

test('grok: update swaps the link and says from what; a version by name goes back', async () => {
  const root = tmpDir('tb-grok-');
  const grok = fakeGrok(root);
  const r = await runGrokUpdate({ grokPath: grok, cwd: path.join(root, 'run'), exec: defaultExec, timeoutMs: 10_000 });
  assert.deepEqual([r.kind, r.kind === 'updated' && r.from, r.kind === 'updated' && r.to], ['updated', '1.0.77', '1.0.78']);
  const again = await runGrokUpdate({ grokPath: grok, cwd: path.join(root, 'run'), exec: defaultExec, timeoutMs: 10_000 });
  assert.equal(again.kind, 'unchanged');
  const back = await runGrokUpdate({ grokPath: grok, cwd: path.join(root, 'run'), exec: defaultExec, timeoutMs: 10_000, version: '1.0.70' });
  assert.equal(back.kind === 'updated' && back.to, '1.0.70');
  const bad = await runGrokUpdate({ grokPath: grok, cwd: path.join(root, 'run'), exec: defaultExec, timeoutMs: 10_000, version: '9.9.9' });
  assert.equal(bad.kind, 'failed');
});

// R/data/devin/cli/_versions/<v>/bin/devin, current -> <v>, R/bin/devin ->
// .../_versions/current/bin/devin, as cli.devin.ai/install.sh leaves it.
function fakeDevin(root: string, versions = ['3000.11.205']): { devin: string; versionsDir: string } {
  const versionsDir = path.join(root, 'data', 'devin', 'cli', '_versions');
  for (const v of versions) {
    script(path.join(versionsDir, v, 'bin', 'devin'), `echo "devin ${v} (abc)"`);
    fs.writeFileSync(path.join(versionsDir, v, 'distribution'), 'curl-bash\n');
  }
  fs.symlinkSync(versions[versions.length - 1], path.join(versionsDir, 'current'));
  const devin = path.join(root, 'bin', 'devin');
  fs.mkdirSync(path.dirname(devin), { recursive: true });
  fs.symlinkSync(path.join(versionsDir, 'current', 'bin', 'devin'), devin);
  return { devin, versionsDir };
}

// A bundle for `version` as Devin ships one, and an exec whose curl copies
// it rather than downloading.
function bundle(root: string, version: string): { file: string; sha256: string; exec: Exec; downloads: string[] } {
  const src = path.join(root, `src-${version}`);
  script(path.join(src, 'bin', 'devin'), `echo "devin ${version} (def)"`);
  fs.mkdirSync(path.join(src, 'share', 'devin'), { recursive: true });
  const file = path.join(root, `devin-${version}.tar.gz`);
  execFileSync('/usr/bin/tar', ['czf', file, '-C', src, 'bin', 'share']);
  const sha256 = crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
  const downloads: string[] = [];
  const exec: Exec = async (command, args, opts) => {
    if (command === '/usr/bin/curl') {
      downloads.push(args[args.length - 1]);
      fs.copyFileSync(file, args[args.indexOf('-o') + 1]);
      return { code: 0, stdout: '', stderr: '' };
    }
    return defaultExec(command, args, opts);
  };
  return { file, sha256, exec, downloads };
}

function manifest(version: string, sha256: string): string {
  return JSON.stringify({ version, platforms: { [devinTarget()!]: { url: `${DEVIN_BASE_URL}/${version}/devin-${version}.tar.gz`, sha256 } } });
}

test('devin: the installer layout is found, and anything else is left alone', async () => {
  const root = tmpDir('tb-devin-');
  const { devin, versionsDir } = fakeDevin(root);
  assert.deepEqual(devinInstall(devin), { versionsDir: fs.realpathSync(versionsDir), version: '3000.11.205', distribution: 'curl-bash' });
  assert.equal(await devinInstalledVersion(devin, defaultExec), '3000.11.205');
  const loose = path.join(root, 'loose', 'devin');
  script(loose, 'echo "devin 3000.1.2 (x)"');
  assert.equal(devinInstall(loose), undefined);
  assert.equal(await devinInstalledVersion(loose, defaultExec), '3000.1.2');
  const r = await runDevinUpdate({ devinPath: loose, exec: defaultExec, fetchText: async () => manifest('3000.12.32', 'a'.repeat(64)), timeoutMs: 10_000, lookupMs: 1000 });
  assert.equal(r.kind, 'failed');
});

test('devin: an update checks the bundle, unpacks it beside the others and swaps current', async () => {
  const root = tmpDir('tb-devin-');
  const { devin, versionsDir } = fakeDevin(root);
  const b = bundle(root, '3000.12.32');
  const fetched: string[] = [];
  const fetchText = async (url: string) => {
    fetched.push(url);
    return manifest('3000.12.32', b.sha256);
  };
  const r = await runDevinUpdate({ devinPath: devin, exec: b.exec, fetchText, timeoutMs: 10_000, lookupMs: 1000 });
  assert.equal(r.kind, 'updated', r.kind === 'failed' ? r.message : '');
  assert.deepEqual(fetched, [`${DEVIN_BASE_URL}/current/manifest.json`]);
  assert.equal(fs.readlinkSync(path.join(versionsDir, 'current')), '3000.12.32');
  assert.equal(execFileSync(devin, ['--version'], { encoding: 'utf8' }).trim(), 'devin 3000.12.32 (def)');
  assert.equal(fs.readFileSync(path.join(versionsDir, '3000.12.32', 'distribution'), 'utf8'), 'curl-bash\n');
  // The old version stays for anything still running it; no leftovers.
  assert.ok(fs.existsSync(path.join(versionsDir, '3000.11.205', 'bin', 'devin')));
  assert.deepEqual(fs.readdirSync(versionsDir).sort(), ['3000.11.205', '3000.12.32', '_download', 'current']);
  assert.deepEqual(fs.readdirSync(path.join(versionsDir, '_download')), []);
  // Already there: nothing to do.
  const again = await runDevinUpdate({ devinPath: devin, exec: b.exec, fetchText, timeoutMs: 10_000, lookupMs: 1000 });
  assert.equal(again.kind, 'unchanged');
  // Going back to a kept version needs no download.
  const back = await runDevinUpdate({ devinPath: devin, exec: b.exec, fetchText: async () => manifest('3000.11.205', b.sha256), timeoutMs: 10_000, lookupMs: 1000, version: '3000.11.205' });
  assert.equal(back.kind === 'updated' && back.to, '3000.11.205');
  assert.equal(b.downloads.length, 1);
});

test("devin: a bundle whose checksum doesn't match is never installed", async () => {
  const root = tmpDir('tb-devin-');
  const { devin, versionsDir } = fakeDevin(root);
  const b = bundle(root, '3000.12.32');
  const r = await runDevinUpdate({ devinPath: devin, exec: b.exec, fetchText: async () => manifest('3000.12.32', 'b'.repeat(64)), timeoutMs: 10_000, lookupMs: 1000 });
  assert.equal(r.kind, 'failed');
  assert.match(r.kind === 'failed' ? r.message : '', /checksum/);
  assert.equal(fs.readlinkSync(path.join(versionsDir, 'current')), '3000.11.205');
  assert.ok(!fs.existsSync(path.join(versionsDir, '3000.12.32')));
});

test('devin: a manifest pointing anywhere but static.devin.ai is refused', async () => {
  const root = tmpDir('tb-devin-');
  const { devin } = fakeDevin(root);
  const b = bundle(root, '3000.12.32');
  const evil = JSON.stringify({ version: '3000.12.32', platforms: { [devinTarget()!]: { url: 'https://example.com/devin.tar.gz', sha256: b.sha256 } } });
  const r = await runDevinUpdate({ devinPath: devin, exec: b.exec, fetchText: async () => evil, timeoutMs: 10_000, lookupMs: 1000 });
  assert.equal(r.kind, 'failed');
  assert.equal(b.downloads.length, 0);
});

test('devin: cleanup keeps the current one and the newest before it, and any in use', () => {
  const root = tmpDir('tb-devin-');
  const { devin, versionsDir } = fakeDevin(root, ['3000.1.0', '3000.2.0', '3000.3.0', '3000.4.0']);
  const real = fs.realpathSync(versionsDir);
  const t = Date.now() / 1000;
  ['3000.1.0', '3000.2.0', '3000.3.0', '3000.4.0'].forEach((v, i) => fs.utimesSync(path.join(versionsDir, v), t - 100 + i, t - 100 + i));
  const removed = cleanupDevinVersions(devin, 2, `${path.join(real, '3000.1.0', 'bin', 'devin')} acp\n`);
  assert.deepEqual(removed, ['3000.2.0']);
  assert.deepEqual(fs.readdirSync(versionsDir).sort(), ['3000.1.0', '3000.3.0', '3000.4.0', 'current']);
});

test('the Updates list shows Grok and Devin only once they are installed', async () => {
  const root = tmpDir('tb-updates-gd-');
  const home = path.join(root, 'home');
  fs.mkdirSync(home, { recursive: true });
  const hooks = { restartPending: () => false, releaseIdle: () => undefined, versionsInUse: () => [], codexBinariesInUse: () => [] };
  const make = (cfg: Partial<ReturnType<typeof testConfig>>) =>
    new CliUpdates({ cfg: testConfig(home, cfg), log: nullLogger, clock: new FakeClock(), hooks, codex: { resolve: (s) => ({ command: s, native: false }), checkSurface: () => ({ missing: [], added: [] }), testedVersion: '0.1.0' }, env: { THREADBEAR_CLI_UPDATES: 'off' } });
  assert.deepEqual(make({}).list().map((c) => c.provider), ['claude', 'codex']);
  const grok = fakeGrok(root);
  const { devin } = fakeDevin(root);
  const u = make({ grokPath: grok, devinPath: devin });
  u.start();
  await u.settled();
  u.stop();
  const list = u.list();
  assert.deepEqual(list.map((c) => c.provider), ['claude', 'codex', 'grok', 'devin']);
  assert.deepEqual(list.slice(2).map((c) => [c.installedVersion, c.managedBy]), [['1.0.77', 'cli'], ['3000.11.205', 'threadbear']]);
});
