// A shared thread's rows as far back as the reader has gone: the newest from
// share:view live, and older chunks read once with beforeSeq. The part of
// share.tsx with no Convex in it, so it can be tested alone (the phone's
// MessageHistory.swift does the same).
//
// The live query stays the size it opened with. It used to grow by raising
// its limit, up to 2,000 rows, and a live query reads its whole page again for
// every new row: a scrolled-back phone doing the same cost 37.6 MB of Convex
// I/O in a day (8 Nov). Older rows are read once, as past rows don't change.

export type Row = { seq: number };
export type Picture = { seq: number; url: string };
export type Page<R extends Row, P extends Picture> = { messages: R[]; images: P[]; hasMore: boolean };

export type History<R extends Row, P extends Picture> = {
  live?: Page<R, P>;
  // Older rows from chunks, oldest first, all before the live page's.
  older: R[];
  olderImages: P[];
  // Whether there's more before `older`; undefined until a chunk has come.
  olderHasMore?: boolean;
  // What chunks the reader loaded added, on top of the caps below.
  loaded?: { rows: number; pictures: number; bytes: number };
  // The lowest seq this view has held: only rows below it earn room.
  lowest?: number;
  // A chunk brought nothing that stayed, so no more are read.
  stuck?: boolean;
};

// How much more than the reader has loaded with "Load earlier" `older`
// keeps, in rows, pictures and bytes (about: see rowBytes), the same as one
// page of share:view can be at most (PAGE_MAX, PAGE_BYTES in
// convex/share.ts). Rows sliding off the live page used to be kept for as
// long as the page was open, so a runner writing to its own shared thread a
// few rows at a time grew the reader's memory without end, every row drawn
// (Daybreak, 10 Nov). Now what slides off by itself fits in this, past which
// the oldest rows go (and can be loaded again). Each chunk loaded raises the
// allowance by the size of what it brings that's older than anything the
// page has held, up to LOADED_MAX times the caps, so going back is never cut
// off short of that and memory only grows as far back as the reader goes.
// Rows let go and read again earn nothing (Codex's review: the phone, which
// loads by itself, could be made to fetch them over and over, each time
// adding room), and a chunk that brings nothing that stays ends loading.
export const OLDER_MAX_ROWS = 2000;
export const OLDER_MAX_PICTURES = 2000;
export const OLDER_MAX_BYTES = 8 * 1024 * 1024;
export const LOADED_MAX = 10;

// About how much a row or picture holds: the length of each of its strings,
// plus a little for the rest. Lengths the engine already knows, so it costs
// next to nothing per update.
export function rowBytes(r: object): number {
  let n = 64;
  for (const value of Object.values(r)) {
    if (typeof value === 'string') n += value.length;
    else if (Array.isArray(value)) n += value.length * 256;
  }
  return n;
}

// Keeps one of each picture, then lets the oldest of `older` go until it's
// within its allowance, with their pictures. Walks from the newest, so the
// rows nearest the live page stay. A picture counts with the row it goes
// after (the nearest at or before its seq), and one before every row (or
// with no rows kept) counts on its own, so no picture goes uncounted.
// Pictures came again with every live answer before (Codex's review): one
// at a seq below the first row, with no older rows, was added each time.
function capOlder<R extends Row, P extends Picture>(h: History<R, P>): History<R, P> {
  const seen = new Set<string>();
  const olderImages = h.olderImages.filter((p) => !seen.has(p.url) && seen.add(p.url));
  const loaded = h.loaded ?? { rows: 0, pictures: 0, bytes: 0 };
  const maxRows = OLDER_MAX_ROWS + loaded.rows;
  const maxPictures = OLDER_MAX_PICTURES + loaded.pictures;
  const maxBytes = OLDER_MAX_BYTES + loaded.bytes;
  const older = h.older;
  const pics = [...olderImages].sort((a, b) => b.seq - a.seq);
  let kept = 0;
  let pictureCount = 0;
  let total = 0;
  let keepFrom = older.length;
  while (keepFrom > 0) {
    const m = older[keepFrom - 1]!;
    let size = rowBytes(m);
    let n = 0;
    while (kept + n < pics.length && pics[kept + n]!.seq >= m.seq) size += rowBytes(pics[kept + n++]!);
    if (older.length - keepFrom + 1 > maxRows || pictureCount + n > maxPictures || total + size > maxBytes) break;
    total += size;
    pictureCount += n;
    kept += n;
    keepFrom--;
  }
  if (keepFrom === 0) {
    while (kept < pics.length && pictureCount < maxPictures && total + rowBytes(pics[kept]!) <= maxBytes) {
      total += rowBytes(pics[kept]!);
      pictureCount++;
      kept++;
    }
  }
  if (keepFrom === 0 && kept === pics.length) return { ...h, olderImages };
  const keep = new Set(pics.slice(0, kept).map((p) => p.url));
  return {
    ...h,
    older: older.slice(keepFrom),
    olderImages: olderImages.filter((p) => keep.has(p.url)),
    olderHasMore: keepFrom > 0 ? true : h.olderHasMore,
  };
}

