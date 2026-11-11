// Ports of app/Tests/ThreadbearCoreTests/WorkGroupTests.swift: folding runs
// of work into group rows, the working line and the changed files card.
#![allow(non_snake_case)]

mod support;

use serde_json::{json, Value};
use std::collections::BTreeSet;
use support::*;
use threadbear_core::reducer::*;

fn set(values: &[usize]) -> BTreeSet<usize> {
    values.iter().copied().collect()
}

// `Self.started`, `Self.reasoning`, `Self.edit` from the Swift test class.
fn started(seq: i64) -> Event {
    started_turn(seq, "t1")
}

fn started_turn(seq: i64, turn: &str) -> Event {
    event_turn(seq, "turn.started", json!({"ts": "2026-10-26T10:00:00.250Z"}), Some(turn))
}

fn reasoning(seq: i64, item: &str) -> Event {
    event(seq, "reasoning", json!({"itemId": item, "text": "Looking at it"}))
}

fn edit(seq: i64, item: &str, path: &str) -> Event {
    edit_with(seq, item, path, "a", "b", "Edit")
}

fn edit_with(seq: i64, item: &str, path: &str, old: &str, new: &str, name: &str) -> Event {
    let kind = if name == "Write" { "write" } else { "edit" };
    let input = if name == "Write" { json!({"file_path": path, "content": new}) } else { json!({"file_path": path, "old_string": old, "new_string": new}) };
    call_with(seq, item, name, kind, &format!("{name} {path}"), input)
}

fn asked(seq: i64, id: &str) -> Event {
    event(seq, "approval.requested", json!({"approvalId": id, "name": "Bash", "title": "rm x", "input": {"command": "rm x"}, "options": ["allow", "deny"]}))
}

fn resolved(seq: i64, id: &str, decision: &str) -> Event {
    event(seq, "approval.resolved", json!({"approvalId": id, "decision": decision}))
}

fn asked_for(seq: i64, id: &str, tool: Option<&str>) -> Event {
    let mut fields = json!({"approvalId": id, "name": "Write", "title": "Write /a.sh", "input": {"file_path": "/a.sh", "content": "x"}, "options": ["allow", "deny"]});
    if let Some(tool) = tool {
        fields["itemId"] = Value::from(tool);
    }
    event(seq, "approval.requested", fields)
}

fn approval<'a>(model: &'a TranscriptModel, id: &str) -> Option<&'a ApprovalItem> {
    match model.item(id).map(|r| &r.content) {
        Some(RowContent::Approval(item)) => Some(item),
        _ => None,
    }
}

fn group_of<'a>(model: &'a TranscriptModel, id: &str) -> &'a WorkGroup {
    match model.row(id).map(|r| &r.content) {
        Some(RowContent::Group(group)) => group,
        other => panic!("a group at {id}: {other:?}"),
    }
}

// MARK: Grouping rules

#[test]
fn testARunOfOneIsShownAsItself() {
    let model = from_events(vec![user(1, "Go"), call(2, "c1"), result(3, "c1"), text(4, "m1", "Done")]);
    assert_eq!(ids(&model)[..3], ["u:1", "t:c1", "a:m1"]);
}

/// A finished turn is Mike's message, one line of work with everything the
/// agent did, its reply and its cards, like Codex desktop: its progress
/// notes fold away too.
#[test]
fn testAFinishedTurnFoldsAllItsWorkIntoOneLine() {
    let model = from_events(vec![
        user(1, "Go"),
        text(2, "m0", "Looking."),
        call(3, "c1"),
        result(4, "c1"),
        text(5, "m1", "Found it, fixing."),
        call(6, "c2"),
        result(7, "c2"),
        text(8, "m2", "Fixed."),
        done(9),
    ]);
    assert_eq!(ids(&model), ["u:1", "g:a:m0", "a:m2", "f:t1"]);
    let group = group_of(&model, "g:a:m0");
    assert_eq!(group.member_ids, ["a:m0", "t:c1", "a:m1", "t:c2"]);
    assert_eq!(group.summary, "Worked for 4s");
    // Opening it shows everything, in order, above the reply.
    let mut open = model.clone();
    open.set_expanded("g:a:m0", true);
    assert_eq!(ids(&open), ["u:1", "g:a:m0", "a:m0", "t:c1", "a:m1", "t:c2", "a:m2", "f:t1"]);
    // Live, the turn's lines become the one as it finishes.
    let events = model.events().to_vec();
    let mut live = from_events(events[..events.len() - 1].to_vec());
    let mut table = ids(&live);
    let change = live.apply(vec![done(9)]);
    replay(&change, &mut table, &live);
    assert_eq!(ids(&live), ids(&model));
}

#[test]
fn testMessagesCardsPlansImagesAndErrorsEndARun() {
    let model = from_events(vec![
        user(1, "Go"),
        reasoning(2, "r1"),
        call(3, "c1"),
        result(4, "c1"),
        text(5, "m1", "First look done."),
        call(6, "c2"),
        event(7, "plan.updated", json!({"entries": [{"text": "A", "status": "pending"}]})),
        call(8, "c3"),
        call(9, "c4"),
        result_images(10, "c4", json!([{"path": "/tmp/a.png", "mediaType": "image/png"}])),
        call(11, "c5"),
        event(12, "error", json!({"message": "Oops"})),
        call(13, "c6"),
        event(14, "system.note", json!({"text": "Compacted the conversation"})),
        event(15, "approval.requested", json!({"approvalId": "ap1", "name": "Bash", "title": "rm x", "input": {}, "options": ["allow"]})),
    ]);
    assert_eq!(ids(&model), ["u:1", "g:r:r1", "a:m1", "t:c2", "p:7", "g:t:c3", "i:c4", "t:c5", "e:12", "g:t:c6", "ap:ap1"]);
    let group = group_of(&model, "g:t:c6");
    assert_eq!(group.member_ids, ["t:c6", "n:14"], "system notes fold in with the work");
}

