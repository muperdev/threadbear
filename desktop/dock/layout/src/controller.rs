// The layout and everything about a drag in progress: where each group is
// drawn, which drop the pointer means, where a panel goes when it's let go.
// Ported from dock/Sources/BearDock/DockController.swift, minus what belongs to
// a UI toolkit (cursors, animation, windows); the view layer reads this state
// and does those. Everything here takes points in the dock's own coordinates,
// top-left origin, unless it says otherwise.
use crate::geometry::{Point, Rect, Size};
use crate::layout::*;
use std::collections::{HashMap, HashSet};

/// What the compass in the middle of a group offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompassItem {
    Tab,
    Split(DockEdge),
    Swap,
    /// Out of the dock into a window of its own.
    Window,
}

impl CompassItem {
    pub fn label(self) -> &'static str {
        match self {
            CompassItem::Tab => "Add as a tab",
            CompassItem::Swap => "Swap places",
            CompassItem::Window => "Open in a new window",
            CompassItem::Split(DockEdge::Left) => "Dock on the left",
            CompassItem::Split(DockEdge::Right) => "Dock on the right",
            CompassItem::Split(DockEdge::Top) => "Dock above",
            CompassItem::Split(DockEdge::Bottom) => "Dock below",
        }
    }
}

/// Where the pointer is pointing while a panel is being dragged.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DropTarget {
    /// Nil over the compass's gaps or a drop that would go nowhere.
    pub drop: Option<DockDrop>,
    /// The space the panel would take, in the dock's coordinates.
    pub preview: Rect,
    /// The compass, while the pointer is over the middle of a group.
    pub compass: Option<CompassState>,
    /// Set when letting go would open the panel in a window of its own:
    /// that window's frame on screen. `drop` is nil then.
    pub float: Option<Rect>,
    /// Over a group's bar: `preview` is the new tab's place in the bar,
    /// drawn as a tab, and the panel lands in `land` (the whole group).
    pub in_bar: bool,
    pub land: Option<Rect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompassState {
    pub group: String,
    pub center: Point,
    pub hovered: Option<CompassItem>,
    /// Items that would do nothing here (the panel's own group), drawn dimmed.
    pub disabled: HashSet<CompassItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DragState {
    pub panel: PanelId,
    pub source_group: String,
    /// The group's frame when the drag started.
    pub source_frame: Rect,
    /// Where in the panel it was picked up, 0...1 each way, so that point
    /// stays under the pointer as it shrinks. For a window's panel, an
    /// offset from the window's corner in points.
    pub grab: Point,
    pub location: Point,
    pub lifted: bool,
    pub target: Option<DropTarget>,
    /// A panel in a window of its own, dragged by its title: the window
    /// itself moves with the pointer, so there's no ghost, and it can be
    /// dropped back into the dock. The window's frame when it started.
    pub window: Option<Rect>,
}

/// The ghost flying to where the panel landed (or back home), after the drop.
#[derive(Clone, Debug, PartialEq)]
pub struct Landing {
    pub panel: PanelId,
    pub source_size: Size,
    pub to: Rect,
}

/// Which window a point is in, for `DockController::panel_at`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DockWindow<'a> {
    /// The dock's own window.
    Main,
    /// A panel's window of its own.
    Panel(&'a str),
    /// Anything else (a settings window, say).
    Other,
}

/// What a window's bar being dragged wants done to the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowDragUpdate {
    pub origin: Point,
    /// See-through over a place to dock, so the targets under it show.
    pub alpha: f32,
}

// Feel. Tuned by eye in the Swift demo, unchanged.
/// How far in from a group's side, as a share of its size, counts as that side.
pub const EDGE_BAND: f64 = 0.28;
/// Within this of the window's side, the drop spans the whole side.
pub const WINDOW_BAND: f64 = 22.;
/// The ghost's width once lifted, within limits.
pub const GHOST_WIDTH: f64 = 240.;
/// A new window takes the panel's size within these.
pub const WINDOW_MIN: Size = Size::new(320., 260.);
pub const WINDOW_MAX: Size = Size::new(760., 680.);
/// Over a bar, the window's top band is only this deep.
pub const WINDOW_TOP_OVER_BARS: f64 = 6.;
/// The width of the tab drawn in a bar for a drop there.
pub const BAR_TAB_WIDTH: f64 = 120.;
/// How long a press on a bar or a panel's background has to be held before
/// it picks the panel up (HoldDrag.hold).
pub const HOLD_SECONDS: f64 = 0.35;
/// How far a press moves before it counts as a drag (5 points, HoldDrag).
pub const DRAG_DISTANCE: f64 = 5.;

