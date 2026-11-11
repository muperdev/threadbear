// The row under the composer's text: "+" (pictures and the folder), the
// account, model and permission chips and the context ring (ComposerControls.swift).
// The numbers are the Swift app's, at 100% zoom.
use threadbear_core::new_thread::ChipBadge;
use crate::app::Threadbear;
use crate::composer::Composer;
use crate::style::{self, size};
use gpui_kit::assets::IconName;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Selectable, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::json;
use threadbear_core::composer::{effort_porridge, effort_title, ContextMeter};
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::logins::{permission_help, permission_title, provider_title, ChipSettings, PickerRow, RunnerLogins};

/// What a chip row needs to know, read when it's drawn.
pub(crate) struct ChipData {
    pub(crate) summary: Option<ThreadSummary>,
    pub(crate) logins: RunnerLogins,
    /// The new-thread page's box: these are what its settings start from.
    pub(crate) is_new: bool,
    /// Another runner's thread, or a new one for another runner.
    pub(crate) away: Option<AwayRunner>,
}

/// The other runner a box's thread (or new draft) is on (`ComposerControls`).
pub(crate) struct AwayRunner {
    pub(crate) name: String,
    pub(crate) icon: &'static str,
    /// "On bruce", or "On bruce, offline".
    pub(crate) place: String,
    /// Its logins have come (`remoteLogins`); until then just its name.
    pub(crate) logins_known: bool,
    pub(crate) cwd: String,
}

impl ChipData {
    fn settings(&self) -> Option<ChipSettings<'_>> {
        let thread = self.summary.as_ref()?;
        Some(ChipSettings {
            logins: &self.logins,
            account: thread.account_id.clone(),
            provider: if thread.provider.is_empty() { "claude".to_owned() } else { thread.provider.clone() },
            pool_id: thread.pool_id.clone(),
            model: thread.model.clone(),
            model_in_use: thread.model_in_use.clone(),
            effort: thread.effort.clone(),
            effort_in_use: thread.effort_in_use.clone(),
            permission_mode: if thread.permission_mode.is_empty() { "full".to_owned() } else { thread.permission_mode.clone() },
            cwd: thread.cwd.clone(),
            is_new: self.is_new,
        })
    }
}

impl Composer {
    /// The chips for the box's thread: `app.threads` (with `update_open`'s
    /// guess) first, as it's what a click just changed, then the pane's copy.
    /// For the new-thread page's box, what the draft is set to.
    pub(crate) fn chip_data(&self, cx: &App) -> ChipData {
        let Some(app) = self.app.upgrade() else { return ChipData { summary: None, logins: RunnerLogins::default(), is_new: self.thread_id.is_none(), away: None } };
        let t = app.read(cx);
        if self.thread_id.is_none() {
            let away = t.new_thread.picks.runner.clone().map(|runner| AwayRunner {
                name: t.runner_name_of(&runner),
                icon: t.runner_icon_symbol(Some(&runner)),
                place: t.runner_place_of(&runner),
                logins_known: t.remote.logins.contains_key(&runner),
                cwd: t.draft_summary().cwd,
            });
            return ChipData { summary: Some(t.draft_summary()), logins: t.draft_logins(), is_new: true, away };
        }
        let summary = self.thread_id.as_ref().and_then(|id| t.threads.get(id).cloned()).or_else(|| self.pane.upgrade().map(|p| p.read(cx).summary.clone()));
        let (logins, away) = match summary.as_ref().and_then(|s| s.runner_id.clone()) {
            None => (t.logins.clone(), None),
            // Another runner's thread: its Mac's logins, once they've come.
            Some(runner) => {
                let known = t.remote.logins.get(&runner).cloned();
                let away = AwayRunner {
                    name: t.runner_name_of(&runner),
                    icon: t.runner_icon_symbol(Some(&runner)),
                    place: t.runner_place_of(&runner),
                    logins_known: known.is_some(),
                    cwd: summary.as_ref().map_or_else(String::new, |s| s.cwd.clone()),
                };
                (known.unwrap_or_default(), Some(away))
            }
        };
        ChipData { summary, logins, is_new: false, away }
    }

