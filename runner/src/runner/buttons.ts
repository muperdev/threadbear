// Reply buttons (docs/protocol.md, "Reply buttons"): an agent's add_button
// makes one, its reply links button:<id>, and Mike's press starts a thread
// with the brief or sends the message, once.
// Part of the Runner (runner.ts); each method's `this` is the Runner.
import type { Runner } from '../runner.ts';
import type { RunnerPart } from './part.ts';
import type { Ask, EffortLevel, EventBody, PermissionMode, ReplyButton } from '../protocol.ts';
import { AGENT_MESSAGE_PREFIX, ErrorCode, RpcError } from '../protocol.ts';
import { isDirectory } from '../fsutil.ts';
import { findPool } from '../pools.ts';
import type { ThreadState } from '../runner.ts';
import { runCommand } from '../patches.ts';
import { threadEnv } from '../restart.ts';
import { safeStart } from '../safe-cut.ts';
import { experimentOn } from '../experiments.ts';
import crypto from 'node:crypto';
import path from 'node:path';

export const MAX_BUTTON_TITLE = 60;
export const MAX_BUTTON_TEXT = 20_000;
// Kept per thread, newest. A daily brief makes about six a day.
export const MAX_BUTTONS = 200;
// How much of a run's output the agent is told, from the end.
const RUN_TELL_CHARS = 8000;

// Ranked like threadbear-mcp.ts's ACCESS: plan only changes what Mike approves.
const RANK: Record<PermissionMode, number> = { ask: 0, plan: 0, auto: 1, full: 2 };
const MODE_NAMES: Record<PermissionMode, string> = { full: 'Full access', auto: 'Auto', ask: 'Ask before edits', plan: 'Plan first' };

export type ButtonInput = {
  title: string;
  // A new thread with this brief...
  start?: { text: string; title?: string; cwd?: string; accountId?: string; poolId?: string; model?: string; effort?: EffortLevel; permissionMode?: PermissionMode; why?: string };
  // ...or a message to this thread.
  send?: { threadId: string; text: string };
};

const clean = (s: string | undefined): string | undefined => (s?.trim() ? s.trim() : undefined);