/// SwiftUI's spring(response:dampingFraction:), for the view layer to run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub response: f64,
    pub damping_fraction: f64,
}

pub const LIFT: Spring = Spring { response: 0.32, damping_fraction: 0.72 };
pub const SETTLE: Spring = Spring { response: 0.34, damping_fraction: 0.86 };
/// `Animation.snappy(duration: 0.16)`, for the target highlight moving.
pub const TRACK_SECONDS: f64 = 0.16;
/// How long a landed ghost stays before it fades (DockController.finish).
pub const LANDING_SECONDS: f64 = 0.3;
pub const LANDING_FADE_SECONDS: f64 = 0.12;

/// The compass's size and spacing.
pub const COMPASS_BUTTON: f64 = 36.;
pub const COMPASS_GAP: f64 = 6.;
pub const COMPASS_SWAP_WIDTH: f64 = 84.;
pub const COMPASS_SWAP_HEIGHT: f64 = 26.;
/// The strip you grab a divider by, and the smallest a panel gets.
pub const DIVIDER_STRIP: f64 = 14.;
pub const MIN_PANEL: f64 = 80.;
/// The ghost's tilt and the hole's inset (DragOverlay's Ghost, GroupView).
pub const GHOST_TILT_DEGREES: f64 = -1.2;
/// The opacity of a window being dragged over a place to dock.
pub const WINDOW_SEE_THROUGH: f32 = 0.5;

/// Offset of "new window" outlines from where the panel was, for the
/// menu's Open in New Window.
const NEW_WINDOW_OFFSET: f64 = 30.;

/// The layout and everything about a drag in progress. One per dock.
#[derive(Clone, Debug)]
pub struct DockController {
    pub layout: DockLayout,
    pub drag: Option<DragState>,
    pub landing: Option<Landing>,
    /// Each group's frame in the dock's coordinates, from the views.
    pub frames: HashMap<String, Rect>,
    /// Each group's bar, while it shows one, and where the empty part of it
    /// starts (after the title or tabs), for dropping a panel in as a tab.
    pub bars: HashMap<String, Rect>,
    pub bar_space_starts: HashMap<String, f64>,
    pub bounds: Rect,
    /// The window's whole content area in the dock's coordinates, when the
    /// window has more than the dock in it (an app's rail beside it, say).
    /// `None` means the window is exactly the dock.
    pub window_rect: Option<Rect>,
    /// Where the dock's (0, 0) is on screen, for frames of windows of their
    /// own. `None` until the view has been laid out.
    pub screen_origin: Option<Point>,
    /// The panel whose window just opened from a drag or the menu, brought
    /// to the front and made key, where windows restored at launch aren't.
    pub focus_next: Option<PanelId>,
    /// Set by Escape until the gesture that was cancelled ends.
    cancelled: bool,
}

impl DockController {
    pub fn new(layout: DockLayout) -> Self {
        Self {
            layout,
            drag: None,
            landing: None,
            frames: HashMap::new(),
            bars: HashMap::new(),
            bar_space_starts: HashMap::new(),
            bounds: Rect::ZERO,
            window_rect: None,
            screen_origin: None,
            focus_next: None,
            cancelled: false,
        }
    }

    // MARK: Dragging

