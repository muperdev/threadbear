// A folder in the side pane, a port of the Swift app's FolderBrowser: a list with
// columns and fold-out rows, a path bar to go back up, a right-click menu, and
// double-click to go in. What's in a folder comes from the runner's `files.list`
// (threadbear_core::folders has the sort and the path bar's words), so a
// thread on another Mac browses the same. The icon grid, Quick Look on the space
// bar and drag-out are later (`folder.icons`, `folder.keys`, `drag.fileout`).
use crate::call;
use crate::highlights::{icon, palette, MenuItem, Palette, PanePreview, SidePane, Target, FILES};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
// Shadows gpui's px, so sizes here follow the app's zoom (style.rs).
use crate::style::px;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use threadbear_core::folders::{self as core_folders, crumbs, join, parent, size_words, Crumb, FolderContents, FolderEntry, FolderSort, SortKey};
use threadbear_core::highlights::{is_app_project, runs_code};
use threadbear_core::inbox::ThreadSummary;

const ROW_HEIGHT: f32 = 24.;
const HEADER_HEIGHT: f32 = 24.;
const PATH_BAR_HEIGHT: f32 = 26.;
/// Narrower than this the Date Modified column goes, then Size
/// (FolderPaneView's layout).
const HIDE_DATE_UNDER: f32 = 380.;
const HIDE_SIZE_UNDER: f32 = 260.;

/// One folder browser's state: its listings and what's open in it.
pub(crate) struct FolderView {
    pub(crate) root: String,
    listings: HashMap<String, FolderContents>,
    /// Folders asked for and not heard back about (`loadingFolders`).
    loading: HashSet<String>,
    expanded: HashSet<String>,
    selected: Vec<String>,
    failure: Option<String>,
    sort: FolderSort,
    show_hidden: bool,
    scroll: UniformListScrollHandle,
}

impl FolderView {
    fn new(root: String, sort: FolderSort, show_hidden: bool) -> Self {
        Self { root, listings: HashMap::new(), loading: HashSet::new(), expanded: HashSet::new(), selected: vec![], failure: None, sort, show_hidden, scroll: UniformListScrollHandle::new() }
    }

    fn entries_of(&self, folder: &str) -> Vec<FolderEntry> {
        let Some(listing) = self.listings.get(folder) else { return vec![] };
        let visible: Vec<FolderEntry> = listing.entries.iter().filter(|e| self.show_hidden || !e.hidden).cloned().collect();
        self.sort.apply(&visible)
    }

    /// The rows on screen, top to bottom, indented by depth.
    fn visible_rows(&self) -> Vec<(FolderEntry, usize)> {
        let mut out = Vec::new();
        fn add(view: &FolderView, folder: &str, depth: usize, out: &mut Vec<(FolderEntry, usize)>) {
            for entry in view.entries_of(folder) {
                let open = entry.is_folder && view.expanded.contains(&entry.path);
                out.push((entry.clone(), depth));
                if open {
                    add(view, &entry.path, depth + 1, out);
                }
            }
        }
        add(self, &self.root, 0, &mut out);
        out
    }

    /// The scripted app's and dump's line (`FolderPaneView.summary`).
    fn summary(&self) -> String {
        let rows = self.entries_of(&self.root);
        let names: Vec<String> = rows.iter().take(40).map(|e| e.name.clone()).collect();
        let mut words = format!("(list, {} rows, sort {} {}, rows drawn {}", rows.len(), self.sort.key.name(), if self.sort.ascending { "up" } else { "down" }, self.visible_rows().len());
        if !self.selected.is_empty() {
            let picked: Vec<&str> = self.selected.iter().map(|p| core_folders_name(p)).collect();
            words += &format!(", selected {}", picked.join(" + "));
        }
        if let Some(note) = self.note() {
            words += &format!(", says \"{note}\"");
        }
        words + "): " + &names.join(", ")
    }

