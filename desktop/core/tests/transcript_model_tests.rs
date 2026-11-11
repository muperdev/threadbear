// Ports of app/Tests/ThreadbearCoreTests/TranscriptModelTests.swift, test
// for test, with the Swift names so the two can be compared.
#![allow(non_snake_case)]

mod support;

use serde_json::json;
use std::collections::BTreeSet;
use support::*;
use threadbear_core::reducer::*;

fn set(values: &[usize]) -> BTreeSet<usize> {
    values.iter().copied().collect()
}

#[test]
fn testBasicTurnBecomesRows() {
    let model = from_events(vec![
        event(1, "turn.started", json!({"turnId": "t1"})),
        user(2, "Fix the test"),
        event(3, "reasoning", json!({"itemId": "r1", "text": "Looking at it"})),
        call_titled(4, "c1", "npm test"),
        result_text(5, "c1", "1 failing"),
        text(6, "m1", "Fixed it."),
        done(7),
    ]);
    assert_eq!(model.items().iter().map(|r| r.kind()).collect::<Vec<_>>(), [RowKind::User, RowKind::Reasoning, RowKind::Tool, RowKind::Assistant, RowKind::Footer]);
    assert_eq!(item_ids(&model), ["u:2", "r:r1", "t:c1", "a:m1", "f:t1"]);
    // The thinking and the tool call fold into one group row.
    assert_eq!(ids(&model), ["u:2", "g:r:r1", "a:m1", "f:t1"]);
    let RowContent::Tool(tool) = &model.items()[2].content else { panic!("a tool") };
    assert_eq!(tool.result.as_ref().map(|r| r.text.as_str()), Some("1 failing"));
    assert_eq!(model.first_seq(), 1);
    assert_eq!(model.last_seq(), 7);
}

/// "Woke up after hibernating for 2 days." is what Mike opened the thread
/// to read, so it has a row of its own under the turn before.
#[test]
fn testAWakeNoteStaysOutOfTheGroupAbove() {
    let model = from_events(vec![
        event(1, "turn.started", json!({"turnId": "t1"})),
        user(2, "Fix the test"),
        call_titled(3, "c1", "npm test"),
        result_text(4, "c1", "ok"),
        text(5, "m1", "Fixed it."),
        done(6),
        event_no_turn(7, "system.note", json!({"text": "Put in Threadbear"})),
        event_no_turn(8, "system.note", json!({"text": "Woke up after hibernating for 2 days."})),
    ]);
    assert_eq!(ids(&model), ["u:2", "g:t:c1", "a:m1", "f:t1", "n:8"]);
    assert!(!model.rows().last().unwrap().in_group);
    for note in ["Woke up after hibernating for 2 days.", "Woke up from hibernating.", "Woke up after 3 hours, as the runner restarted.", "Woke up: Mike mentioned woodworking."] {
        assert!(RowContent::Note(note.to_owned()).is_wake_note(), "{note}");
    }
    assert!(!RowContent::Note("Put in Threadbear".to_owned()).is_wake_note());
}

/// The summary's background work is the last row while no turn runs: it
/// comes and goes in place, and a running turn's working line takes its
/// place.
#[test]
fn testBackgroundWorkIsTheLastRowBetweenTurns() {
    let mut model = from_events(vec![event(1, "turn.started", json!({"turnId": "t1"})), user(2, "Rebuild the pages"), text(3, "m1", "Sent three sub-agents off."), done(4)]);
    assert_eq!(ids(&model), ["u:2", "a:m1", "f:t1"]);
    let cubs = vec![BackgroundTask::new(BackgroundKind::Agent, "Page A", false), BackgroundTask::new(BackgroundKind::Agent, "Page B", false)];
    let mut change = model.set_background(cubs.clone());
    assert_eq!(change.inserted, set(&[3]));
    assert_eq!(ids(&model), ["u:2", "a:m1", "f:t1", "bg:"]);
    assert_eq!(model.rows()[3].content, RowContent::Background(BackgroundStatus { tasks: cubs.clone() }));
    assert_eq!(model.rows()[3].kind(), RowKind::Background);
    assert!(model.set_background(cubs.clone()).is_empty(), "nothing changed");
    // One finishes: the same row, new words.
    let version = model.rows()[3].version;
    let one_done = vec![cubs[0].clone(), BackgroundTask::new(BackgroundKind::Agent, "Page B", true)];
    change = model.set_background(one_done);
    assert_eq!(change.updated, set(&[3]));
    assert!(model.rows()[3].version > version);
    assert_eq!(RowDebug::text(&model.rows()[3]), "1 sub-agent working in the background, 1 just finished · Page A, Page B (finished)");
    // The CLI starts a turn to read it: the working line replaces it, under
    // a divider marking the wait (TurnGapTests).
    change = model.apply(vec![event_turn(5, "turn.started", json!({"turnId": "t2"}), Some("t2"))]);
    assert_eq!(ids(&model), ["u:2", "a:m1", "f:t1", "gap:t2", "w:t2"]);
    assert!(change.removed.contains(&3));
    // That turn ends with the other still out: back to the background line.
    // The turn carried on from the one before, so the two show as one, each
    // reply kept, under one footer.
    model.apply(vec![text(6, "m2", "Page B is done.").with_turn("t2"), done_turn(7, "t2")]);
    assert_eq!(ids(&model), ["u:2", "a:m1", "gap:t2", "a:m2", "f:t2", "bg:"]);
    // All done and read: it goes.
    change = model.set_background(vec![]);
    assert_eq!(change.removed, set(&[5]));
    assert_eq!(ids(&model), ["u:2", "a:m1", "gap:t2", "a:m2", "f:t2"]);
}

