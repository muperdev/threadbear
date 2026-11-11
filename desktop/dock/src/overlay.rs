// Drawn over the dock while a panel is moving: where it would land, the
// compass in the middle of a group, and the panel itself under the pointer.
// Ported from dock/Sources/BearDock/DragOverlay.swift. The springs run on GPUI's
// frames, computed from elapsed time (SpringConfig's closed form), so a busy
// main thread drops frames instead of running slowly: the same property the
// Swift ghost gets from withAnimation.
use crate::theme::DockTheme;
use crate::view::DockView;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::Instant;
use threadbear_dock_layout as dl;
use threadbear_dock_layout::{CompassItem, CompassState, DockDrop, DockEdge};

/// A spring from one value to another, as SwiftUI's
/// `spring(response:dampingFraction:)`, sampled by time. No interruption or
/// retargeting: each run is one flight.
#[derive(Clone, Copy)]
pub struct SpringRun {
    config: SpringConfig,
    from: f32,
    to: f32,
    started: Instant,
}

/// SwiftUI's response and damping fraction as GPUI's physical spring: response
/// is the period of the undamped spring, so ω = 2π / response.
fn spring_config(spring: dl::Spring) -> SpringConfig {
    let omega = (2.0 * std::f64::consts::PI) / spring.response;
    SpringConfig::new((omega * omega) as f32, (2.0 * spring.damping_fraction * omega) as f32, 1.0)
}

impl SpringRun {
    pub fn new(spring: dl::Spring, from: f32, to: f32) -> Self {
        Self { config: spring_config(spring), from, to, started: Instant::now() }
    }

    /// Where it is now. Computed from the elapsed time in one analytic step.
    pub fn value(&self, now: Instant) -> f32 {
        let t = now.saturating_duration_since(self.started).as_secs_f32();
        self.config.step(SpringState { position: self.from, velocity: 0. }, self.to, t).position
    }

    pub fn settled(&self, now: Instant) -> bool {
        let t = now.saturating_duration_since(self.started).as_secs_f32();
        let s = self.config.step(SpringState { position: self.from, velocity: 0. }, self.to, t);
        (s.position - self.to).abs() < 0.002 && s.velocity.abs() < 0.02
    }
}

/// A rect moving to another over a set time, ease-out (the drop preview's
/// `Animation.snappy(duration: 0.16)`).
#[derive(Clone, Copy)]
pub struct Tween {
    from: dl::Rect,
    to: dl::Rect,
    started: Instant,
    seconds: f64,
}

impl Tween {
    pub fn new(from: dl::Rect, to: dl::Rect, seconds: f64) -> Self {
        Self { from, to, started: Instant::now(), seconds }
    }

    pub fn value(&self, now: Instant) -> dl::Rect {
        if self.seconds <= 0. {
            return self.to;
        }
        let t = (now.saturating_duration_since(self.started).as_secs_f64() / self.seconds).min(1.);
        // Ease-out, the snappy shape: fast then settling.
        let p = 1. - (1. - t).powi(3);
        lerp_rect(self.from, self.to, p)
    }

    pub fn settled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started).as_secs_f64() >= self.seconds
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn lerp_rect(a: dl::Rect, b: dl::Rect, t: f64) -> dl::Rect {
    dl::Rect::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t), lerp(a.width, b.width, t), lerp(a.height, b.height, t))
}

/// The ghost flying to where the panel landed, or home.
#[derive(Clone)]
pub struct LandingRun {
    pub from: dl::Rect,
    pub to: dl::Rect,
    pub panel: String,
    pub source_size: dl::Size,
    spring: SpringRun,
    /// Set when the 300 ms is up and the ghost starts fading.
    pub fading_since: Option<Instant>,
}

impl LandingRun {
    pub fn new(from: dl::Rect, to: dl::Rect, panel: String, source_size: dl::Size) -> Self {
        Self { from, to, panel, source_size, spring: SpringRun::new(dl::SETTLE, 0., 1.), fading_since: None }
    }

    fn progress(&self, now: Instant) -> f64 {
        self.spring.value(now) as f64
    }
}

/// Lays the child out with rems scaled by `scale`, so what's sized in rems
/// shrinks with the ghost. Zed's `WithRemSize` does this for zoom. GPUI can't
/// transform an arbitrary subtree, so this is as close as the ghost gets to
/// "drawn at its real size and scaled": content sized in pixels stays its size
/// and is clipped by the ghost's rounded corners.
struct RemScaled {
    child: AnyElement,
    scale: f32,
}