    fn note(&self) -> Option<String> {
        if let Some(failure) = &self.failure {
            return Some(failure.clone());
        }
        if self.loading.contains(&self.root) {
            return Some("Loading…".to_owned());
        }
        (self.listings.contains_key(&self.root) && self.visible_rows().is_empty()).then(|| "This folder is empty.".to_owned())
    }
}

fn core_folders_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Every open folder browser's state.
pub(crate) struct FoldersState {
    views: HashMap<String, FolderView>,
    /// Sort and hidden files are the same for every folder, as the Swift app
    /// keeps them in its defaults (`savedSort`, `showHidden`).
    sort: FolderSort,
    show_hidden: bool,
}

impl Default for FoldersState {
    fn default() -> Self {
        Self { views: HashMap::new(), sort: FolderSort::default(), show_hidden: false }
    }
}

fn view_key(thread_id: &str, root: &str) -> String {
    format!("{thread_id}\n{root}")
}

/// `date_words`, the list's Date Modified column (`DateFormatter`, medium date,
/// short time, "Today" and "Yesterday" relative).
fn date_words(ms: i64) -> String {
    use threadbear_core::chrono::{Datelike as _, Local, TimeZone as _};
    let Some(date) = Local.timestamp_millis_opt(ms).single() else { return "--".to_owned() };
    let now = Local::now();
    let time = date.format("%-I:%M %p").to_string();
    let day_words = if date.date_naive() == now.date_naive() {
        "Today".to_owned()
    } else if date.date_naive().succ_opt() == Some(now.date_naive()) {
        "Yesterday".to_owned()
    } else if date.year() == now.year() {
        date.format("%-d %b").to_string()
    } else {
        date.format("%-d %b %Y").to_string()
    };
    format!("{day_words} at {time}")
}

impl SidePane {
    /// The thread's Files panel: its folder, listed again each time it's
    /// shown, so what the agent has made since is there.
    pub(crate) fn load_folder_for_files(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let Some(thread) = self.thread(cx).filter(|t| t.id == thread_id) else { return };
        if let Some(view) = self.folders.views.get_mut(&view_key(thread_id, &thread.cwd)) {
            view.listings.remove(&thread.cwd);
        }
        self.ensure_folder(thread_id, &thread.cwd, &thread.cwd, thread.runner_id.clone(), cx);
    }

    pub(crate) fn load_folder_for_preview(&mut self, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let Some(preview) = self.previews.get(&thread.id).cloned() else { return };
        let Target::Folder(path) = &preview.target else { return };
        self.ensure_folder(&thread.id, path, path, thread.runner_id.clone(), cx);
    }

