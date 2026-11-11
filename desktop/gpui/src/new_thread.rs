// The new-thread page: DetailView.swift's ComposerPlacement, NewThreadHeading,
// NewThreadNote, NewThreadProjectChip and AccountRoomCard, and the new-thread
// half of AppModel's send path. The box is the same Composer a thread has, with
// no thread behind it (`Composer::thread_id` None); what it's set to lives here
// as `NewThreadState`, and what those settings work out to (which pool and
// login, the words under the box) is in threadbear_core::new_thread.
use crate::app::Threadbear;
use crate::composer::Composer;
use crate::prefs::{self, Prefs};
use crate::style::{self, size};
use crate::{call, now_ms};
use gpui_kit::assets::IconName;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// Shadows gpui's px, so sizes here follow the app's zoom (style.rs).
use crate::style::px;
use serde_json::{json, Value};
use threadbear_client::runner::{RpcError, Status};
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::logins::RunnerLogins;
use threadbear_core::new_thread::{self as draft_rules, Draft, NewThreadPicks, RoomCard, RoomLook};
use threadbear_core::reducer::ImageRef;

/// Everything about the new thread being written; the app holds one.
pub(crate) struct NewThreadState {
    /// The page's message box, made the first time the page shows.
    pub(crate) composer: Option<Entity<Composer>>,
    pub(crate) picks: NewThreadPicks,
    pub(crate) prefs: Prefs,
    /// A `threads.create` from the box hasn't come back: its words stay,
    /// greyed and locked, with a spinner for Send (AppModel.creatingThread).
    pub(crate) creating: bool,
    pub(crate) error: Option<String>,
    /// The chosen runner can't be reached, or hasn't said what logins it has.
    pub(crate) runner_problem: Option<String>,
    /// The box should take the cursor at the next render (⌘N, or the button).
    pub(crate) focus_pending: bool,
}

impl NewThreadState {
    pub(crate) fn new() -> Self {
        NewThreadState { composer: None, picks: NewThreadPicks::default(), prefs: prefs::load(), creating: false, error: None, runner_problem: None, focus_pending: false }
    }

    /// Saved when something that sticks changes (a model, a login used, the
    /// default runner).
    pub(crate) fn save(&self) {
        prefs::save(&self.prefs);
    }
}

fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_default()
}

impl Threadbear {
    /// `newThreadLogins`: this Mac's, or the chosen runner's once it has said
    /// what logins it has (nil until then).
    pub(crate) fn new_thread_logins(&self) -> Option<&RunnerLogins> {
        match &self.new_thread.picks.runner {
            None => Some(&self.logins),
            Some(runner) => self.remote.logins.get(runner),
        }
    }

    /// `draftLogins`: the same, or nothing yet for a runner that hasn't said.
    pub(crate) fn draft_logins(&self) -> RunnerLogins {
        self.new_thread_logins().cloned().unwrap_or_default()
    }

