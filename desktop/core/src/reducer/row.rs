// One row in the transcript: a port of Transcript/TranscriptRow.swift.
use super::diff::DiffBuilder;
use super::event::*;
use super::groups::{BackgroundStatus, ChangedFiles, WorkGroup, WorkingStatus};
use super::text;
use super::turn_gap::TurnGap;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

/// Row versions are unique across all models and rebuilds, so a cache keyed
/// by (row id, version) can never return a stale render (`RowVersion`).
pub(crate) fn next_row_version() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed) + 1
}

/// `TranscriptRow`.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    /// Bumped every time the content changes, so caches keyed by (id,
    /// version) know when to re-render or re-measure.
    pub version: u64,
    /// Seq of the event that created the row.
    pub seq: i64,
    pub turn_id: Option<String>,
    pub content: RowContent,
    /// Shown inside an open work group. Set on display rows only; the same
    /// item renders differently there.
    pub in_group: bool,
}

impl Row {
    pub fn new(id: &str, seq: i64, turn_id: Option<String>, content: RowContent) -> Row {
        Row { id: id.to_owned(), version: 0, seq, turn_id, content, in_group: false }
    }

    pub fn kind(&self) -> RowKind {
        self.content.kind()
    }
}

/// `RowKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowKind {
    User,
    Assistant,
    Reasoning,
    Tool,
    Diff,
    Images,
    Approval,
    Question,
    Review,
    Plan,
    Footer,
    Note,
    Error,
    Secret,
    Asks,
    Group,
    Working,
    Background,
    Changes,
    Gap,
    PlanCard,
}

impl RowKind {
    /// `rawValue`, as `dump` and the golden files show it.
    pub fn as_str(self) -> &'static str {
        match self {
            RowKind::User => "user",
            RowKind::Assistant => "assistant",
            RowKind::Reasoning => "reasoning",
            RowKind::Tool => "tool",
            RowKind::Diff => "diff",
            RowKind::Images => "images",
            RowKind::Approval => "approval",
            RowKind::Question => "question",
            RowKind::Review => "review",
            RowKind::Plan => "plan",
            RowKind::Footer => "footer",
            RowKind::Note => "note",
            RowKind::Error => "error",
            RowKind::Secret => "secret",
            RowKind::Asks => "asks",
            RowKind::Group => "group",
            RowKind::Working => "working",
            RowKind::Background => "background",
            RowKind::Changes => "changes",
            RowKind::Gap => "gap",
            RowKind::PlanCard => "planCard",
        }
    }
}

/// `TextItem`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TextItem {
    pub item_id: String,
    pub text: String,
    /// Still receiving deltas.
    pub is_streaming: bool,
    /// The turn's summary from end_turn: the turn's reply, while its other
    /// messages fold into the work.
    pub is_summary: bool,
    /// A summary written with bullet replies off: any message of the turn
    /// is the reply over it, however short.
    pub gives_way: bool,
    /// Claude sent it as a narration block: its text is only the API's
    /// paraphrase of what the agent wrote, not the words.
    pub is_narration: bool,
    /// One reply's reply: the turn's reply, whatever else the turn wrote.
    pub is_final: bool,
}

impl TextItem {
    pub fn new(item_id: &str, text: &str) -> TextItem {
        TextItem { item_id: item_id.to_owned(), text: text.to_owned(), ..TextItem::default() }
    }
}

/// A message sent from this app and not shown by the runner yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    Sending,
    Queued,
    /// Held until a restarting runner is back.
    WaitingForRunner,
}

/// `UserMessage`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct UserMessage {
    pub text: String,
    pub images: Vec<ImageRef>,
    /// "runner" when the runner sent it by itself, "schedule" when a
    /// schedule did, None when a person or a client did.
    pub origin: Option<String>,
    /// The asks it answered from the card.
    pub answers: Vec<AskAnswer>,
    /// His answer to a review.
    pub review: Option<ReviewAnswer>,
    /// The seq of the message this one replaced, when Mike edited one.
    pub edit_of: Option<i64>,
    /// Mike can edit it and carry on from there. Set by the reducer.
    pub editable: bool,
    /// Sent from this app and not shown by the runner yet.
    pub pending: Option<Pending>,
}

