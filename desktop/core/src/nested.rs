// Nested threads (docs/protocol.md, "Nested threads"; the nestedThreads
// experiment): a thread another thread started sits indented under the thread
// that started it in the sidebar, like a sub-agent, rather than with a row of
// its own in the Inbox or Running. Mike picked this on 13 Nov 2026 after
// "GPUI Port Coordinator" started 25 threads in a day that sat loose in his
// lists with nothing saying where they came from (mockup sub-A).
//
// Only the rules live here: which threads nest and under what, where a group
// goes, whether it opens itself, and what its rows say. The GPUI app draws
// them (sidebar.rs, projects_list.rs, thread.rs) and `dump` reports them.
//
// Not yet: dragging a row in or out of a group. AGENTS.md wants a motion
// mockup clip of a drag before it's built, so for now the row's menu has
// Move Out From Under and Put Back Under instead.
use crate::inbox::{section_of, ThreadSummary, Wake};
use std::borrow::Borrow;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

/// The experiment's id in `hello.experiments` and Settings.
pub const EXPERIMENT: &str = "nestedThreads";

/// The thread that started this one, when a thread did and it hasn't been
/// moved out from under it (`detached`). Another runner's thread, a tool's or
/// a schedule's has none.
pub fn starter_id(t: &ThreadSummary) -> Option<&str> {
    if t.detached == Some(true) {
        return None;
    }
    started_by_thread_id(t)
}

/// `startedBy.threadId` of a thread a thread started, detached or not.
fn started_by_thread_id(t: &ThreadSummary) -> Option<&str> {
    let by = t.started_by.as_ref()?;
    if by.get("kind")?.as_str()? != "thread" {
        return None;
    }
    by.get("threadId")?.as_str().filter(|id| !id.is_empty())
}

/// The thread `t` nests under: the top one of its line, one level deep, as a
/// thread started by a nested thread goes under the one at the top. `find`
/// looks a thread up by id. None when it doesn't nest: nothing started it,
/// it's detached, its starter is gone or archived, or the line goes round in
/// a circle. A finished (archived) thread in the middle of the line doesn't
/// break it: what it started is still the top thread's work.
pub fn top_starter<'a>(t: &ThreadSummary, find: &dyn Fn(&str) -> Option<&'a ThreadSummary>) -> Option<&'a ThreadSummary> {
    let mut seen: HashSet<&str> = HashSet::new();
    seen.insert(t.id.as_str());
    let mut top: Option<&'a ThreadSummary> = None;
    let mut next = starter_id(t).map(str::to_owned);
    while let Some(id) = next {
        let starter = find(&id)?;
        if !seen.insert(starter.id.as_str()) {
            // Round in a circle (A started B started A): neither nests.
            return None;
        }
        top = Some(starter);
        next = starter_id(starter).map(str::to_owned);
        // Guard against a runaway line; nobody nests 64 deep.
        if seen.len() > 64 {
            return None;
        }
    }
    top.filter(|s| !s.is_archived())
}

/// Waits on Mike: the Inbox's reasons, less a thread that's just idle there
/// (`section_of`'s first Inbox clause), and not while it hibernates.
pub fn needs_mike(t: &ThreadSummary, now_ms: i64) -> bool {
    !t.is_archived() && !t.snoozed(now_ms) && (t.review_waiting() || t.status == "waiting" || t.waiting_on_mike() || (t.needs_you && !t.working_in_background()))
}

/// Working: a turn under way or background work, not hibernating.
pub fn working(t: &ThreadSummary, now_ms: i64) -> bool {
    !t.is_archived() && !t.snoozed(now_ms) && !needs_mike(t, now_ms) && (t.busy() || t.working_in_background())
}

