// Ports of app/Tests/ThreadbearCoreTests/StretchTests.swift, TurnGapTests.swift
// and WorkLinksTests.swift (the reply rules and finding the step a work: link
// quotes; link parsing belongs to the view, so it isn't here).
#![allow(non_snake_case)]

mod support;

use serde_json::{json, Value};
use support::*;
use threadbear_core::reducer::*;

// `StretchTests.runner`, `mike`, `note`, `summary`, `final`, `text`, `endTurn`.
fn runner(seq: i64, words: &str, turn: &str) -> Event {
    event_turn(seq, "user.message", json!({"text": words, "images": [], "origin": "runner"}), Some(turn))
}

fn mike(seq: i64, words: &str, turn: &str) -> Event {
    mike_from(seq, words, turn, None)
}

fn mike_from(seq: i64, words: &str, turn: &str, origin: Option<&str>) -> Event {
    let mut fields = json!({"text": words, "images": []});
    if let Some(origin) = origin {
        fields["origin"] = Value::from(origin);
    }
    event_turn(seq, "user.message", fields, Some(turn))
}

fn note(seq: i64, words: &str) -> Event {
    event_no_turn(seq, "system.note", json!({"text": words}))
}

fn summary(seq: i64, item: &str, words: &str) -> Event {
    summary_turn(seq, item, words, "t1")
}

fn summary_turn(seq: i64, item: &str, words: &str, turn: &str) -> Event {
    event_turn(seq, "assistant.text", json!({"itemId": item, "text": words, "summary": true}), Some(turn))
}

fn final_text(seq: i64, item: &str, words: &str, turn: &str) -> Event {
    event_turn(seq, "assistant.text", json!({"itemId": item, "text": words, "final": true}), Some(turn))
}

fn turn_text(seq: i64, item: &str, words: &str, turn: &str) -> Event {
    event_turn(seq, "assistant.text", json!({"itemId": item, "text": words}), Some(turn))
}

fn end_turn(seq: i64, id: &str, turn: &str) -> Vec<Event> {
    vec![
        call_with(seq, id, "mcp__threadbear__end_turn", "command", "ls", json!({"needs": "you"})).with_turn(turn),
        result_text(seq + 1, id, "Noted: this thread will wait in Needs you.").with_turn(turn),
    ]
}

fn edit(seq: i64, item: &str, path: &str) -> Event {
    call_with(seq, item, "Edit", "edit", &format!("Edit {path}"), json!({"file_path": path, "old_string": "a", "new_string": "b"}))
}

/// Mike's message, a turn that merges and waits for the app, the restart
/// note, and the runner's turn that checks it.
fn restart_check() -> Vec<Event> {
    vec![
        mike(1, "Say Archived", "t1"),
        note(2, "Put in Threadbear by the librarian."),
        call_input(3, "c1", json!({"command": "./check.sh --app"})).with_turn("t1"),
        result(4, "c1").with_turn("t1"),
        edit(5, "c2", "/r/Sections.swift").with_turn("t1"),
        result(6, "c2").with_turn("t1"),
        summary_turn(7, "e1.summary", "- Merged; I'll check it after the restart.", "t1"),
        done_turn(8, "t1"),
        note(9, "Threadbear app restarted: 3929d8e to c95c95a"),
        runner(10, "The Threadbear app has restarted onto c95c95a.", "t2"),
        call_input(11, "c3", json!({"command": "cat build.json"})).with_turn("t2"),
        result(12, "c3").with_turn("t2"),
        summary_turn(13, "e2.summary", "- Renamed it; [checks pass](work:./check.sh --app); it's live.", "t2"),
        done_turn(14, "t2"),
    ]
}

