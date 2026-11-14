// The Highlights pane's logic with no UI in it: ports of the Swift app's
// Highlights, HighlightSections, HighlightFold, HighlightProjects, PreviewRules,
// FileIcon, HighlightText, PanelIcon, PanelItem.headings, ScheduleRows,
// StartedThreadRows and PatchGroups, with the protocol types they read
// (docs/protocol.md, "Highlights" and "Patches"). `desktop/gpui/src/highlights.rs`
// draws from these, and the tests are the Swift ones (HighlightsTests,
// StartedThreadRowsTests).
use crate::inbox::{parse_ms, Project, ThreadSummary};
use crate::sidebar;
use chrono::{DateTime, TimeZone};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

// ---------------------------------------------------------------- the protocol

/// What a `file`, `link`, `pr` or `commit` highlight is. An unknown kind is
/// kept, and shows as a link or a file if it has a url or a path, else not at
/// all (HighlightSections).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum HighlightKind {
    File,
    Link,
    Pr,
    Commit,
    #[default]
    Other,
}

impl HighlightKind {
    fn parse(raw: &str) -> HighlightKind {
        match raw {
            "file" => HighlightKind::File,
            "link" => HighlightKind::Link,
            "pr" => HighlightKind::Pr,
            "commit" => HighlightKind::Commit,
            _ => HighlightKind::Other,
        }
    }
}

/// How it turned up. "" for a word this app doesn't know.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct How(String);

impl How {
    pub fn pinned(&self) -> bool {
        self.0 == "pinned"
    }
    /// The word on a row: "Written", "Opened". Empty for one this app doesn't
    /// know, rather than a raw word it can't vouch for.
    pub fn word(&self) -> &'static str {
        match self.0.as_str() {
            "pinned" => "Pinned",
            "written" => "Written",
            "opened" => "Opened",
            "mentioned" => "Mentioned",
            "created" => "Created",
            "committed" => "Committed",
            _ => "",
        }
    }
}

/// One thing a thread made, shared or pinned for Mike (`Highlight`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Highlight {
    pub id: String,
    pub kind: HighlightKind,
    pub title: String,
    pub url: Option<String>,
    pub path: Option<String>,
    pub how: How,
    pub note: Option<String>,
    pub show: bool,
    pub sha: Option<String>,
    pub branch: Option<String>,
    pub pushed: bool,
    pub seq: i64,
    pub last_seq: i64,
    pub at_ms: Option<i64>,
}

/// A string field if it's there and a string: a runner of another version may
/// send something else in its place.
fn field(item: &Value, key: &str) -> Option<String> {
    item.get(key).and_then(Value::as_str).map(str::to_owned)
}

impl Highlight {
    /// Nil for an entry with no id, as a malformed one is simply left out.
    fn decode(item: &Value) -> Option<Highlight> {
        let id = field(item, "id")?;
        let path = field(item, "path");
        let url = field(item, "url");
        let title = field(item, "title").unwrap_or_else(|| path.as_deref().map(last_component).or(url.as_deref()).unwrap_or(&id).to_owned());
        let seq = item.get("seq").and_then(Value::as_i64).unwrap_or(0);
        Some(Highlight {
            kind: HighlightKind::parse(item.get("kind").and_then(Value::as_str).unwrap_or("")),
            how: How(field(item, "how").unwrap_or_else(|| "mentioned".into())),
            note: field(item, "note"),
            show: item.get("show").and_then(Value::as_bool).unwrap_or(false),
            sha: field(item, "sha"),
            branch: field(item, "branch"),
            pushed: item.get("pushed").and_then(Value::as_bool).unwrap_or(false),
            seq,
            last_seq: item.get("lastSeq").and_then(Value::as_i64).unwrap_or(seq),
            at_ms: field(item, "at").as_deref().and_then(parse_ms),
            id,
            title,
            url,
            path,
        })
    }

    pub fn is_pinned(&self) -> bool {
        self.how.pinned()
    }

    /// What it opens: a file by its path, else the link.
    pub fn is_commit(&self) -> bool {
        self.kind == HighlightKind::Commit
    }
}

/// What `threads.highlights` answers (`ThreadHighlights`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ThreadHighlights {
    pub highlights: Vec<Highlight>,
    pub edited: Vec<String>,
    pub brief: Option<String>,
}

impl ThreadHighlights {
    /// Tolerant, like the rest of the protocol: an answer it can't read at all
    /// is empty, an entry with no id is skipped.
    pub fn decode(value: &Value) -> ThreadHighlights {
        ThreadHighlights {
            highlights: value.get("highlights").and_then(Value::as_array).into_iter().flatten().filter_map(Highlight::decode).collect(),
            edited: value.get("edited").and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
            brief: value.get("brief").and_then(|b| b.get("text")).and_then(Value::as_str).map(str::to_owned),
        }
    }
}

/// A link on a project (`ProjectLink`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ProjectLink {
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub title: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub url: String,
}

/// One project from `projects.list`, as much of it as Highlights uses.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectInfo {
    #[serde(default, deserialize_with = "crate::wire::id")]
    pub id: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub title: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub archived: bool,
    #[serde(default, deserialize_with = "crate::wire::list")]
    pub links: Vec<ProjectLink>,
    /// The Notion board, as it came (`ProjectTasks`). The heading reads its
    /// open count; the board decodes its columns and cards through projects.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub tasks: Option<Value>,
    /// A picture or a symbol, as it came (`ProjectIcon`).
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub icon: Option<Value>,
}

impl ProjectInfo {
    /// `projects.list`'s answer, one project at a time so one that won't read
    /// can't lose the rest.
    pub fn list(value: &Value) -> Vec<ProjectInfo> {
        value.as_array().into_iter().flatten().filter_map(|p| crate::wire::read(p.clone())).collect()
    }

    /// `ProjectTasks.openCount`: cards in a column that isn't a Done one.
    pub fn open_task_count(&self) -> usize {
        let Some(tasks) = self.tasks.as_ref().filter(|t| t.get("loading").and_then(Value::as_bool) != Some(true)) else { return 0 };
        let done: HashSet<&str> = tasks
            .get("columns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|c| c.get("kind").and_then(Value::as_str) == Some("done"))
            .filter_map(|c| c.get("name").and_then(Value::as_str))
            .collect();
        tasks.get("items").and_then(Value::as_array).into_iter().flatten().filter(|i| !i.get("status").and_then(Value::as_str).is_some_and(|s| done.contains(s))).count()
    }
}

// ---------------------------------------------------------------- HighlightSections

/// How many of a long group show before "Show all": a long thread can have
/// fifteen commits, which pushed Files and Links off screen.
pub const COMMIT_LIMIT: usize = 5;
pub const LINK_LIMIT: usize = 10;
pub const SUBAGENT_LIMIT: usize = 5;

/// What each folding heading is remembered as (`HighlightFold`). A fold holds
/// for every thread, so Sub-agents stays out of the way once Mike has put it
/// away (asked 3 Nov); a project's is its own, as only its threads show it.
pub mod fold {
    pub const NEEDS_YOU: &str = "needsYou";
    pub const SCHEDULED: &str = "scheduled";
    pub const SUBAGENTS: &str = "subagents";
    pub const STARTED_THREADS: &str = "startedThreads";
    pub const PINNED: &str = "pinned";
    pub const FILES: &str = "files";
    pub const LINKS: &str = "links";
    pub const CHANGES: &str = "changes";

    pub fn project(id: &str) -> String {
        format!("project:{id}")
    }
}

/// The Highlights section's groups, top to bottom (`HighlightSections`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct HighlightSections {
    pub brief: Option<String>,
    pub pinned: Vec<Highlight>,
    pub files: Vec<Highlight>,
    /// Links and pull requests.
    pub links: Vec<Highlight>,
    pub commits: Vec<Highlight>,
    /// Edited files not already listed as a pin or a file.
    pub edited: Vec<String>,
}

impl HighlightSections {
    pub fn build(highlights: &ThreadHighlights) -> HighlightSections {
        let mut out = HighlightSections::default();
        let brief = highlights.brief.as_deref().unwrap_or("").trim();
        out.brief = (!brief.is_empty()).then(|| brief.to_owned());
        for item in &highlights.highlights {
            if item.is_pinned() {
                out.pinned.push(item.clone());
                continue;
            }
            match item.kind {
                HighlightKind::File => out.files.push(item.clone()),
                HighlightKind::Link | HighlightKind::Pr => out.links.push(item.clone()),
                HighlightKind::Commit => out.commits.push(item.clone()),
                // A kind from a newer runner: as a file or a link if it's one,
                // else nothing this app knows how to open.
                HighlightKind::Other => {
                    if item.path.is_some() {
                        out.files.push(item.clone());
                    } else if item.url.is_some() {
                        out.links.push(item.clone());
                    }
                }
            }
        }
        let listed: HashSet<&str> = out.pinned.iter().chain(&out.files).filter_map(|h| h.path.as_deref()).collect();
        let mut seen: HashSet<&str> = HashSet::new();
        out.edited = highlights.edited.iter().filter(|p| !listed.contains(p.as_str()) && seen.insert(p.as_str())).cloned().collect();
        out
    }