impl UserMessage {
    pub fn new(text: &str) -> UserMessage {
        UserMessage { text: text.to_owned(), ..UserMessage::default() }
    }

    /// Mike wrote it, in the app or on the phone: not the runner, a
    /// schedule or an agent.
    pub fn is_mike(&self) -> bool {
        (self.origin.is_none() || self.origin.as_deref() == Some("phone")) && self.agent_header().is_none()
    }

    /// Sent by the runner, not by Mike: a quiet centred line.
    pub fn is_from_runner(&self) -> bool {
        self.origin.as_deref() == Some("runner")
    }

    /// Sent by a schedule: also a note.
    pub fn is_from_schedule(&self) -> bool {
        self.origin.as_deref() == Some("schedule")
    }

    /// Drawn as a centred note rather than Mike's bubble.
    pub fn shows_as_note(&self) -> bool {
        self.is_from_runner() || self.is_from_schedule()
    }

    /// The schedule's title and time from the message's first line.
    pub fn scheduled_header(&self) -> Option<ScheduledHeader> {
        if self.is_from_schedule() {
            ScheduledHeader::parse(&self.text)
        } else {
            None
        }
    }

    /// Sent by another thread's agent, or by this thread's own agent as the
    /// `then` of a `move_thread`: drawn as a card, not Mike's bubble.
    pub fn agent_header(&self) -> Option<AgentHeader> {
        if self.origin.is_none() {
            AgentHeader::parse(&self.text)
        } else {
            None
        }
    }
}

/// The line the runner starts a message from another thread's agent with
/// (`AgentHeader`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AgentHeader {
    /// The sending thread's title and id; empty for a move's `then`.
    pub title: String,
    pub thread_id: String,
    /// For a move's `then`: the folder the thread moved to.
    pub moved_to: Option<String>,
    /// The message without the header line.
    pub body: String,
}

impl AgentHeader {
    pub fn parse(message: &str) -> Option<AgentHeader> {
        let (first_raw, rest) = split_first_line(message);
        let first = text::trim_whitespace(first_raw);
        const THREAD: &str = "[Sent by the agent in the thread \"";
        const MOVE: &str = "[Sent by the agent before moving to ";
        let mut header = AgentHeader::default();
        if first.starts_with(THREAD) && first.ends_with(")]") {
            let inner = &first[THREAD.len()..first.len() - 2];
            // The title may hold quotes and brackets; the id never holds `" (`.
            let split = inner.rfind("\" (")?;
            header.title = inner[..split].to_owned();
            header.thread_id = inner[split + 3..].to_owned();
        } else if first.starts_with(MOVE) && first.ends_with(']') && text::count(first) > text::count(MOVE) + 1 {
            header.moved_to = Some(first[MOVE.len()..first.len() - 1].to_owned());
        } else {
            return None;
        }
        header.body = rest.map(|r| text::trim_newlines(r).to_owned()).unwrap_or_default();
        Some(header)
    }
}

/// `ScheduledHeader`: the line a schedule's message starts with,
/// `[Scheduled: "<title>", <whenText>]`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ScheduledHeader {
    pub title: String,
    pub when_text: String,
    /// The message without the header line.
    pub body: String,
}

impl ScheduledHeader {
    pub fn parse(message: &str) -> Option<ScheduledHeader> {
        let (first_raw, rest) = split_first_line(message);
        let first = text::trim_whitespace(first_raw);
        const LEAD: &str = "[Scheduled: \"";
        if !first.starts_with(LEAD) || !first.ends_with(']') {
            return None;
        }
        let inner = &first[LEAD.len()..first.len() - 1];
        // The title may hold quotes and commas; the runner's words for the
        // time never hold `", `, so the last one splits them.
        let split = inner.rfind("\", ")?;
        Some(ScheduledHeader {
            title: inner[..split].to_owned(),
            when_text: inner[split + 3..].to_owned(),
            body: rest.map(|r| text::trim_newlines(r).to_owned()).unwrap_or_default(),
        })
    }
}