    /// `threads.update` for this box's thread, shown at once (`update_open`);
    /// on the new-thread page's box, the draft's settings instead.
    fn change_thread(&self, change: impl FnOnce(&mut ThreadSummary), params: serde_json::Value, cx: &mut App) {
        let is_new = self.thread_id.is_none();
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| {
                if is_new {
                    app.change_new_draft(&params, cx);
                } else {
                    let _ = app.update_open(change, params, cx);
                }
            });
        }
    }

    pub(crate) fn set_permission(&mut self, mode: &'static str, cx: &mut App) {
        self.change_thread(move |t| t.permission_mode = mode.to_owned(), json!({ "permissionMode": mode }), cx);
    }

    fn set_model(&self, model: Option<String>, cx: &mut App) {
        let local = model.clone();
        self.change_thread(move |t| t.model = local, json!({ "model": model }), cx);
    }

    fn set_effort(&self, level: String, cx: &mut App) {
        let local = level.clone();
        self.change_thread(move |t| t.effort = Some(local), json!({ "effort": level }), cx);
    }

    fn set_cwd(&self, folder: String, cx: &mut App) {
        let local = folder.clone();
        self.change_thread(move |t| t.cwd = local, json!({ "cwd": folder }), cx);
    }

    fn set_account(&self, account: String, cx: &mut App) {
        let local = account.clone();
        self.change_thread(move |t| t.account_id = local, json!({ "accountId": account }), cx);
    }

    /// A pick from a provider column or pool that isn't the thread's own
    /// (ThreadSettings.pick): the model moves with it. Without pools a
    /// provider's column moves the thread to that provider's account.
    fn pick_model(&self, row: &PickerRow, column: &str, data: &ChipData, cx: &mut App) {
        let Some(settings) = data.settings() else { return };
        let model = (!row.is_default).then(|| row.id.clone());
        if settings.uses_pools() {
            if settings.pool().is_some_and(|p| p.id == column) || column == PINNED_COLUMN {
                return self.set_model(model, cx);
            }
            // On a new thread the draft moves to that pool and its provider
            // (AppModel.choosePool); the model is remembered under it.
            if data.is_new {
                let provider = data.logins.pool(column).and_then(|p| p.account_ids.iter().find_map(|id| data.logins.account(id))).map(|a| a.provider_id().to_owned());
                return self.change_thread(|_| {}, json!({ "poolId": column, "provider": provider, "model": model }), cx);
            }
            // Picks the pool, and in it the model.
            let (pool, guess_pool, guess_model) = (column.to_owned(), column.to_owned(), model.clone());
            self.change_thread(
                move |t| {
                    t.pool_id = Some(guess_pool);
                    t.model = guess_model;
                },
                json!({ "poolId": pool, "model": model }),
                cx,
            );
            return;
        }
        let provider = settings.provider.clone();
        if column == provider {
            return self.set_model(model, cx);
        }
        if let Some(account) = data.logins.accounts.iter().find(|a| a.provider_id() == column) {
            let (id, guess_id, guess_provider, guess_model) = (account.id.clone(), account.id.clone(), column.to_owned(), model.clone());
            self.change_thread(
                move |t| {
                    t.account_id = guess_id;
                    t.provider = guess_provider;
                    t.model = guess_model;
                },
                json!({ "accountId": id, "model": model }),
                cx,
            );
        }
    }

    /// The folders "+" > Folder offers (ThreadSettings.recentFolders): this
    /// thread's, the default, then the ones most recently used, eight at most.
    /// Worked out when the menu opens, not at every render: it looks at every
    /// thread, and the box is drawn on every frame of a scroll.
    fn recent_folders(app: &WeakEntity<Threadbear>, current: &str, default_folder: &str, cx: &App) -> Vec<String> {
        let mut folders: Vec<String> = vec![current.to_owned(), default_folder.to_owned()];
        if let Some(app) = app.upgrade() {
            let mut threads: Vec<&ThreadSummary> = app.read(cx).threads.values().collect();
            threads.sort_by_cached_key(|t| std::cmp::Reverse(t.activity_ms()));
            folders.extend(threads.into_iter().map(|t| t.cwd.clone()));
        }
        let mut out = Vec::new();
        for folder in folders {
            if !folder.is_empty() && !out.contains(&folder) {
                out.push(folder);
            }
            if out.len() == 8 {
                break;
            }
        }
        out
    }

    /// Picks files for "+" > Attach images… (ComposerView.chooseImages).
    fn attach_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Add".into()) });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                this.update_in(cx, |this, window, cx| this.take_paths(paths, window, cx)).ok();
            }
        })
        .detach();
    }

    /// "+" > Folder > Choose… (ThreadSettings.chooseFolder).
    fn choose_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Choose".into()) });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = paths.await {
                if let Some(path) = paths.pop() {
                    this.update_in(cx, |this, _, cx| this.set_cwd(path.to_string_lossy().into_owned(), cx)).ok();
                }
            }
        })
        .detach();
    }

    // ---- The row ----

    pub(crate) fn render_controls(&mut self, send_button: AnyElement, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let data = self.chip_data(cx);
        let mut row = h_flex().w_full().h(px(size::SEND_BUTTON)).gap(px(4.));
        let composer = cx.entity();
        // What leads the row depends on where the thread is (controls(remote:)
        // and its neighbours in ComposerControls.swift): this Mac's has "+".
        match (&data.away, data.is_new) {
            // Another runner's thread before its logins have come: its Mac's
            // name where the chips would be, and send (remoteControls).
            (Some(away), false) if !away.logins_known => {
                return row.child(div().ml(px(-7.)).child(render_runner_chip(away, cx))).child(div().flex_1()).child(send_button).into_any_element();
            }
            // Its logins known: the machine's chip in place of "+".
            (Some(away), false) => row = row.child(div().ml(px(-7.)).child(render_runner_chip(away, cx))).child(chip_divider(cx)),
            // A new thread for another runner, before it's said what logins
            // it has: the machine chip, and how it's going (draftWaitingControls).
            (Some(away), true) if !away.logins_known => {
                let problem = self.app.upgrade().and_then(|a| a.read(cx).new_thread.runner_problem.clone());
                row = row.child(div().ml(px(-7.)).child(self.render_runs_on_chip(cx)));
                row = match problem {
                    Some(problem) => row.child(div().id("runner-problem").text_size(px(size::CHIP)).text_color(style::secondary_text(cx)).child(format!("Can't reach {}", away.name)).tooltip(move |window, cx| Tooltip::new(problem.clone()).build(window, cx))),
                    None => row.child(div().id("runner-waiting").child(gpui_kit::component::spinner::Spinner::new().color(style::quiet_text(cx)).with_size(px(14.))).tooltip({
                        let tip = format!("Connecting to {}", away.name);
                        move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                    })),
                };
                return row.child(div().flex_1()).child(send_button).into_any_element();
            }
            // A new thread for another runner: no pictures or folders from
            // here (paths on this Mac), so the machine chip leads.
            (Some(_), true) => row = row.child(div().ml(px(-7.)).child(self.render_runs_on_chip(cx))),
            // This Mac: "+", then the machine chip on a new thread.
            (None, _) => {
                row = row.child(div().ml(px(-7.)).child(self.render_plus(&data, window, cx)));
                if data.is_new && self.app.upgrade().is_some_and(|a| !a.read(cx).remote.runners.is_empty()) {
                    row = row.child(self.render_runs_on_chip(cx));
                }
            }
        }
        if let Some(settings) = data.settings() {
            // With pools the runner picks the login, so there's no account chip.
            if !settings.uses_pools() {
                row = row.child(self.render_account_chip(&data, cx)).child(chip_divider(cx));
            }
            row = row.child(self.render_model_chip(&data, window, cx)).child(chip_divider(cx));
            row = row.child(self.render_permission_chip(settings.permission_mode.clone(), &composer, cx));
        }
        row = row.child(div().flex_1());
        if let Some(meter) = data.summary.as_ref().and_then(|t| ContextMeter::new(t.context_tokens, t.context_window)) {
            row = row.child(self.render_context_ring(meter, &data));
        }
        row.child(send_button).into_any_element()
    }

    /// Opens a small menu: "Attach images…" and the folder (PlusButton).
    fn render_plus(&self, data: &ChipData, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let composer = cx.entity().downgrade();
        let current = data.summary.as_ref().map(|t| t.cwd.clone()).unwrap_or_default();
        let default_folder = data.logins.defaults.cwd.clone();
        let app = self.app.clone();
        Popover::new("composer-plus")
            .anchor(Anchor::BottomLeft)
            .trigger(ChipTrigger { badge: None, id: "composer-plus-button".into(), kind: TriggerKind::Plus, title: SharedString::default(), icon: None, tip: "Attach images, change the folder or schedule the message".into(), selected: false })
            .content(move |_, window, cx| {
                let folders = Self::recent_folders(&app, &current, &default_folder, cx);
                let popover = cx.entity();
                let c1 = composer.clone();
                let (popover2, c2) = (popover.clone(), composer.clone());
                v_flex()
                    .w(px(260.))
                    .p(px(4.))
                    .child(menu_row("attach", Some(IconName::Image), "Attach images…", None, false, window, cx, move |window, cx| {
                        popover.update(cx, |s, cx| s.dismiss(window, cx));
                        if let Some(c) = c1.upgrade() {
                            c.update(cx, |c, cx| c.attach_images(window, cx));
                        }
                    }))
                    .child(section_label("Folder", cx))
                    .children(folders.iter().enumerate().map(|(i, folder)| {
                        let (popover, c, path) = (popover2.clone(), c2.clone(), folder.clone());
                        let subtitle = (*folder == default_folder).then(|| "Default".to_owned());
                        menu_row(("folder", i), Some(IconName::Folder), &abbreviate_home(folder), subtitle.as_deref(), *folder == current, window, cx, move |window, cx| {
                            popover.update(cx, |s, cx| s.dismiss(window, cx));
                            if let Some(c) = c.upgrade() {
                                c.update(cx, |c, cx| c.set_cwd(path.clone(), cx));
                            }
                        })
                    }))
                    .child({
                        let (popover, c) = (popover2.clone(), c2.clone());
                        menu_row("folder-choose", Some(IconName::FolderOpen), "Choose…", None, false, window, cx, move |window, cx| {
                            popover.update(cx, |s, cx| s.dismiss(window, cx));
                            if let Some(c) = c.upgrade() {
                                c.update(cx, |c, cx| c.choose_folder(window, cx));
                            }
                        })
                    })
            })
    }

    fn render_permission_chip(&self, mode: String, composer: &Entity<Composer>, cx: &mut Context<Self>) -> impl IntoElement {
        let title = permission_title(&mode);
        let help = permission_help(&mode);
        let icon = permission_icon(&mode);
        let composer = composer.downgrade();
        let provider = self.chip_data(cx).summary.map(|t| t.provider).unwrap_or_default();
        Popover::new("composer-permission")
            .anchor(Anchor::BottomLeft)
            .trigger(ChipTrigger { badge: None, id: "composer-permission-chip".into(), kind: TriggerKind::Chip, title: title.into(), icon: Some(icon), tip: help.into(), selected: false })
            .content(move |_, window, cx| {
                let popover = cx.entity();
                let modes: &[&str] = if threadbear_core::logins::has_auto_mode(&provider) || mode == "auto" { &["full", "auto", "ask", "plan"] } else { &["full", "ask", "plan"] };
                v_flex().w(px(320.)).p(px(4.)).children(modes.iter().map(|m| {
                    let m = *m;
                    let (popover, composer) = (popover.clone(), composer.clone());
                    menu_row(m, Some(permission_icon(m)), permission_title(m), Some(permission_help(m)), m == mode, window, cx, move |window, cx| {
                        popover.update(cx, |s, cx| s.dismiss(window, cx));
                        if let Some(c) = composer.upgrade() {
                            c.update(cx, |c, cx| c.set_permission(m, cx));
                        }
                    })
                }))
            })
    }

    fn render_account_chip(&self, data: &ChipData, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(settings) = data.settings() else { return div().into_any_element() };
        let title = settings.account_chip_title();
        let tip = settings.account_chip_help();
        let current = settings.account.clone();
        let accounts = data.logins.accounts.clone();
        let composer = cx.entity().downgrade();
        Popover::new("composer-account")
            .anchor(Anchor::BottomLeft)
            .trigger(ChipTrigger { badge: settings.account_badge(), id: "composer-account-chip".into(), kind: TriggerKind::Chip, title: title.into(), icon: Some(IconName::CircleUser), tip: tip.into(), selected: false })
            .content(move |_, window, cx| {
                let popover = cx.entity();
                v_flex().w(px(320.)).p(px(4.)).children(accounts.iter().enumerate().map(|(i, account)| {
                    let (popover, composer, id) = (popover.clone(), composer.clone(), account.id.clone());
                    menu_row(("account", i), None, &account.name(), account.note.as_deref(), account.id == current, window, cx, move |window, cx| {
                        popover.update(cx, |s, cx| s.dismiss(window, cx));
                        if let Some(c) = composer.upgrade() {
                            c.update(cx, |c, cx| c.set_account(id.clone(), cx));
                        }
                    })
                }))
            })
            .into_any_element()
    }

    fn render_model_chip(&mut self, data: &ChipData, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(settings) = data.settings() else { return div().into_any_element() };
        let title = settings.model_chip_title();
        let tip = settings.model_chip_help();
        // The provider's mark leads the chip when there's no pool icon to show.
        let icon = if settings.uses_pools() && settings.pool().is_none() { IconName::CircleUser } else { IconName::Sparkles };
        let picker = self.picker_state(window, cx);
        Popover::new("composer-model")
            .anchor(Anchor::BottomLeft)
            .trigger(ChipTrigger { badge: settings.model_chip_badge(), id: "composer-model-chip".into(), kind: TriggerKind::Chip, title: title.into(), icon: Some(icon), tip: tip.into(), selected: false })
            .content(move |_, _, _| picker.clone())
            .into_any_element()
    }

    fn render_context_ring(&self, meter: ContextMeter, data: &ChipData) -> impl IntoElement {
        let model_line = data.settings().map(|s| threadbear_core::logins::chip_title(s.effective_model().as_deref(), s.effective_effort().as_deref(), s.choices())).unwrap_or_default();
        let login_line = data.summary.as_ref().and_then(|t| data.logins.account(&t.account_id).map(|a| a.name())).unwrap_or_default();
        let provider = data.summary.as_ref().map(|t| t.provider.clone()).unwrap_or_default();
        let cost = data.summary.as_ref().and_then(|t| t.cost_usd).filter(|c| *c > 0.).map(|c| format!("${c:.2}"));
        let tip = meter.tip();
        Popover::new("composer-context")
            .anchor(Anchor::BottomRight)
            .trigger(ChipTrigger { badge: None, id: "composer-context-ring".into(), kind: TriggerKind::Ring(meter), title: SharedString::default(), icon: None, tip: tip.into(), selected: false })
            .content(move |_, _, cx| {
                let fraction = meter.fraction() as f32;
                let note = format!("{} compacts it by itself when it's nearly full.", if provider == "codex" { "Codex" } else { "Claude" });
                let row = |name: &'static str, value: String, cx: &mut Context<PopoverState>| {
                    h_flex().justify_between().gap(px(12.)).text_size(px(12.)).child(div().text_color(cx.theme().muted_foreground).child(name)).child(div().truncate().child(value))
                };
                v_flex()
                    .w(px(260.))
                    .p(px(14.))
                    .gap(px(6.))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Context"))
                    .child(Progress::new("context-fill").value(fraction * 100.).h(px(4.)))
                    .child(h_flex().justify_between().text_size(px(12.)).child(meter.amount()).child(div().text_color(cx.theme().muted_foreground).child(format!("{}%", meter.percent()))))
                    .child(div().text_size(px(11.)).text_color(cx.theme().muted_foreground).child(note.clone()))
                    .child(div().h(px(1.)).bg(style::hairline(cx)))
                    .child(row("Model", model_line.clone(), cx))
                    .child(row("Login", login_line.clone(), cx))
                    .when_some(cost.clone(), |s, cost| s.child(row("Cost so far", cost, cx)))
            })
    }

    /// The machine chip on the new-thread box (`runsOnChip`): which Mac the
    /// thread will run on, its menu opening under the pointer.
    fn render_runs_on_chip(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (icon, tip) = match self.app.upgrade().and_then(|a| a.read(cx).new_thread.picks.runner.clone()) {
            Some(runner) => {
                let app = self.app.upgrade();
                let t = app.as_ref().expect("the app").read(cx);
                let name = t.runner_name_of(&runner);
                let tip = if t.remote.runners.iter().any(|r| r.id == runner && !r.online) { format!("Runs on {name}, offline") } else { format!("Runs on {name}") };
                (t.runner_icon_symbol(Some(&runner)), tip)
            }
            None => (self.app.upgrade().map_or("desktopcomputer", |a| a.read(cx).self_runner_symbol()), "Runs on this Mac".to_owned()),
        };
        let app = self.app.clone();
        h_flex()
            .gap(px(4.))
            .child(
                div()
                    .id("composer-runner-chip")
                    .test_support()
                    .aria_label(tip.clone())
                    .flex()
                    .items_center()
                    .h(px(26.))
                    .px(px(7.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(|s| s.bg(style::chip_hover_fill(cx)))
                    .child(crate::icons::symbol(icon).with_size(px(size::CHIP - 1.)).text_color(style::faint_icon(cx)))
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .on_click(move |ev: &ClickEvent, window, cx| {
                        if let Some(app) = app.upgrade() {
                            let at = ev.position();
                            app.update(cx, |app, cx| app.open_runner_menu(point(at.x, at.y - px(12.)), window, cx));
                        }
                    }),
            )
            .child(chip_divider(cx))
    }

    /// The model picker's state lives on the composer so its search text and
    /// column stay put while it's open.
    fn picker_state(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<ModelPickerView> {
        if let Some(picker) = &self.model_picker {
            return picker.clone();
        }
        let composer = cx.entity().downgrade();
        let picker = cx.new(|cx| ModelPickerView::new(composer, window, cx));
        self.model_picker = Some(picker.clone());
        picker
    }
}

const PINNED_COLUMN: &str = "@pinned";

/// A quiet menu chip: icon, short label and a small chevron in secondary
/// colours (ComposerChip).
enum TriggerKind {
    Chip,
    Plus,
    Ring(ContextMeter),
}

#[derive(IntoElement)]
struct ChipTrigger {
    id: ElementId,
    kind: TriggerKind,
    title: SharedString,
    icon: Option<IconName>,
    tip: SharedString,
    selected: bool,
    /// After the title: "· 12%" in orange, "Limit reached" in red
    /// (`ComposerChip.badge`). Colour only here, where it means the login,
    /// or every login in the pool, is running out.
    badge: Option<ChipBadge>,
}

impl Selectable for ChipTrigger {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for ChipTrigger {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tip = self.tip.clone();
        let base = div().id(self.id).flex().items_center().cursor_pointer().tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
        match self.kind {
            TriggerKind::Plus => base
                .justify_center()
                .size(px(26.))
                .rounded_full()
                .hover(|s| s.bg(style::hover_fill(cx)))
                .child(Icon::from(IconName::Plus).with_size(px(15.)).text_color(style::quiet_text(cx)))
                .into_any_element(),
            TriggerKind::Ring(meter) => base.justify_center().size(px(26.)).rounded_full().hover(|s| s.bg(style::hover_fill(cx))).child(context_ring(meter, cx)).into_any_element(),
            TriggerKind::Chip => base
                .h(px(26.))
                .px(px(7.))
                .gap(px(4.))
                .rounded(px(7.))
                .hover(|s| s.bg(style::chip_hover_fill(cx)))
                .when_some(self.icon, |s, icon| s.child(Icon::from(icon).with_size(px(size::CHIP - 1.)).text_color(style::faint_icon(cx))))
                .when(!self.title.is_empty(), |s| s.child(div().text_size(px(size::CHIP)).text_color(style::quiet_text(cx)).whitespace_nowrap().child(self.title.clone())))
                .when_some(self.badge.clone(), |s, badge| {
                    let color = if badge == ChipBadge::Blocked { cx.theme().danger } else { style::needs_you(cx) };
                    s.child(div().text_size(px(size::CHIP)).text_color(color).whitespace_nowrap().child(badge_words(&badge)))
                })
                .when(!self.title.is_empty(), |s| s.child(Icon::from(IconName::ChevronDown).with_size(px(9.)).text_color(style::faint_icon(cx))))
                .into_any_element(),
        }
    }
}

fn chip_divider(cx: &App) -> impl IntoElement {
    div().w(px(1.)).h(px(12.)).bg(style::chip_divider(cx))
}

/// The machine chip on another runner's thread (`runnerChip`): its icon and
/// name at the front of the row, in place of "+" and the folder.
fn render_runner_chip(away: &AwayRunner, cx: &App) -> impl IntoElement {
    let tip = format!("{}, in {}", away.place, abbreviate_home(&away.cwd));
    h_flex()
        .id("composer-runner-name")
        .gap(px(4.))
        .h(px(26.))
        .px(px(7.))
        .child(crate::icons::symbol(away.icon).with_size(px(size::CHIP - 1.)).text_color(style::faint_icon(cx)))
        .child(div().text_size(px(size::CHIP)).text_color(style::quiet_text(cx)).whitespace_nowrap().child(away.name.clone()))
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
}

/// The ring for how full the context is (ContextRing): grey, orange near full,
/// a sliver even when nearly empty, so it reads as a meter.
fn context_ring(meter: ContextMeter, cx: &App) -> impl IntoElement {
    let track = style::chip_divider(cx);
    let color = if meter.is_nearly_full() { hsla(0.09, 0.9, 0.55, 1.) } else { style::quiet_text(cx) };
    let fraction = meter.fraction().max(0.03) as f32;
    canvas(|_, _, _| (), move |bounds, _, window, _| {
        let side = px(15.);
        let center = point(bounds.origin.x + bounds.size.width / 2., bounds.origin.y + bounds.size.height / 2.);
        let radius = side / 2. - px(1.25);
        let line = px(2.5);
        // A stroked circle, then the part of it that's used from the top,
        // clockwise. Short straight pieces, which at this size is a circle.
        let draw = |window: &mut Window, from: f32, to: f32, color: Hsla| {
            let mut builder = PathBuilder::stroke(line);
            let steps = ((to - from) * 96.).ceil().max(2.) as usize;
            for step in 0..=steps {
                let t = from + (to - from) * step as f32 / steps as f32;
                let angle = t * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                let p = point(center.x + radius * angle.cos(), center.y + radius * angle.sin());
                if step == 0 {
                    builder.move_to(p);
                } else {
                    builder.line_to(p);
                }
            }
            if let Ok(path) = builder.build() {
                window.paint_path(path, color);
            }
        };
        draw(window, 0., 1., track);
        draw(window, 0., fraction, color);
    })
    .size(px(26.))
}

fn permission_icon(mode: &str) -> IconName {
    match mode {
        "full" => IconName::LockOpen,
        "ask" => IconName::Lock,
        "auto" => IconName::ShieldCheck,
        "plan" => IconName::ClipboardList,
        _ => IconName::CircleQuestionMark,
    }
}

fn abbreviate_home(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path.starts_with(&home) => format!("~{}", &path[home.len()..]),
        _ => path.to_owned(),
    }
}

fn section_label(title: &'static str, cx: &App) -> impl IntoElement {
    div().px(px(10.)).pt(px(8.)).pb(px(2.)).text_size(px(11.)).font_weight(FontWeight::MEDIUM).text_color(style::faint_text(cx)).child(title)
}

/// A row in one of the chips' menus: an icon, a title, a line under it and a
/// tick when it's the current one (ActionMenuItem).
fn menu_row(id: impl Into<ElementId>, icon: Option<IconName>, title: &str, subtitle: Option<&str>, checked: bool, window: &mut Window, cx: &mut App, action: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    let _ = window;
    h_flex()
        .id(id.into())
        .gap(px(8.))
        .px(px(10.))
        .py(px(6.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(style::hover_fill(cx)))
        .on_click(move |_, window, cx| action(window, cx))
        .when_some(icon, |s, icon| s.child(Icon::from(icon).with_size(px(14.)).text_color(style::quiet_text(cx))))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).truncate().child(title.to_owned()))
                .when_some(subtitle, |s, subtitle| s.child(div().text_size(px(11.)).text_color(cx.theme().muted_foreground).line_clamp(2).child(subtitle.to_owned()))),
        )
        .when(checked, |s| s.child(Icon::from(IconName::Check).with_size(px(14.)).text_color(cx.theme().foreground)))
}

