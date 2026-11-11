# The Mac-only parts, audited for the GPUI port

Read from the code on 9 Nov 2026 for docs/gpui-port.md. Tags say what was read and what was inferred.

Nothing was edited, built or run; this is all from reading code. Tags: **[code]** I read it, **[comment]** only the code's own comments say so, **[inferred]** my reasoning. Short paths are under ``. GPUI facts come from the crates the spike already pulled, in `~/.cargo/registry/src/*/` (`gpui-pre-0.3.11`, `gpui-pre-macos-0.3.11`, `gpui-base-0.9.0`, `gpui-component-0.9.0`).

**Size:** `app/Sources` is 76,587 lines in 248 files. ThreadbearCore has 22,254 lines and no AppKit. The UI target has 54,333 lines, and 120 of its files import AppKit. Tests add 25,552 lines in 153 files.

## 1. BearDock (`dock/`)

**(a) What it is.** The window isn't a fixed layout: every area (sidebar, thread, Highlights, panels) is a panel you can drag anywhere, like VS Code. Panels can sit side by side, stack as tabs, or be torn out into their own window, and the arrangement is remembered. Without it you get one fixed layout.

**(b) How it works [code].** Library 2,436 lines:
- `DockLayout.swift` (458): the pure tree, with no UI.
- `DockController.swift` (578): drag state, hit testing, feel constants.
- `DockView.swift` (937): SwiftUI rendering and gestures.
- `DockWindows.swift` (242): torn-out windows.
- `DragOverlay.swift` (221): indicators and the ghost.
- A demo app (501 lines). 21 app files import it. App wiring is in `app/Sources/Threadbear/Views/WindowDock.swift` (416).

Public API:
- **Tree types:** `PanelID`, `DockNode` (`.group` / `.split`), `DockGroup{id,panels,selected}`, `DockSplit{id,axis,children,fractions}`, `DockDrop` (`edge(group,side)`, `tab`, `swap`, `window(side)`), `DockFloat{panel,frame,home,share}`.
- **`DockLayout`:** `root`, `floating`, `hiddenTitles`, `canDrop`, `canFloat`, `move`, `add`, `float`, `close`, `select`, `setFractions`, `setTitleHidden`, `showsHeader`, `summary`, `topLeadingGroupID`.
- **`DockController`:** `layout`, `onChange`, `dragChanged/Ended`, `cancelDrag`, `show`, `toggle`, `open`, `openInWindow`, `openInNewWindow`, `putBack`, `close`, `select`, `isShowing`, `setLayout`, `frame(of:)`, `panel(at:in:)`.
- **`DockContent`:** closures `title`, `symbol`, `image`, `badge`, `view`, `header`, `accessory`, `closable`, `available`.
- **`DockOptions`:** `headerHeight`, `titleSize`, `trafficLights`, `cornerMovesWindow`, `barsMoveWindow`, `tip`, `windowContent`, `prepareWindow`.
- **Also:** `DockView`, `DockPanelWindow`, `WindowGrip`, `.dockDragDisabled()`, `isDockGhost`.

Persistence is the `Codable` tree as JSON in UserDefaults key `dockLayout` (WindowDock.swift:47,69-80).

Tests: 31 functions in 373 lines, nearly all on the tree. Drag feel is only tested through `BEARDOCK_FREEZE` / `BEARDOCK_EVENTS` in the demo.

**GPUI comparison.** `gpui-base` has a real dock (8,638 lines) with tabs, splits, Left/Right/Bottom/Center docks, drag, `DropIndicator` and `dump`/`load` as serde. Grep finds **no floating or torn-out windows** in it. I did not exhaustively check for compass, swap, hold-to-lift or the shrinking ghost; I saw none. **[inferred]** Port BearDock's tree (it's pure logic with good tests) and write the interaction on GPUI, using gpui-base only for ideas.

**(c) Must preserve.**
- [ ] Arbitrary nesting. After every change: no empty groups, no one-child splits, same-axis splits flattened, fractions sum to 1.
- [ ] Same-axis edge drop joins as a sibling halving the target's space. Window-edge drop takes 25% or its old share, clamped to 10-90%.
- [ ] Panels with `available == false` keep their place but give up their room (per-thread patch panels).
- [ ] Removing the selected tab selects its neighbour. Last panel can't close or float. Middle-click closes on release.
- [ ] Drop targets, tried in this order:
  - Over another bar: ghost tab drawn in it, after the titles.
  - Outer 22 pt of window: whole side (only the outer 6 pt along the top row).
  - Nearest side within 28% of a group: half split.
  - Middle: compass (cross of five, Swap, Window) with caption, dimmed when it would do nothing.
  - Past the window edge: outline of the new window.
