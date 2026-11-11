// A folder in the side pane's browser, with no UI in it: ports of the Swift
// app's FolderEntry, FolderContents, FolderSort and FolderListing
// (ThreadbearCore/Threads/FolderListing.swift). What's in a folder always comes
// from the runner's `files.list`, so a thread on another Mac works the same.
use crate::inbox::parse_ms;
use serde_json::Value;
use std::cmp::Ordering;

/// One thing in a folder (`FolderEntry`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct FolderEntry {
    pub name: String,
    pub path: String,
    pub is_folder: bool,
    pub hidden: bool,
    /// A file's. A folder has none of its own, as in the Finder.
    pub size: Option<i64>,
    pub modified_ms: Option<i64>,
}

/// What a folder holds (`FolderContents`). `truncated`: the runner said
/// there's more than it would send.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct FolderContents {
    pub path: String,
    pub entries: Vec<FolderEntry>,
    pub truncated: bool,
}

impl FolderContents {
    /// `files.list`'s answer. Tolerant, like the rest of the protocol: an entry
    /// with no name is dropped, and anything else it doesn't say is left out.
    pub fn decode(value: &Value) -> FolderContents {
        let path = value.get("path").and_then(Value::as_str).unwrap_or("").to_owned();
        let entries = value
            .get("entries")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let name = item.get("name").and_then(Value::as_str).filter(|n| !n.is_empty())?.to_owned();
                Some(FolderEntry {
                    path: join(&path, &name),
                    is_folder: item.get("isFolder").and_then(Value::as_bool).unwrap_or(false),
                    hidden: item.get("hidden").and_then(Value::as_bool).unwrap_or_else(|| name.starts_with('.')),
                    // Another runner sends it: a size that isn't a whole number
                    // of bytes is none (Daybreak, 7 Nov).
                    size: item.get("size").and_then(|s| s.as_i64().or_else(|| s.as_f64().filter(|v| v.fract() == 0. && *v >= 0. && *v < 9.0e15).map(|v| v as i64))),
                    modified_ms: item.get("modifiedAt").and_then(Value::as_str).and_then(parse_ms),
                    name,
                })
            })
            .collect();
        FolderContents { path, entries, truncated: value.get("truncated").and_then(Value::as_bool).unwrap_or(false) }
    }
}

/// The column Mike last clicked (`FolderSort.Key`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Name,
    Modified,
    Size,
}

impl SortKey {
    pub fn name(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Modified => "modified",
            SortKey::Size => "size",
        }
    }

    pub fn from_name(name: &str) -> Option<SortKey> {
        [SortKey::Name, SortKey::Modified, SortKey::Size].into_iter().find(|k| k.name() == name)
    }

    /// The column's title and its row in the Sort by menu.
    pub fn title(self) -> &'static str {
        match self {
            SortKey::Name => "Name",
            SortKey::Modified => "Date Modified",
            SortKey::Size => "Size",
        }
    }
}

/// How a folder's rows are ordered (`FolderSort`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FolderSort {
    pub key: SortKey,
    pub ascending: bool,
}

impl Default for FolderSort {
    fn default() -> Self {
        FolderSort { key: SortKey::Name, ascending: true }
    }
}

impl FolderSort {
    /// Folders first whatever the column, then by it. Names sort as the Finder
    /// sorts them (`clip 2` before `clip 10`). A row with nothing in the column
    /// goes last either way, and ties go by name, so the order never jumps when
    /// something else in the folder changes.
    pub fn apply(self, entries: &[FolderEntry]) -> Vec<FolderEntry> {
        let mut rows = entries.to_vec();
        rows.sort_by(|a, b| self.compare(a, b));
        rows
    }

    fn compare(self, a: &FolderEntry, b: &FolderEntry) -> Ordering {
        if a.is_folder != b.is_folder {
            return if a.is_folder { Ordering::Less } else { Ordering::Greater };
        }
        let column = match self.key {
            SortKey::Name => None,
            SortKey::Modified => order(a.modified_ms, b.modified_ms, self.ascending),
            SortKey::Size => order(a.size, b.size, self.ascending),
        };
        if let Some(column) = column.filter(|o| *o != Ordering::Equal) {
            return column;
        }
        let names = standard_compare(&a.name, &b.name);
        if names == Ordering::Equal {
            return a.path.cmp(&b.path);
        }
        // Only the name column's own direction flips the names.
        if self.key == SortKey::Name && !self.ascending { names.reverse() } else { names }
    }
}