/// The model picker's popover (ModelPickerView): search, rail, the column's
/// models and the effort levels, on the `Composer` it picks for.
pub(crate) struct ModelPickerView {
    composer: WeakEntity<Composer>,
    search: Entity<InputState>,
    column: Option<String>,
    _subs: Vec<Subscription>,
}

impl ModelPickerView {
    fn new(composer: WeakEntity<Composer>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search models"));
        let sub = cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify());
        ModelPickerView { composer, search, column: None, _subs: vec![sub] }
    }
}

impl Render for ModelPickerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(composer) = self.composer.upgrade() else { return div().into_any_element() };
        let data = composer.read(cx).chip_data(cx);
        let Some(settings) = data.settings() else { return div().into_any_element() };
        let pools = settings.uses_pools();
        let columns: Vec<(String, String, Option<String>)> = if pools {
            data.logins.pools.iter().map(|p| (p.id.clone(), p.title.clone(), None)).collect()
        } else {
            let mut seen: Vec<String> = Vec::new();
            for a in &data.logins.accounts {
                if !seen.iter().any(|p| p == a.provider_id()) {
                    seen.push(a.provider_id().to_owned());
                }
            }
            seen.into_iter().map(|p| (p.clone(), provider_title(&p), None)).collect()
        };
        let own_column = if pools { settings.pool().map_or(PINNED_COLUMN.to_owned(), |p| p.id.clone()) } else { settings.provider.clone() };
        let column = self.column.clone().unwrap_or(own_column);
        let query = self.search.read(cx).value().to_string();
        let provider = if pools {
            data.logins.pool(&column).and_then(|p| p.account_ids.iter().find_map(|id| data.logins.account(id))).map_or(settings.provider.clone(), |a| a.provider_id().to_owned())
        } else {
            column.clone()
        };
        let rows = settings.rows(&provider, &query);
        let (main, older): (Vec<_>, Vec<_>) = rows.into_iter().partition(|r| !r.is_older);
        let levels = settings.effort_levels();
        let chosen = settings.effective_effort();
        let width = if pools { 440. } else { 380. };
        let rail_width = if pools { 96. } else { 44. };
        let this = cx.entity().downgrade();
        let rail = v_flex().w(px(rail_width)).flex_shrink_0().py(px(6.)).px(px(6.)).gap(px(4.)).bg(style::chip_fill(cx)).children(columns.into_iter().enumerate().map(|(i, (id, title, _))| {
            let active = id == column;
            let this = this.clone();
            div()
                .id(("picker-rail", i))
                .px(px(6.))
                .py(px(8.))
                .rounded(px(7.))
                .text_size(px(12.))
                .text_center()
                .truncate()
                .cursor_pointer()
                .text_color(if active { cx.theme().foreground } else { cx.theme().muted_foreground })
                .when(active, |s| s.bg(style::hover_fill(cx)))
                .hover(|s| s.bg(style::hover_fill(cx)))
                .child(title)
                .on_click(move |_, _, cx| {
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| {
                            this.column = Some(id.clone());
                            cx.notify();
                        });
                    }
                })
        }));
        let row_view = |row: PickerRow, cx: &App| {
            let composer = self.composer.clone();
            let (data, column, picked) = (self.chip_snapshot(cx), column.clone(), row.clone());
            model_row(row, cx, move |window, cx| {
                if let Some(c) = composer.upgrade() {
                    c.update(cx, |c, cx| c.pick_model(&picked, &column, &data, cx));
                }
                window.dispatch_action(Box::new(gpui_kit::base::actions::Cancel), cx);
            })
        };
        let mut list = v_flex().p(px(6.)).gap(px(1.));
        if main.is_empty() && older.is_empty() {
            list = list.child(div().p(px(10.)).text_size(px(13.)).text_color(cx.theme().muted_foreground).child(if query.is_empty() { "No models listed yet" } else { "Nothing matches" }));
        }
        list = list.children(main.into_iter().map(|r| row_view(r, cx)));
        if !older.is_empty() {
            list = list.child(div().px(px(10.)).pt(px(8.)).pb(px(2.)).text_size(px(11.)).font_weight(FontWeight::MEDIUM).text_color(style::faint_text(cx)).child("Older")).children(older.into_iter().map(|r| row_view(r, cx)));
        }
        let composer_for_effort = self.composer.clone();
        let effort = (!levels.is_empty()).then(|| {
            h_flex().m(px(12.)).p(px(2.)).gap(px(2.)).rounded(px(7.)).bg(style::chip_fill(cx)).children(levels.into_iter().map(|level| {
                let active = chosen.as_deref() == Some(level.as_str());
                let title = effort_title(&level);
                let tip = effort_porridge(&level).unwrap_or(&title).to_owned();
                let (composer, level2) = (composer_for_effort.clone(), level.clone());
                div()
                    .id(SharedString::from(format!("effort-{level}")))
                    .flex_1()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .text_size(px(11.))
                    .when(active, |s| s.bg(cx.theme().background).font_weight(FontWeight::SEMIBOLD))
                    .text_color(if active { cx.theme().foreground } else { cx.theme().muted_foreground })
                    .hover(|s| s.bg(style::hover_fill(cx)))
                    .child(title)
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .on_click(move |_, _, cx| {
                        if let Some(c) = composer.upgrade() {
                            c.update(cx, |c, cx| c.set_effort(level2.clone(), cx));
                        }
                    })
            }))
        });
        let right = v_flex()
            .flex_1()
            .min_w_0()
            .child(h_flex().h(px(36.)).px(px(12.)).gap(px(6.)).border_b_1().border_color(style::hairline(cx)).child(Icon::from(IconName::Search).with_size(px(12.)).text_color(style::faint_text(cx))).child(div().flex_1().child(Input::new(&self.search).appearance(false).bordered(false).text_size(px(13.)))))
            .child(div().id("picker-list").max_h(px(320.)).overflow_y_scroll().child(list))
            .children(effort.map(|e| div().border_t_1().border_color(style::hairline(cx)).child(e)));
        h_flex().w(px(width)).items_start().child(rail).child(div().w(px(1.)).h_full().bg(style::hairline(cx))).child(right).into_any_element()
    }
}

