// What a thread may do over the RPC (docs/protocol.md, "A thread's own key").
// The app and tb outside a thread have the runner's token and answer to
// nobody. A connection that belongs to a thread answers to the same rules as
// that thread's own threadbear tools (threadbear-mcp.ts, "Raising access"),
// read at each call, so a thread lowered to Ask loses the lot at once.
//
// Those tools can put up a card and wait; a shell command can't sensibly hold
// one open, so here anything above the caller is refused, and the refusal
// names the tool that can ask. Refusing by default is the point: a method
// that's added later is closed until someone decides it's safe here.
import type { PermissionMode } from './protocol.ts';
import { ErrorCode, RpcError } from './protocol.ts';
import { above, MODE_NAMES, raises, scheduleModes, type Authority } from './threadbear-mcp.ts';

type Params = Record<string, unknown>;

// Anyone holding a thread's key may read: these change nothing outside the
// connection. media.import only copies a file into the runner's inbox for a
// tb --image, which the thread's shell could already read.
export const GATED_READS: ReadonlySet<string> = new Set([
  'auth',
  'auth.whoami',
  'hello',
  'threads.list',
  'threads.search',
  'threads.events',
  'threads.subscribe',
  'threads.unsubscribe',
  'threads.subagent',
  'threads.highlights',
  'threads.commands',
  'threads.contextUsage',
  'buttons.list',
  'reviews.list',
  'reviews.get',
  'pools.list',
  'schedules.list',
  'schedules.preview',
  'runners.list',
  'runners.stats',
  'remoteThreads.list',
  'projects.list',
  'projects.get',
  'instructions.get',
  'librarian.status',
  'settings.get',
  'runner.status',
  'upstream.status',
  'clis.list',
  'connectors.status',
  'usage.stats',
  'media.import',
]);

type ThreadLike = { permissionMode?: PermissionMode; title: string; status?: string };

export type CallerGateDeps = {
  runner: {
    has(threadId: string): boolean;
    get(threadId: string): ThreadLike;
    accessOf(threadId: string): PermissionMode;
    confinedOf(threadId: string): boolean;
  };
  // Threads a draining runner still has (rpc.ts's RemoteThreads).
  remote?: { owns(threadId: string): boolean; summaries(include: ('inbox' | 'snoozed' | 'archived')[]): (ThreadLike & { id: string })[] };
  schedules?: {
    find(idOrPrefix: string): { id: string; title: string; threadId?: string; permissionMode?: PermissionMode; check?: string; paused?: boolean };
    list(threadId?: string): { threadId?: string; check?: string }[];
  };
};

const ALL = ['inbox', 'snoozed', 'archived'] as const;

function str(p: Params, k: string): string | undefined {
  return typeof p[k] === 'string' ? (p[k] as string) : undefined;
}

export class CallerGate {
  private deps: CallerGateDeps;
  constructor(deps: CallerGateDeps) {
    this.deps = deps;
  }

  // The caller's authority at this moment. A thread the runner can't place
  // (deleted, say) is as low as it gets: confined counts everything as above.
  private authority(threadId: string): Authority {
    const { runner, remote } = this.deps;
    if (runner.has(threadId)) return { mode: runner.get(threadId).permissionMode ?? 'ask', confined: runner.confinedOf(threadId) };
    const t = remote?.owns(threadId) ? remote.summaries([...ALL]).find((x) => x.id === threadId) : undefined;
    return t ? { mode: t.permissionMode ?? 'ask', confined: false } : { mode: 'ask', confined: true };
  }

  // What another thread's agent has, for anything that reaches it. One this
  // runner can't read counts as full, as threadbear-mcp.ts's accessOf does.
  private targetAccess(threadId: string): Authority {
    const { runner, remote } = this.deps;
    if (runner.has(threadId)) return { mode: runner.accessOf(threadId), confined: runner.confinedOf(threadId) };
    const t = remote?.owns(threadId) ? remote.summaries([...ALL]).find((x) => x.id === threadId) : undefined;
    // A busy thread on a draining runner may be running a turn that started
    // with more than its saved mode; a summary doesn't say, so assume it did.
    const busy = t?.status === 'running' || t?.status === 'starting' || t?.status === 'waiting';
    return { mode: busy ? 'full' : (t?.permissionMode ?? 'full'), confined: false };
  }

  private title(threadId: string): string {
    const { runner, remote } = this.deps;
    if (runner.has(threadId)) return `"${runner.get(threadId).title}"`;
    const t = remote?.owns(threadId) ? remote.summaries([...ALL]).find((x) => x.id === threadId) : undefined;
    return t ? `"${t.title}"` : threadId.slice(0, 8);
  }

