// Words for approval and plan cards: a port of Transcript/CardText.swift.
use super::event::{ApprovalDecision, ApprovalRequest, PlanEntry, PlanStatus, QuestionRequest};
use super::text;
use super::tool_presentation::Field;
use serde_json::Value;

/// Words for approval cards, and for the one line an approval becomes once
/// Mike has answered it (`ApprovalText`).
pub struct ApprovalText;

impl ApprovalText {
    /// The card's question: "Run this command?", "Write release.sh?".
    pub fn title(request: &ApprovalRequest) -> String {
        let file = Self::file_path(request).map(|p| Self::shown(text::last_path_component(&p)));
        match request.name.as_str() {
            "Bash" => "Run this command?".to_owned(),
            "Edit" | "MultiEdit" | "NotebookEdit" => file.map(|f| format!("Edit {f}?")).unwrap_or_else(|| "Edit this file?".to_owned()),
            "Write" => file.map(|f| format!("Write {f}?")).unwrap_or_else(|| "Write this file?".to_owned()),
            "WebFetch" => "Fetch this page?".to_owned(),
            "WebSearch" => "Search the web?".to_owned(),
            _ => format!("Use {}?", Self::tool_name(&request.name)),
        }
    }

    /// What was asked for, for the line a settled approval becomes: the
    /// command's first line, "Write release.sh", the page, or the tool.
    pub fn target(request: &ApprovalRequest) -> String {
        let input = &request.input;
        match request.name.as_str() {
            "Bash" => {
                // Named before it's split, so a U+2028 can't make `ls` alone
                // look like the line that was allowed.
                if let Some(line) = input.get("command").and_then(Value::as_str).and_then(|c| Self::first_line(&Self::shown(c))) {
                    return line;
                }
            }
            "WebFetch" => {
                if let Some(url) = input.get("url").and_then(Value::as_str).filter(|u| !u.is_empty()) {
                    return format!("Fetch {}", Self::shown(url));
                }
            }
            "WebSearch" => {
                if let Some(query) = input.get("query").and_then(Value::as_str).filter(|q| !q.is_empty()) {
                    return format!("Search for {}", Self::shown(query));
                }
            }
            _ => {}
        }
        let mut title = Self::title(request);
        if title.ends_with('?') {
            title.pop();
        }
        title
    }

    /// Why Threadbear's own tool is asking, in the runner's words ("Switch
    /// this thread to Full access"), for under the card's title: the
    /// generic "Use update thread?" alone hid what Allow allowed (Daybreak,
    /// 9 Nov 2026, finding 67). The fields below it are still all of it.
    pub fn runner_reason(request: &ApprovalRequest) -> Option<String> {
        if !request.name.starts_with("mcp__threadbear__") {
            return None;
        }
        let title = request.title.trim();
        (!title.is_empty()).then(|| Self::shown(title))
    }

