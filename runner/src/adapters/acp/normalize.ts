// Turns ACP's session/update notifications into the runner's events. Pure
// apart from NormalizerContext, so recorded agent output can be fed straight
// through it in tests.
import crypto from 'node:crypto';
import path from 'node:path';
import type { ImageRef, PlanEntry, ToolKind } from '../../protocol.ts';
import { diffLines } from '../../diff.ts';
import { displayPath, truncateInput } from '../../tools.ts';
import { firstLineSafely } from '../../safe-cut.ts';
import { truncateResultText, type NormalizerContext } from '../context.ts';
import type { AdapterOutput } from '../types.ts';

export type AcpToolCall = {
  toolCallId?: string;
  title?: string;
  name?: string;
  kind?: string;
  status?: string;
  locations?: { path?: string }[];
  rawInput?: unknown;
  rawOutput?: unknown;
  content?: AcpToolContent[];
};

type AcpBlock = { type?: string; text?: string; data?: string; mimeType?: string };
type AcpToolContent = { type?: string; content?: AcpBlock; path?: string; newText?: string; oldText?: string | null };

type Tool = {
  id: string;
  emitted: boolean;
  done: boolean;
  acpKind?: string;
  name?: string;
  title?: string;
  input: Record<string, unknown>;
  paths: string[];
  output: AcpToolContent[];
  rawOutput?: unknown;
  diffed: Set<string>;
};

function toolKind(acp: string | undefined): ToolKind {
  switch (acp) {
    case 'read':
      return 'read';
    case 'edit':
    case 'delete':
    case 'move':
      return 'edit';
    case 'search':
      return 'search';
    case 'execute':
      return 'command';
    case 'fetch':
      return 'web';
    default:
      return 'other';
  }
}

// Every file a permission request names, from what the agent says it touches
// and from the tool's own arguments, as one could say less than the other.
// A check takes all of them: more names only makes it stricter.
const PATH_KEY = /path|file|dest|target|document|^to$|^from$/i;
// The files a patch body names: unified diffs and the `*** Update File:` kind.
const PATCH_HEADER = /^(?:\*\*\* (?:Add|Update|Delete) File: |\*\*\* Move to: |\+\+\+ |--- |Index: )(.+)$/gm;

