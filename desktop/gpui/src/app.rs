// The app's state: the connection, the thread list, what's open, and the
// window's shell around it (shell.rs, rails.rs, header.rs). The sidebar
// (sidebar.rs) and the thread pane (thread.rs) draw from it.
use crate::composer::Composer;
use crate::settings::Settings;
use crate::shell::{self, ArchivedNote, InboxEndNote, NavPlace, SidebarMode};
use crate::highlights::SidePane;
use crate::thread::ThreadPane;
use crate::{call, now_ms, ChannelSink, Seen, RT};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::Theme;
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use threadbear_client::config::RunnerConfig;
use threadbear_client::runner::{Client, Status};
use threadbear_core::inbox::{self, iso_time, ThreadSummary};
use threadbear_core::logins::RunnerLogins;
use threadbear_core::navigation::NavigationHistory;
use threadbear_core::shortcuts::ShortcutBindings;
use threadbear_dock::{DockLayout, DockModel, DockView};
use tokio::sync::mpsc;

pub struct Threadbear {
    pub(crate) client: Arc<Client>,
    pub(crate) status: Status,
    pub(crate) threads: HashMap<String, ThreadSummary>,
    pub(crate) list_error: Option<String>,
    pub(crate) selected: Option<String>,
    pub(crate) pane: Option<Entity<ThreadPane>>,
    /// The open pane's requests to the app (a thread: link was clicked).
    pane_events: Option<Subscription>,
    pub(crate) search: Entity<InputState>,
    /// The sections folded away under their headings, by key.
    pub(crate) folded: HashSet<&'static str>,
    /// The runner's accounts, pools and defaults, from its `hello`, for the
    /// composer's chips.
    pub(crate) logins: RunnerLogins,
    /// Each thread's message box, kept so what's typed (with its undo and
    /// pictures) stays with the thread while others are open.
    pub(crate) composers: HashMap<String, Entity<Composer>>,
    pub(crate) sidebar: crate::sidebar::SidebarState,

    // The window shell (shell.rs, rails.rs, header.rs).
    /// Holds the keyboard when nothing in the window has it (no thread open,
    /// say), so the app's keys and menus still reach the shell's actions.
    pub(crate) root_focus: FocusHandle,
    pub(crate) settings: Settings,
    pub(crate) settings_dirty: bool,
    pub(crate) settings_last_save_ms: i64,
    pub(crate) settings_save_task: Option<Task<()>>,
    pub(crate) dock: Entity<DockModel>,
    pub(crate) dock_view: Entity<DockView>,
    /// View > Show Frame Rate's readout, while it's showing.
    #[cfg(feature = "frame-stats")]
    pub(crate) frame_rate: Option<Entity<crate::frame_stats::FrameRate>>,
    /// The pane sections Hide Panels closed, for Show Panels (`hiddenPanes`).
    pub(crate) hidden_panes: Vec<String>,
    pub(crate) mode: SidebarMode,
    pub(crate) history: NavigationHistory<NavPlace>,
    /// shortcuts.json's keys, and the chords bound to them so far.
    pub(crate) shortcuts: ShortcutBindings,
    pub(crate) bound_keys: HashMap<String, shell::Command>,
    pub(crate) shortcuts_watch: Option<Task<()>>,
    pub(crate) shortcuts_path: Option<PathBuf>,
    /// What the menu bar says, so it's rebuilt only when that changes.
    pub(crate) menu_signature: String,
    /// Something changed since the menu bar was last worked out. It's
    /// rebuilt only then, not every frame: scrolling redraws the window as
    /// fast as the screen refreshes, and this was a noticeable part of each
    /// frame (set by an observer of the app itself, in `new`).
    pub(crate) menus_stale: bool,
    /// The login each thread was on when it went into error
    /// (`failedOnAccount`), so the limit line blames that login only while
    /// the thread's still on it, not the one it's just been switched to.
    pub(crate) failed_on_account: HashMap<String, String>,
    /// The Threads button's badge, kept until the app next changes (the same
    /// observer clears it), as working it out sorts every thread.
    pub(crate) inbox_count_cache: std::cell::Cell<Option<usize>>,
    pub(crate) last_window_title: String,
    pub(crate) archived_note: Option<ArchivedNote>,
    pub(crate) inbox_end_note: Option<InboxEndNote>,
    pub(crate) note_counter: u64,
    pub(crate) note_task: Option<Task<()>>,
    pub(crate) archived_task: Option<Task<()>>,
    /// The pointer's on a note, which keeps it up.
    pub(crate) toast_hovered: bool,
    /// The error bar's text (`errorMessage`).
    pub(crate) error_message: Option<String>,
    pub(crate) creating_investigation: bool,
    /// The title being renamed in the header, and its field.
    pub(crate) renaming: Option<String>,
    pub(crate) rename_field: Option<(Entity<InputState>, Subscription)>,
    pub(crate) share_popover: Option<String>,
    pub(crate) share_busy: bool,
    pub(crate) continuing_refusal: Option<String>,
    /// The runner's website, from `runner.status` (`websiteURL`).
    pub(crate) website_url: Option<String>,
    /// When the runner last said it was restarting.
    pub(crate) restart_notice_ms: Option<i64>,
    pub(crate) schedules_supported: bool,
    pub(crate) active_schedule_count: usize,
    pub(crate) projects_supported: bool,
    /// Every project's id, title and whether it's archived, for the
    /// header's chips (`projects.list`).
    pub(crate) all_projects: Vec<(String, String, bool)>,

