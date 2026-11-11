// Turns protocol events into transcript rows: a port of
// Transcript/TranscriptReducer.swift, kept in the Swift's order, with the
// Swift's reasons in its comments, so one can be read beside the other.
//
// There are two lists. `items` has one row per thing the agent did (a
// message, a tool call with its result, a thinking block). `rows` is what the
// table shows: each run of work inside a turn folded into a group row (with
// its items under it when it's open), plus the derived rows: the "Edited 3
// files" card before a turn's footer and the "Working for 12s" line while a
// turn runs. Live changes only rebuild `rows` from the first run they touch
// to the end, which is nearly always the current turn.
use super::diff::DiffBuilder;
use super::event::*;
use super::format::{duration, TurnTokens};
use super::groups::*;
use super::pending::PendingMessage;
use super::row::*;
use super::turn_gap::{parse_iso8601, TurnGap};
use super::work_links::WorkLinks;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::{Range, RangeInclusive};

#[derive(Clone, Debug, PartialEq, Default)]
pub struct PrependResult {
    /// Rows that appeared above the previous first row.
    pub added: usize,
    /// Rows that were already there but changed (a tool call that met its
    /// result across the page boundary, say).
    pub changed_ids: Vec<String>,
    /// True when the rows from `added` on are exactly the previous rows, in
    /// order, so a view can insert `added` rows at the top. False when a row
    /// moved above the old first row or an older event's row landed below
    /// it; the view then has to reload instead.
    pub keeps_order: bool,
}

/// What changed in the row list after applying events or opening a group
/// (`TranscriptChange`). `removed` are indices in the list before the
/// change; `inserted` and `updated` in the list after it. A list removes,
/// then inserts, then reloads.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TranscriptChange {
    pub removed: BTreeSet<usize>,
    pub inserted: BTreeSet<usize>,
    pub updated: BTreeSet<usize>,
    /// Persisted events arrived with a seq gap; refetch after `last_seq`.
    pub gap_detected: bool,
}

impl TranscriptChange {
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.inserted.is_empty() && self.updated.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum StreamKind {
    Assistant,
    Reasoning,
}

/// `TranscriptModel`.
#[derive(Clone, Debug, Default)]
pub struct TranscriptModel {
    rows: Vec<Row>,
    items: Vec<Row>,
    /// Persisted events we hold, in seq order.
    events: Vec<Event>,
    first_seq: i64,
    last_seq: i64,
    /// Open groups (`g:` ids) and open files in changed-files cards. Kept
    /// across rebuilds.
    expanded: HashSet<String>,
    /// The turn that has started and not finished, if any.
    open_turn: Option<String>,
    /// Text streamed by deltas for items with no persisted event yet.
    streaming: HashMap<String, (StreamKind, String)>,
    item_index: HashMap<String, usize>,
    index: HashMap<String, usize>,
    /// For each row, the index of the first item it stands for. Never
    /// decreases, so the rows for items from `n` on are a binary search away.
    row_starts: Vec<usize>,
    plan_row_for_turn: HashMap<String, String>,
    /// "Plan first": the approved plan card that takes the steps the agent
    /// then works through (plan.updated), and the turn it does so in (None
    /// until it starts: Codex's goes ahead in the next turn).
    plan_taking_steps: Option<String>,
    plan_steps_turn: Option<String>,
    /// Approval requests for a plan card's ExitPlanMode call, which get no row.
    plan_approvals: HashSet<String>,
    /// Rows of commands Mike ran from a chip: left out of the turn's line
    /// of work before them, so what he pressed shows where he pressed it.
    pressed_runs: HashSet<String>,
    /// Cards of asks by the seq of their turn.completed, and the open one
    /// (the last finished turn's), if it has asks.
    asks_row_for_seq: HashMap<i64, String>,
    open_asks_row: Option<String>,
    /// The option Mike picked on each card's asks (ask index to option
    /// index), by the card's seq. Kept here rather than in the rows, so a
    /// rebuild keeps them; only the app sets them.
    ask_picks: BTreeMap<i64, BTreeMap<i64, i64>>,
    /// What Mike has written in each card's Something else boxes.
    ask_notes: BTreeMap<i64, BTreeMap<i64, AskNote>>,
    /// Other runners' requests for a thread, read by approval id.
    start_requests: BTreeMap<String, StartRequest>,
    /// The follow-ups Mike ticked on each done card, by the card's seq.
    next_picks: BTreeMap<i64, BTreeSet<i64>>,
    /// The follow-ups with their "?" on, by card seq.
    next_explains: BTreeMap<i64, BTreeSet<i64>>,
    /// The runner's loose-ends check for a done card, from the summary.
    loose_ends: Option<LooseEnds>,
    /// Each turn's last error row, to learn the turn's `error_kind`.
    error_row_for_turn: HashMap<String, String>,
    turn_starts: HashMap<String, f64>,
    /// When the last turn ended, and the turns anyone's message started: a
    /// turn with no message is one the agent started itself, and the wait
    /// before it gets a divider.
    last_turn_end: Option<f64>,
    messaged_turns: HashSet<String>,
    /// Turns whose `turn.started` has been seen: a message after it went
    /// into that turn, so it can't be edited on its own.
    started_turns: HashSet<String>,
    /// The seqs edits cut (`thread.rewound`), from every rewind event held.
    /// Their events stay in `events` but make no rows.
    hidden: Vec<RangeInclusive<i64>>,
    completed_turns: HashSet<String>,
    /// Turns with a summary from end_turn: their other messages are work.
    summarized_turns: HashSet<String>,
    /// Turns with a One reply final message: like a summary, it's the reply,
    /// so the turn gets no changes card either.
    replied_turns: HashSet<String>,
    /// Edit and Write tool items per turn, in order, for the changes card.
    edits_by_turn: HashMap<String, Vec<String>>,
    /// The seq each tool row's result came at, by row id.
    result_seqs: HashMap<String, i64>,
    /// Approval and question rows still waiting, and their turn. A turn
    /// waiting on Mike isn't "working". BTreeMap, not HashMap: the Swift's
    /// dictionary order is random, and rows must come out the same each run.
    pending_cards: BTreeMap<String, String>,
    /// Approval rows by the `t:` row of the tool call they asked about.
    approval_for_tool: HashMap<String, String>,
    /// The first item changed in place, and the first item added, since
    /// `rows` was last brought up to date.
    dirty_update: Option<usize>,
    dirty_insert: Option<usize>,
    /// One past the last item changed in place (and a changed edit's turn
    /// footer, whose card counts its lines). Changes in place only rebuild
    /// the rows up to here.
    dirty_update_end: usize,
    /// Something the working line depends on changed, so the rows to the end
    /// need rebuilding.
    tail_dirty: bool,
    /// The thread's background sub-agents and jobs, from its summary.
    background: Vec<BackgroundTask>,
    /// The thread's working folder.
    root: Option<String>,
    /// Messages sent from this app that the runner hasn't shown yet.
    pending: Vec<PendingMessage>,
    /// The runner said there's nothing before the first event (a page's
    /// `relay.atStart`).
    pub known_start: bool,
    /// Takes persisted events that arrive behind ones already applied, and
    /// puts them in order (`ThreadFeed.sparse`).
    pub sparse: bool,
}

impl TranscriptModel {
    pub fn new(root: Option<String>) -> TranscriptModel {
        TranscriptModel { root, ..TranscriptModel::default() }
    }