impl ModelPickerView {
    fn chip_snapshot(&self, cx: &App) -> ChipData {
        self.composer.upgrade().map(|c| c.read(cx).chip_data(cx)).unwrap_or(ChipData { summary: None, logins: RunnerLogins::default(), is_new: false, away: None })
    }
}

fn model_row(row: PickerRow, cx: &App, action: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    let selected = row.is_selected;
    h_flex()
        .id(SharedString::from(format!("model-{}", row.id)))
        .gap(px(10.))
        .pl(px(6.))
        .pr(px(10.))
        .py(px(5.))
        .rounded(px(8.))
        .cursor_pointer()
        .when(selected, |s| s.bg(style::hover_fill(cx)))
        .hover(|s| s.bg(style::hover_fill(cx)))
        .on_click(move |_, window, cx| action(window, cx))
        .child(div().size(px(26.)).rounded(px(7.)).bg(style::chip_fill(cx)).flex().items_center().justify_center().when(row.is_older, |s| s.opacity(0.6)).child(Icon::from(IconName::Sparkles).with_size(px(15.)).text_color(if selected { cx.theme().foreground } else { cx.theme().muted_foreground })))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(row.title.clone()))
                .when_some(row.subtitle.clone().filter(|s| !s.is_empty()), |s, sub| s.child(div().text_size(px(11.)).text_color(cx.theme().muted_foreground).truncate().child(sub))),
        )
        .when(row.is_default, |s| s.child(Icon::from(IconName::Star).with_size(px(9.)).text_color(style::faint_text(cx))))
}

