// What sits over and around the thread: its title line in the panel's bar with
// back, forward, share and the ... menu (DetailView's HeaderTitle and
// ThreadMenuButton, HistoryButtons, ShareButton), the banners above the
// transcript (ConnectionBanner, ErrorBanner, LimitBanner, SignInBanner,
// RefusalOffer) and the notes that float over the panel (ArchivedToast,
// InboxEndToast).
use crate::app::Threadbear;
use crate::shell::{self, SidebarMode};
use crate::sidebar::{MenuAction, MenuRow};
use crate::style::{self, px};
use crate::{call, icons, now_ms};
use gpui_kit::assets::IconName;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::{json, Value};
use threadbear_client::runner::Status;
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::navigation::history_tip;
use threadbear_core::reducer::{RowContent, TurnCompletion, TurnErrorKind, TurnOutcome};
use threadbear_core::shortcuts::ShortcutCommand;

/// `PaneHeader.height`: the panels' bars, and the title bar's strip.
pub(crate) const HEADER_HEIGHT: f32 = 38.;

/// A control's tooltip and the label scripts find it by (Swift's `.tip`), as
/// `tipped!` in the sidebar: the element's result is whatever `test_support`
/// makes of it, so this is a macro rather than a function.
macro_rules! tipped {
    ($el:expr, $tip:expr $(,)?) => {{
        let tip_text: SharedString = $tip.into();
        let shown = tip_text.clone();
        $el.test_support().aria_label(tip_text).tooltip(move |window, cx| Tooltip::new(shown.clone()).build(window, cx))
    }};
}

/// `Banner`: a line about the runner, quiet or warning.
struct Banner {
    quiet: bool,
    text: String,
    detail: Option<String>,
    retry: bool,
}

/// The first of what `limit_advice` and `sign_in` say about the open thread.
pub(crate) struct LimitAdvice {
    text: String,
    target: Option<(String, String)>,
    /// A pool thread waiting for room: it carries on by itself, so a quiet
    /// clock rather than the red gauge, and the button pins it outside the pool.
    waiting: bool,
}

impl Threadbear {
    // The title line (`HeaderTitle`).

