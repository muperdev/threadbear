// The message box under a thread: a port of the Swift app's ComposerView.swift
// and PictureChips.swift, and the send button from ComposerControls.swift. One
// Composer per thread lives on the app (Threadbear.composers), so what's typed,
// its undo and its pictures stay with the thread while others are open.
//
// Pictures are real inline tokens in gpui-kit's text input (InlineToken): one
// U+FFFC mark per chip, the chip drawn by the renderer below, and the input
// keeps them atomic through edits and undo. That mark is exactly what the
// Swift draft holds (InlinePictures), so the two apps' drafts read the same.
use crate::app::Threadbear;
use crate::composer_card::CardState;
use crate::composer_chips::ModelPickerView;
use crate::style;
use crate::thread::ThreadPane;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Enter, Escape, IndentInline, InlineToken, InlineTokenContext, InlineTokenHoverEvent, InputContent, InputEvent, MoveDown, MoveUp, Textarea, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use threadbear_client::runner::Client;
use threadbear_core::composer::{self, BOX_MAX_LINES, BOX_MIN_LINES, PICTURE_MARK, PICTURE_MARK_STR};
use threadbear_core::reducer::ImageRef;
use threadbear_core::slash::{PaletteKey, PaletteState};
use std::cell::Cell;
use std::rc::Rc;

/// A picture in the box: the token that draws its chip, what it came from,
/// and what the runner made of it (`media.import`).
struct PictureChip {
    /// The file it came from, for the thumbnail meanwhile.
    source: PathBuf,
    /// None while the runner is still copying it in.
    image: Option<ImageRef>,
}

pub struct Composer {
    /// None for the new-thread page's box.
    pub(crate) thread_id: Option<String>,
    pub(crate) client: Arc<Client>,
    pub(crate) app: WeakEntity<Threadbear>,
    /// The open thread's pane, once it has one; only ever read, or asked to
    /// send, from here (the pane never updates this composer back from those
    /// same calls, so no double lease).
    pub(crate) pane: WeakEntity<ThreadPane>,
    pub(crate) input: Entity<TextareaState>,
    /// The chips in the box, by token id, in no particular order: the box's
    /// tokens say the order (`chips`).
    pictures: HashMap<SharedString, PictureChip>,
    next_picture: u64,
    /// Each chip's place among the box's pictures, worked out each render so
    /// the renderer (which can't edit the input) can read it.
    chip_numbers: HashMap<SharedString, usize>,
    /// The chip the pointer is over, and the card showing it after a beat.
    hovered_chip: Option<(SharedString, Bounds<Pixels>)>,
    peek_shown: Option<(SharedString, Bounds<Pixels>)>,
    hover_serial: u64,
    /// Stop was pressed and the turn hasn't ended yet: a spinner in its place.
    stopping: bool,
    /// A refused send's draft, to put back at the next render.
    restore: Option<RestoreDraft>,
    /// The model picker's search and column, kept while it's open.
    pub(crate) model_picker: Option<Entity<ModelPickerView>>,
    /// What's picked on the open card, and how the card's showing.
    pub(crate) card: CardState,
    shown_placeholder: &'static str,
    /// The slash palette's own state (`PaletteState`), and whether this
    /// opening of it has asked for the thread's commands yet.
    pub(crate) palette: PaletteState,
    pub(crate) palette_asked: bool,
    /// The palette's list, kept so the arrows can scroll the pick into sight.
    pub(crate) palette_scroll: ScrollHandle,
    /// The box's width at the last paint, for the palette's columns.
    pub(crate) card_width: Rc<Cell<f32>>,
    _subs: Vec<Subscription>,
}

/// What a refused send gives back: the words and pictures, and the picks that
/// went with them (AppModel.restoreDraft and the `failed` closure in `send`).
pub struct RestoreDraft {
    pub(crate) text: String,
    pub(crate) images: Vec<ImageRef>,
    pub(crate) card: Option<SavedPicks>,
}

#[derive(Clone)]
pub struct SavedPicks {
    pub(crate) seq: i64,
    pub(crate) picks: std::collections::BTreeMap<i64, i64>,
    pub(crate) notes: std::collections::BTreeMap<i64, threadbear_core::reducer::AskNote>,
    pub(crate) next_picks: std::collections::BTreeSet<i64>,
    pub(crate) explain: std::collections::BTreeSet<i64>,
}

