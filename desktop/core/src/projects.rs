// Projects (docs/protocol.md, "Projects") with no UI in them, ported from the
// Swift app: the project itself and its icon and task board (Project.swift),
// what `projects.update` changes (ProjectUpdate in RunnerAPI.swift), the
// sidebar's sections with parts nested under their parent
// (ProjectSections.swift), the Projects page's cards (ProjectCards.swift),
// decisions (ProjectDecisions.swift), the folded rows (ProjectFolds.swift),
// what a project's page borrows from its threads (ProjectPane, PatchGroups),
// and the words the page and the cards say (ProjectPage.swift,
// ProjectsHome.swift). The GPUI app draws from these; its `dump` says what
// they say, so the specs compare both apps' answers.
//
// The sections borrow the projects and threads they're built from rather than
// copying them: the sidebar builds them on every draw, and Mike has over a
// thousand archived threads.
use crate::highlights::{text::address, Panel, PatchInfo};
use crate::inbox::{folder_name, parse_ms, Project as Membership, ThreadSummary};
use crate::sidebar::{relative_short, RowLook, RowStatus};
use chrono::{DateTime, Datelike, TimeZone};
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------- the project

/// Who made a project, put a thread in one or chose its icon
/// (`ProjectAuthor`). One from a newer runner reads as an agent, the middle
/// of the three: neither Mike's own choice nor the librarian's guess.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Author {
    Mike,
    #[default]
    Agent,
    Librarian,
}

impl Author {
    pub fn from_raw(raw: &str) -> Author {
        match raw {
            "mike" => Author::Mike,
            "librarian" => Author::Librarian,
            _ => Author::Agent,
        }
    }

    /// Missing altogether reads as the librarian's, as the Swift decoder has it.
    fn read(v: &Value) -> Author {
        v.as_str().map_or(Author::Librarian, Author::from_raw)
    }
}

/// A project's icon (`ProjectIcon`, protocol "Icons"): a picture copied into
/// the runner's data dir, else an SF Symbol. One neither can draw is shown as
/// the title's first letter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Icon {
    pub image_path: Option<String>,
    pub symbol: Option<String>,
    /// The file the picture was copied from.
    pub from: Option<String>,
    /// The librarian's is kept up to date by itself; Mike's or an agent's
    /// stays until it's set back to automatic.
    pub by: Author,
}

impl Icon {
    /// Chosen by Mike or an agent rather than found.
    pub fn is_chosen(&self) -> bool {
        self.by != Author::Librarian
    }

    fn decode(v: &Value) -> Option<Icon> {
        if !v.is_object() {
            return None;
        }
        Some(Icon {
            image_path: v["image"]["path"].as_str().filter(|p| !p.is_empty()).map(str::to_owned),
            symbol: v["symbol"].as_str().filter(|s| !s.is_empty()).map(str::to_owned),
            from: v["from"].as_str().map(str::to_owned),
            by: Author::read(&v["by"]),
        })
    }
}

/// A place Mike keeps going back to for a project (`ProjectLink`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Link {
    pub title: String,
    pub url: String,
}

impl Link {
    /// One without a title is named by its address; one without a url is no link.
    fn decode(v: &Value) -> Option<Link> {
        let url = v["url"].as_str()?.to_owned();
        let title = v["title"].as_str().filter(|t| !t.is_empty()).map_or_else(|| address(&url), str::to_owned);
        Some(Link { title, url })
    }
}

/// Notion's group for a Status option (`TaskColumn.Kind`); an unknown one
/// reads as to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TaskKind {
    #[default]
    Todo,
    Doing,
    Done,
}

/// A column of a project's task board (`TaskColumn`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskColumn {
    pub name: String,
    pub kind: TaskKind,
    /// Notion's colour name: gray, blue, orange, green...
    pub color: Option<String>,
}

/// A card on the board (`ProjectTask`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub url: String,
    pub status: String,
    pub who: Vec<String>,
    pub due: Option<String>,
    pub due_end: Option<String>,
    /// A Threadbear thread id (or the start of one) working on it.
    pub thread: Option<String>,
}

impl Task {
    /// Its Thread column names this thread: the id or a prefix of it, six
    /// characters at least, so a stray word doesn't match.
    pub fn is_worked_by(&self, thread_id: &str) -> bool {
        let Some(mine) = self.thread.as_deref().map(|t| t.trim().to_lowercase()).filter(|t| t.chars().count() >= 6) else { return false };
        thread_id.to_lowercase().starts_with(&mine)
    }
}

/// A project's task board, read from the Notion database on its page
/// (`ProjectTasks`). Notion is where it's changed; the app only shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tasks {
    /// The database (or page) to open in Notion.
    pub url: String,
    pub columns: Vec<TaskColumn>,
    pub items: Vec<Task>,
    pub fetched_ms: Option<i64>,
    /// The first read hasn't come back yet.
    pub loading: bool,
    /// The last read failed; the columns and cards are the last good ones.
    pub error: Option<String>,
    /// Over 100 cards: only the first 100 are here.
    pub more: bool,
}

impl Tasks {
    fn decode(v: &Value) -> Option<Tasks> {
        if !v.is_object() {
            return None;
        }
        let some = |key: &str| v[key].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        Some(Tasks {
            url: v["url"].as_str().unwrap_or("").to_owned(),
            columns: v["columns"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| {
                    Some(TaskColumn {
                        name: c["name"].as_str()?.to_owned(),
                        kind: match c["kind"].as_str() {
                            Some("doing") => TaskKind::Doing,
                            Some("done") => TaskKind::Done,
                            _ => TaskKind::Todo,
                        },
                        color: c["color"].as_str().map(str::to_owned),
                    })
                })
                .collect(),
            items: v["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|i| {
                    let some = |key: &str| i[key].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
                    Some(Task {
                        id: i["id"].as_str()?.to_owned(),
                        title: some("title").unwrap_or_else(|| "Untitled".to_owned()),
                        url: some("url").unwrap_or_default(),
                        status: some("status").unwrap_or_default(),
                        who: strings(&i["who"]),
                        due: some("due"),
                        due_end: some("dueEnd"),
                        thread: some("thread"),
                    })
                })
                .collect(),
            fetched_ms: v["fetchedAt"].as_str().and_then(parse_ms),
            loading: v["loading"].as_bool().unwrap_or(false),
            error: some("error"),
            more: v["more"].as_bool().unwrap_or(false),
        })
    }

    /// The cards in a column, in Notion's order.
    pub fn items_in<'a>(&'a self, column: &TaskColumn) -> Vec<&'a Task> {
        self.items.iter().filter(|t| t.status == column.name).collect()
    }

    /// Cards not in a Done column.
    pub fn open_count(&self) -> usize {
        self.items.iter().filter(|item| self.columns.iter().find(|c| c.name == item.status).is_none_or(|c| c.kind != TaskKind::Done)).count()
    }
}

/// A grouping above folders that carries goals, state and notes forward
/// (`Project`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    pub id: String,
    pub title: String,
    /// Absolute paths, in the order patches load from them.
    pub folders: Vec<String>,
    pub goals: String,
    pub state: String,
    pub notes: String,
    pub links: Vec<Link>,
    pub created_by: Author,
    /// The thread whose agent or look made it.
    pub created_by_thread: Option<String>,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub state_updated_ms: Option<i64>,
    pub archived: bool,
    /// None until the librarian has found or picked one.
    pub icon: Option<Icon>,
    /// Threads in it, put away ones included.
    pub thread_count: usize,
    pub last_activity_ms: Option<i64>,
    /// A Notion page (or database) with its task board on it, and the board
    /// as the runner last read it (protocol "Project tasks").
    pub notion: Option<String>,
    pub tasks: Option<Tasks>,
    /// The project it's a part of (protocol, Projects, "Parts"); none for a
    /// project of its own and from a runner before parts.
    pub parent_id: Option<String>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_owned)).collect()
}

/// A count that may come as a number or as its digits.
fn lenient_count(v: &Value) -> Option<usize> {
    v.as_u64().map(|n| n as usize).or_else(|| v.as_f64().filter(|f| *f >= 0.).map(|f| f as usize)).or_else(|| v.as_str()?.trim().parse().ok())
}

impl Project {
    /// One project as the runner sends it, taking what it can: a field
    /// missing or of the wrong kind reads as nothing, and only an id is a
    /// must.
    pub fn decode(v: &Value) -> Option<Project> {
        let id = v["id"].as_str()?.to_owned();
        let text = |key: &str| v[key].as_str().unwrap_or("").to_owned();
        let some = |key: &str| v[key].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        let created_ms = v["createdAt"].as_str().and_then(parse_ms).unwrap_or(0);
        Some(Project {
            id,
            title: some("title").unwrap_or_else(|| "Untitled project".to_owned()),
            folders: strings(&v["folders"]),
            goals: text("goals"),
            state: text("state"),
            notes: text("notes"),
            links: v["links"].as_array().into_iter().flatten().filter_map(Link::decode).collect(),
            created_by: Author::read(&v["createdBy"]),
            created_by_thread: v["createdByThread"].as_str().map(str::to_owned),
            created_ms,
            updated_ms: v["updatedAt"].as_str().and_then(parse_ms).unwrap_or(created_ms),
            state_updated_ms: v["stateUpdatedAt"].as_str().and_then(parse_ms),
            archived: v["archived"].as_bool().unwrap_or(false),
            icon: Icon::decode(&v["icon"]),
            thread_count: lenient_count(&v["threadCount"]).unwrap_or(0),
            last_activity_ms: v["lastActivityAt"].as_str().and_then(parse_ms),
            notion: some("notion"),
            tasks: Tasks::decode(&v["tasks"]),
            parent_id: some("parentId"),
        })
    }

