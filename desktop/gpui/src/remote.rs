// Other runners' threads (docs/protocol.md, "Other runners' threads" and "...
// through the backend"), a port of the parts of RemoteRunners.swift the app
// needs while it reaches them through this Mac's runner and Convex (the
// default; the direct link's own connections aren't ported yet). Their rows
// come from this Mac's runner, marked with `runnerId`, and sit in
// `Threadbear::threads` beside this Mac's own, so the sidebar and the
// transcript take them as they are: every request about one goes to this
// Mac's runner, which answers from the backend.
use threadbear_core::inbox::ThreadSummary;
use crate::app::Threadbear;
use crate::call;
use gpui_kit::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use threadbear_core::logins::RunnerLogins;
use threadbear_core::remote::{self, RemoteRunner, RemoteThreads, RunnerInfo};

/// What the app knows of the other runners; the app holds one of these.
#[derive(Default)]
pub(crate) struct RemoteState {
    pub(crate) runners: Vec<RemoteRunner>,
    /// The rows as last heard (`remoteIndex`).
    pub(crate) index: RemoteThreads,
    /// Each runner's logins, once `remoteThreads.logins` has answered:
    /// nil until then, as for `RemoteRunners.logins(for:)`.
    pub(crate) logins: HashMap<String, RunnerLogins>,
    /// Threads made here on another runner that the sidebar's table hasn't
    /// listed yet (`madeRemote`): they stay, as its, until it does.
    pub(crate) made: HashSet<String>,
    /// Logins being fetched, so one's asked for once.
    pub(crate) fetching: HashSet<String>,
    /// `runners.list`: every runner, this Mac's too.
    pub(crate) all_runners: Vec<RunnerInfo>,
    /// `hello.experiments`: what's on for this runner (the Move to menu is
    /// `moveToRunner`'s).
    pub(crate) experiments: Vec<String>,
}

impl Threadbear {
    /// The list after hello; `remoteThreads.updated` replaces it from then
    /// on (`loadRemoteThreads`). A runner from before them answers -32601,
    /// which is none: the sidebar has only this Mac's.
    pub(crate) fn load_remote(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client.clone(), "remoteThreads.list", json!({})).await;
            if let Ok(value) = res {
                this.update(cx, |this, cx| this.receive_remote(&value, cx)).ok();
            }
            // Every runner, this Mac's too: its icon and where a move can go.
            if let Ok(value) = call(client, "runners.list", json!({})).await {
                this.update(cx, |this, cx| {
                    this.remote.all_runners = remote::decode_runners(&value);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// `selfRunnerIcon`: this Mac's icon, as a symbol this app has.
    pub(crate) fn self_runner_symbol(&self) -> &'static str {
        crate::icons::runner_symbol(self.remote.all_runners.iter().find(|r| r.is_self).and_then(|r| r.icon.as_deref()))
    }

    /// `receive(remote:)`: the whole list, as `remoteThreads.list` and
    /// `remoteThreads.updated` give it.
    pub(crate) fn receive_remote(&mut self, value: &Value, cx: &mut Context<Self>) {
        let remote = remote::decode_remote_threads(value);
        self.remote.runners = remote.runners.clone();
        self.remote.index = remote;
        self.apply_remote_index(cx);
        self.apply_default_runner(cx);
        cx.notify();
    }

    /// `applyRemoteIndex`: puts the table's rows in `threads` and lets go of
    /// rows that have gone from it (deleted there).
    pub(crate) fn apply_remote_index(&mut self, cx: &mut Context<Self>) {
        let mut listed = HashSet::new();
        for summary in self.remote.index.threads.clone() {
            // Never in place of one of this Mac's own.
            if self.threads.get(&summary.id).is_some_and(|t| t.runner_id.is_none()) {
                continue;
            }
            listed.insert(summary.id.clone());
            self.remote.made.remove(&summary.id);
            let before = self.threads.insert(summary.id.clone(), summary.clone());
            self.note_failed_account(before.as_ref(), &summary);
        }
        let gone: Vec<String> = self.threads.values().filter(|t| t.runner_id.is_some() && !listed.contains(&t.id) && !self.remote.made.contains(&t.id)).map(|t| t.id.clone()).collect();
        for id in gone {
            self.threads.remove(&id);
            self.failed_on_account.remove(&id);
        }
        // An open remote thread keeps up with its row.
        if let Some(id) = self.selected.clone() {
            if let (Some(pane), Some(t)) = (self.pane.clone(), self.threads.get(&id).cloned()) {
                if t.runner_id.is_some() {
                    pane.update(cx, |p, cx| p.summary_changed(&t, cx));
                }
            }
        }
    }

    /// `loadRelayLogins`: that runner's logins for the pickers, from the
    /// backend through this Mac's runner, once per runner.
    pub(crate) fn load_remote_logins(&mut self, runner: String, cx: &mut Context<Self>) {
        if self.remote.logins.contains_key(&runner) || !self.remote.fetching.insert(runner.clone()) {
            return;
        }
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client, "remoteThreads.logins", json!({ "runnerId": runner })).await;
            this.update(cx, |this, cx| {
                this.remote.fetching.remove(&runner);
                match res {
                    Ok(value) => {
                        this.remote.logins.insert(runner.clone(), RunnerLogins::from_hello(&value));
                        if this.new_thread.picks.runner.as_deref() == Some(runner.as_str()) {
                            this.new_thread.runner_problem = None;
                        }
                    }
                    Err(e) => {
                        if this.new_thread.picks.runner.as_deref() == Some(runner.as_str()) {
                            this.new_thread.runner_problem = Some(e.message);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `runnerName(of:)`: "bruce" for another runner's thread.
    pub(crate) fn runner_name_of(&self, runner_id: &str) -> String {
        self.remote.runners.iter().find(|r| r.id == runner_id).map_or_else(|| "another Mac".to_owned(), |r| r.name.clone())
    }

    /// `runnerLabel(of:)`: the runner's name on a sidebar row ("m5",
    /// "bruce"), this Mac's own threads included: Mike moves threads between
    /// machines and asked to see at a glance where each one is (9 Nov). None
    /// with only this Mac, where every row would just repeat the same name.
    pub(crate) fn runner_label(&self, t: &ThreadSummary) -> Option<String> {
        if let Some(id) = t.runner_id.as_deref() {
            return Some(self.runner_name_of(id));
        }
        if self.remote.runners.is_empty() {
            return None;
        }
        self.remote.all_runners.iter().find(|r| r.is_self).map(|r| r.name.clone())
    }

    /// `runnerPlace(of:)`: "On bruce", or "On bruce, offline".
    pub(crate) fn runner_place_of(&self, runner_id: &str) -> String {
        let name = self.runner_name_of(runner_id);
        if self.remote.runners.iter().any(|r| r.id == runner_id && !r.online) {
            format!("On {name}, offline")
        } else {
            format!("On {name}")
        }
    }

    /// `runnerIcon(of:)`: the symbol for a thread's runner.
    pub(crate) fn runner_icon_symbol(&self, runner_id: Option<&str>) -> &'static str {
        crate::icons::runner_symbol(runner_id.and_then(|id| self.remote.runners.iter().find(|r| r.id == id)).and_then(|r| r.icon.as_deref()))
    }
}
