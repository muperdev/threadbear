// Cards the agent is waiting on in the transcript: approvals (with queued
// calls stacked on them), questions, secrets, plans, reviews and another
// runner's request for a thread (ApprovalRowCell, QuestionRowCell,
// SecretRowCell, PlanCardCell and ReviewRowCell in Cells.swift).
use super::metrics::*;
use super::work::{self, icon};
use super::{row_wrap, tip};
use crate::style::{self, size};
use crate::thread::ThreadPane;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::collections::HashMap;
use threadbear_core::reducer::{
    ApprovalDecision, ApprovalItem, ApprovalOption, ApprovalRequest, ApprovalText, DesignReview, DiffBuilder, PathDisplay, PlanCard, PlanCardState, PlanCardText, PlanSummary, Question, QuestionItem, QuestionRequest, Row, SecretItem, StartRequest, ToolCall, ToolKind, ToolPresentation,
};

/// `PillButton` in the shape all the cards' buttons share: a capsule, the
/// prominent one filled with the accent colour, the others a hairline outline.
/// A button that isn't enabled gets no click from its caller and reads as off.
pub(crate) fn pill(id: impl Into<ElementId>, title: &str, prominent: bool, enabled: bool, shortcut: Option<&str>, cx: &App) -> Stateful<Div> {
    let (accent, card_bg, card_border, inline_bg, label, tertiary) = (style::control_accent(cx), style::card_background(cx), style::card_border(cx), style::inline_code_background(cx), style::label_text(cx), style::tertiary_label(cx));
    let (bg, border, fg): (Hsla, Hsla, Hsla) = match (prominent, enabled) {
        (true, true) => (accent, gpui_kit::transparent_black(), gpui_kit::white()),
        (true, false) => (inline_bg, gpui_kit::transparent_black(), tertiary),
        (false, true) => (card_bg, card_border, label),
        (false, false) => (card_bg, card_border, tertiary),
    };
    let pressed = if prominent { hsla(accent.h, accent.s, (accent.l - 0.1).max(0.), accent.a) } else { inline_bg };
    h_flex()
        .id(id)
        .role(Role::Button)
        .aria_label(SharedString::from(title.to_owned()))
        .h(px(BUTTON_HEIGHT))
        .min_w(px(76.))
        .px(px(15.))
        .rounded(px(BUTTON_HEIGHT / 2.))
        .justify_center()
        .items_center()
        .gap(px(6.))
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_size(px(size::LABEL))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .when(enabled, move |d| d.cursor_pointer().active(move |s| s.bg(pressed)))
        .child(div().whitespace_nowrap().child(title.to_owned()))
        .when_some(shortcut, |d, shortcut| d.child(div().text_size(px(size::SMALL)).text_color(if prominent && enabled { gpui_kit::white().opacity(0.7) } else { tertiary }).child(shortcut.to_owned())))
}

/// A card's frame: the faint fill, the hairline border, and for one waiting on
/// Mike an accent bar down its left edge, drawn over the border and following
/// the corners (`FillView.accent`).
fn card_frame(content: impl IntoElement, accent: Option<Hsla>, cx: &App) -> Div {
    let left = CARD_PAD + if accent.is_some() { ACCENT_BAR } else { 0. };
    div()
        .relative()
        .rounded(px(CARD_RADIUS))
        .bg(style::card_background(cx))
        .border_1()
        .border_color(style::card_border(cx))
        .child(div().pl(px(left)).pr(px(CARD_PAD)).py(px(CARD_PAD)).child(content))
        .when_some(accent, |d, accent| d.child(div().absolute().inset_0().rounded(px(CARD_RADIUS)).border_l(px(ACCENT_BAR)).border_color(accent)))
}

/// The vertical room a card leaves (`rowTop + 2` above, `rowBottom + 2` below).
fn card_row(row: &Row, card: impl IntoElement) -> AnyElement {
    row_wrap(row, div().pt(px(ROW_TOP + 2.)).pb(px(ROW_BOTTOM + 2.)).child(card))
}

/// `mono`: the code font in the card's code box.
fn mono(id: impl Into<ElementId>, words: impl Into<SharedString>, color: Hsla, cx: &App) -> Stateful<Div> {
    work::mono_text(id, words, color, cx)
}