impl IntoElement for RemScaled {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for RemScaled {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let rem = window.rem_size() * self.scale;
        let child = &mut self.child;
        (window.with_rem_size(Some(rem), |window| child.request_layout(window, cx)), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) {
        let rem = window.rem_size() * self.scale;
        let child = &mut self.child;
        window.with_rem_size(Some(rem), |window| {
            child.prepaint(window, cx);
        });
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), _: &mut (), window: &mut Window, cx: &mut App) {
        let rem = window.rem_size() * self.scale;
        let child = &mut self.child;
        window.with_rem_size(Some(rem), |window| child.paint(window, cx));
    }
}

/// The drop preview, ghost and compass, positioned in the dock's coordinates.
pub fn overlay(view: &mut DockView, theme: &DockTheme, window: &mut Window, cx: &mut Context<DockView>) -> AnyElement {
    let now = Instant::now();
    let controller = view.model.read(cx).controller.clone();
    let mut any_running = false;
    let mut root = div().absolute().top_0().left_0().size_full();

    // The accent-tinted space the panel would take.
    if let (Some(tween), Some(target)) = (&view.preview, controller.drag.as_ref().and_then(|d| d.target.as_ref()).filter(|t| t.drop.is_some())) {
        any_running |= !tween.settled(now);
        let rect = tween.value(now);
        let is_tab = matches!(target.drop, Some(DockDrop::Tab { .. }));
        let panel = controller.drag.as_ref().map(|d| d.panel.clone());
        root = root.child(drop_preview(view, rect, is_tab, target.in_bar, panel.as_deref(), theme, window, cx));
    }

    // The ghost: the panel under the pointer, or flying to where it landed.
    let ghost: Option<(String, dl::Size, dl::Rect, f32, f32, Option<f32>)> = if let Some(d) = controller.drag.as_ref().filter(|d| d.window.is_none()) {
        let lift = view.lift.as_ref().map(|r| r.value(now)).unwrap_or(1.);
        any_running |= view.lift.as_ref().is_some_and(|r| !r.settled(now));
        // See-through over the compass, so its choices stay readable, and
        // gone while it would open as a window: the window's outline shows
        // where instead.
        let target = d.target.as_ref();
        let opacity = if target.is_some_and(|t| t.compass.is_some()) {
            0.45
        } else if target.is_some_and(|t| t.float.is_some()) {
            0.
        } else {
            1.
        };
        Some((d.panel.clone(), d.source_frame.size(), dl::DockController::ghost_rect(d, lift as f64), lift, opacity, None))
    } else if let Some(run) = &view.landing {
        let progress = run.progress(now) as f64;
        // The spring's overshoot carries a little past the spot; that's the settle.
        let rect = lerp_rect(run.from, run.to, progress);
        let fade = run.fading_since.map(|since| (now.saturating_duration_since(since).as_secs_f64() / dl::LANDING_FADE_SECONDS).min(1.) as f32).unwrap_or(0.);
        any_running = true;
        // The text grows back to its own size as it lands, so the landing
        // matches the real panel it cross-fades into, even when the new spot
        // is a different width from the one it left.
        let start_scale = dl::DockController::ghost_scale(run.source_size) as f32;
        let content_scale = start_scale + (1. - start_scale) * (progress as f32).clamp(0., 1.);
        Some((run.panel.clone(), run.source_size, rect, 0., 1. - fade, Some(content_scale)))
    } else {
        None
    };
    if let Some((panel, source, rect, lift, opacity, content_scale)) = ghost {
        if opacity > 0.001 {
            root = root.child(ghost_element(view, &panel, source, rect, lift, opacity, content_scale, theme, window, cx));
        }
    }

    // Over the ghost: the pointer is on it, so the ghost would hide it.
    if let Some(compass) = controller.drag.as_ref().and_then(|d| d.target.as_ref()).and_then(|t| t.compass.as_ref()) {
        root = root.child(compass_element(compass, theme));
    }

    if any_running {
        window.request_animation_frame();
    }
    root.into_any_element()
}

/// A rounded card tilted about its centre, for the ghost's plate. GPUI can
/// rotate a path but not a subtree of elements, so only the plate tilts; the
/// content sits inside it, inset a little, level.
fn paint_tilted_card(bounds: Bounds<Pixels>, radius: f32, degrees: f32, fill: Hsla, stroke: Hsla, window: &mut Window) {
    let (w, h) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
    let (hw, hh) = (w / 2., h / 2.);
    let r = radius.min(hw).min(hh);
    let mut points = Vec::with_capacity(4 * 9);
    let steps = 8;
    let corners = [(hw - r, -hh + r, -90f32), (hw - r, hh - r, 0.), (-hw + r, hh - r, 90.), (-hw + r, -hh + r, 180.)];
    for (cx_, cy, start) in corners {
        for i in 0..=steps {
            let a = (start + 90. * i as f32 / steps as f32).to_radians();
            points.push(point(px(cx_ + r * a.cos()), px(cy + r * a.sin())));
        }
    }
    let center = bounds.center();
    let mut plate = PathBuilder::fill();
    plate.add_polygon(&points, true);
    plate.rotate(degrees);
    plate.translate(center);
    if let Ok(path) = plate.build() {
        window.paint_path(path, fill);
    }
    let mut edge = PathBuilder::stroke(px(1.));
    edge.add_polygon(&points, true);
    edge.rotate(degrees);
    edge.translate(center);
    if let Ok(path) = edge.build() {
        window.paint_path(path, stroke);
    }
}