  whoami(threadId: string): { threadId: string; mode: PermissionMode; limited: boolean } {
    const a = this.authority(threadId);
    return { threadId, mode: a.mode, limited: a.confined || a.mode !== 'full' };
  }

  // Throws a refusal, or returns the params to run with (the caller's own
  // mode filled in, what only a client with the token may say taken out).
  check(threadId: string, method: string, p: Params): Params {
    const caller = this.authority(threadId);
    if (caller.mode === 'full' && !caller.confined) return p;
    if (GATED_READS.has(method)) return p;
    const who = caller.confined ? 'This thread runs in a sandbox' : `This thread is on ${MODE_NAMES[caller.mode]}`;
    const needs = (what: string, tool?: string) =>
      new RpcError(ErrorCode.notAllowed, `${who}, so ${what} needs the user's OK. ${tool ? `Use the ${tool} tool, which asks them on a card.` : 'Ask them to do it in the app.'}`);
    switch (method) {
      case 'threads.create': {
        // Left out, a tool's new thread takes the caller's mode, not the
        // runner's default (which may be full).
        const mode = (p.permissionMode ?? caller.mode) as PermissionMode;
        if (above({ mode, confined: false }, caller)) throw needs(`starting a thread with ${MODE_NAMES[mode] ?? mode}`, 'create_thread');
        const given = p.startedBy && typeof p.startedBy === 'object' ? (p.startedBy as Params) : {};
        // Never out of a sandbox from here.
        const { unconfine: _u, ...rest } = p;
        return { ...rest, permissionMode: mode, startedBy: { ...given, kind: 'thread', threadId, label: typeof given.label === 'string' && given.label ? given.label : 'Agent' } };
      }
      case 'threads.send': {
        const id = str(p, 'threadId');
        if (id && id !== threadId) {
          const t = this.targetAccess(id);
          if (above(t, caller)) throw needs(`sending a message to ${this.title(id)}, which has ${MODE_NAMES[t.mode]}`, 'send_message');
        }
        // Only the app's Needs you answers and a phone command passed on
        // say these.
        const { answers: _a, origin: _o, ...rest } = p;
        return rest;
      }
      case 'threads.interrupt': {
        const id = str(p, 'threadId');
        if (id === threadId) throw new RpcError(ErrorCode.notAllowed, "That's the thread this runs in. Just end the turn.");
        return { ...p, byAgent: true };
      }
      case 'threads.update':
        return this.update(threadId, caller, p, needs);
      case 'threads.setProjects': {
        const id = str(p, 'threadId');
        if (id && id !== threadId && above(this.targetAccess(id), caller)) throw needs(`changing the projects of ${this.title(id)}`, 'join_project');
        return { ...p, by: 'agent' };
      }
      case 'threads.showPanel': {
        if (str(p, 'threadId') !== threadId) throw needs("opening another thread's panel", 'show_panel');
        return p;
      }
      case 'schedules.create':
        return this.scheduleCreate(threadId, caller, p, needs);
      case 'schedules.update':
        return this.scheduleUpdate(caller, p, needs);
      case 'schedules.run':
        return this.scheduleRun(caller, p, needs);
      case 'schedules.delete':
        return p;
      default:
        throw needs(`calling ${method}`);
    }
  }

  private update(threadId: string, caller: Authority, p: Params, needs: (what: string, tool?: string) => RpcError): Params {
    const out: Params = { ...p };
    const id = str(p, 'threadId');
    if (!id) return out;
    const self = id === threadId;
    for (const k of Object.keys(p)) {
      if (!['threadId', 'title', 'titleBy', 'cwd', 'accountId', 'poolId', 'model', 'effort', 'permissionMode', 'snoozedUntil', 'archived', 'archivedBy', 'starred'].includes(k)) {
        throw needs(`setting ${k} on a thread`);
      }
    }
    if (self && (p.archived === true || (p.snoozedUntil !== undefined && p.snoozedUntil !== null))) {
      throw new RpcError(ErrorCode.notAllowed, "Can't snooze or archive the thread this runs in from inside its own turn. Call end_turn with archive: true or a snooze instead.");
    }
    if (p.title !== undefined) out.titleBy = 'agent';
    if (p.archived === true) out.archivedBy = 'agent';
    else delete out.archivedBy;
    if (p.permissionMode !== undefined) {
      const to = p.permissionMode as PermissionMode;
      const t = this.savedMode(id);
      if (raises(to, t)) throw needs(`switching ${self ? 'this thread' : this.title(id)} to ${MODE_NAMES[to] ?? to}`, 'update_thread');
      // The mode it already has is left out, so this write can't raise it
      // back if it lands after someone else lowers it.
      if (to === t) delete out.permissionMode;
    }
    if (p.cwd !== undefined || p.accountId !== undefined || p.poolId !== undefined) {
      if (!self && above(this.targetAccess(id), caller)) throw needs(`moving ${this.title(id)}`, 'move_thread');
      // A schedule's check runs in its thread's folder, as the user.
      if (this.deps.schedules?.list(id).some((sc) => sc.threadId === id && sc.check?.trim())) throw needs(`moving ${self ? 'this thread' : this.title(id)}, whose folder a schedule's check runs in`, 'move_thread');
      // Never out of a sandbox from here.
      out.unconfine = false;
    }
    return out;
  }

