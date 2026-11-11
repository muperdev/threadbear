import { execFile, execFileSync } from 'node:child_process';

// git in a folder Threadbear hasn't trusted, or whose .git it didn't make.
// A folder from a zip, a tarball or a shared drive can carry its own
// .git/config, and `core.fsmonitor` there names a program git starts on
// anything that reads the working tree: `git ls-files`, `git status`,
// `git worktree add`. Settings given with -c beat the repo's own, so these
// turn it off (and the untracked cache, which feeds from it) whatever the
// folder says. Every git the runner runs in a thread's folder goes through
// these (docs/protocol.md, "Trusting a folder's patches").
export const UNTRUSTED_GIT_CONFIG: readonly string[] = ['-c', 'core.fsmonitor=false', '-c', 'core.untrackedCache=false'];

// The arguments for `git -C <dir> ...args` with those settings first.
export function untrustedGitArgs(dir: string, args: readonly string[]): string[] {
  return [...UNTRUSTED_GIT_CONFIG, '-C', dir, ...args];
}

// The runner's environment without any GIT_ variable: one set when the
// runner was started from inside a git hook (or by hand) could point git
// at another repo, or add config (GIT_CONFIG_COUNT, GIT_CONFIG_PARAMETERS)
// that comes after the -c settings above.
export function untrustedGitEnv(base: NodeJS.ProcessEnv = process.env): NodeJS.ProcessEnv {
  const env = { ...base };
  for (const k of Object.keys(env)) if (k.startsWith('GIT_')) delete env[k];
  return env;
}

// A listing never fetches or runs a hook. A folder's .git can also say
// it's a partial clone with objects missing (a sparse index whose tree
// isn't there), and git fetches a missing object by itself as it reads the
// index, through the repo's own core.sshCommand, a remote's uploadpack or
// an ext:: address: `git ls-files -s` ran a planted core.sshCommand with
// fsmonitor off (Codex's review of this fix, 9 Nov 2026). GIT_NO_LAZY_FETCH
// stops that in git 2.44 and later. For older git, every transport is
// refused too, each by name, as a protocol's own setting in the repo's
// config would beat protocol.allow. A listing missing an object leaves
// those files out, which is what Claude would see too.
const PROTOCOL_NAMES = ['ext', 'fd', 'file', 'git', 'ssh', 'http', 'https'];
function protocols(allowed: readonly string[]): string[] {
  return ['-c', 'protocol.allow=never', ...PROTOCOL_NAMES.flatMap((p) => ['-c', `protocol.${p}.allow=${allowed.includes(p) ? 'always' : 'never'}`])];
}
const NO_FETCH: readonly string[] = protocols([]);
export const LISTING_GIT_CONFIG: readonly string[] = [...UNTRUSTED_GIT_CONFIG, ...NO_FETCH, '-c', 'core.hooksPath=/dev/null'];

// A git listing in `dir`, read whole: what claude-trust and tracked-packages
// use to see what a folder holds before it's trusted. Throws as
// execFileSync does when git fails.
export function untrustedGitSync(dir: string, args: readonly string[]): string {
  return execFileSync('git', [...LISTING_GIT_CONFIG, '-C', dir, ...args], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
    timeout: 10_000,
    maxBuffer: 64 * 1024 * 1024,
    env: { ...untrustedGitEnv(), GIT_NO_LAZY_FETCH: '1' },
  });
}