    /// Makes the folder view and asks for `folder`'s rows if it hasn't them.
    fn ensure_folder(&mut self, thread_id: &str, root: &str, folder: &str, runner: Option<String>, cx: &mut Context<Self>) {
        let key = view_key(thread_id, root);
        let (sort, hidden) = (self.folders.sort, self.folders.show_hidden);
        let view = self.folders.views.entry(key.clone()).or_insert_with(|| FolderView::new(root.to_owned(), sort, hidden));
        if view.listings.contains_key(folder) || !view.loading.insert(folder.to_owned()) {
            return;
        }
        let mut params = json!({ "path": folder });
        if let Some(runner) = runner {
            params["runnerId"] = json!(runner);
        }
        let client = self.client.clone();
        let folder = folder.to_owned();
        let root = root.to_owned();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = call(client, "files.list", params).await;
            this.update(cx, |this, cx| {
                let Some(view) = this.folders.views.get_mut(&view_key_of(&key)) else { return };
                view.loading.remove(&folder);
                match res {
                    Ok(v) => {
                        let mut contents = FolderContents::decode(&v);
                        // By the folder as it was asked for (`FolderListing.read`).
                        contents.path = folder.clone();
                        let listing = FolderContents { entries: contents.entries.iter().map(|e| FolderEntry { path: join(&folder, &e.name), ..e.clone() }).collect(), ..contents };
                        view.listings.insert(folder.clone(), listing);
                    }
                    Err(e) if folder == root => view.failure = Some(e.message),
                    Err(_) => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn render_files_panel(&mut self, thread: &ThreadSummary, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let preview = crate::files::files_section_preview(thread);
        let root = thread.cwd.clone();
        self.render_folder(thread, &root, &preview, window, cx)
    }

    pub(crate) fn render_folder(&mut self, thread: &ThreadSummary, path: &str, preview: &PanePreview, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        // Files panel's first draw, or a folder opened over a section.
        self.ensure_folder(&thread.id, path, path, thread.runner_id.clone(), cx);
        let key = view_key(&thread.id, path);
        let thread2 = thread.clone();
        let preview = preview.clone();
        let (rows, note, sort, selected, scroll) = {
            let Some(view) = self.folders.views.get(&key) else { return div().into_any_element() };
            (view.visible_rows(), view.note(), view.sort, view.selected.clone(), view.scroll.clone())
        };
        let entity = cx.entity().downgrade();
        let crumb_list = crumbs(path, &crate::files::home_dir());
        let path_bar = self.path_bar(&key, &crumb_list, thread, &preview, p, cx).into_any_element();
        let header = self.folder_header(&key, sort, p, cx).into_any_element();
        let count = rows.len();
        let (show_date, show_size) = (self.width >= HIDE_DATE_UNDER, self.width >= HIDE_SIZE_UNDER);
        let rows_arc: std::rc::Rc<Vec<(FolderEntry, usize)>> = std::rc::Rc::new(rows);
        let rows2 = rows_arc.clone();
        let (preview2, key2) = (preview.clone(), key.clone());
        let list = uniform_list("folder-rows", count, move |range, window, cx| {
            let mut out = Vec::with_capacity(range.len());
            for index in range {
                let (entry, depth) = rows2[index].clone();
                let selected = selected.contains(&entry.path);
                let expanded_key = (key2.clone(), entry.clone());
                let (entity2, thread3, preview3, key3) = (entity.clone(), thread2.clone(), preview2.clone(), key2.clone());
                out.push(folder_row(index, entry, depth, selected, expanded_key.0, entity2, thread3, preview3, key3, p, show_date, show_size, window, cx).into_any_element());
            }
            out
        })
        .track_scroll(&scroll)
        .size_full();
        // A click on the blank space below the rows, or a right-click, goes to the empty space's menu.
        let empty_key = key.clone();
        let surface = div()
            .id("folder-surface")
            .flex_1()
            .min_h_0()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    if let Some(thread) = this.thread(cx) {
                        let items = this.folder_menu_items(&empty_key, None, &thread);
                        this.open_menu(items, ev.position, window, cx);
                    }
                }),
            )
            .child(list);
        let mut column = v_flex().size_full().child(header);
        column = if let Some(note) = note { column.child(div().flex_1().min_h_0().child(crate::files::pane_note(&note, p))) } else { column.child(surface) };
        // The path bar under the list, as the Finder's is.
        column = column.child(path_bar);
        let _ = window;
        column.into_any_element()
    }

    fn path_bar(&mut self, key: &str, crumb_list: &[Crumb], thread: &ThreadSummary, preview: &PanePreview, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        // The end of the path: too many folders to fit show their last few
        // after a "…", as the Finder's path bar tucks the rest.
        const SHOWN: usize = 4;
        let tucked = crumb_list.len() > SHOWN;
        let shown = if tucked { &crumb_list[crumb_list.len() - SHOWN..] } else { crumb_list };
        let mut row = h_flex().flex_none().h(px(PATH_BAR_HEIGHT)).px(px(8.)).items_center().justify_end().gap(px(3.)).border_t_1().border_color(p.hairline).text_size(px(crate::style::size::SMALL)).text_color(p.secondary).overflow_x_hidden();
        if tucked {
            row = row.child(div().flex_none().child("…")).child(icon("chevron.right", 9., p.faint));
        }
        for (i, crumb) in shown.iter().enumerate() {
            if i > 0 {
                row = row.child(icon("chevron.right", 9., p.faint));
            }
            let (path, name) = (crumb.path.clone(), crumb.name.clone());
            let (key2, thread2, preview2) = (key.to_owned(), thread.clone(), preview.clone());
            let last = i + 1 == shown.len();
            row = row.child(
                div()
                    .id(SharedString::from(format!("crumb-{i}")))
                    .flex_none()
                    .px(px(3.))
                    .rounded(px(4.))
                    .when(!last, |s| s.cursor_pointer().hover(|h| h.text_color(p.fg)))
                    .when(last, |s| s.text_color(p.fg))
                    .child(name)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.go_to_crumb(&key2.clone(), path.clone(), &thread2, &preview2, cx))),
            );
        }
        row
    }

    fn folder_header(&mut self, key: &str, sort: FolderSort, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let column = |this: &mut Self, sort_key: SortKey, title: &'static str, width: Option<f32>, right: bool, cx: &mut Context<Self>| {
            let active = sort.key == sort_key;
            let chevron = if !active { None } else { Some(if sort.ascending { "chevron.up" } else { "chevron.down" }) };
            let key2 = key.to_owned();
            let _ = &this;
            div()
                .id(SharedString::from(format!("sort-{}", sort_key.name())))
                .when_some(width, |s, w| s.w(px(w)).flex_none())
                .when(width.is_none(), |s| s.flex_1().min_w_0())
                .h_full()
                .flex()
                .items_center()
                .gap(px(3.))
                .when(right, |s| s.justify_end())
                .cursor_pointer()
                .text_color(if active { p.fg } else { p.secondary })
                .text_size(px(crate::style::size::SMALL))
                .child(title)
                .when_some(chevron, |s, c| s.child(icon(c, 9., p.secondary)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    // Same column again turns it round; a new one starts its
                    // useful way: names up, dates and sizes newest and biggest first.
                    let ascending = if sort.key == sort_key { !sort.ascending } else { sort_key == SortKey::Name };
                    this.set_folder_sort(&key2.clone(), FolderSort { key: sort_key, ascending }, cx);
                }))
        };
        let name = column(self, SortKey::Name, "Name", None, false, cx);
        let date = column(self, SortKey::Modified, "Date Modified", Some(150.), false, cx);
        let size = column(self, SortKey::Size, "Size", Some(72.), true, cx);
        let (show_date, show_size) = (self.width >= HIDE_DATE_UNDER, self.width >= HIDE_SIZE_UNDER);
        h_flex()
            .flex_none()
            .h(px(HEADER_HEIGHT))
            .px(px(10.))
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(p.hairline)
            .child(name)
            .when(show_date, |s| s.child(date))
            .when(show_size, |s| s.child(size))
    }

    pub(crate) fn set_folder_sort(&mut self, key: &str, sort: FolderSort, cx: &mut Context<Self>) {
        self.folders.sort = sort;
        for view in self.folders.views.values_mut() {
            view.sort = sort;
        }
        let _ = key;
        cx.notify();
    }

    pub(crate) fn set_folder_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        self.folders.show_hidden = hidden;
        for view in self.folders.views.values_mut() {
            view.show_hidden = hidden;
        }
        cx.notify();
    }

    fn toggle_expanded(&mut self, key: &str, entry: &FolderEntry, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let root = {
            let Some(view) = self.folders.views.get_mut(key) else { return };
            if !view.expanded.remove(&entry.path) {
                view.expanded.insert(entry.path.clone());
            }
            view.root.clone()
        };
        self.ensure_folder(&thread.id, &root, &entry.path, thread.runner_id.clone(), cx);
        cx.notify();
    }

    fn select_row(&mut self, key: &str, entry: &FolderEntry, cx: &mut Context<Self>) {
        if let Some(view) = self.folders.views.get_mut(key) {
            view.selected = vec![entry.path.clone()];
        }
        cx.notify();
    }

    /// A double-click on a row: a folder goes in, a file shows in the pane
    /// with Back to here, a project file opens in its app (`openFromFolder`).
    fn open_row(&mut self, key: &str, entry: &FolderEntry, thread: &ThreadSummary, preview: &PanePreview, cx: &mut Context<Self>) {
        let memory = self.folders.views.get(key).map(|v| crate::highlights::FolderMemory { selected: v.selected.clone(), expanded: v.expanded.clone() }).unwrap_or_default();
        let mut current = preview.clone();
        current.memory = memory;
        let back_stack = {
            let mut back = std::mem::take(&mut current.back);
            back.push(current.clone());
            back
        };
        if entry.is_folder {
            let next = PanePreview { target: Target::Folder(entry.path.clone()), title: entry.name.clone(), from: preview.from.clone(), opened: 0, back: back_stack, memory: Default::default() };
            self.show_preview(next, &thread.id, cx);
            return;
        }
        if !thread.is_remote() && is_app_project(&entry.path) {
            self.open_externally(&entry.path, cx);
            return;
        }
        let next = PanePreview { target: Target::File(entry.path.clone()), title: entry.name.clone(), from: preview.from.clone(), opened: 0, back: back_stack, memory: Default::default() };
        self.show_preview(next, &thread.id, cx);
    }

    /// A folder in the path bar: from a folder browser opened in the pane, a
    /// new preview with Back to here; the Files panel's own bar, too (`goUp`).
    fn go_to_crumb(&mut self, key: &str, path: String, thread: &ThreadSummary, preview: &PanePreview, cx: &mut Context<Self>) {
        let root_is = self.folders.views.get(key).map(|v| v.root.clone());
        if root_is.as_deref() == Some(path.as_str()) {
            return;
        }
        let entry = FolderEntry { name: core_folders_name(&path).to_owned(), path, is_folder: true, ..Default::default() };
        self.open_row(key, &entry, thread, preview, cx);
    }

    /// `⌘↑` and the path bar's parent (`goUp`): the folder above, selecting
    /// the one Mike came up out of, as the Finder does.
    pub(crate) fn folder_up(&mut self, key: &str, thread: &ThreadSummary, preview: &PanePreview, cx: &mut Context<Self>) {
        let Some(root) = self.folders.views.get(key).map(|v| v.root.clone()) else { return };
        let Some(up) = parent(&root) else { return };
        let entry = FolderEntry { name: core_folders_name(&up).to_owned(), path: up, is_folder: true, ..Default::default() };
        self.open_row(key, &entry, thread, preview, cx);
        // The new listing selects the folder Mike came up out of (`selecting`).
        if let Some(view) = self.folders.views.values_mut().find(|v| v.root == entry.path) {
            view.selected = vec![root];
        }
    }

    /// `FolderPaneView.menu`: a row's menu, or the empty space's.
    pub(crate) fn folder_menu_items(&self, key: &str, entry: Option<&FolderEntry>, thread: &ThreadSummary) -> Vec<MenuItem> {
        use crate::highlights::MenuAction as A;
        let mut items = vec![];
        if let Some(entry) = entry {
            items.push(MenuItem::action("Open", if entry.is_folder { "folder" } else { "sidebar.right" }, A::OpenFolderRow { key: key.to_owned(), entry: entry.clone() }));
            if !thread.is_remote() {
                items.push(MenuItem::action(if entry.is_folder { "Open in Finder" } else { "Open in its app" }, "arrow.up.forward.app", A::OpenFile { path: entry.path.clone() }));
                items.push(MenuItem::action(crate::files::REVEAL_TITLE, "folder", A::Reveal { path: entry.path.clone() }));
            }
            items.push(MenuItem::action("Copy path", "doc.on.doc", A::Copy { text: entry.path.clone() }));
            items.push(MenuItem::separator());
        }
        let hidden = self.folders.show_hidden;
        let sort = self.folders.sort;
        let mut sort_children = vec![];
        for key_kind in [SortKey::Name, SortKey::Modified, SortKey::Size] {
            let symbol = match key_kind {
                SortKey::Name => "textformat",
                SortKey::Modified => "calendar",
                SortKey::Size => "scalemass",
            };
            let ascending = if sort.key == key_kind { !sort.ascending } else { key_kind == SortKey::Name };
            let mut item = MenuItem::action(key_kind.title(), symbol, A::SortFolder { key: key.to_owned(), sort: FolderSort { key: key_kind, ascending } });
            item.checked = sort.key == key_kind;
            sort_children.push(item);
        }
        // The icon grid is later work; the item keeps the menu's shape.
        items.push(MenuItem::disabled("Show as icons", "square.grid.2x2"));
        items.push(MenuItem::submenu("Sort by", "arrow.up.arrow.down", sort_children));
        items.push(MenuItem::action(if hidden { "Hide hidden files" } else { "Show hidden files" }, if hidden { "eye.slash" } else { "eye" }, A::FolderHidden { hidden: !hidden }));
        items
    }

    pub(crate) fn run_folder_row_action(&mut self, key: &str, entry: FolderEntry, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let preview = self.previews.get(&thread.id).filter(|p| matches!(p.target, Target::Folder(_))).cloned().unwrap_or_else(|| crate::files::files_section_preview(thread));
        self.open_row(key, &entry, thread, &preview, cx);
    }

    #[cfg(feature = "automation")]
    pub(crate) fn folder_summary(&self, thread: &ThreadSummary, cx: &App) -> Option<String> {
        let _ = cx;
        let key = self.current_folder_key(thread)?;
        self.folders.views.get(&key).map(|v| v.summary())
    }

    /// The folder browser showing, if any: the pane's own or the Files panel's.
    pub(crate) fn current_folder_key(&self, thread: &ThreadSummary) -> Option<String> {
        let section = self.section_for(thread);
        if let Some(preview) = self.previews.get(&thread.id).filter(|p| p.from == section) {
            return match &preview.target {
                Target::Folder(path) => Some(view_key(&thread.id, path)),
                _ => None,
            };
        }
        (section == FILES).then(|| view_key(&thread.id, &thread.cwd))
    }
}