    pub(crate) fn with_draft<R>(&self, logins: &RunnerLogins, f: impl FnOnce(&Draft<'_>) -> R) -> R {
        let home = home_dir();
        let draft = Draft { logins, picks: &self.new_thread.picks, mem: &self.new_thread.prefs.remembered, home: &home, now_ms: now_ms() };
        f(&draft)
    }

    /// The box's chips read the draft through a `ThreadSummary`, as a
    /// thread's do; only the fields the chips look at matter.
    pub(crate) fn draft_summary(&self) -> ThreadSummary {
        let logins = self.draft_logins();
        self.with_draft(&logins, |d| {
            let provider = d.provider();
            ThreadSummary {
                account_id: d.account_id(),
                provider: provider.clone(),
                pool_id: d.pool_id(),
                model: self.new_thread.prefs.remembered.last_models.get(&provider).cloned(),
                effort: self.new_thread.prefs.remembered.last_efforts.get(&provider).cloned(),
                permission_mode: d.permission(),
                cwd: d.cwd(),
                ..Default::default()
            }
        })
    }

    pub(crate) fn ensure_new_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<Composer> {
        if let Some(composer) = &self.new_thread.composer {
            return composer.clone();
        }
        let client = self.client.clone();
        let app = cx.entity().downgrade();
        let composer = cx.new(|cx| Composer::new(None, client, app, window, cx));
        self.new_thread.composer = Some(composer.clone());
        composer
    }

    // ---- Sending ----

    /// The new-thread half of `AppModel.send` (and `sendNewThread` when
    /// another runner is chosen): the words stay in the box, greyed, with a
    /// spinner for Send until the runner has made the thread; then it opens.
    /// While disconnected nothing is touched at all.
    pub(crate) fn send_new_thread(&mut self, text: String, images: Vec<ImageRef>, stored: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.new_thread.creating {
            return;
        }
        if !matches!(self.status, Status::Connected { .. }) {
            self.new_thread.error = Some("Not connected to the runner, so that wasn't sent. It's still in the box.".to_owned());
            cx.notify();
            return;
        }
        let logins = self.draft_logins();
        let runner = self.new_thread.picks.runner.clone();
        let logins_known = runner.is_none() || self.remote.logins.contains_key(runner.as_deref().unwrap_or(""));
        let image_json: Vec<Value> = images.iter().map(crate::composer::image_ref_json).collect();
        let mut params = self.with_draft(&logins, |d| d.create_params(&text, image_json, !logins_known));
        if let Some(runner) = &runner {
            params["runnerId"] = json!(runner);
        }
        let project = self.new_thread.picks.project.clone().filter(|p| self.sidebar.projects.iter().any(|known| &known.id == p));
        if let Some(project) = &project {
            params["projectIds"] = json!([project]);
        }
        let account = self.with_draft(&logins, |d| d.account_id());
        let used_pool = params.get("poolId").is_some();
        self.new_thread.creating = true;
        self.new_thread.error = None;
        cx.notify();
        let client = self.client.clone();
        let runner_name = runner.as_deref().map(|id| self.runner_name_of(id));
        cx.spawn_in(window, async move |this, cx| {
            let res = call(client, "threads.create", params).await;
            this.update_in(cx, |this, window, cx| this.finish_new_thread(res, stored, runner, runner_name, account, used_pool, window, cx)).ok();
        })
        .detach();
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_new_thread(&mut self, res: Result<Value, RpcError>, stored: String, runner: Option<String>, runner_name: Option<String>, account: String, used_pool: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.new_thread.creating = false;
        let thread = match res {
            Ok(value) => serde_json::from_value::<ThreadSummary>(value["thread"].clone()).ok().filter(|t| !t.id.is_empty()),
            Err(e) => {
                // The draft never left the box.
                self.new_thread.error = Some(match &runner_name {
                    Some(name) => format!("Couldn't start it on {name}. {}", e.message),
                    None => e.message,
                });
                cx.notify();
                return;
            }
        };
        let Some(mut thread) = thread else {
            self.new_thread.error = Some("The runner didn't say what it made.".to_owned());
            cx.notify();
            return;
        };
        // Only what was sent goes; the box was locked meanwhile.
        if let Some(composer) = self.new_thread.composer.clone() {
            composer.update(cx, |c, cx| c.clear_sent_draft(&stored, window, cx));
        }
        // In a pool the runner picked the login; that's the one that counts
        // as used. A thread started on another runner doesn't move this
        // Mac's last login (`sendNewThread` doesn't remember one).
        if runner.is_none() {
            let used = if used_pool { thread.account_id.clone() } else { account };
            let provider = self.logins.account(&used).map_or("claude", |a| a.provider_id()).to_owned();
            self.new_thread.prefs.remembered.used_account(&used, &provider);
            self.new_thread.save();
        }
        // The folder, a picked mode and a picked login or pool were for this
        // thread; the next one starts from the defaults again. The model and
        // effort stick (Remembered).
        let picks = &mut self.new_thread.picks;
        *picks = NewThreadPicks { runner: None, runner_picked: false, ..Default::default() };
        self.new_thread.runner_problem = None;
        thread.runner_id = runner.clone();
        if runner.is_some() {
            // A relayed one waits for the sidebar table's row, listed as
            // that runner's meanwhile.
            self.remote.made.insert(thread.id.clone());
        }
        let id = thread.id.clone();
        self.threads.insert(id.clone(), thread);
        self.open(id, window, cx);
        self.apply_default_runner(cx);
        cx.notify();
    }

    /// A chip's change to the draft (`AppModel.newThread*` setters), from the
    /// same params a thread's chip would send to `threads.update`.
    pub(crate) fn change_new_draft(&mut self, params: &Value, cx: &mut Context<Self>) {
        let logins = self.draft_logins();
        if let Some(mode) = params["permissionMode"].as_str() {
            self.new_thread.picks.permission = Some(mode.to_owned());
        }
        if let Some(cwd) = params["cwd"].as_str() {
            self.new_thread.picks.cwd = Some(cwd.to_owned());
        }
        if let Some(account) = params["accountId"].as_str() {
            self.new_thread.picks.account = Some(account.to_owned());
            self.new_thread.picks.pool = None;
            self.new_thread.picks.provider = None;
        }
        if let Some(pool) = params["poolId"].as_str() {
            let provider = params["provider"].as_str().map(str::to_owned);
            self.new_thread.picks.account = None;
            self.new_thread.picks.pool = Some(pool.to_owned());
            self.new_thread.picks.provider = provider.clone();
            self.new_thread.prefs.remembered.used_pool(pool, provider.as_deref());
        }
        let provider = self.with_draft(&logins, |d| d.provider());
        if let Some(model) = params.get("model") {
            self.new_thread.prefs.remembered.set_model(&provider, model.as_str());
        }
        if let Some(effort) = params.get("effort") {
            self.new_thread.prefs.remembered.set_effort(&provider, effort.as_str());
        }
        self.new_thread.save();
        cx.notify();
    }

    // ---- Which runner ----

    /// `chooseNewThreadRunner`: this Mac (nil), or another runner: what was
    /// picked so far was for this Mac, so it goes back to that runner's
    /// defaults, and its logins are fetched for the pickers.
    pub(crate) fn choose_new_thread_runner(&mut self, runner: Option<String>, by_mike: bool, cx: &mut Context<Self>) {
        if by_mike {
            self.new_thread.picks.runner_picked = true;
        }
        if self.new_thread.picks.runner == runner {
            return;
        }
        let picks = &mut self.new_thread.picks;
        picks.runner = runner.clone();
        picks.cwd = None;
        picks.account = None;
        picks.pool = None;
        picks.provider = None;
        // Projects are this Mac's: another Mac's thread starts outside one.
        if runner.is_some() {
            picks.project = None;
        }
        self.new_thread.runner_problem = None;
        if let Some(runner) = runner {
            self.load_remote_logins(runner, cx);
        }
        cx.notify();
    }

    /// `setDefaultRunner`: where the next new thread starts (nil: this Mac),
    /// and the runner the draft in front of Mike starts on.
    pub(crate) fn set_default_runner(&mut self, runner: Option<String>, cx: &mut Context<Self>) {
        self.new_thread.prefs.remembered.default_runner = runner;
        self.new_thread.picks.runner_picked = false;
        self.new_thread.save();
        self.apply_default_runner(cx);
        cx.notify();
    }

    /// `applyDefaultRunner`: the default runner if it can take a new thread
    /// now (listed and online), else this Mac; unless Mike picked one himself
    /// for this draft.
    pub(crate) fn apply_default_runner(&mut self, cx: &mut Context<Self>) {
        if self.new_thread.picks.runner_picked {
            return;
        }
        let ready = self.new_thread.prefs.remembered.default_runner.clone().filter(|id| self.remote.runners.iter().any(|r| &r.id == id && r.online));
        self.choose_new_thread_runner(ready, false, cx);
    }

    /// The runner menu as the Swift `runnerMenu` builds it, for
    /// `composer-menu`: `[x]` on the chosen, `*` on the default.
    #[cfg(feature = "automation")]
    pub(crate) fn runner_menu_lines(&self) -> Vec<String> {
        let chosen = self.new_thread.picks.runner.clone();
        let default = self.new_thread.prefs.remembered.default_runner.clone();
        let mut lines = vec![format!("{}This Mac{}", if chosen.is_none() { "[x] " } else { "[ ] " }, if default.is_none() { " *" } else { "" }), "---".to_owned()];
        for runner in &self.remote.runners {
            let mut line = format!("{}{}", if chosen.as_deref() == Some(runner.id.as_str()) { "[x] " } else { "[ ] " }, runner.name);
            if default.as_deref() == Some(runner.id.as_str()) {
                line.push_str(" *");
            }
            if !runner.online {
                line.push_str(" - Offline");
            }
            lines.push(line);
        }
        lines.push("---".to_owned());
        lines.push("[ ] Default".to_owned());
        lines.push(format!("    {}This Mac", if default.is_none() { "[x] " } else { "[ ] " }));
        for runner in &self.remote.runners {
            lines.push(format!("    {}{}", if default.as_deref() == Some(runner.id.as_str()) { "[x] " } else { "[ ] " }, runner.name));
        }
        lines
    }

    /// `remoteFolders`: folders a new thread on another runner might start
    /// in: its default and the folders of its threads this Mac lists, newest
    /// first. Paths on that Mac, so nothing here is checked.
    fn remote_folders(&self, runner: &str) -> Vec<String> {
        let default_folder = self.remote.logins.get(runner).map_or_else(String::new, |l| l.defaults.cwd.clone());
        let mut threads: Vec<&ThreadSummary> = self.threads.values().filter(|t| t.runner_id.as_deref() == Some(runner)).collect();
        threads.sort_by_cached_key(|t| std::cmp::Reverse(t.activity_ms()));
        let mut out = Vec::new();
        for folder in std::iter::once(default_folder).chain(threads.into_iter().map(|t| t.cwd.clone())) {
            if !folder.is_empty() && !out.contains(&folder) {
                out.push(folder);
            }
        }
        out.truncate(8);
        out
    }

    /// The "Runs on" menu (`runnerMenu`): this Mac, then each other runner
    /// (the offline ones saying so), a star on the default, then "Default"
    /// to set it here; on another runner, its folders after that.
    pub(crate) fn open_runner_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        use crate::sidebar::{MenuAction, MenuRow};
        let chosen = self.new_thread.picks.runner.clone();
        let default = self.new_thread.prefs.remembered.default_runner.clone();
        let own = self.self_runner_symbol();
        let mut rows = vec![MenuRow::runner("This Mac", own, chosen.is_none(), default.is_none(), MenuAction::Runner(None)), MenuRow::separator()];
        for runner in &self.remote.runners {
            let name = if runner.online { runner.name.clone() } else { format!("{} (offline)", runner.name) };
            let symbol = crate::icons::runner_symbol(runner.icon.as_deref());
            rows.push(MenuRow::runner(name, symbol, chosen.as_deref() == Some(runner.id.as_str()), default.as_deref() == Some(runner.id.as_str()), MenuAction::Runner(Some(runner.id.clone()))));
        }
        rows.push(MenuRow::separator());
        let mut defaults = vec![MenuRow::runner("This Mac", own, default.is_none(), false, MenuAction::DefaultRunner(None))];
        for runner in &self.remote.runners {
            defaults.push(MenuRow::runner(runner.name.clone(), crate::icons::runner_symbol(runner.icon.as_deref()), default.as_deref() == Some(runner.id.as_str()), false, MenuAction::DefaultRunner(Some(runner.id.clone()))));
        }
        rows.push(MenuRow::submenu("Default", "star", defaults));
        // On another runner there's no "+": its folders are here.
        if let Some(runner) = chosen.as_deref().filter(|r| self.remote.logins.contains_key(*r)) {
            let current = self.draft_summary().cwd;
            let folders: Vec<MenuRow> = self.remote_folders(runner).into_iter().map(|f| { let on = f == current; MenuRow::runner_folder(f, on) }).collect();
            if !folders.is_empty() {
                rows.push(MenuRow::separator());
                rows.push(MenuRow::submenu("Folder", "folder", folders));
            }
        }
        self.open_menu(None, rows, position, false, window, cx);
    }

    // ---- Drawing ----

    /// The page: the box about 40% of the way down, the heading above it and
    /// the note under it (ComposerPlacement). One layout for any window: the
    /// box's top is placed rather than its middle, so it grows downwards as
    /// Mike types instead of creeping up the pane. Unlike ComposerPlacement
    /// it doesn't measure the heading and note to stop them being cut off in a
    /// very short window; the window's minimum height keeps them on screen.
    pub(crate) fn render_new_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let composer = self.ensure_new_composer(window, cx);
        if std::mem::take(&mut self.new_thread.focus_pending) {
            // After the frame: focusing is for the next render, not this one.
            let target = composer.clone();
            window.defer(cx, move |window, cx| target.update(cx, |c, cx| c.focus(window, cx)));
        }
        let logins = self.new_thread_logins().cloned();
        let logins_known = logins.is_some();
        let logins = logins.unwrap_or_default();
        let runner_name = self.new_thread.picks.runner.as_deref().map(|id| self.runner_name_of(id));
        let now = threadbear_core::sidebar::local_now();
        let (note, note_tip, rooms) = self.with_draft(&logins, |d| (d.note(runner_name.as_deref(), logins_known), d.note_tip(runner_name.as_deref()), d.room_cards(&now)));
        let project = self.new_thread.picks.project.clone().and_then(|id| self.sidebar.projects.iter().find(|p| p.id == id).cloned());
        let heading = div().w_full().flex().justify_center().px(px(24.)).child(
            div().text_size(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().foreground).text_center().child(draft_rules::HEADING),
        );
        let note_text = div().id("new-thread-note").text_size(px(size::LABEL)).text_color(style::faint_text(cx)).child(note).tooltip(move |window, cx| Tooltip::new(note_tip.clone()).build(window, cx));
        let error = self.new_thread.error.clone();
        let danger = cx.theme().danger;
        let notes = v_flex()
            .w_full()
            .items_center()
            .gap(px(14.))
            .mt(px(-2.))
            .when_some(project, |s, project| s.child(self.render_project_chip(project, cx)))
            .when_some(error, |s, e| s.child(div().text_size(px(size::LABEL)).text_color(danger).child(e)))
            .child(note_text)
            .when(!rooms.is_empty(), |s| s.child(self.render_room_cards(rooms, cx)));
        // The composer's place: the box's top at 40% of the pane, less half
        // an empty box (ComposerPlacementMath.composerTop). The heading sits
        // 18 pt above the box and the note 2 pt below the box's frame.
        div().relative().size_full().overflow_hidden().child(
            v_flex()
                .absolute()
                .left_0()
                .right_0()
                .top(relative(0.4))
                .mt(px(-56.))
                .child(div().relative().w_full().child(div().absolute().left_0().right_0().bottom_full().pb(px(18.)).child(heading)).child(composer))
                .child(notes),
        )
        .into_any_element()
    }

