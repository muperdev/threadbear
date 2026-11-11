// What fills a thread's context (docs/protocol.md, "Context breakdown"):
// the composer ring's popover asks when it opens. Claude's CLI answers it
// with its own /context rows, so it's asked of the thread's live CLI and the
// answer kept with the thread for when the CLI has gone (idle CLIs stop
// after 30 minutes). A CLI is never started just for this: Ghostex starts a
// headless copy of the session to ask, which costs a process per look.
// Each method's `this` is the Runner; runner.ts puts them on its prototype.
import type { Runner } from '../runner.ts';
import type { RunnerPart } from './part.ts';
import type { ContextUsage } from '../protocol.ts';

// A CLI busy with a turn still answers control requests straight away;
// one that hasn't in this long isn't going to.
const ASK_MS = 20_000;

export type ContextUsageAnswer = { usage?: ContextUsage; live: boolean; note?: string };

export const contextUsagePart = {
  async contextUsage(this: Runner, threadId: string): Promise<ContextUsageAnswer> {
    const st = this.mustGet(threadId);
    const { rec, rt } = st;
    // Only for the native session it was asked of: /clear or a move to
    // another login or provider starts a new one, and the old rows are
    // another conversation's.
    // A move to another login waiting (handoff) starts a new session with
    // the next message, so the kept rows are already the past; an edit
    // that cuts the conversation back drops them (runner/editing.ts).
    const kept = (): ContextUsage | undefined => {
      const k = rec.contextUsage;
      if (!k || k.sessionId !== rec.nativeSessionId || rec.handoff) return undefined;
      const { sessionId: _, ...usage } = k;
      return usage;
    };
    const keptOnly = (live: boolean, note?: string): ContextUsageAnswer => {
      const usage = kept();
      return { ...(usage ? { usage } : {}), live, ...(note ? { note } : {}) };
    };
    const session = rt.session;
    // Going away (released, or /clear's old session waiting to go): it
    // would describe a conversation the thread has left.
    // Only Claude's CLI says: a thread moved to Codex has nothing to show.
    if (rec.provider !== 'claude') return { live: false };
    if (!session?.contextUsage || rt.closing || rt.releaseWhenIdle) return keptOnly(false);
    const already = this.contextAsks.get(threadId);
    if (already) return already;
    const sessionId = rec.nativeSessionId;
    const ask = (async (): Promise<ContextUsageAnswer> => {
      try {
        let timer: ReturnType<typeof setTimeout> | undefined;
        const timeout = new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(new Error(`no answer in ${ASK_MS / 1000}s`)), ASK_MS);
          timer.unref?.();
        });
        const got = await Promise.race([session.contextUsage!(), timeout]).finally(() => clearTimeout(timer));
        if (!got) return keptOnly(false);
        // Deleted, handed over or moved to another runner while it asked:
        // saving would bring its thread.json back (Codex review).
        if (rt.deleted || this.threads.get(threadId) !== st) return { live: false };
        // The thread moved on to another session while it asked.
        if (!sessionId || rec.nativeSessionId !== sessionId || rec.handoff) return keptOnly(Boolean(rt.session));
        const usage: ContextUsage = { ...got, at: new Date(this.clock.now()).toISOString() };
        rec.contextUsage = { ...usage, sessionId };
        // Straight to thread.json, not markDirty: nothing in the summary
        // changed, so no thread.updated goes to the app or the backend.
        try {
          this.store.save(rec);
        } catch (e) {
          this.log.warn('could not save context breakdown', { threadId, error: String(e) });
        }
        return { usage, live: true };
      } catch (e) {
        this.log.warn('context breakdown failed', { threadId, error: String(e) });
        return keptOnly(true, "Claude didn't answer just now, so this is the last breakdown it gave.");
      } finally {
        this.contextAsks.delete(threadId);
      }
    })();
    this.contextAsks.set(threadId, ask);
    return ask;
  },
};

export type ContextUsagePart = RunnerPart<typeof contextUsagePart>;