#[test]
fn testTheRunnersTurnJoinsTheTurnBeforeIt() {
    let model = from_events(restart_check());
    assert_eq!(ids(&model), ["u:1", "g:n:2", "a:e2.summary", "f:t2"]);
    let Some(RowContent::Group(group)) = model.row("g:n:2").map(|r| &r.content) else { panic!("a group") };
    assert_eq!(group.member_ids, ["n:2", "t:c1", "t:c2", "a:e1.summary", "n:9", "u:10", "t:c3"], "the earlier summary, the notes and the runner's message fold into the work");
    assert_eq!(group.summary, "Worked for 8s", "both turns' time");
    let Some(RowContent::Footer(footer)) = model.row("f:t2").map(|r| &r.content) else { panic!("a footer") };
    assert_eq!(footer.duration_ms, 8400.0);
    assert!((footer.cost_usd.unwrap_or(0.0) - 0.24).abs() < 0.0001);
    assert!(model.row("cf:t2").is_none(), "a summarized stretch has no changes card");
}

/// A background job ended after the agent had replied, and the CLI woke it
/// on its own: no message from anyone, so nothing replaced the reply.
#[test]
fn testAWakeUpAfterAReplyLeavesTheReplyOnShow() {
    let mut first = vec![mike(1, "Draw a bear", "t1"), call_input(2, "c1", json!({"command": "swift draw.swift"})).with_turn("t1"), result(3, "c1").with_turn("t1")];
    first.extend(end_turn(4, "e1", "t1"));
    first.extend([final_text(6, "m1", "Here's a test bear.", "t1"), done_turn(7, "t1")]);
    // As it happened: the wake-up wrote a line, was sent back for end_turn
    // and wrote another. And as it goes now: the line is that turn's reply.
    let mut wrote = first.clone();
    wrote.extend([
        event_turn(8, "turn.started", json!({}), Some("t2")),
        event_turn(9, "system.note", json!({"text": "Background command stopped"}), Some("t2")),
        turn_text(10, "m2", "That wait hit its limit, which changes nothing.", "t2"),
    ]);
    wrote.extend(end_turn(11, "e2", "t2"));
    let mut twice = wrote.clone();
    twice.extend([final_text(13, "m3", "Still waiting on you.", "t2"), done_turn(14, "t2")]);
    let rows = ids(&from_events(twice));
    assert!(rows.iter().any(|r| r == "a:m1"), "the real reply stays: {rows:?}");
    assert!(rows.iter().any(|r| r == "a:m3"), "{rows:?}");
    assert!(!rows.iter().any(|r| r == "a:m2"), "{rows:?}");
    let mut plain = wrote;
    plain.push(done_turn(13, "t2"));
    let rows = ids(&from_events(plain));
    assert!(rows.iter().any(|r| r == "a:m1"), "{rows:?}");
    assert!(rows.iter().any(|r| r == "a:m2"), "{rows:?}");
}

/// The runner's own turns take the last summary's place, as their end_turn
/// is told to cover the stretch. A wake-up isn't one of them.
#[test]
fn testAWakeUpsSummaryDoesntTakeTheReplysPlace() {
    let events = vec![
        mike(1, "Fix it", "t1"),
        call_input(2, "c1", json!({"command": "make"})).with_turn("t1"),
        result(3, "c1").with_turn("t1"),
        summary_turn(4, "e1.summary", "- Fixed it, and here's what was wrong.", "t1"),
        done_turn(5, "t1"),
        event_turn(6, "turn.started", json!({}), Some("t2")),
        event_turn(7, "system.note", json!({"text": "Background command stopped"}), Some("t2")),
        summary_turn(8, "e2.summary", "- Nothing changed.", "t2"),
        done_turn(9, "t2"),
    ];
    let rows = ids(&from_events(events));
    assert!(rows.iter().any(|r| r == "a:e1.summary"), "{rows:?}");
    assert!(rows.iter().any(|r| r == "a:e2.summary"), "{rows:?}");
}