    /// The decision in a word or two.
    pub fn decision(decision: ApprovalDecision) -> &'static str {
        match decision {
            ApprovalDecision::Allow => "Allowed",
            ApprovalDecision::AllowAlways => "Always allowed",
            ApprovalDecision::Deny => "Denied",
            ApprovalDecision::Answered => "Answered",
            ApprovalDecision::Expired => "Expired when the runner restarted",
        }
    }

    /// `text` as an approval card shows it: every character that would
    /// break a line, hide, or reorder what's drawn is written out by name
    /// instead (`⟨U+2028⟩`), as the shell reads it as an ordinary character
    /// (security review, 9 Nov 2026). A line break and a tab are left.
    pub fn shown(text: &str) -> String {
        if !text.chars().any(Self::hides) {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len());
        for scalar in text.chars() {
            if Self::hides(scalar) {
                out.push_str(&format!("\u{27E8}U+{:04X}\u{27E9}", scalar as u32));
            } else {
                out.push(scalar);
            }
        }
        out
    }

    /// What a file card's diff doesn't draw, which Allow allows too: every
    /// argument but the path and the text the diff shows (`replace_all`, a
    /// notebook cell's id and kind, an edit's own flags), named as for a
    /// command (Codex review, 10 Nov 2026: `replace_all: true` and
    /// `false` drew the same card).
    pub fn fields_beyond_diff(input: &Value) -> Vec<Field> {
        const DRAWN: &[&str] = &["file_path", "notebook_path", "path", "content", "old_string", "new_string", "edits"];
        let mut out: Vec<Field> = super::tool_presentation::ToolPresentation::fields(input, true).into_iter().filter(|f| !DRAWN.contains(&f.key.as_str())).collect();
        if let Some(edits) = input.get("edits").and_then(Value::as_array) {
            for (i, edit) in edits.iter().enumerate() {
                let rest = match edit.as_object() {
                    Some(fields) => {
                        let rest: serde_json::Map<String, Value> = fields.iter().filter(|(k, _)| k.as_str() != "old_string" && k.as_str() != "new_string").map(|(k, v)| (k.clone(), v.clone())).collect();
                        (!rest.is_empty()).then(|| Value::Object(rest).to_string())
                    }
                    None => Some(edit.to_string()),
                };
                if let Some(rest) = rest {
                    out.push(Field { key: format!("edit {}", i + 1), value: rest });
                }
            }
        }
        out.extend(Self::line_break_notes(input));
        Self::shown_fields(&out)
    }

    /// Whether an edit's new text ends with a line break where its old text
    /// doesn't, or the other way round: a diff drawn by lines can't show it,
    /// and it moves the next line of the file (a commented-out `rm` onto a
    /// line of its own, Codex review, 10 Nov 2026).
    pub fn line_break_notes(input: &Value) -> Vec<Field> {
        fn note(key: String, edit: &Value) -> Option<Field> {
            let ends = |k: &str| edit.get(k).and_then(Value::as_str).map(|s| s.ends_with('\n'));
            match (ends("old_string"), ends("new_string")) {
                (Some(false), Some(true)) => Some(Field { key, value: "the new text adds a line break at its end, so what follows it starts a new line".to_owned() }),
                (Some(true), Some(false)) => Some(Field { key, value: "the new text drops the line break at its end, so what follows it joins its last line".to_owned() }),
                _ => None,
            }
        }
        let mut out: Vec<Field> = note("line break".to_owned(), input).into_iter().collect();
        // A whole file that doesn't end with one (a cron entry needs it).
        if input.get("content").and_then(Value::as_str).is_some_and(|c| !c.is_empty() && !c.ends_with('\n')) {
            out.push(Field { key: "line break".to_owned(), value: "the file doesn't end with a line break".to_owned() });
        }
        if let Some(edits) = input.get("edits").and_then(Value::as_array) {
            out.extend(edits.iter().enumerate().filter_map(|(i, e)| note(format!("edit {} line break", i + 1), e)));
        }
        out
    }

    /// `shown`, with the whitespace a diff or a command can hide made
    /// visible: a tab as `⇥` (it was drawn as four spaces, so swapping one
    /// for the other drew nothing, and a `<<-` heredoc ends on one), and
    /// spaces at a line's end as `·`. For what Allow runs or writes: a
    /// command and an edit's lines (Codex review, 10 Nov 2026).
    pub fn shown_exactly(text: &str) -> String {
        let named = Self::shown(text);
        let mut out = String::with_capacity(named.len() + 8);
        for line in named.split_inclusive('\n') {
            let (body, end) = line.strip_suffix('\n').map_or((line, ""), |b| (b, "\n"));
            let kept = body.trim_end_matches(' ');
            out.push_str(&kept.replace('\t', "\u{21E5}   "));
            out.extend(std::iter::repeat('\u{B7}').take(body.len() - kept.len()));
            out.push_str(end);
        }
        out
    }

    /// The same for a card's fields, keys and values.
    pub fn shown_fields(fields: &[Field]) -> Vec<Field> {
        fields.iter().map(|f| Field { key: Self::shown(&f.key), value: Self::shown(&f.value) }).collect()
    }

    fn hides(scalar: char) -> bool {
        if scalar == '\n' || scalar == '\t' {
            return false;
        }
        let v = scalar as u32;
        // Characters that draw as nothing though they're there, the braille
        // blank, spaces that aren't the shell's, right-to-left letters and
        // controls, format characters and line and paragraph separators.
        if is_default_ignorable(v) || v == 0x2800 {
            return true;
        }
        if is_space_separator(v) {
            return scalar != ' ';
        }
        if right_to_left(v) {
            return true;
        }
        is_control(v) || is_format(v) || v == 0x2028 || v == 0x2029
    }

    /// The file a request would write or edit, if it names one.
    pub fn file_path(request: &ApprovalRequest) -> Option<String> {
        let input = &request.input;
        let path = input
            .get("file_path")
            .and_then(Value::as_str)
            .or_else(|| input.get("notebook_path").and_then(Value::as_str))
            .or_else(|| input.get("path").and_then(Value::as_str))?;
        if path.is_empty() {
            None
        } else {
            Some(path.to_owned())
        }
    }

    /// The answers to a question card in one line, in the order the
    /// questions were asked.
    pub fn answer_summary(request: &QuestionRequest, answers: &std::collections::HashMap<String, String>) -> String {
        request
            .questions
            .iter()
            .filter_map(|q| answers.get(&q.question).filter(|a| !a.is_empty()).cloned())
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// "mcp__github__get_pull_request" is "github get_pull_request".
    pub fn tool_name(name: &str) -> String {
        let parts: Vec<&str> = name.split('_').filter(|p| !p.is_empty()).collect();
        if name.starts_with("mcp__") && parts.len() >= 3 {
            parts[2..].join(" ")
        } else {
            name.to_owned()
        }
    }

    fn first_line(s: &str) -> Option<String> {
        let line = s.split(text::is_newline).find(|p| !p.is_empty()).map(text::trim_whitespace).unwrap_or("");
        if line.is_empty() {
            return None;
        }
        Some(if text::count(line) > 160 { format!("{}…", text::prefix(line, 159)) } else { line.to_owned() })
    }
}