fn order<T: Ord>(a: Option<T>, b: Option<T>, ascending: bool) -> Option<Ordering> {
    match (a, b) {
        (None, None) => None,
        (None, _) => Some(Ordering::Greater),
        (_, None) => Some(Ordering::Less),
        (Some(a), Some(b)) => Some(if ascending { a.cmp(&b) } else { b.cmp(&a) }),
    }
}

/// `localizedStandardCompare`, the Finder's order: runs of digits compare as
/// numbers, and letters without regard to case.
pub fn standard_compare(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(d) = it.next_if(|c| c.is_ascii_digit()) {
                        digits.push(d);
                    }
                    digits
                };
                let (da, db) = (take(&mut a), take(&mut b));
                let (na, nb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
                let cmp = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
                if cmp != Ordering::Equal {
                    return cmp;
                }
            }
            (Some(x), Some(y)) => {
                let (lx, ly) = (x.to_lowercase().to_string(), y.to_lowercase().to_string());
                if lx != ly {
                    return lx.cmp(&ly);
                }
                a.next();
                b.next();
            }
        }
    }
}

/// One step of the path bar (`FolderListing.Crumb`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crumb {
    pub name: String,
    pub path: String,
}

fn trimmed(path: &str) -> &str {
    let t = path.trim_end_matches('/');
    if t.is_empty() { "/" } else { t }
}

