// "Copy message" and "Copy transcript as Markdown": a port of
// Transcript/MarkdownExport.swift, and the row's context menu.
use super::metrics::*;
use crate::thread::ThreadPane;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;
use threadbear_core::reducer::{cost_text, duration, ApprovalDecision, DiffHunk, Event, EventsPage, PlanCardState, PlanStatus, Row, RowContent, RowKind, TranscriptModel, TurnOutcome};

/// The row's own content, for "Copy message". None for rows with nothing to
/// copy (the working line, the background line).
pub(crate) fn message_text(row: &Row) -> Option<String> {
    Some(match &row.content {
        RowContent::User(message) => {
            let mut parts = vec![message.text.clone()];
            parts.extend(message.images.iter().map(|i| format!("![image]({})", i.path)));
            parts.join("\n\n")
        }
        RowContent::Assistant(item) => item.text.clone(),
        RowContent::Reasoning(item) => item.text.split('\n').map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n"),
        RowContent::Tool(tool) => {
            let mut text = format!("**{}**: `{}`", tool.name(), tool.title().replace('`', "'"));
            let diffs: Vec<_> = tool.effective_diffs().into_iter().filter(|d| !d.hunks.is_empty()).collect();
            if !diffs.is_empty() {
                let several = diffs.len() > 1;
                for diff in &diffs {
                    let body = diff.hunks.iter().map(hunk_text).collect::<Vec<_>>().join("\n");
                    let header = if several && !diff.path.is_empty() { format!("--- {}\n", diff.path) } else { String::new() };
                    text.push_str(&format!("\n\n```diff\n{header}{body}\n```"));
                }
            } else if let Some(result) = tool.result.as_ref().filter(|r| !r.text.is_empty()) {
                text.push_str(&format!("\n\n{}", fenced(&result.text)));
            }
            text
        }
        RowContent::Images { images, .. } => images.iter().map(|i| format!("![image]({})", i.path)).collect::<Vec<_>>().join("\n"),
        RowContent::Approval(item) => {
            let decision = item.resolution.as_ref().map(|r| format!(" ({})", decision_label(r.decision))).unwrap_or_default();
            format!("**Approval**: {}{decision}", item.request.title)
        }
        RowContent::Question(item) => item.request.questions.iter().map(|q| format!("**Question**: {}", q.question)).collect::<Vec<_>>().join("\n"),
        // The card's text only: the app never has the value.
        RowContent::Secret(item) => format!("**Asked for a secret**: {}", item.resolved_text().unwrap_or_else(|| item.request.label.clone())),
        RowContent::Review(review) => {
            let mut lines = vec![format!("**Design review**: {} ({})", review.title, review.status)];
            lines.extend(review.note.clone());
            let response = review.response.as_ref().unwrap_or(&review.draft);
            for option in &review.options {
                let mut line = format!("- {}. {}", option.id, option.title);
                if response.picked_ids.contains(&option.id) {
                    line.push_str(" (picked)");
                }
                if option.reference {
                    line.push_str(" (reference)");
                }
                if option.shots.len() > 1 {
                    line.push_str(&format!(" ({} shots)", option.shots.len()));
                }
                if let Some(note) = response.notes.get(&option.id).filter(|n| !n.is_empty()) {
                    line.push_str(&format!(": {note}"));
                }
                lines.push(line);
            }
            if response.none {
                lines.push("None of them".to_owned());
            }
            lines.extend(response.reply.clone().filter(|r| !r.is_empty()));
            lines.join("\n")
        }
        RowContent::PlanCard(card) => {
            let state = match card.state {
                PlanCardState::Waiting => "",
                PlanCardState::Approved => " (approved)",
                PlanCardState::SentBack => " (sent back)",
            };
            format!("**Plan**{state}\n\n{}", card.text)
        }
        RowContent::Plan(entries) => entries.iter().map(|e| format!("- {} {}", if e.status == PlanStatus::Completed { "[x]" } else { "[ ]" }, e.text)).collect::<Vec<_>>().join("\n"),
        // The asks card's export comes with the composer card (Decisions).
        RowContent::Asks(_) => return None,
        RowContent::Footer(completion) => {
            let mut text = format!("_{}", duration(completion.duration_ms));
            if let Some(cost) = cost_text(completion.cost_usd) {
                text.push_str(&format!(", {cost}"));
            }
            if completion.outcome != TurnOutcome::Completed {
                text.push_str(&format!(", {}", completion.outcome.as_str()));
            }
            format!("{text}_")
        }
        RowContent::Note(text) => format!("_{text}_"),
        RowContent::Error { message, .. } => format!("**Error**: {message}"),
        RowContent::Group(group) => format!("_{}_", group.summary),
        RowContent::Working(_) | RowContent::Background(_) => return None,
        RowContent::Changes(card) => {
            let mut lines = vec![format!("{}:", card.title())];
            lines.extend(card.files.iter().map(|f| format!("- `{}` +{} −{}", f.path, f.added, f.removed)));
            lines.join("\n")
        }
        RowContent::Gap(gap) => format!("_{}_", gap.label()),
    })
}

