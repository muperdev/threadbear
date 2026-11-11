// Reply text: MarkdownRenderer.swift's look in gpui-kit's TextView. The kit
// does the blocks; this sets its sizes, spacing and colours to the Swift
// app's, and sends links where they should go.
use super::metrics::CODE_PAD;
use crate::style::{self, size};
use crate::open_gate::{self, Origin};
use crate::thread::ThreadPane;
use threadbear_core::open_policy::{self, LinkKind};
use gpui_kit::base::{TextViewState, TextViewStyle};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::*;
// The zoom-aware px (style.rs), in place of GPUI's.
use crate::style::px;

/// How far a list's text sits in from the text around it (listIndent).
const LIST_INDENT: f32 = 30.;
/// The gap between paragraphs (paragraphGap).
const PARAGRAPH_GAP: f32 = 10.;

/// The reading column's style. Built once per appearance and shared: a new
/// `heading` closure per frame would make the kit see a different style and
/// lay every reply out again.
pub(crate) fn reading_style(cx: &App) -> TextViewStyle {
    let (label, muted, link, code_bg, inline_bg, border, hairline, mono) = (
        style::label_text(cx),
        style::secondary_label(cx),
        style::link_text(cx),
        style::code_background(cx),
        style::inline_code_background(cx),
        style::card_border(cx),
        style::hairline(cx),
        cx.theme().mono_font_family.clone(),
    );
    let rem = f32::from(cx.theme().font_size);
    let code_block = StyleRefinement::default()
        .rounded(px(7.))
        .border_0()
        .p(px(CODE_PAD))
        .text_size(px(size::CODE))
        .line_height(px(size::CODE_LINE))
        .font_family(mono.clone());
    // The pill behind inline code, with a little room either side (pillPad);
    // the kit reads padding and radius from this.
    let inline_code = StyleRefinement::default().bg(inline_bg).rounded(px(4.)).px(px(2.5)).py(px(1.)).text_size(px(size::CODE)).font_family(mono.clone()).text_color(label);
    // A rule under the header and hairlines between rows, none around them
    // (renderTable). No cell backgrounds.
    let table = StyleRefinement::default().border_0().rounded(px(0.));
    let table_head = StyleRefinement::default().text_size(px(size::LABEL)).font_weight(FontWeight::SEMIBOLD).text_color(muted).bg(gpui_kit::transparent_black()).border_color(border);
    // Transparent: the kit draws a column divider in the cell border colour, and
    // the rows carry the rules (renderTable has no vertical lines).
    let table_cell = StyleRefinement::default().py(px(4.)).px(px(12.)).border_color(gpui_kit::transparent_black());
    TextViewStyle::default()
        .with_foreground(label)
        .with_muted_foreground(muted)
        .with_link(link)
        .with_selection(cx.theme().selection)
        .with_code_background(code_bg)
        .with_border(hairline)
        .with_paragraph_gap(rems(PARAGRAPH_GAP / rem))
        .with_heading(move |level| {
            // Sizes and the 6 above / 6 below from MarkdownRenderer (the
            // paragraph before leaves 10 of the 16 above a heading).
            let (font, line) = match level {
                1 => (18., 26.),
                2 => (16., 23.),
                _ => (15., 21.),
            };
            StyleRefinement::default().text_size(px(font)).line_height(px(line)).font_weight(FontWeight::SEMIBOLD).pt(px(6.)).pb(px(6.))
        })
        .with_code_block(code_block)
        .with_inline_code(HighlightStyle { background_color: Some(inline_bg), ..Default::default() })
        .with_inline_code_style(inline_code)
        .with_list_style(StyleRefinement::default().text_color(label).min_w(px(LIST_INDENT)).pr(px(LIST_INDENT - 22.)))
        .with_blockquote_style(StyleRefinement::default().border_l(px(0.)).pl(px(14.)).text_color(muted))
        .with_table(table)
        .with_table_head(table_head)
        .with_table_cell(table_cell)
        .with_dark(cx.theme().is_dark())
}

