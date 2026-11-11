// The panels taken out of the dock into windows of their own, kept in step
// with the layout's `floating` list. Ported from
// dock/Sources/BearDock/DockWindows.swift. Each window draws its panel's bar
// over the panel, and dragging the bar moves the window with the drop targets
// showing over the main window (see platform.rs for what that takes).
use crate::content::{DockOptions, SharedContent};
use crate::model::DockModel;
use crate::platform;
use crate::theme::DockTheme;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::*;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use threadbear_dock_layout as dl;
use threadbear_dock_layout::{DockFloat, PanelId};

/// The windows of panels taken out of the dock, and the outline of one about
/// to open.
#[derive(Default)]
pub struct FloatingWindows {
    windows: HashMap<PanelId, AnyWindowHandle>,
    outline: Option<OutlineWindow>,
    /// A middle-click pressed on this panel's tab, closing it on release.
    pub(crate) middle_down: Option<PanelId>,
}

struct OutlineWindow {
    handle: AnyWindowHandle,
    size: (f64, f64),
}

impl FloatingWindows {
    /// Opens a window for each floating panel without one, closes those
    /// whose panel went back into the dock or was closed. A panel with
    /// nothing to show right now (a thread's patch panel while another
    /// thread is on show) keeps its window, hidden, until it has something
    /// again (Swift's `sync`; hiding needs the raw window, so for now a
    /// window stays showing while its panel is unavailable).
    pub fn sync(&mut self, model: Entity<DockModel>, content: SharedContent, options: Rc<DockOptions>, cx: &mut App) {
        let floating = model.read(cx).controller.layout.floating.clone();
        let gone: Vec<PanelId> = self.windows.keys().filter(|p| !floating.iter().any(|f| &f.panel == *p)).cloned().collect();
        for panel in gone {
            if let Some(handle) = self.windows.remove(&panel) {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        }
        let focus = model.read(cx).controller.focus_next.clone();
        for float in &floating {
            if let Some(handle) = self.windows.get(&float.panel).copied() {
                // Already open (show or a rail button): just bring it forward.
                if focus.as_deref() == Some(float.panel.as_str()) {
                    let _ = handle.update(cx, |_, window, _| window.activate_window());
                }
                continue;
            }
            let handle = open_floating_window(float, focus.as_deref() == Some(float.panel.as_str()), model.clone(), content.clone(), options.clone(), cx);
            if let Some(handle) = handle {
                self.windows.insert(float.panel.clone(), handle);
            }
        }
        if focus.is_some_and(|p| floating.iter().any(|f| f.panel == p)) {
            model.update(cx, |m, _| m.controller.focus_next = None);
        }
    }

    /// The outline of the window a drag would open, following the pointer
    /// outside the dock's window. Shown only where windows can be placed
    /// and moved by hand, so not on Wayland (see platform.rs).
    pub fn update_outline(&mut self, frame: Option<dl::Rect>, title: SharedString, header_height: f32, accent: Option<Hsla>, cx: &mut App) {
        let Some(frame) = frame.filter(|_| platform::can_move_windows_by_hand()) else {
            if let Some(outline) = self.outline.take() {
                let _ = outline.handle.update(cx, |_, window, _| window.remove_window());
            }
            return;
        };
        let origin = point(px(frame.x as f32), px(frame.y as f32));
        if let Some(outline) = &self.outline {
            if outline.size == (frame.width, frame.height) {
                let handle = outline.handle;
                let _ = handle.update(cx, |_, window, _| platform::set_window_origin(window, origin));
                return;
            }
            let old = outline.handle;
            let _ = old.update(cx, |_, window, _| window.remove_window());
            self.outline = None;
        }
        let bounds = Bounds::new(origin, size(px(frame.width as f32), px(frame.height as f32)));
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: None,
                kind: WindowKind::PopUp,
                focus: false,
                show: true,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            },
            |window, cx| {
                // It floats under the pointer it follows: clicks go through.
                platform::set_ignores_mouse_events(window, true);
                cx.new(|_| OutlineView { title, header_height, accent })
            },
        );
        if let Ok(handle) = opened {
            self.outline = Some(OutlineWindow { handle: handle.into(), size: (frame.width, frame.height) });
        }
    }
}