    pub fn is_empty(&self) -> bool {
        self.brief.is_none() && self.pinned.is_empty() && self.files.is_empty() && self.links.is_empty() && self.commits.is_empty() && self.edited.is_empty()
    }

    /// The first `limit` of a group (they come newest first) and how many more
    /// "Show all" would add. All of them once Mike has asked.
    pub fn capped<T>(items: &[T], limit: usize, show_all: bool) -> (&[T], usize) {
        if show_all || items.len() <= limit {
            (items, 0)
        } else {
            (&items[..limit], items.len() - limit)
        }
    }

    /// The brief's bullets: the librarian writes "- " lines (Mike found a
    /// paragraph too long to glance at, 28 Oct). An older paragraph brief
    /// comes back as one point.
    pub fn points(brief: &str) -> Vec<String> {
        brief
            .lines()
            .map(|line| {
                let text = line.trim();
                let stripped = text.strip_prefix(['-', '•', '*']).unwrap_or(text);
                stripped.trim().to_owned()
            })
            .filter(|l| !l.is_empty())
            .collect()
    }
}

// ---------------------------------------------------------------- projects

/// The thread's projects at the top of Highlights (`HighlightProjects`): every
/// project the thread is in, links or not, so a wrong guess of the librarian's
/// shows (Mike asked 30 Oct, after a photo thread sat in the components
/// directory).
#[derive(Clone, Debug)]
pub struct ProjectRow {
    pub project: ProjectInfo,
    pub membership: Project,
}

pub fn project_rows(thread: &ThreadSummary, project: impl Fn(&str) -> Option<ProjectInfo>) -> Vec<ProjectRow> {
    let mut seen = HashSet::new();
    thread
        .projects
        .iter()
        .flatten()
        .filter_map(|member| {
            if !seen.insert(member.id.clone()) {
                return None;
            }
            let found = project(&member.id).filter(|p| !p.archived)?;
            Some(ProjectRow { project: found, membership: member.clone() })
        })
        .collect()
}

/// Who put it there, then its reason when it has one (`HighlightProjects.why`).
pub fn project_why(m: &Project) -> (String, Option<String>) {
    let who = match m.by.as_str() {
        "mike" => "You put it here.".to_owned(),
        "agent" => "Its agent put it here.".to_owned(),
        _ if m.pinned => "The librarian put it here, and you kept it.".to_owned(),
        _ => "The librarian put it here.".to_owned(),
    };
    (who, m.why.clone())
}

/// A guess of the librarian's nobody has kept yet: it can be kept.
pub fn can_keep(m: &Project) -> bool {
    m.by != "mike" && m.by != "agent" && !m.pinned
}

// ---------------------------------------------------------------- PreviewRules

/// Where a click on a highlight, a list panel's row or a file opens it: the
/// pane's own text view, a web view, or the picture and document viewer
/// (`PreviewRules`). Web pages open in the system browser for now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewer {
    /// An `.html` file: a web view. For now, it opens in the browser.
    Web,
    /// Markdown drawn, code coloured, plain text plain, all selectable.
    Text,
    /// Pictures here; PDFs and the like open in their app.
    Picture,
}

fn extension(path: &str) -> String {
    match last_component(path).rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_lowercase(),
        _ => String::new(),
    }
}

/// The name at the end of a path, ignoring a trailing slash (`lastPathComponent`).
pub fn last_component(path: &str) -> &str {
    let path = path.trim_end_matches('/');
    let split = if crate::reducer::text::is_windows_path(path) { path.rsplit_once(['/', '\\']) } else { path.rsplit_once('/') };
    match split {
        Some((_, name)) => name,
        None => path,
    }
}

/// `true` for an http or https address, the kind that can open in the browser
/// or the panel (`PreviewRules.isWebPage`).
pub fn is_web_page(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

pub fn is_markdown(path: &str) -> bool {
    matches!(extension(path).as_str(), "md" | "markdown" | "mdx")
}

const TEXT_EXTENSIONS: &[&str] = &["txt", "text", "csv", "tsv", "log", "env", "gitignore", "editorconfig", "ndjson", "jsonl"];

/// The highlight.js language for a code file, by extension or name
/// (`PreviewRules.codeLanguage`). The same names as gpui-kit's highlighter,
/// where it has one.
pub fn code_language(path: &str) -> Option<&'static str> {
    let name = last_component(path);
    match name {
        "Makefile" | "GNUmakefile" => return Some("makefile"),
        "Dockerfile" => return Some("dockerfile"),
        "Podfile" | "Gemfile" | "Fastfile" => return Some("ruby"),
        _ => {}
    }
    let ext = extension(path);
    Some(match ext.as_str() {
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "swift" => "swift",
        "py" => "python",
        "rb" => "ruby",
        "go" => "go",
        "rs" => "rust",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "c" | "h" => "c",
        "m" => "objectivec",
        "cc" | "cpp" | "hpp" => "cpp",
        "cs" => "csharp",
        "php" => "php",
        "sh" | "bash" | "zsh" => "bash",
        "json" | "jsonc" => "json",
        "yml" | "yaml" => "yaml",
        "toml" | "ini" => "ini",
        "xml" | "plist" | "svg" => "xml",
        "css" => "css",
        "scss" => "scss",
        "sql" => "sql",
        "graphql" => "graphql",
        "lua" => "lua",
        "diff" | "patch" => "diff",
        _ => return None,
    })
}

/// Which view shows a file by its name (`PreviewRules.viewer`).
pub fn viewer_for_path(path: &str) -> Viewer {
    let ext = extension(path);
    if ext == "html" || ext == "htm" {
        return Viewer::Web;
    }
    if is_markdown(path) || TEXT_EXTENSIONS.contains(&ext.as_str()) || code_language(path).is_some() {
        return Viewer::Text;
    }
    // A dotfile's name is all it has (`.env`, `.gitignore`).
    let bare = last_component(path).to_lowercase();
    if TEXT_EXTENSIONS.contains(&bare.trim_start_matches('.')) {
        return Viewer::Text;
    }
    Viewer::Picture
}

/// Whether the start of a file reads as text (`PreviewRules.looksLikeText`): no
/// NUL bytes, and UTF-8 (a character cut off at the end of the sample is fine).
pub fn looks_like_text(sample: &[u8]) -> bool {
    if sample.contains(&0) {
        return false;
    }
    if std::str::from_utf8(sample).is_ok() {
        return true;
    }
    (1..=3.min(sample.len())).any(|cut| std::str::from_utf8(&sample[..sample.len() - cut]).is_ok())
}

/// The kinds of file Mike reviews rather than edits: a transcript link to one
/// of these opens in the pane, not in its app (⌘-click still does that).
const REVIEW_EXTENSIONS: &[&str] = &["md", "html", "htm", "pdf", "png", "jpg", "jpeg", "gif", "webp", "svg", "mp4", "mov", "csv", "txt"];

pub fn previews_in_pane(path: &str) -> bool {
    REVIEW_EXTENSIONS.contains(&extension(path).as_str())
}

/// A picture the viewer may show: an image by its extension. Anything
/// else (a `.command` posing as one) never gets there (`LinkPolicy.runsCode`
/// and the viewer's own decode are the rest of the rule).
pub fn looks_like_picture(path: &str) -> bool {
    matches!(extension(path).as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "heic" | "heif" | "ico")
}

/// Extensions that run something when opened, which this app never hands to
/// its default app without asking (`LinkPolicy.codeExtensions`).
const CODE_EXTENSIONS: &[&str] = &["app", "command", "tool", "terminal", "workflow", "action", "scpt", "scptd", "pkg", "mpkg", "webloc", "inetloc", "fileloc", "url", "shortcut", "mobileconfig", "prefpane", "saver", "qlgenerator", "appex", "kext", "osax", "jar", "jnlp", "exe", "bat", "cmd", "msi", "ps1", "vbs", "desktop", "sh"];

pub fn runs_code(path: &str) -> bool {
    CODE_EXTENSIONS.contains(&extension(path).as_str())
}

/// Project files that mean nothing outside their app (`isAppProject`).
pub fn is_app_project(path: &str) -> bool {
    extension(path) == "tandem"
}

// ---------------------------------------------------------------- words and icons

