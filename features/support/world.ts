// One app, on its own mock runner, for each scenario.
import { After, Before, setDefaultTimeout, setWorldConstructor, Status, World, type IWorldOptions } from '@cucumber/cucumber';
import fs from 'node:fs';
import path from 'node:path';
import { sleep, type Driver, type Dump, type Row, type Thread } from './driver.ts';
import { GpuiDriver } from './gpui.ts';
import { longThreadFolder } from './long-thread.ts';
import { SwiftDriver } from './swift.ts';

// Whatever a person running the specs asked for (features/README.md), kept
// apart from the long-thread scenarios setting it for themselves.
const startingFixtures = process.env.TB_MOCK_FIXTURES;

setDefaultTimeout(90_000);

export class ThreadbearWorld extends World {
  app: Driver;
  /** What the last right-click on a sidebar row offered. */
  menuTitles?: string[];
  /** What the last command answered, for a step to read back. */
  lastAnswer?: string;
  /** A row's position a scenario wants to compare with later. */
  notedIndex?: number;
  /** A row and its height, a scenario wants to compare with later. */
  notedRow?: { id: string; height: number };
  /** What the last `share` command said. */
  shareResult?: string;

  constructor(options: IWorldOptions) {
    super(options);
    this.app = (process.env.TB_APP ?? 'gpui') === 'swift' ? new SwiftDriver() : new GpuiDriver();
  }

  /** Waits until a dump passes `check` (it's slow to land, as everything is). */
  async until<T>(what: string, check: (d: Dump) => T | undefined | false | null, ms = 15_000): Promise<T> {
    const deadline = Date.now() + ms;
    let last: Dump | undefined;
    for (;;) {
      last = await this.app.dump();
      const got = check(last);
      if (got) return got;
      if (Date.now() > deadline) throw new Error(`Never saw: ${what}\n\n${describe(last)}`);
      await sleep(120);
    }
  }

  thread(d: Dump, title: string): Thread | undefined {
    for (const s of d.sidebar.sections) {
      const t = s.threads.find((x) => x.title === title);
      if (t) return t;
    }
    return undefined;
  }

  /** Waits for a sidebar row with that title, wherever it is, and opens it. */
  async open(title: string): Promise<Thread> {
    const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
    await this.app.do({ cmd: 'open', id: t.id });
    await this.until(`"${title}" open with its rows on screen`, (d) => d.selection?.id === t.id && d.transcript.threadId === t.id && d.transcript.state === 'loaded');
    return t;
  }
}

/** A short account of what the app showed, for a failure message. */
export function describe(d: Dump): string {
  const side = d.sidebar.sections.map((s) => `${s.name}${s.collapsed ? ' (folded)' : ''}: ${s.threads.map((t) => t.title).join(' | ') || '(none)'}`).join('\n');
  const rows = d.transcript.rows.map((r: Row) => `  ${r.kind}${r.expanded ? ' (open)' : ''}: ${r.text}`).join('\n');
  return `connection: ${d.connection}\nbanner: ${d.banner?.text ?? '(none)'}\n${side}\nopen: ${d.selection?.title ?? '(none)'}, composer: ${JSON.stringify(d.composer.text)}\n${rows}`;
}

setWorldConstructor(ThreadbearWorld);

// The loading rows only show while the list is on its way, so that scenario's
// mock answers late. Before the app starts, hence above the Before below.
Before({ tags: '@sidebar.skeleton' }, () => {
  process.env.THREADBEAR_MOCK_LIST_DELAY_MS = '9000';
  process.env.TB_DEV_APP_NO_READY = '1';
});
After({ tags: '@sidebar.skeleton' }, () => {
  delete process.env.THREADBEAR_MOCK_LIST_DELAY_MS;
  delete process.env.TB_DEV_APP_NO_READY;
});

Before(async function (this: ThreadbearWorld, { pickle }) {
  // A scenario about paging needs a thread longer than one page of events.
  const long = pickle.tags.some((t) => t.name === '@long-thread');
  if (long) process.env.TB_MOCK_FIXTURES = longThreadFolder();
  else if (startingFixtures === undefined) delete process.env.TB_MOCK_FIXTURES;
  // A scenario about other machines needs the mock's bruce threads, which
  // both drivers leave out otherwise (TB_MOCK_NO_REMOTE in gpui.ts, swift.ts).
  if (pickle.tags.some((t) => t.name === '@remote-threads')) process.env.TB_MOCK_REMOTE = '1';
  else delete process.env.TB_MOCK_REMOTE;
  // The recap box needs a thread that has sat for a while.
  if (pickle.tags.some((t) => t.name === '@quiet-thread')) process.env.TB_MOCK_QUIET = '1';
  else delete process.env.TB_MOCK_QUIET;
  // Decisions on the Projects page: the mock's cards of asks in Threadbear,
  // and the projectDecisions experiment on in its hello.
  if (pickle.tags.some((t) => t.name === '@project-decisions')) process.env.TB_MOCK_DECISIONS = '1';
  else delete process.env.TB_MOCK_DECISIONS;
  await this.app.start();
});

After(async function (this: ThreadbearWorld, { result, pickle }) {
  if (result?.status === Status.FAILED) {
    const dir = path.resolve('reports/failures');
    fs.mkdirSync(dir, { recursive: true });
    const name = pickle.name.replace(/[^a-z0-9]+/gi, '-').toLowerCase();
    const file = path.join(dir, `${this.app.name}-${name}.png`);
    await this.app.shot(file).catch(() => {});
    if (fs.existsSync(file)) this.attach(fs.readFileSync(file), 'image/png');
  }
  await this.app.stop();
});