    /// From a drag on a panel's title, tab or body (not a window's bar: that
    /// is `window_drag_changed`). Locations are in the dock's coordinates.
    pub fn drag_changed(&mut self, panel: &str, start: Point, location: Point) {
        // Escape put it back; the gesture keeps reporting until the button's up.
        if self.cancelled {
            return;
        }
        if self.layout.is_floating(panel) {
            return;
        }
        if self.drag.is_none() {
            if self.landing.is_some() {
                return;
            }
            let Some(group) = self.layout.group_containing(panel) else { return };
            let Some(frame) = self.frames.get(&group.id).copied() else { return };
            // A tab that isn't showing comes to the front as it's picked up.
            if group.selected != panel {
                self.layout.select(panel);
            }
            self.drag = Some(DragState {
                panel: panel.to_string(),
                source_group: group.id.clone(),
                source_frame: frame,
                grab: Point::new((start.x - frame.min_x()) / frame.width.max(1.), (start.y - frame.min_y()) / frame.height.max(1.)),
                location,
                lifted: true,
                target: None,
                window: None,
            });
        }
        if let Some(d) = &mut self.drag {
            d.location = location;
        }
        let mut target = self.target_for(panel, location);
        // Outside the window altogether, or on the compass's New Window: a
        // window of its own, there.
        let wants_window = target.as_ref().and_then(|t| t.compass.as_ref()).and_then(|c| c.hovered) == Some(CompassItem::Window)
            || (target.is_none() && self.is_outside_window(location));
        if wants_window && self.layout.can_float(panel) {
            if let Some(d) = self.drag.clone() {
                target = Some(DropTarget {
                    drop: None,
                    preview: Rect::ZERO,
                    compass: target.and_then(|t| t.compass),
                    float: self.float_frame(&d, location),
                    in_bar: false,
                    land: None,
                });
            }
        }
        if let Some(d) = &mut self.drag {
            d.target = target;
        }
    }

    /// A panel in a window of its own, dragged by its title: the window
    /// follows the pointer, and over the dock the drop targets show as for
    /// any other panel. `window_frame` is the window's frame on screen and
    /// `mouse` the pointer on screen. Says where to put the window.
    pub fn window_drag_changed(&mut self, panel: &str, window_frame: Rect, mouse: Point) -> Option<WindowDragUpdate> {
        if self.cancelled {
            return None;
        }
        if self.drag.is_none() {
            if self.landing.is_some() {
                return None;
            }
            self.drag = Some(DragState {
                panel: panel.to_string(),
                source_group: String::new(),
                source_frame: Rect::ZERO,
                // Kept as an offset from the window's corner, in points.
                grab: Point::new(mouse.x - window_frame.x, mouse.y - window_frame.y),
                location: Point::default(),
                lifted: true,
                target: None,
                window: Some(window_frame),
            });
        }
        let grab = self.drag.as_ref()?.grab;
        let origin = Point::new(mouse.x - grab.x, mouse.y - grab.y);
        let location = self.dock_point(mouse);
        if let (Some(d), Some(location)) = (&mut self.drag, location) {
            d.location = location;
        }
        let target = location.and_then(|p| self.target_for(panel, p));
        // See-through over a place to dock, so the targets under it show.
        let see_through = target.as_ref().is_some_and(|t| t.drop.is_some() || t.compass.is_some());
        if let Some(d) = &mut self.drag {
            d.target = target;
        }
        Some(WindowDragUpdate { origin, alpha: if see_through { WINDOW_SEE_THROUGH } else { 1. } })
    }

    /// The panel was let go. Returns `true` if the view should reset a
    /// window-drag: see `drag`.
    pub fn drag_ended(&mut self) {
        if self.cancelled {
            self.cancelled = false;
            return;
        }
        let Some(d) = self.drag.clone() else { return };
        if d.window.is_some() {
            self.finish_window_drag(&d, false, None);
        } else if let Some(frame) = d.target.as_ref().and_then(|t| t.float) {
            self.focus_next = Some(d.panel.clone());
            self.drag = None;
            let (home, share) = self.home_of(&d.panel);
            self.layout.float(&d.panel, frame, home, share);
        } else {
            let drop = d.target.as_ref().and_then(|t| t.drop.clone());
            self.finish(&d, drop);
        }
    }

    /// Escape, mid-drag: the panel flies home and the rest of the gesture is
    /// ignored. Returns the frame to move a window's panel back to, if it
    /// was a window being dragged.
    pub fn cancel_drag(&mut self, window_frame_now: Option<Rect>) -> Option<Rect> {
        let d = self.drag.clone()?;
        self.cancelled = true;
        if d.window.is_some() {
            self.finish_window_drag(&d, true, window_frame_now);
            d.window
        } else {
            self.finish(&d, None);
            None
        }
    }

