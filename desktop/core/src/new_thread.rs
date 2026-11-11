// What the new-thread page works out: which pool and login a draft starts on
// (Pools.swift, AccountUsageText.pickAccount, AppModel.newThreadAccountId), the
// note under the composer and the room cards (NewThreadNote and AccountRoomCard
// in DetailView.swift, AccountName, AccountUsageText). The Swift types' names
// are in the comments; `desktop/gpui/src/new_thread.rs` draws what's here.
use crate::inbox::{folder_name, parse_ms};
use crate::logins::{provider_title, short_account_name, Account, Pool, RunnerLogins};
use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike};
use serde_json::Value;
use std::collections::HashMap;

// ---- Usage ----

/// `UsageWindow`, what `AccountUsageText` reads of one.
#[derive(Clone, Debug, PartialEq)]
pub struct UsageWindow {
    /// "five_hour", "weekly", "weekly_model", ...
    pub kind: String,
    pub title: String,
    pub remaining_percent: f64,
    /// Milliseconds since the epoch.
    pub resets_at: Option<i64>,
}

/// `AccountUsage`, read leniently from the `Value` an `Account` keeps.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountUsage {
    pub windows: Vec<UsageWindow>,
    pub blocked: bool,
}

/// The account's usage, if the runner sent any.
pub fn usage_of(account: &Account) -> Option<AccountUsage> {
    let v = account.usage.as_ref()?;
    Some(AccountUsage {
        windows: v["windows"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|w: &Value| {
                        Some(UsageWindow {
                            kind: w["kind"].as_str()?.to_owned(),
                            title: w["title"].as_str().unwrap_or("").to_owned(),
                            remaining_percent: w["remainingPercent"].as_f64()?,
                            resets_at: w["resetsAt"].as_str().and_then(parse_ms),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        blocked: v["blocked"].as_bool().unwrap_or(false),
    })
}

/// `AccountUsageText.mainWindows`: the 5-hour and weekly windows, the two
/// that decide whether there's room.
pub fn main_windows(usage: &AccountUsage) -> Vec<&UsageWindow> {
    ["five_hour", "weekly"].iter().filter_map(|kind| usage.windows.iter().find(|w| w.kind == *kind)).collect()
}

/// `AccountUsageText.tightest`: the tighter of the main windows, the one
/// that runs out first (the first of a tie, as `min` keeps it).
pub fn tightest(usage: &AccountUsage) -> Option<&UsageWindow> {
    main_windows(usage).into_iter().min_by(|a, b| a.remaining_percent.partial_cmp(&b.remaining_percent).unwrap_or(std::cmp::Ordering::Equal))
}

/// `AccountUsageText.room`.
pub fn room(usage: &AccountUsage) -> Option<f64> {
    tightest(usage).map(|w| w.remaining_percent)
}

/// `AccountUsageText.percent`: whole numbers, never "100%" for 99.6.
pub fn percent(value: f64) -> String {
    format!("{}%", value.floor() as i64)
}

fn short_name(window: &UsageWindow) -> String {
    match window.kind.as_str() {
        "five_hour" => "5h".to_owned(),
        "weekly" => "week".to_owned(),
        _ => window.title.clone(),
    }
}

/// `AccountUsageText.roomLine`: "5h 89% · week 78% left", or "Limit reached".
pub fn room_line(usage: Option<&AccountUsage>) -> Option<String> {
    let usage = usage?;
    if usage.blocked {
        return Some("Limit reached".to_owned());
    }
    let main = main_windows(usage);
    if main.is_empty() {
        return None;
    }
    Some(format!("{} left", main.iter().map(|w| format!("{} {}", short_name(w), percent(w.remaining_percent))).collect::<Vec<_>>().join(" · ")))
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `AccountUsageText.clock`: "3:40 pm", "1 pm" on the hour.
pub fn clock<Tz: TimeZone>(d: &DateTime<Tz>) -> String {
    let (h, m) = (d.hour(), d.minute());
    let twelve = if h % 12 == 0 { 12 } else { h % 12 };
    let suffix = if h < 12 { "am" } else { "pm" };
    if m == 0 {
        format!("{twelve} {suffix}")
    } else {
        format!("{twelve}:{m:02} {suffix}")
    }
}

fn day_name<Tz: TimeZone>(d: &DateTime<Tz>, now: &DateTime<Tz>) -> String {
    if d.date_naive() == (now.clone() + Duration::days(1)).date_naive() {
        return "tomorrow".to_owned();
    }
    if d.date_naive() == (now.clone() - Duration::days(1)).date_naive() {
        return "yesterday".to_owned();
    }
    WEEKDAYS[d.weekday().num_days_from_sunday() as usize].to_owned()
}

/// `AccountUsageText.resetTime`: "now", "3:40 pm", "Thu 1 pm", "12 Nov".
pub fn reset_time<Tz: TimeZone>(date: &DateTime<Tz>, now: &DateTime<Tz>) -> String {
    if date <= now {
        return "now".to_owned();
    }
    if date.date_naive() == now.date_naive() {
        return clock(date);
    }
    let days = (date.date_naive() - now.date_naive()).num_days();
    if days < 7 {
        return format!("{} {}", day_name(date, now), clock(date));
    }
    format!("{} {}", date.day(), MONTHS[date.month0() as usize])
}

/// `AccountUsageText.tooltip`: "mike@example.com: 5-hour 89% left, weekly
/// 78% left, resets Thu 1 pm."
pub fn room_tooltip<Tz: TimeZone>(account: &Account, now: &DateTime<Tz>) -> String {
    let mut text = account.name();
    if let Some(note) = account.note.as_deref().filter(|n| !n.is_empty()) {
        text.push_str(&format!(" ({note})"));
    }
    let Some(usage) = usage_of(account) else { return text };
    let main = main_windows(&usage);
    if usage.blocked {
        let reset = main.iter().filter(|w| w.remaining_percent < 1.).filter_map(|w| w.resets_at).min().or_else(|| main.iter().filter_map(|w| w.resets_at).min());
        let line = match reset {
            Some(ms) => format!("Limit reached, resets {}", reset_time(&at(ms, now), now)),
            None => "Limit reached".to_owned(),
        };
        return format!("{text}: {line}.");
    }
    if main.is_empty() {
        return text;
    }
    let mut words = main.iter().map(|w| format!("{} {} left", w.title, percent(w.remaining_percent))).collect::<Vec<_>>().join(", ");
    if let Some(ms) = tightest(&usage).and_then(|w| w.resets_at) {
        words.push_str(&format!(", resets {}", reset_time(&at(ms, now), now)));
    }
    format!("{text}: {words}.")
}

/// A moment in milliseconds in the same zone as `reference`.
fn at<Tz: TimeZone>(ms: i64, reference: &DateTime<Tz>) -> DateTime<Tz> {
    reference.timezone().timestamp_millis_opt(ms).single().unwrap_or_else(|| reference.clone())
}

// ---- Names ----

fn local_part(address: &str) -> &str {
    address.split('@').next().unwrap_or(address)
}

fn domain_part(address: &str) -> Option<String> {
    let at = address.find('@')?;
    let first = address[at + 1..].split('.').next().unwrap_or("");
    (!first.is_empty()).then(|| first.to_owned())
}

/// `AccountName.login`: the first part of its email's domain ("acme" for
/// sam@acme.io), or the part before the @ when another login shares that
/// domain, else the whole address. Codex reads as `short` has it.
pub fn login_name(id: &str, accounts: &[Account]) -> String {
    let Some(account) = accounts.iter().find(|a| a.id == id) else { return id.to_owned() };
    if account.provider_id() == "codex" {
        return short_account_name(id, accounts);
    }
    let full = account.name();
    let Some(domain) = domain_part(&full) else { return full };
    let siblings: Vec<String> = accounts.iter().filter(|a| a.id != id && a.provider_id() == account.provider_id()).map(Account::name).collect();
    if !siblings.iter().any(|s| domain_part(s).as_deref() == Some(domain.as_str())) {
        return domain;
    }
    let local = local_part(&full).to_owned();
    if siblings.iter().any(|s| local_part(s) == local) {
        full
    } else {
        local
    }
}

/// `AccountName.label`: a name for a card, where a whole address doesn't fit.
pub fn account_label(id: &str, accounts: &[Account]) -> String {
    let Some(account) = accounts.iter().find(|a| a.id == id) else { return id.to_owned() };
    if account.provider_id() == "codex" {
        return short_account_name(id, accounts);
    }
    let full = account.name();
    let Some(at) = full.find('@') else { return full };
    let siblings: Vec<String> = accounts.iter().filter(|a| a.id != id && a.provider_id() == account.provider_id()).map(Account::name).collect();
    let local = local_part(&full).to_owned();
    if !siblings.iter().any(|s| local_part(s) == local) {
        return local;
    }
    let domain = |address: &str| address.split('@').nth(1).unwrap_or(address).split('.').next().unwrap_or("").to_owned();
    let own = domain(&full[at..]);
    if own.is_empty() || siblings.iter().any(|s| domain(s) == own) {
        full
    } else {
        own
    }
}

// ---- Picking ----

/// `AccountUsageText.pickFloor` and `chipWarning`.
pub const PICK_FLOOR: f64 = 10.;
pub const CHIP_WARNING: f64 = 25.;

fn is_eligible(account: &Account) -> bool {
    let usage = usage_of(account);
    if usage.as_ref().is_some_and(|u| u.blocked) {
        return false;
    }
    if account.note.as_deref().is_some_and(|n| !n.is_empty()) {
        return false;
    }
    !usage.as_ref().and_then(room).is_some_and(|r| r < PICK_FLOOR)
}

/// `AccountUsageText.pickAccount`: the runner's rule for the account a new
/// thread starts on (see its Swift comment): the last-used account's provider,
/// never one that's blocked, nearly out or has a note; most room wins, the
/// last used on a tie; with every candidate skipped, the last-used one.
pub fn pick_account(accounts: &[Account], last_used: Option<&str>) -> Option<String> {
    if accounts.is_empty() {
        return None;
    }
    let last = accounts.iter().find(|a| Some(a.id.as_str()) == last_used);
    let provider = last.map_or(accounts[0].provider_id(), Account::provider_id);
    let eligible: Vec<&Account> = accounts.iter().filter(|a| a.provider_id() == provider && is_eligible(a)).collect();
    let fallback = last.map(|a| a.id.clone());
    if let Some(last) = last {
        if eligible.iter().any(|a| a.id == last.id) && usage_of(last).as_ref().and_then(room).is_none() {
            return Some(last.id.clone());
        }
    }
    let known: Vec<(&Account, f64)> = eligible.iter().filter_map(|a| usage_of(a).as_ref().and_then(room).map(|r| (*a, r))).collect();
    let Some(best) = known.iter().map(|(_, r)| *r).max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)) else { return fallback };
    if let Some(last) = last {
        if known.iter().any(|(a, r)| a.id == last.id && *r >= best) {
            return Some(last.id.clone());
        }
    }
    // `max` keeps the last of equals; the first in the runner's order wins a tie.
    known.iter().find(|(_, r)| *r == best).map(|(a, _)| a.id.clone()).or(fallback)
}

/// `Pools.forFolder`: the pool whose `folders` hold `cwd`, the most specific
/// folder winning. The runner has the same rule.
pub fn pool_for_folder(pools: &[Pool], cwd: &str, home: &str) -> Option<String> {
    let folder = normalised(cwd, home);
    if folder.is_empty() {
        return None;
    }
    let mut best: Option<(String, usize)> = None;
    for pool in pools {
        for entry in &pool.folders {
            let root = normalised(entry, home);
            if root.is_empty() {
                continue;
            }
            let inside = folder == root || folder.starts_with(&if root == "/" { "/".to_owned() } else { format!("{root}/") });
            if inside && best.as_ref().is_none_or(|(_, length)| root.len() > *length) {
                best = Some((pool.id.clone(), root.len()));
            }
        }
    }
    best.map(|(id, _)| id)
}

fn normalised(path: &str, home: &str) -> String {
    let mut text = path.trim().to_owned();
    if text == "~" {
        text = home.to_owned();
    } else if let Some(rest) = text.strip_prefix("~/") {
        text = format!("{home}/{rest}");
    }
    while text.len() > 1 && text.ends_with('/') {
        text.pop();
    }
    text
}

/// `Pools.newThreadPool`: the one Mike picked for this draft, else its
/// folder's, else the one he last picked, else the runner's default, else the
/// first. A pool that's gone is skipped. None only with no pools at all.
pub fn new_thread_pool(pools: &[Pool], picked: Option<&str>, cwd: &str, home: &str, last: Option<&str>, runner_default: Option<&str>) -> Option<String> {
    let has = |id: &str| pools.iter().any(|p| p.id == id);
    if let Some(picked) = picked.filter(|p| has(p)) {
        return Some(picked.to_owned());
    }
    if let Some(folder) = pool_for_folder(pools, cwd, home) {
        return Some(folder);
    }
    if let Some(last) = last.filter(|p| has(p)) {
        return Some(last.to_owned());
    }
    if let Some(default) = runner_default.filter(|p| has(p)) {
        return Some(default.to_owned());
    }
    pools.first().map(|p| p.id.clone())
}

/// `Pools.logins`: the pool's logins the runner has, in the pool's order,
/// optionally only one provider's.
pub fn pool_logins<'a>(pool: &Pool, accounts: &'a [Account], provider: Option<&str>) -> Vec<&'a Account> {
    pool.account_ids.iter().filter_map(|id| accounts.iter().find(|a| a.id == *id)).filter(|a| provider.is_none_or(|p| a.provider_id() == p)).collect()
}

/// `Pools.providers`: the providers in the pool, in the order its logins name them.
pub fn pool_providers(pool: &Pool, accounts: &[Account]) -> Vec<String> {
    let mut seen = Vec::new();
    for account in pool_logins(pool, accounts, None) {
        let provider = account.provider_id().to_owned();
        if !seen.contains(&provider) {
            seen.push(provider);
        }
    }
    seen
}

/// `AccountUsageText.ChipBadge`: after a chip's title, "· 12%" in orange
/// when a login is running out, "Limit reached" in red at its limit.
#[derive(Clone, Debug, PartialEq)]
pub enum ChipBadge {
    Low(String),
    Blocked,
}

/// `AccountUsageText.chipBadge`, for one login.
pub fn chip_badge(account: Option<&Account>) -> Option<ChipBadge> {
    let usage = usage_of(account?)?;
    if usage.blocked {
        return Some(ChipBadge::Blocked);
    }
    let left = room(&usage)?;
    (left < CHIP_WARNING).then(|| ChipBadge::Low(percent(left)))
}

/// `Pools.bestRoom`: the most room any of its logins has, a blocked one
/// counting as none.
pub fn best_room(pool: &Pool, accounts: &[Account]) -> Option<f64> {
    pool_logins(pool, accounts, None)
        .into_iter()
        .filter_map(|a| {
            let usage = usage_of(a)?;
            if usage.blocked {
                Some(0.)
            } else {
                room(&usage)
            }
        })
        .fold(None, |best: Option<f64>, r| Some(best.map_or(r, |b| b.max(r))))
}

/// `Pools.chipBadge`: the model chip's badge on a pool thread, only once
/// every login in the pool is running out (or all are at their limit).
pub fn pool_chip_badge(pool: &Pool, accounts: &[Account]) -> Option<ChipBadge> {
    let usages: Vec<Option<AccountUsage>> = pool_logins(pool, accounts, None).into_iter().map(usage_of).collect();
    if usages.is_empty() {
        return None;
    }
    if usages.iter().all(|u| u.as_ref().is_some_and(|u| u.blocked)) {
        return Some(ChipBadge::Blocked);
    }
    let low = usages.iter().all(|u| match u {
        Some(u) if u.blocked => true,
        Some(u) => room(u).is_some_and(|r| r < CHIP_WARNING),
        None => false,
    });
    if !low {
        return None;
    }
    best_room(pool, accounts).map(|best| ChipBadge::Low(percent(best)))
}

/// `AccountUsageText.switchTarget`: where a thread that hit its limit could
/// go: the login with the most room (known, and at least the floor), on the
/// same provider first, never one with a note or at its limit.
pub fn switch_target<'a>(accounts: &'a [Account], from: &str) -> Option<&'a Account> {
    let room_of = |a: &Account| usage_of(a).as_ref().and_then(room);
    let others: Vec<&'a Account> = accounts.iter().filter(|a| a.id != from && is_eligible(a) && room_of(a).is_some()).collect();
    let best = |list: Vec<&'a Account>| -> Option<&'a Account> {
        let top = list.iter().filter_map(|a| room_of(a)).fold(None, |m: Option<f64>, r| Some(m.map_or(r, |m| m.max(r))))?;
        list.into_iter().find(|a| room_of(a) == Some(top))
    };
    if let Some(provider) = accounts.iter().find(|a| a.id == from).map(|a| a.provider_id()) {
        if let Some(same) = best(others.iter().copied().filter(|a| a.provider_id() == provider).collect()) {
            return Some(same);
        }
    }
    best(others)
}