/// An SF Symbol for a file, by its extension (`FileIcon`).
pub fn file_symbol(path: &str) -> &'static str {
    if path.ends_with('/') {
        return "folder";
    }
    match extension(path).as_str() {
        "md" | "markdown" | "txt" | "rtf" => "doc.text",
        "html" | "htm" => "safari",
        "pdf" => "doc.richtext",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "heic" | "tiff" => "photo",
        "mp4" | "mov" | "m4v" | "webm" => "film",
        "mp3" | "wav" | "m4a" | "aiff" => "waveform",
        "csv" | "tsv" | "xlsx" | "numbers" => "tablecells",
        "json" | "yaml" | "yml" | "toml" | "plist" => "curlybraces.square",
        "zip" | "gz" | "tar" | "dmg" => "archivebox",
        "" => "doc",
        _ => "chevron.left.forwardslash.chevron.right",
    }
}

/// A highlight's: a file's by its extension, the rest by kind.
pub fn highlight_symbol(item: &Highlight) -> &'static str {
    match item.kind {
        HighlightKind::Pr => "arrow.triangle.pull",
        HighlightKind::Commit => "smallcircle.filled.circle",
        HighlightKind::Link => "link",
        HighlightKind::File => file_symbol(item.path.as_deref().unwrap_or(&item.title)),
        HighlightKind::Other => item.path.as_deref().map_or("link", file_symbol),
    }
}

/// Short words for a highlight row (`HighlightText`).
pub mod text {
    use super::*;

    /// The folder a file is in, as a row's second line: inside the thread's
    /// folder relative to it ("docs/mockups"), the folder's own name for a
    /// file at its top; elsewhere the full path with `~`.
    pub fn folder(path: &str, cwd: Option<&str>, home: &str) -> String {
        // One separator to look for, whichever system the runner's on.
        use crate::reducer::text::forward_slashes;
        let (path, cwd, home) = (forward_slashes(path), cwd.map(forward_slashes), forward_slashes(home));
        let (path, cwd, home) = (path.as_str(), cwd.as_deref(), home.as_str());
        let parent = parent_path(path);
        if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
            let root = cwd.strip_suffix('/').unwrap_or(cwd);
            if parent == root {
                return last_component(root).to_owned();
            }
            if let Some(rest) = parent.strip_prefix(&format!("{root}/")) {
                return rest.to_owned();
            }
        }
        if !home.is_empty() && (parent == home || parent.starts_with(&format!("{home}/"))) {
            return format!("~{}", &parent[home.len()..]);
        }
        parent.to_owned()
    }

    fn parent_path(path: &str) -> &str {
        match path.rsplit_once('/') {
            Some(("", _)) => "/",
            Some((parent, _)) => parent,
            None => "",
        }
    }

    /// A link's second line: its host and path, without the scheme.
    pub fn address(url: &str) -> String {
        let mut text = url;
        for scheme in ["https://", "http://"] {
            if let Some(rest) = text.strip_prefix(scheme) {
                text = rest;
            }
        }
        let text = text.strip_prefix("www.").unwrap_or(text);
        text.strip_suffix('/').unwrap_or(text).to_owned()
    }

    /// "pushed" or "not pushed yet", after the branch.
    pub fn commit_line(commit: &Highlight) -> String {
        let state = if commit.pushed { "pushed" } else { "not pushed yet" };
        match commit.branch.as_deref().filter(|b| !b.is_empty()) {
            Some(branch) => format!("{branch} · {state}"),
            None => state.to_owned(),
        }
    }

    pub fn edited_count(count: usize) -> String {
        if count == 1 { "1 file edited".to_owned() } else { format!("{count} files edited") }
    }
}

/// The SF Symbol for a panel's button (`PanelIcon`).
pub fn panel_symbol(panel: &Panel) -> &str {
    if let Some(icon) = panel.icon.as_deref().filter(|i| !i.is_empty()) {
        return icon;
    }
    match panel.id.as_str() {
        "convex.dashboard" => return "cylinder.split.1x2",
        "xcode.simulator" => return "iphone",
        _ => {}
    }
    match panel.kind.as_str() {
        "image" => "photo",
        "list" => "list.bullet",
        _ => "globe",
    }
}

/// Where a panel or an action chip came from (`PanelOrigin`), for the small
/// badge on its rail button and its tab, and for the shelf its chip sits on:
/// two Convex buttons, one from the thread's folder and one from a project's,
/// look alike without it.
#[derive(Clone, Debug, PartialEq)]
pub enum PanelOrigin {
    /// One of the thread's projects' folders: the badge is that project's icon.
    Project(String),
    /// A tool patch (`appliesTo`), here for these paths.
    Tool(Vec<String>),
    /// `~/.threadbear/patches`.
    Home,
    /// A `.threadbear/patches` in the thread's folder or one above it.
    Folder,
    Builtin,
}

impl PanelOrigin {
    pub fn of_panel(panel: &Panel, patches: &[PatchInfo]) -> PanelOrigin {
        Self::of(&panel.patch, panel.project_id.as_deref(), patches)
    }

    pub fn of_action(action: &Action, patches: &[PatchInfo]) -> PanelOrigin {
        Self::of(&action.patch, action.project_id.as_deref(), patches)
    }

    /// A project's folder wins over the rest, as that's what Mike would least
    /// expect to see in another folder's thread; then a tool patch, which is
    /// there because of one file.
    fn of(patch: &str, project_id: Option<&str>, patches: &[PatchInfo]) -> PanelOrigin {
        let info = patches.iter().find(|p| p.id == patch);
        if let Some(id) = project_id.or_else(|| info.and_then(|p| p.project_id.as_deref())) {
            return PanelOrigin::Project(id.to_owned());
        }
        if let Some(matched) = info.and_then(|p| p.matched.as_ref()).filter(|m| !m.is_empty()) {
            return PanelOrigin::Tool(matched.clone());
        }
        match info.map(|p| p.source.as_str()) {
            Some("home") => PanelOrigin::Home,
            Some("builtin") => PanelOrigin::Builtin,
            _ => PanelOrigin::Folder,
        }
    }

    /// The badge's symbol; None for a project, whose own icon is drawn.
    pub fn symbol(&self) -> Option<&'static str> {
        match self {
            PanelOrigin::Project(_) => None,
            PanelOrigin::Tool(_) => Some("wrench.and.screwdriver.fill"),
            PanelOrigin::Home => Some("house.fill"),
            PanelOrigin::Folder => Some("folder.fill"),
            PanelOrigin::Builtin => Some("shippingbox.fill"),
        }
    }

    /// The tooltip: "Convex, from the Threadbear project".
    pub fn tip(&self, title: &str, project_title: Option<&str>) -> String {
        match self {
            PanelOrigin::Project(_) => format!("{title}, {}", project_title.map_or("from a project".to_owned(), |t| format!("from the {t} project"))),
            PanelOrigin::Tool(matched) => {
                let names: Vec<&str> = matched.iter().map(|m| m.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(m)).collect();
                format!("{title}, for {}", names.join(", "))
            }
            PanelOrigin::Home => format!("{title}, from ~/.threadbear"),
            PanelOrigin::Folder => format!("{title}, from this folder's .threadbear"),
            PanelOrigin::Builtin => format!("{title}, built in"),
        }
    }
}

/// A thread's action chips as shelves over its composer, one for each place
/// they came from (`ActionShelf`), so a project's buttons read as the
/// project's rather than as part of writing a message (Mike, 30 Oct). Each
/// shelf holds its chips by patch, so a hairline can sit between two patches
/// from the same place. Shelves and patches keep the order their first chip had.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionShelf {
    pub origin: PanelOrigin,
    pub patches: Vec<Vec<Action>>,
}

impl ActionShelf {
    pub fn shelves(actions: &[Action], patches: &[PatchInfo]) -> Vec<ActionShelf> {
        let mut shelves: Vec<ActionShelf> = Vec::new();
        for action in actions {
            let origin = PanelOrigin::of_action(action, patches);
            let index = match shelves.iter().position(|s| s.origin == origin) {
                Some(i) => i,
                None => {
                    shelves.push(ActionShelf { origin, patches: Vec::new() });
                    shelves.len() - 1
                }
            };
            let shelf = &mut shelves[index];
            match shelf.patches.iter_mut().find(|g| g.first().is_some_and(|a| a.patch == action.patch)) {
                Some(group) => group.push(action.clone()),
                None => shelf.patches.push(vec![action.clone()]),
            }
        }
        shelves
    }
}

/// An action chip's glyph when it has no icon of its own (`ActionChip.glyph`):
/// a bubble for a message, an arrow out for a page, the patch mark for a command.
pub fn action_glyph(action: &Action) -> &str {
    if let Some(icon) = action.icon.as_deref().filter(|i| !i.is_empty()) {
        return icon;
    }
    match action.kind.as_str() {
        "prompt" => "text.bubble",
        "url" => "arrow.up.right.square",
        _ => "square.dashed",
    }
}

// ---------------------------------------------------------------- sub-agents and started threads

