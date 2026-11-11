// How a tool row reads: a port of Transcript/ToolPresentation.swift.
use super::event::{compact_text, ToolKind};
use super::format::int_checked;
use super::row::ToolItem;
use super::text;
use serde_json::{json, Value};

pub struct ToolPresentation;

impl ToolPresentation {
    /// `symbol(for:name:)`: an SF Symbol name, which the desktop app maps
    /// to a bundled icon.
    pub fn symbol(kind: ToolKind, name: &str) -> &'static str {
        match kind {
            // An action from a patch (ActionRun) gets the patch glyph.
            ToolKind::Command => {
                if name == "action" {
                    "square.dashed"
                } else {
                    "terminal"
                }
            }
            // Codex's view_image is looking at a picture, not reading a file.
            ToolKind::Read => {
                if name == "view_image" {
                    "photo"
                } else {
                    "doc.text"
                }
            }
            ToolKind::Edit => "pencil.line",
            ToolKind::Write => "square.and.pencil",
            ToolKind::Search => "magnifyingglass",
            ToolKind::Web => "globe",
            ToolKind::Mcp => "puzzlepiece.extension",
            ToolKind::Task => "person.2",
            ToolKind::Todo => "checklist",
            ToolKind::Image => "photo",
            ToolKind::Other => {
                if name.starts_with("mcp__") {
                    "puzzlepiece.extension"
                } else {
                    "wrench.and.screwdriver"
                }
            }
        }
    }

    /// The title on one line, home folder shortened to ~, capped in length.
    pub fn short_title(title: &str, limit: usize) -> String {
        let mut line = text::split_lf(title).into_iter().find(|p| !p.is_empty()).unwrap_or(title).to_owned();
        let home = text::home_directory();
        if !home.is_empty() && line.contains(&home) {
            line = line.replace(&home, "~");
        }
        if text::count(&line) > limit {
            line = format!("{}…", text::prefix(&line, limit - 1));
        }
        line
    }

    /// For Read, Edit and Write calls with a file path; None for anything else.
    pub fn file_title(tool: &ToolItem) -> Option<FileTitle> {
        let verb = match tool.tool_kind() {
            ToolKind::Read => {
                if tool.name() == "view_image" {
                    "View"
                } else {
                    "Read"
                }
            }
            ToolKind::Edit => "Edit",
            ToolKind::Write => "Write",
            _ => return None,
        };
        let input = &tool.call.as_ref()?.input;
        let path = field(input, "file_path").or_else(|| field(input, "notebook_path")).or_else(|| field(input, "path"))?;
        if path.is_empty() {
            return None;
        }
        let name = text::last_path_component(path).to_owned();
        if name.is_empty() {
            return None;
        }
        Some(FileTitle {
            verb: verb.to_owned(),
            name,
            folder: PathDisplay::folder_relative(path, tool.root.as_deref()),
            path: Self::short_title(path, 400),
        })
    }

    /// The tool output preview shown when a tool row is expanded.
    pub fn output_preview(tool: &ToolItem, max_lines: usize) -> String {
        let source = tool.result.as_ref().map(|r| r.text.as_str()).unwrap_or(&tool.live_output);
        let mut lines = 0;
        let mut end = 0;
        let bytes = source.as_bytes();
        for (i, b) in bytes.iter().enumerate() {
            // Not a "\r\n": Swift's text[index] == "\n" doesn't see those.
            if *b == b'\n' && (i == 0 || bytes[i - 1] != b'\r') {
                lines += 1;
                if lines >= max_lines {
                    end = i;
                    break;
                }
            }
        }
        if lines >= max_lines {
            return format!("{}\n…", &source[..end]);
        }
        source.to_owned()
    }

    /// The title for a tool that's running now: a bare command reads
    /// "Running npm test"; a described one keeps its description.
    pub fn active_title(tool: &ToolItem) -> String {
        let title = Self::short_title(&tool.title(), 80);
        if tool.tool_kind() == ToolKind::Command {
            if let Some(command) = tool.call.as_ref().and_then(|c| field(&c.input, "command")) {
                if Self::short_title(command, 80) == title {
                    return format!("Running {title}");
                }
            }
        }
        title
    }

    /// What an expanded tool row shows, by kind of tool.
    pub fn detail(tool: &ToolItem, max_lines: usize) -> Detail {
        let preview = Self::output_preview(tool, max_lines);
        let output = text::trim_newlines(&preview).to_owned();
        let empty = json!({});
        let input = tool.call.as_ref().map(|c| &c.input).unwrap_or(&empty);
        match tool.tool_kind() {
            ToolKind::Command => {
                if let Some(command) = field(input, "command") {
                    return Detail::Command { command: command.to_owned(), output };
                }
            }
            ToolKind::Read => {
                let path = field(input, "file_path").or_else(|| field(input, "path"));
                if let (Some(path), true) = (path, tool.result.as_ref().map(|r| r.ok).unwrap_or(true)) {
                    // The model writes the offset: never trust it to fit.
                    let offset = input.get("offset").and_then(Value::as_f64).and_then(int_checked).map(|o| o.max(1).min(1_000_000_000)).unwrap_or(1);
                    return Detail::File { path: Self::short_title(path, 400), lines: Self::numbered_lines(&output, offset) };
                }
            }
            ToolKind::Search => {
                if let Some(query) = field(input, "pattern").or_else(|| field(input, "query")) {
                    let scope = field(input, "path").or_else(|| field(input, "glob")).map(|s| Self::short_title(s, 200));
                    return Detail::Search { query: query.to_owned(), scope, output };
                }
            }
            _ => {}
        }
        Detail::Fields { fields: Self::fields(input, false), output }
    }

    /// The input as `key: value` lines: strings as they are, other values as
    /// compact JSON, long values cut to a few lines unless `whole` (an
    /// approval card, which shows all of what Allow allows).
    pub fn fields(input: &Value, whole: bool) -> Vec<Field> {
        let Some(object) = input.as_object() else {
            let compact = compact_text(input);
            return if compact.is_empty() || compact == "null" { Vec::new() } else { vec![Field::new("input", &compact)] };
        };
        let mut keys: Vec<&String> = object.keys().collect();
        keys.sort();
        let mut out = Vec::new();
        for key in keys {
            let value = &object[key.as_str()];
            let mut shown = match value {
                Value::String(s) => s.clone(),
                Value::Null => continue,
                other => compact_text(other),
            };
            if whole {
                out.push(Field::new(key, &shown));
                continue;
            }
            let lines = text::split_lf(&shown);
            if lines.len() > 6 {
                shown = format!("{}\n…", lines[..6].join("\n"));
            }
            if text::count(&shown) > 600 {
                shown = format!("{}…", text::prefix(&shown, 599));
            }
            out.push(Field::new(key, &shown));
        }
        out
    }

    /// The widest "key:" in `fields`, in characters, capped at `cap`.
    pub fn key_width(fields: &[Field], cap: usize) -> usize {
        cap.min(fields.iter().map(|f| text::count(&f.key) + 1).max().unwrap_or(0))
    }

    /// A line of a failed tool's output that says what went wrong
    /// ("error: ...", "fatal: ...").
    pub fn is_error_line(line: &str) -> bool {
        let trimmed = line.trim_start_matches([' ', '\t']);
        for word in ["error", "fatal"] {
            if text::count(trimmed) <= word.len() || text::prefix(trimmed, word.len()).to_lowercase() != word {
                continue;
            }
            let next = trimmed.chars().nth(word.len());
            if matches!(next, Some(':' | ' ' | '[')) {
                return true;
            }
        }
        false
    }

    /// File content with line numbers. Claude's Read results already number
    /// their lines ("    12→text" or "12<tab>text"); those numbers are
    /// used. Otherwise lines are numbered from `start`.
    pub fn numbered_lines(content: &str, start: i64) -> Vec<NumberedLine> {
        if content.is_empty() {
            return Vec::new();
        }
        let raw = text::split_lf(content);
        let mut parsed = Vec::with_capacity(raw.len());
        let mut all_numbered = true;
        for line in &raw {
            if let Some(numbered) = Self::parse_numbered(line) {
                parsed.push(numbered);
            } else if line.starts_with('…') || line.is_empty() {
                parsed.push(NumberedLine { number: None, text: (*line).to_owned() });
            } else {
                all_numbered = false;
                break;
            }
        }
        if all_numbered && parsed.iter().any(|l| l.number.is_some()) {
            return parsed;
        }
        raw.iter()
            .enumerate()
            .map(|(index, line)| {
                if *line == "…" {
                    NumberedLine { number: None, text: "…".to_owned() }
                } else {
                    NumberedLine { number: Some(start + index as i64), text: (*line).to_owned() }
                }
            })
            .collect()
    }

    fn parse_numbered(line: &str) -> Option<NumberedLine> {
        let trimmed = line.trim_start_matches(' ');
        let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() || digits.len() > 7 {
            return None;
        }
        let number = digits.parse::<i64>().ok()?;
        let rest = &trimmed[digits.len()..];
        let mut chars = rest.chars();
        let separator = chars.next()?;
        if separator != '→' && separator != '\t' {
            return None;
        }
        Some(NumberedLine { number: Some(number), text: chars.as_str().to_owned() })
    }
}