  private savedMode(threadId: string): PermissionMode {
    const { runner, remote } = this.deps;
    if (runner.has(threadId)) return runner.get(threadId).permissionMode ?? 'full';
    return (remote?.owns(threadId) ? remote.summaries([...ALL]).find((x) => x.id === threadId)?.permissionMode : undefined) ?? 'full';
  }

  private scheduleCreate(threadId: string, caller: Authority, p: Params, needs: (what: string, tool?: string) => RpcError): Params {
    if (typeof p.check === 'string' && p.check.trim()) throw needs('adding a check to a schedule (a shell command that runs by itself)', 'schedule_message');
    const target = str(p, 'threadId');
    if (target) {
      if (target !== threadId && above(this.targetAccess(target), caller)) throw needs(`scheduling a message to ${this.title(target)}`, 'schedule_message');
      return { ...p, createdBy: threadId };
    }
    const mode = (p.permissionMode ?? caller.mode) as PermissionMode;
    if (above({ mode, confined: false }, caller)) throw needs(`scheduling new threads with ${MODE_NAMES[mode] ?? mode}`, 'schedule_message');
    return { ...p, permissionMode: mode, createdBy: threadId };
  }

  private scheduleUpdate(caller: Authority, p: Params, needs: (what: string, tool?: string) => RpcError): Params {
    const out: Params = { ...p };
    const sc = this.findSchedule(p, needs);
    const modes = scheduleModes(sc);
    const was: Authority = sc.threadId ? this.targetAccess(sc.threadId) : { mode: modes.ceiling, confined: false };
    const raised = !sc.threadId && typeof p.permissionMode === 'string' && raises(p.permissionMode as PermissionMode, modes.floor);
    if (raised) throw needs(`switching the schedule "${sc.title}" to ${MODE_NAMES[p.permissionMode as PermissionMode] ?? p.permissionMode}`, 'update_schedule');
    if (typeof p.check === 'string' && p.check.trim()) throw needs('adding a check to a schedule (a shell command that runs by itself)', 'update_schedule');
    if (p.checkTimeoutSecs !== undefined && p.check === undefined && sc.check) throw needs(`changing how long the schedule "${sc.title}"'s shell command may run`, 'update_schedule');
    // Pausing and renaming-free fields never ask; what it sends, where, or
    // that it runs again do, if its runs are above the caller or it has a check.
    const changes = ['text', 'title', 'at', 'cron', 'cwd'].some((k) => p[k] !== undefined) || (p.paused === false && sc.paused);
    if (changes && (above(was, caller) || sc.check)) throw needs(`changing the schedule "${sc.title}"`, 'update_schedule');
    if (p.permissionMode !== undefined && !sc.threadId && p.permissionMode === sc.permissionMode) delete out.permissionMode;
    return out;
  }

  private scheduleRun(caller: Authority, p: Params, needs: (what: string, tool?: string) => RpcError): Params {
    const sc = this.findSchedule(p, needs);
    const will: Authority = sc.threadId ? this.targetAccess(sc.threadId) : { mode: scheduleModes(sc).ceiling, confined: false };
    if (above(will, caller) || sc.check) throw needs(`running the schedule "${sc.title}" now`, 'update_schedule');
    return p;
  }

  private findSchedule(p: Params, needs: (what: string, tool?: string) => RpcError) {
    const id = str(p, 'scheduleId');
    const s = this.deps.schedules;
    if (!id || !s) throw needs('changing a schedule', 'update_schedule');
    try {
      return s.find(id);
    } catch {
      // Unknown here (another runner's): the tools can see further.
      throw needs('changing that schedule', 'update_schedule');
    }
  }
}
