// The pane beside the thread: its sections (Highlights, a patch's panels, the
// Patches list and the Files panel) and what's open over them, a port of the
// Swift app's DockPane, HighlightsView, PaneModel and PanelRail's data side. What
// a row says comes from threadbear_core::highlights; the file and folder
// viewers are in files.rs and folders.rs, patches and panels in patches.rs.
//
// One `SidePane` entity for the window, showing the selected thread, so the
// shell can host it as a dock panel. `new` and `render` are all it needs.
use crate::app::Threadbear;
use crate::{call, icons, now_ms, style};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// Shadows gpui's px, so sizes here follow the app's zoom (style.rs).
use crate::style::px;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use threadbear_client::runner::Client;
use threadbear_core::highlights::{self as core, fold, Highlight, HighlightKind, HighlightSections, Panel, ProjectInfo, ProjectRow, Schedule, ThreadHighlights};
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::sidebar as side;

pub(crate) const HIGHLIGHTS: &str = "threadbear.highlights";
pub(crate) const PATCHES: &str = "threadbear.patches";
pub(crate) const FILES: &str = "threadbear.files";

/// The pane's width (WindowDock's default for the right side). The shell agent's
/// dock panel will size it itself.
pub(crate) const PANE_WIDTH: f32 = 300.;
/// `SidebarMetrics`.
pub(crate) const OUTER_INSET: f32 = 8.;
pub(crate) const LEADING: f32 = 18.;
pub(crate) const TRAILING: f32 = 10.;
pub(crate) const CORNER: f32 = 8.;
/// `PaneHeader.height`.
const HEADER_HEIGHT: f32 = 38.;

/// `PanePreview.Target`: a file or page open over the section.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    File(String),
    Folder(String),
    Page(String),
}

/// A folder's selection and open rows, kept so coming Back to it finds it as
/// it was left (`FolderMemory`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FolderMemory {
    pub selected: Vec<String>,
    pub expanded: HashSet<String>,
}

/// A file or page open in the pane in place of its section (`PanePreview`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PanePreview {
    pub target: Target,
    pub title: String,
    /// The section it was opened from, which Back goes to.
    pub from: String,
    /// Bumped each time it's opened, so opening the same file again reads it
    /// again (an agent rewrites its mockup between turns).
    pub opened: u64,
    /// What Back goes through before the section: the folders and files Mike
    /// came through in the browser, last one nearest.
    pub back: Vec<PanePreview>,
    pub memory: FolderMemory,
}

impl PanePreview {
    pub(crate) fn file_path(&self) -> Option<&str> {
        match &self.target {
            Target::File(path) => Some(path),
            _ => None,
        }
    }
}

/// "Show all" on the long groups and the edited files open, for the thread on
/// show (`HighlightsFlags`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RowFlags {
    pub edited_open: bool,
    pub all_links: bool,
    pub all_commits: bool,
    pub all_subagents: bool,
    pub all_started: bool,
}

/// A row's menu, open at a place in the window.
pub(crate) struct OpenMenu {
    pub position: Point<Pixels>,
    pub view: Entity<PopupMenu>,
    _dismissed: Subscription,
}

/// One item of a row's menu.
#[derive(Clone)]
pub(crate) struct MenuItem {
    pub title: String,
    pub symbol: Option<&'static str>,
    pub action: Option<MenuAction>,
    pub checked: bool,
    pub children: Vec<MenuItem>,
}

impl MenuItem {
    pub(crate) fn action(title: impl Into<String>, symbol: &'static str, action: MenuAction) -> Self {
        Self { title: title.into(), symbol: Some(symbol), action: Some(action), checked: false, children: vec![] }
    }
    pub(crate) fn separator() -> Self {
        Self { title: "---".into(), symbol: None, action: None, checked: false, children: vec![] }
    }
    pub(crate) fn submenu(title: impl Into<String>, symbol: &'static str, children: Vec<MenuItem>) -> Self {
        Self { title: title.into(), symbol: Some(symbol), action: None, checked: false, children }
    }
    /// Shown, greyed: a choice that's coming but isn't here yet.
    pub(crate) fn disabled(title: impl Into<String>, symbol: &'static str) -> Self {
        Self { title: title.into(), symbol: Some(symbol), action: None, checked: false, children: vec![] }
    }
    pub(crate) fn picture_copy() -> Self {
        Self::action("Copy Image", "doc.on.doc", MenuAction::CopyPicture)
    }
}

/// Builds the kit's popup menu from `items`, submenus included.
fn fill_menu(mut menu: PopupMenu, items: &[MenuItem], entity: WeakEntity<SidePane>, window: &mut Window, cx: &mut Context<PopupMenu>) -> PopupMenu {
    for item in items {
        if item.title == "---" {
            menu = menu.separator();
            continue;
        }
        let symbol = item.symbol.map(icons::symbol);
        if !item.children.is_empty() {
            let (children, entity2) = (item.children.clone(), entity.clone());
            menu = menu.submenu_with_icon(symbol, item.title.clone(), window, cx, move |sub, window, cx| fill_menu(sub, &children, entity2.clone(), window, cx));
            continue;
        }
        let action = item.action.clone();
        let target = entity.clone();
        let mut row = PopupMenuItem::new(item.title.clone()).checked(item.checked).disabled(action.is_none()).on_click(move |_, window, cx| {
            if let Some(action) = action.clone() {
                target
                    .update(cx, |this, cx| {
                        if let Some(thread) = this.thread(cx) {
                            this.run_menu_action(action, &thread, window, cx);
                        }
                    })
                    .ok();
            }
        });
        if let Some(symbol) = symbol {
            row = row.icon(symbol);
        }
        menu = menu.item(row);
    }
    menu
}

/// What choosing a menu item does.
#[derive(Clone, Debug)]
pub(crate) enum MenuAction {
    PreviewFile { path: String, title: String },
    PreviewPage { url: String },
    OpenFile { path: String },
    OpenUrl { url: String },
    Reveal { path: String },
    Copy { text: String },
    Remove { id: String, title: String },
    /// A picture's right-click, the file's own bytes (`copyImage`).
    CopyPicture,
    // The folder browser's (`FolderPaneView.menu`).
    OpenFolderRow { key: String, entry: threadbear_core::folders::FolderEntry },
    SortFolder { key: String, sort: threadbear_core::folders::FolderSort },
    FolderHidden { hidden: bool },
}

/// The app's window of files read so far and folders looked at lives in
/// files.rs and folders.rs; these are the pane's own.
pub(crate) struct SidePane {
    pub(crate) app: WeakEntity<Threadbear>,
    pub(crate) client: Arc<Client>,
    focus: FocusHandle,
    /// What the fetcher last acted on: the thread, its last seq, its highlights
    /// stamp (`PanelRail`'s `HighlightsKey`).
    seen: Option<(String, u64, Option<String>)>,
    pub(crate) highlights: HashMap<String, ThreadHighlights>,
    pub(crate) highlights_unsupported: bool,
    wanted: HashMap<String, u64>,
    fetching: HashSet<String>,
    /// Folded headings, for every thread (`collapsedHighlights`).
    pub(crate) collapsed: HashSet<String>,
    /// The chosen section per thread (`panelChoice`).
    pub(crate) chosen: HashMap<String, String>,
    pub(crate) previews: HashMap<String, PanePreview>,
    pub(crate) flags: HashMap<String, RowFlags>,
    /// Asks being ticked, by "<thread>/<seq>/<index>".
    ticking: HashSet<String>,
    pub(crate) projects: HashMap<String, ProjectInfo>,
    pub(crate) schedules: Vec<Schedule>,
    /// The project whose ⓘ popover is open, and where (`projectWhyShown`).
    pub(crate) project_why: Option<(String, Point<Pixels>)>,
    pub(crate) menu: Option<OpenMenu>,
    pub(crate) scroll: ScrollHandle,
    /// What's being fetched or was fetched for the pane: files.rs, folders.rs, patches.rs.
    pub(crate) files: crate::files::FilesState,
    pub(crate) folders: crate::folders::FoldersState,
    pub(crate) panels: crate::patches::PanelsState,
    pub(crate) viewer: crate::viewer::ViewerState,
    /// How wide the pane was last drawn, for the folder list's columns.
    pub(crate) width: f32,
    _subs: Vec<Subscription>,
}

impl SidePane {
    /// `app` is the window's model, `client` its runner connection.
    pub(crate) fn new(app: WeakEntity<Threadbear>, client: Arc<Client>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut pane = SidePane {
            app,
            client,
            focus: cx.focus_handle(),
            seen: None,
            highlights: HashMap::new(),
            highlights_unsupported: false,
            wanted: HashMap::new(),
            fetching: HashSet::new(),
            collapsed: crate::highlights_store::load_collapsed(),
            chosen: HashMap::new(),
            previews: HashMap::new(),
            flags: HashMap::new(),
            ticking: HashSet::new(),
            projects: HashMap::new(),
            schedules: Vec::new(),
            project_why: None,
            menu: None,
            scroll: ScrollHandle::new(),
            files: Default::default(),
            folders: Default::default(),
            panels: Default::default(),
            viewer: crate::viewer::ViewerState::new(cx),
            width: PANE_WIDTH,
            _subs: vec![],
        };
        // The pane follows the window's model: what's selected, and each
        // summary's seq and highlight stamp for "fetch again".
        if let Some(app) = pane.app.upgrade() {
            pane._subs.push(cx.observe_in(&app, window, |this, app, _window, cx| this.app_changed(&app, cx)));
        }
        pane.load_projects(cx);
        pane.load_schedules(cx);
        pane
    }

