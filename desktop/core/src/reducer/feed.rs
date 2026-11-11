// Keeps a thread's transcript in step with the runner: buffers live events,
// orders persisted ones by seq, spots gaps that need a catch-up fetch, and
// hands batches to the reducer when the view asks. A port of
// Transcript/ThreadFeed.swift.
use super::event::{BackgroundTask, Event, EventPayload, InProgress, LooseEnds, StartRequest};
use super::model::{TranscriptChange, TranscriptModel};
use super::pending::PendingMessage;
use super::row::AskNote;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedAction {
    None,
    /// Something is ready to apply; flush on the next frame tick.
    ScheduleFlush,
    /// A persisted event skipped ahead. Fetch `threads.events(afterSeq:)`.
    NeedsCatchUp { after_seq: i64 },
}

pub struct ThreadFeed {
    model: TranscriptModel,
    /// Events ready to apply, in arrival order with persisted ones contiguous.
    pending: Vec<Event>,
    /// Persisted events that arrived ahead of a gap, keyed by seq.
    ahead: BTreeMap<i64, Event>,
    /// Highest persisted seq accepted into `pending` or the model.
    accepted_seq: i64,
    is_catching_up: bool,
    /// A `threads.subscribe` is in flight. Deltas pushed meanwhile come
    /// after the reply's in-progress text, so they're held until it lands.
    subscribing: bool,
    held: Vec<Event>,
    /// Catch-up fetches in a row that didn't move `accepted_seq` on.
    stalled_catch_ups: u32,
    sparse: bool,
}

impl ThreadFeed {
    /// After this many fruitless catch-ups the hole is skipped: the runner
    /// doesn't have those seqs, and waiting for them would freeze the thread
    /// for good.
    pub const MAX_STALLED_CATCH_UPS: u32 = 3;

    pub fn new(model: TranscriptModel) -> ThreadFeed {
        let accepted_seq = model.last_seq();
        ThreadFeed {
            model,
            pending: Vec::new(),
            ahead: BTreeMap::new(),
            accepted_seq,
            is_catching_up: false,
            subscribing: false,
            held: Vec::new(),
            stalled_catch_ups: 0,
            sparse: false,
        }
    }

    pub fn model(&self) -> &TranscriptModel {
        &self.model
    }

    pub fn accepted_seq(&self) -> i64 {
        self.accepted_seq
    }

    pub fn is_catching_up(&self) -> bool {
        self.is_catching_up
    }