    /// Lets go of a window's panel: into the dock where it's pointing, else
    /// the window stays where it was dragged (or goes back, after Escape).
    /// `frame_now` is the window's current frame.
    fn finish_window_drag(&mut self, d: &DragState, cancelled: bool, frame_now: Option<Rect>) {
        self.drag = None;
        match d.target.as_ref().and_then(|t| t.drop.clone()) {
            Some(drop) if !cancelled => self.layout.move_panel(&d.panel, drop),
            _ => {
                if let Some(frame) = frame_now {
                    self.layout.float(&d.panel, frame, None, None);
                }
            }
        }
    }

    /// The window drag ended with a real frame to remember (the view calls
    /// this after `drag_ended` when the window stayed out of the dock).
    pub fn remember_window_frame(&mut self, panel: &str, frame: Rect) {
        if self.layout.is_floating(panel) {
            self.layout.float(panel, frame, None, None);
        }
    }

    fn finish(&mut self, d: &DragState, drop: Option<DockDrop>) {
        let size = d.source_frame.size();
        let to = d.target.as_ref().map(|t| t.land.unwrap_or(t.preview));
        match (drop, to) {
            (Some(drop), Some(to)) => {
                self.landing = Some(Landing { panel: d.panel.clone(), source_size: size, to });
                self.drag = None;
                self.layout.move_panel(&d.panel, drop);
            }
            _ => {
                // Nowhere to go, or cancelled: fly home.
                let to = self.frames.get(&d.source_group).copied().unwrap_or(d.source_frame);
                self.landing = Some(Landing { panel: d.panel.clone(), source_size: size, to });
                self.drag = None;
            }
        }
    }

    /// Called by the view when the ghost has finished its flight.
    pub fn clear_landing(&mut self) {
        self.landing = None;
    }

    /// A panel's group's frame in the dock, for tests and the demo's
    /// frozen drags.
    pub fn frame_of(&self, panel: &str) -> Option<Rect> {
        self.layout.group_containing(panel).and_then(|g| self.frames.get(&g.id).copied())
    }

    /// The panel showing at `point` (in `window`'s coordinates, top-left
    /// origin): the front panel of the group there, or the panel a window
    /// of its own holds. Threadbear's activity log says where a click was
    /// with it.
    pub fn panel_at(&self, point: Point, window: DockWindow<'_>) -> Option<PanelId> {
        match window {
            DockWindow::Panel(own) => Some(own.to_string()),
            DockWindow::Other => None,
            DockWindow::Main => {
                // Window coordinates into the dock's. `window_rect` is the
                // window in the dock's coordinates (origin is usually 0).
                let offset = self.window_rect.map(|r| r.origin()).unwrap_or_default();
                let p = Point::new(point.x + offset.x, point.y + offset.y);
                self.layout.groups().into_iter().find(|g| self.frames.get(&g.id).is_some_and(|f| f.contains(p))).map(|g| g.selected)
            }
        }
    }

    // MARK: Windows of their own

    /// Right-click, Open in New Window: the panel in a window of its own,
    /// just down and to the right of where it was.
    pub fn open_in_window(&mut self, panel: &str) {
        if !self.layout.can_float(panel) {
            return;
        }
        let Some(frame) = self.frame_of(panel) else { return };
        let size = Self::window_size_for(frame.size());
        let at = Rect::new(frame.min_x() + NEW_WINDOW_OFFSET, frame.min_y() + NEW_WINDOW_OFFSET, size.width, size.height);
        let Some(screen) = self.screen_rect(at) else { return };
        self.focus_next = Some(panel.to_string());
        let (home, share) = self.home_of(panel);
        self.layout.float(panel, screen, home, share);
    }

    /// A panel that isn't in the layout, straight into a window of its own
    /// in the middle of the dock's window, at `size` within the usual
    /// limits. Put Back docks it down the right. One that's in the layout
    /// already comes to the front instead (Threadbear's show_panel, when an
    /// agent asks for a window).
    pub fn open_in_new_window(&mut self, panel: &str, size: Size) {
        if self.layout.contains(panel) {
            self.show(panel, &[], &DockDrop::window(DockEdge::Right));
            return;
        }
        let s = Self::window_size_for(size);
        let rect = Rect::new(self.bounds.mid_x() - s.width / 2., self.bounds.mid_y() - s.height / 2., s.width, s.height);
        // Not on screen yet (no window to measure from): docked will do.
        if self.bounds.width <= 0. {
            self.open(panel, DockDrop::window(DockEdge::Right));
            return;
        }
        let Some(screen) = self.screen_rect(rect) else {
            self.open(panel, DockDrop::window(DockEdge::Right));
            return;
        };
        let mut next = self.layout.clone();
        next.add(panel, DockDrop::window(DockEdge::Right));
        next.float(panel, screen, Some(DockDrop::window(DockEdge::Right)), None);
        self.focus_next = Some(panel.to_string());
        self.layout = next;
    }

