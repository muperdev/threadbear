// The dock's window: the tree of splits and groups, their bars and tabs, and
// the press rules that turn a mouse down on a bar, tab or body into a panel
// drag. Ported from dock/Sources/BearDock/DockView.swift (its comments say why
// each rule is how it is); DragOverlay.swift is in overlay.rs, the torn-out
// windows in windows.rs.
use crate::content::{DockOptions, SharedContent};
use crate::model::DockModel;
use crate::overlay::{self, LandingRun, SpringRun, Tween};
use crate::platform;
use crate::theme::{DockTheme, GAP};
use crate::windows::FloatingWindows;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};
use threadbear_dock_layout as dl;
use threadbear_dock_layout::{DockGroup, DockLayout, DockNode, DockSplit, PanelId, SplitAxis};

/// How a press in some place behaves (Swift's HoldDrag arguments).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PressKind {
    /// On a bar: a plain drag moves the window, when the options allow.
    pub(crate) moves_window: bool,
    /// The hold is timed from the press and lifts while you hold (a bar),
    /// rather than being checked 5 points in (everywhere else).
    pub(crate) from_press: bool,
    /// Picks the panel up as soon as it moves, with no hold: a title or tab,
    /// which is plainly for moving the panel (Mike, 28 Oct).
    pub(crate) needs_hold: bool,
}

pub(crate) const BAR_PRESS: PressKind = PressKind { moves_window: true, from_press: true, needs_hold: true };
pub(crate) const TAB_PRESS: PressKind = PressKind { moves_window: true, from_press: false, needs_hold: false };
pub(crate) const BODY_PRESS: PressKind = PressKind { moves_window: false, from_press: false, needs_hold: true };

/// Waiting for the hold or the first 5 points of movement; then which of the
/// three it became. Swift's HoldDrag.Mode.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum PressMode {
    Waiting,
    /// The OS is moving the window; we've handed the press over.
    Window,
    Nothing,
    Panel,
}

pub(crate) struct Press {
    panel: PanelId,
    kind: PressKind,
    start: dl::Point,
    location: dl::Point,
    pressed_at: Instant,
    mode: PressMode,
    /// For moving the window by hand if this press turns out to do that.
    grab: Option<platform::WindowGrab>,
    /// The hold's timer, for a press that starts on a bar.
    _timer: Option<Task<()>>,
}

/// A divider being dragged: the fractions when it started.
struct DividerDrag {
    split: String,
    index: usize,
    axis: SplitAxis,
    start: f64,
    start_fractions: Vec<f64>,
    shown: Vec<usize>,
    available: f64,
}

/// The whole docking area. Create it with [`DockView::new`] and make it (or a
/// view holding it) a window's root; it fills what it's given.
pub struct DockView {
    pub model: Entity<DockModel>,
    pub(crate) content: SharedContent,
    pub(crate) options: Rc<DockOptions>,
    /// Where the dock is in the window, set each frame.
    root_bounds: Rc<Cell<Bounds<Pixels>>>,
    press: Option<Press>,
    divider: Option<DividerDrag>,
    /// A tab that was dragged doesn't also select on its mouse-up.
    suppress_click: bool,
    /// The last press moved the window or picked up a panel: see `press_moved`.
    press_moved: bool,
    /// Escape while dragging, whatever has focus. Only while dragging, so the
    /// key reaches the app the rest of the time.
    escape_watch: Option<Subscription>,
    /// Each split's length along its axis, from the last frame, for the
    /// dividers (the 80 pt minimum is in points).
    split_lengths: Rc<RefCell<HashMap<String, f64>>>,
    // The drag's animations: SwiftUI gets these from withAnimation; here they
    // run on GPUI's frames, driven by time, not frame counts.
    pub(crate) lift: Option<SpringRun>,
    pub(crate) landing: Option<LandingRun>,
    pub(crate) preview: Option<Tween>,
    pub(crate) last_preview: Option<dl::Rect>,
    pub(crate) windows: FloatingWindows,
    _subscriptions: Vec<Subscription>,
}

impl DockView {
    /// A dock for `layout`. It redraws itself when the model changes, and
    /// opens and closes the torn-out windows the layout says it has.
    pub fn new(model: Entity<DockModel>, content: SharedContent, options: DockOptions, cx: &mut Context<Self>) -> Self {
        let observed = cx.observe(&model, |this, _, cx| {
            // Any window of the dock can change it (a torn-out window's bar
            // drives a drag from over there): this window follows.
            this.after_drag_update(cx);
            this.sync_windows(cx);
            cx.notify();
        });
        // Windows saved open reopen once the app's up; the model changing
        // does the same for ones that open or close later.
        let view = cx.entity();
        cx.defer(move |cx| view.update(cx, |this, cx| this.sync_windows(cx)));
        Self {
            model,
            content,
            options: Rc::new(options),
            root_bounds: Rc::new(Cell::new(Bounds::default())),
            press: None,
            divider: None,
            suppress_click: false,
            press_moved: false,
            escape_watch: None,
            split_lengths: Rc::new(RefCell::new(HashMap::new())),
            lift: None,
            landing: None,
            preview: None,
            last_preview: None,
            windows: FloatingWindows::default(),
            _subscriptions: vec![observed],
        }
    }

