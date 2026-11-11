# The website

Threadbear in a browser, so a thread has a link you can paste anywhere:
`https://opulent-moose-249.ap-southeast-2.convex.site/t/<threadId>`.
Built 28 Oct 2026 (thread 66935d23).

## What it is

- `web/`: a small Vite + React app, looking like the Mac app (refreshed
  3 Nov 2026, thread c1d311fd, from design B in
  `docs/web-refresh-design/`). Signed in, it's the Mac's window: a sidebar
  (`web/src/sidebar.tsx`) you can drag wider or narrower, with Starred,
  Inbox, Running, Hibernating and Archived under headings that fold and
  show their number. Which thread goes where is `web/src/sections.ts`, the
  same rules as the Mac's ThreadSections and the phone's InboxSections, so
  change them together. Hibernating and Archived start folded and are
  loaded only while open or while searching or filtering (`phone:counts`
  has their numbers until then). The Inbox heading sorts (needs you first,
  newest, oldest); the search looks through every section's titles, last
  replies and places, with `phone:search` for archived threads past the
  newest 100; the filter beside it picks needs you, unread, shared, or one
  project or folder; and the arrow keys walk the rows. Folds, the sort and
  the width are kept in this browser's localStorage. Down the left edge
  is a rail, as on the Mac (`web/src/rail.tsx`): Threads, Scheduled and
  Projects, a tile per runner with a dot for online, then Usage and a gear
  (appearance, sign out). Each opens a list in the sidebar and a page beside
  it (`web/src/pages.tsx`, at `/scheduled`, `/projects`, `/runners`, `/usage`,
  each with `/<id>`), which subscribe only while they're showing. A schedule
  has Pause and Run now (`phone:schedule`); anything else, and any change to a
  project, is on the Mac or through an agent. A project's page lists all its
  threads, live then archived, newest first, from `phone:projectThreads` (Show
  more for older ones). A runner's page has its CPU, memory and disk, reported
  only while that view is open (docs/protocol.md, Sync, "Runner figures"), and
  its tile an orange dot while it's low on memory or disk. The sidebar and rail
  read `phone:wakes`, not the schedules. On a phone the rail is a tab bar, with Runners and Usage under
  More. The pencil beside the search starts a thread (`/new`, `phone:create`)
  on the runner's default folder and login. And a
  thread whose finished turns fold to "Worked for 4m 12s" and the reply
  (`web/src/turns.ts`, a port of the iPhone's `Turns.fold`), with the
  pending approval, the end_turn card of asks or the done card's
  follow-ups joined to the composer. Return sends; Shift-Return is a new
  line. The header's Share button and ... menu (Copy link, Star,
  Hibernate or Wake, Archive or Bring back) do the rest. Opening a thread
  marks it seen.
- It's a client of the phone's API (`convex/phone.ts`, docs/convex-backend.md
  section 6) and nothing else, so it sees exactly what the phone sees: the
  text messages and one line per tool call, with no tool output, and only
  the images an agent showed Mike or a shared thread's. Everything
  it does is a `commands` row the runner applies.
- Hosted by `@convex-dev/static-hosting` on the same Convex deployment.
  Since 3 Nov the app owns HTTP routing (`convex/http.ts`) rather than
  mounting the component's routes at `/`: `registerStaticRoutes` serves
  the site, and one route of our own, `/s/`, serves a shared thread's
  page (below). Unknown paths fall back to `index.html`, so `/t/<id>`
  works cold.
- The Mac app's thread `...` menu has **Open on the web**. It takes the site
  from `runner.status`'s `sync.url` (`.convex.cloud` → `.convex.site`), so
  it only shows while sync is on.

## Sharing

A thread can be shared by link (docs/protocol.md, "Sharing"): from the
share button beside the Mac app's ... menu, or from the website, where
Mike's own thread page and the shared page (when he's signed in) have a
Share button: Share with a link, Copy, Stop sharing, and on his thread
page New link. Those queue a `share` command (`phone:share`) the runner
applies, as the Mac's button does. Built 28-29 Oct 2026 (thread ec75fe8b,
backend APIs 7 and 8).

The link is `<site>/s/<slug>`, a page with no sign-in (`web/src/share.tsx`),
read-only and live, served by `share:view` (`convex/share.ts`), the only
function that takes no phone token. It gives the title, when the thread
started, the model, the messages (with origin, summary and turn, so the
page folds each turn as the apps do), the tool lines and the thread's
pictures, and nothing else; or `gone` for a link that isn't shared.