/// `WorkLinks.escapeDestinations` and the "↑" after a work link: a space ends a
/// link's destination for the parser, and agents write `[tests pass](work:npm
/// test)`. The arrow says the link goes up the thread, not out of it, and goes
/// in the link's own words so it's coloured and selected with them.
pub(crate) fn prepare(text: &str) -> String {
    link_code_paths(&escape_work_links(text))
}

fn escape_work_links(text: &str) -> String {
    if !text.contains("](work:") && !text.contains("](thread:") {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    loop {
        let (start, is_work) = match (rest.find("](work:"), rest.find("](thread:")) {
            (Some(w), Some(t)) if w < t => (w, true),
            (_, Some(t)) => (t, false),
            (Some(w), None) => (w, true),
            (None, None) => {
                out.push_str(rest);
                return out;
            }
        };
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        // No `)` to end it: not a link, and the rest of its line is used up.
        let end = after.find([')', '\n']).unwrap_or(after.len());
        if after[end..].starts_with(')') {
            if is_work {
                out.push('\u{2009}');
                out.push('↑');
            }
            out.push_str("](");
            out.push_str(&after[..end].replace(' ', "%20"));
            out.push(')');
            rest = &after[end + 1..];
        } else {
            out.push_str("](");
            out.push_str(&after[..end]);
            rest = &after[end..];
        }
    }
}

/// `` `path` `` written in reply words: a code span that reads as a file path
/// (`FilePathRefs.looksLikePath`) becomes a link to it, kept looking like code.
/// Only single-backtick spans on one line outside fenced code count, as cmark
/// would find them; the kit draws the pill and the link colour.
fn link_code_paths(text: &str) -> String {
    if !text.contains('`') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 32);
    let mut in_fence = false;
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start_matches(' ');
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        link_spans_in_line(line, &mut out);
    }
    out
}

fn link_spans_in_line(line: &str, out: &mut String) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            // Copy up to the next backtick.
            let next = line[i..].find('`').map(|n| i + n).unwrap_or(line.len());
            out.push_str(&line[i..next]);
            i = next;
            continue;
        }
        // A run of backticks opens a span closed by a run of the same length.
        let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
        let search_from = i + run;
        let close = line[search_from..].find(&"`".repeat(run)).map(|n| search_from + n);
        let Some(close) = close.filter(|c| *c >= search_from) else {
            out.push_str(&line[i..i + run]);
            i += run;
            continue;
        };
        let end = close + run;
        let span = &line[i..end];
        let code = &line[search_from..close];
        // Already a link's words (`[` before it, `](` after it), or more than
        // one backtick around it: left as it is.
        let in_link = i > 0 && bytes[i - 1] == b'[' && line[end..].starts_with("](");
        if run == 1 && !in_link && looks_like_path(code) {
            out.push('[');
            out.push_str(span);
            out.push_str("](tbpath:?p=");
            out.push_str(&encode_component(code));
            out.push(')');
        } else {
            out.push_str(span);
        }
        i = end;
    }
}

