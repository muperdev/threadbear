// Other runners' threads (docs/protocol.md, "Other runners' threads"): what
// `remoteThreads.list` answers and `remoteThreads.updated` sends, a port of
// RemoteThreads.swift. Their sidebar rows come from this Mac's runner, marked
// with a `runnerId`; everything else about them goes through that runner too
// (the relay), so the rows, the transcript and the composer take them as they
// are.
use crate::inbox::{iso_time, parse_ms, ThreadSummary};
use serde_json::Value;

/// A runner the backend knows of (`RemoteRunner`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RemoteRunner {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub online: bool,
}

/// `RemoteThreads`: what the sidebar shows of the other runners.
#[derive(Clone, Debug, Default)]
pub struct RemoteThreads {
    pub runners: Vec<RemoteRunner>,
    pub threads: Vec<ThreadSummary>,
}

impl RemoteRunner {
    fn decode(v: &Value) -> Option<RemoteRunner> {
        let id = v["id"].as_str().filter(|s| !s.is_empty())?.to_owned();
        let name = v["name"].as_str().filter(|s| !s.is_empty()).unwrap_or(&id).to_owned();
        Some(RemoteRunner { name, icon: v["icon"].as_str().filter(|s| !s.is_empty()).map(str::to_owned), online: v["online"].as_bool().unwrap_or(false), id })
    }
}

/// One row of another runner's sidebar table as a summary the sidebar takes
/// like a local one (`RemoteThread.summary`): the table has no seqs, so an
/// unread one gets 1 and 0, which is all the rows look at.
fn summary(row: &Value) -> Option<ThreadSummary> {
    let runner_id = row["runnerId"].as_str().filter(|s| !s.is_empty())?.to_owned();
    let id = row["threadId"].as_str().filter(|s| !s.is_empty())?.to_owned();
    let text = |key: &str| row[key].as_str().unwrap_or("").to_owned();
    let unread = row["unread"].as_bool().unwrap_or(false);
    let created = row["createdAt"].as_str().and_then(parse_ms).unwrap_or(0);
    let last_activity_at = row["lastActivityAt"].as_str().filter(|s| parse_ms(s).is_some()).map_or_else(|| iso_time(created), str::to_owned);
    let snoozed_until = row["snoozedUntil"].as_str().filter(|s| parse_ms(s).is_some()).map(str::to_owned);
    // Hibernating until that runner restarts is kept as a date in the year 9999.
    let until_restart = snoozed_until.as_deref().and_then(parse_ms).is_some_and(|t| t > 200_000_000_000_000);
    let provider = row["provider"].as_str().filter(|s| !s.is_empty()).unwrap_or("claude").to_owned();
    let status = row["status"].as_str().filter(|s| !s.is_empty()).unwrap_or("idle").to_owned();
    Some(ThreadSummary {
        id,
        title: text("title"),
        cwd: text("cwd"),
        account_id: text("accountId"),
        provider,
        model: row["model"].as_str().filter(|s| !s.is_empty()).map(str::to_owned),
        status,
        needs_you: row["needsYou"].as_bool().unwrap_or(false),
        preview: text("preview"),
        last_activity_at,
        snoozed_until,
        snoozed_until_restart: until_restart.then_some(true),
        archived_at: row["archivedAt"].as_str().filter(|s| parse_ms(s).is_some()).map(str::to_owned),
        starred_at: row["starredAt"].as_str().filter(|s| parse_ms(s).is_some()).map(str::to_owned),
        started_by: row.get("startedBy").filter(|v| !v.is_null()).cloned(),
        asked: row["asked"].as_str().filter(|s| !s.is_empty()).map(str::to_owned),
        moving: row.get("moving").filter(|v| !v.is_null()).cloned(),
        last_seq: if unread { 1 } else { 0 },
        last_seen_seq: 0,
        runner_id: Some(runner_id),
        permission_mode: "full".to_owned(),
        ..Default::default()
    })
}

/// `remoteThreads.list`'s answer (or `remoteThreads.updated`'s params), read
/// leniently: an unreadable row is left out.
pub fn decode_remote_threads(value: &Value) -> RemoteThreads {
    RemoteThreads {
        runners: value["runners"].as_array().into_iter().flatten().filter_map(RemoteRunner::decode).collect(),
        threads: value["threads"].as_array().into_iter().flatten().filter_map(summary).collect(),
    }
}

impl RemoteThreads {
    pub fn runner(&self, id: &str) -> Option<&RemoteRunner> {
        self.runners.iter().find(|r| r.id == id)
    }
}

/// A runner from `runners.list` (`RunnerInfo`): every machine, this one too.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunnerInfo {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub online: bool,
    pub is_self: bool,
}

