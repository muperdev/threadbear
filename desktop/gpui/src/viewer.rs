// A viewer for pictures in the transcript, the in-app replacement for Quick
// Look on Windows and Linux (and the Mac, for now): click a tile for an
// overlay with arrows through that message's pictures, Escape closes
// (docs/gpui-port/mac-only-parts.md, "Quick Look").
//
// The security rule carries over from the Swift app's `quickLooks` (Codex review,
// 7 Nov): only a real, decodable picture is ever shown, and never handed to
// its default app, so a `.command` posing as an image can't run. The image
// is decoded from the bytes here, not opened by the system.
use crate::highlights::{icon, SidePane};
use crate::style;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
// Shadows gpui's px, so sizes here follow the app's zoom (style.rs).
use crate::style::px;
use serde_json::{json, Value};
use std::sync::Arc;
use threadbear_core::folders::picture_size;
use threadbear_core::highlights::{looks_like_picture, runs_code};
use threadbear_core::reducer::ImageRef;

/// What's open in the viewer.
pub(crate) struct ViewerState {
    /// Each picture's path in this message, and its bytes once it's been read.
    pictures: Vec<(String, Option<Arc<Image>>, Option<(u32, u32)>)>,
    index: usize,
    open: bool,
    /// Holds the keys while it's up: Escape and the arrows.
    focus: FocusHandle,
}

impl ViewerState {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self { pictures: vec![], index: 0, open: false, focus: cx.focus_handle() }
    }
}

impl ViewerState {
    fn current_path(&self) -> Option<&str> {
        self.pictures.get(self.index).map(|p| p.0.as_str())
    }
}

/// A handle the transcript's rows use to open the viewer. One per window:
/// the tiles are drawn by the thread pane, the viewer by the side pane.
#[derive(Clone)]
pub(crate) struct ViewerHandle(pub WeakEntity<SidePane>);
impl Global for ViewerHandle {}

/// Whether a tile can open in the viewer at all (`quickLooks`): named like a
/// picture, and not something whose default app runs code.
fn opens_here(path: &str) -> bool {
    looks_like_picture(path) && !runs_code(path)
}

/// A click on a tile: the viewer on that picture, stepping through the
/// message's other real pictures.
pub(crate) fn open_from_tile(images: &[ImageRef], index: usize, window: &mut Window, cx: &mut App) {
    let Some(handle) = cx.try_global::<ViewerHandle>().cloned() else { return };
    let paths: Vec<String> = images.iter().map(|i| i.path.clone()).filter(|p| opens_here(p)).collect();
    let Some(chosen) = images.get(index).map(|i| i.path.clone()).filter(|p| paths.contains(p)) else { return };
    handle
        .0
        .update(cx, |pane, cx| pane.open_viewer(paths, &chosen, window, cx))
        .ok();
}

impl SidePane {
    pub(crate) fn open_viewer(&mut self, paths: Vec<String>, chosen: &str, window: &mut Window, cx: &mut Context<Self>) {
        let index = paths.iter().position(|p| p == chosen).unwrap_or(0);
        self.viewer.pictures = paths.into_iter().map(|p| (p, None, None)).collect();
        self.viewer.index = index;
        self.viewer.open = true;
        // The overlay holds the keys: Escape closes, the arrows step.
        window.focus(&self.viewer.focus, cx);
        self.read_viewer_picture(index, cx);
        cx.notify();
    }

    pub(crate) fn close_viewer(&mut self, cx: &mut Context<Self>) {
        self.viewer.open = false;
        cx.notify();
    }

    pub(crate) fn viewer_step(&mut self, delta: i64, cx: &mut Context<Self>) {
        let count = self.viewer.pictures.len() as i64;
        if !self.viewer.open || count == 0 {
            return;
        }
        let next = (self.viewer.index as i64 + delta).rem_euclid(count) as usize;
        self.viewer.index = next;
        self.read_viewer_picture(next, cx);
        cx.notify();
    }

