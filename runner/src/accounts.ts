import fs from 'node:fs';
import { acpAccounts } from './adapters/acp/agents.ts';
import { experimentOn } from './experiments.ts';
import path from 'node:path';
import { isEffortLevel, type Account, type EffortLevel, type ModelChoice } from './protocol.ts';
import type { UsageStore } from './usage.ts';
import type { Config } from './config.ts';
import { isDirectory } from './fsutil.ts';

// Reads only oauthAccount.emailAddress from a .claude.json. Tokens live in
// the Keychain and are never read; this file only names the login.
export function readLoginEmail(claudeJsonPath: string): string | undefined {
  try {
    const raw = fs.readFileSync(claudeJsonPath, 'utf8');
    const parsed = JSON.parse(raw) as { oauthAccount?: { emailAddress?: unknown } };
    const email = parsed.oauthAccount?.emailAddress;
    return typeof email === 'string' && email ? email : undefined;
  } catch {
    return undefined;
  }
}

// The model an account's own settings.json names ("claude-fable-5-1[1m]"),
// which the CLI uses for a session that names none. Only that one key is
// read. Absent, blank or not a string: the CLI's own default.
export function readSettingsModel(settingsPath: string): string | undefined {
  try {
    const parsed = JSON.parse(fs.readFileSync(settingsPath, 'utf8')) as { model?: unknown };
    return typeof parsed.model === 'string' && parsed.model.trim() ? parsed.model.trim() : undefined;
  } catch {
    return undefined;
  }
}

// "claude-fable-5-1[1m]" is the 1M-context form of "claude-fable-5-1".
export function stripModelSuffix(model: string): string {
  return model.replace(/\[[^\]]*\]$/, '');
}

// What the runner has learned about an account since it started: the
// CLI's model list (asked once), and the model a thread with none ran on.
export type LearnedAccountInfo = { models?: ModelChoice[]; defaultModel?: string };

// The full id a thread with no model runs on. A turn that showed it wins;
// otherwise the account's settings.json, turned into a full id through the
// CLI's model list when that's known. Nothing set: unknown, since the CLI's
// own default is only reported once it runs.
export function resolveDefaultModel(settingsModel: string | undefined, learned: LearnedAccountInfo | undefined): string | undefined {
  if (learned?.defaultModel) return learned.defaultModel;
  if (!settingsModel) return undefined;
  const row = learned?.models?.find((m) => m.id === settingsModel || m.model === settingsModel);
  return row?.model ?? stripModelSuffix(settingsModel);
}

// The top-level `key = value` lines of a Codex config.toml, before its first
// [table]. Only the named keys are kept; nothing else in the file is read
// into memory past this, and there's no TOML dependency for three strings.
export function readCodexConfig(configTomlPath: string): { model?: string; effort?: EffortLevel; credentialsStore?: string } {
  let text: string;
  try {
    text = fs.readFileSync(configTomlPath, 'utf8');
  } catch {
    return {};
  }
  const out: { model?: string; effort?: EffortLevel; credentialsStore?: string } = {};
  for (const line of text.split(/\r?\n/)) {
    if (/^\s*\[/.test(line)) break;
    const m = /^\s*([A-Za-z0-9_-]+)\s*=\s*(?:"((?:[^"\\]|\\.)*)"|'([^']*)')\s*(?:#.*)?$/.exec(line);
    if (!m) continue;
    const value = (m[2] ?? m[3] ?? '').trim();
    if (!value) continue;
    if (m[1] === 'model') out.model = value;
    else if (m[1] === 'model_reasoning_effort' && isEffortLevel(value)) out.effort = value;
    else if (m[1] === 'cli_auth_credentials_store') out.credentialsStore = value;
  }
  return out;
}

// A Codex home counts as a login when auth.json exists (checked, never
// opened: it holds the tokens) or config.toml keeps them in the Keychain.
export function isCodexLogin(dir: string): boolean {
  if (!isDirectory(dir)) return false;
  if (fs.existsSync(path.join(dir, 'auth.json'))) return true;
  const store = readCodexConfig(path.join(dir, 'config.toml')).credentialsStore;
  return store === 'keyring' || store === 'auto';
}

