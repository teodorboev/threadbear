// The Librarian view's model and words, ported from the Swift app: the
// runner's background job that files threads in projects, keeps their notes
// and renames threads that have wandered off (docs/protocol.md, "Projects").
// LibrarianStatus is Project.swift's, the words ProjectSections.swift's
// LibrarianText, the page's tiles LibrarianHome.swift's LibrarianStats.
use crate::home::HomeStat;
use crate::inbox::{parse_ms, ThreadSummary};
use serde_json::Value;
use std::collections::HashMap;

/// What it's doing (`LibrarianStatus.State`); an unknown state reads as idle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Idle,
    Working,
    Paused,
    Off,
}

/// Why a look was queued (`LibrarianReason`); an unknown one reads as asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reason {
    FirstMessage,
    FirstTurn,
    NewFolder,
    Moved,
    CatchUp,
    #[default]
    Asked,
    /// A project's notes got long and were tidied: the entry is the
    /// project's, with no thread.
    Notes,
}

impl Reason {
    fn from_raw(raw: &str) -> Reason {
        match raw {
            "first-message" => Reason::FirstMessage,
            "first-turn" => Reason::FirstTurn,
            "new-folder" => Reason::NewFolder,
            "moved" => Reason::Moved,
            "catch-up" => Reason::CatchUp,
            "notes" => Reason::Notes,
            _ => Reason::Asked,
        }
    }
}

/// What a look did (`LibrarianEntry.Outcome`); `Other` is from a newer runner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Joined,
    Created,
    Updated,
    Left,
    Nothing,
    Skipped,
    Failed,
    Other,
}

impl Outcome {
    fn from_raw(raw: &str) -> Outcome {
        match raw {
            "joined" => Outcome::Joined,
            "created" => Outcome::Created,
            "updated" => Outcome::Updated,
            "left" => Outcome::Left,
            "nothing" => Outcome::Nothing,
            "skipped" => Outcome::Skipped,
            "failed" => Outcome::Failed,
            _ => Outcome::Other,
        }
    }
}

/// A look it will take, or is taking (`LibrarianJob`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Job {
    pub thread_id: String,
    pub thread_title: String,
    pub reason: Reason,
    /// The reason in words: "Edited files in ~/dev/me/poddle".
    pub why: String,
    pub due_ms: Option<i64>,
    /// Only on the current one: when the look began.
    pub started_ms: Option<i64>,
}

/// Something it did (`LibrarianEntry`).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub at_ms: Option<i64>,
    pub thread_id: String,
    pub thread_title: String,
    pub reason: Reason,
    pub outcome: Outcome,
    /// "Put it in Threadbear", "Made Poddle and put it there".
    pub text: String,
    pub project_ids: Vec<String>,
    /// Why it joined each project, by project id.
    pub reasons: HashMap<String, String>,
    pub cost_usd: Option<f64>,
}

/// What the librarian is doing, has done and will do next (`LibrarianStatus`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub state: State,
    /// Off: why.
    pub reason: Option<String>,
    pub paused: bool,
    pub current: Option<Job>,
    pub queue: Vec<Job>,
    pub recent: Vec<Entry>,
    pub looks_today: i64,
    /// Catch-ups skipped today as too quiet for a model call.
    pub skipped_today: i64,
    /// Tokens in and out today.
    pub tokens_today: i64,
}

fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_owned()
}

fn int(v: &Value, key: &str) -> i64 {
    v[key].as_i64().or_else(|| v[key].as_f64().map(|f| f as i64)).unwrap_or(0)
}

fn job(v: &Value) -> Option<Job> {
    Some(Job {
        thread_id: v["threadId"].as_str()?.to_owned(),
        thread_title: text(v, "threadTitle"),
        reason: Reason::from_raw(v["reason"].as_str().unwrap_or("")),
        why: text(v, "why"),
        due_ms: v["dueAt"].as_str().and_then(parse_ms),
        started_ms: v["startedAt"].as_str().and_then(parse_ms),
    })
}

fn entry(v: &Value) -> Option<Entry> {
    Some(Entry {
        id: v["id"].as_str()?.to_owned(),
        at_ms: v["at"].as_str().and_then(parse_ms),
        thread_id: text(v, "threadId"),
        thread_title: text(v, "threadTitle"),
        reason: Reason::from_raw(v["reason"].as_str().unwrap_or("")),
        outcome: Outcome::from_raw(v["outcome"].as_str().unwrap_or("")),
        text: text(v, "text"),
        project_ids: v["projectIds"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_owned)).collect(),
        reasons: v["reasons"].as_object().map(|m| m.iter().filter_map(|(k, why)| Some((k.clone(), why.as_str()?.to_owned()))).collect()).unwrap_or_default(),
        cost_usd: v["costUsd"].as_f64(),
    })
}

