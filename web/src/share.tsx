// A thread shared by link (/s/<slug>; docs/website.md, "Sharing"):
// read-only, for anyone, with its pictures. Whoever opens it usually isn't
// using Threadbear, so it reads as a short write-up rather than a chat:
// the title, a byline, then each turn as the ask, its work folded to one
// line and the answer, with an outline of the asks to jump between. When
// this browser is signed in as Mike the page also has ShareControl, so he
// can stop sharing it or share it again.
import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import { useConvex, useMutation, useQuery } from 'convex/react';
import { useLiveQuery } from './live';
import { api } from '../../convex/_generated/api';
import type { Id } from '../../convex/_generated/dataModel';
import { MessageText, Pictures, picturesByRow, ToolRun, Worked, type PageMessage, type Picture } from './messages';
import { CommandState } from './command-state';
import { empty, hasMore, nextBefore, pictures, rows, takeLive, takeOlder, type History } from './share-history';
import { fold, splitTurns, duration, roughly, type Item, type Turn } from './turns';
import { day, modelLabel, plain, sender, sentences } from './format';
import * as Icon from './icons';

// Big enough that most threads arrive whole: the outline and byline only
// know the turns that have loaded. It's the live query's size for good:
// older rows are read once each (share-history.ts).
const FIRST_PAGE = 400;
const MORE = 200;
const SITE = 'https://threadbear.mikecann.app';

export function SharePage({ slug, token }: { slug: string; token: string | null }) {
  const convex = useConvex();
  const data = useLiveQuery(api.share.view, { slug, limit: FIRST_PAGE, ...(token ? { token } : {}) });
  const [shown, setShown] = useState<typeof data>(undefined);
  // The rows as far back as the reader has gone: the live answer, then older
  // chunks (share-history.ts).
  const [history, setHistory] = useState<History<Row, Pic>>(empty);
  const historyRef = useRef(history);
  const [loadingChunk, setLoadingChunk] = useState(false);
  useEffect(() => {
    if (!data) return;
    setShown(data);
    if (data.state !== 'ok') return;
    historyRef.current = takeLive(historyRef.current, data);
    setHistory(historyRef.current);
  }, [data]);

  const more = () => {
    const before = nextBefore(historyRef.current);
    if (before === undefined || loadingChunk) return;
    setLoadingChunk(true);
    convex
      .query(api.share.view, { slug, limit: MORE, beforeSeq: before, ...(token ? { token } : {}) })
      .then((chunk) => {
        if (chunk.state !== 'ok') return;
        const took = takeOlder(historyRef.current, chunk, before);
        if (!took) return;
        historyRef.current = took;
        setHistory(took);
      })
      // What's shown stays; the button can try again.
      .catch((e) => console.warn('could not load earlier messages', e))
      .finally(() => setLoadingChunk(false));
  };

  // The same rows until the history changes: the write-up keeps its place
  // after loading earlier ones when they change, and a new array each render
  // (a click re-renders at once) would spend that before they came.
  const all = useMemo(() => {
    if (shown?.state !== 'ok' || !history.live) return shown;
    return { ...shown, messages: rows(history), images: pictures(history), hasMore: hasMore(history) };
  }, [shown, history]);

  const title = shown?.state === 'ok' ? shown.title || 'Untitled' : undefined;
  useEffect(() => {
    document.title = title ? `${title} · Threadbear` : 'Threadbear';
  }, [title]);

  if (!shown) return <div className="pad muted">Loading…</div>;
  if (shown.state === 'gone') {
    return (
      <main className="signin">
        <img src="/icon.png" alt="" width={72} height={72} />
        <h1>This link doesn’t work</h1>
        <p className="muted">It may not be shared any more, or it has a new link now.</p>
      </main>
    );
  }
  return <WriteUp slug={slug} token={token} data={all as Shared} loading={data !== shown || loadingChunk} onMore={more} />;
}

type Row = Shared['messages'][number];
type Pic = Shared['images'][number];

type Shared = Extract<NonNullable<ReturnType<typeof useQuery<typeof api.share.view>>>, { state: 'ok' }>;