#[test]
fn testRunsDoNotCrossTurns() {
    let model = from_events(vec![call(1, "c1"), result(2, "c1"), call_titled(3, "c2", "pwd").with_turn("t2"), call(4, "c3").with_turn("t2")]);
    assert_eq!(ids(&model), ["t:c1", "g:t:c2"]);
}

// MARK: Approvals in runs

/// An approval Mike allowed is a step of the work: it folds into the run
/// with the tool it let run. Waiting, denied and expired ones stay cards
/// between runs.
#[test]
fn testAllowedApprovalsFoldIntoTheRun() {
    let allowed = from_events(vec![
        user(1, "Go"),
        call(2, "c1"),
        result(3, "c1"),
        asked(4, "ap1"),
        resolved(5, "ap1", "allow"),
        call_titled(6, "c2", "rm x"),
        result(7, "c2"),
        text(8, "m1", "Done"),
    ]);
    assert_eq!(ids(&allowed), ["u:1", "g:t:c1", "a:m1"]);
    let group = group_of(&allowed, "g:t:c1");
    assert_eq!(group.member_ids, ["t:c1", "ap:ap1", "t:c2"]);
    assert_eq!(group.summary, "Ran 2 commands", "the approval isn't counted on top of the command it allowed");

    for decision in ["deny", "expired"] {
        let model = from_events(vec![user(1, "Go"), call(2, "c1"), asked(3, "ap1"), resolved(4, "ap1", decision), call(5, "c2")]);
        assert_eq!(ids(&model), ["u:1", "t:c1", "ap:ap1", "t:c2"], "{decision}");
    }
    let waiting = from_events(vec![user(1, "Go"), call(2, "c1"), asked(3, "ap1")]);
    assert_eq!(ids(&waiting), ["u:1", "t:c1", "ap:ap1"]);
}

#[test]
fn testARunOfOnlyAnAllowedApprovalIsShownAsItself() {
    let model = from_events(vec![user(1, "Go"), asked(2, "ap1"), resolved(3, "ap1", "allowAlways")]);
    assert_eq!(ids(&model), ["u:1", "ap:ap1"]);
    let two = from_events(vec![user(1, "Go"), asked(2, "ap1"), resolved(3, "ap1", "allow"), asked(4, "ap2"), resolved(5, "ap2", "allow")]);
    assert_eq!(group_of(&two, "g:ap:ap1").summary, "Allowed 2 requests");
}

/// Allowing the card between two runs joins them into the first one's
/// group, through the same incremental change as any other update: the
/// first group keeps its id, the card and the second group go.
#[test]
fn testAllowingACardMergesTheRunsAroundIt() {
    let mut model = from_events(vec![user(1, "Go"), started(2), call(3, "c1"), call(4, "c2"), asked(5, "ap1")]);
    // A tool that ran in the background while the card waited.
    model.apply(vec![call(6, "c3"), call(7, "c4")]);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "ap:ap1", "g:t:c3"]);
    let mut table = ids(&model);
    let group_version = model.row("g:t:c1").map(|r| r.version);

    let change = model.apply(vec![resolved(8, "ap1", "allow")]);
    replay(&change, &mut table, &model);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "w:t1"]);
    assert_eq!(change.removed, set(&[2, 3]), "the card and the second group go");
    assert_eq!(change.updated, set(&[1]));
    assert_ne!(model.row("g:t:c1").map(|r| r.version), group_version);
    let group = group_of(&model, "g:t:c1");
    assert_eq!(group.member_ids, ["t:c1", "t:c2", "ap:ap1", "t:c3", "t:c4"]);
    assert!(group.is_live);
}

/// A settled approval whose tool call has its own row doesn't open: that
/// row opens to the same file or command. The runner sends the call first;
/// the approval can also come first (an older page, the mock).
#[test]
fn testASettledApprovalWithItsToolsRowDoesNotOpen() {
    let call_first = from_events(vec![user(1, "Go"), edit_with(2, "w", "/a.sh", "a", "b", "Write"), asked_for(3, "ap1", Some("w")), resolved(4, "ap1", "allow"), result(5, "w")]);
    assert_eq!(approval(&call_first, "ap:ap1").map(|a| a.tool_shown), Some(true));
    assert_eq!(approval(&call_first, "ap:ap1").map(|a| a.opens()), Some(false));

    let mut approval_first = from_events(vec![user(1, "Go"), started(2), asked_for(3, "ap1", Some("w"))]);
    assert_eq!(approval(&approval_first, "ap:ap1").map(|a| a.opens()), Some(true), "a waiting card always shows what it asks");
    approval_first.apply(vec![resolved(4, "ap1", "allow")]);
    assert_eq!(approval(&approval_first, "ap:ap1").map(|a| a.opens()), Some(true), "nothing else shows the file yet");
    let version = approval_first.item("ap:ap1").map(|r| r.version);
    approval_first.apply(vec![edit_with(5, "w", "/a.sh", "a", "b", "Write")]);
    assert_eq!(approval(&approval_first, "ap:ap1").map(|a| a.opens()), Some(false));
    assert_ne!(approval_first.item("ap:ap1").map(|r| r.version), version, "the line is measured again without its chevron");
    // The result after it changes nothing more on the approval.
    let settled = approval_first.item("ap:ap1").map(|r| r.version);
    approval_first.apply(vec![result(6, "w")]);
    assert_eq!(approval_first.item("ap:ap1").map(|r| r.version), settled);

    // A subagent's tool has no item id and no row: its approval still opens.
    let subagent = from_events(vec![user(1, "Go"), asked_for(2, "ap1", None), resolved(3, "ap1", "allow")]);
    assert_eq!(approval(&subagent, "ap:ap1").map(|a| a.opens()), Some(true));
    // Denied with its tool shown: the line is the record, the tool row the detail.
    let denied = from_events(vec![user(1, "Go"), edit_with(2, "w", "/a.sh", "a", "b", "Write"), asked_for(3, "ap1", Some("w")), resolved(4, "ap1", "deny")]);
    assert_eq!(approval(&denied, "ap:ap1").map(|a| a.opens()), Some(false));
}

