// Devin threads (the devin experiment, docs/devin-plan.md): one `devin acp`
// per live thread. Devin speaks ACP as Grok does, so the Grok adapter
// drives it with Devin's dialect: its spawn and environment, session/load
// for a resume (its replay dropped), Threadbear's instructions ahead of the
// first prompt, models, effort (`thought_level`) and mode as session config
// options, and its tool names.
import fs from 'node:fs';
import type { Account, EffortLevel, ModelChoice, PermissionMode } from '../../protocol.ts';
import { cliEnv } from '../claude/adapter.ts';
import { GrokAdapter, type AcpDialect, type GrokAdapterDeps } from '../grok/adapter.ts';
import { DEVIN_TERMINAL_ONLY, devinToolOf } from '../grok/normalize.ts';
import { devinInstall } from '../../updates/devin.ts';

type AnyMsg = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any

export type DevinAdapterDeps = Omit<GrokAdapterDeps, 'grokBinary' | 'grokArgs' | 'dialect'> & {
  devinBinary: () => string;
  devinArgs?: string[];
};

// Variables an agent session sets for the commands it runs: a runner
// started from inside one would pass them on.
const INHERITED_VARS = ['GROK_HOME', 'GROK_SANDBOX', 'GROK_AGENT', 'GROK_AGENT_SECRET', 'CODEX_HOME', 'CODEX_THREAD_ID', 'CODEX_SANDBOX', 'DEVIN_MODEL', 'DEVIN_PERMISSION_MODE', 'DEVIN_SANDBOX'];

export function devinEnv(_account: Account, threadEnv: Record<string, string> | undefined, base: NodeJS.ProcessEnv = process.env): Record<string, string | undefined> {
  const env = cliEnv({ id: 'claude-main', provider: 'claude', title: '', configDir: '' }, base);
  delete env.CLAUDE_CONFIG_DIR;
  delete env.CLAUDE_AGENT_SDK_CLIENT_APP;
  for (const k of INHERITED_VARS) delete env[k];
  return { ...env, ...threadEnv };
}

// Devin's effort ids (`thought_level`: medium, high, max on SWE-2) and the
// protocol's.
export function devinEffortIn(v: unknown): EffortLevel | undefined {
  if (v === 'minimal' || v === 'low') return 'low';
  if (v === 'medium' || v === 'high' || v === 'xhigh' || v === 'max') return v;
  return undefined;
}
export function devinEffortOut(e: EffortLevel): string {
  return e === 'xhigh' || e === 'ultra' ? 'max' : e;
}

// Threadbear's modes as Devin's. Over ACP Devin offers Code (accept-edits:
// workspace edits run, commands ask), Smart (a fast model decides), Plan
// and Bypass; its "normal" mode, which asks before edits too, isn't one of
// them, so Ask before edits is Code.
export function devinMode(m: PermissionMode): string {
  if (m === 'full') return 'bypass';
  if (m === 'auto') return 'smart';
  if (m === 'plan') return 'plan';
  return 'accept-edits';
}

function configOption(res: AnyMsg | undefined, id: string): AnyMsg | undefined {
  return Array.isArray(res?.configOptions) ? (res.configOptions as AnyMsg[]).find((o) => o?.id === id) : undefined;
}

// The `model` config option's choices as the model picker's, each with
// the session's `thought_level` choices as its effort levels.
export function devinModels(res: AnyMsg | undefined): { rows: ModelChoice[]; current?: string; windows: Map<string, number> } {
  const model = configOption(res, 'model');
  const effort = configOption(res, 'thought_level');
  const order: EffortLevel[] = ['low', 'medium', 'high', 'xhigh', 'max'];
  const levels = Array.isArray(effort?.options) ? (effort.options as AnyMsg[]).map((o) => devinEffortIn(o?.value)).filter((x): x is EffortLevel => Boolean(x)) : [];
  const effortLevels = order.filter((l) => levels.includes(l));
  const rows: ModelChoice[] = [];
  for (const o of Array.isArray(model?.options) ? (model.options as AnyMsg[]) : []) {
    const id = typeof o?.value === 'string' ? o.value : '';
    if (!id) continue;
    const row: ModelChoice = { id, model: id, title: typeof o.name === 'string' && o.name ? o.name : id };
    if (typeof o.description === 'string' && o.description) row.description = o.description;
    if (effortLevels.length) row.effortLevels = effortLevels;
    rows.push(row);
  }
  return { rows, current: typeof model?.currentValue === 'string' ? model.currentValue : undefined, windows: new Map() };
}

export function devinDialect(deps: Pick<DevinAdapterDeps, 'devinBinary' | 'devinArgs'>): AcpDialect {
  // The version folder the link points at, so a running Devin keeps its
  // own files when an update moves `current` on (updates/devin.ts).
  const binary = () => {
    try {
      return fs.realpathSync(deps.devinBinary());
    } catch {
      return deps.devinBinary();
    }
  };
  return {
    provider: 'devin',
    name: 'Devin',
    binary,
    spawnArgs: () => ['acp', ...(deps.devinArgs ?? [])],
    envFor: (account, threadEnv) => devinEnv(account, threadEnv),
    version: () => devinInstall(deps.devinBinary())?.version,
    resume: 'session/load',
    rulesIn: 'prompt',
    effortConfigId: 'thought_level',
    effortIn: devinEffortIn,
    effortOut: devinEffortOut,
    modelsOf: (_init, res) => devinModels(res),
    modelsNeedSession: true,
    modeFor: devinMode,
    // -32016 "Session not found", data["cognition.ai/errorKind"]: session_not_found.
    notFound: (e) => (e.data as AnyMsg | undefined)?.['cognition.ai/errorKind'] === 'session_not_found' || e.code === -32016 || /session not found/i.test(e.message),
    toolOf: devinToolOf,
    terminalOnly: DEVIN_TERMINAL_ONLY,
  };
}

export function devinAdapter(deps: DevinAdapterDeps): GrokAdapter {
  const dialect = devinDialect(deps);
  return new GrokAdapter({ ...deps, grokBinary: dialect.binary, dialect });
}