impl Status {
    /// A `LibrarianStatus` as the runner sends it, taking what it can: a
    /// missing or odd field reads as nothing, as the Swift decoder has it.
    pub fn decode(v: &Value) -> Status {
        let paused = v["paused"].as_bool().unwrap_or(false);
        let state = match v["state"].as_str() {
            Some("working") => State::Working,
            Some("paused") => State::Paused,
            Some("off") => State::Off,
            Some(_) => State::Idle,
            None if paused => State::Paused,
            None => State::Idle,
        };
        Status {
            state,
            reason: v["reason"].as_str().map(str::to_owned),
            paused,
            current: job(&v["current"]),
            queue: v["queue"].as_array().into_iter().flatten().filter_map(job).collect(),
            recent: v["recent"].as_array().into_iter().flatten().filter_map(entry).collect(),
            looks_today: int(v, "looksToday"),
            skipped_today: int(v, "skippedToday"),
            tokens_today: int(v, "tokensToday"),
        }
    }

    /// The status as it will be once a pause or resume has landed, so the
    /// button and switch change at once. One that's off stays off: pausing
    /// doesn't change why it can't run.
    pub fn setting(&self, paused: bool) -> Status {
        let mut status = self.clone();
        status.paused = paused;
        if self.state != State::Off {
            status.state = if paused {
                State::Paused
            } else if self.current.is_none() {
                State::Idle
            } else {
                State::Working
            };
        }
        status
    }

    /// The activity bar's badge: the look under way and the ones waiting.
    pub fn pending_count(&self) -> usize {
        self.queue.len() + usize::from(self.current.is_some())
    }
}

/// "Idle", "Looking at “Rename the Settings tabs”", "Paused", "Off: Needs a
/// Claude pool to run on" (`LibrarianText.state`).
pub fn state_text(status: &Status) -> String {
    match status.state {
        State::Idle if status.paused => "Paused".into(),
        State::Idle => "Idle".into(),
        State::Working => match &status.current {
            Some(current) if !current.thread_title.is_empty() => format!("Looking at \u{201C}{}\u{201D}", current.thread_title),
            Some(_) => "Looking at a thread".into(),
            None => "Working".into(),
        },
        State::Paused => "Paused".into(),
        State::Off => match status.reason.as_deref().filter(|r| !r.is_empty()) {
            Some(reason) => format!("Off: {reason}"),
            None => "Off".into(),
        },
    }
}

/// A queued look's reason when the runner gave no words for it.
pub fn reason_text(reason: Reason) -> &'static str {
    match reason {
        Reason::FirstMessage => "Its first message came in",
        Reason::FirstTurn => "Its first turn ended",
        Reason::NewFolder => "Edited files in a new folder",
        Reason::Moved => "Moved to another folder",
        Reason::CatchUp => "More turns since the last look",
        Reason::Asked => "Asked for",
        Reason::Notes => "Its notes got long",
    }
}

pub fn why(job: &Job) -> String {
    if job.why.is_empty() {
        reason_text(job.reason).to_owned()
    } else {
        job.why.clone()
    }
}

/// "14 looks today · 3 skipped · 52k tokens". No money: looks run on a
/// Claude login, which uses its room, and what they'd have cost on the API
/// read as a bill Mike wasn't paying.
pub fn today(status: &Status) -> String {
    let mut parts = vec![if status.looks_today == 1 { "1 look today".to_owned() } else { format!("{} looks today", status.looks_today) }];
    if status.skipped_today > 0 {
        parts.push(format!("{} skipped", status.skipped_today));
    }
    if status.tokens_today > 0 {
        parts.push(format!("{} tokens", tokens(status.tokens_today)));
    }
    parts.join(" · ")
}

/// 820, 52k, 1.2M.
pub fn tokens(n: i64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{}k", (n as f64 / 1000.).round() as i64)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.)
    }
}

/// "in 4m", "now", or "waiting" for one more than a minute overdue: looks
/// wait while it's paused, off or busy with another.
pub fn due(due_ms: Option<i64>, now_ms: i64) -> String {
    let Some(at) = due_ms else { return String::new() };
    if at - now_ms > -60_000 {
        crate::highlights::schedule_relative(at, now_ms)
    } else {
        "waiting".into()
    }
}

/// A line for each project the look filed the thread in, in the row's order:
/// "Walkies: Plans the dog walk route". Nothing on other looks.
pub fn filing_reasons(entry: &Entry, title: impl Fn(&str) -> Option<String>) -> Vec<String> {
    entry
        .project_ids
        .iter()
        .filter_map(|id| {
            let why = entry.reasons.get(id).filter(|w| !w.is_empty())?;
            Some(title(id).map_or_else(|| why.clone(), |t| format!("{t}: {why}")))
        })
        .collect()
}

