// What the runner sends, as Rust types: a port of Protocol/Event.swift and
// the parts of Models.swift, DesignReview.swift and StartRequest.swift the
// reducer reads.
//
// Decoding goes through serde_json::Value by hand rather than serde's derive,
// because the Swift decoders' tolerance (docs/protocol.md: "keep both sides
// tolerant of unknown fields and event types") is written field by field:
// some fields fall back when the value is the wrong type (`try?`), some fail
// the whole event (`try`), and which is which is part of the behaviour. Each
// helper below names the Swift expression it stands for.
use super::format::{int_checked, whole_count};
use super::text;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

type Object = Map<String, Value>;

/// `try? c.decodeIfPresent(String.self, forKey:)`: missing, null or the
/// wrong type is all None.
pub(crate) fn string(c: &Object, key: &str) -> Option<String> {
    c.get(key)?.as_str().map(str::to_owned)
}

/// `try c.decodeIfPresent(String.self, forKey:)`: the wrong type fails the
/// whole value.
pub(crate) fn string_strict(c: &Object, key: &str) -> Result<Option<String>, ()> {
    match c.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(()),
    }
}

/// `try? c.decodeIfPresent(Bool.self, forKey:)`. A JSON boolean only.
fn boolean(c: &Object, key: &str) -> Option<bool> {
    c.get(key)?.as_bool()
}

/// `try? c.decodeIfPresent(Double.self, forKey:)`. A JSON number only.
fn number(c: &Object, key: &str) -> Option<f64> {
    c.get(key)?.as_f64()
}

/// `try? c.decodeIfPresent(JSONValue.self, forKey:)`: null is None.
fn any(c: &Object, key: &str) -> Option<Value> {
    c.get(key).filter(|v| !v.is_null()).cloned()
}

/// JSONDecoder's `Int`: a number with no fractional part that fits (1.0 is
/// fine, 1.5 and 2^64 aren't).
fn int_value(v: &Value) -> Option<i64> {
    if let Some(i) = v.as_i64() {
        return Some(i);
    }
    let f = v.as_f64()?;
    if f.fract() == 0.0 && (-9.223372036854775808e18..9.223372036854775808e18).contains(&f) {
        Some(f as i64)
    } else {
        None
    }
}

/// `c.lenientInt(forKey:)`: a whole number or numeric string within what
/// JavaScript counts exactly.
pub(crate) fn lenient_int(c: &Object, key: &str) -> Option<i64> {
    const MOST: i64 = 9_007_199_254_740_991;
    let held = |value: i64| (-MOST..=MOST).contains(&value).then_some(value);
    let value = c.get(key)?;
    if let Some(i) = int_value(value) {
        return held(i);
    }
    if let Some(f) = value.as_f64() {
        return int_checked(f).and_then(held);
    }
    if let Some(s) = value.as_str() {
        return s.parse::<i64>().ok().and_then(held);
    }
    None
}

/// `c.lossyArray(_:forKey:)`: elements that don't decode are skipped, and a
/// missing, null or non-array value is empty.
fn lossy<T>(c: &Object, key: &str, mut decode: impl FnMut(&Value) -> Option<T>) -> Vec<T> {
    match c.get(key) {
        Some(Value::Array(items)) => items.iter().filter_map(&mut decode).collect(),
        _ => Vec::new(),
    }
}

/// A `[String]` that's all strings, or None (a `try?` over a strict array).
fn strings_strict(c: &Object, key: &str) -> Option<Vec<String>> {
    c.get(key)?.as_array()?.iter().map(|v| v.as_str().map(str::to_owned)).collect()
}

fn lossy_strings(c: &Object, key: &str) -> Vec<String> {
    lossy(c, key, |v| v.as_str().map(str::to_owned))
}

// ---- Models the rows carry ----

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ImageRef {
    pub path: String,
    pub media_type: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

impl ImageRef {
    pub fn new(path: &str, media_type: &str) -> ImageRef {
        ImageRef { path: path.to_owned(), media_type: media_type.to_owned(), width: None, height: None }
    }

    /// Lenient about sizes (never `Int(_:)` of a number off the wire), strict
    /// that `path` is a string.
    pub fn decode(v: &Value) -> Option<ImageRef> {
        let c = v.as_object()?;
        let path = c.get("path")?.as_str()?.to_owned();
        let media_type = string_strict(c, "mediaType").ok()?.unwrap_or_else(|| "image/png".to_owned());
        let size = |key: &str| -> Result<Option<i64>, ()> {
            match c.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => {
                    let f = v.as_f64().ok_or(())?;
                    Ok(whole_count(f, 1_000_000.0))
                }
            }
        };
        let width = size("width").ok()?;
        let height = size("height").ok()?;
        Some(ImageRef { path, media_type, width, height })
    }
}

/// Work the live CLI is still running in the background (`BackgroundTask`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BackgroundKind {
    Agent,
    Shell,
    Monitor,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundTask {
    pub kind: BackgroundKind,
    /// The CLI's own words: the sub-agent's brief, the command. May be empty.
    pub description: String,
    pub finished: bool,
}

impl BackgroundTask {
    pub fn new(kind: BackgroundKind, description: &str, finished: bool) -> BackgroundTask {
        BackgroundTask { kind, description: description.to_owned(), finished }
    }

    /// `BackgroundTask.init(from:)`: lenient about a kind it doesn't know,
    /// strict about the types of the fields it does.
    pub fn decode(v: &Value) -> Option<BackgroundTask> {
        let c = v.as_object()?;
        let kind = match c.get("kind") {
            None | Some(Value::Null) => BackgroundKind::Other,
            Some(Value::String(raw)) => match raw.as_str() {
                "agent" => BackgroundKind::Agent,
                "shell" => BackgroundKind::Shell,
                "monitor" => BackgroundKind::Monitor,
                _ => BackgroundKind::Other,
            },
            Some(_) => return None,
        };
        let description = string_strict(c, "description").ok()?.unwrap_or_default();
        let finished = match c.get("finished") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return None,
        };
        Some(BackgroundTask { kind, description, finished })
    }
}

/// What's still open on a thread's done card (`LooseEnds`).
#[derive(Clone, Debug, PartialEq)]
pub struct LooseEnds {
    pub seq: i64,
    pub ends: Vec<LooseEnd>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LooseEnd {
    pub kind: String,
    pub text: String,
    pub thread_id: Option<String>,
    pub at: Option<String>,
}

impl LooseEnds {
    /// `LooseEnds.init(from:)`: no `seq` means there's nothing to hang it on.
    pub fn decode(v: &Value) -> Option<LooseEnds> {
        let c = v.as_object()?;
        let seq = lenient_int(c, "seq")?;
        Some(LooseEnds { seq, ends: lossy(c, "ends", LooseEnd::decode) })
    }

    /// Any that should stop a plain Archive: a booked check-in doesn't.
    pub fn any_open(&self) -> bool {
        self.ends.iter().any(|end| end.kind != "scheduled")
    }
}

impl LooseEnd {
    pub fn new(kind: &str, text: &str) -> LooseEnd {
        LooseEnd { kind: kind.to_owned(), text: text.to_owned(), thread_id: None, at: None }
    }