// A Grok Build home counts as a login when auth.json exists (checked,
// never opened, as Codex's).
export function isGrokLogin(dir: string): boolean {
  return isDirectory(dir) && fs.existsSync(path.join(dir, 'auth.json'));
}

export type DiscoveryConfig = Pick<Config, 'claudeMainDir' | 'claudeMainJson' | 'tokenStatsAccountsDir'> &
  Partial<Pick<Config, 'codexMainDir' | 'grokMainDir' | 'accountNotes' | 'accountsDir' | 'home' | 'cursorPath' | 'opencodePath'>> & {
    // Whether Grok logins are listed (the grok experiment), asked on every scan.
    grokOn?: () => boolean;
  };

// claude-main is ~/.claude (its .claude.json sits at ~/.claude.json);
// codex-main is ~/.codex. Token Stats' Accounts folder holds claude-N and
// codex-N, each its own CLAUDE_CONFIG_DIR or CODEX_HOME. With the grok
// experiment on, grok-main is ~/.grok and grok-N are GROK_HOMEs. Claude accounts
// come first, so the first account is Claude's.
export function discoverAccounts(cfg: DiscoveryConfig): Account[] {
  const out: Account[] = [];
  if (isDirectory(cfg.claudeMainDir)) {
    out.push(claudeAccount('claude-main', cfg.claudeMainDir, readLoginEmail(cfg.claudeMainJson) ?? readLoginEmail(path.join(cfg.claudeMainDir, '.claude.json'))));
  }
  // Token Stats' folder (Mike's app), then the data dir's own accounts
  // folder, which is how anyone else adds a login; the first with an id wins.
  const dirOf = new Map<string, string>();
  for (const root of [cfg.tokenStatsAccountsDir, cfg.accountsDir]) {
    if (!root) continue;
    let names: string[] = [];
    try {
      names = fs.readdirSync(root);
    } catch {
      names = [];
    }
    for (const n of names) if (!dirOf.has(n)) dirOf.set(n, path.join(root, n));
  }
  const numbered = (prefix: string) =>
    [...dirOf.keys()]
      .map((n) => new RegExp(`^${prefix}-(\\d+)$`).exec(n))
      .filter((m): m is RegExpExecArray => m !== null)
      .sort((a, b) => Number(a[1]) - Number(b[1]))
      .map((m) => ({ id: m[0], dir: dirOf.get(m[0])! }));
  for (const { id, dir } of numbered('claude')) {
    if (isDirectory(dir)) out.push(claudeAccount(id, dir, readLoginEmail(path.join(dir, '.claude.json'))));
  }
  const codex = cfg.codexMainDir ? [{ id: 'codex-main', dir: cfg.codexMainDir }] : [];
  for (const { id, dir } of [...codex, ...numbered('codex')]) {
    if (isCodexLogin(dir)) out.push(codexAccount(id, dir));
  }
  if (cfg.grokOn?.()) {
    const grok = cfg.grokMainDir ? [{ id: 'grok-main', dir: cfg.grokMainDir }] : [];
    for (const { id, dir } of [...grok, ...numbered('grok')]) {
      if (isGrokLogin(dir)) out.push({ id, provider: 'grok', title: providerTitle('grok', undefined, id), configDir: dir });
    }
  }
  if (cfg.home && experimentOn(cfg.home, 'acpAgents')) {
    out.push(...acpAccounts({ paths: { cursor: cfg.cursorPath, opencode: cfg.opencodePath } }));
  }
  for (const a of out) {
    const note = cfg.accountNotes?.[a.id];
    if (note) a.note = note;
  }
  return out;
}

function claudeAccount(id: string, configDir: string, email: string | undefined): Account {
  const account: Account = { id, provider: 'claude', title: email ?? id, configDir };
  if (email) account.email = email;
  const model = resolveDefaultModel(readSettingsModel(path.join(configDir, 'settings.json')), undefined);
  if (model) account.defaultModel = model;
  return account;
}

function codexAccount(id: string, configDir: string): Account {
  const account: Account = { id, provider: 'codex', title: codexTitle(undefined, id), configDir };
  const conf = readCodexConfig(path.join(configDir, 'config.toml'));
  if (conf.model) account.defaultModel = conf.model;
  if (conf.effort) account.defaultEffort = conf.effort;
  return account;
}

