// The changelog page (/changelog): what's new in each daily release, for people who use
// Threadbear rather than people reading its commits. Mike picked this layout on 10 Nov 2026
// ("release days") after seeing Wayne Sutton's bearfooding changelog
// (tame-weasel-262.convex.site): a thread runs down the page with a sewn button at each
// release, the release's biggest changes get a card with a before/after or a short clip, and
// the rest are plain Added / Changed / Fixed lines.
//
// Each release is a file in site/changelog/<YYYY-MM-DD>.json, written by the daily changelog
// thread (docs/changelog.md has how). Its pictures and clips aren't in the repo, so every fork
// doesn't have to download them: they're in the R2 bucket the Worker serves at /media/
// (worker.mjs), uploaded by scripts/media.mjs from site/.media/, which `npm run serve` also
// serves locally.
//
// A release file:
//   {
//     "release": "2026-11-09",
//     "highlights": [
//       { "kind": "added" | "changed" | "fixed", "title": "...", "text": "...", "credit"?: "Wayne Sutton",
//         "media"?: { "type": "compare", "before": "x-before.webp", "after": "x-after.webp", "width": 1240, "height": 820, "alt": "..." }
//                 | { "type": "image", "src": "x.webp", "width", "height", "alt" }
//                 | { "type": "clip", "src": "x.mp4", "poster": "x.webp", "width", "height", "alt" } }
//     ],
//     "added": ["..."], "changed": ["..."], "fixed": ["..."]
//   }
// Media names are relative to /media/changelog/<release>/.
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const KINDS = ["added", "changed", "fixed"];
const LABELS = { added: "Added", changed: "Changed", fixed: "Fixed" };
const MEDIA_NAME = /^[a-z0-9][a-z0-9-]*\.(webp|png|jpg|mp4)$/;

/** The media address for a file of a release. */
export const mediaPath = (release, name) => `/media/changelog/${release}/${name}`;

/** Every release file in `dir`, checked, newest first. A bad one stops the build with its name. */
export function readReleases(dir) {
  const files = readdirSync(dir).filter((f) => /^\d{4}-\d{2}-\d{2}\.json$/.test(f)).sort().reverse();
  return files.map((file) => {
    const where = `site/changelog/${file}`;
    let r;
    try {
      r = JSON.parse(readFileSync(join(dir, file), "utf8"));
    } catch (e) {
      throw new Error(`${where} isn't JSON: ${e.message}`);
    }
    if (r.release !== file.slice(0, 10)) throw new Error(`${where}: "release" should be ${file.slice(0, 10)}`);
    for (const k of KINDS) {
      r[k] ??= [];
      if (!Array.isArray(r[k]) || r[k].some((l) => typeof l !== "string" || !l.trim())) throw new Error(`${where}: "${k}" should be a list of lines`);
    }
    r.highlights ??= [];
    r.highlights.forEach((h, i) => {
      const at = `${where} highlight ${i + 1}`;
      if (!KINDS.includes(h.kind)) throw new Error(`${at}: kind should be added, changed or fixed`);
      if (!h.title?.trim() || !h.text?.trim()) throw new Error(`${at}: needs a title and a text`);
      const m = h.media;
      if (!m) return;
      const names = m.type === "compare" ? [m.before, m.after] : m.type === "image" ? [m.src] : m.type === "clip" ? [m.src, m.poster] : null;
      if (!names) throw new Error(`${at}: media type should be compare, image or clip`);
      if (names.some((n) => !MEDIA_NAME.test(n ?? ""))) throw new Error(`${at}: media names are lower-case-with-dashes files (.webp, .png, .jpg or .mp4)`);
      if (!(m.width > 0 && m.height > 0)) throw new Error(`${at}: media needs its width and height`);
      if (!m.alt?.trim()) throw new Error(`${at}: media needs alt text saying what it shows`);
    });
    if (!r.highlights.length && !KINDS.some((k) => r[k].length)) throw new Error(`${where} has nothing in it`);
    return r;
  });
}

/**
 * Page text: HTML-escaped, and em and en dashes made a comma and a hyphen (the build refuses a
 * page with either), braces as entities so a mention of a {{placeholder}} doesn't trip the
 * build's leftover check. `code` spans in backticks become <code>.
 */
