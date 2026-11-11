// Shared projects (docs/multiplayer-design.md): this runner's side of a
// team. It keeps the member key, follows the team's hub (convex/team.ts) on
// whichever deployment hosts it, keeps linked projects in step both ways,
// sends a card for each of this Mac's threads in a shared project, and
// looks after the messages and handovers between members' agents.
//
// Behind the `multiplayer` experiment. With it off, or no team joined,
// nothing here talks to anything.
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import type { Clock, TimerHandle } from './clock.ts';
import type { Logger } from './log.ts';
import type { Notification } from './runner.ts';
import type { ProjectLink, ProjectTeam, ThreadSummary } from './protocol.ts';
import { ErrorCode, RpcError } from './protocol.ts';
import type { Post, Subscriber } from './sync.ts';
import { convexPoster, convexSubscriber } from './sync.ts';
import type { ProjectRecord, Projects } from './projects.ts';
import { atomicWriteFileSync, isDirectory, readJsonSync } from './fsutil.ts';
import { paths } from './config.ts';

/**
 * A git remote as one string every member's clone agrees on
 * (github.com/org/repo). The same as convex/teamAccess.ts normalRepo, which
 * the hub uses; team.test.ts checks they agree.
 */
export function normalRepo(remote: unknown): string | undefined {
  if (typeof remote !== 'string') return undefined;
  let r = remote.trim();
  if (!r) return undefined;
  const scp = r.match(/^[\w.-]+@([\w.-]+):(.+)$/);
  if (scp) r = `${scp[1]}/${scp[2]}`;
  else {
    const m = r.match(/^(?:https?|ssh|git):\/\/(?:[^@/]+@)?([\w.-]+)(?::\d+)?\/(.+)$/i);
    if (!m) return undefined;
    r = `${m[1]}/${m[2]}`;
  }
  r = r.replace(/\.git$/i, '').replace(/\/+$/, '').toLowerCase();
  return /^[\w.-]+\/[\w.\-/]+$/.test(r) && r.length <= 200 ? r : undefined;
}

// What the runner keeps for the team it's in: secrets/team.json, 0600.
export type TeamConfig = {
  url: string;
  key: string;
  memberId: string;
  name: string;
  teamName: string;
  host: boolean;
};

export type CardStatus = 'working' | 'needsThem' | 'idle' | 'done';

// As the hub's queries return them (convex/team.ts state and live).
export type HubMember = { memberId: string; name: string; host: boolean; joinedAt: number; seenAt?: number };
export type HubLog = { memberId: string; text: string; at: number; threadTitle?: string };
export type HubProject = {
  id: string;
  title: string;
  goals: string;
  state: string;
  notes: string;
  links: ProjectLink[];
  repos: string[];
  sharedBy: string;
  createdAt: number;
  updatedAt: number;
  updatedBy: string;
  log: HubLog[];
};
export type HubCard = {
  projectId: string;
  memberId: string;
  threadId: string;
  title: string;
  status: CardStatus;
  headline?: string;
  branch?: string;
  repo?: string;
  handedTo?: string;
  updatedAt: number;
};
export type HubMessage = {
  id: string;
  projectId: string;
  kind: 'message' | 'handover';
  from: string;
  fromThread: string;
  fromTitle: string;
  to: string;
  toThread?: string;
  text: string;
  repo?: string;
  branch?: string;
  links?: ProjectLink[];
  createdAt: number;
  status: 'waiting' | 'passed' | 'dismissed' | 'taken' | 'declined';
  answeredAt?: number;
  answer?: string;
};
type HubState = { team: { name: string }; me: HubMember; members: HubMember[]; projects: HubProject[] };
type HubLive = { cards: HubCard[]; inbox: HubMessage[]; answered: HubMessage[] };

// What the app and tb are sent (team.get, team.updated).
export type TeamMemberView = { memberId: string; name: string; host: boolean; me: boolean; online: boolean; seenAt?: string };
export type TeamCardView = HubCard & { id: string; memberName: string; handedToName?: string; mine: boolean };
export type TeamWaiting = {
  id: string;
  kind: 'message' | 'handover';
  projectId: string;
  localProjectId?: string;
  fromName: string;
  fromTitle: string;
  toThread?: string;
  toThreadTitle?: string;
  text: string;
  repo?: string;
  branch?: string;
  links?: ProjectLink[];
  createdAt: string;
};
export type SharedProjectView = { id: string; title: string; sharedBy: string; repos: string[]; updatedAt: string; localProjectId?: string; mine: boolean };
export type TeamState = {
  on: boolean;
  joined: boolean;
  connected: boolean;
  error?: string;
  url?: string;
  name?: string;
  me?: TeamMemberView;
  members: TeamMemberView[];
  projects: SharedProjectView[];
  cards: TeamCardView[];
  waiting: TeamWaiting[];
  log: Record<string, (HubLog & { memberName: string })[]>;
};

// What the team needs from the runner.
export type TeamHost = {
  threads(): ThreadSummary[];
  thread(threadId: string): ThreadSummary | undefined;
  note(threadId: string, text: string): void;
  send(threadId: string, text: string): void;
  create(p: { text: string; cwd?: string; title?: string; projectIds: string[] }): ThreadSummary;
  publish(n: Notification): void;
  onNotification(fn: (n: Notification) => void): () => void;
  defaultCwd(): string;
};

export type Hub = { post: Post; subscriber?: Subscriber };

export type TeamDeps = {
  home: string;
  clock: Clock;
  log: Logger;
  on: () => boolean;
  projects: Projects;
  host: TeamHost;
  // This runner's own backend and key, for making a team on it.
  ownBackend: () => { url: string; key: string } | undefined;
  hub?: (url: string) => Hub;
  remoteOf?: (folder: string) => Promise<string | undefined>;
  // Where to look for clones of a shared project's repos.
  searchRoots?: () => string[];
};

export const ONLINE_MS = 5 * 60_000;
const HEARTBEAT_MS = 2 * 60_000;
const PUSH_DELAY_MS = 2000;
const CARD_EVERY_MS = 10_000;
const NOTE_PREVIEW = 300;
// Cards the brief names, and log entries it shows.
const BRIEF_CARDS = 6;
const BRIEF_LOG = 3;

