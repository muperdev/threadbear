# Porting the desktop app to GPUI

Status: approved by Mike on 9 Nov 2026. Stage 0 is done and stage 1 (the daily
driver) is nearly there; progress is in [PARITY.md](gpui-port/PARITY.md).

The goal is one Rust desktop app, drawn with [GPUI](https://gpui.rs) and
[gpui-kit](https://gpui-kit.com), for Mac, Windows and Linux. It replaces the
Swift Mac app in `app/` once it does everything that app does. The iPhone app
stays Swift. The runner and its protocol don't change for this, apart from
the Windows work in [Windows runner](#windows-runner).

The working app is in `desktop/` (`client/` for the runner connection,
`gpui/` for the app). It signs in, shows the inbox in the website's order,
streams a thread live, answers cards and approvals, sends and stops.

Mike's four choices (9 Nov), each explained further down:

- [The web view](#choice-1-the-web-view): CEF, downloaded on first use.
- [The spec format](#choice-2-the-spec-format): Gherkin `.feature` files, set
  up the way Let's Build It does.
- [Where Windows and Linux run](#choice-3-where-windows-and-linux-run):
  GitHub Actions first, then a Hetzner Windows box when the runner work starts.
- [The order of work](#choice-4-order): the daily driver first.

The detail lives beside this file:

- [features.md](gpui-port/features.md): every feature, with how GPUI would do
  it and a size. This is the list parity tracking is built from.
- [mac-only-parts.md](gpui-port/mac-only-parts.md): the Mac-only subsystems,
  with the behaviour each replacement has to keep.
- [runner-windows.md](gpui-port/runner-windows.md): every Unix assumption in
  the runner and what replaces it.
- [ghostex-and-gpui.md](gpui-port/ghostex-and-gpui.md): what Ghostex did, what
  gpui-kit and GPUI actually have, and the web view options.

## How big it is

The Mac app is about 79,500 lines of Swift, more than the 61k first guessed:
22,300 in `ThreadbearCore` (no UI), 54,300 in the app and 2,900 in BearDock.
About 4,800 of the app is test tooling. `features.md` lists 307 of
the app's features and 18 planned ones.

GPUI turns out to cover more than we thought. gpui-kit has a virtualised list
that follows the tail (`MessageScroller`), selectable markdown (`TextView`),
text inputs and a code editor, tables, trees, tabs, popovers, menus, tooltips,
toasts, a dock and a test harness with headless windows. GPUI itself now has
accessibility (AccessKit) on all three systems, notifications, file drops, IME
and URL schemes.

What it doesn't have, on any system: a menu bar or tray icon, a Dock badge, a
Quick Look, a video player. Files dragged out of the app work on Mac and
Wayland only. The web view crate is experimental and doesn't do Wayland.

## What it takes, area by area

Listed most used first. The route says
where the work is: **kit** means gpui-kit or GPUI has it, **build** means we
draw it ourselves with no OS code, **web** needs the embedded web engine,
**shim** needs code for each system.

| Area | Route | How | Size |
|---|---|---|---|
| Transcript | kit + build | `MessageScroller` with one element per block, not one attributed string per reply. Streaming re-renders only the open block, as now. This is the biggest rewrite. | XL |
| Composer | kit + build | `Textarea` with inline picture chips. Pasting has to be right first, as most of what goes in is dictated and pasted. | L |
| Sidebar and inbox | build | Sections, hover buttons, row menus, hibernate presets. The rules are ported from `ThreadbearCore` with shared fixtures. | L |
| Cards (done, needs you, approvals, questions) | build | Same text engine as the transcript, with real fields placed inside the flow. | L |
| The window and its panels (BearDock) | build + shim | [Its own crate](#beardock). | L |
| Highlights | kit + build | Native list, folds, Notion board. | M |
| Options review window | build + shim | Pan and zoom, video and HTML shots use the shared players. | L |
| File previews and Quick Look | build + web | [In-app viewer](#quick-look). | L |
| Web panels, patches panels, Convex | web | [Web view choice](#choice-1-the-web-view). | L |
| Rich blocks (mermaid, HTML) | web | Sealed page in the web engine. Falls back to showing the source until it's installed. | M |
| Terminal | kit + build | `alacritty_terminal` drawn in GPUI, with ConPTY on Windows. | L |
| Folder browser, text editor | kit | `Tree`, `DataTable`, gpui-kit's editor. | L |
| Projects, schedules, usage, runners, connectors, updates | kit + build | Pages built from the same parts. | L |
| Settings | kit + build | Plain forms, stored in a JSON file instead of UserDefaults. | M |
| Bears, flights, juice | build | [Animations](#animations). | L |
| First-run tour | build | [Tour](#first-run-tour). | L |

Icons are one job that touches everything: 116 or so SF Symbols can't ship
outside Apple's systems, so we pick a bundled set (gpui-kit ships Lucide) and
map each one. Size M.

## The Mac-only parts, in plain words

### BearDock

BearDock is what lets you drag Threadbear's panels around: the sidebar, the
thread, Highlights and anything else can sit side by side, stack as tabs or
be torn out into their own window, like VS Code. The window is built on it,
so **it's a piece of work of its own**, a crate at `desktop/dock`.

gpui-kit has a dock, but it has fixed left, right, bottom and centre areas
and no torn-out windows, and its feel is different. We'd port BearDock's own
pure tree (`DockLayout`, with its 31 tests) to Rust line for line, and write
the interaction on GPUI. In order: the tree and its tests, splits and tabs and
dividers, drag with the ghost and the drop targets (tab slot, side split,
compass, window edge), hidden titles, then torn-out windows. The full list of
behaviours to keep is in `mac-only-parts.md`.

Size L, about 10 slices. Torn-out windows across a Wayland desktop are the one
piece likely to be poor, as apps can't read the pointer outside their windows
there. Those fall back to a menu command.

### DragSpace

A workaround for a macOS 27.2 bug: after a window is dragged between displays,
every menu in the app could turn invisible until a relaunch. DragSpace quietly
fixes the window using private Apple calls.

The bug hurts system menus. GPUI draws menus inside the window, so **we don't
port it**. If we ever use native menus on the Mac it comes back as a small
Mac-only module. It doesn't exist on Windows or Linux.

### Quick Look

The previewer the Mac spacebar opens. Threadbear uses it for pictures in the
chat, PDFs and documents in the side panel, and file thumbnails. Windows and
Linux have nothing like it, so we build a small viewer: pictures and GIFs in
GPUI, PDFs through the web engine (Chromium draws them), video through a
player, and "open in the default app" for the rest. On the Mac we can keep
Quick Look itself for the odd file types through a small shim. Security rules
carry over: only real images, never a file whose default app runs code.

Video needs a choice of player: the web engine playing it in a `<video>` tag,
or an optional `mpv` download. The web engine avoids a second big extra, but
stock Chromium builds usually leave out H.264, which most screen recordings
use, so stage 0 checks that first. If it can't play them, it's `mpv`.

### Self-update and restart handover

Today the installed app notices main has changed, rebuilds itself in a private
clone at low priority, then swaps in the new build. When you aren't using it,
the new app opens hidden under the old window and takes over with no flicker.

The Mac version carries over: `cargo build` in place of `swift build`, the
same two renames, the same idle rule. Handover used macOS's notification
centre; it becomes a local socket, and the window ordering is a small Mac shim.
Windows can't replace a running `.exe`, so it renames the old one aside and
starts the new one, and Linux can overwrite. Windows and Linux get a plain
restart rather than a seamless handover at first. See
[Install](#install-updates-and-toolchain) for releases.

### Animations

Core Animation runs flights and bears in the Mac's render server, so they keep
moving while the app's main thread is busy. That was the whole fix for the
archive flight, since the same click opens the next thread. GPUI animates from
the main thread. So the rule becomes: nothing heavy on the main thread after a
click, thread loading is asynchronous, and the "busy main thread" test
AGENTS.md asks for (a temporary sleep after the click) runs on each flight. If that's not enough on the Mac,
the flight can go back to Core Animation through a shim.

Everything else (springs, glides, bears, spinners, honey pot) is GPUI's
`Animation` and our geometry. Bears port directly from `BearGeometry`.

### Notifications

Banners when a thread needs you: GPUI's `show_system_notification` works on
all three. The Dock badge needs a small shim per system (Dock tile on the Mac,
taskbar overlay on Windows, launcher count where a Linux desktop has one). The
ding is a sound crate. While the Swift app is still around, only one app may
post, or you'd get two of everything. The phone's own notifications come from
the runner and aren't affected, but it does decide "at the Mac" from a client
named exactly `threadbear-mac` (`runner/src/rpc.ts`), so that widens.

### HangWatch

A background watchdog that notices the app's interface freezing and records
what it was stuck on, because a force-quit leaves no evidence. The heartbeat
ports directly. The sample of the stuck thread is per system: `sample` on the
Mac, a minidump on Windows, `gdb` or `eu-stack` on Linux if installed, with
the frames reported to Sentry through its Rust SDK. Size M.

### The dev-app.sh tools

These let agents open, click and screenshot a throwaway copy of the app without
touching Mike's. They're how agents check UI work, and the specs below depend
on them, so they come first, not last. `desktop/tools/dev-app` keeps the same
verbs (`start`, `do`, `dump`, `shot`, `rebuild`, `stop`) and the debug app
takes the same JSON commands over a local socket. The mock runner and its
fixtures carry over as they are.

Screenshots use `render_to_image` on Mac and Linux, and a window grab on
Windows. On the Mac the window stays tucked behind Mike's work. On Linux it's
Xvfb with software Vulkan, as `desktop/scripts/shot.sh` already does.

### First-run tour

The dimmed window, the lit-up part and the card. A dark overlay with a hole
cut in it, targets registered by name so it finds them wherever the dock has
put them. The ten steps carry over as data, and `threads.samples` is on the
runner already. `tour-e2e.mjs` carries over if the commands keep their names.

## How it's put together

```
desktop/
  client/      the runner connection (exists)
  core/        protocol types, the transcript reducer, inbox rules. No UI.
  dock/        BearDock for GPUI
  platform/    the per-system shims, one small module per service
  web/         the web engine and its sealed previews
  gpui/        the app
  tools/       dev-app and the spec driver
```

`core` is a port of `ThreadbearCore`, which is the real source of the event
rules (`web/` works from Convex rows, so it's not the same thing). While the
Swift app and the phone exist, the same logic lives in Swift, TypeScript and
Rust. So the tricky rules (folding, inbox order, link parsing) get shared
fixture files: event streams with expected output that every side's tests
read, the way `desktop/test/auth-vector-*.txt` already works for sign-in.

gpui-kit stock, pinned to exact versions, no fork. Ghostex forked both gpui and
its component library and has had to re-merge them many times since. We take a
fork only when a shim can't do the job, and write down why.

### Borrowed from Ghostex (MIT, notice kept)

- The parity log format, tightened (below).
- The CEF embedding: a windowed child in a normal GPUI layout slot.
- The download-on-first-use store: signed manifest, size and SHA-256, Ed25519
  signature, then unpack.
- Real sign-in popup windows, and in-process DevTools with no open port, which
  is probably why Google's sign-in works there.
- Its list of Mac shims, as a checklist.

Things not to copy: a long run of source-only slices with no launch of the app
(its slices 214 to 268 built parsers and gates while the screen stayed
placeholder), no UI tests, and the forks.

## Parity and behaviour specs

A parity list on its own goes stale. So every line in `features.md` that a
person would notice gets a behaviour spec, and the list ticks itself from
whether the spec passes.

**Specs are written against the Swift app first.** It's the definition of
right. Each spec runs against the mock runner, through the app's scripted
commands, on both apps: the Swift one through `dev-app.sh`, the GPUI one
through its own debug socket. Then GPUI has to pass the same file.

**The log** (`docs/gpui-port/PARITY.md`) keeps Ghostex's shape, one entry a
slice: what changed, files, what's left, plus a `Verification:` line that has
to be a spec run or a screenshot from a running app. A slice with no runtime
check doesn't count. The table has one column per system, so Windows and Linux
gaps stay in view.

A spec that can't be automated (a real trackpad gesture, a Wayland drag) is
marked `manual` and listed, so what hasn't been machine-checked stays honest.

A small set is checked by people or by ComputerCub-type tools early on, because
they've gone wrong for Ghostex and can't be seen from a test: Voice Type's
paste landing in the composer, and agents being able to see inside the window
through accessibility.

### Choice 2: the spec format

Decided: **A, Gherkin**. The other two are kept here for the reasoning.

| | How it works | Good | Bad |
|---|---|---|---|
| **A. Gherkin `.feature` files** (my pick) | `Given a thread needs an answer / When I press Allow / Then ...`, run by one Node runner that talks to whichever app | Reads as plain English, it's what Poddle and Let's Build It use, and one file covers Swift, Mac, Windows and Linux | A step-definition layer to keep up |
| **B. JSON or YAML command scripts** | The existing `do`/`dump` commands saved as files with expected output | No new tooling, close to `tour-e2e.mjs` | Unreadable as a list of behaviour, so it won't serve as the parity list |
| **C. Rust `#[gpui_kit::test]` tests only** | Fast in-process tests, headless on every system | Very fast, simplest setup | Doesn't touch the Swift app, so nothing defines "same as today" |

Fast in-process tests still get used under A for components, just not as the
parity record.

It follows Let's Build It's conventions, which `features/README.md` spells out:
one `<name>.feature` to one `steps/<name>.steps.ts`, shared steps in
`_common.steps.ts`, a pairing check that fails on an orphan file, features as
an intent contract for designed behaviour and not a regression dragnet, and its
Rendering Rule: a scenario asserts on what's drawn, not that a shell mounted.
What differs is the driver. Let's Build It runs in a browser through
Playwright-BDD, and here it's Cucumber.js driving two native apps.

### Choice 3: where Windows and Linux run

Decided: **A from the first slice, B when the Windows runner work starts.**

Linux runs on `cloud`, which already has the packages and takes screenshots.
Windows is the question.

| | What | Cost | Good | Bad |
|---|---|---|---|---|
| **A. GitHub Actions** | Every push runs the specs on Windows, Mac and Linux, as `.github/workflows/desktop.yml` already builds them | Free for this public repo. $0.010 a minute if it ever goes private | No machine to keep up, and it covers x64 | Agents can't drive it live or see a real desktop, and it's slow to iterate |
| **B. A Windows Server box on Hetzner** | An x64 cloud server next to `cloud`, reached over ssh, for running the runner and driving the app live | Roughly 10 to 30 euro a month for the server (to be confirmed), plus a Windows licence of our own, or a 180-day Server evaluation | A real place to run threads and to debug on | No GPU, so software rendering; another machine to keep patched |
| **C. A Windows VM on Tinker Desk** | Windows 11 on Arm in UTM or Parallels | Free | Close at hand, real desktop | Arm only, and it takes Tinker Desk's memory while other tests are running |

The Windows runner work needs a real machine and real logins, which is why B
comes in then. Real screenshots of the Windows GPUI app can come from Actions'
desktop session.

## Order of work

Each slice is small, ends with a spec or a screenshot, and lands on its own.
Rough sizes, in slices. A slice with a real check is more like half an hour of
agent time than Ghostex's seven minutes.

| Stage | What | Slices |
|---|---|---|
| 0 | Split the crates, spec runner and the driver on both apps, first 20 specs. Three short risk spikes: 5,000-row transcript speed and selection, the web engine in a panel with a menu over it, accessibility and Voice Type | 15 |
| 1 | The daily driver: the transcript, composer, sidebar, cards and BearDock, the most used things first, until Mike can use it all day | 60 |
| 2 | Panels and content: web panels, patches, previews, rich blocks, options window, terminal, folders, editor | 45 |
| 3 | Pages: projects, schedules, usage, runners, connectors, updates, settings, tour | 35 |
| 4 | Feel: flights, bears, swipes | 15 |
| 5 | Self-update, releases, the cutover | 15 |
| W | Windows runner (below), alongside | 25 |

That's about 210 slices. Windows and Linux aren't a stage of their own: each
spec runs on all three from the day it's written, and a system's failing specs
become its slices.

### Choice 4: order

Decided: **A, the daily driver first.**

| | What comes first | Good | Bad |
|---|---|---|---|
| **A. Mike's daily driver first** (my pick) | Stage 0's spikes, then stage 1 | He uses it within weeks, and real use finds what specs can't | Polish lands late, and it's rough for a while |
| **B. All the risky parts first** | Web engine, torn-out windows, Windows runner, then features | No nasty surprise late on | Weeks with nothing you can actually use |
| **C. Windows and Linux first** | A thin app for all three, getting everyone onto it | Reaches non-Mac people soonest | Mike stays on Swift, so GPUI gets little real use |

## Rollout

1. **An experiment, on only for Mike** (AGENTS.md, "Experiments"). The app
   installs beside the Swift one, as `~/Applications/Threadbear GPUI.app` with
   its own bundle id, and connects to the same runner. Nobody else builds or
   gets it. `hello.experiments` tells it what's on.
2. **Mike's daily app on the Mac.** It uses the same data folder and runner.
   Settings in UserDefaults (dock layout, zoom, appearance) get imported once.
   Only one app posts notifications, and `threadbear-desktop` counts as being
   at the Mac.
3. **Freeze new features in the Swift app.** Bug fixes only. New features go
   in the GPUI app with a spec, so the list stops moving.
4. **Cutover** once every Mac row in the parity table passes and Mike's used it
   for two weeks without going back. One last Swift build moves other people
   across: installs Rust if needed, builds the GPUI app, and swaps itself.
   `app/` and `dock/` then go.
5. **Windows and Linux get it as releases** once their parity rows pass.

The website and the iPhone app aren't touched. Later, Ghostex's GPUI-in-the-
browser build might replace `web/`, but it's a separate decision and Ghostex
itself doesn't draw everything that way yet.

## Windows runner

Native, without WSL. The full list is in `runner-windows.md`. In short: almost
every Unix call is wrapped in a `try` that treats failure as "nothing there",
so the runner would half work rather than crash. Each has to be ported on
purpose.

| Assumption | What replaces it |
|---|---|
| Shelling out to bash, zsh, sh | Requires Git for Windows, which the runner needs for `git.exe` anyway. Git's `bash.exe` runs `check.sh` and the shell scripts, and our shell-trust code stays valid |
| `ps`, `lsof`, process groups, kill | A small helper exe: Job Objects, process table with start times, TCP owner table |
| Signals | SIGTERM kills outright on Windows, so a graceful stop goes over the existing IPC channel |
| launchd, systemd, Runner.app | A per-user Scheduled Task at logon, and a tiny launcher so no console flashes. A Windows Service would run as the wrong user |
| `~/Library` paths | `%LOCALAPPDATA%\Threadbear`, through one `platformPaths` module |
| Paths and `/` splitting | About 45 real sites go to `path` helpers, with case-insensitive containment, including the security checks |
| Symlinks (`current` release, `node_modules`) | A text file for `current`, a junction for `node_modules` |
| `sandbox-exec` | Only used for one diagnostic. The real sandbox mode is Docker and has no native Windows answer yet, so it's refused there for now |
| Claude's PowerShell tool | Turned off at first, as Threadbear's permission gates match on the tool name `Bash` |
| Patch trust, secrets guard | Fail closed until they're ported, and each port gets reviewed by another lab |

Order: foundations that can land on Mac and Linux straight away (paths, one
spawn wrapper, `.gitattributes`), then the helper exe, lifecycle, a first real
thread from source, install and releases, then security parity. Roughly 2 to 3
weeks of work to a usable runner from source, 6 to 8 to full parity.

Clients need one additive protocol change: runners report `platform`,
`pathStyle` and `homeDir`, and clients split paths on both separators.

## Install, updates and toolchain

**From source (Mac, as today).** rustup and Xcode's command line tools, no full
Xcode.
Checked on Tinker Desk on 9 Nov with a throwaway rustup and
`DEVELOPER_DIR=/Library/Developer/CommandLineTools`: the release build took 3
minutes 24 seconds from nothing, used 1.6 GB of `target` plus about 850 MB of
Rust and crates, and made a 26 MB binary that started and opened its window.
That Mac does have Xcode on it, so it isn't a machine that has never seen
Xcode, but the build didn't use it.
A first build takes about 10 minutes on the M5. A small debug rebuild was 11
s, and a release one should be a minute or two (not timed yet). The install script sets `CARGO_HTTP_MULTIPLEXING=false
CARGO_NET_RETRY=10`, as crates.io failed for an hour without them on a flaky
connection. The `target` folder grows to several GB, like Swift's `.build`.

**Published releases.** On Windows, building from source means Visual Studio's
C++ tools and Rust, which is a lot to ask, so those users get ready-built
releases. Linux likewise. GitHub Actions already builds all three.

| System | Package | Update |
|---|---|---|
| Mac | keeps building from source; a signed, notarised DMG can come later | the existing self-update |
| Windows | an installer, unsigned at first (SmartScreen asks once) | Velopack, as Ghostex does |
| Linux | AppImage and a tarball | AppImage update |

The web engine is 146 to 186 MB per system if we go with Choice 1's first
option, published the same way with a signature checked before it's unpacked.

## Choice 1: the web view

Decided: **A, CEF downloaded on first use.**

Threadbear shows web pages in patch panels, the Convex dashboard, sealed HTML
and mermaid blocks and the options window. Whichever we pick, GPUI's own
floating elements (menus, tooltips) can't draw over a page, so those are
either separate small windows, as Ghostex does, or hidden while a menu is open.

| | What happens | Good | Bad |
|---|---|---|---|
| **A. CEF, downloaded on first use** (my pick) | The app ships small. The first web content asks "Install web runtime (about 150 MB)?", with Not now. Until then, HTML shows its source and panels offer to open in the browser | Same Chromium on all three systems, so one set of sandbox rules to prove. Google sign-in worked in Ghostex on the Mac. Not everyone pays for it | One more thing to publish and sign, and Chromium security updates every few weeks |
| **B. CEF, bundled in every build** | Same engine, always there | Works with no prompt, simpler code path | Every download and update is about 150 MB bigger, for people who may never use it |
| **C. The system's web view** (`wry`) | WKWebView, WebView2, WebKitGTK | No download at all, OS keeps it patched | Three engines to test. gpui-webview doesn't work on Wayland. Google's rules on embedded sign-in make that risky. Our sealed-page rules would need proving three times |

On Linux, A and B both mean running through X11 or XWayland while a web panel
is in use, as Ghostex does, since CEF has no Wayland embedding yet. The app
would use native Wayland otherwise.

A variant of A is to use the system web view for simple previews and CEF only
for real browsing. I'd skip it: two engines means two sandboxes to prove.

## Risks

What stage 0 found (9 Nov):

- **The transcript's speed: fine once the panels around it were cached.** On
  Tinker Desk's M2, a release build loads and folds 12,500 rows in 94 ms. The
  first frame times (0.6 ms) came from scripted frames and missed what real
  scrolling costs. With real trackpad flings (`features/tools/scroll-perf.ts`,
  GPUI's own frame profiler) and 60 threads in the list, a frame took 7.4 ms
  in the middle and 22 to 28 ms for the slowest 1 in 100, so more than half
  would be late on Mike's 120 Hz screen (8.3 ms a frame). GPUI draws every
  view again on every frame unless it's cached, and the list's scroll marks
  every view above it as changed, so each frame was rebuilding the whole
  window, thread list included. Caching the Threads panel and the side pane,
  sorting threads by keys worked out once, and rebuilding the menu bar only
  when something changes took it to about 6 ms in the middle, 8 ms at 90% and
  10 ms at 99% (10 Nov). View > Show Frame Rate shows it live in the
  installed app. The rule that follows: a big panel is a cached view
  (`AnyView::cached`), and it observes whatever it draws from, as a cached
  view only draws again when it's notified. What's left each frame is the
  root, the dock's bars and the thread pane above the list; caching those
  needs the dock to hand out its bars as views. Scroll anchoring and
  streaming with no jumps still have to prove themselves as the real rows
  arrive.
- **Web pages inside GPUI: works on the Mac.** A spike put a real Chromium page
  in a normal layout slot, resizing with it. GPUI's own menus can't draw over a
  page, so those popups open in small borderless windows of their own, as
  Ghostex does (the spike is the `gpui-web-spike` branch). Not run yet: Windows,
  Linux, Google sign-in, PDFs, video.
- **Accessibility: a gap to close.** Voice Type's paste already works with GPUI
  apps (it treats them as "can't tell" and goes by whether the app read the
  clipboard, 7e47882). But a screen reader or computer-use tool sees almost
  nothing below the window: on Tinker Desk the tree was the window and a title
  at first, then a broken repeating list once AccessKit woke up. Agents don't
  need it, because they use the app's own automation socket, but people with
  screen readers do, so this gets looked into before cutover.

Still open:

- **Keeping up with GPUI.** gpui-kit is young and pre-1.0. Pin it, upgrade on
  purpose, and let the specs catch what breaks.
- **Two apps for a while.** Hence the freeze as soon as Mike moves over.
- **Sealed HTML and patch trust** are security work and have to be proved again
  on the new engine and on Windows. Each gets another lab trying to break it.
