// What Swift's String and NSString do that Rust's str doesn't, in one place,
// so the rest of the port can read like the Swift it came from.
//
// Swift counts and cuts text by grapheme cluster (what a person would call a
// character) and compares it with Unicode canonical equivalence. Rust's
// standard library has neither, and this crate takes only serde, so `count`
// and `prefix` work in Unicode scalars: text over a length limit that has
// combining marks, joined emoji or flags in it gets cut a few characters off
// from where Swift cuts it, and nothing else is affected. Using graphemes
// here (the unicode-segmentation crate) is all it would take to change that.

/// `text.count`, in scalars (see the note at the top of the file).
pub fn count(s: &str) -> usize {
    s.chars().count()
}

/// `String(text.prefix(n))`.
pub fn prefix(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `text.dropFirst(n)`.
pub fn drop_first(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

/// `Character.isNewline`, for the separators in `split(whereSeparator:)`:
/// a "\r\n" pair is one Swift character and two of these, and the empty
/// piece between them is dropped by the callers just as Swift drops empties.
pub fn is_newline(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}')
}

/// `CharacterSet.whitespaces`, as Core Foundation defines it: tab, the
/// Unicode space separators and, oddly, U+200B ZERO WIDTH SPACE (a separator
/// when the set was written, and still in Foundation's table).
pub fn is_cf_whitespace(c: char) -> bool {
    matches!(c as u32, 0x20 | 0x09 | 0xA0 | 0x1680 | 0x2000..=0x200B | 0x202F | 0x205F | 0x3000)
}

/// `CharacterSet.newlines`.
pub fn is_cf_newline(c: char) -> bool {
    matches!(c as u32, 0x0A..=0x0D | 0x85 | 0x2028 | 0x2029)
}

/// `trimmingCharacters(in: .whitespaces)`.
pub fn trim_whitespace(s: &str) -> &str {
    s.trim_matches(is_cf_whitespace)
}

/// `trimmingCharacters(in: .newlines)`.
pub fn trim_newlines(s: &str) -> &str {
    s.trim_matches(is_cf_newline)
}

/// `trimmingCharacters(in: .whitespacesAndNewlines)`.
pub fn trim_ws_nl(s: &str) -> &str {
    s.trim_matches(|c| is_cf_whitespace(c) || is_cf_newline(c))
}

/// `s.split(separator: "\n", omittingEmptySubsequences: false)`. Swift's
/// "\r\n" is a single character that isn't equal to "\n", so a CRLF line
/// ending doesn't split there. Tool output from something that writes CRLF
/// shows as one long line in the Swift app too.
pub fn split_lf(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' && (i == 0 || bytes[i - 1] != b'\r') {
            out.push(&s[start..i]);
            start = i + 1;
        }
    }
    out.push(&s[start..]);
    out
}

/// `NSString.lastPathComponent`.
pub fn last_path_component(path: &str) -> &str {
    if path.is_empty() {
        return "";
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/";
    }
    match trimmed.rfind('/') {
        Some(i) => &trimmed[i + 1..],
        None => trimmed,
    }
}

/// `NSString.deletingLastPathComponent`.
pub fn deleting_last_path_component(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.is_empty() { String::new() } else { "/".to_string() };
    }
    match trimmed.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => trimmed[..i].trim_end_matches('/').to_string(),
        None => String::new(),
    }
}

/// `NSString.expandingTildeInPath`. Another user's "~name" isn't expanded.
pub fn expanding_tilde(path: &str) -> String {
    if path == "~" {
        return home_directory();
    }
    match path.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", home_directory()),
        None => path.to_string(),
    }
}

/// `NSHomeDirectory()`.
pub fn home_directory() -> String {
    // With / between its parts, so it matches a path that's been through
    // `forward_slashes` on Windows.
    forward_slashes(&home_directory_as_reported())
}

fn home_directory_as_reported() -> String {
    // Windows sets USERPROFILE; HOME only exists there under Git Bash.
    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default()
}

/// A Windows path (`C:\\x\\y`, `\\\\server\\share`) with `/` between its
/// parts, as the rest of this code writes paths. A runner on Windows reports
/// them this way, and folder names and "relative to the thread" need one
/// separator to look for. Anything else is left exactly as it came: a
/// backslash in a Mac or Linux path is just a character.
pub fn forward_slashes(path: &str) -> String {
    if is_windows_path(path) {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    }
}

/// Whether a path is a Windows one: it starts with a drive or a server share.
pub fn is_windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/')) || path.starts_with("\\\\")
}

#[cfg(test)]
mod windows_path_tests {
    use super::*;

    #[test]
    fn windows_paths_get_forward_slashes_and_others_stay_as_they_are() {
        assert_eq!(forward_slashes(r"C:\work\site\docs\a.md"), "C:/work/site/docs/a.md");
        assert_eq!(forward_slashes(r"\\server\share\a.md"), "//server/share/a.md");
        assert_eq!(forward_slashes(r"/Users/mike/odd\name.txt"), r"/Users/mike/odd\name.txt");
        assert_eq!(forward_slashes(r"notes\today.md"), r"notes\today.md");
    }
}