/// The panel itself, picked up: drawn at its real size and scaled, so it
/// shrinks rather than reflowing, with the grab point under the pointer.
fn ghost_element(view: &DockView, panel: &str, source: dl::Size, rect: dl::Rect, lift: f32, opacity: f32, content_scale: Option<f32>, theme: &DockTheme, window: &mut Window, cx: &mut Context<DockView>) -> AnyElement {
    let lift = lift.clamp(0., 1.2);
    let s = content_scale.unwrap_or((rect.width / source.width.max(1.)) as f32);
    let header_h = view.options.header_height * s;
    let title_size = view.options.title_size * s;
    let content = view.content.clone();
    let title = content.title(panel, cx);
    let icon = content.icon(panel, window, cx);
    let body = content.view(panel, true, window, cx);
    let shadow_alpha = 0.5 * lift.min(1.);
    // Tilted by the plate only: GPUI can't rotate elements (see paint_tilted_card).
    let degrees = dl::GHOST_TILT_DEGREES as f32 * lift.clamp(0., 1.);
    let (plate_fill, plate_edge) = (theme.panel, theme.ghost_border);

    div()
        .absolute()
        .left(px(rect.x as f32))
        .top(px(rect.y as f32))
        .w(px(rect.width as f32))
        .h(px(rect.height as f32))
        .opacity(opacity)
        .rounded(px(10.))
        .shadow(vec![BoxShadow { color: hsla(0., 0., 0., shadow_alpha), offset: point(px(0.), px(12. * lift)), blur_radius: px(24. * lift), spread_radius: px(0.), inset: false }])
        .child(canvas(|_, _, _| {}, move |bounds, _, window, _| paint_tilted_card(bounds, 10., degrees, plate_fill, plate_edge, window)).absolute().top_0().left_0().size_full())
        .child(
            div()
                .absolute()
                .top(px(3.))
                .left(px(3.))
                .right(px(3.))
                .bottom(px(3.))
                .rounded(px(8.))
                .overflow_hidden()
                .bg(theme.panel)
                .flex()
                .flex_col()
                .child(
            div()
                .flex()
                .flex_none()
                .flex_row()
                .items_center()
                .gap(px(6. * s))
                .h(px(header_h))
                .px(px(14. * s))
                .border_b_1()
                .border_color(theme.bar_line)
                .text_color(theme.secondary)
                .when_some(icon, |d, i| d.child(RemScaled { child: i, scale: s }))
                .child(div().text_size(px(title_size)).font_weight(FontWeight::SEMIBOLD).text_color(theme.header_text).child(title)),
                )
                .child(div().flex_1().min_h(px(0.)).overflow_hidden().relative().child(RemScaled { child: body, scale: s })),
        )
        .into_any_element()
}

/// The accent-tinted space the panel would take, or the new tab itself, in a bar.
fn drop_preview(view: &DockView, rect: dl::Rect, is_tab: bool, in_bar: bool, panel: Option<&str>, theme: &DockTheme, window: &mut Window, cx: &mut Context<DockView>) -> AnyElement {
    if in_bar {
        let content = view.content.clone();
        let icon = panel.and_then(|p| content.icon(p, window, cx));
        let title = panel.map(|p| content.title(p, cx));
        return div()
            .absolute()
            .left(px(rect.x as f32))
            .top(px(rect.y as f32))
            .w(px(rect.width.max(0.) as f32))
            .h(px(rect.height.max(0.) as f32))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .rounded(px(6.))
            .bg(theme.accent.opacity(0.22))
            .border(px(1.5))
            .border_color(theme.accent.opacity(0.85))
            .text_color(theme.header_text)
            .when_some(icon, |d, i| d.child(i))
            .when_some(title, |d, t| d.child(div().text_size(px(view.options.title_size)).font_weight(FontWeight::SEMIBOLD).truncate().child(t)))
            .into_any_element();
    }
    let r = rect.inset(4., 4.);
    div()
        .absolute()
        .left(px(r.x as f32))
        .top(px(r.y as f32))
        .w(px(r.width.max(0.) as f32))
        .h(px(r.height.max(0.) as f32))
        .rounded(px(8.))
        .bg(theme.accent.opacity(0.14))
        .border(px(1.5))
        .border_color(theme.accent.opacity(0.75))
        .when(is_tab, |d| {
            // A tab-shaped nub, so "join as a tab" reads differently from "replace".
            d.child(
                div()
                    .absolute()
                    .left(px(8.))
                    .top(px(6.))
                    .w(px((r.width * 0.4).min(90.) as f32))
                    .h(px(18.))
                    .rounded(px(5.))
                    .bg(theme.accent.opacity(0.55)),
            )
        })
        .into_any_element()
}