    /// From a window of its own back into the dock, where it came from if
    /// that's still there.
    pub fn put_back(&mut self, panel: &str) {
        let Some(float) = self.layout.floating.iter().find(|f| f.panel == panel).cloned() else { return };
        let drop = float.home.filter(|h| self.layout.can_drop(panel, h)).unwrap_or(DockDrop::window(DockEdge::Right));
        self.layout.move_panel(panel, drop);
    }

    /// Where Put Back docks a panel: as a tab of the group it's leaving if
    /// that has others, else along the side of the window it's nearest, at
    /// the share of the window it has now.
    pub fn home_of(&self, panel: &str) -> (Option<DockDrop>, Option<f64>) {
        let Some(group) = self.layout.group_containing(panel) else { return (None, None) };
        if group.panels.len() > 1 {
            return (Some(DockDrop::tab(group.id)), None);
        }
        let Some(f) = self.frames.get(&group.id).copied() else { return (None, None) };
        if self.bounds.width <= 0. || self.bounds.height <= 0. {
            return (None, None);
        }
        let b = self.bounds;
        let sides = [
            (DockEdge::Left, f.mid_x() - b.min_x()),
            (DockEdge::Right, b.max_x() - f.mid_x()),
            (DockEdge::Top, f.mid_y() - b.min_y()),
            (DockEdge::Bottom, b.max_y() - f.mid_y()),
        ];
        let edge = sides.iter().min_by(|a, b| a.1.total_cmp(&b.1)).expect("four sides").0;
        let share = if edge.axis() == SplitAxis::Horizontal { f.width / b.width } else { f.height / b.height };
        (Some(DockDrop::window(edge)), Some(share))
    }

    pub fn window_size_for(size: Size) -> Size {
        Size::new(size.width.max(WINDOW_MIN.width).min(WINDOW_MAX.width), size.height.max(WINDOW_MIN.height).min(WINDOW_MAX.height))
    }

    /// The window a dragged panel would open as, on screen: its own size
    /// (within limits), with the spot it was picked up by under the pointer.
    pub fn float_frame(&self, d: &DragState, p: Point) -> Option<Rect> {
        let size = Self::window_size_for(d.source_frame.size());
        let rect = Rect::new(p.x - d.grab.x * size.width, p.y - d.grab.y * size.height, size.width, size.height);
        self.screen_rect(rect)
    }

    /// A rect in the dock's coordinates, on screen.
    pub fn screen_rect(&self, rect: Rect) -> Option<Rect> {
        self.screen_origin.map(|o| rect.offset(o))
    }

    /// A point on screen in the dock's coordinates.
    pub fn dock_point(&self, p: Point) -> Option<Point> {
        self.screen_origin.map(|o| Point::new(p.x - o.x, p.y - o.y))
    }

    /// Past the edge of the window the dock's in, not just the dock (the
    /// app may have more beside it, like Threadbear's rail).
    pub fn is_outside_window(&self, p: Point) -> bool {
        !self.window_rect.unwrap_or(self.bounds).contains(p)
    }

    // MARK: Other changes

    pub fn close(&mut self, panel: &str) {
        self.layout.close(panel);
    }

    pub fn set_title_hidden(&mut self, panel: &str, hidden: bool) {
        self.layout.set_title_hidden(panel, hidden);
    }

    pub fn select(&mut self, panel: &str) {
        self.layout.select(panel);
    }

    /// In the layout and the tab showing in its group, or in a window of its own.
    pub fn is_showing(&self, panel: &str) -> bool {
        self.layout.is_floating(panel) || self.layout.group_containing(panel).is_some_and(|g| g.selected == panel)
    }

