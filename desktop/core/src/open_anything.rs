// Open Anything, ⌘K: one box over the window that opens a thread, a panel, a
// project or one of the app's pages by name. The GPUI app's own (the Swift
// app is frozen and hasn't got it); Mike picked this design, "A, the palette",
// in thread 5d33618f's review. What it lists and in what order is worked out
// here, with no window; the box itself is gpui/src/open_anything.rs.
//
// Nothing typed: what was opened lately (through the box or otherwise), then
// the pages. Typing: the matches, grouped Panels, Threads, Projects, App.
// Threads match as the sidebar's search does (inbox::matches: title, last
// reply, the librarian's search words), archived and hibernating ones too.
use crate::highlights::Panel;
use crate::inbox::{self, fold, ThreadSummary};
use crate::projects::{is_inside, Project};
use chrono::{DateTime, TimeZone};
use std::collections::HashSet;

/// How many entries Recent keeps, and how many the box shows.
pub const RECENT_KEPT: usize = 12;
pub const RECENT_SHOWN: usize = 4;
/// At most this many of each group while typing, so one group can't push
/// the rest out of sight.
const PANELS_SHOWN: usize = 6;
const THREADS_SHOWN: usize = 8;
const PROJECTS_SHOWN: usize = 4;
const APP_SHOWN: usize = 6;

/// The words in the empty box.
pub const PROMPT: &str = "Open a thread, panel, project or page";

/// The app's own places: the activity bar's pages and panels, and Settings.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Inbox,
    Scheduled,
    Projects,
    Librarian,
    /// Settings, on one of its sections (its name, "keyboard") or as it was.
    Settings(Option<String>),
    Usage,
    Updates,
    Connectors,
    Runners,
}

impl Page {
    pub fn name(&self) -> String {
        match self {
            Page::Inbox => "inbox".into(),
            Page::Scheduled => "scheduled".into(),
            Page::Projects => "projects".into(),
            Page::Librarian => "librarian".into(),
            Page::Settings(None) => "settings".into(),
            Page::Settings(Some(tab)) => format!("settings.{tab}"),
            Page::Usage => "usage".into(),
            Page::Updates => "updates".into(),
            Page::Connectors => "connectors".into(),
            Page::Runners => "runners".into(),
        }
    }

    fn symbol(&self) -> &'static str {
        match self {
            Page::Inbox => "bubble.left.and.bubble.right",
            Page::Scheduled => "clock",
            Page::Projects => "square.stack.3d.up",
            Page::Librarian => "books.vertical",
            Page::Settings(_) => "gearshape",
            Page::Usage => "gauge.with.dots.needle.33percent",
            Page::Updates => "arrow.down.circle",
            Page::Connectors => "powerplug",
            Page::Runners => "server.rack",
        }
    }
}

/// What picking an entry opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Thread(String),
    Project(String),
    /// A patch panel: its id, the newest thread that has it from that folder
    /// (whose summary has it now, badge and all), and the project its folder
    /// is in, if any, whose page has it on the rail too.
    Panel { id: String, thread: String, project: Option<String> },
    Page(Page),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Panel,
    Thread,
    Project,
    App,
}

impl Kind {
    fn group(self) -> &'static str {
        match self {
            Kind::Panel => "Panels",
            Kind::Thread => "Threads",
            Kind::Project => "Projects",
            Kind::App => "App",
        }
    }

    /// Before an entry's own words under Recent, where the groups don't say it.
    fn word(self) -> Option<&'static str> {
        match self {
            Kind::Panel => Some("panel"),
            Kind::Thread => Some("thread"),
            Kind::Project => Some("project"),
            Kind::App => None,
        }
    }
}