    /// `NewThreadProjectChip`: "In builder-skills" with an x to start it
    /// outside the project instead.
    fn render_project_chip(&mut self, project: crate::sidebar::ProjectBrief, cx: &mut Context<Self>) -> impl IntoElement {
        let title = project.title.clone();
        h_flex()
            .id("new-thread-project")
            .gap(px(6.))
            .px(px(10.))
            .py(px(5.))
            .rounded_full()
            .bg(style::primary_opacity(cx, 0.06))
            .tooltip({
                let tip = format!("The thread joins {title} as it starts, so the agent has its goals from the first message");
                move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
            })
            .child(self.project_glyph(&project, 16., cx))
            .child(div().text_size(px(size::LABEL)).font_weight(FontWeight::MEDIUM).child(format!("In {}", project.title)))
            .child(
                div()
                    .id("new-thread-project-clear")
                    .cursor_pointer()
                    .text_color(style::quiet_text(cx))
                    .hover(|s| s.text_color(cx.theme().foreground))
                    .child(Icon::from(IconName::X).with_size(px(9.)))
                    .tooltip(|window, cx| Tooltip::new("Start it outside the project").build(window, cx))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.new_thread.picks.project = None;
                        cx.notify();
                    })),
            )
    }

    /// `ProjectIconView` at a small size: its picture or symbol, or its letter.
    fn project_glyph(&self, project: &crate::sidebar::ProjectBrief, side: f32, cx: &App) -> AnyElement {
        let icon = self.sidebar.project_icons.get(&project.id);
        let letter = project.title.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
        let tile = |content: AnyElement| div().flex().items_center().justify_center().size(px(side)).rounded(px(4.)).bg(style::primary_opacity(cx, 0.08)).child(content).into_any_element();
        match icon {
            Some(icon) if icon.image_path.is_some() => img(std::path::PathBuf::from(icon.image_path.clone().unwrap_or_default())).size(px(side)).rounded(px(4.)).object_fit(ObjectFit::Contain).into_any_element(),
            Some(icon) if icon.symbol.as_deref().is_some_and(|s| crate::icons::try_symbol(s).is_some()) => {
                tile(crate::icons::symbol(icon.symbol.as_deref().unwrap_or("")).with_size(px(side * 0.7)).text_color(style::secondary_text(cx)).into_any_element())
            }
            _ => tile(div().text_size(px(side * 0.6)).font_weight(FontWeight::SEMIBOLD).text_color(style::secondary_text(cx)).child(letter).into_any_element()),
        }
    }

    /// The logins' cards (`NewThreadNote`'s grid), adaptive like its
    /// LazyVGrid: 140 wide at least, 200 at most, filling the row.
    fn render_room_cards(&mut self, rooms: Vec<RoomCard>, cx: &mut Context<Self>) -> impl IntoElement {
        div().w_full().max_w(px(style::MAX_CONTENT_WIDTH)).px(px(24.)).flex().flex_wrap().gap(px(8.)).children(rooms.into_iter().map(|room| self.render_room_card(room, cx)))
    }

    /// `AccountRoomCard`: its name, a bar of the tighter window, and
    /// "5h 89% · week 78% left". A click pins the draft to that login.
    fn render_room_card(&mut self, room: RoomCard, cx: &mut Context<Self>) -> impl IntoElement {
        let id = room.id.clone();
        let tip = room.tooltip.clone();
        let (selected, fraction, look) = (room.selected, room.fraction, room.look);
        let fill = match look {
            RoomLook::Blocked => style::failed(cx),
            RoomLook::Low => style::needs_you(cx),
            RoomLook::Ample => style::primary_opacity(cx, 0.4),
        };
        let line_color = if look == RoomLook::Blocked { style::failed(cx) } else { style::secondary_text(cx) };
        let (hover, background, edge) = (style::primary_opacity(cx, 0.05), style::primary_opacity(cx, if selected { 0.07 } else { 0.025 }), if selected { style::accent(cx).opacity(0.7) } else { style::primary_opacity(cx, 0.06) });
        v_flex()
            .id(ElementId::Name(format!("room-card-{id}").into()))
            .test_support()
            .aria_label(tip.clone())
            .flex_1()
            .flex_basis(px(140.))
            .max_w(px(200.))
            .gap(px(5.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(8.))
            .bg(background)
            .when(!selected, |s| s.hover(move |s| s.bg(hover)))
            .border_1()
            .border_color(edge)
            .cursor_pointer()
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                // Pins the draft to that login, like the picker's "Pin to a login".
                this.new_thread.picks.pool = None;
                this.new_thread.picks.provider = None;
                this.new_thread.picks.account = Some(id.clone());
                cx.notify();
            }))
            .child(
                h_flex()
                    .items_baseline()
                    .gap(px(6.))
                    .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(cx.theme().foreground).truncate().child(room.label))
                    .when_some(room.note, |s, note| s.child(div().text_size(px(11.)).text_color(style::tertiary_label(cx)).truncate().child(note))),
            )
            .child(
                div().relative().w_full().h(px(3.)).rounded_full().bg(style::primary_opacity(cx, 0.08)).when_some(fraction, |s, fraction| {
                    s.child(div().absolute().top_0().left_0().bottom_0().w(relative(fraction as f32)).rounded_full().bg(fill))
                }),
            )
            .child(div().text_size(px(11.)).text_color(line_color).truncate().child(room.line))
    }
}