    pub fn from_events(events: Vec<Event>, root: Option<String>) -> TranscriptModel {
        let mut model = TranscriptModel::new(root);
        model.load(events, &[]);
        model
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn items(&self) -> &[Row] {
        &self.items
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn first_seq(&self) -> i64 {
        self.first_seq
    }

    pub fn last_seq(&self) -> i64 {
        self.last_seq
    }

    pub fn expanded(&self) -> &HashSet<String> {
        &self.expanded
    }

    pub fn open_turn(&self) -> Option<&str> {
        self.open_turn.as_deref()
    }

    pub fn hidden(&self) -> &[RangeInclusive<i64>] {
        &self.hidden
    }

    pub fn background(&self) -> &[BackgroundTask] {
        &self.background
    }

    pub fn root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    pub fn pending(&self) -> &[PendingMessage] {
        &self.pending
    }

    pub fn ask_picks(&self) -> &BTreeMap<i64, BTreeMap<i64, i64>> {
        &self.ask_picks
    }

    pub fn ask_notes(&self) -> &BTreeMap<i64, BTreeMap<i64, AskNote>> {
        &self.ask_notes
    }

    pub fn start_requests(&self) -> &BTreeMap<String, StartRequest> {
        &self.start_requests
    }

    pub fn next_picks(&self) -> &BTreeMap<i64, BTreeSet<i64>> {
        &self.next_picks
    }

    pub fn next_explains(&self) -> &BTreeMap<i64, BTreeSet<i64>> {
        &self.next_explains
    }

    pub fn loose_ends(&self) -> Option<&LooseEnds> {
        self.loose_ends.as_ref()
    }

    pub fn has_older_events(&self) -> bool {
        self.first_seq > 1 && !self.known_start
    }

    /// A row in `rows` (shown in the list).
    pub fn row(&self, id: &str) -> Option<&Row> {
        self.index.get(id).map(|&i| &self.rows[i])
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.index.get(id).copied()
    }

    /// The last finished turn's card while it's open (Needs you or Done):
    /// the app draws it in the composer, not the transcript. A message on
    /// its way is his reply already, so the card goes as soon as he sends
    /// it, and comes back if the send fails.
    pub fn open_card(&self) -> Option<&Row> {
        if self.pending.is_empty() {
            self.open_asks_row.as_deref().and_then(|id| self.row(id))
        } else {
            None
        }
    }

    /// An item, shown or folded away in a closed group.
    pub fn item(&self, id: &str) -> Option<&Row> {
        self.item_index.get(id).map(|&i| &self.items[i])
    }

    pub fn is_expanded(&self, key: &str) -> bool {
        self.expanded.contains(key)
    }

    /// Approval cards (not questions) still waiting on Mike, oldest first.
    /// Return in an empty composer allows the one there is.
    pub fn pending_approval_ids(&self) -> Vec<String> {
        let mut ids: Vec<&String> = self.pending_cards.keys().filter(|k| k.starts_with("ap:")).collect();
        ids.sort_by_key(|k| self.item_index.get(k.as_str()).copied().unwrap_or(0));
        ids.into_iter().map(|k| k[3..].to_owned()).collect()
    }

    /// Replaces everything with this page of events (oldest first).
    pub fn load(&mut self, new_events: Vec<Event>, in_progress: &[InProgress]) {
        let mut events: Vec<Event> = new_events.into_iter().filter(|e| !e.is_transient()).collect();
        events.sort_by_key(|e| e.seq);
        self.first_seq = events.first().map(|e| e.seq).unwrap_or(0);
        self.last_seq = events.last().map(|e| e.seq).unwrap_or(0);
        self.events = events;
        PendingMessage::remove_landed(self.events.iter(), &mut self.pending);
        self.streaming.clear();
        for item in in_progress {
            let kind = if item.type_ == "reasoning" { StreamKind::Reasoning } else { StreamKind::Assistant };
            self.streaming.insert(item.item_id.clone(), (kind, item.text.clone()));
        }
        self.rebuild();
    }

    /// Adds an older page in front. Returns how many rows appeared above the
    /// previous first row, so the view can keep its scroll anchor.
    pub fn prepend(&mut self, older_events: Vec<Event>) -> PrependResult {
        let first_seq = self.first_seq;
        let mut older: Vec<Event> = older_events.into_iter().filter(|e| !e.is_transient() && (first_seq == 0 || e.seq < first_seq)).collect();
        if older.is_empty() {
            return PrependResult { added: 0, changed_ids: Vec::new(), keeps_order: true };
        }
        let previous_ids: Vec<String> = self.rows.iter().map(|r| r.id.clone()).collect();
        let mut previous: HashMap<String, u64> = HashMap::new();
        for row in &self.rows {
            previous.entry(row.id.clone()).or_insert(row.version);
        }
        older.sort_by_key(|e| e.seq);
        older.append(&mut self.events);
        self.events = older;
        self.first_seq = self.events.first().map(|e| e.seq).unwrap_or(0);
        self.rebuild();
        let changed: Vec<String> = self.rows.iter().filter(|row| previous.get(&row.id).is_some_and(|v| *v != row.version)).map(|row| row.id.clone()).collect();
        let Some(previous_first) = previous_ids.first() else {
            return PrependResult { added: 0, changed_ids: changed, keeps_order: true };
        };
        let Some(&new_index) = self.index.get(previous_first) else {
            // The old first row is gone: a group grew backwards over the page
            // boundary and took a new id, or swallowed a lone item. Anchor on
            // the first old row that's still there; the view reloads.
            let survivor = previous_ids.iter().find_map(|id| self.index.get(id).copied()).unwrap_or(0);
            return PrependResult { added: survivor, changed_ids: changed, keeps_order: false };
        };
        let keeps_order =
            self.rows.len() - new_index == previous_ids.len() && self.rows[new_index..].iter().zip(&previous_ids).all(|(row, id)| row.id == *id);
        PrependResult { added: new_index, changed_ids: changed, keeps_order }
    }

    /// Drops all but the newest `count` persisted events (and their rows),
    /// keeping anything still streaming. Older pages come back through
    /// `prepend` if they're scrolled to again.
    pub fn trim(&mut self, keeping_last: usize) {
        if self.events.len() <= keeping_last {
            return;
        }
        let keep_from = self.events.len() - keeping_last;
        self.events.drain(..keep_from);
        // What's gone can be read again.
        self.known_start = false;
        self.first_seq = self.events.first().map(|e| e.seq).unwrap_or(0);
        self.rebuild();
    }

    /// Applies live events (persisted or transient) and reports what changed.
    pub fn apply(&mut self, incoming: Vec<Event>) -> TranscriptChange {
        let mut gap = false;
        let mut rewound = false;
        let mut late: Vec<Event> = Vec::new();
        for event in incoming {
            if !event.is_transient() {
                if event.seq <= self.last_seq && self.last_seq != 0 {
                    if self.sparse && event.seq >= self.first_seq && !self.events.iter().any(|e| e.seq == event.seq) {
                        late.push(event);
                    }
                    continue;
                }
                if self.last_seq != 0 && event.seq > self.last_seq + 1 {
                    gap = true;
                }
                self.events.push(event.clone());
                if self.first_seq == 0 {
                    self.first_seq = event.seq;
                }
                self.last_seq = event.seq;
                if matches!(event.payload, EventPayload::ThreadRewound(_)) {
                    rewound = true;
                }
                if PendingMessage::remove_landed(std::iter::once(&event), &mut self.pending) {
                    self.tail_dirty = true;
                }
            }
            self.reduce(&event);
        }
        // An edit cut rows that are already showing, or events came late:
        // build them all again and have the list reload, rather than work
        // out what moved.
        if !late.is_empty() {
            self.events.extend(late);
            self.events.sort_by_key(|e| e.seq);
            let before = self.rows.len();
            self.rebuild();
            return TranscriptChange { removed: (0..before).collect(), inserted: (0..self.rows.len()).collect(), updated: BTreeSet::new(), gap_detected: false };
        }
        if rewound {
            // The runner drops messages queued behind an edited one
            // (docs/protocol.md, "Editing a message").
            self.pending.retain(|m| !m.queued);
            let before = self.rows.len();
            self.rebuild();
            return TranscriptChange { removed: (0..before).collect(), inserted: (0..self.rows.len()).collect(), updated: BTreeSet::new(), gap_detected: gap };
        }
        let mut change = self.refresh_rows();
        change.gap_detected = gap;
        change
    }

    /// Whether an edit has cut the event at `seq`.
    pub fn is_hidden(&self, seq: i64) -> bool {
        self.hidden.iter().any(|range| range.contains(&seq))
    }

    /// The seqs cut by the `thread.rewound` events among `events`, merged.
    /// A later edit can cut back over an earlier one's marker; its range
    /// still counts.
    pub fn hidden_ranges(events: &[Event]) -> Vec<RangeInclusive<i64>> {
        let mut ranges: Vec<RangeInclusive<i64>> = Vec::new();
        for event in events {
            if let EventPayload::ThreadRewound(rewind) = &event.payload {
                if let Some(range) = rewind.range() {
                    ranges.push(range);
                }
            }
        }
        ranges.sort_by_key(|r| *r.start());
        ranges
    }

    /// The thread's summary said what's running in the background. The line
    /// at the end comes or goes (or changes its words) in place.
    pub fn set_background(&mut self, tasks: Vec<BackgroundTask>) -> TranscriptChange {
        if tasks == self.background {
            return TranscriptChange::default();
        }
        self.background = tasks;
        self.tail_dirty = true;
        self.refresh_rows()
    }

    /// The messages sent from this app that the runner hasn't shown yet.
    /// Any that the events held already show are left out (a reload can
    /// bring one in).
    pub fn set_pending(&mut self, mut list: Vec<PendingMessage>) -> TranscriptChange {
        if let Some(earliest) = list.iter().map(|m| m.after_seq).min() {
            // Only the events after `earliest`, in order.
            let start = self.events.iter().rposition(|e| e.seq <= earliest).map(|p| p + 1).unwrap_or(0);
            PendingMessage::remove_landed(self.events[start..].iter(), &mut list);
        }
        if list == self.pending {
            return TranscriptChange::default();
        }
        self.pending = list;
        self.tail_dirty = true;
        self.refresh_rows()
    }

    /// Marks the transcript as having a turn underway with no `turn.started`
    /// for it: a sub-agent's events (from `threads.subagent`) carry no turns,
    /// but while it runs it should read like one, with its last group live
    /// and the working line after it. `turn` "" matches events with no
    /// turn id. None ends it.
    pub fn set_live_turn(&mut self, turn: Option<&str>, started_at: Option<f64>) -> TranscriptChange {
        if turn == self.open_turn.as_deref() {
            return TranscriptChange::default();
        }
        self.open_turn = turn.map(str::to_owned);
        if let (Some(turn), Some(started_at)) = (turn, started_at) {
            self.turn_starts.insert(turn.to_owned(), started_at);
        }
        self.tail_dirty = true;
        // The last run of work becomes (or stops being) the live group.
        if !self.items.is_empty() {
            self.mark_inserted(self.items.len() - 1);
        }
        self.refresh_rows()
    }

    fn close_open_asks(&mut self) {
        let Some(open) = self.open_asks_row.take() else { return };
        self.update(&open, |content| {
            if let RowContent::Asks(card) = content {
                card.is_open = false;
            }
        });
    }

    /// Mike picked (or unpicked) answers on the card from `seq`.
    pub fn set_ask_picks(&mut self, seq: i64, picks: BTreeMap<i64, i64>) -> TranscriptChange {
        if self.ask_picks.get(&seq).cloned().unwrap_or_default() == picks {
            return TranscriptChange::default();
        }
        if picks.is_empty() {
            self.ask_picks.remove(&seq);
        } else {
            self.ask_picks.insert(seq, picks.clone());
        }
        let Some(card_id) = self.asks_row_for_seq.get(&seq).cloned() else { return TranscriptChange::default() };
        self.update(&card_id, |content| {
            if let RowContent::Asks(card) = content {
                card.picks = picks;
            }
        });
        self.refresh_rows()
    }

    /// Another runner's request was read for the card `approval_id`.
    pub fn set_start_request(&mut self, approval_id: &str, request: StartRequest) -> TranscriptChange {
        if self.start_requests.get(approval_id) == Some(&request) {
            return TranscriptChange::default();
        }
        self.start_requests.insert(approval_id.to_owned(), request.clone());
        let id = format!("ap:{approval_id}");
        if !self.item_index.contains_key(&id) {
            return TranscriptChange::default();
        }
        self.update(&id, |content| {
            if let RowContent::Approval(item) = content {
                item.start_request = Some(request);
            }
        });
        self.refresh_rows()
    }

    /// Mike wrote or pasted in the Something else boxes on the card from `seq`.
    pub fn set_ask_notes(&mut self, seq: i64, notes: BTreeMap<i64, AskNote>) -> TranscriptChange {
        if self.ask_notes.get(&seq).cloned().unwrap_or_default() == notes {
            return TranscriptChange::default();
        }
        if notes.is_empty() {
            self.ask_notes.remove(&seq);
        } else {
            self.ask_notes.insert(seq, notes.clone());
        }
        let Some(card_id) = self.asks_row_for_seq.get(&seq).cloned() else { return TranscriptChange::default() };
        self.update(&card_id, |content| {
            if let RowContent::Asks(card) = content {
                card.notes = notes;
            }
        });
        self.refresh_rows()
    }

    /// The thread's summary has a new loose-ends check (or none): it goes on
    /// the done card it's for.
    pub fn set_loose_ends(&mut self, check: Option<LooseEnds>) -> TranscriptChange {
        if check == self.loose_ends {
            return TranscriptChange::default();
        }
        let old = self.loose_ends.take();
        self.loose_ends = check.clone();
        let mut touched = false;
        let mut seqs: Vec<i64> = old.iter().chain(check.iter()).map(|c| c.seq).collect();
        seqs.sort_unstable();
        seqs.dedup();
        for seq in seqs {
            let Some(card_id) = self.asks_row_for_seq.get(&seq).cloned() else { continue };
            let check = check.clone();
            self.update(&card_id, |content| {
                if let RowContent::Asks(card) = content {
                    if card.is_done() {
                        card.loose_ends = check.filter(|c| c.seq == seq).map(|c| c.ends);
                    }
                }
            });
            touched = true;
        }
        if touched {
            self.refresh_rows()
        } else {
            TranscriptChange::default()
        }
    }

    /// Mike ticked (or unticked) follow-ups on the done card from `seq`.
    pub fn set_next_picks(&mut self, seq: i64, picks: BTreeSet<i64>, explain: BTreeSet<i64>) -> TranscriptChange {
        if self.next_picks.get(&seq).cloned().unwrap_or_default() == picks && self.next_explains.get(&seq).cloned().unwrap_or_default() == explain {
            return TranscriptChange::default();
        }
        if picks.is_empty() {
            self.next_picks.remove(&seq);
        } else {
            self.next_picks.insert(seq, picks.clone());
        }
        if explain.is_empty() {
            self.next_explains.remove(&seq);
        } else {
            self.next_explains.insert(seq, explain.clone());
        }
        let Some(card_id) = self.asks_row_for_seq.get(&seq).cloned() else { return TranscriptChange::default() };
        self.update(&card_id, |content| {
            if let RowContent::Asks(card) = content {
                card.picked_next = picks;
                card.explain_next = explain;
            }
        });
        self.refresh_rows()
    }

    /// Opens or closes a group (`g:` id) or a file in a changes card
    /// (`ChangedFiles::expansion_key`, or its `DiffPreview::all_key` to show
    /// all of the file's diff). Opening a group inserts its items under it;
    /// the group keeps its id, so its cached height survives.
    pub fn set_expanded(&mut self, key: &str, open: bool) -> TranscriptChange {
        if self.expanded.contains(key) == open {
            return TranscriptChange::default();
        }
        DiffPreview::set(key, open, &mut self.expanded);
        let row_id = if key.starts_with("cf:") { key.split('|').next().unwrap_or(key) } else { key };
        let Some(&position) = self.index.get(row_id) else { return TranscriptChange::default() };
        self.mark_updated(self.row_starts[position]);
        self.refresh_rows()
    }

    // ---- Items ----

    fn rebuild(&mut self) {
        // Items and rows whose content comes out the same keep their version,
        // so their cached renders and heights stay valid. The old rows stay
        // in place until the new ones are built: derived rows (groups, cards)
        // look their old versions up there.
        let mut previous: HashMap<String, Row> = HashMap::new();
        for item in std::mem::take(&mut self.items) {
            previous.entry(item.id.clone()).or_insert(item);
        }
        self.items = Vec::new();
        self.item_index.clear();
        self.plan_row_for_turn.clear();
        self.plan_taking_steps = None;
        self.plan_steps_turn = None;
        self.plan_approvals.clear();
        self.pressed_runs.clear();
        self.asks_row_for_seq.clear();
        self.open_asks_row = None;
        self.error_row_for_turn.clear();
        self.turn_starts.clear();
        self.last_turn_end = None;
        self.messaged_turns.clear();
        self.started_turns.clear();
        self.hidden = Self::hidden_ranges(&self.events);
        self.completed_turns.clear();
        self.summarized_turns.clear();
        self.replied_turns.clear();
        self.edits_by_turn.clear();
        self.result_seqs.clear();
        self.pending_cards.clear();
        self.approval_for_tool.clear();
        self.open_turn = None;
        self.items.reserve(self.events.len());
        // Set aside what's streaming now before replaying history: a
        // `turn.completed` in the history (any earlier turn) would otherwise
        // end it, and the live text would vanish.
        let streaming_now = std::mem::take(&mut self.streaming);
        let events = std::mem::take(&mut self.events);
        for event in &events {
            if !self.is_hidden(event.seq) {
                self.reduce(event);
            }
        }
        self.events = events;
        self.streaming.clear();
        // Re-add anything still streaming, unless the history now has its
        // persisted text. Reducing a delta puts it back in `streaming`.
        let mut sorted: Vec<(String, (StreamKind, String))> = streaming_now.into_iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (item_id, (kind, text)) in sorted {
            let prefix = if kind == StreamKind::Reasoning { "r:" } else { "a:" };
            if self.item_index.contains_key(&format!("{prefix}{item_id}")) {
                continue;
            }
            let payload = if kind == StreamKind::Reasoning {
                EventPayload::ReasoningDelta { item_id, text }
            } else {
                EventPayload::AssistantTextDelta { item_id, text }
            };
            let event = Event::new(self.last_seq, "", "", None, "", payload);
            self.reduce(&event);
        }
        for item in &mut self.items {
            if let Some(old) = previous.get(&item.id) {
                if old.content == item.content {
                    item.version = old.version;
                }
            }
        }
        // A turn whose start is on an older page (a long run of more than a
        // page of events) still counts as running if nothing in this page
        // finished it. A page that has the turn's message would have its
        // start too.
        if self.open_turn.is_none() && self.has_older_events() {
            if let Some(first) = self.events.first().and_then(|e| e.turn_id.clone()) {
                let last_with_turn = self.events.iter().rev().find_map(|e| e.turn_id.as_deref());
                let has_user_message = self.events.iter().any(|e| matches!(e.payload, EventPayload::UserMessage { .. }) && e.turn_id.as_deref() == Some(first.as_str()));
                if last_with_turn == Some(first.as_str()) && !self.completed_turns.contains(&first) && !has_user_message {
                    self.open_turn = Some(first);
                }
            }
        }
        let (rows, starts) = self.build_rows(0, None, true);
        self.rows = rows;
        self.row_starts = starts;
        self.index = HashMap::with_capacity(self.rows.len());
        for (i, row) in self.rows.iter().enumerate() {
            self.index.entry(row.id.clone()).or_insert(i);
        }
        self.dirty_update = None;
        self.dirty_insert = None;
        self.dirty_update_end = 0;
        self.tail_dirty = false;
    }

    fn mark_updated(&mut self, item_index: usize) {
        if item_index >= self.items.len() {
            self.mark_inserted(item_index);
            return;
        }
        self.dirty_update = Some(self.dirty_update.map_or(item_index, |d| d.min(item_index)));
        self.dirty_update_end = self.dirty_update_end.max(self.run_end(item_index as isize));
        // An edit in a finished turn changes its changes card too.
        if let RowContent::Tool(tool) = &self.items[item_index].content {
            if !tool.diffs.is_empty() || tool.call.as_ref().map(DiffBuilder::is_edit_tool).unwrap_or(false) {
                let turn = self.items[item_index].turn_id.clone().unwrap_or_default();
                if let Some(&footer) = self.item_index.get(&format!("f:{turn}")) {
                    self.dirty_update_end = self.dirty_update_end.max(footer + 1);
                }
            }
        }
    }

    fn mark_inserted(&mut self, item_index: usize) {
        self.dirty_insert = Some(self.dirty_insert.map_or(item_index, |d| d.min(item_index)));
    }

    fn append(&mut self, mut row: Row) {
        row.version = next_row_version();
        self.mark_inserted(self.items.len());
        self.item_index.insert(row.id.clone(), self.items.len());
        self.items.push(row);
    }

    /// Inserts right after the item with `after_id` (used for a tool's images).
    fn insert(&mut self, mut row: Row, after_id: &str) {
        let anchor = match self.item_index.get(after_id) {
            Some(&anchor) if anchor < self.items.len() - 1 => anchor,
            _ => {
                self.append(row);
                return;
            }
        };
        row.version = next_row_version();
        self.items.insert(anchor + 1, row);
        for i in (anchor + 1)..self.items.len() {
            self.item_index.insert(self.items[i].id.clone(), i);
        }
        self.mark_inserted(anchor + 1);
    }

    fn update(&mut self, id: &str, body: impl FnOnce(&mut RowContent)) {
        let Some(&i) = self.item_index.get(id) else { return };
        body(&mut self.items[i].content);
        self.items[i].version = next_row_version();
        self.mark_updated(i);
    }

    /// A tool call got its row (`t:` id): the approval that asked about it,
    /// if any, stops opening to the same thing once it's settled. The runner
    /// sends the call first, but an older page can bring the approval after
    /// it.
    fn note_tool_row(&mut self, id: &str) {
        let Some(approval) = self.approval_for_tool.get(id).cloned() else { return };
        match self.item(&approval).map(|row| &row.content) {
            Some(RowContent::Approval(existing)) if !existing.tool_shown => {}
            _ => return,
        }
        self.update(&approval, |content| {
            if let RowContent::Approval(item) = content {
                item.tool_shown = true;
            }
        });
    }

    fn note_edit(&mut self, item_id: &str, turn_id: Option<&str>) {
        let edits = self.edits_by_turn.entry(turn_id.unwrap_or("").to_owned()).or_default();
        if !edits.iter().any(|e| e == item_id) {
            edits.push(item_id.to_owned());
        }
    }

    fn reduce(&mut self, event: &Event) {
        let seq = event.seq;
        let turn_id = event.turn_id.clone();
        match &event.payload {
            EventPayload::UserMessage { text, images, origin, answers, edit_of, review } => {
                // Mike's reply answers the open card, so it goes, picked or not.
                if origin.as_deref() != Some("runner") {
                    self.close_open_asks();
                }
                let mut message = UserMessage {
                    text: text.clone(),
                    images: images.clone(),
                    origin: origin.clone(),
                    answers: answers.clone(),
                    edit_of: *edit_of,
                    review: review.clone(),
                    ..UserMessage::default()
                };
                // Only one that started its own turn can be cut back to; one
                // sent into a running turn went into that turn.
                message.editable = message.is_mike() && turn_id.as_ref().is_some_and(|t| !self.started_turns.contains(t));
                if let Some(turn) = &turn_id {
                    self.messaged_turns.insert(turn.clone());
                }
                self.append(Row::new(&format!("u:{seq}"), seq, turn_id, RowContent::User(message)));
            }

            EventPayload::ThreadRewound(rewind) => {
                // Mike's edit closes the card the cut turns left, as his reply would.
                self.close_open_asks();
                self.append(Row::new(&format!("rw:{seq}"), seq, turn_id, RowContent::Note(rewind.summary())));
            }

            EventPayload::AssistantText { item_id, text, summary: true, gives_way, .. } => {
                // The turn's summary is its reply: what the agent wrote
                // before it folds into the work, so the turn reads as one
                // line of work, the bullets, then the card.
                let id = format!("a:{item_id}");
                if self.item_index.contains_key(&id) || text.is_empty() {
                    return;
                }
                if let Some(turn) = &turn_id {
                    if !self.summarized_turns.contains(turn) {
                        self.summarized_turns.insert(turn.clone());
                        if let Some(first) = self.items.iter().position(|i| i.turn_id.as_ref() == Some(turn)) {
                            self.mark_inserted(first);
                        }
                    }
                }
                let item = TextItem { item_id: item_id.clone(), text: text.clone(), is_summary: true, gives_way: *gives_way, ..TextItem::default() };
                self.append(Row::new(&id, seq, turn_id, RowContent::Assistant(item)));
            }

            EventPayload::AssistantText { item_id, text, narration, final_reply, .. } => {
                self.streaming.remove(item_id);
                if *final_reply {
                    if let Some(turn) = &turn_id {
                        self.replied_turns.insert(turn.clone());
                    }
                }
                let id = format!("a:{item_id}");
                let thought = format!("r:{item_id}");
                let make = |item_id: &str, text: &str| TextItem { item_id: item_id.to_owned(), text: text.to_owned(), is_narration: *narration, is_final: *final_reply, ..TextItem::default() };
                let live_thought = match self.item_index.get(&thought) {
                    Some(&i) if !self.item_index.contains_key(&id) && matches!(&self.items[i].content, RowContent::Reasoning(live) if live.is_streaming) => Some(i),
                    _ => None,
                };
                if let Some(i) = live_thought {
                    // A reply the CLI sent as a thinking block signed as
                    // narration (runner: isNarration in normalize.ts) streams
                    // as reasoning and lands as text: it takes that row's place.
                    let mut row = Row::new(&id, self.items[i].seq, self.items[i].turn_id.clone(), RowContent::Assistant(make(item_id, text)));
                    row.version = next_row_version();
                    self.items[i] = row;
                    self.item_index.remove(&thought);
                    self.item_index.insert(id, i);
                    self.mark_inserted(i);
                } else if self.item_index.contains_key(&id) {
                    let item = make(item_id, text);
                    self.update(&id, move |content| *content = RowContent::Assistant(item));
                } else if !text.is_empty() {
                    let item = make(item_id, text);
                    self.append(Row::new(&id, seq, turn_id, RowContent::Assistant(item)));
                }
            }

            EventPayload::Reasoning { item_id, text } => {
                self.streaming.remove(item_id);
                let id = format!("r:{item_id}");
                let item = TextItem::new(item_id, text);
                if self.item_index.contains_key(&id) {
                    self.update(&id, move |content| *content = RowContent::Reasoning(item));
                } else if !super::text::trim_ws_nl(text).is_empty() {
                    self.append(Row::new(&id, seq, turn_id, RowContent::Reasoning(item)));
                }
            }

            EventPayload::AssistantTextDelta { item_id, text } => {
                let id = format!("a:{item_id}");
                if self.item_index.contains_key(&id) {
                    let delta = text.clone();
                    self.update(&id, move |content| {
                        if let RowContent::Assistant(item) = content {
                            item.text.push_str(&delta);
                            item.is_streaming = true;
                        }
                    });
                } else {
                    let item = TextItem { is_streaming: true, ..TextItem::new(item_id, text) };
                    self.append(Row::new(&id, seq, turn_id, RowContent::Assistant(item)));
                }
                if let Some(RowContent::Assistant(item)) = self.item(&id).map(|r| &r.content) {
                    let so_far = item.text.clone();
                    self.streaming.insert(item_id.clone(), (StreamKind::Assistant, so_far));
                }
            }

            EventPayload::ReasoningDelta { item_id, text } => {
                let id = format!("r:{item_id}");
                if self.item_index.contains_key(&id) {
                    let delta = text.clone();
                    self.update(&id, move |content| {
                        if let RowContent::Reasoning(item) = content {
                            item.text.push_str(&delta);
                            item.is_streaming = true;
                        }
                    });
                } else {
                    let item = TextItem { is_streaming: true, ..TextItem::new(item_id, text) };
                    self.append(Row::new(&id, seq, turn_id, RowContent::Reasoning(item)));
                }
                if let Some(RowContent::Reasoning(item)) = self.item(&id).map(|r| &r.content) {
                    let so_far = item.text.clone();
                    self.streaming.insert(item_id.clone(), (StreamKind::Reasoning, so_far));
                }
            }

            EventPayload::StreamingText { item_id, is_reasoning, text } => {
                let kind = if *is_reasoning { StreamKind::Reasoning } else { StreamKind::Assistant };
                let id = format!("{}{item_id}", if *is_reasoning { "r:" } else { "a:" });
                let streaming_item = TextItem { is_streaming: true, ..TextItem::new(item_id, text) };
                let content = if *is_reasoning { RowContent::Reasoning(streaming_item) } else { RowContent::Assistant(streaming_item) };
                if let Some(&i) = self.item_index.get(&id) {
                    // A row with persisted text is final; leave it alone.
                    let still_streaming = matches!(&self.items[i].content, RowContent::Assistant(item) | RowContent::Reasoning(item) if item.is_streaming);
                    if still_streaming {
                        self.update(&id, move |c| *c = content);
                        self.streaming.insert(item_id.clone(), (kind, text.clone()));
                    }
                } else if !text.is_empty() {
                    self.append(Row::new(&id, seq, turn_id, content));
                    self.streaming.insert(item_id.clone(), (kind, text.clone()));
                }
            }

            EventPayload::ToolCall(call) => {
                let id = format!("t:{}", call.item_id);
                if self.item_index.contains_key(&id) {
                    // The result arrived first (it was on a newer page); fill in the call.
                    let call = call.clone();
                    self.update(&id, move |content| {
                        if let RowContent::Tool(tool) = content {
                            tool.call = Some(call);
                        }
                    });
                } else {
                    let tool = ToolItem { item_id: call.item_id.clone(), call: Some(call.clone()), root: self.root.clone(), ..ToolItem::default() };
                    self.append(Row::new(&id, seq, turn_id.clone(), RowContent::Tool(tool)));
                    self.note_tool_row(&id);
                }
                if DiffBuilder::is_edit_tool(call) {
                    self.note_edit(&id, turn_id.as_deref());
                }
            }

            EventPayload::ToolResult(result) => {
                let id = format!("t:{}", result.item_id);
                self.result_seqs.insert(id.clone(), seq);
                if self.item_index.contains_key(&id) {
                    let update = result.clone();
                    self.update(&id, move |content| {
                        if let RowContent::Tool(tool) = content {
                            tool.result = Some(update);
                            tool.live_output = String::new();
                        }
                    });
                } else {
                    let tool = ToolItem { item_id: result.item_id.clone(), result: Some(result.clone()), root: self.root.clone(), ..ToolItem::default() };
                    self.append(Row::new(&id, seq, turn_id.clone(), RowContent::Tool(tool)));
                    self.note_tool_row(&id);
                }
                if !result.images.is_empty() {
                    let images_id = format!("i:{}", result.item_id);
                    let generated = matches!(self.item(&id).map(|r| &r.content), Some(RowContent::Tool(tool)) if tool.tool_kind() == ToolKind::Image);
                    let content = RowContent::Images { item_id: result.item_id.clone(), images: result.images.clone(), generated };
                    if self.item_index.contains_key(&images_id) {
                        self.update(&images_id, move |c| *c = content);
                    } else {
                        self.insert(Row::new(&images_id, seq, turn_id, content), &id);
                    }
                }
            }

            EventPayload::ToolOutputDelta { item_id, text } => {
                let delta = text.clone();
                self.update(&format!("t:{item_id}"), move |content| {
                    if let RowContent::Tool(tool) = content {
                        if tool.result.is_none() {
                            tool.live_output.push_str(&delta);
                            // Keep only the tail; it's a live peek, not the record.
                            if tool.live_output.len() > 32_768 {
                                let count = tool.live_output.chars().count();
                                tool.live_output = tool.live_output.chars().skip(count - 16_384).collect();
                            }
                        }
                    }
                });
            }

            EventPayload::EditDiff(diff) => {
                // Nothing to attach a diff with no item to, and nothing to
                // show for one with no hunks.
                if diff.item_id.is_empty() || diff.hunks.is_empty() {
                    return;
                }
                let id = format!("t:{}", diff.item_id);
                if self.item_index.contains_key(&id) {
                    let update = diff.clone();
                    self.update(&id, move |content| {
                        if let RowContent::Tool(tool) = content {
                            tool.add(update);
                        }
                    });
                } else {
                    let tool = ToolItem { item_id: diff.item_id.clone(), diffs: vec![diff.clone()], root: self.root.clone(), ..ToolItem::default() };
                    self.append(Row::new(&id, seq, turn_id.clone(), RowContent::Tool(tool)));
                    self.note_tool_row(&id);
                }
                self.note_edit(&id, turn_id.as_deref());
            }

            EventPayload::ApprovalRequested(request)
                if request.name == "ExitPlanMode" && self.item_index.contains_key(&format!("pl:{}", request.item_id.as_deref().unwrap_or(""))) =>
            {
                // Claude's plan waiting on its ExitPlanMode call: the plan
                // card is its card. Still pending, so Return in an empty
                // composer approves it and the turn reads as waiting on Mike.
                self.pending_cards.insert(format!("ap:{}", request.approval_id), turn_id.unwrap_or_default());
                self.plan_approvals.insert(request.approval_id.clone());
                self.tail_dirty = true;
            }

            EventPayload::ApprovalRequested(request) => {
                let id = format!("ap:{}", request.approval_id);
                // A subagent's tools have no item id, and no row of their own.
                let tool_id = request.item_id.as_deref().filter(|i| !i.is_empty()).map(|i| format!("t:{i}"));
                if let Some(tool_id) = &tool_id {
                    self.approval_for_tool.insert(tool_id.clone(), id.clone());
                }
                let shown = tool_id.as_ref().is_some_and(|t| self.item_index.contains_key(t));
                let start_request = self.start_requests.get(&request.approval_id).cloned();
                // One card per approval id. The runner never asks twice under
                // one id, but another runner can: a harmless card, then a
                // start request under the same id, left two cards up, and
                // Allow on the harmless one answered the start request (by
                // id) with the fingerprint of what the other card read
                // (Codex's review, 8 Nov). The newer request takes the card's
                // place instead. A card already answered stays as it was, and
                // so does a start request: it shows what this Mac's runner
                // read, so nothing the asking runner sends under its id takes
                // its place (Codex, round two).
                if let Some(&index) = self.item_index.get(&id) {
                    let settled = match &self.items[index].content {
                        RowContent::Approval(old) => old.resolution.is_some() || old.is_start_request(),
                        _ => true,
                    };
                    if !settled {
                        let replacement = ApprovalItem::new(request.clone(), self.root.clone(), shown, start_request);
                        self.update(&id, move |content| *content = RowContent::Approval(replacement));
                        self.tail_dirty = true;
                    }
                    return;
                }
                let item = ApprovalItem::new(request.clone(), self.root.clone(), shown, start_request);
                self.append(Row::new(&id, seq, turn_id.clone(), RowContent::Approval(item)));
                self.pending_cards.insert(id, turn_id.unwrap_or_default());
                self.tail_dirty = true;
            }

            EventPayload::ApprovalQueued(queue) => {
                let id = format!("ap:{}", queue.approval_id);
                let calls = queue.calls.clone();
                self.update(&id, move |content| {
                    // Never on another runner's start request (ApprovalItem::queued).
                    if let RowContent::Approval(item) = content {
                        if item.resolution.is_none() && !item.is_start_request() {
                            item.set_queued(calls);
                        }
                    }
                });
            }

            EventPayload::QuestionRequested(request) => {
                let id = format!("q:{}", request.approval_id);
                let item = QuestionItem { request: request.clone(), resolution: None };
                self.append(Row::new(&id, seq, turn_id.clone(), RowContent::Question(item)));
                self.pending_cards.insert(id, turn_id.unwrap_or_default());
                self.tail_dirty = true;
            }

            EventPayload::SecretRequested(request) => {
                // `s:` keeps it out of pending_approval_ids: Return in an
                // empty composer must never answer a secret card.
                let id = format!("s:{}", request.approval_id);
                let item = SecretItem { request: request.clone(), resolution: None };
                self.append(Row::new(&id, seq, turn_id.clone(), RowContent::Secret(item)));
                self.pending_cards.insert(id, turn_id.unwrap_or_default());
                self.tail_dirty = true;
            }

            EventPayload::ReviewUpdated(review) => {
                let id = format!("review:{}", review.id);
                if let Some(previous_revision) = self.item(&id).and_then(|row| match &row.content {
                    RowContent::Review(previous) => Some(previous.revision),
                    _ => None,
                }) {
                    // A stale snapshot must not turn an answered review back
                    // into a pending request after reconnecting.
                    if review.revision > previous_revision {
                        let update = review.clone();
                        self.update(&id, move |content| *content = RowContent::Review(update));
                    }
                } else {
                    self.append(Row::new(&id, seq, turn_id, RowContent::Review(review.clone())));
                }
            }

            EventPayload::ApprovalResolved(resolution) => {
                let approval_id = &resolution.approval_id;
                if self.plan_approvals.remove(approval_id) {
                    self.pending_cards.remove(&format!("ap:{approval_id}"));
                    self.tail_dirty = true;
                }
                for prefix in ["ap:", "q:", "s:"] {
                    let id = format!("{prefix}{approval_id}");
                    if !self.item_index.contains_key(&id) {
                        continue;
                    }
                    self.pending_cards.remove(&id);
                    self.tail_dirty = true;
                    let resolution = resolution.clone();
                    self.update(&id, move |content| match content {
                        RowContent::Approval(item) => item.resolution = Some(resolution),
                        RowContent::Question(item) => item.resolution = Some(resolution),
                        RowContent::Secret(item) => item.resolution = Some(resolution),
                        _ => {}
                    });
                }
            }

            EventPayload::PlanProposed { item_id, text } => {
                let id = format!("pl:{item_id}");
                self.plan_taking_steps = None;
                if self.item_index.contains_key(&id) {
                    let card = PlanCard::new(item_id, text);
                    self.update(&id, move |content| *content = RowContent::PlanCard(card));
                } else {
                    self.append(Row::new(&id, seq, turn_id, RowContent::PlanCard(PlanCard::new(item_id, text))));
                }
            }

            EventPayload::PlanResolved { item_id, go } => {
                let id = format!("pl:{item_id}");
                let go = *go;
                self.update(&id, move |content| {
                    if let RowContent::PlanCard(card) = content {
                        card.state = if go { PlanCardState::Approved } else { PlanCardState::SentBack };
                    }
                });
                if go && self.item_index.contains_key(&id) {
                    self.plan_taking_steps = Some(id);
                    self.plan_steps_turn = turn_id;
                }
            }

            EventPayload::PlanUpdated(entries) => {
                let key = turn_id.clone().unwrap_or_else(|| format!("seq{seq}"));
                let taking = self.plan_taking_steps.clone().filter(|id| self.item_index.contains_key(id));
                let existing = self.plan_row_for_turn.get(&key).cloned().filter(|id| self.item_index.contains_key(id));
                if let Some(id) = taking {
                    // The approved plan's own steps: its card becomes the checklist.
                    self.plan_row_for_turn.insert(key, id.clone());
                    let entries = entries.clone();
                    self.update(&id, move |content| {
                        if let RowContent::PlanCard(card) = content {
                            card.entries = entries;
                        }
                    });
                } else if let Some(id) = existing {
                    let entries = entries.clone();
                    self.update(&id, move |content| *content = RowContent::Plan(entries));
                } else {
                    let id = format!("p:{seq}");
                    self.plan_row_for_turn.insert(key, id.clone());
                    self.append(Row::new(&id, seq, turn_id, RowContent::Plan(entries.clone())));
                }
            }

            EventPayload::TurnStarted { woke_by } => {
                let key = turn_id.clone().unwrap_or_default();
                if self.plan_taking_steps.is_some() && self.plan_steps_turn.is_none() {
                    self.plan_steps_turn = Some(key.clone());
                }
                self.open_turn = Some(key.clone());
                self.started_turns.insert(key.clone());
                if let Some(date) = parse_iso8601(&event.ts) {
                    self.turn_starts.insert(key.clone(), date);
                }
                // A turn nobody's message started: the agent picked up again
                // as background work finished. Mark how long it was away.
                let gap_id = format!("gap:{key}");
                if !self.messaged_turns.contains(&key) && !self.item_index.contains_key(&gap_id) {
                    if let (Some(from), Some(to)) = (self.last_turn_end, self.turn_starts.get(&key).copied()) {
                        let gap = TurnGap { from, to, woke_by: woke_by.clone() };
                        self.append(Row::new(&gap_id, seq, turn_id, RowContent::Gap(gap)));
                    }
                }
                // Nothing new to show but the working line (and a live group).
                self.mark_inserted(self.items.len());
            }

            EventPayload::TurnCompleted(completion) => {
                // A turn that ends leaves nothing streaming.
                self.finish_streaming();
                let key = if completion.turn_id.is_empty() {
                    turn_id.clone().unwrap_or_else(|| format!("seq{seq}"))
                } else {
                    completion.turn_id.clone()
                };
                self.completed_turns.insert(key.clone());
                if self.plan_taking_steps.is_some() && self.plan_steps_turn.as_deref() == Some(key.as_str()) {
                    self.plan_taking_steps = None;
                    self.plan_steps_turn = None;
                }
                if let Some(date) = parse_iso8601(&event.ts) {
                    self.last_turn_end = Some(date);
                }
                if let Some(turn) = &turn_id {
                    self.completed_turns.insert(turn.clone());
                }
                if self.open_turn.as_deref().is_some_and(|open| open == key || Some(open) == turn_id.as_deref() || completion.turn_id.is_empty()) {
                    self.open_turn = None;
                }
                // The error row just before says why, once the runner knows.
                if let Some(kind) = completion.error_kind {
                    let error_id = self.error_row_for_turn.get(&key).cloned().or_else(|| turn_id.as_ref().and_then(|t| self.error_row_for_turn.get(t).cloned()));
                    if let Some(error_id) = error_id {
                        self.update(&error_id, move |content| {
                            if let RowContent::Error { message, .. } = content {
                                *content = RowContent::Error { message: std::mem::take(message), kind: Some(kind) };
                            }
                        });
                    }
                }
                let id = format!("f:{key}");
                if self.item_index.contains_key(&id) {
                    let footer = completion.clone();
                    self.update(&id, move |content| *content = RowContent::Footer(footer));
                } else {
                    let row_turn = turn_id.clone().or_else(|| Some(completion.turn_id.clone()));
                    self.append(Row::new(&id, seq, row_turn.clone(), RowContent::Footer(completion.clone())));
                    // Any finished turn closes the last card; one that needs
                    // Mike opens the next, under its footer.
                    self.close_open_asks();
                    // The card sits under the turn's footer, its answers
                    // picked there and sent from the composer (Mike, 28 Oct).
                    if completion.outcome == TurnOutcome::Completed && completion.needs.as_deref() == Some("you") && !completion.asks.is_empty() {
                        let card_id = format!("ak:{seq}");
                        let mut card = AsksCard::new(seq, completion.asks.clone());
                        card.ticked = completion.asks.iter().enumerate().filter(|(_, ask)| ask.ticked).map(|(i, _)| i as i64).collect();
                        card.picks = self.ask_picks.get(&seq).cloned().unwrap_or_default();
                        card.notes = self.ask_notes.get(&seq).cloned().unwrap_or_default();
                        self.append(Row::new(&card_id, seq, row_turn, RowContent::Asks(card)));
                        self.asks_row_for_seq.insert(seq, card_id.clone());
                        self.open_asks_row = Some(card_id);
                    } else if completion.outcome == TurnOutcome::Completed && completion.needs.as_deref() == Some("nothing") {
                        // The done card: goes, as an asks card does, once
                        // Mike replies or the next turn ends.
                        let card_id = format!("ak:{seq}");
                        let mut card = AsksCard::new(seq, Vec::new());
                        card.next = completion.next.clone();
                        card.picked_next = self.next_picks.get(&seq).cloned().unwrap_or_default();
                        card.explain_next = self.next_explains.get(&seq).cloned().unwrap_or_default();
                        card.loose_ends = self.loose_ends.as_ref().filter(|l| l.seq == seq).map(|l| l.ends.clone());
                        self.append(Row::new(&card_id, seq, row_turn, RowContent::Asks(card)));
                        self.asks_row_for_seq.insert(seq, card_id.clone());
                        self.open_asks_row = Some(card_id);
                    }
                }
            }

            EventPayload::AsksTicked { asks_seq, ticked } => {
                if let Some(card_id) = self.asks_row_for_seq.get(asks_seq).cloned() {
                    let ticked: BTreeSet<i64> = ticked.iter().copied().collect();
                    self.update(&card_id, move |content| {
                        if let RowContent::Asks(card) = content {
                            card.ticked = ticked;
                        }
                    });
                }
            }

            EventPayload::SystemNote(text) => {
                self.append(Row::new(&format!("n:{seq}"), seq, turn_id, RowContent::Note(text.clone())));
            }

            EventPayload::Error(message) => {
                let id = format!("e:{seq}");
                self.append(Row::new(&id, seq, turn_id.clone(), RowContent::Error { message: message.clone(), kind: None }));
                if let Some(turn) = turn_id {
                    self.error_row_for_turn.insert(turn, id);
                }
            }

            EventPayload::ActionRun(run) => {
                // A finished command, drawn like a tool call with its output.
                let item_id = ActionRun::item_id(seq);
                let (mut call, mut result) = run.as_tool_call();
                call.item_id = item_id.clone();
                result.item_id = item_id.clone();
                let row_id = format!("t:{item_id}");
                if run.trigger.is_none() {
                    self.pressed_runs.insert(row_id.clone());
                }
                let tool = ToolItem { item_id, call: Some(call), result: Some(result), root: self.root.clone(), ..ToolItem::default() };
                self.append(Row::new(&row_id, seq, turn_id, RowContent::Tool(tool)));
            }

            // No row: the reply that links it draws it (ReplyButtons).
            EventPayload::ButtonUpdated(_) | EventPayload::Unknown => {}
        }
    }

    fn finish_streaming(&mut self) {
        if self.streaming.is_empty() {
            return;
        }
        let streaming = std::mem::take(&mut self.streaming);
        for (item_id, (kind, _)) in streaming {
            let id = format!("{}{item_id}", if kind == StreamKind::Reasoning { "r:" } else { "a:" });
            self.update(&id, |content| match content {
                RowContent::Assistant(item) | RowContent::Reasoning(item) => item.is_streaming = false,
                _ => {}
            });
        }
    }

    // ---- Rows ----

    /// Work that folds into groups: thinking, tool calls, system notes and
    /// approvals Mike allowed. Everything else (messages, plans, images,
    /// errors, footers, and cards still waiting or turned down) ends a run
    /// and is never hidden in one.
    pub(crate) fn is_work_row(row: &Row) -> bool {
        match &row.content {
            RowContent::Tool(tool) => !Self::is_taken_end_turn(tool),
            RowContent::Note(_) if row.content.is_wake_note() => false,
            RowContent::Reasoning(_) | RowContent::Note(_) => true,
            RowContent::Approval(item) => item.is_allowed(),
            _ => false,
        }
    }

    /// An end_turn call the runner took (or hasn't answered yet). It isn't
    /// shown: the card under the turn's footer says what it said. A refused
    /// one stays, as work, since the turn carried on because of it.
    pub fn is_taken_end_turn(tool: &ToolItem) -> bool {
        let Some(name) = tool.call.as_ref().map(|c| c.name.as_str()) else { return false };
        if !name.starts_with("mcp__threadbear") || !name.ends_with("__end_turn") {
            return false;
        }
        tool.result.as_ref().map(|r| r.ok).unwrap_or(true)
    }

    /// `is_work_row`, plus a summarized turn's own messages, which fold into
    /// its work under the summary.
    fn is_work(&self, row: &Row) -> bool {
        if let RowContent::Assistant(text) = &row.content {
            if !text.is_summary {
                if let Some(turn) = &row.turn_id {
                    if self.summarized_turns.contains(turn) {
                        return true;
                    }
                }
            }
        }
        Self::is_work_row(row)
    }

    /// The items of the finished stretch (`done_block`) or, while a later
    /// turn in its stretch still runs, the finished turn item `i` is in (a
    /// turn's items are together). A finished turn shows as Mike's messages,
    /// one line of work, its reply and its cards, like Codex desktop.
    fn done_turn(&self, i: isize) -> Option<Range<usize>> {
        if let Some(block) = self.done_block(i) {
            return Some(block);
        }
        if i < 0 || i as usize >= self.items.len() {
            return None;
        }
        let i = i as usize;
        let turn = self.items[i].turn_id.clone()?;
        if !self.completed_turns.contains(&turn) {
            return None;
        }
        let mut low = i;
        while low > 0 && self.items[low - 1].turn_id.as_ref() == Some(&turn) {
            low -= 1;
        }
        let mut high = i + 1;
        while high < self.items.len() && self.items[high].turn_id.as_ref() == Some(&turn) {
            high += 1;
        }
        Some(low..high)
    }

    /// A message from Mike (or a schedule, or another thread's agent) that
    /// started a turn of its own. The runner's messages carry on from the
    /// turn before, and one Mike sent into a running turn went into that
    /// turn.
    fn starts_block(&self, k: usize) -> bool {
        match &self.items[k].content {
            RowContent::User(message) if !message.is_from_runner() => {}
            _ => return false,
        }
        let Some(turn) = &self.items[k].turn_id else { return true };
        let mut j = k as isize - 1;
        while j >= 0 && self.items[j as usize].turn_id.is_none() {
            j -= 1;
        }
        j < 0 || self.items[j as usize].turn_id.as_ref() != Some(turn)
    }

    /// The first item of the stretch item `i` is in: the message that
    /// started it, or the first item held.
    fn block_start(&self, i: usize) -> usize {
        if self.items.is_empty() {
            return 0;
        }
        let mut low = i.min(self.items.len() - 1);
        while low > 0 && !self.starts_block(low) {
            low -= 1;
        }
        low
    }

    /// The finished part of item `i`'s stretch: from the message that
    /// started it up to the next one, or to the first item of a turn still
    /// going. Mike's message, the turn it started, the runner's turns after
    /// it and the notes between show as one line of work and one reply.
    fn done_block(&self, i: isize) -> Option<Range<usize>> {
        if i < 0 || i as usize >= self.items.len() {
            return None;
        }
        let i = i as usize;
        // An item of a running turn is never in a finished stretch, and this
        // is most of what's asked while a turn streams.
        if let Some(turn) = &self.items[i].turn_id {
            if !self.is_finished(turn) {
                return None;
            }
        }
        let low = self.block_start(i);
        let mut high = low;
        while high < self.items.len() && (high == low || !self.starts_block(high)) {
            if let Some(turn) = &self.items[high].turn_id {
                if !self.is_finished(turn) {
                    break;
                }
            }
            high += 1;
        }
        if i < high && self.items[low..high].iter().any(|item| item.turn_id.is_some()) {
            Some(low..high)
        } else {
            None
        }
    }

    fn is_finished(&self, turn: &str) -> bool {
        self.completed_turns.contains(turn) && self.open_turn.as_deref() != Some(turn)
    }

    /// A finished turn's reply (`reply_of`). In a stretch with the runner's
    /// turns in it, each turn from the one with the last summary on (each
    /// turn, without one) has a reply, so a turn that ended on a line rather
    /// than a summary isn't folded away behind the one before. Only a turn
    /// with a message of its own can take over like that.
    fn replies(&self, turn: Range<usize>) -> HashSet<usize> {
        // Items with no turn id (live text not yet given one) are a turn of
        // their own, "".
        let key = |k: usize| self.items[k].turn_id.clone().unwrap_or_default();
        let mut turns: Vec<String> = Vec::new();
        for k in turn.clone() {
            let id = key(k);
            if !turns.contains(&id) {
                turns.push(id);
            }
        }
        let messaged: HashSet<String> = turn.clone().filter(|&k| matches!(self.items[k].content, RowContent::User(_))).map(key).collect();
        let summary = turn.clone().rev().find(|&k| self.text_item(k).is_some_and(|t| t.is_summary) && messaged.contains(&key(k)));
        let from = summary.and_then(|k| turns.iter().position(|id| *id == key(k))).unwrap_or(0);
        let mut out = HashSet::new();
        for id in &turns[from..] {
            let items: Vec<usize> = turn.clone().filter(|&k| key(k) == *id).collect();
            out.extend(self.reply_of(&items));
        }
        out
    }

    /// One turn's reply: the messages it wrote after its last real step and
    /// before its end_turn, when they say more than its summary; else its
    /// summary; else its last message. A narration block is only a
    /// paraphrase of what the agent wrote, so it never beats the bullets.
    /// (`reply(of:)` in the Swift has the full history of each rule here.)
    fn reply_of(&self, turn: &[usize]) -> HashSet<usize> {
        let text = |k: usize| self.text_item(k);
        // A message sent into the turn (the runner's "the app restarted")
        // starts a fresh answer, so only an end_turn after it ends this one.
        let fresh = turn.iter().rposition(|&k| matches!(self.items[k].content, RowContent::User(_))).map_or(0, |p| p + 1);
        // The last end_turn that gave a card, not one that only added archive
        // or snooze ("Added: ...").
        let is_taken = |k: usize| matches!(&self.items[k].content, RowContent::Tool(tool) if Self::is_taken_end_turn(tool));
        let end = turn[fresh..]
            .iter()
            .rposition(|&k| match &self.items[k].content {
                RowContent::Tool(tool) => Self::is_taken_end_turn(tool) && !tool.result.as_ref().is_some_and(|r| r.text.starts_with("Added:")),
                _ => false,
            })
            .map(|p| p + fresh)
            .or_else(|| turn[fresh..].iter().position(|&k| is_taken(k)).map(|p| p + fresh))
            .unwrap_or(turn.len());
        // One reply: the agent's message after end_turn is the reply, and
        // nothing else the turn wrote competes with it. One between the
        // fresh answer's start and its last card belongs to a card that was
        // replaced after a wake.
        let finals: Vec<usize> = (0..turn.len()).filter(|&p| text(turn[p]).is_some_and(|t| t.is_final) && !(fresh..end).contains(&p)).map(|p| turn[p]).collect();
        if !finals.is_empty() {
            return finals.into_iter().collect();
        }
        let before_end = &turn[..end];
        // After the last step's row and after its result, as a message can
        // land between a call and its result. A message sent into the
        // running turn starts a new answer too; it counts by place, as live
        // text carries the seq of the event before it.
        let steps: Vec<usize> = before_end.iter().copied().filter(|&k| matches!(self.items[k].content, RowContent::User(_)) || Self::is_real_step(&self.items[k])).collect();
        let last_result = steps.iter().filter_map(|&k| self.result_seqs.get(&self.items[k].id).copied()).max().unwrap_or(i64::MIN);
        let last_step = steps.last().copied();
        let final_words: Vec<usize> = before_end
            .iter()
            .copied()
            .filter(|&k| text(k).is_some_and(|t| !t.is_summary) && last_step.map_or(true, |last| k > last) && self.items[k].seq > last_result)
            .collect();
        // The last card's own summary: one a wake replaced says what was true
        // before it (from Codex's review).
        let summary_scope = if end < turn.len() { &turn[end..] } else { turn };
        if let Some(&summary) = summary_scope.iter().rev().find(|&&k| text(k).is_some_and(|t| t.is_summary)) {
            // A narration block is only a paraphrase, so it never beats the bullets.
            let mut words: Vec<usize> = final_words.iter().copied().filter(|&k| text(k).is_some_and(|t| !t.is_narration)).collect();
            if words.is_empty() {
                let tail = if end < turn.len() { &turn[end..] } else { &turn[turn.len()..] };
                words = tail.iter().copied().filter(|&k| text(k).is_some_and(|t| !t.is_summary && !t.is_narration)).collect();
            }
            let said: usize = words.iter().map(|&k| text(k).map_or(0, |t| super::text::count(&t.text))).sum();
            // With bullet replies off, a sign-off line wins too.
            let bar = if text(summary).is_some_and(|t| t.gives_way) { 0 } else { text(summary).map_or(0, |t| super::text::count(&t.text)) };
            return if said > bar { words.into_iter().collect() } else { HashSet::from([summary]) };
        }
        // Words win over a narration paraphrase of them.
        let words: Vec<usize> = final_words.iter().copied().filter(|&k| text(k).is_some_and(|t| !t.is_narration)).collect();
        if !words.is_empty() {
            return words.into_iter().collect();
        }
        if !final_words.is_empty() {
            return final_words.into_iter().collect();
        }
        match turn.iter().rev().find(|&&k| text(k).is_some()) {
            Some(&k) => HashSet::from([k]),
            None => HashSet::new(),
        }
    }

    fn text_item(&self, k: usize) -> Option<&TextItem> {
        match &self.items[k].content {
            RowContent::Assistant(text) => Some(text),
            _ => None,
        }
    }

    /// A step that did something: a tool call outside Threadbear's own tools
    /// (pin, update_project, end_turn) and ToolSearch, which only loads a
    /// tool. A message after a pin is still the agent's last word.
    fn is_real_step(row: &Row) -> bool {
        let RowContent::Tool(tool) = &row.content else { return false };
        let Some(name) = tool.call.as_ref().map(|c| c.name.as_str()) else { return true };
        !name.starts_with("mcp__threadbear") && name != "ToolSearch"
    }

    /// A background subagent's card still waiting (no tool of the agent's to
    /// point at). It's drawn at the end of the transcript rather than where
    /// it came in: it stays open across turns, so that spot can be folded
    /// into a finished turn or scrolled far above the latest reply. Once
    /// answered it goes back to its place.
    fn is_floating_card(&self, k: usize) -> bool {
        let item = &self.items[k];
        if !self.pending_cards.contains_key(&item.id) {
            return false;
        }
        if let Some(turn) = &item.turn_id {
            if Some(turn.as_str()) == self.open_turn.as_deref() {
                return false;
            }
        }
        match &item.content {
            RowContent::Approval(approval) => approval.request.item_id.as_deref().unwrap_or("").is_empty(),
            RowContent::Question(question) => question.request.item_id.as_deref().unwrap_or("").is_empty(),
            _ => false,
        }
    }

    /// What stays out of a finished turn's line of work: Mike's messages,
    /// the reply, errors, reviews waiting on him, pictures the agent made,
    /// the footer and the card. The runner's messages fold in: they're how
    /// the work carried on. The note an edit of Mike's leaves stays.
    fn stays_out(&self, k: usize, replies: &HashSet<usize>) -> bool {
        let item = &self.items[k];
        if replies.contains(&k) || item.id.starts_with("rw:") || item.content.is_wake_note() {
            return true;
        }
        match &item.content {
            RowContent::User(message) => !message.is_from_runner(),
            RowContent::Error { .. } | RowContent::Footer(_) | RowContent::Asks(_) | RowContent::Review(_) | RowContent::Gap(_) | RowContent::PlanCard(_) => true,
            RowContent::Images { generated, .. } => *generated,
            RowContent::Tool(tool) => Self::is_taken_end_turn(tool) || self.pressed_runs.contains(&item.id),
            _ => false,
        }
    }

    /// The waits in a finished stretch that get a divider, by item: only one
    /// with something of the agent's shown above and below it. Waits with
    /// nothing shown between them are one divider, from the first one's
    /// start to the last one's end.
    fn dividers(&self, turn: Range<usize>, replies: &HashSet<usize>) -> HashMap<usize, TurnGap> {
        let mut out = HashMap::new();
        let mut shown_before = false;
        let mut waiting: Option<(usize, TurnGap)> = None;
        for k in turn {
            if let RowContent::Gap(gap) = &self.items[k].content {
                if !shown_before {
                    continue;
                }
                waiting = Some(match waiting.take() {
                    Some((_, earlier)) => (k, TurnGap { from: earlier.from, to: gap.to, woke_by: [earlier.woke_by, gap.woke_by.clone()].concat() }),
                    None => (k, gap.clone()),
                });
                continue;
            }
            if !self.shows_in_stretch(k, replies) {
                continue;
            }
            if let Some((at, gap)) = waiting.take() {
                out.insert(at, gap);
            }
            shown_before = true;
        }
        out
    }

    /// Something of the agent's that a finished stretch shows on its own
    /// (not Mike's message, the footer or a card that's gone).
    fn shows_in_stretch(&self, k: usize, replies: &HashSet<usize>) -> bool {
        if !self.stays_out(k, replies) {
            return false;
        }
        match &self.items[k].content {
            RowContent::User(_) | RowContent::Footer(_) | RowContent::Gap(_) => false,
            RowContent::Asks(card) => card.is_open,
            RowContent::Tool(tool) => !Self::is_taken_end_turn(tool),
            _ => true,
        }
    }

    /// The first item of the run of work that item `i` is in (or `i`); a
    /// finished turn is one run. `i` may be -1 or `items.len()`.
    fn run_start(&self, i: isize) -> usize {
        if let Some(turn) = self.done_turn(i) {
            return turn.start;
        }
        if i < 0 || i as usize >= self.items.len() || !self.is_work(&self.items[i as usize]) {
            return i.max(0) as usize;
        }
        let i = i as usize;
        let turn = &self.items[i].turn_id;
        let mut j = i;
        while j > 0 && self.is_work(&self.items[j - 1]) && self.items[j - 1].turn_id == *turn {
            j -= 1;
        }
        j
    }

    /// One past the last item of the run that item `i` is in (or `i + 1`).
    fn run_end(&self, i: isize) -> usize {
        if let Some(turn) = self.done_turn(i) {
            return turn.end;
        }
        if i < 0 || i as usize >= self.items.len() || !self.is_work(&self.items[i as usize]) {
            return ((i + 1).max(0) as usize).min(self.items.len());
        }
        let i = i as usize;
        let turn = &self.items[i].turn_id;
        let mut j = i + 1;
        while j < self.items.len() && self.is_work(&self.items[j]) && self.items[j].turn_id == *turn {
            j += 1;
        }
        j
    }

    /// The step a work: link in turn `turn_id` means (`WorkLinks.step`), and
    /// the group row it's folded into, None when it's shown as itself.
    pub fn work_step(&self, quote: &str, turn_id: &str) -> Option<(String, Option<String>)> {
        let mut found = WorkLinks::step(quote, self.items.iter().filter(|item| item.turn_id.as_deref() == Some(turn_id)));
        // A runner's turn's summary covers its whole stretch, so it can
        // point at a step from the turn Mike's message started.
        if found.is_none() {
            if let Some(block) = self.items.iter().position(|item| item.turn_id.as_deref() == Some(turn_id)).and_then(|first| self.done_block(first as isize)) {
                found = WorkLinks::step(quote, self.items[block].iter());
            }
        }
        let row = found?;
        let i = *self.item_index.get(&row.id)?;
        if let Some(turn) = self.done_turn(i as isize) {
            let replies = self.replies(turn.clone());
            if self.stays_out(i, &replies) {
                return Some((row.id.clone(), None));
            }
            return Some((row.id.clone(), turn.into_iter().find(|&k| !self.stays_out(k, &replies)).map(|k| format!("g:{}", self.items[k].id))));
        }
        let start = self.run_start(i as isize);
        Some((row.id.clone(), if self.run_end(i as isize) - start > 1 { Some(format!("g:{}", self.items[start].id)) } else { None }))
    }

    /// The first row standing for item `item` or a later one.
    fn first_row(&self, item: usize) -> usize {
        let (mut low, mut high) = (0, self.row_starts.len());
        while low < high {
            let mid = (low + high) / 2;
            if self.row_starts[mid] < item {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        low
    }

    /// Brings `rows` up to date with the items changed since the last call
    /// and says what changed. New items rebuild from the first run they
    /// touch to the end (nearly always just the current turn); changes in
    /// place only rebuild the runs they're in.
    fn refresh_rows(&mut self) -> TranscriptChange {
        if self.dirty_update.is_none() && self.dirty_insert.is_none() && !self.tail_dirty {
            return TranscriptChange::default();
        }
        let mut start = self.items.len();
        let mut end = self.items.len();
        // A changed item can change its group's summary.
        if let Some(changed) = self.dirty_update {
            start = start.min(self.run_start(changed as isize));
        }
        // A new item can join the run before it, or end it (so it's no
        // longer the live group).
        if let Some(added) = self.dirty_insert {
            start = start.min(self.run_start(added as isize)).min(self.run_start(added as isize - 1));
            // A new turn or a note can join a finished stretch or end it
            // (done_block), which changes rows back to its first message.
            if added > 0 && added < self.items.len() && self.items[added].turn_id != self.items[added - 1].turn_id {
                start = start.min(self.block_start(added));
            }
        }
        let bounded = self.dirty_insert.is_none() && !self.tail_dirty;
        if bounded {
            end = self.items.len().min(self.dirty_update_end);
        }
        self.dirty_update = None;
        self.dirty_insert = None;
        self.dirty_update_end = 0;
        self.tail_dirty = false;

        let first = self.first_row(start);
        // Rows from `last` on stand for items past the change (and the
        // working line) and stay as they are.
        let last = if bounded { self.first_row(end) } else { self.rows.len() };
        let (built_rows, built_starts) = self.build_rows(start, Some(end), !bounded);

        let mut change = TranscriptChange::default();
        let mut kept: HashSet<&str> = HashSet::with_capacity(built_rows.len());
        for (offset, row) in built_rows.iter().enumerate() {
            kept.insert(&row.id);
            match self.index.get(&row.id) {
                Some(&old) if old >= first && old < last => {
                    let previous = &self.rows[old];
                    if previous.version != row.version || previous.in_group != row.in_group {
                        change.updated.insert(first + offset);
                    }
                }
                _ => {
                    change.inserted.insert(first + offset);
                }
            }
        }
        for old in first..last {
            if !kept.contains(self.rows[old].id.as_str()) {
                change.removed.insert(old);
                let id = self.rows[old].id.clone();
                self.index.remove(&id);
            }
        }
        drop(kept);
        let shifted_all = built_rows.len() != last - first;
        let built_count = built_rows.len();
        self.rows.splice(first..last, built_rows);
        self.row_starts.splice(first..last, built_starts);
        let shifted = if shifted_all { self.rows.len() } else { first + built_count };
        for position in first..shifted {
            let id = self.rows[position].id.clone();
            self.index.insert(id, position);
        }
        change
    }

    /// The rows for items from `start` (a run boundary) up to `end`, with
    /// the working line after them while a turn runs.
    fn build_rows(&self, start: usize, end: Option<usize>, working_line: bool) -> (Vec<Row>, Vec<usize>) {
        let end = end.unwrap_or(self.items.len());
        let mut out: Vec<Row> = Vec::with_capacity(end.saturating_sub(start) + 2);
        let mut starts: Vec<usize> = Vec::with_capacity(out.capacity());

        // Derived rows keep their old version while their content is the same.
        let derived = |id: &str, seq: i64, turn_id: Option<String>, content: RowContent| -> Row {
            if let Some(&old) = self.index.get(id) {
                if old < self.rows.len() && self.rows[old].content == content {
                    return self.rows[old].clone();
                }
            }
            let mut row = Row::new(id, seq, turn_id, content);
            row.version = next_row_version();
            row
        };

        let mut i = start;
        while i < end {
            let item = &self.items[i];
            if let Some(turn) = self.done_turn(i as isize).filter(|t| t.start == i) {
                // Every row of a finished turn starts at its first item, so a
                // change anywhere in it rebuilds it whole (run_start).
                let replies = self.replies(turn.clone());
                let folded: Vec<usize> = turn.clone().filter(|&k| !self.stays_out(k, &replies) && !self.is_floating_card(k)).collect();
                // The line of work goes above the reply, even when the reply
                // came first: a turn that only answered has its summary
                // folded under it (reply_of).
                let group_at = folded.first().map(|&first| replies.iter().copied().min().map_or(first, |r| first.min(r)));
                // A stretch with the runner's turns in it has a footer for
                // each: the last one stands for them all, with their time and
                // cost added up, and the changes card has every turn's edits.
                let footers: Vec<(usize, TurnCompletion)> = turn
                    .clone()
                    .filter_map(|k| match &self.items[k].content {
                        RowContent::Footer(done) => Some((k, done.clone())),
                        _ => None,
                    })
                    .collect();
                let worked: f64 = footers.iter().map(|(_, done)| done.duration_ms).sum();
                let gaps = self.dividers(turn.clone(), &replies);
                for k in turn.clone() {
                    let row = &self.items[k];
                    if self.is_floating_card(k) {
                        continue;
                    }
                    if Some(k) == group_at && !folded.is_empty() {
                        let lead = &self.items[folded[0]];
                        let id = format!("g:{}", lead.id);
                        let open = self.expanded.contains(&id);
                        let members: Vec<&Row> = folded.iter().map(|&f| &self.items[f]).collect();
                        let mut group = WorkGroup::from_members(&members, open, false);
                        if worked > 0.0 {
                            group.summary = format!("Worked for {}", duration(worked));
                        }
                        out.push(derived(&id, lead.seq, lead.turn_id.clone(), RowContent::Group(group)));
                        starts.push(turn.start);
                        if open {
                            for &m in &folded {
                                let mut member = self.items[m].clone();
                                member.in_group = true;
                                out.push(member);
                                starts.push(turn.start);
                            }
                        }
                        if replies.contains(&k) {
                            out.push(row.clone());
                            starts.push(turn.start);
                        }
                    } else if !self.stays_out(k, &replies) {
                        continue;
                    } else if matches!(&row.content, RowContent::Asks(card) if !card.is_open) {
                        continue;
                    } else if matches!(&row.content, RowContent::Tool(tool) if Self::is_taken_end_turn(tool)) {
                        continue;
                    } else if matches!(row.content, RowContent::Gap(_)) {
                        let Some(gap) = gaps.get(&k) else { continue };
                        out.push(derived(&row.id, row.seq, row.turn_id.clone(), RowContent::Gap(gap.clone())));
                        starts.push(turn.start);
                    } else if let RowContent::Footer(done) = &row.content {
                        if Some(k) != footers.last().map(|(index, _)| *index) {
                            continue;
                        }
                        let stretch: Vec<String> = footers.iter().map(|(index, _)| self.items[*index].turn_id.clone().unwrap_or_default()).collect();
                        // A turn that ended with a summary has no changes
                        // card: the summary says what changed, and the edits
                        // are still in the folded work (Mike, 30 Oct). One
                        // reply's final message counts too (6 Nov).
                        if !stretch.iter().any(|t| self.summarized_turns.contains(t) || self.replied_turns.contains(t)) {
                            if let Some(card) = self.changed_files(&stretch) {
                                out.push(derived(&format!("cf:{}", card.turn_id), row.seq, row.turn_id.clone(), RowContent::Changes(card)));
                                starts.push(turn.start);
                            }
                        }
                        if footers.len() > 1 {
                            let mut done = done.clone();
                            done.duration_ms = worked;
                            let costs: Vec<f64> = footers.iter().filter_map(|(_, d)| d.cost_usd).collect();
                            done.cost_usd = if costs.is_empty() { None } else { Some(costs.iter().sum()) };
                            let tokens: Vec<TurnTokens> = footers.iter().filter_map(|(_, d)| TurnTokens::from_usage(d.usage.as_ref())).collect();
                            done.usage = tokens.iter().copied().reduce(TurnTokens::add).map(TurnTokens::usage);
                            out.push(derived(&row.id, row.seq, row.turn_id.clone(), RowContent::Footer(done)));
                        } else {
                            out.push(row.clone());
                        }
                        starts.push(turn.start);
                    } else {
                        out.push(row.clone());
                        starts.push(turn.start);
                    }
                }
                i = turn.end;
                continue;
            }
            if self.is_floating_card(i) {
                i += 1;
                continue;
            }
            if self.is_work(item) {
                let run_end = self.run_end(i as isize);
                // A summarized turn's one message still folds away: the
                // summary is the reply.
                let folded_text = matches!(item.content, RowContent::Assistant(_));
                if run_end - i == 1 && !folded_text {
                    // A run of one is shown as itself.
                    out.push(item.clone());
                    starts.push(i);
                } else {
                    let id = format!("g:{}", item.id);
                    let open = self.expanded.contains(&id);
                    let live = run_end == self.items.len() && self.open_turn.is_some() && item.turn_id.as_deref().unwrap_or("") == self.open_turn.as_deref().unwrap_or("");
                    let members: Vec<&Row> = self.items[i..run_end].iter().collect();
                    let group = WorkGroup::from_members(&members, open, live);
                    out.push(derived(&id, item.seq, item.turn_id.clone(), RowContent::Group(group)));
                    starts.push(i);
                    if open {
                        for k in i..run_end {
                            let mut member = self.items[k].clone();
                            member.in_group = true;
                            out.push(member);
                            starts.push(k);
                        }
                    }
                }
                i = run_end;
            } else if matches!(&item.content, RowContent::Asks(card) if !card.is_open) {
                // A card that's been answered or moved past isn't shown: the
                // answers bubble says what was picked (Mike, 28 Oct).
                i += 1;
            } else if matches!(&item.content, RowContent::Tool(tool) if Self::is_taken_end_turn(tool)) {
                i += 1;
            } else {
                if matches!(item.content, RowContent::Footer(_)) {
                    if let Some(card) = self.changed_files(&[item.turn_id.clone().unwrap_or_default()]) {
                        out.push(derived(&format!("cf:{}", card.turn_id), item.seq, item.turn_id.clone(), RowContent::Changes(card)));
                        starts.push(i);
                    }
                }
                out.push(item.clone());
                starts.push(i);
                i += 1;
            }
        }
        let floating: Vec<usize> = if working_line {
            let mut list: Vec<usize> = self.pending_cards.keys().filter_map(|k| self.item_index.get(k).copied()).filter(|&k| self.is_floating_card(k)).collect();
            list.sort_unstable();
            list
        } else {
            Vec::new()
        };
        for &k in &floating {
            out.push(self.items[k].clone());
            starts.push(self.items.len());
        }
        let turn = self.open_turn.clone();
        if working_line && turn.as_ref().is_some_and(|t| !self.pending_cards.values().any(|v| v == t)) {
            let turn = turn.unwrap_or_default();
            let status = WorkingStatus { turn_id: turn.clone(), started_at: self.turn_starts.get(&turn).copied() };
            out.push(derived(&format!("w:{turn}"), self.last_seq, Some(turn), RowContent::Working(status)));
            starts.push(self.items.len());
        } else if working_line && self.open_turn.is_none() && !self.background.is_empty() && floating.is_empty() {
            // Between turns with sub-agents or jobs still going: the thread
            // is still being worked on, and this says so where the working
            // line would be. Not under a card a sub-agent is waiting on.
            let status = BackgroundStatus { tasks: self.background.clone() };
            out.push(derived("bg:", self.last_seq, None, RowContent::Background(status)));
            starts.push(self.items.len());
        }
        // Mike's messages on their way, last: under the working line, as a
        // queued one waits for that turn to end.
        if working_line {
            for message in &self.pending {
                out.push(derived(&format!("p:{}", message.id), self.last_seq, None, message.row().content));
                starts.push(self.items.len());
            }
        }
        (out, starts)
    }

    /// Claude's plan mode writes its plan to ~/.claude/plans/<name>.md.
    /// That's the CLI's own note, shown as the plan card; "Edited 1 file"
    /// for it, in a turn that changed nothing of the project's, read as a
    /// change.
    pub fn is_plan_file(path: &str) -> bool {
        path.contains("/.claude/plans/") && path.ends_with(".md")
    }

    /// The files a turn (or a stretch's turns) changed, from its Edit and
    /// Write calls that didn't fail, each with its edits' line counts added
    /// up, as one card under the last one's footer.
    fn changed_files(&self, turns: &[String]) -> Option<ChangedFiles> {
        let turn = turns.last()?;
        let ids: Vec<&String> = turns.iter().flat_map(|t| self.edits_by_turn.get(t).into_iter().flatten()).collect();
        if ids.is_empty() {
            return None;
        }
        let mut order: Vec<String> = Vec::new();
        let mut files: HashMap<String, ChangedFile> = HashMap::new();
        for id in ids {
            let Some(&i) = self.item_index.get(id) else { continue };
            let RowContent::Tool(tool) = &self.items[i].content else { continue };
            if tool.failed() {
                continue;
            }
            // Every file a tool changed: a Codex patch can touch several.
            for diff in tool.effective_diffs() {
                if diff.hunks.is_empty() || Self::is_plan_file(&diff.path) {
                    continue;
                }
                let path = if diff.path.is_empty() { tool.title() } else { diff.path.clone() };
                let (added, removed) = DiffBuilder::stats(&diff);
                if let Some(file) = files.get_mut(&path) {
                    file.added += added;
                    file.removed += removed;
                    file.diffs.push(diff);
                } else {
                    order.push(path.clone());
                    let key = ChangedFiles::expansion_key(turn, &path);
                    files.insert(
                        path.clone(),
                        ChangedFile {
                            path,
                            added,
                            removed,
                            diffs: vec![diff],
                            is_expanded: self.expanded.contains(&key),
                            shows_all: self.expanded.contains(&DiffPreview::all_key(&key)),
                            root: self.root.clone(),
                        },
                    );
                }
            }
        }
        if order.is_empty() {
            return None;
        }
        Some(ChangedFiles { turn_id: turn.clone(), files: order.iter().filter_map(|p| files.get(p).cloned()).collect() })
    }
}