    /// Brings a panel up: its tab if it's in the layout, else opened as a
    /// tab beside the first of `near` that's open, else at `fallback`.
    pub fn show(&mut self, panel: &str, near: &[&str], fallback: &DockDrop) {
        if self.layout.is_floating(panel) {
            // The view brings the window forward (DockView::sync_windows).
            self.focus_next = Some(panel.to_string());
            return;
        }
        if let Some(group) = self.layout.group_containing(panel) {
            if group.selected != panel {
                self.layout.select(panel);
            }
            return;
        }
        let drop = near.iter().find_map(|p| self.layout.group_containing(p)).map(|g| DockDrop::tab(g.id)).unwrap_or_else(|| fallback.clone());
        self.open(panel, drop);
    }

    /// A rail button, VS Code's way: closes a panel that's showing, brings
    /// up one that's open behind another tab, opens one that isn't.
    pub fn toggle(&mut self, panel: &str, near: &[&str], fallback: &DockDrop) {
        if self.is_showing(panel) {
            self.close(panel);
        } else {
            self.show(panel, near, fallback);
        }
    }

    /// Opens a panel that isn't in the layout, along the right of the window
    /// by default.
    pub fn open(&mut self, panel: &str, drop: DockDrop) {
        self.layout.add(panel, drop);
    }

    pub fn set_layout(&mut self, layout: DockLayout) {
        self.layout = layout;
    }

    // MARK: Hit testing

    pub fn target_for(&self, panel: &str, p: Point) -> Option<DropTarget> {
        if self.is_outside_window(p) {
            return None;
        }
        // A group's bar: join it as a tab, shown as a tab in the bar (Mike,
        // 28 Oct). Over the window's top band too, which runs along the top
        // row's bars, except its outermost few points, so docking along the
        // whole top of the window is still there.
        if p.y - self.bounds.min_y() >= WINDOW_TOP_OVER_BARS {
            if let Some(target) = self.bar_target(panel, p) {
                return Some(target);
            }
        }
        // The window's own sides first: a thin strip all along each.
        if self.bounds.width > 0. {
            if let Some(edge) = self.window_edge(p) {
                let drop = DockDrop::window(edge);
                if self.layout.can_drop(panel, &drop) {
                    return Some(DropTarget {
                        preview: Self::slice(self.bounds, edge, 0.25),
                        drop: Some(drop),
                        ..DropTarget::default()
                    });
                }
            }
        }
        let group = self.layout.groups().into_iter().find(|g| self.frames.get(&g.id).is_some_and(|f| f.contains(p)))?;
        let frame = *self.frames.get(&group.id)?;

        let center = Point::new(frame.mid_x(), frame.mid_y());
        let items = Self::compass_items(center);
        let compass_area = union_all(items.iter().map(|(_, r)| *r)).inset(-14., -14.);

        // The sides of the group, unless the pointer is on the compass.
        if !compass_area.contains(p) {
            let fx = (p.x - frame.min_x()) / frame.width;
            let fy = (p.y - frame.min_y()) / frame.height;
            let sides = [(DockEdge::Left, fx), (DockEdge::Right, 1. - fx), (DockEdge::Top, fy), (DockEdge::Bottom, 1. - fy)];
            let (edge, distance) = sides.iter().copied().min_by(|a, b| a.1.total_cmp(&b.1)).expect("four sides");
            if distance < EDGE_BAND {
                let drop = DockDrop::edge(group.id.clone(), edge);
                return Some(DropTarget {
                    preview: Self::slice(frame, edge, 0.5),
                    drop: self.layout.can_drop(panel, &drop).then_some(drop),
                    ..DropTarget::default()
                });
            }
        }

        // The middle: the compass, and whatever it's on (a tab by default).
        let disabled: HashSet<CompassItem> = items
            .iter()
            .map(|(item, _)| *item)
            .filter(|item| {
                if *item == CompassItem::Window {
                    return !self.layout.can_float(panel);
                }
                !self.layout.can_drop(panel, &Self::drop_for(*item, &group.id))
            })
            .collect();
        let hovered = items.iter().find(|(_, r)| r.contains(p)).map(|(item, _)| *item);
        let item = hovered.unwrap_or(CompassItem::Tab);
        // New window isn't a place in the dock: drag_changed works out its frame.
        let drop = if disabled.contains(&item) || item == CompassItem::Window { None } else { Some(Self::drop_for(item, &group.id)) };
        let preview = match item {
            CompassItem::Split(edge) => Self::slice(frame, edge, 0.5),
            CompassItem::Tab | CompassItem::Swap | CompassItem::Window => frame,
        };
        Some(DropTarget {
            drop,
            preview,
            compass: Some(CompassState { group: group.id.clone(), center, hovered, disabled }),
            ..DropTarget::default()
        })
    }