/// Each event as it comes: the first turn shows as it did when it finished
/// until the runner's turn is over, then the two become one.
#[test]
fn testLiveTheStretchJoinsUpOnceTheRunnersTurnEnds() {
    let events = restart_check();
    let mut model = from_events(events[..6].to_vec());
    let mut table = ids(&model);
    for ev in &events[6..] {
        let change = model.apply(vec![ev.clone()]);
        replay(&change, &mut table, &model);
        assert_eq!(ids(&model), ids(&from_events(model.events().to_vec())), "after seq {}", ev.seq);
        if ev.seq == 12 {
            assert_eq!(ids(&model), ["u:1", "g:n:2", "a:e1.summary", "f:t1", "u:10", "t:c3"], "while the runner's turn runs, the first turn shows as it did once it finished");
        }
    }
    assert_eq!(ids(&model), ["u:1", "g:n:2", "a:e2.summary", "f:t2"]);
}

#[test]
fn testMikesNextMessageOrAScheduleStartsAStretchOfItsOwn() {
    for origin in [None, Some("phone"), Some("schedule")] {
        let model = from_events(vec![
            mike(1, "Go", "t1"),
            call(2, "c1"),
            result(3, "c1"),
            summary_turn(4, "e1.summary", "- One.", "t1"),
            done(5),
            mike_from(6, "And again", "t2", origin),
            call(7, "c2").with_turn("t2"),
            result(8, "c2").with_turn("t2"),
            summary_turn(9, "e2.summary", "- Two.", "t2"),
            done_turn(10, "t2"),
        ]);
        assert_eq!(ids(&model), ["u:1", "g:t:c1", "a:e1.summary", "f:t1", "u:6", "g:t:c2", "a:e2.summary", "f:t2"], "origin {origin:?}");
    }
}

/// One Mike sent into the running turn went into that turn: it isn't the
/// start of a new stretch.
#[test]
fn testAMessageSentIntoARunningTurnStaysInIt() {
    let model = from_events(vec![mike(1, "Go", "t1"), call(2, "c1"), result(3, "c1"), mike(4, "Also this", "t1"), call(5, "c2"), result(6, "c2"), summary_turn(7, "e1.summary", "- Both.", "t1"), done(8)]);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "u:4", "a:e1.summary", "f:t1"]);
}

/// A runner's turn that ended on a line, not a summary, keeps that line
/// under the summary before it rather than folding it away.
#[test]
fn testARunnersTurnThatEndsOnALineKeepsIt() {
    let model = from_events(vec![
        mike(1, "Go", "t1"),
        call(2, "c1"),
        result(3, "c1"),
        summary_turn(4, "e1.summary", "- Done.", "t1"),
        done(5),
        runner(6, "[Threadbear hook] Typecheck passed", "t2"),
        event_turn(7, "assistant.text", json!({"itemId": "m2", "text": "Hook passed."}), Some("t2")),
        done_turn(8, "t2"),
    ]);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "a:e1.summary", "a:m2", "f:t2"]);
}

/// The runner's turn's summary covers the stretch, so its work links can
/// quote a step from the turn before.
#[test]
fn testAWorkLinkCanPointIntoTheTurnBefore() {
    let model = from_events(restart_check());
    assert_eq!(model.work_step("./check.sh --app", "t2").map(|s| s.0), Some("t:c1".to_owned()));
    assert_eq!(model.work_step("./check.sh --app", "t2").and_then(|s| s.1), Some("g:n:2".to_owned()));
    assert_eq!(model.work_step("cat build.json", "t2").map(|s| s.0), Some("t:c3".to_owned()), "its own steps first");
}

// ---- TurnGapTests ----

