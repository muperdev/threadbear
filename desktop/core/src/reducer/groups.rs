// Rows the model derives rather than reads from events: a port of
// Transcript/WorkGroups.swift and the words from Threads/BackgroundWork.swift.
use super::diff::DiffBuilder;
use super::event::{BackgroundKind, BackgroundTask, EditDiff, ToolKind};
use super::row::{Row, RowContent, ToolItem};
use super::text;
use super::tool_presentation::{PathDisplay, ToolPresentation};
use std::collections::HashSet;

/// A run of work inside one turn folded into one line (`WorkGroup`).
#[derive(Clone, Debug, PartialEq)]
pub struct WorkGroup {
    /// Row ids of the folded items, oldest first.
    pub member_ids: Vec<String>,
    /// "Read 3 files, searched twice, ran 4 commands".
    pub summary: String,
    /// Tool calls in the group that failed.
    pub failed_count: usize,
    pub is_expanded: bool,
    /// The last group of a turn that's still running.
    pub is_live: bool,
    /// While live, what's happening now ("Running npm test"), if anything is.
    pub current_action: Option<String>,
}

impl WorkGroup {
    pub fn new(member_ids: Vec<String>, summary: &str) -> WorkGroup {
        WorkGroup { member_ids, summary: summary.to_owned(), failed_count: 0, is_expanded: false, is_live: false, current_action: None }
    }

    pub(crate) fn from_members(members: &[&Row], is_expanded: bool, is_live: bool) -> WorkGroup {
        let (summary, failed_count) = WorkSummary::describe(members);
        WorkGroup {
            member_ids: members.iter().map(|m| m.id.clone()).collect(),
            summary,
            failed_count,
            is_expanded,
            is_live,
            current_action: if is_live { WorkSummary::current_action(members) } else { None },
        }
    }
}

/// The "Working for 12s" line at the end of a running turn (`WorkingStatus`).
/// The view counts the seconds itself, so the row doesn't change as they
/// pass.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkingStatus {
    pub turn_id: String,
    /// When the turn started (seconds since 1970), if the loaded events say.
    pub started_at: Option<f64>,
}

/// The line at the end of a thread whose turn is over but whose CLI still
/// has sub-agents or jobs running in the background (`BackgroundStatus`).
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundStatus {
    pub tasks: Vec<BackgroundTask>,
}

/// The "Edited 3 files" card at the end of a turn that changed files
/// (`ChangedFiles`).
#[derive(Clone, Debug, PartialEq)]
pub struct ChangedFiles {
    pub turn_id: String,
    pub files: Vec<ChangedFile>,
}

impl ChangedFiles {
    pub fn title(&self) -> String {
        if self.files.len() == 1 {
            "Edited 1 file".to_owned()
        } else {
            format!("Edited {} files", self.files.len())
        }
    }

    pub fn added(&self) -> i64 {
        self.files.iter().map(|f| f.added).sum()
    }

    pub fn removed(&self) -> i64 {
        self.files.iter().map(|f| f.removed).sum()
    }