function WriteUp({ slug, token, data, loading, onMore }: { slug: string; token: string | null; data: Shared; loading: boolean; onMore: () => void }) {
  const messages = data.messages as PageMessage[];
  const turns = useMemo(() => splitTurns(messages), [messages]);
  const asked = turns.filter((t) => t.starter);
  const worked = turns.reduce((n, t) => n + (t.ms ?? 0), 0);
  const inText = useMemo(() => new Map(data.images.filter((p) => p.src).map((p) => [p.src!, p.url])), [data.images]);
  const after = useMemo(() => picturesByRow(messages, data.images), [messages, data.images]);
  const active = useActiveTurn(turns.length);

  // After loading earlier messages, stay where you were rather than jumping:
  // the turns on screen when it was asked for go back to where they sat.
  // By a turn on screen rather than the distance from the bottom, as a new
  // reply can grow the page below while they load (Codex, 9 Nov). Several,
  // as the first one shown may be cut off by the page's edge and get a new
  // first row once the rest of it comes. And only once rows older than the
  // first one shown have come: a live update can change the rows meanwhile.
  const keep = useRef<{ turns: { seq: string; top: number }[]; first: number } | null>(null);
  useEffect(() => {
    const k = keep.current;
    if (!k || (messages[0]?.seq ?? Infinity) >= k.first) return;
    keep.current = null;
    for (const t of k.turns) {
      const el = document.querySelector(`section.turn[data-turn="${t.seq}"]`);
      if (!el) continue;
      window.scrollBy(0, el.getBoundingClientRect().top - t.top);
      return;
    }
  }, [messages]);

  const started = data.createdAt ?? messages[0]?.at;
  const byline = [started ? day(started) : null, asked.length ? `${asked.length} ${asked.length === 1 ? 'turn' : 'turns'}` : null, worked >= 60_000 ? `worked ${roughly(worked)}` : null].filter(Boolean);
  const made = (
    <>
      Made with <a href={SITE}>Threadbear</a>
    </>
  );

  return (
    <div className="doc-page">
      <header className="doc-top">
        <TurnsMenu turns={turns} active={active} />
        <span className="spacer" />
        {data.manage && token ? (
          <ShareControl token={token} threadId={data.manage.threadId} share={{ access: data.manage.shared ? 'link' : 'off', slug }} onSharePage preview={previewOf(data.title, messages, data.images)} />
        ) : null}
        <CopyButton text={() => location.origin + location.pathname} label="Copy link" />
      </header>
      <div className="doc-grid">
        <nav className="outline" aria-label="Turns">
          {asked.length > 1 ? (
            <>
              <h2>
                {asked.length} turns
              </h2>
              <ol>
                {turns.map((t, i) =>
                  t.starter ? (
                    <li key={i}>
                      <a href={`#turn-${i + 1}`} className={i === active ? 'on' : ''}>
                        <span className="n">{asked.indexOf(t) + 1}</span>
                        <span className="ask">{askText(t.starter)}</span>
                      </a>
                    </li>
                  ) : null,
                )}
              </ol>
            </>
          ) : null}
          <p className="made">{made}</p>
        </nav>
        <article className="doc">
          <h1>{data.title || 'Untitled'}</h1>
          <p className="byline">
            <strong>{modelLabel(data.provider, data.model)}</strong>
            {byline.length ? ` · ${byline.join(' · ')}` : ''}
          </p>
          <hr />
          {data.hasMore ? (
            <button
              className="btn earlier"
              disabled={loading}
              onClick={() => {
                const turns = [...document.querySelectorAll<HTMLElement>('section.turn[data-turn]')]
                  .filter((el) => el.getBoundingClientRect().bottom > 0)
                  .slice(0, 5)
                  .map((el) => ({ seq: el.dataset.turn!, top: el.getBoundingClientRect().top }));
                keep.current = { turns, first: messages[0]?.seq ?? Infinity };
                onMore();
              }}
            >
              {loading ? 'Loading…' : 'Load earlier messages'}
            </button>
          ) : null}
          {turns.map((t, i) => (
            <TurnSection key={t.messages[0]!.seq} id={`turn-${i + 1}`} turn={t} busy={data.running && i === turns.length - 1} after={after} inText={inText} rows={messages} />
          ))}
          {data.running && !messages.length ? (
            <div className="working">
              <span className="spinner" /> {data.working || 'Bear with me…'}
            </div>
          ) : null}
          <p className="doc-end">{made}</p>
        </article>
      </div>
    </div>
  );
}