/// One line in the box.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Unique in the box, and what Recent keeps.
    pub key: String,
    pub kind: Kind,
    pub title: String,
    /// On the right: where it's from, its state or its key.
    pub detail: String,
    /// A panel's count, as its rail button has it.
    pub badge: Option<i64>,
    /// The icon's SF Symbol name; a panel's own picture is drawn from its thread.
    pub symbol: &'static str,
    pub target: Target,
    /// How its name fits what's typed: 2 all of it, 1 its start, 0 neither
    /// (`name_fit`). The pick goes to the best of these, so "usage" picks
    /// Usage over a thread that only mentions the word, as a launcher does.
    pub named: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub title: &'static str,
    pub items: Vec<Item>,
}

/// One of the app's pages, as the app offers it now: only the ones it has
/// (Scheduled needs a runner that schedules, Runners more than one runner),
/// each with its key from shortcuts.json if it has one.
#[derive(Clone, Debug, PartialEq)]
pub struct PageEntry {
    pub page: Page,
    pub title: String,
    pub shortcut: Option<String>,
}

/// Everything the box can open, built from what the app holds.
pub struct Catalog {
    panels: Vec<(Item, ThreadSummary)>,
    threads: Vec<(Item, ThreadSummary)>,
    projects: Vec<Item>,
    pages: Vec<(Item, bool)>,
}

pub fn thread_key(id: &str) -> String {
    format!("thread:{id}")
}

pub fn project_key(id: &str) -> String {
    format!("project:{id}")
}

pub fn page_key(page: &Page) -> String {
    format!("page:{}", page.name())
}

/// A panel's place, one entry per place however many threads have it there:
/// its project, else its patch's folder, else the thread's own folder (a
/// built-in's), with "home" for ~/.threadbear's, which every thread has.
pub fn panel_key(panel: &Panel, thread: &ThreadSummary) -> String {
    format!("panel:{}|{}", panel_origin(panel, thread).0, panel.id)
}

/// The place's key, and its folder when it has one.
fn panel_origin(panel: &Panel, thread: &ThreadSummary) -> (String, Option<String>) {
    let patch = thread.patches().into_iter().find(|p| p.id == panel.patch);
    if let Some(id) = panel.project_id.clone().or_else(|| patch.as_ref().and_then(|p| p.project_id.clone())) {
        return (format!("project:{id}"), None);
    }
    if patch.as_ref().is_some_and(|p| p.source == "home") {
        return ("home".into(), None);
    }
    let folder = patch.and_then(|p| p.folder).filter(|f| !f.is_empty()).unwrap_or_else(|| thread.cwd.clone());
    (folder.clone(), Some(folder))
}