/// A thread's sub-agents for Highlights: running ones first, then the rest
/// newest first (`SubagentRows.highlights`).
pub fn subagents_for_highlights(t: &ThreadSummary) -> Vec<sidebar::Subagent> {
    let mut newest = sidebar::subagents(t);
    newest.reverse();
    let (running, rest): (Vec<_>, Vec<_>) = newest.into_iter().partition(|s| s.running());
    running.into_iter().chain(rest).collect()
}

/// Where a started thread is up to, in the order the section lists them
/// (`StartedThreadRows.State`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StartedState {
    NeedsYou,
    Failed,
    Working,
    Hibernating,
    Done,
    Archived,
}

#[derive(Clone, Debug)]
pub struct StartedRow {
    pub thread: ThreadSummary,
    pub state: StartedState,
    pub started: bool,
}

fn started_by_thread(t: &ThreadSummary) -> Option<&str> {
    let by = t.started_by.as_ref()?;
    (by.get("kind").and_then(Value::as_str) == Some("thread")).then(|| by.get("threadId").and_then(Value::as_str)).flatten()
}

/// Every thread `parent` started, and any it's waiting on that it didn't,
/// newest first within each state (`StartedThreadRows.rows`).
pub fn started_rows<'a>(parent: &ThreadSummary, threads: impl Iterator<Item = &'a ThreadSummary>, now_ms: i64) -> Vec<StartedRow> {
    let waited: HashSet<&str> = parent.waiting_on.iter().flatten().filter_map(|w| w.get("threadId").and_then(Value::as_str)).collect();
    let mut rows: Vec<StartedRow> = threads
        .filter_map(|t| {
            if t.id == parent.id {
                return None;
            }
            let started = started_by_thread(t) == Some(parent.id.as_str());
            (started || waited.contains(t.id.as_str())).then(|| StartedRow { thread: t.clone(), state: started_state(t, now_ms), started })
        })
        .collect();
    // Keys worked out once per row: a time is text to read (inbox.rs, `sections_ordered`).
    rows.sort_by_cached_key(|r| (r.state.clone(), std::cmp::Reverse(r.thread.activity_ms())));
    rows
}

pub fn started_state(t: &ThreadSummary, now_ms: i64) -> StartedState {
    if t.is_archived() {
        StartedState::Archived
    } else if t.snoozed(now_ms) {
        StartedState::Hibernating
    } else if t.status == "waiting" || t.review_waiting() || t.waiting_on_mike() {
        StartedState::NeedsYou
    } else if t.status == "error" {
        StartedState::Failed
    } else if t.busy() || t.agents_still_working() || t.background.as_ref().is_some_and(|b| !b.is_empty()) {
        StartedState::Working
    } else {
        StartedState::Done
    }
}

pub fn started_icon(state: StartedState) -> &'static str {
    match state {
        StartedState::NeedsYou => "hand.raised",
        StartedState::Failed => "xmark",
        StartedState::Working => "circle.dashed",
        StartedState::Hibernating => "moon",
        StartedState::Done => "checkmark",
        StartedState::Archived => "archivebox",
    }
}

/// `ThreadRefs.status`: where a thread is and what it's doing.
pub fn thread_status<Tz: TimeZone>(t: &ThreadSummary, now: &DateTime<Tz>) -> String {
    let now_ms = now.timestamp_millis();
    if t.is_archived() {
        return "Archived".into();
    }
    if t.snoozed_until_release == Some(true) {
        return "Hibernating until the runner updates".into();
    }
    if t.snoozed_until_restart == Some(true) {
        return "Hibernating until the runner restarts".into();
    }
    if let (true, Some(until)) = (t.snoozed(now_ms), t.snoozed_until_ms()) {
        if let Some((words, _)) = sidebar::wake_row(t, now) {
            return format!("Hibernating: {words}");
        }
        if until > now_ms {
            return format!("Hibernating until {}", sidebar::until_words(until, now));
        }
    }
    match t.status.as_str() {
        "running" | "starting" => format!("Working {}", sidebar::working_label(t.activity_ms(), now_ms)),
        "waiting" => "Needs you".into(),
        "error" => "Failed".into(),
        "stopped" => "Stopped".into(),
        _ => {
            if t.background.as_ref().is_some_and(|b| !b.is_empty()) {
                return "In the background".into();
            }
            let seconds = (now_ms - t.activity_ms()) / 1000;
            let time = sidebar::relative_short(t.activity_ms(), now);
            if seconds < 60 {
                "Active just now".into()
            } else if seconds < 86_400 {
                format!("Active {time} ago")
            } else {
                format!("Active {time}")
            }
        }
    }
}

/// The row's second line (`StartedThreadRows.detail`).
pub fn started_detail<Tz: TimeZone>(row: &StartedRow, now: &DateTime<Tz>) -> String {
    let status = if row.state == StartedState::NeedsYou { "Needs you".to_owned() } else { thread_status(&row.thread, now) };
    if row.started { status } else { format!("Waiting on it · {status}") }
}

// ---------------------------------------------------------------- schedules

/// A message the runner sends at a set time (`Schedule`), as much as the
/// Scheduled group needs.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Schedule {
    #[serde(default, deserialize_with = "crate::wire::id")]
    pub id: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub title: String,
    /// What it sends.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub text: String,
    /// A one-off: when it runs.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub at: Option<String>,
    /// Or repeating: a five-field cron or @daily and friends.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub cron: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub timezone: Option<String>,
    /// The runner's words for `at` or `cron`: "Weekdays at 9:00".
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub when_text: String,
    /// Sends to this thread; None starts a new thread each time.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub thread_id: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub cwd: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub account_id: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub pool_id: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub model: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub effort: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub permission_mode: Option<String>,
    /// The thread whose agent made it; None for a person.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub created_by: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub paused: bool,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub finished_at: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub next_run_at: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub created_at: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub runs: i64,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub last_run: Option<LastRun>,
    /// The runner it runs on, its name and icon (docs/protocol.md, "Schedules").
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub runner_id: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub runner_name: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub runner_icon: Option<String>,
    /// A Run now asked of another runner, waiting for it to hear.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub run_requested_at: Option<String>,
}

/// A schedule's last run (`Schedule.LastRun`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct LastRun {
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub at: Option<String>,
    /// "sent", "skipped" or "failed"; anything else reads as sent.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub outcome: String,
    /// The thread it sent to or started.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub thread_id: Option<String>,
    /// Why it was skipped or failed.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub note: Option<String>,
}

impl Schedule {
    pub fn list(value: &Value) -> Vec<Schedule> {
        value.as_array().into_iter().flatten().filter_map(|s| crate::wire::read(s.clone())).collect()
    }
    pub fn is_finished(&self) -> bool {
        self.finished_at.is_some()
    }
    pub fn is_active(&self) -> bool {
        !self.paused && !self.is_finished()
    }
    pub fn is_one_off(&self) -> bool {
        self.cron.is_none()
    }
    /// It runs on another runner than this one (`runsElsewhere`); a schedule
    /// from before runners, or a list of runners not in yet, is this one's.
    pub fn runs_elsewhere(&self, self_runner_id: Option<&str>) -> bool {
        matches!((self.runner_id.as_deref(), self_runner_id), (Some(runner), Some(own)) if runner != own)
    }
    fn belongs(&self, thread_id: &str) -> bool {
        self.thread_id.as_deref() == Some(thread_id) || self.created_by.as_deref() == Some(thread_id)
    }
    pub fn next_ms(&self) -> Option<i64> {
        self.next_run_at.as_deref().and_then(parse_ms)
    }
    /// When another runner was asked to run it now, while it hasn't yet.
    pub fn run_requested_ms(&self) -> Option<i64> {
        self.run_requested_at.as_deref().and_then(parse_ms)
    }
}

/// A thread's row in the Scheduled group (`ScheduleRows.Row`).
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleRow {
    pub id: String,
    pub title: String,
    /// At the end of the title's line: "in 3h", "due now", "paused".
    pub when: Option<String>,
    pub detail: String,
    pub paused: bool,
}

/// `ScheduleTime.relative`: "in 12m", "in 3h", "in 2 days", or "now".
pub fn schedule_relative(date_ms: i64, now_ms: i64) -> String {
    let seconds = (date_ms - now_ms) as f64 / 1000.;
    if seconds <= 0. {
        return "now".into();
    }
    let minutes = (seconds / 60.).ceil() as i64;
    if minutes < 60 {
        return format!("in {minutes}m");
    }
    let hours = (seconds / 3600.).round() as i64;
    if hours < 24 {
        return format!("in {hours}h");
    }
    let days = (seconds / 86_400.).round() as i64;
    if days == 1 { "in 1 day".into() } else { format!("in {days} days") }
}