/// `FilePathRefs.looksLikePath`: no scheme, and either rooted (`/`, `~/`, `./`,
/// `../`) or ending in a file name with an extension; folders need the leading
/// root or a slash in them; single spaces only where they can't be a command.
pub(crate) fn looks_like_path(text: &str) -> bool {
    if text.is_empty() || text.chars().count() >= 400 || text.contains("://") || text.contains('*') || text.contains('<') || text.contains('`') {
        return false;
    }
    let path = strip_line(split_query(text).0);
    if text.chars().any(char::is_whitespace) && !spaced_path_ok(path) {
        return false;
    }
    if path.starts_with('/') {
        return path.chars().count() > 1;
    }
    if path.starts_with("~/") || path.starts_with("./") || path.starts_with("../") {
        return true;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let good_ext = !ext.is_empty() && ext.chars().count() <= 10 && ext.chars().all(char::is_alphanumeric) && ext.chars().any(char::is_alphabetic) && name.chars().count() > ext.chars().count() + 1;
    if !good_ext {
        // `runner/src/` and the like: a folder inside the project.
        return path.ends_with('/') && path[..path.len() - 1].contains('/');
    }
    !(!path.contains('/') && DOMAIN_ENDINGS.contains(&ext.to_lowercase().as_str()))
}

/// Endings that are web addresses far more often than files.
const DOMAIN_ENDINGS: [&str; 9] = ["com", "org", "net", "io", "dev", "co", "ai", "au", "uk"];

/// `spacedPathOK`: single spaces only, after the first folder, no flags or
/// shell punctuation, and a spaced last part needs an extension.
fn spaced_path_ok(path: &str) -> bool {
    if !path.contains(' ') || path.chars().any(|c| c.is_whitespace() && c != ' ') || path.starts_with(' ') || path.ends_with(' ') || path.contains("  ") {
        return false;
    }
    if path.chars().any(|c| matches!(c, '|' | ';' | '$' | '>' | '"' | '\'')) || path.contains("&&") {
        return false;
    }
    let (Some(slash), Some(space)) = (path.find('/'), path.find(' ')) else { return false };
    if slash > space || path.split(' ').any(|w| w.starts_with('-')) {
        return false;
    }
    let last = if path.ends_with('/') { "" } else { path.rsplit('/').next().unwrap_or("") };
    !last.contains(' ') || last.rsplit_once('.').is_some()
}

/// `Foo.swift:12` or `Foo.swift:12:4` names the file, and a line in it.
fn strip_line(text: &str) -> &str {
    let mut path = text;
    for _ in 0..2 {
        match path.rsplit_once(':') {
            Some((head, tail)) if !head.is_empty() && !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => path = head,
            _ => break,
        }
    }
    path
}

/// A file and any `?query` or `#fragment` for its page, split at the first.
fn split_query(text: &str) -> (&str, Option<&str>) {
    match text.find(['?', '#']) {
        Some(mark) => (&text[..mark], Some(&text[mark..])),
        None => (text, None),
    }
}

/// Everything that isn't safe in a link's destination, percent-escaped.
fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `%20` and friends back to what the link's words were.
pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(a * 16 + b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

impl ThreadPane {
    /// The kit's document for a row, kept so scrolling a row away and back
    /// doesn't parse it again, and so a streaming reply only parses what's new
    /// (`set_text` appends when the text extends what's there).
    pub(crate) fn markdown_state(&mut self, key: &str, text: &str, cx: &mut Context<Self>) -> Entity<TextViewState> {
        let state = match self.rows.documents.get(key) {
            Some(state) => state.clone(),
            None => {
                let state = cx.new(|cx| TextViewState::markdown(text, cx));
                if self.rows.documents.len() >= DOCUMENTS_KEPT {
                    // Far more than a screen's worth; the oldest are the least
                    // likely to come back soon.
                    if let Some(oldest) = self.rows.document_order.first().cloned() {
                        self.rows.documents.remove(&oldest);
                        self.rows.document_order.remove(0);
                    }
                }
                self.rows.document_order.push(key.to_owned());
                self.rows.documents.insert(key.to_owned(), state.clone());
                state
            }
        };
        state.update(cx, |s, cx| s.set_text(text, cx));
        state
    }

    /// Where a link goes. `work:`, `thread:` and `review:` links act here;
    /// everything else goes through the open gate (open_gate.rs), which
    /// opens a web page, asks before a program or another app, and never
    /// opens another Mac's file here: that one shows in the pane, read
    /// through its runner.
    pub(crate) fn link_clicked(&mut self, url: &str, row_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let scheme = url.split_once(':').map(|(s, _)| s).filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))).map(str::to_ascii_lowercase);
        let rest = scheme.as_ref().map_or(url, |s| &url[s.len() + 1..]);
        match scheme.as_deref() {
            Some("work") => self.open_work_link(&percent_decode(rest), row_id, window, cx),
            Some("thread") => self.open_thread_link(&percent_decode(rest), window, cx),
            Some("review") => {}
            Some("tbpath") => {
                let text = percent_decode(rest.strip_prefix("?p=").unwrap_or(rest));
                self.open_thread_file(strip_line(split_query(&text).0), cx);
            }
            // `Foo.swift:12` reads as a scheme, but it's a file and a line.
            Some(s) if s.contains('.') && strip_line(url) != url => self.open_thread_file(strip_line(url), cx),
            _ => match open_policy::link_kind(url) {
                LinkKind::File(path) => self.open_thread_file(strip_line(&path), cx),
                _ => open_gate::open_link(url, self.origin(), &self.summary.cwd, cx),
            },
        }
    }

    pub(crate) fn origin(&self) -> Origin {
        if self.summary.is_remote() {
            Origin::OtherMac
        } else {
            Origin::ThisMac
        }
    }

    /// A file a reply names, by a link or in inline code: the thread's
    /// folder is where a relative one is from (`FilePathRefs.resolve`,
    /// without its search of the files the thread's tools touched, which
    /// comes with the file links' menus). This runner's opens in its app
    /// through the gate; another runner's shows in the pane, read through
    /// that runner, as nothing of it is on this Mac.
    pub(crate) fn open_thread_file(&mut self, path: &str, cx: &mut Context<Self>) {
        let home = threadbear_core::reducer::text::home_directory();
        let Some(absolute) = open_policy::absolute(path, &self.summary.cwd, &home) else { return };
        if self.summary.is_remote() {
            let side = self.composer.read(cx).app.upgrade().map(|app| app.read(cx).side.clone());
            let thread = self.summary.clone();
            if let Some(side) = side {
                side.update(cx, |side, cx| side.preview_path(&absolute, None, &thread, cx));
            }
            return;
        }
        open_gate::open_file(&absolute, Origin::ThisMac, cx);
    }
}