// `text.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false)`.
fn split_first_line(s: &str) -> (&str, Option<&str>) {
    let mut parts = text::split_lf(s).into_iter();
    let first = parts.next().unwrap_or("");
    if s.len() > first.len() {
        (first, Some(&s[first.len() + 1..]))
    } else {
        (first, None)
    }
}

/// `ToolItem`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ToolItem {
    pub item_id: String,
    pub call: Option<ToolCall>,
    pub result: Option<ToolResult>,
    /// Live output from `tool.output.delta`, dropped once the result lands.
    pub live_output: String,
    /// The runner's `edit.diff`s for this item, one per path, in the order
    /// the paths first came. A Codex patch can touch several files.
    pub diffs: Vec<EditDiff>,
    /// The thread's working folder, so a file row names its folder from
    /// there ("scripts", not "photo-sync/scripts").
    pub root: Option<String>,
}

impl ToolItem {
    /// Adds an `edit.diff`: one for a path already here replaces that
    /// path's diff (Codex sends a patch again as it changes), a new path
    /// adds one.
    pub fn add(&mut self, diff: EditDiff) {
        if let Some(index) = self.diffs.iter().position(|d| d.path == diff.path) {
            self.diffs[index] = diff;
        } else {
            self.diffs.push(diff);
        }
    }

    pub fn name(&self) -> String {
        self.call.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| "Tool".to_owned())
    }

    pub fn title(&self) -> String {
        self.call.as_ref().map(|c| c.title.clone()).unwrap_or_else(|| self.name())
    }

    pub fn tool_kind(&self) -> ToolKind {
        self.call.as_ref().map(|c| c.kind).unwrap_or(ToolKind::Other)
    }

    pub fn is_finished(&self) -> bool {
        self.result.is_some()
    }

    pub fn failed(&self) -> bool {
        self.result.as_ref().map(|r| !r.ok).unwrap_or(false)
    }

    /// The diffs to show: the runner's `edit.diff`s if it sent any,
    /// otherwise one built from the Edit/MultiEdit/Write input.
    pub fn effective_diffs(&self) -> Vec<EditDiff> {
        if !self.diffs.is_empty() {
            return self.diffs.clone();
        }
        self.call.as_ref().and_then(DiffBuilder::diff_for_tool).map(|d| vec![d]).unwrap_or_default()
    }

    /// The paths it changes, cheaply (no diff is built): the runner's
    /// diffs, else the input's file path, else Codex's `paths` list.
    pub fn edited_paths(&self) -> Vec<String> {
        if !self.diffs.is_empty() {
            return self.diffs.iter().map(|d| d.path.clone()).collect();
        }
        let input = self.call.as_ref().map(|c| &c.input);
        if let Some(path) = input.and_then(|i| i.get("file_path").and_then(|v| v.as_str()).or_else(|| i.get("path").and_then(|v| v.as_str()))) {
            return vec![path.to_owned()];
        }
        input
            .and_then(|i| i.get("paths"))
            .and_then(|v| v.as_array())
            .map(|paths| paths.iter().filter_map(|p| p.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    }
}

/// `PlanCard.State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlanCardState {
    #[default]
    Waiting,
    Approved,
    SentBack,
}

/// "Plan first": the plan the agent put to Mike (`PlanCard`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PlanCard {
    pub item_id: String,
    pub text: String,
    pub state: PlanCardState,
    pub entries: Vec<PlanEntry>,
}

impl PlanCard {
    pub fn new(item_id: &str, text: &str) -> PlanCard {
        PlanCard { item_id: item_id.to_owned(), text: text.to_owned(), ..PlanCard::default() }
    }
}