/// The picker as text for the automation's `composer-menu` (ThreadSettings
/// pickerLines): `[x]` on the chosen row, `*` on the default, the description
/// after a dash, older rows indented, then the effort levels.
#[cfg(feature = "automation")]
pub(crate) fn picker_lines(data: &ChipData) -> Vec<String> {
    let Some(settings) = data.settings() else { return Vec::new() };
    let mut lines = Vec::new();
    let add_rows = |rows: Vec<PickerRow>, lines: &mut Vec<String>| {
        for row in rows {
            let mut line = format!("{}{}{}", if row.is_older { "    " } else { "" }, if row.is_selected { "[x] " } else { "[ ] " }, row.title);
            if row.is_default {
                line.push_str(" *");
            }
            if let Some(sub) = row.subtitle.filter(|s| !s.is_empty()) {
                line.push_str(&format!(" - {sub}"));
            }
            lines.push(line);
        }
    };
    if settings.uses_pools() {
        for pool in &data.logins.pools {
            let is_own = settings.pool().is_some_and(|p| p.id == pool.id);
            lines.push(format!("# {}{}", if is_own { "[x] " } else { "[ ] " }, pool.title));
            let provider = pool.account_ids.iter().find_map(|id| data.logins.account(id)).map_or("claude".to_owned(), |a| a.provider_id().to_owned());
            add_rows(settings.rows(&provider, ""), &mut lines);
        }
    } else {
        let mut seen: Vec<String> = Vec::new();
        for a in &data.logins.accounts {
            if !seen.iter().any(|p| p == a.provider_id()) {
                seen.push(a.provider_id().to_owned());
            }
        }
        for provider in seen {
            lines.push(format!("# {}", provider_title(&provider)));
            add_rows(settings.rows(&provider, ""), &mut lines);
        }
    }
    lines.push("# Effort".to_owned());
    let chosen = settings.effective_effort();
    for level in settings.effort_levels() {
        let mut line = format!("{}{}", if chosen.as_deref() == Some(level.as_str()) { "[x] " } else { "[ ] " }, effort_title(&level));
        if let Some(porridge) = effort_porridge(&level) {
            line.push_str(&format!(" - {porridge}"));
        }
        lines.push(line);
    }
    lines
}