/// A starter's threads, and where the group goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// The threads under it that aren't archived: waiting on Mike first, then
    /// working, then the rest, newest first within each.
    pub children: Vec<String>,
    /// Its archived ones, newest archived first: one "N finished" line.
    pub finished: Vec<String>,
    /// How many of `children` wait on Mike, and how many are working.
    pub needs_you: usize,
    pub working: usize,
    /// The heading the group goes under: its most urgent member's.
    pub section: &'static str,
    /// For ordering the group among the rows of its heading: its most urgent
    /// member's urgency, its newest activity.
    pub urgency: u8,
    pub activity_ms: i64,
}

impl Group {
    /// Opens by itself while one of its threads waits on Mike.
    pub fn opens_itself(&self) -> bool {
        self.needs_you > 0
    }
}

/// Which threads nest under which, worked out once for the whole list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Nesting {
    /// Each nested thread's top starter.
    under: HashMap<String, String>,
    /// Each starter's group.
    groups: HashMap<String, Group>,
}

impl Nesting {
    /// `threads` is every thread the app has, archived ones included (they
    /// make the "N finished" line). `wakes` as for `sections_ordered`.
    pub fn build<'a, T: Borrow<ThreadSummary> + 'a>(threads: impl IntoIterator<Item = &'a T>, now_ms: i64, wakes: &HashMap<String, Wake>) -> Nesting {
        let by_id: HashMap<&str, &ThreadSummary> = threads.into_iter().map(|row| {
            let t = crate::inbox::summary(row);
            (t.id.as_str(), t)
        }).collect();
        let find = |id: &str| by_id.get(id).copied();
        let mut under = HashMap::new();
        let mut members: HashMap<&str, (Vec<&ThreadSummary>, Vec<&ThreadSummary>)> = HashMap::new();
        for &t in by_id.values() {
            // Only threads someone started can nest: the walk is skipped for
            // the rest, which is nearly all of them.
            if starter_id(t).is_none() {
                continue;
            }
            let Some(top) = top_starter(t, &find) else { continue };
            under.insert(t.id.clone(), top.id.clone());
            let entry = members.entry(top.id.as_str()).or_default();
            if t.is_archived() { entry.1.push(t) } else { entry.0.push(t) }
        }
        let rank = |t: &ThreadSummary| if needs_mike(t, now_ms) { 0 } else if working(t, now_ms) { 1 } else { 2 };
        let mut groups = HashMap::new();
        for (starter_id, (mut live, mut done)) in members {
            let starter = by_id[starter_id];
            live.sort_by_cached_key(|t| (rank(t), Reverse(t.activity_ms()), t.id.clone()));
            done.sort_by_cached_key(|t| (Reverse(t.archived_ms()), t.id.clone()));
            let needs_you = live.iter().filter(|t| needs_mike(t, now_ms)).count();
            let working_count = live.iter().filter(|t| working(t, now_ms)).count();
            let own = section_of(starter, now_ms, wakes);
            let section = if own == "starred" {
                // Starred is Mike's own "keep at the top", which a thread
                // waiting on him doesn't leave either.
                "starred"
            } else if needs_you > 0 || needs_mike(starter, now_ms) {
                "inbox"
            } else if working_count > 0 || working(starter, now_ms) {
                "running"
            } else {
                own
            };
            let awake = live.iter().filter(|t| !t.snoozed(now_ms));
            let urgency = awake.clone().map(|t| t.urgency()).chain([starter.urgency()]).min().unwrap_or(4);
            let activity_ms = awake.map(|t| t.activity_ms()).chain([starter.activity_ms()]).max().unwrap_or(0);
            groups.insert(
                starter_id.to_owned(),
                Group {
                    children: live.iter().map(|t| t.id.clone()).collect(),
                    finished: done.iter().map(|t| t.id.clone()).collect(),
                    needs_you,
                    working: working_count,
                    section,
                    urgency,
                    activity_ms,
                },
            );
        }
        Nesting { under, groups }
    }

    /// The thread it sits under, when it's nested.
    pub fn top_of(&self, id: &str) -> Option<&str> {
        self.under.get(id).map(String::as_str)
    }