/// As it happened: Mike answers, the turn says the researchers are still
/// running, one comes back six minutes later, the other half a minute after
/// that, and the last turn ends with a summary and a reply.
fn research() -> Vec<Event> {
    let mut events = vec![
        mike(1, "Technical Convex video", "t1"),
        at(event_turn(2, "turn.started", json!({}), Some("t1")), "06:16:00"),
        call_with(3, "c1", "Agent", "task", "Mine the repo", json!({})).with_turn("t1"),
        result(4, "c1").with_turn("t1"),
        turn_text(5, "m1", "The two researchers are still running.", "t1"),
        at(done_turn(6, "t1"), "06:17:23"),
        woke(7, "t2", "06:23:00", json!([{"kind": "agent", "description": "Mine LBI repo for video stories"}, {"kind": "shell", "description": "Scrape YouTube"}])),
        note(8, "I read origin/main").with_turn("t2"),
        turn_text(9, "m2", "The repo research is back.", "t2"),
        at(done_turn(10, "t2"), "06:23:14"),
        woke(11, "t3", "06:23:47", json!([{"kind": "agent", "description": "Prior art and demand research"}])),
        turn_text(12, "m3", "Both reports are in.", "t3"),
        call_input(13, "c2", json!({"command": "render"})).with_turn("t3"),
        result(14, "c2").with_turn("t3"),
    ];
    events.extend(end_turn(15, "e3", "t3"));
    events.extend([final_text(17, "m4", "We'd actually got further than I said.", "t3"), at(done_turn(18, "t3"), "06:25:36")]);
    events
}

fn gap_label(model: &TranscriptModel, id: &str) -> String {
    match model.row(id).map(|r| &r.content) {
        Some(RowContent::Gap(gap)) => gap.label(),
        other => panic!("a gap at {id}: {other:?}"),
    }
}

#[test]
fn testAFinishedStretchMarksEachWaitBetweenReplies() {
    let model = from_events(research());
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "a:m1", "gap:t2", "a:m2", "gap:t3", "a:m4", "f:t3"]);
    assert_eq!(gap_label(&model, "gap:t2"), "6 min later · Mine LBI repo for video stories finished");
    assert_eq!(gap_label(&model, "gap:t3"), "33s later · Prior art and demand research finished");
}

/// Each event as it comes matches building from scratch, and the wait shows
/// as soon as the woken turn starts.
#[test]
fn testLiveTheDividerShowsAsTheWokenTurnStarts() {
    let events = research();
    let mut model = from_events(events[..6].to_vec());
    let mut table = ids(&model);
    for ev in &events[6..] {
        let change = model.apply(vec![ev.clone()]);
        replay(&change, &mut table, &model);
        assert_eq!(ids(&model), ids(&from_events(model.events().to_vec())), "after seq {}", ev.seq);
        if ev.seq == 7 {
            assert!(ids(&model).iter().any(|r| r == "gap:t2"), "{:?}", ids(&model));
        }
    }
    assert_eq!(table, ids(&model));
}

/// A woken turn that shows nothing of its own (it only wrote work) has no
/// divider of its own: the next one covers both waits.
#[test]
fn testWaitsWithNothingShownBetweenAreOneDivider() {
    let mut events = vec![
        mike(1, "Go", "t1"),
        at(event_turn(2, "turn.started", json!({}), Some("t1")), "06:00:00"),
        turn_text(3, "m1", "Waiting on two jobs.", "t1"),
        at(done_turn(4, "t1"), "06:01:00"),
        woke(5, "t2", "06:03:00", json!([{"kind": "shell", "description": "npm test"}])),
        call_input(6, "c1", json!({"command": "cat out.log"})).with_turn("t2"),
        result(7, "c1").with_turn("t2"),
    ];
    events.extend(end_turn(8, "e2", "t2"));
    events.extend([
        at(done_turn(10, "t2"), "06:03:10"),
        woke(11, "t3", "06:05:00", json!([{"kind": "shell", "description": "npm run build"}])),
        turn_text(12, "m3", "Both done.", "t3"),
        at(done_turn(13, "t3"), "06:05:05"),
    ]);
    let model = from_events(events);
    let gaps: Vec<&Row> = model.rows().iter().filter(|r| r.kind() == RowKind::Gap).collect();
    assert_eq!(gaps.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["gap:t3"], "{:?}", ids(&model));
    let RowContent::Gap(gap) = &gaps[0].content else { panic!("a gap") };
    assert_eq!(gap.label(), "4 min later · npm test and npm run build finished");
}