/// A chip's badge as it reads after its title (`ComposerChip`).
pub(crate) fn badge_words(badge: &ChipBadge) -> String {
    match badge {
        ChipBadge::Low(percent) => format!("\u{00B7} {percent}"),
        ChipBadge::Blocked => "Limit reached".to_owned(),
    }
}

/// The model chip's words, for the dump's `composer.chip`.
#[cfg(feature = "automation")]
pub(crate) fn model_chip_text(data: &ChipData) -> String {
    data.settings().map(|s| s.model_chip_title()).unwrap_or_default()
}

/// The model chip's badge as it reads, for the dump's `composer.chipBadge`.
#[cfg(feature = "automation")]
pub(crate) fn model_chip_badge_text(data: &ChipData) -> Option<String> {
    data.settings().and_then(|s| s.model_chip_badge()).map(|b| badge_words(&b))
}

/// The modes the permission menu offers, in its order.
#[cfg(feature = "automation")]
pub(crate) fn permission_modes(summary: &ThreadSummary) -> Vec<&'static str> {
    if threadbear_core::logins::has_auto_mode(&summary.provider) || summary.permission_mode == "auto" {
        vec!["full", "auto", "ask", "plan"]
    } else {
        vec!["full", "ask", "plan"]
    }
}

/// The permission menu as text (Automation's describe(menu)).
#[cfg(feature = "automation")]
pub(crate) fn permission_lines(summary: &ThreadSummary) -> Vec<String> {
    let current = if summary.permission_mode.is_empty() { "full" } else { summary.permission_mode.as_str() };
    permission_modes(summary).into_iter().map(|m| format!("{}{} - {}", if m == current { "[x] " } else { "[ ] " }, permission_title(m), permission_help(m))).collect()
}

