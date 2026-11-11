// The row's own words for dumps and golden files: a port of
// Transcript/RowDebug.swift (`excerpt`, `text` and `TokenText`).
use super::card_text::PlanCardText;
use super::format::{cost_text, duration, token_line};
use super::groups::BackgroundWork;
use super::row::{Pending, PlanCardState, Row, RowContent, UserMessage};
use super::text;
use std::collections::BTreeSet;

pub struct RowDebug;

impl RowDebug {
    /// The row's text on one line, cut to `limit` characters.
    pub fn excerpt(row: &Row, limit: usize) -> String {
        Self::one_line(&Self::text(row), limit)
    }

    /// The row's words, whatever kind it is.
    pub fn text(row: &Row) -> String {
        match &row.content {
            RowContent::User(message) => Self::user_text(message),
            RowContent::Assistant(item) | RowContent::Reasoning(item) => item.text.clone(),
            RowContent::Tool(tool) => {
                if tool.failed() {
                    format!("{} (failed)", tool.title())
                } else {
                    tool.title()
                }
            }
            RowContent::Images { images, .. } => {
                if images.len() == 1 {
                    "1 image".to_owned()
                } else {
                    format!("{} images", images.len())
                }
            }
            RowContent::Approval(item) => match &item.resolution {
                Some(resolution) => format!("{} ({})", item.request.title, resolution.decision.as_str()),
                None => item.request.title.clone(),
            },
            RowContent::Question(item) => item.request.questions.iter().map(|q| q.question.clone()).collect::<Vec<_>>().join(" / "),
            RowContent::Secret(item) => item.resolved_text().unwrap_or_else(|| item.request.label.clone()),
            RowContent::Review(review) => format!("{} · {} images · {}", review.title, review.options.len(), review.status),
            RowContent::Plan(entries) => entries.iter().map(|e| e.text.clone()).collect::<Vec<_>>().join("; "),
            RowContent::PlanCard(card) => {
                let state = match card.state {
                    PlanCardState::Waiting => "waiting",
                    PlanCardState::Approved => "approved",
                    PlanCardState::SentBack => "sent back",
                };
                if card.entries.is_empty() {
                    format!("plan ({state}): {}", PlanCardText::summary(&card.text))
                } else {
                    format!("plan ({state}), {}", card.entries.iter().map(|e| e.text.clone()).collect::<Vec<_>>().join("; "))
                }
            }
            RowContent::Asks(card) => {
                let openness = if card.is_open { "open" } else { "closed" };
                if card.is_done() {
                    let loose = match &card.loose_ends {
                        None => String::new(),
                        Some(ends) if ends.is_empty() => ", loose ends: none".to_owned(),
                        Some(ends) => format!(", loose ends: {}", ends.iter().map(|e| e.text.clone()).collect::<Vec<_>>().join("; ")),
                    };
                    let next = if card.next.is_empty() {
                        String::new()
                    } else {
                        let items: Vec<String> =
                            card.next.iter().enumerate().map(|(i, n)| format!("{}{n}", if picked(&card.picked_next, i) { "[x] " } else { "" })).collect();
                        format!(", next: {}", items.join("; "))
                    };
                    return format!("{openness}, nothing needed{loose}{next}");
                }
                format!("{openness}, {} of {} ticked: {}", card.ticked_count(), card.asks.len(), card.asks.iter().map(|a| a.action.clone()).collect::<Vec<_>>().join("; "))
            }
            RowContent::Footer(completion) => {
                let mut words = duration(completion.duration_ms);
                if let Some(cost) = cost_text(completion.cost_usd) {
                    words.push_str(&format!(", {cost}"));
                }
                if completion.outcome != super::event::TurnOutcome::Completed {
                    words.push_str(&format!(", {}", completion.outcome.as_str()));
                }
                // Codex's footer shows only the time; its tokens are here.
                if let Some(tokens) = token_line(completion.usage.as_ref()) {
                    words.push_str(&format!(", {tokens}"));
                }
                words
            }
            RowContent::Note(note) => note.clone(),
            RowContent::Error { message, .. } => message.clone(),
            RowContent::Group(group) => group.summary.clone(),
            RowContent::Working(_) => "Working".to_owned(),
            RowContent::Background(status) => BackgroundWork::line_with_detail(&status.tasks),
            RowContent::Changes(card) => card.title(),
            RowContent::Gap(gap) => gap.label(),
        }
    }

    fn user_text(message: &UserMessage) -> String {
        // One on its way says so, as its spinner or clock does.
        if let Some(pending) = message.pending {
            let word = match pending {
                Pending::Sending => "sending",
                Pending::Queued => "queued",
                Pending::WaitingForRunner => "waiting for the runner",
            };
            let plain = UserMessage { pending: None, ..message.clone() };
            return format!("({word}) {}", Self::user_text(&plain));
        }
        if message.text.is_empty() {
            return if message.images.len() == 1 { "1 image".to_owned() } else { format!("{} images", message.images.len()) };
        }
        // A dump should say it wasn't Mike, as the transcript does.
        if message.is_from_runner() {
            return format!("(sent by the runner) {}", message.text);
        }
        if message.is_from_schedule() {
            return format!("(sent by a schedule) {}", message.text);
        }
        message.text.clone()
    }

    /// `oneLine`: runs of whitespace (Swift's Character.isWhitespace, the
    /// same Unicode property as char::is_whitespace) as one space, cut to
    /// `limit` characters.
    pub fn one_line(words: &str, limit: usize) -> String {
        let joined = words.split(char::is_whitespace).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
        if text::count(&joined) <= limit {
            return joined;
        }
        format!("{}…", text::prefix(&joined, limit.saturating_sub(1)))
    }
}

fn picked(set: &BTreeSet<i64>, index: usize) -> bool {
    set.contains(&(index as i64))
}
