# GPUI port: parity

What the GPUI app does compared with the Swift app, worked out from which
behaviour specs pass (`features/`). The table is written by
`node features/parity.mjs` from each machine's report. Don't edit it by hand.

A blank cell means no spec covers that yet, not that the feature is missing.
The full list of features is [features.md](features.md).

## The table

<!-- parity:start -->

114 of 329 features have a spec so far, across 202 scenarios.

Scenarios passing: Swift: 191 of 202, Mac: 193 of 202, Windows: 192 of 202, Linux: 193 of 202.


### 0. Foundations everything else needs

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `foundation.rpc` Runner RPC client | pass | pass | pass | pass |
| `foundation.zoom` Zoom | partly | pass | pass | pass |

### 1. Windows and panel dock

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `dock.layout` Layout tree | pass | pass | pass | pass |
| `dock.titlebar` No title bar | pending | pending | pending | pending |
| `dock.frame` Window frame memory | pending | pending | pending | pending |
| `dock.reset` Default layout and Reset | pass | pass | pass | pass |

### 2. Rails

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `rail.activity` Activity bar | pass | pass | pass | pass |
| `rail.dots` Rail dots | pass | pass | pass | pass |
| `rail.right` Right rail | pass | pass | pass | pass |

### 3. Sidebar and inbox

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `sidebar.sections` Sections | pass | pass | pass | pass |
| `sidebar.sort` Inbox sort | pass | pass | pass | pass |
| `sidebar.search` Thread search | pass | pass | pass | pass |
| `sidebar.newthread` New thread button | pass | pass | pass | pass |
| `sidebar.row` Thread row | pass | pass | pass | pass |
| `sidebar.row.marks` Row left marks | pass | pass | pass | pass |
| `sidebar.row.origin` Origin mark | pass | pass | pass | pass |
| `sidebar.row.status` Row right status | pass | pass | pass | pass |
| `sidebar.row.remote` Other-Mac icon | pass | pass | pass | pass |
| `sidebar.row.hoverbuttons` Hover buttons | pass | pass | pass | pass |
| `sidebar.row.menu` Row context menu | pass | pass | pass | pass |
| `sidebar.subagents` Sub-agent tree | pass | pass | pass | pass |
| `sidebar.archived.paging` Archived paging | pass | pass | pass | pass |
| `sidebar.skeleton` Loading skeleton | pass | pass | pass | pass |
| `sidebar.hibernate.presets` Hibernate presets | pass | pass | pass | pass |
| `sidebar.hibernate.pick` Pick a time | pending | pending | pending | pending |
| `sidebar.hibernate.wake` Wake when… | pass | pass | pass | pass |
| `sidebar.move` Move to another runner | pass | pass | pass | pass |

### 4. Thread header, banners, toasts

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `header.title` Title line | pass | pass | pass | pass |
| `header.backforward` Back and forward | pass | pass | pass | pass |
| `header.share` Share | pass | pass | pass | pass |
| `header.more` ... menu | pass | pass | pass | pass |
| `banner.connection` Connection banner | pass | pass | pass | pass |
| `banner.error` Error banner | pass | pass | pass | pass |
| `banner.limit` Limit banner | pass | pass | pending | pass |
| `toast.archived` Archived toast | pass | pass | pass | pass |
| `toast.inboxend` Inbox end toast | pass | pass | pass | pass |
| `thread.newpage` New-thread page | pass | pass | pass | pass |

### 5. Transcript row kinds

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `transcript.list` The transcript list | pass | pass | pass | pass |
| `transcript.user` Your bubble | pass | pass | pass | pass |
| `transcript.reply` Assistant reply | pass | pass | pass | pass |
| `transcript.thinking` Thinking line | pass | pass | pass | pass |
| `transcript.tool` Tool line | pass | pass | pass | pass |
| `transcript.tooloutput` Tool output styling | pass | pass | pass | pass |
| `transcript.diff` Diff row | pass | pass | pass | pass |
| `transcript.images` Images row | partly | pass | pass | pass |
| `transcript.approval` Approval card | pass | pass | pass | pass |
| `transcript.approval.stacked` Stacked approval | pass | pass | pass | pass |
| `transcript.approval.settled` Settled approval | pass | pass | pass | pass |
| `transcript.question` Question card | pass | pass | pass | pass |
| `transcript.secret` Secret card | pass | pass | pass | pass |
| `transcript.review` Review card | pass | pass | pass | pass |
| `transcript.turnfooter` Turn footer | pass | pass | pass | pass |
| `transcript.error` Error row | pass | pass | pass | pass |
| `transcript.workedfor` Worked-for group | pass | pass | pass | pass |
| `transcript.working` Working line | pass | pass | pass | pass |
| `transcript.changes` Changes card | pass | pass | pass | pass |
| `transcript.rowmenu` Row context menu | pass | pass | pass | pass |

### 7. Composer

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `composer.textbox` Text box | pass | pass | pass | pass |
| `composer.return` Return, Shift-Return | pass | pass | pass | pass |
| `composer.listcontinue` List continuation | pass | pass | pass | pass |
| `composer.picturechips` Picture chips | pass | pass | pass | pass |
| `composer.chiphover` Chip hover and × | pass | pass | pass | pass |
| `composer.filedrop` File path drop | pass | pass | pass | pass |
| `composer.dropanywhere` Drop anywhere on thread | pass | pass | pass | pass |
| `composer.runner` Runner chip | pass | pass | pass | pass |
| `composer.modelpicker` Model picker | pass | pass | pass | pass |
| `composer.permission` Permission chip | pass | pass | pass | pass |
| `composer.contextring` Context ring | pass | partly | partly | partly |
| `composer.send` Send and Stop | pass | pass | pass | pass |
| `composer.starting` Starting state | pass | pass | pass | pass |
| `composer.slash` Slash palette | pass | pass | pass | pass |
| `composer.slash.keys` Palette keys | pass | pass | pass | pass |

