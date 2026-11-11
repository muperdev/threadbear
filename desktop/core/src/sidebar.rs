// What a sidebar row says, with no UI in it: ports of the Swift app's
// ThreadRowLook, RowMode, ThreadByline, ProjectMark, RelativeTime, Snooze,
// WakeWords, WokeWords, Pools.waitingLine, BackgroundWork, BusyThreadAction,
// SubagentRows, ArchiveRows and PreviewText. The GPUI app draws from these;
// `dump` says what they say, so the specs compare both apps' answers.
//
// Times that depend on the calendar take a `DateTime` in the time zone to use
// (the Mac's own in the app, a fixed one in the tests), as the Swift functions
// take a `Calendar`.
use crate::inbox::{parse_ms, BackgroundTask, ThreadSummary};
use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike};
use serde::Deserialize;

/// The wall clock right now, in the machine's own time zone.
pub fn local_now() -> DateTime<chrono::Local> {
    chrono::Local::now()
}

/// What a sidebar row shows at the right of its second line (`ThreadRowStatus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowStatus {
    Time,
    Working,
    Background,
    NeedsYou,
    Failed,
    Stopped,
}

/// How a thread's row looks (`ThreadRowLook`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowLook {
    pub status: RowStatus,
    /// The dot in the gutter: waiting on Mike.
    pub unread: bool,
    /// Semibold title.
    pub emphasised: bool,
    /// The status in words; a working row under Running shows just its timer.
    pub labelled: bool,
    /// Unread, and its last turn said nothing waits on Mike: a tick, not a dot.
    pub done: bool,
    /// Back from hibernating by itself, not opened yet: "Woke up".
    pub woke: bool,
}

impl RowLook {
    pub const PLAIN: RowLook = RowLook { status: RowStatus::Time, unread: false, emphasised: false, labelled: true, done: false, woke: false };

    /// `section` is the key of the heading it's under ("running" and so on).
    pub fn of(t: &ThreadSummary, section: Option<&str>) -> RowLook {
        let status = if t.review_waiting() {
            RowStatus::NeedsYou
        } else {
            match t.status.as_str() {
                "waiting" => RowStatus::NeedsYou,
                "running" | "starting" => RowStatus::Working,
                "error" => RowStatus::Failed,
                "stopped" => RowStatus::Stopped,
                _ if t.background.as_ref().is_none_or(|b| b.is_empty()) || t.waiting_on_mike() => RowStatus::Time,
                _ => RowStatus::Background,
            }
        };
        let done = status == RowStatus::Time && !t.needs_you && t.said_done() && t.unread();
        let unread = status == RowStatus::NeedsYou || (t.needs_you && status != RowStatus::Working && status != RowStatus::Background) || done;
        let emphasised = t.needs_you || status == RowStatus::NeedsYou || done;
        let labelled = !(status == RowStatus::Working && section == Some("running"));
        let woke = status == RowStatus::Time && t.woke.is_some();
        RowLook { status, unread, emphasised, labelled, done, woke }
    }
}

/// How long a running turn has been going: "53s", "4m", "1h 2m"
/// (`ThreadRowLook.workingLabel`).
pub fn working_label(since_ms: i64, now_ms: i64) -> String {
    let seconds = ((now_ms - since_ms) / 1000).max(0);
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if minutes % 60 == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {}m", minutes % 60)
    }
}

/// What kind of list a row is in: how it reads and its hover buttons (`RowMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowMode {
    Inbox,
    Hibernating,
    Archive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowAction {
    Star,
    Snooze,
    Archive,
    Unsnooze,
    Unarchive,
    Delete,
}

impl RowMode {
    /// A starred thread reads as what it is, as `RowMode.init` has it: one
    /// hibernating says when it wakes.
    pub fn of(section: &str, t: &ThreadSummary, now_ms: i64) -> RowMode {
        match section {
            "starred" | "hibernating" if t.snoozed(now_ms) => RowMode::Hibernating,
            "starred" | "hibernating" => RowMode::Inbox,
            "archived" => RowMode::Archive,
            _ => RowMode::Inbox,
        }
    }

    pub fn actions(self) -> &'static [RowAction] {
        match self {
            RowMode::Inbox => &[RowAction::Star, RowAction::Snooze, RowAction::Archive],
            RowMode::Hibernating => &[RowAction::Star, RowAction::Unsnooze, RowAction::Archive],
            RowMode::Archive => &[RowAction::Unarchive, RowAction::Delete],
        }
    }
}

impl RowAction {
    /// The hover button's tooltip, as `RowActions` in SidebarView has them.
    pub fn tip(self, t: &ThreadSummary) -> &'static str {
        match self {
            RowAction::Star if t.is_starred() => "Remove star",
            RowAction::Star => "Star: keep at the top",
            RowAction::Snooze => "Hibernate",
            RowAction::Archive => "Archive",
            RowAction::Unsnooze => "Wake",
            RowAction::Unarchive => "Unarchive",
            RowAction::Delete => "Delete chat…",
        }
    }

    /// Its SF Symbol, which `gpui/src/icons.rs` maps to the bundled icon.
    pub fn symbol(self, t: &ThreadSummary) -> &'static str {
        match self {
            RowAction::Star if t.is_starred() => "star.fill",
            RowAction::Star => "star",
            RowAction::Snooze => "moon.zzz",
            RowAction::Archive => "archivebox",
            RowAction::Unsnooze | RowAction::Unarchive => "arrow.uturn.backward",
            RowAction::Delete => "trash",
        }
    }
}

/// What a tile at the start of a row shows (`ProjectTiles`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tile {
    /// The project's first letter or digit.
    Letter(String),
    /// In no project.
    Folder,
}

/// The tile, and whether there's a second project's edge behind it.
pub fn tile(t: &ThreadSummary) -> (Tile, bool) {
    match t.projects.as_deref() {
        Some(projects) if !projects.is_empty() => (Tile::Letter(project_letter(&projects[0].title)), projects.len() > 1),
        _ => (Tile::Folder, false),
    }
}

/// "C" for "Convex docs", "7" for "7 day plan", "?" for a title with neither
/// (`ProjectMark.letter`).
pub fn project_letter(title: &str) -> String {
    title.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())
}

