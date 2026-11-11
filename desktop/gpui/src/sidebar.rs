// The left side of the window, a port of the Swift app's SidebarView and its
// friends (InboxList, ThreadSection, SectionHeading, ThreadRowView, RowActions,
// ThreadMenu, SnoozeMenu, SubagentList, ArchiveRows, SearchField). What a row
// says comes from threadbear_core::sidebar and its sizes and colours from
// style.rs; this file draws them and answers clicks. Every click shows at once
// (`update_thread` changes the summary before the runner hears), and the one
// slow thing with no optimistic state, deleting a chat, shows a spinner in its
// place and stops taking clicks until it lands.
use crate::app::Threadbear;
use crate::{call, icons, now_ms, style};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _, StyledExt as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use threadbear_core::inbox::{iso_time, InboxOrder, ThreadSummary};
use threadbear_core::sidebar::{self as rows, RowAction, RowLook, RowMode, Tile, TrailingKind};

type Clock = threadbear_core::chrono::DateTime<threadbear_core::chrono::Local>;

/// The Threads panel as a view of its own, so the dock can reuse its last
/// frame (`cached` in shell.rs). Scrolling the transcript redraws the window
/// every frame, and before this every frame rebuilt every row of the thread
/// list as well: with 60 threads, about half the frames on a 120 Hz screen
/// were late. Everything it draws is the app's state, so it draws again
/// whenever the app notifies, which the scroll doesn't.
pub(crate) struct SidebarView {
    app: WeakEntity<Threadbear>,
    _app_changed: Subscription,
}

impl SidebarView {
    pub(crate) fn new(app: &Entity<Threadbear>, cx: &mut Context<Self>) -> Self {
        let app_changed = cx.observe(app, |_, _, cx| cx.notify());
        Self { app: app.downgrade(), _app_changed: app_changed }
    }
}

impl Render for SidebarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.app.upgrade() {
            Some(app) => app.update(cx, |t, cx| t.render_threads_panel(window, cx)),
            None => div().into_any_element(),
        }
    }
}

/// A project's icon (protocol `ProjectIcon`): a picture, else a symbol.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectIcon {
    pub(crate) image_path: Option<String>,
    pub(crate) symbol: Option<String>,
}

/// What the new-thread page needs to know of a project (`Project`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectBrief {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) folders: Vec<String>,
}

/// Each project's id, title and folders from a `projects.list` answer.
pub(crate) fn project_briefs(projects: &Value) -> Vec<ProjectBrief> {
    projects
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(ProjectBrief {
                id: p["id"].as_str()?.to_string(),
                title: p["title"].as_str().unwrap_or("").to_string(),
                folders: p["folders"].as_array().into_iter().flatten().filter_map(|f| f.as_str().map(str::to_owned)).collect(),
            })
        })
        .collect()
}

/// Each project's icon from a `projects.list` (or `projects.updated`) answer.
pub(crate) fn project_icons(projects: &Value) -> HashMap<String, ProjectIcon> {
    projects
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let id = p["id"].as_str()?.to_string();
            let icon = p.get("icon")?;
            let image_path = icon.get("image").and_then(|i| i.get("path")).and_then(Value::as_str).map(String::from);
            let symbol = icon.get("symbol").and_then(Value::as_str).map(String::from);
            Some((id, ProjectIcon { image_path, symbol }))
        })
        .collect()
}

/// A menu open over a row or a heading: its rows (for scripts) and the view.
pub(crate) struct OpenMenu {
    /// The thread whose row it belongs to, which keeps its buttons showing.
    pub(crate) thread: Option<String>,
    position: Point<Pixels>,
    view: Entity<PopupMenu>,
    /// What's on it, for scripts (`menu-popover`, `realMenu`).
    #[cfg_attr(not(feature = "automation"), allow(dead_code))]
    pub(crate) rows: Vec<MenuRow>,
    /// Hibernate choices ask first if the thread's busy: the moon's menu does,
    /// the row's right-click one doesn't (`SnoozeNSMenu`, `SnoozeMenuItems`).
    confirm_busy: bool,
    _dismissed: Subscription,
}

/// One line in a menu (Swift builds these as Buttons in `ThreadMenu`).
#[derive(Clone)]
pub(crate) struct MenuRow {
    pub(crate) title: String,
    symbol: Option<&'static str>,
    action: Option<MenuAction>,
    checked: bool,
    enabled: bool,
    children: Vec<MenuRow>,
    separator: bool,
}

impl MenuRow {
    pub(crate) fn item(title: impl Into<String>, symbol: &'static str, action: MenuAction) -> Self {
        Self { title: title.into(), symbol: Some(symbol), action: Some(action), checked: false, enabled: true, children: vec![], separator: false }
    }
    pub(crate) fn separator() -> Self {
        Self { title: String::new(), symbol: None, action: None, checked: false, enabled: false, children: vec![], separator: true }
    }
    pub(crate) fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
    pub(crate) fn submenu(title: impl Into<String>, symbol: &'static str, children: Vec<MenuRow>) -> Self {
        Self { title: title.into(), symbol: Some(symbol), action: None, checked: false, enabled: true, children, separator: false }
    }
    /// A line of the new-thread box's Runs on menu: `star` marks the default
    /// (`markDefault`'s ★, drawn on the title; this menu has no badge yet).
    /// A ticked line has no symbol: the kit's menu shows the tick in the
    /// icon's place, and draws no tick at all beside one.
    pub(crate) fn runner(title: impl Into<String>, symbol: &'static str, checked: bool, star: bool, action: MenuAction) -> Self {
        let mut title: String = title.into();
        if star {
            title.push_str("  ★");
        }
        Self { title, symbol: (!checked).then_some(symbol), action: Some(action), checked, enabled: true, children: vec![], separator: false }
    }
    /// A folder on the runner a new thread will start on (Runs on's Folder).
    pub(crate) fn runner_folder(folder: String, checked: bool) -> Self {
        Self { title: folder.clone(), symbol: (!checked).then_some("folder"), action: Some(MenuAction::DraftFolder(folder)), checked, enabled: true, children: vec![], separator: false }
    }
}

