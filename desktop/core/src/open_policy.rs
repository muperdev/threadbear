// What the GPUI app hands to the system, and when it asks first: the rules of
// the Swift app's OpenGate and LinkPolicy (SECURITY.md, "Your Mac and phone
// from links and files"), with no UI in them so they can be tested anywhere.
// `desktop/gpui/src/open_gate.rs` is the only place that acts on them, and
// `tests/open_gate_source.rs` fails on an opener anywhere else.
//
// A link or path can come from an agent's words, a cloned repo's skill, a
// patch, the backend or another Mac's runner, and the system opener runs
// whatever app it has for the target: a `file://` URL to a `.command` runs it
// in Terminal, an `.app` launches, and a `#` left unescaped in a URL built by
// pasting a path in makes `Inspect.app#help/SKILL.md` open `Inspect.app`
// (Daybreak, 9 Nov 2026, findings 58, 59 and 64). So:
//
// - web addresses (http, https with a host) open in the browser;
// - any other scheme starts its app only once Mike says yes to that link;
// - a file is looked at after its symlinks are followed, and that real file
//   is what opens, through a URL built here from the path, never by pasting
//   a path into `file://`. A known kind of document opens; anything else (a
//   program, a bundle, a script, a kind it can't place, a document whose app
//   runs it) asks first, and another Mac's path never opens here.
use std::path::{Path, PathBuf};

/// Whose link or file it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// This runner's thread, a pin, a folder Mike's browsing: a program asks.
    ThisMac,
    /// A thread on another runner. Its paths name files on that machine, so
    /// none opens here; its web links still do, and other links ask.
    OtherMac,
}

/// What a link is, for opening it (`LinkPolicy.Kind`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// http or https with a host: the browser, without asking.
    Web(String),
    /// A `file:` URL or a bare path: its decoded path, without the ?query or
    /// #fragment, which never name part of the file.
    File(String),
    /// Any other scheme (`mailto:`, `slack:`): it starts another app, which
    /// acts on what the link says, so only once Mike says yes to it.
    Ask(String),
    /// Nothing that can be opened.
    Refuse,
}

/// The longest link looked at (`LinkPolicy.web`'s limit).
const MAX_LINK: usize = 8_192;

/// What `url` is. A link with no scheme is a path, relative or not; the
/// caller resolves a relative one against the thread's folder.
pub fn link_kind(url: &str) -> LinkKind {
    let url = url.trim();
    if url.is_empty() || url.len() > MAX_LINK || url.chars().any(char::is_control) {
        return LinkKind::Refuse;
    }
    match scheme(url) {
        // A link to a place in the page itself (`#usage`) names no file.
        None => match url_path(url) {
            path if path.is_empty() => LinkKind::Refuse,
            path => LinkKind::File(path),
        },
        Some(s) if s == "http" || s == "https" => {
            if crate::reducer::event::is_web_address(url) {
                LinkKind::Web(url.to_owned())
            } else {
                LinkKind::Refuse
            }
        }
        Some(s) if s == "file" => {
            let rest = &url[5..];
            // file:///a/b and file:/a/b name /a/b; file://host/a names a
            // file on another machine, which isn't opened from here.
            let path = if let Some(after) = rest.strip_prefix("//") {
                let (host, path) = after.find('/').map_or((after, ""), |i| (&after[..i], &after[i..]));
                if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                    return LinkKind::Refuse;
                }
                path
            } else {
                rest
            };
            let path = url_path(path);
            if path.is_empty() {
                LinkKind::Refuse
            } else {
                LinkKind::File(path)
            }
        }
        // A Windows path's drive letter reads as a one-letter scheme.
        Some(s) if s.len() == 1 && url[2..].starts_with(['/', '\\']) => LinkKind::File(url.to_owned()),
        Some(_) => LinkKind::Ask(url.to_owned()),
    }
}

/// The scheme of a URL, lowercased, or None for a path.
fn scheme(url: &str) -> Option<String> {
    let (s, _) = url.split_once(':')?;
    let mut chars = s.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')) {
        return None;
    }
    Some(s.to_ascii_lowercase())
}

/// The path part of a URL: cut at the first `?` or `#` before anything is
/// decoded, as a URL parser does, so a `%23` in a name stays part of it and
/// what's opened is what was looked at.
fn url_path(url: &str) -> String {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    percent_decode(&url[..end])
}

