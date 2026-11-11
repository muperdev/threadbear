// How each transcript row is drawn: RowRenderer.swift and Cells.swift. The rows
// themselves come from the reducer in threadbear-core; this is the look, in the
// order RowContent lists them. The work lines are in work.rs, diffs in diff.rs,
// cards in cards.rs and pictures in images.rs.
mod cards;
mod diff;
mod export;
mod images;
pub(crate) mod list;
pub(crate) mod markdown;
pub(crate) use markdown::reading_style;
pub(crate) mod metrics;
mod work;

use self::metrics::*;
use crate::style::{self, size, MAX_CONTENT_WIDTH, SIDE_INSET};
use crate::thread::ThreadPane;
use gpui_kit::base::{SelectableText, TextView};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use threadbear_core::reducer::{
    cost_text, duration, AgentHeader, Pending, PlanEntry, PlanStatus, PlanSummary, Row, RowContent, TextItem, TurnCompletion, TurnErrorKind, TurnOutcome, TurnTokens, UserMessage,
};

/// A `thread:` link was clicked: the app opens that thread.
pub struct OpenThreadRequest(pub String);
impl EventEmitter<OpenThreadRequest> for ThreadPane {}

/// Everything the rows hold on to between frames, so the rows themselves can
/// stay plain data from the reducer.
#[derive(Default)]
pub(crate) struct RowsState {
    /// Parsed markdown by row id, so scrolling a row away and back doesn't
    /// parse it again and a streaming reply only parses what's new.
    pub(crate) documents: HashMap<String, Entity<gpui_kit::base::TextViewState>>,
    pub(crate) document_order: Vec<String>,
    pub(crate) reading_style: Option<(bool, gpui_kit::base::TextViewStyle)>,
    /// Question cards' "Something else" fields, and their typed text.
    pub(crate) others: HashMap<String, Entity<gpui_kit::component::input::InputState>>,
    /// Secret cards' masked fields. Never kept past the card's own answer.
    pub(crate) secrets: HashMap<String, Entity<gpui_kit::component::input::InputState>>,
    pub(crate) secret_states: HashMap<String, cards::SecretState>,
    /// Approvals whose answer is on its way (`ThreadSession.answering`).
    pub(crate) answering: HashSet<String>,
    /// Calls Mike has unticked on a card with queued calls, by approval id.
    pub(crate) unticked: HashMap<String, HashSet<String>>,
    pub(crate) start_requests_loading: HashSet<String>,
    pub(crate) subscriptions: Vec<Subscription>,
    /// The working line's once-a-second redraw is queued.
    pub(crate) tick_scheduled: bool,
    /// A step lit up by a work link, for a moment.
    pub(crate) lit_row: Option<String>,
    /// The list's width at its last layout (for picture grids, which lay out
    /// to it). The list can't be asked mid-layout.
    pub(crate) list_width: f32,
    /// Where to come back to once older pages have brought its row in: the
    /// pane is new each time a thread opens, with only the newest page.
    pub(crate) pending_restore: Option<ScrollMemory>,
}

/// Where a thread was reading, kept across opening other threads (the pane is
/// new each time). `pinned` is at the bottom.
#[derive(Clone, Debug)]
pub struct ScrollMemory {
    pub pinned: bool,
    pub anchor_id: Option<String>,
    pub offset: f32,
}

static SCROLL_MEMORY: LazyLock<Mutex<HashMap<String, ScrollMemory>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn remember_scroll(thread_id: &str, memory: ScrollMemory) {
    if let Ok(mut map) = SCROLL_MEMORY.lock() {
        map.insert(thread_id.to_owned(), memory);
    }
}

pub(crate) fn recalled_scroll(thread_id: &str) -> Option<ScrollMemory> {
    SCROLL_MEMORY.lock().ok().and_then(|map| map.get(thread_id).cloned())
}