impl Catalog {
    /// `threads` in any order; the newest of those that have a panel from a
    /// place is the one it opens on, and gives its badge.
    pub fn new<'a, Tz: TimeZone>(threads: impl IntoIterator<Item = &'a ThreadSummary>, projects: &[Project], pages: Vec<PageEntry>, settings_tabs: &[(String, String)], now: &DateTime<Tz>) -> Catalog {
        let now_ms = now.timestamp_millis();
        let mut all: Vec<&ThreadSummary> = threads.into_iter().collect();
        // Open threads before archived ones, then newest first: a panel opens
        // on the thread Mike's most likely still at.
        all.sort_by(|a, b| a.is_archived().cmp(&b.is_archived()).then(b.activity_ms().cmp(&a.activity_ms())));
        let project_title = |id: &str| projects.iter().find(|p| p.id == id).map(|p| if p.title.is_empty() { "Untitled project".to_owned() } else { p.title.clone() });
        let mut seen = HashSet::new();
        let mut panels = Vec::new();
        for thread in &all {
            for panel in thread.panels() {
                let key = panel_key(&panel, thread);
                if !seen.insert(key.clone()) {
                    continue;
                }
                let (origin, folder) = panel_origin(&panel, thread);
                // On the project's page when its folder is in one: project
                // pages have their folders' panels on the rail.
                let project = origin.strip_prefix("project:").map(str::to_owned).or_else(|| folder.as_deref().and_then(|f| projects.iter().find(|p| p.folders.iter().any(|pf| is_inside(f, pf))).map(|p| p.id.clone())));
                let detail = match (origin.strip_prefix("project:"), folder.as_deref()) {
                    (Some(id), _) => project_title(id).unwrap_or_else(|| "a project".into()),
                    (None, Some(folder)) => inbox::folder_name(folder),
                    (None, None) => "your own patches".into(),
                };
                let item = Item { key, kind: Kind::Panel, title: panel.title.clone(), detail, badge: panel.badge.filter(|b| *b > 0), symbol: "square.dashed", target: Target::Panel { id: panel.id.clone(), thread: thread.id.clone(), project }, named: 0 };
                panels.push((item, (*thread).clone()));
            }
        }
        let threads = all
            .iter()
            .map(|t| {
                let title = if t.title.is_empty() { "Untitled".to_owned() } else { t.title.clone() };
                let item = Item { key: thread_key(&t.id), kind: Kind::Thread, title, detail: thread_detail(t, now_ms, now), badge: None, symbol: "text.alignleft", target: Target::Thread(t.id.clone()), named: 0 };
                (item, (*t).clone())
            })
            .collect();
        let projects = projects
            .iter()
            .map(|p| {
                let count = all.iter().filter(|t| !t.is_archived() && t.projects.iter().flatten().any(|m| m.id == p.id)).count();
                let detail = match count {
                    0 => String::new(),
                    1 => "1 thread".into(),
                    n => format!("{n} threads"),
                };
                let title = if p.title.is_empty() { "Untitled project".to_owned() } else { p.title.clone() };
                Item { key: project_key(&p.id), kind: Kind::Project, title, detail, badge: None, symbol: "square.stack.3d.up", target: Target::Project(p.id.clone()), named: 0 }
            })
            .collect();
        let settings_key = pages.iter().find(|p| p.page == Page::Settings(None)).and_then(|p| p.shortcut.clone());
        let mut page_items: Vec<(Item, bool)> = pages.into_iter().map(|p| (page_item(&p.page, p.title, p.shortcut), true)).collect();
        // Settings' sections too, under App only: "Settings › Keyboard".
        for (name, title) in settings_tabs {
            let page = Page::Settings(Some(name.clone()));
            page_items.push((page_item(&page, format!("Settings › {title}"), settings_key.clone()), false));
        }
        Catalog { panels, threads, projects, pages: page_items }
    }

    /// The groups for what's typed; Recent and Go to with nothing.
    pub fn groups(&self, query: &str, recent: &[String]) -> Vec<Group> {
        let query = query.trim();
        if query.is_empty() {
            let mut out = Vec::new();
            let recent_items: Vec<Item> = recent.iter().filter_map(|key| self.find(key)).map(recent_look).take(RECENT_SHOWN).collect();
            if !recent_items.is_empty() {
                out.push(Group { title: "Recent", items: recent_items });
            }
            out.push(Group { title: "Go to", items: self.pages.iter().filter(|(_, go_to)| *go_to).map(|(i, _)| i.clone()).collect() });
            return out;
        }
        let q = fold(query);
        let ranked = |items: Vec<(i32, i64, Item)>, limit: usize| -> Vec<Item> {
            let mut items = items;
            items.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
            items.into_iter().take(limit).map(|(_, _, i)| i).collect()
        };
        let panels = ranked(self.panels.iter().filter_map(|(i, t)| best(&q, &[&i.title, &i.detail]).map(|s| (s, t.activity_ms(), i.clone()))).collect(), PANELS_SHOWN);
        let threads = ranked(
            self.threads
                .iter()
                .filter_map(|(i, t)| {
                    // The title first; the last reply and the librarian's
                    // words find it too, below anything named for it.
                    let score = best(&q, &[&i.title]).or_else(|| inbox::matches(t, &q).then_some(150))?;
                    // Archived ones after open ones that match as well.
                    Some((score - if t.is_archived() { 50 } else { 0 }, t.activity_ms(), i.clone()))
                })
                .collect(),
            THREADS_SHOWN,
        );
        let projects = ranked(self.projects.iter().filter_map(|i| best(&q, &[&i.title]).map(|s| (s, 0, i.clone()))).collect(), PROJECTS_SHOWN);
        let app = ranked(self.pages.iter().filter_map(|(i, _)| best(&q, &[&i.title]).map(|s| (s, 0, i.clone()))).collect(), APP_SHOWN);
        [(Kind::Panel, panels), (Kind::Thread, threads), (Kind::Project, projects), (Kind::App, app)]
            .into_iter()
            .filter(|(_, items)| !items.is_empty())
            .map(|(kind, items)| Group { title: kind.group(), items: items.into_iter().map(|i| Item { named: name_fit(&q, &i.title), ..i }).collect() })
            .collect()
    }

    /// An entry by its key, wherever it is.
    pub fn find(&self, key: &str) -> Option<Item> {
        self.panels.iter().map(|(i, _)| i).chain(self.threads.iter().map(|(i, _)| i)).chain(self.projects.iter()).chain(self.pages.iter().map(|(i, _)| i)).find(|i| i.key == key).cloned()
    }

    /// The thread a panel's entry draws its icon from.
    pub fn panel_thread(&self, key: &str) -> Option<&ThreadSummary> {
        self.panels.iter().find(|(i, _)| i.key == key).map(|(_, t)| t)
    }
}