export const buttonsPart = {
  // Only the thread's own agent calls this (add_button, bound to its
  // thread); there's no RPC that makes one, so nothing outside the thread
  // can put a button in its replies.
  createButton(this: Runner, threadId: string, input: ButtonInput): { button: ReplyButton } {
    const st = this.mustGet(threadId);
    this.checkAccepting();
    const title = clean(input.title)?.replace(/\s+/g, ' ');
    if (!title) throw new RpcError(ErrorCode.badParams, 'A button needs a title.');
    if (title.length > MAX_BUTTON_TITLE) throw new RpcError(ErrorCode.badParams, `A button's title is at most ${MAX_BUTTON_TITLE} characters.`);
    if (Boolean(input.start) === Boolean(input.send)) throw new RpcError(ErrorCode.badParams, 'Give a button either start (a new thread) or send (a message to a thread), not both.');
    // What the maker's agent can do now: a button can't hand Mike a
    // thread or a message with more access than that, as its title is the
    // agent's and he presses it trusting it (threadbear-mcp.ts, "Raising access").
    const maker = this.accessOf(threadId);
    const button: ReplyButton = { id: this.newButtonId(st), title, createdAt: this.isoNow(), ...(st.rt.turn ? { turnId: st.rt.turn.turnId } : {}) };
    if (input.start) {
      const s = input.start;
      const text = clean(s.text);
      if (!text) throw new RpcError(ErrorCode.badParams, 'start needs text: the brief for the new thread.');
      if (text.length > MAX_BUTTON_TEXT) throw new RpcError(ErrorCode.badParams, `The brief is at most ${MAX_BUTTON_TEXT} characters.`);
      const cwd = path.resolve(st.rec.cwd, s.cwd?.trim() || '.');
      if (!isDirectory(cwd)) throw new RpcError(ErrorCode.badParams, `Folder not found: ${cwd}`);
      if (s.accountId !== undefined && !this.accounts.get(s.accountId)) throw new RpcError(ErrorCode.accountNotFound, `No account called ${s.accountId}`);
      if (s.poolId !== undefined && !findPool(this.hello().pools ?? [], s.poolId)) throw new RpcError(ErrorCode.badParams, `No pool ${s.poolId}`);
      if (s.accountId && s.model && !this.modelFits(this.accounts.get(s.accountId)!, s.model)) throw new RpcError(ErrorCode.badParams, `${s.accountId} doesn't have the model ${s.model}.`);
      const permissionMode = s.permissionMode ?? (maker === 'full' ? 'full' : maker);
      if (RANK[permissionMode] > RANK[maker]) throw new RpcError(ErrorCode.badParams, `This thread has ${MODE_NAMES[maker]}, so its buttons can't start a thread with ${MODE_NAMES[permissionMode]}.`);
      button.start = {
        text,
        ...(clean(s.title) ? { title: clean(s.title)!.slice(0, 200) } : {}),
        cwd,
        ...(s.accountId ? { accountId: s.accountId } : {}),
        ...(s.poolId && !s.accountId ? { poolId: findPool(this.hello().pools ?? [], s.poolId)!.id } : {}),
        ...(clean(s.model) ? { model: clean(s.model)! } : {}),
        ...(s.effort ? { effort: s.effort } : {}),
        permissionMode,
        ...(clean(s.why) ? { why: clean(s.why)!.replace(/\s+/g, ' ').slice(0, 80) } : {}),
      };
    } else {
      const s = input.send!;
      const text = clean(s.text);
      if (!text) throw new RpcError(ErrorCode.badParams, 'send needs text: the message.');
      if (text.length > MAX_BUTTON_TEXT) throw new RpcError(ErrorCode.badParams, `The message is at most ${MAX_BUTTON_TEXT} characters.`);
      const target = this.threads.get(s.threadId);
      if (!target) throw new RpcError(ErrorCode.threadNotFound, `No thread ${s.threadId} on this runner.`);
      if (target.rec.id === threadId) throw new RpcError(ErrorCode.badParams, "A button can't message its own thread.");
      const theirs = this.accessOf(target.rec.id);
      if (RANK[theirs] > RANK[maker]) throw new RpcError(ErrorCode.badParams, `"${target.rec.title}" has ${MODE_NAMES[theirs]} and this thread has ${MODE_NAMES[maker]}, so a button here can't message it. send_message asks the user first.`);
      button.send = { threadId: target.rec.id, threadTitle: target.rec.title, text };
    }
    this.persistButton(st, button);
    return { button: structuredClone(button) };
  },

  listButtons(this: Runner, threadId: string): { buttons: ReplyButton[] } {
    return { buttons: structuredClone(this.mustGet(threadId).rec.buttons ?? []) };
  },

  // Mike's press, from the Mac, the phone or the website. Once only: a
  // button already pressed comes back as it is, so a retried request or a
  // press from two places never starts two threads.
  pressButton(this: Runner, threadId: string, buttonId: string): { button: ReplyButton } {
    const st = this.mustGet(threadId);
    const button = st.rec.buttons?.find((b) => b.id === buttonId);
    if (!button) throw new RpcError(ErrorCode.badParams, `Button not found: ${buttonId}`);
    if (button.pressed || button.running) return { button: structuredClone(button) };
    if (button.run) return this.pressRun(st, button);
    // Access is checked again now, not only when it was made: the maker or
    // the target may have changed mode since (Codex's review, 7 Nov 2026).
    // A refusal here is saved as a failed press, like any other.
    const maker = this.accessOf(threadId);
    const over = button.start && RANK[button.start.permissionMode] > RANK[maker]
      ? `This thread now has ${MODE_NAMES[maker]}, so its button can't start a thread with ${MODE_NAMES[button.start.permissionMode]}.`
      : button.send && this.threads.has(button.send.threadId) && RANK[this.accessOf(button.send.threadId)] > RANK[maker]
        ? `"${button.send.threadTitle}" now has more access than this thread, so its button can't message it. Ask in this thread for it to use send_message.`
        : undefined;
    if (over) {
      this.persistButton(st, { ...button, failed: { at: this.isoNow(), error: over } });
      throw new RpcError(ErrorCode.badParams, over);
    }
    // The words are the agent's, so they say so, as send_message's do.
    const from = `${AGENT_MESSAGE_PREFIX}"${st.rec.title}" (${st.rec.id.slice(0, 8)})]\n\n`;
    let next: ReplyButton;
    try {
      if (button.start) {
        const s = button.start;
        // Nothing waits between here and the press being saved (create is
        // synchronous), so a second press can't slip in.
        const r = this.create({
          text: from + s.text,
          ...(s.title ? { title: s.title } : { title: button.title }),
          cwd: s.cwd,
          ...(s.accountId ? { accountId: s.accountId } : {}),
          ...(s.poolId ? { poolId: s.poolId } : {}),
          ...(s.model ? { model: s.model } : {}),
          ...(s.effort ? { effort: s.effort } : {}),
          permissionMode: s.permissionMode,
          // Mike's press, of a button he allowed when a sandboxed thread
          // made it (add_button's card says "outside the sandbox"). One
          // into a sandbox folder is still shut in there.
          unconfine: true,
          startedBy: { kind: 'thread', label: 'Button', threadId: st.rec.id, threadTitle: st.rec.title, ...(s.why ? { why: s.why } : {}) },
        });
        next = { ...button, pressed: { at: this.isoNow(), threadId: r.thread.id } };
      } else {
        const s = button.send!;
        this.send(s.threadId, from + s.text);
        next = { ...button, pressed: { at: this.isoNow(), threadId: s.threadId } };
      }
      delete next.failed;
    } catch (e) {
      const error = (e as Error).message ?? String(e);
      this.persistButton(st, { ...button, failed: { at: this.isoNow(), error: error.slice(0, 300) } });
      throw e instanceof RpcError ? e : new RpcError(ErrorCode.badParams, error);
    }
    this.persistButton(st, next);
    return { button: structuredClone(next) };
  },

  // The turn's asks with a command get their button here, as the card is
  // made, so no agent ever holds the id of one: a link to it in a reply
  // couldn't show the command it would run.
  armRunAsks(this: Runner, st: ThreadState, asks: Ask[], turnId: string): Ask[] {
    if (!asks.some((a) => a.run)) return asks;
    const on = experimentOn(this.cfg.home, 'runAsks');
    return asks.map((ask) => {
      if (!ask.run) return ask;
      if (!on) {
        const { run: _run, ...rest } = ask;
        return rest;
      }
      const cwd = st.rec.cwd;
      const button: ReplyButton = { id: this.newButtonId(st), title: 'Run', createdAt: this.isoNow(), turnId, run: { command: ask.run.command, cwd } };
      this.persistButton(st, button);
      return { ...ask, run: { command: ask.run.command, cwd, buttonId: button.id } };
    });
  },

  // Starts a run's command and answers at once: it can take minutes, and
  // the card shows it going through `running`.
  pressRun(this: Runner, st: ThreadState, button: ReplyButton): { button: ReplyButton } {
    const problem = this.runProblem(st, button);
    if (problem) {
      this.persistButton(st, { ...button, failed: { at: this.isoNow(), error: problem } });
      throw new RpcError(ErrorCode.badParams, problem);
    }
    const { failed: _failed, ...rest } = button;
    const started: ReplyButton = { ...rest, running: { at: this.isoNow() } };
    // Saved before it starts, so a second press from another device can't
    // slip in and a restart can tell it may have partly run.
    this.persistButton(st, started);
    void this.runPressed(st, started).catch((e: unknown) => this.log.warn('a run button failed', { threadId: st.rec.id, button: button.id, error: String(e) }));
    return { button: structuredClone(started) };
  },

  runProblem(this: Runner, st: ThreadState, button: ReplyButton): string | undefined {
    if (this.stopping || this.draining) return 'This runner is restarting. Press Run again in a moment.';
    // Only the open card: an older one's command was for how things stood then.
    if (!st.rec.needs?.asks?.some((a) => a.run?.buttonId === button.id)) return 'That card has been replaced, so its command is out of date.';
    // The agent has moved on, so what it asked for may not stand.
    if (st.rt.turn) return 'The agent has started working again, so this card is out of date.';
    if (st.rt.cleaning || st.rec.archivedAt) return 'This thread is archived, so its folder may be in use by another thread.';
    const cwd = button.run!.cwd;
    const gone = st.rec.worktree;
    if (gone?.removedAt && cwd.startsWith(gone.path)) return `This thread's worktree at ${gone.path} was removed, so nothing runs there.`;
    if (!isDirectory(cwd)) return `Folder not found: ${cwd}`;
    return undefined;
  },

  async runPressed(this: Runner, st: ThreadState, button: ReplyButton): Promise<void> {
    const key = `button:${button.id}`;
    // Holds archiving, handovers and moves until it's done, as an action does.
    const running = (st.rt.runningActions ??= new Set());
    running.add(key);
    try {
      const run = button.run!;
      const outcome = await runCommand(run.command, {
        cwd: run.cwd,
        env: { ...process.env, ...threadEnv(st.rec.id, this.cfg.home) },
        shell: this.actionShell,
        timeoutMs: this.actionTimeoutMs,
        killGroup: true,
      });
      if (st.rt.deleted) return;
      const { running: _running, ...rest } = button;
      const ran: NonNullable<ReplyButton['ran']> = { ok: outcome.ok, durationMs: outcome.durationMs, ...(outcome.exitCode !== undefined ? { exitCode: outcome.exitCode } : {}) };
      this.persistButton(st, { ...rest, pressed: { at: this.isoNow(), threadId: st.rec.id }, ran });
      const body: EventBody = { type: 'action.run', actionId: key, patch: 'card', title: 'Run', command: run.command, ok: outcome.ok, text: outcome.text, truncated: outcome.truncated, durationMs: outcome.durationMs };
      if (outcome.exitCode !== undefined) body.exitCode = outcome.exitCode;
      this.emit(st, body, st.rt.turn?.turnId);
      st.rec.lastActivityAt = this.isoNow();
      this.markDirty(st);
      this.recheckLooseEnds(st);
      // Its last part, begun where no secret is cut in two (safe-cut.ts), as it goes to Convex as a message.
      const shown = outcome.text.length > RUN_TELL_CHARS ? outcome.text.slice(safeStart(outcome.text, outcome.text.length - RUN_TELL_CHARS)) : outcome.text;
      const verdict = outcome.ok ? 'succeeded' : outcome.exitCode !== undefined ? `failed with exit ${outcome.exitCode}` : 'was stopped before it finished, or timed out';
      const told =
        `[Threadbear] The user pressed Run on your card. In ${run.cwd} this ${verdict} after ${Math.round(outcome.durationMs / 1000)}s:\n\n` +
        '```\n' + run.command + '\n```\n\nOutput:\n\n```\n' + (shown.trim() || '(no output)') + '\n```\n\nCarry on from here: they don\'t need to paste anything back.';
      try {
        this.sendTo(st, told, [], { origin: 'runner', queue: true });
      } catch (e) {
        this.emit(st, { type: 'system.note', text: "Couldn't tell the agent how the command went, so tell it yourself." });
        this.log.warn('could not tell the agent a run result', { threadId: st.rec.id, button: button.id, error: String(e) });
      }
    } finally {
      running.delete(key);
      if (running.size === 0) this.archiveWhenGone(st);
    }
  },

  // A runner that stopped with a command going took it with it. Run can be
  // pressed again, but it says what happened, as it may have done half.
  tellInterruptedRuns(this: Runner, st: ThreadState): void {
    for (const button of st.rec.buttons ?? []) {
      if (!button.running || st.rt.runningActions?.has(`button:${button.id}`)) continue;
      const { running: _running, ...rest } = button;
      this.persistButton(st, { ...rest, failed: { at: this.isoNow(), error: 'Threadbear restarted while this was running, so it may only have partly run.' } });
      try {
        this.sendTo(st, `[Threadbear] Threadbear restarted while the command on your card was running, so it may only have partly run:\n\n\`\`\`\n${button.run?.command ?? ''}\n\`\`\`\n\nCheck how far it got before asking them to run it again.`, [], { origin: 'runner', queue: true });
      } catch (e) {
        this.log.warn('could not tell the agent a run was interrupted', { threadId: st.rec.id, button: button.id, error: String(e) });
      }
    }
  },

  newButtonId(this: Runner, st: ThreadState): string {
    const taken = new Set((st.rec.buttons ?? []).map((b) => b.id));
    for (;;) {
      // 8 of [a-z0-9]: short enough for the agent to copy into a link.
      const id = Array.from(crypto.randomBytes(8), (b) => 'abcdefghijklmnopqrstuvwxyz0123456789'[b % 36]).join('');
      if (!taken.has(id)) return id;
    }
  },

  persistButton(this: Runner, st: ThreadState, button: ReplyButton): void {
    const previous = st.rec.buttons ?? [];
    const buttons = (previous.some((b) => b.id === button.id) ? previous.map((b) => (b.id === button.id ? button : b)) : [...previous, button]).slice(-MAX_BUTTONS);
    // What the open card shows of a run, which is all the phone has of it.
    if (button.run && st.rec.needs?.asks?.some((a) => a.run?.buttonId === button.id)) {
      st.rec.needs = {
        ...st.rec.needs,
        asks: st.rec.needs.asks.map((a) => {
          if (a.run?.buttonId !== button.id) return a;
          const { running: _running, failed: _failed, ...run } = a.run;
          return { ...a, run: { ...run, ...(button.running ? { running: true as const } : {}), ...(button.failed ? { failed: button.failed.error } : {}) } };
        }),
      };
      this.markDirty(st);
    }
    this.store.save({ ...st.rec, buttons });
    st.rec.buttons = buttons;
    this.emit(st, { type: 'button.updated', button });
  },
};

export type ButtonsPart = RunnerPart<typeof buttonsPart>;