impl ThreadPane {
    pub(crate) fn render_item(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.feed.model().rows().get(ix).cloned() else { return div().into_any_element() };
        let body: AnyElement = match &row.content {
            RowContent::User(message) => self.user_row(ix, &row, message, cx),
            RowContent::Assistant(item) => self.assistant_row(ix, &row, item, cx),
            RowContent::Reasoning(item) => self.thinking_row(ix, &row, item, cx),
            RowContent::Tool(tool) => self.tool_row(ix, &row, tool, cx),
            RowContent::Images { images, generated, .. } => self.images_row(&row, images, *generated),
            RowContent::Approval(item) if item.resolution.is_some() => self.settled_approval_row(ix, &row, item, cx),
            RowContent::Approval(item) if !item.queued().is_empty() => self.stacked_approval_card(ix, &row, item, window, cx),
            RowContent::Approval(item) => self.approval_card(ix, &row, item, window, cx),
            RowContent::Question(item) => self.question_card(ix, &row, item, window, cx),
            RowContent::Secret(item) if item.resolution.is_some() => self.settled_secret_row(ix, &row, item, cx),
            RowContent::Secret(item) => self.secret_card(ix, &row, item, window, cx),
            RowContent::Review(review) => self.review_card(ix, &row, review, cx),
            RowContent::Plan(entries) => self.plan_row(ix, entries, cx),
            RowContent::PlanCard(card) => self.plan_card(ix, &row, card, window, cx),
            RowContent::Footer(done) => self.footer_row(done, cx),
            // The open card is drawn in the composer (ComposerCard), joined to the
            // box it's answered in. Height nothing here, as the Swift row is.
            RowContent::Asks(asks) if asks.is_open => div().into_any_element(),
            RowContent::Asks(asks) => self.asks_row(ix, asks, cx),
            RowContent::Note(note) if row.in_group => self.grouped_note_row(ix, &row, note, cx),
            RowContent::Note(note) => self.note_row(&row, note, cx),
            RowContent::Error { message, kind } => self.error_row(message, *kind, cx),
            RowContent::Group(group) => self.group_row(ix, &row, group, cx),
            RowContent::Working(status) => self.working_row(status.started_at, window, cx),
            RowContent::Background(status) => self.background_row(status, cx),
            RowContent::Gap(gap) => self.gap_row(gap, cx),
            RowContent::Changes(changes) => self.changes_card(ix, &row, changes, cx),
        };
        // A step a work link went to: an accent rail and tint for a moment
        // (`TranscriptController.light`). The right-click menu goes over
        // everything else (`menu(forRow:)`).
        let lit = self.rows.lit_row.as_deref() == Some(row.id.as_str());
        let (accent, weak, menu_row) = (style::control_accent(cx), cx.weak_entity(), row.clone());
        div()
            .id(("row", ix))
            .w_full()
            .when(lit, |d| d.rounded(px(6.)).bg(accent.opacity(0.24)).border_l(px(3.)).border_color(accent))
            .child(body)
            .context_menu(move |menu, _, _| Self::row_context_menu(menu_row.clone(), menu, weak.clone()))
            .into_any_element()
    }

    /// `userText`: Mike's words, or for a message answering asks or a review
    /// each question small and quiet with his answer under it.
    fn user_row(&mut self, ix: usize, row: &Row, message: &UserMessage, cx: &mut Context<Self>) -> AnyElement {
        if message.shows_as_note() {
            return self.runner_note_row(message, cx);
        }
        if let Some(header) = message.agent_header() {
            return self.agent_message_row(ix, &header, cx);
        }
        let bubble_bg = style::bubble_background(cx);
        let label = style::label_text(cx);
        let quiet = style::quiet_text(cx);
        let faint = style::faint_text(cx);
        let pending = message.pending;
        let faded = pending.is_some();
        let mut col = v_flex().items_end().pt(px(USER_TOP)).pb(px(USER_BOTTOM));
        if !message.images.is_empty() {
            col = col.child(self.user_images(&message.images));
        }
        let show_text = !message.text.is_empty() || message.images.is_empty();
        if show_text {
            let mut line = h_flex().w_full().justify_end().items_end().gap(px(6.));
            match pending {
                // A spinner while it's on its way, a clock when queued behind
                // the running turn; the note below says the rest.
                Some(Pending::Sending) => line = line.child(mark_box().child(self.spinner(14., cx))),
                Some(Pending::Queued) => line = line.child(mark_box().child(work::icon("clock", 14., quiet))),
                _ => {}
            }
            let text = if message.text.is_empty() { " ".to_string() } else { message.text.clone() };
            let answered = !message.answers.is_empty() || message.review.is_some();
            let bubble = div()
                .max_w(relative(BUBBLE_SHARE))
                .px(px(BUBBLE_PAD_H))
                .py(px(BUBBLE_PAD_V))
                .rounded(px(BUBBLE_RADIUS))
                .bg(bubble_bg)
                .text_size(px(size::BODY))
                .line_height(px(size::BODY_LINE))
                .text_color(label)
                .when(faded, |d| d.opacity(0.5))
                .when(!answered, |d| d.child(SelectableText::new(("user-text", ix), text)))
                .when(answered, |d| d.child(answers_body(ix, message, cx)));
            col = col.child(line.child(bubble));
        }
        if pending == Some(Pending::WaitingForRunner) {
            col = col.child(div().mt(px(WAITING_NOTE_GAP)).pr(px(4.)).h(px(WAITING_NOTE_HEIGHT)).text_size(px(size::SMALL)).text_color(faint).child("Runner restarting. Sends when it's back."));
        }
        row_wrap(row, col)
    }