fn field<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str)
}

/// A file tool's title in parts: "Read" "UploadTask.swift" and its folder
/// (`ToolPresentation.FileTitle`).
#[derive(Clone, Debug, PartialEq)]
pub struct FileTitle {
    pub verb: String,
    pub name: String,
    /// The folder inside the thread's folder, empty for a file at its top;
    /// short for a file outside it.
    pub folder: String,
    /// The whole path with the home folder as ~, for the tooltip.
    pub path: String,
}

/// What an expanded tool row shows (`ToolPresentation.Detail`).
#[derive(Clone, Debug, PartialEq)]
pub enum Detail {
    Command { command: String, output: String },
    File { path: String, lines: Vec<NumberedLine> },
    Search { query: String, scope: Option<String>, output: String },
    Fields { fields: Vec<Field>, output: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub key: String,
    pub value: String,
}

impl Field {
    pub fn new(key: &str, value: &str) -> Field {
        Field { key: key.to_owned(), value: value.to_owned() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NumberedLine {
    pub number: Option<i64>,
    pub text: String,
}

/// Short forms of paths for the transcript (`PathDisplay`).
pub struct PathDisplay;

impl PathDisplay {
    /// `folder(of:relativeTo:)`: a file's folder as seen from the thread's
    /// folder `root`: "Sources/Upload" for a file in there, empty for one
    /// at its top. Outside it (or with no root), as `folder`.
    pub fn folder_relative(path: &str, root: Option<&str>) -> String {
        let path = &text::forward_slashes(path);
        if let Some(relative) = root.and_then(|r| Self::relative_path(path, r)) {
            return text::deleting_last_path_component(&relative);
        }
        Self::folder(path)
    }

    /// `path` inside `root` without the root ("scripts/release.sh"), or None
    /// when it isn't inside it.
    pub fn relative_path(path: &str, root: &str) -> Option<String> {
        let path = &text::forward_slashes(path);
        let mut base = text::expanding_tilde(&text::forward_slashes(root));
        while text::count(&base) > 1 && base.ends_with('/') {
            base.pop();
        }
        if base.is_empty() || base == "/" {
            return None;
        }
        let full = text::expanding_tilde(path);
        let inside = format!("{base}/");
        if full.starts_with(&inside) && full.len() > inside.len() {
            Some(full[inside.len()..].to_owned())
        } else {
            None
        }
    }

    /// A file's folder, short: its last two components ("Sources/Upload"),
    /// "~" for something right in the home folder, empty at the root.
    pub fn folder(path: &str) -> String {
        let path = &text::forward_slashes(path);
        let parent = text::deleting_last_path_component(path);
        if parent.is_empty() || parent == "/" {
            return String::new();
        }
        if parent == text::home_directory() || parent == "~" {
            return "~".to_owned();
        }
        let parts: Vec<&str> = parent.split('/').filter(|p| !p.is_empty()).collect();
        parts[parts.len().saturating_sub(2)..].join("/")
    }
}

#[cfg(test)]
mod windows_path_display_tests {
    use super::*;

    #[test]
    fn a_windows_path_is_named_relative_to_its_thread_the_same_way() {
        assert_eq!(PathDisplay::folder_relative(r"C:\work\site\mockups\home.html", Some(r"C:\work\site")), "mockups");
        assert_eq!(PathDisplay::relative_path(r"C:\work\site\src\components\Hero.astro", r"C:\work\site").as_deref(), Some("src/components/Hero.astro"));
    }
}