/// `%20` and the like. Bytes that don't make UTF-8 are replaced, never dropped.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(v) = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A path as a `file://` URL: every byte but letters, digits and `/-_.~`
/// percent-encoded, so `#`, `?`, `%` and spaces stay part of the name. The
/// only way the app makes a file URL (`URL(fileURLWithPath:)`).
pub fn file_url(path: &str) -> String {
    // A backslash is a separator only on Windows. Elsewhere it's part of a
    // name, and turning it into `/` would make the URL name another file
    // than the one looked at: `Payload.app\` is a plain folder beside the
    // app `Payload.app` (Codex review, 10 Nov 2026).
    let mut path = if cfg!(windows) { path.replace('\\', "/") } else { path.to_owned() };
    if !path.starts_with('/') {
        // C:/Users/... is file:///C:/Users/...
        path.insert(0, '/');
    }
    let mut out = String::with_capacity(path.len() + 7);
    out.push_str("file://");
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            // The drive's colon, left as it is.
            b':' if cfg!(windows) && out.len() == 9 => out.push(':'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `path` made absolute: `~/` from home, a relative one from the thread's
/// folder. None when a relative path has no folder to be from.
pub fn absolute(path: &str, cwd: &str, home: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix("~/") {
        return (!home.is_empty()).then(|| format!("{}/{rest}", home.trim_end_matches('/')));
    }
    if path.starts_with('/') || crate::reducer::text::is_windows_path(path) {
        return Some(path.to_owned());
    }
    if cwd.is_empty() {
        return None;
    }
    Some(format!("{}/{}", cwd.trim_end_matches(['/', '\\']), path.trim_start_matches("./")))
}

// ---------------------------------------------------------------- files

/// Extensions macOS opens by running something (`LinkPolicy.codeExtensions`),
/// and the other systems' programs and scripts. Never opened without asking,
/// whatever app is set for them.
pub fn runs_code(path: &str) -> bool {
    crate::highlights::runs_code(path)
}

/// Kinds of file that open to be looked at: pictures, films, sound, PDFs,
/// text and source code, office documents and fonts (`OpenGate.documentTypes`).
/// Scripts aren't here: Python Launcher and the like run them as they open.
/// Anything not here asks, as an app can claim any extension.
const DOCUMENT_EXTENSIONS: &[&str] = &[
    // pictures
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "ico", "svg", "avif",
    // films and sound
    "mp4", "mov", "m4v", "webm", "mkv", "avi", "mp3", "wav", "m4a", "aac", "aiff", "aif", "flac", "ogg", "opus",
    // PDFs and text
    "pdf", "txt", "text", "md", "markdown", "mdx", "rtf", "csv", "tsv", "log", "json", "jsonc", "jsonl", "yaml", "yml", "toml", "ini", "xml", "plist", "html", "htm", "css",
    "scss", "less", "diff", "patch", "srt", "vtt",
    // source code that's compiled, not run as it opens
    "swift", "rs", "ts", "tsx", "jsx", "c", "h", "m", "mm", "cc", "cpp", "hpp", "cs", "java", "kt", "kts", "go", "sql", "graphql", "proto", "gradle",
    // office documents
    "doc", "docx", "xls", "xlsx", "ppt", "pptx", "key", "numbers", "pages", "odt", "ods", "odp", "rtfd",
    // fonts
    "ttf", "otf", "woff", "woff2",
];

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn is_document(path: &Path) -> bool {
    DOCUMENT_EXTENSIONS.contains(&extension(path).as_str())
}

/// Apps that run the file they're given (`LinkPolicy.codeRunnerApps`).
const CODE_RUNNER_APPS: &[&str] = &[
    "com.apple.terminal", "com.googlecode.iterm2", "org.python.pythonlauncher", "com.apple.installer", "com.apple.shortcuts", "com.apple.jarlauncher",
    "com.apple.automator.automator-application-stub", "com.apple.systempreferences", "com.apple.systemsettings", "com.apple.scripteditor.id.scriptapplet",
    "net.kovidgoyal.kitty", "dev.warp.warp-stable", "com.mitchellh.ghostty", "io.alacritty", "co.zeit.hyper", "com.github.wez.wezterm", "com.panic.prompt3",
];

pub fn is_code_runner(bundle_id: &str) -> bool {
    CODE_RUNNER_APPS.contains(&bundle_id.to_ascii_lowercase().as_str())
}

/// The app the system would open a file in, as far as this machine can say:
/// its bundle id and name on a Mac. The GPUI app asks Launch Services; the
/// tests hand in their own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpeningApp {
    /// Where the app is: the file itself when it's an app.
    pub path: PathBuf,
    pub bundle_id: Option<String>,
    pub name: String,
}

/// What looking at a file found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    /// The real file, symlinks followed: what opens.
    pub resolved: PathBuf,
    /// Why opening it would or might run something, or None.
    pub runs: Option<String>,
    /// The path given was a symlink, or went through one.
    pub linked: bool,
}