fn page_item(page: &Page, title: String, shortcut: Option<String>) -> Item {
    Item { key: page_key(page), kind: Kind::App, title, detail: shortcut.unwrap_or_default(), badge: None, symbol: page.symbol(), target: Target::Page(page.clone()), named: 0 }
}

/// A thread's right-hand words: archived or hibernating (and when), running,
/// else its project or folder and when it last did anything.
fn thread_detail<Tz: TimeZone>(t: &ThreadSummary, now_ms: i64, now: &DateTime<Tz>) -> String {
    let when = crate::sidebar::relative_short(t.activity_ms(), now);
    let when = if when == "now" { "just now".to_owned() } else { when };
    if t.is_archived() {
        format!("archived · {when}")
    } else if t.snoozed(now_ms) {
        "hibernating".into()
    } else if t.busy() {
        "running".into()
    } else {
        format!("{} · {when}", t.place())
    }
}

/// Under Recent the group doesn't say what each is, so its line does:
/// "panel · Walkies", "thread · running", "project".
fn recent_look(mut item: Item) -> Item {
    // Its own key in the box, as the same entry can be under Go to as well.
    item.key = format!("{RECENT_MARK}{}", item.key);
    if let Some(word) = item.kind.word() {
        let state = match item.kind {
            // A thread's state, not its folder and date: the place is the
            // other entries' business.
            Kind::Thread => item.detail.split(" · ").next().filter(|s| matches!(*s, "running" | "hibernating" | "archived")).map(str::to_owned),
            Kind::Project => None,
            _ => Some(item.detail.clone()).filter(|d| !d.is_empty()),
        };
        item.detail = match state {
            Some(state) => format!("{word} · {state}"),
            None => word.to_owned(),
        };
    }
    item
}

const RECENT_MARK: &str = "recent|";

impl Item {
    /// The key Recent keeps: the same for an entry under Recent and elsewhere.
    pub fn kept_key(&self) -> &str {
        self.key.strip_prefix(RECENT_MARK).unwrap_or(&self.key)
    }
}

/// The best of `score` over a few texts.
fn best(query: &str, texts: &[&str]) -> Option<i32> {
    texts.iter().filter_map(|t| score(query, &fold(t))).max()
}