/// An open group shows the allowed approval as one of its lines.
#[test]
fn testAnOpenGroupListsItsAllowedApproval() {
    let mut model = from_events(vec![call(1, "c1"), asked(2, "ap1"), resolved(3, "ap1", "allow"), call(4, "c2")]);
    model.set_expanded("g:t:c1", true);
    assert_eq!(ids(&model), ["g:t:c1", "t:c1", "ap:ap1", "t:c2"]);
    assert_eq!(model.row("ap:ap1").map(|r| r.in_group), Some(true));
}

// MARK: Summaries

#[test]
fn testSummaryCountsWorkInTheOrderItHappened() {
    let model = from_events(vec![
        call_with(1, "c1", "Read", "read", "Read a", json!({"file_path": "/a.swift"})),
        call_with(2, "c2", "Read", "read", "Read b", json!({"file_path": "/b.swift"})),
        call_with(3, "c3", "Read", "read", "Read a again", json!({"file_path": "/a.swift"})),
        call_with(4, "c4", "Grep", "search", "Search", json!({})),
        call_with(5, "c5", "Glob", "search", "Find", json!({})),
        edit(6, "c6", "/a.swift"),
        edit_with(7, "c7", "/b.swift", "a", "b", "Write"),
        edit(8, "c8", "/a.swift"),
        call_titled(9, "c9", "npm test"),
        call_titled(10, "c10", "npm run lint"),
        result_with(11, "c10", false, "lint failed", json!([])),
        reasoning(12, "r1"),
    ]);
    let Some(RowContent::Group(group)) = model.rows().first().map(|r| &r.content) else { panic!("a group") };
    assert_eq!(group.summary, "Read 2 files, searched twice, edited 2 files, ran 2 commands");
    assert_eq!(group.failed_count, 1);
    assert_eq!(group.member_ids.len(), 11);
}

#[test]
fn testSummaryWordsForOtherWork() {
    fn summary(events: Vec<Event>) -> String {
        match from_events(events).rows().first().map(|r| r.content.clone()) {
            Some(RowContent::Group(group)) => group.summary,
            _ => String::new(),
        }
    }
    assert_eq!(summary(vec![reasoning(1, "r1"), reasoning(2, "r2")]), "Thought");
    assert_eq!(summary(vec![call_with(1, "c1", "Task", "task", "Explore", json!({})), reasoning(2, "r1")]), "Ran an agent");
    assert_eq!(
        summary(vec![
            call_with(1, "c1", "WebFetch", "web", "Fetch a", json!({})),
            call_with(2, "c2", "WebFetch", "web", "Fetch b", json!({})),
            call_with(3, "c3", "WebSearch", "web", "Search the web", json!({})),
        ]),
        "Fetched 2 pages, searched once"
    );
    assert_eq!(
        summary(vec![
            call_with(1, "c1", "mcp__notion__search", "mcp", "Notion", json!({})),
            call_with(2, "c2", "Mystery", "other", "?", json!({})),
            call_titled(3, "c3", "ls"),
        ]),
        "Used 2 tools, ran a command"
    );
    assert_eq!(summary(vec![event(1, "system.note", json!({"text": "Resumed session"})), event(2, "system.note", json!({"text": "Compacted"}))]), "Resumed session");
}

// MARK: Live updates

/// The group keeps its id (the first item's) as it grows, so its expansion
/// and cached height survive streaming; each step is a small change, never
/// a reload.
#[test]
fn testAGroupKeepsItsIdAsItGrows() {
    let mut model = from_events(vec![user(1, "Go"), started(2)]);
    let mut table = ids(&model);
    assert_eq!(table, ["u:1", "w:t1"]);

    let mut c = model.apply(vec![reasoning(3, "r1")]);
    replay(&c, &mut table, &model);
    assert_eq!(ids(&model), ["u:1", "r:r1", "w:t1"]);
    assert_eq!(c.inserted, set(&[1]));

    // A second item turns the lone row into a group in its place.
    c = model.apply(vec![call_with(4, "c1", "Bash", "command", "npm test", json!({"command": "npm test"}))]);
    replay(&c, &mut table, &model);
    assert_eq!(ids(&model), ["u:1", "g:r:r1", "w:t1"]);
    assert_eq!(c.removed, set(&[1]));
    assert_eq!(c.inserted, set(&[1]));
    assert_eq!(c.updated, set(&[]), "the working line is left as it is");

    let version = model.rows()[1].version;
    c = model.apply(vec![result(5, "c1"), call_with(6, "c2", "Read", "read", "Read a.swift", json!({}))]);
    replay(&c, &mut table, &model);
    assert_eq!(ids(&model), ["u:1", "g:r:r1", "w:t1"]);
    assert_eq!(c, change(&[], &[], &[1]));
    assert!(model.rows()[1].version > version);
    let group = group_of(&model, "g:r:r1");
    assert_eq!(group.summary, "Ran a command, read a file");
    assert!(group.is_live);
    assert_eq!(group.current_action.as_deref(), Some("Read a.swift"));

    // Text after it ends the run: the group is no longer the live one.
    c = model.apply(vec![result(7, "c2"), delta(7, "m1", "Found")]);
    replay(&c, &mut table, &model);
    assert_eq!(ids(&model), ["u:1", "g:r:r1", "a:m1", "w:t1"]);
    assert_eq!(c.inserted, set(&[2]));
    assert_eq!(c.updated, set(&[1]), "only the group, no longer live");
    let finished = group_of(&model, "g:r:r1");
    assert!(!finished.is_live);
    assert_eq!(finished.current_action, None);

    // Streaming text only touches its own row.
    c = model.apply(vec![delta(7, "m1", " it.")]);
    assert_eq!(c, change(&[], &[], &[2]));
}