impl Composer {
    pub fn new(thread_id: Option<String>, client: Arc<Client>, app: WeakEntity<Threadbear>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Two lines at least, ten at most, then it scrolls (ComposerChips.boxHeight).
        let input = cx.new(|cx| TextareaState::new(window, cx).placeholder("Message").auto_grow(BOX_MIN_LINES, BOX_MAX_LINES).submit_on_enter(true));
        let sub = cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { shift: false, .. } => this.return_pressed(window, cx),
            // The send button, the picture strip and the slash palette follow
            // what's in the box.
            InputEvent::Change => {
                this.palette_text_changed(cx);
                cx.notify();
            }
            _ => {}
        });
        Composer {
            thread_id,
            client,
            app,
            pane: WeakEntity::new_invalid(),
            input,
            pictures: HashMap::new(),
            next_picture: 0,
            chip_numbers: HashMap::new(),
            hovered_chip: None,
            peek_shown: None,
            hover_serial: 0,
            stopping: false,
            restore: None,
            model_picker: None,
            card: CardState::default(),
            shown_placeholder: "Message",
            palette: PaletteState::default(),
            palette_asked: false,
            palette_scroll: ScrollHandle::new(),
            card_width: Rc::new(Cell::new(style::MAX_CONTENT_WIDTH)),
            _subs: vec![sub],
        }
    }

    pub fn attach(&mut self, pane: WeakEntity<ThreadPane>) {
        self.pane = pane;
    }

    /// A new thread is on its way from this box (`AppModel.creatingThread`).
    fn creating(&self, cx: &App) -> bool {
        self.thread_id.is_none() && self.app.upgrade().is_some_and(|a| a.read(cx).new_thread.creating)
    }

    /// The thread exists: what was sent leaves the box, if it's still what
    /// the box holds (it was locked while the runner made it).
    pub(crate) fn clear_sent_draft(&mut self, stored: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).value().as_ref() == stored {
            self.input.update(cx, |s, cx| s.set_value("", window, cx));
            self.pictures.clear();
        }
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |s, cx| s.focus(window, cx));
    }

    /// What the box holds as a draft (ComposerTextView.draftNow): the words,
    /// a mark for each chip whose picture is in, and those pictures in order.
    /// A chip still being copied in isn't part of it yet, nor is a stray
    /// U+FFFC pasted as text.
    fn draft(&self, cx: &App) -> (String, Vec<ImageRef>) {
        let content = self.input.read(cx).content();
        let text = content.text().to_string();
        let mut out = String::new();
        let mut images = Vec::new();
        let mut last = 0;
        for span in content.tokens() {
            let range = span.range();
            out.push_str(&composer::draft_words(&text[last..range.start]));
            if let Some(PictureChip { image: Some(image), .. }) = self.pictures.get(span.token().id()) {
                out.push(PICTURE_MARK);
                images.push(image.clone());
            }
            last = range.end;
        }
        out.push_str(&composer::draft_words(&text[last..]));
        (out, images)
    }

    /// The box's text as the Swift dump gives it: chips as the markers they'll
    /// go out as (`InlinePictures.messageText`).
    #[cfg(feature = "automation")]
    pub fn draft_text(&self, cx: &App) -> String {
        composer::message_text(&self.draft(cx).0, 1)
    }

    /// How many pictures are in the box (the dump's `composer.images`).
    #[cfg(feature = "automation")]
    pub fn draft_images(&self, cx: &App) -> usize {
        self.draft(cx).1.len()
    }

    fn pane(&self) -> Option<Entity<ThreadPane>> {
        self.pane.upgrade()
    }

    // ---- Keys ----

    /// Return in the box (ComposerTextView.keyDown): with nothing typed it
    /// goes to `return_when_empty`, otherwise it sends.
    fn return_pressed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).value().is_empty() && self.return_when_empty(window, cx) {
            return;
        }
        self.send(window, cx);
    }

    /// ComposerView.returnWhenEmpty: Return with nothing typed allows the
    /// sole waiting approval (the ⏎ its Allow button shows).
    fn return_when_empty(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.return_with_picks(window, cx) {
            return true;
        }
        let Some(pane) = self.pane() else { return false };
        pane.update(cx, |p, cx| p.allow_sole_approval(cx))
    }

    /// Shift-Return, before the input takes it: a markdown list carries on
    /// (ComposerTextView.insertLineContinuingList), through one edit so it's
    /// one undo.
    fn shift_return(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (text, selection) = {
            let state = self.input.read(cx);
            (state.value().to_string(), state.selected_range())
        };
        let Some(edit) = composer::list_newline(&text, selection.clone()) else { return false };
        self.input.update(cx, |s, cx| {
            if edit.range != selection {
                // An item with nothing in it: the marker comes off.
                s.set_selected_range(edit.range.clone(), cx);
            }
            s.replace(edit.text.clone(), window, cx);
        });
        true
    }

    // ---- Sending ----

    /// Whether there's anything to send (ComposerView.canSend).
    fn can_send(&self, cx: &App) -> bool {
        let (text, images) = self.draft(cx);
        composer::can_send(&text, images.len()) || !self.card.picks.is_empty() || !self.card.next_picks.is_empty() || !self.card.explain.is_empty()
    }

    /// AppModel.send: the box empties at once and the message shows dimmed
    /// at the end of the transcript until the runner has it; it comes back
    /// if the runner doesn't take it.
    pub(crate) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.thread_id.is_none() {
            return self.send_new_thread(window, cx);
        }
        let (draft_text, images) = self.draft(cx);
        // Pictures from the asks' boxes come first, numbered as the answers
        // say, then the box's own chips, whose markers go after them.
        let boxes = self.note_images_len(cx);
        let words = composer::sent_text(&draft_text, boxes + 1).trim().to_string();
        let (text, images_to_send, answers) = match self.card_message(&words, cx) {
            Some((message, mut note_images, answers)) => {
                note_images.extend(images.clone());
                (message, note_images, answers)
            }
            None => (words, images.clone(), Value::Null),
        };
        if text.trim().is_empty() && images_to_send.is_empty() {
            return;
        }
        let Some(pane) = self.pane() else { return };
        let picks = self.saved_picks();
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        self.pictures.clear();
        self.clear_picks(cx);
        let composer = cx.entity().downgrade();
        let restore = RestoreDraft { text: draft_text, images, card: picks };
        pane.update(cx, |p, cx| p.send_message(text, images_to_send, answers, Some(restore), composer, cx));
        cx.notify();
    }

    /// `AppModel.send(threadId: nil)`: the new-thread page's box. The words
    /// stay (greyed, locked) until the thread exists; a failure leaves them.
    fn send_new_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (draft_text, images) = self.draft(cx);
        let text = composer::sent_text(&draft_text, 1).trim().to_string();
        if text.is_empty() && images.is_empty() {
            return;
        }
        let stored = self.input.read(cx).value().to_string();
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| app.send_new_thread(text, images, stored, window, cx));
        }
    }

    fn saved_picks(&self) -> Option<SavedPicks> {
        self.card.seq.map(|seq| SavedPicks { seq, picks: self.card.picks.clone(), notes: self.card.notes.clone(), next_picks: self.card.next_picks.clone(), explain: self.card.explain.clone() })
    }

    fn note_images_len(&self, cx: &App) -> usize {
        self.open_card(cx).filter(|c| !c.is_done()).map_or(0, |c| composer::note_images(&c.asks, &self.card.picks, &self.card.notes).len())
    }

    /// The Stop button (⌘. goes through ThreadPane::stop too).
    fn stop(&mut self, cx: &mut Context<Self>) {
        self.stopping = true;
        if let Some(pane) = self.pane() {
            pane.update(cx, |p, cx| p.stop(cx));
        }
        cx.notify();
    }

    /// A send the runner refused: what was typed goes back in the box
    /// (AppModel.restoreDraft), unless something new has been typed since.
    /// At the next render, where there's a window to do it with.
    pub fn restore_draft(&mut self, draft: RestoreDraft, cx: &mut Context<Self>) {
        self.restore = Some(draft);
        cx.notify();
    }

    fn apply_restore(&mut self, restore: RestoreDraft, window: &mut Window, cx: &mut Context<Self>) {
        let RestoreDraft { text: draft, images, card } = restore;
        if let Some(saved) = card {
            if self.card.seq == Some(saved.seq) {
                self.card.picks = saved.picks;
                self.card.notes = saved.notes;
                self.card.next_picks = saved.next_picks;
                self.card.explain = saved.explain;
                self.push_card_state(cx);
            }
        }
        if !self.input.read(cx).value().is_empty() {
            return;
        }
        // Chips again where the marks are, each picture back in place.
        let mut content = InputContent::new(draft.clone());
        let mut images = images.into_iter();
        for (start, _) in draft.match_indices(PICTURE_MARK) {
            let Some(image) = images.next() else { break };
            let id = self.fresh_picture_id();
            let token = picture_token(&id);
            content = match content.clone().with_token(start..start + PICTURE_MARK.len_utf8(), token) {
                Ok(content) => content,
                Err(_) => content,
            };
            self.pictures.insert(id, PictureChip { source: PathBuf::from(&image.path), image: Some(image) });
        }
        self.input.update(cx, |s, cx| s.set_value(content, window, cx));
    }

    fn fresh_picture_id(&mut self) -> SharedString {
        self.next_picture += 1;
        format!("picture-{}", self.next_picture).into()
    }

    // ---- Pictures ----

    /// Paste: pictures and files first, as ComposerTextView.takeImages takes
    /// them. Returns whether it took the paste; words paste as words.
    pub(crate) fn take_clipboard(&mut self, item: &ClipboardItem, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut paths = Vec::new();
        let mut images = Vec::new();
        let mut has_text = false;
        for entry in item.entries() {
            match entry {
                ClipboardEntry::ExternalPaths(p) => paths.extend(p.paths().iter().cloned()),
                ClipboardEntry::Image(image) => images.push(image.clone()),
                ClipboardEntry::String(_) => has_text = true,
            }
        }
        if !paths.is_empty() {
            self.take_paths(paths, window, cx);
            return true;
        }
        // Text with an image flavour (rich text from a browser) stays text.
        if has_text || images.is_empty() {
            return false;
        }
        let saved: Vec<PathBuf> = images.iter().filter_map(save_pasted_image).collect();
        if saved.is_empty() {
            return false;
        }
        self.insert_pictures(saved, window, cx);
        true
    }

    /// Files pasted or dropped: pictures become chips where the cursor is, and
    /// anything else goes in as its full path in backticks, for the agent to
    /// open where it is (ComposerTextView.takeImages).
    pub(crate) fn take_paths(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let (images, others): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| is_picture(p));
        if !others.is_empty() {
            self.insert_paths(others, window, cx);
        }
        if !images.is_empty() {
            self.insert_pictures(images, window, cx);
        }
    }

    /// ComposerTextView.insertPaths: the paths with a space either side.
    fn insert_paths(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let (text, selection) = self.text_and_selection(cx);
        let mut piece = composer::path_text(&paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>());
        if text[..selection.start].chars().next_back().is_some_and(|c| !c.is_whitespace()) {
            piece.insert(0, ' ');
        }
        if !text[selection.end..].chars().next().is_some_and(char::is_whitespace) {
            piece.push(' ');
        }
        self.input.update(cx, |s, cx| {
            s.replace(piece, window, cx);
            s.focus(window, cx);
        });
    }

    fn text_and_selection(&self, cx: &App) -> (String, Range<usize>) {
        let state = self.input.read(cx);
        (state.value().to_string(), state.selected_range())
    }

    /// ComposerTextView.insertPictures: chips where the cursor is, with a
    /// space either side, each copied in through the runner as it shows,
    /// dimmed until it's in; one that can't be read goes again.
    pub(crate) fn insert_pictures(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<SharedString> = paths.iter().map(|_| self.fresh_picture_id()).collect();
        let tokens: Vec<InlineToken> = ids.iter().map(picture_token).collect();
        let (text, selection) = self.text_and_selection(cx);
        let prev_is_space = text[..selection.start].chars().next_back().map_or(true, char::is_whitespace);
        let next_is_space = text[selection.end..].chars().next().is_some_and(char::is_whitespace);
        self.input.update(cx, |s, cx| {
            if !selection.is_empty() {
                s.replace("", window, cx);
            }
            if !prev_is_space {
                s.insert(" ", window, cx);
            }
            for (i, token) in tokens.iter().enumerate() {
                if i > 0 {
                    s.insert(" ", window, cx);
                }
                let _ = s.replace_with_token(token.clone(), window, cx);
            }
            if !next_is_space {
                s.insert(" ", window, cx);
            }
            s.focus(window, cx);
        });
        for (id, path) in ids.into_iter().zip(paths) {
            self.pictures.insert(id.clone(), PictureChip { source: path.clone(), image: None });
            self.import_picture(id, path, window, cx);
        }
        cx.notify();
    }

    fn import_picture(&mut self, id: SharedString, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let params = json!({ "path": path.to_string_lossy() });
        cx.spawn_in(window, async move |this, cx| {
            let result = crate::call(client, "media.import", params).await;
            // The runner keeps its own copy (or said no): a pasted picture's
            // staged file goes now, whatever happens to the chip.
            unstage(&path);
            this.update_in(cx, |this, window, cx| this.settle_picture(id, result, window, cx)).ok();
        })
        .detach();
    }

    /// ComposerTextView.settle: a chip's picture is in, or couldn't be read.
    /// Gone from the words by then (undone, deleted), it's dropped.
    fn settle_picture(&mut self, id: SharedString, result: Result<Value, threadbear_client::runner::RpcError>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(range) = self.chip_range(&id, cx) else {
            self.pictures.remove(&id);
            return;
        };
        match result {
            Ok(v) => {
                let image = ImageRef::decode(&v["image"]);
                if let (Some(chip), Some(image)) = (self.pictures.get_mut(&id), image) {
                    chip.image = Some(image);
                }
            }
            Err(e) => {
                // Taken out, with the space that came with it.
                self.remove_chip_range(range, window, cx);
                self.pictures.remove(&id);
                if let Some(pane) = self.pane() {
                    pane.update(cx, |p, cx| p.show_error(format!("Couldn't add that picture: {}", e.message), cx));
                }
            }
        }
        cx.notify();
    }

    /// The token ranges of the chips in the box, in order.
    fn chips(&self, cx: &App) -> Vec<(Range<usize>, SharedString)> {
        self.input.read(cx).tokens().iter().map(|span| (span.range(), span.token().id().clone())).collect()
    }

    fn chip_range(&self, id: &SharedString, cx: &App) -> Option<Range<usize>> {
        self.chips(cx).into_iter().find(|(_, chip)| chip == id).map(|(r, _)| r)
    }

    /// Takes a chip out with one space beside it (ComposerTextView.withSpace),
    /// keeping the cursor where it was as far as it can.
    fn remove_chip_range(&mut self, range: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let (text, selection) = self.text_and_selection(cx);
        let bytes = text.as_bytes();
        let gone = if bytes.get(range.end) == Some(&b' ') {
            range.start..range.end + 1
        } else if range.start > 0 && bytes[range.start - 1] == b' ' {
            range.start - 1..range.end
        } else {
            range.clone()
        };
        let shift = |at: usize| if at <= gone.start { at } else { at.saturating_sub(gone.len()).max(gone.start) };
        let caret = shift(selection.start)..shift(selection.end);
        self.input.update(cx, |s, cx| {
            s.set_selected_range(gone.clone(), cx);
            s.replace("", window, cx);
            s.set_selected_range(caret.clone(), cx);
        });
    }

    /// The strip's × (ComposerView.removePicture): the chip goes too, and the
    /// cursor stays where it was.
    fn remove_picture(&mut self, id: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(range) = self.chip_range(&id, cx) {
            self.remove_chip_range(range, window, cx);
        }
        self.pictures.remove(&id);
        cx.notify();
    }

    fn renumber_chips(&mut self, cx: &App) {
        self.chip_numbers = self.chips(cx).into_iter().enumerate().map(|(i, (_, id))| (id, i + 1)).collect();
    }

    /// The hover card over a chip, after a beat, as ComposerTextView.hoverChip
    /// does (0.35 s).
    fn hover_chip(&mut self, event: &InlineTokenHoverEvent, cx: &mut Context<Self>) {
        self.hover_serial += 1;
        let serial = self.hover_serial;
        self.peek_shown = None;
        self.hovered_chip = event.is_hovered().then(|| (event.token().id().clone(), event.bounds()));
        cx.notify();
        if self.hovered_chip.is_some() {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(350)).await;
                this.update(cx, |this, cx| {
                    if this.hover_serial == serial {
                        this.peek_shown = this.hovered_chip.clone();
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    /// Shows the chip numbered `number`'s picture, as the automation's
    /// `composer-peek` does (ComposerTextView.debugHoverChip).
    #[cfg(feature = "automation")]
    pub fn debug_peek(&mut self, number: Option<usize>, cx: &mut Context<Self>) -> Result<(), String> {
        self.hovered_chip = None;
        match number {
            None => self.peek_shown = None,
            Some(number) => {
                let chips = self.chips(cx);
                let Some((_, id)) = chips.get(number.saturating_sub(1)) else { return Err(format!("no chip {number}")) };
                self.peek_shown = Some((id.clone(), Bounds::default()));
            }
        }
        cx.notify();
        Ok(())
    }

    // ---- Drawing ----

    fn picture_path(&self, id: &SharedString) -> Option<PathBuf> {
        self.pictures.get(id).map(|chip| chip.image.as_ref().map_or_else(|| chip.source.clone(), |i| PathBuf::from(&i.path)))
    }

    /// A picture's chip among the words: a small rounded tag with the
    /// thumbnail and "Picture N" (PictureChip.draw), its sizes from the font
    /// of the words around it. Dimmed until the picture's in.
    fn render_chip(&self, ctx: &InlineTokenContext, cx: &App) -> AnyElement {
        let id = ctx.token().id();
        let chip = self.pictures.get(id);
        let number = self.chip_numbers.get(id).copied().unwrap_or(1);
        let dimmed = chip.is_none_or(|c| c.image.is_none());
        let path = self.picture_path(id);
        let selected = ctx.is_selected();
        let fill = style::chip_fill(cx);
        let edge = if selected { style::accent(cx) } else { style::hairline(cx) };
        let label_color = if dimmed { style::faint_text(cx) } else { style::quiet_text(cx) };
        h_flex()
            .h_full()
            .items_center()
            .child(
                h_flex()
                    .h(px(style::size::CHIP_HEIGHT))
                    .pl(px(3.))
                    .pr(px(7.))
                    .gap(px(5.))
                    .rounded(px(6.))
                    .bg(fill)
                    .border_1()
                    .border_color(edge)
                    .when_some(path, |s, path| s.child(img(path).size(px(14.)).rounded(px(3.)).object_fit(ObjectFit::Cover).when(dimmed, |s| s.opacity(0.5))))
                    .child(div().text_size(px(13.)).text_color(label_color).child(format!("Picture {number}"))),
            )
            .into_any_element()
    }

    /// The strip over the words (AttachmentStrip): a thumbnail for each
    /// picture that's in, with × on hover, and a placeholder for each still
    /// being copied in, so the strip doesn't jump when it arrives.
    fn render_strip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let chips = self.chips(cx);
        if chips.is_empty() {
            return None;
        }
        let side = px(style::size::THUMBNAIL);
        let radius = px(8.);
        let edge = style::hairline(cx);
        let mut row = h_flex().gap(px(8.));
        for (index, (_, id)) in chips.iter().enumerate() {
            let chip = self.pictures.get(id);
            let image = chip.and_then(|c| c.image.as_ref());
            let tile = div().relative().size(side).rounded(radius).border_1().border_color(edge).overflow_hidden().bg(style::chip_fill(cx));
            let tile = match image {
                Some(image) => tile.child(img(PathBuf::from(&image.path)).size(side).rounded(radius).object_fit(ObjectFit::Cover)),
                None => tile.flex().items_center().justify_center().child(Spinner::new().color(style::quiet_text(cx)).with_size(px(14.))),
            };
            let remove_id = id.clone();
            let remove = div()
                .id(("remove-picture", index))
                .absolute()
                .top(px(4.))
                .right(px(4.))
                .size(px(18.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(hsla(0., 0., 0., 0.72))
                .cursor_pointer()
                .child(Icon::from(IconName::X).with_size(px(9.)).text_color(gpui_kit::white()))
                .tooltip(|window, cx| Tooltip::new("Remove").build(window, cx))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.remove_picture(remove_id.clone(), window, cx)));
            row = row.child(div().id(("strip-tile", index)).group("tile").relative().child(tile).child(div().invisible().group_hover("tile", |s| s.visible()).child(remove)));
        }
        Some(row.into_any_element())
    }

    fn render_send_button(&self, running: bool, creating: bool, cx: &mut Context<Self>) -> AnyElement {
        let can_send = self.can_send(cx);
        let side = px(style::size::SEND_BUTTON);
        let circle = || div().flex().items_center().justify_center().size(side).rounded_full();
        // The runner is making the thread: a spinner in the button's place
        // until it has, however long a busy Mac makes that (SendButton).
        if creating {
            return circle()
                .id("composer-starting")
                .bg(style::send_faint_fill(cx))
                .child(Spinner::new().color(style::quiet_text(cx)).with_size(px(14.)))
                .tooltip(|window, cx| Tooltip::new("Starting the thread").build(window, cx))
                .into_any_element();
        }
        if running && !can_send && self.stopping {
            return circle()
                .id("composer-stopping")
                .bg(style::send_faint_fill(cx))
                .child(Spinner::new().color(style::quiet_text(cx)).with_size(px(14.)))
                .tooltip(|window, cx| Tooltip::new("Stopping").build(window, cx))
                .into_any_element();
        }
        if running && !can_send {
            let (fg, bg) = (cx.theme().background, cx.theme().foreground);
            return circle()
                .id("composer-stop")
                .cursor_pointer()
                .bg(bg)
                .child(Icon::from(IconName::Square).with_size(px(10.)).text_color(fg))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.stop(cx)))
                .tooltip(|window, cx| Tooltip::new(if cfg!(target_os = "macos") { "Stop (⌘.)" } else { "Stop (Ctrl+.)" }).build(window, cx))
                .into_any_element();
        }
        let tip = if running { "Send (into this turn)" } else { "Send (Return)" };
        let (fg, bg) = if can_send { (cx.theme().background, cx.theme().foreground) } else { (style::faint_text(cx), style::send_faint_fill(cx)) };
        circle()
            .id("composer-send")
            .when(can_send, |s| s.cursor_pointer())
            .bg(bg)
            .child(Icon::from(IconName::ArrowUp).with_size(px(14.)).text_color(fg))
            .when(can_send, |s| s.on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.send(window, cx))))
            .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
            .into_any_element()
    }

    /// The text field, at the transcript's body size and line height so a
    /// message keeps its rhythm when it's sent (ComposerTextField).
    fn render_text_field(&self, creating: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let paste_target = this.clone();
        let hover_target = this.clone();
        let token_owner = this.clone();
        let drop_target = this;
        div()
            .w_full()
            // Greyed and locked while a new thread is being made from it.
            .when(creating, |s| s.opacity(0.5))
            .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                if action.shift && this.shift_return(window, cx) {
                    cx.stop_propagation();
                } else if !action.shift && this.palette_key(PaletteKey::Enter, window, cx) {
                    cx.stop_propagation();
                }
            }))
            // The palette's keys, while it's open (ComposerView.paletteKey);
            // otherwise the box has them as usual.
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                if this.palette_key(PaletteKey::Up, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                if this.palette_key(PaletteKey::Down, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                if this.palette_key(PaletteKey::Tab, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.palette_key(PaletteKey::Escape, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_drop(move |paths: &ExternalPaths, window, cx| {
                if let Some(this) = drop_target.upgrade() {
                    this.update(cx, |this, cx| this.take_paths(paths.paths().to_vec(), window, cx));
                }
            })
            .child(
                Textarea::new(&self.input)
                    .appearance(false)
                    .bordered(false)
                    .disabled(creating)
                    .text_size(px(style::size::COMPOSER))
                    .line_height(px(style::size::BODY_LINE))
                    .token(move |ctx, _window, cx| match token_owner.upgrade() {
                        Some(this) => this.read(cx).render_chip(ctx, cx),
                        None => div().into_any_element(),
                    })
                    .on_paste(move |item, window, cx| paste_target.upgrade().is_some_and(|this| this.update(cx, |this, cx| this.take_clipboard(item, window, cx))))
                    .on_token_hover(move |event, _window, cx| {
                        if let Some(this) = hover_target.upgrade() {
                            this.update(cx, |this, cx| this.hover_chip(event, cx));
                        }
                    }),
            )
    }

    /// The card over a chip while the pointer rests on it: the picture, to
    /// tell which is which (LinkPeekCard with pictureOnly).
    fn render_peek(&self, cx: &App) -> Option<AnyElement> {
        let (id, bounds) = self.peek_shown.as_ref()?;
        let path = self.picture_path(id)?;
        Some(
            deferred(
                anchored().position(bounds.origin).anchor(Anchor::BottomLeft).snap_to_window().offset(point(px(0.), px(-8.))).child(
                    div().p(px(4.)).rounded(px(10.)).bg(cx.theme().popover).border_1().border_color(style::hairline(cx)).shadow_lg().child(img(path).max_w(px(320.)).max_h(px(240.)).rounded(px(6.))),
                ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(restore) = self.restore.take() {
            self.apply_restore(restore, window, cx);
        }
        self.renumber_chips(cx);
        // With answers picked the box is for anything else; under asks with
        // none picked it's the way to answer in other words.
        let placeholder = self.card_placeholder(cx);
        if self.shown_placeholder != placeholder {
            self.shown_placeholder = placeholder;
            self.input.update(cx, |s, cx| s.set_placeholder(placeholder, window, cx));
        }
        let running = self.pane().is_some_and(|p| p.read(cx).summary.busy());
        if !running {
            self.stopping = false;
        }
        let strip = self.render_strip(cx);
        let creating = self.creating(cx);
        let send_button = self.render_send_button(running, creating, cx);
        let controls = self.render_controls(send_button, window, cx);
        // The box: the open card over a line, then the message.
        let card = self.render_card(window, cx);
        let needs_you = self.open_card(cx).is_some_and(|c| !c.is_done());
        let palette = self.render_palette(window, cx);
        let width = self.card_width.clone();
        let body = v_flex().w_full().gap(px(style::COMPOSER_GAP)).p(px(style::COMPOSER_PADDING)).when_some(strip, |s, strip| s.child(strip)).child(self.render_text_field(creating, cx)).child(controls);
        // The transcript's reading column: 780 wide at most, centred, with
        // the same side insets once the window is narrower.
        h_flex()
            .w_full()
            .justify_center()
            .px(px(style::SIDE_INSET))
            .pt(px(4.))
            .pb(px(20.))
            .child(
                div()
                    .relative()
                    .w_full()
                    .max_w(px(style::MAX_CONTENT_WIDTH))
                    .rounded(px(style::COMPOSER_RADIUS))
                    .bg(style::composer_fill(cx))
                    .border_1()
                    .border_color(if needs_you { style::accent(cx) } else { style::hairline(cx) })
                    .when(!cx.theme().is_dark(), |s| s.shadow_md())
                    // Clicking the card's padding puts the cursor in the text.
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _: &MouseDownEvent, window, cx| this.focus(window, cx)))
                    .children(card)
                    .child(body)
                    // Measures the box for the palette, which sits on its top edge.
                    .child(canvas(move |bounds, _, _| width.set(f32::from(bounds.size.width)), |_, _, _, _| {}).absolute().size_full())
                    .children(palette),
            )
            .children(self.render_peek(cx))
    }
}

/// The token a chip draws from. The token's text is the mark the draft holds
/// (InlinePictures.mark); its label is only for accessibility and measuring.
fn picture_token(id: &SharedString) -> InlineToken {
    InlineToken::new(id.clone(), PICTURE_MARK_STR).with_label("Picture")
}

/// Whether a dropped or pasted file is a picture, by its name
/// (ComposerTextView.isImage asks the file's type; a name is what every
/// system can tell).
pub(crate) fn is_picture(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "heif" | "tif" | "tiff" | "bmp")
    )
}

/// A picture pasted as raw image data (a screenshot on the clipboard, say),
/// saved to a file the runner can import, as ComposerTextView.takeImages does.
/// PNG and JPEG go as they are; the runner turns TIFF and BMP into PNG
/// (`media.import`). An SVG stays out: the runner doesn't take them.
fn save_pasted_image(image: &gpui_kit::Image) -> Option<PathBuf> {
    let ext = match image.format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Webp => "webp",
        ImageFormat::Gif => "gif",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Tiff => "tiff",
        _ => return None,
    };
    stage_private(&paste_base(), &format!("pasted-{:x}.{ext}", image.id), &image.bytes)
}

