// Other runners' threads (docs/protocol.md, "Other runners' threads" and "...
// through the backend"), a port of the parts of RemoteRunners.swift the app
// needs while it reaches them through this Mac's runner and Convex (the
// only route since the direct link was removed on 13 Nov 2026). Their rows
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
    /// Threads made or moved here whose destination has not listed them
    /// yet. Keep their destination until its first authoritative row arrives.
    pub(crate) made: HashSet<String>,
    /// Logins being fetched, so one's asked for once.
    pub(crate) fetching: HashSet<String>,
    /// When each runner's logins last came: they carry how much room each
    /// has (backend API 77), which the limit box goes by, so they're asked
    /// for again on opening one of its threads once they're a minute old.
    pub(crate) logins_at: HashMap<String, std::time::Instant>,
    /// Asks again each minute for the open thread's runner's logins while
    /// it's another runner's, so its limit box follows that login's room as
    /// it runs out or resets (Codex's review, 13 Nov: left open, it said
    /// "has reached its limit" long after the limit had reset).
    pub(crate) logins_refresh: Option<Task<()>>,
    /// `runners.list`: every runner, this Mac's too.
    pub(crate) all_runners: Vec<RunnerInfo>,
    /// `hello.experiments`: what's on for this runner.
    pub(crate) experiments: Vec<String>,
    /// The runner has said which other Macs' threads there are (or that it
    /// can't), so a thread not among them isn't coming.
    pub(crate) heard: bool,
}

impl Threadbear {
    /// A thread's old runner let it go during a move: keep its row as that
    /// runner's, with its move and so its locked box, until the destination
    /// lists it (`remote::moved_away`, Daybreak 113). False when it isn't a
    /// move, and the row should go.
    pub(crate) fn moved_away(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(kept) = self.threads.get(id).and_then(|t| remote::moved_away(t)) else { return false };
        let self_id = self.remote.all_runners.iter().find(|r| r.is_self).map(|r| r.id.clone());
        let here = remote::moving_to(&kept).is_some_and(|to| Some(to) == self_id.as_deref());
        self.remote.made.insert(id.to_owned());
        self.kept.forget(id, cx);
        let panes: Vec<_> = self.pane.iter().chain(self.subagent_pane.iter()).chain(self.thread_panels.panes()).cloned().collect();
        for pane in panes {
            if pane.read(cx).summary.id == id {
                pane.update(cx, |p, cx| {
                    p.unsubscribe();
                    p.summary_changed(&kept, cx);
                    p.connection_back();
                });
            }
        }
        self.threads.insert(id.to_owned(), std::sync::Arc::new(kept));
        // Only this Mac's own list says a move here has arrived (load_list
        // takes it from `made` then); one to another runner arrives with
        // that runner's index row (apply_remote_index).
        if here { self.load_list(cx); }
        self.refresh_dock_badge();
        true
    }