    /// Whether the last press on a bar moved the window or picked up a panel.
    /// Something in a bar that acts on a click (the thread's title renames)
    /// checks this from its click handler, which runs on the same release,
    /// after the dock has seen it: with the window following the pointer, the
    /// release lands on the same spot and looks like a click otherwise.
    pub fn press_moved(&self) -> bool {
        self.press_moved
    }

    /// Applies new options (the bars' size under the app's zoom, say).
    pub fn set_options(&mut self, options: DockOptions, cx: &mut Context<Self>) {
        self.options = Rc::new(options);
        cx.notify();
    }

    /// The dock's position in its window.
    fn dock_point(&self, position: Point<Pixels>) -> dl::Point {
        let origin = self.root_bounds.get().origin;
        dl::Point::new((position.x - origin.x).as_f32() as f64, (position.y - origin.y).as_f32() as f64)
    }

    fn drags_window(&self, kind: PressKind, panel: &str, cx: &App) -> bool {
        kind.moves_window && (self.options.bars_move_window || self.model.read(cx).controller.layout.is_floating(panel))
    }

    fn begin_press(&mut self, panel: PanelId, kind: PressKind, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.press.is_some() {
            return;
        }
        let start = self.dock_point(event.position);
        self.suppress_click = false;
        self.press_moved = false;
        // On a bar the hold is timed from the press and lifts while you
        // hold, so you can see it's got.
        let timer = kind.from_press.then(|| {
            let panel = panel.clone();
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs_f64(dl::HOLD_SECONDS)).await;
                let _ = this.update(cx, |this, cx| this.hold_fired(&panel, cx));
            })
        });
        self.press = Some(Press { panel, kind, start, location: start, pressed_at: Instant::now(), mode: PressMode::Waiting, grab: platform::grab_window(window), _timer: timer });
        cx.notify();
    }

    fn hold_fired(&mut self, panel: &str, cx: &mut Context<Self>) {
        let Some(p) = &mut self.press else { return };
        if p.panel != panel || p.mode != PressMode::Waiting {
            return;
        }
        p.mode = PressMode::Panel;
        let (start, location) = (p.start, p.location);
        self.press_moved = true;
        self.drag_changed(panel, start, location, cx);
    }

    fn drag_changed(&mut self, panel: &str, start: dl::Point, location: dl::Point, cx: &mut Context<Self>) {
        self.model.update(cx, |m, cx| m.update_controller(cx, |c| c.drag_changed(panel, start, location)));
        // Now as well as from the model's observer, so a frame drawn in this
        // same event (the scripted demo draws right after dispatching) is current.
        self.after_drag_update(cx);
    }

    /// After the controller has moved a drag along: start the lift, follow
    /// the target with the preview, and draw.
    pub(crate) fn after_drag_update(&mut self, cx: &mut Context<Self>) {
        let drag = self.model.read(cx).controller.drag.clone();
        match &drag {
            // A panel being dragged from here or from a window of its own:
            // where it would land shows here either way (Swift's overlay).
            Some(d) => {
                // The preview is for places in the dock; opening a window has
                // its own outline. Swift's DragOverlay draws it for `drop != nil`.
                let preview = d.target.as_ref().filter(|t| t.drop.is_some()).map(|t| t.preview);
                self.update_preview(preview);
                if d.window.is_none() {
                    if self.lift.is_none() {
                        self.lift = Some(SpringRun::new(dl::LIFT, 0., 1.));
                    }
                    self.watch_escape(cx);
                    // Outside the window (or on the compass's Window button):
                    // the outline of the window it would open, there.
                    let float = d.target.as_ref().and_then(|t| t.float);
                    let title = self.content.title(&d.panel, cx);
                    let (height, accent) = (self.options.header_height, self.options.accent);
                    self.windows.update_outline(float, title, height, accent, cx);
                }
            }
            None => {
                self.lift = None;
                self.update_preview(None);
            }
        }
        cx.notify();
    }

    /// While a panel is dragging, Escape cancels it, whatever has focus, and
    /// is swallowed rather than also closing a sheet or clearing a field.
    /// GPUI's key listeners are tied to the focus path, so this hangs on the
    /// app-wide interceptor (the Swift dock's NSEvent monitor).
    pub(crate) fn watch_escape(&mut self, cx: &mut Context<Self>) {
        if self.escape_watch.is_some() {
            return;
        }
        let view = cx.entity().downgrade();
        self.escape_watch = Some(cx.intercept_keystrokes(move |event, window, cx| {
            if event.keystroke.key == "escape" && view.update(cx, |v, cx| v.cancel_drag(window, cx)).unwrap_or(false) {
                cx.stop_propagation();
            }
        }));
    }

    /// The highlighted space moves to the new target over 0.16 s, or appears
    /// where it is (Swift: `withAnimation(Self.track)` on `drag?.target`).
    fn update_preview(&mut self, target: Option<dl::Rect>) {
        if target == self.last_preview {
            return;
        }
        self.last_preview = target;
        let now = Instant::now();
        self.preview = match (self.preview.take(), target) {
            (Some(run), Some(to)) => Some(Tween::new(run.value(now), to, dl::TRACK_SECONDS)),
            (None, Some(to)) => Some(Tween::new(to, to, 0.)),
            (_, None) => None,
        };
    }

    fn pointer_moved(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        // The button came up without us seeing it (released between two
        // frames, or on something that took the mouse-up): the press is over.
        if event.pressed_button != Some(MouseButton::Left) {
            if self.press.is_some() || self.divider.is_some() {
                self.press_ended_without_release(window, cx);
            }
            return;
        }
        let location = self.dock_point(event.position);
        if self.divider.is_some() {
            self.divider_moved(location, cx);
            return;
        }
        let Some(p) = &mut self.press else { return };
        p.location = location;
        let (panel, start, kind) = (p.panel.clone(), p.start, p.kind);
        if p.mode == PressMode::Waiting {
            let moved = ((location.x - start.x).powi(2) + (location.y - start.y).powi(2)).sqrt() >= dl::DRAG_DISTANCE;
            if moved {
                let held = !kind.needs_hold || p.pressed_at.elapsed().as_secs_f64() >= dl::HOLD_SECONDS;
                let mode = if kind.from_press {
                    // Moving first means the window (or nothing), as the
                    // hold's timer hasn't fired.
                    if self.drags_window(kind, &panel, cx) { PressMode::Window } else { PressMode::Nothing }
                } else if held {
                    PressMode::Panel
                } else if self.drags_window(kind, &panel, cx) {
                    PressMode::Window
                } else {
                    PressMode::Nothing
                };
                if let Some(p) = &mut self.press {
                    p.mode = mode;
                    p._timer = None;
                }
                self.press_moved = matches!(mode, PressMode::Window | PressMode::Panel);
                if mode == PressMode::Window && self.press.as_ref().is_some_and(|p| p.grab.is_none()) {
                    window.start_window_move();
                }
            }
        }
        let (mode, grab) = self.press.as_ref().map_or((None, None), |p| (Some(p.mode), p.grab));
        if mode == Some(PressMode::Panel) {
            self.suppress_click = true;
            self.drag_changed(&panel, start, location, cx);
        } else if let (Some(PressMode::Window), Some(grab)) = (mode, grab) {
            platform::move_grabbed_window(window, grab);
        }
    }

    fn pointer_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.divider.take().is_some() {
            cx.notify();
            return;
        }
        let Some(p) = self.press.take() else { return };
        match p.mode {
            PressMode::Panel => {
                self.drag_ended(window, cx);
            }
            // Once more where it let go: the last move event can be a point
            // or two short of it.
            PressMode::Window => {
                if let Some(grab) = p.grab {
                    platform::move_grabbed_window(window, grab);
                }
            }
            PressMode::Waiting if self.drags_window(p.kind, &p.panel, cx) && event.click_count == 2 => {
                // A double-click that didn't move zooms the window (Swift's
                // HoldDrag.ended).
                window.zoom_window();
            }
            _ => {}
        }
        cx.notify();
    }

    /// No release was seen but the button is up: nothing lifted stays lifted.
    fn press_ended_without_release(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.divider = None;
        if let Some(p) = self.press.take() {
            if p.mode == PressMode::Panel {
                self.drag_ended(window, cx);
            }
        }
        cx.notify();
    }

    /// The panel was let go: the flight to where it landed starts here.
    fn drag_ended(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.model.read(cx).controller.drag.clone();
        let lift = self.lift.as_ref().map(|r| r.value(Instant::now()) as f64).unwrap_or(1.);
        self.model.update(cx, |m, cx| m.update_controller(cx, |c| c.drag_ended()));
        self.after_drop(before, lift, window, cx);
    }

    /// Starts the flight for a drop or a cancel. `drag` is the drag as it
    /// was just before it ended.
    fn after_drop(&mut self, drag: Option<dl::DragState>, lift: f64, window: &mut Window, cx: &mut Context<Self>) {
        self.lift = None;
        self.update_preview(None);
        self.windows.update_outline(None, SharedString::default(), self.options.header_height, self.options.accent, cx);
        let landing = self.model.read(cx).controller.landing.clone();
        if let (Some(d), Some(landing)) = (drag, landing) {
            self.landing = Some(LandingRun::new(dl::DockController::ghost_rect(&d, lift), landing.to, d.panel.clone(), d.source_frame.size()));
            // Swift: after 300 ms the landing clears with a 120 ms fade.
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs_f64(dl::LANDING_SECONDS)).await;
                let _ = this.update(cx, |this, cx| {
                    this.model.update(cx, |m, _| m.controller.clear_landing());
                    if let Some(run) = &mut this.landing {
                        run.fading_since = Some(Instant::now());
                    }
                    cx.notify();
                });
                cx.background_executor().timer(Duration::from_secs_f64(dl::LANDING_FADE_SECONDS)).await;
                let _ = this.update(cx, |this, cx| {
                    this.landing = None;
                    cx.notify();
                });
            })
            .detach();
        }
        self.windows_focus_pending(cx);
        cx.notify();
    }

    /// Escape, whatever has focus: the panel flies home. Returns true if a
    /// drag was cancelled.
    fn cancel_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let before = self.model.read(cx).controller.drag.clone();
        if before.is_none() {
            return false;
        }
        let lift = self.lift.as_ref().map(|r| r.value(Instant::now()) as f64).unwrap_or(1.);
        self.model.update(cx, |m, cx| m.update_controller(cx, |c| c.cancel_drag(None)));
        self.after_drop(before, lift, window, cx);
        true
    }

    // MARK: Dividers

    fn begin_divider(&mut self, split: &DockSplit, index: usize, shown: Vec<usize>, position: Point<Pixels>, cx: &mut Context<Self>) {
        let p = self.dock_point(position);
        let start = if split.axis == SplitAxis::Horizontal { p.x } else { p.y };
        let length = self.split_lengths.borrow().get(&split.id).copied().unwrap_or(0.);
        let available = length - GAP as f64 * (shown.len().saturating_sub(1)) as f64;
        self.divider =
            Some(DividerDrag { split: split.id.clone(), index, axis: split.axis, start, start_fractions: split.fractions.clone(), shown, available });
        cx.notify();
    }

    fn divider_moved(&mut self, location: dl::Point, cx: &mut Context<Self>) {
        let Some(d) = &self.divider else { return };
        let t = if d.axis == SplitAxis::Horizontal { location.x } else { location.y } - d.start;
        let available = d.available.max(1.);
        let visible = dl::normalized(&d.shown.iter().map(|&i| d.start_fractions[i]).collect::<Vec<_>>());
        let moved = dl::resized(&visible, d.index, t / available, dl::MIN_PANEL / available);
        let all = dl::merged(&moved, &d.start_fractions, &d.shown);
        let split = d.split.clone();
        self.model.update(cx, |m, cx| m.update_controller(cx, |c| c.layout.set_fractions(&split, &all)));
    }

    // MARK: Rendering

    fn node(&mut self, node: &DockNode, layout: &DockLayout, theme: &DockTheme, available: &Rc<HashSet<PanelId>>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match node {
            DockNode::Group(g) => self.group(g, layout, theme, available, window, cx),
            DockNode::Split(s) => self.split(s, layout, theme, available, window, cx),
        }
    }

    fn split(&mut self, split: &DockSplit, layout: &DockLayout, theme: &DockTheme, available: &Rc<HashSet<PanelId>>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let avail = available.clone();
        let is_available = move |p: &str| avail.contains(p);
        // Children with nothing to show right now (a thread's patch panel,
        // moved to a side of its own, while a thread without it is on show)
        // are left out and their room shared among the rest, as unavailable
        // tabs are within a group. The layout keeps them, so they're back
        // when there's something to show again.
        let shown = dl::shown_indices(split, &is_available);
        let fractions = dl::normalized(&shown.iter().map(|&i| split.fractions[i]).collect::<Vec<_>>());
        let horizontal = split.axis == SplitAxis::Horizontal;

        let mut row = div().flex().size_full().min_w(px(0.)).min_h(px(0.));
        row = if horizontal { row.flex_row() } else { row.flex_col() };
        for (k, &i) in shown.iter().enumerate() {
            let child = self.node(&split.children[i], layout, theme, available, window, cx);
            row = row.child(flex_cell(fractions[k]).child(child));
            if k + 1 < shown.len() {
                row = row.child(gap_cell(horizontal));
            }
        }

        // The grab strips go over the panels (a strip beside the next panel
        // in the flow would be under it), in a second row with the same
        // shares so each lands on its gap without knowing the pixels.
        let mut rail = div().absolute().top_0().left_0().size_full().flex();
        rail = if horizontal { rail.flex_row() } else { rail.flex_col() };
        for (k, _) in shown.iter().enumerate() {
            rail = rail.child(flex_cell(fractions[k]));
            if k + 1 < shown.len() {
                rail = rail.child(self.divider(split, k, shown.clone(), horizontal, theme, cx));
            }
        }

        let lengths = self.split_lengths.clone();
        let id = split.id.clone();
        div()
            .relative()
            .size_full()
            .child(measure(move |bounds, _, _| {
                let length = if horizontal { bounds.size.width } else { bounds.size.height };
                lengths.borrow_mut().insert(id.clone(), length.as_f32() as f64);
            }))
            .child(row)
            .child(rail)
            .into_any_element()
    }

    /// The invisible strip over a hairline that resizes the panels either
    /// side: 14 pt wide, lit in the accent as you hover (Mike, 30 Oct).
    fn divider(&self, split: &DockSplit, k: usize, shown: Vec<usize>, horizontal: bool, theme: &DockTheme, cx: &mut Context<Self>) -> Div {
        let dragging_this = self.divider.as_ref().is_some_and(|d| d.split == split.id && d.index == k);
        let group = SharedString::from(format!("dock-divider-{}-{}", split.id, k));
        let accent = theme.accent;
        let split_for_press = split.clone();
        // The strip extends 6.5 each side of the 1 pt gap: 14 pt to grab.
        let strip = div()
            .id(ElementId::Name(format!("dock-divider-strip-{}-{}", split.id, k).into()))
            .group(group.clone())
            .absolute()
            .flex()
            .items_center()
            .justify_center()
            .when(horizontal, |d| d.left(px(-6.5)).top_0().bottom_0().w(px(14.)).cursor(CursorStyle::ResizeLeftRight))
            .when(!horizontal, |d| d.top(px(-6.5)).left_0().right_0().h(px(14.)).cursor(CursorStyle::ResizeUpDown))
            .child(
                div()
                    .when(horizontal, |d| d.w(px(3.)).h_full())
                    .when(!horizontal, |d| d.h(px(3.)).w_full())
                    .when(dragging_this, |d| d.bg(accent.opacity(0.85)))
                    .group_hover(group, move |s| s.bg(accent.opacity(0.85))),
            )
            .tooltip(|window, cx| Tooltip::new("Drag to resize").build(window, cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    this.begin_divider(&split_for_press, k, shown.clone(), e.position, cx);
                    cx.stop_propagation();
                }),
            );
        div().flex_none().relative().when(horizontal, |d| d.w(px(GAP))).when(!horizontal, |d| d.h(px(GAP))).child(strip)
    }

    fn group(&mut self, group: &DockGroup, layout: &DockLayout, theme: &DockTheme, available: &Rc<HashSet<PanelId>>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let avail = available.clone();
        let is_available = move |p: &str| avail.contains(p);
        // The tabs with something to show; the layout keeps the rest.
        let visible = dl::visible_group(group, &is_available);
        let panel = visible.selected.clone();
        let tabbed = visible.panels.len() > 1;
        let (lifted, landing) = {
            let ctrl = &self.model.read(cx).controller;
            let lifted = ctrl.drag.as_ref().is_some_and(|d| d.lifted && d.window.is_none() && d.panel == visible.selected);
            let landing = ctrl.landing.as_ref().is_some_and(|l| l.panel == visible.selected);
            (lifted, landing)
        };
        let header = layout.shows_header(&visible);
        // Under the traffic lights, in a window without a title bar: the
        // bar stays (empty, if the title's hidden) so they've somewhere to sit.
        let corner = self.options.traffic_lights > 0. && dl::top_leading_group_id(&layout.root, &is_available) == group.id;
        let header_height = self.options.header_height;

        let frames_model = self.model.clone();
        let frame_group = group.id.clone();
        let root_bounds = self.root_bounds.clone();
        let hidden_strip = !header && !corner;

        let menu = self.panel_menu(panel.clone(), tabbed);
        let body_press = panel.clone();
        let content = self.content.view(&panel, false, window, cx);
        let body = div()
            .id(ElementId::Name(format!("dock-body-{}", group.id).into()))
            .relative()
            .flex_1()
            .min_h(px(0.))
            .min_w(px(0.))
            .overflow_hidden()
            .opacity(if lifted { 0.18 } else { 1. })
            .child(content)
            // A plain bubble listener, the lowest priority: any drag the
            // panel's own content handles (it stops the press) wins, so only
            // a held press on its background moves the panel.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| this.begin_press(body_press.clone(), BODY_PRESS, e, window, cx)),
            )
            .context_menu(menu);

        v_flex()
            .id(ElementId::Name(format!("dock-group-{}", group.id).into()))
            .relative()
            .size_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .overflow_hidden()
            .bg(theme.panel)
            .when(landing, |d| d.opacity(0.))
            .child(measure(move |bounds, _, cx| {
                let origin = root_bounds.get().origin;
                let rect = bounds_to_rect(bounds, origin);
                frames_model.update(cx, |m, _| {
                    m.controller.frames.insert(frame_group.clone(), rect);
                });
            }))
            .when(header, |d| d.child(self.header(&visible, corner, theme, window, cx)))
            .when(!header && corner, |d| d.child(self.corner_bar(panel.clone(), header_height, theme, cx)))
            .child(body)
            .when(hidden_strip, |d| d.child(self.hidden_strip(panel.clone(), theme, cx)))
            .when(lifted, |d| {
                // The hole it left behind.
                d.child(div().absolute().inset(px(6.)).rounded(px(8.)).border_1().border_dashed().border_color(theme.faint))
            })
            .into_any_element()
    }

    fn header(&mut self, group: &DockGroup, corner: bool, theme: &DockTheme, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let single = group.panels.len() == 1;
        let leading = if corner { self.options.traffic_lights } else { 0. };
        let moves_window_strip = corner && self.options.corner_moves_window && leading > 0.;
        let root_bounds = self.root_bounds.clone();
        let (group_id, group_id2) = (group.id.clone(), group.id.clone());
        let model = self.model.clone();
        let model2 = self.model.clone();
        let selected = group.selected.clone();

        let mut bar = h_flex()
            .relative()
            .flex_none()
            .w_full()
            .h(px(self.options.header_height))
            .pl(px(6. + leading))
            .pr(px(6.))
            .gap(px(2.))
            .border_b_1()
            .border_color(theme.bar_line);
        // Where a dragged panel joins this group as a tab (bar_target).
        let measure_root = root_bounds.clone();
        bar = bar.child(measure(move |bounds, _, cx| {
            let rect = bounds_to_rect(bounds, measure_root.get().origin);
            model.update(cx, |m, _| {
                m.controller.bars.insert(group_id.clone(), rect);
            });
        }));
        // The strip beside the traffic lights: moves the window, double-click zooms.
        if moves_window_strip {
            bar = bar.child(div().id("dock-window-space").absolute().left_0().top_0().bottom_0().w(px(leading)).on_mouse_down(
                MouseButton::Left,
                |e: &MouseDownEvent, window, _| {
                    if e.click_count == 2 {
                        window.zoom_window();
                    } else {
                        window.start_window_move();
                    }
                },
            ));
        }
        let custom = if single { self.content.header(&selected, window, cx) } else { None };
        if let Some(custom) = custom {
            let press_panel = selected.clone();
            bar = bar.child(
                div()
                    .id(ElementId::Name(format!("dock-header-{}", group.id).into()))
                    .pl(px(8.))
                    .min_w(px(0.))
                    .child(custom)
                    // Lowest priority: the header's own buttons and chips win.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| this.begin_press(press_panel.clone(), BAR_PRESS, e, window, cx)),
                    )
                    .context_menu(self.panel_menu(selected.clone(), false)),
            );
        } else {
            for panel in &group.panels {
                bar = bar.child(self.tab(panel, *panel == group.selected, single, theme, window, cx));
            }
        }
        // The empty part of a bar: moves its panel (or the window, HoldDrag).
        let press_panel = selected.clone();
        let space_root = root_bounds;
        bar = bar.child(
            div()
                .id(ElementId::Name(format!("dock-bar-space-{}", group.id).into()))
                .flex_1()
                .h_full()
                .min_w(px(0.))
                .child(measure(move |bounds, _, cx| {
                    let origin = space_root.get().origin;
                    let min_x = (bounds.origin.x - origin.x).as_f32() as f64;
                    model2.update(cx, |m, _| {
                        m.controller.bar_space_starts.insert(group_id2.clone(), min_x);
                    });
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| this.begin_press(press_panel.clone(), BAR_PRESS, e, window, cx)),
                )
                .context_menu(self.panel_menu(selected.clone(), !single)),
        );
        if let Some(accessory) = self.content.accessory(&selected, window, cx) {
            bar = bar.child(accessory);
        }
        bar
    }

    /// A tab, or the title when the panel's alone (TabItem): picks up the
    /// panel when dragged, selects on click, × to close, middle-click closes.
    fn tab(&mut self, panel: &str, selected: bool, single: bool, theme: &DockTheme, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let content = self.content.clone();
        let title = content.title(panel, cx);
        let icon = content.icon(panel, window, cx);
        let badge = content.badge(panel, window, cx);
        let closable = content.closable(panel, cx);
        let id = panel_key(panel);
        let group_name = SharedString::from(format!("dock-tab-{id}"));
        let title_size = self.options.title_size;
        let header_height = self.options.header_height;

        let (press, click, close1, close2, close_x, tip, middle_down, middle_up) = (
            panel.to_string(),
            panel.to_string(),
            panel.to_string(),
            panel.to_string(),
            panel.to_string(),
            self.move_tip(panel, cx),
            panel.to_string(),
            panel.to_string(),
        );
        let is_hidden_tip = Rc::new(tip);

        let mut row = h_flex()
            .id(ElementId::Name(format!("dock-tab-{id}").into()))
            .group(group_name.clone())
            .h(px((header_height - 8.).max(20.)))
            .pl(px(8.))
            .pr(px(if closable { 4. } else { 8. }))
            .gap(px(6.))
            .rounded(px(6.))
            .when(single, |d| d.min_w(px(0.)).flex_shrink_1())
            .when(!single, |d| d.flex_none())
            .when(selected && !single, |d| d.bg(theme.tab_selected))
            .when(!(selected && !single), |d| d.hover(|s| s.bg(theme.tab_hover)));

        if let Some(icon) = icon {
            row = row.child(div().relative().flex_none().child(icon).when_some(badge, |d, b| d.child(div().absolute().right(px(-2.)).bottom(px(-2.)).child(b))));
        }
        row = row.child(
            div()
                .text_size(px(title_size))
                .when(selected, |d| d.font_weight(FontWeight::SEMIBOLD))
                .text_color(if selected { theme.header_text } else { theme.secondary })
                .when(single, |d| d.truncate())
                .when(!single, |d| d.whitespace_nowrap())
                .child(title),
        );
        if closable {
            let close_group = group_name.clone();
            let title_for_tip = content.title(panel, cx);
            row = row.child(
                div()
                    .id(ElementId::Name(format!("dock-tab-close-{id}").into()))
                    .flex_none()
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .invisible_unless_hover(close_group, selected && !single)
                    .hover(|s| s.bg(theme.tab_selected))
                    .child(svg().path(Lucide::Close.path()).size(px(10.)).text_color(theme.secondary))
                    .tooltip(move |window, cx| Tooltip::new(format!("Close {title_for_tip}")).build(window, cx))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.model.update(cx, |m, cx| m.close(&close_x, cx)))),
            );
        }

        row.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, window, cx| this.begin_press(press.clone(), TAB_PRESS, e, window, cx)),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            // After a drag the mouse-up lands on the tab that was dragged.
            if !std::mem::take(&mut this.suppress_click) {
                this.model.update(cx, |m, cx| m.select(&click, cx));
            }
        }))
        // Middle-click closes a tab, on release and only over the tab, as
        // in a browser: pressing then sliding off lets you change your mind.
        .when(closable, |d| {
            d.on_mouse_down(MouseButton::Middle, cx.listener(move |this, _: &MouseDownEvent, _, _| this.windows.middle_down = Some(middle_down.clone())))
                .on_mouse_up(
                    MouseButton::Middle,
                    cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                        if this.windows.middle_down.take().as_deref() == Some(middle_up.as_str()) {
                            this.model.update(cx, |m, cx| m.close(&close1, cx));
                        }
                    }),
                )
        })
        .tooltip(move |window, cx| Tooltip::new(is_hidden_tip.as_str().to_string()).build(window, cx))
        .context_menu(self.panel_menu(close2, !single))
        .into_any_element()
    }

    /// No hold to mention: a tab moves as soon as it's dragged. Mike couldn't
    /// tell a title could be dragged at all (5 Nov).
    fn move_tip(&self, panel: &str, cx: &App) -> String {
        let layout = &self.model.read(cx).controller.layout;
        if layout.is_floating(panel) {
            "Drag to move, or onto the main window to dock it".into()
        } else if layout.can_float(panel) {
            "Drag to move, or out of the window to open it on its own".into()
        } else {
            "Drag to move".into()
        }
    }

    /// Right-click on a title, a tab, the strip of a panel without a title,
    /// or a panel's background (PanelMenu).
    pub(crate) fn panel_menu(&self, panel: PanelId, tabbed: bool) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let model = self.model.clone();
        let content = self.content.clone();
        move |menu, _, cx| {
            let (hidden, floating, can_float) = {
                let layout = &model.read(cx).controller.layout;
                (layout.hidden_titles.contains(&panel), layout.is_floating(&panel), layout.can_float(&panel))
            };
            let m1 = model.clone();
            let p1 = panel.clone();
            let mut menu = if hidden {
                menu.item(PopupMenuItem::new("Show Title").icon(Icon::new(Lucide::Eye)).on_click(move |_, _, cx| m1.update(cx, |m, cx| m.set_title_hidden(&p1, false, cx))))
            } else {
                // With tabs the bar has to stay, so say when it'll take effect.
                let label = if tabbed { "Hide Title When on Its Own" } else { "Hide Title" };
                menu.item(PopupMenuItem::new(label).icon(Icon::new(Lucide::EyeOff)).on_click(move |_, _, cx| m1.update(cx, |m, cx| m.set_title_hidden(&p1, true, cx))))
            };
            if floating {
                let (m2, p2) = (model.clone(), panel.clone());
                menu = menu.item(
                    PopupMenuItem::new("Put Back in Main Window").icon(Icon::new(Lucide::PanelLeftClose)).on_click(move |_, _, cx| m2.update(cx, |m, cx| m.put_back(&p2, cx))),
                );
            } else if can_float {
                let (m2, p2) = (model.clone(), panel.clone());
                menu = menu.item(
                    PopupMenuItem::new("Open in New Window").icon(Icon::new(Lucide::ExternalLink)).on_click(move |_, _, cx| m2.update(cx, |m, cx| m.open_in_window(&p2, cx))),
                );
            }
            if content.closable(&panel, cx) {
                let (m3, p3) = (model.clone(), panel.clone());
                let title = content.title(&panel, cx);
                menu = menu.separator().item(PopupMenuItem::new(format!("Close {title}")).icon(Icon::new(Lucide::Close)).on_click(move |_, _, cx| m3.update(cx, |m, cx| m.close(&p3, cx))));
            }
            menu
        }
    }

    /// In place of the title bar for a panel that hides its title: a strip
    /// along the top that shows a grab handle on hover, to drag the panel by
    /// and right-click for the title back.
    fn hidden_strip(&self, panel: PanelId, theme: &DockTheme, cx: &mut Context<Self>) -> AnyElement {
        let group = SharedString::from(format!("dock-strip-{}", panel_key(&panel)));
        let press = panel.clone();
        let grip = theme.header_text.opacity(0.28);
        div()
            .id(ElementId::Name(format!("dock-hidden-strip-{}", panel_key(&panel)).into()))
            .group(group.clone())
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(10.))
            .flex()
            .justify_center()
            .items_center()
            .cursor(CursorStyle::OpenHand)
            .child(div().w(px(34.)).h(px(4.)).rounded_full().group_hover(group, move |s| s.bg(grip)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| this.begin_press(press.clone(), BODY_PRESS, e, window, cx)),
            )
            .tooltip(|window, cx| Tooltip::new("Press and hold, then drag to move this panel. Right-click to show its title.").build(window, cx))
            .context_menu(self.panel_menu(panel, false))
            .into_any_element()
    }

    /// The bar of a title-less panel under the traffic lights: empty, moving
    /// the window (or the panel, with a title bar), with the grab handle on
    /// hover to move the panel by. In Threadbear that's the sidebar's bar,
    /// the one place left to grab the window by.
    fn corner_bar(&self, panel: PanelId, height: f32, theme: &DockTheme, cx: &mut Context<Self>) -> AnyElement {
        let leading = self.options.traffic_lights;
        let press = panel.clone();
        let mut row = h_flex().flex_none().w_full().h(px(height));
        row = if self.options.corner_moves_window {
            row.child(div().id("dock-corner-window").flex_1().h_full().on_mouse_down(MouseButton::Left, |e: &MouseDownEvent, window, _| {
                if e.click_count == 2 {
                    window.zoom_window();
                } else {
                    window.start_window_move();
                }
            }))
        } else {
            row.child(div().flex_none().w(px(leading))).child(
                div().id("dock-corner-space").flex_1().h_full().on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| this.begin_press(press.clone(), BAR_PRESS, e, window, cx)),
                ),
            )
        };
        row = row.child(div().relative().flex_none().w(px(60.)).h_full().child(self.hidden_strip(panel, theme, cx)));
        row = row.child(div().flex_none().w(px(12.)));
        row.into_any_element()
    }

    fn sync_windows(&mut self, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let content = self.content.clone();
        let options = self.options.clone();
        self.windows.sync(model, content, options, cx);
    }

    fn windows_focus_pending(&mut self, cx: &mut Context<Self>) {
        self.sync_windows(cx);
    }
}

