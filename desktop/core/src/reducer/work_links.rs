// Finding the step a `work:` link quotes: the part of Transcript/WorkLinks.swift
// the reducer's `workStep` uses. The regular expressions in the Swift are
// written out by hand, as this crate has no regex dependency.
use super::row::{Row, RowContent};
use super::text;
use serde_json::Value;

pub struct WorkLinks;

impl WorkLinks {
    /// The item a quote means among a turn's items: the last whose command
    /// (or one of its `&&` parts) starts with the words, else the last whose
    /// command, description, path, pattern, title or text mentions them,
    /// else the last whose output does, else the last thought that does.
    pub fn step<'a>(quote: &str, items: impl IntoIterator<Item = &'a Row>) -> Option<&'a Row> {
        let q = normal(quote);
        if q.is_empty() {
            return None;
        }
        let mut best: Option<(i32, &'a Row)> = None;
        for row in items {
            let s = score(row, &q);
            if s > 0 && s >= best.map(|(b, _)| b).unwrap_or(0) {
                best = Some((s, row));
            }
        }
        best.map(|(_, row)| row)
    }
}

fn score(row: &Row, q: &str) -> i32 {
    match &row.content {
        RowContent::Tool(tool) => {
            let input = tool.call.as_ref().map(|c| &c.input);
            let command: Option<String> = input.and_then(|i| i.get("command")).and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Array(parts) => Some(parts.iter().filter_map(|p| p.as_str().map(str::to_owned)).collect::<Vec<_>>().join(" ")),
                _ => None,
            });
            if let Some(command) = &command {
                // Not inside quotes: `git commit -m "...; npm test passes"`
                // isn't a test run.
                let bare = strip_quoted(command);
                let parts = normal(&bare);
                if parts.split(['&', ';', '|']).any(|part| trim_cd(part).starts_with(q)) {
                    return 4;
                }
            }
            // The input's title is the step's own name where a tool takes
            // one: Codex's computer-use calls are all "cua_repl: js" as rows,
            // and "Reconnect to Chrome desktop" only there.
            let field = |key: &str| input.and_then(|i| i.get(key)).and_then(Value::as_str).map(str::to_owned);
            let named = [
                command,
                field("description"),
                field("file_path"),
                field("path"),
                field("pattern"),
                field("url"),
                field("query"),
                field("title"),
                tool.call.as_ref().map(|c| c.title.clone()),
            ];
            if named.iter().any(|n| n.as_ref().map(|s| normal(s).contains(q)).unwrap_or(false)) {
                return 3;
            }
            if tool.result.as_ref().map(|r| normal(&r.text).contains(q)).unwrap_or(false) {
                return 2;
            }
            0
        }
        RowContent::Assistant(item) if !item.is_summary => {
            if mentions(&item.text, q) {
                3
            } else {
                0
            }
        }
        // Last of all: an agent drafts its summary in its thinking just
        // before end_turn, quoting the words its links quote.
        RowContent::Reasoning(item) => {
            if mentions(&item.text, q) {
                1
            } else {
                0
            }
        }
        RowContent::Note(note) => {
            if normal(note).contains(q) {
                3
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// Whether the agent's own text mentions the words outside its work: links.
fn mentions(words: &str, q: &str) -> bool {
    normal(&strip_work_links(words)).contains(q)
}

// `text.replacing(/\]\(work:[^)\n]*(\))?/)`: a whole `](work:...)` becomes
// "]"; one with no `)` uses up its line and stays.
fn strip_work_links(words: &str) -> String {
    let mut out = String::with_capacity(words.len());
    let mut rest = words;
    while let Some(start) = rest.find("](work:") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 7..];
        let end = after.find([')', '\n']).unwrap_or(after.len());
        if after[end..].starts_with(')') {
            out.push(']');
            rest = &after[end + 1..];
        } else {
            out.push_str(&rest[start..start + 7 + end]);
            rest = &after[end..];
        }
    }
    out.push_str(rest);
    out
}

// `command.replacingOccurrences(of: #"'[^']*'|"[^"]*""#, with: "")`: removes
// quoted runs, each quote matched to the next of its kind.
fn strip_quoted(command: &str) -> String {
    let mut out = String::with_capacity(command.len());
    let mut rest = command;
    while let Some(c) = rest.chars().next() {
        if c == '\'' || c == '"' {
            if let Some(close) = rest[1..].find(c) {
                rest = &rest[close + 2..];
                continue;
            }
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// "cd app && npm test" starts with "npm test" too.
fn trim_cd(part: &str) -> String {
    let t = text::trim_whitespace(part);
    let Some(after) = t.strip_prefix("cd ") else { return t.to_owned() };
    match after.find(' ') {
        Some(space) => text::trim_whitespace(&after[space..]).to_owned(),
        None => t.to_owned(),
    }
}

/// Lowercased, quotes and backticks gone, runs of space as one
/// (`normal`, whose `\s+` is ICU's: tab, newline, form feed, return and the
/// Unicode separators).
pub(crate) fn normal(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut in_space = false;
    for c in lower.chars() {
        if matches!(c, '`' | '"' | '\'' | '*') {
            continue;
        }
        let space = matches!(c, '\t' | '\n' | '\u{000C}' | '\r')
            || matches!(c as u32, 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000);
        if space {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    text::trim_whitespace(&out).to_owned()
}