    /// The new-thread page's box and what it's set to (new_thread.rs).
    pub(crate) new_thread: crate::new_thread::NewThreadState,
    /// Other runners' threads and logins (remote.rs).
    pub(crate) remote: crate::remote::RemoteState,
    /// Each thread's slash commands, for the composer's palette.
    pub(crate) slash: crate::slash_palette::SlashState,
    /// The Move sheet, while it's up (move_sheet.rs).
    pub(crate) move_sheet: Option<Entity<crate::move_sheet::MoveSheet>>,
    /// The pane beside the thread: Highlights, previews, Patches, Files (highlights.rs).
    pub(crate) side: Entity<SidePane>,
    /// The Threads panel's view, made the first time the dock asks for it
    /// (sidebar.rs, `SidebarView`).
    pub(crate) sidebar_view: Option<Entity<crate::sidebar::SidebarView>>,
    _events: Task<()>,
    _subs: Vec<Subscription>,
}

impl Threadbear {
    pub(crate) fn new(settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // gpui-kit starts in its light theme whatever the system says; follow
        // the system's light or dark, and keep following it when it changes,
        // in the system's own window colours (style.rs). Scroll bars show only
        // while scrolling, whatever the system's set to, as the Swift app's
        // transcript and composer have them (`scrollerStyle = .overlay`).
        crate::style::install_window_colours(cx);
        Theme::set_scrollbar_mode(gpui_kit::component::scroll::ScrollbarMode::Scrolling, cx);
        Theme::sync_system_appearance(Some(window), cx);
        let appearance = window.observe_window_appearance(|window, cx| Theme::sync_system_appearance(Some(window), cx));
        crate::style::set_zoom_percent(settings.zoom_percent());
        shell::apply_zoom_to_theme(settings.zoom_percent(), cx);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let client = Client::new(RunnerConfig::from_env(), ChannelSink(tx));
        RT.spawn(client.clone().run());
        let events = cx.spawn(async move |this, cx| {
            while let Some(seen) = rx.recv().await {
                if this.update(cx, |this, cx| this.on_seen(seen, cx)).is_err() {
                    break;
                }
            }
        });
        let status = client.status();
        let weak_app = cx.entity().downgrade();
        let side_client = client.clone();
        let side = cx.new(|cx| SidePane::new(weak_app, side_client, window, cx));
        crate::viewer::viewer_handle_global(side.downgrade(), cx);
        crate::open_gate::install(cx.entity().downgrade(), cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search threads"));
        // The filter is worked out at render, so a change only needs a redraw.
        let searching = cx.subscribe(&search, |_, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        });
        #[cfg(feature = "automation")]
        crate::automation::serve(cx.entity().downgrade(), window.window_handle(), cx);

        // The dock: the saved layout or the default, and a save whenever it
        // changes (a moment after the last change, as a divider drag is many).
        let layout = shell::make_layout(settings.saved.dock_layout.as_ref());
        let app = cx.entity().downgrade();
        let dock = cx.new(|_| {
            let mut model = DockModel::new(layout);
            let app = app.clone();
            model.on_change(move |layout: &DockLayout, cx: &mut App| {
                let layout = layout.clone();
                let app = app.clone();
                // Deferred: this runs inside whatever changed the layout,
                // which is often the app itself (a rail button, Reset).
                cx.defer(move |cx| {
                    let _ = app.update(cx, |this, cx| this.dock_layout_changed(layout, cx));
                });
            });
            model
        });
        let content = Rc::new(shell::Panels { app: app.clone() });
        let options = shell::dock_options(cx);
        let dock_view = {
            let model = dock.clone();
            cx.new(|cx| DockView::new(model, content, options, cx))
        };
        let frame = cx.observe_window_bounds(window, |this, window, cx| this.window_frame_changed(window, cx));
        // Everything the menus say is the app's state, which notifies when it
        // changes (the dock's layout too: `dock_layout_changed`).
        // Its 20-second tick notifies too (SidebarState), so the badge's
        // count follows a snooze running out as the list does.
        let menus = cx.observe(&cx.entity(), |this: &mut Threadbear, _, _| {
            this.menus_stale = true;
            this.inbox_count_cache.set(None);
        });

        let mut sidebar = crate::sidebar::SidebarState::new(cx);
        sidebar.order = settings.inbox_order();
        let folded: HashSet<&'static str> = match &settings.saved.collapsed_sections {
            // The section keys are the Swift enum's raw values.
            Some(saved) => ["starred", "inbox", "running", "hibernating", "archived"].into_iter().filter(|k| saved.iter().any(|s| s == k)).collect(),
            None => HashSet::from(["hibernating", "archived"]),
        };
        let mut this = Threadbear {
            client,
            status,
            threads: HashMap::new(),
            list_error: None,
            selected: None,
            pane: None,
            pane_events: None,
            search,
            folded,
            composers: HashMap::new(),
            logins: RunnerLogins::default(),
            sidebar,
            root_focus: cx.focus_handle(),
            settings,
            settings_dirty: false,
            settings_last_save_ms: 0,
            settings_save_task: None,
            dock,
            dock_view,
            #[cfg(feature = "frame-stats")]
            frame_rate: None,
            hidden_panes: Vec::new(),
            mode: SidebarMode::Inbox,
            history: NavigationHistory::default(),
            shortcuts: ShortcutBindings::default(),
            bound_keys: HashMap::new(),
            shortcuts_watch: None,
            shortcuts_path: None,
            menu_signature: String::new(),
            menus_stale: true,
            failed_on_account: HashMap::new(),
            inbox_count_cache: Default::default(),
            last_window_title: String::new(),
            archived_note: None,
            inbox_end_note: None,
            note_counter: 0,
            note_task: None,
            archived_task: None,
            toast_hovered: false,
            error_message: None,
            creating_investigation: false,
            renaming: None,
            rename_field: None,
            share_popover: None,
            share_busy: false,
            continuing_refusal: None,
            website_url: None,
            restart_notice_ms: None,
            schedules_supported: false,
            active_schedule_count: 0,
            projects_supported: false,
            all_projects: Vec::new(),
            new_thread: crate::new_thread::NewThreadState::new(),
            remote: Default::default(),
            slash: Default::default(),
            move_sheet: None,
            side,
            sidebar_view: None,
            _events: events,
            _subs: vec![appearance, searching, frame, menus],
        };
        // The window starts on the Inbox with nothing open (`noteNavigation`
        // on launch), so Back from the first thread comes back here.
        this.note_navigation();
        this.watch_shortcuts(cx);
        this
    }

    fn on_seen(&mut self, seen: Seen, cx: &mut Context<Self>) {
        match seen {
            Seen::Status(status) => {
                let connected = matches!(status, Status::Connected { .. });
                if let Status::Connected { hello, .. } = &status {
                    self.logins = RunnerLogins::from_hello(hello);
                    self.remote.experiments = hello["experiments"].as_array().into_iter().flatten().filter_map(|e| e.as_str().map(str::to_owned)).collect();
                }
                self.status = status;
                // Each time it's connected, the whole list again, and the
                // open thread read again: thread.updated only says what changed.
                if connected {
                    self.restart_notice_ms = None;
                    self.load_list(cx);
                    self.refresh_restart(cx);
                    self.load_projects(cx);
                    self.load_schedules(cx);
                    self.load_remote(cx);
                    self.side.update(cx, |side, cx| side.connected(cx));
                    if let Some(pane) = &self.pane {
                        pane.update(cx, |p, cx| p.load(cx));
                    }
                }
            }
            Seen::Note(method, params) => match method.as_str() {
                "thread.updated" => {
                    if let Ok(mut t) = serde_json::from_value::<ThreadSummary>(params["thread"].clone()) {
                        // Another runner's thread from this Mac's runner (the
                        // relay) stays marked as that runner's.
                        t.runner_id = self.threads.get(&t.id).and_then(|old| old.runner_id.clone());
                        if let Some(pane) = &self.pane {
                            pane.update(cx, |p, cx| p.summary_changed(&t, cx));
                        }
                        let before = self.threads.insert(t.id.clone(), t.clone());
                        self.note_failed_account(before.as_ref(), &t);
                    }
                }
                "thread.deleted" => {
                    let id = params["threadId"].as_str().unwrap_or("");
                    self.threads.remove(id);
                    self.failed_on_account.remove(id);
                }
                "runner.notice" => {
                    // The runner said it's restarting: the connection going
                    // away is expected for a while (`RunnerRestart.notice`).
                    if params["message"].as_str().is_some_and(|m| m.contains("restart")) {
                        self.restart_notice_ms = Some(now_ms());
                    }
                    self.refresh_restart(cx);
                }
                "projects.updated" => {
                    self.sidebar.project_icons = crate::sidebar::project_icons(&params["projects"]);
                    self.sidebar.projects = crate::sidebar::project_briefs(&params["projects"]);
                    self.all_projects = project_list(&params["projects"]);
                    self.side.update(cx, |side, cx| side.projects_updated(&params["projects"], cx));
                }
                "schedules.updated" => {
                    self.load_schedules(cx);
                    self.side.update(cx, |side, cx| side.schedules_updated(&params["schedules"], cx));
                }
                "remoteThreads.updated" => self.receive_remote(&params, cx),
                "thread.event" => {
                    if let Some(pane) = &self.pane {
                        pane.update(cx, |p, cx| p.event(&params, cx));
                    }
                }
                _ => {}
            },
        }
        cx.notify();
    }

    fn load_list(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.list", json!({ "include": ["inbox", "snoozed", "archived"] })).await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(v) => {
                        let list: Vec<ThreadSummary> = serde_json::from_value(v["threads"].clone()).unwrap_or_default();
                        let before = std::mem::take(&mut this.threads);
                        this.threads = list.into_iter().map(|t| (t.id.clone(), t)).collect();
                        this.note_failed_accounts(&before);
                        // Other runners' rows stay listed beside these.
                        this.apply_remote_index(cx);
                        this.list_error = None;
                        this.sidebar.loaded = true;
                    }
                    Err(e) => this.list_error = Some(e.message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Whether the runner has schedules at all, and how many are active
    /// (`schedulesSupported`, `activeScheduleCount`), for the activity bar.
    fn load_schedules(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "schedules.list", json!({})).await;
            this.update(cx, |this, cx| {
                if let Ok(v) = res {
                    this.schedules_supported = true;
                    this.active_schedule_count = v["schedules"].as_array().map_or(0, |list| list.iter().filter(|s| schedule_is_active(s)).count());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn open(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(id.as_str()) {
            return;
        }
        let Some(summary) = self.threads.get(&id).cloned() else { return };
        // Another runner's thread: its logins fill the pickers once they've
        // come (`openClient`'s `loadRelayLogins`).
        if let Some(runner) = summary.runner_id.clone() {
            self.load_remote_logins(runner, cx);
        }
        if let Some(old) = &self.pane {
            old.read(cx).unsubscribe();
        }
        let client = self.client.clone();
        let app = cx.entity().downgrade();
        let composer = self
            .composers
            .entry(id.clone())
            .or_insert_with(|| cx.new(|cx| Composer::new(Some(id.clone()), client.clone(), app, window, cx)))
            .clone();
        self.selected = Some(id);
        let pane = cx.new(|cx| ThreadPane::new(client, summary, composer.clone(), cx));
        composer.update(cx, |c, cx| {
            c.attach(pane.downgrade());
            c.focus(window, cx);
            cx.notify();
        });
        // A thread: link in the transcript opens that thread.
        self.pane_events = Some(cx.subscribe_in(&pane, window, |this, _, event: &crate::rows::OpenThreadRequest, window, cx| {
            let matches: Vec<String> = this.threads.keys().filter(|k| **k == event.0 || k.starts_with(&event.0)).cloned().collect();
            if let [only] = matches.as_slice() {
                this.open(only.clone(), window, cx);
            }
        }));
        self.pane = Some(pane);
        self.note_navigation();
        cx.notify();
    }

    /// Nothing open: the new-thread screen's place in the history. For going
    /// Back to it (`show(place)` with no thread).
    pub(crate) fn select_none(&mut self, cx: &mut Context<Self>) {
        if let Some(old) = self.pane.take() {
            old.read(cx).unsubscribe();
        }
        self.selected = None;
        self.note_navigation();
        cx.notify();
    }

    /// `threads.update` for the open thread, with the summary changed here
    /// first: anything clicked shows at once, and the runner's answer (or its
    /// thread.updated) puts it right.
    pub(crate) fn update_open(&mut self, change: impl FnOnce(&mut ThreadSummary), params: Value, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(id) = self.selected.clone() else { return Err("no thread open".into()) };
        let before = self.threads.get(&id).cloned();
        if let Some(t) = self.threads.get_mut(&id) {
            change(t);
        }
        let mut params = params;
        params["threadId"] = json!(id);
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.update", params).await;
            this.update(cx, |this, cx| {
                if let Err(e) = res {
                    this.list_error = Some(e.message);
                    if let Some(before) = before {
                        this.threads.insert(before.id.clone(), before);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
        Ok(())
    }

    #[cfg(feature = "automation")]
    pub(crate) fn snooze_open(&mut self, seconds: i64, cx: &mut Context<Self>) -> Result<(), String> {
        let until = iso_time(now_ms() + seconds * 1000);
        let local = until.clone();
        self.update_open(move |t| t.snoozed_until = Some(local), json!({ "snoozedUntil": until }), cx)
    }

    pub(crate) fn stop_open(&mut self, cx: &mut Context<Self>) {
        if let Some(pane) = &self.pane {
            pane.update(cx, |p, cx| p.stop(cx));
        }
    }

    #[cfg(feature = "automation")]
    pub(crate) fn composer(&self, cx: &App) -> Option<Entity<Composer>> {
        // The open thread's box, or the new-thread page's.
        self.pane.as_ref().map(|p| p.read(cx).composer.clone()).or_else(|| self.new_thread.composer.clone())
    }

    #[cfg(feature = "automation")]
    pub(crate) fn set_row_open(&mut self, row: &str, open: bool, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(pane) = &self.pane else { return Err("no thread open".into()) };
        pane.update(cx, |p, cx| p.set_row_open(row, open, cx))
    }

    #[cfg(feature = "automation")]
    pub(crate) fn load_all_events(&mut self, cx: &mut Context<Self>) {
        if let Some(pane) = &self.pane {
            pane.update(cx, |p, cx| p.load_all_events(cx));
        }
    }

    /// Runs `f` on the open pane, for the automation commands.
    #[cfg(feature = "automation")]
    pub(crate) fn update_pane<R>(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut ThreadPane, &mut Context<ThreadPane>) -> R) -> Result<R, String> {
        let Some(pane) = &self.pane else { return Err("no thread open".into()) };
        Ok(pane.update(cx, f))
    }

    /// A thread named by the start of its id, as the Swift app's automation has it.
    #[cfg(feature = "automation")]
    pub(crate) fn thread_id_from(&self, start: &str) -> Result<String, String> {
        let mut found = self.threads.keys().filter(|id| id.starts_with(start));
        match (found.next(), found.next()) {
            (Some(id), None) => Ok(id.clone()),
            (None, _) => Err(format!("no thread starting {start:?}")),
            _ => Err(format!("more than one thread starts {start:?}")),
        }
    }

    /// The window as data, in the Swift app's shape (app/Sources/Threadbear/AppDump.swift).
    #[cfg(feature = "automation")]
    pub(crate) fn dump(&self, cx: &App) -> Value {
        let connection = match self.status {
            Status::Connected { .. } => "connected",
            Status::Connecting { .. } => "connecting",
            Status::Disconnected { .. } => "lost",
        };
        let pane = self.pane.as_ref().map(|p| p.read(cx));
        let dock = self.dock.read(cx).layout().summary().replace("threadbear.", "");
        let mut sidebar = self.sidebar_dump(cx);
        sidebar["mode"] = json!(self.mode.name());
        json!({
            "connection": connection,
            "selection": self.selection_dump(),
            "sidebar": sidebar,
            "composer": self.dump_composer(cx),
            "newThread": crate::new_thread::page_dump(self, cx),
            "palette": self.composer(cx).map_or(Value::Null, |c| c.read(cx).palette_dump(cx)),
            "moveSheet": self.move_sheet.as_ref().map_or(Value::Null, |s| s.read(cx).dump(cx)),
            "transcript": pane.map(|p| p.dump(cx)).unwrap_or_else(|| json!({ "rows": [] })),
            "decisions": pane.and_then(|p| p.composer.read(cx).decision_lines(cx)),
            "banner": self.banner_dump(),
            "limit": self.limit_dump(cx),
            "error": self.error_message.clone(),
            "dock": dock,
            "zoom": crate::style::zoom_percent(),
            "archivedNote": self.archived_note.as_ref().map(|n| n.thread.clone()),
            "inboxEndNote": self.inbox_end_note.as_ref().map(|n| if n.empty { "empty" } else { "end" }),
            "rails": self.rails_dump(cx),
            "header": self.header_dump(),
            "panels": self.side.read(cx).dump(cx),
            "actions": self.side.read(cx).thread(cx).map(|t| crate::patches::actions_dump(self.side.read(cx), &t)).unwrap_or_else(|| json!([])),
        })
    }

    /// The composer's part of the dump (AppDump.composer).
    #[cfg(feature = "automation")]
    fn dump_composer(&self, cx: &App) -> Value {
        let composer = self.composer(cx);
        let composer = composer.as_ref().map(|c| c.read(cx));
        let mut out = json!({
            "draft": self.selected.clone().unwrap_or_else(|| "new".into()),
            "text": composer.map(|c| c.draft_text(cx)).unwrap_or_default(),
            "images": composer.map_or(0, |c| c.draft_images(cx)),
            "creating": self.selected.is_none() && self.new_thread.creating,
        });
        if let Some(composer) = composer {
            let data = composer.chip_data(cx);
            if let Some(t) = &data.summary {
                out["cwd"] = json!(t.cwd);
                out["account"] = json!(t.account_id);
                out["mode"] = json!(if t.permission_mode.is_empty() { "full" } else { t.permission_mode.as_str() });
            }
            if let Some(away) = &data.away {
                out["runner"] = json!(away.name);
            }
            out["chip"] = json!(crate::composer_chips::model_chip_text(&data));
            out["chipBadge"] = json!(crate::composer_chips::model_chip_badge_text(&data));
            if let Some(card) = composer.card_lines(cx) {
                out["card"] = json!(card);
            }
        }
        out
    }

    /// The sections with the search applied, as they're drawn.
    pub(crate) fn shown_sections(&self, cx: &App, now: i64) -> Vec<inbox::Section> {
        let query = self.search.read(cx).value().trim().to_lowercase();
        inbox::sections_ordered(self.threads.values().filter(|t| query.is_empty() || inbox::matches(t, &query)), now, self.sidebar.order)
    }

    pub(crate) fn connection_note(&self) -> Option<String> {
        match &self.status {
            Status::Connected { .. } => None,
            Status::Connecting { .. } => Some("Connecting to the runner…".into()),
            Status::Disconnected { reason, retry_in_ms, .. } => Some(format!("Can't reach the runner: {reason}. Trying again in {}s.", retry_in_ms.div_ceil(1000))),
        }
    }
}

pub(crate) fn project_list(projects: &Value) -> Vec<(String, String, bool)> {
    projects.as_array().map_or_else(Vec::new, |list| list.iter().filter_map(|p| Some((p["id"].as_str()?.to_string(), p["title"].as_str()?.to_string(), p["archived"].as_bool().unwrap_or(false)))).collect())
}

/// `Schedule.isActive`: not paused and not finished.
fn schedule_is_active(schedule: &Value) -> bool {
    !schedule["paused"].as_bool().unwrap_or(false) && schedule["finishedAt"].is_null() && !schedule["finished"].as_bool().unwrap_or(false)
}

impl Render for Threadbear {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.menus_stale) {
            self.sync_menus(cx);
        }
        let open_menu = self.render_open_menu();
        #[cfg(feature = "frame-stats")]
        let open_menu = open_menu.into_iter().map(IntoElement::into_any_element).chain(self.frame_rate.clone().map(|f| f.into_any_element()));
        self.render_shell(window, cx).children(open_menu)
    }
}
