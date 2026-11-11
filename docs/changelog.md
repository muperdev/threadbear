# The changelog (threadbear.mikecann.app/changelog)

What's new in each daily release, for people who use Threadbear rather than people reading its
commits. Mike picked the layout on 10 Nov 2026 after seeing Wayne Sutton's bearfooding changelog
(tame-weasel-262.convex.site): a thread down the page with a sewn button at each release, the
release's biggest changes as cards with a before/after or a short clip, and everything else as
plain Added / Changed / Fixed lines.

This is Mike's copy's page. In a fork there's nothing to do here.

## Where things are

- `site/changelog/<YYYY-MM-DD>.json`: one file per release tag (`release/<YYYY-MM-DD>`). The
  format is at the top of `site/scripts/changelog.mjs`, which builds the page and refuses a bad
  file. A release with nothing a user would notice gets no file.
- Pictures and clips: not in the repo, so a fork doesn't download them. They're made in
  `site/.media/changelog/<release>/` (ignored by git; `npm run serve` serves it at `/media/`),
  then `npm run media -- upload <release>` puts them in the R2 bucket `threadbear-site-media`,
  which the site's Worker serves at `/media/changelog/<release>/<name>`. A name never changes
  once it's up (they're cached for good): a new picture gets a new name.
- `npm run media -- check` says whether every file the JSON names is up on the live site.

## Writing a release's entry

The daily changelog thread does this once a new `release/*` tag exists, for each release tag
that has no file yet (`git tag -l 'release/*'` against `ls site/changelog`).

1. Read the release: `git log --no-merges --format='%h %s%n%b---' release/<previous>..release/<this>`,
   and the tag's own plain notes (`git cat-file -p release/<this>`, its "What's new:" lines, a
   good start but check them against the commits).
2. Write `site/changelog/<this>.json`:
   - `highlights`: the 1 to 3 changes a user would most want to see, each with a short title and
     one or two sentences on what you can now do and where (menu path, setting, shortcut).
     `credit` names someone when a commit says the work came from their fork or PR.
   - `added` / `changed` / `fixed`: every other change a user would notice, one plain line each,
     several commits about one thing as one line. Leave out tests, refactors, docs, CI, agent
     instructions, review rounds and security-scan internals; security fixes users should know
     about are one plain `fixed` line.
   - Plain words for someone who's never seen the code: no file or function names, no em or en
     dashes (the build refuses them), no marketing words, Australian spelling. Check it against
     `~/.claude/skills/mike-voice/references/ai-tells.md`.
3. Pictures for the highlights (below), then their `media` fields.
4. `cd site && npm run build`, look at `/changelog` with `npm run serve`, then
   `npm run media -- upload <this>`, land the commit, `npm run deploy` from the main checkout
   and `npm run media -- check`.

## Making the pictures

Use the debug app on the mock runner (`app/Tools/dev-app.sh`; the commands are in
app/README.md, "Seeing and driving it from a script"). Its window stays behind Mike's work and
nothing moves the real pointer, so it's fine on Mike's Mac, one build at a time with
`taskpolicy -b`. Always `--appearance dark --size 1240x820`, and stop every instance when done.

- For the release's own build: borrow a worktree (`./borrow-worktree.sh`, it has build caches),
  `git checkout --detach release/<this>` in it, then `app/Tools/dev-app.sh start --mock
  --appearance dark --size 1240x820 --id cl-after`. Give it back afterwards.
- **compare** (before/after) is for a change to something that already existed. Build
  `release/<previous>` the same way as `cl-before`, put both apps in the same state with the same
  commands, and shoot both. Pick threads you haven't touched in either app, so the only
  difference is the change; crop both to the same box.
- **image** is for something new that reads standing still. **clip** is for something that moves
  or happens in steps (a palette opening as you type, a card filling as you swipe): compile
  `site/scripts/record-window.swift` once, get the window's id with
  `dev-app.sh do '{"cmd":"window","sheet":false,"settings":false}'` (the third word), record it
  for a few seconds while sending the commands with short sleeps between, then crop and encode:
  `ffmpeg -ss <start> -i raw.mov -t <secs> -vf "crop=W:H:X:Y,fps=30,format=yuv420p" -c:v libx264
  -preset slow -crf 24 -threads 2 -movflags +faststart -an <name>.mp4`, and a WebP poster from a
  frame that shows the point.
- Shots are at 2x. Crop to the part that shows the change, with a little around it, as WebP
  (`ffmpeg -i shot.png -vf crop=W:H:X:Y -c:v libwebp -quality 88 <name>.webp`); the page caps
  pictures at 520 points high, so very tall crops come out small.
- Nothing private in a picture: the demo threads are made up, but some mock data (the account
  picker design review, for one) shows Mike's real email addresses. Crop those out or use
  another screen.
- Some things the mock can't show (the Browser panel, a Plan first card, a thread waiting for a
  connection). Leave that highlight without `media` rather than faking it.
- `alt` says what the picture shows, for someone who can't see it.
