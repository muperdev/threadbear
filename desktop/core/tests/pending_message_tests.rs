// Ports of app/Tests/ThreadbearCoreTests/PendingMessageTests.swift: a
// message sent from the app shows at once, dimmed, and gives way to the
// runner's own `user.message` for it.
#![allow(non_snake_case)]

mod support;

use serde_json::json;
use std::collections::BTreeSet;
use support::*;
use threadbear_core::reducer::*;

fn set(values: &[usize]) -> BTreeSet<usize> {
    values.iter().copied().collect()
}

fn history() -> TranscriptModel {
    from_events(vec![event(1, "turn.started", json!({"turnId": "t1"})), user(2, "yes"), text(3, "m1", "Done."), done(4)])
}

fn pending_ids(model: &TranscriptModel) -> Vec<String> {
    model.pending().iter().map(|m| m.id.clone()).collect()
}

#[test]
fn testAPendingMessageIsTheLastRowUntilItsUserMessageArrives() {
    let mut model = history();
    let pending = PendingMessage::new("a", "Ship it", 4);
    let mut change = model.set_pending(vec![pending]);
    assert_eq!(change.inserted, set(&[3]));
    assert_eq!(ids(&model), ["u:2", "a:m1", "f:t1", "p:a"]);
    let RowContent::User(message) = &model.rows()[3].content else { panic!("a user row") };
    assert_eq!(message.text, "Ship it");
    assert_eq!(message.pending, Some(Pending::Sending));
    assert!(!message.editable, "it isn't in the thread yet");
    assert!(model.items().iter().all(|r| !r.id.starts_with("p:")), "only a row: copying the thread leaves it out");

    // The runner shows it: the real row takes its place in one change.
    change = model.apply(vec![event_turn(5, "turn.started", json!({"turnId": "t2"}), Some("t2")), event_turn(6, "user.message", json!({"text": "Ship it", "images": []}), Some("t2"))]);
    assert!(model.pending().is_empty());
    // The turn started before his message reached it, so it's one the agent
    // woke up for, and the wait before it is marked.
    assert_eq!(ids(&model), ["u:2", "a:m1", "f:t1", "gap:t2", "u:6", "w:t2"]);
    assert!(change.removed.contains(&3));
}

/// Under a running turn's working line, as a queued message waits for that
/// turn; the runner queuing it turns its spinner into a clock.
#[test]
fn testItSitsUnderTheWorkingLineAndShowsWhenQueued() {
    let mut model = from_events(vec![event(1, "turn.started", json!({"turnId": "t1"})), user(2, "Build it")]);
    let mut pending = PendingMessage::new("a", "And the docs", 2);
    model.set_pending(vec![pending.clone()]);
    assert_eq!(ids(&model), ["u:2", "w:t1", "p:a"]);
    let version = model.rows()[2].version;
    pending.queued = true;
    let change = model.set_pending(vec![pending.clone()]);
    assert_eq!(change.updated, set(&[2]));
    assert!(model.rows()[2].version > version);
    let RowContent::User(message) = &model.rows()[2].content else { panic!("a user row") };
    assert_eq!(message.pending, Some(Pending::Queued));
    assert!(model.set_pending(vec![pending]).is_empty(), "nothing changed");
}

/// The same words earlier in the thread aren't it, nor is a message the
/// runner, a schedule, the phone or an edit sent.
#[test]
fn testOnlyALaterMessageOfMikesFromTheAppCounts() {
    let mut model = history();
    model.set_pending(vec![PendingMessage::new("a", "yes", 4)]);
    assert_eq!(model.pending().len(), 1, "the yes at seq 2 came before it");
    model.apply(vec![
        event(5, "user.message", json!({"text": "yes", "images": [], "origin": "runner"})),
        event(6, "user.message", json!({"text": "yes", "images": [], "origin": "phone"})),
        event(7, "user.message", json!({"text": "yes", "images": [], "editOf": 2})),
    ]);
    assert_eq!(model.pending().len(), 1);
    model.apply(vec![user(8, "yes")]);
    assert!(model.pending().is_empty());
}