/// Picks a model row or effort level by its title, as a click would
/// (ThreadSettings.pickerClick).
#[cfg(feature = "automation")]
pub(crate) fn picker_click(composer: &Entity<Composer>, title: &str, cx: &mut App) -> Result<(), String> {
    let data = composer.read(cx).chip_data(cx);
    let settings = data.settings().ok_or("no thread")?;
    let columns: Vec<(String, String)> = if settings.uses_pools() {
        let mut order: Vec<&threadbear_core::logins::Pool> = data.logins.pools.iter().collect();
        if let Some(own) = settings.pool() {
            order.sort_by_key(|p| p.id != own.id);
        }
        order.iter().map(|p| (p.id.clone(), p.account_ids.iter().find_map(|id| data.logins.account(id)).map_or("claude".to_owned(), |a| a.provider_id().to_owned()))).collect()
    } else {
        let mut providers: Vec<String> = Vec::new();
        for a in &data.logins.accounts {
            if !providers.iter().any(|p| p == a.provider_id()) {
                providers.push(a.provider_id().to_owned());
            }
        }
        providers.into_iter().map(|p| (p.clone(), p)).collect()
    };
    for (column, provider) in &columns {
        if let Some(row) = settings.rows(provider, "").into_iter().find(|r| r.title == title) {
            composer.update(cx, |c, cx| c.pick_model(&row, column, &data, cx));
            return Ok(());
        }
    }
    if let Some(level) = settings.effort_levels().into_iter().find(|l| effort_title(l) == title) {
        composer.update(cx, |c, cx| c.set_effort(level, cx));
        return Ok(());
    }
    Err(format!("no item {title}"))
}