/// Looks at the file at `path` (absolute) after following its symlinks.
/// None when nothing's there. `app_for` says which app would open the real
/// file (None where the system can't say).
pub fn look(path: &Path, app_for: &dyn Fn(&Path) -> Option<OpeningApp>) -> Option<Look> {
    let resolved = std::fs::canonicalize(path).ok()?;
    let meta = std::fs::metadata(&resolved).ok()?;
    let linked = std::fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false) || resolved.file_name() != path.file_name();
    let runs = runs(&resolved, &meta, app_for);
    Some(Look { resolved, runs, linked })
}

/// Why opening `file` would or might run something (`OpenGate.runs`).
fn runs(file: &Path, meta: &std::fs::Metadata, app_for: &dyn Fn(&Path) -> Option<OpeningApp>) -> Option<String> {
    let name = file.to_string_lossy();
    if runs_code(&name) {
        return Some(format!("It's a .{} file, which runs when it's opened.", extension(file)));
    }
    if meta.is_dir() {
        // A bundle is a folder macOS launches or installs rather than shows.
        if file.join("Contents/Info.plist").exists() || file.join("Contents/MacOS").exists() || file.join("Info.plist").exists() {
            return Some("It's an app or a bundle, which runs when it's opened.".to_owned());
        }
    }
    let app = app_for(file);
    if let Some(app) = &app {
        // An app opens as itself: Launch Services names a bundle with no
        // known extension that way too.
        if app.path == file {
            return Some("It's a program.".to_owned());
        }
        if app.bundle_id.as_deref().is_some_and(is_code_runner) {
            return Some(format!("It opens in {}, which runs it.", app.name));
        }
    }
    // A plain folder goes to the file browser.
    if meta.is_dir() {
        return None;
    }
    if is_document(file) {
        return None;
    }
    if is_executable(meta) {
        return Some("It's marked as a program, so opening it may run it.".to_owned());
    }
    let opener = app.map(|a| format!(" It opens in {},", a.name)).unwrap_or_default();
    Some(format!("Threadbear can't tell what this kind of file does when it's opened.{opener} which may run what's in it."))
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &std::fs::Metadata) -> bool {
    false
}

/// What to do with a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileVerdict {
    /// Open it: the URL is the real file's, built by `file_url`.
    Open { url: String, resolved: PathBuf },
    /// Ask first, then open `url` only on a yes (and only if the file is
    /// still what was asked about: `still_same`).
    Ask { question: Question, url: String, look: Look },
    /// Don't, and say why.
    Refuse(String),
}

/// What asks Mike: the question's words and its yes button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub title: String,
    pub detail: String,
    pub yes: String,
}

/// Decides about the file at `path`, which must be absolute already.
pub fn decide_file(path: &str, origin: Origin, app_for: &dyn Fn(&Path) -> Option<OpeningApp>) -> FileVerdict {
    let name = crate::highlights::last_component(path).to_owned();
    if origin == Origin::OtherMac {
        return FileVerdict::Refuse(format!("{} is on another Mac, so Threadbear doesn't open it here. It shows in the pane instead.", if name.is_empty() { "That file" } else { &name }));
    }
    let given = Path::new(path);
    if !given.is_absolute() {
        return FileVerdict::Refuse(format!("Threadbear couldn't tell where {name} is."));
    }
    let Some(look) = look(given, app_for) else {
        return FileVerdict::Refuse(format!("{} isn't there any more.", if name.is_empty() { "That file" } else { &name }));
    };
    let url = file_url(&look.resolved.to_string_lossy());
    match &look.runs {
        None => FileVerdict::Open { url, resolved: look.resolved },
        Some(why) => {
            let real = look.resolved.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let link = if look.linked { format!("\n\n{name} is a link to {real}.") } else { String::new() };
            let question = Question {
                title: format!("Open {real}?"),
                detail: format!("{why} It's from a thread, so Threadbear asks before running it.{link}\n\n{}", look.resolved.to_string_lossy()),
                yes: "Open".to_owned(),
            };
            FileVerdict::Ask { question, url, look }
        }
    }
}