/// Where pasted pictures wait for the runner: the session's own runtime
/// folder on Linux (`XDG_RUNTIME_DIR`, which only this user can open), else
/// the temp folder (already this user's own on a Mac and Windows; `/tmp` on
/// a Linux without one, which everyone can list).
fn paste_base() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_absolute() && d.is_dir()).unwrap_or_else(std::env::temp_dir)
}

/// Writes `bytes` as `name` in a folder of its own under `base`: made fresh
/// with a random name and 0700, the file new with 0600, so on a shared
/// `/tmp` nobody else can read it, list it, or make the folder first and
/// have it written into theirs (Daybreak, 10 Nov 2026, finding 63). The
/// folder goes once the runner has its own copy (`import_picture`).
fn stage_private(base: &std::path::Path, name: &str, bytes: &[u8]) -> Option<PathBuf> {
    use std::io::Write as _;
    let mut builder = tempfile::Builder::new();
    builder.prefix("threadbear-paste-");
    // Without a mode the folder would take the umask's (0755 under 022).
    // Windows has none: a folder there takes the user's temp folder's access.
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700));
    let folder = builder.tempdir_in(base).ok()?.keep();
    let path = folder.join(name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options.open(&path).and_then(|mut f| f.write_all(bytes));
    if written.is_err() {
        let _ = std::fs::remove_dir_all(&folder);
        return None;
    }
    STAGED.lock().unwrap_or_else(|e| e.into_inner()).push(folder);
    Some(path)
}