/// Active ones soonest first, then paused ones; a one-off that has run is left
/// out, unless it's the schedule that started the thread (`ScheduleRows.rows`).
pub fn schedule_rows(thread: &ThreadSummary, schedules: &[Schedule], now_ms: i64, target: impl Fn(&Schedule) -> String) -> Vec<ScheduleRow> {
    let origin = thread.schedule.as_ref().and_then(|s| s.get("id")).and_then(Value::as_str);
    let mut kept: Vec<&Schedule> = schedules.iter().filter(|s| s.belongs(&thread.id) || Some(s.id.as_str()) == origin).filter(|s| !s.is_finished() || Some(s.id.as_str()) == origin).collect();
    kept.sort_by(|a, b| {
        b.is_active().cmp(&a.is_active()).then_with(|| match (a.next_ms(), b.next_ms()) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.created_at.cmp(&b.created_at),
        })
    });
    kept.into_iter()
        .map(|s| {
            // "Started this thread" leads, so a long cron can't push it off
            // the end of the line.
            let is_origin = Some(s.id.as_str()) == origin;
            let mut parts: Vec<String> = if is_origin { vec!["Started this thread".to_owned()] } else { vec![] };
            if !s.when_text.is_empty() {
                parts.push(s.when_text.clone());
            }
            if !is_origin && s.thread_id.as_deref() != Some(thread.id.as_str()) {
                parts.push(target(s));
            }
            let when = if s.paused {
                Some("paused".to_owned())
            } else if s.is_finished() {
                None
            } else {
                s.next_ms().map(|ms| schedule_relative(ms, now_ms)).map(|w| if w == "now" { "due now".to_owned() } else { w })
            };
            ScheduleRow { id: s.id.clone(), title: if s.title.is_empty() { "Untitled".into() } else { s.title.clone() }, when, detail: parts.join(" · "), paused: s.paused }
        })
        .collect()
}

// ---------------------------------------------------------------- patches and panels

/// A panel a patch gives a thread (`Panel`), kept as far as the pane uses it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Panel {
    #[serde(default, deserialize_with = "crate::wire::id")]
    pub id: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub title: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub kind: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub icon: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub icon_file: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub patch: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub note: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub summary: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub badge: Option<i64>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub project_id: Option<String>,
    /// The built-in Convex panel's deployment: shown as Convex's embedded
    /// dashboard rather than `url` (docs/protocol.md, "The Convex panel").
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub convex: Option<crate::convex_panel::Deployment>,
}

impl Panel {
    /// Tolerant of a kind this app doesn't know: shown as a page if it has a
    /// url, the one thing every panel can be.
    pub fn from_value(v: &Value) -> Option<Panel> {
        let mut panel: Panel = crate::wire::read(v.clone())?;
        if panel.kind != "url" && panel.kind != "image" && panel.kind != "list" {
            panel.kind = "url".into();
        }
        Some(panel)
    }
}

/// A chip's action (`Action`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Action {
    #[serde(default, deserialize_with = "crate::wire::id")]
    pub id: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub title: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub kind: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub command: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub prompt: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub patch: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub note: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub folder: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub project_id: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub badge: Option<i64>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub icon: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub icon_file: Option<String>,
}

/// A patch that applies to a thread (`PatchInfo`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PatchInfo {
    #[serde(default, deserialize_with = "crate::wire::id")]
    pub id: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub error: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub folder: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub project_id: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::optional_list")]
    pub matched: Option<Vec<String>>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub live_error: Option<String>,
    /// A home patch runs something Threadbear can't follow, so threads that
    /// ask first ask before every file edit: what, where, and what to change.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub edits_ask: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub icon: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub icon_file: Option<String>,
    #[serde(default)]
    pub untrusted: Option<String>,
    #[serde(default)]
    pub hash: Option<String>,
    // The Trust prompt must show the complete set of helpers and grants.
    // An unreadable disclosure fails the patch row closed.
    #[serde(default)]
    pub helpers: Option<Vec<String>>,
    #[serde(default)]
    pub grants: Option<Vec<String>>,
}

impl PatchInfo {
    /// A Trust button needs complete, understood disclosures and an actual
    /// target. Unknown sources remain readable without offering to run code.
    pub fn can_trust(&self) -> bool {
        self.untrusted.is_some() && matches!(self.source.as_str(), "builtin" | "home" | "project" | "claude")
            && self.file.as_deref().is_some_and(|s| !s.is_empty())
            && self.hash.as_deref().is_some_and(|s| !s.is_empty())
    }
}

/// A hook (`HookInfo`); only its patch and title are shown ("gives").
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HookInfo {
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub patch: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub title: String,
}

/// One row of a `list` panel (`PanelItem`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct PanelItem {
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub title: String,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub detail: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub path: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub icon: Option<String>,
    /// "attention", "failed" or "done"; anything else is plain.
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub tone: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub section: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::field")]
    pub trailing: Option<String>,
}

impl PanelItem {
    pub fn tone_kind(&self) -> Option<&str> {
        self.tone.as_deref().filter(|t| matches!(*t, "attention" | "failed" | "done"))
    }

    /// The row's own icon, else one for its tone, else none (`ListPanel.icon`).
    pub fn symbol(&self) -> Option<&str> {
        if let Some(icon) = self.icon.as_deref().filter(|i| !i.is_empty()) {
            return Some(icon);
        }
        match self.tone_kind() {
            Some("attention") => Some("exclamationmark.circle"),
            Some("failed") => Some("xmark.circle"),
            Some("done") => Some("checkmark"),
            _ => None,
        }
    }

    /// The heading over each row: its section where it differs from the row
    /// before's, so a patch's groups come out in its own order (`headings`).
    pub fn headings(items: &[PanelItem]) -> Vec<Option<String>> {
        let mut last: Option<&str> = None;
        items
            .iter()
            .map(|item| {
                let heading = item.section.as_deref().filter(|s| !s.is_empty() && Some(*s) != last).map(str::to_owned);
                last = item.section.as_deref();
                heading
            })
            .collect()
    }
}

/// What `panels.render` answers (`PanelRender`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelRender {
    pub image: Option<crate::reducer::ImageRef>,
    pub items: Option<Vec<PanelItem>>,
    pub note: Option<String>,
}

impl PanelRender {
    pub fn decode(v: &Value) -> PanelRender {
        PanelRender {
            image: v.get("image").and_then(crate::reducer::ImageRef::decode),
            items: v.get("items").and_then(Value::as_array).map(|a| a.iter().filter_map(|i| crate::wire::read(i.clone())).collect()),
            note: v.get("note").and_then(Value::as_str).map(str::to_owned),
        }
    }
}

/// The Patches section's words (`PatchesList`).
pub mod patches {
    use super::*;