/// Scripted commands for the page: `new` (optionally for a project, as the
/// project page's New Thread is) and `options`, the Swift names and shapes.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    let name = cmd["cmd"].as_str().unwrap_or("");
    match name {
        "new" => {
            let project = cmd["project"].as_str().map(str::to_owned);
            app.update(cx, |t, cx| {
                t.new_thread(cx);
                t.ensure_new_composer(window, cx);
                if let Some(project) = &project {
                    // New Thread on a project's page: back to this Mac, in
                    // the project's first folder, joining the project.
                    t.choose_new_thread_runner(None, true, cx);
                    if let Some(known) = t.sidebar.projects.iter().find(|p| &p.id == project).cloned() {
                        t.new_thread.picks.project = Some(known.id.clone());
                        t.new_thread.picks.cwd = known.folders.first().cloned();
                    }
                }
                cx.notify();
            });
            if project.is_some() && app.read(cx).new_thread.picks.project.is_none() {
                return Err("no such project".to_owned());
            }
            Ok(json!("ok"))
        }
        "options" => {
            let unknown: Vec<&str> = cmd.as_object().map(|o| o.keys().map(String::as_str).filter(|k| !["cmd", "rid", "token", "cwd", "account", "model", "effort", "ask", "pool", "provider"].contains(k)).collect()).unwrap_or_default();
            if !unknown.is_empty() {
                return Err(format!("options takes cwd, account, pool, provider, model, effort and ask, not {}", unknown.join(", ")));
            }
            if let Some(account) = cmd["account"].as_str() {
                let known = app.read(cx).logins.accounts.iter().any(|a| a.id == account);
                if !app.read(cx).logins.accounts.is_empty() && !known {
                    return Err(format!("no account {account}"));
                }
            }
            app.update(cx, |t, cx| {
                if let Some(cwd) = cmd["cwd"].as_str() {
                    t.new_thread.picks.cwd = Some(cwd.to_owned());
                }
                if let Some(account) = cmd["account"].as_str() {
                    t.new_thread.picks.account = Some(account.to_owned());
                }
                // A pool picked for the draft (null: back to the folder's or
                // the last one), and which of a mixed pool's providers. Both
                // unpin it, as the Swift `options` does.
                if let Some(pool) = cmd.get("pool") {
                    t.new_thread.picks.pool = pool.as_str().map(str::to_owned);
                    t.new_thread.picks.account = None;
                }
                if let Some(provider) = cmd.get("provider") {
                    t.new_thread.picks.provider = provider.as_str().map(str::to_owned);
                }
                if let Some(ask) = cmd["ask"].as_bool() {
                    t.new_thread.picks.permission = Some(if ask { "ask" } else { "full" }.to_owned());
                }
                let logins = t.draft_logins();
                let provider = t.with_draft(&logins, |d| d.provider());
                if let Some(model) = cmd.get("model") {
                    t.new_thread.prefs.remembered.set_model(&provider, model.as_str());
                }
                if let Some(effort) = cmd.get("effort") {
                    t.new_thread.prefs.remembered.set_effort(&provider, effort.as_str());
                }
                cx.notify();
            });
            Ok(json!(new_thread_dump_line(app, cx)))
        }
        _ => Err(format!("unknown new-thread command {name}")),
    }
}