    /// `runnerMessageText` and `scheduledMessageText`: a centred quiet note
    /// saying where the message came from, then the message itself.
    fn runner_note_row(&mut self, message: &UserMessage, cx: &mut Context<Self>) -> AnyElement {
        let faint = style::faint_text(cx);
        let quiet = style::quiet_text(cx);
        let (mut header, body) = if message.is_from_schedule() {
            match message.scheduled_header() {
                Some(h) => (format!("Scheduled: {} · {}", h.title, h.when_text), h.body),
                None => ("Sent by a schedule".to_owned(), message.text.clone()),
            }
        } else {
            ("Sent by the runner after it restarted".to_owned(), message.text.clone())
        };
        if message.is_from_schedule() {
            // Clock glyph in the text's place (`symbol("clock", ...)`).
            header.insert_str(0, "\u{23F2}\u{FE0E} ");
        }
        let body = body.trim().to_owned();
        let col = v_flex()
            .pt(px(RUNNER_TOP))
            .pb(px(RUNNER_BOTTOM))
            .items_center()
            .child(div().text_size(px(size::SMALL)).line_height(px(17.)).text_color(faint).text_center().child(header))
            .when(!body.is_empty(), |d| d.child(div().text_size(px(size::LABEL)).line_height(px(19.)).text_color(quiet).text_center().child(body)));
        h_flex().w_full().px(px(SIDE_INSET)).justify_center().child(div().w_full().max_w(px(MAX_CONTENT_WIDTH)).child(col)).into_any_element()
    }

    /// `agentMessageText`: another thread's agent wrote this, so a card on the
    /// left that can't pass for Mike's bubble, headed by that thread.
    fn agent_message_row(&mut self, ix: usize, header: &AgentHeader, cx: &mut Context<Self>) -> AnyElement {
        let faint = style::faint_text(cx);
        let (card_bg, border) = (style::card_background(cx), style::card_border(cx));
        let title = match &header.moved_to {
            Some(folder) => format!("Left by this thread's agent when it moved to {folder}"),
            None if header.title.is_empty() => format!("From the agent in {}", header.thread_id),
            None => format!("From the agent in {}", header.title),
        };
        let body = header.body.trim().to_owned();
        let doc = (!body.is_empty()).then(|| self.markdown_state(&format!("agent-{ix}"), &body, cx));
        let text_style = self.reading_style(cx);
        let card = v_flex()
            .p(px(CARD_PAD))
            .rounded(px(BUBBLE_RADIUS))
            .bg(card_bg)
            .border_1()
            .border_color(border)
            .child(div().mb(px(8.)).text_size(px(size::SMALL)).line_height(px(17.)).text_color(faint).child(title))
            .when_some(doc, |d, doc| d.child(div().text_size(px(size::BODY)).line_height(px(size::BODY_LINE)).child(self.linked_text(&doc, &format!("agent-{ix}"), cx).selectable(true).style(text_style).code_block_highlighter(plain_code_blocks))));
        h_flex().w_full().px(px(SIDE_INSET)).justify_center().child(div().w_full().max_w(px(MAX_CONTENT_WIDTH)).pt(px(USER_TOP)).pb(px(USER_BOTTOM)).child(card)).into_any_element()
    }

    /// A row's markdown, its links going to `link_clicked` (and from there
    /// through the open gate), never to the system's opener as gpui's own
    /// text view would send them.
    pub(crate) fn linked_text(&self, doc: &Entity<gpui_kit::base::TextViewState>, row_id: &str, cx: &mut Context<Self>) -> TextView {
        let weak = cx.weak_entity();
        let row_id = row_id.to_owned();
        crate::open_gate::text_view(doc, self.origin(), &self.summary.cwd).on_link_click(move |url: &SharedString, _event: &ClickEvent, window: &mut Window, cx: &mut App| {
            let (url, row_id) = (url.to_string(), row_id.clone());
            let _ = weak.update(cx, |this, cx| this.link_clicked(&url, &row_id, window, cx));
        })
    }

