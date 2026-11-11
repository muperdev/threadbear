// The one way the GPUI app hands a link or a file to the system: a port of the
// Swift app's OpenGate (SECURITY.md, "Your Mac and phone from links and
// files"). The rules are `threadbear_core::open_policy`; this does what they
// say, asks with a real alert, and says why when it won't.
//
// Nothing else in the app calls `open_url` or `reveal_path`:
// desktop/core/tests/open_gate_source.rs fails on one anywhere else, as a
// second opener is how findings 58, 59 and 64 (Daybreak, 9 Nov 2026) got in.
use crate::app::Threadbear;
use gpui_kit::*;
use std::path::Path;
use threadbear_core::open_policy::{self as policy, FileVerdict, LinkKind, OpeningApp, Question};

pub(crate) use threadbear_core::open_policy::Origin;

/// Where a refusal is said: the app's error bar, as the Swift app's
/// `AppModel.errorMessage`.
pub(crate) struct GateNotices(pub WeakEntity<Threadbear>);
impl Global for GateNotices {}

pub(crate) fn install(app: WeakEntity<Threadbear>, cx: &mut App) {
    cx.set_global(GateNotices(app));
}

fn refuse(message: String, cx: &mut App) {
    let Some(app) = cx.try_global::<GateNotices>().and_then(|n| n.0.upgrade()) else { return };
    app.update(cx, |app, cx| app.show_error(message, cx));
}

/// The system's own opener. Only this file reaches it.
fn system_open(url: &str, cx: &mut App) {
    cx.open_url(url);
}

// ---------------------------------------------------------------- links

/// A web address in the browser. Anything else is refused, said so: for a
/// link the backend or a runner sent that must only ever be a web page (a
/// shared page, the thread on the website).
pub(crate) fn open_web(url: &str, cx: &mut App) -> bool {
    match policy::link_kind(url) {
        LinkKind::Web(web) => {
            system_open(&web, cx);
            true
        }
        _ => {
            refuse(format!("That isn't a web address, so Threadbear didn't open it: {}", policy::shown(url, 300)), cx);
            false
        }
    }
}

/// Any link from a thread, a page, a patch or a pin: a web page in the
/// browser, a file through `open_file` (a relative one from `cwd`), anything
/// else only once Mike says yes to it.
pub(crate) fn open_link(url: &str, origin: Origin, cwd: &str, cx: &mut App) {
    match policy::link_kind(url) {
        LinkKind::Web(web) => system_open(&web, cx),
        LinkKind::File(path) => match policy::absolute(&path, cwd, &crate::files::home_dir()) {
            Some(path) => open_file(&path, origin, cx),
            None => refuse(format!("Threadbear couldn't tell where {} is.", threadbear_core::highlights::last_component(&path)), cx),
        },
        LinkKind::Ask(other) => open_other(&other, cx),
        LinkKind::Refuse => refuse(format!("Threadbear couldn't open that link: {}", policy::shown(url, 300)), cx),
    }
}

/// A link that starts another app: only Threadbear's own isn't handed back,
/// and the rest ask, naming the app.
fn open_other(url: &str, cx: &mut App) {
    if policy::is_own_scheme(url) {
        refuse("Threadbear's own links only open threads from inside a thread.".to_owned(), cx);
        return;
    }
    let scheme = url.split(':').next().unwrap_or("");
    let Some(app) = app_for_link(url) else {
        refuse(format!("Nothing on this computer opens {scheme}: links."), cx);
        return;
    };
    let url = url.to_owned();
    ask(policy::link_question(&url, &app), cx, move |cx| system_open(&url, cx));
}

// ---------------------------------------------------------------- rich text

/// Every rich text view in the app is made here, with its links going
/// through the gate. gpui's own TextView hands a clicked link to the
/// system's opener when it has no handler of its own (Codex review of the
/// 10 Nov fix: a `file://` link to a `.command` in a previewed SKILL.md ran
/// it), so a view made anywhere else would be a way round the gate. A caller
/// with somewhere better for its links (the transcript's `link_clicked`)
/// sets its own `on_link_click` after, which replaces this one.
pub(crate) fn text_view(state: &Entity<gpui_kit::base::TextViewState>, origin: Origin, base: &str) -> gpui_kit::base::TextView {
    let base = base.to_owned();
    gpui_kit::base::TextView::new(state).on_link_click(move |url: &SharedString, _: &ClickEvent, _: &mut Window, cx: &mut App| open_link(url, origin, &base, cx))
}

