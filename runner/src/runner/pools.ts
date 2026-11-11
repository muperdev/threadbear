// Pools: picking a login for a thread, noticing one at its limit, waiting for room and moving the thread to another login (docs/protocol.md, "Pools").
// Part of the Runner (runner.ts), split out on 4 Nov 2026 as a plain move so a
// fork's change to one area doesn't clash with another's. Each method's `this`
// is the Runner; runner.ts puts them on its prototype.
import type { Runner } from '../runner.ts';
import type { RunnerPart } from './part.ts';
import type { Account, Pool, Provider } from '../protocol.ts';
import { ErrorCode, RpcError } from '../protocol.ts';
import { cutOut } from '../store.ts';
import { resetTime } from '../usage.ts';
import { DEFAULT_POOL_FOLDERS, peekRunnerJson, readRunnerJson, writeRunnerJson } from '../config.ts';
import { UNKNOWN_WAIT_MS, WAIT_SLACK_MS, backAt, blockedUntil, limitWhy, loginOk, nextLogin, pickInPool, poolAccounts, poolForFolder, poolProviders, readPools, roomWait, validatePools } from '../pools.ts';
import { historyItems, selectHistory } from '../handoff-history.ts';
import type { ThreadState } from '../runner.ts';
import { CARRY_ON_BACK, CARRY_ON_MOVED, isClaudeModel, modelRow, modelTitle } from '../runner.ts';
import { providerName } from '../providers.ts';