/// The tile's tooltip: every project the thread is in, or its whole folder
/// (`ProjectMark.help`).
pub fn tile_help(t: &ThreadSummary) -> String {
    match t.projects.as_deref() {
        Some(projects) if !projects.is_empty() => projects.iter().map(|p| if p.title.is_empty() { "Untitled project" } else { p.title.as_str() }).collect::<Vec<_>>().join(", "),
        _ => t.cwd.clone(),
    }
}

/// The place line's tooltip: what Mike last said to it, else the place
/// (`ProjectMark.lineTip`).
pub fn place_tip(t: &ThreadSummary) -> String {
    match t.asked.as_deref().filter(|a| !a.is_empty()) {
        Some(asked) => format!("You: {asked}"),
        None => t.place(),
    }
}

/// `ThreadSummary.startedBy` as `ThreadByline` reads it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct StartedBy {
    kind: String,
    label: String,
    icon: Option<String>,
    thread_title: Option<String>,
    why: Option<String>,
}

/// The line under the title of a thread Mike didn't start: a symbol for who
/// started it, then its place, who and why in the tooltip (`ThreadByline`).
pub struct Byline {
    pub symbol: String,
    pub tip: String,
}

pub fn byline(t: &ThreadSummary) -> Option<Byline> {
    let started: StartedBy = match (&t.started_by, &t.schedule) {
        (Some(v), _) => serde_json::from_value(v.clone()).ok()?,
        (None, Some(_)) => StartedBy { kind: "schedule".into(), label: "Schedule".into(), ..Default::default() },
        (None, None) => return None,
    };
    let symbol = match started.kind.as_str() {
        "schedule" => "clock".to_string(),
        "thread" => "pawprint".to_string(),
        "librarian" => "books.vertical".to_string(),
        "runner" => "bell".to_string(),
        _ => started.icon.clone().filter(|i| !i.is_empty()).unwrap_or_else(|| "wrench.and.screwdriver".into()),
    };
    let from = started.thread_title.clone().filter(|s| !s.is_empty());
    let who = match started.kind.as_str() {
        "thread" => from.map_or("Started by another thread".to_string(), |f| format!("Started by another thread: {f}")),
        "librarian" => from.map_or("Started by the librarian".to_string(), |f| format!("Started by the librarian, from: {f}")),
        _ => format!("Started by {}", started.label),
    };
    let tip = match started.why.filter(|w| !w.is_empty()) {
        Some(why) => format!("{who} ({why})"),
        None => who,
    };
    Some(Byline { symbol, tip })
}

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

fn local<Tz: TimeZone>(ms: i64, tz: &Tz) -> DateTime<Tz> {
    tz.timestamp_millis_opt(ms).earliest().unwrap_or_else(|| tz.timestamp_millis_opt(0).unwrap())
}

fn weekday_name<Tz: TimeZone>(d: &DateTime<Tz>) -> &'static str {
    WEEKDAYS[d.weekday().num_days_from_monday() as usize]
}

fn same_day<Tz: TimeZone>(a: &DateTime<Tz>, b: &DateTime<Tz>) -> bool {
    a.date_naive() == b.date_naive()
}

/// "5:00 am", with its minutes always (`Pools.waitingParts`' clock).
fn clock<Tz: TimeZone>(d: &DateTime<Tz>) -> String {
    let hour = d.hour();
    let twelve = if hour % 12 == 0 { 12 } else { hour % 12 };
    format!("{twelve}:{:02} {}", d.minute(), if hour < 12 { "am" } else { "pm" })
}

/// The time as the Mac writes a short one: "3:05 pm". Whole hours are "3 pm"
/// in `Date.FormatStyle` (.hour().minute() shows ":00" too on Mike's locale).
fn time_short<Tz: TimeZone>(d: &DateTime<Tz>) -> String {
    clock(d)
}

/// Short relative times for the sidebar: "now", "5m", "3h", "Mon", "12 Oct"
/// (`RelativeTime.short`).
pub fn relative_short<Tz: TimeZone>(at_ms: i64, now: &DateTime<Tz>) -> String {
    let seconds = (now.timestamp_millis() - at_ms) / 1000;
    if seconds < 60 {
        return "now".into();
    }
    if seconds < 3600 {
        return format!("{}m", seconds / 60);
    }
    if seconds < 86_400 {
        return format!("{}h", seconds / 3600);
    }
    let date = local(at_ms, &now.timezone());
    if seconds < 6 * 86_400 {
        return weekday_name(&date).to_string();
    }
    let day_month = format!("{} {}", date.day(), MONTHS[date.month0() as usize]);
    if date.year() == now.year() {
        day_month
    } else {
        format!("{day_month} {}", date.year())
    }
}

/// "3:05 pm" today, "Mon 3:05 pm" otherwise, as a hibernating row says when
/// it wakes (`RelativeTime.until`).
pub fn until_words<Tz: TimeZone>(at_ms: i64, now: &DateTime<Tz>) -> String {
    let date = local(at_ms, &now.timezone());
    if same_day(&date, now) {
        time_short(&date)
    } else {
        format!("{} {}", weekday_name(&date), time_short(&date))
    }
}

/// "Waiting for room · 5:00 am", "… · tomorrow 5:00 am" (`Pools.waitingLine`).
pub fn waiting_line<Tz: TimeZone>(until_ms: i64, now: &DateTime<Tz>) -> String {
    format!("Waiting for room · {}", waiting_time(until_ms, now))
}

fn waiting_time<Tz: TimeZone>(until_ms: i64, now: &DateTime<Tz>) -> String {
    if until_ms <= now.timestamp_millis() {
        return "any moment now".into();
    }
    match waiting_parts(until_ms, now) {
        (Some(day), clock_text) => format!("{day} {clock_text}"),
        (None, clock_text) => clock_text,
    }
}

/// The open thread's line while it waits: "Every login in Evenings is out of
/// room. It carries on by itself at 5:25 pm." (`Pools.waitingSentence`).
pub fn waiting_sentence<Tz: TimeZone>(pool_title: &str, until_ms: i64, now: &DateTime<Tz>) -> String {
    let start = format!("Every login in {pool_title} is out of room. ");
    if until_ms <= now.timestamp_millis() {
        return start + "It carries on any moment now.";
    }
    match waiting_parts(until_ms, now) {
        (Some(day), clock_text) => format!("{start}It carries on by itself {day} at {clock_text}."),
        (None, clock_text) => format!("{start}It carries on by itself at {clock_text}."),
    }
}