/// `AccountUsageText.switchTitle`: "Switch to mike (78% left)".
pub fn switch_title(target: &Account, accounts: &[Account]) -> String {
    let mut title = format!("Switch to {}", short_account_name(&target.id, accounts));
    if let Some(left) = usage_of(target).as_ref().and_then(room) {
        title.push_str(&format!(" ({} left)", percent(left)));
    }
    title
}

/// `Pools.hasRoom`: not blocked, and at or over the floor (or usage unknown).
pub fn has_room(account: &Account) -> bool {
    let Some(usage) = usage_of(account) else { return true };
    !usage.blocked && !room(&usage).is_some_and(|r| r < PICK_FLOOR)
}

fn weekly_reset(account: &Account, now_ms: i64) -> f64 {
    let reset = usage_of(account).and_then(|u| u.windows.iter().find(|w| w.kind == "weekly").and_then(|w| w.resets_at));
    match reset {
        Some(ms) if ms > now_ms => (ms as f64 / 60_000.).round(),
        _ => f64::INFINITY,
    }
}

/// `Pools.pickLogin`: the login a turn of `provider` would run on: `current`
/// while it has room, else, of those with room, the one whose weekly window
/// resets soonest (the most room breaking a tie), as the runner's `pickInPool`;
/// else one whose usage isn't known, else the first.
pub fn pick_login(pool: &Pool, accounts: &[Account], provider: &str, current: Option<&str>, now_ms: i64) -> Option<String> {
    let own = pool_logins(pool, accounts, Some(provider));
    if let Some(current) = current {
        if own.iter().any(|a| a.id == current && has_room(a)) {
            return Some(current.to_owned());
        }
    }
    let open: Vec<&&Account> = own.iter().filter(|a| has_room(a)).collect();
    let mut best: Option<(String, f64, f64)> = None;
    for login in &open {
        let Some(login_room) = usage_of(login).as_ref().and_then(room) else { continue };
        let reset = weekly_reset(login, now_ms);
        if best.as_ref().is_some_and(|(_, have_room, have_reset)| !(reset < *have_reset || (reset == *have_reset && login_room > *have_room))) {
            continue;
        }
        best = Some((login.id.clone(), login_room, reset));
    }
    if let Some((id, _, _)) = best {
        return Some(id);
    }
    own.iter().find(|a| has_room(a)).or(own.first()).map(|a| a.id.clone())
}

