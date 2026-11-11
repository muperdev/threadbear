// Builders for the ported reducer tests, mirroring the Swift tests' `E`
// helper (app/Tests/ThreadbearCoreTests/FakeTransport.swift), so the tests can
// be read beside the Swift ones. Names follow the Swift: `user`, `text`,
// `delta`, `call`, `result`, `done`.
#![allow(dead_code)]

use serde_json::{json, Value};
use threadbear_core::reducer::*;

pub fn event_turn(seq: i64, type_: &str, fields: Value, turn: Option<&str>) -> Event {
    let mut object = json!({"seq": seq, "ts": "2026-10-26T10:00:00Z", "threadId": "th1", "type": type_});
    if let Some(turn) = turn {
        object["turnId"] = json!(turn);
    }
    if let Value::Object(map) = fields {
        for (key, value) in map {
            object[key] = value;
        }
    }
    Event::decode(&object).expect("an event object")
}

/// `E.event(seq, type, fields)`, with its default turn "t1".
pub fn event(seq: i64, type_: &str, fields: Value) -> Event {
    event_turn(seq, type_, fields, Some("t1"))
}

/// `E.event(seq, type, fields, turn: nil)`.
pub fn event_no_turn(seq: i64, type_: &str, fields: Value) -> Event {
    event_turn(seq, type_, fields, None)
}

pub fn user(seq: i64, text: &str) -> Event {
    event(seq, "user.message", json!({"text": text, "images": []}))
}

pub fn text(seq: i64, item: &str, text: &str) -> Event {
    event(seq, "assistant.text", json!({"itemId": item, "text": text}))
}

pub fn delta(seq: i64, item: &str, text: &str) -> Event {
    event(seq, "assistant.text.delta", json!({"itemId": item, "text": text}))
}

pub fn reasoning_delta(seq: i64, item: &str, text: &str) -> Event {
    event(seq, "reasoning.delta", json!({"itemId": item, "text": text}))
}

pub fn call(seq: i64, item: &str) -> Event {
    call_with(seq, item, "Bash", "command", "ls", json!({}))
}

pub fn call_titled(seq: i64, item: &str, title: &str) -> Event {
    call_with(seq, item, "Bash", "command", title, json!({}))
}

pub fn call_input(seq: i64, item: &str, input: Value) -> Event {
    call_with(seq, item, "Bash", "command", "ls", input)
}

pub fn call_with(seq: i64, item: &str, name: &str, kind: &str, title: &str, input: Value) -> Event {
    event(seq, "tool.call", json!({"itemId": item, "name": name, "kind": kind, "title": title, "input": input}))
}

pub fn result(seq: i64, item: &str) -> Event {
    result_with(seq, item, true, "done", json!([]))
}

pub fn result_text(seq: i64, item: &str, text: &str) -> Event {
    result_with(seq, item, true, text, json!([]))
}

pub fn result_images(seq: i64, item: &str, images: Value) -> Event {
    result_with(seq, item, true, "done", images)
}

pub fn result_with(seq: i64, item: &str, ok: bool, text: &str, images: Value) -> Event {
    event(seq, "tool.result", json!({"itemId": item, "ok": ok, "text": text, "truncated": false, "images": images}))
}

pub fn done(seq: i64) -> Event {
    done_turn(seq, "t1")
}

pub fn done_turn(seq: i64, turn: &str) -> Event {
    event_turn(seq, "turn.completed", json!({"turnId": turn, "outcome": "completed", "durationMs": 4200, "costUsd": 0.12}), Some(turn))
}

/// `event.withTurn("t2")`.
pub trait WithTurn {
    fn with_turn(self, turn: &str) -> Event;
}

impl WithTurn for Event {
    fn with_turn(mut self, turn: &str) -> Event {
        self.turn_id = Some(turn.to_owned());
        self
    }
}

pub fn ids(model: &TranscriptModel) -> Vec<String> {
    model.rows().iter().map(|r| r.id.clone()).collect()
}

pub fn item_ids(model: &TranscriptModel) -> Vec<String> {
    model.items().iter().map(|r| r.id.clone()).collect()
}

pub fn kinds(model: &TranscriptModel) -> Vec<RowKind> {
    model.rows().iter().map(|r| r.kind()).collect()
}

pub fn from_events(events: Vec<Event>) -> TranscriptModel {
    TranscriptModel::from_events(events, None)
}

pub fn assistant_content(item: &str, words: &str) -> RowContent {
    RowContent::Assistant(TextItem::new(item, words))
}

pub fn streaming_assistant(item: &str, words: &str) -> RowContent {
    RowContent::Assistant(TextItem { is_streaming: true, ..TextItem::new(item, words) })
}

/// `WorkGroupTests.replay`: plays a change onto a list of ids the way a
/// list does (removals at old indices, then inserts at new ones) and checks
/// it ends up as the model's rows, and that updates point at real rows.
pub fn replay(change: &TranscriptChange, table: &mut Vec<String>, model: &TranscriptModel) {
    for &index in change.removed.iter().rev() {
        table.remove(index);
    }
    for &index in &change.inserted {
        table.insert(index, model.rows()[index].id.clone());
    }
    assert_eq!(*table, ids(model), "replayed change");
    assert!(change.updated.iter().all(|&u| u < model.rows().len()), "updates point at real rows");
    assert!(change.updated.intersection(&change.inserted).next().is_none(), "an update isn't also an insert");
}

/// `TranscriptChange(inserted:, updated:)` and friends, for equality checks.
pub fn change(removed: &[usize], inserted: &[usize], updated: &[usize]) -> TranscriptChange {
    TranscriptChange { removed: removed.iter().copied().collect(), inserted: inserted.iter().copied().collect(), updated: updated.iter().copied().collect(), gap_detected: false }
}

// The Swift tests' `TurnGapTests.at` and `woke`.
pub fn at(mut event: Event, time: &str) -> Event {
    event.ts = format!("2026-11-03T{time}Z");
    event
}

pub fn woke(seq: i64, turn: &str, time: &str, tasks: Value) -> Event {
    let fields = if tasks.as_array().is_some_and(|t| t.is_empty()) { json!({}) } else { json!({"wokeBy": tasks}) };
    at(event_turn(seq, "turn.started", fields, Some(turn)), time)
}