// The tables behind `hides`, from the Unicode Character Database. Swift reads
// these from its own copy of the database, so a new Unicode version can move
// them in Swift first; these were checked against Swift's answers for every
// scalar on 9 Nov 2026 (Swift's `hides` is private, so that was done through
// `shown`).
//
// Default_Ignorable_Code_Point.
fn is_default_ignorable(v: u32) -> bool {
    matches!(
        v,
        0x00AD
            | 0x034F
            | 0x061C
            | 0x115F..=0x1160
            | 0x17B4..=0x17B5
            | 0x180B..=0x180F
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x206F
            | 0x3164
            | 0xFE00..=0xFE0F
            | 0xFEFF
            | 0xFFA0
            | 0xFFF0..=0xFFF8
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0000..=0xE0FFF
    )
}

// General_Category Zs.
fn is_space_separator(v: u32) -> bool {
    matches!(v, 0x0020 | 0x00A0 | 0x1680 | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000)
}

// General_Category Cc.
fn is_control(v: u32) -> bool {
    matches!(v, 0x0000..=0x001F | 0x007F..=0x009F)
}

// General_Category Cf.
fn is_format(v: u32) -> bool {
    matches!(
        v,
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

// Hebrew, Arabic, Syriac, Thaana, N'Ko and the other right-to-left scripts'
// blocks, and their presentation forms.
fn right_to_left(v: u32) -> bool {
    (0x0590..=0x08FF).contains(&v) || (0xFB1D..=0xFDFF).contains(&v) || (0xFE70..=0xFEFF).contains(&v) || (0x10800..=0x10FFF).contains(&v) || (0x1E800..=0x1EFFF).contains(&v)
}

/// The plan card's title: how far along it is (`PlanSummary`).
pub struct PlanSummary;

impl PlanSummary {
    /// "2 of 5 steps done", or "All 3 steps done" at the end.
    pub fn title(entries: &[PlanEntry]) -> String {
        let total = entries.len();
        let done = entries.iter().filter(|e| e.status == PlanStatus::Completed).count();
        let steps = if total == 1 { "step" } else { "steps" };
        if total == 0 {
            return "No steps yet".to_owned();
        }
        if done == total {
            return if total == 1 { "1 step done".to_owned() } else { format!("All {total} steps done") };
        }
        format!("{done} of {total} {steps} done")
    }
}

/// The plan card's words, "Plan first" (`PlanCardText`).
pub struct PlanCardText;

impl PlanCardText {
    /// How many lines of a long plan the card shows before "Show full plan".
    pub const PREVIEW_LINES: usize = 12;

    /// A plan that starts with a heading: that heading, for the card's
    /// title, and the plan after it. Under a "Plan" title of its own the
    /// heading said Plan twice.
    pub fn split(plan: &str) -> (Option<String>, String) {
        let trimmed = text::trim_ws_nl(plan);
        // Any level: Codex starts its plans with "## Add bye.txt".
        let end = trimmed.find('\n').unwrap_or(trimmed.len());
        let first = &trimmed[..end];
        let hashes = first.chars().take_while(|c| *c == '#').count();
        if !(1..=3).contains(&hashes) || !first[hashes..].starts_with(' ') {
            return (None, trimmed.to_owned());
        }
        let title = text::trim_whitespace(&first[hashes..]);
        let body = text::trim_ws_nl(&trimmed[end..]).to_owned();
        if title.is_empty() {
            (None, trimmed.to_owned())
        } else {
            (Some(title.to_owned()), body)
        }
    }

    /// The start of a long plan, cut at a line, and whether anything was
    /// left out. A plan of up to a couple more lines than that shows whole.
    pub fn preview(plan: &str, limit: usize) -> (String, bool) {
        let trimmed = text::trim_ws_nl(plan);
        let lines: Vec<&str> = trimmed.split('\n').collect();
        if lines.len() <= limit + 2 {
            return (trimmed.to_owned(), false);
        }
        let mut kept: Vec<String> = lines[..limit].iter().map(|l| (*l).to_owned()).collect();
        // Don't end on a heading or a blank line: they read as cut off.
        while let Some(last) = kept.last() {
            if text::trim_whitespace(last).is_empty() || last.starts_with('#') {
                kept.pop();
            } else {
                break;
            }
        }
        // An open code fence would swallow the rest of the card.
        if kept.iter().filter(|l| text::trim_whitespace(l).starts_with("```")).count() % 2 == 1 {
            kept.push("```".to_owned());
        }
        (kept.join("\n"), true)
    }

    /// The plan's first line of prose, for a one-line mention: its
    /// heading's words if that's all there is.
    pub fn summary(plan: &str) -> String {
        let lines: Vec<&str> = plan.split('\n').filter(|l| !l.is_empty()).map(text::trim_whitespace).filter(|l| !l.is_empty()).collect();
        let prose = lines.iter().find(|l| !l.starts_with('#')).or_else(|| lines.first()).copied().unwrap_or("");
        let line = prose.trim_start_matches(['#', ' ']);
        if text::count(line) > 160 {
            format!("{}…", text::prefix(line, 159))
        } else {
            line.to_owned()
        }
    }
}