/// Where a panel would open as a window of its own, drawn while it's dragged
/// out of the dock: the window's outline with its bar, see-through.
struct OutlineView {
    title: SharedString,
    header_height: f32,
    accent: Option<Hsla>,
}

impl Render for OutlineView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = DockTheme::new(cx, self.accent);
        let accent = theme.accent;
        div()
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .overflow_hidden()
            .bg(accent.opacity(0.08))
            .border(px(1.5))
            .border_color(accent.opacity(0.75))
            .text_color(accent)
            .child(
                h_flex()
                    .flex_none()
                    .h(px(self.header_height))
                    .pl(px(78.))
                    .bg(accent.opacity(0.14))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .child("New window"),
            )
    }
}

/// A frame saved on a display that's since gone comes back on the main one.
fn on_screen(frame: dl::Rect, cx: &App) -> dl::Rect {
    let displays = cx.displays();
    let visible = displays.iter().any(|d| {
        let b = d.bounds();
        let bx = b.origin.x.as_f32() as f64;
        let by = b.origin.y.as_f32() as f64;
        frame.x < bx + b.size.width.as_f32() as f64 && frame.max_x() > bx && frame.y < by + b.size.height.as_f32() as f64 && frame.max_y() > by
    });
    if visible || displays.is_empty() {
        return frame;
    }
    let Some(display) = cx.primary_display() else { return frame };
    let b = display.bounds();
    let cx_ = (b.origin.x.as_f32() as f64) + b.size.width.as_f32() as f64 / 2.;
    let cy = (b.origin.y.as_f32() as f64) + b.size.height.as_f32() as f64 / 2.;
    dl::Rect::new(cx_ - frame.width / 2., cy - frame.height / 2., frame.width, frame.height)
}

fn open_floating_window(float: &DockFloat, focus: bool, model: Entity<DockModel>, content: SharedContent, options: Rc<DockOptions>, cx: &mut App) -> Option<AnyWindowHandle> {
    let frame = on_screen(float.frame, cx);
    let bounds = Bounds::new(point(px(frame.x as f32), px(frame.y as f32)), size(px(frame.width as f32), px(frame.height as f32)));
    let panel = float.panel.clone();
    let title: SharedString = content.title(&panel, cx);
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        // The panel's own bar is the top of the window, as in the main one.
        titlebar: Some(TitlebarOptions { title: Some(title), appears_transparent: true, traffic_light_position: None }),
        focus,
        show: true,
        // The bar is the panel's: AppKit must not treat the top of the
        // window as its title bar and start moving the window on a drag
        // there (Swift's DockAnchor locks it for the same reason).
        app_owns_titlebar_drag: true,
        window_min_size: Some(size(px(240.), px(160.))),
        app_id: Some("threadbear".into()),
        ..Default::default()
    };
    let result = cx.open_window(opts, |window, cx| {
        if let Some(prepare) = &options.prepare_window {
            prepare(window, cx);
        }
        let view = cx.new(|cx| FloatingPanelView::new(panel, model, content, options, window, cx));
        // The red button closes the panel, as its × would, or puts back one
        // the app always keeps (Threadbear's thread).
        let on_close = view.clone();
        window.on_window_should_close(cx, move |_, cx| {
            let (panel, model, closable) = on_close.read_with(cx, |v, cx| (v.panel.clone(), v.model.clone(), v.content.closable(&v.panel, cx)));
            model.update(cx, |m, cx| if closable { m.close(&panel, cx) } else { m.put_back(&panel, cx) });
            // The sync closes the window.
            false
        });
        cx.new(|cx| gpui_kit::base::Root::new(view, window, cx))
    });
    result.ok().map(Into::into)
}