    /// `projects.list`'s (or `projects.updated`'s) list, one at a time so
    /// one that won't read can't lose the rest.
    pub fn list(v: &Value) -> Vec<Project> {
        v.as_array().into_iter().flatten().filter_map(Project::decode).collect()
    }

    /// The tile's letter when there's no icon to draw (`ProjectMark.letter`).
    pub fn letter(&self) -> String {
        crate::sidebar::project_letter(&self.title)
    }

    /// Everything a search looks through. Folders by name only: the path
    /// before it is the same for most projects, so "dev" would find them all.
    fn search_text(&self) -> String {
        let mut parts = vec![self.title.clone(), self.state.clone(), self.goals.clone(), self.notes.clone()];
        parts.extend(self.folders.iter().map(|f| folder_name(f)));
        parts.extend(self.links.iter().map(|l| l.title.clone()));
        parts.join("\n")
    }
}

// ---------------------------------------------------------------- changing one

/// Fields for `projects.update` (`ProjectUpdate`): what's given replaces
/// what was there, and what isn't is left out of the request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Update {
    pub title: Option<String>,
    pub folders: Option<Vec<String>>,
    pub goals: Option<String>,
    pub state: Option<String>,
    pub notes: Option<String>,
    pub archived: Option<bool>,
    /// A Notion page with the project's task board; empty takes it off.
    pub notion: Option<String>,
    /// The project it's to be a part of; empty makes it its own again.
    pub parent_id: Option<String>,
}

impl Update {
    /// The request's fields, with the project's id.
    pub fn params(&self, project_id: &str) -> Value {
        let mut params = json!({ "projectId": project_id });
        let put = |params: &mut Value, key: &str, value: Value| params[key] = value;
        if let Some(title) = &self.title {
            put(&mut params, "title", json!(title));
        }
        if let Some(folders) = &self.folders {
            put(&mut params, "folders", json!(folders));
        }
        if let Some(goals) = &self.goals {
            put(&mut params, "goals", json!(goals));
        }
        if let Some(state) = &self.state {
            put(&mut params, "state", json!(state));
        }
        if let Some(notes) = &self.notes {
            put(&mut params, "notes", json!(notes));
        }
        if let Some(archived) = self.archived {
            put(&mut params, "archived", json!(archived));
        }
        if let Some(notion) = &self.notion {
            put(&mut params, "notion", json!(notion));
        }
        if let Some(parent) = &self.parent_id {
            put(&mut params, "parentId", json!(parent));
        }
        params
    }

    /// The project as it will be once the runner has made this change, so a
    /// save shows straight away however slow the runner is; its reply
    /// replaces the guess. A different Notion page has a board nobody has
    /// read yet, so the old one goes. The runner keeps a title on an empty one.
    pub fn applied(&self, project: &Project, now_ms: i64) -> Project {
        let mut project = project.clone();
        if let Some(title) = self.title.as_deref().filter(|t| !t.is_empty()) {
            project.title = title.to_owned();
        }
        if let Some(folders) = &self.folders {
            project.folders = folders.clone();
        }
        if let Some(goals) = &self.goals {
            project.goals = goals.clone();
        }
        if let Some(state) = &self.state {
            project.state = state.clone();
            project.state_updated_ms = Some(now_ms);
        }
        if let Some(notes) = &self.notes {
            project.notes = notes.clone();
        }
        if let Some(archived) = self.archived {
            project.archived = archived;
        }
        if let Some(notion) = &self.notion {
            let link = (!notion.is_empty()).then(|| notion.clone());
            if link != project.notion {
                project.tasks = None;
            }
            project.notion = link;
        }
        if let Some(parent) = &self.parent_id {
            project.parent_id = (!parent.is_empty()).then(|| parent.clone());
        }
        project
    }
}

/// A thread's projects once Mike's add and remove have landed (`changing`):
/// what he adds is pinned and his, as the runner's `threads.setProjects`
/// does, so a click shows at once. `title` names a project being added; one
/// it doesn't know waits for the runner's reply.
pub fn changing(projects: &[Membership], add: &[String], remove: &[String], title: impl Fn(&str) -> Option<String>) -> Vec<Membership> {
    let mut out: Vec<Membership> = projects.iter().filter(|p| !remove.contains(&p.id)).cloned().collect();
    for id in add {
        if let Some(existing) = out.iter_mut().find(|p| &p.id == id) {
            existing.pinned = true;
        } else if let Some(title) = title(id) {
            out.push(Membership { id: id.clone(), title, by: "mike".to_owned(), pinned: true, why: None });
        }
    }
    out
}

/// The live projects a thread could be put in, by name (`projectsToAdd`).
pub fn projects_to_add<'a>(projects: &'a [Project], thread: &ThreadSummary) -> Vec<&'a Project> {
    let already: HashSet<&str> = thread.projects.iter().flatten().map(|m| m.id.as_str()).collect();
    let mut out: Vec<&Project> = projects.iter().filter(|p| !p.archived && !already.contains(p.id.as_str())).collect();
    out.sort_by(|a, b| standard_compare(&a.title, &b.title));
    out
}

/// The live projects, by name, for a thread's Projects ▸ menu.
pub fn live_by_title(projects: &[Project]) -> Vec<&Project> {
    let mut out: Vec<&Project> = projects.iter().filter(|p| !p.archived).collect();
    out.sort_by(|a, b| standard_compare(&a.title, &b.title));
    out
}

/// What a project could be made a part of (the project menu's Make a Part
/// of), by name. One level only: none for a project with parts of its own,
/// and only projects that aren't parts themselves.
pub fn possible_parents<'a>(projects: &'a [Project], project: &Project) -> Vec<&'a Project> {
    if project.parent_id.is_some() || projects.iter().any(|p| p.parent_id.as_deref() == Some(project.id.as_str())) {
        return vec![];
    }
    let mut out: Vec<&Project> = projects.iter().filter(|p| p.id != project.id && !p.archived && p.parent_id.is_none()).collect();
    out.sort_by(|a, b| standard_compare(&a.title, &b.title));
    out
}

// ---------------------------------------------------------------- comparing words

/// `localizedStandardCompare`, near enough: case aside, and a run of digits
/// by its number, so "Poddle 2" comes before "Poddle 10".
pub fn standard_compare(a: &str, b: &str) -> Ordering {
    let (a, b) = (fold(a), fold(b));
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut n = String::new();
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    n.push(c);
                    x.next();
                }
                let mut m = String::new();
                while let Some(d) = y.peek().copied().filter(char::is_ascii_digit) {
                    m.push(d);
                    y.next();
                }
                let (n, m) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let order = n.len().cmp(&m.len()).then_with(|| n.cmp(m));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Lower case with the accents off the Latin letters that have them, so
/// "cafe" finds "Café" as the Swift search's `.diacriticInsensitive` does.
/// Without a Unicode table in the build this covers the accented letters of
/// the European languages, which is what titles here have.
pub fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
            'ď' | 'đ' => 'd',
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
            'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
            'ĥ' | 'ħ' => 'h',
            'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
            'ĵ' => 'j',
            'ķ' => 'k',
            'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
            'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
            'ŕ' | 'ŗ' | 'ř' => 'r',
            'ś' | 'ŝ' | 'ş' | 'š' => 's',
            'ţ' | 'ť' | 'ŧ' => 't',
            'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
            'ŵ' => 'w',
            'ý' | 'ÿ' | 'ŷ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            other => other,
        })
        .collect()
}

// ---------------------------------------------------------------- the sidebar's sections

/// A project's part of the sidebar's Projects view: its heading and the
/// threads under it (`ProjectSection`).
#[derive(Clone, Debug)]
pub struct Section<'a> {
    pub project: &'a Project,
    /// Its threads in the inbox (not put away), newest activity first.
    pub threads: Vec<&'a ThreadSummary>,
    /// Its archived threads, newest activity first: folded away under the
    /// open ones in the sidebar, a few at a time.
    pub archived: Vec<&'a ThreadSummary>,
    /// Its newest thread's activity (put away ones too), else the runner's.
    pub last_activity_ms: i64,
}

impl Section<'_> {
    pub fn id(&self) -> &str {
        &self.project.id
    }
}

/// The Projects view (`ProjectSections`): a section per live project, newest
/// activity first, then the threads in no project. A part nests under its
/// parent (protocol, Projects, "Parts").
#[derive(Clone, Debug, Default)]
pub struct Sections<'a> {
    /// Every live project's section, parts too: for finding one by id and
    /// counting threads.
    pub sections: Vec<Section<'a>>,
    /// Inbox threads in no live project, newest first.
    pub unfiled: Vec<&'a ThreadSummary>,
    /// What the lists show at the top, as indexes into `sections`: projects
    /// of their own, and parts whose parent isn't shown. A parent goes by
    /// the newest activity in it or its parts.
    top: Vec<usize>,
    /// Each shown parent's parts, by its id, in title order.
    parts: HashMap<String, Vec<usize>>,
}

