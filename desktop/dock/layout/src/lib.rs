//! BearDock's layout tree and drag decisions, with no UI toolkit in sight, so
//! they can be tested fast and on their own. A line-for-line port of
//! `dock/Sources/BearDock/DockLayout.swift` (the tree) and the hit testing and
//! feel constants of `DockController.swift`. The GPUI views live in the
//! `threadbear-dock` crate, which re-exports this one.
pub mod controller;
pub mod geometry;
pub mod layout;

pub use controller::*;
pub use geometry::*;
pub use layout::*;
