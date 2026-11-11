//! BearDock for GPUI: dockable, rearrangeable panels, like the Swift original in
//! `dock/` (and it, not this, defines what's right; keep the two in step). The
//! window's panels can sit side by side, stack as tabs, or be torn out into
//! windows of their own, and the arrangement is plain data the app can save.
//!
//! The tree and the drag decisions are in the UI-free
//! [`threadbear_dock_layout`] crate, which this one re-exports. What's here is
//! the GPUI on top: [`DockView`] draws the tree and handles the pointer, and the
//! app says what each panel shows through [`DockContent`].
pub mod content;
pub mod model;
pub mod overlay;
pub mod platform;
pub mod theme;
pub mod view;
pub mod windows;

use gpui_kit::{InteractiveElement, MouseButton};

/// Keeps a region of a panel from picking the panel up when dragged, for
/// content with no drag of its own (a canvas, a read-only list) where moving
/// the panel would surprise; the header still moves it. Any mouse-down that
/// stops propagation does the same, and is how a panel's own drags win over the
/// dock's. Swift's `.dockDragDisabled()`.
pub fn dock_drag_disabled<E: InteractiveElement + Sized>(element: E) -> E {
    element.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub use content::{DockContent, DockOptions, SharedContent};
pub use model::DockModel;
pub use threadbear_dock_layout as layout;
pub use threadbear_dock_layout::*;
pub use view::DockView;
