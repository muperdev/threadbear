// Thread worktrees: making one before a turn, its Land action, and cleaning up (docs/thread-worktrees.md).
// Part of the Runner (runner.ts), split out on 4 Nov 2026 as a plain move so a
// fork's change to one area doesn't clash with another's. Each method's `this`
// is the Runner; runner.ts puts them on its prototype.
import type { Runner } from '../runner.ts';
import type { RunnerPart } from './part.ts';
import type { Action, EventBody } from '../protocol.ts';
import type { ThreadRecord } from '../store.ts';
import { BIN_DIR, threadEnv } from '../restart.ts';
import { cleanUpWorktree, makeWorktree, readPlainFile, sameSubfolder, stillCheckedOut, worktreeFolderNow, worktreeGone, type WorktreeRecord } from '../worktrees.ts';
import { isDirectory } from '../fsutil.ts';
import { automaticTrust, findWorktreePatch, isInside, readWorktreePatch, runCommand, type PatchSet, type WorktreePatch, worktreePatchFiles } from '../patches.ts';
import fs from 'node:fs';
import path from 'node:path';
import type { ThreadState } from '../runner.ts';

export const worktreesPart = {
  // ------------------------------------------------------------ worktrees
  // A worktree per thread (docs/thread-worktrees.md, worktrees.ts).

  // Whether a new thread in `cwd` might get a worktree: there's at least
  // one patch file in its own folder chain to read.
  mayWantWorktree(this: Runner, cwd: string): boolean {
    try {
      return worktreePatchFiles(cwd, this.panelsRoot).length > 0;
    } catch {
      return false;
    }
  },

  // Before a new thread's first turn: if its repo's patch asks for a
  // worktree, makes one, moves the thread into it and runs the patch's
  // setup there. Anything going wrong is said, and the thread starts
  // where it was asked to.
  async prepareWorktree(this: Runner, st: ThreadState, turnId: string): Promise<void> {
    const { rec, rt } = st;
    delete rec.worktreePending;
    this.markDirty(st);
    // A session already ran in the folder (a turn that failed later, say):
    // moving it now would leave that session behind.
    if (rec.sessionUsed || rec.worktree) return;
    let patch;
    try {
      patch = await findWorktreePatch(rec.cwd, this.panelsRoot, this.patchCache, undefined, this.patchTrust);
    } catch (e) {
      this.log.warn('could not read patches for a worktree', { threadId: rec.id, error: String(e) });
      return;
    }
    if (!patch || rt.deleted) return;
    const asked = rec.cwd;
    const made = await makeWorktree({ cwd: asked, threadId: rec.id, patch });
    if (rt.deleted) return;
    if (!made.ok) {
      this.log.warn('could not make a worktree', { threadId: rec.id, reason: made.reason });
      this.emit(st, { type: 'system.note', text: `Couldn't make a worktree for this thread: ${made.reason}. It starts in ${asked} instead.` }, turnId);
      return;
    }
    // Made afresh from main (or a spare reset to it): what a move left
    // there is gone, so the checkout's own trust reaches it again.
    this.patchTrust.vouch?.(made.worktree.path);
    rec.worktree = made.worktree;
    rec.cwd = made.cwd;
    rec.updatedAt = this.isoNow();
    this.markDirty(st);
    this.log.info('made a worktree', { threadId: rec.id, path: made.worktree.path, branch: made.worktree.branch });
    this.emit(st, { type: 'system.note', text: `Working in a worktree of its own at ${made.worktree.path}, on the branch ${made.worktree.branch}. tb land puts it on main.` }, turnId);
    await this.setUpWorktree(st, made, patch, turnId);
  },

  // A new worktree's setup (the patch's `setup`), run from the worktree's
  // own copy of the patch once that's trusted. A moved thread's worktree
  // (runner/moves.ts) runs it too.
  async setUpWorktree(this: Runner, st: ThreadState, made: { worktree: WorktreeRecord; patchFile: string }, patch: WorktreePatch, turnId?: string): Promise<void> {
    const { rec, rt } = st;
    if (!patch.setup) return;
    const copy = await this.worktreeCopy(made.patchFile, patch.patch, 'setup');
    if (rt.deleted) return;
    if ('skip' in copy) {
      this.log.warn("didn't run a worktree's setup", { threadId: rec.id, patch: made.patchFile, reason: copy.skip });
      this.emit(st, { type: 'system.note', text: `Didn't run the worktree's setup (${patch.setup}): ${copy.skip}` }, turnId);
      return;
    }
    // Trusted, but asking for no setup there: nothing to run.
    const setup = { command: copy.patch?.setup, run: copy.patch?.setupRun };
    if (!setup.command) return;
    // Bound to a hashed script, it runs from the worktree: the copy must
    // still hash to the version that approved it, read afresh just now.
    if (setup.run) {
      const by = automaticTrust(setup.run);
      if (!by || !this.patchTrust.verify(by.file, by.hash)) {
        this.emit(st, { type: 'system.note', text: `Didn't run the worktree's setup (${patch.setup}): its patch or a script it runs changed since it was checked.` }, turnId);
        return;
      }
    }
    const outcome = await runCommand(setup.run ?? setup.command, {
      cwd: made.worktree.path,
      env: { ...process.env, ...threadEnv(rec.id, this.cfg.home) },
      shell: this.actionShell,
      timeoutMs: this.actionTimeoutMs,
      background: this.backgroundJobs,
    });
    if (rt.deleted) return;
    const body: EventBody = {
      type: 'action.run',
      actionId: `${patch.patch}.worktree-setup`,
      patch: patch.patch,
      title: 'Set up the worktree',
      command: setup.command,
      ok: outcome.ok,
      text: outcome.text,
      truncated: outcome.truncated,
      durationMs: outcome.durationMs,
      trigger: 'worktree.setup',
    };
    if (outcome.exitCode !== undefined) body.exitCode = outcome.exitCode;
    this.emit(st, body, rt.turn?.turnId ?? turnId);
    this.markDirty(st);
  },

  // The worktree's own copy of the patch, for its setup or teardown. The
  // patch trusted in the main checkout isn't what runs there: the worktree
  // is main's commit, which can differ from the main checkout's files (an
  // uncommitted edit, a script changed on main), and by the time it's
  // archived it's the thread's branch, which anything can have changed (a
  // pull, a commit from someone else). Both run by themselves. So that copy,
  // with the scripts it names as they are in the worktree, must be a version
  // Mike trusted (Daybreak, 5 Nov). Threadbear's own worktrees count as its
  // checkout, so stay trusted. `patch` is absent when it has no `worktree`.
  async worktreeCopy(this: Runner, copy: string, id: string, what: 'setup' | 'teardown'): Promise<{ patch?: WorktreePatch } | { skip: string }> {
    let verdict;
    try {
      verdict = await this.patchTrust.check(copy, fs.statSync(copy).mtimeMs);
    } catch (e) {
      return { skip: `couldn't read its copy of the ${id} patch (${String((e as Error).message ?? e)}).` };
    }
    if (!verdict.trusted) {
      if (verdict.reason === 'unchecked') return { skip: `its copy of the ${id} patch can't be checked: ${verdict.problem}` };
      return { skip: `its copy of the ${id} patch isn't a version you've trusted. Trust it in Patches, then run the ${what} yourself if you want it.` };
    }
    let again;
    try {
      again = await readWorktreePatch(copy, this.patchCache, undefined, this.patchTrust);
    } catch (e) {
      return { skip: `couldn't load its copy of the ${id} patch (${String((e as Error).message ?? e)}).` };
    }
    // Refused as it loaded: a setup that isn't one hashed script, say.
    if (again && 'error' in again) return { skip: `its copy of the ${id} patch didn't load. ${again.error}` };
    return again ? { patch: again } : {};
  },

  // The teardown to run as a worktree is cleaned up, from the worktree's own
  // copy of the patch that asked for it, trusted as setup's is: what's there
  // now is the thread's branch, so a teardown changed on it needs trusting
  // like any other change to the patch. Undefined when there's none to run.
  async worktreeTeardownCommand(this: Runner, st: ThreadState): Promise<{ command: string; run?: string; patch: string } | { skip: string } | undefined> {
    const { rec } = st;
    const w = rec.worktree!;
    const from = isInside(rec.cwd, w.path) && isDirectory(rec.cwd) ? rec.cwd : w.path;
    // Older records don't say which patch it was: the nearest one in the
    // worktree with a `worktree`, as findWorktreePatch picks.
    const files = w.patchFile ? [path.join(w.path, w.patchFile)] : worktreePatchFiles(from, this.panelsRoot).filter((f) => isInside(f, w.path));
    for (const file of files) {
      if (!fs.existsSync(file)) continue;
      const id = path.basename(file, path.extname(file));
      const copy = await this.worktreeCopy(file, id, 'teardown');
      if ('skip' in copy) {
        // An untrusted copy can't be loaded to see if it has a teardown, so
        // it's only worth a note when it looks like it might: most don't.
        // One that isn't a plain file of a sensible size (a fifo, which
        // would hold the runner up for ever: Codex's review, 9 Nov) might.
        const text = readPlainFile(file);
        if (text === undefined || /\bteardown\b/.test(text)) return { skip: copy.skip };
        continue;
      }
      if (!copy.patch) continue;
      return copy.patch.teardown ? { command: copy.patch.teardown, run: copy.patch.teardownRun, patch: copy.patch.patch } : undefined;
    }
    return undefined;
  },

  // Runs the patch's teardown in the worktree's root, on every archive and
  // delete, landed or not, before the cleanup decides what goes: what setup
  // made outside the folder (a Convex deployment per worktree) would
  // otherwise outlive the thread, and a branch landed through a squashed PR
  // never counts as merged (elie-h's fork, approved 9 Nov). A kept spare's
  // too: what the thread made is its own, not the next thread's. Once per
  // archive (tornDownAt). Its output goes in the transcript as an
  // action.run; a deleted thread has no transcript, so a notice says if it
  // didn't run or failed. A failure never stops the cleanup.
  async tearDownWorktree(this: Runner, st: ThreadState, deleted: boolean, stillOurs: () => boolean): Promise<void> {
    const { rec } = st;
    const w = rec.worktree;
    if (!w || w.removedAt || w.tornDownAt) return;
    // Only in the thread's own checkout: a slot folder can be another
    // thread's by now (THREADBEAR-16), and its teardown isn't ours to run.
    if (!(await stillCheckedOut(w)) || !stillOurs()) return;
    const say = (text: string) => {
      if (deleted) this.notice('info', `"${rec.title}" was deleted, but ${text}`);
      else if (!st.rt.deleted) {
        this.emit(st, { type: 'system.note', text });
        this.markDirty(st);
      }
    };
    let found;
    try {
      found = await this.worktreeTeardownCommand(st);
    } catch (e) {
      found = { skip: `couldn't read its patch (${String((e as Error).message ?? e)}).` };
    }
    if (!found || !stillOurs()) return;
    if ('skip' in found) {
      this.log.warn("didn't run a worktree's teardown", { threadId: rec.id, path: w.path, reason: found.skip });
      say(deleted ? `its worktree's teardown didn't run: ${found.skip}` : `Didn't run the worktree's teardown: ${found.skip}`);
      return;
    }
    // Bound to a hashed script, it runs from the worktree: the copy must
    // still hash to the version that approved it, read afresh just now.
    if (found.run) {
      const by = automaticTrust(found.run);
      if (!by || !this.patchTrust.verify(by.file, by.hash)) {
        say(deleted ? `its worktree's teardown (${found.command}) didn't run: its patch or a script it runs changed since it was checked.` : `Didn't run the worktree's teardown (${found.command}): its patch or a script it runs changed since it was checked.`);
        return;
      }
    }
    // The last check, with nothing awaited between it and the start: the
    // folder can have been swapped (for a link to the main checkout, say)
    // while the patch was read, and it runs by the path checked.
    const cwd = worktreeFolderNow(w);
    if (!cwd) {
      this.log.warn("didn't run a worktree's teardown: its folder changed", { threadId: rec.id, path: w.path });
      return;
    }
    const outcome = await runCommand(found.run ?? found.command, {
      cwd,
      env: { ...process.env, ...threadEnv(rec.id, this.cfg.home) },
      shell: this.actionShell,
      timeoutMs: this.actionTimeoutMs,
      background: this.backgroundJobs,
      // The cleanup, and everything waiting on it, waits for this: nothing
      // it leaves running can hold the pipes open past it, as for hooks.
      killGroup: true,
    });
    w.tornDownAt = this.isoNow();
    this.log.info('worktree teardown', { threadId: rec.id, path: w.path, ok: outcome.ok, exitCode: outcome.exitCode });
    if (deleted) {
      if (!outcome.ok) {
        const end = outcome.text.trim().split('\n').slice(-5).join('\n');
        const code = outcome.exitCode !== undefined ? ` (exit code ${outcome.exitCode})` : '';
        this.notice('info', `"${rec.title}" was deleted, but its worktree's teardown (${found.command}) failed${code}${end ? `: ${end}` : '.'}`);
      }
      return;
    }
    // Deleted meanwhile: its own cleanup skips the teardown (tornDownAt).
    if (st.rt.deleted) return;
    const body: EventBody = {
      type: 'action.run',
      actionId: `${found.patch}.worktree-teardown`,
      patch: found.patch,
      title: 'Tear down the worktree',
      command: found.command,
      ok: outcome.ok,
      text: outcome.text,
      truncated: outcome.truncated,
      durationMs: outcome.durationMs,
      trigger: 'worktree.teardown',
    };
    if (outcome.exitCode !== undefined) body.exitCode = outcome.exitCode;
    this.emit(st, body);
    this.markDirty(st);
  },

  // A thread brought back to a worktree that stayed (its work wasn't on
  // main) carries on there, as before teardowns. But its teardown ran, so
  // what setup made outside the folder may be gone: its next turn says so,
  // to Mike and the agent, rather than setup quietly running again (setup
  // is written for a fresh worktree). Returns the line for the agent.
  tellTornDown(this: Runner, st: ThreadState, turnId: string): string | undefined {
    const { rec } = st;
    const w = rec.worktree;
    if (!w?.tornDownAt || w.removedAt || !isInside(rec.cwd, w.path)) return undefined;
    delete w.tornDownAt;
    this.markDirty(st);
    this.emit(st, { type: 'system.note', text: "This thread's worktree was torn down when it was archived (its patch's teardown ran), so anything its setup made outside the folder may be gone. It carries on in the worktree." }, turnId);
    return `[Threadbear] Your worktree's teardown ran when this thread was archived, so anything its setup made outside ${w.path} (a database, a deployment) may be gone. Run the worktree's setup again if you need it.`;
  },

  // A thread whose worktree was removed (it was archived with everything
  // landed) and has now come back carries on in the main checkout. Returns
  // a line for the agent, whose system prompt still names the worktree.
  // The thread's worktree has gone since its CLI last ran, so its next
  // start moves it to the main checkout (leaveRemovedWorktree).
  // Whether a folder is at that path now says nothing: slot folders are
  // reused, so one there is a spare, another thread's or a borrowed one.
  // On 8 Nov a removed slot-10 was made again by borrow-worktree.sh, and
  // the thread brought back carried on in it, beside the borrower's work.
  leavesRemovedWorktree(this: Runner, rec: ThreadRecord): boolean {
    const w = rec.worktree;
    return Boolean(w?.removedAt && isInside(rec.cwd, w.path));
  },

  leaveRemovedWorktree(this: Runner, st: ThreadState, turnId: string): string | undefined {
    const { rec } = st;
    const w = rec.worktree;
    if (!w || !this.leavesRemovedWorktree(rec)) return undefined;
    const from = rec.cwd;
    const to = sameSubfolder(from, w.path, w.mainCheckout);
    if (rec.sessionUsed) {
      if (this.adapters[rec.provider]?.movesFolders) rec.moved ??= { fromCwd: from };
      else rec.handoff ??= { fromCwd: from, fromAccountId: rec.accountId };
    }
    rec.cwd = to;
    rec.updatedAt = this.isoNow();
    this.markDirty(st);
    this.tellActivity({ kind: 'moved', threadId: rec.id, from, to });
    const went = w.spare ? 'was handed on to other threads' : 'was removed';
    this.emit(st, { type: 'system.note', text: `This thread's worktree ${went} when it was archived, so it carries on in the main checkout, ${to}.` }, turnId);
    return `[Threadbear] Your worktree at ${w.path} ${went} after its work landed on main. You're now in the main checkout, ${to}, which other threads share: stage your own files by name.`;
  },

  // The Land chip, for a thread with a worktree of its own.
  worktreeAction(this: Runner, rec: ThreadRecord): Action | undefined {
    const w = rec.worktree;
    if (!w || w.removedAt) return undefined;
    // This runner's own tb, in full: a login shell's PATH may not have it.
    const tb = path.join(BIN_DIR, 'tb');
    const quoted = /^[\w./-]+$/.test(tb) ? tb : `'${tb.replace(/'/g, `'\\''`)}'`;
    return {
      id: 'worktree.land',
      title: 'Land',
      kind: 'command',
      command: `${quoted} land`,
      patch: 'worktree',
      note: `Rebase ${w.branch} on main, run the check, then move main to it`,
    };
  },

  actionsOf(this: Runner, st: ThreadState, set: PatchSet): Action[] {
    const land = this.worktreeAction(st.rec);
    return land ? [...set.actions, land] : set.actions;
  },

  // At start (and when a thread is handed over): an archived thread whose
  // worktree and branch have gone (or whose folder another thread has
  // taken) but whose record never said so (a handover saved it mid-cleanup
  // before 6 Nov, THREADBEAR-16, or the runner stopped there) is told now,
  // so it shows no Land chip and, brought back, never carries on in the
  // folder. Each check counts as the thread's cleanup, so a message sent
  // meanwhile waits for it (deliver).
  settleLostCleanups(this: Runner, threads: Iterable<ThreadState> = this.threads.values(), retry = true): Promise<void> {
    const checks = [];
    for (const st of threads) {
      const { rec, rt } = st;
      const w = rec.worktree;
      if (!rec.archivedAt || !w || w.removedAt || rt.cleaning) continue;
      // Archived, but the runner went before the cleanup ran: it's still to
      // do, not just to record. Not from a handover: one that comes with this
      // still owed gave up waiting for its git (letGo), which may still be
      // removing the folder, and a second one beside it could read that as
      // uncommitted changes. The next start does it.
      const check = (retry && w.cleanupOwed ? this.cleanUpWorktree(st) : worktreeGone(w).then((gone) => {
          if (!gone || rt.deleted || w.removedAt) return;
          w.removedAt = this.isoNow();
          if (gone === 'taken') w.spare = true;
          this.markDirty(st);
          this.log.info("recorded a worktree's removal the record missed", { threadId: rec.id, path: w.path });
        }))
        .catch((e: unknown) => this.log.warn("couldn't check an archived thread's worktree", { threadId: rec.id, error: String(e) }))
        .finally(() => {
          if (rt.cleaning === check) rt.cleaning = undefined;
        });
      rt.cleaning = check;
      checks.push(check);
    }
    return Promise.all(checks).then(() => undefined);
  },

  // When the thread is archived or deleted: its worktree and branch go if
  // everything on them is on main; otherwise they stay, and the thread (or,
  // once it's deleted, a notice) says so and how to land or drop them.
  async cleanUpWorktree(this: Runner, st: ThreadState, deleted = false): Promise<void> {
    const { rec } = st;
    const w = rec.worktree;
    if (!w || w.removedAt) return;
    const stillOurs = () => deleted || !st.rt.deleted;
    try {
      await this.tearDownWorktree(st, deleted, stillOurs);
    } catch (e) {
      this.log.error('worktree teardown failed', { threadId: rec.id, error: String(e) });
    }
    let result;
    try {
      result = await cleanUpWorktree(w, stillOurs);
    } catch (e) {
      result = { removed: false as const, reason: String((e as Error).message ?? e) };
    }
    this.log.info('worktree cleanup', { threadId: rec.id, path: w.path, ...result });
    // Deleted meanwhile: nothing to record it in, and emit would throw.
    if (st.rt.deleted && !deleted) return;
    delete w.cleanupOwed;
    if (result.removed) {
      w.removedAt = this.isoNow();
      // A folder that's another checkout's now counts as a spare: the
      // thread, brought back, never carries on in it (leavesRemovedWorktree).
      if (result.spare || result.taken) w.spare = true;
      if (deleted) return;
      const text = result.branchLeft
        ? `Removed this thread's worktree, but its branch ${w.branch} stays: ${result.branchLeft}. Everything on it is on main, so git -C ${w.mainCheckout} branch -D ${w.branch} loses nothing.`
        : result.taken
        ? `This thread's worktree had already gone, and its folder ${w.path} is another checkout's now, so it was left alone. Its branch ${w.branch} is gone too.`
        : w.spare
          ? `Deleted this thread's branch ${w.branch} (everything on it is on main) and kept its worktree, build caches and all, for the next thread.`
          : `Removed this thread's worktree and its branch ${w.branch}: everything on it is on main.`;
      this.emit(st, { type: 'system.note', text });
      this.markDirty(st);
      return;
    }
    const how =
      `To land it, run tb land in ${w.path}${deleted ? '' : ' (or bring the thread back and press Land)'}. ` +
      `To drop it, git -C ${w.mainCheckout} worktree remove --force ${w.path} && git -C ${w.mainCheckout} branch -D ${w.branch}.`;
    if (deleted) {
      this.notice('info', `"${rec.title}" was deleted, but its worktree at ${w.path} and branch ${w.branch} stay: ${result.reason}. ${how}`);
      return;
    }
    this.emit(st, { type: 'system.note', text: `This thread's worktree at ${w.path} and its branch ${w.branch} stay: ${result.reason}. ${how}` });
    this.markDirty(st);
  },
};

export type WorktreesPart = RunnerPart<typeof worktreesPart>;