fn view_key_of(key: &str) -> String {
    key.to_owned()
}

#[allow(clippy::too_many_arguments)]
fn folder_row(
    index: usize,
    entry: FolderEntry,
    depth: usize,
    selected: bool,
    key: String,
    entity: WeakEntity<SidePane>,
    thread: ThreadSummary,
    preview: PanePreview,
    key2: String,
    p: Palette,
    show_date: bool,
    show_size: bool,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let _ = window;
    let indent = 10. + depth as f32 * 14.;
    let (entry2, entry3, entity2, entity3, entity4, thread2, thread3, preview2, key3, key4) = (entry.clone(), entry.clone(), entity.clone(), entity.clone(), entity.clone(), thread.clone(), thread.clone(), preview.clone(), key.clone(), key2.clone());
    let select_entry = entry.clone();
    let name_color = if entry.hidden { p.secondary } else { p.fg };
    let expanded = false; // Filled in below from the view's state.
    let _ = expanded;
    let row = div()
        .id(("folder-row", index))
        .w_full()
        .h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(indent))
        .pr(px(10.))
        .cursor_pointer()
        .when(selected, |s| s.bg(p.hover))
        .hover(move |s| s.bg(p.hover))
        .on_click(move |event, _window, cx| {
            let (entity, entry) = (entity2.clone(), select_entry.clone());
            let (thread, preview, key) = (thread2.clone(), preview2.clone(), key3.clone());
            let double = event.click_count() >= 2;
            entity
                .update(cx, |this, cx| {
                    if double {
                        this.open_row(&key, &entry, &thread, &preview, cx);
                    } else {
                        this.select_row(&key, &entry, cx);
                    }
                })
                .ok();
        })
        .on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            let (entity, entry) = (entity3.clone(), entry2.clone());
            let (thread, key) = (thread3.clone(), key4.clone());
            let position = event.position;
            entity
                .update(cx, |this, cx| {
                    this.select_row(&key, &entry, cx);
                    let items = this.folder_menu_items(&key, Some(&entry), &thread);
                    this.open_menu(items, position, window, cx);
                })
                .ok();
        });
    let mut name_row = h_flex().flex_1().min_w_0().items_center().gap(px(5.));
    if entry.is_folder {
        let (entity5, thread4, entry4, key5) = (entity4, thread.clone(), entry3, key2.clone());
        let is_open = entity5.upgrade().and_then(|e| e.read(cx).folders.views.get(&key5).map(|v| v.expanded.contains(&entry4.path))).unwrap_or(false);
        let (entity6, entry5, key6, thread5) = (entity5.clone(), entry4.clone(), key5.clone(), thread4.clone());
        name_row = name_row.child(
            div()
                .id(("folder-tri", index))
                .flex_none()
                .size(px(14.))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(if is_open { "chevron.down" } else { "chevron.right" }, 9., p.faint))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let (entity, entry, key, thread) = (entity6.clone(), entry5.clone(), key6.clone(), thread5.clone());
                    entity
                        .update(cx, |this, cx| this.toggle_expanded(&key, &entry, &thread, cx))
                        .ok();
                }),
        );
    } else {
        name_row = name_row.child(div().flex_none().w(px(14.)));
    }
    let symbol = if entry.is_folder { "folder" } else { threadbear_core::highlights::file_symbol(&entry.name) };
    name_row = name_row.child(icon(symbol, 12., p.icon).flex_none());
    name_row = name_row.child(div().flex_1().min_w_0().truncate().text_size(px(crate::style::size::LABEL)).text_color(name_color).child(entry.name.clone()));
    let date = entry.modified_ms.map(date_words).unwrap_or_else(|| "--".to_owned());
    let size = entry.size.map(size_words).unwrap_or_else(|| "--".to_owned());
    row.child(name_row)
        .when(show_date, |s| s.child(div().w(px(150.)).flex_none().truncate().text_size(px(crate::style::size::DETAIL)).text_color(p.secondary).child(date)))
        .when(show_size, |s| s.child(div().w(px(72.)).flex_none().text_right().truncate().text_size(px(crate::style::size::DETAIL)).text_color(p.secondary).child(size)))
}