    /// The list after hello; `remoteThreads.updated` replaces it from then
    /// on (`loadRemoteThreads`). A runner from before them answers -32601,
    /// which is none: the sidebar has only this Mac's.
    pub(crate) fn load_remote(&mut self, cx: &mut Context<Self>) {
        if self.remote.logins_refresh.is_none() {
            self.remote.logins_refresh = Some(cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(std::time::Duration::from_secs(60)).await;
                // Every thread showing, its own panels too, not only the
                // sidebar's selection (Codex's second review, 13 Nov).
                let gone = this
                    .update(cx, |t, cx| {
                        let panes: Vec<_> = t.pane.iter().chain(t.thread_panels.panes()).cloned().collect();
                        let runners: HashSet<String> = panes.iter().filter_map(|p| p.read(cx).summary.runner_id.clone()).collect();
                        for runner in runners {
                            t.load_remote_logins(runner, cx);
                        }
                    })
                    .is_err();
                if gone {
                    break;
                }
            }));
        }
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let res = call(client.clone(), "remoteThreads.list", json!({})).await;
            match res {
                Ok(value) => {
                    this.update(cx, |this, cx| this.receive_remote(&value, cx)).ok();
                }
                // None to come (a runner from before them).
                Err(_) => {
                    this.update(cx, |this, cx| {
                        this.remote.heard = true;
                        this.try_waiting_link(cx);
                    })
                    .ok();
                }
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
        if value["ready"].as_bool() == Some(false) {
            self.remote.heard = false;
            cx.notify();
            return;
        }
        let remote = remote::decode_remote_threads(value);
        self.remote.runners = remote.runners.clone();
        self.remote.index = remote;
        self.remote.heard = true;
        // Mike's runner order, for the Runners panel.
        self.receive_runner_order();
        self.apply_remote_index(cx);
        self.consider_milestone(cx);
        self.apply_default_runner(cx);
        // The Dock badge counts other runners' threads too, as Swift's does.
        self.refresh_dock_badge();
        // A link to one of their threads, opened before they'd come.
        self.try_waiting_link(cx);
        cx.notify();
    }

    /// `applyRemoteIndex`: puts the table's rows in `threads` and lets go of
    /// rows that have gone from it (deleted there).
    pub(crate) fn apply_remote_index(&mut self, cx: &mut Context<Self>) {
        if !self.remote.heard { return; }
        let mut listed = HashSet::new();
        let mut reload = HashSet::new();
        for summary in self.remote.index.threads.clone() {
            // Never in place of one of this Mac's own.
            let waiting = self.remote.made.contains(&summary.id);
            if let Some(current) = self.threads.get(&summary.id) {
                if current.runner_id.is_none() && !waiting { continue; }
                // While it waits on its move only the runner it's going to
                // and the one it's leaving speak for it: an old Mac's
                // lingering row can't take it, and the one it's leaving
                // putting its row back keeps the thread that runner's,
                // still locked with its move (Daybreak 113), rather than
                // being ignored for a destination that never listed it.
                if waiting && summary.runner_id != current.runner_id && summary.runner_id.as_deref() != remote::moving_to(current) { continue; }
            }
            listed.insert(summary.id.clone());
            let waiting = self.remote.made.remove(&summary.id);
            let before = self.threads.insert(summary.id.clone(), std::sync::Arc::new(summary.clone()));
            if waiting || before.as_ref().is_some_and(|old| old.runner_id != summary.runner_id) {
                reload.insert(summary.id.clone());
            }
            self.note_failed_account(before.as_deref(), &summary);
        }
        let gone: Vec<String> = self.threads.values().map(|t| &**t).filter(|t| self.remote.index.can_remove_missing() && t.runner_id.is_some() && !listed.contains(&t.id) && !self.remote.made.contains(&t.id)).map(|t| t.id.clone()).collect();
        for id in gone {
            if !self.moved_away(&id, cx) {
                self.threads.remove(&id);
                self.failed_on_account.remove(&id);
            }
        }
        // Every visible or kept pane follows ownership. Re-read only once
        // the destination lists it: before then the relay cannot route it.
        self.sync_thread_panes(&reload, cx);
        // A kept thread that's no longer listed (deleted, here or on its
        // runner, while the connection was down) goes from the kept ones.
        let listed: HashSet<String> = self.threads.keys().cloned().collect();
        self.kept.retain(cx, |id| listed.contains(id));
        // Their titles' fades too (Daybreak 117).
        crate::open_motion::forget_titles(|id| listed.contains(id));
    }

    pub(crate) fn sync_thread_panes(&mut self, reload: &HashSet<String>, cx: &mut Context<Self>) {
        let panes: Vec<_> = self.pane.iter().chain(self.subagent_pane.iter()).chain(self.thread_panels.panes()).chain(self.kept.panes()).cloned().collect();
        let mut seen = HashSet::new();
        for pane in panes {
            if !seen.insert(pane.entity_id()) { continue; }
            let Some(t) = self.threads.get(&pane.read(cx).summary.id).cloned() else { continue };
            pane.update(cx, |p, cx| {
                p.summary_changed(&t, cx);
                if reload.contains(&t.id) {
                    p.unsubscribe();
                    p.load(cx);
                }
            });
        }
    }

    /// `loadRelayLogins`: that runner's logins for the pickers, from the
    /// backend through this Mac's runner, once per runner.
    pub(crate) fn load_remote_logins(&mut self, runner: String, cx: &mut Context<Self>) {
        let fresh = self.remote.logins_at.get(&runner).is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(60));
        if (self.remote.logins.contains_key(&runner) && fresh) || !self.remote.fetching.insert(runner.clone()) {
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
                        this.remote.logins_at.insert(runner.clone(), std::time::Instant::now());
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