#[test]
fn testAClosedGroupIgnoresOutputFromItsTools() {
    let mut model = from_events(vec![started(1), call(2, "c1"), result(3, "c1"), call_titled(4, "c2", "npm test")]);
    assert_eq!(ids(&model), ["g:t:c1", "w:t1"]);
    let c = model.apply(vec![event(4, "tool.output.delta", json!({"itemId": "c2", "text": "PASS a.test.ts\n"}))]);
    assert!(c.is_empty(), "nothing on screen changed");
    let Some(RowContent::Tool(tool)) = model.item("t:c2").map(|r| &r.content) else { panic!("a tool") };
    assert_eq!(tool.live_output, "PASS a.test.ts\n");
}

#[test]
fn testOpeningAGroupInsertsItsItemsUnderIt() {
    let mut model = from_events(vec![user(1, "Go"), reasoning(2, "r1"), call(3, "c1"), result(4, "c1"), text(5, "m1", "Done")]);
    let mut table = ids(&model);
    let version = model.rows()[1].version;
    let mut c = model.set_expanded("g:r:r1", true);
    replay(&c, &mut table, &model);
    assert_eq!(ids(&model), ["u:1", "g:r:r1", "r:r1", "t:c1", "a:m1"]);
    assert_eq!(c.inserted, set(&[2, 3]));
    assert_eq!(c.updated, set(&[1]), "the chevron turns");
    assert_eq!(model.rows().iter().filter(|r| r.in_group).map(|r| r.id.clone()).collect::<Vec<_>>(), ["r:r1", "t:c1"]);
    assert_ne!(model.rows()[1].version, version);
    assert!(group_of(&model, "g:r:r1").is_expanded);

    // A tool that finishes inside an open group updates its own row.
    c = model.apply(vec![result_text(6, "c1", "changed")]);
    assert!(c.updated.contains(&3));
    assert!(c.inserted.is_empty() && c.removed.is_empty());

    // Opening it again is nothing; closing takes its items out.
    assert!(model.set_expanded("g:r:r1", true).is_empty());
    c = model.set_expanded("g:r:r1", false);
    replay(&c, &mut table, &model);
    assert_eq!(c.removed, set(&[2, 3]));
    assert_eq!(ids(&model), ["u:1", "g:r:r1", "a:m1"]);
}

#[test]
fn testAnOpenGroupGainsNewItemsInPlace() {
    let mut model = from_events(vec![started(1), call(2, "c1"), call(3, "c2")]);
    model.set_expanded("g:t:c1", true);
    let mut table = ids(&model);
    let c = model.apply(vec![call(4, "c3")]);
    replay(&c, &mut table, &model);
    assert_eq!(ids(&model), ["g:t:c1", "t:c1", "t:c2", "t:c3", "w:t1"]);
    assert_eq!(c.inserted, set(&[3]));
    assert_eq!(c.updated, set(&[0]));
}

#[test]
fn testOpenGroupsStayOpenWhenOlderEventsArePrepended() {
    let mut model = from_events(vec![user(5, "Go"), call(6, "c1"), call(7, "c2"), text(8, "m1", "Done")]);
    model.set_expanded("g:t:c1", true);
    let result = model.prepend(vec![user(3, "Earlier"), text(4, "m0", "Hi")]);
    assert!(result.keeps_order);
    assert_eq!(result.added, 2);
    assert_eq!(ids(&model), ["u:3", "a:m0", "u:5", "g:t:c1", "t:c1", "t:c2", "a:m1"]);
    assert_eq!(result.changed_ids, Vec::<String>::new(), "the group's content and version are unchanged");
}

