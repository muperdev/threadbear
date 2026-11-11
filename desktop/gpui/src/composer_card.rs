// The open card at the top of the composer (ComposerCard.swift, with the
// card's text and buttons from RowRenderer.asksText): Needs you with its asks,
// or Done with its follow-ups, drawn as ordinary elements. The Swift app's card
// is one attributed string with buttons drawn in it; here each ask is a block,
// which is how the transcript's rows are built too.
use crate::composer::Composer;
use crate::style;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Enter, InputEvent, Textarea, TextareaState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;
use threadbear_core::composer::{self, OWN_ANSWER};
use threadbear_core::reducer::{Ask, AskNote, AsksCard, RowContent};

/// What Mike has picked on the open card, kept with the composer so it
/// survives moving to another thread and back (AppModel's askPicks, nextPicks
/// and collapsedCards).
#[derive(Default)]
pub(crate) struct CardState {
    /// The card these are for.
    pub(crate) seq: Option<i64>,
    pub(crate) picks: BTreeMap<i64, i64>,
    pub(crate) notes: BTreeMap<i64, AskNote>,
    pub(crate) next_picks: BTreeSet<i64>,
    pub(crate) explain: BTreeSet<i64>,
    /// Folded to its heading (ComposerCard's chevron).
    pub(crate) collapsed: bool,
    /// The ask a paged card is showing.
    pub(crate) page: usize,
    /// The boxes under "Something else", by ask.
    inputs: HashMap<usize, Entity<TextareaState>>,
    subs: Vec<Subscription>,
}

impl Composer {
    /// The open card the thread's transcript has (TranscriptModel.openCard),
    /// as long as it's open.
    pub(crate) fn open_card(&self, cx: &App) -> Option<AsksCard> {
        let pane = self.pane.upgrade()?;
        let row = pane.read(cx).feed.model().open_card()?;
        let RowContent::Asks(card) = &row.content else { return None };
        card.is_open.then(|| card.clone())
    }

    /// The card's state, reset when a new card turns up: it opens on its
    /// first unanswered ask.
    fn sync_card(&mut self, card: &AsksCard) {
        if self.card.seq != Some(card.seq) {
            self.card = CardState { seq: Some(card.seq), ..Default::default() };
        }
    }

    /// Hands the picks to the reducer, so the rest of the app reads them off
    /// the row as the Swift app's does (setAskPicks and friends), and puts the
    /// picks back after the pane has been remade.
    pub(crate) fn push_card_state(&self, cx: &mut App) {
        let (Some(pane), Some(seq)) = (self.pane.upgrade(), self.card.seq) else { return };
        let (picks, notes, next, explain) = (self.card.picks.clone(), self.card.notes.clone(), self.card.next_picks.clone(), self.card.explain.clone());
        pane.update(cx, |p, cx| p.set_card_picks(seq, picks, notes, next, explain, cx));
    }