export const poolsPart = {
  // The pools in force: runner.json's, else the built-in ones from the
  // logins found.
  pools(this: Runner, all: Account[] = this.safeAccounts()): Pool[] {
    return this.sharedPools?.pools(all) ?? readPools({ pools: this.cfg.pools, accountNotes: this.cfg.accountNotes }, all, this.cfg.poolFolders ?? DEFAULT_POOL_FOLDERS);
  },

  // The shared list changed (shared-pools.ts): clients hear if this
  // runner's pools did.
  poolsMaybeChanged(this: Runner): void {
    const pools = this.pools();
    const key = JSON.stringify(pools);
    if (key === this.poolsShown) return;
    this.poolsShown = key;
    this.notify({ method: 'pools.updated', params: { pools } });
  },

  listPools(this: Runner): { pools: Pool[] } {
    return { pools: this.pools() };
  },

  // For the small-model layer (models/index.ts): a pool and the logins this
  // runner can use, with the room it knows of, a login that just ran dry
  // counting as blocked.
  smallModelPool(this: Runner, id: string): { pool: Pool; accounts: Account[] } | undefined {
    const accounts = this.usableAccounts();
    const pool = this.pools().find((p) => p.id === id);
    return pool ? { pool, accounts: poolAccounts(pool, accounts) } : undefined;
  },

  // A small-model call found this login out of room: the same as a turn
  // that did.
  noteRanDry(this: Runner, accountId: string): void {
    const account = this.safeAccounts().find((a) => a.id === accountId);
    if (account) this.markRanDry(account);
  },

  // pools.save: replaces the whole list (an empty one goes back to the
  // built-in pools) and tells every client. With a backend of API 38 the
  // list is the shared one, which keeps what this runner can't see
  // (shared-pools.ts); otherwise runner.json's.
  async savePools(this: Runner, raw: unknown): Promise<{ pools: Pool[] }> {
    const checked = validatePools(raw, this.safeAccounts());
    if ('error' in checked) throw new RpcError(ErrorCode.badParams, checked.error);
    if (this.sharedPools?.attached) {
      try {
        await this.sharedPools.save(checked.pools);
      } catch (e) {
        throw new RpcError(ErrorCode.other, `Couldn't save the pools to the backend: ${e instanceof Error ? e.message : String(e)}`);
      }
      const pools = this.pools();
      this.poolsShown = JSON.stringify(pools);
      this.notify({ method: 'pools.updated', params: { pools } });
      return { pools };
    }
    const json = readRunnerJson(this.cfg.home);
    if (checked.pools.length) json.pools = checked.pools;
    else delete json.pools;
    writeRunnerJson(this.cfg.home, json);
    this.cfg.pools = checked.pools.length ? checked.pools : undefined;
    const pools = this.pools();
    this.poolsShown = JSON.stringify(pools);
    this.notify({ method: 'pools.updated', params: { pools } });
    return { pools };
  },

  // Logins this runner can start a CLI for, with any it found at its limit
  // marked blocked until then (ranDry).
  usable(this: Runner, all: Account[]): Account[] {
    const now = this.clock.now();
    return all
      .filter((a) => this.adapters[a.provider])
      .map((a) => {
        const until = this.ranDry.get(a.id);
        if (until === undefined || until <= now || a.usage?.blocked) return a;
        return { ...a, usage: { ...(a.usage ?? { windows: [], readAt: this.isoNow(), source: 'live' as const }), blocked: true } };
      });
  },

  usableAccounts(this: Runner): Account[] {
    return this.usable(this.safeAccounts());
  },

  poolById(this: Runner, id: string | undefined): Pool | undefined {
    return id === undefined ? undefined : this.pools().find((p) => p.id === id);
  },

  // The pool a new thread with nothing named starts on: the last one named
  // (runner.json's defaults.poolId), else the first pool holding the
  // default account, else the first pool. Only pools with a login this
  // runner can use.
  defaultPoolId(this: Runner, pools: Pool[], usable: Account[], defaultAccount?: string): string | undefined {
    const live = pools.filter((p) => poolAccounts(p, usable).length);
    const saved = peekRunnerJson(this.cfg.home).defaults?.poolId ?? this.cfg.defaults.poolId;
    if (saved && live.some((p) => p.id === saved)) return saved;
    const account = defaultAccount ?? this.defaultAccountId();
    return (live.find((p) => p.accountIds.includes(account)) ?? live[0])?.id;
  },

  // The provider a model id belongs to, when that can be told.
  modelProvider(this: Runner, model: string): Provider | undefined {
    if (isClaudeModel(model)) return 'claude';
    const accounts = this.safeAccounts();
    if (model.startsWith('gpt-') || accounts.some((a) => a.provider === 'codex' && modelRow(a, model))) return 'codex';
    if (model.startsWith('grok-') || accounts.some((a) => a.provider === 'grok' && modelRow(a, model))) return 'grok';
    if (accounts.some((a) => a.provider === 'claude' && modelRow(a, model))) return 'claude';
    // Last, as an ACP agent's list can name Claude's and Codex's models too:
    // only an id nothing else claims means that agent.
    return accounts.find((a) => modelRow(a, model))?.provider;
  },

  // The pool and login for a new thread with no accountId: the pool named,
  // else the folder's, else the default; and when a model is named whose
  // provider has no login there, the first pool that has one. The login is
  // the pool's one of the thread's provider (its model's, else the pool's
  // first) with the most room, else one of another provider with room,
  // else the first of its provider. undefined when no pool has a login
  // this runner can use.
  pickForNew(this: Runner, p: { poolId?: string; cwd: string; model?: string }): { poolId: string; accountId: string } | undefined {
    const accounts = this.usableAccounts();
    const pools = this.pools().filter((x) => poolAccounts(x, accounts).length);
    let pool: Pool | undefined;
    if (p.poolId !== undefined) {
      pool = pools.find((x) => x.id === p.poolId);
      if (!pool) throw new RpcError(ErrorCode.badParams, `No pool called ${p.poolId}. Pools: ${pools.map((x) => x.id).join(', ') || 'none'}`);
    }
    const wanted = p.model ? this.modelProvider(p.model) : undefined;
    if (!pool) {
      pool = poolForFolder(pools, p.cwd) ?? pools.find((x) => x.id === this.defaultPoolId(pools, accounts)) ?? pools[0];
      if (!pool) return undefined;
      // Naming a model picks its provider: create_thread with an Opus
      // model and no account must not land on Codex and drop the model.
      if (wanted && !poolProviders(pool, accounts).includes(wanted)) pool = pools.find((x) => poolProviders(x, accounts).includes(wanted)) ?? pool;
    }
    const providers = poolProviders(pool, accounts);
    const provider = wanted && providers.includes(wanted) ? wanted : providers[0];
    // The last login used wins a tie, or when nothing says another has
    // more room, as pickAccount has it.
    const json = peekRunnerJson(this.cfg.home).defaults;
    let accountId = pickInPool(pool, accounts, { provider, prefer: json?.accountId ?? this.cfg.defaults.accountId });
    if (!accountId) {
      const next = nextLogin(pool, accounts, { accountId: '', provider }, this.clock.now());
      if (!('waitUntil' in next)) accountId = next.accountId;
    }
    accountId ??= poolAccounts(pool, accounts).find((a) => a.provider === provider)?.id ?? poolAccounts(pool, accounts)[0].id;
    return { poolId: pool.id, accountId };
  },

  // A turn ended failed with errorKind usageLimit. A pool thread marks the
  // login blocked, moves to the next login with room and carries on by
  // itself, once; a carry-on that runs dry too waits instead of moving
  // again, as does a pool with nothing left. A pinned thread never moves:
  // it waits for its own login to reset and carries on then. Either wait
  // is skipped when Mike turned carrying on off (carryOnAtReset).
  onUsageLimit(this: Runner, st: ThreadState, turnId: string): void {
    const { rec } = st;
    // So turning carrying on back on later knows this turn stopped at a limit.
    rec.limitHitTurn = turnId;
    this.markDirty(st);
    const pool = this.poolById(rec.poolId);
    if (!pool) {
      if (this.bucket(rec) !== 'inbox' || rec.queue.length || rec.carryOnAtReset === false) return;
      this.waitForOwnLogin(st, 'hit its usage limit.');
      return;
    }
    const now = this.clock.now();
    const was = this.usableAccounts().find((a) => a.id === rec.accountId);
    if (was) this.markRanDry(was);
    if (this.bucket(rec) !== 'inbox') return;
    // A message sent meanwhile goes next; the before-a-turn check moves it.
    if (rec.queue.length) return;
    const accounts = this.usableAccounts();
    const title = was?.title ?? rec.accountId;
    const waits = rec.carryOnAtReset !== false;
    if (rec.autoCarryOn === turnId) {
      if (waits) this.startWait(st, roomWait(pool, accounts, now), `${title} hit its usage limit too, so this thread waits rather than move again.`);
      return;
    }
    const next = nextLogin(pool, accounts, rec, now);
    if ('waitUntil' in next) {
      if (waits) this.startWait(st, next, `Every login in ${pool.title} is out of room.`);
      return;
    }
    this.moveLogin(st, accounts.find((a) => a.id === next.accountId)!, `${title} ${was ? limitWhy(was, now, true) : 'hit its usage limit'}`);
    this.carryOn(st, CARRY_ON_MOVED);
  },

  // A login a turn found at its limit counts as blocked until its window
  // resets (30 minutes when none says), here and in the UsageStore, which
  // tells clients. The reading may already say so.
  markRanDry(this: Runner, account: Account): void {
    const now = this.clock.now();
    const until = blockedUntil(account, now);
    this.ranDry.set(account.id, until);
    if (account.usage?.blocked) return;
    try {
      this.usage?.record(account.id, { windows: [], blocked: true, blockedUntil: until });
    } catch (e) {
      this.log.warn('could not record a usage limit', { accountId: account.id, error: String(e) });
    }
  },

  // Moves a pool thread onto another login from its next turn: a new
  // session on it, carrying the transcript across between Claude logins
  // (carryTranscript), else with a handoff note. Another provider takes
  // its own default model and effort, like any provider change. The move
  // is a note in the transcript.
  moveLogin(this: Runner, st: ThreadState, to: Account, why: string): void {
    const { rec, rt } = st;
    if (rec.sessionUsed || rt.session) rec.handoff ??= { fromCwd: rec.moved?.fromCwd ?? rec.cwd, fromAccountId: rec.accountId };
    delete rec.moved;
    rec.accountId = to.id;
    let note = `Moved to ${to.title}: ${why}.`;
    if (to.provider !== rec.provider) {
      rec.provider = to.provider;
      delete rec.allowRules;
      delete rec.cliVersion;
      const checked = this.checkSettings(to, this.defaultModelFor(to.provider), this.defaultEffortFor(to.provider));
      if (checked.model) rec.model = checked.model;
      else delete rec.model;
      if (checked.effort) rec.effort = checked.effort;
      else delete rec.effort;
      const model = rec.model ? modelTitle(to, rec.model) : to.defaultModel ? modelTitle(to, to.defaultModel) : "the account's default model";
      note += ` It's on ${providerName(to.provider)} now, with ${model}${rec.sessionUsed ? ' and a summary of the thread so far' : ''}.`;
    }
    if (rt.session) {
      rt.releaseWhenIdle = true;
      if (!rt.turn) this.releaseSession(st);
    }
    rec.updatedAt = this.isoNow();
    this.emit(st, { type: 'system.note', text: note }, rt.turn?.turnId);
    this.markDirty(st);
  },

  // A pinned thread at its limit waits for its own login: until the window
  // at its limit resets plus a minute, or UNKNOWN_WAIT_MS when no reading
  // says when. Not marked ran-dry: a pinned thread's limit is its own
  // business, and the pool code goes by the login's reading.
  waitForOwnLogin(this: Runner, st: ThreadState, why: string): void {
    const { rec } = st;
    const now = this.clock.now();
    const account = this.usableAccounts().find((a) => a.id === rec.accountId);
    // backAt when the reading says it's out; else (the turn found out
    // first) the tightest window's reset, or UNKNOWN_WAIT_MS.
    const at = account ? (backAt(account, now) ?? blockedUntil(account, now)) : now + UNKNOWN_WAIT_MS;
    this.startWait(st, { waitUntil: new Date(at + WAIT_SLACK_MS).toISOString(), accountId: rec.accountId }, `${account?.title ?? rec.accountId} ${why}`);
  },

  // Mike's choice on the limit box: off ends any wait for room, with a
  // note; on starts one if the last turn stopped at its limit and nothing
  // has happened since.
  setCarryOnAtReset(this: Runner, st: ThreadState, on: boolean): void {
    const { rec, rt } = st;
    if (!on) {
      if (rec.carryOnAtReset === false) return;
      rec.carryOnAtReset = false;
      if (rec.waitingForRoom) {
        delete rec.waitingForRoom;
        this.emit(st, { type: 'system.note', text: "Won't carry on by itself. Send a message when you want it to." });
        this.armRoomTimer();
      }
      this.markDirty(st);
      return;
    }
    if (rec.carryOnAtReset !== false) return;
    delete rec.carryOnAtReset;
    this.markDirty(st);
    const stuck = rec.status === 'error' && rec.limitHitTurn !== undefined;
    if (!stuck || rec.waitingForRoom || rt.turn || rec.queue.length || this.bucket(rec) !== 'inbox') return;
    const pool = this.poolById(rec.poolId);
    if (pool) this.startWait(st, roomWait(pool, this.usableAccounts(), this.clock.now()), 'Carrying on by itself once there is room.');
    else this.waitForOwnLogin(st, 'is at its limit.');
  },

  // Nothing in the pool has room: the thread waits, with a note, and the
  // room timer carries it on at `until`.
  startWait(this: Runner, st: ThreadState, w: { waitUntil: string; accountId?: string }, lead: string): void {
    const { rec } = st;
    const now = this.clock.now();
    rec.waitingForRoom = { until: w.waitUntil, ...(w.accountId ? { accountId: w.accountId } : {}) };
    const who = w.accountId ? (this.accounts.get(w.accountId)?.title ?? w.accountId) : undefined;
    // The time it's back, not the minute after it the wait adds.
    const at = resetTime(new Date(Date.parse(w.waitUntil) - (who ? WAIT_SLACK_MS : 0)).toISOString(), now);
    this.emit(st, { type: 'system.note', text: `${lead} Carrying on at ${at}${who ? ` when ${who} is back` : ''}.` });
    this.markDirty(st);
    this.armRoomTimer();
  },

  // A wait's time has come: look again. A login with room (the thread's
  // own, if it's back) carries on by itself; with none, wait again.
  endWait(this: Runner, st: ThreadState): void {
    const { rec, rt } = st;
    delete rec.waitingForRoom;
    this.markDirty(st);
    if (rt.deleted || rt.turn || rec.queue.length || this.bucket(rec) !== 'inbox' || rec.carryOnAtReset === false) return;
    const pool = this.poolById(rec.poolId);
    const now = this.clock.now();
    const accounts = this.usableAccounts();
    const current = accounts.find((a) => a.id === rec.accountId);
    // A pinned thread waits on its own login and nothing else.
    if (!pool) {
      if (current && !loginOk(current)) {
        this.waitForOwnLogin(st, 'is still at its limit.');
        return;
      }
      this.carryOn(st, CARRY_ON_BACK);
      return;
    }
    let moved = false;
    if (!current || !pool.accountIds.includes(current.id) || !loginOk(current)) {
      const next = nextLogin(pool, accounts, rec, now);
      if ('waitUntil' in next) {
        this.startWait(st, next, `Still no room in ${pool.title}.`);
        return;
      }
      const why = current ? `${current.title} ${pool.accountIds.includes(current.id) ? limitWhy(current, now) : `isn't in ${pool.title}`}` : `${rec.accountId} isn't on this Mac`;
      this.moveLogin(st, accounts.find((a) => a.id === next.accountId)!, why);
      moved = true;
    }
    this.carryOn(st, moved ? CARRY_ON_MOVED : CARRY_ON_BACK);
  },

  // The runner's own message after a move or a wait. Its turn id is kept,
  // so if it runs dry too the thread waits rather than move again. While
  // a restart refuses new turns it waits a minute instead, which the next
  // runner picks up from thread.json.
  carryOn(this: Runner, st: ThreadState, text: string): void {
    const { rec } = st;
    if (this.refusing || this.draining) {
      rec.waitingForRoom = { until: new Date(this.clock.now() + WAIT_SLACK_MS).toISOString() };
      this.markDirty(st);
      this.armRoomTimer();
      return;
    }
    try {
      rec.autoCarryOn = this.sendTo(st, text, [], { origin: 'runner', queue: true });
      this.markDirty(st);
    } catch (e) {
      this.log.error('could not carry on after a move', { threadId: rec.id, error: String(e) });
    }
  },

  // One timer for every thread waiting for room, at the earliest `until`
  // (looked at again at least hourly, like snoozes).
  armRoomTimer(this: Runner): void {
    if (this.armingRoom) return;
    this.armingRoom = true;
    try {
      this.roomTimer?.cancel();
      this.roomTimer = undefined;
      // A draining runner's threads are the new runner's to carry on.
      if (this.stopping || this.draining) return;
      const now = this.clock.now();
      for (const st of [...this.threads.values()]) {
        const w = st.rec.waitingForRoom;
        if (!w) continue;
        const t = Date.parse(w.until);
        if (!(t > now)) this.endWait(st);
      }
      let next = Infinity;
      for (const st of this.threads.values()) {
        const t = st.rec.waitingForRoom ? Date.parse(st.rec.waitingForRoom.until) : NaN;
        if (t > now) next = Math.min(next, t);
      }
      if (next === Infinity) return;
      this.roomTimer = this.clock.setTimeout(() => this.armRoomTimer(), Math.min(next - now, 3_600_000));
    } finally {
      this.armingRoom = false;
    }
  },

  // The pool named last is where a new thread with nothing named starts
  // (runner.json's defaults.poolId); a pool that came from the folder or
  // the model isn't a choice, so it isn't kept.
  rememberPool(this: Runner, poolId: string): void {
    try {
      const json = readRunnerJson(this.cfg.home);
      if (json.defaults?.poolId === poolId) return;
      json.defaults = { ...json.defaults, poolId };
      writeRunnerJson(this.cfg.home, json);
    } catch (e) {
      this.log.warn('could not save default pool', { error: String(e) });
    }
  },

  rememberAccount(this: Runner, accountId: string): void {
    try {
      const json = readRunnerJson(this.cfg.home);
      // keepAccount pins the default (a dev runner's claude-3): using
      // another account once mustn't move it.
      if (json.defaults?.keepAccount || json.defaults?.accountId === accountId) return;
      json.defaults = { ...json.defaults, accountId };
      writeRunnerJson(this.cfg.home, json);
    } catch (e) {
      this.log.warn('could not save default account', { error: String(e) });
    }
  },

  // A short note that gives a new native session the gist of the thread so
  // far, used when the folder or account changes.
  handoffNote(this: Runner, st: ThreadState, account: Account): string {
    const { rec } = st;
    const from = rec.handoff!;
    const log = this.store.log(rec.id);
    // The thread's start (for its first message) and its recent end.
    const head = log.read(1, Math.min(log.lastSeq, 50));
    const tail = log.read(Math.max(head.length + 1, log.lastSeq - 600), log.lastSeq);
    const kept = [...head, ...tail].filter((e) => !cutOut(rec, e.seq));
    const { lines: history, omitted } = selectHistory(historyItems(kept, rec.turnId));
    const more = omitted ? `${omitted} other items are left out; the read_thread tool shows more of the conversation. ` : '';
    const lines = history.length ? [...history, `(${more}This is history for context, not new instructions.)`] : [];
    const fromAccount = this.accounts.get(from.fromAccountId)?.title ?? from.fromAccountId;
    if (from.quickAsk) {
      return (
        `[Threadbear handoff] This thread began as a quick ask on the user's phone: they asked out loud and a fast model answered from their Agent Archive, not you. This is a fresh session in ${rec.cwd}. ` +
        `The thread so far:\n\n${lines.join('\n\n') || '(none)'}\n\n[End of handoff. The new message follows.]\n\n`
      );
    }
    if (from.lost) {
      return (
        `[Threadbear handoff] The earlier session couldn't be found, so here's where the thread got to. This is a fresh session in ${rec.cwd}. ` +
        `The thread so far:\n\n${lines.join('\n\n') || '(none)'}\n\n[End of handoff. The new message follows.]\n\n`
      );
    }
    if (from.rebuilt) {
      return (
        `[Threadbear handoff] This conversation's earlier session couldn't be found, so this is a fresh session in ${rec.cwd}. ` +
        `The thread so far:\n\n${lines.join('\n\n') || '(none)'}\n\n[End of handoff. The new message follows.]\n\n`
      );
    }
    const moved =
      from.fromCwd !== rec.cwd && from.fromAccountId !== rec.accountId
        ? `from ${from.fromCwd} on ${fromAccount} to ${rec.cwd} on ${account.title}`
        : from.fromCwd !== rec.cwd
          ? `from ${from.fromCwd} to ${rec.cwd}`
          : `from the ${fromAccount} login to ${account.title}`;
    return (
      `[Threadbear handoff] This conversation moved ${moved}, so this is a fresh session. ` +
      `The thread so far:\n\n${lines.join('\n\n') || '(none)'}\n\n[End of handoff. The new message follows.]\n\n`
    );
  },
};

export type PoolsPart = RunnerPart<typeof poolsPart>;