/// How many row documents a thread keeps parsed.
const DOCUMENTS_KEPT: usize = 600;

#[cfg(test)]
mod tests {
    use super::{looks_like_path, prepare};

    #[test]
    fn reads_paths_as_paths_and_commands_as_commands() {
        for path in ["app/Sources/Foo.swift", "Foo.swift:12", "/tmp/a.txt:12:4", "~/.zshrc", "./check.sh", "runner/src/", "guide.md#links"] {
            assert!(looks_like_path(path), "{path} should read as a path");
        }
        for not_path in ["npm test", "tb land", "example.com", "readme", "-rf", "a*b", "https://x.y/z", "open exports/a.mp4"] {
            assert!(!looks_like_path(not_path), "{not_path} shouldn't read as a path");
        }
        // Spaces only where they can't be a command.
        assert!(looks_like_path("exports/Threadbear Launch.mp4"));
        assert!(!looks_like_path("./check.sh --app"));
    }

    #[test]
    fn code_paths_become_links_outside_fences_and_links() {
        assert_eq!(prepare("see `src/a.rs` here"), "see [`src/a.rs`](tbpath:?p=src/a.rs) here");
        assert_eq!(prepare("`npm test` stays"), "`npm test` stays");
        // Already a link's words, or in a code block: left as they are.
        assert_eq!(prepare("[`src/a.rs`](https://x.y)"), "[`src/a.rs`](https://x.y)");
        assert_eq!(prepare("```\n`src/a.rs`\n```"), "```\n`src/a.rs`\n```");
        // Work links keep their words, gain the arrow and escape their spaces.
        assert_eq!(prepare("[tests](work:npm test)"), "[tests\u{2009}↑](work:npm%20test)");
    }
}
