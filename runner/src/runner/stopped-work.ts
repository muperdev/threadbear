// Background work a restart stopped (shells, subagents) and frozen CLIs: telling the thread and carrying it on.
// Part of the Runner (runner.ts), split out on 4 Nov 2026 as a plain move so a
// fork's change to one area doesn't clash with another's. Each method's `this`
// is the Runner; runner.ts puts them on its prototype.
import type { Runner } from '../runner.ts';
import type { RunnerPart } from './part.ts';
import type { BackgroundTask as ProtocolBackgroundTask } from '../protocol.ts';
import type { BackgroundTask } from '../adapters/types.ts';
import type { ThreadState } from '../runner.ts';
import { CARRY_ON_RESTART, STOPPED_CARRY_ON_MAX_AGE_MS, STOPPED_WORK_WAKES, STOPPED_WORK_WAKE_MAX_AGE_MS, stoppedWorkMessage } from '../runner.ts';

export const stoppedWorkPart = {
  rememberStoppedWork(this: Runner, st: ThreadState): void {
    const { rec, rt } = st;
    if (rt.tasks.length === 0) return;
    const what = rt.tasks.map((t) => t.description || t.type);
    rec.stoppedWork = { what: [...(rec.stoppedWork?.what ?? []), ...what], at: this.isoNow() };
    this.markDirty(st);
  },

  // Once this runner is listening, for the threads it loaded at startup. A
  // thread coming across from a draining runner is told as adopt() loads it.
  tellStoppedWork(this: Runner): void {
    for (const st of this.threads.values()) this.tellOneStoppedWork(st);
  },

  tellOneStoppedWork(this: Runner, st: ThreadState): void {
    const { rec } = st;
    this.tellInterruptedRuns(st);
    const stopped = rec.stoppedWork;
    if (!stopped) return;
    delete rec.stoppedWork;
    this.markDirty(st);
    // Put away since: its next message would bring it back, and the note
    // is there to read then. This one mustn't bring it back by itself.
    if (this.bucket(rec) !== 'inbox') return;
    const wakes = rec.stoppedWorkWakes ?? 0;
    const why =
      this.clock.now() - Date.parse(stopped.at) > STOPPED_WORK_WAKE_MAX_AGE_MS
        ? 'the runner was away for more than a day'
        : wakes >= STOPPED_WORK_WAKES
          ? `it has already been woken ${STOPPED_WORK_WAKES} times for this since anyone last wrote to it`
          : undefined;
    try {
      if (why) {
        this.emit(st, { type: 'system.note', text: `Not waking this thread about its stopped background work: ${why}.` });
        return;
      }
      rec.stoppedWorkWakes = wakes + 1;
      this.sendTo(st, stoppedWorkMessage(stopped.what, wakes + 1 >= STOPPED_WORK_WAKES), [], { origin: 'runner', queue: true });
      this.log.info('told a thread its background work was stopped', { threadId: rec.id, what: stopped.what });
    } catch (e) {
      this.log.error('could not tell a thread its background work was stopped', { threadId: rec.id, error: String(e) });
    }
  },

  // Threads a restart, a crash or the Mac restarting cut off mid-turn
  // carry on by themselves with the runner's own message, as Mike would
  // otherwise have to tell each one to (30 Oct 2026, after a reboot:
  // "sorry I had to reboot, please continue"). main.ts calls this once
  // listening, after the restart's own resume and tellStoppedWork, so a
  // thread one of those already sent a message to isn't told twice. Not
  // a thread put away, one with a message waiting, one stopped more than
  // STOPPED_CARRY_ON_MAX_AGE_MS ago, or a carry-on that was itself cut
  // off (a turn that crashes the runner mustn't loop). If the network
  // isn't back yet, the carry-on waits for it (onNetworkLost).
  // `threadId`: just that one (a thread a crashed draining runner had).
  carryOnStopped(this: Runner, threadId?: string): void {
    let carried = 0;
    let left = 0;
    for (const st of this.threads.values()) {
      if (threadId && st.rec.id !== threadId) continue;
      const { rec, rt } = st;
      const stopped = rec.stoppedMidTurn;
      if (!stopped) continue;
      delete rec.stoppedMidTurn;
      this.markDirty(st);
      if (rt.deleted || rec.status !== 'stopped' || rt.turn || rec.queue.length || this.bucket(rec) !== 'inbox') continue;
      const old = !(this.clock.now() - Date.parse(stopped.at) <= STOPPED_CARRY_ON_MAX_AGE_MS);
      if (old || rec.restartCarryOn === stopped.turnId) {
        left++;
        continue;
      }
      try {
        rec.restartCarryOn = this.sendTo(st, CARRY_ON_RESTART, [], { origin: 'runner', queue: true });
        carried++;
        this.log.info('carried on a thread the runner stopped mid-turn', { threadId: rec.id });
      } catch (e) {
        left++;
        this.log.error('could not carry on a thread the runner stopped mid-turn', { threadId: rec.id, error: String(e) });
      }
    }
    // At startup the app is rarely connected yet, so it hears later.
    const say = (m: string) => (threadId ? this.notice('info', m) : this.startupNotice('info', m));
    if (carried) say(`${carried === 1 ? 'A thread the runner stopped mid-turn is' : `${carried} threads the runner stopped mid-turn are`} carrying on by ${carried === 1 ? 'itself' : 'themselves'}.`);
    if (left) say(`${left === 1 ? 'A thread was' : `${left} threads were`} mid-turn when the runner stopped; send a message to carry on.`);
  },

  // Threads whose CLI is running background shells or subagents. A restart
  // waits for them (restart.ts), since stopping the CLI stops them.
  backgroundWork(this: Runner): { threadId: string; tasks: string[] }[] {
    const out: { threadId: string; tasks: string[] }[] = [];
    for (const st of this.threads.values()) {
      const tasks = this.background(st).map((t) => (t.finished ? `${t.description || 'a background task'} (just finished, which the agent is about to look at)` : t.description || 'a background task'));
      if (tasks.length) out.push({ threadId: st.rec.id, tasks });
    }
    return out;
  },

  // Claude threads with no turn going whose CLI still runs background
  // work, for the stuck-job check (stuck-jobs.ts). Its output files are
  // named by the session and task ids.
  idleBackground(this: Runner): { threadId: string; sessionId: string; tasks: BackgroundTask[] }[] {
    const out: { threadId: string; sessionId: string; tasks: BackgroundTask[] }[] = [];
    for (const { rec, rt } of this.threads.values()) {
      if (rt.deleted || rec.archivedAt || rec.provider !== 'claude' || !rec.nativeSessionId || !rt.session) continue;
      if (rt.turn || rt.wakeTimer || rec.status !== 'idle' || !rt.tasks.length) continue;
      out.push({ threadId: rec.id, sessionId: rec.nativeSessionId, tasks: rt.tasks });
    }
    return out;
  },

  // Claude turns the CLI has said nothing in for a while, for the frozen
  // CLI check (frozen-cli.ts). Not one waiting on Mike's approval: that's
  // quiet because of him, and the CLI is fine.
  quietTurns(this: Runner, minMs: number): { threadId: string; turnId: string; sessionId: string; quietMs: number }[] {
    const now = this.clock.now();
    const out: { threadId: string; turnId: string; sessionId: string; quietMs: number }[] = [];
    for (const { rec, rt } of this.threads.values()) {
      const turn = rt.turn;
      if (!turn || rt.deleted || !rt.session || rt.closing || rec.provider !== 'claude' || !rec.nativeSessionId) continue;
      if (turn.auto && !turn.cliStarted) continue;
      if (rec.status === 'waiting') continue;
      const quietMs = now - Math.max(turn.startedAt, rt.lastOutputAt ?? 0);
      if (quietMs >= minMs) out.push({ threadId: rec.id, turnId: turn.turnId, sessionId: rec.nativeSessionId, quietMs });
    }
    return out;
  },

  // Says so in the thread and puts it in Needs you, as the agent can't: a
  // frozen CLI isn't reading anything.
  cliFrozen(this: Runner, threadId: string, turnId: string, text: string): void {
    const st = this.threads.get(threadId);
    if (!st || st.rt.turn?.turnId !== turnId) return;
    st.rt.frozen = true;
    st.rec.forceUnread = true;
    this.emit(st, { type: 'system.note', text }, turnId);
    this.markDirty(st);
  },

  cliMovingAgain(this: Runner, threadId: string, turnId: string, text: string): void {
    const st = this.threads.get(threadId);
    if (!st) return;
    st.rt.frozen = false;
    st.rec.forceUnread = false;
    // A turn that has ended meanwhile (stopped by hand) has nothing to add.
    if (st.rt.turn?.turnId === turnId) this.emit(st, { type: 'system.note', text }, turnId);
    this.markDirty(st);
  },

  // Ends the turn and stops the CLI. TERM can't reach a process blocked in
  // the kernel, but the spawner follows it with KILL (proc.ts).
  stopFrozenCli(this: Runner, threadId: string, turnId: string, text: string): void {
    const st = this.threads.get(threadId);
    if (!st || st.rt.turn?.turnId !== turnId) return;
    const session = st.rt.session;
    st.rt.frozen = false;
    st.rec.forceUnread = false;
    this.failTurn(st, text);
    session?.kill();
  },

  // The summary's background list: what the CLI is still running, then
  // (while the wake grace runs) what just finished and is about to be read.
  background(this: Runner, st: ThreadState): ProtocolBackgroundTask[] {
    const { rt } = st;
    const out: ProtocolBackgroundTask[] = rt.tasks.map((t) => ({ kind: t.kind, description: t.description || t.type }));
    if (rt.wakeTimer) {
      // A grace with nothing recorded as finished (a task list the CLI
      // replaced wholesale) still says a result is on its way.
      const finished = rt.finished.length ? rt.finished : [{ id: '', type: 'task', kind: 'other' as const, description: '' }];
      for (const t of finished) out.push({ kind: t.kind, description: t.description || (t.type === 'task' ? '' : t.type), finished: true });
    }
    return out;
  },
};

export type StoppedWorkPart = RunnerPart<typeof stoppedWorkPart>;
