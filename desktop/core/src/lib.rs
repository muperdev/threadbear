// What the desktop app knows with no window in sight, a port of the Swift
// app's ThreadbearCore: the inbox's rules and how a thread's events become
// rows. Keeping it out of the GPUI crate means its tests need no graphics
// libraries and take seconds.
pub mod composer;
pub mod folders;
pub mod highlights;
pub mod inbox;
pub mod logins;
pub mod navigation;
pub mod shortcuts;
pub mod new_thread;
pub mod open_policy;
pub mod remote;
pub mod sidebar;
pub mod slash;
pub mod transcript;
pub mod zoom;
pub use chrono;
pub mod reducer;