/// What choosing a menu line does.
#[derive(Clone, Debug)]
pub(crate) enum MenuAction {
    Star(bool),
    Archive,
    Unarchive,
    Wake,
    Hibernate(&'static str),
    HibernateUntilRestart,
    PickTime,
    WakeWhen,
    Delete,
    Rename,
    CopyId,
    Sort(InboxOrder),
    OpenInPanel,
    /// The header's ... menu (header.rs).
    CopyTranscript,
    OpenOnWeb,
    OpenProject,
    ThreadProject { thread: String, add: Option<String>, remove: Option<String> },
    /// Runs on: where the new thread starts, its default, and a folder on
    /// the chosen runner (new_thread.rs).
    Runner(Option<String>),
    DefaultRunner(Option<String>),
    DraftFolder(String),
    /// Move to ▸ a runner, and Cancel Move once it's going (move_sheet.rs).
    MoveTo(String, String),
    CancelMove,
}

/// The sidebar's own state; the app holds one of these (`Threadbear::sidebar`).
pub(crate) struct SidebarState {
    pub(crate) order: InboxOrder,
    /// The row under the pointer.
    pub(crate) hovered: Option<String>,
    /// As `hover` in the Swift app's automation: a row shown as if the
    /// pointer were over it, so scripts don't move a real pointer.
    pub(crate) hover_override: Option<String>,
    pub(crate) archive_limit: usize,
    pub(crate) menu: Option<OpenMenu>,
    /// Work on the way that has no optimistic state ("delete:<id>"): its
    /// control shows a spinner and stops taking clicks (`AppModel+Busy`).
    pub(crate) busy: HashSet<String>,
    /// The runner's first list has come; until then, loading rows.
    pub(crate) loaded: bool,
    /// A runner restart is waiting (`runner.status`), so the Hibernate menu
    /// offers "Until the runner restarts".
    pub(crate) restart_pending: bool,
    /// Each project's icon by id, for the tiles (`projects.list`).
    pub(crate) project_icons: HashMap<String, ProjectIcon>,
    /// The projects themselves, for the new-thread page's chip.
    pub(crate) projects: Vec<ProjectBrief>,
    scroll: ScrollHandle,
    _tick: Task<()>,
}

impl SidebarState {
    pub(crate) fn new(cx: &mut Context<Threadbear>) -> Self {
        // Redraws once a second while something ticks (a working row's timer,
        // a sub-agent's duration), otherwise every 20 s so "5m" moves on.
        let tick = cx.spawn(async move |this, cx| {
            let mut n = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                n = n.wrapping_add(1);
                let alive = this.update(cx, |t, cx| {
                    let ticking = t.threads.values().any(|t| t.busy() || t.subagents.iter().flatten().any(|s| s.get("status").and_then(Value::as_str) == Some("running")));
                    if n % 20 == 0 || ticking {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        SidebarState {
            order: InboxOrder::default(),
            hovered: None,
            hover_override: None,
            archive_limit: rows::ARCHIVE_FIRST,
            menu: None,
            busy: HashSet::new(),
            loaded: false,
            restart_pending: false,
            project_icons: HashMap::new(),
            projects: Vec::new(),
            scroll: ScrollHandle::new(),
            _tick: tick,
        }
    }
}

/// The colours a frame of the sidebar uses, picked once (`cx.theme()` borrows
/// the whole app, which the row listeners also need).
#[derive(Clone, Copy)]
struct Palette {
    fg: Hsla,
    secondary: Hsla,
    faint: Hsla,
    accent: Hsla,
    failed: Hsla,
    hover: Hsla,
    selected: Hsla,
    tile: Hsla,
    tile_empty: Hsla,
    tile_ring: Hsla,
    hairline: Hsla,
    field_fill: Hsla,
    separator: Hsla,
    icon_hover: Hsla,
    icon_pressed: Hsla,
    guide: Hsla,
    skeleton: Hsla,
    side: Hsla,
}

fn palette(cx: &App) -> Palette {
    let theme = cx.theme();
    Palette {
        fg: theme.sidebar_foreground,
        secondary: style::secondary_text(cx),
        faint: style::faint_text(cx),
        accent: style::accent(cx),
        failed: style::failed(cx),
        hover: style::sidebar_hover(cx),
        selected: style::sidebar_selected(cx),
        tile: style::primary_opacity(cx, 0.08),
        tile_empty: style::primary_opacity(cx, 0.04),
        tile_ring: theme.sidebar,
        hairline: style::primary_opacity(cx, 0.08),
        field_fill: style::primary_opacity(cx, 0.035),
        separator: theme.sidebar_border,
        icon_hover: style::primary_opacity(cx, 0.08),
        icon_pressed: style::primary_opacity(cx, 0.14),
        guide: style::primary_opacity(cx, 0.16),
        skeleton: style::primary_opacity(cx, 0.07),
        side: theme.sidebar,
    }
}

/// `SectionMetrics` etc: the sidebar's measurements at 100%.
mod metrics {
    /// From the sidebar's edges to a row's highlight.
    pub const OUTER_INSET: f32 = 8.;
    /// Inside a row's highlight, left of its text: wider than the right side
    /// to leave a gutter for the unread dot, so titles line up with or without.
    pub const LEADING: f32 = 18.;
    pub const TRAILING: f32 = 10.;
    pub const CORNER: f32 = 8.;
    pub const TILE: f32 = 30.;
    /// `SubagentRowView.trunk`: the tree line runs down the middle of the tile.
    pub const TRUNK: f32 = LEADING + 15.;
    pub const INDENT: f32 = 18.;
}
use metrics::*;

/// Under where a button was clicked, where a dropdown opens (`MenuAnchor`'s
/// `below`), so the first item isn't under the pointer as the mouse lifts.
fn below(position: Point<Pixels>) -> Point<Pixels> {
    point(position.x, position.y + px(12.))
}

/// An SF Symbol's icon, at a point size and in a colour.
fn icon(name: &str, size: f32, color: Hsla) -> Icon {
    icons::symbol(name).with_size(px(size)).text_color(color)
}

/// A button-like control: its tooltip, and the label scripts find it by, as
/// the Swift app's `.tip` is both. The macro keeps the two in one place.
macro_rules! tipped {
    ($el:expr, $id:expr, $tip:expr) => {{
        let tip_text: SharedString = $tip.into();
        let shown = tip_text.clone();
        $el.id($id).test_support().aria_label(tip_text).tooltip(move |window, cx| Tooltip::new(shown.clone()).build(window, cx))
    }};
}

/// `RowIconButtonStyle`: a 22 pt borderless icon button with its own faint
/// hover square.
fn icon_button(id: impl Into<ElementId>, symbol: Icon, tip: impl Into<SharedString>, p: &Palette, on_press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    let (hover, pressed, secondary, fg) = (p.icon_hover, p.icon_pressed, p.secondary, p.fg);
    tipped!(div().flex_none().size(px(22.)).rounded(px(5.)).flex().items_center().justify_center().hover(move |s| s.bg(hover).text_color(fg)).text_color(secondary), id, tip)
        .active(move |s| s.bg(pressed))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |ev, window, cx| on_press(ev, window, cx))
        .child(symbol)
}

impl Threadbear {
    /// `threads.update` for any thread, changing the summary here first:
    /// anything clicked shows at once, and the runner's answer (or its
    /// thread.updated) puts it right or rolls it back.
    pub(crate) fn update_thread(&mut self, id: &str, change: impl FnOnce(&mut ThreadSummary), params: Value, cx: &mut Context<Self>) {
        let before = self.threads.get(id).cloned();
        if let Some(t) = self.threads.get_mut(id) {
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
    }

    /// The Starred, Inbox and Running rows in order (`sections.all`), for
    /// picking what to open when the open thread goes away.
    fn live_row_ids(&self, cx: &App, now: i64) -> Vec<String> {
        self.shown_sections(cx, now).into_iter().filter(|s| matches!(s.key, "starred" | "inbox" | "running")).flat_map(|s| s.rows.into_iter().map(|t| t.id)).collect()
    }

    /// The row after this one, or before it when it's the last
    /// (`AppModel.neighbour`).
    fn neighbour_of(&self, id: &str, cx: &App, now: i64) -> Option<String> {
        let ordered = self.live_row_ids(cx, now);
        let index = ordered.iter().position(|x| x == id)?;
        ordered.get(index + 1).or_else(|| index.checked_sub(1).and_then(|i| ordered.get(i))).cloned()
    }

    /// Moves off a thread that's leaving the list, if it's the open one.
    fn leave_selection(&mut self, id: &str, next: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.as_deref() != Some(id) {
            return;
        }
        match next {
            Some(next) => self.open(next, window, cx),
            None => {
                self.selected = None;
                self.pane = None;
                cx.notify();
            }
        }
    }

    pub(crate) fn star_row(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        let at = on.then(|| iso_time(now_ms()));
        self.update_thread(id, move |t| t.starred_at = at, json!({ "starred": on }), cx);
    }

    pub(crate) fn archive_row(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.neighbour_of(&id, cx, now_ms());
        let at = iso_time(now_ms());
        // "Archived" and Undo, at the top of the panel (`ArchivedToast`).
        if let Some(title) = self.threads.get(&id).map(|t| t.title.clone()) {
            let was_open = self.selected.as_deref() == Some(id.as_str());
            self.show_archived_note(id.clone(), title, was_open, cx);
        }
        self.update_thread(&id, move |t| t.archived_at = Some(at), json!({ "archived": true, "archivedBy": "mike" }), cx);
        self.leave_selection(&id, next, window, cx);
    }

    pub(crate) fn unarchive_row(&mut self, id: &str, cx: &mut Context<Self>) {
        self.update_thread(id, |t| t.archived_at = None, json!({ "archived": false }), cx);
    }

    pub(crate) fn wake_row(&mut self, id: &str, cx: &mut Context<Self>) {
        self.update_thread(
            id,
            |t| {
                t.snoozed_until = None;
                t.snoozed_until_restart = None;
                t.wake_when = None;
            },
            json!({ "snoozedUntil": Value::Null }),
            cx,
        );
    }

    /// Snooze until a moment, moving off it if it's open (`AppModel.snooze`).
    pub(crate) fn snooze_row(&mut self, id: String, until_ms: i64, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.neighbour_of(&id, cx, now_ms());
        let until = iso_time(until_ms);
        let local = until.clone();
        self.update_thread(
            &id,
            move |t| {
                t.snoozed_until = Some(local);
                t.wake_when = None;
            },
            json!({ "snoozedUntil": until }),
            cx,
        );
        self.leave_selection(&id, next, window, cx);
    }

    /// Asks whether a runner restart is waiting. The runner says so to every
    /// client while one is (`runner.notice`), and on connect there may be one.
    pub(crate) fn refresh_restart(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "runner.status", json!({})).await;
            this.update(cx, |this, cx| {
                let status = res.ok();
                this.sidebar.restart_pending = status.as_ref().is_some_and(|v| v.get("pendingRestart").is_some_and(|p| !p.is_null()));
                this.website_url = status.as_ref().and_then(crate::header::website_url);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Hibernate until the runner next starts (`snoozedUntil: "restart"`).
    fn snooze_until_restart(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.neighbour_of(&id, cx, now_ms());
        self.update_thread(
            &id,
            |t| {
                t.snoozed_until = Some("9999-12-31T00:00:00.000Z".into());
                t.snoozed_until_restart = Some(true);
            },
            json!({ "snoozedUntil": "restart" }),
            cx,
        );
        self.leave_selection(&id, next, window, cx);
    }

    /// Hibernate until when Mike said, in his words (`AppModel.hibernateWhen`):
    /// away at once, saying Thinking… until the runner works out when; back
    /// with the runner's reason if it can't.
    pub(crate) fn hibernate_when(&mut self, id: String, said: String, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.neighbour_of(&id, cx, now_ms());
        let before = self.threads.get(&id).cloned();
        if let Some(t) = self.threads.get_mut(&id) {
            t.snoozed_until = Some("9999-12-31T00:00:00.000Z".into());
            t.snoozed_until_restart = None;
            t.wake_when = Some(json!({ "said": said, "pending": true }));
        }
        self.leave_selection(&id, next, window, cx);
        let client = self.client.clone();
        let params = json!({ "threadId": id, "said": said });
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.hibernateWhen", params).await;
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
    }

    /// Delete from the sidebar, only for an archived chat, after asking
    /// (`AppModel.deleteThread`). Its trash button shows a spinner until the
    /// runner answers, so it can't be pressed twice.
    fn delete_row(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let busy_key = format!("delete:{id}");
        if self.sidebar.busy.contains(&busy_key) {
            return;
        }
        let Some(thread) = self.threads.get(&id) else { return };
        if !thread.is_archived() {
            return;
        }
        let title = format!("Delete \u{201C}{}\u{201D}?", thread.title);
        let entity = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (entity, id, busy_key) = (entity.clone(), id.clone(), busy_key.clone());
            alert
                .title(title.clone())
                .description("This removes the chat from Threadbear and moves it to the runner's trash. It can't be undone in the app.")
                .show_cancel(true)
                .button_props(DialogButtonProps::default().ok_text("Delete").show_cancel(true))
                .on_ok(move |_, _, cx| {
                    let (id, busy_key) = (id.clone(), busy_key.clone());
                    entity
                        .update(cx, move |this, cx| {
                            this.sidebar.busy.insert(busy_key.clone());
                            cx.notify();
                            let client = this.client.clone();
                            let params = json!({ "threadId": id });
                            cx.spawn(async move |this, cx| {
                                let res = call(client, "threads.delete", params).await;
                                this.update(cx, |this, cx| {
                                    this.sidebar.busy.remove(&busy_key);
                                    match res {
                                        Ok(_) => {
                                            this.threads.remove(&id);
                                        }
                                        Err(e) => this.list_error = Some(e.message),
                                    }
                                    cx.notify();
                                })
                                .ok();
                            })
                            .detach();
                        })
                        .ok();
                    true
                })
        });
    }

    /// A hover button's archive or hibernate on a thread with work going
    /// asks first (`BusyConfirm`); the menus and ⌘E are deliberate and don't.
    fn confirm_if_busy(&mut self, id: &str, archive: bool, window: &mut Window, cx: &mut Context<Self>, go: impl Fn(&mut Threadbear, &mut Window, &mut Context<Threadbear>) + 'static) {
        let Some(thread) = self.threads.get(id).cloned() else { return };
        if !rows::needs_busy_confirm(&thread) {
            go(self, window, cx);
            return;
        }
        let (title, detail) = rows::busy_prompt(&thread, archive);
        let verb = if archive { "Archive" } else { "Hibernate" };
        let entity = cx.entity().downgrade();
        let go = std::rc::Rc::new(go);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (entity, go) = (entity.clone(), go.clone());
            alert.title(title.clone()).description(detail.clone()).button_props(DialogButtonProps::default().ok_text(verb).show_cancel(true)).on_ok(move |_, window, cx| {
                let go = go.clone();
                entity.update(cx, move |this, cx| go(this, window, cx)).ok();
                true
            })
        });
    }

    /// Runs a menu line for a row (or the Inbox's sort).
    /// Runs a menu line for a row (or the Inbox's sort), after asking if the
    /// moon's menu is about to stop a running turn (`BusyConfirm`).
    pub(crate) fn run_menu_action(&mut self, thread: Option<String>, action: MenuAction, window: &mut Window, cx: &mut Context<Self>) {
        let confirm_busy = self.sidebar.menu.take().is_some_and(|m| m.confirm_busy);
        match action {
            MenuAction::Sort(order) => {
                self.sidebar.order = order;
                self.sidebar_settings_changed(cx);
                cx.notify();
                return;
            }
            MenuAction::Runner(runner) => return self.choose_new_thread_runner(runner, true, cx),
            MenuAction::DefaultRunner(runner) => return self.set_default_runner(runner, cx),
            MenuAction::DraftFolder(folder) => return self.change_new_draft(&json!({ "cwd": folder }), cx),
            _ => {}
        }
        let Some(id) = thread else { return };
        if confirm_busy && matches!(action, MenuAction::Hibernate(_) | MenuAction::HibernateUntilRestart | MenuAction::PickTime | MenuAction::WakeWhen) {
            let (inner_id, inner_action) = (id.clone(), action.clone());
            self.confirm_if_busy(&id, false, window, cx, move |this, window, cx| this.run_row_action(inner_id.clone(), inner_action.clone(), window, cx));
            return;
        }
        self.run_row_action(id, action, window, cx);
    }

    fn run_row_action(&mut self, id: String, action: MenuAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            MenuAction::Star(on) => self.star_row(&id, on, cx),
            MenuAction::Archive => self.archive_row(id, window, cx),
            MenuAction::Unarchive => self.unarchive_row(&id, cx),
            MenuAction::Wake => self.wake_row(&id, cx),
            MenuAction::Hibernate(preset) => {
                let until = rows::snooze_date(preset, &rows::local_now());
                self.snooze_row(id, until, window, cx);
            }
            MenuAction::HibernateUntilRestart => self.snooze_until_restart(id, window, cx),
            MenuAction::PickTime => self.open_pick_time(id, window, cx),
            MenuAction::WakeWhen => self.open_wake_when(id, window, cx),
            MenuAction::Delete => self.delete_row(id, window, cx),
            MenuAction::Rename => self.open_rename(id, window, cx),
            MenuAction::CopyId => cx.write_to_clipboard(ClipboardItem::new_string(id)),
            // BearDock isn't in this app yet, so a thread opens in the main pane.
            MenuAction::OpenInPanel => self.open(id, window, cx),
            MenuAction::CopyTranscript => {
                if let Some(pane) = &self.pane {
                    pane.update(cx, |p, cx| p.copy_transcript(cx));
                }
            }
            MenuAction::OpenOnWeb => {
                if let Some(link) = self.thread_web_link(&id) {
                    crate::open_gate::open_web(&link, cx);
                }
            }
            // A project's page isn't in this app yet: the Projects view is
            // where it will open.
            MenuAction::OpenProject => self.set_mode(crate::shell::SidebarMode::Projects, cx),
            MenuAction::ThreadProject { thread, add, remove } => self.set_thread_projects(thread, add, remove, cx),
            MenuAction::MoveTo(runner_id, runner_name) => self.open_move(crate::move_sheet::MoveRequest { thread_id: id, runner_id, runner_name }, window, cx),
            MenuAction::CancelMove => self.cancel_move(id, cx),
            MenuAction::Sort(_) | MenuAction::Runner(_) | MenuAction::DefaultRunner(_) | MenuAction::DraftFolder(_) => {}
        }
        cx.notify();
    }

    /// `ThreadMenu`: the row's right-click menu.
    pub(crate) fn thread_menu_rows(&self, t: &ThreadSummary) -> Vec<MenuRow> {
        let mut rows = vec![
            // Greyed out until the window has panels (BearDock).
            MenuRow::item("Open in New Panel", "rectangle.split.2x1", MenuAction::OpenInPanel).disabled(),
            MenuRow::separator(),
            MenuRow::item("Rename", "pencil", MenuAction::Rename),
            if t.is_starred() { MenuRow::item("Remove Star", "star.slash", MenuAction::Star(false)) } else { MenuRow::item("Star", "star", MenuAction::Star(true)) },
            MenuRow::submenu("Hibernate", "moon.zzz", self.snooze_menu_rows()),
        ];
        if t.snoozed_until.is_some() {
            rows.push(MenuRow::item("Wake", "sun.max", MenuAction::Wake));
        }
        if t.is_archived() {
            rows.push(MenuRow::item("Unarchive", "tray.and.arrow.up", MenuAction::Unarchive));
            rows.push(MenuRow::item("Delete…", "trash", MenuAction::Delete));
        } else {
            rows.push(MenuRow::item("Archive", "archivebox", MenuAction::Archive));
        }
        // Move to ▸ another runner (`ThreadMoveMenu`), behind the experiment;
        // while a move's going, Cancel Move instead.
        if self.can_move_threads() {
            let moving = t.moving.as_ref().filter(|m| !matches!(m["state"].as_str(), Some("cancelled") | Some("failed")));
            if moving.is_some() {
                rows.push(MenuRow::item("Cancel Move", "xmark.circle", MenuAction::CancelMove));
            } else {
                let targets = self.move_targets(t.runner_id.as_deref());
                if !targets.is_empty() {
                    let children = targets
                        .into_iter()
                        .map(|r| {
                            let title = if r.online { r.name.clone() } else { format!("{}, offline", r.name) };
                            let mut row = MenuRow::item(title, crate::icons::runner_symbol(r.icon.as_deref()), MenuAction::MoveTo(r.id, r.name));
                            row.enabled = r.online;
                            row
                        })
                        .collect();
                    rows.push(MenuRow::submenu("Move to", "arrow.right.square", children));
                }
            }
        }
        // "Copy thread id" comes after a divider, as `.iconContextMenu` adds it.
        rows.push(MenuRow::separator());
        rows.push(MenuRow::item("Copy thread id", "number", MenuAction::CopyId));
        rows
    }

    /// `SnoozeMenuItems`/`SnoozeNSMenu`: the presets, then Pick a time and Wake when.
    fn snooze_menu_rows(&self) -> Vec<MenuRow> {
        let now = rows::local_now();
        let mut items: Vec<MenuRow> = rows::snooze_presets(&now).into_iter().map(|p| MenuRow::item(p.title, p.symbol(), MenuAction::Hibernate(p.id))).collect();
        // Only while a restart is waiting, so a thread holding it up can go
        // away now and come back after (`app.restartPending`).
        if self.sidebar.restart_pending {
            items.push(MenuRow::item(rows::SNOOZE_UNTIL_RESTART, "arrow.clockwise", MenuAction::HibernateUntilRestart));
        }
        items.push(MenuRow::separator());
        items.push(MenuRow::item(rows::SNOOZE_PICK_TITLE, "calendar", MenuAction::PickTime));
        items.push(MenuRow::item(rows::WAKE_MENU_TITLE, "text.bubble", MenuAction::WakeWhen));
        items
    }

    fn sort_menu_rows(&self) -> Vec<MenuRow> {
        InboxOrder::ALL
            .into_iter()
            .map(|order| {
                let mut row = MenuRow::item(order.title(), order.symbol(), MenuAction::Sort(order));
                row.checked = order == self.sidebar.order;
                row
            })
            .collect()
    }

    /// Opens `rows` as a popup menu at `position`, for a row or a heading.
    pub(crate) fn open_menu(&mut self, thread: Option<String>, menu_rows: Vec<MenuRow>, position: Point<Pixels>, confirm_busy: bool, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity().downgrade();
        let (build_rows, build_thread) = (menu_rows.clone(), thread.clone());
        let view = PopupMenu::build(window, cx, move |menu, window, cx| fill_menu(menu, &build_rows, entity.clone(), build_thread.clone(), window, cx));
        // The menu closes itself on Escape, a click outside or a choice.
        let dismissed = cx.subscribe(&view, |this, _, _: &DismissEvent, cx| {
            this.sidebar.menu = None;
            cx.notify();
        });
        self.sidebar.menu = Some(OpenMenu { thread, position, view, rows: menu_rows, confirm_busy, _dismissed: dismissed });
        cx.notify();
    }

    pub(crate) fn open_row_menu(&mut self, id: String, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.threads.get(&id).cloned() else { return };
        let menu_rows = self.thread_menu_rows(&t);
        self.open_menu(Some(id), menu_rows, position, false, window, cx);
    }

    pub(crate) fn open_hibernate_menu(&mut self, id: String, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let menu_rows = self.snooze_menu_rows();
        self.open_menu(Some(id), menu_rows, position, true, window, cx);
    }

    /// "Wake when…": a text box for when, in Mike's own words (`WakeWhenPrompt`).
    pub(crate) fn open_wake_when(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity().downgrade();
        let field = cx.new(|cx| InputState::new(window, cx).placeholder(rows::WAKE_PLACEHOLDER));
        let field_focus = field.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (entity, field_read, id) = (entity.clone(), field.clone(), id.clone());
            dialog
                .title("Hibernate until")
                .width(px(460.))
                .child(Input::new(&field))
                .button_props(DialogButtonProps::default().ok_text("Hibernate").show_cancel(true))
                .on_ok(move |_, window, cx| {
                    let said = field_read.read(cx).value().trim().to_string();
                    let id = id.clone();
                    if !said.is_empty() {
                        entity.update(cx, |this, cx| this.hibernate_when(id, said, window, cx)).ok();
                    }
                    true
                })
        });
        let handle = field_focus.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// "Pick a time…": a calendar and a time, and a word if it's already
    /// gone (`SnoozePicker`).
    pub(crate) fn open_pick_time(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::component::date_picker::{DatePicker, DatePickerState};
        use gpui_kit::component::time_field::TimePrecision;
        use threadbear_core::chrono::{Duration as ChronoDuration, Local, NaiveDateTime, TimeZone as _};
        let entity = cx.entity().downgrade();
        let start: NaiveDateTime = (Local::now() + ChronoDuration::hours(3)).naive_local();
        let picker = cx.new(|cx| {
            let mut state = DatePickerState::new(window, cx).time_precision(TimePrecision::Minute);
            state.set_date_time(start, window, cx);
            state
        });
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (entity, picker_read, id) = (entity.clone(), picker.clone(), id.clone());
            dialog
                .title("Hibernate until")
                .width(px(360.))
                .child(DatePicker::new(&picker))
                .button_props(DialogButtonProps::default().ok_text("Hibernate").show_cancel(true))
                .on_ok(move |_, window, cx| {
                    let when = picker_read.read(cx).date_time().start();
                    let Some(ms) = when.and_then(|w| Local.from_local_datetime(&w).earliest()).map(|d| d.timestamp_millis()) else { return true };
                    if ms <= now_ms() {
                        // An earlier time today would wake it straight back up.
                        window.push_notification("That time has already gone. Pick a later one.", cx);
                        return false;
                    }
                    let id = id.clone();
                    entity.update(cx, |this, cx| this.snooze_row(id, ms, window, cx)).ok();
                    true
                })
        });
    }

