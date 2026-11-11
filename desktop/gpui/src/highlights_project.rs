// A project's heading in Highlights: its icon, and the ⓘ that says who put the
// thread there and why, with Keep, Take out and Open project, so a wrong
// guess of the librarian's is undone where it's seen (`ProjectWhyButton`).
use crate::highlights::{icon, Palette, SidePane};
use crate::{call, style};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// Shadows gpui's px, so sizes here follow the app's zoom (style.rs).
use crate::style::px;
use serde_json::{json, Value};
use std::path::PathBuf;
use threadbear_core::highlights::{can_keep, project_why, ProjectInfo, ProjectRow};
use threadbear_core::inbox::{Project, ThreadSummary};

/// The project's icon, as `ProjectIconView` draws it: a picture, else a
/// symbol, else its letter on a tile.
pub(crate) fn project_icon(project: &ProjectInfo, cx: &App) -> AnyElement {
    let picture = project.icon.as_ref().and_then(|i| i.get("image")).and_then(|i| i.get("path")).and_then(Value::as_str).map(PathBuf::from);
    if let Some(path) = picture {
        return img(path).size(px(14.)).rounded(px(3.)).into_any_element();
    }
    if let Some(name) = project.icon.as_ref().and_then(|i| i.get("symbol")).and_then(Value::as_str).filter(|s| crate::icons::try_symbol(s).is_some()) {
        return icon(name, 11., style::secondary_text(cx)).into_any_element();
    }
    let letter = threadbear_core::sidebar::project_letter(&project.title);
    div()
        .size(px(14.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .bg(style::primary_opacity(cx, 0.1))
        .text_size(px(9.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(style::secondary_text(cx))
        .child(letter)
        .into_any_element()
}

impl SidePane {
    /// The popover's words, the ⓘ's click and the scripted `project-why`.
    fn why_lines(&self, member: &Project) -> (Option<String>, String, bool) {
        let (who, reason) = project_why(member);
        (reason, who, can_keep(member))
    }
}

/// The ⓘ at the end of the project's heading.
pub(crate) fn why_button(row: &ProjectRow, p: Palette, cx: &mut Context<SidePane>) -> impl IntoElement {
    let project_id = row.project.id.clone();
    let tip = format!("Why it's in {}", row.project.title);
    let open_id = project_id.clone();
    div()
        .id(SharedString::from(format!("hl-why-{project_id}")))
        .size(px(14.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .child(icon("info.circle", 11., p.secondary))
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
            let position = ev.position();
            this.project_why = match &this.project_why {
                Some((id, _)) if *id == open_id => None,
                _ => Some((open_id.clone(), position)),
            };
            cx.notify();
        }))
}

impl SidePane {
    /// The ⓘ's popover, when one is open. Drawn over everything at the place
    /// it was asked for, as a menu is.
    pub(crate) fn why_popover(&mut self, thread: &ThreadSummary, p: Palette, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (project_id, position) = self.project_why.clone()?;
        let member = thread.projects.as_ref()?.iter().find(|m| m.id == project_id)?.clone();
        let project = self.projects.get(&project_id)?.clone();
        let (reason, who, keep) = self.why_lines(&member);
        let (bg, border) = (cx.theme().popover, p.card_border);
        let title = project.title.clone();
        let mut buttons = h_flex().gap(px(8.)).items_center();
        if keep {
            let (id, t) = (project_id.clone(), thread.id.clone());
            buttons = buttons.child(self.why_action("Keep", format!("Keep it in {title}, so the librarian never takes it out"), p, cx, move |this, cx| this.set_project_membership(t.clone(), vec![id.clone()], vec![], cx)));
        }
        let (id, t) = (project_id.clone(), thread.id.clone());
        buttons = buttons.child(self.why_action("Take out", format!("Take it out of {title}; the librarian won't put it back"), p, cx, move |this, cx| this.set_project_membership(t.clone(), vec![], vec![id.clone()], cx)));
        buttons = buttons.child(div().flex_1());
        // Project pages are a later slice (`panel.projectpage`); it's greyed until then.
        buttons = buttons.child(div().text_size(px(style::size::DETAIL)).text_color(p.faint).child("Open project"));
        let card = v_flex()
            .w(px(280.))
            .p(px(14.))
            .gap(px(8.))
            .bg(bg)
            .border_1()
            .border_color(border)
            .rounded(px(8.))
            .shadow_md()
            .occlude()
            .when_some(reason, |s, r| s.child(div().text_size(px(style::size::LABEL)).child(r)))
            .child(div().text_size(px(style::size::DETAIL)).text_color(p.secondary).child(who))
            .child(buttons);
        // A click anywhere else closes it, as a popover does (a clear
        // backdrop over the window; the card sits on top of it).
        let size = window.viewport_size();
        let backdrop = div()
            .id("why-backdrop")
            .w(size.width)
            .h(size.height)
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.project_why = None;
                cx.notify();
            }))
            .child(anchored().position(position).snap_to_window_with_margin(px(8.)).child(card));
        Some(deferred(anchored().position(point(px(0.), px(0.))).child(backdrop)).with_priority(1).into_any_element())
    }

    fn why_action(&mut self, label: &'static str, tip: String, p: Palette, cx: &mut Context<Self>, action: impl Fn(&mut SidePane, &mut Context<SidePane>) + 'static) -> impl IntoElement {
        crate::highlights::tipped(
            div().id(SharedString::from(format!("hl-why-{label}"))).px(px(8.)).py(px(3.)).rounded(px(5.)).bg(style::button_fill(cx)).border_1().border_color(style::button_edge(cx)).cursor_pointer().text_size(px(style::size::DETAIL)).text_color(p.fg).child(label),
            tip,
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| action(this, cx)))
    }

    /// `threads.setProjects`, changing the thread's list here first
    /// (`app.setProjects`): the popover goes and the runner's answer confirms.
    fn set_project_membership(&mut self, thread_id: String, add: Vec<String>, remove: Vec<String>, cx: &mut Context<Self>) {
        self.project_why = None;
        let before = self.app.upgrade().and_then(|app| app.read(cx).threads.get(&thread_id).and_then(|t| t.projects.clone()));
        if let Some(app) = self.app.upgrade() {
            let (add2, remove2) = (add.clone(), remove.clone());
            let id2 = thread_id.clone();
            app.update(cx, |t, cx| {
                if let Some(summary) = t.threads.get_mut(&id2) {
                    let list = summary.projects.get_or_insert_with(Vec::new);
                    list.retain(|m| !remove2.contains(&m.id));
                    for id in &add2 {
                        if let Some(existing) = list.iter_mut().find(|m| m.id == *id) {
                            existing.pinned = true;
                        }
                    }
                }
                cx.notify();
            });
        }
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.setProjects", json!({ "threadId": thread_id, "add": add, "remove": remove })).await;
            this.update(cx, |t, cx| {
                if let Err(e) = res {
                    if let Some(app) = t.app.upgrade() {
                        let id2 = thread_id.clone();
                        app.update(cx, |t, cx| {
                            if let Some(summary) = t.threads.get_mut(&id2) {
                                summary.projects = before;
                            }
                            cx.notify();
                        });
                    }
                    t.note_error(e.message, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

/// `project-why`: opens or closes the popover and says what's in it.
#[cfg(feature = "automation")]
pub(crate) fn project_why_command(side: &Entity<SidePane>, thread: &ThreadSummary, id: Option<&str>, cx: &mut App) -> Result<String, String> {
    let Some(id) = id else {
        side.update(cx, |pane, cx| {
            pane.project_why = None;
            cx.notify();
        });
        return Ok("ok closed".into());
    };
    let member = thread.projects.as_ref().and_then(|list| list.iter().find(|m| m.id.starts_with(id))).ok_or_else(|| format!("the open thread isn't in {id}"))?.clone();
    let (reason, who, keep) = side.read(cx).why_lines(&member);
    side.update(cx, |pane, cx| {
        pane.project_why = Some((member.id.clone(), point(px(600.), px(200.))));
        cx.notify();
    });
    let mut words = vec![reason.unwrap_or_else(|| "(no reason)".to_owned()), who];
    if keep {
        words.push("Keep".to_owned());
    }
    words.push("Take out".to_owned());
    words.push("Open project".to_owned());
    Ok(format!("ok {}", serde_json::to_string(&words).unwrap_or_default()))
}