function pathsIn(v: unknown, key: string, out: Set<string>, depth: number): void {
  if (depth > 8) return;
  if (typeof v === 'string') {
    if (v && PATH_KEY.test(key)) out.add(v);
    if (v.includes('\n')) {
      for (const m of v.matchAll(PATCH_HEADER)) {
        const p = m[1].trim().replace(/^[ab]\//, '').split('\t')[0];
        if (p && p !== '/dev/null') out.add(p);
      }
    }
  } else if (Array.isArray(v)) {
    for (const x of v) pathsIn(x, key, out, depth + 1);
  } else if (isPlainObject(v)) {
    for (const [k, x] of Object.entries(v)) pathsIn(x, k, out, depth + 1);
  }
}

export function requestPaths(call: AcpToolCall): string[] {
  const out = new Set<string>();
  for (const l of Array.isArray(call.locations) ? call.locations : []) if (typeof l?.path === 'string' && l.path) out.add(l.path);
  for (const c of Array.isArray(call.content) ? call.content : []) if (c?.type === 'diff' && typeof c.path === 'string' && c.path) out.add(c.path);
  pathsIn(call.rawInput, '', out, 0);
  return [...out];
}

// The arguments of the plain file tools the agents have, where what's named
// is all there is. Any other shape might write somewhere nobody has found:
// such a call can't be covered by "Always allow".
const PLAIN_EDIT_KEYS = new Set(['filepath', 'file_path', 'path', 'content', 'oldstring', 'newstring', 'old_string', 'new_string', 'diff', 'replaceall', 'replace_all', 'description']);

export function isPlainEdit(input: Record<string, unknown>): boolean {
  return Object.entries(input).every(([k, v]) => PLAIN_EDIT_KEYS.has(k.toLowerCase()) && (typeof v === 'string' || typeof v === 'boolean'));
}

// Kinds whose paths say what a call will change.
export function writesFiles(kind: string | undefined): boolean {
  return kind === 'edit' || kind === 'delete' || kind === 'move';
}

export function isPlainObject(v: unknown): v is Record<string, unknown> {
  return Boolean(v) && typeof v === 'object' && !Array.isArray(v);
}

const firstLine = (s: string): string => firstLineSafely(s, 200, '…');

// The threadbear tools reach the agent as an MCP server; called
// `threadbear_end_turn` there, and mcp__threadbear__end_turn everywhere else.
export function canonicalToolName(name: string): string {
  const m = /^threadbear[_:.-]+(.+)$/.exec(name);
  return m ? `mcp__threadbear__${m[1]}` : name;
}

export class AcpNormalizer {
  private ctx: NormalizerContext;
  private tools = new Map<string, Tool>();
  private text?: { messageId?: string; itemId: string; text: string };
  private thought?: { messageId?: string; itemId: string; text: string };
  private lastAssistant = '';
  costUsd?: number;

  constructor(ctx: NormalizerContext) {
    this.ctx = ctx;
  }

  // The last reply's words, for a Plan first thread's plan card.
  takeLastAssistantText(): string {
    const t = this.lastAssistant;
    this.lastAssistant = '';
    return t;
  }

  handle(update: unknown): AdapterOutput[] {
    if (!isPlainObject(update)) return [];
    const out: AdapterOutput[] = [];
    switch (update.sessionUpdate) {
      case 'agent_message_chunk':
        this.chunk(update, 'text', out);
        break;
      case 'agent_thought_chunk':
        this.chunk(update, 'thought', out);
        break;
      case 'tool_call':
      case 'tool_call_update':
        this.flush(out);
        this.toolUpdate(update as AcpToolCall, out);
        break;
      case 'plan':
        this.flush(out);
        if (Array.isArray(update.entries)) out.push({ kind: 'event', event: { type: 'plan.updated', entries: planEntries(update.entries) } });
        break;
      case 'usage_update':
        if (typeof update.used === 'number') out.push({ kind: 'context', contextTokens: update.used, ...(typeof update.size === 'number' ? { contextWindow: update.size } : {}) });
        if (isPlainObject(update.cost) && update.cost.currency === 'USD' && typeof update.cost.amount === 'number') this.costUsd = update.cost.amount;
        break;
      // user_message_chunk, modes, commands and anything newer: nothing to show.
    }
    return out;
  }

  private chunk(update: Record<string, unknown>, which: 'text' | 'thought', out: AdapterOutput[]): void {
    const block = (isPlainObject(update.content) ? update.content : {}) as AcpBlock;
    if (block.type !== 'text' || typeof block.text !== 'string' || !block.text) return;
    const messageId = typeof update.messageId === 'string' ? update.messageId : undefined;
    const open = which === 'text' ? this.text : this.thought;
    if (which === 'text') this.flushThought(out);
    else this.flushText(out);
    let current = open;
    if (!current || (messageId && current.messageId && current.messageId !== messageId)) {
      if (which === 'text') this.flushText(out);
      else this.flushThought(out);
      current = { messageId, itemId: crypto.randomUUID(), text: '' };
      if (which === 'text') this.text = current;
      else this.thought = current;
    }
    current.text += block.text;
    out.push({ kind: 'delta', delta: { type: which === 'text' ? 'assistant.text.delta' : 'reasoning.delta', itemId: current.itemId, text: block.text } });
  }

  private flushText(out: AdapterOutput[]): void {
    const t = this.text;
    this.text = undefined;
    if (!t?.text) return;
    this.lastAssistant = t.text;
    out.push({ kind: 'event', event: { type: 'assistant.text', itemId: t.itemId, text: t.text } });
  }

  private flushThought(out: AdapterOutput[]): void {
    const t = this.thought;
    this.thought = undefined;
    if (t?.text) out.push({ kind: 'event', event: { type: 'reasoning', itemId: t.itemId, text: t.text } });
  }

  // Closes what's being written, before a tool row or the end of a turn.
  flush(out: AdapterOutput[] = []): AdapterOutput[] {
    this.flushThought(out);
    this.flushText(out);
    return out;
  }

  private tool(id: string): Tool {
    let t = this.tools.get(id);
    if (!t) {
      t = { id, emitted: false, done: false, input: {}, paths: [], output: [], diffed: new Set() };
      this.tools.set(id, t);
      if (this.tools.size > 500) this.tools.delete(this.tools.keys().next().value!);
    }
    return t;
  }

  private merge(t: Tool, u: AcpToolCall): void {
    if (typeof u.kind === 'string') t.acpKind = u.kind;
    if (typeof u.name === 'string' && u.name) t.name = u.name;
    if (typeof u.title === 'string' && u.title) t.title = u.title;
    if (isPlainObject(u.rawInput) && Object.keys(u.rawInput).length) t.input = u.rawInput;
    // Everything this call has ever named, however it was named.
    for (const p of requestPaths(u)) if (!t.paths.includes(p)) t.paths.push(p);
    if (Array.isArray(u.content)) t.output = u.content;
    if (u.rawOutput !== undefined) t.rawOutput = u.rawOutput;
  }

  // A permission request carries the call as the agent sees it then: a row
  // for it comes before the card, which names the row.
  noteCall(call: AcpToolCall): { paths: string[]; kind?: string; input: Record<string, unknown>; title?: string; events: AdapterOutput[] } {
    const id = typeof call.toolCallId === 'string' && call.toolCallId ? call.toolCallId : crypto.randomUUID();
    const t = this.tool(id);
    this.merge(t, call);
    const events: AdapterOutput[] = [];
    this.emitCall(t, events);
    this.emitDiffs(t, events);
    return { paths: [...t.paths], kind: t.acpKind, input: t.input, title: t.title, events };
  }

  private toolUpdate(u: AcpToolCall, out: AdapterOutput[]): void {
    if (typeof u.toolCallId !== 'string' || !u.toolCallId) return;
    const t = this.tool(u.toolCallId);
    this.merge(t, u);
    // Agents can open a call with nothing filled in yet (OpenCode: title
    // "write", rawInput {}), so the row waits for its input or its start.
    const under = u.status === 'in_progress' || u.status === 'completed' || u.status === 'failed';
    if (Object.keys(t.input).length || under) this.emitCall(t, out);
    if (t.emitted) this.emitDiffs(t, out);
    if ((u.status === 'completed' || u.status === 'failed') && !t.done) this.emitResult(t, u.status === 'completed', out);
  }

  private title(t: Tool): string {
    const command = typeof t.input.command === 'string' ? t.input.command : '';
    if (t.acpKind === 'execute' && command) return firstLine(command);
    if (writesFiles(t.acpKind) && t.paths.length === 1) return `Edit ${displayPath(t.paths[0], this.ctx.cwd)}`;
    return firstLine(t.title || t.name || t.acpKind || 'Tool');
  }

  private emitCall(t: Tool, out: AdapterOutput[]): void {
    if (t.emitted) return;
    t.emitted = true;
    const name = canonicalToolName(t.name || t.title || t.acpKind || 'tool');
    const kind = name.startsWith('mcp__') ? 'mcp' : toolKind(t.acpKind);
    out.push({ kind: 'event', event: { type: 'tool.call', itemId: t.id, name, kind, title: this.title(t), input: truncateInput(t.input) } });
  }

  private emitDiffs(t: Tool, out: AdapterOutput[]): void {
    for (const c of t.output) {
      if (c?.type !== 'diff' || typeof c.path !== 'string' || typeof c.newText !== 'string' || t.diffed.has(c.path)) continue;
      t.diffed.add(c.path);
      out.push({ kind: 'event', event: { type: 'edit.diff', itemId: t.id, path: c.path, hunks: diffLines(typeof c.oldText === 'string' ? c.oldText : '', c.newText) } });
    }
  }

  private emitResult(t: Tool, ok: boolean, out: AdapterOutput[]): void {
    t.done = true;
    const texts: string[] = [];
    const images: ImageRef[] = [];
    for (const c of t.output) {
      if (c?.type !== 'content' || !c.content) continue;
      if (c.content.type === 'text' && typeof c.content.text === 'string') texts.push(c.content.text);
      else if (c.content.type === 'image' && typeof c.content.data === 'string' && typeof c.content.mimeType === 'string') {
        try {
          images.push(this.ctx.saveImage(c.content.data, c.content.mimeType, t.id));
        } catch {
          // Not an image this can save: the row still shows its text.
        }
      }
    }
    if (!texts.length && isPlainObject(t.rawOutput)) {
      const said = t.rawOutput.output ?? t.rawOutput.error;
      if (typeof said === 'string') texts.push(said);
    }
    out.push({ kind: 'event', event: { type: 'tool.result', itemId: t.id, ok, ...truncateResultText(texts.join('\n'), t.id, this.ctx), images } });
  }

  // The turn has ended, or the agent has gone: no more updates are coming
  // for anything still open.
  closeOpen(reason: string): AdapterOutput[] {
    const out = this.flush();
    for (const t of this.tools.values()) {
      if (t.done || !t.emitted) continue;
      t.done = true;
      out.push({ kind: 'event', event: { type: 'tool.result', itemId: t.id, ok: false, text: reason, truncated: false, images: [] } });
    }
    return out;
  }
}

function planEntries(raw: unknown[]): PlanEntry[] {
  return raw.filter(isPlainObject).map((e) => ({
    text: typeof e.content === 'string' ? e.content : '',
    status: e.status === 'in_progress' || e.status === 'completed' ? e.status : 'pending',
  }));
}

// Whether a path someone reported is inside the given folder, resolved.
export const absolutePath = (p: string, cwd: string): string => path.resolve(cwd, p);