/// "5:00 am", and the day when it isn't today ("tomorrow", "Thu")
/// (`Pools.waitingParts`).
fn waiting_parts<Tz: TimeZone>(until_ms: i64, now: &DateTime<Tz>) -> (Option<&'static str>, String) {
    let date = local(until_ms, &now.timezone());
    let clock_text = clock(&date);
    if same_day(&date, now) {
        return (None, clock_text);
    }
    let tomorrow = now.clone() + Duration::days(1);
    let yesterday = now.clone() - Duration::days(1);
    let day = if same_day(&date, &tomorrow) {
        "tomorrow"
    } else if same_day(&date, &yesterday) {
        "yesterday"
    } else {
        weekday_name(&date)
    };
    (Some(day), clock_text)
}

/// Short, for the sidebar: "3 sub-agents", "1 job", "Finishing" when all
/// that's left is a result about to be read (`BackgroundWork.short`).
pub fn background_short(tasks: &[BackgroundTask]) -> String {
    let running: Vec<&BackgroundTask> = tasks.iter().filter(|t| !t.finished).collect();
    if running.is_empty() {
        return "Finishing".into();
    }
    let first_kind = kind_of(running[0]);
    if running.iter().all(|t| kind_of(t) == first_kind) {
        count(running.len(), first_kind)
    } else {
        count(running.len(), "shell")
    }
}