/// Newest activity first, then by id so the order never wobbles.
fn newest_first(a: &ThreadSummary, b: &ThreadSummary) -> Ordering {
    b.activity_ms().cmp(&a.activity_ms()).then_with(|| a.id.cmp(&b.id))
}

impl<'a> Sections<'a> {
    /// Archived projects are left out, and so are archived and hibernating
    /// threads, as in the inbox (archived ones go in `archived`). A thread in
    /// two projects is under both.
    pub fn build(projects: &'a [Project], threads: impl IntoIterator<Item = &'a ThreadSummary>, now_ms: i64) -> Sections<'a> {
        let shown: Vec<&Project> = projects.iter().filter(|p| !p.archived).collect();
        let shown_ids: HashSet<&str> = shown.iter().map(|p| p.id.as_str()).collect();
        let mut members: HashMap<&str, Vec<&ThreadSummary>> = HashMap::new();
        let mut archived: HashMap<&str, Vec<&ThreadSummary>> = HashMap::new();
        let mut newest: HashMap<&str, i64> = HashMap::new();
        let mut unfiled = Vec::new();
        for thread in threads {
            let inbox = !thread.is_archived() && !thread.snoozed(now_ms);
            let mut filed = false;
            for membership in thread.projects.iter().flatten() {
                let Some(id) = shown_ids.get(membership.id.as_str()).copied() else { continue };
                filed = true;
                let at = newest.entry(id).or_insert(i64::MIN);
                *at = (*at).max(thread.activity_ms());
                if inbox {
                    members.entry(id).or_default().push(thread);
                }
                if thread.is_archived() {
                    archived.entry(id).or_default().push(thread);
                }
            }
            if inbox && !filed {
                unfiled.push(thread);
            }
        }
        let mut sections: Vec<Section> = shown
            .into_iter()
            .map(|project| {
                let mut threads = members.remove(project.id.as_str()).unwrap_or_default();
                threads.sort_by(|a, b| newest_first(a, b));
                // A thread in a project twice (a summary a moment behind) is listed once.
                threads.dedup_by(|a, b| a.id == b.id);
                let mut put_away = archived.remove(project.id.as_str()).unwrap_or_default();
                put_away.sort_by(|a, b| newest_first(a, b));
                put_away.dedup_by(|a, b| a.id == b.id);
                let own = project.last_activity_ms.unwrap_or(project.updated_ms);
                let last_activity_ms = newest.get(project.id.as_str()).copied().unwrap_or(i64::MIN).max(own);
                Section { project, threads, archived: put_away, last_activity_ms }
            })
            .collect();
        sections.sort_by(|a, b| b.last_activity_ms.cmp(&a.last_activity_ms).then_with(|| standard_compare(&a.project.title, &b.project.title)));
        unfiled.sort_by(|a, b| newest_first(a, b));
        Sections::with_parts(sections, unfiled)
    }

    /// Works out which sections are parts. A part nests only under a shown
    /// parent that isn't a part itself, so whatever comes in (a cycle, three
    /// levels, a parent that's gone) nothing is hidden.
    fn with_parts(sections: Vec<Section<'a>>, unfiled: Vec<&'a ThreadSummary>) -> Sections<'a> {
        let mut parent_of: HashMap<&str, Option<&str>> = HashMap::new();
        for s in &sections {
            parent_of.entry(s.project.id.as_str()).or_insert(s.project.parent_id.as_deref());
        }
        let is_part = |s: &Section| s.project.parent_id.as_deref().is_some_and(|p| p != s.project.id && parent_of.get(p) == Some(&None));
        let mut parts: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, s) in sections.iter().enumerate() {
            if is_part(s) {
                parts.entry(s.project.parent_id.clone().unwrap_or_default()).or_default().push(i);
            }
        }
        for list in parts.values_mut() {
            list.sort_by(|a, b| standard_compare(&sections[*a].project.title, &sections[*b].project.title));
        }
        let newest = |i: usize| parts.get(&sections[i].project.id).into_iter().flatten().map(|p| sections[*p].last_activity_ms).fold(sections[i].last_activity_ms, i64::max);
        let mut top: Vec<usize> = (0..sections.len()).filter(|i| !is_part(&sections[*i])).collect();
        // Stable, so ties keep the sections' own order.
        top.sort_by_key(|i| std::cmp::Reverse(newest(*i)));
        Sections { sections, unfiled, top, parts }
    }

    /// What the lists and the Projects page show at the top, in order.
    pub fn top_level(&self) -> Vec<&Section<'a>> {
        self.top.iter().map(|i| &self.sections[*i]).collect()
    }

    /// A shown parent's parts, in title order; none for anything else.
    pub fn parts_of(&self, id: &str) -> Vec<&Section<'a>> {
        self.parts.get(id).into_iter().flatten().map(|i| &self.sections[*i]).collect()
    }

    pub fn section(&self, id: &str) -> Option<&Section<'a>> {
        self.sections.iter().find(|s| s.project.id == id)
    }

    /// The project's threads and its parts', each once, newest first.
    pub fn threads_with_parts(&self, section: &Section<'a>) -> Vec<&'a ThreadSummary> {
        let mut seen = HashSet::new();
        let mut out: Vec<&ThreadSummary> = std::iter::once(section).chain(self.parts_of(section.id())).flat_map(|s| s.threads.iter().copied()).filter(|t| seen.insert(t.id.as_str())).collect();
        out.sort_by(|a, b| newest_first(a, b));
        out
    }

    /// Thread ids top to bottom, for the arrow keys: a parent, then its parts,
    /// then the threads in none. A thread in two projects is listed once, where
    /// it first comes.
    pub fn order(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let drawn = self.top_level().into_iter().flat_map(|s| std::iter::once(s).chain(self.parts_of(s.id())));
        drawn.flat_map(|s| s.threads.iter().copied()).chain(self.unfiled.iter().copied()).filter(|t| seen.insert(t.id.as_str())).map(|t| t.id.clone()).collect()
    }
}

/// A project's threads (any, put away ones too), newest activity first, for
/// its page.
pub fn threads_in<'a>(project_id: &str, threads: impl IntoIterator<Item = &'a ThreadSummary>) -> Vec<&'a ThreadSummary> {
    let mut out: Vec<&ThreadSummary> = threads.into_iter().filter(|t| t.projects.iter().flatten().any(|m| m.id == project_id)).collect();
    out.sort_by(|a, b| newest_first(a, b));
    out
}

// ---------------------------------------------------------------- folded rows

/// "Not in a project"'s id among the open rows (`ProjectFolds.unfiledId`).
pub const UNFILED_ID: &str = "-";

/// Thread ids top to bottom as the folded list shows them, for the arrow
/// keys: only open projects' threads, each once. A part's threads show when it
/// and its parent are both open.
pub fn fold_order(sections: &Sections, open: &HashSet<String>) -> Vec<String> {
    let mut shown: Vec<&ThreadSummary> = Vec::new();
    for top in sections.top_level() {
        if !open.contains(top.id()) {
            continue;
        }
        shown.extend(top.threads.iter().copied());
        for part in sections.parts_of(top.id()) {
            if open.contains(part.id()) {
                shown.extend(part.threads.iter().copied());
            }
        }
    }
    if open.contains(UNFILED_ID) {
        shown.extend(sections.unfiled.iter().copied());
    }
    let mut seen = HashSet::new();
    shown.into_iter().filter(|t| seen.insert(t.id.as_str())).map(|t| t.id.clone()).collect()
}

/// What the folded list draws, a row id each: a project's row as "p:<id>",
/// a thread as "<project>/<thread>" (`ProjectFolds.layout`).
pub fn fold_layout(sections: &Sections, open: &HashSet<String>) -> Vec<String> {
    let mut rows = Vec::new();
    for top in sections.top_level() {
        rows.push(format!("p:{}", top.id()));
        if !open.contains(top.id()) {
            continue;
        }
        rows.extend(top.threads.iter().map(|t| format!("{}/{}", top.id(), t.id)));
        for part in sections.parts_of(top.id()) {
            rows.push(format!("p:{}", part.id()));
            if open.contains(part.id()) {
                rows.extend(part.threads.iter().map(|t| format!("{}/{}", part.id(), t.id)));
            }
        }
    }
    if !sections.unfiled.is_empty() {
        rows.push(format!("p:{UNFILED_ID}"));
        if open.contains(UNFILED_ID) {
            rows.extend(sections.unfiled.iter().map(|t| format!("{UNFILED_ID}/{}", t.id)));
        }
    }
    rows
}

// ---------------------------------------------------------------- decisions

/// The thread is waiting on Mike with a card of asks he hasn't finished
/// (`ProjectDecisions.isWaiting`): its last turn said it needs him, nothing is
/// running, and some ask is unticked. One that needs him with no asks (a plain
/// reply) isn't a decision.
pub fn is_waiting(t: &ThreadSummary) -> bool {
    t.waiting_on_mike() && !t.is_archived() && t.asks_seq.is_some() && t.asks.iter().flatten().any(|a| a.ticked != Some(true))
}

/// How many of its asks are still open.
pub fn open_asks(t: &ThreadSummary) -> usize {
    if is_waiting(t) {
        t.asks.iter().flatten().filter(|a| a.ticked != Some(true)).count()
    } else {
        0
    }
}