/// `UsageGroups.byProvider`: Claude's logins, then Codex's, then the rest,
/// each in the runner's order. Flattened, as the room cards want them.
pub fn by_provider(accounts: &[Account]) -> Vec<&Account> {
    let mut order: Vec<String> = Vec::new();
    for a in accounts {
        if !order.iter().any(|p| p == a.provider_id()) {
            order.push(a.provider_id().to_owned());
        }
    }
    let rank = |p: &str| match p {
        "claude" => 0,
        "codex" => 1,
        _ => 2,
    };
    let mut indexed: Vec<(usize, &String)> = order.iter().enumerate().collect();
    indexed.sort_by_key(|(i, p)| (rank(p), *i));
    indexed.into_iter().flat_map(|(_, p)| accounts.iter().filter(move |a| a.provider_id() == p.as_str())).collect()
}

// ---- The draft ----

/// What Mike has picked for the next new thread (AppModel's newThread*);
/// each is cleared when it's sent, except what `Remembered` keeps.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewThreadPicks {
    pub cwd: Option<String>,
    pub account: Option<String>,
    pub pool: Option<String>,
    pub provider: Option<String>,
    pub permission: Option<String>,
    pub project: Option<String>,
    /// The runner it starts on; None for this Mac.
    pub runner: Option<String>,
    pub runner_picked: bool,
}

