// Grok Build's ACP updates turned into protocol events (docs/grok-plan.md).
// Pure apart from the NormalizerContext, so it's tested by feeding it
// updates shaped like the spikes' traces.
//
// Tools are given the names the rest of Threadbear already knows (Bash,
// Edit, Write, Read, TodoWrite, mcp__<server>__<tool>), so their rows,
// titles and approval cards look like Claude's. A Grok tool with no such
// name keeps its own, with ACP's kind and title.
import type { EffortLevel, Hunk, PlanEntry, ToolKind } from '../../protocol.ts';
import { isEffortLevel } from '../../protocol.ts';
import type { Logger } from '../../log.ts';
import type { AdapterOutput, TurnResult } from '../types.ts';
import { truncateResultText, type NormalizerContext } from '../context.ts';
import { computeEditDiff, toolKind, toolTitle, truncateInput } from '../../tools.ts';
import { diffLines } from '../../diff.ts';
import type { SlashCommand } from '../../slash-commands.ts';

type AnyMsg = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any

// A Grok tool as the rest of Threadbear sees it.
export type MappedTool = { name: string; kind: ToolKind; input: Record<string, unknown> };

const ACP_KINDS: Record<string, ToolKind> = {
  execute: 'command',
  read: 'read',
  edit: 'edit',
  delete: 'edit',
  move: 'edit',
  search: 'search',
  fetch: 'web',
  think: 'todo',
};

function str(v: unknown): string {
  return typeof v === 'string' ? v : '';
}

// Grok's tool name and arguments to Threadbear's. `meta` is the tool's
// `_meta["x.ai/tool"]` (name, kind, namespace), when the update has it.
export function mapTool(rawName: string, rawInput: AnyMsg | undefined, meta?: AnyMsg): MappedTool {
  const input = rawInput && typeof rawInput === 'object' ? { ...rawInput } : {};
  delete input.variant;
  const name = str(meta?.name) || rawName;
  const ns = str(meta?.namespace);
  switch (name) {
    case 'run_terminal_command':
      return { name: 'Bash', kind: 'command', input: { command: str(input.command), ...(input.description ? { description: input.description } : {}), ...(input.is_background ? { run_in_background: true } : {}) } };
    case 'search_replace': {
      const file_path = str(input.file_path);
      // An empty old_string makes a new file, or replaces an empty one.
      if (str(input.old_string) === '') return { name: 'Write', kind: 'write', input: { file_path, content: str(input.new_string) } };
      return { name: 'Edit', kind: 'edit', input: { file_path, old_string: str(input.old_string), new_string: str(input.new_string), ...(input.replace_all ? { replace_all: true } : {}) } };
    }
    case 'write_file':
    case 'create_file':
      return { name: 'Write', kind: 'write', input: { file_path: str(input.file_path) || str(input.path), content: str(input.content) || str(input.contents) } };
    case 'read_file':
      return { name: 'Read', kind: 'read', input: { file_path: str(input.target_file) || str(input.file_path) || str(input.path), ...(input.offset != null ? { offset: input.offset } : {}), ...(input.limit != null ? { limit: input.limit } : {}) } };
    case 'todo_write':
      return { name: 'TodoWrite', kind: 'todo', input: { todos: Array.isArray(input.todos) ? input.todos : [] } };
    case 'search_tool':
      return { name: 'ToolSearch', kind: 'other', input: { query: str(input.query) } };
    case 'use_tool': {
      // An MCP tool reached through Grok's tool search: `<server>__<tool>`.
      const named = str(input.tool_name);
      const args = input.tool_input && typeof input.tool_input === 'object' ? (input.tool_input as Record<string, unknown>) : {};
      const at = named.indexOf('__');
      if (at > 0) {
        const full = `mcp__${named.slice(0, at)}__${named.slice(at + 2)}`;
        return { name: full, kind: toolKind(full), input: args };
      }
      return { name: named || 'use_tool', kind: 'mcp', input: args };
    }
  }
  // An MCP server's tool: its namespace is the server (grok_build is Grok's own).
  if (ns && ns !== 'grok_build' && ns !== 'builtin') {
    const full = `mcp__${ns}__${name}`;
    return { name: full, kind: toolKind(full), input };
  }
  const mcp = /^mcp__[^_]/.test(name) ? name : '';
  if (mcp) return { name: mcp, kind: toolKind(mcp), input };
  return { name, kind: ACP_KINDS[str(meta?.kind)] ?? 'other', input };
}

