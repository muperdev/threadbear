// Moving a thread to another runner: this runner's side of each step
// (src/moves.ts decides when; docs/protocol.md, "Moving a thread to
// another runner"). Part of the Runner (runner.ts); each method's `this` is
// the Runner.
//
// The runner it leaves (S): describe, freeze, release the CLI and gather
// its files, transcript and git work, then put its copy away once the
// other runner has it. The runner it goes to (T): plan the folder and
// login, place everything in a staging folder (and its worktree and
// transcript where they belong), then after the switch move the staging
// folder in among the live threads and adopt it.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { Runner } from '../runner.ts';
import type { RunnerPart } from './part.ts';
import { providerName } from '../providers.ts';
import { paths } from '../config.ts';
import { isDirectory } from '../fsutil.ts';
import { readTree, writeTree, type PackEntry } from '../move-pack.ts';
import { changedFiles, cloneRepo, cloneableUrl, originUrl, packGit, repoKey, repoTop, unpackGit, withoutCredentials, type GitMeta } from '../move-git.ts';
import { movedSchedules, type MoveInfo, type MovePlan, type MovePreview, type MoveRow, type MovedSchedule, type ThreadMoving } from '../moves.ts';
import { projectDirName, sessionTranscriptPath } from '../transcripts.ts';
import { findWorktreePatch } from '../patches.ts';
import { isPatchFile, patchHash } from '../patch-trust.ts';
import { claudeCommands } from '../claude-trust.ts';
import { untrustedGit } from '../safe-git.ts';
import { isPermissionMode } from '../protocol.ts';
import { cleanUpWorktree, makeWorktree, mainCheckoutOf, realpath, type WorktreeRecord } from '../worktrees.ts';

// What T placed for a move, so adopting after a restart and throwing it
// away after a failure both know (<home>/moves/<moveId>/placed.json).
type Placed = { cwd: string; worktree?: WorktreeRecord; patchFile?: string; patchName?: string; transcript: string[]; leftBehind: string[]; fromName: string; schedules?: unknown[]; gitTips?: string[] };

// A moved thread's old folder is kept this long on S, in case.
const KEPT_MS = 7 * 24 * 60 * 60_000;

function stageOf(home: string, moveId: string): string {
  if (!/^[0-9a-f]{8,64}$/.test(moveId)) throw new Error('bad move id');
  return path.join(home, 'moves', moveId);
}

// A path on S under its home, as the same path under this machine's.
function swapHome(p: string, fromHome: string): string {
  const home = os.homedir();
  if (p === fromHome) return home;
  if (fromHome && p.startsWith(fromHome.endsWith('/') ? fromHome : `${fromHome}/`)) return path.join(home, p.slice(fromHome.length));
  return p;
}

// What the moved record (the old runner's word, and that runner may be an
// extra Mac) may not bring: anything that runs or approves something here.
// "Always allow" rules were answers on the other machine, for its folder;
// reply buttons (a command to run, a thread to start with its own mode, a
// message to send) and an open card's commands were made there, for
// there. They're asked or made again on this machine. True if it changed.
function dropMovedPowers(rec: Record<string, any>): boolean {
  let changed = false;
  if (rec.allowRules !== undefined) {
    delete rec.allowRules;
    changed = true;
  }
  if (rec.buttons !== undefined) {
    delete rec.buttons;
    changed = true;
  }
  for (const ask of Array.isArray(rec.needs?.asks) ? rec.needs.asks : []) {
    if (ask && typeof ask === 'object' && 'run' in ask) {
      delete ask.run;
      changed = true;
    }
  }
  // A mode this runner doesn't know compares as nothing (2 > undefined is
  // false), so every check that a thread may raise access would pass:
  // Ask. A missing return mode from Plan first means Full (leavePlan), so
  // one that isn't a mode is Ask too.
  if (!isPermissionMode(rec.permissionMode)) {
    rec.permissionMode = 'ask';
    changed = true;
  }
  if (rec.modeBeforePlan !== undefined && !isPermissionMode(rec.modeBeforePlan)) {
    rec.modeBeforePlan = 'ask';
    changed = true;
  }
  return changed;
}