    /// Picks an answer, or unpicks it when it's already the pick
    /// (AppModel.pickAnswer). A picked answer on a paged card moves on to the
    /// next one still waiting, a beat later so the tick shows first.
    pub(crate) fn pick_answer(&mut self, ask: usize, option: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self.open_card(cx) else { return };
        self.sync_card(&card);
        let key = ask as i64;
        let picked = self.card.picks.get(&key) == Some(&option);
        if picked {
            self.card.picks.remove(&key);
        } else {
            self.card.picks.insert(key, option);
        }
        self.push_card_state(cx);
        cx.notify();
        if !picked && option == OWN_ANSWER {
            // A box on another page of a paged card can't take the cursor
            // (only a script reaches this), so it stays in the composer.
            if !is_paged(&card) || self.card.page == ask {
                self.focus_answer_box(ask, window, cx);
            }
            return;
        }
        if !picked && option != OWN_ANSWER && is_paged(&card) {
            let page = self.card.page;
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(300)).await;
                this.update_in(cx, |this, _window, cx| {
                    if this.card.page != page {
                        return;
                    }
                    if let Some(card) = this.open_card(cx) {
                        if let Some(next) = composer::next_unanswered(page, card.asks.len(), &this.card.picks, false) {
                            this.card.page = next;
                            cx.notify();
                        }
                    }
                })
                .ok();
            })
            .detach();
        } else if !picked {
            self.focus(window, cx);
        }
    }

    fn set_note(&mut self, ask: usize, text: String, cx: &mut Context<Self>) {
        let note = self.card.notes.entry(ask as i64).or_default();
        note.text = text;
        self.push_card_state(cx);
        cx.notify();
    }

    fn toggle_follow_up(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let key = index as i64;
        if !self.card.next_picks.remove(&key) {
            self.card.next_picks.insert(key);
        }
        self.push_card_state(cx);
        cx.notify();
        self.focus(window, cx);
    }

    fn toggle_explain(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let key = index as i64;
        if !self.card.explain.remove(&key) {
            self.card.explain.insert(key);
        }
        self.push_card_state(cx);
        cx.notify();
        self.focus(window, cx);
    }

    fn set_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.card.page = page;
        cx.notify();
    }

    fn toggle_collapsed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.card.collapsed = !self.card.collapsed;
        cx.notify();
        self.focus(window, cx);
    }

    fn archive_thread(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| app.toggle_archive(window, cx));
        }
    }

    /// The box under an ask's "Something else" (AskAnswerBox): its own text
    /// field, kept for as long as the card is open.
    fn answer_box(&mut self, ask: usize, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextareaState> {
        if let Some(input) = self.card.inputs.get(&ask) {
            return input.clone();
        }
        let existing = self.card.notes.get(&(ask as i64)).map(|n| n.text.clone()).unwrap_or_default();
        let input = cx.new(|cx| TextareaState::new(window, cx).placeholder("Say something else…").auto_grow(1, 8).submit_on_enter(true).default_value(existing));
        let sub = cx.subscribe_in(&input, window, move |this, input, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => {
                let text = input.read(cx).value().to_string();
                this.set_note(ask, text, cx);
            }
            InputEvent::PressEnter { secondary, shift: false } => this.answer_box_return(ask, *secondary, window, cx),
            _ => {}
        });
        self.card.subs.push(sub);
        self.card.inputs.insert(ask, input.clone());
        input
    }

    fn focus_answer_box(&mut self, ask: usize, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.answer_box(ask, window, cx);
        input.update(cx, |s, cx| s.focus(window, cx));
    }

    /// Return in a Something else box goes on to the next ask still
    /// unanswered, and sends only once there's none; ⌘Return sends whatever's
    /// answered (TranscriptController.answerBoxReturn).
    fn answer_box_return(&mut self, ask: usize, command: bool, window: &mut Window, cx: &mut Context<Self>) {
        if command || !self.show_unanswered(ask, true, window, cx) {
            self.send(window, cx);
        }
    }

    /// Puts the next ask with nothing picked in front: its page on a paged
    /// card, or the Swift app's beep when they're all in sight already. False
    /// when none is left (TranscriptController.showUnanswered).
    pub(crate) fn show_unanswered(&mut self, ask: usize, or_this: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(card) = self.open_card(cx) else { return false };
        let filled = composer::filled_picks(&self.card.picks, &self.card.notes);
        let Some(next) = composer::next_unanswered(ask, card.asks.len(), &filled, or_this) else { return false };
        if is_paged(&card) && self.card.page != next {
            self.card.page = next;
            cx.notify();
            self.focus(window, cx);
        } else {
            // On a card showing every ask it's in sight already; the box says
            // so by staying put (the Swift app beeps here).
        }
        true
    }

    /// Return with nothing typed and picks on the card
    /// (ComposerView.returnWhenEmpty): the next unanswered ask, or send them.
    pub(crate) fn return_with_picks(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.card.picks.is_empty() && self.card.next_picks.is_empty() && self.card.explain.is_empty() {
            return false;
        }
        if !self.card.picks.is_empty() && self.show_unanswered(self.card.page, true, window, cx) {
            return true;
        }
        self.send(window, cx);
        true
    }

    /// What the card puts in the message (AppModel.send): the answers go
    /// first, each ask then its answer, and the box's own words after them,
    /// or the ticked follow-ups. None when the card adds nothing.
    pub(crate) fn card_message(&self, words: &str, cx: &App) -> Option<(String, Vec<threadbear_core::reducer::ImageRef>, Value)> {
        let card = self.open_card(cx)?;
        if card.is_done() {
            let ticked: Vec<String> = card.next.iter().enumerate().filter(|(i, _)| self.card.next_picks.contains(&(*i as i64))).map(|(_, t)| t.clone()).collect();
            let explain: Vec<String> = card.next.iter().enumerate().filter(|(i, _)| self.card.explain.contains(&(*i as i64))).map(|(_, t)| t.clone()).collect();
            if ticked.is_empty() && explain.is_empty() {
                return None;
            }
            return Some((composer::follow_up_message(&ticked, &explain, words), Vec::new(), Value::Null));
        }
        let answers = composer::answers(&card.asks, &self.card.picks, &self.card.notes);
        if answers.is_empty() {
            return None;
        }
        let answers_json: Vec<Value> = answers.iter().map(|(_, a)| json!({ "action": a.action, "answer": a.answer })).collect();
        let images = composer::note_images(&card.asks, &self.card.picks, &self.card.notes);
        Some((composer::answers_message(&card.asks, &self.card.picks, &self.card.notes, words), images, Value::Array(answers_json)))
    }

    /// What the box's card asks for next: its placeholder, by what's picked
    /// (ComposerView.placeholder).
    pub(crate) fn card_placeholder(&self, cx: &App) -> &'static str {
        let needs_you = self.open_card(cx).is_some_and(|c| !c.is_done());
        if !self.card.picks.is_empty() || !self.card.next_picks.is_empty() || !self.card.explain.is_empty() {
            "Add anything else…"
        } else if needs_you {
            "Or say something else…"
        } else {
            "Message"
        }
    }

    /// Everything goes as one message, so the picks go too when it's sent.
    pub(crate) fn clear_picks(&mut self, cx: &mut App) {
        self.card.picks.clear();
        self.card.notes.clear();
        self.card.next_picks.clear();
        self.card.explain.clear();
        self.card.inputs.clear();
        self.card.subs.clear();
        self.push_card_state(cx);
    }

    // ---- Drawing ----

    /// The card, over a line, at the top of the box.
    pub(crate) fn render_card(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let card = self.open_card(cx)?;
        self.sync_card(&card);
        let body = if card.is_done() { self.render_done_card(&card, cx) } else { self.render_asks_card(&card, window, cx) };
        let max_height = (f32::from(window.viewport_size().height) * 0.45).max(200.);
        let column = div().id("composer-card").w_full().px(px(style::COMPOSER_PADDING)).pt(px(8.)).pb(px(10.)).max_h(px(max_height)).overflow_y_scroll().child(body);
        Some(v_flex().w_full().child(column).child(div().w_full().h(px(1.)).bg(style::hairline(cx))).into_any_element())
    }

    /// RowRenderer.doneText: Done, with Archive at the right, and the agent's
    /// follow-ups under "Or carry on", each a row with a tick and a "?".
    fn render_done_card(&mut self, card: &AsksCard, cx: &mut Context<Self>) -> AnyElement {
        let mut column = v_flex().w_full();
        let this = cx.entity().downgrade();
        column = column.child(
            h_flex()
                .w_full()
                .h(px(38.))
                .justify_between()
                .child(h_flex().gap(px(6.)).text_size(px(style::size::CARD_TITLE)).font_weight(FontWeight::SEMIBOLD).child(Icon::from(IconName::Check).with_size(px(14.)).text_color(hsla(0.38, 0.65, 0.5, 1.))).child("Done"))
                .child(card_button(("done-archive", 0usize), "Archive", ButtonLook::Plain, "Archive this thread", cx).on_click({
                    let this = this.clone();
                    move |_, window, cx| {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| this.archive_thread(window, cx));
                        }
                    }
                })),
        );
        if let Some(ends) = &card.loose_ends {
            if !ends.is_empty() {
                let open = ends.iter().any(|e| e.kind != "booked");
                column = column.child(small_label(if open { "Still open" } else { "Coming back" }, cx));
                for end in ends {
                    column = column.child(div().pt(px(4.)).pl(px(8.)).text_size(px(style::size::COMPOSER)).child(end.text.clone()));
                }
            }
        }
        if !card.next.is_empty() {
            column = column.child(small_label("Or carry on", cx));
            for (index, follow_up) in card.next.iter().enumerate() {
                let (ticked, explained) = (self.card.next_picks.contains(&(index as i64)), self.card.explain.contains(&(index as i64)));
                let this2 = this.clone();
                let this3 = this.clone();
                column = column.child(
                    h_flex()
                        .id(("follow-up", index))
                        .w_full()
                        .items_start()
                        .gap(px(8.))
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(style::hover_fill(cx)))
                        .tooltip(|window, cx| Tooltip::new("Send this with your next message").build(window, cx))
                        .on_click(move |_, window, cx| {
                            if let Some(this) = this2.upgrade() {
                                this.update(cx, |this, cx| this.toggle_follow_up(index, window, cx));
                            }
                        })
                        .child(div().pt(px(4.)).child(tick_circle(ticked, cx)))
                        .child(div().flex_1().min_w_0().text_size(px(style::size::COMPOSER)).line_height(px(style::size::BODY_LINE)).child(follow_up.clone()))
                        .child(
                            div()
                                .id(("follow-up-explain", index))
                                .size(px(22.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|s| s.bg(style::hover_fill(cx)))
                                .child(Icon::from(IconName::CircleQuestionMark).with_size(px(14.)).text_color(if explained { style::accent(cx) } else { style::quiet_text(cx) }))
                                .tooltip(|window, cx| Tooltip::new("Ask what this means").build(window, cx))
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    if let Some(this) = this3.upgrade() {
                                        this.update(cx, |this, cx| this.toggle_explain(index, window, cx));
                                    }
                                }),
                        ),
                );
            }
        }
        column.into_any_element()
    }

    /// RowRenderer.asksText: the heading, then each ask with its answers. Over
    /// the height a card can take, one ask at a time with ‹ 1 of 3 ›.
    fn render_asks_card(&mut self, card: &AsksCard, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let collapsed = self.card.collapsed;
        let paged = !collapsed && is_paged(card);
        let page = self.card.page.min(card.asks.len().saturating_sub(1));
        let decisions = card.asks.iter().all(|a| !a.options.is_empty());
        let words = if !decisions {
            "Needed from you"
        } else if card.asks.len() == 1 {
            "Decision needed"
        } else {
            "Decisions needed"
        };
        let this = cx.entity().downgrade();
        let collapse_this = this.clone();
        let mut heading = h_flex().w_full().min_h(px(28.)).justify_between().child(
            h_flex().gap(px(6.)).text_size(px(style::size::CARD_TITLE)).font_weight(FontWeight::SEMIBOLD).child(Icon::from(IconName::Hand).with_size(px(14.)).text_color(style::needs_you(cx))).child(words),
        );
        let mut right = h_flex().gap(px(2.));
        if paged {
            let (this_back, this_next) = (this.clone(), this.clone());
            right = right
                .child(pager_arrow("‹", page > 0, "Previous ask", ("page-back", 0usize), cx, move |_, _, cx| {
                    if let Some(this) = this_back.upgrade() {
                        this.update(cx, |this, cx| this.set_page(page.saturating_sub(1), cx));
                    }
                }))
                .child(div().px(px(4.)).text_size(px(style::size::LABEL)).text_color(style::quiet_text(cx)).child(format!("{} of {}", page + 1, card.asks.len())))
                .child(pager_arrow("›", page + 1 < card.asks.len(), "Next ask", ("page-next", 0usize), cx, move |_, _, cx| {
                    if let Some(this) = this_next.upgrade() {
                        this.update(cx, |this, cx| this.set_page(page + 1, cx));
                    }
                }));
        }
        right = right.child(
            div()
                .id("card-collapse")
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|s| s.bg(style::hover_fill(cx)))
                .child(Icon::from(if collapsed { IconName::ChevronUp } else { IconName::ChevronDown }).with_size(px(12.)).text_color(style::quiet_text(cx)))
                .tooltip(move |window, cx| Tooltip::new(if collapsed { "Expand" } else { "Collapse" }).build(window, cx))
                .on_click(move |_, window, cx| {
                    if let Some(this) = collapse_this.upgrade() {
                        this.update(cx, |this, cx| this.toggle_collapsed(window, cx));
                    }
                }),
        );
        heading = heading.child(right);
        let mut column = v_flex().w_full().child(heading);
        if collapsed {
            return column.into_any_element();
        }
        let numbered = card.asks.len() > 1;
        for (index, ask) in card.asks.iter().enumerate() {
            if paged && index != page {
                continue;
            }
            column = column.child(self.render_ask(index, ask, numbered, window, cx));
        }
        column.into_any_element()
    }

    fn render_ask(&mut self, index: usize, ask: &Ask, numbered: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let picked = self.card.picks.get(&(index as i64)).copied();
        let choices = composer::choices(ask);
        let indent = if numbered { px(20.) } else { px(0.) };
        let paragraph = ask_paragraph(ask, index, numbered);
        let mut block = v_flex().w_full().pt(px(8.));
        // Its links through the open gate, from the thread's folder; with no
        // thread to say, or another runner's, no file of it opens here.
        let thread = self.chip_data(cx).summary;
        let origin = if thread.as_ref().is_some_and(|t| !t.is_remote()) { crate::open_gate::Origin::ThisMac } else { crate::open_gate::Origin::OtherMac };
        let cwd = thread.map(|t| t.cwd).unwrap_or_default();
        block = block.child(div().text_size(px(style::size::COMPOSER)).line_height(px(style::size::BODY_LINE)).child(crate::open_gate::markdown_view(("ask-text", index), paragraph, origin, &cwd)));
        if let Some(place) = ask.where_.as_deref().filter(|w| !w.is_empty()) {
            block = block.child(div().pl(indent).text_size(px(12.)).text_color(style::faint_text(cx)).child(place.to_owned()));
        }
        let this = cx.entity().downgrade();
        if ask.option_notes.is_empty() {
            // Buttons in a row that wraps, with Something else… after them.
            let mut buttons = h_flex().pl(indent).pt(px(6.)).gap(px(8.)).flex_wrap();
            for (n, choice) in choices.iter().enumerate() {
                let is_picked = picked == Some(n as i64);
                let look = if is_picked {
                    ButtonLook::Picked
                } else if ask.suggested == Some(n) {
                    ButtonLook::Suggested
                } else {
                    ButtonLook::Plain
                };
                let title = if is_picked { format!("✓ {choice}") } else { choice.clone() };
                let this = this.clone();
                buttons = buttons.child(card_button(("answer", index * 100 + n), &title, look, "Pick this answer", cx).on_click(move |_, window, cx| {
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| this.pick_answer(index, n as i64, window, cx));
                    }
                }));
            }
            let own = picked == Some(OWN_ANSWER);
            let this2 = this.clone();
            buttons = buttons.child(card_button(("answer-own", index), if own { "✓ Something else" } else { "Something else…" }, if own { ButtonLook::Picked } else { ButtonLook::Plain }, "Answer in your own words", cx).on_click(move |_, window, cx| {
                if let Some(this) = this2.upgrade() {
                    this.update(cx, |this, cx| this.pick_answer(index, OWN_ANSWER, window, cx));
                }
            }));
            block = block.child(buttons);
        } else {
            block = block.child(self.render_answer_rows(index, ask, picked, indent, cx));
        }
        if picked == Some(OWN_ANSWER) {
            let input = self.answer_box(index, window, cx);
            let this3 = this.clone();
            block = block.child(
                div()
                    .pl(indent)
                    .pt(px(8.))
                    .capture_action(move |action: &Enter, window, cx| {
                        if action.secondary {
                            if let Some(this) = this3.upgrade() {
                                this.update(cx, |this, cx| this.answer_box_return(index, true, window, cx));
                                cx.stop_propagation();
                            }
                        }
                    })
                    .child(
                        div()
                            .w_full()
                            .px(px(10.))
                            .py(px(6.))
                            .rounded(px(10.))
                            .border_1()
                            .border_color(style::hairline(cx))
                            .bg(style::answer_box_fill(cx))
                            .child(Textarea::new(&input).appearance(false).bordered(false).text_size(px(style::size::COMPOSER)).line_height(px(style::size::BODY_LINE))),
                    ),
            );
        }
        block.into_any_element()
    }

    /// An ask's answers as rows (RowRenderer.answerRows): a circle, the label,
    /// a star and "Recommended" on the agent's, what it does under it, then "Something else…".
    fn render_answer_rows(&mut self, index: usize, ask: &Ask, picked: Option<i64>, indent: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let choices = composer::choices(ask);
        let this = cx.entity().downgrade();
        let mut rows = v_flex().mt(px(10.)).ml(indent).rounded(px(10.)).overflow_hidden().bg(style::answer_rows_fill(cx));
        for n in 0..=choices.len() {
            let own = n == choices.len();
            let value = if own { OWN_ANSWER } else { n as i64 };
            let is_picked = picked == Some(value);
            let suggested = !own && ask.suggested == Some(n);
            let this = this.clone();
            let label = if own { "Something else…".to_owned() } else { choices[n].clone() };
            let note = (!own).then(|| ask.option_notes.get(n).cloned()).flatten().filter(|s| !s.is_empty());
            rows = rows.child(
                h_flex()
                    .id(("answer-row", index * 100 + n))
                    .w_full()
                    .items_start()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(7.))
                    .when(n > 0, |s| s.border_t_1().border_color(style::hairline(cx)))
                    .cursor_pointer()
                    .hover(|s| s.bg(style::hover_fill(cx)))
                    .tooltip(move |window, cx| Tooltip::new(if own { "Answer in your own words" } else { "Pick this answer" }).build(window, cx))
                    .on_click(move |_, window, cx| {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| this.pick_answer(index, value, window, cx));
                        }
                    })
                    .child(div().pt(px(3.)).child(tick_circle(is_picked, cx)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex().gap(px(8.)).child(div().text_size(px(style::size::COMPOSER)).text_color(if own { style::quiet_text(cx) } else { cx.theme().foreground }).child(label)).when(suggested, |s| {
                                    s.child(
                                        h_flex()
                                            .gap(px(3.))
                                            .child(div().text_size(px(10.)).text_color(style::recommended_star(cx)).child("★"))
                                            .child(div().text_size(px(12.)).text_color(style::quiet_text(cx)).child("Recommended")),
                                    )
                                }),
                            )
                            .when_some(note, |s, note| s.child(div().text_size(px(12.)).text_color(style::quiet_text(cx)).child(note))),
                    ),
            );
        }
        rows.into_any_element()
    }

    // ---- For the automation ----

    /// The picks as the Swift dump's `decisions` says them.
    #[cfg(feature = "automation")]
    pub(crate) fn decision_lines(&self, cx: &App) -> Option<Vec<String>> {
        let card = self.open_card(cx)?;
        if card.is_done() {
            return None;
        }
        let mut lines = vec![format!("{} picked", self.card.picks.len())];
        for (i, ask) in card.asks.iter().enumerate() {
            let options = composer::choices(ask);
            let choices: Vec<String> = options.iter().enumerate().map(|(n, o)| if ask.suggested == Some(n) { format!("*{o}") } else { o.clone() }).collect();
            let picked = self.card.picks.get(&(i as i64));
            let answer = match picked {
                Some(&OWN_ANSWER) => format!("something else: \"{}\"", self.card.notes.get(&(i as i64)).map(|n| n.text.clone()).unwrap_or_default()),
                Some(n) => options.get(*n as usize).cloned().unwrap_or_else(|| "no answer".to_owned()),
                None => "no answer".to_owned(),
            };
            lines.push(format!("  {}. {} [{}] -> {}", i + 1, ask.action, choices.join(" | "), answer));
        }
        Some(lines)
    }

    /// The Swift automation's `decide`: picks as its buttons pick, waiting in
    /// the composer. `pick` is "ask:choice", 1-based, choice 0 for Something else.
    #[cfg(feature = "automation")]
    pub(crate) fn decide(&mut self, cmd: &Value, window: &mut Window, cx: &mut Context<Self>) -> Result<String, String> {
        let Some(card) = self.open_card(cx).filter(|c| !c.is_done()) else { return Err("the open thread has no card of asks to answer".to_owned()) };
        self.sync_card(&card);
        if cmd["clear"].as_bool().unwrap_or(false) {
            self.card.picks.clear();
            self.push_card_state(cx);
        }
        if let Some(pick) = cmd["pick"].as_str() {
            let parts: Vec<usize> = pick.split(':').filter_map(|p| p.parse().ok()).collect();
            let [ask, choice] = parts.as_slice() else { return Err(format!("no choice {pick}")) };
            let Some(the_ask) = card.asks.get(ask.wrapping_sub(1)) else { return Err(format!("no choice {pick}")) };
            if *choice > composer::choices(the_ask).len() {
                return Err(format!("no choice {pick}"));
            }
            let option = if *choice == 0 { OWN_ANSWER } else { *choice as i64 - 1 };
            self.pick_answer(ask - 1, option, window, cx);
        }
        if let Some(value) = cmd["write"].as_str() {
            let Some((n, rest)) = value.split_once(':') else { return Err(format!("no ask in {value}")) };
            let Ok(n) = n.parse::<usize>() else { return Err(format!("no ask in {value}")) };
            if !(1..=card.asks.len()).contains(&n) {
                return Err(format!("no ask in {value}"));
            }
            let ask = n - 1;
            if self.card.picks.get(&(ask as i64)) != Some(&OWN_ANSWER) {
                self.pick_answer(ask, OWN_ANSWER, window, cx);
            }
            self.set_note(ask, rest.to_owned(), cx);
            let input = self.answer_box(ask, window, cx);
            input.update(cx, |s, cx| s.set_value(rest.to_owned(), window, cx));
        }
        if cmd["send"].as_bool().unwrap_or(false) {
            self.send(window, cx);
            return Ok("ok sent".to_owned());
        }
        Ok(format!("ok {}", self.decision_lines(cx).unwrap_or_default().join("\n")))
    }

    /// The Swift automation's `ask`: a card's button by its action
    /// ("pick:0:1", "next:0", "explain:0", "page:1", "collapse", "archive").
    #[cfg(feature = "automation")]
    pub(crate) fn card_action(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(card) = self.open_card(cx) else { return Err("that card is closed".to_owned()) };
        self.sync_card(&card);
        let parts: Vec<&str> = action.split(':').collect();
        match parts.as_slice() {
            ["archive"] => self.archive_thread(window, cx),
            ["collapse"] => self.toggle_collapsed(window, cx),
            ["page", n] => self.set_page(n.parse().map_err(|_| "bad page")?, cx),
            ["next", n] => self.toggle_follow_up(n.parse().map_err(|_| "bad index")?, window, cx),
            ["explain", n] => self.toggle_explain(n.parse().map_err(|_| "bad index")?, window, cx),
            ["pick", a, n] => {
                let (a, n): (usize, i64) = (a.parse().map_err(|_| "bad ask")?, n.parse().map_err(|_| "bad choice")?);
                self.pick_answer(a, n, window, cx);
            }
            _ => return Err(format!("unknown card action {action}")),
        }
        Ok(())
    }

    /// The card as text lines, for the dump (composer.card): its heading and
    /// each follow-up or ask with what's picked.
    #[cfg(feature = "automation")]
    pub(crate) fn card_lines(&self, cx: &App) -> Option<Vec<String>> {
        let card = self.open_card(cx)?;
        let mut lines = Vec::new();
        if card.is_done() {
            lines.push("Done".to_owned());
            for (i, next) in card.next.iter().enumerate() {
                let tick = if self.card.next_picks.contains(&(i as i64)) { "[x]" } else { "[ ]" };
                lines.push(format!("{tick} {next}{}", if self.card.explain.contains(&(i as i64)) { " (?)" } else { "" }));
            }
        } else {
            lines.push(if self.card.collapsed { "Needs you (collapsed)".to_owned() } else { "Needs you".to_owned() });
            if is_paged(&card) && !self.card.collapsed {
                lines.push(format!("page {} of {}", self.card.page + 1, card.asks.len()));
            }
            for (i, ask) in card.asks.iter().enumerate() {
                let picked = self.card.picks.get(&(i as i64));
                let mark = if picked.is_some() { "[x]" } else { "[ ]" };
                lines.push(format!("{mark} {}. {}", i + 1, ask.action));
            }
        }
        Some(lines)
    }
}