// "Codex · <email>", so an app that only shows titles can tell codex-main
// from a Claude login with the same address. Grok's the same.
function codexTitle(email: string | undefined, id: string): string {
  return `Codex · ${email ?? id}`;
}

function providerTitle(provider: Account['provider'], email: string | undefined, id: string): string {
  if (provider === 'codex') return codexTitle(email, id);
  if (provider === 'grok') return `Grok · ${email ?? id}`;
  return email ?? id;
}

// The model an account names for sessions that name none: Claude's
// settings.json, Codex's config.toml.
function settingsModel(account: Account): string | undefined {
  if (account.provider === 'grok') return undefined;
  if (account.provider === 'codex') return readCodexConfig(path.join(account.configDir, 'config.toml')).model;
  return readSettingsModel(path.join(account.configDir, 'settings.json'));
}

// What the registry needs from the UsageStore (usage.ts).
export type AccountUsageSource = Pick<UsageStore, 'usageFor' | 'identityFor'>;

export class AccountRegistry {
  private cfg: DiscoveryConfig;
  private usage?: AccountUsageSource;
  private cache: Account[] = [];
  private scanned = '';
  private at = 0;
  private learned = new Map<string, LearnedAccountInfo>();
  private listeners = new Set<(accounts: Account[]) => void>();

  constructor(cfg: DiscoveryConfig, usage?: AccountUsageSource) {
    this.cfg = cfg;
    this.usage = usage;
  }

  // Fired when a rescan finds a different list of accounts (a login added
  // or removed, a new email or default). Usage changes come from the
  // UsageStore's own onChange.
  onChange(fn: (accounts: Account[]) => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  // Re-scans at most every 10 seconds so a newly added login shows up
  // without restarting the runner. Usage is added fresh on every call.
  list(): Account[] {
    if (Date.now() - this.at > 10_000 || this.cache.length === 0) {
      const found = discoverAccounts(this.cfg);
      const key = JSON.stringify(found);
      const changed = this.scanned !== '' && key !== this.scanned;
      this.scanned = key;
      this.cache = found.map((a) => this.enrich(a));
      this.at = Date.now();
      if (changed) {
        const list = this.cache.map((a) => this.withUsage(a));
        for (const fn of this.listeners) fn(list);
      }
    }
    return this.cache.map((a) => this.withUsage(a));
  }

  // Keeps what a probe or a turn showed about an account. Returns whether
  // anything changed, so the runner can tell its clients.
  learn(id: string, info: LearnedAccountInfo): boolean {
    const have = this.learned.get(id) ?? {};
    const next: LearnedAccountInfo = { ...have };
    if (info.models && JSON.stringify(info.models) !== JSON.stringify(have.models)) next.models = info.models;
    if (info.defaultModel && info.defaultModel !== have.defaultModel) next.defaultModel = info.defaultModel;
    if (JSON.stringify(next) === JSON.stringify(have)) return false;
    this.learned.set(id, next);
    this.cache = this.cache.map((a) => (a.id === id ? this.enrich(a) : a));
    return true;
  }

  private enrich(account: Account): Account {
    const learned = this.learned.get(account.id);
    if (!learned) return account;
    const out: Account = { ...account };
    const model = resolveDefaultModel(settingsModel(account), learned);
    if (model) out.defaultModel = model;
    else delete out.defaultModel;
    if (learned.models) out.models = learned.models;
    return out;
  }

  // Email, plan and title from Token Stats or the account's last reading
  // (they beat .claude.json, which once held a stale address), and usage.
  private withUsage(account: Account): Account {
    if (!this.usage) return account;
    const out: Account = { ...account };
    const id = this.usage.identityFor(account);
    if (id.email) out.email = id.email;
    if (id.plan) out.plan = id.plan;
    out.title = out.provider === 'codex' || out.provider === 'grok' ? providerTitle(out.provider, out.email, out.id) : out.provider === 'claude' ? (out.email ?? out.id) : out.title;
    const usage = this.usage.usageFor(out);
    if (usage) out.usage = usage;
    return out;
  }

  get(id: string): Account | undefined {
    return this.list().find((a) => a.id === id);
  }
}