### 8. Cards in the composer

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `card.done` Done card | pass | pass | pass | pass |
| `composer.recap` Recap box | pass | pending | pending | pending |
| `card.needsyou` Needs you card | pass | pass | pass | pass |
| `card.collapse` Collapse | pass | pass | pass | pass |
| `card.somethingelse` Something else box | pass | pass | pass | pass |
| `card.returnflow` Return flow | pass | pass | pass | pass |
| `card.answers` Answer and follow-up state | pass | pass | pass | pass |

### 9. Highlights

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `highlights.panel` Panel and folds | pass | pass | pass | pass |
| `highlights.librarian` Librarian line | pass | pass | pass | pass |
| `highlights.needsyou` Needs you list | pass | pass | pass | pass |
| `highlights.projects` Project blocks | pass | pass | pass | pass |
| `highlights.scheduled` Scheduled list | pass | pass | pass | pass |
| `highlights.subagents` Sub-agents list | pass | pass | pass | pass |
| `highlights.started` Threads it started | pass | pass | pass | pass |
| `highlights.pinned` Pinned, Files, Links | pass | pass | pass | pass |
| `highlights.changes` Changes | pass | pass | pass | pass |
| `highlights.rowmenus` Row menus | pass | pass | pass | pass |

### 10. Patches panels and web panels

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `patches.list` Patches list | pass | pass | pass | pass |
| `patches.run` Patch chips run | pass | pass | pass | pass |
| `panel.list` List panel | pass | pass | pass | pass |
| `panel.image` Image panel | pass | pass | pass | pass |

### 12. File previews and folders

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `file.host` Preview host | pass | pass | pass | pass |
| `file.picture` Picture view | pass | pass | pass | pass |
| `file.textview` Text viewer | pass | pass | pass | pass |
| `file.markdown` Markdown preview | pass | pass | pass | pass |
| `folder.list` Folder list | pass | pass | pass | pass |
| `folder.pathbar` Path bar | pass | pass | pass | pass |
| `folder.menu` Folder menu | pass | pass | pass | pass |
| `folder.filespanel` Files panel | pass | pass | pass | pass |

### 13. Terminal

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `terminal.panel` Terminal panel | pending | pending | pending | pending |
| `cards.run` Run on the card | pending | pending | pending | pending |
| `terminal.putcommand` Put a command in the Terminal | pending | pending | pending | pending |

### 17. Navigation, shortcuts, main menu

| Feature | Swift | Mac | Windows | Linux |
|---|---|---|---|---|
| `nav.history` Back/forward history | partly | pass | pass | pass |
| `nav.sidebuttons` Mouse side buttons | pass | pass | pass | pass |
| `nav.nextinbox` Next Inbox Thread | pass | pass | pass | pass |
| `menu.app` App menu | pass | pass | pass | pass |
| `menu.file` File menu | pass | pass | pass | pass |
| `menu.view` View menu | pass | pass | pass | pass |
| `menu.go` Go menu | pass | pass | pass | pass |
| `menu.thread` Thread menu | pass | pass | pass | pass |
| `menu.help` Help menu | pass | pass | pass | pass |
| `nav.shortcuts` Keyboard shortcuts | partly | pass | pass | pass |

<!-- parity:end -->

## The log

One entry a slice, newest first. Every entry has a `Verification:` line: a spec
run or a screenshot from the running app. A slice with no runtime check doesn't
count. (Ghostex's port log is where this shape comes from; its slices 214 to 268
built parsers and gates with no launch of the app, which is the trap.)

### 2026-11-10 One gate for opening links and files (Daybreak 58, 59, 63, 64, 67)

- Every link and file the app hands to the system goes through
  `desktop/gpui/src/open_gate.rs`, on the rules of the Swift app's OpenGate,
  written in `desktop/core/src/open_policy.rs`: a web page opens, any other
  scheme asks by name, a file is judged and opened as its real target after
  symlinks, a known document opens and anything else (a program, a bundle, a
  script, an unknown kind, a document whose app runs it, which macOS's Launch
  Services is asked) asks first, and another runner's file never opens here.
  File URLs are built from the path with `#` and `?` escaped, never pasted.
  `desktop/core/tests/open_gate_source.rs` fails on an opener anywhere else.
- The slash palette's Open file previews the skill in the pane; a file link in
  another runner's thread previews there too, read through that runner.
- Every rich text view (replies, plan and ask text, Markdown previews) is made
  by the gate's `text_view` or `markdown_view`, as the kit's own TextView opens
  a link with the system's opener when it has no handler; a link in a
  previewed Markdown file to another file previews in the pane.
- Threadbear's own approval cards show the runner's words under the title
  (`ApprovalText::runner_reason`), in both apps. File cards show every line
  of the diff, any argument it doesn't draw (`replace_all`, a notebook cell)
  and a line break at an edit's end in words, in both apps (and the runner
  sends an edit's card whole, refusing one too long to show), and Return on a stacked card allows only the ticked
  calls, as its Allow button does.
- Pasted pictures are staged in a fresh 0700 folder (`XDG_RUNTIME_DIR` when
  there is one), written 0600, and removed once the runner has its copy.
- Remaining: the Swift app's forwarding page for a local `.html` link's
  `?query` and `#fragment`, which the gate drops; previewing transcript file
  links in the pane on this runner's threads (they open in their app).
- Verification: `cargo test --workspace` on Tinker Desk (macOS, with the Launch
  Services code); the open policy's own tests and the source scan on Linux;
  the whole spec suite on Linux (see the report for the run).
### 2026-11-10 Looking like the Swift app, and the pool limit line