/// `runners.list`'s answer, this runner first and then the rest by name.
pub fn decode_runners(value: &Value) -> Vec<RunnerInfo> {
    value["runners"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some(RunnerInfo {
                id: r["id"].as_str().filter(|s| !s.is_empty())?.to_owned(),
                name: r["name"].as_str().unwrap_or("").to_owned(),
                icon: r["icon"].as_str().filter(|s| !s.is_empty()).map(str::to_owned),
                online: r["online"].as_bool().unwrap_or(false),
                is_self: r["self"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

/// One login a move could carry on with on the other runner.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoveLogin {
    pub id: String,
    pub title: String,
    pub email: Option<String>,
}

impl MoveLogin {
    /// As the sheet's picker names it (MoveSheet.body).
    pub fn label(&self) -> String {
        match &self.email {
            Some(email) => format!("{} ({email})", self.title),
            None => self.title.clone(),
        }
    }
}

/// `threads.movePreview`'s answer (`MovePreview`): the other runner's guess at
/// where a thread would go and on which login, what's in the way, and what
/// won't come with it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MovePreview {
    pub cwd: Option<String>,
    pub account_id: Option<String>,
    pub accounts: Vec<MoveLogin>,
    pub left_behind: Vec<String>,
    pub problems: Vec<String>,
    /// It gets a new worktree of its own in `cwd`'s repo.
    pub worktree: bool,
    /// That repo is cloned there first.
    pub clone: Option<String>,
    /// The thread's own runner said what it would leave behind.
    pub source_checked: bool,
}

impl MovePreview {
    pub fn decode(value: &Value) -> MovePreview {
        let text = |key: &str| value[key].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        let lines = |key: &str| value[key].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(str::to_owned)).collect::<Vec<_>>();
        MovePreview {
            cwd: text("cwd"),
            account_id: text("accountId"),
            accounts: value["accounts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| Some(MoveLogin { id: a["id"].as_str()?.to_owned(), title: a["title"].as_str().unwrap_or("").to_owned(), email: a["email"].as_str().map(str::to_owned) }))
                .collect(),
            left_behind: lines("leftBehind"),
            problems: lines("problems"),
            worktree: value["worktree"].as_bool().unwrap_or(false),
            clone: text("clone"),
            source_checked: value["sourceChecked"].as_bool().unwrap_or(false),
        }
    }

    /// Nothing to decide or be told: Move to goes without showing the sheet.
    pub fn can_go_as_is(&self) -> bool {
        self.source_checked && self.problems.is_empty() && self.left_behind.is_empty() && self.cwd.is_some() && self.account_id.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rows_become_summaries_marked_with_their_runner() {
        let list = decode_remote_threads(&json!({
            "runners": [{ "id": "r", "name": "bruce", "icon": "macmini", "online": true }, { "id": "r2" }],
            "threads": [
                { "runnerId": "r", "threadId": "t1", "title": "Backup", "cwd": "/Users/bruce/misc", "accountId": "claude-main", "provider": "claude",
                  "model": "claude-opus-5-5", "status": "idle", "needsYou": true, "unread": true, "preview": "done", "createdAt": "2026-11-09T01:00:00.000Z", "lastActivityAt": "2026-11-09T02:00:00.000Z" },
                { "runnerId": "r", "threadId": "t2", "status": "running", "createdAt": "2026-11-09T01:00:00.000Z" },
                { "threadId": "no-runner" },
            ],
            "order": ["runner_mock", "r"],
        }));
        assert_eq!(list.runners.len(), 2);
        assert_eq!(list.runner("r").unwrap().name, "bruce");
        assert_eq!(list.runner("r2").unwrap().name, "r2");
        assert_eq!(list.threads.len(), 2);
        let t = &list.threads[0];
        assert_eq!(t.id, "t1");
        assert_eq!(t.runner_id.as_deref(), Some("r"));
        assert!(t.needs_you);
        assert!(t.unread());
        assert_eq!(t.last_activity_at, "2026-11-09T02:00:00.000Z");
        // No activity time falls back to its creation.
        assert_eq!(list.threads[1].last_activity_at, "2026-11-09T01:00:00.000Z");
        assert!(list.threads[1].busy());
    }

    #[test]
    fn a_preview_with_nothing_to_say_can_just_go() {
        let answer = json!({ "cwd": "/Users/bruce/x", "accountId": "claude-main", "accounts": [{ "id": "claude-main", "title": "Claude", "email": "a@b" }], "leftBehind": [], "problems": [], "sourceChecked": true });
        assert!(MovePreview::decode(&answer).can_go_as_is());
        let left = json!({ "cwd": "/x", "accountId": "a", "leftBehind": ["a job"], "sourceChecked": true });
        assert!(!MovePreview::decode(&left).can_go_as_is());
        let unchecked = json!({ "cwd": "/x", "accountId": "a", "sourceChecked": false });
        assert!(!MovePreview::decode(&unchecked).can_go_as_is());
        assert_eq!(MovePreview::decode(&answer).accounts[0].label(), "Claude (a@b)");
    }

    #[test]
    fn runners_list_decodes_itself_and_the_others() {
        let list = decode_runners(&json!({ "runners": [{ "id": "m", "name": "m5", "icon": "laptopcomputer", "online": true, "self": true }, { "id": "b", "name": "bruce", "online": false }] }));
        assert_eq!(list.len(), 2);
        assert!(list[0].is_self);
        assert!(!list[1].online);
    }

    #[test]
    fn a_far_off_snooze_means_until_that_runner_restarts() {
        let list = decode_remote_threads(&json!({ "runners": [], "threads": [
            { "runnerId": "r", "threadId": "t", "createdAt": "2026-11-09T01:00:00.000Z", "snoozedUntil": "9999-12-31T00:00:00.000Z" }
        ]}));
        assert_eq!(list.threads[0].snoozed_until_restart, Some(true));
    }
}