/// `ApprovalItem`.
#[derive(Clone, Debug, PartialEq)]
pub struct ApprovalItem {
    pub request: ApprovalRequest,
    pub resolution: Option<ApprovalResolution>,
    /// The thread's working folder, for the file's folder in the card.
    pub root: Option<String>,
    /// The tool call it asked about has a row of its own, which opens to
    /// the same command or file. Settled, this line then doesn't open.
    pub tool_shown: bool,
    /// Another runner asking for a thread: the request as it was filed,
    /// once this Mac's runner has read it.
    pub start_request: Option<StartRequest>,
    queued_calls: Vec<ApprovalRequest>,
}

impl ApprovalItem {
    pub fn new(request: ApprovalRequest, root: Option<String>, tool_shown: bool, start_request: Option<StartRequest>) -> ApprovalItem {
        ApprovalItem { request, resolution: None, root, tool_shown, start_request, queued_calls: Vec::new() }
    }

    pub fn is_start_request(&self) -> bool {
        self.request.name == StartRequest::CARD_NAME
    }

    /// The agent's other calls waiting behind this one. Always empty on
    /// another runner's start request: a queue on the card swapped the
    /// request this Mac's runner read for the owning runner's own
    /// harmless-looking calls (Daybreak, 8 Nov). See `ApprovalItem.queued`.
    pub fn queued(&self) -> &[ApprovalRequest] {
        if self.is_start_request() {
            &[]
        } else {
            &self.queued_calls
        }
    }

    pub fn set_queued(&mut self, calls: Vec<ApprovalRequest>) {
        self.queued_calls = calls.into_iter().filter(|c| c.name != StartRequest::CARD_NAME).collect();
    }

    /// It shows what it asked: always while it waits (a card then), and
    /// once settled only if its tool call has no row to show it.
    pub fn opens(&self) -> bool {
        self.resolution.is_none() || !self.tool_shown
    }

    /// Mike let it happen: the request is now just a step of the work it
    /// allowed, so it folds into the run of work around it.
    pub fn is_allowed(&self) -> bool {
        matches!(self.resolution.as_ref().map(|r| r.decision), Some(ApprovalDecision::Allow) | Some(ApprovalDecision::AllowAlways))
    }
}

/// `QuestionItem`.
#[derive(Clone, Debug, PartialEq)]
pub struct QuestionItem {
    pub request: QuestionRequest,
    pub resolution: Option<ApprovalResolution>,
}

/// `SecretItem`: an agent asking for a secret, and how it ended. The value
/// itself is never in here.
#[derive(Clone, Debug, PartialEq)]
pub struct SecretItem {
    pub request: SecretRequest,
    pub resolution: Option<ApprovalResolution>,
}

impl SecretItem {
    /// The settled card's one line: "Mac administrator password · Saved
    /// securely", "Declined", "Request ended".
    pub fn resolved_text(&self) -> Option<String> {
        let resolution = self.resolution.as_ref()?;
        Some(match resolution.decision {
            ApprovalDecision::Answered => format!("{} · Saved securely", self.request.label),
            ApprovalDecision::Expired => format!("{} · Request ended", self.request.label),
            _ => format!("{} · Declined", self.request.label),
        })
    }
}

/// `RowContent`.
#[derive(Clone, Debug, PartialEq)]
pub enum RowContent {
    User(UserMessage),
    Assistant(TextItem),
    Reasoning(TextItem),
    Tool(ToolItem),
    /// A tool's pictures, under its row. `generated` is an image the agent
    /// made (Codex's image generation), shown larger than a screenshot.
    Images { item_id: String, images: Vec<ImageRef>, generated: bool },
    Approval(ApprovalItem),
    Question(QuestionItem),
    Secret(SecretItem),
    Review(DesignReview),
    Plan(Vec<PlanEntry>),
    PlanCard(PlanCard),
    Footer(TurnCompletion),
    /// The asks a finished turn left for Mike, under its footer.
    Asks(AsksCard),
    Note(String),
    /// A failed turn's message, and why it failed when the runner said.
    Error { message: String, kind: Option<TurnErrorKind> },
    Group(WorkGroup),
    Working(WorkingStatus),
    Background(BackgroundStatus),
    Changes(ChangedFiles),
    /// "6 min later · Mine the repo finished" before a turn background work
    /// woke.
    Gap(TurnGap),
}

