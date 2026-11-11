// Grok threads: one `grok agent --no-leader stdio` per live thread, driven
// over ACP (process.ts, docs/grok-plan.md). The session turns the runner's
// start/send/interrupt into session/new or session/resume and
// session/prompt, answers Grok's permission requests from the thread's
// mode and approval cards, serves the threadbear tools over MCP
// (mcp-http.ts), and hands every update to the normaliser.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { Account, EffortLevel, ModelChoice, PermissionMode } from '../../protocol.ts';
import type { Logger } from '../../log.ts';
import type { ProcessRegistry } from '../../proc.ts';
import type { AdapterOutput, AgentAdapter, AgentSession, SessionSink, StartSessionOptions, TurnResult, UserInput } from '../types.ts';
import { SessionNotFoundError } from '../types.ts';
import { fileContext } from '../context.ts';
import { CodexTools, toolGuidance, type ToolDefinition } from '../codex/tools.ts';
import { SERVER_NAME } from '../../threadbear-mcp.ts';
import { runsAsCodeByName } from '../../code-writes.ts';
import { placePictures } from '../../pictures.ts';
import { truncateInput } from '../../tools.ts';
import type { SlashCommand } from '../../slash-commands.ts';
import type { McpHttpServer } from '../../mcp-http.ts';
import { GrokExitedError, GrokProcess, GrokRpcError, type GrokProcessOptions, type GrokTimings, type RequestId } from './process.ts';
import { GrokNormalizer, grokToolOf, type MappedTool, type ToolOf } from './normalize.ts';

// What differs between the ACP agents this adapter drives: Grok Build and
// the Devin CLI (adapters/devin, docs/devin-plan.md).
export type AcpDialect = {
  provider: 'grok' | 'devin';
  name: string; // in what the thread shows: "Grok", "Devin"
  binary: () => string;
  spawnArgs?: () => string[]; // undefined: `grok agent --no-leader <grokArgs> stdio`
  envFor?: GrokProcessOptions['envFor'];
  version?: () => string | undefined; // when initialize doesn't say
  // Grok picks a conversation up with session/resume; Devin only has
  // session/load, which replays it first (dropped: the thread has it).
  resume: 'session/resume' | 'session/load';
  // Threadbear's instructions: Grok takes them as session/new's
  // `_meta.rules`; Devin ignores that, so they lead the first prompt.
  rulesIn: 'meta' | 'prompt';
  effortConfigId: string;
  effortIn: (v: unknown) => EffortLevel | undefined;
  effortOut: (e: EffortLevel) => string;
  // The model rows, the one running and context windows, from initialize
  // and session/new (or session/load).
  modelsOf: (init: AnyMsg | undefined, res: AnyMsg | undefined) => { rows: ModelChoice[]; current?: string; windows: Map<string, number> };
  modelsNeedSession: boolean; // Devin's are only in a session's config options
  modeFor?: (m: PermissionMode) => string | undefined; // the agent's own mode (Devin's `mode` option)
  notFound: (e: GrokRpcError) => boolean;
  toolOf?: ToolOf;
  terminalOnly?: ReadonlySet<string>;
};

export function grokDialect(deps: Pick<GrokAdapterDeps, 'grokBinary'>): AcpDialect {
  return {
    provider: 'grok',
    name: 'Grok',
    binary: deps.grokBinary,
    resume: 'session/resume',
    rulesIn: 'meta',
    effortConfigId: 'reasoning_effort',
    effortIn: grokEffortIn,
    effortOut: grokEffortOut,
    modelsOf: (init, res) => {
      const models = (res?.models ?? (init?._meta as AnyMsg | undefined)?.modelState) as AnyMsg | undefined;
      return { rows: grokModelChoices(models?.availableModels), current: typeof models?.currentModelId === 'string' ? models.currentModelId : undefined, windows: contextWindows(models?.availableModels) };
    },
    modelsNeedSession: false,
    notFound: (e) => e.detailCode === 'FS_NOT_FOUND' || /not found|no such session/i.test(e.message),
    toolOf: grokToolOf,
  };
}