    /// Rename, from the row's menu (the Swift app makes the header a field;
    /// this app's header isn't editable yet).
    fn open_rename(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.threads.get(&id).map(|t| t.title.clone()) else { return };
        let entity = cx.entity().downgrade();
        let field = cx.new(|cx| InputState::new(window, cx).placeholder("Name this thread").default_value(current));
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (entity, field_read, id) = (entity.clone(), field.clone(), id.clone());
            dialog.title("Rename thread").width(px(400.)).child(Input::new(&field)).button_props(DialogButtonProps::default().ok_text("Rename").show_cancel(true)).on_ok(move |_, _, cx| {
                let title = field_read.read(cx).value().trim().to_string();
                if !title.is_empty() {
                    let id = id.clone();
                    entity
                        .update(cx, |this, cx| {
                            let local = title.clone();
                            this.update_thread(&id, move |t| t.title = local, json!({ "title": title }), cx)
                        })
                        .ok();
                }
                true
            })
        });
    }

    /// The projects' icons, for the tiles: on connect and whenever a
    /// project changes (`projects.updated`).
    pub(crate) fn load_projects(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(v) = call(client, "projects.list", json!({ "archived": true })).await {
                this.update(cx, |this, cx| {
                    this.sidebar.project_icons = project_icons(&v["projects"]);
                    this.projects_supported = true;
                    this.all_projects = crate::app::project_list(&v["projects"]);
                    this.sidebar.projects = project_briefs(&v["projects"]);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// The whole list: search and new-thread line, then the sections.
    pub(crate) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let now = now_ms();
        let clock: Clock = rows::local_now();
        let query = self.search.read(cx).value().trim().to_lowercase();
        let searching = !query.is_empty();
        let sections = self.shown_sections(cx, now);
        let mut list = v_flex().px(px(OUTER_INSET)).pb(px(32.));
        let loaded = self.sidebar.loaded;
        if !loaded {
            list = list.child(render_loading_rows(&p));
        } else if searching && sections.iter().all(|s| s.rows.is_empty()) {
            list = list.child(div().pl(px(LEADING)).pt(px(10.)).text_size(px(style::size::SIDEBAR_PREVIEW)).text_color(p.secondary).child("No threads match."));
        } else {
            for section in &sections {
                // Empty sections other than Inbox are left out, and Inbox too
                // while searching (`ThreadSection`).
                if section.rows.is_empty() && (section.key != "inbox" || searching) {
                    continue;
                }
                let folded = !searching && self.folded.contains(section.key);
                let total = section.rows.len();
                // The Archived section shows the most recent few (`ArchiveRows`).
                let shown = if section.key == "archived" {
                    let selected = self.selected.as_ref().and_then(|id| section.rows.iter().position(|t| &t.id == id));
                    rows::archive_rows(total, self.sidebar.archive_limit, searching, selected)
                } else {
                    (total, 0)
                };
                list = list.child(self.render_heading(section.key, section.title, total, folded, searching, &p, cx));
                if folded {
                    continue;
                }
                for t in section.rows.iter().take(shown.0) {
                    list = list.child(self.render_row(t, section.key, now, &clock, &p, cx));
                    if !matches!(section.key, "hibernating" | "archived") {
                        for sub in self.render_subagents(t, section.key, now, &p, cx) {
                            list = list.child(sub);
                        }
                    }
                }
                if section.key == "archived" && shown.1 > 0 {
                    list = list.child(self.render_show_more(shown.1, &p, cx));
                }
                // Under the empty Inbox heading.
                if section.key == "inbox" && section.rows.is_empty() && !searching {
                    list = list.child(div().pl(px(LEADING)).pt(px(14.)).text_size(px(style::size::SIDEBAR_PREVIEW)).text_color(p.secondary).child("Nothing in the inbox."));
                }
            }
        }
        let new_thread_active = self.selected.is_none();
        let conn_note = self.connection_note();
        let list_error = self.list_error.clone();
        let red = p.failed;
        v_flex()
            .relative()
            .size_full()
            .bg(p.side)
            // One line: search and New thread (Mike asked 30 Oct).
            .child(h_flex().gap(px(6.)).px(px(OUTER_INSET)).pt(px(4.)).items_stretch().child(self.render_search(&p, cx)).child(self.render_new_thread(new_thread_active, &p, cx)))
            .when_some(conn_note, |s, note| s.child(div().mx(px(OUTER_INSET)).mt_2().p_2().rounded(px(CORNER)).bg(p.field_fill).text_sm().text_color(p.secondary).child(note)))
            .when_some(list_error, |s, e| s.child(div().mx(px(OUTER_INSET)).mt_2().text_sm().text_color(red).child(e)))
            .child(
                // The scrolling list, with no scrollbar (the times stay put),
                // fading out at the foot rather than cutting through a row.
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(div().id("sidebar-list").size_full().overflow_y_scroll().track_scroll(&self.sidebar.scroll).child(list))
                    .child(div().absolute().left_0().right_0().bottom_0().h(px(34.)).bg(linear_gradient(180., linear_color_stop(p.side.opacity(0.), 0.), linear_color_stop(p.side, 0.88)))),
            )
    }

    /// The open row or heading menu, drawn over the whole window rather than
    /// inside the sidebar panel (the dock clips what a panel draws).
    pub(crate) fn render_open_menu(&self) -> Option<AnyElement> {
        self.sidebar.menu.as_ref().map(|m| deferred(anchored().position(m.position).snap_to_window_with_margin(px(8.)).child(div().occlude().child(m.view.clone()))).with_priority(1).into_any_element())
    }

    fn render_search(&mut self, p: &Palette, _cx: &mut Context<Self>) -> impl IntoElement {
        // A field, not a row: a lighter fill and a hairline (`SearchField`).
        // The glass and the prompt sit on the same columns as a button row's
        // icon and title, so the Input's own padding is dropped.
        h_flex()
            .id("search-field")
            .tooltip(|window, cx| Tooltip::new("Search threads").build(window, cx))
            .flex_1()
            .min_w_0()
            .pl(px(LEADING - 2.))
            .pr(px(9.))
            .py(px(3.))
            .gap(px(8.))
            .items_center()
            .rounded(px(CORNER))
            .bg(p.field_fill)
            .border_1()
            .border_color(p.separator)
            .child(div().flex_none().w(px(16.)).flex().justify_center().child(icon("magnifyingglass", 12.5, p.faint)))
            .child(div().flex_1().min_w_0().child(Input::new(&self.search).appearance(false).small().cleanable(true).pl(px(0.))))
    }

    /// `NewThreadButton`: the compose icon, lit while the new thread's
    /// composer is showing.
    fn render_new_thread(&mut self, active: bool, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (hover, selected, fg, secondary) = (p.hover, p.selected, p.fg, p.secondary);
        let color = if active { fg } else { secondary };
        tipped!(div().w(px(32.)).flex_none().flex().items_center().justify_center().rounded(px(CORNER)).when(active, |s| s.bg(selected)).when(!active, |s| s.hover(move |h| h.bg(hover).text_color(fg))), "new-thread", "Start a new thread")
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.new_thread(cx)))
            .child(icon("square.and.pencil", style::size::SIDEBAR_BUTTON, color))
    }

    /// `app.newThread()`: leaves the open thread for a fresh start, the
    /// cursor in the new box.
    pub(crate) fn new_thread(&mut self, cx: &mut Context<Self>) {
        if let Some(old) = self.pane.take() {
            old.read(cx).unsubscribe();
        }
        self.selected = None;
        self.note_navigation();
        self.new_thread.picks.project = None;
        self.new_thread.focus_pending = true;
        cx.notify();
    }

    /// `SectionHeading`: a quiet divider with room above each group, folding
    /// its rows away when clicked. Folded, it says how many.
    fn render_heading(&mut self, key: &'static str, title: &'static str, count: usize, folded: bool, searching: bool, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let symbol = match key {
            "starred" => "star",
            "inbox" => "tray",
            "running" => "bolt",
            "hibernating" => "moon.zzz",
            _ => "archivebox",
        };
        let group = SharedString::from(format!("heading-{key}"));
        let (fg, secondary, faint, hairline) = (p.fg, p.secondary, p.faint, p.hairline);
        let chevron_group = group.clone();
        let hover_group = group.clone();
        let label = h_flex()
            .group(group.clone())
            .flex_1()
            .min_w_0()
            .gap(px(6.))
            .pl(px(LEADING))
            .pt(px(18.))
            .pb(px(6.))
            .items_center()
            .child(icon(symbol, 11., secondary).flex_none())
            .child(div().flex_none().text_size(px(style::size::SIDEBAR_HEADING)).line_height(px(16.)).font_semibold().text_color(secondary).group_hover(hover_group, move |s| s.text_color(fg)).child(title))
            .child(div().flex_1().min_w_0().h(px(1.)).mx(px(4.)).bg(hairline))
            .when(folded && count > 0, |s| s.child(div().flex_none().text_size(px(style::size::SIDEBAR_META)).text_color(faint).child(count.to_string())))
            .when(!searching, |s| {
                s.child(div().flex_none().when(!folded, |d| d.invisible()).group_hover(chevron_group, |d| d.visible()).child(icon(if folded { "chevron.right" } else { "chevron.down" }, 9., faint)))
            });
        h_flex()
            .pr(px(TRAILING))
            .items_center()
            .child(if searching {
                label.into_any_element()
            } else {
                // The padding is inside the fold button, so the whole band
                // above the rows folds.
                let tip = if folded { format!("Show {title}") } else { format!("Hide {title}") };
                tipped!(div().flex_1().min_w_0(), SharedString::from(format!("fold-{key}")), tip)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if !this.folded.remove(key) {
                            this.folded.insert(key);
                        }
                        this.sidebar_settings_changed(cx);
                        cx.notify();
                    }))
                    .child(label)
                    .into_any_element()
            })
            .when(key == "inbox", |s| s.child(self.render_sort_button(p, cx)))
    }

    /// `InboxOrderMenu`: the sort button at the end of the Inbox heading.
    fn render_sort_button(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let order = self.sidebar.order;
        let color = if order == InboxOrder::NeedsYou { p.faint } else { p.secondary };
        let (fg, hover) = (p.fg, p.icon_hover);
        tipped!(div().flex_none().mt(px(12.)).size(px(16.)).rounded(px(4.)).flex().items_center().justify_center().text_color(color).hover(move |s| s.bg(hover).text_color(fg)), "sort-inbox", format!("Sort: {}", order.title()))
            .on_click(cx.listener(|this, ev: &ClickEvent, window, cx| {
                let menu_rows = this.sort_menu_rows();
                this.open_menu(None, menu_rows, below(ev.position()), false, window, cx);
            }))
            .child(icon("arrow.up.arrow.down", 10., color))
    }

    fn render_show_more(&mut self, more: usize, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let step = more.min(rows::ARCHIVE_STEP);
        let tip = format!("{more} older archived {} not shown", if more == 1 { "thread" } else { "threads" });
        let (hover, secondary, fg) = (p.hover, p.secondary, p.fg);
        tipped!(
            h_flex().gap(px(8.)).pl(px(LEADING - 2.)).pr(px(TRAILING)).py(px(7.)).rounded(px(CORNER)).hover(move |s| s.bg(hover)),
            "archive-show-more",
            tip
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.sidebar.archive_limit += rows::ARCHIVE_STEP;
            cx.notify();
        }))
        .child(div().w(px(16.)).child(icon("ellipsis", style::size::SIDEBAR_BUTTON, secondary)))
        .child(div().text_size(px(style::size::SIDEBAR_BUTTON)).text_color(fg).child(format!("Show {step} more")))
    }

    /// `ThreadRowView`: tile, title, place line, status or hover buttons.
    fn render_row(&mut self, t: &ThreadSummary, section: &'static str, now: i64, clock: &Clock, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let mode = RowMode::of(section, t, now);
        // The Hibernating and Archived rows read plainly; only live ones say
        // what they're doing (`ThreadRowView.body`).
        let look = if mode == RowMode::Inbox { RowLook::of(t, Some(section)) } else { RowLook::PLAIN };
        let trailing = rows::trailing(t, &look, mode, clock);
        let id = t.id.clone();
        let selected = self.selected.as_deref() == Some(id.as_str());
        // A script's hover wins over the real pointer, which on a shared
        // desktop can be resting on another row (`ThreadRowView.pointerOver`).
        let hovered_row = self.sidebar.hover_override.as_deref().or(self.sidebar.hovered.as_deref());
        let show_buttons = hovered_row == Some(id.as_str()) || self.sidebar.menu.as_ref().is_some_and(|m| m.thread.as_deref() == Some(id.as_str()));
        let title = if t.title.is_empty() { "Untitled".to_string() } else { t.title.clone() };
        let tip = format!("{title}\n{}", abbreviate_home(&t.cwd));
        let (bg_hover, bg_sel, fg, secondary) = (p.hover, p.selected, p.fg, p.secondary);

        let hover_id = id.clone();
        let menu_id = id.clone();
        let select_id = id.clone();

        // The tile with the unread dot or tick in the gutter to its left.
        let behind = if selected { p.tile_ring.blend(p.selected) } else if show_buttons { p.tile_ring.blend(p.hover) } else { p.tile_ring };
        let project_icon = t.projects.as_ref().and_then(|ps| ps.first()).and_then(|first| self.sidebar.project_icons.get(&first.id));
        // Another runner's thread with no project shows its machine's icon
        // where the folder would be (`ProjectTiles.runnerIcon`) and its name
        // in the tile's tooltip, faded when that machine's offline
        // (`ThreadRowView.body`).
        let runner_tile = t.runner_id.as_deref().filter(|_| t.projects.as_ref().is_none_or(|ps| ps.is_empty())).map(|id| (self.runner_icon_symbol(Some(id)), self.runner_place_of(id)));
        let tile = render_tile(t, &look, behind, project_icon, runner_tile, p);
        let offline = t.runner_id.as_deref().is_some_and(|id| self.remote.runners.iter().any(|r| r.id == id && !r.online));

        let byline = rows::byline(t);
        let place = t.place();
        let runner_label = self.runner_label(t);
        let place_tip_text = if byline.is_some() { byline.as_ref().unwrap().tip.clone() } else { rows::place_tip(t) };
        let place_id: ElementId = ElementId::Name(format!("place-{id}").into());

        let title_el = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(style::size::SIDEBAR_TITLE))
            .line_height(px(17.))
            .text_color(fg)
            .when(look.emphasised, |s| s.font_semibold())
            .child(title);

        let second_line = h_flex()
            .items_baseline()
            .gap(px(6.))
            .child(
                h_flex()
                    .id(place_id)
                    .min_w_0()
                    .items_baseline()
                    .gap(px(4.))
                    .text_color(secondary)
                    .tooltip(move |window, cx| Tooltip::new(place_tip_text.clone()).build(window, cx))
                    .when_some(byline.as_ref(), |s, b| s.child(icons::try_symbol(&b.symbol).unwrap_or_else(|| icons::symbol("wrench.and.screwdriver")).with_size(px(style::size::SIDEBAR_META)).text_color(secondary).flex_none()))
                    .child(div().min_w_0().truncate().text_size(px(style::size::SIDEBAR_META + 1.)).line_height(px(15.)).child(place)),
            )
            // Its own text after the place, kept whole while a long project
            // name is cut short, and capped, as a runner's name can be any
            // length (`CappedWidth(limit: 100)` in ThreadRowView).
            .when_some(runner_label, |s, runner| {
                let tip = format!("On {runner}");
                s.child(
                    div()
                        .id(ElementId::Name(format!("runner-{id}").into()))
                        .flex_none()
                        .max_w(px(100.))
                        .truncate()
                        .ml(px(-2.))
                        .text_size(px(style::size::SIDEBAR_META + 1.))
                        .line_height(px(15.))
                        .text_color(secondary)
                        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                        .child(format!("\u{2022} {runner}")),
                )
            })
            .child(div().flex_1().min_w(px(6.)))
            .when(!show_buttons, |s| s.child(render_trailing(&trailing, id.clone(), p)));

        let actions = mode.actions();
        let buttons = if show_buttons {
            Some(self.render_row_actions(t, actions, cx))
        } else {
            None
        };

        h_flex()
            .id(ElementId::Name(format!("row-{id}").into()))
            .test_support()
            .aria_label(tip.clone())
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .when(offline, |s| s.opacity(0.5))
            .relative()
            .items_center()
            .gap(px(10.))
            .pt(px(8.))
            .pb(px(8.))
            .pl(px(LEADING))
            .pr(px(TRAILING))
            .rounded(px(CORNER))
            .when(selected, |s| s.bg(bg_sel))
            .when(!selected && show_buttons, |s| s.bg(bg_hover))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.sidebar.hovered = Some(hover_id.clone());
                } else if this.sidebar.hovered.as_deref() == Some(hover_id.as_str()) {
                    this.sidebar.hovered = None;
                }
                cx.notify();
            }))
            // Selects on mouse down, as Mail does (`ThreadRowView`); a click
            // waits for the mouse to come back up.
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _: &MouseDownEvent, window, cx| this.open(select_id.clone(), window, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| this.open_row_menu(menu_id.clone(), ev.position, window, cx)))
            .child(tile)
            .child(v_flex().flex_1().min_w_0().gap(px(3.)).child(h_flex().child(title_el).when(show_buttons, |s| s.child(div().flex_none().w(px(actions.len() as f32 * 24.)).h(px(1.))))).child(second_line))
            .when_some(buttons, |s, b| s.child(h_flex().absolute().right(px(TRAILING - 5.)).top_0().bottom_0().items_center().gap(px(2.)).child(b)))
            .into_any_element()
    }

    /// `RowActions`: the small buttons a row shows on hover in place of its
    /// time. Star, the moon's menu, archive; or unarchive and delete.
    fn render_row_actions(&mut self, t: &ThreadSummary, actions: &[RowAction], cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let id = t.id.clone();
        let mut row = h_flex().gap(px(2.));
        for action in actions {
            let tip = action.tip(t);
            let symbol = action.symbol(t);
            let key = ElementId::Name(format!("{tip}:{id}").into());
            let button: AnyElement = match action {
                RowAction::Star => {
                    let (on, id) = (!t.is_starred(), id.clone());
                    let listener = cx.listener(move |this, _: &ClickEvent, _, cx| this.star_row(&id, on, cx));
                    icon_button(key, icon(symbol, 13., p.secondary), tip, &p, listener).into_any_element()
                }
                RowAction::Snooze => {
                    let id = id.clone();
                    let listener = cx.listener(move |this, ev: &ClickEvent, window, cx| this.open_hibernate_menu(id.clone(), below(ev.position()), window, cx));
                    icon_button(key, icon(symbol, 13., p.secondary), tip, &p, listener).into_any_element()
                }
                RowAction::Archive => {
                    let id = id.clone();
                    let listener = cx.listener(move |this, _: &ClickEvent, window, cx| {
                        let confirmed_id = id.clone();
                        this.confirm_if_busy(&id, true, window, cx, move |this, window, cx| this.archive_row(confirmed_id.clone(), window, cx));
                    });
                    icon_button(key, icon(symbol, 13., p.secondary), tip, &p, listener).into_any_element()
                }
                RowAction::Unsnooze => {
                    let id = id.clone();
                    icon_button(key, icon(symbol, 13., p.secondary), tip, &p, cx.listener(move |this, _: &ClickEvent, _, cx| this.wake_row(&id, cx))).into_any_element()
                }
                RowAction::Unarchive => {
                    let id = id.clone();
                    icon_button(key, icon(symbol, 13., p.secondary), tip, &p, cx.listener(move |this, _: &ClickEvent, _, cx| this.unarchive_row(&id, cx))).into_any_element()
                }
                RowAction::Delete => {
                    let busy_key = format!("delete:{id}");
                    if self.sidebar.busy.contains(&busy_key) {
                        // On the way: a spinner, and no more clicks.
                        div().flex_none().size(px(22.)).flex().items_center().justify_center().child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(p.secondary).with_size(px(13.))).into_any_element()
                    } else {
                        let id = id.clone();
                        icon_button(key, icon(symbol, 13., p.secondary), tip, &p, cx.listener(move |this, _: &ClickEvent, window, cx| this.delete_row(id.clone(), window, cx))).into_any_element()
                    }
                }
            };
            row = row.child(button);
        }
        row.into_any_element()
    }

    /// `SubagentList`: indented rows under the thread with a drawn trunk.
    fn render_subagents(&mut self, t: &ThreadSummary, section: &'static str, now: i64, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let all = rows::subagents(t);
        if all.is_empty() {
            return vec![];
        }
        let selected = self.selected.as_deref() == Some(t.id.as_str());
        // Under Inbox and Starred only a running one shows.
        let finished_shown = section != "inbox" && section != "starred";
        let shown = rows::subagent_rows(&all, selected, None, now, finished_shown);
        let last = shown.len().saturating_sub(1);
        shown
            .iter()
            .enumerate()
            .map(|(i, row)| render_subagent_row(t, row, i == 0, i == last, now, p, cx).into_any_element())
            .collect()
    }

    /// The sidebar as `dump` reports it, in the Swift app's shape.
    #[cfg(feature = "automation")]
    pub(crate) fn sidebar_dump(&self, cx: &App) -> Value {
        let now = now_ms();
        let clock: Clock = rows::local_now();
        let selected_sub = self.selected.clone();
        let row = |t: &ThreadSummary, section: Option<&str>| -> Value {
            let key = section.unwrap_or("inbox");
            let mode = RowMode::of(key, t, now);
            let look = RowLook::of(t, section);
            let plain = if mode == RowMode::Inbox { look } else { RowLook::PLAIN };
            let tail = rows::trailing(t, &plain, mode, &clock);
            let hovered = self.sidebar.hover_override.as_deref() == Some(t.id.as_str());
            let tile = match rows::tile(t).0 {
                Tile::Letter(l) => l,
                Tile::Folder if t.runner_id.is_some() => "runner".to_string(),
                Tile::Folder => "folder".to_string(),
            };
            let mark = if plain.done { Some("tick") } else if plain.unread { Some("dot") } else { None };
            let finished_shown = key != "inbox" && key != "starred";
            let subagents: Vec<Value> = if matches!(key, "hibernating" | "archived") {
                vec![]
            } else {
                let is_selected = selected_sub.as_deref() == Some(t.id.as_str());
                rows::subagent_rows(&rows::subagents(t), is_selected, None, now, finished_shown)
                    .iter()
                    .map(|s| json!({ "id": s.subagent.id, "depth": s.depth, "status": s.subagent.status, "title": rows::subagent_title(&s.subagent), "detail": rows::subagent_detail(&s.subagent), "selected": false }))
                    .collect()
            };
            json!({
                "id": t.id, "title": t.title, "status": t.status, "needsYou": t.needs_you, "unread": look.unread, "hovered": hovered,
                "background": (look.status == rows::RowStatus::Background).then(|| rows::background_short(&t.background_tasks())),
                "waiting": t.waiting_for_room_until_ms().map(|u| rows::waiting_line(u, &clock)).or_else(|| t.waiting_for_network.as_ref().map(|_| "Waiting for connection".to_string())),
                "subagents": subagents,
                "origin": rows::byline(t).map(|b| b.symbol),
                "place": t.place(),
                "bold": plain.emphasised,
                "mark": mark,
                "trailing": tail.text,
                "tile": tile,
                "hoverButtons": hovered.then(|| mode.actions().iter().map(|a| a.tip(t)).collect::<Vec<_>>()),
                "runner": t.runner_id.as_deref().map(|id| self.runner_place_of(id)),
            })
        };
        let shown = self.shown_sections(cx, now);
        let searching = !self.search.read(cx).value().trim().is_empty();
        let sections: Vec<Value> = shown
            .iter()
            .map(|s| {
                let folded = self.folded.contains(s.key);
                let count = (!searching && folded && !s.rows.is_empty()).then_some(s.rows.len());
                let drawn = (s.key == "archived").then(|| {
                    let selected = self.selected.as_ref().and_then(|id| s.rows.iter().position(|t| &t.id == id));
                    rows::archive_rows(s.rows.len(), self.sidebar.archive_limit, searching, selected).0
                });
                json!({ "name": s.title, "collapsed": folded, "count": count, "shown": drawn, "threads": s.rows.iter().map(|t| row(t, Some(s.key))).collect::<Vec<_>>() })
            })
            .collect();
        let snoozed = shown.iter().find(|s| s.key == "hibernating").map_or(0, |s| s.rows.len());
        let archived = shown.iter().find(|s| s.key == "archived").map_or(0, |s| s.rows.len());
        json!({ "mode": "inbox", "sections": sections, "snoozed": snoozed, "archived": archived, "sort": self.sidebar.order.name(), "loading": !self.sidebar.loaded })
    }

    /// `row` as `dump` shows the open thread (`selection`).
    #[cfg(feature = "automation")]
    pub(crate) fn selection_dump(&self) -> Value {
        let Some(t) = self.selected.as_ref().and_then(|id| self.threads.get(id)) else { return Value::Null };
        json!({ "id": t.id, "title": t.title, "status": t.status, "needsYou": t.needs_you, "unread": RowLook::of(t, None).unread, "runner": t.runner_id.as_deref().map(|id| self.runner_place_of(id)) })
    }
}

