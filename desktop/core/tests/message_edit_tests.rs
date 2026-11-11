// Ports of app/Tests/ThreadbearCoreTests/MessageEditTests.swift: editing a
// message (docs/protocol.md, "Editing a message"), where the cut events make
// no rows, and only Mike's turn-starting messages can be edited.
#![allow(non_snake_case)]

mod support;

use serde_json::{json, Value};
use support::*;
use threadbear_core::reducer::*;

/// A message that starts its turn, as the runner writes it: the
/// user.message, then turn.started with the same turnId.
fn turn(seq: i64, id: &str, words: &str, reply: &str) -> Vec<Event> {
    vec![
        event_turn(seq, "user.message", json!({"text": words, "images": []}), Some(id)),
        event_turn(seq + 1, "turn.started", json!({"turnId": id}), Some(id)),
        event_turn(seq + 2, "assistant.text", json!({"itemId": format!("m{seq}"), "text": reply}), Some(id)),
        done_turn(seq + 3, id),
    ]
}

fn rewound(seq: i64, from: i64, undone: Value, failed: Value) -> Event {
    event_no_turn(seq, "thread.rewound", json!({"fromSeq": from, "toSeq": seq - 1, "files": {"undone": undone, "failed": failed, "how": "checkpoint"}}))
}

fn edited(seq: i64, id: &str, words: &str, edit_of: i64) -> Event {
    event_turn(seq, "user.message", json!({"text": words, "images": [], "editOf": edit_of}), Some(id))
}

#[test]
fn testDecodesRewindAndEditOf() {
    let e = rewound(9, 5, json!(["/a/b.ts"]), json!([{"path": "/a/c.ts", "reason": "it changed since"}]));
    let EventPayload::ThreadRewound(rewind) = &e.payload else { panic!("{:?}", e.payload) };
    assert_eq!(rewind.range(), Some(5..=8));
    assert_eq!(rewind.undone, ["/a/b.ts"]);
    assert_eq!(rewind.failed, [RewindFailedFile { path: "/a/c.ts".to_owned(), reason: "it changed since".to_owned() }]);
    assert_eq!(rewind.how, "checkpoint");
    assert_eq!(rewind.summary(), "Edited a message, so the thread carries on from there. Undid changes to 1 file. Couldn't undo c.ts (it changed since).");
    let EventPayload::UserMessage { edit_of, .. } = edited(10, "t3", "x", 5).payload else { panic!("a user message") };
    assert_eq!(edit_of, Some(5));
}

#[test]
fn testCutEventsMakeNoRows() {
    let mut events = turn(1, "t1", "One", "First");
    events.extend(turn(5, "t2", "Two", "Second"));
    events.push(rewound(9, 5, json!([]), json!([])));
    events.push(edited(10, "t3", "Two, better", 5));
    let model = from_events(events);
    assert_eq!(item_ids(&model), ["u:1", "a:m1", "f:t1", "rw:9", "u:10"]);
    assert!(model.is_hidden(6));
    assert!(!model.is_hidden(9));
    let Some(RowContent::Note(note)) = model.item("rw:9").map(|r| &r.content) else { panic!("a note") };
    assert_eq!(note, "Edited a message, so the thread carries on from there.");
}

/// Live: the rewind arrives after the rows it cuts are showing. The list
/// reloads, and what came after it goes on as usual.
#[test]
fn testLiveRewindRemovesShownRows() {
    let mut events = turn(1, "t1", "One", "First");
    events.extend(turn(5, "t2", "Two", "Second"));
    let mut model = from_events(events);
    assert_eq!(ids(&model), ["u:1", "a:m1", "f:t1", "u:5", "a:m5", "f:t2"]);
    let change = model.apply(vec![rewound(9, 5, json!(["/x.ts", "/y.ts"]), json!([]))]);
    assert_eq!(change.removed, (0..6).collect());
    assert_eq!(change.inserted, (0..model.rows().len()).collect());
    assert_eq!(ids(&model), ["u:1", "a:m1", "f:t1", "rw:9"]);
    model.apply(vec![edited(10, "t3", "Two, better", 5), event_turn(11, "turn.started", json!({"turnId": "t3"}), Some("t3"))]);
    assert_eq!(ids(&model)[..5], ["u:1", "a:m1", "f:t1", "rw:9", "u:10"]);
}