    pub fn source_name(source: &str) -> &'static str {
        match source {
            "builtin" => "built in",
            "home" => "~/.threadbear",
            "claude" => "Claude Code settings",
            _ => "project",
        }
    }

    /// The orange line on a waiting row. The folder's Claude Code settings say
    /// what was held back: the rest of them still apply.
    pub fn waiting_text(patch: &PatchInfo) -> &'static str {
        let changed = patch.untrusted.as_deref() == Some("changed");
        if patch.source == "claude" {
            if changed { "Changed since you trusted them, so they're skipped" } else { "Hooks, commands and environment settings skipped until you trust them" }
        } else if changed {
            "Changed since you trusted it"
        } else {
            "Not run yet"
        }
    }

    /// "actions Typecheck, Reseed · panel Storybook · hook Typecheck".
    pub fn gives(patch_id: &str, actions: &[Action], panels: &[Panel], hooks: &[HookInfo]) -> String {
        let mut parts = Vec::new();
        let titles: Vec<&str> = actions.iter().filter(|a| a.patch == patch_id).map(|a| a.title.as_str()).collect();
        if !titles.is_empty() {
            parts.push(format!("{} {}", if titles.len() == 1 { "action" } else { "actions" }, titles.join(", ")));
        }
        let titles: Vec<&str> = panels.iter().filter(|p| p.patch == patch_id).map(|p| p.title.as_str()).collect();
        if !titles.is_empty() {
            parts.push(format!("{} {}", if titles.len() == 1 { "panel" } else { "panels" }, titles.join(", ")));
        }
        let titles: Vec<&str> = hooks.iter().filter(|h| h.patch == patch_id).map(|h| h.title.as_str()).collect();
        if !titles.is_empty() {
            parts.push(format!("{} {}", if titles.len() == 1 { "hook" } else { "hooks" }, titles.join(", ")));
        }
        parts.join(" · ")
    }

    /// A group of patches under its project (`PatchGroup`).
    #[derive(Clone, Debug)]
    pub struct PatchGroup {
        pub project_id: Option<String>,
        pub title: Option<String>,
        pub patches: Vec<PatchInfo>,
    }

    /// One group with no title when nothing came from a project
    /// (`PatchGroups.group`). `project_title` is `projects.list`'s title for an
    /// id the thread isn't in any more.
    pub fn group(thread: &ThreadSummary, patches: &[PatchInfo], project_title: impl Fn(&str) -> Option<String>) -> Vec<PatchGroup> {
        let own: Vec<PatchInfo> = patches.iter().filter(|p| p.project_id.is_none()).cloned().collect();
        let from_projects: Vec<&PatchInfo> = patches.iter().filter(|p| p.project_id.is_some()).collect();
        if from_projects.is_empty() {
            return if own.is_empty() { vec![] } else { vec![PatchGroup { project_id: None, title: None, patches: own }] };
        }
        let mut groups = Vec::new();
        if !own.is_empty() {
            groups.push(PatchGroup { project_id: None, title: None, patches: own });
        }
        // The thread's projects in its order, then any it isn't in any more
        // (a summary a moment behind).
        let thread_projects = thread.projects.as_deref().unwrap_or(&[]);
        let mut ids: Vec<String> = thread_projects.iter().map(|p| p.id.clone()).collect();
        for p in &from_projects {
            let id = p.project_id.clone().unwrap_or_default();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        for id in ids {
            let list: Vec<PatchInfo> = from_projects.iter().filter(|p| p.project_id.as_deref() == Some(id.as_str())).map(|p| (*p).clone()).collect();
            if list.is_empty() {
                continue;
            }
            let title = thread_projects.iter().find(|p| p.id == id).map(|p| p.title.clone()).filter(|t| !t.is_empty()).or_else(|| project_title(&id)).unwrap_or_else(|| "A project".into());
            groups.push(PatchGroup { project_id: Some(id), title: Some(title), patches: list });
        }
        groups
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn chips_sit_on_a_shelf_for_each_place_they_came_from_grouped_by_patch() {
        let patch = |id: &str, extra: serde_json::Value| {
            let mut v = serde_json::json!({ "id": id, "source": "folder" });
            v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            serde_json::from_value::<PatchInfo>(v).unwrap()
        };
        let patches = vec![
            patch("app-store", serde_json::json!({})),
            patch("checks", serde_json::json!({})),
            patch("convex", serde_json::json!({ "projectId": "p1" })),
            patch("tandem", serde_json::json!({ "matched": ["/Users/mike/v/cut.tandem"] })),
            patch("mine", serde_json::json!({ "source": "home" })),
        ];
        let action = |id: &str, patch: &str| Action { id: id.into(), title: id.into(), kind: "url".into(), patch: patch.into(), ..Default::default() };
        let actions = vec![action("site", "app-store"), action("dash", "convex"), action("land", "checks"), action("sentry", "app-store"), action("cut", "tandem"), action("note", "mine")];
        let shelves = ActionShelf::shelves(&actions, &patches);
        let shape: Vec<(PanelOrigin, Vec<Vec<&str>>)> = shelves.iter().map(|s| (s.origin.clone(), s.patches.iter().map(|g| g.iter().map(|a| a.id.as_str()).collect()).collect())).collect();
        assert_eq!(
            shape,
            vec![
                (PanelOrigin::Folder, vec![vec!["site", "sentry"], vec!["land"]]),
                (PanelOrigin::Project("p1".into()), vec![vec!["dash"]]),
                (PanelOrigin::Tool(vec!["/Users/mike/v/cut.tandem".into()]), vec![vec!["cut"]]),
                (PanelOrigin::Home, vec![vec!["note"]]),
            ]
        );
        assert_eq!(PanelOrigin::Tool(vec!["/Users/mike/v/cut.tandem".into()]).tip("Cut", None), "Cut, for cut.tandem");
        assert_eq!(PanelOrigin::Project("p1".into()).tip("Convex", Some("Threadbear")), "Convex, from the Threadbear project");
        assert_eq!(action_glyph(&action("x", "app-store")), "arrow.up.right.square");
    }

    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "highlights": [
                {"id": "file:/w/site/mockups/hero.html", "kind": "file", "title": "hero.html", "path": "/w/site/mockups/hero.html", "how": "pinned", "note": "Does the headline work?", "show": true, "seq": 40, "lastSeq": 40, "at": "2026-10-27T08:00:00Z"},
                {"id": "file:/w/site/docs/copy.md", "kind": "file", "title": "copy.md", "path": "/w/site/docs/copy.md", "how": "written", "seq": 30, "lastSeq": 31, "at": "2026-10-27T07:00:00Z"},
                {"id": "pr:https://github.com/m/site/pull/14", "kind": "pr", "title": "m/site#14", "url": "https://github.com/m/site/pull/14", "how": "created", "seq": 28, "lastSeq": 28},
                {"id": "link:http://localhost:4321/", "kind": "link", "title": "localhost:4321", "url": "http://localhost:4321/", "how": "opened", "seq": 20, "lastSeq": 20},
                {"id": "commit:abc1234", "kind": "commit", "title": "Rebuild the hero", "sha": "abc1234", "branch": "rebuild", "pushed": true, "how": "committed", "seq": 18, "lastSeq": 18},
                {"id": "hologram:x", "kind": "hologram", "title": "From the future", "how": "beamed", "seq": 5, "lastSeq": 5},
                {"id": "hologram:y", "kind": "hologram", "title": "A future link", "url": "https://example.com", "how": "mentioned", "seq": 4, "lastSeq": 4, "extra": {"a": 1}},
                {"kind": "file", "title": "no id"}
            ],
            "edited": ["/w/site/src/Hero.tsx", "/w/site/docs/copy.md", "/w/site/src/Hero.tsx", "/w/site/src/app.css"],
            "brief": {"text": "  The three pages are rebuilt; Quick Ask's copy still needs a pass.  ", "at": "2026-10-27T08:00:00Z"},
            "somethingNew": true
        })
    }

    #[test]
    fn decodes_highlights_tolerantly() {
        let decoded = ThreadHighlights::decode(&sample());
        assert_eq!(decoded.highlights.len(), 7, "the one without an id is skipped, the rest kept");
        let pin = &decoded.highlights[0];
        assert_eq!(pin.kind, HighlightKind::File);
        assert!(pin.is_pinned());
        assert!(pin.show);
        assert_eq!(pin.note.as_deref(), Some("Does the headline work?"));
        assert_eq!(pin.at_ms, parse_ms("2026-10-27T08:00:00Z"));
        assert_eq!(decoded.highlights[4].sha.as_deref(), Some("abc1234"));
        assert!(decoded.highlights[4].pushed);
        assert!(!decoded.highlights[1].pushed);
        assert_eq!(decoded.highlights[5].kind, HighlightKind::Other);
        assert_eq!(decoded.highlights[5].how.word(), "");
        assert_eq!(decoded.edited.len(), 4);
        // An older runner's empty answer, or one with nothing at all.
        assert_eq!(ThreadHighlights::decode(&json!({})), ThreadHighlights::default());
    }

    #[test]
    fn sections_group_pins_files_links_and_commits() {
        let decoded = ThreadHighlights::decode(&sample());
        let sections = HighlightSections::build(&decoded);
        assert_eq!(sections.brief.as_deref(), Some("The three pages are rebuilt; Quick Ask's copy still needs a pass."));
        let titles = |list: &[Highlight]| list.iter().map(|h| h.title.clone()).collect::<Vec<_>>();
        assert_eq!(titles(&sections.pinned), ["hero.html"]);
        assert_eq!(titles(&sections.files), ["copy.md"]);
        // The unknown kind with a url is a link; the one with neither is left out.
        assert_eq!(titles(&sections.links), ["m/site#14", "localhost:4321", "A future link"]);
        assert_eq!(sections.commits[0].sha.as_deref(), Some("abc1234"));
        // A file already listed isn't repeated under edited, nor is a repeat.
        assert_eq!(sections.edited, ["/w/site/src/Hero.tsx", "/w/site/src/app.css"]);
        assert!(!sections.is_empty());
        assert!(HighlightSections::build(&ThreadHighlights::default()).is_empty());
        assert!(HighlightSections::build(&ThreadHighlights { brief: Some("  ".into()), ..Default::default() }).is_empty());
    }

    #[test]
    fn brief_points_split_the_bullets() {
        assert_eq!(
            HighlightSections::points("- Added the Tandem card\n\n  - Panel follows the thread \n• Waiting on Mike to test"),
            ["Added the Tandem card", "Panel follows the thread", "Waiting on Mike to test"]
        );
        // An older paragraph brief stays one point.
        assert_eq!(HighlightSections::points("Rebuilt the pages; copy needs a pass."), ["Rebuilt the pages; copy needs a pass."]);
    }

    #[test]
    fn web_pages_are_http_links_only() {
        assert!(is_web_page("http://localhost:4321/poddle"));
        assert!(is_web_page("HTTPS://github.com/m/site/pull/14"));
        assert!(!is_web_page("file:///tmp/a.html"));
        assert!(!is_web_page("mailto:mike@example.com"));
        assert_eq!(viewer_for_path("/a/b/Index.HTML"), Viewer::Web);
        assert_eq!(viewer_for_path("/a/b/shot.png"), Viewer::Picture);
        assert_eq!(viewer_for_path("/a/b/clip.mp4"), Viewer::Picture);
        assert_eq!(viewer_for_path("/a/b/paper.pdf"), Viewer::Picture);
    }

    #[test]
    fn text_files_get_the_text_view() {
        for path in ["/a/INSTALL.md", "/a/notes.MD", "/a/b.markdown", "/a/readme.txt", "/a/data.csv", "/a/.env", "/a/x.log", "/a/app.ts", "/a/Package.swift", "/a/config.json", "/a/Makefile"] {
            assert_eq!(viewer_for_path(path), Viewer::Text, "{path}");
        }
        assert!(is_markdown("/a/INSTALL.md"));
        assert!(!is_markdown("/a/notes.txt"));
    }

    #[test]
    fn code_files_get_their_language() {
        assert_eq!(code_language("/r/runner/test/unit/release.test.ts"), Some("typescript"));
        assert_eq!(code_language("/r/App.SWIFT"), Some("swift"));
        assert_eq!(code_language("/r/Makefile"), Some("makefile"));
        assert_eq!(code_language("/r/README.md"), None);
        assert_eq!(code_language("/r/clip.mp4"), None);
    }

    #[test]
    fn text_is_told_from_binary_by_its_bytes() {
        assert!(looks_like_text(b"import test from 'node:test';\n"));
        assert!(looks_like_text(b""));
        // "é" cut in half at the end of the sample.
        let mut cut = b"caf".to_vec();
        cut.push(0xC3);
        assert!(looks_like_text(&cut));
        // An MPEG transport stream packet: sync byte, then binary.
        assert!(!looks_like_text(&[0x47, 0x40, 0x00, 0x10, 0x00]));
        assert!(!looks_like_text(&[0xFF, 0xFE, 0xFD, 0x41]));
    }

    #[test]
    fn transcript_links_to_review_files_preview_in_the_pane() {
        for path in ["/tmp/a.md", "/tmp/b.HTML", "/tmp/c.pdf", "/tmp/d.png", "/tmp/e.mov", "/tmp/f.csv", "/tmp/g.svg"] {
            assert!(previews_in_pane(path), "{path}");
        }
        for path in ["/tmp/a.swift", "/tmp/b.ts", "/tmp/folder", "/tmp/c.zip"] {
            assert!(!previews_in_pane(path), "{path}");
        }
    }

    /// A picture only opens in the viewer if it's named as one and its default
    /// app wouldn't run it (`quickLooks`, `LinkPolicy.runsCode`).
    #[test]
    fn only_pictures_reach_the_viewer() {
        assert!(looks_like_picture("/a/shot.PNG"));
        assert!(looks_like_picture("/a/photo.heic"));
        assert!(!looks_like_picture("/a/run.command"));
        assert!(!looks_like_picture("/a/notes.md"));
        assert!(runs_code("/a/run.command"));
        assert!(runs_code("/a/Tool.APP"));
        assert!(!runs_code("/a/shot.png"));
    }

    #[test]
    fn tandem_projects_are_app_projects() {
        assert!(is_app_project("/a/Pebble Index mic test.tandem"));
        assert!(is_app_project("/a/B.TANDEM"));
        // Tandem's hidden folder beside a project isn't one.
        assert!(!is_app_project("/a/.tandem"));
        assert!(!is_app_project("/a/notes.md"));
    }

    #[test]
    fn file_icons_by_extension() {
        assert_eq!(file_symbol("/a/notes.md"), "doc.text");
        assert_eq!(file_symbol("/a/mock.html"), "safari");
        assert_eq!(file_symbol("/a/shot.PNG"), "photo");
        assert_eq!(file_symbol("/a/demo.mov"), "film");
        assert_eq!(file_symbol("/a/data.csv"), "tablecells");
        assert_eq!(file_symbol("/a/App.swift"), "chevron.left.forwardslash.chevron.right");
        assert_eq!(file_symbol("/a/Makefile"), "doc");
        let pr = Highlight { id: "pr:1".into(), kind: HighlightKind::Pr, title: "m/s#1".into(), ..Default::default() };
        assert_eq!(highlight_symbol(&pr), "arrow.triangle.pull");
        let link = Highlight { id: "link:1".into(), kind: HighlightKind::Link, title: "x".into(), ..Default::default() };
        assert_eq!(highlight_symbol(&link), "link");
    }

    #[test]
    fn a_windows_path_sits_in_its_folder_the_same_way() {
        assert_eq!(text::folder(r"C:\work\site\mockups\home.html", Some(r"C:\work\site"), r"C:\Users\mike"), "mockups");
        assert_eq!(last_component(r"C:\work\site\mockups\home.html"), "home.html");
    }

    #[test]
    fn folder_is_relative_to_the_threads_folder_else_home() {
        let cwd = Some("/Users/mike/dev/site");
        assert_eq!(text::folder("/Users/mike/dev/site/docs/mockups/a.html", cwd, "/Users/mike"), "docs/mockups");
        assert_eq!(text::folder("/Users/mike/dev/site/README.md", Some("/Users/mike/dev/site/"), "/Users/mike"), "site");
        assert_eq!(text::folder("/Users/mike/dev/site-old/a.md", cwd, "/Users/mike"), "~/dev/site-old");
        assert_eq!(text::folder("/tmp/a.md", cwd, "/Users/mike"), "/tmp");
        assert_eq!(text::address("https://www.github.com/m/site/pull/14/"), "github.com/m/site/pull/14");
        let commit = |branch: Option<&str>, pushed: bool| Highlight { id: "c".into(), kind: HighlightKind::Commit, title: "x".into(), branch: branch.map(str::to_owned), pushed, ..Default::default() };
        assert_eq!(text::commit_line(&commit(Some("main"), true)), "main · pushed");
        assert_eq!(text::commit_line(&commit(None, false)), "not pushed yet");
        // Agents often commit without the branch showing: no empty " · ".
        assert_eq!(text::commit_line(&commit(Some(""), true)), "pushed");
        assert_eq!(text::edited_count(1), "1 file edited");
        assert_eq!(text::edited_count(3), "3 files edited");
    }

    #[test]
    fn long_groups_are_capped_until_show_all() {
        let commits: Vec<i32> = (1..=15).collect();
        let (shown, more) = HighlightSections::capped(&commits, COMMIT_LIMIT, false);
        assert_eq!(shown, [1, 2, 3, 4, 5]);
        assert_eq!(more, 10);
        assert_eq!(HighlightSections::capped(&commits, 5, true).0.len(), 15);
        assert_eq!(HighlightSections::capped(&(1..=5).collect::<Vec<i32>>(), 5, false).1, 0);
    }

    #[test]
    fn list_panel_headings_change_where_the_section_does() {
        let item = |section: Option<&str>| PanelItem { title: "x".into(), section: section.map(str::to_owned), ..Default::default() };
        let items = [item(Some("This branch")), item(Some("This branch")), item(Some("Elsewhere")), item(None)];
        assert_eq!(PanelItem::headings(&items), [Some("This branch".to_owned()), None, Some("Elsewhere".to_owned()), None]);
    }

    #[test]
    fn panel_render_decodes_items_skipping_broken_ones() {
        let render = PanelRender::decode(&json!({"items": [
            {"title": "main", "detail": "/w/site abc1234 [main]", "icon": "arrow.triangle.branch"},
            {"title": "Preview", "url": "http://localhost:4321", "tone": "attention"},
            {"title": "Build", "tone": "failed", "path": "/w/site/build.log"},
            {"title": "Deploy", "tone": "done"},
            {"title": "Odd", "tone": "sparkly"},
            {"detail": "no title"}
        ], "note": null}));
        let items = render.items.expect("items");
        assert_eq!(items.len(), 6, "a row with no title has one that's empty");
        assert_eq!(items[0].symbol(), Some("arrow.triangle.branch"));
        assert_eq!(items[1].symbol(), Some("exclamationmark.circle"));
        assert_eq!(items[2].symbol(), Some("xmark.circle"));
        assert_eq!(items[3].symbol(), Some("checkmark"));
        assert_eq!(items[4].symbol(), None, "a tone this app doesn't know is plain");
        assert_eq!(items[4].tone_kind(), None);
        assert!(render.note.is_none());
    }

    #[test]
    fn panels_with_unknown_kinds_stay_pages() {
        let panel = Panel::from_value(&json!({"id": "future.thing", "title": "Later", "kind": "hologram", "url": "https://example.com", "patch": "future"})).unwrap();
        assert_eq!(panel.kind, "url");
        let list = Panel::from_value(&json!({"id": "tools.worktrees", "title": "Worktrees", "kind": "list", "icon": "arrow.triangle.branch", "patch": "tools"})).unwrap();
        assert_eq!(list.kind, "list");
        assert_eq!(panel_symbol(&list), "arrow.triangle.branch");
        assert_eq!(panel_symbol(&panel), "globe");
    }

    #[test]
    fn patches_say_what_they_give() {
        let actions = vec![Action { title: "Typecheck".into(), patch: "tools".into(), ..Default::default() }, Action { title: "Reseed".into(), patch: "tools".into(), ..Default::default() }];
        let panels = vec![Panel { title: "Storybook".into(), patch: "tools".into(), ..Default::default() }];
        let hooks = vec![HookInfo { patch: "tools".into(), title: "Typecheck".into() }];
        assert_eq!(patches::gives("tools", &actions, &panels, &hooks), "actions Typecheck, Reseed · panel Storybook · hook Typecheck");
        assert_eq!(patches::gives("other", &actions, &panels, &hooks), "");
        assert_eq!(patches::source_name("builtin"), "built in");
        assert_eq!(patches::source_name("home"), "~/.threadbear");
        assert_eq!(patches::source_name("project"), "project");
        let waiting = PatchInfo { source: "project".into(), untrusted: Some("new".into()), ..Default::default() };
        assert_eq!(patches::waiting_text(&waiting), "Not run yet");
        let changed = PatchInfo { source: "claude".into(), untrusted: Some("changed".into()), ..Default::default() };
        assert_eq!(patches::waiting_text(&changed), "Changed since you trusted them, so they're skipped");
    }

    #[test]
    fn patch_groups_put_own_folder_first_then_projects_in_order() {
        let thread = ThreadSummary { id: "t".into(), projects: Some(vec![Project { id: "p2".into(), title: "Walkies".into(), ..Default::default() }, Project { id: "p1".into(), title: "Site".into(), ..Default::default() }]), ..Default::default() };
        let patch = |id: &str, project: Option<&str>| PatchInfo { id: id.into(), project_id: project.map(str::to_owned), ..Default::default() };
        let list = vec![patch("own", None), patch("a", Some("p1")), patch("b", Some("p2")), patch("gone", Some("p9"))];
        let groups = patches::group(&thread, &list, |id| (id == "p9").then(|| "Old project".to_owned()));
        assert_eq!(groups.iter().map(|g| g.title.clone()).collect::<Vec<_>>(), [None, Some("Walkies".into()), Some("Site".into()), Some("Old project".into())]);
        assert_eq!(patches::group(&thread, &[patch("own", None)], |_| None).len(), 1, "one group, no title, when nothing came from a project");
        assert!(patches::group(&thread, &[], |_| None).is_empty());
    }

    #[test]
    fn project_words_say_who_and_whether_it_can_be_kept() {
        let m = |by: &str, pinned: bool| Project { id: "p".into(), title: "x".into(), by: by.into(), pinned, why: None };
        assert_eq!(project_why(&m("mike", true)).0, "You put it here.");
        assert_eq!(project_why(&m("agent", true)).0, "Its agent put it here.");
        assert_eq!(project_why(&m("librarian", true)).0, "The librarian put it here, and you kept it.");
        assert_eq!(project_why(&m("librarian", false)).0, "The librarian put it here.");
        assert!(can_keep(&m("librarian", false)));
        assert!(!can_keep(&m("librarian", true)));
        assert!(!can_keep(&m("mike", false)));
    }

    #[test]
    fn project_rows_skip_archived_and_unknown_and_repeats() {
        let thread = ThreadSummary {
            id: "t".into(),
            projects: Some(vec![Project { id: "a".into(), ..Default::default() }, Project { id: "b".into(), ..Default::default() }, Project { id: "a".into(), ..Default::default() }, Project { id: "c".into(), ..Default::default() }]),
            ..Default::default()
        };
        let rows = project_rows(&thread, |id| match id {
            "a" => Some(ProjectInfo { id: "a".into(), title: "A".into(), ..Default::default() }),
            "b" => Some(ProjectInfo { id: "b".into(), archived: true, ..Default::default() }),
            _ => None,
        });
        assert_eq!(rows.iter().map(|r| r.project.id.clone()).collect::<Vec<_>>(), ["a"]);
    }

    #[test]
    fn open_tasks_are_counted_from_the_columns() {
        let project = ProjectInfo {
            tasks: Some(json!({"columns": [{"name": "Doing", "kind": "progress"}, {"name": "Done", "kind": "done"}], "items": [{"status": "Doing"}, {"status": "Done"}, {"status": "Doing"}]})),
            ..Default::default()
        };
        assert_eq!(project.open_task_count(), 2);
        let loading = ProjectInfo { tasks: Some(json!({"loading": true, "items": [{"status": "Doing"}]})), ..Default::default() };
        assert_eq!(loading.open_task_count(), 0);
    }

    fn summary(id: &str, status: &str) -> ThreadSummary {
        ThreadSummary { id: id.into(), title: id.into(), status: status.into(), last_activity_at: "2026-11-09T10:00:00Z".into(), ..Default::default() }
    }

    #[test]
    fn started_threads_list_what_needs_mike_first() {
        let mut parent = summary("parent", "idle");
        parent.waiting_on = Some(vec![json!({"threadId": "waited", "title": "waited"})]);
        let by = |t: &mut ThreadSummary| t.started_by = Some(json!({"kind": "thread", "threadId": "parent"}));
        let mut failed = summary("failed", "error");
        by(&mut failed);
        let mut working = summary("working", "running");
        by(&mut working);
        let mut asking = summary("asking", "waiting");
        by(&mut asking);
        let waited = summary("waited", "idle");
        let stranger = summary("stranger", "idle");
        let threads = [parent.clone(), failed, working, asking, waited, stranger];
        let now = parse_ms("2026-11-09T11:00:00Z").unwrap();
        let rows = started_rows(&parent, threads.iter(), now);
        assert_eq!(rows.iter().map(|r| r.thread.id.as_str()).collect::<Vec<_>>(), ["asking", "failed", "working", "waited"]);
        assert_eq!(rows[0].state, StartedState::NeedsYou);
        assert_eq!(started_icon(rows[1].state), "xmark");
        assert_eq!(started_icon(rows[3].state), "checkmark");
        let clock = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now).unwrap();
        assert_eq!(started_detail(&rows[0], &clock), "Needs you");
        // One it waits on but didn't start says so.
        assert!(started_detail(&rows[3], &clock).starts_with("Waiting on it · "));
    }

    #[test]
    fn schedule_rows_list_active_first_and_say_where() {
        let now = parse_ms("2026-11-09T10:00:00Z").unwrap();
        let schedule = |id: &str, next: Option<&str>, paused: bool, thread: Option<&str>| Schedule {
            id: id.into(),
            title: id.into(),
            when_text: "Weekdays at 9:00".into(),
            thread_id: thread.map(str::to_owned),
            paused,
            next_run_at: next.map(str::to_owned),
            ..Default::default()
        };
        let thread = ThreadSummary { id: "t".into(), schedule: Some(json!({"id": "started-it"})), ..Default::default() };
        let mut finished = schedule("done-once", None, false, Some("t"));
        finished.finished_at = Some("2026-11-08T10:00:00Z".into());
        let mut origin = schedule("started-it", None, false, None);
        origin.finished_at = Some("2026-11-08T10:00:00Z".into());
        origin.cwd = Some("/x/misc".into());
        let list = [
            schedule("later", Some("2026-11-09T13:00:00Z"), false, Some("t")),
            schedule("soon", Some("2026-11-09T10:12:00Z"), false, Some("t")),
            schedule("paused", Some("2026-11-09T10:01:00Z"), true, Some("t")),
            schedule("someone-elses", Some("2026-11-09T10:01:00Z"), false, Some("other")),
            finished,
            origin,
        ];
        let rows = schedule_rows(&thread, &list, now, |_| "New thread in misc".into());
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["soon", "later", "paused", "started-it"]);
        assert_eq!(rows[0].when.as_deref(), Some("in 12m"));
        assert_eq!(rows[1].when.as_deref(), Some("in 3h"));
        assert_eq!(rows[2].when.as_deref(), Some("paused"));
        assert_eq!(rows[3].when, None);
        assert_eq!(rows[3].detail, "Started this thread · Weekdays at 9:00");
        assert_eq!(schedule_relative(now, now), "now");
        assert_eq!(schedule_relative(now + 36 * 3_600_000, now), "in 2 days");
    }
}