/// The `folder` automation command (`Automation.swift`).
#[cfg(feature = "automation")]
pub(crate) fn folder_command(side: &Entity<SidePane>, thread: &ThreadSummary, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    let _ = window;
    let pane = side.read(cx);
    let key = pane.current_folder_key(thread).ok_or("no folder is showing in the pane")?;
    let preview = pane.previews.get(&thread.id).filter(|p| p.from == pane.section_for(thread)).cloned().unwrap_or_else(|| crate::files::files_section_preview(thread));
    let find = |pane: &SidePane, name: &str| pane.folders.views.get(&key).and_then(|v| v.visible_rows().into_iter().map(|(e, _)| e).find(|e| e.name == name));
    if let Some(mode) = cmd["mode"].as_str() {
        if mode != "list" {
            return Err("the icon view isn't drawn in this app yet".into());
        }
    }
    if let Some(sort) = cmd["sort"].as_str() {
        let sort_key = SortKey::from_name(sort).ok_or("sort is name, modified or size")?;
        let ascending = cmd["ascending"].as_bool().unwrap_or(true);
        side.update(cx, |pane, cx| pane.set_folder_sort(&key, FolderSort { key: sort_key, ascending }, cx));
    }
    if let Some(hidden) = cmd["hidden"].as_bool() {
        side.update(cx, |pane, cx| pane.set_folder_hidden(hidden, cx));
    }
    if let Some(name) = cmd["expand"].as_str() {
        let entry = find(side.read(cx), name).filter(|e| e.is_folder).ok_or_else(|| format!("no folder called {name}"))?;
        side.update(cx, |pane, cx| pane.toggle_expanded(&key, &entry, thread, cx));
    }
    if let Some(name) = cmd["select"].as_str() {
        let entry = find(side.read(cx), name).ok_or_else(|| format!("no row called {name}"))?;
        side.update(cx, |pane, cx| pane.select_row(&key, &entry, cx));
    }
    if let Some(name) = cmd["open"].as_str() {
        let entry = find(side.read(cx), name).ok_or_else(|| format!("no row called {name}"))?;
        side.update(cx, |pane, cx| pane.open_row(&key, &entry, thread, &preview, cx));
    }
    if cmd["up"].as_bool() == Some(true) {
        side.update(cx, |pane, cx| pane.folder_up(&key, thread, &preview, cx));
    }
    if let Some(name) = cmd["crumb"].as_str() {
        let (path, root) = {
            let pane = side.read(cx);
            let view = pane.folders.views.get(&key).ok_or("no folder is showing")?;
            let crumb = crumbs(&view.root, &crate::files::home_dir()).into_iter().find(|c| c.name == name).ok_or_else(|| format!("no folder called {name} in the path bar"))?;
            (crumb.path, view.root.clone())
        };
        let _ = root;
        side.update(cx, |pane, cx| pane.go_to_crumb(&key, path, thread, &preview, cx));
    }
    if cmd["pathBar"].as_bool() == Some(true) {
        let pane = side.read(cx);
        let view = pane.folders.views.get(&key).ok_or("no folder is showing")?;
        let words: Vec<String> = crumbs(&view.root, &crate::files::home_dir()).into_iter().map(|c| c.name).collect();
        return Ok(json!(format!("ok {}", words.join(" > "))));
    }
    if let Some(title) = cmd["choose"].as_str() {
        let row = cmd["row"].as_str();
        let (action, ok) = {
            let pane = side.read(cx);
            let entry = match row {
                Some(name) if !name.is_empty() => Some(find(pane, name).ok_or_else(|| format!("no row called {name}"))?),
                _ => None,
            };
            let items = pane.folder_menu_items(&key, entry.as_ref(), thread);
            let all = flatten(items);
            let item = all.into_iter().find(|i| i.title == title).ok_or_else(|| format!("no menu item {title}"))?;
            (item.action, true)
        };
        let _ = ok;
        if let Some(action) = action {
            side.update(cx, |pane, cx| pane.run_menu_action(action, thread, window, cx));
        }
    }
    if let Some(name) = cmd["menu"].as_str() {
        let pane = side.read(cx);
        let entry = if name.is_empty() { None } else { Some(find(pane, name).ok_or_else(|| format!("no row called {name}"))?) };
        let items = pane.folder_menu_items(&key, entry.as_ref(), thread);
        let titles: Vec<String> = items.iter().map(|i| i.title.clone()).collect();
        return Ok(json!(format!("ok {}", titles.join(" | "))));
    }
    let pane = side.read(cx);
    let preview_words = pane.previews.get(&thread.id).map_or_else(|| "no preview".to_owned(), |p| format!("showing {}", p.title));
    let summary = pane.current_folder_key(thread).and_then(|k| pane.folders.views.get(&k)).map_or_else(|| "no folder".to_owned(), |v| v.summary());
    Ok(json!(format!("ok {preview_words}; {summary}")))
}

#[cfg(feature = "automation")]
fn flatten(items: Vec<MenuItem>) -> Vec<MenuItem> {
    let mut out = vec![];
    for item in items {
        let children = item.children.clone();
        out.push(item);
        out.extend(flatten(children));
    }
    out
}

// Used by core_folders imports only under the automation build.
#[allow(dead_code)]
fn _touch() {
    let _ = core_folders::join;
    let _ = runs_code;
}