type OpenTool = {
  id: string;
  rawName: string;
  tool: MappedTool;
  title?: string; // ACP's own, for a tool with no name of ours
  called: boolean;
  diffed: boolean;
  streamed?: number; // how much of a command's output has gone out as deltas
  lastText?: string; // the newest text an update carried (Devin's output)
  exitCode?: number; // `_meta.terminal_exit.exit_code` (Devin's)
};

// A tool_call (or an update or permission request's toolCall) as the
// tool it is: Grok's name and input are in `_meta["x.ai/tool"]`.
export type ToolOf = (u: AnyMsg | undefined) => { rawName: string; tool: MappedTool };

export const grokToolOf: ToolOf = (u) => {
  const meta = u?._meta?.['x.ai/tool'] as AnyMsg | undefined;
  const rawName = str(meta?.name) || str(u?.title);
  return { rawName, tool: mapTool(rawName, (meta?.input as AnyMsg) ?? u?.rawInput, meta) };
};

// Devin's (devin acp, checked on 3000.12.32): the tool's name is
// `_meta["cognition.ai/inferenceToolName"]` (exec, read, write, edit, ...;
// MCP tools as mcp__<server>__<tool>, also in `cognition.ai/toolName`),
// and its input already uses Claude's names.
export const devinToolOf: ToolOf = (u) => {
  const meta = (u?._meta ?? {}) as AnyMsg;
  const rawName = str(meta['cognition.ai/toolName']) || str(meta['cognition.ai/inferenceToolName']) || str(u?.title);
  const input = u?.rawInput && typeof u.rawInput === 'object' ? { ...(u.rawInput as AnyMsg) } : {};
  const pick = (...keys: string[]) => Object.fromEntries(keys.filter((k) => input[k] !== undefined && input[k] !== null).map((k) => [k, input[k]]));
  switch (rawName) {
    case 'exec':
      return { rawName, tool: { name: 'Bash', kind: 'command', input: { command: str(input.command), ...pick('description'), ...(input.run_in_background || input.is_background ? { run_in_background: true } : {}) } } };
    case 'read':
      return { rawName, tool: { name: 'Read', kind: 'read', input: { file_path: str(input.file_path) || str(input.path), ...pick('offset', 'limit') } } };
    case 'write':
      return { rawName, tool: { name: 'Write', kind: 'write', input: { file_path: str(input.file_path) || str(input.path), content: str(input.content) } } };
    case 'edit':
      return { rawName, tool: { name: 'Edit', kind: 'edit', input: { file_path: str(input.file_path), old_string: str(input.old_string), new_string: str(input.new_string), ...(input.replace_all ? { replace_all: true } : {}) } } };
    case 'todo_write':
    case 'todowrite':
      return { rawName, tool: { name: 'TodoWrite', kind: 'todo', input: { todos: Array.isArray(input.todos) ? input.todos : [] } } };
  }
  if (/^mcp__[^_]/.test(rawName)) return { rawName, tool: { name: rawName, kind: toolKind(rawName), input } };
  return { rawName, tool: { name: rawName || 'tool', kind: ACP_KINDS[str(u?.kind)] ?? 'other', input } };
};

type OpenText = { itemId: string; type: 'assistant.text' | 'reasoning'; text: string };

// Effort ids Grok uses and the protocol's names for them.
function effortOf(v: unknown): EffortLevel | undefined {
  if (v === 'minimal') return 'low';
  return isEffortLevel(v) ? v : undefined;
}

export class GrokNormalizer {
  private ctx: NormalizerContext;
  private log?: Logger;
  private tools = new Map<string, OpenTool>();
  private text?: OpenText;
  private items = 0;
  private prefix: string;
  private model?: string;
  private effort?: EffortLevel;
  private sessionId?: string;
  contextWindow?: number;
  commands: SlashCommand[] = [];
  title?: string;
  private name: string;
  private toolOf: ToolOf;
  private terminalOnly: ReadonlySet<string>;