    /// The key `TranscriptModel::set_expanded` takes to open one file's diff.
    pub fn expansion_key(turn_id: &str, path: &str) -> String {
        format!("cf:{turn_id}|{path}")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChangedFile {
    pub path: String,
    pub added: i64,
    pub removed: i64,
    /// Every successful edit to the file in the turn, in order.
    pub diffs: Vec<EditDiff>,
    /// Its diff is open under it in the card.
    pub is_expanded: bool,
    /// Its open diff shows every line, not just the first few.
    pub shows_all: bool,
    /// The thread's working folder.
    pub root: Option<String>,
}

impl ChangedFile {
    pub fn name(&self) -> String {
        text::last_path_component(&self.path).to_owned()
    }

    /// The file's folder inside the thread's folder ("Sources/Upload",
    /// empty at its top), or short for a file outside it.
    pub fn folder(&self) -> String {
        PathDisplay::folder_relative(&self.path, self.root.as_deref())
    }
}

/// How much of an opened diff shows (`DiffPreview`).
pub struct DiffPreview;

impl DiffPreview {
    pub const LINES: usize = 12;
    /// Hiding fewer lines than this isn't worth a click.
    pub const SLACK: usize = 6;

    /// How many of a diff's `total` display lines show while it's opened short.
    pub fn shown(total: usize) -> usize {
        if total <= Self::LINES + Self::SLACK {
            total
        } else {
            Self::LINES
        }
    }

    /// The expansion key that shows all of an opened diff: a tool row's id,
    /// or a changes card file's `ChangedFiles::expansion_key`, plus "|all".
    pub fn all_key(key: &str) -> String {
        format!("{key}|all")
    }

    /// Opens or closes `key` in a set of open keys. Closing also forgets
    /// "show all", so a diff opens short again next time.
    pub fn set(key: &str, open: bool, expanded: &mut HashSet<String>) {
        if open {
            expanded.insert(key.to_owned());
        } else {
            expanded.remove(key);
            expanded.remove(&Self::all_key(key));
        }
    }
}

/// Words for a work group's one-line summary (`WorkSummary`).
pub struct WorkSummary;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Category {
    Read,
    Search,
    Edit,
    Command,
    Web,
    Agent,
    Plan,
    ImageMade,
    ImageViewed,
    Tool,
}

impl WorkSummary {
    /// The summary and how many tools failed. Parts come in the order the
    /// work happened, so the line reads like what the agent did.
    pub fn describe(items: &[&Row]) -> (String, usize) {
        let mut order: Vec<Category> = Vec::new();
        let mut counts: Vec<(Category, i64)> = Vec::new();
        let mut read_paths: HashSet<String> = HashSet::new();
        let mut edit_paths: HashSet<String> = HashSet::new();
        let mut failed = 0usize;
        let mut thought = false;
        let mut first_note: Option<String> = None;
        let mut allowed = 0usize;

        fn count(order: &mut Vec<Category>, counts: &mut Vec<(Category, i64)>, category: Category, amount: i64) {
            if !counts.iter().any(|(c, _)| *c == category) {
                order.push(category);
                counts.push((category, 0));
            }
            if let Some(entry) = counts.iter_mut().find(|(c, _)| *c == category) {
                entry.1 += amount;
            }
        }

        for item in items {
            match &item.content {
                RowContent::Reasoning(_) => thought = true,
                RowContent::Note(text) => {
                    if first_note.is_none() {
                        first_note = Some(text.clone());
                    }
                }
                // The tool it let run is counted when it comes; the approval
                // only names the run while that hasn't arrived yet.
                RowContent::Approval(_) => allowed += 1,
                RowContent::Tool(tool) => {
                    if tool.failed() {
                        failed += 1;
                    }
                    let category = category_of(tool);
                    match category {
                        Category::Read => {
                            // The same file read twice is still one file.
                            let input = tool.call.as_ref().map(|c| &c.input);
                            let path = input
                                .and_then(|i| i.get("file_path").and_then(|v| v.as_str()).or_else(|| i.get("path").and_then(|v| v.as_str())))
                                .map(str::to_owned)
                                .unwrap_or_else(|| tool.item_id.clone());
                            let fresh = read_paths.insert(path);
                            count(&mut order, &mut counts, Category::Read, i64::from(fresh));
                        }
                        Category::Edit => {
                            // Not `effective_diffs`: that builds the whole
                            // diff, and a live group's summary is rebuilt on
                            // every streamed delta. A Codex patch counts each
                            // file it touches.
                            let paths = tool.edited_paths();
                            let paths = if paths.is_empty() { vec![tool.item_id.clone()] } else { paths };
                            let added: i64 = paths.into_iter().map(|p| i64::from(edit_paths.insert(p))).sum();
                            count(&mut order, &mut counts, Category::Edit, added);
                        }
                        Category::ImageMade => {
                            // One that failed made nothing; the red icon says so.
                            let made = if tool.failed() { 0 } else { tool.result.as_ref().map(|r| r.images.len() as i64).unwrap_or(1).max(1) };
                            count(&mut order, &mut counts, Category::ImageMade, made);
                        }
                        Category::ImageViewed => count(&mut order, &mut counts, Category::ImageViewed, 1),
                        other => count(&mut order, &mut counts, other, 1),
                    }
                }
                _ => {}
            }
        }

        let parts: Vec<String> = order
            .iter()
            .filter_map(|c| phrase(*c, counts.iter().find(|(k, _)| k == c).map(|(_, n)| *n).unwrap_or(0)))
            .collect();
        let mut summary = parts.join(", ");
        if summary.is_empty() {
            summary = if thought {
                "thought".to_owned()
            } else if let Some(note) = first_note {
                Self::first_line(&note)
            } else if allowed > 0 {
                if allowed == 1 {
                    "allowed a request".to_owned()
                } else {
                    format!("allowed {allowed} requests")
                }
            } else {
                "worked".to_owned()
            };
        }
        (capitalized_first(&summary), failed)
    }

    /// What a live group is doing right now: the newest tool still waiting
    /// for its result, or thinking that's still streaming. None between
    /// steps.
    pub fn current_action(items: &[&Row]) -> Option<String> {
        for item in items.iter().rev() {
            match &item.content {
                RowContent::Tool(tool) if !tool.is_finished() => return Some(ToolPresentation::active_title(tool)),
                RowContent::Reasoning(text) if text.is_streaming => return Some("Thinking".to_owned()),
                _ => continue,
            }
        }
        None
    }

    pub fn first_line(text: &str) -> String {
        let line = text.split(self::text::is_newline).find(|p| !p.is_empty()).unwrap_or(text);
        if self::text::count(line) > 120 {
            format!("{}…", self::text::prefix(line, 119))
        } else {
            line.to_owned()
        }
    }

    /// A thinking block's one-line summary: its first sentence, full stop
    /// and all, when that's a reasonable length. Cut wherever the column
    /// ended, a long first line could stop just after a full stop and read
    /// "confirmed.…".
    pub fn thought(text: &str) -> String {
        let mut line = text.split(self::text::is_newline).find(|p| !p.is_empty()).unwrap_or("").to_owned();
        if self::text::count(&line) > 200 {
            line = self::text::prefix(&line, 200);
        }
        let Some(stop) = line.find(". ") else { return line };
        let sentence = format!("{}.", &line[..stop]);
        if (20..=140).contains(&self::text::count(&sentence)) {
            sentence
        } else {
            line
        }
    }
}

fn category_of(tool: &ToolItem) -> Category {
    match tool.tool_kind() {
        // Codex's view_image is a read of a picture, not of a file's text.
        ToolKind::Read => {
            if tool.name() == "view_image" {
                Category::ImageViewed
            } else {
                Category::Read
            }
        }
        ToolKind::Search => Category::Search,
        ToolKind::Edit | ToolKind::Write => Category::Edit,
        ToolKind::Command => Category::Command,
        ToolKind::Web => {
            if tool.name().to_lowercase().contains("search") {
                Category::Search
            } else {
                Category::Web
            }
        }
        ToolKind::Task => Category::Agent,
        ToolKind::Todo => Category::Plan,
        ToolKind::Image => Category::ImageMade,
        ToolKind::Mcp | ToolKind::Other => {
            if tool.call.as_ref().map(DiffBuilder::is_edit_tool).unwrap_or(false) {
                Category::Edit
            } else {
                Category::Tool
            }
        }
    }
}

fn phrase(category: Category, n: i64) -> Option<String> {
    if n <= 0 {
        return None;
    }
    let plural = |one: &str, many: String| if n == 1 { one.to_owned() } else { many };
    Some(match category {
        Category::Read => plural("read a file", format!("read {n} files")),
        Category::Search => {
            if n == 1 {
                "searched once".to_owned()
            } else if n == 2 {
                "searched twice".to_owned()
            } else {
                format!("searched {n} times")
            }
        }
        Category::Edit => plural("edited a file", format!("edited {n} files")),
        Category::Command => plural("ran a command", format!("ran {n} commands")),
        Category::Web => plural("fetched a page", format!("fetched {n} pages")),
        Category::Agent => plural("ran an agent", format!("ran {n} agents")),
        Category::Plan => "updated the plan".to_owned(),
        Category::ImageMade => plural("made an image", format!("made {n} images")),
        Category::ImageViewed => plural("viewed an image", format!("viewed {n} images")),
        Category::Tool => plural("used a tool", format!("used {n} tools")),
    })
}

fn capitalized_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Words for a thread's background work (`BackgroundWork`).
pub struct BackgroundWork;

impl BackgroundWork {
    /// One line, for the transcript: "3 sub-agents working in the
    /// background", "1 job running in the background, 1 just finished",
    /// "2 sub-agents just finished, about to read their results".
    pub fn line(tasks: &[BackgroundTask]) -> String {
        let running: Vec<&BackgroundTask> = tasks.iter().filter(|t| !t.finished).collect();
        let finished = tasks.len() - running.len();
        if running.is_empty() {
            let kinds: HashSet<BackgroundKind> = tasks.iter().filter(|t| t.finished).map(|t| t.kind).collect();
            let kind = if kinds.len() == 1 { *kinds.iter().next().unwrap_or(&BackgroundKind::Shell) } else { BackgroundKind::Shell };
            let what = count_words(finished, kind);
            return format!("{what} just finished, about to read {}", if finished == 1 { "its result" } else { "their results" });
        }
        let parts: Vec<String> = [BackgroundKind::Agent, BackgroundKind::Shell, BackgroundKind::Monitor, BackgroundKind::Other]
            .iter()
            .filter_map(|kind| {
                let n = running.iter().filter(|t| t.kind == *kind).count();
                if n == 0 {
                    None
                } else {
                    Some(count_words(n, *kind))
                }
            })
            .collect();
        let verb = if parts.len() == 1 && running[0].kind == BackgroundKind::Agent { "working" } else { "running" };
        let mut line = format!("{} {verb} in the background", join(&parts));
        if finished > 0 {
            line.push_str(&format!(", {finished} just finished"));
        }
        line
    }

    /// The descriptions in one line after the summary, for the transcript.
    /// Descriptions are the CLI's, so a long one is cut.
    pub fn line_with_detail(tasks: &[BackgroundTask]) -> String {
        let names: Vec<String> = tasks
            .iter()
            .filter_map(|task| {
                if task.description.is_empty() {
                    return None;
                }
                let one_line = task.description.split(text::is_newline).find(|p| !p.is_empty()).unwrap_or(&task.description);
                let cut = if text::count(one_line) > 60 { format!("{}…", text::prefix(one_line, 59)) } else { one_line.to_owned() };
                Some(if task.finished { format!("{cut} (finished)") } else { cut })
            })
            .collect();
        if names.is_empty() {
            Self::line(tasks)
        } else {
            format!("{} · {}", Self::line(tasks), names.join(", "))
        }
    }
}

fn count_words(n: usize, kind: BackgroundKind) -> String {
    let (one, many) = match kind {
        BackgroundKind::Agent => ("sub-agent", "sub-agents"),
        BackgroundKind::Shell => ("job", "jobs"),
        BackgroundKind::Monitor => ("monitor", "monitors"),
        BackgroundKind::Other => ("task", "tasks"),
    };
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn join(parts: &[String]) -> String {
    match parts.len() {
        0 => String::new(),
        1 => parts[0].clone(),
        2 => format!("{} and {}", parts[0], parts[1]),
        _ => format!("{} and {}", parts[..parts.len() - 1].join(", "), parts[parts.len() - 1]),
    }
}