export type GrokAdapterDeps = {
  grokBinary: () => string;
  grokArgs?: string[];
  registry: ProcessRegistry;
  log: Logger;
  // The threadbear tools for a thread, served to its Grok over MCP.
  toolsFor?: (threadId: string) => ToolDefinition[];
  mcp?: McpHttpServer;
  // More instructions for a new thread with tools (an experiment's line).
  extraGuidance?: () => string;
  timings?: Partial<GrokTimings>;
  now?: () => number;
  // Another ACP agent (Devin); Grok's when unset.
  dialect?: AcpDialect;
};

type AnyMsg = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any

// Grok's model rows (initialize's `_meta.modelState`, or session/new's
// `models`) as the model picker's.
export function grokModelChoices(rows: unknown): ModelChoice[] {
  if (!Array.isArray(rows)) return [];
  const out: ModelChoice[] = [];
  for (const r of rows as AnyMsg[]) {
    const id = typeof r?.modelId === 'string' ? r.modelId : '';
    if (!id) continue;
    const row: ModelChoice = { id, model: id, title: typeof r.name === 'string' && r.name ? r.name : id };
    if (typeof r.description === 'string' && r.description) row.description = r.description;
    const meta = (r._meta ?? {}) as AnyMsg;
    if (meta.supportsReasoningEffort && Array.isArray(meta.reasoningEfforts)) {
      const levels = (meta.reasoningEfforts as AnyMsg[]).map((e) => grokEffortIn(e?.value ?? e?.id)).filter((x): x is EffortLevel => Boolean(x));
      // Lowest first, as the picker lists them.
      const order: EffortLevel[] = ['low', 'medium', 'high', 'xhigh'];
      if (levels.length) row.effortLevels = order.filter((l) => levels.includes(l));
      const def = (meta.reasoningEfforts as AnyMsg[]).find((e) => e?.default === true);
      const level = grokEffortIn(def?.value ?? def?.id ?? meta.reasoningEffort);
      if (level) row.defaultEffort = level;
    }
    out.push(row);
  }
  return out;
}

// Grok's effort ids and the protocol's.
export function grokEffortIn(v: unknown): EffortLevel | undefined {
  if (v === 'minimal' || v === 'low') return 'low';
  if (v === 'medium' || v === 'high' || v === 'xhigh') return v;
  return undefined;
}
export function grokEffortOut(e: EffortLevel): string {
  return e === 'max' || e === 'ultra' ? 'xhigh' : e;
}

// Context windows per model, from the same rows.
function contextWindows(rows: unknown): Map<string, number> {
  const out = new Map<string, number>();
  if (!Array.isArray(rows)) return out;
  for (const r of rows as AnyMsg[]) {
    const n = r?._meta?.totalContextTokens;
    if (typeof r?.modelId === 'string' && typeof n === 'number') out.set(r.modelId, n);
  }
  return out;
}