    /// The reading column's text: markdown, selectable, links going where they
    /// go. `row.rs`'s `TextItem::text` is the whole reply so far; `prepare`
    /// escapes work links and `set_text` appends what's new.
    fn assistant_row(&mut self, _ix: usize, row: &Row, item: &TextItem, cx: &mut Context<Self>) -> AnyElement {
        let source = markdown::prepare(&item.text);
        let doc = self.markdown_state(&item.item_id, &source, cx);
        let text_style = self.reading_style(cx);
        let text = self.linked_text(&doc, &row.id, cx).selectable(true).style(text_style).code_block_highlighter(plain_code_blocks);
        row_wrap(row, div().pt(px(ROW_TOP)).pb(px(10.)).text_size(px(size::BODY)).line_height(px(size::BODY_LINE)).text_color(style::label_text(cx)).child(text))
    }

    /// The plan an agent is working through (`planText`): a title and its
    /// steps, in a card.
    fn plan_row(&mut self, ix: usize, entries: &[PlanEntry], cx: &mut Context<Self>) -> AnyElement {
        let (card_bg, border, label) = (style::card_background(cx), style::card_border(cx), style::label_text(cx));
        let card = v_flex()
            .mt(px(ROW_TOP + 2.))
            .mb(px(ROW_BOTTOM + 2.))
            .p(px(CARD_PAD))
            .rounded(px(10.))
            .bg(card_bg)
            .border_1()
            .border_color(border)
            .child(div().pb(px(8.)).text_size(px(size::WORK)).line_height(px(size::WORK_LINE)).font_weight(FontWeight::MEDIUM).text_color(label).child(PlanSummary::title(entries)))
            .child(plan_steps(ix, entries, cx));
        h_flex().w_full().px(px(SIDE_INSET)).justify_center().child(div().w_full().max_w(px(MAX_CONTENT_WIDTH)).child(card)).into_any_element()
    }

    /// `footerText`: how long it took and what it cost; a failure the runner
    /// doesn't explain says why after it.
    fn footer_row(&mut self, done: &TurnCompletion, cx: &mut Context<Self>) -> AnyElement {
        let failed = done.outcome == TurnOutcome::Failed;
        let color = if failed { style::failed_text(cx) } else { style::faint_text(cx) };
        h_flex()
            .w_full()
            .px(px(SIDE_INSET))
            .justify_center()
            .child(div().w_full().max_w(px(MAX_CONTENT_WIDTH)).h(px(FOOTER_HEIGHT)).flex().items_center().text_size(px(size::SMALL)).text_color(color).child(div().truncate().child(footer_text(done))))
            .into_any_element()
    }

    /// A quiet centred system note.
    fn note_row(&mut self, row: &Row, note: &str, cx: &mut Context<Self>) -> AnyElement {
        let quiet = style::quiet_text(cx);
        let content = div().py(px(ROW_TOP)).w_full().flex().justify_center().child(div().text_size(px(size::SMALL)).line_height(px(17.)).text_color(quiet).text_center().child(note.to_owned()));
        row_wrap(row, content)
    }

    /// A failed turn's message in its pink box, and what to do about it when
    /// the runner said why.
    fn error_row(&mut self, message: &str, kind: Option<TurnErrorKind>, cx: &mut Context<Self>) -> AnyElement {
        let (bg, failed, quiet) = (style::error_background(cx), style::failed_text(cx), style::quiet_text(cx));
        let advice = kind.and_then(error_advice);
        let box_ = v_flex()
            .mt(px(ROW_TOP))
            .mb(px(ROW_BOTTOM))
            .p(px(BOX_PAD))
            .rounded(px(10.))
            .bg(bg)
            .child(div().text_size(px(size::BODY)).line_height(px(size::BODY_LINE)).text_color(failed).child(message.to_owned()))
            .when_some(advice, |d, advice| d.child(div().text_size(px(size::DETAIL)).line_height(px(19.)).text_color(quiet).child(advice)));
        h_flex().w_full().px(px(SIDE_INSET)).justify_center().child(div().w_full().max_w(px(MAX_CONTENT_WIDTH)).child(box_)).into_any_element()
    }

    pub(crate) fn spinner(&self, side: f32, cx: &App) -> impl IntoElement {
        let color = style::quiet_text(cx);
        gpui_kit::component::spinner::Spinner::new().with_size(gpui_kit::component::Size::Size(px(side))).color(color)
    }