/// What the next new thread remembers across sends (AppModel's UserDefaults:
/// lastAccountId, lastPoolId, lastModels, lastEfforts). Saved in the app's
/// own settings file (desktop/gpui/src/prefs.rs).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Remembered {
    pub last_account: Option<String>,
    pub last_account_by_provider: HashMap<String, String>,
    pub last_pool: Option<String>,
    pub last_pool_providers: HashMap<String, String>,
    pub last_models: HashMap<String, String>,
    pub last_efforts: HashMap<String, String>,
    /// The runner new threads start on ("Runs on"'s star), nil for this Mac.
    pub default_runner: Option<String>,
}

impl Remembered {
    /// AppModel.rememberLastAccount.
    pub fn used_account(&mut self, account: &str, provider: &str) {
        self.last_account = Some(account.to_owned());
        self.last_account_by_provider.insert(provider.to_owned(), account.to_owned());
    }

    /// AppModel.rememberPool.
    pub fn used_pool(&mut self, pool: &str, provider: Option<&str>) {
        self.last_pool = Some(pool.to_owned());
        if let Some(provider) = provider {
            self.last_pool_providers.insert(pool.to_owned(), provider.to_owned());
        }
    }

    /// AppModel.remember(model:provider:); empty clears it.
    pub fn set_model(&mut self, provider: &str, model: Option<&str>) {
        match model.filter(|m| !m.is_empty()) {
            Some(m) => self.last_models.insert(provider.to_owned(), m.to_owned()),
            None => self.last_models.remove(provider),
        };
    }

    pub fn set_effort(&mut self, provider: &str, effort: Option<&str>) {
        match effort.filter(|e| !e.is_empty()) {
            Some(e) => self.last_efforts.insert(provider.to_owned(), e.to_owned()),
            None => self.last_efforts.remove(provider),
        };
    }
}

/// One login's card on the page (AccountRoomCard).
#[derive(Clone, Debug, PartialEq)]
pub struct RoomCard {
    pub id: String,
    pub label: String,
    pub note: Option<String>,
    /// The roomLine, or "Nothing known yet".
    pub line: String,
    pub tooltip: String,
    pub selected: bool,
    /// How much of the bar is filled, 0 to 1; none with nothing known.
    pub fraction: Option<f64>,
    pub look: RoomLook,
}

/// The bar's colour (AccountRoomCard.fill): grey with room, orange under a
/// quarter, red at the limit; `Blocked` also turns the words red.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomLook {
    Ample,
    Low,
    Blocked,
}

/// The new-thread draft against a set of logins: what the composer's chips
/// and the note under it say.
pub struct Draft<'a> {
    pub logins: &'a RunnerLogins,
    pub picks: &'a NewThreadPicks,
    pub mem: &'a Remembered,
    pub home: &'a str,
    pub now_ms: i64,
}