/// A panel in a window of its own: its bar (title, buttons, menu) over it, the
/// bar dragging it back into the dock.
pub struct FloatingPanelView {
    pub panel: PanelId,
    pub model: Entity<DockModel>,
    pub content: SharedContent,
    options: Rc<DockOptions>,
    press: Option<FloatPress>,
    _subscriptions: Vec<Subscription>,
}

struct FloatPress {
    /// Escape, whatever has focus (see DockView::watch_escape).
    escape_watch: Option<Subscription>,
    start: Point<Pixels>,
    pressed_at: Instant,
    /// From the title (5 pt, no hold) or the bar's empty part (a hold).
    needs_hold: bool,
    mode: FloatMode,
    frame_at_start: dl::Rect,
    mouse_at_start: dl::Point,
    /// For moving the window by hand, on the Mac (`FloatMode::WindowMove`).
    grab: Option<platform::WindowGrab>,
    _timer: Option<Task<()>>,
}

#[derive(Clone, Copy, PartialEq)]
enum FloatMode {
    Waiting,
    /// Just moving the window: by hand on the Mac (`grab`), otherwise the OS
    /// is doing it and the release ends it.
    WindowMove,
    /// Moving it by hand, with the drop targets showing.
    Panel,
}

impl FloatingPanelView {
    fn new(panel: PanelId, model: Entity<DockModel>, content: SharedContent, options: Rc<DockOptions>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let observed = cx.observe(&model, |_, _, cx| cx.notify());
        // Where the user moves or resizes it (the OS's own drag from the
        // strip beside the traffic lights, or the window's edges), saved.
        // A drag by the title saves where it ends up when it's let go.
        let bounds = cx.observe_window_bounds(window, |this, window, cx| {
            let dragging = this.model.read(cx).controller.drag.as_ref().is_some_and(|d| d.panel == this.panel);
            if dragging {
                return;
            }
            let frame = Self::window_frame(window);
            let panel = this.panel.clone();
            this.model.update(cx, |m, cx| m.update_controller(cx, |c| c.remember_window_frame(&panel, frame)));
        });
        Self { panel, model, content, options, press: None, _subscriptions: vec![observed, bounds] }
    }

    /// Where the window is, and where the pointer is on screen.
    fn window_frame(window: &Window) -> dl::Rect {
        let b = window.bounds();
        dl::Rect::new(b.origin.x.as_f32() as f64, b.origin.y.as_f32() as f64, b.size.width.as_f32() as f64, b.size.height.as_f32() as f64)
    }

    fn mouse_on_screen(position: Point<Pixels>, window: &Window) -> dl::Point {
        let b = window.bounds();
        // These windows have full-size content: the frame's origin is the content's.
        dl::Point::new((b.origin.x + position.x).as_f32() as f64, (b.origin.y + position.y).as_f32() as f64)
    }