export function empty<R extends Row, P extends Picture>(): History<R, P> {
  return { older: [], olderImages: [] };
}

export function rows<R extends Row, P extends Picture>(h: History<R, P>): R[] {
  return [...h.older, ...(h.live?.messages ?? [])];
}

// The pictures of every row shown, each once.
export function pictures<R extends Row, P extends Picture>(h: History<R, P>): P[] {
  const seen = new Set<string>();
  return [...h.olderImages, ...(h.live?.images ?? [])].filter((p) => !seen.has(p.url) && seen.add(p.url));
}

export function hasMore<R extends Row, P extends Picture>(h: History<R, P>): boolean {
  return !h.stuck && (h.olderHasMore ?? h.live?.hasMore ?? false);
}

export function firstSeq<R extends Row, P extends Picture>(h: History<R, P>): number | undefined {
  return h.older[0]?.seq ?? h.live?.messages[0]?.seq;
}

// A new answer from the live query. Rows that slide off its top as new ones
// come in move to `older`, or there'd be a hole above the live ones.
export function takeLive<R extends Row, P extends Picture>(h: History<R, P>, page: Page<R, P>): History<R, P> {
  const next = takeLiveRows(h, page);
  const first = page.messages[0]?.seq;
  return first === undefined ? next : { ...next, lowest: Math.min(next.lowest ?? first, first) };
}

function takeLiveRows<R extends Row, P extends Picture>(h: History<R, P>, page: Page<R, P>): History<R, P> {
  const first = page.messages[0]?.seq;
  const old = h.live;
  if (!old || first === undefined) return { ...h, live: page };
  const last = old.messages[old.messages.length - 1]?.seq;
  // Nothing in common with the answer before (the page was away while a whole
  // page of new rows came): what's between is unknown, so start again from
  // the newest rather than show a hole.
  if (last !== undefined && last < first) return { ...empty<R, P>(), live: page };
  // Its top can also move back up (big rows above the byte cap that got
  // smaller): those rows are the live page's again.
  const older = h.older.filter((m) => m.seq < first);
  const olderImages = h.olderImages.filter((p) => p.seq < first);
  const after = older[older.length - 1]?.seq ?? -Infinity;
  return capOlder({
    ...h,
    live: page,
    older: [...older, ...old.messages.filter((m) => m.seq < first && m.seq > after)],
    olderImages: [...olderImages, ...old.images.filter((p) => p.seq < first && p.seq > after)],
  });
}

// The seq "Load earlier messages" reads a chunk before, or undefined when
// there's no more.
export function nextBefore<R extends Row, P extends Picture>(h: History<R, P>): number | undefined {
  return hasMore(h) ? firstSeq(h) : undefined;
}

// A chunk of rows before `before`, or undefined if the shown rows no longer
// start there (it came too late).
export function takeOlder<R extends Row, P extends Picture>(h: History<R, P>, chunk: Page<R, P>, before: number): History<R, P> | undefined {
  if (firstSeq(h) !== before) return undefined;
  const messages = chunk.messages.filter((m) => m.seq < before);
  const images = chunk.images.filter((p) => p.seq < before);
  // What's older than anything held before is room on top of the caps, up
  // to LOADED_MAX times them; rows read again earn nothing.
  const floor = h.lowest ?? before;
  const fresh = messages.filter((m) => m.seq < floor);
  const freshImages = images.filter((p) => p.seq < floor);
  const was = h.loaded ?? { rows: 0, pictures: 0, bytes: 0 };
  const loaded = {
    rows: Math.min(was.rows + fresh.length, OLDER_MAX_ROWS * LOADED_MAX),
    pictures: Math.min(was.pictures + freshImages.length, OLDER_MAX_PICTURES * LOADED_MAX),
    bytes: Math.min(was.bytes + fresh.reduce((n, m) => n + rowBytes(m), 0) + freshImages.reduce((n, p) => n + rowBytes(p), 0), OLDER_MAX_BYTES * LOADED_MAX),
  };
  const lowest = messages[0] ? Math.min(floor, messages[0].seq) : h.lowest;
  const next = capOlder({ ...h, loaded, lowest, older: [...messages, ...h.older], olderImages: [...images, ...h.olderImages], olderHasMore: chunk.hasMore });
  // Nothing it brought stayed: stop, rather than read it again.
  return (firstSeq(next) ?? before) >= before ? { ...next, stuck: true } : next;
}