// One turn: the ask as a quote, its work folded, the answer, and the
// pictures from the work after the answer, large.
function TurnSection({ id, turn, busy, after, inText, rows }: { id: string; turn: Turn<PageMessage>; busy: boolean; after: Map<number, Picture[]>; inText: Map<string, string>; rows: PageMessage[] }) {
  const items = fold(turn.messages, busy);
  const fromWork: Picture[] = [];
  const seqsOf = (item: Item<PageMessage>): number[] =>
    item.kind === 'message' ? [item.m.seq] : item.kind === 'tools' ? item.items.map((m) => m.seq) : item.items.flatMap(seqsOf);
  for (const item of items) if (item.kind === 'worked') fromWork.push(...seqsOf(item).flatMap((s) => after.get(s) ?? []));

  const text = (m: PageMessage) => <MessageText text={m.text} pictures={inText} seq={m.seq} rows={rows} />;
  return (
    <section className="turn" id={id} data-turn={turn.messages[0]!.seq}>
      {items.map((item) => {
        if (item.kind === 'worked') {
          return (
            <Worked key={`w${item.key}`} ms={item.ms}>
              {item.items.map((w) =>
                w.kind === 'tools' ? (
                  <ToolRun key={w.items[0]!.seq} items={w.items} open />
                ) : w.kind === 'message' ? (
                  <Fragment key={w.m.seq}>{text(w.m)}</Fragment>
                ) : null,
              )}
            </Worked>
          );
        }
        if (item.kind === 'tools') {
          return (
            <div key={item.items[0]!.seq}>
              <ToolRun items={item.items} />
              <Pictures items={item.items.flatMap((m) => after.get(m.seq) ?? [])} />
            </div>
          );
        }
        const m = item.m;
        if (m === turn.starter) {
          const from = sender(m);
          return (
            <div key={m.seq} className={`asked ${from ? 'from' : ''}`}>
              {from ? <span className="who">{from.who}</span> : null}
              {from ? <MessageText text={from.body} pictures={inText} seq={m.seq} rows={rows} /> : text(m)}
              <Pictures items={after.get(m.seq) ?? []} />
            </div>
          );
        }
        return (
          <div key={m.seq} className="reply">
            {text(m)}
            <Pictures items={after.get(m.seq) ?? []} />
          </div>
        );
      })}
      {fromWork.length ? <Pictures items={fromWork} /> : null}
      {busy ? (
        <div className="working">
          <span className="spinner" /> Working…
        </div>
      ) : null}
    </section>
  );
}

// An ask as the outline shows it: plain text, without the line a schedule
// or another thread's agent starts it with.
function askText(m: PageMessage): string {
  const from = sender(m);
  return plain(from ? `${from.who}: ${from.body}` : m.text);
}

// The outline on a narrow screen: "3 turns" in the bar, opening to the asks.
function TurnsMenu({ turns, active }: { turns: Turn<PageMessage>[]; active: number }) {
  const [open, setOpen] = useState(false);
  const asked = turns.filter((t) => t.starter);
  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener('click', close);
    return () => window.removeEventListener('click', close);
  }, [open]);
  if (asked.length < 2) return <img className="turns-menu" src="/icon.png" alt="" width={24} height={24} />;
  return (
    <div className="turns-menu">
      <button
        className="btn"
        onClick={(e) => {
          e.stopPropagation();
          setOpen((o) => !o);
        }}
        aria-expanded={open}
      >
        {asked.length} turns <Icon.ChevronDown />
      </button>
      {open ? (
        <div className="turns-list">
          {turns.map((t, i) =>
            t.starter ? (
              <a key={i} href={`#turn-${i + 1}`} className={i === active ? 'on' : ''}>
                <span className="n">{asked.indexOf(t) + 1}</span>
                <span>{askText(t.starter).slice(0, 120)}</span>
                {t.ms && t.ms >= 1000 ? <span className="took">Worked for {duration(t.ms)}</span> : null}
              </a>
            ) : null,
          )}
        </div>
      ) : null}
    </div>
  );
}

