// threadbear.mikecann.app: the built site in site/dist, served with Workers static assets. The
// Worker runs first for every request (run_worker_first in wrangler.jsonc), so the 405s and
// the security and cache headers in headers.mjs apply to every file and to the 404 page.
// Static assets do the rest: / serves index.html, and anything that isn't there gets 404.html
// with a 404. Copied from Quick Ask's site.
import { headersFor } from "./headers.mjs";

export default {
  async fetch(request, env) {
    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("Method not allowed", { status: 405, headers: { Allow: "GET, HEAD" } });
    }
    const url = new URL(request.url);
    if (url.pathname.startsWith("/media/")) return media(request, env, url.pathname);
    const response = await env.ASSETS.fetch(request);
    const headers = new Headers(response.headers);
    const extra = headersFor(new URL(request.url).pathname, headers.get("Content-Type") ?? "");
    for (const [name, value] of Object.entries(extra)) headers.set(name, value);
    // Static assets answer a Range request with the whole file, and Safari on an iPhone won't
    // play a <video> without byte ranges. The hero's clips are a few hundred KB, so cut the
    // range out here.
    const range = /^bytes=(\d*)-(\d*)$/.exec(request.headers.get("Range") ?? "");
    if (headers.get("Content-Type")?.startsWith("video/")) headers.set("Accept-Ranges", "bytes");
    if (range && response.status === 200 && headers.get("Content-Type")?.startsWith("video/")) {
      const body = await response.arrayBuffer();
      const size = body.byteLength;
      const start = range[1] ? Number(range[1]) : Math.max(0, size - Number(range[2]));
      const end = range[1] && range[2] ? Math.min(Number(range[2]), size - 1) : size - 1;
      if (start >= size || start > end) {
        return new Response(null, { status: 416, headers: { "Content-Range": `bytes */${size}` } });
      }
      headers.set("Content-Range", `bytes ${start}-${end}/${size}`);
      headers.set("Content-Length", String(end - start + 1));
      return new Response(request.method === "HEAD" ? null : body.slice(start, end + 1), { status: 206, headers });
    }
    return new Response(response.body, { status: response.status, statusText: response.statusText, headers });
  },
};

/**
 * The changelog's pictures and clips, from the R2 bucket bound as MEDIA (wrangler.jsonc): the
 * key is the path without /media/, so /media/changelog/2026-11-09/x.webp is
 * changelog/2026-11-09/x.webp. They're kept out of the repo so a fork doesn't download every
 * one; scripts/media.mjs puts them up. A name never changes once it's up (a new picture gets a
 * new name), so they're cached for good. Byte ranges come from R2 itself, since Safari won't
 * play a <video> without them.
 */
async function media(request, env, pathname) {
  const key = decodeURIComponent(pathname.slice("/media/".length));
  const type = { webp: "image/webp", png: "image/png", jpg: "image/jpeg", mp4: "video/mp4" }[key.split(".").pop()];
  if (!type || key.includes("..") || !/^[a-z0-9][a-z0-9/._-]*$/.test(key)) return notFound(request, env);
  let object;
  try {
    object = await env.MEDIA.get(key, { range: request.headers, onlyIf: request.headers });
  } catch {
    // R2 throws on a range it can't serve (past the end, say).
    return new Response(null, { status: 416 });
  }
  if (!object) return notFound(request, env);
  const headers = new Headers({ "Content-Type": type, "Accept-Ranges": "bytes", ETag: object.httpEtag });
  for (const [name, value] of Object.entries(headersFor(pathname, type))) headers.set(name, value);
  headers.set("Cache-Control", "public, max-age=31536000, immutable");
  // A conditional request whose ETag still matches: R2 gives back the object without a body.
  if (!("body" in object)) return new Response(null, { status: 304, headers });
  if (object.range && request.headers.has("Range")) {
    const { offset = 0, length = object.size - offset } = object.range;
    headers.set("Content-Range", `bytes ${offset}-${offset + length - 1}/${object.size}`);
    headers.set("Content-Length", String(length));
    return new Response(request.method === "HEAD" ? null : object.body, { status: 206, headers });
  }
  headers.set("Content-Length", String(object.size));
  return new Response(request.method === "HEAD" ? null : object.body, { headers });
}

/** The site's own 404 page, with a 404, for a media path that isn't there. */
async function notFound(request, env) {
  const page = await env.ASSETS.fetch(new Request(new URL("/404.html", request.url)));
  const headers = new Headers(page.headers);
  for (const [name, value] of Object.entries(headersFor("/404.html", "text/html"))) headers.set(name, value);
  return new Response(page.body, { status: 404, headers });
}