- Found by putting both apps' pictures of every demo thread side by side
  (`features/tools/compare.ts`, which Mike's "a number of things that are
  off" asked for).
- Dark mode was near-black: gpui-kit's own dark theme is #0a0a0a, where the
  Swift app draws on the system's window colour (#1e1e1e on macOS 26 and 27).
  The GPUI themes now take NSColor.windowBackgroundColor from AppKit on the
  Mac, and those values elsewhere (`style::install_window_colours`). Menus
  and popovers sit a little lighter, as macOS's do.
- Scroll bars show only while scrolling, as the Swift transcript sets them,
  whatever the system's setting. The side pane's second "Highlights" title
  under the dock's is gone. Rows name their Mac ("• m5") once there's
  another, as Swift's have since 9 Nov, and a long status gives way before
  the folder does. The thread's bar shows the project's own icon, not its
  letter.
- A thread waiting on a permission card counts as busy, as Swift's
  `isBusy` has it, so the composer offers Stop.
- `banner.limit`: a pool thread out of room says "Every login in Evenings is
  out of room. It carries on by itself at …" with a quiet clock and "Pin to
  mike (78% left)"; an account at its limit names its short login, when it
  resets, and the login with most room. The model chip says "Limit reached"
  (or "· 12%" running low) when every login in its pool is, and the account
  chip for its login. Ported from Swift's `limitAdvice`, `Pools.chipBadge`,
  `AccountUsageText.switchTarget` and `switchTitle` into `desktop/core`.
- Files: `desktop/gpui/src/{style,app,header,highlights,sidebar,remote,
  composer_chips,automation}.rs`, `desktop/core/src/{inbox,new_thread,
  logins,sidebar}.rs`, `AppDump.swift` (`limit`, `composer.chipBadge`),
  `features/header.feature`.
- Remaining from the pictures: reply buttons drawn as pills, the thread chips
  bar ("Review design options"), the rail's Connectors, Runners, Updates and
  Setup buttons and the Librarian's count, and the providers' logos on the
  model chip.
- Codex (gpt-6.1-sol) tried to break the drag, the caching, the sort, `busy()`
  and the limit port. It found no stale views, no lease panic and no change of
  order, and two real problems, both fixed: a torn-out window got its last
  move from a press that ended without a mouse-up, so it could jump to where
  the pointer had gone since (`pointer_up(released, ..)` in
  `desktop/dock/src/windows.rs`); and the limit line lacked Swift's case for a
  turn that failed on its limit while the account still says it has room
  (`failedOnAccount`, now `note_failed_account` in `header.rs`). That case
  looks for the newest turn footer, as Swift's does, and the demo thread that
  failed on its limit has none for the failed turn, so neither app shows a
  line there; there's no scenario for it yet.
- Idle frames: an open thread draws nothing once it has settled (four seconds,
  no frames); the readout's 30 fps was the second after opening one.
- Verification: `features/tools/compare.ts` pictures of the changed threads
  beside the Swift app's; specs 193 of 193 on Tinker Desk's Mac and on Linux
  (`cloud`), the Swift app 191 of 191 there, the new `banner.limit` scenario
  passing on both apps; `cargo test -p threadbear-core` 163 of 163.

### 2026-11-10 Dragging the window, and smooth scrolling at 120 Hz

- Mike couldn't move the installed app's window by its top. The strips that
  start the OS's window drag on the press worked; the panels' bars and the
  thread's title didn't, as they only know it's a drag once the pointer moves,
  and AppKit won't start a window drag from a move event. On the Mac those now
  move the window by hand, as Swift's dock does (`platform::grab_window`), and
  a drag from the title no longer starts a rename (`DockView::press_moved`).
- Scrolling: every frame rebuilt the whole window, thread list included. The
  Threads panel and the side pane are cached views now, the Inbox sorts by
  keys worked out once, and the menu bar and the Inbox badge are worked out
  only when the app changes. Tinker Desk (M2, 60 Hz), 6,000 rows and 60
  threads, real flings: work per frame 7.8 ms in the middle, 13.4 at 90% and
  21.8 at 99% before; 6.3, 7.9 and 9.7 after (28 late frames to 1).
- View > Show Frame Rate in the installed build (`frame-stats`, GPUI's frame
  profiler) shows frames a second, late frames and the slowest frame.
- Files: `desktop/dock/src/{platform,view,windows}.rs`, `desktop/gpui/src/
  {frame_stats,sidebar,shell,app,rails,header,composer_chips}.rs`,
  `desktop/core/src/inbox.rs`, `features/tools/{window-drag,scroll-perf,
  real-screen}.ts`.
- Remaining: the root, the dock's bars and the thread pane still draw again on
  every frame of a scroll (the dock would have to hand out its bars as views).
- Verification: `features/tools/window-drag.ts` on Tinker Desk's desktop,
  every part of the top moves the window and a click on the title renames;
  `features/tools/scroll-perf.ts` for the numbers above; specs 192 of 192 on
  Tinker Desk's Mac and 191 of 192 on Linux (`cloud`). The one was a step
  that took an older reply for the streaming one when the stream was slow to
  start; it now tells them apart, and passes 4 of 4 on Linux and on both apps
  on the Mac.

### 2026-11-10 Every spec on real Windows

- Windows paths: the runner's `C:\\` paths work in folder names, relative paths
  and Highlights (`forward_slashes` in `desktop/core`), and the mock accepts them.
- Remaining: the folder browser still joins with `/`, which Windows tolerates
  but shows as mixed separators.
- Verification: `Behaviour specs` in the Desktop app workflow, 192 of 192 on
  Windows, Mac and Linux (run 42648513140).

### 2026-11-09 The first app that can be used all day

- Merged: the sidebar, the transcript's real rows, the composer and its cards,
  the window on BearDock with its rails, header, banners, menus and shortcuts,
  Highlights, previews, Patches, the Files panel, the new-thread page, the slash
  palette and other runners' threads.
- Fixed in the merge: dropping a file on selectable text now works, as a drop
  layer goes over the thread while a file's being dragged.
- Remaining: Scheduled, Projects, Librarian, Usage, Updates and Settings pages,
  the bears and flights, web panels, the terminal.
- Verification: 192 scenarios pass on Linux and on a real Mac; the Swift app passes
  the 188 of them that aren't marked GPUI-only.

### 2026-11-09 Against a real runner

- `features/tools/real-runner.ts`: the GPUI app on a dev runner, not the mock,
  sending a message on claude-haiku-5-5 and getting the reply back.
- Verification: on `cloud`, "reply after 4480 ms" with rows `user group assistant
  footer`, the same row kinds as the Swift app.
### 2026-11-09 The window shell: BearDock, rails, the thread's bar, banners, notes, menus and keys

- The app's window is BearDock now: `desktop/gpui/src/shell.rs` hosts the dock
  (threads, thread, Highlights as panels, default layout and View > Reset, the
  layout saved and restored), with no title bar and the traffic lights in the
  top-left panel's bar, the bars moving the window. `rails.rs` is the activity
  bar and the right rail (`ActivityBar`, `PanelRail`, with the Inbox count, lit
  buttons and the Highlights dot). `header.rs` is the thread's title line in the
  panel's bar (inline rename, project and schedule chips), back and forward with
  tips that name where they go, share with its popover, the ... menu, the banners
  (connection, error with Investigate, limit, sign-in, the refusal offer) and the
  floating notes (Archived with Undo, end of inbox). `settings.rs` is one JSON file
  in the runner's data folder (`desktop-settings.json`, keyed by the Swift
  UserDefaults names): window frame, zoom, folds, the Inbox's sort, the panel
  layout. Zoom scales everything: every `px` in the window goes through
  `style::px` (the Swift `zoomed`), which each drawing file gets with one
  `use crate::style::px;` that shadows GPUI's, and gpui-kit's own sizes follow
  through the theme's rem size. Shortcuts are `threadbear-core`'s `shortcuts`
  (the Swift tests ported), bound from shortcuts.json with a poll that picks up
  edits. Menus are the same model for the Mac's menu bar and the `menus` command.