/// `approvalTitleText`: "Write release.sh?", its folder dimmed after it, and
/// why it's asking (Codex's own words).
fn approval_title(item: &ApprovalItem, cx: &App) -> Div {
    let (label, faint, quiet) = (style::label_text(cx), style::faint_text(cx), style::quiet_text(cx));
    let title = div().text_size(px(size::CARD_TITLE)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(label);
    let folder = ApprovalText::file_path(&item.request).map(|path| PathDisplay::folder_relative(&path, item.root.as_deref())).filter(|f| !f.is_empty());
    let reason = item.request.input.get("reason").and_then(Value::as_str).map(str::trim).filter(|r| !r.is_empty());
    let runner_reason = ApprovalText::runner_reason(&item.request);
    v_flex()
        .child(title.child(h_flex().flex_wrap().items_baseline().child(ApprovalText::title(&item.request)).when_some(folder, |d, folder| d.child(div().ml(px(8.)).text_size(px(size::DETAIL)).font_weight(FontWeight::NORMAL).text_color(faint).child(ApprovalText::shown(&folder))))))
        .when_some(runner_reason, |d, why| d.child(div().text_size(px(size::DETAIL)).line_height(px(19.)).text_color(label).child(why)))
        .when_some(reason, |d, reason| d.child(div().text_size(px(size::DETAIL)).line_height(px(19.)).text_color(quiet).child(ApprovalText::shown(reason))))
}

/// `approvalPayload`: the command, what would be written to a file, the
/// page, or the tool's input, whole, as Allow allows all of it.
pub(crate) fn approval_payload(ix: usize, n: usize, request: &ApprovalRequest, root: Option<&str>, cx: &App) -> AnyElement {
    let label = style::label_text(cx);
    let input = &request.input;
    if let Some(command) = input.get("command").and_then(Value::as_str) {
        let mut col = v_flex().child(mono(("cmd", ix * 100 + n), ApprovalText::shown_exactly(command), label, cx));
        let extra = ApprovalText::shown_fields(&ToolPresentation::fields(input, true).into_iter().filter(|f| !matches!(f.key.as_str(), "command" | "description" | "reason")).collect::<Vec<_>>());
        if !extra.is_empty() {
            col = col.child(div().mt(px(4.)).child(work::fields_text(ix * 100 + n, &extra, cx)));
        }
        return col.into_any_element();
    }
    let is_mcp = request.name.starts_with("mcp__");
    if !is_mcp {
        if ApprovalText::file_path(request).is_some() {
            let call = ToolCall { item_id: request.approval_id.clone(), name: request.name.clone(), kind: if request.name == "Write" { ToolKind::Write } else { ToolKind::Edit }, title: request.title.clone(), input: input.clone() };
            if let Some(mut diff) = DiffBuilder::diff_for_tool(&call).filter(|d| !d.hunks.is_empty()) {
                // What it writes, with any character that hides, breaks or
                // reorders a line named, and tabs and spaces at a line's end
                // shown, as for a command.
                for hunk in &mut diff.hunks {
                    hunk.lines = hunk.lines.iter().map(|l| ApprovalText::shown_exactly(l)).collect();
                }
                // Every line it writes, and every argument the diff doesn't
                // draw: Allow allows all of it, so the card grows to fit, as
                // for a command (Codex review, 10 Nov 2026: a ninth line past
                // eight harmless ones was hidden with no note).
                let mut col = v_flex().child(super::diff::diff_with_note(SharedString::from(format!("approval-diff-{ix}-{n}")), std::slice::from_ref(&diff), 1_000_000, 0., root, super::diff::DiffNote::None, cx));
                let extra = ApprovalText::fields_beyond_diff(input);
                if !extra.is_empty() {
                    col = col.child(div().mt(px(4.)).child(work::fields_text(ix * 100 + n, &extra, cx)));
                }
                return col.into_any_element();
            }
            // Nothing a diff can draw (a notebook cell, say): every field, below.
        } else if let Some(url) = input.get("url").and_then(Value::as_str) {
            return mono(("url", ix * 100 + n), ApprovalText::shown(url), label, cx).into_any_element();
        }
    }
    let mut fields = ApprovalText::shown_fields(&ToolPresentation::fields(input, true));
    if !is_mcp && ApprovalText::file_path(request).is_some() {
        fields.extend(ApprovalText::line_break_notes(input));
    }
    if fields.is_empty() {
        return mono(("title", ix * 100 + n), request.title.clone(), label, cx).into_any_element();
    }
    work::fields_text(ix * 100 + n, &fields, cx).into_any_element()
}

/// The folder, access and model it would start with, then the whole brief:
/// what Mike allows is all of it (`startRequestPayload`).
fn start_request_payload(ix: usize, info: Option<&StartRequest>, cx: &App) -> AnyElement {
    let (quiet, label) = (style::quiet_text(cx), style::label_text(cx));
    let info = match info.filter(|i| i.error.is_none()) {
        Some(i) => i,
        None => {
            let note = if info.is_some() { "" } else { "Reading the request…" };
            return div().text_size(px(size::DETAIL)).line_height(px(19.)).text_color(quiet).child(note.to_owned()).into_any_element();
        }
    };
    let r = &info.request;
    let access = match r.permission_mode.as_deref() {
        Some("ask") => "Asks before edits".to_owned(),
        Some("auto") => "Auto".to_owned(),
        Some("plan") => "Plan first".to_owned(),
        None | Some("") | Some("full") => "Full access".to_owned(),
        // The runner refuses a mode it doesn't know rather than starting on
        // full access, so the card says what was asked, not "Full".
        Some(other) => format!("Unknown ({other})"),
    };
    let mut facts = vec![format!("Folder: {}", r.cwd.clone().filter(|c| !c.is_empty()).unwrap_or_else(|| "the usual one".to_owned())), format!("Access: {access}")];
    if let Some(model) = r.model.as_ref().filter(|m| !m.is_empty()) {
        facts.push(format!("Model: {model}{}", r.effort.as_ref().map(|e| format!(", {e}")).unwrap_or_default()));
    }
    v_flex()
        .text_size(px(size::DETAIL))
        .line_height(px(19.))
        .child(div().text_color(quiet).child(facts.join("\n")))
        .child(div().mt(px(19.)).text_color(label).child(gpui_kit::base::SelectableText::new(("brief", ix), r.text.clone())))
        .into_any_element()
}

/// `startRequestTitle`: who's asking and where it would start.
fn start_request_title(info: Option<&StartRequest>, cx: &App) -> Div {
    let (label, quiet) = (style::label_text(cx), style::quiet_text(cx));
    let title = |words: String| div().text_size(px(size::CARD_TITLE)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(label).child(words);
    let quiet_line = |words: String| div().text_size(px(size::DETAIL)).line_height(px(19.)).text_color(quiet).child(words);
    let Some(info) = info else { return v_flex().child(title("Another machine wants to start a thread".to_owned())) };
    if let Some(error) = &info.error {
        return v_flex().child(title("Couldn't read this request".to_owned())).child(quiet_line(error.clone()));
    }
    let place = info.target.as_ref().map(|t| if t.is_self { "this Mac".to_owned() } else { t.name.clone() }).unwrap_or_else(|| "this Mac".to_owned());
    let mut col = v_flex().child(title(format!("{} wants to start a thread on {place}", info.request.runner_name)));
    let mut notes = vec![];
    if info.target.is_none() {
        notes.push(format!("It asked for \u{201C}{}\u{201D}, which isn't one of your machines.", info.request.target));
    }
    if let Some(folder) = &info.missing_folder {
        notes.push(format!("{folder} isn't a folder on this Mac. Start it sends the request back, to ask again with one that is."));
    }
    if let Some(why) = info.request.why.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
        notes.push(why.to_owned());
    }
    for note in notes {
        col = col.child(quiet_line(note));
    }
    col
}

/// The answers map for `approvals.respond`, as `QuestionRowCell.answers`:
/// "Something else" is the answer to the last question, replacing a single
/// choice and adding to multiple ones.
pub(crate) fn question_answers(request: &QuestionRequest, chosen: &HashMap<usize, Vec<String>>, typed: &str) -> HashMap<String, String> {
    let mut answers = HashMap::new();
    let last = request.questions.len().saturating_sub(1);
    for (index, question) in request.questions.iter().enumerate() {
        let mut parts: Vec<String> = question.options.iter().map(|o| o.label.clone()).filter(|l| chosen.get(&index).is_some_and(|c| c.contains(l))).collect();
        if !typed.is_empty() && index == last {
            if question.multi_select {
                parts.push(typed.to_owned());
            } else {
                parts = vec![typed.to_owned()];
            }
        }
        if !parts.is_empty() {
            answers.insert(question.question.clone(), parts.join(", "));
        }
    }
    answers
}

/// A spinner in the small space beside a card's buttons.
fn card_spinner(pane: &ThreadPane, busy: bool, cx: &App) -> Option<AnyElement> {
    busy.then(|| div().ml(px(2.)).flex().items_center().child(pane.spinner(16., cx)).into_any_element())
}

impl ThreadPane {
    fn approval_buttons(&mut self, ix: usize, item: &ApprovalItem, busy: bool, cx: &mut Context<Self>) -> Div {
        let id = item.request.approval_id.clone();
        let start = item.is_start_request();
        // Another runner asking for a thread: Start it or Don't, once its
        // request has been read.
        let options: Vec<ApprovalOption> = if start {
            match &item.start_request {
                Some(info) if info.error.is_none() => vec![ApprovalOption::Allow, ApprovalOption::Deny],
                _ => vec![],
            }
        } else {
            item.request.options.clone()
        };
        let mut row = h_flex().mt(px(12.)).gap(px(8.)).items_center();
        for option in [ApprovalOption::Allow, ApprovalOption::AllowAlways, ApprovalOption::Deny] {
            if !options.contains(&option) {
                continue;
            }
            let (title, decision, prominent, tip_text, n): (&str, &str, bool, &str, usize) = match option {
                ApprovalOption::Allow if start => ("Start it", "allow", true, "Start this thread with this brief", 0),
                ApprovalOption::Allow => ("Allow", "allow", true, "Let it do this once (Return)", 0),
                ApprovalOption::AllowAlways => ("Always allow", "allowAlways", false, "Allow this, and anything like it for the rest of this thread", 1),
                ApprovalOption::Deny if start => ("Don't", "deny", false, "Don't start it; the asking thread is told no", 2),
                ApprovalOption::Deny => ("Deny", "deny", false, "Don't let it do this; the agent is told no", 2),
            };
            let (id, decision_str) = (id.clone(), decision.to_owned());
            let start_allow = start && option == ApprovalOption::Allow;
            let start_card = start;
            let mut button = pill(("approve", ix * 10 + n), title, prominent, !busy, if !start && option == ApprovalOption::Allow { Some("⏎") } else { None }, cx).tooltip(tip(tip_text));
            if !busy {
                button = button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if start_card {
                        this.answer_start_request(&id, start_allow, cx);
                    } else {
                        this.respond_approval(&id, &decision_str, None, None, cx);
                    }
                }));
            }
            row = row.child(button.test_support());
        }
        row.children(card_spinner(self, busy, cx))
    }

    pub(crate) fn approval_card(&mut self, ix: usize, row: &Row, item: &ApprovalItem, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let id = &item.request.approval_id;
        let busy = self.rows.answering.contains(id);
        let start = item.is_start_request();
        if start && item.start_request.is_none() {
            self.load_start_request(id, cx);
        }
        let title = if start { start_request_title(item.start_request.as_ref(), cx) } else { approval_title(item, cx) };
        let payload = if start { start_request_payload(ix, item.start_request.as_ref(), cx) } else { approval_payload(ix, 0, &item.request, item.root.as_deref(), cx) };
        let content = v_flex().child(title).child(div().mt(px(10.)).p(px(BOX_PAD)).rounded(px(8.)).bg(style::code_background(cx)).child(payload)).child(self.approval_buttons(ix, item, busy, cx));
        card_row(row, card_frame(content, Some(style::control_accent(cx)), cx))
    }

    /// A card with calls queued on it: its own call and each queued one, each
    /// with its title, its diff or command, and a tick and cross, with Allow
    /// and Deny all under them. No Always allow: a rule from one click on many
    /// calls isn't something Mike was asked about (`configureStacked`).
    pub(crate) fn stacked_approval_card(&mut self, ix: usize, row: &Row, item: &ApprovalItem, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let approval_id = item.request.approval_id.clone();
        let busy = self.rows.answering.contains(&approval_id);
        let calls: Vec<&ApprovalRequest> = std::iter::once(&item.request).chain(item.queued().iter()).collect();
        // A queued call's approvalId is its own item id (ApprovalQueue).
        let ids: Vec<String> = std::iter::once(item.request.item_id.clone().unwrap_or_default()).chain(item.queued().iter().map(|c| c.approval_id.clone())).collect();
        let unticked = self.rows.unticked.get(&approval_id).cloned().unwrap_or_default();
        let count = ids.iter().filter(|i| !unticked.contains(*i)).count();
        let label = style::label_text(cx);
        let title = if calls.iter().all(|c| matches!(c.name.as_str(), "Edit" | "MultiEdit" | "Write" | "NotebookEdit")) { format!("Make these {} changes?", calls.len()) } else { format!("Allow these {} steps?", calls.len()) };
        let mut content = v_flex().child(div().text_size(px(size::CARD_TITLE)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(label).child(title));
        let (hairline, code_bg) = (style::hairline(cx), style::code_background(cx));
        for (i, call) in calls.iter().enumerate() {
            let call_item = ApprovalItem::new((*call).clone(), item.root.clone(), false, None);
            let ticked = !unticked.contains(&ids[i]);
            let step = if ticked { 1. } else { 0.45 };
            let (tick_id, cross_id, aid) = (ids[i].clone(), ids[i].clone(), approval_id.clone());
            let aid2 = aid.clone();
            let mut tick = pill(("tick", ix * 100 + i), "✓", ticked, !busy, None, cx).min_w(px(stacked::TOGGLE_WIDTH)).w(px(stacked::TOGGLE_WIDTH)).h(px(stacked::TOGGLE_HEIGHT)).px(px(0.)).tooltip(tip("Allow this one"));
            let mut cross = pill(("cross", ix * 100 + i), "✕", !ticked, !busy, None, cx).min_w(px(stacked::TOGGLE_WIDTH)).w(px(stacked::TOGGLE_WIDTH)).h(px(stacked::TOGGLE_HEIGHT)).px(px(0.)).tooltip(tip("Don't allow this one; the agent is told no"));
            if !busy {
                tick = tick.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_ticked(&aid, &tick_id, true, cx)));
                cross = cross.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_ticked(&aid2, &cross_id, false, cx)));
            }
            let head = h_flex().items_center().justify_between().opacity(step).child(div().flex_1().min_w_0().child(approval_title(&call_item, cx))).child(h_flex().flex_none().gap(px(6.)).ml(px(12.)).child(tick.test_support()).child(cross.test_support()));
            let body = div().mt(px(8.)).p(px(BOX_PAD)).rounded(px(8.)).bg(code_bg).opacity(step).child(approval_payload(ix, i, call, item.root.as_deref(), cx));
            // The gap with its hairline in the middle (StackedApproval.gap).
            content = content.child(div().h(px(stacked::GAP / 2.)).border_b_1().border_color(hairline).mb(px(stacked::GAP / 2.))).child(head).child(body);
        }
        let allow_words = format!("Allow {count}");
        let allow_tip = if count == ids.len() { "Let it do all of these (Return)" } else { "Let it do the ticked ones; the rest are refused (Return)" };
        let id_for_allow = approval_id.clone();
        let mut allow = pill(("stack-allow", ix), &allow_words, true, !busy && count > 0, Some("⏎"), cx).tooltip(tip(allow_tip));
        if !busy && count > 0 {
            allow = allow.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.respond_queued(&id_for_allow, "allow", cx)));
        }
        let id_for_deny = approval_id.clone();
        let mut deny = pill(("stack-deny", ix), "Deny all", false, !busy, None, cx).tooltip(tip("Don't let it do any of these; the agent is told no"));
        if !busy {
            deny = deny.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.respond_queued(&id_for_deny, "deny", cx)));
        }
        let buttons = h_flex().mt(px(12.)).gap(px(8.)).items_center().child(allow.test_support()).child(deny.test_support()).children(card_spinner(self, busy, cx));
        card_row(row, card_frame(content.child(buttons), Some(style::control_accent(cx)), cx))
    }

    fn set_ticked(&mut self, approval_id: &str, item_id: &str, ticked: bool, cx: &mut Context<Self>) {
        let set = self.rows.unticked.entry(approval_id.to_owned()).or_default();
        if ticked {
            set.remove(item_id);
        } else {
            set.insert(item_id.to_owned());
        }
        cx.notify();
    }

    /// `queuedDecisions`: its own call's answer, and each queued call's, from
    /// the ticks. Deny denies the lot.
    pub(crate) fn respond_queued(&mut self, approval_id: &str, decision: &str, cx: &mut Context<Self>) {
        let Some(item) = self.feed.model().rows().iter().find_map(|r| match &r.content {
            threadbear_core::reducer::RowContent::Approval(a) if a.request.approval_id == approval_id => Some(a.clone()),
            _ => None,
        }) else {
            return;
        };
        let unticked = self.rows.unticked.get(approval_id).cloned().unwrap_or_default();
        let allow = decision != "deny";
        let mut queued = serde_json::Map::new();
        for call in item.queued() {
            queued.insert(call.approval_id.clone(), Value::String((if allow && !unticked.contains(&call.approval_id) { "allow" } else { "deny" }).to_owned()));
        }
        let own_allow = allow && !unticked.contains(item.request.item_id.as_deref().unwrap_or(""));
        // Nothing ticked: Allow has nothing to allow.
        if allow && !own_allow && !queued.values().any(|v| v == "allow") {
            return;
        }
        self.respond_approval(approval_id, if own_allow { "allow" } else { "deny" }, None, Some(Value::Object(queued)), cx);
    }

    /// `ThreadSession.respond`: the card spins and takes no more clicks until
    /// the answer lands; on a failure the buttons come back with the error.
    pub(crate) fn respond_approval(&mut self, approval_id: &str, decision: &str, answers: Option<(serde_json::Map<String, Value>, String)>, queued: Option<Value>, cx: &mut Context<Self>) {
        if self.rows.answering.contains(approval_id) {
            return;
        }
        self.rows.answering.insert(approval_id.to_owned());
        cx.notify();
        let mut params = json!({ "threadId": self.summary.id, "approvalId": approval_id, "decision": decision });
        if let Some((answers, message)) = answers {
            params["answers"] = Value::Object(answers);
            params["message"] = Value::String(message);
        }
        if let Some(queued) = queued.filter(|q| q.as_object().is_some_and(|o| !o.is_empty())) {
            params["queued"] = queued;
        }
        let id = approval_id.to_owned();
        self.request_then("approvals.respond", params, move |this, failed, cx| {
            if failed {
                this.rows.answering.remove(&id);
                cx.notify();
            }
        }, cx);
    }

    fn load_start_request(&mut self, approval_id: &str, cx: &mut Context<Self>) {
        if !self.rows.start_requests_loading.insert(approval_id.to_owned()) {
            return;
        }
        let (client, thread, id) = (self.client_handle(), self.summary.id.clone(), approval_id.to_owned());
        cx.spawn(async move |this, cx| {
            let result = crate::call(client, "requests.get", json!({ "threadId": thread, "approvalId": id })).await;
            this.update(cx, |this, cx| {
                this.rows.start_requests_loading.remove(&id);
                let info = match result {
                    Ok(v) => StartRequest::decode(&v).unwrap_or_else(|| failed_start_request("Couldn't read this request.")),
                    Err(e) => failed_start_request(&e.message),
                };
                let change = this.feed.set_start_request(&id, info);
                this.apply(change, cx);
            })
            .ok();
        })
        .detach();
    }

    /// `answerStartRequest`: this Mac's runner starts it and answers the
    /// card, never the thread's own runner, which may not start threads
    /// elsewhere. The fingerprint ties the answer to what the card showed.
    fn answer_start_request(&mut self, approval_id: &str, allow: bool, cx: &mut Context<Self>) {
        let Some(item) = self.feed.model().rows().iter().find_map(|r| match &r.content {
            threadbear_core::reducer::RowContent::Approval(a) if a.request.approval_id == approval_id && a.is_start_request() => Some(a.clone()),
            _ => None,
        }) else {
            return;
        };
        let shown = item.start_request.clone();
        if allow && shown.as_ref().is_none_or(|s| s.error.is_some() || s.fingerprint.is_none()) {
            return;
        }
        if !self.rows.answering.insert(approval_id.to_owned()) {
            return;
        }
        cx.notify();
        let mut params = json!({ "threadId": self.summary.id, "approvalId": approval_id, "allow": allow });
        if allow {
            if let Some(runner) = shown.as_ref().and_then(|s| s.target.as_ref()) {
                params["runnerId"] = json!(runner.id);
            }
        }
        if let Some(fingerprint) = shown.and_then(|s| s.fingerprint) {
            params["fingerprint"] = json!(fingerprint);
        }
        let id = approval_id.to_owned();
        self.request_then("requests.answer", params, move |this, failed, cx| {
            if failed {
                this.rows.answering.remove(&id);
                // Read it again, so a second click answers what's true now.
                this.load_start_request(&id, cx);
                cx.notify();
            }
        }, cx);
    }

    pub(crate) fn question_card(&mut self, ix: usize, row: &Row, item: &QuestionItem, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let settled = item.resolution.is_some();
        let approval_id = item.request.approval_id.clone();
        let chosen = self.chosen.get(&approval_id).cloned().unwrap_or_default();
        let busy = self.rows.answering.contains(&approval_id);
        let (label, quiet, secondary, accent, faint_icon) = (style::label_text(cx), style::quiet_text(cx), style::secondary_label(cx), style::control_accent(cx), style::faint_icon(cx));
        let mut content = v_flex();
        for (qi, question) in item.request.questions.iter().enumerate() {
            let text = div().text_size(px(size::CARD_TITLE)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(if settled { quiet } else { label }).child(question.question.clone());
            if settled {
                content = content.child(div().mb(px(ANSWERED_GAP)).child(text));
                continue;
            }
            let mut options = v_flex().mt(px(8.));
            for (oi, option) in question.options.iter().enumerate() {
                let picked = chosen.get(&qi).is_some_and(|c| c.contains(&option.label));
                options = options.child(question_option(ix, qi, oi, question, approval_id.clone(), &option.label, option.description.as_deref(), picked, busy, label, secondary, accent, faint_icon, cx));
            }
            content = content.child(div().mb(px(12.)).child(v_flex().child(text).child(options)));
        }
        if let Some(resolution) = &item.resolution {
            let (symbol, tint): (&str, Hsla) = match resolution.decision {
                ApprovalDecision::Answered => ("checkmark.circle.fill", accent),
                ApprovalDecision::Expired => ("clock", faint_icon),
                _ => ("xmark.circle", faint_icon),
            };
            let message = resolution.message.as_deref().map(|m| m.split('\n').next().unwrap_or("").to_owned()).filter(|m| !m.is_empty());
            let text = if resolution.decision == ApprovalDecision::Answered { message.clone().unwrap_or_else(|| "Answered".to_owned()) } else { ApprovalText::decision(resolution.decision).to_owned() };
            let color = if resolution.decision == ApprovalDecision::Answered { label } else { quiet };
            content = content.child(
                h_flex()
                    .id(("answered", ix))
                    .h(px(ANSWER_HEIGHT))
                    .items_center()
                    .when_some(message, |d, m| d.tooltip(tip(&m)))
                    .child(div().flex_none().w(px(TITLE_INDENT)).child(icon(symbol, 14., tint)))
                    .child(div().min_w_0().truncate().text_size(px(size::WORK)).text_color(color).child(text)),
            );
        } else {
            let other = self.other_input(&approval_id, window, cx);
            let typed = other.read(cx).value().trim().to_owned();
            let answers = question_answers(&item.request, &chosen, &typed);
            let ready = !busy && !answers.is_empty();
            let (id, request) = (approval_id.clone(), item.request.clone());
            let mut send = pill(("send-answer", ix), "Send answer", true, ready, None, cx).min_w(px(110.)).tooltip(tip("Send your answers to the agent"));
            if ready {
                send = send.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.send_question_answer(&id, &request, cx)));
            }
            content = content.child(input_box(ix, "other", &other, !busy, window, cx)).child(h_flex().mt(px(14.)).items_center().child(send.test_support()).children(card_spinner(self, busy, cx)));
        }
        let accent_bar = (!settled).then_some(accent);
        let mut frame = card_frame(content, accent_bar, cx);
        // Answered, it's history: an outline with no fill or accent.
        if settled {
            frame = frame.bg(gpui_kit::transparent_black());
        }
        card_row(row, frame)
    }

    fn other_input(&mut self, approval_id: &str, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(input) = self.rows.others.get(approval_id) {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Something else"));
        let id = approval_id.to_owned();
        let subscription = cx.subscribe(&input, move |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                // Typing for a single-choice last question clears its picked
                // option, so the answer is one or the other, never both.
                let typed = input.read(cx).value().trim().to_string();
                if !typed.is_empty() {
                    let single = this.feed.model().rows().iter().find_map(|r| match &r.content {
                        threadbear_core::reducer::RowContent::Question(q) if q.request.approval_id == id => q.request.questions.last().map(|l| (q.request.questions.len() - 1, l.multi_select)),
                        _ => None,
                    });
                    if let Some((last, false)) = single {
                        if let Some(picks) = this.chosen.get_mut(&id) {
                            picks.remove(&last);
                        }
                    }
                }
                cx.notify();
            }
        });
        self.rows.subscriptions.push(subscription);
        self.rows.others.insert(approval_id.to_owned(), input.clone());
        input
    }

    fn send_question_answer(&mut self, approval_id: &str, request: &QuestionRequest, cx: &mut Context<Self>) {
        let chosen = self.chosen.get(approval_id).cloned().unwrap_or_default();
        let typed = self.rows.others.get(approval_id).map(|i| i.read(cx).value().trim().to_owned()).unwrap_or_default();
        let answers = question_answers(request, &chosen, &typed);
        if answers.is_empty() {
            return;
        }
        // The answers also go as the message, which the runner keeps in the
        // transcript, so the card can show what was chosen after the drafts
        // are gone.
        let message = ApprovalText::answer_summary(request, &answers);
        let map: serde_json::Map<String, Value> = answers.into_iter().map(|(k, v)| (k, Value::String(v))).collect();
        self.respond_approval(approval_id, "answered", Some((map, message)), None, cx);
    }

    pub(crate) fn secret_card(&mut self, ix: usize, row: &Row, item: &SecretItem, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (label, quiet, faint, faint_icon, failed) = (style::label_text(cx), style::quiet_text(cx), style::faint_text(cx), style::faint_icon(cx), style::failed_text(cx));
        let approval_id = item.request.approval_id.clone();
        let state = self.rows.secret_states.get(&approval_id).cloned().unwrap_or(SecretState::Ready);
        let busy = matches!(state, SecretState::Sending | SecretState::Sent);
        let field = self.secret_input(&approval_id, item.request.placeholder.as_deref(), window, cx);
        let can_save = !busy && !field.read(cx).value().trim().is_empty();
        let text = v_flex()
            .child(div().text_size(px(size::CARD_TITLE)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(label).child(item.request.label.clone()))
            .when(!item.request.reason.is_empty(), |d| d.child(div().text_size(px(size::DETAIL)).line_height(px(19.)).text_color(quiet).child(item.request.reason.clone())));
        let (id, for_save) = (approval_id.clone(), field.clone());
        let mut save = pill(("secret-save", ix), "Save securely", true, can_save, None, cx).min_w(px(110.)).tooltip(tip("Save it to a private file for the agent (Return)"));
        if can_save {
            save = save.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                let value = for_save.read(cx).value().to_string();
                this.answer_secret(&id, Some(value), window, cx);
            }));
        }
        let controls = h_flex().mt(px(12.)).gap(px(8.)).items_center().child(div().flex_1().min_w_0().child(input_box(ix, "secret", &field, !busy, window, cx))).child(save.test_support());
        // What happens to it, or why it didn't go, then Decline at the right.
        let (note_symbol, note_color, note_text) = match &state {
            SecretState::Failed(message) => ("exclamationmark.triangle", failed, message.clone()),
            _ => ("lock.shield", faint, PROMISE.to_owned()),
        };
        let note_tint = if matches!(state, SecretState::Failed(_)) { failed } else { faint_icon };
        let (id, tooltip_text) = (approval_id.clone(), if matches!(state, SecretState::Failed(_)) { note_text.clone() } else { PROMISE_TIP.to_owned() });
        let mut decline = div().id(("decline", ix)).role(Role::Button).aria_label(SharedString::from("Decline")).flex_none().ml(px(6.)).text_size(px(size::LABEL)).text_color(quiet).tooltip(tip("Don't give it; the agent is told you declined")).child("Decline");
        if !busy {
            decline = decline.cursor_pointer().on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.answer_secret(&id, None, window, cx)));
        }
        let footer = h_flex()
            .id(("secret-note", ix))
            .mt(px(10.))
            .items_start()
            .justify_between()
            .tooltip(tip(&tooltip_text))
            .child(h_flex().flex_1().min_w_0().items_start().child(div().flex_none().w(px(TITLE_INDENT)).child(icon(note_symbol, 14., note_tint))).child(div().min_w_0().text_size(px(size::SMALL)).line_height(px(16.)).text_color(note_color).child(note_text)))
            .child(decline.test_support());
        let content = v_flex().child(text).child(controls).child(footer);
        card_row(row, card_frame(content, Some(style::control_accent(cx)), cx))
    }

    fn secret_input(&mut self, approval_id: &str, placeholder: Option<&str>, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(input) = self.rows.secrets.get(approval_id) {
            return input.clone();
        }
        let hint = placeholder.unwrap_or("Paste the secret").to_owned();
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder(hint);
            state.set_masked(true, window, cx);
            state
        });
        let id = approval_id.to_owned();
        let subscription = cx.subscribe_in(&input, window, move |this, input, event: &InputEvent, window, cx| match event {
            InputEvent::Change => cx.notify(),
            // Return saves; nothing else in the field does anything special.
            InputEvent::PressEnter { .. } => {
                let value = input.read(cx).value().to_string();
                if !value.trim().is_empty() {
                    this.answer_secret(&id, Some(value), window, cx);
                }
            }
            _ => {}
        });
        self.rows.subscriptions.push(subscription);
        self.rows.secrets.insert(approval_id.to_owned(), input.clone());
        input
    }

    /// `saveClicked` / `declineClicked`: the value goes straight into the one
    /// request and out of the field the moment it goes, so a failed send
    /// means typing it again, never a value left sitting in the window.
    fn answer_secret(&mut self, approval_id: &str, value: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.rows.secret_states.get(approval_id).cloned().unwrap_or(SecretState::Ready);
        if matches!(state, SecretState::Sending | SecretState::Sent) {
            return;
        }
        if let Some(field) = self.rows.secrets.get(approval_id).cloned() {
            field.update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.rows.secret_states.insert(approval_id.to_owned(), SecretState::Sending);
        cx.notify();
        let mut params = json!({ "threadId": self.summary.id, "approvalId": approval_id });
        match value {
            Some(v) => params["secret"] = Value::String(v),
            None => params["decline"] = Value::Bool(true),
        }
        let id = approval_id.to_owned();
        let client = self.client_handle();
        cx.spawn(async move |this, cx| {
            let result = crate::call(client, "secrets.answer", params).await;
            this.update(cx, |this, cx| {
                // It went; the card closes when approval.resolved comes.
                this.rows.secret_states.insert(id, match result {
                    Ok(_) => SecretState::Sent,
                    Err(e) => SecretState::Failed(e.message),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn plan_card(&mut self, ix: usize, row: &Row, plan: &PlanCard, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let expanded = self.open_rows.contains(&row.id);
        let (label, secondary) = (style::label_text(cx), style::secondary_label(cx));
        let waiting = plan.state == PlanCardState::Waiting;
        let (split_title, body_text) = PlanCardText::split(&plan.text);
        let (preview, clipped) = PlanCardText::preview(&body_text, PlanCardText::PREVIEW_LINES);
        let title = if plan.entries.is_empty() { split_title.unwrap_or_else(|| "Plan".to_owned()) } else { PlanSummary::title(&plan.entries) };
        let status = match plan.state {
            PlanCardState::Waiting => "Waiting for you",
            PlanCardState::SentBack => "Sent back",
            PlanCardState::Approved if plan.entries.is_empty() => "Approved",
            PlanCardState::Approved => "",
        };
        let heading = h_flex()
            .items_center()
            .justify_between()
            .child(
                h_flex()
                    .min_w_0()
                    .items_center()
                    .gap(px(4.))
                    .child(div().text_size(px(size::CARD_TITLE)).line_height(px(20.)).font_weight(FontWeight::MEDIUM).text_color(label).child(title))
                    .child(div().id(("plan-info", ix)).flex_none().tooltip(tip(PLAN_INFO)).child(icon("info.circle", 14., style::faint_icon(cx)))),
            )
            .child(div().flex_none().text_size(px(size::LABEL)).text_color(secondary).child(status));
        // The plan (its start, until it's opened), or once approved its
        // checklist with the plan under it when opened.
        let plan_markdown = if expanded { body_text.clone() } else { preview };
        let doc = (!plan_markdown.is_empty() && (plan.entries.is_empty() || expanded)).then(|| self.markdown_state(&format!("plan-{}", plan.item_id), &markdown_source(&plan_markdown), cx));
        let text_style = self.reading_style(cx);
        let mut content = v_flex().child(heading);
        let mut body = v_flex().mt(px(8.));
        if !plan.entries.is_empty() {
            body = body.child(super::plan_steps(ix, &plan.entries, cx));
        }
        if let Some(doc) = doc {
            body = body.child(div().when(!plan.entries.is_empty(), |d| d.mt(px(23.))).text_size(px(size::BODY)).line_height(px(size::BODY_LINE)).child(self.linked_text(&doc, &row.id, cx).selectable(true).style(text_style).code_block_highlighter(super::plain_code_blocks)));
        }
        content = content.child(body);
        let has_footer = waiting || !plan.entries.is_empty() || clipped;
        if has_footer {
            let busy = waiting && self.rows.answering.contains(&plan.item_id);
            let mut footer = h_flex().mt(px(12.)).items_center().gap(px(8.));
            if waiting {
                let (item_id_a, thread_a) = (plan.item_id.clone(), self.summary.id.clone());
                let mut go = pill(("plan-go", ix), "Go ahead", true, !busy, None, cx).tooltip(tip("Approve the plan and let it start"));
                if !busy {
                    go = go.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.rows.answering.insert(item_id_a.clone());
                        cx.notify();
                        let (item, params) = (item_id_a.clone(), json!({ "threadId": thread_a, "itemId": item_id_a }));
                        this.request_then("plans.approve", params, move |this, _, cx| {
                            this.rows.answering.remove(&item);
                            cx.notify();
                        }, cx);
                    }));
                }
                let mut back = pill(("plan-back", ix), "Send back…", false, !busy, None, cx).tooltip(tip("Tell it what to change; it plans again"));
                if !busy {
                    back = back.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.focus_composer(window, cx)));
                }
                footer = footer.child(go.test_support()).child(back.test_support()).children(card_spinner(self, busy, cx));
            }
            let toggle_title: Option<&str> = if !plan.entries.is_empty() {
                Some(if expanded { "Hide plan" } else { "Show plan" })
            } else if clipped {
                Some(if expanded { "Show less" } else { "Show full plan" })
            } else {
                None
            };
            if let Some(toggle_title) = toggle_title {
                let row_for_toggle = row.clone();
                footer = footer.child(
                    div()
                        .id(("plan-toggle", ix))
                        .role(Role::Button)
                        .aria_label(SharedString::from(toggle_title.to_owned()))
                        .cursor_pointer()
                        .text_size(px(size::LABEL))
                        .text_color(secondary)
                        .tooltip(tip(if expanded { "Show less of it" } else { "Show the whole plan" }))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_row(&row_for_toggle, cx)))
                        .child(toggle_title.to_owned())
                        .test_support(),
                );
            }
            content = content.child(footer);
        }
        card_row(row, card_frame(content, None, cx))
    }

    pub(crate) fn review_card(&mut self, ix: usize, row: &Row, review: &DesignReview, cx: &mut Context<Self>) -> AnyElement {
        let (label, quiet, secondary) = (style::label_text(cx), style::quiet_text(cx), style::secondary_label(cx));
        let accent = review.is_pending().then(|| style::needs_you_text(cx));
        let picked = review.response.as_ref().and_then(|r| r.picked_ids.first()).and_then(|id| review.options.iter().find(|o| &o.id == id));
        let state = if review.is_pending() {
            "Waiting for your review".to_owned()
        } else if review.status == "answered" {
            picked.map(|o| format!("Picked {}. {}", o.id, o.title)).unwrap_or_else(|| "Notes sent".to_owned())
        } else if review.status == "superseded" {
            "Answered in the thread".to_owned()
        } else {
            "Review closed".to_owned()
        };
        let detail = format!("{} images · {state}", review.options.len());
        let open_title = if review.is_pending() { format!("Compare {} images", review.options.len()) } else { "Open review".to_owned() };
        let content = v_flex()
            .child(h_flex().items_center().gap(px(6.)).child(div().flex_none().w(px(18.)).child(icon("rectangle.on.rectangle", 14., secondary))).child(div().min_w_0().truncate().text_size(px(size::WORK)).line_height(px(22.)).font_weight(FontWeight::SEMIBOLD).text_color(label).child(review.title.clone())))
            .child(div().mt(px(4.)).text_size(px(size::DETAIL)).line_height(px(20.)).text_color(quiet).child(detail))
            // The window that compares the images isn't ported yet, so this
            // doesn't pretend it opens one (options window, stage 2).
            .child(h_flex().mt(px(10.)).child(pill(("open-review", ix), &open_title, false, false, None, cx).min_w(px(140.)).tooltip(tip("Comparing images isn't ported to this app yet; use the Swift app's review window"))));
        row_wrap(row, div().py(px(ROW_TOP)).child(card_frame(content, accent, cx)))
    }
}