/// Whatever order events arrive in, applying them one at a time ends with
/// the rows a fresh load makes, and every change replays exactly.
#[test]
fn testIncrementalRowsMatchAFreshLoad() {
    let events = vec![
        user(1, "Fix it"),
        started(2),
        reasoning(3, "r1"),
        call_with(4, "c1", "Grep", "search", "Search", json!({})),
        result(5, "c1"),
        call_with(6, "c2", "Read", "read", "Read a", json!({"file_path": "/a"})),
        result(7, "c2"),
        text(8, "m1", "Found it."),
        edit(9, "c3", "/a"),
        result(10, "c3"),
        call_titled(11, "c4", "Screenshot"),
        result_images(12, "c4", json!([{"path": "/tmp/s.png", "mediaType": "image/png"}])),
        call_titled(13, "c5", "npm test"),
        result(14, "c5"),
        event(15, "approval.requested", json!({"approvalId": "ap1", "name": "Bash", "title": "rm x", "input": {}, "options": ["allow"]})),
        event(16, "approval.resolved", json!({"approvalId": "ap1", "decision": "allow"})),
        call_titled(17, "c6", "rm x"),
        result(18, "c6"),
        text(19, "m2", "All done."),
        done(20),
    ];
    for open in [false, true] {
        let mut model = TranscriptModel::new(None);
        let mut table: Vec<String> = Vec::new();
        for (step, ev) in events.iter().enumerate() {
            let c = model.apply(vec![ev.clone()]);
            replay(&c, &mut table, &model);
            if open {
                let closed: Vec<String> = model.rows().iter().filter(|r| r.kind() == RowKind::Group && !model.is_expanded(&r.id)).map(|r| r.id.clone()).collect();
                for id in closed {
                    let c = model.set_expanded(&id, true);
                    replay(&c, &mut table, &model);
                }
            }
            let mut fresh = TranscriptModel::new(None);
            fresh.load(events[..=step].to_vec(), &[]);
            let keys: Vec<String> = model.expanded().iter().cloned().collect();
            for key in keys {
                fresh.set_expanded(&key, true);
            }
            assert_eq!(ids(&model), ids(&fresh), "after event {}", step + 1);
            assert_eq!(model.rows().iter().map(|r| &r.content).collect::<Vec<_>>(), fresh.rows().iter().map(|r| &r.content).collect::<Vec<_>>(), "after event {}", step + 1);
        }
    }
}

// MARK: Working line

#[test]
fn testTheWorkingLineShowsWhileATurnRunsAndNotWhileItWaits() {
    let mut model = from_events(vec![user(1, "Go"), started(2), call(3, "c1")]);
    assert_eq!(ids(&model), ["u:1", "t:c1", "w:t1"]);
    let Some(RowContent::Working(status)) = model.rows().last().map(|r| &r.content) else { panic!("working") };
    assert_eq!(status.turn_id, "t1");
    assert_eq!(status.started_at, Some(1_793_008_800.25));
    let mut table = ids(&model);

    let mut c = model.apply(vec![event(4, "approval.requested", json!({"approvalId": "ap1", "name": "Bash", "title": "rm x", "input": {}, "options": ["allow"]}))]);
    replay(&c, &mut table, &model);
    assert_eq!(ids(&model), ["u:1", "t:c1", "ap:ap1"], "waiting on Mike isn't working");

    c = model.apply(vec![event(5, "approval.resolved", json!({"approvalId": "ap1", "decision": "allow"}))]);
    replay(&c, &mut table, &model);
    // Allowed, the card folds into the run with the command before it.
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "w:t1"]);

    c = model.apply(vec![done(6)]);
    replay(&c, &mut table, &model);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "f:t1"]);
    assert_eq!(model.open_turn(), None);
}

/// The working line is the turn's one spinner wherever it sits, so it
/// doesn't change (or re-render) as a live group above it comes, opens and
/// finishes.
#[test]
fn testTheWorkingLineKeepsItsVersionAsWorkComesAndGoes() {
    let working = |model: &TranscriptModel| model.rows().iter().rev().find(|r| r.kind() == RowKind::Working).map(|r| r.version);
    let mut model = from_events(vec![user(1, "Go"), started(2), call(3, "c1")]);
    let version = working(&model);
    assert!(version.is_some());
    model.apply(vec![call(4, "c2")]);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "w:t1"]);
    assert_eq!(working(&model), version, "under the live group");
    model.set_expanded("g:t:c1", true);
    assert_eq!(working(&model), version, "under the open group");
    model.apply(vec![text(5, "m1", "Found it")]);
    assert_eq!(working(&model), version, "under a reply");
}

#[test]
fn testATurnThatStartedOnAnOlderPageIsStillRunning() {
    let model = from_events(vec![call(600, "c1"), result(601, "c1")]);
    assert_eq!(model.open_turn(), Some("t1"));
    let Some(RowContent::Working(status)) = model.rows().last().map(|r| &r.content) else { panic!("no working line") };
    assert_eq!(status.started_at, None, "the start is on a page we haven't loaded");

    // A page that starts with the turn's message would have had its start.
    let finished = from_events(vec![user(600, "Go"), call(601, "c1")]);
    assert_eq!(finished.open_turn(), None);
}

// MARK: Changed files card