fn hunk_text(hunk: &DiffHunk) -> String {
    let mut lines = vec![format!("@@ -{},{} +{},{} @@", hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines)];
    lines.extend(hunk.lines.iter().cloned());
    lines.join("\n")
}

/// What a row's menu does (`menu(forRow:)`), so the real menu and the
/// automation `menu` command run the same thing.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum RowMenuAction {
    CopyMessage,
    Toggle,
    CopyTranscript,
    CopyDebugInfo,
}

/// The items of a row's menu, in order.
pub(crate) fn row_menu(row: &Row, open: bool) -> Vec<(&'static str, RowMenuAction)> {
    let mut items = vec![];
    if message_text(row).is_some() {
        items.push(("Copy message", RowMenuAction::CopyMessage));
        if matches!(row.kind(), RowKind::Tool | RowKind::Reasoning | RowKind::Diff | RowKind::Group) {
            items.push((if open { "Collapse" } else { "Expand" }, RowMenuAction::Toggle));
        }
    }
    items.push(("Copy transcript as Markdown", RowMenuAction::CopyTranscript));
    items.push(("Copy debug info", RowMenuAction::CopyDebugInfo));
    items
}

/// A fence longer than any backtick run inside the text.
fn fenced(text: &str) -> String {
    let (mut longest, mut run) = (0, 0);
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat(3.max(longest + 1));
    format!("{fence}\n{text}\n{fence}")
}

fn decision_label(decision: ApprovalDecision) -> &'static str {
    match decision {
        ApprovalDecision::Allow => "allowed",
        ApprovalDecision::AllowAlways => "always allowed",
        ApprovalDecision::Deny => "denied",
        ApprovalDecision::Answered => "answered",
        ApprovalDecision::Expired => "expired",
    }
}

/// `MarkdownExport.transcript` over a whole thread, as `section(for:)` reads it.
fn transcript_text(title: &str, rows: &[Row]) -> String {
    let mut parts = vec![];
    if !title.is_empty() {
        parts.push(format!("# {title}"));
    }
    for row in rows {
        let section = match &row.content {
            RowContent::User(message) => {
                let speaker = if message.is_from_runner() {
                    "Runner"
                } else if message.is_from_schedule() {
                    "Schedule"
                } else {
                    "You"
                };
                let mut text = format!("## {speaker}\n\n{}", message.text);
                for image in &message.images {
                    text.push_str(&format!("\n\n![image]({})", image.path));
                }
                Some(text)
            }
            RowContent::Assistant(item) => Some(format!("## Claude\n\n{}", item.text)),
            _ => message_text(row),
        };
        parts.extend(section);
    }
    format!("{}\n", parts.join("\n\n"))
}

