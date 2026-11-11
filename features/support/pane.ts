// What the pane beside the thread draws (`dump`'s `panels.pane`, the same
// shape on both apps), for the highlights, previews, patches and folder steps.
import type { Dump } from './driver.ts';

export type PaneRow = { title: string; lead?: string | null; trailing?: string | null; note?: string | null; detail?: string | null; symbol?: string | null; ticked?: boolean | null };
export type PaneSection = { key: string; title: string; count: number; folded: boolean; rows: PaneRow[] };
export type Pane = {
  section: string;
  empty?: string | null;
  brief?: string[] | null;
  cards?: string[] | null;
  sections?: PaneSection[] | null;
  patches?: { id: string; source: string; waiting?: string | null; trusting?: boolean | null; gives?: string | null; error?: string | null; file?: string | null }[] | null;
  list?: PaneSection[] | null;
  image?: string | null;
  previewKind?: string | null;
  previewText?: string | null;
  folder?: string | null;
};

type WithPane = Dump & { panels?: { preview?: string | null; highlights?: string | null; pane?: Pane | null; items?: { id: string; title: string; kind: string; active: boolean }[] } | null };

export const pane = (d: Dump): Pane | undefined => (d as WithPane).panels?.pane ?? undefined;
// With / between its parts whatever the system: on Windows a path has backslashes.
export const previewLine = (d: Dump): string | undefined => (d as WithPane).panels?.preview?.replaceAll('\\', '/') ?? undefined;
export const paneItems = (d: Dump) => (d as WithPane).panels?.items ?? [];
export const sections = (d: Dump): PaneSection[] => pane(d)?.sections ?? [];
export const section = (d: Dump, title: string) => sections(d).find((s) => s.title === title);
export const row = (d: Dump, heading: string, title: string) => section(d, heading)?.rows.find((r) => r.title === title);