#[test]
fn testEmptyReasoningAndEmptyTextMakeNoRows() {
    let model = from_events(vec![event(1, "reasoning", json!({"itemId": "r1", "text": "  "})), text(2, "m1", "")]);
    assert!(model.rows().is_empty());
}

#[test]
fn testDeltasStreamThenPersistedTextReplaces() {
    let mut model = from_events(vec![user(1, "Hi")]);
    let mut change = model.apply(vec![delta(1, "m1", "Hel")]);
    assert_eq!(change.inserted, set(&[1]));
    change = model.apply(vec![delta(1, "m1", "lo")]);
    assert_eq!(change.updated, set(&[1]));
    assert_eq!(model.rows()[1].content, streaming_assistant("m1", "Hello"));
    let version = model.rows()[1].version;
    change = model.apply(vec![text(2, "m1", "Hello!")]);
    assert_eq!(change.updated, set(&[1]));
    assert_eq!(model.rows()[1].content, assistant_content("m1", "Hello!"));
    assert!(model.rows()[1].version > version);
}

/// A reply the CLI sent as a narration block streams as reasoning and lands
/// as text with the same item id: the text takes the live reasoning row's
/// place rather than leaving it streaming forever.
#[test]
fn testTextLandingForStreamingReasoningTakesItsPlace() {
    let mut model = from_events(vec![user(1, "Hi")]);
    model.apply(vec![reasoning_delta(1, "m1:1", "All done")]);
    assert_eq!(item_ids(&model), ["u:1", "r:m1:1"]);
    model.apply(vec![text(2, "m1:1", "All done.")]);
    assert_eq!(item_ids(&model), ["u:1", "a:m1:1"]);
    assert_eq!(ids(&model), ["u:1", "a:m1:1"]);
    assert_eq!(model.rows()[1].content, assistant_content("m1:1", "All done."));
    // A finished thought with the same id is left alone.
    model.apply(vec![event(3, "reasoning", json!({"itemId": "m2:0", "text": "Thought."})), text(4, "m2:0", "Said.")]);
    assert_eq!(item_ids(&model), ["u:1", "a:m1:1", "r:m2:0", "a:m2:0"]);
}

/// A narration block reaches the runner only as a summary; the reply the
/// agent writes again after end_turn comes with the summary's item id and
/// takes its place, above the end_turn call, whether it arrives live or with
/// the history.
#[test]
fn testALaterTextWithTheSameItemIdReplacesASummaryInPlace() {
    let events = vec![
        user(1, "Walk me through it"),
        event(2, "assistant.text", json!({"itemId": "m1:1", "text": "I've sent the full steps.", "narration": true})),
        call_with(3, "e1", "mcp__threadbear__end_turn", "mcp", "end_turn", json!({})),
        result_text(4, "e1", "Noted: this thread will wait in Needs you."),
    ];
    let reply = text(5, "m1:1", "**Step 1:** open the app.");
    let mut live = from_events(events.clone());
    live.apply(vec![reply.clone()]);
    let loaded = from_events(events.into_iter().chain([reply]).collect());
    for model in [&live, &loaded] {
        assert_eq!(item_ids(model), ["u:1", "a:m1:1", "t:e1"]);
        assert_eq!(model.items()[1].content, assistant_content("m1:1", "**Step 1:** open the app."));
    }
}

#[test]
fn testDuplicateAndOldEventsAreIgnoredAndGapsFlagged() {
    let mut model = from_events(vec![user(1, "a"), text(2, "m1", "b")]);
    assert!(model.apply(vec![text(2, "m1", "b")]).is_empty());
    let change = model.apply(vec![text(5, "m2", "later")]);
    assert!(change.gap_detected);
}

#[test]
fn testToolResultImagesGetARowAfterTheirTool() {
    let mut model = from_events(vec![call_with(1, "c1", "Read", "read", "Read a.png", json!({})), call(2, "c2")]);
    assert_eq!(ids(&model), ["g:t:c1"]);
    let change = model.apply(vec![result_images(3, "c1", json!([{"path": "/tmp/a.png", "mediaType": "image/png"}]))]);
    assert_eq!(item_ids(&model), ["t:c1", "i:c1", "t:c2"]);
    // Images are never folded away: they split the group into two lone tools.
    assert_eq!(ids(&model), ["t:c1", "i:c1", "t:c2"]);
    assert_eq!(change.removed, set(&[0]));
    assert_eq!(change.inserted, set(&[0, 1, 2]));
    assert_eq!(change.updated, set(&[]));
    assert_eq!(model.index_of("t:c2"), Some(2));
}