/// What a look did, when the runner sent no words of its own (`outcomeWords`).
pub fn outcome_words(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Joined => "Put it in a project",
        Outcome::Created => "Made a project",
        Outcome::Updated => "Updated a project",
        Outcome::Left => "Took it out of a project",
        Outcome::Nothing => "Left it as it was",
        Outcome::Skipped => "Skipped it",
        Outcome::Failed => "Couldn't look",
        Outcome::Other => "Looked",
    }
}

/// What prompted a look, and what it cost: the end of an entry's tooltip.
pub fn entry_help(entry: &Entry) -> String {
    let mut parts = vec![reason_text(entry.reason).to_owned()];
    if let Some(cost) = crate::reducer::format::cost_text(entry.cost_usd) {
        parts.push(cost);
    }
    parts.join(" · ")
}

/// The page's tiles, from the status and what the app knows of projects and
/// threads (`LibrarianStats.tiles`). `projects_made` is the projects the
/// librarian started that aren't archived.
pub fn stats(status: &Status, projects_made: usize, threads: &[&ThreadSummary]) -> Vec<HomeStat> {
    let filed = threads.iter().filter(|t| t.archived_at.is_none() && t.projects.iter().flatten().any(|p| p.by == "librarian")).count();
    vec![
        HomeStat::new(status.looks_today, "Looks today", "Threads it has looked at since midnight"),
        HomeStat::new(status.skipped_today, "Skipped today", "Catch-ups after only a quick reply, skipped without asking the model"),
        HomeStat::new(tokens(status.tokens_today), "Tokens today", "What today's looks sent and got back"),
        HomeStat::new(projects_made, if projects_made == 1 { "Project it made" } else { "Projects it made" }, "Projects the librarian started that aren't archived"),
        HomeStat::new(filed, if filed == 1 { "Thread it filed" } else { "Threads it filed" }, "Open threads the librarian put in a project"),
        HomeStat::new(status.queue.len(), "Waiting", "Threads queued for a look, listed in the sidebar"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_status_reads_leniently_and_says_what_its_doing() {
        let status = Status::decode(&json!({
            "state": "working", "paused": false,
            "current": { "threadId": "t1", "threadTitle": "Rename the Settings tabs", "reason": "first-turn", "startedAt": "2026-11-11T08:00:00Z" },
            "queue": [{ "threadId": "t2", "reason": "catch-up" }, { "reason": "asked" }],
            "recent": [{ "id": "e1", "threadId": "t1", "outcome": "joined", "projectIds": ["p1"], "reasons": { "p1": "Plans the release" } }, { "id": "e2", "outcome": "brand-new" }],
            "looksToday": 14, "skippedToday": 3, "tokensToday": 52_400,
        }));
        assert_eq!(state_text(&status), "Looking at \u{201C}Rename the Settings tabs\u{201D}");
        assert_eq!(status.queue.len(), 1, "a job with no thread is left out");
        assert_eq!(status.pending_count(), 2);
        assert_eq!(why(&status.queue[0]), "More turns since the last look");
        assert_eq!(status.recent[1].outcome, Outcome::Other);
        assert_eq!(today(&status), "14 looks today · 3 skipped · 52k tokens");
        assert_eq!(filing_reasons(&status.recent[0], |_| Some("Threadbear".into())), vec!["Threadbear: Plans the release"]);
        let off = Status::decode(&json!({ "state": "off", "reason": "Needs a Claude pool to run on" }));
        assert_eq!(state_text(&off), "Off: Needs a Claude pool to run on");
        assert_eq!(state_text(&Status::decode(&json!({ "paused": true }))), "Paused");
    }

    #[test]
    fn pausing_shows_at_once_but_off_stays_off() {
        let working = Status::decode(&json!({ "state": "working", "current": { "threadId": "t1" } }));
        assert_eq!(working.setting(true).state, State::Paused);
        assert_eq!(working.setting(true).setting(false).state, State::Working);
        let off = Status::decode(&json!({ "state": "off" }));
        assert_eq!(off.setting(false).state, State::Off);
    }

    #[test]
    fn tokens_and_when_a_look_is_due() {
        assert_eq!(tokens(820), "820");
        assert_eq!(tokens(52_400), "52k");
        assert_eq!(tokens(1_234_567), "1.2M");
        let now = 1_000_000_000_000;
        assert_eq!(due(Some(now + 4 * 60_000), now), "in 4m");
        assert_eq!(due(Some(now - 30_000), now), "now");
        assert_eq!(due(Some(now - 5 * 60_000), now), "waiting");
        assert_eq!(due(None, now), "");
    }
}
