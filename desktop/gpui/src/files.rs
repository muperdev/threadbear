// A file or page open in the pane, in place of its section: a port of the Swift
// app's PreviewBody, QuickLookView/FilePreviewView and TextFileView. What the
// file holds always comes from the runner (`files.read`), so a thread on
// another Mac works the same; nothing here reads this Mac's own disk.
//
// A picture is drawn here with Copy Image; Markdown drawn as a page; code and
// plain text in the kit's read-only editor, coloured by its highlighter. PDFs
// and everything else say they open in their app, as this first pass has no
// viewer for them (`file.quicklook`), and web pages go to the browser (`file.htmlpreview`,
// `panel.web` are later work).
use crate::highlights::{palette, Palette, PanePreview, SidePane, Target, FILES};
use crate::{call, style};
use gpui_kit::base::{TextViewState, TextViewStyle};
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::{h_flex, spinner::Spinner, v_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// Shadows gpui's px, so sizes here follow the app's zoom (style.rs).
use crate::style::px;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use threadbear_core::folders::{picture_size, FileRead};
use threadbear_core::highlights::{self as core, Viewer};
use threadbear_core::inbox::ThreadSummary;

/// Whether the system has "Finder", for the menu's words.
#[cfg(target_os = "macos")]
pub(crate) const REVEAL_TITLE: &str = "Reveal in Finder";
#[cfg(not(target_os = "macos"))]
pub(crate) const REVEAL_TITLE: &str = "Show in folder";

pub(crate) const OPEN_IN_BROWSER: &str = "Open in Browser";

/// The bytes of a file come from the runner; this is how much a preview asks for
/// (`files.read`'s own default is 16 MB, its limit 48).
const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Past this much text, Markdown and code are shown plain: highlighting
/// a huge file line by line would stall (`TextFileController.highlightLimit`).
const HIGHLIGHT_LIMIT: usize = 400_000;

/// What the pane's reading: one open at a time per thread.
pub(crate) struct OpenFile {
    pub path: String,
    pub opened: u64,
    pub body: FileBody,
    /// Only the start of the file came (`truncated`).
    pub truncated: bool,
}

#[derive(Default)]
pub(crate) struct FilesState {
    /// The file open in the pane, per thread.
    pub open: HashMap<String, OpenFile>,
    /// Paths the runner has said are folders, so a click goes to the browser.
    known_folders: HashSet<String>,
    /// The pane's own text style, built once per appearance.
    reading_style: Option<(bool, TextViewStyle)>,
}

impl FilesState {
    pub(crate) fn is_known_folder(&self, path: &str) -> bool {
        self.known_folders.contains(path)
    }
}

/// What's drawn for a file.
pub(crate) enum FileBody {
    /// A spinner would only flash for a normal file; a beat later there's a note.
    Loading,
    Failed { message: String },
    Markdown { doc: Entity<TextViewState>, text: Arc<str> },
    /// Code or plain text: a read-only editor, coloured for code. The editor
    /// is made at the next frame, as it needs a window.
    Text { editor: Option<Entity<EditorState>>, text: Arc<str>, language: Option<&'static str> },
    Picture { image: Arc<Image>, size: Option<(u32, u32)> },
    /// Something only its own app can show.
    Opaque { note: String },
}

/// An image's format from its name, for the kit's decoder.
fn image_format(path: &str) -> Option<ImageFormat> {
    let ext = path.rsplit('.').next()?.to_lowercase();
    Some(match ext.as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "gif" => ImageFormat::Gif,
        "webp" => ImageFormat::Webp,
        "bmp" => ImageFormat::Bmp,
        "tif" | "tiff" => ImageFormat::Tiff,
        "svg" => ImageFormat::Svg,
        _ => return None,
    })
}

/// `%20` and the like, in a file link.
pub(crate) fn percent_decode(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 1 {
            if let Some(v) = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn home_dir() -> String {
    // Windows sets USERPROFILE; HOME only exists there under Git Bash.
    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default()
}

/// A web page in the browser (the panel page is later work). Only http and
/// https: anything else is refused by the open gate, said so.
pub(crate) fn open_web(url: &str, cx: &mut App) {
    crate::open_gate::open_web(url, cx);
}

/// A file on this runner in its own app, through the open gate: a document
/// opens, a program asks first (OpenGate's rule).
pub(crate) fn open_file_in_app(path: &str, origin: crate::open_gate::Origin, cx: &mut App) {
    crate::open_gate::open_file(path, origin, cx);
}

pub(crate) fn reveal(path: &str, origin: crate::open_gate::Origin, cx: &mut App) {
    crate::open_gate::reveal(path, origin, cx);
}

/// A quiet line in the middle of the pane (`PaneNote`).
pub(crate) fn pane_note(text: &str, p: Palette) -> impl IntoElement {
    v_flex().size_full().items_center().justify_center().p(px(24.)).child(div().text_center().text_size(px(style::size::LABEL)).text_color(p.secondary).child(text.to_owned()))
}

/// An ask's words without their markdown (`plainAction`).
pub(crate) fn plain_markdown(text: &str) -> String {
    threadbear_core::sidebar::preview_plain(text)
}

impl SidePane {
    /// Reads the file the pane's preview names, if it's one that shows here.
    pub(crate) fn begin_preview(&mut self, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let Some(preview) = self.previews.get(&thread.id).cloned() else { return };
        match &preview.target {
            Target::File(path) => match core::viewer_for_path(path) {
                // A page in the system browser for now, and the preview
                // closes. Another Mac's file has no copy here to open.
                Viewer::Web if thread.is_remote() => {
                    let note = "Web pages from another Mac can't be opened from here yet.".to_owned();
                    self.files.open.insert(thread.id.clone(), OpenFile { path: path.clone(), opened: preview.opened, body: FileBody::Opaque { note }, truncated: false });
                    cx.notify();
                }
                // The real file, after its symlinks: a page named .html that's
                // really something else asks first, as any file does.
                Viewer::Web => {
                    open_file_in_app(path, crate::open_gate::Origin::ThisMac, cx);
                    self.previews.remove(&thread.id);
                    cx.notify();
                }
                _ => self.load_file(thread, path.clone(), preview.opened, cx),
            },
            Target::Page(url) => {
                open_web(url, cx);
                self.previews.remove(&thread.id);
                cx.notify();
            }
            Target::Folder(_) => self.load_folder_for_preview(thread, cx),
        }
    }

    fn load_file(&mut self, thread: &ThreadSummary, path: String, opened: u64, cx: &mut Context<Self>) {
        // The same open again (a repaint, a re-select) reads nothing new;
        // opening it again from a row does (`opened` moved).
        if self.files.open.get(&thread.id).is_some_and(|f| f.path == path && f.opened == opened) {
            return;
        }
        let keep_previous = self.files.open.get(&thread.id).is_some_and(|f| f.path == path);
        if !keep_previous {
            self.files.open.insert(thread.id.clone(), OpenFile { path: path.clone(), opened, body: FileBody::Loading, truncated: false });
            cx.notify();
        }
        let client = self.client.clone();
        let thread_id = thread.id.clone();
        let mut params = json!({ "path": path, "maxBytes": MAX_BYTES });
        if let Some(runner) = &thread.runner_id {
            params["runnerId"] = json!(runner);
        }
        cx.spawn(async move |this, cx| {
            let res = call(client, "files.read", params).await;
            this.update(cx, |this, cx| {
                // A newer file opened while this one was on its way.
                let current = this.previews.get(&thread_id).and_then(|p| p.file_path().map(str::to_owned));
                if current.as_deref() != Some(path.as_str()) {
                    return;
                }
                match res {
                    Ok(v) => match FileRead::decode(&v) {
                        Some(read) => this.show_read(&thread_id, &path, opened, read, cx),
                        None => this.fail_file(&thread_id, &path, opened, "Couldn't read that answer from the runner.".into(), cx),
                    },
                    Err(e) if e.message.starts_with("Not a file") => {
                        // A folder: into the browser, as a click on one is.
                        this.files.known_folders.insert(path.clone());
                        this.files.open.remove(&thread_id);
                        if let Some(p) = this.previews.get_mut(&thread_id) {
                            p.target = Target::Folder(path.clone());
                        }
                        if let Some(thread) = this.thread(cx) {
                            this.begin_preview(&thread, cx);
                        }
                        cx.notify();
                    }
                    Err(e) => this.fail_file(&thread_id, &path, opened, e.message, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn fail_file(&mut self, thread_id: &str, path: &str, opened: u64, message: String, cx: &mut Context<Self>) {
        self.files.open.insert(thread_id.to_owned(), OpenFile { path: path.to_owned(), opened, body: FileBody::Failed { message }, truncated: false });
        cx.notify();
    }

    fn show_read(&mut self, thread_id: &str, path: &str, opened: u64, read: FileRead, cx: &mut Context<Self>) {
        let viewer = core::viewer_for_path(path);
        let is_picture = viewer == Viewer::Picture && (core::looks_like_picture(path) || image_format(path).is_some());
        let body = if is_picture {
            match image_format(path) {
                Some(format) => {
                    FileBody::Picture { size: picture_size(&read.data), image: Arc::new(Image::from_bytes(format, read.data)) }
                }
                None => FileBody::Opaque { note: "This kind of picture opens in its own app.".into() },
            }
        } else {
            let text_ok = viewer == Viewer::Text || core::looks_like_text(&read.data);
            let opaque = viewer != Viewer::Text && !core::looks_like_text(&read.data);
            if opaque || !text_ok {
                FileBody::Opaque { note: "This file opens in its own app.".into() }
            } else {
                match String::from_utf8(read.data) {
                    Err(_) => FileBody::Opaque { note: "This file isn't text this app can read.".into() },
                    Ok(text) => self.text_body(path, text, cx),
                }
            }
        };
        self.files.open.insert(thread_id.to_owned(), OpenFile { path: path.to_owned(), opened, body, truncated: read.truncated });
        cx.notify();
    }

    /// Markdown drawn, code coloured, text plain, all of it selectable.
    fn text_body(&mut self, path: &str, text: String, cx: &mut Context<Self>) -> FileBody {
        let text: Arc<str> = text.into();
        if core::is_markdown(path) {
            let source = text.to_string();
            let doc = cx.new(|cx| TextViewState::markdown(&source, cx));
            return FileBody::Markdown { doc, text };
        }
        let language = core::code_language(path).filter(|_| text.len() < HIGHLIGHT_LIMIT);
        // The editor is made at the next frame, with a window (render_file).
        FileBody::Text { editor: None, text, language }
    }

    /// A previewed Markdown file, its links going through the open gate:
    /// a file it links to (from the file's own folder) previews here too,
    /// read through the runner, so a repo's page can't hand the system one
    /// of its programs; a web page opens; anything else asks.
    fn preview_text(&self, doc: &Entity<TextViewState>, thread: &ThreadSummary, path: &str, cx: &mut Context<Self>) -> gpui_kit::base::TextView {
        use threadbear_core::open_policy::{self as policy, LinkKind};
        let origin = if thread.is_remote() { crate::open_gate::Origin::OtherMac } else { crate::open_gate::Origin::ThisMac };
        let folder = path.rsplit_once(['/', '\\']).map_or(String::new(), |(dir, _)| dir.to_owned());
        let (side, thread) = (cx.entity().downgrade(), thread.clone());
        let base = folder.clone();
        crate::open_gate::text_view(doc, origin, &folder).on_link_click(move |url: &SharedString, _: &ClickEvent, _: &mut Window, cx: &mut App| match policy::link_kind(url) {
            LinkKind::File(target) => {
                if let Some(target) = policy::absolute(&target, &base, &home_dir()) {
                    let _ = side.update(cx, |side, cx| side.preview_path(&target, None, &thread, cx));
                }
            }
            _ => crate::open_gate::open_link(url, origin, &base, cx),
        })
    }

    /// The bar's buttons over a preview: Reveal, Open (`PreviewButtons`).
    pub(crate) fn preview_buttons(&mut self, preview: &PanePreview, thread: &ThreadSummary, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = h_flex().flex_none().gap(px(0.));
        if let Some(path) = preview.file_path().filter(|_| !thread.is_remote()).map(str::to_owned) {
            let reveal_path = path.clone();
            row = row.child(self.header_button("hl-reveal", "folder", REVEAL_TITLE.to_owned(), p, cx, move |_, cx| reveal(&reveal_path.clone(), crate::open_gate::Origin::ThisMac, cx)));
            let open_path = path.clone();
            row = row.child(self.header_button("hl-open", "arrow.up.right", "Open in its app".to_owned(), p, cx, move |this, cx| this.open_externally(&open_path.clone(), cx)));
        }
        row
    }

    pub(crate) fn render_preview(&mut self, thread: &ThreadSummary, preview: &PanePreview, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        match &preview.target {
            Target::File(path) => self.render_file(thread, path, window, cx),
            Target::Page(url) => pane_note(&format!("{url} opened in your browser."), p).into_any_element(),
            Target::Folder(path) => self.render_folder(thread, path, preview, window, cx),
        }
    }

    fn render_file(&mut self, thread: &ThreadSummary, path: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        // A file's text that's waiting for its editor gets one now.
        if let Some(open) = self.files.open.get_mut(&thread.id).filter(|f| f.path == path) {
            if let FileBody::Text { editor, text, language } = &mut open.body {
                if editor.is_none() {
                    let (value, language) = (text.to_string(), *language);
                    let made = cx.new(|cx| {
                        let state = EditorState::new(window, cx).soft_wrap(true);
                        match language {
                            Some(language) => state.language(language),
                            None => state,
                        }
                    });
                    made.update(cx, |state, cx| state.set_value(value, window, cx));
                    *editor = Some(made);
                }
            }
        }
        let Some(open) = self.files.open.get(&thread.id).filter(|f| f.path == path) else {
            return div().size_full().into_any_element();
        };
        let cut = open.truncated.then(|| div().flex_none().w_full().py(px(6.)).text_center().text_size(px(style::size::SMALL)).text_color(p.secondary).child("Only the start of this file came from the runner."));
        let body: AnyElement = match &open.body {
            // A spinner so a click shows something at once; the header already
            // says which file it's getting.
            FileBody::Loading => div().size_full().flex().items_center().justify_center().child(Spinner::new().with_size(px(14.)).color(p.secondary)).into_any_element(),
            FileBody::Failed { message } => pane_note(&format!("Couldn't read it: {message}"), p).into_any_element(),
            FileBody::Opaque { note } => pane_note(note, p).into_any_element(),
            FileBody::Picture { image, .. } => {
                let source = ImageSource::Image(image.clone());
                let (path2, entity) = (path.to_owned(), cx.entity().downgrade());
                div()
                    .id("hl-picture")
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(0.))
                    .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                        // Copy Image, the picture's own right-click (`FilePreviewView.menu`).
                        let position = event.position;
                        let path3 = path2.clone();
                        entity
                            .update(cx, |this, cx| this.open_picture_menu(path3, position, window, cx))
                            .ok();
                    })
                    .child(img(source).size_full().object_fit(ObjectFit::Contain))
                    .into_any_element()
            }
            FileBody::Markdown { doc, .. } => {
                let dark = cx.theme().is_dark();
                let style = match &self.files.reading_style {
                    Some((was, style)) if *was == dark => style.clone(),
                    _ => {
                        let built = crate::rows::reading_style(cx);
                        self.files.reading_style = Some((dark, built.clone()));
                        built
                    }
                };
                // A page, with margins and lines no longer than the Swift
                // app's pageWidth however wide the panel is.
                div()
                    .id("hl-markdown")
                    .size_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .max_w(px(720.))
                            .mx_auto()
                            .px(px(24.))
                            .py(px(18.))
                            .text_size(px(style::size::BODY))
                            .line_height(px(style::size::BODY_LINE))
                            .child(self.preview_text(doc, thread, path, cx).selectable(true).style(style)),
                    )
                    .into_any_element()
            }
            FileBody::Text { editor: Some(editor), .. } => div()
                .size_full()
                .child(Editor::new(editor).readonly(true).h_full().appearance(false).bordered(false).text_size(px(style::size::DETAIL)).font_family(cx.theme().mono_font_family.clone()))
                .into_any_element(),
            FileBody::Text { .. } => div().size_full().into_any_element(),
        };
        v_flex().size_full().when_some(cut, |s, c| s.child(c)).child(div().flex_1().min_h_0().child(body)).into_any_element()
    }

    /// `Copy Image`: the file's own bytes, not a path, so it pastes as the picture.
    fn open_picture_menu(&mut self, _path: String, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let items = vec![crate::highlights::MenuItem::picture_copy()];
        self.open_menu(items, position, window, cx);
    }

    /// Puts the picture on the clipboard as the file's own bytes (a PNG stays a
    /// PNG), which every app that takes a pasted picture reads (`copyImage`).
    pub(crate) fn copy_open_picture(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let thread = self.thread(cx)?;
        let open = self.files.open.get(&thread.id)?;
        let FileBody::Picture { image, .. } = &open.body else { return None };
        let format = match image.format() {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::Gif => "gif",
            ImageFormat::Webp => "webp",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Tiff => "tiff",
            ImageFormat::Svg => "svg",
            _ => "image",
        };
        cx.write_to_clipboard(ClipboardItem::new_image(image));
        Some(format.to_owned())
    }

    /// `dump`'s preview line (`Panels.preview`).
    #[cfg(feature = "automation")]
    pub(crate) fn preview_line(&self, preview: &PanePreview) -> String {
        match &preview.target {
            Target::File(path) => format!("file {path}"),
            Target::Folder(path) => format!("folder {path}"),
            Target::Page(url) => format!("page {url}"),
        }
    }

    #[cfg(feature = "automation")]
    pub(crate) fn fill_preview_dump(&self, pane: &mut serde_json::Map<String, Value>, preview: &PanePreview, cx: &App) {
        match &preview.target {
            Target::Page(url) => {
                pane.insert("previewKind".into(), json!("page"));
                pane.insert("previewText".into(), json!(url));
            }
            Target::Folder(path) => {
                pane.insert("previewKind".into(), json!("folder"));
                pane.insert("previewText".into(), json!(path));
            }
            Target::File(path) => {
                let Some(thread) = self.thread(cx) else { return };
                let Some(open) = self.files.open.get(&thread.id).filter(|f| &f.path == path) else { return };
                match &open.body {
                    FileBody::Loading => {}
                    FileBody::Failed { message } => {
                        pane.insert("previewKind".into(), json!("note"));
                        pane.insert("previewText".into(), json!(format!("Couldn't read it: {message}")));
                    }
                    FileBody::Opaque { note } => {
                        pane.insert("previewKind".into(), json!("note"));
                        pane.insert("previewText".into(), json!(note));
                    }
                    FileBody::Picture { size, .. } => {
                        pane.insert("previewKind".into(), json!("picture"));
                        pane.insert("previewText".into(), json!(size.map(|(w, h)| format!("{w}x{h}"))));
                    }
                    FileBody::Markdown { text, .. } => {
                        pane.insert("previewKind".into(), json!("markdown"));
                        pane.insert("previewText".into(), json!(text.chars().take(4000).collect::<String>()));
                    }
                    FileBody::Text { text, language, .. } => {
                        pane.insert("previewKind".into(), json!(if language.is_some() { "code" } else { "text" }));
                        pane.insert("previewText".into(), json!(text.chars().take(4000).collect::<String>()));
                    }
                }
            }
        }
    }
}

/// The `preview` automation command (`Automation.swift`).
#[cfg(feature = "automation")]
pub(crate) fn preview_command(side: &Entity<SidePane>, thread: &ThreadSummary, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    if cmd["copyImage"].as_bool() == Some(true) {
        let format = side.update(cx, |pane, cx| pane.copy_open_picture(cx)).ok_or("no picture is showing")?;
        return Ok(json!(format!("ok copied {format}")));
    }
    if cmd["close"].as_bool() == Some(true) {
        side.update(cx, |pane, cx| pane.close_preview(&thread.id, cx));
    } else if cmd["back"].as_bool() == Some(true) {
        side.update(cx, |pane, cx| pane.preview_back(&thread.id, cx));
    } else if let Some(path) = cmd["path"].as_str() {
        side.update(cx, |pane, cx| pane.preview_path(path, None, thread, cx));
    } else if let Some(url) = cmd["url"].as_str() {
        side.update(cx, |pane, cx| {
            let section = pane.section_for(thread);
            pane.show_preview(PanePreview { target: Target::Page(url.to_owned()), title: core::text::address(url), from: section, opened: 0, back: vec![], memory: Default::default() }, &thread.id, cx);
        });
    } else {
        return Err("preview needs path, url or close".into());
    }
    let _ = window;
    let shown = side.read(cx).previews.get(&thread.id).map_or_else(|| "no preview".to_owned(), |p| format!("showing {}", p.title));
    Ok(json!(format!("ok {shown}")))
}

/// The Files panel has no preview stored: its section shows the thread's folder.
pub(crate) fn files_section_preview(thread: &ThreadSummary) -> PanePreview {
    PanePreview { target: Target::Folder(thread.cwd.clone()), title: "Files".into(), from: FILES.to_owned(), opened: 0, back: vec![], memory: Default::default() }
}