impl Render for DockView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = DockTheme::new(cx, self.options.accent);
        let layout = self.model.read(cx).controller.layout.clone();
        let available: Rc<HashSet<PanelId>> = {
            let content = self.content.clone();
            Rc::new(layout.panels().into_iter().filter(|p| content.available(p, cx)).collect())
        };

        // Measures the dock before anything else in the frame, so frames
        // measured below are relative to this frame's origin.
        let root_bounds = self.root_bounds.clone();
        let measure_model = self.model.clone();
        let measure_root = measure(move |bounds, window, cx| {
            root_bounds.set(bounds);
            let window_bounds = window.bounds();
            let viewport = window.viewport_size();
            measure_model.update(cx, |m, _| {
                let c = &mut m.controller;
                c.bounds = dl::Rect::new(0., 0., bounds.size.width.as_f32() as f64, bounds.size.height.as_f32() as f64);
                // The window's content area in dock coordinates, and where the
                // dock's corner is on screen. Windows in this dock have full-size
                // content, so the window's frame origin is the content's.
                c.window_rect = Some(dl::Rect::new(
                    -bounds.origin.x.as_f32() as f64,
                    -bounds.origin.y.as_f32() as f64,
                    viewport.width.as_f32() as f64,
                    viewport.height.as_f32() as f64,
                ));
                c.screen_origin = Some(dl::Point::new(
                    (window_bounds.origin.x + bounds.origin.x).as_f32() as f64,
                    (window_bounds.origin.y + bounds.origin.y).as_f32() as f64,
                ));
            });
        });

        let tree = self.node(&layout.root, &layout, &theme, &available, window, cx);
        let divider_axis = self.divider.as_ref().map(|d| d.axis);
        let dragging_panel = self.model.read(cx).controller.drag.is_some();
        if !dragging_panel && self.escape_watch.is_some() {
            // Not from inside the interceptor's own callback, so it can go.
            self.escape_watch = None;
        }
        let overlay = overlay::overlay(self, &theme, window, cx);
        let entity = cx.entity().downgrade();

        div()
            .id("beardock")
            .relative()
            .size_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .overflow_hidden()
            // The hairlines between panels, showing through the 1 pt gaps:
            // the separator colour over the panel colour, as Swift's are.
            .bg(theme.panel)
            .text_color(theme.header_text)
            .child(measure_root)
            .child(div().absolute().top_0().left_0().size_full().bg(theme.hairline))
            .child(tree)
            .child(overlay)
            // Last, so the drag's cursor wins over what's under the pointer.
            // Always there, not just while something's in progress: a press
            // and its release can land between two frames, and a press that
            // misses its release would pick the panel up on the next move.
            .child(listeners(entity, dragging_panel, divider_axis))
    }
}