/// `abbreviatingHome`: "/Users/mike/dev/x" becomes "~/dev/x", for the row's tooltip.
fn abbreviate_home(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path == home => "~".into(),
        Ok(home) if !home.is_empty() && path.starts_with(&format!("{home}/")) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

/// `ProjectTiles`: the row's tile, with the dot or tick in the gutter.
/// `behind` is the colour drawn just under the tile (the row's highlight over
/// the sidebar), which the ring round the front tile takes so a stacked pair
/// reads as Swift's cut-out does. `runner_tile`: another runner's thread with
/// no project draws its machine's icon (a little larger, as a Mac mini is a
/// flat box) in place of the folder, and names it in the tooltip.
fn render_tile(t: &ThreadSummary, look: &RowLook, behind: Hsla, project_icon: Option<&ProjectIcon>, runner_tile: Option<(&'static str, String)>, p: &Palette) -> impl IntoElement {
    let (tile, stacked) = rows::tile(t);
    let help = match &runner_tile {
        Some((_, place)) => format!("{place}, in {}", abbreviate_home(&t.cwd)),
        None => rows::tile_help(t),
    };
    let size = px(TILE);
    let tinted = |content: AnyElement| div().flex().items_center().justify_center().size(size).rounded(px(7.)).bg(p.tile).child(content).into_any_element();
    let face = match (&tile, project_icon) {
        (Tile::Folder, _) => {
            let glyph = match &runner_tile {
                Some((symbol, _)) => icon(symbol, 16., p.secondary),
                None => icon("folder", 12., p.faint),
            };
            div().flex().items_center().justify_center().size(size).rounded(px(7.)).bg(p.tile_empty).child(glyph).into_any_element()
        }
        // The project's picture fills the tile; the letter stands in if the
        // file can't be read (a project on another runner's disk).
        (Tile::Letter(letter), Some(ProjectIcon { image_path: Some(path), .. })) => {
            let (letter, secondary, fill) = (letter.clone(), p.secondary, p.tile);
            img(std::path::PathBuf::from(path))
                .size(size)
                .rounded(px(7.))
                .object_fit(ObjectFit::Contain)
                .with_fallback(move || div().flex().items_center().justify_center().size(px(TILE)).rounded(px(7.)).bg(fill).text_size(px(14.)).font_semibold().text_color(secondary).child(letter.clone()).into_any_element())
                .into_any_element()
        }
        // Its symbol, when the bundled set has one for it.
        (Tile::Letter(_), Some(ProjectIcon { symbol: Some(symbol), .. })) if icons::try_symbol(symbol).is_some() => tinted(icons::symbol(symbol).with_size(px(14.)).text_color(p.secondary).into_any_element()),
        (Tile::Letter(letter), _) => tinted(div().text_size(px(14.)).font_semibold().text_color(p.secondary).child(letter.clone()).into_any_element()),
    };
    let mark: Option<AnyElement> = if look.done {
        // Finished with nothing needed: a tick where the dot would be.
        Some(div().absolute().left(px(-(LEADING + 10.) / 2. - 1.)).top_0().bottom_0().flex().items_center().child(icon("checkmark", 10., p.accent)).into_any_element())
    } else if look.unread {
        Some(
            div()
                .absolute()
                .left(px(-(LEADING + 7.) / 2. - 1.))
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .child(div().size(px(7.)).rounded_full().bg(p.accent))
                .into_any_element(),
        )
    } else {
        None
    };
    div()
        .relative()
        .flex_none()
        .size(px(TILE + 4.))
        .id(ElementId::Name(format!("tile-{}", t.id).into()))
        .tooltip(move |window, cx| Tooltip::new(help.clone()).build(window, cx))
        .when(stacked, |s| {
            // A second project's edge showing behind, up and to the right.
            s.child(div().absolute().left(px(4.)).top_0().size(size).rounded(px(7.)).bg(p.tile)).child(div().absolute().left_0().bottom_0().size(px(TILE + 2.)).rounded(px(8.)).bg(behind))
        })
        .child(div().absolute().left_0().bottom_0().child(face))
        .when_some(mark, |s, m| s.child(m))
}

/// What the right of the second line says: a word or the time, with its look.
fn render_trailing(trailing: &rows::Trailing, id: String, p: &Palette) -> AnyElement {
    let text = trailing.text.clone();
    let meta = px(style::size::SIDEBAR_META);
    let tip = trailing.tip.clone();
    // A long status ("Waiting for room · 2:51 pm") gives way before the
    // folder and the machine do, as SwiftUI lays out ThreadRowView's line:
    // the shorter text keeps its width and the longer one is cut short.
    // Shrinking twenty times as readily does the same in a flex row.
    let gives_way = |d: Div| {
        let mut d = d.min_w_0().truncate();
        d.style().flex_shrink = Some(20.);
        d
    };
    let el = match trailing.kind {
        TrailingKind::Quiet => gives_way(div()).text_size(meta).line_height(px(15.)).text_color(p.faint).child(text).into_any_element(),
        TrailingKind::Working => h_flex()
            .flex_none()
            .items_baseline()
            .gap(px(5.))
            .text_size(meta).line_height(px(15.))
            .font_medium()
            .text_color(p.accent)
            .child(div().flex_none().size(px(12.)).child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(p.accent).with_size(px(12.))))
            .child(text)
            .into_any_element(),
        TrailingKind::Background => h_flex()
            .flex_none()
            .items_baseline()
            .gap(px(5.))
            .text_size(meta).line_height(px(15.))
            .font_medium()
            .text_color(p.secondary)
            .child(div().flex_none().size(px(12.)).child(Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(p.secondary).with_size(px(12.))))
            .child(text)
            .into_any_element(),
        TrailingKind::Failed => gives_way(div()).text_size(meta).line_height(px(15.)).font_medium().text_color(p.failed).child(text).into_any_element(),
        TrailingKind::Stopped => gives_way(div()).text_size(meta).line_height(px(15.)).text_color(p.secondary).child(text).into_any_element(),
        TrailingKind::Woke => gives_way(div()).text_size(meta).line_height(px(15.)).font_medium().text_color(p.accent).child(text).into_any_element(),
    };
    match tip {
        Some(tip) => gives_way(div()).id(ElementId::Name(format!("trailing-{id}").into())).tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx)).child(el).into_any_element(),
        None => el,
    }
}

