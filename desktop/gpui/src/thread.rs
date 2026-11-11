// One thread: its transcript (rows come from the reducer in threadbear-core),
// its cards and the composer. Reads the newest page of events, then subscribes
// from the last of them, as the Mac app's ThreadSession does.
use crate::composer::Composer;
use crate::rows::list::{transcript_list, TranscriptList};
use crate::{call, RT};
use gpui_kit::component::{v_flex, ActiveTheme as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use threadbear_client::runner::Client;
use threadbear_core::inbox::ThreadSummary;
use threadbear_core::reducer::{BackgroundTask, Event, EventsPage, FeedAction, ImageRef, PendingMessage, Row, RowContent, ThreadFeed, TranscriptChange, TranscriptModel};

/// Events asked for when a thread opens, as ThreadSession.eventsPage.
const EVENTS_PAGE: u64 = 500;
/// Streamed text arrives faster than it's worth drawing: flushed at most this
/// often, as ThreadSession does (30 Hz).
const FLUSH_EVERY: Duration = Duration::from_millis(33);

pub struct ThreadPane {
    client: Arc<Client>,
    pub(crate) summary: ThreadSummary,
    pub(crate) feed: ThreadFeed,
    loaded: bool,
    error: Option<String>,
    pub(crate) scroller: Entity<TranscriptList>,
    /// Everything the rows themselves hold between frames (rows.rs).
    pub(crate) rows: crate::rows::RowsState,
    pub(crate) composer: Entity<Composer>,
    /// Tool and thinking rows opened, by row id (and "<row>|all" for a diff
    /// showing every line). Groups keep theirs in the model.
    pub(crate) open_rows: HashSet<String>,
    pub(crate) chosen: HashMap<String, HashMap<usize, Vec<String>>>,
    pending: Vec<PendingMessage>,
    next_pending: u64,
    flush_queued: bool,
    /// An older page is on its way.
    pub(crate) loading_older: bool,
    /// A page that came back with nothing before the seq asked for: the next
    /// page ends before this seq instead (`olderSkip`).
    older_skip: Option<(i64, i64)>,
    older_failures: u32,
    _subs: Vec<Subscription>,
}

impl ThreadPane {
    pub fn new(client: Arc<Client>, summary: ThreadSummary, composer: Entity<Composer>, cx: &mut Context<Self>) -> Self {
        let scroller = cx.new(|cx| TranscriptList::new(0, cx));
        // Any scroll or layout change: older rows may be due and where the
        // view is reading is remembered for when the thread is left.
        let scrolled = cx.observe(&scroller, |this: &mut ThreadPane, _, cx| this.after_list_change(cx));
        let mut pane = ThreadPane {
            client,
            summary,
            feed: ThreadFeed::new(TranscriptModel::new(None)),
            loaded: false,
            error: None,
            scroller,
            rows: Default::default(),
            composer,
            open_rows: HashSet::new(),
            chosen: HashMap::new(),
            pending: vec![],
            next_pending: 0,
            flush_queued: false,
            loading_older: false,
            older_skip: None,
            older_failures: 0,
            _subs: vec![scrolled],
        };
        pane.load(cx);
        pane
    }

    pub(crate) fn client_handle(&self) -> Arc<Client> {
        self.client.clone()
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if self.summary.busy() {
            self.request("threads.interrupt", json!({ "threadId": self.summary.id }), cx);
        }
    }

    /// Every row measured again, for a new zoom (their heights are in points).
    pub(crate) fn remeasure_rows(&mut self, cx: &mut Context<Self>) {
        let count = self.feed.model().rows().len();
        self.scroller.update(cx, |s, cx| {
            s.remeasure_items(0..count, cx);
        });
        cx.notify();
    }

    /// The thread's last message of his, for Send again (`sendAgain`).
    pub(crate) fn last_user_text(&self) -> Option<String> {
        self.feed.model().rows().iter().rev().take(80).find_map(|row| match &row.content {
            RowContent::User(message) if !message.is_from_runner() => Some(message.text.clone()),
            _ => None,
        })
    }

    fn background_tasks(&self) -> Vec<BackgroundTask> {
        self.summary.background.iter().flatten().filter_map(BackgroundTask::decode).collect()
    }

    pub fn load(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let id = self.summary.id.clone();
        let root = self.summary.cwd.clone();
        let background = self.background_tasks();
        cx.spawn(async move |this, cx| {
            let page = match call(client.clone(), "threads.events", json!({ "threadId": id, "limit": EVENTS_PAGE })).await {
                Ok(v) => EventsPage::decode(&v).unwrap_or_default(),
                Err(e) => {
                    this.update(cx, |this, cx| {
                        this.error = Some(e.message);
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            let after_seq = this
                .update(cx, |this, cx| {
                    // Where the view was reading, if this is a reload.
                    let memory = this.loaded.then(|| this.capture_memory(cx));
                    let mut model = TranscriptModel::new(Some(root));
                    model.load(page.events, &page.in_progress);
                    model.known_start = page.at_start;
                    let _ = model.set_background(background);
                    this.feed = ThreadFeed::new(model);
                    // Another runner's thread through this Mac's runner has
                    // gaps in its seqs (`feed.sparse = viaRelay`).
                    this.feed.set_sparse(this.summary.runner_id.is_some());
                    this.feed.begin_subscribe();
                    this.loaded = true;
                    this.loading_older = false;
                    this.older_skip = None;
                    this.scroller.update(cx, |s, cx| s.reset(this.feed.model().rows().len(), cx));
                    this.restore_memory(memory, cx);
                    this.mark_seen();
                    // Paging older rows in is driven by the view reaching the
                    // top, and a first page shorter than the view can't.
                    this.load_older_if_near_top(cx);
                    cx.notify();
                    this.feed.accepted_seq()
                })
                .unwrap_or(0);
            let sub = call(client, "threads.subscribe", json!({ "threadId": id, "afterSeq": after_seq })).await;
            this.update(cx, |this, cx| match sub.ok().and_then(|v| EventsPage::decode(&v)) {
                Some(page) => {
                    let action = this.feed.finish_subscribe(page.events, &page.in_progress);
                    this.handle(action, cx);
                }
                None => this.feed.catch_up_failed(),
            })
            .ok();
        })
        .detach();
    }

    // Opening it reads it, as on the Mac.
    fn mark_seen(&self) {
        if self.summary.unread() {
            let client = self.client.clone();
            let params = json!({ "threadId": self.summary.id, "seq": self.summary.last_seq });
            RT.spawn(async move { client.call("threads.markSeen", params).await });
        }
    }

    pub fn unsubscribe(&self) {
        let client = self.client.clone();
        let params = json!({ "threadId": self.summary.id });
        RT.spawn(async move { client.call("threads.unsubscribe", params).await });
    }

    pub fn summary_changed(&mut self, t: &ThreadSummary, cx: &mut Context<Self>) {
        if t.id == self.summary.id {
            self.summary = t.clone();
            if self.loaded {
                self.mark_seen();
                let change = self.feed.set_background(self.background_tasks());
                self.apply(change, cx);
            }
            cx.notify();
        }
    }

    pub fn event(&mut self, params: &Value, cx: &mut Context<Self>) {
        if params["threadId"].as_str() != Some(self.summary.id.as_str()) {
            return;
        }
        if let Some(event) = Event::decode(&params["event"]) {
            let action = self.feed.receive(event);
            self.handle(action, cx);
        }
    }

    fn handle(&mut self, action: FeedAction, cx: &mut Context<Self>) {
        match action {
            FeedAction::None => {}
            FeedAction::ScheduleFlush => self.schedule_flush(cx),
            FeedAction::NeedsCatchUp { after_seq } => self.catch_up(after_seq, cx),
        }
    }

    fn schedule_flush(&mut self, cx: &mut Context<Self>) {
        if self.flush_queued {
            return;
        }
        self.flush_queued = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FLUSH_EVERY).await;
            this.update(cx, |this, cx| {
                this.flush_queued = false;
                let change = this.feed.flush();
                this.apply(change, cx);
            })
            .ok();
        })
        .detach();
    }

    fn catch_up(&mut self, after_seq: i64, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let id = self.summary.id.clone();
        cx.spawn(async move |this, cx| {
            let page = call(client, "threads.events", json!({ "threadId": id, "afterSeq": after_seq })).await;
            this.update(cx, |this, cx| match page.ok().and_then(|v| EventsPage::decode(&v)) {
                Some(page) => {
                    let action = this.feed.catch_up(page.events);
                    this.handle(action, cx);
                }
                None => this.feed.catch_up_failed(),
            })
            .ok();
        })
        .detach();
    }

    /// Tells the list what the reducer changed, so it keeps its place: rows
    /// gone first, from the bottom up, then new ones from the top down. GPUI's
    /// list holds the view on the row at its top (or follows the tail) across
    /// all of it, which is what `apply` in TranscriptController did by hand.
    pub(crate) fn apply(&mut self, change: TranscriptChange, cx: &mut Context<Self>) {
        self.pending = self.feed.model().pending().to_vec();
        if change.is_empty() {
            return;
        }
        self.scroller.update(cx, |s, cx| {
            for &i in change.removed.iter().rev() {
                s.splice(i..i + 1, 0, cx);
            }
            for &i in change.inserted.iter() {
                s.splice(i..i, 1, cx);
            }
            for &i in change.updated.iter() {
                s.remeasure_items(i..i + 1, cx);
            }
        });
        cx.notify();
    }

    pub(crate) fn send(&mut self, text: String, answers: Option<Value>, cx: &mut Context<Self>) {
        self.send_message(text, Vec::new(), answers.unwrap_or(Value::Null), None, WeakEntity::new_invalid(), cx);
    }

    /// Sends a message (AppModel.post): it shows dimmed at the end of the
    /// transcript straight away, and goes back into the box if the runner
    /// doesn't take it. `draft` is what to give back then.
    pub(crate) fn send_message(&mut self, text: String, images: Vec<ImageRef>, answers: Value, draft: Option<crate::composer::RestoreDraft>, composer: WeakEntity<Composer>, cx: &mut Context<Self>) {
        // Shown straight away and put right when the runner echoes it back.
        self.next_pending += 1;
        let pending_id = format!("local-{}", self.next_pending);
        let mut message = PendingMessage::new(&pending_id, &text, self.feed.accepted_seq());
        message.images = images.clone();
        self.pending.push(message);
        let change = self.feed.set_pending(self.pending.clone());
        self.apply(change, cx);
        self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));

        let mut params = json!({ "threadId": self.summary.id, "text": text });
        if !images.is_empty() {
            params["images"] = json!(images.iter().map(crate::composer::image_ref_json).collect::<Vec<_>>());
        }
        if !answers.is_null() {
            params["answers"] = answers;
        }
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "threads.send", params).await;
            let failed = res.is_err();
            this.update(cx, |this, cx| {
                this.error = res.err().map(|e| e.message);
                if failed {
                    // Off the end of the transcript and back in the box.
                    this.pending.retain(|m| m.id != pending_id);
                    let change = this.feed.set_pending(this.pending.clone());
                    this.apply(change, cx);
                }
                this.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
                cx.notify();
            })
            .ok();
            if failed {
                if let (Some(composer), Some(draft)) = (composer.upgrade(), draft) {
                    composer.update(cx, |c, cx| c.restore_draft(draft, cx));
                }
            }
        })
        .detach();
    }

    /// Return in an empty box allows the sole waiting approval, the ⏎ its
    /// Allow button shows (ComposerView.allowSoleApproval); with two, Return
    /// can't know which, and another runner asking for a thread is started
    /// only by a click on its card.
    pub(crate) fn allow_sole_approval(&mut self, cx: &mut Context<Self>) -> bool {
        let pending = self.feed.model().pending_approval_ids();
        let [id] = pending.as_slice() else { return false };
        let id = id.clone();
        let stacked = match self.feed.model().item(&format!("ap:{id}")) {
            Some(Row { content: RowContent::Approval(item), .. }) if item.is_start_request() => return false,
            Some(Row { content: RowContent::Approval(item), .. }) => !item.queued().is_empty(),
            _ => false,
        };
        // A stacked card's Return is its "Allow N": the ticked calls only,
        // the crossed ones refused (Codex review, 10 Nov 2026: a bare allow
        // here ran a call Mike had crossed out). Either way the card spins
        // until the answer lands.
        if stacked {
            self.respond_queued(&id, "allow", cx);
        } else {
            self.respond_approval(&id, "allow", None, None, cx);
        }
        true
    }

    /// The composer's picks on the open card, handed to the reducer so its
    /// row reads them (ThreadSession.setAskPicks and friends).
    pub(crate) fn set_card_picks(&mut self, seq: i64, picks: std::collections::BTreeMap<i64, i64>, notes: std::collections::BTreeMap<i64, threadbear_core::reducer::AskNote>, next: std::collections::BTreeSet<i64>, explain: std::collections::BTreeSet<i64>, cx: &mut Context<Self>) {
        let change = self.feed.set_ask_picks(seq, picks);
        self.apply(change, cx);
        let change = self.feed.set_ask_notes(seq, notes);
        self.apply(change, cx);
        let change = self.feed.set_next_picks(seq, next, explain);
        self.apply(change, cx);
        cx.notify();
    }

    /// Something the composer tried that didn't work, for the error line.
    pub(crate) fn show_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = Some(message);
        cx.notify();
    }

    pub(crate) fn request(&mut self, method: &'static str, params: Value, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, method, params).await;
            this.update(cx, |this, cx| {
                this.error = res.err().map(|e| e.message);
                this.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A request that says how it went, for cards that show a spinner until
    /// the answer lands and need their buttons back when it doesn't.
    pub(crate) fn request_then(&mut self, method: &'static str, params: Value, done: impl FnOnce(&mut Self, bool, &mut Context<Self>) + 'static, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, method, params).await;
            this.update(cx, |this, cx| {
                let failed = res.is_err();
                this.error = res.err().map(|e| e.message);
                done(this, failed, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The composer takes the cursor, as Send back on a plan asks.
    pub(crate) fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.clone().update(cx, |c, cx| c.focus(window, cx));
    }

    pub(crate) fn toggle_row(&mut self, row: &Row, cx: &mut Context<Self>) {
        let open = !self.row_open(row);
        self.set_open(row, open, cx);
    }

    /// "Show 30 more lines" under an opened diff: every line of it.
    pub(crate) fn show_all_lines(&mut self, key: &str, row_id: &str, cx: &mut Context<Self>) {
        self.open_rows.insert(key.to_owned());
        if let Some(i) = self.feed.model().index_of(row_id) {
            self.scroller.update(cx, |s, cx| {
                s.remeasure_items(i..i + 1, cx);
            });
        }
        cx.notify();
    }

    /// A file's line in the changes card: its diff opens and closes in the model.
    pub(crate) fn toggle_changed_file(&mut self, key: &str, cx: &mut Context<Self>) {
        let open = !self.feed.model().is_expanded(key);
        let change = self.feed.set_expanded(key, open);
        self.apply(change, cx);
        cx.notify();
    }

    /// "Show all" on a changes card's diff: the model's to open.
    pub(crate) fn show_all_changed(&mut self, key: &str, cx: &mut Context<Self>) {
        let change = self.feed.set_expanded(key, true);
        self.apply(change, cx);
        cx.notify();
    }

    /// Older events have been reached, or the list moved: asks for an older
    /// page when the top is near, and remembers where the view is.
    fn after_list_change(&mut self, cx: &mut Context<Self>) {
        self.load_older_if_near_top(cx);
        if self.loaded {
            crate::rows::remember_scroll(&self.summary.id, self.capture_memory(cx));
        }
    }

    /// `captureMemory`: pinned to the bottom, or the row at the top of the
    /// view and how far down it the view starts (a row part scrolled off the
    /// top has a negative offset, as the Swift app measures it).
    pub(crate) fn capture_memory(&self, cx: &App) -> crate::rows::ScrollMemory {
        let scroller = self.scroller.read(cx);
        if scroller.at_bottom() {
            return crate::rows::ScrollMemory { pinned: true, anchor_id: None, offset: 0. };
        }
        match scroller.top_row() {
            Some((index, offset_in_item)) => {
                let anchor_id = self.feed.model().rows().get(index).map(|r| r.id.clone());
                crate::rows::ScrollMemory { pinned: anchor_id.is_none(), anchor_id, offset: -f32::from(offset_in_item) }
            }
            None => crate::rows::ScrollMemory { pinned: true, anchor_id: None, offset: 0. },
        }
    }

    /// Back to a remembered place: a row still in the loaded rows, or the
    /// bottom if it's gone with an older page.
    pub(crate) fn restore_memory(&mut self, memory: Option<crate::rows::ScrollMemory>, cx: &mut Context<Self>) {
        let memory = memory.or_else(|| crate::rows::recalled_scroll(&self.summary.id));
        let Some(memory) = memory else { return };
        if memory.pinned {
            return;
        }
        if let Some(index) = memory.anchor_id.as_deref().and_then(|id| self.feed.model().index_of(id)) {
            self.scroller.update(cx, |s, cx| s.scroll_to_row(index, px(memory.offset), cx));
        } else if memory.anchor_id.is_some() && self.has_more_above() {
            // In an older page than the first one the pane has: bring pages
            // in until it's there, or they run out (`finish_older` retries).
            self.rows.pending_restore = Some(memory);
            self.load_older(cx);
        }
    }

    fn older_before(&self) -> i64 {
        let first = self.feed.model().first_seq();
        match self.older_skip {
            Some((at, before)) if at == first => before,
            _ => first,
        }
    }

    /// `hasMoreAbove`: there are events the runner hasn't sent.
    pub(crate) fn has_more_above(&self) -> bool {
        self.feed.model().has_older_events() && self.older_before() > 1
    }

    /// `loadOlderIfNearTop`: with GPUI's list every row of the loaded events
    /// is a row in the list already, so "near the top" is near its first
    /// row by count: the rows above it haven't been measured.
    fn load_older_if_near_top(&mut self, cx: &mut Context<Self>) {
        if !self.loaded || self.loading_older || !self.has_more_above() {
            return;
        }
        // While it follows the tail the list reports no top row: it's at the
        // bottom, as far from older rows as it gets.
        let near = self.scroller.read(cx).top_row().is_some_and(|(index, _)| index < 30);
        if near {
            self.load_older(cx);
        }
    }

    /// Fetches the next older page and adds it above, keeping the rows on
    /// screen where they are (`ThreadSession.loadOlder`).
    pub(crate) fn load_older(&mut self, cx: &mut Context<Self>) {
        if !self.loaded || self.loading_older || !self.has_more_above() {
            return;
        }
        self.loading_older = true;
        let before = self.older_before();
        let first_seq = self.feed.model().first_seq();
        let client = self.client.clone();
        let id = self.summary.id.clone();
        cx.spawn(async move |this, cx| {
            let page = call(client, "threads.events", json!({ "threadId": id, "beforeSeq": before, "limit": EVENTS_PAGE })).await;
            this.update(cx, |this, cx| this.finish_older(page, before, first_seq, cx)).ok();
        })
        .detach();
    }

    fn finish_older(&mut self, page: Result<Value, threadbear_client::runner::RpcError>, before: i64, first_seq: i64, cx: &mut Context<Self>) {
        let page = match page.map(|v| EventsPage::decode(&v)) {
            Ok(Some(page)) => page,
            _ => {
                self.loading_older = false;
                // Try again a few times, further apart each time.
                self.older_failures += 1;
                if self.older_failures <= 3 {
                    let delay = Duration::from_secs(u64::from(self.older_failures) * 2);
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(delay).await;
                        this.update(cx, |this, cx| this.load_older_if_near_top(cx)).ok();
                    })
                    .detach();
                }
                return;
            }
        };
        self.older_failures = 0;
        if !page.events.iter().any(|e| !e.is_transient() && e.seq < before) {
            // Step back past the range the page covered and go on.
            self.older_skip = Some((first_seq, (before - EVENTS_PAGE as i64).max(1)));
            self.loading_older = false;
            self.load_older_if_near_top(cx);
            return;
        }
        let memory = self.capture_memory(cx);
        let mut model = self.feed.model().clone();
        let result = model.prepend(page.events);
        if page.at_start {
            model.known_start = true;
        }
        self.feed.replace_model(model);
        let count = self.feed.model().rows().len();
        if result.keeps_order && result.added > 0 {
            let added = result.added;
            self.scroller.update(cx, |s, cx| {
                s.prepend(added, cx);
            });
            for id in result.changed_ids {
                if let Some(i) = self.feed.model().index_of(&id) {
                    self.scroller.update(cx, |s, cx| {
                        s.remeasure_items(i..i + 1, cx);
                    });
                }
            }
        } else if !result.keeps_order {
            // A row moved across the old first row (a result whose call was
            // on the older page), or the first row was a group that grew back
            // over the page and took a new id: the list can't just gain rows
            // at the top. Reload it around the anchor, or if that row's gone,
            // the first row that's still there.
            self.scroller.update(cx, |s, cx| s.reset(count, cx));
            let anchor = memory.anchor_id.as_deref().and_then(|id| self.feed.model().index_of(id));
            if !memory.pinned {
                let (index, offset) = match anchor {
                    Some(i) => (i, memory.offset),
                    None => (result.added, 0.),
                };
                self.scroller.update(cx, |s, cx| s.scroll_to_row(index, px(offset), cx));
            }
        }
        self.pending = self.feed.model().pending().to_vec();
        self.loading_older = false;
        cx.notify();
        if let Some(memory) = self.rows.pending_restore.take() {
            // Keeps paging if its row isn't here yet (`restore_memory`).
            self.restore_memory(Some(memory), cx);
        } else {
            self.load_older_if_near_top(cx);
        }
    }

    fn set_open(&mut self, row: &Row, open: bool, cx: &mut Context<Self>) {
        if matches!(row.content, RowContent::Group(_)) {
            let change = self.feed.set_expanded(&row.id, open);
            self.apply(change, cx);
        } else {
            if open {
                self.open_rows.insert(row.id.clone());
            } else {
                self.open_rows.remove(&row.id);
            }
            if let Some(i) = self.feed.model().index_of(&row.id) {
                self.scroller.update(cx, |s, cx| {
                    s.remeasure_items(i..i + 1, cx);
                });
            }
        }
        cx.notify();
    }

    pub(crate) fn row_open(&self, row: &Row) -> bool {
        match &row.content {
            RowContent::Group(group) => group.is_expanded,
            _ => self.open_rows.contains(&row.id),
        }
    }

    /// A work link in a reply: opens the step's group, scrolls to the step and
    /// lights it up for a moment (`openThreadLink`).
    pub(crate) fn open_work_link(&mut self, quote: &str, row_id: &str, _window: &mut Window, cx: &mut Context<Self>) {
        let model = self.feed.model();
        let Some(turn) = model.row(row_id).and_then(|r| r.turn_id.clone()) else { return };
        let Some((step, group)) = model.work_step(quote, &turn) else { return };
        if let Some(group) = group {
            let closed = model.row(&group).is_some_and(|r| matches!(&r.content, RowContent::Group(g) if !g.is_expanded));
            if closed {
                let change = self.feed.set_expanded(&group, true);
                self.apply(change, cx);
            }
        }
        if let Some(index) = self.feed.model().index_of(&step) {
            self.scroller.update(cx, |s, cx| s.scroll_to_row(index, px(0.), cx));
            self.light_row(step, cx);
        }
    }

    /// Lights a row up for a moment so the eye lands on the step (`light`).
    fn light_row(&mut self, row_id: String, cx: &mut Context<Self>) {
        self.rows.lit_row = Some(row_id.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(3300)).await;
            this.update(cx, |this, cx| {
                if this.rows.lit_row.as_deref() == Some(row_id.as_str()) {
                    this.rows.lit_row = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// A `thread:` link: the app opens it (its id or a unique start of one).
    pub(crate) fn open_thread_link(&mut self, id: &str, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(crate::rows::OpenThreadRequest(id.to_lowercase()));
    }

    /// Ticks the working line's elapsed time once a second while it's on
    /// screen, so the row itself never has to change as time passes.
    pub(crate) fn ensure_ticking(&mut self, active: bool, cx: &mut Context<Self>) {
        if !active || self.rows.tick_scheduled {
            return;
        }
        self.rows.tick_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            this.update(cx, |this, cx| {
                this.rows.tick_scheduled = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(feature = "automation")]
impl ThreadPane {
    /// The rows as the Swift app's dump gives them, with their heights once
    /// they've been measured.
    pub fn dump(&self, cx: &App) -> Value {
        let scroller = self.scroller.read(cx);
        let rows: Vec<Value> = self
            .feed
            .model()
            .rows()
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let mut out = json!({ "id": row.id, "kind": row.kind().as_str(), "text": threadbear_core::reducer::RowDebug::excerpt(row, 100), "inGroup": row.in_group, "seq": row.seq, "onScreen": scroller.row_on_screen(index) });
                if let Some(height) = scroller.row_height(index) {
                    out["height"] = json!(f32::from(height).round());
                }
                if let Some(turn) = &row.turn_id {
                    out["turn"] = json!(turn);
                }
                if matches!(row.content, RowContent::Group(_) | RowContent::Tool(_) | RowContent::Reasoning(_)) || row.kind() == threadbear_core::reducer::RowKind::Diff {
                    out["expanded"] = json!(self.row_open(row));
                }
                out
            })
            .collect();
        json!({ "threadId": self.summary.id, "state": if self.loaded { "loaded" } else { "loading" }, "atBottom": scroller.at_bottom(), "hasMoreAbove": self.has_more_above(), "rows": rows })
    }

    pub fn set_row_open(&mut self, row: &str, open: bool, cx: &mut Context<Self>) -> Result<(), String> {
        let found = self.feed.model().rows().iter().find(|r| r.id.starts_with(row)).cloned();
        let Some(found) = found else { return Err(format!("no row starting {row:?}")) };
        self.set_open(&found, open, cx);
        Ok(())
    }

    /// `scroll`: to the top or bottom, or a row (offset in points below the
    /// top of the view, as the Swift command has it).
    pub fn scroll_command(&mut self, to: Option<&str>, row: Option<&str>, offset: f32, cx: &mut Context<Self>) -> Result<(), String> {
        match to {
            Some("top") => self.scroller.update(cx, |s, cx| s.scroll_to_row(0, px(0.), cx)),
            Some("bottom") => self.scroller.update(cx, |s, cx| s.scroll_to_end(cx)),
            _ => {
                let Some(row) = row else { return Err("scroll needs to or row".into()) };
                let Some(index) = self.feed.model().rows().iter().position(|r| r.id.starts_with(row)) else { return Err(format!("no row starting {row:?}")) };
                self.scroller.update(cx, |s, cx| s.scroll_to_row(index, px(offset), cx));
            }
        }
        Ok(())
    }

    /// `link`: follows a `work:` or `thread:` link as a click in that row would.
    pub fn follow_link_command(&mut self, row: &str, url: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(row_id) = self.feed.model().rows().iter().find(|r| r.id.starts_with(row)).map(|r| r.id.clone()) else { return Err(format!("no row starting {row:?}")) };
        self.link_clicked(url, &row_id, window, cx);
        Ok(())
    }

    /// `expand`/`collapse` with `file` on a changes card, or `all` for a diff.
    pub fn expand_detail(&mut self, row: &str, file: Option<&Value>, all: bool, open: bool, cx: &mut Context<Self>) -> Result<(), String> {
        let found = self.feed.model().rows().iter().find(|r| r.id.starts_with(row)).cloned();
        let Some(found) = found else { return Err(format!("no row starting {row:?}")) };
        if let (RowContent::Changes(changes), Some(file)) = (&found.content, file) {
            let index = file.as_u64().map(|n| n as usize).or_else(|| changes.files.iter().position(|f| Some(f.path.as_str()) == file.as_str()));
            let Some(changed) = index.and_then(|i| changes.files.get(i)) else { return Err("no such file on that card".into()) };
            let key = threadbear_core::reducer::ChangedFiles::expansion_key(&changes.turn_id, &changed.path);
            let change = self.feed.set_expanded(&key, open);
            self.apply(change, cx);
            cx.notify();
            return Ok(());
        }
        if all {
            let key = threadbear_core::reducer::DiffPreview::all_key(&found.id);
            if open {
                self.open_rows.insert(found.id.clone());
                self.open_rows.insert(key);
            } else {
                self.open_rows.remove(&key);
            }
            if let Some(i) = self.feed.model().index_of(&found.id) {
                self.scroller.update(cx, |s, cx| {
                    s.remeasure_items(i..i + 1, cx);
                });
            }
            cx.notify();
            return Ok(());
        }
        self.set_open(&found, open, cx);
        Ok(())
    }

    /// Every event the thread has, for the long-thread check.
    pub fn load_all_events(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let id = self.summary.id.clone();
        let root = self.summary.cwd.clone();
        cx.spawn(async move |this, cx| {
            let page = call(client, "threads.events", json!({ "threadId": id, "limit": 1000000 })).await;
            this.update(cx, |this, cx| {
                if let Some(page) = page.ok().and_then(|v| EventsPage::decode(&v)) {
                    let mut model = TranscriptModel::new(Some(root));
                    model.load(page.events, &page.in_progress);
                    this.feed = ThreadFeed::new(model);
                    this.scroller.update(cx, |s, cx| s.reset(this.feed.model().rows().len(), cx));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

impl Render for ThreadPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger, bg) = (theme.muted_foreground, theme.danger, theme.background);
        self.rows.list_width = f32::from(self.scroller.read(cx).viewport().size.width);
        let composer = self.composer.clone();
        // The thread's title and project line are in the panel's bar (header.rs).
        // The whole thread is drop space for the box (ComposerDropView), outlined
        // while a file is over it. It's a layer over the thread for as long as a
        // file's being dragged, because selectable text under the pointer takes
        // the hover for itself, and a drop on a reply then went nowhere.
        let dragging_file = cx.has_active_drag();
        let drop_layer = dragging_file.then(|| {
            div()
                .absolute()
                .inset_0()
                .on_drop(move |paths: &ExternalPaths, window, cx| {
                    let paths = paths.paths().to_vec();
                    composer.update(cx, |c, cx| c.take_paths(paths, window, cx));
                })
                .drag_over::<ExternalPaths>(|s, _, _, cx| s.bg(crate::style::accent(cx).opacity(0.06)).border_2().border_color(crate::style::accent(cx)))
        });
        v_flex()
            .relative()
            .size_full()
            .when_some(self.error.clone(), |s, e| s.child(div().px_7().py_1().text_sm().text_color(danger).child(e)))
            .when(!self.loaded && self.error.is_none(), |s| s.child(div().p_4().text_sm().text_center().text_color(muted).child("Loading…")))
            .child(div().flex_1().min_h_0().child(transcript_list(&self.scroller, bg, cx.processor(|this, ix: usize, window, cx| this.render_item(ix, window, cx)), cx)))
            .when_some(crate::viewer::action_chips(&self.summary, cx), |s, chips| s.child(chips))
            .child(self.composer.clone())
            .children(drop_layer)
    }
}