    pub fn stalled_catch_ups(&self) -> u32 {
        self.stalled_catch_ups
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Another runner's thread read through Convex: its seqs have gaps by
    /// design and an event can come late, so every persisted event is taken
    /// as it comes, never waited on, and the model places a late one where
    /// it belongs.
    pub fn set_sparse(&mut self, sparse: bool) {
        self.sparse = sparse;
        self.model.sparse = sparse;
    }

    pub fn sparse(&self) -> bool {
        self.sparse
    }

    pub fn receive(&mut self, event: Event) -> FeedAction {
        if event.is_transient() && self.subscribing {
            self.held.push(event);
            return FeedAction::None;
        }
        if event.is_transient() {
            self.pending.push(event);
            return FeedAction::ScheduleFlush;
        }
        if self.sparse {
            self.accepted_seq = self.accepted_seq.max(event.seq);
            self.pending.push(event);
            return FeedAction::ScheduleFlush;
        }
        if event.seq <= self.accepted_seq {
            return FeedAction::None;
        }
        if event.seq == self.accepted_seq + 1 {
            self.accept(event);
            self.drain_ahead();
            return FeedAction::ScheduleFlush;
        }
        self.ahead.insert(event.seq, event);
        if self.is_catching_up {
            return FeedAction::None;
        }
        self.is_catching_up = true;
        FeedAction::NeedsCatchUp { after_seq: self.accepted_seq }
    }

    pub fn receive_all(&mut self, events: Vec<Event>) -> FeedAction {
        let mut result = FeedAction::None;
        for event in events {
            match self.receive(event) {
                FeedAction::None => {}
                FeedAction::ScheduleFlush => {
                    if result == FeedAction::None {
                        result = FeedAction::ScheduleFlush;
                    }
                }
                action @ FeedAction::NeedsCatchUp { .. } => result = action,
            }
        }
        result
    }

    /// Call before `threads.subscribe`: events pushed while it's in flight
    /// are held until its reply arrives through `finish_subscribe`, instead
    /// of starting a second fetch.
    pub fn begin_subscribe(&mut self) {
        self.is_catching_up = true;
        self.subscribing = true;
        self.held.clear();
    }

    /// Feeds a `threads.subscribe` reply. Its `in_progress` is what was
    /// streamed before the subscription started, including deltas that went
    /// out between the `threads.events` load and the subscribe, which this
    /// app never got; it replaces each streaming row's text, then the deltas
    /// held since `begin_subscribe` carry on from it.
    pub fn finish_subscribe(&mut self, events: Vec<Event>, in_progress: &[InProgress]) -> FeedAction {
        self.subscribing = false;
        let mut action = self.catch_up(events);
        for item in in_progress {
            self.pending.push(Event::new(
                self.accepted_seq,
                "",
                "",
                None,
                "inProgress",
                EventPayload::StreamingText { item_id: item.item_id.clone(), is_reasoning: item.type_ == "reasoning", text: item.text.clone() },
            ));
        }
        self.pending.append(&mut self.held);
        if action == FeedAction::None && !self.pending.is_empty() {
            action = FeedAction::ScheduleFlush;
        }
        action
    }

    /// Feeds the result of a catch-up fetch (or a subscribe replay). Returns
    /// another catch-up if a gap is still open (the page was limited).
    pub fn catch_up(&mut self, mut events: Vec<Event>) -> FeedAction {
        self.is_catching_up = false;
        events.sort_by_key(|e| e.seq);
        if self.sparse {
            for event in events {
                if !event.is_transient() {
                    self.accepted_seq = self.accepted_seq.max(event.seq);
                    self.pending.push(event);
                }
            }
            return if self.pending.is_empty() { FeedAction::None } else { FeedAction::ScheduleFlush };
        }
        let before = self.accepted_seq;
        for event in events {
            if event.is_transient() || event.seq <= self.accepted_seq {
                continue;
            }
            if event.seq == self.accepted_seq + 1 {
                self.accept(event);
                self.drain_ahead();
            } else {
                self.ahead.insert(event.seq, event);
            }
        }
        if !self.ahead.is_empty() {
            self.stalled_catch_ups = if self.accepted_seq == before { self.stalled_catch_ups + 1 } else { 0 };
            if self.stalled_catch_ups >= Self::MAX_STALLED_CATCH_UPS {
                if let Some(next) = self.ahead.keys().next().copied() {
                    self.stalled_catch_ups = 0;
                    self.accepted_seq = next - 1;
                    self.drain_ahead();
                }
            }
        }
        if !self.ahead.is_empty() {
            self.is_catching_up = true;
            return FeedAction::NeedsCatchUp { after_seq: self.accepted_seq };
        }
        self.stalled_catch_ups = 0;
        if self.pending.is_empty() {
            FeedAction::None
        } else {
            FeedAction::ScheduleFlush
        }
    }

    /// A catch-up fetch failed (the connection dropped, say). Doesn't count
    /// as a stall; the next subscribe replays from `accepted_seq`.
    pub fn catch_up_failed(&mut self) {
        self.is_catching_up = false;
        // A failed subscribe: the next one brings fresh in-progress text.
        self.subscribing = false;
        self.held.clear();
    }

    /// Applies everything pending to the model.
    pub fn flush(&mut self) -> TranscriptChange {
        if self.pending.is_empty() {
            return TranscriptChange::default();
        }
        let batch = std::mem::take(&mut self.pending);
        self.model.apply(batch)
    }

    pub fn set_expanded(&mut self, key: &str, open: bool) -> TranscriptChange {
        self.model.set_expanded(key, open)
    }

    pub fn set_ask_picks(&mut self, seq: i64, picks: BTreeMap<i64, i64>) -> TranscriptChange {
        self.model.set_ask_picks(seq, picks)
    }

    pub fn set_start_request(&mut self, approval_id: &str, request: StartRequest) -> TranscriptChange {
        self.model.set_start_request(approval_id, request)
    }

    pub fn set_ask_notes(&mut self, seq: i64, notes: BTreeMap<i64, AskNote>) -> TranscriptChange {
        self.model.set_ask_notes(seq, notes)
    }

    pub fn set_next_picks(&mut self, seq: i64, picks: BTreeSet<i64>, explain: BTreeSet<i64>) -> TranscriptChange {
        self.model.set_next_picks(seq, picks, explain)
    }

    pub fn set_loose_ends(&mut self, check: Option<LooseEnds>) -> TranscriptChange {
        self.model.set_loose_ends(check)
    }

    pub fn set_background(&mut self, tasks: Vec<BackgroundTask>) -> TranscriptChange {
        self.model.set_background(tasks)
    }

    pub fn set_pending(&mut self, list: Vec<PendingMessage>) -> TranscriptChange {
        self.model.set_pending(list)
    }

    pub fn set_live_turn(&mut self, turn: Option<&str>, started_at: Option<f64>) -> TranscriptChange {
        self.model.set_live_turn(turn, started_at)
    }

    /// Swaps in a model that had an older page prepended off the main
    /// thread. Only valid if nothing was flushed since the copy was taken.
    pub fn replace_model(&mut self, model: TranscriptModel) {
        self.model = model;
    }

    /// Replaces the model (after a reload) and forgets queued events that
    /// it covers.
    pub fn reset(&mut self, mut model: TranscriptModel) {
        model.sparse = self.sparse;
        self.accepted_seq = model.last_seq();
        self.model = model;
        let accepted = self.accepted_seq;
        self.pending.retain(|e| e.is_transient() || e.seq > accepted);
        self.ahead.retain(|seq, _| *seq > accepted);
        self.is_catching_up = false;
        self.drain_ahead();
    }

    fn accept(&mut self, event: Event) {
        self.accepted_seq = event.seq;
        self.pending.push(event);
    }

    fn drain_ahead(&mut self) {
        while let Some(next) = self.ahead.remove(&(self.accepted_seq + 1)) {
            self.accept(next);
        }
    }
}