  // `name`, `toolOf` and `terminalOnly` make it another ACP agent's (Devin's).
  constructor(ctx: NormalizerContext, o: { log?: Logger; prefix?: string; name?: string; toolOf?: ToolOf; terminalOnly?: ReadonlySet<string> } = {}) {
    this.ctx = ctx;
    this.log = o.log;
    this.name = o.name ?? 'Grok';
    this.toolOf = o.toolOf ?? grokToolOf;
    this.terminalOnly = o.terminalOnly ?? TERMINAL_ONLY;
    // Item ids must be unique across the thread's life, resumes included.
    this.prefix = o.prefix ?? `g${Date.now().toString(36)}`;
  }

  setSession(id: string, model?: string): void {
    this.sessionId = id;
    if (model) this.model = model;
  }

  // One update: session/update's or _x.ai/session_notification's `update`.
  handle(update: AnyMsg | undefined): AdapterOutput[] {
    const out: AdapterOutput[] = [];
    if (!update || typeof update !== 'object') return out;
    const kind = str(update.sessionUpdate);
    switch (kind) {
      case 'agent_message_chunk':
        this.chunk(out, 'assistant.text', update.content);
        break;
      case 'agent_thought_chunk':
        this.chunk(out, 'reasoning', update.content);
        break;
      case 'tool_call':
        this.closeText(out);
        this.toolCall(out, update);
        break;
      case 'tool_call_update':
        this.toolUpdate(out, update);
        break;
      case 'plan':
        this.plan(out, update.entries);
        break;
      case 'available_commands_update':
        this.commands = commandsFrom(update.availableCommands, this.terminalOnly);
        break;
      case 'usage_update':
        // ACP's own context line (Devin): tokens in the window, its size.
        if (typeof update.used === 'number' && update.used > 0) out.push({ kind: 'context', contextTokens: update.used, ...(typeof update.size === 'number' ? { contextWindow: update.size } : this.contextWindow ? { contextWindow: this.contextWindow } : {}) });
        break;
      case 'session_info_update':
        if (typeof update.title === 'string' && update.title) this.title = update.title;
        break;
      case 'model_changed':
        this.settings(out, update.model_id, update.reasoning_effort);
        break;
      case 'response_completed':
        this.contextFrom(out, update.usage);
        break;
      default:
        break;
    }
    return out;
  }

  // ---- Text

  private chunk(out: AdapterOutput[], type: OpenText['type'], content: AnyMsg | undefined): void {
    const text = content?.type === 'text' ? str(content.text) : '';
    if (!text) return;
    if (this.text && this.text.type !== type) this.closeText(out);
    if (!this.text) this.text = { itemId: `${this.prefix}-${type === 'reasoning' ? 'r' : 'm'}${++this.items}`, type, text: '' };
    this.text.text += text;
    out.push({ kind: 'delta', delta: { type: type === 'assistant.text' ? 'assistant.text.delta' : 'reasoning.delta', itemId: this.text.itemId, text } });
  }

  private closeText(out: AdapterOutput[]): void {
    const t = this.text;
    this.text = undefined;
    if (!t || !t.text.trim()) return;
    out.push({ kind: 'event', event: t.type === 'assistant.text' ? { type: 'assistant.text', itemId: t.itemId, text: t.text } : { type: 'reasoning', itemId: t.itemId, text: t.text } });
  }

  // ---- Tools

  private toolCall(out: AdapterOutput[], u: AnyMsg): void {
    const id = str(u.toolCallId);
    if (!id) return;
    const { rawName, tool } = this.toolOf(u);
    const open: OpenTool = { id, rawName, tool, called: false, diffed: false };
    this.tools.set(id, open);
    this.emitCall(out, open);
    this.status(out, open, u);
  }

  private toolUpdate(out: AdapterOutput[], u: AnyMsg): void {
    const id = str(u.toolCallId);
    let open = this.tools.get(id);
    if (!open) {
      // An update for a call we never saw start (a resume mid-call).
      if (!id) return;
      open = { id, ...this.toolOf(u), called: false, diffed: false };
      this.tools.set(id, open);
    }
    if (typeof u.title === 'string' && u.title) open.title = u.title;
    const text = contentText(u.content);
    if (text) open.lastText = text;
    const exit = (u._meta as AnyMsg | undefined)?.terminal_exit?.exit_code;
    if (typeof exit === 'number') open.exitCode = exit;
    if (!open.called) this.emitCall(out, open);
    if (!open.diffed && Array.isArray(u.content)) this.diff(out, open, u.content as AnyMsg[], u.status === 'completed');
    if (u.status === 'in_progress' && open.tool.name === 'Bash') this.stream(out, open, contentText(u.content));
    this.status(out, open, u);
  }