/// The descriptions, one per line, for the tooltip (`BackgroundWork.detail`).
pub fn background_detail(tasks: &[BackgroundTask]) -> String {
    tasks
        .iter()
        .map(|t| {
            let name = if t.description.is_empty() { count(1, kind_of(t)) } else { t.description.clone() };
            if t.finished { format!("{name} (finished)") } else { name }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn kind_of(t: &BackgroundTask) -> &str {
    match t.kind.as_str() {
        "agent" | "shell" | "monitor" => t.kind.as_str(),
        _ => "other",
    }
}

fn count(n: usize, kind: &str) -> String {
    let word = match kind {
        "agent" => "sub-agent",
        "shell" => "job",
        "monitor" => "monitor",
        _ => "task",
    };
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// How a thread hibernated in Mike's words says when it wakes (`WakeWords`).
pub const WAKE_MENU_TITLE: &str = "Wake when…";
pub const WAKE_PLACEHOLDER: &str = "at the weekend, in a few weeks, next time I talk about woodworking";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct WakeWhen {
    said: String,
    topic: Option<String>,
    pending: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct WaitingOn {
    title: String,
    until: Option<String>,
    done: bool,
}

/// The time a snoozed thread wakes by, if it has a real one: the runner's
/// 9999-12-31 for a topic or restart isn't one (`WakeWords.time`).
fn wake_time(t: &ThreadSummary) -> Option<i64> {
    // 200 years from 1970: any real time is well before it.
    const FAR_OFF: i64 = 200 * 365 * 86_400 * 1000;
    if t.snoozed_until_restart == Some(true) {
        return None;
    }
    t.snoozed_until_ms().filter(|&until| until < FAR_OFF)
}

/// What a Hibernating row shows where its time would be, and the tooltip
/// with the rest (`WakeWords.row`): a topic, or Thinking… while the runner
/// works out when. Nil for one hibernated the ordinary way.
pub fn wake_row<Tz: TimeZone>(t: &ThreadSummary, now: &DateTime<Tz>) -> Option<(String, String)> {
    let waiting: Vec<WaitingOn> = t.waiting_on.iter().flatten().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect();
    let left: Vec<&WaitingOn> = waiting.iter().filter(|w| !w.done).collect();
    if let Some(first) = left.first() {
        let names = if left.len() == 1 { format!("\"{}\"", first.title) } else { format!("\"{}\" and {} more", first.title, left.len() - 1) };
        let what = if left.iter().all(|w| w.until.is_none()) {
            let joined = left.iter().map(|w| format!("\"{}\"", w.title)).collect::<Vec<_>>().join(if left.len() == 2 { " and " } else { ", " });
            format!("{joined}{}", if left.len() == 1 { " finishes" } else { " finish" })
        } else {
            left.iter().map(|w| format!("\"{}\" {}", w.title, w.until.as_deref().unwrap_or("finishes"))).collect::<Vec<_>>().join("; ")
        };
        let mut tip = format!("Back when {what}");
        if let Some(time) = wake_time(t) {
            tip += &format!(", or {} at the latest", until_words(time, now));
        }
        return Some((format!("Waiting on {names}"), format!("{tip}.")));
    }
    let wake: WakeWhen = serde_json::from_value(t.wake_when.clone()?).ok()?;
    if wake.pending {
        return Some(("Thinking…".into(), format!("Working out when from \"{}\"", wake.said)));
    }
    let topic = wake.topic?;
    let mut tip = format!("Back when you next talk about {topic}");
    if let Some(time) = wake_time(t) {
        tip += &format!(", or {} at the latest", until_words(time, now));
    }
    Some((topic, format!("{tip}. You said \"{}\".", wake.said)))
}

/// The tooltip for "Woke up": "Woke up after hibernating for 2 days"
/// (`WokeWords.tip`).
pub fn woke_tip(t: &ThreadSummary) -> String {
    let woke = t.woke.as_ref();
    let at = woke.and_then(|w| w.get("at")).and_then(|v| v.as_str()).and_then(parse_ms);
    let since = woke.and_then(|w| w.get("since")).and_then(|v| v.as_str()).and_then(parse_ms);
    match (at, since) {
        (Some(at), Some(since)) if at - since >= 60_000 => format!("Woke up after hibernating for {}", away((at - since) / 1000)),
        _ => "Woke up from hibernating".into(),
    }
}

/// "5 minutes", "an hour", "3 hours", "a day", "12 days" (`WokeWords.away`).
pub fn away(seconds: i64) -> String {
    let minutes = ((seconds as f64 / 60.).round() as i64).max(1);
    if minutes < 60 {
        return if minutes == 1 { "a minute".into() } else { format!("{minutes} minutes") };
    }
    let hours = (seconds as f64 / 3600.).round() as i64;
    if hours < 24 {
        return if hours == 1 { "an hour".into() } else { format!("{hours} hours") };
    }
    let days = (seconds as f64 / 86_400.).round() as i64;
    if days == 1 { "a day".into() } else { format!("{days} days") }
}

/// How the words at the right of a row are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrailingKind {
    /// Faint and plain: the time, a hibernating row's wake time, "Waiting for room".
    Quiet,
    /// The accent spinner and timer.
    Working,
    /// The grey spinner and what's running.
    Background,
    /// Red.
    Failed,
    /// Secondary.
    Stopped,
    /// Accent, medium: "Woke up".
    Woke,
}

/// What a row says at its right end (`ThreadRowView.trailing`). `kind` picks
/// the style; `tip` is the tooltip when it has one.
pub struct Trailing {
    pub text: String,
    pub kind: TrailingKind,
    pub tip: Option<String>,
}

/// `ThreadRowView.trailing(_:)`. `section` is the heading's key.
pub fn trailing<Tz: TimeZone>(t: &ThreadSummary, look: &RowLook, mode: RowMode, now: &DateTime<Tz>) -> Trailing {
    let now_ms = now.timestamp_millis();
    if let Some(moving) = &t.moving {
        let state = moving.get("state").and_then(|v| v.as_str()).unwrap_or("");
        let name = moving.get("toName").and_then(|v| v.as_str()).unwrap_or("");
        return match state {
            "failed" => Trailing { text: "Couldn't move".into(), kind: TrailingKind::Failed, tip: moving.get("note").and_then(|v| v.as_str()).map(String::from) },
            "cancelled" => Trailing { text: "Move cancelled".into(), kind: TrailingKind::Stopped, tip: None },
            _ => Trailing { text: format!("Moving to {name}"), kind: TrailingKind::Background, tip: None },
        };
    }
    if mode != RowMode::Hibernating && !t.is_archived() {
        if let Some(until) = t.waiting_for_room_until_ms() {
            return Trailing { text: waiting_line(until, now), kind: TrailingKind::Quiet, tip: None };
        }
        if t.waiting_for_network.is_some() {
            return Trailing { text: "Waiting for connection".into(), kind: TrailingKind::Quiet, tip: Some("The internet dropped during its turn. It carries on by itself once the connection is back.".into()) };
        }
    }
    let time = || relative_short(if mode == RowMode::Archive { t.archived_ms().unwrap_or(t.activity_ms()) } else { t.activity_ms() }, now);
    match look.status {
        RowStatus::Time => {
            if mode == RowMode::Hibernating {
                if let Some((text, tip)) = wake_row(t, now) {
                    return Trailing { text, kind: TrailingKind::Quiet, tip: Some(tip) };
                }
                let text = if t.snoozed_until_release == Some(true) {
                    "Back on update".to_string()
                } else if t.snoozed_until_restart == Some(true) {
                    "Back on restart".to_string()
                } else if let Some(until) = t.snoozed_until_ms() {
                    format!("Back {}", until_words(until, now))
                } else {
                    String::new()
                };
                return Trailing { text, kind: TrailingKind::Quiet, tip: None };
            }
            if look.woke {
                return Trailing { text: "Woke up".into(), kind: TrailingKind::Woke, tip: Some(woke_tip(t)) };
            }
            Trailing { text: time(), kind: TrailingKind::Quiet, tip: None }
        }
        RowStatus::Working => {
            let elapsed = working_label(t.activity_ms(), now_ms);
            let text = if look.labelled { format!("Working {elapsed}") } else { elapsed };
            Trailing { text, kind: TrailingKind::Working, tip: Some("Working".into()) }
        }
        RowStatus::Background => {
            let tasks = t.background_tasks();
            Trailing { text: background_short(&tasks), kind: TrailingKind::Background, tip: Some(background_detail(&tasks)) }
        }
        RowStatus::NeedsYou => Trailing { text: time(), kind: TrailingKind::Quiet, tip: None },
        RowStatus::Failed => Trailing { text: "Failed".into(), kind: TrailingKind::Failed, tip: Some("The last turn failed".into()) },
        RowStatus::Stopped => Trailing { text: "Stopped".into(), kind: TrailingKind::Stopped, tip: Some("Stopped when the runner restarted".into()) },
    }
}

/// Whether a hover button should ask first: a turn underway, or background
/// work the runner would stop (`BusyThreadAction.needsConfirm`).
pub fn needs_busy_confirm(t: &ThreadSummary) -> bool {
    t.busy() || t.background.as_ref().is_some_and(|b| !b.is_empty())
}

/// The question and detail for hibernating or archiving a busy thread
/// (`BusyThreadAction.prompt`). `archive` false means hibernate.
pub fn busy_prompt(t: &ThreadSummary, archive: bool) -> (String, String) {
    let verb = if archive { "Archive" } else { "Hibernate" };
    let doing = if archive { "Archiving" } else { "Hibernating" };
    if !t.busy() {
        let what = background_short(&t.background_tasks()).to_lowercase();
        return (format!("{verb} and stop the background work?"), format!("This thread still has {what} in the background. {doing} it now stops them."));
    }
    let state = if t.status == "waiting" { "is waiting for you" } else { "is still working" };
    (format!("{verb} and stop the turn?"), format!("This thread {state}. {doing} it now stops its turn."))
}

/// One preset in the Hibernate menu (`SnoozePreset`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnoozePreset {
    pub id: &'static str,
    pub title: &'static str,
    pub until_ms: i64,
}

impl SnoozePreset {
    /// Its icon in the menu.
    pub fn symbol(&self) -> &'static str {
        match self.id {
            "evening" => "sunset",
            "tomorrow" => "sunrise",
            "monday" => "briefcase",
            _ => "clock",
        }
    }
}

/// `Snooze.untilRestartTitle`, shown only while a runner restart is waiting.
pub const SNOOZE_UNTIL_RESTART: &str = "Until the runner restarts";
pub const SNOOZE_PICK_TITLE: &str = "Pick a time…";

/// The snooze menu: 1 hour, this evening 6 pm, tomorrow 9 am, next Monday
/// 9 am (`Snooze.presets`).
pub fn snooze_presets<Tz: TimeZone>(now: &DateTime<Tz>) -> Vec<SnoozePreset> {
    let tz = now.timezone();
    let now_ms = now.timestamp_millis();
    let at = |day: &DateTime<Tz>, hour: u32| -> Option<i64> {
        tz.with_ymd_and_hms(day.year(), day.month(), day.day(), hour, 0, 0).earliest().map(|d| d.timestamp_millis())
    };
    let mut presets = vec![SnoozePreset { id: "hour", title: "1 hour", until_ms: now_ms + 3_600_000 }];
    if let Some(evening) = at(now, 18).filter(|&e| e - now_ms > 3_600_000) {
        // Hidden within the hour before 6 pm, where it would duplicate "1 hour".
        presets.push(SnoozePreset { id: "evening", title: "This evening, 6 pm", until_ms: evening });
    }
    let tomorrow = now.clone() + Duration::days(1);
    if let Some(nine) = at(&tomorrow, 9) {
        presets.push(SnoozePreset { id: "tomorrow", title: "Tomorrow, 9 am", until_ms: nine });
    }
    if let Some(monday) = next_monday(now).and_then(|m| at(&m, 9)) {
        presets.push(SnoozePreset { id: "monday", title: "Next Monday, 9 am", until_ms: monday });
    }
    presets
}

/// The next Monday strictly after today.
fn next_monday<Tz: TimeZone>(now: &DateTime<Tz>) -> Option<DateTime<Tz>> {
    (1..=7).map(|offset| now.clone() + Duration::days(offset)).find(|d| d.weekday() == chrono::Weekday::Mon)
}

/// A preset's date, worked out when it's chosen (`Snooze.date`): "This
/// evening" chosen too late to make sense falls back to an hour.
pub fn snooze_date<Tz: TimeZone>(preset_id: &str, now: &DateTime<Tz>) -> i64 {
    snooze_presets(now).into_iter().find(|p| p.id == preset_id).map_or(now.timestamp_millis() + 3_600_000, |p| p.until_ms)
}

/// A sub-agent as `ThreadSummary.subagents` has it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Subagent {
    pub id: String,
    pub parent_id: Option<String>,
    pub description: String,
    pub agent_type: Option<String>,
    pub status: String,
    pub background: bool,
    pub activity: Option<String>,
    pub summary: Option<String>,
    pub tool_uses: Option<i64>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

impl Subagent {
    pub fn running(&self) -> bool {
        self.status == "running"
    }
    pub fn started_ms(&self) -> i64 {
        parse_ms(&self.started_at).unwrap_or(0)
    }
    pub fn ended_ms(&self) -> Option<i64> {
        self.ended_at.as_deref().and_then(parse_ms)
    }
}

/// The thread's sub-agents, read leniently.
pub fn subagents(t: &ThreadSummary) -> Vec<Subagent> {
    t.subagents.iter().flatten().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect()
}

/// A sub-agent's row under its thread: 0 under the thread, 1 under another
/// sub-agent shown above it (`SubagentRow`).
#[derive(Clone, Debug)]
pub struct SubagentRow {
    pub subagent: Subagent,
    pub depth: usize,
}

/// A finished sub-agent stays under its thread this long (`SubagentRows.recent`).
const RECENT_MS: i64 = 10 * 60 * 1000;
/// Most finished ones shown under the selected thread.
const MAX_FINISHED_WHEN_SELECTED: usize = 10;

/// `SubagentRows.visible`: running ones, ones that finished in the last 10
/// minutes, and for the selected thread its finished ones too. A selected
/// sub-agent is always shown; a nested one sits under its parent when the
/// parent is shown, otherwise at the top level.
pub fn subagent_rows(all: &[Subagent], thread_selected: bool, selected_subagent: Option<&str>, now_ms: i64, finished_shown: bool) -> Vec<SubagentRow> {
    use std::collections::{HashMap, HashSet};
    if all.is_empty() {
        return vec![];
    }
    let recently_ended = |s: &Subagent| !s.running() && s.ended_ms().is_some_and(|e| now_ms - e < RECENT_MS);
    let mut shown: HashSet<&str> = all
        .iter()
        .filter(|s| s.running() || Some(s.id.as_str()) == selected_subagent || (finished_shown && recently_ended(s)))
        .map(|s| s.id.as_str())
        .collect();
    if thread_selected && finished_shown {
        let finished_already = all.iter().filter(|s| !s.running() && shown.contains(s.id.as_str())).count();
        let room = MAX_FINISHED_WHEN_SELECTED.saturating_sub(finished_already);
        let rest: Vec<&Subagent> = all.iter().filter(|s| !s.running() && !shown.contains(s.id.as_str())).collect();
        for s in rest.iter().rev().take(room) {
            shown.insert(s.id.as_str());
        }
    }
    let kept: Vec<&Subagent> = all.iter().filter(|s| shown.contains(s.id.as_str())).collect();
    let ids: HashSet<&str> = kept.iter().map(|s| s.id.as_str()).collect();
    let mut children: HashMap<&str, Vec<&Subagent>> = HashMap::new();
    let mut roots: Vec<&Subagent> = vec![];
    for s in &kept {
        match s.parent_id.as_deref().filter(|p| *p != s.id && ids.contains(p)) {
            Some(parent) => children.entry(parent).or_default().push(s),
            None => roots.push(s),
        }
    }
    fn place<'a>(s: &'a Subagent, depth: usize, children: &HashMap<&'a str, Vec<&'a Subagent>>, placed: &mut HashSet<&'a str>, rows: &mut Vec<SubagentRow>) {
        if !placed.insert(s.id.as_str()) {
            return;
        }
        rows.push(SubagentRow { subagent: s.clone(), depth });
        for child in children.get(s.id.as_str()).into_iter().flatten() {
            place(child, (depth + 1).min(2), children, placed, rows);
        }
    }
    let mut rows = vec![];
    let mut placed = HashSet::new();
    for root in roots {
        place(root, 0, &children, &mut placed, &mut rows);
    }
    // A parent loop leaves both out of roots.
    for s in kept {
        place(s, 0, &children, &mut placed, &mut rows);
    }
    rows
}

/// The row's title: its brief, or its type for one with none (`SubagentRows.title`).
pub fn subagent_title(s: &Subagent) -> String {
    let text = s.description.trim();
    if !text.is_empty() {
        return text.to_string();
    }
    s.agent_type.as_ref().map_or("Sub-agent".to_string(), |t| format!("{t} sub-agent"))
}

/// The quieter second line: what it's doing, or what it reported (`SubagentRows.detail`).
pub fn subagent_detail(s: &Subagent) -> String {
    let plain = |text: &Option<String>| text.as_deref().map(preview_plain).unwrap_or_default();
    match s.status.as_str() {
        "running" => {
            let activity = plain(&s.activity);
            if activity.is_empty() { "Starting".into() } else { activity }
        }
        "completed" => {
            let summary = plain(&s.summary);
            if summary.is_empty() { "Done".into() } else { summary }
        }
        "failed" => {
            let summary = plain(&s.summary);
            if summary.is_empty() { "Failed".into() } else { format!("Failed: {summary}") }
        }
        _ => "Stopped".into(),
    }
}

/// How long it ran, or has run so far: "53s", "4m" (`SubagentRows.duration`).
pub fn subagent_duration(s: &Subagent, now_ms: i64) -> String {
    working_label(s.started_ms(), s.ended_ms().unwrap_or(now_ms))
}

/// The status in words, for tooltips (`SubagentRows.statusText`).
pub fn subagent_status_text(s: &Subagent) -> &'static str {
    match s.status.as_str() {
        "running" if s.background => "Running in the background",
        "running" => "Running",
        "completed" => "Done",
        "failed" => "Failed",
        _ => "Stopped",
    }
}