#[test]
fn testEditCallsBecomeDiffRows() {
    let mut model = from_events(vec![call_with(1, "c1", "Edit", "edit", "Edit a.swift", json!({"file_path": "/a.swift", "old_string": "let a = 1", "new_string": "let a = 2"}))]);
    assert_eq!(model.rows().first().map(|r| r.kind()), Some(RowKind::Diff));
    let RowContent::Tool(tool) = &model.rows()[0].content else { panic!("a tool") };
    assert_eq!(tool.effective_diffs().first().and_then(|d| d.hunks.first()).map(|h| h.lines.clone()), Some(vec!["-let a = 1".to_owned(), "+let a = 2".to_owned()]));
    model.apply(vec![event(2, "edit.diff", json!({"itemId": "c1", "path": "/a.swift", "hunks": [
        {"oldStart": 10, "oldLines": 1, "newStart": 10, "newLines": 1, "lines": ["-let a = 1", "+let a = 3"]},
    ]}))]);
    let RowContent::Tool(updated) = &model.rows()[0].content else { panic!("a tool") };
    assert_eq!(updated.effective_diffs().first().and_then(|d| d.hunks.first()).map(|h| h.old_start), Some(10));
}

#[test]
fn testApprovalAndQuestionResolution() {
    let mut model = from_events(vec![
        event(1, "approval.requested", json!({"approvalId": "ap1", "name": "Bash", "title": "rm -rf build", "input": {}, "options": ["allow", "deny"]})),
        event(2, "question.requested", json!({"approvalId": "q1", "questions": [{"question": "Which?", "multiSelect": false, "options": [{"label": "A"}]}]})),
    ]);
    assert_eq!(kinds(&model), [RowKind::Approval, RowKind::Question]);
    let change = model.apply(vec![
        event(3, "approval.resolved", json!({"approvalId": "ap1", "decision": "deny"})),
        event(4, "approval.resolved", json!({"approvalId": "q1", "decision": "answered"})),
    ]);
    assert_eq!(change.updated, set(&[0, 1]));
    let (RowContent::Approval(approval), RowContent::Question(question)) = (&model.rows()[0].content, &model.rows()[1].content) else { panic!("cards") };
    assert_eq!(approval.resolution.as_ref().map(|r| r.decision), Some(ApprovalDecision::Deny));
    assert_eq!(question.resolution.as_ref().map(|r| r.decision), Some(ApprovalDecision::Answered));
}

/// A subagent's card can be open with no turn, across turns: it was folded
/// into the finished turn and the thread sat in Needs you with nothing to
/// answer in sight.
#[test]
fn testASubagentsOpenCardStaysAtTheEndUntilAnswered() {
    let mut model = TranscriptModel::new(None);
    model.apply(vec![
        event_turn(1, "turn.started", json!({"turnId": "t1"}), Some("t1")),
        event_turn(2, "user.message", json!({"text": "go"}), Some("t1")),
        event_turn(3, "assistant.text", json!({"itemId": "a1", "text": "Started."}), Some("t1")),
        event_turn(4, "turn.completed", json!({"turnId": "t1", "outcome": "completed", "durationMs": 100}), Some("t1")),
        event_no_turn(5, "approval.requested", json!({"approvalId": "ap1", "name": "Write", "title": "Write a.ts", "input": {}, "options": ["allow", "deny"]})),
    ]);
    model.set_background(vec![BackgroundTask::new(BackgroundKind::Agent, "Port it", false)]);
    assert_eq!(model.rows().last().map(|r| r.id.as_str()), Some("ap:ap1"));
    assert!(!model.rows().iter().any(|r| r.kind() == RowKind::Background), "no \"working\" under a card it's waiting on");
    assert_eq!(model.pending_approval_ids(), ["ap1"]);
    // A later turn goes above it.
    model.apply(vec![
        event_turn(6, "turn.started", json!({"turnId": "t2"}), Some("t2")),
        event_turn(7, "user.message", json!({"text": "ping"}), Some("t2")),
        event_turn(8, "assistant.text", json!({"itemId": "a2", "text": "pong"}), Some("t2")),
    ]);
    assert!(model.rows().iter().any(|r| r.id == "ap:ap1"));
    let position = |id: &str| model.rows().iter().position(|r| r.id == id);
    assert!(position("a:a2").unwrap() < position("ap:ap1").unwrap());
    model.apply(vec![event_turn(9, "turn.completed", json!({"turnId": "t2", "outcome": "completed", "durationMs": 100}), Some("t2"))]);
    assert_eq!(model.rows().last().map(|r| r.id.as_str()), Some("ap:ap1"));
    assert_eq!(model.rows().iter().filter(|r| r.id == "ap:ap1").count(), 1);
    // Denied: it goes back to its place above t2.
    model.apply(vec![event_no_turn(10, "approval.resolved", json!({"approvalId": "ap1", "decision": "deny"}))]);
    assert!(!model.rows().iter().any(|r| r.id == "ap:ap1"), "answered, it folds into its finished turn like any card");
    assert!(model.pending_approval_ids().is_empty());
    assert!(model.rows().iter().any(|r| r.kind() == RowKind::Background), "and the background line is back");
}