  // What a permission request or a resolved call needs from the open call:
  // its tool.call goes out first.
  toolFor(id: string, toolCall?: AnyMsg): { itemId: string; tool: MappedTool; title: string; outs: AdapterOutput[] } {
    const outs: AdapterOutput[] = [];
    let open = this.tools.get(id);
    if (!open) {
      open = { id, ...this.toolOf(toolCall), called: false, diffed: false };
      if (typeof toolCall?.title === 'string') open.title = toolCall.title;
      this.tools.set(id, open);
    }
    if (!open.called) this.emitCall(outs, open);
    return { itemId: id, tool: open.tool, title: this.titleOf(open), outs };
  }

  private titleOf(open: OpenTool): string {
    const own = toolTitle(open.tool.name, open.tool.input, this.ctx.cwd);
    // toolTitle hands back the bare name for a tool it doesn't know.
    if (own !== open.tool.name || !open.title) return own;
    return open.title.replace(/`([^`]*)`/g, '$1');
  }

  private emitCall(out: AdapterOutput[], open: OpenTool): void {
    open.called = true;
    out.push({ kind: 'event', event: { type: 'tool.call', itemId: open.id, name: open.tool.name, kind: open.tool.kind, title: this.titleOf(open), input: truncateInput(open.tool.input) } });
  }

  private diff(out: AdapterOutput[], open: OpenTool, content: AnyMsg[], done: boolean): void {
    const diffs = content.filter((c) => c?.type === 'diff' && typeof c.path === 'string');
    if (!diffs.length) return;
    open.diffed = true;
    for (const d of diffs) {
      const path = str(d.path);
      let hunks: Hunk[] | undefined;
      // Before the edit runs (its approval, or the update just before it),
      // the file on disk is still the old one.
      if (!done && (open.tool.name === 'Edit' || open.tool.name === 'Write')) {
        hunks = computeEditDiff(open.tool.name, { ...open.tool.input, file_path: path }, this.ctx.readFile(path))?.hunks;
      }
      if (!hunks) hunks = offsetHunks(diffLines(str(d.oldText), str(d.newText)), d._meta);
      out.push({ kind: 'event', event: { type: 'edit.diff', itemId: open.id, path, hunks } });
    }
  }

  // A running command's output so far (each update has all of it): the new
  // part goes out as a delta.
  private stream(out: AdapterOutput[], open: OpenTool, text: string): void {
    const from = open.streamed ?? 0;
    if (text.length <= from) return;
    open.streamed = text.length;
    out.push({ kind: 'delta', delta: { type: 'tool.output.delta', itemId: open.id, text: text.slice(from) } });
  }

  private status(out: AdapterOutput[], open: OpenTool, u: AnyMsg): void {
    const status = str(u.status);
    if (status !== 'completed' && status !== 'failed') return;
    this.tools.delete(open.id);
    const raw = (u.rawOutput && typeof u.rawOutput === 'object' ? u.rawOutput : {}) as AnyMsg;
    let ok = status === 'completed';
    let text = '';
    if (raw.type === 'Bash') {
      // The command's own output; output_for_prompt (what the model sees)
      // starts with "exit: <code>", which a failure keeps.
      const failed = (typeof raw.exit_code === 'number' && raw.exit_code !== 0) || raw.timed_out === true;
      if (failed) ok = false;
      text = failed ? str(raw.output_for_prompt) : contentText(u.content) || str(raw.output_for_prompt).replace(/^exit: 0\n?/, '');
    } else if (raw.type === 'MCP') {
      text = mcpOutput(raw.output);
      if (raw.output && typeof raw.output === 'object' && !('OkayOutput' in raw.output)) ok = false;
    } else if (raw.type === 'ReadFile' && raw.FileContent && typeof raw.FileContent === 'object') {
      text = str(raw.FileContent.raw_output) || str(raw.FileContent.content);
    } else if (open.tool.name === 'Bash' && open.lastText !== undefined && !contentText(u.content)) {
      // Devin's: the output came with the updates while it ran, the exit
      // code in terminal_exit.
      text = open.lastText;
      if (open.exitCode !== undefined && open.exitCode !== 0) ok = false;
    } else {
      text = contentText(u.content) || outputForPrompt(raw);
      if (open.exitCode !== undefined && open.exitCode !== 0) ok = false;
    }
    if (!text && !ok) text = contentText(u.content) || 'Failed';
    out.push({ kind: 'event', event: { type: 'tool.result', itemId: open.id, ok, ...truncateResultText(text, open.id, this.ctx), images: [] } });
  }

  // ---- Plan, settings, context

  private plan(out: AdapterOutput[], entries: unknown): void {
    if (!Array.isArray(entries)) return;
    const list: PlanEntry[] = (entries as AnyMsg[])
      .filter((e) => typeof e?.content === 'string')
      .map((e) => ({ text: e.content, status: e.status === 'completed' ? 'completed' : e.status === 'in_progress' ? 'in_progress' : 'pending' }));
    out.push({ kind: 'event', event: { type: 'plan.updated', entries: list } });
  }

  private settings(out: AdapterOutput[], model: unknown, effort: unknown): void {
    if (typeof model === 'string' && model && model !== this.model && this.sessionId) {
      this.model = model;
      out.push({ kind: 'session', nativeSessionId: this.sessionId, model });
    }
    const level = effortOf(effort);
    if (level && level !== this.effort) {
      this.effort = level;
      out.push({ kind: 'effort', level });
    }
  }

  private contextFrom(out: AdapterOutput[], u: AnyMsg | undefined): void {
    if (!u || typeof u !== 'object') return;
    const n = (k: string) => (typeof u[k] === 'number' ? u[k] : 0);
    const tokens = n('input_tokens') + n('cache_read_input_tokens') + n('cache_creation_input_tokens') + n('output_tokens');
    if (!tokens) return;
    out.push({ kind: 'context', contextTokens: tokens, ...(this.contextWindow ? { contextWindow: this.contextWindow } : {}) });
  }

  // ---- Turn end

  // The prompt's answer: the turn's result, after anything still open is
  // closed. The adapter adds which messages it answered.
  finish(stop: { stopReason?: unknown; _meta?: AnyMsg } | undefined, durationMs: number, error?: string): AdapterOutput[] {
    const out: AdapterOutput[] = [];
    this.closeText(out);
    const reason = str(stop?.stopReason);
    const outcome: TurnResult['outcome'] = error ? 'failed' : reason === 'cancelled' ? 'interrupted' : reason === 'refusal' ? 'failed' : 'completed';
    out.push(...this.closeOpen(outcome === 'interrupted' ? 'Interrupted' : 'Not finished'));
    const r: TurnResult = { outcome, durationMs };
    const usage = turnUsage(stop?._meta?.usage ?? (stop as AnyMsg | undefined)?.usage);
    if (usage) r.usage = usage;
    if (error) {
      r.error = error;
      const kind = errorKind(error);
      if (kind) r.errorKind = kind;
    } else if (reason === 'refusal') r.error = `${this.name} refused to go on with this.`;
    out.push({ kind: 'result', result: r });
    return out;
  }

  // Calls still open when a turn ends or the process goes.
  closeOpen(reason: string): AdapterOutput[] {
    const out: AdapterOutput[] = [];
    this.closeText(out);
    for (const open of this.tools.values()) {
      if (!open.called) this.emitCall(out, open);
      out.push({ kind: 'event', event: { type: 'tool.result', itemId: open.id, ok: false, text: reason, truncated: false, images: [] } });
    }
    this.tools.clear();
    return out;
  }
}

// diffLines' hunks count from line 1 of the snippet; Grok says where the
// snippet sits in the file (`_meta.details[0].old_line`, or `_meta.old_line`).
function offsetHunks(hunks: Hunk[], meta: AnyMsg | undefined): Hunk[] {
  const d = Array.isArray(meta?.details) ? meta.details[0] : meta;
  const oldLine = typeof d?.old_line === 'number' ? d.old_line : 1;
  const newLine = typeof d?.new_line === 'number' ? d.new_line : oldLine;
  if (oldLine === 1 && newLine === 1) return hunks;
  return hunks.map((h) => ({ ...h, oldStart: h.oldStart + oldLine - 1, newStart: h.newStart + newLine - 1 }));
}

// An MCP call's rawOutput.output: {OkayOutput: text} or {ErrorOutput: ...}.
function mcpOutput(o: unknown): string {
  if (typeof o === 'string') return o;
  if (!o || typeof o !== 'object') return '';
  for (const v of Object.values(o as AnyMsg)) {
    if (typeof v === 'string') return v;
    if (v && typeof v === 'object') return contentText((v as AnyMsg).content) || JSON.stringify(v);
  }
  return '';
}

function contentText(content: unknown): string {
  if (!Array.isArray(content)) return '';
  return (content as AnyMsg[])
    .map((c) => (c?.type === 'content' && c.content?.type === 'text' ? str(c.content.text) : ''))
    .filter(Boolean)
    .join('\n');
}

// rawOutput's `tool_output_for_prompt`, one level down ({EditsApplied: {...}}).
function outputForPrompt(raw: AnyMsg): string {
  for (const v of Object.values(raw)) {
    if (v && typeof v === 'object' && typeof (v as AnyMsg).tool_output_for_prompt === 'string') return (v as AnyMsg).tool_output_for_prompt;
  }
  return '';
}

// The prompt answer's `_meta.usage` as turn.completed's usage, in Codex's
// shape (input counts cached reads too).
export function turnUsage(u: AnyMsg | undefined): Record<string, unknown> | undefined {
  if (!u || typeof u !== 'object') return undefined;
  const n = (k: string) => (typeof u[k] === 'number' ? u[k] : 0);
  if (!n('totalTokens') && !n('inputTokens') && !n('outputTokens')) return undefined;
  return {
    inputTokens: n('inputTokens'),
    cachedInputTokens: n('cachedReadTokens'),
    cacheWriteTokens: n('cacheCreationTokens'),
    outputTokens: n('outputTokens'),
    reasoningTokens: n('reasoningTokens'),
  };
}

export function errorKind(message: string): TurnResult['errorKind'] | undefined {
  const m = message.toLowerCase();
  if (/usage limit|rate limit|quota|too many requests|429/.test(m)) return 'usageLimit';
  if (/context (window|length)|too long|maximum context/.test(m)) return 'contextWindow';
  if (/unauthori[sz]ed|401|sign in|log ?in|authenticat|expired token/.test(m)) return 'auth';
  if (/econnrefused|enotfound|etimedout|network|connection (reset|refused|closed)|dns/.test(m)) return 'network';
  return undefined;
}

// available_commands_update's list as the composer's slash commands.
// Commands that only make sense in Grok's terminal app are left out.
const TERMINAL_ONLY = new Set(['model', 'effort', 'always-approve', 'hooks-trust', 'hooks-untrust', 'hooks-add', 'hooks-remove', 'feedback', 'dashboard', 'theme', 'density', 'settings', 'exit', 'quit']);

// Devin's: its login, modes (the thread's mode sets them) and terminal UI.
export const DEVIN_TERMINAL_ONLY: ReadonlySet<string> = new Set(['login', 'logout', 'billing', 'workspace', 'ask', 'plan', 'code', 'bypass', 'smart', 'mode', 'model', 'help', 'bug', 'mcp', 'share', 'rename', 'fast', 'exit', 'quit', 'theme', 'settings']);

export function commandsFrom(list: unknown, terminalOnly: ReadonlySet<string> = TERMINAL_ONLY): SlashCommand[] {
  if (!Array.isArray(list)) return [];
  const out: SlashCommand[] = [];
  for (const c of list as AnyMsg[]) {
    const name = str(c?.name);
    if (!name || terminalOnly.has(name)) continue;
    const cmd: SlashCommand = { name, source: 'cli' };
    if (str(c.description)) cmd.description = str(c.description);
    const hint = str(c.input?.hint);
    if (hint) cmd.argumentHint = hint;
    out.push(cmd);
  }
  return out;
}