impl Draft<'_> {
    /// Where it will start.
    pub fn cwd(&self) -> String {
        self.picks.cwd.clone().unwrap_or_else(|| self.logins.defaults.cwd.clone())
    }

    /// `newThreadUsesPools`: pools are on when the draft's logins have them.
    pub fn uses_pools(&self) -> bool {
        !self.logins.pools.is_empty()
    }

    /// `AppModel.lastPoolId`.
    fn last_pool(&self) -> Option<&str> {
        self.mem.last_pool.as_deref().or(self.logins.defaults.pool_id.as_deref())
    }

    /// `AppModel.newThreadPoolId`: None when it's pinned to a login.
    pub fn pool_id(&self) -> Option<String> {
        if !self.uses_pools() || self.picks.account.is_some() {
            return None;
        }
        new_thread_pool(&self.logins.pools, self.picks.pool.as_deref(), &self.cwd(), self.home, self.last_pool(), self.logins.defaults.pool_id.as_deref())
    }

    pub fn pool(&self) -> Option<&Pool> {
        self.pool_id().and_then(|id| self.logins.pool(&id))
    }

    /// `AppModel.newThreadProvider(in:)`.
    pub fn provider_in(&self, pool: &Pool) -> String {
        let providers = pool_providers(pool, &self.logins.accounts);
        for candidate in [self.picks.provider.as_deref(), self.mem.last_pool_providers.get(&pool.id).map(String::as_str)].into_iter().flatten() {
            if providers.iter().any(|p| p == candidate) {
                return candidate.to_owned();
            }
        }
        providers.first().cloned().unwrap_or_else(|| "claude".to_owned())
    }

    /// `AppModel.newThreadAccountId`.
    pub fn account_id(&self) -> String {
        if let Some(account) = self.picks.account.as_deref().filter(|a| self.picks.runner.is_none() || self.logins.account(a).is_some()) {
            return account.to_owned();
        }
        if let Some(pool) = self.pool() {
            let provider = self.provider_in(pool);
            if let Some(login) = pick_login(pool, &self.logins.accounts, &provider, self.mem.last_account_by_provider.get(&provider).map(String::as_str), self.now_ms) {
                return login;
            }
        }
        let last = self.mem.last_account.as_deref().unwrap_or(&self.logins.defaults.account_id);
        // On another runner only one of its own logins will do, so nothing
        // picked means its default; on this Mac it's the last one used.
        let fallback = if self.picks.runner.is_some() { self.logins.defaults.account_id.clone() } else { last.to_owned() };
        pick_account(&self.logins.accounts, Some(last)).unwrap_or(fallback)
    }

    pub fn provider(&self) -> String {
        self.logins.account(&self.account_id()).map_or_else(|| "claude".to_owned(), |a| a.provider_id().to_owned())
    }

    /// `AppModel.newThreadPermission`: picked, else the runner's default,
    /// else full access.
    pub fn permission(&self) -> String {
        self.picks.permission.clone().or_else(|| self.logins.defaults.permission_mode.clone()).filter(|m| !m.is_empty()).unwrap_or_else(|| "full".to_owned())
    }

    /// What the note names after "on": the pool, or the login.
    fn on(&self) -> String {
        match self.pool() {
            Some(pool) => format!("the {} pool", pool.title),
            None => {
                let account = self.account_id();
                if self.uses_pools() {
                    login_name(&account, &self.logins.accounts)
                } else {
                    short_account_name(&account, &self.logins.accounts)
                }
            }
        }
    }

    /// NewThreadNote's words: "Starts in misc on the Claude pool", or with
    /// another runner, "Starts in misc on bruce, with the Claude pool"; a
    /// runner whose hello hasn't come says only "Starts on bruce".
    pub fn note(&self, runner_name: Option<&str>, logins_known: bool) -> String {
        match runner_name {
            Some(name) if !logins_known => format!("Starts on {name}"),
            Some(name) => format!("Starts in {} on {name}, with {}", short_folder(&self.cwd()), self.on()),
            None => format!("Starts in {} on {}", short_folder(&self.cwd()), self.on()),
        }
    }

    /// NewThreadNote's tooltip.
    pub fn note_tip(&self, runner_name: Option<&str>) -> String {
        let cwd = self.cwd();
        let what = match self.pool() {
            Some(pool) => format!("the {} pool, which picks the login", pool.title),
            None => {
                let account = self.account_id();
                self.logins.account(&account).map_or(account, Account::name)
            }
        };
        match runner_name {
            Some(name) => format!("{cwd} on {name}, with {what}"),
            None => format!("{} on {what}", abbreviate_home(&cwd, self.home)),
        }
    }

    /// The login cards under the note (NewThreadNote's grid): the pool's
    /// logins, or every one when the draft isn't in a pool; empty unless
    /// there's more than one and any has usage to compare.
    pub fn room_cards<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> Vec<RoomCard> {
        let pool = self.pool();
        let selected = self.account_id();
        let cards: Vec<&Account> = by_provider(&self.logins.accounts).into_iter().filter(|a| pool.is_none_or(|p| p.account_ids.contains(&a.id))).collect();
        if cards.len() <= 1 || !cards.iter().any(|a| a.usage.is_some()) {
            return Vec::new();
        }
        cards
            .iter()
            .map(|account| {
                let usage = usage_of(account);
                let tight = usage.as_ref().and_then(tightest);
                let look = match tight {
                    Some(t) if usage.as_ref().is_some_and(|u| u.blocked) || t.remaining_percent < 1. => RoomLook::Blocked,
                    Some(t) if t.remaining_percent < CHIP_WARNING => RoomLook::Low,
                    _ => RoomLook::Ample,
                };
                RoomCard {
                    id: account.id.clone(),
                    label: account_label(&account.id, &self.logins.accounts),
                    note: account.note.clone().filter(|n| !n.is_empty()),
                    line: room_line(usage.as_ref()).unwrap_or_else(|| "Nothing known yet".to_owned()),
                    tooltip: room_tooltip(account, now),
                    selected: pool.is_none() && account.id == selected,
                    fraction: tight.map(|t| (t.remaining_percent / 100.).clamp(0., 1.)),
                    look,
                }
            })
            .collect()
    }

    /// `threads.create`'s params for this draft (AppModel.newThreadRequest):
    /// the text, its pictures' JSON, and the folder, pool or login, model,
    /// effort and mode. `away` is a runner that hasn't said its logins yet.
    pub fn create_params(&self, text: &str, images: Vec<Value>, away: bool) -> Value {
        let mut out = serde_json::json!({ "text": text, "permissionMode": self.permission() });
        if !images.is_empty() {
            out["images"] = Value::Array(images);
        }
        if away {
            if let Some(cwd) = &self.picks.cwd {
                out["cwd"] = serde_json::json!(cwd);
            }
            return out;
        }
        out["cwd"] = serde_json::json!(self.cwd());
        if let Some(pool) = self.pool() {
            let provider = self.provider_in(pool);
            out["poolId"] = serde_json::json!(pool.id);
            let mut model = self.mem.last_models.get(&provider).cloned();
            if model.is_none() && pool_providers(pool, &self.logins.accounts).len() > 1 {
                model = self.logins.defaults.model_for(&provider).map(str::to_owned).or_else(|| self.logins.account(&self.account_id()).and_then(|a| a.default_model.clone()));
            }
            if let Some(model) = model {
                out["model"] = serde_json::json!(model);
            }
            if let Some(effort) = self.mem.last_efforts.get(&provider) {
                out["effort"] = serde_json::json!(effort);
            }
            return out;
        }
        let account = self.account_id();
        out["accountId"] = serde_json::json!(account);
        let provider = self.logins.account(&account).map_or("claude", Account::provider_id).to_owned();
        if let Some(model) = self.mem.last_models.get(&provider) {
            out["model"] = serde_json::json!(model);
        }
        if let Some(effort) = self.mem.last_efforts.get(&provider) {
            out["effort"] = serde_json::json!(effort);
        }
        out
    }
}