    fn bar_target(&self, panel: &str, p: Point) -> Option<DropTarget> {
        let group = self.layout.groups().into_iter().find(|g| self.layout.shows_header(g) && self.bars.get(&g.id).is_some_and(|b| b.contains(p)))?;
        let bar = *self.bars.get(&group.id)?;
        let frame = *self.frames.get(&group.id)?;
        let drop = DockDrop::tab(group.id.clone());
        // After the titles, or as far along as there's room for.
        let start = self.bar_space_starts.get(&group.id).copied().unwrap_or(bar.min_x());
        let width = BAR_TAB_WIDTH.min((bar.width - 16.).max(40.));
        let x = (bar.min_x() + 4.).max((start + 4.).min(bar.max_x() - width - 4.));
        let tab = Rect::new(x, bar.min_y() + 4., width, (bar.height - 8.).max(12.));
        Some(DropTarget {
            preview: tab,
            drop: self.layout.can_drop(panel, &drop).then_some(drop),
            in_bar: true,
            land: Some(frame),
            ..DropTarget::default()
        })
    }

    fn window_edge(&self, p: Point) -> Option<DockEdge> {
        let b = WINDOW_BAND;
        let bounds = self.bounds;
        if p.x - bounds.min_x() < b {
            return Some(DockEdge::Left);
        }
        if bounds.max_x() - p.x < b {
            return Some(DockEdge::Right);
        }
        if p.y - bounds.min_y() < b {
            return Some(DockEdge::Top);
        }
        if bounds.max_y() - p.y < b {
            return Some(DockEdge::Bottom);
        }
        None
    }

    pub fn drop_for(item: CompassItem, group: &str) -> DockDrop {
        match item {
            CompassItem::Tab | CompassItem::Window => DockDrop::tab(group),
            CompassItem::Swap => DockDrop::swap(group),
            CompassItem::Split(edge) => DockDrop::edge(group, edge),
        }
    }

    /// The compass's buttons around a group's centre: a cross of five, and
    /// Swap and New Window side by side under it.
    pub fn compass_items(c: Point) -> Vec<(CompassItem, Rect)> {
        let s = COMPASS_BUTTON;
        let step = COMPASS_BUTTON + COMPASS_GAP;
        let at = |dx: f64, dy: f64| Rect::new(c.x + dx * step - s / 2., c.y + dy * step - s / 2., s, s);
        let y = c.y + 2. * step - s / 2. + 6.;
        let swap = Rect::new(c.x - COMPASS_SWAP_WIDTH - COMPASS_GAP / 2., y, COMPASS_SWAP_WIDTH, COMPASS_SWAP_HEIGHT);
        let window = Rect::new(c.x + COMPASS_GAP / 2., y, COMPASS_SWAP_WIDTH, COMPASS_SWAP_HEIGHT);
        vec![
            (CompassItem::Tab, at(0., 0.)),
            (CompassItem::Split(DockEdge::Top), at(0., -1.)),
            (CompassItem::Split(DockEdge::Bottom), at(0., 1.)),
            (CompassItem::Split(DockEdge::Left), at(-1., 0.)),
            (CompassItem::Split(DockEdge::Right), at(1., 0.)),
            (CompassItem::Swap, swap),
            (CompassItem::Window, window),
        ]
    }

    /// The share of a rect along one side.
    pub fn slice(r: Rect, edge: DockEdge, share: f64) -> Rect {
        match edge {
            DockEdge::Left => Rect::new(r.min_x(), r.min_y(), r.width * share, r.height),
            DockEdge::Right => Rect::new(r.max_x() - r.width * share, r.min_y(), r.width * share, r.height),
            DockEdge::Top => Rect::new(r.min_x(), r.min_y(), r.width, r.height * share),
            DockEdge::Bottom => Rect::new(r.min_x(), r.max_y() - r.height * share, r.width, r.height * share),
        }
    }

