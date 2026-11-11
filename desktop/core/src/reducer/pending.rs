// A message sent from this app that the runner hasn't shown yet: a port of
// Transcript/PendingMessage.swift.
use super::event::{AskAnswer, Event, EventPayload, ImageRef};
use super::row::{Pending, Row, RowContent, UserMessage};

/// It sits at the end of the transcript as his bubble, dimmed, so a click
/// on Send always shows something at once, however long a busy runner takes.
/// It goes once the thread's own `user.message` for it arrives, or when the
/// send fails or the connection drops.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PendingMessage {
    /// Made up by the app; the runner never sees it.
    pub id: String,
    pub text: String,
    pub images: Vec<ImageRef>,
    pub answers: Vec<AskAnswer>,
    /// The thread's last seq when it was sent. Only a `user.message` after
    /// it can be this one: an older "yes" in the history isn't.
    pub after_seq: i64,
    /// The runner has taken it but not shown it: it's waiting behind the
    /// running turn, and its `user.message` comes when its own turn starts.
    pub queued: bool,
    /// Held in the app until a restarting runner is back.
    pub waiting_for_runner: bool,
}

impl PendingMessage {
    pub fn new(id: &str, text: &str, after_seq: i64) -> PendingMessage {
        PendingMessage { id: id.to_owned(), text: text.to_owned(), after_seq, ..PendingMessage::default() }
    }

    /// Whether `event` is the runner showing this message. Matched on the
    /// words (and how many pictures, as the runner copies them under new
    /// paths): its `user.message` usually arrives before the reply to
    /// `threads.send` does, so there's no turn id to go on yet. Messages
    /// from the runner, a schedule, the phone or an edit are never ours.
    pub fn is_landed_by(&self, event: &Event) -> bool {
        if event.seq <= self.after_seq {
            return false;
        }
        match &event.payload {
            EventPayload::UserMessage { text, images, origin: None, edit_of: None, .. } => *text == self.text && images.len() == self.images.len(),
            _ => false,
        }
    }

    /// How the bubble shows it.
    pub fn row(&self) -> Row {
        let pending = if self.waiting_for_runner {
            Pending::WaitingForRunner
        } else if self.queued {
            Pending::Queued
        } else {
            Pending::Sending
        };
        let message = UserMessage { text: self.text.clone(), images: self.images.clone(), answers: self.answers.clone(), pending: Some(pending), ..UserMessage::default() };
        Row::new(&format!("p:{}", self.id), self.after_seq, None, RowContent::User(message))
    }

    /// Takes out the messages that `events` show, each event standing for
    /// one message (two sends of "yes" need two). Returns whether any went.
    pub fn remove_landed<'a>(events: impl IntoIterator<Item = &'a Event>, list: &mut Vec<PendingMessage>) -> bool {
        if list.is_empty() {
            return false;
        }
        let mut removed = false;
        for event in events {
            if !matches!(event.payload, EventPayload::UserMessage { .. }) {
                continue;
            }
            if let Some(index) = list.iter().position(|m| m.is_landed_by(event)) {
                list.remove(index);
                removed = true;
                if list.is_empty() {
                    break;
                }
            }
        }
        removed
    }
}