/// The path bar's steps to `path`: from the home folder when it's under there
/// (the Finder's long run up from the disk says nothing Mike wants to click),
/// else from the top of the disk (`FolderListing.crumbs`).
pub fn crumbs(path: &str, home: &str) -> Vec<Crumb> {
    let home = trimmed(home);
    let path = trimmed(path);
    let mut out = Vec::new();
    let (mut at, rest) = if !home.is_empty() && home != "/" && (path == home || path.starts_with(&format!("{home}/"))) {
        out.push(Crumb { name: name_of(home).to_owned(), path: home.to_owned() });
        (home.to_owned(), &path[home.len()..])
    } else {
        out.push(Crumb { name: "/".into(), path: "/".into() });
        (String::new(), path)
    };
    for part in rest.split('/').filter(|p| !p.is_empty()) {
        at.push('/');
        at.push_str(part);
        out.push(Crumb { name: part.to_owned(), path: at.clone() });
    }
    out
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The folder above, or none at the top of the disk (`FolderListing.parent`).
pub fn parent(path: &str) -> Option<String> {
    let path = trimmed(path);
    if path == "/" || path.is_empty() {
        return None;
    }
    let up = path.rsplit_once('/').map_or("", |(up, _)| up);
    Some(if up.is_empty() { "/".to_owned() } else { up.to_owned() })
}

pub fn join(folder: &str, name: &str) -> String {
    if folder.ends_with('/') { format!("{folder}{name}") } else { format!("{folder}/{name}") }
}

/// A file's size in the Finder's words (`ByteCountFormatter`, file style):
/// decimal units, one decimal only past a megabyte.
pub fn size_words(bytes: i64) -> String {
    const KB: f64 = 1000.;
    let b = bytes as f64;
    if b < KB {
        return if bytes == 1 { "1 byte".to_owned() } else { format!("{bytes} bytes") };
    }
    if b < KB * KB {
        return format!("{} KB", (b / KB).round() as i64);
    }
    if b < KB * KB * KB {
        return format!("{:.1} MB", b / (KB * KB));
    }
    format!("{:.2} GB", b / (KB * KB * KB))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(name: &str, folder: bool, size: Option<i64>, at: Option<i64>) -> FolderEntry {
        FolderEntry { name: name.into(), path: format!("/f/{name}"), is_folder: folder, hidden: false, size, modified_ms: at }
    }

    fn names(rows: &[FolderEntry]) -> Vec<String> {
        rows.iter().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn name_sort_keeps_folders_first_and_numbers_in_order() {
        let rows = [entry("clip 10.mp4", false, None, None), entry("zed", true, None, None), entry("clip 2.mp4", false, None, None), entry("alpha", true, None, None)];
        assert_eq!(names(&FolderSort::default().apply(&rows)), ["alpha", "zed", "clip 2.mp4", "clip 10.mp4"]);
        assert_eq!(names(&FolderSort { key: SortKey::Name, ascending: false }.apply(&rows)), ["zed", "alpha", "clip 10.mp4", "clip 2.mp4"]);
    }

    /// Newest first is how Mike finds what an agent just made.
    #[test]
    fn date_sort_puts_the_undated_last_both_ways() {
        let rows = [entry("old", false, None, Some(1)), entry("none", false, None, None), entry("new", false, None, Some(9)), entry("mid", false, None, Some(5))];
        assert_eq!(names(&FolderSort { key: SortKey::Modified, ascending: false }.apply(&rows)), ["new", "mid", "old", "none"]);
        assert_eq!(names(&FolderSort { key: SortKey::Modified, ascending: true }.apply(&rows)), ["old", "mid", "new", "none"]);
    }

    #[test]
    fn ties_go_by_name_whichever_way_the_column_runs() {
        let rows = [entry("b", false, Some(5), None), entry("a", false, Some(5), None), entry("c", false, Some(9), None)];
        assert_eq!(names(&FolderSort { key: SortKey::Size, ascending: false }.apply(&rows)), ["c", "a", "b"]);
        assert_eq!(names(&FolderSort { key: SortKey::Size, ascending: true }.apply(&rows)), ["a", "b", "c"]);
    }

    fn crumb(name: &str, path: &str) -> Crumb {
        Crumb { name: name.into(), path: path.into() }
    }

    #[test]
    fn crumbs_start_at_home_when_under_it() {
        assert_eq!(crumbs("/Users/me/dev/clips/", "/Users/me"), [crumb("me", "/Users/me"), crumb("dev", "/Users/me/dev"), crumb("clips", "/Users/me/dev/clips")]);
        assert_eq!(crumbs("/Users/me", "/Users/me"), [crumb("me", "/Users/me")]);
    }

    #[test]
    fn crumbs_start_at_the_disk_outside_home() {
        // A sibling that only shares the prefix isn't under home.
        assert_eq!(crumbs("/Users/merle/x", "/Users/me"), [crumb("/", "/"), crumb("Users", "/Users"), crumb("merle", "/Users/merle"), crumb("x", "/Users/merle/x")]);
        assert_eq!(crumbs("/", "/Users/me"), [crumb("/", "/")]);
    }

    #[test]
    fn parent_stops_at_the_top() {
        assert_eq!(parent("/a/b/").as_deref(), Some("/a"));
        assert_eq!(parent("/a").as_deref(), Some("/"));
        assert_eq!(parent("/"), None);
    }

    #[test]
    fn decodes_a_runners_list() {
        let folder = FolderContents::decode(&json!({
            "path": "/w/site",
            "truncated": true,
            "entries": [
                {"name": "docs", "isFolder": true, "hidden": false, "modifiedAt": "2026-11-09T01:02:03.000Z"},
                {"name": ".env", "isFolder": false, "size": 12, "modifiedAt": "2026-11-09T01:02:03.000Z"},
                {"name": "big.bin", "isFolder": false, "size": 1e100},
                {"isFolder": false}
            ]
        }));
        assert_eq!(folder.path, "/w/site");
        assert!(folder.truncated);
        assert_eq!(folder.entries.len(), 3, "the one with no name is dropped");
        assert_eq!(folder.entries[0].path, "/w/site/docs");
        assert!(folder.entries[0].is_folder);
        assert_eq!(folder.entries[0].size, None);
        assert!(folder.entries[1].hidden, "a dotfile with no hidden flag is hidden");
        assert_eq!(folder.entries[1].size, Some(12));
        assert_eq!(folder.entries[2].size, None, "a size of 1e100 isn't a size");
    }

    #[test]
    fn sizes_read_like_the_finder() {
        assert_eq!(size_words(0), "0 bytes");
        assert_eq!(size_words(1), "1 byte");
        assert_eq!(size_words(999), "999 bytes");
        assert_eq!(size_words(1_000), "1 KB");
        assert_eq!(size_words(12_345), "12 KB");
        assert_eq!(size_words(4_500_000), "4.5 MB");
        assert_eq!(size_words(1_530_000_000), "1.53 GB");
    }

    #[test]
    fn base64_round_trips_with_and_without_padding() {
        assert_eq!(base64_decode("SGVsbG8=").unwrap(), b"Hello");
        assert_eq!(base64_decode("SGVsbG8").unwrap(), b"Hello");
        assert_eq!(base64_decode("").unwrap(), b"");
        assert!(base64_decode("not*valid").is_none());
    }

    #[test]
    fn a_file_read_answer_decodes() {
        let read = FileRead::decode(&json!({ "path": "/w/a.md", "size": 5, "modifiedAt": "2026-11-09T00:00:00.000Z", "mediaType": "text/markdown", "data": "SGVsbG8=", "truncated": true })).unwrap();
        assert_eq!(read.data, b"Hello");
        assert_eq!(read.media_type.as_deref(), Some("text/markdown"));
        assert!(read.truncated);
        let same = FileRead::decode(&json!({ "path": "/w/a.md", "unchanged": true, "size": 5, "modifiedAt": "x" })).unwrap();
        assert!(same.unchanged);
        assert!(same.data.is_empty());
        // Neither bytes nor "unchanged" is no answer at all.
        assert!(FileRead::decode(&json!({ "path": "/w/a.md" })).is_none());
    }

    #[test]
    fn picture_sizes_read_from_their_headers() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&[0, 0, 4, 176, 0, 0, 3, 32]);
        assert_eq!(picture_size(&png), Some((1200, 800)));
        let gif = [b"GIF89a".as_slice(), &[40, 0, 30, 0, 0, 0]].concat();
        assert_eq!(picture_size(&gif), Some((40, 30)));
        assert_eq!(picture_size(b"not a picture"), None);
    }
}