    // Synthesized Decodable: required fields strict, optionals strict in type.
    fn decode(v: &Value) -> Option<LooseEnd> {
        let c = v.as_object()?;
        Some(LooseEnd {
            kind: c.get("kind")?.as_str()?.to_owned(),
            text: c.get("text")?.as_str()?.to_owned(),
            thread_id: string_strict(c, "threadId").ok()?,
            at: string_strict(c, "at").ok()?,
        })
    }
}

/// One thing a finished turn needs Mike to do (`Ask`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Ask {
    pub action: String,
    pub where_: Option<String>,
    pub reply: Option<String>,
    pub why: Option<String>,
    pub options: Vec<String>,
    /// One note per option, or none: a list that doesn't line up is dropped.
    pub option_notes: Vec<String>,
    pub suggested: Option<usize>,
    pub links: Vec<AskLink>,
    pub ticked: bool,
}

impl Ask {
    pub fn new(action: &str) -> Ask {
        Ask { action: action.to_owned(), ..Ask::default() }
    }

    pub fn decode(v: &Value) -> Option<Ask> {
        let c = v.as_object()?;
        let action = c.get("action")?.as_str()?.to_owned();
        let options = strings_strict(c, "options").unwrap_or_default();
        let notes = strings_strict(c, "optionNotes").unwrap_or_default();
        let option_notes = if notes.len() == options.len() && notes.iter().any(|n| !n.is_empty()) { notes } else { Vec::new() };
        let suggested = c
            .get("suggested")
            .and_then(int_value)
            .filter(|i| (0..options.len() as i64).contains(i))
            .map(|i| i as usize);
        Some(Ask {
            action,
            where_: string(c, "where"),
            reply: string(c, "reply"),
            why: string(c, "why"),
            options,
            option_notes,
            suggested,
            links: lossy(c, "links", AskLink::decode),
            ticked: boolean(c, "ticked").unwrap_or(false),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AskLink {
    pub title: String,
    pub url: String,
}

impl AskLink {
    pub fn decode(v: &Value) -> Option<AskLink> {
        let c = v.as_object()?;
        Some(AskLink { title: c.get("title")?.as_str()?.to_owned(), url: c.get("url")?.as_str()?.to_owned() })
    }
}

/// One of the open card's asks a message answered (`AskAnswer`).
#[derive(Clone, Debug, PartialEq)]
pub struct AskAnswer {
    pub action: String,
    pub answer: String,
}

impl AskAnswer {
    pub fn decode(v: &Value) -> Option<AskAnswer> {
        let c = v.as_object()?;
        Some(AskAnswer { action: c.get("action")?.as_str()?.to_owned(), answer: c.get("answer")?.as_str()?.to_owned() })
    }
}

// ---- Design reviews (DesignReview.swift) ----

/// A draft is saved separately from submission (`ReviewDraft`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ReviewDraft {
    pub picked_ids: Vec<String>,
    pub notes: BTreeMap<String, String>,
    pub reply: Option<String>,
    pub none: bool,
}

impl ReviewDraft {
    /// The first pick: all a runner from before several picks knows about.
    pub fn picked_id(&self) -> Option<&str> {
        self.picked_ids.first().map(String::as_str)
    }

    pub fn decode(v: &Value) -> Option<ReviewDraft> {
        let c = v.as_object()?;
        let first = string_strict(c, "pickedId").ok()?;
        let all = strings_strict(c, "pickedIds");
        let picked_ids = all.or_else(|| first.map(|id| vec![id])).unwrap_or_default();
        let notes = c
            .get("notes")
            .and_then(Value::as_object)
            .and_then(|map| {
                // A `[String: String]`: one value of the wrong type drops them all.
                map.iter().map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned()))).collect::<Option<BTreeMap<_, _>>>()
            })
            .unwrap_or_default();
        Some(ReviewDraft { picked_ids, notes, reply: string(c, "reply"), none: boolean(c, "none").unwrap_or(false) })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewShot {
    pub title: Option<String>,
    pub image: ImageRef,
    pub media: Option<ImageRef>,
}

impl ReviewShot {
    fn decode(v: &Value) -> Option<ReviewShot> {
        let c = v.as_object()?;
        let title = string_strict(c, "title").ok()?;
        let image = ImageRef::decode(c.get("image")?)?;
        let media = match c.get("media") {
            None | Some(Value::Null) => None,
            Some(v) => Some(ImageRef::decode(v)?),
        };
        Some(ReviewShot { title, image, media })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewOption {
    pub id: String,
    pub title: String,
    pub note: Option<String>,
    pub reference: bool,
    pub image: ImageRef,
    pub shots: Vec<ReviewShot>,
    pub url: Option<String>,
}

impl ReviewOption {
    fn decode(v: &Value) -> Option<ReviewOption> {
        let c = v.as_object()?;
        let id = c.get("id")?.as_str()?.to_owned();
        let title = string_strict(c, "title").ok()?.unwrap_or_else(|| id.clone());
        let note = string_strict(c, "note").ok()?;
        let reference = boolean(c, "reference").unwrap_or(false);
        let image = ImageRef::decode(c.get("image")?)?;
        // Only a web page (LinkPolicy.web): a runner, maybe another Mac's,
        // sends this, and Open live hands it to the system.
        let url = string(c, "url").filter(|u| is_web_address(u));
        Some(ReviewOption { id, title, note, reference, image, shots: lossy(c, "shots", ReviewShot::decode), url })
    }
}

/// `LinkPolicy.web` for a string: an http(s) address with a host. Swift
/// checks it with URL(string:); this applies the rule without a URL parser,
/// and also refuses what URL(string:) would (spaces, control characters).
pub fn is_web_address(url: &str) -> bool {
    if text::count(url) > 8192 {
        return false;
    }
    let Some(colon) = url.find("://") else { return false };
    let scheme = url[..colon].to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return false;
    }
    if url.chars().any(|c| c.is_control() || c == ' ') {
        return false;
    }
    let rest = &url[colon + 3..];
    let authority = rest.split(|c| c == '/' || c == '?' || c == '#').next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    !host.is_empty()
}

/// The latest persisted snapshot of a review (`DesignReview`).
#[derive(Clone, Debug, PartialEq)]
pub struct DesignReview {
    pub id: String,
    pub title: String,
    pub note: Option<String>,
    pub recommended: Option<String>,
    pub options: Vec<ReviewOption>,
    pub status: String,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
    pub draft: ReviewDraft,
    pub response: Option<ReviewDraft>,
    pub reply_turn_id: Option<String>,
}

impl DesignReview {
    pub fn is_pending(&self) -> bool {
        self.status == "pending"
    }

    pub fn decode(v: &Value) -> Option<DesignReview> {
        let c = v.as_object()?;
        let id = c.get("id")?.as_str()?.to_owned();
        let title = string_strict(c, "title").ok()?.unwrap_or_else(|| "Design options".to_owned());
        let note = string_strict(c, "note").ok()?;
        let recommended = string(c, "recommended");
        let options = lossy(c, "options", ReviewOption::decode);
        let status = string_strict(c, "status").ok()?.unwrap_or_else(|| "unknown".to_owned());
        let revision = lenient_int(c, "revision").unwrap_or(0);
        let created_at = string_strict(c, "createdAt").ok()?.unwrap_or_default();
        let updated_at = string_strict(c, "updatedAt").ok()?.unwrap_or_else(|| created_at.clone());
        let draft = match c.get("draft") {
            None | Some(Value::Null) => ReviewDraft::default(),
            Some(v) => ReviewDraft::decode(v)?,
        };
        let response = match c.get("response") {
            None | Some(Value::Null) => None,
            Some(v) => Some(ReviewDraft::decode(v)?),
        };
        let reply_turn_id = string_strict(c, "replyTurnId").ok()?;
        Some(DesignReview { id, title, note, recommended, options, status, revision, created_at, updated_at, draft, response, reply_turn_id })
    }
}

/// Mike's answer to a review, read back out of the message the runner sent
/// for it (`ReviewAnswer`), so his bubble can say what he picked.
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewAnswer {
    pub lines: Vec<ReviewAnswerLine>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewAnswerLine {
    /// Small and quiet, over the line: what it's about.
    pub label: String,
    /// In his own words' style: what he picked or wrote.
    pub text: String,
}

impl ReviewAnswer {
    /// None when the text isn't the runner's wording for this review and
    /// draft, so the bubble shows it as it is.
    pub fn parse(text: &str, review_id: Option<&str>, response: Option<&ReviewDraft>) -> Option<ReviewAnswer> {
        let (review_id, response) = (review_id?, response?);
        // "Review: <title> (<id>)"
        let head = format!(" ({review_id})");
        let mut rest = Self::without_passed_over(text).strip_prefix("Review: ")?;
        let close = rest.find(&head)?;
        let title = rest[..close].to_owned();
        rest = &rest[close + head.len()..];
        let mut lines: Vec<ReviewAnswerLine> = Vec::new();
        // "Picked <id>. <option title>.", or "Picked A + D. <title> + <title>."
        let picks = &response.picked_ids;
        if !picks.is_empty() {
            let lead = format!("\n\nPicked {}. ", picks.join(" + "));
            rest = rest.strip_prefix(&lead)?;
            let end = rest.find("\n\n").unwrap_or(rest.len());
            let mut option = &rest[..end];
            if let Some(without) = option.strip_suffix('.') {
                option = without;
            }
            let what = if picks.len() == 1 { Self::bare(option, &picks[0]) } else { option };
            lines.push(ReviewAnswerLine { label: title.clone(), text: format!("Picked {}: {what}", picks.join(" + ")) });
            rest = &rest[end..];
        }
        if response.none {
            rest = rest.strip_prefix("\n\nPicked none of them.")?;
            lines.push(ReviewAnswerLine { label: title.clone(), text: "None of them".to_owned() });
        }
        // His reply, as he wrote it, under the pick or on its own.
        if let Some(reply) = response.reply.as_deref().filter(|r| !r.is_empty()) {
            let lead = format!("\n\n{reply}");
            rest = rest.strip_prefix(&lead)?;
            let label = if lines.is_empty() { title.clone() } else { String::new() };
            lines.push(ReviewAnswerLine { label, text: reply.to_owned() });
        }
        // "Notes on <id> (<option title>): <note>", in the review's option
        // order, which the draft doesn't keep, so whichever comes next.
        let mut notes: Vec<(&str, &str)> =
            response.notes.iter().filter(|(_, note)| !note.is_empty()).map(|(k, v)| (k.as_str(), v.as_str())).collect();
        while !notes.is_empty() {
            let position = notes.iter().position(|(id, _)| rest.starts_with(&format!("\n\nNotes on {id} (")))?;
            let (id, note) = notes.remove(position);
            rest = &rest[format!("\n\nNotes on {id} (").len()..];
            let tail = format!("): {note}");
            let found = rest.find(&tail)?;
            let option = &rest[..found];
            // With nothing picked, the review's title goes over the first one.
            let label = format!("{}Notes on {id}: {}", if lines.is_empty() { format!("{title}\n") } else { String::new() }, Self::bare(option, id));
            lines.push(ReviewAnswerLine { label, text: note.to_owned() });
            rest = &rest[found + tail.len()..];
        }
        (rest.is_empty() && !lines.is_empty()).then_some(ReviewAnswer { lines })
    }

    /// The runner's last paragraph names the options he passed over, for the
    /// agent writing follow-ups (`PASSED_OVER_TAIL` in runner/src/reviews.ts);
    /// his bubble leaves it out.
    pub const PASSED_OVER_TAIL: &'static str =
        "Don't offer one of these later (a next step, a follow-up) as if it were new: say it's one they passed over.";

    fn without_passed_over(text: &str) -> &str {
        match text.strip_suffix(Self::PASSED_OVER_TAIL).and_then(|_| text.rfind("\n\nPassed over: ")) {
            Some(start) => &text[..start],
            None => text,
        }
    }

    // Agents often title options "A: Summary first".
    fn bare<'a>(option: &'a str, id: &str) -> &'a str {
        for sep in [": ", ". ", " - ", ") "] {
            let lead = format!("{id}{sep}");
            if option.starts_with(&lead) && text::count(option) > text::count(&lead) {
                return &option[lead.len()..];
            }
        }
        option
    }
}

// ---- Another runner asking for a thread (StartRequest.swift) ----

#[derive(Clone, Debug, PartialEq)]
pub struct StartRequestFiled {
    pub runner_name: String,
    pub thread_id: String,
    pub target: String,
    pub text: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: Option<String>,
    pub why: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StartRequestRunner {
    pub id: String,
    pub name: String,
    /// This Mac's own runner (`self` in the JSON).
    pub is_self: bool,
}

/// What another runner asked for (`StartRequest`): an approval card's body
/// read from this Mac's own runner, never from the card.
#[derive(Clone, Debug, PartialEq)]
pub struct StartRequest {
    pub request: StartRequestFiled,
    pub target: Option<StartRequestRunner>,
    pub runners: Vec<StartRequestRunner>,
    pub fingerprint: Option<String>,
    pub missing_folder: Option<String>,
    pub error: Option<String>,
}

impl StartRequest {
    pub const CARD_NAME: &'static str = "threadbear.start_thread";

    /// Synthesized Decodable in Swift: every required field strict.
    pub fn decode(v: &Value) -> Option<StartRequest> {
        let c = v.as_object()?;
        fn runner(v: &Value) -> Option<StartRequestRunner> {
            let c = v.as_object()?;
            Some(StartRequestRunner {
                id: c.get("id")?.as_str()?.to_owned(),
                name: c.get("name")?.as_str()?.to_owned(),
                is_self: c.get("self")?.as_bool()?,
            })
        }
        let r = c.get("request")?.as_object()?;
        let request = StartRequestFiled {
            runner_name: r.get("runnerName")?.as_str()?.to_owned(),
            thread_id: r.get("threadId")?.as_str()?.to_owned(),
            target: r.get("target")?.as_str()?.to_owned(),
            text: r.get("text")?.as_str()?.to_owned(),
            title: string_strict(r, "title").ok()?,
            cwd: string_strict(r, "cwd").ok()?,
            model: string_strict(r, "model").ok()?,
            effort: string_strict(r, "effort").ok()?,
            permission_mode: string_strict(r, "permissionMode").ok()?,
            why: string_strict(r, "why").ok()?,
        };
        let target = match c.get("target") {
            None | Some(Value::Null) => None,
            Some(v) => Some(runner(v)?),
        };
        let runners = c.get("runners")?.as_array()?.iter().map(runner).collect::<Option<Vec<_>>>()?;
        Some(StartRequest {
            request,
            target,
            runners,
            fingerprint: string_strict(c, "fingerprint").ok()?,
            missing_folder: string_strict(c, "missingFolder").ok()?,
            error: string_strict(c, "error").ok()?,
        })
    }
}

// ---- Tools ----

/// `image` is image generation (Codex). A kind this app doesn't know reads
/// as `other`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ToolKind {
    Command,
    Read,
    Edit,
    Write,
    Search,
    Web,
    Mcp,
    Task,
    Todo,
    Image,
    #[default]
    Other,
}

impl ToolKind {
    pub fn from_raw(raw: &str) -> ToolKind {
        match raw {
            "command" => ToolKind::Command,
            "read" => ToolKind::Read,
            "edit" => ToolKind::Edit,
            "write" => ToolKind::Write,
            "search" => ToolKind::Search,
            "web" => ToolKind::Web,
            "mcp" => ToolKind::Mcp,
            "task" => ToolKind::Task,
            "todo" => ToolKind::Todo,
            "image" => ToolKind::Image,
            _ => ToolKind::Other,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub item_id: String,
    pub name: String,
    pub kind: ToolKind,
    pub title: String,
    pub input: Value,
}

impl ToolCall {
    pub fn new(item_id: &str, name: &str, kind: ToolKind, title: &str, input: Value) -> ToolCall {
        ToolCall { item_id: item_id.to_owned(), name: name.to_owned(), kind, title: title.to_owned(), input }
    }

    fn decode(c: &Object) -> ToolCall {
        let name = string(c, "name").unwrap_or_else(|| "Tool".to_owned());
        ToolCall {
            item_id: string(c, "itemId").unwrap_or_default(),
            kind: c.get("kind").and_then(Value::as_str).map(ToolKind::from_raw).unwrap_or(ToolKind::Other),
            title: string(c, "title").unwrap_or_else(|| name.clone()),
            input: any(c, "input").unwrap_or_else(|| json!({})),
            name,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ToolResult {
    pub item_id: String,
    pub ok: bool,
    pub text: String,
    pub truncated: bool,
    pub blob_path: Option<String>,
    pub images: Vec<ImageRef>,
    /// Another runner's thread through Convex: the output is on that runner.
    pub held_back_seq: Option<i64>,
}

impl ToolResult {
    pub fn new(item_id: &str, ok: bool, text: &str) -> ToolResult {
        ToolResult { item_id: item_id.to_owned(), ok, text: text.to_owned(), ..ToolResult::default() }
    }

    fn decode(c: &Object) -> ToolResult {
        ToolResult {
            item_id: string(c, "itemId").unwrap_or_default(),
            ok: boolean(c, "ok").unwrap_or(true),
            text: string(c, "text").unwrap_or_default(),
            truncated: boolean(c, "truncated").unwrap_or(false),
            blob_path: string(c, "blobPath"),
            images: lossy(c, "images", ImageRef::decode),
            held_back_seq: c.get("heldBack").and_then(Value::as_object).and_then(|held| held.get("seq")).and_then(int_value),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct DiffHunk {
    pub old_start: i64,
    pub old_lines: i64,
    pub new_start: i64,
    pub new_lines: i64,
    pub lines: Vec<String>,
}

impl DiffHunk {
    /// Missing counts default to what the lines say, so a hunk the runner
    /// sends in a slightly different shape still draws.
    fn decode(v: &Value) -> Option<DiffHunk> {
        let c = v.as_object()?;
        let lines = lossy_strings(c, "lines");
        let old_start = lenient_int(c, "oldStart").unwrap_or(1);
        let new_start = lenient_int(c, "newStart").unwrap_or(old_start);
        let old_lines = lenient_int(c, "oldLines").unwrap_or_else(|| lines.iter().filter(|l| !l.starts_with('+')).count() as i64);
        let new_lines = lenient_int(c, "newLines").unwrap_or_else(|| lines.iter().filter(|l| !l.starts_with('-')).count() as i64);
        Some(DiffHunk { old_start, old_lines, new_start, new_lines, lines })
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct EditDiff {
    pub item_id: String,
    pub path: String,
    pub hunks: Vec<DiffHunk>,
}

impl EditDiff {
    fn decode(c: &Object) -> EditDiff {
        EditDiff {
            item_id: string(c, "itemId").unwrap_or_default(),
            path: string(c, "path").unwrap_or_default(),
            hunks: lossy(c, "hunks", DiffHunk::decode),
        }
    }
}

// ---- Approvals, questions and secrets ----

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ApprovalOption {
    Allow,
    AllowAlways,
    Deny,
}

impl ApprovalOption {
    fn from_raw(raw: &str) -> Option<ApprovalOption> {
        match raw {
            "allow" => Some(ApprovalOption::Allow),
            "allowAlways" => Some(ApprovalOption::AllowAlways),
            "deny" => Some(ApprovalOption::Deny),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApprovalRequest {
    pub approval_id: String,
    pub item_id: Option<String>,
    pub name: String,
    pub title: String,
    pub input: Value,
    pub options: Vec<ApprovalOption>,
}

impl ApprovalRequest {
    pub fn new(approval_id: &str, name: &str, title: &str, input: Value) -> ApprovalRequest {
        ApprovalRequest {
            approval_id: approval_id.to_owned(),
            item_id: None,
            name: name.to_owned(),
            title: title.to_owned(),
            input,
            options: vec![ApprovalOption::Allow, ApprovalOption::AllowAlways, ApprovalOption::Deny],
        }
    }

    pub fn decode(v: &Value) -> Option<ApprovalRequest> {
        Some(Self::decode_object(v.as_object()?))
    }

    fn decode_object(c: &Object) -> ApprovalRequest {
        let name = string(c, "name").unwrap_or_else(|| "Tool".to_owned());
        // Unknown options are dropped rather than failing the event. A card
        // left with none still gets Allow and Deny, which every runner takes,
        // rather than no buttons.
        let raw: Vec<String> = match c.get("options") {
            Some(Value::Array(items)) => items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
            _ => vec!["allow".to_owned(), "deny".to_owned()],
        };
        let known: Vec<ApprovalOption> = raw.iter().filter_map(|o| ApprovalOption::from_raw(o)).collect();
        ApprovalRequest {
            approval_id: string(c, "approvalId").unwrap_or_default(),
            item_id: string(c, "itemId"),
            title: string(c, "title").unwrap_or_else(|| name.clone()),
            name,
            input: any(c, "input").unwrap_or_else(|| json!({})),
            options: if known.is_empty() { vec![ApprovalOption::Allow, ApprovalOption::Deny] } else { known },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
}

impl QuestionOption {
    fn decode(v: &Value) -> Option<QuestionOption> {
        // A bare string is taken as the label.
        if let Some(label) = v.as_str() {
            return Some(QuestionOption { label: label.to_owned(), description: None });
        }
        let c = v.as_object()?;
        Some(QuestionOption {
            label: string(c, "label").or_else(|| string(c, "value")).unwrap_or_default(),
            description: string(c, "description"),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Question {
    pub question: String,
    pub header: Option<String>,
    pub multi_select: bool,
    pub options: Vec<QuestionOption>,
}

impl Question {
    fn decode(v: &Value) -> Option<Question> {
        let c = v.as_object()?;
        Some(Question {
            question: string(c, "question").unwrap_or_default(),
            header: string(c, "header"),
            multi_select: boolean(c, "multiSelect").unwrap_or(false),
            options: lossy(c, "options", QuestionOption::decode).into_iter().filter(|o| !o.label.is_empty()).collect(),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuestionRequest {
    pub approval_id: String,
    /// Absent for a background subagent's.
    pub item_id: Option<String>,
    pub questions: Vec<Question>,
}

impl QuestionRequest {
    fn decode(c: &Object) -> QuestionRequest {
        QuestionRequest {
            approval_id: string(c, "approvalId").unwrap_or_default(),
            item_id: string(c, "itemId"),
            questions: lossy(c, "questions", Question::decode),
        }
    }
}

/// An agent's `ask_secret` card (`SecretRequest`). Nothing here, or in what
/// the app keeps, is the value.
#[derive(Clone, Debug, PartialEq)]
pub struct SecretRequest {
    pub approval_id: String,
    pub item_id: Option<String>,
    pub label: String,
    pub reason: String,
    pub placeholder: Option<String>,
}

impl SecretRequest {
    pub fn new(approval_id: &str, label: &str) -> SecretRequest {
        SecretRequest { approval_id: approval_id.to_owned(), item_id: None, label: label.to_owned(), reason: String::new(), placeholder: None }
    }

    fn decode(c: &Object) -> SecretRequest {
        let label = string(c, "label").map(|l| text::trim_ws_nl(&l).to_owned()).unwrap_or_default();
        let placeholder = string(c, "placeholder").map(|p| text::trim_ws_nl(&p).to_owned());
        SecretRequest {
            approval_id: string(c, "approvalId").unwrap_or_default(),
            item_id: string(c, "itemId").filter(|id| !id.is_empty()),
            label: if label.is_empty() { "A secret".to_owned() } else { label },
            reason: string(c, "reason").map(|r| text::trim_ws_nl(&r).to_owned()).unwrap_or_default(),
            placeholder: placeholder.filter(|p| !p.is_empty()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ApprovalDecision {
    Allow,
    AllowAlways,
    Deny,
    Answered,
    Expired,
}

impl ApprovalDecision {
    /// `rawValue`.
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalDecision::Allow => "allow",
            ApprovalDecision::AllowAlways => "allowAlways",
            ApprovalDecision::Deny => "deny",
            ApprovalDecision::Answered => "answered",
            ApprovalDecision::Expired => "expired",
        }
    }

    fn from_raw(raw: &str) -> ApprovalDecision {
        match raw {
            "allow" => ApprovalDecision::Allow,
            "allowAlways" => ApprovalDecision::AllowAlways,
            "deny" => ApprovalDecision::Deny,
            "answered" => ApprovalDecision::Answered,
            // A decision this app doesn't know reads as expired.
            _ => ApprovalDecision::Expired,
        }
    }
}

/// The agent's other calls from the same reply, waiting behind an approval
/// card (`ApprovalQueue`, docs/protocol.md "Queued calls").
#[derive(Clone, Debug, PartialEq)]
pub struct ApprovalQueue {
    pub approval_id: String,
    pub calls: Vec<ApprovalRequest>,
}

impl ApprovalQueue {
    fn decode(c: &Object) -> ApprovalQueue {
        // Each is shaped as its own card would be, with its tool call's id
        // as both ids. One with no item id can't be answered, so it's dropped
        // rather than given a made-up id.
        let calls = lossy(c, "calls", |v| {
            let call = v.as_object()?;
            let item_id = call.get("itemId")?.as_str()?.to_owned();
            if item_id.is_empty() {
                return None;
            }
            let name = string_strict(call, "name").ok()?;
            let title = string_strict(call, "title").ok()?;
            Some(ApprovalRequest {
                approval_id: item_id.clone(),
                item_id: Some(item_id),
                title: title.or_else(|| name.clone()).unwrap_or_else(|| "Tool".to_owned()),
                name: name.unwrap_or_else(|| "Tool".to_owned()),
                input: any(call, "input").unwrap_or_else(|| json!({})),
                options: vec![ApprovalOption::Allow, ApprovalOption::Deny],
            })
        });
        ApprovalQueue { approval_id: string(c, "approvalId").unwrap_or_default(), calls }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApprovalResolution {
    pub approval_id: String,
    pub decision: ApprovalDecision,
    pub message: Option<String>,
}

impl ApprovalResolution {
    fn decode(c: &Object) -> ApprovalResolution {
        ApprovalResolution {
            approval_id: string(c, "approvalId").unwrap_or_default(),
            decision: c.get("decision").and_then(Value::as_str).map(ApprovalDecision::from_raw).unwrap_or(ApprovalDecision::Expired),
            message: string(c, "message"),
        }
    }
}

// ---- Plans and turns ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanEntry {
    pub text: String,
    pub status: PlanStatus,
}

impl PlanEntry {
    pub fn new(text: &str, status: PlanStatus) -> PlanEntry {
        PlanEntry { text: text.to_owned(), status }
    }

    /// Claude's TodoWrite calls the text `content`; either is accepted.
    fn decode(v: &Value) -> Option<PlanEntry> {
        let c = v.as_object()?;
        let text = string(c, "text").or_else(|| string(c, "content")).unwrap_or_default();
        let status = match c.get("status").and_then(Value::as_str) {
            Some("in_progress") => PlanStatus::InProgress,
            Some("completed") => PlanStatus::Completed,
            _ => PlanStatus::Pending,
        };
        Some(PlanEntry { text, status })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TurnOutcome {
    #[default]
    Completed,
    Interrupted,
    Failed,
}

impl TurnOutcome {
    /// `rawValue`.
    pub fn as_str(self) -> &'static str {
        match self {
            TurnOutcome::Completed => "completed",
            TurnOutcome::Interrupted => "interrupted",
            TurnOutcome::Failed => "failed",
        }
    }

    fn from_raw(raw: &str) -> TurnOutcome {
        match raw {
            "interrupted" => TurnOutcome::Interrupted,
            "failed" => TurnOutcome::Failed,
            // An outcome this app doesn't know reads as completed.
            _ => TurnOutcome::Completed,
        }
    }
}

/// Why a failed turn failed, when the runner knows, so the app can offer
/// the fix without reading the message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnErrorKind {
    UsageLimit,
    ContextWindow,
    Auth,
    Network,
}

impl TurnErrorKind {
    fn from_raw(raw: &str) -> Option<TurnErrorKind> {
        match raw {
            "usageLimit" => Some(TurnErrorKind::UsageLimit),
            "contextWindow" => Some(TurnErrorKind::ContextWindow),
            "auth" => Some(TurnErrorKind::Auth),
            "network" => Some(TurnErrorKind::Network),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TurnCompletion {
    pub turn_id: String,
    pub outcome: TurnOutcome,
    pub duration_ms: f64,
    /// None for Codex turns, which report no cost.
    pub cost_usd: Option<f64>,
    /// Token counts by name (`usage`), as sent.
    pub usage: Option<Value>,
    pub error: Option<String>,
    /// None when unknown, including a kind this app doesn't know yet.
    pub error_kind: Option<TurnErrorKind>,
    /// What the turn said it needs from Mike: "you" (with `asks`), "nothing",
    /// or None when it said neither.
    pub needs: Option<String>,
    pub asks: Vec<Ask>,
    /// With needs "nothing": follow-ups the agent suggested.
    pub next: Vec<String>,
}

impl TurnCompletion {
    fn decode(c: &Object) -> TurnCompletion {
        // `error` may be a string or an object depending on the runner version.
        let error = match c.get("error") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(other) => Some(other.get("message").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| compact_text(other))),
        };
        TurnCompletion {
            turn_id: string(c, "turnId").unwrap_or_default(),
            outcome: c.get("outcome").and_then(Value::as_str).map(TurnOutcome::from_raw).unwrap_or(TurnOutcome::Completed),
            duration_ms: number(c, "durationMs").unwrap_or(0.0),
            cost_usd: number(c, "costUsd"),
            usage: any(c, "usage"),
            error,
            error_kind: string(c, "errorKind").and_then(|k| TurnErrorKind::from_raw(&k)),
            needs: string(c, "needs"),
            asks: lossy(c, "asks", Ask::decode),
            next: lossy_strings(c, "next"),
        }
    }
}

/// A command action that ran (`ActionRun`): what it printed, outside any
/// turn. Shown as a tool row.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionRun {
    pub action_id: String,
    pub patch: String,
    pub title: String,
    pub command: String,
    pub ok: bool,
    pub exit_code: Option<i64>,
    pub text: String,
    pub truncated: bool,
    pub duration_ms: f64,
    /// "turn.end" when a hook ran it; None for a chip or tb.
    pub trigger: Option<String>,
}

impl ActionRun {
    fn decode(c: &Object) -> ActionRun {
        let action_id = string(c, "actionId").unwrap_or_default();
        ActionRun {
            patch: string(c, "patch").unwrap_or_default(),
            title: string(c, "title").unwrap_or_else(|| action_id.clone()),
            action_id,
            command: string(c, "command").unwrap_or_default(),
            ok: boolean(c, "ok").unwrap_or(true),
            exit_code: lenient_int(c, "exitCode"),
            text: string(c, "text").unwrap_or_default(),
            truncated: boolean(c, "truncated").unwrap_or(false),
            duration_ms: number(c, "durationMs").unwrap_or(0.0),
            trigger: string(c, "trigger"),
        }
    }

    /// The row's item id: one per run.
    pub fn item_id(seq: i64) -> String {
        format!("action:{seq}")
    }

    /// The run as the tool call and result the transcript draws.
    pub fn as_tool_call(&self) -> (ToolCall, ToolResult) {
        let mut title = if self.ok {
            self.title.clone()
        } else {
            format!("{} failed{}", self.title, self.exit_code.map(|code| format!(" (exit {code})")).unwrap_or_default())
        };
        // A hook's run says when it ran, as the design's patch line does.
        if self.trigger.as_deref() == Some("turn.end") {
            title.push_str(" · after the turn");
        }
        let call = ToolCall {
            item_id: String::new(),
            name: "action".to_owned(),
            kind: ToolKind::Command,
            title,
            input: json!({"command": self.command, "patch": self.patch, "actionId": self.action_id}),
        };
        let result = ToolResult { item_id: String::new(), ok: self.ok, text: self.text.clone(), truncated: self.truncated, ..ToolResult::default() };
        (call, result)
    }
}

/// A `thread.rewound` event: the range of seqs an edit cut, and what
/// happened to the files the agent had changed since (`ThreadRewind`).
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadRewind {
    pub from_seq: i64,
    pub to_seq: i64,
    /// Files put back as they were.
    pub undone: Vec<String>,
    pub failed: Vec<RewindFailedFile>,
    /// "checkpoint", "diffs" or "none".
    pub how: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RewindFailedFile {
    pub path: String,
    pub reason: String,
}

impl ThreadRewind {
    fn decode(c: &Object) -> ThreadRewind {
        let files = c.get("files").and_then(Value::as_object);
        let failed = |v: &Value| {
            let f = v.as_object()?;
            Some(RewindFailedFile { path: string(f, "path").unwrap_or_default(), reason: string(f, "reason").unwrap_or_default() })
        };
        ThreadRewind {
            from_seq: lenient_int(c, "fromSeq").unwrap_or(0),
            to_seq: lenient_int(c, "toSeq").unwrap_or(0),
            undone: files.map(|f| lossy_strings(f, "undone")).unwrap_or_default(),
            failed: files.map(|f| lossy(f, "failed", failed)).unwrap_or_default(),
            how: files.and_then(|f| string(f, "how")).unwrap_or_else(|| "none".to_owned()),
        }
    }

    /// The seqs it hides; None for a range that makes no sense.
    pub fn range(&self) -> Option<RangeInclusive<i64>> {
        (self.from_seq > 0 && self.to_seq >= self.from_seq).then_some(self.from_seq..=self.to_seq)
    }

    /// The quiet line where the cut was: that a message was edited, and what
    /// happened to the files.
    pub fn summary(&self) -> String {
        let mut parts = vec!["Edited a message, so the thread carries on from there.".to_owned()];
        if !self.undone.is_empty() {
            let files = if self.undone.len() == 1 { "1 file".to_owned() } else { format!("{} files", self.undone.len()) };
            parts.push(format!("Undid changes to {files}."));
        }
        if !self.failed.is_empty() {
            let named: Vec<String> = self
                .failed
                .iter()
                .take(3)
                .map(|file| {
                    let name = text::last_path_component(&file.path);
                    if file.reason.is_empty() {
                        name.to_owned()
                    } else {
                        format!("{name} ({})", file.reason)
                    }
                })
                .collect();
            let more = if self.failed.len() > 3 { format!(" and {} more", self.failed.len() - 3) } else { String::new() };
            parts.push(format!("Couldn't undo {}{more}.", named.join(", ")));
        }
        parts.join(" ")
    }
}

// ---- Events ----

#[derive(Clone, Debug, PartialEq)]
pub enum EventPayload {
    /// `origin` is "runner" for a message the runner sent by itself (the
    /// resume after a restart); `edit_of` is the seq of the message this one
    /// replaced; `review` is Mike's answer to a review, when the runner sent
    /// it for one.
    UserMessage { text: String, images: Vec<ImageRef>, origin: Option<String>, answers: Vec<AskAnswer>, edit_of: Option<i64>, review: Option<ReviewAnswer> },
    /// `summary`: the turn's summary, written by the runner from end_turn.
    /// `gives_way`: on a summary, with bullet replies off. `final_reply`:
    /// One reply's reply, the agent's message after end_turn.
    AssistantText { item_id: String, text: String, summary: bool, narration: bool, gives_way: bool, final_reply: bool },
    Reasoning { item_id: String, text: String },
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    EditDiff(EditDiff),
    ApprovalRequested(ApprovalRequest),
    QuestionRequested(QuestionRequest),
    SecretRequested(SecretRequest),
    ReviewUpdated(DesignReview),
    /// A reply button made, pressed or failed: no row of its own, but kept
    /// as sent for whatever draws the reply that links it.
    ButtonUpdated(Value),
    ApprovalResolved(ApprovalResolution),
    /// The calls queued on an approval card, the whole list each time.
    ApprovalQueued(ApprovalQueue),
    PlanUpdated(Vec<PlanEntry>),
    /// "Plan first": the agent's plan, waiting for Mike.
    PlanProposed { item_id: String, text: String },
    /// `go` approved, otherwise sent back.
    PlanResolved { item_id: String, go: bool },
    /// `woke_by`: what finished and started a turn the agent began by itself.
    TurnStarted { woke_by: Vec<BackgroundTask> },
    TurnCompleted(TurnCompletion),
    SystemNote(String),
    Error(String),
    ActionRun(ActionRun),
    /// The ticks on the card of asks that came with the turn.completed at
    /// `asks_seq`: the whole set each time.
    AsksTicked { asks_seq: i64, ticked: Vec<i64> },
    /// Mike edited the message at `from_seq`: every event from there to
    /// `to_seq` is gone from the conversation.
    ThreadRewound(ThreadRewind),
    AssistantTextDelta { item_id: String, text: String },
    ReasoningDelta { item_id: String, text: String },
    ToolOutputDelta { item_id: String, text: String },
    /// Everything streamed so far for an item, from a subscribe reply's
    /// `inProgress`. Made by the app, never sent as an event.
    StreamingText { item_id: String, is_reasoning: bool, text: String },
    /// An event type this app doesn't know yet, or one it couldn't read.
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub seq: i64,
    pub ts: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub type_: String,
    pub payload: EventPayload,
}

impl Event {
    pub fn new(seq: i64, ts: &str, thread_id: &str, turn_id: Option<&str>, type_: &str, payload: EventPayload) -> Event {
        Event { seq, ts: ts.to_owned(), thread_id: thread_id.to_owned(), turn_id: turn_id.map(str::to_owned), type_: type_.to_owned(), payload }
    }

    /// Transient events stream between persisted ones and never consume a
    /// seq. Judged by type too, so a delta that couldn't be read stays
    /// transient.
    pub fn is_transient(&self) -> bool {
        matches!(
            self.payload,
            EventPayload::AssistantTextDelta { .. }
                | EventPayload::ReasoningDelta { .. }
                | EventPayload::ToolOutputDelta { .. }
                | EventPayload::StreamingText { .. }
        ) || matches!(self.type_.as_str(), "assistant.text.delta" | "reasoning.delta" | "tool.output.delta")
    }

    /// `Event.init(from:)`. Never fails for a JSON object: a payload this
    /// app can't read becomes `Unknown` but keeps its seq, as dropping it
    /// would leave a hole in the seq order and the feed would ask for it
    /// again forever. Anything that isn't an object is None, as Swift's
    /// lossy arrays drop it.
    pub fn decode(v: &Value) -> Option<Event> {
        let c = v.as_object()?;
        let seq = lenient_int(c, "seq").unwrap_or(0);
        let ts = string(c, "ts").unwrap_or_default();
        let thread_id = string(c, "threadId").unwrap_or_default();
        let turn_id = string(c, "turnId");
        let type_ = string(c, "type").unwrap_or_default();
        let payload = decode_payload(&type_, c).unwrap_or(EventPayload::Unknown);
        Some(Event { seq, ts, thread_id, turn_id, type_, payload })
    }
}

/// Reads the fields of `user.message`, `assistant.text`, `reasoning`, the
/// deltas, `system.note` and `error`, tolerantly (`TextFields`).
struct TextFields {
    item_id: String,
    text: String,
    images: Vec<ImageRef>,
    message: Option<String>,
    origin: Option<String>,
    answers: Vec<AskAnswer>,
    edit_of: Option<i64>,
    review_id: Option<String>,
    review_response: Option<ReviewDraft>,
    summary: bool,
    narration: bool,
    gives_way: bool,
    final_reply: bool,
}

impl TextFields {
    fn decode(c: &Object) -> TextFields {
        TextFields {
            item_id: string(c, "itemId").unwrap_or_default(),
            text: string(c, "text").unwrap_or_default(),
            images: lossy(c, "images", ImageRef::decode),
            message: string(c, "message"),
            origin: string(c, "origin"),
            answers: lossy(c, "answers", AskAnswer::decode),
            edit_of: lenient_int(c, "editOf"),
            review_id: string(c, "reviewId"),
            review_response: c.get("reviewResponse").and_then(ReviewDraft::decode),
            summary: boolean(c, "summary").unwrap_or(false),
            narration: boolean(c, "narration").unwrap_or(false),
            gives_way: boolean(c, "givesWay").unwrap_or(false),
            final_reply: boolean(c, "final").unwrap_or(false),
        }
    }
}

/// Err is a value that made the Swift decoder throw, which the event then
/// reads as `Unknown` (`Event.init(from:)`).
fn decode_payload(type_: &str, c: &Object) -> Result<EventPayload, ()> {
    Ok(match type_ {
        "user.message" => {
            let f = TextFields::decode(c);
            let review = ReviewAnswer::parse(&f.text, f.review_id.as_deref(), f.review_response.as_ref());
            EventPayload::UserMessage { text: f.text, images: f.images, origin: f.origin, answers: f.answers, edit_of: f.edit_of, review }
        }
        "assistant.text" => {
            let f = TextFields::decode(c);
            EventPayload::AssistantText { item_id: f.item_id, text: f.text, summary: f.summary, narration: f.narration, gives_way: f.gives_way, final_reply: f.final_reply }
        }
        "reasoning" => {
            let f = TextFields::decode(c);
            EventPayload::Reasoning { item_id: f.item_id, text: f.text }
        }
        "tool.call" => EventPayload::ToolCall(ToolCall::decode(c)),
        "tool.result" => EventPayload::ToolResult(ToolResult::decode(c)),
        "edit.diff" => EventPayload::EditDiff(EditDiff::decode(c)),
        "approval.requested" => EventPayload::ApprovalRequested(ApprovalRequest::decode_object(c)),
        "question.requested" => EventPayload::QuestionRequested(QuestionRequest::decode(c)),
        "secret.requested" => EventPayload::SecretRequested(SecretRequest::decode(c)),
        "review.updated" => {
            let review = c.get("review").ok_or(())?;
            EventPayload::ReviewUpdated(DesignReview::decode(review).ok_or(())?)
        }
        // Swift decodes the whole button here and throws on one it can't
        // read; either way it makes no row, so the raw JSON is kept instead
        // of porting ReplyButton for no gain.
        "button.updated" => EventPayload::ButtonUpdated(c.get("button").ok_or(())?.clone()),
        "approval.resolved" => EventPayload::ApprovalResolved(ApprovalResolution::decode(c)),
        "approval.queued" => EventPayload::ApprovalQueued(ApprovalQueue::decode(c)),
        "plan.updated" => EventPayload::PlanUpdated(lossy(c, "entries", PlanEntry::decode)),
        "plan.proposed" => {
            // `PlanCardFields`: itemId is required and text's type is strict.
            let item_id = c.get("itemId").and_then(Value::as_str).ok_or(())?.to_owned();
            let text = string_strict(c, "text")?.unwrap_or_default();
            let _decision = string_strict(c, "decision")?;
            EventPayload::PlanProposed { item_id, text }
        }
        "plan.resolved" => {
            let item_id = c.get("itemId").and_then(Value::as_str).ok_or(())?.to_owned();
            let _text = string_strict(c, "text")?;
            let decision = string_strict(c, "decision")?;
            EventPayload::PlanResolved { item_id, go: decision.as_deref() == Some("go") }
        }
        "turn.started" => {
            // Not lossy, unlike most lists: one task that can't be read drops
            // the whole list.
            let woke_by = c
                .get("wokeBy")
                .and_then(Value::as_array)
                .and_then(|tasks| tasks.iter().map(BackgroundTask::decode).collect::<Option<Vec<_>>>())
                .unwrap_or_default();
            EventPayload::TurnStarted { woke_by }
        }
        "turn.completed" => EventPayload::TurnCompleted(TurnCompletion::decode(c)),
        "system.note" => EventPayload::SystemNote(TextFields::decode(c).text),
        "error" => {
            let f = TextFields::decode(c);
            EventPayload::Error(f.message.or_else(|| string(c, "text")).unwrap_or_else(|| "Something went wrong".to_owned()))
        }
        "action.run" => EventPayload::ActionRun(ActionRun::decode(c)),
        "thread.rewound" => EventPayload::ThreadRewound(ThreadRewind::decode(c)),
        "asks.ticked" => {
            // Synthesized Decodable: a required strict Int, and an optional
            // strict [Int], so one bad value makes the whole event unknown.
            let asks_seq = c.get("asksSeq").and_then(int_value).ok_or(())?;
            let ticked = match c.get("ticked") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(items)) => items.iter().map(int_value).collect::<Option<Vec<_>>>().ok_or(())?,
                Some(_) => return Err(()),
            };
            EventPayload::AsksTicked { asks_seq, ticked }
        }
        "assistant.text.delta" => {
            let f = TextFields::decode(c);
            EventPayload::AssistantTextDelta { item_id: f.item_id, text: f.text }
        }
        "reasoning.delta" => {
            let f = TextFields::decode(c);
            EventPayload::ReasoningDelta { item_id: f.item_id, text: f.text }
        }
        "tool.output.delta" => {
            let f = TextFields::decode(c);
            EventPayload::ToolOutputDelta { item_id: f.item_id, text: f.text }
        }
        _ => EventPayload::Unknown,
    })
}

/// What was streamed before the page was fetched (`EventsPage.InProgress`).
#[derive(Clone, Debug, PartialEq)]
pub struct InProgress {
    pub item_id: String,
    pub type_: String,
    pub text: String,
}

impl InProgress {
    pub fn new(item_id: &str, type_: &str, text: &str) -> InProgress {
        InProgress { item_id: item_id.to_owned(), type_: type_.to_owned(), text: text.to_owned() }
    }

    fn decode(v: &Value) -> Option<InProgress> {
        let c = v.as_object()?;
        Some(InProgress {
            item_id: c.get("itemId")?.as_str()?.to_owned(),
            type_: c.get("type")?.as_str()?.to_owned(),
            text: c.get("text")?.as_str()?.to_owned(),
        })
    }
}

/// Result of `threads.events` (`EventsPage`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct EventsPage {
    pub events: Vec<Event>,
    pub first_seq: i64,
    pub last_seq: i64,
    pub in_progress: Vec<InProgress>,
    /// Nothing comes before this page (`relay.atStart`).
    pub at_start: bool,
}

impl EventsPage {
    pub fn decode(v: &Value) -> Option<EventsPage> {
        let c = v.as_object()?;
        let events: Vec<Event> = lossy(c, "events", Event::decode);
        let first_seq = lenient_int(c, "firstSeq").or_else(|| events.first().map(|e| e.seq)).unwrap_or(0);
        let last_seq = lenient_int(c, "lastSeq").or_else(|| events.last().map(|e| e.seq)).unwrap_or(0);
        let at_start = c.get("relay").and_then(Value::as_object).and_then(|r| r.get("atStart")).and_then(Value::as_bool).unwrap_or(false);
        Some(EventsPage { events, first_seq, last_seq, in_progress: lossy(c, "inProgress", InProgress::decode), at_start })
    }
}

/// `JSONValue.compactText`: keys sorted, numbers as Swift's JSONEncoder
/// writes a Double (no ".0"), slashes unescaped.
pub fn compact_text(value: &Value) -> String {
    let mut out = String::new();
    write_compact(value, &mut out);
    out
}

fn write_compact(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&swift_number(n.as_f64().unwrap_or(0.0))),
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_compact(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_compact(&map[key.as_str()], out);
            }
            out.push('}');
        }
    }
}

// As serde_json would write it, keeping non-ASCII as it is. Swift escapes
// the same control characters.
fn write_string(s: &str, out: &mut String) {
    out.push_str(&serde_json::to_string(s).unwrap_or_default());
}

/// `Double.description` with its ".0" taken off, as JSONEncoder writes a
/// number. Whole numbers up to 2^53 print plainly; beyond that, and below
/// 1e-4, Swift goes scientific ("1e+16", "1e-05").
fn swift_number(value: f64) -> String {
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if !(-4..16).contains(&exponent) && value != 0.0 {
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{:02}", exponent.abs());
    }
    let plain = format!("{value}");
    plain.strip_suffix(".0").map(str::to_owned).unwrap_or(plain)
}