/// A card from inside a running turn stays put, and floats once that ends.
#[test]
fn testASubagentsCardFromATurnFloatsWhenThatTurnEnds() {
    let mut model = TranscriptModel::new(None);
    model.apply(vec![
        event_turn(1, "turn.started", json!({"turnId": "t1"}), Some("t1")),
        event_turn(2, "user.message", json!({"text": "go"}), Some("t1")),
        event_turn(3, "approval.requested", json!({"approvalId": "ap1", "name": "Write", "title": "Write a.ts", "input": {}, "options": ["allow", "deny"]}), Some("t1")),
        event_turn(4, "assistant.text", json!({"itemId": "a1", "text": "Started."}), Some("t1")),
    ]);
    let position = |model: &TranscriptModel, id: &str| model.rows().iter().position(|r| r.id == id);
    assert!(position(&model, "ap:ap1").unwrap() < position(&model, "a:a1").unwrap());
    model.apply(vec![event_turn(5, "turn.completed", json!({"turnId": "t1", "outcome": "completed", "durationMs": 100}), Some("t1"))]);
    assert_eq!(model.rows().last().map(|r| r.id.as_str()), Some("ap:ap1"));
}

/// A secret card is a pending approval of its own (`s:`): resolved by
/// approval.resolved, and never one Return in the composer can answer.
#[test]
fn testSecretCardWaitsThenSettles() {
    let mut model = from_events(vec![
        event(1, "secret.requested", json!({"approvalId": "sec1", "label": "Mac administrator password", "reason": "For pmset"})),
        event(2, "secret.requested", json!({"approvalId": "sec2", "label": "API key", "reason": "For wrangler"})),
        event(3, "secret.requested", json!({"approvalId": "sec3", "label": "Token", "reason": ""})),
    ]);
    assert_eq!(ids(&model), ["s:sec1", "s:sec2", "s:sec3"]);
    assert_eq!(kinds(&model), [RowKind::Secret, RowKind::Secret, RowKind::Secret]);
    assert_eq!(model.pending_approval_ids(), Vec::<String>::new(), "Return must never answer a secret card");
    let change = model.apply(vec![
        event(4, "approval.resolved", json!({"approvalId": "sec1", "decision": "answered"})),
        event(5, "approval.resolved", json!({"approvalId": "sec2", "decision": "deny"})),
        event(6, "approval.resolved", json!({"approvalId": "sec3", "decision": "expired"})),
    ]);
    assert_eq!(change.updated, set(&[0, 1, 2]));
    let texts: Vec<String> = model
        .rows()
        .iter()
        .filter_map(|row| match &row.content {
            RowContent::Secret(item) => item.resolved_text(),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["Mac administrator password · Saved securely", "API key · Declined", "Token · Request ended"]);
}

/// What Return in an empty composer can allow: approval cards still
/// waiting, oldest first, never questions.
#[test]
fn testPendingApprovalIds() {
    let mut model = from_events(vec![
        event(1, "approval.requested", json!({"approvalId": "ap1", "name": "Bash", "title": "ls", "input": {}, "options": ["allow", "deny"]})),
        event(2, "question.requested", json!({"approvalId": "q1", "questions": [{"question": "Which?", "multiSelect": false, "options": [{"label": "A"}]}]})),
        event(3, "approval.requested", json!({"approvalId": "ap2", "name": "Bash", "title": "pwd", "input": {}, "options": ["allow", "deny"]})),
    ]);
    assert_eq!(model.pending_approval_ids(), ["ap1", "ap2"]);
    model.apply(vec![event(4, "approval.resolved", json!({"approvalId": "ap1", "decision": "allow"}))]);
    assert_eq!(model.pending_approval_ids(), ["ap2"]);
    model.apply(vec![event(5, "approval.resolved", json!({"approvalId": "ap2", "decision": "deny"}))]);
    assert_eq!(model.pending_approval_ids(), Vec::<String>::new());
}

#[test]
fn testPlanUpdatesOneRowPerTurn() {
    let mut model = from_events(vec![event(1, "plan.updated", json!({"entries": [{"text": "A", "status": "pending"}]}))]);
    let change = model.apply(vec![event(2, "plan.updated", json!({"entries": [{"text": "A", "status": "completed"}]}))]);
    assert_eq!(model.rows().len(), 1);
    assert_eq!(change.updated, set(&[0]));
    assert_eq!(model.rows()[0].content, RowContent::Plan(vec![PlanEntry::new("A", PlanStatus::Completed)]));
}

#[test]
fn testTurnCompletedStopsStreaming() {
    let mut model = from_events(vec![user(1, "Hi")]);
    model.apply(vec![delta(1, "m1", "Partial")]);
    let change = model.apply(vec![event(2, "turn.completed", json!({"turnId": "t1", "outcome": "interrupted", "durationMs": 100}))]);
    assert_eq!(change.inserted, set(&[2]));
    assert_eq!(change.updated, set(&[1]));
    assert_eq!(model.rows()[1].content, assistant_content("m1", "Partial"));
}

#[test]
fn testToolOutputDeltaThenResult() {
    let mut model = from_events(vec![call(1, "c1")]);
    model.apply(vec![event(1, "tool.output.delta", json!({"itemId": "c1", "text": "line 1\n"}))]);
    let RowContent::Tool(live) = &model.rows()[0].content else { panic!("a tool") };
    assert_eq!(live.live_output, "line 1\n");
    model.apply(vec![result_text(2, "c1", "all done")]);
    let RowContent::Tool(finished) = &model.rows()[0].content else { panic!("a tool") };
    assert_eq!(finished.live_output, "");
    assert_eq!(finished.result.as_ref().map(|r| r.text.as_str()), Some("all done"));
}

#[test]
fn testPrependOlderPageKeepsIdsAndMergesSplitToolCalls() {
    // The newer page starts with a result whose call is on the older page.
    // (The turn's end is on the newer page too; a page that starts mid-turn
    // with no end would be a turn still running.)
    let mut model = from_events(vec![result_text(5, "c1", "ok"), text(6, "m1", "Done"), done(7)]);
    assert_eq!(ids(&model), ["g:t:c1", "a:m1", "f:t1"]);
    let added = model.prepend(vec![user(3, "Run it"), call_titled(4, "c1", "make")]);
    assert_eq!(ids(&model), ["u:3", "g:t:c1", "a:m1", "f:t1"]);
    assert_eq!(added.added, 1);
    // The call it merged with is folded in the turn's line, which reads the same.
    assert_eq!(added.changed_ids, Vec::<String>::new());
    let Some(RowContent::Tool(tool)) = model.item("t:c1").map(|r| &r.content) else { panic!("a tool") };
    assert_eq!(tool.title(), "make");
    assert_eq!(tool.result.as_ref().map(|r| r.text.as_str()), Some("ok"));
    assert_eq!(model.first_seq(), 3);
    assert!(model.has_older_events());
}

#[test]
fn testPrependKeepsVersionsOfUnchangedRows() {
    let mut model = from_events(vec![user(5, "b"), text(6, "m1", "reply")]);
    let before: Vec<u64> = model.rows().iter().map(|r| r.version).collect();
    let result = model.prepend(vec![user(4, "a")]);
    assert_eq!(result, PrependResult { added: 1, changed_ids: vec![], keeps_order: true });
    assert_eq!(model.rows()[1..].iter().map(|r| r.version).collect::<Vec<_>>(), before);
}

#[test]
fn testTrimKeepsTheNewestEventsAndStreamingText() {
    let mut events = Vec::new();
    let mut seq = 0;
    for turn in 0..10 {
        seq += 1;
        events.push(user(seq, &format!("question {turn}")));
        seq += 1;
        events.push(call(seq, &format!("c{turn}")));
        seq += 1;
        events.push(result(seq, &format!("c{turn}")));
        seq += 1;
        events.push(text(seq, &format!("m{turn}"), &format!("answer {turn}")));
    }
    let mut model = from_events(events.clone());
    model.apply(vec![delta(seq, "live", "still typing")]);
    let last_row_version = model.row("a:m9").map(|r| r.version);
    model.trim(6);
    assert_eq!(model.events().len(), 6);
    assert_eq!(model.first_seq(), seq - 5);
    assert_eq!(model.last_seq(), seq);
    assert!(model.has_older_events());
    assert!(model.row("u:1").is_none());
    assert_eq!(model.row("a:m9").map(|r| r.version), last_row_version, "unchanged rows keep their version");
    let Some(RowContent::Assistant(live)) = model.row("a:live").map(|r| &r.content) else { panic!("streaming row lost") };
    assert_eq!(live.text, "still typing");
    // Paging back in restores the rows.
    let added = model.prepend(events[..events.len() - 6].to_vec());
    assert!(added.added > 0);
    assert!(model.row("u:1").is_some());
    assert_eq!(model.events().len(), events.len());
}

#[test]
fn testVersionsAreUniqueAcrossModels() {
    let a = from_events(vec![user(1, "x")]);
    let b = from_events(vec![user(1, "x")]);
    assert_ne!(a.rows()[0].version, b.rows()[0].version);
}

#[test]
fn testPrependKeepsStreamingText() {
    let mut model = from_events(vec![user(5, "b")]);
    model.apply(vec![delta(5, "m1", "streaming")]);
    model.prepend(vec![user(4, "a")]);
    assert_eq!(ids(&model), ["u:4", "u:5", "a:m1"]);
    assert_eq!(model.rows()[2].content, streaming_assistant("m1", "streaming"));
}

#[test]
fn testInProgressFromEventsPage() {
    let mut model = TranscriptModel::new(None);
    model.load(vec![user(1, "Hi")], &[InProgress::new("m1", "assistant.text", "Half")]);
    assert_eq!(model.rows().last().map(|r| r.content.clone()), Some(streaming_assistant("m1", "Half")));
}

// Histories with an earlier turn.completed: replaying it used to end
// whatever was streaming now, so the live row vanished.
#[test]
fn testLoadMidTurnKeepsInProgressTextAfterEarlierTurns() {
    let mut model = TranscriptModel::new(None);
    model.load(vec![user(1, "Hi"), text(2, "m0", "Hello"), done(3), user(4, "More")], &[InProgress::new("live", "assistant.text", "Half way")]);
    assert_eq!(model.rows().last().map(|r| r.content.clone()), Some(streaming_assistant("live", "Half way")));
}

#[test]
fn testPrependDuringAReplyKeepsTheLiveText() {
    let mut model = from_events(vec![done_turn(5, "t0"), user(6, "Next")]);
    model.apply(vec![delta(6, "live", "Streaming")]);
    model.prepend(vec![user(3, "First"), done_turn(4, "tA")]);
    assert_eq!(model.row("a:live").map(|r| r.content.clone()), Some(streaming_assistant("live", "Streaming")));
    model.apply(vec![delta(6, "live", " more")]);
    assert_eq!(model.row("a:live").map(|r| r.content.clone()), Some(streaming_assistant("live", "Streaming more")));
}

#[test]
fn testTrimDuringAReplyKeepsTheLiveText() {
    let mut model = from_events(vec![user(1, "a"), done_turn(2, "t0"), user(3, "b"), done_turn(4, "t1"), user(5, "c")]);
    model.apply(vec![delta(5, "live", "Still going")]);
    model.trim(2);
    assert_eq!(model.row("a:live").map(|r| r.content.clone()), Some(streaming_assistant("live", "Still going")));
}

#[test]
fn testRebuildDropsStreamingTextOnceItsPersisted() {
    let mut model = from_events(vec![user(5, "b")]);
    model.apply(vec![delta(5, "m1", "Hel")]);
    model.apply(vec![text(6, "m1", "Hello"), done(7)]);
    model.prepend(vec![user(4, "a")]);
    assert_eq!(model.rows().iter().filter(|r| r.id == "a:m1").count(), 1);
    assert_eq!(model.row("a:m1").map(|r| r.content.clone()), Some(assistant_content("m1", "Hello")));
}

#[test]
fn testPrependWithAMovedRowSaysTheOrderChanged() {
    // Parallel tool calls: both calls on the older page, the second result
    // first on the newer page, then the first result. After the merge t:c1
    // sits above t:c2, the old first row.
    let mut model = from_events(vec![result(5, "c2"), result(6, "c1"), text(7, "m1", "Done"), done(8)]);
    assert_eq!(item_ids(&model), ["t:c2", "t:c1", "a:m1", "f:t1"]);
    assert_eq!(ids(&model), ["g:t:c2", "a:m1", "f:t1"]);
    let result = model.prepend(vec![user(2, "Go"), call(3, "c1"), call(4, "c2")]);
    assert_eq!(item_ids(&model), ["u:2", "t:c1", "t:c2", "a:m1", "f:t1"]);
    // The group now starts at t:c1, so it has a new id and the old first
    // row is gone; the first old row still there is the reply.
    assert_eq!(ids(&model), ["u:2", "g:t:c1", "a:m1", "f:t1"]);
    assert_eq!(result.added, 2);
    assert!(!result.keeps_order, "t:c1 moved above the old first row");
}

#[test]
fn testPrependWithAnOlderRowBelowTheOldFirstRowSaysTheOrderChanged() {
    // The newer page starts with c1's result; between the call and the
    // result the older page has some text, which lands below t:c1.
    let mut model = from_events(vec![result(6, "c1"), text(7, "m1", "Done"), done(8)]);
    let result = model.prepend(vec![call(4, "c1"), text(5, "m0", "Running it")]);
    // Both land in the finished turn's line of work, so what shows keeps
    // its order; open, the line lists them in their order.
    assert_eq!(ids(&model), ["g:t:c1", "a:m1", "f:t1"]);
    assert!(result.keeps_order);
    let Some(RowContent::Group(work)) = model.row("g:t:c1").map(|r| &r.content) else { panic!("a group") };
    assert_eq!(work.member_ids, ["t:c1", "a:m0"]);
}

#[test]
fn testPrependWithTheCallLastOnTheOlderPageKeepsOrder() {
    let mut model = from_events(vec![result(5, "c1"), text(6, "m1", "Done"), done(7)]);
    let result = model.prepend(vec![user(3, "Run it"), call(4, "c1")]);
    assert!(result.keeps_order);
    assert_eq!(result.added, 1);
}

#[test]
fn testUnknownEventsAreIgnored() {
    let mut model = from_events(vec![user(1, "a")]);
    let change = model.apply(vec![event(2, "brand.new", json!({"x": 1}))]);
    assert!(change.is_empty());
    assert_eq!(model.last_seq(), 2);
}

// ---- ThreadFeedTests ----

#[test]
fn testInOrderEventsFlush() {
    let mut feed = ThreadFeed::new(from_events(vec![user(1, "a")]));
    assert_eq!(feed.receive(text(2, "m1", "b")), FeedAction::ScheduleFlush);
    assert_eq!(feed.receive(delta(2, "m2", "c")), FeedAction::ScheduleFlush);
    let change = feed.flush();
    assert_eq!(change.inserted, set(&[1, 2]));
    assert!(!feed.has_pending());
}

#[test]
fn testGapAsksForCatchUpThenFills() {
    let mut feed = ThreadFeed::new(from_events(vec![user(1, "a")]));
    assert_eq!(feed.receive(text(4, "m3", "d")), FeedAction::NeedsCatchUp { after_seq: 1 });
    assert_eq!(feed.receive(text(5, "m4", "e")), FeedAction::None); // already catching up
    assert_eq!(feed.catch_up(vec![text(2, "m1", "b"), text(3, "m2", "c")]), FeedAction::ScheduleFlush);
    feed.flush();
    assert_eq!(ids(feed.model()), ["u:1", "a:m1", "a:m2", "a:m3", "a:m4"]);
    assert_eq!(feed.accepted_seq(), 5);
}

#[test]
fn testSubscribeReplayRacingPushes() {
    let mut feed = ThreadFeed::new(from_events(vec![user(1, "a")]));
    feed.begin_subscribe();
    // A push for seq 3 lands before the subscribe reply with 2 and 3.
    assert_eq!(feed.receive(text(3, "m2", "c")), FeedAction::None);
    assert_eq!(feed.catch_up(vec![text(2, "m1", "b"), text(3, "m2", "c")]), FeedAction::ScheduleFlush);
    feed.flush();
    assert_eq!(ids(feed.model()), ["u:1", "a:m1", "a:m2"]);
}

/// Another runner's thread through Convex (relay seqs): gaps are how it is,
/// so nothing waits for a catch-up, and a late event lands where its seq
/// says.
#[test]
fn testSparseFeedTakesGapsAndLateEvents() {
    let mut feed = ThreadFeed::new(from_events(vec![user(13, "a")]));
    feed.set_sparse(true);
    assert_eq!(feed.receive(text(37, "m1", "b")), FeedAction::ScheduleFlush);
    assert_eq!(feed.receive(text(49, "m2", "c")), FeedAction::ScheduleFlush);
    feed.flush();
    assert_eq!(ids(feed.model()), ["u:13", "a:m1", "a:m2"]);
    assert_eq!(feed.receive(text(38, "m3", "late")), FeedAction::ScheduleFlush);
    assert_eq!(feed.receive(text(49, "m2", "c")), FeedAction::ScheduleFlush);
    feed.flush();
    assert_eq!(ids(feed.model()), ["u:13", "a:m1", "a:m3", "a:m2"]);
    assert_eq!(feed.accepted_seq(), 49);
    // A subscribe reply with a gap doesn't ask again either.
    assert_eq!(feed.catch_up(vec![text(61, "m4", "d")]), FeedAction::ScheduleFlush);
}

#[test]
fn testKnownStartMeansNothingOlder() {
    let mut model = from_events(vec![user(13, "a")]);
    assert!(model.has_older_events());
    model.known_start = true;
    assert!(!model.has_older_events());
}

#[test]
fn testStillGappedAfterCatchUpAsksAgain() {
    let mut feed = ThreadFeed::new(from_events(vec![user(1, "a")]));
    feed.receive(text(10, "m9", "z"));
    assert_eq!(feed.catch_up(vec![text(2, "m1", "b")]), FeedAction::NeedsCatchUp { after_seq: 2 });
}

/// The runner sent seq 2 in a shape this app can't read. It used to be
/// dropped, so every catch-up came back without it and the thread froze.
/// Now it decodes as Unknown and the feed moves on.
#[test]
fn testAnUnreadableEventDoesNotStallTheFeed() {
    let mut feed = ThreadFeed::new(from_events(vec![user(1, "a")]));
    let broken = event(2, "edit.diff", json!({"itemId": "c1", "hunks": "not an array"}));
    assert_eq!(feed.receive(broken), FeedAction::ScheduleFlush);
    assert_eq!(feed.receive(text(3, "m1", "after")), FeedAction::ScheduleFlush);
    feed.flush();
    assert_eq!(feed.accepted_seq(), 3);
    assert_eq!(ids(feed.model()), ["u:1", "a:m1"]);
}

/// If the runner really doesn't have a seq, give up on it after a few
/// fruitless catch-ups instead of fetching forever.
#[test]
fn testAHoleTheRunnerCantFillIsSkippedAfterAFewTries() {
    let mut feed = ThreadFeed::new(from_events(vec![user(1, "a")]));
    assert_eq!(feed.receive(text(3, "m1", "after the hole")), FeedAction::NeedsCatchUp { after_seq: 1 });
    for _ in 1..ThreadFeed::MAX_STALLED_CATCH_UPS {
        assert_eq!(feed.catch_up(vec![text(3, "m1", "after the hole")]), FeedAction::NeedsCatchUp { after_seq: 1 });
    }
    assert_eq!(feed.catch_up(vec![text(3, "m1", "after the hole")]), FeedAction::ScheduleFlush);
    feed.flush();
    assert_eq!(feed.accepted_seq(), 3);
    assert_eq!(feed.model().rows().last().map(|r| r.id.as_str()), Some("a:m1"));
    assert!(!feed.is_catching_up());
}

/// Opening a thread mid-reply: `threads.events` gave "Hel", then the runner
/// streamed "lo wor" before the subscription started (so those deltas never
/// reached the app), then "ld" after. The subscribe reply's in-progress text
/// fills the hole; before, the row read "Helld".
#[test]
fn testSubscribeInProgressFillsDeltasMissedBeforeTheSubscription() {
    let mut model = TranscriptModel::new(None);
    model.load(vec![user(1, "Hi")], &[InProgress::new("m1", "assistant.text", "Hel")]);
    let mut feed = ThreadFeed::new(model);
    feed.begin_subscribe();
    // Pushed after the runner registered the subscription, but handled
    // before the subscribe reply.
    assert_eq!(feed.receive(delta(1, "m1", "ld")), FeedAction::None);
    let action = feed.finish_subscribe(vec![], &[InProgress::new("m1", "assistant.text", "Hello wor")]);
    assert_eq!(action, FeedAction::ScheduleFlush);
    feed.flush();
    assert_eq!(feed.model().rows().last().map(|r| r.content.clone()), Some(streaming_assistant("m1", "Hello world")));
    // Later deltas carry on from there, and the persisted text replaces it.
    feed.receive(delta(1, "m1", "!"));
    feed.flush();
    assert_eq!(feed.model().rows().last().map(|r| r.content.clone()), Some(streaming_assistant("m1", "Hello world!")));
    feed.receive(text(2, "m1", "Hello world!"));
    feed.flush();
    assert_eq!(ids(feed.model()), ["u:1", "a:m1"]);
    assert_eq!(feed.model().rows().last().map(|r| r.content.clone()), Some(assistant_content("m1", "Hello world!")));
}

/// Reasoning streams the same way, and an item that started streaming only
/// after `threads.events` gets its row from the subscribe reply.
#[test]
fn testSubscribeInProgressAddsReasoningThatStartedSinceTheLoad() {
    let mut feed = ThreadFeed::new(from_events(vec![user(1, "Hi")]));
    feed.begin_subscribe();
    feed.finish_subscribe(vec![], &[InProgress::new("r1", "reasoning", "Thinking it over")]);
    feed.flush();
    assert_eq!(feed.model().rows().last().map(|r| r.content.clone()), Some(RowContent::Reasoning(TextItem { is_streaming: true, ..TextItem::new("r1", "Thinking it over") })));
}

/// The replay already has the finished text: the in-progress list wouldn't
/// name it, but if it did, the persisted text wins.
#[test]
fn testInProgressNeverOverwritesPersistedText() {
    let mut feed = ThreadFeed::new(from_events(vec![user(1, "Hi")]));
    feed.begin_subscribe();
    feed.finish_subscribe(vec![text(2, "m1", "Done.")], &[InProgress::new("m1", "assistant.text", "Do")]);
    feed.flush();
    assert_eq!(feed.model().rows().last().map(|r| r.content.clone()), Some(assistant_content("m1", "Done.")));
}

#[test]
fn testAFailedCatchUpIsNotAStall() {
    let mut feed = ThreadFeed::new(from_events(vec![user(1, "a")]));
    feed.receive(text(3, "m1", "x"));
    feed.catch_up_failed();
    assert_eq!(feed.stalled_catch_ups(), 0);
    assert!(!feed.is_catching_up());
    // The reconnect's subscribe replay fills it.
    feed.begin_subscribe();
    assert_eq!(feed.catch_up(vec![text(2, "m0", "y"), text(3, "m1", "x")]), FeedAction::ScheduleFlush);
    assert_eq!(feed.accepted_seq(), 3);
}