/// Over this, a card of several asks shows one at a time (ComposerCardHost.
/// pagedOver is 240 pt). The Swift app measures the laid-out text; this
/// estimates it, and the first frame can't be measured any other way.
pub(crate) fn is_paged(card: &AsksCard) -> bool {
    if !card.is_open || card.asks.len() < 2 {
        return false;
    }
    // Characters on a line of the reading column at the body size.
    let width_chars = 95_f32;
    let height: f32 = card
        .asks
        .iter()
        .map(|ask| {
            let lines: f32 = ask_paragraph(ask, 0, card.asks.len() > 1).split('\n').map(|line| (line.chars().count() as f32 / width_chars).ceil().max(1.)).sum();
            let place = if ask.where_.as_deref().is_some_and(|w| !w.is_empty()) { 21. } else { 0. };
            let answers = if ask.option_notes.is_empty() { 44. } else { 38. * (ask.options.len() as f32 + 1.) };
            12. + lines * style::size::BODY_LINE + place + answers
        })
        .sum();
    22. + height > 240.
}

/// An ask's paragraph, as the agent would have written it (Decisions.paragraph),
/// numbered when the card has several, under the same bold number the Swift
/// card puts by it.
fn ask_paragraph(ask: &Ask, index: usize, numbered: bool) -> String {
    let action: String = ask.action.split('\n').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    let why = ask.why.as_deref().unwrap_or("").trim();
    let body = if why.is_empty() {
        action
    } else {
        let ends = action.chars().rev().find(|c| !"\"'”’)]*_`".contains(*c)).is_some_and(|c| ".?!…:".contains(c));
        format!("{action}{} {why}", if ends { "" } else { "." })
    };
    if numbered {
        format!("{}. {}", index + 1, composer_continued(&body, 3))
    } else {
        body
    }
}

