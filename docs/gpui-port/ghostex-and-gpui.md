# GPUI port research: Ghostex, gpui-kit and web view options

**Read at:**
- Ghostex [`49b92f8`](https://github.com/maddada/Ghostex/tree/49b92f842ae7c5d752d476ae1ba7b4c6f695d311) (release 11.2.1, 9 Nov)
- gpui-kit 0.9.0, gpui-component 0.9.0 and gpui-pre 0.3.11 from `~/.cargo/registry`, which is what the spike compiles (gpui-pre is a snapshot of `zed@f816e1f`, 1 Nov)
- [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit) head `ca479f3`

Nothing was built or run. **[V]** means I read it in source or an API response. **[I]** means inferred.

## Corrections to what we thought
- **GPUI does have accessibility now.** AccessKit is wired up on all three desktop platforms. The note that GPUI exposes nothing below the window is out of date for the framework. Why Mike's tools saw nothing in Ghostex is still unexplained (see "Accessibility" below).
- **A web view component exists.** It's a separate crate, [`gpui-webview` 0.9.0](https://github.com/longbridge/gpui-kit/tree/main/crates/webview), labelled experimental. It isn't in `gpui-kit`'s own dependencies.
- **Ghostex's port log has 517 numbered slices,** not 200+, and that's the workspace log alone.

## Part 1: Ghostex

### How the port was organised
Three ledgers: [Workspace](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/WORKSPACE_PARITY_PROGRESS.md) (6,780 lines), [Settings](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/SETTINGS_PARITY_PROGRESS.md) and [Titlebar/modals](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/TITLEBAR_APP_MODAL_PARITY_PROGRESS.md). The split is in [ARCHITECTURE.md](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/ARCHITECTURE.md). [V]

Entry format [V]:
```
### 2026-06-22-05:22 Task slice 2

- Improved the GPUI Agents terminal tab bar chrome while keeping libghostty as a black placeholder surface.
- ...
- Files touched: `gpui/src/main.rs`, `gpui/WORKSPACE_PARITY_PROGRESS.md`.
- Commands run: `cargo fmt`; `cargo check`.
- Remaining gaps for later slices: real close behavior, tab overflow measurement/reveal behavior, ...
```
Later entries add a `<!-- CDXC:Area date: why -->` comment and a `Verification:` line.

**Slice size [V, measured]:**
- The workspace log has 522 entries over 22 to 27 June, at most 124 a day.
- The median entry is 13 lines, and the median gap between entries is 7 minutes.
- Workspace ran one worker at a time on "gpt-5.5 xhigh", usually touching only `main.rs` and the ledger.
- Later rules asked for larger bounded bundles, and the last slices (around 514 to 517) used two parallel panes.

**Verification [V, line counts by regex, so approximate]:**
- `cargo check` appears on 421 lines and `cargo test` on 266.
- "No app launch" appears on 203.
- From about slice 200, many entries say "Checks skipped by explicit user instruction".

**What went wrong:**
- Agents built "source-only evidence" with no runtime proof. Slices 214 to 268 add gates and parsers while the panes stay placeholders. The user then ruled that source-only parity counted, and that he would check at runtime later. [V]
- Slice 224 had to tell workers to stop splitting ledgers. [V]
- The "reuse React in CEF" plan was reversed. The React sidebar went on 21 Sep, Kanban on 30 Sep and the modal host on 1 Oct, all replaced by native GPUI. [V]
- `main.rs` became the monolith and was later split (now 663 lines; the split doc isn't public). [V for size, I for history]
- The Windows and Linux CEF adapters were written "without hardware". [V]
- Fork drift is ongoing (next section).

### Which GPUI [V]
- `gpui = { git = zed }`, redirected by `[patch]` to a gitlink [`maddada/zed@0fa8b8c`](https://github.com/maddada/zed/tree/0fa8b8c10cb32fdd196450dbd6729961d66389af). It isn't gpui-ce.
- That fork is **Zed main (`4a6121bb0f`) with GPUI Fast 5bf7480 imported**, plus 147 commits ahead of Zed main (20 behind, 230 files). The crates touched are gpui (110 files), gpui_windows (36), gpui_wgpu (23), gpui_linux (23), gpui_apple (18), gpui_macos (11).
- Patch themes:
  - Windows child-window input and focus, and "compose GPUI below native child windows"
  - X11 focus proxy and transient windows
  - hover clearing into child views
  - tooltip presenter and native-view occlusion regions
  - glass and blur on every OS
  - Metal shader effects, view retention, and Taffy 0.14 layout
  - a web DOM mirror for accessibility
- Components: [`maddada/gpui-component@2708d8c`](https://github.com/maddada/gpui-component/tree/2708d8ca46e9339cb9e720b2612624346353295d), a fork of gpui-kit (122 ahead, 35 behind). It carries input pills, a document-order selection registry, managed tooltips, table layouts and scrollbar control.
- Own or other components: `zorite-editor` (Docs), and libghostty-vt drawn as a GPUI element (terminal).

### What ships per OS [V, release assets]

| OS | Package | Updater | Signing |
|---|---|---|---|
| macOS arm64 only (13+) | 57 MB DMG | Sparkle, loaded at runtime ([585-line shim](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/native/macos/GpuiSparkleUpdater.m)); `appcast.xml` in the repo, Ed-signed | Developer ID, `notarytool` then staple ([macos-notary.sh](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/tooling/release-gpui/macos-notary.sh)) |
| Windows x64 and arm64 | 61 to 69 MB installer | Velopack with a GitHub feed ([windows_updater.rs](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/src/windows_updater.rs)) | SignPath wired in three rounds but **off: ships unsigned** ([code-signing.md](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/docs/code-signing.md)) |
| Linux x64 and arm64 | deb, rpm, AUR, tar.zst, AppImage (94 to 100 MB) | AppImage only ([linux_updater.rs](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/src/linux_updater.rs)); the others only get a notice | GPG on the AppImage |

Builds run in [GitHub Actions](https://github.com/maddada/Ghostex/tree/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/.github/workflows) on `macos-15`, `windows-2025`, `windows-11-vs2026-arm` and `ubuntu-22.04`(-arm).

### CEF ([`apps/desktop/src/cef`](https://github.com/maddada/Ghostex/tree/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/src/cef)) [V]

**Crates and size:**
- [`cef-rs`](https://github.com/tauri-apps/cef-rs) by gitlink, resolving to 156.3.0+156.0.15. Plus `x11rb` and `windows-sys`.
- `cef/` is 11,229 lines: about 7,600 in `shell/`, macOS 677, Windows 720, Linux 1,594 across three files.
- Outside that: 2,240-line AppKit hooks, 2,724-line component store, 1,230-line install window, plus bootstrap and helper binaries.

**Embedding:**
- A windowed child (`set_as_child`) in Alloy style, with an external message pump driven from `on_schedule_message_pump_work`.
- `CefElement` ([cef_surface.rs](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/src/app/element/cef_surface.rs)) takes a normal GPUI layout slot and sets native bounds in prepaint.
- The page sits above GPUI, so menus and popups are separate windows, and tooltips lay themselves out around native views.

**Per OS:**

| OS | State |
|---|---|
| macOS | Primary target |
| Windows | Child HWND. The file header still says it needs device verification, but it ships [I that it works] |
| Linux | **X11 only.** Device-verified on Ubuntu 26.04 XWayland/KDE. The whole app runs as X11 (hides `WAYLAND_DISPLAY`, restores it for child processes), accepting worse fractional scaling and IME |

**Download on first use:**
- `on-demand-resources.json` (schema 2) lists component, version, tag, repo, and for each platform the asset name, SHA-256, size and signature.
- Order of checks: size and SHA-256, then an [Ed25519 signature](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/src/component_store/signature.rs) with a built-in key, and only then quarantine removal and unpacking. Release builds refuse unsigned archives.
- Archives ([`ghostex-components` releases](https://github.com/maddada/ghostex-components/releases)):

| Platform | Compressed size |
|---|---|
| macOS arm64 | 146 MB |
| Windows x64 | 186 MB |
| Windows arm64 | 184 MB |
| Linux x64 (stripped) | 156 MB |
| Linux arm64 (stripped) | 158 MB |

- Windows delay-loads `libcef.dll`. Linux links it at load time, so a launcher installs it first and [reads the ELF version](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/src/cef_library_version.rs), because a mismatched library aborts the process.

**Borrowable (MIT):** `component_store/`, the optional-runtime state machine ([web_runtime.rs](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/src/app/helpers/web_runtime.rs)), [sign_in_popup.rs](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/src/cef/shell/sign_in_popup.rs), `devtools_relay.rs`, and the per-OS platform seam.

Not traced: how the code signature on the downloaded macOS framework is established.

### Mac-only shims ([`native/macos`](https://github.com/maddada/Ghostex/tree/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/desktop/native/macos), about 9,600 lines) [V]

| File | Lines | Purpose |
|---|---|---|
| GpuiCefAppKitHooks | 2,240 | CEF pump, native view frames, focus |
| GpuiMenuBarStatusItem | 1,706 | Status item |
| GpuiSettingsNotifications | 729 | Notifications, settings actions |
| GpuiSidebarReveal | 673 | Sidebar slide animation |
| GpuiSparkleUpdater | 585 | Updates |
| GpuiVoiceAudio | 493 | Audio |
| GpuiAppToastWindowChrome | 405 | Toast windows |
| GpuiLidSleepHelperClient (+ Swift helper) | 394 + 250 | Keep awake |
| GpuiGhostexCapture | 333 | Global hotkeys, screenshots |
| GpuiDocsDrawer | 264 | Drawer animation |
| GpuiReadAloud | 177 | Speech |
| GpuiAppIcon | 174 | Dock icon |
| GpuiPermissionGuide | 140 | Privacy pane helper, file drag out |
| GpuiAccessibilityDisplayOptions | 124 | Reduce motion and similar |
| GpuiKeyboardShortcuts | 114 | Layout translation |
| GpuiIntroVideoWebView | 109 | WKWebView for the intro video |
| GpuiChatDialogWindow, GpuiColorPanel | 95 each | Style mask focus fix, colour panel |
| GpuiNavigationGestures, GpuiOpenWith, GpuiWorkspacePowerEvents | 79, 70, 68 | Back/forward gestures, Open With, sleep/wake |
| GpuiCrashReports, GpuiStandardAboutPanel, GpuiTerminalMouseCursor | 36, 20, 14 | Small services |

Windows equivalents are in Rust: WinRT toasts, `windows_open_with.rs`, the colour dialog, and `wry` for the intro video. I found no tray code for Windows or Linux.

### Text, transcript and accessibility
- **Transcript [V]:** `gpui::list` with `ListState` and `FollowMode::Tail`, in `native_chat/` (66,149 lines). Markdown goes through `TextView`, with long code blocks capped and scrolled inside. Selection uses the fork's run registry.
- **Composer [V]:** forked `Textarea`/`Input` with inline pills and word-level undo.
- **Now upstream:** [`MessageScroller`](https://github.com/longbridge/gpui-kit/blob/main/crates/component/src/message_scroller.rs) is built on the same list, and selection works across virtualised rich content.
- **Accessibility [V]:** gpui-pre has `window/a11y.rs` and `_accessibility.rs`, with `accesskit_macos`, `accesskit_windows` and `accesskit_unix`. Nodes appear only with **a global id and a role**, and activate only on request from assistive tech.
- **Ghostex:** it has about 454 role or label lines in `native_chat` and doesn't call `new_inaccessible`, yet ComputerCub saw nothing (9 Nov notes). Unexplained; I didn't run it. Test this in the spike with ComputerCub early.
- **Cost:** GPUI Fast retains nothing while accessibility is on. [V, from a Ghostex comment]

### gpui-web [V]
[`apps/gpui-web`](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/gpui-web/README.md) symlinks the desktop's source into a wasm32 build (WebGPU, WebGL2 fallback). It replaced the React `apps/web`, deleted 24 Sep.
- **Works:** sidebar and actions, chat, terminal, dialogs, Quick Access.
- **Not drawn:** Code, Browser, Kanban, Automate and Docs tabs. Anything needing the OS answers with a toast.
- gpui-kit labels wasm "showcase only".

### Tests [V]
- 652 `#[test]` in 157 files, **zero `#[gpui::test]`**. Policy says "GPUI app tests are intentionally absent", and AGENTS.md says not to write tests unless asked.
- UI checks use the web build through [`shot.mjs`](https://github.com/maddada/Ghostex/blob/49b92f842ae7c5d752d476ae1ba7b4c6f695d311/apps/gpui-web/shot.mjs) (headless Chrome), the SpaceO virtual display, and `native-modal-demo` offscreen PNGs.
- Don't copy this. gpui-kit's harness is far better.

## Part 2: gpui-kit 0.9.0 and GPUI

**Versions on crates.io:** gpui-kit 0.9.0, gpui-pre 0.3.12 (0.3.11 compiled), gpui 0.2.2, [gpui-fast](https://github.com/longbridge/gpui-fast) 0.1.14, gpui-ce 0.3.0.

**Components [V].** The docs treat any page without a label as Stable. Only `speech`, `webview` and `mobile` are experimental; `title-bar`, `native-extension` and `system-notification` are platform-dependent.

| Need | Present | Notes |
|---|---|---|
| List, virtual list | Yes | `list/`, thin `virtual_list` |
| Table | Yes | DataTable, 4.8k lines |
| Tree, tabs | Yes | |
| Dock, resizable | Yes | Dock 3.8k lines |
| Markdown | Yes | `TextView`, also HTML and selection |
| Input, textarea, editor, code editor | Yes | 8.1k lines; LSP hooks, tree-sitter or syntect |
| Popover, menus, tooltip, dialog, sheet | Yes | |
| Toast | Yes | Can also post a system notification |
| Web view | **Separate crate, experimental** | Wraps `lb-wry` 0.53.3 |

**Headless tests [V]:** `#[gpui_kit::test]`, `TestAppContext`, element queries, simulated input, accessibility snapshots ([TESTING.md](https://github.com/longbridge/gpui-kit/blob/main/crates/kit/TESTING.md)). Pixel capture uses `render_to_image` on macOS (Metal) and Linux (WGPU). Windows only gets the portable suite.

**GPUI by platform [V]:**

| Feature | macOS | Windows | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| Runs | Yes | Yes (DirectX) | Yes | Yes |
| App menu | NSMenu | Stored, not native [I drawn by `AppMenuBar`] | Same | Same |
| Native popup menu | Yes | Yes | Drawn fallback, clipped to window | Same |
| Files dropped in | Yes | Yes | Yes | Yes |
| **Files dragged out** | Yes | **No** | **No** | Yes |
| IME | Yes | Yes | XIM | text-input |
| AccessKit | Yes | Yes | Yes | Yes |
| System notifications | Yes | Yes | Yes (D-Bus) | Yes |
| **Tray or status icon** | **No** | **No** | **No** | **No** |

## Part 3: showing a web page inside GPUI

| | CEF on first use | CEF bundled | wry |
|---|---|---|---|
| Added size | App stays small; 146 to 186 MB on request [V] | About the same added to every download [I]; about 320 MiB unpacked on Mac (our earlier browser research) | Near zero. [WebView2 ships with Windows 11; bootstrapper about 2 MB, fixed version over 250 MB](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution). WebKitGTK size not found |
| Engine | Same Chromium everywhere | Same | Three different engines |
| Google sign-in | **Worked on Mac in Ghostex** (Mike, 9 Nov, stock Alloy, real popups); Windows and Linux untested | Same | Risky. [Google's policy](https://developers.googleblog.com/en/upcoming-security-changes-to-googles-oauth-20-authorization-endpoint-in-embedded-webviews/) covers embedded webviews; WebView2 has [forum reports and user-agent hacks](https://kiwix.lema.org/stackoverflow.com_en_all_2023-11/questions/69088824/webview2-wpf-google-log-in); no data for WebKitGTK |
| Wayland | X11/XWayland only. [cef#2804](https://github.com/chromiumembedded/cef/issues/2804) still open, subsurface embedding commits linked Oct 2026, merge unconfirmed | Same | [gpui-webview errors on Wayland](https://github.com/longbridge/gpui-kit/blob/main/website/docs/webview.md) [V]. wry's Wayland path needs a GTK container [V], which a GPUI window doesn't have [I]; [no cross-client embedding protocol](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/issues/103) |
| GPUI overlays above page | No; needs windows or occlusion work, as Ghostex does | No | gpui-pre: page covers overlays and can't be clipped. GPUI Fast fixes macOS and Linux, **not Windows**. The Windows example disables DirectComposition |
| Cost | Security updates every few weeks, CHECK crashes in-process, no Chrome UI under Alloy | Same, plus every release is big | OS-maintained, but behaviour differs per OS |

For sign-in without any embedded engine, GPUI has `register_url_scheme` and `on_open_urls`, so a system browser hand-off is available. [V]

## What this suggests for the plan [my judgement]
1. Use gpui-kit's stock components and test harness first, and resist forking. Ghostex's two forks now need repeated re-merges.
2. Decide early whether Threadbear needs GPUI overlays over pages. If so, that forces GPUI Fast or Ghostex-style separate windows.
3. wry for simple previews, with optional CEF for the real browser pane, seems the best split. Copy Ghostex's download and signature design.
4. Linux with web panes means running on X11/XWayland for now.
5. Budget our own shims for the tray and for drag-out on Windows and X11.
6. Keep runtime checks in every slice. Ghostex's "source-only" stretch is the clearest warning.

Clones are left in `$TMPDIR` for follow-up: `ghostex-J82edU`, `gpui-kit-x2Zeu0`, `gx-zed-ioOJkz`.