    /// The reading column's text style, one per appearance (see `reading_style`).
    pub(crate) fn reading_style(&mut self, cx: &mut Context<Self>) -> gpui_kit::base::TextViewStyle {
        let dark = cx.theme().is_dark();
        match &self.rows.reading_style {
            Some((d, style)) if *d == dark => style.clone(),
            _ => {
                let built = markdown::reading_style(cx);
                self.rows.reading_style = Some((dark, built.clone()));
                built
            }
        }
    }

    // The asks card is the composer agent's: unchanged from before (`Decisions`
    // and `asksText` land with the composer's card).
    fn asks_row(&mut self, ix: usize, asks: &threadbear_core::reducer::AsksCard, cx: &mut Context<Self>) -> AnyElement {
        let (muted, border, warning) = {
            let theme = cx.theme();
            (theme.muted_foreground, theme.border, theme.warning)
        };
        let column = |el: Div| h_flex().w_full().justify_center().child(el.w_full().max_w(px(760.)));
        if asks.is_done() {
            let mut col = v_flex().gap_1().child(div().font_semibold().child("Done"));
            for next in &asks.next {
                col = col.child(div().text_sm().text_color(muted).child(next.clone()));
            }
            return column(card(border).child(col)).into_any_element();
        }
        let mut card = card(if asks.is_open { warning } else { border });
        for (ai, ask) in asks.asks.iter().enumerate() {
            let mut block = v_flex().gap_1().child(div().font_semibold().child(ask.action.clone()));
            if let Some(why) = &ask.why {
                block = block.child(div().text_sm().text_color(muted).child(crate::open_gate::markdown_view(("why", ix * 10 + ai), why.clone(), self.origin(), &self.summary.cwd)));
            }
            if asks.is_open && !ask.options.is_empty() {
                let mut buttons = h_flex().gap_2().flex_wrap().pt_1();
                for (oi, option) in ask.options.iter().enumerate() {
                    let text = format!("{}\n{}", ask.action, option);
                    let answers = json!([{ "action": ask.action, "answer": option }]);
                    buttons = buttons.child(Button::new(("ask", ix * 100 + ai * 10 + oi)).small().label(option.clone()).when(ask.suggested == Some(oi), |b| b.primary()).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| this.send(text.clone(), Some(answers.clone()), cx),
                    )));
                }
                block = block.child(buttons);
            }
            card = card.child(block);
        }
        column(card).into_any_element()
    }
}

/// The left mark's box, as tall as the bubble's last line.
fn mark_box() -> Div {
    div().size(px(22.)).flex().items_center().justify_center()
}

/// The reading column, centred, with an open group's items indented
/// (`RowMetrics.column`).
pub(crate) fn row_wrap(row: &Row, inner: impl IntoElement) -> AnyElement {
    h_flex()
        .w_full()
        .px(px(SIDE_INSET))
        .justify_center()
        .child(div().w_full().min_w_0().max_w(px(MAX_CONTENT_WIDTH)).when(row.in_group, |d| d.pl(px(GROUP_INDENT))).child(inner))
        .into_any_element()
}

/// The body of a message that answered asks or a review (`userWords`): each
/// question small and quiet, its answer as his own words, then the rest.
fn answers_body(ix: usize, message: &UserMessage, cx: &App) -> Div {
    let question = style::secondary_label(cx);
    let lines: Vec<(String, String)> = if let Some(review) = &message.review {
        review.lines.iter().map(|l| (l.label.clone(), l.text.clone())).collect()
    } else {
        message.answers.iter().map(|a| (a.action.clone(), a.answer.clone())).collect()
    };
    let mut body = v_flex().gap(px(8.));
    for (i, (label, text)) in lines.iter().enumerate() {
        body = body.child(
            v_flex()
                .when(!label.is_empty(), |d| d.child(div().text_size(px(size::SMALL)).line_height(px(18.)).text_color(question).child(label.clone())))
                .child(SelectableText::new(("answer-text", ix * 1000 + i), text.clone())),
        );
    }
    body
}