/// After a yes: the file at `path` still resolves to what was asked about,
/// and is still the same kind, so a swap while the question was up doesn't
/// open something else.
pub fn still_same(path: &str, asked: &Look, app_for: &dyn Fn(&Path) -> Option<OpeningApp>) -> bool {
    look(Path::new(path), app_for).is_some_and(|now| now.resolved == asked.resolved && now.runs == asked.runs)
}

/// A link shown in a question: long ones cut in the middle (`LinkPolicy.shown`).
pub fn shown(url: &str, limit: usize) -> String {
    let count = url.chars().count();
    if count <= limit {
        return url.to_owned();
    }
    let head: String = url.chars().take(limit * 2 / 3).collect();
    let tail: String = url.chars().skip(count - limit / 3).collect();
    format!("{head}…{tail}")
}

/// The question for a link that starts another app (`OpenGate.openOther`).
pub fn link_question(url: &str, app: &str) -> Question {
    Question {
        title: format!("Open this link in {app}?"),
        detail: format!("It isn't a web page: it starts {app}, which acts on what the link says. It's from a thread, so Threadbear asks first.\n\n{}", shown(url, 300)),
        yes: format!("Open in {app}"),
    }
}

/// Threadbear's own scheme: only ever a thread, never handed back to the system.
pub fn is_own_scheme(url: &str) -> bool {
    scheme(url).is_some_and(|s| s == "threadbear")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn none(_: &Path) -> Option<OpeningApp> {
        None
    }

    /// A folder of its own under the system's temp folder, removed at the end.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("tb-open-policy-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(fs::canonicalize(&dir).unwrap())
        }
        fn path(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }
        fn file(&self, rel: &str, body: &str) -> PathBuf {
            let p = self.path(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, body).unwrap();
            p
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn app_bundle(s: &Scratch, rel: &str) -> PathBuf {
        let app = s.path(rel);
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/Info.plist"), "<plist/>").unwrap();
        app
    }

    /// Finding 58: a skill path with a `#` in it named the sibling `.app`
    /// once pasted into `file://`. The URL is built from the path, and the
    /// path is what's looked at, so the `.app` never comes into it.
    #[test]
    fn a_hash_in_a_path_stays_part_of_the_name() {
        let s = Scratch::new("hash");
        app_bundle(&s, ".agents/skills/Inspect.app");
        let skill = s.file(".agents/skills/Inspect.app#help/SKILL.md", "# skill");
        let url = file_url(&skill.to_string_lossy());
        assert!(url.ends_with("/Inspect.app%23help/SKILL.md"), "{url}");
        assert!(!url.contains('#'));
        match decide_file(&skill.to_string_lossy(), Origin::ThisMac, &none) {
            FileVerdict::Open { url: opened, resolved } => {
                assert_eq!(resolved, skill);
                assert_eq!(opened, url);
            }
            other => panic!("{other:?}"),
        }
        // A link to it written as a URL keeps the %23 in the name, and a
        // bare # cuts the fragment off, as a URL parser would, before the
        // path is decoded: the .app is a folder, so it's refused as a program.
        let encoded = format!("{}/.agents/skills/Inspect.app%23help/SKILL.md", s.0.to_string_lossy());
        assert_eq!(link_kind(&encoded), LinkKind::File(skill.to_string_lossy().into_owned()));
        let bare = ["file://", &s.0.to_string_lossy(), "/.agents/skills/Inspect.app#help/SKILL.md"].concat();
        let LinkKind::File(cut) = link_kind(&bare) else { panic!() };
        assert!(cut.ends_with("/Inspect.app"));
        assert!(matches!(decide_file(&cut, Origin::ThisMac, &none), FileVerdict::Ask { .. }));
    }

    /// Findings 59 and 64: a `.command` (or any program) a reply links to,
    /// inline code or a Markdown link, asks first.
    #[test]
    fn programs_ask_first() {
        let s = Scratch::new("programs");
        for rel in ["docs/report.command", "run.sh", "x.tool", "Setup.pkg", "a.terminal", "go.webloc", "a.jar"] {
            let p = s.file(rel, "#!/bin/sh\necho hi\n");
            assert!(matches!(decide_file(&p.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Ask { .. }), "{rel}");
        }
        let app = app_bundle(&s, "Tool.app");
        assert!(matches!(decide_file(&app.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Ask { .. }));
        // A bundle without the name: still a bundle.
        let plain = app_bundle(&s, "Notes");
        assert!(matches!(decide_file(&plain.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Ask { .. }));
    }

    #[test]
    fn scripts_and_unknown_kinds_ask() {
        let s = Scratch::new("scripts");
        for rel in ["build.py", "a.js", "tool.rb", "x.applescript", "noextension", "thing.weird"] {
            let p = s.file(rel, "print(1)");
            assert!(matches!(decide_file(&p.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Ask { .. }), "{rel}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_executable_without_a_kind_asks() {
        use std::os::unix::fs::PermissionsExt;
        let s = Scratch::new("exec");
        let p = s.file("payload", "#!/bin/sh\n");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        let FileVerdict::Ask { question, .. } = decide_file(&p.to_string_lossy(), Origin::ThisMac, &none) else { panic!() };
        assert!(question.detail.contains("marked as a program"));
    }

    #[test]
    fn documents_and_folders_open() {
        let s = Scratch::new("docs");
        for rel in ["docs/report.md", "shot.png", "a.pdf", "src/main.rs", "data.csv", "page.html"] {
            let p = s.file(rel, "x");
            assert!(matches!(decide_file(&p.to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Open { .. }), "{rel}");
        }
        assert!(matches!(decide_file(&s.path("docs").to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Open { .. }));
    }

    /// A document whose app would run it (a `.md` set to open in Terminal).
    #[test]
    fn a_document_going_to_a_code_runner_asks() {
        let s = Scratch::new("runner");
        let p = s.file("notes.md", "x");
        let terminal = |_: &Path| Some(OpeningApp { path: PathBuf::from("/System/Applications/Utilities/Terminal.app"), bundle_id: Some("com.apple.Terminal".into()), name: "Terminal".into() });
        let FileVerdict::Ask { question, .. } = decide_file(&p.to_string_lossy(), Origin::ThisMac, &terminal) else { panic!() };
        assert!(question.detail.contains("Terminal, which runs it"));
        // An app the system says opens as itself.
        let me = s.file("Thing", "x");
        let itself = |p: &Path| Some(OpeningApp { path: p.to_owned(), bundle_id: Some("com.evil.thing".into()), name: "Thing".into() });
        assert!(matches!(decide_file(&me.to_string_lossy(), Origin::ThisMac, &itself), FileVerdict::Ask { .. }));
    }

    /// A symlink is judged, and opened, as the file it points at.
    #[cfg(unix)]
    #[test]
    fn symlinks_are_followed() {
        let s = Scratch::new("links");
        let app = app_bundle(&s, "Payload.app");
        std::os::unix::fs::symlink(&app, s.path("README.md")).unwrap();
        let FileVerdict::Ask { question, url, look } = decide_file(&s.path("README.md").to_string_lossy(), Origin::ThisMac, &none) else { panic!() };
        assert!(look.linked);
        assert_eq!(look.resolved, app);
        assert!(url.ends_with("/Payload.app"));
        assert!(question.detail.contains("README.md is a link to Payload.app"));
        // The other way round, a program's name on a document opens the document.
        let doc = s.file("real.md", "x");
        std::os::unix::fs::symlink(&doc, s.path("fake.command")).unwrap();
        assert!(matches!(decide_file(&s.path("fake.command").to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Open { resolved, .. } if resolved == doc));
    }

    /// A swap while the question's up isn't what was said yes to.
    #[cfg(unix)]
    #[test]
    fn a_swapped_file_isnt_opened_after_a_yes() {
        let s = Scratch::new("swap");
        let p = s.file("go.command", "#!/bin/sh\n");
        let FileVerdict::Ask { look, .. } = decide_file(&p.to_string_lossy(), Origin::ThisMac, &none) else { panic!() };
        assert!(still_same(&p.to_string_lossy(), &look, &none));
        fs::remove_file(&p).unwrap();
        let other = app_bundle(&s, "Other.app");
        std::os::unix::fs::symlink(&other, &p).unwrap();
        assert!(!still_same(&p.to_string_lossy(), &look, &none));
    }

    #[test]
    fn another_macs_files_and_missing_ones_are_refused() {
        let s = Scratch::new("refuse");
        let p = s.file("a.md", "x");
        assert!(matches!(decide_file(&p.to_string_lossy(), Origin::OtherMac, &none), FileVerdict::Refuse(_)));
        assert!(matches!(decide_file(&s.path("gone.md").to_string_lossy(), Origin::ThisMac, &none), FileVerdict::Refuse(_)));
        assert!(matches!(decide_file("relative/a.md", Origin::ThisMac, &none), FileVerdict::Refuse(_)));
    }

    #[test]
    fn links_by_kind() {
        assert_eq!(link_kind("https://example.com/a?b#c"), LinkKind::Web("https://example.com/a?b#c".into()));
        assert_eq!(link_kind("HTTPS://example.com"), LinkKind::Web("HTTPS://example.com".into()));
        assert_eq!(link_kind("https://"), LinkKind::Refuse);
        assert_eq!(link_kind("https://a b.com"), LinkKind::Refuse);
        assert_eq!(link_kind("mailto:a@b.c"), LinkKind::Ask("mailto:a@b.c".into()));
        assert_eq!(link_kind("x-apple.systempreferences:com.apple.preference"), LinkKind::Ask("x-apple.systempreferences:com.apple.preference".into()));
        assert_eq!(link_kind("file:///tmp/a%20b.md#x"), LinkKind::File("/tmp/a b.md".into()));
        assert_eq!(link_kind("file://localhost/tmp/a.md"), LinkKind::File("/tmp/a.md".into()));
        assert_eq!(link_kind("file://evil.example/share/a.app"), LinkKind::Refuse);
        assert_eq!(link_kind("docs/report.command"), LinkKind::File("docs/report.command".into()));
        assert_eq!(link_kind("guide.md#links"), LinkKind::File("guide.md".into()));
        assert_eq!(link_kind("a\u{0}b"), LinkKind::Refuse);
        assert_eq!(link_kind("#usage"), LinkKind::Refuse);
        assert!(is_own_scheme("threadbear://thread/x"));
    }

    #[test]
    fn file_urls_escape_what_a_url_would_read() {
        assert_eq!(file_url("/a b/c#d?e%f.md"), "file:///a%20b/c%23d%3Fe%25f.md");
        if cfg!(windows) {
            assert_eq!(file_url("C:\\Users\\a.md"), "file:///C:/Users/a.md");
        } else {
            assert_eq!(file_url("/w/Payload.app\\"), "file:///w/Payload.app%5C");
        }
    }

    /// A folder named `Payload.app\` beside the app is judged and opened as
    /// itself, never as the app.
    #[cfg(unix)]
    #[test]
    fn a_backslash_in_a_name_stays_part_of_it() {
        let s = Scratch::new("backslash");
        app_bundle(&s, "Payload.app");
        let plain = s.path("Payload.app\\");
        fs::create_dir_all(&plain).unwrap();
        let LinkKind::File(path) = link_kind(&[&s.0.to_string_lossy(), "/Payload.app%5C"].concat()) else { panic!() };
        match decide_file(&path, Origin::ThisMac, &none) {
            FileVerdict::Open { url, resolved } => {
                assert_eq!(resolved, plain);
                assert!(url.ends_with("/Payload.app%5C"), "{url}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn absolute_paths() {
        assert_eq!(absolute("docs/a.md", "/w/repo/", "/h"), Some("/w/repo/docs/a.md".into()));
        assert_eq!(absolute("./a.md", "/w", "/h"), Some("/w/a.md".into()));
        assert_eq!(absolute("~/a.md", "/w", "/h/"), Some("/h/a.md".into()));
        assert_eq!(absolute("/x", "", "/h"), Some("/x".into()));
        assert_eq!(absolute("a.md", "", "/h"), None);
    }
}