    fn begin_press(&mut self, needs_hold: bool, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.press.is_some() {
            return;
        }
        let timer = needs_hold.then(|| {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs_f64(dl::HOLD_SECONDS)).await;
                let _ = this.update_in(cx, |this, window, cx| this.hold_fired(window, cx));
            })
        });
        self.press = Some(FloatPress {
            escape_watch: None,
            start: event.position,
            pressed_at: Instant::now(),
            needs_hold,
            mode: FloatMode::Waiting,
            frame_at_start: Self::window_frame(window),
            mouse_at_start: Self::mouse_on_screen(event.position, window),
            grab: platform::grab_window(window),
            _timer: timer,
        });
        cx.notify();
    }

    fn hold_fired(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.press.as_ref().is_some_and(|p| p.mode == FloatMode::Waiting) {
            self.lift(window, cx);
        }
    }

    /// Becomes a panel drag: the window moves with the pointer from here on.
    fn lift(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let watch = cx.intercept_keystrokes(move |event, window, cx| {
            if event.keystroke.key == "escape" && weak.update(cx, |v, cx| v.escape(window, cx)).unwrap_or(false) {
                cx.stop_propagation();
            }
        });
        let Some(p) = &mut self.press else { return };
        p.mode = FloatMode::Panel;
        p._timer = None;
        p.escape_watch = Some(watch);
        let (frame, mouse) = (p.frame_at_start, p.mouse_at_start);
        let panel = self.panel.clone();
        let update = self.model.update(cx, |m, cx| m.update_controller(cx, |c| c.window_drag_changed(&panel, frame, mouse)));
        if let Some(u) = update {
            apply_window_update(window, u);
        }
    }

    fn pointer_moved(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        // The button came up without a release (between two frames): over.
        if event.pressed_button != Some(MouseButton::Left) {
            if self.press.is_some() {
                self.pointer_up(false, window, cx);
            }
            return;
        }
        let Some(p) = &mut self.press else { return };
        let dx = (event.position.x - p.start.x).as_f32() as f64;
        let dy = (event.position.y - p.start.y).as_f32() as f64;
        let moved = (dx * dx + dy * dy).sqrt() >= dl::DRAG_DISTANCE;
        if p.mode == FloatMode::Waiting && moved {
            let held = !p.needs_hold || p.pressed_at.elapsed().as_secs_f64() >= dl::HOLD_SECONDS;
            if held && platform::can_move_windows_by_hand() {
                self.lift(window, cx);
            } else if let Some(p) = &mut self.press {
                // Moving first (or where the window can't be moved by
                // hand): the OS's own window drag. No targets, but the
                // window still goes where it's dragged.
                p.mode = FloatMode::WindowMove;
                p._timer = None;
                if p.grab.is_none() {
                    window.start_window_move();
                }
            }
        }
        let Some(p) = &self.press else { return };
        if let (FloatMode::WindowMove, Some(grab)) = (p.mode, p.grab) {
            platform::move_grabbed_window(window, grab);
            return;
        }
        if p.mode == FloatMode::Panel {
            // Fresh from the OS rather than the event's own position, which
            // was worked out before we last moved the window (see platform.rs).
            let local = platform::pointer_in_window(window).unwrap_or(event.position);
            let mouse = Self::mouse_on_screen(local, window);
            let frame = p.frame_at_start;
            let panel = self.panel.clone();
            let update = self.model.update(cx, |m, cx| m.update_controller(cx, |c| c.window_drag_changed(&panel, frame, mouse)));
            if let Some(u) = update {
                apply_window_update(window, u);
            }
        }
    }

    /// `released`: a real mouse-up ended the press. Otherwise the button was
    /// found up on a later move (it was let go over another app), and the
    /// pointer has gone on since, so the window stays where the drag left it.
    fn pointer_up(&mut self, released: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.press.take() else { return };
        cx.notify();
        if let (FloatMode::WindowMove, Some(grab), true) = (p.mode, p.grab, released) {
            // Once more where it let go, as the main window's does.
            platform::move_grabbed_window(window, grab);
        }
        if p.mode == FloatMode::Panel {
            platform::set_window_alpha(window, 1.);
            let frame = Self::window_frame(window);
            let panel = self.panel.clone();
            self.model.update(cx, |m, cx| {
                m.update_controller(cx, |c| {
                    c.drag_ended();
                    // It stayed out of the dock: remember where it was left.
                    c.remember_window_frame(&panel, frame);
                })
            });
        } else if p.mode == FloatMode::Waiting {
            let frame = Self::window_frame(window);
            let panel = self.panel.clone();
            self.model.update(cx, |m, cx| m.update_controller(cx, |c| c.remember_window_frame(&panel, frame)));
        }
    }

    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(p) = &self.press else { return false };
        if p.mode != FloatMode::Panel {
            return false;
        }
        let frame = Self::window_frame(window);
        let back = self.model.update(cx, |m, cx| m.update_controller(cx, |c| c.cancel_drag(Some(frame))));
        platform::set_window_alpha(window, 1.);
        if let Some(from) = back {
            platform::set_window_origin(window, point(px(from.x as f32), px(from.y as f32)));
        }
        true
    }
}