// The "Always allow" rule a Grok tool call matches, in Claude's syntax so
// the thread keeps it the same way: a command allows that command, an edit
// or a write allows all of them, anything else allows that tool.
export function ruleFor(tool: MappedTool): string {
  if (tool.name === 'Bash') return `Bash(${String(tool.input.command ?? '')})`;
  return tool.name;
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function processFor(deps: GrokAdapterDeps, d: AcpDialect, o: { account: Account; cwd: string; label: string; env?: Record<string, string> }): GrokProcess {
  return new GrokProcess({
    binary: d.binary(),
    args: deps.grokArgs,
    ...(d.spawnArgs ? { spawnArgs: d.spawnArgs() } : {}),
    ...(d.envFor ? { envFor: d.envFor } : {}),
    name: d.provider,
    knownVersion: d.version?.(),
    account: o.account,
    env: o.env,
    cwd: o.cwd,
    registry: deps.registry,
    log: deps.log,
    label: o.label,
    timings: deps.timings,
  });
}

export class GrokAdapter implements AgentAdapter {
  readonly provider: 'grok' | 'devin';
  readonly wakesAfterTasks = false;
  private deps: GrokAdapterDeps;
  private dialect: AcpDialect;
  private live = new Set<GrokProcess>();

  constructor(deps: GrokAdapterDeps) {
    this.deps = deps;
    this.dialect = deps.dialect ?? grokDialect(deps);
    this.provider = this.dialect.provider;
  }

  start(opts: StartSessionOptions, sink: SessionSink): AgentSession {
    if (opts.confined) throw new Error(`This thread runs in a sandbox, and ${this.dialect.name} can't, so it wasn't started. Move it to a Claude login.`);
    const s = new GrokSession(opts, sink, this.deps, (p, live) => (live ? this.live.add(p) : this.live.delete(p)));
    s.start();
    return s;
  }

  binariesInUse(): string[] {
    return this.live.size ? [this.dialect.binary()] : [];
  }

  // The models the account offers: Grok's initialize says, without a
  // session; Devin's come with a session, made in a temp folder and deleted.
  async models(account: Account): Promise<ModelChoice[]> {
    const d = this.dialect;
    const cwd = d.modelsNeedSession ? fs.mkdtempSync(path.join(os.tmpdir(), `tb-${d.provider}-models-`)) : os.tmpdir();
    const p = processFor(this.deps, d, { account, cwd, label: `models:${account.id}` });
    try {
      const init = (await p.start()) as AnyMsg;
      if (!d.modelsNeedSession) return d.modelsOf(init, undefined).rows;
      const res = await p.request<AnyMsg>('session/new', { cwd, mcpServers: [] });
      if (typeof res?.sessionId === 'string') await p.request('session/delete', { sessionId: res.sessionId }).catch(() => undefined);
      return d.modelsOf(init, res).rows;
    } finally {
      await p.close().catch(() => undefined);
      if (d.modelsNeedSession) fs.rmSync(cwd, { recursive: true, force: true });
    }
  }
}

type State = 'starting' | 'ready' | 'turn' | 'closing' | 'closed';

type ActiveTurn = {
  ids: string[];
  startedAt: number;
  interrupted: boolean;
  abort: AbortController; // its permission cards
};

export class GrokSession implements AgentSession {
  private opts: StartSessionOptions;
  private sink: SessionSink;
  private deps: GrokAdapterDeps;
  private d: AcpDialect;
  private track: (p: GrokProcess, live: boolean) => void;
  private now: () => number;
  private proc: GrokProcess;
  private normalizer: GrokNormalizer;
  private state: State = 'starting';
  private sessionId?: string;
  private turn?: ActiveTurn;
  private held: UserInput[] = [];
  private early: [string, unknown][] = [];
  private rules: Set<string>;
  private mode: PermissionMode;
  private model?: string; // what the thread asks for (undefined: Grok's default)
  private effort?: EffortLevel;
  private liveModel?: string;
  private liveEffort?: string;
  private liveMode?: string; // the agent's own (Devin's)
  // Devin's session/load replays the conversation; it's dropped.
  private replaying = false;
  // Instructions that lead the first prompt (Devin's rulesIn: 'prompt').
  private pendingRules?: string;
  private windows = new Map<string, number>();
  private tools?: CodexTools;
  private removeEndpoint?: () => void;
  // Threadbear tool calls Grok has started, oldest first, so an MCP call
  // can name the row its approval card belongs to.
  private mcpCalls: { id: string; tool: string }[] = [];
  private exitedCalled = false;
  private exitPromise: Promise<void>;
  private resolveExit!: () => void;

  constructor(opts: StartSessionOptions, sink: SessionSink, deps: GrokAdapterDeps, track: (p: GrokProcess, live: boolean) => void) {
    this.opts = opts;
    this.sink = sink;
    this.deps = deps;
    this.d = deps.dialect ?? grokDialect(deps);
    this.track = track;
    this.now = deps.now ?? Date.now;
    this.mode = opts.permissionMode;
    this.model = opts.model;
    this.effort = opts.effort;
    this.rules = new Set(opts.allowRules ?? []);
    this.exitPromise = new Promise((r) => (this.resolveExit = r));
    this.normalizer = new GrokNormalizer(fileContext(opts.cwd, opts.mediaDir, opts.blobsDir), {
      log: deps.log,
      name: this.d.name,
      toolOf: this.d.toolOf,
      terminalOnly: this.d.terminalOnly,
    });
    this.proc = processFor(deps, this.d, { account: opts.account, cwd: opts.cwd, label: opts.threadId, env: opts.env });
    this.proc.onNotification = (method, params) => this.notification(method, params);
    this.proc.onRequest = (id, method, params) => this.request(id, method, params);
    this.proc.onExit = (err) => this.exited(err);
  }

  get binaryPath(): string {
    return this.d.binary();
  }

  // ---- Start

  start(): void {
    this.track(this.proc, true);
    void this.boot().catch((e: unknown) => {
      if (this.state === 'closing' || this.state === 'closed') return void this.finish();
      this.deps.log.warn(`${this.d.provider} session did not start`, { threadId: this.opts.threadId, error: errText(e) });
      void this.finish(e instanceof Error ? e : new Error(String(e)), true);
    });
  }

  private async mcpServers(): Promise<unknown[]> {
    const defs = this.deps.toolsFor?.(this.opts.threadId) ?? [];
    if (!defs.length || !this.deps.mcp) return [];
    const tools = new CodexTools(defs, {
      requestApproval: (req) => this.sink.requestApproval(req),
      cwd: () => this.opts.cwd,
      mode: () => this.mode,
      rules: () => this.rules,
      addRules: (rules) => this.addRules(rules),
    });
    this.tools = tools;
    const ep = await this.deps.mcp.add({
      name: SERVER_NAME,
      tools: defs,
      call: async ({ name, arguments: args, signal }) => {
        const r = await tools.call({ namespace: SERVER_NAME, tool: name, arguments: args, callId: this.takeMcpCall(name) } as never, { fromChild: false, signal });
        const text = (r.contentItems ?? []).map((c) => (c as { text?: string }).text ?? '').join('\n');
        return { text, isError: !r.success };
      },
    });
    this.removeEndpoint = ep.remove;
    return [{ type: 'http', name: SERVER_NAME, url: ep.url, headers: [] }];
  }

  private async boot(): Promise<void> {
    const { opts } = this;
    const init = await this.proc.start();
    if (this.state !== 'starting') {
      await this.proc.close();
      return;
    }
    const mcpServers = await this.mcpServers();
    let res: AnyMsg;
    if (opts.resume) {
      this.sessionId = opts.sessionId;
      this.replaying = this.d.resume === 'session/load';
      try {
        res = await this.proc.request<AnyMsg>(this.d.resume, { sessionId: opts.sessionId, cwd: opts.cwd, mcpServers });
      } catch (e) {
        if (e instanceof GrokRpcError && this.d.notFound(e)) {
          throw new SessionNotFoundError(`The ${this.d.name} session to resume wasn't found (${e.message}).`);
        }
        throw e;
      } finally {
        this.replaying = false;
      }
    } else {
      const defs = this.deps.toolsFor?.(opts.threadId) ?? [];
      const rules = [mcpServers.length ? toolGuidance(defs) : '', mcpServers.length ? (this.deps.extraGuidance?.() ?? '') : '', opts.instructions ?? ''].filter(Boolean).join('\n\n');
      const meta = rules && this.d.rulesIn === 'meta' ? { _meta: { rules } } : {};
      if (rules && this.d.rulesIn === 'prompt') this.pendingRules = rules;
      res = await this.proc.request<AnyMsg>('session/new', { cwd: opts.cwd, mcpServers, ...meta });
      if (typeof res?.sessionId !== 'string') throw new Error(`${this.d.name} started a session without an id`);
      this.sessionId = res.sessionId;
    }
    if (this.state !== 'starting') return;
    const models = this.d.modelsOf(init as AnyMsg, res);
    this.windows = models.windows;
    this.liveModel = models.current;
    const effortOpt = Array.isArray(res?.configOptions) ? (res.configOptions as AnyMsg[]).find((o) => o?.id === this.d.effortConfigId) : undefined;
    if (typeof effortOpt?.currentValue === 'string') this.liveEffort = effortOpt.currentValue;
    const mode = res?.modes?.currentModeId;
    if (typeof mode === 'string') this.liveMode = mode;
    this.normalizer.setSession(this.sessionId!, this.liveModel);
    this.normalizer.contextWindow = this.liveModel ? this.windows.get(this.liveModel) : undefined;
    await this.applySettings();
    if (this.state !== 'starting') return;
    this.sink.output({ kind: 'session', nativeSessionId: this.sessionId!, ...(this.liveModel ? { model: this.liveModel } : {}) });
    if (this.proc.version) this.sink.output({ kind: 'cliVersion', version: this.proc.version });
    const level = this.d.effortIn(this.liveEffort);
    if (level) this.sink.output({ kind: 'effort', level });
    this.state = 'ready';
    const early = this.early;
    this.early = [];
    for (const [m, p] of early) this.notification(m, p);
    this.next();
  }

  // The model and effort the thread asks for, set on the session when they
  // differ from what it runs.
  private async applySettings(): Promise<void> {
    if (!this.sessionId) return;
    const model = this.model ?? this.opts.account.defaultModel;
    if (model && model !== this.liveModel) {
      try {
        await this.proc.request('session/set_config_option', { sessionId: this.sessionId, configId: 'model', value: model });
        this.liveModel = model;
        this.normalizer.setSession(this.sessionId, model);
        this.normalizer.contextWindow = this.windows.get(model) ?? this.normalizer.contextWindow;
      } catch (e) {
        this.sink.output({ kind: 'event', event: { type: 'system.note', text: `${this.d.name} didn't switch to ${model}: ${errText(e)}` } });
      }
    }
    const effort = this.effort ? this.d.effortOut(this.effort) : undefined;
    if (effort && effort !== this.liveEffort) {
      try {
        await this.proc.request('session/set_config_option', { sessionId: this.sessionId, configId: this.d.effortConfigId, value: effort });
        this.liveEffort = effort;
      } catch (e) {
        this.deps.log.info(`${this.d.provider} did not take the effort level`, { threadId: this.opts.threadId, effort, error: errText(e) });
      }
    }
    const mode = this.d.modeFor?.(this.mode);
    if (mode && mode !== this.liveMode) {
      try {
        await this.proc.request('session/set_config_option', { sessionId: this.sessionId, configId: 'mode', value: mode });
        this.liveMode = mode;
      } catch (e) {
        this.deps.log.info(`${this.d.provider} did not take the mode`, { threadId: this.opts.threadId, mode, error: errText(e) });
      }
    }
  }

  // ---- Grok's traffic

  private notification(method: string, params: unknown): void {
    if (method !== 'session/update' && method !== '_x.ai/session_notification') return;
    const p = (params ?? {}) as AnyMsg;
    if (this.replaying) return;
    if (!this.sessionId || this.state === 'starting') {
      this.early.push([method, params]);
      return;
    }
    // Another session's traffic (a subagent's) isn't this thread's rows.
    if (typeof p.sessionId === 'string' && p.sessionId !== this.sessionId) return;
    const update = p.update as AnyMsg | undefined;
    if (update?.sessionUpdate === 'tool_call') this.noteMcpCall(update);
    this.emit(this.normalizer.handle(update));
  }

  private emit(outs: AdapterOutput[]): void {
    for (const o of outs) this.sink.output(o);
  }

  private noteMcpCall(u: AnyMsg): void {
    if (typeof u.toolCallId !== 'string') return;
    const name = (this.d.toolOf ?? grokToolOf)(u).tool.name;
    const prefix = `mcp__${SERVER_NAME}__`;
    if (!name.startsWith(prefix)) return;
    this.mcpCalls.push({ id: u.toolCallId, tool: name.slice(prefix.length) });
    if (this.mcpCalls.length > 50) this.mcpCalls.shift();
  }

  private takeMcpCall(tool: string): string | undefined {
    const at = this.mcpCalls.findIndex((c) => c.tool === tool);
    if (at < 0) return undefined;
    return this.mcpCalls.splice(at, 1)[0].id;
  }

  private request(id: RequestId, method: string, params: unknown): void {
    if (method === 'session/request_permission') {
      void this.permission(id, (params ?? {}) as AnyMsg);
      return;
    }
    this.proc.respondError(id, -32601, `Threadbear doesn't handle ${method}.`);
  }

  // A tool the agent wants to run, answered from the thread's mode, its "Always
  // allow" rules, or the approval card.
  private async permission(id: RequestId, p: AnyMsg): Promise<void> {
    const options = (Array.isArray(p.options) ? p.options : []) as AnyMsg[];
    const pick = (kind: string) => options.find((o) => o?.kind === kind)?.optionId as string | undefined;
    const answer = (decision: 'allow' | 'deny' | 'cancel') => {
      const optionId = decision === 'allow' ? pick('allow_once') : decision === 'deny' ? pick('reject_once') : undefined;
      this.proc.respond(id, optionId ? { outcome: { outcome: 'selected', optionId } } : { outcome: { outcome: 'cancelled' } });
    };
    const call = (p.toolCall ?? {}) as AnyMsg;
    const { itemId, tool, title, outs } = this.normalizer.toolFor(String(call.toolCallId ?? ''), call);
    this.emit(outs);
    // The threadbear tools ask (or not) in the MCP handler, by Threadbear's rules.
    if (tool.name.startsWith(`mcp__${SERVER_NAME}__`)) return answer('allow');
    const file = typeof tool.input.file_path === 'string' ? path.resolve(this.opts.cwd, tool.input.file_path) : undefined;
    const asCode = file ? runsAsCodeByName(file) || Boolean(this.opts.runsAsCode?.(file)) : false;
    if (this.mode === 'full') return answer('allow');
    const rule = ruleFor(tool);
    if (!asCode && this.rules.has(rule)) return answer('allow');
    const signal = this.turn?.abort.signal ?? AbortSignal.timeout(24 * 3600_000);
    const ans = await this.sink.requestApproval({ toolUseId: itemId, name: tool.name, title, input: truncateInput(tool.input), canAlwaysAllow: !asCode, signal });
    if (this.state === 'closed') return;
    if (ans.decision === 'allowAlways' && !asCode) this.addRules([rule]);
    if (ans.mode && ans.mode !== this.mode) this.mode = ans.mode;
    answer(ans.decision === 'allow' || ans.decision === 'allowAlways' ? 'allow' : signal.aborted ? 'cancel' : 'deny');
  }

  private addRules(rules: string[]): void {
    for (const r of rules) this.rules.add(r);
    this.sink.output({ kind: 'allowRules', rules });
  }

  // ---- Turns

  send(input: UserInput): void {
    if (this.state === 'closing' || this.state === 'closed') {
      this.deps.log.warn(`a message reached a ${this.d.provider} session that has ended`, { threadId: this.opts.threadId });
      return;
    }
    this.held.push(input);
    this.next();
  }

  // Messages sent during a turn wait for it, then go together as one.
  private next(): void {
    if (this.state !== 'ready' || this.turn || !this.held.length) return;
    const inputs = this.held;
    this.held = [];
    const turn: ActiveTurn = { ids: inputs.map((m) => m.id).filter((x): x is string => Boolean(x)), startedAt: this.now(), interrupted: false, abort: new AbortController() };
    this.turn = turn;
    this.state = 'turn';
    void this.prompt(turn, inputs);
  }

  private async prompt(turn: ActiveTurn, inputs: UserInput[]): Promise<void> {
    let res: AnyMsg | undefined;
    let error: string | undefined;
    try {
      await this.applySettings();
      const blocks = promptBlocks(inputs);
      // Devin's instructions, once, ahead of the first message.
      if (this.pendingRules) blocks.unshift({ type: 'text', text: `<threadbear_instructions>\n${this.pendingRules}\n</threadbear_instructions>\n\n` });
      this.pendingRules = undefined;
      res = await this.proc.request<AnyMsg>('session/prompt', { sessionId: this.sessionId, prompt: blocks });
    } catch (e) {
      if (e instanceof GrokExitedError || this.state === 'closing' || this.state === 'closed') return; // the exit reports it
      error = errText(e);
    }
    if (this.turn !== turn) return;
    this.turn = undefined;
    turn.abort.abort();
    this.state = 'ready';
    const outs = this.normalizer.finish(turn.interrupted && !res ? { stopReason: 'cancelled' } : res, Math.max(0, this.now() - turn.startedAt), turn.interrupted ? undefined : error);
    for (const o of outs) {
      if (o.kind !== 'result') this.sink.output(o);
      else this.sink.output({ kind: 'result', result: this.answered(turn, o.result) });
    }
    this.next();
  }

  private answered(turn: ActiveTurn, base: TurnResult): TurnResult {
    const r: TurnResult = { ...base };
    if (turn.ids.length) r.answers = turn.ids[turn.ids.length - 1];
    r.answered = [...turn.ids];
    r.queuedTurns = this.held.length;
    return r;
  }

  async interrupt(): Promise<void> {
    if (this.state === 'closed' || this.state === 'closing') {
      if (!this.proc.alive) throw new Error(`The ${this.d.name} process has already exited.`);
      return;
    }
    const dropped = this.held.map((m) => m.id).filter((x): x is string => Boolean(x));
    this.held = [];
    if (dropped.length) this.sink.output({ kind: 'result', result: { outcome: 'interrupted', durationMs: 0, answers: dropped[0], answered: dropped, queuedTurns: 0 } });
    const turn = this.turn;
    if (!turn || !this.sessionId) return;
    turn.interrupted = true;
    turn.abort.abort();
    this.proc.notify('session/cancel', { sessionId: this.sessionId });
  }

  async setModel(model: string | undefined): Promise<void> {
    this.model = model;
    if (this.state === 'ready') await this.applySettings();
  }

  async setEffort(effort: EffortLevel | undefined): Promise<void> {
    this.effort = effort;
    if (this.state === 'ready') await this.applySettings();
  }

  async setPermissionMode(mode: PermissionMode): Promise<void> {
    this.mode = mode;
    if (this.state === 'ready' && this.d.modeFor) await this.applySettings();
  }

  async commands(): Promise<SlashCommand[]> {
    return this.normalizer.commands;
  }

  // ---- End

  async end(opts: { hard?: boolean } = {}): Promise<void> {
    if (this.state === 'closed') return this.exitPromise;
    this.state = 'closing';
    if (opts.hard) this.proc.kill();
    await this.finish();
  }

  kill(): void {
    if (this.state === 'closed') return;
    this.state = 'closing';
    this.proc.kill();
    this.exited();
  }

  private async finish(error?: Error, failedStart = false): Promise<void> {
    if (this.state !== 'closed') this.state = 'closing';
    this.proc.onExit = null;
    await this.proc.close().catch(() => undefined);
    this.exited(error, failedStart);
  }

  private exited(error?: Error, failedStart = false): void {
    if (this.exitedCalled) return;
    this.exitedCalled = true;
    this.state = 'closed';
    this.removeEndpoint?.();
    this.turn?.abort.abort();
    this.proc.onRequest = null;
    this.track(this.proc, false);
    this.emit(this.normalizer.closeOpen('Interrupted'));
    const turn = this.turn;
    this.turn = undefined;
    if (turn && error) {
      this.sink.output({ kind: 'result', result: this.answered(turn, { outcome: 'failed', durationMs: Math.max(0, this.now() - turn.startedAt), error: error.message }) });
    }
    if (error && !(error instanceof SessionNotFoundError) && !failedStart) {
      const tail = this.proc.stderrLines().slice(-5).join('\n');
      if (tail && !error.message.includes(tail)) error = new Error(`${error.message}\n${tail}`);
    }
    this.resolveExit();
    this.sink.exited(error);
  }
}

// A turn's messages as ACP prompt blocks: words, and images (read from the
// copies the runner made in media/) where their [Picture N] markers are.
export function promptBlocks(inputs: UserInput[]): AnyMsg[] {
  const blocks: AnyMsg[] = [];
  inputs.forEach((input, i) => {
    if (i > 0) blocks.push({ type: 'text', text: '\n\n' });
    const parts = placePictures(input.text, input.images);
    if (!parts.length) blocks.push({ type: 'text', text: input.text });
    for (const part of parts) {
      if ('text' in part) {
        if (part.text) blocks.push({ type: 'text', text: part.text });
        continue;
      }
      try {
        blocks.push({ type: 'image', mimeType: part.image.mediaType, data: fs.readFileSync(part.image.path).toString('base64') });
      } catch {
        blocks.push({ type: 'text', text: `[A picture that couldn't be read: ${part.image.path}]` });
      }
    }
  });
  return blocks;
}