/// `FolderName.short`: the last part of a path ("/" for the root).
pub fn short_folder(path: &str) -> String {
    if path.trim_matches('/').is_empty() {
        return if path.is_empty() { String::new() } else { "/".to_owned() };
    }
    folder_name(path)
}

/// `String.abbreviatingHome`.
pub fn abbreviate_home(path: &str, home: &str) -> String {
    if !home.is_empty() && path == home {
        "~".to_owned()
    } else if !home.is_empty() && path.starts_with(&format!("{home}/")) {
        format!("~{}", &path[home.len()..])
    } else {
        path.to_owned()
    }
}

/// The page's heading (`NewThreadHeading`).
pub const HEADING: &str = "What should we work on?";

/// The title `provider_title` gives, kept here for the page's tooltips.
pub fn provider_name(provider: &str) -> String {
    provider_title(provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};
    use serde_json::json;

    fn usage(five: f64, week: f64) -> Value {
        json!({ "blocked": false, "windows": [
            { "kind": "five_hour", "title": "5-hour", "remainingPercent": five, "resetsAt": "2026-11-09T18:00:00.000Z" },
            { "kind": "weekly", "title": "Weekly", "remainingPercent": week }
        ]})
    }

    fn logins(pools: Value, cwd: &str) -> RunnerLogins {
        RunnerLogins::from_hello(&json!({
            "accounts": [
                { "id": "claude-main", "provider": "claude", "email": "mike@gmail.com", "usage": usage(52.0, 67.0) },
                { "id": "claude-2", "provider": "claude", "email": "mike@convex.dev", "note": "Convex work only", "usage": usage(90.0, 90.0) },
                { "id": "claude-3", "provider": "claude", "email": "mike@bearworks.io", "usage": usage(20.0, 88.0) },
                { "id": "codex-main", "provider": "codex", "title": "Codex · sam@example.com" }
            ],
            "pools": pools,
            "defaults": { "cwd": cwd, "accountId": "claude-main", "poolId": "claude", "permissionMode": "auto" }
        }))
    }

    #[test]
    fn a_thread_at_its_limit_is_offered_the_login_with_most_room() {
        let l = with_pools();
        // claude-main has 52% (5h, the tighter), claude-3 20%, claude-2 a note.
        let target = switch_target(&l.accounts, "claude-3").expect("somewhere to go");
        assert_eq!(target.id, "claude-main");
        // Three Claude logins share "mike", so the whole address (AccountName.short).
        assert_eq!(switch_title(target, &l.accounts), "Switch to mike@gmail.com (52% left)");
        // The noted one is never offered, and codex has no room known.
        assert_eq!(switch_target(&l.accounts, "claude-main").map(|a| a.id.as_str()), Some("claude-3"));
    }

    #[test]
    fn a_pools_chip_badge_waits_for_every_login_to_run_low() {
        let mut l = with_pools();
        let pool = l.pool("claude").unwrap().clone();
        assert_eq!(pool_chip_badge(&pool, &l.accounts), None, "claude-main still has room");
        l.accounts[0].usage = Some(usage(12.0, 40.0));
        assert_eq!(pool_chip_badge(&pool, &l.accounts), Some(ChipBadge::Low("20%".into())), "the best room of two low logins");
        for a in l.accounts.iter_mut() {
            a.usage = Some(json!({ "blocked": true, "windows": [] }));
        }
        assert_eq!(pool_chip_badge(&pool, &l.accounts), Some(ChipBadge::Blocked));
        assert_eq!(chip_badge(l.accounts.first()), Some(ChipBadge::Blocked));
    }

    fn with_pools() -> RunnerLogins {
        logins(json!([
            { "id": "claude", "title": "Claude", "accountIds": ["claude-main", "claude-3"] },
            { "id": "convex", "title": "Convex", "accountIds": ["claude-2"], "folders": ["~/dev/convex"] }
        ]), "/Users/mike/dev/me/misc")
    }

    fn no_pools() -> RunnerLogins {
        logins(json!([]), "/Users/mike/dev/me/misc")
    }

    fn draft<'a>(l: &'a RunnerLogins, picks: &'a NewThreadPicks, mem: &'a Remembered) -> Draft<'a> {
        Draft { logins: l, picks, mem, home: "/Users/mike", now_ms: 1_762_600_000_000 }
    }

    #[test]
    fn the_note_says_where_a_draft_starts() {
        let l = with_pools();
        let (picks, mem) = (NewThreadPicks::default(), Remembered::default());
        assert_eq!(draft(&l, &picks, &mem).note(None, true), "Starts in misc on the Claude pool");
    }

    #[test]
    fn the_folder_decides_the_pool() {
        let l = with_pools();
        let picks = NewThreadPicks { cwd: Some("/Users/mike/dev/convex/app".into()), ..Default::default() };
        let mem = Remembered::default();
        let d = draft(&l, &picks, &mem);
        assert_eq!(d.pool_id().as_deref(), Some("convex"));
        assert_eq!(d.note(None, true), "Starts in app on the Convex pool");
    }

    #[test]
    fn pinning_a_login_leaves_the_pool() {
        let l = with_pools();
        let picks = NewThreadPicks { account: Some("claude-3".into()), ..Default::default() };
        let mem = Remembered::default();
        let d = draft(&l, &picks, &mem);
        assert_eq!(d.pool_id(), None);
        assert_eq!(d.account_id(), "claude-3");
        assert_eq!(d.note(None, true), "Starts in misc on bearworks");
        assert_eq!(d.note_tip(None), "~/dev/me/misc on mike@bearworks.io");
    }

    #[test]
    fn without_pools_the_note_uses_the_short_name() {
        let l = no_pools();
        let (picks, mem) = (NewThreadPicks::default(), Remembered::default());
        let d = draft(&l, &picks, &mem);
        // claude-3 has the least room (20), so the note follows the picked
        // account: claude-main (67) wins, and its email's local part is shared.
        assert_eq!(d.account_id(), "claude-main");
        assert_eq!(d.note(None, true), "Starts in misc on mike@gmail.com");
    }

    #[test]
    fn another_runner_is_named() {
        let l = no_pools();
        let picks = NewThreadPicks { runner: Some("r".into()), ..Default::default() };
        let mem = Remembered::default();
        let d = draft(&l, &picks, &mem);
        assert_eq!(d.note(Some("bruce"), false), "Starts on bruce");
        assert!(d.note(Some("bruce"), true).starts_with("Starts in misc on bruce, with "));
    }

    #[test]
    fn room_cards_are_the_pools_logins_with_usage() {
        let l = with_pools();
        let (picks, mem) = (NewThreadPicks::default(), Remembered::default());
        let now = FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2026, 11, 9, 9, 0, 0).unwrap();
        let cards = draft(&l, &picks, &mem).room_cards(&now);
        let lines: Vec<(&str, &str, bool)> = cards.iter().map(|c| (c.label.as_str(), c.line.as_str(), c.selected)).collect();
        // In a pool none is ringed: the pool picks.
        assert_eq!(lines, vec![("gmail", "5h 52% · week 67% left", false), ("bearworks", "5h 20% · week 88% left", false)]);
        assert_eq!(cards[1].look, RoomLook::Low);
        assert_eq!(cards[0].fraction, Some(0.52));
        assert_eq!(cards[0].tooltip, "mike@gmail.com: 5-hour 52% left, Weekly 67% left, resets 6 pm.");
    }

    #[test]
    fn one_account_has_nothing_to_compare() {
        let l = RunnerLogins::from_hello(&json!({
            "accounts": [{ "id": "claude-main", "provider": "claude", "email": "mike@gmail.com", "usage": usage(52.0, 67.0) }],
            "pools": [], "defaults": { "cwd": "/x", "accountId": "claude-main" }
        }));
        let (picks, mem) = (NewThreadPicks::default(), Remembered::default());
        let now = FixedOffset::east_opt(0).unwrap().timestamp_millis_opt(0).unwrap();
        assert!(draft(&l, &picks, &mem).room_cards(&now).is_empty());
    }

    #[test]
    fn a_pinned_card_is_ringed_outside_a_pool() {
        let l = no_pools();
        let picks = NewThreadPicks { account: Some("claude-3".into()), ..Default::default() };
        let mem = Remembered::default();
        let now = FixedOffset::east_opt(0).unwrap().timestamp_millis_opt(0).unwrap();
        let cards = draft(&l, &picks, &mem).room_cards(&now);
        assert_eq!(cards.iter().filter(|c| c.selected).map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["claude-3"]);
    }

    #[test]
    fn blocked_accounts_read_as_at_their_limit() {
        let blocked = Account { id: "b".into(), usage: Some(json!({ "blocked": true, "windows": [{ "kind": "weekly", "title": "Weekly", "remainingPercent": 0.0 }] })), ..Default::default() };
        let usage = usage_of(&blocked).unwrap();
        assert_eq!(room_line(Some(&usage)), Some("Limit reached".to_owned()));
        assert!(!has_room(&blocked));
    }

    #[test]
    fn pick_account_follows_the_runners_rule() {
        let l = no_pools();
        // The most room among Claude logins without a note.
        assert_eq!(pick_account(&l.accounts, Some("claude-3")).as_deref(), Some("claude-main"));
        // A last-used login with its usage unknown stays.
        let unknown = Account { id: "x".into(), provider: "claude".into(), ..Default::default() };
        assert_eq!(pick_account(&[unknown.clone()], Some("x")).as_deref(), Some("x"));
        // Nothing eligible on the provider: the last used, if there is one.
        let noted = Account { id: "n".into(), provider: "claude".into(), note: Some("work".into()), ..Default::default() };
        assert_eq!(pick_account(&[noted], Some("n")).as_deref(), Some("n"));
    }

    #[test]
    fn pick_login_keeps_the_current_one_while_it_has_room() {
        let l = with_pools();
        let pool = l.pool("claude").unwrap();
        assert_eq!(pick_login(pool, &l.accounts, "claude", Some("claude-3"), 0).as_deref(), Some("claude-3"));
        assert_eq!(pick_login(pool, &l.accounts, "claude", None, 0).as_deref(), Some("claude-main"));
    }

    #[test]
    fn folders_win_over_the_last_pool() {
        let l = with_pools();
        assert_eq!(pool_for_folder(&l.pools, "/Users/mike/dev/convex/x", "/Users/mike").as_deref(), Some("convex"));
        assert_eq!(pool_for_folder(&l.pools, "/Users/mike/dev/me", "/Users/mike"), None);
    }

    #[test]
    fn new_thread_pool_order() {
        let l = with_pools();
        assert_eq!(new_thread_pool(&l.pools, Some("convex"), "/x", "/h", None, None).as_deref(), Some("convex"));
        assert_eq!(new_thread_pool(&l.pools, Some("gone"), "/x", "/h", Some("claude"), None).as_deref(), Some("claude"));
        assert_eq!(new_thread_pool(&l.pools, None, "/x", "/h", Some("gone"), Some("convex")).as_deref(), Some("convex"));
        assert_eq!(new_thread_pool(&l.pools, None, "/x", "/h", None, None).as_deref(), Some("claude"));
        assert_eq!(new_thread_pool(&[], None, "/x", "/h", None, None), None);
    }

    #[test]
    fn create_params_name_the_pool_or_the_login() {
        let l = with_pools();
        let mut mem = Remembered::default();
        mem.set_model("claude", Some("claude-opus-5-5"));
        mem.set_effort("claude", Some("high"));
        let picks = NewThreadPicks::default();
        let d = draft(&l, &picks, &mem);
        let params = d.create_params("hello", vec![], false);
        assert_eq!(params["poolId"], "claude");
        assert_eq!(params["cwd"], "/Users/mike/dev/me/misc");
        assert_eq!(params["model"], "claude-opus-5-5");
        assert_eq!(params["effort"], "high");
        assert_eq!(params["permissionMode"], "auto");
        assert!(params.get("accountId").is_none());
        let picks = NewThreadPicks { account: Some("codex-main".into()), permission: Some("ask".into()), ..Default::default() };
        let d = draft(&l, &picks, &mem);
        let params = d.create_params("hello", vec![], false);
        assert_eq!(params["accountId"], "codex-main");
        assert_eq!(params["permissionMode"], "ask");
        assert!(params.get("poolId").is_none());
        assert!(params.get("model").is_none(), "the claude model doesn't follow a codex login");
    }

    #[test]
    fn a_mixed_pool_always_names_its_model() {
        let l = RunnerLogins::from_hello(&json!({
            "accounts": [
                { "id": "codex-main", "provider": "codex", "defaultModel": "gpt-6-luna" },
                { "id": "claude-1", "provider": "claude", "defaultModel": "claude-fable-5-1" }
            ],
            "pools": [{ "id": "mixed", "title": "Mixed", "accountIds": ["codex-main", "claude-1"] }],
            "defaults": { "cwd": "/x", "accountId": "codex-main", "models": { "codex": "gpt-6-luna" } }
        }));
        let (picks, mem) = (NewThreadPicks::default(), Remembered::default());
        let d = draft(&l, &picks, &mem);
        assert_eq!(d.provider_in(l.pool("mixed").unwrap()), "codex");
        assert_eq!(d.create_params("x", vec![], false)["model"], "gpt-6-luna");
    }

    #[test]
    fn a_runner_with_no_logins_yet_gets_only_the_mode() {
        let l = RunnerLogins::default();
        let picks = NewThreadPicks { runner: Some("r".into()), cwd: Some("/there".into()), ..Default::default() };
        let mem = Remembered::default();
        let params = draft(&l, &picks, &mem).create_params("x", vec![], true);
        assert_eq!(params["cwd"], "/there");
        assert_eq!(params["permissionMode"], "full");
        assert!(params.get("accountId").is_none());
    }

    #[test]
    fn labels_tell_the_logins_apart() {
        let l = no_pools();
        assert_eq!(account_label("claude-main", &l.accounts), "gmail");
        assert_eq!(account_label("claude-3", &l.accounts), "bearworks");
        assert_eq!(account_label("codex-main", &l.accounts), "Codex");
        assert_eq!(login_name("claude-2", &l.accounts), "convex");
    }

    #[test]
    fn claude_comes_before_codex_on_the_cards() {
        let accounts = vec![
            Account { id: "c1".into(), provider: "codex".into(), ..Default::default() },
            Account { id: "a1".into(), provider: "claude".into(), ..Default::default() },
        ];
        assert_eq!(by_provider(&accounts).iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["a1", "c1"]);
    }

    #[test]
    fn reset_times_read_as_the_app_says_them() {
        let tz = FixedOffset::east_opt(0).unwrap();
        let now = tz.with_ymd_and_hms(2026, 11, 9, 9, 0, 0).unwrap();
        assert_eq!(reset_time(&now, &now), "now");
        assert_eq!(reset_time(&tz.with_ymd_and_hms(2026, 11, 9, 13, 40, 0).unwrap(), &now), "1:40 pm");
        assert_eq!(reset_time(&tz.with_ymd_and_hms(2026, 11, 10, 13, 0, 0).unwrap(), &now), "tomorrow 1 pm");
        assert_eq!(reset_time(&tz.with_ymd_and_hms(2026, 11, 12, 13, 0, 0).unwrap(), &now), "Thu 1 pm");
        assert_eq!(reset_time(&tz.with_ymd_and_hms(2026, 11, 20, 13, 0, 0).unwrap(), &now), "20 Nov");
    }

    #[test]
    fn folder_names_and_home() {
        assert_eq!(short_folder("/Users/mike/dev/me/misc"), "misc");
        assert_eq!(short_folder("/"), "/");
        assert_eq!(abbreviate_home("/Users/mike/dev", "/Users/mike"), "~/dev");
        assert_eq!(abbreviate_home("/opt", "/Users/mike"), "/opt");
    }
}