impl RowContent {
    pub fn kind(&self) -> RowKind {
        match self {
            RowContent::User(_) => RowKind::User,
            RowContent::Assistant(_) => RowKind::Assistant,
            RowContent::Reasoning(_) => RowKind::Reasoning,
            RowContent::Tool(tool) => {
                if tool.call.as_ref().map(DiffBuilder::is_edit_tool).unwrap_or(false) || !tool.diffs.is_empty() {
                    RowKind::Diff
                } else {
                    RowKind::Tool
                }
            }
            RowContent::Images { .. } => RowKind::Images,
            RowContent::Approval(_) => RowKind::Approval,
            RowContent::Question(_) => RowKind::Question,
            RowContent::Secret(_) => RowKind::Secret,
            RowContent::Review(_) => RowKind::Review,
            RowContent::Plan(_) => RowKind::Plan,
            RowContent::PlanCard(_) => RowKind::PlanCard,
            RowContent::Footer(_) => RowKind::Footer,
            RowContent::Asks(_) => RowKind::Asks,
            RowContent::Note(_) => RowKind::Note,
            RowContent::Error { .. } => RowKind::Error,
            RowContent::Group(_) => RowKind::Group,
            RowContent::Working(_) => RowKind::Working,
            RowContent::Background(_) => RowKind::Background,
            RowContent::Changes(_) => RowKind::Changes,
            RowContent::Gap(_) => RowKind::Gap,
        }
    }

    /// The note a thread back from hibernating says so in (`isWakeNote`).
    /// The runner words it (runner/src/runner/hibernation.ts) and it's what
    /// Mike came to the thread to read, so it never folds into the work
    /// above it.
    pub fn is_wake_note(&self) -> bool {
        matches!(self, RowContent::Note(text) if text.starts_with("Woke up"))
    }
}

/// What Mike has put in an ask's own box (its Something else): his words
/// and any pictures he pasted or dropped there.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AskNote {
    pub text: String,
    pub images: Vec<ImageRef>,
}

impl AskNote {
    pub fn is_empty(&self) -> bool {
        text::trim_ws_nl(&self.text).is_empty() && self.images.is_empty()
    }
}

/// A finished turn's asks (`AsksCard`, docs/protocol.md "Needs you"). Only
/// the card of the thread's last finished turn is open.
#[derive(Clone, Debug, PartialEq)]
pub struct AsksCard {
    /// The seq of the turn.completed the asks came with.
    pub seq: i64,
    pub asks: Vec<Ask>,
    pub ticked: BTreeSet<i64>,
    pub is_open: bool,
    /// The option picked on each ask (ask index to option index).
    pub picks: BTreeMap<i64, i64>,
    /// The done card's follow-ups (end_turn's `next`).
    pub next: Vec<String>,
    pub picked_next: BTreeSet<i64>,
    /// The follow-ups whose "?" is on, by index.
    pub explain_next: BTreeSet<i64>,
    /// What Mike has written and pasted in the boxes under the asks.
    pub notes: BTreeMap<i64, AskNote>,
    /// The done card's loose ends, as the runner checked them: None when
    /// it didn't, empty when nothing's left.
    pub loose_ends: Option<Vec<LooseEnd>>,
}

impl AsksCard {
    pub fn new(seq: i64, asks: Vec<Ask>) -> AsksCard {
        AsksCard {
            seq,
            asks,
            ticked: BTreeSet::new(),
            is_open: true,
            picks: BTreeMap::new(),
            next: Vec::new(),
            picked_next: BTreeSet::new(),
            explain_next: BTreeSet::new(),
            notes: BTreeMap::new(),
            loose_ends: None,
        }
    }

    pub fn ticked_count(&self) -> usize {
        (0..self.asks.len() as i64).filter(|i| self.ticked.contains(i)).count()
    }

    /// A turn that said it needs nothing gets a card too, with no asks: a
    /// turn that needs Mike only gets a card when it has asks, so no asks
    /// means this.
    pub fn is_done(&self) -> bool {
        self.asks.is_empty()
    }
}