/// The cross of choices in the middle of a group: dock to a side of it, join
/// it as a tab, or swap places.
fn compass_element(state: &CompassState, theme: &DockTheme) -> AnyElement {
    let items = dl::DockController::compass_items(state.center);
    let area = items.iter().fold(items[0].1, |a, (_, r)| a.union(r));
    let caption = state.hovered.unwrap_or(CompassItem::Tab).label();
    let caption_rect = dl::Rect::new(area.mid_x() - 70., area.max_y() + 4., 140., 26.);
    let plate = area.inset(-10., -10.).union(&caption_rect);

    let mut root = div().absolute().top_0().left_0().size_full().child(
        div()
            .absolute()
            .left(px(plate.x as f32))
            .top(px(plate.y as f32))
            .w(px(plate.width as f32))
            .h(px(plate.height as f32))
            .rounded(px(14.))
            .bg(theme.plate)
            .border_1()
            .border_color(theme.plate_border)
            .shadow(vec![BoxShadow { color: hsla(0., 0., 0., 0.45), offset: point(px(0.), px(8.)), blur_radius: px(18.), spread_radius: px(0.), inset: false }]),
    );
    for (item, rect) in items {
        let hovered = state.hovered == Some(item) || (state.hovered.is_none() && item == CompassItem::Tab);
        root = root.child(compass_button(item, rect, hovered, state.disabled.contains(&item), theme));
    }
    root = root.child(
        div()
            .absolute()
            .left(px((area.mid_x() - 70.) as f32))
            .top(px((area.max_y() + 10.) as f32))
            .w(px(140.))
            .flex()
            .justify_center()
            .text_size(px(11.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.secondary)
            .child(caption),
    );
    root.into_any_element()
}

fn compass_button(item: CompassItem, rect: dl::Rect, hovered: bool, disabled: bool, theme: &DockTheme) -> AnyElement {
    let lit = hovered && !disabled;
    let fg = if lit { hsla(0., 0., 1., 1.) } else { theme.secondary };
    let fill = if lit { theme.accent.opacity(0.9) } else { theme.header_text.opacity(0.07) };
    let glyph: AnyElement = match item {
        CompassItem::Swap => label_glyph("Swap", Lucide::Replace, fg),
        CompassItem::Window => label_glyph("Window", Lucide::ExternalLink, fg),
        CompassItem::Tab => tab_glyph(fg),
        CompassItem::Split(edge) => split_glyph(edge, fg),
    };
    div()
        .absolute()
        .left(px(rect.x as f32))
        .top(px(rect.y as f32))
        .w(px(rect.width as f32))
        .h(px(rect.height as f32))
        .rounded(px(8.))
        .bg(fill)
        .flex()
        .items_center()
        .justify_center()
        .opacity(if disabled { 0.3 } else { 1. })
        .child(glyph)
        .into_any_element()
}

fn label_glyph(text: &'static str, icon: Lucide, color: Hsla) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(svg().path(icon.path()).size(px(12.)).text_color(color))
        .child(text)
        .into_any_element()
}

/// A little window with a tab on top.
fn tab_glyph(color: Hsla) -> AnyElement {
    div()
        .relative()
        .w(px(18.))
        .h(px(18.))
        .child(div().absolute().left_0().top(px(4.)).w(px(18.)).h(px(14.)).rounded(px(2.5)).border(px(1.4)).border_color(color))
        .child(div().absolute().left(px(1.)).top_0().w(px(8.)).h(px(5.)).rounded(px(1.5)).bg(color))
        .into_any_element()
}

/// A little window with the half it would take filled in.
fn split_glyph(edge: DockEdge, color: Hsla) -> AnyElement {
    let (w, h) = (18., 16.);
    let r = dl::DockController::slice(dl::Rect::new(0., 0., w, h), edge, 0.5).inset(2.5, 2.5);
    div()
        .relative()
        .w(px(w as f32))
        .h(px(h as f32))
        .child(div().absolute().size_full().rounded(px(2.5)).border(px(1.4)).border_color(color))
        .child(div().absolute().left(px(r.x as f32)).top(px(r.y as f32)).w(px(r.width as f32)).h(px(r.height as f32)).rounded(px(1.)).bg(color))
        .into_any_element()
}
