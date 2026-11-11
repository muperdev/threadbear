# BearDock for GPUI

The dockable, rearrangeable panel layout Threadbear's window is built on, as a
GPUI library: panels side by side, stacked as tabs, or torn out into windows of
their own, with the arrangement saved as plain JSON. A port of the Swift
library in [`dock/`](../../dock); that package is still the definition of right,
and this crate keeps its names (`DockLayout`, `DockController`, `DockDrop` ...)
and feel constants.

## Two crates

- **`layout/` (`threadbear-dock-layout`)**: the tree and the drag decisions, with
  no UI toolkit. `cargo test -p threadbear-dock-layout` runs its 32 tests (Swift's
  31 under their own names, plus one for the saved-layout JSON) in a moment, so
  it's safe on any machine. Its JSON is Swift's Codable shape, so a layout the
  Mac app saved still loads.
- **`threadbear-dock` (here)**: the GPUI views: splits and dividers, tab groups,
  bars, Hide Title, the right-click menu, the press rules, the ghost, drop
  targets, compass and torn-out windows. It re-exports the layout crate.

Nothing here is Threadbear-specific. An app says what each panel shows through
the `DockContent` trait, the same seam as Swift's `DockContent`.

## Using it

```rust
let model = cx.new(|_| {
    let mut m = DockModel::new(saved_layout_or_default);
    m.on_change(|layout, _cx| save_it(layout));
    m
});
let dock = cx.new(|cx| DockView::new(model.clone(), Rc::new(MyPanels), DockOptions::default(), cx));
// Render `dock` as (part of) the window. Change the layout from anywhere with
// model.update(cx, |m, cx| m.toggle("highlights", &[], DockDrop::window(DockEdge::Right), cx));
```

`DockModel` has the controller's commands (`show`, `toggle`, `open`, `close`,
`select`, `set_title_hidden`, `put_back`, `open_in_window`, `open_in_new_window`,
`set_layout`), and `model.read(cx).layout().summary()` is the one-line dump.
`DockView::set_options` changes the bars' size afterwards (the app's zoom does).

A window hosting the dock must have no title bar over it and let the app own
title bar drags, or AppKit moves the window on a drag of a panel's bar before
the panel gets it (BearDock's DockAnchor exists for the same reason):

```rust
WindowOptions {
    titlebar: Some(TitlebarOptions { appears_transparent: true, ..Default::default() }),
    app_owns_titlebar_drag: true,
    ..Default::default()
}
```

Panel content wins over the dock's drag by stopping the press:
`.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())`, or
`dock_drag_disabled` for a region with no drag of its own.

## The demo

```bash
cargo run -p threadbear-dock --bin dock-demo     # the layout is saved in ~/.beardock-demo-layout.json
BEARDOCK_APPEARANCE=light cargo run -p threadbear-dock --bin dock-demo
```

With the `automation` feature (`--features automation`, never in a release) the
demo takes `BEARDOCK_EVENTS`: real GPUI events through real hit testing,
screenshots of the window, and the other steps in `src/bin/dock-demo/script.rs`.
`shots.sh` runs that on a machine's desktop (`desktop/scripts/remote.sh tinker-desk
--desk -- bash desktop/dock/shots.sh <name> dark '<events>'`), and `pointer-test.sh`
drives the real pointer and Escape key with CGEvents and records a film strip.

Tests: `cargo test -p threadbear-dock-layout`; and through the real views, headless:
`cargo test -p threadbear-dock --features automation --test press` (on a build
machine, not Mike's Mac).

## How it differs from the Swift version

- **The ghost.** GPUI can't transform a subtree of elements, so the ghost can't be
  the real panel scaled and tilted the way SwiftUI does it. It's a second live
  copy of the panel (`view(panel, ghost = true, ...)`): what's sized in rems
  shrinks with it (`with_rem_size`), what's in pixels stays its size and is
  clipped, and only the plate behind it tilts (a rotated path; content stays
  level). A panel can draw something simpler when `ghost` is true.
- **Springs** run on GPUI's frames from elapsed time (`SpringConfig`'s closed
  form), so a busy main thread drops frames instead of running slowly.
- **Torn-out windows** need the OS: the Mac moves and fades them with AppKit
  (`src/platform.rs`), and the outline window follows the pointer past the
  window's edge. On Windows and X11 a window's title bar drag falls back to the
  OS's own window drag, without drop targets (the panel can still go back with
  Put Back or by the compass from the main window). On Wayland apps can't read
  the pointer outside their windows or place them, so the compass's Window
  button and the Put Back menu command are the way there.
- A panel with nothing to show gives up its room in its group or split as in
  Swift, but a torn-out window stays showing rather than hiding (GPUI has no
  hide).