#[test]
fn testAChangedFilesCardComesBeforeTheFooter() {
    let mut model = from_events(vec![
        user(1, "Change things"),
        started(2),
        edit_with(3, "c1", "/repo/Sources/A.swift", "a\nb", "a\nc\nd", "Edit"),
        result(4, "c1"),
        edit_with(5, "c2", "/repo/README.md", "a", "# Hi\n\nText\n", "Write"),
        result(6, "c2"),
        edit_with(7, "c3", "/repo/Sources/A.swift", "d", "e", "Edit"),
        result(8, "c3"),
        edit(9, "c4", "/repo/Sources/B.swift"),
        result_with(10, "c4", false, "old_string not found", json!([])),
        text(11, "m1", "Done."),
    ]);
    assert!(model.rows().iter().all(|r| r.kind() != RowKind::Changes), "no card until the turn ends");
    let mut table = ids(&model);
    let c = model.apply(vec![done(12)]);
    replay(&c, &mut table, &model);
    assert_eq!(ids(&model)[ids(&model).len() - 3..], ["a:m1", "cf:t1", "f:t1"]);
    let Some(RowContent::Changes(card)) = model.row("cf:t1").map(|r| &r.content) else { panic!("a card") };
    assert_eq!(card.title(), "Edited 2 files");
    assert_eq!(card.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(), ["/repo/Sources/A.swift", "/repo/README.md"]);
    assert_eq!(card.files.iter().map(|f| f.added).collect::<Vec<_>>(), [3, 3]);
    assert_eq!(card.files.iter().map(|f| f.removed).collect::<Vec<_>>(), [2, 0]);
    assert_eq!(card.files[0].diffs.len(), 2, "both edits to A.swift");
    assert_eq!(card.files[0].name(), "A.swift");
    assert_eq!(card.files[0].folder(), "repo/Sources");

    let version = model.row("cf:t1").unwrap().version;
    let index = model.index_of("cf:t1").unwrap();
    let c = model.set_expanded(&ChangedFiles::expansion_key("t1", "/repo/README.md"), true);
    assert_eq!(c, change(&[], &[], &[index]));
    let Some(RowContent::Changes(opened)) = model.row("cf:t1").map(|r| &r.content) else { panic!("a card") };
    assert_eq!(opened.files.iter().map(|f| f.is_expanded).collect::<Vec<_>>(), [false, true]);
    assert_ne!(model.row("cf:t1").map(|r| r.version), Some(version));
}

/// A file opened in the card shows the first lines of its diff; "Show N
/// more lines" opens the rest through its own key, and closing the file
/// forgets that, so it opens short again.
#[test]
fn testShowingAllOfAChangedFilesDiffRoundTrips() {
    let long: String = (1..=40).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
    let mut model = from_events(vec![user(1, "Write it"), edit_with(2, "c1", "/repo/Long.swift", "a", &long, "Write"), result(3, "c1"), done(4)]);
    fn file(model: &TranscriptModel) -> Option<ChangedFile> {
        match model.row("cf:t1").map(|r| &r.content) {
            Some(RowContent::Changes(card)) => card.files.first().cloned(),
            _ => None,
        }
    }
    let key = ChangedFiles::expansion_key("t1", "/repo/Long.swift");
    let all = DiffPreview::all_key(&key);
    assert_eq!(all, "cf:t1|/repo/Long.swift|all");
    model.set_expanded(&key, true);
    assert_eq!(file(&model).map(|f| f.is_expanded), Some(true));
    assert_eq!(file(&model).map(|f| f.shows_all), Some(false));

    let version = model.row("cf:t1").unwrap().version;
    let index = model.index_of("cf:t1").unwrap();
    let c = model.set_expanded(&all, true);
    assert_eq!(c, change(&[], &[], &[index]), "only the card changes");
    assert_ne!(model.row("cf:t1").map(|r| r.version), Some(version), "so its height is measured again");
    assert_eq!(file(&model).map(|f| f.shows_all), Some(true));
    assert!(model.is_expanded(&all));

    model.set_expanded(&key, false);
    assert_eq!(file(&model).map(|f| f.is_expanded), Some(false));
    assert_eq!(file(&model).map(|f| f.shows_all), Some(false));
    assert!(!model.is_expanded(&all));
    model.set_expanded(&key, true);
    assert_eq!(file(&model).map(|f| f.shows_all), Some(false), "opens short again");
}

#[test]
fn testADiffIsOnlyShortenedWhenThatHidesEnough() {
    assert_eq!(DiffPreview::shown(5), 5);
    assert_eq!(DiffPreview::shown(15), 15, "not 12 and \"Show 3 more lines\"");
    assert_eq!(DiffPreview::shown(18), 18);
    assert_eq!(DiffPreview::shown(19), 12);
    assert_eq!(DiffPreview::shown(400), 12);
}

/// Tool rows keep their open state in the app's own set, by the same rule.
#[test]
fn testDiffPreviewKeysInASet() {
    let mut open = std::collections::HashSet::new();
    DiffPreview::set("t:c1", true, &mut open);
    DiffPreview::set(&DiffPreview::all_key("t:c1"), true, &mut open);
    assert_eq!(open, ["t:c1".to_owned(), "t:c1|all".to_owned()].into_iter().collect());
    DiffPreview::set("t:c1", false, &mut open);
    assert!(open.is_empty());
}

#[test]
fn testAThoughtIsSummedUpByItsFirstSentence() {
    assert_eq!(
        WorkSummary::thought("Uploads are chunked already, so the missing piece is remembering which chunks the server confirmed. I should look at UploadTask first."),
        "Uploads are chunked already, so the missing piece is remembering which chunks the server confirmed."
    );
    assert_eq!(WorkSummary::thought("Hmm. Let me look at the logs first and then decide."), "Hmm. Let me look at the logs first and then decide.", "too short a sentence to stand alone");
    assert_eq!(WorkSummary::thought("One long line with no full stop at all"), "One long line with no full stop at all");
    assert_eq!(WorkSummary::thought("First line only.\nSecond line"), "First line only.");
    assert_eq!(WorkSummary::thought(""), "");
}

