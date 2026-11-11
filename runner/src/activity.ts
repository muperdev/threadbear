// tb activity: the Mac app's activity log (docs/guide.md, "The activity
// log"). The app writes a line for each thing done in it (a click, a menu
// item, a shortcut, a thread opened, a message sent) to
// <home>/activity/<day>.jsonl, but only while it's switched on in Settings >
// General (app/Sources/Threadbear/ActivityRecorder.swift has the fields).
// It's for an agent working on Threadbear to see how the app is really used.
// The files are read straight off the disk, so this works with the runner
// down; thread titles come from the runner when it's up.
import fs from 'node:fs';
import path from 'node:path';
import { type Config } from './config.ts';
import { newTbClient } from './tb-connect.ts';
import { RunnerClient } from './client.ts';
import type { ThreadSummary } from './protocol.ts';

export type ActivityEvent = {
  at: string;
  kind: string;
  target?: string;
  key?: string;
  button?: string;
  clicks?: number;
  modifiers?: string;
  view?: string;
  window?: string;
  panel?: string;
  thread?: string;
  mode?: string;
  project?: string;
  subagent?: string;
  page?: string;
  showing?: string[];
  new?: boolean;
  edit?: boolean;
  images?: number;
  answers?: number;
  decision?: string;
  state?: string;
  version?: string;
  build?: string;
};

type Flags = Record<string, string | boolean | string[] | undefined>;

export function activityDir(home: string): string {
  return path.join(home, 'activity');
}

// "30m", "2h", "7d" back from now, or a time ("2026-10-29T09:00").
export function parseSince(s: string, now: number): number {
  const m = /^(\d+)\s*(m|h|d)$/.exec(s.trim());
  if (m) return now - Number(m[1]) * { m: 60_000, h: 3_600_000, d: 86_400_000 }[m[2] as 'm' | 'h' | 'd'];
  const t = Date.parse(s);
  if (Number.isNaN(t)) throw new Error(`--since takes 30m, 2h, 7d or a time, not "${s}"`);
  return t;
}

// The events from `since` on, oldest first. Only the day files that can
// hold them are read (by name, a day either side for time zones), and a
// line that doesn't parse is skipped rather than stopping the rest.
export function readActivity(dir: string, since: number): ActivityEvent[] {
  let names: string[];
  try {
    names = fs.readdirSync(dir);
  } catch {
    return [];
  }
  const firstDay = new Date(since - 86_400_000).toISOString().slice(0, 10);
  const files = names.filter((n) => /^\d{4}-\d{2}-\d{2}\.jsonl$/.test(n) && n.slice(0, 10) >= firstDay).sort();
  const out: ActivityEvent[] = [];
  for (const name of files) {
    let text: string;
    try {
      text = fs.readFileSync(path.join(dir, name), 'utf8');
    } catch {
      continue;
    }
    for (const line of text.split('\n')) {
      if (!line) continue;
      try {
        const e = JSON.parse(line) as ActivityEvent;
        if (typeof e.at === 'string' && typeof e.kind === 'string' && Date.parse(e.at) >= since) out.push(e);
      } catch {
        // A line cut short by a crash.
      }
    }
  }
  return out.sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
}