    /// The ghost's scale once lifted: to about `GHOST_WIDTH` wide, within limits.
    pub fn ghost_scale(size: Size) -> f64 {
        (GHOST_WIDTH / size.width.max(1.)).max(0.3).min(0.85)
    }

    /// The ghost's rect while lifted: shrunk, with the grab point under the
    /// pointer. `lift` runs from 0 (at the panel's own size) to 1 (shrunk), a
    /// spring's value may overshoot past it.
    pub fn ghost_rect(d: &DragState, lift: f64) -> Rect {
        let size = d.source_frame.size();
        let scale = 1. + (Self::ghost_scale(size) - 1.) * lift;
        let w = size.width * scale;
        let h = size.height * scale;
        Rect::new(d.location.x - d.grab.x * w, d.location.y - d.grab.y * h, w, h)
    }
}

fn union_all(rects: impl IntoIterator<Item = Rect>) -> Rect {
    let mut iter = rects.into_iter();
    let first = iter.next().unwrap_or(Rect::ZERO);
    iter.fold(first, |acc, r| acc.union(&r))
}

// MARK: How a tree is drawn. GroupView and SplitView's static helpers.

/// The group as drawn: without its unavailable panels, showing the selected
/// one if it's available, else the first that is. All of them, if none is.
pub fn visible_group(group: &DockGroup, available: &dyn Fn(&str) -> bool) -> DockGroup {
    let panels: Vec<PanelId> = group.panels.iter().filter(|p| available(p)).cloned().collect();
    if panels.is_empty() {
        return group.clone();
    }
    let selected = if panels.contains(&group.selected) { group.selected.clone() } else { panels[0].clone() };
    DockGroup { id: group.id.clone(), panels, selected }
}

/// The group in the window's top-leading corner as drawn, where the
/// traffic lights sit: a side left out for having nothing to show passes
/// the corner on to the next.
pub fn top_leading_group_id(root: &DockNode, available: &dyn Fn(&str) -> bool) -> String {
    let mut node = root;
    while let DockNode::Split(s) = node {
        match shown_indices(s, available).first() {
            Some(first) => node = &s.children[*first],
            None => break,
        }
    }
    node.id().to_string()
}

/// The children that have a panel with something to show, by index. All
/// of them if none has, as a group shows all its tabs then.
pub fn shown_indices(split: &DockSplit, available: &dyn Fn(&str) -> bool) -> Vec<usize> {
    let shown: Vec<usize> = (0..split.children.len()).filter(|i| has_available(&split.children[*i], available)).collect();
    if shown.is_empty() {
        (0..split.children.len()).collect()
    } else {
        shown
    }
}

pub fn has_available(node: &DockNode, available: &dyn Fn(&str) -> bool) -> bool {
    match node {
        DockNode::Group(g) => g.panels.iter().any(|p| available(p)),
        DockNode::Split(s) => s.children.iter().any(|c| has_available(c, available)),
    }
}

/// `SplitView.normalized`: shares that sum to 1 (equal if they don't sum to more than 0).
pub fn normalized(f: &[f64]) -> Vec<f64> {
    let total: f64 = f.iter().sum();
    if total > 0. {
        f.iter().map(|x| x / total).collect()
    } else {
        vec![1. / f.len().max(1) as f64; f.len()]
    }
}

/// Fractions of the shown children (summing to 1), put back into the whole
/// split's: the hidden ones keep theirs, the shown ones share what's left
/// in the new proportions.
pub fn merged(visible: &[f64], all: &[f64], shown: &[usize]) -> Vec<f64> {
    let mut out = all.to_vec();
    let room: f64 = shown.iter().map(|&i| all[i]).sum();
    for (k, &i) in shown.iter().enumerate() {
        out[i] = visible[k] * room;
    }
    out
}

/// Moves the boundary after child i, keeping both sides at least `min`.
pub fn resized(f: &[f64], i: usize, delta: f64, min: f64) -> Vec<f64> {
    let mut f = f.to_vec();
    let pair = f[i] + f[i + 1];
    let lo = min;
    let hi = pair - min;
    if !(lo < hi) {
        return f;
    }
    f[i] = (f[i] + delta).max(lo).min(hi);
    f[i + 1] = pair - f[i];
    f
}
