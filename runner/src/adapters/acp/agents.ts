// The agents Threadbear runs over ACP, one row each: everything that differs
// between them. A new agent is a row here, a row in providers.ts and its
// line in docs/on-your-mac.md.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { Account, PermissionMode, Provider } from '../../protocol.ts';
import type { InitializeResult } from './connection.ts';

export type AcpProvider = Extract<Provider, 'cursor' | 'opencode'>;

export type AcpAgent = {
  provider: AcpProvider;
  accountId: string;
  title: string;
  // Where the agent keeps its login and settings. Never opened by Threadbear.
  configDir(home: string): string;
  // Whether it looks signed in, without reading what the login holds.
  loggedIn(home: string): boolean;
  // explicit: runner.json's path for it. Undefined when not installed.
  findBinary(o: { explicit?: string; home: string; pathEnv: string }): string | undefined;
  args: string[];
  // Runs the agent's own updater, when Update is pressed.
  updateArgs: string[];
  // Set on every process it starts. What stops a thread running tools
  // unguarded is here, so it applies whatever mode the thread starts in and
  // whatever it changes to later: the runner answers each request (AcpSession).
  // noTools: for a small job, where the agent may not use anything at all.
  env(o?: { noTools?: boolean }): Record<string, string>;
  // Whether the process that answered initialize really is this agent, as
  // the same command name can be another program's.
  identifies(init: InitializeResult): boolean;
  // The agent's own session mode for a thread's permission mode, or
  // undefined to leave it as it is.
  modeFor(mode: PermissionMode): string | undefined;
};

function isExecutableFile(file: string): boolean {
  try {
    fs.accessSync(file, fs.constants.X_OK);
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
}

function onPath(name: string, pathEnv: string): string | undefined {
  for (const dir of pathEnv.split(':')) {
    if (!path.isAbsolute(dir)) continue;
    const file = path.join(dir, name);
    if (isExecutableFile(file)) return file;
  }
  return undefined;
}

// Cursor's CLI installs as ~/.local/bin/agent, a link into
// ~/.local/share/cursor-agent/versions/<v>/cursor-agent. `agent` is a generic
// name: on Mike's Mac both ~/.local/bin/agent and the PATH's are Grok's. So a
// file counts only when where it really lives says Cursor.
export function isCursorBinary(file: string): boolean {
  let real: string;
  try {
    real = fs.realpathSync(file);
  } catch {
    return false;
  }
  return /(^|\/)cursor-agent(\/|$)/.test(real);
}

// Every tool asks unless it only reads, so a tool an agent adds later asks
// too. Read of something outside the folder asks as external_directory, and
// the threadbear tools have their own gate (tool-gate.ts).
const ASK_FOR_EVERYTHING: Record<string, string> = { '*': 'ask', read: 'allow', glob: 'allow', grep: 'allow', list: 'allow', external_directory: 'ask', 'threadbear_*': 'allow' };
const DENY_EVERYTHING: Record<string, string> = { '*': 'deny' };

// OpenCode runs anything without asking unless its config says otherwise,
// and a folder's own opencode.json outranks this inline config: tried on
// 1.18.23 (9 Nov 2026), a repo with `permission: allow` wrote files with no
// request. A repo's .opencode/plugins also run the moment a session opens.
// So the folder's OpenCode config isn't read at all, as a folder's Claude
// settings aren't until trusted. The rules go on the agents too: agent-level
// rules outrank top-level ones.
export function opencodeEnv(o: { noTools?: boolean } = {}): Record<string, string> {
  const permission = o.noTools ? DENY_EVERYTHING : ASK_FOR_EVERYTHING;
  return {
    OPENCODE_DISABLE_PROJECT_CONFIG: '1',
    OPENCODE_DISABLE_AUTOUPDATE: '1',
    OPENCODE_CONFIG_CONTENT: JSON.stringify({ permission, agent: { build: { permission }, plan: { permission }, general: { permission }, explore: { permission } } }),
  };
}

export const ACP_AGENTS: Record<AcpProvider, AcpAgent> = {
  opencode: {
    provider: 'opencode',
    accountId: 'opencode-main',
    title: 'OpenCode',
    configDir: (home) => path.join(home, '.config', 'opencode'),
    // Free models work with no login, so being installed is enough.
    loggedIn: () => true,
    findBinary({ explicit, home, pathEnv }) {
      // Whole paths only: a relative one would resolve against the thread's
      // folder when it's run, and that can be anyone's repo.
      if (explicit) return path.isAbsolute(explicit) && isExecutableFile(explicit) ? explicit : undefined;
      const own = path.join(home, '.opencode', 'bin', 'opencode');
      return isExecutableFile(own) ? own : onPath('opencode', pathEnv);
    },
    // --pure: no plugins, the user's own included, for the same reason.
    args: ['acp', '--pure'],
    updateArgs: ['upgrade'],
    env: opencodeEnv,
    identifies: (init) => /opencode/i.test(init.agentInfo?.name ?? '') || Boolean(init.authMethods?.some((m) => m.id === 'opencode-login')),
    modeFor: (mode) => (mode === 'plan' ? 'plan' : 'build'),
  },
  cursor: {
    provider: 'cursor',
    accountId: 'cursor-main',
    title: 'Cursor',
    configDir: (home) => path.join(home, '.cursor'),
    // Written once the CLI has been used; only whether it exists is read.
    loggedIn: (home) => fs.existsSync(path.join(home, '.cursor', 'cli-config.json')),
    findBinary({ explicit, home, pathEnv }) {
      if (explicit) return path.isAbsolute(explicit) && isExecutableFile(explicit) ? explicit : undefined;
      const candidates = [path.join(home, '.local', 'bin', 'cursor-agent'), onPath('cursor-agent', pathEnv), path.join(home, '.local', 'bin', 'agent'), onPath('agent', pathEnv)];
      return candidates.find((f): f is string => Boolean(f) && isExecutableFile(f!) && isCursorBinary(f!));
    },
    args: ['acp'],
    updateArgs: ['update'],
    // Nothing to set: the mode does the job. See modeFor.
    env: () => ({}),
    identifies: (init) => Boolean(init.authMethods?.some((m) => m.id === 'cursor_login')) || /cursor/i.test(init.agentInfo?.name ?? ''),
    // Cursor's own allow-list and a folder's .cursor/cli.json can approve
    // things without a request, and the docs give no way to turn that off.
    // Its ask and plan modes are read-only, so a thread that has to ask
    // runs in one of those: it can't change anything, card or no card.
    modeFor: (mode) => (mode === 'full' ? 'agent' : mode === 'plan' ? 'plan' : 'ask'),
  },
};

export const ACP_PROVIDERS = Object.keys(ACP_AGENTS) as AcpProvider[];

// The logins an ACP agent gives: at most one each, as these CLIs have one
// per machine.
export function acpAccounts(o: { paths?: Partial<Record<AcpProvider, string>>; home?: string; pathEnv?: string } = {}): Account[] {
  const home = o.home ?? os.homedir();
  const pathEnv = o.pathEnv ?? process.env.PATH ?? '';
  const out: Account[] = [];
  for (const p of ACP_PROVIDERS) {
    const agent = ACP_AGENTS[p];
    if (!agent.findBinary({ explicit: o.paths?.[p], home, pathEnv }) || !agent.loggedIn(home)) continue;
    out.push({ id: agent.accountId, provider: p, title: agent.title, configDir: agent.configDir(home) });
  }
  return out;
}