const SYNCED_FIELDS = ['title', 'goals', 'state', 'notes', 'links'] as const;
type SyncedField = (typeof SYNCED_FIELDS)[number];

const randomId = (prefix: string, bytes = 6) => prefix + crypto.randomBytes(bytes).toString('hex');
const randomKey = (prefix: string) => prefix + crypto.randomBytes(32).toString('base64url');
export const sha256 = (s: string) => crypto.createHash('sha256').update(s).digest('hex');
const iso = (ms: number) => new Date(ms).toISOString();
const oneLine = (s: string, max: number) => {
  const t = s.replace(/\s+/g, ' ').trim();
  return t.length > max ? t.slice(0, max - 1) + '…' : t;
};

// An invite as printed: tbteam:<hub url>#<code>.
export function formatInvite(url: string, code: string): string {
  return `tbteam:${url.replace(/\/+$/, '')}#${code}`;
}
export function parseInvite(s: string): { url: string; code: string } {
  const m = s.trim().match(/^tbteam:(https?:\/\/[^#\s]+)#([A-Za-z0-9_-]{16,})$/);
  if (!m) throw bad('That is not a team invite. It looks like tbteam:https://…#…');
  return { url: m[1]!, code: m[2]! };
}

function bad(message: string): RpcError {
  return new RpcError(ErrorCode.badParams, message);
}

// A hub error's own words, without Convex's wrapping.
export function hubMessage(e: unknown): string {
  const data = (e as { data?: { message?: unknown } } | null)?.data;
  if (data && typeof data.message === 'string') return data.message;
  const text = e instanceof Error ? e.message : String(e);
  const json = text.match(/\{[^{}]*"message"\s*:\s*"([^"]*)"[^{}]*\}/)?.[1];
  return json ?? text;
}

// A card's status from the thread's summary.
export function cardStatus(t: ThreadSummary): CardStatus {
  if (t.archivedAt) return 'done';
  if (t.needsYou || t.status === 'waiting') return 'needsThem';
  if (t.status === 'running' || t.status === 'starting') return 'working';
  return 'idle';
}

// A card's id for agents: team:<member>:<thread8>.
export function cardRef(memberName: string, threadId: string): string {
  return `team:${memberName.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')}:${threadId.slice(0, 8)}`;
}

function gitOut(cwd: string, args: string[]): Promise<string | undefined> {
  return new Promise((resolve) => {
    execFile('git', ['-C', cwd, ...args], { timeout: 5000, encoding: 'utf8' }, (err, out) => resolve(err ? undefined : out.trim() || undefined));
  });
}

export async function gitRemote(folder: string): Promise<string | undefined> {
  return normalRepo(await gitOut(folder, ['remote', 'get-url', 'origin']));
}

export class Team {
  private deps: TeamDeps;
  private file: string;
  private stateFile: string;
  private cfg?: TeamConfig;
  private hub?: Hub;
  private unsubs: (() => void)[] = [];
  private unlisten?: () => void;
  private unlistenProjects?: () => void;
  private heartbeatTimer?: TimerHandle;
  private hubState?: HubState;
  private hubLive?: HubLive;
  private connected = false;
  private error?: string;
  // Message ids already said in a thread or delivered, kept across restarts.
  private seen = new Set<string>();
  // Fields changed here, waiting to go up, by local project id.
  private pending = new Map<string, Set<SyncedField>>();
  private pushTimer?: TimerHandle;
  // While a hub change is written into a local project: not sent back.
  private applying = false;
  // The cards as last sent, by project and thread.
  private sentCards = new Map<string, string>();
  private dirtyThreads = new Set<string>();
  private cardTimer?: TimerHandle;
  private remotes = new Map<string, Promise<string | undefined>>();
  private publishTimer?: TimerHandle;

  constructor(deps: TeamDeps) {
    this.deps = deps;
    this.file = path.join(paths.secrets(deps.home), 'team.json');
    this.stateFile = path.join(deps.home, 'team-state.json');
  }

  // ------------------------------------------------------------ lifecycle

  start(): void {
    const saved = readJsonSync<{ seen?: string[] }>(this.stateFile);
    for (const id of saved?.seen ?? []) this.seen.add(id);
    this.cfg = this.readConfig();
    this.unlisten = this.deps.host.onNotification((n) => this.onRunner(n));
    this.unlistenProjects = this.deps.projects.onChange((rec, prev, source) => this.onLocalProject(rec, prev, source));
    if (this.cfg && this.deps.on()) this.connect();
  }

  async stop(): Promise<void> {
    this.unlisten?.();
    this.unlistenProjects?.();
    await this.disconnect();
  }

  private readConfig(): TeamConfig | undefined {
    const c = readJsonSync<TeamConfig>(this.file);
    if (!c || typeof c.url !== 'string' || typeof c.key !== 'string' || typeof c.memberId !== 'string') return undefined;
    return c;
  }

  private writeConfig(c: TeamConfig | undefined): void {
    if (!c) {
      fs.rmSync(this.file, { force: true });
      return;
    }
    fs.mkdirSync(path.dirname(this.file), { recursive: true, mode: 0o700 });
    atomicWriteFileSync(this.file, JSON.stringify(c, null, 2), 0o600);
  }

  private saveSeen(): void {
    // The newest few hundred are plenty: answered messages leave the inbox.
    const ids = [...this.seen].slice(-500);
    this.seen = new Set(ids);
    atomicWriteFileSync(this.stateFile, JSON.stringify({ seen: ids }));
  }

  private hubFor(url: string): Hub {
    return this.deps.hub ? this.deps.hub(url) : { post: convexPoster(url), subscriber: convexSubscriber(url) };
  }

  private connect(): void {
    const cfg = this.cfg!;
    this.hub = this.hubFor(cfg.url);
    const sub = this.hub.subscriber;
    const onError = (e: Error) => {
      this.connected = false;
      this.error = hubMessage(e);
      this.deps.log.warn('team hub', { error: this.error });
      this.changed();
    };
    if (sub) {
      this.unsubs.push(
        sub.subscribe('team:state', { key: cfg.key }, (v) => this.onState(v as HubState), onError),
        sub.subscribe('team:live', { key: cfg.key }, (v) => this.onLive(v as HubLive), onError),
      );
    }
    this.beat();
    this.deps.log.info('team connected', { team: cfg.teamName, url: cfg.url });
  }

  private async disconnect(): Promise<void> {
    for (const u of this.unsubs.splice(0)) u();
    this.heartbeatTimer?.cancel();
    this.pushTimer?.cancel();
    this.cardTimer?.cancel();
    await this.hub?.subscriber?.close();
    this.hub = undefined;
    this.hubState = undefined;
    this.hubLive = undefined;
    this.connected = false;
  }

  private beat(): void {
    this.heartbeatTimer?.cancel();
    void this.call('mutation', 'team:heartbeat', {}).catch(() => {});
    this.heartbeatTimer = this.deps.clock.setTimeout(() => this.beat(), HEARTBEAT_MS);
  }

  // Reads the hub now, for a hub with no subscriptions (tests, a poll).
  async refresh(): Promise<void> {
    if (!this.cfg || !this.hub) return;
    this.onState((await this.call('query', 'team:state', {})) as HubState);
    this.onLive((await this.call('query', 'team:live', {})) as HubLive);
  }

  private async call(kind: 'query' | 'mutation', fn: string, args: Record<string, unknown>): Promise<unknown> {
    if (!this.cfg || !this.hub) throw bad('Not on a team. Join one with tb team join <invite>, or make one with tb team create <name>.');
    try {
      return await this.hub.post(kind, fn, { key: this.cfg.key, ...args });
    } catch (e) {
      throw new RpcError(ErrorCode.other, hubMessage(e));
    }
  }

  private mustOn(): void {
    if (!this.deps.on()) throw bad('Shared projects are an experiment that is off. Turn on "multiplayer" in Settings › Experiments (or runner.json) first.');
  }

  private mustJoined(): TeamConfig {
    this.mustOn();
    if (!this.cfg) throw bad('Not on a team. Join one with tb team join <invite>, or make one with tb team create <name>.');
    return this.cfg;
  }

  // ------------------------------------------------------------ joining

  async create(p: { name: string; memberName?: string }): Promise<TeamState> {
    this.mustOn();
    if (this.cfg) throw bad(`Already on the team ${this.cfg.teamName}. Leave it first.`);
    const own = this.deps.ownBackend();
    if (!own) throw bad("A team lives on this Mac's own Convex backend, and sync is off here.");
    const memberId = randomId('m_');
    const key = randomKey('tbm_');
    const memberName = (p.memberName ?? '').trim() || os.userInfo().username;
    const hub = this.hubFor(own.url);
    try {
      await hub.post('mutation', 'team:create', { ownerKey: own.key, name: p.name, memberId, memberName, keyHash: sha256(key) });
    } catch (e) {
      throw new RpcError(ErrorCode.other, hubMessage(e));
    }
    await hub.subscriber?.close();
    this.cfg = { url: own.url, key, memberId, name: memberName, teamName: p.name.trim(), host: true };
    this.writeConfig(this.cfg);
    this.connect();
    await this.refresh().catch(() => {});
    this.changed();
    return this.state();
  }

  async invite(p: { days?: number } = {}): Promise<{ invite: string; expiresAt: string }> {
    const cfg = this.mustJoined();
    const code = crypto.randomBytes(18).toString('base64url');
    const r = (await this.call('mutation', 'team:invite', { codeHash: sha256(code), ...(p.days ? { days: p.days } : {}) })) as { expiresAt: number };
    return { invite: formatInvite(cfg.url, code), expiresAt: iso(r.expiresAt) };
  }

  async join(p: { invite: string; name?: string }): Promise<TeamState> {
    this.mustOn();
    if (this.cfg) throw bad(`Already on the team ${this.cfg.teamName}. Leave it first.`);
    const { url, code } = parseInvite(p.invite);
    const memberId = randomId('m_');
    const key = randomKey('tbm_');
    const name = (p.name ?? '').trim() || os.userInfo().username;
    const hub = this.hubFor(url);
    let r: { name: string; memberName: string };
    try {
      r = (await hub.post('mutation', 'team:join', { codeHash: sha256(code), memberId, name, keyHash: sha256(key) })) as { name: string; memberName: string };
    } catch (e) {
      throw new RpcError(ErrorCode.other, hubMessage(e));
    } finally {
      await hub.subscriber?.close();
    }
    this.cfg = { url, key, memberId, name: r.memberName, teamName: r.name, host: false };
    this.writeConfig(this.cfg);
    this.connect();
    await this.refresh().catch(() => {});
    this.changed();
    return this.state();
  }

  // Leaving keeps the linked projects as this Mac's own.
  async leave(): Promise<TeamState> {
    const cfg = this.mustJoined();
    if (cfg.host) throw bad("You host this team, so you can't leave it. Remove the others, or stop sharing the projects.");
    await this.call('mutation', 'team:leave', {});
    await this.forget();
    return this.state();
  }

  // Forgets the team here without asking the hub (a removed member, or a hub that's gone).
  async forget(): Promise<void> {
    await this.disconnect();
    for (const p of this.deps.projects.list({ archived: true })) if (p.team) this.deps.projects.setTeam(p.id, undefined);
    this.cfg = undefined;
    this.writeConfig(undefined);
    this.sentCards.clear();
    this.changed();
  }

  async remove(p: { member: string }): Promise<TeamState> {
    this.mustJoined();
    const m = this.member(p.member);
    await this.call('mutation', 'team:remove', { memberId: m.memberId });
    await this.refresh().catch(() => {});
    return this.state();
  }

  private member(q: string): HubMember {
    const want = q.trim().toLowerCase();
    const all = this.hubState?.members ?? [];
    const m = all.find((x) => x.memberId === q || x.name.toLowerCase() === want) ?? (all.filter((x) => x.name.toLowerCase().startsWith(want)).length === 1 ? all.find((x) => x.name.toLowerCase().startsWith(want)) : undefined);
    if (!m) throw bad(`No one called ${q} on the team. On it: ${all.map((x) => x.name).join(', ') || 'nobody yet'}.`);
    return m;
  }

  private nameOf(memberId: string): string {
    return this.hubState?.members.find((m) => m.memberId === memberId)?.name ?? 'someone who left';
  }

  // ------------------------------------------------------------ projects

  // Puts a project of this Mac's on the team.
  async share(p: { projectId: string }): Promise<TeamState> {
    this.mustJoined();
    const rec = this.localProject(p.projectId);
    if (rec.team) throw bad(`${rec.title} is already shared.`);
    const id = randomId('tp_', 5);
    const repos = await this.reposOf(rec.folders);
    await this.call('mutation', 'team:shareProject', { project: { id, title: rec.title, goals: rec.goals, state: rec.state, notes: rec.notes, links: rec.links, repos } });
    this.deps.projects.setTeam(rec.id, { id, syncedAt: this.deps.clock.now() });
    await this.refresh().catch(() => {});
    this.queueAllCards();
    this.changed();
    return this.state();
  }

  // Makes a linked project here from a shared one, with this Mac's clones of its repos as folders.
  async add(p: { sharedId: string }): Promise<{ projectId: string; missing: string[] }> {
    this.mustJoined();
    const shared = this.hubState?.projects.find((x) => x.id === p.sharedId || x.title.toLowerCase() === p.sharedId.toLowerCase());
    if (!shared) throw bad(`No shared project ${p.sharedId}. tb team shows them.`);
    const linked = this.linkedTo(shared.id);
    if (linked) return { projectId: linked.id, missing: [] };
    const found = await this.findClones(shared.repos);
    const created = this.deps.projects.create({ title: shared.title, folders: found.folders, goals: shared.goals, state: shared.state, notes: shared.notes, links: shared.links, by: 'mike' });
    this.deps.projects.setTeam(created.id, { id: shared.id, syncedAt: shared.updatedAt });
    this.changed();
    return { projectId: created.id, missing: found.missing };
  }

  // Stops sharing it (the person who shared it, or the host); everyone keeps
  // their copy as their own.
  async unshare(p: { projectId: string }): Promise<TeamState> {
    this.mustJoined();
    const rec = this.localProject(p.projectId);
    if (!rec.team) throw bad(`${rec.title} isn't shared.`);
    await this.call('mutation', 'team:unshareProject', { id: rec.team.id });
    this.deps.projects.setTeam(rec.id, undefined);
    await this.refresh().catch(() => {});
    return this.state();
  }

  // Unlinks this Mac's copy without changing the team's.
  unlink(p: { projectId: string }): TeamState {
    const rec = this.localProject(p.projectId);
    this.deps.projects.setTeam(rec.id, undefined);
    this.changed();
    return this.state();
  }

  setDelivery(p: { projectId: string; delivery: 'ask' | 'deliver' }): TeamState {
    const rec = this.localProject(p.projectId);
    if (!rec.team) throw bad(`${rec.title} isn't shared.`);
    if (p.delivery !== 'ask' && p.delivery !== 'deliver') throw bad('delivery is ask or deliver');
    this.deps.projects.setTeam(rec.id, { ...rec.team, delivery: p.delivery === 'deliver' ? 'deliver' : undefined });
    this.changed();
    return this.state();
  }

  private localProject(q: string): ProjectRecord {
    const rec = this.deps.projects.record(q) ?? this.deps.projects.find(q);
    if (!rec) throw new RpcError(ErrorCode.projectNotFound, `No project called ${q}`);
    return rec;
  }

  private linkedTo(sharedId: string): ProjectRecord | undefined {
    return this.deps.projects.list({ archived: true }).map((p) => this.deps.projects.record(p.id)!).find((r) => r?.team?.id === sharedId);
  }

  private linked(): ProjectRecord[] {
    return this.deps.projects
      .list({ archived: true })
      .map((p) => this.deps.projects.record(p.id)!)
      .filter((r) => r?.team);
  }

  private remoteOf(folder: string): Promise<string | undefined> {
    let hit = this.remotes.get(folder);
    if (!hit) {
      hit = (this.deps.remoteOf ?? gitRemote)(folder);
      this.remotes.set(folder, hit);
    }
    return hit;
  }

  private async reposOf(folders: string[]): Promise<string[]> {
    const out: string[] = [];
    for (const f of folders) {
      const r = await this.remoteOf(f);
      if (r && !out.includes(r)) out.push(r);
    }
    return out;
  }

  // Folders on this Mac with these repos: the projects' and threads'
  // folders first, then a look one and two levels down the usual places.
  async findClones(repos: string[]): Promise<{ folders: string[]; missing: string[] }> {
    const want = new Set(repos);
    const found = new Map<string, string>();
    const tryFolder = async (f: string) => {
      if (found.size === want.size || !isDirectory(f)) return;
      const r = await this.remoteOf(f);
      if (r && want.has(r) && !found.has(r)) found.set(r, f);
    };
    const known = new Set<string>([...this.deps.projects.allFolders(), ...this.deps.host.threads().map((t) => t.worktree?.mainCheckout ?? t.cwd)]);
    for (const f of known) await tryFolder(f);
    const roots = this.deps.searchRoots?.() ?? ['Documents', 'Projects', 'dev', 'code', 'src', 'repos', 'git', 'work', ''].map((d) => path.join(os.homedir(), d));
    for (const root of roots) {
      if (found.size === want.size) break;
      for (const child of listDirs(root)) {
        if (fs.existsSync(path.join(child, '.git'))) await tryFolder(child);
        else if (path.dirname(root) !== '/') for (const grand of listDirs(child).slice(0, 50)) if (fs.existsSync(path.join(grand, '.git'))) await tryFolder(grand);
      }
    }
    return { folders: [...found.values()], missing: repos.filter((r) => !found.has(r)) };
  }

  // A project here changed: its shared fields go up shortly.
  private onLocalProject(rec: ProjectRecord, prev: ProjectRecord | undefined, source: string): void {
    if (this.applying || !rec.team || !this.cfg || source === 'team') return;
    const changed = SYNCED_FIELDS.filter((f) => JSON.stringify(rec[f]) !== JSON.stringify(prev?.[f]));
    if (!changed.length && prev && JSON.stringify(rec.folders) === JSON.stringify(prev.folders)) return;
    const set = this.pending.get(rec.id) ?? new Set<SyncedField>();
    for (const f of changed) set.add(f);
    this.pending.set(rec.id, set);
    this.pushTimer ??= this.deps.clock.setTimeout(() => {
      this.pushTimer = undefined;
      void this.flushProjects();
    }, PUSH_DELAY_MS);
  }

  async flushProjects(): Promise<void> {
    const work = [...this.pending.entries()];
    this.pending.clear();
    for (const [localId, fields] of work) {
      const rec = this.deps.projects.record(localId);
      if (!rec?.team) continue;
      const args: Record<string, unknown> = { id: rec.team.id };
      for (const f of fields) if ((SYNCED_FIELDS as readonly string[]).includes(f)) args[f] = rec[f];
      const repos = await this.reposOf(rec.folders);
      if (repos.length) args.addRepos = repos;
      try {
        const r = (await this.call('mutation', 'team:updateProject', args)) as { updatedAt: number };
        const now = this.deps.projects.record(localId);
        if (now?.team) this.deps.projects.setTeam(localId, { ...now.team, syncedAt: Math.max(now.team.syncedAt ?? 0, r.updatedAt) });
      } catch (e) {
        this.deps.log.warn('team project not sent', { project: localId, error: hubMessage(e) });
      }
    }
  }

  // A note an agent added to a linked project: into the team's log too.
  async logNote(localProjectId: string, text: string, threadId?: string): Promise<void> {
    const rec = this.deps.projects.record(localProjectId);
    if (!rec?.team || !this.cfg || !this.deps.on()) return;
    const title = threadId ? this.deps.host.thread(threadId)?.title : undefined;
    await this.call('mutation', 'team:addLog', { projectId: rec.team.id, text: text.trim().slice(0, 4000), ...(title ? { threadTitle: title } : {}) }).catch((e) => this.deps.log.warn('team log not sent', { error: hubMessage(e) }));
  }

  private onState(s: HubState): void {
    if (!s || !s.me) return;
    this.hubState = s;
    this.connected = true;
    this.error = undefined;
    if (this.cfg && (s.team.name !== this.cfg.teamName || s.me.name !== this.cfg.name || s.me.host !== this.cfg.host)) {
      this.cfg = { ...this.cfg, teamName: s.team.name, name: s.me.name, host: s.me.host };
      this.writeConfig(this.cfg);
    }
    // The hub's changes into the linked projects here.
    const live = new Set(s.projects.map((p) => p.id));
    for (const rec of this.linked()) {
      const hub = s.projects.find((p) => p.id === rec.team!.id);
      if (!hub) {
        if (!live.has(rec.team!.id)) {
          // No longer shared: it stays here as this Mac's own.
          this.deps.projects.setTeam(rec.id, undefined);
          this.deps.log.info('team project no longer shared', { project: rec.id });
        }
        continue;
      }
      if (hub.updatedBy === this.cfg?.memberId || hub.updatedAt <= (rec.team!.syncedAt ?? 0)) continue;
      const patch: Record<string, unknown> = {};
      for (const f of SYNCED_FIELDS) if (JSON.stringify(hub[f]) !== JSON.stringify(rec[f]) && !this.pending.get(rec.id)?.has(f)) patch[f] = hub[f];
      this.applying = true;
      try {
        if (Object.keys(patch).length) this.deps.projects.update(rec.id, { ...patch, by: 'team' } as never);
        this.deps.projects.setTeam(rec.id, { ...this.deps.projects.record(rec.id)!.team!, syncedAt: hub.updatedAt });
      } catch (e) {
        this.deps.log.warn('team project not applied', { project: rec.id, error: e instanceof Error ? e.message : String(e) });
      } finally {
        this.applying = false;
      }
    }
    this.queueAllCards();
    this.changed();
  }

  // ------------------------------------------------------------ cards

  private onRunner(n: Notification): void {
    if (!this.cfg || !this.deps.on()) return;
    if (n.method === 'thread.updated') this.queueCard(n.params.thread.id);
    else if (n.method === 'thread.deleted') this.queueCard(n.params.threadId);
  }

  private queueAllCards(): void {
    for (const t of this.deps.host.threads()) this.queueCard(t.id);
    // Cards sent for threads no longer here.
    for (const k of this.sentCards.keys()) this.dirtyThreads.add(k.split('|')[1]!);
  }

  private queueCard(threadId: string): void {
    this.dirtyThreads.add(threadId);
    this.cardTimer ??= this.deps.clock.setTimeout(() => {
      this.cardTimer = undefined;
      void this.flushCards();
    }, CARD_EVERY_MS);
  }

  // The cards this Mac's threads should have now, by project|thread.
  private cardsFor(threadId: string): Map<string, Record<string, unknown>> {
    const out = new Map<string, Record<string, unknown>>();
    const t = this.deps.host.thread(threadId);
    if (!t) return out;
    for (const m of t.projects ?? []) {
      const rec = this.deps.projects.record(m.id);
      if (!rec?.team) continue;
      out.set(`${rec.team.id}|${t.id}`, {
        projectId: rec.team.id,
        threadId: t.id,
        title: t.title,
        status: cardStatus(t),
        ...(t.preview ? { headline: oneLine(t.preview, 200) } : {}),
        ...(t.worktree?.branch ? { branch: t.worktree.branch } : {}),
      });
    }
    return out;
  }

  async flushCards(): Promise<void> {
    if (!this.cfg || !this.hub) return;
    const ids = [...this.dirtyThreads];
    this.dirtyThreads.clear();
    const cards: Record<string, unknown>[] = [];
    const gone: { projectId: string; threadId: string }[] = [];
    const sentNow = new Map<string, string>();
    for (const id of ids) {
      const want = this.cardsFor(id);
      for (const [k, c] of want) {
        const t = this.deps.host.thread(id);
        const repo = t ? await this.remoteOf(t.worktree?.mainCheckout ?? t.cwd) : undefined;
        const card = { ...c, ...(repo ? { repo } : {}) };
        const json = JSON.stringify(card);
        if (this.sentCards.get(k) !== json) {
          cards.push(card);
          sentNow.set(k, json);
        }
      }
      for (const k of [...this.sentCards.keys()]) {
        const [projectId, threadId] = k.split('|') as [string, string];
        if (threadId === id && !want.has(k)) {
          gone.push({ projectId, threadId });
          this.sentCards.delete(k);
        }
      }
    }
    if (!cards.length && !gone.length) return;
    try {
      for (let i = 0; i < Math.max(cards.length, 1); i += 100) await this.call('mutation', 'team:putCards', { cards: cards.slice(i, i + 100), ...(i === 0 && gone.length ? { gone } : {}) });
      for (const [k, v] of sentNow) this.sentCards.set(k, v);
    } catch (e) {
      // Sent again with the next change.
      for (const id of ids) this.dirtyThreads.add(id);
      this.deps.log.warn('team cards not sent', { error: hubMessage(e) });
    }
  }

  // ------------------------------------------------------------ messages

  private onLive(l: HubLive): void {
    if (!l || !Array.isArray(l.cards)) return;
    this.hubLive = l;
    this.connected = true;
    // Cards of mine the hub has that this runner hasn't sent since it
    // started: what it'd send now is compared against them.
    if (this.cfg) {
      for (const c of l.cards) {
        if (c.memberId !== this.cfg.memberId) continue;
        const k = `${c.projectId}|${c.threadId}`;
        if (!this.sentCards.has(k)) {
          this.sentCards.set(k, '(hub)');
          this.dirtyThreads.add(c.threadId);
        }
      }
      if (this.dirtyThreads.size) this.queueCard([...this.dirtyThreads][0]!);
    }
    let saved = false;
    for (const m of l.inbox) {
      if (this.seen.has(m.id)) continue;
      this.seen.add(m.id);
      saved = true;
      void this.arrived(m);
    }
    const told: string[] = [];
    for (const m of l.answered) {
      if (this.seen.has(`told:${m.id}`)) {
        told.push(m.id);
        continue;
      }
      this.seen.add(`told:${m.id}`);
      saved = true;
      this.tellSender(m);
      told.push(m.id);
    }
    if (saved) this.saveSeen();
    if (told.length) void this.call('mutation', 'team:told', { ids: told }).catch(() => {});
    this.changed();
  }

  private framed(m: HubMessage): string {
    const from = this.nameOf(m.from);
    const ref = cardRef(from, m.fromThread);
    if (m.kind === 'message') {
      return `[From ${from}'s agent, in their thread "${m.fromTitle}", via the ${this.cfg?.teamName ?? ''} team. Answer with send_message to ${ref}.]\n\n${m.text}`;
    }
    const lines = [`[Handover from ${from}, via the ${this.cfg?.teamName ?? ''} team: their thread "${m.fromTitle}" is yours now.]`, '', m.text];
    if (m.repo) lines.push('', `Repo: ${m.repo}${m.branch ? `, branch ${m.branch} (git fetch, then check it out; ask ${from} to push it if it isn't there)` : ''}.`);
    if (m.links?.length) lines.push('', 'Links:', ...m.links.map((x) => `- ${x.title}: ${x.url}`));
    lines.push('', `Get your bearings first, then tell ${from} you've taken it with send_message to ${ref}.`);
    return lines.join('\n');
  }

  private localFor(sharedProjectId: string): ProjectRecord | undefined {
    return this.linkedTo(sharedProjectId);
  }

  private async arrived(m: HubMessage): Promise<void> {
    const from = this.nameOf(m.from);
    const local = this.localFor(m.projectId);
    const target = m.toThread ? this.deps.host.thread(m.toThread) : undefined;
    if (m.kind === 'message' && target) {
      if (local?.team?.delivery === 'deliver') {
        await this.answer({ id: m.id, action: 'pass' }).catch((e) => this.deps.log.warn('team message not delivered', { error: e instanceof Error ? e.message : String(e) }));
        return;
      }
      this.deps.host.note(target.id, `${from}'s agent sent this thread a message (from "${m.fromTitle}"). It waits in the project's Team section for you to pass it on or dismiss: "${oneLine(m.text, NOTE_PREVIEW)}"`);
      return;
    }
    const what = m.kind === 'handover' ? `${from} handed you their thread "${m.fromTitle}"` : `${from}'s agent sent you a message`;
    this.deps.host.publish({ method: 'runner.notice', params: { level: 'info', message: `${what}${local ? ` in ${local.title}` : ''}. It's waiting in the project's Team section.` } });
  }

  private tellSender(m: HubMessage): void {
    const t = this.deps.host.thread(m.fromThread);
    if (!t) return;
    const to = this.nameOf(m.to);
    const why = m.answer ? `: ${m.answer}` : '';
    const text =
      m.status === 'passed' ? `${to} passed your message on to their thread.`
      : m.status === 'dismissed' ? `${to} dismissed your message${why}.`
      : m.status === 'taken' ? `${to} took over this work${m.answer ? ` in their thread "${m.answer}"` : ''}.`
      : `${to} declined the handover${why}.`;
    this.deps.host.note(t.id, text);
  }

  // Pass on or dismiss a message; take or decline a handover.
  async answer(p: { id: string; action: 'pass' | 'dismiss' | 'take' | 'decline'; reason?: string }): Promise<{ threadId?: string }> {
    this.mustJoined();
    const m = this.hubLive?.inbox.find((x) => x.id === p.id);
    if (!m) throw bad('That is no longer waiting.');
    const local = this.localFor(m.projectId);
    let threadId: string | undefined;
    let answer: string | undefined = p.reason;
    if (p.action === 'pass') {
      if (m.kind !== 'message') throw bad('A handover is taken or declined.');
      const target = m.toThread ? this.deps.host.thread(m.toThread) : undefined;
      if (target) {
        this.deps.host.send(target.id, this.framed(m));
        threadId = target.id;
      } else {
        const t = this.deps.host.create({ text: this.framed(m), cwd: local?.folders[0], title: `From ${this.nameOf(m.from)}: ${oneLine(m.fromTitle, 50)}`, projectIds: local ? [local.id] : [] });
        threadId = t.id;
      }
    } else if (p.action === 'take') {
      if (m.kind !== 'handover') throw bad('A message is passed on or dismissed.');
      let cwd = local?.folders[0];
      if (m.repo) cwd = (await this.findClones([m.repo])).folders[0] ?? cwd;
      const t = this.deps.host.create({ text: this.framed(m), cwd, title: oneLine(m.fromTitle, 80), projectIds: local ? [local.id] : [] });
      threadId = t.id;
      answer = t.title;
    } else if (p.action === 'dismiss' && m.kind !== 'message') throw bad('A handover is taken or declined.');
    else if (p.action === 'decline' && m.kind !== 'handover') throw bad('A message is passed on or dismissed.');
    const status = { pass: 'passed', dismiss: 'dismissed', take: 'taken', decline: 'declined' }[p.action];
    if (!status) throw bad('action is pass, dismiss, take or decline');
    await this.call('mutation', 'team:answer', { id: m.id, status, ...(answer ? { answer } : {}) });
    if (this.hubLive) this.hubLive = { ...this.hubLive, inbox: this.hubLive.inbox.filter((x) => x.id !== m.id) };
    this.changed();
    return threadId ? { threadId } : {};
  }

  // A card an agent named (team:<member>:<thread8>), as the board has it.
  resolveCard(ref: string): TeamCardView | undefined {
    const want = ref.trim().toLowerCase();
    if (!want.startsWith('team:')) return undefined;
    return this.state().cards.find((c) => !c.mine && c.id === want);
  }

  isCardRef(ref: string): boolean {
    return /^team:/i.test(ref.trim());
  }

  // send_message to a teammate's thread.
  async sendToCard(p: { ref: string; text: string; fromThreadId: string }): Promise<string> {
    this.mustJoined();
    const card = this.resolveCard(p.ref);
    if (!card) throw bad(`No teammate's thread ${p.ref}. read_project lists them on the team board.`);
    const from = this.deps.host.thread(p.fromThreadId);
    const r = (await this.call('mutation', 'team:send', {
      message: { id: randomId('tm_'), projectId: card.projectId, kind: 'message', to: card.memberId, toThread: card.threadId, fromThread: p.fromThreadId, fromTitle: from?.title ?? 'Untitled', text: p.text },
    })) as { toName: string };
    return `Sent to ${r.toName}'s thread "${card.title}". It reaches their Mac through the team; you'll get a note here when they pass it on or dismiss it.`;
  }

  // hand_over: this thread's work to a teammate.
  async handOver(p: { to: string; note: string; threadId: string; links?: ProjectLink[] }): Promise<string> {
    this.mustJoined();
    const m = this.member(p.to);
    if (m.memberId === this.cfg!.memberId) throw bad("That's you.");
    const t = this.deps.host.thread(p.threadId);
    if (!t) throw bad(`No thread ${p.threadId}`);
    const shared = (t.projects ?? []).map((x) => this.deps.projects.record(x.id)).find((r) => r?.team);
    if (!shared) throw bad("This thread isn't in a shared project, so there's no team to hand it to. join_project a shared one first.");
    const repo = await this.remoteOf(t.worktree?.mainCheckout ?? t.cwd);
    await this.call('mutation', 'team:send', {
      message: {
        id: randomId('tm_'),
        projectId: shared.team!.id,
        kind: 'handover',
        to: m.memberId,
        fromThread: t.id,
        fromTitle: t.title,
        text: p.note,
        ...(repo ? { repo } : {}),
        ...(t.worktree?.branch ? { branch: t.worktree.branch } : {}),
        ...(p.links?.length ? { links: p.links.slice(0, 8) } : {}),
      },
    });
    // Its card goes up now with the mark.
    this.queueCard(t.id);
    return `Handed over to ${m.name}. It waits on their Mac until they take it; you'll get a note here when they do.${t.worktree?.branch ? ` Push ${t.worktree.branch} so they can check it out.` : ''}`;
  }

  // ------------------------------------------------------------ reading

  joined(): boolean {
    return Boolean(this.cfg) && this.deps.on();
  }

  state(): TeamState {
    const on = this.deps.on();
    const cfg = this.cfg;
    if (!on || !cfg) return { on, joined: Boolean(cfg), connected: false, members: [], projects: [], cards: [], waiting: [], log: {} };
    const now = this.deps.clock.now();
    const s = this.hubState;
    const members: TeamMemberView[] = (s?.members ?? []).map((m) => ({
      memberId: m.memberId,
      name: m.name,
      host: m.host,
      me: m.memberId === cfg.memberId,
      online: m.memberId === cfg.memberId ? this.connected : Boolean(m.seenAt && now - m.seenAt < ONLINE_MS),
      ...(m.seenAt ? { seenAt: iso(m.seenAt) } : {}),
    }));
    const linked = new Map(this.linked().map((r) => [r.team!.id, r.id]));
    const projects: SharedProjectView[] = (s?.projects ?? []).map((p) => ({
      id: p.id,
      title: p.title,
      sharedBy: this.nameOf(p.sharedBy),
      repos: p.repos,
      updatedAt: iso(p.updatedAt),
      mine: p.sharedBy === cfg.memberId,
      ...(linked.has(p.id) ? { localProjectId: linked.get(p.id) } : {}),
    }));
    const cards: TeamCardView[] = (this.hubLive?.cards ?? []).map((c) => {
      const memberName = this.nameOf(c.memberId);
      return { ...c, id: cardRef(memberName, c.threadId), memberName, mine: c.memberId === cfg.memberId, ...(c.handedTo ? { handedToName: this.nameOf(c.handedTo) } : {}) };
    });
    const waiting: TeamWaiting[] = (this.hubLive?.inbox ?? []).map((m) => {
      const t = m.toThread ? this.deps.host.thread(m.toThread) : undefined;
      const local = linked.get(m.projectId);
      return {
        id: m.id,
        kind: m.kind,
        projectId: m.projectId,
        ...(local ? { localProjectId: local } : {}),
        fromName: this.nameOf(m.from),
        fromTitle: m.fromTitle,
        ...(m.toThread ? { toThread: m.toThread } : {}),
        ...(t ? { toThreadTitle: t.title } : {}),
        text: m.text,
        ...(m.repo ? { repo: m.repo } : {}),
        ...(m.branch ? { branch: m.branch } : {}),
        ...(m.links ? { links: m.links } : {}),
        createdAt: iso(m.createdAt),
      };
    });
    const log: TeamState['log'] = {};
    for (const p of s?.projects ?? []) log[p.id] = p.log.map((l) => ({ ...l, memberName: this.nameOf(l.memberId) }));
    return {
      on,
      joined: true,
      connected: this.connected,
      ...(this.error ? { error: this.error } : {}),
      url: cfg.url,
      name: cfg.teamName,
      ...(members.find((m) => m.me) ? { me: members.find((m) => m.me) } : { me: { memberId: cfg.memberId, name: cfg.name, host: cfg.host, me: true, online: this.connected } }),
      members,
      projects,
      cards,
      waiting,
      log,
    };
  }

  // What a session in a linked project is told about the team, for its brief.
  brief(rec: ProjectRecord): string | undefined {
    if (!rec.team || !this.joined()) return undefined;
    const st = this.state();
    const others = st.members.filter((m) => !m.me).map((m) => m.name);
    const cards = st.cards.filter((c) => c.projectId === rec.team!.id && !c.mine && c.status !== 'done').sort((a, b) => b.updatedAt - a.updatedAt);
    const lines = [`Shared with the ${st.name} team${others.length ? `: ${others.join(', ')}` : ''}. Notes you add with appendNote reach every teammate's agents.`];
    if (cards.length) {
      lines.push("Teammates' threads now:");
      for (const c of cards.slice(0, BRIEF_CARDS)) lines.push(`- ${c.id} ${c.memberName}, "${oneLine(c.title, 60)}", ${c.status === 'needsThem' ? 'waiting on them' : c.status}${c.headline ? `: ${oneLine(c.headline, 90)}` : ''}`);
      if (cards.length > BRIEF_CARDS) lines.push(`- and ${cards.length - BRIEF_CARDS} more (read_project)`);
      lines.push("Before you change something a teammate's thread is working on, tell it with send_message to its team: id.");
    }
    const log = (st.log[rec.team.id] ?? []).slice(-BRIEF_LOG);
    if (log.length) {
      lines.push('Latest team notes:');
      for (const l of log) lines.push(`- ${l.memberName}: ${oneLine(l.text, 160)}`);
    }
    return lines.join('\n');
  }

  // read_project's team part.
  projectLines(rec: ProjectRecord): string[] {
    if (!rec.team || !this.joined()) return [];
    const st = this.state();
    const cards = st.cards.filter((c) => c.projectId === rec.team!.id && !c.mine);
    const out = ['', `Team: ${st.name}, ${st.members.map((m) => `${m.name}${m.me ? ' (you)' : m.online ? ' (online)' : ''}`).join(', ')}`];
    out.push(`Teammates' threads (${cards.length}):`);
    if (!cards.length) out.push('- none yet');
    for (const c of cards.sort((a, b) => b.updatedAt - a.updatedAt).slice(0, 30)) {
      out.push(`- ${c.id} ${c.memberName}: "${c.title}", ${c.status}${c.branch ? `, branch ${c.branch}` : ''}${c.repo ? ` in ${c.repo}` : ''}${c.handedToName ? `, handed to ${c.handedToName}` : ''}${c.headline ? ` · ${oneLine(c.headline, 120)}` : ''}`);
    }
    const log = st.log[rec.team.id] ?? [];
    if (log.length) {
      out.push('Team notes (newest last):');
      for (const l of log.slice(-10)) out.push(`- ${l.memberName}${l.threadTitle ? ` in "${l.threadTitle}"` : ''}: ${l.text}`);
    }
    out.push('send_message to a team: id reaches that teammate\'s thread; hand_over gives this thread\'s work to a teammate.');
    return out;
  }

  members(): TeamMemberView[] {
    return this.state().members;
  }

  // ------------------------------------------------------------ the app

  private changed(): void {
    this.publishTimer ??= this.deps.clock.setTimeout(() => {
      this.publishTimer = undefined;
      this.deps.host.publish({ method: 'team.updated', params: { team: this.state() } } as Notification);
    }, 50);
  }

  // The experiment was turned on or off: connect or let go.
  async experimentChanged(): Promise<void> {
    if (this.deps.on() && this.cfg && !this.hub) this.connect();
    else if (!this.deps.on() && this.hub) await this.disconnect();
    this.changed();
  }
}

function listDirs(dir: string): string[] {
  try {
    return fs
      .readdirSync(dir, { withFileTypes: true })
      .filter((d) => d.isDirectory() && !d.name.startsWith('.') && d.name !== 'node_modules' && d.name !== 'Library')
      .map((d) => path.join(dir, d.name));
  } catch {
    return [];
  }
}

export function teamMethods(team: Team): Record<string, (p: Record<string, unknown>) => unknown> {
  const str = (v: unknown, what: string): string => {
    if (typeof v !== 'string' || !v.trim()) throw bad(`${what} is required`);
    return v;
  };
  return {
    'team.get': () => ({ team: team.state() }),
    'team.create': async (p) => ({ team: await team.create({ name: str(p.name, 'name'), memberName: typeof p.memberName === 'string' ? p.memberName : undefined }) }),
    'team.invite': async (p) => team.invite({ days: typeof p.days === 'number' ? p.days : undefined }),
    'team.join': async (p) => ({ team: await team.join({ invite: str(p.invite, 'invite'), name: typeof p.name === 'string' ? p.name : undefined }) }),
    'team.leave': async () => ({ team: await team.leave() }),
    'team.remove': async (p) => ({ team: await team.remove({ member: str(p.member, 'member') }) }),
    'team.share': async (p) => ({ team: await team.share({ projectId: str(p.projectId, 'projectId') }) }),
    'team.add': async (p) => team.add({ sharedId: str(p.sharedId, 'sharedId') }),
    'team.unshare': async (p) => ({ team: await team.unshare({ projectId: str(p.projectId, 'projectId') }) }),
    'team.unlink': (p) => ({ team: team.unlink({ projectId: str(p.projectId, 'projectId') }) }),
    'team.setDelivery': (p) => ({ team: team.setDelivery({ projectId: str(p.projectId, 'projectId'), delivery: str(p.delivery, 'delivery') as 'ask' | 'deliver' }) }),
    'team.answer': async (p) => ({ ...(await team.answer({ id: str(p.id, 'id'), action: str(p.action, 'action') as 'pass', reason: typeof p.reason === 'string' ? p.reason : undefined })), team: team.state() }),
  };
}

export type { ProjectTeam };
