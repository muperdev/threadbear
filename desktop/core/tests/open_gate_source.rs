// Nothing in the desktop apps hands a link or a file to the system except the
// open gate (desktop/gpui/src/open_gate.rs), as OpenGateSourceTests is for the
// Swift app. Each of Daybreak's findings 58, 59 and 64 (9 Nov 2026) was a new
// button or link that called the system's opener itself, with a URL made by
// pasting a path into `file://`.
use std::path::{Path, PathBuf};

/// What reaches the system's opener, or starts a program, directly. The
/// kit's own rich text, links and menu links count: with no handler of their
/// own they call `open_url` themselves (Codex review of the 10 Nov fix), so
/// they're made only by the gate's `text_view` and `markdown_view`.
const OPENERS: &[&str] = &[
    ".open_url(",
    ".reveal_path(",
    "open_with_system",
    "Command::new(",
    "NSWorkspace",
    "ShellExecute",
    "xdg-open",
    "format!(\"file://",
    "TextView::new(",
    "TextView::markdown(",
    "TextView::html(",
    "PopupMenuItem::link(",
    "Link::new(",
    "EditorHost",
];

/// The gate itself, and a test helper that starts the mock runner.
const ALLOWED: &[&str] = &["gpui/src/open_gate.rs", "client/tests/mock_runner.rs"];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target" || n == "golden") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn nothing_opens_outside_the_gate() {
    let desktop = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_owned();
    let mut files = Vec::new();
    for crate_dir in ["gpui", "dock", "client", "core/src"] {
        rust_files(&desktop.join(crate_dir), &mut files);
    }
    assert!(files.iter().any(|f| f.ends_with("gpui/src/open_gate.rs")), "the gate wasn't found, so this test isn't looking in the right place");
    let mut found = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&desktop).unwrap().to_string_lossy().replace('\\', "/");
        if ALLOWED.contains(&rel.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap();
        for (n, line) in text.lines().enumerate() {
            // Comments may name them; `file://` itself has a `//` in it, so
            // only whole comment lines are passed over.
            if line.trim_start().starts_with("//") {
                continue;
            }
            for opener in OPENERS {
                if line.contains(opener) {
                    found.push(format!("{rel}:{}: {}", n + 1, line.trim()));
                }
            }
        }
    }
    assert!(found.is_empty(), "open links and files through open_gate.rs (open_web, open_link, open_file, reveal), not directly:\n{}", found.join("\n"));
}

/// The gate's own sink is the only `open_url` in it, so everything in it
/// goes through the checks above it.
#[test]
fn the_gate_opens_in_one_place() {
    let gate = Path::new(env!("CARGO_MANIFEST_DIR")).join("../gpui/src/open_gate.rs");
    let text = std::fs::read_to_string(gate).unwrap();
    assert_eq!(text.matches(".open_url(").count(), 1);
    assert!(!text.contains("format!(\"file://"));
}