/// Mike's own message starts a turn: no divider, his bubble says it.
#[test]
fn testATurnMikeStartedHasNoDivider() {
    let events = vec![
        mike(1, "Hi", "t1"),
        at(event_turn(2, "turn.started", json!({}), Some("t1")), "06:00:00"),
        turn_text(3, "m1", "Hello", "t1"),
        at(done_turn(4, "t1"), "06:00:05"),
        mike(5, "Again", "t2"),
        at(event_turn(6, "turn.started", json!({}), Some("t2")), "07:00:00"),
        turn_text(7, "m2", "Hi again", "t2"),
        at(done_turn(8, "t2"), "07:00:05"),
    ];
    assert!(!from_events(events).rows().iter().any(|r| r.kind() == RowKind::Gap));
}

#[test]
fn testLabels() {
    let gap = |seconds: f64, tasks: Vec<BackgroundTask>| TurnGap { from: 0.0, to: seconds, woke_by: tasks }.label();
    let task = |description: &str| BackgroundTask::new(BackgroundKind::Agent, description, false);
    assert_eq!(gap(0.5, vec![]), "Moments later");
    assert_eq!(gap(45.0, vec![]), "45s later");
    assert_eq!(gap(90.0 * 60.0, vec![]), "1 h 30 min later");
    assert_eq!(gap(2.0 * 3600.0, vec![]), "2 h later");
    assert_eq!(gap(26.0 * 3600.0, vec![]), "A day later");
    assert_eq!(gap(60.0, vec![task("")]), "1 min later · A sub-agent finished");
    assert_eq!(gap(60.0, vec![task("A"), task("B")]), "1 min later · A and B finished");
    assert_eq!(gap(60.0, vec![task("A"), task("B"), task("C")]), "1 min later · A and 2 more finished");
}

// ---- WorkLinksTests ----

fn e1_end_turn(seq: i64) -> Event {
    call_with(seq, "e1", "mcp__threadbear__end_turn", "mcp", "end_turn", json!({}))
}

#[test]
fn testASummaryIsTheReplyAndTheTurnsOtherMessagesFoldIntoItsWork() {
    let mut model = from_events(vec![
        user(1, "Fix the slugs"),
        text(2, "m1", "Looking at the slug code."),
        call_input(3, "c1", json!({"command": "npm test", "description": "Run the tests"})),
        result(4, "c1"),
        text(5, "m2", "Fixed it; committing."),
    ]);
    // Before the summary, the messages show as themselves.
    assert_eq!(ids(&model), ["u:1", "a:m1", "t:c1", "a:m2"]);
    model.apply(vec![summary(6, "e1.summary", "- Fixed the slug bug.\n- [Tests pass](work:npm test)."), done(7)]);
    assert_eq!(ids(&model), ["u:1", "g:a:m1", "a:e1.summary", "f:t1"]);
    let RowContent::Group(group) = &model.rows()[1].content else { panic!("a group") };
    assert_eq!(group.member_ids, ["a:m1", "t:c1", "a:m2"]);
    // Loading the same events from scratch gives the same rows.
    assert_eq!(ids(&from_events(model.events().to_vec())), ids(&model));
}

/// Mike asked what option A and B meant; the turn did no work, only
/// answered, and its bullets said "Explained the choice above" over a folded
/// explanation. The answer is the reply.
#[test]
fn testAnAnswerWithNoStepsIsTheReplyOverItsSummary() {
    let model = from_events(vec![
        user(1, "Can you explain this a bit more?"),
        text(2, "m1", "Sure. It comes down to timing against proof. **Option A:** wait and post it with a short video. **Option B:** post text only, now."),
        e1_end_turn(3),
        result_text(4, "e1", "Noted"),
        summary(5, "e1.summary", "- Explained the choice above"),
        done(6),
    ]);
    assert_eq!(ids(&model), ["u:1", "g:a:e1.summary", "a:m1", "f:t1"]);
}