- Files: `desktop/gpui/src/{shell,rails,header,settings}.rs` (new), `app.rs`
  (state and the render), `main.rs` (window options, frame memory), `style.rs`
  (zoom), small additive edits to `sidebar.rs`, `thread.rs`, `composer_card.rs`,
  `rows/*`, `automation.rs` (the Swift names: `dock-move`, `panel`, `float`,
  `mode`, `zoom`, `rename`, `undo-archive`, `open-archived`, `error`,
  `mouse-button`, `copy-thread-id`, `menus`, `menu-item`, `app-menu`, `data-dir`,
  `share`, `signin`, `refusal`, `appearance`, `resize`), `desktop/core`
  (`navigation`, `shortcuts`, `zoom`), `desktop/client` (`retry_now`),
  `desktop/dock/src/view.rs` (`set_options`, so the bars follow the zoom). Specs:
  `features/{window,rails,header,navigation}.feature`. Swift: `AppDump` has
  `rails`, `header`, `zoom`, `inboxEndNote`; new debug commands `menus`,
  `menu-item`, `data-dir`; `dev-app.sh rebuild --no-build`.
- Remaining: the Scheduled, Projects and Librarian views and Usage/Settings pages
  are placeholders (later stages). Highlights' content comes from
  `desktop/gpui/src/highlights.rs` (other branch): `render_highlights_panel` in
  `shell.rs` is the one place to call it. Files is a placeholder panel. The
  Browser rail button, panel origin badges and a thread's own patch panels'
  content aren't there yet. Menus aren't drawn on Windows and Linux yet
  (`AppMenuBar`, `gpui-kit`'s `title-bar` for the window buttons there).
  Sign in… says the CLI command rather than opening Terminal. The refusal offer
  sits with the banners, not above the composer. Hold-⌘ hints, swipes and the
  honey pot are other rows. A scripted Swift app's menu items keep their greyed
  state from the last time the menu was opened, so three key-press scenarios are
  `@swift-pending` (`⌘[`, `⌘0`, `shortcuts.json` pressing the new key); the same
  state shows they pass through the menu on GPUI. The picture's traffic lights
  aren't in the GPUI shot (it draws in-process); their position was set from the
  Swift shot and not checked against a real window capture.
- Written but with no runtime check yet: window frame memory (scripted runs never
  save a frame, so only `zoom::on_screen` and the settings file have tests); the
  limit, sign-in and refusal banners and the project and schedule chips (no spec
  or fixture reaches them); a real mouse's side buttons (`mouse-button` calls the
  same handler, not a real event). On GPUI the `@menu.*` scenarios read the menu
  model, not the system menu bar. Simplified from Swift: the limit banner offers
  the first login not at its limit rather than the one with most room, and a
  waiting restart's banner has one fixed sentence.
- Verification: `TB_APP=swift` on Tinker Desk: 37 scenarios (37 passed); `TB_APP=gpui`
  on `cloud` (Xvfb): 40 scenarios (40 passed); `TB_APP=gpui` on Tinker Desk: 40
  scenarios (40 passed), the `@dock.layout or @dock.reset or @foundation.zoom or
  @rail.* or @header.* or @banner.* or @toast.* or @nav.* or @menu.* or
  @sidebar.sections or @sidebar.sort` set. `cargo test -p threadbear-core` (the new
  `navigation`, `shortcuts` and `zoom` modules) and `settings` tests. Side-by-side
  picture of the window on Tinker Desk: the Swift app and the GPUI app with the
  same thread open, 1180 by 800, dark.
### 2026-11-09 Move to another runner (`sidebar.move`)