// Which turn is being read: the last whose top has passed a third of the
// way down the window.
function useActiveTurn(count: number): number {
  const [active, setActive] = useState(0);
  useEffect(() => {
    const onScroll = () => {
      const line = window.innerHeight / 3;
      let at = 0;
      for (let i = 0; i < count; i++) {
        const el = document.getElementById(`turn-${i + 1}`);
        if (el && el.getBoundingClientRect().top <= line) at = i;
      }
      // At the bottom of the page the last turn is the one being read,
      // even if it's too short for its top to reach the line.
      if (window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 4) at = count - 1;
      setActive(at);
    };
    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, [count]);
  return active;
}

export function CopyButton({ text, label, className = 'btn' }: { text: () => string; label: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      className={className}
      onClick={async () => {
        await navigator.clipboard.writeText(text());
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      }}
    >
      <Icon.Link />
      {copied ? 'Copied' : label}
    </button>
  );
}

// What a link unfurls as (convex/http.ts): the title, the start of the
// last reply, the first picture.
export type Unfurl = { title: string; description: string; image?: string };

export function previewOf(title: string, messages: PageMessage[], images: Picture[]): Unfurl {
  const last = [...messages].reverse().find((m) => m.role === 'assistant' && m.text.trim());
  const text = last ? sentences(last.text) : '';
  return { title: title || 'Untitled', description: text.length > 200 ? `${text.slice(0, 197)}…` : text, image: images[0]?.url };
}

// Mike's sharing controls, on his own thread page and on the shared page
// when he's signed in: a switch for sharing by link, the link to copy, how
// it unfurls, and a new link. Each is a command the runner applies
// (phone:share), so it shows "Waiting for the Mac" while the Mac is
// asleep. No New link on the shared page itself: its own address would
// stop working under him.
export function ShareControl({ token, threadId, share, onSharePage, preview }: { token: string; threadId: string; share?: { access: string; slug: string }; onSharePage?: boolean; preview?: Unfurl }) {
  const send = useMutation(api.phone.share);
  const [pending, setPending] = useState<Id<'commands'> | null>(null);
  const shared = share?.access === 'link';
  const link = share ? `${location.origin}/s/${share.slug}` : undefined;
  const run = async (access: 'off' | 'link', newLink?: boolean) => setPending(await send({ token, threadId, access, ...(newLink ? { newLink: true } : {}) }));
  return (
    <details className="pop">
      <summary className={`share-btn ${shared ? 'on' : ''}`} title={shared ? 'Anyone with the link can read this thread' : 'Share this thread'}>
        {shared ? (
          <>
            <span className="live" />
            <span className="narrow-only">
              <Icon.Link />
            </span>
          </>
        ) : (
          <Icon.Share />
        )}
        <span className="word">{shared ? 'Shared' : 'Share'}</span>
      </summary>
      <div className="pop-panel share-panel">
        <div className="row-line">
          <h3>Share with a link</h3>
          <button className={`switch ${shared ? 'on' : ''}`} role="switch" aria-checked={shared} aria-label="Share with a link" disabled={pending !== null} onClick={() => run(shared ? 'off' : 'link')} />
        </div>
        <p>{shared ? 'Anyone with the link can read this thread, and new replies show up as they come in. No sign-in.' : 'Only you can see this thread.'}</p>
        {shared && link ? (
          <div className="share-link">
            <input readOnly value={link} onFocus={(e) => e.target.select()} aria-label="Link" />
            <CopyButton text={() => link} label="Copy" className="btn primary" />
          </div>
        ) : null}
        {shared && preview ? (
          <>
            <div className="split" />
            <span className="label">How the link shows up in Slack, X or Messages</span>
            <div className="unfurl">
              <span className="site">
                <img src="/icon.png" alt="" width={16} height={16} /> Threadbear
              </span>
              <span className="t">{preview.title}</span>
              {preview.description ? <span className="d">{preview.description}</span> : null}
              {preview.image ? <img src={preview.image} alt="" /> : null}
            </div>
          </>
        ) : null}
        {shared && !onSharePage ? (
          <div className="buttons">
            <button className="btn" disabled={pending !== null} onClick={() => run('link', true)} title="The old link stops working">
              New link
            </button>
          </div>
        ) : null}
        {pending ? <CommandState token={token} commandId={pending} onDone={() => setPending(null)} /> : null}
      </div>
    </details>
  );
}