// On the main Mac, a thread from another runner arrives with at most Ask
// (Mike, 10 Nov 2026): its mode is that runner's word, an extra Mac may be
// compromised, and the main Mac's sidebar never showed what mode the
// thread had there, so a thread that looked careful could arrive running
// anything. Full and Auto become Ask (Plan first ranks with Ask and
// stays), and so does the mode Plan first would go back to. Says what it
// was, for the thread's note, or undefined when nothing changed.
const ABOVE_ASK = new Set(['full', 'auto']);
function capMovedMode(rec: Record<string, any>): { was?: string; changed: boolean } {
  let was: string | undefined;
  let changed = false;
  if (rec.permissionMode !== 'ask' && rec.permissionMode !== 'plan') {
    if (ABOVE_ASK.has(rec.permissionMode)) was = rec.permissionMode;
    rec.permissionMode = 'ask';
    changed = true;
  }
  if (rec.permissionMode === 'plan') {
    // Approving the plan goes back to this, and to Full when it's missing
    // (leavePlan), so it's always set.
    if (rec.modeBeforePlan !== 'ask') {
      if (ABOVE_ASK.has(rec.modeBeforePlan)) was ??= rec.modeBeforePlan;
      rec.modeBeforePlan = 'ask';
      changed = true;
    }
  } else if (rec.modeBeforePlan !== undefined) {
    delete rec.modeBeforePlan;
    changed = true;
  }
  return { ...(was ? { was } : {}), changed };
}

const MODE_WORDS: Record<string, string> = { full: 'Full access', auto: 'Auto' };
function cappedNote(was: string, from: string): string {
  return `It had ${MODE_WORDS[was] ?? was} on ${from}, so it asks before tools run here: give it more access again if you want.`;
}

// What a moved record may keep here, applied to the record wherever it is
// (staging, the live folder, or loaded already): dropMovedPowers, and on
// the main Mac capMovedMode. The note's line about the mode goes in
// `notes`. True if it changed.
function sanitizeMoved(rec: Record<string, any>, atMostAsk: boolean, from: string, notes: string[]): boolean {
  let changed = dropMovedPowers(rec);
  if (atMostAsk) {
    const capped = capMovedMode(rec);
    if (capped.was && !notes.some((n) => n.includes('so it asks before tools run here'))) notes.push(cappedNote(capped.was, from));
    changed ||= capped.changed;
  }
  return changed;
}

// Whether the worktree's setup would run something the moved git work
// changed: the patch's own copy, or a file it reaches, differs in the
// worktree from the commit it was made from (this runner's own main), or
// can't be followed. Threadbear's own checkout is always trusted, so for
// its worktrees nothing else would stop the old runner's version of the
// setup script running here (Codex's review, 10 Nov 2026). Says why, or
// undefined when setup can run.
export async function setupChangedByMove(w: WorktreeRecord, patchFile: string): Promise<string | undefined> {
  // placeMove keeps the absolute path (makeWorktree's patchFile); the
  // record's own is relative to the worktree.
  const copy = path.isAbsolute(patchFile) ? patchFile : path.join(w.path, patchFile);
  let reached: string[];
  try {
    const h = patchHash(copy);
    if (h.incomplete) return `what it runs can't all be followed (${h.incomplete})`;
    reached = [copy, ...h.deps.map((d) => (path.isAbsolute(d.path) ? d.path : path.resolve(path.dirname(copy), d.path))), ...h.unnamed.map((u) => (path.isAbsolute(u) ? u : path.resolve(path.dirname(copy), u)))];
  } catch (e) {
    return `its patch couldn't be read (${e instanceof Error ? e.message : String(e)})`;
  }
  const root = realpath(w.path);
  for (const file of new Set(reached)) {
    const real = realpath(file);
    const rel = path.relative(root, real);
    // Outside the worktree: the move didn't bring it.
    if (rel.startsWith('..') || path.isAbsolute(rel)) continue;
    const there = await untrustedGit(w.path, ['cat-file', '-e', `${w.base}:${rel.split(path.sep).join('/')}`]);
    const exists = fs.existsSync(real);
    if (!there.ok) {
      if (exists) return `the moved work adds ${rel}, which it runs`;
      continue;
    }
    if (!exists) return `the moved work removes ${rel}, which it runs`;
    const same = await untrustedGit(w.path, ['diff', '--quiet', '--no-ext-diff', '--no-textconv', w.base, '--', rel.split(path.sep).join('/')]);
    if (!same.ok) return `the moved work changes ${rel}, which it runs`;
  }
  return undefined;
}