/// How well `query` (folded) names `text` (folded): its start, a word's
/// start, anywhere in it, each of its words somewhere in it, or its letters
/// in order ("ytcm" for "yt-channel-manager"). None when it doesn't.
pub fn score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    // A shorter name it fits is the better match: "Usage" over "Usage history".
    let shorter = -(text.chars().count().min(200) as i32);
    if text.starts_with(query) {
        return Some(1000 + shorter);
    }
    if let Some(at) = text.find(query) {
        let word_start = text[..at].chars().last().is_none_or(|c| !c.is_alphanumeric());
        return Some(if word_start { 800 } else { 600 } + shorter);
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.len() > 1 && words.iter().all(|w| text.contains(w)) {
        return Some(500 + shorter);
    }
    // Letters in order, worth less the more spread out they are.
    let mut chars = text.chars().enumerate();
    let (mut first, mut last) = (None, 0usize);
    for q in query.chars().filter(|c| !c.is_whitespace()) {
        let (i, _) = chars.by_ref().find(|(_, c)| *c == q)?;
        first.get_or_insert(i);
        last = i;
    }
    let spread = (last - first.unwrap_or(0)) as i32;
    let fits = query.chars().filter(|c| !c.is_whitespace()).count() as i32;
    // Too spread out isn't a match anyone meant.
    if spread > fits * 4 + 4 {
        return None;
    }
    Some((300 - (spread - fits) * 10).max(1) + shorter / 10)
}

/// Recent with `key` at the front, once, kept to RECENT_KEPT.
pub fn used(key: &str, recent: &[String]) -> Vec<String> {
    std::iter::once(key.to_owned()).chain(recent.iter().filter(|k| *k != key).cloned()).take(RECENT_KEPT).collect()
}

/// The entries in the order the arrows go through them.
pub fn flat(groups: &[Group]) -> Vec<&Item> {
    groups.iter().flat_map(|g| g.items.iter()).collect()
}

/// How a name fits `query` (folded): 2 when it's all of it, 1 when it starts
/// with it, else 0. A Settings section goes by its own name too, the part
/// after "Settings › ".
fn name_fit(query: &str, title: &str) -> u8 {
    let title = fold(title);
    let section = title.rsplit_once("› ").map(|(_, s)| s.to_owned());
    [Some(title), section].into_iter().flatten().map(|name| if name == query { 2 } else { u8::from(name.starts_with(query)) }).max().unwrap_or(0)
}

/// The picked entry: the one chosen if it's still listed, else the first
/// whose name fits what's typed best (`Item::named`), else the first.
pub fn picked<'a>(groups: &'a [Group], selected: Option<&str>) -> Option<&'a Item> {
    let all = flat(groups);
    // max_by_key keeps the last of equals; the first in the box should win.
    let best = || all.iter().rev().max_by_key(|i| i.named).filter(|i| i.named > 0).copied();
    selected.and_then(|k| all.iter().find(|i| i.key == k).copied()).or_else(best).or_else(|| all.first().copied())
}