fn apply_window_update(window: &Window, update: dl::WindowDragUpdate) {
    platform::set_window_origin(window, point(px(update.origin.x as f32), px(update.origin.y as f32)));
    platform::set_window_alpha(window, update.alpha as f64);
}

impl Render for FloatingPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = DockTheme::new(cx, self.options.accent);
        let panel = self.panel.clone();
        let content = self.content.clone();
        let title = content.title(&panel, cx);
        let icon = content.icon(&panel, window, cx);
        let accessory = content.accessory(&panel, window, cx);
        let body = content.view(&panel, false, window, cx);
        let body_menu = self.menu();
        let bar_menu = self.menu();
        let dragging = self.press.as_ref().is_some_and(|p| p.mode == FloatMode::Panel);
        let this = cx.entity().downgrade();

        let lights = 78.;
        let title_el = div()
            .id("dock-window-title")
            .px(px(8.))
            .flex_shrink_1()
            .min_w(px(0.))
            .text_size(px(self.options.title_size))
            .font_weight(FontWeight::SEMIBOLD)
            .truncate()
            .child(title.clone())
            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Drag to move, or onto the main window to dock it").build(window, cx))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, window, cx| this.begin_press(false, e, window, cx)))
            .context_menu(self.menu());

        let bar = h_flex()
            .flex_none()
            .w_full()
            .h(px(self.options.header_height))
            .pl(px(lights))
            .pr(px(6.))
            .gap(px(2.))
            .items_center()
            .border_b_1()
            .border_color(theme.bar_line)
            .when_some(icon, |d, i| d.child(i))
            .child(title_el)
            .child(
                div()
                    .id("dock-window-bar-space")
                    .flex_1()
                    .h_full()
                    .min_w(px(0.))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, window, cx| this.begin_press(true, e, window, cx)))
                    .context_menu(bar_menu),
            )
            .when_some(accessory, |d, a| d.child(a));

        div()
            .id("dock-window")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.panel)
            .text_color(theme.header_text)
            .child(bar)
            .child(
                div()
                    .id("dock-window-body")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .relative()
                    .child(body)
                    .context_menu(body_menu),
            )
            // Always there: a press and its release can land between frames.
            .child(float_listeners(this, dragging))
    }
}

impl FloatingPanelView {
    fn menu(&self) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let model = self.model.clone();
        let content = self.content.clone();
        let panel = self.panel.clone();
        move |menu, _, cx| {
            let hidden = model.read(cx).controller.layout.hidden_titles.contains(&panel);
            let (m1, p1) = (model.clone(), panel.clone());
            let mut menu = menu.item(PopupMenuItem::new(if hidden { "Show Title" } else { "Hide Title" }).on_click(move |_, _, cx| m1.update(cx, |m, cx| m.set_title_hidden(&p1, !hidden, cx))));
            let (m2, p2) = (model.clone(), panel.clone());
            menu = menu.item(PopupMenuItem::new("Put Back in Main Window").on_click(move |_, _, cx| m2.update(cx, |m, cx| m.put_back(&p2, cx))));
            if content.closable(&panel, cx) {
                let (m3, p3) = (model.clone(), panel.clone());
                let title = content.title(&panel, cx);
                menu = menu.separator().item(PopupMenuItem::new(format!("Close {title}")).on_click(move |_, _, cx| m3.update(cx, |m, cx| m.close(&p3, cx))));
            }
            menu
        }
    }
}

fn float_listeners(view: WeakEntity<FloatingPanelView>, dragging: bool) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            if dragging {
                window.set_window_cursor_style(CursorStyle::ClosedHand);
            }
            let moved = view.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    let _ = moved.update(cx, |v, cx| v.pointer_moved(e, window, cx));
                }
            });
            let released = view.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    let _ = released.update(cx, |v, cx| v.pointer_up(true, window, cx));
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