/// Two sends of the same words need two messages from the runner.
#[test]
fn testEachUserMessageTakesOne() {
    let mut model = history();
    model.set_pending(vec![PendingMessage::new("a", "go", 4), PendingMessage::new("b", "go", 4)]);
    model.apply(vec![user(5, "go")]);
    assert_eq!(pending_ids(&model), ["b"]);
    assert_eq!(model.rows().last().map(|r| r.id.as_str()), Some("p:b"));
}

/// Pictures match by count: the runner copies them under new paths.
#[test]
fn testPicturesMatchByCount() {
    let mut model = history();
    let image = ImageRef::new("/tmp/inbox/a.png", "image/png");
    model.set_pending(vec![PendingMessage { images: vec![image.clone()], ..PendingMessage::new("a", "", 4) }]);
    let Some(RowContent::User(message)) = model.rows().last().map(|r| &r.content) else { panic!("a user row") };
    assert_eq!(message.images, [image], "the picture shows while it's on its way");
    model.apply(vec![event(5, "user.message", json!({"text": "", "images": []}))]);
    assert_eq!(model.pending().len(), 1);
    model.apply(vec![event(6, "user.message", json!({"text": "", "images": [{"path": "/media/a.png", "mediaType": "image/png"}]}))]);
    assert!(model.pending().is_empty());
}

/// A reload (the thread opened again, a catch-up) that already holds the
/// message drops it; one that doesn't keeps it.
#[test]
fn testAReloadThatHoldsItDropsIt() {
    let mut model = history();
    model.set_pending(vec![PendingMessage::new("a", "Ship it", 4), PendingMessage::new("b", "Later", 4)]);
    model.load(vec![event(1, "turn.started", json!({"turnId": "t1"})), user(2, "yes"), done(4), user(5, "Ship it")], &[]);
    assert_eq!(pending_ids(&model), ["b"]);
    assert_eq!(model.rows().last().map(|r| r.id.as_str()), Some("p:b"));
    // Set again from the app's side with both: the one shown stays out.
    model.set_pending(vec![PendingMessage::new("a", "Ship it", 4), PendingMessage::new("b", "Later", 4)]);
    assert_eq!(pending_ids(&model), ["b"]);
}

/// An edit drops the messages queued behind it on the runner, so the queued
/// ones go here too; one still on its way stays.
#[test]
fn testAnEditDropsQueuedOnes() {
    let mut model = history();
    model.set_pending(vec![PendingMessage { queued: true, ..PendingMessage::new("a", "queued", 4) }, PendingMessage::new("b", "sending", 4)]);
    model.apply(vec![event_no_turn(5, "thread.rewound", json!({"fromSeq": 2, "toSeq": 4, "files": {"undone": [], "failed": [], "how": "none"}}))]);
    assert_eq!(pending_ids(&model), ["b"]);
    assert_eq!(model.rows().last().map(|r| r.id.as_str()), Some("p:b"));
}

#[test]
fn testClearingTakesTheRowsAway() {
    let mut model = history();
    model.set_pending(vec![PendingMessage::new("a", "Ship it", 4)]);
    let change = model.set_pending(vec![]);
    assert_eq!(change.removed, set(&[3]));
    assert_eq!(ids(&model), ["u:2", "a:m1", "f:t1"]);
}

/// Opening a group near the top doesn't touch the rows at the end.
#[test]
fn testPendingRowsSurviveAnUpdateInPlace() {
    let mut model = from_events(vec![
        event(1, "turn.started", json!({"turnId": "t1"})),
        user(2, "Look"),
        call(3, "c1"),
        result(4, "c1"),
        call(5, "c2"),
        result(6, "c2"),
        text(7, "m1", "Looked."),
        done(8),
    ]);
    model.set_pending(vec![PendingMessage::new("a", "Thanks", 8)]);
    model.set_expanded("g:t:c1", true);
    assert_eq!(model.rows().last().map(|r| r.id.as_str()), Some("p:a"));
    model.set_expanded("g:t:c1", false);
    assert_eq!(ids(&model), ["u:2", "g:t:c1", "a:m1", "f:t1", "p:a"]);
}