    /// Reads the picture through the runner, so another Mac's works the same.
    /// Anything that doesn't decode just shows "Couldn't show this picture":
    /// the bytes are never handed to another app.
    fn read_viewer_picture(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((path, image, _)) = self.viewer.pictures.get(index) else { return };
        if image.is_some() {
            return;
        }
        let path = path.clone();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = crate::call(client, "files.read", json!({ "path": path })).await;
            let read = res.ok().as_ref().and_then(threadbear_core::folders::FileRead::decode);
            this.update(cx, |t, cx| {
                let Some(slot) = t.viewer.pictures.iter_mut().find(|p| p.0 == path) else { return };
                if let Some(read) = read {
                    let format = format_of(&path, &read.data);
                    if let Some(format) = format {
                        slot.2 = picture_size(&read.data);
                        slot.1 = Some(Arc::new(Image::from_bytes(format, read.data)));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The overlay, drawn over the whole window while it's open.
    pub(crate) fn viewer_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.viewer.open {
            return None;
        }
        let bounds = window.viewport_size();
        let (index, total) = (self.viewer.index, self.viewer.pictures.len());
        let current = self.viewer.pictures.get(index)?;
        let (path, image) = (current.0.clone(), current.1.clone());
        let name = threadbear_core::highlights::last_component(&path).to_owned();
        let backdrop = gpui_kit::hsla(0., 0., 0., 0.82);
        let fg = gpui_kit::hsla(0., 0., 1., 0.92);
        let dim = gpui_kit::hsla(0., 0., 1., 0.55);
        let body: AnyElement = match image {
            Some(image) => img(ImageSource::Image(image)).max_w(bounds.width - px(120.)).max_h(bounds.height - px(120.)).object_fit(ObjectFit::Contain).into_any_element(),
            None => div().text_size(px(style::size::LABEL)).text_color(dim).child(if self.viewer_loading(index) { "Loading…".to_owned() } else { format!("Couldn't show {name}") }).into_any_element(),
        };
        let arrow = |id: &'static str, symbol: &'static str, delta: i64, tip: &'static str, cx: &mut Context<Self>| {
            crate::highlights::tipped(
                div().id(id).size(px(44.)).flex().items_center().justify_center().rounded_full().bg(gpui_kit::hsla(0., 0., 1., 0.1)).hover(|s| s.bg(gpui_kit::hsla(0., 0., 1., 0.2))).cursor_pointer().child(icon(symbol, 18., fg)),
                tip,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                // Not the backdrop's click, which closes it.
                cx.stop_propagation();
                this.viewer_step(delta, cx);
            }))
            .into_any_element()
        };
        let many = total > 1;
        let prev = many.then(|| arrow("viewer-prev", "chevron.left", -1, "Previous picture", cx));
        let next = many.then(|| arrow("viewer-next", "chevron.right", 1, "Next picture", cx));
        let close = crate::highlights::tipped(div().id("viewer-close").size(px(32.)).flex().items_center().justify_center().rounded_full().bg(gpui_kit::hsla(0., 0., 1., 0.12)).hover(|s| s.bg(gpui_kit::hsla(0., 0., 1., 0.24))).cursor_pointer().child(icon("xmark", 14., fg)), "Close")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_viewer(cx)));
        let caption = if total > 1 { format!("{name}  ·  {} of {total}", index + 1) } else { name };
        let overlay = div()
            .id("picture-viewer")
            .track_focus(&self.viewer.focus)
            .w(bounds.width)
            .h(bounds.height)
            .bg(backdrop)
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| match ev.keystroke.key.as_str() {
                "escape" => this.close_viewer(cx),
                "left" => this.viewer_step(-1, cx),
                "right" => this.viewer_step(1, cx),
                _ => {}
            }))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_viewer(cx)))
            .child(
                h_flex()
                    .size_full()
                    .items_center()
                    .justify_between()
                    .px(px(16.))
                    .child(div().w(px(44.)).when_some(prev, |s, b| s.child(b)))
                    // Clicks on the picture itself don't close it.
                    .child(v_flex().flex_1().min_w_0().items_center().gap(px(10.)).child(div().id("viewer-body").on_click(|_, _, cx| cx.stop_propagation()).child(body)).child(div().text_size(px(style::size::DETAIL)).text_color(dim).child(caption)))
                    .child(div().w(px(44.)).when_some(next, |s, b| s.child(b))),
            )
            .child(div().absolute().top(px(16.)).right(px(16.)).child(close));
        Some(deferred(anchored().position(point(px(0.), px(0.))).child(overlay)).with_priority(2).into_any_element())
    }

    fn viewer_loading(&self, index: usize) -> bool {
        // A picture only reads once; "Couldn't" after that means it failed.
        self.viewer.pictures.get(index).is_some_and(|p| p.1.is_none())
    }
}

/// The image format from its bytes, so a mislabelled file still decodes as
/// what it really is, and a non-picture is refused.
fn format_of(path: &str, bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG") {
        return Some(ImageFormat::Png);
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        return Some(ImageFormat::Jpeg);
    }
    if bytes.starts_with(b"GIF8") {
        return Some(ImageFormat::Gif);
    }
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(ImageFormat::Webp);
    }
    if bytes.starts_with(b"BM") {
        return Some(ImageFormat::Bmp);
    }
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return Some(ImageFormat::Tiff);
    }
    // An SVG is text, and only ever drawn, never run.
    (path.to_lowercase().ends_with(".svg") && String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).contains("<svg")).then_some(ImageFormat::Svg)
}

/// `viewer` for scripts: says what's open, and `step`, `close`, `open` act.
#[cfg(feature = "automation")]
pub(crate) fn viewer_command(side: &Entity<SidePane>, cmd: &Value, cx: &mut App) -> Result<Value, String> {
    if let Some(delta) = cmd["step"].as_i64() {
        side.update(cx, |pane, cx| pane.viewer_step(delta, cx));
    }
    if cmd["close"].as_bool() == Some(true) {
        side.update(cx, |pane, cx| pane.close_viewer(cx));
    }
    let pane = side.read(cx);
    let viewer = &pane.viewer;
    if !viewer.open {
        return Ok(json!("ok closed"));
    }
    let state = viewer.pictures.get(viewer.index);
    let drawn = state.is_some_and(|p| p.1.is_some());
    Ok(json!(format!("ok showing {} of {} {}{}", viewer.index + 1, viewer.pictures.len(), viewer.current_path().unwrap_or(""), if drawn { "" } else { " (loading)" })))
}

/// The thread's action chips, for the thread pane to draw over the composer
/// (the chips live with the patches, patches.rs).
pub(crate) fn action_chips(thread: &threadbear_core::inbox::ThreadSummary, cx: &mut App) -> Option<AnyElement> {
    if !SidePane::has_action_chips(thread) {
        return None;
    }
    let handle = cx.try_global::<ViewerHandle>()?.clone();
    let entity = handle.0.upgrade()?;
    entity.update(cx, |pane, cx| pane.render_action_chips(thread, cx))
}

/// The tile's tooltip words on a transcript row (`transcript.images`).
pub(crate) const TILE_TIP: &str = "Open the picture";

pub(crate) fn viewer_handle_global(entity: WeakEntity<SidePane>, cx: &mut App) {
    cx.set_global(ViewerHandle(entity));
}

/// Value only used by the automation build.
#[cfg(not(feature = "automation"))]
#[allow(dead_code)]
fn _unused(_: &Value) {}