/// The runner's preview is the start of the last assistant text, which is
/// markdown; the sidebar shows it as plain words (`PreviewText.plain`).
pub fn preview_plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // [label](url) and ![label](url) read as the label.
        if c == '[' || (c == '!' && chars.get(i + 1) == Some(&'[')) {
            let start = if c == '!' { i + 2 } else { i + 1 };
            let mut j = start;
            while j < chars.len() && chars[j] != ']' {
                j += 1;
            }
            if j < chars.len() && chars.get(j + 1) == Some(&'(') {
                let mut k = j + 2;
                while k < chars.len() && chars[k] != ')' {
                    k += 1;
                }
                if k < chars.len() {
                    out.extend(&chars[start..j]);
                    i = k + 1;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    // Heading and quote markers wherever a line started: after a start or
    // whitespace, and followed by whitespace.
    let mut no_markers = String::with_capacity(out.len());
    let mut chars = out.chars().peekable();
    let mut at_start = true;
    while let Some(c) = chars.next() {
        if at_start && (c == '#' || c == '>') {
            let mut run = String::from(c);
            if c == '#' {
                while chars.peek() == Some(&'#') && run.len() < 6 {
                    run.push(chars.next().unwrap());
                }
            }
            if chars.peek().is_some_and(|n| n.is_whitespace()) {
                chars.next();
                at_start = true;
                continue;
            }
            no_markers.push_str(&run);
            at_start = false;
            continue;
        }
        at_start = c.is_whitespace();
        no_markers.push(c);
    }
    let stripped = no_markers.replace("**", "").replace("__", "").replace("~~", "").replace('`', "");
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Which archived threads the Archived section draws: the most recent few,
/// every match while searching, and the open thread always (`ArchiveRows`).
pub const ARCHIVE_FIRST: usize = 20;
pub const ARCHIVE_STEP: usize = 50;

/// How many rows to draw and how many are left out.
pub fn archive_rows(count: usize, limit: usize, searching: bool, selected_index: Option<usize>) -> (usize, usize) {
    if searching {
        return (count, 0);
    }
    let selected = selected_index.map_or(0, |i| i + 1);
    let shown = count.min(limit.max(selected));
    (shown, count - shown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};
    use serde_json::json;

    fn thread(over: serde_json::Value) -> ThreadSummary {
        let mut v = json!({ "id": "t", "title": "A thread", "cwd": "/Users/mike/dev/misc", "status": "idle", "lastActivityAt": "2026-11-09T08:00:00Z", "preview": "", "lastSeq": 5, "lastSeenSeq": 5 });
        v.as_object_mut().unwrap().extend(over.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    }

    fn at(s: &str) -> DateTime<FixedOffset> {
        // Melbourne in summer, as the Swift tests' calendar is an explicit one.
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&FixedOffset::east_opt(11 * 3600).unwrap())
    }

    #[test]
    fn row_look_follows_the_rules() {
        // A question: the dot, bold.
        let asking = thread(json!({ "status": "waiting", "needsYou": true, "lastSeq": 6 }));
        let look = RowLook::of(&asking, Some("inbox"));
        assert_eq!(look.status, RowStatus::NeedsYou);
        assert!(look.unread && look.emphasised && !look.done);

        // Finished, said it needs nothing, not read: a tick, bold.
        let done = thread(json!({ "needs": "nothing", "lastSeq": 7 }));
        let look = RowLook::of(&done, Some("inbox"));
        assert_eq!(look.status, RowStatus::Time);
        assert!(look.done && look.unread && look.emphasised);

        // Read and done: nothing.
        let quiet = thread(json!({ "needs": "nothing" }));
        let look = RowLook::of(&quiet, Some("inbox"));
        assert!(!look.unread && !look.emphasised && !look.done);

        // A turn under way: under Running it doesn't say Working.
        let running = thread(json!({ "status": "running" }));
        assert!(!RowLook::of(&running, Some("running")).labelled);
        assert!(RowLook::of(&running, Some("starred")).labelled);

        // Background work: grey spinner, no dot even if it asked for something.
        let waiting = thread(json!({ "needsYou": true, "background": [{ "kind": "agent", "description": "x" }] }));
        assert_eq!(RowLook::of(&waiting, Some("running")).status, RowStatus::Background);
        assert!(!RowLook::of(&waiting, Some("running")).unread);

        // An ask with a background job: the ask wins and it's the row's status.
        let job = thread(json!({ "needs": "you", "needsYou": true, "background": [{ "kind": "shell", "description": "npm test" }] }));
        assert_eq!(RowLook::of(&job, Some("inbox")).status, RowStatus::Time);
        assert!(RowLook::of(&job, Some("inbox")).unread);
    }

    #[test]
    fn working_timer() {
        assert_eq!(working_label(0, 53_000), "53s");
        assert_eq!(working_label(0, 240_000), "4m");
        assert_eq!(working_label(0, 3_720_000), "1h 2m");
        assert_eq!(working_label(0, 7_200_000), "2h");
    }

    #[test]
    fn relative_time_words() {
        let now = at("2026-11-09T20:00:00+11:00");
        let ms = |s: &str| at(s).timestamp_millis();
        assert_eq!(relative_short(ms("2026-11-09T19:59:40+11:00"), &now), "now");
        assert_eq!(relative_short(ms("2026-11-09T19:55:00+11:00"), &now), "5m");
        assert_eq!(relative_short(ms("2026-11-09T17:00:00+11:00"), &now), "3h");
        assert_eq!(relative_short(ms("2026-11-07T20:00:00+11:00"), &now), "Sat");
        assert_eq!(relative_short(ms("2026-10-12T20:00:00+11:00"), &now), "12 Oct");
        assert_eq!(relative_short(ms("2025-10-12T20:00:00+11:00"), &now), "12 Oct 2025");

        assert_eq!(until_words(ms("2026-11-09T22:05:00+11:00"), &now), "10:05 pm");
        assert_eq!(until_words(ms("2026-11-10T09:00:00+11:00"), &now), "Tue 9:00 am");
    }

    #[test]
    fn waiting_for_room_says_when() {
        let now = at("2026-11-09T20:00:00+11:00");
        let ms = |s: &str| at(s).timestamp_millis();
        assert_eq!(waiting_line(ms("2026-11-09T21:25:00+11:00"), &now), "Waiting for room · 9:25 pm");
        assert_eq!(waiting_line(ms("2026-11-10T05:00:00+11:00"), &now), "Waiting for room · tomorrow 5:00 am");
        assert_eq!(waiting_line(ms("2026-11-12T05:00:00+11:00"), &now), "Waiting for room · Thu 5:00 am");
        assert_eq!(waiting_line(ms("2026-11-09T19:00:00+11:00"), &now), "Waiting for room · any moment now");
        assert_eq!(waiting_sentence("Evenings", ms("2026-11-09T21:25:00+11:00"), &now), "Every login in Evenings is out of room. It carries on by itself at 9:25 pm.");
        assert_eq!(waiting_sentence("Evenings", ms("2026-11-10T05:00:00+11:00"), &now), "Every login in Evenings is out of room. It carries on by itself tomorrow at 5:00 am.");
        assert_eq!(waiting_sentence("Evenings", ms("2026-11-09T19:00:00+11:00"), &now), "Every login in Evenings is out of room. It carries on any moment now.");
    }

    #[test]
    fn background_words() {
        let tasks = |list: &[(&str, bool)]| list.iter().map(|(kind, finished)| BackgroundTask { kind: kind.to_string(), description: String::new(), finished: *finished }).collect::<Vec<_>>();
        assert_eq!(background_short(&tasks(&[("agent", false), ("agent", false), ("agent", false)])), "3 sub-agents");
        assert_eq!(background_short(&tasks(&[("shell", false)])), "1 job");
        assert_eq!(background_short(&tasks(&[("agent", false), ("shell", false)])), "2 jobs");
        assert_eq!(background_short(&tasks(&[("agent", true)])), "Finishing");
    }

    #[test]
    fn trailing_words() {
        let now = at("2026-11-09T20:00:00+11:00");
        let room = thread(json!({ "needsYou": true, "waitingForRoom": { "until": "2026-11-09T18:00:00Z" } }));
        let look = RowLook::of(&room, Some("inbox"));
        assert_eq!(trailing(&room, &look, RowMode::Inbox, &now).text, "Waiting for room · tomorrow 5:00 am");

        let failed = thread(json!({ "status": "error" }));
        let look = RowLook::of(&failed, Some("inbox"));
        let out = trailing(&failed, &look, RowMode::Inbox, &now);
        assert_eq!((out.text.as_str(), out.kind), ("Failed", TrailingKind::Failed));

        let woke = thread(json!({ "woke": { "at": "2026-11-09T08:00:00Z", "since": "2026-11-07T08:00:00Z" } }));
        let look = RowLook::of(&woke, Some("inbox"));
        let out = trailing(&woke, &look, RowMode::Inbox, &now);
        assert_eq!(out.text, "Woke up");
        assert_eq!(out.tip.as_deref(), Some("Woke up after hibernating for 2 days"));

        let sleeping = thread(json!({ "snoozedUntil": "2026-11-09T22:00:00Z" }));
        assert_eq!(trailing(&sleeping, &RowLook::PLAIN, RowMode::Hibernating, &now).text, "Back Tue 9:00 am");

        let topic = thread(json!({ "snoozedUntil": "9999-12-31T00:00:00.000Z", "wakeWhen": { "said": "next time I talk about wood", "topic": "wood" } }));
        let out = trailing(&topic, &RowLook::PLAIN, RowMode::Hibernating, &now);
        assert_eq!(out.text, "wood");
        assert!(out.tip.unwrap().starts_with("Back when you next talk about wood."));

        let thinking = thread(json!({ "snoozedUntil": "9999-12-31T00:00:00.000Z", "wakeWhen": { "said": "soon", "pending": true } }));
        assert_eq!(trailing(&thinking, &RowLook::PLAIN, RowMode::Hibernating, &now).text, "Thinking…");

        let restart = thread(json!({ "snoozedUntil": "9999-12-31T00:00:00.000Z", "snoozedUntilRestart": true }));
        assert_eq!(trailing(&restart, &RowLook::PLAIN, RowMode::Hibernating, &now).text, "Back on restart");
    }

    #[test]
    fn hibernate_presets() {
        let now = at("2026-11-09T14:00:00+11:00"); // a Monday afternoon
        let presets = snooze_presets(&now);
        let ids: Vec<&str> = presets.iter().map(|p| p.id).collect();
        assert_eq!(ids, ["hour", "evening", "tomorrow", "monday"]);
        assert_eq!(presets[1].until_ms, at("2026-11-09T18:00:00+11:00").timestamp_millis());
        assert_eq!(presets[2].until_ms, at("2026-11-10T09:00:00+11:00").timestamp_millis());
        // The Monday after, not today's.
        assert_eq!(presets[3].until_ms, at("2026-11-16T09:00:00+11:00").timestamp_millis());

        // Within the hour before 6 pm, the evening one goes.
        let late = at("2026-11-09T17:30:00+11:00");
        assert_eq!(snooze_presets(&late).iter().map(|p| p.id).collect::<Vec<_>>(), ["hour", "tomorrow", "monday"]);
        assert_eq!(snooze_date("evening", &late), late.timestamp_millis() + 3_600_000);
    }

    #[test]
    fn subagent_tree() {
        let all: Vec<Subagent> = serde_json::from_value(json!([
            { "id": "a", "description": "Done long ago", "status": "completed", "startedAt": "2026-11-09T07:00:00Z", "endedAt": "2026-11-09T07:10:00Z" },
            { "id": "b", "description": "Running", "status": "running", "activity": "Reading **files**", "startedAt": "2026-11-09T08:58:00Z" },
            { "id": "c", "parentId": "b", "description": "Nested", "status": "running", "startedAt": "2026-11-09T08:59:00Z" },
            { "id": "d", "description": "Just failed", "status": "failed", "summary": "No such file", "startedAt": "2026-11-09T08:50:00Z", "endedAt": "2026-11-09T08:58:00Z" },
        ])).unwrap();
        let now = parse_ms("2026-11-09T09:00:00Z").unwrap();
        // Under Running: running ones and the recently failed one, with the nested one under its parent.
        let rows = subagent_rows(&all, false, None, now, true);
        assert_eq!(rows.iter().map(|r| (r.subagent.id.as_str(), r.depth)).collect::<Vec<_>>(), [("b", 0), ("c", 1), ("d", 0)]);
        // Under Inbox: running only.
        assert_eq!(subagent_rows(&all, false, None, now, false).iter().map(|r| r.subagent.id.as_str()).collect::<Vec<_>>(), ["b", "c"]);
        // Selected thread: its finished ones too.
        assert_eq!(subagent_rows(&all, true, None, now, true).iter().map(|r| r.subagent.id.as_str()).collect::<Vec<_>>(), ["a", "b", "c", "d"]);

        assert_eq!(subagent_detail(&all[1]), "Reading files");
        assert_eq!(subagent_detail(&all[3]), "Failed: No such file");
        assert_eq!(subagent_duration(&all[1], now), "2m");
    }

    #[test]
    fn preview_text() {
        assert_eq!(preview_plain("Done **fixing** the [bug](http://x) in `main`"), "Done fixing the bug in main");
        assert_eq!(preview_plain("# Heading\nmore   text"), "Heading more text");
    }

    #[test]
    fn archive_paging() {
        assert_eq!(archive_rows(30, 20, false, None), (20, 10));
        assert_eq!(archive_rows(30, 20, true, None), (30, 0));
        // The open thread is always in, even if it's past the limit.
        assert_eq!(archive_rows(30, 20, false, Some(25)), (26, 4));
        assert_eq!(archive_rows(10, 20, false, None), (10, 0));
    }

    #[test]
    fn row_modes_and_buttons() {
        let plain = thread(json!({}));
        assert_eq!(RowMode::of("inbox", &plain, 0).actions(), &[RowAction::Star, RowAction::Snooze, RowAction::Archive]);
        assert_eq!(RowMode::of("archived", &plain, 0).actions(), &[RowAction::Unarchive, RowAction::Delete]);
        let asleep = thread(json!({ "snoozedUntil": "2030-01-01T00:00:00Z" }));
        assert_eq!(RowMode::of("hibernating", &asleep, 0), RowMode::Hibernating);
        assert_eq!(RowMode::of("starred", &asleep, 0).actions(), &[RowAction::Star, RowAction::Unsnooze, RowAction::Archive]);
        let starred = thread(json!({ "starredAt": "2026-11-09T00:00:00Z" }));
        assert_eq!(RowAction::Star.tip(&starred), "Remove star");
        assert_eq!(RowAction::Star.symbol(&starred), "star.fill");
    }

    #[test]
    fn places_tiles_and_bylines() {
        let t = thread(json!({ "projects": [{ "id": "p1", "title": "Photo sync" }, { "id": "p2", "title": "Threadbear" }] }));
        assert_eq!(t.place(), "Photo sync, Threadbear");
        assert_eq!(tile(&t), (Tile::Letter("P".into()), true));
        let loose = thread(json!({ "cwd": "/Users/mike/dev/me/misc" }));
        assert_eq!(loose.place(), "misc");
        assert_eq!(tile(&loose), (Tile::Folder, false));
        assert_eq!(project_letter("7 day plan"), "7");
        assert_eq!(project_letter("🎉"), "?");

        let scheduled = thread(json!({ "startedBy": { "kind": "schedule", "label": "Schedule", "why": "every morning" } }));
        let line = byline(&scheduled).unwrap();
        assert_eq!(line.symbol, "clock");
        assert_eq!(line.tip, "Started by Schedule (every morning)");
        assert!(byline(&loose).is_none());
    }

    #[test]
    fn sort_orders() {
        use crate::inbox::{sections_ordered, InboxOrder, Section};
        let a = thread(json!({ "id": "old", "lastActivityAt": "2026-11-09T01:00:00Z" }));
        let b = thread(json!({ "id": "new", "lastActivityAt": "2026-11-09T08:00:00Z" }));
        let c = thread(json!({ "id": "asking", "status": "waiting", "lastActivityAt": "2026-11-09T04:00:00Z" }));
        let all = [a, b, c];
        let inbox = |s: &[Section]| s.iter().find(|x| x.key == "inbox").unwrap().rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
        assert_eq!(inbox(&sections_ordered(&all, 0, InboxOrder::NeedsYou)), ["asking", "new", "old"]);
        assert_eq!(inbox(&sections_ordered(&all, 0, InboxOrder::Newest)), ["new", "asking", "old"]);
        assert_eq!(inbox(&sections_ordered(&all, 0, InboxOrder::Oldest)), ["old", "asking", "new"]);
    }

    /// The shared urgency rule (`app/Tests/Fixtures/inbox-urgency-cases.json`):
    /// the Mac, the phone and the website read this file too.
    #[test]
    fn urgency_matches_the_shared_cases() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../app/Tests/Fixtures/inbox-urgency-cases.json");
        let cases: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for case in cases["cases"].as_array().unwrap() {
            let mut t = thread(json!({ "needsYou": case["thread"]["needsYou"].as_bool() == Some(true) }));
            t.pending_reviews = case["thread"]["pendingReviews"].as_u64();
            if let Some(status) = case["thread"]["status"].as_str() {
                t.status = status.into();
            }
            t.last_seq = if case["thread"]["unread"].as_bool() == Some(true) { 9 } else { 5 };
            let name = case["name"].as_str().unwrap();
            assert_eq!(t.urgency() as u64, case["urgency"].as_u64().unwrap(), "{name}");
            assert_eq!(t.wants_you(), case["counted"].as_bool().unwrap(), "{name}");
        }
    }

    #[test]
    fn keeps_times_in_the_given_zone() {
        // A fixed zone west of Melbourne, so a day boundary falls differently.
        let tz = FixedOffset::east_opt(-8 * 3600).unwrap();
        let now = tz.with_ymd_and_hms(2026, 11, 9, 20, 0, 0).unwrap();
        let evening = snooze_presets(&now).iter().find(|p| p.id == "tomorrow").unwrap().until_ms;
        assert_eq!(evening, tz.with_ymd_and_hms(2026, 11, 10, 9, 0, 0).unwrap().timestamp_millis());
    }
}