pub(crate) fn plan_steps(ix: usize, entries: &[PlanEntry], cx: &App) -> Div {
    let (added, accent, faint, label, secondary) = (style::added_text(cx), style::control_accent(cx), style::faint_text(cx), style::label_text(cx), style::secondary_label(cx));
    let mut col = v_flex().gap(px(4.)).text_size(px(size::BODY)).line_height(px(22.));
    for (i, entry) in entries.iter().enumerate() {
        let (mark, color, text_color): (&str, Hsla, Hsla) = match entry.status {
            PlanStatus::Completed => ("✓", added, secondary),
            PlanStatus::InProgress => ("●", accent, label),
            PlanStatus::Pending => ("○", faint, label),
        };
        col = col.child(
            h_flex()
                .items_start()
                .child(div().flex_none().w(px(24.)).text_color(color).child(mark))
                .child(
                    div()
                        .id(("plan-step", ix * 1000 + i))
                        .flex_1()
                        .min_w_0()
                        .text_color(text_color)
                        .when(entry.status == PlanStatus::Completed, |d| d.line_through())
                        .child(entry.text.clone()),
                ),
        );
    }
    col
}

/// `RowRenderer.errorAdvice`.
fn error_advice(kind: TurnErrorKind) -> Option<&'static str> {
    match kind {
        TurnErrorKind::ContextWindow => Some("The conversation is too long for this model. Start a new thread to carry on."),
        TurnErrorKind::Auth => Some("Its login was refused. Sign in again from the line above, then send again."),
        TurnErrorKind::Network => Some("The internet dropped. This thread carries on by itself once the connection is back."),
        TurnErrorKind::UsageLimit => None,
    }
}

/// `footerText`: "12s · 1.7k out · 381k in".
fn footer_text(done: &TurnCompletion) -> String {
    let mut parts = vec![match done.outcome {
        TurnOutcome::Completed => duration(done.duration_ms),
        TurnOutcome::Interrupted => format!("Stopped after {}", duration(done.duration_ms)),
        TurnOutcome::Failed => format!("Failed after {}", duration(done.duration_ms)),
    }];
    if let Some(tokens) = TurnTokens::from_usage(done.usage.as_ref()) {
        parts.push(format!("{} out · {} in", short_tokens(tokens.output), short_tokens(tokens.read())));
    } else if let Some(cost) = cost_text(done.cost_usd) {
        parts.push(cost);
    }
    let mut text = parts.join(" · ");
    if let Some(error) = done.error.as_deref().filter(|e| !e.is_empty() && done.error_kind.is_none()) {
        text.push_str(": ");
        text.push_str(error.lines().next().unwrap_or(""));
    }
    text
}

/// `UsageStatsMath.tokens`: 950, "1.7k", "381k", "4.2M".
fn short_tokens(n: i64) -> String {
    let value = n as f64;
    let short = |v: f64, suffix: &str| {
        let text = if v < 10. { format!("{v:.1}") } else { format!("{}", v.round() as i64) };
        format!("{}{suffix}", text.strip_suffix(".0").unwrap_or(&text))
    };
    if value >= 1e9 {
        short(value / 1e9, "B")
    } else if value >= 1e6 {
        short(value / 1e6, "M")
    } else if value >= 1e3 {
        short(value / 1e3, "k")
    } else {
        n.to_string()
    }
}

fn card(border: Hsla) -> Div {
    v_flex().gap_3().p_3p5().rounded_xl().border_1().border_color(border)
}

/// A tooltip builder for `.tooltip(...)`, Every control gets one.
/// The transcript's code blocks stay plain, as the Swift app's do; the kit's
/// highlighter is for the file preview (files.rs).
pub(crate) fn plain_code_blocks(_: &gpui_kit::base::text::CodeBlock) -> Vec<(std::ops::Range<usize>, gpui_kit::HighlightStyle)> {
    Vec::new()
}

impl crate::thread::ThreadPane {
    /// The pictures of the last row with at least `count` of them, for
    /// scripts (the `tile` command presses one of them).
    #[cfg(feature = "automation")]
    pub(crate) fn last_row_images(&self, count: usize) -> Result<Vec<threadbear_core::reducer::ImageRef>, String> {
        self.feed
            .model()
            .rows()
            .iter()
            .rev()
            .find_map(|row| match &row.content {
                RowContent::Images { images, .. } if images.len() >= count.max(1) => Some(images.clone()),
                RowContent::User(message) if message.images.len() >= count.max(1) => Some(message.images.clone()),
                _ => None,
            })
            .ok_or_else(|| format!("no row of {count} pictures in this thread"))
    }
}

pub(crate) fn tip(text: &str) -> impl Fn(&mut Window, &mut App) -> AnyView + use<> {
    let text: SharedString = text.to_owned().into();
    move |window, cx| Tooltip::new(text.clone()).build(window, cx)
}