// git that a repo's own config can't make run anything, for work beyond a
// listing in a folder Threadbear didn't vet: a move's git (move-git.ts)
// reads, hashes and checks out the working tree, makes commits and fetches
// (security review, 10 Nov 2026: a move's preview ran `git status` in a
// shared checkout, and a .git/config from an archive with a
// `filter.<name>.clean` command, selected by its .gitattributes, ran as
// the owner). On top of a listing's settings:
// - every filter driver named in any config git reads is switched off
//   (clean, smudge and process blank, required false): a blank command is
//   no filter, and the owner's own ones go too, as git-lfs's filter itself
//   runs commands a repo's config names (`lfs.extension.*`, custom
//   transfers). Files go across as the disk has them.
// - diff drivers' programs blank, for the same reason, though none of a
//   move's commands diff.
// - no commit signing (gpg.program is the repo's to name), no submodule
//   recursion (a submodule's own config isn't read here), no auto gc.
// - with `network`, for a fetch or a clone: only https, ssh and file, and
//   the programs a transport runs (credential helpers, core.sshCommand,
//   core.askPass, core.alternateRefsCommand) are the owner's own (system
//   or global config) whatever the repo's says. A remote's uploadpack is
//   the one setting where git takes the first it reads, not the last, so a
//   fetch or clone is given git's own upload-pack on its command line. A
//   local origin's upload-pack reads that repo's config, but git takes
//   its one program setting (uploadpack.packObjectsHook) only from the
//   owner's own, and it doesn't lazy fetch (GIT_NO_LAZY_FETCH).
// The config is read each time, just before the command, so what's
// switched off is what that command would find. A live command's git
// (shell-allowlist.ts liveCommandEnv) refuses a repo whose own config
// sets anything unusual instead; a move can't, as it has to work in any
// repo a thread is in (husky's core.hooksPath, a local git-lfs install),
// so it switches every such thing off.
export type UntrustedGitResult = { ok: boolean; code: number; stdout: string; stderr: string };
export type UntrustedGitOptions = {
  // Extra variables (GIT_INDEX_FILE, the author), set after the runner's
  // own GIT_ ones are dropped.
  env?: Record<string, string>;
  // 'bundle': a fetch from a bundle file (the file protocol only).
  // 'remote': a fetch or clone from a remote, see above.
  network?: 'bundle' | 'remote';
  timeoutMs?: number;
};

const TRUSTED_SCOPES = new Set(['system', 'global', 'unknown']); // 'unknown': Apple's own git config
const ALWAYS: readonly string[] = ['-c', 'safe.bareRepository=explicit', '-c', 'http.saveCookies=false', '-c', 'commit.gpgSign=false', '-c', 'tag.gpgSign=false', '-c', 'submodule.recurse=false', '-c', 'fetch.recurseSubmodules=false', '-c', 'gc.auto=0', '-c', 'maintenance.auto=false'];
// Transport programs a fetch or clone may run, with what stands in when the
// owner's config has none.
const TRANSPORT: { key: RegExp; fallback: string }[] = [
  { key: /^core\.sshcommand$/, fallback: 'ssh' },
  { key: /^core\.askpass$/, fallback: '' },
  { key: /^core\.alternaterefscommand$/, fallback: 'true' },
];

type ConfigEntry = { scope: string; key: string; value: string };

// `git config --list --show-scope -z`: scope NUL key LF value NUL, ...
export function parseConfigList(out: string): ConfigEntry[] {
  const parts = out.split('\0');
  const list: ConfigEntry[] = [];
  for (let i = 0; i + 1 < parts.length; i += 2) {
    const kv = parts[i + 1]!;
    const nl = kv.indexOf('\n');
    list.push({ scope: parts[i]!, key: nl < 0 ? kv : kv.slice(0, nl), value: nl < 0 ? '' : kv.slice(nl + 1) });
  }
  return list;
}

// A name git's -c can carry: it splits `key=value` at the first =.
function settable(key: string): string {
  if (key.includes('=') || key.includes('\n')) throw new Error(`This repo's git config has a setting Threadbear can't switch off (${JSON.stringify(key.slice(0, 80))}), so it left git alone.`);
  return key;
}