/// Plan text goes through the same preparation as a reply's.
fn markdown_source(text: &str) -> String {
    super::markdown::prepare(text)
}

fn question_option(
    ix: usize,
    qi: usize,
    oi: usize,
    question: &Question,
    approval_id: String,
    title: &str,
    description: Option<&str>,
    picked: bool,
    busy: bool,
    label: Hsla,
    secondary: Hsla,
    accent: Hsla,
    faint_icon: Hsla,
    cx: &mut Context<ThreadPane>,
) -> AnyElement {
    let multi = question.multi_select;
    let (symbol, color) = if picked { (if multi { "checkbox.on" } else { "radio.on" }, accent) } else { (if multi { "checkbox.off" } else { "radio.off" }, faint_icon) };
    let glyph = radio_or_check(symbol, color, multi, picked);
    let (id_label, approval_title) = (title.to_owned(), title.to_owned());
    let aid = Some(approval_id);
    let mut line = h_flex()
        .id(("q-option", ix * 10000 + qi * 100 + oi))
        .role(if multi { Role::CheckBox } else { Role::RadioButton })
        .aria_label(SharedString::from(id_label))
        .h(px(OPTION_HEIGHT))
        .items_center()
        .gap(px(6.))
        .text_size(px(size::WORK))
        .text_color(label)
        .child(glyph)
        .child(div().min_w_0().truncate().child(title.to_owned()));
    if !busy {
        line = line.cursor_pointer().on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            let Some(id) = aid.clone() else { return };
            let picks = this.chosen.entry(id.clone()).or_default().entry(qi).or_default();
            if multi {
                if let Some(at) = picks.iter().position(|p| *p == approval_title) {
                    picks.remove(at);
                } else {
                    picks.push(approval_title.clone());
                }
            } else {
                *picks = vec![approval_title.clone()];
                // Choosing an option for the last question replaces typed text.
                if qi + 1 == this.question_count(&id) {
                    if let Some(other) = this.rows.others.get(&id).cloned() {
                        other.update(cx, |s, cx| s.set_value("", window, cx));
                    }
                }
            }
            cx.notify();
        }));
    }
    let option = v_flex().child(line.test_support()).when_some(description.filter(|d| !d.is_empty()), |d, description| {
        d.child(div().pl(px(20.)).mt(px(-2.)).text_size(px(size::LABEL)).line_height(px(OPTION_DESCRIPTION)).text_color(secondary).child(description.to_owned()))
    });
    option.into_any_element()
}