impl ThreadPane {
    /// `menu(forRow:)`: Copy message, Expand or Collapse, Copy transcript as
    /// Markdown, and what an agent needs to find this row again.
    pub(crate) fn row_context_menu(row: Row, menu: PopupMenu, pane: WeakEntity<ThreadPane>) -> PopupMenu {
        let open = matches!(&row.content, RowContent::Group(g) if g.is_expanded);
        let mut menu = menu.min_w(px(CARD_PAD * 16.));
        for (title, action) in row_menu(&row, open) {
            if action == RowMenuAction::CopyDebugInfo {
                menu = menu.separator();
            }
            let (row, pane) = (row.clone(), pane.clone());
            menu = menu.item(PopupMenuItem::new(title).on_click(move |_, _, cx| {
                let row = row.clone();
                let _ = pane.update(cx, |this, cx| this.run_row_menu(action, &row, cx));
            }));
        }
        menu
    }

    /// What a choice from the row's menu does.
    pub(crate) fn run_row_menu(&mut self, action: RowMenuAction, row: &Row, cx: &mut Context<Self>) {
        match action {
            RowMenuAction::CopyMessage => {
                if let Some(text) = message_text(row) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            RowMenuAction::Toggle => self.toggle_row(row, cx),
            RowMenuAction::CopyTranscript => self.copy_transcript(cx),
            RowMenuAction::CopyDebugInfo => {
                let info = self.debug_info(row);
                cx.write_to_clipboard(ClipboardItem::new_string(info));
            }
        }
    }

    /// `menu`: lists a row's menu, or (with `click`) runs an item of it as
    /// choosing it would. Copies land on the clipboard, read back with
    /// `clipboard`.
    #[cfg(feature = "automation")]
    pub fn menu_command(&mut self, row: &str, click: Option<&str>, cx: &mut Context<Self>) -> Result<String, String> {
        let Some(found) = self.feed.model().rows().iter().find(|r| r.id.starts_with(row)).cloned() else { return Err(format!("no row starting {row:?}")) };
        let open = self.row_open(&found);
        let items = row_menu(&found, open);
        let Some(title) = click else {
            return Ok(format!("ok {}", serde_json::to_string(&items.iter().map(|(t, _)| *t).collect::<Vec<_>>()).unwrap_or_default()));
        };
        let Some((_, action)) = items.iter().find(|(t, _)| *t == title).copied() else { return Err(format!("no item {title:?}")) };
        self.run_row_menu(action, &found, cx);
        Ok("ok".to_owned())
    }

    /// What "Copy debug info" says: enough to find the row and the events
    /// behind it (`RowDebug.info`).
    pub(crate) fn debug_info(&self, row: &Row) -> String {
        let mut lines = vec![format!("thread: {}", self.summary.id), format!("row: {}", row.id), format!("kind: {}", row.kind().as_str())];
        if let Some(turn) = row.turn_id.as_deref().filter(|t| !t.is_empty()) {
            lines.push(format!("turn: {turn}"));
        }
        lines.push(format!("first seq: {}", row.seq));
        lines.join("\n")
    }

    /// `copyTranscript`: fetches any older pages first, then copies the whole
    /// thread as Markdown, nothing folded left out.
    pub(crate) fn copy_transcript(&mut self, cx: &mut Context<Self>) {
        let (client, id, title, root) = (self.client_handle(), self.summary.id.clone(), self.summary.title.clone(), self.summary.cwd.clone());
        let mut events: Vec<Event> = self.feed.model().events().to_vec();
        let mut before = self.feed.model().first_seq();
        let more = self.feed.model().has_older_events();
        cx.spawn(async move |this, cx| {
            let mut at_start = !more;
            while !at_start && before > 1 {
                let page = crate::call(client.clone(), "threads.events", serde_json::json!({ "threadId": id, "beforeSeq": before, "limit": 500 })).await;
                let Some(page) = page.ok().and_then(|v| EventsPage::decode(&v)) else { return };
                let mut older: Vec<Event> = page.events.into_iter().filter(|e| !e.is_transient() && e.seq < before).collect();
                if older.is_empty() {
                    break;
                }
                older.sort_by_key(|e| e.seq);
                before = older[0].seq;
                at_start = page.at_start;
                older.append(&mut events);
                events = older;
            }
            let model = TranscriptModel::from_events(events, Some(root));
            let text = transcript_text(&title, model.items());
            this.update(cx, |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(text))).ok();
        })
        .detach();
    }
}