- [ ] Ghost is the real panel content scaled to about 240 pt wide (0.3 to 0.85), grab point under pointer, -1.2 degree tilt, dashed hole behind, see-through over the compass. Springs: lift .32/.72, settle .34/.86, track .16.
- [ ] Escape (whatever has focus) cancels and flies home. Drop elsewhere flies home.
- [ ] Press semantics: title or tab drags at 5 pt with no hold. Bar or background needs a 350 ms hold. Moving sooner on a bar moves the window, on a background does nothing. Double-click zooms. Panel content's own drags win.
- [ ] Floating windows: saved frame, off-screen frame recentres, hide while panel unavailable, red button closes (or puts back if unclosable), drag back shows targets with the window at 50% alpha, Put Back restores tab or side at old share.
- [ ] Hidden titles per panel (follow the panel), grab strip, right-click menu. Divider strip 14 pt, min 80 pt panels.
- [ ] `summary` string (used by dumps), and old saved layouts keep loading.
- [ ] Traffic lights sit in the top-left panel's bar.

## 2. DragSpace.swift (`app/Sources/Threadbear/DragSpace.swift`, 251 lines)

**(a) What it is.** A workaround for a macOS 27.2 bug where, after dragging a window between displays, every right-click or dropdown menu in the app became invisible until a relaunch. It quietly checks the app's windows and uses private Apple calls to take them off a hidden internal "drag Space". Without it, menus can silently stop appearing.

**(b) How it works.**
- [comment] The window server puts a dragged window on a hidden Space and on 27.2 forgets to take it off, if the drag also refiles it to the display it's on. Windows get misfiled when the app relaunches in the background while Mike is on another display, which is what update restarts do.
- [code] `dlsym` looks up `CGSMainConnectionID`, `CGSCopySpacesForWindows` (mask 0xFF), `CGSCopyManagedDisplaySpaces`, `CGSRemoveWindowsFromSpaces`, and `CGSAddWindowsToSpaces` (automation only).
- [code] Runs on: launch, window moved (0.5 s debounce), app active, display change, Space change, a 5 s timer, and before a menu opens (`MenuItems.swift:101`, `MenuWatch.swift:103`). Skips while the mouse is down.
- [code] Partner: `MenuWatch.swift` (141) replaces a menu not visible after 0.25 s with a popover. 9 unit tests on the set logic.