/// A radio button or checkbox drawn like a macOS one: a filled grey disc, in
/// the accent with a white mark once picked.
fn radio_or_check(_symbol: &str, color: Hsla, multi: bool, picked: bool) -> impl IntoElement {
    let glyph = div().size(px(14.)).flex_none().flex().items_center().justify_center();
    let (empty, accent) = (hsla(0., 0., 1., 0.14), color);
    if multi {
        glyph.rounded(px(4.)).bg(if picked { accent } else { empty }).when(picked, |d| d.child(icon("checkmark", 11., gpui_kit::white())))
    } else if picked {
        glyph.rounded_full().bg(accent).child(div().size(px(5.)).rounded_full().bg(gpui_kit::white()))
    } else {
        glyph.rounded_full().bg(empty)
    }
}

/// The boxed single-line field a question's "Something else" and a secret
/// are typed in: a plain field in a rounded box, the outline taking the
/// accent colour while it's being typed in (`updateOtherBox`).
fn input_box(ix: usize, kind: &'static str, input: &Entity<InputState>, enabled: bool, window: &Window, cx: &App) -> AnyElement {
    let focused = input.read(cx).focus_handle(cx).contains_focused(window, cx);
    let (code_bg, border, accent) = (style::code_background(cx), style::card_border(cx), style::control_accent(cx));
    div()
        .id((SharedString::from(format!("{kind}-box-{ix}")), 0usize))
        .h(px(OTHER_FIELD_HEIGHT))
        .flex()
        .items_center()
        .px(px(BOX_PAD))
        .rounded(px(8.))
        .bg(code_bg)
        .border_1()
        .border_color(if focused { accent.opacity(0.5) } else { border })
        .text_size(px(size::WORK))
        .child(Input::new(input).appearance(false).small().px_0().py_0().disabled(!enabled))
        .into_any_element()
}