/// The folders `stage_private` made and hasn't removed: only these are
/// ever deleted, never a folder that just has the same kind of name (Codex
/// review, 10 Nov 2026: a picture dropped from `/tmp/threadbear-paste-x/`
/// would have taken that whole folder with it).
static STAGED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// A staged picture's folder, once the runner has copied it (or couldn't).
/// A dropped or pasted file's own folder is never touched.
fn unstage(path: &std::path::Path) {
    let Some(folder) = path.parent() else { return };
    let mut staged = STAGED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = staged.iter().position(|f| f == folder) {
        staged.remove(at);
        drop(staged);
        let _ = std::fs::remove_dir_all(folder);
    }
}

#[cfg(test)]
mod paste_tests {
    use super::{stage_private, unstage};
    use std::fs;

    /// Finding 63: under umask 022 the old folder was 0755 and the file
    /// 0644, in a fixed `/tmp/threadbear-paste` anyone could list or make
    /// first. Each paste now gets a fresh folder only its owner can open.
    #[cfg(unix)]
    #[test]
    fn a_pasted_picture_is_kept_private() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let path = stage_private(base.path(), "pasted-1.png", b"png bytes").unwrap();
        let folder = path.parent().unwrap();
        assert_eq!(fs::metadata(folder).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read(&path).unwrap(), b"png bytes");
        // Not a name anyone can guess: the same picture twice goes to two folders.
        let again = stage_private(base.path(), "pasted-1.png", b"png bytes").unwrap();
        assert_ne!(again.parent(), path.parent());
        assert!(folder.file_name().unwrap().to_string_lossy().len() > "threadbear-paste-".len() + 4);
    }

    /// A folder someone else made first, under the old fixed name, is never
    /// written into.
    #[test]
    fn a_folder_made_first_isnt_used() {
        let base = tempfile::tempdir().unwrap();
        let planted = base.path().join("threadbear-paste");
        fs::create_dir(&planted).unwrap();
        let path = stage_private(base.path(), "pasted-2.png", b"x").unwrap();
        assert!(!path.starts_with(&planted));
        assert_eq!(fs::read_dir(&planted).unwrap().count(), 0);
    }

    /// Once the runner has its copy, the staged folder goes, and only a
    /// folder this made: a dropped file's own folder stays, even one with
    /// the same kind of name.
    #[test]
    fn the_staged_copy_goes_after_the_import() {
        let base = tempfile::tempdir().unwrap();
        let path = stage_private(base.path(), "pasted-3.png", b"x").unwrap();
        let folder = path.parent().unwrap().to_owned();
        unstage(&path);
        assert!(!folder.exists());
        for name in ["Desktop", "threadbear-paste-project"] {
            let dropped_dir = base.path().join(name);
            fs::create_dir(&dropped_dir).unwrap();
            fs::write(dropped_dir.join("other.txt"), b"keep").unwrap();
            let dropped = dropped_dir.join("shot.png");
            fs::write(&dropped, b"x").unwrap();
            unstage(&dropped);
            assert!(dropped.exists() && dropped_dir.join("other.txt").exists(), "{name}");
        }
    }
}

/// The JSON a picture goes as (`ImageRef` in docs/protocol.md).
pub fn image_ref_json(image: &ImageRef) -> Value {
    let mut out = json!({ "path": image.path, "mediaType": image.media_type });
    if let Some(w) = image.width {
        out["width"] = json!(w);
    }
    if let Some(h) = image.height {
        out["height"] = json!(h);
    }
    out
}
