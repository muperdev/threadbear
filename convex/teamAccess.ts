// The rules for a team's hub (convex/team.ts, docs/multiplayer-design.md),
// as plain TypeScript with no Convex imports, so the runner's unit tests can
// check them without a deployment.

export const TEAM_NAME_MAX = 60;
export const MEMBER_NAME_MAX = 40;
export const TITLE_MAX = 80;
export const SHORT_MAX = 2000;
export const NOTES_MAX = 50_000;
export const LOG_MAX = 4000;
export const TEXT_MAX = 20_000;
export const LINKS_MAX = 8;
export const REPOS_MAX = 20;
// A team is a handful of people; past this it's not what this is for.
export const MEMBERS_MAX = 25;
export const PROJECTS_MAX = 50;
// One member's cards across the team's projects.
export const CARDS_MAX = 300;
// What one member may leave waiting for others at once.
export const WAITING_MAX = 50;
export const INVITE_DAYS_MAX = 14;
// The log entries state hands back per project.
export const LOG_SHOWN = 30;
// Done cards are dropped this long after their last change.
export const DONE_KEPT_MS = 7 * 24 * 3600_000;
// Answered messages the sender has been told of are dropped after this.
export const ANSWERED_KEPT_MS = 3 * 24 * 3600_000;
// A heartbeat writes seenAt at most this often.
export const SEEN_EVERY_MS = 60_000;

export const CARD_STATUSES = ['working', 'needsThem', 'idle', 'done'] as const;
export type CardStatus = (typeof CARD_STATUSES)[number];
export const ANSWERS = { message: ['passed', 'dismissed'], handover: ['taken', 'declined'] } as const;

// Ids the runners make: a prefix and 8 to 32 letters, digits, - or _.
const ID = /^[A-Za-z0-9_-]{4,64}$/;
export function validId(id: unknown): id is string {
  return typeof id === 'string' && ID.test(id);
}

// A sha256 in hex, as the runners send for keys and invite codes.
export function validHash(h: unknown): h is string {
  return typeof h === 'string' && /^[0-9a-f]{64}$/.test(h);
}

/** A name or title: trimmed, one line, not empty, at most `max`. Throws a message. */
export function cleanLine(what: string, s: unknown, max: number): string {
  if (typeof s !== 'string') throw new Error(`${what} must be text`);
  const t = s.trim().replace(/\s+/g, ' ');
  if (!t) throw new Error(`${what} is empty`);
  if (t.length > max) throw new Error(`${what} is at most ${max} characters`);
  return t;
}

/** Text up to `max`, kept as written. Throws a message. */
export function checkText(what: string, s: unknown, max: number): string {
  if (typeof s !== 'string') throw new Error(`${what} must be text`);
  if (s.length > max) throw new Error(`${what} is at most ${max} characters`);
  return s;
}

export type Link = { title: string; url: string };
export function checkLinks(links: unknown): Link[] {
  if (!Array.isArray(links)) throw new Error('links must be a list');
  if (links.length > LINKS_MAX) throw new Error(`at most ${LINKS_MAX} links`);
  return links.map((l) => {
    const o = (l ?? {}) as Record<string, unknown>;
    const url = typeof o.url === 'string' ? o.url.trim() : '';
    if (!/^https?:\/\/\S+$/i.test(url) || url.length > 2000) throw new Error(`not a web link: ${String(o.url).slice(0, 80)}`);
    const title = typeof o.title === 'string' && o.title.trim() ? o.title.trim().slice(0, TITLE_MAX) : url;
    return { title, url };
  });
}

/**
 * A git remote as one string every member's clone agrees on:
 * git@github.com:org/repo.git, https://github.com/org/repo and
 * ssh://git@github.com/org/repo all give github.com/org/repo. Undefined for
 * anything that isn't a remote (a local path).
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

export function checkRepos(repos: unknown): string[] {
  if (!Array.isArray(repos)) throw new Error('repos must be a list');
  const out: string[] = [];
  for (const r of repos) {
    const n = normalRepo(r) ?? (typeof r === 'string' && /^[\w.-]+\/[\w.\-/]+$/.test(r) ? r.toLowerCase() : undefined);
    if (n && !out.includes(n)) out.push(n);
  }
  return out.slice(0, REPOS_MAX);
}

/** Names compare without case or surrounding space. */
export function sameName(a: string, b: string): boolean {
  return a.trim().toLowerCase() === b.trim().toLowerCase();
}

/** Who may answer a message: the member it's to, with an answer its kind takes. */
export function mayAnswer(row: { to: string; kind: 'message' | 'handover'; status: string }, memberId: string, status: string): string | undefined {
  if (row.to !== memberId) return "that isn't yours to answer";
  if (row.status !== 'waiting') return 'it has already been answered';
  if (!(ANSWERS[row.kind] as readonly string[]).includes(status)) return `a ${row.kind} is answered with ${ANSWERS[row.kind].join(' or ')}`;
  return undefined;
}