/// While a press, drag or divider is live: window-level mouse listeners, so
/// moves and the release are seen wherever they happen, and the drag's cursor.
fn listeners(view: WeakEntity<DockView>, dragging: bool, divider_axis: Option<SplitAxis>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            if let Some(axis) = divider_axis {
                window.set_window_cursor_style(if axis == SplitAxis::Horizontal { CursorStyle::ResizeLeftRight } else { CursorStyle::ResizeUpDown });
            } else if dragging {
                window.set_window_cursor_style(CursorStyle::ClosedHand);
            }
            let moved = view.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    let _ = moved.update(cx, |v, cx| v.pointer_moved(e, window, cx));
                }
            });
            let released = view.clone();
            window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    let _ = released.update(cx, |v, cx| v.pointer_up(e, window, cx));
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A flex cell taking `fraction` of the split: basis as a percentage, shrinking
/// together, so with the 1 pt gaps each gets the same share of what's left that
/// Swift's SplitLayout gives it.
fn flex_cell(fraction: f64) -> Div {
    div().flex_basis(relative(fraction as f32)).flex_shrink_1().min_w(px(0.)).min_h(px(0.)).overflow_hidden()
}

fn gap_cell(horizontal: bool) -> Div {
    div().flex_none().when(horizontal, |d| d.w(px(GAP))).when(!horizontal, |d| d.h(px(GAP)))
}