/// `LoadingRows`: grey rows the size of thread rows, gently pulsing, until
/// the runner's first list has come.
fn render_loading_rows(p: &Palette) -> impl IntoElement {
    const WIDTHS: [(f32, f32); 6] = [(170., 90.), (130., 70.), (190., 110.), (150., 80.), (120., 95.), (175., 75.)];
    let color = p.skeleton;
    v_flex()
        .pt(px(10.))
        .with_animation("sidebar-skeleton", Animation::new(Duration::from_millis(1800)).repeat().with_easing(bounce(ease_in_out)), |el, delta| el.opacity(1. - delta * 0.5))
        .children(WIDTHS.into_iter().map(move |(title, place)| {
            h_flex()
                .gap(px(10.))
                .pt(px(8.))
                .pb(px(8.))
                .pl(px(LEADING))
                .pr(px(TRAILING))
                .child(div().flex_none().size(px(TILE)).rounded(px(7.)).bg(color))
                .child(v_flex().gap(px(7.)).child(div().w(px(title)).h(px(9.)).rounded_full().bg(color)).child(div().w(px(place)).h(px(7.)).rounded_full().bg(color)))
        }))
}

/// `SubagentRowView`: one indented row, hanging off the thread's tile.
fn render_subagent_row(t: &ThreadSummary, row: &rows::SubagentRow, first: bool, last: bool, now: i64, p: &Palette, cx: &mut Context<Threadbear>) -> impl IntoElement {
    let sub = &row.subagent;
    let elbow_end = TRUNK + 6. + INDENT * row.depth as f32;
    let leading = TRUNK + 9. + INDENT * row.depth as f32;
    let (guide, secondary, faint, fg, failed) = (p.guide, p.secondary, p.faint, p.fg, p.failed);
    let running = sub.running();
    // Selected sub-agents can't be shown yet (no transcript for one in this
    // app); a click opens its thread instead.
    let glyph: AnyElement = match sub.status.as_str() {
        "running" => Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).color(secondary).with_size(px(11.)).into_any_element(),
        "completed" => icon("checkmark", 9., faint).into_any_element(),
        "failed" => icon("xmark", 9., failed).into_any_element(),
        _ => icon("minus", 9., faint).into_any_element(),
    };
    let tip = {
        let mut parts = vec![rows::subagent_status_text(sub).to_string()];
        if let Some(kind) = sub.agent_type.as_deref().filter(|k| !k.is_empty()) {
            parts.insert(0, kind.to_string());
        }
        if let Some(n) = sub.tool_uses.filter(|n| *n > 0) {
            parts.push(if n == 1 { "1 tool call".to_string() } else { format!("{n} tool calls") });
        }
        parts.join(" · ")
    };
    let thread_id = t.id.clone();
    let shown_tip: SharedString = tip.into();
    let tooltip_text = shown_tip.clone();
    let hover = p.hover;
    div()
        .id(ElementId::Name(format!("sub-{}-{}", t.id, sub.id).into()))
        .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
        .relative()
        .pt(px(5.))
        .pb(px(6.))
        .pl(px(leading))
        .pr(px(TRAILING))
        .rounded(px(CORNER))
        .hover(move |s| s.bg(hover))
        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _: &MouseDownEvent, window, cx| this.open(thread_id.clone(), window, cx)))
        // A faint line down from the thread's tile with an elbow into the
        // glyph, like a file tree (`SubagentGuide`).
        .child(render_guide(first, last, elbow_end, guide))
        .child(
            h_flex()
                .gap(px(7.))
                .items_start()
                .child(div().flex_none().mt(px(2.)).size(px(11.)).flex().items_center().justify_center().child(glyph))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(
                            h_flex()
                                .items_baseline()
                                .gap(px(6.))
                                .child(div().flex_1().min_w_0().truncate().text_size(px(style::size::SIDEBAR_PREVIEW)).line_height(px(16.)).text_color(if running { fg } else { secondary }).child(rows::subagent_title(sub)))
                                .child(div().flex_none().text_size(px(style::size::SIDEBAR_META)).line_height(px(16.)).text_color(faint).child(rows::subagent_duration(sub, now))),
                        )
                        .child(div().truncate().text_size(px(style::size::SIDEBAR_META + 1.)).line_height(px(15.)).text_color(if sub.status == "failed" { failed } else { faint }).child(rows::subagent_detail(sub))),
                ),
        )
}