/// A Sentry triage turn's report came after its last step (and a pin), and
/// folded away under bullets that never said what was broken. Everything
/// before the last step is the work.
#[test]
fn testTheReportAfterTheLastStepIsTheReplyOverAShorterSummary() {
    let model = from_events(vec![
        user(1, "Triage this"),
        text(2, "m1", "Committing and opening the PR."),
        call_input(3, "c1", json!({"command": "git commit -qm fix"})),
        result(4, "c1"),
        call_with(5, "c2", "ToolSearch", "search", "Find tools", json!({})),
        result(6, "c2"),
        call_with(7, "p1", "mcp__threadbear__pin", "mcp", "Pin PR #2", json!({})),
        result(8, "p1"),
        text(9, "m2", "**What's broken:** every sync since 22 Oct has failed. **My recommendation:** the planner is wrong and the guard is right."),
        e1_end_turn(10),
        result_text(11, "e1", "Noted"),
        summary(12, "e1.summary", "- PR #2 opened"),
        done(13),
    ]);
    assert_eq!(ids(&model), ["u:1", "g:a:m1", "a:m2", "f:t1"]);
    let RowContent::Group(group) = &model.rows()[1].content else { panic!("a group") };
    assert_eq!(group.member_ids, ["a:m1", "t:c1", "t:c2", "t:p1", "a:e1.summary"]);
}

/// A narration block is only the API's paraphrase of what the agent wrote,
/// so it never beats the bullets, however long it is.
#[test]
fn testANarrationParaphraseNeverBeatsTheSummary() {
    let model = from_events(vec![
        user(1, "More names?"),
        call_input(2, "c1", json!({"command": "which browsy clicky"})),
        result(3, "c1"),
        event(4, "assistant.text", json!({"itemId": "m1", "text": "I've checked these whimsical name pairs for clashes and recommend Browsy McBrowseface as the clearest fit, noting the old name is still in review.", "narration": true})),
        e1_end_turn(5),
        result_text(6, "e1", "Noted"),
        summary(7, "e1.summary", "- Five name pairs\n- My pick: Browsy"),
        done(8),
    ]);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "a:e1.summary", "f:t1"]);
}

/// The runner's "the app restarted" landed between an end_turn that asked
/// for a narration's words and the words themselves. It starts a fresh
/// answer: the agent's last words after its last step are the reply, not the
/// narration before the restart.
#[test]
fn testAMessageSentIntoTheTurnStartsAFreshAnswer() {
    let model = from_events(vec![
        user(1, "Set up end-to-end tests"),
        call_input(2, "c1", json!({"command": "./check.sh"})),
        result(3, "c1"),
        event(4, "assistant.text", json!({"itemId": "m1", "text": "Added an end-to-end test that walks all 14 tour steps on a real runner.", "narration": true})),
        e1_end_turn(5),
        result_text(6, "e1", "Noted. Mike got only a short summary"),
        event(7, "user.message", json!({"text": "The app has restarted onto 062f66a", "images": [], "origin": "runner"})),
        text(8, "m2", "The tour now has an end-to-end test against a real runner, and the fixes are on main."),
        call_input(9, "c2", json!({"command": "tb status"})),
        result(10, "c2"),
        event(11, "assistant.text", json!({"itemId": "m3", "text": "M5 is updated with the fixes and Tinker Desk is still building.", "narration": true})),
        call_with(12, "e2", "mcp__threadbear__end_turn", "mcp", "end_turn", json!({})),
        result_with(13, "e2", false, "Already ended", json!([])),
        text(14, "m4", "Your M5 app is now on 062f66a, with the fixes the end-to-end test found. Tinker Desk is still building."),
        done(15),
    ]);
    let shown: Vec<String> = model.rows().iter().filter(|r| matches!(r.content, RowContent::Assistant(_))).map(|r| r.id.clone()).collect();
    assert_eq!(shown, ["a:m4"]);
}