/// Decisions.continued: later lines indented past a list marker, so a list
/// in the why stays inside the item.
fn composer_continued(text: &str, width: usize) -> String {
    let pad = " ".repeat(width);
    text.split('\n')
        .enumerate()
        .map(|(i, line)| if i == 0 || line.is_empty() { line.to_owned() } else { format!("{pad}{line}") })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Copy, PartialEq)]
enum ButtonLook {
    Plain,
    Suggested,
    Picked,
}

/// The card's button (AskButton): a rounded pill, filled when picked, its
/// border in the accent on the agent's suggestion.
fn card_button(id: impl Into<ElementId>, title: &str, look: ButtonLook, tip: &'static str, cx: &App) -> Stateful<Div> {
    let (bg, fg, border) = match look {
        ButtonLook::Picked => (cx.theme().foreground, cx.theme().background, cx.theme().foreground),
        ButtonLook::Suggested => (Hsla::transparent_black(), cx.theme().foreground, style::accent(cx)),
        ButtonLook::Plain => (style::button_fill(cx), cx.theme().foreground, style::button_edge(cx)),
    };
    div()
        .id(id.into())
        .h(px(28.))
        .px(px(11.))
        .flex()
        .items_center()
        .rounded(px(7.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .text_size(px(style::size::LABEL))
        .cursor_pointer()
        .hover(|s| s.opacity(0.85))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
        .child(title.to_owned())
}

fn pager_arrow(glyph: &'static str, enabled: bool, tip: &'static str, id: impl Into<ElementId>, cx: &App, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    div()
        .id(id.into())
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .text_size(px(style::size::LABEL))
        .text_color(if enabled { cx.theme().foreground } else { style::faint_text(cx) })
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
        .when(enabled, |s| s.cursor_pointer().hover(|s| s.bg(style::hover_fill(cx))).on_click(on_click))
        .child(glyph)
}

fn tick_circle(ticked: bool, cx: &App) -> impl IntoElement {
    let side = px(16.);
    if ticked {
        Icon::from(IconName::CircleCheck).with_size(side).text_color(style::accent(cx)).into_any_element()
    } else {
        div().size(side).rounded_full().border_1().border_color(style::quiet_text(cx)).into_any_element()
    }
}

fn small_label(text: &'static str, cx: &App) -> impl IntoElement {
    div().pt(px(10.)).pl(px(8.)).text_size(px(style::size::DETAIL)).text_color(style::quiet_text(cx)).child(text)
}