/// The threads with decisions, newest activity first. Takes a section's
/// threads, which already leave out archived and hibernating ones.
pub fn decision_threads<'a>(threads: &[&'a ThreadSummary]) -> Vec<&'a ThreadSummary> {
    let mut out: Vec<&ThreadSummary> = threads.iter().copied().filter(|t| is_waiting(t)).collect();
    out.sort_by(|a, b| newest_first(a, b));
    out
}

/// Open asks across these threads.
pub fn decision_count(threads: &[&ThreadSummary]) -> usize {
    threads.iter().map(|t| open_asks(t)).sum()
}

/// "1 decision", "3 decisions".
pub fn decision_label(count: usize) -> String {
    if count == 1 { "1 decision".to_owned() } else { format!("{count} decisions") }
}

/// The section's line beside its heading: "4 decisions from 3 threads", or
/// just the count when each thread has one.
pub fn decision_heading(decisions: usize, threads: usize) -> String {
    if decisions == threads {
        decision_label(decisions)
    } else {
        format!("{} from {}", decision_label(decisions), if threads == 1 { "1 thread".to_owned() } else { format!("{threads} threads") })
    }
}

/// One line on what the thread is doing: its preview's first line.
pub fn status_line(t: &ThreadSummary) -> Option<String> {
    let line = t.preview.lines().next().unwrap_or("").trim();
    (!line.is_empty()).then(|| line.to_owned())
}

/// An answer sent from the page (for card `sent_for`) the thread hasn't
/// taken yet: once its turn starts it isn't waiting, and the card drops off.
pub fn is_sending(t: &ThreadSummary, sent_for: Option<u64>) -> bool {
    sent_for.is_some_and(|seq| t.asks_seq == Some(seq) && is_waiting(t))
}

/// A waiting thread's asks as its card has them (`decisionCard`), every one,
/// ticked ones too, for drawing on its project's page the way its composer
/// draws them. The summary carries what the card needs: the asks with their
/// options, what each does, and their links.
pub fn decision_asks(t: &ThreadSummary) -> Vec<crate::reducer::Ask> {
    t.asks
        .iter()
        .flatten()
        .filter_map(|a| {
            crate::reducer::Ask::decode(&json!({
                "action": a.action, "where": a.place, "reply": a.reply, "why": a.why, "options": a.options,
                "optionNotes": a.option_notes, "suggested": a.suggested, "links": a.links, "ticked": a.ticked,
            }))
        })
        .collect()
}

/// A decision as the dump says it: "Title: first ask | second ask", its open
/// asks in plain words, and "(sending)" while an answer's on its way.
pub fn decision_line(t: &ThreadSummary, sending: bool) -> String {
    let asks: Vec<String> = t.asks.iter().flatten().filter(|a| a.ticked != Some(true)).map(|a| crate::sidebar::preview_plain(&a.action)).collect();
    format!("{}: {}{}", t.title, asks.join(" | "), if sending { " (sending)" } else { "" })
}

// ---------------------------------------------------------------- the Projects page's cards

/// What a project's card says about its threads (`ProjectCardStatus`):
/// counted over the inbox threads its section lists, so the card and the
/// sidebar agree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CardStatus {
    pub working: usize,
    pub needs_you: usize,
    pub failed: usize,
    pub unread: usize,
    /// Open threads (not archived or hibernating).
    pub open: usize,
    /// Open asks on its threads' cards, only with the projectDecisions
    /// experiment on. A thread counted here isn't counted as new as well.
    pub decisions: usize,
}

/// One piece of a card's status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardPart {
    Decisions(usize),
    NeedsYou(usize),
    Working(usize),
    Failed(usize),
    Unread(usize),
}

impl CardPart {
    pub fn text(&self) -> String {
        match self {
            CardPart::Decisions(n) => decision_label(*n),
            CardPart::NeedsYou(n) => format!("{n} needs you"),
            CardPart::Working(n) => format!("{n} working"),
            CardPart::Failed(n) => format!("{n} failed"),
            CardPart::Unread(n) => format!("{n} new"),
        }
    }
}

impl CardStatus {
    pub fn of(threads: &[&ThreadSummary], decisions: bool) -> CardStatus {
        let mut status = CardStatus { open: threads.len(), ..Default::default() };
        for thread in threads {
            let asks = if decisions { open_asks(thread) } else { 0 };
            status.decisions += asks;
            let look = RowLook::of(thread, None);
            match look.status {
                // Sub-agents or jobs still going count as working: the
                // project isn't quiet.
                RowStatus::Working | RowStatus::Background => status.working += 1,
                RowStatus::NeedsYou => status.needs_you += 1,
                RowStatus::Failed => status.failed += 1,
                RowStatus::Time | RowStatus::Stopped => {}
            }
            // Its dot is an unread dot, but it's counted as needing him already.
            if look.unread && look.status != RowStatus::NeedsYou && asks == 0 {
                status.unread += 1;
            }
        }
        status
    }

    /// Nothing running, waiting or new.
    pub fn is_quiet(&self) -> bool {
        self.working == 0 && self.needs_you == 0 && self.failed == 0 && self.unread == 0 && self.decisions == 0
    }

    /// The status line, most pressing first: "1 needs you · 2 working".
    /// Empty when it's quiet; the card says when it was last active instead.
    pub fn parts(&self) -> Vec<CardPart> {
        let mut parts = Vec::new();
        if self.decisions > 0 {
            parts.push(CardPart::Decisions(self.decisions));
        }
        if self.needs_you > 0 {
            parts.push(CardPart::NeedsYou(self.needs_you));
        }
        if self.working > 0 {
            parts.push(CardPart::Working(self.working));
        }
        if self.failed > 0 {
            parts.push(CardPart::Failed(self.failed));
        }
        if self.unread > 0 {
            parts.push(CardPart::Unread(self.unread));
        }
        parts
    }

    /// The line as the dump says it: the parts, or "quiet".
    pub fn words(&self) -> String {
        if self.is_quiet() {
            "quiet".to_owned()
        } else {
            self.parts().iter().map(CardPart::text).collect::<Vec<_>>().join(" · ")
        }
    }
}

/// One project's card on the Projects page (`ProjectCard`): a card for each
/// project of its own, with its parts inside it.
#[derive(Clone, Debug)]
pub struct Card<'a> {
    pub project: &'a Project,
    pub status: CardStatus,
    pub last_activity_ms: i64,
    /// Its newest few open threads' titles.
    pub recent: Vec<String>,
    /// Its parts, shown inside its card; its status counts theirs.
    pub parts: Vec<&'a Project>,
}

/// The cards in the sidebar's order, newest activity first. `decisions`
/// counts each one's open asks (the projectDecisions experiment).
pub fn cards<'a>(sections: &Sections<'a>, recent: usize, decisions: bool) -> Vec<Card<'a>> {
    sections
        .top_level()
        .into_iter()
        .map(|section| {
            let parts = sections.parts_of(section.id());
            let threads = sections.threads_with_parts(section);
            Card {
                project: section.project,
                status: CardStatus::of(&threads, decisions),
                last_activity_ms: parts.iter().map(|p| p.last_activity_ms).fold(section.last_activity_ms, i64::max),
                recent: threads.iter().take(recent).map(|t| if t.title.is_empty() { "Untitled".to_owned() } else { t.title.clone() }).collect(),
                parts: parts.iter().map(|p| p.project).collect(),
            }
        })
        .collect()
}

/// Across every project, each thread once: one in two projects is on both
/// cards but is still one thread working.
pub fn overall(sections: &Sections, decisions: bool) -> CardStatus {
    let mut seen = HashSet::new();
    let threads: Vec<&ThreadSummary> = sections.sections.iter().flat_map(|s| s.threads.iter().copied()).filter(|t| seen.insert(t.id.as_str())).collect();
    CardStatus::of(&threads, decisions)
}

/// The cards a search keeps (`ProjectCard.matching`). Every word has to be
/// somewhere in the project, case and accents aside; goals and notes count,
/// as you often remember what a project was for and not its name, and so
/// does a part's name. That casts a wide net, so cards with every word in
/// the title go first, each group newest first.
pub fn matching<'a>(cards: Vec<Card<'a>>, query: &str) -> Vec<Card<'a>> {
    let words: Vec<String> = query.split_whitespace().map(fold).collect();
    if words.is_empty() {
        return cards;
    }
    let hit = |text: &str| {
        let text = fold(text);
        words.iter().all(|w| text.contains(w.as_str()))
    };
    let (mut in_title, mut rest) = (Vec::new(), Vec::new());
    for card in cards {
        let text = std::iter::once(card.project.search_text()).chain(card.parts.iter().map(|p| p.search_text())).collect::<Vec<_>>().join("\n");
        if !hit(&text) {
            continue;
        }
        if hit(&card.project.title) {
            in_title.push(card);
        } else {
            rest.push(card);
        }
    }
    in_title.extend(rest);
    in_title
}

/// The Projects page's count while its search has text.
pub fn match_count(shown: usize, total: usize) -> String {
    format!("{shown} of {total} {}", if total == 1 { "project" } else { "projects" })
}