/// A diff that lands after its turn finished updates the turn's card, and
/// only the rows for that turn change.
#[test]
fn testALateDiffUpdatesItsTurnsCard() {
    let mut model = from_events(vec![
        user(1, "Edit it"),
        edit(2, "c1", "/a.swift"),
        result(3, "c1"),
        text(4, "m1", "Done"),
        done(5),
        user(6, "Thanks").with_turn("t2"),
        text(7, "m2", "Any time").with_turn("t2"),
        done_turn(8, "t2"),
    ]);
    let mut table = ids(&model);
    let after: Vec<u64> = model.rows()[model.rows().len() - 3..].iter().map(|r| r.version).collect();
    let c = model.apply(vec![event(9, "edit.diff", json!({"itemId": "c1", "path": "/a.swift", "hunks": [
        {"oldStart": 40, "oldLines": 1, "newStart": 40, "newLines": 2, "lines": ["-a", "+b", "+c"]},
    ]}))]);
    replay(&c, &mut table, &model);
    assert_eq!(c.updated, set(&[3]), "its turn's card; the edit is folded in the turn's line");
    assert_eq!(model.rows()[model.rows().len() - 3..].iter().map(|r| r.version).collect::<Vec<_>>(), after, "the next turn's rows are untouched");
    let Some(RowContent::Changes(card)) = model.row("cf:t1").map(|r| &r.content) else { panic!("a card") };
    assert_eq!(card.files.first().map(|f| f.added), Some(2));
}

/// Opening a group near the top of a long thread only touches that group's
/// rows; everything after it keeps its version.
#[test]
fn testOpeningAnEarlyGroupLeavesLaterRowsAlone() {
    let mut events = Vec::new();
    let mut seq = 0;
    for turn in 0..20 {
        let id = format!("t{turn}");
        seq += 1;
        events.push(user(seq, &format!("Go {turn}")).with_turn(&id));
        seq += 1;
        events.push(call(seq, &format!("a{turn}")).with_turn(&id));
        seq += 1;
        events.push(call(seq, &format!("b{turn}")).with_turn(&id));
        seq += 1;
        events.push(text(seq, &format!("m{turn}"), "Done").with_turn(&id));
        seq += 1;
        events.push(done_turn(seq, &id));
    }
    let mut model = from_events(events);
    let mut table = ids(&model);
    let later: Vec<String> = model.rows()[4..].iter().map(|r| format!("{}|{}", r.id, r.version)).collect();
    let c = model.set_expanded("g:t:a0", true);
    replay(&c, &mut table, &model);
    assert_eq!(c, change(&[], &[2, 3], &[1]));
    assert_eq!(model.rows()[6..].iter().map(|r| format!("{}|{}", r.id, r.version)).collect::<Vec<_>>(), later);
    assert_eq!(model.index_of("f:t19"), Some(model.rows().len() - 1), "rows after the group moved down in the index too");
}

#[test]
fn testASummarizedTurnHasNoCard() {
    let model = from_events(vec![
        user(1, "Change things"),
        started(2),
        edit_with(3, "c1", "/repo/Sources/A.swift", "a", "b", "Edit"),
        result(4, "c1"),
        event_turn(5, "assistant.text", json!({"itemId": "m1", "text": "- Changed A", "summary": true}), Some("t1")),
        done(6),
    ]);
    assert!(model.row("a:m1").is_some());
    assert!(model.rows().iter().all(|r| r.kind() != RowKind::Changes));
}

/// One reply: the final message is the reply, as the summary was, so the
/// turn has no card either (it came back once One reply was on).
#[test]
fn testAOneReplyTurnHasNoCard() {
    let model = from_events(vec![
        user(1, "Change things"),
        started(2),
        edit_with(3, "c1", "/repo/Sources/A.swift", "a", "b", "Edit"),
        result(4, "c1"),
        event_turn(5, "assistant.text", json!({"itemId": "m1", "text": "Changed A.", "final": true}), Some("t1")),
        done(6),
    ]);
    assert!(model.row("a:m1").is_some());
    assert!(model.rows().iter().all(|r| r.kind() != RowKind::Changes));
}

fn end_turn_call(seq: i64, id: &str) -> Event {
    call_with(seq, id, "mcp__threadbear__end_turn", "mcp", "end_turn", json!({})).with_turn("t1")
}

/// A sign-off shorter than the bullets loses to them, unless the summary
/// was written with bullet replies off (docs/protocol.md, "The summary").
#[test]
fn testASummaryThatGivesWayLosesToAShortMessage() {
    fn model(gives_way: bool) -> TranscriptModel {
        let mut summary = json!({"itemId": "e1.summary", "text": "- Fixed the loader bug and the tests pass", "summary": true});
        if gives_way {
            summary["givesWay"] = json!(true);
        }
        from_events(vec![
            user(1, "Fix it"),
            started(2),
            call(3, "c1").with_turn("t1"),
            result(4, "c1").with_turn("t1"),
            text(5, "m1", "Fixed it.").with_turn("t1"),
            end_turn_call(6, "e1"),
            result_text(7, "e1", "Noted: this thread is done.").with_turn("t1"),
            event_turn(8, "assistant.text", summary, Some("t1")),
            done(9),
        ])
    }
    assert!(model(false).row("a:e1.summary").is_some());
    assert!(model(false).row("a:m1").is_none(), "folded into the work");
    assert!(model(true).row("a:m1").is_some());
    assert!(model(true).row("a:e1.summary").is_none(), "folded into the work");
}