    /// The thread on show: the window's selected one (`app.paneThread`).
    pub(crate) fn thread(&self, cx: &App) -> Option<ThreadSummary> {
        let app = self.app.upgrade()?;
        let t = app.read(cx);
        t.selected.as_ref().and_then(|id| t.threads.get(id)).cloned()
    }

    /// The runner's list came (or came back): projects and schedules read again.
    pub(crate) fn connected(&mut self, cx: &mut Context<Self>) {
        self.load_projects(cx);
        self.load_schedules(cx);
        // Later: this runs inside the app's update, and reading the selection
        // from it then would be a double borrow.
        cx.spawn(async move |this, cx| {
            this.update(cx, |this, cx| {
                if let Some(thread) = this.thread(cx) {
                    this.refresh_highlights(thread.id, Duration::ZERO, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn projects_updated(&mut self, projects: &Value, cx: &mut Context<Self>) {
        self.projects = ProjectInfo::list(projects).into_iter().map(|p| (p.id.clone(), p)).collect();
        cx.notify();
    }

    pub(crate) fn schedules_updated(&mut self, schedules: &Value, cx: &mut Context<Self>) {
        self.schedules = Schedule::list(schedules);
        cx.notify();
    }

    /// What it's about to show changed: a new selection fetches straight
    /// away, a new event or pin about a second later, as PanelRail does.
    fn app_changed(&mut self, app: &Entity<Threadbear>, cx: &mut Context<Self>) {
        // It draws from the app's state (the open thread, its title, its
        // panels) and the dock reuses its last frame until it's told
        // otherwise (shell.rs, `render_side_section`), so any change to the
        // app draws it again. Fetching again is only for a new thread or new
        // events, below.
        cx.notify();
        let t = app.read(cx);
        let Some(thread) = t.selected.as_ref().and_then(|id| t.threads.get(id)) else { return };
        let key = (thread.id.clone(), thread.last_seq, thread.highlights_at.clone());
        if self.seen.as_ref() == Some(&key) {
            return;
        }
        let new_thread = self.seen.as_ref().map(|k| &k.0) != Some(&key.0);
        self.seen = Some(key);
        let id = thread.id.clone();
        if new_thread {
            let fetched = self.highlights.contains_key(&id);
            // Show-all and the edited files don't stick to a thread we leave (HighlightsView's @State per thread).
            self.flags.clear();
            self.project_why = None;
            self.refresh_highlights(id.clone(), if fetched { Duration::from_secs(1) } else { Duration::ZERO }, cx);
            self.file_shown(&id, cx);
        } else {
            self.refresh_highlights(id, Duration::from_secs(1), cx);
        }
        cx.notify();
    }

    /// The projects' links and order for its heading block.
    fn load_projects(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(v) = call(client, "projects.list", json!({ "archived": true })).await {
                let list = ProjectInfo::list(&v["projects"]);
                this.update(cx, |this, cx| {
                    this.projects = list.into_iter().map(|p| (p.id.clone(), p)).collect();
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn load_schedules(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(v) = call(client, "schedules.list", json!({})).await {
                let list = Schedule::list(&v["schedules"]);
                this.update(cx, |this, cx| {
                    this.schedules = list;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Asks for a thread's highlights after `delay`, once however many times
    /// it's asked meanwhile (`refreshHighlights`): a busy turn moves `lastSeq`
    /// many times a second, and this keeps it to about one fetch a second.
    pub(crate) fn refresh_highlights(&mut self, id: String, delay: Duration, cx: &mut Context<Self>) {
        if self.highlights_unsupported {
            return;
        }
        *self.wanted.entry(id.clone()).or_default() += 1;
        if !self.fetching.insert(id.clone()) {
            return;
        }
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            if delay > Duration::ZERO {
                cx.background_executor().timer(delay).await;
            }
            let mut fetched = 0;
            loop {
                let Ok(Some(wanted)) = this.update(cx, |t, _| (!t.highlights_unsupported).then(|| t.wanted.get(&id).copied().unwrap_or(0))) else { break };
                if wanted == fetched {
                    break;
                }
                fetched = wanted;
                let res = call(client.clone(), "threads.highlights", json!({ "threadId": id })).await;
                let id2 = id.clone();
                let more = this.update(cx, |t, cx| {
                    match res {
                        Ok(v) => {
                            let fresh = ThreadHighlights::decode(&v);
                            if t.highlights.get(&id2) != Some(&fresh) {
                                t.highlights.insert(id2, fresh);
                                cx.notify();
                            }
                        }
                        // An older runner: says so in the pane rather than asking again.
                        Err(e) if e.code == -32601 => {
                            t.highlights_unsupported = true;
                            cx.notify();
                        }
                        Err(_) => {}
                    }
                });
                if more.is_err() {
                    break;
                }
            }
            this.update(cx, |t, _| {
                t.fetching.remove(&id);
            })
            .ok();
        })
        .detach();
    }

    // MARK: sections

    /// The thread's section showing: the one last chosen while it's still
    /// there, else Highlights (`paneSection`). Previews open over it.
    pub(crate) fn section_for(&self, thread: &ThreadSummary) -> String {
        let remembered = self.chosen.get(&thread.id).cloned().unwrap_or_else(|| HIGHLIGHTS.to_owned());
        let exists = remembered == HIGHLIGHTS || remembered == FILES || (remembered == PATCHES && thread.patches.as_ref().is_some_and(|p| !p.is_empty())) || thread.panels().iter().any(|p| p.id == remembered);
        if exists {
            remembered
        } else {
            HIGHLIGHTS.to_owned()
        }
    }

    /// The buttons of the pane: Highlights, each panel, Files, Patches, what's
    /// in the right-hand rail in the Swift app.
    pub(crate) fn section_ids(thread: &ThreadSummary) -> Vec<String> {
        let mut ids = vec![HIGHLIGHTS.to_owned()];
        ids.extend(thread.panels().into_iter().map(|p| p.id));
        ids.push(FILES.to_owned());
        if thread.patches.as_ref().is_some_and(|p| !p.is_empty()) {
            ids.push(PATCHES.to_owned());
        }
        ids
    }

    /// Shows a section (`choosePanel`), closing a preview over another one.
    pub(crate) fn choose_section(&mut self, thread_id: &str, id: &str, cx: &mut Context<Self>) {
        if self.previews.get(thread_id).is_some_and(|p| p.from != id) {
            self.previews.remove(thread_id);
        }
        self.chosen.insert(thread_id.to_owned(), id.to_owned());
        self.menu = None;
        self.section_shown(id, thread_id, cx);
        cx.notify();
    }

    fn section_shown(&mut self, id: &str, thread_id: &str, cx: &mut Context<Self>) {
        if id == FILES {
            self.load_folder_for_files(thread_id, cx);
        } else if id != HIGHLIGHTS && id != PATCHES {
            self.refresh_panel(thread_id, id, cx);
        }
    }

    fn file_shown(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        // The selected thread's current section may want fresh data.
        if let Some(section) = self.chosen.get(thread_id).cloned() {
            self.section_shown(&section, thread_id, cx);
        }
    }

    // MARK: opening things

    /// A click on a highlight: a file previews in the pane (a project file opens
    /// in its app), a page goes to the browser for now (`AppModel.open`).
    pub(crate) fn open_highlight(&mut self, item: &Highlight, thread: &ThreadSummary, cx: &mut Context<Self>) {
        if let Some(path) = &item.path {
            if core::is_app_project(path) && !thread.is_remote() {
                self.open_externally(path, cx);
            } else {
                self.preview_path(path, Some(item.title.clone()), thread, cx);
            }
        } else if let Some(url) = &item.url {
            self.follow_link(url, cx);
        }
    }

    /// A link from a row: a file in the pane, a page in the browser (for
    /// now). Anything else (a `slack:` link and the like) isn't opened from
    /// here: an agent's links only start other apps once Mike has been
    /// asked, which the app can't do yet (`LinkPolicy.ask`).
    pub(crate) fn follow_link(&mut self, url: &str, cx: &mut Context<Self>) {
        if let Some(path) = url.strip_prefix("file://") {
            if let Some(thread) = self.thread(cx) {
                let decoded = crate::files::percent_decode(path);
                self.preview_path(&decoded, None, &thread, cx);
            }
            return;
        }
        if core::is_web_page(url) {
            crate::files::open_web(url, cx);
        }
    }

    /// Opens a file on this runner in its own app (OpenGate's rule: a
    /// program never opens from a click here). A thread on another Mac has
    /// no copy here, so its rows don't offer this.
    pub(crate) fn open_externally(&mut self, path: &str, cx: &mut Context<Self>) {
        crate::files::open_file_in_app(path, self.origin(cx), cx);
    }

    /// Whose files the pane's thread names: with no thread, nobody's, so
    /// none opens.
    pub(crate) fn origin(&self, cx: &App) -> crate::open_gate::Origin {
        match self.thread(cx) {
            Some(t) if !t.is_remote() => crate::open_gate::Origin::ThisMac,
            _ => crate::open_gate::Origin::OtherMac,
        }
    }

    pub(crate) fn preview_path(&mut self, path: &str, title: Option<String>, thread: &ThreadSummary, cx: &mut Context<Self>) {
        let section = self.section_for(thread);
        // A folder browsed in place of its picture; the runner says which it is.
        let target = if self.files.is_known_folder(path) { Target::Folder(path.to_owned()) } else { Target::File(path.to_owned()) };
        let title = title.unwrap_or_else(|| core::last_component(path).to_owned());
        let preview = PanePreview { target, title, from: section, opened: 0, back: vec![], memory: Default::default() };
        self.show_preview(preview, &thread.id.clone(), cx);
    }

    pub(crate) fn show_preview(&mut self, mut preview: PanePreview, thread_id: &str, cx: &mut Context<Self>) {
        preview.opened = self.previews.get(thread_id).map_or(0, |p| p.opened) + 1;
        self.chosen.entry(thread_id.to_owned()).or_insert_with(|| preview.from.clone());
        self.previews.insert(thread_id.to_owned(), preview);
        self.menu = None;
        if let Some(thread) = self.thread(cx) {
            self.begin_preview(&thread, cx);
        }
        cx.notify();
    }

    /// Back: through the folders and files Mike came through in the browser,
    /// then out to the section (`previewBack`).
    pub(crate) fn preview_back(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let Some(current) = self.previews.get(thread_id).cloned() else { return };
        match current.back.split_last() {
            None => {
                self.previews.remove(thread_id);
            }
            Some((last, rest)) => {
                let mut previous = last.clone();
                previous.back = rest.to_vec();
                previous.opened = current.opened + 1;
                self.previews.insert(thread_id.to_owned(), previous);
                if let Some(thread) = self.thread(cx) {
                    self.begin_preview(&thread, cx);
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn close_preview(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        self.previews.remove(thread_id);
        cx.notify();
    }

    pub(crate) fn remove_highlight(&mut self, id: String, title: String, thread_id: String, cx: &mut Context<Self>) {
        if let Some(list) = self.highlights.get_mut(&thread_id) {
            list.highlights.retain(|h| h.id != id);
        }
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.removeHighlight", json!({ "threadId": thread_id, "highlightId": id })).await;
            this.update(cx, |t, cx| {
                if let Err(e) = res {
                    t.note_error(format!("Couldn't remove {title}: {}", e.message), cx);
                }
                t.refresh_highlights(thread_id, Duration::ZERO, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Shows a line in the window's error bar.
    pub(crate) fn note_error(&mut self, message: String, cx: &mut Context<Self>) {
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |t, cx| {
                t.list_error = Some(message);
                cx.notify();
            });
        }
    }

    /// The same `threads.tickAsk` the transcript's card uses: the tick shows at
    /// once, and the runner's answer (or its `thread.updated`) puts it right.
    pub(crate) fn tick_ask(&mut self, thread_id: String, seq: u64, index: usize, ticked: bool, cx: &mut Context<Self>) {
        let key = format!("{thread_id}/{seq}/{index}");
        if !self.ticking.insert(key.clone()) {
            return;
        }
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |t, cx| {
                if let Some(ask) = t.threads.get_mut(&thread_id).and_then(|t| t.asks.as_mut()).and_then(|a| a.get_mut(index)) {
                    ask.ticked = Some(ticked);
                }
                cx.notify();
            });
        }
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.tickAsk", json!({ "threadId": thread_id, "asksSeq": seq, "index": index, "ticked": ticked })).await;
            this.update(cx, |t, cx| {
                t.ticking.remove(&key);
                if let Err(e) = res {
                    if let Some(app) = t.app.upgrade() {
                        app.update(cx, |app, cx| {
                            if let Some(ask) = app.threads.get_mut(&thread_id).and_then(|t| t.asks.as_mut()).and_then(|a| a.get_mut(index)) {
                                ask.ticked = Some(!ticked);
                            }
                            cx.notify();
                        });
                    }
                    t.note_error(e.message, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A menu item chosen.
    pub(crate) fn run_menu_action(&mut self, action: MenuAction, thread: &ThreadSummary, _window: &mut Window, cx: &mut Context<Self>) {
        match action {
            MenuAction::PreviewFile { path, title } => self.preview_path(&path, Some(title), thread, cx),
            MenuAction::PreviewPage { url } => crate::files::open_web(&url, cx),
            MenuAction::OpenFile { path } => self.open_externally(&path, cx),
            MenuAction::OpenUrl { url } => {
                if core::is_web_page(&url) {
                    crate::files::open_web(&url, cx);
                }
            }
            MenuAction::Reveal { path } => crate::files::reveal(&path, if thread.is_remote() { crate::open_gate::Origin::OtherMac } else { crate::open_gate::Origin::ThisMac }, cx),
            MenuAction::Copy { text } => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            MenuAction::Remove { id, title } => self.remove_highlight(id, title, thread.id.clone(), cx),
            MenuAction::CopyPicture => {
                self.copy_open_picture(cx);
            }
            MenuAction::OpenFolderRow { key, entry } => self.run_folder_row_action(&key, entry, thread, cx),
            MenuAction::SortFolder { key, sort } => self.set_folder_sort(&key, sort, cx),
            MenuAction::FolderHidden { hidden } => self.set_folder_hidden(hidden, cx),
        }
        self.menu = None;
        cx.notify();
    }

    pub(crate) fn open_menu(&mut self, items: Vec<MenuItem>, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity().downgrade();
        let build = items.clone();
        let view = PopupMenu::build(window, cx, move |menu, window, cx| fill_menu(menu, &build, entity.clone(), window, cx));
        // The menu closes itself on Escape, a click outside or a choice.
        let dismissed = cx.subscribe(&view, |this, _, _: &DismissEvent, cx| {
            this.menu = None;
            cx.notify();
        });
        self.menu = Some(OpenMenu { position, view, _dismissed: dismissed });
        cx.notify();
    }

    // MARK: row menus (`FileMenu`, `LinkMenu`)

    /// A file row's menu (`FileMenu`).
    pub(crate) fn file_menu(path: &str, title: &str, thread: &ThreadSummary) -> Vec<MenuItem> {
        let mut items = vec![MenuItem::action("Preview here", "sidebar.right", MenuAction::PreviewFile { path: path.to_owned(), title: title.to_owned() })];
        // Another Mac's file has no copy here to open or reveal (yet).
        if !thread.is_remote() {
            items.push(MenuItem::action("Open", "arrow.up.forward.app", MenuAction::OpenFile { path: path.to_owned() }));
            items.push(MenuItem::action(crate::files::REVEAL_TITLE, "folder", MenuAction::Reveal { path: path.to_owned() }));
        }
        items.push(MenuItem::action("Copy path", "doc.on.doc", MenuAction::Copy { text: path.to_owned() }));
        items
    }

    /// A link row's menu (`LinkMenu`). The browser's own item for now: the
    /// Chrome profile list and the panel page are later work.
    pub(crate) fn link_menu(url: &str) -> Vec<MenuItem> {
        let mut items = vec![MenuItem::action(crate::files::OPEN_IN_BROWSER, "safari", MenuAction::OpenUrl { url: url.to_owned() })];
        if core::is_web_page(url) {
            items.push(MenuItem::action("Open in Panel", "sidebar.right", MenuAction::PreviewPage { url: url.to_owned() }));
        }
        items.push(MenuItem::action("Copy link", "link", MenuAction::Copy { text: url.to_owned() }));
        items
    }

    /// `row`'s right-click menu, as HighlightsView builds it.
    pub(crate) fn highlight_menu(item: &Highlight, thread: &ThreadSummary) -> Vec<MenuItem> {
        let mut items = if let Some(path) = &item.path {
            Self::file_menu(path, &item.title, thread)
        } else if let Some(url) = &item.url {
            Self::link_menu(url)
        } else if let Some(sha) = &item.sha {
            vec![MenuItem::action("Copy sha", "number", MenuAction::Copy { text: sha.clone() })]
        } else {
            vec![]
        };
        items.push(MenuItem::separator());
        items.push(MenuItem::action("Remove", "pin.slash", MenuAction::Remove { id: item.id.clone(), title: item.title.clone() }));
        items
    }

    fn open_highlight_menu(&mut self, item: &Highlight, thread: &ThreadSummary, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let items = Self::highlight_menu(item, thread);
        self.open_menu(items, position, window, cx);
    }

    fn open_file_row_menu(&mut self, path: &str, title: &str, thread: &ThreadSummary, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let items = Self::file_menu(path, title, thread);
        self.open_menu(items, position, window, cx);
    }

    fn open_link_row_menu(&mut self, url: &str, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let items = Self::link_menu(url);
        self.open_menu(items, position, window, cx);
    }
}

// ---------------------------------------------------------------- rendering

/// The pane's colours, picked once a frame (`cx.theme()` borrows the app).
#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub fg: Hsla,
    pub secondary: Hsla,
    pub faint: Hsla,
    pub icon: Hsla,
    pub hover: Hsla,
    pub hairline: Hsla,
    pub accent: Hsla,
    pub failed: Hsla,
    pub needs_you: Hsla,
    pub card: Hsla,
    pub card_border: Hsla,
}

pub(crate) fn palette(cx: &App) -> Palette {
    Palette {
        fg: style::label_text(cx),
        secondary: style::secondary_text(cx),
        faint: style::faint_text(cx),
        // A notch brighter than Swift's `faintIcon`: Lucide's thin strokes
        // read fainter than SF Symbols at the same size.
        icon: style::pane_icon(cx),
        hover: style::sidebar_hover(cx),
        hairline: style::hairline(cx),
        accent: style::accent(cx),
        failed: style::failed(cx),
        needs_you: style::needs_you_text(cx),
        card: style::primary_opacity(cx, 0.05),
        card_border: style::card_border(cx),
    }
}

/// An SF Symbol's icon, at a point size and colour.
pub(crate) fn icon(name: &str, size: f32, color: Hsla) -> Icon {
    // A patch can name any SF Symbol; one with no bundled twin gets a plain
    // square rather than `icons::symbol`'s question mark.
    icons::try_symbol(name).unwrap_or_else(|| Icon::new(gpui_kit::assets::IconName::SquareDashed)).with_size(px(size)).text_color(color)
}

/// A control's tooltip and the label scripts find it by, as Swift's `.tip`.
pub(crate) fn tipped(el: Stateful<Div>, tip: impl Into<SharedString>) -> gpui_kit::base::ObservedElement<Stateful<Div>> {
    let text: SharedString = tip.into();
    let shown = text.clone();
    el.test_support().aria_label(text).tooltip(move |window, cx| Tooltip::new(shown.clone()).build(window, cx))
}

/// One row in the pane's lists (`PaneRow`): a grey icon, the title with a word
/// at its end, then a note and a grey line under it.
pub(crate) struct PaneRow {
    pub id: ElementId,
    pub icon: Option<String>,
    pub icon_color: Option<Hsla>,
    pub title: String,
    pub lead: Option<String>,
    pub trailing: Option<String>,
    pub trailing_symbol: Option<String>,
    pub note: Option<String>,
    pub detail: Option<String>,
    pub quiet: bool,
    pub indent: bool,
    pub reserves_icon: bool,
    pub detail_lines: usize,
    pub tip: Option<String>,
    pub on_click: Option<Rc<dyn Fn(&mut SidePane, &mut Window, &mut Context<SidePane>)>>,
    pub on_menu: Option<Rc<dyn Fn(&mut SidePane, Point<Pixels>, &mut Window, &mut Context<SidePane>)>>,
}

impl PaneRow {
    pub(crate) fn new(id: impl Into<ElementId>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            icon: None,
            icon_color: None,
            title: title.into(),
            lead: None,
            trailing: None,
            trailing_symbol: None,
            note: None,
            detail: None,
            quiet: false,
            indent: false,
            reserves_icon: false,
            detail_lines: 1,
            tip: None,
            on_click: None,
            on_menu: None,
        }
    }

    pub(crate) fn icon(mut self, symbol: impl Into<String>) -> Self {
        self.icon = Some(symbol.into());
        self
    }

    pub(crate) fn icon_opt(mut self, symbol: Option<impl Into<String>>) -> Self {
        self.icon = symbol.map(Into::into);
        self
    }

    pub(crate) fn tinted(mut self, color: Option<Hsla>) -> Self {
        self.icon_color = color;
        self
    }

    pub(crate) fn lead(mut self, lead: Option<String>) -> Self {
        self.lead = lead;
        self
    }

    pub(crate) fn trailing(mut self, trailing: Option<String>) -> Self {
        self.trailing = trailing;
        self
    }

    pub(crate) fn trailing_symbol(mut self, symbol: Option<impl Into<String>>) -> Self {
        self.trailing_symbol = symbol.map(Into::into);
        self
    }

    pub(crate) fn note(mut self, note: Option<String>) -> Self {
        self.note = note;
        self
    }

    pub(crate) fn detail(mut self, detail: Option<String>) -> Self {
        self.detail = detail;
        self
    }

    pub(crate) fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }

    pub(crate) fn indent(mut self) -> Self {
        self.indent = true;
        self
    }

    pub(crate) fn reserves_icon(mut self, reserves: bool) -> Self {
        self.reserves_icon = reserves;
        self
    }

    pub(crate) fn detail_lines(mut self, lines: usize) -> Self {
        self.detail_lines = lines;
        self
    }

    pub(crate) fn tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    pub(crate) fn on_click(mut self, f: impl Fn(&mut SidePane, &mut Window, &mut Context<SidePane>) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }

    pub(crate) fn on_menu(mut self, f: impl Fn(&mut SidePane, Point<Pixels>, &mut Window, &mut Context<SidePane>) + 'static) -> Self {
        self.on_menu = Some(Rc::new(f));
        self
    }

    pub(crate) fn render(self, p: Palette, cx: &mut Context<SidePane>) -> impl IntoElement + use<> {
        let hover = p.hover;
        let label_color = if self.quiet { p.secondary } else { p.fg };
        let title_row = h_flex()
            .items_baseline()
            .gap(px(6.))
            .min_w_0()
            .when_some(self.lead, |s, lead| s.child(div().flex_none().font_family(cx.theme().mono_font_family.clone()).text_size(px(style::size::DETAIL)).text_color(p.secondary).child(lead)))
            .child(div().min_w_0().truncate().text_size(px(style::size::LABEL)).line_height(px(17.)).text_color(label_color).child(self.title.clone()))
            .child(div().flex_1().min_w(px(4.)))
            .when_some(self.trailing, |s, t| s.child(div().flex_none().text_size(px(style::size::SMALL)).text_color(p.faint).child(t)))
            .when_some(self.trailing_symbol, |s, symbol| s.child(icon(&symbol, 11., p.secondary)));
        let body = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(2.))
            .child(title_row)
            .when_some(self.note.filter(|n| !n.is_empty()), |s, note| s.child(div().text_size(px(style::size::DETAIL)).line_height(px(16.)).text_color(p.secondary).line_clamp(4).overflow_hidden().text_ellipsis().child(note)))
            .when_some(self.detail.filter(|d| !d.is_empty()), |s, detail| {
                let mut d = div().text_size(px(style::size::SMALL)).line_height(px(14.)).text_color(p.faint);
                d = if self.detail_lines > 1 { d.line_clamp(self.detail_lines).overflow_hidden().text_ellipsis() } else { d.truncate() };
                s.child(d.child(detail))
            });
        let needs_blank_icon = self.reserves_icon && self.icon.is_none();
        let row = h_flex()
            .items_start()
            .gap(px(8.))
            .pl(px(LEADING - OUTER_INSET + if self.indent { 24. } else { 0. }))
            .pr(px(TRAILING))
            .py(px(6.))
            .rounded(px(CORNER))
            .when_some(self.icon, |s, symbol| s.child(div().w(px(16.)).flex_none().flex().justify_center().pt(px(1.)).child(icon(&symbol, 12., self.icon_color.unwrap_or(p.icon)))))
            .when(needs_blank_icon, |s| s.child(div().w(px(16.)).flex_none()))
            .child(body);
        let row = row.when(self.on_click.is_some(), |s| s.hover(move |st| st.bg(hover)).cursor_pointer());
        let mut wrapper = div().id(self.id).px(px(OUTER_INSET)).child(row);
        if let Some(tip) = self.tip {
            wrapper = wrapper.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
        }
        let mut wrapper = wrapper.test_support().aria_label(SharedString::from(self.title));
        if let Some(click) = self.on_click {
            wrapper = wrapper.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| (click)(this, window, cx)));
        }
        if let Some(menu) = self.on_menu {
            wrapper = wrapper.on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| (menu)(this, ev.position, window, cx)));
        }
        wrapper
    }

}

/// `SectionHeading`: a quiet heading, a hairline to the edge, the count when
/// it's folded and a chevron on hover.
pub(crate) struct Heading {
    pub key: String,
    pub title: String,
    pub icon: Option<AnyElement>,
    pub count: Option<usize>,
    pub folded: bool,
    pub foldable: bool,
    pub accessory: Option<AnyElement>,
}

impl SidePane {
    pub(crate) fn heading(&self, h: Heading, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let group = SharedString::from(format!("hl-heading-{}", h.key));
        let key = h.key.clone();
        let (fg, secondary, faint, hairline) = (p.fg, p.secondary, p.faint, p.hairline);
        let hover_group = group.clone();
        let chevron_group = group.clone();
        let title_el = div().flex_none().text_size(px(style::size::LABEL)).line_height(px(16.)).font_semibold().text_color(secondary).group_hover(hover_group, move |s| s.text_color(fg)).child(h.title.clone());
        let mut label = h_flex().group(group.clone()).flex_1().min_w_0().gap(px(6.)).pl(px(LEADING)).pt(px(18.)).pb(px(6.)).items_center();
        if let Some(leading) = h.icon {
            label = label.child(leading);
        }
        let label = label
            .child(title_el)
            .child(div().flex_1().min_w_0().h(px(1.)).mx(px(4.)).bg(hairline))
            .when(h.folded && h.count.unwrap_or(0) > 0, |s| s.child(div().flex_none().text_size(px(style::size::SMALL)).text_color(faint).child(h.count.unwrap_or(0).to_string())))
            .when(h.foldable, |s| s.child(div().flex_none().when(!h.folded, |d| d.invisible()).group_hover(chevron_group, |d| d.visible()).child(icon(if h.folded { "chevron.right" } else { "chevron.down" }, 9., faint))));
        let mut row = h_flex().pr(px(TRAILING)).items_center();
        if h.foldable {
            let tip = format!("{} {}", if h.folded { "Show" } else { "Hide" }, h.title);
            row = row.child(tipped(div().id(SharedString::from(format!("hl-fold-{key}"))).flex_1().min_w_0().child(label), tip).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_fold(&key, cx))));
        } else {
            row = row.child(label);
        }
        row.when_some(h.accessory, |s, a| s.child(div().pt(px(18.)).pb(px(6.)).child(a)))
    }

    pub(crate) fn toggle_fold(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
        }
        crate::highlights_store::save_collapsed(&self.collapsed);
        cx.notify();
    }

    pub(crate) fn is_folded(&self, key: &str) -> bool {
        self.collapsed.contains(key)
    }

    fn project_block(&mut self, row: &ProjectRow, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let key = fold::project(&row.project.id);
        let folded = self.is_folded(&key);
        let count = row.project.links.len() + row.project.open_task_count();
        let project_id = row.project.id.clone();
        let accessory = crate::highlights_project::why_button(row, p, cx).into_any_element();
        let mut list = v_flex();
        list = list.child(self.heading(Heading { key: key.clone(), title: row.project.title.clone(), icon: Some(crate::highlights_project::project_icon(&row.project, cx).into_any_element()), count: Some(count), folded: folded && count > 0, foldable: count > 0, accessory: Some(accessory) }, p, cx));
        if !(folded && count > 0) {
            for (i, link) in row.project.links.iter().enumerate() {
                let address = core::text::address(&link.url);
                let url = link.url.clone();
                let menu_url = link.url.clone();
                let mut pr = PaneRow::new(SharedString::from(format!("hl-plink-{project_id}-{i}")), link.title.clone())
                    .icon("globe")
                    .tip(format!("Open {address}"))
                    .on_click(move |this, _window, cx| this.follow_link(&url.clone(), cx))
                    .on_menu(move |this, pos, window, cx| this.open_link_row_menu(&menu_url.clone(), pos, window, cx));
                if address != link.title {
                    pr = pr.detail(Some(address));
                }
                list = list.child(pr.render(p, cx));
            }
        }
        list.into_any_element()
    }

    /// `row`, a highlight (`HighlightsView.row`).
    fn highlight_row(&mut self, item: &Highlight, thread: &ThreadSummary, index: &str, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let is_commit = item.is_commit();
        let detail = if let Some(path) = &item.path {
            Some(core::text::folder(path, Some(&thread.cwd), &crate::files::home_dir()))
        } else if is_commit {
            Some(core::text::commit_line(item))
        } else {
            // Not when it says the same as the title (a bare localhost link).
            item.url.as_deref().map(core::text::address).filter(|a| *a != item.title)
        };
        // A pin says why in its note; the rest say how they turned up.
        let how = if item.is_pinned() || is_commit { None } else { Some(item.how.word()).filter(|w| !w.is_empty()).map(str::to_owned) };
        let mut pr = PaneRow::new(SharedString::from(format!("hl-row-{index}-{}", item.id)), item.title.clone())
            .icon(core::highlight_symbol(item))
            .lead(is_commit.then(|| item.sha.clone()).flatten())
            .trailing(how)
            .note(item.note.clone())
            .detail(detail);
        if !is_commit {
            let (open, thread2) = (item.clone(), thread.clone());
            pr = pr.on_click(move |this, _window, cx| {
                this.open_highlight(&open, &thread2, cx);
            });
        }
        let (menu_item, menu_thread) = (item.clone(), thread.clone());
        pr = pr.on_menu(move |this, pos, window, cx| this.open_highlight_menu(&menu_item, &menu_thread, pos, window, cx));
        pr.render(p, cx).into_any_element()
    }

    fn render_highlights(&mut self, thread: &ThreadSummary, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        if self.highlights_unsupported {
            return crate::files::pane_note("This runner doesn't keep highlights yet.", p).into_any_element();
        }
        // The first fetch is quick; a spinner would only flash.
        let Some(fetched) = self.highlights.get(&thread.id).cloned() else { return div().size_full().into_any_element() };
        let sections = HighlightSections::build(&fetched);
        let projects = core::project_rows(thread, |id| self.projects.get(id).cloned());
        let scheduled = self.schedule_rows(thread, cx);
        let clock = side::local_now();
        let now = now_ms();
        let started = match self.app.upgrade() {
            Some(app) => core::started_rows(thread, app.read(cx).threads.values(), now),
            None => vec![],
        };
        let subagents = core::subagents_for_highlights(thread);
        let asks = thread.asks.clone().unwrap_or_default();
        let tools: HashSet<String> = thread.patches().into_iter().filter(|p| !p.matched.clone().unwrap_or_default().is_empty()).map(|p| p.id).collect();
        let tool_panels: Vec<Panel> = thread.panels().into_iter().filter(|panel| tools.contains(&panel.patch)).collect();
        if sections.is_empty() && asks.is_empty() && projects.is_empty() && tool_panels.is_empty() && scheduled.is_empty() && subagents.is_empty() && started.is_empty() {
            return crate::files::pane_note("Nothing here yet. Files, links and commits from this thread show up here as it works.", p).into_any_element();
        }
        let flags = self.flags.get(&thread.id).copied().unwrap_or_default();
        let mut list = v_flex().pb(px(16.));

        for panel in tool_panels {
            list = list.child(crate::patches::tool_card(&panel, thread.id.clone(), p, cx));
        }
        if let Some(brief) = &sections.brief {
            let points = HighlightSections::points(brief);
            let many = points.len() > 1;
            let mut block = v_flex().gap(px(4.)).px(px(LEADING)).pt(px(14.)).pb(px(2.));
            for point in points {
                block = block.child(
                    h_flex()
                        .items_start()
                        .gap(px(6.))
                        // An old paragraph brief is one point: no bullet.
                        .when(many, |s| s.child(div().flex_none().text_size(px(style::size::LABEL)).text_color(p.secondary).child("•")))
                        .child(div().flex_1().min_w_0().text_size(px(style::size::LABEL)).line_height(px(17.)).text_color(p.secondary).child(point)),
                );
            }
            list = list.child(block);
        }
        // The last turn's asks while its card is open, ticked here or in the
        // card alike (the same `threads.tickAsk`).
        if let Some(seq) = thread.asks_seq.filter(|_| !asks.is_empty()) {
            let open = asks.iter().filter(|a| a.ticked != Some(true)).count();
            let folded = self.is_folded(fold::NEEDS_YOU);
            list = list.child(self.heading(Heading { key: fold::NEEDS_YOU.to_owned(), title: "Needs you".into(), icon: Some(icon("hand.raised", 11., p.secondary).into_any_element()), count: Some(open), folded, foldable: true, accessory: None }, p, cx));
            if !folded {
                for (index, ask) in asks.iter().enumerate() {
                    list = list.child(self.ask_row(ask, index, seq, thread.id.clone(), p, cx));
                }
            }
        }
        // The thread's projects, each under its own icon with why it's there
        // behind an ⓘ, then the places Mike keeps going back to for it.
        for row in &projects {
            list = list.child(self.project_block(row, p, cx));
        }
        if !scheduled.is_empty() {
            let folded = self.is_folded(fold::SCHEDULED);
            list = list.child(self.heading(Heading { key: fold::SCHEDULED.to_owned(), title: "Scheduled".into(), icon: Some(icon("clock", 11., p.secondary).into_any_element()), count: Some(scheduled.len()), folded, foldable: true, accessory: None }, p, cx));
            if !folded {
                for row in scheduled {
                    let pr = PaneRow::new(SharedString::from(format!("hl-sched-{}", row.id)), row.title).icon(if row.paused { "pause.circle" } else { "clock" }).trailing(row.when).detail(Some(row.detail));
                    list = list.child(pr.render(p, cx));
                }
            }
        }
        if !subagents.is_empty() {
            let folded = self.is_folded(fold::SUBAGENTS);
            list = list.child(self.heading(Heading { key: fold::SUBAGENTS.to_owned(), title: "Sub-agents".into(), icon: Some(icon("person.2", 11., p.secondary).into_any_element()), count: Some(subagents.len()), folded, foldable: true, accessory: None }, p, cx));
            if !folded {
                let total = subagents.len();
                let (shown, more) = HighlightSections::capped(&subagents, core::SUBAGENT_LIMIT, flags.all_subagents);
                for s in shown {
                    let (symbol, color) = match s.status.as_str() {
                        "running" => ("circle.dashed", None),
                        "completed" => ("checkmark", None),
                        "failed" => ("xmark", Some(p.failed)),
                        _ => ("minus", None),
                    };
                    let id = thread.id.clone();
                    let pr = PaneRow::new(SharedString::from(format!("hl-sub-{}", s.id)), side::subagent_title(s))
                        .icon(symbol)
                        .tinted(color)
                        .trailing(Some(side::subagent_duration(s, now)))
                        .detail(Some(side::subagent_detail(s)))
                        .tip(format!("{}. Click to read its transcript.", side::subagent_status_text(s)))
                        .on_click(move |this, window, cx| {
                            let id = id.clone();
                            if let Some(app) = this.app.upgrade() {
                                app.update(cx, |t, cx| t.open(id, window, cx));
                            }
                        });
                    list = list.child(pr.render(p, cx));
                }
                if more > 0 {
                    list = list.child(self.show_all_row("subagents", total, thread.id.clone(), p, cx));
                }
            }
        }
        // The threads it handed work to, or is waiting on, with where each is up to.
        if !started.is_empty() {
            let folded = self.is_folded(fold::STARTED_THREADS);
            list = list.child(self.heading(Heading { key: fold::STARTED_THREADS.to_owned(), title: "Threads it started".into(), icon: Some(icon("pawprint", 11., p.secondary).into_any_element()), count: Some(started.len()), folded, foldable: true, accessory: None }, p, cx));
            if !folded {
                let total = started.len();
                let (shown, more) = HighlightSections::capped(&started, core::SUBAGENT_LIMIT, flags.all_started);
                for row in shown {
                    let color = match row.state {
                        core::StartedState::NeedsYou => Some(p.accent),
                        core::StartedState::Failed => Some(p.failed),
                        _ => None,
                    };
                    let (id, title) = (row.thread.id.clone(), row.thread.title.clone());
                    let open_id = id.clone();
                    let pr = PaneRow::new(SharedString::from(format!("hl-started-{id}")), title)
                        .icon(core::started_icon(row.state))
                        .tinted(color)
                        .detail(Some(core::started_detail(row, &clock)))
                        .tip("Open this thread")
                        .on_click(move |this, window, cx| {
                            let id = open_id.clone();
                            if let Some(app) = this.app.upgrade() {
                                app.update(cx, |t, cx| t.open(id, window, cx));
                            }
                        });
                    list = list.child(pr.render(p, cx));
                }
                if more > 0 {
                    list = list.child(self.show_all_row("started", total, thread.id.clone(), p, cx));
                }
            }
        }
        if !sections.pinned.is_empty() {
            let folded = self.is_folded(fold::PINNED);
            list = list.child(self.heading(Heading { key: fold::PINNED.to_owned(), title: "Pinned".into(), icon: Some(icon("pin", 11., p.secondary).into_any_element()), count: Some(sections.pinned.len()), folded, foldable: true, accessory: None }, p, cx));
            if !folded {
                for item in &sections.pinned {
                    list = list.child(self.highlight_row(item, thread, "pinned", p, cx));
                }
            }
        }
        if !sections.files.is_empty() {
            let folded = self.is_folded(fold::FILES);
            list = list.child(self.heading(Heading { key: fold::FILES.to_owned(), title: "Files".into(), icon: Some(icon("doc", 11., p.secondary).into_any_element()), count: Some(sections.files.len()), folded, foldable: true, accessory: None }, p, cx));
            if !folded {
                for item in &sections.files {
                    list = list.child(self.highlight_row(item, thread, "files", p, cx));
                }
            }
        }
        if !sections.links.is_empty() {
            let folded = self.is_folded(fold::LINKS);
            list = list.child(self.heading(Heading { key: fold::LINKS.to_owned(), title: "Links".into(), icon: Some(icon("link", 11., p.secondary).into_any_element()), count: Some(sections.links.len()), folded, foldable: true, accessory: None }, p, cx));
            if !folded {
                let (shown, more) = HighlightSections::capped(&sections.links, core::LINK_LIMIT, flags.all_links);
                for item in shown {
                    list = list.child(self.highlight_row(item, thread, "links", p, cx));
                }
                if more > 0 {
                    list = list.child(self.show_all_row("links", sections.links.len(), thread.id.clone(), p, cx));
                }
            }
        }
        if !sections.commits.is_empty() || !sections.edited.is_empty() {
            let folded = self.is_folded(fold::CHANGES);
            let count = sections.commits.len() + sections.edited.len();
            list = list.child(self.heading(Heading { key: fold::CHANGES.to_owned(), title: "Changes".into(), icon: Some(icon("arrow.triangle.branch", 11., p.secondary).into_any_element()), count: Some(count), folded, foldable: true, accessory: None }, p, cx));
            if !folded {
                let (shown, more) = HighlightSections::capped(&sections.commits, core::COMMIT_LIMIT, flags.all_commits);
                for item in shown {
                    list = list.child(self.highlight_row(item, thread, "changes", p, cx));
                }
                if more > 0 {
                    list = list.child(self.show_all_row("commits", sections.commits.len(), thread.id.clone(), p, cx));
                }
                if !sections.edited.is_empty() {
                    list = list.child(self.edited_rows(&sections.edited, flags.edited_open, thread, p, cx));
                }
            }
        }
        let _ = window;
        div().id("highlights-scroll").size_full().overflow_y_scroll().track_scroll(&self.scroll).child(list).into_any_element()
    }

    fn schedule_rows(&self, thread: &ThreadSummary, cx: &App) -> Vec<core::ScheduleRow> {
        let threads = self.app.upgrade().map(|a| a.read(cx).threads.clone()).unwrap_or_default();
        core::schedule_rows(thread, &self.schedules, now_ms(), |s| match s.thread_id.as_deref() {
            Some(id) => threads.get(id).map_or_else(|| "A thread that's gone".to_owned(), |t| if t.title.is_empty() { "Untitled".to_owned() } else { t.title.clone() }),
            None => format!("New thread in {}", threadbear_core::inbox::folder_name(s.cwd.as_deref().unwrap_or(""))),
        })
    }

    fn ask_row(&mut self, ask: &threadbear_core::inbox::OpenAsk, index: usize, seq: u64, thread_id: String, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let ticked = ask.ticked == Some(true);
        let tip = if ticked { "Not done" } else { "Done" };
        let where_text = ask.where_text();
        let tick_color = if ticked { p.accent } else { p.icon };
        let label_color = if ticked { p.secondary } else { p.fg };
        let symbol = if ticked { "checkmark.square.fill" } else { "square" };
        let action = crate::files::plain_markdown(&ask.action);
        let (key, id2) = (format!("{thread_id}-{index}"), thread_id.clone());
        let button = tipped(div().id(SharedString::from(format!("hl-tick-{key}"))).flex_none().w(px(16.)).flex().justify_center().cursor_pointer().child(icon(symbol, 13., tick_color)), tip)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.tick_ask(id2.clone(), seq, index, !ticked, cx)));
        h_flex()
            .items_start()
            .gap(px(8.))
            .pl(px(LEADING))
            .pr(px(TRAILING + OUTER_INSET))
            .py(px(6.))
            .child(button)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(div().text_size(px(style::size::LABEL)).line_height(px(17.)).text_color(label_color).when(ticked, |s| s.line_through()).child(action))
                    .when_some(where_text, |s, place| s.child(div().text_size(px(style::size::SMALL)).text_color(p.faint).line_clamp(2).child(place))),
            )
            .into_any_element()
    }

    fn show_all_row(&mut self, which: &'static str, count: usize, thread_id: String, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let id: SharedString = format!("hl-showall-{which}").into();
        PaneRow::new(id, format!("Show all {count}"))
            .icon("ellipsis")
            .quiet()
            .on_click(move |this, _window, cx| {
                let flags = this.flags.entry(thread_id.clone()).or_default();
                match which {
                    "links" => flags.all_links = true,
                    "commits" => flags.all_commits = true,
                    "subagents" => flags.all_subagents = true,
                    _ => flags.all_started = true,
                }
                cx.notify();
            })
            .render(p, cx)
            .into_any_element()
    }

    fn edited_rows(&mut self, paths: &[String], open: bool, thread: &ThreadSummary, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut list = v_flex();
        let thread_id = thread.id.clone();
        list = list.child(
            PaneRow::new("hl-edited", core::text::edited_count(paths.len()))
                .icon(if open { "chevron.down" } else { "chevron.right" })
                .quiet()
                .on_click(move |this, _window, cx| {
                    let flags = this.flags.entry(thread_id.clone()).or_default();
                    flags.edited_open = !flags.edited_open;
                    cx.notify();
                })
                .render(p, cx),
        );
        if open {
            for path in paths {
                let (open_path, menu_path, title) = (path.clone(), path.clone(), core::last_component(path).to_owned());
                let (menu_title, thread2, menu_thread) = (title.clone(), thread.clone(), thread.clone());
                list = list.child(
                    PaneRow::new(SharedString::from(format!("hl-edited-{path}")), title)
                        .icon(core::file_symbol(path))
                        .detail(Some(core::text::folder(path, Some(&thread.cwd), &crate::files::home_dir())))
                        .indent()
                        .tip("Open in its app")
                        .on_click(move |this, _window, cx| {
                            // Code to read in an editor, not to review. Another Mac's
                            // copy is fetched and opened by files.rs.
                            if thread2.is_remote() {
                                this.preview_path(&open_path.clone(), None, &thread2, cx);
                            } else {
                                this.open_externally(&open_path.clone(), cx);
                            }
                        })
                        .on_menu(move |this, pos, window, cx| this.open_file_row_menu(&menu_path.clone(), &menu_title.clone(), &menu_thread, pos, window, cx))
                        .render(p, cx),
                );
            }
        }
        list.into_any_element()
    }

    // MARK: the section's top bar

    fn section_title(&self, thread: &ThreadSummary, section: &str) -> String {
        match section {
            HIGHLIGHTS => "Highlights".to_owned(),
            PATCHES => "Patches".to_owned(),
            FILES => "Files".to_owned(),
            id => thread.panels().into_iter().find(|p| p.id == id).map_or_else(|| id.to_owned(), |p| p.title),
        }
    }

    /// The header above the pane's content: a preview's Back and buttons, else
    /// the section's name, with the section icons along the right.
    fn render_header(&mut self, thread: &ThreadSummary, section: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let preview = self.previews.get(&thread.id).filter(|pv| pv.from == section).cloned();
        let mut row = h_flex().flex_none().h(px(HEADER_HEIGHT)).w_full().pl(px(14.)).pr(px(6.)).items_center().gap(px(6.)).border_b_1().border_color(p.hairline);
        if let Some(preview) = &preview {
            let thread_id = thread.id.clone();
            let back_to = preview.back.last().map_or_else(|| self.section_title(thread, &preview.from), |b| b.title.clone());
            row = row.child(self.header_button("hl-back", "chevron.left", format!("Back to {back_to}"), p, cx, move |this, cx| this.preview_back(&thread_id.clone(), cx)));
            row = row.child(div().flex_1().min_w_0().truncate().text_size(px(style::size::LABEL)).font_semibold().child(preview.title.clone()));
            row = row.child(self.preview_buttons(preview, thread, p, cx));
        } else {
            row = row.child(div().flex_1().min_w_0().truncate().text_size(px(style::size::LABEL)).font_semibold().child(self.section_title(thread, section)));
        }
        // Sections are switched from the window's right rail (rails.rs).
        row
    }

    pub(crate) fn header_button(&self, id: &'static str, symbol: &str, tip: String, p: Palette, cx: &mut Context<Self>, action: impl Fn(&mut SidePane, &mut Context<SidePane>) + 'static) -> impl IntoElement {
        let hover = p.hover;
        tipped(div().id(id).size(px(26.)).flex().flex_none().items_center().justify_center().rounded(px(6.)).hover(move |s| s.bg(hover)).cursor_pointer().child(icon(symbol, 13., p.secondary)), tip)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| action(this, cx)))
    }
}

impl Render for SidePane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let bg = cx.theme().background;
        let Some(thread) = self.thread(cx) else {
            return v_flex().size_full().bg(bg).child(crate::files::pane_note("Pick a thread to see its Highlights.", p)).into_any_element();
        };
        let section = self.section_for(&thread);
        // Only a preview has a header of its own here, for its Back and its
        // buttons. Otherwise the dock's bar above already names the section,
        // and the Swift pane has nothing between the two (PaneHeader is gone
        // from its panes); a second "Highlights" under the first looked off.
        let previewing = self.previews.get(&thread.id).is_some_and(|pv| pv.from == section);
        let header = previewing.then(|| self.render_header(&thread, &section, cx).into_any_element());
        let body = if let Some(preview) = self.previews.get(&thread.id).filter(|pv| pv.from == section).cloned() {
            self.render_preview(&thread, &preview, window, cx)
        } else {
            match section.as_str() {
                HIGHLIGHTS => self.render_highlights(&thread, window, cx),
                PATCHES => self.render_patches(&thread, cx),
                FILES => self.render_files_panel(&thread, window, cx),
                id => self.render_panel(&thread, id, window, cx),
            }
        };
        let menu = self.menu.as_ref().map(|m| (m.position, m.view.clone()));
        let popover = self.why_popover(&thread, p, window, cx);
        let viewer = self.viewer_overlay(window, cx);
        // The pane's width as drawn, for the folder list's columns (the
        // column hides under a width, FolderPaneView's layout).
        let measure = {
            let entity = cx.entity().downgrade();
            canvas(
                move |bounds, _, cx| {
                    let width = f32::from(bounds.size.width);
                    entity
                        .update(cx, |this, cx| {
                            if (this.width - width).abs() >= 1. {
                                this.width = width;
                                cx.notify();
                            }
                        })
                        .ok();
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full()
        };
        v_flex()
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(bg)
            .text_color(p.fg)
            .child(measure)
            .children(header)
            .child(div().flex_1().min_h_0().child(body))
            .when_some(menu, |s, (position, view)| s.child(deferred(anchored().position(position).snap_to_window_with_margin(px(8.)).child(div().occlude().child(view))).with_priority(1)))
            .when_some(popover, |s, card| s.child(card))
            .when_some(viewer, |s, v| s.child(v))
            .into_any_element()
    }
}

// ---------------------------------------------------------------- the dump (Swift's `panels.pane`)

impl SidePane {
    /// The `panels` part of the dump, in AppDump's shape.
    #[cfg(feature = "automation")]
    pub(crate) fn dump(&self, cx: &App) -> Value {
        let Some(thread) = self.thread(cx) else { return Value::Null };
        let section = self.section_for(&thread);
        let mut items = vec![json!({ "id": HIGHLIGHTS, "title": "Highlights", "kind": "highlights", "active": section == HIGHLIGHTS })];
        for panel in thread.panels() {
            items.push(json!({ "id": panel.id, "title": panel.title, "kind": panel.kind, "url": panel.url, "note": panel.note, "active": section == panel.id }));
        }
        items.push(json!({ "id": FILES, "title": "Files", "kind": "files", "active": section == FILES }));
        if thread.patches.as_ref().is_some_and(|p| !p.is_empty()) {
            let summary = thread.patches().iter().map(|p| if p.error.is_some() { format!("{} (error)", p.id) } else { p.id.clone() }).collect::<Vec<_>>().join(", ");
            items.push(json!({ "id": PATCHES, "title": "Patches", "kind": "patches", "note": summary, "active": section == PATCHES }));
        }
        let preview = self.previews.get(&thread.id).map(|pv| self.preview_line(pv));
        json!({ "shown": true, "width": PANE_WIDTH as i64, "items": items, "preview": preview, "highlights": self.highlights_line(&thread.id), "pane": self.pane_dump(&thread, &section, cx) })
    }

    /// `highlightsLine`.
    #[cfg(feature = "automation")]
    fn highlights_line(&self, id: &str) -> String {
        if self.highlights_unsupported {
            return "not kept by this runner".into();
        }
        let Some(fetched) = self.highlights.get(id) else { return "not fetched".into() };
        let s = HighlightSections::build(fetched);
        if s.is_empty() {
            return "empty".into();
        }
        let mut parts: Vec<String> = vec![];
        if s.brief.is_some() {
            parts.push("a brief".into());
        }
        for (name, n) in [("pinned", s.pinned.len()), ("files", s.files.len()), ("links", s.links.len()), ("commits", s.commits.len()), ("edited", s.edited.len())] {
            if n > 0 {
                parts.push(format!("{n} {name}"));
            }
        }
        if !self.collapsed.is_empty() {
            let mut folded: Vec<&String> = self.collapsed.iter().collect();
            folded.sort();
            parts.push(format!("folded: {}", folded.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")));
        }
        parts.join(", ")
    }

    #[cfg(feature = "automation")]
    fn pane_dump(&self, thread: &ThreadSummary, section: &str, cx: &App) -> Value {
        let mut pane = serde_json::Map::new();
        pane.insert("section".into(), json!(section));
        if let Some(folder) = self.folder_summary(thread, cx) {
            pane.insert("folder".into(), json!(folder));
        }
        if let Some(preview) = self.previews.get(&thread.id).filter(|pv| pv.from == section) {
            self.fill_preview_dump(&mut pane, preview, cx);
            return Value::Object(pane);
        }
        match section {
            HIGHLIGHTS => self.fill_highlights_dump(&mut pane, thread, cx),
            PATCHES => self.fill_patches_dump(&mut pane, thread),
            FILES => {}
            id => self.fill_panel_dump(&mut pane, thread, id),
        }
        Value::Object(pane)
    }

    #[cfg(feature = "automation")]
    fn fill_highlights_dump(&self, pane: &mut serde_json::Map<String, Value>, thread: &ThreadSummary, cx: &App) {
        if self.highlights_unsupported {
            pane.insert("empty".into(), json!("This runner doesn't keep highlights yet."));
            return;
        }
        let Some(fetched) = self.highlights.get(&thread.id) else { return };
        let sections = HighlightSections::build(fetched);
        let projects = core::project_rows(thread, |id| self.projects.get(id).cloned());
        let scheduled = self.schedule_rows(thread, cx);
        let (clock, now) = (side::local_now(), now_ms());
        let threads = self.app.upgrade().map(|a| a.read(cx).threads.clone()).unwrap_or_default();
        let started = core::started_rows(thread, threads.values(), now);
        let subagents = core::subagents_for_highlights(thread);
        let asks = thread.asks.clone().unwrap_or_default();
        let tools: HashSet<String> = thread.patches().into_iter().filter(|p| !p.matched.clone().unwrap_or_default().is_empty()).map(|p| p.id).collect();
        let tool_panels: Vec<Panel> = thread.panels().into_iter().filter(|panel| tools.contains(&panel.patch)).collect();
        if sections.is_empty() && asks.is_empty() && projects.is_empty() && tool_panels.is_empty() && scheduled.is_empty() && subagents.is_empty() && started.is_empty() {
            pane.insert("empty".into(), json!("Nothing here yet. Files, links and commits from this thread show up here as it works."));
            return;
        }
        let flags = self.flags.get(&thread.id).copied().unwrap_or_default();
        let mut out: Vec<Value> = vec![];
        let add = |key: &str, title: &str, count: usize, rows: Vec<Value>, out: &mut Vec<Value>| {
            let folded = self.collapsed.contains(key);
            out.push(json!({ "key": key, "title": title, "count": count, "folded": folded, "rows": if folded { vec![] } else { rows } }));
        };
        if !tool_panels.is_empty() {
            pane.insert("cards".into(), json!(tool_panels.iter().map(|p| match p.summary.as_ref().or(p.note.as_ref()) { Some(s) => format!("{}: {s}", p.title), None => p.title.clone() }).collect::<Vec<_>>()));
        }
        if let Some(brief) = &sections.brief {
            pane.insert("brief".into(), json!(HighlightSections::points(brief)));
        }
        if !asks.is_empty() {
            let rows = asks.iter().map(|a| json!({ "title": crate::files::plain_markdown(&a.action), "detail": a.where_text(), "symbol": if a.ticked == Some(true) { "checkmark.square.fill" } else { "square" }, "ticked": a.ticked == Some(true) })).collect();
            add(fold::NEEDS_YOU, "Needs you", asks.iter().filter(|a| a.ticked != Some(true)).count(), rows, &mut out);
        }
        for row in &projects {
            let rows = row.project.links.iter().map(|l| {
                let address = core::text::address(&l.url);
                json!({ "title": l.title, "detail": if address == l.title { None } else { Some(address) }, "symbol": "globe" })
            }).collect();
            add(&fold::project(&row.project.id), &row.project.title, row.project.links.len() + row.project.open_task_count(), rows, &mut out);
        }
        if !scheduled.is_empty() {
            let rows = scheduled.iter().map(|r| json!({ "title": r.title, "trailing": r.when, "detail": r.detail, "symbol": if r.paused { "pause.circle" } else { "clock" } })).collect();
            add(fold::SCHEDULED, "Scheduled", scheduled.len(), rows, &mut out);
        }
        if !subagents.is_empty() {
            let (shown, more) = HighlightSections::capped(&subagents, core::SUBAGENT_LIMIT, flags.all_subagents);
            let mut rows: Vec<Value> = shown
                .iter()
                .map(|s| {
                    let symbol = match s.status.as_str() {
                        "running" => "circle.dashed",
                        "completed" => "checkmark",
                        "failed" => "xmark",
                        _ => "minus",
                    };
                    json!({ "title": side::subagent_title(s), "trailing": side::subagent_duration(s, now), "detail": side::subagent_detail(s), "symbol": symbol })
                })
                .collect();
            if more > 0 {
                rows.push(json!({ "title": format!("Show all {}", subagents.len()), "symbol": "ellipsis" }));
            }
            add(fold::SUBAGENTS, "Sub-agents", subagents.len(), rows, &mut out);
        }
        if !started.is_empty() {
            let (shown, more) = HighlightSections::capped(&started, core::SUBAGENT_LIMIT, flags.all_started);
            let mut rows: Vec<Value> = shown.iter().map(|r| json!({ "title": r.thread.title, "detail": core::started_detail(r, &clock), "symbol": core::started_icon(r.state) })).collect();
            if more > 0 {
                rows.push(json!({ "title": format!("Show all {}", started.len()), "symbol": "ellipsis" }));
            }
            add(fold::STARTED_THREADS, "Threads it started", started.len(), rows, &mut out);
        }
        let item_row = |h: &Highlight| -> Value {
            let is_commit = h.is_commit();
            let detail = if let Some(path) = &h.path {
                Some(core::text::folder(path, Some(&thread.cwd), &crate::files::home_dir()))
            } else if is_commit {
                Some(core::text::commit_line(h))
            } else {
                h.url.as_deref().map(core::text::address).filter(|a| *a != h.title)
            };
            let how = if h.is_pinned() || is_commit { None } else { Some(h.how.word()).filter(|w| !w.is_empty()) };
            json!({ "title": h.title, "lead": if is_commit { h.sha.clone() } else { None }, "trailing": how, "note": h.note, "detail": detail, "symbol": core::highlight_symbol(h) })
        };
        if !sections.pinned.is_empty() {
            add(fold::PINNED, "Pinned", sections.pinned.len(), sections.pinned.iter().map(&item_row).collect(), &mut out);
        }
        if !sections.files.is_empty() {
            add(fold::FILES, "Files", sections.files.len(), sections.files.iter().map(&item_row).collect(), &mut out);
        }
        if !sections.links.is_empty() {
            let (shown, more) = HighlightSections::capped(&sections.links, core::LINK_LIMIT, flags.all_links);
            let mut rows: Vec<Value> = shown.iter().map(&item_row).collect();
            if more > 0 {
                rows.push(json!({ "title": format!("Show all {}", sections.links.len()), "symbol": "ellipsis" }));
            }
            add(fold::LINKS, "Links", sections.links.len(), rows, &mut out);
        }
        if !sections.commits.is_empty() || !sections.edited.is_empty() {
            let (shown, more) = HighlightSections::capped(&sections.commits, core::COMMIT_LIMIT, flags.all_commits);
            let mut rows: Vec<Value> = shown.iter().map(&item_row).collect();
            if more > 0 {
                rows.push(json!({ "title": format!("Show all {}", sections.commits.len()), "symbol": "ellipsis" }));
            }
            if !sections.edited.is_empty() {
                rows.push(json!({ "title": core::text::edited_count(sections.edited.len()), "symbol": if flags.edited_open { "chevron.down" } else { "chevron.right" } }));
                if flags.edited_open {
                    for path in &sections.edited {
                        rows.push(json!({ "title": core::last_component(path), "detail": core::text::folder(path, Some(&thread.cwd), &crate::files::home_dir()), "symbol": core::file_symbol(path) }));
                    }
                }
            }
            add(fold::CHANGES, "Changes", sections.commits.len() + sections.edited.len(), rows, &mut out);
        }
        pane.insert("sections".into(), Value::Array(out));
    }
}

/// Scripted commands for the pane, the same names and shapes as the Swift
/// app's (app/README.md): `panel`, `highlight`, `preview`, `folder`,
/// `trust`, `action`, `project-why`, and `collapse`/`expand` with
/// `highlightSection`.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    let side = app.read(cx).side.clone();
    let text = |key: &str| cmd[key].as_str().map(str::to_owned);
    let thread = side.read(cx).thread(cx).ok_or("no thread selected")?;
    match cmd["cmd"].as_str().unwrap_or("") {
        "expand" | "collapse" => {
            let key = text("highlightSection").ok_or("highlightSection needed")?;
            side.update(cx, |pane, cx| {
                let remove = cmd["cmd"] == "expand";
                if remove {
                    pane.collapsed.remove(&key);
                } else {
                    pane.collapsed.insert(key.clone());
                }
                crate::highlights_store::save_collapsed(&pane.collapsed);
                cx.notify();
            });
            Ok(json!("ok"))
        }
        "panel" => {
            if let Some(id) = text("id") {
                let ids = SidePane::section_ids(&thread);
                if !ids.contains(&id) {
                    return Err(format!("no panel {id}; this thread has {}", ids.join(", ")));
                }
                side.update(cx, |pane, cx| pane.choose_section(&thread.id.clone(), &id, cx));
            }
            let section = side.read(cx).section_for(&thread);
            Ok(json!(format!("ok showing {section}")))
        }
        "project-why" => {
            let id = cmd["id"].as_str();
            let answer = crate::highlights_project::project_why_command(&side, &thread, id, cx)?;
            Ok(json!(answer))
        }
        "highlight" => highlight_command(&side, &thread, cmd, window, cx),
        "preview" => crate::files::preview_command(&side, &thread, cmd, window, cx),
        "folder" => crate::folders::folder_command(&side, &thread, cmd, window, cx),
        "trust" => crate::patches::trust_command(&side, &thread, cmd, cx),
        "action" => crate::patches::action_command(&side, &thread, cmd, cx),
        other => Err(format!("unknown pane command {other:?}")),
    }
}

#[cfg(feature = "automation")]
fn highlight_command(side: &Entity<SidePane>, thread: &ThreadSummary, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    if let Some(index) = cmd["tick"].as_u64().map(|i| i as usize) {
        let asks = thread.asks.clone().unwrap_or_default();
        let ask = asks.get(index).ok_or_else(|| format!("no ask {index}"))?;
        let seq = thread.asks_seq.ok_or("no open card of asks")?;
        let ticked = ask.ticked != Some(true);
        side.update(cx, |pane, cx| pane.tick_ask(thread.id.clone(), seq, index, ticked, cx));
        return Ok(json!(if ticked { "ok ticked" } else { "ok unticked" }));
    }
    if let Some(open) = cmd["edited"].as_bool() {
        side.update(cx, |pane, cx| {
            pane.flags.entry(thread.id.clone()).or_default().edited_open = open;
            cx.notify();
        });
        return Ok(json!("ok"));
    }
    if let Some(which) = cmd["showAll"].as_str() {
        side.update(cx, |pane, cx| {
            let flags = pane.flags.entry(thread.id.clone()).or_default();
            match which {
                "links" => flags.all_links = true,
                "commits" => flags.all_commits = true,
                "subagents" => flags.all_subagents = true,
                "started" => flags.all_started = true,
                _ => {}
            }
            cx.notify();
        });
        return Ok(json!("ok"));
    }
    let pane = side.read(cx);
    let fetched = pane.highlights.get(&thread.id).ok_or("highlights not fetched")?.clone();
    let sections = HighlightSections::build(&fetched);
    let find = |title: &str| sections.pinned.iter().chain(&sections.files).chain(&sections.links).chain(&sections.commits).find(|h| h.title == title).cloned();
    if let Some(title) = cmd["open"].as_str() {
        let item = find(title).ok_or_else(|| format!("no row {title}"))?;
        if item.kind == HighlightKind::Commit {
            return Err("a commit doesn't open".into());
        }
        side.update(cx, |pane, cx| pane.open_highlight(&item, thread, cx));
        let shown = side.read(cx).previews.get(&thread.id).map_or_else(|| "no preview".to_owned(), |p| format!("showing {}", p.title));
        return Ok(json!(format!("ok {shown}")));
    }
    if let Some(title) = cmd["menu"].as_str() {
        let item = find(title).ok_or_else(|| format!("no row {title}"))?;
        let items = SidePane::highlight_menu(&item, thread);
        return Ok(json!(format!("ok {}", items.iter().map(|i| i.title.clone()).collect::<Vec<_>>().join(" | "))));
    }
    if let (Some(chosen), Some(title)) = (cmd["choose"].as_str(), cmd["row"].as_str()) {
        let item = find(title).ok_or_else(|| format!("no row {title}"))?;
        let action = SidePane::highlight_menu(&item, thread).into_iter().find(|i| i.title == chosen).and_then(|i| i.action).ok_or_else(|| format!("no menu item {chosen}"))?;
        side.update(cx, |pane, cx| pane.run_menu_action(action, thread, window, cx));
        let copied = cx.read_from_clipboard().and_then(|c| c.text()).unwrap_or_default();
        return Ok(json!(format!("ok {copied}")));
    }
    Err("highlight needs tick, edited, showAll, open, menu or choose".into())
}