/// Text after end_turn never takes the answer's place: an agent wrote its
/// "What I need from you" there, and it showed instead of the explanation
/// before it.
#[test]
fn testTextAfterEndTurnFoldsAndTheAnswerBeforeItStays() {
    let model = from_events(vec![
        user(1, "What can we do?"),
        call_input(2, "c1", json!({"command": "sed -n 1,80p needs.ts"})),
        result(3, "c1"),
        text(4, "m1", "Two fixes."),
        e1_end_turn(5),
        result_text(6, "e1", "Noted"),
        text(7, "m2", "**What I need from you**\n1. Pick one."),
        done(8),
    ]);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "a:m1", "f:t1"]);
}

/// Asked for the details for a call to Qantas, Opus called end_turn first,
/// with "Put together the details" as a bullet, and wrote them after it.
/// With no words before end_turn, the message after it is the reply; a
/// sign-off shorter than the bullets isn't.
#[test]
fn testTheMessageAfterBulletsThatStoodForItIsTheReply() {
    let model = from_events(vec![
        user(1, "Give me the details for the call"),
        call_input(2, "c1", json!({"command": "browserbear read 1"})),
        result(3, "c1"),
        e1_end_turn(4),
        result_text(5, "e1", "Noted"),
        summary(6, "e1.summary", "- Put together the details you'll need for the call."),
        text(7, "m1", "**Phone:** 13 13 13\n\n**Bookings:** EXBEEI (you), E9SMYP (Kelsie and Joshua), last name CANN"),
        done(8),
    ]);
    assert_eq!(ids(&model), ["u:1", "g:t:c1", "a:m1", "f:t1"]);
    let RowContent::Group(group) = &model.rows()[1].content else { panic!("a group") };
    assert_eq!(group.member_ids, ["t:c1", "a:e1.summary"]);

    let short = from_events(vec![
        user(1, "Fix it"),
        call_input(2, "c1", json!({"command": "npm test"})),
        result(3, "c1"),
        e1_end_turn(4),
        result_text(5, "e1", "Noted"),
        summary(6, "e1.summary", "- Fixed the slug bug; tests pass."),
        text(7, "m1", "Done."),
        done(8),
    ]);
    assert_eq!(ids(&short), ["u:1", "g:t:c1", "a:e1.summary", "f:t1"]);
}

#[test]
fn testATurnWithoutASummaryKeepsItsMessages() {
    let model = from_events(vec![
        user(1, "Hi"),
        text(2, "m1", "Hello."),
        done(3),
        user(4, "Fix it"),
        event_turn(5, "turn.started", json!({}), Some("t2")),
        event_turn(6, "assistant.text", json!({"itemId": "m2", "text": "On it."}), Some("t2")),
        summary_turn(7, "e2.summary", "- Fixed.", "t2"),
    ]);
    assert!(model.rows().iter().any(|r| r.id == "a:m1"), "the first turn has no summary");
    assert!(!model.rows().iter().any(|r| r.id == "a:m2"), "the second does");
}