    pub fn is_nested(&self, id: &str) -> bool {
        self.under.contains_key(id)
    }

    /// The group a starter heads, if it started any thread that nests.
    pub fn group(&self, starter: &str) -> Option<&Group> {
        self.groups.get(starter)
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// What a starter's row says after its place: "1 needs you · 5 working",
/// else how many threads it has ("3 threads"), else "19 finished".
pub fn group_words(g: &Group) -> String {
    let mut parts = vec![];
    if g.needs_you > 0 {
        parts.push(format!("{} needs you", g.needs_you));
    }
    if g.working > 0 {
        parts.push(format!("{} working", g.working));
    }
    if parts.is_empty() {
        let n = g.children.len();
        if n > 0 {
            parts.push(if n == 1 { "1 thread".into() } else { format!("{n} threads") });
        } else {
            parts.push(finished_words(g.finished.len()));
        }
    }
    parts.join(" · ")
}

/// "19 finished".
pub fn finished_words(n: usize) -> String {
    format!("{n} finished")
}

/// The open-or-folded button's tooltip.
pub fn toggle_tip(open: bool, g: &Group) -> String {
    let n = g.children.len() + g.finished.len();
    let what = if n == 1 { "the thread it started".to_string() } else { format!("the {n} threads it started") };
    if open { format!("Fold {what}") } else { format!("Show {what}") }
}

/// Mike's own open or fold of a group, kept while the app runs, and whether
/// the group was opening itself when he chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldChoice {
    pub open: bool,
    pub opened_itself: bool,
}

/// Whether the group shows its threads, and whether Mike's choice still
/// stands. His choice holds until the group's own reason changes: one he
/// folded while a thread waited on him opens again for the next question
/// once that one's answered, and one he opened folds once nothing waits.
pub fn group_open(choice: Option<FoldChoice>, g: &Group) -> (bool, bool) {
    let itself = g.opens_itself();
    match choice {
        Some(c) if c.opened_itself == itself => (c.open, true),
        Some(_) => (itself, false),
        None => (itself, true),
    }
}

/// The choice a click on the group's toggle makes.
pub fn toggled(choice: Option<FoldChoice>, g: &Group) -> FoldChoice {
    let (open, _) = group_open(choice, g);
    FoldChoice { open: !open, opened_itself: g.opens_itself() }
}

/// A nested thread's second line: its question while it waits on Mike (drawn
/// orange), else where it's up to: its byline (`startedBy.why`, which its
/// `end_turn` replaces with how it came out), else the start of its last
/// reply, else its place.
pub fn child_line(t: &ThreadSummary, now_ms: i64) -> (String, bool) {
    if needs_mike(t, now_ms) {
        let ask = t.asks.iter().flatten().find(|a| a.ticked != Some(true) && !a.action.trim().is_empty()).map(|a| a.action.trim().to_string());
        let said = ask.or_else(|| first_line(&t.preview)).unwrap_or_else(|| "Needs you".into());
        return (said, true);
    }
    let why = t.started_by.as_ref().and_then(|b| b.get("why")).and_then(|w| w.as_str()).map(str::trim).filter(|w| !w.is_empty()).map(str::to_owned);
    (why.or_else(|| first_line(&t.preview)).unwrap_or_else(|| t.place()), false)
}

fn first_line(text: &str) -> Option<String> {
    let plain = crate::sidebar::preview_plain(text.lines().find(|l| !l.trim().is_empty())?);
    (!plain.is_empty()).then_some(plain)
}

/// The line above a thread's transcript that says which thread started it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartedLine {
    /// The starter, when the app has it, for the link to open.
    pub thread_id: Option<String>,
    pub title: String,
    pub why: Option<String>,
}

impl StartedLine {
    /// "Started by GPUI Port Coordinator · building the Browser pane".
    pub fn text(&self) -> String {
        match &self.why {
            Some(why) => format!("Started by {} \u{b7} {why}", self.title),
            None => format!("Started by {}", self.title),
        }
    }
}