/// The same for the kit's markdown text made from a string.
pub(crate) fn markdown_view(id: impl Into<ElementId>, text: impl Into<SharedString>, origin: Origin, base: &str) -> gpui_kit::component::text::TextView {
    let base = base.to_owned();
    gpui_kit::component::text::TextView::markdown(id, text).on_link_click(move |url: &SharedString, _: &ClickEvent, _: &mut Window, cx: &mut App| open_link(url, origin, &base, cx))
}

// ---------------------------------------------------------------- files

/// A file in its own app, after its symlinks are followed: a document opens,
/// a program asks first, another Mac's file never opens here. `path` is
/// absolute.
pub(crate) fn open_file(path: &str, origin: Origin, cx: &mut App) {
    match policy::decide_file(path, origin, &app_for) {
        FileVerdict::Open { url, .. } => system_open(&url, cx),
        FileVerdict::Refuse(why) => refuse(why, cx),
        FileVerdict::Ask { question, url, look } => {
            let path = path.to_owned();
            ask(question, cx, move |cx| {
                // What was said yes to, still: not a file swapped in while
                // the question was up.
                if policy::still_same(&path, &look, &app_for) {
                    system_open(&url, cx);
                } else {
                    refuse(format!("{} changed while Threadbear was asking, so it didn't open it.", threadbear_core::highlights::last_component(&path)), cx);
                }
            });
        }
    }
}

/// A file shown in its folder (Finder, or the system's file browser), which
/// shows it without opening it. Another Mac's file isn't on this one.
pub(crate) fn reveal(path: &str, origin: Origin, cx: &mut App) {
    if origin == Origin::OtherMac {
        refuse(format!("{} is on another Mac.", threadbear_core::highlights::last_component(path)), cx);
        return;
    }
    cx.reveal_path(Path::new(path));
}

// ---------------------------------------------------------------- asking

/// The question as an alert over the app's window, Cancel first so Return is
/// no; `yes` runs only on the yes button. Asked once the click that led here
/// has finished, as the window is busy with it until then. With no window
/// to ask over, the answer is no.
fn ask(question: Question, cx: &mut App, yes: impl FnOnce(&mut App) + 'static) {
    cx.defer(move |cx| {
        let Some(handle) = cx.active_window().or_else(|| cx.windows().into_iter().next()) else { return };
        let answers = [PromptButton::cancel("Cancel"), PromptButton::new(question.yes.clone())];
        let Ok(answer) = handle.update(cx, |_, window, cx| window.prompt(PromptLevel::Warning, &question.title, Some(&question.detail), &answers, cx)) else { return };
        cx.spawn(async move |cx| {
            if let Ok(1) = answer.await {
                cx.update(yes);
            }
        })
        .detach();
    });
}

// ---------------------------------------------------------------- the system's apps

/// The app the system would open `file` in (`NSWorkspace.urlForApplication`).
#[cfg(target_os = "macos")]
fn app_for(file: &Path) -> Option<OpeningApp> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy()));
    let app = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&url)?;
    describe_app(&app)
}

/// Elsewhere nothing says, and the extension rules stand alone.
#[cfg(not(target_os = "macos"))]
fn app_for(_file: &Path) -> Option<OpeningApp> {
    None
}

/// The app a link would start, by name, or None if nothing opens it.
#[cfg(target_os = "macos")]
fn app_for_link(link: &str) -> Option<String> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    let url = NSURL::URLWithString(&NSString::from_str(link))?;
    let app = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&url)?;
    describe_app(&app).map(|a| a.name)
}

/// Elsewhere the system can't be asked, so the link's scheme stands for its app.
#[cfg(not(target_os = "macos"))]
fn app_for_link(link: &str) -> Option<String> {
    Some(format!("the app for {}: links", link.split(':').next().unwrap_or("")))
}

#[cfg(target_os = "macos")]
fn describe_app(app: &objc2_foundation::NSURL) -> Option<OpeningApp> {
    use objc2_foundation::NSBundle;
    let path = std::path::PathBuf::from(app.path()?.to_string());
    let bundle_id = NSBundle::bundleWithURL(app).and_then(|b| b.bundleIdentifier()).map(|s| s.to_string());
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "its app".to_owned());
    Some(OpeningApp { path, bundle_id, name })
}