/// "4 projects · 3 decisions · 2 working · 1 needs you", each thread counted
/// once (`ProjectsHome.summary`).
pub fn summary(cards: usize, overall: &CardStatus) -> String {
    let mut parts = vec![if cards == 1 { "1 project".to_owned() } else { format!("{cards} projects") }];
    if overall.decisions > 0 {
        parts.push(decision_label(overall.decisions));
    }
    if overall.working > 0 {
        parts.push(format!("{} working", overall.working));
    }
    if overall.needs_you > 0 {
        parts.push(format!("{} needs you", overall.needs_you));
    }
    parts.join(" · ")
}

/// The state is markdown-ish prose the librarian writes; a card wants its
/// opening sentences on one run, without the line breaks.
pub fn first_lines(text: &str) -> String {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// "1 open thread", "3 open threads": the card's corner count's tooltip.
pub fn open_threads_tip(count: usize) -> String {
    if count == 1 { "1 open thread".to_owned() } else { format!("{count} open threads") }
}

// ---------------------------------------------------------------- a project's page

/// "20 Oct": a day and a short month, in the time zone of `now`.
pub fn day_month<Tz: TimeZone>(at_ms: i64, now: &DateTime<Tz>) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let date = now.timezone().timestamp_millis_opt(at_ms).single().map(|d| (d.day(), d.month0())).unwrap_or((1, 0));
    format!("{} {}", date.0, MONTHS[date.1 as usize])
}

/// "Made by the librarian on 20 Oct · 5 threads · Archived"
/// (`ProjectPage.madeBy`).
pub fn made_by<Tz: TimeZone>(project: &Project, threads: usize, now: &DateTime<Tz>) -> String {
    let who = match project.created_by {
        Author::Mike => "Made by you",
        Author::Agent => "Made by an agent",
        Author::Librarian => "Made by the librarian",
    };
    let mut parts = vec![format!("{who} on {}", day_month(project.created_ms, now))];
    let count = threads.max(project.thread_count);
    parts.push(if count == 1 { "1 thread".to_owned() } else { format!("{count} threads") });
    if project.archived {
        parts.push("Archived".to_owned());
    }
    parts.join(" · ")
}

/// The big icon's tooltip: where it came from (`IconButton.help`).
pub fn icon_help(project: &Project, home: &str) -> String {
    let Some(icon) = &project.icon else { return "No icon yet: the librarian looks in the project's folders for one. Click to choose a picture.".to_owned() };
    let source = match (&icon.from, &icon.symbol) {
        (Some(from), _) => format!("from {}", crate::new_thread::abbreviate_home(from, home)),
        (None, Some(symbol)) => format!("the {symbol} symbol"),
        (None, None) => String::new(),
    };
    match icon.by {
        Author::Librarian => format!("The librarian found this ({source}) and keeps it up to date. Click to choose another."),
        Author::Mike => format!("You chose this ({source}). Click to change it."),
        Author::Agent => format!("An agent chose this ({source}). Click to change it."),
    }
}

/// Goals, state or notes: the page's three texts (`TextSection.Field`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextField {
    Goals,
    State,
    Notes,
}

impl TextField {
    pub const ALL: [TextField; 3] = [TextField::Goals, TextField::State, TextField::Notes];

    pub fn title(self) -> &'static str {
        match self {
            TextField::Goals => "Goals",
            TextField::State => "State",
            TextField::Notes => "Notes",
        }
    }

    /// What an empty one says it's for.
    pub fn empty(self) -> &'static str {
        match self {
            TextField::Goals => "What it's for and what done looks like.",
            TextField::State => "Where it's up to, what's next and what's blocked.",
            TextField::Notes => "Decisions, gotchas, commands and links worth keeping.",
        }
    }

    pub fn text(self, project: &Project) -> &str {
        match self {
            TextField::Goals => &project.goals,
            TextField::State => &project.state,
            TextField::Notes => &project.notes,
        }
    }

    /// Saving `text` in this field.
    pub fn update(self, text: String) -> Update {
        match self {
            TextField::Goals => Update { goals: Some(text), ..Default::default() },
            TextField::State => Update { state: Some(text), ..Default::default() },
            TextField::Notes => Update { notes: Some(text), ..Default::default() },
        }
    }

    /// Beside the heading: when the state was last written ("updated 5m ago").
    pub fn detail<Tz: TimeZone>(self, project: &Project, now: &DateTime<Tz>) -> Option<String> {
        let at = project.state_updated_ms.filter(|_| self == TextField::State)?;
        Some(format!("updated {}", ago(at, now)))
    }
}

/// "just now", "5m ago": `RelativeTime.short` read as how long ago.
pub fn ago<Tz: TimeZone>(at_ms: i64, now: &DateTime<Tz>) -> String {
    match relative_short(at_ms, now) {
        short if short == "now" => "just now".to_owned(),
        short => format!("{short} ago"),
    }
}

/// A thread row's right-hand words on the page: "Archived · 3d", or just when
/// it was last active (`ProjectThreadRow.meta`).
pub fn thread_meta<Tz: TimeZone>(t: &ThreadSummary, now: &DateTime<Tz>) -> String {
    let when = relative_short(t.activity_ms(), now);
    if t.is_archived() {
        format!("Archived · {when}")
    } else if t.snoozed(now.timestamp_millis()) {
        format!("Hibernating · {when}")
    } else {
        when
    }
}

/// Who put a thread in the project, for its row's tooltip.
pub fn put_here_by(m: &Membership) -> &'static str {
    match Author::from_raw(&m.by) {
        Author::Mike => "You put it here",
        Author::Agent if m.pinned => "Its agent put it here",
        Author::Agent => "An agent put it here",
        Author::Librarian if m.pinned => "The librarian put it here and you kept it",
        Author::Librarian => "The librarian put it here",
    }
}

/// "8 open · read 2m ago", beside the Tasks heading; none while it loads.
pub fn tasks_detail<Tz: TimeZone>(tasks: &Tasks, now: &DateTime<Tz>) -> Option<String> {
    if tasks.loading {
        return None;
    }
    let mut parts = vec![format!("{} open", tasks.open_count())];
    if let Some(at) = tasks.fetched_ms {
        parts.push(format!("read {}", ago(at, now)));
    }
    Some(parts.join(" · "))
}

/// A card's dates: "29 Nov", or "29 Nov to 4 Dec" (`TaskCard.due`).
pub fn task_due(task: &Task) -> Option<String> {
    let day = |s: &str| -> Option<String> {
        const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        let date = s.get(..10)?;
        let month: usize = date.get(5..7)?.parse().ok()?;
        let day: u32 = date.get(8..10)?.parse().ok()?;
        (1..=12).contains(&month).then(|| format!("{day} {}", MONTHS[month - 1]))
    };
    let start = task.due.as_deref().and_then(day)?;
    Some(match task.due_end.as_deref().and_then(day) {
        Some(end) => format!("{start} to {end}"),
        None => start,
    })
}

/// A part's row on its parent's page: its state's first line.
pub fn part_line(project: &Project) -> Option<String> {
    project.state.lines().next().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned)
}

/// "1 thread", "4 threads".
pub fn thread_count_words(count: usize) -> String {
    if count == 1 { "1 thread".to_owned() } else { format!("{count} threads") }
}

/// The Done column shows its newest few; the rest are a click away in Notion.
pub const DONE_SHOWN: usize = 3;

// ---------------------------------------------------------------- what a page borrows from its threads

/// `path` is `folder` or inside it, as the runner's isInside (trailing
/// slashes aside).
pub fn is_inside(path: &str, folder: &str) -> bool {
    let trim = |p: &str| -> String {
        let t = p.trim_end_matches('/');
        if t.is_empty() { "/".to_owned() } else { t.to_owned() }
    };
    let (p, f) = (trim(path), trim(folder));
    p == f || p.starts_with(&if f.ends_with('/') { f.clone() } else { format!("{f}/") })
}

/// What a thread has from the project (`ProjectPane.patches`): patches
/// tagged with it, and the thread's own ones from inside one of its folders,
/// which the runner doesn't tag. Home patches go everywhere, so they aren't
/// the project's.
pub fn project_patches(project: &Project, thread: &ThreadSummary) -> Vec<PatchInfo> {
    thread
        .patches()
        .into_iter()
        .filter(|patch| {
            if let Some(id) = &patch.project_id {
                return *id == project.id;
            }
            if patch.source == "home" {
                return false;
            }
            // A built-in from the thread's own folder says no folder.
            let from = patch.folder.as_deref().or(patch.file.as_deref()).unwrap_or(&thread.cwd);
            !from.is_empty() && project.folders.iter().any(|f| is_inside(from, f))
        })
        .collect()
}

/// The panels those patches give (`ProjectPane.panels`).
pub fn project_panels(project: &Project, thread: &ThreadSummary) -> Vec<Panel> {
    let own: HashSet<String> = project_patches(project, thread).into_iter().filter(|p| p.project_id.is_none()).map(|p| p.id).collect();
    thread.panels().into_iter().filter(|p| p.project_id.as_deref() == Some(project.id.as_str()) || (p.project_id.is_none() && own.contains(&p.patch))).collect()
}

/// The thread a project's page borrows its panels from (`ProjectPane.standIn`):
/// the newest open one that has anything from the project, else the newest
/// archived one. None when none of its threads has anything from it.
pub fn stand_in<'a>(project: &Project, threads: impl IntoIterator<Item = &'a ThreadSummary>) -> Option<&'a ThreadSummary> {
    let better = |a: &ThreadSummary, b: &ThreadSummary| {
        if a.is_archived() != b.is_archived() {
            return !a.is_archived();
        }
        newest_first(a, b) == Ordering::Less
    };
    let mut best: Option<&ThreadSummary> = None;
    for thread in threads {
        if !thread.projects.iter().flatten().any(|m| m.id == project.id) {
            continue;
        }
        if best.is_some_and(|b| !better(thread, b)) {
            continue;
        }
        if project_patches(project, thread).is_empty() {
            continue;
        }
        best = Some(thread);
    }
    best
}