/// For a thread a thread started (detached or not): who, with the starter's
/// title as the app has it now, else as it was when it started this one.
pub fn started_line(t: &ThreadSummary, starter: Option<&ThreadSummary>) -> Option<StartedLine> {
    let by = t.started_by.as_ref()?;
    if by.get("kind")?.as_str()? != "thread" {
        return None;
    }
    let text = |key: &str| by.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    let starter = starter.filter(|s| Some(s.id.as_str()) == started_by_thread_id(t));
    let title = starter.map(|s| s.title.trim().to_owned()).filter(|s| !s.is_empty()).or_else(|| text("threadTitle")).unwrap_or_else(|| "another thread".into());
    Some(StartedLine { thread_id: starter.map(|s| s.id.clone()), title, why: text("why") })
}

/// The row menu's line for a thread a thread started: Move Out From Under
/// the thread it sits under, or Put Back Under the one that started it when
/// it was moved out and would nest again. The title, and the value for
/// `threads.update`'s `detached`.
pub fn menu_line<'a>(t: &ThreadSummary, find: &dyn Fn(&str) -> Option<&'a ThreadSummary>) -> Option<(String, bool)> {
    let titled = |s: &ThreadSummary| if s.title.trim().is_empty() { "Untitled".to_string() } else { s.title.trim().to_string() };
    if t.detached == Some(true) {
        let mut back = t.clone();
        back.detached = None;
        let top = top_starter(&back, find)?;
        let starter = started_by_thread_id(t).and_then(find).filter(|s| !s.is_archived()).unwrap_or(top);
        return Some((format!("Put Back Under {}", titled(starter)), false));
    }
    if t.is_archived() {
        return None;
    }
    let top = top_starter(t, find)?;
    Some((format!("Move Out From Under {}", titled(top)), true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::{sections_nested, InboxOrder};
    use serde_json::{json, Value};

    const NOW: &str = "2026-11-13T12:00:00Z";

    fn now() -> i64 {
        crate::inbox::parse_ms(NOW).unwrap()
    }

    fn at(minutes_ago: i64) -> String {
        crate::inbox::iso_time(now() - minutes_ago * 60_000)
    }

    fn thread(id: &str, extra: Value) -> ThreadSummary {
        let mut v = json!({ "id": id, "title": id, "status": "idle", "lastActivityAt": at(10), "cwd": "/Users/m/dev/x" });
        for (k, val) in extra.as_object().unwrap() {
            v[k] = val.clone();
        }
        serde_json::from_value(v).unwrap()
    }

    fn by(starter: &str) -> Value {
        json!({ "kind": "thread", "label": "Agent", "threadId": starter, "threadTitle": "Old title", "why": "building the Browser pane" })
    }

    fn child(id: &str, starter: &str, extra: Value) -> ThreadSummary {
        let mut t = thread(id, extra);
        t.started_by = Some(by(starter));
        t
    }

    fn asks() -> Value {
        json!({ "needs": "you", "needsYou": true, "asks": [{ "action": "Should the sign-in window come in front of yours?" }] })
    }

    fn ids<T: Borrow<ThreadSummary>>(s: &[crate::inbox::Section<T>], key: &str) -> Vec<String> {
        s.iter().find(|s| s.key == key).map(|s| s.rows.iter().map(|t| t.borrow().id.clone()).collect()).unwrap_or_default()
    }

    fn build(all: &[ThreadSummary]) -> Nesting {
        Nesting::build(all, now(), &HashMap::new())
    }

    fn sections(all: &[ThreadSummary]) -> Vec<crate::inbox::Section> {
        sections_nested(all, now(), InboxOrder::NeedsYou, &HashMap::new(), &build(all))
    }

    #[test]
    fn a_thread_a_thread_started_nests_under_it_and_nowhere_else() {
        let all = [thread("boss", json!({})), child("a", "boss", json!({ "status": "running" })), child("b", "boss", json!({})), thread("loose", json!({}))];
        let n = build(&all);
        assert_eq!(n.top_of("a"), Some("boss"));
        assert_eq!(n.top_of("b"), Some("boss"));
        assert_eq!(n.top_of("loose"), None);
        assert_eq!(n.group("boss").unwrap().children, ["a", "b"], "the working one first");
        let s = sections(&all);
        assert_eq!(ids(&s, "running"), ["boss"], "the group, under Running for its working thread");
        assert_eq!(ids(&s, "inbox"), ["loose"], "no rows of their own");
    }

    #[test]
    fn only_threads_a_live_thread_started_nest() {
        let tool = {
            let mut t = thread("tool", json!({}));
            t.started_by = Some(json!({ "kind": "tool", "label": "Sentry" }));
            t
        };
        let all = [
            thread("boss", json!({})),
            thread("gone_boss", json!({ "archivedAt": at(5) })),
            child("detached", "boss", json!({ "detached": true })),
            child("orphan", "deleted", json!({})),
            child("under_archived", "gone_boss", json!({})),
            {
                let mut t = thread("remote", json!({}));
                t.started_by = Some(json!({ "kind": "thread", "label": "Agent", "threadTitle": "On bruce" }));
                t
            },
            tool,
        ];
        let n = build(&all);
        for id in ["detached", "orphan", "under_archived", "remote", "tool"] {
            assert!(!n.is_nested(id), "{id} keeps its own row");
        }
        assert!(n.group("gone_boss").is_none());
        assert!(n.is_empty());
    }

    #[test]
    fn a_grandchild_nests_under_the_top_of_its_line() {
        let all = [
            thread("boss", json!({})),
            child("mid", "boss", json!({})),
            child("grand", "mid", json!({ "status": "running" })),
            child("done_mid", "boss", json!({ "archivedAt": at(30) })),
            child("grand2", "done_mid", json!({})),
        ];
        let n = build(&all);
        assert_eq!(n.top_of("grand"), Some("boss"), "one level deep");
        assert_eq!(n.top_of("grand2"), Some("boss"), "a finished thread in the middle doesn't break the line");
        assert!(n.group("mid").is_none());
        let g = n.group("boss").unwrap();
        assert_eq!(g.children, ["grand", "grand2", "mid"]);
        assert_eq!(g.finished, ["done_mid"]);
    }

    #[test]
    fn a_detached_middle_heads_its_own_group() {
        let all = [thread("boss", json!({})), child("mid", "boss", json!({ "detached": true })), child("grand", "mid", json!({}))];
        let n = build(&all);
        assert_eq!(n.top_of("grand"), Some("mid"));
        assert!(!n.is_nested("mid"));
    }

    #[test]
    fn a_circle_nests_nothing() {
        let all = [child("a", "b", json!({})), child("b", "a", json!({}))];
        assert!(build(&all).is_empty());
    }

    #[test]
    fn the_group_goes_where_its_most_urgent_member_would() {
        // A child waiting on Mike takes a hibernating starter to the Inbox.
        let asleep = json!({ "snoozedUntil": at(-600) });
        let all = [thread("boss", asleep.clone()), child("q", "boss", asks()), child("w", "boss", json!({ "status": "running" }))];
        let s = sections(&all);
        assert_eq!(ids(&s, "inbox"), ["boss"]);
        assert!(ids(&s, "hibernating").is_empty());
        let g = build(&all).group("boss").cloned().unwrap();
        assert_eq!((g.needs_you, g.working), (1, 1));
        assert_eq!(g.children, ["q", "w"], "the question first");

        // Working children, nothing for Mike: Running.
        let all = [thread("boss", json!({})), child("w", "boss", json!({ "status": "running" }))];
        assert_eq!(ids(&sections(&all), "running"), ["boss"]);

        // The starter itself waiting on Mike: Inbox.
        let all = [thread("boss", asks()), child("w", "boss", json!({ "status": "running" }))];
        assert_eq!(ids(&sections(&all), "inbox"), ["boss"]);

        // Nothing doing: where the starter is.
        let all = [thread("boss", asleep), child("idle", "boss", json!({}))];
        assert_eq!(ids(&sections(&all), "hibernating"), ["boss"]);

        // A hibernating child waiting on Mike doesn't count.
        let all = [thread("boss", json!({})), child("q", "boss", { let mut a = asks(); a["snoozedUntil"] = json!(at(-60)); a })];
        let g = build(&all).group("boss").cloned().unwrap();
        assert_eq!(g.needs_you, 0);
        assert_eq!(g.section, "inbox", "the starter's own place: idle, so the Inbox");

        // Starred stays starred.
        let all = [thread("boss", json!({ "starredAt": at(100) })), child("q", "boss", asks())];
        assert_eq!(ids(&sections(&all), "starred"), ["boss"]);
    }

    #[test]
    fn a_group_sorts_by_its_most_urgent_and_newest_member() {
        let all = [
            thread("plain", json!({ "lastActivityAt": at(1) })),
            thread("boss", json!({ "lastActivityAt": at(500) })),
            child("q", "boss", { let mut a = asks(); a["lastActivityAt"] = json!(at(2)); a }),
        ];
        assert_eq!(ids(&sections(&all), "inbox"), ["boss", "plain"], "its child's question ranks it");
    }

    #[test]
    fn heading_counts_leave_nested_threads_out() {
        let mut all = vec![thread("boss", json!({ "status": "running" }))];
        for i in 0..6 {
            all.push(child(&format!("c{i}"), "boss", json!({ "status": "running" })));
        }
        all.push(thread("other", json!({ "status": "running" })));
        assert_eq!(ids(&sections(&all), "running"), ["boss", "other"]);
    }

    #[test]
    fn archived_children_are_one_finished_line() {
        let all = [
            thread("boss", json!({})),
            child("old", "boss", json!({ "archivedAt": at(300) })),
            child("older", "boss", json!({ "archivedAt": at(600) })),
            child("live", "boss", json!({})),
        ];
        let s = sections(&all);
        assert!(ids(&s, "archived").is_empty(), "not in Archived either");
        let g = build(&all).group("boss").cloned().unwrap();
        assert_eq!(g.finished, ["old", "older"]);
        assert_eq!(finished_words(g.finished.len()), "2 finished");
    }

    #[test]
    fn it_opens_itself_while_a_child_waits_on_mike() {
        let quiet = [thread("boss", json!({})), child("w", "boss", json!({ "status": "running" }))];
        let asking = [thread("boss", json!({})), child("w", "boss", asks())];
        let quiet = build(&quiet).group("boss").cloned().unwrap();
        let asking = build(&asking).group("boss").cloned().unwrap();
        assert_eq!(group_open(None, &quiet), (false, true), "folded while nothing waits");
        assert_eq!(group_open(None, &asking), (true, true));
        // Mike folds the one with a question: it stays folded while that waits.
        let folded = toggled(None, &asking);
        assert_eq!(group_open(Some(folded), &asking), (false, true));
        // Once answered his choice goes, and the next question opens it again.
        assert_eq!(group_open(Some(folded), &quiet), (false, false));
        // He opens a quiet one: it stays open while quiet.
        let opened = toggled(None, &quiet);
        assert_eq!(group_open(Some(opened), &quiet), (true, true));
    }

    #[test]
    fn the_starter_row_counts_what_needs_mike_and_what_works() {
        let all = [thread("boss", json!({})), child("q", "boss", asks()), child("w1", "boss", json!({ "status": "running" })), child("w2", "boss", json!({ "status": "starting" }))];
        let g = build(&all).group("boss").cloned().unwrap();
        assert_eq!(group_words(&g), "1 needs you \u{b7} 2 working");
        let all = [thread("boss", json!({})), child("w", "boss", json!({ "status": "running" }))];
        assert_eq!(group_words(build(&all).group("boss").unwrap()), "1 working");
        let all = [thread("boss", json!({})), child("i", "boss", json!({})), child("j", "boss", json!({}))];
        assert_eq!(group_words(build(&all).group("boss").unwrap()), "2 threads");
        let all = [thread("boss", json!({})), child("i", "boss", json!({ "archivedAt": at(3) }))];
        assert_eq!(group_words(build(&all).group("boss").unwrap()), "1 finished");
        assert_eq!(toggle_tip(false, build(&all).group("boss").unwrap()), "Show the thread it started");
    }

    #[test]
    fn a_child_row_shows_its_question_or_where_its_up_to() {
        let q = child("q", "boss", asks());
        assert_eq!(child_line(&q, now()), ("Should the sign-in window come in front of yours?".into(), true));
        let w = child("w", "boss", json!({ "status": "running", "preview": "Landing, 4th in the queue" }));
        assert_eq!(child_line(&w, now()), ("building the Browser pane".into(), false), "its byline first");
        let mut plain = child("p", "boss", json!({ "preview": "**Done**: merged #12\nmore" }));
        plain.started_by = Some(json!({ "kind": "thread", "threadId": "boss" }));
        assert_eq!(child_line(&plain, now()), ("Done: merged #12".into(), false));
    }

    #[test]
    fn the_started_by_line_links_the_starter() {
        let boss = thread("boss", json!({ "title": "GPUI Port Coordinator" }));
        let t = child("c", "boss", json!({}));
        let line = started_line(&t, Some(&boss)).unwrap();
        assert_eq!(line.text(), "Started by GPUI Port Coordinator \u{b7} building the Browser pane");
        assert_eq!(line.thread_id.as_deref(), Some("boss"));
        // The starter gone: its title as it was, and no link.
        let line = started_line(&t, None).unwrap();
        assert_eq!(line.title, "Old title");
        assert_eq!(line.thread_id, None);
        // Not a thread's: no line.
        let mut tool = thread("tool", json!({}));
        tool.started_by = Some(json!({ "kind": "tool", "label": "Sentry" }));
        assert!(started_line(&tool, None).is_none());
        assert!(started_line(&thread("mine", json!({})), None).is_none());
    }

    #[test]
    fn the_menu_moves_a_thread_out_and_back() {
        let all = [thread("boss", json!({ "title": "GPUI Port Coordinator" })), child("c", "boss", json!({})), child("d", "boss", json!({ "detached": true })), thread("x", json!({}))];
        let find = |id: &str| all.iter().find(|t| t.id == id);
        assert_eq!(menu_line(&all[1], &find), Some(("Move Out From Under GPUI Port Coordinator".into(), true)));
        assert_eq!(menu_line(&all[2], &find), Some(("Put Back Under GPUI Port Coordinator".into(), false)));
        assert_eq!(menu_line(&all[3], &find), None);
        // A detached one whose starter's archived has nowhere to go back to.
        let gone = [thread("boss", json!({ "archivedAt": at(1) })), child("d", "boss", json!({ "detached": true }))];
        let find = |id: &str| gone.iter().find(|t| t.id == id);
        assert_eq!(menu_line(&gone[1], &find), None);
    }

    #[test]
    fn detached_reads_tolerantly() {
        let t: ThreadSummary = serde_json::from_value(json!({ "id": "a", "detached": "yes" })).unwrap();
        assert_eq!(t.detached, None, "a value it can't read is left out");
        let t: ThreadSummary = serde_json::from_value(json!({ "id": "a", "detached": true })).unwrap();
        assert_eq!(t.detached, Some(true));
        let t: ThreadSummary = serde_json::from_value(json!({ "id": "a" })).unwrap();
        assert_eq!(t.detached, None);
    }
}