// Whether the moved git work changed anything patch or Claude settings
// trust covers in the worktree, from the commit T made it from: a file
// under any .threadbear or .claude folder, a .mcp.json, or a file a patch
// there or the Claude settings reach. Threadbear's own checkout is always
// trusted, and its worktrees with it, so without this the old runner's
// patches would load and its Claude hooks run here (Codex's review, 10 Nov
// 2026). True when it did, or when it can't tell.
// `tips`: the move's own commits (its head, and the commit of its
// uncommitted work), whose trees say exactly what it brought, ignore rules
// or not (a file this machine ignores is still written, untracked). Without
// them (one placed by a runner from before), the worktree as it is, every
// untracked file included.
export async function trustTouchedByMove(w: WorktreeRecord, home: string, tips?: readonly string[], cwd?: string): Promise<boolean> {
  const lists: { ok: boolean; stdout: string }[] = [];
  if (tips?.length) {
    for (const tip of tips) lists.push(await untrustedGit(w.path, ['diff', '--name-only', '-z', '--no-renames', '--no-ext-diff', '--no-textconv', w.base, tip]));
  } else {
    lists.push(await untrustedGit(w.path, ['diff', '--name-only', '-z', '--no-renames', '--no-ext-diff', '--no-textconv', w.base]));
    lists.push(await untrustedGit(w.path, ['ls-files', '-z', '--others']));
  }
  if (lists.some((l) => !l.ok)) return true;
  const changed = [...new Set(lists.flatMap((l) => l.stdout.split('\0')).filter(Boolean))];
  if (!changed.length) return false;
  // Without case: on the Mac .CLAUDE is .claude, and a name from a
  // case-sensitive machine can be either.
  // node_modules too: a committed package is code a patch may import.
  if (changed.some((f) => f.toLowerCase().split('/').some((part) => part === '.threadbear' || part === '.claude' || part === 'node_modules') || path.posix.basename(f).toLowerCase() === '.mcp.json')) return true;
  const touchedLower = new Set(changed.map((f) => path.join(realpath(w.path), ...f.split('/')).toLowerCase()));
  const root = realpath(w.path);
  // A changed symlink can point an unchanged patch's import at another
  // file, which the reached paths name rather than the link: any changed
  // link counts (Codex's review).
  for (const f of changed) {
    try {
      if (fs.lstatSync(path.join(w.path, ...f.split('/'))).isSymbolicLink()) return true;
    } catch {
      // Gone: a removal, compared below.
    }
  }
  const touched = new Set(changed.map((f) => path.join(root, ...f.split('/'))));
  const hit = (file: string) => {
    const real = realpath(path.isAbsolute(file) ? file : path.join(root, file));
    return touched.has(real) || touchedLower.has(real.toLowerCase());
  };
  // What the patches already there reach (a patch file itself changing
  // was caught above).
  const patches = await untrustedGit(w.path, ['ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', ':(glob)**/.threadbear/patches/*']);
  if (!patches.ok) return true;
  for (const rel of patches.stdout.split('\0').filter(Boolean)) {
    const file = path.join(root, ...rel.split('/'));
    if (!isPatchFile(file)) continue;
    try {
      const h = patchHash(file);
      if (h.incomplete) return true;
      const near = (p: string) => (path.isAbsolute(p) ? p : path.resolve(path.dirname(file), p));
      if ([...h.deps.map((d) => d.path), ...h.unnamed, ...h.code.keys()].some((p) => hit(near(p)))) return true;
    } catch {
      return true;
    }
  }
  // What runs, and what lets tools run unasked (a permissions allow).
  // From the worktree's top and from the folder Claude starts in (a thread
  // in a subfolder), as Claude reads the settings above where it starts.
  for (const dir of new Set([w.path, ...(cwd ? [cwd] : [])])) {
    for (const grants of [false, true]) {
      const claude = claudeCommands(dir, home, { grants });
      if (claude && (claude.incomplete || [...claude.files, ...claude.named].some(hit))) return true;
    }
  }
  return false;
}

// Every untracked file under a node_modules folder in `dir`, removed (not
// followed: git lists a linked node_modules as the link, and leaves it).
export async function dropUntrackedPackages(dir: string): Promise<void> {
  const listed = await untrustedGit(dir, ['ls-files', '-z', '--others', '--', ':(glob)**/node_modules/**']);
  for (const rel of listed.ok ? listed.stdout.split('\0').filter(Boolean) : []) fs.rmSync(path.join(dir, ...rel.split('/')), { force: true });
}

const UNVOUCHED_NOTE = "Its worktree's work changes this repo's patches or Claude Code settings, so they wait for you to trust them in Patches here.";

