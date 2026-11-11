// The Rust side of the shared golden files: for every mock-runner fixture,
// the rows this reducer makes from its events must match, row for row, what
// the Swift reducer wrote to tests/golden/<fixture>.json
// (app/Tests/ThreadbearCoreTests/GoldenTranscriptTests.swift, which is the
// definition of right; UPDATE_GOLDEN=1 there rewrites the files).
//
// As in the Swift test: the thread's status plays no part, because the
// Working row comes from the events alone (a turn.started with no
// turn.completed); only the root, background work and loose-ends check
// come from thread.json, as in ThreadSession.open. Nothing in the row text
// depends on the time or the machine, so nothing is normalised out.
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use threadbear_core::reducer::*;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).expect("repo root").to_path_buf()
}

fn golden_rows(model: &TranscriptModel) -> Vec<Value> {
    model.rows().iter().map(golden_row).collect()
}

fn golden_row(row: &Row) -> Value {
    let mut fields = serde_json::Map::new();
    fields.insert("id".into(), json!(row.id));
    fields.insert("kind".into(), json!(row.kind().as_str()));
    fields.insert("text".into(), json!(RowDebug::excerpt(row, 100)));
    if opens(row) {
        fields.insert("expanded".into(), json!(is_expanded(row)));
    }
    fields.insert("inGroup".into(), json!(row.in_group));
    fields.insert("seq".into(), json!(row.seq));
    if let Some(turn) = &row.turn_id {
        fields.insert("turn".into(), json!(turn));
    }
    Value::Object(fields)
}

/// AppDump.opens: the rows that show a chevron.
fn opens(row: &Row) -> bool {
    match &row.content {
        RowContent::Group(_) | RowContent::Tool(_) | RowContent::Reasoning(_) => true,
        RowContent::Approval(item) => item.resolution.is_some() && item.opens(),
        _ => false,
    }
}

/// A group carries its open state; item rows keep theirs in the app's
/// session, which is empty here (TranscriptController.isExpanded).
fn is_expanded(row: &Row) -> bool {
    matches!(&row.content, RowContent::Group(group) if group.is_expanded)
}

fn load_fixture(folder: &Path) -> (Vec<Event>, String, Vec<BackgroundTask>, Option<LooseEnds>) {
    let ndjson = fs::read_to_string(folder.join("events.ndjson")).expect("events.ndjson");
    let events: Vec<Event> = ndjson
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: Value = serde_json::from_str(line).expect("an event line");
            Event::decode(&value).expect("an event object")
        })
        .collect();
    let thread: Value = serde_json::from_str(&fs::read_to_string(folder.join("thread.json")).expect("thread.json")).expect("thread.json parses");
    let cwd = thread.get("cwd").and_then(Value::as_str).unwrap_or("").to_owned();
    let background = thread
        .get("background")
        .and_then(Value::as_array)
        .map(|tasks| tasks.iter().filter_map(BackgroundTask::decode).collect())
        .unwrap_or_default();
    let loose_ends = thread.get("looseEnds").and_then(LooseEnds::decode);
    (events, cwd, background, loose_ends)
}

fn check_rows(name: &str, which: &str, produced: &[Value], expected: Option<&Value>, failures: &mut Vec<String>) {
    let Some(expected) = expected.and_then(Value::as_array) else {
        failures.push(format!("{name} {which}: no rows in the golden file"));
        return;
    };
    let produced_ids: Vec<&str> = produced.iter().map(|r| r["id"].as_str().unwrap_or("")).collect();
    let expected_ids: Vec<&str> = expected.iter().map(|r| r["id"].as_str().unwrap_or("")).collect();
    if produced_ids != expected_ids {
        failures.push(format!("{name} {which}: the row ids differ\n  produced {produced_ids:?}\n  expected {expected_ids:?}"));
        return;
    }
    for (produced_row, expected_row) in produced.iter().zip(expected) {
        if produced_row != expected_row {
            failures.push(format!("{name} {which}: {}\n  produced {produced_row}\n  expected {expected_row}", expected_row["id"]));
        }
    }
}

#[test]
fn every_fixture_matches_its_golden_file() {
    let repo = repo_root();
    let fixtures = repo.join("app/Tools/mock-runner/fixtures");
    let golden = repo.join("desktop/core/tests/golden");
    let mut names: Vec<String> = fs::read_dir(&fixtures)
        .expect("the fixtures folder")
        .map(|e| e.expect("an entry").file_name().to_string_lossy().into_owned())
        .filter(|name| fixtures.join(name).join("events.ndjson").exists())
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no fixtures found");

    let mut failures = Vec::new();
    for entry in fs::read_dir(&golden).expect("the golden folder") {
        let file = entry.expect("an entry").file_name().to_string_lossy().into_owned();
        if let Some(stem) = file.strip_suffix(".json") {
            if !names.iter().any(|n| n == stem) {
                failures.push(format!("{file} has no fixture"));
            }
        }
    }

    for name in &names {
        let (events, cwd, background, loose_ends) = load_fixture(&fixtures.join(name));
        // As ThreadSession.open does for a thread that's opened.
        let mut model = TranscriptModel::new(Some(cwd));
        model.set_loose_ends(loose_ends);
        model.load(events, &[]);
        model.set_background(background);
        let closed = golden_rows(&model);

        // Every group open, so the steps folded in them show.
        let group_ids: Vec<String> = model.rows().iter().filter(|r| r.kind() == RowKind::Group).map(|r| r.id.clone()).collect();
        for id in group_ids {
            model.set_expanded(&id, true);
        }
        let open = golden_rows(&model);

        let golden_path = golden.join(format!("{name}.json"));
        let text = match fs::read_to_string(&golden_path) {
            Ok(text) => text,
            Err(_) => {
                failures.push(format!("{name}: no golden file (write them from the Swift test with UPDATE_GOLDEN=1)"));
                continue;
            }
        };
        let expected: Value = serde_json::from_str(&text).expect("a golden file parses");
        check_rows(name, "closed", &closed, expected.get("closed"), &mut failures);
        check_rows(name, "open", &open, expected.get("open"), &mut failures);
    }
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}