/// ↑ or ↓ from the picked entry, stopping at the ends as the slash
/// palette's arrows do.
pub fn step(groups: &[Group], selected: Option<&str>, down: bool) -> Option<String> {
    let all = flat(groups);
    let at = picked(groups, selected).and_then(|p| all.iter().position(|i| i.key == p.key))?;
    let next = if down { (at + 1).min(all.len() - 1) } else { at.saturating_sub(1) };
    Some(all[next].key.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    fn thread(id: &str, title: &str, cwd: &str, minutes_ago: i64) -> ThreadSummary {
        let at = Utc::now() - chrono::Duration::minutes(minutes_ago);
        serde_json::from_value(json!({ "id": id, "title": title, "cwd": cwd, "status": "idle", "lastActivityAt": at.to_rfc3339() })).unwrap()
    }

    fn with_panel(mut t: ThreadSummary, id: &str, title: &str, patch: &str, folder: &str, badge: Option<i64>) -> ThreadSummary {
        let mut panel = json!({ "id": id, "title": title, "kind": "url", "url": "http://x", "patch": patch });
        if let Some(b) = badge {
            panel["badge"] = json!(b);
        }
        t.panels.get_or_insert_with(Vec::new).push(panel);
        t.patches.get_or_insert_with(Vec::new).push(json!({ "id": patch, "source": "project", "folder": folder }));
        t
    }

    fn project(id: &str, title: &str, folders: &[&str]) -> Project {
        Project { id: id.into(), title: title.into(), folders: folders.iter().map(|f| f.to_string()).collect(), ..Default::default() }
    }

    fn pages() -> Vec<PageEntry> {
        vec![
            PageEntry { page: Page::Inbox, title: "Inbox".into(), shortcut: Some("⌘1".into()) },
            PageEntry { page: Page::Scheduled, title: "Scheduled".into(), shortcut: Some("⌘2".into()) },
            PageEntry { page: Page::Settings(None), title: "Settings".into(), shortcut: Some("⌘,".into()) },
            PageEntry { page: Page::Usage, title: "Usage".into(), shortcut: None },
        ]
    }

    fn tabs() -> Vec<(String, String)> {
        vec![("general".into(), "General".into()), ("keyboard".into(), "Keyboard".into()), ("devices".into(), "Phone & browser".into())]
    }

    fn titles(g: &Group) -> Vec<&str> {
        g.items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn nothing_typed_shows_recent_then_the_pages() {
        let a = thread("a", "Plan the GPUI port", "/w/threadbear", 5);
        let c = Catalog::new([&a], &[], pages(), &tabs(), &Utc::now());
        let groups = c.groups("", &[thread_key("a"), thread_key("gone"), page_key(&Page::Usage)]);
        assert_eq!(groups[0].title, "Recent");
        assert_eq!(titles(&groups[0]), ["Plan the GPUI port", "Usage"], "a thread that's gone is left out");
        assert_eq!(groups[0].items[0].detail, "thread");
        assert_eq!(groups[1].title, "Go to");
        assert_eq!(titles(&groups[1]), ["Inbox", "Scheduled", "Settings", "Usage"], "Settings' sections only when typed");
        assert_eq!(groups[1].items[0].detail, "⌘1");
        // Nothing opened yet: just the pages.
        assert_eq!(c.groups("  ", &[]).iter().map(|g| g.title).collect::<Vec<_>>(), ["Go to"]);
    }

    #[test]
    fn typing_groups_panels_threads_projects_and_the_app() {
        let yt = with_panel(thread("y", "YouTube management patch", "/w/yt-channel-manager", 60), "yt.comments", "YouTube comments", "yt", "/w/yt-channel-manager", Some(4));
        let other = thread("n", "Nipunn video: YouTube title test", "/w/videos", 120);
        let projects = [project("p1", "Convex YouTube channel", &["/w/videos"])];
        let c = Catalog::new([&yt, &other], &projects, pages(), &tabs(), &Utc::now());
        let groups = c.groups("yout", &[]);
        assert_eq!(groups.iter().map(|g| g.title).collect::<Vec<_>>(), ["Panels", "Threads", "Projects"]);
        assert_eq!(groups[0].items[0].title, "YouTube comments");
        assert_eq!(groups[0].items[0].badge, Some(4));
        assert_eq!(groups[0].items[0].detail, "yt-channel-manager", "which folder it's from");
        assert_eq!(titles(&groups[1]), ["YouTube management patch", "Nipunn video: YouTube title test"], "a title that starts with it first");
        assert_eq!(groups[2].items[0].detail, "", "no open threads in it");
    }

    #[test]
    fn threads_match_as_the_sidebar_search_does_archived_and_hibernating_too() {
        let mut t = thread("w", "Wisdom tooth", "/w/misc", 10);
        t.keywords = Some("dentist appointment".into());
        t.archived_at = Some(Utc::now().to_rfc3339());
        let mut h = thread("h", "Dentist bill", "/w/misc", 20);
        h.snoozed_until_restart = Some(true);
        let c = Catalog::new([&t, &h], &[], vec![], &[], &Utc::now());
        let groups = c.groups("dentist", &[]);
        assert_eq!(titles(&groups[0]), ["Dentist bill", "Wisdom tooth"], "named for it beats the librarian's words");
        assert_eq!(groups[0].items[0].detail, "hibernating");
        assert!(groups[0].items[1].detail.starts_with("archived · "));
    }

    #[test]
    fn the_app_has_settings_sections_as_well_as_the_pages() {
        let c = Catalog::new([], &[], pages(), &tabs(), &Utc::now());
        let groups = c.groups("keyb", &[]);
        assert_eq!(groups[0].title, "App");
        assert_eq!(groups[0].items[0].title, "Settings › Keyboard");
        assert_eq!(groups[0].items[0].detail, "⌘,");
        assert_eq!(groups[0].items[0].target, Target::Page(Page::Settings(Some("keyboard".into()))));
        assert_eq!(titles(&c.groups("phone", &[])[0]), ["Settings › Phone & browser"]);
    }

    #[test]
    fn a_panel_from_one_place_is_one_entry_on_its_newest_thread_with_its_project() {
        let old = with_panel(thread("old", "Older", "/w/walkies", 90), "convex.dashboard", "Convex", "convex", "/w/walkies", None);
        let new = with_panel(thread("new", "Newer", "/w/walkies/app", 5), "convex.dashboard", "Convex", "convex", "/w/walkies", Some(2));
        let elsewhere = with_panel(thread("e", "Else", "/w/poddle", 1), "convex.dashboard", "Convex", "convex", "/w/poddle", None);
        let projects = [project("walk", "Walkies", &["/w/walkies"])];
        let c = Catalog::new([&old, &new, &elsewhere], &projects, vec![], &[], &Utc::now());
        let panels = &c.groups("convex", &[])[0];
        assert_eq!(panels.items.len(), 2, "one per folder");
        let walkies = panels.items.iter().find(|i| i.detail == "walkies").unwrap();
        assert_eq!(walkies.target, Target::Panel { id: "convex.dashboard".into(), thread: "new".into(), project: Some("walk".into()) });
        assert_eq!(walkies.badge, Some(2), "the newest thread's count");
        let poddle = panels.items.iter().find(|i| i.detail == "poddle").unwrap();
        assert_eq!(poddle.target, Target::Panel { id: "convex.dashboard".into(), thread: "e".into(), project: None }, "not in a project: its thread");
    }

    #[test]
    fn recent_says_what_each_is() {
        let mut busy = thread("b", "Plan the GPUI port", "/w/tb", 1);
        busy.status = "running".into();
        let p = with_panel(thread("y", "Y", "/w/yt", 1), "yt.c", "YouTube comments", "yt", "/w/yt", None);
        let projects = [project("p", "Threadbear GPUI port", &["/w/tb"])];
        let c = Catalog::new([&busy, &p], &projects, vec![], &[], &Utc::now());
        let key = c.groups("youtube", &[])[0].items[0].key.clone();
        let recent = c.groups("", &[key, thread_key("b"), project_key("p")]);
        let details: Vec<&str> = recent[0].items.iter().map(|i| i.detail.as_str()).collect();
        assert_eq!(details, ["panel · yt", "thread · running", "project"]);
    }

    #[test]
    fn a_score_prefers_the_start_then_a_word_then_anywhere_then_letters_in_order() {
        let s = |q: &str, t: &str| score(q, t);
        assert!(s("you", "youtube") > s("you", "my youtube"));
        assert!(s("tube", "my tube") > s("tube", "youtube"));
        assert!(s("tube", "youtube") > s("ytcm", "yt-channel-manager"));
        assert!(s("ytcm", "yt-channel-manager").is_some(), "letters in order");
        assert_eq!(s("zz", "youtube"), None);
        assert_eq!(s("ab", &format!("a{}b", "x".repeat(40))), None, "too spread out");
        assert!(s("usage", "usage") > s("usage", "usage history"), "the shorter name");
        assert!(s("gpui port", "plan the gpui port").is_some());
        assert!(s("port gpui", "plan the gpui port").is_some(), "each word somewhere");
    }

    #[test]
    fn recent_keeps_each_once_newest_first() {
        let r = used("b", &["a".into(), "b".into(), "c".into()]);
        assert_eq!(r, ["b", "a", "c"]);
        let long: Vec<String> = (0..20).map(|i| i.to_string()).collect();
        assert_eq!(used("x", &long).len(), RECENT_KEPT);
    }

    #[test]
    fn a_page_named_for_whats_typed_is_the_pick_over_a_thread_that_mentions_it() {
        // "usage" found "Make a poster for the meetup" by its last reply, and
        // as Threads come before App it was the pick over Usage (13 Nov).
        let mut poster = thread("m", "Make a poster for the meetup", "/w/meetup", 10);
        poster.keywords = Some("check the usage rights on the photos".into());
        let c = Catalog::new([&poster], &[], pages(), &tabs(), &Utc::now());
        let groups = c.groups("usage", &[]);
        assert_eq!(groups.iter().map(|g| g.title).collect::<Vec<_>>(), ["Threads", "App"], "the groups keep their order");
        assert_eq!(picked(&groups, None).unwrap().title, "Usage");
        assert_eq!(picked(&groups, None).unwrap().title, picked(&c.groups("usa", &[]), None).unwrap().title, "the start of its name as well");
        // A Settings section by its own name.
        let mut keys = thread("k", "Tidy the imports", "/w/misc", 10);
        keys.keywords = Some("keyboard".into());
        let c = Catalog::new([&keys], &[], pages(), &tabs(), &Utc::now());
        assert_eq!(picked(&c.groups("keyboard", &[]), None).unwrap().title, "Settings › Keyboard");
        // The arrows still go from the top, through the thread.
        let groups = c.groups("keyboard", &[]);
        assert_eq!(step(&groups, None, false).unwrap(), thread_key("k"));
        // A thread named for it stays first, as it's first in the box.
        let named = thread("u", "Usage graph looks off", "/w/misc", 10);
        let c = Catalog::new([&named], &[], pages(), &tabs(), &Utc::now());
        assert_eq!(picked(&c.groups("usage g", &[]), None).unwrap().title, "Usage graph looks off");
        assert_eq!(picked(&c.groups("usage", &[]), None).unwrap().title, "Usage", "but a name that's all of it wins");
        assert_eq!(picked(&c.groups("usag", &[]), None).unwrap().title, "Usage graph looks off", "and between two starts, the first");
        // Nothing named for it: the first, as before.
        assert_eq!(picked(&c.groups("graph", &[]), None).unwrap().title, "Usage graph looks off");
    }

    #[test]
    fn the_arrows_move_through_every_group_and_stop_at_the_ends() {
        let c = Catalog::new([], &[], pages(), &[], &Utc::now());
        let groups = c.groups("", &[page_key(&Page::Usage)]);
        assert_eq!(picked(&groups, None).unwrap().title, "Usage", "the first to start with");
        let down = step(&groups, None, true).unwrap();
        assert_eq!(down, page_key(&Page::Inbox), "into Go to");
        assert_eq!(step(&groups, None, false).unwrap(), format!("recent|{}", page_key(&Page::Usage)), "stops at the top");
        assert_eq!(picked(&groups, None).unwrap().kept_key(), page_key(&Page::Usage), "Recent keeps it as itself");
        let last = flat(&groups).last().unwrap().key.clone();
        assert_eq!(step(&groups, Some(&last), true).unwrap(), last, "stops at the bottom");
        assert_eq!(picked(&groups, Some("page:gone")).unwrap().title, "Usage", "a pick that's gone falls back to the first");
    }
}