    /// The thread's title in the panel's bar, or a field while it's renamed.
    pub(crate) fn render_header_title(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(thread) = self.open_thread().cloned().filter(|_| self.mode == SidebarMode::Inbox) else { return div().into_any_element() };
        let faint = style::faint_text(cx);
        let secondary = style::secondary_text(cx);
        let hover = style::primary_opacity(cx, 0.06);
        let header_size = px(style::size::HEADER);
        let projects = self.render_project_chips(&thread, cx);
        let title_el: AnyElement = if self.renaming.as_deref() == Some(thread.id.as_str()) {
            self.render_rename_field(cx)
        } else {
            let title: SharedString = if thread.title.is_empty() { "Untitled".into() } else { thread.title.clone().into() };
            let id = thread.id.clone();
            tipped!(
                div()
                    .id("header-title")
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .px(px(4.))
                    .py(px(1.))
                    .mx(px(-4.))
                    .rounded(px(5.))
                    .hover(move |s| s.bg(hover)),
                "Click to rename",
            )
            // The press goes on to the bar under it, so a drag from the title
            // moves the window, as dragging a title bar does (and Swift's
            // tap gesture let it). Only a press that didn't is a click.
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                if !this.dock_view.read(cx).press_moved() {
                    this.start_renaming(id.clone(), window, cx);
                }
            }))
            .child(title)
            .into_any_element()
        };
        let worktree = thread.worktree.as_ref().and_then(|w| w["branch"].as_str()).map(String::from);
        let schedule = thread.schedule.clone();
        let _ = window;
        h_flex()
            .min_w_0()
            .gap(px(6.))
            .text_size(header_size)
            .child(projects)
            .child(title_el)
            .when(thread.is_archived(), |s| s.child(div().flex_none().text_color(faint).child("·  Archived")))
            .when(thread.on_old_runner == Some(true), |s| {
                s.child(tipped!(div().id("old-runner").flex_none().text_color(faint), "This turn started before the runner updated, so it runs on the previous version. The thread moves to the new one when the turn ends.").child("·  Finishing on the previous version"))
            })
            .when_some(worktree, |s, branch| s.child(div().flex_none().text_color(faint).child(format!("·  {branch}"))))
            .when_some(schedule, |s, origin| {
                let tip = origin["title"].as_str().map_or("Started by a schedule".to_string(), |t| format!("Started by the schedule \u{201C}{t}\u{201D}"));
                s.child(
                    tipped!(h_flex().id("schedule-chip").flex_none().ml(px(4.)).gap(px(4.)).items_center().pl(px(5.)).pr(px(7.)).py(px(2.)).rounded_full().bg(style::primary_opacity(cx, 0.05)).text_size(px(style::size::DETAIL)).text_color(secondary), tip)
                        .child(icon("clock", 11., secondary))
                        .child("Scheduled"),
                )
            })
            .into_any_element()
    }

    /// One of the thread's projects, as a small tile before the title; or the
    /// folder glyph when it's in none (`ProjectIconButton`, the `folder` mark).
    fn render_project_chips(&mut self, thread: &ThreadSummary, cx: &mut Context<Self>) -> AnyElement {
        let projects = thread.projects.clone().unwrap_or_default();
        let secondary = style::secondary_text(cx);
        let cwd = thread.cwd.clone();
        if projects.is_empty() || !self.projects_supported {
            let tip = format!("In {}", abbreviate(&cwd));
            return tipped!(div().id("header-folder").flex_none(), tip).child(icon("folder", style::size::DETAIL, secondary)).into_any_element();
        }
        let tile_fill = style::primary_opacity(cx, 0.08);
        let hover = style::primary_opacity(cx, 0.08);
        let icons_by_project = self.sidebar.project_icons.clone();
        let icon_of = move |id: &str| icons_by_project.get(id).cloned();
        h_flex()
            .flex_none()
            .gap(px(3.))
            .children(projects.into_iter().enumerate().map(|(index, project)| {
                let (open_id, menu_project, menu_thread) = (project.clone(), project.clone(), thread.id.clone());
                let letter: SharedString = project.title.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default().into();
                let tip = format!("{}, in {}. Click to open the project", project.title, abbreviate(&cwd));
                tipped!(h_flex().id(("project-chip", index)).size(px(20.)).items_center().justify_center().rounded(px(5.)).hover(move |s| s.bg(hover)), tip)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let _ = &open_id;
                        // A project's page isn't in this app yet: the Projects
                        // view is where it will open.
                        this.set_mode(SidebarMode::Projects, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                            let rows = this.project_chip_rows(&menu_thread, &menu_project);
                            let thread = menu_thread.clone();
                            this.open_menu(Some(thread), rows, ev.position, false, window, cx);
                        }),
                    )
                    .child(project_face(icon_of(&project.id), letter, tile_fill, secondary))
            }))
            .into_any_element()
    }

    /// The menu on a project's chip: open it, keep it, take the thread out, or
    /// add it to another (`ProjectIconButton`'s menu).
    fn project_chip_rows(&self, thread_id: &str, project: &threadbear_core::inbox::Project) -> Vec<MenuRow> {
        let mut rows = vec![MenuRow::item(format!("Open {}", project.title), "arrow.right.circle", MenuAction::OpenProject), MenuRow::separator()];
        if !project.pinned {
            rows.push(MenuRow::item(format!("Keep it in {}", project.title), "pin", MenuAction::ThreadProject { thread: thread_id.to_string(), add: Some(project.id.clone()), remove: None }));
        }
        rows.push(MenuRow::item(format!("Take it out of {}", project.title), "minus.circle", MenuAction::ThreadProject { thread: thread_id.to_string(), add: None, remove: Some(project.id.clone()) }));
        let others = self.projects_to_add(thread_id);
        if !others.is_empty() {
            let children = others.into_iter().map(|(id, title)| MenuRow::item(title, "square.stack", MenuAction::ThreadProject { thread: thread_id.to_string(), add: Some(id), remove: None })).collect();
            rows.push(MenuRow::submenu("Add to another project", "plus.square.on.square", children));
        }
        rows
    }

    /// `projectsToAdd`: the projects this thread isn't in, by name.
    fn projects_to_add(&self, thread_id: &str) -> Vec<(String, String)> {
        let already: Vec<&str> = self.threads.get(thread_id).and_then(|t| t.projects.as_ref()).map(|p| p.iter().map(|x| x.id.as_str()).collect()).unwrap_or_default();
        let mut others: Vec<(String, String)> = self.all_projects.iter().filter(|(id, _, archived)| !archived && !already.contains(&id.as_str())).map(|(id, title, _)| (id.clone(), title.clone())).collect();
        others.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        others
    }

    /// `threads.setProjects`, for a chip's menu.
    pub(crate) fn set_thread_projects(&mut self, thread: String, add: Option<String>, remove: Option<String>, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let params = json!({ "threadId": thread, "add": add.map(|a| vec![a]).unwrap_or_default(), "remove": remove.map(|r| vec![r]).unwrap_or_default() });
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.setProjects", params).await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(v) => {
                        if let Ok(t) = serde_json::from_value::<ThreadSummary>(v["thread"].clone()) {
                            this.threads.insert(t.id.clone(), t);
                        }
                    }
                    Err(e) => this.show_error(e.message, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_rename_field(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some((field, _)) = &self.rename_field else { return div().into_any_element() };
        div()
            .id("title-field")
            .min_w(px(160.))
            .flex_1()
            .font_weight(FontWeight::SEMIBOLD)
            // Escape puts it back: captured so the field doesn't just keep it.
            .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                if ev.keystroke.key == "escape" {
                    this.finish_rename(false, cx);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .tooltip(|window, cx| Tooltip::new("Return saves, Escape cancels").build(window, cx))
            .child(Input::new(field).appearance(false).small().pl(px(0.)))
            .into_any_element()
    }

    /// Makes the title a field, as a click on it does (`startRenaming`).
    pub(crate) fn start_renaming(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.threads.get(&id).map(|t| t.title.clone()) else { return };
        if self.selected.as_deref() != Some(id.as_str()) {
            self.open(id.clone(), window, cx);
        }
        let field = cx.new(|cx| InputState::new(window, cx).placeholder("Thread title").default_value(title));
        let subscription = cx.subscribe(&field, |this, _, ev: &InputEvent, cx| match ev {
            InputEvent::PressEnter { .. } | InputEvent::Blur => this.finish_rename(true, cx),
            _ => {}
        });
        let handle = field.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        self.rename_field = Some((field, subscription));
        self.renaming = Some(id);
        cx.notify();
    }

    /// Return or clicking away saves, Escape puts it back. A name given here
    /// is Mike's, so the runner won't let a model or an agent rename it after.
    pub(crate) fn finish_rename(&mut self, save: bool, cx: &mut Context<Self>) {
        let Some(id) = self.renaming.take() else { return };
        let Some((field, _)) = self.rename_field.take() else { return };
        if save {
            let draft = field.read(cx).value().to_string();
            self.rename_thread(&id, &draft, cx);
        }
        cx.notify();
    }

    /// `rename`: Mike's own name for a thread, shown at once, put back if it
    /// fails.
    pub(crate) fn rename_thread(&mut self, id: &str, title: &str, cx: &mut Context<Self>) {
        let title = title.trim().to_string();
        if title.is_empty() || self.threads.get(id).is_some_and(|t| t.title == title) {
            return;
        }
        let local = title.clone();
        self.update_thread(id, move |t| t.title = local, json!({ "title": title }), cx);
    }

    // The bar's buttons: back, forward, share, the ... menu.

    pub(crate) fn render_header_buttons(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let thread = self.open_thread().cloned().filter(|_| self.mode == SidebarMode::Inbox);
        let back = self.render_history_button(true, cx);
        let forward = self.render_history_button(false, cx);
        let share = thread.as_ref().filter(|_| self.website_url.is_some()).map(|t| self.render_share_button(t, cx));
        let more = thread.as_ref().map(|t| self.render_more_button(t, cx));
        h_flex().flex_none().items_center().child(back).child(forward).children(share).children(more).into_any_element()
    }

    /// `HistoryButton`: ‹ or ›, tipped with where it goes, dimmed with nowhere.
    fn render_history_button(&mut self, back: bool, cx: &mut Context<Self>) -> AnyElement {
        let enabled = if back { self.can_go_back() } else { self.can_go_forward() };
        let destination = enabled.then(|| self.destination(back)).flatten();
        let shortcut = self.shortcuts.key(if back { ShortcutCommand::Back } else { ShortcutCommand::Forward }).map(|k| k.display());
        let tip = history_tip(back, destination.as_deref(), shortcut.as_deref());
        let secondary = style::secondary_text(cx);
        let symbol = if back { "chevron.left" } else { "chevron.right" };
        tipped!(div().id(if back { "history-back" } else { "history-forward" }).w(px(22.)).h(px(26.)).flex().items_center().justify_center().when(!enabled, |s| s.opacity(0.35)), tip)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(enabled, |s| {
                s.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    if back {
                        this.go_back(window, cx);
                    } else {
                        this.go_forward(window, cx);
                    }
                    this.sync_menus(cx);
                }))
            })
            .child(icon(symbol, style::size::LABEL, secondary))
            .into_any_element()
    }

    /// The share icon: accent when someone else can read the thread.
    fn render_share_button(&mut self, thread: &ThreadSummary, cx: &mut Context<Self>) -> AnyElement {
        let on = share_is_on(thread);
        let tip = if on { "Shared: anyone with the link can read it" } else { "Share this thread on the web" };
        let color = if on { style::accent(cx) } else { style::secondary_text(cx) };
        let open = self.share_popover.as_deref() == Some(thread.id.as_str());
        let id = thread.id.clone();
        let hover = style::sidebar_hover(cx);
        let mut button = tipped!(div().id("header-share").relative().w(px(26.)).h(px(26.)).flex().items_center().justify_center().rounded(px(6.)).hover(move |s| s.bg(hover)), tip)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.share_popover = if this.share_popover.as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
                cx.notify();
            }))
            .child(icon("square.and.arrow.up", style::size::LABEL, color));
        if open {
            button = button.child(self.render_share_popover(thread, cx));
        }
        button.into_any_element()
    }

    /// Who can read the thread, the link, and Copy (`SharePopover`).
    fn render_share_popover(&mut self, thread: &ThreadSummary, cx: &mut Context<Self>) -> AnyElement {
        let on = share_is_on(thread);
        let link = self.share_link(thread);
        let id = thread.id.clone();
        let busy = self.share_busy;
        let (secondary, bg, border) = (style::secondary_text(cx), cx.theme().popover, cx.theme().border);
        let option = |label: &'static str, access: &'static str, selected: bool, id: String, cx: &mut Context<Self>| {
            tipped!(div().id(label).flex_1().flex().justify_center().px(px(8.)).py(px(4.)).rounded(px(6.)).when(selected, |s| s.bg(style::primary_opacity(cx, 0.08))).text_size(px(style::size::LABEL)), "Who can read it")
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if !this.share_busy {
                        this.set_share(id.clone(), access, false, cx);
                    }
                }))
                .child(label)
        };
        let rows = vec![option("Only you", "off", !on, id.clone(), cx).into_any_element(), option("Anyone with the link", "link", on, id.clone(), cx).into_any_element()];
        let id_for_new = id.clone();
        let link_row = link.clone().map(|link| {
            let (copy, open) = (link.clone(), link.clone());
            h_flex()
                .gap(px(6.))
                .items_center()
                .child(div().flex_1().min_w_0().truncate().text_size(px(style::size::DETAIL)).text_color(secondary).child(link))
                .child(self.small_button("share-new", "arrow.triangle.2.circlepath", "Make a new link. The old one stops working.", cx.listener(move |this, _: &ClickEvent, _, cx| this.set_share(id_for_new.clone(), "link", true, cx)), cx))
                .child(self.small_button("share-open", "arrow.up.right", "Open the shared page in your browser", cx.listener(move |_, _: &ClickEvent, _, cx| { crate::open_gate::open_web(&open, cx); }), cx))
                .child(tipped!(div().id("share-copy").px(px(8.)).py(px(3.)).rounded(px(6.)).bg(style::primary_opacity(cx, 0.08)).text_size(px(style::size::DETAIL)), "Copy the link to paste anywhere").on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone())))).child("Copy link"))
        });
        deferred(
            anchored().child(
                div()
                    .id("share-popover")
                    .occlude()
                    .w(px(380.))
                    .p(px(16.))
                    .rounded(px(12.))
                    .bg(bg)
                    .border_1()
                    .border_color(border)
                    .shadow_lg()
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.share_popover = None;
                        cx.notify();
                    }))
                    .child(
                        v_flex()
                            .gap(px(12.))
                            .child(div().font_weight(FontWeight::SEMIBOLD).text_size(px(style::size::LABEL)).child("Share this thread"))
                            .child(h_flex().gap(px(4.)).children(rows))
                            .when(busy, |s| s.child(div().size(px(14.)).child(Spinner::new().icon(IconName::LoaderCircle).color(secondary).with_size(px(14.)))))
                            .when_some(link_row, |s, l| s.child(l)),
                    ),
            ),
        )
        .with_priority(2)
        .into_any_element()
    }

    fn small_button(&self, id: &'static str, symbol: &'static str, tip: &'static str, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static, cx: &mut Context<Self>) -> AnyElement {
        let secondary = style::secondary_text(cx);
        tipped!(div().id(id).size(px(22.)).flex().items_center().justify_center().rounded(px(5.)), tip).on_click(on_click).child(icon(symbol, 12., secondary)).into_any_element()
    }

    pub(crate) fn share_link(&self, thread: &ThreadSummary) -> Option<String> {
        let website = self.website_url.as_ref()?;
        let share = thread.share.as_ref()?;
        (share["access"] == "link").then(|| format!("{}/s/{}", website.trim_end_matches('/'), share["slug"].as_str().unwrap_or("")))
    }

    /// Sets sharing through `threads.share`, the same call the popover makes.
    pub(crate) fn set_share(&mut self, id: String, access: &str, new_link: bool, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let params = json!({ "threadId": id, "access": access, "newLink": new_link });
        self.share_busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.share", params).await;
            this.update(cx, |this, cx| {
                this.share_busy = false;
                match res {
                    Ok(v) => {
                        if let Ok(t) = serde_json::from_value::<ThreadSummary>(v["thread"].clone()) {
                            this.threads.insert(t.id.clone(), t);
                        }
                    }
                    Err(e) => this.show_error(e.message, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The thread's ... menu: the same as its row's, with Copy transcript and
    /// the thread's own web page (`ThreadMenuButton`).
    fn render_more_button(&mut self, thread: &ThreadSummary, cx: &mut Context<Self>) -> AnyElement {
        let id = thread.id.clone();
        let secondary = style::secondary_text(cx);
        let hover = style::sidebar_hover(cx);
        tipped!(div().id("header-more").w(px(26.)).h(px(26.)).flex().items_center().justify_center().rounded(px(6.)).hover(move |s| s.bg(hover)), "More")
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                let at = ev.position();
                this.open_more_menu(id.clone(), point(at.x, at.y + px(14.)), window, cx);
            }))
            .child(icon("ellipsis", style::size::LABEL, secondary))
            .into_any_element()
    }

    pub(crate) fn open_more_menu(&mut self, id: String, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(thread) = self.threads.get(&id).cloned() else { return };
        let mut rows = self.thread_menu_rows(&thread);
        // Swift's menu has "Copy thread id" last and Copy transcript above it;
        // the row's menu ends with that divider and item.
        rows.truncate(rows.len().saturating_sub(2));
        rows.push(MenuRow::separator());
        rows.push(MenuRow::item("Copy transcript as Markdown", "doc.plaintext", MenuAction::CopyTranscript));
        rows.push(MenuRow::item("Copy thread id", "number", MenuAction::CopyId));
        // Only while there's a website to go to, as with the share button.
        if self.website_url.is_some() {
            rows.push(MenuRow::item("Open on the web", "safari", MenuAction::OpenOnWeb));
        }
        self.open_menu(Some(id), rows, position, false, window, cx);
    }

    pub(crate) fn thread_web_link(&self, id: &str) -> Option<String> {
        self.website_url.as_ref().map(|w| format!("{}/t/{}", w.trim_end_matches('/'), id))
    }

    // The banners over the transcript.

    pub(crate) fn render_banners(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let _ = window;
        let thread = self.open_thread().cloned();
        let mut column = v_flex().flex_none();
        if let Some(banner) = self.connection_banner() {
            column = column.child(self.render_connection_banner(banner, cx));
        }
        if let Some(error) = self.error_message.clone() {
            column = column.child(self.render_error_banner(error, cx));
        }
        if let Some(thread) = &thread {
            if let Some(advice) = self.limit_advice(thread, cx) {
                column = column.child(self.render_limit_banner(thread, advice, cx));
            }
            if let Some(account) = self.sign_in_account(thread, cx) {
                column = column.child(self.render_signin_banner(thread, account, cx));
            }
            if let Some((account, title)) = self.refusal_target(thread, cx) {
                column = column.child(self.render_refusal_offer(thread, account, title, cx));
            }
        }
        column.into_any_element()
    }

    /// `AppModel.banner`: the line about the runner, if there's anything to say.
    fn connection_banner(&self) -> Option<Banner> {
        let restarting = self.restart_expected();
        match &self.status {
            Status::Connected { .. } => {
                if restarting {
                    return Some(Banner { quiet: true, text: "The runner is restarting now.".into(), detail: None, retry: false });
                }
                if self.sidebar.restart_pending {
                    return Some(Banner { quiet: true, text: "The runner will restart once its threads are done.".into(), detail: None, retry: false });
                }
                None
            }
            Status::Connecting { .. } if restarting => Some(Banner { quiet: true, text: "The runner is restarting. Reconnecting…".into(), detail: None, retry: false }),
            Status::Disconnected { .. } if restarting => Some(Banner { quiet: true, text: "The runner is restarting. Reconnecting…".into(), detail: None, retry: false }),
            Status::Connecting { .. } => Some(Banner { quiet: false, text: "Connecting to the runner…".into(), detail: None, retry: true }),
            Status::Disconnected { reason, .. } => Some(Banner { quiet: false, text: "Lost the connection to the runner. Reconnecting…".into(), detail: Some(reason.clone()), retry: true }),
        }
    }

    /// Whether the runner said it's restarting, recently enough that it being
    /// gone is expected (`RunnerRestart.patience`).
    fn restart_expected(&self) -> bool {
        self.restart_notice_ms.is_some_and(|t| now_ms() - t < 120_000)
    }

    fn render_connection_banner(&mut self, banner: Banner, cx: &mut Context<Self>) -> AnyElement {
        let hairline = style::hairline(cx);
        let secondary = style::secondary_text(cx);
        let spinner = || Spinner::new().icon(IconName::LoaderCircle).color(secondary).with_size(px(13.));
        if banner.quiet {
            return h_flex()
                .flex_none()
                .px(px(14.))
                .py(px(7.))
                .gap(px(8.))
                .items_center()
                .border_b_1()
                .border_color(hairline)
                .text_size(px(style::size::LABEL))
                .text_color(secondary)
                .child(spinner())
                .child(div().truncate().child(banner.text))
                .into_any_element();
        }
        let detail = banner.detail.clone().unwrap_or_default();
        let warning_fill = style::warning_fill(cx);
        h_flex()
            .flex_none()
            .px(px(14.))
            .py(px(8.))
            .gap(px(8.))
            .items_center()
            .bg(warning_fill)
            .text_size(px(style::size::LABEL))
            .child(icon("bolt.horizontal", 13., secondary))
            .child(tipped!(div().id("connection-text").flex_1().min_w_0().truncate(), detail).child(banner.text))
            .when(banner.retry, |s| {
                s.child(
                    tipped!(div().id("banner-retry").px(px(8.)).py(px(3.)).rounded(px(5.)).bg(style::primary_opacity(cx, 0.08)), "Try to reach the runner now rather than waiting")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reconnect_now(cx)))
                        .child("Try again"),
                )
            })
            .into_any_element()
    }

    fn render_error_banner(&mut self, error: String, cx: &mut Context<Self>) -> AnyElement {
        let fill = style::needs_you(cx).opacity(0.1);
        let orange = style::needs_you(cx);
        let tip = error.clone();
        h_flex()
            .flex_none()
            .px(px(14.))
            .py(px(7.))
            .gap(px(8.))
            .items_center()
            .bg(fill)
            .text_size(px(style::size::LABEL))
            .child(icon("exclamationmark.triangle", 13., orange))
            .child(tipped!(div().id("error-text").flex_1().min_w_0(), tip).child(div().line_clamp(2).child(error)))
            .when(self.own_checkout().is_some(), |s| s.child(self.render_investigate_button(cx)))
            .child(tipped!(div().id("error-dismiss").px(px(6.)).py(px(2.)).rounded(px(5.)).text_color(style::secondary_text(cx)), "Hide this message").on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.dismiss_error(cx))).child("Dismiss"))
            .into_any_element()
    }

    fn render_investigate_button(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let creating = self.creating_investigation;
        let secondary = style::secondary_text(cx);
        h_flex()
            .gap(px(6.))
            .items_center()
            .when(creating, |s| s.child(Spinner::new().icon(IconName::LoaderCircle).color(secondary).with_size(px(13.))))
            .child(
                tipped!(div().id("error-investigate").px(px(6.)).py(px(2.)).rounded(px(5.)).when(creating, |s| s.opacity(0.5)), "Start a thread in Threadbear's own repo to find what caused this and fix it")
                    .when(!creating, |s| s.on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.investigate_error(window, cx))))
                    .child("Investigate"),
            )
            .into_any_element()
    }

    /// `noteFailedAccounts`: the login a thread was on as it went into
    /// error, kept until it leaves error.
    pub(crate) fn note_failed_account(&mut self, before: Option<&ThreadSummary>, now: &ThreadSummary) {
        if now.status == "error" {
            if before.is_none_or(|b| b.status != "error") || !self.failed_on_account.contains_key(&now.id) {
                self.failed_on_account.insert(now.id.clone(), now.account_id.clone());
            }
        } else {
            self.failed_on_account.remove(&now.id);
        }
    }

    /// The same over a new list of every thread, against the one before it.
    pub(crate) fn note_failed_accounts(&mut self, before: &std::collections::HashMap<String, ThreadSummary>) {
        let now: Vec<ThreadSummary> = self.threads.values().cloned().collect();
        for t in &now {
            self.note_failed_account(before.get(&t.id), t);
        }
        let known: std::collections::HashSet<String> = self.threads.keys().cloned().collect();
        self.failed_on_account.retain(|id, _| known.contains(id));
    }

    /// `limitAdvice`: when the open thread can't go on where it is, says so,
    /// and where it could go instead.
    fn limit_advice(&self, thread: &ThreadSummary, cx: &App) -> Option<LimitAdvice> {
        use threadbear_core::new_thread::{has_room, pool_logins, switch_target, switch_title, usage_of};
        let accounts = &self.logins.accounts;
        let target = switch_target(accounts, &thread.account_id).map(|a| (a.id.clone(), switch_title(a, accounts)));
        // A pool thread is the runner's to move: nothing while its pool has a
        // login with room, and when it carries on once it's waiting. The
        // button still offers a login outside the pool, which pins it there.
        if self.logins.uses_pools() {
            if let Some(pool) = thread.pool_id.as_deref().and_then(|id| self.logins.pool(id)) {
                if let Some(until) = thread.waiting_for_room_until_ms() {
                    let now = threadbear_core::sidebar::local_now();
                    return Some(LimitAdvice { text: threadbear_core::sidebar::waiting_sentence(&pool.title, until, &now), target, waiting: true });
                }
                if pool_logins(pool, accounts, None).into_iter().any(has_room) {
                    return None;
                }
            }
        }
        let usage = self.logins.account(&thread.account_id).and_then(usage_of);
        let blocked = usage.as_ref().is_some_and(|u| u.blocked);
        // Only while the thread is still on the login the turn failed on:
        // after "Switch to ..." the status stays error until the next turn,
        // and the line would blame the new login.
        let failed_on_limit = thread.status == "error"
            && self.failed_on_account.get(&thread.id) == Some(&thread.account_id)
            && self.latest_completion(cx).is_some_and(|c| c.error_kind == Some(TurnErrorKind::UsageLimit));
        if !blocked && !failed_on_limit {
            return None;
        }
        let mut text = format!("{} has reached its limit", threadbear_core::logins::short_account_name(&thread.account_id, accounts));
        // The soonest of the windows that are used up (`mainWindows`).
        let reset = usage.as_ref().and_then(|u| threadbear_core::new_thread::main_windows(u).into_iter().filter(|w| w.remaining_percent < 1.).filter_map(|w| w.resets_at).min());
        if let Some(ms) = reset {
            let now = threadbear_core::sidebar::local_now();
            let at = threadbear_core::chrono::TimeZone::timestamp_millis_opt(&now.timezone(), ms).single();
            if let Some(at) = at {
                text.push_str(&format!(", resets {}", threadbear_core::new_thread::reset_time(&at, &now)));
            }
        }
        text.push('.');
        Some(LimitAdvice { text, target, waiting: false })
    }

    /// `limit` in the dump (`AppDump.LimitInfo`): the line's words and its button.
    #[cfg(feature = "automation")]
    pub(crate) fn limit_dump(&self, cx: &App) -> Value {
        let Some(advice) = self.open_thread().and_then(|t| self.limit_advice(t, cx)) else { return Value::Null };
        json!({ "text": advice.text, "button": advice.target.map(|(_, title)| limit_button_title(title, advice.waiting)) })
    }

    fn render_limit_banner(&mut self, thread: &ThreadSummary, advice: LimitAdvice, cx: &mut Context<Self>) -> AnyElement {
        let red = cx.theme().danger;
        let hairline = style::hairline(cx);
        let id = thread.id.clone();
        h_flex()
            .flex_none()
            .px(px(14.))
            .py(px(7.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(hairline)
            .text_size(px(style::size::LABEL))
            .child(if advice.waiting { icon("clock", 13., style::secondary_text(cx)) } else { icon("gauge.with.dots.needle.100percent", 13., red) })
            .child({
                let tip = advice.text.clone();
                tipped!(div().id("limit-text").flex_1().min_w_0().truncate(), tip).child(advice.text)
            })
            .when_some(advice.target.map(|(account, title)| (account, limit_button_title(title, advice.waiting))), |s, (account, title)| {
                s.child(
                    tipped!(div().id("limit-switch").px(px(8.)).py(px(3.)).rounded(px(5.)).bg(style::primary_opacity(cx, 0.08)), "Carry on this thread on that account, from where it is")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.switch_account(&id, account.clone(), cx)))
                        .child(title),
                )
            })
            .into_any_element()
    }

    pub(crate) fn switch_account(&mut self, thread: &str, account: String, cx: &mut Context<Self>) {
        self.update_thread(thread, |_| {}, json!({ "accountId": account }), cx);
    }

    /// `signInAccount`: the login to sign in again, when the open thread's
    /// last turn failed with its login refused (and it's this runner's).
    fn sign_in_account(&self, thread: &ThreadSummary, cx: &App) -> Option<(String, String)> {
        if thread.status != "error" {
            return None;
        }
        let completion = self.latest_completion(cx)?;
        if completion.outcome != TurnOutcome::Failed || completion.error_kind != Some(TurnErrorKind::Auth) {
            return None;
        }
        let provider = thread.provider.clone();
        let name = if provider == "codex" { "Codex".to_string() } else { "Claude".to_string() };
        Some((thread.account_id.clone(), name))
    }

    fn render_signin_banner(&mut self, thread: &ThreadSummary, account: (String, String), cx: &mut Context<Self>) -> AnyElement {
        let (account_id, name) = account;
        let hairline = style::hairline(cx);
        let orange = style::needs_you(cx);
        let id = thread.id.clone();
        let command = if name == "Codex" { "codex login" } else { "claude login" };
        let tip = format!("The login {account_id} was refused: signed out, or its session ran out");
        let hint = format!("Run {command} in Terminal to sign in, then Send again");
        h_flex()
            .flex_none()
            .px(px(14.))
            .py(px(7.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(hairline)
            .text_size(px(style::size::LABEL))
            .child(icon("person.crop.circle.badge.exclamationmark", 13., orange))
            .child(tipped!(div().id("signin-text").flex_1().min_w_0().truncate(), tip).child(format!("{name} needs you to sign in again.")))
            .child(
                tipped!(div().id("signin-button").px(px(8.)).py(px(3.)).rounded(px(5.)).bg(style::primary_opacity(cx, 0.08)), hint.clone())
                    .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| window.push_notification(hint.clone(), cx)))
                    .child("Sign in…"),
            )
            .child(
                tipped!(div().id("signin-again").px(px(8.)).py(px(3.)).rounded(px(5.)).bg(style::primary_opacity(cx, 0.08)), "Send your last message again, once you've signed in")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.send_again(&id, cx)))
                    .child("Send again"),
            )
            .into_any_element()
    }

    /// Sends the thread's last message of his again, after signing in
    /// (`sendAgain`).
    pub(crate) fn send_again(&mut self, _thread: &str, cx: &mut Context<Self>) {
        if let Some(pane) = &self.pane {
            pane.update(cx, |p, cx| {
                if let Some(text) = p.last_user_text() {
                    p.send(text, None, cx);
                }
            });
        }
    }

    pub(crate) fn latest_completion(&self, cx: &App) -> Option<TurnCompletion> {
        let pane = self.pane.as_ref()?;
        let p = pane.read(cx);
        p.feed.model().rows().iter().rev().take(40).find_map(|row| match &row.content {
            RowContent::Footer(c) => Some(c.clone()),
            _ => None,
        })
    }

    /// `refusalTarget`: a Claude turn the safeguards refused, with a Codex
    /// login that can carry on.
    pub(crate) fn refusal_target(&self, thread: &ThreadSummary, cx: &App) -> Option<(String, String)> {
        if thread.status != "error" || thread.provider != "claude" {
            return None;
        }
        let completion = self.latest_completion(cx)?;
        if completion.outcome != TurnOutcome::Failed {
            return None;
        }
        let error = completion.error.as_deref()?.to_lowercase();
        let refused = error.contains("api error:") && (error.contains("reasoning_extraction") || (error.contains("safeguards flagged this message") && error.contains("claude")));
        if !refused {
            return None;
        }
        self.logins
            .accounts
            .iter()
            .find(|a| a.provider == "codex" && a.models.iter().any(|m| m.id == REFUSAL_MODEL) && !a.usage.as_ref().is_some_and(|u| u["blocked"].as_bool() == Some(true)) && a.note.as_deref().unwrap_or("").is_empty())
            .map(|a| (a.id.clone(), a.name()))
    }

    fn render_refusal_offer(&mut self, thread: &ThreadSummary, account: String, title: String, cx: &mut Context<Self>) -> AnyElement {
        let continuing = self.continuing_refusal.as_deref() == Some(thread.id.as_str());
        let id = thread.id.clone();
        let tip = format!("Move this thread to {title} on GPT-6.1-Sol and continue the task. Your draft stays in the box.");
        let (hairline, fill, secondary) = (style::hairline(cx), style::card_background(cx), style::secondary_text(cx));
        h_flex()
            .flex_none()
            .mx(px(14.))
            .my(px(8.))
            .p(px(12.))
            .gap(px(12.))
            .items_center()
            .rounded(px(12.))
            .bg(fill)
            .border_1()
            .border_color(hairline)
            .text_size(px(style::size::LABEL))
            .child(div().flex_1().min_w_0().text_color(secondary).child("Claude couldn't continue with this message."))
            .child(
                tipped!(div().id("refusal-continue").px(px(10.)).py(px(4.)).rounded(px(6.)).bg(style::primary_opacity(cx, 0.1)).when(continuing, |s| s.opacity(0.5)), tip)
                    .when(!continuing, |s| {
                        s.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.continue_refusal(id.clone(), account.clone(), cx);
                        }))
                    })
                    .child(if continuing { "Continuing…" } else { "Continue with Codex Sol" }),
            )
            .into_any_element()
    }

    /// `continueRefusal`: moves the thread to the Codex login, then sends the
    /// continuation; the draft stays where it is.
    pub(crate) fn continue_refusal(&mut self, thread: String, account: String, cx: &mut Context<Self>) {
        if self.continuing_refusal.as_deref() == Some(thread.as_str()) {
            return;
        }
        self.continuing_refusal = Some(thread.clone());
        cx.notify();
        let client = self.client.clone();
        let params = json!({ "threadId": thread, "accountId": account, "model": REFUSAL_MODEL });
        cx.spawn(async move |this, cx| {
            let moved = call(client.clone(), "threads.update", params).await;
            let sent = match &moved {
                Ok(v) if v["thread"]["provider"] == "codex" => call(client, "threads.send", json!({ "threadId": thread, "text": REFUSAL_CONTINUATION })).await.map(|_| ()),
                Ok(_) => Err(threadbear_client::runner::RpcError { code: -1, message: "Codex Sol is no longer available. Choose a model to carry on.".into(), data: None }),
                Err(e) => Err(e.clone()),
            };
            this.update(cx, |this, cx| {
                this.continuing_refusal = None;
                if let Err(e) = sent {
                    this.show_error(e.message, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Shows `message` in the error bar (`showError`).
    pub(crate) fn show_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.error_message = Some(message);
        cx.notify();
    }

    fn dismiss_error(&mut self, cx: &mut Context<Self>) {
        self.error_message = None;
        cx.notify();
    }

    /// The checkout this app follows, where a thread can fix it: for a debug
    /// build, the one it was compiled from, when the file is there
    /// (`ownCheckout`). A release build asks the runner in a later slice.
    fn own_checkout(&self) -> Option<String> {
        if cfg!(debug_assertions) {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            if path.join("AGENTS.md").exists() {
                return path.canonicalize().ok().map(|p| p.to_string_lossy().to_string());
            }
        }
        None
    }

    /// The error bar's Investigate: a new thread in the app's own checkout
    /// that finds the error's cause and fixes it; the bar goes and the new
    /// thread opens once it's made (`investigateError`).
    pub(crate) fn investigate_error(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(message), Some(checkout)) = (self.error_message.clone(), self.own_checkout()) else { return };
        if self.creating_investigation {
            return;
        }
        self.creating_investigation = true;
        cx.notify();
        let client = self.client.clone();
        let params = json!({ "cwd": checkout, "text": format!("Find what caused this error in Threadbear and fix it:\n\n{message}"), "title": format!("Investigate: {message}"), "permissionMode": "full" });
        cx.spawn_in(window, async move |this, cx| {
            let res = call(client, "threads.create", params).await;
            this.update_in(cx, |this, window, cx| {
                this.creating_investigation = false;
                match res {
                    Ok(v) => {
                        this.error_message = None;
                        if let Ok(t) = serde_json::from_value::<ThreadSummary>(v["thread"].clone()) {
                            let id = t.id.clone();
                            this.threads.insert(t.id.clone(), t);
                            this.open(id, window, cx);
                        }
                    }
                    Err(e) => this.show_error(e.message, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn reconnect_now(&mut self, cx: &mut Context<Self>) {
        self.client.retry_now();
        cx.notify();
    }

    // The notes floating over the panel.

    pub(crate) fn render_toasts(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let _ = window;
        let overlay = div().absolute().top(px(10.)).left_0().right_0().flex().justify_center().items_start();
        if let Some(note) = self.archived_note.clone() {
            return overlay.child(self.render_archived_toast(note, cx)).into_any_element();
        }
        if let Some(note) = self.inbox_end_note {
            return overlay.child(self.render_inbox_end_toast(note, cx)).into_any_element();
        }
        div().into_any_element()
    }

    fn toast_capsule(&self, id: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        let bg = cx.theme().popover;
        let border = cx.theme().border;
        div()
            .id(id)
            .occlude()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(14.))
            .py(px(7.))
            .rounded_full()
            .bg(bg)
            .border_1()
            .border_color(border)
            .shadow_lg()
            .text_size(px(style::size::LABEL))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.toast_hovered = *hovered;
                cx.notify();
            }))
    }

    /// "Archived" and the thread's name with Undo; a click on it opens the
    /// thread in the Archived section (`ArchivedToast`).
    fn render_archived_toast(&mut self, note: shell::ArchivedNote, cx: &mut Context<Self>) -> AnyElement {
        let (secondary, separator, accent) = (style::secondary_text(cx), style::hairline(cx), style::accent(cx));
        let title = if note.title.is_empty() { "Untitled".to_string() } else { note.title.clone() };
        self.toast_capsule("archived-toast", cx)
            .child(
                tipped!(h_flex().id("archived-open").gap(px(7.)).items_center(), "Open this thread")
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.open_archived_note(window, cx)))
                    .child(icon("archivebox", style::size::LABEL, secondary))
                    .child(div().text_color(secondary).child("Archived"))
                    .child(div().max_w(px(280.)).truncate().child(title)),
            )
            .child(div().w(gpui_kit::px(1.)).h(px(14.)).bg(separator))
            .child(
                tipped!(div().id("archived-undo").text_color(accent).font_weight(FontWeight::MEDIUM), "Put it back in the inbox")
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.undo_archive(window, cx)))
                    .child("Undo"),
            )
            .into_any_element()
    }

    /// Next Inbox Thread with nowhere to go (`InboxEndToast`): a click takes
    /// it down.
    fn render_inbox_end_toast(&mut self, note: shell::InboxEndNote, cx: &mut Context<Self>) -> AnyElement {
        let (secondary, green) = (style::secondary_text(cx), style::tick_green(cx));
        let (title, detail) = if note.empty { ("Inbox clear", "Nothing's waiting on you") } else { ("That's the end of the inbox", "You're all caught up") };
        self.toast_capsule("inbox-end-toast", cx)
            .gap(px(7.))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.dismiss_inbox_end(cx)))
            .child(icon("checkmark.circle.fill", style::size::LABEL, green))
            .child(title)
            .child(div().text_color(secondary).child(detail))
            .into_any_element()
    }

    // For the automation and the specs.

    /// The header as `dump` reports it (`ThreadHeader`).
    #[cfg(feature = "automation")]
    pub(crate) fn header_dump(&self) -> Option<Value> {
        let t = self.open_thread().filter(|_| self.mode == SidebarMode::Inbox)?;
        let back_to = self.can_go_back().then(|| self.destination(true)).flatten();
        let forward_to = self.can_go_forward().then(|| self.destination(false)).flatten();
        let back_key = self.shortcuts.key(ShortcutCommand::Back).map(|k| k.display());
        let forward_key = self.shortcuts.key(ShortcutCommand::Forward).map(|k| k.display());
        Some(json!({
            "title": if t.title.is_empty() { "Untitled" } else { t.title.as_str() },
            "renaming": self.renaming.as_deref() == Some(t.id.as_str()),
            "backTip": history_tip(true, back_to.as_deref(), back_key.as_deref()),
            "forwardTip": history_tip(false, forward_to.as_deref(), forward_key.as_deref()),
            "shared": share_is_on(t),
        }))
    }

    /// `banner` in the dump (`AppDump.BannerInfo`).
    #[cfg(feature = "automation")]
    pub(crate) fn banner_dump(&self) -> Value {
        match self.connection_banner() {
            Some(b) => json!({ "style": if b.quiet { "quiet" } else { "warning" }, "text": b.text, "detail": b.detail }),
            None => Value::Null,
        }
    }

    /// The share popover's state, as the Swift `share` command says it.
    #[cfg(feature = "automation")]
    pub(crate) fn share_status(&self) -> String {
        let Some(t) = self.open_thread() else { return "error no thread is open".into() };
        let access = t.share.as_ref().and_then(|s| s["access"].as_str()).unwrap_or("off");
        let link = self.share_link(t).unwrap_or_else(|| "none".into());
        let web = self.thread_web_link(&t.id).unwrap_or_else(|| "none".into());
        format!("ok access {access} link {link} popover {} web {web}", if self.share_popover.as_deref() == Some(t.id.as_str()) { "shown" } else { "closed" })
    }

    #[cfg(feature = "automation")]
    pub(crate) fn signin_status(&self, cx: &App) -> String {
        match self.open_thread().and_then(|t| self.sign_in_account(t, cx)) {
            Some((account, _)) => format!("ok {account} {}", self.open_thread().map(|t| t.provider.clone()).unwrap_or_default()),
            None => "ok none".into(),
        }
    }

    #[cfg(feature = "automation")]
    pub(crate) fn refusal_status(&self, cx: &App) -> String {
        match self.open_thread().and_then(|t| self.refusal_target(t, cx)) {
            Some((account, _)) => format!("ok {account} {REFUSAL_MODEL}"),
            None => "ok none".into(),
        }
    }
}

/// `RefusalRecovery.model` and `continuation`.
const REFUSAL_MODEL: &str = "gpt-6.1-sol";
const REFUSAL_CONTINUATION: &str = "Please continue with the task from where it stopped.";

fn share_is_on(thread: &ThreadSummary) -> bool {
    thread.share.as_ref().is_some_and(|s| s["access"] == "link")
}

/// The site for the runner's `runner.status` `sync`: its `site`, else the
/// deployment's .convex.site host (`RunnerStatus.websiteURL`).
pub(crate) fn website_url(status: &Value) -> Option<String> {
    let sync = status.get("sync")?;
    if let Some(site) = sync["site"].as_str().filter(|s| s.starts_with("https://")) {
        return Some(site.to_string());
    }
    let url = sync["url"].as_str()?;
    url.contains(".convex.cloud").then(|| url.replace(".convex.cloud", ".convex.site"))
}

/// An SF Symbol's icon, at a point size and in a colour.
fn icon(name: &str, size: f32, color: Hsla) -> Icon {
    icons::symbol(name).with_size(px(size)).text_color(color)
}

/// `abbreviatingHome`, for tooltips.
fn abbreviate(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path == home => "~".into(),
        Ok(home) if !home.is_empty() && path.starts_with(&format!("{home}/")) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

/// `ProjectIconView` at 16 pt: the project's picture, else its symbol from
/// the bundled set, else its first letter on a faint tile (the header showed
/// only the letter, where the Swift app shows the bear or the picture).
fn project_face(icon: Option<crate::sidebar::ProjectIcon>, letter: SharedString, fill: Hsla, secondary: Hsla) -> AnyElement {
    let letter_tile = move || div().size(px(16.)).flex().items_center().justify_center().rounded(px(4.)).bg(fill).text_size(px(10.)).font_weight(FontWeight::SEMIBOLD).child(letter.clone()).into_any_element();
    match icon {
        Some(crate::sidebar::ProjectIcon { image_path: Some(path), .. }) => {
            let fallback = letter_tile.clone();
            img(std::path::PathBuf::from(path)).size(px(16.)).rounded(px(4.)).object_fit(ObjectFit::Contain).with_fallback(fallback).into_any_element()
        }
        Some(crate::sidebar::ProjectIcon { symbol: Some(symbol), .. }) if crate::icons::try_symbol(&symbol).is_some() => {
            div().size(px(16.)).flex().items_center().justify_center().child(crate::icons::symbol(&symbol).with_size(px(13.)).text_color(secondary)).into_any_element()
        }
        _ => letter_tile(),
    }
}

/// The limit line's button: "Switch to mike (78% left)", or "Pin to ..." on a
/// pool thread waiting for room, as a login outside the pool pins it there.
fn limit_button_title(title: String, waiting: bool) -> String {
    if waiting {
        title.replacen("Switch to", "Pin to", 1)
    } else {
        title
    }
}