export const movesPart = {
  // ThreadSummary.moving, and while it's set no turn starts: messages wait
  // in the queue, which moves with the thread. Cleared, the queue goes on.
  setMoving(this: Runner, threadId: string, moving: ThreadMoving | undefined): void {
    const st = this.threads.get(threadId);
    if (!st) return;
    const was = st.rt.moving;
    st.rt.moving = moving;
    if (JSON.stringify(was) !== JSON.stringify(moving)) this.markDirty(st);
    if (!moving && was) this.kickQueue(st);
  },

  // Nothing would be cut short by packing it now. Background shells are
  // stopped (and said); sub-agents finish first.
  moveIdle(this: Runner, threadId: string): boolean {
    const st = this.threads.get(threadId);
    if (!st) return false;
    const { rt } = st;
    if (rt.turn || rt.approvals.size || rt.wakeTimer || rt.hooksRunning || rt.runningActions?.size || rt.editing || rt.cleaning) return false;
    return !rt.tasks.some((t) => t.kind === 'agent') && !st.rec.subagents?.some((a) => a.status === 'running');
  },

  async describeForMove(this: Runner, threadId: string, schedules: number): Promise<MoveInfo | undefined> {
    const st = this.threads.get(threadId);
    if (!st) return undefined;
    const { rec } = st;
    const account = this.accounts.list().find((a) => a.id === rec.accountId);
    const own = Boolean(rec.worktree && !rec.worktree.removedAt);
    const top = await repoTop(rec.cwd);
    // Which repo, never a password or token in its address (move-git.ts).
    const raw = top ? await originUrl(top) : undefined;
    const origin = raw ? withoutCredentials(raw) : undefined;
    const rel = top ? path.relative(top, rec.cwd) : '';
    return {
      title: rec.title,
      cwd: rec.cwd,
      home: os.homedir(),
      provider: rec.provider,
      accountId: rec.accountId,
      ...(account?.email ? { email: account.email } : {}),
      ...(rec.model ? { model: rec.model } : {}),
      ...(rec.effort ? { effort: rec.effort } : {}),
      permissionMode: rec.permissionMode,
      ...(origin ? { origin } : {}),
      ...(top ? { repoTop: own ? rec.worktree!.mainCheckout : ((await mainCheckoutOf(top)) ?? top), subdir: rel.split(path.sep).join('/') } : {}),
      worktree: own,
      ...(rec.sessionUsed && rec.nativeSessionId ? { sessionId: rec.nativeSessionId } : {}),
      schedules,
    };
  },

  // What packForMove will leave behind, in words for before the move. Move
  // to goes straight away unless there's something here to say first.
  async leftBehindByMove(this: Runner, threadId: string): Promise<string[]> {
    const st = this.threads.get(threadId);
    if (!st) return [];
    const { rec, rt } = st;
    const out: string[] = [];
    const shells = rt.tasks.filter((t) => t.kind !== 'agent').length;
    if (shells) out.push(`${shells} background ${shells === 1 ? 'job, which gets' : 'jobs, which get'} stopped`);
    if (!rec.worktree || rec.worktree.removedAt) {
      const top = await repoTop(rec.cwd);
      const files = top ? await changedFiles(top) : [];
      if (files.length) out.push(`uncommitted changes in ${top}, as other threads share that checkout: ${files.join(', ')}`);
    }
    return out;
  },

  // Releases the CLI and gathers what goes: the thread's folder, its
  // Claude transcript, and its worktree's git work.
  async packForMove(this: Runner, threadId: string, row: MoveRow): Promise<{ entries: PackEntry[]; leftBehind: string[] }> {
    const st = this.threads.get(threadId);
    if (!st) throw new Error("The thread isn't here any more.");
    const { rec, rt } = st;
    const leftBehind: string[] = [];
    const shells = rt.tasks.filter((t) => t.kind !== 'agent').length;
    if (shells) leftBehind.push(`${shells} background ${shells === 1 ? 'job was' : 'jobs were'} stopped, as they can't move.`);
    this.flushDeltas(st);
    if (rt.session && !rt.closing) {
      this.rememberStoppedWork(st);
      this.releaseSession(st);
    }
    if (rt.closing) await rt.closing;
    this.store.save(rec);
    const entries = readTree(paths.thread(this.cfg.home, rec.id), 'thread', ['panels']);
    const account = this.accounts.list().find((a) => a.id === rec.accountId);
    if (rec.provider === 'claude' && rec.sessionUsed && rec.nativeSessionId && account) {
      const file = sessionTranscriptPath(account.configDir, rec.cwd, rec.nativeSessionId);
      if (file) {
        entries.push({ path: `transcript/${rec.nativeSessionId}.jsonl`, data: fs.readFileSync(file) });
        entries.push(...readTree(path.join(path.dirname(file), rec.nativeSessionId), `transcript/${rec.nativeSessionId}`));
      }
    }
    if (rec.worktree && !rec.worktree.removedAt) {
      const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-move-'));
      try {
        const git = await packGit(rec.worktree.path, rec.cwd, path.join(scratch, 'move.bundle'));
        if (!git.ok) throw new Error(git.reason);
        entries.push({ path: 'git/meta.json', data: Buffer.from(JSON.stringify(git.meta)) });
        if (git.bundle) entries.push({ path: 'git/move.bundle', data: fs.readFileSync(git.bundle) });
      } finally {
        fs.rmSync(scratch, { recursive: true, force: true });
      }
    } else {
      const top = await repoTop(rec.cwd);
      const files = top ? await changedFiles(top) : [];
      if (files.length) leftBehind.push(`Uncommitted changes in ${top} stayed on ${this.moveRunnerName(row.from)}, as other threads share that checkout: ${files.join(', ')}.`);
    }
    return { entries, leftBehind };
  },

  // After the move: the thread is the other runner's. Its folder goes to
  // <home>/moved/ for a week, and clients see it go (their list of other
  // runners' threads brings it back as that runner's).
  async putAwayMoved(this: Runner, threadId: string): Promise<void> {
    const st = this.threads.get(threadId);
    if (st) await this.letGo(st);
    const dir = paths.thread(this.cfg.home, threadId);
    const moved = path.join(this.cfg.home, 'moved');
    fs.mkdirSync(moved, { recursive: true });
    if (fs.existsSync(dir)) fs.renameSync(dir, path.join(moved, `${threadId}-${Date.now()}`));
    for (const name of fs.readdirSync(moved)) {
      const at = Number(/-(\d+)$/.exec(name)?.[1]);
      if (at && at < Date.now() - KEPT_MS) fs.rmSync(path.join(moved, name), { recursive: true, force: true });
    }
    this.notify({ method: 'thread.deleted', params: { threadId } });
    this.log.info('moved a thread away', { threadId });
  },

  // T: where the thread will run here, and on which login, or why it can't.
  async planMove(this: Runner, row: MoveRow): Promise<MovePlan | { problem: string }> {
    const info = row.info!;
    const here = this.moveRunnerName(row.to);
    const accounts = this.accounts.list();
    let account = row.accountId ? accounts.find((a) => a.id === row.accountId) : undefined;
    if (row.accountId && !account) return { problem: `${here} has no login ${row.accountId}.` };
    account ??=
      accounts.find((a) => a.provider === info.provider && a.email && info.email && a.email.toLowerCase() === info.email.toLowerCase()) ??
      accounts.find((a) => a.id === this.defaultAccountId() && a.provider === info.provider) ??
      accounts.find((a) => a.provider === info.provider);
    if (!account) return { problem: `${here} has no ${providerName(info.provider)} login.` };
    const leftBehind: string[] = [];
    const subdir = info.subdir ?? '';
    const into = (top: string) => (subdir ? path.join(top, ...subdir.split('/')) : top);

    // A repo: the same one here, by its origin.
    const findRepo = async (): Promise<string | undefined> => {
      if (!info.origin) return undefined;
      const want = repoKey(info.origin);
      const tops = new Set<string>();
      if (info.repoTop) tops.add(swapHome(info.repoTop, info.home));
      for (const t of this.threads.values()) tops.add(t.rec.worktree?.mainCheckout ?? t.rec.cwd);
      for (const c of tops) {
        if (!isDirectory(c)) continue;
        const top = await repoTop(c);
        const origin = top ? await originUrl(top) : undefined;
        if (top && origin && repoKey(origin) === want) return (await mainCheckoutOf(top)) ?? top;
      }
      return undefined;
    };

    if (row.cwd) {
      if (!isDirectory(row.cwd)) return { problem: `There's no folder ${row.cwd} on ${here}.` };
      const top = await repoTop(row.cwd);
      const origin = top ? await originUrl(top) : undefined;
      if (info.worktree && top && origin && info.origin && repoKey(origin) === repoKey(info.origin)) {
        const repo = (await mainCheckoutOf(top)) ?? top;
        return { cwd: repo, accountId: account.id, worktree: { repo, subdir }, leftBehind };
      }
      if (info.worktree) leftBehind.push(`Its worktree's git work stays behind, as ${row.cwd} isn't a copy of ${info.origin ?? 'its repo'}.`);
      return { cwd: row.cwd, accountId: account.id, leftBehind };
    }
    if (info.origin) {
      let repo = await findRepo();
      let cloneTo: { url: string; to: string } | undefined;
      if (!repo && info.repoTop) {
        const to = swapHome(info.repoTop, info.home);
        // Both are the old runner's word: a clone only from an address git
        // can't read as an option, into a new folder named in full.
        const plain = path.isAbsolute(to) && path.resolve(to) === to;
        if (plain && cloneableUrl(info.origin) && !fs.existsSync(to) && isDirectory(path.dirname(to))) {
          cloneTo = { url: info.origin, to };
          repo = to;
        }
      }
      if (!repo) return { problem: `${here} has no copy of ${info.origin}.` };
      const plan: MovePlan = info.worktree ? { cwd: repo, accountId: account.id, worktree: { repo, subdir }, leftBehind } : { cwd: into(repo), accountId: account.id, leftBehind };
      if (cloneTo) plan.clone = cloneTo;
      if (!cloneTo && !info.worktree && !isDirectory(plan.cwd)) return { problem: `There's no folder ${plan.cwd} on ${here}.` };
      return plan;
    }
    const cwd = swapHome(info.cwd, info.home);
    if (!isDirectory(cwd)) return { problem: `There's no folder ${cwd} on ${here}. Pick one to move it into.` };
    return { cwd, accountId: account.id, leftBehind };
  },

  // T: everything in place but the thread itself, which waits in staging
  // until the switch. Throws (after undoing what it made) to fail the move.
  async placeMove(this: Runner, row: MoveRow, entries: PackEntry[]): Promise<void> {
    const plan = row.plan!;
    const stage = stageOf(this.cfg.home, row.moveId);
    fs.rmSync(stage, { recursive: true, force: true });
    fs.mkdirSync(stage, { recursive: true, mode: 0o700 });
    const placed: Placed = { cwd: plan.cwd, transcript: [], leftBehind: [...plan.leftBehind], fromName: this.moveRunnerName(row.from) };
    const save = () => fs.writeFileSync(path.join(stage, 'placed.json'), JSON.stringify(placed));
    try {
      writeTree(entries, 'thread', path.join(stage, 'thread'));
      const recFile = path.join(stage, 'thread', 'thread.json');
      const rec = JSON.parse(fs.readFileSync(recFile, 'utf8'));
      if (rec.id !== row.threadId) throw new Error("The moved thread's record is for another thread.");
      if (rec.confined) throw new Error("A thread in a sandbox can't move yet.");
      const manifest = entries.find((e) => e.path === 'move.json');
      if (manifest) {
        const m = JSON.parse(manifest.data.toString('utf8')) as { leftBehind?: string[]; fromName?: unknown };
        placed.leftBehind.push(...(m.leftBehind ?? []));
        // A runner on its own key doesn't hear the other runners' names, so
        // the old one says its own (the note read "Moved from 7928812d92c1").
        if (placed.fromName === row.from && typeof m.fromName === 'string' && m.fromName.trim()) placed.fromName = m.fromName.trim().slice(0, 80);
      }
      // Only when and what each sends (moves.ts movedSchedules): nothing
      // that runs or approves anything comes from the other runner.
      const schedules = entries.find((e) => e.path === 'schedules.json');
      if (schedules) {
        const { schedules: kept, dropped } = movedSchedules(JSON.parse(schedules.data.toString('utf8')), row.threadId);
        placed.schedules = kept;
        if (dropped) placed.leftBehind.push(`${dropped === 1 ? "A schedule's shell check was" : `${dropped} schedules' shell checks were`} left off here: set ${dropped === 1 ? 'it' : 'them'} again if still wanted, with a card on this machine.`);
      }
      if (plan.clone) {
        const cloned = await cloneRepo(plan.clone.url, plan.clone.to);
        if (!cloned.ok) throw new Error(cloned.reason);
      }

      if (plan.worktree) {
        const patch = await findWorktreePatch(plan.worktree.repo, this.panelsRoot, this.patchCache, undefined, this.patchTrust);
        if (!patch) throw new Error(`${plan.worktree.repo} on ${this.moveRunnerName(row.to)} has no worktree patch, so the thread's worktree has nowhere to go.`);
        const made = await makeWorktree({ cwd: plan.worktree.repo, threadId: row.threadId, patch });
        if (!made.ok) throw new Error(`Couldn't make the thread's worktree: ${made.reason}`);
        placed.worktree = made.worktree;
        placed.patchFile = made.patchFile;
        placed.patchName = patch.patch;
        placed.cwd = plan.worktree.subdir ? path.join(made.worktree.path, ...plan.worktree.subdir.split('/')) : made.worktree.path;
        save();
        const metaEntry = entries.find((e) => e.path === 'git/meta.json');
        if (metaEntry) {
          const meta = JSON.parse(metaEntry.data.toString('utf8')) as GitMeta;
          const bundle = entries.find((e) => e.path === 'git/move.bundle');
          const bundleFile = path.join(stage, 'move.bundle');
          if (bundle) fs.writeFileSync(bundleFile, bundle.data);
          const r = await unpackGit(made.worktree.path, meta, bundle ? bundleFile : undefined);
          fs.rmSync(bundleFile, { force: true });
          if (!r.ok) throw new Error(r.reason);
          // Patches and Claude settings it changed are held to the stored
          // hashes here, even in an always-trusted checkout.
          placed.gitTips = [meta.head, ...(meta.wip ? [meta.wip] : [])];
          if (await trustTouchedByMove(made.worktree, os.homedir(), placed.gitTips, plan.worktree.subdir ? path.join(made.worktree.path, ...plan.worktree.subdir.split('/')) : made.worktree.path)) {
            this.patchTrust.unvouch?.(made.worktree.path);
            placed.leftBehind.push(UNVOUCHED_NOTE);
            save();
          }
        }
      }

      const account = this.accounts.list().find((a) => a.id === plan.accountId);
      if (!account) throw new Error(`No login ${plan.accountId} here.`);
      const sid: string | undefined = rec.nativeSessionId;
      const hasTranscript = sid && entries.some((e) => e.path === `transcript/${sid}.jsonl`);
      const oldCwd = rec.cwd as string;
      const oldAccount = rec.accountId as string;
      if (account.provider === 'claude' && rec.provider === 'claude' && hasTranscript) {
        const dir = path.join(account.configDir, 'projects', projectDirName(realpath(placed.cwd)));
        const mine = entries.filter((e) => e.path.startsWith('transcript/'));
        placed.transcript = mine.map((e) => path.join(dir, ...e.path.slice('transcript/'.length).split('/')));
        save();
        writeTree(mine, 'transcript', dir);
      } else if (rec.sessionUsed) {
        // No transcript to carry on from: a fresh session with the handoff
        // note, as a move to another provider is (runner.ts carryTranscript
        // can't find S's folder or login here, so it falls back).
        rec.handoff = { fromCwd: oldCwd, fromAccountId: oldAccount };
      }
      if (account.provider !== rec.provider) {
        rec.provider = account.provider;
        delete rec.model;
        delete rec.effort;
      }
      // Nothing that runs or approves anything comes (dropMovedPowers),
      // and on the main Mac no more than Ask (capMovedMode).
      sanitizeMoved(rec, Boolean(this.movedInAtMostAsk?.()), placed.fromName, placed.leftBehind);
      rec.cwd = placed.cwd;
      rec.accountId = account.id;
      delete rec.poolId;
      delete rec.worktreePending;
      delete rec.turnId;
      delete rec.stoppedMidTurn;
      rec.pendingApprovals = [];
      if (placed.worktree) rec.worktree = placed.worktree;
      else delete rec.worktree;
      fs.writeFileSync(recFile, JSON.stringify(rec, null, 2));
      save();
    } catch (e) {
      await this.discardMove(row);
      throw e;
    }
  },

  // T, after the switch: the staging folder joins the live threads. Safe to
  // call again (after a restart, say): once it's here there's nothing left.
  async adoptMoved(this: Runner, row: MoveRow): Promise<{ adopted: boolean; schedules?: MovedSchedule[] }> {
    const stage = stageOf(this.cfg.home, row.moveId);
    const placedFile = path.join(stage, 'placed.json');
    if (!fs.existsSync(placedFile)) return { adopted: false };
    const placed = JSON.parse(fs.readFileSync(placedFile, 'utf8')) as Placed;
    const dest = paths.thread(this.cfg.home, row.threadId);
    const atMostAsk = Boolean(this.movedInAtMostAsk?.());
    // The record as placeMove leaves it is clean already, but one placed by
    // a runner from before (and adopted after it updated) may not be: made
    // clean before it joins the live threads, and again wherever a crash
    // left it (the live folder, or loaded at start).
    const clean = (recFile: string) => {
      try {
        const rec = JSON.parse(fs.readFileSync(recFile, 'utf8'));
        if (sanitizeMoved(rec, atMostAsk, placed.fromName, placed.leftBehind)) fs.writeFileSync(recFile, JSON.stringify(rec, null, 2));
      } catch {
        // No record to read: adopt says the thread didn't load.
      }
    };
    // Worked out again here: one placed by a runner from before this
    // never was (trustTouchedByMove), and it must be before anything loads
    // the worktree's patches or starts its CLI.
    // One placed by a runner from before unpackGit refused packages in the
    // uncommitted work may have them there, untracked, which trust reads
    // as installed and doesn't hash: they go (setup installs them again).
    if (placed.worktree && !placed.gitTips) await dropUntrackedPackages(placed.worktree.path);
    if (placed.worktree && (await trustTouchedByMove(placed.worktree, os.homedir(), placed.gitTips, placed.cwd))) {
      this.patchTrust.unvouch?.(placed.worktree.path);
      if (!placed.leftBehind.includes(UNVOUCHED_NOTE)) placed.leftBehind.push(UNVOUCHED_NOTE);
    }
    if (!fs.existsSync(dest)) {
      clean(path.join(stage, 'thread', 'thread.json'));
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      fs.renameSync(path.join(stage, 'thread'), dest);
    } else clean(path.join(dest, 'thread.json'));
    this.adopt(row.threadId);
    const st = this.threads.get(row.threadId);
    if (!st) throw new Error("The moved thread didn't load.");
    if (sanitizeMoved(st.rec as unknown as Record<string, any>, atMostAsk, placed.fromName, placed.leftBehind)) this.store.save(st.rec);
    const lines = [`Moved from ${placed.fromName} to ${this.moveRunnerName(row.to)}, in ${placed.cwd}.`, ...placed.leftBehind];
    this.emit(st, { type: 'system.note', text: lines.join(' ') });
    this.markDirty(st);
    fs.rmSync(stage, { recursive: true, force: true });
    if (placed.worktree && placed.patchFile && placed.patchName) {
      const patch = await findWorktreePatch(placed.worktree.mainCheckout, this.panelsRoot, this.patchCache, undefined, this.patchTrust).catch(() => undefined);
      const changed = patch?.setup ? await setupChangedByMove(placed.worktree, placed.patchFile) : undefined;
      if (changed) this.emit(st, { type: 'system.note', text: `Didn't run the worktree's setup (${patch!.setup}): ${changed}. Look at it, then run it yourself if it's right.` });
      else if (patch) await this.setUpWorktree(st, { worktree: placed.worktree, patchFile: placed.patchFile }, patch);
    }
    this.log.info('took a moved thread in', { threadId: row.threadId, from: row.from });
    // Through the same filter again: a placed.json from before it had one.
    const schedules = movedSchedules(placed.schedules ?? [], row.threadId).schedules;
    return { adopted: true, ...(schedules.length ? { schedules } : {}) };
  },

  // T: the move failed or was cancelled before the switch.
  async discardMove(this: Runner, row: MoveRow): Promise<void> {
    let stage: string;
    try {
      stage = stageOf(this.cfg.home, row.moveId);
    } catch {
      return;
    }
    const placedFile = path.join(stage, 'placed.json');
    if (fs.existsSync(placedFile) && !this.threads.has(row.threadId)) {
      const placed = JSON.parse(fs.readFileSync(placedFile, 'utf8')) as Placed;
      for (const f of placed.transcript) fs.rmSync(f, { force: true });
      if (placed.worktree) await cleanUpWorktree(placed.worktree).catch(() => undefined);
    }
    fs.rmSync(stage, { recursive: true, force: true });
  },

  // T, for the Mac's Move sheet (threads.movePreview, a `moves.preview`
  // call): planMove's guess for a thread S described, with nothing made or
  // cloned, and this runner's logins of the thread's provider to pick from.
  // What would stop the move goes in `problems` rather than throwing, so the
  // sheet can say it and still offer the pickers.
  async previewMove(this: Runner, p: { info: MoveInfo; self: string; cwd?: string; accountId?: string }): Promise<MovePreview> {
    const accounts = this.accounts
      .list()
      .filter((a) => a.provider === p.info.provider)
      .map((a) => ({ id: a.id, provider: a.provider, title: a.title, ...(a.email ? { email: a.email } : {}) }));
    const now = Date.now();
    const row: MoveRow = { moveId: 'preview', threadId: '', from: '', to: p.self, state: 'asking', info: p.info, ...(p.cwd ? { cwd: p.cwd } : {}), ...(p.accountId ? { accountId: p.accountId } : {}), createdAt: now, updatedAt: now };
    const plan = await this.planMove(row);
    if ('problem' in plan) return { accounts, leftBehind: [], problems: [plan.problem] };
    const problems: string[] = [];
    // placeMove's own check, seen before Mike presses Move rather than
    // after the thread has frozen and packed. A repo still to be cloned
    // can't be looked at yet.
    if (plan.worktree && !plan.clone) {
      const patch = await findWorktreePatch(plan.worktree.repo, this.panelsRoot, this.patchCache, undefined, this.patchTrust).catch(() => undefined);
      if (!patch) problems.push(`${plan.worktree.repo} on ${this.moveRunnerName(p.self)} has no worktree patch, so the thread's worktree has nowhere to go.`);
    }
    return {
      cwd: plan.cwd,
      accountId: plan.accountId,
      accounts,
      leftBehind: plan.leftBehind,
      problems,
      ...(plan.worktree ? { worktree: true } : {}),
      ...(plan.clone ? { clone: plan.clone.to } : {}),
    };
  },

  moveRunnerName(this: Runner, runnerId: string): string {
    return this.runnerNames?.(runnerId) ?? runnerId;
  },
};

export type MovesPart = RunnerPart<typeof movesPart>;