Whoever opens a shared link usually isn't using Threadbear, so the page
reads as a short write-up rather than a chat: a big title, a byline
(model, date, turns, time worked), then each turn as the ask set as a
quote, its work folded to one "Worked for" line, and the answer, with the
pictures from the work shown large after it. On a wide screen an outline
of the asks sits on the left and marks the one you're reading; on a
narrow one it's the "3 turns" menu in the top bar. Copy link is top
right, and "Made with Threadbear" links to the landing page.

**Link previews.** Slack, X and Messages read a page's meta tags without
running its script, so `/s/<slug>` is served by `convex/http.ts`: the
site's `index.html` with the thread's title, the start of its last reply
(`share:preview`, Markdown flattened to sentences) and its first shared
picture as Open Graph and Twitter card tags, `no-store` so a link that
stops being shared stops unfurling. Mike's Share panel shows the same
preview. With Mike's token it also answers while the
thread isn't shared, with `manage`, so his page keeps its controls.

Pictures are in file storage (`shareImages`), uploaded by the runner only
while the thread is shared and dropped when it stops, and only real
pictures from the thread's media folder (attachments, tool pictures, and
what the agent copies into `THREADBEAR_MEDIA_DIR`), never its working
folder or a temp folder (docs/protocol.md, "Sharing"). A picture written in
a message's Markdown (`src`) takes the place of that Markdown image; the
rest go after the row they came with. Mike's own thread page shows them
too (`phone:messages` returns them), with what he pasted into his own messages
(`src` `picture:N`, `web/src/picture-chips.ts`), which goes up for every thread as
it's sent, never for a shared page. A Markdown picture that wasn't
uploaded shows as its alt text, not a broken image.

Sharing with specific people (an emailed code, through Resend) was built
on 28 Oct and taken out on 29 Oct: Mike wanted a public link and nothing
else for now, and it needed a Resend key Threadbear doesn't have.

## Signing in

The browser holds a phone token (`convex/tokens.ts`) in localStorage. The
Mac app issues one through the runner (docs/protocol.md "Devices"):
Settings › Phone & browser › "Open signed in" opens the site in the default
browser with the token in the link's `#fragment`, and the same tab lists
the signed-in browsers with Revoke. Since 28 Oct that's the usual way.

From a terminal, by someone logged in to the Convex team:

    npm run web:sign-in                  # Chrome on this Mac, prod, 90 days
    npm run web:sign-in -- --qr          # a QR code for another device
    npm run web:sign-in -- --dev         # the dev deployment's site
    npm run web:sign-in -- --local       # Vite on localhost:5181, dev data

`web/scripts/sign-in.swift` puts the token in the link's `#fragment`, which
browsers never send to a server; the page keeps it and wipes it from the
address bar. The token is never printed. Revoke a browser from Settings ›
Phone & browser, or with `npx convex run tokens:revoke '{"name":"web-mac"}'
--prod`; it then shows the sign-in page. The page is `noindex` and sends no referrer.

## Working on it

    VITE_CONVEX_URL=https://earnest-puffin-640.ap-southeast-2.convex.cloud npm run web:dev
    npm run web:sign-in -- --local

The dev deployment has no threads unless a dev runner mirrors to it, so for
a real look use prod data with a throwaway token (`--name web-test --days 1`,
revoke after).

**Deploying:** `npm run web:deploy` runs `convex deploy` (prod) and then
uploads `web/dist`. It asks before pushing to prod; from an agent, answer
through a pseudo-terminal (`(sleep 40; printf 'y\r') | script -q /dev/null
npm run web:deploy`). For a site-only change,
`npx static-hosting upload --build --build-command 'npm run web:build' --dist web/dist --prod`.
A plain `npm run web:build` refuses to run without `VITE_CONVEX_URL`, which
the static-hosting CLI sets: a build without it can't reach Convex.
In a thread's worktree there's no `.env.local` (it isn't in git), and the
upload says it "Could not reach component staticHosting": copy the main
checkout's in for the upload and delete it after.

## Not done

- A custom domain (e.g. threadbear.mikecann.app) instead of the
  `.convex.site` host.
- Signing in from a browser without the Mac to hand (e.g. approve a new
  browser from the iPhone app). A QR code for another device's browser is
  still `npm run web:sign-in -- --qr`.
- Sharing from the iPhone app or an agent's tools.