/// `StartRequest.failed`: an unreadable request, so the card says so and
/// offers nothing to press.
fn failed_start_request(message: &str) -> StartRequest {
    StartRequest {
        request: threadbear_core::reducer::StartRequestFiled { runner_name: String::new(), thread_id: String::new(), target: String::new(), text: String::new(), title: None, cwd: None, model: None, effort: None, permission_mode: None, why: None },
        target: None,
        runners: vec![],
        fingerprint: None,
        missing_folder: None,
        error: Some(message.to_owned()),
    }
}

/// `SecretRowCell.promise` and its tip.
const PROMISE: &str = "Kept out of chat, in a private file this agent's commands can read for 24 hours";
const PROMISE_TIP: &str = "The value never goes in the chat, the phone or the archive. The agent gets a private file its commands can read, deleted after 24 hours. A command that printed it would show it.";
const PLAN_INFO: &str = "Plan first: the agent reads and plans, and changes nothing until you approve. Go ahead lets it start, in the permission mode the thread had before. Send back, or anything you write, asks it for another go.";

/// Where a secret card's answer is. Kept here (not in the row, which redraws)
/// and never holding the value itself (`SecretCardState`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SecretState {
    Ready,
    Sending,
    /// It went; the card closes when the runner's approval.resolved comes.
    Sent,
    Failed(String),
}

impl ThreadPane {
    fn question_count(&self, approval_id: &str) -> usize {
        self.feed
            .model()
            .rows()
            .iter()
            .find_map(|r| match &r.content {
                threadbear_core::reducer::RowContent::Question(q) if q.request.approval_id == approval_id => Some(q.request.questions.len()),
                _ => None,
            })
            .unwrap_or(0)
    }
}