/// The piece of tree line a sub-agent row draws: down the trunk and across
/// to the glyph (`SubagentGuide`). Drawn as a few plain lines, one pixel wide.
fn render_guide(first: bool, last: bool, elbow_end: f32, color: Hsla) -> impl IntoElement {
    let overhang = if first { 6. } else { 0. };
    let elbow_y = 13.;
    // The last row's trunk stops at its elbow; the others run on down the row.
    let trunk = div().absolute().left(px(TRUNK)).top(px(-overhang)).w(px(1.)).bg(color);
    let trunk = if last { trunk.h(px(elbow_y + overhang + 1.)) } else { trunk.bottom_0() };
    div().absolute().top_0().left_0().right_0().bottom_0().child(trunk).child(div().absolute().left(px(TRUNK)).top(px(elbow_y)).w(px(elbow_end - TRUNK)).h(px(1.)).bg(color))
}

/// Adds `rows` to a gpui-kit menu, with submenus, wiring each line to the
/// action it runs on the app.
fn fill_menu(mut menu: PopupMenu, rows_list: &[MenuRow], entity: WeakEntity<Threadbear>, thread: Option<String>, window: &mut Window, cx: &mut Context<PopupMenu>) -> PopupMenu {
    for row in rows_list {
        if row.separator {
            menu = menu.separator();
            continue;
        }
        let symbol = row.symbol.map(icons::symbol);
        if !row.children.is_empty() {
            let (children, entity2, thread2) = (row.children.clone(), entity.clone(), thread.clone());
            menu = menu.submenu_with_icon(symbol, row.title.clone(), window, cx, move |sub, window, cx| fill_menu(sub, &children, entity2.clone(), thread2.clone(), window, cx));
            continue;
        }
        let action = row.action.clone();
        let (entity2, thread2) = (entity.clone(), thread.clone());
        let mut item = PopupMenuItem::new(row.title.clone()).checked(row.checked).disabled(!row.enabled).on_click(move |_, window, cx| {
            if let Some(action) = action.clone() {
                let thread = thread2.clone();
                entity2.update(cx, |this, cx| this.run_menu_action(thread, action, window, cx)).ok();
            }
        });
        if let Some(symbol) = symbol {
            item = item.icon(symbol);
        }
        menu = menu.item(item);
    }
    menu
}