export function pageText(s) {
  return s
    .replace(/\s*—\s*/g, ", ").replace(/–/g, "-")
    .replace(/[&<>"{}]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "{": "&#123;", "}": "&#125;" })[c])
    .replace(/`([^`]+)`/g, "<code>$1</code>");
}

/** "Monday 9 November" for a YYYY-MM-DD, and the year on its own. */
function dayName(day) {
  const d = new Date(`${day}T12:00:00Z`);
  const opts = { timeZone: "UTC" };
  return {
    weekday: d.toLocaleDateString("en-AU", { ...opts, weekday: "long" }),
    date: d.toLocaleDateString("en-AU", { ...opts, day: "numeric", month: "long" }),
    year: d.getUTCFullYear(),
  };
}

function media(release, m) {
  const src = (n) => mediaPath(release, n);
  const size = `width="${m.width}" height="${m.height}"`;
  const alt = pageText(m.alt);
  if (m.type === "image") return `<img class="cl-shot" src="${src(m.src)}" alt="${alt}" ${size} loading="lazy" decoding="async">`;
  if (m.type === "clip") {
    // Muted and playsinline so phones play it in place; site.js only plays one that's on screen.
    return `<video class="cl-shot" src="${src(m.src)}" poster="${src(m.poster)}" ${size} muted loop playsinline preload="none" aria-label="${alt}"></video>`;
  }
  // Before and after: the after picture, with the before one over it cut off at the slider
  // (site.js moves the cut; without it, it stays halfway, so both still show).
  return `<div class="cl-compare" role="group" aria-label="${alt}">
              <img class="cl-after" src="${src(m.after)}" alt="After: ${alt}" ${size} loading="lazy" decoding="async">
              <img class="cl-before" src="${src(m.before)}" alt="Before: ${alt}" ${size} loading="lazy" decoding="async">
              <span class="cl-tag cl-tag-before">Before</span><span class="cl-tag cl-tag-after">After</span>
              <input class="cl-slider" type="range" min="0" max="100" value="50" aria-label="Slide between before and after">
            </div>`;
}

function highlight(release, h) {
  const credit = h.credit ? ` <span class="cl-credit">Thanks to ${pageText(h.credit)}</span>` : "";
  return `          <article class="cl-feature">
            <p class="cl-kind ${h.kind}">${LABELS[h.kind]}</p>
            <h3>${pageText(h.title)}</h3>
            <p>${pageText(h.text)}${credit}</p>
            ${h.media ? `<div class="cl-media">\n              ${media(release, h.media)}\n            </div>` : ""}
          </article>`;
}

/** The page's content, for writePage. */
export function changelogPage({ releases, githubUrl }) {
  const sections = releases.map((r, i) => {
    const { weekday, date, year } = dayName(r.release);
    const counts = KINDS.map((k) => [k, r[k].length + r.highlights.filter((h) => h.kind === k).length]).filter(([, n]) => n);
    const tally = counts.map(([k, n]) => `${n} ${k === "added" ? "new" : k}`).join(", ");
    const previous = releases[i + 1];
    const every = previous
      ? `${githubUrl}/compare/release/${previous.release}...release/${r.release}`
      : `${githubUrl}/commits/release/${r.release}`;
    const lists = KINDS.filter((k) => r[k].length).map((k) => `            <div>
              <p class="cl-kind ${k}">${r.highlights.length ? (k === "added" ? "Also new" : `Also ${k}`) : LABELS[k]}</p>
              <ul>
${r[k].map((l) => `                <li>${pageText(l)}</li>`).join("\n")}
              </ul>
            </div>`);
    return `        <section class="cl-release" id="${r.release}" aria-labelledby="h-${r.release}">
          <span class="cl-button" aria-hidden="true"></span>
          <h2 id="h-${r.release}"><a href="#${r.release}">${weekday} ${date}<span class="cl-year"> ${year}</span></a></h2>
          <p class="cl-when">${tally}${tally ? " · " : ""}<a href="${every}">Every change</a></p>
${r.highlights.map((h) => highlight(r.release, h)).join("\n")}
${lists.length ? `          <div class="cl-lists">\n${lists.join("\n")}\n          </div>` : ""}
        </section>`;
  });
  return `    <article class="changelog">
      <header class="cl-head">
        <div>
          <h1>What's new</h1>
          <p class="lede">A new version of Threadbear comes out most days. The big changes are up top with a look at them, and the little ones are underneath.</p>
        </div>
        <img src="/img/bear-sit.webp" alt="" width="271" height="374">
      </header>
      <div class="cl-thread">
${sections.join("\n")}
      </div>
    </article>`;
}