/// `options`' answer, as the Swift one words it.
#[cfg(feature = "automation")]
fn new_thread_dump_line(app: &Entity<Threadbear>, cx: &App) -> String {
    let t = app.read(cx);
    let logins = t.draft_logins();
    t.with_draft(&logins, |d| {
        let provider = d.provider();
        let model = t.new_thread.prefs.remembered.last_models.get(&provider).cloned().unwrap_or_else(|| "default".to_owned());
        let effort = t.new_thread.prefs.remembered.last_efforts.get(&provider).cloned().unwrap_or_else(|| "default".to_owned());
        let ask = d.permission() == "ask";
        format!("ok cwd={} account={}{} model={model} effort={effort} ask={ask}", d.cwd(), d.account_id(), d.pool_id().map(|p| format!(" pool={p}")).unwrap_or_default())
    })
}

/// The Runs on menu for scripts (`composer-menu` with `runner`): its lines,
/// or `click` picks one, or `setDefault` sets the star.
#[cfg(feature = "automation")]
pub(crate) fn runner_menu_command(app: &Entity<Threadbear>, cmd: &Value, cx: &mut App) -> Result<Value, String> {
    if app.read(cx).selected.is_some() {
        return Err("runner is for a new thread".to_owned());
    }
    if let Some(name) = cmd["setDefault"].as_str() {
        let runner = if name == "This Mac" { None } else { Some(app.read(cx).remote.runners.iter().find(|r| r.name == name).ok_or_else(|| format!("no runner {name}"))?.id.clone()) };
        app.update(cx, |t, cx| t.set_default_runner(runner, cx));
        return Ok(json!("ok"));
    }
    if let Some(title) = cmd["click"].as_str() {
        let runner = if title == "This Mac" { None } else { Some(app.read(cx).remote.runners.iter().find(|r| r.name == title).ok_or_else(|| format!("no item {title}"))?.id.clone()) };
        app.update(cx, |t, cx| t.choose_new_thread_runner(runner, true, cx));
        return Ok(json!("ok"));
    }
    Ok(json!(app.read(cx).runner_menu_lines()))
}

/// The new-thread page's part of the dump (`AppDump.newThread`), or null
/// with something else in the pane.
#[cfg(feature = "automation")]
pub(crate) fn page_dump(t: &Threadbear, _cx: &App) -> Value {
    if t.selected.is_some() {
        return Value::Null;
    }
    let logins = t.new_thread_logins().cloned();
    let known = logins.is_some();
    let logins = logins.unwrap_or_default();
    let runner_name = t.new_thread.picks.runner.as_deref().map(|id| t.runner_name_of(id));
    let now = threadbear_core::sidebar::local_now();
    let (note, rooms) = t.with_draft(&logins, |d| (d.note(runner_name.as_deref(), known), d.room_cards(&now)));
    let project = t.new_thread.picks.project.as_ref().and_then(|id| t.sidebar.projects.iter().find(|p| &p.id == id)).map(|p| format!("In {}", p.title));
    json!({
        "heading": draft_rules::HEADING,
        "note": note,
        "project": project,
        "rooms": rooms.iter().map(|r| json!({ "label": r.label, "line": r.line, "selected": r.selected, "note": r.note })).collect::<Vec<_>>(),
    })
}
