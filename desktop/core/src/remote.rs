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
    /// Where this Mac keeps that runner's token, once one's been copied in
    /// (`tb pair`): what a direct connection to it needs (Settings › Runners).
    pub token_file: Option<String>,
}

/// `RemoteThreads`: what the sidebar shows of the other runners.
#[derive(Clone, Debug, Default)]
pub struct RemoteThreads {
    pub runners: Vec<RemoteRunner>,
    pub threads: Vec<ThreadSummary>,
    /// Mike's runner order (backend API 58), every runner's id: what the
    /// phone and the Runners panel list them in. None from a backend without one.
    pub order: Option<Vec<String>>,
}

impl RemoteRunner {
    fn decode(v: &Value) -> Option<RemoteRunner> {
        let id = v["id"].as_str().filter(|s| !s.is_empty())?.to_owned();
        let name = v["name"].as_str().filter(|s| !s.is_empty()).unwrap_or(&id).to_owned();
        let token_file = v["tokenFile"].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        Some(RemoteRunner { name, icon: v["icon"].as_str().filter(|s| !s.is_empty()).map(str::to_owned), online: v["online"].as_bool().unwrap_or(false), token_file, id })
    }
}

/// One row of another runner's sidebar table as a summary the sidebar takes
/// like a local one (`RemoteThread.summary`): the table has no seqs, so an
/// unread one gets 1 and 0, which is all the rows look at.
fn summary(row: &Value) -> Option<ThreadSummary> {
    let runner_id = row["runnerId"].as_str().filter(|s| !s.is_empty())?.to_owned();
    // The backend takes any string a runner writes, and an extra Mac may be
    // compromised: a row whose id isn't a thread id's shape is left out, as
    // the id goes into shell lines and links (thread_link::is_thread_id).
    let id = row["threadId"].as_str().filter(|s| crate::thread_link::is_thread_id(s))?.to_owned();
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
        // The done card's loose ends, which this Mac's runner adds to the row
        // of a thread it's watching (relay.ts `overlay`): the sidebar's own
        // rows don't carry them, and this summary replaces the last one.
        loose_ends: row.get("looseEnds").filter(|v| v.is_object()).cloned(),
        ..Default::default()
    })
}

/// `remoteThreads.list`'s answer (or `remoteThreads.updated`'s params), read
/// leniently: an unreadable row is left out.
pub fn decode_remote_threads(value: &Value) -> RemoteThreads {
    RemoteThreads {
        runners: value["runners"].as_array().into_iter().flatten().filter_map(RemoteRunner::decode).collect(),
        threads: value["threads"].as_array().into_iter().flatten().filter_map(summary).collect(),
        order: value["order"].as_array().map(|ids| ids.iter().filter_map(|id| id.as_str().map(str::to_owned)).collect()),
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
    pub last_seen_ms: Option<i64>,
    /// What its last heartbeat said, for the Runners panel (runners.rs).
    /// None from a runner before they were passed on (5 Nov).
    pub version: Option<String>,
    pub release: Option<String>,
    pub started_ms: Option<i64>,
    /// An old release finishing its threads in a handover.
    pub draining: bool,
}

/// `runners.list`'s answer, this runner first and then the rest by name.
pub fn decode_runners(value: &Value) -> Vec<RunnerInfo> {
    value["runners"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let id = r["id"].as_str().filter(|s| !s.is_empty())?.to_owned();
            let text = |key: &str| r[key].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
            let is_self = r["self"].as_bool().unwrap_or(false);
            Some(RunnerInfo {
                name: text("name").unwrap_or_else(|| id.clone()),
                icon: text("icon"),
                // The answering runner is plainly up; anything else unsaid is offline.
                online: r["online"].as_bool().unwrap_or(is_self),
                is_self,
                last_seen_ms: r["lastSeenAt"].as_str().and_then(parse_ms),
                version: text("version"),
                release: text("release"),
                started_ms: r["startedAt"].as_str().and_then(parse_ms),
                draining: r["draining"].as_bool().unwrap_or(false),
                id,
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

    // The relay puts a watched thread's done-card loose ends on its row
    // (relay.ts `overlay`): they reach the done card, for the seq it's on.
    #[test]
    fn a_rows_loose_ends_reach_the_summary() {
        let list = decode_remote_threads(&json!({
            "runners": [{ "id": "r", "name": "bruce" }],
            "threads": [
                { "runnerId": "r", "threadId": "t1", "status": "idle", "createdAt": "2026-11-09T01:00:00.000Z",
                  "looseEnds": { "seq": 47, "ends": [{ "kind": "scheduled", "text": "Look at how the experiment went", "at": "2026-11-15T01:30:00.000Z" }] } },
                { "runnerId": "r", "threadId": "t2", "status": "idle", "createdAt": "2026-11-09T01:00:00.000Z", "looseEnds": "junk" },
            ],
        }));
        let check = list.threads[0].loose_ends_check().expect("its loose ends");
        assert_eq!(check.seq, 47);
        assert_eq!(check.ends[0].text, "Look at how the experiment went");
        assert!(list.threads[1].loose_ends.is_none(), "one that isn't an object is left off");
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
        let list = decode_runners(&json!({ "runners": [
            { "id": "m", "name": "m5", "icon": "laptopcomputer", "self": true, "version": "0.1.0+20261105T064853Z-c4d90ca", "release": "20261105T064853Z-c4d90ca", "startedAt": "2026-11-11T01:00:00.000Z" },
            { "id": "b", "name": "bruce", "online": false, "lastSeenAt": "2026-11-09T01:00:00.000Z", "draining": true },
            { "id": "c", "name": "" },
        ] }));
        assert_eq!(list.len(), 3);
        assert!(list[0].is_self);
        assert!(list[0].online, "the answering runner is up even if it doesn't say");
        assert_eq!(list[0].release.as_deref(), Some("20261105T064853Z-c4d90ca"));
        assert_eq!(list[0].started_ms, parse_ms("2026-11-11T01:00:00.000Z"));
        assert!(!list[1].online);
        assert!(list[1].draining);
        assert_eq!(list[1].last_seen_ms, parse_ms("2026-11-09T01:00:00.000Z"));
        assert_eq!(list[2].name, "c", "no name is its id");
        assert!(!list[2].online);
    }

    #[test]
    fn the_runner_order_comes_with_the_other_runners() {
        assert_eq!(decode_remote_threads(&json!({ "runners": [], "threads": [], "order": ["m", "b", 3] })).order, Some(vec!["m".to_string(), "b".to_string()]));
        assert_eq!(decode_remote_threads(&json!({ "runners": [], "threads": [] })).order, None);
    }

    #[test]
    fn a_far_off_snooze_means_until_that_runner_restarts() {
        let list = decode_remote_threads(&json!({ "runners": [], "threads": [
            { "runnerId": "r", "threadId": "t", "createdAt": "2026-11-09T01:00:00.000Z", "snoozedUntil": "9999-12-31T00:00:00.000Z" }
        ]}));
        assert_eq!(list.threads[0].snoozed_until_restart, Some(true));
    }
}