**Does it apply to a non-AppKit drag? Would GPUI have it?**
- The fault is in the window server, not AppKit [comment + inferred], so it isn't about the UI toolkit.
- A GPUI macOS window is an `NSWindow` subclass (`gpui-pre-macos/src/window.rs:104`), and `start_window_move` is literally `performWindowDragWithEvent:` (`:2269`). **So a GPUI window dragged that way goes down the same path and could get stranded.** [inferred; I couldn't test it.]
- The current app already uses that path only in `WindowGrip` (`DockView.swift:933`) and the About window. BearDock's bar drag and floating-window drag move with `setFrameOrigin`, and panel drags stay inside the window, so neither is a window-server drag [code; that they avoid the bug is inferred].
- The damage is to **system `NSMenu` windows**. GPUI/gpui-component menus are drawn inside the window, so they'd be immune [inferred]. Native ones would not be (`gpui-component/src/native_menu/macos.rs:110`). Menu bar menus and GPUI's separate pop-up windows: unknown.
- **Advice:** skip this in the port if all menus are in-window. Keep it as a small Mac-only module if native menus get used. It doesn't apply to Windows or Linux.

**(c) If kept:**
- [ ] Never touch windows mid-drag, except right before a menu.
- [ ] No-op if the symbols are missing.
- [ ] Don't refile windows on full-screen Spaces or straddling displays (75% rule).
- [ ] Fallback to a popover for dead menus.

## 3. Quick Look

**(a) What it is.** macOS's built-in file previewer, the one the spacebar opens in Finder. Threadbear uses it to pop up pictures from the chat and to show PDFs, Keynote files and so on in the side panel, and for file thumbnails. Without it, pictures don't open in place and most document types show nothing.

**(b) Four separate uses [code]:**
1. `QLPreviewPanel` floating panel: `QuickLookCoordinator` in `app/Sources/Threadbear/Transcript/Thumbnails.swift:73-122`, with responder hooks in `TranscriptController.swift:19-21` and `FolderBrowser.swift:973,1002`.
2. Embedded `QLPreviewView`: `FilePreviewView`, `app/Sources/Threadbear/Views/HighlightsView.swift:857` onward. Pictures use `NSImageView` and video uses `AVPlayerView` deliberately, as Quick Look draws in another process and ate the presses that start a drag-out.
3. `QLThumbnailGenerator`: `LinkPeek.swift:145`, `FolderBrowser.swift:1274`.
4. `ThumbnailLoader` (ImageIO, not Quick Look): decodes at drawn size off the main thread, 4 concurrent, 24 MB cache.

GPUI has no Quick Look or equivalent (grep empty). **[inferred]** Windows and Linux have no comparable previewer, so plan a small in-app viewer for images, video and PDF, and "open in default app" for the rest.

**(c) Must preserve.**
- [ ] Click a tile: preview, arrow through only the local, real pictures in that message.
- [ ] Security (7 Nov review): only real decodable images, never a file whose default app runs code (`quickLooks`, `LinkPolicy.isCodeRunner`).
- [ ] A remote runner's pictures: fetch, "Loading…", "Couldn't get it from X", click retries, truncated means "On X".
- [ ] Drag out as the file (`FileDrag`, 4 pt threshold). Copy Image puts original bytes plus TIFF, never a path.
- [ ] Video letterbox fit, controls keep their presses, animated GIFs, refresh on reopen, pause on close.
- [ ] Clicks count when the app isn't in front.

## 4. Self-update and restart handover

**(a) What it is.** The installed app rebuilds itself from your git checkout when `main` changes, then swaps to the new version, usually with no visible flicker. Agents can ask to be told when that's happened. Without it, every app change needs a manual rebuild and relaunch, and agents can't verify their own work in the live app.

**(b) How it works [code].** Key files: `app/Sources/Threadbear/AppUpdate.swift` (576), `app/Sources/Threadbear/Handover.swift` (331), `app/stage-update.sh` (82), `runner/src/app-restarts.ts` (130).

*Stage.*
- `AppUpdater` compares `git rev-parse main:app main:dock` to the bundle's `build.json`.
- Triggers: 20 s after launch, every 5 min, or a directory watch on the ref (3 s debounce).
- A new tree runs `taskpolicy -b stage-update.sh`: own clone and build folder in `~/Library/Caches/Threadbear/update`, output to `~/Applications/.Threadbear.app.next`.
- This is compile-from-source on the user's machine, with **no downloads and no Sparkle**. A failed tree isn't retried until main moves. `followMain: false` stops it.

*When to restart (checked every 1 s, `shouldAutoRestart`).*
- Requires: staged, threads loaded, connected, nothing typed (drafts, schedule editor open, card notes).
- Then: goes at once if the app isn't in front; if it is in front, needs 5 s with no input *inside the app* (an `NSEvent` local monitor, not system idle) and the main window as key window.

*Swap and handover.*
- Two renames: installed to `.old`, `.next` to installed. `applyAtQuit` covers plain quits, `applyStagedAtLaunch` covers kills.
- Handover: the old app starts a second copy with `--handover-from/-below/-frame`. The new one stays alpha 0, click-through, same frame.
- It loads (15 s cap) and settles .25 s, orders itself just under the old window and sends "ready". The old sends "go" with active state, mode, selection and draft words. New replies "done". Old hides and quits.
- Transport is `DistributedNotificationCenter`, object is the old pid. Also uses `NSApp.yieldActivation`, and watches the old pid's exit. Timeouts: ready 20 s, done 1 s.
- Fallback: a shell loop waits for the pid to exit, then `open [-g]`. Only the main window can hand over (not minimised, full-screen, or hidden).

*Runner side.*
- `wait_for_app_restart` stores watchers in `app-restart.json`. On a `hello` whose `build.tree` differs, each watcher gets a message. Requests expire after a day.
- `runner/src/update.ts` (724) and `launch.mjs` (448) concern **only the runner**. To the app a runner handover is a reconnect, as clients only talk to the primary.

**Trip wires:**
- `runner/src/rpc.ts:251,561` only count a client named exactly `threadbear-mac`, so a new app must use that name or these must widen (also affects presence, section 6).
- `RunnerLauncher` (`Support.swift`) starts the runner with `launchctl`.
- [inferred] Windows can't overwrite a running `.exe`; building from source there needs a toolchain. GPUI's `restart()` is the plain wait-then-reopen only.

**(c) Must preserve.**
- [ ] Only the real installed copy self-updates (not dev, perf, or scripted runs).
- [ ] Builds never see a dirty tree or disturb agents. Low priority. One at a time. Stale-lock cleanup.
- [ ] Never lose typed text. Running threads don't block restart.
- [ ] Reopens same thread, list and scroll anchor. Window frame unchanged.
- [ ] Never half-installed. Works after a crash.
- [ ] Incoming app silent (no notifications, updater, automation) until takeover.
- [ ] Sends build info in `hello`.
- [ ] Signed with a stable identity so permissions persist (`build-app.sh:96-110`).

## 5. Animations of about 100 ms or more

**Important trait [AGENTS.md + code]:** the longer ones run in macOS's render server, so they keep moving while the main thread is busy. That was the whole fix for the archive flight, as the same click opens the next thread. GPUI's `Animation`/`with_spring` re-render frames from the foreground thread (`elements/animation.rs`) [read; "no render-server equivalent" is inferred]. So a port needs a main thread that stays free, or a different flight design.

Transcript rows deliberately do not animate (`withAnimation: []`, `NSAnimationContext` duration 0).

**Core Animation (render server)** (`app/Sources/Threadbear/`):

| What | File | Timing |
|---|---|---|
| Archive flight (picture of the thread drops into Archived box; overlay window; 32 keyframes of position, transform, radius, opacity) | Views/ArchiveFlight.swift, ThreadbearCore/Threads/ArchiveFlightPath.swift | .44 s; next thread fades in after .2 for .28 |
| Hibernate flight (into moon, dimming layer) | same | .52 s; next thread .26 then .36, rises 8 pt |
| Flight fallback (Reduce Motion, or no target) | same | .18 fade |
| Sent message lifts | Views/Juice.swift `SendLift` | .02 + .17 |
| Card settles into composer | Views/ComposerCard.swift:289 | .2 |
| Card page slide, counter roll | Views/ComposerCard.swift `CardDeck` | .18 |
| Done tick draws on | Transcript/Cells.swift:744 | `tickDraw` |
| "Copied" bubble | Transcript/CopiedBubble.swift | 1.1 s total |
| Row glow from summary bullet | Transcript/TranscriptController.swift:641 | holds 2.5, fades .8 |
| Spinner arc | Views/Spinner.swift | .9 s, forever |
| Waving bear (pop, wave bursts, breath, blink, pom-pom; hover waves back, click hops) | Views/WavingBearView.swift, BearGeometry.swift | loops 6 / 3.4 / 7.3 s; hop .42 |
| Sleeping bear, drifting Zs | Views/InkLayers.swift | breath loop, 3 s drift |

**SwiftUI** (would also need redoing):

| What | File | Timing |
|---|---|---|
| Lift / settle / track / landing fade | dock/DockController.swift | springs .32, .34; .16; .12 |
| Title hide, grab strip, compass hover | dock/ | .22, .15, .14 |
| Shared tokens: rows, selection, hover, pop, swap | Views/Juice.swift | .32, .2, .12, spring .3, .25 |
| Mode switch, fold/show more, reorder, scroll | Views/SidebarView.swift | .22, .2, .2, .25 |
| Loading skeleton pulse | SidebarView:181 | .9 s forever |
| Row landed tint, press, slide-off | SidebarView:849,839,883 | 1.1 (+.3), .14, .16 or spring .3 |
| Thread row glide between sections (matched geometry) | SidebarView (`.matched`) | uses rows token |
| Archive box bounce, moon bounce and lit, count roll, draw-on icon | SidebarView:440-474,756 | system effects |
| Row swipe | Views/RowSwipe.swift | spring .3, .16 |
| Reorder rows | Views/ReorderList.swift | spring .26 |
| Page swipe arrow | SwipeNavigation.swift | .15 |
| Banners, inbox-zero dismiss, bear wave | Views/DetailView.swift | .3, .15, keyframes |
| Home line, wobble | Views/HomePage.swift | .18, .3, keyframes |
| Inbox-zero bear, pane bear | Views/Juice.swift | .4/.35 + wave keyframes |
| Honey pot fill, level, drip | Views/HoneyPot.swift | .7, .26, keyframes |
| Icon tap scale, badge, selection | Views/ActivityBar.swift | bouncy .35 |
| Send arrow nudge, composer attention | ComposerControls:1021, ComposerView:177 | keyframes, .2 |
| Tour hole move, glow, finish | Views/Tour.swift, WindowDock:295 | .3, 1.1 forever, .2 |
| About window open | Views/DenWindow.swift | .22 + wave |
| Gauges, hovers, folds, scrolls | RunnersPanel, ProjectsHome, HighlightsView, HighlightsBoard, ScheduledList, ReviewWindow, ExplainPane, SetupChecklist | .15 to .3 |

**(c) Must preserve.**
- [ ] Reduce Motion shortens or removes everything (`Juice.reduceMotion`).
- [ ] Nothing delays a click.
- [ ] Bears are geometry-driven (`BearGeometry`), with Santa hat 1-26 Dec and bedtime logic.
- [ ] Flights fall back if the target isn't on screen. Hibernate aims where the heading will appear.

## 6. Notifications

**(a) What it is.** System banners when a thread needs you, a ding when one finishes, and a red number on the Dock icon. There's no menu bar icon. Without these you must watch the app.

**(b) How it works [code].**
- Banner: `Notifier` in `app/Sources/Threadbear/Support.swift` (`UNUserNotificationCenter`). Fires when `becameNeedy && !showing && notificationsEnabled && !thread.isRemote` (`AppModel+Connection.swift:278`). Grouped by thread, and the text is the first open ask, "Waiting for you to answer.", the preview, or "Finished.".
- **No action buttons, and no `content.sound`**, so banners are silent; `willPresent` shows them even in front.
- Clicking: `AppDelegate` activates the app and opens the thread, deferring if threads aren't loaded yet.
- Ding: `DingPlayer.swift`, `NSSound` from a generated WAV, chosen in Settings, one per 1.5 s.
- Badge: `NSApp.dockTile.badgeLabel` is the needs-you count (`AppModel+Chrome.swift:148`).
- `NSStatusItem` and `MenuBarExtra`: **0 uses**.
- Phone push doesn't come from the app. Runner to Convex to APNs/relay. The app's only role is presence: `runner/src/presence.ts` says he's "at the Mac" when `rpcServer.appConnected()` (client name check, section 4) and HID idle under 2 min, from `ioreg`, **returning undefined off macOS**.

GPUI: `show_system_notification` (tag, title, body, buttons) and a response callback, on all three platforms. No Dock badge API (grep empty). No sound API seen.

**(c) Must preserve.**
- [ ] Not for the visible thread or remote runners' threads. Scripted runs silent.
- [ ] Click opens the thread, including at cold start.
- [ ] Badge count. Ding throttling and volume.
- [ ] Presence needs a non-Mac idle source, or phones will behave as "always away" there [inferred].

## 7. HangWatch (`app/Sources/Threadbear/HangWatch.swift`, 238, plus `ThreadbearCore/HangSample.swift`, 79)

**(a) What it is.** A background watchdog that notices the app's UI freezing and captures what it was stuck on, because a force-quit otherwise leaves no evidence. Without it, freezes are guesswork. The user sees nothing either way.

**(b) How it works [code].** A background queue posts a heartbeat to the main thread each second. No beat for 4 s means it runs `/usr/bin/sandbox-exec … /usr/bin/sample <pid> 3`, writing `~/Library/Logs/Threadbear/hangs/hang-<time>.{json,sample}`. The sandbox profile denies paths like Desktop, Documents and volumes (`HangSample.sandboxProfile`), which I read as avoiding permission prompts [inferred]. It records recovery time plus context: main-thread priority via `thread_info`, load average, system idle. The next launch sends stalls of 15 s or more to Sentry as frames only. GPUI has a built-in `profiler/hang.rs`, but it's journal-based (which task overran), not a stack sample.

**(c) Must preserve.**
- [ ] Works however stuck the main thread is; the watchdog thread takes no locks the main thread could hold.
- [ ] Clock pauses while asleep (uptime).
- [ ] Installed app only. Never user text in reports. Reports only frames, duration, build and context.
- [ ] Ignores idle-looking samples. 30 s settle time, keep 10 files.
- [ ] [inferred] Needs another sampler on Windows and Linux; `sample` is Mac-only.

## 8. Scripting and test tools

**(a) What it is.** Hidden debug controls that let agents open, click and screenshot a throwaway copy of the app without disturbing you, plus a fake backend and performance and screenshot runs. Users never see them. Without them, agents can't verify UI work, and this repo's rules depend on that.

**(b) How it works [code].**
- `app/Tools/dev-app.sh` (827): `start`, `do`, `shot`, `menu`, `dump`, `rebuild`, `log`, `stop`, `status`. Instances are per thread and checkout, each with its own bundle id, data folder, channel and pasteboard. Quits after 60 min idle.
- Inside the app: `Automation.swift` + `+Commands` + `+Waiting` (2,710 lines, DEBUG only), about 140 case labels. They cover navigation (`open`, `expand`, `scroll`, `back`), input (`type`, `paste`, `send`, `drop`, `click`, `point`, `key`), panels and dock (`dock-move`, `panel`, `float`), app-level (`tour`, `setup`, `handover`, `updates`, `waitFor`, `dump`, `screenshot`, `drag-space`, `stall`, `mock`).
- Transport: JSON in a `DistributedNotificationCenter` notification, one result line in a log. Refuses the live runner's data. Window tucked behind everything (`TuckBehind.swift`), not in Dock, own pasteboard. `shot` is `screencapture -l` by window id.
- `Tools/mock-runner/server.mjs` (3,241, plain Node + `ws`), `make-synthetic.mjs` (646), 18 fixtures.
- `Tools/screenshots.sh` with `Screenshots.swift` (659), `Tools/perf-run.sh` with `Perf.swift` (814), `Tools/tour-e2e.mjs` (224), and `realdrag.swift` / `post-gesture.swift` for real input on Tinker Desk.

**Carries over:** the mock runner (the spike's tests already use it), fixtures, and `tour-e2e.mjs` if the same command names are kept. **Needs replacing:** transport, bundle/plist tricks, window tucking, `screencapture`, and all of the in-app command code.

**(c) Must preserve.**
- [ ] No focus stealing, pointer movement, real clipboard or live-data access.
- [ ] Debug builds only. Several instances at once. Ownership rules.
- [ ] `dump` exposing rows, composer, send target and banner.
- [ ] Offscreen-safe screenshots of the app window only.

## 9. First-run tour (`app/Sources/Threadbear/Views/Tour.swift`, 724)

**(a) What it is.** The guided walkthrough on first launch: the window dims, one part lights up with a card beside it, and it finishes with sending your first thread. Without it, a new user lands in a dense window with no guidance.

**(b) How it works [code].** 10 steps and 7 one-card intros for particular pages (schedules, projects, librarian, usage, updates, connectors, pools), all in plain data. The overlay is a dark layer with a rounded hole (even-odd fill), glow, card placement trying each side, and an arrow. UI parts are tagged with `.tourTarget` (14 sites), so it finds them wherever the dock put them, and skips steps whose target is closed. Sample threads come from the runner via `threads.samples` (`runner/src/samples.ts`, 76 lines, so already portable), in `~/ThreadbearThreads/Welcome`, and are removed at the end. The only hands-on task now is `sendFirst`; the doc comment still mentions moving panels and pressing chips, but that's stale.

**(c) Must preserve.**
- [ ] Runs once (`tourSeen`), no Skip on first run. Help > Take the Tour is skippable.
- [ ] Samples removed however it ends, and excluded from milestone counts.
- [ ] Clicks blocked except in the lit part of a hands-on step.
- [ ] One short sentence per card (`TourTests` enforces).
- [ ] Each card links to its docs page.
- [ ] Never starts in scripted runs. `tour-e2e.mjs` before landing.

## 10. Everything else Mac-only (grep over `app/Sources/Threadbear`; hits / files)

**Absent (0 hits):** Sparkle, `NSStatusItem`, global hotkeys, `NSServices`, `NSFilePromise`, file coordination, IOKit, login items, AppleScript, Keychain (1 comment only, `Usage.swift:176`), `NSDocument`, `NSSplitView`. No entitlements file or usage-description keys, and no notarisation seen. A TCC-style regex had 1 hit in `ComposerView.swift`; it isn't a capture API, likely the word "Photos" (not confirmed).

| Family | Count | Files or note | Port impact |
|---|---|---|---|
| **Transcript engine** (`NSTableView`, TextKit, `NSAttributedString`) | 11/3, 49/11, 338/21 | `TranscriptController`, `Cells` (2,449), `TextKit`, `MarkdownRenderer` | Biggest rewrite. I didn't study its behaviours here. |
| `NSTextView` composer, IME | 24/9 | `ComposerView` (1,580) | Inline picture chips. Mike dictates, so test Voice Type against it [inferred] |
| **WKWebView** | 35/5 + 13/3 related | `PanelsView` (1,253), `RichRenderer`, `LocalPreviewSchemeHandler`, `PreviewSandbox`, `FirstClick` | GPUI has no web view seen; patch panels and html/mermaid blocks depend on one. The lockdown is security work that needs re-proving (`SECURITY.md`). |
| AVKit | 13/2 | `ReviewWindow`, `HighlightsView` | No video player seen in GPUI |
| SwiftTerm | 13/2 | `TerminalPanel` | Needs another terminal |
| Highlightr, Charts, CoreImage (QR) | 7/1, 3/1, 2/1 | `TextFileView`, `UsagePanel`, `DevicesSettings` | gpui-component has highlighter and chart folders; QR needs a crate |
| `NSMenu`, `NSPopover`, `NSPanel` | 121/14, 5/3, 4/4 | `MenuItems`, `Tips`, palette | In-window menus also avoid section 2 |
| Native dialogs | 13/7, 11/6 | `NSAlert`, open/save | GPUI has path prompts |
| `NSEvent` monitors, trackpad, side buttons | 12/11, 28/9 | `SwipeNavigation` (360), `Navigation` | GPUI has a gestures hook; unverified how far it goes |
| `acceptsFirstMouse` | 24/14 | clicks count when app is behind | Behaviour must match |
| Drag and drop | 15/6 | `FileDrag`, `ThreadDrag`, composer | GPUI `start_external_drag`: macOS yes, Windows no (default false), Linux unknown |
| SF Symbols | 302/58 | everywhere | Icon set needed |
| Tooltips, every-click rule | `Tips.swift` 348 | `.tip`, `TipsTests`, `EveryClickShowsTests` | Project rules to recreate |
| `NSPasteboard` | 51/11 | rich copy, private scripted pasteboard | GPUI clipboard is simpler |
| `NSWorkspace`, `OpenGate` | 33/17 | security gate on opening files and links (`OpenGate.swift`, 328) | Must be redone per OS |
| Settings storage | 69/22 + 10/5 | `UserDefaults`, `@AppStorage` | Needs config file |
| Materials, window chrome | 10/8 | Glass, traffic lights, `macOS 26` minimum | Windows and Linux differ |
| Accessibility | 16/6 | sparse today | Not checked in GPUI |
| Process, shell-outs | 9/6 | `launchctl`, `open`, `taskpolicy`, `sample`, `screencapture`, `git` | Mostly OS-specific |
| Sockets, IPC | 2/1, 6/2 | `BrowserBearPipe` Unix socket, distributed notifications | Named pipes on Windows [inferred] |
| Sentry Cocoa | 1 | `CrashReporting.swift` (69), `SentryBinary/`, dSYM upload | Rust SDK [inferred] |
| URL scheme | Info.plist + delegate | `threadbear://thread/<id>` | GPUI has `on_open_urls`, `register_url_scheme` |
| Menu bar, windows | `ThreadbearApp.swift` | `Go`, `Thread`, Settings, Help menus | `set_menus` exists |
| Private calls | | SkyLight; `_setPeerConnectionEnabled:` (`PreviewSandbox:36`); `_features`/`_setEnabled:forFeature:` (`PanelsView:915`); `preferredImageVisibility` (`MenuItems:28`) | Mac only |

TCC, meaning macOS privacy prompts, sits with the runner: `Threadbear Runner.app` exists so prompts name it, and the runner checks Full Disk Access (`docs/on-your-mac.md:264`).