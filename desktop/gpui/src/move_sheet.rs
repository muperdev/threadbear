// The Move sheet (MoveSheet.swift) and the Move to menu's path through
// AppModel+Moves.swift, for the `moveToRunner` experiment. A move that can just
// go (threads.movePreview says nothing's in the way) goes straight away; the
// sheet is for a problem or something that won't come, and it asks the other
// runner again as the folder or login changes, so what it says can't be done
// shows before the thread freezes.
use crate::app::Threadbear;
use crate::call;
use crate::style::{self, size};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, Selectable, Sizable as _, StyledExt as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// Shadows gpui's px, so sizes here follow the app's zoom (style.rs).
use crate::style::px;
use serde_json::json;
#[cfg(feature = "automation")]
use serde_json::Value;
use threadbear_client::runner::Status;
use threadbear_core::remote::{MovePreview, RunnerInfo};

/// `AppModel.MoveRequest`: which thread, to which runner.
#[derive(Clone)]
pub(crate) struct MoveRequest {
    pub(crate) thread_id: String,
    pub(crate) runner_id: String,
    pub(crate) runner_name: String,
}

impl Threadbear {
    /// The menu shows: the experiment's on here and there's somewhere to go.
    pub(crate) fn can_move_threads(&self) -> bool {
        self.remote.experiments.iter().any(|e| e == "moveToRunner")
    }

    /// `moveTargets`: every runner but the one it's on, offline ones included
    /// so the menu can grey them.
    pub(crate) fn move_targets(&self, runner_id: Option<&str>) -> Vec<RunnerInfo> {
        let self_id = self.remote.all_runners.iter().find(|r| r.is_self).map(|r| r.id.clone());
        let here = runner_id.map(str::to_owned).or(self_id);
        self.remote.all_runners.iter().filter(|r| Some(r.id.clone()) != here).cloned().collect()
    }

