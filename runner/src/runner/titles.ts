// Thread titles and the explain pane: from the first message, from a small model, or from the librarian.
// Part of the Runner (runner.ts), split out on 4 Nov 2026 as a plain move so a
// fork's change to one area doesn't clash with another's. Each method's `this`
// is the Runner; runner.ts puts them on its prototype.
import type { Runner } from '../runner.ts';
import type { RunnerPart } from './part.ts';
import { ErrorCode, RpcError } from '../protocol.ts';
import { EXPLAIN_SYSTEM, RECENT_CHARS, RECENT_MESSAGES, explainPrompt } from '../explain.ts';
import type { ExplainTurn } from '../explain.ts';
import type { ThreadState } from '../runner.ts';
import { RECAP_BRIEF_CHARS, RECAP_PURPOSE_CHARS, TITLE_PROMPT, cleanTitle } from '../runner.ts';
import { cutSafely } from '../safe-cut.ts';

export const titlesPart = {
  // Asks a small model for a better title than the first line. Without a
  // pool, out of room or in a private folder it says why and nothing
  // changes. A title set meanwhile (by Mike or an agent) wins.
  // `reply`, the start of the agent's first answer, is there when an older
  // thread is named (retitle): a terse first message like "Explain thread
  // count" means little without it.
  async titleFromModel(this: Runner, st: ThreadState, text: string, reply?: string): Promise<{ title: string } | { reason: string }> {
    if (!this.models) return { reason: 'Model calls are off in this runner.' };
    const before = st.rec.title;
    const res = await this.models.generate({
      feature: 'titles',
      cwd: st.rec.cwd,
      system: TITLE_PROMPT,
      prompt: reply ? `Message:\n${text.slice(0, 4000)}\n\nStart of the reply:\n${reply.slice(0, 1000)}` : `Message:\n${text.slice(0, 4000)}`,
      // Room for Haiku to think first: at 30 it sometimes ran out before
      // writing a word. A long answer is turned down by cleanTitle anyway.
      maxTokens: 300,
    });
    if (!res.ok) return { reason: res.message };
    if (st.rt.deleted) return { reason: 'Deleted meanwhile.' };
    if (st.rec.titleSet || st.rec.title !== before) return { reason: 'Renamed meanwhile.' };
    const title = cleanTitle(res.value);
    if (!title) return { reason: 'The model gave no title.' };
    st.rec.title = title;
    st.rec.updatedAt = this.isoNow();
    this.markDirty(st);
    return { title };
  },

  // Names the threads that started before there was a key (or whose call
  // failed): the ones still titled with their first message's first line
  // and never named by anyone. A model title doesn't set titleSet, so the
  // first-line check is what keeps those from being asked again. Given
  // threadIds, it names those whatever their title, so a poor model title
  // can be asked for again; one named by hand still stays.
  async retitle(this: Runner, params: { dryRun?: boolean; threadIds?: string[] }): Promise<{ threads: { threadId: string; from: string; to?: string; reason?: string }[] }> {
    const picked = params.threadIds?.map((id) => this.mustGet(id));
    const due: { st: ThreadState; text: string; reply?: string }[] = [];
    for (const st of picked ?? this.threads.values()) {
      if (st.rec.titleSet) continue;
      const { text, reply } = this.opening(st.rec.id);
      const line = text?.split('\n').map((l) => l.trim()).find(Boolean);
      // titleFrom cut it at some length; an older runner may have cut it elsewhere.
      if (line && (picked || (st.rec.title && line.startsWith(st.rec.title)))) due.push({ st, text: text!, reply });
    }
    const threads = await Promise.all(
      due.map(async ({ st, text, reply }) => {
        const from = st.rec.title;
        if (params.dryRun) return { threadId: st.rec.id, from };
        const r = await this.titleFromModel(st, text, reply);
        return 'title' in r ? { threadId: st.rec.id, from, to: r.title } : { threadId: st.rec.id, from, reason: r.reason };
      }),
    );
    this.log.info('retitle', { dryRun: Boolean(params.dryRun), found: threads.length, named: threads.filter((t) => t.to).length });
    return { threads };
  },

  // Explains words Mike selected in a thread (explain.ts), on the small
  // model. A side conversation: nothing is written to the thread.
  // `message` is the whole message the selection came from, as the app
  // shows it (a row's seq can be a streamed delta's, not the final text's).
  async explain(this: Runner, params: { threadId: string; selection: string; message?: string; question?: string; earlier?: ExplainTurn[] }): Promise<{ answer: string; model: string }> {
    const st = this.mustGet(params.threadId);
    if (!this.models) throw new RpcError(ErrorCode.other, 'Model calls are off in this runner.');
    const gist = this.threadGist(st.rec.id, { recent: RECENT_MESSAGES, chars: RECENT_CHARS });
    const res = await this.models.generate({
      feature: 'explain',
      cwd: st.rec.cwd,
      system: EXPLAIN_SYSTEM,
      prompt: explainPrompt({ selection: params.selection, message: params.message, first: gist.first, recent: gist.recent, earlier: params.earlier ?? [], question: params.question }),
      maxTokens: 800,
    });
    if (!res.ok) throw new RpcError(ErrorCode.other, res.message || `No explanation (${res.reason}).`);
    return { answer: res.value.trim(), model: res.model };
  },

  // The librarian's line on where the thread is up to.
  setBrief(this: Runner, threadId: string, text: string, purpose?: string): void {
    const st = this.threads.get(threadId);
    if (!st || st.rt.deleted) return;
    // Cut where no secret is split (safe-cut.ts): the recap puts the purpose
    // in front of the brief, so a purpose cut just before a URL's "@" left the
    // password mid-text, where the scrub no longer knew it (Daybreak, 10 Nov).
    const clean = cutSafely(text.trim(), RECAP_BRIEF_CHARS);
    // A look that gave no purpose keeps the one the thread has.
    const why = (purpose ? cutSafely(purpose.replace(/\s+/g, ' ').trim(), RECAP_PURPOSE_CHARS) : '') || st.rec.brief?.purpose;
    if (!clean || (clean === st.rec.brief?.text && why === st.rec.brief?.purpose)) return;
    st.rec.brief = { text: clean, at: this.isoNow(), ...(why ? { purpose: why } : {}) };
    this.highlightsChanged(st);
  },

  // The librarian's title for a thread that has moved on from its old one.
  // Never over one Mike or an agent set, and only while the title is still
  // `from`, the one the look was given, so a rename meanwhile wins. Like a
  // model title it doesn't set titleSet, so a later look can rename again.
  retitleFromLibrarian(this: Runner, threadId: string, title: string, from: string): string | undefined {
    const st = this.threads.get(threadId);
    if (!st || st.rt.deleted || st.rec.titleSet || st.rec.title !== from) return undefined;
    const clean = cleanTitle(title);
    if (!clean || clean === st.rec.title) return undefined;
    st.rec.title = clean;
    st.rec.updatedAt = this.isoNow();
    this.markDirty(st);
    return clean;
  },
};

export type TitlesPart = RunnerPart<typeof titlesPart>;