/// The project's patches across its threads, each once by id and folder,
/// with the thread each came from (`PatchGroups.patches(of:in:)`).
pub fn patches_across<'a>(project: &Project, threads: &[&'a ThreadSummary]) -> Vec<(PatchInfo, &'a ThreadSummary)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for thread in threads {
        for patch in project_patches(project, thread) {
            if seen.insert(format!("{}|{}", patch.id, patch.folder.as_deref().unwrap_or(""))) {
                out.push((patch, *thread));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::{iso_time, OpenAsk};
    use chrono::FixedOffset;

    const NOW: i64 = 1_000_000_000;

    fn minutes_ago(m: f64) -> String {
        iso_time(NOW - (m * 60_000.) as i64)
    }

    fn project(id: &str, title: &str) -> Project {
        Project { id: id.into(), title: title.into(), updated_ms: NOW - 86_400_000, ..Default::default() }
    }

    fn member(id: &str) -> Membership {
        Membership { id: id.into(), title: id.to_uppercase(), by: "librarian".into(), pinned: false, why: None }
    }

    fn thread(id: &str, minutes: f64, projects: &[&str]) -> ThreadSummary {
        ThreadSummary {
            id: id.into(),
            title: id.into(),
            status: "idle".into(),
            last_activity_at: minutes_ago(minutes),
            projects: (!projects.is_empty()).then(|| projects.iter().map(|p| member(p)).collect()),
            ..Default::default()
        }
    }

    fn asking(id: &str, minutes: f64, projects: &[&str], asks: usize) -> ThreadSummary {
        let mut t = thread(id, minutes, projects);
        t.needs_you = true;
        t.needs = Some("you".into());
        t.asks = Some((0..asks).map(|n| OpenAsk { action: format!("Ask {n}?"), options: Some(vec!["Yes".into(), "No".into()]), ..Default::default() }).collect());
        t.asks_seq = Some(7);
        t
    }

    fn ids(threads: &[&ThreadSummary]) -> Vec<String> {
        threads.iter().map(|t| t.id.clone()).collect()
    }

    #[test]
    fn a_project_reads_whole_and_tolerates_what_it_doesnt_know() {
        let p = Project::decode(&json!({
            "id": "p_3b1d9e20", "title": "Threadbear", "folders": ["/Users/mike/dev/me/threadbear", 5, "/Users/mike/dev/me/notes"],
            "goals": "Mike's own agent app.", "createdBy": "librarian", "createdByThread": "th1",
            "createdAt": "2026-10-20T10:00:00Z", "updatedAt": "2026-10-27T10:00:00Z", "threadCount": "7",
            "lastActivityAt": "2026-10-27T11:00:00Z", "colour": "teal", "parentId": "",
            "links": [{ "title": "Live site", "url": "https://cruise.example/" }, { "url": "https://www.cruise.example/admin/" }, { "title": "no url" }, "https://loose.example"],
        }))
        .unwrap();
        assert_eq!(p.folders, ["/Users/mike/dev/me/threadbear", "/Users/mike/dev/me/notes"], "a folder that isn't a string is skipped");
        assert_eq!(p.created_by, Author::Librarian);
        assert_eq!(p.thread_count, 7);
        assert_eq!(p.last_activity_ms, parse_ms("2026-10-27T11:00:00Z"));
        assert_eq!(p.parent_id, None, "an empty parent is none");
        assert_eq!(p.links, vec![Link { title: "Live site".into(), url: "https://cruise.example/".into() }, Link { title: "cruise.example/admin".into(), url: "https://www.cruise.example/admin/".into() }]);

        let bare = Project::decode(&json!({ "id": "p_1", "createdBy": "someone-new" })).unwrap();
        assert_eq!(bare.title, "Untitled project");
        assert_eq!(bare.created_by, Author::Agent, "an author from a newer runner reads as an agent");
        assert_eq!(bare.icon, None);
        assert_eq!(Project::list(&json!([{ "id": "a" }, { "title": "no id" }, 3])).len(), 1);
    }

    #[test]
    fn icons_and_task_boards_read_leniently() {
        let picture = Project::decode(&json!({ "id": "p", "icon": { "image": { "path": "/d/p-ab.png" }, "from": "/r/AppIcon.png", "by": "librarian" } })).unwrap();
        let icon = picture.icon.unwrap();
        assert_eq!(icon.image_path.as_deref(), Some("/d/p-ab.png"));
        assert!(!icon.is_chosen());
        let symbol = Project::decode(&json!({ "id": "p", "icon": { "symbol": "film", "by": "mike" } })).unwrap().icon.unwrap();
        assert!(symbol.is_chosen());
        assert_eq!(Project::decode(&json!({ "id": "p", "icon": "film" })).unwrap().icon, None, "an icon that isn't an object is dropped, not the project");

        let p = Project::decode(&json!({ "id": "p_1", "notion": "https://www.notion.so/abc", "tasks": {
            "url": "https://www.notion.so/db", "fetchedAt": "2026-10-30T05:00:00Z", "error": "",
            "columns": [{ "name": "To do", "kind": "todo", "color": "gray" }, { "name": "Blocked", "kind": "stuck" }, { "name": "Done", "kind": "done" }],
            "items": [{ "id": "t1", "title": "Eventbrite link", "status": "To do", "who": ["Zaran", 3], "due": "2026-11-29", "thread": "60f772ab" },
                      { "id": "t2", "status": "Done" }, { "title": "no id" }] } }))
        .unwrap();
        let tasks = p.tasks.unwrap();
        assert_eq!(tasks.columns.iter().map(|c| c.kind).collect::<Vec<_>>(), [TaskKind::Todo, TaskKind::Todo, TaskKind::Done], "an unknown kind reads as to do");
        assert_eq!(tasks.items.len(), 2, "a card without an id is skipped");
        assert_eq!(tasks.items[0].who, ["Zaran"]);
        assert_eq!(tasks.items[1].title, "Untitled");
        assert_eq!(tasks.error, None, "an empty error is none");
        assert_eq!(tasks.open_count(), 1);
        assert_eq!(tasks.items_in(&tasks.columns[2]).iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["t2"]);
        assert_eq!(task_due(&tasks.items[0]).as_deref(), Some("29 Nov"));
        let range = Task { due: Some("2026-11-29".into()), due_end: Some("2026-12-04T00:00:00Z".into()), ..Default::default() };
        assert_eq!(task_due(&range).as_deref(), Some("29 Nov to 4 Dec"));
        assert!(!Task { thread: Some("abc".into()), ..Default::default() }.is_worked_by("abcdef12"), "a short Thread value matches nothing");
        assert!(Task { thread: Some("85046057".into()), ..Default::default() }.is_worked_by("85046057-1df7"));
    }

    #[test]
    fn an_update_guesses_what_it_changes() {
        let base = Project { id: "p".into(), title: "T".into(), folders: vec!["/a".into()], notion: Some("https://n/1".into()), tasks: Some(Tasks { url: "https://n/1".into(), ..Default::default() }), ..Default::default() };
        assert_eq!(Update { goals: Some("G".into()), ..Default::default() }.applied(&base, 100).goals, "G");
        let state = Update { state: Some("S".into()), ..Default::default() }.applied(&base, 100);
        assert_eq!((state.state.as_str(), state.state_updated_ms), ("S", Some(100)));
        assert_eq!(Update { title: Some(String::new()), ..Default::default() }.applied(&base, 100).title, "T", "the runner keeps a title on an empty one");
        assert_eq!(Update::default().applied(&base, 100), base);
        let other = Update { notion: Some("https://n/2".into()), ..Default::default() }.applied(&base, 100);
        assert_eq!((other.notion.as_deref(), other.tasks.is_none()), (Some("https://n/2"), true), "a different page drops the old board");
        assert!(Update { notion: Some("https://n/1".into()), ..Default::default() }.applied(&base, 100).tasks.is_some(), "the same page keeps it");
        assert_eq!(Update { notion: Some(String::new()), ..Default::default() }.applied(&base, 100).notion, None);
        let part = Update { parent_id: Some("tb".into()), ..Default::default() }.applied(&base, 100);
        assert_eq!(part.parent_id.as_deref(), Some("tb"));
        assert_eq!(Update { parent_id: Some(String::new()), ..Default::default() }.applied(&part, 100).parent_id, None);
        assert_eq!(Update { title: Some("U".into()), ..Default::default() }.applied(&part, 100).parent_id.as_deref(), Some("tb"), "left out, it stays a part");
        let params = Update { folders: Some(vec!["/a".into()]), notes: Some(String::new()), ..Default::default() }.params("p");
        assert_eq!(params, json!({ "projectId": "p", "folders": ["/a"], "notes": "" }), "an emptied field is sent, so it's cleared");
    }

    #[test]
    fn changing_a_threads_projects_pins_whats_added() {
        let changed = changing(&[member("a"), member("b")], &["a".into(), "c".into(), "unknown".into()], &["b".into()], |id| (id == "c").then(|| "C".to_owned()));
        assert_eq!(changed.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["a", "c"], "one it can't name waits for the reply");
        assert!(changed.iter().all(|m| m.pinned));
        assert_eq!(changed[1].by, "mike");
    }

    #[test]
    fn sections_newest_first_with_a_thread_in_two_and_unfiled_last() {
        let mut quiet = project("quiet", "Quiet");
        quiet.updated_ms = NOW - 7 * 86_400_000;
        let projects = vec![project("a", "A"), project("b", "B"), Project { archived: true, ..project("gone", "Gone") }, quiet];
        let mut t3 = thread("t3", 1., &["b"]);
        t3.archived_at = Some(iso_time(NOW));
        let mut t6 = thread("t6", 4., &["a"]);
        t6.snoozed_until = Some(iso_time(NOW + 3_600_000));
        let threads = vec![thread("t1", 30., &["a"]), thread("t2", 5., &["b", "a"]), t3, thread("t4", 2., &[]), thread("t5", 3., &["gone"]), t6];
        let built = Sections::build(&projects, &threads, NOW);
        assert_eq!(built.sections.iter().map(|s| s.id()).collect::<Vec<_>>(), ["b", "a", "quiet"], "b's archived thread is its newest activity; archived projects are left out");
        assert_eq!(ids(&built.sections[0].threads), ["t2"], "archived threads aren't listed");
        assert_eq!(ids(&built.sections[1].threads), ["t2", "t1"], "a thread in two projects is under both; hibernating ones aren't");
        assert_eq!(ids(&built.sections[0].archived), ["t3"]);
        assert_eq!(ids(&built.unfiled), ["t4", "t5"], "a thread only in an archived project counts as in none");
        assert_eq!(built.order(), ["t2", "t1", "t4", "t5"]);
        assert_eq!(ids(&threads_in("b", &threads)), ["t3", "t2"], "the page lists put away ones too");
    }

    fn parts_world() -> (Vec<Project>, Vec<ThreadSummary>) {
        let projects = vec![
            project("tb", "Threadbear"),
            Project { parent_id: Some("tb".into()), ..project("port", "GPUI port") },
            Project { parent_id: Some("tb".into()), ..project("android", "Android app") },
            project("pool", "Pool"),
            Project { parent_id: Some("gone".into()), ..project("orphan", "Orphan") },
        ];
        let threads = vec![thread("t-pool", 5., &["pool"]), asking("t-port", 1., &["port"], 1), thread("t-tb", 30., &["tb"]), asking("t-android", 40., &["android"], 1)];
        (projects, threads)
    }

    #[test]
    fn parts_nest_under_their_parent_which_goes_by_their_activity() {
        let (projects, threads) = parts_world();
        let s = Sections::build(&projects, &threads, NOW);
        assert_eq!(s.top_level().iter().map(|x| x.id()).collect::<Vec<_>>(), ["tb", "pool", "orphan"], "Threadbear first, by its part's newest thread; a part whose parent's gone stands alone");
        assert_eq!(s.parts_of("tb").iter().map(|x| x.id()).collect::<Vec<_>>(), ["android", "port"], "parts in title order");
        assert_eq!(s.sections.len(), 5);
        assert_eq!(ids(&s.threads_with_parts(s.top_level()[0])), ["t-port", "t-tb", "t-android"]);
        assert_eq!(s.order(), ["t-tb", "t-android", "t-port", "t-pool"], "the arrow keys go through a parent, then its parts");

        let open = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<HashSet<_>>();
        assert_eq!(fold_layout(&s, &open(&[])), ["p:tb", "p:pool", "p:orphan"], "a part folds away with its parent");
        assert_eq!(fold_layout(&s, &open(&["tb"])), ["p:tb", "tb/t-tb", "p:android", "p:port", "p:pool", "p:orphan"]);
        assert_eq!(fold_order(&s, &open(&["tb", "port"])), ["t-tb", "t-port"]);
        assert!(fold_order(&s, &open(&["port"])).is_empty(), "a part's threads only show with its parent open");

        let all = cards(&s, 3, true);
        assert_eq!(all.iter().map(|c| c.project.id.as_str()).collect::<Vec<_>>(), ["tb", "pool", "orphan"]);
        assert_eq!(all[0].parts.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Android app", "GPUI port"]);
        assert_eq!((all[0].status.decisions, all[0].status.open), (2, 3), "both parts' decisions");
        assert_eq!(matching(all, "GPUI").iter().map(|c| c.project.id.as_str()).collect::<Vec<_>>(), ["tb"], "a part's name finds its parent's card");
    }

    #[test]
    fn a_cycle_or_three_levels_hides_nothing() {
        let projects = vec![
            Project { parent_id: Some("b".into()), ..project("a", "A") },
            Project { parent_id: Some("a".into()), ..project("b", "B") },
            project("top", "Top"),
            Project { parent_id: Some("top".into()), ..project("mid", "Mid") },
            Project { parent_id: Some("mid".into()), ..project("low", "Low") },
        ];
        let s = Sections::build(&projects, std::iter::empty(), NOW);
        let mut top: Vec<&str> = s.top_level().iter().map(|x| x.id()).collect();
        top.sort();
        assert_eq!(top, ["a", "b", "low", "top"]);
        assert_eq!(s.parts_of("top").iter().map(|x| x.id()).collect::<Vec<_>>(), ["mid"]);
        assert_eq!(possible_parents(&projects, &projects[2]).len(), 0, "a project with a part can't become one");
        assert_eq!(possible_parents(&projects, &projects[3]).len(), 0, "a part can't be a part again");
    }

    #[test]
    fn folded_rows_show_only_open_projects_threads() {
        let projects = vec![project("a", "A"), project("b", "B")];
        let threads = vec![thread("t1", 1., &["a", "b"]), thread("t2", 2., &["b"]), thread("loose", 3., &[])];
        let s = Sections::build(&projects, &threads, NOW);
        let open = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<HashSet<_>>();
        assert!(fold_order(&s, &open(&[])).is_empty(), "folded by default: nothing to arrow through");
        assert_eq!(fold_layout(&s, &open(&[])), ["p:a", "p:b", "p:-"], "a row a project, and one for the rest");
        assert_eq!(fold_order(&s, &open(&["b", "a"])), ["t1", "t2"], "a thread in two open projects comes once");
        assert_eq!(fold_layout(&s, &open(&["b"])), ["p:a", "p:b", "b/t1", "b/t2", "p:-"]);
        assert_eq!(fold_order(&s, &open(&["-"])), ["loose"]);
    }

    #[test]
    fn cards_count_what_each_projects_threads_are_doing() {
        let mut quiet = project("quiet", "Quiet");
        quiet.updated_ms = NOW - 7 * 86_400_000;
        let projects = vec![project("a", "A"), quiet, Project { archived: true, ..project("gone", "Gone") }];
        let with = |id: &str, minutes: f64, status: &str| ThreadSummary { status: status.into(), ..thread(id, minutes, &["a"]) };
        let mut bg = with("bg", 2., "idle");
        bg.title = String::new();
        bg.background = Some(vec![json!({ "kind": "shell", "description": "npm test" })]);
        let wait = ThreadSummary { needs_you: true, ..with("wait", 3., "waiting") };
        let mut new = ThreadSummary { needs_you: true, ..with("new", 4., "idle") };
        new.title = "Finished".into();
        let mut away = with("away", 7., "waiting");
        away.archived_at = Some(iso_time(NOW));
        let threads = vec![with("run", 1., "running"), bg, wait, new, with("bad", 5., "error"), with("old", 6., "idle"), away];
        let s = Sections::build(&projects, &threads, NOW);
        let all = cards(&s, 3, false);
        assert_eq!(all.iter().map(|c| c.project.id.as_str()).collect::<Vec<_>>(), ["a", "quiet"], "archived projects get no card");
        assert_eq!(all[0].status, CardStatus { working: 2, needs_you: 1, failed: 1, unread: 1, open: 6, decisions: 0 }, "background work counts as working; archived threads aren't counted");
        assert_eq!(all[0].status.parts().iter().map(CardPart::text).collect::<Vec<_>>(), ["1 needs you", "2 working", "1 failed", "1 new"]);
        assert_eq!(all[0].recent, ["run", "Untitled", "wait"]);
        assert!(all[1].status.is_quiet());
        assert_eq!(all[1].status.words(), "quiet");

        let both = vec![ThreadSummary { status: "running".into(), ..thread("x", 1., &["a", "quiet"]) }];
        let s = Sections::build(&projects, &both, NOW);
        assert_eq!(cards(&s, 3, false).iter().map(|c| c.status.working).collect::<Vec<_>>(), [1, 1], "on both cards");
        assert_eq!(overall(&s, false).working, 1, "but one thread working");
        assert_eq!(summary(2, &overall(&s, false)), "2 projects · 1 working");
    }

    #[test]
    fn a_decision_is_a_thread_waiting_on_a_card_of_asks() {
        assert!(is_waiting(&asking("t", 1., &["a"], 1)));
        assert!(!is_waiting(&ThreadSummary { status: "running".into(), ..asking("t", 1., &["a"], 1) }), "running: not his move yet");
        assert!(!is_waiting(&asking("t", 1., &["a"], 0)), "needs him with no asks is a plain reply");
        assert!(!is_waiting(&ThreadSummary { asks_seq: None, ..asking("t", 1., &["a"], 1) }), "no card to answer");
        let mut ticked = asking("t", 1., &["a"], 1);
        ticked.asks.as_mut().unwrap()[0].ticked = Some(true);
        assert!(!is_waiting(&ticked), "every ask ticked hands it back");
        let mut two = asking("new", 1., &["a"], 3);
        two.asks.as_mut().unwrap()[2].ticked = Some(true);
        let old = asking("old", 60., &["a"], 1);
        let busy = ThreadSummary { status: "running".into(), ..asking("busy", 0., &["a"], 1) };
        let list = vec![&old, &two, &busy];
        assert_eq!(ids(&decision_threads(&list)), ["new", "old"]);
        assert_eq!(decision_count(&list), 3, "two open on one card, one on the other; the running one waits");
        assert_eq!(decision_heading(3, 2), "3 decisions from 2 threads");
        assert_eq!(decision_heading(2, 2), "2 decisions");
        assert!(is_sending(&old, Some(7)));
        assert!(!is_sending(&old, Some(3)), "an answer to an older card");
        assert!(!is_sending(&busy, Some(7)), "taken: its turn started");

        let projects = vec![project("a", "A")];
        let threads = vec![asking("ask", 1., &["a"], 1), ThreadSummary { needs_you: true, ..thread("new", 1., &["a"]) }];
        let s = Sections::build(&projects, &threads, NOW);
        assert_eq!(cards(&s, 3, false)[0].status.words(), "2 new", "the experiment off: as before");
        assert_eq!(cards(&s, 3, true)[0].status.words(), "1 decision · 1 new", "a thread with a decision isn't counted as new too");
        let mut t = asking("t", 1., &["a"], 1);
        t.preview = "Batching is in (#67).\nMore after".into();
        assert_eq!(status_line(&t).as_deref(), Some("Batching is in (#67)."));
    }

    #[test]
    fn a_decisions_card_has_what_its_composer_would_draw() {
        let t: ThreadSummary = serde_json::from_value(json!({
            "id": "t", "title": "Compare SQLite", "status": "idle", "needs": "you", "needsYou": true, "asksSeq": 7,
            "asks": [
                { "action": "Keep **plain files** for now?", "why": "Faster for every size.", "options": ["Keep files", "Move to SQLite"],
                  "optionNotes": ["Revisit at 5,000 threads", "About two days of work"], "suggested": 0, "links": [{ "title": "Numbers", "url": "https://example.com/n" }] },
                { "action": "Done already", "ticked": true },
            ],
        }))
        .unwrap();
        let asks = decision_asks(&t);
        assert_eq!(asks.len(), 2, "ticked ones too, as the card draws them");
        assert_eq!(asks[0].option_notes, ["Revisit at 5,000 threads", "About two days of work"]);
        assert_eq!((asks[0].suggested, asks[0].links.len(), asks[1].ticked), (Some(0), 1, true));
        assert_eq!(decision_line(&t, false), "Compare SQLite: Keep plain files for now?", "open asks only, in plain words");
        assert_eq!(decision_line(&t, true), "Compare SQLite: Keep plain files for now? (sending)");
    }

    #[test]
    fn searching_cards_looks_through_what_a_project_is_about() {
        let projects = vec![
            Project { state: "The deterrent speaks through a Pi".into(), ..project("state", "Sammy Cam") },
            Project { folders: vec!["/Users/mike/dev/me/mike-pod-site".into()], ..project("folder", "Podcast site") },
            Project { goals: "Sell coffee".into(), ..project("goals", "Café") },
            Project { notes: "Moored by the café".into(), ..project("notes", "Boat") },
            Project { links: vec![Link { title: "Admin".into(), url: "https://example.com/x".into() }], ..project("link", "Shop") },
            project("title", "The Deterrent"),
        ];
        let all: Vec<Card> = projects.iter().map(|p| Card { project: p, status: CardStatus::default(), last_activity_ms: 0, recent: vec![], parts: vec![] }).collect();
        let found = |q: &str| matching(all.clone(), q).iter().map(|c| c.project.id.clone()).collect::<Vec<_>>();
        assert_eq!(found("  \n").len(), 6, "nothing typed leaves every card");
        assert_eq!(found("deterrent"), ["title", "state"], "a title match goes above one in the state");
        assert_eq!(found("DETERRENT sammy"), ["state"], "every word has to match, in any field");
        assert_eq!(found("cafe"), ["goals", "notes"], "case and accents don't count");
        assert_eq!(found("mike-pod"), ["folder"]);
        assert!(found("users").is_empty(), "a folder's name counts, not the path every project shares");
        assert_eq!(found("admin"), ["link"]);
        assert_eq!(match_count(3, 76), "3 of 76 projects");
        assert_eq!(match_count(0, 1), "0 of 1 project");
        assert_eq!(first_lines("Line one.\n\n  Line two  "), "Line one. Line two");
    }

    #[test]
    fn titles_sort_as_finder_does() {
        let mut titles = vec!["poddle 10", "Poddle 2", "apple", "Walkies"];
        titles.sort_by(|a, b| standard_compare(a, b));
        assert_eq!(titles, ["apple", "Poddle 2", "poddle 10", "Walkies"]);
    }

    #[test]
    fn the_page_says_who_made_it_and_where_its_icon_came_from() {
        let tz = FixedOffset::east_opt(8 * 3600).unwrap();
        let now = tz.timestamp_millis_opt(NOW).unwrap();
        let created = parse_ms("2026-10-20T10:00:00Z").unwrap();
        let p = Project { created_ms: created, created_by: Author::Librarian, thread_count: 5, ..project("p", "P") };
        assert_eq!(made_by(&p, 2, &now), "Made by the librarian on 20 Oct · 5 threads", "the runner's count when it knows more");
        assert_eq!(made_by(&Project { archived: true, created_by: Author::Mike, thread_count: 0, ..p.clone() }, 1, &now), "Made by you on 20 Oct · 1 thread · Archived");
        assert_eq!(icon_help(&p, "/Users/mike"), "No icon yet: the librarian looks in the project's folders for one. Click to choose a picture.");
        let found = Project { icon: Some(Icon { from: Some("/Users/mike/dev/x/icon.png".into()), ..Default::default() }), ..p.clone() };
        assert_eq!(icon_help(&found, "/Users/mike"), "An agent chose this (from ~/dev/x/icon.png). Click to change it.");
        let symbol = Project { icon: Some(Icon { symbol: Some("film".into()), by: Author::Librarian, ..Default::default() }), ..p };
        assert_eq!(icon_help(&symbol, "/Users/mike"), "The librarian found this (the film symbol) and keeps it up to date. Click to choose another.");
        let kept = Membership { pinned: true, ..member("a") };
        assert_eq!(put_here_by(&kept), "The librarian put it here and you kept it");
        assert_eq!(put_here_by(&Membership { by: "mike".into(), ..member("a") }), "You put it here");
    }

    #[test]
    fn a_page_borrows_the_newest_open_threads_panels() {
        let project = Project { folders: vec!["/walkies".into()], ..project("p_w", "Walkies") };
        let in_w = |id: &str, cwd: &str, minutes: f64| ThreadSummary { cwd: cwd.into(), ..thread(id, minutes, &["p_w"]) };
        let mut misc = in_w("misc", "/misc", 10.);
        misc.patches = Some(vec![json!({ "id": "checks", "source": "project", "file": "/misc/.threadbear/patches/checks.ts" }), json!({ "id": "walkies:convex", "source": "builtin", "folder": "/walkies", "projectId": "p_w" })]);
        misc.panels = Some(vec![json!({ "id": "worktrees", "title": "Worktrees", "kind": "list", "patch": "checks" }), json!({ "id": "walkies:convex", "title": "Convex", "kind": "url", "patch": "walkies:convex", "projectId": "p_w" })]);
        let mut inside = in_w("inside", "/walkies-threads/x", 1.);
        inside.patches = Some(vec![json!({ "id": "convex", "source": "builtin" })]);
        let newer = in_w("bare", "/elsewhere", 0.);
        let mut away = in_w("away", "/walkies", 0.);
        away.archived_at = Some(iso_time(NOW));
        away.patches = Some(vec![json!({ "id": "xcode", "source": "builtin" })]);
        let threads = vec![misc.clone(), inside, newer.clone(), away.clone()];
        let pane = stand_in(&project, &threads).unwrap();
        assert_eq!(pane.id, "misc", "a worktree outside the folder isn't inside it; nothing, archived or not in the project lose");
        assert_eq!(project_patches(&project, pane).iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["walkies:convex"], "only what came from the project");
        assert_eq!(project_panels(&project, pane).iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["walkies:convex"]);
        assert_eq!(stand_in(&project, &[newer.clone(), away.clone()]).map(|t| t.id.as_str()), Some("away"), "an archived thread when no open one has any");
        assert!(stand_in(&project, &[newer]).is_none());
        assert!(is_inside("/walkies", "/walkies/"));
        assert!(!is_inside("/walkies2", "/walkies"));
        let found = patches_across(&project, &[&misc, &away]);
        assert_eq!(found.iter().map(|(p, t)| (p.id.as_str(), t.id.as_str())).collect::<Vec<_>>(), [("walkies:convex", "misc"), ("xcode", "away")]);
    }
}