    /// Move to ▸ a runner (`openMove`): it goes straight away when the target
    /// has a clean answer; the sheet is for a problem, or something that
    /// won't come. The row says "Moving to …" at once, and goes back if the
    /// sheet turns out to be needed or the runner turns the move down.
    pub(crate) fn open_move(&mut self, request: MoveRequest, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.threads.get(&request.thread_id).cloned();
        if !matches!(self.status, Status::Connected { .. }) {
            // The sheet opens too, and waits for a connection.
            self.show_move_sheet(request, None, window, cx);
            return;
        }
        if let Some(t) = self.threads.get_mut(&request.thread_id) {
            t.moving = Some(json!({ "moveId": "", "to": request.runner_id, "toName": request.runner_name, "state": "asking" }));
        }
        cx.notify();
        let client = self.client.clone();
        let params = json!({ "threadId": request.thread_id, "runnerId": request.runner_id });
        let put_back = before.clone();
        let thread_id = request.thread_id.clone();
        cx.spawn_in(window, async move |this, cx| {
            let answer = call(client, "threads.movePreview", params).await;
            this.update_in(cx, |this, window, cx| {
                let preview = answer.as_ref().ok().map(MovePreview::decode);
                if preview.as_ref().is_some_and(MovePreview::can_go_as_is) {
                    this.start_move(request.clone(), None, None, cx);
                } else {
                    // The sheet comes up instead; the guess goes back until
                    // Move is pressed there.
                    if let (Some(t), Some(before)) = (this.threads.get_mut(&thread_id), &put_back) {
                        if t.moving.as_ref().is_some_and(|m| m["moveId"] == "") {
                            t.moving = before.moving.clone();
                        }
                    }
                    let shown = preview.or_else(|| answer.err().map(|e| MovePreview { problems: vec![e.message], ..Default::default() }));
                    this.show_move_sheet(request.clone(), shown, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// `threads.move`, as Move in the sheet does; the row says it's going
    /// at once and goes back if the runner turns it down.
    pub(crate) fn start_move(&mut self, request: MoveRequest, cwd: Option<String>, account: Option<String>, cx: &mut Context<Self>) {
        let before = self.threads.get(&request.thread_id).cloned();
        if let Some(t) = self.threads.get_mut(&request.thread_id) {
            if t.moving.is_none() {
                t.moving = Some(json!({ "moveId": "", "to": request.runner_id, "toName": request.runner_name, "state": "asking" }));
            }
        }
        cx.notify();
        let client = self.client.clone();
        let mut params = json!({ "threadId": request.thread_id, "runnerId": request.runner_id });
        if let Some(cwd) = cwd {
            params["cwd"] = json!(cwd);
        }
        if let Some(account) = account {
            params["accountId"] = json!(account);
        }
        let thread_id = request.thread_id.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.move", params).await;
            this.update(cx, |this, cx| {
                if let Err(e) = res {
                    this.list_error = Some(e.message);
                    if let (Some(t), Some(before)) = (this.threads.get_mut(&thread_id), &before) {
                        if t.moving.as_ref().is_some_and(|m| m["moveId"] == "") {
                            t.moving = before.moving.clone();
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Cancel on a moving row, before the thread has gone (`cancelMove`).
    pub(crate) fn cancel_move(&mut self, thread_id: String, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.moveCancel", json!({ "threadId": thread_id })).await;
            if let Err(e) = res {
                this.update(cx, |this, cx| {
                    this.list_error = Some(e.message);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// The sheet, filled from what the runner answered (nil: it asks itself).
    fn show_move_sheet(&mut self, request: MoveRequest, answer: Option<MovePreview>, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.threads.get(&request.thread_id).map_or_else(String::new, |t| t.title.clone());
        let busy = self.threads.get(&request.thread_id).is_some_and(|t| t.busy());
        let app = cx.entity().downgrade();
        let app_for_close = app.clone();
        let view = cx.new(|cx| MoveSheet::new(app, request, title, busy, answer, window, cx));
        self.move_sheet = Some(view.clone());
        cx.notify();
        window.open_dialog(cx, move |dialog, _, _| {
            let app = app_for_close.clone();
            dialog
                .w(px(460.))
                .close_button(false)
                .child(view.clone())
                .footer(div())
                .on_close(move |_, _, cx| {
                    if let Some(app) = app.upgrade() {
                        app.update(cx, |app, cx| {
                            app.move_sheet = None;
                            cx.notify();
                        });
                    }
                })
        });
    }
}

/// The sheet's content (`MoveSheet`).
pub(crate) struct MoveSheet {
    app: WeakEntity<Threadbear>,
    request: MoveRequest,
    title: String,
    busy: bool,
    preview: Option<MovePreview>,
    asking: bool,
    folder: Entity<InputState>,
    login: String,
    /// What the runner guessed, so only Mike's own changes are sent.
    guessed_folder: String,
    guessed_login: String,
    /// The runner's first guess has arrived.
    filled: bool,
    /// The newest check; an answer to an older one is ignored.
    asks: u64,
    _subs: Vec<Subscription>,
}

impl MoveSheet {
    fn new(app: WeakEntity<Threadbear>, request: MoveRequest, title: String, busy: bool, answer: Option<MovePreview>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let folder = cx.new(|cx| InputState::new(window, cx).placeholder(format!("Folder on {}", request.runner_name)));
        let sub = cx.subscribe_in(&folder, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.ask(window, cx);
            }
        });
        let mut sheet = MoveSheet { app, request, title, busy, preview: None, asking: false, folder, login: String::new(), guessed_folder: String::new(), guessed_login: String::new(), filled: false, asks: 0, _subs: vec![sub] };
        match answer {
            // Opened with the runner's answer in hand (`request.answer`).
            Some(known) => sheet.apply(known, None, None, window, cx),
            None => sheet.ask(window, cx),
        }
        sheet
    }

    fn connected(&self, cx: &App) -> bool {
        self.app.upgrade().is_some_and(|a| matches!(a.read(cx).status, Status::Connected { .. }))
    }

    /// Asks again for what the fields say, leaving out whatever is still the
    /// runner's guess.
    fn ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.asks += 1;
        let ask = self.asks;
        let folder = self.folder.read(cx).value().to_string();
        let (cwd, account) = ((folder != self.guessed_folder).then_some(folder), (self.login != self.guessed_login).then(|| self.login.clone()));
        let mut params = json!({ "threadId": self.request.thread_id, "runnerId": self.request.runner_id });
        if let Some(cwd) = &cwd {
            params["cwd"] = json!(cwd);
        }
        if let Some(account) = &account {
            params["accountId"] = json!(account);
        }
        self.asking = true;
        cx.notify();
        let Some(client) = self.app.upgrade().map(|a| a.read(cx).client.clone()) else { return };
        cx.spawn_in(window, async move |this, cx| {
            let answer = call(client, "threads.movePreview", params).await;
            this.update_in(cx, |this, window, cx| {
                if this.asks != ask {
                    return;
                }
                this.asking = false;
                match answer {
                    Ok(value) => this.apply(MovePreview::decode(&value), cwd, account, window, cx),
                    // Not an answer about the move: said here until asked again.
                    Err(e) => this.preview = Some(MovePreview { problems: vec![e.message], ..Default::default() }),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A field still on the guess follows it; one Mike has changed since
    /// asking is left alone.
    fn apply(&mut self, answer: MovePreview, cwd: Option<String>, account: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if cwd.is_none() {
            if let Some(guess) = answer.cwd.clone() {
                let current = self.folder.read(cx).value().to_string();
                if current == self.guessed_folder {
                    self.folder.update(cx, |f, cx| f.set_value(guess.clone(), window, cx));
                }
                self.guessed_folder = guess;
            }
        }
        if account.is_none() {
            if let Some(guess) = answer.account_id.clone() {
                if self.login == self.guessed_login {
                    self.login = guess.clone();
                }
                self.guessed_login = guess;
            }
        }
        self.preview = Some(answer);
        self.filled = true;
        cx.notify();
    }

    fn can_move(&self, cx: &App) -> bool {
        let folder = self.folder.read(cx).value();
        !self.asking && self.connected(cx) && !self.login.is_empty() && !folder.is_empty() && self.preview.as_ref().is_some_and(|p| p.problems.is_empty())
    }

    fn set_login(&mut self, login: String, window: &mut Window, cx: &mut Context<Self>) {
        if login != self.login {
            let changed_from = self.login.clone();
            self.login = login;
            if !changed_from.is_empty() {
                self.ask(window, cx);
            }
            cx.notify();
        }
    }

    fn close(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.close_dialog(cx);
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| {
                app.move_sheet = None;
                cx.notify();
            });
        }
    }

    fn press_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_move(cx) {
            return;
        }
        let folder = self.folder.read(cx).value().to_string();
        let (cwd, account) = ((folder != self.guessed_folder).then_some(folder), (self.login != self.guessed_login).then(|| self.login.clone()));
        let request = self.request.clone();
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| app.start_move(request, cwd, account, cx));
        }
        self.close(window, cx);
    }

    /// The dump's `moveSheet` (`AppDump.MoveSheetInfo`).
    #[cfg(feature = "automation")]
    pub(crate) fn dump(&self, cx: &App) -> Value {
        let preview = self.preview.clone().unwrap_or_default();
        let subtitle = self.subtitle();
        let header = format!("Move \u{201C}{}\u{201D} to {}", self.display_title(), self.request.runner_name);
        json!({
            "title": header,
            "subtitle": subtitle,
            "folder": self.folder.read(cx).value().to_string(),
            "login": self.login,
            "problems": preview.problems,
            "leftBehind": preview.left_behind,
            "clone": preview.clone,
            "asking": self.asking,
            "canMove": self.can_move(cx),
        })
    }

    fn display_title(&self) -> String {
        if self.title.is_empty() {
            "Untitled".to_owned()
        } else {
            self.title.clone()
        }
    }

    fn subtitle(&self) -> &'static str {
        if self.busy {
            "It goes once this turn ends. Messages sent meanwhile go with it."
        } else {
            "Its whole conversation and its git work go with it, and it carries on there."
        }
    }

    /// The Login picker: the runner's logins, the chosen one ticked.
    fn render_login_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accounts = self.preview.as_ref().map(|p| p.accounts.clone()).unwrap_or_default();
        let current = accounts.iter().find(|a| a.id == self.login).map_or_else(|| self.login.clone(), |a| a.label());
        let sheet = cx.entity().downgrade();
        Popover::new("move-login")
            .anchor(Anchor::BottomLeft)
            .trigger(MoveLoginTrigger { label: current.into() })
            .content(move |_, _window, cx| {
                let popover = cx.entity();
                v_flex().w(px(300.)).p(px(4.)).children(accounts.iter().enumerate().map(|(i, account)| {
                    let (popover, sheet, id) = (popover.clone(), sheet.clone(), account.id.clone());
                    let on = sheet.upgrade().is_some_and(|s| s.read(cx).login == account.id);
                    h_flex()
                        .id(("move-login", i))
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(style::hover_fill(cx)))
                        .on_click(move |_, window, cx| {
                            popover.update(cx, |s, cx| s.dismiss(window, cx));
                            if let Some(sheet) = sheet.upgrade() {
                                sheet.update(cx, |s, cx| s.set_login(id.clone(), window, cx));
                            }
                        })
                        .child(div().flex_1().min_w_0().truncate().text_size(px(size::LABEL)).child(account.label()))
                        .when(on, |s| s.child(Icon::from(gpui_kit::assets::IconName::Check).with_size(px(14.))))
                }))
            })
    }
}

/// The picker's face: the current login with a chevron.
#[derive(IntoElement)]
struct MoveLoginTrigger {
    label: SharedString,
}

impl Selectable for MoveLoginTrigger {
    fn selected(self, _selected: bool) -> Self {
        self
    }
    fn is_selected(&self) -> bool {
        false
    }
}

impl RenderOnce for MoveLoginTrigger {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .id("move-login-trigger")
            .w_full()
            .h(px(28.))
            .px(px(8.))
            .gap(px(6.))
            .rounded(px(6.))
            .border_1()
            .border_color(style::hairline(cx))
            .bg(style::button_fill(cx))
            .cursor_pointer()
            .tooltip(|window, cx| Tooltip::new("The login it carries on with there").build(window, cx))
            .child(div().flex_1().min_w_0().truncate().text_size(px(size::LABEL)).child(self.label))
            .child(Icon::from(gpui_kit::assets::IconName::ChevronDown).with_size(px(11.)).text_color(style::faint_icon(cx)))
    }
}

impl Render for MoveSheet {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.request.runner_name.clone();
        let header = format!("Move \u{201C}{}\u{201D} to {name}", self.display_title());
        let preview = self.preview.clone();
        let can_move = self.can_move(cx);
        let connected = self.connected(cx);
        let waiting = self.asking || !connected;
        let (problems, left_behind, clone, worktree) = preview.map_or((vec![], vec![], None, false), |p| (p.problems, p.left_behind, p.clone, p.worktree));
        let folder_tip = if worktree { format!("The repo on {name} it gets a new worktree of its own in. Press Return to check another folder.") } else { format!("Where it runs on {name}. Press Return to check another folder.") };
        let login_picker = self.render_login_picker(cx);
        let label = |text: &'static str| div().w(px(52.)).flex_none().text_right().text_size(px(size::LABEL)).text_color(style::secondary_text(cx)).child(text);
        v_flex()
            .gap(px(12.))
            .child(
                v_flex()
                    .gap(px(4.))
                    .child(div().text_size(px(size::CARD_TITLE)).font_semibold().line_clamp(2).child(header))
                    .child(div().text_size(px(size::LABEL)).text_color(style::secondary_text(cx)).child(self.subtitle())),
            )
            .child(
                v_flex()
                    .gap(px(8.))
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .items_center()
                            .child(label("Folder"))
                            .child(div().id("move-folder").flex_1().min_w_0().tooltip(move |window, cx| Tooltip::new(folder_tip.clone()).build(window, cx)).child(Input::new(&self.folder).small().font_family(cx.theme().mono_font_family.clone()))),
                    )
                    .child(h_flex().gap(px(10.)).items_center().child(label("Login")).child(div().flex_1().min_w_0().child(login_picker))),
            )
            .children(problems.into_iter().map(|line| h_flex().gap(px(6.)).items_start().text_size(px(size::LABEL)).text_color(style::failed(cx)).child(Icon::from(gpui_kit::assets::IconName::OctagonX).with_size(px(13.))).child(div().flex_1().child(line))))
            .when_some(clone, |s, clone| s.child(div().text_size(px(size::LABEL)).text_color(style::secondary_text(cx)).child(format!("{name} has no copy of its repo, so it's cloned to {clone} first."))))
            .children(left_behind.into_iter().map(|line| div().text_size(px(size::LABEL)).text_color(style::needs_you_text(cx)).child(format!("Won't come: {line}"))))
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .when(waiting, |s| {
                        s.child(Spinner::new().color(style::quiet_text(cx)).with_size(px(12.)))
                            .child(div().text_size(px(size::LABEL)).text_color(style::secondary_text(cx)).child(if connected { format!("Asking {name}…") } else { "Waiting for this Mac's runner…".to_owned() }))
                    })
                    .child(div().flex_1())
                    .child(div().id("move-cancel").test_support().aria_label("Close without moving it".to_owned()).child(Button::new("move-cancel-button").label("Cancel").small().outline().tooltip("Close without moving it").on_click(cx.listener(|this, _, window, cx| this.close(window, cx)))))
                    .child(
                        div()
                            .id("move-go")
                            .test_support()
                            .aria_label(if can_move { format!("Move it to {name} once this turn ends") } else { "Fix what's in the way first".to_owned() })
                            .child(
                                Button::new("move-go-button")
                                    .label("Move")
                                    .small()
                                    .primary()
                                    .disabled(!can_move)
                                    .tooltip(if can_move { format!("Move it to {name} once this turn ends") } else { "Fix what's in the way first".to_owned() })
                                    .on_click(cx.listener(|this, _, window, cx| this.press_move(window, cx))),
                            ),
                    ),
            )
    }
}

/// The `move` script command: as Move to ▸ a runner, by its name or id.
#[cfg(feature = "automation")]
pub(crate) fn automation(app: &Entity<Threadbear>, cmd: &Value, window: &mut Window, cx: &mut App) -> Result<Value, String> {
    let thread_id = match cmd["id"].as_str() {
        Some(id) => app.read(cx).thread_id_from(id)?,
        None => app.read(cx).selected.clone().ok_or("no thread open")?,
    };
    let want = cmd["runner"].as_str().unwrap_or("").to_lowercase();
    let runner_id_of_thread = app.read(cx).threads.get(&thread_id).and_then(|t| t.runner_id.clone());
    let runner = app.read(cx).move_targets(runner_id_of_thread.as_deref()).into_iter().find(|r| r.name.to_lowercase() == want || r.id == want).ok_or_else(|| format!("no runner {want} to move it to"))?;
    let request = MoveRequest { thread_id: thread_id.clone(), runner_id: runner.id, runner_name: runner.name };
    app.update(cx, |t, cx| t.open_move(request, window, cx));
    Ok(json!(format!("ok {thread_id}")))
}