- The row menu's Move to ▸ (behind `moveToRunner`): it goes straight away when
  the other runner has a clean answer, else the sheet comes up with its guess at
  the folder and login, what's in the way and what won't come, and asks again as
  they change. Cancel Move while it's going, and "Moving to bruce" on the row.
- Files: `desktop/gpui/src/move_sheet.rs`, `desktop/core/src/remote.rs`,
  `desktop/gpui/src/sidebar.rs` (the menu lines), `features/remote-threads.feature`,
  `app/Sources/Threadbear/Views/MoveSheet.swift` and `AppDump.swift` (the dump's
  `moveSheet`, debug only).
- Remaining: the login picker is a popover where Swift's is a pop-up menu; a
  summary landing while a move is being asked for takes the row's guess off
  (`keepingAskedMove`), which only shows as a flicker.
- Verification: `@sidebar.move`, 5 of 5 on `TB_APP=swift` and on `TB_APP=gpui` on
  Tinker Desk, and in the whole-suite runs under the new-thread page's entry
  below; a picture of the sheet on Tinker Desk.

### 2026-11-09 Other runners' threads and the runner chip (`sidebar.row.remote`, `composer.runner`)

- Rows from `remoteThreads.list`/`remoteThreads.updated` sit beside this Mac's,
  with the machine's icon in the tile and "On bruce" (faded when offline); opening
  one reads its events and sends through this Mac's runner, the app's default way
  in (the direct link's own connections aren't ported, `directRemoteLink`).
  The box on such a thread leads with its machine's chip; a thread's logins and
  folders are that runner's once they've come (`remoteThreads.logins`), as is the
  draft's. The new-thread box has the Runs on chip: this Mac, each runner, the star
  on the default and a Default submenu that sets it, and on another runner its
  folders. `composer-menu` with `runner` reads and clicks it, `setDefault` sets
  the star.
- Files: `desktop/gpui/src/remote.rs`, `desktop/gpui/src/composer_chips.rs`,
  `desktop/core/src/remote.rs`, `features/remote-threads.feature`,
  `app/Tools/mock-runner/server.mjs` (the mock now answers another runner's
  threads over its own connection, as the real runner does through the backend,
  and `remoteThreads.logins`).
- Remaining: the direct link (Settings > Runners), `files.read` of another
  runner's paths (their pictures show their own placeholder), the sparse mode is
  on for the relay but a real relay hasn't been run from this app.
- Verification: `@sidebar.row.remote` and `@composer.runner`, 7 of 7 on both
  apps on Tinker Desk, and in the whole-suite runs under the new-thread page's
  entry below.

### 2026-11-09 The slash command palette (`composer.slash`, `composer.slash.keys`)

- Floating over the box in the same window (a deferred layer, as wide as the box,
  half the window tall): categories with counts and Recent, the matches, the picked
  command in full with Run, Add details and Open file. The keys come from the box's
  own actions while it's open (arrows move the pick, Tab fills it in, Return runs
  it, Esc closes it until the words change), so the cursor stays in the box.
  Recent is kept in `gpui-prefs.json`. The rules are in `desktop/core/src/slash.rs`
  with tests; the dump's `palette` is on both apps.
- Files: `desktop/gpui/src/slash_palette.rs`, `desktop/gpui/src/composer.rs`,
  `desktop/core/src/slash.rs`, `features/slash-palette.feature`,
  `app/Sources/Threadbear/AppDump.swift` and `ComposerView.swift` (debug only).
- Remaining: `composer.slash.resize` (dragging its edges and columns);
  descriptions read as plain text rather than inline markdown. (Open file now
  previews the skill in the pane, as the Swift app does: see the 10 Nov entry.)
