// The small decisions behind the rail's views (rail.tsx, pages.tsx), with no
// React so the tests can run them bare.

export type RunnerRow = { runnerId: string; name: string; lastSeenAt: string; draining: boolean };
export type ScheduleRow = { id: string; title: string; whenText: string; paused?: boolean; finishedAt?: string; nextRunAt?: string; threadId?: string };

// Heard from in this long counts as online, as the backend's ONLINE_MS.
const ONLINE_MS = 3 * 60_000;
// A runner not heard from in this long has probably gone for good, so its
// tile goes. It keeps a row in the backend, and its threads stay where they are.
const FORGOTTEN_MS = 7 * 24 * 3600_000;

export const isOnline = (r: { lastSeenAt: string }, now: number) => now - Date.parse(r.lastSeenAt) < ONLINE_MS;

// The runners worth showing: online ones first, then by name.
export function runnersShown<T extends RunnerRow>(rows: T[], now: number): T[] {
  return rows
    .filter((r) => now - Date.parse(r.lastSeenAt) < FORGOTTEN_MS)
    .sort((a, b) => Number(isOnline(b, now)) - Number(isOnline(a, now)) || a.name.localeCompare(b.name));
}

export function runnerState(r: RunnerRow, now: number): string {
  if (!isOnline(r, now)) return `Offline, last heard from ${sinceWords(Date.parse(r.lastSeenAt), now)} ago`;
  return r.draining ? `Updating, heard from ${sinceWords(Date.parse(r.lastSeenAt), now)} ago` : `Online, heard from ${sinceWords(Date.parse(r.lastSeenAt), now)} ago`;
}

// "12s", "4m", "3h", "2d"
export function sinceWords(at: number, now: number): string {
  const s = Math.max(0, Math.round((now - at) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

// "in 20m", "in 3h", "in 2d", "due" once its time has passed.
export function untilWords(iso: string | undefined, now: number): string {
  if (!iso) return '';
  const at = Date.parse(iso);
  return at <= now ? 'due' : `in ${sinceWords(now, at)}`;
}

// As the Mac: active soonest first, paused by name, done most recent first. A
// one-off that has run is done even if it was paused first.
export function scheduleSections<T extends ScheduleRow>(rows: T[]) {
  const done = rows.filter((s) => s.finishedAt).sort((a, b) => b.finishedAt!.localeCompare(a.finishedAt!));
  const paused = rows.filter((s) => !s.finishedAt && s.paused).sort((a, b) => a.title.localeCompare(b.title));
  const active = rows.filter((s) => !s.finishedAt && !s.paused).sort((a, b) => (a.nextRunAt ?? '￿').localeCompare(b.nextRunAt ?? '￿') || a.title.localeCompare(b.title));
  return { active, paused, done };
}

export function matchesSchedule(s: { title: string; whenText: string; text?: string }, query: string): boolean {
  const q = query.trim().toLowerCase();
  return !q || [s.title, s.whenText, s.text ?? ''].some((x) => x.toLowerCase().includes(q));
}

const GB = 1024 ** 3;

// "6.9 GB", "212 GB", "1.2 TB": a place after the point only while it matters.
export function bytesWords(n: number): string {
  if (n >= 1024 * GB) return `${(n / (1024 * GB)).toFixed(1)} TB`;
  const gb = n / GB;
  return `${gb < 100 ? gb.toFixed(1) : Math.round(gb)} GB`;
}

// Figures this old are from the last time someone looked: the runner reports
// only while it's being watched, and it isn't, or has gone quiet.
const STATS_STALE_MS = 60_000;
export const statsFresh = (s: { at: string }, now: number) => now - Date.parse(s.at) < STATS_STALE_MS;

type UsageWindow = { title: string; remainingPercent: number; resetsAt?: string };

// How much room a login has: its tightest window, or undefined when it has
// never said.
export function tightest(windows: UsageWindow[] | undefined): UsageWindow | undefined {
  return windows?.length ? [...windows].sort((a, b) => a.remainingPercent - b.remainingPercent)[0] : undefined;
}

// The recap box: the same rule as the Mac's Recap.swift, the phone's
// Recap.swift and the runner's recapShowing (docs/protocol.md, "The recap
// box"). 12 hours is AWAY_MS in runner.ts.
const RECAP_QUIET_MS = 12 * 3600_000;

export function recapDue(t: { recap?: string; status: string; archivedAt?: string; lastActivityAt: string }, dismissedAt: string | null, now: number): boolean {
  if (!t.recap || t.status !== 'idle' || t.archivedAt) return false;
  return now - Date.parse(t.lastActivityAt) >= RECAP_QUIET_MS && dismissedAt !== t.lastActivityAt;
}

// The sentence on what it's for, if the note has one, and the bullets.
export function recapParts(recap: string): { purpose?: string; points: string[] } {
  let purpose: string | undefined;
  const points: string[] = [];
  for (const raw of recap.split('\n')) {
    const line = raw.trim();
    if (!line) continue;
    if (line.startsWith('- ')) points.push(line.slice(2));
    else if (purpose === undefined && points.length === 0) purpose = line;
    else points.push(line);
  }
  return { ...(purpose !== undefined ? { purpose } : {}), points };
}

export function lastWorkedOn(at: string, now: number): string {
  const hours = Math.round((now - Date.parse(at)) / 3600_000);
  return `Last worked on ${hours < 48 ? `${hours} hours ago` : `${Math.round(hours / 24)} days ago`}`;
}