// The -c settings for `entries` (a repo's config as git reads it): see above.
export function repoProofConfig(entries: readonly ConfigEntry[], network?: 'bundle' | 'remote'): string[] {
  const out: string[] = [...UNTRUSTED_GIT_CONFIG, '-c', 'core.hooksPath=/dev/null', ...ALWAYS, ...protocols(network === 'remote' ? ['https', 'ssh', 'file'] : network === 'bundle' ? ['file'] : [])];
  const drivers = new Set<string>();
  const allowed = network === 'remote' ? ['https', 'ssh', 'file'] : network === 'bundle' ? ['file'] : [];
  for (const e of entries) {
    // A protocol the repo names beats protocol.allow too (ftp, a helper's
    // name for `foo::`), so each one it names is turned off by name.
    const p = /^protocol\.(.+)\.allow$/.exec(e.key);
    if (p && !allowed.includes(p[1]!) && !PROTOCOL_NAMES.includes(p[1]!)) out.push('-c', `${settable(e.key)}=never`);
    const f = /^filter\.(.+)\.[^.]+$/.exec(e.key);
    if (f) drivers.add(`filter.${f[1]}`);
    const d = /^diff\.(.+)\.(command|textconv)$/.exec(e.key);
    if (d) out.push('-c', `${settable(`diff.${d[1]}.${d[2]}`)}=`);
  }
  for (const d of drivers) out.push('-c', `${settable(`${d}.clean`)}=`, '-c', `${d}.smudge=`, '-c', `${d}.process=`, '-c', `${d}.required=false`);
  if (network === 'remote') {
    const fromRepo = (match: (k: string) => boolean) => entries.some((e) => match(e.key) && !TRUSTED_SCOPES.has(e.scope));
    const owners = (match: (k: string) => boolean) => entries.filter((e) => match(e.key) && TRUSTED_SCOPES.has(e.scope));
    const helper = (k: string) => /^credential\.(.+\.)?helper$/.test(k);
    if (fromRepo(helper)) {
      // An empty helper empties the list so far; then the owner's, in order.
      out.push('-c', 'credential.helper=');
      for (const e of owners(helper)) out.push('-c', `${settable(e.key)}=${e.value}`);
    }
    for (const t of TRANSPORT) {
      for (const key of new Set(entries.filter((e) => t.key.test(e.key) && !TRUSTED_SCOPES.has(e.scope)).map((e) => e.key))) {
        const own = owners((k) => k === key).pop();
        out.push('-c', `${settable(key)}=${own ? own.value : t.fallback}`);
      }
    }
  }
  return out;
}

function execGit(dir: string, args: readonly string[], env: NodeJS.ProcessEnv, timeoutMs: number): Promise<UntrustedGitResult> {
  return new Promise((resolve) => {
    execFile('git', args as string[], { cwd: dir, env, maxBuffer: 64 * 1024 * 1024, timeout: timeoutMs, killSignal: 'SIGKILL' }, (err, stdout, stderr) => {
      const e = err as { code?: unknown; signal?: unknown } | null;
      const code = !e ? 0 : typeof e.code === 'number' ? e.code : e.signal ? -1 : 1;
      resolve({ ok: !err, code, stdout: String(stdout), stderr: String(stderr) });
    });
  });
}

// `git -C dir ...args` as above. A config git can't read, or one with a
// setting -c can't reach, runs nothing and fails.
export async function untrustedGit(dir: string, args: readonly string[], opts: UntrustedGitOptions = {}): Promise<UntrustedGitResult> {
  // An empty GIT_ASKPASS stops git asking any program, core.askPass too.
  // No lazy fetch even for a fetch: a local origin's upload-pack inherits
  // it, and would otherwise fetch what it lacks through its own config.
  const env: NodeJS.ProcessEnv = { ...untrustedGitEnv(), GIT_TERMINAL_PROMPT: '0', GIT_ASKPASS: '', GIT_OPTIONAL_LOCKS: '0', GIT_NO_LAZY_FETCH: '1', ...opts.env };
  // A clone or fetch of a big repo can take a while.
  const timeout = opts.timeoutMs ?? (opts.network === 'remote' ? 30 * 60_000 : 5 * 60_000);
  const listed = await execGit(dir, [...LISTING_GIT_CONFIG, '-C', dir, 'config', '--list', '--show-scope', '-z'], { ...untrustedGitEnv(), GIT_NO_LAZY_FETCH: '1' }, 10_000);
  // Outside a repo it still lists the owner's own config (empty is 0 too);
  // a config git can't parse fails the listing, and so this.
  if (!listed.ok) return { ok: false, code: listed.code || 1, stdout: '', stderr: `couldn't read the repo's git config: ${listed.stderr.trim() || `git config exited ${listed.code}`}` };
  let config: string[];
  try {
    config = repoProofConfig(parseConfigList(listed.stdout), opts.network);
  } catch (e) {
    return { ok: false, code: 1, stdout: '', stderr: e instanceof Error ? e.message : String(e) };
  }
  const [sub, ...rest] = args;
  const pack = opts.network === 'remote' && (sub === 'fetch' || sub === 'clone' || sub === 'ls-remote') ? ['--upload-pack=git-upload-pack'] : [];
  return execGit(dir, [...config, '-C', dir, ...(sub ? [sub, ...pack, ...rest] : [])], env, timeout);
}