/// Reports an element's bounds each frame, for hit testing.
fn measure(on_bounds: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static) -> impl IntoElement {
    // With explicit insets: an absolute child with none sits where it would
    // have been in the flow, after whatever comes before it.
    canvas(move |bounds, window, cx| on_bounds(bounds, window, cx), |_, _, _, _| {}).absolute().top_0().left_0().size_full()
}

fn bounds_to_rect(bounds: Bounds<Pixels>, origin: Point<Pixels>) -> dl::Rect {
    dl::Rect::new(
        (bounds.origin.x - origin.x).as_f32() as f64,
        (bounds.origin.y - origin.y).as_f32() as f64,
        bounds.size.width.as_f32() as f64,
        bounds.size.height.as_f32() as f64,
    )
}

/// A panel id made safe to embed in an element id.
pub(crate) fn panel_key(panel: &str) -> String {
    panel.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
}

use gpui_kit::component::Icon;

/// Fades the close button in only on hover, or stays shown on the selected
/// tab with others beside it.
trait InvisibleUnlessHover {
    fn invisible_unless_hover(self, group: SharedString, always: bool) -> Self;
}

impl InvisibleUnlessHover for Stateful<Div> {
    fn invisible_unless_hover(self, group: SharedString, always: bool) -> Self {
        if always {
            self
        } else {
            self.invisible().group_hover(group, |s| s.visible())
        }
    }
}