/// One reply (docs/protocol.md, "One reply"): the message after end_turn is
/// the reply, even under a longer message before it (a narration block or a
/// draft), which folds into the work.
#[test]
fn testOneReplysFinalMessageIsTheReply() {
    let model = from_events(vec![
        user(1, "Why is the bundle so big?"),
        started(2),
        call(3, "c1").with_turn("t1"),
        result(4, "c1").with_turn("t1"),
        event_turn(5, "assistant.text", json!({"itemId": "m1", "text": "A long draft of the answer. ".repeat(20), "narration": true}), Some("t1")),
        end_turn_call(6, "e1"),
        result_text(7, "e1", "Noted: this thread is done. Now write your reply: it's the only text the user sees from this turn.").with_turn("t1"),
        event_turn(8, "assistant.text", json!({"itemId": "m2", "text": "It's moment's locales.", "final": true}), Some("t1")),
        done(9),
    ]);
    assert!(model.row("a:m2").is_some());
    assert!(model.row("a:m1").is_none(), "folded into the work");
}

/// Thread 46de9389 (7 Nov 2026): a background `tb land` finished around
/// end_turn, and the agent carried on and gave a second card. The report
/// before that card is the reply, not "still landing".
#[test]
fn testAfterAWakeTheLastCardsMessageIsTheReply() {
    let wake = json!({"text": "Background command \"Land\" completed (exit code 0)", "wake": true});
    let model = from_events(vec![
        user(1, "Land it"),
        started(2),
        call(3, "c1").with_turn("t1"),
        result(4, "c1").with_turn("t1"),
        event_turn(5, "system.note", wake, Some("t1")),
        text(6, "m1", "Still landing.").with_turn("t1"),
        end_turn_call(7, "e1"),
        result_text(8, "e1", "Noted: this thread is done.").with_turn("t1"),
        call(9, "c2").with_turn("t1"),
        result(10, "c2").with_turn("t1"),
        text(11, "m2", "Landed on main and pushed.").with_turn("t1"),
        end_turn_call(12, "e2"),
        result_text(13, "e2", "Noted: this thread is done.").with_turn("t1"),
        done(14),
    ]);
    assert!(model.row("a:m2").is_some());
    assert!(model.row("a:m1").is_none(), "folded into the work");
}

/// One reply: a reply to the card a wake replaced folds away too.
#[test]
fn testAfterAWakeAnEarlierFinalReplyGivesWay() {
    let wake = json!({"text": "Background command \"Land\" completed (exit code 0)", "wake": true});
    let model = from_events(vec![
        user(1, "Land it"),
        started(2),
        end_turn_call(3, "e1"),
        result_text(4, "e1", "Noted: this thread is done. Now write your reply.").with_turn("t1"),
        event_turn(5, "assistant.text", json!({"itemId": "m1", "text": "Still landing.", "final": true}), Some("t1")),
        event_turn(6, "system.note", wake, Some("t1")),
        call(7, "c2").with_turn("t1"),
        result(8, "c2").with_turn("t1"),
        text(9, "m2", "Landed on main and pushed.").with_turn("t1"),
        end_turn_call(10, "e2"),
        result_text(11, "e2", "Noted: this thread is done. Stop here: your message above is the reply.").with_turn("t1"),
        done(12),
    ]);
    assert!(model.row("a:m2").is_some());
    assert!(model.row("a:m1").is_none(), "folded into the work");
}

/// From Codex's review: the replaced card's long summary doesn't beat the
/// short report under the card after the wake.
#[test]
fn testAfterAWakeAReplacedSummaryGivesWay() {
    let wake = json!({"text": "Background command \"Land\" completed (exit code 0)", "wake": true});
    let model = from_events(vec![
        user(1, "Land it"),
        started(2),
        call(3, "c1").with_turn("t1"),
        result(4, "c1").with_turn("t1"),
        end_turn_call(5, "e1"),
        result_text(6, "e1", "Noted: this thread is done.").with_turn("t1"),
        event_turn(7, "assistant.text", json!({"itemId": "e1.summary", "text": "- Still building, and landing once the check on Tinker Desk passes", "summary": true}), Some("t1")),
        event_turn(8, "system.note", wake, Some("t1")),
        call(9, "c2").with_turn("t1"),
        result(10, "c2").with_turn("t1"),
        text(11, "m2", "Landed and pushed.").with_turn("t1"),
        end_turn_call(12, "e2"),
        result_text(13, "e2", "Noted: this thread is done.").with_turn("t1"),
        done(14),
    ]);
    assert!(model.row("a:m2").is_some());
    assert!(model.row("a:e1.summary").is_none(), "folded into the work");
}

/// From Codex's review: one reply, woken, and stopped with no new card while
/// other work runs. The report shows, beside the earlier reply.
#[test]
fn testAfterAWakeWithNoNewCardTheReportShows() {
    let wake = json!({"text": "Background command \"Land\" completed (exit code 0)", "wake": true});
    let model = from_events(vec![
        user(1, "Land it"),
        started(2),
        end_turn_call(3, "e1"),
        result_text(4, "e1", "Noted: this thread is done. Now write your reply.").with_turn("t1"),
        event_turn(5, "assistant.text", json!({"itemId": "m1", "text": "Still landing.", "final": true}), Some("t1")),
        event_turn(6, "system.note", wake, Some("t1")),
        call(7, "c2").with_turn("t1"),
        result(8, "c2").with_turn("t1"),
        event_turn(9, "assistant.text", json!({"itemId": "m2", "text": "Landed on main and pushed.", "final": true}), Some("t1")),
        done(10),
    ]);
    assert!(model.row("a:m2").is_some());
}

#[test]
fn testATurnWithNoEditsHasNoCard() {
    let model = from_events(vec![user(1, "Hi"), call(2, "c1"), text(3, "m1", "Hello"), done(4)]);
    assert!(model.rows().iter().all(|r| r.kind() != RowKind::Changes));
}