// The thread an event is about, for --thread: a prefix of its id.
export function aboutThread(e: ActivityEvent, prefix: string): boolean {
  return Boolean(e.thread?.startsWith(prefix)) || Boolean(e.showing?.some((p) => p.startsWith(`thread:${prefix}`)));
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

// One event as a line of the timeline, without its time.
export function describe(e: ActivityEvent, titles: Map<string, string> = new Map()): string {
  const thread = (id: string | undefined) => (id ? `${id.slice(0, 8)}${titles.get(id) ? ` ${titles.get(id)}` : ''}` : '');
  const where = [e.panel && `in ${e.panel}`, e.window && e.window !== 'main' && `[${e.window}]`].filter(Boolean).join(' ');
  const on = e.thread ? ` · ${thread(e.thread)}` : '';
  switch (e.kind) {
    case 'click': {
      const what = e.target ? `"${e.target}"` : (e.view ?? 'nothing named');
      const how = [e.button, e.clicks && e.clicks > 1 && `x${e.clicks}`, e.modifiers && `with ${e.modifiers}`].filter(Boolean).join(' ');
      return `click   ${what}${how ? ` (${how})` : ''}${where ? ` ${where}` : ''}${on}`;
    }
    case 'menu':
      return `menu    "${e.target ?? ''}"${e.key ? ` (${e.key})` : ''}${on}`;
    case 'key':
      return `key     ${e.target ?? ''}${e.window && e.window !== 'main' ? ` [${e.window}]` : ''}${on}`;
    case 'hover':
      return `hover   "${e.target ?? ''}"${e.window && e.window !== 'main' ? ` [${e.window}]` : ''}`;
    case 'view': {
      const place = e.subagent
        ? `sub-agent ${e.subagent} of ${thread(e.thread)}`
        : e.page
          ? `${e.page} page`
          : e.project
            ? `project ${e.project}`
            : e.thread
              ? thread(e.thread)
              : 'new thread';
      return `view    ${place}${e.mode && e.mode !== 'inbox' ? ` (${e.mode})` : ''}`;
    }
    case 'panels':
      return `panels  ${(e.showing ?? []).join(', ')}`;
    case 'send': {
      const what = e.new ? 'new thread' : e.edit ? 'edit' : 'message';
      const extra = [e.images && plural(e.images, 'picture'), e.answers && plural(e.answers, 'answer')].filter(Boolean).join(', ');
      return `send    ${what}${extra ? ` with ${extra}` : ''}${on}`;
    }
    case 'approval':
      return `answer  ${e.decision ?? ''}${on}`;
    case 'window':
      return `window  ${e.window ?? ''}${e.panel ? ` (${e.panel})` : ''}`;
    case 'app':
      return `app     ${e.state ?? ''}`;
    case 'launch':
      return `launch  ${[e.version, e.build].filter(Boolean).join(' ')}${e.showing ? ` showing ${e.showing.join(', ')}` : ''}`;
    case 'quit':
      // The build says which app went: in a handover the new one has
      // launched by then.
      return `quit${e.build ? `    ${e.build}` : ''}`;
    default:
      return `${e.kind.padEnd(7)} ${e.target ?? ''}${on}`;
  }
}

const pad2 = (n: number) => String(n).padStart(2, '0');
const clock = (d: Date) => `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
const dayName = (d: Date) => d.toLocaleDateString('en-AU', { weekday: 'short', day: 'numeric', month: 'short' });

// The timeline: a line per event under a heading for each day.
export function formatTimeline(events: ActivityEvent[], titles: Map<string, string> = new Map()): string {
  const lines: string[] = [];
  let day = '';
  for (const e of events) {
    const d = new Date(e.at);
    if (dayName(d) !== day) {
      day = dayName(d);
      lines.push(day);
    }
    lines.push(`${clock(d)}  ${describe(e, titles)}`);
  }
  return lines.join('\n');
}

// What's used most: for each kind, its targets by count.
export function formatSummary(events: ActivityEvent[], titles: Map<string, string> = new Map(), top = 15): string {
  if (!events.length) return '';
  const first = new Date(events[0].at);
  const last = new Date(events[events.length - 1].at);
  const lines = [`${plural(events.length, 'event')} from ${dayName(first)} ${clock(first).slice(0, 5)} to ${dayName(last)} ${clock(last).slice(0, 5)}`];
  const count = (pick: (e: ActivityEvent) => string | undefined, kinds: string[]) => {
    const m = new Map<string, number>();
    for (const e of events) {
      if (!kinds.includes(e.kind)) continue;
      const k = pick(e);
      if (k) m.set(k, (m.get(k) ?? 0) + 1);
    }
    return [...m.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  };
  const section = (title: string, rows: [string, number][]) => {
    if (!rows.length) return;
    const total = rows.reduce((n, [, c]) => n + c, 0);
    lines.push('', `${title} (${total})`);
    for (const [k, c] of rows.slice(0, top)) lines.push(`${String(c).padStart(6)}  ${k}`);
    if (rows.length > top) lines.push(`        and ${plural(rows.length - top, 'other')}`);
  };
  section('Clicks', count((e) => (e.target ? `"${e.target}"` : e.view ? e.view : undefined) ?? 'nothing named', ['click']));
  // Outside the dock in the main window is the rails either side of it.
  section('Clicks by panel', count((e) => e.panel ?? (e.window === 'main' ? 'rails' : e.window), ['click']));
  section('Menu items', count((e) => `"${e.target}"${e.key ? ` (${e.key})` : ''}`, ['menu']));
  section('Shortcuts', count((e) => e.target, ['key']));
  section('Tooltips read', count((e) => `"${e.target}"`, ['hover']));
  section('Opened', count((e) => (e.subagent ? 'a sub-agent' : e.page ? `${e.page} page` : e.project ? 'a project' : e.thread ? 'a thread' : 'new thread'), ['view']));
  section('Threads opened most', count((e) => (e.thread && !e.subagent ? `${e.thread.slice(0, 8)}${titles.get(e.thread) ? ` ${titles.get(e.thread)}` : ''}` : undefined), ['view']));
  section('Sent', count((e) => (e.new ? 'new thread' : e.edit ? 'edit' : 'message'), ['send']));
  section('Answers', count((e) => e.decision, ['approval']));
  return lines.join('\n');
}

// Thread titles by id, from the runner if it answers within a moment.
async function threadTitles(cfg: Config): Promise<Map<string, string>> {
  const titles = new Map<string, string>();
  let client: RunnerClient | undefined;
  let timer: NodeJS.Timeout | undefined;
  try {
    client = newTbClient(cfg);
    const c = client;
    const timeout = new Promise<never>((_, reject) => (timer = setTimeout(() => reject(new Error('timeout')), 1500)));
    const r = await Promise.race([
      c.opened.then(() => c.call<{ threads: ThreadSummary[] }>('threads.list', { include: ['inbox', 'snoozed', 'archived'] })),
      timeout,
    ]);
    for (const t of r.threads) titles.set(t.id, t.title);
  } catch {
    // No titles, just ids.
  } finally {
    clearTimeout(timer);
    client?.close();
  }
  return titles;
}

export async function activityCommand(cfg: Config, f: Flags): Promise<number> {
  const dir = activityDir(cfg.home);
  if (!fs.existsSync(dir)) {
    console.error(`No activity log in ${dir}. The app only keeps one while it's switched on in Settings > General.`);
    return 1;
  }
  const now = Date.now();
  const since = parseSince(typeof f.since === 'string' ? f.since : f.summary ? '7d' : '1h', now);
  let events = readActivity(dir, since);
  if (typeof f.thread === 'string' && f.thread) events = events.filter((e) => aboutThread(e, f.thread as string));
  if (f.json) {
    for (const e of events) console.log(JSON.stringify(e));
    return 0;
  }
  if (!events.length) {
    console.error(`Nothing in the activity log since ${new Date(since).toLocaleString('en-AU')}.`);
    return 0;
  }
  const titles = await threadTitles(cfg);
  console.log(f.summary ? formatSummary(events, titles) : formatTimeline(events, titles));
  return 0;
}