// ---------------------------------------------------------------- files.read

/// What `files.read` answers (`FileRead`): the bytes (at most `maxBytes`,
/// `truncated` when the file is longer), or `unchanged` with no bytes when
/// `known` still matched the file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileRead {
    pub path: String,
    pub size: i64,
    pub modified_at: String,
    pub media_type: Option<String>,
    pub data: Vec<u8>,
    pub truncated: bool,
    pub unchanged: bool,
}

impl FileRead {
    /// Neither the bytes nor "unchanged" is no answer at all.
    pub fn decode(value: &Value) -> Option<FileRead> {
        let unchanged = value.get("unchanged").and_then(Value::as_bool).unwrap_or(false);
        let data = value.get("data").and_then(Value::as_str).map(base64_decode);
        let data = match data {
            Some(bytes) => bytes?,
            None if unchanged => Vec::new(),
            None => return None,
        };
        Some(FileRead {
            path: value.get("path").and_then(Value::as_str).unwrap_or("").to_owned(),
            size: value.get("size").and_then(Value::as_i64).unwrap_or(0),
            modified_at: value.get("modifiedAt").and_then(Value::as_str).unwrap_or("").to_owned(),
            media_type: value.get("mediaType").and_then(Value::as_str).map(str::to_owned),
            data,
            truncated: value.get("truncated").and_then(Value::as_bool).unwrap_or(false),
            unchanged,
        })
    }
}

/// Plain base64, as the runner sends a file's bytes; `None` if it isn't.
/// Written out rather than a crate for the one place that reads it.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0;
    for &b in bytes.iter().take_while(|&&b| b != b'=') {
        buffer = (buffer << 6) | value(b)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A picture's size from its header (`ImageHeader`): PNG, JPEG, GIF and WebP.
/// None for anything else.
pub fn picture_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return Some((width, height));
    }
    if bytes.len() >= 10 && bytes.starts_with(b"GIF8") {
        let width = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let height = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
        return Some((width, height));
    }
    if bytes.len() >= 30 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return match &bytes[12..16] {
            b"VP8 " => Some((u16::from_le_bytes([bytes[26], bytes[27]]) as u32 & 0x3fff, u16::from_le_bytes([bytes[28], bytes[29]]) as u32 & 0x3fff)),
            b"VP8L" => {
                let b = &bytes[21..25];
                Some((((b[1] as u32 & 0x3f) << 8 | b[0] as u32) + 1, (((b[3] as u32 & 0xf) << 10) | ((b[2] as u32) << 2) | ((b[1] as u32 & 0xc0) >> 6)) + 1))
            }
            b"VP8X" => Some((1 + (bytes[24] as u32 | (bytes[25] as u32) << 8 | (bytes[26] as u32) << 16), 1 + (bytes[27] as u32 | (bytes[28] as u32) << 8 | (bytes[29] as u32) << 16))),
            _ => None,
        };
    }
    if bytes.len() >= 4 && bytes.starts_with(&[0xff, 0xd8]) {
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            if marker == 0xff {
                i += 1;
                continue;
            }
            let length = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            // Start-of-frame markers (not the DHT, JPEG or arithmetic-coding
            // ones between them) carry the size.
            if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                let height = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let width = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Some((width, height));
            }
            if length < 2 {
                return None;
            }
            i += 2 + length;
        }
    }
    None
}