/// Scripted commands for the sidebar, the same names and shapes as the Swift
/// app's (app/README.md): hover, sort, folding a heading, starring and
/// unarchiving, snoozing a row, opening and reading its menus.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
    let name = cmd["cmd"].as_str().unwrap_or("");
    // Its full id from the start of it, as the Swift app's `target` does.
    let target = |cx: &App| -> Result<String, String> {
        match cmd["id"].as_str() {
            Some(id) => app.read(cx).thread_id_from(id),
            None => app.read(cx).selected.clone().ok_or_else(|| "no thread open".to_string()),
        }
    };
    match name {
        "hover" => {
            let id = cmd["id"].as_str().map(|id| app.read(cx).thread_id_from(id)).transpose()?;
            app.update(cx, |t, cx| {
                t.sidebar.hover_override = id.clone();
                cx.notify();
            });
            Ok(json!(id.map_or("ok".to_string(), |id| format!("ok {id}"))))
        }
        // The Archived section's Show more row.
        "archive-more" => {
            app.update(cx, |t, cx| {
                t.sidebar.archive_limit += rows::ARCHIVE_STEP;
                cx.notify();
            });
            Ok(json!("ok"))
        }
        "sort" => {
            let Some(order) = cmd["order"].as_str().and_then(InboxOrder::from_name) else { return Err(format!("order must be one of {:?}", InboxOrder::ALL.map(InboxOrder::name))) };
            app.update(cx, |t, cx| {
                t.sidebar.order = order;
                t.sidebar_settings_changed(cx);
                cx.notify();
            });
            Ok(json!("ok"))
        }
        "expand" | "collapse" => {
            // An inbox heading, as clicking it does. The Rust app's section keys
            // are the Swift enum's raw values.
            let section = cmd["section"].as_str().unwrap_or("");
            let key: &'static str = ["starred", "inbox", "running", "hibernating", "archived"].into_iter().find(|k| *k == section).ok_or_else(|| format!("section must be one of starred, inbox, running, hibernating, archived, not {section:?}"))?;
            app.update(cx, |t, cx| {
                if name == "expand" {
                    t.folded.remove(key);
                } else {
                    t.folded.insert(key);
                }
                t.sidebar_settings_changed(cx);
                cx.notify();
            });
            Ok(json!("ok"))
        }
        "star" | "unstar" => {
            let id = target(cx)?;
            app.update(cx, |t, cx| t.star_row(&id, name == "star", cx));
            Ok(json!(format!("ok {id}")))
        }
        "unarchive" => {
            let id = target(cx)?;
            app.update(cx, |t, cx| t.unarchive_row(&id, cx));
            Ok(json!(format!("ok {id}")))
        }
        "archive" => {
            let id = target(cx)?;
            app.update(cx, |t, cx| t.archive_row(id.clone(), window, cx));
            Ok(json!(format!("ok {id}")))
        }
        "snooze" => {
            let id = target(cx)?;
            if let Some(said) = cmd["said"].as_str() {
                let said = said.to_string();
                app.update(cx, |t, cx| t.hibernate_when(id.clone(), said, window, cx));
            } else {
                let seconds = cmd["seconds"].as_f64().unwrap_or(3600.);
                let until = now_ms() + (seconds * 1000.) as i64;
                app.update(cx, |t, cx| t.snooze_row(id.clone(), until, window, cx));
            }
            Ok(json!(format!("ok {id}")))
        }
        "new" => {
            app.update(cx, |t, cx| t.new_thread(cx));
            Ok(json!("ok"))
        }
        // A control by its tooltip, through a real click, as the Swift app's
        // `click` with `tip` (or `tipPrefix`, for a tip that ends with a shortcut).
        "click" => {
            let (tip, prefix) = match (cmd["tip"].as_str(), cmd["tipPrefix"].as_str()) {
                (Some(tip), _) => (tip, false),
                (_, Some(tip)) => (tip, true),
                _ => return Err("click needs title, tip or tipPrefix".into()),
            };
            window.render_frame(cx);
            let matches = |label: &str| if prefix { label.starts_with(tip) } else { label == tip };
            let found = window.elements().into_iter().filter(|e| e.visible() && e.label().is_some_and(&matches)).collect::<Vec<_>>();
            let Some(target) = found.last() else { return Err(format!("no control with the tip {tip:?}")) };
            let at = target.bounds().center();
            window.pointer_move(at, None, cx);
            window.pointer_down(at, MouseButton::Left, cx);
            window.pointer_up(at, MouseButton::Left, cx);
            Ok(json!("ok"))
        }
        // The row's right-click menu, opened the way a right-click does and
        // read back; `tipPrefix` names the row by the start of its tip.
        "realMenu" => {
            let prefix = cmd["tipPrefix"].as_str().ok_or("realMenu here takes tipPrefix")?;
            // A right-click through real events and hit testing, as a person's
            // goes, on the row whose tooltip starts with this.
            window.render_frame(cx);
            let Some(row) = window.elements().into_iter().filter(|e| e.visible() && e.label().is_some_and(|l| l.starts_with(prefix))).last() else {
                return Err(format!("no row with the tip {prefix:?} on screen"));
            };
            let at = row.bounds().center();
            window.pointer_move(at, None, cx);
            window.pointer_down(at, MouseButton::Right, cx);
            window.pointer_up(at, MouseButton::Right, cx);
            let titles: Vec<String> =app.read(cx).sidebar.menu.iter().flat_map(|m| m.rows.iter()).filter(|r| !r.separator).map(|r| if r.children.is_empty() { r.title.clone() } else { format!("{} >", r.title) }).collect();
            app.update(cx, |t, cx| {
                t.sidebar.menu = None;
                cx.notify();
            });
            Ok(json!(format!("ok {}", serde_json::to_string(&titles).unwrap_or_default())))
        }
        // The menus that stand in for a real menu in the Swift app: `show`
        // opens the Hibernate menu for a thread, `click` picks a line by
        // its title, and with neither it lists what's on show.
        "menu-popover" => {
            if let Some(title) = cmd["click"].as_str() {
                let (thread, action) = {
                    let t = app.read(cx);
                    let m = t.sidebar.menu.as_ref().ok_or_else(|| format!("no menu showing to pick {title:?}"))?;
                    let row = m.rows.iter().find(|r| r.title == title).ok_or_else(|| format!("no row {title}"))?;
                    (m.thread.clone(), row.action.clone().ok_or_else(|| format!("{title} has no action"))?)
                };
                app.update(cx, |t, cx| t.run_menu_action(thread, action, window, cx));
                return Ok(json!("ok"));
            }
            if let Some(which) = cmd["show"].as_str() {
                if which != "hibernate" {
                    return Err(format!("menu-popover show {which:?} isn't drawn here yet"));
                }
                let id = target(cx)?;
                app.update(cx, |t, cx| t.open_hibernate_menu(id, point(px(320.), px(160.)), window, cx));
                return Ok(json!("ok"));
            }
            let t = app.read(cx);
            let lines: Vec<String> = t
                .sidebar
                .menu
                .iter()
                .flat_map(|m| m.rows.iter())
                .map(|r| if r.separator { "---".to_string() } else { format!("{}{}", if r.checked { "[x] " } else { "" }, r.title) })
                .collect();
            let shown = t.sidebar.menu.is_some();
            Ok(json!(format!("ok shown {shown} {}", serde_json::to_string(&lines).unwrap_or_default())))
        }
        _ => Err(format!("unknown sidebar command {name}")),
    }
}