/// Paging: the newest page holds the marker, and the older page with the
/// start of the cut range comes later. Its cut events stay hidden.
#[test]
fn testOlderPageKeepsCutEventsHidden() {
    let mut all = turn(1, "t1", "One", "First");
    all.extend(turn(5, "t2", "Two", "Second"));
    all.extend(turn(9, "t3", "Three", "Third"));
    all.push(rewound(13, 5, json!([]), json!([])));
    all.push(edited(14, "t4", "Two again", 5));
    let mut model = from_events(all.iter().filter(|e| e.seq >= 7).cloned().collect());
    assert_eq!(item_ids(&model), ["rw:13", "u:14"]);
    model.prepend(all.iter().filter(|e| e.seq < 7).cloned().collect());
    assert_eq!(item_ids(&model), ["u:1", "a:m1", "f:t1", "rw:13", "u:14"]);
}

/// A second edit further back cuts over the first edit's marker too.
#[test]
fn testLaterEditCutsAcrossAnEarlierOne() {
    let mut events = turn(1, "t1", "One", "First");
    events.extend(turn(5, "t2", "Two", "Second"));
    events.extend([
        rewound(9, 5, json!([]), json!([])),
        edited(10, "t3", "Two, better", 5),
        event_turn(11, "turn.started", json!({"turnId": "t3"}), Some("t3")),
        done_turn(12, "t3"),
        rewound(13, 1, json!([]), json!([])),
        edited(14, "t4", "One, better", 1),
    ]);
    let model = from_events(events);
    assert_eq!(item_ids(&model), ["rw:13", "u:14"]);
    assert_eq!(model.hidden(), [1..=12, 5..=8]);
}

#[test]
fn testOnlyMikesTurnStartingMessagesAreEditable() {
    let events = vec![
        event_turn(1, "user.message", json!({"text": "Start", "images": []}), Some("t1")),
        event_turn(2, "turn.started", json!({"turnId": "t1"}), Some("t1")),
        // Sent into the running turn.
        event_turn(3, "user.message", json!({"text": "Also this", "images": []}), Some("t1")),
        done_turn(4, "t1"),
        event_turn(5, "user.message", json!({"text": "Runner restarted", "images": [], "origin": "runner"}), Some("t2")),
        event_turn(6, "turn.started", json!({"turnId": "t2"}), Some("t2")),
        done_turn(7, "t2"),
        event_turn(8, "user.message", json!({"text": "From the phone", "images": [], "origin": "phone"}), Some("t3")),
        event_turn(9, "turn.started", json!({"turnId": "t3"}), Some("t3")),
        done_turn(10, "t3"),
        event_turn(11, "user.message", json!({"text": "[Sent by the agent in the thread \"Other\" (ab12cd34)]\nHello", "images": []}), Some("t4")),
        event_turn(12, "turn.started", json!({"turnId": "t4"}), Some("t4")),
        done_turn(13, "t4"),
        event_turn(14, "user.message", json!({"text": "Scheduled", "images": [], "origin": "schedule"}), Some("t5")),
        event_turn(15, "turn.started", json!({"turnId": "t5"}), Some("t5")),
        done_turn(16, "t5"),
        event_no_turn(17, "user.message", json!({"text": "No turn id", "images": []})),
    ];
    let model = from_events(events);
    let editable: Vec<i64> = model
        .items()
        .iter()
        .filter_map(|row| match &row.content {
            RowContent::User(message) if message.editable => Some(row.seq),
            _ => None,
        })
        .collect();
    assert_eq!(editable, [1, 8]);
}