- Verification: `@composer.slash` and `@composer.slash.keys`, 9 of 9 on both apps
  on Tinker Desk, and in the whole-suite runs under the new-thread page's entry
  below; `cargo test -p threadbear-core` (the palette's own rules, 18 tests); a
  picture of `/co` on Tinker Desk read against SlashPaletteView.

### 2026-11-09 The new-thread page (`thread.newpage`, `composer.starting`)

- ⌘N and the compose button open a page with the box about 40% of the way down,
  the heading above it, "Starts in misc on the Claude pool" under it and the
  logins' room cards (a click pins the draft to that login); a project's New
  Thread adds its "In …" chip and folder. Sending goes through `threads.create` on
  the draft's pool or login, the words greyed and locked with a spinner for Send
  until the thread exists, and starts the next one from the defaults again. `new`
  (with `project`), `options`, and `composer-menu` work on the page as in the
  Swift app. The picks that stick (model, effort, last login, default runner, recent
  commands) live in `<data dir>/gpui-prefs.json`; `docs/on-your-mac.md` should
  name it when this app ships.
- Files: `desktop/gpui/src/new_thread.rs`, `desktop/gpui/src/prefs.rs`,
  `desktop/core/src/new_thread.rs`, `features/new-thread.feature`, a page branch
  in `desktop/gpui/src/app.rs`.
- Remaining: the page doesn't clamp the heading and note for very short windows
  as ComposerPlacement does; the bear over the heading.
- Verification: `TB_APP=gpui` whole suite, 125 of 125 on Tinker Desk (real Mac
  desktop) and 125 of 125 on Linux (`cloud`) at the final commit; `TB_APP=swift`
  whole suite, 125 of 125 on Tinker Desk; `cargo test -p threadbear-core`
  (92 lib tests); side-by-side of the page at
  `/tmp/gpui-new-thread-side-by-side.png`.
### 2026-11-09 Pictures in the transcript: an in-app viewer

- A click on a picture tile opens an overlay on that picture, with arrows through
  its message's other pictures and Escape to close, in place of the file's default
  app. Only a real, decodable picture opens (named as one, not a code runner, then
  decoded from its bytes): `viewer.rs`, `looks_like_picture` and `runs_code` in core,
  the `quickLooks` rule carried over.
- `tile` and `viewer` join the automation commands on both apps (Swift's `viewer`
  says what Quick Look would have shown, as the real panel comes to the front of a
  desktop); the arrows are GPUI's own, so that scenario is `@swift-pending`.
- Files: `desktop/gpui/src/viewer.rs`, `rows/images.rs`, `features/picture-viewer.feature`.
- Remaining: zoom and pan, video, PDFs, drag out (`file.quicklook`, `drag.fileout`).
- Verification: `TB_APP=swift` 1 of 1 and `TB_APP=gpui` 2 of 2 on Tinker Desk (real Mac) and on Linux (`cloud`), the arrows scenario `@swift-pending`; picture of the overlay on the GPUI app. Whole suite with the pane's 19 new scenarios: `TB_APP=swift` 124 of 124 on Tinker Desk, `TB_APP=gpui` 125 of 125 on Tinker Desk and 124 of 125 on Linux (one streaming scenario was too slow on a loaded box and passed on its own).

### 2026-11-09 Files panel and the folder browser

- The Files panel and folder previews: a list with fold-out rows, sortable columns
  (Date Modified and Size go when the pane is narrow, as the Finder's do), a path bar
  under the list, double-click to go in with Back to come out, and the right-click menu
  (Open, Open in its app, Reveal, Copy path, Sort by, Show hidden files). Rows come
  from the runner's `files.list`, so a thread on another Mac browses the same.
- Files: `desktop/core/src/folders.rs` (FolderSort, crumbs, sizes, `files.read`
  decoding), `desktop/gpui/src/folders.rs`, `features/folders.feature`.
- Remaining: the icon grid (`folder.icons`, the menu item is there, greyed), Finder keys
  and Quick Look on Space (`folder.keys`), type-to-select, the folder watch.
- Verification: `TB_APP=swift` 6 of 6 on Tinker Desk; `TB_APP=gpui` 6 of 6 on Tinker Desk (real Mac) and on Linux (`cloud`); side-by-side pictures against the Swift pane.

### 2026-11-09 Patches: the list, chips and native panels

- The Patches section (Trust, waiting lines, what each gives, grouped under its
  project), chips over the composer (`actions.run`, spinner while it runs), list panels
  (headings, tones, Show in pane rows), image panels (a refreshed picture through
  `files.read`, so another Mac's works) and the web-panel placeholder with Open in Browser.
- Files: `desktop/gpui/src/patches.rs`, `features/patches.feature`, core `Panel`,
  `Action`, `PatchInfo`, `PanelRender`.
- Remaining: chip look (the Swift app's grouped bar with its +N overflow; these are
  simple pills), patch and `.app` picture icons (`patches.icons`), web panels
  (`panel.web`).
- Verification: `TB_APP=swift` 5 of 5 on Tinker Desk; `TB_APP=gpui` 5 of 5 on Tinker Desk (real Mac) and on Linux (`cloud`); side-by-side pictures against the Swift pane.

### 2026-11-09 File previews in the pane

- `PanePreview` and its Back stack, the preview host, a picture view with Copy Image
  (the file's own bytes), code in a read-only editor coloured by the kit's syntect
  highlighter (the `syntect` feature on `gpui-kit`; the transcript's code blocks stay
  plain, as the Swift app's are), Markdown drawn as a page, and web pages and `.html`
  files handed to the system browser for now.
- Files come from the runner (`files.read`), with `runnerId` for another machine's
  thread, so a preview never touches the Mac's own disk.
- Files: `desktop/gpui/src/files.rs`, `features/file-previews.feature`.
- Remaining: the in-app web view (`file.htmlpreview`, later work), video and PDF
  (`file.video`, `file.quicklook`), editing and find (`file.editor`, `file.find`),
  the Markdown Preview/Raw menu, drag out of a picture.
- Verification: `TB_APP=swift` 5 of 5 on Tinker Desk and `TB_APP=gpui` 5 of 5 on Tinker Desk (real Mac); side-by-side pictures against the Swift pane.

### 2026-11-09 The pane beside the thread: Highlights

- `desktop/core/src/highlights.rs` ports the logic and protocol types of
  HighlightSections, HighlightFold, HighlightProjects, PreviewRules, FileIcon,
  HighlightText, PanelIcon, ScheduleRows, StartedThreadRows and PatchGroups,
  with the Swift tests. `desktop/gpui/src/highlights.rs` is the pane: one
  self-contained `SidePane` entity beside the thread in a simple split (`new`
  and `render`; the shell will host it as a dock panel), drawing the brief,
  Needs you ticks (the same `threads.tickAsk`, optimistic), project blocks with
  the ⓘ popover (Keep, Take out), Scheduled, Sub-agents, Threads it started,
  Pinned, Files, Links and Changes, with Show all, the folds remembered across
  launches, and the row menus.
- `dump`'s new `panels.pane` says what's drawn on both apps (AppDump.swift,
  `highlights.rs` `dump`); `highlight` joins the automation commands
  (app/README.md).
- Files: `desktop/core/src/highlights.rs`, `desktop/gpui/src/highlights*.rs`,
  `features/highlights.feature`, `features/support/pane.ts`,
  `app/Sources/Threadbear/AppDump.swift`, `Automation+Commands.swift`.
- Remaining: the Notion board (`highlights.board`), the project page link from
  the ⓘ, middle truncation of file names, drag-out, Open in Panel's page view.
- Verification: `TB_APP=swift` 14 of 14 and `TB_APP=gpui` 14 of 14 on Linux
  (`cloud`) and on Tinker Desk; side-by-side picture of the pane against Swift's.

### 2026-11-09 BearDock for GPUI

- New crates: `desktop/dock/layout` (the tree, line for line from Swift, no GPUI)
  and `desktop/dock` (rendering, dragging, drop targets, torn-out windows), with
  a demo.
- Not in the app yet. GPUI can't scale or tilt a live subtree, so the ghost is a
  second copy of the panel. Glides between layouts aren't there. Moving a window
  by hand is Mac only; Windows and X11 use the OS drag, Wayland a menu command.
- Remaining: integrating it into the app, then the glides.
- Verification: `cargo test -p threadbear-dock-layout`, 32 tests; `press` tests
  headless, 4; real CGEvent drags on Tinker Desk through `pointer-test.sh`.

### 2026-11-09 Composer: the open card in the box

- The open card (Needs you or Done) is drawn at the top of the composer from the
  reducer's `open_card()`, and its row in the list is zero height, as the Swift
  one is. Done has Archive and its follow-ups with tick and "?"; Needs you has its
  asks with answer buttons or rows, Something else boxes, Return going on to the
  next unanswered ask, collapse, and one ask at a time when it's tall. Picks stay
  with the composer while another thread is open, and `decide` and `ask` match
  the Swift automation (`dump` has `composer.card` and `decisions` on both apps,
  the Swift one added to AppDump.swift).
- Files: `desktop/gpui/src/composer_card.rs`, `desktop/gpui/src/rows.rs` (one
  line), `features/composer-cards.feature`, `app/Sources/Threadbear/AppDump.swift`.
- Remaining: pictures in the Something else boxes, the page slide, the card
  keeping its tallest page's height, link buttons on asks, the beep when Return
  can't go on, the picks' chips when collapsed, the new-thread page and the
  slash palette.
- Verification: whole suite 44 of 44 on `TB_APP=gpui` on Linux (`cloud`) and a
  real Mac (Tinker Desk), and 44 of 44 on `TB_APP=swift` on Tinker Desk;
  side-by-side screenshot of the paged card against Swift's.

### 2026-11-09 Composer: the chips under the box

- The row under the words: "+" (Attach images…, folder), the model chip with its
  picker (search, rail of pools or providers, rows with default and selection,
  effort), the permission chip, the account chip when there are no pools, and the
  context ring with its popover. Each click updates at once through the
  `update_open` guess. `composer-menu` (permission, model, context) matches the
  Swift text; `desktop/core/src/logins.rs` ports the picker logic with tests.
- Files: `desktop/gpui/src/composer_chips.rs`, `desktop/core/src/logins.rs`,
  `features/composer-chips.feature`.
- Remaining: provider logos on the model rows and chip, usage badges on the
  account and model chips, Older rail details, Schedule… in "+".
- Verification: `TB_APP=swift` on Tinker Desk 4 of 4 for
  `@composer.permission @composer.modelpicker`; `TB_APP=gpui` on Linux (`cloud`)
  4 of 4 and on Tinker Desk 10 of 10 together with the composer's text scenarios;
  screenshot of the chip row against Swift's.

### 2026-11-09 Composer: pictures as inline chips

- Paste, drop or the automation's `paste-image` puts a chip with a thumbnail in the
  words at the cursor, each picture copied in through `media.import`, dimmed
  until it's in; the strip of thumbnails over the words has × on hover; a chip's
  hover shows the picture after a beat; `[Picture N]` markers go with the words.
  These are real gpui-kit inline tokens (`InlineToken` on `TextareaState`, 0.9.0),
  so no fork: atomic in the editing, in undo, with the Swift draft's U+FFFC mark.
  The whole thread is a drop target (`on_drop` and `drag_over` on the pane).
- Files: `desktop/gpui/src/composer.rs`, `features/sending.feature`,
  `features/support/sample.png`, `features/tools/shot.ts` (optional commands).
- Remaining: `composer.chipcopy`, a whole-insert undo step (each chip and its
  spaces are separate undo steps today).
- Verification: `TB_APP=gpui` on Linux (`cloud`) and a real Mac (Tinker Desk), 4 of
  4 for `@composer.picturechips @composer.filedrop @composer.chiphover`;
  `TB_APP=swift` on Tinker Desk 4 of 4; screenshot of two chips against Swift's.

### 2026-11-09 Composer: its own file, the rounded box, list continuation, paste

- The composer is out of `thread.rs` into `desktop/gpui/src/composer.rs`, one
  `Composer` entity per thread on the app (`Threadbear.composers`) so what's typed
  and its undo stay with the thread. Rounded card, 15/23 text, two to ten lines,
  Return sends, Shift-Return makes a new line and carries markdown lists on
  through one edit (`ListContinuation` is in `desktop/core/src/composer.rs` with
  its tests), the send circle and Stop. `paste` joins the automation commands.
- Files: `desktop/gpui/src/composer.rs`, `desktop/core/src/composer.rs`,
  `features/sending.feature`.
- Remaining: pictures, the chips under the box, the cards, the new-thread page.
- Verification: `TB_APP=gpui` on Linux (`cloud`) and a real Mac (Tinker Desk) 7 of
  7 for `@composer.textbox @composer.return @composer.send @composer.listcontinue`;
  `TB_APP=swift` on Tinker Desk 4 of 4 for the new ones; side-by-side screenshot of
  the box against the Swift one.
### 2026-11-09 The transcript's rows, drawn like the Swift app

- What changed: the reading column, Mike's bubble, assistant markdown in
  gpui-kit's TextView styled from Theme.swift's values (`rows/markdown.rs`),
  work lines opening to their detail (`rows/work.rs`), diff rows and the Edited
  N files card (`rows/diff.rs`), every card with its buttons, spinner and
  tooltips (`rows/cards.rs`), images (`rows/images.rs`), notes, gaps, errors,
  footers, the row's right-click menu with Copy message, Copy transcript and
  Copy debug info (`rows/export.rs`), work links scrolling to their step, and
  the list itself: GPUI's `list` with TranscriptController's behaviour
  (`rows/list.rs`): tail following, paging older events in near the top,
  remembered scroll per thread (across a thread being reopened, paging until
  its row is back), not moving when rows open or are added above.
- Files: `desktop/gpui/src/rows.rs`, `desktop/gpui/src/rows/`, the list parts
  of `desktop/gpui/src/thread.rs`, additions to `desktop/gpui/src/style.rs`,
  `automation.rs` (`scroll`, `link`, `menu`, `clipboard`, `expand` with `file`
  and `all`); `features/reading-a-thread.feature`, `features/cards.feature`,
  `features/scrolling.feature` and their steps; `features/tools/rows.ts`.
- Remaining: the bear in the working line (a paw for now; foundation.bear),
  file-path links' search of the files a thread's tools touched and their
  right-click menus (paths in inline code link and open from the thread's
  folder), reply buttons, thread and picture chips, the selection bar, the
  edit pencil and send lift, the table's natural widths (the kit stretches
  them to the column), held-back tool output for other runners' threads, the
  lit-up step's fade, opening a review.
- Verification: `TB_APP=swift cucumber-js` on Tinker Desk's desktop, 41 of 41
  scenarios; `TB_APP=gpui cucumber-js` on Tinker Desk's desktop, 41 of 41, and
  on `cloud` under Xvfb, 41 of 41. `features/tools/long-thread.ts` on Tinker
  Desk (M2), release build, real rows: first page open in 23 ms, 10,000 rows
  loaded and folded in 123 ms, a scrolling frame 0.55 ms median, 1.2 ms worst
  (0.62 and 2.5 ms for fast scrolling). Side-by-side screenshots of every card
  and row kind from both apps.
### 2026-11-09 The sidebar: rows, headings, actions, Hibernate

- Rules ported to `desktop/core/src/sidebar.rs` and `inbox.rs`: `ThreadSections`
  (the Inbox order, and the Mac's Inbox/Running split where the first port had
  followed the website; `InboxUrgency` now checked against its shared cases),
  `ThreadRowLook`, `RowMode`, `ThreadByline`, `ProjectMark`, `RelativeTime` and
  the trailing words, `Snooze`, `WakeWords`, `BackgroundWork`,
  `BusyThreadAction`, `SubagentRows`, `ArchiveRows`.
- `desktop/gpui/src/sidebar.rs` draws them: tile (letter, symbol or picture),
  bold title, place line with the origin mark, blue dot or tick, status and
  time with the ticking timer, headings with counts and folds, the Inbox sort,
  hover buttons that change the row at once, the right-click menu, Hibernate
  presets, Pick a time, Wake when, Rename and Delete, search, New thread, the
  sub-agent tree, archived paging, the loading rows. `icons.rs` maps SF Symbols
  to Lucide.
- `sidebar.archived.foot` dropped: the Swift app undid it on 9 Nov (cfbf3cb3).
- Specs: four new files, 32 scenarios (`sidebar-rows`, `-sections`, `-actions`,
  `-loading`); the Swift `dump` says what a row draws; `desktop/scripts/spec.sh`
  builds and runs them through `remote.sh` with a private copy of the binary.
- Remaining: other runners' rows and Move to, swipe and drag, row motion; Open
  in New Panel is greyed until BearDock; a sub-agent's click opens its thread
  (no sub-agent transcript yet); the open row doesn't hold its place when read;
  folds and sort aren't kept across launches; Pick a time has no script path on
  either app (`@manual`).
- Verification: `TB_APP=swift cucumber-js` on Tinker Desk, 56 of 56 scenarios;
  `TB_APP=gpui cucumber-js` on Tinker Desk 56 of 56 and on `cloud` 56 of 56;
  `cargo test -p threadbear-core`; side-by-side pictures from Tinker Desk.

### 2026-11-09 Risk checks: Chromium in a slot, accessibility, Windows

- Spike (`gpui-web-spike` branch): CEF 156 in a GPUI layout slot on macOS, with a
  gpui-kit menu in its own window over the page.
- Accessibility: Voice Type works; the AX tree below the window doesn't yet.
- Windows: CI ran all 24 scenarios on a real `windows-latest` machine.
- Remaining: Windows and Linux for the web engine, the accessibility tree.
- Verification: spike screenshots from Tinker Desk; `ax-dump` on Tinker Desk;
  `Behaviour specs` in the Desktop app workflow, 24 of 24 on Windows.

### 2026-11-09 The transcript's logic in Rust

- Ported `ThreadbearCore/Transcript` to `desktop/core/src/reducer`, and the
  GPUI thread pane now draws its rows from it (a first pass at how each kind
  looks).
- Golden files: `GoldenTranscriptTests.swift` writes the rows for all 18
  fixtures, closed and open, and both languages check them.
- Remaining: every row's real look, composer card, paging older rows in.
- Verification: `cargo test -p threadbear-core`, 197 tests; specs 24 of 24 on
  Linux (`cloud`) and on a real Mac desktop (Tinker Desk).

### 2026-11-09 Long-thread speed check

- `features/tools/long-thread.ts` on Tinker Desk (M2), release build: a thread
  of 12,500 rows loads and folds in 94 ms, and a frame takes 0.6 ms at the
  median and 2.5 ms at worst while scrolling. A debug build is about ten times
  slower and still fast enough.
- Remaining: paging, and the same check after real row layouts land.
- Verification: the numbers above, run on the machine.

### 2026-11-09 The spec harness

- Added `features/` (Gherkin, Cucumber.js), driving the Swift app through
  `dev-app.sh` and the GPUI app through a debug-only automation socket.
- Files: `features/**`, `desktop/gpui/src/automation.rs`,
  `desktop/scripts/remote.sh`.
- Remaining: the rest of the daily driver.
- Verification: `TB_APP=swift cucumber-js` on Tinker Desk, 24 of 24 scenarios;
  `TB_APP=gpui cucumber-js` on `cloud` under Xvfb, 17 of 24 at this point.
