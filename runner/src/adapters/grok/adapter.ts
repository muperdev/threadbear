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
import { GrokExitedError, GrokProcess, GrokRpcError, type GrokTimings, type RequestId } from './process.ts';
import { GrokNormalizer, mapTool, type MappedTool } from './normalize.ts';

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
function grokEffortIn(v: unknown): EffortLevel | undefined {
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

export class GrokAdapter implements AgentAdapter {
  readonly provider = 'grok';
  readonly wakesAfterTasks = false;
  private deps: GrokAdapterDeps;
  private live = new Set<GrokProcess>();

  constructor(deps: GrokAdapterDeps) {
    this.deps = deps;
  }

  start(opts: StartSessionOptions, sink: SessionSink): AgentSession {
    if (opts.confined) throw new Error("This thread runs in a sandbox, and Grok can't, so it wasn't started. Move it to a Claude login.");
    const s = new GrokSession(opts, sink, this.deps, (p, live) => (live ? this.live.add(p) : this.live.delete(p)));
    s.start();
    return s;
  }

  binariesInUse(): string[] {
    return this.live.size ? [this.deps.grokBinary()] : [];
  }

  // The models the account's Grok offers: initialize says, without a session.
  async models(account: Account): Promise<ModelChoice[]> {
    const p = new GrokProcess({
      binary: this.deps.grokBinary(),
      args: this.deps.grokArgs,
      account,
      cwd: os.tmpdir(),
      registry: this.deps.registry,
      log: this.deps.log,
      label: `models:${account.id}`,
      timings: this.deps.timings,
    });
    try {
      const init = await p.start();
      return grokModelChoices((init?._meta as AnyMsg | undefined)?.modelState?.availableModels);
    } finally {
      await p.close().catch(() => undefined);
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
    this.track = track;
    this.now = deps.now ?? Date.now;
    this.mode = opts.permissionMode;
    this.model = opts.model;
    this.effort = opts.effort;
    this.rules = new Set(opts.allowRules ?? []);
    this.exitPromise = new Promise((r) => (this.resolveExit = r));
    this.normalizer = new GrokNormalizer(fileContext(opts.cwd, opts.mediaDir, opts.blobsDir), { log: deps.log });
    this.proc = new GrokProcess({
      binary: deps.grokBinary(),
      args: deps.grokArgs,
      account: opts.account,
      env: opts.env,
      cwd: opts.cwd,
      registry: deps.registry,
      log: deps.log,
      label: opts.threadId,
      timings: deps.timings,
    });
    this.proc.onNotification = (method, params) => this.notification(method, params);
    this.proc.onRequest = (id, method, params) => this.request(id, method, params);
    this.proc.onExit = (err) => this.exited(err);
  }

  get binaryPath(): string {
    return this.deps.grokBinary();
  }

  // ---- Start

  start(): void {
    this.track(this.proc, true);
    void this.boot().catch((e: unknown) => {
      if (this.state === 'closing' || this.state === 'closed') return void this.finish();
      this.deps.log.warn('grok session did not start', { threadId: this.opts.threadId, error: errText(e) });
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
      try {
        res = await this.proc.request<AnyMsg>('session/resume', { sessionId: opts.sessionId, cwd: opts.cwd, mcpServers });
      } catch (e) {
        if (e instanceof GrokRpcError && (e.detailCode === 'FS_NOT_FOUND' || /not found|no such session/i.test(e.message))) {
          throw new SessionNotFoundError(`The Grok session to resume wasn't found (${e.message}).`);
        }
        throw e;
      }
    } else {
      const defs = this.deps.toolsFor?.(opts.threadId) ?? [];
      const rules = [mcpServers.length ? toolGuidance(defs) : '', mcpServers.length ? (this.deps.extraGuidance?.() ?? '') : '', opts.instructions ?? ''].filter(Boolean).join('\n\n');
      res = await this.proc.request<AnyMsg>('session/new', { cwd: opts.cwd, mcpServers, ...(rules ? { _meta: { rules } } : {}) });
      if (typeof res?.sessionId !== 'string') throw new Error('Grok started a session without an id');
      this.sessionId = res.sessionId;
    }
    if (this.state !== 'starting') return;
    const models = (res?.models ?? (init?._meta as AnyMsg | undefined)?.modelState) as AnyMsg | undefined;
    this.windows = contextWindows(models?.availableModels);
    this.liveModel = typeof models?.currentModelId === 'string' ? models.currentModelId : undefined;
    const effortOpt = Array.isArray(res?.configOptions) ? (res.configOptions as AnyMsg[]).find((o) => o?.id === 'reasoning_effort') : undefined;
    if (typeof effortOpt?.currentValue === 'string') this.liveEffort = effortOpt.currentValue;
    this.normalizer.setSession(this.sessionId!, this.liveModel);
    this.normalizer.contextWindow = this.liveModel ? this.windows.get(this.liveModel) : undefined;
    await this.applySettings();
    if (this.state !== 'starting') return;
    this.sink.output({ kind: 'session', nativeSessionId: this.sessionId!, ...(this.liveModel ? { model: this.liveModel } : {}) });
    if (this.proc.version) this.sink.output({ kind: 'cliVersion', version: this.proc.version });
    const level = grokEffortIn(this.liveEffort);
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
        this.sink.output({ kind: 'event', event: { type: 'system.note', text: `Grok didn't switch to ${model}: ${errText(e)}` } });
      }
    }
    const effort = this.effort ? grokEffortOut(this.effort) : undefined;
    if (effort && effort !== this.liveEffort) {
      try {
        await this.proc.request('session/set_config_option', { sessionId: this.sessionId, configId: 'reasoning_effort', value: effort });
        this.liveEffort = effort;
      } catch (e) {
        this.deps.log.info('grok did not take the effort level', { threadId: this.opts.threadId, effort, error: errText(e) });
      }
    }
  }

  // ---- Grok's traffic

  private notification(method: string, params: unknown): void {
    if (method !== 'session/update' && method !== '_x.ai/session_notification') return;
    const p = (params ?? {}) as AnyMsg;
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
    const meta = u._meta?.['x.ai/tool'] as AnyMsg | undefined;
    const name = mapTool(String(meta?.name ?? u.title ?? ''), (meta?.input as AnyMsg) ?? u.rawInput, meta).name;
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

  // A tool Grok wants to run, answered from the thread's mode, its "Always
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
      this.deps.log.warn('a message reached a grok session that has ended', { threadId: this.opts.threadId });
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
      res = await this.proc.request<AnyMsg>('session/prompt', { sessionId: this.sessionId, prompt: promptBlocks(inputs) });
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
      if (!this.proc.alive) throw new Error('The Grok process has already exited.');
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