#[test]
fn testAWorkLinkFindsTheStepItQuotes() {
    let model = from_events(vec![
        user(1, "Fix the slugs"),
        call_input(2, "c1", json!({"command": "cat src/slug.js"})),
        result(3, "c1"),
        call_input(4, "c2", json!({"command": "sed -i '' 's/a/b/' package.json && npm test 2>&1 | tail -5", "description": "Fix import typo and rerun"})),
        result_text(5, "c2", "OpenAI error 401"),
        call_input(6, "c3", json!({"command": "git commit -qm 'Strip accents; npm test passes'"})),
        result(7, "c3"),
        text(8, "m1", "Found it: the car check restarts GPS every 5 s."),
        summary(9, "e1.summary", "- Done."),
        done(10),
    ]);
    let step = |quote: &str, turn: &str| model.work_step(quote, turn).map(|s| s.0);
    // A command that starts with the words beats one that only mentions them.
    assert_eq!(step("npm test", "t1").as_deref(), Some("t:c2"));
    assert_eq!(step("Fix import typo and rerun", "t1").as_deref(), Some("t:c2"));
    assert_eq!(step("OpenAI error 401", "t1").as_deref(), Some("t:c2"));
    assert_eq!(step("restarts GPS every 5 s", "t1").as_deref(), Some("a:m1"));
    assert_eq!(step("src/slug.js", "t1").as_deref(), Some("t:c1"));
    assert_eq!(step("nothing like it", "t1"), None);
    assert_eq!(step("npm test", "t9"), None);
    // The step is folded in the turn's group, which is what to open.
    assert_eq!(model.work_step("npm test", "t1").and_then(|s| s.1).as_deref(), Some("g:t:c1"));
}

#[test]
fn testAWorkLinkFindsACodexComputerUseStepByItsTitle() {
    // Codex names each computer-use step in the call's input; the row's own
    // title is just "cua_repl: js".
    let model = from_events(vec![
        user(1, "ready"),
        call_with(2, "c1", "mcp__cua_repl__js", "mcp", "cua_repl: js", json!({"code": "let app = await cua.getApp(\"com.google.Chrome\");", "timeout_ms": 60000, "title": "Reconnect to Chrome desktop"})),
        result(3, "c1"),
        call_with(4, "c2", "mcp__cua_repl__js", "mcp", "cua_repl: js", json!({"code": "await app.click(699); await app.getAXState();", "title": "Check Google verification request"})),
        result(5, "c2"),
        summary(6, "e1.summary", "- [Chrome desktop control now works](work:Reconnect to Chrome desktop)."),
        done(7),
    ]);
    assert_eq!(model.work_step("Reconnect to Chrome desktop", "t1").map(|s| s.0).as_deref(), Some("t:c1"));
    assert_eq!(model.work_step("Check Google verification request", "t1").map(|s| s.0).as_deref(), Some("t:c2"));
}

#[test]
fn testTheAgentDraftingItsLinksDoesNotTakeThemFromTheStep() {
    // Opus drafts its summary, links and all, in its thinking just before
    // end_turn, and at times in a message too, so they're the latest mentions
    // of the words. The link still means the step.
    let link = "[The beep means no matching step](work:Read recent work-link clicks from the app's unified log)";
    let model = from_events(vec![
        user(1, "Why the beep?"),
        call_input(2, "c1", json!({"command": "/usr/bin/log show --last 2h", "description": "Read recent work-link clicks from the app's unified log"})),
        result_text(3, "c1", "work link: no step"),
        call_input(4, "c2", json!({"command": "tb events d8c829e8"})),
        result_text(5, "c2", "the slug regex drops accents"),
        event(6, "reasoning", json!({"itemId": "r1", "text": format!("First bullet: {link}. The slug regex drops accents.")})),
        text(7, "m1", &format!("- {link}.")),
        summary(8, "e1.summary", &format!("- {link}.")),
        done(9),
    ]);
    let step = |quote: &str| model.work_step(quote, "t1").map(|s| s.0);
    assert_eq!(step("Read recent work-link clicks from the app's unified log").as_deref(), Some("t:c1"));
    // A thought loses to a step's output as well...
    assert_eq!(step("the slug regex drops accents").as_deref(), Some("t:c2"));
    // ...but still answers when nothing else mentions the words.
    assert_eq!(step("First bullet").as_deref(), Some("r:r1"));
}
